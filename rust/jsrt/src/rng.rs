//! The game's random generators, transliterated from
//! `src/ai/random.js` and `src/weapons.js`. Draw order is behaviour:
//! the shared AI generator means every draw anywhere moves every
//! later result, so ported code draws through these types, in the
//! order the JavaScript draws, and the replay harness logs draws per
//! phase against them (see `DrawSink`).

use crate::conv::{fround, to_int32, to_uint32};
use crate::hash::fnv1a_mix;
use crate::map::JsMap;

/// Called on every draw when a sink is installed, so a replay can
/// check the draw stream (which generator, how many draws) phase by
/// phase without changing the generators. Production installs none.
pub trait DrawSink {
    /// `stream` names the generator a draw came from ("classic" for a
    /// bare `ClassicRandom`, the stream name inside `RandomStreams`,
    /// "mulberry32" for a press's salvo draws); `value` is the raw
    /// thing drawn (a `next()`'s 32 bits widened, a uniform, ...).
    fn draw(&mut self, stream: &str, value: f64);
}

/// `ClassicRandom` (`src/ai/random.js:22-68`): the 2004 game's
/// generator. One 32-bit state; each draw steps it twice through
/// x' = x * 1664525 + 1013904223 (mod 2^32) and returns the high
/// half of the second step over the high half of the first; the
/// second step is the new state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassicRandom {
    /// `this.state`: the current 32 bits (`seed` reads and writes it).
    pub state: u32,
}

impl Default for ClassicRandom {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl ClassicRandom {
    /// `new ClassicRandom(seed)`: the state is `seed >>> 0`.
    pub fn new(seed: f64) -> Self {
        ClassicRandom { state: to_uint32(seed) }
    }

    /// From a state already in 32 bits (what `new` reduces to).
    pub fn from_state(state: u32) -> Self {
        ClassicRandom { state }
    }

    /// `get seed()`: the state as JavaScript holds it (a double).
    pub fn seed(&self) -> f64 {
        self.state as f64
    }

    /// `set seed(value)`: `value >>> 0`.
    pub fn set_seed(&mut self, value: f64) {
        self.state = to_uint32(value);
    }

    /// `next()`: a raw 32-bit draw, as an unsigned integer (here in
    /// its 32 bits; [`ClassicRandom::next_f64`] is the double the
    /// JavaScript returns).
    pub fn next_u32(&mut self) -> u32 {
        // Math.imul keeps the low 32 bits: wrapping_mul, then the
        // added constant mod 2^32.
        let a = (self.state as i32)
            .wrapping_mul(1664525)
            .wrapping_add(1013904223) as u32;
        let b = (a as i32).wrapping_mul(1664525).wrapping_add(1013904223) as u32;
        self.state = b;
        (b & 0xffff0000) | (a >> 16)
    }

    /// `next()` as the double JavaScript returns.
    pub fn next_f64(&mut self) -> f64 {
        self.next_u32() as f64
    }

    /// `uniform()`: [0, 1) in float32 steps of 2^-23 — the draw's
    /// top 23 bits over 8388608, exact in a double.
    pub fn uniform(&mut self) -> f64 {
        (self.next_u32() >> 9) as f64 / 8388608.0
    }

    /// `int(lo, hi)`: `lo` when `hi <= lo` (no draw), else one draw
    /// and `Math.trunc(f32(f32(u * f32(hi - lo + 1)) + lo))`, with
    /// every `f32()` where the JavaScript rounds.
    pub fn int(&mut self, lo: f64, hi: f64) -> f64 {
        if hi.partial_cmp(&lo) != Some(core::cmp::Ordering::Greater) {
            return lo;
        }
        let u = self.uniform();
        fround(fround(u * fround(hi - lo + 1.0)) + lo).trunc()
    }

    /// `range(lo, hi)`: one draw, then
    /// `f32(f32(lo) + f32(u * f32(hi - lo)))`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        let u = self.uniform();
        fround(fround(lo) + fround(u * fround(hi - lo)))
    }

    /// `mod(n)`: `n > 0 ? this.next() % n : 0` — the `%` is on
    /// doubles, so a fractional `n` behaves as it does there.
    pub fn modulo(&mut self, n: f64) -> f64 {
        if n > 0.0 {
            self.next_f64() % n
        } else {
            0.0
        }
    }
}

/// `RandomStreams` (`src/ai/random.js:74-93`): named independent
/// streams, each a `ClassicRandom` seeded from the match seed and
/// the name by [`fnv1a_mix`], created on first ask. The map is a
/// `JsMap`, so its iteration order (first-ask order) is the
/// JavaScript Map's if anything ever observes it.
#[derive(Clone, Debug)]
pub struct RandomStreams {
    /// `this.seed`: the match seed, `>>> 0`.
    pub seed: u32,
    streams: JsMap<String, ClassicRandom>,
}

impl Default for RandomStreams {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl RandomStreams {
    /// `new RandomStreams(seed)`.
    pub fn new(seed: f64) -> Self {
        RandomStreams {
            seed: to_uint32(seed),
            streams: JsMap::new(),
        }
    }

    /// `get(name)`: the stream for a name, seeding it on first ask.
    pub fn get(&mut self, name: &str) -> &mut ClassicRandom {
        if !self.streams.has(&name.to_string()) {
            let stream = ClassicRandom::from_state(fnv1a_mix(self.seed, name));
            self.streams.set(name.to_string(), stream);
        }
        self.streams.get_mut(&name.to_string()).unwrap()
    }

    /// The stream for a name, if it has been asked for.
    pub fn peek(&self, name: &str) -> Option<&ClassicRandom> {
        self.streams.get(&name.to_string())
    }

    /// `reset(seed)`: keep or replace the match seed and drop every
    /// stream (the next asks reseed from the start).
    pub fn reset(&mut self, seed: Option<f64>) {
        if let Some(seed) = seed {
            self.seed = to_uint32(seed);
        }
        self.streams.clear();
    }

    /// How many streams have been asked for (the Map's `size`).
    pub fn len(&self) -> usize {
        self.streams.size()
    }

    pub fn is_empty(&self) -> bool {
        self.streams.size() == 0
    }

    /// The streams asked for so far, in first-ask order, with their
    /// current states: what a `randomState` digest reads.
    pub fn states(&self) -> Vec<(String, u32)> {
        self.streams
            .entries()
            .into_iter()
            .map(|(name, stream)| (name, stream.state))
            .collect()
    }
}

/// `spreadSeed(unit, channel, press)` (`src/weapons.js:71-78`): a
/// press's spread seed — FNV-1a's fold over the soldier's id, his
/// generation, the channel and the press's id, each `>>> 0` first,
/// with a `h ^= h >>> 15` stir after every multiply. Missing id or
/// generation (`unit?.id ?? 0`) pass 0.
pub fn spread_seed(id: f64, generation: f64, channel: f64, press: f64) -> u32 {
    let mut h: i32 = 0x811c9dc5_u32 as i32;
    for n in [id, generation, channel, press] {
        h = (h ^ to_int32(n)).wrapping_mul(0x01000193);
        h ^= ((h as u32) >> 15) as i32;
    }
    h as u32
}

/// [`spread_seed`] as the double JavaScript returns (`h >>> 0`).
pub fn spread_seed_f64(id: f64, generation: f64, channel: f64, press: f64) -> f64 {
    spread_seed(id, generation, channel, press) as f64
}

/// The generator `seededRandom(seed)` makes (`src/weapons.js:82-90`):
/// mulberry32. State `a` is held as a signed 32-bit value, exactly
/// as the JavaScript's `| 0` keeps it; each call returns a double
/// in [0, 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mulberry32 {
    /// The JavaScript closure's `a`, as its signed 32 bits.
    pub a: i32,
}

impl Mulberry32 {
    /// `seededRandom(seed)`: `a = seed >>> 0` (its bits, read signed
    /// by the first `| 0`).
    pub fn new(seed: f64) -> Self {
        Mulberry32 { a: to_uint32(seed) as i32 }
    }

    /// From a seed already in 32 bits.
    pub fn from_seed_u32(seed: u32) -> Self {
        Mulberry32 { a: seed as i32 }
    }

    /// One call of the returned function: a double in [0, 1) with 32
    /// bits of granularity.
    pub fn next_f64(&mut self) -> f64 {
        self.a = self.a.wrapping_add(0x6d2b79f5);
        let a = self.a;
        let mut t = (a ^ ((a as u32) >> 15) as i32).wrapping_mul(1 | a);
        t = t.wrapping_add((t ^ ((t as u32) >> 7) as i32).wrapping_mul(61 | t)) ^ t;
        ((t ^ ((t as u32) >> 14) as i32) as u32) as f64 / 4294967296.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Values the JavaScript produces (checked against
    // src/ai/random.js and src/weapons.js under Node by the
    // differential harness; these pin the shapes in unit tests).
    #[test]
    fn classic_random_first_draws() {
        let mut r = ClassicRandom::from_state(0);
        // a = 1013904223; b = a*1664525 + 1013904223 mod 2^32.
        let first = r.next_u32();
        assert_eq!(r.state, {
            let a = 1013904223i32;
            a.wrapping_mul(1664525).wrapping_add(1013904223) as u32
        });
        assert_eq!(first & 0xffff, 1013904223u32 >> 16);
        let mut r = ClassicRandom::from_state(12345);
        for _ in 0..1000 {
            let u = r.uniform();
            assert!((0.0..1.0).contains(&u));
            assert_eq!(fround(u), u);
        }
    }

    #[test]
    fn classic_random_int_range_mod_shapes() {
        let mut r = ClassicRandom::from_state(7);
        let state = r.state;
        assert_eq!(r.int(5.0, 5.0), 5.0);
        assert_eq!(r.state, state, "int with hi <= lo must not draw");
        assert_eq!(r.int(5.0, 4.0), 5.0);
        assert_eq!(r.state, state);
        for _ in 0..1000 {
            let v = r.int(2.0, 5.0);
            assert!((2.0..=5.0).contains(&v) && v == v.trunc());
        }
        assert_eq!(r.modulo(0.0), 0.0);
        assert_eq!(r.modulo(-3.0), 0.0);
    }

    #[test]
    fn mulberry32_is_deterministic_and_bounded() {
        let mut a = Mulberry32::from_seed_u32(42);
        let mut b = Mulberry32::from_seed_u32(42);
        for _ in 0..1000 {
            let (x, y) = (a.next_f64(), b.next_f64());
            assert_eq!(x.to_bits(), y.to_bits());
            assert!((0.0..1.0).contains(&x));
        }
    }

    #[test]
    fn random_streams_seed_by_name() {
        let mut streams = RandomStreams::new(99.0);
        let expected = fnv1a_mix(99, "gameplay");
        assert_eq!(streams.get("gameplay").state, expected);
        // Asking again returns the same stream, advanced.
        streams.get("gameplay").next_u32();
        assert_ne!(streams.peek("gameplay").unwrap().state, expected);
        // Reset reseeds from the top.
        streams.reset(None);
        assert!(streams.is_empty());
        assert_eq!(streams.get("gameplay").state, expected);
    }
}
