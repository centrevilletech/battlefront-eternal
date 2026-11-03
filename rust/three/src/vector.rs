//! `Vector3.js`, `Vector2.js` and `Vector4.js` (r170). Note the
//! spots where three's spelling is behaviour: `divideScalar` is
//! `multiplyScalar(1 / scalar)`, `normalize` divides by
//! `length || 1`, `applyMatrix4` multiplies by a computed `1 / w`
//! rather than dividing, and `clampLength` divides by
//! `length || 1` before scaling. Each is kept as written.

use jsrt::conv::{js_max, js_min, js_round};
use jsrt::num_falsy;
use jsrt::v8math;

use crate::euler::Euler;
use crate::geom::{Cylindrical, Spherical};
use crate::mathutils::clamp;
use crate::matrix::{Matrix3, Matrix4};
use crate::quaternion::Quaternion;

/// The scalar `length || 1` of three's normalize/clampLength:
/// 0 and NaN lengths divide by 1.
#[inline(always)]
fn length_or_1(length: f64) -> f64 {
    if num_falsy(length) { 1.0 } else { length }
}

/// A Vector3 component named by letter (`p[forward]` in the
/// JavaScript, where forward is 'x', 'y' or 'z').
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// `THREE.Vector3`, r170.
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

    /// `set(x, y, z)` (three's two-argument sprite form, which
    /// keeps z, is `set_xy`).
    pub fn set(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        self.x = x;
        self.y = y;
        self.z = z;
        self
    }

    /// `set(x, y)` with z undefined: z keeps its value.
    pub fn set_xy(&mut self, x: f64, y: f64) -> &mut Self {
        self.x = x;
        self.y = y;
        self
    }

    pub fn set_scalar(&mut self, scalar: f64) -> &mut Self {
        self.x = scalar;
        self.y = scalar;
        self.z = scalar;
        self
    }

    pub fn set_x(&mut self, x: f64) -> &mut Self {
        self.x = x;
        self
    }

    pub fn set_y(&mut self, y: f64) -> &mut Self {
        self.y = y;
        self
    }

    pub fn set_z(&mut self, z: f64) -> &mut Self {
        self.z = z;
        self
    }

    /// `setComponent(index, value)`: panics where three throws
    /// (an index outside 0..=2 never reaches a tick).
    pub fn set_component(&mut self, index: usize, value: f64) -> &mut Self {
        match index {
            0 => self.x = value,
            1 => self.y = value,
            2 => self.z = value,
            _ => panic!("index is out of range: {index}"),
        }
        self
    }

    pub fn get_component(&self, index: usize) -> f64 {
        match index {
            0 => self.x,
            1 => self.y,
            2 => self.z,
            _ => panic!("index is out of range: {index}"),
        }
    }

    /// The component by axis, as `v[axis]` reads it in the
    /// JavaScript (a porting convenience; three has no Axis type).
    pub fn get(&self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.x,
            Axis::Y => self.y,
            Axis::Z => self.z,
        }
    }

    /// `v[axis] = value` in the JavaScript.
    pub fn set_axis(&mut self, axis: Axis, value: f64) {
        match axis {
            Axis::X => self.x = value,
            Axis::Y => self.y = value,
            Axis::Z => self.z = value,
        }
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

    pub fn add_scalar(&mut self, s: f64) -> &mut Self {
        self.x += s;
        self.y += s;
        self.z += s;
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

    pub fn sub_scalar(&mut self, s: f64) -> &mut Self {
        self.x -= s;
        self.y -= s;
        self.z -= s;
        self
    }

    pub fn sub_vectors(&mut self, a: &Vector3, b: &Vector3) -> &mut Self {
        self.x = a.x - b.x;
        self.y = a.y - b.y;
        self.z = a.z - b.z;
        self
    }

    pub fn multiply(&mut self, v: &Vector3) -> &mut Self {
        self.x *= v.x;
        self.y *= v.y;
        self.z *= v.z;
        self
    }

    pub fn multiply_scalar(&mut self, scalar: f64) -> &mut Self {
        self.x *= scalar;
        self.y *= scalar;
        self.z *= scalar;
        self
    }

    pub fn multiply_vectors(&mut self, a: &Vector3, b: &Vector3) -> &mut Self {
        self.x = a.x * b.x;
        self.y = a.y * b.y;
        self.z = a.z * b.z;
        self
    }

    /// `applyEuler(euler)`: through a fresh quaternion, as three's
    /// module `_quaternion` does.
    pub fn apply_euler(&mut self, euler: &Euler) -> &mut Self {
        let mut q = Quaternion::default();
        q.set_from_euler(euler);
        self.apply_quaternion(&q)
    }

    /// `applyAxisAngle(axis, angle)`.
    pub fn apply_axis_angle(&mut self, axis: &Vector3, angle: f64) -> &mut Self {
        let mut q = Quaternion::default();
        q.set_from_axis_angle(axis, angle);
        self.apply_quaternion(&q)
    }

    /// `applyMatrix3(m)`.
    pub fn apply_matrix3(&mut self, m: &Matrix3) -> &mut Self {
        let (x, y, z) = (self.x, self.y, self.z);
        let e = &m.elements;
        self.x = e[0] * x + e[3] * y + e[6] * z;
        self.y = e[1] * x + e[4] * y + e[7] * z;
        self.z = e[2] * x + e[5] * y + e[8] * z;
        self
    }

    /// `applyNormalMatrix(m)`.
    pub fn apply_normal_matrix(&mut self, m: &Matrix3) -> &mut Self {
        self.apply_matrix3(m);
        self.normalize()
    }

    /// `applyMatrix4(m)`: note the reciprocal w, multiplied.
    pub fn apply_matrix4(&mut self, m: &Matrix4) -> &mut Self {
        let (x, y, z) = (self.x, self.y, self.z);
        let e = &m.elements;
        let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
        self.x = (e[0] * x + e[4] * y + e[8] * z + e[12]) * w;
        self.y = (e[1] * x + e[5] * y + e[9] * z + e[13]) * w;
        self.z = (e[2] * x + e[6] * y + e[10] * z + e[14]) * w;
        self
    }

    /// `applyQuaternion(q)`: assumes unit length.
    pub fn apply_quaternion(&mut self, q: &Quaternion) -> &mut Self {
        let (vx, vy, vz) = (self.x, self.y, self.z);
        let (qx, qy, qz, qw) = (q.x, q.y, q.z, q.w);
        let tx = 2.0 * (qy * vz - qz * vy);
        let ty = 2.0 * (qz * vx - qx * vz);
        let tz = 2.0 * (qx * vy - qy * vx);
        self.x = vx + qw * tx + qy * tz - qz * ty;
        self.y = vy + qw * ty + qz * tx - qx * tz;
        self.z = vz + qw * tz + qx * ty - qy * tx;
        self
    }

    /// `project(camera)`: the two matrices are the arguments
    /// (`camera.matrixWorldInverse`, `camera.projectionMatrix`).
    pub fn project(&mut self, matrix_world_inverse: &Matrix4, projection_matrix: &Matrix4) -> &mut Self {
        self.apply_matrix4(matrix_world_inverse);
        self.apply_matrix4(projection_matrix)
    }

    /// `unproject(camera)` (`camera.projectionMatrixInverse`,
    /// `camera.matrixWorld`).
    pub fn unproject(&mut self, projection_matrix_inverse: &Matrix4, matrix_world: &Matrix4) -> &mut Self {
        self.apply_matrix4(projection_matrix_inverse);
        self.apply_matrix4(matrix_world)
    }

    /// `transformDirection(m)`.
    pub fn transform_direction(&mut self, m: &Matrix4) -> &mut Self {
        let (x, y, z) = (self.x, self.y, self.z);
        let e = &m.elements;
        self.x = e[0] * x + e[4] * y + e[8] * z;
        self.y = e[1] * x + e[5] * y + e[9] * z;
        self.z = e[2] * x + e[6] * y + e[10] * z;
        self.normalize()
    }

    pub fn divide(&mut self, v: &Vector3) -> &mut Self {
        self.x /= v.x;
        self.y /= v.y;
        self.z /= v.z;
        self
    }

    /// `divideScalar(scalar)`: `multiplyScalar(1 / scalar)`.
    pub fn divide_scalar(&mut self, scalar: f64) -> &mut Self {
        self.multiply_scalar(1.0 / scalar)
    }

    pub fn min(&mut self, v: &Vector3) -> &mut Self {
        self.x = js_min(self.x, v.x);
        self.y = js_min(self.y, v.y);
        self.z = js_min(self.z, v.z);
        self
    }

    pub fn max(&mut self, v: &Vector3) -> &mut Self {
        self.x = js_max(self.x, v.x);
        self.y = js_max(self.y, v.y);
        self.z = js_max(self.z, v.z);
        self
    }

    /// `clamp(min, max)`: assumes min < max componentwise.
    pub fn clamp(&mut self, min: &Vector3, max: &Vector3) -> &mut Self {
        self.x = js_max(min.x, js_min(max.x, self.x));
        self.y = js_max(min.y, js_min(max.y, self.y));
        self.z = js_max(min.z, js_min(max.z, self.z));
        self
    }

    pub fn clamp_scalar(&mut self, min_val: f64, max_val: f64) -> &mut Self {
        self.x = js_max(min_val, js_min(max_val, self.x));
        self.y = js_max(min_val, js_min(max_val, self.y));
        self.z = js_max(min_val, js_min(max_val, self.z));
        self
    }

    pub fn clamp_length(&mut self, min: f64, max: f64) -> &mut Self {
        let length = self.length();
        self.divide_scalar(length_or_1(length));
        self.multiply_scalar(js_max(min, js_min(max, length)))
    }

    pub fn floor(&mut self) -> &mut Self {
        self.x = self.x.floor();
        self.y = self.y.floor();
        self.z = self.z.floor();
        self
    }

    pub fn ceil(&mut self) -> &mut Self {
        self.x = self.x.ceil();
        self.y = self.y.ceil();
        self.z = self.z.ceil();
        self
    }

    pub fn round(&mut self) -> &mut Self {
        self.x = js_round(self.x);
        self.y = js_round(self.y);
        self.z = js_round(self.z);
        self
    }

    /// `roundToZero()`: `Math.trunc` per component.
    pub fn round_to_zero(&mut self) -> &mut Self {
        self.x = self.x.trunc();
        self.y = self.y.trunc();
        self.z = self.z.trunc();
        self
    }

    pub fn negate(&mut self) -> &mut Self {
        self.x = -self.x;
        self.y = -self.y;
        self.z = -self.z;
        self
    }

    pub fn dot(&self, v: &Vector3) -> f64 {
        self.x * v.x + self.y * v.y + self.z * v.z
    }

    pub fn length_sq(&self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    /// `length()`: `Math.sqrt(x*x + y*y + z*z)` — not `hypot`.
    pub fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn manhattan_length(&self) -> f64 {
        self.x.abs() + self.y.abs() + self.z.abs()
    }

    /// `normalize()`: `divideScalar(this.length() || 1)`.
    pub fn normalize(&mut self) -> &mut Self {
        let l = self.length();
        self.divide_scalar(length_or_1(l))
    }

    pub fn set_length(&mut self, length: f64) -> &mut Self {
        self.normalize();
        self.multiply_scalar(length)
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

    /// `projectOnVector(v)`.
    pub fn project_on_vector(&mut self, v: &Vector3) -> &mut Self {
        let denominator = v.length_sq();
        if denominator == 0.0 {
            return self.set(0.0, 0.0, 0.0);
        }
        let scalar = v.dot(self) / denominator;
        self.copy(v);
        self.multiply_scalar(scalar)
    }

    /// `projectOnPlane(planeNormal)`.
    pub fn project_on_plane(&mut self, plane_normal: &Vector3) -> &mut Self {
        let mut projected = *self;
        projected.project_on_vector(plane_normal);
        self.sub(&projected)
    }

    /// `reflect(normal)`: assumes unit length. three computes
    /// `2 * this.dot(normal)` against the not-yet-moved vector.
    pub fn reflect(&mut self, normal: &Vector3) -> &mut Self {
        let d = 2.0 * self.dot(normal);
        let mut scaled = *normal;
        scaled.multiply_scalar(d);
        self.sub(&scaled)
    }

    /// `angleTo(v)`.
    pub fn angle_to(&self, v: &Vector3) -> f64 {
        let denominator = (self.length_sq() * v.length_sq()).sqrt();
        if denominator == 0.0 {
            return core::f64::consts::PI / 2.0;
        }
        let theta = self.dot(v) / denominator;
        v8math::acos(clamp(theta, -1.0, 1.0))
    }

    pub fn distance_to(&self, v: &Vector3) -> f64 {
        self.distance_to_squared(v).sqrt()
    }

    pub fn distance_to_squared(&self, v: &Vector3) -> f64 {
        let dx = self.x - v.x;
        let dy = self.y - v.y;
        let dz = self.z - v.z;
        dx * dx + dy * dy + dz * dz
    }

    pub fn manhattan_distance_to(&self, v: &Vector3) -> f64 {
        (self.x - v.x).abs() + (self.y - v.y).abs() + (self.z - v.z).abs()
    }

    pub fn set_from_spherical(&mut self, s: &Spherical) -> &mut Self {
        self.set_from_spherical_coords(s.radius, s.phi, s.theta)
    }

    pub fn set_from_spherical_coords(&mut self, radius: f64, phi: f64, theta: f64) -> &mut Self {
        let sin_phi_radius = v8math::sin(phi) * radius;
        self.x = sin_phi_radius * v8math::sin(theta);
        self.y = v8math::cos(phi) * radius;
        self.z = sin_phi_radius * v8math::cos(theta);
        self
    }

    pub fn set_from_cylindrical(&mut self, c: &Cylindrical) -> &mut Self {
        self.set_from_cylindrical_coords(c.radius, c.theta, c.y)
    }

    pub fn set_from_cylindrical_coords(&mut self, radius: f64, theta: f64, y: f64) -> &mut Self {
        self.x = radius * v8math::sin(theta);
        self.y = y;
        self.z = radius * v8math::cos(theta);
        self
    }

    pub fn set_from_matrix_position(&mut self, m: &Matrix4) -> &mut Self {
        let e = &m.elements;
        self.x = e[12];
        self.y = e[13];
        self.z = e[14];
        self
    }

    /// `setFromMatrixScale(m)`: note three walks the columns through
    /// `this`, so the lengths are taken column by column in order;
    /// the values are the three column lengths.
    pub fn set_from_matrix_scale(&mut self, m: &Matrix4) -> &mut Self {
        let mut column = Vector3::default();
        let sx = column.set_from_matrix_column(m, 0).length();
        let sy = column.set_from_matrix_column(m, 1).length();
        let sz = column.set_from_matrix_column(m, 2).length();
        self.x = sx;
        self.y = sy;
        self.z = sz;
        self
    }

    pub fn set_from_matrix_column(&mut self, m: &Matrix4, index: usize) -> &mut Self {
        self.from_array(&m.elements, index * 4)
    }

    pub fn set_from_matrix3_column(&mut self, m: &Matrix3, index: usize) -> &mut Self {
        self.from_array(&m.elements, index * 3)
    }

    /// `setFromEuler(e)`: the three angles as components.
    pub fn set_from_euler(&mut self, e: &Euler) -> &mut Self {
        self.x = e.x;
        self.y = e.y;
        self.z = e.z;
        self
    }

    /// `equals(v)`: `===` per component (so NaN never equals, and
    /// -0 equals 0, exactly as JavaScript's `===` on numbers).
    pub fn equals(&self, v: &Vector3) -> bool {
        v.x == self.x && v.y == self.y && v.z == self.z
    }

    pub fn from_array(&mut self, array: &[f64], offset: usize) -> &mut Self {
        self.x = array[offset];
        self.y = array[offset + 1];
        self.z = array[offset + 2];
        self
    }

    /// Reads through f32 storage (a Float32Array in JS): each value
    /// widens as JavaScript reads it.
    pub fn from_array_f32(&mut self, array: &[f32], offset: usize) -> &mut Self {
        self.x = array[offset] as f64;
        self.y = array[offset + 1] as f64;
        self.z = array[offset + 2] as f64;
        self
    }

    pub fn to_array<'t>(&self, array: &'t mut [f64], offset: usize) -> &'t mut [f64] {
        array[offset] = self.x;
        array[offset + 1] = self.y;
        array[offset + 2] = self.z;
        array
    }

    /// `random()` with the three draws handed in, in order x, y, z.
    pub fn random(&mut self, random: &mut impl FnMut() -> f64) -> &mut Self {
        self.x = random();
        self.y = random();
        self.z = random();
        self
    }

    /// `randomDirection()` with the two draws handed in (theta's,
    /// then u's), in three's order.
    pub fn random_direction(&mut self, random: &mut impl FnMut() -> f64) -> &mut Self {
        let theta = random() * core::f64::consts::PI * 2.0;
        let u = random() * 2.0 - 1.0;
        let c = (1.0 - u * u).sqrt();
        self.x = c * v8math::cos(theta);
        self.y = u;
        self.z = c * v8math::sin(theta);
        self
    }
}

/// `THREE.Vector2`, r170. (three's `width`/`height` getters alias
/// x/y; use the fields.)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector2 {
    pub x: f64,
    pub y: f64,
}

impl Vector2 {
    pub const fn new(x: f64, y: f64) -> Self {
        Vector2 { x, y }
    }

    pub fn set(&mut self, x: f64, y: f64) -> &mut Self {
        self.x = x;
        self.y = y;
        self
    }

    pub fn set_scalar(&mut self, scalar: f64) -> &mut Self {
        self.x = scalar;
        self.y = scalar;
        self
    }

    pub fn set_x(&mut self, x: f64) -> &mut Self {
        self.x = x;
        self
    }

    pub fn set_y(&mut self, y: f64) -> &mut Self {
        self.y = y;
        self
    }

    pub fn set_component(&mut self, index: usize, value: f64) -> &mut Self {
        match index {
            0 => self.x = value,
            1 => self.y = value,
            _ => panic!("index is out of range: {index}"),
        }
        self
    }

    pub fn get_component(&self, index: usize) -> f64 {
        match index {
            0 => self.x,
            1 => self.y,
            _ => panic!("index is out of range: {index}"),
        }
    }

    pub fn copy(&mut self, v: &Vector2) -> &mut Self {
        self.x = v.x;
        self.y = v.y;
        self
    }

    pub fn add(&mut self, v: &Vector2) -> &mut Self {
        self.x += v.x;
        self.y += v.y;
        self
    }

    pub fn add_scalar(&mut self, s: f64) -> &mut Self {
        self.x += s;
        self.y += s;
        self
    }

    pub fn add_vectors(&mut self, a: &Vector2, b: &Vector2) -> &mut Self {
        self.x = a.x + b.x;
        self.y = a.y + b.y;
        self
    }

    pub fn add_scaled_vector(&mut self, v: &Vector2, s: f64) -> &mut Self {
        self.x += v.x * s;
        self.y += v.y * s;
        self
    }

    pub fn sub(&mut self, v: &Vector2) -> &mut Self {
        self.x -= v.x;
        self.y -= v.y;
        self
    }

    pub fn sub_scalar(&mut self, s: f64) -> &mut Self {
        self.x -= s;
        self.y -= s;
        self
    }

    pub fn sub_vectors(&mut self, a: &Vector2, b: &Vector2) -> &mut Self {
        self.x = a.x - b.x;
        self.y = a.y - b.y;
        self
    }

    pub fn multiply(&mut self, v: &Vector2) -> &mut Self {
        self.x *= v.x;
        self.y *= v.y;
        self
    }

    pub fn multiply_scalar(&mut self, scalar: f64) -> &mut Self {
        self.x *= scalar;
        self.y *= scalar;
        self
    }

    pub fn divide(&mut self, v: &Vector2) -> &mut Self {
        self.x /= v.x;
        self.y /= v.y;
        self
    }

    pub fn divide_scalar(&mut self, scalar: f64) -> &mut Self {
        self.multiply_scalar(1.0 / scalar)
    }

    /// `applyMatrix3(m)`: r170 applies the affine part only, with
    /// no perspective divide.
    pub fn apply_matrix3(&mut self, m: &Matrix3) -> &mut Self {
        let (x, y) = (self.x, self.y);
        let e = &m.elements;
        self.x = e[0] * x + e[3] * y + e[6];
        self.y = e[1] * x + e[4] * y + e[7];
        self
    }

    pub fn min(&mut self, v: &Vector2) -> &mut Self {
        self.x = js_min(self.x, v.x);
        self.y = js_min(self.y, v.y);
        self
    }

    pub fn max(&mut self, v: &Vector2) -> &mut Self {
        self.x = js_max(self.x, v.x);
        self.y = js_max(self.y, v.y);
        self
    }

    pub fn clamp(&mut self, min: &Vector2, max: &Vector2) -> &mut Self {
        self.x = js_max(min.x, js_min(max.x, self.x));
        self.y = js_max(min.y, js_min(max.y, self.y));
        self
    }

    pub fn clamp_scalar(&mut self, min_val: f64, max_val: f64) -> &mut Self {
        self.x = js_max(min_val, js_min(max_val, self.x));
        self.y = js_max(min_val, js_min(max_val, self.y));
        self
    }

    pub fn clamp_length(&mut self, min: f64, max: f64) -> &mut Self {
        let length = self.length();
        self.divide_scalar(length_or_1(length));
        self.multiply_scalar(js_max(min, js_min(max, length)))
    }

    pub fn floor(&mut self) -> &mut Self {
        self.x = self.x.floor();
        self.y = self.y.floor();
        self
    }

    pub fn ceil(&mut self) -> &mut Self {
        self.x = self.x.ceil();
        self.y = self.y.ceil();
        self
    }

    pub fn round(&mut self) -> &mut Self {
        self.x = js_round(self.x);
        self.y = js_round(self.y);
        self
    }

    pub fn round_to_zero(&mut self) -> &mut Self {
        self.x = self.x.trunc();
        self.y = self.y.trunc();
        self
    }

    pub fn negate(&mut self) -> &mut Self {
        self.x = -self.x;
        self.y = -self.y;
        self
    }

    pub fn dot(&self, v: &Vector2) -> f64 {
        self.x * v.x + self.y * v.y
    }

    /// `cross(v)`: the scalar z of the 2D cross product.
    pub fn cross(&self, v: &Vector2) -> f64 {
        self.x * v.y - self.y * v.x
    }

    pub fn length_sq(&self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    pub fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn manhattan_length(&self) -> f64 {
        self.x.abs() + self.y.abs()
    }

    pub fn normalize(&mut self) -> &mut Self {
        let l = self.length();
        self.divide_scalar(length_or_1(l))
    }

    /// `angle()`: `Math.atan2(-y, -x) + Math.PI`.
    pub fn angle(&self) -> f64 {
        v8math::atan2(-self.y, -self.x) + core::f64::consts::PI
    }

    pub fn angle_to(&self, v: &Vector2) -> f64 {
        let denominator = (self.length_sq() * v.length_sq()).sqrt();
        if denominator == 0.0 {
            return core::f64::consts::PI / 2.0;
        }
        let theta = self.dot(v) / denominator;
        v8math::acos(clamp(theta, -1.0, 1.0))
    }

    pub fn distance_to(&self, v: &Vector2) -> f64 {
        self.distance_to_squared(v).sqrt()
    }

    pub fn distance_to_squared(&self, v: &Vector2) -> f64 {
        let dx = self.x - v.x;
        let dy = self.y - v.y;
        dx * dx + dy * dy
    }

    pub fn manhattan_distance_to(&self, v: &Vector2) -> f64 {
        (self.x - v.x).abs() + (self.y - v.y).abs()
    }

    pub fn set_length(&mut self, length: f64) -> &mut Self {
        self.normalize();
        self.multiply_scalar(length)
    }

    pub fn lerp(&mut self, v: &Vector2, alpha: f64) -> &mut Self {
        self.x += (v.x - self.x) * alpha;
        self.y += (v.y - self.y) * alpha;
        self
    }

    pub fn lerp_vectors(&mut self, v1: &Vector2, v2: &Vector2, alpha: f64) -> &mut Self {
        self.x = v1.x + (v2.x - v1.x) * alpha;
        self.y = v1.y + (v2.y - v1.y) * alpha;
        self
    }

    pub fn equals(&self, v: &Vector2) -> bool {
        v.x == self.x && v.y == self.y
    }

    pub fn from_array(&mut self, array: &[f64], offset: usize) -> &mut Self {
        self.x = array[offset];
        self.y = array[offset + 1];
        self
    }

    pub fn from_array_f32(&mut self, array: &[f32], offset: usize) -> &mut Self {
        self.x = array[offset] as f64;
        self.y = array[offset + 1] as f64;
        self
    }

    pub fn to_array<'t>(&self, array: &'t mut [f64], offset: usize) -> &'t mut [f64] {
        array[offset] = self.x;
        array[offset + 1] = self.y;
        array
    }

    /// `rotateAround(center, angle)`.
    pub fn rotate_around(&mut self, center: &Vector2, angle: f64) -> &mut Self {
        let c = v8math::cos(angle);
        let s = v8math::sin(angle);
        let x = self.x - center.x;
        let y = self.y - center.y;
        self.x = x * c - y * s + center.x;
        self.y = x * s + y * c + center.y;
        self
    }

    /// `random()` with the two draws handed in.
    pub fn random(&mut self, random: &mut impl FnMut() -> f64) -> &mut Self {
        self.x = random();
        self.y = random();
        self
    }
}

/// `THREE.Vector4`, r170. (`width`/`height` in three are getters
/// for z/w; read and write the fields here.)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector4 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Default for Vector4 {
    /// `new Vector4()`: (0, 0, 0, 1).
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0, 1.0)
    }
}

impl Vector4 {
    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Vector4 { x, y, z, w }
    }

    pub fn set(&mut self, x: f64, y: f64, z: f64, w: f64) -> &mut Self {
        self.x = x;
        self.y = y;
        self.z = z;
        self.w = w;
        self
    }

    pub fn set_scalar(&mut self, scalar: f64) -> &mut Self {
        self.x = scalar;
        self.y = scalar;
        self.z = scalar;
        self.w = scalar;
        self
    }

    pub fn set_x(&mut self, x: f64) -> &mut Self {
        self.x = x;
        self
    }

    pub fn set_y(&mut self, y: f64) -> &mut Self {
        self.y = y;
        self
    }

    pub fn set_z(&mut self, z: f64) -> &mut Self {
        self.z = z;
        self
    }

    pub fn set_w(&mut self, w: f64) -> &mut Self {
        self.w = w;
        self
    }

    pub fn set_component(&mut self, index: usize, value: f64) -> &mut Self {
        match index {
            0 => self.x = value,
            1 => self.y = value,
            2 => self.z = value,
            3 => self.w = value,
            _ => panic!("index is out of range: {index}"),
        }
        self
    }

    pub fn get_component(&self, index: usize) -> f64 {
        match index {
            0 => self.x,
            1 => self.y,
            2 => self.z,
            3 => self.w,
            _ => panic!("index is out of range: {index}"),
        }
    }

    /// `copy(v)`: note three's `v.w !== undefined ? v.w : 1` — a
    /// Vector4 always has a w here, so it is a plain copy; copying
    /// from a Vector3 in three gives w = 1, which is
    /// [`Vector4::copy_vector3`].
    pub fn copy(&mut self, v: &Vector4) -> &mut Self {
        self.x = v.x;
        self.y = v.y;
        self.z = v.z;
        self.w = v.w;
        self
    }

    /// `copy(v)` where three was handed a Vector3 (w reads
    /// undefined, so it becomes 1).
    pub fn copy_vector3(&mut self, v: &Vector3) -> &mut Self {
        self.x = v.x;
        self.y = v.y;
        self.z = v.z;
        self.w = 1.0;
        self
    }

    pub fn add(&mut self, v: &Vector4) -> &mut Self {
        self.x += v.x;
        self.y += v.y;
        self.z += v.z;
        self.w += v.w;
        self
    }

    pub fn add_scalar(&mut self, s: f64) -> &mut Self {
        self.x += s;
        self.y += s;
        self.z += s;
        self.w += s;
        self
    }

    pub fn add_vectors(&mut self, a: &Vector4, b: &Vector4) -> &mut Self {
        self.x = a.x + b.x;
        self.y = a.y + b.y;
        self.z = a.z + b.z;
        self.w = a.w + b.w;
        self
    }

    pub fn add_scaled_vector(&mut self, v: &Vector4, s: f64) -> &mut Self {
        self.x += v.x * s;
        self.y += v.y * s;
        self.z += v.z * s;
        self.w += v.w * s;
        self
    }

    pub fn sub(&mut self, v: &Vector4) -> &mut Self {
        self.x -= v.x;
        self.y -= v.y;
        self.z -= v.z;
        self.w -= v.w;
        self
    }

    pub fn sub_scalar(&mut self, s: f64) -> &mut Self {
        self.x -= s;
        self.y -= s;
        self.z -= s;
        self.w -= s;
        self
    }

    pub fn sub_vectors(&mut self, a: &Vector4, b: &Vector4) -> &mut Self {
        self.x = a.x - b.x;
        self.y = a.y - b.y;
        self.z = a.z - b.z;
        self.w = a.w - b.w;
        self
    }

    pub fn multiply(&mut self, v: &Vector4) -> &mut Self {
        self.x *= v.x;
        self.y *= v.y;
        self.z *= v.z;
        self.w *= v.w;
        self
    }

    pub fn multiply_scalar(&mut self, scalar: f64) -> &mut Self {
        self.x *= scalar;
        self.y *= scalar;
        self.z *= scalar;
        self.w *= scalar;
        self
    }

    pub fn apply_matrix4(&mut self, m: &Matrix4) -> &mut Self {
        let (x, y, z, w) = (self.x, self.y, self.z, self.w);
        let e = &m.elements;
        self.x = e[0] * x + e[4] * y + e[8] * z + e[12] * w;
        self.y = e[1] * x + e[5] * y + e[9] * z + e[13] * w;
        self.z = e[2] * x + e[6] * y + e[10] * z + e[14] * w;
        self.w = e[3] * x + e[7] * y + e[11] * z + e[15] * w;
        self
    }

    pub fn divide(&mut self, v: &Vector4) -> &mut Self {
        self.x /= v.x;
        self.y /= v.y;
        self.z /= v.z;
        self.w /= v.w;
        self
    }

    pub fn divide_scalar(&mut self, scalar: f64) -> &mut Self {
        self.multiply_scalar(1.0 / scalar)
    }

    /// `setAxisAngleFromQuaternion(q)`.
    pub fn set_axis_angle_from_quaternion(&mut self, q: &Quaternion) -> &mut Self {
        self.w = 2.0 * v8math::acos(q.w);
        let s = (1.0 - q.w * q.w).sqrt();
        if s < 0.0001 {
            self.x = 1.0;
            self.y = 0.0;
            self.z = 0.0;
        } else {
            self.x = q.x / s;
            self.y = q.y / s;
            self.z = q.z / s;
        }
        self
    }

    /// `setAxisAngleFromRotationMatrix(m)`, with three's epsilon
    /// branches and its 0.707106781 constants (three's literal, not
    /// FRAC_1_SQRT_2: a different double).
    #[allow(clippy::approx_constant)]
    pub fn set_axis_angle_from_rotation_matrix(&mut self, m: &Matrix4) -> &mut Self {
        let epsilon = 0.01f64;
        let epsilon2 = 0.1f64;
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
        if (m12 - m21).abs() < epsilon && (m13 - m31).abs() < epsilon && (m23 - m32).abs() < epsilon
        {
            if (m12 + m21).abs() < epsilon2
                && (m13 + m31).abs() < epsilon2
                && (m23 + m32).abs() < epsilon2
                && (m11 + m22 + m33 - 3.0).abs() < epsilon2
            {
                self.set(1.0, 0.0, 0.0, 0.0);
                return self;
            }
            let angle = core::f64::consts::PI;
            let xx = (m11 + 1.0) / 2.0;
            let yy = (m22 + 1.0) / 2.0;
            let zz = (m33 + 1.0) / 2.0;
            let xy = (m12 + m21) / 4.0;
            let xz = (m13 + m31) / 4.0;
            let yz = (m23 + m32) / 4.0;
            let (x, y, z);
            if xx > yy && xx > zz {
                if xx < epsilon {
                    x = 0.0;
                    y = 0.707106781;
                    z = 0.707106781;
                } else {
                    x = xx.sqrt();
                    y = xy / x;
                    z = xz / x;
                }
            } else if yy > zz {
                if yy < epsilon {
                    x = 0.707106781;
                    y = 0.0;
                    z = 0.707106781;
                } else {
                    y = yy.sqrt();
                    x = xy / y;
                    z = yz / y;
                }
            } else if zz < epsilon {
                x = 0.707106781;
                y = 0.707106781;
                z = 0.0;
            } else {
                z = zz.sqrt();
                x = xz / z;
                y = yz / z;
            }
            self.set(x, y, z, angle);
            return self;
        }
        let mut s = ((m32 - m23) * (m32 - m23)
            + (m13 - m31) * (m13 - m31)
            + (m21 - m12) * (m21 - m12))
            .sqrt();
        if s.abs() < 0.001 {
            s = 1.0;
        }
        self.x = (m32 - m23) / s;
        self.y = (m13 - m31) / s;
        self.z = (m21 - m12) / s;
        self.w = v8math::acos((m11 + m22 + m33 - 1.0) / 2.0);
        self
    }

    pub fn set_from_matrix_position(&mut self, m: &Matrix4) -> &mut Self {
        let e = &m.elements;
        self.x = e[12];
        self.y = e[13];
        self.z = e[14];
        self.w = e[15];
        self
    }

    pub fn min(&mut self, v: &Vector4) -> &mut Self {
        self.x = js_min(self.x, v.x);
        self.y = js_min(self.y, v.y);
        self.z = js_min(self.z, v.z);
        self.w = js_min(self.w, v.w);
        self
    }

    pub fn max(&mut self, v: &Vector4) -> &mut Self {
        self.x = js_max(self.x, v.x);
        self.y = js_max(self.y, v.y);
        self.z = js_max(self.z, v.z);
        self.w = js_max(self.w, v.w);
        self
    }

    pub fn clamp(&mut self, min: &Vector4, max: &Vector4) -> &mut Self {
        self.x = js_max(min.x, js_min(max.x, self.x));
        self.y = js_max(min.y, js_min(max.y, self.y));
        self.z = js_max(min.z, js_min(max.z, self.z));
        self.w = js_max(min.w, js_min(max.w, self.w));
        self
    }

    pub fn clamp_scalar(&mut self, min_val: f64, max_val: f64) -> &mut Self {
        self.x = js_max(min_val, js_min(max_val, self.x));
        self.y = js_max(min_val, js_min(max_val, self.y));
        self.z = js_max(min_val, js_min(max_val, self.z));
        self.w = js_max(min_val, js_min(max_val, self.w));
        self
    }

    pub fn clamp_length(&mut self, min: f64, max: f64) -> &mut Self {
        let length = self.length();
        self.divide_scalar(length_or_1(length));
        self.multiply_scalar(js_max(min, js_min(max, length)))
    }

    pub fn floor(&mut self) -> &mut Self {
        self.x = self.x.floor();
        self.y = self.y.floor();
        self.z = self.z.floor();
        self.w = self.w.floor();
        self
    }

    pub fn ceil(&mut self) -> &mut Self {
        self.x = self.x.ceil();
        self.y = self.y.ceil();
        self.z = self.z.ceil();
        self.w = self.w.ceil();
        self
    }

    pub fn round(&mut self) -> &mut Self {
        self.x = js_round(self.x);
        self.y = js_round(self.y);
        self.z = js_round(self.z);
        self.w = js_round(self.w);
        self
    }

    pub fn round_to_zero(&mut self) -> &mut Self {
        self.x = self.x.trunc();
        self.y = self.y.trunc();
        self.z = self.z.trunc();
        self.w = self.w.trunc();
        self
    }

    pub fn negate(&mut self) -> &mut Self {
        self.x = -self.x;
        self.y = -self.y;
        self.z = -self.z;
        self.w = -self.w;
        self
    }

    pub fn dot(&self, v: &Vector4) -> f64 {
        self.x * v.x + self.y * v.y + self.z * v.z + self.w * v.w
    }

    pub fn length_sq(&self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    pub fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt()
    }

    pub fn manhattan_length(&self) -> f64 {
        self.x.abs() + self.y.abs() + self.z.abs() + self.w.abs()
    }

    pub fn normalize(&mut self) -> &mut Self {
        let l = self.length();
        self.divide_scalar(length_or_1(l))
    }

    pub fn set_length(&mut self, length: f64) -> &mut Self {
        self.normalize();
        self.multiply_scalar(length)
    }

    pub fn lerp(&mut self, v: &Vector4, alpha: f64) -> &mut Self {
        self.x += (v.x - self.x) * alpha;
        self.y += (v.y - self.y) * alpha;
        self.z += (v.z - self.z) * alpha;
        self.w += (v.w - self.w) * alpha;
        self
    }

    pub fn lerp_vectors(&mut self, v1: &Vector4, v2: &Vector4, alpha: f64) -> &mut Self {
        self.x = v1.x + (v2.x - v1.x) * alpha;
        self.y = v1.y + (v2.y - v1.y) * alpha;
        self.z = v1.z + (v2.z - v1.z) * alpha;
        self.w = v1.w + (v2.w - v1.w) * alpha;
        self
    }

    pub fn equals(&self, v: &Vector4) -> bool {
        v.x == self.x && v.y == self.y && v.z == self.z && v.w == self.w
    }

    pub fn from_array(&mut self, array: &[f64], offset: usize) -> &mut Self {
        self.x = array[offset];
        self.y = array[offset + 1];
        self.z = array[offset + 2];
        self.w = array[offset + 3];
        self
    }

    pub fn from_array_f32(&mut self, array: &[f32], offset: usize) -> &mut Self {
        self.x = array[offset] as f64;
        self.y = array[offset + 1] as f64;
        self.z = array[offset + 2] as f64;
        self.w = array[offset + 3] as f64;
        self
    }

    pub fn to_array<'t>(&self, array: &'t mut [f64], offset: usize) -> &'t mut [f64] {
        array[offset] = self.x;
        array[offset + 1] = self.y;
        array[offset + 2] = self.z;
        array[offset + 3] = self.w;
        array
    }

    /// `random()` with the four draws handed in.
    pub fn random(&mut self, random: &mut impl FnMut() -> f64) -> &mut Self {
        self.x = random();
        self.y = random();
        self.z = random();
        self.w = random();
        self
    }
}
