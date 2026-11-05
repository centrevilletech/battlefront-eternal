//! Incremental Ogg Opus (RFC 7845), including the discrete multistream
//! mapping written by the asset tools. A page and one packet are bounded;
//! neither the compressed file nor decoded track is retained.
use crate::{
    input::{Input, INPUT_LIMIT},
    Metadata, Pcm,
};
use opus_pure::OpusDecoder;
use std::collections::VecDeque;
// Opus packets can contain up to 120 ms at the mandatory 48 kHz timeline.
// This bound sizes per-packet scratch independently of track duration.
const MAX_FRAMES: usize = 5760;
// A packet can span pages, but each completed packet is decoded once and
// released. end marks the final packet on the EOS page for duration checks.
struct Packet {
    data: Vec<u8>,
    end: bool,
}
// One logical Ogg stream belongs to one loading-worker decoder job. Page
// assembly, Opus state and PCM allocations stay there; render callbacks do
// not call this parser, wait on it, or borrow any of its memory.
pub struct Ogg {
    serial: Option<u32>,
    sequence: u32,
    partial: Vec<u8>,
    packets: VecDeque<Packet>,
    decoders: Vec<OpusDecoder>,
    coupled: usize,
    mapping: Vec<u8>,
    family: u8,
    // All three counters are in 48 kHz decoded frames, before rate conversion.
    // pre-skip removes codec priming; the final granule removes encoder padding.
    skip: u64,
    position: u64,
    final_position: Option<u64>,
    tags: bool,
    eos: bool,
    scratch: Vec<f32>,
    rebuilt: Vec<u8>,
    pub info: Option<Metadata>,
}
impl Ogg {
    pub fn new() -> Self {
        Self {
            serial: None,
            sequence: 0,
            partial: Vec::new(),
            packets: VecDeque::new(),
            decoders: Vec::new(),
            coupled: 0,
            mapping: Vec::new(),
            family: 0,
            skip: 0,
            position: 0,
            final_position: None,
            tags: false,
            eos: false,
            scratch: vec![0.0; MAX_FRAMES * 2],
            rebuilt: Vec::new(),
            info: None,
        }
    }
    // Do not advance Input until the complete header, lacing table and body
    // are present. A split network read must produce NeedInput, not a failed
    // checksum or a partially committed page.
    fn page(&mut self, input: &mut Input) -> Result<bool, String> {
        let b = input.bytes();
        if b.len() < 27 {
            input.short()?;
            return Ok(false);
        }
        if &b[..4] != b"OggS" || b[4] != 0 {
            return Err("invalid Ogg page".into());
        }
        let count = b[26] as usize;
        if b.len() < 27 + count {
            input.short()?;
            return Ok(false);
        }
        // A page contains at most 255 laces, each at most 255 bytes. Validate the
        // entire encoded span before slicing its body; a packet may still continue
        // onto another page and is bounded separately by INPUT_LIMIT.
        let size = 27 + count + b[27..27 + count].iter().map(|x| *x as usize).sum::<usize>();
        if b.len() < size {
            input.short()?;
            return Ok(false);
        }
        // Serial and sequence checks forbid gaps and concatenated logical streams.
        // Decoder state cannot cross a new OpusHead without resetting its channel
        // mapping, pre-skip and granule origin.
        let serial = u32::from_le_bytes(b[14..18].try_into().unwrap());
        let sequence = u32::from_le_bytes(b[18..22].try_into().unwrap());
        if self.serial.is_none() {
            if b[5] & 2 == 0 || sequence != 0 {
                return Err("Ogg lacks its first page".into());
            }
            self.serial = Some(serial);
        }
        if self.serial != Some(serial) || sequence != self.sequence {
            return Err("Ogg page discontinuity or chained stream".into());
        }
        // A continuation flag must agree with retained packet bytes. Otherwise the
        // first lacing segment could be mistaken for a new Opus packet.
        if (b[5] & 1 != 0) != !self.partial.is_empty() {
            return Err("Ogg continuation mismatch".into());
        }
        // Checksum bytes are part of the page header but are zeroed by crc.
        // Only after validation may sequence, EOS and packet queues change; retrying
        // a short read must leave these persistent fields untouched.
        let expected = u32::from_le_bytes(b[22..26].try_into().unwrap());
        if crc(&b[..size]) != expected {
            return Err("Ogg page checksum mismatch".into());
        }
        self.sequence = self.sequence.wrapping_add(1);
        // The EOS granule is an absolute decoded-frame endpoint, not the number
        // of frames on this page. Retain it while draining the page's packet queue.
        let end = b[5] & 4 != 0;
        if end {
            self.final_position = Some(u64::from_le_bytes(b[6..14].try_into().unwrap()));
            self.eos = true;
        }
        // Laces of 255 continue a packet; a smaller lace, including zero, ends it.
        // The accumulated packet has its own budget because it can cross pages.
        // Queue only this page's completed packets before reading another page.
        let mut at = 27 + count;
        for (i, &n) in b[27..27 + count].iter().enumerate() {
            let n = n as usize;
            if self.partial.len() + n > INPUT_LIMIT {
                return Err("Ogg packet exceeds the input budget".into());
            }
            self.partial.extend_from_slice(&b[at..at + n]);
            at += n;
            if n < 255 {
                self.packets.push_back(Packet {
                    data: std::mem::take(&mut self.partial),
                    end: end && i + 1 == count,
                });
            }
        }
        if end && !self.partial.is_empty() {
            return Err("Ogg ended inside a packet".into());
        }
        input.consume(size);
        Ok(true)
    }
    // OpusHead sets mapping and decoder gain. Its original-input-rate field is
    // informational: RFC 7845 granules and pre-skip always use 48 kHz. Decode
    // there first and let the common resampler deliver the requested device rate.
    fn head(&mut self, p: &[u8]) -> Result<(), String> {
        if p.len() < 19 || &p[..8] != b"OpusHead" || p[8] & 0xf0 != 0 {
            return Err("invalid OpusHead".into());
        }
        let channels = p[9] as usize;
        if channels == 0 || channels > 32 {
            return Err("unsupported Opus channel count".into());
        }
        self.skip = u16::from_le_bytes(p[10..12].try_into().unwrap()) as u64;
        let gain = i16::from_le_bytes(p[16..18].try_into().unwrap());
        self.family = p[18];
        let streams;
        // Family zero is mono or one coupled stereo stream. Families one and 255
        // carry explicit stream counts and lane maps; 255 is the tools' discrete
        // multichannel format, including layouts wider than ordinary surround.
        if self.family == 0 {
            if channels > 2 {
                return Err("invalid family-zero Opus layout".into());
            }
            streams = 1;
            self.coupled = channels - 1;
            self.mapping = (0..channels as u8).collect();
        } else {
            if !matches!(self.family, 1 | 255) || p.len() < 21 + channels {
                return Err("unsupported Opus mapping".into());
            }
            streams = p[19] as usize;
            self.coupled = p[20] as usize;
            self.mapping = p[21..21 + channels].to_vec();
            // A map byte of 255 names a silent lane, not decoder channel 255. Every
            // other byte must name one of the coupled or uncoupled decoded channels.
            if streams == 0
                || self.coupled > streams
                || streams + self.coupled > 255
                || self
                    .mapping
                    .iter()
                    .any(|&m| m != 255 && m as usize >= streams + self.coupled)
            {
                return Err("invalid Opus channel mapping".into());
            }
        }
        // Preserve Chromium kOpusVorbisChannelMap, including its family-one
        // ordering. The asset tools use family 255, whose lanes stay discrete.
        if self.family == 1 {
            let order: &[usize] = match channels {
                1 => &[0],
                2 => &[0, 1],
                3 => &[0, 2, 1],
                4 => &[0, 1, 2, 3],
                5 => &[0, 4, 1, 2, 3],
                6 => &[0, 4, 1, 2, 3, 5],
                7 => &[0, 4, 1, 2, 3, 5, 6],
                8 => &[0, 6, 1, 2, 3, 4, 5, 7],
                _ => return Err("invalid Vorbis Opus layout".into()),
            };
            self.mapping = order.iter().map(|&c| self.mapping[c]).collect();
        }
        // Coupled streams come first and each produces two interleaved channels.
        // Construct every decoder at 48 kHz; lower-rate synthesis would select a
        // different codec path instead of reproducing host decoding plus conversion.
        // The signed Q8 dB header gain is applied once by the Opus decoder itself.
        for s in 0..streams {
            let mut decoder = OpusDecoder::new(48000, if s < self.coupled { 2 } else { 1 })
                .map_err(|e| format!("Opus init: {e}"))?;
            decoder.gain_q8 = gain as i32;
            self.decoders.push(decoder);
        }
        self.info = Some(Metadata {
            source_rate: 48000,
            rate: 48000,
            channels,
            bits: 32,
            frames: None,
            format: crate::layout::OPUS as u32,
        });
        Ok(())
    }
    // Drain queued packets before parsing another page. Header and tags packets
    // carry no audible PCM; ordinary packets may also be wholly removed by
    // pre-skip, so continue until a nonempty window or a genuine input boundary.
    pub fn next(&mut self, input: &mut Input) -> Result<Option<Pcm>, String> {
        loop {
            let packet = match self.packets.pop_front() {
                Some(p) => p,
                None => {
                    if self.eos {
                        return Ok(None);
                    }
                    if !self.page(input)? {
                        return Ok(None);
                    }
                    continue;
                }
            };
            if self.info.is_none() {
                self.head(&packet.data)?;
                continue;
            }
            // OpusTags follows the identification header and carries no decoder state.
            // It is still assembled as a bounded packet so a large comment block cannot
            // force whole-file retention or be mistaken for compressed audio.
            if !self.tags {
                if !packet.data.starts_with(b"OpusTags") {
                    return Err("missing OpusTags".into());
                }
                self.tags = true;
                continue;
            }
            let channels = self.mapping.len();
            let mut streams = &packet.data[..];
            let mut decoded = 0;
            // Zero initialization supplies silent mapped lanes. Scatter each decoder's
            // interleaved output straight into planar lanes, with a fixed temporary stride
            // until pre-skip and end trimming establish this packet's final frame count.
            let mut data = vec![0.0; MAX_FRAMES * channels];
            for s in 0..self.decoders.len() {
                let last = s + 1 == self.decoders.len();
                // All but the last elementary stream use self-delimited framing. Rebuild
                // its ordinary packet for the single-stream decoder and advance by exactly
                // the encoded span; the final stream occupies the remaining packet bytes.
                let advance = if last {
                    streams.len()
                } else {
                    undelimit(streams, &mut self.rebuilt)?
                };
                let bytes = if last { streams } else { &self.rebuilt };
                let n = self.decoders[s]
                    .decode(bytes, MAX_FRAMES, &mut self.scratch)
                    .map_err(|e| format!("Opus packet: {e}"))?;
                // A multistream packet represents one common time interval. Different
                // stream durations would misalign channels and invalidate the final granule.
                if s != 0 && decoded != n {
                    return Err("Opus streams disagree on duration".into());
                }
                decoded = n;
                let width = if s < self.coupled { 2 } else { 1 };
                // Decoded channel numbering places both lanes of each coupled stream
                // before the uncoupled channels. The map can duplicate a channel in several
                // output lanes; scatter each occurrence without assuming a permutation.
                let target = if s < self.coupled {
                    s * 2
                } else {
                    s + self.coupled
                };
                for (c, &mapping) in self.mapping.iter().enumerate() {
                    if (mapping as usize) >= target && (mapping as usize) < target + width {
                        for i in 0..n {
                            data[c * MAX_FRAMES + i] =
                                self.scratch[i * width + mapping as usize - target];
                        }
                    }
                }
                streams = &streams[advance..];
            }
            // Apply priming and padding in the original 48 kHz timeline before any
            // rate conversion. position advances by decoded frames even when the whole
            // packet is skipped, so a long pre-skip cannot shift later packets.
            let start = self.skip.saturating_sub(self.position).min(decoded as u64) as usize;
            let end = self.final_position.map_or(decoded, |last| {
                last.saturating_sub(self.position).min(decoded as u64) as usize
            });
            if end < start {
                return Err("invalid Opus end granule".into());
            }
            let frames = end - start;
            // Pack the surviving span into tightly planar output. copy_within handles
            // overlap as later channels move toward the front of the same allocation.
            for c in 0..channels {
                data.copy_within(c * MAX_FRAMES + start..c * MAX_FRAMES + end, c * frames);
            }
            data.truncate(channels * frames);
            // Granules count all decoded samples, including priming and padding.
            // Only the packed PCM window loses those frames; moving position by the
            // trimmed length would apply pre-skip repeatedly and misread the EOS count.
            self.position += decoded as u64;
            if packet.end {
                let last = self.final_position.ok_or("missing Opus end granule")?;
                if last > self.position || last < self.skip {
                    return Err("invalid final Opus sample count".into());
                }
                self.info.as_mut().unwrap().frames = Some(last - self.skip);
            }
            if frames > 0 {
                return Ok(Some(Pcm {
                    data,
                    channels,
                    frames,
                }));
            }
        }
    }
    // Receiving EOS alone is insufficient: its page can still have several
    // queued audio packets. Completion waits until each has been decoded and
    // trimmed against the final granule, preserving the last audible samples.
    pub fn finished(&self) -> bool {
        self.eos && self.packets.is_empty()
    }
}
// Ogg uses its own non-reflected polynomial with the four checksum bytes
// treated as zero. Verify a page before trusting its lacing or EOS duration;
// the compressed-tail duration probe uses the identical checksum rule.
fn crc(bytes: &[u8]) -> u32 {
    let mut value = 0u32;
    for (i, &byte) in bytes.iter().enumerate() {
        value ^= (if (22..26).contains(&i) {
            0
        } else {
            byte as u32
        }) << 24;
        for _ in 0..8 {
            value = (value << 1)
                ^ if value & 0x80000000 != 0 {
                    0x04c11db7
                } else {
                    0
                };
        }
    }
    value
}
// RFC 6716 lengths below 252 occupy one byte; larger lengths use the
// first byte plus four times a second byte, rather than a generic varint.
fn size(data: &[u8], at: &mut usize) -> Result<usize, String> {
    let a = *data.get(*at).ok_or("short self-delimited Opus size")? as usize;
    *at += 1;
    if a < 252 {
        Ok(a)
    } else {
        let b = *data.get(*at).ok_or("short self-delimited Opus size")? as usize;
        *at += 1;
        Ok(a + 4 * b)
    }
}
// RFC 6716 Appendix B: remove only the extra last-frame length. All other
// framing and padding is retained for the single-stream decoder to validate.
fn undelimit(data: &[u8], out: &mut Vec<u8>) -> Result<usize, String> {
    let toc = *data.first().ok_or("empty Opus stream")?;
    let mut at = 1;
    let mut payload = 0;
    let mut padding = 0;
    let (count, vbr) = match toc & 3 {
        0 => (1, false),
        1 => (2, false),
        2 => (2, true),
        _ => {
            let flags = *data.get(at).ok_or("short Opus count")?;
            at += 1;
            if flags & 64 != 0 {
                loop {
                    let n = *data.get(at).ok_or("short Opus padding")? as usize;
                    at += 1;
                    padding += if n == 255 { 254 } else { n };
                    if n != 255 {
                        break;
                    }
                }
            }
            (flags as usize & 63, flags & 128 != 0)
        }
    };
    // At most 48 frames fit in an Opus packet. Parsing their byte extents is
    // only demultiplexing; the decoder still validates TOC duration and packet
    // legality after the self-delimited length is removed.
    if count == 0 || count > 48 {
        return Err("invalid Opus frame count".into());
    }
    if vbr {
        for _ in 1..count {
            payload += size(data, &mut at)?;
        }
    }
    // Earlier VBR lengths belong to the ordinary packet and must survive.
    // Only the extra last-frame size is removed; CBR uses that size for every
    // frame. Include encoded padding in the returned elementary-stream span.
    let remove = at;
    let last = size(data, &mut at)?;
    payload += if vbr { last } else { last * count };
    let end = at
        .checked_add(payload + padding)
        .ok_or("Opus size overflow")?;
    if end > data.len() {
        return Err("truncated self-delimited Opus stream".into());
    }
    // Reuse one reconstruction buffer across elementary streams and packets.
    // This allocation belongs to the loading worker. The single-stream decoder
    // must consume the borrowed bytes before the buffer is rebuilt again.
    out.clear();
    out.extend_from_slice(&data[..remove]);
    out.extend_from_slice(&data[at..end]);
    Ok(end)
}
impl Ogg {
    // A seekable asset's last page supplies its exact duration before the
    // first PCM window plays. The tail remains compressed and bounded.
    pub fn tail_frames(&self, bytes: &[u8]) -> Result<u64, String> {
        // Scan backward for a complete, checksum-valid EOS page. A capture string
        // inside compressed audio is insufficient. Subtract pre-skip here so the
        // published asset length counts audible PCM rather than codec priming.
        for at in (0..bytes.len().saturating_sub(26)).rev() {
            let b = &bytes[at..];
            if &b[..4] != b"OggS" || b[4] != 0 || b[5] & 4 == 0 {
                continue;
            }
            let n = b[26] as usize;
            if b.len() < 27 + n {
                continue;
            }
            let size = 27 + n + b[27..27 + n].iter().map(|x| *x as usize).sum::<usize>();
            if b.len() < size
                || crc(&b[..size]) != u32::from_le_bytes(b[22..26].try_into().unwrap())
            {
                continue;
            }
            // The tail result is audible source duration, not an output-rate count.
            // Decoder.tail converts it once for scheduling and informs the resampler,
            // while forward parsing will independently validate its final packet count.
            let last = u64::from_le_bytes(b[6..14].try_into().unwrap());
            return last
                .checked_sub(self.skip)
                .ok_or("Opus end precedes pre-skip".into());
        }
        Err("Opus asset lacks a complete final page".into())
    }
}
