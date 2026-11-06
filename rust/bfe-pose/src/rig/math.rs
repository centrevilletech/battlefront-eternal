//! The transcendental `Math` functions the rigs, poses and hulls call, in
//! one place: `sin`, `cos`, `atan2`, `asin`, `acos`, `atan` and `exp`.
//!
//! The reference is the production server's V8 (13.6, Node 24.22), whose
//! functions are fdlibm ports. Rust's own `f64::sin` and friends are the
//! host's libm, which differs from fdlibm in the last bit on a share of
//! inputs, so nothing that must match the JavaScript may call them: every
//! rig calls the functions here, and these call rust/v8math, which computes
//! each as V8 13.6 does, bit for bit.
//!
//! Under the crate's `replay` feature a test can install an [`oracle`]: what
//! V8 answered for every call the JavaScript made in a recording, keyed by
//! function and argument bits. Each call here is then checked against it:
//! an answer that differs from v8math's is a mismatch, and a call the
//! JavaScript never made with those exact arguments (the arithmetic before
//! it parted from the JavaScript's) is a miss. The value used is always
//! v8math's. The oracle is keyed rather than ordered because three.js
//! computes values the simulation never reads (an Object3D's Euler angles
//! after every quaternion write), which the rigs here compute only when
//! they are read.
//!
//! `sqrt` is not here: IEEE square roots are exact in both, and the rigs
//! call `f64::sqrt` as the JavaScript calls `Math.sqrt`. `Math.hypot` of two
//! numbers is V8's own algorithm, `v8math::hypot`.

/// Whether the values here are V8's to the last bit: rust/v8math's are.
pub const EXACT: bool = true;

/// The functions, as the oracle names them (and the recorders' codes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MathFn {
    Sin = 1,
    Cos = 2,
    Atan2 = 3,
    Asin = 4,
    Acos = 5,
    Atan = 6,
    Exp = 7,
}

impl MathFn {
    pub fn from_code(code: u8) -> Option<MathFn> {
        Some(match code {
            1 => MathFn::Sin,
            2 => MathFn::Cos,
            3 => MathFn::Atan2,
            4 => MathFn::Asin,
            5 => MathFn::Acos,
            6 => MathFn::Atan,
            7 => MathFn::Exp,
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
            MathFn::Atan => "atan",
            MathFn::Exp => "exp",
        }
    }
}

/// v8math's answer, checked against the replay's oracle where one is
/// installed.
#[inline(always)]
fn checked(f: MathFn, a: f64, b: f64, value: f64) -> f64 {
    #[cfg(feature = "replay")]
    oracle::check(f, a, b, value);
    let _ = (f, a, b);
    value
}

/// Math.sin
#[inline(always)]
pub fn sin(x: f64) -> f64 {
    checked(MathFn::Sin, x, 0.0, v8math::sin(x))
}

/// Math.cos
#[inline(always)]
pub fn cos(x: f64) -> f64 {
    checked(MathFn::Cos, x, 0.0, v8math::cos(x))
}

/// Math.atan2(y, x)
#[inline(always)]
pub fn atan2(y: f64, x: f64) -> f64 {
    checked(MathFn::Atan2, y, x, v8math::atan2(y, x))
}

/// Math.asin
#[inline(always)]
pub fn asin(x: f64) -> f64 {
    checked(MathFn::Asin, x, 0.0, v8math::asin(x))
}

/// Math.acos
#[inline(always)]
pub fn acos(x: f64) -> f64 {
    checked(MathFn::Acos, x, 0.0, v8math::acos(x))
}

/// Math.atan
#[inline(always)]
pub fn atan(x: f64) -> f64 {
    checked(MathFn::Atan, x, 0.0, v8math::atan(x))
}

/// Math.exp
#[inline(always)]
pub fn exp(x: f64) -> f64 {
    checked(MathFn::Exp, x, 0.0, v8math::exp(x))
}

/// What V8 answered, for the replays (feature `replay`, native only).
#[cfg(feature = "replay")]
pub mod oracle {
    use super::MathFn;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// The answers a recording holds, by function and argument bits; the
    /// calls checked against them, those it did not answer, and those whose
    /// answer was not v8math's.
    #[derive(Default)]
    pub struct Oracle {
        answers: HashMap<(u8, u64, u64), u64>,
        /// Calls the recording answered (and v8math agreed with).
        pub answered: u64,
        /// Calls it held no answer for: (function, first argument, second).
        pub misses: Vec<(MathFn, f64, f64)>,
        /// Calls whose recorded answer was not v8math's: (function, first
        /// argument, second, V8's answer, v8math's).
        pub mismatches: Vec<(MathFn, f64, f64, f64, f64)>,
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

    /// Checks every call on this thread against `oracle` while `f` runs, and
    /// gives the oracle back (with its misses and mismatches) along with
    /// `f`'s result.
    pub fn with<T>(oracle: Oracle, f: impl FnOnce() -> T) -> (T, Oracle) {
        ORACLE.with(|o| *o.borrow_mut() = Some(oracle));
        let result = f();
        let oracle = ORACLE
            .with(|o| o.borrow_mut().take())
            .expect("the oracle was taken");
        (result, oracle)
    }

    /// Runs `f` with no oracle installed (what it computes is not in the
    /// recording, as the content a battle loads is not), then puts the
    /// oracle back.
    pub fn without<T>(f: impl FnOnce() -> T) -> T {
        let held = ORACLE.with(|o| o.borrow_mut().take());
        let result = f();
        ORACLE.with(|o| *o.borrow_mut() = held);
        result
    }

    /// A call checked against the installed oracle, if there is one.
    pub(super) fn check(f: MathFn, a: f64, b: f64, value: f64) {
        ORACLE.with(|o| {
            let mut o = o.borrow_mut();
            let Some(oracle) = o.as_mut() else { return };
            match oracle.answers.get(&(f as u8, key(a), key(b))) {
                Some(&r) => {
                    let r = f64::from_bits(r);
                    if r.to_bits() == value.to_bits() || (r.is_nan() && value.is_nan()) {
                        oracle.answered += 1;
                    } else {
                        oracle.mismatches.push((f, a, b, r, value));
                    }
                }
                None => oracle.misses.push((f, a, b)),
            }
        })
    }
}
