//! StreamAssets reads streamed segments off the window and device threads.
//! A segment stays Xbox ADPCM; the device decodes it a block at a time as it
//! plays (audio::voices::Sample::adpcm). The shared audio runtime still
//! chooses every segment, generation, channel layout and transition.
use audio_control::streams::Request;
use std::{
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::mpsc::{sync_channel, Receiver, SyncSender},
};
pub struct Segment {
    pub file: PathBuf,
    pub offset: u64,
    pub length: usize,
    pub rate: f64,
    pub channels: usize,
    pub substreams: usize,
    pub interleave: usize,
}
pub struct Pcm {
    pub rate: f64,
    pub channels: usize,
    pub frames: usize,
    pub sample: audio::voices::Sample,
}
pub struct Response {
    pub request: Request,
    pub pcm: Option<Pcm>,
}
pub struct Assets {
    send: Option<SyncSender<Request>>,
    receive: Receiver<Response>,
    thread: Option<std::thread::JoinHandle<()>>,
}
/// An upscaled segment the enhanced mode plays in place of the original
/// (tools/enhance_audio.py's Opus file for a layout) and its playback gain.
pub struct Variant {
    pub file: PathBuf,
    pub gain_db: f64,
}
impl Assets {
    pub fn new(segments: Vec<Segment>, variants: Vec<Variant>) -> Self {
        let (send, jobs) = sync_channel::<Request>(128);
        let (results, receive) = sync_channel(128);
        let thread = std::thread::spawn(move || {
            while let Ok(request) = jobs.recv() {
                let pcm = match variants.get(request.variant) {
                    Some(variant) => read_variant(variant).map_err(|err| eprintln!("enhanced audio: {} ({err})", variant.file.display())).ok(),
                    None => segments.get(request.segment).and_then(|segment| read(segment).ok().flatten()),
                };
                if results.send(Response { request, pcm }).is_err() {
                    break;
                }
            }
        });
        Self { send: Some(send), receive, thread: Some(thread) }
    }
    pub fn request(&self, request: Request) -> bool {
        self.send.as_ref().is_some_and(|sender| sender.try_send(request).is_ok())
    }
    pub fn receive(&self) -> Option<Response> {
        self.receive.try_recv().ok()
    }
}
impl Drop for Assets {
    fn drop(&mut self) {
        self.send.take();
        while self.receive.recv().is_ok() {}
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn read(segment: &Segment) -> Result<Option<Pcm>, std::io::Error> {
    // The whole authored segment, including padding to a substream chunk,
    // split into its substreams the way the browser decoder reads them.
    let mut file = std::fs::File::open(&segment.file)?;
    file.seek(SeekFrom::Start(segment.offset))?;
    let mut bytes = vec![0; segment.length];
    file.read_exact(&mut bytes)?;
    let count = segment.substreams.max(1);
    let parts = if count == 1 {
        vec![bytes]
    } else {
        if segment.interleave == 0 {
            return Ok(None);
        }
        let mut parts = vec![Vec::with_capacity(bytes.len() / count + segment.interleave); count];
        for (chunk, run) in bytes.chunks_exact(segment.interleave).enumerate() {
            parts[chunk % count].extend_from_slice(run);
        }
        parts
    };
    Ok(audio::voices::Sample::adpcm(segment.rate, segment.channels, parts)
        .map(|sample| Pcm { rate: segment.rate, channels: sample.channels, frames: sample.frames, sample }))
}

// The Opus decoder feeding an upscaled segment's ring as it plays, given
// its compressed bytes in reads of 64 KiB (the decoder's input is bounded).
struct Opus {
    decoder: audio_decode::Decoder,
    channels: usize,
    bytes: Vec<u8>,
    at: usize,
    failed: bool,
}
impl Opus {
    // The next step that is not a request for input.
    fn step(&mut self, frames: usize) -> Result<audio_decode::Step, String> {
        loop {
            match self.decoder.step(frames)? {
                audio_decode::Step::NeedInput => {
                    if self.at >= self.bytes.len() {
                        return Err("truncated upscaled segment".into());
                    }
                    let end = (self.at + 65536).min(self.bytes.len());
                    self.decoder.push(&self.bytes[self.at..end], end == self.bytes.len())?;
                    self.at = end;
                }
                step => return Ok(step),
            }
        }
    }
}
impl audio::streaming::Producer for Opus {
    fn produce(&mut self, frames: usize) -> Option<(&[f32], usize)> {
        loop {
            match self.step(frames) {
                Ok(audio_decode::Step::Output(n)) => return Some((&self.decoder.output[..n * self.channels], n)),
                Ok(audio_decode::Step::Metadata) => continue,
                Ok(_) => return None,
                Err(err) => {
                    if !self.failed {
                        self.failed = true;
                        eprintln!("enhanced audio: an upscaled segment stopped ({err})");
                    }
                    return None;
                }
            }
        }
    }
}
/// An upscaled segment, kept compressed and decoded as it plays.
fn read_variant(variant: &Variant) -> Result<Pcm, String> {
    let bytes = std::fs::read(&variant.file).map_err(|e| e.to_string())?;
    let tail = bytes[bytes.len().saturating_sub(65536)..].to_vec();
    let mut opus = Opus { decoder: audio_decode::Decoder::new(0, variant.gain_db, false), channels: 0, bytes, at: 0, failed: false };
    match opus.step(2048)? {
        audio_decode::Step::Metadata => {}
        _ => return Err("no audio metadata".into()),
    }
    if opus.decoder.info.as_ref().is_some_and(|info| info.frames.is_none()) {
        opus.decoder.tail(&tail)?;
    }
    let info = opus.decoder.info.clone().ok_or("no audio metadata")?;
    let (rate, channels, frames) = (info.rate as f64, info.channels, info.frames.unwrap_or(0) as usize);
    opus.channels = channels;
    let sample = audio::voices::Sample::codec(rate, channels, frames, Box::new(opus)).ok_or("unsupported upscaled segment")?;
    static SHOWN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    if SHOWN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 4 {
        eprintln!("enhanced audio: streaming {} ({channels} ch, {frames} frames)", variant.file.display());
    }
    Ok(Pcm { rate, channels, frames: sample.frames, sample })
}
