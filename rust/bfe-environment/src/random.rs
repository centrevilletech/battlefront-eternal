//! The destruction effects' random numbers: one stream, as the page's single
//! Math.random was for them. The page seeds it once; the births, their seeds
//! and the cosmetic randomness all draw from it in the order the effects'
//! code asks, so a run repeats from its seed.

use three::Vector3;

pub struct Random {
    state: u32,
}

impl Random {
    pub fn new(seed: u32) -> Random {
        Random { state: seed }
    }

    /// [0, 1): a 32-bit LCG's next state over 2^32.
    pub fn next(&mut self) -> f64 {
        self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
        self.state as f64 / 4294967296.0
    }

    /// lo + (hi - lo) x a draw (rand).
    pub fn rand(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }

    /// A random direction, its up component scaled by `lift` and offset by
    /// `rise` (direction).
    pub fn direction(&mut self, lift: f64, rise: f64) -> Vector3 {
        let mut out = Vector3::default();
        out.random_direction(&mut || self.next());
        out.y = out.y * lift + rise;
        out.normalize();
        out
    }
}
