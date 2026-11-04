//! The AI input step for a bot in a seat (src/ai/classic/vehicles/
//! steering.js): the controller's movement request and aim become the
//! vehicle's analogs, per family, as the 2004 game's AI input consumers do.
//!
//! - hover (6): ray avoidance on a copy of the request, the shared
//!   translation, the nonlinear yaw and pitch, the moving-turn floor and
//!   the reverse-and-turn stuck recovery;
//! - flyer (5): the pilot's stick (src/flight/ai.js, the flight package's:
//!   a call across the boundary), on the orbit's request while pattern 15
//!   holds one;
//! - walker (2): travel held after a yielding contact, forward and body
//!   yaw only, head yaw and pitch from the aim, the stuck recovery;
//! - anything else (7, gunners and turrets): yaw and pitch toward the aim.
//!
//! Every function takes the control it steers as a [`UnitView`] read once
//! at its start (nothing moves a hull while the AI runs) and reaches its
//! steering bag, its eye and its aim through the [`World`].

use std::f64::consts::PI;

use three::Vector3;
use v8math::{abs, asin, atan2, cos, hypot, hypot3, max, min, sin};

use super::orbit::{orbit_steer, OrbitResult};
use super::world::{kind, num, Obj, Orbit, PilotEnv, Request, Steer, Stick, UnitView, World};

const WALKER_HOLD: f64 = 7.5;
const WALKER_PRESS: f64 = 3.0;
const WALKER_STUCK_RADIUS: f64 = 10.0;
const REQUEST_LIFE: f64 = 0.5;
const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);

/// `Math.cos((5 * Math.PI) / 180)` (steering.js:36), with V8's cos.
fn cos5() -> f64 {
    cos((5.0 * PI) / 180.0)
}

/// sign(0) = +1, as the 2004 game's (steering.js:46).
fn sign(v: f64) -> f64 {
    if v < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// `x || d` for a number.
fn or(x: Option<f64>, d: f64) -> f64 {
    match x {
        Some(v) if v != 0.0 && !v.is_nan() => v,
        _ => d,
    }
}

/// What the steering's environment carries (index.js analogs' `env`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Env {
    pub now: f64,
    pub has_path: bool,
    pub status: f64,
    pub min_fly_height: f64,
    pub max_fly_height: f64,
    /// The controller whose seat this is (the walker's replan reaches it).
    pub controller: Option<Obj>,
}

/// steerState(unit) (steering.js:49): the control's bag, made whole the
/// first time.
pub fn steer_state<W: World + ?Sized, R>(w: &mut W, control: Obj, f: impl FnOnce(&mut Steer) -> R) -> R {
    w.edit_steer(control, |slot| f(slot.get_or_insert_with(Steer::full)))
}

/// arrivalRadius(unit) (steering.js:87).
pub fn arrival_radius(unit: &UnitView) -> f64 {
    let k = unit.kind;
    if k == 0.0 || k == 1.0 {
        return 1.0;
    }
    if k == kind::HOVER {
        return 0.5 * unit.class.as_ref().and_then(|c| c.forward_speed).unwrap_or(0.0);
    }
    4.0
}

/// bodyForward(unit, out) (steering.js:95).
fn body_forward(unit: &UnitView) -> Vector3 {
    let yaw = unit.body_yaw;
    let mut out = Vector3::new(sin(yaw), 0.0, cos(yaw));
    if out.length_sq() < 0.0001 {
        out.set(1.0, 0.0, 0.0);
    }
    *out.normalize()
}

/// translate's answer (steering.js:106).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Move {
    pub forward: f64,
    pub right: f64,
    pub near: bool,
}

/// translate(unit, request, maxForward, maxStrafe, hasPath, out) (steering.js:106).
pub fn translate(unit: &UnitView, request: Option<&Request>, max_forward: f64, max_strafe: f64, has_path: bool) -> Move {
    let mut out = Move::default();
    let Some(request) = request.filter(|r| r.valid) else { return out };
    let p = unit.position;
    let mut d = Vector3::new(request.steering_target.x - p.x, 0.0, request.steering_target.z - p.z);
    if abs(d.x) < 0.0001 && abs(d.z) < 0.0001 {
        out.near = true;
        return out;
    }
    let r = arrival_radius(unit);
    let dist = d.length();
    let near = dist * dist < r * r;
    out.near = near;
    if near && !has_path && unit.kind == kind::WALKER {
        return out;
    }
    let s = if near && !has_path { min(max(dist / r, 0.0), 1.0) } else { 1.0 };
    d.divide_scalar(dist);
    let fwd = body_forward(unit);
    let a = fwd.x * d.x + fwd.z * d.z;
    let b = -(fwd.z * d.x - fwd.x * d.z);
    if max_strafe > 0.0001 {
        let u = a / max_forward;
        let v = b / max_strafe;
        let m = or(Some(max(abs(u), abs(v))), 1.0);
        out.forward = (u / m) * s;
        out.right = (v / m) * s;
    } else {
        out.forward = min(max(a, 0.0), 1.0) * s;
        out.right = 0.0;
    }
    if unit.kind == kind::WALKER && !near && a < cos5() {
        out.forward = 0.0;
    }
    out
}

/// facingPoint(unit, request, aim, near, maxStrafe, out) (steering.js:149).
fn facing_point(unit: &UnitView, request: Option<&Request>, aim: Option<&Vector3>, near: bool, max_strafe: f64) -> Vector3 {
    let valid = request.is_some_and(|r| r.valid);
    let use_aim = max_strafe > 0.0001 || !valid || near;
    if use_aim {
        if let Some(aim) = aim {
            return *aim;
        }
        if valid && !near {
            let r = request.unwrap();
            let mut q = Vector3::new(r.steering_target.x - unit.position.x, 0.0, r.steering_target.z - unit.position.z);
            if q.length_sq() > 1e-8 {
                q.normalize();
                return *unit.position.clone().add_scaled_vector(&q, 30.0);
            }
        }
        let fwd = body_forward(unit);
        return *unit.position.clone().add_scaled_vector(&fwd, 30.0);
    }
    request.unwrap().steering_target
}

/// nonlinearYaw(unit, point) (steering.js:168).
pub fn nonlinear_yaw(unit: &UnitView, point: &Vector3) -> f64 {
    let mut q = Vector3::default();
    q.sub_vectors(point, &unit.position);
    q.y = 0.0;
    let fwd = body_forward(unit);
    let right = Vector3::new(-fwd.z, 0.0, fwd.x);
    let length = hypot(q.x, q.z);
    if length < 1e-9 {
        return 0.0;
    }
    q.divide_scalar(length);
    let mut s = q.x * right.x + q.z * right.z;
    let ahead = q.x * fwd.x + q.z * fwd.z;
    if ahead < 0.0 {
        s = sign(s);
    }
    let rate = unit.class.as_ref().and_then(|c| c.turn_rate).unwrap_or(2.0);
    let g = 4.5 - min(max(rate, 1.0), 3.0);
    let t = min(max(g * s, -1.0), 1.0);
    sign(t) * t * t
}

/// nonlinearPitch(unit, point) (steering.js:186).
pub fn nonlinear_pitch<W: World + ?Sized>(w: &mut W, unit: &UnitView, point: &Vector3) -> f64 {
    let origin = w.origin(unit.obj);
    let mut q = Vector3::default();
    q.sub_vectors(point, &origin);
    let length = q.length();
    if length < 1e-9 {
        return 0.0;
    }
    let fwd = w.forward(unit.obj);
    let mut right = Vector3::default();
    right.cross_vectors(&fwd, &UP);
    if right.length_sq() < 1e-9 {
        return 0.0;
    }
    right.normalize();
    let mut p = Vector3::default();
    p.cross_vectors(&right, &fwd).normalize();
    let s = (q.x * p.x + q.y * p.y + q.z * p.z) / length;
    let rate = unit.class.as_ref().and_then(|c| c.pitch_rate).unwrap_or(1.0);
    let g = 5.0 - min(max(rate, 1.0), 3.0);
    let t = min(max(g * s, -1.0), 1.0);
    sign(t) * t * t
}

/// linearAim's answer (`angles`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Angles {
    pub turn: f64,
    pub pitch: f64,
}

/// linearAim(unit, point, dt, yawRate, pitchRate, fast, out) (steering.js:207).
pub fn linear_aim<W: World + ?Sized>(w: &mut W, unit: &UnitView, point: &Vector3, dt: f64, yaw_rate: f64, pitch_rate: f64, fast: bool) -> Angles {
    let mut out = Angles::default();
    let origin = w.origin(unit.obj);
    let mut q = Vector3::default();
    q.sub_vectors(point, &origin);
    let flat = hypot(q.x, q.z);
    let fwd = w.forward(unit.obj);
    let (fx, fz) = (fwd.x, fwd.z);
    let fl = or(Some(hypot(fx, fz)), 1.0);
    if flat > 1e-6 {
        let err = -((fz / fl) * (q.x / flat) - (fx / fl) * (q.z / flat));
        let limit = if fast { 2.0 } else { 1.0 };
        let rate = yaw_rate * dt;
        out.turn = max(-limit, min(limit, if rate > 0.0 { err / rate } else { 0.0 }));
    } else {
        out.turn = 0.0;
    }
    let ahead = q.x * fwd.x + q.y * fwd.y + q.z * fwd.z;
    if ahead <= 0.0 {
        out.pitch = 0.0;
        return out;
    }
    let want = atan2(q.y, flat);
    let have = asin(max(-1.0, min(1.0, fwd.y)));
    let rate = pitch_rate * dt;
    out.pitch = max(-1.0, min(1.0, if rate > 0.0 { sin(want - have) / rate } else { 0.0 }));
    out
}

/// The facing choice's "already arrived" (steering.js:242).
fn arrived(m: &Move, env: &Env) -> bool {
    m.near && !env.has_path
}

/// hoverInput(unit, request, aim, dt, env, out) (steering.js:248).
pub fn hover_input<W: World + ?Sized>(w: &mut W, unit: &UnitView, request: Option<&Request>, aim: Option<&Vector3>, _dt: f64, env: &Env, out: &mut Stick) {
    let c = unit.class.clone().unwrap_or_default();
    steer_state(w, unit.obj, |_| ());
    let mut factor = 1.0;
    let mut req = request.copied();
    let huge = c.ai_size_type.as_deref().unwrap_or("").to_lowercase() == "huge";
    if let (Some(r), false) = (request.filter(|r| r.valid), huge) {
        let v = unit.velocity;
        let d_len = 1.5 * hypot3(v.x, v.y, v.z);
        if d_len > 5.0 {
            let mut d = Vector3::new(r.steering_target.x - unit.position.x, 0.0001, r.steering_target.z - unit.position.z);
            d.normalize();
            let right = Vector3::new(-d.z, 0.0, d.x);
            let mut p = unit.position;
            p.add(&UP);
            let body = unit.vehicle.and_then(|h| w.hull(h).body);
            if let Some(hit) = w.cast(&p, &d, d_len, body) {
                // avoidRequest.copyFrom(request): the request's own fields.
                let mut avoid = Request { blend: None, throttle: None, ..*r };
                let side = if hit.normal.x * right.x + hit.normal.z * right.z > 0.0 { 1.0 } else { -1.0 };
                avoid.steering_target.add_scaled_vector(&right, side * 0.25 * d_len);
                req = Some(avoid);
                factor = 0.5;
            }
        }
    }
    let strafe = c.strafe_speed.unwrap_or(0.0);
    let m = translate(unit, req.as_ref(), or(c.forward_speed, 1.0), strafe, env.has_path);
    let aim_point = facing_point(unit, req.as_ref(), aim, arrived(&m, env), strafe);
    out.forward = m.forward;
    out.right = m.right;
    out.turn = nonlinear_yaw(unit, &aim_point);
    out.pitch = match aim {
        Some(a) => nonlinear_pitch(w, unit, a),
        None => 0.0,
    };
    let moving_turn_at = steer_state(w, unit.obj, |st| num(st.moving_turn_at));
    if c.moving_turn_only == Some(true) && env.now <= moving_turn_at + REQUEST_LIFE && abs(out.turn) > 0.0001 {
        out.forward = if out.forward < 0.0 { min(-0.5, out.forward) } else { max(0.5, out.forward) };
    }
    out.forward *= factor;
    hover_recovery(w, unit, env, out);
}

/// hoverRecovery(unit, env, st, out) (steering.js:307).
fn hover_recovery<W: World + ?Sized>(w: &mut W, unit: &UnitView, env: &Env, out: &mut Stick) {
    let now = env.now;
    steer_state(w, unit.obj, |st| {
    if num(st.recovery) == 0.0 {
        if env.status != 0.0 && now > num(st.last_recovery) + 4.0 {
            st.recovery = Some(1.0);
            st.saved_forward = Some(body_forward(unit));
        } else {
            return;
        }
    }
    if num(st.recovery) == 1.0 {
        let fwd = body_forward(unit);
        let saved = st.saved_forward.unwrap_or_default();
        if saved.dot(&fwd) < 0.0 {
            st.recovery = Some(2.0);
            st.recovery_since = Some(now);
        } else {
            out.forward = -1.0;
            out.turn = 0.5;
            return;
        }
    }
    out.forward = 1.0;
    out.turn = 0.5;
    if now > num(st.recovery_since) + 3.0 {
        st.recovery = Some(0.0);
        st.last_recovery = Some(now);
    }
    });
}

/// walkerInput(unit, request, aim, dt, env, out) (steering.js:340).
pub fn walker_input<W: World + ?Sized>(w: &mut W, unit: &UnitView, request: Option<&Request>, aim: Option<&Vector3>, dt: f64, env: &Env, out: &mut Stick) {
    let c = unit.class.clone().unwrap_or_default();
    let holding = steer_state(w, unit.obj, |st| {
        let holding = env.now <= num(st.hold_since) + WALKER_HOLD;
        st.holding = Some(holding);
        holding
    });
    let req = if holding { None } else { request };
    let m = translate(unit, req, or(c.max_speed, 1.0), 0.0, env.has_path);
    out.forward = m.forward;
    out.right = 0.0;
    let max_pitch = c.max_pitch_speed.unwrap_or(0.5);
    if req.is_some_and(|r| r.valid) && !arrived(&m, env) {
        let point = facing_point(unit, req, None, false, 0.0);
        let angles = linear_aim(w, unit, &point, dt, c.max_turn_speed.unwrap_or(2.0), max_pitch, false);
        out.turn = angles.turn;
    } else if let Some(a) = aim {
        let angles = linear_aim(w, unit, a, dt, c.max_yaw_speed.unwrap_or(1.0), max_pitch, false);
        out.turn = angles.turn;
    } else {
        out.turn = 0.0;
    }
    if let Some(a) = aim {
        let angles = linear_aim(w, unit, a, dt, c.max_yaw_speed.unwrap_or(1.0), max_pitch, false);
        out.pitch = angles.pitch;
    } else {
        out.pitch = 0.0;
    }
    let yaw = unit.body_yaw;
    let turning = req.is_some_and(|r| r.valid) && !m.near && m.forward == 0.0;
    let (holding, pressing, recovering, wedged) = steer_state(w, unit.obj, |st| {
    if !turning || !(abs(yaw - num(st.turn_yaw)) < 0.01) {
        st.turn_yaw = Some(yaw);
        st.turn_at = Some(env.now);
    }
    let wedged = env.now > num(st.turn_at) + 2.0;
    let holding = st.holding == Some(true);
    if m.forward > 0.0 && !holding {
        if !(num(st.walk_since) <= env.now) {
            st.walk_since = Some(env.now);
        }
    } else {
        st.walk_since = Some(f64::NAN);
    }
    let pressing = m.forward > 0.0 && env.now >= num(st.walk_since) + WALKER_PRESS;
    let recovering = num(st.recovery) != 0.0;
        (holding, pressing, recovering, wedged)
    });
    if !holding && (recovering || pressing || wedged) {
        hover_recovery(w, unit, env, out);
    }
    let replan_kind = steer_state(w, unit.obj, |st| {
    if recovering && num(st.recovery) == 0.0 {
        let p = unit.position;
        match st.stuck_at.flatten() {
            Some(at) if hypot(p.x - at.x, p.z - at.z) < WALKER_STUCK_RADIUS => st.stuck_count = Some(num(st.stuck_count) + 1.0),
            _ => {
                st.stuck_at = Some(Some(super::world::StuckAt { x: p.x, z: p.z }));
                st.stuck_count = Some(1.0);
            }
        }
        let count = num(st.stuck_count);
        if count == 2.0 {
            return Some(false);
        } else if count >= 4.0 {
            st.stuck_at = Some(None);
            st.stuck_count = Some(0.0);
            return Some(true);
        }
    }
        None
    });
    if let Some(another) = replan_kind { replan(w, env, another); }
}

/// The analogs' `env.replan(another)` (index.js analogs): another post, or
/// the movement helper's trip planned again.
fn replan<W: World + ?Sized>(w: &mut W, env: &Env, another: bool) {
    let Some(c) = env.controller else { return };
    if another {
        w.request_objective(c, true);
        return;
    }
    let view = w.controller(c);
    let Some(agent) = view.agent else { return };
    if view.helper_state == Some(6.0) {
        if let Some(destination) = view.helper_destination {
            w.helper_goto(agent, &destination, false);
        }
    }
}

/// walkerYields(self, other) (steering.js:416). `other` is `{ kind,
/// position, velocity, classDef: { bounds } }`.
pub fn walker_yields(me: &UnitView, other_kind: f64, other_position: &Vector3, other_velocity: &Vector3, other_bounds: Option<[[f64; 3]; 2]>) -> bool {
    if other_kind != kind::WALKER && other_kind != kind::HOVER {
        return false;
    }
    let va = volume(me.class.as_ref().and_then(|c| c.bounds));
    let vb = volume(other_bounds);
    if va > 3.0 * vb {
        return false;
    }
    if vb > 3.0 * va {
        return true;
    }
    let a = me.velocity;
    let b = *other_velocity;
    if !(a.x * a.x + a.z * a.z > 0.0001) || !(b.x * b.x + b.z * b.z > 0.0001) {
        return false;
    }
    let mut p = a;
    p.normalize();
    let mut q = b;
    q.normalize();
    let mut d = Vector3::default();
    d.sub_vectors(other_position, &me.position).normalize();
    -d.dot(&p) < d.dot(&q)
}

/// volume(unit) (steering.js:431).
fn volume(bounds: Option<[[f64; 3]; 2]>) -> f64 {
    let Some(b) = bounds else { return 1.0001 };
    (b[1][0] - b[0][0]) * (b[1][1] - b[0][1]) * (b[1][2] - b[0][2]) + 0.0001
}

/// walkerContact(unit, other, now) (steering.js:438).
pub fn walker_contact<W: World + ?Sized>(w: &mut W, me: &UnitView, other_kind: f64, other_position: &Vector3, other_velocity: &Vector3, other_bounds: Option<[[f64; 3]; 2]>, now: f64) {
    if me.kind == kind::WALKER && walker_yields(me, other_kind, other_position, other_velocity, other_bounds) {
        steer_state(w, me.obj, |st| st.hold_since = Some(now));
    }
}

/// stationaryInput(unit, request, aim, dt, env, out) (steering.js:444).
pub fn stationary_input<W: World + ?Sized>(w: &mut W, unit: &UnitView, aim: Option<&Vector3>, dt: f64, out: &mut Stick) {
    out.forward = 0.0;
    out.right = 0.0;
    let Some(aim) = aim else {
        out.turn = 0.0;
        out.pitch = 0.0;
        return;
    };
    let c = unit.class.clone().unwrap_or_default();
    let angles = linear_aim(w, unit, aim, dt, c.turn_rate.unwrap_or(2.0), c.pitch_rate.unwrap_or(1.0), false);
    out.turn = angles.turn;
    out.pitch = angles.pitch;
}

/// orbitInput(unit) (steering.js:472): the orbit's movement request while
/// pattern 15 holds one and its walker lives, with the tow pass's weapon
/// switches; None when no orbit steers.
pub fn orbit_input<W: World + ?Sized>(w: &mut W, unit: &UnitView) -> Option<Request> {
    let orbit: Orbit = w.edit_steer(unit.obj, |s| s.as_ref().and_then(|s| s.orbit()))?;
    let target = orbit.target?;
    let entry = w.unit(target.obj);
    let hull = entry.vehicle?;
    let v = unit.vehicle?;
    let hull = w.hull(hull);
    if !hull.alive || entry.generation != target.generation {
        return None;
    }
    let me = w.hull(v);
    let mut fwd = Vector3::new(0.0, 0.0, 1.0);
    fwd.apply_quaternion(&me.quaternion);
    let mut result = OrbitResult::default();
    orbit_steer(&me.position, &fwd, &hull.position, &hull.matrix, &orbit, &mut result);
    if result.tow_pass {
        for seat in &me.seats {
            let Some(lease) = seat.lease.map(|l| w.lease(l)) else { continue };
            let network = lease.unit.is_some_and(|u| w.unit(u).network_human);
            let control = if !lease.human && !network { lease.controllable } else { None };
            let Some(control) = control else { continue };
            let controller = w.unit(control).controller;
            let Some(weapons) = w.weapons(control) else { continue };
            let Some(tow) = weapons.iter().position(|wv| wv.base.as_deref() == Some("towcableweapon")) else { continue };
            let Some(controller) = controller else { continue };
            if w.controller(controller).fire_channel != Some(weapons[tow].channel) {
                w.switch_weapon(controller, control, tow);
            }
        }
    }
    let mut r = Request { valid: true, mode: 3.0, ..Request::default() };
    r.steering_target = result.point;
    r.waypoint = result.point;
    r.segment_direction.sub_vectors(&result.point, &me.position);
    r.segment_direction.x += 0.0001;
    r.segment_direction.normalize();
    r.blend = Some(result.blend);
    r.throttle = Some(result.throttle);
    Some(r)
}

/// seatAnalogs(unit, request, aim, dt, out, env) (steering.js:506): the
/// analogs, with the vehicle fields written into the control's intent.
pub fn seat_analogs<W: World + ?Sized>(w: &mut W, unit: &UnitView, request: Option<&Request>, aim: Option<&Vector3>, dt: f64, env: &Env) -> Stick {
    let mut full = Stick::default();
    match unit.kind {
        k if k == kind::HOVER => hover_input(w, unit, request, aim, dt, env, &mut full),
        k if k == kind::FLYER => {
            let orbit = orbit_input(w, unit);
            let request = orbit.as_ref().or(request);
            // src/flight/ai.js `const st = unit.steer ||= {}`.
            w.edit_steer(unit.obj, |s| { s.get_or_insert_with(Steer::empty); });
            let pilot = PilotEnv { has_path: env.has_path, min_fly_height: env.min_fly_height, max_fly_height: env.max_fly_height };
            full = w.pilot_input(unit.obj, request, aim, dt, &pilot);
        }
        k if k == kind::WALKER => walker_input(w, unit, request, aim, dt, env, &mut full),
        _ => stationary_input(w, unit, aim, dt, &mut full),
    }
    w.write_vehicle_intent(unit.obj, super::world::SeatIntent {
        up: if full.up != 0.0 && !full.up.is_nan() { 1.0 } else { 0.0 },
        down: if full.down != 0.0 && !full.down.is_nan() { 1.0 } else { 0.0 },
        brake: full.brake, trick: full.trick, immelmann: full.immelmann, boost: full.boost,
    });
    Stick { forward: full.forward, right: full.right, turn: full.turn, pitch: full.pitch, ..Stick::default() }
}

