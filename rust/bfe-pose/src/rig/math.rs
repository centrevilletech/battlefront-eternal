//! The transcendental `Math` functions the rigs, poses and hulls call:
//! `sin`, `cos`, `atan2`, `asin`, `acos`, `atan` and `exp`, through
//! rust/v8math, which computes each as V8 13.6 does, bit for bit. Rust's
//! own `f64::sin` and friends use the host's libm, which differs in the last
//! bit on some inputs, so nothing that must match the reference calls them.
//! `sqrt` is exact everywhere and stays `f64::sqrt`; `Math.hypot` of two
//! numbers is `v8math::hypot`.

/// Math.sin
#[inline(always)]
pub fn sin(x: f64) -> f64 {
    v8math::sin(x)
}

/// Math.cos
#[inline(always)]
pub fn cos(x: f64) -> f64 {
    v8math::cos(x)
}

/// Math.atan2(y, x)
#[inline(always)]
pub fn atan2(y: f64, x: f64) -> f64 {
    v8math::atan2(y, x)
}

/// Math.asin
#[inline(always)]
pub fn asin(x: f64) -> f64 {
    v8math::asin(x)
}

/// Math.acos
#[inline(always)]
pub fn acos(x: f64) -> f64 {
    v8math::acos(x)
}

/// Math.atan
#[inline(always)]
pub fn atan(x: f64) -> f64 {
    v8math::atan(x)
}

/// Math.exp
#[inline(always)]
pub fn exp(x: f64) -> f64 {
    v8math::exp(x)
}
