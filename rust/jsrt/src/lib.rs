#![forbid(unsafe_code)]
//! JavaScript semantics for the Rust port of the world simulation.
//!
//! Every function here reproduces what the JavaScript computes, to the
//! last bit, against the production server's engine: V8 13.6 as shipped
//! in Node 24.22. The sim's ticks, snapshots and hashes must come out
//! identical wherever they run, so Rust code porting JavaScript takes
//! its conversions, rounding, comparisons, sorts, iteration orders,
//! number formatting and random draws from this crate and never from
//! Rust's own equivalents, whose rules differ (saturating `as` casts,
//! ties-away `round`, NaN-ignoring `min`/`max`, unstable or differently
//! stable sorts, `HashMap` iteration order).
//!
//! Transcendental `Math` functions (sin, cos, pow, ...) are not here:
//! they belong to `rust/v8math`, which ports V8's own implementations.
//! Until that crate lands, [`v8math`] holds a feature-gated stub with
//! the same signatures so callers can be written now; its values are
//! the host libm's and are NOT bit-identical. The algebraic functions
//! in this crate (conversions, `js_round`, `js_min`/`js_max`, `fround`,
//! `hypot` of two and three numbers, integer ops) are exact and proven
//! against Node by the differential harness in `tools/`.
//!
//! Rules for everything in this crate, as everywhere in the port:
//! f64 arithmetic in JavaScript's operation order, no fused
//! multiply-add, no reassociation, f32 rounding exactly where the
//! JavaScript rounds.

pub mod conv;
pub mod hash;
pub mod map;
pub mod numfmt;
pub mod object;
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
