//! The master dynamics (src/audio/worklets/limiter.js): a compressor with
//! a soft knee and makeup, then a true-peak limiter (4x oversampled peak
//! detection, 1.5 ms lookahead, the window's minimum want from a rising
//! queue, its moving average, an exponential release), all channels
//! linked under one gain. The cheap paths are the worklet's own: a block
//! that cannot reach the ceiling skips its oversampled peaks.

use crate::{host, js_max, js_round, Block, BlockMut};

const PHASES: usize = 4;
const TAPS: usize = 12; // per phase: a 48-tap interpolator

fn interpolator() -> [[f32; TAPS]; PHASES] {
    // Windowed-sinc lowpass at the original Nyquist, split into 4 phases.
    let n = PHASES * TAPS;
    let mut h = [0.0f32; PHASES * TAPS];
    let mid = (n as f64 - 1.0) / 2.0;
    for i in 0..n {
        let t = (i as f64 - mid) / PHASES as f64;
        let sinc = if t == 0.0 { 1.0 } else { host::sin(std::f64::consts::PI * t) / (std::f64::consts::PI * t) };
        let w = 0.5 - 0.5 * host::cos((2.0 * std::f64::consts::PI * (i as f64 + 0.5)) / n as f64);
        h[i] = (sinc * w) as f32;
    }
    let mut phases = [[0.0f32; TAPS]; PHASES];
    for p in 0..PHASES {
        for k in 0..TAPS {
            phases[p][k] = h[k * PHASES + p];
        }
    }
    phases
}

/// The dynamics fields in the order one list of them is exchanged:
/// threshold (dB), ratio, knee (dB), attack (s), release (s), makeup
/// (dB), skip (a channel the detector ignores, or -1).
pub const DYNAMICS_COUNT: usize = 7;

pub struct Limiter {
    rate: f64,
    ceiling: f64,
    release: f64,
    look: usize,
    phases: [[f32; TAPS]; PHASES],
    reach: f64,
    channels: usize,
    history: Vec<Vec<f32>>,
    hpos: usize,
    delay: Vec<Vec<f32>>,
    avg: Vec<f32>,
    avg_sum: f64,
    pos: usize,
    gain: f64,
    min_gain: f64,
    rel_coef: f64,
    report: f64,
    queue: Vec<f32>,
    queue_at: Vec<f64>,
    head: usize,
    size: usize,
    clock: u64,
    threshold: f64,
    ratio: f64,
    knee: f64,
    attack_coef: f64,
    release_coef: f64,
    makeup: f64,
    skip: i64,
    quiet: f64,
    comp_gain: f64,
    ms: f64,
    ms_coef: f64,
    scaled: Vec<Vec<f32>>,
    sample_gain: Vec<f64>,
    wants: Vec<f32>,
    gains: Vec<f64>,
    block: usize,
}

impl Limiter {
    /// The worklet's processorOptions: ceiling (dBTP), release (s),
    /// lookahead (s), and the dynamics profile (masked fields set, the
    /// rest at their defaults, as setDynamics has them).
    pub fn new(rate: f64, ceiling: f64, release: f64, lookahead: f64, dynamics: &[f64; DYNAMICS_COUNT], mask: u32) -> Limiter {
        let look = usize::max(js_round(lookahead * rate) as usize, 1);
        let phases = interpolator();
        let mut reach = 1.0;
        for p in 1..PHASES {
            reach = js_max(reach, phases[p].iter().fold(0.0, |s, &h| s + (h as f64).abs()));
        }
        let mut l = Limiter {
            rate,
            ceiling: host::pow(10.0, ceiling / 20.0),
            release,
            look,
            phases,
            reach,
            channels: 0,
            history: Vec::new(),
            hpos: 0,
            delay: Vec::new(),
            avg: vec![1.0; look],
            avg_sum: look as f64,
            pos: 0,
            gain: 1.0,
            min_gain: 1.0,
            rel_coef: 1.0 - host::exp(-1.0 / (release * rate)),
            report: 0.0,
            queue: vec![0.0; look + 1],
            queue_at: vec![0.0; look + 1],
            head: 0,
            size: 0,
            clock: 0,
            threshold: 0.0,
            ratio: 1.0,
            knee: 0.0,
            attack_coef: 0.0,
            release_coef: 0.0,
            makeup: 1.0,
            skip: -1,
            quiet: 0.0,
            comp_gain: 1.0,
            ms: 0.0,
            ms_coef: 1.0 - host::exp(-1.0 / (0.01 * rate)),
            scaled: Vec::new(),
            sample_gain: Vec::new(),
            wants: Vec::new(),
            gains: Vec::new(),
            block: 0,
        };
        l.set_dynamics(dynamics, mask);
        l
    }

    pub fn set_ceiling(&mut self, db: f64) {
        self.ceiling = host::pow(10.0, db / 20.0);
    }

    pub fn set_release(&mut self, release: f64) {
        self.release = release;
        self.rel_coef = 1.0 - host::exp(-1.0 / (self.release * self.rate));
    }

    pub fn set_dynamics(&mut self, d: &[f64; DYNAMICS_COUNT], mask: u32) {
        let take = |i: usize, default: f64| if mask & (1 << i) != 0 { d[i] } else { default };
        let rate = self.rate;
        self.threshold = take(0, 0.0);
        self.ratio = js_max(take(1, 1.0), 1.0);
        self.knee = js_max(take(2, 0.0), 0.0);
        self.attack_coef = 1.0 - host::exp(-1.0 / (js_max(take(3, 0.005), 1e-4) * rate));
        self.release_coef = 1.0 - host::exp(-1.0 / (js_max(take(4, 0.25), 1e-3) * rate));
        self.makeup = host::pow(10.0, take(5, 0.0) / 20.0);
        self.skip = take(6, -1.0) as i64;
        // Mean squares under this are under the knee, where curve() gives 0.
        self.quiet = host::pow(10.0, (self.threshold - self.knee / 2.0) / 10.0) * (1.0 - 1e-9);
    }

    // The compressor's gain for a level in dB (soft knee).
    fn curve(&self, level: f64) -> f64 {
        let t = self.threshold;
        let k = self.knee;
        let slope = 1.0 - 1.0 / self.ratio;
        if self.ratio <= 1.0 {
            return 0.0;
        }
        if level < t - k / 2.0 {
            return 0.0;
        }
        if k > 0.0 && level < t + k / 2.0 {
            let x = level - t + k / 2.0;
            return (-slope * x * x) / (2.0 * k);
        }
        -slope * (level - t)
    }

    fn setup(&mut self, channels: usize) {
        self.channels = channels;
        // Each channel's last TAPS samples, written twice (at p and
        // p + TAPS) so the interpolator reads them in one run.
        self.history = (0..channels).map(|_| vec![0.0; 2 * TAPS]).collect();
        self.hpos = 0;
        self.delay = (0..channels).map(|_| vec![0.0; self.look]).collect();
        self.block = 0;
    }

    fn scratch(&mut self, n: usize) {
        if self.block >= n {
            return;
        }
        self.block = n;
        self.scaled = (0..self.channels).map(|_| vec![0.0; n]).collect();
        self.sample_gain = vec![0.0; n];
        self.wants = vec![0.0; n];
        self.gains = vec![0.0; n];
    }

    /// One block. Returns the gain reduction in dB to report, about ten
    /// times a second, when one is due.
    pub fn process(&mut self, input: &Block, output: &mut BlockMut) -> Option<f64> {
        if input.channels == 0 {
            output.fill(0.0);
            return None;
        }
        if input.channels != self.channels {
            self.setup(input.channels);
        }
        let n = input.frames;
        let channels = self.channels;
        let look = self.look;
        self.scratch(n);

        // The compressor, then the makeup: each sample's gain.
        if self.ratio > 1.0 {
            let skip = self.skip;
            let quiet = self.quiet;
            let ms_coef = self.ms_coef;
            let mut ms = self.ms;
            let mut comp_gain = self.comp_gain;
            for i in 0..n {
                let mut m = 0.0;
                for c in 0..channels {
                    if c as i64 == skip {
                        continue;
                    }
                    let x = input.ch(c)[i] as f64;
                    if x * x > m {
                        m = x * x;
                    }
                }
                ms += (m - ms) * ms_coef;
                let want = if ms + 1e-12 < quiet { 1.0 } else { host::pow(10.0, self.curve(10.0 * host::log10(ms + 1e-12)) / 20.0) };
                comp_gain += (want - comp_gain) * (if want < comp_gain { self.attack_coef } else { self.release_coef });
                self.sample_gain[i] = self.makeup * comp_gain;
            }
            self.ms = ms;
            self.comp_gain = comp_gain;
        } else {
            self.sample_gain[..n].fill(self.makeup);
        }
        // The scaled signal, and its largest sample (with the history's).
        let mut top = 0.0;
        for c in 0..channels {
            for k in 0..TAPS {
                let a = (self.history[c][k] as f64).abs();
                if a > top {
                    top = a;
                }
            }
            let x = input.ch(c);
            for i in 0..n {
                // The worklet's v is the value before the Float32Array
                // store rounds it (an assignment's value in JavaScript),
                // and its largest sample is taken of those.
                let v = x[i] as f64 * self.sample_gain[i];
                self.scaled[c][i] = v as f32;
                let a = v.abs();
                if a > top {
                    top = a;
                }
            }
        }

        // What each sample wants of the limiter: 1 unless its true peak is
        // over the ceiling.
        if top * self.reach * (1.0 + 1e-9) <= self.ceiling {
            // Nothing here can reach the ceiling; only the history moves on.
            self.wants[..n].fill(1.0);
            let from = n.saturating_sub(TAPS);
            for c in 0..channels {
                for i in from..n {
                    let p = (self.hpos + i) % TAPS;
                    let v = self.scaled[c][i];
                    self.history[c][p] = v;
                    self.history[c][p + TAPS] = v;
                }
            }
            self.hpos = (self.hpos + n) % TAPS;
        } else {
            for i in 0..n {
                let mut peak = 0.0;
                let hpos = self.hpos;
                for c in 0..channels {
                    let v = self.scaled[c][i];
                    self.history[c][hpos] = v;
                    self.history[c][hpos + TAPS] = v;
                    let x = (v as f64).abs();
                    if x > peak {
                        peak = x;
                    }
                    for p in 1..PHASES {
                        let taps = self.phases[p];
                        let mut acc = 0.0;
                        for k in 0..TAPS {
                            acc += taps[k] as f64 * self.history[c][hpos + TAPS - k] as f64;
                        }
                        let a = acc.abs();
                        if a > peak {
                            peak = a;
                        }
                    }
                }
                self.hpos = (hpos + 1) % TAPS;
                self.wants[i] = (if peak > self.ceiling { self.ceiling / peak } else { 1.0 }) as f32;
            }
        }

        // The gain: the window's minimum, its moving average, the release.
        let q = self.queue.len();
        let rel_coef = self.rel_coef;
        let mut head = self.head;
        let mut size = self.size;
        let mut clock = self.clock;
        let mut avg_sum = self.avg_sum;
        let mut gain = self.gain;
        let mut min_gain = self.min_gain;
        let mut pos = self.pos;
        for i in 0..n {
            let w = self.wants[i] as f64;
            while size > 0 && self.queue[(head + size - 1) % q] as f64 >= w {
                size -= 1;
            }
            let back = (head + size) % q;
            self.queue[back] = self.wants[i];
            self.queue_at[back] = clock as f64;
            size += 1;
            if self.queue_at[head] <= clock as f64 - look as f64 {
                head = (head + 1) % q;
                size -= 1;
            }
            let m = self.queue[head] as f64;
            clock += 1;
            avg_sum += m - self.avg[pos] as f64;
            self.avg[pos] = m as f32;
            let smooth = avg_sum / look as f64;
            gain = if smooth < gain { smooth } else { gain + (smooth - gain) * rel_coef };
            if gain < min_gain {
                min_gain = gain;
            }
            self.gains[i] = gain;
            pos += 1;
            if pos == look {
                pos = 0;
            }
        }
        // The delayed signal at that gain.
        for c in 0..channels {
            let mut p = self.pos;
            if c < output.channels {
                for i in 0..n {
                    let delayed = self.delay[c][p];
                    self.delay[c][p] = self.scaled[c][i];
                    output.set(c, i, delayed as f64 * self.gains[i]);
                    p += 1;
                    if p == look {
                        p = 0;
                    }
                }
            } else {
                for i in 0..n {
                    self.delay[c][p] = self.scaled[c][i];
                    p += 1;
                    if p == look {
                        p = 0;
                    }
                }
            }
        }
        self.head = head;
        self.size = size;
        self.clock = clock;
        self.avg_sum = avg_sum;
        self.gain = gain;
        self.min_gain = min_gain;
        self.pos = pos;
        // Gain reduction, for the meters, about 10 times a second.
        self.report += n as f64;
        if self.report >= self.rate / 10.0 {
            let reduction = 20.0 * host::log10(self.min_gain);
            self.min_gain = 1.0;
            self.report = 0.0;
            return Some(reduction);
        }
        None
    }
}
