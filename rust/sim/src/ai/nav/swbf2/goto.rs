//! 2005 ground movement helper and per-tick control update, completely
//! separate from the released 2004 `GroundGoto` implementation in `nav/goto.rs`.

use super::super::analogs::{analogs_with_turn, apply_jet_and_posture, Intent};
use super::super::goto::{GotoMode, GotoWorld};
use super::super::math::{d2, f32, h2, normalize, Vec3};
use super::super::planner::PathNode;
use super::super::units::{has_jetpack, traversal_mask, traversal_radius, Air, NavUnit, UnitRef};
use super::super::{ControlHost, Nav};
use super::cursor::Swbf2Cursor;
use super::graph::{RouteQuery, ARC_JET_JUMP, ARC_JUMP};
use super::ProbeKind;

/// The leap branch of a 2005 soldier's controller update (the game's leap flag, taken only while
/// no jet sequence runs): one jump press carrying the vector (the jump launches along it), and
/// nothing after it: no second press, no jet. The request and the vector are spent whether or not
/// the jump can start (in the air or under a lock the game's jump refuses and the flag is gone);
/// returns whether it was taken.
pub(super) fn leap_swbf2(unit: &NavUnit, intent: &mut Intent, vector: Vec3) -> bool {
    let Some(s) = unit.soldier.as_ref() else { return false };
    if s.lock > 0.0 || s.air != Air::None {
        return true;
    }
    let aimed = (vector.x * vector.x + vector.y * vector.y + vector.z * vector.z > 0.0).then_some(vector);
    intent.jump = true;
    intent.jet_direction = Some(aimed);
    true
}

/// The jet branch of a 2005 soldier's controller update, then the released posture branch.
/// The game's flags: the request (here `pending`), the running sequence (`jetting`) and the
/// end-jet flag (`end`). A request first presses jump from the ground, and that press carries
/// the request's vector (the game's jump reads it: a bot's jump along the vector); the body in
/// its jump then presses again with the vector, which lights the jet; the sequence lasts while
/// the body jets. An end request is consumed while a sequence runs: a body in its jet presses
/// jump, which ends the jet into a fall (the game's end-jet call); in any other posture it does
/// nothing. Returns the request, the sequence and the end flag as they are left.
pub(super) fn apply_jet_and_posture_swbf2(
    unit: &NavUnit,
    intent: &mut Intent,
    pending: bool,
    jetting: bool,
    vector: Vec3,
    stance: f64,
    end: bool,
) -> (bool, bool, bool) {
    let running = pending || jetting;
    let Some(s) = unit.soldier.as_ref().filter(|_| running) else {
        let (pending, jetting) = apply_jet_and_posture(unit, intent, pending, jetting, vector, stance);
        return (pending, jetting, end);
    };
    if s.lock > 0.0 {
        return (false, jetting, end);
    }
    let aimed = (vector.x * vector.x + vector.y * vector.y + vector.z * vector.z > 0.0).then_some(vector);
    let mut jetting = jetting;
    if end && s.air == Air::Jet {
        intent.jump = true;
    }
    if s.air == Air::Jump {
        intent.jump = true;
        intent.jet_direction = Some(aimed);
    } else if !jetting {
        intent.jump = true;
        intent.jet_direction = Some(aimed);
        jetting = true;
    } else if s.air != Air::Jet {
        jetting = false;
    }
    (false, jetting, false)
}

/// Extended world queries used only on 2005 navigation paths.
pub trait Swbf2GotoWorld: GotoWorld {
    /// Upcast to `GotoWorld` without relying on unstable/newer trait upcasting.
    fn as_goto_world_mut(&mut self) -> &mut dyn GotoWorld;

    /// Whether a ground unit can walk directly from `from` to `to` without
    /// hitting static walls or buildings.
    fn line_of_walk(&mut self, from: Vec3, to: Vec3, unit: &NavUnit) -> bool;
    fn crosses(&mut self, from: Vec3, to: Vec3, unit: &NavUnit, _movement: u8) -> bool {
        self.line_of_walk(from, to, unit)
    }
    /// The same check within the tick's shared work. `None` asks again in a
    /// later tick: it is neither clear nor blocked.
    fn try_crosses(
        &mut self,
        from: Vec3,
        to: Vec3,
        unit: &NavUnit,
        movement: u8,
        _kind: ProbeKind,
    ) -> Option<bool> {
        Some(self.crosses(from, to, unit, movement))
    }
    fn observe_navigation(&mut self, nav: &mut super::super::route::MapNavigation) -> bool {
        self.collision_snapshot()
            .is_some_and(|(e, f)| nav.observe_collision(e, f))
    }
    fn collision_snapshot(&mut self) -> Option<((u64, u64), &[u8])> {
        None
    }
    fn objective_approach_radius(&self, _unit: &NavUnit, _point: Vec3) -> Option<f64> {
        None
    }
    fn step_floor(&mut self, point: Vec3) -> Option<Vec3> {
        self.foot(point)
    }
    fn combat_floor(&mut self, point: Vec3) -> Option<Vec3> {
        self.foot(point)
    }
    fn sees_approach(&mut self, _at: Vec3, _target: Vec3) -> bool {
        true
    }
    fn approach_floor(&mut self, point: Vec3) -> Option<Vec3> {
        self.foot(point)
    }
    fn foot(&mut self, p: Vec3) -> Option<Vec3> {
        Some(p)
    }
}

/// The plan's place number `place` was searched (or had no floor) and nothing
/// was found, unless the search stopped for this tick's work.
fn out_of_reach(h: &mut Swbf2GroundGoto, place: &mut u8, found: bool) {
    if !found && !h.budget.waiting && h.tried <= *place {
        h.tried = *place + 1;
    }
    *place = place.saturating_add(1);
}

/// The game's path follower calls a soldier stuck after three seconds within
/// two metres of one spot.
const STALL_SECONDS: f64 = 3.;

/// A 2005 bot's turn rate over its MaxTurnSpeed: four times for a soldier (the game's soldier
/// tick), once for anything else.
pub(crate) fn bot_turn(unit: &NavUnit) -> f64 {
    if unit.kind == super::super::units::kind::SOLDIER { crate::movement::soldier::swbf2::SWBF2_AI_TURN } else { 1.0 }
}

/// Off the ground: jumping, jetting or falling.
fn airborne(unit: &NavUnit) -> bool {
    unit.soldier.as_ref().is_some_and(|s| s.air != Air::None)
}

/// The stick of a body jetting at a target: full travel toward the point on the flat, mapped
/// to the class's forward and strafe as the landing stick maps it. None out of the jet.
pub(super) fn jet_stick(unit: &NavUnit, point: Vec3) -> Option<(f64, f64)> {
    let s = unit.soldier.as_ref()?;
    if s.air != Air::Jet {
        return None;
    }
    let (dx, dz) = (point.x - unit.position.x, point.z - unit.position.z);
    let d = v8math::hypot(dx, dz);
    if !(d > 1e-6) {
        return Some((0., 0.));
    }
    let coef = super::super::analogs::coefficients(unit);
    let yaw = s.aim_yaw.unwrap_or(0.);
    let (fx, fz) = (v8math::sin(yaw), v8math::cos(yaw));
    let (ux, uz) = (dx / d, dz / d);
    let a = fx * ux + fz * uz;
    let b = -(fz * ux - fx * uz);
    if coef.strafe <= 1e-9 {
        return Some((v8math::min(v8math::max(a, 0.), 1.), 0.));
    }
    let (u, w) = (a / coef.forward, b / coef.strafe);
    Some(if v8math::abs(u) <= v8math::abs(w) {
        (u / v8math::abs(w), if w <= 0. { -1. } else { 1. })
    } else {
        (if u <= 0. { -1. } else { 1. }, w / v8math::abs(u))
    })
}

/// The stick of a soldier in a jump or a fall. In the air the 2005 stick
/// accelerates the body with no speed limit, so steering straight at the
/// waypoint carries a bot past it. Steer instead for the velocity that brings
/// it down on the waypoint (the waypoint's height reached at the gravity the
/// movers use), the stick at full travel when a metre a second or more is
/// still missing.
fn landing_stick(unit: &NavUnit, waypoint: Vec3) -> Option<(f64, f64)> {
    let s = unit.soldier.as_ref()?;
    if !matches!(s.air, Air::Jump | Air::Other) {
        return None;
    }
    let (p, v) = (unit.position, unit.velocity);
    let g = crate::movement::soldier::GRAVITY;
    let disc = v.y * v.y + 2. * g * (p.y - waypoint.y);
    let t = if disc > 0. { (v.y + v8math::sqrt(disc)) / g } else { v.y / g };
    if !(t > 1. / 60.) {
        return None;
    }
    let (wx, wz) = ((waypoint.x - p.x) / t - v.x, (waypoint.z - p.z) / t - v.z);
    let need = v8math::hypot(wx, wz);
    if !(need > 1e-6) {
        return Some((0., 0.));
    }
    let scale = v8math::min(need, 1.);
    let coef = super::super::analogs::coefficients(unit);
    let yaw = s.aim_yaw.unwrap_or(0.);
    let (fx, fz) = (v8math::sin(yaw), v8math::cos(yaw));
    let (ux, uz) = (wx / need, wz / need);
    let a = fx * ux + fz * uz;
    let b = -(fz * ux - fx * uz);
    if coef.strafe <= 1e-9 {
        return Some((v8math::min(v8math::max(a, 0.), 1.) * scale, 0.));
    }
    let (u, w) = (a / coef.forward, b / coef.strafe);
    Some(if v8math::abs(u) <= v8math::abs(w) {
        ((u / v8math::abs(w)) * scale, (if w <= 0. { -1. } else { 1. }) * scale)
    } else {
        ((if u <= 0. { -1. } else { 1. }) * scale, (w / v8math::abs(u)) * scale)
    })
}

/// A soldier at rest settles by a hair from tick to tick. It asks again from
/// where it first asked, so that a check can go on.
/// A soldier that asked to turn waits this many ticks for the answer where it asked.
const TURN_WAIT: u32 = 8;

/// Whether a turn asked on tick `since` still waits on tick `now`; a tick before `since` and the
/// end of the tick range both read as still waiting or done, never as a wrapped sum.
fn turn_waits(now: u32, since: u32) -> bool {
    now.saturating_sub(since) < TURN_WAIT
}

fn resting_place(before: Option<Waiting>, unit: &NavUnit) -> Vec3 {
    before
        .map(|w| w.from)
        .filter(|&p| d2(p, unit.position) <= 0.01)
        .unwrap_or(unit.position)
}

/// An order that goes on in a later tick, when there is work for it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Waiting {
    /// Where its plan began. It is searched from there, so that each tick
    /// goes on with the checks of the last one.
    pub(super) from: Vec3,
    since: u32,
    stamp: u32,
    kind: ProbeKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Placed {
    Leg,
    Waiting,
    Failed,
}

/// Dedicated 2005 ground movement helper. Chosen once at helper construction
/// when `Nav::swbf2` is present, leaving 2004 `GroundGoto` untouched.
#[derive(Clone, Debug)]
pub struct Swbf2GroundGoto {
    pub control: usize,
    pub destination: Vec3,
    requested: Vec3,
    mode: GotoMode,
    pub failed: bool,
    pub cursor: Option<Swbf2Cursor>,
    arrived: bool,
    target: Option<UnitRef>,
    follow_distance: f64,
    remembered: bool,
    combat: bool,
    last_refresh: f64,
    replans: u8,
    pub(super) budget: super::walk::SearchBudget,
    pub(super) waiting: Option<Waiting>,
    progress_origin: Vec3,
    stationary: f64,
    aim_point: Vec3,
    aim_target: Option<UnitRef>,
    /// An order given in the air, planned where the soldier lands.
    deferred: bool,
    /// The order's places to walk to (its point, the objective's floor, the
    /// approaches around it) already found out of reach, so that a plan that
    /// waits for a later tick goes on with the next.
    tried: u8,
}

impl Swbf2GroundGoto {
    pub fn new(control: usize) -> Self {
        Self {
            control,
            destination: Vec3::default(),
            requested: Vec3::default(),
            mode: GotoMode::Route,
            failed: false,
            cursor: None,
            arrived: true,
            target: None,
            follow_distance: 0.,
            remembered: false,
            combat: false,
            last_refresh: 0.,
            replans: 0,
            budget: super::walk::SearchBudget::new(),
            waiting: None,
            progress_origin: Vec3::default(),
            stationary: 0.,
            aim_point: Vec3::default(),
            aim_target: None,
            deferred: false,
            tried: 0,
        }
    }
}

impl super::super::route::MapNavigation {
    /// 2005 per-tick path update: runs the path executor and triggers one-shot
    /// jump (`0x20`) or jet-jump (`0x40`) actions when crossing flagged nodes.
    pub fn swbf2_control_update(
        &mut self,
        control: usize,
        dt: f64,
        unit: &mut NavUnit,
        world: &mut dyn Swbf2GotoWorld,
    ) {
        if !self.check_ground_leg(control, unit, world) {
            return;
        }
        let leader_ref = self.controls[control].leader;
        let leader = leader_ref.and_then(|r| world.leader(r));
        if let Some(leader) = &leader {
            self.control_sample_trail(control, dt, leader, world.as_goto_world_mut(), unit);
        }
        let has_executor = self.controls[control].executor.is_some();
        if !has_executor && self.controls[control].stranded {
            self.controls[control].stranded_for = f32(self.controls[control].stranded_for + dt);
        } else if self.controls[control].stranded_for != 0.0
            && d2(unit.position, self.controls[control].stranded_at) > 4.0
        {
            self.controls[control].stranded_for = 0.0;
        }
        let Some(mut executor) = self.controls[control].executor.take() else {
            return;
        };
        // Ground waypoints are handed to the mover one at a time. Keep the
        // executor steering through its ordinary one-metre arrival circle.
        let delta = executor.waypoint;
        let close = h2(delta.x, delta.z, unit.position.x, unit.position.z) < 0.0016
            && (delta.y - unit.position.y).abs() < 0.55;
        if unit.soldier.is_some() {
            let state = self.swbf2.as_mut().unwrap();
            let leg = state.legs[control];
            executor.hold = state.held[control]
                || airborne(unit)
                || !(close
                    || leg.is_some_and(|l| {
                        (l.local
                            && (l.movement & super::walk::DROP == 0
                                || (unit.position.y - delta.y).abs() < 0.55)
                            && self
                                .common
                                .barriers
                                .point_clear(unit.position, l.radius, l.mask)
                                .0)
                            || (l.final_leg
                                && (unit.position.y - delta.y).abs() < 0.55
                                && h2(delta.x, delta.z, unit.position.x, unit.position.z) <= 1.
                                && super::walk::clear(
                                    &mut self.common.barriers,
                                    unit.position,
                                    delta,
                                    l.radius,
                                    l.mask,
                                )
                                && state.take_execution_check()
                                && world.try_crosses(
                                    unit.position,
                                    delta,
                                    unit,
                                    0,
                                    ProbeKind::Turn,
                                ) == Some(true))
                    }));
        }
        let complete = {
            let Nav {
                store,
                controls,
                queue,
                current,
                ..
            } = &mut self.common;
            let mut host = ControlHost {
                controls,
                queue,
                current,
                control,
            };
            executor.tick(store, &mut host, dt, unit);
            executor.complete
        };
        if complete && leader.is_none() {
            executor.release(&mut self.store);
        } else if self.controls[control].executor.is_none() {
            self.controls[control].executor = Some(executor);
        } else {
            executor.release(&mut self.store);
        }
    }

    /// 2005 navigation half of the AI input step.
    pub fn swbf2_navigation_input(
        &mut self,
        control: usize,
        unit: &mut NavUnit,
        dt: f64,
        aim: Option<Vec3>,
        facing: bool,
        world: &mut dyn Swbf2GotoWorld,
    ) -> Intent {
        self.control_attach(control, Some(unit));
        let pre_parapet = self
            .swbf2
            .as_ref()
            .and_then(|s| s.legs.get(control))
            .and_then(|l| *l)
            .is_some_and(|l| {
                l.local
                    && l.movement == super::walk::PARAPET_DROP
                    && unit.position.y > l.end.y + 0.75
            });
        if pre_parapet {
            if let Some(s) = unit.soldier.as_mut() {
                s.avoidance = Some(Vec3::default());
            }
        }
        self.swbf2_control_update(control, dt, unit, world);
        let (request, hold) = {
            let c = &self.controls[control];
            (c.request, c.executor.as_ref().map(|e| e.hold))
        };
        let active_leg = self
            .swbf2
            .as_ref()
            .and_then(|s| s.legs.get(control))
            .and_then(|l| *l);
        let parapet_target = active_leg
            .filter(|l| {
                l.local
                    && l.movement == super::walk::PARAPET_DROP
                    && request.valid
                    && unit.position.y > l.end.y + 0.75
            })
            .map(|l| l.end);
        let escaping_barrier = active_leg.is_some_and(|l| {
            l.local
                && request.valid
                && !self
                    .common
                    .barriers
                    .point_clear(unit.position, l.radius, l.mask)
                    .0
        });
        let (aim_used, facing_used, hold_used) = match parapet_target {
            Some(end) => (Some(end), true, Some(false)),
            None if escaping_barrier => (aim, facing, Some(false)),
            None => (aim, facing, hold),
        };
        let mut intent = analogs_with_turn(unit, Some(&request), aim_used, dt, facing_used, hold_used, bot_turn(unit));
        if request.valid {
            if let Some((forward, right)) = landing_stick(unit, request.waypoint) {
                intent.forward = forward;
                intent.right = right;
            }
        }
        if let Some((forward, right)) = self.controls[control].jet.steer.and_then(|point| jet_stick(unit, point)) {
            intent.forward = forward;
            intent.right = right;
        }
        let (pending, jetting, vector, stance, end, leap) = {
            let c = &self.controls[control];
            (
                c.jet.pending,
                c.jetting,
                c.jet.vector,
                if parapet_target.is_some() { 0.0 } else { c.stance },
                c.jet.end,
                c.jet.leap,
            )
        };
        let leaped = leap && !(pending || jetting) && leap_swbf2(unit, &mut intent, vector);
        let (pending, jetting, end) =
            apply_jet_and_posture_swbf2(unit, &mut intent, pending, jetting, vector, stance, end);
        let c = &mut self.controls[control];
        c.jet.pending = pending;
        c.jetting = jetting;
        c.jet.end = end;
        if leaped {
            c.jet.leap = false;
            c.jet.vector = Vec3::default();
        }
        if let Some(swbf2) = self.swbf2.as_mut() {
            if swbf2.take_jump_pending(control) {
                intent.jump = true;
            }
            if let Some(end) = parapet_target {
                if let Some(leg) = swbf2.legs.get_mut(control).and_then(Option::as_mut) {
                    let stalled_facing = unit.soldier.as_ref().is_some_and(|s| {
                        if s.air != Air::None || s.lock > 0.0 {
                            return false;
                        }
                        let (dx, dz) = (end.x - unit.position.x, end.z - unit.position.z);
                        let dist = v8math::hypot(dx, dz);
                        if dist <= 0.25 {
                            return false;
                        }
                        let v = unit.velocity;
                        if v.x * dx + v.z * dz >= dist {
                            return false;
                        }
                        let yaw = s.aim_yaw.unwrap_or(0.0);
                        v8math::sin(yaw) * dx + v8math::cos(yaw) * dz > 0.7 * dist
                    });
                    if stalled_facing {
                        leg.parapet_stall = leg.parapet_stall.saturating_add(1);
                        if leg.parapet_stall >= 4 {
                            intent.jump = true;
                            intent.right = 0.0;
                            intent.forward = v8math::max(intent.forward, 0.75);
                            leg.parapet_stall = 0;
                        }
                    } else {
                        leg.parapet_stall = 0;
                    }
                }
            }
        }
        intent
    }

    /// Cheap, deterministic, zero-allocation reachability and route-length query
    /// between two world points. Works uniformly across 2005 graphs, 2004 graphs,
    /// and graphless barrier/collision worlds.
    #[allow(clippy::too_many_arguments)]
    pub fn route_distance<F>(
        &mut self,
        from: Vec3,
        to: Vec3,
        radius: f64,
        mask: u32,
        can_jet: bool,
        mut line_of_walk: F,
    ) -> RouteQuery
    where
        F: FnMut(Vec3, Vec3) -> bool,
    {
        self.distance_with_clearance(from, to, radius, mask, can_jet, false, |a, b, _| {
            line_of_walk(a, b)
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn distance_with_clearance<F>(
        &mut self,
        from: Vec3,
        to: Vec3,
        radius: f64,
        mask: u32,
        can_jet: bool,
        use_cache: bool,
        mut line_of_walk: F,
    ) -> RouteQuery
    where
        F: FnMut(Vec3, Vec3, u8) -> bool,
    {
        self.try_distance_with_clearance(from, to, radius, mask, can_jet, use_cache, |a, b, m| {
            Some(line_of_walk(a, b, m))
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn try_distance_with_clearance<F>(
        &mut self,
        from: Vec3,
        to: Vec3,
        radius: f64,
        mask: u32,
        can_jet: bool,
        use_cache: bool,
        mut line_of_walk: F,
    ) -> RouteQuery
    where
        F: FnMut(Vec3, Vec3, u8) -> Option<bool>,
    {
        if mask == 0 || !radius.is_finite() || radius < 0. {
            return RouteQuery::UNKNOWN;
        }
        if let Some(state) = &mut self.swbf2 {
            if mask != 1 || !state.has_query_credit() {
                return RouteQuery::UNKNOWN;
            }
            state.queries_left -= 1;
            if !self.common.barriers.point_clear(from, radius, mask).0 {
                // A mover can already overlap an inflated panel while standing
                // beside it. Its controller may separate from it on a later tick.
                return RouteQuery::UNKNOWN;
            }
            if !self.common.barriers.point_clear(to, radius, mask).0 {
                return if self.common.barriers.point_clear(to, 0., mask).0 {
                    RouteQuery::UNKNOWN
                } else {
                    RouteQuery::UNREACHABLE
                };
            }
            if let Some(graph) = state.graph.as_ref().filter(|g| !g.hubs.is_empty()) {
                if state.floors.disconnected(graph, from, to, mask, can_jet) {
                    return RouteQuery::UNREACHABLE;
                }
                struct WalkOnly<F>(F);
                impl<F: FnMut(Vec3, Vec3, u8) -> Option<bool>> super::walk::PointProbe for WalkOnly<F> {
                    fn foot(&mut self, p: Vec3) -> Option<Vec3> {
                        Some(p)
                    }
                    fn walk(&mut self, a: Vec3, b: Vec3, m: u8) -> Option<bool> {
                        (self.0)(a, b, m)
                    }
                }
                let mut probe = WalkOnly(&mut line_of_walk);
                return state
                    .point_trip(
                        &mut self.common.barriers,
                        from,
                        to,
                        radius,
                        mask,
                        can_jet,
                        use_cache,
                        &mut super::walk::SearchBudget::new(),
                        &mut None,
                        0.,
                        &mut probe,
                    )
                    .map_or(RouteQuery::UNKNOWN, |t| t.result);
            }
        }
        let distance = v8math::sqrt(d2(from, to));
        if (distance < 50. || self.graph.is_none())
            && line_of_walk(from, to, 0) == Some(true)
            && self.barriers.point_clear(from, radius, mask).0
            && self.barriers.point_clear(to, radius, mask).0
            && !self.barriers.segment_blocked(from, to, radius, mask, None)
        {
            return RouteQuery::found(distance, 0);
        }
        self.graph.as_ref().map_or(RouteQuery::UNKNOWN, |g| {
            route_distance_2004(g, from, to, mask)
        })
    }

    pub fn swbf2_ground_goto(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &mut NavUnit,
        world: &mut dyn Swbf2GotoWorld,
        point: Vec3,
        mode: GotoMode,
    ) {
        self.ground_approach(h, unit, world, point, mode, false);
    }

    pub(crate) fn ground_approach(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &mut NavUnit,
        world: &mut dyn Swbf2GotoWorld,
        point: Vec3,
        mode: GotoMode,
        combat: bool,
    ) {
        h.target = None;
        let same = h.combat == combat;
        h.combat = combat;
        if same
            && (h.waiting.is_some()
                || h.cursor
                    .as_ref()
                    .is_some_and(|c| c.revision == self.swbf2.as_ref().unwrap().revision))
            && h.mode == mode
            && d2(h.requested, point) < 0.01
        {
            return;
        }
        if airborne(unit) && self.controls[h.control].executor.is_some() {
            // The soldier keeps steering along the leg it left the ground on,
            // so it comes down where that leg was planned.
            h.mode = mode;
            h.requested = point;
            h.failed = false;
            h.arrived = false;
            h.deferred = true;
            return;
        }
        self.start_order(h, unit, world, point, mode);
    }

    fn start_order(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &mut NavUnit,
        world: &mut dyn Swbf2GotoWorld,
        point: Vec3,
        mode: GotoMode,
    ) {
        h.deferred = false;
        self.clear_order_path(h.control);
        h.mode = mode;
        h.replans = 0;
        h.budget = super::walk::SearchBudget::new();
        h.progress_origin = unit.position;
        h.stationary = 0.;
        self.begin_walk(h, unit, world, point);
    }

    pub fn swbf2_ground_install_path(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &mut NavUnit,
        world: &mut dyn Swbf2GotoWorld,
        points: &[Vec3],
    ) {
        let Some(&first) = points.first() else { return };
        h.target = None;
        self.clear_order_path(h.control);
        let path = self.common.store.make();
        for point in points {
            if !self.common.store.append(
                path,
                PathNode {
                    x: point.x,
                    y: point.y,
                    z: point.z,
                    flag: 0,
                },
            ) {
                self.common.store.release(path);
                self.fail_walk(h, unit);
                return;
            }
        }
        self.swbf2.as_mut().unwrap().orders[h.control] = Some((path, 1));
        h.mode = GotoMode::Route;
        h.combat = false;
        h.replans = 0;
        h.budget = super::walk::SearchBudget::new();
        h.progress_origin = unit.position;
        h.stationary = 0.;
        self.begin_walk(h, unit, world, first);
    }

    fn begin_walk(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &mut NavUnit,
        world: &mut dyn Swbf2GotoWorld,
        point: Vec3,
    ) {
        self.control_attach(h.control, Some(unit));
        h.deferred = false;
        h.tried = 0;
        h.failed = false;
        h.arrived = false;
        h.requested = point;
        h.destination = point;
        h.cursor = None;
        self.continue_walk(h, unit, world);
    }

    /// Place the requested order, or go on with one that waits for work.
    fn continue_walk(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &mut NavUnit,
        world: &mut dyn Swbf2GotoWorld,
    ) {
        let before = h.waiting.take();
        if h.mode != GotoMode::Route {
            let start = resting_place(before, unit);
            if self.local_segment(h, unit, world, start, h.requested) == Placed::Waiting
                && !self.wait_walk(h, before, start, ProbeKind::Step)
            {
                self.fail_walk(h, unit);
            }
            return;
        }
        // A soldier on the move keeps the start of its plan while it is near.
        let start = before
            .map(|w| w.from)
            .filter(|&p| d2(p, unit.position) <= 4.)
            .unwrap_or(unit.position);
        world.observe_navigation(self);
        let valid = self.check_ground_leg(h.control, unit, world);
        let moving = valid
            && self.controls[h.control].executor.is_some()
            && self.swbf2.as_ref().unwrap().legs[h.control].is_some();
        // Keep one of this order's checks for a local step when starting from rest.
        let reserve = usize::from(!moving && h.budget.drives > 1);
        h.budget.drives -= reserve;
        let route = self.plan_walk(h, unit, world, start, h.requested);
        let exhausted = h.budget.spent();
        h.budget.drives += reserve;
        let waiting = std::mem::take(&mut h.budget.waiting);
        if route.is_none() && waiting && self.wait_walk(h, before, start, ProbeKind::Plan) {
            // The leg under way is still clear. Finish it meanwhile.
            if let Some(leg) = self.swbf2.as_mut().unwrap().legs[h.control].as_mut() {
                leg.final_leg = true;
                leg.destination = leg.end;
            }
            return;
        }
        if let Some(cursor) = route {
            h.cursor = Some(cursor);
            self.advance_walk(h, unit, world, false);
        } else if waiting || exhausted || self.barrier_trapped_route(start, h.requested, unit) {
            self.refuse_exhausted(h, unit, world);
        } else {
            self.fail_walk(h, unit);
        }
    }

    fn barrier_trapped_route(&mut self, start: Vec3, requested: Vec3, unit: &NavUnit) -> bool {
        let (radius, mask) = (traversal_radius(unit), traversal_mask(unit));
        if mask == 0 || self.common.barriers.point_clear(start, radius, mask).0 {
            return false;
        }
        let mut probe = start;
        let _ = self.common.barriers.correct(&mut probe, radius, mask);
        self.swbf2.as_ref().is_some_and(|s| {
            s.graph.as_ref().is_none_or(|g| {
                g.hubs.is_empty()
                    || s.floors.may_connect_within(
                        g,
                        probe,
                        requested,
                        [super::walk::MAX_DROP, 8.0],
                        mask,
                    )
            })
        })
    }

    /// Keep the order's place among the waiting. False once it has waited
    /// long enough to fall back.
    fn wait_walk(
        &self,
        h: &mut Swbf2GroundGoto,
        before: Option<Waiting>,
        from: Vec3,
        kind: ProbeKind,
    ) -> bool {
        let state = self.swbf2.as_ref().unwrap();
        let now = state.query_tick.unwrap_or(0);
        let since = before.map_or(now, |w| w.since);
        let waits = now.saturating_sub(since) < super::Swbf2Nav::LONGEST_WAIT;
        h.waiting = waits.then_some(Waiting {
            from,
            since,
            stamp: state.order_stamps[h.control],
            kind,
        });
        waits
    }

    /// Go on with what ran out of a tick's work: the changed leg under way
    /// first, then the order.
    pub fn swbf2_ground_resume(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &mut NavUnit,
        world: &mut dyn Swbf2GotoWorld,
    ) {
        if !self.check_ground_leg(h.control, unit, world) {
            return;
        }
        let Some(w) = h.waiting else { return };
        if w.stamp != self.swbf2.as_ref().unwrap().order_stamps[h.control] {
            // Another helper has ordered this control since.
            h.waiting = None;
        } else if h
            .cursor
            .as_ref()
            .is_some_and(|c| c.revision == self.swbf2.as_ref().unwrap().revision)
        {
            self.advance_walk(h, unit, world, false);
        } else {
            // Not planned yet, or the corridors have changed since.
            h.cursor = None;
            self.continue_walk(h, unit, world);
        }
    }

    /// The walk ended with the soldier's life or at a seat.
    pub(crate) fn swbf2_ground_abandon(&mut self, h: &mut Swbf2GroundGoto) {
        h.waiting = None;
        h.deferred = false;
        if let Some(leg) = self.swbf2.as_mut().unwrap().legs.get_mut(h.control) {
            *leg = None;
        }
    }

    /// Release an external walk's order paths, waiting state, and control.
    pub(crate) fn swbf2_ground_cancel(&mut self, h: &mut Swbf2GroundGoto) {
        self.drop_order(h.control);
        h.deferred = false;
        h.target = None;
        h.cursor = None;
        h.waiting = None;
        h.failed = false;
        h.arrived = false;
        self.control_stop(h.control, None);
        self.control_strand(h.control, false, None);
        let control = &mut self.common.controls[h.control];
        control.request.valid = false;
        control.jet.pending = false;
        control.jetting = false;
        if let Some(state) = self.swbf2.as_mut() {
            if let Some(leg) = state.legs.get_mut(h.control) {
                *leg = None;
            }
            state.set_jump_pending(h.control, false);
            if let Some(held) = state.held.get_mut(h.control) {
                *held = false;
            }
        }
    }

    pub(crate) fn swbf2_ground_waiting(&self, h: &Swbf2GroundGoto) -> bool {
        h.waiting.is_some()
    }

    /// The place of a helper among those that ran out of a tick's work, and
    /// what it waits for. Soldiers that stand on a leg come first, then the
    /// longest waiting orders.
    pub(crate) fn swbf2_ground_place(
        &self,
        slot: usize,
        h: &Swbf2GroundGoto,
    ) -> Option<((bool, u32, usize), ProbeKind)> {
        let leg = self.swbf2.as_ref()?.legs.get(h.control)?.as_ref();
        let (since, kind) = if leg.is_some_and(|leg| leg.check_body) {
            (0, ProbeKind::Leg)
        } else {
            h.waiting.map(|w| (w.since, w.kind))?
        };
        Some(((kind.plans(), since, slot), kind))
    }

    fn refuse_exhausted(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &mut NavUnit,
        world: &mut dyn Swbf2GotoWorld,
    ) {
        self.clear_order_path(h.control);
        if let Some(leg) = self.swbf2.as_mut().unwrap().legs[h.control]
            .as_mut()
            .filter(|l| !l.check_body)
        {
            if self.common.controls[h.control].executor.is_some() {
                leg.final_leg = true;
                leg.destination = leg.end;
                h.failed = true;
                h.cursor = None;
                self.control_strand(h.control, true, Some(unit));
                return;
            }
        }
        if h.budget.drives != 0 {
            // The released local approach steers toward its target on the present floor.
            // Stop at half a second's travel and consume only the remaining order credit.
            let (dx, dz) = (
                h.requested.x - unit.position.x,
                h.requested.z - unit.position.z,
            );
            let stride = unit.soldier.and_then(|s| s.max_speed).unwrap_or(6.) * 0.5;
            let fraction = (stride / v8math::hypot(dx, dz).max(0.01)).min(1.);
            let step = Vec3::new(
                unit.position.x + dx * fraction,
                unit.position.y,
                unit.position.z + dz * fraction,
            );
            if self.local_segment(h, unit, world, unit.position, step) == Placed::Waiting {
                self.fail_walk(h, unit);
                return;
            }
            h.failed = true;
            h.cursor = None;
            self.control_strand(h.control, true, Some(unit));
        } else {
            self.fail_walk(h, unit);
        }
    }

    fn plan_walk(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &NavUnit,
        world: &mut dyn Swbf2GotoWorld,
        start: Vec3,
        point: Vec3,
    ) -> Option<Swbf2Cursor> {
        let tactical = h.combat || h.target.is_some();
        let area = if tactical {
            let stride = unit.soldier.and_then(|s| s.max_speed).unwrap_or(6.) * 0.5;
            let approach = v8math::min(stride, 4.);
            Some(if h.target.is_some() {
                v8math::max(h.follow_distance, approach)
            } else {
                approach
            })
        } else {
            world.objective_approach_radius(unit, point)
        };
        let floor = |world: &mut dyn Swbf2GotoWorld, p| {
            if tactical {
                world.combat_floor(p)
            } else {
                world.approach_floor(p)
            }
        };
        let exact = world
            .foot(point)
            .or_else(|| area.and_then(|_| floor(world, point)));
        if exact.is_none() && area.is_none() {
            return None;
        }
        let mut to = exact.unwrap_or(point);
        h.destination = to;
        let radius = traversal_radius(unit);
        let mask = traversal_mask(unit);
        let cache = world.observe_navigation(self);
        let state = self.swbf2.as_mut().expect("Battlefront II navigation");
        if let Some(graph) = state.graph.as_ref().filter(|g| !g.hubs.is_empty()) {
            // The approaches below lie within the area and up to a storey
            // higher or lower.
            let slack = area.unwrap_or(0.) + 8.;
            if !state.floors.may_connect(graph, start, point, slack, mask) {
                return None;
            }
        }
        let mut entry = to;
        let mut trip = None;
        if state.graph.as_ref().is_some_and(|g| !g.hubs.is_empty()) {
            let objective_floor = if tactical {
                None
            } else {
                area.and_then(|reach| state.floors.objective_floor(point, reach))
            };
            struct WorldProbe<'a> {
                world: &'a mut dyn Swbf2GotoWorld,
                unit: &'a NavUnit,
            }
            impl super::walk::PointProbe for WorldProbe<'_> {
                fn foot(&mut self, p: Vec3) -> Option<Vec3> {
                    self.world
                        .foot(p)
                        .or_else(|| self.world.combat_floor(p))
                        .or_else(|| self.world.step_floor(p))
                }
                fn walk(&mut self, a: Vec3, b: Vec3, m: u8) -> Option<bool> {
                    self.world.try_crosses(a, b, self.unit, m, ProbeKind::Plan)
                }
            }
            let start_slack = area.map_or(0., |r| 2. * r + 8.);
            let mut initial = None;
            let mut route =
                |to, world: &mut dyn Swbf2GotoWorld, budget: &mut super::walk::SearchBudget| {
                    let mut probe = WorldProbe { world, unit };
                    state.point_trip(
                        &mut self.common.barriers,
                        start,
                        to,
                        radius,
                        mask,
                        has_jetpack(unit),
                        cache,
                        budget,
                        &mut initial,
                        start_slack,
                        &mut probe,
                    )
                };
            // Each place found out of reach is not searched again when the
            // plan goes on in a later tick.
            let mut place = 0u8;
            if h.tried == 0 {
                trip = exact.and_then(|p| route(p, world, &mut h.budget));
            }
            out_of_reach(h, &mut place, trip.is_some());
            if trip.is_none() && !h.budget.stopped() {
                if let Some(p) = objective_floor
                    .filter(|&p| exact.is_none_or(|q| d2(p, q) > 0.0001))
                    .filter(|_| h.tried <= 1)
                {
                    trip = route(p, world, &mut h.budget);
                    if trip.is_some() {
                        to = p;
                        h.destination = p;
                    }
                }
            }
            out_of_reach(h, &mut place, trip.is_some());
            if trip.is_none() {
                if let Some(reach) = area {
                    // The controller accepts a position within its objective.
                    // Try supported approaches within that same distance.
                    'approaches: for fraction in [0.5, 0.9] {
                        for [x, z] in [
                            [1., 0.],
                            [-1., 0.],
                            [0., 1.],
                            [0., -1.],
                            [0.707, 0.707],
                            [-0.707, 0.707],
                            [0.707, -0.707],
                            [-0.707, -0.707],
                        ] {
                            if h.budget.stopped() {
                                break 'approaches;
                            }
                            if h.tried > place {
                                place += 1;
                                continue;
                            }
                            let sample = Vec3::new(
                                point.x + reach * fraction * x,
                                point.y,
                                point.z + reach * fraction * z,
                            );
                            if let Some(floor) = floor(world, sample)
                                .filter(|&p| !tactical || world.sees_approach(p, point))
                            {
                                if let Some(t) = route(floor, world, &mut h.budget) {
                                    to = floor;
                                    h.destination = to;
                                    trip = Some(t);
                                    break 'approaches;
                                }
                            }
                            out_of_reach(h, &mut place, false);
                        }
                    }
                }
            }
            let t = trip?;
            entry = state.floors.point(t.start.node);
        } else if exact.is_none()
            || h.budget
                .check(|| world.try_crosses(start, to, unit, 0, ProbeKind::Plan))
                != Some(true)
            || mask == 0
            || !self.common.point_clear(to, radius, mask)
            || !self.common.point_clear(start, radius, mask)
            || self
                .common
                .barriers
                .segment_blocked(start, to, radius, mask, None)
        {
            return None;
        }
        Some(Swbf2Cursor::new(trip, to, entry, state.revision))
    }

    /// Direct approaches and straight recovery moves steer in the target's
    /// horizontal direction. The sampled floor supplies the vertical component.
    fn local_segment(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &mut NavUnit,
        world: &mut dyn Swbf2GotoWorld,
        start: Vec3,
        target: Vec3,
    ) -> Placed {
        let (radius, mask) = (traversal_radius(unit), traversal_mask(unit));
        let origin = start;
        let requested = v8math::hypot(target.x - origin.x, target.z - origin.z);
        if !requested.is_finite() || mask == 0 {
            self.fail_walk(h, unit);
            return Placed::Failed;
        }
        let origin_clear = self.common.barriers.point_clear(origin, radius, mask).0;
        let mut end = target;
        if origin_clear {
            if self
                .barriers
                .obstruct(origin, &mut end, radius, mask, None, None)
            {
                let length = v8math::sqrt(d2(origin, end));
                let back = 0.25 / v8math::max(length, 0.25);
                end.x += (origin.x - end.x) * back;
                end.z += (origin.z - end.z) * back;
            }
        } else {
            self.common
                .barriers
                .obstruct_solid_body_outside_start(origin, &mut end, mask);
        }
        let (dx, dz) = (end.x - origin.x, end.z - origin.z);
        let length = v8math::hypot(dx, dz);
        if !length.is_finite() {
            self.fail_walk(h, unit);
            return Placed::Failed;
        }
        let cached = world.observe_navigation(self);
        let mut floor = origin;
        let steps = (v8math::min(length, 64.) / 0.5).ceil() as usize;
        for step in 1..=steps {
            let t = v8math::min(1., step as f64 * 0.5 / length);
            let Some(next) =
                world.step_floor(Vec3::new(origin.x + dx * t, floor.y, origin.z + dz * t))
            else {
                break;
            };
            floor = next;
        }
        // A wall can cut a long order short. Offer the longest checked prefix.
        let mut best = origin;
        let mut movement = 0u8;
        let mut found = false;
        let mut low = 0.;
        let mut high = 1.;
        for pass in 0..6 {
            let t = if pass == 0 { 1. } else { (low + high) * 0.5 };
            let mid = Vec3::new(
                origin.x + (floor.x - origin.x) * t,
                origin.y + (floor.y - origin.y) * t,
                origin.z + (floor.z - origin.z) * t,
            );
            if let Some(p) = world.foot(mid) {
                let barrier_ok = if origin_clear {
                    super::walk::clear(&mut self.common.barriers, origin, p, radius, mask)
                } else {
                    self.common.barriers.point_clear(p, radius, mask).0
                        && !self
                            .common
                            .barriers
                            .segment_crosses_solid_body_outside_start(origin, p, mask)
                };
                if barrier_ok {
                    let clearance = &mut self.swbf2.as_mut().unwrap().clearance;
                    let budget = &mut h.budget;
                    let mut test = |a, b| {
                        budget.check(|| world.try_crosses(a, b, unit, 0, ProbeKind::Step))
                    };
                    let clear = if cached {
                        clearance.try_check(origin, p, test)
                    } else {
                        test(origin, p)
                    };
                    let Some(clear) = clear else {
                        break;
                    };
                    if clear {
                        best = p;
                        found = true;
                        low = t;
                        if t == 1. {
                            break;
                        }
                        continue;
                    }
                }
            }
            high = t;
        }
        // When standing inside a barrier margin and the requested ray did not exit it,
        // accept the barrier's nearest ejection only if it lands on checked walkable floor
        // without crossing another solid barrier body; otherwise fall back to nearby
        // clear compass exits (for L-corner margins or physically blocked faces) or a
        // checked bounded drop off the ejection edge (for barrier-covered rooftops).
        let min_progress_sq = if origin_clear { 0.04 } else { 1e-4 };
        let mut progressed = found && d2(unit.position, best) >= min_progress_sq;
        let mut try_compass = false;
        let mut drop_base: Option<Vec3> = None;
        if !origin_clear && !progressed && !h.budget.waiting {
            let mut ejected = origin;
            if self.common.barriers.correct(&mut ejected, radius, mask) {
                if self
                    .common
                    .barriers
                    .segment_crosses_solid_body_outside_start(origin, ejected, mask)
                {
                    try_compass = true;
                } else if let Some(p_tight) = world.foot(ejected) {
                    let mut buffered = origin;
                    let p_buf = self
                        .common
                        .barriers
                        .correct(&mut buffered, radius + 0.25, mask)
                        .then(|| {
                            (!self
                                .common
                                .barriers
                                .segment_crosses_solid_body_outside_start(origin, buffered, mask))
                            .then(|| world.foot(buffered))
                            .flatten()
                        })
                        .flatten();
                    let mut checked_any = false;
                    for p in [p_buf, Some(p_tight)].into_iter().flatten() {
                        if !self.common.barriers.point_clear(p, radius, mask).0
                            || self
                                .common
                                .barriers
                                .segment_crosses_solid_body_outside_start(origin, p, mask)
                        {
                            continue;
                        }
                        checked_any = true;
                        match h
                            .budget
                            .check(|| world.try_crosses(origin, p, unit, 0, ProbeKind::Step))
                        {
                            Some(true) => {
                                best = p;
                                found = true;
                                progressed = d2(unit.position, best) >= min_progress_sq;
                                if progressed {
                                    break;
                                }
                            }
                            Some(false) => {
                                try_compass = true;
                                drop_base = Some(ejected);
                            }
                            None => break,
                        }
                    }
                    if !checked_any {
                        try_compass = true;
                    }
                } else {
                    drop_base = Some(ejected);
                }
            } else {
                try_compass = true;
            }
        }
        if try_compass && !progressed && !h.budget.waiting {
            'compass: for dist in [1.0, 2.0] {
                for [ux, uz] in [
                    [1., 0.],
                    [-1., 0.],
                    [0., 1.],
                    [0., -1.],
                    [0.707, 0.707],
                    [-0.707, 0.707],
                    [0.707, -0.707],
                    [-0.707, -0.707],
                ] {
                    let cand = Vec3::new(origin.x + ux * dist, origin.y, origin.z + uz * dist);
                    if !self.common.barriers.point_clear(cand, radius, mask).0
                        || self
                            .common
                            .barriers
                            .segment_crosses_solid_body_outside_start(origin, cand, mask)
                    {
                        continue;
                    }
                    let Some(p) = world.foot(cand) else {
                        if drop_base.is_none() {
                            if let Some(landing) = world
                                .combat_floor(cand)
                                .or_else(|| world.approach_floor(cand))
                            {
                                let drop_h = origin.y - landing.y;
                                if super::walk::drop_kind(drop_h).is_some()
                                    && self.common.barriers.point_clear(landing, radius, mask).0
                                    && !self
                                        .common
                                        .barriers
                                        .segment_crosses_solid_body_outside_start(
                                            origin, landing, mask,
                                        )
                                {
                                    drop_base = Some(cand);
                                }
                            }
                        }
                        continue;
                    };
                    if !self.common.barriers.point_clear(p, radius, mask).0
                        || self
                            .common
                            .barriers
                            .segment_crosses_solid_body_outside_start(origin, p, mask)
                    {
                        continue;
                    }
                    match h
                        .budget
                        .check(|| world.try_crosses(origin, p, unit, 0, ProbeKind::Step))
                    {
                        None => break 'compass,
                        Some(true) => {
                            best = p;
                            found = true;
                            progressed = d2(unit.position, best) >= min_progress_sq;
                            if progressed {
                                break 'compass;
                            }
                        }
                        Some(false) => {}
                    }
                }
            }
        }
        let above_feet = Vec3::new(origin.x, origin.y + 0.5, origin.z);
        if !progressed && !h.budget.waiting && drop_base.is_some() && world.foot(origin).is_some() {
            if let Some(ej) = drop_base {
                let (dx, dz) = (ej.x - origin.x, ej.z - origin.z);
                let span = v8math::hypot(dx, dz);
                let (ux, uz) = if span > 0.05 {
                    (dx / span, dz / span)
                } else {
                    (0., 0.)
                };
                for extra in [0.0, 0.75, 1.5] {
                    let probe = Vec3::new(ej.x + ux * extra, origin.y, ej.z + uz * extra);
                    if !self.common.barriers.point_clear(probe, radius, mask).0
                        || self
                            .common
                            .barriers
                            .segment_crosses_solid_body_outside_start(origin, probe, mask)
                    {
                        continue;
                    }
                    let Some(landing) = world
                        .combat_floor(probe)
                        .or_else(|| world.approach_floor(probe))
                    else {
                        continue;
                    };
                    let drop_h = origin.y - landing.y;
                    let Some(kind) = super::walk::drop_kind(drop_h) else {
                        continue;
                    };
                    if !self.common.barriers.point_clear(landing, radius, mask).0
                        || self
                            .common
                            .barriers
                            .segment_crosses_solid_body_outside_start(origin, landing, mask)
                    {
                        continue;
                    }
                    let mut tried = h.budget.check(|| {
                        world.try_crosses(origin, landing, unit, kind, ProbeKind::Step)
                    });
                    let mut chosen = kind;
                    if tried == Some(false)
                        && span + extra <= 10.0
                        && world.clear_above(above_feet, unit)
                    {
                        tried = h.budget.check(|| {
                            world.try_crosses(
                                origin,
                                landing,
                                unit,
                                super::walk::PARAPET_DROP,
                                ProbeKind::Step,
                            )
                        });
                        chosen = super::walk::PARAPET_DROP;
                    }
                    match tried {
                        None => break,
                        Some(true) => {
                            best = landing;
                            movement = chosen;
                            found = true;
                            progressed = d2(unit.position, best) >= min_progress_sq;
                            if progressed {
                                break;
                            }
                        }
                        Some(false) => break,
                    }
                }
            }
        }
        // Straight recovery boxed onto a small ledge or awning can step off a bounded drop
        // onto clear lower floor instead of failing every direction in place.
        if !progressed
            && !h.budget.waiting
            && h.mode == GotoMode::Straight
            && requested > 0.5
            && d2(origin, floor) < 0.25
            && world.foot(origin).is_some()
        {
            let (ux, uz) = (
                (target.x - origin.x) / requested,
                (target.z - origin.z) / requested,
            );
            for dist in [1.5, 2.5] {
                let probe = Vec3::new(origin.x + ux * dist, origin.y, origin.z + uz * dist);
                let Some(landing) = world
                    .combat_floor(probe)
                    .or_else(|| world.approach_floor(probe))
                else {
                    continue;
                };
                let drop_h = origin.y - landing.y;
                let Some(kind) = super::walk::drop_kind(drop_h) else {
                    continue;
                };
                if !self.common.barriers.point_clear(landing, radius, mask).0
                    || self
                        .common
                        .barriers
                        .segment_crosses_solid_body_outside_start(origin, landing, mask)
                {
                    continue;
                }
                match h.budget.check(|| {
                    world.try_crosses(origin, landing, unit, kind, ProbeKind::Step)
                }) {
                    None => break,
                    Some(true) => {
                        best = landing;
                        movement = kind;
                        found = true;
                        progressed = d2(unit.position, best) >= 0.04;
                        break;
                    }
                    Some(false) => {}
                }
            }
        }
        // A recovery that has stood still for the game's stall time without a
        // checked step walks on as the game's straight walk does: two metres
        // toward its point, the body's own collision deciding (a wall stops
        // it, an edge drops it), over a floor that lies below the end and
        // through no solid barrier. A soldier that can jet out under open sky
        // does that below instead.
        if !progressed
            && !h.budget.waiting
            && h.mode == GotoMode::Straight
            && requested > 0.5
            && !airborne(unit)
            && self.controls[h.control].stranded_for >= STALL_SECONDS
            && !(has_jetpack(unit) && world.clear_above(above_feet, unit))
        {
            let end = Vec3::new(
                origin.x + (target.x - origin.x) / requested * 2.,
                origin.y,
                origin.z + (target.z - origin.z) / requested * 2.,
            );
            if let Some(below) = world
                .approach_floor(end)
                .filter(|p| p.y <= origin.y + 0.5 && origin.y - p.y <= super::walk::DEEP_DROP_LIMIT)
            {
                let step = Vec3::new(end.x, below.y, end.z);
                if !self
                    .common
                    .barriers
                    .segment_crosses_solid_body_outside_start(origin, step, mask)
                {
                    best = step;
                    movement = if origin.y - below.y >= 0.75 {
                        super::walk::DROP
                    } else {
                        0
                    };
                    found = true;
                    progressed = d2(unit.position, best) >= 0.04;
                }
            }
        }
        let waiting = std::mem::take(&mut h.budget.waiting);
        if (!found || !progressed) && (requested > 0.5 || !origin_clear) {
            if waiting {
                // Stand until a later tick has checked the step.
                self.control_stop(h.control, Some(unit));
                self.controls[h.control].request.valid = false;
                return Placed::Waiting;
            }
            let jet_recover = h.mode == GotoMode::Straight
                && self.controls[h.control].stranded_for >= 3.0
                && has_jetpack(unit)
                && world.clear_above(above_feet, unit);
            self.fail_walk(h, unit);
            if jet_recover {
                self.controls[h.control].jet.pending = true;
                self.controls[h.control].jet.vector = Vec3::default();
                self.controls[h.control].stranded_for = 0.0;
            }
            return Placed::Failed;
        }
        let parapet_stall = if movement == super::walk::PARAPET_DROP {
            self.swbf2.as_ref().unwrap().legs[h.control]
                .filter(|l| {
                    l.local
                        && l.movement == super::walk::PARAPET_DROP
                        && d2(l.end, best) < 4.0
                })
                .map_or(0, |l| l.parapet_stall)
        } else {
            0
        };
        if movement == ARC_JUMP {
            self.swbf2.as_mut().unwrap().set_jump_pending(h.control, true);
        }
        self.swbf2.as_mut().unwrap().legs[h.control] = Some(super::WalkLeg {
            start: origin,
            end: best,
            destination: best,
            radius,
            mask,
            arc: None,
            movement,
            parapet_stall,
            check_body: false,
            local: true,
            final_leg: true,
        });
        h.destination = best;
        let mut c = Swbf2Cursor::new(None, best, best, self.swbf2.as_ref().unwrap().revision);
        c.count = 0;
        c.placed = true;
        h.cursor = Some(c);
        let path = self.common.store.segment(origin, best);
        self.common
            .control_install(h.control, path, true, false, unit);
        Placed::Leg
    }

    fn fail_walk(&mut self, h: &mut Swbf2GroundGoto, unit: &NavUnit) {
        self.drop_order(h.control);
        h.waiting = None;
        h.deferred = false;
        h.failed = true;
        h.arrived = false;
        h.cursor = None;
        self.control_stop(h.control, Some(unit));
        self.control_strand(h.control, true, Some(unit));
        self.controls[h.control].request.valid = false;
    }

    fn advance_walk(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &mut NavUnit,
        world: &mut dyn Swbf2GotoWorld,
        turn: bool,
    ) {
        let radius = traversal_radius(unit);
        let mask = traversal_mask(unit);
        let cached = world.observe_navigation(self);
        let state = self.swbf2.as_mut().unwrap();
        let before = h.waiting.take();
        let feet = resting_place(before, unit);
        // Only a soldier at rest can wait for an answer where it asked. One
        // that moves asks again now and then until it rests.
        let v = unit.velocity;
        let rests = before.is_some_and(|w| w.from == feet) || v.x * v.x + v.z * v.z <= 0.01;
        if !turn && !rests {
            let now = state.query_tick.unwrap_or(0);
            if before.is_some_and(|w| w.kind == ProbeKind::Turn && turn_waits(now, w.since)) {
                h.waiting = before;
                return;
            }
        }
        // An early corner may still be obstructed from the actual feet. Commit the
        // cursor only when the next segment can replace the current executor.
        let mut c = h.cursor.as_ref().unwrap().clone();
        let in_passage = c.in_passage;
        if c.count == 0 {
            if c.at_goal {
                if (unit.position.y - h.destination.y).abs() < 0.55
                    && h2(
                        unit.position.x,
                        unit.position.z,
                        h.destination.x,
                        h.destination.z,
                    ) <= 1.
                {
                    let arrival = if state.take_execution_check() {
                        world.try_crosses(feet, h.destination, unit, 0, ProbeKind::Leg)
                    } else {
                        None
                    };
                    match arrival {
                        None => {
                            self.wait_leg(h, before, unit, feet, ProbeKind::Leg);
                            return;
                        }
                        Some(false) => {
                            self.fail_walk(h, unit);
                            return;
                        }
                        Some(true) => {}
                    }
                    state.legs[h.control] = None;
                    if let Some((path, index)) = state.orders[h.control] {
                        if let Some(next) = self.common.store.paths[path].nodes.get(index) {
                            let next = Vec3::new(next.x, next.y, next.z);
                            state.orders[h.control] = Some((path, index + 1));
                            self.begin_walk(h, unit, world, next);
                            return;
                        }
                        self.drop_order(h.control);
                    }
                    h.cursor = None;
                    h.arrived = true;
                } else {
                    self.fail_walk(h, unit);
                }
                return;
            }
            if c.here == c.goal {
                c.at_goal = true;
                c.arc = None;
                c.leg(c.exit_bend, h.destination, 0);
            } else {
                let next_leg = if let Some(edge) = c.first_edge.take() {
                    state.floors.corridor(c.here, edge)
                } else {
                    h.budget.share_rows(&mut state.row_entries_left, |budget| {
                        state.floors.next(
                            state.graph.as_ref().unwrap(),
                            &mut self.common.barriers,
                            c.here,
                            c.goal,
                            radius,
                            mask,
                            has_jetpack(unit),
                            budget,
                            &mut |a, b, m| world.try_crosses(a, b, unit, m, ProbeKind::Plan),
                        )
                    })
                };
                let Some((next, bend, flags, arc, after_jump)) = next_leg else {
                    let waiting = std::mem::take(&mut h.budget.waiting);
                    if turn {
                        return;
                    }
                    if waiting {
                        self.wait_leg(h, before, unit, feet, ProbeKind::Plan);
                    } else if h.budget.spent() {
                        self.refuse_exhausted(h, unit, world);
                    } else {
                        self.fail_walk(h, unit);
                    }
                    return;
                };
                c.here = next;
                c.arc = Some(arc);
                c.leg(super::walk::Waypoints::NONE, state.floors.point(next), flags);
                c.after_jump = after_jump;
                c.in_passage = flags == 0 && bend.points().len() > 1;
                for &point in bend.points().iter().rev() {
                    c.pending[c.count] = point;
                    c.count += 1;
                }
            }
        }
        c.count -= 1;
        let mut point = c.pending[c.count];
        let movement = if c.count == usize::from(c.after_jump) || c.flags == super::walk::DROP {
            c.flags
        } else {
            0
        };
        if turn && in_passage {
            // A door passage is certified through its surveyed turns. Finish the
            // current approach when momentum still carries the body away from
            // the next leg. A stopped soldier can take the clear leg immediately.
            let forward = (point.x - unit.position.x) * unit.velocity.x
                + (point.z - unit.position.z) * unit.velocity.z;
            if forward < 0. {
                return;
            }
        }
        let special = movement != 0;
        if turn && special {
            // The airborne segment is certified from its launch point.
            return;
        }
        let leg_kind = if turn || !rests {
            ProbeKind::Turn
        } else {
            ProbeKind::Leg
        };
        let door_center = (!special
            && !turn
            && super::walk::clear(&mut self.common.barriers, feet, point, radius, mask))
        .then(|| {
            state
                .floors
                .closed_door_center(feet, point, &state.clearance.flags)
        })
        .flatten();
        let mut clear = |world: &mut dyn Swbf2GotoWorld, a, b, alternate| {
            if !super::walk::clear(&mut self.common.barriers, a, b, radius, mask) {
                return Some(false);
            }
            if special {
                return Some(true);
            }
            let mut test = |a, b| {
                if alternate {
                    return h
                        .budget
                        .check(|| world.try_crosses(a, b, unit, 0, ProbeKind::Plan));
                }
                state.execution_checks_left = state.execution_checks_left.checked_sub(1)?;
                world.try_crosses(a, b, unit, 0, leg_kind)
            };
            if cached {
                state.clearance.try_check(a, b, test)
            } else {
                test(a, b)
            }
        };
        let Some(chosen_clear) = clear(world, feet, point, false) else {
            if !turn {
                self.wait_leg(h, before, unit, feet, leg_kind);
            }
            return;
        };
        if !chosen_clear {
            if turn {
                return;
            }
            let mut stepped_to_door = false;
            if let Some(door) = door_center {
                let dist = v8math::sqrt(d2(feet, door));
                if dist > 3. {
                    let t = (dist - 2.) / dist;
                    let cand = Vec3::new(
                        feet.x + (door.x - feet.x) * t,
                        feet.y + (door.y - feet.y) * t,
                        feet.z + (door.z - feet.z) * t,
                    );
                    if let Some(mid) = world.foot(cand) {
                        match clear(world, feet, mid, false) {
                            None => {
                                self.wait_leg(h, before, unit, feet, leg_kind);
                                return;
                            }
                            Some(true) => {
                                c.count += 1;
                                point = mid;
                                stepped_to_door = true;
                            }
                            Some(false) => {}
                        }
                    }
                }
                if !stepped_to_door {
                    // Stand at the closed proximity shutter until it clears or times out.
                    self.wait_leg(h, before, unit, feet, ProbeKind::Leg);
                    return;
                }
            }
            if !stepped_to_door {
                // The mover may round a corner on a slightly different line from
                // the centreline. Validate both halves before offering a new bend.
                let bypass = (!special && !c.correcting)
                    .then(|| {
                        super::walk::handoff_bends(feet, point).find_map(|candidate| {
                            let mid = world.foot(candidate)?;
                            (clear(world, feet, mid, true)? && clear(world, mid, point, true)?)
                                .then_some(mid)
                        })
                    })
                    .flatten();
                if std::mem::take(&mut h.budget.waiting) && bypass.is_none() {
                    self.wait_leg(h, before, unit, feet, ProbeKind::Plan);
                    return;
                }
                let Some(mid) = bypass else {
                    self.replan_walk(h, unit, world);
                    return;
                };
                c.count += 1;
                c.correcting = true;
                c.recovery_attempts = c.recovery_attempts.saturating_add(1);
                point = mid;
            }
        } else {
            c.correcting = false;
        }
        if movement & ARC_JET_JUMP != 0 {
            let mut dir = Vec3::new(
                point.x - unit.position.x,
                point.y - unit.position.y,
                point.z - unit.position.z,
            );
            normalize(&mut dir);
            self.common.controls[h.control].jet.pending = true;
            self.common.controls[h.control].jet.vector = dir;
        } else if movement & ARC_JUMP != 0 {
            state.set_jump_pending(h.control, true);
        }
        state.legs[h.control] = Some(super::WalkLeg {
            start: unit.position,
            end: point,
            destination: h.destination,
            radius,
            mask,
            arc: c.arc,
            movement,
            parapet_stall: 0,
            check_body: false,
            local: false,
            final_leg: c.at_goal && c.count == 0,
        });
        c.waypoint = point;
        c.best = f64::INFINITY;
        c.idle = 0.;
        c.placed = true;
        h.cursor = Some(c);
        let path = self.common.store.make();
        if !self.common.store.append(
            path,
            PathNode {
                x: point.x,
                y: point.y,
                z: point.z,
                flag: 0,
            },
        ) {
            self.common.store.release(path);
            self.fail_walk(h, unit);
            return;
        }
        self.common
            .control_install(h.control, path, true, false, unit);
    }

    /// A leg that could not be checked in this tick. A soldier that still
    /// moves along its last one does not count the wait.
    fn wait_leg(
        &mut self,
        h: &mut Swbf2GroundGoto,
        before: Option<Waiting>,
        unit: &NavUnit,
        feet: Vec3,
        kind: ProbeKind,
    ) {
        let before = before.filter(|_| kind != ProbeKind::Turn);
        if !self.wait_walk(h, before, feet, kind) {
            self.fail_walk(h, unit);
        } else if self.common.controls[h.control].executor.is_none() {
            self.common.controls[h.control].request.valid = false;
        }
    }

    fn check_ground_leg(
        &mut self,
        control: usize,
        unit: &NavUnit,
        world: &mut dyn Swbf2GotoWorld,
    ) -> bool {
        let state = self.swbf2.as_mut().unwrap();
        let Some(leg) = state.legs.get_mut(control).and_then(Option::as_mut) else {
            return true;
        };
        if !leg.check_body {
            leg.start = unit.position;
        } else {
            // The check belongs to where the soldier stopped. One that is
            // carried off from there has lost the leg.
            let lost = leg.movement & ARC_JUMP != 0 || d2(leg.start, unit.position) > 1.;
            let clear = match state.execution_checks_left.checked_sub(1) {
                None => None,
                Some(left) => {
                    state.execution_checks_left = left;
                    if lost {
                        Some(false)
                    } else {
                        world.try_crosses(leg.start, leg.end, unit, leg.movement, ProbeKind::Leg)
                    }
                }
            };
            let Some(clear) = clear else {
                // Stand on the changed leg until it has been checked.
                self.common.controls[control].request.valid = false;
                return false;
            };
            leg.check_body = false;
            if !clear {
                state.legs[control] = None;
                state.set_jump_pending(control, false);
                self.common.control_stop(control, Some(unit));
                self.common.controls[control].request.valid = false;
                self.common.controls[control].jet.pending = false;
            }
        }
        true
    }

    fn replan_walk(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &mut NavUnit,
        world: &mut dyn Swbf2GotoWorld,
    ) {
        if h.mode == GotoMode::Route && h.replans < 2 && !h.budget.spent() {
            h.replans += 1;
            self.begin_walk(h, unit, world, h.requested);
        } else {
            self.fail_walk(h, unit);
        }
    }

    pub fn swbf2_ground_tick(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &mut NavUnit,
        world: &mut dyn Swbf2GotoWorld,
        dt: f64,
    ) {
        world.observe_navigation(self);
        if airborne(unit) && self.controls[h.control].executor.is_some() {
            return;
        }
        if h.deferred {
            let (point, mode) = (h.requested, h.mode);
            self.start_order(h, unit, world, point, mode);
            return;
        }
        if let Some(target) = h.target {
            if world.time() - h.last_refresh >= 0.5 {
                let Some(actor) = world.resolve(target) else {
                    self.swbf2_ground_stop(h, unit, world);
                    return;
                };
                h.last_refresh = world.time();
                let point = if h.remembered {
                    world.remembered(actor.id).unwrap_or(actor.position)
                } else {
                    actor.position
                };
                let tolerance = v8math::max(h.follow_distance, 1.);
                if d2(point, h.destination) > tolerance * tolerance {
                    self.begin_walk(h, unit, world, point);
                }
            }
        }
        if !self.check_ground_leg(h.control, unit, world) {
            return;
        }
        if h.waiting.is_some() {
            self.swbf2_ground_resume(h, unit, world);
            return;
        }
        let Some(cursor) = &mut h.cursor else { return };
        let state = self.swbf2.as_ref().unwrap();
        if cursor.revision != state.revision {
            if state.legs[h.control].is_none() {
                self.begin_walk(h, unit, world, h.requested);
                return;
            }
            cursor.first_edge = None;
            cursor.revision = state.revision;
        }
        if self.swbf2.as_ref().unwrap().held[h.control] {
            return;
        }
        if self.controls[h.control].executor.is_some()
            && state.legs[h.control]
                .is_some_and(|leg| !leg.final_leg && !leg.local && leg.movement == 0)
            && (unit.position.y - cursor.waypoint.y).abs() < 0.55
            && h2(
                unit.position.x,
                unit.position.z,
                cursor.waypoint.x,
                cursor.waypoint.z,
            ) < 1.
        {
            self.advance_walk(h, unit, world, true);
            return;
        }
        if d2(h.progress_origin, unit.position) > 0.25 {
            h.progress_origin = unit.position;
            h.stationary = 0.;
        } else if h.mode == GotoMode::Route {
            h.stationary += dt;
            if h.stationary >= 5. {
                self.fail_walk(h, unit);
                return;
            }
        }
        if self.controls[h.control].executor.is_none() || !cursor.placed {
            self.advance_walk(h, unit, world, false);
        } else {
            let remaining = v8math::sqrt(d2(unit.position, cursor.waypoint));
            if remaining + 0.15 < cursor.best {
                cursor.best = remaining;
                cursor.idle = 0.;
            } else {
                cursor.idle += dt;
                if cursor.idle >= 0.5
                    && remaining < 0.18
                    && self.swbf2.as_mut().unwrap().take_execution_check()
                    && world.try_crosses(
                        unit.position,
                        cursor.waypoint,
                        unit,
                        0,
                        ProbeKind::Turn,
                    ) == Some(true)
                {
                    self.common.control_stop(h.control, Some(unit));
                    self.advance_walk(h, unit, world, false);
                } else if cursor.idle >= 1.8 && h.mode == GotoMode::Route {
                    let to = cursor.waypoint;
                    let (radius, mask) = (traversal_radius(unit), traversal_mask(unit));
                    if h.budget.spent() {
                        self.fail_walk(h, unit);
                        return;
                    }
                    let detour = (cursor.recovery_attempts < 2)
                        .then(|| {
                            super::walk::handoff_bends(unit.position, to).find_map(|p| {
                                let mid = world.foot(p)?;
                                (super::walk::clear(
                                    &mut self.common.barriers,
                                    unit.position,
                                    mid,
                                    radius,
                                    mask,
                                ) && super::walk::clear(
                                    &mut self.common.barriers,
                                    mid,
                                    to,
                                    radius,
                                    mask,
                                ) && h.budget.check(|| {
                                    let from = unit.position;
                                    world.try_crosses(from, mid, unit, 0, ProbeKind::Plan)
                                })? && h.budget.check(|| {
                                    world.try_crosses(mid, to, unit, 0, ProbeKind::Plan)
                                })?)
                                .then_some(mid)
                            })
                        })
                        .flatten();
                    if std::mem::take(&mut h.budget.waiting) && detour.is_none() {
                        return;
                    }
                    if let Some(mid) = detour.filter(|_| cursor.count + 2 <= cursor.pending.len()) {
                        cursor.pending[cursor.count] = to;
                        cursor.pending[cursor.count + 1] = mid;
                        cursor.count += 2;
                        cursor.recovery_attempts += 1;
                        self.common.control_stop(h.control, Some(unit));
                        self.advance_walk(h, unit, world, false);
                    } else if cursor.idle >= 2.5 {
                        self.replan_walk(h, unit, world);
                    }
                } else if cursor.idle >= 2.5 {
                    self.fail_walk(h, unit);
                }
            }
        }
    }

    pub fn swbf2_ground_stop(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &mut NavUnit,
        _: &mut dyn Swbf2GotoWorld,
    ) {
        self.drop_order(h.control);
        h.deferred = false;
        h.target = None;
        h.cursor = None;
        h.waiting = None;
        h.failed = false;
        h.arrived = true;
        self.control_stop(h.control, Some(unit));
        self.control_strand(h.control, false, Some(unit));
    }
    pub fn swbf2_ground_arrived(&self, h: &Swbf2GroundGoto) -> bool {
        h.arrived && !h.failed
    }
    pub fn swbf2_ground_follow(
        &mut self,
        h: &mut Swbf2GroundGoto,
        unit: &mut NavUnit,
        world: &mut dyn Swbf2GotoWorld,
        target: Option<UnitRef>,
        distance: f64,
        remembered: bool,
    ) {
        let Some(actor) = target.and_then(|r| world.resolve(r)) else {
            self.swbf2_ground_stop(h, unit, world);
            return;
        };
        let point = if remembered {
            world.remembered(actor.id).unwrap_or(actor.position)
        } else {
            actor.position
        };
        self.clear_order_path(h.control);
        h.mode = GotoMode::Route;
        h.combat = false;
        h.replans = 0;
        h.budget = super::walk::SearchBudget::new();
        h.progress_origin = unit.position;
        h.stationary = 0.;
        h.target = target;
        h.remembered = remembered;
        h.follow_distance = distance;
        h.last_refresh = world.time();
        self.begin_walk(h, unit, world, point);
    }
    pub fn swbf2_ground_hold_path(&mut self, h: &Swbf2GroundGoto, on: bool) {
        self.swbf2.as_mut().unwrap().held[h.control] = on;
        if let Some(e) = &mut self.controls[h.control].executor {
            e.hold = on;
        }
    }
    pub fn swbf2_ground_stance(
        &mut self,
        h: &mut Swbf2GroundGoto,
        _: &mut NavUnit,
        _: &mut dyn Swbf2GotoWorld,
        v: f64,
    ) {
        self.controls[h.control].stance = v;
    }
    pub fn swbf2_ground_jet(
        &mut self,
        h: &mut Swbf2GroundGoto,
        _: &mut NavUnit,
        _: &mut dyn Swbf2GotoWorld,
        v: Vec3,
    ) {
        self.controls[h.control].jet.pending = true;
        self.controls[h.control].jet.vector = v;
    }
    pub fn swbf2_ground_aim_at(
        &mut self,
        h: &mut Swbf2GroundGoto,
        _: &mut NavUnit,
        _: &mut dyn Swbf2GotoWorld,
        p: Vec3,
    ) {
        h.aim_point = p;
        h.aim_target = None;
    }
    pub fn swbf2_ground_aim_at_target(
        &mut self,
        h: &mut Swbf2GroundGoto,
        _: &mut NavUnit,
        _: &mut dyn Swbf2GotoWorld,
        r: Option<UnitRef>,
    ) {
        h.aim_target = r;
    }
    pub fn swbf2_ground_aim_along_path(
        &mut self,
        h: &mut Swbf2GroundGoto,
        _: &mut NavUnit,
        _: &mut dyn Swbf2GotoWorld,
    ) {
        h.aim_target = None;
    }
}

fn route_distance_2004(
    graph: &super::super::graph::NavGraph,
    from: Vec3,
    to: Vec3,
    mask: u32,
) -> RouteQuery {
    let table = graph.table(mask);
    if table < 0 || graph.hubs.is_empty() {
        return RouteQuery::UNREACHABLE;
    }
    let attach_hub = |p: Vec3| -> Option<usize> {
        let att = graph.attach(p, mask);
        if let Some(h) = att.hub {
            return Some(h);
        }
        let arc_idx = att.arc.or_else(|| graph.nearest_arc(p, mask))?;
        let arc = &graph.arcs[arc_idx];
        let da = h2(p.x, p.z, graph.hubs[arc.a].x, graph.hubs[arc.a].z);
        let db = h2(p.x, p.z, graph.hubs[arc.b].x, graph.hubs[arc.b].z);
        Some(if da <= db { arc.a } else { arc.b })
    };
    let (Some(s_hub), Some(d_hub)) = (attach_hub(from), attach_hub(to)) else {
        return RouteQuery::UNREACHABLE;
    };
    let approach = v8math::sqrt(h2(from.x, from.z, graph.hubs[s_hub].x, graph.hubs[s_hub].z));
    let egress = v8math::sqrt(h2(to.x, to.z, graph.hubs[d_hub].x, graph.hubs[d_hub].z));
    if s_hub == d_hub {
        return RouteQuery::found(approach + egress, 0);
    }
    // A routing slot is a byte, with one value reserved. At most two arrival
    // directions per usable slot can enter the queue, even on a cyclic map.
    let n = graph.hubs.len();
    let mut queue = [(0usize, 0.0, 0u32); 511];
    let mut seen = [[false; 2]; 255];
    let (mut read, mut end) = (0, 1);
    queue[0] = (s_hub, approach, 0);
    while read < end {
        let (hub, distance, hops) = queue[read];
        read += 1;
        let h = &graph.hubs[hub];
        let word = graph.words[h.base + table as usize * n + d_hub];
        for byte in word.to_le_bytes() {
            if byte == 0 {
                break;
            }
            let slot = usize::from(h.slots[usize::from(byte & 7)]);
            let Some(arc) = graph.arcs.get(slot) else {
                continue;
            };
            let direction = usize::from(arc.b == hub);
            if slot == 255
                || byte >> 3 == 0
                || arc.degenerate
                || arc.mask & mask == 0
                || (arc.a != hub && arc.b != hub)
                || seen[slot][direction]
            {
                continue;
            }
            seen[slot][direction] = true;
            let next = graph.other_end(slot, hub);
            let cost = distance + v8math::sqrt(d2(h.pos(), graph.hubs[next].pos()));
            if next == d_hub {
                return RouteQuery::found(cost + egress, hops + 1);
            }
            queue[end] = (next, cost, hops + 1);
            end += 1;
        }
    }
    RouteQuery::UNREACHABLE
}

#[cfg(test)]
mod bound_tests {
    use super::*;

    #[test]
    fn a_turn_waits_eight_ticks_and_the_sum_cannot_wrap_at_the_bound() {
        assert!(turn_waits(100, 100) && turn_waits(107, 100));
        assert!(!turn_waits(108, 100));
        assert!(turn_waits(5, 100), "asked in the future: still waiting");
        assert!(turn_waits(u32::MAX, u32::MAX - 7));
        assert!(!turn_waits(u32::MAX, u32::MAX - 8));
        assert!(turn_waits(u32::MAX, u32::MAX));
    }
}
