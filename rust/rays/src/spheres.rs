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

use crate::{apply_matrix4, candidate, dot, enabled, f32_at, grid_raycast, io, js_max, js_min, state, sub, Corner, Corners, Ray, State, Tree};
use std::ptr::addr_of_mut;

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
    count: usize,
    grouped: bool,
    index: PolygonIndex,
}

impl Default for Gathered {
    fn default() -> Self { Gathered { polygons: Vec::new(), count: 0, grouped: false, index: PolygonIndex::new() } }
}

impl Gathered {
    pub fn new() -> Self { Self::default() }
    pub fn list(&self) -> &[Polygon] { &self.polygons[..self.count] }
    pub fn list_mut(&mut self) -> &mut [Polygon] { &mut self.polygons[..self.count] }

    fn clear(&mut self) {
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
        self.count += 1;
        if self.grouped { self.index.set(id, n); }
        let p = &mut self.polygons[n];
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
/// `triangle` gets each leaf triangle met (its number and corners) and ends
/// the cast by returning true, which the cast returns.
fn shapecast<P, B, T>(t: &Tree, p: &P, bounds: &mut B, triangle: &mut T) -> bool
where P: Corner + ?Sized, B: FnMut(&Bounds) -> bool, T: FnMut(usize, [f64; 3], [f64; 3], [f64; 3]) -> bool {
    for words in &t.roots {
        if traverse(t, p, words, 0, bounds, triangle) { return true; }
    }
    false
}

fn traverse<P, B, T>(t: &Tree, p: &P, words: &[u32], node: usize, bounds: &mut B, triangle: &mut T) -> bool
where P: Corner + ?Sized, B: FnMut(&Bounds) -> bool, T: FnMut(usize, [f64; 3], [f64; 3], [f64; 3]) -> bool {
    let w7 = words[node + 7];
    if (w7 >> 16) == 0xffff {
        let offset = words[node + 6] as usize;
        let count = (w7 & 0xffff) as usize;
        for i in offset..offset + count {
            let tri = t.indirect[i] as usize;
            let (a, b, c) = corners(t, p, tri);
            if triangle(tri, a, b, c) { return true; }
        }
        return false;
    }
    let (left, right) = (node + 8, words[node + 6] as usize);
    if bounds(&node_bounds(words, left)) && traverse(t, p, words, left, bounds, triangle) { return true; }
    if bounds(&node_bounds(words, right)) && traverse(t, p, words, right, bounds, triangle) { return true; }
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
fn gather_tree(s: &State, tree: usize, polygons: &[u32], offset: f64, matrix: Option<&[f64; 16]>, set: u32, bounds: &Bounds, out: &mut Gathered) {
    let t = &s.trees[tree];
    match &s.positions[t.positions] {
        Corners::F32(p) => gather_walk(t, p.as_slice(), polygons, offset, matrix, set, bounds, out),
        Corners::F64(p) => gather_walk(t, p.as_slice(), polygons, offset, matrix, set, bounds, out),
    }
}

fn gather_walk<P: Corner + ?Sized>(t: &Tree, p: &P, polygons: &[u32], offset: f64, matrix: Option<&[f64; 16]>, set: u32, bounds: &Bounds, out: &mut Gathered) {
    shapecast(t, p, &mut |node| box_intersects(node, bounds), &mut |index, a, b, c| {
        let tri_bounds = [
            js_min(js_min(a[0], b[0]), c[0]), js_min(js_min(a[1], b[1]), c[1]), js_min(js_min(a[2], b[2]), c[2]),
            js_max(js_max(a[0], b[0]), c[0]), js_max(js_max(a[1], b[1]), c[1]), js_max(js_max(a[2], b[2]), c[2]),
        ];
        if !box_intersects(&tri_bounds, bounds) { return false; }
        let tri = match matrix {
            Some(m) => Tri { a: apply_matrix4(m, a), b: apply_matrix4(m, b), c: apply_matrix4(m, c), set, index: index as u32 },
            None => Tri { a, b, c, set, index: index as u32 },
        };
        out.polygon(polygons[index] as f64 + offset, matrix.is_some()).tris.push(tri);
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
    let mut visit = |index: usize, _: [f64; 3], _: [f64; 3], _: [f64; 3]| {
        let o = index * 6;
        if (b[o + 3] as f64) < bounds[0] || (b[o] as f64) > bounds[3] || (b[o + 4] as f64) < bounds[1] || (b[o + 1] as f64) > bounds[4]
            || (b[o + 5] as f64) < bounds[2] || (b[o + 2] as f64) > bounds[5] { return false; }
        list.push(index as u32);
        false
    };
    match &s.positions[t.positions] {
        Corners::F32(p) => shapecast(t, p.as_slice(), &mut |node| box_intersects(node, bounds), &mut visit),
        Corners::F64(p) => shapecast(t, p.as_slice(), &mut |node| box_intersects(node, bounds), &mut visit),
    };
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
            gather_tree(s, f.tree, &f.polygons, f.offset, None, statics as u32, &bounds, out);
            if let Some(b) = blockers {
                let f = &s.sets[b];
                gather_tree(s, f.tree, &f.polygons, f.offset, None, b as u32, &bounds, out);
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
        gather_tree(s, body.tree, &tree.polygons, body.offset, Some(&body.matrix), NO_SET, &local, out);
    } else {
        gather_tree(s, body.tree, &tree.polygons, body.offset, None, NO_SET, bounds, out);
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
fn candidates_for(s: &State, world: usize, bounds: &Bounds, ignore: i64, list: Option<&[u32]>) -> usize {
    let w = &s.worlds[world];
    let out = unsafe { &mut *addr_of_mut!(crate::CANDIDATES) };
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
    let mut tri = |_: usize, a, b, c| overlaps_triangle(a, b, c, center, radius_sq);
    match &s.positions[t.positions] {
        Corners::F32(p) => shapecast(t, p.as_slice(), &mut node, &mut tri),
        Corners::F64(p) => shapecast(t, p.as_slice(), &mut node, &mut tri),
    }
}

/// The ground cells (x then z ranges) a sphere's bounds cover, as
/// TriangleGrid.near and overlapsSphere find them, each a floor.
#[inline(always)]
fn grid_cells(g: &crate::Grid, center: [f64; 3], radius: f64) -> (f64, f64, f64, f64) {
    let size = g.size;
    let x0 = ((center[0] - radius - g.min_x) / size).floor();
    let x1 = ((center[0] + radius - g.min_x) / size).floor();
    let z0 = ((center[2] - radius - g.min_z) / size).floor();
    let z1 = ((center[2] + radius - g.min_z) / size).floor();
    (x0, x1, z0, z1)
}

/// TriangleGrid.nextSerial.
#[inline(always)]
fn next_serial(g: &mut crate::Grid) {
    g.serial = g.serial.wrapping_add(1);
    if g.serial == 0xffff_ffff {
        g.stamp.iter_mut().for_each(|v| *v = 0);
        g.serial = 1;
    }
}

/// Each triangle filed in the cells a sphere's bounds cover, once, in the
/// grid's order, until `visit` (given the grid's index and the triangle's
/// number) returns true, which this then returns.
fn grid_each<V: FnMut(&[u32], usize) -> bool>(g: &mut crate::Grid, center: [f64; 3], radius: f64, mut visit: V) -> bool {
    let (x0, x1, z0, z1) = grid_cells(g, center, radius);
    next_serial(g);
    let serial = g.serial;
    let crate::Grid { start, cells, stamp, index, nx, nz, .. } = g;
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

// The exports for collision.js. A JavaScript gather or overlap runs in two
// calls: the first does the fixed parts and names the bodies to try in
// CANDIDATES; JavaScript writes whether each is enabled in ENABLED; the
// second does those bodies.

static mut GATHERED: Option<Gathered> = None;
static mut CACHES: Vec<GatherCache> = Vec::new();
static mut QUERY: Bounds = EMPTY;
/// A JavaScript gather's polygons, written out: per polygon its id, 1 if it
/// is a moving body's (else 0) and its triangle count; per triangle its set
/// (-1 for a body's) and number, and a body's its nine corners.
static mut OUT: Vec<f64> = Vec::new();
static mut NEAR: Vec<u32> = Vec::new();

fn gathered() -> &'static mut Gathered {
    unsafe { (*addr_of_mut!(GATHERED)).get_or_insert_with(Gathered::new) }
}

fn caches() -> &'static mut Vec<GatherCache> {
    unsafe { &mut *addr_of_mut!(CACHES) }
}

/// A fixed set over tree `tree` of `triangles` triangles, its polygon
/// numbers offset by `offset`; fill its bounds and polygon numbers at
/// `set_bounds` and `set_polygons`.
#[no_mangle]
pub extern "C" fn set_new(tree: u32, triangles: u32, offset: f64) -> u32 {
    let s = state();
    s.sets.push(FixedSet { tree: tree as usize, bounds: vec![0.0; triangles as usize * 6], polygons: vec![0; triangles as usize], offset });
    (s.sets.len() - 1) as u32
}

#[no_mangle]
pub extern "C" fn set_bounds(id: u32) -> *mut f32 { state().sets[id as usize].bounds.as_mut_ptr() }

#[no_mangle]
pub extern "C" fn set_polygons(id: u32) -> *mut u32 { state().sets[id as usize].polygons.as_mut_ptr() }

/// The world's buildings as fixed set `set`.
#[no_mangle]
pub extern "C" fn world_statics(world: u32, set: u32) { state().worlds[world as usize].statics = Some(set as usize); }

/// Room for a tree's `count` polygon numbers (a body's), to fill at the pointer.
#[no_mangle]
pub extern "C" fn tree_polygons(id: u32, count: u32) -> *mut u32 {
    let t = &mut state().trees[id as usize];
    t.polygons.clear();
    t.polygons.resize(count as usize, 0);
    t.polygons.as_mut_ptr()
}

/// A body's polygon number offset.
#[no_mangle]
pub extern "C" fn body_offset(slot: u32, offset: f64) { state().bodies[slot as usize].offset = offset; }

/// A gather cache for JavaScript; returns its id.
#[no_mangle]
pub extern "C" fn cache_new(margin: f64) -> u32 {
    caches().push(GatherCache::new(margin));
    (caches().len() - 1) as u32
}

/// gather's first call, the center in IO 0-2 and the radius in 3: the fixed
/// parts (through cache `cache`, -1 none; with the barriers of set
/// `blockers`, -1 none), and the bodies to try (`ignore` a slot or -1) in
/// CANDIDATES; returns how many.
#[no_mangle]
pub extern "C" fn gather_begin(world: u32, cache: i32, blockers: i32, ignore: i32) -> u32 {
    let v = io();
    let (center, radius) = ([v[0], v[1], v[2]], v[3]);
    let s = state();
    let world = world as usize;
    let blockers = (blockers >= 0).then_some(blockers as usize);
    let out = gathered();
    let count = if cache >= 0 {
        let cache = &mut caches()[cache as usize];
        let bounds = gather_fixed(s, world, center, radius, blockers, Some(&mut *cache), out);
        unsafe { *addr_of_mut!(QUERY) = bounds; }
        let list = cached_bodies(s, world, cache);
        candidates_for(s, world, &bounds, ignore as i64, Some(list))
    } else {
        let bounds = gather_fixed(s, world, center, radius, blockers, None, out);
        unsafe { *addr_of_mut!(QUERY) = bounds; }
        candidates_for(s, world, &bounds, ignore as i64, None)
    };
    count as u32
}

/// gather's second call, over the first `count` candidates whose ENABLED is
/// written: their polygons, and the gather written out (OUT, its address in
/// IO 58 and length in 59); returns the polygon count.
#[no_mangle]
pub extern "C" fn gather_end(world: u32, count: u32) -> u32 {
    let s = state();
    let out = gathered();
    let bounds = unsafe { *addr_of_mut!(QUERY) };
    let order = &s.worlds[world as usize].order;
    for c in 0..count as usize {
        if !enabled(c) { continue; }
        gather_body(s, order[candidate(c)] as usize, &bounds, out);
    }
    let list = out.list();
    let w = unsafe { &mut *addr_of_mut!(OUT) };
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
    let v = io();
    v[58] = w.as_ptr() as usize as f64;
    v[59] = w.len() as f64;
    list.len() as u32
}

/// overlapsSphere's first call, the center in IO 0-2 and the radius in 3:
/// -1 if the buildings or the terrain overlap it, else the bodies to try
/// (`ignore` a slot or -1) in CANDIDATES and how many.
#[no_mangle]
pub extern "C" fn overlap_begin(world: u32, ignore: i32) -> i32 {
    let v = io();
    let (center, radius) = ([v[0], v[1], v[2]], v[3]);
    let s = state();
    if overlaps_fixed(s, world as usize, center, radius) { return -1; }
    candidates_for(s, world as usize, &sphere_bounds(center, radius), ignore as i64, None) as i32
}

/// overlapsSphere's second call over the first `count` candidates whose
/// ENABLED is written: 1 if one overlaps, else 0.
#[no_mangle]
pub extern "C" fn overlap_end(world: u32, count: u32) -> u32 {
    let v = io();
    let (center, radius) = ([v[0], v[1], v[2]], v[3]);
    let s = state();
    let order = &s.worlds[world as usize].order;
    for c in 0..count as usize {
        if enabled(c) && overlaps_body(s, order[candidate(c)] as usize, center, radius) { return 1; }
    }
    0
}

/// terrainNear's triangles (the center in IO 0-2, the radius in 3): their
/// numbers at the address in IO 58; returns how many.
#[no_mangle]
pub extern "C" fn terrain_near_ray(grid: u32) -> u32 {
    let v = io();
    let (center, radius) = ([v[0], v[1], v[2]], v[3]);
    let out = unsafe { &mut *addr_of_mut!(NEAR) };
    terrain_near(state(), grid as usize, center, radius, out);
    v[58] = out.as_ptr() as usize as f64;
    out.len() as u32
}

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
}
