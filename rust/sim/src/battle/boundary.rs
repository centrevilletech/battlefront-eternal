//! The playable area from the map's `bnd_` config
//! (src/ai/classic/nav/boundary.js; the AI's navigation package may take it
//! over): each polygon closes into edges, last to first and then in order,
//! each its start (x, z) and unnormalized normal (-dz, dx). A point is inside
//! a polygon when it is on the inner side of, or on, every edge; several
//! polygons form a union; with none, nothing is outside.

/// One edge: its start and its normal.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edge {
    pub x: f64,
    pub z: f64,
    pub nx: f64,
    pub nz: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Boundary {
    pub polygons: Vec<Vec<Edge>>,
}

impl Boundary {
    /// new Boundary(polygons) (boundary.js:15): each polygon's points as
    /// (x, z).
    pub fn new(polygons: &[Vec<[f64; 2]>]) -> Boundary {
        let polygons = polygons
            .iter()
            .map(|pts| {
                let mut edges = Vec::new();
                let n = pts.len();
                if n == 0 {
                    return edges;
                }
                let mut push = |a: [f64; 2], b: [f64; 2]| edges.push(Edge { x: a[0], z: a[1], nx: -(b[1] - a[1]), nz: b[0] - a[0] });
                push(pts[n - 1], pts[0]);
                for i in 0..n - 1 {
                    push(pts[i], pts[i + 1]);
                }
                edges
            })
            .collect();
        Boundary { polygons }
    }

    /// `empty`.
    pub fn empty(&self) -> bool {
        self.polygons.is_empty()
    }

    /// contains(i, p): inside polygon i.
    pub fn contains(&self, i: usize, x: f64, z: f64) -> bool {
        for e in &self.polygons[i] {
            if (x - e.x) * e.nx + (z - e.z) * e.nz > 0.0 {
                return false;
            }
        }
        true
    }

    /// outside(p): outside every polygon of a nonempty boundary.
    pub fn outside(&self, x: f64, z: f64) -> bool {
        if self.polygons.is_empty() {
            return false;
        }
        for i in 0..self.polygons.len() {
            if self.contains(i, x, z) {
                return false;
            }
        }
        true
    }
}
