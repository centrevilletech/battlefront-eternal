//! The transcendental `Math` functions the rigs and poses call, in one
//! place: `sin`, `cos`, `atan2`, `asin` and `acos`.
//!
//! The reference is the production server's V8 (13.6, Node 24.22), whose
//! functions are fdlibm ports. Rust's own `f64::sin` and friends are the
//! host's libm, which differs from fdlibm in the last bit on a share of
//! inputs, so nothing that must match the JavaScript may call them
//! directly: every rig calls the functions here, and only here is the
//! choice of implementation made.
//!
//! Until the V8-exact crate (rust/v8math) is linked, the values below come
//! from the host's libm and are not exact ([`EXACT`] says so). The rigs'
//! differential tests do not depend on that: under the crate's `replay`
//! feature a test installs an [`oracle`] holding what V8 answered for every
//! call the JavaScript made, keyed by function and argument bits, and every
//! call here is answered from it. A call the JavaScript never made with
//! those exact arguments (the arithmetic before it parted from the
//! JavaScript's) is counted as a miss and the test fails on it. The oracle
//! is keyed rather than ordered because three.js computes values the
//! simulation never reads (an Object3D's Euler angles after every quaternion
//! write) and the rigs here compute those only when they are read.
//!
//! `sqrt` is not here: IEEE square roots are exact in both, and the rigs
//! call `f64::sqrt` as the JavaScript calls `Math.sqrt`. `Math.hypot` of two
//! numbers is V8's own algorithm, `crate::movement::soldier::js_hypot2`.

/// Whether the values here are V8's to the last bit. False until
/// rust/v8math is linked: a rig may run on these values, but no flip of
/// the simulation onto Rust may ship on them.
pub const EXACT: bool = false;

/// The functions, as the oracle names them (and the recorder's codes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MathFn {
    Sin = 1,
    Cos = 2,
    Atan2 = 3,
    Asin = 4,
    Acos = 5,
}

impl MathFn {
    pub fn from_code(code: u8) -> Option<MathFn> {
        Some(match code {
            1 => MathFn::Sin,
            2 => MathFn::Cos,
            3 => MathFn::Atan2,
            4 => MathFn::Asin,
            5 => MathFn::Acos,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            MathFn::Sin => "sin",
            MathFn::Cos => "cos",
            MathFn::Atan2 => "atan2",
            MathFn::Asin => "asin",
            MathFn::Acos => "acos",
        }
    }
}

// The provisional values: the host's libm, until rust/v8math is linked.
mod provisional {
    #[inline(always)]
    pub fn sin(x: f64) -> f64 {
        x.sin()
    }
    #[inline(always)]
    pub fn cos(x: f64) -> f64 {
        x.cos()
    }
    #[inline(always)]
    pub fn atan2(y: f64, x: f64) -> f64 {
        y.atan2(x)
    }
    #[inline(always)]
    pub fn asin(x: f64) -> f64 {
        x.asin()
    }
    #[inline(always)]
    pub fn acos(x: f64) -> f64 {
        x.acos()
    }
}

/// Math.sin
#[inline(always)]
pub fn sin(x: f64) -> f64 {
    #[cfg(feature = "replay")]
    if let Some(v) = oracle::answer(MathFn::Sin, x, 0.0) {
        return v;
    }
    provisional::sin(x)
}

/// Math.cos
#[inline(always)]
pub fn cos(x: f64) -> f64 {
    #[cfg(feature = "replay")]
    if let Some(v) = oracle::answer(MathFn::Cos, x, 0.0) {
        return v;
    }
    provisional::cos(x)
}

/// Math.atan2(y, x)
#[inline(always)]
pub fn atan2(y: f64, x: f64) -> f64 {
    #[cfg(feature = "replay")]
    if let Some(v) = oracle::answer(MathFn::Atan2, y, x) {
        return v;
    }
    provisional::atan2(y, x)
}

/// Math.asin
#[inline(always)]
pub fn asin(x: f64) -> f64 {
    #[cfg(feature = "replay")]
    if let Some(v) = oracle::answer(MathFn::Asin, x, 0.0) {
        return v;
    }
    provisional::asin(x)
}

/// Math.acos
#[inline(always)]
pub fn acos(x: f64) -> f64 {
    #[cfg(feature = "replay")]
    if let Some(v) = oracle::answer(MathFn::Acos, x, 0.0) {
        return v;
    }
    provisional::acos(x)
}

/// What V8 answered, for the replays (feature `replay`, native only).
#[cfg(feature = "replay")]
pub mod oracle {
    use super::MathFn;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// The answers a recording holds, by function and argument bits, and
    /// the calls made that it did not answer.
    #[derive(Default)]
    pub struct Oracle {
        answers: HashMap<(u8, u64, u64), u64>,
        /// Calls answered from the recording.
        pub answered: u64,
        /// Calls it held no answer for: (function, first argument, second).
        pub misses: Vec<(MathFn, f64, f64)>,
    }

    impl Oracle {
        pub fn new() -> Oracle {
            Oracle::default()
        }

        /// One answer: `f(a, b) = r` (b is 0 for the one-argument functions).
        pub fn insert(&mut self, f: MathFn, a: f64, b: f64, r: f64) {
            self.answers.insert((f as u8, key(a), key(b)), r.to_bits());
        }

        /// Answers from a recording's flat table: [code, a, b, r] quads.
        pub fn insert_table(&mut self, table: &[f64]) {
            for q in table.chunks_exact(4) {
                if let Some(f) = MathFn::from_code(q[0] as u8) {
                    self.insert(f, q[1], q[2], q[3]);
                }
            }
        }

        pub fn len(&self) -> usize {
            self.answers.len()
        }

        pub fn is_empty(&self) -> bool {
            self.answers.is_empty()
        }
    }

    /// An argument's key: its bits, any NaN as one (JavaScript cannot tell
    /// NaNs apart, and V8 and Rust leave different payloads in them).
    fn key(x: f64) -> u64 {
        if x.is_nan() {
            f64::NAN.to_bits()
        } else {
            x.to_bits()
        }
    }

    thread_local! {
        static ORACLE: RefCell<Option<Oracle>> = const { RefCell::new(None) };
    }

    /// Answers every call on this thread from `oracle` while `f` runs, and
    /// gives the oracle back (with its misses) along with `f`'s result.
    pub fn with<T>(oracle: Oracle, f: impl FnOnce() -> T) -> (T, Oracle) {
        ORACLE.with(|o| *o.borrow_mut() = Some(oracle));
        let result = f();
        let oracle = ORACLE.with(|o| o.borrow_mut().take()).expect("the oracle was taken");
        (result, oracle)
    }

    /// The installed oracle's answer, or None without one. A miss falls
    /// back to the provisional value (and is counted).
    pub(super) fn answer(f: MathFn, a: f64, b: f64) -> Option<f64> {
        ORACLE.with(|o| {
            let mut o = o.borrow_mut();
            let oracle = o.as_mut()?;
            match oracle.answers.get(&(f as u8, key(a), key(b))) {
                Some(&r) => {
                    oracle.answered += 1;
                    Some(f64::from_bits(r))
                }
                None => {
                    oracle.misses.push((f, a, b));
                    None
                }
            }
        })
    }
}
