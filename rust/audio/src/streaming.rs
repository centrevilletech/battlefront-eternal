//! Bounded PCM windows. The loading worker supplies chunks between render
//! calls; the source advances an absolute read cursor without allocating.
use crate::voices::Sample;
use std::cell::Cell;
// This ring is owned by the worklet, not shared between threads. PCM
// messages append between render calls; the render callback advances read
// through Cell while borrowing Sample immutably. Cell is not a thread lock.
/// Frames of decoded audio a streamed segment keeps ahead (about 0.17 s at
/// 48 kHz): many quanta, even at a high pitch.
const RING_FRAMES: usize = 8192;
const MAX_CHANNELS: usize = 12;
/// A client codec feeding a streamed segment a window at a time (the
/// enhanced mode's Opus streams): the next window of at most `frames`
/// frames, channel-major with the window's own length as stride; a window of
/// no frames when none is ready yet (asked again next quantum); None at the
/// end. It is called on the device thread between quanta, so it must only
/// hand over windows decoded elsewhere.
pub trait Producer: Send {
    fn produce(&mut self, frames: usize) -> Option<(&[f32], usize)>;
}
/// What feeds a streamed segment's ring as it plays.
pub enum Feed {
    Adpcm(Adpcm),
    Codec(Box<dyn Producer>),
}
/// Frames of decoded audio a codec-fed segment keeps ahead (about 0.34 s).
const CODEC_RING_FRAMES: usize = 16384;
/// What remains to decode of a streamed ADPCM segment.
pub struct Adpcm {
    parts: Vec<Vec<u8>>,
    channels: usize,
    blocks: usize,
    next: usize,
}
pub struct Ring {
    capacity: usize,
    written: usize,
    read: Cell<usize>,
    pub eof: bool,
    pub underrun: Cell<bool>,
}
impl Sample {
    // Reserve the entire bounded planar ring before scheduling playback.
    // frames is the advertised track length, independent of ring capacity;
    // physical storage wraps while source indices stay absolute.
    pub fn streaming(frames: usize, channels: usize, rate: f64, capacity: usize) -> Self {
        Self { frames, channels, rate, data: vec![0.0; capacity*channels], ring: Some(Ring {
            capacity, written: 0, read: Cell::new(0), eof: false, underrun: Cell::new(false),
        }), feed: None }
    }
    /// A streamed Xbox ADPCM segment kept compressed (a quarter of its 16-bit
    /// size), decoded a block at a time into a bounded ring as playback needs
    /// it. `parts` are the segment's substreams, `channels` each; the ring's
    /// channels run part by part, as the whole-segment decoder lays them out.
    /// Starts with the ring full, so a source can begin at once.
    pub fn adpcm(rate: f64, channels: usize, parts: Vec<Vec<u8>>) -> Option<Self> {
        let total = channels.checked_mul(parts.len())?;
        if channels == 0 || total == 0 || total > MAX_CHANNELS {
            return None;
        }
        let blocks = parts[0].len() / (36 * channels);
        let frames = blocks * 64;
        if frames == 0 {
            return Some(Self { frames, channels: total, rate, data: Vec::new(), ring: None, feed: None });
        }
        let mut sample = Self::streaming(frames, total, rate, frames.min(RING_FRAMES));
        sample.feed = Some(Box::new(Feed::Adpcm(Adpcm { parts, channels, blocks, next: 0 })));
        sample.feed();
        Some(sample)
    }
    /// A streamed segment `frames` long that `producer` decodes as it plays
    /// into a bounded ring. Starts with the ring full.
    pub fn codec(rate: f64, channels: usize, frames: usize, producer: Box<dyn Producer>) -> Option<Self> {
        if channels == 0 || channels > MAX_CHANNELS || frames == 0 {
            return None;
        }
        // Room for two whole windows past the segment's length, so a short
        // segment (or a decoder giving a few frames more than the header said)
        // always has the 2048 frames of room a window needs.
        let mut sample = Self::streaming(frames, channels, rate, (frames + 2 * 2048).min(CODEC_RING_FRAMES));
        sample.feed = Some(Box::new(Feed::Codec(producer)));
        sample.feed();
        Some(sample)
    }
    /// Decodes whole blocks while the ring has room for them; never
    /// allocates or frees, so it runs on the device thread between quanta. A
    /// feed that has ended stays with its sample, to be freed with it off
    /// the device thread.
    pub fn feed(&mut self) {
        if self.ring.as_ref().is_none_or(|r| r.eof) { return; }
        let Some(mut feed) = self.feed.take() else { return };
        match &mut *feed {
            Feed::Adpcm(adpcm) => { self.feed_adpcm(adpcm); }
            Feed::Codec(producer) => { self.feed_codec(producer.as_mut()); }
        }
        self.feed = Some(feed);
    }
    // Windows of at most 2048 frames while the ring has room; false once the
    // producer has ended (the ring then knows its true length).
    fn feed_codec(&mut self, producer: &mut dyn Producer) -> bool {
        while self.writable() >= 2048 {
            match producer.produce(2048) {
                Some((_, 0)) => return true,
                Some((pcm, frames)) => {
                    if !self.append(&pcm[..frames * self.channels], frames, false) {
                        self.append(&[], 0, true);
                        return false;
                    }
                }
                None => {
                    self.append(&[], 0, true);
                    return false;
                }
            }
        }
        true
    }
    fn feed_adpcm(&mut self, feed: &mut Adpcm) -> bool {
        let mut block = [0f32; 64 * MAX_CHANNELS];
        let mut next = feed.next;
        while next < feed.blocks && self.writable() >= 64 {
            let size = 36 * feed.channels;
            for (p, bytes) in feed.parts.iter().enumerate() {
                for c in 0..feed.channels {
                    let at = (p * feed.channels + c) * 64;
                    match bytes.get(next * size..(next + 1) * size) {
                        Some(bytes) => crate::adpcm::decode_block(bytes, feed.channels, c, &mut block[at..at + 64]),
                        None => block[at..at + 64].fill(0.0),
                    }
                }
            }
            next += 1;
            self.append(&block[..64 * self.channels], 64, next == feed.blocks);
        }
        feed.next = next;
        next < feed.blocks
    }
    // Credits count frames that can be appended without overwriting samples
    // still needed by interpolation. They are capacity-based, never track-length
    // based, so a long track costs no larger decoded allocation.
    pub fn writable(&self) -> usize { self.ring.as_ref().map_or(0, |r| r.capacity-(r.written-r.read.get())) }
    // A complete planar transfer must fit its issued credit. Reject overflow
    // rather than overwriting unread audio. EOF confirms the actual source count
    // without reallocating the ring or changing absolute cursor coordinates.
    pub fn append(&mut self, pcm: &[f32], frames: usize, eof: bool) -> bool {
        if frames > self.writable() || pcm.len() != frames*self.channels { return false; }
        let r = self.ring.as_mut().unwrap();
        for c in 0..self.channels { for i in 0..frames {
            self.data[c*r.capacity+(r.written+i)%r.capacity]=pcm[c*frames+i];
        } }
        r.written += frames; r.eof=eof;
        if eof { self.frames=r.written; }
        true
    }
    pub fn value(&self, c: usize, index: usize) -> f32 {
        match &self.ring { Some(r) => self.data[c*r.capacity+index%r.capacity], None => self.data[c*self.frames+index] }
    }
    pub fn available(&self, index: usize) -> bool { self.ring.as_ref().is_none_or(|r| index < r.written) }
    // Keep two old frames for pitched interpolation and final extrapolation.
    // Clamp retirement to written data, so silence after an underrun cannot grant
    // a producer credit for frames that were never received.
    pub fn consumed(&self, position: usize) {
        if let Some(r)=&self.ring { r.read.set(position.saturating_sub(2).min(r.written)); }
    }
}

impl Sample {
    // Unity-rate beds copy contiguous runs just as retained samples do.
    // Resolve ring wrap once per channel rather than divide every sample.
    pub fn copy_chunk(&self, c:usize, index:usize, out:&mut [f32], add:bool) {
        let (stride,at)=self.ring.as_ref().map_or((self.frames,index),|r|(r.capacity,index%r.capacity));
        let count=out.len().min(stride-at);
        let src=&self.data[c*stride+at..c*stride+at+count];let dst=&mut out[..count];
        if add {for i in 0..count {dst[i]+=src[i];}} else {dst.copy_from_slice(src);}
        if count<out.len() {
            let n=out.len()-count;let src=&self.data[c*stride..c*stride+n];let dst=&mut out[count..];
            if add {for i in 0..n {dst[i]+=src[i];}} else {dst.copy_from_slice(src);}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::voices::Sample;
    // Any bytes are valid Xbox ADPCM; a fixed pseudo-random run stands in.
    fn bytes(n: usize, seed: u32) -> Vec<u8> {
        let mut x = seed;
        (0..n).map(|_| { x = x.wrapping_mul(1664525).wrapping_add(1013904223); (x >> 24) as u8 }).collect()
    }
    #[test]
    fn a_streamed_segment_plays_the_same_samples_as_the_whole_decode() {
        for (channels, count, interleave) in [(2, 1, 0), (1, 3, 36 * 4)] {
            let raw = bytes(36 * channels * 400 * count, 7);
            let mut whole = crate::adpcm::Decoder { bytes: raw.clone(), output: Vec::new() };
            let frames = whole.decode(channels, count, interleave.max(1));
            let parts = if count == 1 { vec![raw] } else {
                let mut parts = vec![Vec::new(); count];
                for (i, run) in raw.chunks_exact(interleave).enumerate() { parts[i % count].extend_from_slice(run); }
                parts
            };
            let mut sample = Sample::adpcm(44100., channels, parts).unwrap();
            assert_eq!((sample.frames, sample.channels), (frames, channels * count));
            assert!(sample.data.len() < frames * channels * count, "decoded window is bounded");
            for index in 0..frames {
                if !sample.available(index) {
                    sample.consumed(index);
                    sample.feed();
                }
                for c in 0..channels * count {
                    assert_eq!(sample.value(c, index).to_bits(), whole.output[c][index].to_bits(), "channel {c} frame {index}");
                }
            }
            assert!(sample.ring.as_ref().unwrap().eof);
        }
    }
    // A ramp in windows of 300 frames, two channels (the second negated).
    struct Ramp { next: usize, total: usize, out: Vec<f32> }
    impl super::Producer for Ramp {
        fn produce(&mut self, frames: usize) -> Option<(&[f32], usize)> {
            let n = frames.min(300).min(self.total - self.next);
            if n == 0 { return None; }
            self.out.clear();
            for c in 0..2 { for i in 0..n { let v = (self.next + i) as f32; self.out.push(if c == 0 { v } else { -v }); } }
            self.next += n;
            Some((&self.out, n))
        }
    }
    // The ramp, ready only every third time it is asked (a decoder running behind).
    struct Late { ramp: Ramp, asked: usize }
    impl super::Producer for Late {
        fn produce(&mut self, frames: usize) -> Option<(&[f32], usize)> {
            self.asked += 1;
            if self.asked % 3 != 0 { return Some((&[], 0)); }
            self.ramp.produce(frames)
        }
    }
    #[test]
    fn a_window_not_ready_yet_is_asked_for_again_and_nothing_is_lost() {
        let total = 20_000;
        let mut sample = Sample::codec(48000., 2, total, Box::new(Late { ramp: Ramp { next: 0, total, out: Vec::new() }, asked: 0 })).unwrap();
        let mut index = 0;
        for _ in 0..100_000 {
            if index == total { break; }
            if sample.available(index) {
                assert_eq!(sample.value(0, index), index as f32);
                index += 1;
            } else {
                sample.consumed(index);
                sample.feed();
            }
        }
        assert_eq!(index, total);
        sample.consumed(total);
        // The end, too, may take a few quanta to be told.
        for _ in 0..3 { sample.feed(); }
        assert!(sample.ring.as_ref().unwrap().eof && sample.frames == total);
    }
    #[test]
    fn a_codec_fed_segment_plays_every_frame_in_order_then_ends() {
        let total = 40_000;
        let mut sample = Sample::codec(48000., 2, total, Box::new(Ramp { next: 0, total, out: Vec::new() })).unwrap();
        assert!(sample.data.len() < total * 2, "decoded window is bounded");
        for index in 0..total {
            if !sample.available(index) {
                sample.consumed(index);
                sample.feed();
            }
            assert_eq!((sample.value(0, index), sample.value(1, index)), (index as f32, -(index as f32)), "frame {index}");
        }
        sample.consumed(total);
        sample.feed();
        assert!(sample.ring.as_ref().unwrap().eof && sample.frames == total);
    }
}
