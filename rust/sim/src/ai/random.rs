//! The 2004 game's random numbers, drawn the way the game draws them
//! (src/ai/random.js). This is the `jsrt` crate's generator, ported here
//! until that crate lands; the names and the draw for draw behaviour are
//! the JavaScript's, so moving it later is a rename.
//!
//! A generator is one 32-bit state. Each draw steps it twice through
//! x' = x * 1664525 + 1013904223 (mod 2^32) and returns the high half of the
//! second step over the high half of the first (random.js:22-68).

use std::collections::HashMap;

const MUL: u32 = 1664525;
const ADD: u32 = 1013904223;

/// `ClassicRandom` (random.js:22).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassicRandom {
    pub state: u32,
}

impl ClassicRandom {
    /// random.js:23: the seed as `seed >>> 0` of a JavaScript number.
    pub fn new(seed: f64) -> Self {
        ClassicRandom { state: v8math::to_uint32(seed) }
    }

    /// A generator whose state is already a u32.
    pub fn from_state(state: u32) -> Self {
        ClassicRandom { state }
    }

    /// A raw 32-bit draw, as an unsigned integer (random.js:28).
    pub fn next(&mut self) -> u32 {
        let a = self.state.wrapping_mul(MUL).wrapping_add(ADD);
        let b = a.wrapping_mul(MUL).wrapping_add(ADD);
        self.state = b;
        (b & 0xffff_0000) | (a >> 16)
    }

    /// [0, 1) in float32 steps (random.js:36): `(next >>> 9) / 8388608`,
    /// an exact binary fraction, so the division is exact in f64.
    pub fn uniform(&mut self) -> f64 {
        (self.next() >> 9) as f64 / 8388608.0
    }

    /// An inclusive integer in [lo, hi], with the game's float32 rounding
    /// (random.js:41). The JavaScript's `hi - lo + 1` and `+ lo` are f64
    /// arithmetic on its (number) arguments.
    pub fn int(&mut self, lo: f64, hi: f64) -> f64 {
        if !(hi > lo) {
            return lo;
        }
        let u = self.uniform();
        v8math::trunc(v8math::fround(v8math::fround(u * v8math::fround(hi - lo + 1.0)) + lo))
    }

    /// A float in [lo, hi): lo + U * (hi - lo), in float32 (random.js:49).
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        let u = self.uniform();
        v8math::fround(v8math::fround(lo) + v8math::fround(u * v8math::fround(hi - lo)))
    }

    /// A raw draw modulo n (n > 0), unsigned (random.js:56). The
    /// JavaScript's `%` runs on the draw as an f64; for the counts the
    /// game passes (small whole numbers) that is the integer remainder.
    pub fn modulo(&mut self, n: f64) -> f64 {
        if n > 0.0 {
            self.next() as f64 % n
        } else {
            0.0
        }
    }

    /// The state (random.js:61 `get seed`).
    pub fn seed(&self) -> u32 {
        self.state
    }

    /// random.js:65 `set seed`.
    pub fn set_seed(&mut self, value: f64) {
        self.state = v8math::to_uint32(value);
    }
}

/// Independent seeded streams by name (random.js:74). Iteration order of
/// the JavaScript's Map is observable only through `game.randomState()`,
/// which the wire package owns; the streams themselves are kept in
/// insertion order here too.
#[derive(Clone, Debug)]
pub struct RandomStreams {
    pub seed: u32,
    order: Vec<String>,
    streams: HashMap<String, ClassicRandom>,
}

impl RandomStreams {
    pub fn new(seed: f64) -> Self {
        RandomStreams { seed: v8math::to_uint32(seed), order: Vec::new(), streams: HashMap::new() }
    }

    /// random.js:80. Names are hashed over their UTF-16 code units, as
    /// `name.charCodeAt(i)` reads them.
    pub fn get(&mut self, name: &str) -> &mut ClassicRandom {
        if !self.streams.contains_key(name) {
            let stream = ClassicRandom::from_state(mix(self.seed, name));
            self.order.push(name.to_string());
            self.streams.insert(name.to_string(), stream);
        }
        self.streams.get_mut(name).expect("just inserted")
    }

    /// random.js:89, with its default argument (the current seed) spelled
    /// by the caller.
    pub fn reset(&mut self, seed: f64) {
        self.seed = v8math::to_uint32(seed);
        self.order.clear();
        self.streams.clear();
    }

    /// The stream names in insertion order.
    pub fn names(&self) -> &[String] {
        &self.order
    }
}

/// FNV-1a over the name's UTF-16 units, folded with the seed
/// (random.js:119). `Math.imul` keeps the low 32 bits, as wrapping_mul
/// does; the `^` before it is on the signed view of the same bits.
fn mix(seed: u32, name: &str) -> u32 {
    let mut h = 0x811c_9dc5u32 ^ seed;
    for unit in name.encode_utf16() {
        h ^= unit as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}
