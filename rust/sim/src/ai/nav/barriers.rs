//! Planning barriers (nav/barriers.js): authored rectangles agents route
//! around, with the uniform grid for candidate gathering. Barriers are
//! indices into `list` where the JavaScript holds the objects. The
//! planner's work counter (the JavaScript's `barriers.work` field while
//! it searches) is passed to the queries that count it instead of being
//! stored: the counts are the same.

use super::math::{f32, normalize, tiny, tiny_dir, Vec3, FLT_MAX};
use std::collections::HashMap;

/// The planner's work budget (planner.js:104): counts barrier tests.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Work {
    pub count: f64,
    pub cap: f64,
}

/// A barrier record of ai-nav.json as numbers (barriers.js:22 reads
/// `record.xfrm`, `record.size`, `record.flags`, `record.name`).
#[derive(Clone, Debug)]
pub struct BarrierRecord {
    pub name: Option<String>,
    pub xfrm: [f64; 12],
    pub size: [f64; 3],
    pub flags: f64,
}

/// A loaded barrier (barriers.js:26).
#[derive(Clone, Debug, PartialEq)]
pub struct Barrier {
    pub index: usize,
    pub name: Option<String>,
    pub flags: u32,
    pub cx: f64,
    pub cy: f64,
    pub cz: f64,
    pub xx: f64,
    pub xy: f64,
    pub xz: f64,
    pub zx: f64,
    pub zy: f64,
    pub zz: f64,
    pub hx: f64,
    pub hy: f64,
    pub hz: f64,
    pub bound: f64,
}

/// barriers.js:22. The stored translation's z is negated; basis rows are
/// kept as stored.
pub fn barrier_from_record(record: &BarrierRecord, index: usize) -> Result<Barrier, String> {
    let m = &record.xfrm;
    let (sx, sy, sz) = (record.size[0], record.size[1], record.size[2]);
    for v in m.iter().copied().chain([sx, sy, sz]) {
        if !v.is_finite() {
            return Err(format!("ai-nav: barrier {index} is not finite"));
        }
    }
    Ok(Barrier {
        index,
        name: record.name.clone(),
        flags: v8math::to_uint32(record.flags),
        cx: f32(m[9]),
        cy: f32(m[10]),
        cz: f32(-m[11]),
        xx: f32(m[0]),
        xy: f32(m[1]),
        xz: f32(m[2]),
        zx: f32(m[6]),
        zy: f32(m[7]),
        zz: f32(m[8]),
        hx: f32(sx),
        hy: f32(sy),
        hz: f32(sz),
        bound: f32(v8math::sqrt(sx * sx + sy * sy + sz * sz)),
    })
}

/// Strictly inside the footprint grown by r (barriers.js:41).
pub fn inside(b: &Barrier, px: f64, py: f64, pz: f64, r: f64) -> bool {
    let dx = b.cx - px;
    let dy = b.cy - py;
    let dz = b.cz - pz;
    v8math::abs(b.xx * dx + b.xy * dy + b.xz * dz) < b.hx + r && v8math::abs(b.zx * dx + b.zy * dy + b.zz * dz) < b.hz + r
}

/// Corner i of the footprint grown by r + e (barriers.js:51).
pub fn corner(b: &Barrier, i: usize, r: f64, e: f64) -> Vec3 {
    let u = b.hx + r + e;
    let v = b.hz + r + e;
    let su = if i < 2 { 1.0 } else { -1.0 };
    let sv = if i == 0 || i == 3 { 1.0 } else { -1.0 };
    Vec3::new(
        f32(b.cx + su * b.xx * u + sv * b.zx * v),
        f32(b.cy + su * b.xy * u + sv * b.zy * v),
        f32(b.cz + su * b.xz * u + sv * b.zz * v),
    )
}

/// A query segment: ends, midpoint and 3D half length (barriers.js:63).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Segment {
    pub sx: f64,
    pub sy: f64,
    pub sz: f64,
    pub ex: f64,
    pub ey: f64,
    pub ez: f64,
    pub mx: f64,
    pub my: f64,
    pub mz: f64,
    pub half: f64,
}

pub fn segment(s: Vec3, e: Vec3) -> Segment {
    let mx = (s.x + e.x) / 2.0;
    let my = (s.y + e.y) / 2.0;
    let mz = (s.z + e.z) / 2.0;
    let (hx, hy, hz) = (mx - s.x, my - s.y, mz - s.z);
    Segment {
        sx: s.x,
        sy: s.y,
        sz: s.z,
        ex: e.x,
        ey: e.y,
        ez: e.z,
        mx,
        my,
        mz,
        half: v8math::sqrt(hx * hx + hy * hy + hz * hz),
    }
}

/// What a crossing reports (barriers.js:116).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CrossingHit {
    pub distance_sq: f64,
    pub normal: Vec3,
}

/// Segment against one barrier grown by r (barriers.js:83).
pub fn crossing(b: &Barrier, seg: &Segment, r: f64, work: Option<&mut Work>, hit: Option<&mut CrossingHit>) -> bool {
    let mx = seg.mx - b.cx;
    let mz = seg.mz - b.cz;
    let grow = seg.half + r;
    let reach = b.bound + grow;
    if !(mx * mx + mz * mz < reach * reach) {
        return false;
    }
    if !inside(b, seg.mx, seg.my, seg.mz, grow) {
        return false;
    }
    if let Some(w) = work {
        w.count += 1.0;
    }
    let corners = [corner(b, 0, r, 0.0), corner(b, 1, r, 0.0), corner(b, 2, r, 0.0), corner(b, 3, r, 0.0)];
    let dx = seg.ex - seg.sx;
    let dz = seg.ez - seg.sz;
    let mut best = 1e10;
    let (mut nx, mut nz) = (0.0, 0.0);
    for i in 0..4 {
        let c = corners[i];
        let n = corners[(i + 1) & 3];
        let ex = n.x - c.x;
        let ez = n.z - c.z;
        let det = dx * ez - dz * ex;
        if !(v8math::abs(det) > tiny()) {
            continue;
        }
        let inv = 1.0 / det;
        let wx = c.x - seg.sx;
        let wz = c.z - seg.sz;
        let u = (wx * dz - wz * dx) * inv;
        let t = (wx * ez - wz * ex) * inv;
        if t >= 0.0 && t <= 1.0 && u >= 0.0 && u <= 1.0 && t < best {
            best = t;
            nx = -ez;
            nz = ex;
        }
    }
    if !(best < 1e10) {
        return false;
    }
    if let Some(hit) = hit {
        hit.distance_sq = best * best * (dx * dx + dz * dz);
        hit.normal = Vec3::new(nx, 0.0, nz);
        normalize(&mut hit.normal);
    }
    true
}

/// Ejection candidate i (barriers.js:289); also returns the candidate
/// point (the JavaScript's `out`).
pub fn ejection(b: &Barrier, p: Vec3, r: f64, i: usize) -> (f64, Vec3) {
    let a = corner(b, i, r, 0.0);
    let c = corner(b, if i > 2 { 0 } else { i + 1 }, r, 0.0);
    let (px, py, pz) = (p.x, 0.0, p.z);
    let (ex, ey, ez) = (a.x - c.x, a.y - c.y, a.z - c.z);
    let t = (px - c.x) * ex + (py - c.y) * ey + (pz - c.z) * ez;
    let (qx, qy, qz);
    if t > 0.0 {
        let len = ex * ex + ey * ey + ez * ez;
        if t < len {
            let k = t / len;
            qx = c.x + k * ex;
            qy = c.y + k * ey;
            qz = c.z + k * ez;
        } else {
            qx = a.x;
            qy = a.y;
            qz = a.z;
        }
    } else {
        qx = c.x;
        qy = c.y;
        qz = c.z;
    }
    let score = (px - qx) * (px - qx) + (py - qy) * (py - qy) + (pz - qz) * (pz - qz);
    let mut n = Vec3::new(c.z - a.z, 0.0, -(c.x - a.x));
    normalize(&mut n);
    (score, Vec3::new(qx + 0.01 * n.x, qy, qz + 0.01 * n.z))
}

/// The two extreme detour corners (barriers.js:319). Returns (found bits,
/// left, right); a side that was not found leaves its point as the
/// caller passed it, like the JavaScript's out parameters.
pub fn detour(b: &Barrier, p: Vec3, q: Vec3, r: f64, left: &mut Vec3, right: &mut Vec3) -> u32 {
    let mut dx = q.x - p.x;
    let mut dz = q.z - p.z;
    if dx * dx + dz * dz < tiny_dir() {
        dx = 1.0;
        dz = 0.0;
    }
    let mut l = v8math::sqrt(dx * dx + dz * dz);
    dx /= l;
    dz /= l;
    let mut best_left = 1.0;
    let mut best_right = 1.0;
    let mut found = 0u32;
    for i in 0..4 {
        let c = corner(b, i, r, 0.5);
        let mut vx = c.x - p.x;
        let mut vz = c.z - p.z;
        let v2 = vx * vx + vz * vz;
        if !(v2 > tiny_dir()) {
            continue;
        }
        l = v8math::sqrt(v2);
        vx /= l;
        vz /= l;
        let along = dx * vx + dz * vz;
        let cross = dz * vx - dx * vz;
        if cross < 0.0 && along < best_left {
            *left = c;
            best_left = along;
            found |= 1;
        } else if cross > 0.0 && along < best_right {
            *right = c;
            best_right = along;
            found |= 2;
        }
    }
    found
}

fn cell_key(x: f64, z: f64) -> i32 {
    // barriers.js:142 `x * 73856093 ^ z * 19349663`: f64 products, then
    // the ^ operator's ToInt32 on each.
    v8math::to_int32(x * 73856093.0) ^ v8math::to_int32(z * 19349663.0)
}

/// The barriers of a map with a uniform grid (barriers.js:124).
#[derive(Clone, Debug)]
pub struct BarrierSet {
    pub list: Vec<Barrier>,
    pub cell: f64,
    cells: HashMap<i32, Vec<usize>>,
    stamp: Vec<u32>,
    serial: u32,
    found: Vec<usize>,
    seg: Segment,
    hit: CrossingHit,
}

impl BarrierSet {
    /// barriers.js:125, with the JavaScript's default cell of 16.
    pub fn new(barriers: Vec<Barrier>) -> Self {
        Self::with_cell(barriers, 16.0)
    }

    pub fn with_cell(barriers: Vec<Barrier>, cell: f64) -> Self {
        let mut cells: HashMap<i32, Vec<usize>> = HashMap::new();
        for b in &barriers {
            let x0 = v8math::floor((b.cx - b.bound) / cell);
            let x1 = v8math::floor((b.cx + b.bound) / cell);
            let z0 = v8math::floor((b.cz - b.bound) / cell);
            let z1 = v8math::floor((b.cz + b.bound) / cell);
            let mut x = x0;
            while x <= x1 {
                let mut z = z0;
                while z <= z1 {
                    cells.entry(cell_key(x, z)).or_default().push(b.index);
                    z += 1.0;
                }
                x += 1.0;
            }
        }
        let n = barriers.len();
        BarrierSet {
            list: barriers,
            cell,
            cells,
            stamp: vec![0; n],
            serial: 0,
            found: Vec::new(),
            seg: Segment::default(),
            hit: CrossingHit::default(),
        }
    }

    /// Barriers whose bounding circle meets the box, in source order
    /// (barriers.js:153). The JavaScript returns its reused `found`
    /// array; this returns a copy of the indices, same contents.
    pub fn gather(&mut self, x0: f64, z0: f64, x1: f64, z1: f64) -> Vec<usize> {
        self.found.clear();
        self.serial = self.serial.wrapping_add(1);
        if self.serial == 0xffff_ffff {
            self.stamp.fill(0);
            self.serial = 1;
        }
        let c = self.cell;
        let cx0 = v8math::floor(x0 / c);
        let cx1 = v8math::floor(x1 / c);
        let cz0 = v8math::floor(z0 / c);
        let cz1 = v8math::floor(z1 / c);
        let mut x = cx0;
        while x <= cx1 {
            let mut z = cz0;
            while z <= cz1 {
                if let Some(list) = self.cells.get(&cell_key(x, z)) {
                    for i in 0..list.len() {
                        let bi = list[i];
                        if self.stamp[bi] == self.serial {
                            continue;
                        }
                        self.stamp[bi] = self.serial;
                        let b = &self.list[bi];
                        let qx = v8math::min(v8math::max(b.cx, x0), x1) - b.cx;
                        let qz = v8math::min(v8math::max(b.cz, z0), z1) - b.cz;
                        if qx * qx + qz * qz <= b.bound * b.bound {
                            self.found.push(bi);
                        }
                    }
                }
                z += 1.0;
            }
            x += 1.0;
        }
        if self.found.len() > 1 {
            // out.sort((a, b) => a.index - b.index): unique indices, so
            // any correct sort gives the JavaScript's order.
            self.found.sort_unstable();
        }
        self.found.clone()
    }

    /// barriers.js:184. Returns (clear, the JavaScript's out.barrier).
    pub fn point_clear(&mut self, p: Vec3, r: f64, mask: u32) -> (bool, Option<usize>) {
        let list = self.gather(p.x - r, p.z - r, p.x + r, p.z + r);
        for bi in list {
            let b = &self.list[bi];
            if (b.flags & mask) == 0 {
                continue;
            }
            if inside(b, p.x, p.y, p.z, r) {
                return (false, Some(bi));
            }
        }
        (true, None)
    }

    /// barriers.js:199.
    pub fn segment_blocked(&mut self, s: Vec3, e: Vec3, r: f64, mask: u32, mut work: Option<&mut Work>) -> bool {
        let dx = s.x - e.x;
        let dy = s.y - e.y;
        let dz = s.z - e.z;
        if dx * dx + dy * dy + dz * dz < tiny() {
            return false;
        }
        self.seg = segment(s, e);
        let list = self.gather(v8math::min(s.x, e.x) - r, v8math::min(s.z, e.z) - r, v8math::max(s.x, e.x) + r, v8math::max(s.z, e.z) + r);
        for bi in list {
            if (self.list[bi].flags & mask) == 0 {
                continue;
            }
            let seg = self.seg;
            if crossing(&self.list[bi], &seg, r, work.as_deref_mut(), None) {
                return true;
            }
        }
        false
    }

    /// barriers.js:213. Returns (any, barrier, distanceSq).
    pub fn nearest(&mut self, s: Vec3, e: Vec3, r: f64, mask: u32, mut work: Option<&mut Work>) -> (bool, Option<usize>, f64) {
        self.seg = segment(s, e);
        let list = self.gather(v8math::min(s.x, e.x) - r, v8math::min(s.z, e.z) - r, v8math::max(s.x, e.x) + r, v8math::max(s.z, e.z) + r);
        let mut any = false;
        let mut best = FLT_MAX;
        let mut barrier = None;
        for bi in list {
            if (self.list[bi].flags & mask) == 0 {
                continue;
            }
            let seg = self.seg;
            let mut hit = self.hit;
            if !crossing(&self.list[bi], &seg, r, work.as_deref_mut(), Some(&mut hit)) {
                continue;
            }
            self.hit = hit;
            any = true;
            if hit.distance_sq < best {
                best = hit.distance_sq;
                barrier = Some(bi);
            }
        }
        (any, barrier, best)
    }

    /// barriers.js:235. Push p out of the barrier that blocks it.
    pub fn correct(&mut self, p: &mut Vec3, r: f64, mask: u32) -> bool {
        let (clear, barrier) = self.point_clear(*p, r, mask);
        if clear {
            return true;
        }
        let b = self.list[barrier.expect("point_clear names the blocker")].clone();
        let mut scores = [0.0; 4];
        let mut cands = [Vec3::default(); 4];
        for i in 0..4 {
            let (score, cand) = ejection(&b, *p, r, i);
            scores[i] = score;
            cands[i] = cand;
        }
        for _attempt in 0..4 {
            let mut pick = 0;
            let mut low = FLT_MAX;
            for i in 0..4 {
                if scores[i] < low {
                    low = scores[i];
                    pick = i;
                }
            }
            if self.point_clear(cands[pick], r, mask).0 {
                p.x = cands[pick].x;
                p.z = cands[pick].z;
                return true;
            }
            scores[pick] = FLT_MAX;
        }
        false
    }

    /// barriers.js:265. Shortens `e` to the nearest crossing; the crossed
    /// edge's normal comes back in `normal` when given.
    pub fn obstruct(&mut self, s: Vec3, e: &mut Vec3, r: f64, mask: u32, mut work: Option<&mut Work>, normal: Option<&mut Vec3>) -> bool {
        let mut dir = Vec3::new(e.x - s.x, e.y - s.y, e.z - s.z);
        normalize(&mut dir);
        self.seg = segment(s, *e);
        let grow = 2.0 * r;
        let list = self.gather(v8math::min(s.x, e.x) - grow, v8math::min(s.z, e.z) - grow, v8math::max(s.x, e.x) + grow, v8math::max(s.z, e.z) + grow);
        let mut any = false;
        let mut normal = normal;
        for bi in list {
            if (self.list[bi].flags & mask) == 0 {
                continue;
            }
            let seg = self.seg;
            let mut hit = self.hit;
            if !crossing(&self.list[bi], &seg, r, work.as_deref_mut(), Some(&mut hit)) {
                continue;
            }
            self.hit = hit;
            let d = v8math::sqrt(hit.distance_sq);
            e.x = s.x + dir.x * d;
            e.y = s.y + dir.y * d;
            e.z = s.z + dir.z * d;
            self.seg = segment(s, *e);
            if let Some(n) = normal.as_deref_mut() {
                *n = hit.normal;
            }
            any = true;
        }
        any
    }
}
