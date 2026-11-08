//! Displayed unit effects keep their clocks and bounded birth stores here.
//! Browser hosts retain scene anchors and answer collision requests only.
pub mod jetpack;
pub mod shield;
pub mod trail;
use three::Vector3;
pub(crate) fn vector(row: &[f64], at: usize) -> Vector3 {
    Vector3::new(row[at], row[at + 1], row[at + 2])
}
pub(crate) fn put(row: &mut [f64], at: usize, v: Vector3) {
    row[at] = v.x;
    row[at + 1] = v.y;
    row[at + 2] = v.z;
}
pub(crate) fn lerp(a: f64, b: f64, t: f64) -> f64 {
    (1.0 - t) * a + t * b
}

#[cfg(test)]
mod tests;
