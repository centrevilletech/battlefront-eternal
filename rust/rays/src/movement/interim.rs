//! CollisionWorld.gather and overlapsSphere (src/collision.js) for the
//! movement kernel, with the signatures of the shared sphere queries they
//! stand in for until those land in the crate.
//!
//! A three-mesh-bvh shapecast visits a tree's nodes depth first, the left
//! child before the right, testing each child's bounds before going down,
//! and calls back for every triangle of a leaf it reaches, in the order of
//! the tree's indirect buffer. The same walk over the packed nodes here
//! visits the same triangles in the same order, so the polygons come out in
//! the order the JavaScript files them, each with its triangles in order.

use crate::{apply_matrix4, js_max, js_min, Corner, Corners, State, Tree};
use std::ptr::addr_of_mut;

/// A gathered triangle's corners, in world space.
#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub c: [f64; 3],
}

/// One polygon of a gather: its number (with its body's offset), whether it
/// is a moving body's, and its triangles near the sphere.
#[derive(Clone, Debug)]
pub struct Polygon {
    pub id: f64,
    pub moving: bool,
    pub done: bool,
    pub tris: Vec<Tri>,
}

/// A gather's polygons, kept for reuse by the next.
#[derive(Default)]
pub struct Gathered {
    polygons: Vec<Polygon>,
    len: usize,
}

impl Gathered {
    pub fn new() -> Gathered {
        Gathered::default()
    }

    pub fn list(&self) -> &[Polygon] {
        &self.polygons[..self.len]
    }

    pub fn list_mut(&mut self) -> &mut [Polygon] {
        &mut self.polygons[..self.len]
    }

    /// CollisionWorld.polygon(id, moving): the gather's record of polygon
    /// `id`, a new one at the end of the list the first time.
    fn polygon(&mut self, id: f64, moving: bool) -> &mut Polygon {
        if let Some(at) = (0..self.len).rev().find(|&i| self.polygons[i].id == id) {
            return &mut self.polygons[at];
        }
        if self.len == self.polygons.len() {
            self.polygons.push(Polygon { id: 0.0, moving: false, done: false, tris: Vec::new() });
        }
        let polygon = &mut self.polygons[self.len];
        self.len += 1;
        polygon.id = id;
        polygon.tris.clear();
        polygon.done = false;
        polygon.moving = moving;
        polygon
    }
}

/// A caller's nearby fixed triangles (CollisionWorld.gatherCache).
pub struct GatherCache {
    margin: f64,
    bounds: [f64; 6],
    statics: Vec<u32>,
}

impl GatherCache {
    pub fn new(margin: f64) -> GatherCache {
        let inf = f64::INFINITY;
        GatherCache { margin, bounds: [inf, inf, inf, -inf, -inf, -inf], statics: Vec::new() }
    }
}

/// Each tree's polygon numbers (polygonOf), and each body's offset, by slot.
#[derive(Default)]
pub struct Polygons {
    pub trees: Vec<Option<Vec<u32>>>,
    pub offsets: Vec<f64>,
}

static mut POLYGONS: Option<Polygons> = None;

/// The table, for the uploads (one call at a time, never during a query).
pub fn polygons_mut() -> &'static mut Polygons {
    unsafe { (*addr_of_mut!(POLYGONS)).get_or_insert_with(Polygons::default) }
}

fn polygons() -> &'static Polygons {
    polygons_mut()
}

/// A tree's polygon numbers, which must have been uploaded.
fn polygon_numbers(tree: usize) -> &'static [u32] {
    match polygons().trees.get(tree) {
        Some(Some(list)) => list.as_slice(),
        _ => panic!("movement: no polygon numbers for a tree"),
    }
}

/// Box3.intersectsBox: `this` a, the argument b.
#[inline(always)]
fn intersects(a: &[f64; 6], b: &[f64; 6]) -> bool {
    b[3] >= a[0] && b[0] <= a[3] && b[4] >= a[1] && b[1] <= a[4] && b[5] >= a[2] && b[2] <= a[5]
}

/// collision.js's triangle-bounds test against a box (gatherSet, fillCache).
#[inline(always)]
fn outside(t: &[f64; 6], b: &[f64; 6]) -> bool {
    t[3] < b[0] || t[0] > b[3] || t[4] < b[1] || t[1] > b[4] || t[5] < b[2] || t[2] > b[5]
}

/// Box3.containsBox
#[inline(always)]
fn contains(a: &[f64; 6], b: &[f64; 6]) -> bool {
    a[0] <= b[0] && b[3] <= a[3] && a[1] <= b[1] && b[4] <= a[4] && a[2] <= b[2] && b[5] <= a[5]
}

/// Box3.applyMatrix4: the box of its eight corners transformed.
fn transform_box(b: &[f64; 6], e: &[f64; 16]) -> [f64; 6] {
    if b[3] < b[0] || b[4] < b[1] || b[5] < b[2] {
        return *b;
    }
    let inf = f64::INFINITY;
    let mut out = [inf, inf, inf, -inf, -inf, -inf];
    for &(x, y, z) in &[(b[0], b[1], b[2]), (b[0], b[1], b[5]), (b[0], b[4], b[2]), (b[0], b[4], b[5]), (b[3], b[1], b[2]), (b[3], b[1], b[5]), (b[3], b[4], b[2]), (b[3], b[4], b[5])] {
        let p = apply_matrix4(e, [x, y, z]);
        for k in 0..3 {
            out[k] = js_min(out[k], p[k]);
            out[k + 3] = js_max(out[k + 3], p[k]);
        }
    }
    out
}

#[inline(always)]
fn node_bounds(words: &[u32], node: usize) -> [f64; 6] {
    let f = |i: usize| f32::from_bits(words[node + i]) as f64;
    [f(0), f(1), f(2), f(3), f(4), f(5)]
}

/// three-mesh-bvh's shapecastTraverse from one node: `tri` takes each
/// triangle's place in the indirect buffer, true to stop.
fn traverse(words: &[u32], node: usize, query: &[f64; 6], tri: &mut dyn FnMut(usize) -> bool) -> bool {
    let w7 = words[node + 7];
    if (w7 >> 16) == 0xffff {
        let offset = words[node + 6] as usize;
        let count = (w7 & 0xffff) as usize;
        for i in offset..offset + count {
            if tri(i) {
                return true;
            }
        }
        return false;
    }
    let (left, right) = (node + 8, words[node + 6] as usize);
    if intersects(&node_bounds(words, left), query) && traverse(words, left, query, tri) {
        return true;
    }
    if intersects(&node_bounds(words, right), query) && traverse(words, right, query, tri) {
        return true;
    }
    false
}

/// MeshBVH.shapecast over every root, with a node test against `query`.
fn shapecast(tree: &Tree, query: &[f64; 6], tri: &mut dyn FnMut(usize) -> bool) -> bool {
    for words in &tree.roots {
        if traverse(words, 0, query, tri) {
            return true;
        }
    }
    false
}

/// A tree's triangle `t` (geometry order), as setTriangle reads it.
#[inline(always)]
fn corners<P: Corner + ?Sized>(tree: &Tree, p: &P, t: usize) -> [[f64; 3]; 3] {
    let o = t * 3;
    if tree.indexed {
        [p.corner(tree.index[o] as usize), p.corner(tree.index[o + 1] as usize), p.corner(tree.index[o + 2] as usize)]
    } else {
        [p.corner(o), p.corner(o + 1), p.corner(o + 2)]
    }
}

fn tri_corners(s: &State, tree: &Tree, t: usize) -> [[f64; 3]; 3] {
    match &s.positions[tree.positions] {
        Corners::F32(p) => corners(tree, p.as_slice(), t),
        Corners::F64(p) => corners(tree, p.as_slice(), t),
    }
}

#[inline(always)]
fn bounds_of(c: &[[f64; 3]; 3]) -> [f64; 6] {
    [
        js_min(js_min(c[0][0], c[1][0]), c[2][0]),
        js_min(js_min(c[0][1], c[1][1]), c[2][1]),
        js_min(js_min(c[0][2], c[1][2]), c[2][2]),
        js_max(js_max(c[0][0], c[1][0]), c[2][0]),
        js_max(js_max(c[0][1], c[1][1]), c[2][1]),
        js_max(js_max(c[0][2], c[1][2]), c[2][2]),
    ]
}

/// gatherFrom(bvh, polygonOf, offset) with the gather's box `query` (a
/// moving body's in its own space) and `matrix` (a moving body's).
fn gather_tree(s: &State, tree: usize, offset: f64, query: &[f64; 6], matrix: Option<&[f64; 16]>, out: &mut Gathered) {
    let t = &s.trees[tree];
    let polygon_of = polygon_numbers(tree);
    shapecast(t, query, &mut |i| {
        let index = t.indirect[i] as usize;
        let c = tri_corners(s, t, index);
        if !intersects(&bounds_of(&c), query) {
            return false;
        }
        let tri = match matrix {
            Some(e) => Tri { a: apply_matrix4(e, c[0]), b: apply_matrix4(e, c[1]), c: apply_matrix4(e, c[2]) },
            None => Tri { a: c[0], b: c[1], c: c[2] },
        };
        out.polygon(polygon_of[index] as f64 + offset, matrix.is_some()).tris.push(tri);
        false
    });
}

/// CollisionWorld.gather(center, radius, vehicle, cache, ignoreBody): the
/// polygons with a triangle whose bounds meet the sphere's, in `out`;
/// returns how many. Bodies by `enabled`, their places in the world's order.
#[allow(clippy::too_many_arguments)]
pub fn gather(s: &State, world: usize, center: [f64; 3], radius: f64, vehicle: bool, cache: Option<&mut GatherCache>, ignore: i64, enabled: &[u8], out: &mut Gathered) -> usize {
    debug_assert!(!vehicle, "the interim gather has no vehicle-only barriers");
    let query = [center[0] - radius, center[1] - radius, center[2] - radius, center[0] + radius, center[1] + radius, center[2] + radius];
    out.len = 0;
    let w = &s.worlds[world];
    let tree = &s.trees[w.tree];
    let polygon_of = polygon_numbers(w.tree);
    match cache {
        Some(cache) => {
            if !contains(&cache.bounds, &query) {
                let m = cache.margin;
                let fill = [query[0] + -m, query[1] + -m, query[2] + -m, query[3] + m, query[4] + m, query[5] + m];
                cache.bounds = fill;
                cache.statics.clear();
                let statics = &mut cache.statics;
                shapecast(tree, &fill, &mut |i| {
                    let index = tree.indirect[i] as usize;
                    if !outside(&bounds_of(&tri_corners(s, tree, index)), &fill) {
                        statics.push(index as u32);
                    }
                    false
                });
            }
            for &index in &cache.statics {
                let c = tri_corners(s, tree, index as usize);
                if outside(&bounds_of(&c), &query) {
                    continue;
                }
                out.polygon(polygon_of[index as usize] as f64, false).tris.push(Tri { a: c[0], b: c[1], c: c[2] });
            }
        }
        None => gather_tree(s, w.tree, 0.0, &query, None, out),
    }
    let offsets = &polygons().offsets;
    for (i, &slot) in w.order.iter().enumerate() {
        let body = &s.bodies[slot as usize];
        if enabled[i] == 0 || slot as i64 == ignore || !intersects(&body.bounds, &query) {
            continue;
        }
        let offset = *offsets.get(slot as usize).expect("movement: no offset for a body");
        if body.moving {
            gather_tree(s, body.tree, offset, &transform_box(&query, &body.inverse), Some(&body.matrix), out);
        } else {
            gather_tree(s, body.tree, offset, &query, None, out);
        }
    }
    out.len
}

/// Triangle.closestPointToPoint(p) of the triangle a, b, c.
pub fn closest_point_to_point(a: [f64; 3], b: [f64; 3], c: [f64; 3], p: [f64; 3]) -> [f64; 3] {
    let sub = |u: [f64; 3], v: [f64; 3]| [u[0] - v[0], u[1] - v[1], u[2] - v[2]];
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let scaled = |o: [f64; 3], v: [f64; 3], s: f64| [o[0] + v[0] * s, o[1] + v[1] * s, o[2] + v[2] * s];
    let vab = sub(b, a);
    let vac = sub(c, a);
    let vap = sub(p, a);
    let d1 = dot(vab, vap);
    let d2 = dot(vac, vap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let vbp = sub(p, b);
    let d3 = dot(vab, vbp);
    let d4 = dot(vac, vbp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return scaled(a, vab, v);
    }
    let vcp = sub(p, c);
    let d5 = dot(vab, vcp);
    let d6 = dot(vac, vcp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return scaled(a, vac, w);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let vbc = sub(c, b);
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return scaled(b, vbc, w);
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    scaled(scaled(a, vab, v), vac, w)
}

#[inline(always)]
fn distance_sq(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    dx * dx + dy * dy + dz * dz
}

/// A tree's overlapQuery shapecast: any triangle of a reached leaf within
/// `radius_sq` of `center` (no bounds test of its own).
fn tree_overlaps(s: &State, tree: usize, query: &[f64; 6], center: [f64; 3], radius_sq: f64) -> bool {
    let t = &s.trees[tree];
    shapecast(t, query, &mut |i| {
        let c = tri_corners(s, t, t.indirect[i] as usize);
        distance_sq(closest_point_to_point(c[0], c[1], c[2], center), center) < radius_sq
    })
}

/// TriangleGrid.overlapsSphere(center, radius).
fn grid_overlaps(s: &mut State, grid: usize, center: [f64; 3], radius: f64) -> bool {
    let State { grids, positions, .. } = s;
    let g = &mut grids[grid];
    let size = g.size;
    let r2 = radius * radius;
    let x0 = ((center[0] - radius - g.min_x) / size).floor();
    let x1 = ((center[0] + radius - g.min_x) / size).floor();
    let z0 = ((center[2] - radius - g.min_z) / size).floor();
    let z1 = ((center[2] + radius - g.min_z) / size).floor();
    g.serial = g.serial.wrapping_add(1);
    if g.serial == 0xffff_ffff {
        g.stamp.iter_mut().for_each(|v| *v = 0);
        g.serial = 1;
    }
    let serial = g.serial;
    let mut z = js_max(0.0, z0);
    while z <= js_min(g.nz - 1.0, z1) {
        let mut x = js_max(0.0, x0);
        while x <= js_min(g.nx - 1.0, x1) {
            let cell = (z * g.nx + x) as usize;
            for i in g.start[cell] as usize..g.start[cell + 1] as usize {
                let t = g.cells[i] as usize;
                if g.stamp[t] == serial {
                    continue;
                }
                g.stamp[t] = serial;
                let o = t * 3;
                let (ia, ib, ic) = (g.index[o] as usize, g.index[o + 1] as usize, g.index[o + 2] as usize);
                let (a, b, c) = match &positions[g.positions] {
                    Corners::F32(p) => (p.corner(ia), p.corner(ib), p.corner(ic)),
                    Corners::F64(p) => (p.corner(ia), p.corner(ib), p.corner(ic)),
                };
                if distance_sq(closest_point_to_point(a, b, c, center), center) < r2 {
                    return true;
                }
            }
            x += 1.0;
        }
        z += 1.0;
    }
    false
}

/// CollisionWorld.overlapsSphere(center, radius, ignoreBody).
pub fn overlaps_sphere(s: &mut State, world: usize, center: [f64; 3], radius: f64, ignore: i64, enabled: &[u8]) -> bool {
    let query = [center[0] + -radius, center[1] + -radius, center[2] + -radius, center[0] + radius, center[1] + radius, center[2] + radius];
    let radius_sq = radius * radius;
    let (tree, grid) = (s.worlds[world].tree, s.worlds[world].grid);
    if tree_overlaps(s, tree, &query, center, radius_sq) {
        return true;
    }
    if let Some(grid) = grid {
        if grid_overlaps(s, grid, center, radius) {
            return true;
        }
    }
    let w = &s.worlds[world];
    for (i, &slot) in w.order.iter().enumerate() {
        let body = &s.bodies[slot as usize];
        if enabled[i] == 0 || slot as i64 == ignore || !intersects(&body.bounds, &query) {
            continue;
        }
        let hit = if body.moving {
            tree_overlaps(s, body.tree, &transform_box(&query, &body.inverse), apply_matrix4(&body.inverse, center), radius_sq)
        } else {
            tree_overlaps(s, body.tree, &query, center, radius_sq)
        };
        if hit {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_point_regions() {
        let (a, b, c) = ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        assert_eq!(closest_point_to_point(a, b, c, [-1.0, 1.0, -1.0]), a);
        assert_eq!(closest_point_to_point(a, b, c, [2.0, 1.0, 0.0]), b);
        assert_eq!(closest_point_to_point(a, b, c, [0.25, 1.0, 0.25]), [0.25, 0.0, 0.25]);
        assert_eq!(closest_point_to_point(a, b, c, [0.5, 0.0, -1.0]), [0.5, 0.0, 0.0]);
    }
}
