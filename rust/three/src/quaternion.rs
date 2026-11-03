//! `Quaternion.js` (r170). The `_onChangeCallback` sync to a
//! sibling Euler lives in [`crate::TransformNode`]; a bare
//! `Quaternion` here is just the four numbers, as in three when
//! nothing registered a callback.

use jsrt::v8math;

use crate::euler::{Euler, EulerOrder};
use crate::mathutils::clamp;
use crate::matrix::Matrix4;
use crate::vector::Vector3;

/// `THREE.Quaternion`, r170.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quaternion {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Default for Quaternion {
    /// `new Quaternion()`: (0, 0, 0, 1).
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0, 1.0)
    }
}

impl Quaternion {
    pub fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Quaternion { x, y, z, w }
    }

    /// `Quaternion.slerpFlat(dst, dstOffset, src0, srcOffset0, src1,
    /// srcOffset1, t)` on flat f64 slices (in three these may be
    /// Float32Arrays; a caller sorting through f32 storage rounds
    /// at the stores itself — the arithmetic here is the double
    /// arithmetic three does between the reads and writes).
    pub fn slerp_flat(
        dst: &mut [f64],
        dst_offset: usize,
        src0: &[f64],
        src_offset0: usize,
        src1: &[f64],
        src_offset1: usize,
        t: f64,
    ) {
        let mut x0 = src0[src_offset0];
        let mut y0 = src0[src_offset0 + 1];
        let mut z0 = src0[src_offset0 + 2];
        let mut w0 = src0[src_offset0 + 3];
        let x1 = src1[src_offset1];
        let y1 = src1[src_offset1 + 1];
        let z1 = src1[src_offset1 + 2];
        let w1 = src1[src_offset1 + 3];
        if t == 0.0 {
            dst[dst_offset] = x0;
            dst[dst_offset + 1] = y0;
            dst[dst_offset + 2] = z0;
            dst[dst_offset + 3] = w0;
            return;
        }
        if t == 1.0 {
            dst[dst_offset] = x1;
            dst[dst_offset + 1] = y1;
            dst[dst_offset + 2] = z1;
            dst[dst_offset + 3] = w1;
            return;
        }
        let mut t = t;
        if w0 != w1 || x0 != x1 || y0 != y1 || z0 != z1 {
            let mut s = 1.0 - t;
            let cos = x0 * x1 + y0 * y1 + z0 * z1 + w0 * w1;
            let dir = if cos >= 0.0 { 1.0 } else { -1.0 };
            let sqr_sin = 1.0 - cos * cos;
            if sqr_sin > f64::EPSILON {
                let sin = sqr_sin.sqrt();
                let len = v8math::atan2(sin, cos * dir);
                s = v8math::sin(s * len) / sin;
                t = v8math::sin(t * len) / sin;
            }
            let t_dir = t * dir;
            x0 = x0 * s + x1 * t_dir;
            y0 = y0 * s + y1 * t_dir;
            z0 = z0 * s + z1 * t_dir;
            w0 = w0 * s + w1 * t_dir;
            if s == 1.0 - t {
                let f = 1.0 / (x0 * x0 + y0 * y0 + z0 * z0 + w0 * w0).sqrt();
                x0 *= f;
                y0 *= f;
                z0 *= f;
                w0 *= f;
            }
        }
        dst[dst_offset] = x0;
        dst[dst_offset + 1] = y0;
        dst[dst_offset + 2] = z0;
        dst[dst_offset + 3] = w0;
    }

    /// `Quaternion.multiplyQuaternionsFlat(...)`.
    pub fn multiply_quaternions_flat<'t>(
        dst: &'t mut [f64],
        dst_offset: usize,
        src0: &[f64],
        src_offset0: usize,
        src1: &[f64],
        src_offset1: usize,
    ) -> &'t mut [f64] {
        let x0 = src0[src_offset0];
        let y0 = src0[src_offset0 + 1];
        let z0 = src0[src_offset0 + 2];
        let w0 = src0[src_offset0 + 3];
        let x1 = src1[src_offset1];
        let y1 = src1[src_offset1 + 1];
        let z1 = src1[src_offset1 + 2];
        let w1 = src1[src_offset1 + 3];
        dst[dst_offset] = x0 * w1 + w0 * x1 + y0 * z1 - z0 * y1;
        dst[dst_offset + 1] = y0 * w1 + w0 * y1 + z0 * x1 - x0 * z1;
        dst[dst_offset + 2] = z0 * w1 + w0 * z1 + x0 * y1 - y0 * x1;
        dst[dst_offset + 3] = w0 * w1 - x0 * x1 - y0 * y1 - z0 * z1;
        dst
    }

    /// `set(x, y, z, w)`.
    pub fn set(&mut self, x: f64, y: f64, z: f64, w: f64) -> &mut Self {
        self.x = x;
        self.y = y;
        self.z = z;
        self.w = w;
        self
    }

    /// `copy(quaternion)`.
    pub fn copy(&mut self, quaternion: &Quaternion) -> &mut Self {
        self.x = quaternion.x;
        self.y = quaternion.y;
        self.z = quaternion.z;
        self.w = quaternion.w;
        self
    }

    /// `setFromEuler(euler, update)`.
    pub fn set_from_euler(&mut self, euler: &Euler) -> &mut Self {
        let (x, y, z) = (euler.x, euler.y, euler.z);
        let c1 = v8math::cos(x / 2.0);
        let c2 = v8math::cos(y / 2.0);
        let c3 = v8math::cos(z / 2.0);
        let s1 = v8math::sin(x / 2.0);
        let s2 = v8math::sin(y / 2.0);
        let s3 = v8math::sin(z / 2.0);
        match euler.order {
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

    /// `setFromAxisAngle(axis, angle)`: assumes the axis is
    /// normalized.
    pub fn set_from_axis_angle(&mut self, axis: &Vector3, angle: f64) -> &mut Self {
        let half_angle = angle / 2.0;
        let s = v8math::sin(half_angle);
        self.x = axis.x * s;
        self.y = axis.y * s;
        self.z = axis.z * s;
        self.w = v8math::cos(half_angle);
        self
    }

    /// `setFromRotationMatrix(m)`: assumes the upper 3x3 is a pure
    /// rotation matrix. The four branches are three's, gimbal
    /// neighbourhood included.
    pub fn set_from_rotation_matrix(&mut self, m: &Matrix4) -> &mut Self {
        let te = &m.elements;
        let m11 = te[0];
        let m12 = te[4];
        let m13 = te[8];
        let m21 = te[1];
        let m22 = te[5];
        let m23 = te[9];
        let m31 = te[2];
        let m32 = te[6];
        let m33 = te[10];
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

    /// `setFromUnitVectors(vFrom, vTo)`: assumes both normalized.
    pub fn set_from_unit_vectors(&mut self, v_from: &Vector3, v_to: &Vector3) -> &mut Self {
        let mut r = v_from.dot(v_to) + 1.0;
        if r < f64::EPSILON {
            r = 0.0;
            if v_from.x.abs() > v_from.z.abs() {
                self.x = -v_from.y;
                self.y = v_from.x;
                self.z = 0.0;
                self.w = r;
            } else {
                self.x = 0.0;
                self.y = -v_from.z;
                self.z = v_from.y;
                self.w = r;
            }
        } else {
            self.x = v_from.y * v_to.z - v_from.z * v_to.y;
            self.y = v_from.z * v_to.x - v_from.x * v_to.z;
            self.z = v_from.x * v_to.y - v_from.y * v_to.x;
            self.w = r;
        }
        self.normalize()
    }

    /// `angleTo(q)`.
    pub fn angle_to(&self, q: &Quaternion) -> f64 {
        2.0 * v8math::acos(clamp(self.dot(q), -1.0, 1.0).abs())
    }

    /// `rotateTowards(q, step)`.
    pub fn rotate_towards(&mut self, q: &Quaternion, step: f64) -> &mut Self {
        let angle = self.angle_to(q);
        if angle == 0.0 {
            return self;
        }
        let t = jsrt::conv::js_min(1.0, step / angle);
        self.slerp(q, t);
        self
    }

    /// `identity()`.
    pub fn identity(&mut self) -> &mut Self {
        self.set(0.0, 0.0, 0.0, 1.0)
    }

    /// `invert()`: assumes unit length (it is conjugate).
    pub fn invert(&mut self) -> &mut Self {
        self.conjugate()
    }

    /// `conjugate()`.
    pub fn conjugate(&mut self) -> &mut Self {
        self.x *= -1.0;
        self.y *= -1.0;
        self.z *= -1.0;
        self
    }

    /// `dot(v)`.
    pub fn dot(&self, v: &Quaternion) -> f64 {
        self.x * v.x + self.y * v.y + self.z * v.z + self.w * v.w
    }

    /// `lengthSq()`.
    pub fn length_sq(&self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    /// `length()`.
    pub fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt()
    }

    /// `normalize()`: a zero quaternion becomes (0, 0, 0, 1).
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

    /// `multiply(q)`.
    pub fn multiply(&mut self, q: &Quaternion) -> &mut Self {
        let a = *self;
        self.multiply_quaternions(&a, q)
    }

    /// `premultiply(q)`.
    pub fn premultiply(&mut self, q: &Quaternion) -> &mut Self {
        let b = *self;
        self.multiply_quaternions(q, &b)
    }

    /// `multiplyQuaternions(a, b)`.
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
        if t == 0.0 {
            return self;
        }
        if t == 1.0 {
            return self.copy(qb);
        }
        let (x, y, z, w) = (self.x, self.y, self.z, self.w);
        let mut cos_half_theta = w * qb.w + x * qb.x + y * qb.y + z * qb.z;
        if cos_half_theta < 0.0 {
            self.w = -qb.w;
            self.x = -qb.x;
            self.y = -qb.y;
            self.z = -qb.z;
            cos_half_theta = -cos_half_theta;
        } else {
            self.copy(qb);
        }
        if cos_half_theta >= 1.0 {
            self.w = w;
            self.x = x;
            self.y = y;
            self.z = z;
            return self;
        }
        let sqr_sin_half_theta = 1.0 - cos_half_theta * cos_half_theta;
        if sqr_sin_half_theta <= f64::EPSILON {
            let s = 1.0 - t;
            self.w = s * w + t * self.w;
            self.x = s * x + t * self.x;
            self.y = s * y + t * self.y;
            self.z = s * z + t * self.z;
            self.normalize();
            return self;
        }
        let sin_half_theta = sqr_sin_half_theta.sqrt();
        let half_theta = v8math::atan2(sin_half_theta, cos_half_theta);
        let ratio_a = v8math::sin((1.0 - t) * half_theta) / sin_half_theta;
        let ratio_b = v8math::sin(t * half_theta) / sin_half_theta;
        self.w = w * ratio_a + self.w * ratio_b;
        self.x = x * ratio_a + self.x * ratio_b;
        self.y = y * ratio_a + self.y * ratio_b;
        self.z = z * ratio_a + self.z * ratio_b;
        self
    }

    /// `slerpQuaternions(qa, qb, t)`.
    pub fn slerp_quaternions(&mut self, qa: &Quaternion, qb: &Quaternion, t: f64) -> &mut Self {
        self.copy(qa);
        self.slerp(qb, t)
    }

    /// `random()` (Shoemake), with the three `Math.random` draws
    /// handed in, in three's order: theta1, theta2, x0.
    pub fn random(&mut self, random: &mut impl FnMut() -> f64) -> &mut Self {
        let theta1 = 2.0 * core::f64::consts::PI * random();
        let theta2 = 2.0 * core::f64::consts::PI * random();
        let x0 = random();
        let r1 = (1.0 - x0).sqrt();
        let r2 = x0.sqrt();
        self.set(
            r1 * v8math::sin(theta1),
            r1 * v8math::cos(theta1),
            r2 * v8math::sin(theta2),
            r2 * v8math::cos(theta2),
        )
    }

    /// `equals(quaternion)`: `===` on every component.
    pub fn equals(&self, quaternion: &Quaternion) -> bool {
        quaternion.x == self.x
            && quaternion.y == self.y
            && quaternion.z == self.z
            && quaternion.w == self.w
    }

    /// `fromArray(array, offset)`.
    pub fn from_array(&mut self, array: &[f64], offset: usize) -> &mut Self {
        self.x = array[offset];
        self.y = array[offset + 1];
        self.z = array[offset + 2];
        self.w = array[offset + 3];
        self
    }

    /// `toArray(array, offset)`.
    pub fn to_array<'t>(&self, array: &'t mut [f64], offset: usize) -> &'t mut [f64] {
        array[offset] = self.x;
        array[offset + 1] = self.y;
        array[offset + 2] = self.z;
        array[offset + 3] = self.w;
        array
    }
}
