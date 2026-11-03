//! The arithmetic the rounds and items do, as three.js r170 and V8 do it:
//! each function is a three.js method (or a `Math` function) transliterated
//! operation by operation, so every result is the JavaScript's to the last
//! bit (no reassociation, no fused multiply-add, `x / s` as three's
//! `x * (1 / s)` where three divides that way).
//!
//! Transcendental functions are V8's own (rust/v8math: the production
//! server's V8 13.6), never the host's libm; everything else here is exact
//! on any host (square roots are correctly rounded everywhere).

pub use bfe_content::js::{js_hypot, js_max, js_min};
pub use v8math::{cos, sin};

pub type V3 = [f64; 3];
/// x, y, z, w.
pub type Quat = crate::store::Quat;

pub const UP: V3 = [0.0, 1.0, 0.0];
pub const QUAT_IDENTITY: Quat = [0.0, 0.0, 0.0, 1.0];

/// `x || fallback` for a number: the fallback when x is 0, -0 or NaN.
#[inline]
pub fn or(x: f64, fallback: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        fallback
    } else {
        x
    }
}

// Vector3.

#[inline]
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// subVectors(a, b).
#[inline]
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// multiplyScalar.
#[inline]
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// divideScalar: three multiplies by the reciprocal.
#[inline]
pub fn divide(a: V3, s: f64) -> V3 {
    scale(a, 1.0 / s)
}

/// addScaledVector: `this.x += v.x * s`.
#[inline]
pub fn add_scaled(a: V3, v: V3, s: f64) -> V3 {
    [a[0] + v[0] * s, a[1] + v[1] * s, a[2] + v[2] * s]
}

#[inline]
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[inline]
pub fn length_sq(a: V3) -> f64 {
    a[0] * a[0] + a[1] * a[1] + a[2] * a[2]
}

#[inline]
pub fn length(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// normalize: `divideScalar(length() || 1)`.
#[inline]
pub fn normalize(a: V3) -> V3 {
    divide(a, or(length(a), 1.0))
}

/// crossVectors(a, b).
#[inline]
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// distanceTo: the root of distanceToSquared, `dx = this.x - v.x`.
#[inline]
pub fn distance_to(a: V3, b: V3) -> f64 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// lerp(v, alpha) on `a`: `this.x += (v.x - this.x) * alpha`.
#[inline]
pub fn lerp(a: V3, v: V3, alpha: f64) -> V3 {
    [a[0] + (v[0] - a[0]) * alpha, a[1] + (v[1] - a[1]) * alpha, a[2] + (v[2] - a[2]) * alpha]
}

/// applyQuaternion (q of unit length).
#[inline]
pub fn apply_quaternion(v: V3, q: Quat) -> V3 {
    let (vx, vy, vz) = (v[0], v[1], v[2]);
    let (qx, qy, qz, qw) = (q[0], q[1], q[2], q[3]);
    let tx = 2.0 * (qy * vz - qz * vy);
    let ty = 2.0 * (qz * vx - qx * vz);
    let tz = 2.0 * (qx * vy - qy * vx);
    [vx + qw * tx + qy * tz - qz * ty, vy + qw * ty + qz * tx - qx * tz, vz + qw * tz + qx * ty - qy * tx]
}

/// applyAxisAngle: applyQuaternion(setFromAxisAngle(axis, angle)).
#[inline]
pub fn apply_axis_angle(v: V3, axis: V3, angle: f64) -> V3 {
    apply_quaternion(v, axis_angle(axis, angle))
}

/// MathUtils.clamp: `Math.max(min, Math.min(max, value))`.
#[inline]
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    js_max(min, js_min(max, value))
}

// Quaternion.

/// setFromAxisAngle (axis of unit length).
#[inline]
pub fn axis_angle(axis: V3, angle: f64) -> Quat {
    let half = angle / 2.0;
    let s = sin(half);
    [axis[0] * s, axis[1] * s, axis[2] * s, cos(half)]
}

/// multiplyQuaternions(a, b).
#[inline]
pub fn multiply(a: Quat, b: Quat) -> Quat {
    let (qax, qay, qaz, qaw) = (a[0], a[1], a[2], a[3]);
    let (qbx, qby, qbz, qbw) = (b[0], b[1], b[2], b[3]);
    [
        qax * qbw + qaw * qbx + qay * qbz - qaz * qby,
        qay * qbw + qaw * qby + qaz * qbx - qax * qbz,
        qaz * qbw + qaw * qbz + qax * qby - qay * qbx,
        qaw * qbw - qax * qbx - qay * qby - qaz * qbz,
    ]
}

/// setFromRotationMatrix(new Matrix4().makeBasis(x, y, z)).
pub fn from_basis(x: V3, y: V3, z: V3) -> Quat {
    use bfe_content::three::{Matrix4, Quaternion, Vector3};
    let v = |a: V3| Vector3 { x: a[0], y: a[1], z: a[2] };
    let mut q = Quaternion::default();
    q.set_from_rotation_matrix(&Matrix4::make_basis(&v(x), &v(y), &v(z)));
    [q.x, q.y, q.z, q.w]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn division_is_by_the_reciprocal_as_three_does() {
        // 1 / 3 then times: not the same as a straight division for every x.
        let x = 0.1 + 0.2;
        assert_eq!(divide([x, 0.0, 0.0], 3.0)[0].to_bits(), (x * (1.0 / 3.0)).to_bits());
        assert_eq!(normalize([0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
        assert_eq!(or(f64::NAN, 1.0), 1.0);
        assert_eq!(or(-0.0, 2.0), 2.0);
    }

    #[test]
    fn a_basis_turns_as_three_turns_it() {
        // Nose along +x, its x across the world's up: a quarter turn about y.
        let forward = [1.0, 0.0, 0.0];
        let right = normalize(cross(UP, forward));
        let up = cross(forward, right);
        let q = from_basis(right, up, forward);
        let turned = apply_quaternion([0.0, 0.0, 1.0], q);
        assert!((turned[0] - 1.0).abs() < 1e-12 && turned[2].abs() < 1e-12);
    }
}
