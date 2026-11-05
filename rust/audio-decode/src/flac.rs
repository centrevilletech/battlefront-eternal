//! Native FLAC frames, decoded as integers before matching the host's PCM
//! conversion. FLAC itself never resamples, mixes or changes precision.
use crate::{input::Input, Metadata, Pcm};
use std::io::Cursor;
// This parser owns FLAC container progress, while Claxon owns integer
// frame reconstruction and checksums. It replaces AudioFileReader's FLAC
// input without inheriting the device rate or browser channel mixing.
pub struct Flac {
    header: bool,
    metadata_done: bool,
    // Large pictures and tags are counted down across reads. Their contents
    // never need to fit in the compressed window.
    skip: usize,
    last: bool,
    maximum_frame: usize,
    scratch: Vec<i32>,
    pub info: Option<Metadata>,
    pub frames: u64,
}
impl Flac {
    pub fn new() -> Self {
        Self {
            header: false,
            metadata_done: false,
            skip: 0,
            last: false,
            maximum_frame: 0,
            scratch: Vec::new(),
            info: None,
            frames: 0,
        }
    }
    // Return at most one decoded frame. None means more compressed bytes are
    // needed, or the declared sample count is complete; finished distinguishes
    // the two. Calls and their temporary allocations stay on the loading worker.
    pub fn next(&mut self, input: &mut Input) -> Result<Option<Pcm>, String> {
        if !self.header {
            if input.bytes().len() < 4 {
                input.short()?;
                return Ok(None);
            }
            if &input.bytes()[..4] != b"fLaC" {
                return Err("invalid FLAC signature".into());
            }
            input.consume(4);
            self.header = true;
        }
        // FLAC metadata can cross arbitrary network chunk boundaries. Do not
        // consume a header until its fields are available; skipped payload bytes
        // can be discarded immediately without rolling the parser back.
        while !self.metadata_done {
            if self.skip > 0 {
                let count = self.skip.min(input.bytes().len());
                input.consume(count);
                self.skip -= count;
                if self.skip > 0 {
                    input.short()?;
                    return Ok(None);
                }
                if self.last {
                    self.metadata_done = true;
                    break;
                }
            }
            let b = input.bytes();
            if b.len() < 4 {
                input.short()?;
                return Ok(None);
            }
            // The high bit terminates the metadata chain, independent of block type.
            // STREAMINFO is the only metadata needed for playback.
            let last = b[0] & 128 != 0;
            let kind = b[0] & 127;
            let length = ((b[1] as usize) << 16) | ((b[2] as usize) << 8) | b[3] as usize;
            if kind == 0 {
                if length != 34 {
                    return Err("invalid FLAC STREAMINFO length".into());
                }
                if b.len() < 38 {
                    input.short()?;
                    return Ok(None);
                }
                let data = &b[4..38];
                // STREAMINFO packs rate, channel count, precision and total samples into
                // one big-endian word. A zero total is unspecified, not an empty file.
                let packed = u64::from_be_bytes(data[10..18].try_into().unwrap());
                let rate = (packed >> 44) as u32;
                let channels = (((packed >> 41) & 7) + 1) as usize;
                let bits = (((packed >> 36) & 31) + 1) as u32;
                let frames = packed & 0xfffffffff;
                if rate == 0 || !matches!(bits, 8 | 12 | 16 | 20 | 24) {
                    return Err(format!(
                        "unsupported FLAC precision/rate: {bits} bits, {rate} Hz"
                    ));
                }
                // Some encoders omit the maximum compressed frame size. Bound it from
                // maximum block size and raw precision, with room for frame overhead.
                // This lets the frame reader run only when a complete frame can fit.
                let block = u16::from_be_bytes(data[2..4].try_into().unwrap()) as usize;
                self.maximum_frame =
                    ((data[7] as usize) << 16) | ((data[8] as usize) << 8) | data[9] as usize;
                if self.maximum_frame == 0 {
                    self.maximum_frame =
                        (block * channels * (bits as usize + 1)).div_ceil(8) + 1024;
                }
                // Refuse a frame that could defeat the bounded input contract. This is
                // an asset error, not permission to buffer the rest of a track.
                if self.maximum_frame > crate::input::INPUT_LIMIT {
                    return Err("FLAC frame exceeds the input budget".into());
                }
                self.info = Some(Metadata {
                    source_rate: rate,
                    rate,
                    channels,
                    bits,
                    frames: if frames == 0 { None } else { Some(frames) },
                    format: crate::layout::FLAC as u32,
                });
                input.consume(38);
                self.last = last;
                if last {
                    self.metadata_done = true;
                }
            } else {
                input.consume(4);
                self.skip = length;
                self.last = last;
                if length == 0 && last {
                    self.metadata_done = true;
                }
            }
        }
        let info = self.info.as_ref().ok_or("FLAC lacks STREAMINFO")?;
        // A known sample count ends playback even if metadata or transport bytes
        // follow. Conversely, early EOF must not silently shorten scheduled music.
        if info.frames.is_some_and(|total| self.frames == total) {
            return Ok(None);
        }
        if input.bytes().is_empty() {
            if input.eof && info.frames.is_some_and(|total| self.frames != total) {
                return Err("FLAC ended before its declared sample count".into());
            }
            return Ok(None);
        }
        // Claxon's frame reader is synchronous, so wait for the advertised frame
        // budget rather than treating a split network frame as corrupt. At EOF the
        // remaining bytes must contain a valid final frame or decoding fails.
        if !input.eof && input.bytes().len() < self.maximum_frame {
            return Ok(None);
        }
        // The cursor borrows the compressed window only during this call. Consume
        // exactly what Claxon read; retain the remainder for the next FLAC frame.
        let mut cursor = Cursor::new(input.bytes());
        let mut reader = claxon::frame::FrameReader::new(&mut cursor);
        let block = reader
            .read_next_or_eof(std::mem::take(&mut self.scratch))
            .map_err(|e| format!("FLAC frame: {e}"))?
            .ok_or("missing FLAC frame")?;
        let consumed = cursor.position() as usize;
        if block.channels() as usize != info.channels {
            return Err("FLAC frame changes its channel count".into());
        }
        let frames = block.duration() as usize;
        // Claxon exposes signed integer samples by channel. Convert directly to
        // planar f32 so no interleaved whole-track copy is needed. Recycle its integer
        // scratch separately from the PCM window passed to the resampler.
        let mut data = vec![0.0; frames * info.channels];
        for c in 0..info.channels {
            for i in 0..frames {
                data[c * frames + i] = integer_pcm(block.sample(c as u32, i as u32), info.bits);
            }
        }
        self.scratch = block.into_buffer();
        self.frames += frames as u64;
        input.consume(consumed);
        if info.frames.is_some_and(|total| self.frames > total) {
            return Err("FLAC exceeds its declared sample count".into());
        }
        Ok(Some(Pcm {
            data,
            channels: info.channels,
            frames,
        }))
    }
    // Unknown-length FLAC ends only after metadata and all compressed input
    // are exhausted. Known-length files use their lossless sample count.
    pub fn finished(&self, input: &Input) -> bool {
        self.info
            .as_ref()
            .is_some_and(|i| i.frames == Some(self.frames))
            || (self.metadata_done && input.eof && input.bytes().is_empty())
    }
}
// AudioFileReader's integer conversion uses a float reciprocal. For S16,
// positive and negative full scale differ; division in f64 is not equivalent.
// For 8/12-bit input, promote into the signed 16-bit range first. The
// browser's positive reciprocal is 1/32767 and its negative reciprocal is
// 1/32768; multiply after rounding both operands to f32. Symmetric division
// would alter positive samples even for otherwise lossless FLAC.
// Above 16 bits the browser uses power-of-two scaling instead. This differs
// from the original game's raw S16 bank, which deliberately uses 1/32768
// on both signs; the two asset formats must not share a normalization rule.
pub fn integer_pcm(value: i32, bits: u32) -> f32 {
    if bits <= 16 {
        let shifted = value << (16 - bits);
        let reciprocal = if shifted < 0 {
            1.0f32 / 32768.0
        } else {
            1.0f32 / 32767.0
        };
        shifted as f32 * reciprocal
    } else {
        (value as f64 * (1.0 / (1u64 << (bits - 1)) as f64)) as f32
    }
}
