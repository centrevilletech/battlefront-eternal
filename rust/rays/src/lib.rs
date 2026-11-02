//! The collision world's ray casts (src/collision.js) as WebAssembly: the
//! first hit along a ray through a three-mesh-bvh tree, through the terrain's
//! grid of triangles, and through the world's bodies, for `cast`,
//! `castMoving` and `ground`.
//!
//! Every answer must be the JavaScript's own to the last bit, so the world's
//! ticks come out as they do without this module. Each function here repeats
//! the JavaScript it stands for operation by operation, in the same order, in
//! f64: three.js's Ray, Vector3 and Triangle methods, three-mesh-bvh's
//! traversal, and collision.js's grid walk and body loop. Nothing here uses a
//! function whose last bit could differ from V8's (no sin, cos, exp or pow;
//! sqrt and floor are exact in both); the one such value a cast needs,
//! Math.hypot of the ray's direction, comes in from JavaScript. Math.min and
//! Math.max are `js_min` and `js_max` below, which keep JavaScript's rules
//! for NaN and signed zeros. WebAssembly has no fused multiply-add unless
//! asked for, and Rust never contracts f64 operations by itself.
//!
//! Data comes in once, as the trees are built (packed nodes, corners, index
//! and indirect buffers); the bodies' places come as they move, and which of
//! a cast's candidate bodies are enabled before it. Each call reads its ray
//! from `IO` and writes its hit there.

use std::ptr::addr_of_mut;

pub mod spheres;

/// The f64 slots calls exchange: 0-2 ray origin, 3-5 direction, 6 far,
/// 7 Math.hypot(direction.x, direction.z), 8 near; 10 hit distance,
/// 11-13 point, 14-16 normal; 20-57 a body's bounds, matrix and inverse
/// (`body_set`); 0-6 a grid's numbers (`grid_new`).
const IO_LEN: usize = 64;
static mut IO: [f64; IO_LEN] = [0.0; IO_LEN];
/// A cast's candidate bodies (`world_candidates`): their places in the
/// world's order, and whether each is enabled, which JavaScript writes
/// before the cast itself (`world_cast`). Fixed statics, so their addresses
/// never move.
pub const MAX_BODIES: usize = 1 << 16;
pub(crate) static mut CANDIDATES: [u32; MAX_BODIES] = [0; MAX_BODIES];
static mut ENABLED: [u8; MAX_BODIES] = [0; MAX_BODIES];

/// Return codes of the world casts.
pub const MISS: i32 = -2;
pub const TERRAIN: i32 = -3;
pub const STATIC: i32 = -1;

#[derive(Clone, Copy, Debug)]
pub struct Ray {
    pub ox: f64, pub oy: f64, pub oz: f64,
    pub dx: f64, pub dy: f64, pub dz: f64,
}

/// 1 / direction, as three-mesh-bvh's intersectRay and three's
/// intersectBox compute it at every box (the same division, made once).
#[derive(Clone, Copy, Debug)]
struct Inverse { x: f64, y: f64, z: f64 }

#[inline(always)]
fn inverse(ray: &Ray) -> Inverse {
    Inverse { x: 1.0 / ray.dx, y: 1.0 / ray.dy, z: 1.0 / ray.dz }
}

/// A hit: three-mesh-bvh's { distance, point, face.normal }, or the grid's
/// { distance, point, normal }.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub distance: f64,
    pub px: f64, pub py: f64, pub pz: f64,
    pub nx: f64, pub ny: f64, pub nz: f64,
}

/// A tree's or grid's triangle corners, x y z each: f32 when JavaScript
/// holds them in a Float32Array (each read widened to f64, exactly as
/// JavaScript reads them), else f64.
pub enum Corners { F32(Vec<f32>), F64(Vec<f64>) }

pub trait Corner {
    fn corner(&self, i: usize) -> [f64; 3];
}

impl Corner for [f32] {
    #[inline(always)]
    fn corner(&self, i: usize) -> [f64; 3] {
        let o = i * 3;
        [self[o] as f64, self[o + 1] as f64, self[o + 2] as f64]
    }
}

impl Corner for [f64] {
    #[inline(always)]
    fn corner(&self, i: usize) -> [f64; 3] {
        let o = i * 3;
        [self[o], self[o + 1], self[o + 2]]
    }
}

pub struct Tree {
    pub roots: Vec<Vec<u32>>,
    pub positions: usize,
    pub index: Vec<u32>,
    pub indexed: bool,
    pub indirect: Vec<u32>,
    /// Each triangle's polygon number, for a body's tree (`tree_polygons`;
    /// spheres.rs); empty until set.
    pub polygons: Vec<u32>,
}

pub struct Grid {
    pub positions: usize,
    pub index: Vec<u32>,
    pub cells: Vec<u32>,
    pub start: Vec<u32>,
    pub stamp: Vec<u32>,
    pub serial: u32,
    pub min_x: f64, pub min_z: f64, pub max_x: f64, pub max_z: f64,
    pub size: f64, pub nx: f64, pub nz: f64,
}

pub struct Body {
    pub tree: usize,
    pub moving: bool,
    /// What its polygon numbers are offset by (collision.js body.offset).
    pub offset: f64,
    /// Box3: min x, y, z, max x, y, z.
    pub bounds: [f64; 6],
    /// Matrix4.elements (column-major), and its inverse.
    pub matrix: [f64; 16],
    pub inverse: [f64; 16],
}

pub struct World {
    pub tree: usize,
    pub terrain: Option<usize>,
    pub grid: Option<usize>,
    /// The world's bodies in its list's order, by slot.
    pub order: Vec<u32>,
    /// Bumped whenever `order` is set (gather caches keep their bodies until
    /// it changes, as collision.js's do with bodiesVersion).
    pub version: u64,
    /// The buildings as a fixed set for gathers (spheres.rs), once set.
    pub statics: Option<usize>,
}

#[derive(Default)]
pub struct State {
    pub positions: Vec<Corners>,
    pub trees: Vec<Tree>,
    pub grids: Vec<Grid>,
    pub bodies: Vec<Body>,
    pub worlds: Vec<World>,
    /// Fixed sets of triangles for gathers (spheres.rs).
    pub sets: Vec<spheres::FixedSet>,
}

static mut STATE: Option<State> = None;

/// The module's state. One thread: JavaScript calls in one at a time, and
/// a function of this crate that holds it must not call back into an export.
pub fn state() -> &'static mut State {
    // One thread: JavaScript calls in one at a time.
    unsafe { (*addr_of_mut!(STATE)).get_or_insert_with(State::default) }
}

pub(crate) fn io() -> &'static mut [f64; IO_LEN] {
    unsafe { &mut *addr_of_mut!(IO) }
}

pub(crate) fn enabled(c: usize) -> bool {
    unsafe { (*addr_of_mut!(ENABLED))[c] != 0 }
}

pub(crate) fn candidate(c: usize) -> usize {
    unsafe { (*addr_of_mut!(CANDIDATES))[c] as usize }
}

/// Math.min(a, b): NaN if either is, and -0 below +0.
#[inline(always)]
pub fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { return f64::NAN; }
    if a < b { a } else if b < a { b } else if a == 0.0 && a.is_sign_negative() { a } else { b }
}

/// Math.max(a, b): NaN if either is, and +0 above -0.
#[inline(always)]
pub fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { return f64::NAN; }
    if a > b { a } else if b > a { b } else if a == 0.0 && a.is_sign_positive() { a } else { b }
}

/// Vector3.dot: x * x' + y * y' + z * z', left to right.
#[inline(always)]
pub fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Vector3.crossVectors(a, b).
#[inline(always)]
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

#[inline(always)]
pub fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Ray.intersectTriangle(a, b, c, backfaceCulling, target): the point, or None.
#[inline(always)]
pub fn intersect_triangle(ray: &Ray, a: [f64; 3], b: [f64; 3], c: [f64; 3], backface_culling: bool) -> Option<[f64; 3]> {
    let d = [ray.dx, ray.dy, ray.dz];
    let edge1 = sub(b, a);
    let edge2 = sub(c, a);
    let normal = cross(edge1, edge2);
    let mut ddn = dot(d, normal);
    let sign;
    if ddn > 0.0 {
        if backface_culling { return None; }
        sign = 1.0;
    } else if ddn < 0.0 {
        sign = -1.0;
        ddn = -ddn;
    } else {
        return None;
    }
    let diff = sub([ray.ox, ray.oy, ray.oz], a);
    let ddqxe2 = sign * dot(d, cross(diff, edge2));
    if ddqxe2 < 0.0 { return None; }
    let dde1xq = sign * dot(d, cross(edge1, diff));
    if dde1xq < 0.0 { return None; }
    if ddqxe2 + dde1xq > ddn { return None; }
    let qdn = -sign * dot(diff, normal);
    if qdn < 0.0 { return None; }
    Some(at(ray, qdn / ddn))
}

/// Ray.at(t): origin + direction * t, as addScaledVector does it.
#[inline(always)]
fn at(ray: &Ray, t: f64) -> [f64; 3] {
    [ray.ox + ray.dx * t, ray.oy + ray.dy * t, ray.oz + ray.dz * t]
}

/// Vector3.distanceTo.
#[inline(always)]
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Triangle.getNormal(a, b, c, target).
#[inline(always)]
pub fn triangle_normal(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    let t = cross(sub(c, b), sub(a, b));
    let length_sq = t[0] * t[0] + t[1] * t[1] + t[2] * t[2];
    if length_sq > 0.0 {
        let s = 1.0 / length_sq.sqrt();
        return [t[0] * s, t[1] * s, t[2] * s];
    }
    [0.0, 0.0, 0.0]
}

#[inline(always)]
pub(crate) fn f32_at(words: &[u32], i: usize) -> f64 {
    f32::from_bits(words[i]) as f64
}

/// three-mesh-bvh's intersectRay: whether the ray meets a node's bounds
/// between near and far.
#[inline(always)]
fn intersect_node(words: &[u32], node: usize, ray: &Ray, inv: &Inverse, near: f64, far: f64) -> bool {
    let (mut tmin, mut tmax);
    let (invdirx, invdiry, invdirz) = (inv.x, inv.y, inv.z);
    let (ox, oy, oz) = (ray.ox, ray.oy, ray.oz);
    let minx = f32_at(words, node);
    let maxx = f32_at(words, node + 3);
    let miny = f32_at(words, node + 1);
    let maxy = f32_at(words, node + 4);
    let minz = f32_at(words, node + 2);
    let maxz = f32_at(words, node + 5);
    if invdirx >= 0.0 {
        tmin = (minx - ox) * invdirx;
        tmax = (maxx - ox) * invdirx;
    } else {
        tmin = (maxx - ox) * invdirx;
        tmax = (minx - ox) * invdirx;
    }
    let (tymin, tymax);
    if invdiry >= 0.0 {
        tymin = (miny - oy) * invdiry;
        tymax = (maxy - oy) * invdiry;
    } else {
        tymin = (maxy - oy) * invdiry;
        tymax = (miny - oy) * invdiry;
    }
    if (tmin > tymax) || (tymin > tmax) { return false; }
    if tymin > tmin || tmin.is_nan() { tmin = tymin; }
    if tymax < tmax || tmax.is_nan() { tmax = tymax; }
    let (tzmin, tzmax);
    if invdirz >= 0.0 {
        tzmin = (minz - oz) * invdirz;
        tzmax = (maxz - oz) * invdirz;
    } else {
        tzmin = (maxz - oz) * invdirz;
        tzmax = (minz - oz) * invdirz;
    }
    if (tmin > tzmax) || (tzmin > tmax) { return false; }
    if tzmin > tmin || tmin != tmin { tmin = tzmin; }
    if tzmax < tmax || tmax != tmax { tmax = tzmax; }
    tmin <= far && tmax >= near
}

/// three's Ray.intersectsBox(box): Ray.intersectBox(box) !== null.
#[inline(always)]
fn intersects_box(ray: &Ray, inv: &Inverse, b: &[f64; 6]) -> bool {
    let (mut tmin, mut tmax);
    let (invdirx, invdiry, invdirz) = (inv.x, inv.y, inv.z);
    if invdirx >= 0.0 {
        tmin = (b[0] - ray.ox) * invdirx;
        tmax = (b[3] - ray.ox) * invdirx;
    } else {
        tmin = (b[3] - ray.ox) * invdirx;
        tmax = (b[0] - ray.ox) * invdirx;
    }
    let (tymin, tymax);
    if invdiry >= 0.0 {
        tymin = (b[1] - ray.oy) * invdiry;
        tymax = (b[4] - ray.oy) * invdiry;
    } else {
        tymin = (b[4] - ray.oy) * invdiry;
        tymax = (b[1] - ray.oy) * invdiry;
    }
    if (tmin > tymax) || (tymin > tmax) { return false; }
    if tymin > tmin || tmin.is_nan() { tmin = tymin; }
    if tymax < tmax || tmax.is_nan() { tmax = tymax; }
    let (tzmin, tzmax);
    if invdirz >= 0.0 {
        tzmin = (b[2] - ray.oz) * invdirz;
        tzmax = (b[5] - ray.oz) * invdirz;
    } else {
        tzmin = (b[5] - ray.oz) * invdirz;
        tzmax = (b[2] - ray.oz) * invdirz;
    }
    if (tmin > tzmax) || (tzmin > tmax) { return false; }
    if tzmin > tmin || tmin != tmin { tmin = tzmin; }
    if tzmax < tmax || tmax != tmax { tmax = tzmax; }
    let _ = tmin;
    !(tmax < 0.0)
}

/// Vector3.applyMatrix4(m).
#[inline(always)]
pub fn apply_matrix4(e: &[f64; 16], v: [f64; 3]) -> [f64; 3] {
    let (x, y, z) = (v[0], v[1], v[2]);
    let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
    [
        (e[0] * x + e[4] * y + e[8] * z + e[12]) * w,
        (e[1] * x + e[5] * y + e[9] * z + e[13]) * w,
        (e[2] * x + e[6] * y + e[10] * z + e[14]) * w,
    ]
}

/// Vector3.transformDirection(m): the rotated direction, normalized
/// (divideScalar(length() || 1), a multiplication by the reciprocal).
#[inline(always)]
fn transform_direction(e: &[f64; 16], v: [f64; 3]) -> [f64; 3] {
    let (x, y, z) = (v[0], v[1], v[2]);
    let t = [e[0] * x + e[4] * y + e[8] * z, e[1] * x + e[5] * y + e[9] * z, e[2] * x + e[6] * y + e[10] * z];
    let length = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
    let s = 1.0 / if length == 0.0 || length.is_nan() { 1.0 } else { length };
    [t[0] * s, t[1] * s, t[2] * s]
}

/// three-mesh-bvh's intersectTri (via checkBufferGeometryIntersection), for
/// a double-sided cast: the hit with its face normal, or None.
#[inline(always)]
fn triangle_hit<P: Corner + ?Sized>(tree: &Tree, p: &P, ray: &Ray, tri: usize, near: f64, far: f64) -> Option<Hit> {
    let o = tri * 3;
    let (a, b, c) = if tree.indexed {
        (tree.index[o] as usize, tree.index[o + 1] as usize, tree.index[o + 2] as usize)
    } else {
        (o, o + 1, o + 2)
    };
    let va = p.corner(a);
    let vb = p.corner(b);
    let vc = p.corner(c);
    let point = intersect_triangle(ray, va, vb, vc, false)?;
    let d = distance([ray.ox, ray.oy, ray.oz], point);
    if d < near || d > far { return None; }
    let n = triangle_normal(va, vb, vc);
    Some(Hit { distance: d, px: point[0], py: point[1], pz: point[2], nx: n[0], ny: n[1], nz: n[2] })
}

/// three-mesh-bvh's _raycastFirst (indirect) from one node.
fn raycast_node<P: Corner + ?Sized>(tree: &Tree, p: &P, words: &[u32], node: usize, ray: &Ray, inv: &Inverse, near: f64, far: f64) -> Option<Hit> {
    let w7 = words[node + 7];
    if (w7 >> 16) == 0xffff {
        // intersectClosestTri_indirect
        let offset = words[node + 6] as usize;
        let count = (w7 & 0xffff) as usize;
        let mut dist = f64::INFINITY;
        let mut res = None;
        for i in offset..offset + count {
            if let Some(hit) = triangle_hit(tree, p, ray, tree.indirect[i] as usize, near, far) {
                if hit.distance < dist {
                    dist = hit.distance;
                    res = Some(hit);
                }
            }
        }
        return res;
    }
    let split = w7 as usize;
    let ray_dir = match split { 0 => ray.dx, 1 => ray.dy, 2 => ray.dz, _ => f64::NAN };
    let left_to_right = ray_dir >= 0.0;
    let (c1, c2) = if left_to_right { (node + 8, words[node + 6] as usize) } else { (words[node + 6] as usize, node + 8) };
    let c1_result = if intersect_node(words, c1, ray, inv, near, far) { raycast_node(tree, p, words, c1, ray, inv, near, far) } else { None };
    if let Some(hit) = c1_result {
        if split < 3 {
            let point = [hit.px, hit.py, hit.pz][split];
            let outside = if left_to_right { point <= f32_at(words, c2 + split) } else { point >= f32_at(words, c2 + split + 3) };
            if outside { return c1_result; }
        }
    }
    let c2_result = if intersect_node(words, c2, ray, inv, near, far) { raycast_node(tree, p, words, c2, ray, inv, near, far) } else { None };
    match (c1_result, c2_result) {
        (Some(a), Some(b)) => Some(if a.distance <= b.distance { a } else { b }),
        (a, b) => a.or(b),
    }
}

/// MeshBVH.raycastFirst(ray, DoubleSide, near, far).
pub fn tree_raycast(s: &State, tree: usize, ray: &Ray, near: f64, far: f64) -> Option<Hit> {
    let t = &s.trees[tree];
    match &s.positions[t.positions] {
        Corners::F32(p) => roots_raycast(t, p.as_slice(), ray, near, far),
        Corners::F64(p) => roots_raycast(t, p.as_slice(), ray, near, far),
    }
}

fn roots_raycast<P: Corner + ?Sized>(t: &Tree, p: &P, ray: &Ray, near: f64, far: f64) -> Option<Hit> {
    let mut closest: Option<Hit> = None;
    let inv = inverse(ray);
    for words in &t.roots {
        if let Some(hit) = raycast_node(t, p, words, 0, ray, &inv, near, far) {
            if closest.map_or(true, |c| hit.distance < c.distance) { closest = Some(hit); }
        }
    }
    closest
}

/// TriangleGrid.testCell.
#[inline(always)]
fn grid_cell<P: Corner + ?Sized>(g: &mut Grid, p: &P, ray: &Ray, cell: usize, far: f64, mut best: f64, hit: &mut Hit) -> f64 {
    let serial = g.serial;
    let (begin, end) = (g.start[cell] as usize, g.start[cell + 1] as usize);
    for i in begin..end {
        let t = g.cells[i] as usize;
        if g.stamp[t] == serial { continue; }
        g.stamp[t] = serial;
        let o = t * 3;
        let a = p.corner(g.index[o] as usize);
        let b = p.corner(g.index[o + 1] as usize);
        let c = p.corner(g.index[o + 2] as usize);
        let Some(point) = intersect_triangle(ray, a, b, c, false) else { continue };
        let d = distance([ray.ox, ray.oy, ray.oz], point);
        if d > far || !(d < best) { continue; }
        best = d;
        let n = triangle_normal(a, b, c);
        *hit = Hit { distance: d, px: point[0], py: point[1], pz: point[2], nx: n[0], ny: n[1], nz: n[2] };
    }
    best
}

/// TriangleGrid.raycastFirst(ray, far).
pub fn grid_raycast(s: &mut State, grid: usize, ray: &Ray, far: f64) -> Option<Hit> {
    let State { grids, positions, .. } = s;
    let g = &mut grids[grid];
    match &positions[g.positions] {
        Corners::F32(p) => grid_walk(g, p.as_slice(), ray, far),
        Corners::F64(p) => grid_walk(g, p.as_slice(), ray, far),
    }
}

fn grid_walk<P: Corner + ?Sized>(g: &mut Grid, p: &P, ray: &Ray, far: f64) -> Option<Hit> {
    let (o, d, size) = ([ray.ox, ray.oy, ray.oz], [ray.dx, ray.dy, ray.dz], g.size);
    let mut t0 = 0.0;
    let mut t1 = f64::INFINITY;
    if d[0].abs() < 1e-12 {
        if o[0] < g.min_x || o[0] >= g.max_x { return None; }
    } else {
        let a = (g.min_x - o[0]) / d[0];
        let b = (g.max_x - o[0]) / d[0];
        t0 = js_max(t0, js_min(a, b));
        t1 = js_min(t1, js_max(a, b));
    }
    if d[2].abs() < 1e-12 {
        if o[2] < g.min_z || o[2] >= g.max_z { return None; }
    } else {
        let a = (g.min_z - o[2]) / d[2];
        let b = (g.max_z - o[2]) / d[2];
        t0 = js_max(t0, js_min(a, b));
        t1 = js_min(t1, js_max(a, b));
    }
    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if length == 0.0 { return None; }
    t1 = js_min(t1, far / length);
    if !(t0 <= t1) { return None; }
    // nextSerial
    g.serial = g.serial.wrapping_add(1);
    if g.serial == 0xffff_ffff {
        g.stamp.iter_mut().for_each(|v| *v = 0);
        g.serial = 1;
    }
    let mut x = js_min(g.nx - 1.0, js_max(0.0, ((o[0] + d[0] * t0 - g.min_x) / size).floor()));
    let mut z = js_min(g.nz - 1.0, js_max(0.0, ((o[2] + d[2] * t0 - g.min_z) / size).floor()));
    // A ray JavaScript would walk forever (NaN cells): never cast; no hit.
    if x.is_nan() || z.is_nan() { return None; }
    let step_x = if d[0] > 0.0 { 1.0 } else { -1.0 };
    let step_z = if d[2] > 0.0 { 1.0 } else { -1.0 };
    let mut next_x = if d[0].abs() < 1e-12 { f64::INFINITY } else { (g.min_x + (x + if step_x > 0.0 { 1.0 } else { 0.0 }) * size - o[0]) / d[0] };
    let mut next_z = if d[2].abs() < 1e-12 { f64::INFINITY } else { (g.min_z + (z + if step_z > 0.0 { 1.0 } else { 0.0 }) * size - o[2]) / d[2] };
    let delta_x = if d[0].abs() < 1e-12 { f64::INFINITY } else { size / d[0].abs() };
    let delta_z = if d[2].abs() < 1e-12 { f64::INFINITY } else { size / d[2].abs() };
    let mut best = f64::INFINITY;
    let mut hit = Hit { distance: 0.0, px: 0.0, py: 0.0, pz: 0.0, nx: 0.0, ny: 0.0, nz: 0.0 };
    loop {
        best = grid_cell(g, p, ray, (z * g.nx + x) as usize, far, best, &mut hit);
        let exit = js_min(next_x, next_z);
        if best <= exit * length || exit >= t1 { break; }
        if next_x < next_z {
            x += step_x;
            next_x += delta_x;
            if x < 0.0 || x >= g.nx { break; }
        } else {
            z += step_z;
            next_z += delta_z;
            if z < 0.0 || z >= g.nz { break; }
        }
    }
    if best < f64::INFINITY { Some(hit) } else { None }
}

/// CollisionWorld.castTerrain(ray, far): the grid for rays crossing few of
/// its cells, the terrain's tree for long ones.
fn cast_terrain(s: &mut State, world: usize, ray: &Ray, far: f64, hypot_xz: f64) -> Option<Hit> {
    let (grid, terrain) = (s.worlds[world].grid?, s.worlds[world].terrain);
    let length = (ray.dx * ray.dx + ray.dy * ray.dy + ray.dz * ray.dz).sqrt();
    if far * hypot_xz / length <= s.grids[grid].size * 12.0 { return grid_raycast(s, grid, ray, far); }
    tree_raycast(s, terrain?, ray, 0.0, far)
}

/// Ray.copy(ray).applyMatrix4(m).
#[inline(always)]
fn transform_ray(ray: &Ray, e: &[f64; 16]) -> Ray {
    let o = apply_matrix4(e, [ray.ox, ray.oy, ray.oz]);
    let d = transform_direction(e, [ray.dx, ray.dy, ray.dz]);
    Ray { ox: o[0], oy: o[1], oz: o[2], dx: d[0], dy: d[1], dz: d[2] }
}

/// The world's bodies a cast must try, in its order: those not ignored, not
/// left out (a moving one in a static-only cast; a fixed one in castMoving's)
/// and whose bounds the ray meets. Each one's place in the order goes in
/// CANDIDATES; JavaScript then says which are enabled. Every test of the
/// JavaScript's body loop reads and changes nothing, so making them in two
/// passes gives the same bodies in the same order.
pub fn world_candidates_for(s: &State, world: usize, ray: &Ray, ignore: i64, static_only: bool, moving_only: bool) -> usize {
    let w = &s.worlds[world];
    let inv = inverse(ray);
    let out = unsafe { &mut *addr_of_mut!(CANDIDATES) };
    let mut count = 0;
    for (i, &slot) in w.order.iter().enumerate() {
        let body = &s.bodies[slot as usize];
        if (moving_only && !body.moving) || slot as i64 == ignore || (static_only && body.moving) || !intersects_box(ray, &inv, &body.bounds) { continue; }
        out[count] = i as u32;
        count += 1;
    }
    count
}

/// CollisionWorld.cast over the first `count` candidates: the code (MISS,
/// TERRAIN, STATIC or the body's slot) and the hit, its normal facing back
/// along the ray.
pub fn world_cast_ray(s: &mut State, world: usize, ray: &Ray, far: f64, hypot_xz: f64, count: usize) -> (i32, Hit) {
    let w = &s.worlds[world];
    let mut hit = tree_raycast(s, w.tree, ray, 0.0, far);
    let mut code = STATIC;
    for c in 0..count {
        if !enabled(c) { continue; }
        let slot = w.order[candidate(c)];
        let body = &s.bodies[slot as usize];
        let local = if body.moving { transform_ray(ray, &body.inverse) } else { *ray };
        let mut near = tree_raycast(s, body.tree, &local, 0.0, hit.map_or(far, |h| h.distance));
        if let (Some(h), true) = (near.as_mut(), body.moving) {
            let p = apply_matrix4(&body.matrix, [h.px, h.py, h.pz]);
            let n = transform_direction(&body.matrix, [h.nx, h.ny, h.nz]);
            (h.px, h.py, h.pz, h.nx, h.ny, h.nz) = (p[0], p[1], p[2], n[0], n[1], n[2]);
        }
        if let Some(n) = near {
            if hit.map_or(true, |h| n.distance < h.distance) {
                hit = Some(n);
                code = slot as i32;
            }
        }
    }
    let low = cast_terrain(s, world, ray, hit.map_or(far, |h| h.distance), hypot_xz);
    let terrain = matches!((low, hit), (Some(l), Some(h)) if l.distance < h.distance) || (low.is_some() && hit.is_none());
    let mut out = if terrain {
        code = TERRAIN;
        low.unwrap()
    } else {
        match hit { Some(h) => h, None => return (MISS, Hit { distance: 0.0, px: 0.0, py: 0.0, pz: 0.0, nx: 0.0, ny: 0.0, nz: 0.0 }) }
    };
    if dot([out.nx, out.ny, out.nz], [ray.dx, ray.dy, ray.dz]) > 0.0 {
        out.nx = -out.nx;
        out.ny = -out.ny;
        out.nz = -out.nz;
    }
    (code, out)
}

/// CollisionWorld.castMoving over the first `count` candidates (moving
/// bodies only): the nearest one's hit (MISS or the body's slot), its normal
/// facing back along the ray.
pub fn world_cast_moving_ray(s: &State, world: usize, ray: &Ray, far: f64, count: usize) -> (i32, Hit) {
    let w = &s.worlds[world];
    let mut closest = far;
    let mut code = MISS;
    let mut out = Hit { distance: 0.0, px: 0.0, py: 0.0, pz: 0.0, nx: 0.0, ny: 0.0, nz: 0.0 };
    for c in 0..count {
        if !enabled(c) { continue; }
        let slot = w.order[candidate(c)];
        let body = &s.bodies[slot as usize];
        let local = transform_ray(ray, &body.inverse);
        let Some(hit) = tree_raycast(s, body.tree, &local, 0.0, closest) else { continue };
        closest = hit.distance;
        let p = apply_matrix4(&body.matrix, [hit.px, hit.py, hit.pz]);
        let mut n = transform_direction(&body.matrix, [hit.nx, hit.ny, hit.nz]);
        if dot(n, [ray.dx, ray.dy, ray.dz]) > 0.0 { n = [-n[0], -n[1], -n[2]]; }
        out = Hit { distance: closest, px: p[0], py: p[1], pz: p[2], nx: n[0], ny: n[1], nz: n[2] };
        code = slot as i32;
    }
    (code, out)
}

fn read_ray() -> Ray {
    let v = io();
    Ray { ox: v[0], oy: v[1], oz: v[2], dx: v[3], dy: v[4], dz: v[5] }
}

fn write_hit(h: &Hit) {
    let v = io();
    v[10] = h.distance;
    v[11] = h.px; v[12] = h.py; v[13] = h.pz;
    v[14] = h.nx; v[15] = h.ny; v[16] = h.nz;
}

// The module's exports, for src/ray-kernel.js.

#[no_mangle]
pub extern "C" fn io_ptr() -> *mut f64 { addr_of_mut!(IO) as *mut f64 }

#[no_mangle]
pub extern "C" fn enabled_ptr() -> *mut u8 { addr_of_mut!(ENABLED) as *mut u8 }

#[no_mangle]
pub extern "C" fn candidates_ptr() -> *mut u32 { addr_of_mut!(CANDIDATES) as *mut u32 }

/// Room for `count` corner coordinates, f32 (`wide` 0) or f64; returns its
/// id (`positions_at` for where).
#[no_mangle]
pub extern "C" fn positions_new(count: u32, wide: u32) -> u32 {
    let s = state();
    s.positions.push(if wide != 0 { Corners::F64(vec![0.0; count as usize]) } else { Corners::F32(vec![0.0; count as usize]) });
    (s.positions.len() - 1) as u32
}

#[no_mangle]
pub extern "C" fn positions_at(id: u32) -> *mut u8 {
    match &mut state().positions[id as usize] {
        Corners::F32(v) => v.as_mut_ptr() as *mut u8,
        Corners::F64(v) => v.as_mut_ptr() as *mut u8,
    }
}

/// A tree over positions `positions`: `indices` u32s of index (0 for none) and
/// `indirect` of indirect buffer; fill them at `tree_index` and
/// `tree_indirect`, and add each root with `tree_root`.
#[no_mangle]
pub extern "C" fn tree_new(positions: u32, indices: u32, indirect: u32) -> u32 {
    let s = state();
    s.trees.push(Tree { roots: Vec::new(), positions: positions as usize, index: vec![0; indices as usize], indexed: indices > 0, indirect: vec![0; indirect as usize], polygons: Vec::new() });
    (s.trees.len() - 1) as u32
}

#[no_mangle]
pub extern "C" fn tree_index(id: u32) -> *mut u32 { state().trees[id as usize].index.as_mut_ptr() }

#[no_mangle]
pub extern "C" fn tree_indirect(id: u32) -> *mut u32 { state().trees[id as usize].indirect.as_mut_ptr() }

/// A root's packed node buffer of `words` u32s, to fill at the pointer.
#[no_mangle]
pub extern "C" fn tree_root(id: u32, words: u32) -> *mut u32 {
    let t = &mut state().trees[id as usize];
    t.roots.push(vec![0; words as usize]);
    t.roots.last_mut().unwrap().as_mut_ptr()
}

/// A terrain grid over positions `positions`, with `indices` u32s of index,
/// `cells` of cells, `start` of cell starts and `triangles` stamps, its
/// numbers in IO 0-6 (minX, minZ, maxX, maxZ, size, nx, nz).
#[no_mangle]
pub extern "C" fn grid_new(positions: u32, indices: u32, cells: u32, start: u32, triangles: u32) -> u32 {
    let v = *io();
    let s = state();
    s.grids.push(Grid { positions: positions as usize, index: vec![0; indices as usize], cells: vec![0; cells as usize], start: vec![0; start as usize],
        stamp: vec![0; triangles as usize], serial: 0, min_x: v[0], min_z: v[1], max_x: v[2], max_z: v[3], size: v[4], nx: v[5], nz: v[6] });
    (s.grids.len() - 1) as u32
}

#[no_mangle]
pub extern "C" fn grid_index(id: u32) -> *mut u32 { state().grids[id as usize].index.as_mut_ptr() }

#[no_mangle]
pub extern "C" fn grid_cells(id: u32) -> *mut u32 { state().grids[id as usize].cells.as_mut_ptr() }

#[no_mangle]
pub extern "C" fn grid_start(id: u32) -> *mut u32 { state().grids[id as usize].start.as_mut_ptr() }

/// A body over tree `tree`; returns its slot, or -1 when there is no room.
#[no_mangle]
pub extern "C" fn body_new(tree: u32, moving: u32) -> i32 {
    let s = state();
    if s.bodies.len() >= MAX_BODIES { return -1; }
    s.bodies.push(Body { tree: tree as usize, moving: moving != 0, offset: 0.0, bounds: [0.0; 6], matrix: [0.0; 16], inverse: [0.0; 16] });
    (s.bodies.len() - 1) as i32
}

/// A body's bounds from IO 20-25, and a moving one's matrix and inverse
/// from IO 26-41 and 42-57.
#[no_mangle]
pub extern "C" fn body_set(slot: u32) {
    let v = io();
    let b = &mut state().bodies[slot as usize];
    b.bounds.copy_from_slice(&v[20..26]);
    if b.moving {
        b.matrix.copy_from_slice(&v[26..42]);
        b.inverse.copy_from_slice(&v[42..58]);
    }
}

/// A world over the buildings' tree, and the terrain's tree and grid (-1 for none).
#[no_mangle]
pub extern "C" fn world_new(tree: u32, terrain: i32, grid: i32) -> u32 {
    let s = state();
    s.worlds.push(World { tree: tree as usize, terrain: (terrain >= 0).then_some(terrain as usize), grid: (grid >= 0).then_some(grid as usize), order: Vec::new(), version: 0, statics: None });
    (s.worlds.len() - 1) as u32
}

/// Room for the world's `count` bodies' slots, in order, to fill at the pointer.
#[no_mangle]
pub extern "C" fn world_order(id: u32, count: u32) -> *mut u32 {
    let w = &mut state().worlds[id as usize];
    w.order.clear();
    w.order.resize(count as usize, 0);
    w.version += 1;
    w.order.as_mut_ptr()
}

/// A cast's candidate bodies (the ray in IO), in CANDIDATES; returns how
/// many. `ignore` is the ignored body's slot (-1 none).
#[no_mangle]
pub extern "C" fn world_candidates(id: u32, ignore: i32, static_only: u32, moving_only: u32) -> u32 {
    world_candidates_for(state(), id as usize, &read_ray(), ignore as i64, static_only != 0, moving_only != 0) as u32
}

/// cast(origin, direction, far, out, ignoreBody, staticOnly) with the ray,
/// far and hypot in IO, over `count` candidates whose ENABLED is written:
/// MISS, TERRAIN, STATIC or the body's slot, the hit in IO.
#[no_mangle]
pub extern "C" fn world_cast(id: u32, count: u32) -> i32 {
    let ray = read_ray();
    let (far, hypot) = (io()[6], io()[7]);
    let (code, hit) = world_cast_ray(state(), id as usize, &ray, far, hypot, count as usize);
    if code != MISS { write_hit(&hit); }
    code
}

/// castMoving(origin, direction, far, out, ignoreBody) over `count` moving
/// candidates: MISS or the body's slot.
#[no_mangle]
pub extern "C" fn world_cast_moving(id: u32, count: u32) -> i32 {
    let ray = read_ray();
    let (code, hit) = world_cast_moving_ray(state(), id as usize, &ray, io()[6], count as usize);
    if code != MISS { write_hit(&hit); }
    code
}

/// TriangleGrid.raycastFirst(ray, far) with the ray and far in IO: 1 and the
/// hit in IO, or 0.
#[no_mangle]
pub extern "C" fn grid_ray(id: u32) -> u32 {
    let ray = read_ray();
    match grid_raycast(state(), id as usize, &ray, io()[6]) {
        Some(hit) => { write_hit(&hit); 1 }
        None => 0,
    }
}

/// MeshBVH.raycastFirst(ray, DoubleSide, near, far) on one tree, near in IO 8.
#[no_mangle]
pub extern "C" fn tree_ray(id: u32) -> u32 {
    let ray = read_ray();
    let v = io();
    match tree_raycast(state(), id as usize, &ray, v[8], v[6]) {
        Some(hit) => { write_hit(&hit); 1 }
        None => 0,
    }
}

/// Nothing: what a call into the module costs by itself.
#[no_mangle]
pub extern "C" fn nothing() -> u32 { 0 }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_max_keep_javascript_rules() {
        assert!(js_min(f64::NAN, 1.0).is_nan() && js_max(1.0, f64::NAN).is_nan());
        assert!(js_min(0.0, -0.0).is_sign_negative() && js_min(-0.0, 0.0).is_sign_negative());
        assert!(js_max(0.0, -0.0).is_sign_positive() && js_max(-0.0, 0.0).is_sign_positive());
        assert_eq!(js_min(1.0, 2.0), 1.0);
        assert_eq!(js_max(1.0, 2.0), 2.0);
    }

    #[test]
    fn ray_meets_a_triangle_from_either_side() {
        let (a, b, c) = ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let down = Ray { ox: 0.25, oy: 1.0, oz: 0.25, dx: 0.0, dy: -1.0, dz: 0.0 };
        let up = Ray { ox: 0.25, oy: -1.0, oz: 0.25, dx: 0.0, dy: 1.0, dz: 0.0 };
        assert_eq!(intersect_triangle(&down, a, b, c, false), Some([0.25, 0.0, 0.25]));
        assert_eq!(intersect_triangle(&up, a, b, c, false), Some([0.25, 0.0, 0.25]));
        assert_eq!(triangle_normal(a, b, c), [0.0, -1.0, 0.0]);
        let miss = Ray { ox: 2.0, oy: 1.0, oz: 2.0, dx: 0.0, dy: -1.0, dz: 0.0 };
        assert_eq!(intersect_triangle(&miss, a, b, c, false), None);
    }
}
