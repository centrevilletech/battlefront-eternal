//! Sphere queries in the collision world (src/collision.js): gather (the
//! polygons whose triangles' bounds meet a sphere's, each with those
//! triangles), its cached form, overlapsSphere and terrainNear, and the
//! terrain's height under a point (ground). For the world's JavaScript,
//! through the exports at the end, and for this crate's other modules,
//! through the pub fns, which run whole inside one call into the module.
//!
//! The rules are lib.rs's: each function repeats the JavaScript it stands
//! for, operation by operation and in the same order, in f64, so every
//! answer is the JavaScript's to the last bit: the same polygons in the same
//! order, each with the same triangles in the same order and the same
//! corners. three-mesh-bvh's shapecast visits a tree's leaves left to right,
//! testing each child's bounds before going down to it (never the root's),
//! and a leaf's triangles in its indirect order. Every comparison is written
//! as collision.js and three.js write it: `a < b` and `!(a >= b)` part ways
//! when a number is NaN.
//!
//! Bodies are switched on and off by JavaScript (`enabled`, a plain field),
//! so a query that meets bodies takes their flags from its caller: a slice
//! by position in the world's order for this crate's callers, and for
//! JavaScript's, the flags of the candidates the query's first call names
//! (as the casts do).

use crate::collision::{apply_matrix4, dot, f32_at, grid_raycast, js_max, js_min, sub, triangle_normal, Corner, Corners, Ray, State, Tree, MAX_BODIES};
use super::CollisionOwner;

/// collision.js BLOCKER_POLYGONS: the vehicle-only barriers' polygon numbers
/// are their own plus this.
pub const BLOCKER_POLYGONS: f64 = 1879048192.0;
/// collision.js GROUP_SCAN: past this many polygons a gather files them in
/// an index instead of searching its list.
const GROUP_SCAN: usize = 16;
/// The set of a triangle that belongs to none (a body's).
pub const NO_SET: u32 = u32::MAX;

/// A box as [min x, min y, min z, max x, max y, max z].
pub type Bounds = [f64; 6];

const EMPTY: Bounds = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];

/// Fixed triangles a cached gather files by number (collision.js staticSet
/// and blockerSet): the buildings, or the vehicle-only barriers.
pub struct FixedSet {
    pub tree: usize,
    /// Each triangle's bounds as collision.js's triangleBounds makes them,
    /// f32 as its Float32Array holds them: min x, y, z, then max x, y, z.
    pub bounds: Vec<f32>,
    /// Each triangle's polygon number.
    pub polygons: Vec<u32>,
    /// What the polygon numbers are offset by (0, or BLOCKER_POLYGONS).
    pub offset: f64,
}

/// A triangle of a gather: its corners in world space, and the fixed set and
/// triangle number it is (`set` NO_SET for a body's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tri {
    pub a: [f64; 3],
    pub b: [f64; 3],
    pub c: [f64; 3],
    pub set: u32,
    pub index: u32,
}

/// A polygon of a gather (collision.js's { id, triangles, done, moving }).
#[derive(Debug, Default)]
pub struct Polygon {
    pub id: f64,
    /// It belongs to a moving body (a hull).
    pub moving: bool,
    /// Free for the caller (touchPolygons marks the polygons it used); false
    /// as a gather makes it.
    pub done: bool,
    pub tris: Vec<Tri>,
}

/// A gather's polygons, kept for the next gather to reuse (as collision.js
/// pools them): `list()` holds the last gather's, in order.
pub struct Gathered {
    polygons: Vec<Polygon>,
    spare_indices: Vec<Option<usize>>,
    count: usize,
    grouped: bool,
    index: PolygonIndex,
    // The vehicle phase binds triangle storage to geometry identities at load.
    // Query order may change without repeatedly growing a different slot.
    spares: Vec<(u64, Vec<Tri>)>,
}

impl Default for Gathered {
    fn default() -> Self {
        Gathered { polygons: Vec::new(), spare_indices: Vec::new(), count: 0,
            grouped: false, index: PolygonIndex::new(), spares: Vec::new() }
    }
}

// CollisionWorld assigns a body's public polygon offset when it joins the
// live list. Its immutable triangle storage instead follows the kernel body
// slot, so preparing a hull before that birth cannot tie buffers to an old id.
// Fixed geometry has nonnegative polygon ids; the high bit names body storage.
fn body_polygon_key(body: usize, polygon: u32) -> u64 {
    (1 << 63) | ((body as u64) << 32) | polygon as u64
}

impl Gathered {
    pub fn new() -> Self { Self::default() }

    /// CollisionWorld's pooled polygon records, prepared while loading a body.
    /// Reserve storage only: no cache is observed, filled or invalidated here.
    pub fn reserve(&mut self, polygons: usize, triangles: usize) {
        self.polygons.reserve(polygons.saturating_sub(self.polygons.len()));
        while self.polygons.len() < polygons { self.polygons.push(Polygon::default()); }
        self.spare_indices.resize(self.polygons.len(),None);
        for polygon in &mut self.polygons {
            polygon.tris.reserve(triangles.saturating_sub(polygon.tris.len()));
        }
        while 2 * polygons > self.index.mask + 1 { self.index.grow(); }
    }
    pub fn list(&self) -> &[Polygon] { &self.polygons[..self.count] }
    pub fn list_mut(&mut self) -> &mut [Polygon] { &mut self.polygons[..self.count] }

    /// CollisionWorld's fixed and body geometry bounds the persistent storage.
    /// Keep buffers per polygon id; their total capacity is linear in triangles,
    /// rather than reserving the largest polygon for every possible query slot.
    pub fn prepare(&mut self, s: &State, world: usize, blockers: Option<usize>) {
        self.prepare_geometry(s,world,blockers,0);
        self.polygons.resize_with(self.spares.len(),Polygon::default);
        self.spare_indices.resize(self.polygons.len(),None);
        self.index.reserve(self.spares.len());
    }

    /// Soldier.touchPolygons uses a small pooled query footprint. Only dense
    /// polygons need dedicated triangle buffers; ordinary ones reuse the slot's
    /// prepared minimum. Avoid duplicating every map triangle in this scratch.
    pub fn prepare_dense(&mut self, s: &State, world: usize, minimum: usize) {
        self.prepare_geometry(s,world,None,minimum);
    }

    fn prepare_geometry(&mut self, s: &State, world: usize, blockers: Option<usize>, minimum: usize) {
        self.clear();
        let mut counts = std::collections::BTreeMap::<u64, usize>::new();
        let mut add = |ids: &[u32], offset: f64, body: Option<usize>| {
            for &id in ids {
                let key = body.map_or_else(|| (id as f64 + offset).to_bits(), |body| body_polygon_key(body,id));
                *counts.entry(key).or_default() += 1;
            }
        };
        if let Some(set) = s.worlds[world].statics { let set = &s.sets[set]; add(&set.polygons,set.offset,None); }
        if let Some(set) = blockers { let set = &s.sets[set]; add(&set.polygons,set.offset,None); }
        for &slot in s.worlds[world].order.iter().chain(&s.worlds[world].prepared) {
            let body = &s.bodies[slot as usize]; add(&s.trees[body.tree].polygons,body.offset,Some(slot as usize));
        }
        for (id,n) in counts {
            if n <= minimum { continue; }
            match self.spares.binary_search_by_key(&id, |row| row.0) {
                Ok(i) => { let v = &mut self.spares[i].1; v.reserve(n.saturating_sub(v.len())); }
                Err(i) => self.spares.insert(i,(id,Vec::with_capacity(n))),
            }
        }
    }
    pub fn capacity(&self) -> usize { self.polygons.len() }
    fn clear(&mut self) {
        if !self.spares.is_empty() {
            for (at,p) in self.polygons[..self.count].iter_mut().enumerate() {
                if let Some(i) = self.spare_indices[at] {
                    std::mem::swap(&mut p.tris,&mut self.spares[i].1);
                }
            }
        }
        self.count = 0;
        if self.grouped {
            self.index.clear();
            self.grouped = false;
        }
    }

    /// collision.js polygon(id, moving): the record of polygon `id`, a new
    /// one at the end of the list the first time. Ids in a list are unique,
    /// so how it is found does not change which it is.
    fn polygon(&mut self, id: f64, moving: bool) -> &mut Polygon {
        self.polygon_key(id,moving,id.to_bits())
    }
    // collision.js still groups and visits by the original numeric id.
    // Only reuse of a triangle Vec uses the private retained-geometry key.
    fn polygon_key(&mut self, id: f64, moving: bool, key: u64) -> &mut Polygon {
        let n = self.count;
        if self.grouped {
            if let Some(at) = self.index.get(id) { return &mut self.polygons[at]; }
        } else {
            for i in (0..n).rev() {
                if self.polygons[i].id == id { return &mut self.polygons[i]; }
            }
            if n >= GROUP_SCAN {
                for i in 0..n { self.index.set(self.polygons[i].id, i); }
                self.grouped = true;
            }
        }
        if n == self.polygons.len() { self.polygons.push(Polygon::default()); }
        // Geometry is inserted only by prepare_geometry, after clear returns
        // every borrowed buffer. Its sorted places stay stable for this query,
        // so cleanup can return the buffer without searching for its key again.
        let spare = self.spares.binary_search_by_key(&key, |row| row.0).ok();
        if n == self.spare_indices.len() { self.spare_indices.push(spare); } else { self.spare_indices[n] = spare; }
        self.count += 1;
        if self.grouped { self.index.set(id, n); }
        let p = &mut self.polygons[n];
        if let Some(i) = spare {
            std::mem::swap(&mut p.tris,&mut self.spares[i].1);
        }
        p.id = id;
        p.tris.clear();
        p.done = false;
        p.moving = moving;
        p
    }
}

/// Polygon ids to list places, open addressing emptied by moving to the
/// next stamp (collision.js PolygonIndex).
struct PolygonIndex {
    bits: u32,
    mask: usize,
    count: usize,
    stamp: u32,
    ids: Vec<f64>,
    at: Vec<u32>,
    stamps: Vec<u32>,
}

impl PolygonIndex {
    fn new() -> Self {
        let mut p = PolygonIndex { bits: 0, mask: 0, count: 0, stamp: 1, ids: Vec::new(), at: Vec::new(), stamps: Vec::new() };
        p.allocate(8);
        p
    }
    fn reserve(&mut self, count: usize) {
        let size = (count * 2).max(256).next_power_of_two();
        if size > self.mask + 1 { self.allocate(size.trailing_zeros()); }
    }
    fn allocate(&mut self, bits: u32) {
        let size = 1usize << bits;
        self.bits = bits;
        self.mask = size - 1;
        self.count = 0;
        self.ids = vec![0.0; size];
        self.at = vec![0; size];
        self.stamps = vec![0; size];
    }
    fn clear(&mut self) {
        self.count = 0;
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == u32::MAX {
            self.stamps.iter_mut().for_each(|s| *s = 0);
            self.stamp = 1;
        }
    }
    fn slot(&self, id: f64) -> usize {
        let mut h = ((id as u64 as u32).wrapping_mul(0x9e37_79b1) >> (32 - self.bits)) as usize;
        while self.stamps[h] == self.stamp && self.ids[h] != id { h = (h + 1) & self.mask; }
        h
    }
    fn get(&self, id: f64) -> Option<usize> {
        let h = self.slot(id);
        (self.stamps[h] == self.stamp).then(|| self.at[h] as usize)
    }
    fn set(&mut self, id: f64, at: usize) {
        if 2 * (self.count + 1) > self.mask + 1 { self.grow(); }
        let h = self.slot(id);
        if self.stamps[h] != self.stamp {
            self.stamps[h] = self.stamp;
            self.ids[h] = id;
            self.count += 1;
        }
        self.at[h] = at as u32;
    }
    fn grow(&mut self) {
        let (ids, at, stamps, stamp) = (std::mem::take(&mut self.ids), std::mem::take(&mut self.at), std::mem::take(&mut self.stamps), self.stamp);
        self.allocate(self.bits + 1);
        for i in 0..ids.len() {
            if stamps[i] == stamp { self.set(ids[i], at[i] as usize); }
        }
    }
}

/// A caller's cache of the fixed triangles near where it queries
/// (collision.js gatherCache): one per caller that queries around the same
/// spot every tick. Gathers through it come out exactly as without it.
pub struct GatherCache {
    bounds: Bounds,
    margin: f64,
    fill: u64,
    statics_set: i64,
    statics: Vec<u32>,
    blockers_set: i64,
    blockers: Vec<u32>,
    /// The bodies a gather can meet, as places in the world's order, for
    /// the world, its order's version and the fill they were made for.
    bodies: Vec<u32>,
    bodies_for: Option<(usize, u64, u64)>,
}

impl GatherCache {
    /// Caller construction reserves index lists for all loaded geometry.
    /// A later cache refill changes values and length, never its allocation.
    pub fn reserve(&mut self, statics: usize, blockers: usize, bodies: usize) {
        self.statics.reserve(statics.saturating_sub(self.statics.len()));
        self.blockers.reserve(blockers.saturating_sub(self.blockers.len()));
        self.bodies.reserve(bodies.saturating_sub(self.bodies.len()));
    }
    /// margin: how far past a query's bounds the cached triangles reach
    /// (collision.js's default is 0.75).
    pub fn new(margin: f64) -> Self {
        GatherCache { bounds: EMPTY, margin, fill: 0, statics_set: -1, statics: Vec::new(), blockers_set: -1, blockers: Vec::new(), bodies: Vec::new(), bodies_for: None }
    }
}

/// Box3.intersectsBox: a.intersectsBox(b).
#[inline(always)]
pub fn box_intersects(a: &Bounds, b: &Bounds) -> bool {
    b[3] >= a[0] && b[0] <= a[3] && b[4] >= a[1] && b[1] <= a[4] && b[5] >= a[2] && b[2] <= a[5]
}

/// Box3.containsBox: a.containsBox(b).
#[inline(always)]
pub fn box_contains(a: &Bounds, b: &Bounds) -> bool {
    a[0] <= b[0] && b[3] <= a[3] && a[1] <= b[1] && b[4] <= a[4] && a[2] <= b[2] && b[5] <= a[5]
}

/// Box3.applyMatrix4(m): its eight corners through the matrix, and the box
/// around them.
pub fn box_apply_matrix4(b: &Bounds, e: &[f64; 16]) -> Bounds {
    if b[3] < b[0] || b[4] < b[1] || b[5] < b[2] { return *b; }
    let corners = [
        [b[0], b[1], b[2]], [b[0], b[1], b[5]], [b[0], b[4], b[2]], [b[0], b[4], b[5]],
        [b[3], b[1], b[2]], [b[3], b[1], b[5]], [b[3], b[4], b[2]], [b[3], b[4], b[5]],
    ];
    let mut out = EMPTY;
    for v in corners {
        let p = apply_matrix4(e, v);
        for k in 0..3 {
            out[k] = js_min(out[k], p[k]);
            out[k + 3] = js_max(out[k + 3], p[k]);
        }
    }
    out
}

/// Vector3.addScaledVector: a + v * s.
#[inline(always)]
fn add_scaled(a: [f64; 3], v: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] + v[0] * s, a[1] + v[1] * s, a[2] + v[2] * s]
}

/// Vector3.distanceToSquared: from a to b.
#[inline(always)]
pub fn distance_sq(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    dx * dx + dy * dy + dz * dz
}

/// three's Triangle.closestPointToPoint(p): the point of triangle (a, b, c)
/// nearest p.
pub fn closest_point_to_point(a: [f64; 3], b: [f64; 3], c: [f64; 3], p: [f64; 3]) -> [f64; 3] {
    let vab = sub(b, a);
    let vac = sub(c, a);
    let vap = sub(p, a);
    let d1 = dot(vab, vap);
    let d2 = dot(vac, vap);
    if d1 <= 0.0 && d2 <= 0.0 { return a; }
    let vbp = sub(p, b);
    let d3 = dot(vab, vbp);
    let d4 = dot(vac, vbp);
    if d3 >= 0.0 && d4 <= d3 { return b; }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return add_scaled(a, vab, v);
    }
    let vcp = sub(p, c);
    let d5 = dot(vab, vcp);
    let d6 = dot(vac, vcp);
    if d6 >= 0.0 && d5 <= d6 { return c; }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return add_scaled(a, vac, w);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let vbc = sub(c, b);
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return add_scaled(b, vbc, w);
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    add_scaled(add_scaled(a, vab, v), vac, w)
}

/// A node's bounds, from its f32 words.
#[inline(always)]
fn node_bounds(words: &[u32], node: usize) -> Bounds {
    [f32_at(words, node), f32_at(words, node + 1), f32_at(words, node + 2), f32_at(words, node + 3), f32_at(words, node + 4), f32_at(words, node + 5)]
}

/// A tree triangle's corners.
#[inline(always)]
fn corners<P: Corner + ?Sized>(t: &Tree, p: &P, tri: usize) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let o = tri * 3;
    if t.indexed {
        (p.corner(t.index[o] as usize), p.corner(t.index[o + 1] as usize), p.corner(t.index[o + 2] as usize))
    } else {
        (p.corner(o), p.corner(o + 1), p.corner(o + 2))
    }
}

/// MeshBVH.shapecast({ intersectsBounds, intersectsTriangle }) over every
/// root of a tree: `bounds` decides which children to go down to, and
/// `triangle` gets the number of each leaf triangle met (its corners are
/// corners(), for a visitor that reads them) and ends the cast by returning
/// true, which the cast returns.
fn shapecast<B, T>(t: &Tree, bounds: &mut B, triangle: &mut T) -> bool
where B: FnMut(&Bounds) -> bool, T: FnMut(usize) -> bool {
    for words in &t.roots {
        if traverse(t, words, 0, bounds, triangle) { return true; }
    }
    false
}

fn traverse<B, T>(t: &Tree, words: &[u32], node: usize, bounds: &mut B, triangle: &mut T) -> bool
where B: FnMut(&Bounds) -> bool, T: FnMut(usize) -> bool {
    let w7 = words[node + 7];
    if (w7 >> 16) == 0xffff {
        let offset = words[node + 6] as usize;
        let count = (w7 & 0xffff) as usize;
        for i in offset..offset + count {
            if triangle(t.indirect[i] as usize) { return true; }
        }
        return false;
    }
    let (left, right) = (node + 8, words[node + 6] as usize);
    if bounds(&node_bounds(words, left)) && traverse(t, words, left, bounds, triangle) { return true; }
    if bounds(&node_bounds(words, right)) && traverse(t, words, right, bounds, triangle) { return true; }
    false
}

/// A sphere's bounds: the center less and plus the radius.
#[inline(always)]
pub fn sphere_bounds(center: [f64; 3], radius: f64) -> Bounds {
    [center[0] - radius, center[1] - radius, center[2] - radius, center[0] + radius, center[1] + radius, center[2] + radius]
}

/// collision.js gatherFrom(bvh, polygonOf, offset) with gatherQuery: each
/// triangle of the tree whose bounds meet `bounds`, filed under its polygon
/// (moved by `matrix`, a moving body's, when there is one).
fn gather_tree(s: &State, tree: usize, polygons: &[u32], offset: f64, matrix: Option<&[f64; 16]>, set: u32, body: Option<usize>, bounds: &Bounds, out: &mut Gathered) {
    let t = &s.trees[tree];
    match &s.positions[t.positions] {
        Corners::F32(p) => gather_walk(t, p.as_slice(), polygons, offset, matrix, set, body, bounds, out),
        Corners::F64(p) => gather_walk(t, p.as_slice(), polygons, offset, matrix, set, body, bounds, out),
    }
}

fn gather_walk<P: Corner + ?Sized>(t: &Tree, p: &P, polygons: &[u32], offset: f64, matrix: Option<&[f64; 16]>, set: u32, body: Option<usize>, bounds: &Bounds, out: &mut Gathered) {
    shapecast(t, &mut |node| box_intersects(node, bounds), &mut |index| {
        let (a, b, c) = corners(t, p, index);
        let tri_bounds = [
            js_min(js_min(a[0], b[0]), c[0]), js_min(js_min(a[1], b[1]), c[1]), js_min(js_min(a[2], b[2]), c[2]),
            js_max(js_max(a[0], b[0]), c[0]), js_max(js_max(a[1], b[1]), c[1]), js_max(js_max(a[2], b[2]), c[2]),
        ];
        if !box_intersects(&tri_bounds, bounds) { return false; }
        let tri = match matrix {
            Some(m) => Tri { a: apply_matrix4(m, a), b: apply_matrix4(m, b), c: apply_matrix4(m, c), set, index: index as u32 },
            None => Tri { a, b, c, set, index: index as u32 },
        };
        let id = polygons[index] as f64 + offset;
        let key = body.map_or_else(|| id.to_bits(), |body| body_polygon_key(body,polygons[index]));
        out.polygon_key(id,matrix.is_some(),key).tris.push(tri);
        false
    });
}

/// collision.js fillCache: a cache's list of the set's triangles whose
/// bounds meet `bounds` (the cache's), in the tree's order.
fn fill_cache(s: &State, set: usize, bounds: &Bounds, list: &mut Vec<u32>) {
    let f = &s.sets[set];
    let t = &s.trees[f.tree];
    list.clear();
    let b = &f.bounds;
    // Only each triangle's bounds are read (not its corners).
    shapecast(t, &mut |node| box_intersects(node, bounds), &mut |index: usize| {
        let o = index * 6;
        if (b[o + 3] as f64) < bounds[0] || (b[o] as f64) > bounds[3] || (b[o + 4] as f64) < bounds[1] || (b[o + 1] as f64) > bounds[4]
            || (b[o + 5] as f64) < bounds[2] || (b[o + 2] as f64) > bounds[5] { return false; }
        list.push(index as u32);
        false
    });
}

/// collision.js gatherSet: the cached triangles of a fixed set whose bounds
/// meet the query's, filed by polygon as the tree's own query files them.
fn gather_set(s: &State, set: usize, list: &[u32], bounds: &Bounds, out: &mut Gathered) {
    let f = &s.sets[set];
    let t = &s.trees[f.tree];
    match &s.positions[t.positions] {
        Corners::F32(p) => gather_set_walk(f, t, p.as_slice(), set, list, bounds, out),
        Corners::F64(p) => gather_set_walk(f, t, p.as_slice(), set, list, bounds, out),
    }
}

fn gather_set_walk<P: Corner + ?Sized>(f: &FixedSet, t: &Tree, p: &P, set: usize, list: &[u32], bounds: &Bounds, out: &mut Gathered) {
    let b = &f.bounds;
    let (min_x, min_y, min_z, max_x, max_y, max_z) = (bounds[0], bounds[1], bounds[2], bounds[3], bounds[4], bounds[5]);
    for &index in list {
        let index = index as usize;
        let o = index * 6;
        if (b[o + 3] as f64) < min_x || (b[o] as f64) > max_x || (b[o + 4] as f64) < min_y || (b[o + 1] as f64) > max_y
            || (b[o + 5] as f64) < min_z || (b[o + 2] as f64) > max_z { continue; }
        let (a, bb, c) = corners(t, p, index);
        out.polygon(f.polygons[index] as f64 + f.offset, false).tris.push(Tri { a, b: bb, c, set: set as u32, index: index as u32 });
    }
}

/// collision.js gatherCached: the fixed triangles through the cache,
/// refilled when the query's bounds leave it or it was filled for other
/// barriers.
fn gather_cached(s: &State, statics: usize, blockers: Option<usize>, cache: &mut GatherCache, bounds: &Bounds, out: &mut Gathered) {
    let set = blockers.map_or(-1, |b| b as i64);
    if !box_contains(&cache.bounds, bounds) || cache.blockers_set != set {
        let m = cache.margin;
        let fill = [bounds[0] + -m, bounds[1] + -m, bounds[2] + -m, bounds[3] + m, bounds[4] + m, bounds[5] + m];
        cache.bounds = fill;
        cache.fill += 1;
        fill_cache(s, statics, &fill, &mut cache.statics);
        cache.statics_set = statics as i64;
        match blockers {
            Some(b) => {
                fill_cache(s, b, &fill, &mut cache.blockers);
                cache.blockers_set = b as i64;
            }
            None => cache.blockers_set = -1,
        }
    }
    gather_set(s, statics, &cache.statics, bounds, out);
    if let Some(b) = blockers { gather_set(s, b, &cache.blockers, bounds, out); }
}

/// collision.js cachedBodies: the bodies a cached query can meet, as places
/// in the world's order: every moving one, and the fixed ones whose bounds
/// meet the cache's.
fn cached_bodies<'a>(s: &State, world: usize, cache: &'a mut GatherCache) -> &'a [u32] {
    let w = &s.worlds[world];
    let key = Some((world, w.version, cache.fill));
    if cache.bodies_for != key {
        cache.bodies.clear();
        for (i, &slot) in w.order.iter().enumerate() {
            let body = &s.bodies[slot as usize];
            if body.moving || box_intersects(&body.bounds, &cache.bounds) { cache.bodies.push(i as u32); }
        }
        cache.bodies_for = key;
    }
    &cache.bodies
}

/// gather's first part: the query's bounds (returned), and the buildings'
/// polygons and, with `blockers` (a hull's query: the vehicle-only
/// barriers' set), the barriers', through the cache or not. Starts `out`.
pub fn gather_fixed(s: &State, world: usize, center: [f64; 3], radius: f64, blockers: Option<usize>, cache: Option<&mut GatherCache>, out: &mut Gathered) -> Bounds {
    let bounds = sphere_bounds(center, radius);
    out.clear();
    let w = &s.worlds[world];
    let statics = w.statics.expect("the world's buildings are not set for gathers");
    match cache {
        Some(cache) => gather_cached(s, statics, blockers, cache, &bounds, out),
        None => {
            let f = &s.sets[statics];
            gather_tree(s, f.tree, &f.polygons, f.offset, None, statics as u32, None, &bounds, out);
            if let Some(b) = blockers {
                let f = &s.sets[b];
                gather_tree(s, f.tree, &f.polygons, f.offset, None, b as u32, None, &bounds, out);
            }
        }
    }
    bounds
}

/// gather's part for one body (one whose bounds meet the query's, and which
/// is enabled and not left out): its polygons, a moving one's tested in its
/// frame and brought out to the world's.
pub fn gather_body(s: &State, slot: usize, bounds: &Bounds, out: &mut Gathered) {
    let body = &s.bodies[slot];
    let tree = &s.trees[body.tree];
    if body.moving {
        let local = box_apply_matrix4(bounds, &body.inverse);
        gather_tree(s, body.tree, &tree.polygons, body.offset, Some(&body.matrix), NO_SET, Some(slot), &local, out);
    } else {
        gather_tree(s, body.tree, &tree.polygons, body.offset, None, NO_SET, Some(slot), bounds, out);
    }
}

/// CollisionWorld.gather(center, radius, vehicle, cache, ignoreBody): the
/// polygons near the sphere into `out` (their count returned, the list in
/// `out.list()`), in the JavaScript's order. blockers: a hull's query's
/// vehicle-only barriers (their set), else None. ignore: a body's slot to
/// leave out, or -1. enabled: each body's flag, by its place in the
/// world's order. The world's order must be current (collision.js
/// rayBodies), and its moving bodies' places (moveBody).
#[allow(clippy::too_many_arguments)]
pub fn gather(s: &State, world: usize, center: [f64; 3], radius: f64, blockers: Option<usize>, mut cache: Option<&mut GatherCache>, ignore: i64, enabled: &[u8], out: &mut Gathered) -> usize {
    let bounds = gather_fixed(s, world, center, radius, blockers, cache.as_deref_mut(), out);
    let w = &s.worlds[world];
    let mut each = |place: usize| {
        let slot = w.order[place] as usize;
        let body = &s.bodies[slot];
        if enabled[place] == 0 || slot as i64 == ignore || !box_intersects(&body.bounds, &bounds) { return; }
        gather_body(s, slot, &bounds, out);
    };
    match cache {
        Some(cache) => for &place in cached_bodies(s, world, cache) { each(place as usize) },
        None => for place in 0..w.order.len() { each(place) },
    }
    out.count
}

/// The bodies a gather or overlap must try (not left out, bounds meeting
/// the query's), as places in the world's order, into CANDIDATES for
/// JavaScript to flag; their count.

pub(crate) fn candidates_into(s: &State, world: usize, bounds: &Bounds, ignore: i64, list: Option<&[u32]>, out: &mut [u32]) -> usize {
    let w = &s.worlds[world];
    let mut count = 0;
    let mut each = |place: usize| {
        let slot = w.order[place] as usize;
        if slot as i64 == ignore || !box_intersects(&s.bodies[slot].bounds, bounds) { return; }
        out[count] = place as u32;
        count += 1;
    };
    match list {
        Some(list) => for &place in list { each(place as usize) },
        None => for place in 0..w.order.len() { each(place) },
    }
    count
}

/// The overlap query's test of one triangle: within the radius of the center.
#[inline(always)]
fn overlaps_triangle(a: [f64; 3], b: [f64; 3], c: [f64; 3], center: [f64; 3], radius_sq: f64) -> bool {
    distance_sq(closest_point_to_point(a, b, c, center), center) < radius_sq
}

/// bvh.shapecast(overlapQuery): whether a triangle of the tree whose node
/// bounds meet `bounds` comes within the radius of the center.
fn tree_overlaps(s: &State, tree: usize, bounds: &Bounds, center: [f64; 3], radius_sq: f64) -> bool {
    let t = &s.trees[tree];
    let mut node = |n: &Bounds| box_intersects(n, bounds);
    match &s.positions[t.positions] {
        Corners::F32(p) => shapecast(t, &mut node, &mut |i| { let (a, b, c) = corners(t, p.as_slice(), i); overlaps_triangle(a, b, c, center, radius_sq) }),
        Corners::F64(p) => shapecast(t, &mut node, &mut |i| { let (a, b, c) = corners(t, p.as_slice(), i); overlaps_triangle(a, b, c, center, radius_sq) }),
    }
}

/// The ground cells (x then z ranges) a sphere's bounds cover, as
/// TriangleGrid.near and overlapsSphere find them, each a floor.
#[inline(always)]
fn grid_cells(g: &crate::collision::Grid, center: [f64; 3], radius: f64) -> (f64, f64, f64, f64) {
    let size = g.size;
    let x0 = ((center[0] - radius - g.min_x) / size).floor();
    let x1 = ((center[0] + radius - g.min_x) / size).floor();
    let z0 = ((center[2] - radius - g.min_z) / size).floor();
    let z1 = ((center[2] + radius - g.min_z) / size).floor();
    (x0, x1, z0, z1)
}

/// TriangleGrid.nextSerial.
#[inline(always)]
fn next_serial(g: &mut crate::collision::Grid) {
    g.serial = g.serial.wrapping_add(1);
    if g.serial == 0xffff_ffff {
        g.stamp.iter_mut().for_each(|v| *v = 0);
        g.serial = 1;
    }
}

/// Each triangle filed in the cells a sphere's bounds cover, once, in the
/// grid's order, until `visit` (given the grid's index and the triangle's
/// number) returns true, which this then returns.
fn grid_each<V: FnMut(&[u32], usize) -> bool>(g: &mut crate::collision::Grid, center: [f64; 3], radius: f64, mut visit: V) -> bool {
    let (x0, x1, z0, z1) = grid_cells(g, center, radius);
    next_serial(g);
    let serial = g.serial;
    let crate::collision::Grid { start, cells, stamp, index, nx, nz, .. } = g;
    let (nx, nz) = (*nx, *nz);
    let mut z = js_max(0.0, z0);
    while z <= js_min(nz - 1.0, z1) {
        let mut x = js_max(0.0, x0);
        while x <= js_min(nx - 1.0, x1) {
            let cell = (z * nx + x) as usize;
            for i in start[cell] as usize..start[cell + 1] as usize {
                let t = cells[i] as usize;
                if stamp[t] == serial { continue; }
                stamp[t] = serial;
                if visit(index, t) { return true; }
            }
            x += 1.0;
        }
        z += 1.0;
    }
    false
}

/// A grid triangle's corners, from its index and positions.
#[inline(always)]
fn grid_corners(index: &[u32], p: &Corners, t: usize) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let o = t * 3;
    let (a, b, c) = (index[o] as usize, index[o + 1] as usize, index[o + 2] as usize);
    match p {
        Corners::F32(p) => (p.corner(a), p.corner(b), p.corner(c)),
        Corners::F64(p) => (p.corner(a), p.corner(b), p.corner(c)),
    }
}

/// TriangleGrid.overlapsSphere(center, radius).
pub fn grid_overlaps(s: &mut State, grid: usize, center: [f64; 3], radius: f64) -> bool {
    let State { grids, positions, .. } = s;
    let g = &mut grids[grid];
    let p = &positions[g.positions];
    let radius_sq = radius * radius;
    grid_each(g, center, radius, |index, t| {
        let (a, b, c) = grid_corners(index, p, t);
        overlaps_triangle(a, b, c, center, radius_sq)
    })
}

/// TriangleGrid.near(center, radius): the triangles filed in the cells the
/// sphere's bounds cover, each once, in order (their numbers, into `out`).
pub fn terrain_near(s: &mut State, grid: usize, center: [f64; 3], radius: f64, out: &mut Vec<u32>) {
    out.clear();
    grid_each(&mut s.grids[grid], center, radius, |_, t| { out.push(t as u32); false });
}

/// A grid triangle's corners (for terrain_near's numbers).
pub fn terrain_triangle(s: &State, grid: usize, t: usize) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let g = &s.grids[grid];
    grid_corners(&g.index, &s.positions[g.positions], t)
}

/// overlapsSphere's first part: the buildings and the terrain.
pub fn overlaps_fixed(s: &mut State, world: usize, center: [f64; 3], radius: f64) -> bool {
    let bounds = sphere_bounds(center, radius);
    let (tree, grid) = (s.worlds[world].tree, s.worlds[world].grid);
    if tree_overlaps(s, tree, &bounds, center, radius * radius) { return true; }
    match grid { Some(g) => grid_overlaps(s, g, center, radius), None => false }
}

/// overlapsSphere's part for one body (enabled, not left out, its bounds
/// meeting the sphere's): a moving one's tested in its frame.
pub fn overlaps_body(s: &State, slot: usize, center: [f64; 3], radius: f64) -> bool {
    let body = &s.bodies[slot];
    let bounds = sphere_bounds(center, radius);
    let radius_sq = radius * radius;
    if body.moving {
        let local = box_apply_matrix4(&bounds, &body.inverse);
        tree_overlaps(s, body.tree, &local, apply_matrix4(&body.inverse, center), radius_sq)
    } else {
        tree_overlaps(s, body.tree, &bounds, center, radius_sq)
    }
}

/// CollisionWorld.overlapsSphere(center, radius, ignoreBody): whether any
/// triangle of the buildings, the terrain or an enabled body (not `ignore`,
/// a slot or -1) comes within the radius of the center. enabled: each
/// body's flag by its place in the world's order.
pub fn overlaps_sphere(s: &mut State, world: usize, center: [f64; 3], radius: f64, ignore: i64, enabled: &[u8]) -> bool {
    if overlaps_fixed(s, world, center, radius) { return true; }
    let bounds = sphere_bounds(center, radius);
    let w = &s.worlds[world];
    for (place, &slot) in w.order.iter().enumerate() {
        let slot = slot as usize;
        if enabled[place] == 0 || slot as i64 == ignore || !box_intersects(&s.bodies[slot].bounds, &bounds) { continue; }
        if overlaps_body(s, slot, center, radius) { return true; }
    }
    false
}

/// CollisionWorld.ground(x, z, top): the terrain's height and its face
/// normal (turned up) straight under (x, z), searched down from `top`, or
/// None where there is no terrain.
pub fn ground(s: &mut State, world: usize, x: f64, z: f64, top: f64) -> Option<(f64, [f64; 3])> {
    let grid = s.worlds[world].grid?;
    let ray = Ray { ox: x, oy: top, oz: z, dx: 0.0, dy: -1.0, dz: 0.0 };
    let hit = grid_raycast(s, grid, &ray, f64::INFINITY)?;
    let n = if hit.ny < 0.0 { [-hit.nx, -hit.ny, -hit.nz] } else { [hit.nx, hit.ny, hit.nz] };
    Some((hit.py, n))
}

/// Vector3.applyQuaternion(q), q as x, y, z, w.
#[inline(always)]
pub fn apply_quaternion(v: [f64; 3], q: [f64; 4]) -> [f64; 3] {
    let (vx, vy, vz) = (v[0], v[1], v[2]);
    let (qx, qy, qz, qw) = (q[0], q[1], q[2], q[3]);
    let tx = 2.0 * (qy * vz - qz * vy);
    let ty = 2.0 * (qz * vx - qx * vz);
    let tz = 2.0 * (qx * vy - qy * vx);
    [vx + qw * tx + qy * tz - qz * ty, vy + qw * ty + qz * tx - qx * tz, vz + qw * tz + qx * ty - qy * tx]
}

/// A hull's wall probes as vehicles.js pushWalls runs them: the probes'
/// places in the hull's frame and their radius, the hull's turn (q), a
/// walker's turn and place a step ago (own, previous; its probes give way
/// to another hull only as far as they came), and whether the terrain's
/// steep faces count.
pub struct Probes<'a> {
    pub points: &'a [[f64; 3]],
    pub radius: f64,
    pub q: [f64; 4],
    pub own: Option<[f64; 4]>,
    pub previous: [f64; 3],
    pub terrain: bool,
}

/// The hull a probe pushes (vehicles.js pushOut's state): its place and
/// velocity, and whether its turn this tick is to be undone (yaw).
pub struct Pushed {
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub undo_turn: bool,
}

/// vehicles.js pushOut(tri, oldYaw, hull): a probe at `point` out of a wall
/// triangle (not a floor: suspension handles those), and the hull with it.
fn push_out(a: [f64; 3], b: [f64; 3], c: [f64; 3], point: &mut [f64; 3], start: &[f64; 3], hull: bool, radius: f64, h: &mut Pushed) {
    let face = triangle_normal(a, b, c);
    if face[1].abs() > 0.65 { return; }
    let nearest = closest_point_to_point(a, b, c, *point);
    let v = sub(*point, nearest);
    let distance = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if distance >= radius || distance < 1e-6 { return; }
    let k = 1.0 / distance;
    let n = [v[0] * k, v[1] * k, v[2] * k];
    let mut depth = radius - distance + 0.002;
    if hull {
        // How far this probe came toward the other hull this step.
        let came = -(n[0] * (point[0] - start[0]) + n[1] * (point[1] - start[1]) + n[2] * (point[2] - start[2]));
        depth = js_min(depth, js_max(0.0, came));
        if !(depth > 0.0) { return; }
    }
    h.position = add_scaled(h.position, n, depth);
    *point = add_scaled(*point, n, depth);
    let into = dot(h.velocity, n);
    if into < 0.0 { h.velocity = add_scaled(h.velocity, n, -into); }
    if depth > 0.15 { h.undo_turn = true; }
}

/// vehicles.js pushWalls: two passes over the probes, each pushed out of the
/// polygons of the buildings, props, barriers (blockers) and other hulls
/// gathered about it (through its cache), then out of the terrain's
/// triangles near it. enabled: each body's flag by its place in the world's
/// order (the hull's own body off). `out` and `near` are scratch.
/// Each probe lends its cache only for its own gather. Indexed banks can
/// therefore reuse a cache without constructing aliased mutable references.
pub trait ProbeCaches {
    fn cache(&mut self, probe: usize) -> Option<&mut GatherCache>;
}
impl ProbeCaches for [Option<&mut GatherCache>] {
    fn cache(&mut self, probe: usize) -> Option<&mut GatherCache> { self.get_mut(probe)?.as_deref_mut() }
}
struct IndexedCaches<'a> { ids: &'a [i64], all: &'a mut [GatherCache] }
impl ProbeCaches for IndexedCaches<'_> {
    fn cache(&mut self, probe: usize) -> Option<&mut GatherCache> {
        let id = *self.ids.get(probe)?;
        if id < 0 { None } else { self.all.get_mut(id as usize) }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn push_walls(s: &mut State, world: usize, p: &Probes, caches: &mut (impl ProbeCaches + ?Sized), blockers: Option<usize>, enabled: &[u8], h: &mut Pushed, out: &mut Gathered, near: &mut Vec<u32>) {
    let grid = s.worlds[world].grid;
    let mut start = [0.0; 3];
    for _pass in 0..2 {
        for (i, probe) in p.points.iter().enumerate() {
            let at = apply_quaternion(*probe, p.q);
            let mut point = [at[0] + h.position[0], at[1] + h.position[1], at[2] + h.position[2]];
            if let Some(own) = p.own {
                let was = apply_quaternion(*probe, own);
                start = [was[0] + p.previous[0], was[1] + p.previous[1], was[2] + p.previous[2]];
            }
            gather(s, world, point, p.radius, blockers, caches.cache(i), -1, enabled, out);
            for polygon in out.list() {
                let hull = p.own.is_some() && polygon.moving;
                for t in &polygon.tris { push_out(t.a, t.b, t.c, &mut point, &start, hull, p.radius, h); }
            }
            if let (true, Some(g)) = (p.terrain, grid) {
                terrain_near(s, g, point, p.radius, near);
                for &t in near.iter() {
                    let (a, b, c) = terrain_triangle(s, g, t as usize);
                    push_out(a, b, c, &mut point, &start, false, p.radius, h);
                }
            }
        }
    }
}

// The exports for collision.js. A JavaScript gather or overlap runs in two
// calls: the first does the fixed parts and names the bodies to try in
// CANDIDATES; JavaScript writes whether each is enabled in ENABLED; the
// second does those bodies.

/// Retained collision.js gather banks. Caches and the current two-call
/// query belong to the collision owner, independently of movement's gathers.
pub(crate) struct Gathers {
    gathered: Option<Gathered>,
    pub(crate) caches: Vec<GatherCache>,
    query: Bounds,
    out: Vec<f64>, near: Vec<u32>, pub(crate) input: Vec<f64>,
    push_points: Vec<[f64; 3]>, push_ids: Vec<i64>,
    pub(crate) places: [u8; MAX_BODIES], push_out: Option<Gathered>, push_near: Vec<u32>,
}
impl Gathers {
    pub const fn new() -> Self {
        Self { gathered: None, caches: Vec::new(), query: EMPTY, out: Vec::new(), near: Vec::new(), input: Vec::new(), push_points: Vec::new(), push_ids: Vec::new(), places: [0; MAX_BODIES], push_out: None, push_near: Vec::new() }
    }
}

// A narrow facade loan for vehicle collision services, never a whole-owner
// borrow. Native owned callers use their explicit gather bank instead.
pub(crate) fn caches() -> &'static mut Vec<GatherCache> {
    unsafe { &mut *std::ptr::addr_of_mut!(crate::abi::cold().collision.gathers.caches) }
}

/// A fixed set over tree `tree` of `triangles` triangles, its polygon
/// numbers offset by `offset`; fill its bounds and polygon numbers at
/// `set_bounds` and `set_polygons`.
#[no_mangle]
pub extern "C" fn set_new(tree: u32, triangles: u32, offset: f64) -> u32 { super::owner().set_new(tree, triangles, offset) }

#[no_mangle]
pub extern "C" fn set_bounds(id: u32) -> *mut f32 { super::owner().set_bounds(id) }

#[no_mangle]
pub extern "C" fn set_polygons(id: u32) -> *mut u32 { super::owner().set_polygons(id) }

/// The world's buildings as fixed set `set`.
#[no_mangle]
pub extern "C" fn world_statics(world: u32, set: u32) { super::owner().world_statics(world, set) }

/// Room for a tree's `count` polygon numbers (a body's), to fill at the pointer.
#[no_mangle]
pub extern "C" fn tree_polygons(id: u32, count: u32) -> *mut u32 { super::owner().tree_polygons(id, count) }

/// A body's polygon number offset.
#[no_mangle]
pub extern "C" fn body_offset(slot: u32, offset: f64) { super::owner().body_offset(slot, offset) }

/// A gather cache for JavaScript; returns its id.
#[no_mangle]
pub extern "C" fn cache_new(margin: f64) -> u32 { super::owner().cache_new(margin) }

/// gather's first call, the center in IO 0-2 and the radius in 3: the fixed
/// parts (through cache `cache`, -1 none; with the barriers of set
/// `blockers`, -1 none), and the bodies to try (`ignore` a slot or -1) in
/// CANDIDATES; returns how many.
#[no_mangle]
pub extern "C" fn gather_begin(world: u32, cache: i32, blockers: i32, ignore: i32) -> u32 { super::owner().gather_begin(world, cache, blockers, ignore) }

/// Fixed gather and candidate selection, with all scratch owned by the caller.
pub(crate) fn gather_candidates_into(s: &State, world: usize, center: [f64; 3], radius: f64, blockers: Option<usize>, cache: Option<&mut GatherCache>, ignore: i64, out: &mut Gathered, candidates: &mut [u32]) -> (Bounds, usize) {
    if let Some(cache) = cache {
        let bounds = gather_fixed(s, world, center, radius, blockers, Some(&mut *cache), out);
        let list = cached_bodies(s, world, cache);
        let count = candidates_into(s, world, &bounds, ignore, Some(list), candidates);
        (bounds, count)
    } else {
        let bounds = gather_fixed(s, world, center, radius, blockers, None, out);
        let count = candidates_into(s, world, &bounds, ignore, None, candidates);
        (bounds, count)
    }
}

/// gather's second call, over the first `count` candidates whose ENABLED is
/// written: their polygons, and the gather written out (OUT, its address in
/// IO 58 and length in 59); returns the polygon count.
#[no_mangle]
pub extern "C" fn gather_end(world: u32, count: u32) -> u32 { super::owner().gather_end(world, count) }

/// Complete and encode the gather without borrowing any module storage.
pub(crate) fn gather_complete(s: &State, world: usize, bounds: &Bounds, candidates: &[u32], enabled: &[u8], out: &mut Gathered, w: &mut Vec<f64>) -> usize {
    let order = &s.worlds[world].order;
    for c in 0..candidates.len() {
        if enabled[c] == 0 { continue; }
        gather_body(s, order[candidates[c] as usize] as usize, bounds, out);
    }
    let list = out.list();
    w.clear();
    for p in list {
        w.push(p.id);
        w.push(if p.moving { 1.0 } else { 0.0 });
        w.push(p.tris.len() as f64);
        for t in &p.tris {
            if t.set == NO_SET {
                w.push(-1.0);
                w.push(t.index as f64);
                w.extend_from_slice(&[t.a[0], t.a[1], t.a[2], t.b[0], t.b[1], t.b[2], t.c[0], t.c[1], t.c[2]]);
            } else {
                w.push(t.set as f64);
                w.push(t.index as f64);
            }
        }
    }
    list.len()
}

/// Room for `count` f64s of input; its address.
#[no_mangle]
pub extern "C" fn input_reserve(count: u32) -> *mut f64 { super::owner().input_reserve(count) }

/// Where each body's enabled flag goes, by its place in the world's order.
#[no_mangle]
pub extern "C" fn places_ptr() -> *mut u8 { super::owner().places_ptr() }

/// pushWalls for a hull, its input written at input_reserve's address: 0-3
/// its turn q, 4-7 its turn a step ago and 8 1 if it has one, 9-11 its
/// place a step ago, 12-14 its place, 15-17 its velocity, 18 the probes'
/// radius, 19 1 if the terrain counts, 20 the barriers' set (-1 none), 21
/// the probe count, then from 22 each probe's place in the hull's frame and
/// its cache (-1 none), four each; bodies' flags at places_ptr. Its place
/// and velocity come back in IO 0-5, and in IO 6 1 if its turn is undone.
#[no_mangle]
pub extern "C" fn push_walls_ray(world: u32) { super::owner().push_walls_ray(world) }

/// overlapsSphere's first call, the center in IO 0-2 and the radius in 3:
/// -1 if the buildings or the terrain overlap it, else the bodies to try
/// (`ignore` a slot or -1) in CANDIDATES and how many.
#[no_mangle]
pub extern "C" fn overlap_begin(world: u32, ignore: i32) -> i32 { super::owner().overlap_begin(world, ignore) }

/// overlapsSphere's second call over the first `count` candidates whose
/// ENABLED is written: 1 if one overlaps, else 0.
#[no_mangle]
pub extern "C" fn overlap_end(world: u32, count: u32) -> u32 { super::owner().overlap_end(world, count) }

pub(crate) fn overlaps_candidates(s: &State, world: usize, center: [f64; 3], radius: f64, candidates: &[u32], enabled: &[u8]) -> bool {
    let order = &s.worlds[world].order;
    for c in 0..candidates.len() {
        if enabled[c] != 0 && overlaps_body(s, order[candidates[c] as usize] as usize, center, radius) { return true; }
    }
    false
}

/// terrainNear's triangles (the center in IO 0-2, the radius in 3): their
/// numbers at the address in IO 58; returns how many.
#[no_mangle]
pub extern "C" fn terrain_near_ray(grid: u32) -> u32 { super::owner().terrain_near_ray(grid) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_point_regions() {
        let (a, b, c) = ([0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 0.0, 2.0]);
        assert_eq!(closest_point_to_point(a, b, c, [-1.0, 1.0, -1.0]), a);
        assert_eq!(closest_point_to_point(a, b, c, [3.0, 1.0, -1.0]), b);
        assert_eq!(closest_point_to_point(a, b, c, [-1.0, 1.0, 3.0]), c);
        assert_eq!(closest_point_to_point(a, b, c, [1.0, 1.0, -1.0]), [1.0, 0.0, 0.0]);
        assert_eq!(closest_point_to_point(a, b, c, [0.5, 5.0, 0.5]), [0.5, 0.0, 0.5]);
        assert_eq!(closest_point_to_point(a, b, c, [2.0, 0.0, 2.0]), [1.0, 0.0, 1.0]);
    }

    #[test]
    fn boxes_keep_three_rules() {
        let a = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        assert!(box_intersects(&a, &[1.0, 1.0, 1.0, 2.0, 2.0, 2.0]));
        assert!(!box_intersects(&a, &[1.5, 0.0, 0.0, 2.0, 1.0, 1.0]));
        assert!(!box_intersects(&a, &[f64::NAN, 0.0, 0.0, 2.0, 1.0, 1.0]));
        assert!(box_contains(&[-1.0, -1.0, -1.0, 2.0, 2.0, 2.0], &a));
        assert!(!box_contains(&EMPTY, &a));
        let turn = [0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 5.0, 0.0, 0.0, 1.0];
        assert_eq!(box_apply_matrix4(&a, &turn), [5.0, 0.0, -1.0, 6.0, 1.0, 0.0]);
        assert_eq!(box_apply_matrix4(&EMPTY, &turn), EMPTY);
    }

    #[test]
    fn polygon_index_files_past_the_scan() {
        let mut g = Gathered::new();
        for round in 0..3 {
            g.clear();
            for i in 0..200u32 {
                let id = (i * 7919 % 211) as f64 + round as f64 * 1e6;
                g.polygon(id, false).tris.push(Tri { a: [0.0; 3], b: [0.0; 3], c: [0.0; 3], set: 0, index: i });
                g.polygon(id, false);
            }
            assert_eq!(g.list().len(), 200);
            for (i, p) in g.list().iter().enumerate() {
                assert_eq!(p.id, ((i as u32) * 7919 % 211) as f64 + round as f64 * 1e6);
                assert_eq!(p.tris.len(), 1);
            }
        }
    }

    #[test]
    fn body_triangle_storage_survives_assignment_of_its_live_polygon_offset() {
        let key = body_polygon_key(3,7);
        let mut gathered = Gathered::new();
        gathered.reserve(1,0);
        gathered.spares.push((key,Vec::with_capacity(3)));
        let storage = gathered.spares[0].1.as_ptr();
        // A prepared hull gets its public offset when it joins world.order.
        // Both observations keep the exact public id and the same storage.
        for offset in [0.0,16777216.0,33554432.0] {
            gathered.clear();
            // Loading geometry between queries can move the sorted places.
            // A subsequent query resolves the new place before borrowing it.
            if offset == 16777216.0 { gathered.spares.insert(0,(0,Vec::with_capacity(1))); }
            let polygon = gathered.polygon_key(offset + 7.0,true,key);
            polygon.tris.push(Tri { a:[0.0;3],b:[1.0,0.0,0.0],c:[0.0,0.0,1.0],set:NO_SET,index:0 });
            assert_eq!(polygon.id,offset + 7.0);
            assert_eq!(polygon.tris.as_ptr(),storage);
            assert_eq!(gathered.list().len(),1);
        }
    }
}

impl CollisionOwner {
    pub fn set_new(&mut self, tree: u32, triangles: u32, offset: f64) -> u32 {
        let s = &mut self.state;
        s.sets.push(FixedSet { tree: tree as usize, bounds: vec![0.0; triangles as usize * 6], polygons: vec![0; triangles as usize], offset });
        (s.sets.len() - 1) as u32
    }
    pub fn set_bounds(&mut self, id: u32) -> *mut f32 { self.state.sets[id as usize].bounds.as_mut_ptr() }
    pub fn set_polygons(&mut self, id: u32) -> *mut u32 { self.state.sets[id as usize].polygons.as_mut_ptr() }
    pub fn world_statics(&mut self, world: u32, set: u32) { self.state.worlds[world as usize].statics = Some(set as usize); }
    pub fn tree_polygons(&mut self, id: u32, count: u32) -> *mut u32 {
        let t = &mut self.state.trees[id as usize];
        t.polygons.clear();
        t.polygons.resize(count as usize, 0);
        t.polygons.as_mut_ptr()
    }
    pub fn body_offset(&mut self, slot: u32, offset: f64) { self.state.bodies[slot as usize].offset = offset; }
    pub fn cache_new(&mut self, margin: f64) -> u32 {
        self.gathers.caches.push(GatherCache::new(margin));
        (self.gathers.caches.len() - 1) as u32
    }
    pub fn gather_begin(&mut self, world: u32, cache: i32, blockers: i32, ignore: i32) -> u32 {
        let v = &mut self.io;
        let (center, radius) = ([v[0], v[1], v[2]], v[3]);
        let s = &mut self.state;
        let world = world as usize;
        let blockers = (blockers >= 0).then_some(blockers as usize);
        let cache = if cache >= 0 { Some(&mut self.gathers.caches[cache as usize]) } else { None };
        let candidates = &mut self.candidates;
        let (bounds, count) = gather_candidates_into(s, world, center, radius, blockers, cache, ignore as i64, self.gathers.gathered.get_or_insert_with(Gathered::new), candidates);
        self.gathers.query = bounds;
        count as u32
    }
    pub fn gather_end(&mut self, world: u32, count: u32) -> u32 {
        let s = &mut self.state;
        let out = self.gathers.gathered.get_or_insert_with(Gathered::new);
        let bounds = self.gathers.query;
        let candidates = &self.candidates;
        let enabled = &self.enabled;
        let w = &mut self.gathers.out;
        let count = gather_complete(s, world as usize, &bounds, &candidates[..(count as usize).min(MAX_BODIES)], &enabled[..(count as usize).min(MAX_BODIES)], out, w);
        let v = &mut self.io;
        v[58] = w.as_ptr() as usize as f64;
        v[59] = w.len() as f64;
        count as u32
    }
    pub fn input_reserve(&mut self, count: u32) -> *mut f64 {
        let v = &mut self.gathers.input;
        if v.len() < count as usize { v.resize(count as usize, 0.0); }
        // rayPushWalls writes four words per probe after its fixed header.
        // Reserve the derived banks at the same admission boundary as input.
        let probes = (count as usize).saturating_sub(22) / 4;
        let points = &mut self.gathers.push_points;
        points.reserve(probes.saturating_sub(points.len()));
        let ids = &mut self.gathers.push_ids;
        ids.reserve(probes.saturating_sub(ids.len()));
        v.as_mut_ptr()
    }
    pub fn places_ptr(&mut self) -> *mut u8 { self.gathers.places.as_mut_ptr() }
    pub fn push_walls_ray(&mut self, world: u32) {
        let v = &self.gathers.input;
        let s = &mut self.state;
        if v.len() < 22 { return; }
        let count = (v[21] as usize).min((v.len() - 22) / 4);
        let points = &mut self.gathers.push_points;
        let ids = &mut self.gathers.push_ids;
        points.clear(); ids.clear();
        for i in 0..count {
            let o = 22 + i * 4;
            points.push([v[o], v[o + 1], v[o + 2]]);
            ids.push(v[o + 3] as i64);
        }
        let p = Probes { points, radius: v[18], q: [v[0], v[1], v[2], v[3]], own: (v[8] != 0.0).then_some([v[4], v[5], v[6], v[7]]),
            previous: [v[9], v[10], v[11]], terrain: v[19] != 0.0 };
        let mut h = Pushed { position: [v[12], v[13], v[14]], velocity: [v[15], v[16], v[17]], undo_turn: false };
        let mut caches = IndexedCaches { ids, all: &mut self.gathers.caches };
        let blockers = (v[20] >= 0.0).then_some(v[20] as usize);
        let n = s.worlds[world as usize].order.len();
        let places: &[u8; MAX_BODIES] = &self.gathers.places;
        let enabled = &places[..n];
        let out = self.gathers.push_out.get_or_insert_with(Gathered::new);
        let near = &mut self.gathers.push_near;
        push_walls(s, world as usize, &p, &mut caches, blockers, enabled, &mut h, out, near);
        let io = &mut self.io;
        io[0] = h.position[0]; io[1] = h.position[1]; io[2] = h.position[2];
        io[3] = h.velocity[0]; io[4] = h.velocity[1]; io[5] = h.velocity[2];
        io[6] = if h.undo_turn { 1.0 } else { 0.0 };
    }
    pub fn overlap_begin(&mut self, world: u32, ignore: i32) -> i32 {
        let v = &mut self.io;
        let (center, radius) = ([v[0], v[1], v[2]], v[3]);
        let s = &mut self.state;
        if overlaps_fixed(s, world as usize, center, radius) { return -1; }
        candidates_into(s, world as usize, &sphere_bounds(center, radius), ignore as i64, None, &mut self.candidates) as i32
    }
    pub fn overlap_end(&mut self, world: u32, count: u32) -> u32 {
        let v = &mut self.io;
        let (center, radius) = ([v[0], v[1], v[2]], v[3]);
        let candidates = &self.candidates;
        let enabled = &self.enabled;
        overlaps_candidates(&mut self.state, world as usize, center, radius, &candidates[..(count as usize).min(MAX_BODIES)], &enabled[..(count as usize).min(MAX_BODIES)]) as u32
    }
    pub fn terrain_near_ray(&mut self, grid: u32) -> u32 {
        let v = &mut self.io;
        let (center, radius) = ([v[0], v[1], v[2]], v[3]);
        let out = &mut self.gathers.near;
        terrain_near(&mut self.state, grid as usize, center, radius, out);
        v[58] = out.as_ptr() as usize as f64;
        out.len() as u32
    }
}
