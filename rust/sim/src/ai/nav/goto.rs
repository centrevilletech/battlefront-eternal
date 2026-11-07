//! The ground and flyer movement helpers agents drive (nav/goto.js),
//! and the NavControl operations that need the world (the follow
//! trail and the per-tick update, control.js:202-267).
//!
//! What a helper asks of the rest of the battle is `GotoWorld`: the
//! time, the two generators, following's target resolution and
//! remembered point, the jet attempt's 40 m clear-above test, and the
//! formation far radius of the trail. The helper's unit is the snapshot
//! of units.rs handed to each call; the JavaScript's `get unit()`
//! (goto.js:66) also attaches the control, which the entry points here
//! do first. A helper's aim commands keep the controller's aim state
//! the way controller.js:613-625 writes it (lookAt/aimAt/lookAlongPath
//! only set those fields).

use super::analogs::{analogs, apply_jet_and_posture, Intent};
use super::cursor::GraphCursor;
use super::math::{d2, f32, flat_dir, h2, same, tiny, Vec3, FLT_MAX};
use super::planner::PathNode;
use super::units::{has_jetpack, is_soldier, is_soldier_or_droid, kind, nominal_speed, traversal_mask, traversal_radius, NavUnit, UnitRef};
use super::{ControlHost, Nav};
use crate::ai::random::Random;

/// A resolved follow/leader target: its identity token and the fields
/// the helpers read (goto.js:722 `resolve`, control.js:259).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resolved {
    pub id: u64,
    pub position: Vec3,
    pub velocity: Option<Vec3>,
}

/// What goto and the follow trail ask of the battle. Implementations
/// reproduce the JavaScript's lookups:
/// - `random_a` is world.random (the AI generator, goto.js:80);
/// - `random_b` is the jet attempt's second generator (goto.js:746:
///   world.gameRandom, else a private stream seeded 0x9e3779b9 kept on
///   the world, shared by every helper) and the trail's
///   (control.js:227: randomB ?? gameRandom ?? random);
/// - `resolve` is goto.js:722 (world.units.resolve when present, else
///   the ref's unit when alive and of the ref's generation);
/// - `leader` is control.js:259 (the ref's unit when alive exactly
///   (truthy) and of the ref's generation), as a full snapshot, since
///   the trail reads its position and forward;
/// - `remembered` is the helper options' remembered point
///   (goto.js:538);
/// - `clear_above` is goto.js:730 (world.los 40 m up ignoring the unit,
///   else the collision cast, else clear);
/// - `follow_far` is control.js:228 (the AI's followRadii.far, else the
///   mission formation far, else 3).
pub trait GotoWorld {
    fn time(&self) -> f64;
    fn random_a(&mut self) -> &mut dyn Random;
    fn random_b(&mut self) -> &mut dyn Random;
    fn resolve(&mut self, r: UnitRef) -> Option<Resolved>;
    fn leader(&mut self, r: UnitRef) -> Option<NavUnit>;
    fn remembered(&mut self, target: u64) -> Option<Vec3>;
    fn clear_above(&mut self, p: Vec3, unit: &NavUnit) -> bool;
    fn follow_far(&mut self) -> f64;
}

/// goto.js:722's fallback branch, for implementors: the ref's unit when
/// `alive !== false` and (no generation named or it matches).
pub fn resolve_ref(r: UnitRef, unit: Option<&NavUnit>) -> Option<Resolved> {
    let unit = unit.filter(|u| u.id == r.unit)?;
    if unit.alive_false {
        return None;
    }
    if let Some(generation) = r.generation {
        if unit.generation != Some(generation) {
            return None;
        }
    }
    Some(Resolved { id: unit.id, position: unit.position, velocity: Some(unit.velocity) })
}

/// Which route a goto asks for (goto.js:88's options.mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GotoMode {
    Route,
    Direct,
    Straight,
}

/// GroundGotoHelper (goto.js:40), with the controller's aim state it
/// writes (aim_state, aim_point, aim_target).
#[derive(Clone, Debug)]
pub struct GroundGoto {
    pub control: usize,
    pub state: f64,
    pub pending: f64,
    pub destination: Vec3,
    pub failed: bool,
    pub follow_distance: f64,
    pub cursor: Option<GraphCursor>,
    pub adjusted: bool,
    pub normal: Vec3,
    pub retries: f64,
    pub last_target: Vec3,
    pub last_refresh: f64,
    pub follow_remembered: bool,
    pub target: Option<UnitRef>,
    pub detour_side: f64,
    pub detour_point: Vec3,
    pub detour_final: Vec3,
    pub jet_deadline: f64,
    pub aim_state: f64,
    pub aim_point: Vec3,
    pub aim_target: Option<UnitRef>,
}

impl GroundGoto {
    pub fn new(control: usize) -> Self {
        GroundGoto {
            control,
            state: 1.0,
            pending: 0.0,
            destination: Vec3::default(),
            failed: false,
            follow_distance: 0.0,
            cursor: None,
            adjusted: false,
            normal: Vec3::default(),
            retries: 0.0,
            last_target: Vec3::new(-FLT_MAX, 0.0, -FLT_MAX),
            last_refresh: -100.0,
            follow_remembered: false,
            target: None,
            detour_side: 0.0,
            detour_point: Vec3::default(),
            detour_final: Vec3::default(),
            jet_deadline: 0.0,
            aim_state: 2.0,
            aim_point: Vec3::default(),
            aim_target: None,
        }
    }
}

/// FlyGotoHelper (goto.js:605).
#[derive(Clone, Debug)]
pub struct FlyGoto {
    pub control: usize,
    pub state: f64,
    pub pending: f64,
    pub destination: Vec3,
    pub failed: bool,
}

impl FlyGoto {
    pub fn new(control: usize) -> Self {
        FlyGoto { control, state: 1.0, pending: 0.0, destination: Vec3::default(), failed: false }
    }
}

impl Nav {
    // ----- control operations that need the world (control.js) -----

    /// Follow's temporary path recorder (control.js:202). Installing
    /// the recorded path steers at once, hence the unit.
    pub fn control_record_trail(&mut self, control: usize, on: bool, unit: &mut NavUnit) {
        if on {
            if let Some(trail) = self.controls[control].trail.take() {
                self.store.release(trail);
            }
            let trail = self.store.make();
            self.controls[control].trail = Some(trail);
            return;
        }
        let Some(path) = self.controls[control].trail.take() else { return };
        if self.store.length(path) == 0 {
            self.store.release(path);
            return;
        }
        if self.controls[control].executor.is_none() {
            self.control_install(control, path, true, false, unit);
        } else {
            let mut executor = self.controls[control].executor.take().expect("checked");
            let Nav { store, controls, queue, current, .. } = self;
            let mut host = ControlHost { controls, queue, current, control };
            executor.append(store, &mut host, path, unit, false);
            executor.complete = false;
            if host.controls[control].executor.is_none() {
                host.controls[control].executor = Some(executor);
            }
        }
    }

    /// Quarter-second trail sampling (control.js:222).
    pub fn control_sample_trail(&mut self, control: usize, dt: f64, leader: &NavUnit, world: &mut dyn GotoWorld, unit: &mut NavUnit) {
        if self.controls[control].executor.as_ref().map(|e| e.hold).unwrap_or(false) {
            return;
        }
        self.controls[control].trail_delay = f32(self.controls[control].trail_delay - dt);
        if !(self.controls[control].trail_delay < 0.0) {
            return;
        }
        let far = world.follow_far();
        let rng = world.random_b();
        let z = 2.0 * f32(rng.uniform() - 0.5) * far;
        rng.uniform(); // the discarded local Y still consumes a draw
        let x = 2.0 * f32(rng.uniform() - 0.5) * far;
        let mut forward = leader.forward.unwrap_or(Vec3::new(0.0, 0.0, 1.0));
        forward.y = 0.0;
        super::math::three_normalize(&mut forward);
        let lp = leader.position;
        // leader.position + forward * z + (forward.z * x, 0, -forward.x * x)
        let point = Vec3::new(lp.x + forward.x * z + forward.z * x, lp.y + forward.y * z, lp.z + forward.z * z - forward.x * x);
        let tail = {
            let c = &self.controls[control];
            match c.trail {
                Some(trail) => self.store.paths[trail].nodes.last().map(|n| Vec3::new(n.x, n.y, n.z)),
                None => c.executor.as_ref().map(|e| e.fin),
            }
        };
        if let Some(tail) = tail {
            if d2(point, tail) <= 0.0625 {
                return;
            }
        }
        self.controls[control].trail_delay = 0.25;
        if let Some(trail) = self.controls[control].trail {
            if self.store.length(trail) as f64 > 29.0 {
                self.store.drop_front(trail, 1.0);
            }
            self.store.append(trail, PathNode { x: point.x, y: point.y, z: point.z, flag: 0 });
        } else if self.controls[control].executor.is_some() {
            let mut executor = self.controls[control].executor.take().expect("checked");
            let Nav { store, controls, queue, current, .. } = self;
            let mut host = ControlHost { controls, queue, current, control };
            executor.append_point(store, &mut host, point, unit, 30.0);
            if host.controls[control].executor.is_none() {
                host.controls[control].executor = Some(executor);
            }
        }
    }

    /// The per-tick path step (control.js:257).
    pub fn control_update(&mut self, control: usize, dt: f64, unit: &mut NavUnit, world: &mut dyn GotoWorld) {
        let leader_ref = self.controls[control].leader;
        let leader = leader_ref.and_then(|r| world.leader(r));
        if let Some(leader) = &leader {
            self.control_sample_trail(control, dt, leader, world, unit);
        }
        let has_executor = self.controls[control].executor.is_some();
        if !has_executor && self.controls[control].stranded {
            self.controls[control].stranded_for = f32(self.controls[control].stranded_for + dt);
        } else if self.controls[control].stranded_for != 0.0 && d2(unit.position, self.controls[control].stranded_at) > 4.0 {
            self.controls[control].stranded_for = 0.0;
        }
        let Some(mut executor) = self.controls[control].executor.take() else { return };
        let complete = {
            let Nav { store, controls, queue, current, .. } = self;
            let mut host = ControlHost { controls, queue, current, control };
            executor.tick(store, &mut host, dt, unit);
            executor.complete
        };
        // The JavaScript drops the executor it ticked when it completed
        // and is still the installed one (control.js:266); nothing in a
        // tick installs another, so the executor taken out is it.
        if complete && leader.is_none() {
            executor.release(&mut self.store);
        } else if self.controls[control].executor.is_none() {
            self.controls[control].executor = Some(executor);
        } else {
            executor.release(&mut self.store);
        }
    }

    /// The navigation half of the AI input step (analogs.js:208).
    pub fn navigation_input(&mut self, control: usize, unit: &mut NavUnit, dt: f64, aim: Option<Vec3>, facing: bool, world: &mut dyn GotoWorld) -> Intent {
        self.control_attach(control, Some(unit));
        self.control_update(control, dt, unit, world);
        let (request, hold) = {
            let c = &self.controls[control];
            (c.request, c.executor.as_ref().map(|e| e.hold))
        };
        let mut intent = analogs(unit, Some(&request), aim, dt, facing, hold);
        let (pending, jetting, vector, stance) = {
            let c = &self.controls[control];
            (c.jet.pending, c.jetting, c.jet.vector, c.stance)
        };
        let (pending, jetting) = apply_jet_and_posture(unit, &mut intent, pending, jetting, vector, stance);
        let c = &mut self.controls[control];
        c.jet.pending = pending;
        c.jetting = jetting;
        intent
    }

    // ----- GroundGotoHelper (goto.js:40) -----

    fn goto_attach(&mut self, helper: &GroundGoto, unit: &NavUnit) {
        self.control_attach(helper.control, Some(unit));
    }

    /// goto.js:88.
    pub fn ground_goto(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld, point: Vec3, mode: GotoMode) {
        self.goto_attach(helper, unit);
        self.control_strand(helper.control, false, Some(unit));
        match mode {
            GotoMode::Direct => {
                if is_soldier_or_droid(unit) {
                    self.ground_goto_direct(helper, unit, point);
                } else {
                    self.ground_goto_detour(helper, point);
                }
            }
            GotoMode::Straight => self.ground_goto_straight(helper, unit, point),
            GotoMode::Route => {
                self.ground_drop_route(helper);
                self.ground_route(helper, unit, world, false, point);
                helper.pending = 2.0;
            }
        }
        self.ground_flush(helper, unit, world);
    }

    /// goto.js:105.
    pub fn ground_stop(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld) {
        self.ground_drop_route(helper);
        self.control_strand(helper.control, false, Some(unit));
        helper.pending = 7.0;
        self.ground_flush(helper, unit, world);
    }

    /// goto.js:114.
    pub fn ground_hold_path(&mut self, helper: &GroundGoto, on: bool) {
        if let Some(e) = self.controls[helper.control].executor.as_mut() {
            e.hold = on;
        }
    }

    /// goto.js:115.
    pub fn ground_install_path(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, points: &[Vec3]) {
        self.goto_attach(helper, unit);
        if points.is_empty() {
            return;
        }
        self.ground_drop_route(helper);
        let path = self.store.make();
        for point in points {
            self.store.append(path, PathNode { x: point.x, y: point.y, z: point.z, flag: 0 });
        }
        helper.destination = *points.last().expect("checked");
        self.control_install(helper.control, path, true, false, unit);
        helper.state = 8.0;
        helper.pending = 0.0;
    }

    /// goto.js:130.
    pub fn ground_follow(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld, target: Option<UnitRef>, distance: f64, remembered: bool) {
        self.goto_attach(helper, unit);
        helper.follow_distance = distance;
        helper.follow_remembered = remembered;
        // resolve(this.target) === resolve(ref), unit identity or both null.
        let same_target = {
            let ra = helper.target.and_then(|a| world.resolve(a)).map(|r| r.id);
            let rb = target.and_then(|b| world.resolve(b)).map(|r| r.id);
            ra == rb
        };
        if helper.state == 5.0 && same_target {
            self.ground_flush(helper, unit, world);
            return;
        }
        self.ground_drop_route(helper);
        self.control_strand(helper.control, false, Some(unit));
        helper.target = target;
        if is_soldier_or_droid(unit) {
            helper.pending = 5.0;
        }
        self.ground_flush(helper, unit, world);
    }

    /// goto.js:147.
    pub fn ground_tick(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld, dt: f64) {
        self.goto_attach(helper, unit);
        for _ in 0..8 {
            self.ground_flush(helper, unit, world);
            self.ground_work(helper, unit, world, dt);
            if helper.pending == 0.0 {
                break;
            }
        }
        self.ground_flush(helper, unit, world);
    }

    /// goto.js:157.
    pub fn ground_arrived(&self, helper: &GroundGoto) -> bool {
        let c = &self.controls[helper.control];
        match helper.state {
            2.0 | 5.0 => false,
            3.0 | 4.0 => c.executor.is_none() && same(c.request.waypoint, helper.destination),
            7.0 => true,
            _ => match &helper.cursor {
                None => c.executor.is_none(),
                Some(cursor) => {
                    if c.executor.is_some() {
                        false
                    } else {
                        same(cursor.waypoint, helper.destination)
                    }
                }
            },
        }
    }

    /// goto.js:184; `current` defaults as written to state === 7.
    pub fn ground_destination_point(&mut self, helper: &GroundGoto, unit: &NavUnit, world: &mut dyn GotoWorld, current: Option<bool>) -> Vec3 {
        let current = current.unwrap_or(helper.state == 7.0);
        if !current {
            if helper.state != 5.0 {
                return helper.destination;
            }
            if let Some(point) = self.ground_follow_point(helper, unit, world) {
                return point;
            }
        }
        unit.position
    }

    /// goto.js:193.
    pub fn ground_stance(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld, value: f64) {
        self.controls[helper.control].stance = value;
        self.ground_flush(helper, unit, world);
    }

    /// goto.js:200.
    pub fn ground_jet(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld, vector: Vec3) {
        let c = &mut self.controls[helper.control];
        c.jet.pending = true;
        c.jet.vector = vector;
        self.ground_flush(helper, unit, world);
    }

    /// goto.js:209 (the controller branch: controller.js:613).
    pub fn ground_aim_at(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld, point: Vec3) {
        helper.aim_state = 1.0;
        helper.aim_point = point;
        self.ground_flush(helper, unit, world);
    }

    /// goto.js:219 (controller.js:618: aimTarget = ref ?? null).
    pub fn ground_aim_at_target(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld, target: Option<UnitRef>) {
        helper.aim_state = 0.0;
        helper.aim_target = target;
        self.ground_flush(helper, unit, world);
    }

    /// goto.js:229.
    pub fn ground_aim_along_path(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld) {
        helper.aim_state = 2.0;
        self.ground_flush(helper, unit, world);
    }

    fn ground_flush(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld) {
        for _ in 0..16 {
            if helper.pending == 0.0 {
                break;
            }
            let next = helper.pending;
            helper.pending = 0.0;
            helper.state = next;
            self.ground_enter(helper, unit, world, next);
        }
    }

    fn ground_enter(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld, state: f64) {
        match state {
            2.0 | 7.0 => self.control_stop(helper.control, Some(unit)),
            3.0 => {
                helper.retries = 0.0;
                helper.adjusted = false;
                let destination = helper.destination;
                self.ground_direct_path(helper, unit, destination);
            }
            5.0 => {
                helper.retries = 0.0;
                helper.adjusted = false;
                helper.last_refresh = -100.0;
                helper.last_target = Vec3::new(-FLT_MAX, 0.0, -FLT_MAX);
                if !self.ground_refresh_follow(helper, unit, world) {
                    helper.pending = 7.0;
                }
            }
            4.0 => {
                helper.detour_point = helper.destination;
                helper.detour_side = 0.0;
                helper.detour_final = Vec3::default();
                self.ground_local_detour(helper, unit);
            }
            _ => {}
        }
    }

    fn ground_work(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld, dt: f64) {
        let _ = dt;
        match helper.state {
            2.0 => self.ground_process_path(helper, unit, world, false),
            3.0 => {
                self.ground_jet_attempt(helper, unit, world);
                if self.controls[helper.control].executor.is_none() {
                    if self.ground_arrived(helper) || {
                        helper.retries += 1.0;
                        helper.retries > 5.0
                    } {
                        helper.pending = 7.0;
                    } else if helper.adjusted {
                        let target = self.ground_sidestep(helper, unit, world);
                        self.ground_direct_path(helper, unit, target);
                    } else {
                        let destination = helper.destination;
                        self.ground_direct_path(helper, unit, destination);
                    }
                }
            }
            4.0 => {
                self.ground_jet_attempt(helper, unit, world);
                if self.controls[helper.control].executor.is_none() {
                    if self.ground_arrived(helper) {
                        helper.pending = 7.0;
                        return;
                    }
                    helper.detour_point = helper.destination;
                }
                self.ground_local_detour(helper, unit);
            }
            5.0 => {
                self.ground_jet_attempt(helper, unit, world);
                if !self.ground_refresh_follow(helper, unit, world) {
                    helper.pending = 7.0;
                }
            }
            6.0 => {
                self.ground_jet_attempt(helper, unit, world);
                if self.controls[helper.control].executor.is_some() {
                    self.ground_process_path(helper, unit, world, true);
                } else {
                    helper.pending = if helper.cursor.is_some() { 2.0 } else { 7.0 };
                }
            }
            7.0 => self.control_stop(helper.control, Some(unit)),
            8.0 => {
                self.ground_jet_attempt(helper, unit, world);
                if self.controls[helper.control].executor.is_none() {
                    helper.pending = 7.0;
                }
            }
            _ => {}
        }
    }

    fn ground_drop_route(&mut self, helper: &mut GroundGoto) {
        helper.cursor = None;
        self.planner_cancel(helper.control);
    }

    fn ground_route(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld, local: bool, point: Vec3) {
        let start = unit.position;
        let mut dest = point;
        let (radius, mask) = (traversal_radius(unit), traversal_mask(unit));
        self.barriers.correct(&mut dest, radius, mask);
        helper.destination = dest;
        helper.failed = false;
        self.controls[helper.control].segments = 0;
        let far = h2(unit.position.x, unit.position.z, helper.destination.x, helper.destination.z) >= 2500.0;
        let graph_ok = self.graph.as_ref().map(|g| g.table(mask) >= 0).unwrap_or(false);
        if !local && far && graph_ok {
            self.ground_graph_route(helper, unit, world, start);
        } else {
            let destination = helper.destination;
            self.ground_local_leg(helper, unit, start, destination, 0.0);
        }
        self.control_stop(helper.control, Some(unit));
    }

    fn ground_graph_route(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld, start: Vec3) {
        let (radius, mask) = (traversal_radius(unit), traversal_mask(unit));
        let clearance = if unit.kind == kind::DROIDEKA { 4.0 } else { 0.0 };
        let destination = helper.destination;
        let mut cursor = GraphCursor::new();
        {
            let graph = self.graph.as_mut().expect("a graph route has a graph");
            cursor.init(graph, start, destination, mask, clearance, world.random_a());
        }
        if !self.barriers.point_clear(cursor.waypoint, radius, mask).0 {
            let Nav { graph, barriers, .. } = self;
            let graph = graph.as_ref().expect("a graph route has a graph");
            cursor.retry(graph, barriers, radius, world.random_a());
        }
        let waypoint = cursor.waypoint;
        helper.cursor = Some(cursor);
        self.ground_local_leg(helper, unit, start, waypoint, 1.0);
    }

    fn ground_local_leg(&mut self, helper: &GroundGoto, unit: &NavUnit, start: Vec3, end: Vec3, priority: f64) {
        let (radius, mask) = (traversal_radius(unit), traversal_mask(unit));
        let mut from = start;
        self.barriers.correct(&mut from, radius, mask);
        self.ground_request(helper, unit, from, end, priority);
    }

    fn ground_request(&mut self, helper: &GroundGoto, unit: &NavUnit, start: Vec3, end: Vec3, priority: f64) {
        let (radius, mask) = (traversal_radius(unit), traversal_mask(unit));
        if !self.planner_request(helper.control, start, end, radius, mask, priority) {
            self.control_publish(helper.control, None, true);
        }
    }

    fn ground_process_path(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld, chained: bool) {
        let (ready, success) = self.control_consume(helper.control, if chained { 1.0 } else { 0.0 }, unit);
        if !ready {
            if !chained {
                self.control_stop(helper.control, Some(unit));
            }
            return;
        }
        if !success {
            helper.failed = true;
            helper.cursor = None;
            self.control_strand(helper.control, true, Some(unit));
        } else if helper.cursor.is_some() {
            let (radius, mask) = (traversal_radius(unit), traversal_mask(unit));
            let waypoint = helper.cursor.as_ref().expect("checked").waypoint;
            if same(waypoint, helper.destination) {
                helper.cursor = None;
            } else {
                let start = waypoint;
                let advanced = {
                    let Nav { graph, .. } = self;
                    let graph = graph.as_mut().expect("a cursor has a graph");
                    helper.cursor.as_mut().expect("checked").advance(graph, world.random_a())
                };
                if !advanced {
                    self.control_strand(helper.control, true, Some(unit));
                    helper.pending = 7.0;
                    return;
                }
                let mut end = helper.cursor.as_ref().expect("checked").waypoint;
                let mut lost = false;
                if !self.barriers.point_clear(end, radius, mask).0 {
                    let retried = {
                        let Nav { graph, barriers, .. } = self;
                        let graph = graph.as_ref().expect("a cursor has a graph");
                        helper.cursor.as_mut().expect("checked").retry(graph, barriers, radius, world.random_a())
                    };
                    if retried {
                        end = helper.cursor.as_ref().expect("checked").waypoint;
                    } else {
                        lost = true;
                    }
                }
                if lost {
                    helper.cursor = None;
                    self.control_strand(helper.control, true, Some(unit));
                } else {
                    self.ground_request(helper, unit, start, end, 1.0);
                }
            }
        }
        if !chained {
            helper.pending = 6.0;
        }
    }

    fn ground_goto_direct(&mut self, helper: &mut GroundGoto, _unit: &NavUnit, point: Vec3) {
        if helper.state == 5.0 && same(helper.destination, point) {
            return;
        }
        self.ground_drop_route(helper);
        helper.destination = point;
        helper.follow_distance = 0.0;
        helper.pending = 3.0;
    }

    fn ground_goto_detour(&mut self, helper: &mut GroundGoto, point: Vec3) {
        if same(helper.destination, point) {
            return;
        }
        self.ground_drop_route(helper);
        helper.destination = point;
        helper.follow_distance = 0.0;
        helper.pending = 4.0;
    }

    fn ground_goto_straight(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, point: Vec3) {
        self.ground_drop_route(helper);
        let p = unit.position;
        let path = self.store.segment(p, point);
        self.control_install(helper.control, path, true, false, unit);
        helper.pending = 8.0;
    }

    fn ground_direct_path(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, target: Vec3) {
        let p = unit.position;
        let mut start = p;
        let mut end = target;
        let (radius, mask) = (traversal_radius(unit), traversal_mask(unit));
        self.barriers.correct(&mut start, radius, mask);
        let mut normal = helper.normal;
        helper.adjusted = self.barriers.obstruct(start, &mut end, radius, mask, None, Some(&mut normal));
        helper.normal = normal;
        if helper.adjusted {
            pull_back(start, &mut end);
        }
        self.ground_drop_route(helper);
        let path = self.store.segment(start, end);
        self.control_install(helper.control, path, true, false, unit);
    }

    fn ground_sidestep(&mut self, helper: &GroundGoto, unit: &NavUnit, world: &mut dyn GotoWorld) -> Vec3 {
        let r = world.random_a();
        let distance = r.int(10.0, 20.0);
        let lo = f32(std::f64::consts::PI / 4.0);
        let hi = f32(std::f64::consts::PI / 2.0);
        let angle = f32(r.uniform() * (hi - lo) + lo);
        let side = if r.int(0.0, 1.0) != 0.0 { -1.0 } else { 1.0 };
        let a = side * angle;
        let s = v8math::sin(a);
        let c = v8math::cos(a);
        let n = helper.normal;
        let x = n.x * c - n.z * s;
        let z = n.z * c + n.x * s;
        let p = unit.position;
        Vec3::new(p.x + x * distance, p.y + n.y * distance, p.z + z * distance)
    }

    fn ground_local_detour(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit) {
        let mask = traversal_mask(unit);
        let r = f32(2.0 * traversal_radius(unit));
        let mut start = unit.position;
        self.barriers.correct(&mut start, r, mask);
        for _ in 0..5 {
            let reach = v8math::max(1.0, v8math::min(30.0, 1.5 * nominal_speed(unit)));
            let point = helper.detour_point;
            let dx = point.x - start.x;
            let dz = point.z - start.z;
            let mut probe = point;
            if reach * reach < dx * dx + dz * dz {
                let dir = flat_dir(dx, dz);
                probe = Vec3::new(start.x + reach * dir.x, start.y, start.z + reach * dir.z);
            }
            let (hit, barrier, _) = self.barriers.nearest(start, probe, r, mask, None);
            if !hit {
                break;
            }
            let barrier = &self.barriers.list[barrier.expect("nearest names its barrier")];
            let (mut a, mut b) = (Vec3::default(), Vec3::default());
            let sides = super::barriers::detour(barrier, start, probe, r, &mut a, &mut b);
            let chosen;
            if helper.detour_side == 0.0 {
                let da = if sides & 1 != 0 { h2(a.x, a.z, start.x, start.z) } else { f64::INFINITY };
                let db = if sides & 2 != 0 { h2(b.x, b.z, start.x, start.z) } else { f64::INFINITY };
                if da == f64::INFINITY && db == f64::INFINITY {
                    break;
                }
                if db <= da {
                    helper.detour_side = 2.0;
                    chosen = b;
                } else {
                    helper.detour_side = 1.0;
                    chosen = a;
                }
            } else {
                chosen = if helper.detour_side == 1.0 { a } else { b };
                if sides & (helper.detour_side as u32) == 0 {
                    break;
                }
            }
            helper.detour_point = chosen;
        }
        if same(helper.detour_point, helper.detour_final) {
            return;
        }
        self.ground_drop_route(helper);
        let path = self.store.segment(start, helper.detour_point);
        self.control_install(helper.control, path, true, false, unit);
        helper.detour_final = helper.detour_point;
    }

    fn ground_follow_point(&mut self, helper: &GroundGoto, unit: &NavUnit, world: &mut dyn GotoWorld) -> Option<Vec3> {
        let target = helper.target.and_then(|t| world.resolve(t))?;
        if !helper.follow_remembered {
            let tp = target.position;
            let p = unit.position;
            let dir = flat_dir(p.x - tp.x, p.z - tp.z);
            return Some(Vec3::new(tp.x + dir.x * helper.follow_distance, tp.y, tp.z + dir.z * helper.follow_distance));
        }
        world.remembered(target.id)
    }

    fn ground_refresh_follow(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld) -> bool {
        let Some(mut point) = self.ground_follow_point(helper, unit, world) else {
            return false;
        };
        let target = helper.target.and_then(|t| world.resolve(t));
        if let Some(target) = target {
            if target.id == unit.id {
                self.control_stop(helper.control, Some(unit));
                return true;
            }
        }
        if h2(point.x, point.z, helper.last_target.x, helper.last_target.z) > 1.0 {
            let throttle = if h2(unit.position.x, unit.position.z, point.x, point.z) > 25.0 { 3.0 } else { 1.5 };
            let now = world.time();
            if helper.last_refresh + throttle < now {
                helper.last_refresh = now;
                if let Some(v) = target.and_then(|t| t.velocity) {
                    point = Vec3::new(point.x + v.x, point.y + v.y, point.z + v.z);
                }
                self.ground_direct_path(helper, unit, point);
                helper.last_target = point;
            }
        }
        if self.controls[helper.control].executor.is_none() && h2(unit.position.x, unit.position.z, point.x, point.z) > 4.0 {
            self.control_strand(helper.control, true, Some(unit));
        }
        true
    }

    fn ground_jet_attempt(&mut self, helper: &mut GroundGoto, unit: &mut NavUnit, world: &mut dyn GotoWorld) {
        if !is_soldier(unit) || !has_jetpack(unit) {
            return;
        }
        let soldier = unit.soldier.as_ref().expect("jetAttempt reads the soldier of a Soldier");
        let border = unit
            .class_def
            .as_ref()
            .and_then(|d| d.jet_fuel_min_border)
            .or_else(|| soldier.jet.and_then(|j| j.min_border))
            .unwrap_or(0.0);
        let now = world.time();
        if !(soldier.fuel > border) {
            helper.jet_deadline = f32(now + 5.0 + f32(world.random_b().uniform() * 10.0));
            return;
        }
        if self.controls[helper.control].jetting {
            return;
        }
        if !(now > helper.jet_deadline) {
            return;
        }
        helper.jet_deadline = f32(helper.jet_deadline + 5.0);
        if world.random_b().next() & 1 == 0 {
            return;
        }
        let mut q = self.ground_destination_point(helper, unit, world, Some(false));
        if !(d2(unit.position, q) > 400.0) {
            return;
        }
        if !world.clear_above(unit.position, unit) {
            return;
        }
        q = self.ground_destination_point(helper, unit, world, Some(false));
        if d2(q, helper.destination) < tiny() {
            q = self.controls[helper.control].request.steering_target;
        }
        if !world.clear_above(q, unit) {
            return;
        }
        self.ground_jet(helper, unit, world, Vec3::default());
    }

    // ----- FlyGotoHelper (goto.js:605) -----

    /// goto.js:624.
    pub fn fly_goto(&mut self, helper: &mut FlyGoto, unit: &mut NavUnit, point: Vec3, mode: GotoMode) {
        self.control_attach(helper.control, Some(unit));
        if mode != GotoMode::Route {
            self.planner_cancel(helper.control);
            helper.destination = point;
            helper.pending = 3.0;
        } else {
            let mask = traversal_mask(unit);
            let radius = traversal_radius(unit);
            let mut dest = point;
            self.barriers.correct(&mut dest, radius, mask);
            helper.destination = dest;
            helper.failed = false;
            self.controls[helper.control].segments = 0;
            let mut from = unit.position;
            self.barriers.correct(&mut from, radius, mask);
            self.planner_request(helper.control, from, dest, radius, mask, 0.0);
            self.control_stop(helper.control, Some(unit));
            helper.pending = 2.0;
        }
        self.fly_flush(helper, unit);
    }

    /// goto.js:648.
    pub fn fly_stop(&mut self, helper: &mut FlyGoto, unit: &mut NavUnit) {
        helper.pending = 5.0;
        self.fly_flush(helper, unit);
    }

    fn fly_flush(&mut self, helper: &mut FlyGoto, unit: &mut NavUnit) {
        for _ in 0..16 {
            if helper.pending == 0.0 {
                break;
            }
            let next = helper.pending;
            helper.pending = 0.0;
            helper.state = next;
            if next == 2.0 || next == 5.0 {
                self.control_stop(helper.control, Some(unit));
            } else if next == 3.0 {
                let p = unit.position;
                let path = self.store.segment(p, helper.destination);
                self.control_install(helper.control, path, false, false, unit);
            }
        }
    }

    /// goto.js:666.
    pub fn fly_tick(&mut self, helper: &mut FlyGoto, unit: &mut NavUnit) {
        self.control_attach(helper.control, Some(unit));
        self.fly_flush(helper, unit);
        if helper.state == 2.0 {
            let (ready, success) = self.control_consume(helper.control, 0.0, unit);
            if ready {
                helper.failed = !success;
                helper.pending = if success { 4.0 } else { 5.0 };
            } else {
                self.control_stop(helper.control, Some(unit));
            }
        } else if helper.state == 5.0 {
            self.control_stop(helper.control, Some(unit));
        } else if (helper.state == 3.0 || helper.state == 4.0) && self.fly_arrived(helper) {
            helper.pending = 5.0;
        }
        self.fly_flush(helper, unit);
    }

    /// goto.js:685.
    pub fn fly_arrived(&self, helper: &FlyGoto) -> bool {
        let c = &self.controls[helper.control];
        if helper.state == 2.0 {
            return false;
        }
        if helper.state == 3.0 {
            return c.executor.is_none() && same(helper.destination, c.request.waypoint);
        }
        if helper.state == 5.0 {
            return true;
        }
        c.executor.is_none()
    }
}

/// Shorten the end 1 m toward the start (goto.js:707).
fn pull_back(start: Vec3, end: &mut Vec3) {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let dz = end.z - start.z;
    let len2 = dx * dx + dy * dy + dz * dz;
    if len2 > 0.1 && len2 > 1.0 {
        let l = v8math::sqrt(len2);
        end.x -= dx / l;
        end.y -= dy / l;
        end.z -= dz / l;
        return;
    }
    *end = start;
}
