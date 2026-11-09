//! The loudness meter (meter DSP) after ITU-R BS.1770 /
//! EBU R128: K-weighting designed the way libebur128 does (MIT, see
//! THIRD-PARTY-NOTICES.md), channel weights, 100 ms blocks, momentary,
//! short-term and gated integrated loudness, true peak (4x oversampled)
//! and each channel's peak. Readings come out per 100 ms block.

use crate::{host, js_round, Block};

const PHASES: usize = 4;
const TAPS: usize = 12;

struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
}

fn k_weighting(rate: f64) -> [Biquad; 2] {
    let f0 = 1681.974450955533;
    let g = 3.999843853973347;
    let q = 0.7071752369554196;
    let k = host::tan((std::f64::consts::PI * f0) / rate);
    let vh = host::pow(10.0, g / 20.0);
    let vb = host::pow(vh, 0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let pre = Biquad {
        b: [(vh + (vb * k) / q + k * k) / a0, (2.0 * (k * k - vh)) / a0, (vh - (vb * k) / q + k * k) / a0],
        a: [(2.0 * (k * k - 1.0)) / a0, (1.0 - k / q + k * k) / a0],
    };
    let f0 = 38.13547087602444;
    let q = 0.5003270373238773;
    let k = host::tan((std::f64::consts::PI * f0) / rate);
    let a0 = 1.0 + k / q + k * k;
    let rlb = Biquad { b: [1.0, -2.0, 1.0], a: [(2.0 * (k * k - 1.0)) / a0, (1.0 - k / q + k * k) / a0] };
    [pre, rlb]
}

/// One reading, as the worklet posts it (all in LUFS or dB, -inf for
/// silence; channels are each channel's peak in dB).
pub struct Reading {
    pub momentary: f64,
    pub short_term: f64,
    pub integrated: f64,
    pub peak: f64,
    pub true_peak: f64,
    pub channels: [f64; 16],
    pub count: usize,
}

pub struct Meter {
    weights: Option<Vec<f64>>,
    filters: [Biquad; 2],
    block: usize,
    channels: usize,
    blocks: Vec<f64>,
    integrated: Vec<f64>,
    acc: f64,
    count: usize,
    peak: f64,
    true_peak: f64,
    chan_peak: [f64; 16],
    phases: [[f32; TAPS]; PHASES],
    state: [[f64; 8]; 16],
    hist: [[f32; 2 * TAPS]; 16],
    hpos: usize,
    energy: [f64; crate::FRAMES],
    default_weights: [f64; 16],
}

impl Meter {
    /// `weights` per channel, when given (0 drops one, the LFE).
    pub fn new(rate: f64, weights: Option<Vec<f64>>) -> Meter {
        let n = PHASES * TAPS;
        let mut h = [0.0f64; PHASES * TAPS];
        let mid = (n as f64 - 1.0) / 2.0;
        for (i, slot) in h.iter_mut().enumerate().take(n) {
            let t = (i as f64 - mid) / PHASES as f64;
            let sinc = if t == 0.0 { 1.0 } else { host::sin(std::f64::consts::PI * t) / (std::f64::consts::PI * t) };
            *slot = sinc * (0.5 - 0.5 * host::cos((2.0 * std::f64::consts::PI * (i as f64 + 0.5)) / n as f64));
        }
        let mut phases = [[0.0f32; TAPS]; PHASES];
        for p in 0..PHASES {
            for k in 0..TAPS {
                phases[p][k] = h[k * PHASES + p] as f32;
            }
        }
        Meter {
            weights,
            filters: k_weighting(rate),
            block: js_round(rate / 10.0) as usize,
            channels: 0,
            blocks: Vec::with_capacity(31),
            integrated: Vec::with_capacity(36000),
            acc: 0.0,
            count: 0,
            peak: 0.0,
            true_peak: 0.0,
            chan_peak: [0.0; 16],
            phases,
            state: [[0.0; 8]; 16],
            hist: [[0.0; 2 * TAPS]; 16],
            hpos: 0,
            energy: [0.0; crate::FRAMES],
            default_weights: [1.0; 16],
        }
    }

    /// Per-channel weights, as processorOptions gives them (a wrong
    /// length falls back to the defaults at setup, as the worklet's does).
    pub fn set_weights(&mut self, weights: Vec<f64>) {
        self.weights = Some(weights);
    }

    /// The worklet's reset message.
    pub fn reset(&mut self) {
        self.integrated.clear();
        self.blocks.clear();
        self.true_peak = 0.0;
    }

    fn setup(&mut self, channels: usize) {
        self.channels = channels;
        assert!(channels <= 16);
        self.state.fill([0.0; 8]);
        self.hist.fill([0.0; 2 * TAPS]);
        self.hpos = 0;
        self.chan_peak.fill(0.0);
        self.default_weights.fill(1.0);
        if channels >= 6 {
            self.default_weights[3] = 0.0;
            self.default_weights[4..channels].fill(1.41);
        }
    }

    /// Reserve more integrated history between render quanta. The host
    /// requests this with a minute left in the current hour's capacity.
    pub fn reserve(&mut self) { self.integrated.reserve(36000); }
    pub fn remaining(&self) -> usize { self.integrated.capacity() - self.integrated.len() }

    /// A render quantum completes at most one 100 ms meter block.
    pub fn process(&mut self, input: &Block) -> Option<Reading> {
        if input.channels == 0 { return None; }
        assert!(input.frames <= crate::FRAMES);
        if input.channels != self.channels { self.setup(input.channels); }
        let mut reading = None;
        let mut from = 0;
        while from < input.frames {
            let to = usize::min(input.frames, from + self.block - self.count);
            self.run(input, from, to);
            from = to;
            if self.count >= self.block { reading = Some(self.end_block()); }
        }
        reading
    }

    // Samples from..to of every channel: each channel's K-weighted power
    // into the block's sum (in channel order, sample by sample), its peaks.
    fn run(&mut self, input: &Block, from: usize, to: usize) {
        let (pre, rlb) = (&self.filters[0], &self.filters[1]);
        let (pb0, pb1, pb2, pa0, pa1) = (pre.b[0], pre.b[1], pre.b[2], pre.a[0], pre.a[1]);
        let (rb0, rb1, rb2, ra0, ra1) = (rlb.b[0], rlb.b[1], rlb.b[2], rlb.a[0], rlb.a[1]);
        let (p1, p2, p3) = (self.phases[1], self.phases[2], self.phases[3]);
        self.energy[from..to].fill(0.0);
        let mut true_peak = self.true_peak;
        let mut peak = self.peak;
        let h0 = self.hpos;
        for c in 0..self.channels {
            let x = input.ch(c);
            let w = self.weights.as_ref().filter(|w| w.len() == self.channels).map_or(self.default_weights[c], |w| w[c]);
            let s = &mut self.state[c];
            let (mut s0, mut s1, mut s2, mut s3, mut s4, mut s5, mut s6, mut s7) = (s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]);
            let mut chan_peak = self.chan_peak[c];
            let mut hpos = h0;
            for (i, &sample) in x.iter().enumerate().take(to).skip(from) {
                let v = sample as f64;
                // Two biquads, direct form I.
                let y1 = pb0 * v + pb1 * s0 + pb2 * s1 - pa0 * s2 - pa1 * s3;
                s1 = s0;
                s0 = v;
                s3 = s2;
                s2 = y1;
                let y2 = rb0 * y1 + rb1 * s4 + rb2 * s5 - ra0 * s6 - ra1 * s7;
                s5 = s4;
                s4 = y1;
                s7 = s6;
                s6 = y2;
                self.energy[i] += w * y2 * y2;
                let a = v.abs();
                if a > chan_peak {
                    chan_peak = a;
                }
                // True peak.
                self.hist[c][hpos] = x[i];
                self.hist[c][hpos + TAPS] = x[i];
                let mut tp = a;
                let top = hpos + TAPS;
                for taps in [p1, p2, p3] {
                    let mut acc = 0.0;
                    for (k, &tap) in taps.iter().enumerate() {
                        acc += tap as f64 * self.hist[c][top - k] as f64;
                    }
                    if acc.abs() > tp {
                        tp = acc.abs();
                    }
                }
                if tp > true_peak {
                    true_peak = tp;
                }
                if tp > peak {
                    peak = tp;
                }
                hpos += 1;
                if hpos == TAPS {
                    hpos = 0;
                }
            }
            *s = [s0, s1, s2, s3, s4, s5, s6, s7];
            self.chan_peak[c] = chan_peak;
        }
        self.hpos = (h0 + (to - from)) % TAPS;
        self.true_peak = true_peak;
        self.peak = peak;
        let mut acc = self.acc;
        for i in from..to {
            acc += self.energy[i];
        }
        self.acc = acc;
        self.count += to - from;
    }

    fn end_block(&mut self) -> Reading {
        self.blocks.push(self.acc / self.count as f64);
        self.acc = 0.0;
        self.count = 0;
        if self.blocks.len() > 30 {
            self.blocks.remove(0);
        }
        let mean = |k: usize| {
            let list = &self.blocks[self.blocks.len().saturating_sub(k)..];
            if list.is_empty() { 0.0 } else { list.iter().fold(0.0, |s, x| s + x) / list.len() as f64 }
        };
        let lufs = |ms: f64| if ms > 0.0 { -0.691 + 10.0 * host::log10(ms) } else { f64::NEG_INFINITY };
        let momentary_ms = mean(4);
        if self.blocks.len() >= 4 {
            assert!(self.remaining() > 0, "audio: meter history reservation exhausted");
            self.integrated.push(momentary_ms);
        }
        // Integrated: absolute gate at -70 LUFS, then the relative gate 10
        // LU under the mean of what passed.
        // Preserve chronological addition order in both gates, without
        // materializing the passed/gated subsets on the audio thread.
        let mut passed_sum = 0.0;
        let mut passed_count = 0;
        for &ms in &self.integrated { if lufs(ms) > -70.0 { passed_sum += ms; passed_count += 1; } }
        let mut integrated = f64::NEG_INFINITY;
        if passed_count > 0 {
            let rel = lufs(passed_sum / passed_count as f64) - 10.0;
            let mut sum = 0.0;
            let mut count = 0;
            for &ms in &self.integrated { if lufs(ms) > -70.0 && lufs(ms) > rel { sum += ms; count += 1; } }
            if count > 0 { integrated = lufs(sum / count as f64); }
        }
        let mut channels = [f64::NEG_INFINITY; 16];
        for c in 0..self.channels { let p = self.chan_peak[c]; channels[c] = if p > 0.0 { 20.0 * host::log10(p) } else { f64::NEG_INFINITY }; }
        let reading = Reading {
            momentary: lufs(momentary_ms),
            short_term: lufs(mean(30)),
            integrated,
            peak: if self.peak > 0.0 { 20.0 * host::log10(self.peak) } else { f64::NEG_INFINITY },
            true_peak: if self.true_peak > 0.0 { 20.0 * host::log10(self.true_peak) } else { f64::NEG_INFINITY },
            channels,
            count: self.channels,
        };
        self.peak = 0.0;
        self.chan_peak.fill(0.0);
        reading
    }
}
