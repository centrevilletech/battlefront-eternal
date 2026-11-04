//! What a Classic bot notices (src/ai/classic/perception.js): the sight
//! scan and its 112-entry heap, the global deferred line-of-sight queue (two
//! 200-entry heaps), each agent's target tracker, hearing, grenade danger,
//! and the alerts shots and damage raise, with the interrupt helper's
//! responses to them.

use three::Vector3;

use super::agent::dispatch;
use super::binding::CombatAi;
use super::aim::{drop_correct, resolve, Aimer};
use super::math::{angle, dot32, f, length_sq32, length_sq_xz32, lookup_cos, normalize_or_x, FLT_MAX};
use super::weapons_ai::{def_at, facing_and_range, forward_speed, max_expanded_range, selected_weapon, sensor_range, set_target, FireControl, Shared};
use super::world::{draw_range, uniform, Gen, Obj, QueryFilter, Ref, World};

/// perception.js:42, 53-56.
pub const LOCAL_CAPACITY: usize = 112;
pub const LOCAL_SENTINEL: f64 = 20000.0;
pub const GLOBAL_CAPACITY: usize = 200;
pub const GLOBAL_SENTINEL: f64 = 200.0;
pub const SCAN_CAPACITY: usize = 175;

// ---------------------------------------------------------------------------
// Heaps (perception.js:66).

/// perception.js:66 `PriorityHeap`: a one-based max-heap over priorities
/// kept in an Int32Array (each stored as ToInt32), with a sentinel at 0.
/// A newcomer's priority is compared as given (before its ToInt32), as the
/// JavaScript compares its argument with the stored ones.
#[derive(Clone, Debug, PartialEq)]
pub struct PriorityHeap<T> {
    pub capacity: usize,
    pub priority: Vec<i32>,
    pub items: Vec<Option<T>>,
    pub count: usize,
    pub dropped: f64,
}

/// What `insert` returns: nothing, the displaced item, or the newcomer
/// itself (turned away).
#[derive(Clone, Debug, PartialEq)]
pub enum Inserted<T> {
    Placed,
    Displaced(Option<T>),
    Dropped,
}

impl<T: Clone> PriorityHeap<T> {
    pub fn new(capacity: usize, sentinel: f64) -> Self {
        let mut priority = vec![0i32; capacity + 1];
        priority[0] = v8math::to_int32(sentinel);
        PriorityHeap { capacity, priority, items: vec![None; capacity + 1], count: 0, dropped: 0.0 }
    }

    pub fn size(&self) -> usize {
        self.count
    }

    /// perception.js:80 `clear()`.
    pub fn clear(&mut self) {
        for i in 1..=self.count {
            self.items[i] = None;
        }
        self.count = 0;
    }

    /// perception.js:86 `place(k, priority, item)`: bubble up past
    /// strictly lower parents.
    pub fn place(&mut self, mut k: usize, priority: f64, item: Option<T>) {
        let mut parent = k >> 1;
        while (self.priority[parent] as f64) < priority {
            self.priority[k] = self.priority[parent];
            self.items[k] = self.items[parent].clone();
            k = parent;
            parent = k >> 1;
        }
        self.priority[k] = v8math::to_int32(priority);
        self.items[k] = item;
    }

    /// perception.js:100 `minIndex()`: zero-based, scanning floor(n/2)..n,
    /// the earlier on ties.
    pub fn min_index(&self) -> isize {
        let mut best = self.count >> 1;
        for i in best + 1..=self.count {
            if self.priority[i] < self.priority[best] {
                best = i;
            }
        }
        best as isize - 1
    }

    /// perception.js:108 `insert(priority, item)`.
    pub fn insert(&mut self, priority: f64, item: Option<T>) -> Inserted<T> {
        if self.count < self.capacity {
            self.count += 1;
            self.place(self.count, priority, item);
            return Inserted::Placed;
        }
        let slot = (self.min_index() + 1) as usize;
        if (self.priority[slot] as f64) < priority {
            let old = self.items[slot].clone();
            self.place(slot, priority, item);
            return Inserted::Displaced(old);
        }
        self.dropped += 1.0;
        Inserted::Dropped
    }

    /// perception.js:128 `peekPriority()`.
    pub fn peek_priority(&self) -> f64 {
        if self.count > 0 {
            self.priority[1] as f64
        } else {
            0.0
        }
    }

    /// perception.js:132 `pop()`.
    pub fn pop(&mut self) -> Option<Option<T>> {
        if self.count == 0 {
            return None;
        }
        let top = self.items[1].clone();
        let n = self.count;
        let vp = self.priority[n];
        let vi = self.items[n].take();
        self.count = n - 1;
        let mut i = 1;
        if self.count >= 2 {
            loop {
                let mut c = i * 2;
                if c < self.count && self.priority[c] < self.priority[c + 1] {
                    c += 1;
                }
                if vp >= self.priority[c] {
                    break;
                }
                self.priority[i] = self.priority[c];
                self.items[i] = self.items[c].clone();
                i = c;
                if c > self.count >> 1 {
                    break;
                }
            }
        }
        if self.count >= 1 {
            self.priority[i] = vp;
            self.items[i] = vi;
        }
        Some(top)
    }
}

// ---------------------------------------------------------------------------
// Matchup priority and the target cost (perception.js:170).

const PRIORITY_INFANTRY: [f64; 11] = [10.0, 5.0, 20.0, 10.0, 5.0, 3.0, 15.0, 0.0, 20.0, 1.0, 0.0];
const PRIORITY_GROUND: [f64; 11] = [5.0, 1.0, 20.0, 5.0, 1.0, 10.0, 10.0, 0.0, 10.0, 1.0, 0.0];
const PRIORITY_FLYER: [f64; 11] = [5.0, 1.0, 20.0, 5.0, 10.0, 20.0, 10.0, 0.0, 20.0, 10.0, 0.0];

/// perception.js:174 `matchupPriority(source, target)`.
pub fn matchup_priority(source: f64, target: f64) -> f64 {
    let row = if source == 0.0 || source == 1.0 || source == 3.0 {
        &PRIORITY_INFANTRY
    } else if source == 2.0 || source == 4.0 || source == 6.0 || source == 8.0 {
        &PRIORITY_GROUND
    } else if source == 5.0 {
        &PRIORITY_FLYER
    } else {
        return 0.0;
    };
    super::world::index_of(target).and_then(|i| row.get(i).copied()).unwrap_or(0.0)
}

/// perception.js:191, 200.
pub const OBJECTIVE_PRIORITY: f64 = 20.0;
pub const TOWED_PRIORITY: f64 = 21.0;

/// perception.js:201 `targetPriority(unit, target, controller)`.
pub fn target_priority<W: World + ?Sized>(w: &W, unit: Obj, target: Obj, controller: Option<Obj>) -> f64 {
    let t = w.unit(target);
    if let Some(post) = t.building_post {
        if controller.and_then(|c| w.ctl(c).objective) == Some(post) {
            return OBJECTIVE_PRIORITY;
        }
    }
    let u = w.unit(unit);
    if u.pilot && u.vehicle.is_some() && u.tow_on {
        if let Some(tv) = t.vehicle {
            if u.towed == Some(tv) {
                return TOWED_PRIORITY;
            }
        }
    }
    matchup_priority(u.kind, t.kind)
}

/// perception.js:210 `rawCost(unit, target, controller)`.
pub fn raw_cost<W: World + ?Sized>(w: &mut W, unit: Obj, target: Obj, controller: Option<Obj>) -> f64 {
    let eye = w.origin(unit);
    let at = w.aim_point(target);
    let d = f(v8math::sqrt(length_sq32(f(at.x - eye.x), f(at.y - eye.y), f(at.z - eye.z))));
    v8math::trunc(d) * (21.0 - target_priority(w, unit, target, controller))
}

// ---------------------------------------------------------------------------
// Sight geometry (perception.js:220).

/// perception.js:225 `coneDistanceSq(unit, target)`: the squared distance
/// to the target's aim point inside the 77.5-degree cone, else -2.
pub fn cone_distance_sq<W: World + ?Sized>(w: &mut W, unit: Obj, target: Obj) -> f64 {
    let cone_cos = lookup_cos(angle::cone());
    let eye = w.origin(unit);
    let look = w.forward(unit);
    let at = w.aim_point(target);
    let rx = f(at.x - eye.x);
    let ry = f(at.y - eye.y);
    let rz = f(at.z - eye.z);
    let l2 = length_sq32(rx, ry, rz);
    if f(0.0001) < l2 {
        let z = dot32(look.x, look.y, look.z, rx, ry, rz);
        if z <= 0.0 || f(z * z) <= f(f(l2 * cone_cos) * cone_cos) {
            return -2.0;
        }
    }
    l2
}

/// perception.js:243 `POSTURE_RADIUS`.
fn posture_radius(posture: &str) -> f64 {
    let w = [f(0.7200000286), f(0.4499999881), f(0.8000000119)];
    let h = [f(1.8999999762), f(1.6000000238), f(0.5500000119)];
    let upright = |i: usize| f(v8math::sqrt(f(f(f(2.0 * f(w[i] * w[i]))) + f(0.25 * f(f(h[i] + f(0.2)) * f(h[i] + f(0.2)))))));
    match posture {
        "crouch" => upright(1),
        "prone" => {
            let p = f(w[2] * f(0.3332999945));
            f(v8math::sqrt(f(f(p * p) + f(2.5600001812))))
        }
        _ => upright(0),
    }
}

/// perception.js:251 `boundRadius(unit)`.
pub fn bound_radius<W: World + ?Sized>(w: &mut W, unit: Obj) -> f64 {
    let u = w.unit(unit);
    if u.kind == 0.0 {
        if let Some(s) = &u.soldier {
            let posture = if s.air { "stand" } else { s.posture.str().unwrap_or("stand") };
            return posture_radius(posture);
        }
    }
    f(w.sphere(unit).radius)
}

/// perception.js:260 `sightRange(owner, target)`.
pub fn sight_range<W: World + ?Sized>(w: &mut W, owner: Obj, target: Obj) -> f64 {
    let m = if w.unit(owner).kind == 5.0 { 2.0 } else { 1.0 };
    f(f(f(f(bound_radius(w, target) * 20.0) + 30.0)) * m)
}

/// perception.js:266 `validTarget(world, unit, target)`.
pub fn valid_target<W: World + ?Sized>(w: &mut W, unit: Obj, target: Obj) -> bool {
    let t = w.unit(target);
    if !t.alive || target == unit {
        return false;
    }
    if t.targetable_false {
        return false;
    }
    let max = if !t.combat_max_health.is_nullish() {
        t.combat_max_health.num()
    } else {
        let c = t.class.as_ref().map(|c| c.max_health.clone()).unwrap_or_default();
        if c.is_nullish() {
            0.0
        } else {
            c.num()
        }
    };
    if !(max < FLT_MAX) {
        return false;
    }
    let (a, b) = (w.unit(unit).team, w.unit(target).team);
    w.relation(a, b) < 0.0
}

/// perception.js:279 `losPoint(target, out)`: midway between the target's
/// eye and its aim point (its aim point without an eye).
pub fn los_point<W: World + ?Sized>(w: &mut W, target: Obj) -> Vector3 {
    let mut out = w.aim_point(target);
    if w.unit(target).has_origin {
        let eye = w.origin(target);
        out = Vector3 { x: f(f(eye.x + out.x) * 0.5), y: f(f(eye.y + out.y) * 0.5), z: f(f(eye.z + out.z) * 0.5) };
    }
    out
}

/// perception.js:290 `clearShot(world, unit, point, target)`.
pub fn clear_shot<W: World + ?Sized>(w: &mut W, unit: Obj, point: &Vector3, target: Option<Obj>) -> bool {
    let eye = w.origin(unit);
    w.los(&eye, point, Some(unit), target)
}

// ---------------------------------------------------------------------------
// Per-controller perception (perception.js:299).

/// perception.js:299 `Perception`.
#[derive(Clone, Debug, PartialEq)]
pub struct Perception {
    pub heap: PriorityHeap<Ref>,
    pub candidate: Option<Ref>,
    pub candidate_point: Vector3,
    pub aimer: Aimer,
    pub fire: FireControl,
    pub last_scan_count: f64,
    /// The Enhanced aimer (src/ai/enhanced/aim.js aimerOf), when made.
    pub enhanced: Option<super::enhanced::EnhancedAimer>,
}

impl Default for Perception {
    fn default() -> Self {
        Perception {
            heap: PriorityHeap::new(LOCAL_CAPACITY, LOCAL_SENTINEL),
            candidate: None,
            candidate_point: Vector3::default(),
            aimer: Aimer::default(),
            fire: FireControl::default(),
            last_scan_count: 0.0,
            enhanced: None,
        }
    }
}

impl Perception {
    /// perception.js:310 `reset()`.
    pub fn reset(&mut self) {
        self.heap.clear();
        self.candidate = None;
        self.aimer.started = false;
        self.fire.reset();
    }
}

fn ref_of<W: World + ?Sized>(w: &W, u: Option<Obj>) -> Option<Ref> {
    u.map(|u| Ref { obj: u, generation: w.unit(u).generation })
}

// ---------------------------------------------------------------------------
// The scan (perception.js:334).

/// perception.js:334 `scan(controller, world, dispatch)`.
pub fn scan<W: World + ?Sized>(w: &mut W, cx: &mut CombatAi, controller: Obj) -> f64 {
    let unit = w.ctl(controller).unit;
    let Some(unit) = unit else { return 0.0 };
    if !w.unit(unit).alive || w.ctl(controller).perception.is_none() {
        return 0.0;
    }
    w.ctl_mut(controller).perception.as_mut().unwrap().heap.clear();
    let mut special = false;
    let mut targets: Vec<Obj> = Vec::new();
    if let Some(shared) = w.shared_target(controller) {
        targets.push(shared);
        special = true;
    } else {
        let rs = w.ctl(controller).range_scale;
        let mut radius = max_expanded_range(&cx.shared.weapons, w.unit(unit), rs);
        radius = if radius <= 75.0 { 75.0 } else { radius };
        radius = if 350.0 <= radius { 350.0 } else { radius };
        let eye = w.origin(unit);
        let team = w.unit(unit).team;
        let found = w.query(&eye, radius, QueryFilter::Hostile { except: unit, team });
        targets.extend(found.into_iter().take(SCAN_CAPACITY));
    }
    let mut queued = 0.0;
    for target in targets {
        if !valid_target(w, unit, target) {
            continue;
        }
        if w.unit(target).collision_category == Some(16.0) {
            continue;
        }
        let l2 = if special { -1.0 } else { cone_distance_sq(w, unit, target) };
        if l2 == -2.0 {
            continue;
        }
        let r = sight_range(w, unit, target);
        if !(l2 < f(r * r)) {
            continue;
        }
        let e = dispatch(w, cx, Some(controller), unit, target, false);
        let pattern = e.pattern != 0.0 && !e.pattern.is_nan();
        if !pattern && !e.aux {
            continue;
        }
        let mut cost = v8math::max(1.0, raw_cost(w, unit, target, Some(controller)));
        if !pattern && e.aux {
            cost += 10000.0;
        }
        let r = ref_of(w, Some(target));
        w.ctl_mut(controller).perception.as_mut().unwrap().heap.insert(LOCAL_SENTINEL - cost, r);
        queued += 1.0;
    }
    w.ctl_mut(controller).perception.as_mut().unwrap().last_scan_count = queued;
    queued
}

// ---------------------------------------------------------------------------
// The global deferred-LOS queue (perception.js:377).

/// A sight request (perception.js:388): the controller, the target's
/// reference, the controller's life when it asked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LosRequest {
    pub controller: Obj,
    pub target: Option<Ref>,
    pub generation: f64,
}

/// perception.js:377 `DeferredLos`.
#[derive(Clone, Debug, PartialEq)]
pub struct DeferredLos {
    pub active: PriorityHeap<LosRequest>,
    pub scratch: PriorityHeap<LosRequest>,
    pub overflow: f64,
    pub tested: f64,
}

impl Default for DeferredLos {
    fn default() -> Self {
        DeferredLos {
            active: PriorityHeap::new(GLOBAL_CAPACITY, GLOBAL_SENTINEL),
            scratch: PriorityHeap::new(GLOBAL_CAPACITY, GLOBAL_SENTINEL),
            overflow: 0.0,
            tested: 0.0,
        }
    }
}

/// The candidate a controller's perception holds, while it lives.
fn candidate_of<W: World + ?Sized>(w: &W, controller: Obj) -> Option<Obj> {
    let c = w.ctl(controller).perception.as_ref()?.candidate;
    resolve(w, c)
}

/// perception.js:431 `requestLive(req)`.
fn request_live<W: World + ?Sized>(w: &W, req: &LosRequest) -> bool {
    let c = w.ctl(req.controller);
    let unit_alive = c.unit.is_some_and(|u| w.unit(u).alive);
    unit_alive && c.generation == req.generation && c.perception.is_some() && resolve(w, req.target).is_some()
}

impl DeferredLos {
    /// perception.js:387 `submit(controller, target, priority)`.
    pub fn submit(&mut self, controller: Obj, generation: f64, target: Option<Ref>, priority: f64) {
        let req = LosRequest { controller, target, generation };
        if let Inserted::Dropped = self.active.insert(priority, Some(req)) {
            self.overflow += 1.0;
        }
    }

    /// perception.js:394 `drain(controller)`.
    pub fn drain<W: World + ?Sized>(&mut self, w: &W, controller: Obj) {
        self.scratch.clear();
        while self.active.size() > 0 {
            let priority = self.active.peek_priority();
            let req = self.active.pop().unwrap().expect("a queued request");
            if req.controller == controller || !request_live(w, &req) || candidate_of(w, req.controller).is_some() {
                continue;
            }
            self.scratch.insert(priority, Some(req));
        }
        std::mem::swap(&mut self.active, &mut self.scratch);
    }

    /// perception.js:409 `service(world, budget)`: whether any test ran.
    pub fn service<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut Shared, budget: f64) -> bool {
        let mut tests = 0.0;
        let mut any = false;
        while tests < budget && self.active.size() > 0 {
            let req = self.active.pop().unwrap().expect("a queued request");
            if !request_live(w, &req) {
                continue;
            }
            let controller = req.controller;
            if candidate_of(w, controller).is_some() {
                continue;
            }
            let target = resolve(w, req.target).expect("a live request's target");
            tests += 1.0;
            self.tested += 1.0;
            any = true;
            let unit = w.ctl(controller).unit.expect("a live request's unit");
            let at = los_point(w, target);
            if clear_shot(w, unit, &at, Some(target)) {
                record_target(w, cx, controller, target, &at);
            }
        }
        any
    }
}

/// perception.js:441 `thin(controller, world, retainedRef, queue)`.
pub fn thin<W: World + ?Sized>(w: &mut W, los: &mut DeferredLos, controller: Obj, retained: Option<Ref>) -> f64 {
    if w.ctl(controller).perception.is_none() {
        return 0.0;
    }
    let generation = w.ctl(controller).generation;
    let retained = resolve(w, retained);
    thin_candidates(&mut WorldThin { w, los, controller, generation }, retained)
}

/// The heap and shared draws used by thinning. The live boundary and a
/// fully native combat world use the same rank and random-skip loop.
pub trait ThinQueue {
    fn size(&self) -> usize;
    fn pop(&mut self) -> Option<Obj>;
    fn skip(&mut self) -> bool;
    fn submit(&mut self, target: Option<Obj>, priority: f64);
}

struct WorldThin<'a, W: World + ?Sized> {
    w: &'a mut W,
    los: &'a mut DeferredLos,
    controller: Obj,
    generation: f64,
}

impl<W: World + ?Sized> ThinQueue for WorldThin<'_,W> {
    fn size(&self) -> usize { self.w.ctl(self.controller).perception.as_ref().unwrap().heap.size() }
    fn pop(&mut self) -> Option<Obj> {
        let r = self.w.ctl_mut(self.controller).perception.as_mut().unwrap().heap.pop().unwrap();
        resolve(self.w,r)
    }
    fn skip(&mut self) -> bool { super::world::draw_int(self.w,Gen::A,1.0,2.0) != 1.0 }
    fn submit(&mut self, target: Option<Obj>, priority: f64) {
        self.los.submit(self.controller,self.generation,ref_of(self.w,target),priority);
    }
}

pub fn thin_candidates(q: &mut impl ThinQueue, retained: Option<Obj>) -> f64 {
    let total = q.size();
    let mut rank = LOCAL_CAPACITY as f64;
    let mut submitted = 0.0;
    while q.size() > 0 {
        let mut c = q.pop();
        if total > 4 {
            let mut skip = true;
            while q.size() > 0 && skip {
                skip = q.skip();
                if retained.is_none() || c != retained {
                    if skip {
                        c = q.pop();
                    }
                } else {
                    skip = false;
                }
            }
        }
        let mut priority = rank;
        if c == retained {
            priority = v8math::max(rank, LOCAL_CAPACITY as f64 - 2.0);
        }
        q.submit(c,priority);
        submitted += 1.0;
        rank -= 1.0;
    }
    submitted
}

/// perception.js:477 `recordTarget(controller, target, point, world)`: the
/// controller's candidate, and a soldier target's cluster check.
pub fn record_target<W: World + ?Sized>(w: &mut W, cx: &mut Shared, controller: Obj, target: Obj, point: &Vector3) {
    let r = ref_of(w, Some(target));
    {
        let p = w.ctl_mut(controller).perception.as_mut().unwrap();
        p.candidate = r;
        p.candidate_point = *point;
    }
    let t = w.unit(target);
    if t.kind != 0.0 {
        return;
    }
    let (t_controller, t_generation, t_team, t_position) = (t.controller, t.generation, t.team, t.position);
    let now = f(w.time());
    let Some(key) = cx.clusters.state(target, t_controller, t_generation, true) else { return };
    let scan_at = cx.clusters.states[&key].scan_at;
    if !(f(scan_at + 5.0) < now) {
        return;
    }
    cx.clusters.states.get_mut(&key).unwrap().scan_at = now;
    let attacker = w.ctl(controller).unit.expect("the attacker");
    let found = w.query(&t_position, 9.0, QueryFilter::NotNeutral { except: target, team: t_team });
    let mut count = 1.0;
    for &u in found.iter().take(10) {
        let (a, b) = (w.unit(attacker).team, w.unit(u).team);
        if w.relation(a, b) > 0.0 {
            return;
        }
        if w.unit(u).kind == 0.0 {
            count += 1.0;
        }
    }
    if count > 2.0 {
        cx.clusters.states.get_mut(&key).unwrap().flagged_at = now;
    }
}

/// perception.js:499 `clearCandidate(controller)`.
pub fn clear_candidate<W: World + ?Sized>(w: &mut W, controller: Obj) {
    if let Some(p) = w.ctl_mut(controller).perception.as_mut() {
        p.candidate = None;
    }
}

// ---------------------------------------------------------------------------
// The target tracker (perception.js:509).

const DEFAULT_DELAY: f64 = 3.0;

/// perception.js:509 `TargetTracker`: each agent has one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetTracker {
    pub controller: Obj,
    pub target: Option<Ref>,
    pub fresh: bool,
    pub point: Vector3,
    pub sample_time: f64,
    pub cost: f64,
    pub delay: f64,
    pub frozen: bool,
    pub counting: bool,
    pub deadline: f64,
}

impl TargetTracker {
    pub fn new(controller: Obj) -> TargetTracker {
        TargetTracker {
            controller,
            target: None,
            fresh: false,
            point: Vector3::default(),
            sample_time: -1000.0,
            cost: 10000.0,
            delay: DEFAULT_DELAY,
            frozen: false,
            counting: false,
            deadline: 0.0,
        }
    }

    /// perception.js:523 `get target`.
    pub fn target<W: World + ?Sized>(&self, w: &W) -> Option<Obj> {
        resolve(w, self.target)
    }

    /// perception.js:528 `get freshTarget`.
    pub fn fresh_target<W: World + ?Sized>(&self, w: &W) -> Option<Obj> {
        if self.fresh {
            self.target(w)
        } else {
            None
        }
    }

    /// perception.js:533 `reset(target)` (and `assign`).
    pub fn reset<W: World + ?Sized>(&mut self, w: &W, target: Option<Obj>) {
        self.target = ref_of(w, target);
        self.fresh = false;
        self.sample_time = -1000.0;
        self.cost = 10000.0;
        self.counting = false;
    }

    /// perception.js:547 `refresh(target, point, now)`.
    fn refresh<W: World + ?Sized>(&mut self, w: &mut W, target: Option<Obj>, point: &Vector3, now: f64) {
        self.fresh = true;
        self.point = *point;
        self.sample_time = f(now);
        let unit = w.ctl(self.controller).unit;
        self.cost = match (unit, target) {
            (Some(u), Some(t)) => raw_cost(w, u, t, Some(self.controller)),
            _ => 10000.0,
        };
        self.counting = false;
    }

    /// perception.js:557 `startCountdown(now)`.
    pub fn start_countdown(&mut self, now: f64) {
        self.counting = true;
        self.deadline = if self.frozen { FLT_MAX } else { f(f(now) + f(self.delay)) };
    }

    /// perception.js:564 `setDelay(seconds, now)`.
    pub fn set_delay(&mut self, seconds: f64, now: f64) {
        self.delay = f(seconds);
        if self.counting {
            self.start_countdown(now);
        }
    }

    /// perception.js:569 `defaultDelay(now)`.
    pub fn default_delay(&mut self, now: f64) {
        self.set_delay(DEFAULT_DELAY, now);
    }

    /// perception.js:574 `freeze(on, now)`.
    pub fn freeze(&mut self, on: bool, now: f64) {
        self.frozen = on;
        if self.counting {
            self.start_countdown(now);
        }
    }

    /// perception.js:581 `select(world)`: follow the controller's
    /// candidate; whether the target changed.
    pub fn select<W: World + ?Sized>(&mut self, w: &mut W) -> bool {
        let controller = self.controller;
        let unit = w.ctl(controller).unit;
        let now = w.time();
        let candidate = candidate_of(w, controller);
        let before = self.target(w);
        let mut current = before;
        if let (Some(cur), Some(u)) = (current, unit) {
            let (a, b) = (w.unit(u).team, w.unit(cur).team);
            if w.relation(a, b) >= 0.0 {
                self.reset(w, candidate);
                current = self.target(w);
            }
        }
        if current.is_none() {
            self.reset(w, candidate);
            current = self.target(w);
        } else if current != candidate {
            let cost = match candidate {
                Some(c) => raw_cost(w, unit.expect("the tracker's unit"), c, Some(controller)),
                None => 10000.0,
            };
            if cost < self.cost {
                self.reset(w, candidate);
                current = self.target(w);
            } else if !self.counting {
                self.start_countdown(now);
            } else if f(self.deadline + 0.0) < f(now) {
                self.reset(w, candidate);
                current = self.target(w);
            }
        }
        if current.is_some() && current == candidate {
            let point = w.ctl(controller).perception.as_ref().unwrap().candidate_point;
            self.refresh(w, current, &point, now);
        } else {
            self.fresh = false;
        }
        before != self.target(w)
    }

    /// perception.js:615 `assignWeaponTarget(world)`: the remembered point
    /// when the target is not fresh.
    pub fn assign_weapon_target<W: World + ?Sized>(&self, w: &mut W, table: &super::weapons_ai::WeaponAiTable) -> Option<Vector3> {
        let controller = self.controller;
        let target = self.target(w);
        let unit = w.ctl(controller).unit;
        if !self.fresh && target.is_some() {
            set_target(w, table, controller, unit, None);
            return Some(self.point);
        }
        set_target(w, table, controller, unit, target);
        None
    }

    /// perception.js:628 `pointNearTarget()`.
    pub fn point_near_target<W: World + ?Sized>(&self, w: &mut W) -> bool {
        let Some(target) = self.target(w) else { return false };
        let at = los_point(w, target);
        length_sq32(f(self.point.x - at.x), f(self.point.y - at.y), f(self.point.z - at.z)) < 9.0
    }

    /// perception.js:636 `fireControls(world)`: the controller's two fire
    /// requests.
    pub fn fire_controls<W: World + ?Sized>(&self, w: &mut W, table: &super::weapons_ai::WeaponAiTable) {
        let controller = self.controller;
        let unit = w.ctl(controller).unit;
        let mut a = false;
        let mut b = false;
        let target = self.target(w);
        if let (Some(target), Some(unit)) = (target, unit) {
            if !self.fresh {
                let weapon = selected_weapon(w, unit, Some(controller));
                let lead = match weapon {
                    Some(i) => {
                        let def = def_at(w, unit, i);
                        drop_correct(w, &self.point, unit, &def).0
                    }
                    None => self.point,
                };
                if weapon.is_some() && facing_and_range(w, table, controller, unit, &lead, &self.point) && self.point_near_target(w) {
                    a = clear_shot(w, unit, &self.point, Some(target));
                }
            } else {
                b = true;
            }
        }
        let c = w.ctl_mut(controller);
        c.fire_a = a;
        c.fire_b = b;
    }
}

/// perception.js:659 `update(world, dispatch)` on the agent's tracker:
/// selection, weapon choice, the weapon's target, the fire requests.
pub fn tracker_update<W: World + ?Sized>(w: &mut W, cx: &mut CombatAi, agent: Obj) {
    let mut t = *w.tracker(agent).expect("the agent's tracker");
    t.select(w);
    *w.tracker(agent).unwrap() = t;
    let controller = t.controller;
    let target = t.target(w);
    let unit = w.ctl(controller).unit;
    if let (Some(target), Some(unit)) = (target, unit) {
        if w.unit(unit).arsenal.as_ref().is_some_and(|a| !a.weapons.is_empty()) {
            dispatch(w, cx, Some(controller), unit, target, true);
        }
    }
    let t = *w.tracker(agent).unwrap();
    t.assign_weapon_target(w, &cx.shared.weapons);
    t.fire_controls(w, &cx.shared.weapons);
}

// ---------------------------------------------------------------------------
// Acoustic awareness (perception.js:678).

const DEFAULT_MUTE: f64 = 400.0;

/// perception.js:680 `weaponNoise(weapon, sounds)` (no sound table bound).
fn weapon_noise(wv: Option<&super::world::WeaponView>) -> f64 {
    let Some(wv) = wv else { return 0.0 };
    let active = wv.charging || wv.busy || wv.wait > 1e-6;
    if !active {
        return 0.0;
    }
    let mute = wv.def.get("MuteDistance");
    f(if mute.is_nullish() { DEFAULT_MUTE } else { mute.num() })
}

fn clamp50(x: f64) -> f64 {
    if x < 0.0 {
        0.0
    } else if 50.0 < x {
        50.0
    } else {
        x
    }
}

/// perception.js:699 `noiseRadius(unit, sounds)`.
pub fn noise_radius<W: World + ?Sized>(w: &W, unit: Obj) -> f64 {
    let u = w.unit(unit);
    let speed = match u.velocity {
        Some(v) => f(v8math::sqrt(length_sq32(v.x, v.y, v.z))),
        None => 0.0,
    };
    let k = u.kind;
    if k == 7.0 {
        return 0.0;
    }
    if k == 5.0 {
        return 1.0;
    }
    if k != 0.0 {
        return clamp50(f(speed * 2.0));
    }
    let weapon = match &u.arsenal {
        Some(a) => selected_weapon(w, unit, u.controller).or_else(|| a.current(0.0)),
        None => None,
    };
    let wv = weapon.map(|i| &w.unit(unit).arsenal.as_ref().unwrap().weapons[i]);
    let noise = f(weapon_noise(wv) * 0.5);
    let m = clamp50(f(speed * 3.0));
    if noise <= m {
        m
    } else {
        noise
    }
}

/// perception.js:721 `broadcastAcoustic(source, world, sounds)`: how many
/// bots were told.
pub fn broadcast_acoustic<W: World + ?Sized>(w: &mut W, source: Obj) -> f64 {
    if !w.unit(source).alive {
        return 0.0;
    }
    let raw = noise_radius(w, source);
    let r = if raw <= 30.0 { raw } else { 30.0 };
    if !(r > 0.0) {
        return 0.0;
    }
    let (at, team, generation) = {
        let s = w.unit(source);
        (s.position, s.team, s.generation)
    };
    let heard = w.query(&at, r, QueryFilter::Hostile { except: source, team });
    let mut notified = 0.0;
    for &recipient in heard.iter().take(SCAN_CAPACITY) {
        let Some(controller) = w.unit(recipient).controller else { continue };
        let p = w.unit(recipient).position;
        let dx = f(at.x - p.x);
        let dy = f(at.y - p.y);
        let dz = f(at.z - p.z);
        let n = normalize_or_x(dx, dy, dz);
        let vertical = v8math::abs(dot32(0.0, 1.0, 0.0, n.x, n.y, n.z));
        if f(0.87) <= vertical && f(12.25) <= length_sq32(dx, dy, dz) {
            continue;
        }
        w.ctl_mut(controller).add_acoustic(Ref { obj: source, generation });
        notified += 1.0;
    }
    notified
}

// ---------------------------------------------------------------------------
// Grenade and sticky danger (perception.js:745).

/// A live grenade's danger state (`round.aiDanger`, perception.js:745).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DangerState {
    pub first_contact: f64,
    pub last_notify: f64,
}

impl Default for DangerState {
    fn default() -> Self {
        DangerState { first_contact: f64::INFINITY, last_notify: -1.0 }
    }
}

/// perception.js:753 `dangerDelayRange(enemySkill)`.
pub fn danger_delay_range(enemy_skill: f64) -> (f64, f64) {
    if enemy_skill < 17.0 {
        return if enemy_skill < 14.0 { (f(1.1), f(2.1)) } else { (f(0.6), f(1.3)) };
    }
    (0.0, f(0.8))
}

/// perception.js:758 `notifyDanger(world, state, position, owner,
/// enemySkill, onWarn)`: `warn` names the first soldier warned (the
/// caller's shout).
pub fn notify_danger<W: World + ?Sized>(w: &mut W, state: &mut DangerState, position: &Vector3, owner: Option<Obj>, enemy_skill: f64, warn: &mut dyn FnMut(&mut W, Obj)) -> f64 {
    let (lo, hi) = w.difficulty().danger_row.unwrap_or_else(|| danger_delay_range(enemy_skill));
    let delay = draw_range(w, Gen::A, lo, hi);
    let now = f(w.time());
    if !(f(state.first_contact + delay) < now) || !(f(state.last_notify + 1.0) < now) {
        return 0.0;
    }
    let found = w.query(position, 7.5, QueryFilter::Except { except: owner });
    let mut warned = false;
    let mut notified = 0.0;
    for &u in found.iter().take(SCAN_CAPACITY) {
        let (controller, human, kind) = {
            let v = w.unit(u);
            (v.controller, v.human, v.kind)
        };
        if controller.is_none() && !human {
            continue;
        }
        if !warned && kind == 0.0 {
            warned = true;
            warn(w, u);
        }
        if let Some(c) = controller {
            w.ctl_mut(c).notify_danger(position);
            notified += 1.0;
        }
    }
    state.last_notify = now;
    notified
}

// ---------------------------------------------------------------------------
// Targeted-fire and damage alerts (perception.js:787).

/// perception.js:812 `alertControllers(target, out)`.
pub fn alert_controllers<W: World + ?Sized>(w: &W, target: Obj) -> Vec<Obj> {
    let mut out: Vec<Obj> = Vec::new();
    let mut add = |c: Option<Obj>| {
        if let Some(c) = c {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    };
    let t = w.unit(target);
    match &t.seat {
        Some(lease) if !lease.human && lease.controllable_controller.is_some() => add(lease.controllable_controller),
        _ => add(t.controller),
    }
    if (t.turret.is_some() || t.vehicle.is_some()) && !t.has_lease {
        if let Some(leases) = &t.body_leases {
            for l in leases.iter().flatten() {
                if l.human || l.released || l.live_false {
                    continue;
                }
                add(l.controllable_controller.or(l.unit_controller));
            }
        }
    }
    out
}

/// perception.js:789 `notifyTargetedShot(world, shooter, target,
/// shooterPosition)`: the shooter's Character is the source.
pub fn notify_targeted_shot<W: World + ?Sized>(w: &mut W, shooter: Obj, target: Obj, shooter_position: &Vector3) -> bool {
    if !w.unit(target).alive {
        return false;
    }
    let Some(source) = w.unit(shooter).character else { return false };
    let (a, b) = (w.unit(shooter).team, w.unit(target).team);
    if !(w.relation(a, b) < 0.0) {
        return false;
    }
    if w.unit(target).human {
        return false;
    }
    let mut notified = false;
    for c in alert_controllers(w, target) {
        w.ctl_mut(c).notify_shot(Some(source), true, Some(*shooter_position));
        notified = true;
    }
    notified
}

/// A damage packet (perception.js:832).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Packet {
    pub amount: Option<f64>,
    pub generic: Option<f64>,
    pub typed: Option<f64>,
}

/// An attacker as the damage alert reads it (a unit, a round's frozen
/// owner, or a Character): its Character (`attacker.character`), and its
/// team and its Character's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attacker {
    pub obj: Obj,
    pub character: Option<Obj>,
    pub team: Option<f64>,
    pub character_team: Option<f64>,
}

/// perception.js:836 `notifyDamage(world, victim, attacker, packet, known,
/// point)`.
pub fn notify_damage<W: World + ?Sized>(w: &mut W, victim: Obj, attacker: Attacker, packet: Packet, known: bool, point: Option<Vector3>) -> bool {
    if !w.unit(victim).alive {
        return false;
    }
    let source = attacker.character.unwrap_or(attacker.obj);
    let source_team = if attacker.character.is_some() { attacker.character_team } else { attacker.team };
    let team = attacker.team.or(source_team).unwrap_or(f64::NAN);
    let vt = w.unit(victim).team;
    if !(w.relation(vt, team) < 0.0) {
        return false;
    }
    let amount = f(packet.amount.unwrap_or(0.0));
    let generic = f(packet.generic.unwrap_or(1.0));
    let typed = f(packet.typed.unwrap_or(1.0));
    if !(f(amount * generic) > 1.0 || f(amount * typed) > 1.0) {
        return false;
    }
    let mut notified = false;
    for c in alert_controllers(w, victim) {
        w.ctl_mut(c).notify_shot(Some(source), known, point);
        notified = true;
    }
    notified
}

// ---------------------------------------------------------------------------
// The interrupt helper's responses (perception.js:858).

/// A response (perception.js:858): a combat goal (`kind` 2) in `mode` at
/// `point`, whether the mount is to be left, whether the bot shouts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Response {
    pub mode: f64,
    pub point: Vector3,
    pub exit: bool,
    pub alert: bool,
}

/// perception.js:861 `weaponGate(controller)`.
fn weapon_gate<W: World + ?Sized>(w: &W, controller: Obj) -> bool {
    match w.ctl(controller).unit {
        Some(u) => selected_weapon(w, u, Some(controller)).is_some(),
        None => false,
    }
}

/// perception.js:868 `sourceUnit(controller, source)`: the alert source's
/// current controllable, while it lives.
fn source_unit<W: World + ?Sized>(w: &mut W, source: Option<Obj>) -> Option<Obj> {
    let source = source?;
    if w.is_unit(source) {
        return if w.unit(source).alive { Some(source) } else { None };
    }
    let (bound, u) = w.current_controllable(source);
    let u = if bound && u.is_some() { u } else { w.character_remote(source).or_else(|| w.character_unit(source)) };
    let u = u?;
    if w.unit(u).alive {
        Some(u)
    } else {
        None
    }
}

/// perception.js:903 `engageable(controller, unit, target, world, dispatch)`.
fn engageable<W: World + ?Sized>(w: &mut W, cx: &mut CombatAi, controller: Obj, unit: Obj, target: Obj) -> bool {
    if !w.unit(unit).arsenal.as_ref().is_some_and(|a| !a.weapons.is_empty()) {
        return false;
    }
    let e = dispatch(w, cx, Some(controller), unit, target, false);
    e.pattern != 0.0 && !e.pattern.is_nan()
}

/// perception.js:913 `mountedExit(controller, world, point)`.
fn mounted_exit<W: World + ?Sized>(w: &mut W, controller: Obj, point: &Vector3) -> bool {
    let Some(unit) = w.ctl(controller).unit else { return false };
    if forward_speed(w.unit(unit)) != 0.0 {
        return false;
    }
    if w.ai_stay_in_turrets() {
        return false;
    }
    if w.has_mount_reach() {
        !w.mount_reach(unit, point)
    } else {
        false
    }
}

/// perception.js:881 `alertResponse(controller, world, agent, dispatch)`.
pub fn alert_response<W: World + ?Sized>(w: &mut W, cx: &mut CombatAi, controller: Obj, agent: Option<Obj>) -> Option<Response> {
    let shot = w.ctl(controller).shot;
    if !shot.pending {
        return None;
    }
    let unit = w.ctl(controller).unit;
    let src = source_unit(w, shot.source);
    let (Some(unit), Some(src)) = (unit, src) else { return None };
    let point = w.unit(src).position;
    if !weapon_gate(w, controller) {
        return None;
    }
    match agent {
        Some(a) if w.agent_accepts_alert(a, shot.known, &point) => {}
        _ => return None,
    }
    let r = sight_range(w, unit, src);
    let p = w.unit(unit).position;
    let delta = Vector3 { x: point.x - p.x, y: point.y - p.y, z: point.z - p.z };
    if length_sq32(f(delta.x), f(delta.y), f(delta.z)) < f(r * r) && engageable(w, cx, controller, unit, src) {
        let exit = mounted_exit(w, controller, &point);
        return Some(Response { mode: 0.0, point, exit, alert: false });
    }
    if w.unit(unit).kind != 0.0 {
        return None;
    }
    let z = f(2.0 * f(uniform(w, Gen::B) - 0.5));
    let y = f(2.0 * f(uniform(w, Gen::B) - 0.5));
    let x = f(2.0 * f(uniform(w, Gen::B) - 0.5));
    let p = w.unit(unit).position;
    Some(Response { mode: 2.0, point: Vector3 { x: f(p.x + x), y: f(p.y + y), z: f(p.z + z) }, exit: false, alert: false })
}

/// perception.js:924 `hazardResponse(controller, world, agent)`.
pub fn hazard_response<W: World + ?Sized>(w: &mut W, controller: Obj, agent: Option<Obj>) -> Option<Response> {
    let danger = w.ctl(controller).danger;
    if !danger.pending {
        return None;
    }
    let unit = w.ctl(controller).unit?;
    if !weapon_gate(w, controller) {
        return None;
    }
    match agent {
        Some(a) if w.agent_accepts_hazard(a) => {}
        _ => return None,
    }
    let kind = w.unit(unit).kind;
    if kind == 5.0 && w.nav_in_danger_region(unit).unwrap_or(false) {
        return None;
    }
    if kind != 0.0 {
        return None;
    }
    Some(Response { mode: 2.0, point: danger.point, exit: false, alert: false })
}

/// perception.js:937 `acousticResponse(controller, world, agent)`.
pub fn acoustic_response<W: World + ?Sized>(w: &mut W, cx: &mut CombatAi, controller: Obj, agent: Option<Obj>) -> Option<Response> {
    let unit = w.ctl(controller).unit?;
    if w.ctl(controller).acoustic.is_empty() {
        return None;
    }
    if w.unit(unit).kind == 5.0 && w.nav_in_danger_region(unit).unwrap_or(false) {
        return None;
    }
    let list = w.ctl(controller).acoustic.clone();
    for r in list {
        let Some(src) = resolve(w, Some(r)) else { continue };
        if !weapon_gate(w, controller) {
            continue;
        }
        match agent {
            Some(a) if w.agent_accepts_acoustic(a, src) => {}
            _ => continue,
        }
        let mut range = sensor_range(w, &cx.shared.weapons, unit, controller);
        if w.unit(unit).kind == 3.0 {
            range = f(range * 0.5);
        }
        let point = w.unit(src).position;
        let p = w.unit(unit).position;
        if length_sq_xz32(f(point.x - p.x), f(point.z - p.z)) < f(range * range) {
            let exit = mounted_exit(w, controller, &point);
            return Some(Response { mode: 0.0, point, exit, alert: true });
        }
    }
    None
}
