//! OpenAL Soft's front stabilizer (src/audio/worklets/stabilizer.js),
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
        for i in 0..n {
            let xi = x[i] as f64;
            let y = xi * k + z1;
            z1 = xi - y * k;
            x[i] = y as f32;
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
        let left_in: Vec<f32> = {
            let s = chan(self.left);
            if s.is_empty() { vec![0.0; n] } else { s.to_vec() }
        };
        let right_in: Vec<f32> = {
            let s = chan(self.right);
            if s.is_empty() { vec![0.0; n] } else { s.to_vec() }
        };
        self.split_front(&left_in, 0);
        self.split_front(&right_in, 1);
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
