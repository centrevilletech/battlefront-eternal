#![forbid(unsafe_code)]
//! JavaScript semantics for the Rust port of the world simulation.

pub mod conv;
pub mod hash;
pub mod map;
pub mod numfmt;
pub mod rng;
pub mod sort;
pub mod text;
pub mod v8math;

pub use conv::*;
pub use map::{JsMap, JsSet};
pub use rng::{ClassicRandom, Mulberry32, RandomStreams};

/// Whether a JavaScript number is falsy (`x || d` picks `d`): zero of
/// either sign, or NaN.
#[inline(always)]
pub fn num_falsy(x: f64) -> bool {
    x == 0.0 || x.is_nan()
}

/// Whether a JavaScript number is truthy.
#[inline(always)]
pub fn num_truthy(x: f64) -> bool {
    !num_falsy(x)
}

/// `x || d` for numbers: `d` when `x` is 0, -0 or NaN.
#[inline(always)]
pub fn num_or(x: f64, d: f64) -> f64 {
    if num_falsy(x) { d } else { x }
}

/// Whether a JavaScript string is falsy: only the empty string.
#[inline(always)]
pub fn str_falsy(s: &str) -> bool {
    s.is_empty()
}

/// SameValueZero, the equality `Map`, `Set`, `Array.includes` and
/// `indexOf` use: NaN equals NaN, and +0 equals -0.
#[inline(always)]
pub fn same_value_zero(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

/// SameValue (`Object.is`): SameValueZero, but +0 and -0 differ. The
/// differential tests compare with this (through bit patterns, with
/// every NaN treated alike).
#[inline(always)]
pub fn same_value(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    if a == 0.0 && b == 0.0 {
        return a.is_sign_negative() == b.is_sign_negative();
    }
    a == b
}
