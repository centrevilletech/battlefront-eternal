//! The enhanced mode's clarity stage (clarity DSP):
//! attacks lifted by a linked pair of envelopes, and a top octave made
//! per channel by rectifying the 2.5 to 7 kHz band and high-passing what
//! comes out, mixed back under the gain. The LFE (skip) passes through.

use crate::{host, js_min, Block, BlockMut};

const SUB: usize = 8; // samples between gain updates

#[derive(Clone, Copy)]
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
    release: f64,
    fast: f64,
    slow: f64,
    gain: f64,
    gains: Vec<f32>,
    power: Vec<f64>,
    // Whether every channel's filters have the same coefficients (they
    // are built alike), so two channels can run in one loop on one set.
    alike: bool,
    // The channels that run the filters this block.
    filtered: Vec<usize>,
    // A pair's filter outputs between stages (filter_two).
    stage: [Vec<f64>; 4],
    // The envelopes' coefficients (up and down, fast then slow), as bits
    // (process).
    envelope_bits: [u64; 4],
}

// One channel's four filters, their coefficients in locals and their
// state worked in place, as the worklet works them.
#[inline(always)]
fn filter_one(band: &mut [Biquad; 4], src: &[f32], dst: &mut [f32], gains: &[f32], excite: f64) {
    let (lb0, lb1, lb2, la1, la2) = (band[0].b0, band[0].b1, band[0].b2, band[0].a1, band[0].a2);
    let (hb0, hb1, hb2, ha1, ha2) = (band[1].b0, band[1].b1, band[1].b2, band[1].a1, band[1].a2);
    let (ob0, ob1, ob2, oa1, oa2) = (band[2].b0, band[2].b1, band[2].b2, band[2].a1, band[2].a2);
    let (mut l1, mut l2, mut h1, mut h2) = (band[0].z1, band[0].z2, band[1].z1, band[1].z2);
    let (mut p1, mut p2, mut q1, mut q2) = (band[2].z1, band[2].z2, band[3].z1, band[3].z2);
    for ((&s, d), &g) in src.iter().zip(dst.iter_mut()).zip(gains) {
        let x = s as f64;
        let a = lb0 * x + l1;
        l1 = lb1 * x - la1 * a + l2;
        l2 = lb2 * x - la2 * a;
        let band_v = hb0 * a + h1;
        h1 = hb1 * a - ha1 * band_v + h2;
        h2 = hb2 * a - ha2 * band_v;
        let r = rectify(band_v);
        let e = ob0 * r + p1;
        p1 = ob1 * r - oa1 * e + p2;
        p2 = ob2 * r - oa2 * e;
        let top = ob0 * e + q1;
        q1 = ob1 * e - oa1 * top + q2;
        q2 = ob2 * e - oa2 * top;
        *d = ((x + excite * top) * g as f64) as f32;
    }
    (band[0].z1, band[0].z2, band[1].z1, band[1].z2) = (l1, l2, h1, h2);
    (band[2].z1, band[2].z2, band[3].z1, band[3].z2) = (p1, p2, q1, q2);
}

// The worklet's `band < 0 ? -band : band`, to the bit (-0 and NaN pass
// as they are), with the branch on what is almost always true (a sample
// is nonzero) rather than on its sign, which the CPU cannot guess.
// (Not `v != 0.0`, which holds for NaN too, whose sign abs would clear.)
#[inline(always)]
#[allow(clippy::double_comparisons)]
fn rectify(v: f64) -> f64 {
    if v < 0.0 || v > 0.0 { v.abs() } else { v }
}

// A sample in, as f64 (an input's f32 widened, or a stage's f64).
trait Sample: Copy {
    fn wide(self) -> f64;
}
impl Sample for f32 {
    #[inline(always)]
    fn wide(self) -> f64 {
        self as f64
    }
}
impl Sample for f64 {
    #[inline(always)]
    fn wide(self) -> f64 {
        self
    }
}

// One of the four filters over a block for two channels at once, on the
// coefficients of `k` and each channel's state (z1, z2): the worklet's
// y = b0 u + z1, z1 = b1 u - a1 y + z2, z2 = b2 u - a2 y, sample by
// sample, rectifying u first for the top's first filter.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn stage_two<T: Sample>(k: &Biquad, za: &mut (f64, f64), zb: &mut (f64, f64), ua: &[T], ub: &[T], ya: &mut [f64], yb: &mut [f64], rect: bool) {
    let (b0, b1, b2, a1, a2) = (k.b0, k.b1, k.b2, k.a1, k.a2);
    let (mut s1, mut s2) = *za;
    let (mut t1, mut t2) = *zb;
    for ((&u, &v), (y, w)) in ua.iter().zip(ub).zip(ya.iter_mut().zip(yb.iter_mut())) {
        let (mut u, mut v) = (u.wide(), v.wide());
        if rect {
            u = rectify(u);
            v = rectify(v);
        }
        let p = b0 * u + s1;
        s1 = b1 * u - a1 * p + s2;
        s2 = b2 * u - a2 * p;
        *y = p;
        let q = b0 * v + t1;
        t1 = b1 * v - a1 * q + t2;
        t2 = b2 * v - a2 * q;
        *w = q;
    }
    *za = (s1, s2);
    *zb = (t1, t2);
}

// Two channels' four filters, a filter at a time over the block (each
// sample meets exactly the operations of filter_one, in its order): one
// filter's two recurrences side by side keep few values live, where all
// four filters of two channels at once would not fit the registers.
#[allow(clippy::too_many_arguments)]
fn filter_two(ba: &mut [Biquad; 4], bb: &mut [Biquad; 4], sa: &[f32], sb: &[f32], da: &mut [f32], db: &mut [f32], gains: &[f32], excite: f64, stage: &mut [Vec<f64>; 4]) {
    let n = sa.len();
    let [s0, s1, s2, s3] = stage;
    let (x0, x1, y0, y1) = (&mut s0[..n], &mut s1[..n], &mut s2[..n], &mut s3[..n]);
    let state = |a: &Biquad, b: &Biquad| ((a.z1, a.z2), (b.z1, b.z2));
    // The band: a high-pass at 2.5 kHz, then a low-pass at the top.
    let (mut za, mut zb) = state(&ba[0], &bb[0]);
    stage_two(&ba[0], &mut za, &mut zb, sa, sb, x0, x1, false);
    (ba[0].z1, ba[0].z2, bb[0].z1, bb[0].z2) = (za.0, za.1, zb.0, zb.1);
    let (mut za, mut zb) = state(&ba[1], &bb[1]);
    stage_two(&ba[1], &mut za, &mut zb, x0, x1, y0, y1, false);
    (ba[1].z1, ba[1].z2, bb[1].z1, bb[1].z2) = (za.0, za.1, zb.0, zb.1);
    // The top: the band rectified, through two high-passes on the first's
    // coefficients.
    let (mut za, mut zb) = state(&ba[2], &bb[2]);
    stage_two(&ba[2], &mut za, &mut zb, y0, y1, x0, x1, true);
    (ba[2].z1, ba[2].z2, bb[2].z1, bb[2].z2) = (za.0, za.1, zb.0, zb.1);
    let (mut za, mut zb) = state(&ba[3], &bb[3]);
    let k = Biquad { z1: 0.0, z2: 0.0, ..ba[2] };
    stage_two(&k, &mut za, &mut zb, x0, x1, y0, y1, false);
    (ba[3].z1, ba[3].z2, bb[3].z1, bb[3].z2) = (za.0, za.1, zb.0, zb.1);
    // Mixed back under the gain.
    for (((&s, &t), (d, e)), ((&ta, &tb), &g)) in sa.iter().zip(sb).zip(da.iter_mut().zip(db.iter_mut())).zip(y0.iter().zip(y1.iter()).zip(gains)) {
        let g = g as f64;
        *d = ((s as f64 + excite * ta) * g) as f32;
        *e = ((t as f64 + excite * tb) * g) as f32;
    }
}

impl Biquad {
    fn coefficients(&self) -> [u64; 5] {
        [self.b0.to_bits(), self.b1.to_bits(), self.b2.to_bits(), self.a1.to_bits(), self.a2.to_bits()]
    }
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
        // filter_one and filter_two take the first two filters' and the
        // third's coefficients (the worklet runs its fourth filter on its
        // third's); channels are alike when all three sets match.
        let alike = |b: &[Biquad; 4]| [b[0].coefficients(), b[1].coefficients(), b[2].coefficients()];
        let bands: Vec<[Biquad; 4]> = bands;
        let shared = bands.first().map(alike);
        let alike = bands.iter().all(|b| Some(alike(b)) == shared);
        let (fast_up, fast_down, slow_up, slow_down) = (coef(0.001), coef(0.04), coef(0.025), coef(0.25));
        Clarity {
            envelope_bits: [fast_up.to_bits(), fast_down.to_bits(), slow_up.to_bits(), slow_down.to_bits()],
            alike,
            filtered: Vec::with_capacity(channels),
            stage: [vec![0.0; 128], vec![0.0; 128], vec![0.0; 128], vec![0.0; 128]],
            skip,
            excite: host::pow(10.0, -12.0 / 20.0),
            max_lift: host::pow(10.0, 4.0 / 20.0),
            knee: host::pow(10.0, 1.0 / 10.0),
            bands,
            release: coef(0.06),
            fast: 0.0,
            slow: 0.0,
            gain: 1.0,
            gains: vec![0.0; crate::FRAMES],
            power: vec![0.0; crate::FRAMES],
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
        let release = self.release;
        // Each envelope's coefficient, chosen by whether the power is over
        // it: noise makes that a coin toss the CPU cannot guess, so the
        // choice is made on the coefficients' bits (an integer select,
        // which the browser makes branch-free; a float select it compiles
        // to a jump), giving exactly the coefficient the worklet's ?: does.
        let [fast_up, fast_down, slow_up, slow_down] = self.envelope_bits;
        for i in 0..n {
            let p = self.power[i] / heard;
            fast += f64::from_bits(std::hint::select_unpredictable(p > fast, fast_up, fast_down)) * (p - fast);
            slow += f64::from_bits(std::hint::select_unpredictable(p > slow, slow_up, slow_down)) * (p - slow);
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
        // Channels without filters first; the others after, two a loop.
        self.filtered.clear();
        for c in 0..output.channels {
            let dst = &mut output.data[c * n..(c + 1) * n];
            if c >= channels {
                dst.fill(0.0);
            } else if c as i64 == skip || c >= self.bands.len() {
                dst.copy_from_slice(&input.ch(c)[..n]);
            } else {
                self.filtered.push(c);
            }
        }
        let gains = &self.gains[..n];
        let excite = self.excite;
        let mut k = 0;
        while k < self.filtered.len() {
            let c0 = self.filtered[k];
            if self.alike && k + 1 < self.filtered.len() {
                let c1 = self.filtered[k + 1];
                let [ba, bb] = self.bands.get_disjoint_mut([c0, c1]).expect("distinct channels");
                let [da, db] = output.data.get_disjoint_mut([c0 * n..(c0 + 1) * n, c1 * n..(c1 + 1) * n]).expect("distinct channels");
                if self.stage[0].len() < n {
                    for buf in &mut self.stage {
                        buf.resize(n, 0.0);
                    }
                }
                filter_two(ba, bb, &input.ch(c0)[..n], &input.ch(c1)[..n], da, db, gains, excite, &mut self.stage);
                k += 2;
            } else {
                filter_one(&mut self.bands[c0], &input.ch(c0)[..n], &mut output.data[c0 * n..(c0 + 1) * n], gains, excite);
                k += 1;
            }
        }
    }
}
