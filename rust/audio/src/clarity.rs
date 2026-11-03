//! The enhanced mode's clarity stage (src/audio/worklets/clarity.js):
//! attacks lifted by a linked pair of envelopes, and a top octave made
//! per channel by rectifying the 2.5 to 7 kHz band and high-passing what
//! comes out, mixed back under the gain. The LFE (skip) passes through.

use crate::{host, js_min, Block, BlockMut};

const SUB: usize = 8; // samples between gain updates

struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn new(highpass: bool, f0: f64, q: f64, rate: f64) -> Biquad {
        let w = (2.0 * std::f64::consts::PI * f0) / rate;
        let c = host::cos(w);
        let alpha = host::sin(w) / (2.0 * q);
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = if highpass {
            ((1.0 + c) / 2.0 / a0, -(1.0 + c) / a0, (1.0 + c) / 2.0 / a0)
        } else {
            ((1.0 - c) / 2.0 / a0, (1.0 - c) / a0, (1.0 - c) / 2.0 / a0)
        };
        Biquad { b0, b1, b2, a1: (-2.0 * c) / a0, a2: (1.0 - alpha) / a0, z1: 0.0, z2: 0.0 }
    }
}

pub struct Clarity {
    skip: i64,
    excite: f64,
    max_lift: f64,
    knee: f64,
    // Per channel: the band's high-pass and low-pass, the top's two
    // high-passes (which share their coefficients).
    bands: Vec<[Biquad; 4]>,
    fast_up: f64,
    fast_down: f64,
    slow_up: f64,
    slow_down: f64,
    release: f64,
    fast: f64,
    slow: f64,
    gain: f64,
    gains: Vec<f32>,
    power: Vec<f64>,
}

impl Clarity {
    pub fn new(rate: f64, channels: usize, skip: i64) -> Clarity {
        let q = std::f64::consts::FRAC_1_SQRT_2;
        let top = js_min(7000.0, rate * 0.45);
        let bands = (0..channels)
            .map(|_| [
                Biquad::new(true, 2500.0, q, rate),
                Biquad::new(false, top, q, rate),
                Biquad::new(true, top, q, rate),
                Biquad::new(true, top, q, rate),
            ])
            .collect();
        let coef = |s: f64| 1.0 - host::exp(-1.0 / (s * rate));
        Clarity {
            skip,
            excite: host::pow(10.0, -12.0 / 20.0),
            max_lift: host::pow(10.0, 4.0 / 20.0),
            knee: host::pow(10.0, 1.0 / 10.0),
            bands,
            fast_up: coef(0.001),
            fast_down: coef(0.04),
            slow_up: coef(0.025),
            slow_down: coef(0.25),
            release: coef(0.06),
            fast: 0.0,
            slow: 0.0,
            gain: 1.0,
            gains: Vec::new(),
            power: Vec::new(),
        }
    }

    /// One block. An empty input (no channels) silences the output.
    pub fn process(&mut self, input: &Block, output: &mut BlockMut) {
        if input.channels == 0 {
            output.fill(0.0);
            return;
        }
        let n = output.frames;
        if self.gains.len() < n {
            self.gains = vec![0.0; n];
            self.power = vec![0.0; n];
        }
        let channels = usize::min(input.channels, output.channels);
        let skip = self.skip;
        let heard = i64::max(channels as i64 - (if skip >= 0 && skip < channels as i64 { 1 } else { 0 }), 1) as f64;
        // The mix's power per sample, a channel at a time.
        self.power[..n].fill(0.0);
        for c in 0..channels {
            if c as i64 == skip {
                continue;
            }
            let src = input.ch(c);
            for i in 0..n {
                let x = src[i] as f64;
                self.power[i] += x * x;
            }
        }
        // The attack gain, per sample. The worklet keeps the gains in a
        // Float32Array: each store rounds, as here.
        let (mut fast, mut slow, mut gain) = (self.fast, self.slow, self.gain);
        let mut goal = gain;
        let (fast_up, fast_down, slow_up, slow_down, release) = (self.fast_up, self.fast_down, self.slow_up, self.slow_down, self.release);
        for i in 0..n {
            let p = self.power[i] / heard;
            fast += (if p > fast { fast_up } else { fast_down }) * (p - fast);
            slow += (if p > slow { slow_up } else { slow_down }) * (p - slow);
            if i % SUB == 0 {
                let ratio = (fast + 1e-12) / (slow + 1e-12);
                goal = if ratio > self.knee { js_min(host::pow(ratio / self.knee, 0.25), self.max_lift) } else { 1.0 };
            }
            gain = if goal > gain { goal } else { gain + release * (goal - gain) };
            self.gains[i] = gain as f32;
        }
        self.fast = fast;
        self.slow = slow;
        self.gain = gain;
        // Each channel: the gain, and the top octave (the four filters
        // worked in place, their state in locals, as the worklet's are).
        for c in 0..output.channels {
            if c >= channels {
                for i in 0..n {
                    output.set(c, i, 0.0);
                }
                continue;
            }
            let src = input.ch(c);
            if c as i64 == skip {
                for i in 0..n {
                    output.set(c, i, src[i] as f64);
                }
                continue;
            }
            if c >= self.bands.len() {
                for i in 0..n {
                    output.set(c, i, src[i] as f64);
                }
                continue;
            }
            let band = &mut self.bands[c];
            let (lb0, lb1, lb2, la1, la2) = (band[0].b0, band[0].b1, band[0].b2, band[0].a1, band[0].a2);
            let (hb0, hb1, hb2, ha1, ha2) = (band[1].b0, band[1].b1, band[1].b2, band[1].a1, band[1].a2);
            let (ob0, ob1, ob2, oa1, oa2) = (band[2].b0, band[2].b1, band[2].b2, band[2].a1, band[2].a2);
            let (mut l1, mut l2, mut h1, mut h2) = (band[0].z1, band[0].z2, band[1].z1, band[1].z2);
            let (mut p1, mut p2, mut q1, mut q2) = (band[2].z1, band[2].z2, band[3].z1, band[3].z2);
            for i in 0..n {
                let x = src[i] as f64;
                let a = lb0 * x + l1;
                l1 = lb1 * x - la1 * a + l2;
                l2 = lb2 * x - la2 * a;
                let band_v = hb0 * a + h1;
                h1 = hb1 * a - ha1 * band_v + h2;
                h2 = hb2 * a - ha2 * band_v;
                let r = if band_v < 0.0 { -band_v } else { band_v };
                let e = ob0 * r + p1;
                p1 = ob1 * r - oa1 * e + p2;
                p2 = ob2 * r - oa2 * e;
                let top = ob0 * e + q1;
                q1 = ob1 * e - oa1 * top + q2;
                q2 = ob2 * e - oa2 * top;
                output.set(c, i, (x + self.excite * top) * self.gains[i] as f64);
            }
            band[0].z1 = l1;
            band[0].z2 = l2;
            band[1].z1 = h1;
            band[1].z2 = h2;
            band[2].z1 = p1;
            band[2].z2 = p2;
            band[3].z1 = q1;
            band[3].z2 = q2;
        }
    }
}
