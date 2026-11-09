//! What the weather and atmosphere systems share: the page camera as the
//! phase receives it, the view frustum, the engine's random numbers and an
//! allocation-free stable sort.

use crate::layout::view as v;
use jsrt::conv::to_int32;
use three::{Matrix4, Plane, Vector3};

/// The engine's random numbers (src/atmosphere.js EngineRandom): two steps of
/// a 32-bit LCG make one draw, the seed first xored with 0xC71C2830.
#[derive(Clone, Copy)]
// The engine's random numbers: two steps of a 32-bit LCG make one draw, the
// seed first xored with 0xC71C2830. The fog, snow, wind and sky draw from
// them in the order the 2004 game does.
pub struct EngineRandom {
    s: u32,
}

impl EngineRandom {
    pub fn new(seed: f64) -> EngineRandom {
        // `(seed ^ 0xc71c2830) >>> 0`: the xor is on int32s.
        EngineRandom { s: (to_int32(seed) ^ 0xc71c2830u32 as i32) as u32 }
    }

    pub fn int(&mut self) -> u32 {
        let s1 = self.s.wrapping_mul(1664525).wrapping_add(1013904223);
        let s2 = s1.wrapping_mul(1664525).wrapping_add(1013904223);
        self.s = s2;
        (s2 & 0xffff0000) | (s1 >> 16)
    }

    pub fn float(&mut self) -> f64 {
        (self.int() >> 9) as f64 / 8388608.0
    }
}

/// THREE.Frustum, r170 (WebGL clip space): six planes from a projection
/// times view matrix.
#[derive(Clone, Copy, Default)]
// A view frustum as three.js makes it from a projection and a view (the
// planes' normals point inward).
pub struct Frustum {
    planes: [Plane; 6],
}

impl Frustum {
    pub fn set_from_projection_matrix(&mut self, m: &Matrix4) {
        let me = &m.elements;
        let (m0, m1, m2, m3, m4, m5, m6, m7) = (me[0], me[1], me[2], me[3], me[4], me[5], me[6], me[7]);
        let (m8, m9, m10, m11, m12, m13, m14, m15) = (me[8], me[9], me[10], me[11], me[12], me[13], me[14], me[15]);
        self.planes[0].set_components(m3 - m0, m7 - m4, m11 - m8, m15 - m12).normalize();
        self.planes[1].set_components(m3 + m0, m7 + m4, m11 + m8, m15 + m12).normalize();
        self.planes[2].set_components(m3 + m1, m7 + m5, m11 + m9, m15 + m13).normalize();
        self.planes[3].set_components(m3 - m1, m7 - m5, m11 - m9, m15 - m13).normalize();
        self.planes[4].set_components(m3 - m2, m7 - m6, m11 - m10, m15 - m14).normalize();
        self.planes[5].set_components(m3 + m2, m7 + m6, m11 + m10, m15 + m14).normalize();
    }

    pub fn intersects_sphere(&self, center: &Vector3, radius: f64) -> bool {
        let negative = -radius;
        self.planes.iter().all(|plane| plane.distance_to_point(center).partial_cmp(&negative) != Some(std::cmp::Ordering::Less))
    }

    pub fn intersects_box(&self, min: &Vector3, max: &Vector3) -> bool {
        self.planes.iter().all(|plane| {
            let n = &plane.normal;
            let corner = Vector3::new(if n.x > 0.0 { max.x } else { min.x }, if n.y > 0.0 { max.y } else { min.y }, if n.z > 0.0 { max.z } else { min.z });
            plane.distance_to_point(&corner).partial_cmp(&0.0) != Some(std::cmp::Ordering::Less)
        })
    }
}

/// The camera the page hands over each frame: its world matrix, the world
/// matrix inverse and the projection (three keeps all three), and the lens.
// The camera the page hands over each frame: its world matrix, the world
// matrix inverse and the projection (three keeps all three), the lens, and
// its position (which differs from the world matrix's if it is parented).
pub struct Camera {
    pub world: Matrix4,
    pub projection: Matrix4,
    pub inverse: Matrix4,
    pub dt: f64,
    pub position: [f64; 3],
    pub fov: f64,
    pub aspect: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self::new()
    }
}

impl Camera {
    pub fn new() -> Camera {
        Camera { world: Matrix4::default(), projection: Matrix4::default(), inverse: Matrix4::default(), dt: 0.0, position: [0.0; 3], fov: f64::NAN, aspect: f64::NAN }
    }

    /// Reads the view record. A lens value the camera lacks is NaN there
    /// (`camera.fov ?? default`).
    pub fn read(&mut self, record: &[f64]) {
        self.dt = record[v::DT];
        self.world.elements.copy_from_slice(&record[v::CAMERA..v::CAMERA + 16]);
        self.inverse.elements.copy_from_slice(&record[v::INVERSE..v::INVERSE + 16]);
        self.projection.elements.copy_from_slice(&record[v::PROJECTION..v::PROJECTION + 16]);
        self.position.copy_from_slice(&record[v::POSITION..v::POSITION + 3]);
        self.fov = record[v::FOV];
        self.aspect = record[v::ASPECT];
    }

    pub fn fov_or(&self, default: f64) -> f64 {
        if self.fov.is_nan() { default } else { self.fov }
    }

    pub fn aspect_or_1(&self) -> f64 {
        if self.aspect.is_nan() { 1.0 } else { self.aspect }
    }

    /// `setFromProjectionMatrix(multiplyMatrices(projection, matrixWorldInverse))`.
    pub fn frustum(&self, out: &mut Frustum) {
        let mut view_projection = Matrix4::default();
        view_projection.multiply_matrices(&self.projection, &self.inverse);
        out.set_from_projection_matrix(&view_projection);
    }
}

/// Sorts `order` (indices into `keys`) by key, largest first, keeping the
/// order of equal keys (Array.prototype.sort with `(a, b) => b - a`), using
/// `scratch` of the same length. A merge sort, so nothing is allocated.
// A stable merge sort of indices by key, largest first: the page sorted with
// Array.prototype.sort and a descending comparator (stable), and a sort that
// allocates would allocate every frame.
pub fn sort_descending(keys: &[f64], order: &mut [u32], scratch: &mut [u32]) {
    let n = order.len();
    let mut width = 1;
    while width < n {
        let mut start = 0;
        while start < n {
            let mid = usize::min(start + width, n);
            let end = usize::min(start + 2 * width, n);
            let (mut i, mut j, mut k) = (start, mid, start);
            while i < mid && j < end {
                // The right one goes first only when it is strictly larger.
                if keys[order[j] as usize] - keys[order[i] as usize] > 0.0 {
                    scratch[k] = order[j];
                    j += 1;
                } else {
                    scratch[k] = order[i];
                    i += 1;
                }
                k += 1;
            }
            while i < mid {
                scratch[k] = order[i];
                i += 1;
                k += 1;
            }
            while j < end {
                scratch[k] = order[j];
                j += 1;
                k += 1;
            }
            start = end;
        }
        order[..n].copy_from_slice(&scratch[..n]);
        width *= 2;
    }
}
