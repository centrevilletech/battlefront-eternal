//! The seam to `rust/v8math`, the crate that ports V8 13.6's own
//! transcendental `Math` functions (workstream W13, in flight on its
//! own branch). Every transcendental the ported sim needs goes
//! through the functions named here and nowhere else: never Rust's
//! `f64::sin` and friends, never values taken from a host's `Math`
//! (the client and the server run different V8s and already disagree
//! in the last bit today).
//!
//! Until `rust/v8math` lands, the default `v8math-stub` feature fills
//! these in with the host libm's results. The stub exists so callers,
//! operation order and tests can be written now. Its values are NOT
//! V8's: host libm differs from V8's fdlibm in the last bit on a
//! sizable share of inputs, so differential gates over code that
//! reaches these functions report those differences separately and
//! no sim flip may ship on the stub. When W13 lands, this module
//! becomes a re-export of that crate and the feature goes away.
//!
//! What P1 needs from `rust/v8math` (the full list is also in the
//! package notes): `sin`, `cos`, `tan`, `asin`, `acos`, `atan`,
//! `atan2`, `sinh`, `cosh`, `tanh`, `asinh`, `acosh`, `atanh`, `exp`,
//! `expm1`, `log`, `log1p`, `log2`, `log10`, `cbrt`, `pow`, and the
//! general `hypot` (today proven for the arities the game uses, in
//! [`crate::conv`]). The three.js r170 subset in `rust/three`
//! reaches: `sin`, `cos`, `asin`, `acos`, `atan2`, `exp` (MathUtils
//! damp), `pow` (sRGB helpers are presentation, excluded), `log`
//! (none in the math subset today). Everything else the sim calls at
//! its own sites goes through the same names.

/// True while the stub fills this module in. Sim code may use this to
/// refuse to run a bit-exact gate on stub values; it must never use
/// it to change behaviour.
#[cfg(feature = "v8math-stub")]
pub const IS_STUB: bool = true;

#[cfg(not(feature = "v8math-stub"))]
pub const IS_STUB: bool = false;

macro_rules! stub1 {
    ($name:ident, $js:literal) => {
        stub1!($name, $name, $js);
    };
    ($name:ident, $method:ident, $js:literal) => {
        #[doc = $js]
        #[cfg(feature = "v8math-stub")]
        #[inline(always)]
        pub fn $name(x: f64) -> f64 {
            x.$method()
        }

        #[doc = $js]
        #[cfg(not(feature = "v8math-stub"))]
        #[inline(always)]
        pub fn $name(x: f64) -> f64 {
            // Replaced by rust/v8math's port when that crate lands;
            // without the stub feature there is deliberately no
            // fallback value to ship by accident.
            let _ = x;
            panic!("rust/v8math is not linked; enable jsrt's v8math-stub feature for development builds")
        }
    };
}

macro_rules! stub2 {
    ($name:ident, $method:ident, $js:literal) => {
        #[doc = $js]
        #[cfg(feature = "v8math-stub")]
        #[inline(always)]
        pub fn $name(a: f64, b: f64) -> f64 {
            a.$method(b)
        }

        #[doc = $js]
        #[cfg(not(feature = "v8math-stub"))]
        #[inline(always)]
        pub fn $name(a: f64, b: f64) -> f64 {
            let _ = (a, b);
            panic!("rust/v8math is not linked; enable jsrt's v8math-stub feature for development builds")
        }
    };
}

stub1!(sin, "`Math.sin`");
stub1!(cos, "`Math.cos`");
stub1!(tan, "`Math.tan`");
stub1!(asin, "`Math.asin`");
stub1!(acos, "`Math.acos`");
stub1!(atan, "`Math.atan`");
stub1!(sinh, "`Math.sinh`");
stub1!(cosh, "`Math.cosh`");
stub1!(tanh, "`Math.tanh`");
stub1!(asinh, "`Math.asinh`");
stub1!(acosh, "`Math.acosh`");
stub1!(atanh, "`Math.atanh`");
stub1!(exp, "`Math.exp`");
stub1!(expm1, exp_m1, "`Math.expm1`");
stub1!(cbrt, "`Math.cbrt`");

stub2!(atan2, atan2, "`Math.atan2`");
stub2!(pow, powf, "`Math.pow`, and the `**` operator, which V8 computes the same way");

/// `Math.log` (natural). Rust's inherent `ln` is the stub.
#[cfg(feature = "v8math-stub")]
#[inline(always)]
pub fn log(x: f64) -> f64 {
    x.ln()
}

#[cfg(not(feature = "v8math-stub"))]
#[inline(always)]
pub fn log(x: f64) -> f64 {
    let _ = x;
    panic!("rust/v8math is not linked; enable jsrt's v8math-stub feature for development builds")
}

/// `Math.log1p`.
#[cfg(feature = "v8math-stub")]
#[inline(always)]
pub fn log1p(x: f64) -> f64 {
    x.ln_1p()
}

#[cfg(not(feature = "v8math-stub"))]
#[inline(always)]
pub fn log1p(x: f64) -> f64 {
    let _ = x;
    panic!("rust/v8math is not linked; enable jsrt's v8math-stub feature for development builds")
}

/// `Math.log2`.
#[cfg(feature = "v8math-stub")]
#[inline(always)]
pub fn log2(x: f64) -> f64 {
    x.log2()
}

#[cfg(not(feature = "v8math-stub"))]
#[inline(always)]
pub fn log2(x: f64) -> f64 {
    let _ = x;
    panic!("rust/v8math is not linked; enable jsrt's v8math-stub feature for development builds")
}

/// `Math.log10`.
#[cfg(feature = "v8math-stub")]
#[inline(always)]
pub fn log10(x: f64) -> f64 {
    x.log10()
}

#[cfg(not(feature = "v8math-stub"))]
#[inline(always)]
pub fn log10(x: f64) -> f64 {
    let _ = x;
    panic!("rust/v8math is not linked; enable jsrt's v8math-stub feature for development builds")
}
