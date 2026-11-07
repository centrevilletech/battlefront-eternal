//! Decoded stream PCM: scheduled segment sources and a shared gain timeline.
//! Three source slots cover a segment advance queued just before its end, with exact context-frame starts and stops instead of page timers.

use crate::timeline::Timeline;
use crate::voices::Sample;
use crate::FRAMES;

// StreamOut and FrontendAudio use the same scheduled source semantics.
// The worklet owns Source and its PCM/ring; the loading worker can only send
// new windows. No decoder, network read or allocation belongs in process.
pub const CHANNELS: usize = 12;
// AudioScheduledSourceHandler rounds scheduling times through a 1/1024
// sample grid before selecting an integer frame. Starts/stops round upward;
// unity-rate buffer offsets use nearest-frame rounding instead.
fn frame(time: f64, rate: f64, up: bool) -> f64 {
    let v = (time * rate * 1024.0).round() / 1024.0;
    if up {
        v.ceil()
    } else {
        v.round()
    }
}
// This is the state formerly held by each AudioBufferSourceNode: an
// absolute context start/stop and a fractional source cursor. Copying Source
// copies numbers only; PCM ownership remains in the sample table.
#[derive(Clone, Copy)]
pub struct Source {
    pub on: bool,
    sample: usize,
    pos: f64,
    start: f64,
    stop: f64,
    offset: f64,
    begun: bool,
    step: f64,
    end: f64,
    looping: bool,
}
impl Default for Source {
    fn default() -> Self {
        Self {
            on: false,
            sample: 0,
            pos: 0.0,
            start: 0.0,
            stop: f64::INFINITY,
            offset: 0.0,
            begun: false,
            step: 1.0,
            end: 0.0,
            looping: false,
        }
    }
}
impl Source {
    // StreamOut.start and FrontendAudio.play pass seconds, just as BufferSource
    // start did. Clamp offsets to asset duration and retain source/device rate
    // scaling even though most decoded assets already use the device rate.
    pub fn start(
        &mut self,
        sample: usize,
        data: &Sample,
        when: f64,
        offset: f64,
        pitch: f64,
        looping: bool,
        rate: f64,
    ) {
        let pitch = pitch as f32 as f64; // playbackRate is a float AudioParam.
        let frames = data.frames;
        let offset = offset.clamp(0.0, frames as f64 / data.rate);
        let start = frame(when, rate, true);
        *self = Self {
            on: true,
            sample,
            // The host's unity playback path snaps an offset to a source frame.
            // Pitched playback retains fractional offsets for linear interpolation;
            // applying one rounding rule to both paths would shift loop starts.
            pos: if pitch == 1.0 {
                frame(offset, data.rate, false)
            } else {
                offset * data.rate
            },
            start,
            stop: f64::INFINITY,
            // A start rounded to the next context frame still advances the cursor by
            // the fractional interval that elapsed. Apply this once on first render,
            // not every time a quantum resumes after waiting for stream PCM.
            offset: when * rate - start,
            begun: false,
            step: (data.rate / rate * pitch).clamp(0.0, 1024.0),
            // Native buffer duration travels through seconds before becoming
            // its loop/end frame. At 44.1 kHz this can exceed the integer by
            // an ulp and emit one extrapolated tail sample within a quantum.
            end: (frames as f64 / data.rate) * data.rate,
            looping,
        };
    }
    // Stopping uses the context timeline, independent of source pitch. A
    // future stop remains scheduled even if several quanta pass before it.
    pub fn stop(&mut self, when: f64, rate: f64) {
        self.stop = frame(when, rate, true);
    }
    // Write only the source's active span into the caller's preallocated
    // planar scratch. add distinguishes the first segment from overlapping
    // segments feeding the same former stream GainNode.
    pub(crate) fn process(&mut self, data: &Sample, block: f64, out: &mut [f32], add: bool) {
        if !self.on || self.start >= block + FRAMES as f64 {
            return;
        }
        let frames = data.frames;
        // The compressed tail predicts duration before playback. EOF confirms the
        // actual ring sample count; update the endpoint without replacing its buffer
        // or resetting a cursor already moving through the stream.
        if data.ring.as_ref().is_some_and(|r| r.eof) { self.end = (frames as f64 / data.rate)*data.rate; }
        if !self.looping && self.pos >= frames as f64 {
            self.on = false;
            return;
        }
        if self.looping && self.pos >= self.end {
            self.pos = 0.0;
        }
        // Resolve the scheduled fractional start only once. A Source bound before
        // its start can remain idle for many blocks without advancing its PCM cursor.
        if !self.begun {
            self.begun = true;
            if self.offset < 0.0 {
                self.pos += (self.offset * self.step).abs();
            }
        }
        // Native buffer sources copy whole, unity-rate runs. Most beds
        // are already at the context rate; avoid interpolation work there.
        if self.step == 1.0
            && self.pos.fract() == 0.0
            && self.start <= block
            && self.stop >= block + FRAMES as f64
            && self.pos + FRAMES as f64 <= frames as f64
            && data.available(self.pos as usize + FRAMES - 1)
        {
            // The unity fast path is still a scheduled source: its predicates exclude
            // partial starts, stops, loop boundaries and incomplete ring windows. A
            // contiguous or wrapped copy preserves exactly the same source samples.
            let at = self.pos as usize;
            for c in 0..data.channels.min(CHANNELS) {
                let dst = &mut out[c * FRAMES..(c + 1) * FRAMES];
                data.copy_chunk(c, at, dst, add);
            }
            self.pos += FRAMES as f64;
            data.consumed(self.pos as usize);
            if self.pos >= self.end {
                if self.looping {
                    self.pos = 0.0;
                } else {
                    self.on = false;
                }
            }
            return;
        }
        // Starts/stops are already integer context frames. Resolve the
        // quantum's active span once; pitched mono pulses need not repeat
        // three scheduling tests and a clock conversion for every sample.
        let begin = (self.start - block).clamp(0.0, FRAMES as f64) as usize;
        let limit = (self.stop - block).clamp(0.0, FRAMES as f64) as usize;
        for i in begin..limit {
            let index = self.pos as usize;
            let fraction = self.pos - index as f64;
            // Interpolate across a loop boundary using the wrapped next frame, rather
            // than repeating the last sample. Nonlooping sources clamp the neighbor at
            // the end and preserve the host's separate tail extrapolation below.
            let next = if self.looping && (index + 1) as f64 >= self.end {
                ((self.pos + 1.0 - self.end).max(0.0) as usize).min(frames - 1)
            } else {
                (index + 1).min(frames - 1)
            };
            let index = index.min(frames - 1);
            // Underrun is a producer failure, not clean source completion. Leave the
            // unwritten scratch silent and flag the ring for the host to report. Never
            // block the audio thread waiting for another compressed or PCM window.
            if !data.available(next) || !data.available(index) {
                if let Some(r)=&data.ring { r.underrun.set(true); }
                break;
            }
            for c in 0..data.channels.min(CHANNELS) {
                // The former buffer source extrapolates from the last two frames when a
                // fractional read falls just beyond its final whole frame. Preserve this
                // small tail instead of silently changing it to a held endpoint sample.
                let value = if index == next && index >= 1 {
                    let a = data.value(c, index - 1) as f64;
                    let b = data.value(c, index) as f64;
                    b + (b - a) * fraction
                } else {
                    (1.0 - fraction) * data.value(c, index) as f64
                        + fraction * data.value(c, next) as f64
                };
                // Cursor interpolation uses doubles, then rounds to the source's f32
                // output before any overlapping-source sum. Combining these operations
                // would erase the former BufferSource-to-GainNode rounding boundary.
                let value = value as f32;
                if add {
                    out[c * FRAMES + i] += value;
                } else {
                    out[c * FRAMES + i] = value;
                }
            }
            self.pos += self.step;
            if self.pos >= self.end {
                if self.looping {
                    self.pos = (self.pos - self.end) % self.end;
                } else {
                    self.on = false;
                    break;
                }
            }
        }
        data.consumed(self.pos as usize);
        if self.stop < block + FRAMES as f64 {
            self.on = false;
        }
    }
}

// A logical bed owns three scheduled segment sources and one shared gain.
// Overlaps sum before that gain, matching StreamOut's stream input node.
// Filters and decorrelation delays live as long as the bed, not a segment.
pub struct Stream {
    pub on: bool,
    pub channels: usize,
    pub kind: u32,
    pub spatial: bool,
    pub gain: Timeline,
    pub cutoff: Timeline,
    pub send: Timeline,
    pub sources: [Source; 3],
    pub output: Vec<f32>,
    pub direction: [f32; 3],
    pub positions: [Timeline; 3],
    pub filter: Vec<crate::biquad::Biquad>,
    pub side: Vec<f32>,
    pub upmix_filter: crate::biquad::Biquad,
    pub delays: [Vec<f32>; 4],
    pub delay_at: [usize; 4],
    gains: [f32; FRAMES],
}
impl Stream {
    pub fn new(rate: f64) -> Self {
        Self {
            on: false,
            channels: 1,
            kind: 0,
            spatial: false,
            gain: Timeline::new(1.0),
            cutoff: Timeline::new(20000.0f64.min(rate / 2.0) as f32),
            send: Timeline::new(0.0),
            sources: [Source::default(); 3],
            output: vec![0.0; CHANNELS * FRAMES],
            direction: [0.0, 0.0, -1.0],
            positions: [Timeline::new(0.0), Timeline::new(0.0), Timeline::new(-1.0)],
            filter: Vec::new(),
            side: vec![0.0; FRAMES],
            upmix_filter: crate::biquad::Biquad::new(
                crate::biquad::Kind::Lowpass,
                7000.0,
                1.0,
                0.0,
                rate,
            ),
            // Reserve enough history for the longest fractional tap plus a quantum.
            // The extra sample supplies interpolation at wrap; output routing never
            // resizes these rings while audio is rendering.
            delays: std::array::from_fn(|i| {
                vec![0.0; (rate * [0.012, 0.017, 0.008, 0.011][i]).ceil() as usize + FRAMES + 1]
            }),
            delay_at: [0; 4],
            gains: [0.0; FRAMES],
        }
    }
    // Binding records the lane width learned from the first segment. Later
    // narrower segments leave missing lanes at zero in the cleared output;
    // they do not cause browser-style implicit channel-width changes.
    pub fn configure(&mut self, channels: usize, kind: u32, spatial: bool) {
        self.on = true;
        self.channels = channels.min(CHANNELS);
        self.kind = kind;
        self.spatial = spatial;
    }
    // Clear once, then preserve scheduled segment order during overlap. The
    // first source writes and later sources add into the same fixed scratch;
    // the shared stream gain is applied only after every segment contributes.
    pub fn process(&mut self, samples: &[Option<Sample>], block: u64, rate: f64, x86: bool) {
        self.output.fill(0.0);
        let mut added = false;
        for source in &mut self.sources {
            if source.on {
                if let Some(Some(sample)) = samples.get(source.sample) {
                    source.process(sample, block as f64, &mut self.output, added);
                    added = true;
                }
            }
        }
        // GainNode uses a per-sample curve only when automation reaches this
        // quantum. Preserve its constant zero/unity shortcuts, including avoiding
        // multiplication at unity, and reuse the fixed gain array for active curves.
        if self.gain.sample_accurate(block, rate) {
            self.gain.a_rate(block, &mut self.gains, rate, x86);
            for c in 0..self.channels {
                for i in 0..FRAMES {
                    self.output[c * FRAMES + i] *= self.gains[i];
                }
            }
        } else {
            let gain = self.gain.k_rate(block, rate);
            if gain == 0.0 {
                self.output.fill(0.0);
            } else if gain != 1.0 {
                for v in &mut self.output[..self.channels * FRAMES] {
                    *v *= gain;
                }
            }
        }
    }
}

// The native output binds from its device callback. Reset the logical bed
// as new() does, retaining all admitted PCM scratch, filters and delay rings.
#[cfg(not(target_arch = "wasm32"))]
impl Stream {
    pub fn reset(&mut self, rate: f64) {
        self.on=false;self.channels=1;self.kind=0;self.spatial=false;
        self.gain=Timeline::new(1.0);
        self.cutoff=Timeline::new(20000.0f64.min(rate/2.0) as f32);
        self.send=Timeline::new(0.0);self.sources=[Source::default();3];
        self.output.fill(0.0);self.direction=[0.0,0.0,-1.0];
        self.positions=[Timeline::new(0.0),Timeline::new(0.0),Timeline::new(-1.0)];
        self.filter.clear();self.side.fill(0.0);
        self.upmix_filter=crate::biquad::Biquad::new(crate::biquad::Kind::Lowpass,7000.0,1.0,0.0,rate);
        for delay in &mut self.delays {delay.fill(0.0);}
        self.delay_at=[0;4];self.gains.fill(0.0);
    }
}
