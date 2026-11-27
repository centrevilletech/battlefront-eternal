//! Small vector helpers for the pilots (deterministic: `v8math` only).

#[inline]
pub fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
pub fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
pub fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}
#[inline]
pub fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
#[inline]
pub fn len(a: [f64; 3]) -> f64 {
    v8math::hypot3(a[0], a[1], a[2])
}
#[inline]
pub fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    len(sub(a, b))
}
/// `a` scaled to length one, or straight ahead (+Z) when it has no length.
#[inline]
pub fn unit(a: [f64; 3]) -> [f64; 3] {
    let l = len(a);
    if l > 1e-9 { scale(a, 1.0 / l) } else { [0.0, 0.0, 1.0] }
}
#[inline]
pub fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    v8math::min(hi, v8math::max(lo, x))
}
