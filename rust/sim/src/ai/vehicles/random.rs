//! The AI generator A's draws as the vehicle AI makes them (src/ai/random.js
//! ClassicRandom's uniform, int and range over its raw draw), from whatever
//! serves the raw draws: the battle's generator, or a capture's log.

/// A source of generator A's raw draws (ClassicRandom.next), with the
/// game's float32 pickers over them.
pub trait Draws {
    /// A raw 32-bit draw (random.js:28).
    fn next_a(&mut self) -> u32;

    /// [0, 1) in float32 steps (random.js:36).
    fn uniform(&mut self) -> f64 {
        (self.next_a() >> 9) as f64 / 8388608.0
    }

    /// An inclusive integer in [lo, hi] (random.js:41): lo itself, with no
    /// draw, when hi <= lo.
    fn int(&mut self, lo: f64, hi: f64) -> f64 {
        if !(hi > lo) {
            return lo;
        }
        let u = self.uniform();
        v8math::trunc(v8math::fround(v8math::fround(u * v8math::fround(hi - lo + 1.0)) + lo))
    }

    /// A float in [lo, hi) (random.js:49).
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        let u = self.uniform();
        v8math::fround(v8math::fround(lo) + v8math::fround(u * v8math::fround(hi - lo)))
    }
}

/// The battle's generator itself as a source.
impl Draws for crate::ai::random::ClassicRandom {
    fn next_a(&mut self) -> u32 {
        self.next()
    }
}
