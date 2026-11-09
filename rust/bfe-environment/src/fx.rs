//! The Enhanced destruction's fire, smoke and electric arcs
//! (src/destruction-fx.js DestructionFx): two fixed pools of instanced quad
//! records and a pool of jagged ribbons.
//!
//! A particle is written once at its birth and the vertex shader works out
//! where it is from its age, so the CPU's whole job is to append births,
//! retire the dead (swapping the last record in) and keep the clock
//! precise. Arcs are the exception: their shape is redrawn 30 times a
//! second with a new random jag, each end fixed in the world or carried by
//! an object whose world matrix the page hands over each frame.
//!
//! Everything is allocated in `new`; no call afterwards allocates.

use crate::layout::{fx as limits, result as rs};
use crate::random::Random;
use three::{Matrix4, Vector3};

/// Births are shifted back by this much (s) when the clock passes it, so the
/// clock stays precise in the f32 attributes (REBASE).
// Births are shifted back by this much when the clock passes it, so the clock
// stays precise in the 32-bit attributes.
const REBASE: f64 = 600.0;
const SEGMENTS: usize = 12;
const BRANCH_SEGMENTS: usize = 5;
const BRANCHES: usize = 2;
pub const POINTS_PER_ARC: usize = SEGMENTS + 1 + BRANCHES * (BRANCH_SEGMENTS + 1);
/// New jags a second.
const FLICKER: f64 = 30.0;

/// A fixed pool of instanced quads whose records live in `attributes`
/// arrays of four floats each: births are appended, deaths swap in the last
/// record (Pool).
// A fixed pool of instanced quads: births are appended, deaths swap the last
// record in. The records' meaning is the shader's: it works out where a
// particle is from its age, so the CPU only appends and retires.
pub struct Pool {
    pub capacity: usize,
    pub count: usize,
    /// First record changed since the last update; `usize::MAX` for none.
    pub dirty: usize,
    pub emitted: u32,
    /// Births that found the pool full.
    pub dropped: u32,
    pub attributes: Vec<Vec<f32>>,
}

impl Pool {
    fn new(capacity: usize, attributes: usize) -> Pool {
        Pool { capacity, count: 0, dirty: usize::MAX, emitted: 0, dropped: 0, attributes: vec![vec![0.0; capacity * 4]; attributes] }
    }

    /// A free record's index, or none when the pool is full (the particle is
    /// dropped: the ones already flying matter more than one more).
    // A free record, or none when the pool is full (the particle is dropped: the
    // ones already flying matter more than one more).
    fn take(&mut self) -> Option<usize> {
        if self.count >= self.capacity {
            self.dropped += 1;
            return None;
        }
        let i = self.count;
        self.count += 1;
        if i < self.dirty {
            self.dirty = i;
        }
        self.emitted += 1;
        Some(i)
    }

    fn set(&mut self, attribute: usize, i: usize, x: f64, y: f64, z: f64, w: f64) {
        let a = &mut self.attributes[attribute];
        let k = i * 4;
        a[k] = x as f32;
        a[k + 1] = y as f32;
        a[k + 2] = z as f32;
        a[k + 3] = w as f32;
    }

    /// Retires the particles whose life is up (Pool.update). The scan runs
    /// from the end, so the record swapped into a hole has been tested.
    // Retires the particles whose life is up. The scan runs from the end, so the
    // record swapped into a hole has been tested.
    fn retire(&mut self, time: f64) {
        for i in (0..self.count).rev() {
            let born = self.attributes[0][i * 4 + 3] as f64;
            let life = self.attributes[1][i * 4 + 3] as f64;
            if time - born < life {
                continue;
            }
            self.count -= 1;
            let last = self.count;
            if i != last {
                for array in self.attributes.iter_mut() {
                    array.copy_within(last * 4..last * 4 + 4, i * 4);
                }
                if i < self.dirty {
                    self.dirty = i;
                }
            }
        }
    }

    /// Every birth moved back by `by` seconds, with the clock (Pool.rebase).
    // Every birth moved back with the clock; every record changed.
    fn rebase(&mut self, by: f64) {
        for i in 0..self.count {
            let at = i * 4 + 3;
            self.attributes[0][at] = (self.attributes[0][at] as f64 - by) as f32;
        }
        if self.count > 0 {
            self.dirty = 0;
        }
    }

    fn clear(&mut self) {
        self.count = 0;
        self.dirty = usize::MAX;
    }
}

/// What an arc end is carried by (Anchor): nothing (a point in the world),
/// one of the pieces flying, or a bone of a droid (by the page's slot for it
/// and the bone's place in the list it gave). `matrix` is the carrier's
/// world matrix as last seen; the owner of the carriers refreshes it.
#[derive(Clone, Copy)]
// What an arc end is carried by: nothing (a point in the world), a piece, or a
// bone of a droid. The matrix is the carrier's as last seen; whoever owns
// the carriers refreshes it before the arcs are drawn.
pub struct Anchor {
    pub kind: AnchorKind,
    pub a: u32,
    pub b: u32,
    pub matrix: [f64; 16],
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    World,
    Piece,
    Bone,
}

impl Anchor {
    pub const WORLD: Anchor = Anchor { kind: AnchorKind::World, a: 0, b: 0, matrix: [0.0; 16] };

    pub fn piece(uid: u32, matrix: [f64; 16]) -> Anchor {
        Anchor { kind: AnchorKind::Piece, a: uid, b: 0, matrix }
    }

    pub fn bone(slot: u32, bone: u32, matrix: [f64; 16]) -> Anchor {
        Anchor { kind: AnchorKind::Bone, a: slot, b: bone, matrix }
    }
}

/// One arc (Arcs.arcs[i]).
#[derive(Clone, Copy)]
struct Arc {
    live: bool,
    age: f64,
    life: f64,
    width: f64,
    intensity: f64,
    seed: u32,
    from: Anchor,
    to: Anchor,
    from_point: Vector3,
    to_point: Vector3,
}

impl Default for Arc {
    fn default() -> Arc {
        Arc { live: false, age: 0.0, life: 0.0, width: 0.0, intensity: 0.0, seed: 0, from: Anchor::WORLD, to: Anchor::WORLD, from_point: Vector3::default(), to_point: Vector3::default() }
    }
}

/// A seeded random number in [0, 1) from two integers, without state
/// (jitter): JavaScript's int32 arithmetic, bit for bit.
// A seeded random number in [0, 1) from two integers, without state: the
// arcs' jag is redrawn thirty times a second from it, which is what makes
// them crackle.
fn jitter(seed: u32, n: i32) -> f64 {
    let mut h = (seed as i32 ^ n.wrapping_mul(0x9e3779b1u32 as i32)).wrapping_mul(0x85ebca6bu32 as i32);
    h ^= ((h as u32) >> 13) as i32;
    h = h.wrapping_mul(0xc2b2ae35u32 as i32);
    h ^= ((h as u32) >> 16) as i32;
    (h as u32) as f64 / 4294967296.0
}

pub struct Arcs {
    capacity: usize,
    arcs: Vec<Arc>,
    pub live: usize,
    pub spawned: u32,
    pub vertex: usize,
    pub index: usize,
    pub positions: Vec<f32>,
    pub tangents: Vec<f32>,
    pub meta: Vec<f32>,
    pub indices: Vec<u16>,
    points: [Vector3; SEGMENTS + 1],
    branch_points: [Vector3; BRANCH_SEGMENTS + 1],
    tangent: Vector3,
}

impl Arcs {
    fn new(capacity: usize) -> Arcs {
        let vertices = capacity * POINTS_PER_ARC * 2;
        let quads = capacity * (SEGMENTS + BRANCHES * BRANCH_SEGMENTS);
        Arcs {
            capacity,
            arcs: vec![Arc::default(); capacity],
            live: 0,
            spawned: 0,
            vertex: 0,
            index: 0,
            positions: vec![0.0; vertices * 3],
            tangents: vec![0.0; vertices * 3],
            meta: vec![0.0; vertices * 4],
            indices: vec![0; quads * 6],
            points: [Vector3::default(); SEGMENTS + 1],
            branch_points: [Vector3::default(); BRANCH_SEGMENTS + 1],
            tangent: Vector3::default(),
        }
    }

    /// An arc for `life` seconds (Arcs.add); false when every arc is busy.
    #[allow(clippy::too_many_arguments)]
    // An arc takes the first free slot.
    fn add(&mut self, from: Anchor, a: Vector3, to: Anchor, b: Vector3, life: f64, width: f64, intensity: f64, seed: u32) -> bool {
        if self.live >= self.capacity {
            return false;
        }
        let Some(arc) = self.arcs.iter_mut().find(|arc| !arc.live) else { return false };
        *arc = Arc { live: true, age: 0.0, life, width, intensity, seed, from, to, from_point: a, to_point: b };
        self.live += 1;
        self.spawned += 1;
        true
    }

    /// An arc end in the world (Anchor.resolve).
    fn resolve(point: Vector3, anchor: &Anchor) -> Vector3 {
        let mut out = point;
        if anchor.kind != AnchorKind::World {
            let m = Matrix4 { elements: anchor.matrix };
            out.apply_matrix4(&m);
        }
        out
    }

    /// Hands every live arc's carried ends to `refresh`, which brings their
    /// matrices up to date.
    pub fn refresh(&mut self, refresh: &mut dyn FnMut(&mut Anchor)) {
        for arc in self.arcs.iter_mut().filter(|arc| arc.live) {
            if arc.from.kind != AnchorKind::World {
                refresh(&mut arc.from);
            }
            if arc.to.kind != AnchorKind::World {
                refresh(&mut arc.to);
            }
        }
    }

    /// How many live arcs hang from a bone of this slot.
    pub fn bone_refs(&self, slot: u32) -> usize {
        self.arcs.iter().filter(|arc| arc.live).map(|arc| [arc.from, arc.to].iter().filter(|a| a.kind == AnchorKind::Bone && a.a == slot).count()).sum()
    }

    /// Redraws every live arc's ribbons and retires the spent (Arcs.update).
    // Redraws every live arc's ribbons (a main strand and up to two branches off
    // its inside points, forking away and fading) and retires the spent.
    fn update(&mut self, dt: f64, time: f64) {
        self.vertex = 0;
        self.index = 0;
        let frame = (time * FLICKER).floor() as i32;
        for k in 0..self.capacity {
            if !self.arcs[k].live {
                continue;
            }
            self.arcs[k].age += dt;
            let arc = self.arcs[k];
            if arc.age >= arc.life {
                self.arcs[k].live = false;
                self.live -= 1;
                continue;
            }
            let from = Self::resolve(arc.from_point, &arc.from);
            let to = Self::resolve(arc.to_point, &arc.to);
            let length = from.distance_to(&to);
            if length < 0.02 {
                continue;
            }
            let fade = f64::min(1.0, 3.0 * (1.0 - arc.age / arc.life)) * (0.6 + 0.4 * jitter(arc.seed, frame + 11));
            let mut dir = Vector3::default();
            dir.sub_vectors(&to, &from).divide_scalar(length);
            let mut u = Vector3::new(dir.y, dir.z, -dir.x);
            u.cross(&dir).normalize();
            let mut w = Vector3::default();
            w.cross_vectors(&dir, &u);
            let jag = length * 0.16;
            for i in 0..=SEGMENTS {
                let s = i as f64 / SEGMENTS as f64;
                let envelope = v8math::sin(std::f64::consts::PI * s);
                let ju = (jitter(arc.seed, frame * 97 + i as i32 * 2) - 0.5) * 2.0 * jag * envelope;
                let jw = (jitter(arc.seed, frame * 97 + i as i32 * 2 + 1) - 0.5) * 2.0 * jag * envelope;
                self.points[i].lerp_vectors(&from, &to, s).add_scaled_vector(&u, ju).add_scaled_vector(&w, jw);
            }
            let points = self.points;
            self.strip(&points, SEGMENTS, arc.width, arc.intensity * fade);
            // A branch or two off the inside points, forking away and fading.
            for b in 0..BRANCHES as i32 {
                if jitter(arc.seed, frame * 31 + 50 + b) > 0.55 {
                    continue;
                }
                let at = 2 + (jitter(arc.seed, frame * 31 + 60 + b) * (SEGMENTS - 4) as f64).floor() as usize;
                let reach = length * (0.2 + 0.25 * jitter(arc.seed, frame * 31 + 70 + b));
                self.tangent.sub_vectors(&points[at + 1], &points[at - 1]).normalize();
                self.tangent.add_scaled_vector(&u, (jitter(arc.seed, b * 7 + 80) - 0.5) * 2.4);
                self.tangent.add_scaled_vector(&w, (jitter(arc.seed, b * 7 + 81) - 0.5) * 2.4);
                self.tangent.normalize();
                let tangent = self.tangent;
                for i in 0..=BRANCH_SEGMENTS {
                    let s = i as f64 / BRANCH_SEGMENTS as f64;
                    self.branch_points[i].copy(&points[at]).add_scaled_vector(&tangent, reach * s);
                    self.branch_points[i].add_scaled_vector(&u, (jitter(arc.seed, frame * 53 + b * 13 + i as i32) - 0.5) * reach * 0.35 * s);
                    self.branch_points[i].add_scaled_vector(&w, (jitter(arc.seed, frame * 59 + b * 13 + i as i32) - 0.5) * reach * 0.35 * s);
                }
                let branch = self.branch_points;
                self.strip(&branch, BRANCH_SEGMENTS, arc.width * 0.6, arc.intensity * fade * 0.7);
            }
        }
    }

    /// One strand of points as a ribbon: two vertices a point, a quad a
    /// segment (Arcs.strip).
    // One strand of points as a ribbon: two vertices a point (a side each), a quad
    // a segment.
    fn strip(&mut self, points: &[Vector3], segments: usize, width: f64, intensity: f64) {
        let base = self.vertex;
        let mut vertex = self.vertex;
        for i in 0..=segments {
            let p = points[i];
            let before = points[i.saturating_sub(1)];
            let after = points[usize::min(i + 1, segments)];
            self.tangent.sub_vectors(&after, &before);
            for side in [-1.0f32, 1.0] {
                let v = vertex;
                vertex += 1;
                self.positions[v * 3] = p.x as f32;
                self.positions[v * 3 + 1] = p.y as f32;
                self.positions[v * 3 + 2] = p.z as f32;
                self.tangents[v * 3] = self.tangent.x as f32;
                self.tangents[v * 3 + 1] = self.tangent.y as f32;
                self.tangents[v * 3 + 2] = self.tangent.z as f32;
                self.meta[v * 4] = side;
                self.meta[v * 4 + 1] = (i as f64 / segments as f64) as f32;
                self.meta[v * 4 + 2] = width as f32;
                self.meta[v * 4 + 3] = intensity as f32;
            }
        }
        let mut index = self.index;
        for i in 0..segments {
            let a = (base + i * 2) as u16;
            self.indices[index..index + 6].copy_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
            index += 6;
        }
        self.vertex = vertex;
        self.index = index;
    }

    fn clear(&mut self) {
        for arc in self.arcs.iter_mut() {
            arc.live = false;
        }
        self.live = 0;
        self.vertex = 0;
        self.index = 0;
    }
}

/// How a smoke particle looks besides its motion (smokeParticle's options).
#[derive(Clone, Copy)]
pub struct Smoke {
    pub tone: f64,
    pub heat: f64,
    pub wind: f64,
    pub spin: f64,
    pub floor: f64,
    pub delay: f64,
}

impl Default for Smoke {
    fn default() -> Smoke {
        Smoke { tone: 0.3, heat: 0.0, wind: 1.0, spin: 0.0, floor: -1e4, delay: 0.0 }
    }
}

/// The pools and arcs together (DestructionFx).
pub struct Fx {
    pub time: f64,
    pub hot: Pool,
    pub smoke: Pool,
    pub arcs: Arcs,
}

impl Fx {
    pub fn new(hot: usize, smoke: usize, arcs: usize) -> Fx {
        Fx { time: 0.0, hot: Pool::new(hot, limits::HOT_ATTRIBUTES), smoke: Pool::new(smoke, limits::SMOKE_ATTRIBUTES), arcs: Arcs::new(arcs) }
    }

    /// A birth in the hot pool (hotParticle). style: SPARK, FIRE, FLASH, GLOW
    /// or EMBER. size0, size1: its radius at birth and death (a spark's half
    /// width); pull: gravity's share (negative rises); extra: the floor's
    /// height for sparks and glows, or a flash's tint (0 fire, 1 electric).
    /// A full pool drops it (and the random number its seed would have taken
    /// is not drawn).
    #[allow(clippy::too_many_arguments)]
    // A birth in the hot (additive) pool: sparks, fire, flashes, floor glow and
    // embers.
    pub fn hot_particle(&mut self, rng: &mut Random, style: f64, p: &Vector3, v: &Vector3, life: f64, size0: f64, size1: f64, pull: f64, drag: f64, intensity: f64, extra: f64, delay: f64) {
        let Some(i) = self.hot.take() else { return };
        self.hot.set(0, i, p.x, p.y, p.z, self.time + delay);
        self.hot.set(1, i, v.x, v.y, v.z, life);
        self.hot.set(2, i, size0, size1, pull, drag);
        let seed = rng.next();
        self.hot.set(3, i, style, seed, intensity, extra);
    }

    /// A birth in the smoke pool (smokeParticle). style: SMOKE, CHIP or
    /// SCORCH. tone: 0 oily black .. 1 pale grey; heat: fire light on its
    /// underside while young; wind: its share of the wind; floor: the
    /// height chips skip off and scorch marks lie on.
    #[allow(clippy::too_many_arguments)]
    // A birth in the smoke pool: lit smoke, flakes of plating and scorch marks.
    pub fn smoke_particle(&mut self, rng: &mut Random, style: f64, p: &Vector3, v: &Vector3, life: f64, size0: f64, size1: f64, pull: f64, drag: f64, opacity: f64, look: Smoke) {
        let Some(i) = self.smoke.take() else { return };
        self.smoke.set(0, i, p.x, p.y, p.z, self.time + look.delay);
        self.smoke.set(1, i, v.x, v.y, v.z, life);
        self.smoke.set(2, i, size0, size1, pull, drag);
        let seed = rng.next();
        self.smoke.set(3, i, style, seed, opacity, look.floor);
        self.smoke.set(4, i, look.tone, look.heat, look.wind, look.spin);
    }

    /// An arc from a to b for `life` seconds; false when every arc is busy
    /// (arc).
    #[allow(clippy::too_many_arguments)]
    // An arc from a to b for `life` seconds.
    pub fn arc(&mut self, rng: &mut Random, from: Anchor, a: &Vector3, to: Anchor, b: &Vector3, life: f64, width: f64, intensity: f64) -> bool {
        if self.arcs.live >= self.arcs.capacity {
            return false;
        }
        let seed = (rng.next() * 4294967295.0) as u32;
        self.arcs.add(from, *a, to, *b, life, width, intensity, seed)
    }

    /// How much of a pool is still free, past a reserve kept for blasts and
    /// wrecks: 1 with room to spare, falling to 0 as it fills (room).
    // How much of a pool is free past a reserve kept for blasts and wrecks: 1 with
    // room to spare, falling to 0 as it fills.
    pub fn room(pool: &Pool) -> f64 {
        three::clamp((1.0 - pool.count as f64 / pool.capacity as f64 - 0.25) / 0.35, 0.0, 1.0)
    }

    /// Advances the clock and retires the dead (DestructionFx.update); the
    /// result block takes the counts and the first changed records, which
    /// the page's upload bookkeeping reads.
    // Advances the clock and retires the dead.
    pub fn update(&mut self, dt: f64, result: &mut [f64]) {
        self.time += dt;
        if self.time > REBASE {
            self.time -= REBASE;
            self.hot.rebase(REBASE);
            self.smoke.rebase(REBASE);
        }
        self.hot.retire(self.time);
        self.smoke.retire(self.time);
        self.arcs.update(dt, self.time);
        self.report(result);
    }

    /// The counts and the first changed records, for the page (and the page's
    /// upload bookkeeping): what `update` leaves, and what a frame that did
    /// not step the effects (they were cleared) leaves too.
    pub fn report(&mut self, result: &mut [f64]) {
        let dirty = |d: usize| if d == usize::MAX { -1.0 } else { d as f64 };
        result[rs::TIME] = self.time;
        result[rs::HOT_COUNT] = self.hot.count as f64;
        result[rs::HOT_DIRTY] = dirty(self.hot.dirty);
        result[rs::SMOKE_COUNT] = self.smoke.count as f64;
        result[rs::SMOKE_DIRTY] = dirty(self.smoke.dirty);
        result[rs::ARCS_LIVE] = self.arcs.live as f64;
        result[rs::ARC_VERTICES] = self.arcs.vertex as f64;
        result[rs::ARC_INDICES] = self.arcs.index as f64;
        result[rs::HOT_EMITTED] = self.hot.emitted as f64;
        result[rs::HOT_DROPPED] = self.hot.dropped as f64;
        result[rs::SMOKE_EMITTED] = self.smoke.emitted as f64;
        result[rs::SMOKE_DROPPED] = self.smoke.dropped as f64;
        result[rs::ARCS_SPAWNED] = self.arcs.spawned as f64;
        self.hot.dirty = usize::MAX;
        self.smoke.dirty = usize::MAX;
    }

    pub fn clear(&mut self) {
        self.hot.clear();
        self.smoke.clear();
        self.arcs.clear();
    }
}
