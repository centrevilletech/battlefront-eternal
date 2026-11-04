//! Classic AI navigation (src/ai/classic/nav/*): the `Nav` service the
//! rest of the AI drives (index.js ClassicNav), holding the loaded
//! graph, barriers, hints and boundary, the local path planner
//! (planner.js, search.js) and every unit's NavControl (control.js).
//!
//! Ownership follows the plan's staged transfer: the service owns the
//! navigation state, and the units it steers are the snapshots of
//! units.rs that callers fill per call. Controls are indices into
//! `controls` where the JavaScript holds NavControl objects per
//! controller; paths are indices into the store where it holds AIPath
//! objects. Draws come from generators the caller hands in, in the
//! order the JavaScript draws them.

pub mod analogs;
pub mod barriers;
pub mod boundary;
pub mod control;
pub mod cursor;
pub mod data;
pub mod executor;
pub mod goto;
pub mod graph;
pub mod hints;
pub mod jet_hints;
pub mod math;
pub mod planner;
pub mod search;
#[cfg(test)]
mod tests;
pub mod units;

use barriers::{BarrierSet, Work};
use boundary::Boundary;
use control::NavControl;
use data::{load_nav, NavData, NavPathData};
use executor::{ExecutorHost, PathExecutor};
use graph::NavGraph;
use hints::{HintPosts, HintSet};
use math::{d2, f32, Vec3};
use planner::{PathStore, STARVED};
use search::BarrierSearch;
use units::{kind, NavUnit, UnitRef};

use bfe_content::Value;

/// The planner's options (planner.js:94-105, index.js:30's pools).
#[derive(Clone, Copy, Debug)]
pub struct PlannerOptions {
    pub paths: f64,
    pub path_nodes: f64,
    pub requests: f64,
    pub search_nodes: Option<f64>,
    pub heap: Option<f64>,
}

impl Default for PlannerOptions {
    fn default() -> Self {
        PlannerOptions { paths: 900.0, path_nodes: 7200.0, requests: 75.0, search_nodes: None, heap: None }
    }
}

/// One queued planner request (planner.js:133). `owner` is a control.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Request {
    pub owner: usize,
    pub serial: f64,
    pub start: Vec3,
    pub end: Vec3,
    pub radius: f64,
    pub mask: u32,
    pub priority: f64,
    pub served: f64,
}

/// ClassicNav (index.js:17) with its PathPlanner (planner.js:93) and
/// controls inlined: their methods are the ones below.
pub struct Nav {
    pub map: Value,
    pub source: Value,
    pub graph: Option<NavGraph>,
    pub barriers: BarrierSet,
    pub hints: HintSet,
    pub boundary: Boundary,
    pub paths: Vec<NavPathData>,
    pub regions: Value,
    pub warnings: Vec<Value>,
    // The planner (planner.js:93).
    pub store: PathStore,
    pub queue: Vec<Request>,
    pub current: Option<Request>,
    pub search: BarrierSearch,
    pub work: Work,
    pub served: f64,
    capacity: f64,
    // Per-controller navigation state (control.js), by index.
    pub controls: Vec<NavControl>,
    /// Publications for an external control facade during the ownership
    /// transition. Native controls use their answer slots directly.
    pub publications: Option<Vec<(usize, Option<usize>, bool)>>,
}

impl Nav {
    /// index.js:18. `hint_capacity` is loadNav's option of that name.
    pub fn new(json: &Value, bin: Option<&[u8]>, posts: Option<&dyn HintPosts>, hint_capacity: Option<f64>, pools: PlannerOptions) -> Result<Self, String> {
        let data = load_nav(json, bin, posts, hint_capacity)?;
        Ok(Self::from_data(data, pools))
    }

    fn from_data(data: NavData, pools: PlannerOptions) -> Self {
        Nav {
            map: data.map,
            source: data.source,
            graph: data.graph,
            barriers: data.barriers,
            hints: data.hints,
            boundary: data.boundary,
            paths: data.paths,
            regions: data.regions,
            warnings: data.warnings,
            store: PathStore::new(pools.paths, pools.path_nodes),
            queue: Vec::new(),
            current: None,
            search: BarrierSearch::new(pools.search_nodes, pools.heap),
            work: Work::default(),
            served: 0.0,
            capacity: pools.requests,
            controls: Vec::new(),
            publications: None,
        }
    }

    /// Navigation for a map with no extracted ai-nav.json (index.js:36).
    pub fn empty(pools: PlannerOptions) -> Self {
        Nav {
            map: Value::Null,
            source: Value::Null,
            graph: None,
            barriers: BarrierSet::new(Vec::new()),
            hints: HintSet::default(),
            boundary: Boundary::default(),
            paths: Vec::new(),
            regions: Value::Null,
            warnings: Vec::new(),
            store: PathStore::new(pools.paths, pools.path_nodes),
            queue: Vec::new(),
            current: None,
            search: BarrierSearch::new(pools.search_nodes, pools.heap),
            work: Work::default(),
            served: 0.0,
            capacity: pools.requests,
            controls: Vec::new(),
            publications: None,
        }
    }

    // ----- the planner (planner.js) -----

    /// A new path in the store (planner.js:108).
    pub fn path_new(&mut self) -> usize {
        self.store.make()
    }

    /// Two-point path start -> end (planner.js:113).
    pub fn path_segment(&mut self, start: Vec3, end: Vec3) -> usize {
        self.store.segment(start, end)
    }

    fn live(&self, request: &Request) -> bool {
        let owner = &self.controls[request.owner];
        owner.serial == request.serial && !owner.disposed
    }

    /// Ask for a local path (planner.js:122).
    pub fn planner_request(&mut self, owner: usize, start: Vec3, end: Vec3, radius: f64, mask: u32, priority: f64) -> bool {
        let segments = self.controls[owner].segments as f64;
        // (priority + owner.segments) & 0xff, as a JavaScript number.
        let total = (v8math::to_int32(priority + segments) & 0xff) as f64;
        self.planner_cancel(owner);
        if segments <= 10.0 && !self.barriers.segment_blocked(start, end, radius, mask, None) {
            let path = self.store.segment(start, end);
            self.control_publish(owner, Some(path), true);
            return true;
        }
        if self.queue.len() as f64 >= self.capacity {
            return false;
        }
        let serial = self.controls[owner].serial;
        self.queue.push(Request { owner, serial, start, end, radius, mask, priority: total, served: self.served });
        true
    }

    /// Forget the owner's pending work (planner.js:152).
    pub fn planner_cancel(&mut self, owner: usize) {
        self.control_publish(owner, None, false);
        if self.current.map(|c| c.owner) == Some(owner) {
            self.search.abort();
            self.current = None;
        }
        if let Some(i) = self.queue.iter().position(|r| r.owner == owner) {
            self.queue.remove(i);
        }
    }

    /// An outstanding segment was consumed (planner.js:164).
    pub fn planner_promote(&mut self, owner: usize) {
        if self.current.map(|c| c.owner) == Some(owner) {
            return;
        }
        if let Some(r) = self.queue.iter_mut().find(|q| q.owner == owner) {
            if r.priority != 0.0 {
                r.priority -= 1.0;
            }
        }
    }

    fn select(&mut self) -> Option<Request> {
        let mut i = 0;
        while i < self.queue.len() {
            let r = self.queue[i];
            if r.priority > 10.0 || self.served - r.served < STARVED || !self.live(&r) {
                i += 1;
                continue;
            }
            return Some(self.queue.remove(i));
        }
        for pnum in 0..=10 {
            let p = pnum as f64;
            let mut i = 0;
            while i < self.queue.len() {
                let r = self.queue[i];
                if r.priority != p {
                    i += 1;
                    continue;
                }
                let r = self.queue.remove(i);
                if self.live(&r) {
                    return Some(r);
                }
            }
        }
        None
    }

    /// No search in progress and nothing queued (planner.js:194).
    pub fn planner_idle(&self) -> bool {
        self.search.finished && self.queue.is_empty()
    }

    /// One planner service (planner.js:199); index.js:42 offers 1.0.
    pub fn service(&mut self, budget: f64) {
        self.work.cap = v8math::trunc(f32(f32(f32(budget * 10.0) * 5.0) / 5.0));
        self.work.count = 0.0;
        while self.work.count < self.work.cap {
            if self.search.finished {
                self.current = self.select();
                let Some(r) = self.current else { break };
                // The search borrows the barriers and the store's pools.
                let Nav { barriers, store, search, .. } = self;
                search.begin(barriers, store, r.start, r.end, r.radius, r.mask);
            }
            if !self.search.finished {
                let Nav { barriers, search, work, .. } = self;
                search.step(barriers, work);
            }
            if self.search.finished {
                let Nav { store, search, .. } = self;
                let path = search.reconstruct(store);
                match self.current {
                    Some(r) if self.live(&r) => {
                        let owner = r.owner;
                        self.control_publish(owner, path, true);
                    }
                    _ => {
                        if let Some(path) = path {
                            self.store.release(path);
                        }
                    }
                }
                self.current = None;
                self.served += 1.0;
            }
        }
    }

    // ----- controls (control.js) -----

    /// A controller's navigation state, created on first use
    /// (index.js:52): the caller keeps the index as it keeps
    /// controller.navControl in the JavaScript.
    pub fn new_control(&mut self) -> usize {
        self.controls.push(NavControl::new());
        self.controls.len() - 1
    }

    /// Bind to the unit now driven (control.js:93). `unit` is None where
    /// the JavaScript's controller has no unit.
    pub fn control_attach(&mut self, control: usize, unit: Option<&NavUnit>) {
        let same = {
            let c = &self.controls[control];
            unit.map(|u| u.id) == c.unit && unit.and_then(|u| u.generation) == Some(c.generation)
        };
        if same {
            return;
        }
        self.control_reset(control);
        let c = &mut self.controls[control];
        c.unit = unit.map(|u| u.id);
        c.generation = unit.and_then(|u| u.generation).unwrap_or(-1.0);
    }

    /// control.js:101.
    pub fn control_reset(&mut self, control: usize) {
        self.controls[control].serial += 1.0;
        self.planner_cancel(control);
        self.control_drop_executor(control);
        let trail = {
            let c = &mut self.controls[control];
            c.request.clear();
            c.segments = 0;
            c.jet.pending = false;
            c.jet.vector = Vec3::default();
            c.jetting = false;
            c.stance = 0.0;
            c.avoid_side = 0.0;
            c.stranded = false;
            c.stranded_for = 0.0;
            c.stranded_at = Vec3::new(f64::INFINITY, 0.0, 0.0);
            c.landing = false;
            c.leader = None;
            c.trail_delay = 0.0;
            c.trail.take()
        };
        if let Some(trail) = trail {
            self.store.release(trail);
        }
    }

    /// control.js:123, with the unit's position (None without a unit).
    pub fn control_strand(&mut self, control: usize, on: bool, unit: Option<&NavUnit>) {
        let c = &mut self.controls[control];
        if !on {
            c.stranded = false;
            return;
        }
        if c.stranded {
            return;
        }
        c.stranded = true;
        if let Some(u) = unit {
            if d2(u.position, c.stranded_at) <= 4.0 {
                return;
            }
            c.stranded_for = 0.0;
            c.stranded_at = u.position;
        } else {
            c.stranded_for = 0.0;
        }
    }

    /// control.js:133.
    pub fn control_dispose(&mut self, control: usize) {
        self.control_reset(control);
        self.controls[control].disposed = true;
    }

    /// The planner's answer slot (control.js:140).
    pub fn control_publish(&mut self, control: usize, path: Option<usize>, ready: bool) {
        let old = self.controls[control].published;
        if old.is_some() && old != path {
            self.store.release(old.expect("checked"));
        }
        let c = &mut self.controls[control];
        c.published = path;
        c.ready = ready;
        if let Some(publications) = &mut self.publications { publications.push((control, path, ready)); }
    }

    /// Take a finished answer (control.js:149): (ready, success).
    pub fn control_consume(&mut self, control: usize, mode: f64, unit: &mut NavUnit) -> (bool, bool) {
        if !self.controls[control].ready {
            return (false, false);
        }
        let Some(path) = self.controls[control].published else {
            return (true, false);
        };
        self.controls[control].published = None;
        if mode == 0.0 || self.controls[control].executor.is_none() {
            self.control_install(control, path, true, false, unit);
        } else {
            self.control_append(control, path, unit);
        }
        self.controls[control].ready = false;
        (true, true)
    }

    /// A fresh executor on path (control.js:161).
    pub fn control_install(&mut self, control: usize, path: usize, a: bool, b: bool, unit: &mut NavUnit) {
        self.control_drop_executor(control);
        self.control_strand(control, false, Some(unit));
        {
            let c = &mut self.controls[control];
            c.segments = 1;
            c.install_flags = [a, b];
            c.landing = false;
        }
        let Nav { store, controls, queue, current, .. } = self;
        let mut host = ControlHost { controls, queue, current, control };
        let executor = PathExecutor::new(store, &mut host, path, control, unit);
        host.controls[control].executor = Some(executor);
    }

    /// Continue the current executor with path (control.js:172).
    pub fn control_append(&mut self, control: usize, path: usize, unit: &mut NavUnit) {
        self.controls[control].segments = self.controls[control].segments.wrapping_add(1) & 0xff;
        let mut executor = self.controls[control].executor.take().expect("append with an executor");
        let Nav { store, controls, queue, current, .. } = self;
        let mut host = ControlHost { controls, queue, current, control };
        executor.append(store, &mut host, path, unit, true);
        // The JavaScript's executor stayed installed throughout; putting
        // it back is invisible outside this call.
        if host.controls[control].executor.is_none() {
            host.controls[control].executor = Some(executor);
        }
    }

    /// Drop the path and stand still (control.js:179).
    pub fn control_stop(&mut self, control: usize, unit: Option<&NavUnit>) {
        self.control_drop_executor(control);
        let Some(u) = unit else { return };
        let p = u.position;
        let yaw = u.soldier.as_ref().and_then(|s| s.aim_yaw).unwrap_or(0.0);
        let dir = Vec3::new(v8math::sin(yaw), 0.0, v8math::cos(yaw));
        self.controls[control].request.write(p, p, dir, 0.0, -1.0, 1.0);
    }

    pub fn control_drop_executor(&mut self, control: usize) {
        let Some(executor) = self.controls[control].executor.take() else {
            return;
        };
        executor.release(&mut self.store);
    }

    /// Follow's leader (control.js:195).
    pub fn control_set_leader(&mut self, control: usize, leader: Option<UnitRef>) {
        self.control_drop_executor(control);
        let trail = if leader.is_none() {
            let c = &mut self.controls[control];
            c.leader = None;
            c.trail.take()
        } else {
            let c = &mut self.controls[control];
            c.leader = leader;
            c.trail_delay = 0.0;
            None
        };
        if let Some(trail) = trail {
            self.store.release(trail);
        }
    }

    /// A flagged node was consumed (control.js:246).
    pub fn control_consume_segment(&mut self, control: usize) {
        self.controls[control].segments = self.controls[control].segments.wrapping_sub(1) & 0xff;
        self.planner_promote(control);
    }

    /// Executor progress status (control.js:272).
    pub fn control_status(&self, control: usize, unit: Option<&NavUnit>) -> f64 {
        let c = &self.controls[control];
        if let Some(e) = &c.executor {
            return e.status;
        }
        if !c.stranded {
            return 0.0;
        }
        let Some(kind_of) = unit.map(|u| u.kind) else { return 0.0 };
        if kind_of != kind::SOLDIER && kind_of != kind::DROIDEKA {
            return 0.0;
        }
        let limit = if kind_of == kind::DROIDEKA { 6.0 } else { 5.0 };
        if limit < c.stranded_for {
            1.0
        } else {
            0.0
        }
    }

    /// control.js:281.
    pub fn control_following(&self, control: usize) -> bool {
        self.controls[control].executor.is_some()
    }

    // ----- the service's queries (index.js) -----

    /// No search running and nothing queued (index.js:47).
    pub fn idle(&self) -> bool {
        self.planner_idle()
    }

    /// Does the boundary leave this point outside? (index.js:65)
    pub fn outside(&self, point: Vec3) -> bool {
        self.boundary.outside(point)
    }

    /// An authored path by name (index.js:69).
    pub fn path_by_name(&self, name: &str) -> Option<usize> {
        self.paths.iter().position(|p| p.name.as_deref() == Some(name))
    }

    /// The hint query (index.js:75).
    pub fn query_hints(&self, mask: u32, center: Vec3, half_extent: f64, cap: f64, requester: Option<u64>, now: f64) -> Vec<usize> {
        self.hints.query(mask, center, half_extent, cap, requester, now)
    }

    pub fn hint_available(&self, hint: usize, now: f64) -> bool {
        self.hints.available(hint, now)
    }

    pub fn mark_hint(&mut self, hint: usize, owner: Option<u64>, now: f64) {
        self.hints.reserve(hint, owner, now);
    }

    /// The binding's pointClear (index.js:108): the barriers' answer.
    pub fn point_clear(&mut self, p: Vec3, r: f64, mask: u32) -> bool {
        self.barriers.point_clear(p, r, mask).0
    }

    /// The binding's clipSegment (index.js:109).
    pub fn clip_segment(&mut self, s: Vec3, e: &mut Vec3, r: f64, mask: u32, normal: Option<&mut Vec3>) -> bool {
        self.barriers.obstruct(s, e, r, mask, None, normal)
    }
}

/// The executor's host for one control (control.js's consumeSegment,
/// writeRequest and avoidSide, with the planner's promote).
struct ControlHost<'a> {
    controls: &'a mut Vec<NavControl>,
    queue: &'a mut Vec<Request>,
    current: &'a Option<Request>,
    control: usize,
}

impl ExecutorHost for ControlHost<'_> {
    fn consume_segment(&mut self) {
        let control = self.control;
        self.controls[control].segments = self.controls[control].segments.wrapping_sub(1) & 0xff;
        // planner.promote (planner.js:164).
        if self.current.map(|c| c.owner) == Some(control) {
            return;
        }
        if let Some(r) = self.queue.iter_mut().find(|q| q.owner == control) {
            if r.priority != 0.0 {
                r.priority -= 1.0;
            }
        }
    }

    fn write_request(&mut self, target: Vec3, waypoint: Vec3, direction: Vec3, mode: f64, lower: f64, upper: f64) {
        self.controls[self.control].request.write(target, waypoint, direction, mode, lower, upper);
    }

    fn avoid_side(&mut self) -> &mut f64 {
        &mut self.controls[self.control].avoid_side
    }
}
