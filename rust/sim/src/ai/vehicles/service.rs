//! The vehicle AI service (src/ai/classic/vehicles/index.js VehicleAI): the
//! services the Classic controller, agents, combat and scheduler call for
//! anything to do with vehicles and turrets, and the state it keeps (seat
//! reservations, the notifier's counter, the pads bots keep off, the fire
//! support hubs found for each post, the attack run's distances).
//!
//! What index.js's adaptClassicAI patches into a ClassicAI at run time is
//! here as plain entry points the AI's controller calls (plan P14, made
//! explicit): `current_controllable` (the control a Character's controller
//! drives), `adapt_controller` (the policy it takes on a bind),
//! `mounted_state` (the state its request becomes), `agent_for` (the
//! mounted agent), `seat_input` / `analogs` (a seat's input step).

use three::Vector3;
use v8math::{abs, asin, atan2, cos, hypot, max, min, sin, trunc};

use super::controllers::{self, MountedAgent, Policy, DONE};
use super::patterns::{self, AttackRun, Dispatch, PatternsView};
use super::reservations::{Reservations, ReservationService, SEAT};
use super::steering::{self, seat_analogs, steer_state, Env};
use super::support::{self, out_of_reach, SupportHub, REACH_MARGIN};
use super::notify::Notifier;
use super::world::{kind, ExitOptions, HelperKind, LeaseView, Obj, Origin, QueryFilter, Request, Stick, UnitView, World};

/// Stands of 20 to 30 s a pilot makes at a support point before it asks
/// for another post (index.js SUPPORT_HOLDS).
const SUPPORT_HOLDS: f64 = 3.0;
/// How near two hulls' footprints count as touching (index.js CONTACT_MARGIN).
const CONTACT_MARGIN: f64 = 0.5;

/// A body a registry entry, a control or a lease names (bodyOf).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Body {
    Hull(Obj),
    Turret(Obj),
}

/// The vehicle AI's own state (VehicleAI's fields).
#[derive(Clone, Debug, Default)]
pub struct VehicleAI {
    pub reservations: Reservations,
    pub notifier: Notifier,
    /// `padLeft`: hull -> until when no bot boards it (abandonPad).
    pub pad_left: Vec<(Obj, f64)>,
    /// `supportCache`: post -> "mask/radius" -> its support hubs.
    pub support_cache: Vec<(Obj, SupportKeys)>,
    /// The attack run's distances (patterns.js `run`).
    pub run: AttackRun,
}

/// One post's support hubs by "mask/radius" key, in the order found.
pub type SupportKeys = Vec<(String, Vec<SupportHub>)>;

/// A hull's decay clock (`v.decay`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Decay {
    pub idle: f64,
    pub sample: f64,
}

/// What analogs reads of its options.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnalogOptions {
    /// `options.controller ?? unit.controller`.
    pub controller: Option<Obj>,
    /// The caller ran the path executor's step.
    pub stepped: bool,
}

/// The patrol helper's mounted branch as patrolMounted reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatrolView {
    pub obj: Obj,
    pub agent: Obj,
    pub controller: Obj,
    pub post: Option<Obj>,
    /// `patrol.agent.support` is truthy.
    pub support: bool,
    /// `patrol.helper` is there.
    pub helper: bool,
}

/// patrolMounted's answer, and what it asked of the patrol.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatrolOutcome {
    pub kept: bool,
}

/// JavaScript's `\s` (a whitespace run in String.prototype.split(/\s+/)).
fn js_space(c: char) -> bool {
    bfe_content::js::is_js_space(c)
}

/// degrees(value, fallback) (index.js): two numbers of the setting's
/// text, in radians, else the fallback's.
fn degrees(value: Option<&str>, fallback: [f64; 2]) -> [f64; 2] {
    let text = value.unwrap_or("");
    let mut parts = Vec::new();
    let mut rest = text;
    // split(/\s+/): the pieces between whitespace runs, an empty piece
    // first or last where the text starts or ends with one.
    loop {
        match rest.find(js_space) {
            None => {
                parts.push(rest);
                break;
            }
            Some(i) => {
                parts.push(&rest[..i]);
                let tail = &rest[i..];
                let skip = tail.find(|c: char| !js_space(c)).unwrap_or(tail.len());
                rest = &tail[skip..];
                if rest.is_empty() {
                    parts.push("");
                    break;
                }
            }
        }
    }
    let numbers: Vec<f64> = parts.iter().map(|p| bfe_content::js::string_to_number(p)).filter(|n| n.is_finite()).collect();
    let pair = if numbers.len() >= 2 { [numbers[0], numbers[1]] } else { fallback };
    [(pair[0] * std::f64::consts::PI) / 180.0, (pair[1] * std::f64::consts::PI) / 180.0]
}

impl VehicleAI {
    pub fn new() -> Self {
        VehicleAI::default()
    }

    pub fn body_of_unit(u: &UnitView) -> Option<Body> {
        if let Some(h) = u.vehicle {
            return Some(Body::Hull(h));
        }
        u.turret.map(Body::Turret)
    }

    pub fn body_of_lease(l: &LeaseView) -> Option<Body> {
        if let Some(h) = l.vehicle {
            return Some(Body::Hull(h));
        }
        l.turret.map(Body::Turret)
    }

    pub fn entry_of<W: World + ?Sized>(w: &W, body: Option<Body>) -> Option<Obj> {
        match body? {
            Body::Hull(h) => Some(w.hull(h).entry),
            Body::Turret(t) => w.turret(t).entry,
        }
    }

    pub fn lease_of<W: World + ?Sized>(w: &W, x: Option<Obj>) -> Option<LeaseView> {
        let u = w.unit(x?);
        let own = u.lease.map(|l| w.lease(l)).filter(|l| l.controllable == Some(u.obj));
        let lease = match own {
            Some(l) => l,
            None => w.lease(u.seat?),
        };
        if lease.released || lease.human {
            return None;
        }
        Some(lease)
    }

    pub fn controller_lease<W: World + ?Sized>(w: &W, controller: Option<Obj>) -> Option<LeaseView> {
        Self::lease_of(w, w.controller(controller?).unit)
    }

    pub fn mounted_state(policy: Option<Policy>, state: f64) -> f64 {
        controllers::mounted_state(policy, state)
    }

    pub fn agent_for(policy: Option<Policy>, state: f64) -> Option<MountedAgent> {
        policy?.agent_for(state)
    }

    pub fn seat_count<W: World + ?Sized>(&self, w: &W, entry: Obj) -> f64 { VehicleServices::seat_count(self, w, entry) }

    pub fn occupied_seats<W: World + ?Sized>(&self, w: &W, entry: Obj) -> f64 { VehicleServices::occupied_seats(self, w, entry) }

    pub fn human_occupants<W: World + ?Sized>(&self, w: &W, entry: Obj) -> f64 { VehicleServices::human_occupants(self, w, entry) }

    pub fn human_aboard<W: World + ?Sized>(&self, w: &W, entry: Obj) -> bool { VehicleServices::human_aboard(self, w, entry) }

    pub fn boardables<W: World + ?Sized>(&self, w: &W) -> Vec<Obj> { VehicleServices::boardables(self, w) }

    pub fn cycles_per_second<W: World + ?Sized>(&self, w: &W) -> f64 { VehicleServices::cycles_per_second(self, w) }

    pub fn current_controllable<W: World + ?Sized>(&self, w: &W, character: Option<Obj>) -> Option<Obj> { VehicleServices::current_controllable(self, w, character) }

    pub fn adapt_controller<W: World + ?Sized>(&self, w: &W, unit: Option<Obj>) -> (Option<Policy>, Option<bool>) { VehicleServices::adapt_controller(self, w, unit) }

    pub fn seat_input<W: World + ?Sized>(&mut self, w: &mut W, unit: Obj, dt: f64, aim: Option<&Vector3>) -> Stick { VehicleServices::seat_input(self, w, unit, dt, aim) }

    pub fn analogs<W: World + ?Sized>(&mut self, w: &mut W, unit: Obj, request: Option<&Request>, aim: Option<&Vector3>, dt: f64, options: AnalogOptions) -> Stick { VehicleServices::analogs(self, w, unit, request, aim, dt, options) }

    pub fn walker_contacts<W: World + ?Sized>(&self, w: &mut W, unit: &UnitView) { VehicleServices::walker_contacts(self, w, unit) }

    pub fn create_helper<W: World + ?Sized>(&self, w: &W, controller: Option<Obj>) -> HelperKind { VehicleServices::create_helper(self, w, controller) }

    pub fn boarded<W: World + ?Sized>(&self, w: &W, controller: Option<Obj>) -> bool { VehicleServices::boarded(self, w, controller) }

    pub fn boarded_vehicle<W: World + ?Sized>(&self, w: &W, controller: Option<Obj>) -> Option<Obj> { VehicleServices::boarded_vehicle(self, w, controller) }

    pub fn boarded_vehicle_of<W: World + ?Sized>(&self, w: &W, unit: Option<Obj>) -> Option<Obj> { VehicleServices::boarded_vehicle_of(self, w, unit) }

    pub fn boarded_vehicle_of_character<W: World + ?Sized>(&self, w: &W, character: Option<Obj>) -> Option<Obj> { VehicleServices::boarded_vehicle_of_character(self, w, character) }

    pub fn is_mounted_turret<W: World + ?Sized>(&self, w: &W, sender: Option<Obj>) -> bool { VehicleServices::is_mounted_turret(self, w, sender) }

    pub fn empty_seats<W: World + ?Sized>(&self, w: &W, sender: Option<Obj>) -> f64 { VehicleServices::empty_seats(self, w, sender) }

    pub fn crew_controllers<W: World + ?Sized>(&self, w: &W, entry: Option<Obj>, source: Option<Obj>) -> Vec<Obj> { VehicleServices::crew_controllers(self, w, entry, source) }

    pub fn seats_full<W: World + ?Sized>(&self, w: &W, entry: Obj) -> bool { VehicleServices::seats_full(self, w, entry) }

    pub fn pending_reservations<W: World + ?Sized>(&mut self, w: &W, entry: Obj, team: f64) -> bool { VehicleServices::pending_reservations(self, w, entry, team) }

    pub fn request_exit<W: World + ?Sized>(&self, w: &mut W, controller: Option<Obj>, post: Option<Obj>) -> bool { VehicleServices::request_exit(self, w, controller, post) }

    pub fn low_tickets<W: World + ?Sized>(&self, w: &W, team: f64) -> bool { VehicleServices::low_tickets(self, w, team) }

    pub fn pilot_keeps_seat(&self) -> bool { VehicleServices::pilot_keeps_seat(self) }

    pub fn alerts_blocked(&self) -> bool { VehicleServices::alerts_blocked(self) }

    pub fn refuses_hold(&self) -> bool { VehicleServices::refuses_hold(self) }

    pub fn can_enter<W: World + ?Sized>(&self, w: &mut W, controller: Option<Obj>, candidate: Obj) -> bool { VehicleServices::can_enter(self, w, controller, candidate) }

    pub fn free_seats<W: World + ?Sized>(&mut self, w: &W, candidate: Obj, team: f64, kind: f64) -> f64 { VehicleServices::free_seats(self, w, candidate, team, kind) }

    pub fn reserve<W: World + ?Sized>(&mut self, w: &W, candidate: Obj, unit: Obj, kind: f64) { VehicleServices::reserve(self, w, candidate, unit, kind) }

    pub fn reserve_seat<W: World + ?Sized>(&mut self, w: &W, controller: Option<Obj>, candidate: Option<Obj>) -> bool { VehicleServices::reserve_seat(self, w, controller, candidate) }

    pub fn hold_reservation<W: World + ?Sized>(&mut self, w: &W, controller: Option<Obj>, candidate: Obj) -> bool { VehicleServices::hold_reservation(self, w, controller, candidate) }

    pub fn board_now<W: World + ?Sized>(&mut self, w: &mut W, controller: Option<Obj>, candidate: Obj) -> bool { VehicleServices::board_now(self, w, controller, candidate) }

    pub fn board_path_clear<W: World + ?Sized>(&self, w: &mut W, controller: Option<Obj>, candidate: Obj) -> bool { VehicleServices::board_path_clear(self, w, controller, candidate) }

    pub fn relation<W: World + ?Sized>(&self, w: &mut W, a: f64, b: f64) -> f64 { VehicleServices::relation(self, w, a, b) }

    pub fn board_policy<W: World + ?Sized>(&self, w: &mut W, agent: Obj, controller: Obj, candidate: Obj) -> bool { VehicleServices::board_policy(self, w, agent, controller, candidate) }

    pub fn nominal_speed<W: World + ?Sized>(&self, w: &W, candidate: &UnitView) -> f64 { VehicleServices::nominal_speed(self, w, candidate) }

    pub fn target_reserved<W: World + ?Sized>(&mut self, w: &W, target: Obj, team: f64) -> bool { VehicleServices::target_reserved(self, w, target, team) }

    pub fn reserve_target<W: World + ?Sized>(&mut self, w: &W, target: Obj, unit: Obj) { VehicleServices::reserve_target(self, w, target, unit) }

    pub fn mount_reach<W: World + ?Sized>(&self, w: &mut W, unit: Option<Obj>, point: &Vector3) -> bool { VehicleServices::mount_reach(self, w, unit, point) }

    pub fn body_reach<W: World + ?Sized>(&self, w: &mut W, body: Option<Body>, point: &Vector3) -> bool { VehicleServices::body_reach(self, w, body, point) }

    pub fn unreachable_from_mount<W: World + ?Sized>(&self, w: &mut W, controller: Option<Obj>, point: &Vector3) -> bool { VehicleServices::unreachable_from_mount(self, w, controller, point) }

    pub fn shared_target<W: World + ?Sized>(&self, w: &mut W, controller: Option<Obj>) -> Option<Obj> { VehicleServices::shared_target(self, w, controller) }

    pub fn dispatch<W: World + ?Sized>(&self, w: &mut W, controller: Obj, unit: Obj, target: Obj, switch_allowed: bool) -> Dispatch { VehicleServices::dispatch(self, w, controller, unit, target, switch_allowed) }

    pub fn enter_pattern<W: World + ?Sized>(&mut self, w: &mut W, p: &PatternsView, state: &mut (impl patterns::PatternAccess + ?Sized), pattern: f64) { VehicleServices::enter_pattern(self, w, p, state, pattern) }

    pub fn tick_pattern<W: World + ?Sized>(&mut self, w: &mut W, p: &PatternsView, state: &mut (impl patterns::PatternAccess + ?Sized), pattern: f64) { VehicleServices::tick_pattern(self, w, p, state, pattern) }

    pub fn exit_pattern<W: World + ?Sized>(&self, w: &mut W, p: &PatternsView, pattern: f64) { VehicleServices::exit_pattern(self, w, p, pattern) }

    pub fn sidestep<W: World + ?Sized>(&self, w: &mut W, p: &PatternsView) -> bool { VehicleServices::sidestep(self, w, p) }

    pub fn is_at_at<W: World + ?Sized>(&self, w: &W, unit: Obj) -> bool { VehicleServices::is_at_at(self, w, unit) }

    pub fn patrol_mounted<W: World + ?Sized>(&self, w: &mut W, patrol: &PatrolView) -> bool { VehicleServices::patrol_mounted(self, w, patrol) }

    pub fn support_point<W: World + ?Sized>(&mut self, w: &mut W, unit: Option<Obj>, post: Option<Obj>, helper: Option<Obj>) -> Option<Vector3> { VehicleServices::support_point(self, w, unit, post, helper) }

    pub fn hold_hint<W: World + ?Sized>(&self, w: &mut W, hint: Obj, controller: Obj) { VehicleServices::hold_hint(self, w, hint, controller) }

    pub fn close_hint<W: World + ?Sized>(&self, w: &mut W, hint: Obj, until: f64) { VehicleServices::close_hint(self, w, hint, until) }

    pub fn disembark_passengers<W: World + ?Sized>(&self, w: &mut W, controller: Option<Obj>, post: Option<Obj>) -> f64 { VehicleServices::disembark_passengers(self, w, controller, post) }

    pub fn destroy_stuck<W: World + ?Sized>(&self, w: &mut W, controller: Option<Obj>) { VehicleServices::destroy_stuck(self, w, controller) }

    pub fn abandon_pad<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj) -> f64 { VehicleServices::abandon_pad(self, w, controller) }

    pub fn update_decay<W: World + ?Sized>(&self, w: &mut W, dt: f64) { VehicleServices::update_decay(self, w, dt) }

    pub fn player_near<W: World + ?Sized>(&self, w: &mut W, v: &super::world::HullView) -> bool { VehicleServices::player_near(self, w, v) }

    pub fn reachable<W: World + ?Sized>(&self, w: &mut W, entry: Obj) -> bool { VehicleServices::reachable(self, w, entry) }
}

/// Access to the service's persistent state. Closures only read or write
/// owned fields; they must never call a world method or an import.
pub trait VehicleStorage {
    fn read<R>(&self, f: impl FnOnce(&VehicleAI) -> R) -> R;
    fn write<R>(&mut self, f: impl FnOnce(&mut VehicleAI) -> R) -> R;
}
impl VehicleStorage for VehicleAI {
    fn read<R>(&self, f: impl FnOnce(&VehicleAI) -> R) -> R { f(self) }
    fn write<R>(&mut self, f: impl FnOnce(&mut VehicleAI) -> R) -> R { f(self) }
}
impl<T: VehicleStorage> VehicleServices for T {}

/// The same algorithms run against owned replay state or a live accessor.
/// The live accessor holds no reference to the battle across a world call.
pub trait VehicleServices: VehicleStorage {
    /// seatCount(entry).
    fn seat_count<W: World + ?Sized>(&self, w: &W, entry: Obj) -> f64 {
        match VehicleAI::body_of_unit(&w.unit(entry)) {
            None => 0.0,
            Some(Body::Hull(h)) => w.hull(h).seats.len() as f64,
            Some(Body::Turret(_)) => 1.0,
        }
    }

    /// occupiedSeats(entry).
    fn occupied_seats<W: World + ?Sized>(&self, w: &W, entry: Obj) -> f64 {
        match VehicleAI::body_of_unit(&w.unit(entry)) {
            None => 0.0,
            Some(Body::Hull(h)) => {
                let hull = w.hull(h);
                let human = hull.seats.iter().any(|s| s.lease.is_some_and(|l| w.lease(l).human));
                hull.occupants + if hull.driver && !human { 1.0 } else { 0.0 }
            }
            Some(Body::Turret(t)) => {
                let turret = w.turret(t);
                if turret.lease.is_some() || turret.gunner {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    /// humanOccupants(entry).
    fn human_occupants<W: World + ?Sized>(&self, w: &W, entry: Obj) -> f64 {
        match VehicleAI::body_of_unit(&w.unit(entry)) {
            None => 0.0,
            Some(Body::Hull(h)) => {
                let hull = w.hull(h);
                let n = hull.seats.iter().filter(|s| s.lease.is_some_and(|l| w.lease(l).human)).count() as f64;
                if n != 0.0 {
                    n
                } else if hull.driver {
                    1.0
                } else {
                    0.0
                }
            }
            Some(Body::Turret(t)) => {
                let turret = w.turret(t);
                let human = turret.lease.is_some_and(|l| w.lease(l).human);
                if human || (turret.gunner && turret.lease.is_none()) {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    /// humanAboard(entry).
    fn human_aboard<W: World + ?Sized>(&self, w: &W, entry: Obj) -> bool {
        self.human_occupants(w, entry) > 0.0
    }

    /// boardables(): built turrets' entries, then living vehicles'.
    fn boardables<W: World + ?Sized>(&self, w: &W) -> Vec<Obj> {
        let mut out = Vec::new();
        for t in w.turrets() {
            let turret = w.turret(t);
            if turret.alive {
                if let Some(e) = turret.entry {
                    out.push(e);
                }
            }
        }
        for h in w.hulls() {
            let hull = w.hull(h);
            if hull.alive {
                out.push(hull.entry);
            }
        }
        out
    }

    /// cyclesPerSecond(): `Math.trunc(30 / cycle) || 1`.
    fn cycles_per_second<W: World + ?Sized>(&self, w: &W) -> f64 {
        let n = trunc(30.0 / w.settings().cycle);
        if n == 0.0 || n.is_nan() {
            1.0
        } else {
            n
        }
    }

    /// currentControllable(character): the control a Character's controller
    /// drives, the seat its body sits in (a bot), else None (its own unit).
    fn current_controllable<W: World + ?Sized>(&self, w: &W, character: Option<Obj>) -> Option<Obj> {
        let unit = w.character_unit(character?)?;
        let lease = w.lease(w.unit(unit).seat?);
        if lease.human || lease.released {
            return None;
        }
        let seat = lease.controllable?;
        if w.unit(seat).alive {
            Some(seat)
        } else {
            None
        }
    }

    /// adaptController(controller, unit): the policy a controller bound to
    /// `unit` takes, and the home-facing flag a stationary one sets.
    fn adapt_controller<W: World + ?Sized>(&self, w: &W, unit: Option<Obj>) -> (Option<Policy>, Option<bool>) {
        let view = unit.map(|u| w.unit(u));
        let policy = controllers::controller_policy(view.as_ref());
        let home = match (policy, view.as_ref()) {
            (Some(p), Some(v)) => p.home_facing(v),
            _ => None,
        };
        (policy, home)
    }

    /// seatInput(control, unit, intent, dt, aim, options): the path
    /// executor's step (navigation's), then the seat's analogs, which go
    /// into the intent's four analogs.
    fn seat_input<W: World + ?Sized>(&mut self, w: &mut W, unit: Obj, dt: f64, aim: Option<&Vector3>) -> Stick {
        let controller = w.unit(unit).controller;
        w.nav_attach(controller, unit);
        w.nav_update(controller, dt);
        let request = w.nav_request(controller);
        let out = self.analogs(w, unit, request.as_ref(), aim, dt, AnalogOptions { controller, stepped: true });
        w.set_intent_analogs(unit, &out);
        out
    }

    /// analogs(unit, request, aim, dt, out, options): the AI input step's
    /// analog conversion for a seat.
    fn analogs<W: World + ?Sized>(&mut self, w: &mut W, unit: Obj, request: Option<&Request>, aim: Option<&Vector3>, dt: f64, options: AnalogOptions) -> Stick {
        let view = w.unit(unit);
        let controller = options.controller.or(view.controller);
        let mut request = request.copied();
        if !options.stepped {
            if let Some(c) = controller {
                if w.controller(c).nav_control {
                    w.nav_attach(Some(c), unit);
                    w.nav_update(Some(c), dt);
                    request = w.nav_request(Some(c)).or(request);
                }
            }
        }
        let cv = controller.map(|c| w.controller(c));
        let settings = w.settings().clone();
        let env = Env {
            now: w.now(),
            // Boolean(controller?.navControl?.executor ?? controller?.agent?.helper?.hasPath)
            has_path: cv.as_ref().is_some_and(|c| (c.nav_control && c.executor) || c.helper_has_path),
            status: cv.as_ref().map_or(0.0, |c| c.executor_status),
            min_fly_height: settings.min_fly_height,
            max_fly_height: settings.max_fly_height,
            controller,
        };
        if view.kind == kind::WALKER {
            self.walker_contacts(w, &view);
        }
        seat_analogs(w, &view, request.as_ref(), aim, dt, &env)
    }

    /// walkerContacts(unit): a walker's yielding contacts with the hulls
    /// whose footprints it moves into.
    fn walker_contacts<W: World + ?Sized>(&self, w: &mut W, unit: &UnitView) {
        let Some(me) = unit.vehicle else { return };
        let me = w.hull(me);
        let v = me.velocity;
        for other in w.hulls() {
            let other = w.hull(other);
            if other.obj == me.obj || !other.alive {
                continue;
            }
            let reach = me.bound_radius + other.bound_radius;
            if me.position.distance_to_squared(&other.position) > reach * reach {
                continue;
            }
            if !(v.x * (other.position.x - me.position.x) + v.z * (other.position.z - me.position.z) > 0.0) {
                continue;
            }
            if !footprints_touch(&me, &other, CONTACT_MARGIN) {
                continue;
            }
            let entry = w.unit(other.entry);
            let bounds = [[other.bounds_min.x, other.bounds_min.y, other.bounds_min.z], [other.bounds_max.x, other.bounds_max.y, other.bounds_max.z]];
            let now = w.now();
            steering::walker_contact(w, unit, entry.kind, &other.position, &other.velocity, Some(bounds), now);
        }
    }

    /// createHelper(agent): the movement helper a control's agent gets
    /// (None: the AI's own).
    fn create_helper<W: World + ?Sized>(&self, w: &W, controller: Option<Obj>) -> HelperKind {
        let unit = controller.and_then(|c| w.controller(c).unit);
        let Some(lease) = VehicleAI::lease_of(w, unit) else { return HelperKind::None };
        if unit != lease.controllable {
            return HelperKind::None;
        }
        let k = w.unit(unit.expect("a lease's unit")).kind;
        if k == kind::MOUNTED {
            return HelperKind::Stationary;
        }
        if k == kind::FLYER && w.has_planner() {
            return HelperKind::Fly;
        }
        HelperKind::None
    }

    /// boarded(controller).
    fn boarded<W: World + ?Sized>(&self, w: &W, controller: Option<Obj>) -> bool {
        VehicleAI::controller_lease(w, controller).is_some()
    }

    /// boardedVehicle(controller).
    fn boarded_vehicle<W: World + ?Sized>(&self, w: &W, controller: Option<Obj>) -> Option<Obj> {
        let lease = VehicleAI::controller_lease(w, controller)?;
        VehicleAI::entry_of(w, VehicleAI::body_of_lease(&lease))
    }

    /// boardedVehicleOf(unit).
    fn boarded_vehicle_of<W: World + ?Sized>(&self, w: &W, unit: Option<Obj>) -> Option<Obj> {
        let lease = VehicleAI::lease_of(w, unit)?;
        VehicleAI::entry_of(w, VehicleAI::body_of_lease(&lease))
    }

    /// boardedVehicleOfCharacter(character).
    fn boarded_vehicle_of_character<W: World + ?Sized>(&self, w: &W, character: Option<Obj>) -> Option<Obj> {
        let unit = character.and_then(|c| w.character_unit(c));
        self.boarded_vehicle_of(w, unit)
    }

    /// isMountedTurret(sender): `sender.turret ?? sender.lease?.turret ?? sender.seat?.turret`.
    fn is_mounted_turret<W: World + ?Sized>(&self, w: &W, sender: Option<Obj>) -> bool {
        let Some(s) = sender else { return false };
        let u = w.unit(s);
        u.turret.is_some() || u.lease.is_some_and(|l| w.lease(l).turret.is_some()) || u.seat.is_some_and(|l| w.lease(l).turret.is_some())
    }

    /// emptySeats(sender).
    fn empty_seats<W: World + ?Sized>(&self, w: &W, sender: Option<Obj>) -> f64 {
        let body = VehicleAI::lease_of(w, sender).and_then(|l| VehicleAI::body_of_lease(&l)).or_else(|| sender.and_then(|s| VehicleAI::body_of_unit(&w.unit(s))));
        match body {
            Some(Body::Hull(h)) => w.hull(h).free_seats,
            _ => 0.0,
        }
    }

    /// crewControllers(entry, source): the AI controllers aboard, but the
    /// source Character's.
    fn crew_controllers<W: World + ?Sized>(&self, w: &W, entry: Option<Obj>, source: Option<Obj>) -> Vec<Obj> {
        let mut out = Vec::new();
        let Some(Body::Hull(h)) = entry.and_then(|e| VehicleAI::body_of_unit(&w.unit(e))) else { return out };
        for seat in w.hull(h).seats {
            let Some(lease) = seat.lease.map(|l| w.lease(l)) else { continue };
            // `lease.unit?.character === source` (undefined without a unit).
            let same = lease.unit.is_some_and(|u| w.unit(u).character == source);
            if lease.human || same {
                continue;
            }
            let controller = lease.controllable.and_then(|c| w.unit(c).controller).or_else(|| lease.unit.and_then(|u| w.unit(u).controller));
            if let Some(c) = controller {
                out.push(c);
            }
        }
        out
    }

    /// seatsFull(entry).
    fn seats_full<W: World + ?Sized>(&self, w: &W, entry: Obj) -> bool {
        self.occupied_seats(w, entry) >= self.seat_count(w, entry)
    }

    /// pendingReservations(entry, team).
    fn pending_reservations<W: World + ?Sized>(&mut self, w: &W, entry: Obj, team: f64) -> bool {
        self.reservation_count(w, entry, team, SEAT) > 0.0
    }

    /// requestExit(controller, post): leave at the next input batch, taking
    /// up `post` once out.
    fn request_exit<W: World + ?Sized>(&self, w: &mut W, controller: Option<Obj>, post: Option<Obj>) -> bool {
        let Some(lease) = VehicleAI::controller_lease(w, controller) else { return false };
        let character = lease.unit.and_then(|u| w.unit(u).character);
        if let (Some(post), Some(character)) = (post, character) {
            w.set_objective(character, post);
        }
        let unit = lease.unit;
        if lease.vehicle.is_some() {
            return w.request_exit_vehicle(unit, ExitOptions { post: Some(post), promote: None, placement: None });
        }
        if lease.turret.is_some() {
            return w.request_exit_turret(unit, ExitOptions { post: Some(post), promote: None, placement: None });
        }
        false
    }

    /// lowTickets(team): teams 1 and 2 with 25 reinforcements or fewer.
    fn low_tickets<W: World + ?Sized>(&self, w: &W, team: f64) -> bool {
        if team != 1.0 && team != 2.0 {
            return false;
        }
        w.reinforcements(team).is_some_and(|left| left.is_finite() && left <= 25.0)
    }

    /// pilotKeepsSeat(), alertsBlocked(), refusesHold(): false for every
    /// stock control.
    fn pilot_keeps_seat(&self) -> bool {
        false
    }

    fn alerts_blocked(&self) -> bool {
        false
    }

    fn refuses_hold(&self) -> bool {
        false
    }

    /// canEnter(controller, candidate): a living soldier on foot, the
    /// candidate alive and not left for a pad's while, by the boarding
    /// rules every boarder follows.
    fn can_enter<W: World + ?Sized>(&self, w: &mut W, controller: Option<Obj>, candidate: Obj) -> bool {
        let Some(unit) = controller.and_then(|c| w.controller(c).unit) else { return false };
        let u = w.unit(unit);
        if u.seat.is_some() || !u.soldier || u.walker || u.kind != 0.0 || !u.alive {
            return false;
        }
        if u.class.as_ref().map_or(0.0, |c| c.no_enter_vehicles) != 0.0 {
            return false;
        }
        let entry = w.unit(candidate);
        let Some(body) = VehicleAI::body_of_unit(&entry) else { return false };
        let alive = match body {
            Body::Hull(h) => w.hull(h).alive,
            Body::Turret(t) => w.turret(t).alive,
        };
        if !alive {
            return false;
        }
        let body_obj = match body {
            Body::Hull(h) | Body::Turret(h) => h,
        };
        let until = self.read(|s| s.pad_left.iter().find(|(h, _)| *h == body_obj).map_or(f64::NEG_INFINITY, |(_, t)| *t));
        if until > w.now() {
            return false;
        }
        if let Body::Hull(h) = body {
            if w.hull(h).own_system {
                return w.eligible_seat(unit, h) >= 0.0;
            }
        }
        let team = entry.team;
        if team > 0.0 && team != u.team {
            return false;
        }
        match body {
            Body::Hull(h) => w.first_free_seat(h) >= 0.0,
            Body::Turret(t) => {
                let turret = w.turret(t);
                !turret.gunner && turret.lease.is_none()
            }
        }
    }

    /// freeSeats(candidate, team, type).
    fn free_seats<W: World + ?Sized>(&mut self, w: &W, candidate: Obj, team: f64, kind: f64) -> f64 {
        let seats = self.seat_count(w, candidate);
        let humans = self.human_occupants(w, candidate);
        { let reserved = self.reservation_count(w, candidate, team, kind);
            let players = if kind == SEAT { humans } else { 0.0 };
            if seats < players + reserved { 0.0 } else { seats - players - reserved } }
    }

    /// reserve(candidate, unit, type).
    fn reserve<W: World + ?Sized>(&mut self, w: &W, candidate: Obj, unit: Obj, kind: f64) {
        self.reservation_reserve(w, candidate, unit, kind);
    }

    /// reserveSeat(controller, candidate): the boarding agent's entry.
    fn reserve_seat<W: World + ?Sized>(&mut self, w: &W, controller: Option<Obj>, candidate: Option<Obj>) -> bool {
        let Some(unit) = controller.and_then(|c| w.controller(c).unit) else { return false };
        let Some(candidate) = candidate else { return false };
        if self.reservation_has(w, candidate, unit, SEAT) {
            return true;
        }
        let team = w.unit(unit).team;
        if self.free_seats(w, candidate, team, SEAT) > 0.0 {
            self.reservation_reserve(w, candidate, unit, SEAT);
            return true;
        }
        false
    }

    /// holdReservation(controller, candidate): the boarding agent's tick.
    fn hold_reservation<W: World + ?Sized>(&mut self, w: &W, controller: Option<Obj>, candidate: Obj) -> bool {
        let Some(unit) = controller.and_then(|c| w.controller(c).unit) else { return false };
        if !self.reservation_has(w, candidate, unit, SEAT) {
            return false;
        }
        self.reservation_reserve(w, candidate, unit, SEAT);
        true
    }

    /// boardNow(controller, candidate): the board request, carried out.
    fn board_now<W: World + ?Sized>(&mut self, w: &mut W, controller: Option<Obj>, candidate: Obj) -> bool {
        let unit = controller.and_then(|c| w.controller(c).unit);
        let body = VehicleAI::body_of_unit(&w.unit(candidate));
        let (Some(unit), Some(body)) = (unit, body) else { return false };
        let ok = match body {
            Body::Hull(h) => w.board(unit, h),
            Body::Turret(t) => w.man(unit, t),
        };
        if ok {
            self.write(|s| s.reservations.release(candidate, unit, SEAT));
        }
        ok
    }

    /// boardPathClear(controller, candidate): no eligible enemy within 8 m.
    fn board_path_clear<W: World + ?Sized>(&self, w: &mut W, controller: Option<Obj>, candidate: Obj) -> bool {
        let Some(unit) = controller.and_then(|c| w.controller(c).unit) else { return true };
        let team = w.unit(unit).team;
        let at = w.unit(candidate).position;
        let mut found = w.query(&at, 8.0, QueryFilter::EnemiesOf { unit, team });
        found.truncate(30);
        found.is_empty()
    }

    /// relation(a, b).
    fn relation<W: World + ?Sized>(&self, w: &mut W, a: f64, b: f64) -> f64 {
        w.relation(a, b)
    }

    /// boardPolicy(agent, candidate): a moving candidate always; a
    /// stationary one with a remembered target it could reach, or, with
    /// StayInTurrets, for any role but the repair roles.
    fn board_policy<W: World + ?Sized>(&self, w: &mut W, agent: Obj, controller: Obj, candidate: Obj) -> bool {
        let entry = w.unit(candidate);
        let body = VehicleAI::body_of_unit(&entry);
        if self.nominal_speed(w, &entry) > 0.0 {
            return true;
        }
        if !w.settings().stay_in_turrets {
            let Some(memory) = w.memory_point(agent) else { return false };
            return self.body_reach(w, body, &memory);
        }
        let role = w.controller(controller).unit.and_then(|u| w.unit(u).class).map_or(controllers::ROLE_OTHER, |c| c.role);
        role != controllers::ROLE_SUPPORT && role != controllers::ROLE_PILOT
    }

    /// nominalSpeed(candidate): its class's MaxSpeed (walkers, flyers) or
    /// ForwardSpeed; a turret's 0.
    fn nominal_speed<W: World + ?Sized>(&self, w: &W, candidate: &UnitView) -> f64 {
        let Some(Body::Hull(h)) = VehicleAI::body_of_unit(candidate) else { return 0.0 };
        let hull = w.hull(h);
        let n = if hull.flying || hull.walking { hull.max_speed_setting } else { hull.forward_speed_setting };
        if n.is_finite() {
            n
        } else {
            hull.speed
        }
    }

    /// targetReserved(target, team) / reserveTarget(target, unit): flyer
    /// target reservations (type 2).
    fn target_reserved<W: World + ?Sized>(&mut self, w: &W, target: Obj, team: f64) -> bool {
        self.reservation_count(w, target, team, 2.0) > 0.0
    }

    fn reserve_target<W: World + ?Sized>(&mut self, w: &W, target: Obj, unit: Obj) {
        self.reservation_reserve(w, target, unit, 2.0);
    }

    /// mountReach(unit, point): one of the control's mounts can bring its
    /// weapon to bear on `point` by angle alone.
    fn mount_reach<W: World + ?Sized>(&self, w: &mut W, unit: Option<Obj>, point: &Vector3) -> bool {
        let lease = VehicleAI::lease_of(w, unit).or_else(|| unit.and_then(|u| w.unit(u).lease).map(|l| w.lease(l)));
        let Some(lease) = lease else { return true };
        let origin = Origin::Lease(lease.obj);
        mount_angles(w, lease.vehicle, lease.turret, lease.index, origin, point)
    }

    /// bodyReach(body, point): any mount of a candidate reaches it.
    fn body_reach<W: World + ?Sized>(&self, w: &mut W, body: Option<Body>, point: &Vector3) -> bool {
        match body {
            None => false,
            Some(Body::Turret(t)) => mount_angles(w, None, Some(t), 0.0, Origin::TurretPivot(t), point),
            Some(Body::Hull(h)) => {
                let seats = w.hull(h).seats;
                for (i, seat) in seats.iter().enumerate() {
                    if seat.weapons == 0 {
                        continue;
                    }
                    if mount_angles(w, Some(h), None, i as f64, Origin::SeatOrigin(h, i), point) {
                        return true;
                    }
                }
                false
            }
        }
    }

    /// unreachableFromMount(controller, point): a zero-speed control that
    /// cannot turn to the point.
    fn unreachable_from_mount<W: World + ?Sized>(&self, w: &mut W, controller: Option<Obj>, point: &Vector3) -> bool {
        let Some(unit) = controller.and_then(|c| w.controller(c).unit) else { return false };
        let u = w.unit(unit);
        if u.lease.is_none() || patterns::forward_speed(&u) != 0.0 {
            return false;
        }
        !self.mount_reach(w, Some(unit), point)
    }

    /// sharedTarget(controller): an AI gun in a walker or hover a person
    /// drives fires at the person's target when its mount reaches it.
    fn shared_target<W: World + ?Sized>(&self, w: &mut W, controller: Option<Obj>) -> Option<Obj> {
        let unit = controller.and_then(|c| w.controller(c).unit);
        let lease = VehicleAI::lease_of(w, unit)?;
        let h = lease.vehicle?;
        let body = w.hull(h);
        if body.flying {
            return None;
        }
        let pilot = body.seats.first().and_then(|s| s.lease).map(|l| w.lease(l));
        let pilot = pilot.filter(|p| p.human)?;
        if pilot.obj == lease.obj {
            return None;
        }
        let target = w.human_target(h)?;
        let t = w.unit(target);
        let team = w.unit(unit.expect("a lease's unit")).team;
        if !t.alive || !(w.relation(team, t.team) < 0.0) {
            return None;
        }
        if mount_angles(w, lease.vehicle, lease.turret, lease.index, Origin::Lease(lease.obj), &t.position) {
            Some(target)
        } else {
            None
        }
    }

    /// dispatch(controller, unit, target, switchAllowed, world, out).
    fn dispatch<W: World + ?Sized>(&self, w: &mut W, controller: Obj, unit: Obj, target: Obj, switch_allowed: bool) -> Dispatch {
        let mut out = Dispatch::default();
        let u = w.unit(unit);
        let t = w.unit(target);
        let dense = w.settings().dense;
        patterns::vehicle_dispatch(w, controller, &u, &t, switch_allowed, dense, &mut out);
        out
    }

    /// enterPattern(patterns, state).
    fn enter_pattern<W: World + ?Sized>(&mut self, w: &mut W, p: &PatternsView, state: &mut (impl patterns::PatternAccess + ?Sized), pattern: f64) {
        let settings = w.settings().clone();
        patterns::enter_pattern(w, self, &settings, p, state, pattern);
    }

    /// tickPattern(patterns, state, dt).
    fn tick_pattern<W: World + ?Sized>(&mut self, w: &mut W, p: &PatternsView, state: &mut (impl patterns::PatternAccess + ?Sized), pattern: f64) {
        let settings = w.settings().clone();
        patterns::tick_pattern(w, self, &settings, p, state, pattern);
    }

    /// exitPattern(patterns, state).
    fn exit_pattern<W: World + ?Sized>(&self, w: &mut W, p: &PatternsView, pattern: f64) {
        patterns::exit_pattern(w, p, pattern);
    }

    /// sidestep(patterns).
    fn sidestep<W: World + ?Sized>(&self, w: &mut W, p: &PatternsView) -> bool {
        let per_second = self.cycles_per_second(w);
        patterns::sidestep(w, p, per_second)
    }

    /// isAtAt(unit).
    fn is_at_at<W: World + ?Sized>(&self, w: &W, unit: Obj) -> bool {
        patterns::is_at_at(w, &w.unit(unit))
    }

    /// patrolMounted(patrol): Patrol's mounted branch (state 1).
    fn patrol_mounted<W: World + ?Sized>(&self, w: &mut W, patrol: &PatrolView) -> bool {
        let controller = patrol.controller;
        let unit = w.controller(controller).unit;
        if patrol.support {
            let held = match (unit, patrol.post) {
                (Some(u), Some(post)) => {
                    let team = w.unit(u).team;
                    let post_team = w.post(post).team;
                    w.relation(team, post_team) > 0.0
                }
                _ => false,
            };
            if !held {
                let holds = w.support_holds(patrol.agent).unwrap_or(0.0) + 1.0;
                w.set_support_holds(patrol.agent, holds);
                if holds <= SUPPORT_HOLDS {
                    if patrol.helper {
                        w.helper_stop(patrol.agent);
                    }
                    let draw = w.int(20.0, 30.0);
                    let expires = w.now() + draw;
                    w.set_patrol_expires(patrol.obj, expires);
                    w.patrol_request_state(patrol.obj, 6.0);
                    return true;
                }
            }
            w.request_objective(controller, true);
            w.patrol_request_state(patrol.obj, DONE);
            return true;
        }
        let u = unit.map(|u| w.unit(u));
        if u.as_ref().is_some_and(|u| patterns::forward_speed(u) < 12.5) {
            let u = u.unwrap();
            let mut found = w.query(&u.position, 30.0, QueryFilter::LiveEnemiesOf { unit: u.obj, team: u.team });
            found.truncate(10);
            if found.iter().any(|&f| character_of(w, f)) {
                w.request_objective(controller, true);
                w.patrol_request_state(patrol.obj, DONE);
                return true;
            }
        }
        w.controller_request_exit(controller);
        let t = w.now() + 5.0;
        w.set_reboard_at(controller, t);
        w.patrol_request_state(patrol.obj, DONE);
        false
    }

    /// supportPoint(unit, post, helper, out): where a pilot fights for a
    /// post its hull cannot stand on, or None. `helper` is the agent whose
    /// movement helper the JavaScript hands in (None: none): its traversal
    /// mask and radius are read from it at each point the JavaScript reads
    /// `helper.mask` and `helper.radius`.
    fn support_point<W: World + ?Sized>(&mut self, w: &mut W, unit: Option<Obj>, post: Option<Obj>, helper: Option<Obj>) -> Option<Vector3> {
        let (Some(unit), Some(post)) = (unit, post) else { return None };
        if !w.has_graph() {
            return None;
        }
        let helper = helper?;
        if !(w.helper_traversal(helper, false) > 0.0) || !(w.helper_traversal(helper, true) >= 0.0) {
            return None;
        }
        let u = w.unit(unit);
        let seated_hull = u.seat.and_then(|l| w.lease(l).vehicle).is_some();
        if !seated_hull || (u.kind != kind::WALKER && u.kind != kind::HOVER) {
            return None;
        }
        let center = w.cp_center(post);
        let region = w.capture_region(post);
        let reach = max(region.map_or(3.0, |r| r.0), region.map_or(3.0, |r| r.1)) + REACH_MARGIN;
        let radius = w.helper_traversal(helper, true);
        let mask = w.helper_traversal(helper, false);
        if !out_of_reach(w.nav().map(|n| &mut n.barriers), &center, reach, radius, v8math::to_uint32(mask)) {
            return None;
        }
        // Publish the post's cache before callbacks that may inspect or
        // populate it, just as supportPoint creates its Map first.
        self.write(|s| {
            if !s.support_cache.iter().any(|(p,_)| *p == post) { s.support_cache.push((post,Vec::new())); }
        });
        let key_mask = w.helper_traversal(helper, false);
        let key_radius = w.helper_traversal(helper, true);
        let key = format!("{}/{}", bfe_content::js::number_to_string(key_mask), to_fixed_1(key_radius));
        let cached = self.read(|s| s.support_cache.iter().find(|(p, _)| *p == post).and_then(|(_, m)| m.iter().find(|(k, _)| *k == key)).map(|(_, h)| h.clone()));
        let hubs = match cached {
            Some(h) => h,
            None => {
                let origin = w.origin(unit);
                let eye = if origin.y.is_finite() { max(1.0, origin.y - u.position.y) } else { 3.0 };
                let radius = w.helper_traversal(helper, true);
                let mask = v8math::to_uint32(w.helper_traversal(helper, false));
                // supportHubs: the graph's and barriers' tests per hub, then
                // the line of sight to each in the same order (the queries
                // are navigation's content: only the sight tests are the
                // battle's calls).
                let candidates = match w.nav() {
                    Some(nav) => {
                        let crate::ai::nav::Nav { graph, barriers, .. } = nav;
                        support::support_candidates(graph.as_ref(), Some(barriers), &center, radius, mask)
                    }
                    None => Vec::new(),
                };
                let target = support::post_target(&center);
                let mut hubs = Vec::new();
                for c in candidates {
                    let eye_at = Vector3::new(c.hub_at.x, c.hub_at.y + eye, c.hub_at.z);
                    if w.los(&eye_at, &target) == Some(false) {
                        continue;
                    }
                    hubs.push(SupportHub { hub: c.hub, distance: c.distance });
                }
                support::sort_by_distance(&mut hubs);
                self.write(|s| match s.support_cache.iter_mut().find(|(p, _)| *p == post) {
                    Some((_, m)) => match m.iter_mut().find(|(k,_)| *k==key) { Some((_,value))=>*value=hubs.clone(), None=>m.push((key,hubs.clone())) },
                    None => s.support_cache.push((post, vec![(key, hubs.clone())])),
                });
                hubs
            }
        };
        let mask = v8math::to_uint32(w.helper_traversal(helper, false));
        let near = {
            let graph = w.nav().and_then(|n| n.graph.as_ref())?;
            support::pick_candidates(graph, &hubs, &u.position, mask)?
        };
        let pick = if near.len() > 1 { near[w.int(0.0, (near.len() - 1) as f64) as usize] } else { near[0] };
        let graph = w.nav().and_then(|n| n.graph.as_ref())?;
        let hub = &graph.hubs[pick.hub];
        Some(Vector3::new(hub.x, hub.y, hub.z))
    }

    /// holdHint(hint, controller): the hint reserved for the controller now.
    fn hold_hint<W: World + ?Sized>(&self, w: &mut W, hint: Obj, controller: Obj) {
        let now = w.now();
        w.reserve_hint(hint, controller, now);
    }

    /// closeHint(hint, until): every hint of its ring taken until `until`.
    fn close_hint<W: World + ?Sized>(&self, w: &mut W, hint: Obj, until: f64) {
        let mut h = hint;
        loop {
            w.set_hint_reserved_at(h, until - 4.0);
            match w.hint_link(h) {
                Some(next) if next != hint => h = next,
                _ => break,
            }
        }
    }

    /// disembarkPassengers(controller, post): every AI aboard but the pilot
    /// gets out, taking up `post`.
    fn disembark_passengers<W: World + ?Sized>(&self, w: &mut W, controller: Option<Obj>, post: Option<Obj>) -> f64 {
        let Some(h) = VehicleAI::controller_lease(w, controller).and_then(|l| l.vehicle) else { return 0.0 };
        let seats = w.hull(h).seats;
        let mut n = 0.0;
        for seat in seats.iter().skip(1) {
            let Some(other) = seat.lease.map(|l| w.lease(l)) else { continue };
            if other.human {
                continue;
            }
            let character = other.unit.and_then(|u| w.unit(u).character);
            if let (Some(p), Some(c)) = (post, character) {
                w.set_objective(c, p);
            }
            if w.unboard(other.unit, ExitOptions { post: Some(post), promote: Some(true), placement: Some(true) }) {
                n += 1.0;
            }
        }
        n
    }

    /// destroyStuck(controller): a stranded flyer is destroyed.
    fn destroy_stuck<W: World + ?Sized>(&self, w: &mut W, controller: Option<Obj>) {
        if let Some(h) = VehicleAI::controller_lease(w, controller).and_then(|l| l.vehicle) {
            w.damage(h, f64::INFINITY);
        }
    }

    /// abandonPad(controller): a craft its pilot cannot lift off its pad:
    /// every bot aboard gets out, none boards for 15 s, none is invited for 60.
    fn abandon_pad<W: World + ?Sized>(&mut self, w: &mut W, controller: Obj) -> f64 {
        let Some(lease) = VehicleAI::controller_lease(w, Some(controller)) else { return 0.0 };
        let Some(h) = lease.vehicle else { return 0.0 };
        let now = w.now();
        self.write(|s| match s.pad_left.iter_mut().find(|(b, _)| *b == h) {
            Some(entry) => entry.1 = now + 60.0,
            None => s.pad_left.push((h, now + 60.0)),
        });
        let seats = w.hull(h).seats;
        let mut n = 0.0;
        for seat in seats.iter().rev() {
            let Some(other) = seat.lease.map(|l| w.lease(l)) else { continue };
            let network = other.unit.is_some_and(|u| w.unit(u).network_human);
            if other.obj == lease.obj || other.human || network {
                continue;
            }
            let crew = other.unit.and_then(|u| w.unit(u).character).and_then(|c| w.character_controller(c)).or_else(|| other.controllable.and_then(|c| w.unit(c).controller));
            if let Some(crew) = crew {
                w.set_reboard_at(crew, now + 15.0);
            }
            if w.unboard(other.unit, ExitOptions { post: None, promote: Some(false), placement: Some(true) }) {
                n += 1.0;
            }
        }
        w.set_reboard_at(controller, now + 15.0);
        if w.request_exit_vehicle(lease.unit, ExitOptions { post: None, promote: Some(false), placement: None }) {
            n += 1.0;
        }
        n
    }

    /// updateDecay(dt): the unattended-vehicle service, once a step: a hull
    /// outside the map's boundary, or one that has lost its team on a pad
    /// linked to a post, with nobody aboard, gathers idle time, a sample a
    /// second (a person who could board it near pauses the count); past 20
    /// samples it loses MaxHealth / DecayTime a sample.
    fn update_decay<W: World + ?Sized>(&self, w: &mut W, dt: f64) {
        for h in w.hulls() {
            let v = w.hull(h);
            if !v.alive {
                continue;
            }
            // `v.decay ||= { idle: 0, sample: 0 }`.
            let mut st = w.read_decay(h).unwrap_or_default();
            let inside = !w.nav().is_some_and(|nav| nav.outside(crate::ai::nav::math::Vec3::new(v.position.x, v.position.y, v.position.z)));
            let damage = (|| {
                if inside && (!v.slot_post || v.controlling_team != 0.0) {
                    st.idle = 0.0;
                    return None;
                }
                if v.occupied {
                    st.idle = 0.0;
                    return None;
                }
                st.sample -= dt;
                if !(st.sample < 0.0) {
                    return None;
                }
                st.sample += 1.0;
                Some(())
            })();
            if damage.is_none() {
                w.write_decay(h, st);
                continue;
            }
            w.write_decay(h, st);
            if self.player_near(w, &v) {
                continue;
            }
            st.idle += 1.0;
            w.write_decay(h, st);
            if st.idle > 20.0 {
                let amount = v.max_health / v.decay_time.unwrap_or(10.0);
                w.damage(h, amount);
            }
        }
    }

    /// playerNear(v): a person who could board it within its radius and
    /// his, plus 10 m.
    fn player_near<W: World + ?Sized>(&self, w: &mut W, v: &super::world::HullView) -> bool {
        for unit in w.human_units() {
            let u = w.unit(unit);
            if !u.alive {
                continue;
            }
            let team = v.controlling_team;
            if team > 0.0 && !(w.relation(team, u.team) > 0.0) {
                continue;
            }
            let reach = v.bound_radius + w.sphere(unit).radius + 10.0;
            if u.position.distance_to_squared(&v.position) <= reach * reach {
                return true;
            }
        }
        false
    }

    /// reachable(entry): soldiers can reach where the vehicle sits (the
    /// barriers clear at 2 m, then at its own traversal radius).
    fn reachable<W: World + ?Sized>(&self, w: &mut W, entry: Obj) -> bool {
        let e = w.unit(entry);
        let r = match e.vehicle {
            Some(h) => {
                let size = w.hull(h).size;
                hypot(size.x, size.z) * if e.kind == kind::HOVER { 0.5 } else { 0.25 }
            }
            None => 0.0,
        };
        let p = crate::ai::nav::math::Vec3::new(e.position.x, e.position.y, e.position.z);
        let Some(nav) = w.nav() else { return true };
        if !nav.barriers.point_clear(p, 2.0, 1).0 {
            return false;
        }
        r <= 0.0 || nav.barriers.point_clear(p, r, 1).0
    }
}

fn character_of<W: World + ?Sized>(w: &W, u: Obj) -> bool {
    let unit = w.unit(u);
    if unit.character.is_some() {
        return true;
    }
    if let Some(h) = unit.vehicle {
        for seat in w.hull(h).seats {
            if let Some(l) = seat.lease {
                if w.lease(l).unit.is_some_and(|x| w.unit(x).character.is_some()) {
                    return true;
                }
            }
        }
    }
    unit.turret.and_then(|t| w.turret(t).lease).and_then(|l| w.lease(l).unit).is_some_and(|x| w.unit(x).character.is_some())
}

/// `n.toFixed(1)` (the support cache's key): n rounded exactly to tenths,
/// a tie to the larger, "-" before a negative (but not -0).
pub fn to_fixed_1(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    let (sign, x) = if x < 0.0 { ("-", -x) } else { ("", x) };
    if x >= 1e21 {
        return format!("{sign}{}", bfe_content::js::number_to_string(x));
    }
    // x = mantissa * 2^exponent exactly; n = round(x * 10), ties up.
    let bits = x.to_bits();
    let raw_exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    let (mantissa, exponent) = if raw_exp == 0 { (frac, -1074) } else { (frac | (1u64 << 52), raw_exp - 1075) };
    let num = mantissa as u128 * 10;
    let n: u128 = if exponent >= 0 {
        num << exponent
    } else {
        let k = -exponent as u32;
        if k > 100 {
            0
        } else {
            let q = num >> k;
            let rem = num - (q << k);
            if rem * 2 >= (1u128 << k) {
                q + 1
            } else {
                q
            }
        }
    };
    let digits = n.to_string();
    let digits = if digits.len() < 2 { format!("{}{}", "0".repeat(2 - digits.len()), digits) } else { digits };
    let (a, b) = digits.split_at(digits.len() - 1);
    format!("{sign}{a}.{b}")
}

/// footprint(v) (index.js): a hull's collision box from above: its centre,
/// forward and half extents across and along.
struct Footprint {
    cx: f64,
    cz: f64,
    fx: f64,
    fz: f64,
    hx: f64,
    hz: f64,
}

fn footprint(v: &super::world::HullView) -> Footprint {
    let (min_b, max_b) = (v.bounds_min, v.bounds_max);
    let yaw = v.yaw;
    let fx = sin(yaw);
    let fz = cos(yaw);
    let lx = (min_b.x + max_b.x) / 2.0;
    let lz = (min_b.z + max_b.z) / 2.0;
    Footprint {
        cx: v.position.x + fz * lx + fx * lz,
        cz: v.position.z - fx * lx + fz * lz,
        fx,
        fz,
        hx: (max_b.x - min_b.x) / 2.0,
        hz: (max_b.z - min_b.z) / 2.0,
    }
}

/// footprintsTouch(a, b, margin) (index.js): separating axes.
fn footprints_touch(a: &super::world::HullView, b: &super::world::HullView, margin: f64) -> bool {
    let fa = footprint(a);
    let fb = footprint(b);
    let dx = fb.cx - fa.cx;
    let dz = fb.cz - fa.cz;
    for (ax, az) in [(fa.fx, fa.fz), (fa.fz, -fa.fx), (fb.fx, fb.fz), (fb.fz, -fb.fx)] {
        let ra = fa.hz * abs(fa.fx * ax + fa.fz * az) + fa.hx * abs(fa.fz * ax - fa.fx * az);
        let rb = fb.hz * abs(fb.fx * ax + fb.fz * az) + fb.hx * abs(fb.fz * ax - fb.fx * az);
        if abs(dx * ax + dz * az) > ra + rb + margin {
            return false;
        }
    }
    true
}

/// mountAngles(lease, point) (index.js): the angular reach of one mount:
/// the point's yaw and pitch in the body's frame from the mount's pivot,
/// inside the mount's limits.
pub fn mount_angles<W: World + ?Sized>(w: &mut W, vehicle: Option<Obj>, turret: Option<Obj>, index: f64, origin: Origin, point: &Vector3) -> bool {
    if vehicle.is_none() && turret.is_none() {
        return true;
    }
    let (yaw_setting, pitch_setting) = match (vehicle, turret) {
        (Some(h), _) => {
            let hull = w.hull(h);
            let seat = if index >= 0.0 && (index as usize) < hull.seats.len() && v8math::trunc(index) == index { Some(hull.seats[index as usize].clone()) } else { None };
            (seat.as_ref().and_then(|s| s.yaw_limits.clone()), seat.as_ref().and_then(|s| s.pitch_limits.clone()))
        }
        (None, Some(t)) => {
            let turret = w.turret(t);
            (turret.yaw_limits_setting, turret.pitch_limits_setting)
        }
        _ => unreachable!(),
    };
    let yaw_limits = degrees(yaw_setting.as_deref(), [-180.0, 180.0]);
    let pitch_limits = degrees(pitch_setting.as_deref(), if vehicle.is_some() { [-90.0, 90.0] } else { [-45.0, 45.0] });
    let o = w.mount_origin(origin);
    let mut q = Vector3::default();
    q.sub_vectors(point, &o);
    let length = q.length();
    if length < 1e-9 {
        return true;
    }
    let yaw = match vehicle {
        Some(h) => w.hull(h).yaw,
        None => w.turret(turret.unwrap()).facing,
    };
    // The body's frame: the 2004 game's right row and forward.
    let x = q.x * cos(yaw) - q.z * sin(yaw);
    let z = q.x * sin(yaw) + q.z * cos(yaw);
    let a = atan2(x, z);
    let p = asin(max(-1.0, min(1.0, q.y / length)));
    if let (Some(t), None) = (turret, vehicle) {
        let tv = w.turret(t);
        if let Some(limits) = tv.pitch_limits {
            let held = if tv.yaw_limits.is_some() && tv.has_hold_yaw {
                let s = sin(w.hold_yaw(t, a) - a);
                let c = cos(w.hold_yaw(t, a) - a);
                abs(atan2(s, c)) < 1e-6
            } else {
                true
            };
            return held && p >= limits[0] - 1e-6 && p <= limits[1] + 1e-6;
        }
    }
    a >= yaw_limits[0] && a <= yaw_limits[1] && p >= pitch_limits[0] && p <= pitch_limits[1]
}

/// StationaryHelper (index.js): a movement helper for controls that cannot
/// move: every goto is accepted and goes nowhere; the request stays empty.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StationaryHelper {
    pub arrived: bool,
    pub failed: bool,
    pub status: f64,
    pub has_path: bool,
    pub stopped: bool,
}

impl Default for StationaryHelper {
    fn default() -> Self {
        StationaryHelper { arrived: true, failed: false, status: 0.0, has_path: false, stopped: true }
    }
}

impl StationaryHelper {
    /// goto() and stop(): `controller.request.valid = false`.
    pub fn goto<W: World + ?Sized>(&self, w: &mut W, controller: Obj) {
        w.invalidate_request(controller);
    }
    pub fn stop<W: World + ?Sized>(&self, w: &mut W, controller: Obj) {
        w.invalidate_request(controller);
    }
    /// destinationPoint(out): the control's own place.
    pub fn destination_point<W: World + ?Sized>(&self, w: &W, controller: Obj) -> Option<Vector3> {
        w.controller(controller).unit.map(|u| w.unit(u).position)
    }
}

/// steering's pure parts, for callers holding a control's view.
pub use steering::{arrival_radius, nonlinear_yaw, translate};
/// support's pure parts.
pub use support::{hubs_at, routes};
/// The unused steerState accessor, for tests and debugging.
pub fn steer_of<W: World + ?Sized>(w: &mut W, control: Obj) -> super::world::Steer {
    steer_state(w, control, |st| *st)
}
