//! Deterministic RNG stream for mission scripts, backed by the simulation's
//! `ClassicRandom` generator and seeded from the battle seed.

use std::cell::RefCell;
use std::rc::Rc;

use crate::ai::random::ClassicRandom;
use bfe_script::ScriptRng;

/// FNV-1a over UTF-16 code units folded with the battle seed, matching
/// `crate::ai::random::RandomStreams`.
fn mix_seed(seed: u32, name: &str) -> u32 {
    let mut h = 0x811c_9dc5u32 ^ seed;
    for unit in name.encode_utf16() {
        h ^= unit as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Shared deterministic RNG stream owned jointly by the Lua VM and the script
/// host (so host diagnostics and determinism snapshots can observe its state
/// without taking a draw).
#[derive(Clone, Debug)]
pub struct SimScriptRng {
    inner: Rc<RefCell<ClassicRandom>>,
}

impl SimScriptRng {
    /// Create a script RNG stream deterministically derived from `battle_seed`.
    pub fn from_battle_seed(battle_seed: u32) -> Self {
        Self {
            inner: Rc::new(RefCell::new(ClassicRandom::from_state(mix_seed(
                battle_seed,
                "script",
            )))),
        }
    }

    /// Current 32-bit generator state (for determinism verification).
    pub fn state(&self) -> u32 {
        self.inner.borrow().seed()
    }

    /// Total number of raw draws taken from this stream, stopping at the largest 32-bit number.
    pub fn draw_count(&self) -> u32 {
        u32::try_from(self.inner.borrow().draw_count()).unwrap_or(u32::MAX)
    }
}

impl ScriptRng for SimScriptRng {
    fn seed(&mut self, seed: i32) {
        self.inner.borrow_mut().state = seed as u32;
    }

    fn next_unit(&mut self) -> f32 {
        // Use the top 23 bits -> exact IEEE-754 binary fraction in [0.0, 1.0),
        // matching `ClassicRandom::uniform` bit-for-bit across architectures.
        let raw = self.inner.borrow_mut().next();
        ((raw >> 9) as f32) * (1.0 / 8_388_608.0)
    }
}
