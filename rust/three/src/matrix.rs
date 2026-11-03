//! `Matrix3.js` and `Matrix4.js` (r170). Elements are
//! column-major, as three's `elements` array is; `set` takes its
//! sixteen (or nine) arguments in row-major reading order, as
//! three's does.

use jsrt::conv::js_max;
use jsrt::v8math;

use crate::euler::{Euler, EulerOrder};
use crate::quaternion::Quaternion;
use crate::vector::{Vector2, Vector3};

/// `THREE.Matrix3`, r170.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix3 {
    pub elements: [f64; 9],
}

impl Default for Matrix3 {
    /// `new Matrix3()`: the identity.
    fn default() -> Self {
        Matrix3 {
            elements: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }
    }
}

impl Matrix3 {
    /// `set(n11, n12, n13, n21, ...)`: row-major arguments.
    #[allow(clippy::too_many_arguments)]
    pub fn set(
        &mut self,
        n11: f64, n12: f64, n13: f64,
        n21: f64, n22: f64, n23: f64,
        n31: f64, n32: f64, n33: f64,
    ) -> &mut Self {
        let te = &mut self.elements;
        te[0] = n11; te[1] = n21; te[2] = n31;
        te[3] = n12; te[4] = n22; te[5] = n32;
        te[6] = n13; te[7] = n23; te[8] = n33;
        self
    }

    pub fn identity(&mut self) -> &mut Self {
        self.set(1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0)
    }

    pub fn copy(&mut self, m: &Matrix3) -> &mut Self {
        self.elements = m.elements;
        self
    }

    /// `extractBasis(xAxis, yAxis, zAxis)`.
    pub fn extract_basis(&self, x_axis: &mut Vector3, y_axis: &mut Vector3, z_axis: &mut Vector3) {
        x_axis.set_from_matrix3_column(self, 0);
        y_axis.set_from_matrix3_column(self, 1);
        z_axis.set_from_matrix3_column(self, 2);
    }

    pub fn set_from_matrix4(&mut self, m: &Matrix4) -> &mut Self {
        let me = &m.elements;
        self.set(
            me[0], me[4], me[8],
            me[1], me[5], me[9],
            me[2], me[6], me[10],
        )
    }

    pub fn multiply(&mut self, m: &Matrix3) -> &mut Self {
        let a = *self;
        self.multiply_matrices(&a, m)
    }

    pub fn premultiply(&mut self, m: &Matrix3) -> &mut Self {
        let b = *self;
        self.multiply_matrices(m, &b)
    }

    pub fn multiply_matrices(&mut self, a: &Matrix3, b: &Matrix3) -> &mut Self {
        let ae = &a.elements;
        let be = &b.elements;
        let (a11, a12, a13) = (ae[0], ae[3], ae[6]);
        let (a21, a22, a23) = (ae[1], ae[4], ae[7]);
        let (a31, a32, a33) = (ae[2], ae[5], ae[8]);
        let (b11, b12, b13) = (be[0], be[3], be[6]);
        let (b21, b22, b23) = (be[1], be[4], be[7]);
        let (b31, b32, b33) = (be[2], be[5], be[8]);
        let te = &mut self.elements;
        te[0] = a11 * b11 + a12 * b21 + a13 * b31;
        te[3] = a11 * b12 + a12 * b22 + a13 * b32;
        te[6] = a11 * b13 + a12 * b23 + a13 * b33;
        te[1] = a21 * b11 + a22 * b21 + a23 * b31;
        te[4] = a21 * b12 + a22 * b22 + a23 * b32;
        te[7] = a21 * b13 + a22 * b23 + a23 * b33;
        te[2] = a31 * b11 + a32 * b21 + a33 * b31;
        te[5] = a31 * b12 + a32 * b22 + a33 * b32;
        te[8] = a31 * b13 + a32 * b23 + a33 * b33;
        self
    }

    pub fn multiply_scalar(&mut self, s: f64) -> &mut Self {
        for e in self.elements.iter_mut() {
            *e *= s;
        }
        self
    }

    pub fn determinant(&self) -> f64 {
        let te = &self.elements;
        let (a, b, c) = (te[0], te[1], te[2]);
        let (d, e, f) = (te[3], te[4], te[5]);
        let (g, h, i) = (te[6], te[7], te[8]);
        a * e * i - a * f * h - b * d * i + b * f * g + c * d * h - c * e * g
    }

    /// `invert()`: a zero determinant sets every element to 0.
    pub fn invert(&mut self) -> &mut Self {
        let te = self.elements;
        let (n11, n21, n31) = (te[0], te[1], te[2]);
        let (n12, n22, n32) = (te[3], te[4], te[5]);
        let (n13, n23, n33) = (te[6], te[7], te[8]);
        let t11 = n33 * n22 - n32 * n23;
        let t12 = n32 * n13 - n33 * n12;
        let t13 = n23 * n12 - n22 * n13;
        let det = n11 * t11 + n21 * t12 + n31 * t13;
        if det == 0.0 {
            return self.set(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        }
        let det_inv = 1.0 / det;
        let te = &mut self.elements;
        te[0] = t11 * det_inv;
        te[1] = (n31 * n23 - n33 * n21) * det_inv;
        te[2] = (n32 * n21 - n31 * n22) * det_inv;
        te[3] = t12 * det_inv;
        te[4] = (n33 * n11 - n31 * n13) * det_inv;
        te[5] = (n31 * n12 - n32 * n11) * det_inv;
        te[6] = t13 * det_inv;
        te[7] = (n21 * n13 - n23 * n11) * det_inv;
        te[8] = (n22 * n11 - n21 * n12) * det_inv;
        self
    }

    pub fn transpose(&mut self) -> &mut Self {
        self.elements.swap(1, 3);
        self.elements.swap(2, 6);
        self.elements.swap(5, 7);
        self
    }

    /// `getNormalMatrix(matrix4)`.
    pub fn get_normal_matrix(&mut self, matrix4: &Matrix4) -> &mut Self {
        self.set_from_matrix4(matrix4);
        self.invert();
        self.transpose()
    }

    /// `transposeIntoArray(r)`.
    pub fn transpose_into_array(&self, r: &mut [f64]) {
        let m = &self.elements;
        r[0] = m[0];
        r[1] = m[3];
        r[2] = m[6];
        r[3] = m[1];
        r[4] = m[4];
        r[5] = m[7];
        r[6] = m[2];
        r[7] = m[5];
        r[8] = m[8];
    }

    /// `setUvTransform(tx, ty, sx, sy, rotation, cx, cy)`.
    pub fn set_uv_transform(
        &mut self,
        tx: f64, ty: f64, sx: f64, sy: f64, rotation: f64, cx: f64, cy: f64,
    ) -> &mut Self {
        let c = v8math::cos(rotation);
        let s = v8math::sin(rotation);
        self.set(
            sx * c, sx * s, -sx * (c * cx + s * cy) + cx + tx,
            -sy * s, sy * c, -sy * (-s * cx + c * cy) + cy + ty,
            0.0, 0.0, 1.0,
        )
    }

    /// `scale(sx, sy)`: premultiplies a 2D scale, as three does
    /// through its module temporary (fresh here, same arithmetic).
    pub fn scale(&mut self, sx: f64, sy: f64) -> &mut Self {
        let mut m = Matrix3::default();
        m.make_scale(sx, sy);
        self.premultiply(&m);
        self
    }

    /// `rotate(theta)`: premultiplies `makeRotation(-theta)`.
    pub fn rotate(&mut self, theta: f64) -> &mut Self {
        let mut m = Matrix3::default();
        m.make_rotation(-theta);
        self.premultiply(&m);
        self
    }

    /// `translate(tx, ty)`.
    pub fn translate(&mut self, tx: f64, ty: f64) -> &mut Self {
        let mut m = Matrix3::default();
        m.make_translation(tx, ty);
        self.premultiply(&m);
        self
    }

    /// `makeTranslation(x, y)` (the Vector2 form is
    /// `make_translation_v2`).
    pub fn make_translation(&mut self, x: f64, y: f64) -> &mut Self {
        self.set(1.0, 0.0, x, 0.0, 1.0, y, 0.0, 0.0, 1.0)
    }

    pub fn make_translation_v2(&mut self, v: &Vector2) -> &mut Self {
        self.make_translation(v.x, v.y)
    }

    /// `makeRotation(theta)`: counterclockwise.
    pub fn make_rotation(&mut self, theta: f64) -> &mut Self {
        let c = v8math::cos(theta);
        let s = v8math::sin(theta);
        self.set(c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0)
    }

    /// `makeScale(x, y)`.
    pub fn make_scale(&mut self, x: f64, y: f64) -> &mut Self {
        self.set(x, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 1.0)
    }

    pub fn equals(&self, matrix: &Matrix3) -> bool {
        self.elements == matrix.elements
    }

    pub fn from_array(&mut self, array: &[f64], offset: usize) -> &mut Self {
        self.elements.copy_from_slice(&array[offset..offset + 9]);
        self
    }

    pub fn from_array_f32(&mut self, array: &[f32], offset: usize) -> &mut Self {
        for i in 0..9 {
            self.elements[i] = array[offset + i] as f64;
        }
        self
    }

    pub fn to_array<'t>(&self, array: &'t mut [f64], offset: usize) -> &'t mut [f64] {
        array[offset..offset + 9].copy_from_slice(&self.elements);
        array
    }
}

/// Which coordinate system a projection matrix is made for
/// (`WebGLCoordinateSystem` or `WebGPUCoordinateSystem` in three,
/// the constants `makePerspective`/`makeOrthographic` switch on).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CoordinateSystem {
    #[default]
    WebGL,
    WebGPU,
}

/// `THREE.Matrix4`, r170.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix4 {
    pub elements: [f64; 16],
}

impl Default for Matrix4 {
    /// `new Matrix4()`: the identity.
    fn default() -> Self {
        Matrix4 {
            elements: [
                1.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 1.0,
            ],
        }
    }
}

impl Matrix4 {
    /// `set(...)`: row-major arguments.
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
        self.set(
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    pub fn copy(&mut self, m: &Matrix4) -> &mut Self {
        self.elements = m.elements;
        self
    }

    pub fn copy_position(&mut self, m: &Matrix4) -> &mut Self {
        self.elements[12] = m.elements[12];
        self.elements[13] = m.elements[13];
        self.elements[14] = m.elements[14];
        self
    }

    pub fn set_from_matrix3(&mut self, m: &Matrix3) -> &mut Self {
        let me = &m.elements;
        self.set(
            me[0], me[3], me[6], 0.0,
            me[1], me[4], me[7], 0.0,
            me[2], me[5], me[8], 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    /// `extractBasis(xAxis, yAxis, zAxis)`.
    pub fn extract_basis(&self, x_axis: &mut Vector3, y_axis: &mut Vector3, z_axis: &mut Vector3) {
        x_axis.set_from_matrix_column(self, 0);
        y_axis.set_from_matrix_column(self, 1);
        z_axis.set_from_matrix_column(self, 2);
    }

    pub fn make_basis(&mut self, x_axis: &Vector3, y_axis: &Vector3, z_axis: &Vector3) -> &mut Self {
        self.set(
            x_axis.x, y_axis.x, z_axis.x, 0.0,
            x_axis.y, y_axis.y, z_axis.y, 0.0,
            x_axis.z, y_axis.z, z_axis.z, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    /// `extractRotation(m)`: does not support reflection matrices.
    pub fn extract_rotation(&mut self, m: &Matrix4) -> &mut Self {
        let me = m.elements;
        let mut col = Vector3::default();
        let scale_x = 1.0 / col.set_from_matrix_column(m, 0).length();
        let scale_y = 1.0 / col.set_from_matrix_column(m, 1).length();
        let scale_z = 1.0 / col.set_from_matrix_column(m, 2).length();
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

    /// `makeRotationFromEuler(euler)`.
    pub fn make_rotation_from_euler(&mut self, euler: &Euler) -> &mut Self {
        let (x, y, z) = (euler.x, euler.y, euler.z);
        let a = v8math::cos(x);
        let b = v8math::sin(x);
        let c = v8math::cos(y);
        let d = v8math::sin(y);
        let e = v8math::cos(z);
        let f = v8math::sin(z);
        let te = &mut self.elements;
        match euler.order {
            EulerOrder::XYZ => {
                let (ae, af, be, bf) = (a * e, a * f, b * e, b * f);
                te[0] = c * e;
                te[4] = -c * f;
                te[8] = d;
                te[1] = af + be * d;
                te[5] = ae - bf * d;
                te[9] = -b * c;
                te[2] = bf - ae * d;
                te[6] = be + af * d;
                te[10] = a * c;
            }
            EulerOrder::YXZ => {
                let (ce, cf, de, df) = (c * e, c * f, d * e, d * f);
                te[0] = ce + df * b;
                te[4] = de * b - cf;
                te[8] = a * d;
                te[1] = a * f;
                te[5] = a * e;
                te[9] = -b;
                te[2] = cf * b - de;
                te[6] = df + ce * b;
                te[10] = a * c;
            }
            EulerOrder::ZXY => {
                let (ce, cf, de, df) = (c * e, c * f, d * e, d * f);
                te[0] = ce - df * b;
                te[4] = -a * f;
                te[8] = de + cf * b;
                te[1] = cf + de * b;
                te[5] = a * e;
                te[9] = df - ce * b;
                te[2] = -a * d;
                te[6] = b;
                te[10] = a * c;
            }
            EulerOrder::ZYX => {
                let (ae, af, be, bf) = (a * e, a * f, b * e, b * f);
                te[0] = c * e;
                te[4] = be * d - af;
                te[8] = ae * d + bf;
                te[1] = c * f;
                te[5] = bf * d + ae;
                te[9] = af * d - be;
                te[2] = -d;
                te[6] = b * c;
                te[10] = a * c;
            }
            EulerOrder::YZX => {
                let (ac, ad, bc, bd) = (a * c, a * d, b * c, b * d);
                te[0] = c * e;
                te[4] = bd - ac * f;
                te[8] = bc * f + ad;
                te[1] = f;
                te[5] = a * e;
                te[9] = -b * e;
                te[2] = -d * e;
                te[6] = ad * f + bc;
                te[10] = ac - bd * f;
            }
            EulerOrder::XZY => {
                let (ac, ad, bc, bd) = (a * c, a * d, b * c, b * d);
                te[0] = c * e;
                te[4] = -f;
                te[8] = d * e;
                te[1] = ac * f + bd;
                te[5] = a * e;
                te[9] = ad * f - bc;
                te[2] = bc * f - ad;
                te[6] = b * e;
                te[10] = bd * f + ac;
            }
        }
        te[3] = 0.0;
        te[7] = 0.0;
        te[11] = 0.0;
        te[12] = 0.0;
        te[13] = 0.0;
        te[14] = 0.0;
        te[15] = 1.0;
        self
    }

    /// `makeRotationFromQuaternion(q)`.
    pub fn make_rotation_from_quaternion(&mut self, q: &Quaternion) -> &mut Self {
        let zero = Vector3::new(0.0, 0.0, 0.0);
        let one = Vector3::new(1.0, 1.0, 1.0);
        self.compose(&zero, q, &one)
    }

    /// `lookAt(eye, target, up)`, with three's parallel dodge.
    pub fn look_at(&mut self, eye: &Vector3, target: &Vector3, up: &Vector3) -> &mut Self {
        let mut z = Vector3::default();
        z.sub_vectors(eye, target);
        if z.length_sq() == 0.0 {
            z.z = 1.0;
        }
        z.normalize();
        let mut x = Vector3::default();
        x.cross_vectors(up, &z);
        if x.length_sq() == 0.0 {
            if up.z.abs() == 1.0 {
                z.x += 0.0001;
            } else {
                z.z += 0.0001;
            }
            z.normalize();
            x.cross_vectors(up, &z);
        }
        x.normalize();
        let mut y = Vector3::default();
        y.cross_vectors(&z, &x);
        let te = &mut self.elements;
        te[0] = x.x; te[4] = y.x; te[8] = z.x;
        te[1] = x.y; te[5] = y.y; te[9] = z.y;
        te[2] = x.z; te[6] = y.z; te[10] = z.z;
        self
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

    pub fn multiply_scalar(&mut self, s: f64) -> &mut Self {
        for e in self.elements.iter_mut() {
            *e *= s;
        }
        self
    }

    /// `determinant()`, in three's grouping (the sums associate as
    /// written there).
    pub fn determinant(&self) -> f64 {
        let te = &self.elements;
        let (n11, n12, n13, n14) = (te[0], te[4], te[8], te[12]);
        let (n21, n22, n23, n24) = (te[1], te[5], te[9], te[13]);
        let (n31, n32, n33, n34) = (te[2], te[6], te[10], te[14]);
        let (n41, n42, n43, n44) = (te[3], te[7], te[11], te[15]);
        n41 * (n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33
            + n13 * n22 * n34 - n12 * n23 * n34)
            + n42 * (n11 * n23 * n34 - n11 * n24 * n33 + n14 * n21 * n33 - n13 * n21 * n34
                + n13 * n24 * n31 - n14 * n23 * n31)
            + n43 * (n11 * n24 * n32 - n11 * n22 * n34 - n14 * n21 * n32 + n12 * n21 * n34
                + n14 * n22 * n31 - n12 * n24 * n31)
            + n44 * (-n13 * n22 * n31 - n11 * n23 * n32 + n11 * n22 * n33 + n13 * n21 * n32
                - n12 * n21 * n33 + n12 * n23 * n31)
    }

    pub fn transpose(&mut self) -> &mut Self {
        self.elements.swap(1, 4);
        self.elements.swap(2, 8);
        self.elements.swap(6, 9);
        self.elements.swap(3, 12);
        self.elements.swap(7, 13);
        self.elements.swap(11, 14);
        self
    }

    /// `setPosition(x, y, z)`.
    pub fn set_position(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        self.elements[12] = x;
        self.elements[13] = y;
        self.elements[14] = z;
        self
    }

    /// `setPosition(v)` with a Vector3.
    pub fn set_position_v3(&mut self, v: &Vector3) -> &mut Self {
        self.set_position(v.x, v.y, v.z)
    }

    /// `invert()`: a zero determinant zeroes the matrix.
    pub fn invert(&mut self) -> &mut Self {
        let te0 = self.elements;
        let (n11, n21, n31, n41) = (te0[0], te0[1], te0[2], te0[3]);
        let (n12, n22, n32, n42) = (te0[4], te0[5], te0[6], te0[7]);
        let (n13, n23, n33, n43) = (te0[8], te0[9], te0[10], te0[11]);
        let (n14, n24, n34, n44) = (te0[12], te0[13], te0[14], te0[15]);
        let t11 = n23 * n34 * n42 - n24 * n33 * n42 + n24 * n32 * n43 - n22 * n34 * n43
            - n23 * n32 * n44 + n22 * n33 * n44;
        let t12 = n14 * n33 * n42 - n13 * n34 * n42 - n14 * n32 * n43 + n12 * n34 * n43
            + n13 * n32 * n44 - n12 * n33 * n44;
        let t13 = n13 * n24 * n42 - n14 * n23 * n42 + n14 * n22 * n43 - n12 * n24 * n43
            - n13 * n22 * n44 + n12 * n23 * n44;
        let t14 = n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33
            + n13 * n22 * n34 - n12 * n23 * n34;
        let det = n11 * t11 + n21 * t12 + n31 * t13 + n41 * t14;
        if det == 0.0 {
            return self.set(
                0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            );
        }
        let det_inv = 1.0 / det;
        let te = &mut self.elements;
        te[0] = t11 * det_inv;
        te[1] = (n24 * n33 * n41 - n23 * n34 * n41 - n24 * n31 * n43 + n21 * n34 * n43
            + n23 * n31 * n44 - n21 * n33 * n44) * det_inv;
        te[2] = (n22 * n34 * n41 - n24 * n32 * n41 + n24 * n31 * n42 - n21 * n34 * n42
            - n22 * n31 * n44 + n21 * n32 * n44) * det_inv;
        te[3] = (n23 * n32 * n41 - n22 * n33 * n41 - n23 * n31 * n42 + n21 * n33 * n42
            + n22 * n31 * n43 - n21 * n32 * n43) * det_inv;
        te[4] = t12 * det_inv;
        te[5] = (n13 * n34 * n41 - n14 * n33 * n41 + n14 * n31 * n43 - n11 * n34 * n43
            - n13 * n31 * n44 + n11 * n33 * n44) * det_inv;
        te[6] = (n14 * n32 * n41 - n12 * n34 * n41 - n14 * n31 * n42 + n11 * n34 * n42
            + n12 * n31 * n44 - n11 * n32 * n44) * det_inv;
        te[7] = (n12 * n33 * n41 - n13 * n32 * n41 + n13 * n31 * n42 - n11 * n33 * n42
            - n12 * n31 * n43 + n11 * n32 * n43) * det_inv;
        te[8] = t13 * det_inv;
        te[9] = (n14 * n23 * n41 - n13 * n24 * n41 - n14 * n21 * n43 + n11 * n24 * n43
            + n13 * n21 * n44 - n11 * n23 * n44) * det_inv;
        te[10] = (n12 * n24 * n41 - n14 * n22 * n41 + n14 * n21 * n42 - n11 * n24 * n42
            - n12 * n21 * n44 + n11 * n22 * n44) * det_inv;
        te[11] = (n13 * n22 * n41 - n12 * n23 * n41 - n13 * n21 * n42 + n11 * n23 * n42
            + n12 * n21 * n43 - n11 * n22 * n43) * det_inv;
        te[12] = t14 * det_inv;
        te[13] = (n13 * n24 * n31 - n14 * n23 * n31 + n14 * n21 * n33 - n11 * n24 * n33
            - n13 * n21 * n34 + n11 * n23 * n34) * det_inv;
        te[14] = (n14 * n22 * n31 - n12 * n24 * n31 - n14 * n21 * n32 + n11 * n24 * n32
            + n12 * n21 * n34 - n11 * n22 * n34) * det_inv;
        te[15] = (n12 * n23 * n31 - n13 * n22 * n31 + n13 * n21 * n32 - n11 * n23 * n32
            - n12 * n21 * n33 + n11 * n22 * n33) * det_inv;
        self
    }

    /// `scale(v)`.
    pub fn scale(&mut self, v: &Vector3) -> &mut Self {
        let (x, y, z) = (v.x, v.y, v.z);
        let te = &mut self.elements;
        te[0] *= x; te[4] *= y; te[8] *= z;
        te[1] *= x; te[5] *= y; te[9] *= z;
        te[2] *= x; te[6] *= y; te[10] *= z;
        te[3] *= x; te[7] *= y; te[11] *= z;
        self
    }

    /// `getMaxScaleOnAxis()`: `Math.max` is variadic left-to-right.
    pub fn get_max_scale_on_axis(&self) -> f64 {
        let te = &self.elements;
        let scale_x_sq = te[0] * te[0] + te[1] * te[1] + te[2] * te[2];
        let scale_y_sq = te[4] * te[4] + te[5] * te[5] + te[6] * te[6];
        let scale_z_sq = te[8] * te[8] + te[9] * te[9] + te[10] * te[10];
        (js_max(js_max(scale_x_sq, scale_y_sq), scale_z_sq)).sqrt()
    }

    /// `makeTranslation(x, y, z)`.
    pub fn make_translation(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        self.set(
            1.0, 0.0, 0.0, x,
            0.0, 1.0, 0.0, y,
            0.0, 0.0, 1.0, z,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    pub fn make_translation_v3(&mut self, v: &Vector3) -> &mut Self {
        self.make_translation(v.x, v.y, v.z)
    }

    pub fn make_rotation_x(&mut self, theta: f64) -> &mut Self {
        let c = v8math::cos(theta);
        let s = v8math::sin(theta);
        self.set(
            1.0, 0.0, 0.0, 0.0,
            0.0, c, -s, 0.0,
            0.0, s, c, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    pub fn make_rotation_y(&mut self, theta: f64) -> &mut Self {
        let c = v8math::cos(theta);
        let s = v8math::sin(theta);
        self.set(
            c, 0.0, s, 0.0,
            0.0, 1.0, 0.0, 0.0,
            -s, 0.0, c, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    pub fn make_rotation_z(&mut self, theta: f64) -> &mut Self {
        let c = v8math::cos(theta);
        let s = v8math::sin(theta);
        self.set(
            c, -s, 0.0, 0.0,
            s, c, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    /// `makeRotationAxis(axis, angle)`.
    pub fn make_rotation_axis(&mut self, axis: &Vector3, angle: f64) -> &mut Self {
        let c = v8math::cos(angle);
        let s = v8math::sin(angle);
        let t = 1.0 - c;
        let (x, y, z) = (axis.x, axis.y, axis.z);
        let tx = t * x;
        let ty = t * y;
        self.set(
            tx * x + c, tx * y - s * z, tx * z + s * y, 0.0,
            tx * y + s * z, ty * y + c, ty * z - s * x, 0.0,
            tx * z - s * y, ty * z + s * x, t * z * z + c, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    pub fn make_scale(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        self.set(
            x, 0.0, 0.0, 0.0,
            0.0, y, 0.0, 0.0,
            0.0, 0.0, z, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    /// `makeShear(xy, xz, yx, yz, zx, zy)`.
    pub fn make_shear(&mut self, xy: f64, xz: f64, yx: f64, yz: f64, zx: f64, zy: f64) -> &mut Self {
        self.set(
            1.0, yx, zx, 0.0,
            xy, 1.0, zy, 0.0,
            xz, yz, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    /// `compose(position, quaternion, scale)`.
    pub fn compose(&mut self, position: &Vector3, quaternion: &Quaternion, scale: &Vector3) -> &mut Self {
        let (x, y, z, w) = (quaternion.x, quaternion.y, quaternion.z, quaternion.w);
        let x2 = x + x;
        let y2 = y + y;
        let z2 = z + z;
        let xx = x * x2;
        let xy = x * y2;
        let xz = x * z2;
        let yy = y * y2;
        let yz = y * z2;
        let zz = z * z2;
        let wx = w * x2;
        let wy = w * y2;
        let wz = w * z2;
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

    /// `decompose(position, quaternion, scale)`.
    pub fn decompose(&self, position: &mut Vector3, quaternion: &mut Quaternion, scale: &mut Vector3) {
        let te = &self.elements;
        let mut v = Vector3::default();
        let mut sx = v.set(te[0], te[1], te[2]).length();
        let sy = v.set(te[4], te[5], te[6]).length();
        let sz = v.set(te[8], te[9], te[10]).length();
        if self.determinant() < 0.0 {
            sx = -sx;
        }
        position.x = te[12];
        position.y = te[13];
        position.z = te[14];
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

    /// `makePerspective(left, right, top, bottom, near, far,
    /// coordinateSystem)`.
    pub fn make_perspective(
        &mut self,
        left: f64, right: f64, top: f64, bottom: f64, near: f64, far: f64,
        coordinate_system: CoordinateSystem,
    ) -> &mut Self {
        let x = 2.0 * near / (right - left);
        let y = 2.0 * near / (top - bottom);
        let a = (right + left) / (right - left);
        let b = (top + bottom) / (top - bottom);
        let (c, d) = match coordinate_system {
            CoordinateSystem::WebGL => (-(far + near) / (far - near), (-2.0 * far * near) / (far - near)),
            CoordinateSystem::WebGPU => (-far / (far - near), (-far * near) / (far - near)),
        };
        let te = &mut self.elements;
        te[0] = x; te[4] = 0.0; te[8] = a; te[12] = 0.0;
        te[1] = 0.0; te[5] = y; te[9] = b; te[13] = 0.0;
        te[2] = 0.0; te[6] = 0.0; te[10] = c; te[14] = d;
        te[3] = 0.0; te[7] = 0.0; te[11] = -1.0; te[15] = 0.0;
        self
    }

    /// `makeOrthographic(left, right, top, bottom, near, far,
    /// coordinateSystem)`.
    pub fn make_orthographic(
        &mut self,
        left: f64, right: f64, top: f64, bottom: f64, near: f64, far: f64,
        coordinate_system: CoordinateSystem,
    ) -> &mut Self {
        let w = 1.0 / (right - left);
        let h = 1.0 / (top - bottom);
        let p = 1.0 / (far - near);
        let x = (right + left) * w;
        let y = (top + bottom) * h;
        let (z, z_inv) = match coordinate_system {
            CoordinateSystem::WebGL => ((far + near) * p, -2.0 * p),
            CoordinateSystem::WebGPU => (near * p, -1.0 * p),
        };
        let te = &mut self.elements;
        te[0] = 2.0 * w; te[4] = 0.0; te[8] = 0.0; te[12] = -x;
        te[1] = 0.0; te[5] = 2.0 * h; te[9] = 0.0; te[13] = -y;
        te[2] = 0.0; te[6] = 0.0; te[10] = z_inv; te[14] = -z;
        te[3] = 0.0; te[7] = 0.0; te[11] = 0.0; te[15] = 1.0;
        self
    }

    pub fn equals(&self, matrix: &Matrix4) -> bool {
        self.elements == matrix.elements
    }

    pub fn from_array(&mut self, array: &[f64], offset: usize) -> &mut Self {
        self.elements.copy_from_slice(&array[offset..offset + 16]);
        self
    }

    pub fn from_array_f32(&mut self, array: &[f32], offset: usize) -> &mut Self {
        for i in 0..16 {
            self.elements[i] = array[offset + i] as f64;
        }
        self
    }

    pub fn to_array<'t>(&self, array: &'t mut [f64], offset: usize) -> &'t mut [f64] {
        array[offset..offset + 16].copy_from_slice(&self.elements);
        array
    }
}
