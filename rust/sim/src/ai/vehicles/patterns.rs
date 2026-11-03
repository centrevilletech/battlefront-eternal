//! Vehicle combat (src/ai/classic/vehicles/patterns.js): the engagement
//! dispatch for walker, hover and flyer controls, and the attack patterns
//! only vehicles run: the hover pass (7), the attack run (13), the dormant
//! pursuit (14) and straight run (16), and the AT-AT tow orbit (15), with
//! pattern 2's sidestep for slow walkers and hovers.
//!
//! Combat (the AI combat package) owns the dispatcher and the attack
//! patterns' machine; it calls these through world.vehicles. What they
//! read of a machine is a [`PatternsView`]; the pattern state they keep on
//! it (`patterns.vehicle`) is [`PatternState`].

use three::Vector3;
use v8math::{cos, fround, hypot, max, min, sin};

use super::steering::steer_state;
use super::world::{kind, Obj, Orbit, Ref, Settings, UnitView, World};

/// The attack patterns' machine as the vehicle hooks read it
/// (src/ai/classic/combat.js AttackPatterns): its agent, controller and
/// control, and the agent's target tracker and movement helper.
#[derive(Clone, Debug, PartialEq)]
pub struct PatternsView {
    pub obj: Obj,
    pub agent: Obj,
    pub controller: Obj,
    /// `patterns.controller.unit`.
    pub control: Obj,
    /// `agent.tracker?.point` (None: no tracker).
    pub tracker_point: Option<Vector3>,
    /// `agent.tracker?.freshTarget` / `.target`.
    pub fresh_target: Option<Obj>,
    pub target: Option<Obj>,
    /// `agent.helper` is there, and its `arrived`.
    pub helper: bool,
    pub helper_arrived: bool,
}

/// stateOf(patterns) (patterns.js:84): `patterns.vehicle`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatternState {
    pub toggle: bool,
    pub entry: Vector3,
    pub destination: Vector3,
    pub best: f64,
    pub progress_at: f64,
    pub fallback: bool,
    pub loop_: bool,
    pub inverted: bool,
    pub flip_a: bool,
    pub flip_b: bool,
}

impl Default for PatternState {
    fn default() -> Self {
        PatternState {
            toggle: false,
            entry: Vector3::default(),
            destination: Vector3::default(),
            best: f64::INFINITY,
            progress_at: 0.0,
            fallback: false,
            loop_: false,
            inverted: false,
            flip_a: false,
            flip_b: false,
        }
    }
}

/// The attack run's distances, set once from the first aircraft that runs
/// one (patterns.js:180; the JavaScript's are the process's, these the
/// battle's: plan D16).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AttackRun {
    pub a: Option<f64>,
    pub b: Option<f64>,
}

/// The dispatcher's answer (combat.js dispatch's `out`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dispatch {
    pub pattern: f64,
    pub probability: f64,
    pub aux: bool,
    pub action: f64,
    pub in_band: bool,
    pub weapon: Option<Obj>,
}

fn f(x: f64) -> f64 {
    fround(x)
}

/// isVehicleOrBuilding (patterns.js:32).
fn vehicle_or_building(k: f64) -> bool {
    k == 2.0 || k == 4.0 || k == 6.0 || k == 8.0 || k == 9.0
}

/// isAtAt(unit) (patterns.js:36): the class labelled "AT-AT" (or named
/// imp_walk_atat).
pub fn is_at_at<W: World + ?Sized>(w: &W, unit: &UnitView) -> bool {
    let (label, name) = match unit.vehicle {
        Some(h) => {
            let hull = w.hull(h);
            (hull.label, Some(hull.name))
        }
        None => (unit.own_label.clone(), unit.own_name.clone()),
    };
    label.as_deref() == Some("AT-AT") || name.as_deref() == Some("imp_walk_atat") || unit.class_name.as_deref() == Some("imp_walk_atat")
}

/// forwardSpeed(unit) (weapons-ai.js:199): `f(ForwardSpeed ?? MaxSpeed ?? 0)`.
pub fn forward_speed(unit: &UnitView) -> f64 {
    let c = unit.class.as_ref();
    f(c.and_then(|c| c.forward_speed).or_else(|| c.and_then(|c| c.max_speed)).unwrap_or(0.0))
}

/// selectOnly (patterns.js:23).
fn select_only<W: World + ?Sized>(w: &mut W, controller: Obj, unit: &UnitView, target: Obj, switch_allowed: bool, out: &mut Dispatch) {
    let choice = w.select_weapon(controller, unit.obj, target, switch_allowed);
    out.aux = choice.ok;
    out.action = choice.action;
    out.in_band = choice.in_band;
    out.weapon = choice.weapon;
}

/// vehicleDispatch(controller, unit, target, switchAllowed, world, out, options) (patterns.js:50).
pub fn vehicle_dispatch<W: World + ?Sized>(w: &mut W, controller: Obj, unit: &UnitView, target: &UnitView, switch_allowed: bool, dense: bool, out: &mut Dispatch) {
    out.pattern = 0.0;
    out.probability = 0.0;
    let s = unit.kind;
    let t = target.kind;
    if s == kind::WALKER {
        select_only(w, controller, unit, target.obj, switch_allowed, out);
        if vehicle_or_building(t) && out.aux && out.in_band {
            out.pattern = 2.0;
        }
        return;
    }
    if s == kind::HOVER {
        select_only(w, controller, unit, target.obj, switch_allowed, out);
        if !vehicle_or_building(t) || !out.aux {
            return;
        }
        let c = unit.class.clone().unwrap_or_default();
        if !dense && f(c.forward_speed.unwrap_or(0.0)) >= 14.0 && f(c.strafe_speed.unwrap_or(0.0)) < 5.0 {
            out.pattern = 7.0;
        } else if out.in_band {
            out.pattern = 2.0;
        }
        return;
    }
    if s == kind::FLYER {
        select_only(w, controller, unit, target.obj, switch_allowed, out);
        if t == 2.0 || t == 8.0 || t == 9.0 {
            // The AT-AT branch comes before the weapon test.
            let body = unit.vehicle.map(|h| w.hull(h));
            if is_at_at(w, target) && body.as_ref().is_some_and(|b| b.occupants == 2.0) {
                out.pattern = 15.0;
            } else if out.aux && !body.as_ref().is_some_and(|b| b.driver) {
                out.pattern = 13.0;
            }
        }
        return;
    }
    select_only(w, controller, unit, target.obj, switch_allowed, out);
}

fn now<W: World + ?Sized>(w: &W) -> f64 {
    w.now()
}

/// trackerPoint(patterns, out) (patterns.js:100). The tracker's point is a
/// Vector3 that is always there, so a tracker's answer is its point; the
/// target's place is reached only without a tracker, where there is none.
fn tracker_point<W: World + ?Sized>(w: &W, p: &PatternsView) -> Option<Vector3> {
    if let Some(point) = p.tracker_point {
        return Some(point);
    }
    let target = p.fresh_target.or(p.target)?;
    Some(w.unit(target).position)
}

/// The patterns' bodyForward (patterns.js:396): not normalized.
fn body_forward(unit: &UnitView) -> Vector3 {
    let yaw = unit.body_yaw;
    Vector3::new(sin(yaw), 0.0, cos(yaw))
}

/// normalizeXZ(v) (patterns.js:401).
fn normalize_xz(v: &mut Vector3) {
    v.y = 0.0;
    let l = hypot(v.x, v.z);
    if l > 1e-12 {
        v.multiply_scalar(1.0 / l);
    }
}

/// horizontalDistanceSq(a, b) (patterns.js:408).
fn horizontal_distance_sq(a: &Vector3, b: &Vector3) -> f64 {
    let dx = a.x - b.x;
    let dz = a.z - b.z;
    dx * dx + dz * dz
}

/// hoverPassPoint(patterns, unit, st, out) (patterns.js:116).
fn hover_pass_point<W: World + ?Sized>(w: &mut W, p: &PatternsView, unit: &UnitView, toggle: bool) -> Vector3 {
    let pos = unit.position;
    let v = unit.class.as_ref().and_then(|c| c.forward_speed).or(unit.soldier_max_speed).unwrap_or(0.0);
    let fwd = body_forward(unit);
    let ahead = |pos: Vector3| {
        let mut out = pos;
        out.add_scaled_vector(&fwd, 3.0 * v);
        out
    };
    if !toggle {
        return ahead(pos);
    }
    let Some(q) = tracker_point(w, p) else { return ahead(pos) };
    let mut d = Vector3::default();
    d.sub_vectors(&q, &pos);
    normalize_xz(&mut d);
    let along = fwd.dot(&d);
    let mut n = fwd;
    n.add_scaled_vector(&d, -along);
    normalize_xz(&mut n);
    let target = p.fresh_target;
    let r_unit = w.sphere(unit.obj).radius;
    let r_target = match target {
        Some(t) => w.sphere(t).radius,
        None => 0.0,
    };
    let r = (r_unit + r_target) + 2.0;
    let mut out = q;
    out.add_scaled_vector(&n, r);
    let mut pp = Vector3::default();
    pp.sub_vectors(&out, &pos);
    normalize_xz(&mut pp);
    out.add_scaled_vector(&pp, 0.5 * v);
    out
}

/// hoverEnter(patterns) (patterns.js:132).
fn hover_enter<W: World + ?Sized>(w: &mut W, p: &PatternsView, st: &mut PatternState) {
    let unit = w.unit(p.control);
    st.toggle = true;
    st.entry = unit.position;
    w.set_delay(p.agent, 12.0, now(w));
    hover_leg(w, p, &unit, st);
}

/// hoverLeg(patterns, unit, st) (patterns.js:141).
fn hover_leg<W: World + ?Sized>(w: &mut W, p: &PatternsView, unit: &UnitView, st: &mut PatternState) {
    st.destination = hover_pass_point(w, p, unit, st.toggle);
    if p.helper {
        w.helper_goto(p.agent, &st.destination, true);
    }
    st.best = horizontal_distance_sq(&unit.position, &st.destination);
    st.progress_at = now(w);
    st.fallback = false;
}

/// hoverTick(patterns) (patterns.js:152).
fn hover_tick<W: World + ?Sized>(w: &mut W, p: &PatternsView, st: &mut PatternState) {
    let unit = w.unit(p.control);
    let pp = unit.position;
    let q = st.destination;
    let e = st.entry;
    let crossed = (pp.x - q.x) * (q.x - e.x) + (pp.y - q.y) * (q.y - e.y) + (pp.z - q.z) * (q.z - e.z) > 0.0;
    if (p.helper && p.helper_arrived) || crossed {
        st.toggle = !st.toggle;
        hover_leg(w, p, &unit, st);
    } else {
        let d2 = horizontal_distance_sq(&pp, &q);
        if d2 < st.best {
            st.best = d2;
            st.progress_at = now(w);
        } else if !(now(w) <= st.progress_at + 10.0) && !st.fallback {
            if let Some(q) = tracker_point(w, p) {
                if p.helper {
                    w.helper_goto(p.agent, &q, false);
                }
            }
            st.fallback = true;
        }
    }
    let t = now(w);
    steer_state(w, p.control).moving_turn_at = Some(t);
    w.face(p.obj, 0.0);
}

/// attackRunPath(patterns, unit, maxFly) (patterns.js:188).
fn attack_run_path<W: World + ?Sized>(w: &mut W, run: &mut AttackRun, p: &PatternsView, unit: &UnitView, max_fly: f64) -> Option<[[f64; 3]; 4]> {
    let v = unit.class.as_ref().and_then(|c| c.max_speed).unwrap_or(30.0);
    if run.a.is_none() {
        run.a = Some(max(160.0, 4.0 * v));
        run.b = Some(max(40.0, v));
    }
    let pos = unit.position;
    let q = tracker_point(w, p)?;
    let mut d = Vector3::default();
    d.sub_vectors(&q, &pos);
    normalize_xz(&mut d);
    let clamp = |y: f64| min(y, max_fly);
    let (a, b) = (run.a.unwrap(), run.b.unwrap_or(f64::NAN));
    Some([
        [pos.x, pos.y, pos.z],
        [q.x - a * d.x, clamp(q.y + 90.0), q.z - a * d.z],
        [q.x - b * d.x, clamp(q.y), q.z - b * d.z],
        [q.x + b * d.x, clamp(q.y + 30.0), q.z + b * d.z],
    ])
}

/// attackRunEnter(patterns, env) (patterns.js:206).
fn attack_run_enter<W: World + ?Sized>(w: &mut W, run: &mut AttackRun, settings: &Settings, p: &PatternsView) {
    w.set_delay(p.agent, 15.0, now(w));
    let unit = w.unit(p.control);
    if let Some(path) = attack_run_path(w, run, p, &unit, settings.max_fly_height) {
        if p.helper {
            w.helper_follow_path(p.agent, &path, false, false);
        }
    }
}

/// attackRunTick(patterns, env) (patterns.js:216).
fn attack_run_tick<W: World + ?Sized>(w: &mut W, run: &mut AttackRun, settings: &Settings, p: &PatternsView) {
    let unit = w.unit(p.control);
    if let Some(target) = p.fresh_target {
        let fwd = w.forward(p.control);
        let mut d = Vector3::default();
        d.sub_vectors(&w.unit(target).position, &unit.position);
        if fwd.dot(&d) > 0.5 {
            w.fire_a(p.controller);
        }
    }
    if p.helper && p.helper_arrived {
        attack_run_enter(w, run, settings, p);
    }
    let speed = 0.5 * unit.class.as_ref().and_then(|c| c.max_speed).unwrap_or(30.0);
    min_speed(w, p.control, speed, now(w));
    w.face(p.obj, 0.0);
}

/// pursuitEnter(patterns) (patterns.js:230), dormant: no stock dispatch
/// selects pattern 14.
fn pursuit_enter<W: World + ?Sized>(w: &mut W, p: &PatternsView, st: &mut PatternState) {
    st.loop_ = false;
    st.inverted = false;
    w.set_delay(p.agent, 10.0, now(w));
}

/// pursuitTick(patterns, env) (patterns.js:236), dormant.
fn pursuit_tick<W: World + ?Sized>(w: &mut W, settings: &Settings, p: &PatternsView, st: &mut PatternState) {
    let unit = w.unit(p.control);
    let target = p.fresh_target.map(|t| w.unit(t));
    let fwd = w.forward(p.control);
    let mut d = Vector3::default();
    if let Some(t) = &target {
        d.sub_vectors(&t.position, &unit.position);
        if fwd.dot(&d) > 0.0 || w.int(1.0, 3.0) == 1.0 {
            w.fire_a(p.controller);
        }
    }
    let v = unit.class.as_ref().and_then(|c| c.max_speed).unwrap_or(30.0);
    if !st.loop_ {
        if w.int(1.0, 13.0) == 1.0 {
            st.loop_ = true;
        } else if let Some(t) = &target {
            d.sub_vectors(&t.position, &unit.position);
            let close = d.length_sq() <= v8math::pow(2.0 * v, 2.0);
            let mut pp = Vector3::default();
            if close {
                pp.copy(&unit.position).add_scaled_vector(&fwd, 5.0 * v);
                pp.y = (settings.min_fly_height + settings.max_fly_height) / 2.0;
            } else {
                pp.copy(&t.position).add(&t.velocity);
                pp.y = settings.max_fly_height;
            }
            if p.helper {
                w.helper_goto(p.agent, &pp, true);
            }
        }
    } else {
        let up_y = unit.up_y.unwrap_or(1.0);
        if up_y < 0.0 {
            st.inverted = true;
        } else if st.inverted && up_y > 0.0 {
            st.loop_ = false;
            st.inverted = false;
        }
    }
    let t = now(w);
    let steer = steer_state(w, p.control);
    steer.loop_ = Some(st.loop_);
    steer.loop_at = Some(t);
    min_speed(w, p.control, 0.5 * v, t);
}

/// orbitEnter(patterns) (patterns.js:277): the tow orbit's sides drawn.
fn orbit_enter<W: World + ?Sized>(w: &mut W, p: &PatternsView, st: &mut PatternState) {
    if p.helper {
        w.helper_stop(p.agent);
    }
    st.flip_a = (w.next_a() & 1) == 0;
    st.flip_b = (w.next_a() & 1) == 0;
    w.set_delay(p.agent, 10.0, now(w));
    orbit_request(w, p, st);
}

/// orbitRequest(patterns, st) (patterns.js:287).
fn orbit_request<W: World + ?Sized>(w: &mut W, p: &PatternsView, st: &PatternState) {
    let unit = w.unit(p.control);
    let target = p.target.map(|t| Ref { obj: t, generation: w.unit(t).generation });
    let towed = unit.vehicle.map(|h| w.hull(h).tow_target).unwrap_or(-1.0) >= 0.0;
    let orbit = Orbit {
        target,
        offset: if towed { Vector3::new(0.0, 0.0, 8.75) } else { Vector3::new(if st.flip_a { -12.5 } else { 12.5 }, 0.0, 25.0) },
        radius: if st.flip_b { -25.0 } else { 25.0 },
        factor: if towed { 1.0 } else { 2.5 },
    };
    steer_state(w, p.control).orbit = Some(Some(orbit));
}

/// orbitTick(patterns) (patterns.js:300).
fn orbit_tick<W: World + ?Sized>(w: &mut W, p: &PatternsView, st: &PatternState) {
    orbit_request(w, p, st);
    w.face(p.obj, 0.0);
}

/// orbitExit(patterns) (patterns.js:305).
fn orbit_exit<W: World + ?Sized>(w: &mut W, p: &PatternsView) {
    if let Some(steer) = w.steer(p.control).as_mut() {
        steer.orbit = Some(None);
    }
    w.default_delay(p.agent, now(w));
}

/// directEnter / directTick (patterns.js:312), dormant pattern 16.
fn direct_enter<W: World + ?Sized>(w: &mut W, p: &PatternsView) {
    if p.helper {
        w.helper_stop(p.agent);
    }
    w.set_delay(p.agent, 10.0, now(w));
}

fn direct_tick<W: World + ?Sized>(w: &mut W, p: &PatternsView) {
    if let Some(target) = p.fresh_target {
        let at = w.unit(target).position;
        if p.helper {
            w.helper_goto(p.agent, &at, false);
        }
    }
}

/// minSpeed(unit, speed, time) (patterns.js:324).
fn min_speed<W: World + ?Sized>(w: &mut W, control: Obj, speed: f64, time: f64) {
    let steer = steer_state(w, control);
    steer.min_speed = Some(speed);
    steer.min_speed_at = Some(time);
}

/// enterPattern(patterns, state, env) (patterns.js:332).
pub fn enter_pattern<W: World + ?Sized>(w: &mut W, run: &mut AttackRun, settings: &Settings, p: &PatternsView, state: &mut Option<PatternState>, pattern: f64) {
    match pattern {
        x if x == 7.0 => hover_enter(w, p, state.get_or_insert_with(PatternState::default)),
        x if x == 13.0 => attack_run_enter(w, run, settings, p),
        x if x == 14.0 => pursuit_enter(w, p, state.get_or_insert_with(PatternState::default)),
        x if x == 15.0 => orbit_enter(w, p, state.get_or_insert_with(PatternState::default)),
        x if x == 16.0 => direct_enter(w, p),
        _ => {}
    }
}

/// tickPattern(patterns, state, dt, env) (patterns.js:348).
pub fn tick_pattern<W: World + ?Sized>(w: &mut W, run: &mut AttackRun, settings: &Settings, p: &PatternsView, state: &mut Option<PatternState>, pattern: f64) {
    match pattern {
        x if x == 7.0 => hover_tick(w, p, state.get_or_insert_with(PatternState::default)),
        x if x == 13.0 => attack_run_tick(w, run, settings, p),
        x if x == 14.0 => pursuit_tick(w, settings, p, state.get_or_insert_with(PatternState::default)),
        x if x == 15.0 => {
            let st = *state.get_or_insert_with(PatternState::default);
            orbit_tick(w, p, &st);
        }
        x if x == 16.0 => direct_tick(w, p),
        _ => {}
    }
}

/// exitPattern(patterns, state) (patterns.js:364).
pub fn exit_pattern<W: World + ?Sized>(w: &mut W, p: &PatternsView, pattern: f64) {
    if let Some(steer) = w.steer(p.control).as_mut() {
        steer.min_speed = Some(0.0);
        steer.loop_ = Some(false);
    }
    if pattern == 15.0 {
        orbit_exit(w, p);
        return;
    }
    if pattern >= 13.0 {
        w.default_delay(p.agent, now(w));
    }
}

/// sidestep(patterns, perSecond) (patterns.js:378): pattern 2's sidestep
/// for a slow hull that can strafe, about once in 13 s.
pub fn sidestep<W: World + ?Sized>(w: &mut W, p: &PatternsView, per_second: f64) -> bool {
    let unit = w.unit(p.control);
    let c = unit.class.clone().unwrap_or_default();
    if !(forward_speed(&unit) < 13.0) {
        return false;
    }
    if !(f(c.strafe_speed.unwrap_or(0.0)) > 0.0001) {
        return false;
    }
    if w.int(1.0, 13.0 * per_second) != 1.0 {
        return false;
    }
    let side = w.int(0.0, 1.0) * 2.0 - 1.0;
    let r = w.sphere(p.control).radius;
    let distance = if r == 2.0 * r { r } else { f(r + w.uniform() * (2.0 * r - r)) };
    let yaw = unit.body_yaw;
    // The transform's right row (the 2004 game's, which is the port's left).
    let d = Vector3::new(cos(yaw), 0.0, -sin(yaw));
    let mut pp = unit.position;
    pp.add_scaled_vector(&d, side * distance);
    if p.helper {
        w.helper_goto(p.agent, &pp, true);
    }
    true
}
