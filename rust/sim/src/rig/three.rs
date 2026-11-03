//! The part of three.js r170's math the rigs use: `Vector3`, `Quaternion`,
//! `Matrix4`, `Euler`, `Box3` and `MathUtils.clamp/lerp/smoothstep`, each
//! method the JavaScript's operation by operation, in f64, in the same order
//! and with the same branches (three's comments cite `src/math/*.js`).
//!
//! This is the package's own copy while rust/three (the simulation port's
//! three.js crate) is being built: its types and method names are that
//! crate's, so the rigs move onto it by changing their `use`. Methods that
//! return `this` in three return `&mut Self`; methods that take a target
//! write it and return it. Nothing here keeps an Object3D's Euler angles in
//! step with its quaternion: that is the transform tree's (`tree.rs`), which
//! knows where three's change callbacks fire.

use super::math;
use crate::collision::{js_max, js_min};

/// THREE.MathUtils.clamp: Math.max(min, Math.min(max, value)).
#[inline(always)]
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    js_max(min, js_min(max, value))
}

/// THREE.MathUtils.lerp: (1 - t) * x + t * y.
#[inline(always)]
pub fn lerp(x: f64, y: f64, t: f64) -> f64 {
    (1.0 - t) * x + t * y
}

/// THREE.MathUtils.smoothstep.
#[inline(always)]
pub fn smoothstep(x: f64, min: f64, max: f64) -> f64 {
    if x <= min {
        return 0.0;
    }
    if x >= max {
        return 1.0;
    }
    let x = (x - min) / (max - min);
    x * x * (3.0 - 2.0 * x)
}

/// THREE.MathUtils.degToRad: degrees * (Math.PI / 180).
#[inline(always)]
pub fn deg_to_rad(degrees: f64) -> f64 {
    degrees * (std::f64::consts::PI / 180.0)
}

/// `x || 1` for the lengths three divides by: 0, -0 and NaN become 1.
#[inline(always)]
fn or_one(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        1.0
    } else {
        x
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vector3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vector3 { x, y, z }
    }

    pub fn set(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        self.x = x;
        self.y = y;
        self.z = z;
        self
    }

    pub fn copy(&mut self, v: &Vector3) -> &mut Self {
        self.x = v.x;
        self.y = v.y;
        self.z = v.z;
        self
    }

    pub fn add(&mut self, v: &Vector3) -> &mut Self {
        self.x += v.x;
        self.y += v.y;
        self.z += v.z;
        self
    }

    pub fn add_vectors(&mut self, a: &Vector3, b: &Vector3) -> &mut Self {
        self.x = a.x + b.x;
        self.y = a.y + b.y;
        self.z = a.z + b.z;
        self
    }

    pub fn add_scaled_vector(&mut self, v: &Vector3, s: f64) -> &mut Self {
        self.x += v.x * s;
        self.y += v.y * s;
        self.z += v.z * s;
        self
    }

    pub fn sub(&mut self, v: &Vector3) -> &mut Self {
        self.x -= v.x;
        self.y -= v.y;
        self.z -= v.z;
        self
    }

    pub fn sub_vectors(&mut self, a: &Vector3, b: &Vector3) -> &mut Self {
        self.x = a.x - b.x;
        self.y = a.y - b.y;
        self.z = a.z - b.z;
        self
    }

    pub fn multiply_scalar(&mut self, scalar: f64) -> &mut Self {
        self.x *= scalar;
        self.y *= scalar;
        self.z *= scalar;
        self
    }

    /// `multiplyScalar(1 / scalar)`, as three divides.
    pub fn divide_scalar(&mut self, scalar: f64) -> &mut Self {
        self.multiply_scalar(1.0 / scalar)
    }

    pub fn negate(&mut self) -> &mut Self {
        self.x = -self.x;
        self.y = -self.y;
        self.z = -self.z;
        self
    }

    pub fn apply_matrix4(&mut self, m: &Matrix4) -> &mut Self {
        let (x, y, z) = (self.x, self.y, self.z);
        let e = &m.elements;
        let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
        self.x = (e[0] * x + e[4] * y + e[8] * z + e[12]) * w;
        self.y = (e[1] * x + e[5] * y + e[9] * z + e[13]) * w;
        self.z = (e[2] * x + e[6] * y + e[10] * z + e[14]) * w;
        self
    }

    pub fn apply_quaternion(&mut self, q: &Quaternion) -> &mut Self {
        let (vx, vy, vz) = (self.x, self.y, self.z);
        let (qx, qy, qz, qw) = (q.x, q.y, q.z, q.w);
        // t = 2 * cross( q.xyz, v );
        let tx = 2.0 * (qy * vz - qz * vy);
        let ty = 2.0 * (qz * vx - qx * vz);
        let tz = 2.0 * (qx * vy - qy * vx);
        // v + q.w * t + cross( q.xyz, t );
        self.x = vx + qw * tx + qy * tz - qz * ty;
        self.y = vy + qw * ty + qz * tx - qx * tz;
        self.z = vz + qw * tz + qx * ty - qy * tx;
        self
    }

    pub fn transform_direction(&mut self, m: &Matrix4) -> &mut Self {
        let (x, y, z) = (self.x, self.y, self.z);
        let e = &m.elements;
        self.x = e[0] * x + e[4] * y + e[8] * z;
        self.y = e[1] * x + e[5] * y + e[9] * z;
        self.z = e[2] * x + e[6] * y + e[10] * z;
        self.normalize()
    }

    pub fn dot(&self, v: &Vector3) -> f64 {
        self.x * v.x + self.y * v.y + self.z * v.z
    }

    pub fn length_sq(&self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// `divideScalar(this.length() || 1)`.
    pub fn normalize(&mut self) -> &mut Self {
        let l = or_one(self.length());
        self.divide_scalar(l)
    }

    pub fn lerp(&mut self, v: &Vector3, alpha: f64) -> &mut Self {
        self.x += (v.x - self.x) * alpha;
        self.y += (v.y - self.y) * alpha;
        self.z += (v.z - self.z) * alpha;
        self
    }

    pub fn lerp_vectors(&mut self, v1: &Vector3, v2: &Vector3, alpha: f64) -> &mut Self {
        self.x = v1.x + (v2.x - v1.x) * alpha;
        self.y = v1.y + (v2.y - v1.y) * alpha;
        self.z = v1.z + (v2.z - v1.z) * alpha;
        self
    }

    pub fn cross(&mut self, v: &Vector3) -> &mut Self {
        let a = *self;
        self.cross_vectors(&a, v)
    }

    pub fn cross_vectors(&mut self, a: &Vector3, b: &Vector3) -> &mut Self {
        let (ax, ay, az) = (a.x, a.y, a.z);
        let (bx, by, bz) = (b.x, b.y, b.z);
        self.x = ay * bz - az * by;
        self.y = az * bx - ax * bz;
        self.z = ax * by - ay * bx;
        self
    }

    pub fn distance_to_squared(&self, v: &Vector3) -> f64 {
        let (dx, dy, dz) = (self.x - v.x, self.y - v.y, self.z - v.z);
        dx * dx + dy * dy + dz * dz
    }

    pub fn distance_to(&self, v: &Vector3) -> f64 {
        self.distance_to_squared(v).sqrt()
    }

    pub fn set_from_matrix_position(&mut self, m: &Matrix4) -> &mut Self {
        let e = &m.elements;
        self.x = e[12];
        self.y = e[13];
        self.z = e[14];
        self
    }

    pub fn set_from_matrix_column(&mut self, m: &Matrix4, index: usize) -> &mut Self {
        self.from_array(&m.elements, index * 4)
    }

    pub fn from_array(&mut self, array: &[f64], offset: usize) -> &mut Self {
        self.x = array[offset];
        self.y = array[offset + 1];
        self.z = array[offset + 2];
        self
    }

    pub fn equals(&self, v: &Vector3) -> bool {
        v.x == self.x && v.y == self.y && v.z == self.z
    }

    /// The component by axis letter, as `v[axis]` reads it.
    pub fn get(&self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.x,
            Axis::Y => self.y,
            Axis::Z => self.z,
        }
    }

    pub fn set_axis(&mut self, axis: Axis, value: f64) {
        match axis {
            Axis::X => self.x = value,
            Axis::Y => self.y = value,
            Axis::Z => self.z = value,
        }
    }
}

/// A Vector3 component named by letter (`p[forward]` in the JavaScript).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quaternion {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Default for Quaternion {
    fn default() -> Self {
        Quaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }
    }
}

impl Quaternion {
    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Quaternion { x, y, z, w }
    }

    pub fn set(&mut self, x: f64, y: f64, z: f64, w: f64) -> &mut Self {
        self.x = x;
        self.y = y;
        self.z = z;
        self.w = w;
        self
    }

    pub fn copy(&mut self, q: &Quaternion) -> &mut Self {
        self.x = q.x;
        self.y = q.y;
        self.z = q.z;
        self.w = q.w;
        self
    }

    pub fn identity(&mut self) -> &mut Self {
        self.set(0.0, 0.0, 0.0, 1.0)
    }

    pub fn set_from_euler(&mut self, euler: &Euler) -> &mut Self {
        let (x, y, z, order) = (euler.x, euler.y, euler.z, euler.order);
        let c1 = math::cos(x / 2.0);
        let c2 = math::cos(y / 2.0);
        let c3 = math::cos(z / 2.0);
        let s1 = math::sin(x / 2.0);
        let s2 = math::sin(y / 2.0);
        let s3 = math::sin(z / 2.0);
        match order {
            EulerOrder::XYZ => {
                self.x = s1 * c2 * c3 + c1 * s2 * s3;
                self.y = c1 * s2 * c3 - s1 * c2 * s3;
                self.z = c1 * c2 * s3 + s1 * s2 * c3;
                self.w = c1 * c2 * c3 - s1 * s2 * s3;
            }
            EulerOrder::YXZ => {
                self.x = s1 * c2 * c3 + c1 * s2 * s3;
                self.y = c1 * s2 * c3 - s1 * c2 * s3;
                self.z = c1 * c2 * s3 - s1 * s2 * c3;
                self.w = c1 * c2 * c3 + s1 * s2 * s3;
            }
            EulerOrder::ZXY => {
                self.x = s1 * c2 * c3 - c1 * s2 * s3;
                self.y = c1 * s2 * c3 + s1 * c2 * s3;
                self.z = c1 * c2 * s3 + s1 * s2 * c3;
                self.w = c1 * c2 * c3 - s1 * s2 * s3;
            }
            EulerOrder::ZYX => {
                self.x = s1 * c2 * c3 - c1 * s2 * s3;
                self.y = c1 * s2 * c3 + s1 * c2 * s3;
                self.z = c1 * c2 * s3 - s1 * s2 * c3;
                self.w = c1 * c2 * c3 + s1 * s2 * s3;
            }
            EulerOrder::YZX => {
                self.x = s1 * c2 * c3 + c1 * s2 * s3;
                self.y = c1 * s2 * c3 + s1 * c2 * s3;
                self.z = c1 * c2 * s3 - s1 * s2 * c3;
                self.w = c1 * c2 * c3 - s1 * s2 * s3;
            }
            EulerOrder::XZY => {
                self.x = s1 * c2 * c3 - c1 * s2 * s3;
                self.y = c1 * s2 * c3 - s1 * c2 * s3;
                self.z = c1 * c2 * s3 + s1 * s2 * c3;
                self.w = c1 * c2 * c3 + s1 * s2 * s3;
            }
        }
        self
    }

    /// Assumes the axis is normalized.
    pub fn set_from_axis_angle(&mut self, axis: &Vector3, angle: f64) -> &mut Self {
        let half_angle = angle / 2.0;
        let s = math::sin(half_angle);
        self.x = axis.x * s;
        self.y = axis.y * s;
        self.z = axis.z * s;
        self.w = math::cos(half_angle);
        self
    }

    /// Assumes the upper 3x3 of m is a pure rotation matrix.
    pub fn set_from_rotation_matrix(&mut self, m: &Matrix4) -> &mut Self {
        let te = &m.elements;
        let (m11, m12, m13) = (te[0], te[4], te[8]);
        let (m21, m22, m23) = (te[1], te[5], te[9]);
        let (m31, m32, m33) = (te[2], te[6], te[10]);
        let trace = m11 + m22 + m33;
        if trace > 0.0 {
            let s = 0.5 / (trace + 1.0).sqrt();
            self.w = 0.25 / s;
            self.x = (m32 - m23) * s;
            self.y = (m13 - m31) * s;
            self.z = (m21 - m12) * s;
        } else if m11 > m22 && m11 > m33 {
            let s = 2.0 * (1.0 + m11 - m22 - m33).sqrt();
            self.w = (m32 - m23) / s;
            self.x = 0.25 * s;
            self.y = (m12 + m21) / s;
            self.z = (m13 + m31) / s;
        } else if m22 > m33 {
            let s = 2.0 * (1.0 + m22 - m11 - m33).sqrt();
            self.w = (m13 - m31) / s;
            self.x = (m12 + m21) / s;
            self.y = 0.25 * s;
            self.z = (m23 + m32) / s;
        } else {
            let s = 2.0 * (1.0 + m33 - m11 - m22).sqrt();
            self.w = (m21 - m12) / s;
            self.x = (m13 + m31) / s;
            self.y = (m23 + m32) / s;
            self.z = 0.25 * s;
        }
        self
    }

    /// Assumes unit length: the conjugate.
    pub fn invert(&mut self) -> &mut Self {
        self.conjugate()
    }

    pub fn conjugate(&mut self) -> &mut Self {
        self.x *= -1.0;
        self.y *= -1.0;
        self.z *= -1.0;
        self
    }

    pub fn dot(&self, v: &Quaternion) -> f64 {
        self.x * v.x + self.y * v.y + self.z * v.z + self.w * v.w
    }

    pub fn length_sq(&self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    pub fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt()
    }

    pub fn normalize(&mut self) -> &mut Self {
        let mut l = self.length();
        if l == 0.0 {
            self.x = 0.0;
            self.y = 0.0;
            self.z = 0.0;
            self.w = 1.0;
        } else {
            l = 1.0 / l;
            self.x *= l;
            self.y *= l;
            self.z *= l;
            self.w *= l;
        }
        self
    }

    pub fn multiply(&mut self, q: &Quaternion) -> &mut Self {
        let a = *self;
        self.multiply_quaternions(&a, q)
    }

    pub fn premultiply(&mut self, q: &Quaternion) -> &mut Self {
        let b = *self;
        self.multiply_quaternions(q, &b)
    }

    pub fn multiply_quaternions(&mut self, a: &Quaternion, b: &Quaternion) -> &mut Self {
        let (qax, qay, qaz, qaw) = (a.x, a.y, a.z, a.w);
        let (qbx, qby, qbz, qbw) = (b.x, b.y, b.z, b.w);
        self.x = qax * qbw + qaw * qbx + qay * qbz - qaz * qby;
        self.y = qay * qbw + qaw * qby + qaz * qbx - qax * qbz;
        self.z = qaz * qbw + qaw * qbz + qax * qby - qay * qbx;
        self.w = qaw * qbw - qax * qbx - qay * qby - qaz * qbz;
        self
    }

    /// `slerp(qb, t)`.
    pub fn slerp(&mut self, qb: &Quaternion, t: f64) -> &mut Self {
        self.slerp_tracked(qb, t);
        self
    }

    /// `slerp(qb, t)`, telling what three's change callback saw last: the
    /// value at the last point it fired, or None if it did not fire. Three
    /// fires it when it copies qb in (t 1, or the near hemisphere), and at
    /// the end unless the quaternions were already equal (cos >= 1), where
    /// it puts the original back without firing.
    pub fn slerp_tracked(&mut self, qb: &Quaternion, t: f64) -> Option<Quaternion> {
        if t == 0.0 {
            return None;
        }
        if t == 1.0 {
            self.copy(qb);
            return Some(*self);
        }
        let (x, y, z, w) = (self.x, self.y, self.z, self.w);
        let mut fired = None;
        let mut cos_half_theta = w * qb.w + x * qb.x + y * qb.y + z * qb.z;
        if cos_half_theta < 0.0 {
            self.w = -qb.w;
            self.x = -qb.x;
            self.y = -qb.y;
            self.z = -qb.z;
            cos_half_theta = -cos_half_theta;
        } else {
            self.copy(qb);
            fired = Some(*self);
        }
        if cos_half_theta >= 1.0 {
            self.w = w;
            self.x = x;
            self.y = y;
            self.z = z;
            return fired;
        }
        let sqr_sin_half_theta = 1.0 - cos_half_theta * cos_half_theta;
        if sqr_sin_half_theta <= f64::EPSILON {
            let s = 1.0 - t;
            self.w = s * w + t * self.w;
            self.x = s * x + t * self.x;
            self.y = s * y + t * self.y;
            self.z = s * z + t * self.z;
            self.normalize();
            return Some(*self);
        }
        let sin_half_theta = sqr_sin_half_theta.sqrt();
        let half_theta = math::atan2(sin_half_theta, cos_half_theta);
        let ratio_a = math::sin((1.0 - t) * half_theta) / sin_half_theta;
        let ratio_b = math::sin(t * half_theta) / sin_half_theta;
        self.w = w * ratio_a + self.w * ratio_b;
        self.x = x * ratio_a + self.x * ratio_b;
        self.y = y * ratio_a + self.y * ratio_b;
        self.z = z * ratio_a + self.z * ratio_b;
        Some(*self)
    }

    /// `slerpQuaternions(qa, qb, t)`: `copy(qa).slerp(qb, t)`.
    pub fn slerp_quaternions(&mut self, qa: &Quaternion, qb: &Quaternion, t: f64) -> &mut Self {
        self.copy(qa);
        self.slerp(qb, t)
    }

    pub fn from_array(&mut self, array: &[f64], offset: usize) -> &mut Self {
        self.x = array[offset];
        self.y = array[offset + 1];
        self.z = array[offset + 2];
        self.w = array[offset + 3];
        self
    }

    pub fn equals(&self, q: &Quaternion) -> bool {
        q.x == self.x && q.y == self.y && q.z == self.z && q.w == self.w
    }
}

/// Column-major, as three's `elements`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix4 {
    pub elements: [f64; 16],
}

const IDENTITY: [f64; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

impl Default for Matrix4 {
    fn default() -> Self {
        Matrix4 { elements: IDENTITY }
    }
}

impl Matrix4 {
    /// Row-major arguments, as three's `set`.
    #[allow(clippy::too_many_arguments)]
    pub fn set(
        &mut self,
        n11: f64, n12: f64, n13: f64, n14: f64,
        n21: f64, n22: f64, n23: f64, n24: f64,
        n31: f64, n32: f64, n33: f64, n34: f64,
        n41: f64, n42: f64, n43: f64, n44: f64,
    ) -> &mut Self {
        let te = &mut self.elements;
        te[0] = n11; te[4] = n12; te[8] = n13; te[12] = n14;
        te[1] = n21; te[5] = n22; te[9] = n23; te[13] = n24;
        te[2] = n31; te[6] = n32; te[10] = n33; te[14] = n34;
        te[3] = n41; te[7] = n42; te[11] = n43; te[15] = n44;
        self
    }

    pub fn identity(&mut self) -> &mut Self {
        self.elements = IDENTITY;
        self
    }

    pub fn copy(&mut self, m: &Matrix4) -> &mut Self {
        self.elements = m.elements;
        self
    }

    pub fn from_array(&mut self, array: &[f64], offset: usize) -> &mut Self {
        for i in 0..16 {
            self.elements[i] = array[i + offset];
        }
        self
    }

    pub fn make_basis(&mut self, x_axis: &Vector3, y_axis: &Vector3, z_axis: &Vector3) -> &mut Self {
        self.set(
            x_axis.x, y_axis.x, z_axis.x, 0.0,
            x_axis.y, y_axis.y, z_axis.y, 0.0,
            x_axis.z, y_axis.z, z_axis.z, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    /// Does not support reflection matrices.
    pub fn extract_rotation(&mut self, m: &Matrix4) -> &mut Self {
        let me = m.elements;
        let mut v = Vector3::default();
        let scale_x = 1.0 / v.set_from_matrix_column(m, 0).length();
        let scale_y = 1.0 / v.set_from_matrix_column(m, 1).length();
        let scale_z = 1.0 / v.set_from_matrix_column(m, 2).length();
        let te = &mut self.elements;
        te[0] = me[0] * scale_x;
        te[1] = me[1] * scale_x;
        te[2] = me[2] * scale_x;
        te[3] = 0.0;
        te[4] = me[4] * scale_y;
        te[5] = me[5] * scale_y;
        te[6] = me[6] * scale_y;
        te[7] = 0.0;
        te[8] = me[8] * scale_z;
        te[9] = me[9] * scale_z;
        te[10] = me[10] * scale_z;
        te[11] = 0.0;
        te[12] = 0.0;
        te[13] = 0.0;
        te[14] = 0.0;
        te[15] = 1.0;
        self
    }

    /// `compose(_zero, q, _one)`.
    pub fn make_rotation_from_quaternion(&mut self, q: &Quaternion) -> &mut Self {
        self.compose(&Vector3::new(0.0, 0.0, 0.0), q, &Vector3::new(1.0, 1.0, 1.0))
    }

    pub fn multiply(&mut self, m: &Matrix4) -> &mut Self {
        let a = *self;
        self.multiply_matrices(&a, m)
    }

    pub fn premultiply(&mut self, m: &Matrix4) -> &mut Self {
        let b = *self;
        self.multiply_matrices(m, &b)
    }

    pub fn multiply_matrices(&mut self, a: &Matrix4, b: &Matrix4) -> &mut Self {
        let ae = &a.elements;
        let be = &b.elements;
        let (a11, a12, a13, a14) = (ae[0], ae[4], ae[8], ae[12]);
        let (a21, a22, a23, a24) = (ae[1], ae[5], ae[9], ae[13]);
        let (a31, a32, a33, a34) = (ae[2], ae[6], ae[10], ae[14]);
        let (a41, a42, a43, a44) = (ae[3], ae[7], ae[11], ae[15]);
        let (b11, b12, b13, b14) = (be[0], be[4], be[8], be[12]);
        let (b21, b22, b23, b24) = (be[1], be[5], be[9], be[13]);
        let (b31, b32, b33, b34) = (be[2], be[6], be[10], be[14]);
        let (b41, b42, b43, b44) = (be[3], be[7], be[11], be[15]);
        let te = &mut self.elements;
        te[0] = a11 * b11 + a12 * b21 + a13 * b31 + a14 * b41;
        te[4] = a11 * b12 + a12 * b22 + a13 * b32 + a14 * b42;
        te[8] = a11 * b13 + a12 * b23 + a13 * b33 + a14 * b43;
        te[12] = a11 * b14 + a12 * b24 + a13 * b34 + a14 * b44;
        te[1] = a21 * b11 + a22 * b21 + a23 * b31 + a24 * b41;
        te[5] = a21 * b12 + a22 * b22 + a23 * b32 + a24 * b42;
        te[9] = a21 * b13 + a22 * b23 + a23 * b33 + a24 * b43;
        te[13] = a21 * b14 + a22 * b24 + a23 * b34 + a24 * b44;
        te[2] = a31 * b11 + a32 * b21 + a33 * b31 + a34 * b41;
        te[6] = a31 * b12 + a32 * b22 + a33 * b32 + a34 * b42;
        te[10] = a31 * b13 + a32 * b23 + a33 * b33 + a34 * b43;
        te[14] = a31 * b14 + a32 * b24 + a33 * b34 + a34 * b44;
        te[3] = a41 * b11 + a42 * b21 + a43 * b31 + a44 * b41;
        te[7] = a41 * b12 + a42 * b22 + a43 * b32 + a44 * b42;
        te[11] = a41 * b13 + a42 * b23 + a43 * b33 + a44 * b43;
        te[15] = a41 * b14 + a42 * b24 + a43 * b34 + a44 * b44;
        self
    }

    pub fn determinant(&self) -> f64 {
        let te = &self.elements;
        let (n11, n12, n13, n14) = (te[0], te[4], te[8], te[12]);
        let (n21, n22, n23, n24) = (te[1], te[5], te[9], te[13]);
        let (n31, n32, n33, n34) = (te[2], te[6], te[10], te[14]);
        let (n41, n42, n43, n44) = (te[3], te[7], te[11], te[15]);
        // The unary plus three writes on the first term of each group is
        // no operation.
        n41 * (n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33 + n13 * n22 * n34
            - n12 * n23 * n34)
            + n42
                * (n11 * n23 * n34 - n11 * n24 * n33 + n14 * n21 * n33 - n13 * n21 * n34 + n13 * n24 * n31
                    - n14 * n23 * n31)
            + n43
                * (n11 * n24 * n32 - n11 * n22 * n34 - n14 * n21 * n32 + n12 * n21 * n34 + n14 * n22 * n31
                    - n12 * n24 * n31)
            + n44
                * (-n13 * n22 * n31 - n11 * n23 * n32 + n11 * n22 * n33 + n13 * n21 * n32 - n12 * n21 * n33
                    + n12 * n23 * n31)
    }

    pub fn set_position(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        let te = &mut self.elements;
        te[12] = x;
        te[13] = y;
        te[14] = z;
        self
    }

    pub fn invert(&mut self) -> &mut Self {
        let te = &self.elements;
        let (n11, n21, n31, n41) = (te[0], te[1], te[2], te[3]);
        let (n12, n22, n32, n42) = (te[4], te[5], te[6], te[7]);
        let (n13, n23, n33, n43) = (te[8], te[9], te[10], te[11]);
        let (n14, n24, n34, n44) = (te[12], te[13], te[14], te[15]);
        let t11 = n23 * n34 * n42 - n24 * n33 * n42 + n24 * n32 * n43 - n22 * n34 * n43 - n23 * n32 * n44 + n22 * n33 * n44;
        let t12 = n14 * n33 * n42 - n13 * n34 * n42 - n14 * n32 * n43 + n12 * n34 * n43 + n13 * n32 * n44 - n12 * n33 * n44;
        let t13 = n13 * n24 * n42 - n14 * n23 * n42 + n14 * n22 * n43 - n12 * n24 * n43 - n13 * n22 * n44 + n12 * n23 * n44;
        let t14 = n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33 + n13 * n22 * n34 - n12 * n23 * n34;
        let det = n11 * t11 + n21 * t12 + n31 * t13 + n41 * t14;
        if det == 0.0 {
            self.elements = [0.0; 16];
            return self;
        }
        let det_inv = 1.0 / det;
        let te = &mut self.elements;
        te[0] = t11 * det_inv;
        te[1] = (n24 * n33 * n41 - n23 * n34 * n41 - n24 * n31 * n43 + n21 * n34 * n43 + n23 * n31 * n44 - n21 * n33 * n44) * det_inv;
        te[2] = (n22 * n34 * n41 - n24 * n32 * n41 + n24 * n31 * n42 - n21 * n34 * n42 - n22 * n31 * n44 + n21 * n32 * n44) * det_inv;
        te[3] = (n23 * n32 * n41 - n22 * n33 * n41 - n23 * n31 * n42 + n21 * n33 * n42 + n22 * n31 * n43 - n21 * n32 * n43) * det_inv;
        te[4] = t12 * det_inv;
        te[5] = (n13 * n34 * n41 - n14 * n33 * n41 + n14 * n31 * n43 - n11 * n34 * n43 - n13 * n31 * n44 + n11 * n33 * n44) * det_inv;
        te[6] = (n14 * n32 * n41 - n12 * n34 * n41 - n14 * n31 * n42 + n11 * n34 * n42 + n12 * n31 * n44 - n11 * n32 * n44) * det_inv;
        te[7] = (n12 * n33 * n41 - n13 * n32 * n41 + n13 * n31 * n42 - n11 * n33 * n42 - n12 * n31 * n43 + n11 * n32 * n43) * det_inv;
        te[8] = t13 * det_inv;
        te[9] = (n14 * n23 * n41 - n13 * n24 * n41 - n14 * n21 * n43 + n11 * n24 * n43 + n13 * n21 * n44 - n11 * n23 * n44) * det_inv;
        te[10] = (n12 * n24 * n41 - n14 * n22 * n41 + n14 * n21 * n42 - n11 * n24 * n42 - n12 * n21 * n44 + n11 * n22 * n44) * det_inv;
        te[11] = (n13 * n22 * n41 - n12 * n23 * n41 - n13 * n21 * n42 + n11 * n23 * n42 + n12 * n21 * n43 - n11 * n22 * n43) * det_inv;
        te[12] = t14 * det_inv;
        te[13] = (n13 * n24 * n31 - n14 * n23 * n31 + n14 * n21 * n33 - n11 * n24 * n33 - n13 * n21 * n34 + n11 * n23 * n34) * det_inv;
        te[14] = (n14 * n22 * n31 - n12 * n24 * n31 - n14 * n21 * n32 + n11 * n24 * n32 + n12 * n21 * n34 - n11 * n22 * n34) * det_inv;
        te[15] = (n12 * n23 * n31 - n13 * n22 * n31 + n13 * n21 * n32 - n11 * n23 * n32 - n12 * n21 * n33 + n11 * n22 * n33) * det_inv;
        self
    }

    pub fn compose(&mut self, position: &Vector3, quaternion: &Quaternion, scale: &Vector3) -> &mut Self {
        let (x, y, z, w) = (quaternion.x, quaternion.y, quaternion.z, quaternion.w);
        let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, xy, xz) = (x * x2, x * y2, x * z2);
        let (yy, yz, zz) = (y * y2, y * z2, z * z2);
        let (wx, wy, wz) = (w * x2, w * y2, w * z2);
        let (sx, sy, sz) = (scale.x, scale.y, scale.z);
        let te = &mut self.elements;
        te[0] = (1.0 - (yy + zz)) * sx;
        te[1] = (xy + wz) * sx;
        te[2] = (xz - wy) * sx;
        te[3] = 0.0;
        te[4] = (xy - wz) * sy;
        te[5] = (1.0 - (xx + zz)) * sy;
        te[6] = (yz + wx) * sy;
        te[7] = 0.0;
        te[8] = (xz + wy) * sz;
        te[9] = (yz - wx) * sz;
        te[10] = (1.0 - (xx + yy)) * sz;
        te[11] = 0.0;
        te[12] = position.x;
        te[13] = position.y;
        te[14] = position.z;
        te[15] = 1.0;
        self
    }

    /// `decompose(position, quaternion, scale)`. In three the quaternion's
    /// `setFromRotationMatrix` fires its change callback; the caller that
    /// decomposes into an Object3D's own quaternion syncs its Euler after.
    pub fn decompose(&self, position: &mut Vector3, quaternion: &mut Quaternion, scale: &mut Vector3) {
        let te = &self.elements;
        let mut v = Vector3::default();
        let mut sx = v.set(te[0], te[1], te[2]).length();
        let sy = v.set(te[4], te[5], te[6]).length();
        let sz = v.set(te[8], te[9], te[10]).length();
        // If the determinant is negative, one scale is inverted.
        let det = self.determinant();
        if det < 0.0 {
            sx = -sx;
        }
        position.x = te[12];
        position.y = te[13];
        position.z = te[14];
        // The rotation part, scaled.
        let mut m1 = *self;
        let inv_sx = 1.0 / sx;
        let inv_sy = 1.0 / sy;
        let inv_sz = 1.0 / sz;
        m1.elements[0] *= inv_sx;
        m1.elements[1] *= inv_sx;
        m1.elements[2] *= inv_sx;
        m1.elements[4] *= inv_sy;
        m1.elements[5] *= inv_sy;
        m1.elements[6] *= inv_sy;
        m1.elements[8] *= inv_sz;
        m1.elements[9] *= inv_sz;
        m1.elements[10] *= inv_sz;
        quaternion.set_from_rotation_matrix(&m1);
        scale.x = sx;
        scale.y = sy;
        scale.z = sz;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[allow(clippy::upper_case_acronyms)]
pub enum EulerOrder {
    #[default]
    XYZ,
    YXZ,
    ZXY,
    ZYX,
    YZX,
    XZY,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Euler {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub order: EulerOrder,
}

impl Euler {
    pub const fn new(x: f64, y: f64, z: f64, order: EulerOrder) -> Self {
        Euler { x, y, z, order }
    }

    /// `set(x, y, z, order = this._order)`.
    pub fn set(&mut self, x: f64, y: f64, z: f64, order: Option<EulerOrder>) -> &mut Self {
        self.x = x;
        self.y = y;
        self.z = z;
        if let Some(order) = order {
            self.order = order;
        }
        self
    }

    pub fn copy(&mut self, euler: &Euler) -> &mut Self {
        *self = *euler;
        self
    }

    /// Assumes the upper 3x3 of m is a pure rotation matrix.
    pub fn set_from_rotation_matrix(&mut self, m: &Matrix4, order: Option<EulerOrder>) -> &mut Self {
        let order = order.unwrap_or(self.order);
        let te = &m.elements;
        let (m11, m12, m13) = (te[0], te[4], te[8]);
        let (m21, m22, m23) = (te[1], te[5], te[9]);
        let (m31, m32, m33) = (te[2], te[6], te[10]);
        match order {
            EulerOrder::XYZ => {
                self.y = math::asin(clamp(m13, -1.0, 1.0));
                if m13.abs() < 0.9999999 {
                    self.x = math::atan2(-m23, m33);
                    self.z = math::atan2(-m12, m11);
                } else {
                    self.x = math::atan2(m32, m22);
                    self.z = 0.0;
                }
            }
            EulerOrder::YXZ => {
                self.x = math::asin(-clamp(m23, -1.0, 1.0));
                if m23.abs() < 0.9999999 {
                    self.y = math::atan2(m13, m33);
                    self.z = math::atan2(m21, m22);
                } else {
                    self.y = math::atan2(-m31, m11);
                    self.z = 0.0;
                }
            }
            EulerOrder::ZXY => {
                self.x = math::asin(clamp(m32, -1.0, 1.0));
                if m32.abs() < 0.9999999 {
                    self.y = math::atan2(-m31, m33);
                    self.z = math::atan2(-m12, m22);
                } else {
                    self.y = 0.0;
                    self.z = math::atan2(m21, m11);
                }
            }
            EulerOrder::ZYX => {
                self.y = math::asin(-clamp(m31, -1.0, 1.0));
                if m31.abs() < 0.9999999 {
                    self.x = math::atan2(m32, m33);
                    self.z = math::atan2(m21, m11);
                } else {
                    self.x = 0.0;
                    self.z = math::atan2(-m12, m22);
                }
            }
            EulerOrder::YZX => {
                self.z = math::asin(clamp(m21, -1.0, 1.0));
                if m21.abs() < 0.9999999 {
                    self.x = math::atan2(-m23, m22);
                    self.y = math::atan2(-m31, m11);
                } else {
                    self.x = 0.0;
                    self.y = math::atan2(m13, m33);
                }
            }
            EulerOrder::XZY => {
                self.z = math::asin(-clamp(m12, -1.0, 1.0));
                if m12.abs() < 0.9999999 {
                    self.x = math::atan2(m32, m22);
                    self.y = math::atan2(m13, m11);
                } else {
                    self.x = math::atan2(-m23, m33);
                    self.y = 0.0;
                }
            }
        }
        self.order = order;
        self
    }

    /// `setFromQuaternion(q, order)`: through a rotation matrix.
    pub fn set_from_quaternion(&mut self, q: &Quaternion, order: Option<EulerOrder>) -> &mut Self {
        let mut m = Matrix4::default();
        m.make_rotation_from_quaternion(q);
        self.set_from_rotation_matrix(&m, order)
    }
}

/// Box3, the part the muzzle finders use (min/max start at +/-Infinity).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box3 {
    pub min: Vector3,
    pub max: Vector3,
}

impl Default for Box3 {
    fn default() -> Self {
        Box3 {
            min: Vector3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            max: Vector3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }
}

impl Box3 {
    pub fn make_empty(&mut self) -> &mut Self {
        *self = Box3::default();
        self
    }

    /// `expandByPoint`: Vector3.min/max, Math.min/Math.max per axis.
    pub fn expand_by_point(&mut self, p: &Vector3) -> &mut Self {
        self.min.x = js_min(self.min.x, p.x);
        self.min.y = js_min(self.min.y, p.y);
        self.min.z = js_min(self.min.z, p.z);
        self.max.x = js_max(self.max.x, p.x);
        self.max.y = js_max(self.max.y, p.y);
        self.max.z = js_max(self.max.z, p.z);
        self
    }

    /// `setFromPoints`: made empty, then each point in turn.
    pub fn set_from_points(&mut self, points: &[Vector3]) -> &mut Self {
        self.make_empty();
        for p in points {
            self.expand_by_point(p);
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        // This is a more robust check for empty than ( volume <= 0 ) because
        // volume can get positive with two negative axes.
        self.max.x < self.min.x || self.max.y < self.min.y || self.max.z < self.min.z
    }

    /// `getCenter(target)`: (0, 0, 0) when empty, else (min + max) * 0.5.
    pub fn get_center(&self, target: &mut Vector3) -> Vector3 {
        if self.is_empty() {
            target.set(0.0, 0.0, 0.0);
        } else {
            target.add_vectors(&self.min, &self.max).multiply_scalar(0.5);
        }
        *target
    }
}
