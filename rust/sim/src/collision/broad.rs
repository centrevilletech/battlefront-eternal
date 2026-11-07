//! A broad phase for the bodies a cast tries (lib.rs world_candidates_for),
//! which keeps their order.
//!
//! A cast tries, in the world's order, every body that is not left out and
//! whose bounds its ray meets; the ray's test ignores how far the cast
//! reaches, so every body is tested for every cast (96 of them on Bespin
//! Platforms, where four casts in five reach less than 5 m). Here the fixed
//! bodies are filed in a grid over x and z, and a short cast takes only
//! those filed where its segment can reach, then gives each of them and
//! every moving body the same tests as before, in the world's order. A
//! fixed body left out this way lies wholly clear of the segment, by far
//! more than any rounding: none of its triangles can be hit within the
//! cast's reach, so it could never have changed the cast's answer. Moving
//! bodies are cast against in their own frame (their own rounding of
//! distance), so they all keep the ray's test. Long casts, and casts with no
//! finite reach, test every body as before.

// The grid's own arithmetic (its bounds and cells, never NaN and never
// seen by a cast's answer) uses the host's min and max (clippy.toml).
#![allow(clippy::disallowed_methods)]

use crate::collision::{intersects_box, inverse, js_max, js_min, Body, Ray, State, World};

/// How far a fixed body's bounds and a cast's segment are grown before they
/// are compared (and what a segment's reach is grown by, relatively): far
/// beyond the rounding of a hit's distance or point.
const MARGIN: f64 = 1e-3;
const RELATIVE: f64 = 1e-9;
/// The most cells a segment's bounds may cover for the grid to be used.
const MAX_CELLS: usize = 16;
/// The grid's cells: no smaller than this, and no more than GRID across.
const MIN_CELL: f64 = 4.0;
const GRID: f64 = 64.0;

/// A world's fixed bodies by grid cell, and its moving ones, as places in
/// the world's order; made again when the order or a fixed body's bounds
/// change.
#[derive(Default)]
pub struct Broad {
    made_for: Option<(u64, u64)>,
    min_x: f64,
    min_z: f64,
    size: f64,
    nx: usize,
    nz: usize,
    start: Vec<u32>,
    cells: Vec<u32>,
    /// Places of the moving bodies, and of fixed ones with bounds the grid
    /// cannot file (not finite): tried by every cast, as bit sets.
    moving: Vec<u64>,
    unfiled: Vec<u64>,
    bits: Vec<u64>,
    // Rebuilding after membership or fixed geometry changes reuses its index
    // storage. The first cast of an owned phase must not allocate this scratch.
    ranges: Vec<Option<(usize, usize, usize, usize)>>,
    fill: Vec<u32>,
}

#[inline(always)]
fn finite(b: &[f64; 6]) -> bool {
    b.iter().all(|v| v.is_finite()) && b[0] <= b[3] && b[1] <= b[4] && b[2] <= b[5]
}

impl Broad {
    /// Moving hull births leave the fixed grid unchanged. Size their place
    /// bits and range rows cold, before the first cast can meet the new hull.
    pub fn reserve_bodies(&mut self, count: usize) {
        let words = count.div_ceil(64);
        self.moving.reserve(words.saturating_sub(self.moving.len()));
        self.unfiled.reserve(words.saturating_sub(self.unfiled.len()));
        self.bits.reserve(words.saturating_sub(self.bits.len()));
        self.ranges.reserve(count.saturating_sub(self.ranges.len()));
    }
    fn make(&mut self, bodies: &[Body], w: &World) {
        let n = w.order.len();
        let words = n.div_ceil(64);
        self.moving.resize(words,0); self.moving.fill(0);
        self.unfiled.resize(words,0); self.unfiled.fill(0);
        self.bits.resize(words,0); self.bits.fill(0);
        let (mut min_x, mut min_z, mut max_x, mut max_z) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (place, &slot) in w.order.iter().enumerate() {
            let body = &bodies[slot as usize];
            if body.moving { self.moving[place / 64] |= 1 << (place % 64); continue; }
            if !finite(&body.bounds) { self.unfiled[place / 64] |= 1 << (place % 64); continue; }
            min_x = min_x.min(body.bounds[0]);
            min_z = min_z.min(body.bounds[2]);
            max_x = max_x.max(body.bounds[3]);
            max_z = max_z.max(body.bounds[5]);
        }
        if !(min_x <= max_x) { min_x = 0.0; min_z = 0.0; max_x = 0.0; max_z = 0.0; }
        self.min_x = min_x - MARGIN;
        self.min_z = min_z - MARGIN;
        self.size = MIN_CELL.max((max_x - min_x).max(max_z - min_z) / GRID);
        self.nx = (((max_x + MARGIN - self.min_x) / self.size).floor() as usize + 1).max(1);
        self.nz = (((max_z + MARGIN - self.min_z) / self.size).floor() as usize + 1).max(1);
        let cells = self.nx * self.nz;
        self.start.resize(cells + 1,0); self.start.fill(0);
        self.ranges.clear();
        for place in 0..n {
            let body = &bodies[w.order[place] as usize];
            let range = if body.moving || !finite(&body.bounds) { None } else {
                let b = &body.bounds;
                Some((self.cell_x(b[0] - MARGIN), self.cell_x(b[3] + MARGIN), self.cell_z(b[2] - MARGIN), self.cell_z(b[5] + MARGIN)))
            };
            self.ranges.push(range);
        }
        for &(x0, x1, z0, z1) in self.ranges.iter().flatten() {
            for z in z0..=z1 { for x in x0..=x1 { self.start[z * self.nx + x + 1] += 1; } }
        }
        for c in 0..cells { self.start[c + 1] += self.start[c]; }
        self.fill.clear(); self.fill.extend_from_slice(&self.start[..cells]);
        self.cells.resize(self.start[cells] as usize,0);
        for (place, range) in self.ranges.iter().enumerate() {
            let Some((x0, x1, z0, z1)) = *range else { continue };
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let c = z * self.nx + x;
                    self.cells[self.fill[c] as usize] = place as u32;
                    self.fill[c] += 1;
                }
            }
        }
    }

    #[inline(always)]
    fn cell_x(&self, x: f64) -> usize { (((x - self.min_x) / self.size).floor().max(0.0) as usize).min(self.nx - 1) }
    #[inline(always)]
    fn cell_z(&self, z: f64) -> usize { (((z - self.min_z) / self.size).floor().max(0.0) as usize).min(self.nz - 1) }
}

/// world_candidates_for's list for a cast reaching `far` along the ray: the
/// same bodies in the same order, less fixed bodies wholly clear of its
/// segment (see above). Into CANDIDATES; returns how many.
/// Prepare the cast index when the world installs its body membership. This
/// computes only broadphase filing, never a ray result or a Unit's lazy shapes.
pub fn prepare(s: &mut State, world: usize) {
    let capacity = s.worlds[world].order.len() + s.worlds[world].prepared.len();
    s.worlds[world].broad.reserve_bodies(capacity);
    let key = Some((s.worlds[world].version, s.fixed_version));
    if s.worlds[world].broad.made_for != key {
        let mut broad = std::mem::take(&mut s.worlds[world].broad);
        broad.make(&s.bodies, &s.worlds[world]);
        broad.made_for = key;
        s.worlds[world].broad = broad;
    }
}

/// The same broad phase with caller-owned candidate storage.
pub fn world_candidates_near_into(s: &mut State, world: usize, ray: &Ray, far: f64, ignore: i64, static_only: bool, moving_only: bool, out: &mut [u32]) -> usize {
    let length = (ray.dx * ray.dx + ray.dy * ray.dy + ray.dz * ray.dz).sqrt();
    if moving_only || !(far >= 0.0 && far.is_finite() && length > 0.0 && length.is_finite()) {
        return crate::collision::world_candidates_into(s, world, ray, ignore, static_only, moving_only, out);
    }
    let reach = far / length;
    let (o, d) = ([ray.ox, ray.oy, ray.oz], [ray.dx, ray.dy, ray.dz]);
    let pad = MARGIN + RELATIVE * (far + o[0].abs() + o[1].abs() + o[2].abs());
    let mut seg = [0.0; 6];
    for k in 0..3 {
        let end = o[k] + d[k] * reach;
        seg[k] = js_min(o[k], end) - pad;
        seg[k + 3] = js_max(o[k], end) + pad;
    }
    if seg.iter().any(|v| !v.is_finite()) {
        return crate::collision::world_candidates_into(s, world, ray, ignore, static_only, moving_only, out);
    }
    prepare(s,world);
    let (cells, clear) = {
        let g = &s.worlds[world].broad;
        let x0 = ((seg[0] - g.min_x) / g.size).floor();
        let x1 = ((seg[3] - g.min_x) / g.size).floor();
        let z0 = ((seg[2] - g.min_z) / g.size).floor();
        let z1 = ((seg[5] - g.min_z) / g.size).floor();
        let clear = x1 < 0.0 || z1 < 0.0 || x0 >= g.nx as f64 || z0 >= g.nz as f64;
        let x = (x0.max(0.0) as usize, x1.min(g.nx as f64 - 1.0).max(0.0) as usize);
        let z = (z0.max(0.0) as usize, z1.min(g.nz as f64 - 1.0).max(0.0) as usize);
        ((x, z), clear)
    };
    let ((x0, x1), (z0, z1)) = cells;
    if !clear && (x1 - x0 + 1) * (z1 - z0 + 1) > MAX_CELLS {
        return crate::collision::world_candidates_into(s, world, ray, ignore, static_only, moving_only, out);
    }
    let State { worlds, bodies, .. } = s;
    let w = &mut worlds[world];
    let g = &mut w.broad;
    let bits = &mut g.bits;
    bits.iter_mut().for_each(|b| *b = 0);
    if !clear {
        for z in z0..=z1 {
            for x in x0..=x1 {
                let c = z * g.nx + x;
                for &place in &g.cells[g.start[c] as usize..g.start[c + 1] as usize] {
                    let b = &bodies[w.order[place as usize] as usize].bounds;
                    if b[3] + MARGIN < seg[0] || b[0] - MARGIN > seg[3] || b[4] + MARGIN < seg[1] || b[1] - MARGIN > seg[4]
                        || b[5] + MARGIN < seg[2] || b[2] - MARGIN > seg[5] { continue; }
                    bits[place as usize / 64] |= 1 << (place % 64);
                }
            }
        }
    }
    for (i, b) in bits.iter_mut().enumerate() {
        *b |= g.unfiled[i];
        if !static_only { *b |= g.moving[i]; }
    }
    // The tests world_candidates_for gives each body, in the world's order.
    let inv = inverse(ray);
    let mut count = 0;
    for (i, &word) in bits.iter().enumerate() {
        let mut word = word;
        while word != 0 {
            let place = i * 64 + word.trailing_zeros() as usize;
            word &= word - 1;
            let slot = w.order[place];
            let body = &bodies[slot as usize];
            if slot as i64 == ignore || (static_only && body.moving) || !intersects_box(ray, &inv, &body.bounds) { continue; }
            out[count] = place as u32;
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::{Body, World};

    fn random(seed: &mut u32) -> f64 {
        *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        *seed as f64 / 4294967296.0
    }

    fn list(s: &mut State, ray: &Ray, far: Option<f64>, ignore: i64, static_only: bool) -> Vec<u32> {
        let mut out = vec![0; s.worlds[0].order.len()];
        let count = match far {
            Some(far) => world_candidates_near_into(s, 0, ray, far, ignore, static_only, false, &mut out),
            None => crate::collision::world_candidates_into(s, 0, ray, ignore, static_only, false, &mut out),
        };
        out.truncate(count); out
    }

    #[test]
    fn keeps_the_walk_less_bodies_clear_of_the_segment() {
        let mut seed = 7;
        let mut s = State::default();
        // Fixed props spread over a large map, hulls among them, a few
        // bodies the grid cannot file (empty bounds).
        for i in 0..150 {
            let (x, y, z) = ((random(&mut seed) - 0.5) * 1600.0, random(&mut seed) * 40.0 - 20.0, (random(&mut seed) - 0.5) * 1600.0);
            let size = [0.5, 3.0, 12.0, 80.0][i % 4] * (0.5 + random(&mut seed));
            let bounds = if i % 37 == 5 { [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY] } else { [x, y, z, x + size, y + size * 0.5, z + size] };
            s.bodies.push(Body { tree: 0, moving: i % 7 == 3, offset: 0.0, bounds, matrix: [0.0; 16], inverse: [0.0; 16] });
        }
        let mut order: Vec<u32> = (0..150).collect();
        order.reverse();
        s.worlds.push(World { tree: 0, terrain: None, grid: None, order, prepared: Vec::new(), version: 1, statics: None, broad: Default::default() });
        let mut kept = 0;
        for i in 0..20000 {
            let far = [0.3, 1.0, 4.0, 30.0, 150.0, 900.0][i % 6] * (0.5 + random(&mut seed));
            // Rays from near a body, or anywhere, some along an axis.
            let near = &s.bodies[(random(&mut seed) * 150.0) as usize].bounds;
            let o = if random(&mut seed) < 0.5 && near[0].is_finite() { [near[0] + random(&mut seed) * 3.0, near[1] + random(&mut seed) * 3.0, near[2] - random(&mut seed)] }
                else { [(random(&mut seed) - 0.5) * 1800.0, random(&mut seed) * 60.0 - 30.0, (random(&mut seed) - 0.5) * 1800.0] };
            let mut d = [random(&mut seed) - 0.5, random(&mut seed) - 0.5, random(&mut seed) - 0.5];
            if i % 5 == 0 { d = [0.0, -1.0, 0.0]; }
            if i % 11 == 0 { d = [1.0, 0.0, -0.0]; }
            let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            let ray = Ray { ox: o[0], oy: o[1], oz: o[2], dx: d[0] / length, dy: d[1] / length, dz: d[2] / length };
            let (ignore, static_only) = (if i % 9 == 0 { 40 } else { -1 }, i % 4 == 1);
            let all = list(&mut s, &ray, None, ignore, static_only);
            let some = list(&mut s, &ray, Some(far), ignore, static_only);
            // In the walk's order, and only fixed bodies wholly clear of the
            // cast's segment (by more than the margin) left out.
            let mut at = 0;
            for &place in &all {
                if at < some.len() && some[at] == place { at += 1; continue; }
                let body = &s.bodies[s.worlds[0].order[place as usize] as usize];
                assert!(!body.moving, "a moving body left out");
                let b = &body.bounds;
                let reach = far / 1.0;
                let mut clear = false;
                for k in 0..3 {
                    let end = [ray.ox, ray.oy, ray.oz][k] + [ray.dx, ray.dy, ray.dz][k] * reach;
                    let (lo, hi) = (end.min([ray.ox, ray.oy, ray.oz][k]), end.max([ray.ox, ray.oy, ray.oz][k]));
                    if b[k + 3] < lo - 1e-4 || b[k] > hi + 1e-4 { clear = true; }
                }
                assert!(clear, "a body the segment can meet left out: ray {i}");
            }
            assert_eq!(at, some.len(), "a body the walk does not name, or out of order: ray {i}");
            kept += some.len();
        }
        assert!(kept > 1000, "{kept}");
        // A changed order is filed again.
        s.worlds[0].order.truncate(100);
        s.worlds[0].version += 1;
        let ray = Ray { ox: 0.0, oy: 100.0, oz: 0.0, dx: 0.0, dy: -1.0, dz: 0.0 };
        let all = list(&mut s, &ray, None, -1, false);
        let some = list(&mut s, &ray, Some(500.0), -1, false);
        assert!(some.iter().all(|p| all.contains(p)) && some.iter().all(|&p| p < 100));
    }
}
