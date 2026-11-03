//! Vector arithmetic over navigation's point, as three.js computes it
//! where the JavaScript calls Vector3 methods.

use super::super::nav::math::Vec3;

pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}
pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}
pub fn scale(a: Vec3, s: f64) -> Vec3 {
    Vec3::new(a.x * s, a.y * s, a.z * s)
}
pub fn add_scaled(a: Vec3, b: Vec3, s: f64) -> Vec3 {
    Vec3::new(a.x + b.x * s, a.y + b.y * s, a.z + b.z * s)
}
pub fn dot(a: Vec3, b: Vec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}
pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x)
}
pub fn length_sq(a: Vec3) -> f64 {
    a.x * a.x + a.y * a.y + a.z * a.z
}
pub fn length(a: Vec3) -> f64 {
    v8math::sqrt(length_sq(a))
}
pub fn distance_sq(a: Vec3, b: Vec3) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let dz = a.z - b.z;
    dx * dx + dy * dy + dz * dz
}
/// three.js Vector3.normalize (divide by length || 1 via reciprocal).
pub fn normalize(a: Vec3) -> Vec3 {
    let mut v = a;
    super::super::nav::math::three_normalize(&mut v);
    v
}
