// Matrix, frustum and bounds math done as three.js does it, operation for
// operation, in f64: the values this renderer uploads (a model-view matrix,
// a normal matrix) and the culling it decides are then the ones three would
// have uploaded and decided. Matrices are column-major, as three's elements.

pub type Mat4 = [f64; 16];

// Matrix4.multiplyMatrices(a, b).
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let (a11, a12, a13, a14) = (a[0], a[4], a[8], a[12]);
    let (a21, a22, a23, a24) = (a[1], a[5], a[9], a[13]);
    let (a31, a32, a33, a34) = (a[2], a[6], a[10], a[14]);
    let (a41, a42, a43, a44) = (a[3], a[7], a[11], a[15]);
    let (b11, b12, b13, b14) = (b[0], b[4], b[8], b[12]);
    let (b21, b22, b23, b24) = (b[1], b[5], b[9], b[13]);
    let (b31, b32, b33, b34) = (b[2], b[6], b[10], b[14]);
    let (b41, b42, b43, b44) = (b[3], b[7], b[11], b[15]);
    let mut t = [0.0; 16];
    t[0] = a11 * b11 + a12 * b21 + a13 * b31 + a14 * b41;
    t[4] = a11 * b12 + a12 * b22 + a13 * b32 + a14 * b42;
    t[8] = a11 * b13 + a12 * b23 + a13 * b33 + a14 * b43;
    t[12] = a11 * b14 + a12 * b24 + a13 * b34 + a14 * b44;
    t[1] = a21 * b11 + a22 * b21 + a23 * b31 + a24 * b41;
    t[5] = a21 * b12 + a22 * b22 + a23 * b32 + a24 * b42;
    t[9] = a21 * b13 + a22 * b23 + a23 * b33 + a24 * b43;
    t[13] = a21 * b14 + a22 * b24 + a23 * b34 + a24 * b44;
    t[2] = a31 * b11 + a32 * b21 + a33 * b31 + a34 * b41;
    t[6] = a31 * b12 + a32 * b22 + a33 * b32 + a34 * b42;
    t[10] = a31 * b13 + a32 * b23 + a33 * b33 + a34 * b43;
    t[14] = a31 * b14 + a32 * b24 + a33 * b34 + a34 * b44;
    t[3] = a41 * b11 + a42 * b21 + a43 * b31 + a44 * b41;
    t[7] = a41 * b12 + a42 * b22 + a43 * b32 + a44 * b42;
    t[11] = a41 * b13 + a42 * b23 + a43 * b33 + a44 * b43;
    t[15] = a41 * b14 + a42 * b24 + a43 * b34 + a44 * b44;
    t
}

// Matrix3.getNormalMatrix(m): setFromMatrix4, invert, transpose.
pub fn normal_matrix(m: &Mat4) -> [f64; 9] {
    // setFromMatrix4 leaves the upper 3x3 column-major.
    let te = [m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]];
    let (n11, n21, n31) = (te[0], te[1], te[2]);
    let (n12, n22, n32) = (te[3], te[4], te[5]);
    let (n13, n23, n33) = (te[6], te[7], te[8]);
    let t11 = n33 * n22 - n32 * n23;
    let t12 = n32 * n13 - n33 * n12;
    let t13 = n23 * n12 - n22 * n13;
    let det = n11 * t11 + n21 * t12 + n31 * t13;
    let mut r = [0.0; 9];
    if det != 0.0 {
        let inv = 1.0 / det;
        r[0] = t11 * inv;
        r[1] = (n31 * n23 - n33 * n21) * inv;
        r[2] = (n32 * n21 - n31 * n22) * inv;
        r[3] = t12 * inv;
        r[4] = (n33 * n11 - n31 * n13) * inv;
        r[5] = (n31 * n12 - n32 * n11) * inv;
        r[6] = t13 * inv;
        r[7] = (n21 * n13 - n23 * n11) * inv;
        r[8] = (n22 * n11 - n21 * n12) * inv;
    }
    // transpose()
    r.swap(1, 3);
    r.swap(2, 6);
    r.swap(5, 7);
    r
}

// Whether the matrix mirrors (three draws such a mesh with clockwise front
// faces). Its sign is the upper 3x3's for an affine matrix.
pub fn mirrors(m: &Mat4) -> bool {
    let d = m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2]) + m[8] * (m[1] * m[6] - m[5] * m[2]);
    d < 0.0
}

#[derive(Clone, Copy, Default)]
pub struct Plane {
    pub n: [f64; 3],
    pub c: f64,
}

impl Plane {
    fn set(x: f64, y: f64, z: f64, w: f64) -> Plane {
        // Plane.setComponents(...).normalize()
        let len = (x * x + y * y + z * z).sqrt();
        let inv = 1.0 / len;
        Plane { n: [x * inv, y * inv, z * inv], c: w * inv }
    }
    #[inline]
    pub fn distance(&self, p: &[f64; 3]) -> f64 {
        self.n[0] * p[0] + self.n[1] * p[1] + self.n[2] * p[2] + self.c
    }
}

#[derive(Clone, Copy, Default)]
pub struct Frustum {
    pub planes: [Plane; 6],
}

impl Frustum {
    // Frustum.setFromProjectionMatrix(m), WebGL coordinates.
    pub fn from_matrix(me: &Mat4) -> Frustum {
        let (me0, me1, me2, me3) = (me[0], me[1], me[2], me[3]);
        let (me4, me5, me6, me7) = (me[4], me[5], me[6], me[7]);
        let (me8, me9, me10, me11) = (me[8], me[9], me[10], me[11]);
        let (me12, me13, me14, me15) = (me[12], me[13], me[14], me[15]);
        Frustum {
            planes: [
                Plane::set(me3 - me0, me7 - me4, me11 - me8, me15 - me12),
                Plane::set(me3 + me0, me7 + me4, me11 + me8, me15 + me12),
                Plane::set(me3 + me1, me7 + me5, me11 + me9, me15 + me13),
                Plane::set(me3 - me1, me7 - me5, me11 - me9, me15 - me13),
                Plane::set(me3 - me2, me7 - me6, me11 - me10, me15 - me14),
                Plane::set(me3 + me2, me7 + me6, me11 + me10, me15 + me14),
            ],
        }
    }

    // Frustum.intersectsSphere.
    pub fn sphere(&self, center: &[f64; 3], radius: f64) -> bool {
        let neg = -radius;
        for p in &self.planes {
            if p.distance(center) < neg {
                return false;
            }
        }
        true
    }

    // Frustum.intersectsBox.
    pub fn aabb(&self, min: &[f64; 3], max: &[f64; 3]) -> bool {
        for p in &self.planes {
            let v = [
                if p.n[0] > 0.0 { max[0] } else { min[0] },
                if p.n[1] > 0.0 { max[1] } else { min[1] },
                if p.n[2] > 0.0 { max[2] } else { min[2] },
            ];
            if p.distance(&v) < 0.0 {
                return false;
            }
        }
        true
    }
}

// Sphere.applyMatrix4: Vector3.applyMatrix4 on the centre, the radius by
// Matrix4.getMaxScaleOnAxis.
pub fn sphere_to_world(m: &Mat4, center: &[f64; 3], radius: f64) -> ([f64; 3], f64) {
    let (x, y, z) = (center[0], center[1], center[2]);
    let w = 1.0 / (m[3] * x + m[7] * y + m[11] * z + m[15]);
    let c = [
        (m[0] * x + m[4] * y + m[8] * z + m[12]) * w,
        (m[1] * x + m[5] * y + m[9] * z + m[13]) * w,
        (m[2] * x + m[6] * y + m[10] * z + m[14]) * w,
    ];
    let sx = m[0] * m[0] + m[1] * m[1] + m[2] * m[2];
    let sy = m[4] * m[4] + m[5] * m[5] + m[6] * m[6];
    let sz = m[8] * m[8] + m[9] * m[9] + m[10] * m[10];
    (c, radius * sx.max(sy).max(sz).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normal_matrix_of_scale() {
        let mut m = [0.0; 16];
        m[0] = 2.0; m[5] = 4.0; m[10] = 8.0; m[15] = 1.0;
        let n = normal_matrix(&m);
        assert_eq!(n, [0.5, 0.0, 0.0, 0.0, 0.25, 0.0, 0.0, 0.0, 0.125]);
    }
}
