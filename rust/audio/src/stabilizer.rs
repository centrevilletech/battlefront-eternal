//! OpenAL Soft's front stabilizer (stabilizer DSP),
//! derived there from OpenAL Soft 1.20.1 (LGPL-2.0-or-later, distributed
//! here under GPL-3.0-only; see THIRD-PARTY-NOTICES.md). The front pair is
//! split at 5 kHz by a phase-corrected band splitter, 256 samples late;
//! the lows' sum goes a third of the way to the centre, the highs' a
//! quarter; every other channel is delayed to stay in time.

use crate::{host, Block, BlockMut};

const DELAY: usize = 256;
const MAX_BLOCK: usize = 128;
const CROSSOVER: f64 = 5000.0;

// OpenAL Soft's band splitter: a one-pole all-pass, and a low-pass of two
// one-pole sections sharing its coefficient; the high band is the
// all-pass less the low.
struct Splitter {
    coeff: f64,
    lp1: f64,
    lp2: f64,
    ap1: f64,
}

impl Splitter {
    fn new(rate: f64) -> Splitter {
        let w = (CROSSOVER / rate) * 2.0 * std::f64::consts::PI;
        let cw = host::cos(w);
        Splitter {
            coeff: if cw > 1.1920929e-7 { (host::sin(w) - 1.0) / cw } else { cw * -0.5 },
            lp1: 0.0,
            lp2: 0.0,
            ap1: 0.0,
        }
    }

    // An all-pass over the first n samples of `x` in place, from rest. The
    // worklet's x is a Float32Array: each store rounds, as here.
    fn allpass(&self, x: &mut [f32], n: usize) {
        let k = self.coeff;
        let mut z1 = 0.0;
        for sample in &mut x[..n] {
            let xi = *sample as f64;
            let y = xi * k + z1;
            z1 = xi - y * k;
            *sample = y as f32;
        }
    }

    // The first n samples of x into high and low; x is read from its end
    // (at - i), backwards.
    fn split(&mut self, x: &[f32], at: usize, n: usize, high: &mut [f32], low: &mut [f32]) {
        let ap = self.coeff;
        let lpc = ap * 0.5 + 0.5;
        let (mut lp1, mut lp2, mut ap1) = (self.lp1, self.lp2, self.ap1);
        for i in 0..n {
            let input = x[at - i] as f64;
            let mut d = (input - lp1) * lpc;
            let mut y = lp1 + d;
            lp1 = y + d;
            d = (y - lp2) * lpc;
            y = lp2 + d;
            lp2 = y + d;
            low[i] = y as f32;
            let a = input * ap + ap1;
            ap1 = input - a * ap;
            high[i] = (a - y) as f32;
        }
        self.lp1 = lp1;
        self.lp2 = lp2;
        self.ap1 = ap1;
    }
}

pub struct Stabilizer {
    channels: usize,
    left: usize,
    right: usize,
    centre: usize,
    low_cos: f64,
    low_sin: f64,
    high_cos: f64,
    high_sin: f64,
    history: Vec<[f32; DELAY]>,
    ring: Vec<[f32; DELAY]>,
    pos: usize,
    filters: [Splitter; 2],
    tmp: Vec<f32>,
    bands: [[Vec<f32>; 2]; 2], // per side: [high, low]
    front: [Vec<f32>; 2],
    // The paired path's block and history per side, newest first.
    pair_tmp: [Vec<f32>; 2],
}

impl Stabilizer {
    pub fn new(rate: f64, channels: usize, left: usize, right: usize, centre: usize) -> Stabilizer {
        Stabilizer {
            channels,
            left,
            right,
            centre,
            low_cos: host::cos(std::f64::consts::PI / 6.0),
            low_sin: host::sin(std::f64::consts::PI / 6.0),
            high_cos: host::cos(std::f64::consts::PI / 8.0),
            high_sin: host::sin(std::f64::consts::PI / 8.0),
            history: vec![[0.0; DELAY]; channels],
            ring: vec![[0.0; DELAY]; channels],
            pos: 0,
            filters: [Splitter::new(rate), Splitter::new(rate)],
            tmp: vec![0.0; MAX_BLOCK + DELAY],
            bands: [[vec![0.0; MAX_BLOCK], vec![0.0; MAX_BLOCK]], [vec![0.0; MAX_BLOCK], vec![0.0; MAX_BLOCK]]],
            front: [vec![0.0; MAX_BLOCK], vec![0.0; MAX_BLOCK]],
            pair_tmp: [vec![0.0; MAX_BLOCK + DELAY], vec![0.0; MAX_BLOCK + DELAY]],
        }
    }

    // One front channel through the phase-corrected splitter.
    fn split_front(&mut self, input: &[f32], side: usize) {
        let n = input.len();
        if self.tmp.len() < n + DELAY {
            self.tmp.resize(n + DELAY, 0.0);
            for b in &mut self.bands {
                b[0].resize(n, 0.0);
                b[1].resize(n, 0.0);
            }
        }
        let channel = if side == 0 { self.left } else { self.right };
        // The block newest-first, then the history; the block's start (its
        // newest samples) becomes the new history, as the worklet's does
        // (copied before the all-pass runs over it).
        for i in 0..n {
            self.tmp[i] = input[n - 1 - i];
        }
        self.tmp[n..n + DELAY].copy_from_slice(&self.history[channel]);
        self.history[channel].copy_from_slice(&self.tmp[..DELAY]);
        self.filters[side].allpass(&mut self.tmp, n + DELAY);
        let tmp = std::mem::take(&mut self.tmp);
        let (high, low) = self.bands[side].split_at_mut(1);
        let (high, low) = (&mut high[0], &mut low[0]);
        self.filters[side].split(&tmp, n + DELAY - 1, n, high, low);
        self.tmp = tmp;
    }

    /// One block. Channels past the input's count read as silence, as the
    /// worklet's do; the output must have the channels it was built with.
    pub fn process(&mut self, input: &Block, output: &mut BlockMut) {
        let (l, r, c) = (self.left, self.right, self.centre);
        if l != r && l != c && r != c && l < output.channels && r < output.channels && l < self.history.len() && r < self.history.len() {
            self.process_paired(input, output);
        } else {
            self.process_each(input, output);
        }
    }

    /// The usual layouts (left, right and centre all different): the
    /// same operations as process_each, the two front channels' filters
    /// run side by side in one loop (each filter's sums are its own, so
    /// nothing changes but how long the recurrences keep the CPU waiting),
    /// and the delay lines moved as runs.
    fn process_paired(&mut self, input: &Block, output: &mut BlockMut) {
        let n = output.frames;
        let empty: &[f32] = &[];
        let chan = |c: usize| -> &[f32] { if c < input.channels { input.ch(c) } else { empty } };
        // The other channels: 256 samples late.
        for c in 0..output.channels {
            if c == self.left || c == self.right {
                continue;
            }
            let src = chan(c);
            let ring = &mut self.ring[c];
            let dst = &mut output.data[c * n..(c + 1) * n];
            let (mut p, mut i) = (self.pos, 0);
            while i < n {
                let run = usize::min(n - i, DELAY - p);
                dst[i..i + run].copy_from_slice(&ring[p..p + run]);
                if src.is_empty() {
                    ring[p..p + run].fill(0.0);
                } else {
                    ring[p..p + run].copy_from_slice(&src[i..i + run]);
                }
                i += run;
                p += run;
                if p == DELAY {
                    p = 0;
                }
            }
        }
        self.pos = (self.pos + n) % DELAY;
        // Each front channel's block newest first, then its history; the
        // block's start (its newest samples) becomes the new history,
        // copied before the all-pass runs over it.
        let len = n + DELAY;
        for side in 0..2 {
            if self.pair_tmp[side].len() < len {
                self.pair_tmp[side].resize(len, 0.0);
            }
            if self.bands[side][0].len() < n {
                self.bands[side][0].resize(n, 0.0);
                self.bands[side][1].resize(n, 0.0);
            }
            let channel = if side == 0 { self.left } else { self.right };
            let tmp = &mut self.pair_tmp[side];
            let src = chan(channel);
            if src.is_empty() {
                tmp[..n].fill(0.0);
            } else {
                for (t, &x) in tmp[..n].iter_mut().zip(src[..n].iter().rev()) {
                    *t = x;
                }
            }
            tmp[n..len].copy_from_slice(&self.history[channel]);
            self.history[channel].copy_from_slice(&tmp[..DELAY]);
        }
        // Both all-passes, from rest.
        let [t0, t1] = &mut self.pair_tmp;
        let (t0, t1) = (&mut t0[..len], &mut t1[..len]);
        let (k0, k1) = (self.filters[0].coeff, self.filters[1].coeff);
        let (mut z0, mut z1) = (0.0f64, 0.0f64);
        for (a, b) in t0.iter_mut().zip(t1.iter_mut()) {
            let x = *a as f64;
            let y = x * k0 + z0;
            z0 = x - y * k0;
            *a = y as f32;
            let x = *b as f64;
            let y = x * k1 + z1;
            z1 = x - y * k1;
            *b = y as f32;
        }
        // Both splitters, reading oldest first (from the end).
        {
            let [f0, f1] = &mut self.filters;
            let [[h0, l0], [h1, l1]] = &mut self.bands;
            let (ap0, ap1c) = (f0.coeff, f1.coeff);
            let (lpc0, lpc1) = (ap0 * 0.5 + 0.5, ap1c * 0.5 + 0.5);
            let (mut a_lp1, mut a_lp2, mut a_ap1) = (f0.lp1, f0.lp2, f0.ap1);
            let (mut b_lp1, mut b_lp2, mut b_ap1) = (f1.lp1, f1.lp2, f1.ap1);
            let ins = t0[DELAY..len].iter().rev().zip(t1[DELAY..len].iter().rev());
            let outs = h0[..n].iter_mut().zip(l0[..n].iter_mut()).zip(h1[..n].iter_mut().zip(l1[..n].iter_mut()));
            for ((&xa, &xb), ((ha, la), (hb, lb))) in ins.zip(outs) {
                let input = xa as f64;
                let mut d = (input - a_lp1) * lpc0;
                let mut y = a_lp1 + d;
                a_lp1 = y + d;
                d = (y - a_lp2) * lpc0;
                y = a_lp2 + d;
                a_lp2 = y + d;
                *la = y as f32;
                let a = input * ap0 + a_ap1;
                a_ap1 = input - a * ap0;
                *ha = (a - y) as f32;
                let input = xb as f64;
                let mut d = (input - b_lp1) * lpc1;
                let mut y = b_lp1 + d;
                b_lp1 = y + d;
                d = (y - b_lp2) * lpc1;
                y = b_lp2 + d;
                b_lp2 = y + d;
                *lb = y as f32;
                let a = input * ap1c + b_ap1;
                b_ap1 = input - a * ap1c;
                *hb = (a - y) as f32;
            }
            (f0.lp1, f0.lp2, f0.ap1) = (a_lp1, a_lp2, a_ap1);
            (f1.lp1, f1.lp2, f1.ap1) = (b_lp1, b_lp2, b_ap1);
        }
        // The bands back onto the front pair, and the centre's share.
        let [[lh, ll], [rh, rl]] = &self.bands;
        let (low_cos, high_cos, low_sin, high_sin) = (self.low_cos, self.high_cos, self.low_sin, self.high_sin);
        let bands = lh[..n].iter().zip(ll[..n].iter()).zip(rh[..n].iter().zip(rl[..n].iter()));
        let (l, r, c) = (self.left, self.right, self.centre);
        if c < output.channels {
            let [left, right, centre] = output.data.get_disjoint_mut([l * n..(l + 1) * n, r * n..(r + 1) * n, c * n..(c + 1) * n]).expect("distinct channels");
            for (((&lh, &ll), (&rh, &rl)), ((dl, dr), dc)) in bands.zip(left.iter_mut().zip(right.iter_mut()).zip(centre.iter_mut())) {
                let (lh, ll, rh, rl) = (lh as f64, ll as f64, rh as f64, rl as f64);
                let low = ll + rl;
                let high = lh + rh;
                let s = ll + lh - rl - rh;
                let m = low * low_cos + high * high_cos;
                let cv = low * low_sin + high * high_sin;
                *dl = ((m + s) * 0.5) as f32;
                *dr = ((m - s) * 0.5) as f32;
                *dc = (*dc as f64 + cv * 0.5) as f32;
            }
        } else {
            let [left, right] = output.data.get_disjoint_mut([l * n..(l + 1) * n, r * n..(r + 1) * n]).expect("distinct channels");
            for (((&lh, &ll), (&rh, &rl)), (dl, dr)) in bands.zip(left.iter_mut().zip(right.iter_mut())) {
                let (lh, ll, rh, rl) = (lh as f64, ll as f64, rh as f64, rl as f64);
                let low = ll + rl;
                let high = lh + rh;
                let s = ll + lh - rl - rh;
                let m = low * low_cos + high * high_cos;
                *dl = ((m + s) * 0.5) as f32;
                *dr = ((m - s) * 0.5) as f32;
            }
        }
    }

    /// Any layout, one front channel after the other (the worklet's own
    /// order, which matters only when two of its indices are the same).
    fn process_each(&mut self, input: &Block, output: &mut BlockMut) {
        let n = output.frames;
        let chan = |c: usize| -> &[f32] {
            if c < input.channels {
                input.ch(c)
            } else {
                &[]
            }
        };
        // The other channels: 256 samples late.
        for c in 0..output.channels {
            if c == self.left || c == self.right {
                continue;
            }
            let src = chan(c);
            let mut p = self.pos;
            for i in 0..n {
                let delayed = self.ring[c][p];
                self.ring[c][p] = if src.is_empty() { 0.0 } else { src[i] };
                output.set(c, i, delayed as f64);
                p += 1;
                if p == DELAY {
                    p = 0;
                }
            }
        }
        self.pos = (self.pos + n) % DELAY;
        for (side, ch) in [(0usize, self.left), (1usize, self.right)] {
            if self.front[side].len() < n {
                self.front[side].resize(n, 0.0);
            }
            let s = chan(ch);
            if s.is_empty() {
                self.front[side][..n].fill(0.0);
            } else {
                self.front[side][..n].copy_from_slice(&s[..n]);
            }
        }
        let left_in = std::mem::take(&mut self.front[0]);
        self.split_front(&left_in[..n], 0);
        self.front[0] = left_in;
        let right_in = std::mem::take(&mut self.front[1]);
        self.split_front(&right_in[..n], 1);
        self.front[1] = right_in;
        let has_centre = self.centre < output.channels;
        for i in 0..n {
            let (lh, ll) = (self.bands[0][0][i] as f64, self.bands[0][1][i] as f64);
            let (rh, rl) = (self.bands[1][0][i] as f64, self.bands[1][1][i] as f64);
            let low = ll + rl;
            let high = lh + rh;
            let s = ll + lh - rl - rh;
            let m = low * self.low_cos + high * self.high_cos;
            let c = low * self.low_sin + high * self.high_sin;
            output.set(self.left, i, (m + s) * 0.5);
            output.set(self.right, i, (m - s) * 0.5);
            if has_centre {
                let at = output.at(self.centre, i);
                output.set(self.centre, i, at + c * 0.5);
            }
        }
        let _ = self.channels;
    }
}
