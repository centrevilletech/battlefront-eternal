//! What the hull phase asks of the battle, answered in the module itself
//! where it can be (rust/sim/src/vehicles/module.rs): the collision world's
//! queries from the module's own copy of it (rust/sim/src/collision, which
//! JavaScript's CollisionWorld fills and queries through src/ray-kernel.js),
//! and the map's water and death regions from the map's own data, built as
//! the content crate builds them. Each answer is the one the JavaScript the
//! hulls called (src/vehicles-world.js) gives, to the last bit, because it
//! runs the same code: the kernel functions collision.js's ray paths call,
//! in the order they call them, over the same inputs.
//!
//! - `Collision`: the hulls' casts, wall pushes, ground columns and spring
//!   gathers, their bodies' places and enabled flags. JavaScript gives each
//!   body's enabled flag (a plain field there) by its place in the world's
//!   order before the phase and after any hook, and takes back the flags and
//!   places the hulls changed (`take_changes`) before a hook and after the
//!   phase, so either side always sees what the other did.
//! - `MapRules`: the water (src/runtime/water-state.js over; its level, which
//!   bobs with the battle's clock, comes in with each phase) and the death
//!   regions (game.js inDeathRegion over src/regions.js inRegion).

use crate::gen::{Body, F2BodyHeader, F2BodyToggle, F2BodyPlaced};
use crate::store::Ref;
use super::{BodyId, Ground, Hit};
use crate::collision::spheres::{self, box_apply_matrix4, Bounds, GatherCache, Gathered, Probes, Pushed};
use crate::collision::{self, broad, State, MAX_BODIES, MISS, TERRAIN};
use bfe_content::regions::BuiltRegion;
use bfe_content::value::Value;
use std::collections::HashSet;
use three::{Matrix4, Quaternion, Vector3};
use v8math::{abs, floor, hypot, max, min};

/// A hull's body in the kernel: its slot there, its bounds in its own frame
/// (`body.localBounds`), and the kernel's gather caches of its wall probes
/// and of its springs (-1 for a probe without one).
#[derive(Clone, Debug)]
pub struct Collider {
    pub kernel: u32,
    pub local_bounds: Bounds,
    pub walls: Vec<i64>,
    pub springs: Vec<i64>,
}

/// CollisionWorld.ground's one exact column it remembers (lastGround), kept
/// for the hulls' own queries as the soldiers' movement keeps one for
/// theirs (rust/sim/src/movement/scene.rs): the terrain never changes, so
/// a remembered column's answer is the one a new query would give.
struct GroundCache {
    x: f64,
    z: f64,
    top: f64,
    hit: Option<(f64, [f64; 3])>,
}

impl Default for GroundCache {
    fn default() -> GroundCache {
        GroundCache { x: f64::NAN, z: f64::NAN, top: f64::NAN, hit: None }
    }
}

// Vehicles and infantry can own one coordinated phase. Their enabled bytes
// must then be the same storage: a hull death disables its body before the
// next instant round, and a prop show changes what a later hull cast meets.
struct BodyFlags {
    owned: Vec<u8>,
    shared: *mut u8,
}
impl std::ops::Deref for BodyFlags {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        if self.shared.is_null() { &self.owned }
        else { unsafe { std::slice::from_raw_parts(self.shared, MAX_BODIES) } }
    }
}
impl std::ops::DerefMut for BodyFlags {
    fn deref_mut(&mut self) -> &mut [u8] {
        if self.shared.is_null() { &mut self.owned }
        else { unsafe { std::slice::from_raw_parts_mut(self.shared, MAX_BODIES) } }
    }
}

/// The hulls' view of the module's collision world.
pub struct Collision {
    /// The kernel's world (collision.js rayWorld) and the vehicle-only
    /// barriers' fixed set (rays.fixedSet of blockerSet), if any.
    pub world: usize,
    pub blockers: Option<usize>,
    /// Each body's enabled flag by its place in the world's order: written
    /// by JavaScript (`flags_ptr`), changed by the hulls.
    flags: BodyFlags,
    /// Each kernel slot's place in the world's order (-1: not in it), for
    /// the order's version `places_for`.
    place_of: Vec<i32>,
    places_for: Option<u64>,
    /// The bodies' numbers both ways: the store's (BodyId, the bridge's
    /// body slots) and the kernel's.
    kernel_of: Vec<i32>,
    ref_of: Vec<i32>,
    /// Qualified identity is shared by the unit and vehicle collision owners.
    bodies: Vec<Option<(Ref<Body>, u32)>>,
    /// The hulls' bodies, by BodyId.
    colliders: Vec<Option<Collider>>,
    ground: GroundCache,
    /// Kernel slots whose flag or place the hulls changed since JavaScript
    /// last took them (marks by slot).
    toggled: Vec<u32>,
    placed: Vec<u32>,
    marks: Vec<u8>,
    gathered: Gathered,
    prepared: Option<u64>,
    near: Vec<u32>,
    // Each query context owns its cast order and eligibility scratch.
    candidates: Vec<u32>,
    enabled: Vec<u8>,
    points: Vec<[f64; 3]>,
}

const TOGGLED: u8 = 1;
const PLACED: u8 = 2;

impl Collision {
    /// predictionCollision shares immutable collider/cache handles, but owns
    /// enabled flags and query scratch. A private hull never publishes changes.
    /// Construction is cold; ordinary frames only refresh flags in this view.
    pub fn prediction_view(&self) -> Self {
        let mut view = Self::new(self.world, self.blockers);
        view.kernel_of.clone_from(&self.kernel_of);
        view.ref_of.clone_from(&self.ref_of);
        view.bodies.clone_from(&self.bodies);
        view.colliders.clone_from(&self.colliders);
        view
    }

    /// Import the page's body order once, before any pending ticks replay.
    /// Query-local toggles remain private even when a replay exits early.
    pub fn prediction_flags(&mut self, flags: &[f64]) -> bool {
        if flags.len() > self.flags.len() { return false; }
        for (out, &flag) in self.flags.iter_mut().zip(flags) {
            if flag != 0.0 && flag != 1.0 { return false; }
            *out = flag as u8;
        }
        true
    }

    /// New replicas may add opaque bodies after a pilot's session was born.
    /// Refresh names without copying authoritative transforms or body flags.
    pub fn prediction_names(&mut self, source: &Self) {
        if self.world == source.world {
            self.kernel_of.clone_from(&source.kernel_of);
            self.ref_of.clone_from(&source.ref_of);
            self.bodies.clone_from(&source.bodies);
        }
    }
    pub fn new(world: usize, blockers: Option<usize>) -> Collision {
        Collision {
            world,
            blockers,
            flags: BodyFlags { owned:vec![0; MAX_BODIES], shared:std::ptr::null_mut() },
            place_of: Vec::new(),
            places_for: None,
            kernel_of: Vec::new(),
            ref_of: Vec::new(),
            bodies: Vec::new(),
            colliders: Vec::new(),
            ground: GroundCache::default(),
            toggled: Vec::new(),
            placed: Vec::new(),
            marks: Vec::new(),
            gathered: Gathered::new(),
            prepared: None,
            near: Vec::new(),
            candidates: vec![0; MAX_BODIES],
            enabled: vec![0; MAX_BODIES],
            points: Vec::new(),
        }
    }

    /// Where JavaScript writes the bodies' flags (MAX_BODIES bytes, by place).
    pub fn flags_ptr(&mut self) -> *mut u8 {
        self.flags.as_mut_ptr()
    }
    /// Bind the battle's fixed enabled-body reservation at construction.
    /// The caller keeps MAX_BODIES bytes alive until this Collision is dropped.
    #[cfg(target_arch = "wasm32")]
    pub(crate) unsafe fn share_flags(&mut self, flags: *mut u8) { self.flags.shared = flags; }

    /// Body `body` (the store's number) is kernel slot `kernel`.
    pub fn name_body(&mut self, body: u32, kernel: u32) {
        let (b, k) = (body as usize, kernel as usize);
        if self.kernel_of.len() <= b {
            self.kernel_of.resize(b + 1, -1);
        }
        if self.ref_of.len() <= k {
            self.ref_of.resize(k + 1, -1);
        }
        // Rebinding either namespace retires the old edge. A late body must
        // never inherit an earlier body's generation through a reused slot.
        let old_kernel = self.kernel_of[b];
        if old_kernel >= 0 && old_kernel != kernel as i32 {
            self.ref_of[old_kernel as usize] = -1;
        }
        let old_body = self.ref_of[k];
        if old_body >= 0 && old_body != body as i32 {
            self.kernel_of[old_body as usize] = -1;
        }
        self.kernel_of[b] = kernel as i32;
        self.ref_of[k] = body as i32;
    }

    /// CollisionWorld.rayBodies binds both identities at construction, even
    /// for variants admitted after vehicle topology was prepared.
    pub fn bind_body(&mut self, body: Ref<Body>, kernel: u32) -> bool {
        if body.slot as usize >= MAX_BODIES || kernel as usize >= MAX_BODIES { return false; }
        if self.bodies.get(body.slot as usize).copied().flatten().is_some_and(|(old,_)|
            old.gen != body.gen && body.gen.wrapping_sub(old.gen) >= 1 << 31) { return false; }
        self.name_body(body.slot, kernel);
        let slot = body.slot as usize;
        if self.bodies.len() <= slot { self.bodies.resize(slot + 1, None); }
        self.bodies[slot] = Some((body, kernel));
        true
    }

    /// Seat casts must reject stale identities, including a store slot reused
    /// before its replacement has been named. Missing bodies are not terrain.
    pub fn body_reference(&self, slot: u32) -> Option<Ref<Body>> {
        let (body, bound_kernel) = self.bodies.get(slot as usize).copied().flatten()?;
        let kernel = *self.kernel_of.get(slot as usize)?;
        (kernel >= 0 && kernel as u32 == bound_kernel && self.ref_of.get(kernel as usize) == Some(&(slot as i32))).then_some(body)
    }

    /// Kernel hits and public body slots use the same qualified bijection.
    pub fn kernel_reference(&self, kernel: usize) -> Option<Ref<Body>> {
        let slot = *self.ref_of.get(kernel)?;
        if slot < 0 || self.kernel_of.get(slot as usize).copied()!=Some(kernel as i32) { return None; }
        self.body_reference(slot as u32)
    }

    /// A hull's body: its kernel slot (named with it), bounds and caches.
    pub fn add_collider(&mut self, body: BodyId, collider: Collider) {
        self.name_body(body, collider.kernel);
        let b = body as usize;
        if self.colliders.len() <= b {
            self.colliders.resize(b + 1, None);
        }
        self.colliders[b] = Some(collider);
    }

    /// CollisionWorld construction sizes native wall-query storage from the
    /// loaded level. A new part of the map must not allocate on first contact.
    pub fn prepare(&mut self, s: &State) {
        let w = &s.worlds[self.world];
        if self.prepared != Some(w.version) {
            self.gathered.prepare(s,self.world,self.blockers);
            let n = s.bodies.len();
            self.near.reserve(self.gathered.capacity().saturating_sub(self.near.len()));
            self.placed.reserve(n.saturating_sub(self.placed.len()));
            self.toggled.reserve(n.saturating_sub(self.toggled.len()));
            self.place_of.reserve(n.saturating_sub(self.place_of.len()));
            self.marks.reserve(n.saturating_sub(self.marks.len()));
            self.prepared = Some(w.version);
        }
        let statics = w.statics.map_or(0, |set| s.sets[set].polygons.len());
        let blockers = self.blockers.map_or(0, |set| s.sets[set].polygons.len());
        for c in self.colliders.iter().flatten() {
            self.points.reserve(c.walls.len().saturating_sub(self.points.len()));
            for &id in c.walls.iter().chain(&c.springs) {
                if id >= 0 { spheres::caches()[id as usize].reserve(statics,blockers,w.order.len() + w.prepared.len()); }
            }
        }
    }

    pub fn collider(&self, body: BodyId) -> Option<&Collider> {
        self.colliders.get(body as usize).and_then(Option::as_ref)
    }

    /// Body `body`'s kernel slot, if JavaScript named it (the units'
    /// phase: the body a soldier falls through, src/units-sim.js).
    pub fn kernel_slot(&self, body: BodyId) -> Option<usize> {
        self.kernel(body)
    }

    fn kernel(&self, body: BodyId) -> Option<usize> {
        match self.kernel_of.get(body as usize) {
            Some(&k) if k >= 0 => Some(k as usize),
            _ => None,
        }
    }

    /// The place of kernel slot `k` in the world's order now, if it is in it.
    fn place(&mut self, s: &State, k: usize) -> Option<usize> {
        let w = &s.worlds[self.world];
        if self.places_for != Some(w.version) {
            self.place_of.clear();
            self.place_of.resize(s.bodies.len(), -1);
            for (place, &slot) in w.order.iter().enumerate() {
                self.place_of[slot as usize] = place as i32;
            }
            self.places_for = Some(w.version);
        }
        match self.place_of.get(k) {
            Some(&p) if p >= 0 => Some(p as usize),
            _ => None,
        }
    }

    fn mark(&mut self, k: usize, what: u8) {
        if self.marks.len() <= k {
            self.marks.resize(k + 1, 0);
        }
        if self.marks[k] & what == 0 {
            self.marks[k] |= what;
            if what == TOGGLED { self.toggled.push(k as u32) } else { self.placed.push(k as u32) }
        }
    }

    /// The flags and places the hulls changed since the last call, into
    /// `out`: how many bodies were toggled and how many placed, then each
    /// toggled body's kernel slot and flag, then each placed body's slot,
    /// bounds (6), matrix (16) and inverse (16). The marks are cleared.
    pub fn take_changes(&mut self, s: &mut State, out: &mut Vec<f64>) {
        out.clear();
        let (toggled, placed) = (self.toggled.len(), self.placed.len());
        let mut heap = Vec::new();
        out.resize(F2BodyHeader::WORDS,0.0);
        F2BodyHeader { toggled: toggled as u32, placed: placed as u32 }.put(out,&mut heap);
        for i in 0..toggled {
            let k = self.toggled[i] as usize;
            let enabled = self.place(s,k).is_some_and(|p| self.flags[p] != 0);
            let at = out.len(); out.resize(at + F2BodyToggle::WORDS,0.0);
            F2BodyToggle { kernel: k as u32, enabled }.put(&mut out[at..],&mut heap);
        }
        for &k in &self.placed {
            let b = &s.bodies[k as usize];
            let at = out.len(); out.resize(at + F2BodyPlaced::WORDS,0.0);
            F2BodyPlaced { kernel: k, min: [b.bounds[0],b.bounds[1],b.bounds[2]], max: [b.bounds[3],b.bounds[4],b.bounds[5]],
                matrix: b.matrix, inverse: b.inverse }.put(&mut out[at..],&mut heap);
        }
        for &k in self.toggled.iter().chain(&self.placed) {
            self.marks[k as usize] = 0;
        }
        self.toggled.clear();
        self.placed.clear();
    }

    /// CollisionWorld.overlapsSphere for the Classic exit search. Use the
    /// same enabled order as casts; a candidate must see earlier crew exits.
    pub fn overlaps_sphere(&mut self, s: &mut State, center: &Vector3, radius: f64, ignore: Option<BodyId>) -> bool {
        let ignore = ignore.map_or(-1, |b| self.kernel(b).expect("a named seat body") as i64);
        spheres::overlaps_sphere(s, self.world, [center.x, center.y, center.z], radius, ignore, &self.flags)
    }

    /// CollisionWorld.cast(origin, direction, far, out, ignoreBody) as its
    /// ray path runs it (rayCast): the candidates through the broad phase,
    /// each one's enabled flag, then the cast.
    pub fn cast(&mut self, s: &mut State, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Option<Hit>> {
        let ignore = match ignore {
            Some(b) => self.kernel(b)? as i64,
            None => -1,
        };
        let ray = collision::Ray { ox: origin.x, oy: origin.y, oz: origin.z, dx: direction.x, dy: direction.y, dz: direction.z };
        let count = broad::world_candidates_near_into(s, self.world, &ray, far, ignore, false, false, &mut self.candidates);
        // The candidates' flags, as rayCandidates writes them.
        for (c, flag) in self.enabled.iter_mut().enumerate().take(count) {
            *flag = self.flags[self.candidates[c] as usize];
        }
        let (code, hit) = collision::world_cast_ray_in(s, self.world, &ray, far, hypot(direction.x, direction.z), &self.candidates[..count], &self.enabled[..count]);
        if code == MISS {
            return Some(None);
        }
        let body = if code >= 0 {
            match self.ref_of.get(code as usize) {
                Some(&r) if r >= 0 => Some(r as BodyId),
                // A body JavaScript has not named: the import names it.
                _ => return None,
            }
        } else {
            None
        };
        Some(Some(Hit {
            distance: hit.distance,
            point: Vector3::new(hit.px, hit.py, hit.pz),
            normal: Vector3::new(hit.nx, hit.ny, hit.nz),
            terrain: code == TERRAIN,
            body,
        }))
    }

    /// VehiclesWorld.pushWalls over the kernel (collision.js rayPushWalls):
    /// the hull's probes through its own caches, the barriers, the bodies'
    /// flags now. None: the hull's body is not one this knows.
    #[allow(clippy::too_many_arguments)]
    pub fn push_walls(&mut self, s: &mut State, body: BodyId, probes: &[Vector3], radius: f64, q: &Quaternion, own: Option<&Quaternion>, previous: &Vector3, position: &mut Vector3, velocity: &mut Vector3, terrain: bool) -> Option<bool> {
        let walls = &self.colliders.get(body as usize)?.as_ref()?.walls;
        if walls.len() < probes.len() {
            return None;
        }
        // Uncached probes need only this query's scratch. Acquire the facade
        // cache bank only when a collider actually names a retained cache.
        let mut base: *mut GatherCache = std::ptr::null_mut();
        // Each probe's own cache (distinct per probe, as a hull keeps them).
        let mut caches: [Option<&mut GatherCache>; 256] = [const { None }; 256];
        for (i, &id) in walls.iter().take(probes.len()).enumerate() {
            if id >= 0 {
                if base.is_null() { base = spheres::caches().as_mut_ptr(); }
                caches[i] = Some(unsafe { &mut *base.add(id as usize) });
            }
        }
        self.points.clear();
        self.points.extend(probes.iter().map(|p| [p.x, p.y, p.z]));
        let p = Probes { points: &self.points, radius, q: [q.x, q.y, q.z, q.w], own: own.map(|o| [o.x, o.y, o.z, o.w]), previous: [previous.x, previous.y, previous.z], terrain };
        let mut h = Pushed { position: [position.x, position.y, position.z], velocity: [velocity.x, velocity.y, velocity.z], undo_turn: false };
        let n = s.worlds[self.world].order.len();
        spheres::push_walls(s, self.world, &p, &mut caches[..probes.len()], self.blockers, &self.flags[..n], &mut h, &mut self.gathered, &mut self.near);
        position.set(h.position[0], h.position[1], h.position[2]);
        velocity.set(h.velocity[0], h.velocity[1], h.velocity[2]);
        Some(h.undo_turn)
    }

    /// CollisionWorld.ground(x, z, top) (rayGroundHit through the column
    /// it remembers).
    pub fn ground(&mut self, s: &mut State, x: f64, z: f64, top: f64) -> Option<Ground> {
        s.worlds[self.world].grid?;
        let cached = &mut self.ground;
        if !(cached.x == x && cached.z == z && cached.top == top) {
            cached.x = x;
            cached.z = z;
            cached.top = top;
            cached.hit = spheres::ground(s, self.world, x, z, top);
        }
        cached.hit.map(|(height, n)| Ground { height, normal: Vector3::new(n[0], n[1], n[2]) })
    }

    /// The polygons near a hover's spring `index` (CollisionWorld.gather
    /// through the spring's cache, with the barriers: rayGather), into
    /// `out`. None: the hull's body is not one this knows.
    pub fn spring_polygons(&mut self, s: &mut State, body: BodyId, index: usize, center: &Vector3, radius: f64) -> Option<&[spheres::Polygon]> {
        let id = *self.collider(body)?.springs.get(index).unwrap_or(&-1);
        let cache = if id >= 0 { Some(unsafe { &mut *spheres::caches().as_mut_ptr().add(id as usize) }) } else { None };
        let n = s.worlds[self.world].order.len();
        spheres::gather(s, self.world, [center.x, center.y, center.z], radius, self.blockers, cache, -1, &self.flags[..n], &mut self.gathered);
        // CollisionWorld.gather already retains every triangle. A second
        // owned copy grows on first contact with each new part of the map.
        Some(self.gathered.list())
    }

    /// CollisionWorld.moveBody(body, matrix): its matrix, inverse and bounds,
    /// placed in the kernel (rays.place, body_set). None: not a hull's body
    /// this knows.
    pub fn move_body(&mut self, s: &mut State, body: BodyId, matrix: &Matrix4) -> Option<()> {
        let c = self.collider(body)?;
        let (k, local) = (c.kernel as usize, c.local_bounds);
        let mut inverse = *matrix;
        inverse.invert();
        let bounds = box_apply_matrix4(&local, &matrix.elements);
        let fixed = {
            let b = &mut s.bodies[k];
            b.bounds = bounds;
            if b.moving {
                b.matrix = matrix.elements;
                b.inverse = inverse.elements;
                false
            } else {
                true
            }
        };
        if fixed {
            s.fixed_version += 1;
        }
        self.mark(k, PLACED);
        Some(())
    }

    /// `body.enabled = enabled` for a body in the world's order. None: a
    /// body this cannot place.
    pub fn enable_body(&mut self, s: &State, body: BodyId, enabled: bool) -> Option<()> {
        let k = self.kernel(body)?;
        let p = self.place(s, k)?;
        self.flags[p] = enabled as u8;
        self.mark(k, TOGGLED);
        Some(())
    }

    /// `body.enabled` for a body in the world's order.
    pub fn body_enabled(&mut self, s: &State, body: BodyId) -> Option<bool> {
        let k = self.kernel(body)?;
        let p = self.place(s, k)?;
        Some(self.flags[p] != 0)
    }
}

/// The map's water as a world's hulls meet it (src/runtime/water-state.js).
#[derive(Clone)]
pub struct Water {
    cell: f64,
    offset: [f64; 2],
    grid: f64,
    /// Its cells' numbers (`j * grid + i`), as the Set holds them.
    cells: HashSet<u64>,
}

/// A number as a Set compares it (SameValueZero): -0 is 0.
fn set_key(x: f64) -> u64 {
    if x == 0.0 {
        0
    } else if x.is_nan() {
        f64::NAN.to_bits()
    } else {
        x.to_bits()
    }
}

impl Water {
    /// WaterState.over(x, z): whether (x, z) is over a water cell (off the
    /// grid, the nearest edge cell).
    pub fn over(&self, x: f64, z: f64) -> bool {
        let n = self.grid;
        let i = min(max(floor((x + self.offset[0]) / self.cell), 0.0), n - 1.0);
        let j = min(max(floor((z + self.offset[1]) / self.cell), 0.0), n - 1.0);
        self.cells.contains(&set_key(j * n + i))
    }
}

/// What the hulls ask of the map: its water and its death regions.
#[derive(Clone)]
pub struct MapRules {
    pub water: Option<Water>,
    /// The water's level this phase (`water.level`: it bobs).
    pub level: f64,
    pub kill_y: f64,
    pub regions: Vec<BuiltRegion>,
}

fn number(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(f64::NAN)
}

impl MapRules {
    /// From the map's data as JSON: `water` (cell, offset, grid and the
    /// WaterState's cell numbers, or null) and the death regions as game.js
    /// picks them (`deathRegions`, `mission.deathRegions`); and the kill
    /// height under the battlefield.
    pub fn from_json(map: &Value, kill_y: f64) -> MapRules {
        let water = map.get("water").filter(|w| w.truthy()).map(|w| {
            let offset = w.get("offset");
            Water {
                cell: number(w.get("cell")),
                offset: [number(offset.and_then(|o| o.at(0))), number(offset.and_then(|o| o.at(1)))],
                grid: number(w.get("grid")),
                cells: w.get("cells").and_then(Value::as_array).unwrap_or(&[]).iter().map(|c| set_key(number(Some(c)))).collect(),
            }
        });
        let regions = bfe_content::regions::death_regions(map).into_iter().map(|(_, built)| built).collect();
        MapRules { water, level: f64::NAN, kill_y, regions }
    }

    /// game.js inDeathRegion(p).
    pub fn in_death_region(&self, p: &Vector3) -> bool {
        p.y < self.kill_y || self.regions.iter().any(|r| in_region(r, p))
    }
}

/// src/regions.js inRegion(built, point).
pub fn in_region(built: &BuiltRegion, point: &Vector3) -> bool {
    let kind = built.kind.as_ref().and_then(Value::as_str);
    if kind == Some("sphere") {
        let mut d = *point;
        d.sub(&Vector3::new(built.center.x, built.center.y, built.center.z));
        return d.length() < built.radius;
    }
    let local = collision::apply_matrix4(&built.inverse.elements, [point.x, point.y, point.z]);
    // `const [a, b, c] = built.half`: undefined (NaN in a comparison) past its end.
    let half = |i: usize| built.half.get(i).copied().unwrap_or(f64::NAN);
    match kind {
        Some("cylinder") => hypot(local[0], local[2]) < built.radius && abs(local[1]) < half(1),
        Some("box") => abs(local[0]) < half(0) && abs(local[1]) < half(1) && abs(local[2]) < half(2),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(json: &str, kill_y: f64) -> MapRules {
        MapRules::from_json(&bfe_content::json::parse(json).unwrap(), kill_y)
    }

    #[test]
    fn late_collision_body_names_share_the_seat_cast_bank() {
        let mut collision = Collision::new(0, None);
        let hull = Ref::new(2, 1);
        assert!(collision.bind_body(hull, 7));
        // Late CollisionWorld bodies updated only the numeric mapping,
        // leaving the seat world's separate reference vector at length three.
        let late = Ref::new(4096, 3);
        assert!(collision.bind_body(late, 19));
        assert_eq!(collision.body_reference(late.slot), Some(late));
        assert_eq!(collision.body_reference(u32::MAX), None);
        assert!(!collision.bind_body(Ref::new(u32::MAX, 1), 19));
        assert!(!collision.bind_body(late, u32::MAX));
        assert_eq!(collision.body_reference(hull.slot), Some(hull));
    }

    #[test]
    fn body_rebinding_rejects_old_generations_and_both_old_mapping_edges() {
        let mut collision = Collision::new(0, None);
        let old = Ref::new(4, 2);
        let next = Ref::new(4, 3);
        assert!(collision.bind_body(old, 8));
        assert!(collision.bind_body(next, 9));
        assert!(!collision.bind_body(old, 8));
        assert_eq!(collision.body_reference(4), Some(next));
        assert_eq!(collision.ref_of[8], -1);
        let replacement = Ref::new(12, 1);
        assert!(collision.bind_body(replacement, 9));
        assert_eq!(collision.body_reference(4), None);
        assert_eq!(collision.body_reference(12), Some(replacement));
        // Legacy numeric naming may move geometry but cannot grant a body
        // reference until its exact construction generation is bound.
        collision.name_body(12, 10);
        assert_eq!(collision.body_reference(12), None);
    }

    #[test]
    fn water_cells_as_the_set_holds_them() {
        // grid 4, cell 10, offset 20: x in [-20, 20) maps to columns 0..3.
        let r = rules(r#"{"water":{"cell":10,"offset":[20,20],"grid":4,"cells":[0,5,15]}}"#, -110.0);
        let w = r.water.as_ref().unwrap();
        assert!(w.over(-20.0, -20.0)); // (0, 0) -> 0
        assert!(!w.over(-5.0, -15.0)); // (1, 0) -> 1: not a cell
        assert!(w.over(-5.0, -5.0)); // (1, 1) -> 5
        assert!(w.over(1000.0, 1000.0)); // clamped to (3, 3) -> 15
        assert!(w.over(-1000.0, -1000.0)); // clamped to (0, 0)
        assert!(!w.over(f64::NAN, 0.0));
        assert!(!w.over(1000.0, -1000.0)); // (3, 0) -> 3
    }

    #[test]
    fn death_regions_by_kind() {
        let map = r#"{"deathRegions":[
            {"name":"deathregion1","type":"sphere","size":[0.1,5,0.1],"center":[10,0,0]},
            {"name":"Sarlac01","type":"cylinder","size":[3,2,4],"matrix":[1,0,0,0,0,1,0,0,0,0,1,0,-50,0,0,1]},
            {"name":"other","type":"box","size":[1,1,1],"center":[0,100,0]},
            {"name":"deathregion2","type":"box","size":[2,1,3],"center":[0,50,0]}],
            "mission":{"deathRegions":["sarlac01"]}}"#;
        let r = rules(map, -110.0);
        assert_eq!(r.regions.len(), 3);
        assert!(r.in_death_region(&Vector3::new(10.0, 4.0, 0.0)));
        assert!(!r.in_death_region(&Vector3::new(10.0, 6.0, 0.0)));
        assert!(r.in_death_region(&Vector3::new(-50.0, 1.0, 4.9)));
        assert!(!r.in_death_region(&Vector3::new(-50.0, 2.5, 0.0)));
        assert!(!r.in_death_region(&Vector3::new(0.0, 100.0, 0.0)), "not a death region: name and script leave it out");
        assert!(r.in_death_region(&Vector3::new(1.9, 50.5, -2.9)));
        assert!(!r.in_death_region(&Vector3::new(2.1, 50.5, 0.0)));
        assert!(r.in_death_region(&Vector3::new(0.0, -110.5, 0.0)));
        assert!(!r.in_death_region(&Vector3::new(0.0, -110.0, 0.0)));
    }
}
