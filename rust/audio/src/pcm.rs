//! Decoded stream PCM: scheduled segment sources and a shared gain timeline.
//! Three source slots cover a segment advance queued just before its end, with exact context-frame starts and stops instead of page timers.

use crate::timeline::Timeline;
use crate::voices::Sample;
use crate::FRAMES;

pub const CHANNELS: usize = 12;
fn frame(time: f64, rate: f64, up: bool) -> f64 {
    let v = (time * rate * 1024.0).round() / 1024.0;
    if up {
        v.ceil()
    } else {
        v.round()
    }
}
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
            pos: if pitch == 1.0 {
                frame(offset, data.rate, false)
            } else {
                offset * data.rate
            },
            start,
            stop: f64::INFINITY,
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
    pub fn stop(&mut self, when: f64, rate: f64) {
        self.stop = frame(when, rate, true);
    }
    pub(crate) fn process(&mut self, data: &Sample, block: f64, out: &mut [f32], add: bool) {
        if !self.on || self.start >= block + FRAMES as f64 {
            return;
        }
        let frames = data.frames;
        if !self.looping && self.pos >= frames as f64 {
            self.on = false;
            return;
        }
        if self.looping && self.pos >= self.end {
            self.pos = 0.0;
        }
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
        {
            let at = self.pos as usize;
            for c in 0..data.channels.min(CHANNELS) {
                let src = &data.data[c * frames + at..c * frames + at + FRAMES];
                let dst = &mut out[c * FRAMES..(c + 1) * FRAMES];
                if add {
                    for i in 0..FRAMES {
                        dst[i] += src[i];
                    }
                } else {
                    dst.copy_from_slice(src);
                }
            }
            self.pos += FRAMES as f64;
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
            let next = if self.looping && (index + 1) as f64 >= self.end {
                ((self.pos + 1.0 - self.end).max(0.0) as usize).min(frames - 1)
            } else {
                (index + 1).min(frames - 1)
            };
            let index = index.min(frames - 1);
            for c in 0..data.channels.min(CHANNELS) {
                let at = c * frames;
                let value = if index == next && index >= 1 {
                    let a = data.data[at + index - 1] as f64;
                    let b = data.data[at + index] as f64;
                    b + (b - a) * fraction
                } else {
                    (1.0 - fraction) * data.data[at + index] as f64
                        + fraction * data.data[at + next] as f64
                };
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
        if self.stop < block + FRAMES as f64 {
            self.on = false;
        }
    }
}

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
            delays: std::array::from_fn(|i| {
                vec![0.0; (rate * [0.012, 0.017, 0.008, 0.011][i]).ceil() as usize + FRAMES + 1]
            }),
            delay_at: [0; 4],
            gains: [0.0; FRAMES],
        }
    }
    pub fn configure(&mut self, channels: usize, kind: u32, spatial: bool) {
        self.on = true;
        self.channels = channels.min(CHANNELS);
        self.kind = kind;
        self.spatial = spatial;
    }
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
