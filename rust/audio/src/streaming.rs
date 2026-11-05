//! Bounded PCM windows. The loading worker supplies chunks between render
//! calls; the source advances an absolute read cursor without allocating.
use crate::voices::Sample;
use std::cell::Cell;
// This ring is owned by the worklet, not shared between threads. PCM
// messages append between render calls; the render callback advances read
// through Cell while borrowing Sample immutably. Cell is not a thread lock.
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
        }) }
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
