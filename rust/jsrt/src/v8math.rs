//! The seam to `rust/v8math`, the crate that computes V8 13.6's own
//! transcendental `Math` functions (the production server's, bit for
//! bit). Every transcendental the ported sim needs goes through the
//! functions named here and nowhere else: never Rust's `f64::sin` and
//! friends, never values taken from a host's `Math` (the client and the
//! server run different V8s and already disagree in the last bit today).
//!
//! These are rust/v8math's own functions, re-exported: `pow` is V8 13.6's
//! `Math.pow`, which the `**` operator computes the same way. The general
//! `hypot` of the arities the game uses is [`crate::conv`]'s.

pub use ::v8math::{
    acos, acosh, asin, asinh, atan, atan2, atanh, cbrt, cos, cosh, exp, expm1, log, log10, log1p, log2, pow, sin, sinh,
    tan, tanh,
};
