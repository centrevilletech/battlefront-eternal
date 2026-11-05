//! RIFF PCM and IEEE float, including the sixteen discrete SADIE lanes.
//! Unknown metadata chunks are skipped incrementally, never buffered whole.
use crate::{input::Input, Metadata, Pcm};
// WAV is used for optional SADIE filters as well as ordinary PCM fixtures.
// Its lanes remain discrete: a sixteen-channel impulse file is not a speaker
// layout and must not be downmixed by a browser decoder.
pub struct Wav {
    header: bool,
    skip: usize,
    // The format tuple is encoding, source rate, lanes, bits and frame stride.
    // remaining counts data bytes; frames counts complete decoded sample frames.
    fmt: Option<(u16, u32, usize, u32, usize)>,
    remaining: Option<u64>,
    pub info: Option<Metadata>,
    pub frames: u64,
}
impl Wav {
    pub fn new() -> Self {
        Self {
            header: false,
            skip: 0,
            fmt: None,
            remaining: None,
            info: None,
            frames: 0,
        }
    }
    // Parse incrementally and emit at most 4096 source frames. Allocations
    // belong to the loading worker. The audio thread sees only the resulting
    // bounded PCM transfers, never RIFF chunk headers or sample formats.
    pub fn next(&mut self, input: &mut Input) -> Result<Option<Pcm>, String> {
        if !self.header {
            if input.bytes().len() < 12 {
                input.short()?;
                return Ok(None);
            }
            if &input.bytes()[..4] != b"RIFF" || &input.bytes()[8..12] != b"WAVE" {
                return Err("unsupported WAV container".into());
            }
            input.consume(12);
            self.header = true;
        }
        // Find fmt before data, skipping unrelated chunks without buffering them.
        // RIFF pads odd payload lengths to an even boundary; the pad is not audio.
        while self.remaining.is_none() {
            if self.skip > 0 {
                let n = self.skip.min(input.bytes().len());
                input.consume(n);
                self.skip -= n;
                if self.skip > 0 {
                    input.short()?;
                    return Ok(None);
                }
            }
            let b = input.bytes();
            if b.len() < 8 {
                input.short()?;
                return Ok(None);
            }
            let length = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
            if &b[..4] == b"fmt " {
                if !(16..=1024).contains(&length) {
                    return Err("invalid WAV format chunk".into());
                }
                if b.len() < 8 + length {
                    input.short()?;
                    return Ok(None);
                }
                let f = &b[8..8 + length];
                let mut kind = u16::from_le_bytes(f[..2].try_into().unwrap());
                let channels = u16::from_le_bytes(f[2..4].try_into().unwrap()) as usize;
                let rate = u32::from_le_bytes(f[4..8].try_into().unwrap());
                let align = u16::from_le_bytes(f[12..14].try_into().unwrap()) as usize;
                let bits = u16::from_le_bytes(f[14..16].try_into().unwrap()) as u32;
                // WAVE_FORMAT_EXTENSIBLE carries its actual encoding in the subtype.
                // Channel-mask speaker interpretation is deliberately absent: HRIR lane
                // numbers name harmonic components, whose order must stay unchanged.
                if kind == 65534 {
                    if length < 40 {
                        return Err("short extensible WAV format".into());
                    }
                    kind = u16::from_le_bytes(f[24..26].try_into().unwrap());
                }
                // Validate the exact packed frame stride before indexing sample bytes.
                // Compressed WAV and padded sample containers do not use this PCM reader.
                if channels == 0
                    || channels > 32
                    || rate == 0
                    || !matches!((kind, bits), (1, 8 | 16 | 24 | 32) | (3, 32 | 64))
                    || align != channels * (bits as usize / 8)
                {
                    return Err("unsupported WAV sample format".into());
                }
                self.fmt = Some((kind, rate, channels, bits, align));
                input.consume(8 + length);
                self.skip = length & 1;
            } else if &b[..4] == b"data" {
                let (_, rate, channels, bits, align) =
                    self.fmt.ok_or("WAV data precedes its format")?;
                // A data chunk must end on a complete multichannel frame. Reject a trailing
                // partial frame rather than deriving a different duration from rounded bytes.
                if length % align != 0 {
                    return Err("partial WAV sample frame".into());
                }
                self.remaining = Some(length as u64);
                self.info = Some(Metadata {
                    source_rate: rate,
                    rate,
                    channels,
                    bits,
                    frames: Some((length / align) as u64),
                    format: crate::layout::WAV as u32,
                });
                input.consume(8);
            // Metadata can be larger than the input budget. Retain only its byte
            // count and optional pad, then discard payload across successive reads;
            // never search for data signatures inside unknown chunk contents.
            } else {
                input.consume(8);
                self.skip = length + (length & 1);
            }
        }
        let remaining = self.remaining.unwrap();
        if remaining == 0 {
            return Ok(None);
        }
        let (kind, _, channels, bits, align) = self.fmt.unwrap();
        // A network read may stop halfway through an interleaved frame. Retain that
        // suffix until the next read; consume only complete frames in this window.
        let frames = (input.bytes().len() / align)
            .min((remaining as usize) / align)
            .min(4096);
        if frames == 0 {
            input.short()?;
            return Ok(None);
        }
        // Deinterleave directly into the channel-major codec ABI. IEEE float is
        // already amplitude PCM, so it only needs precision conversion, not integer
        // normalization or clamping of samples outside the nominal unit range.
        let mut data = vec![0.0; frames * channels];
        let width = bits as usize / 8;
        let b = input.bytes();
        for c in 0..channels {
            for i in 0..frames {
                let at = i * align + c * width;
                let s = &b[at..at + width];
                data[c * frames + i] = match (kind, bits) {
                    (3, 32) => f32::from_le_bytes(s.try_into().unwrap()),
                    (3, 64) => f64::from_le_bytes(s.try_into().unwrap()) as f32,
                    // Unsigned WAV bytes are centred on 128 before signed-S16 conversion.
                    // The shared integer conversion preserves AudioFileReader's asymmetric
                    // positive/negative S16 full-scale behavior for both WAV and FLAC.
                    (1, 8) => crate::flac::integer_pcm((s[0] as i32 - 128) << 8, 16),
                    (1, 16) => crate::flac::integer_pcm(
                        i16::from_le_bytes(s.try_into().unwrap()) as i32,
                        16,
                    ),
                    (1, 24) => {
                        // Sign-extend the packed little-endian 24-bit value before scaling.
                        // Reading it as an unsigned magnitude would corrupt every negative sample.
                        let v = ((s[0] as i32) | ((s[1] as i32) << 8) | ((s[2] as i32) << 16)) << 8
                            >> 8;
                        crate::flac::integer_pcm(v, 24)
                    }
                    // Keep signed 32-bit PCM on the high-precision scaling path. Casting to
                    // S16 first would discard signal detail and choose the wrong positive scale.
                    (1, 32) => {
                        crate::flac::integer_pcm(i32::from_le_bytes(s.try_into().unwrap()), 32)
                    }
                    _ => unreachable!(),
                };
            }
        }
        // The declared data length, not transport EOF or trailing RIFF metadata,
        // sets the exact source duration exposed to stream scheduling.
        input.consume(frames * align);
        self.remaining = Some(remaining - (frames * align) as u64);
        self.frames += frames as u64;
        Ok(Some(Pcm {
            data,
            channels,
            frames,
        }))
    }
    // Completion is tied to the data chunk's byte count. Trailing RIFF chunks
    // need not be fetched for playback, and transport EOF before this point is
    // reported as truncation by the next partial-frame check.
    pub fn finished(&self) -> bool {
        self.remaining == Some(0)
    }
}
