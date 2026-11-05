//! Bounded asset decoding, run on a loading worker. The decoder owns its
//! compressed parser, PCM windows and explicit sample-rate conversion.
//! Audio render threads receive only prepared PCM and never call a codec.
mod abi;
mod flac;
mod input;
mod layout;
mod ogg;
mod resample;
mod wav;

// Metadata separates codec PCM rate from delivered PCM rate. All scheduling
// uses the latter and its converted frame count; source_rate is retained for
// asset reporting and the explicit impulse-response compensation factor.
#[derive(Clone, Debug)]
pub struct Metadata {
    pub source_rate: u32,
    pub rate: u32,
    pub channels: usize,
    pub bits: u32,
    pub frames: Option<u64>,
    pub format: u32,
}
// A window owns only its current channel-major samples. Codec output can
// move into rate conversion without keeping a decoded copy of the track.
pub struct Pcm {
    pub data: Vec<f32>,
    pub channels: usize,
    pub frames: usize,
}
enum Format {
    Unknown,
    Flac(flac::Flac),
    Wav(wav::Wav),
    Ogg(ogg::Ogg),
}
// The worker yields between steps so playback jobs can run ahead of bulk
// effect preloads. NeedInput hands control back to asynchronous asset I/O;
// no codec or network operation executes on the render thread.
pub enum Step {
    NeedInput,
    Metadata,
    Output(usize),
    Done,
}
pub struct Decoder {
    input: input::Input,
    format: Format,
    target: u32,
    gain: f64,
    compensate: bool,
    resampler: Option<resample::Resampler>,
    pub info: Option<Metadata>,
    pub output: Vec<f32>,
    source_done: bool,
}
impl Decoder {
    /// A target of zero preserves the codec's native rate. A device rate
    /// is an explicit conversion request, not an implicit decoder contract.
    pub fn new(target: u32, gain_db: f64, compensate: bool) -> Self {
        Self {
            input: input::Input::default(),
            format: Format::Unknown,
            target,
            gain: v8math::pow_legacy(10.0, gain_db / 20.0),
            compensate,
            resampler: None,
            info: None,
            output: Vec::new(),
            source_done: false,
        }
    }
    // Feeding bytes is separate from decoding. This preserves parser state
    // across range reads and makes cancellation independent of codec frame size.
    pub fn push(&mut self, bytes: &[u8], eof: bool) -> Result<(), String> {
        self.input.push(bytes, eof)
    }
    // This replaces the decodeAudioData asset pipeline: identify the container,
    // decode source PCM, then perform an explicit rate/gain conversion. The output
    // has the same rate/length contract as the former AudioBuffer, without a host
    // codec or an implicit context-rate decision.
    pub fn step(&mut self, maximum: usize) -> Result<Step, String> {
        // Inspect signatures rather than extensions: optional HRIR WAVs and tools'
        // Ogg assets can enter through the same loading API.
        if matches!(self.format, Format::Unknown) {
            if self.input.bytes().len() < 4 {
                self.input.short()?;
                return Ok(Step::NeedInput);
            }
            self.format = match &self.input.bytes()[..4] {
                b"fLaC" => Format::Flac(flac::Flac::new()),
                b"OggS" => Format::Ogg(ogg::Ogg::new()),
                b"RIFF" => Format::Wav(wav::Wav::new()),
                _ => return Err("unsupported audio asset signature".into()),
            };
        }
        loop {
            // Drain available converted PCM before decoding another frame. This bounds
            // resident decoded input to the filter history and a small codec window,
            // while batching short Opus packets into useful transfer sizes.
            if let Some(resampler) = &mut self.resampler {
                let frames = if self.source_done || resampler.ready(maximum) {
                    resampler.process(maximum, &mut self.output)
                } else {
                    0
                };
                if frames > 0 {
                    return Ok(Step::Output(frames));
                }
                if resampler.finished() {
                    return Ok(Step::Done);
                }
                // A converter that cannot reach the promised frame count at EOF indicates
                // a malformed asset. Do not manufacture a shorter duration after scheduling.
                if self.source_done {
                    return Err("audio ended before the expected output frame count".into());
                }
            }
            // Container parsers own source counts. They may publish metadata alongside
            // the first PCM frame; retain that frame before returning Metadata to JS.
            let (pcm, info, finished) = match &mut self.format {
                Format::Flac(f) => {
                    let pcm = f.next(&mut self.input)?;
                    (pcm, f.info.clone(), f.finished(&self.input))
                }
                Format::Ogg(f) => {
                    let pcm = f.next(&mut self.input)?;
                    (pcm, f.info.clone(), f.finished())
                }
                Format::Wav(f) => {
                    let pcm = f.next(&mut self.input)?;
                    (pcm, f.info.clone(), f.finished())
                }
                _ => unreachable!(),
            };
            let fresh = self.info.is_none() && info.is_some();
            if fresh {
                let mut info = info.unwrap();
                let target = if self.target == 0 {
                    info.source_rate
                } else {
                    self.target
                };
                if !(4000..=384000).contains(&target) {
                    return Err("unsupported output PCM rate".into());
                }
                // The former HRIR loader multiplied samples by sourceRate/deviceRate after
                // context conversion. Keep that impulse-area correction explicit here. Music
                // and effects leave it disabled; they retain ordinary amplitude scaling.
                let gain = self.gain
                    * if self.compensate {
                        info.source_rate as f64 / target as f64
                    } else {
                        1.0
                    };
                // Construct conversion exactly once, from source metadata rather than
                // from the first transfer's length. Opus priming has already been removed;
                // FLAC/WAV counts still refer to their original lossless sample timeline.
                self.resampler = Some(resample::Resampler::new(
                    info.source_rate,
                    target,
                    info.channels,
                    info.frames,
                    gain,
                ));
                info.rate = target;
                // Use truncation after division in f64, as AudioBuffer rate conversion did.
                // Length controls stream joins, loop offsets and reported duration. Rounding
                // up here would schedule an extra sample and change subsequent segment starts.
                info.frames = info
                    .frames
                    .map(|n| (n as f64 / (info.source_rate as f64 / target as f64)) as u64);
                self.info = Some(info);
            }
            let produced = pcm.is_some();
            if let Some(pcm) = pcm {
                self.resampler
                    .as_mut()
                    .ok_or("audio frame precedes metadata")?
                    .push(pcm);
            }
            // Signal EOF to the sinc window only after the parser is complete. Its
            // trailing half-kernel may then read zeros up to the exact output length.
            if finished {
                self.source_done = true;
                self.resampler
                    .as_mut()
                    .ok_or("audio asset lacks metadata")?
                    .finish();
            }
            if fresh {
                return Ok(Step::Metadata);
            }
            // No progress without completion means this parser needs another range.
            // Do not spin inside a synchronous worker phase; yielding lets ready playback
            // jobs consume their credits before background asset loading resumes.
            if !produced && !finished {
                return Ok(Step::NeedInput);
            }
        }
    }
}
impl Decoder {
    // Opus exposes duration through its final granule rather than OpusHead.
    // Convert that source count with the same arithmetic as ordinary metadata,
    // so early duration discovery and final decoding agree at every device rate.
    pub fn tail(&mut self, bytes: &[u8]) -> Result<(), String> {
        let Format::Ogg(ogg) = &self.format else {
            return Ok(());
        };
        let frames = ogg.tail_frames(bytes)?;
        let info = self.info.as_mut().ok_or("tail precedes metadata")?;
        let total = (frames as f64 / (info.source_rate as f64 / info.rate as f64)) as u64;
        info.frames = Some(total);
        self.resampler.as_mut().unwrap().set_total(total);
        Ok(())
    }
}
