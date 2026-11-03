//! The three.js r170 operations content loading runs (a bone's local matrix
//! decomposed and recomposed, world matrices, inverses, bounding boxes and
//! spheres, points carried through matrices), each transliterated operation
//! by operation from node_modules/three/src/math so its results are three's
//! own to the last bit. `rust/three` (the port's three.js crate) supersedes
//! this once it is part of the build.

use crate::js::{js_max, js_min};

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

    /// Vector3.fromArray(array, offset).
    pub fn from_slice(a: &[f64]) -> Self {
        Vector3 { x: a[0], y: a[1], z: a[2] }
    }

    pub fn to_array(&self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }

    /// Vector3.applyMatrix4: with the perspective divide.
    pub fn apply_matrix4(&mut self, m: &Matrix4) -> &mut Self {
        let (x, y, z) = (self.x, self.y, self.z);
        let e = &m.elements;
        let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
        self.x = (e[0] * x + e[4] * y + e[8] * z + e[12]) * w;
        self.y = (e[1] * x + e[5] * y + e[9] * z + e[13]) * w;
        self.z = (e[2] * x + e[6] * y + e[10] * z + e[14]) * w;
        self
    }

    /// Vector3.transformDirection (then normalize).
    pub fn transform_direction(&mut self, m: &Matrix4) -> &mut Self {
        let (x, y, z) = (self.x, self.y, self.z);
        let e = &m.elements;
        self.x = e[0] * x + e[4] * y + e[8] * z;
        self.y = e[1] * x + e[5] * y + e[9] * z;
        self.z = e[2] * x + e[6] * y + e[10] * z;
        self.normalize()
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

    pub fn add_vectors(&mut self, a: &Vector3, b: &Vector3) -> &mut Self {
        self.x = a.x + b.x;
        self.y = a.y + b.y;
        self.z = a.z + b.z;
        self
    }

    pub fn multiply_scalar(&mut self, s: f64) -> &mut Self {
        self.x *= s;
        self.y *= s;
        self.z *= s;
        self
    }

    pub fn divide_scalar(&mut self, s: f64) -> &mut Self {
        self.multiply_scalar(1.0 / s)
    }

    pub fn length(&self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// Vector3.normalize: divides by the length, or by 1 when it is 0
    /// (or NaN: `length() || 1`).
    pub fn normalize(&mut self) -> &mut Self {
        let l = self.length();
        self.divide_scalar(if l == 0.0 || l.is_nan() { 1.0 } else { l })
    }

    pub fn distance_to_squared(&self, v: &Vector3) -> f64 {
        let dx = self.x - v.x;
        let dy = self.y - v.y;
        let dz = self.z - v.z;
        dx * dx + dy * dy + dz * dz
    }

    pub fn set_from_matrix_position(&mut self, m: &Matrix4) -> &mut Self {
        self.x = m.elements[12];
        self.y = m.elements[13];
        self.z = m.elements[14];
        self
    }

    pub fn set_from_matrix_column(&mut self, m: &Matrix4, index: usize) -> &mut Self {
        *self = Vector3::from_slice(&m.elements[index * 4..]);
        self
    }
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
    /// Quaternion.setFromRotationMatrix (the upper 3x3 a pure rotation).
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
}

/// Column-major, as three's `elements`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix4 {
    pub elements: [f64; 16],
}

impl Default for Matrix4 {
    fn default() -> Self {
        Matrix4::identity()
    }
}

impl Matrix4 {
    pub const fn identity() -> Self {
        Matrix4 { elements: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] }
    }

    /// Matrix4.set, row-major arguments.
    #[allow(clippy::too_many_arguments)]
    pub fn set(
        &mut self, n11: f64, n12: f64, n13: f64, n14: f64, n21: f64, n22: f64, n23: f64, n24: f64, n31: f64, n32: f64,
        n33: f64, n34: f64, n41: f64, n42: f64, n43: f64, n44: f64,
    ) -> &mut Self {
        let te = &mut self.elements;
        te[0] = n11;
        te[4] = n12;
        te[8] = n13;
        te[12] = n14;
        te[1] = n21;
        te[5] = n22;
        te[9] = n23;
        te[13] = n24;
        te[2] = n31;
        te[6] = n32;
        te[10] = n33;
        te[14] = n34;
        te[3] = n41;
        te[7] = n42;
        te[11] = n43;
        te[15] = n44;
        self
    }

    /// Matrix4.fromArray: the 16 values as they are.
    pub fn from_array(a: &[f64]) -> Self {
        let mut elements = [0.0; 16];
        elements.copy_from_slice(&a[..16]);
        Matrix4 { elements }
    }

    /// Matrix4.multiplyMatrices(a, b): a x b.
    pub fn multiply_matrices(a: &Matrix4, b: &Matrix4) -> Matrix4 {
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
        let mut te = [0.0; 16];
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
        Matrix4 { elements: te }
    }

    /// this.multiply(m): this x m.
    pub fn multiply(&mut self, m: &Matrix4) -> &mut Self {
        *self = Matrix4::multiply_matrices(self, m);
        self
    }

    pub fn determinant(&self) -> f64 {
        let te = &self.elements;
        let (n11, n12, n13, n14) = (te[0], te[4], te[8], te[12]);
        let (n21, n22, n23, n24) = (te[1], te[5], te[9], te[13]);
        let (n31, n32, n33, n34) = (te[2], te[6], te[10], te[14]);
        let (n41, n42, n43, n44) = (te[3], te[7], te[11], te[15]);
        n41 * (n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33 + n13 * n22 * n34 - n12 * n23 * n34)
            + n42 * (n11 * n23 * n34 - n11 * n24 * n33 + n14 * n21 * n33 - n13 * n21 * n34 + n13 * n24 * n31 - n14 * n23 * n31)
            + n43 * (n11 * n24 * n32 - n11 * n22 * n34 - n14 * n21 * n32 + n12 * n21 * n34 + n14 * n22 * n31 - n12 * n24 * n31)
            + n44 * (-n13 * n22 * n31 - n11 * n23 * n32 + n11 * n22 * n33 + n13 * n21 * n32 - n12 * n21 * n33 + n12 * n23 * n31)
    }

    /// Matrix4.invert: all zeros when singular.
    pub fn invert(&mut self) -> &mut Self {
        let te = self.elements;
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
        let e = &mut self.elements;
        e[0] = t11 * det_inv;
        e[1] = (n24 * n33 * n41 - n23 * n34 * n41 - n24 * n31 * n43 + n21 * n34 * n43 + n23 * n31 * n44 - n21 * n33 * n44) * det_inv;
        e[2] = (n22 * n34 * n41 - n24 * n32 * n41 + n24 * n31 * n42 - n21 * n34 * n42 - n22 * n31 * n44 + n21 * n32 * n44) * det_inv;
        e[3] = (n23 * n32 * n41 - n22 * n33 * n41 - n23 * n31 * n42 + n21 * n33 * n42 + n22 * n31 * n43 - n21 * n32 * n43) * det_inv;
        e[4] = t12 * det_inv;
        e[5] = (n13 * n34 * n41 - n14 * n33 * n41 + n14 * n31 * n43 - n11 * n34 * n43 - n13 * n31 * n44 + n11 * n33 * n44) * det_inv;
        e[6] = (n14 * n32 * n41 - n12 * n34 * n41 - n14 * n31 * n42 + n11 * n34 * n42 + n12 * n31 * n44 - n11 * n32 * n44) * det_inv;
        e[7] = (n12 * n33 * n41 - n13 * n32 * n41 + n13 * n31 * n42 - n11 * n33 * n42 - n12 * n31 * n43 + n11 * n32 * n43) * det_inv;
        e[8] = t13 * det_inv;
        e[9] = (n14 * n23 * n41 - n13 * n24 * n41 - n14 * n21 * n43 + n11 * n24 * n43 + n13 * n21 * n44 - n11 * n23 * n44) * det_inv;
        e[10] = (n12 * n24 * n41 - n14 * n22 * n41 + n14 * n21 * n42 - n11 * n24 * n42 - n12 * n21 * n44 + n11 * n22 * n44) * det_inv;
        e[11] = (n13 * n22 * n41 - n12 * n23 * n41 - n13 * n21 * n42 + n11 * n23 * n42 + n12 * n21 * n43 - n11 * n22 * n43) * det_inv;
        e[12] = t14 * det_inv;
        e[13] = (n13 * n24 * n31 - n14 * n23 * n31 + n14 * n21 * n33 - n11 * n24 * n33 - n13 * n21 * n34 + n11 * n23 * n34) * det_inv;
        e[14] = (n14 * n22 * n31 - n12 * n24 * n31 - n14 * n21 * n32 + n11 * n24 * n32 + n12 * n21 * n34 - n11 * n22 * n34) * det_inv;
        e[15] = (n12 * n23 * n31 - n13 * n22 * n31 + n13 * n21 * n32 - n11 * n23 * n32 - n12 * n21 * n33 + n11 * n22 * n33) * det_inv;
        self
    }

    pub fn inverted(&self) -> Matrix4 {
        let mut m = *self;
        m.invert();
        m
    }

    /// Matrix4.compose(position, quaternion, scale).
    pub fn compose(position: &Vector3, q: &Quaternion, scale: &Vector3) -> Matrix4 {
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, xy, xz) = (x * x2, x * y2, x * z2);
        let (yy, yz, zz) = (y * y2, y * z2, z * z2);
        let (wx, wy, wz) = (w * x2, w * y2, w * z2);
        let (sx, sy, sz) = (scale.x, scale.y, scale.z);
        let mut te = [0.0; 16];
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
        Matrix4 { elements: te }
    }

    /// Matrix4.decompose: (position, quaternion, scale).
    pub fn decompose(&self) -> (Vector3, Quaternion, Vector3) {
        let te = &self.elements;
        let mut sx = Vector3::new(te[0], te[1], te[2]).length();
        let sy = Vector3::new(te[4], te[5], te[6]).length();
        let sz = Vector3::new(te[8], te[9], te[10]).length();
        // A negative determinant flips one scale.
        let det = self.determinant();
        if det < 0.0 {
            sx = -sx;
        }
        let position = Vector3::new(te[12], te[13], te[14]);
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
        let mut q = Quaternion::default();
        q.set_from_rotation_matrix(&m1);
        (position, q, Vector3::new(sx, sy, sz))
    }

    /// Matrix4.makeBasis(x, y, z): the axes as columns, no translation.
    pub fn make_basis(x: &Vector3, y: &Vector3, z: &Vector3) -> Matrix4 {
        let mut m = Matrix4::identity();
        m.set(x.x, y.x, z.x, 0.0, x.y, y.y, z.y, 0.0, x.z, y.z, z.z, 0.0, 0.0, 0.0, 0.0, 1.0);
        m
    }

    pub fn set_position(&mut self, v: &Vector3) -> &mut Self {
        self.elements[12] = v.x;
        self.elements[13] = v.y;
        self.elements[14] = v.z;
        self
    }

    /// Matrix4.extractBasis: the first three columns.
    pub fn extract_basis(&self) -> [Vector3; 3] {
        [Vector3::from_slice(&self.elements[0..]), Vector3::from_slice(&self.elements[4..]), Vector3::from_slice(&self.elements[8..])]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box3 {
    pub min: Vector3,
    pub max: Vector3,
}

impl Default for Box3 {
    /// new Box3(): empty.
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

    pub fn is_empty(&self) -> bool {
        self.max.x < self.min.x || self.max.y < self.min.y || self.max.z < self.min.z
    }

    pub fn expand_by_point(&mut self, p: &Vector3) -> &mut Self {
        self.min.min(p);
        self.max.max(p);
        self
    }

    /// Box3.setFromPoints.
    pub fn from_points<'a>(points: impl IntoIterator<Item = &'a Vector3>) -> Box3 {
        let mut b = Box3::default();
        for p in points {
            b.expand_by_point(p);
        }
        b
    }

    /// Box3.getCenter.
    pub fn center(&self) -> Vector3 {
        if self.is_empty() {
            Vector3::new(0.0, 0.0, 0.0)
        } else {
            let mut c = Vector3::default();
            c.add_vectors(&self.min, &self.max).multiply_scalar(0.5);
            c
        }
    }

    pub fn union(&mut self, other: &Box3) -> &mut Self {
        self.min.min(&other.min);
        self.max.max(&other.max);
        self
    }
}

/// A position attribute read as three reads one: `count` vertices, each
/// component an f32 widened to f64 (getX/getY/getZ).
pub trait Positions {
    fn count(&self) -> usize;
    fn get(&self, i: usize) -> Vector3;
}

/// BufferGeometry.computeBoundingBox over a position attribute.
pub fn bounding_box(p: &dyn Positions) -> Box3 {
    let mut b = Box3::default();
    for i in 0..p.count() {
        b.expand_by_point(&p.get(i));
    }
    b
}

/// BufferGeometry.computeBoundingSphere over a position attribute: the
/// bounds' centre, and the farthest vertex from it.
pub fn bounding_sphere(p: &dyn Positions) -> (Vector3, f64) {
    let center = bounding_box(p).center();
    let mut max_radius_sq = 0.0f64;
    for i in 0..p.count() {
        max_radius_sq = js_max(max_radius_sq, center.distance_to_squared(&p.get(i)));
    }
    (center, max_radius_sq.sqrt())
}
