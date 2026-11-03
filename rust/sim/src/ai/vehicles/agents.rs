//! The vehicle agents (src/ai/classic/vehicles/agents.js): boarding
//! (controller state 5), leaving (state 6) and the flyer pilot's objective
//! agent (state 7), as the 2004 game has them.
//!
//! They run on the AI control core's agent machine (src/ai/classic
//! agents.js Agent, its Fsm): that package owns an agent's state, pending
//! state, interrupts, helper and aim, and calls these for an agent of one
//! of these kinds: its construction (`*Agent::new`, after the base's), its
//! state entry and exit (`on_enter`, `on_exit`) and its phase-0 behaviour
//! (`behave`), and its policies (`accepts_*`). What they ask of the base is
//! [`AgentHost`]; [`Fsm`] is the machine's state and pending state.

use three::Vector3;
use v8math::{cos, sin};

use super::controllers::DONE;
use super::service::VehicleAI;
use super::world::{kind, HintView, Obj, Ref, World};

/// The agent's machine as these read and write it (`this.state`,
/// `this.pending`: requestState only sets the pending state).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fsm {
    pub state: f64,
    pub pending: f64,
}

impl Fsm {
    pub fn request_state(&mut self, state: f64) {
        self.pending = state;
    }
}

/// An agent of the base as these see it: itself, its controller and the
/// unit it was made for (`this.unit`, the controller's unit then).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AgentCx {
    pub agent: Obj,
    pub controller: Obj,
    pub unit: Option<Obj>,
}

/// What the vehicle agents ask of the agent base (agents.js Agent) and the
/// AI's navigation beyond [`World`].
pub trait AgentHost: World {
    /// `this.interrupts.tick()`: an interrupt took the agent over.
    fn interrupts_tick(&mut self, agent: Obj) -> bool;
    /// `this.updateCombatTarget()`.
    fn update_combat_target(&mut self, agent: Obj);
    /// `this.aim(stance)`.
    fn aim(&mut self, agent: Obj, stance: f64);
    /// `this.tickHelper(dt)`.
    fn tick_helper(&mut self, agent: Obj, dt: f64);
    /// `this.canEngage(target)`.
    fn can_engage(&mut self, agent: Obj, target: Obj) -> bool;
    /// The agent's helper: whether it is there, `arrived`, `failed`.
    fn has_helper(&self, agent: Obj) -> bool;
    fn helper_arrived(&mut self, agent: Obj) -> bool;
    fn helper_failed(&mut self, agent: Obj) -> bool;
    /// The battle's posts (`world.posts.posts`), in order.
    fn posts(&self) -> Vec<Obj>;
    /// `this.ai.nav?.queryHints?.({ mask: 32, center, halfExtent: 80, cap:
    /// 1, requester, now })` (None: no queryHints).
    fn query_hints(&mut self, center: &Vector3, requester: Obj, now: f64) -> Option<Vec<HintView>>;
}

// ---------------------------------------------------------------------------
// State 5: walk to the vehicle and get in.

/// UnitBoardAgent's own field: the candidate's reference (`goal.refA`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoardAgent {
    pub vehicle: Option<Ref>,
}

/// resolveRef(ref) (controller.js:347).
fn resolve<W: World + ?Sized>(w: &W, r: Option<Ref>) -> Option<Obj> {
    let r = r?;
    let u = w.unit(r.obj);
    if u.generation != r.generation || !u.alive {
        return None;
    }
    Some(r.obj)
}

impl BoardAgent {
    /// constructor (agents.js:36), after the base's: the candidate, and
    /// state 2 requested.
    pub fn new(fsm: &mut Fsm, goal_ref: Option<Ref>) -> BoardAgent {
        fsm.request_state(2.0);
        BoardAgent { vehicle: goal_ref }
    }

    fn candidate<W: World + ?Sized>(&self, w: &W) -> Option<Obj> {
        resolve(w, self.vehicle)
    }

    /// onEnter() (agents.js:46).
    pub fn on_enter<W: AgentHost + ?Sized>(&self, w: &mut W, ai: &mut VehicleAI, fsm: &mut Fsm, cx: &AgentCx) {
        if fsm.state == 2.0 {
            let vehicle = self.candidate(w);
            if vehicle.is_none() || !ai.reserve_seat(w, Some(cx.controller), vehicle) {
                fsm.request_state(DONE);
                return;
            }
            if w.has_helper(cx.agent) {
                let at = w.unit(vehicle.unwrap()).position;
                w.helper_goto(cx.agent, &at, false);
            }
        } else if fsm.state == 3.0 && w.has_helper(cx.agent) {
            w.helper_stop(cx.agent);
        }
    }

    /// behave(dt) (agents.js:60).
    pub fn behave<W: AgentHost + ?Sized>(&self, w: &mut W, ai: &mut VehicleAI, fsm: &mut Fsm, cx: &AgentCx, dt: f64) {
        if fsm.state == DONE {
            return;
        }
        if w.interrupts_tick(cx.agent) {
            return;
        }
        w.update_combat_target(cx.agent);
        let vehicle = self.candidate(w);
        let unit = w.controller(cx.controller).unit;
        let (Some(vehicle), Some(unit)) = (vehicle, unit) else {
            fsm.request_state(DONE);
            return;
        };
        if !ai.can_enter(w, Some(cx.controller), vehicle) || !ai.hold_reservation(w, Some(cx.controller), vehicle) {
            fsm.request_state(DONE);
            return;
        }
        let v = w.unit(vehicle).velocity;
        if !(v.x * v.x + v.y * v.y + v.z * v.z < 4.0) {
            fsm.request_state(DONE);
            return;
        }
        if fsm.state == 2.0 {
            let a = w.sphere(unit);
            let b = w.sphere(vehicle);
            let reach = a.radius + b.radius + 0.5;
            if a.center.distance_to_squared(&b.center) < reach * reach {
                fsm.request_state(3.0);
            } else if w.has_helper(cx.agent) && w.helper_arrived(cx.agent) && !w.helper_failed(cx.agent) {
                // The walk is over and the seat is still out of reach: give
                // it up, and the next invitation for a while.
                let t = w.now() + 15.0;
                w.set_reboard_at(cx.controller, t);
                fsm.request_state(DONE);
                return;
            }
            w.aim(cx.agent, 0.0);
        } else if fsm.state == 3.0 {
            ai.board_now(w, Some(cx.controller), vehicle);
        }
        w.tick_helper(cx.agent, dt);
    }

    /// contested() (agents.js:98): alerts, hazards and commands are refused
    /// while the vehicle is uncontested.
    pub fn contested<W: AgentHost + ?Sized>(&self, w: &mut W, ai: &VehicleAI, cx: &AgentCx) -> bool {
        let Some(vehicle) = self.candidate(w) else { return true };
        !ai.board_path_clear(w, Some(cx.controller), vehicle)
    }

    pub fn accepts_hazard<W: AgentHost + ?Sized>(&self, w: &mut W, ai: &VehicleAI, cx: &AgentCx) -> bool {
        self.contested(w, ai, cx)
    }

    pub fn accepts_alert<W: AgentHost + ?Sized>(&self, w: &mut W, ai: &VehicleAI, cx: &AgentCx) -> bool {
        self.contested(w, ai, cx)
    }

    pub fn accepts_target<W: AgentHost + ?Sized>(&self, w: &mut W, ai: &VehicleAI, cx: &AgentCx, target: Obj) -> bool {
        self.contested(w, ai, cx) && w.can_engage(cx.agent, target)
    }

    pub fn accepts_acoustic<W: AgentHost + ?Sized>(&self, w: &mut W, ai: &VehicleAI, cx: &AgentCx, source: Obj) -> bool {
        self.contested(w, ai, cx) && w.can_engage(cx.agent, source)
    }

    pub fn accepts_board(&self) -> bool {
        false
    }

    pub fn accepts_secondary_wait(&self) -> bool {
        false
    }

    pub fn saves_on_interrupt(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// State 6: get out.

/// UnitUnBoardAgent (agents.js:140): no fields of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UnBoardAgent;

impl UnBoardAgent {
    /// constructor: state 0, state 1 requested.
    pub fn new(fsm: &mut Fsm) -> UnBoardAgent {
        fsm.state = 0.0;
        fsm.request_state(1.0);
        UnBoardAgent
    }

    /// onEnter() (agents.js:147): the exit asked for.
    pub fn on_enter<W: AgentHost + ?Sized>(&self, w: &mut W, fsm: &Fsm, cx: &AgentCx) {
        if fsm.state == 1.0 {
            w.controller_request_exit(cx.controller);
        }
    }

    /// behave(dt) (agents.js:151).
    pub fn behave<W: AgentHost + ?Sized>(&self, w: &mut W, ai: &VehicleAI, fsm: &mut Fsm, cx: &AgentCx, dt: f64) {
        if fsm.state == DONE {
            return;
        }
        if w.interrupts_tick(cx.agent) {
            return;
        }
        w.update_combat_target(cx.agent);
        if !ai.boarded(w, Some(cx.controller)) {
            fsm.request_state(DONE);
        }
        w.tick_helper(cx.agent, dt);
    }

    pub fn accepts_hazard(&self) -> bool {
        false
    }
    pub fn accepts_alert(&self) -> bool {
        false
    }
    pub fn accepts_acoustic(&self) -> bool {
        false
    }
    pub fn accepts_board(&self) -> bool {
        false
    }
    pub fn accepts_secondary_wait(&self) -> bool {
        false
    }
    pub fn saves_on_interrupt(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// State 7: the flyer pilot's objective.

/// UnitFlyAgent's own fields (agents.js:209).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlyAgent {
    pub hint: Option<HintView>,
    pub search_at: f64,
    pub flag: bool,
    pub deadline: f64,
    pub post: Option<Obj>,
}

const DOWN_UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);

impl FlyAgent {
    /// constructor (agents.js:210), after the base's: state 0, state 1
    /// requested.
    pub fn new(fsm: &mut Fsm) -> FlyAgent {
        fsm.state = 0.0;
        fsm.request_state(1.0);
        FlyAgent { hint: None, search_at: f64::NEG_INFINITY, flag: false, deadline: 0.0, post: None }
    }

    fn flyer<W: World + ?Sized>(w: &W, cx: &AgentCx) -> Option<super::world::HullView> {
        cx.unit.and_then(|u| w.unit(u).vehicle).map(|h| w.hull(h))
    }

    /// VehicleType Transport.
    fn transport<W: World + ?Sized>(w: &W, cx: &AgentCx) -> bool {
        let t = cx.unit.and_then(|u| w.unit(u).class).and_then(|c| c.vehicle_type);
        t.as_deref().unwrap_or("").to_lowercase() == "transport"
    }

    fn airborne<W: World + ?Sized>(w: &W, cx: &AgentCx) -> bool {
        Self::flyer(w, cx).is_some_and(|v| !v.grounded)
    }

    /// On its pad: landed, or lifting off it (a lift a roof holds).
    fn on_pad<W: World + ?Sized>(w: &W, cx: &AgentCx) -> bool {
        Self::flyer(w, cx).is_some_and(|v| v.grounded || (v.flight_state.as_deref() == Some("TakingOff") && v.gear >= 1.0))
    }

    /// onEnter() (agents.js:242).
    pub fn on_enter<W: AgentHost + ?Sized>(&mut self, w: &mut W, ai: &mut VehicleAI, fsm: &mut Fsm, cx: &AgentCx) {
        let helper = w.has_helper(cx.agent);
        let now = w.now();
        match fsm.state {
            s if s == 1.0 => {
                self.flag = true;
                if Self::airborne(w, cx) {
                    fsm.request_state(4.0);
                } else if helper {
                    w.helper_stop(cx.agent);
                }
            }
            s if s == 2.0 => {
                let u = w.unit(cx.unit.expect("the flyer agent's unit"));
                let yaw = u.body_yaw;
                let p = u.position;
                if helper {
                    let path = [[p.x, p.y, p.z], [p.x + 200.0 * sin(yaw), p.y + 1.0, p.z + 200.0 * cos(yaw)]];
                    w.helper_follow_path(cx.agent, &path, true, true);
                }
            }
            s if s == 3.0 => {
                self.deadline = if Self::transport(w, cx) && self.flag { now + 10.0 } else { 4.0 };
                if helper {
                    w.helper_stop(cx.agent);
                }
            }
            s if s == 4.0 => self.pick_post(w, cx),
            s if s == 5.0 => self.approach(w, ai, fsm, cx),
            s if s == 6.0 => self.landed(w, ai, fsm, cx),
            _ => {}
        }
    }

    /// behave(dt) (agents.js:275).
    pub fn behave<W: AgentHost + ?Sized>(&mut self, w: &mut W, ai: &mut VehicleAI, fsm: &mut Fsm, cx: &AgentCx, dt: f64) {
        if fsm.state == DONE {
            return;
        }
        if w.controller(cx.controller).executor_status == 2.0 {
            // Never off the pad: the crew get out and go on foot.
            if Self::on_pad(w, cx) {
                ai.abandon_pad(w, cx.controller);
                fsm.request_state(DONE);
                return;
            }
            ai.destroy_stuck(w, Some(cx.controller));
            return;
        }
        if w.interrupts_tick(cx.agent) {
            return;
        }
        w.update_combat_target(cx.agent);
        let now = w.now();
        match fsm.state {
            s if s == 1.0 => fsm.request_state(3.0),
            s if s == 2.0 => {
                if self.clear_above(w, cx) {
                    fsm.request_state(4.0);
                }
            }
            s if s == 3.0 => {
                if now > self.deadline {
                    let next = if self.clear_above(w, cx) { 4.0 } else { 2.0 };
                    fsm.request_state(next);
                }
            }
            s if s == 4.0 => {
                if self.find_hint(w, cx) {
                    fsm.request_state(5.0);
                } else if w.has_helper(cx.agent) && w.helper_arrived(cx.agent) {
                    // Arrived with no hint: pick again.
                    self.on_exit(w, fsm, cx);
                    self.on_enter(w, ai, fsm, cx);
                }
            }
            s if s == 5.0 => {
                if let Some(hint) = self.hint {
                    ai.hold_hint(w, hint.obj, cx.controller);
                }
                if !Self::airborne(w, cx) {
                    fsm.request_state(6.0);
                }
            }
            _ => {}
        }
        w.aim(cx.agent, 0.0);
        w.tick_helper(cx.agent, dt);
    }

    /// onExit() (agents.js:320).
    pub fn on_exit<W: AgentHost + ?Sized>(&self, w: &mut W, fsm: &Fsm, cx: &AgentCx) {
        if fsm.state == 5.0 && w.has_helper(cx.agent) {
            w.helper_land(cx.agent, false);
        }
    }

    /// clearAbove() (agents.js:325): 100 m straight up is clear.
    fn clear_above<W: AgentHost + ?Sized>(&self, w: &mut W, cx: &AgentCx) -> bool {
        let p = w.unit(cx.unit.expect("the flyer agent's unit")).position;
        let body = Self::flyer(w, cx).and_then(|v| v.body);
        w.cast(&p, &DOWN_UP, 100.0, body).is_none()
    }

    /// pickPost() (agents.js:334): state 4's destination.
    fn pick_post<W: AgentHost + ?Sized>(&mut self, w: &mut W, cx: &AgentCx) {
        let posts = w.posts();
        if posts.is_empty() {
            return;
        }
        let unit = w.unit(cx.unit.expect("the flyer agent's unit"));
        let transport = Self::transport(w, cx);
        let mut post;
        let mut attempt = 1;
        loop {
            post = posts[w.int(0.0, (posts.len() - 1) as f64) as usize];
            let ok = !transport || if self.flag { !self.friendly(w, &unit, post) } else { self.friendly(w, &unit, post) };
            let near = w.post(post).position.distance_to_squared(&unit.position) < 10000.0;
            if ok || !near || attempt >= 10 {
                break;
            }
            attempt += 1;
        }
        self.post = Some(post);
        let mut q = w.post(post).position;
        q.y += 20.0;
        if w.has_helper(cx.agent) {
            w.helper_goto(cx.agent, &q, false);
        }
    }

    /// friendly(post): held by the unit's side or an ally.
    fn friendly<W: World + ?Sized>(&self, w: &mut W, unit: &super::world::UnitView, post: Obj) -> bool {
        let team = w.post(post).team;
        w.relation(unit.team, team) > 0.0
    }

    /// findHint() (agents.js:359): one landing hint near, every 3 s.
    fn find_hint<W: AgentHost + ?Sized>(&mut self, w: &mut W, cx: &AgentCx) -> bool {
        let now = w.now();
        if !(now > self.search_at) {
            return false;
        }
        self.hint = None;
        self.search_at = now + 3.0;
        let unit = w.unit(cx.unit.expect("the flyer agent's unit"));
        let Some(found) = w.query_hints(&unit.position, cx.controller, now) else { return false };
        if found.is_empty() {
            return false;
        }
        let hint = found[w.int(0.0, (found.len() - 1) as f64) as usize];
        let Some(post) = hint.command_post else { return false };
        if Self::transport(w, cx) {
            let friendly = self.friendly(w, &unit, post);
            if if self.flag { friendly } else { !friendly } {
                return false;
            }
        }
        if hint.position.y > unit.position.y {
            return false;
        }
        self.hint = Some(hint);
        true
    }

    /// approach() (agents.js:379): the landing point in the hint's frame,
    /// and the approach to it.
    fn approach<W: AgentHost + ?Sized>(&mut self, w: &mut W, ai: &mut VehicleAI, fsm: &mut Fsm, cx: &AgentCx) {
        let Some(hint) = self.hint else {
            fsm.request_state(4.0);
            return;
        };
        ai.hold_hint(w, hint.obj, cx.controller);
        let unit = w.unit(cx.unit.expect("the flyer agent's unit"));
        let c = unit.class.clone().unwrap_or_default();
        let time = c.landing_time.unwrap_or(5.0);
        let up = 0.5 * time * c.landing_speed.unwrap_or(3.0) + c.landed_height.unwrap_or(0.0);
        let back = -0.5 * time * c.min_speed.unwrap_or(10.0);
        let mut l = hint.position;
        l.add_scaled_vector(&hint.up.unwrap_or(DOWN_UP), up).add_scaled_vector(&hint.forward, back);
        let d = Vector3::new(hint.forward.x, -0.2, hint.forward.z);
        let mut a = l;
        a.add_scaled_vector(&d, -2.0 * c.max_speed.unwrap_or(30.0));
        let p = unit.position;
        if w.has_helper(cx.agent) {
            w.helper_follow_path(cx.agent, &[[p.x, p.y, p.z], [a.x, a.y, a.z], [l.x, l.y, l.z]], true, true);
            w.helper_land(cx.agent, true);
        }
    }

    /// landed() (agents.js:399).
    fn landed<W: AgentHost + ?Sized>(&mut self, w: &mut W, ai: &mut VehicleAI, fsm: &mut Fsm, cx: &AgentCx) {
        let now = w.now();
        let post = self.hint.and_then(|h| h.command_post).or(self.post);
        if self.flag {
            ai.disembark_passengers(w, Some(cx.controller), post);
        }
        let transport = Self::transport(w, cx);
        if !transport {
            if let Some(hint) = self.hint {
                ai.close_hint(w, hint.obj, now + 20.0);
            }
            ai.request_exit(w, Some(cx.controller), post);
            w.set_reboard_at(cx.controller, now + 15.0);
        }
        if transport || !self.flag {
            self.flag = !self.flag;
            fsm.request_state(3.0);
        }
    }

    /// The pilot's policies: a scanner target only outside the climb-out
    /// and landing states and never for a transport; the crew wait yes;
    /// the interrupted goal saved.
    pub fn accepts_target<W: AgentHost + ?Sized>(&self, w: &mut W, fsm: &Fsm, cx: &AgentCx, target: Obj) -> bool {
        if fsm.state == 2.0 || fsm.state == 5.0 || Self::transport(w, cx) {
            return false;
        }
        w.can_engage(cx.agent, target)
    }
    pub fn accepts_hazard(&self) -> bool {
        false
    }
    pub fn accepts_alert(&self) -> bool {
        false
    }
    pub fn accepts_acoustic(&self) -> bool {
        false
    }
    pub fn accepts_board(&self) -> bool {
        false
    }
    pub fn accepts_secondary_wait(&self) -> bool {
        true
    }
    pub fn saves_on_interrupt(&self) -> bool {
        true
    }
}

/// Whether a unit is a control of a kind the vehicle agents fly.
pub fn is_flyer_control(k: f64) -> bool {
    k == kind::FLYER
}
