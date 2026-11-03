//! The playable area from the `bnd_` config (nav/boundary.js).

use super::math::Vec3;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Edge {
    x: f64,
    z: f64,
    nx: f64,
    nz: f64,
}

/// Boundary (boundary.js:13). Polygons are point lists as loaded; the
/// JavaScript accepts {x, z} objects or [x, y, z, ...] arrays, and
/// data.rs passes the loaded paths' points as Vec3s (y is unused here).
#[derive(Clone, Debug, Default)]
pub struct Boundary {
    polygons: Vec<Vec<Edge>>,
}

impl Boundary {
    pub fn new(polygons: &[Vec<Vec3>]) -> Self {
        let polygons = polygons
            .iter()
            .map(|pts| {
                let mut edges = Vec::new();
                let n = pts.len();
                if n == 0 {
                    return edges;
                }
                let push = |edges: &mut Vec<Edge>, a: Vec3, b: Vec3| {
                    edges.push(Edge { x: a.x, z: a.z, nx: -(b.z - a.z), nz: b.x - a.x });
                };
                push(&mut edges, pts[n - 1], pts[0]);
                for i in 0..n - 1 {
                    push(&mut edges, pts[i], pts[i + 1]);
                }
                edges
            })
            .collect();
        Boundary { polygons }
    }

    /// boundary.js:28.
    pub fn empty(&self) -> bool {
        self.polygons.is_empty()
    }

    /// Inside polygon i (boundary.js:33).
    pub fn contains(&self, i: usize, p: Vec3) -> bool {
        for e in &self.polygons[i] {
            if (p.x - e.x) * e.nx + (p.z - e.z) * e.nz > 0.0 {
                return false;
            }
        }
        true
    }

    /// Outside every polygon of a nonempty boundary (boundary.js:41).
    pub fn outside(&self, p: Vec3) -> bool {
        if self.polygons.is_empty() {
            return false;
        }
        for i in 0..self.polygons.len() {
            if self.contains(i, p) {
                return false;
            }
        }
        true
    }
}
