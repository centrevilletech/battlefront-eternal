//! Following one path (nav/executor.js). The executor's control is
//! reached through `ExecutorHost` (its segment consumption, its
//! movement-request writes and its remembered sidestep side); the Nav
//! service implements it for the control that owns the executor. The
//! unit is the snapshot of units.rs, taken mutably because the
//! sidestep zeroes the soldier's contact vector (executor.js:273).

use super::math::{d2, f32, near_waypoint, normalize, same, tiny, Vec3, FLT_MAX};
use super::planner::{PathNode, PathStore};
use super::units::{kind, NavUnit};

/// The control side of an executor (control.js `consumeSegment`,
/// `writeRequest`, `avoidSide`).
pub trait ExecutorHost {
    fn consume_segment(&mut self);
    fn write_request(&mut self, target: Vec3, waypoint: Vec3, direction: Vec3, mode: f64, lower: f64, upper: f64);
    fn avoid_side(&mut self) -> &mut f64;
}

/// PathExecutor (executor.js:31). `path` is its AIPath in the store.
#[derive(Clone, Debug)]
pub struct PathExecutor {
    pub path: usize,
    pub control: usize,
    pub index: usize,
    pub prev: Vec3,
    pub waypoint: Vec3,
    pub next: Vec3,
    pub fin: Vec3,
    pub direction: Vec3,
    pub mode: f64,
    pub side: f64,
    pub complete: bool,
    pub status: f64,
    pub anchor: Vec3,
    pub stationary: f64,
    pub best: f64,
    pub no_progress: f64,
    pub hold: bool,
    pub target: Vec3,
}

fn node_vec(n: &PathNode) -> Vec3 {
    Vec3::new(n.x, n.y, n.z)
}

impl PathExecutor {
    /// executor.js:34.
    pub fn new(store: &mut PathStore, host: &mut dyn ExecutorHost, path: usize, control: usize, unit: &mut NavUnit) -> Self {
        let mut e = PathExecutor {
            path,
            control,
            index: 0,
            prev: Vec3::default(),
            waypoint: Vec3::default(),
            next: Vec3::default(),
            fin: Vec3::default(),
            direction: Vec3::default(),
            mode: 2.0,
            side: 0.0,
            complete: false,
            status: 0.0,
            anchor: Vec3::default(),
            stationary: 0.0,
            best: FLT_MAX,
            no_progress: 0.0,
            hold: false,
            target: Vec3::default(),
        };
        let len = store.paths[path].nodes.len();
        if len == 0 {
            e.complete = true;
            return e;
        }
        store.paths[path].nodes[len - 1].flag |= 1;
        let first = node_vec(&store.paths[path].nodes[0]);
        if d2(unit.position, first) < 100.0 && (unit.kind == kind::HOVER || unit.kind == kind::FLYER) {
            e.setup(store, host, first, unit);
        } else {
            let start = unit.position;
            e.setup(store, host, start, unit);
        }
        e
    }

    /// The current node (executor.js:67): None past the end, where the
    /// JavaScript reads undefined.
    pub fn node<'a>(&self, store: &'a PathStore) -> Option<&'a PathNode> {
        store.paths[self.path].nodes.get(self.index)
    }

    pub fn last(&self, store: &PathStore) -> bool {
        // executor.js:71: index >= nodes.length - 1, in f64 as written.
        self.index as f64 >= store.paths[self.path].nodes.len() as f64 - 1.0
    }

    /// One tick (executor.js:76).
    pub fn tick(&mut self, store: &mut PathStore, host: &mut dyn ExecutorHost, dt: f64, unit: &mut NavUnit) {
        if store.paths[self.path].nodes.is_empty() {
            return;
        }
        self.progress(dt, unit);
        if !self.hold && (self.reached(store, unit) || self.passed(unit)) {
            self.advance(store, host, unit);
        }
        self.steer(host, unit);
    }

    /// Stuck bookkeeping (executor.js:84).
    pub fn progress(&mut self, dt: f64, unit: &NavUnit) {
        let droideka = unit.kind == kind::DROIDEKA;
        let stationary_limit = if droideka { 6.0 } else { 3.0 };
        let no_progress_limit = if droideka { 6.0 } else { 5.0 };
        self.status = 0.0;
        let p = unit.position;
        let w = self.waypoint;
        let dx = f32(w.x - p.x);
        let dz = f32(w.z - p.z);
        if !(near_waypoint() <= f32(f32(dx * dx) + f32(dz * dz))) {
            return;
        }
        let mut t = 0.0;
        if d2(p, self.anchor) <= 2.0 * 2.0 {
            self.stationary = f32(self.stationary + dt);
            t = self.stationary;
        } else {
            self.anchor = p;
            self.stationary = 0.0;
        }
        if stationary_limit < t {
            self.status = 2.0;
        }
        let dist = f32(d2(p, w));
        if dist < self.best {
            self.best = dist;
            self.no_progress = 0.0;
            return;
        }
        self.no_progress = f32(self.no_progress + dt);
        if no_progress_limit < self.no_progress && self.status == 0.0 {
            self.status = 1.0;
        }
    }

    /// executor.js:113.
    pub fn reached(&self, store: &PathStore, unit: &NavUnit) -> bool {
        let r = if self.last(store) { near_radius(unit) } else { pass_radius(unit) };
        let dx = self.waypoint.x - unit.position.x;
        let dz = self.waypoint.z - unit.position.z;
        dx * dx + dz * dz < r * r
    }

    /// executor.js:122.
    pub fn passed(&self, unit: &NavUnit) -> bool {
        let sx = self.waypoint.x - self.prev.x;
        let sz = self.waypoint.z - self.prev.z;
        if sx * sx + sz * sz < tiny() {
            return true;
        }
        let vx = unit.position.x - self.waypoint.x;
        let vz = unit.position.z - self.waypoint.z;
        if unit.kind == kind::FLYER && vx * vx + vz * vz > 100.0 {
            return false;
        }
        sx * vx + sz * vz > 0.0
    }

    /// Consume one node (executor.js:135).
    pub fn advance(&mut self, store: &mut PathStore, host: &mut dyn ExecutorHost, unit: &mut NavUnit) {
        let flagged = self.node(store).map(|n| n.flag & 1 != 0).unwrap_or(false);
        if !self.complete && flagged {
            host.consume_segment();
        }
        if self.last(store) {
            self.complete = true;
        } else {
            let was = self.waypoint;
            self.index += 1;
            self.setup(store, host, was, unit);
        }
        if self.index > 1 {
            let k = self.index - 1;
            store.drop_front(self.path, k as f64);
            self.index = 1;
        }
        self.best = f32(d2(unit.position, self.waypoint));
        self.no_progress = 0.0;
    }

    /// Segment setup for the current node (executor.js:153).
    pub fn setup(&mut self, store: &mut PathStore, host: &mut dyn ExecutorHost, start: Vec3, unit: &mut NavUnit) {
        self.prev = start;
        self.waypoint = node_vec(self.node(store).expect("setup names a node"));
        self.mode = 2.0;
        self.side = 0.0;
        let len = store.paths[self.path].nodes.len();
        if self.index as f64 >= len as f64 - 1.0 {
            self.next = self.waypoint;
            if !same(self.waypoint, self.prev) {
                self.direction = Vec3::new(self.waypoint.x - self.prev.x, self.waypoint.y - self.prev.y, self.waypoint.z - self.prev.z);
                normalize(&mut self.direction);
                self.mode = 2.0;
            }
        } else {
            self.next = node_vec(&store.paths[self.path].nodes[self.index + 1]);
            if same(unit.position, start) {
                self.direction = Vec3::new(self.next.x - self.waypoint.x, self.next.y - self.waypoint.y, self.next.z - self.waypoint.z);
                normalize(&mut self.direction);
                self.mode = 1.0;
                self.side = 0.0;
            } else if !same(self.next, self.waypoint) {
                let ex = self.next.x - self.waypoint.x;
                let ey = self.next.y - self.waypoint.y;
                let ez = self.next.z - self.waypoint.z;
                self.direction = Vec3::new(ex, ey, ez);
                normalize(&mut self.direction);
                self.mode = 2.0;
                // Zero in exact arithmetic; the float residue picks the side.
                self.side = if self.direction.z * ex - self.direction.x * ez >= 0.0 { 1.0 } else { -1.0 };
            }
        }
        self.fin = node_vec(&store.paths[self.path].nodes[len - 1]);
        self.steer(host, unit);
    }

    /// Append a further path (executor.js:186).
    pub fn append(&mut self, store: &mut PathStore, host: &mut dyn ExecutorHost, path: usize, unit: &mut NavUnit, flag_last: bool) {
        if self.complete {
            if let Some(n) = store.paths[self.path].nodes.get_mut(self.index) {
                n.flag &= !1;
            }
        }
        let mut total = store.paths[self.path].nodes.len() as f64 + store.paths[path].nodes.len() as f64;
        while total > 32.0 && self.index > 0 {
            store.drop_front(self.path, 1.0);
            self.index -= 1;
            total -= 1.0;
        }
        let mut from = 0usize;
        if let (Some(tail), Some(head)) = (store.paths[self.path].nodes.last(), store.paths[path].nodes.first()) {
            if same(node_vec(tail), node_vec(head)) {
                from = 1;
            }
        }
        let additions=store.paths[path].nodes.len()-from;
        for i in 0..additions {
            let node=store.paths[path].nodes[from+i];store.append(self.path,node);
        }
        if flag_last {
            if let Some(last) = store.paths[self.path].nodes.last_mut() {
                last.flag |= 1;
            }
        }
        store.release(path);
        let prev = self.prev;
        self.setup(store, host, prev, unit);
    }

    /// Follow's continuing stream (executor.js:208), cap 30 by default.
    pub fn append_point(&mut self, store: &mut PathStore, host: &mut dyn ExecutorHost, point: Vec3, unit: &mut NavUnit, cap: f64) {
        if self.complete {
            if let Some(n) = store.paths[self.path].nodes.get_mut(self.index) {
                n.flag &= !1;
            }
        }
        let remove = v8math::max(0.0, store.length(self.path) as f64 + 1.0 - cap);
        if remove != 0.0 {
            store.drop_front(self.path, remove);
            self.index = v8math::max(0.0, self.index as f64 - remove) as usize;
        }
        store.append(self.path, PathNode { x: point.x, y: point.y, z: point.z, flag: 0 });
        self.complete = false;
        let prev = self.prev;
        self.setup(store, host, prev, unit);
    }

    /// The steering point (executor.js:221).
    pub fn steer(&mut self, host: &mut dyn ExecutorHost, unit: &mut NavUnit) {
        let mut target = self.waypoint;
        let p = unit.position;
        match unit.kind {
            k if k == kind::FLYER => flyer_steer(self, unit, &mut target),
            k if k == kind::HOVER => {
                target = lookahead(self.prev, self.waypoint, p, speed(unit), target);
            }
            k if k == kind::WALKER => {
                target = lookahead(self.prev, self.waypoint, p, 20.0, target);
            }
            k if k == kind::SOLDIER || k == kind::DROIDEKA => contact_sidestep(self, host, unit, &mut target),
            _ => {}
        }
        self.target = target;
        host.write_request(target, self.waypoint, self.direction, self.mode, -1.0, 1.0);
    }

    /// executor.js:245.
    pub fn release(&self, store: &mut PathStore) {
        store.release(self.path);
    }
}

fn speed(unit: &NavUnit) -> f64 {
    let v = unit.velocity;
    v8math::sqrt(v.x * v.x + v.y * v.y + v.z * v.z)
}

/// Soldier and Droideka steering (executor.js:256).
fn contact_sidestep(exec: &mut PathExecutor, host: &mut dyn ExecutorHost, unit: &mut NavUnit, target: &mut Vec3) {
    let p = unit.position;
    let contact = unit.soldier.as_ref().and_then(|s| s.avoidance);
    let Some(contact) = contact.filter(|c| c.x * c.x + c.y * c.y + c.z * c.z > 0.0) else {
        *host.avoid_side() = 0.0;
        *target = lookahead(exec.prev, exec.waypoint, p, speed(unit), *target);
        return;
    };
    let mut n = contact;
    normalize(&mut n);
    let rx = n.z;
    let rz = -n.x;
    if *host.avoid_side() == 0.0 {
        let mut away = Vec3::new(p.x - exec.waypoint.x, p.y - exec.waypoint.y, p.z - exec.waypoint.z);
        normalize(&mut away);
        *host.avoid_side() = if rx * away.x + rz * away.z <= 0.0 { 1.0 } else { -1.0 };
    }
    let s = 5.0 * *host.avoid_side();
    *target = Vec3::new(p.x + s * rx, p.y, p.z + s * rz);
    if let Some(soldier) = unit.soldier.as_mut() {
        if let Some(c) = soldier.avoidance.as_mut() {
            *c = Vec3::default();
        }
    }
}

/// Segment lookahead (executor.js:281). The `out` of the JavaScript is
/// overwritten on every path, so this returns the point.
pub fn lookahead(a: Vec3, b: Vec3, p: Vec3, ahead: f64, _out: Vec3) -> Vec3 {
    let (ax, az, bx, bz) = (a.x, a.z, b.x, b.z);
    let y = p.y;
    let ex = bx - ax;
    let ez = bz - az;
    let len2 = ex * ex + ez * ez;
    if len2 < tiny() {
        return Vec3::new(bx, y, bz);
    }
    let len = v8math::sqrt(f32(len2));
    let dx = f32(ex / len);
    let dz = f32(ez / len);
    let s = f32(f32(dx * f32(p.x - ax)) + f32(dz * f32(p.z - az)));
    let t = f32(s + ahead + 0.2);
    if f32(t * t) <= f32(len2) {
        return Vec3::new(f32(ax + f32(t * dx)), y, f32(az + f32(t * dz)));
    }
    Vec3::new(bx, y, bz)
}

/// executor.js:300.
pub fn near_radius(unit: &NavUnit) -> f64 {
    let k = unit.kind;
    if k == kind::SOLDIER || k == kind::DROID {
        return 1.0;
    }
    if k == kind::HOVER {
        return 0.5 * unit.class_def.as_ref().and_then(|d| d.forward_speed).unwrap_or(0.0);
    }
    4.0
}

/// executor.js:309.
pub fn pass_radius(unit: &NavUnit) -> f64 {
    let k = unit.kind;
    if k == kind::FLYER {
        return 0.25 * unit.class_def.as_ref().and_then(|d| d.max_speed).unwrap_or(30.0);
    }
    if k == kind::HOVER {
        return 0.5 * unit.class_def.as_ref().and_then(|d| d.forward_speed).unwrap_or(0.0);
    }
    1.0
}

/// Flyer circle steering (executor.js:319). `unit.forward` is the
/// snapshot's forward when the unit has the method, else (0, 0, 1).
fn flyer_steer(exec: &mut PathExecutor, unit: &NavUnit, target: &mut Vec3) {
    let turn = v8math::max(unit.class_def.as_ref().and_then(|d| d.turn_rate).unwrap_or(2.0), 1.5);
    let fwd = unit.forward.unwrap_or(Vec3::new(0.0, 0.0, 1.0));
    let v = unit.velocity;
    let along = v8math::abs(v.x * fwd.x + v.y * fwd.y + v.z * fwd.z);
    let r = v8math::max(5.0, 2.0 * (along / turn));
    let mut f = Vec3::new(fwd.x + 0.0001, 0.0, fwd.z);
    normalize(&mut f);
    *target = flyer_circle(exec.waypoint, exec.direction, unit.position, f, r, exec.side, *target);
}

/// Circle construction (executor.js:334).
pub fn flyer_circle(q: Vec3, u: Vec3, p: Vec3, v: Vec3, r: f64, side: f64, _out: Vec3) -> Vec3 {
    let mut big_u = Vec3::new(u.x, 0.0, u.z);
    normalize(&mut big_u);
    let mut big_v = Vec3::new(v.x, 0.0, v.z);
    normalize(&mut big_v);
    let perp = |w: Vec3| Vec3::new(w.z, 0.0, -w.x);
    let pv = perp(big_v);
    let pu = perp(big_u);
    let a_plus = Vec3::new(p.x + r * pv.x, 0.0, p.z + r * pv.z);
    let a_minus = Vec3::new(p.x - r * pv.x, 0.0, p.z - r * pv.z);
    let h2 = |a: Vec3, b: Vec3| (a.x - b.x) * (a.x - b.x) + (a.z - b.z) * (a.z - b.z);
    let actor_plus = h2(a_plus, q) < h2(a_minus, q);
    let c_plus = Vec3::new(q.x + r * pu.x, 0.0, q.z + r * pu.z);
    let c_minus = Vec3::new(q.x - r * pu.x, 0.0, q.z - r * pu.z);
    let mut plus;
    if side == 1.0 {
        plus = true;
    } else if side == -1.0 {
        plus = false;
    } else {
        plus = h2(c_plus, p) < h2(c_minus, p);
        let a = if actor_plus { a_plus } else { a_minus };
        let c = if plus { c_plus } else { c_minus };
        if h2(a, c) < 4.0 * r * r && actor_plus != plus {
            plus = !plus;
        }
    }
    let c = if plus { c_plus } else { c_minus };
    let dx = p.x - c.x;
    let dz = p.z - c.z;
    let d = v8math::sqrt(dx * dx + dz * dz);
    let (tx, tz);
    if d < r {
        tx = c.x + r * dx / (d + 0.00001);
        tz = c.z + r * dz / (d + 0.00001);
    } else {
        let base = v8math::atan2(dz, dx);
        let swing = v8math::acos(r / d);
        let angle = if plus { base - swing } else { base + swing };
        tx = c.x + r * v8math::cos(angle);
        tz = c.z + r * v8math::sin(angle);
    }
    let wx = tx - c.x;
    let wz = tz - c.z;
    if plus {
        Vec3::new(tx + wz, q.y, tz - wx)
    } else {
        Vec3::new(tx - wz, q.y, tz + wx)
    }
}
