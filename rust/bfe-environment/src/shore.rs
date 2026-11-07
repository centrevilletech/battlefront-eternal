//! WaterShore.waterlineSegment, clipToTile, sampleTile and update. The page
//! supplies collision triangles and downward casts; all geometry, deduplication,
//! cache admission and stable distance selection remain inside this owner.
use crate::layout::{shore_point as point, shore_triangle as tri, surface as lim};
use crate::surface_math::Math;

// Collision is still a separate memory owner. Queries transfer one tile's
// triangles in their gather/shapecast order, and answer exposed-water probes.
// The bridge copies resource facts; it does no shoreline sampling or sorting.
pub trait Queries {
    fn triangles(&mut self, x: f64, z: f64, height: f64, output: &mut [f64]) -> usize;
    fn exposed(&mut self, x: f64, z: f64, height: f64) -> bool;
}
#[derive(Clone, Copy, Default)]
struct Point { p: [f64; 4], key: [f64; 2] }
struct Tile { x: f64, z: f64, count: usize, points: Vec<Point> }
#[derive(Clone, Copy, Default)]
struct Pending { x: f64, z: f64, distance: i32, order: i32 }
#[derive(Clone, Copy, Default)]
struct Candidate { p: [f64; 4], distance: f64 }

pub struct Shore {
    tiles: Vec<Tile>,
    tile_count: usize,
    pending: [Pending; 121],
    pending_count: usize,
    pending_next: usize,
    tile: [f64; 2],
    draw_tile: [f64; 2],
    quality: usize,
    triangles: Vec<f64>,
    selected: [Candidate; lim::SHORE_PATCHES],
    pub points: Vec<f32>,
    pub phases: Vec<f32>,
    pub time: f64,
    pub count: usize,
    pub dirty: bool,
    pub overflow: u64,
}
impl Default for Shore { fn default() -> Self { Self::new(false) } }
impl Shore {
    pub fn new(water: bool) -> Self {
        // An 8 m tile has 17 closed half-metre keys on each axis: 289
        // possible entries. Dry maps never gather tiles or need their cache.
        Self { tiles: (0..if water { lim::SHORE_TILES } else { 0 }).map(|_| Tile { x: 0.0, z: 0.0, count: 0,
                points: vec![Point::default(); lim::SHORE_POINTS] }).collect(), tile_count: 0,
            pending: [Pending::default(); 121], pending_count: 0, pending_next: 0,
            tile: [f64::NAN; 2], draw_tile: [f64::NAN; 2], quality: 0,
            triangles: vec![0.0; if water { lim::SHORE_TRIANGLES * tri::STRIDE } else { 0 }],
            selected: [Candidate::default(); lim::SHORE_PATCHES], points: vec![0.0; lim::SHORE_PATCHES * point::STRIDE],
            phases: vec![0.0; lim::SHORE_PATCHES], time: 0.0, count: 0, dirty: false, overflow: 0 }
    }
    pub fn tiles(&self) -> usize { self.tile_count }
    // WaterShore.update stops its own clock at Low/Medium quality. Returning
    // here also leaves its cache intact for a later quality switch.
    // Camera tiles use floor, including the negative side of the world origin.
    pub fn update<M: Math, Q: Queries>(&mut self, dt: f64, eye_x: f64, eye_z: f64, height: f64, quality: usize, queries: &mut Q) {
        self.dirty = false;
        if self.tiles.is_empty() || quality == 0 || !eye_x.is_finite() || !eye_z.is_finite() { return; }
        self.time += dt;
        let (tx, tz) = ((eye_x / 8.0).floor(), (eye_z / 8.0).floor());
        if self.tile != [tx, tz] {
            self.tile = [tx, tz];
            self.pending_count = 0; self.pending_next = 0;
            // WaterShore.update's Map keeps surviving entries in insertion
            // order. Rotate deleted entries into reusable slots at the tail.
            let mut i = 0;
            while i < self.tile_count {
                if (self.tiles[i].x-tx).abs() > 6.0 || (self.tiles[i].z-tz).abs() > 6.0 {
                    self.tiles[i..self.tile_count].rotate_left(1); self.tile_count -= 1;
                } else { i += 1; }
            }
            for dz in -5i32..=5 { for dx in -5i32..=5 {
                let d = dx*dx + dz*dz;
                if d > 26 { continue; }
                let (x,z) = (tx+dx as f64, tz+dz as f64);
                if self.tiles[..self.tile_count].iter().any(|t| t.x == x && t.z == z) { continue; }
                let n = self.pending_count;
                self.pending[n] = Pending { x, z, distance: d, order: n as i32 }; self.pending_count += 1;
            }}
            // Explicit ordinal ties give JS's stable sort without sort's
            // temporary allocation, including equal-distance diagonal tiles.
            self.pending[..self.pending_count].sort_unstable_by_key(|p| (p.distance, p.order));
        }
        // A camera change invalidates the drawn prefix even before new tiles
        // arrive. Sampling is limited to two tiles regardless of frame length,
        // matching the old amortized collision budget during teleports.
        let mut dirty = self.draw_tile != [tx, tz];
        for _ in 0..2 {
            if self.pending_next >= self.pending_count { break; }
            let p = self.pending[self.pending_next]; self.pending_next += 1;
            if self.tile_count == lim::SHORE_TILES { self.overflow += 1; continue; }
            let n = self.tile_count; self.tile_count += 1;
            self.tiles[n].x = p.x; self.tiles[n].z = p.z; self.tiles[n].count = 0;
            let count = queries.triangles(p.x, p.z, height, &mut self.triangles);
            if count > lim::SHORE_TRIANGLES { self.overflow += 1; }
            for i in 0..count.min(lim::SHORE_TRIANGLES) {
                let at = i * tri::STRIDE;
                let mut vertices = [[0.0;3];3];
                for v in 0..3 { vertices[v].copy_from_slice(&self.triangles[at+v*3..at+v*3+3]); }
                Self::sample(&vertices, height, &mut self.tiles[n], queries, &mut self.overflow);
            }
            dirty = true;
        }
        if !dirty && self.quality == quality { return; }
        self.draw_tile = [tx,tz]; self.quality = quality; self.dirty = true;
        let limit = if quality >= 2 { lim::SHORE_PATCHES } else { lim::SHORE_PATCHES/2 };
        let mut count = 0;
        // WaterShore.update sorted all cached points before taking the nearest
        // prefix. A bounded insertion list gives that same stable prefix and
        // avoids a second large point table or any per-frame allocation.
        for tile in &self.tiles[..self.tile_count] { for p in &tile.points[..tile.count] {
            let (dx,dz) = (p.p[0]-eye_x,p.p[1]-eye_z);
            let distance = dx*dx+dz*dz;
            if distance >= 40.0*40.0 { continue; }
            let mut at = count;
            while at > 0 && self.selected[at-1].distance > distance { at -= 1; }
            if at >= limit { continue; }
            let end = count.min(limit-1);
            self.selected.copy_within(at..end, at+1);
            self.selected[at] = Candidate { p: p.p, distance }; count = (count+1).min(limit);
        }}
        // Only this nearest prefix is visible to the renderer resource shell.
        // Unused attribute tails retain their previous bits, as setXYZW did.
        self.count = count;
        for i in 0..count {
            let p = self.selected[i].p;
            for j in 0..4 { self.points[i*point::STRIDE+j] = p[j] as f32; }
            let phase = M::sin(p[0]*12.9898+p[1]*78.233)*43758.5453;
            self.phases[i] = (((phase % 1.0)+1.0)%1.0) as f32;
        }
    }
    // sampleTile takes architecture triangles followed by terrain triangles.
    // Its half-metre keys replace matching points in place, preserving the
    // first insertion's order while retaining the last normal at that point.
    fn sample<Q: Queries>(triangle: &[[f64;3];3], height: f64, tile: &mut Tile, queries: &mut Q, overflow: &mut u64) {
        // Real triangle intersections create shore; grid-cell edges never do.
        // The exposed probes must disagree or the point lies entirely on
        // dry ground, entirely underwater, or under a covering surface.
        let Some([mut a, mut b]) = waterline_segment(triangle, height) else { return; };
        let (mut lo,mut hi) = (0.0f64,1.0f64);
        for (axis,min) in [(0,tile.x*8.0),(2,tile.z*8.0)] {
            let delta = b[axis]-a[axis];
            if delta.abs() < 1e-8 { if a[axis] < min || a[axis] > min+8.0 { return; } continue; }
            let (t0,t1) = ((min-a[axis])/delta,(min+8.0-a[axis])/delta);
            lo = lo.max(t0.min(t1)); hi = hi.min(t0.max(t1));
        }
        if hi <= lo { return; }
        // clipToTile uses Vector3.lerp (not lerpVectors); both endpoints
        // derive from the original segment and retain its expression order.
        let original = a;
        for axis in 0..3 { a[axis] += (b[axis]-a[axis])*lo; b[axis] = original[axis]+(b[axis]-original[axis])*hi; }
        let (dx,dy,dz) = (b[0]-a[0],b[1]-a[1],b[2]-a[2]);
        let length = (dx*dx+dy*dy+dz*dz).sqrt();
        let (nx,nz) = (-dz/length,dx/length);
        // The original spacing divides each clipped segment into equal steps.
        // Sampling at step centres avoids doubling every shared endpoint.
        // It also keeps normals derived from the whole clipped segment.
        let steps = (length/1.6).ceil().max(1.0) as usize;
        // Pathological overlong geometry is clipped to an 8 m tile, so this
        // bound exceeds any valid segment while preventing malformed loops.
        for i in 0..steps.min(32) {
            let t = (i as f64+0.5)/steps as f64;
            let (px,pz) = ((1.0-t)*a[0]+t*b[0],(1.0-t)*a[2]+t*b[2]);
            let plus = queries.exposed(px+nx*0.45,pz+nz*0.45,height);
            let minus = queries.exposed(px-nx*0.45,pz-nz*0.45,height);
            if plus == minus { continue; }
            let sign = if plus { 1.0 } else { -1.0 };
            let p = Point { p: [px,pz,nx*sign,nz*sign], key: [v8math::round(px*2.0),v8math::round(pz*2.0)] };
            if let Some(at) = tile.points[..tile.count].iter().position(|old| old.key == p.key) { tile.points[at] = p; }
            else if tile.count < lim::SHORE_POINTS { tile.points[tile.count] = p; tile.count += 1; }
            else { *overflow += 1; }
        }
    }
}
// WaterShore.waterlineSegment ignores coplanar edges, merged endpoints and
// segments shorter than 0.1 m, exactly as its original triangle tests did.
// Vertex deduplication uses squared distance, with the original strict limits.
// A triangle meeting the plane at a single vertex produces no foam segment.
// Keep the directed edge order because its normal determines the probe sides.
pub fn waterline_segment(vertices: &[[f64;3];3], height: f64) -> Option<[[f64;3];2]> {
    let mut points = [[0.0;3];3]; let mut count = 0;
    for i in 0..3 {
        let (a,b) = (vertices[i],vertices[(i+1)%3]);
        let (da,db) = (a[1]-height,b[1]-height);
        if da.abs() < 1e-6 && db.abs() < 1e-6 { continue; }
        if da*db > 0.0 || (da-db).abs() < 1e-8 { continue; }
        let t = da/(da-db); let mut p = [0.0;3];
        for j in 0..3 { p[j] = a[j]+(b[j]-a[j])*t; }
        if points[..count].iter().any(|q| distance(q,&p) < 1e-8) { continue; }
        points[count] = p; count += 1;
    }
    if count == 2 && distance(&points[0],&points[1]) > 0.01 { Some([points[0],points[1]]) } else { None }
}
fn distance(a: &[f64;3], b: &[f64;3]) -> f64 { let (x,y,z) = (a[0]-b[0],a[1]-b[1],a[2]-b[2]); x*x+y*y+z*z }

#[cfg(test)]
mod tests {
    use super::*;
    struct Water;
    impl Queries for Water {
        fn triangles(&mut self, _: f64, _: f64, _: f64, _: &mut [f64]) -> usize { panic!("dry shore queried collision") }
        fn exposed(&mut self, x: f64, _: f64, _: f64) -> bool { x >= 0.0 }
    }
    #[test]
    fn dry_maps_reserve_no_tile_cache_or_collision_scratch() {
        let mut shore = Shore::new(false);
        assert_eq!(shore.tiles.capacity(), 0);
        assert_eq!(shore.triangles.capacity(), 0);
        // Even a malformed host quality cannot turn a dry map into a query.
        shore.update::<crate::surface_math::V8, _>(1.0, 0.0, 0.0, 0.0, usize::MAX, &mut Water);
        assert_eq!((shore.tiles(), shore.count, shore.time), (0, 0, 0.0));
    }
    #[test]
    fn a_full_closed_key_grid_keeps_replacing_points_without_growth() {
        let mut shore = Shore::new(true);
        let bytes: usize = shore.tiles.iter().map(|t| t.points.capacity() * std::mem::size_of::<Point>()).sum();
        assert_eq!(bytes, 2_344_368);
        let tile = &mut shore.tiles[0];
        // Populate every key in the closed 8 m square. Later triangles must
        // replace normals in place even when every possible key is occupied.
        for z in 0..=16 { for x in 0..=16 {
            tile.points[tile.count] = Point { p: [x as f64 / 2.0, z as f64 / 2.0, 0.0, 0.0], key: [x as f64, z as f64] };
            tile.count += 1;
        }}
        let ptr = tile.points.as_ptr();
        let mut overflow = 0;
        Shore::sample(&[[0.0,-1.0,0.0],[0.0,1.0,0.0],[0.0,1.0,8.0]], 0.0, tile, &mut Water, &mut overflow);
        assert_eq!(tile.count, 289);
        assert_eq!(tile.points.as_ptr(), ptr);
        assert_eq!(overflow, 0);
        assert!(tile.points.iter().any(|p| p.p[2] != 0.0));
    }
}
