//! The room reverb (src/audio/worklets/reverb.js): a feedback delay
//! network of 8 lines mixed by a Householder matrix, each line damped by
//! a one-pole lowpass, fed through input diffusers after early
//! reflections and a pre-delay, each output channel reading its own
//! signed mix of the lines. Parameters follow I3DL2 / EAX in linear
//! units and glide over about 50 ms.

use crate::{host, js_max, js_min, js_round, Block, BlockMut};

const LINES: usize = 8;
// Mutually prime lengths in ms at density 1 (scaled down for lower density).
const BASE_MS: [f64; 8] = [29.7, 37.1, 41.1, 43.7, 47.3, 53.1, 59.3, 67.1];
const DIFFUSER_MS: [f64; 4] = [4.77, 3.59, 12.73, 9.31];
const EARLY_MS: [f64; 6] = [0.0, 3.1, 7.9, 11.3, 17.9, 23.3];
const EARLY_GAINS: [f64; 6] = [1.0, 0.78, 0.62, 0.55, 0.43, 0.34];
const MAX_DELAY_S: f64 = 0.4;

/// The parameter order everywhere one list of them is exchanged (the
/// worklet's KEYS): room, roomHF, decay, hfRatio, reflections, reflDelay,
/// reverb, reverbDelay, diffusion, density, hfReference.
pub const PARAM_COUNT: usize = 11;
const DEFAULTS: [f64; PARAM_COUNT] = [0.0, 1.0, 1.5, 0.5, 0.3, 0.01, 1.0, 0.02, 1.0, 1.0, 5000.0];

struct Line {
    buf: Vec<f32>,
    pos: usize,
    len: usize,
    lp: f64,
    g: f64,
    a: f64,
}

struct Delay {
    buf: Vec<f32>,
    pos: usize,
}

pub struct Reverb {
    rate: f64,
    lines: Vec<Line>,
    diffusers: Vec<Delay>,
    pre: Delay,
    early: Delay,
    early_taps: [usize; 6],
    signs: Vec<[f64; LINES]>,
    params: [f64; PARAM_COUNT],
    target: [f64; PARAM_COUNT],
    diffusion: f64,
    pre_len: usize,
    early_len: usize,
    in_a: f64,
    in_lp: f64,
    quiet: u64,
    longest: u64,
    ys: Vec<f64>,
    earlies: Vec<f64>,
    mixes: Vec<f64>,
    values: Vec<f32>,
}

impl Reverb {
    /// `params` and `mask` set the starting parameters as the worklet's
    /// processorOptions.params does (only the masked ones, each finite).
    pub fn new(rate: f64, channels: usize, params: &[f64; PARAM_COUNT], mask: u32) -> Reverb {
        let lines = BASE_MS
            .iter()
            .map(|&ms| Line {
                buf: vec![0.0; ((ms / 1000.0) * rate * 1.6).ceil() as usize + 2],
                pos: 0,
                len: js_round((ms / 1000.0) * rate) as usize,
                lp: 0.0,
                g: 0.0,
                a: 0.0,
            })
            .collect::<Vec<_>>();
        let diffusers = DIFFUSER_MS
            .iter()
            .map(|&ms| Delay { buf: vec![0.0; js_round((ms / 1000.0) * rate) as usize + 1], pos: 0 })
            .collect::<Vec<_>>();
        let pre = Delay { buf: vec![0.0; (MAX_DELAY_S * rate).ceil() as usize + 2], pos: 0 };
        let early = Delay { buf: vec![0.0; (MAX_DELAY_S * rate).ceil() as usize + 2], pos: 0 };
        let longest = [pre.buf.len(), early.buf.len()]
            .into_iter()
            .chain(lines.iter().map(|l| l.buf.len()))
            .chain(diffusers.iter().map(|d| d.buf.len()))
            .max()
            .unwrap_or(0) as u64;
        let signs = (0..channels)
            .map(|c| {
                let mut row = [0.0; LINES];
                for (k, v) in row.iter_mut().enumerate() {
                    *v = (if (c * 7 + k * 3 + ((c * k) % 5)) % 2 != 0 { -1.0 } else { 1.0 }) * (1.0 / (LINES as f64).sqrt());
                }
                row
            })
            .collect();
        let mut r = Reverb {
            rate,
            lines,
            diffusers,
            pre,
            early,
            early_taps: EARLY_MS.map(|ms| js_round((ms / 1000.0) * rate) as usize),
            signs,
            params: DEFAULTS,
            target: DEFAULTS,
            diffusion: 0.0,
            pre_len: 0,
            early_len: 0,
            in_a: 0.0,
            in_lp: 0.0,
            quiet: 0,
            longest,
            ys: Vec::new(),
            earlies: Vec::new(),
            mixes: Vec::new(),
            values: Vec::new(),
        };
        r.set(params, mask);
        r.params = r.target;
        r.apply();
        r
    }

    /// Starting parameters in force at once, as construction puts the
    /// processorOptions ones.
    pub fn place(&mut self, params: &[f64; PARAM_COUNT], mask: u32) {
        self.set(params, mask);
        self.params = self.target;
        self.apply();
    }

    /// New targets (the worklet's port message): only the masked, finite
    /// ones are taken.
    pub fn set(&mut self, params: &[f64; PARAM_COUNT], mask: u32) {
        for k in 0..PARAM_COUNT {
            if mask & (1 << k) != 0 && params[k].is_finite() {
                self.target[k] = params[k];
            }
        }
    }

    // Line gains and damping for the decay time and its HF ratio.
    fn apply(&mut self) {
        let sr = self.rate;
        let p = self.params;
        let density = js_min(js_max(p[9], 0.0), 1.0);
        let scale = 0.6 + 0.4 * density;
        let t60 = js_max(p[2], 0.1);
        let t60hf = js_max(t60 * js_min(js_max(p[3], 0.1), 2.0), 0.05);
        let w = (2.0 * std::f64::consts::PI * js_min(p[10], sr * 0.45)) / sr;
        for line in &mut self.lines {
            line.len = usize::max(js_round(line.buf.len() as f64 / 1.6 * scale) as usize, 8);
            let seconds = line.len as f64 / sr;
            let g0 = host::pow(10.0, (-3.0 * seconds) / t60);
            let gh = host::pow(10.0, (-3.0 * seconds) / t60hf);
            let r = js_min(gh / g0, 1.0);
            let mut a = 0.0;
            if r < 0.9999 {
                let c = host::cos(w);
                let r2 = r * r;
                let big_a = 1.0 - r2;
                let big_b = -2.0 * (1.0 - r2 * c);
                let disc = big_b * big_b - 4.0 * big_a * big_a;
                a = if disc > 0.0 { (-big_b - disc.sqrt()) / (2.0 * big_a) } else { 0.0 };
                a = js_min(js_max(a, 0.0), 0.99);
            }
            line.g = g0;
            line.a = a;
        }
        self.diffusion = 0.5 * js_min(js_max(p[8], 0.0), 1.0) + 0.1;
        self.pre_len = usize::min(js_round(js_max(p[7], 0.0) * sr) as usize, self.pre.buf.len() - 1);
        self.early_len = usize::min(js_round(js_max(p[5], 0.0) * sr) as usize, self.early.buf.len() - 200);
        let rh = js_min(js_max(p[1], 0.001), 1.0);
        self.in_a = if rh >= 0.999 { 0.0 } else { js_min(js_max(1.0 - rh, 0.0), 0.95) };
    }

    pub(crate) fn scratch(&mut self, n: usize) {
        if self.ys.len() >= n {
            return;
        }
        self.ys = vec![0.0; n];
        self.earlies = vec![0.0; n];
        self.mixes = vec![0.0; n];
        self.values = vec![0.0; n * LINES];
    }

    /// One block: the input's channels are averaged to mono, the output
    /// filled for every channel it has.
    pub fn process(&mut self, input: &Block, out: &mut BlockMut) {
        let n = out.frames;
        let mut changed = false;
        for k in 0..PARAM_COUNT {
            let d = self.target[k] - self.params[k];
            if d.abs() > 1e-6 {
                self.params[k] += d * 0.1;
                changed = true;
            }
        }
        if changed {
            self.apply();
        }
        let p = self.params;
        out.fill(0.0);
        let room = p[0];
        let inputs1 = input.channels;
        if self.quiet >= self.longest {
            let mut silent = true;
            for i in 0..n {
                if !silent {
                    break;
                }
                let mut x = 0.0;
                if inputs1 != 0 {
                    for c in 0..inputs1 {
                        x += input.ch(c)[i] as f64;
                    }
                    x /= inputs1 as f64;
                }
                if x * room != 0.0 {
                    silent = false;
                }
            }
            if silent {
                return;
            }
        }
        self.scratch(n);
        let shortest = self.lines.iter().map(|l| l.len).min().unwrap_or(0);
        if shortest < n {
            self.sample_by_sample(input, out);
            return;
        }

        // The input: its high cut, the early reflections, the pre-delay,
        // and the diffusers.
        let keep = 1.0 - self.in_a;
        let elen = self.early.buf.len();
        let off = self.early_taps.map(|t| (self.early_len + t) % elen);
        let reflect = p[4] * 0.4;
        let plen = self.pre.buf.len();
        let pre_len = self.pre_len;
        let big_d = self.diffusion;
        let mut ep = self.early.pos;
        let mut pp = self.pre.pos;
        let mut dp = [self.diffusers[0].pos, self.diffusers[1].pos, self.diffusers[2].pos, self.diffusers[3].pos];
        let mut in_lp = self.in_lp;
        let mut stirred = false;
        // The buffers the loop works in, taken out so they can be borrowed
        // together; put back below.
        let mut ys = std::mem::take(&mut self.ys);
        let mut earlies = std::mem::take(&mut self.earlies);
        for i in 0..n {
            let mut x = 0.0;
            if inputs1 != 0 {
                for c in 0..inputs1 {
                    x += input.ch(c)[i] as f64;
                }
                x /= inputs1 as f64;
            }
            x *= room;
            in_lp += (x - in_lp) * keep;
            x = in_lp;
            if x != 0.0 {
                stirred = true;
            }
            self.early.buf[ep] = x as f32;
            let mut early = 0.0;
            for k in 0..6 {
                let at = (ep + elen - off[k]) % elen;
                // The worklet sums the first tap, then adds the rest.
                if k == 0 {
                    early = self.early.buf[at] as f64 * EARLY_GAINS[0];
                } else {
                    early += self.early.buf[at] as f64 * EARLY_GAINS[k];
                }
            }
            ep += 1;
            if ep == elen {
                ep = 0;
            }
            earlies[i] = early * reflect;
            self.pre.buf[pp] = x as f32;
            let mut y = self.pre.buf[(pp + plen - pre_len) % plen] as f64;
            pp += 1;
            if pp == plen {
                pp = 0;
            }
            for d in 0..4 {
                let buf = &mut self.diffusers[d].buf;
                let delayed = buf[dp[d]] as f64;
                let v = y + big_d * delayed;
                buf[dp[d]] = v as f32;
                y = delayed - big_d * v;
                dp[d] += 1;
                if dp[d] == buf.len() {
                    dp[d] = 0;
                }
                if v != 0.0 {
                    stirred = true;
                }
            }
            ys[i] = y;
        }
        self.in_lp = in_lp;
        for d in 0..4 {
            self.diffusers[d].pos = dp[d];
        }
        self.early.pos = ep;
        self.pre.pos = pp;

        // Each line's damped output over the block.
        let mut values = std::mem::take(&mut self.values);
        for k in 0..LINES {
            let line = &mut self.lines[k];
            let len = line.buf.len();
            let a = line.a;
            let b = 1.0 - a;
            let g = line.g;
            let mut lp = line.lp;
            let mut r = (line.pos + len - line.len) % len;
            for i in 0..n {
                lp = line.buf[r] as f64 * b + lp * a;
                values[i * LINES + k] = (lp * g) as f32;
                r += 1;
                if r == len {
                    r = 0;
                }
            }
            line.lp = lp;
            if lp != 0.0 {
                stirred = true;
            }
        }
        // The Householder mix (I - 2/N 11^T), fed back with the input.
        let mut mixes = std::mem::take(&mut self.mixes);
        for i in 0..n {
            let at = i * LINES;
            let sum = values[at] as f64
                + values[at + 1] as f64
                + values[at + 2] as f64
                + values[at + 3] as f64
                + values[at + 4] as f64
                + values[at + 5] as f64
                + values[at + 6] as f64
                + values[at + 7] as f64;
            mixes[i] = (2.0 / LINES as f64) * sum;
        }
        for k in 0..LINES {
            let line = &mut self.lines[k];
            let len = line.buf.len();
            let mut w = line.pos;
            for i in 0..n {
                let v = values[i * LINES + k] as f64 - mixes[i] + ys[i] * 0.5;
                line.buf[w] = v as f32;
                if v != 0.0 {
                    stirred = true;
                }
                w += 1;
                if w == len {
                    w = 0;
                }
            }
            line.pos = w;
        }
        // Each speaker's own signed mix of the lines, and the early part.
        let reverb = p[6];
        for c in 0..out.channels {
            let s = self.signs[c % self.signs.len()];
            let side = if c % 2 != 0 { -0.5 } else { 0.5 };
            let level = if c < 2 { 1.0 } else { 0.6 };
            for i in 0..n {
                let at = i * LINES;
                let v = values[at] as f64 * s[0]
                    + values[at + 1] as f64 * s[1]
                    + values[at + 2] as f64 * s[2]
                    + values[at + 3] as f64 * s[3]
                    + values[at + 4] as f64 * s[4]
                    + values[at + 5] as f64 * s[5]
                    + values[at + 6] as f64 * s[6]
                    + values[at + 7] as f64 * s[7];
                out.set(c, i, v * reverb + earlies[i] * side * level);
            }
        }
        self.ys = ys;
        self.earlies = earlies;
        self.mixes = mixes;
        self.values = values;
        self.quiet = if stirred { 0 } else { self.quiet + n as u64 };
    }

    // The network one sample at a time, for lines shorter than a block
    // (very low sample rates).
    fn sample_by_sample(&mut self, input: &Block, out: &mut BlockMut) {
        let n = out.frames;
        let p = self.params;
        let early_taps = self.early_taps;
        let mut values = [0.0f32; LINES];
        for i in 0..n {
            let mut x = 0.0;
            if input.channels != 0 {
                for c in 0..input.channels {
                    x += input.ch(c)[i] as f64;
                }
                x /= input.channels as f64;
            }
            x *= p[0];
            self.in_lp += (x - self.in_lp) * (1.0 - self.in_a);
            x = self.in_lp;
            let elen = self.early.buf.len() as i64;
            self.early.buf[self.early.pos] = x as f32;
            let mut early = 0.0;
            for k in 0..6 {
                let idx = (self.early.pos as i64 - self.early_len as i64 - early_taps[k] as i64 + elen * 2) % elen;
                early += self.early.buf[idx as usize] as f64 * EARLY_GAINS[k];
            }
            self.early.pos = (self.early.pos + 1) % self.early.buf.len();
            early *= p[4] * 0.4;
            self.pre.buf[self.pre.pos] = x as f32;
            let plen = self.pre.buf.len();
            let mut y = self.pre.buf[(self.pre.pos + plen - self.pre_len) % plen] as f64;
            self.pre.pos = (self.pre.pos + 1) % plen;
            for d in 0..4 {
                let diff = &mut self.diffusers[d];
                let delayed = diff.buf[diff.pos] as f64;
                let v = y + self.diffusion * delayed;
                diff.buf[diff.pos] = v as f32;
                y = delayed - self.diffusion * v;
                diff.pos = (diff.pos + 1) % diff.buf.len();
            }
            let mut sum = 0.0;
            for k in 0..LINES {
                let line = &mut self.lines[k];
                let len = line.buf.len();
                let read = line.buf[(line.pos + len - line.len) % len] as f64;
                line.lp = read * (1.0 - line.a) + line.lp * line.a;
                values[k] = (line.lp * line.g) as f32;
                sum += values[k] as f64;
            }
            let h = (2.0 / LINES as f64) * sum;
            for k in 0..LINES {
                let line = &mut self.lines[k];
                let len = line.buf.len();
                line.buf[line.pos] = (values[k] as f64 - h + y * 0.5) as f32;
                line.pos = (line.pos + 1) % len;
            }
            for c in 0..out.channels {
                let signs = self.signs[c % self.signs.len()];
                let mut v = 0.0;
                for k in 0..LINES {
                    v += values[k] as f64 * signs[k];
                }
                out.set(c, i, v * p[6] + early * (if c % 2 != 0 { -0.5 } else { 0.5 }) * (if c < 2 { 1.0 } else { 0.6 }));
            }
        }
        self.quiet = 0;
    }
}
