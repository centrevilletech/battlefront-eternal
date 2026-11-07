//! Classic combat (src/ai/classic/combat.js): the dispatcher that decides
//! whether and how a bot engages a target, the cover hints and their
//! reservations, the cover helper, the attack patterns, and the combat
//! agent (UnitCombatAgent, controller state 2).
//!
//! The agent runs on the control core's agent base (agents.js Agent): its
//! helper, interrupts and aim are the base's, reached through [`World`];
//! its own machine, its patterns' and its cover helper's are driven here by
//! the generic driver controller.js's Fsm is (`update`, `flush`,
//! `terminate`), transliterated in [`CombatAgent::update`].

use std::collections::HashMap;
use three::Vector3;

use super::aim::resolve;
use super::binding::CombatAi;
use super::weapons_ai::Shared;
use super::math::{angle, dot32, f, length_sq32, length_sq_xz32, lookup_cos, lookup_sin, normalize_d3dx, normalize_xz, FLT_MAX};
use super::perception::{sight_range, valid_target, TargetTracker};
use super::weapons_ai::{
    def_at, effective_range, forward_speed, lower_band, select_weapon, selected_weapon, sensor_range, switch_weapon, upper_band,
};
use super::world::{draw_int, draw_range as draw_range_raw, uniform, Engagement, Gen, GotoMode, HintView, Obj, QueryFilter, Ref, World};
use crate::combat::defs::Js;

/// controller.js:26 `DONE`.
pub const DONE: f64 = 100.0;

/// combat.js:53 `COS_ALIGN`.
fn cos_align() -> f64 {
    f(0.939)
}

/// combat.js:61 `now(world)`: the time as float32.
fn now<W: World + ?Sized>(w: &W) -> f64 {
    f(w.time())
}

// ---------------------------------------------------------------------------
// Engagement dispatch (combat.js:121).

/// combat.js:121 `roleOf(classDef)` (the combat dispatcher's own: ai.json's
/// role, then the class's, then UnitType's name; a name that is one of
/// Object.prototype's lower-case properties reads as no role, NaN).
pub fn role_of(class: Option<&super::world::ClassView>) -> f64 {
    let is_int = |v: &Js| matches!(v, Js::Num(x) if x.is_finite() && x.fract() == 0.0);
    if let Some(c) = class {
        if let Some(role)=c.prepared_role {return role;}
        if is_int(&c.ai_role) {
            return c.ai_role.num();
        }
        if is_int(&c.role) {
            return c.role.num();
        }
    }
    let raw = class.map(|c| c.unit_type.clone()).unwrap_or_default();
    let text = if raw.is_nullish() { String::new() } else { super::weapons_ai::js_string(&raw) };
    let name = text.trim_matches(bfe_content::js::is_js_space).to_lowercase();
    match name.as_str() {
        "scout" => 0.0,
        "anti-armor" => 1.0,
        "assault" => 2.0,
        "support" | "repair" | "special" => 3.0,
        "pilot" => 4.0,
        "trooper" => 5.0,
        "constructor" | "__proto__" => f64::NAN,
        _ => 6.0,
    }
}

/// combat.js:121 `dispatch(controller, unit, target, switchAllowed, world)`.
pub fn dispatch<W:World+?Sized>(w:&mut W,cx:&mut CombatAi,controller:Option<Obj>,unit:Obj,target:Obj,switch_allowed:bool)->Engagement {
    dispatch_shared(w,&mut cx.shared,controller,unit,target,switch_allowed)
}
/// Native nested policy callbacks borrow only the shared policy authorities.
pub fn dispatch_shared<W: World + ?Sized>(w: &mut W, shared: &mut Shared, controller: Option<Obj>, unit: Obj, target: Obj, switch_allowed: bool) -> Engagement {
    let mut out = Engagement { pattern: 0.0, probability: 0.0, aux: false, action: 6.0, in_band: false, weapon: None };
    let s = w.unit_scalars(unit).kind;
    let t = w.unit_scalars(target).kind;
    if s == 0.0 || s == 1.0 {
        let role = if s == 1.0 { 5.0 } else { role_of(w.unit(unit).class.as_ref()) };
        if t == 0.0 || t == 1.0 || t == 3.0 {
            foot_policy(w, shared, controller, unit, target, role, switch_allowed, &mut out);
        } else if t == 2.0 || t == 4.0 || t == 6.0 || t == 8.0 || t == 9.0 {
            vehicle_policy(w, shared, controller, unit, target, switch_allowed, &mut out);
        } else if t == 5.0 {
            select_only(w, shared, controller, unit, target, switch_allowed, &mut out);
        }
    } else if s == 3.0 || s == 7.0 {
        select_only(w, shared, controller, unit, target, switch_allowed, &mut out);
        if t != 5.0 && out.aux && out.in_band {
            out.pattern = 2.0;
        }
    } else {
        if w.has_vehicle_dispatch() {
            return w.vehicle_dispatch(shared, controller.expect("a vehicle dispatch's controller"), unit, target, switch_allowed);
        }
        select_only(w, shared, controller, unit, target, switch_allowed, &mut out);
    }
    out
}

/// combat.js:166 `selectOnly`.
fn select_only<W: World + ?Sized>(w: &mut W, shared: &mut Shared, controller: Option<Obj>, unit: Obj, target: Obj, switch_allowed: bool, out: &mut Engagement) {
    let choice = select_weapon(w, shared, controller, unit, Some(target), switch_allowed);
    out.aux = choice.ok;
    out.action = choice.action;
    out.in_band = choice.in_band;
    out.weapon = choice.index;
}

/// combat.js:179 `footPolicy`.
#[allow(clippy::too_many_arguments)]
fn foot_policy<W: World + ?Sized>(w: &mut W, shared: &mut Shared, controller: Option<Obj>, unit: Obj, target: Obj, role: f64, switch_allowed: bool, out: &mut Engagement) {
    if w.unit_scalars(unit).hero && w.unit_scalars(target).hero {
        return;
    }
    select_only(w, shared, controller, unit, target, switch_allowed, out);
    if w.unit_scalars(target).hero || !out.aux {
        return;
    }
    if role == 0.0 && out.action == 1.0 {
        out.pattern = if out.in_band { 2.0 } else { 0.0 };
        return;
    }
    if role == 2.0 && out.action == 4.0 {
        out.probability = 0.0;
        out.pattern = 6.0;
        return;
    }
    range_policy(w, controller, out);
}

/// combat.js:198 `rangePolicy(controller, world, out)`.
pub fn range_policy<W: World + ?Sized>(w: &W, controller: Option<Obj>, out: &mut Engagement) {
    let b = out.in_band;
    out.pattern = 0.0;
    if out.action == 2.0 {
        if b {
            out.pattern = 2.0;
        }
        return;
    }
    out.probability = f(0.5);
    let d = w.difficulty();
    let skill = if d.present { d.enemy_skill.unwrap_or(14.0) } else { 14.0 };
    let hard = if d.present { d.hard.unwrap_or(d.tier == Some(3.0)) } else { false };
    let roll = controller.map(|c| w.ctl(c).roll).unwrap_or(0.0);
    let i = ((roll % 6.0) + 6.0) % 6.0;
    out.pattern = if skill < 17.0 {
        if skill < 14.0 {
            if i <= 2.0 {
                if b { 2.0 } else { 3.0 }
            } else if i == 3.0 {
                3.0
            } else if i == 4.0 {
                4.0
            } else {
                5.0
            }
        } else if i <= 1.0 {
            if b { 2.0 } else { 3.0 }
        } else if i <= 3.0 {
            3.0
        } else if i == 4.0 {
            4.0
        } else {
            5.0
        }
    } else if hard {
        if i <= 4.0 { 4.0 } else { 5.0 }
    } else if i == 0.0 {
        if b { 2.0 } else { 3.0 }
    } else if i == 1.0 {
        3.0
    } else if i <= 4.0 {
        4.0
    } else {
        5.0
    };
}

/// combat.js:221 `vehiclePolicy`.
fn vehicle_policy<W: World + ?Sized>(w: &mut W, shared: &mut Shared, controller: Option<Obj>, unit: Obj, target: Obj, switch_allowed: bool, out: &mut Engagement) {
    out.probability = f(0.75);
    select_only(w, shared, controller, unit, target, switch_allowed, out);
    if !out.aux {
        out.pattern = 0.0;
        return;
    }
    if out.action <= 3.0 {
        let roll = controller.map(|c| w.ctl(c).roll).unwrap_or(0.0);
        let odd = v8math::abs(roll % 2.0) == 1.0;
        if !odd {
            out.pattern = 5.0;
        } else if out.in_band {
            out.pattern = 2.0;
        }
    } else if out.action == 4.0 || out.action == 6.0 {
        out.pattern = 0.0;
    }
}

// ---------------------------------------------------------------------------
// Cover hints and their reservations (combat.js:244).

/// A hint's reservation (combat.js:244: owner, time).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reservation {
    pub owner: Option<Obj>,
    pub at: f64,
}

/// combat.js:244 `reservations`: by hint (a module WeakMap; the battle's
/// here).
#[derive(Clone, Debug, Default)]
pub struct Reservations {
    pub by_hint: HashMap<Obj, Reservation>,
}

impl Reservations {
    /// Hints are authored once. Reserve their full set before controllers
    /// start taking cover, including rings not selected in the warmup.
    pub fn prepare(&mut self,hints:usize) {self.by_hint.reserve(hints.saturating_add(1));}
    /// combat.js:246 `hintAvailable(hint, owner, t)`.
    fn available(&self, hint: Obj, owner: Option<Obj>, t: f64) -> bool {
        match self.by_hint.get(&hint) {
            None => true,
            Some(r) => r.owner == owner || !(f(t - r.at) < 3.0),
        }
    }

    /// combat.js:251 `reserveHint(hint, owner, t)`: the hint and its ring.
    fn reserve(&mut self, hint: Option<&HintView>, owner: Option<Obj>, t: f64) {
        let Some(hint) = hint else { return };
        for h in ring(hint) {
            self.by_hint.insert(h, Reservation { owner, at: t });
        }
    }

    /// combat.js:256 `releaseHint(hint)`.
    fn release(&mut self, hint: Option<&HintView>) {
        let Some(hint) = hint else { return };
        for h in ring(hint) {
            if let Some(r) = self.by_hint.get_mut(&h) {
                r.at = f(-FLT_MAX);
            }
        }
    }
}

/// combat.js:264 `hintRing(hint)`.
fn ring(hint: &HintView) -> impl Iterator<Item=Obj>+'_ {
    std::iter::once(hint.obj).chain(hint.siblings.iter().copied().filter(|&s|s!=hint.obj))
}

/// `mask & bits` as JavaScript's bitwise and (ToInt32).
fn bits(mask: f64, b: i32) -> bool {
    v8math::to_int32(mask) & b != 0
}

/// combat.js:271 `drawStance(random, mask)`.
fn draw_stance<W: World + ?Sized>(w: &mut W, mask: f64) -> f64 {
    if !bits(mask, 7) {
        return 0.0;
    }
    loop {
        let k = draw_int(w, Gen::A, 0.0, 2.0);
        if bits(mask, 1 << (k as i32)) {
            return k;
        }
    }
}

/// combat.js:280 `drawSide(random, mask)`.
fn draw_side<W: World + ?Sized>(w: &mut W, mask: f64) -> f64 {
    if !bits(mask, 3) {
        return 0.0;
    }
    loop {
        let k = draw_int(w, Gen::A, 0.0, 1.0);
        if bits(mask, 1 << (k as i32)) {
            return k;
        }
    }
}

/// combat.js:961 `drawRange(random, lo, hi)`: lo itself, with no draw,
/// when the ends are equal.
fn draw_range<W: World + ?Sized>(w: &mut W, lo: f64, hi: f64) -> f64 {
    let lo = f(lo);
    let hi = f(hi);
    if lo == hi {
        lo
    } else {
        draw_range_raw(w, Gen::A, lo, hi)
    }
}

// ---------------------------------------------------------------------------
// The machines.

/// Which of the agent's three machines (each a controller.js Fsm).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum M {
    Agent,
    Patterns,
    Cover,
}

/// combat.js:332 `CoverHelper`'s own fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cover {
    pub state: f64,
    pub pending: f64,
    pub hint: Option<HintView>,
    pub posture: f64,
    pub deadline: f64,
    pub check_at: f64,
    pub reselect_at: f64,
}

/// combat.js:497 `AttackPatterns`' own fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Patterns {
    pub state: f64,
    pub pending: f64,
    pub cover: Cover,
    pub probability: f64,
    pub cover_retry: f64,
    pub prior: f64,
    pub path_active: bool,
    pub crouch: f64,
    pub paused: bool,
    pub pause_until: f64,
    pub toward: f64,
    pub side: f64,
    pub band_check_at: f64,
    pub step_at: f64,
    pub facing: bool,
    pub stepped: bool,
    pub lateral_at: f64,
    pub lateral_facing: bool,
    pub last_path: f64,
    pub retreat_at: f64,
    pub cover_hint: Option<HintView>,
    pub jetting: bool,
    pub jet_retry: f64,
    pub jet_start: f64,
    pub jet_hint: Option<Obj>,
    pub hint_jet_retry: f64,
    pub searched: bool,
    pub skip_fire: bool,
}

impl Default for Patterns {
    fn default() -> Self {
        Patterns {
            state: 0.0,
            pending: 0.0,
            cover: Cover::default(),
            probability: 0.0,
            cover_retry: -1.0,
            prior: DONE,
            path_active: false,
            crouch: 0.0,
            paused: false,
            pause_until: 0.0,
            toward: 1.0,
            side: 0.0,
            band_check_at: 0.0,
            step_at: 0.0,
            facing: false,
            stepped: false,
            lateral_at: 0.0,
            lateral_facing: false,
            last_path: -100.0,
            retreat_at: -1.0,
            cover_hint: None,
            jetting: false,
            jet_retry: 0.0,
            jet_start: 0.0,
            jet_hint: None,
            hint_jet_retry: 0.0,
            searched: false,
            skip_fire: false,
        }
    }
}

/// The combat source goal (controller.js SourceGoal: mode 0 location, 1
/// target, 2 evade, 3 prone; its point; refA the target).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Goal {
    /// `goal?.mode` (None: no goal).
    pub mode: Option<f64>,
    pub point: Option<Vector3>,
    pub ref_a: Option<Ref>,
}

/// combat.js:978 `UnitCombatAgent`: its machine and fields, its tracker,
/// its attack patterns. `obj` is the agent itself (what the base's calls
/// name), `controller` its controller.
#[derive(Clone, Debug, PartialEq)]
pub struct CombatAgent {
    pub obj: Obj,
    pub controller: Obj,
    pub state: f64,
    pub pending: f64,
    pub tracker: TargetTracker,
    pub patterns: Patterns,
    pub point: Vector3,
    pub hazard: Vector3,
    pub align_at: f64,
    pub looks: f64,
    pub look_angle: f64,
    pub aligned: bool,
    pub hazard_until: f64,
    pub prone_until: f64,
    pub pattern: f64,
    pub stationary: bool,
}

/// The controller's phase the machine runs in (controller.phase: 0 the
/// behaviour, 1 the ranking).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Behave,
    Rank,
}

/// How many transitions the driver takes in one update (controller.js:224
/// DRAIN_LIMIT) before it gives up (`runaway`).
const DRAIN_LIMIT: u32 = 64;

impl CombatAgent {
    /// combat.js:982 `constructor(controller, goal)`, after the base's
    /// (which made the helper and, through initAgent, the tracker
    /// `tracker`): the patterns, the fields, and the goal's state.
    pub fn new<W: World + ?Sized>(w: &W, obj: Obj, controller: Obj, tracker: TargetTracker, goal: Goal) -> CombatAgent {
        let mut a = CombatAgent {
            obj,
            controller,
            state: 1.0,
            pending: 0.0,
            tracker,
            patterns: Patterns::default(),
            point: Vector3::default(),
            hazard: Vector3::default(),
            align_at: 0.0,
            looks: 0.0,
            look_angle: 0.0,
            aligned: false,
            hazard_until: 0.0,
            prone_until: 0.0,
            pattern: 0.0,
            stationary: false,
        };
        match goal.mode {
            Some(m) if m == 0.0 => {
                a.pending = 2.0;
                if let Some(p) = goal.point {
                    a.point = p;
                }
            }
            Some(m) if m == 1.0 => {
                a.pending = 4.0;
                let t = resolve(w, goal.ref_a);
                a.tracker.reset(w, t);
            }
            Some(m) if m == 2.0 => {
                a.pending = 5.0;
                if let Some(p) = goal.point {
                    a.hazard = p;
                }
            }
            Some(m) if m == 3.0 => {
                a.pending = 6.0;
            }
            _ => {}
        }
        a
    }

    fn st(&self, m: M) -> f64 {
        match m {
            M::Agent => self.state,
            M::Patterns => self.patterns.state,
            M::Cover => self.patterns.cover.state,
        }
    }
    fn set_st(&mut self, m: M, v: f64) {
        match m {
            M::Agent => self.state = v,
            M::Patterns => self.patterns.state = v,
            M::Cover => self.patterns.cover.state = v,
        }
    }
    fn pend(&self, m: M) -> f64 {
        match m {
            M::Agent => self.pending,
            M::Patterns => self.patterns.pending,
            M::Cover => self.patterns.cover.pending,
        }
    }
    fn set_pend(&mut self, m: M, v: f64) {
        match m {
            M::Agent => self.pending = v,
            M::Patterns => self.patterns.pending = v,
            M::Cover => self.patterns.cover.pending = v,
        }
    }

    fn on_enter<W: World + ?Sized>(&mut self, m: M, w: &mut W, cx: &mut CombatAi) {
        match m {
            M::Agent => self.agent_enter(w, cx),
            M::Patterns => self.patterns_enter(w, cx),
            M::Cover => self.cover_enter(w, cx),
        }
    }
    fn on_exit<W: World + ?Sized>(&mut self, m: M, w: &mut W, cx: &mut CombatAi) {
        match m {
            M::Agent => self.agent_exit_state(w, cx),
            M::Patterns => self.patterns_exit(w, cx),
            M::Cover => self.cover_exit(w),
        }
    }
    fn on_tick<W: World + ?Sized>(&mut self, m: M, w: &mut W, cx: &mut CombatAi, dt: f64, phase: Phase) {
        match m {
            M::Agent => {
                // agents.js:367 Agent.onTick: the ranking pass in phase 1,
                // the behaviour in phase 0.
                if phase != Phase::Behave {
                    let retained = self.tracker.target;
                    super::perception::thin(w, &mut cx.los, self.controller, retained);
                    return;
                }
                self.behave(w, cx, dt);
            }
            M::Patterns => self.patterns_tick(w, cx, dt),
            M::Cover => self.cover_tick(w, cx),
        }
    }

    /// controller.js:233 `Fsm.update(dt, alternate)`.
    fn update<W: World + ?Sized>(&mut self, m: M, w: &mut W, cx: &mut CombatAi, dt: f64, alternate: bool, phase: Phase) {
        if alternate {
            if self.st(m) != 0.0 {
                self.on_tick(m, w, cx, dt, phase);
            }
            return;
        }
        let mut rounds = 0;
        let mut next = self.pend(m);
        loop {
            while next != 0.0 {
                rounds += 1;
                if rounds > DRAIN_LIMIT {
                    self.set_pend(m, 0.0);
                    return;
                }
                self.on_exit(m, w, cx);
                let p = self.pend(m);
                self.set_st(m, p);
                self.set_pend(m, 0.0);
                self.on_enter(m, w, cx);
                next = self.pend(m);
            }
            if self.st(m) == 0.0 {
                return;
            }
            self.on_tick(m, w, cx, dt, phase);
            next = self.pend(m);
            if next == 0.0 {
                return;
            }
        }
    }

    /// controller.js:258 `Fsm.flush()`.
    fn flush<W: World + ?Sized>(&mut self, m: M, w: &mut W, cx: &mut CombatAi) {
        let mut rounds = 0;
        while self.pend(m) != 0.0 {
            rounds += 1;
            if rounds > DRAIN_LIMIT {
                self.set_pend(m, 0.0);
                return;
            }
            self.on_exit(m, w, cx);
            let p = self.pend(m);
            self.set_st(m, p);
            self.set_pend(m, 0.0);
            self.on_enter(m, w, cx);
        }
    }

    // -- The base's entry points (agents.js Agent).

    /// agents.js:360 `tick(dt, phase)`: the machine's round; whether the
    /// agent reached its terminal state.
    pub fn tick<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi, dt: f64, phase: Phase) -> bool {
        self.update(M::Agent, w, cx, dt, phase == Phase::Behave, phase);
        self.state == DONE
    }

    /// combat.js:1022 `exit()`: the patterns let go of cover, then the
    /// base ends the helper and the machine (agents.js:379).
    pub fn exit<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        self.patterns_cleanup(cx);
        if w.has_helper(self.obj) {
            w.helper_terminate(self.obj);
        }
        // Fsm.terminate().
        self.agent_exit_state(w, cx);
        self.state = DONE;
    }

    // -- Policies (combat.js:1034).

    /// combat.js:1034 `acceptsAlert(known, point)`.
    pub fn accepts_alert<W: World + ?Sized>(&self, w: &mut W, known: bool, point: &Vector3) -> bool {
        let unit = w.ctl(self.controller).unit;
        let k = unit.map(|u| w.unit_scalars(u).kind);
        if k == Some(2.0) || k == Some(6.0) || k == Some(5.0) {
            return false;
        }
        if let Some(u) = unit {
            if forward_speed(&w.unit(u)) == 0.0 && self.mount_unreachable(w, point) {
                return true;
            }
        }
        if !known {
            return true;
        }
        if self.state == 3.0 {
            return true;
        }
        if self.state == 4.0 {
            return !in_forward_cone(w, unit.expect("the fighting agent's unit"), point);
        }
        false
    }

    /// combat.js:1047 `acceptsHazard()`.
    pub fn accepts_hazard<W: World + ?Sized>(&self, w: &W) -> bool {
        let k = w.ctl(self.controller).unit.map(|u| w.unit_scalars(u).kind);
        k != Some(5.0) && self.state != 5.0
    }

    /// combat.js:1055 `acceptsAcoustic(source)`.
    pub fn accepts_acoustic<W: World + ?Sized>(&self, w: &mut W, cx: &mut CombatAi, source: Option<Obj>) -> bool {
        self.accepts_acoustic_shared(w,&mut cx.shared,source)
    }
    /// Responses invoked during an agent tick share policy tables, rather than
    /// acquiring the already-active combat service owner a second time.
    pub fn accepts_acoustic_shared<W:World+?Sized>(&self,w:&mut W,shared:&mut Shared,source:Option<Obj>)->bool {
        let unit = w.ctl(self.controller).unit;
        let k = unit.map(|u| w.unit_scalars(u).kind);
        let Some(source) = source else { return false };
        if k == Some(2.0) || k == Some(6.0) || k == Some(5.0) {
            return false;
        }
        let unit = unit.expect("a listening agent's unit");
        let point = w.unit_scalars(source).position;
        if forward_speed(&w.unit(unit)) == 0.0 && self.mount_unreachable(w, &point) {
            return true;
        }
        if self.state == 2.0 || in_forward_cone(w, unit, &point) || self.tracker.target(w).is_none() {
            return false;
        }
        let p = w.unit_scalars(unit).position;
        let t = self.tracker.point;
        let a = length_sq_xz32(f(point.x - p.x), f(point.z - p.z));
        let b = length_sq_xz32(f(t.x - p.x), f(t.z - p.z));
        if !(a < f(b * f(0.43560004))) {
            return false;
        }
        can_engage_shared(w, shared, self.controller, Some(source))
    }

    /// combat.js:1075 `acceptsTarget()`: never.
    pub fn accepts_target(&self) -> bool {
        false
    }

    /// combat.js:1081 `acceptsBoard(candidate)`.
    pub fn accepts_board<W: World + ?Sized>(&self, w: &mut W, candidate: Obj) -> bool {
        let unit = w.ctl(self.controller).unit;
        boarding_clear(w, unit, Some(candidate)) && w.agent_board_policy(self.obj, candidate)
    }

    /// combat.js:1087, 1091: stagnation recovery may run; an interrupt
    /// does not save the combat goal.
    pub fn accepts_recovery(&self) -> bool {
        true
    }
    pub fn saves_on_interrupt(&self) -> bool {
        false
    }

    /// combat.js:1095 `mountUnreachable(point)`.
    fn mount_unreachable<W: World + ?Sized>(&self, w: &mut W, point: &Vector3) -> bool {
        if !w.has_mount_reach() {
            return false;
        }
        let unit = w.ctl(self.controller).unit.expect("the mount's unit");
        !w.mount_reach(unit, point)
    }

    // -- Shared steps (combat.js:1104).

    /// combat.js:1104 `facingTarget(cosLimit)`.
    fn facing_target<W: World + ?Sized>(&self, w: &mut W, cos_limit: f64) -> bool {
        let Some(target) = self.tracker.fresh_target(w) else { return false };
        let unit = w.ctl(self.controller).unit.expect("a facing agent's unit");
        let at = w.aim_point(target);
        let o = w.origin(unit);
        let n = normalize_d3dx(f(at.x - o.x), 0.0, f(at.z - o.z));
        let fwd = w.forward(unit);
        cos_limit < dot32(fwd.x, 0.0, fwd.z, n.x, n.y, n.z)
    }

    /// combat.js:1117 `nextLook()`.
    fn next_look<W: World + ?Sized>(&mut self, w: &mut W) {
        if self.looks == 0.0 {
            self.look_angle = f(draw_range(w, 30.0, 75.0) * f(0.017453292));
            if draw_int(w, Gen::A, 0.0, 1.0) == 0.0 {
                self.look_angle = f(self.look_angle * -1.0);
            }
        } else {
            self.look_angle = f(self.look_angle * -2.0);
        }
        let unit = w.ctl(self.controller).unit.expect("a looking agent's unit");
        let ca = w.forward(unit);
        let c = v8math::cos(self.look_angle);
        let s = v8math::sin(self.look_angle);
        let x = ca.x * c + ca.z * s;
        let z = -ca.x * s + ca.z * c;
        let p = w.unit_scalars(unit).position;
        self.point = Vector3 { x: f(p.x + f(x * 100.0)), y: f(p.y + f(f(0.0001) * 100.0)), z: f(p.z + f(z * 100.0)) };
        let point = self.point;
        w.ctl_mut(self.controller).look_at(&point);
        self.looks += 1.0;
    }

    /// combat.js:1134 `alignedTo(point)`.
    fn aligned_to<W: World + ?Sized>(&self, w: &mut W, point: &Vector3) -> bool {
        let unit = w.ctl(self.controller).unit.expect("an aligning agent's unit");
        let o = w.origin(unit);
        let n = normalize_d3dx(f(point.x - o.x), 0.0, f(point.z - o.z));
        let fwd = w.forward(unit);
        let m = normalize_d3dx(fwd.x, 0.0, fwd.z);
        cos_align() < dot32(n.x, n.y, n.z, m.x, m.y, m.z)
    }

    // -- The agent's machine (combat.js:1145).

    /// combat.js:1145 `onEnter()`.
    fn agent_enter<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        let c = self.controller;
        let unit = w.ctl(c).unit;
        let t = now(w);
        let k = unit.map(|u| w.unit_scalars(u).kind);
        let s = self.state;
        if k == Some(5.0) && (s == 2.0 || s == 3.0 || s == 5.0 || s == 6.0) {
            self.pending = DONE;
            return;
        }
        if s == 3.0 && k != Some(0.0) && k != Some(3.0) {
            self.pending = DONE;
            return;
        }
        w.ctl_mut(c).wanted_mode = 1.0;
        if s == 1.0 {
            self.helper_stop(w);
        } else if s == 2.0 {
            self.helper_stop(w);
            let p = w.unit(unit.expect("the agent's unit")).position;
            let dx = f(self.point.x - p.x);
            let dz = f(self.point.z - p.z);
            if length_sq_xz32(dx, dz) < 100.0 {
                let n = normalize_xz(dx, dz);
                self.point = Vector3 { x: f(p.x + f(n.x * 15.0)), y: f(p.y + f(n.y * 15.0)), z: f(p.z + f(n.z * 15.0)) };
            }
            let point = self.point;
            w.ctl_mut(c).look_at(&point);
            w.set_stance(c,0.0);
            self.aligned = false;
        } else if s == 3.0 {
            self.helper_stop(w);
            self.looks = 0.0;
            self.next_look(w);
            self.aligned = false;
        } else if s == 4.0 {
            self.helper_stop(w);
            let Some(target) = self.tracker.target(w) else {
                self.pending = 3.0;
                return;
            };
            let e = dispatch(w, cx, Some(c), unit.expect("the agent's unit"), target, true);
            self.pattern = e.pattern;
            if e.pattern != 0.0 && !e.pattern.is_nan() {
                self.patterns_init(w, cx, e.pattern, e.probability);
            } else {
                self.pending = DONE;
            }
        } else if s == 5.0 {
            self.evade(w, t);
        } else if s == 6.0 {
            self.helper_stop(w);
            w.set_stance(c,2.0);
            w.ctl_mut(c).look_along_path();
            self.prone_until = f(t + 4.0);
        }
    }

    /// combat.js:1197 `evade(t)`.
    fn evade<W: World + ?Sized>(&mut self, w: &mut W, t: f64) {
        let c = self.controller;
        let unit = w.ctl(c).unit.expect("an evading agent's unit");
        let p = w.unit_scalars(unit).position;
        let n = normalize_xz(f(p.x - self.hazard.x), f(p.z - self.hazard.z));
        let ca = Vector3 { x: f(p.x + f(n.x * 20.0)), y: f(p.y + f(n.y * 20.0)), z: f(p.z + f(n.z * 20.0)) };
        let mut dived = false;
        if draw_int(w, Gen::A, 0.0, 1.0) == 0.0 {
            let cn = normalize_xz(f(ca.x - p.x), f(ca.z - p.z));
            let fwd = w.forward(unit);
            let cm = normalize_xz(fwd.x, fwd.z);
            let cross_y = f(f(cn.z * cm.x) - f(cn.x * cm.z));
            let s45 = lookup_sin(angle::melee());
            if s45 < cross_y {
                w.ctl_mut(c).dodge = Some(1.0);
                self.helper_stop(w);
                dived = true;
            } else if cross_y < -s45 {
                w.ctl_mut(c).dodge = Some(-1.0);
                self.helper_stop(w);
                dived = true;
            }
        }
        if !dived {
            self.helper_goto(w, &ca, None);
        }
        w.set_stance(c,0.0);
        w.ctl_mut(c).look_along_path();
        self.hazard_until = f(t + 2.0);
    }

    /// combat.js:1224 `onExit()`.
    fn agent_exit_state<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        w.ctl_mut(self.controller).wanted_mode = 2.0;
        if self.state == 4.0 {
            self.patterns_exit(w, cx);
        }
    }

    /// combat.js:1230 `behave(dt)`: phase 0, one service's work.
    fn behave<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi, dt: f64) {
        let c = self.controller;
        let Some(unit) = w.ctl(c).unit else { return };
        if !w.unit_scalars(unit).alive {
            return;
        }
        let t = now(w);
        w.ctl_mut(c).wanted_mode = 1.0;
        let mut skip_fire = false;
        self.stationary = false;
        let changed = self.tracker.select(w);
        self.tracker.assign_weapon_target(w, &cx.shared.weapons);
        w.ctl_mut(c).alert_anim = Some(true);
        let jetting = self.patterns.state == 11.0 && w.has_helper(self.obj) && w.helper_flying(self.obj);
        if !jetting && self.sync_out(w, |w, agent| w.interrupts_tick(agent,cx)) {
            return;
        }
        let s = self.state;
        if s == 2.0 {
            let point = self.point;
            let mut handled = false;
            if self.aligned_to(w, &point) {
                if self.tracker.target(w).is_some() {
                    self.pending = 4.0;
                    handled = true;
                } else if !self.aligned {
                    self.aligned = true;
                    self.align_at = f(t + f(0.75));
                } else if self.align_at < t {
                    self.pending = 3.0;
                    handled = true;
                }
            }
            if !handled {
                w.ctl_mut(c).look_at(&point);
            }
        } else if s == 3.0 {
            if self.tracker.target(w).is_some() {
                self.pending = 4.0;
            } else {
                let point = self.point;
                let mut handled = false;
                if self.aligned_to(w, &point) {
                    if !self.aligned {
                        self.aligned = true;
                        self.align_at = f(t + f(0.1));
                    } else if self.align_at < t {
                        if self.looks < 2.0 {
                            self.next_look(w);
                            self.aligned = false;
                        } else {
                            self.pending = DONE;
                        }
                        handled = true;
                    }
                }
                if !handled {
                    // fastTurn(c): controller.requestFacing (soldiers and
                    // droidekas).
                    let k = w.ctl(c).unit.map(|u| w.unit_scalars(u).kind);
                    if k == Some(0.0) || k == Some(3.0) {
                        w.ctl_mut(c).facing = true;
                    }
                }
            }
        } else if s == 4.0 {
            'fight: {
                if jetting {
                    self.update(M::Patterns, w, cx, dt, false, Phase::Behave);
                    break 'fight;
                }
                if let Some(target) = self.tracker.target(w) {
                    let p = w.unit_scalars(unit).position;
                    let tp = self.tracker.point;
                    let limit = f(sensor_range(w, &cx.shared.weapons, unit, c) + 10.0);
                    if f(limit * limit) < length_sq_xz32(f(tp.x - p.x), f(tp.z - p.z)) {
                        self.pending = DONE;
                    }
                    let e = dispatch(w, cx, Some(c), unit, target, true);
                    if e.pattern != 0.0 && !e.pattern.is_nan() {
                        if !changed && e.pattern == self.pattern {
                            self.update(M::Patterns, w, cx, dt, false, Phase::Behave);
                            skip_fire = self.patterns.skip_fire;
                            if w.unit_scalars(unit).kind == 3.0 && self.raise_shield(w, unit) {
                                skip_fire = true;
                            }
                            break 'fight;
                        }
                        self.pending = 4.0;
                        break 'fight;
                    }
                }
                self.pending = 3.0;
            }
        } else if s == 5.0 {
            if self.hazard_until < t {
                self.pending = 3.0;
            }
        } else if s == 6.0 && self.prone_until < t {
            self.pending = 3.0;
        }
        if w.has_helper(self.obj) {
            w.helper_tick(self.obj, dt);
        }
        if !skip_fire {
            self.tracker.fire_controls(w, &cx.shared.weapons);
        }
    }

    /// A call into the agent's base that can read or write the agent (the
    /// interrupts read its policies and may request its state): the world
    /// sees the agent as it stands.
    fn sync_out<W: World + ?Sized, R>(&mut self, w: &mut W, call: impl FnOnce(&mut W, Obj) -> R) -> R {
        w.agent_sync(self);
        let r = call(w, self.obj);
        w.agent_resync(self);
        r
    }

    /// combat.js:1323 `raiseShield(unit)`.
    fn raise_shield<W: World + ?Sized>(&mut self, w: &mut W, unit: Obj) -> bool {
        let Some(shield) = w.unit(unit).soldier.as_ref().and_then(|s| s.shield) else { return false };
        if shield.on {
            return false;
        }
        if !(f(f(shield.max.unwrap_or(0.0)) * 0.5) < shield.value.unwrap_or(0.0)) {
            return false;
        }
        switch_weapon(w, self.controller, unit, 1);
        let c = w.ctl_mut(self.controller);
        c.fire_a = true;
        c.fire_b = false;
        true
    }

    fn helper_stop<W: World + ?Sized>(&self, w: &mut W) {
        if w.has_helper(self.obj) {
            w.helper_stop(self.obj);
        }
    }

    /// combat.js:109 `helperGoto(helper, point, options)`.
    fn helper_goto<W: World + ?Sized>(&self, w: &mut W, point: &Vector3, mode: Option<GotoMode>) {
        if w.has_helper(self.obj) {
            w.helper_goto(self.obj, point, mode);
        }
    }

    // -- The cover helper's machine (combat.js:332).

    /// combat.js:351 `init(hint)`.
    fn cover_init<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi, hint: Option<HintView>) {
        let t = now(w);
        self.patterns.cover.hint = hint;
        let owner = w.ctl(self.controller).unit;
        cx.reservations.reserve(self.patterns.cover.hint.as_ref(), owner, t);
        self.patterns.cover.posture = 0.0;
        self.patterns.cover.check_at = f(t + 5.0);
        let n = draw_int(w, Gen::A, 16.0, 25.0);
        self.patterns.cover.reselect_at = f(t + n);
        self.patterns.cover.pending = 2.0;
        self.flush(M::Cover, w, cx);
    }

    /// combat.js:362 `renew()`.
    fn cover_renew<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        let owner = w.ctl(self.controller).unit;
        cx.reservations.reserve(self.patterns.cover.hint.as_ref(), owner, now(w));
    }

    fn cover_hint(&self) -> HintView {
        self.patterns.cover.hint.clone().expect("the cover helper's hint")
    }

    /// combat.js:366 `onEnter()`.
    fn cover_enter<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        let t = now(w);
        let s = self.patterns.cover.state;
        if s == 2.0 {
            self.cover_renew(w, cx);
            let at = self.cover_hint().position;
            self.helper_goto(w, &at, None);
            self.patterns.cover.posture = 0.0;
            self.tracker.freeze(true, t);
        } else if s == 3.0 {
            let n = draw_int(w, Gen::A, 2.0, 4.0);
            self.patterns.cover.deadline = f(t + n);
            let mask = self.cover_hint().primary_mask.unwrap_or(1.0);
            self.patterns.cover.posture = draw_stance(w, mask);
            self.tracker.freeze(true, t);
        } else if s == 4.0 {
            let n = draw_int(w, Gen::A, 2.0, 6.0);
            self.patterns.cover.deadline = f(t + n);
            let mask = self.cover_hint().secondary_mask.unwrap_or(1.0);
            self.patterns.cover.posture = draw_stance(w, mask);
        } else if s == 5.0 {
            let n = draw_int(w, Gen::A, 4.0, 7.0);
            self.patterns.cover.deadline = f(t + n);
            let hint = self.cover_hint();
            let side = draw_side(w, hint.side_mask.unwrap_or(0.0));
            let mut across = Vector3::default();
            across.cross_vectors(&hint.forward, &Vector3 { x: 0.0, y: 1.0, z: 0.0 });
            across.multiply_scalar(if side != 0.0 { 3.0 } else { -3.0 });
            across.add(&hint.position);
            self.helper_goto(w, &across, None);
            self.patterns.cover.posture = draw_stance(w, hint.secondary_mask.unwrap_or(1.0));
            self.tracker.set_delay(4.0, t);
        }
    }

    /// combat.js:395 `onExit()`.
    fn cover_exit<W: World + ?Sized>(&mut self, w: &mut W) {
        let t = now(w);
        self.patterns.cover.posture = 0.0;
        let s = self.patterns.cover.state;
        if s > 1.0 && s < 4.0 {
            self.tracker.freeze(false, t);
        } else if s == 5.0 {
            self.tracker.default_delay(t);
        }
    }

    /// combat.js:403 `onTick()`.
    fn cover_tick<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        if self.patterns.cover.state == DONE {
            return;
        }
        let t = now(w);
        if self.patterns.cover.check_at < t {
            self.patterns.cover.check_at = f(t + 5.0);
            let hint = self.cover_hint();
            if cover_score(w, cx, self, &hint) == FLT_MAX {
                cx.reservations.release(Some(&hint));
                let next = find_cover(w, cx, self);
                match next {
                    None => {
                        self.patterns.cover.hint = None;
                        self.patterns.cover.pending = DONE;
                        return;
                    }
                    Some(h) => {
                        self.patterns.cover.hint = Some(h);
                        self.cover_renew(w, cx);
                        self.patterns.cover.pending = 2.0;
                        return;
                    }
                }
            }
        }
        if self.patterns.cover.reselect_at < t {
            let n = draw_int(w, Gen::A, 16.0, 25.0);
            self.patterns.cover.reselect_at = f(t + n);
            if let Some(h) = find_cover(w, cx, self) {
                self.patterns.cover.hint = Some(h);
                self.cover_renew(w, cx);
                self.patterns.cover.pending = 2.0;
                return;
            }
        }
        let s = self.patterns.cover.state;
        if s == 2.0 {
            let arrived = w.has_helper(self.obj) && w.helper_arrived(self.obj);
            if !arrived {
                self.cover_renew(w, cx);
            } else {
                self.patterns.cover.pending = 3.0;
            }
        } else if s == 3.0 {
            self.cover_renew(w, cx);
            if self.patterns.cover.deadline < t {
                let side = self.cover_hint().side_mask.unwrap_or(0.0);
                self.patterns.cover.pending = if side > 0.0 { 5.0 } else { 4.0 };
            }
        } else if s == 4.0 {
            self.cover_renew(w, cx);
            if self.patterns.cover.deadline < t {
                self.patterns.cover.pending = 3.0;
            }
        } else if s == 5.0 {
            self.cover_renew(w, cx);
            if self.patterns.cover.deadline < t {
                self.patterns.cover.pending = 2.0;
            }
        }
    }

    /// combat.js:458 `cleanup()`.
    fn cover_cleanup(&mut self, cx: &mut CombatAi) {
        cx.reservations.release(self.patterns.cover.hint.as_ref());
        self.patterns.cover.hint = None;
    }

    // -- The attack patterns' machine (combat.js:497).

    /// combat.js:543 `init(pattern, probability)`.
    fn patterns_init<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi, pattern: f64, probability: f64) {
        self.patterns.probability = f(probability);
        self.patterns.prior = DONE;
        self.patterns.pending = pattern;
        self.flush(M::Patterns, w, cx);
    }

    /// combat.js:551 `restore()`.
    fn restore(&mut self) {
        let next = self.patterns.prior;
        self.patterns.prior = DONE;
        self.patterns.pending = next;
    }

    /// combat.js:557 `onEnter()`.
    fn patterns_enter<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        let t = now(w);
        self.patterns.cover_retry = -1.0;
        let n = draw_int(w, Gen::A, 3.0, 8.0);
        self.patterns.jet_retry = f(t + n);
        let n = draw_int(w, Gen::A, 3.0, 8.0);
        self.patterns.hint_jet_retry = f(t + n);
        let s = self.patterns.state;
        if s == 2.0 {
            self.helper_stop(w);
            let roll = w.ctl(self.controller).roll;
            self.patterns.crouch = if roll % 3.0 == 1.0 { 1.0 } else { 0.0 };
        } else if s == 3.0 {
            self.reposition(w, cx);
            self.patterns.paused = false;
        } else if s == 4.0 {
            self.patterns.toward = 1.0;
            self.patterns.side = if draw_int(w, Gen::A, 0.0, 1.0) != 0.0 { 2.0 } else { 0.0 };
            self.patterns.band_check_at = 0.0;
            self.patterns.step_at = 0.0;
            self.flank(w);
        } else if s == 5.0 {
            self.patterns.lateral_at = 0.0;
            self.helper_stop(w);
        } else if s == 6.0 {
            self.patterns.path_active = false;
            self.patterns.last_path = -100.0;
        } else if s == 8.0 {
            self.helper_stop(w);
            self.patterns.retreat_at = -1.0;
        } else if s == 9.0 {
            let hint = self.patterns.cover_hint.clone();
            self.cover_init(w, cx, hint);
        } else if s == 10.0 {
            self.direct_jet(w);
        } else if s == 11.0 {
            if w.has_jet_hints() {
                let h = self.patterns.jet_hint;
                w.jet_hints_enter(self.controller, h);
            }
        } else if s == 12.0 {
            self.tracker.set_delay(16.0, t);
            let at = self.tracker.point;
            self.helper_goto(w, &at, None);
            self.patterns.searched = false;
        } else if s >= 7.0 {
            self.sync_out(w, |w, agent| w.enter_pattern(agent, s));
        }
    }

    /// combat.js:606 `onExit()`.
    fn patterns_exit<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        let t = now(w);
        w.set_stance(self.controller,0.0);
        let s = self.patterns.state;
        if s == 7.0 {
            self.tracker.default_delay(t);
            self.sync_out(w, |w, agent| w.exit_pattern(agent, s));
        } else if s == 12.0 {
            self.tracker.default_delay(t);
        } else if s == 9.0 {
            // The cover helper's own exit first (it holds the target).
            self.cover_exit(w);
            self.cover_cleanup(cx);
            self.patterns.cover.state = 0.0;
            self.patterns.cover.pending = 0.0;
        } else if s == 11.0 {
            if w.has_jet_hints() {
                let h = self.patterns.jet_hint;
                w.jet_hints_exit(self.controller, h);
            }
        } else if s >= 13.0 {
            self.sync_out(w, |w, agent| w.exit_pattern(agent, s));
        }
    }

    /// combat.js:647 `onTick(dt)`.
    fn patterns_tick<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi, dt: f64) {
        self.patterns.skip_fire = false;
        let s = self.patterns.state;
        if s == 2.0 {
            self.standing(w, cx)
        } else if s == 3.0 {
            self.pause_and_move(w, cx)
        } else if s == 4.0 {
            self.band_flank(w, cx)
        } else if s == 5.0 {
            self.lateral(w, cx)
        } else if s == 6.0 {
            self.melee(w, cx)
        } else if s == 8.0 {
            self.retreat(w, cx)
        } else if s == 9.0 {
            self.in_cover(w, cx, dt)
        } else if s == 10.0 {
            self.jet_tick(w)
        } else if s == 11.0 {
            self.hint_jet_tick(w, dt)
        } else if s == 12.0 {
            self.lost_target(w)
        } else {
            self.sync_out(w, |w, agent| w.tick_pattern(agent, s, dt));
        }
    }

    /// combat.js:675 `cleanup()`.
    fn patterns_cleanup(&mut self, cx: &mut CombatAi) {
        self.cover_cleanup(cx);
    }

    /// combat.js:680 `face(posture)`: the agent's aim (agents.js:395
    /// Agent.aim, the base's, through the combat services' currentTarget
    /// and memoryPoint): the stance, then the fresh target, else the
    /// remembered point, else along the path.
    pub(crate) fn face<W: World + ?Sized>(&mut self, w: &mut W, posture: f64) {
        let c = self.controller;
        w.set_stance(c,posture);
        if let Some(t) = self.tracker.fresh_target(w) {
            let r = Ref { obj: t, generation: w.unit_scalars(t).generation };
            w.ctl_mut(c).aim_at(Some(r));
            return;
        }
        if self.tracker.target(w).is_some() {
            let p = self.tracker.point;
            w.ctl_mut(c).look_at(&p);
        } else {
            w.ctl_mut(c).look_along_path();
        }
    }

    /// combat.js:687 `tryCover(from)`.
    fn try_cover<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi, from: f64) -> bool {
        let t = now(w);
        if !(self.patterns.cover_retry < t) {
            return false;
        }
        let n = draw_int(w, Gen::A, 16.0, 25.0);
        self.patterns.cover_retry = f(t + n);
        if !(draw_range_raw(w, Gen::A, 0.0, 1.0) < self.patterns.probability) {
            return false;
        }
        let hint = find_cover(w, cx, self);
        self.patterns.cover_hint = hint.clone();
        let Some(hint) = hint else { return false };
        let owner = w.ctl(self.controller).unit;
        cx.reservations.reserve(Some(&hint), owner, t);
        self.patterns.prior = from;
        self.patterns.pending = 9.0;
        true
    }

    /// combat.js:703 `jetReady()`.
    fn jet_ready<W: World + ?Sized>(&self, w: &W) -> bool {
        let Some(unit) = w.ctl(self.controller).unit else { return false };
        let u = w.unit(unit);
        if u.kind != 0.0 {
            return false;
        }
        // Missing class fields have the same nullish fallback as the empty
        // source object. Do not allocate a shared default for every jet poll.
        let cls = u.class.as_ref();
        let soldier = u.soldier.as_ref();
        let jet = soldier.filter(|s| s.jet);
        let jump = match jet {
            Some(s) if !s.jet_jump.is_nullish() => s.jet_jump.num(),
            _ => cls.map_or(0.0, |c| c.jet_jump.nn(0.0)),
        };
        if !(jump > 0.0) {
            return false;
        }
        let border = match jet {
            Some(s) if !s.jet_min_border.is_nullish() => s.jet_min_border.num(),
            _ => cls.map_or(0.0, |c| c.jet_fuel_min_border.nn(0.0)),
        };
        let fuel = soldier.map(|s| s.fuel.clone()).unwrap_or_default();
        let fuel = if fuel.is_nullish() { 0.0 } else { fuel.num() };
        f(border) < fuel
    }

    /// combat.js:714 `tryHintJet(from)`.
    fn try_hint_jet<W: World + ?Sized>(&mut self, w: &mut W, from: f64) -> bool {
        if !self.jet_ready(w) {
            return false;
        }
        let has = w.has_jet_hints();
        if has && w.jet_hints_excluded(self.controller) {
            return false;
        }
        let t = now(w);
        if !(self.patterns.hint_jet_retry < t) {
            return false;
        }
        let n = draw_int(w, Gen::A, 3.0, 8.0);
        self.patterns.hint_jet_retry = f(t + n);
        let hint = if has { w.jet_hints_find(self.controller) } else { None };
        self.patterns.jet_hint = hint;
        if hint.is_none() {
            return false;
        }
        self.patterns.prior = from;
        self.patterns.pending = 11.0;
        true
    }

    /// combat.js:731 `tryDirectJet(from)`.
    fn try_direct_jet<W: World + ?Sized>(&mut self, w: &mut W, from: f64) -> bool {
        if !self.jet_ready(w) {
            return false;
        }
        let t = now(w);
        if !(self.patterns.jet_retry < t) {
            return false;
        }
        let n = draw_int(w, Gen::A, 6.0, 12.0);
        self.patterns.prior = from;
        self.patterns.jet_retry = f(t + n);
        self.patterns.pending = 10.0;
        true
    }

    /// combat.js:743 `tryLostTarget(from)`.
    fn try_lost_target<W: World + ?Sized>(&mut self, w: &W, from: f64) -> bool {
        let t = now(w);
        if !(f(t - self.tracker.sample_time) > 2.0) {
            return false;
        }
        self.patterns.prior = from;
        self.patterns.pending = 12.0;
        true
    }

    /// combat.js:753 `standing()`: pattern 2.
    fn standing<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        if self.try_cover(w, cx, 2.0) || self.try_hint_jet(w, 2.0) || self.try_direct_jet(w, 2.0) {
            return;
        }
        let unit = w.ctl(self.controller).unit.expect("a standing agent's unit");
        let k = w.unit_scalars(unit).kind;
        if (k == 6.0 || k == 2.0) && forward_speed(&w.unit(unit)) < 13.0 {
            self.sync_out(w, |w, agent| w.sidestep(agent));
        }
        self.stationary = true;
        let crouch = self.patterns.crouch;
        self.face(w, crouch);
    }

    /// combat.js:764 `pauseAndMove()`: pattern 3.
    fn pause_and_move<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        if self.try_cover(w, cx, 3.0) || self.try_hint_jet(w, 3.0) || self.try_direct_jet(w, 3.0) || self.try_lost_target(w, 3.0) {
            return;
        }
        let t = now(w);
        if !self.patterns.paused {
            let arrived = w.has_helper(self.obj) && w.helper_arrived(self.obj);
            if arrived || draw_int(w, Gen::A, 1.0, 6.0) == 2.0 {
                self.patterns.paused = true;
                self.patterns.pause_until = f(t + f(1.9));
            }
            self.face(w, 0.0);
            return;
        }
        if t <= self.patterns.pause_until {
            self.helper_stop(w);
            self.face(w, 0.0);
            return;
        }
        self.patterns.paused = false;
        self.reposition(w, cx);
        self.face(w, 0.0);
    }

    /// combat.js:787 `reposition()`.
    fn reposition<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        let z = f(f(uniform(w, Gen::A) - 0.5) * 2.0);
        uniform(w, Gen::A); // y, dropped
        let x = f(f(uniform(w, Gen::A) - 0.5) * 2.0);
        let pn = normalize_d3dx(f(x + 0.0001), 0.0, z);
        let c = self.controller;
        let unit = w.ctl(c).unit.expect("a moving agent's unit");
        let weapon = selected_weapon(w, unit, Some(c));
        let mut r = 0.0;
        if let Some(i) = weapon {
            let def = def_at(w, unit, i);
            let a = cx.shared.weapons.weapon_ai(&def);
            let opt = a.optimal_range;
            let eff = effective_range(&cx.shared.weapons, w.unit_scalars(unit).kind, w.ctl(c).range_scale, &def);
            r = draw_range(w, f(opt - f(f(opt - a.min_range) * 0.2)), f(f(f(eff - opt) * 0.2) + opt));
        }
        let tp = self.tracker.point;
        let at = Vector3 { x: f(tp.x + f(pn.x * r)), y: f(tp.y + f(pn.y * r)), z: f(tp.z + f(pn.z * r)) };
        self.helper_goto(w, &at, None);
    }

    /// combat.js:812 `bandFlank()`: pattern 4.
    fn band_flank<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        if self.try_cover(w, cx, 4.0) || self.try_hint_jet(w, 4.0) || self.try_direct_jet(w, 4.0) || self.try_lost_target(w, 4.0) {
            return;
        }
        let t = now(w);
        let was = self.patterns.facing;
        self.patterns.facing = self.facing_target(w, lookup_cos(angle::face_tight()));
        if !self.patterns.facing {
            if was {
                self.patterns.step_at = 0.0;
            }
            if self.patterns.step_at < t {
                let in_band = self.step_to_band(w, cx);
                if !in_band {
                    self.patterns.step_at = f(t + 3.0);
                }
                self.patterns.stepped = in_band;
            }
            self.face(w, 0.0);
            return;
        }
        if !was {
            self.patterns.band_check_at = 0.0;
        }
        let c = self.controller;
        let unit = w.ctl(c).unit.expect("a flanking agent's unit");
        let tp = self.tracker.point;
        let p = w.unit_scalars(unit).position;
        let d2 = length_sq_xz32(f(tp.x - p.x), f(tp.z - p.z));
        if self.patterns.band_check_at < t {
            let lo = lower_band(w, &cx.shared.weapons, unit, c);
            if d2 < f(lo * lo) {
                return self.redirect(w, 3.0, t);
            }
            let hi = self.upper_band(w, cx);
            if f(hi * hi) < d2 {
                return self.redirect(w, 1.0, t);
            }
        }
        if !was || draw_int(w, Gen::A, 1.0, 2.0) == 1.0 {
            self.flank(w);
        }
        self.face(w, 0.0);
    }

    /// combat.js:844 `redirect(toward, t)`.
    fn redirect<W: World + ?Sized>(&mut self, w: &mut W, toward: f64, t: f64) {
        self.patterns.toward = toward;
        self.patterns.band_check_at = f(t + 3.0);
        self.flank(w);
        self.face(w, 0.0);
    }

    /// combat.js:851 `upperBand()`.
    fn upper_band<W: World + ?Sized>(&self, w: &mut W, cx: &CombatAi) -> f64 {
        let c = self.controller;
        let unit = w.ctl(c).unit.expect("the agent's unit");
        let sight = match self.tracker.target(w) {
            Some(t) => sight_range(w, unit, t),
            None => 0.0,
        };
        upper_band(w, &cx.shared.weapons, unit, c, sight)
    }

    /// combat.js:859 `flank()`.
    fn flank<W: World + ?Sized>(&mut self, w: &mut W) {
        let c = self.controller;
        let unit = w.ctl(c).unit.expect("a flanking agent's unit");
        let tp = self.tracker.point;
        let p = w.unit_scalars(unit).position;
        let pn = normalize_xz(f(tp.x - p.x), f(tp.z - p.z));
        self.patterns.side = if self.patterns.side != 0.0 { 0.0 } else { 2.0 };
        let mut pc = Vector3::default();
        pc.cross_vectors(&Vector3 { x: pn.x, y: pn.y, z: pn.z }, &Vector3 { x: 0.0, y: 1.0, z: 0.0 });
        let across = if self.patterns.side == 2.0 { 30.0 } else { -30.0 };
        let along = if self.patterns.toward == 1.0 { 30.0 } else { -30.0 };
        let pa = Vector3 {
            x: f(f(pc.x * across) + f(pn.x * along)),
            y: f(f(pc.y * across) + f(pn.y * along)),
            z: f(f(pc.z * across) + f(pn.z * along)),
        };
        let pa = Vector3 { x: f(p.x + pa.x), y: f(p.y + pa.y), z: f(p.z + pa.z) };
        self.helper_goto(w, &pa, None);
        if draw_int(w, Gen::A, 1.0, 5.0) == 2.0 {
            w.ctl_mut(c).dodge = Some(if self.patterns.side == 0.0 { -1.0 } else { 1.0 });
        }
    }

    /// combat.js:877 `stepToBand()`.
    fn step_to_band<W: World + ?Sized>(&mut self, w: &mut W, cx: &CombatAi) -> bool {
        let c = self.controller;
        let unit = w.ctl(c).unit.expect("a stepping agent's unit");
        let tp = self.tracker.point;
        let p = w.unit_scalars(unit).position;
        let dx = f(p.x - tp.x);
        let dz = f(p.z - tp.z);
        let pn = normalize_d3dx(dx, 0.0, dz);
        let d2 = length_sq_xz32(dx, dz);
        let lo = lower_band(w, &cx.shared.weapons, unit, c);
        if d2 < f(lo * lo) {
            let at = Vector3 { x: f(p.x + f(pn.x * 15.0)), y: f(p.y + f(pn.y * 15.0)), z: f(p.z + f(pn.z * 15.0)) };
            self.helper_goto(w, &at, None);
            return false;
        }
        let hi = self.upper_band(w, cx);
        if f(hi * hi) < d2 {
            let at = Vector3 { x: f(p.x - f(pn.x * 15.0)), y: f(p.y - f(pn.y * 15.0)), z: f(p.z - f(pn.z * 15.0)) };
            self.helper_goto(w, &at, None);
            return false;
        }
        self.helper_stop(w);
        true
    }

    /// combat.js:897 `lateral()`: pattern 5.
    fn lateral<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        if self.try_cover(w, cx, 5.0) || self.try_hint_jet(w, 5.0) || self.try_direct_jet(w, 5.0) || self.try_lost_target(w, 5.0) {
            return;
        }
        let t = now(w);
        if self.patterns.lateral_at < t {
            self.patterns.lateral_facing = false;
            let c = self.controller;
            let unit = w.ctl(c).unit.expect("a moving agent's unit");
            let tp = self.tracker.point;
            let p = w.unit_scalars(unit).position;
            let pn = normalize_d3dx(f(p.x - tp.x), 0.0, f(p.z - tp.z));
            if !self.facing_target(w, lookup_cos(angle::face_wide())) {
                if self.step_to_band(w, cx) {
                    self.face(w, 0.0);
                    return;
                }
            } else {
                self.patterns.lateral_facing = true;
                let mut side = Vector3::default();
                side.cross_vectors(&Vector3 { x: pn.x, y: 0.0, z: pn.z }, &Vector3 { x: 0.0, y: 1.0, z: 0.0 });
                let mut pb = w.forward(unit);
                pb.y = 0.0;
                let turn = f(f(pb.z * pn.x) - f(pb.x * pn.z));
                let optimal = match selected_weapon(w, unit, Some(c)) {
                    Some(i) => cx.shared.weapons.weapon_ai(&def_at(w, unit, i)).optimal_range,
                    None => 0.0,
                };
                let k = f(optimal * (if turn > 0.0 { -1.0 } else { 1.0 }));
                let at = Vector3 { x: f(tp.x + f(side.x * k)), y: f(tp.y + f(side.y * k)), z: f(tp.z + f(side.z * k)) };
                self.helper_goto(w, &at, None);
            }
            self.patterns.lateral_at = f(t + 4.0);
        }
        self.face(w, 0.0);
    }

    /// combat.js:929 `melee()`: pattern 6.
    fn melee<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        let _ = cx;
        if self.try_lost_target(w, 6.0) || self.try_direct_jet(w, 6.0) || self.try_hint_jet(w, 6.0) {
            return;
        }
        let t = now(w);
        let unit = w.ctl(self.controller).unit.expect("a fighting agent's unit");
        let target = self.tracker.target(w);
        if let (Some(target), true) = (target, w.has_helper(self.obj)) {
            let tp = w.unit_scalars(target).position;
            if w.nav_direct_clear(unit, &tp).unwrap_or(false) {
                if w.helper_has_follow(self.obj) {
                    w.helper_follow(self.obj, target);
                } else {
                    w.helper_goto(self.obj, &tp, Some(GotoMode::Direct));
                }
                self.patterns.path_active = false;
            } else {
                let mut wait = false;
                if self.patterns.path_active {
                    if w.helper_arrived(self.obj) {
                        self.patterns.path_active = false;
                    } else {
                        wait = true;
                    }
                }
                if !wait && f(self.patterns.last_path + 2.0) < t {
                    self.patterns.path_active = true;
                    self.patterns.last_path = t;
                    w.helper_goto(self.obj, &tp, None);
                }
            }
        }
        self.face(w, 0.0);
    }

    /// combat.js:958 `retreat()`: pattern 8 (no stock dispatch selects it).
    fn retreat<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi) {
        if self.try_cover(w, cx, 8.0) || self.try_hint_jet(w, 8.0) || self.try_direct_jet(w, 8.0) {
            return;
        }
        let t = now(w);
        if self.patterns.retreat_at < t {
            self.patterns.retreat_at = f(t + draw_range(w, 3.0, 6.0));
            let unit = w.ctl(self.controller).unit.expect("a retreating agent's unit");
            let tp = self.tracker.point;
            let p = w.unit_scalars(unit).position;
            let pn = normalize_xz(f(p.x - tp.x), f(p.z - tp.z));
            let a = draw_range(w, f(-0.7853981852531433), f(0.7853981852531433));
            let c = v8math::cos(a);
            let s = v8math::sin(a);
            let at = Vector3 { x: p.x + 100.0 * (pn.x * c + pn.z * s), y: p.y, z: p.z + 100.0 * (-pn.x * s + pn.z * c) };
            self.helper_goto(w, &at, None);
        }
        self.face(w, 0.0);
    }

    /// combat.js:976 `inCover(dt)`: pattern 9.
    fn in_cover<W: World + ?Sized>(&mut self, w: &mut W, cx: &mut CombatAi, dt: f64) {
        self.update(M::Cover, w, cx, dt, false, Phase::Behave);
        if self.patterns.cover.state != DONE {
            let posture = self.patterns.cover.posture;
            self.face(w, posture);
            return;
        }
        self.restore();
    }

    /// combat.js:989 `directJet()`: ten tries at a point, every one of
    /// which navigation's missing pointClear refuses (quirks R1c).
    fn direct_jet<W: World + ?Sized>(&mut self, w: &mut W) {
        let unit = w.ctl(self.controller).unit.expect("a jetting agent's unit");
        for _ in 0..10 {
            let z = f(f(f(uniform(w, Gen::B) - 0.5) * 2.0) * 40.0);
            uniform(w, Gen::B);
            let x = f(f(f(uniform(w, Gen::B) - 0.5) * 2.0) * 40.0);
            if length_sq32(x, 0.0, z) < 400.0 {
                continue;
            }
            let p = w.unit_scalars(unit).position;
            let pa = Vector3 { x: f(p.x + x), y: p.y, z: f(p.z + z) };
            if !w.nav_point_clear(&pa, unit).unwrap_or(false) {
                continue;
            }
            let pb = Vector3 { x: pa.x + 0.0 * 40.0, y: pa.y + 1.0 * 40.0, z: pa.z + 0.0 * 40.0 };
            if !w.los(&pa, &pb, Some(unit), None) {
                continue;
            }
            if !w.mission_no_jet_obstacle_test() && w.nav_segment_blocked(&p, &pa, unit).unwrap_or(false) {
                continue;
            }
            self.helper_goto(w, &pa, Some(GotoMode::Direct));
            w.request_jet(self.controller, &Vector3::default());
            self.patterns.jet_start = now(w);
            self.patterns.jetting = true;
            return;
        }
        self.patterns.jet_start = -10.0;
        self.restore();
    }

    /// combat.js:1015 `jetTick()`.
    fn jet_tick<W: World + ?Sized>(&mut self, w: &mut W) {
        if f(self.patterns.jet_start + 1.0) < now(w) && !(w.has_helper(self.obj) && w.helper_flying(self.obj)) {
            self.patterns.jetting = false;
            self.restore();
        }
    }

    /// combat.js:1022 `hintJetTick(dt)`.
    fn hint_jet_tick<W: World + ?Sized>(&mut self, w: &mut W, dt: f64) {
        let done = if w.has_jet_hints() {
            let h = self.patterns.jet_hint;
            w.jet_hints_tick(self.controller, h, dt)
        } else {
            true
        };
        if done {
            self.restore();
        }
    }

    /// combat.js:1029 `lostTarget()`: pattern 12.
    fn lost_target<W: World + ?Sized>(&mut self, w: &mut W) {
        if self.tracker.fresh_target(w).is_some() {
            self.restore();
            return;
        }
        let arrived = w.has_helper(self.obj) && w.helper_arrived(self.obj);
        if arrived && !self.patterns.searched {
            self.patterns.searched = true;
            self.tracker.default_delay(now(w));
            self.next_look(w);
            return;
        }
        self.face(w, 0.0);
    }
}

/// agents.js:390 Agent.canEngage through the combat services' canEngage
/// (classic/index.js:140): a weapon in the arsenal and the dispatcher
/// would engage.
pub fn can_engage<W:World+?Sized>(w:&mut W,cx:&mut CombatAi,controller:Obj,target:Option<Obj>)->bool {
    can_engage_shared(w,&mut cx.shared,controller,target)
}
pub fn can_engage_shared<W: World + ?Sized>(w: &mut W, shared: &mut Shared, controller: Obj, target: Option<Obj>) -> bool {
    let Some(unit) = w.ctl(controller).unit else { return false };
    if w.weapon_count(unit)==0 {
        return false;
    }
    let Some(target) = target else { return false };
    let e = dispatch_shared(w, shared, Some(controller), unit, target, false);
    e.pattern != 0.0 && !e.pattern.is_nan()
}

/// combat.js:318 `coverScore(agent, hint)`: lower is better, FLT_MAX when
/// unsuitable.
pub fn cover_score<W: World + ?Sized>(w: &mut W, cx: &CombatAi, a: &CombatAgent, hint: &HintView) -> f64 {
    let c = a.controller;
    let unit = w.ctl(c).unit.expect("a covering agent's unit");
    let tp = a.tracker.point;
    let p = w.unit_scalars(unit).position;
    let h = hint.position;
    let cos60 = lookup_cos(angle::face_wide());
    let dx = f(tp.x - p.x);
    let dz = f(tp.z - p.z);
    let ex = f(h.x - tp.x);
    let ey = f(h.y - tp.y);
    let ez = f(h.z - tp.z);
    if !(dot32(dx, 0.0, dz, ex, ey, ez) < 0.0) {
        return FLT_MAX;
    }
    let n = normalize_xz(f(tp.x - h.x), f(tp.z - h.z));
    let fw = hint.forward;
    if !(cos60 < dot32(fw.x, fw.y, fw.z, n.x, n.y, n.z)) {
        return FLT_MAX;
    }
    let r = cover_range(w, cx, c, unit);
    let h2 = length_sq_xz32(ex, ez);
    if !(h2 < f(r * r)) {
        return FLT_MAX;
    }
    let l = lower_band(w, &cx.shared.weapons, unit, c);
    if !(f(l * l) < h2) {
        return FLT_MAX;
    }
    f(f(length_sq_xz32(f(h.x - p.x), f(h.z - p.z)) * 2.25) + h2)
}

/// combat.js:340 `coverRange(agent)`.
fn cover_range<W: World + ?Sized>(w: &W, cx: &CombatAi, c: Obj, unit: Obj) -> f64 {
    let Some(i) = selected_weapon(w, unit, Some(c)) else { return 40.0 };
    let r = effective_range(&cx.shared.weapons, w.unit_scalars(unit).kind, w.ctl(c).range_scale, &def_at(w, unit, i));
    if r <= 40.0 {
        r
    } else {
        40.0
    }
}

/// combat.js:357 `findCover(agent)`: the cover hints within the cover
/// range, visited in a random order; the available one scoring lowest.
pub fn find_cover<W: World + ?Sized>(w: &mut W, cx: &mut CombatAi, a: &CombatAgent) -> Option<HintView> {
    let c = a.controller;
    let unit = w.ctl(c).unit.expect("a covering agent's unit");
    let p = w.unit_scalars(unit).position;
    let half = v8math::trunc(cover_range(w, cx, c, unit));
    let mut list = w.cover_hints(&p, half);
    list.truncate(30);
    let t = now(w);
    let mut best: Option<HintView> = None;
    let mut best_score = FLT_MAX;
    let mut i = list.len() as isize - 1;
    while i >= 0 {
        let k = draw_int(w, Gen::A, 0.0, i as f64) as usize;
        let hint = list[k].clone();
        if cx.reservations.available(hint.obj, Some(unit), t) {
            let s = cover_score(w, cx, a, &hint);
            if s < best_score {
                best = Some(hint);
                best_score = s;
            }
        }
        list[k] = list[i as usize].clone();
        i -= 1;
    }
    best
}

/// combat.js:1339 `boardingClear(world, unit, candidate)`: no valid hostile
/// target within 8 of the candidate (the first 30 met).
pub fn boarding_clear<W: World + ?Sized>(w: &mut W, unit: Option<Obj>, candidate: Option<Obj>) -> bool {
    let Some(candidate) = candidate else { return true };
    let position = w.unit_scalars(candidate).position;
    let found = w.query(&position, 8.0, QueryFilter::ExceptTwo { a: candidate, b: unit });
    let unit = unit.expect("a boarding agent's unit");
    for &u in found.iter().take(30) {
        if valid_target(w, unit, u) {
            return false;
        }
    }
    true
}

/// combat.js:1349 `inForwardCone(unit, point)`.
pub fn in_forward_cone<W: World + ?Sized>(w: &mut W, unit: Obj, point: &Vector3) -> bool {
    let cos_cone = lookup_cos(angle::cone());
    let p = w.unit_scalars(unit).position;
    let cn = normalize_xz(f(point.x - p.x), f(point.z - p.z));
    let fwd = w.forward(unit);
    let cm = normalize_xz(fwd.x, fwd.z);
    cos_cone < dot32(cm.x, cm.y, cm.z, cn.x, cn.y, cn.z)
}

#[cfg(test)]
mod reservation_tests {
    use super::*;
    #[test]
    fn hint_rings_keep_source_order_duplicates_and_reservation_expiry() {
        let hint=HintView {obj:9,siblings:vec![9,4,4,7,9].into(),..HintView::default()};
        assert_eq!(ring(&hint).collect::<Vec<_>>(),vec![9,4,4,7]);
        let mut reservations=Reservations::default();reservations.prepare(3);
        let capacity=reservations.by_hint.capacity();
        for step in 0..120 {
            let now=step as f64;
            reservations.reserve(Some(&hint),Some(12),now);
            assert!(reservations.available(4,Some(12),now));
            assert!(!reservations.available(7,Some(13),now+2.));
            assert!(reservations.available(7,Some(13),now+3.));
            reservations.release(Some(&hint));
            assert!(reservations.available(4,Some(13),now));
        }
        assert_eq!(reservations.by_hint.len(),3);assert_eq!(reservations.by_hint.capacity(),capacity);
    }
}
