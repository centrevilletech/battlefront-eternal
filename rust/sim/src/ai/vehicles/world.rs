//! What the vehicle AI reads of the battle and asks of it: the views of the
//! objects it is handed or looks up (a control a bot drives, a hull, a
//! turret, a seat's lease, a unit, a controller and its agent), the state
//! of its own it keeps on them (a control's steering bag, a hull's decay,
//! an attack pattern's vehicle state), and [`World`], every call it makes
//! across its boundary.
//!
//! The JavaScript reaches these as live objects; the Rust names each by an
//! [`Obj`], the caller's identity token for the object (a unit, a hull, a
//! registry entry, a lease, a controller, a post, a hint), and reads it
//! through the trait's views. Where the JavaScript's `?.` or `??` can find
//! a field missing the view has an `Option` (`None`: undefined or null).
//!
//! The calls are the methods src/ai/classic/vehicles/world.js holds (other
//! packages' module functions and reads worked out when asked) and the
//! methods of live objects the vehicle AI calls (the collision world, the
//! registry, the navigation, the seats, the controllers, helpers and
//! trackers), each as the JavaScript makes it; the capture's labels
//! (tools/sim-capture/vehicle-ai.mjs) name them the same.

use three::Vector3;
use std::rc::Rc;
use crate::ai::owned::scratch::List;

use super::random::Draws;
use crate::vehicles::{BodyId, Hit};

/// A JavaScript object the vehicle AI names: the caller's token for it,
/// compared where the JavaScript compares objects with `===`.
pub type Obj = u64;

/// `{ unit, generation }`: a weak reference (reservations, the orbit's
/// target, the notifier's notes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ref {
    pub obj: Obj,
    pub generation: f64,
}

/// The classifiers (src/ai/classic/nav/units.js KIND). Kinds stay f64:
/// the JavaScript compares them with `===` against these numbers.
pub mod kind {
    pub const SOLDIER: f64 = 0.0;
    pub const DROID: f64 = 1.0;
    pub const WALKER: f64 = 2.0;
    pub const DROIDEKA: f64 = 3.0;
    pub const ANIMAL: f64 = 4.0;
    pub const FLYER: f64 = 5.0;
    pub const HOVER: f64 = 6.0;
    pub const MOUNTED: f64 = 7.0;
    pub const ARMED_BUILDING: f64 = 8.0;
    pub const BUILDING: f64 = 9.0;
}

/// A unit's class as the vehicle AI reads it (`unit.classDef`): for a
/// control, src/seats.js vehicleClassView (or a gunner's or a turret's
/// view, with no speed); for a soldier, its class data. A field the class
/// lacks is `None` (the JavaScript's undefined).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassView {
    /// `AISizeType`, as the string it is (a number written as JavaScript's
    /// String() would).
    pub ai_size_type: Option<Rc<str>>,
    pub forward_speed: Option<f64>,
    pub strafe_speed: Option<f64>,
    pub turn_rate: Option<f64>,
    pub pitch_rate: Option<f64>,
    /// `MovingTurnOnly` (true or false where the view sets it).
    pub moving_turn_only: Option<bool>,
    pub max_speed: Option<f64>,
    pub min_speed: Option<f64>,
    pub max_turn_speed: Option<f64>,
    pub max_pitch_speed: Option<f64>,
    pub max_yaw_speed: Option<f64>,
    /// `VehicleType` (lower case where the view made it; may be null).
    pub vehicle_type: Option<Rc<str>>,
    pub landing_time: Option<f64>,
    pub landing_speed: Option<f64>,
    pub landed_height: Option<f64>,
    /// `bounds`: [[min], [max]] when it is there.
    pub bounds: Option<[[f64; 3]; 2]>,
    /// `NoEnterVehicles` as Number.parseFloat reads it (`?? 0` first).
    pub no_enter_vehicles: f64,
    /// roleOf(classDef) (src/ai/classic/controller.js): the class's role.
    pub role: f64,
}

/// A unit, a control or a registry entry, as the vehicle AI reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnitView {
    pub obj: Obj,
    pub kind: f64,
    pub id: f64,
    pub generation: f64,
    pub alive: bool,
    pub team: f64,
    pub position: Vector3,
    pub velocity: Vector3,
    /// `unit.bodyYaw ?? unit.soldier?.aimYaw ?? 0`.
    pub body_yaw: f64,
    pub class: Option<ClassView>,
    /// `unit.className` (a string, else None).
    pub class_name: Option<Rc<str>>,
    /// `unit.soldier` is there.
    pub soldier: bool,
    /// `unit.soldier?.maxSpeed`.
    pub soldier_max_speed: Option<f64>,
    pub walker: bool,
    pub human: bool,
    pub network_human: bool,
    /// `u.targetable !== false`.
    pub targetable: bool,
    /// `unit.seat`: the lease it holds (a soldier) or its own (a control).
    pub seat: Option<Obj>,
    /// `unit.lease` (a control's own lease).
    pub lease: Option<Obj>,
    /// `unit.vehicle` / `unit.turret` (a control's or an entry's body).
    pub vehicle: Option<Obj>,
    pub turret: Option<Obj>,
    pub controller: Option<Obj>,
    pub character: Option<Obj>,
    /// `unit.upY` (a flyer's up, pattern 14).
    pub up_y: Option<f64>,
    /// isAtAt's reads where the unit has no hull: `(unit.def ??
    /// unit.classDef)`'s `label ?? settings?.Label`, and `unit.name`.
    pub own_label: Option<Rc<str>>,
    pub own_name: Option<Rc<str>>,
}

/// A seat of a hull as the vehicle AI reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeatView {
    pub prepared_limits:Option<[[f64;2];2]>,
    pub lease: Option<Obj>,
    /// `seat.weapons.length`.
    pub weapons: usize,
    /// The seat's `YawLimits` and `PitchLimits` settings, as written
    /// (None: undefined).
    pub yaw_limits: Option<Rc<str>>,
    pub pitch_limits: Option<Rc<str>>,
}

/// A hull (src/vehicles.js Vehicle) as the vehicle AI reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HullView {
    pub obj: Obj,
    /// Its registry entry (`aiTarget`).
    pub entry: Obj,
    pub alive: bool,
    pub name: Rc<str>,
    pub flying: bool,
    pub walking: bool,
    pub grounded: bool,
    pub flight_state: Option<Rc<str>>,
    pub gear: f64,
    pub speed: f64,
    /// `def.settings.MaxSpeed` / `ForwardSpeed` as Number.parseFloat reads them.
    pub max_speed_setting: f64,
    pub forward_speed_setting: f64,
    /// `def.label ?? def.settings?.Label ?? null`.
    pub label: Option<Rc<str>>,
    pub position: Vector3,
    pub velocity: Vector3,
    pub yaw: f64,
    pub quaternion: three::Quaternion,
    pub matrix: three::Matrix4,
    pub bounds_min: Vector3,
    pub bounds_max: Vector3,
    pub bound_radius: f64,
    pub size: Vector3,
    pub body: Option<BodyId>,
    pub seats: Vec<SeatView>,
    pub occupants: f64,
    pub driver: bool,
    pub free_seats: f64,
    pub occupied: bool,
    pub controlling_team: f64,
    pub tow_target: f64,
    /// `v.slot?.desc?.post` is truthy.
    pub slot_post: bool,
    /// `v.slot?.desc?.decayTime`.
    pub decay_time: Option<f64>,
    pub max_health: f64,
    /// `body.system === this.system` (a hull of the battle's own system).
    pub own_system: bool,
}

/// A turret (src/props.js Turret) as the vehicle AI reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurretView {
    pub prepared_limits:Option<[[f64;2];2]>,
    pub obj: Obj,
    pub entry: Option<Obj>,
    pub alive: bool,
    pub gunner: bool,
    pub lease: Option<Obj>,
    pub facing: f64,
    /// `desc.settings` YawLimits / PitchLimits as written.
    pub yaw_limits_setting: Option<Rc<str>>,
    pub pitch_limits_setting: Option<Rc<str>>,
    /// `pitchLimits` when it is an array, and `yawLimits` (its arc).
    pub pitch_limits: Option<[f64; 2]>,
    pub yaw_limits: Option<[f64; 2]>,
    /// `typeof holdYaw === "function"`.
    pub has_hold_yaw: bool,
}

/// A seat's lease (src/seats.js SeatLease) as the vehicle AI reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LeaseView {
    pub obj: Obj,
    pub released: bool,
    pub human: bool,
    pub live: bool,
    pub unit: Option<Obj>,
    pub controllable: Option<Obj>,
    pub vehicle: Option<Obj>,
    pub turret: Option<Obj>,
    pub index: f64,
    pub team: f64,
}

/// What a control's steering reads of the controller driving it
/// (src/ai/classic/controller.js) and of its agent's movement helper.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControllerView {
    pub obj: Obj,
    pub unit: Option<Obj>,
    /// `controller.navControl` is there, and its executor (an object).
    pub nav_control: bool,
    pub executor: bool,
    /// `controller.agent`.
    pub agent: Option<Obj>,
    /// `controller.executorStatus`.
    pub executor_status: f64,
    /// `controller.target` is truthy.
    pub target: bool,
    /// The agent's helper: whether there is one, its `hasPath`, `state`
    /// and `destination` (the walker's replan reads them).
    pub helper: bool,
    pub helper_has_path: bool,
    pub helper_state: Option<f64>,
    pub helper_destination: Option<Vector3>,
    pub is_unit_controller: bool,
    /// `perception.fire.channel` (the tow pass compares it).
    pub fire_channel: Option<f64>,
}

/// The battle's AI settings the vehicle AI reads (the mission's controls,
/// the difficulty's cycle): they do not change during a battle.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// `flyHeight.min ?? -200`, `flyHeight.max ?? 200`.
    pub min_fly_height: f64,
    pub max_fly_height: f64,
    /// `vehicleNotifyRadius ?? 50`.
    pub notify_radius: f64,
    /// Boolean(dense?.value ?? dense).
    pub dense: bool,
    /// ai.stayInTurrets().
    pub stay_in_turrets: bool,
    /// The scheduler's cycle (15, or 3 on Hard).
    pub cycle: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { min_fly_height: -200.0, max_fly_height: 200.0, notify_radius: 50.0, dense: false, stay_in_turrets: false, cycle: 15.0 }
    }
}

/// A movement request (the controller's, a control's, or one the vehicle
/// AI writes: the hover's avoidance copy, the orbit's mode 3), with the
/// orbit's height share and movement scale where it has them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Request {
    pub valid: bool,
    pub steering_target: Vector3,
    pub waypoint: Vector3,
    pub segment_direction: Vector3,
    pub mode: f64,
    pub lower: f64,
    pub upper: f64,
    pub blend: Option<f64>,
    pub throttle: Option<f64>,
}

impl Default for Request {
    fn default() -> Self {
        Request {
            valid: false,
            steering_target: Vector3::default(),
            waypoint: Vector3::default(),
            segment_direction: Vector3::default(),
            mode: 0.0,
            lower: -1.0,
            upper: 1.0,
            blend: None,
            throttle: None,
        }
    }
}

/// The orbit request a pattern leaves on a control (patterns.js
/// orbitRequest): `target` the tracker's target, when there is one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    pub target: Option<Ref>,
    pub offset: Vector3,
    pub radius: f64,
    pub factor: f64,
}

/// Where a walker's last recovery ended (`{ x, z }`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StuckAt {
    pub x: f64,
    pub z: f64,
}

/// A control's steering bag (steering.js steerState: `unit.steer`), each
/// field `None` where the bag lacks it: a flyer's pilot input makes the
/// bag `{}` when it has none (src/flight/ai.js `unit.steer ||= {}`), and
/// the fields are written one by one from then on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Steer {
    pub moving_turn_at: Option<f64>,
    pub loop_: Option<bool>,
    pub loop_at: Option<f64>,
    pub min_speed: Option<f64>,
    pub min_speed_at: Option<f64>,
    /// `orbit`: Some(None) is null, None undefined.
    pub orbit: Option<Option<Orbit>>,
    pub land: Option<bool>,
    pub path_override: Option<bool>,
    pub path_altitude: Option<bool>,
    pub throttle: Option<f64>,
    pub recovery: Option<f64>,
    pub recovery_since: Option<f64>,
    pub last_recovery: Option<f64>,
    pub saved_forward: Option<Vector3>,
    pub hold_since: Option<f64>,
    pub holding: Option<bool>,
    pub turn_yaw: Option<f64>,
    pub turn_at: Option<f64>,
    pub walk_since: Option<f64>,
    /// `stuckAt`: Some(None) is null.
    pub stuck_at: Option<Option<StuckAt>>,
    pub stuck_count: Option<f64>,
}

impl Steer {
    /// steerState's bag (steering.js:49).
    pub fn full() -> Steer {
        Steer {
            moving_turn_at: Some(f64::NEG_INFINITY),
            loop_: Some(false),
            loop_at: Some(f64::NEG_INFINITY),
            min_speed: Some(0.0),
            min_speed_at: Some(f64::NEG_INFINITY),
            orbit: Some(None),
            land: Some(false),
            path_override: Some(false),
            path_altitude: Some(false),
            throttle: Some(1.0),
            recovery: Some(0.0),
            recovery_since: Some(0.0),
            last_recovery: Some(f64::NEG_INFINITY),
            saved_forward: Some(Vector3::default()),
            hold_since: Some(-100.0),
            holding: Some(false),
            turn_yaw: Some(f64::NAN),
            turn_at: Some(0.0),
            walk_since: Some(f64::NAN),
            stuck_at: Some(None),
            stuck_count: Some(0.0),
        }
    }

    /// The pilot input's `{}`.
    pub fn empty() -> Steer {
        Steer {
            moving_turn_at: None,
            loop_: None,
            loop_at: None,
            min_speed: None,
            min_speed_at: None,
            orbit: None,
            land: None,
            path_override: None,
            path_altitude: None,
            throttle: None,
            recovery: None,
            recovery_since: None,
            last_recovery: None,
            saved_forward: None,
            hold_since: None,
            holding: None,
            turn_yaw: None,
            turn_at: None,
            walk_since: None,
            stuck_at: None,
            stuck_count: None,
        }
    }

    /// `steer.orbit` (undefined and null alike).
    pub fn orbit(&self) -> Option<Orbit> {
        self.orbit.flatten()
    }
}

/// A number field as JavaScript reads it: undefined is NaN in arithmetic.
pub fn num(x: Option<f64>) -> f64 {
    x.unwrap_or(f64::NAN)
}

/// The vehicle fields a control's analogs write into its intent
/// (steering.js seatAnalogs).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SeatIntent {
    pub up: f64,
    pub down: f64,
    pub brake: bool,
    pub trick: bool,
    pub immelmann: bool,
    pub boost: bool,
}

/// The analogs of one input step (`out`, and the vehicle fields of
/// steering.js's `full`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stick {
    pub forward: f64,
    pub right: f64,
    pub turn: f64,
    pub pitch: f64,
    pub up: f64,
    pub down: f64,
    pub brake: bool,
    pub trick: bool,
    pub immelmann: bool,
    pub boost: bool,
}

/// What the pilot input reads of the steering's environment (index.js
/// analogs' `env`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PilotEnv {
    pub has_path: bool,
    pub min_fly_height: f64,
    pub max_fly_height: f64,
}

/// A bounding sphere as `unit.sphere` gives it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sphere {
    pub center: Vector3,
    pub radius: f64,
}

/// The weapon choice's answer (weapons-ai.js selectWeapon's `out`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Choice {
    pub ok: bool,
    pub action: f64,
    pub in_band: bool,
    /// The weapon chosen (its token; None: null).
    pub weapon: Option<Obj>,
}

/// The query filters the vehicle AI hands units.query, each as its call
/// site writes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QueryFilter {
    /// boardPathClear: `u !== unit && u.alive && relation(unit.team, u.team) < 0 && u.targetable !== false`.
    EnemiesOf { unit: Obj, team: f64 },
    /// patrolMounted: `u !== unit && u.alive && relation(unit.team, u.team) < 0`.
    LiveEnemiesOf { unit: Obj, team: f64 },
    /// The notifier: `u !== entry && relation(team, u.team) >= 0`.
    FriendsOf { entry: Obj, team: f64 },
}

/// A hub of the navigation graph (support.js reads its place and mask).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hub {
    pub index: usize,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub mask: u32,
}

/// graph.attach's answer: what a point stands on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Attach {
    None,
    Hub(usize),
    Arc(usize, usize),
}

/// A post as the vehicle AI reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PostView {
    pub obj: Obj,
    pub position: Vector3,
    pub team: f64,
}

/// A landing hint as the flyer agent reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HintView {
    pub obj: Obj,
    pub position: Vector3,
    pub forward: Vector3,
    pub up: Option<Vector3>,
    pub command_post: Option<Obj>,
}

/// A weapon of a control's arsenal as the tow pass reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponView {
    pub base: Option<Rc<str>>,
    pub channel: f64,
}

/// Options of a seat's exit or a getting out (`{ post, promote, placement }`):
/// a field the JavaScript's object leaves out is None; `post: Some(None)`
/// is `post: null`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ExitOptions {
    pub post: Option<Option<Obj>>,
    pub promote: Option<bool>,
    pub placement: Option<bool>,
}

/// The movement helper an agent of a control gets (createHelper): the
/// AI's own (None), a stationary control's, or a flyer's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelperKind {
    None,
    Stationary,
    Fly,
}

/// Where a mount's angles are measured from (mountAngles' `lease.origin`):
/// a lease's eye, a turret's gun pivot, a hull seat's eye.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Lease(Obj),
    TurretPivot(Obj),
    SeatOrigin(Obj, usize),
}

/// What the vehicle AI reads of the battle and asks of it. Reads take
/// `&self`; calls across the boundary `&mut self`, in the order the
/// JavaScript makes them; generator A's draws are [`Draws`].
pub trait World: Draws {
    // ---- the battle's clock and settings
    /// `world.time ?? 0`.
    fn now(&self) -> f64;
    fn settings(&self) -> &Settings;

    // ---- views
    fn unit(&self, u: Obj) -> UnitView;
    fn hull(&self, h: Obj) -> HullView;
    /// Native hosts override these with direct F2 seat reads. Replay views
    /// retain their recorded seat table without imposing a live snapshot.
    fn hull_seat_count(&self, h: Obj) -> usize { self.hull(h).seats.len() }
    fn hull_seat(&self, h: Obj, index: usize) -> Option<SeatView> { self.hull(h).seats.get(index).cloned() }
    fn turret(&self, t: Obj) -> TurretView;
    fn lease(&self, l: Obj) -> LeaseView;
    fn controller(&self, c: Obj) -> ControllerView;
    fn post(&self, p: Obj) -> PostView;
    /// A Character's unit and controller (`character.unit`, `.controller`).
    fn character_unit(&self, character: Obj) -> Option<Obj>;
    fn character_controller(&self, character: Obj) -> Option<Obj>;
    /// The battle's hulls (Vehicles.vehicles), in order.
    fn object_buffer(&self) -> List<Obj> { List::default() }
    fn hulls(&self) -> List<Obj>;
    /// The battle's turrets (TurretSeats.turrets), in order.
    fn turrets(&self) -> List<Obj>;
    /// The first tow-cable weapon, as (arsenal index, channel). Native owners
    /// inspect their current arsenal without building a weapon table.
    fn tow_weapon(&self,control:Obj)->Option<(usize,f64)>;
    /// `battle.reinforcements[team]`.
    fn reinforcements(&self, team: f64) -> Option<f64>;
    /// The living human units (ai.humanUnits()), in roster order.
    fn human_units(&mut self) -> List<Obj>;
    /// `world.nav?.planner` / `world.nav?.graph` are there.
    fn has_planner(&self) -> bool;
    fn has_graph(&self) -> bool;
    /// The map's navigation (graph, barriers, boundary), content the map
    /// loads: its queries are navigation's own.
    fn nav(&mut self) -> Option<&mut crate::ai::nav::Nav>;
    /// The agent's `supportHolds` (patrolMounted's count).
    fn support_holds(&self, agent: Obj) -> Option<f64>;
    /// A hint's `link` (the next of its ring).
    fn hint_link(&self, hint: Obj) -> Option<Obj>;

    /// Scoped steering and decay edits finish before another owner callback.
    /// Vehicle intent fields write directly into F2's full intent row.
    fn edit_steer<R>(&mut self, control: Obj, f: impl FnOnce(&mut Option<Steer>) -> R) -> R;
    fn read_decay(&mut self, hull: Obj) -> Option<super::service::Decay>;
    fn write_decay(&mut self, hull: Obj, value: super::service::Decay);
    fn write_vehicle_intent(&mut self, control: Obj, value: SeatIntent);

    // ---- calls: collision, registry, line of sight
    /// CollisionWorld.cast (into its hit scratch), leaving out `ignore`.
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit>;
    /// units.query(center, radius, out, filter): the members found, in the
    /// registry's order.
    fn query(&mut self, center: &Vector3, radius: f64, filter: QueryFilter) -> List<Obj>;
    /// world.relation(a, b).
    fn relation(&mut self, a: f64, b: f64) -> f64;
    /// The notifier uses the world's relations, independently of AI policy.
    fn notify_relation(&mut self, a: f64, b: f64) -> f64 { self.relation(a, b) }
    /// world.los(a, b) (None: the world has none).
    fn los(&mut self, a: &Vector3, b: &Vector3) -> Option<bool>;
    /// A unit's bounding sphere this tick (world.js sphere).
    fn sphere(&mut self, x: Obj) -> Sphere;

    // ---- calls: seats, hulls, turrets
    /// A control's eye and aim (`unit.origin(out)`, `unit.forward(out)`).
    fn origin(&mut self, control: Obj) -> Vector3;
    fn forward(&mut self, control: Obj) -> Vector3;
    /// A mount's pivot (mountAngles' `lease.origin(out)`).
    fn mount_origin(&mut self, origin: Origin) -> Vector3;
    /// turret.holdYaw(a).
    fn hold_yaw(&mut self, turret: Obj, a: f64) -> f64;
    /// Vehicles.eligibleSeat(unit, body) / Vehicle.firstFreeSeat().
    fn eligible_seat(&mut self, unit: Obj, hull: Obj) -> f64;
    fn first_free_seat(&mut self, hull: Obj) -> f64;
    /// Vehicles.board(unit, body) / TurretSeats.man(unit, body).
    fn board(&mut self, unit: Obj, hull: Obj) -> bool;
    fn man(&mut self, unit: Obj, turret: Obj) -> bool;
    /// Vehicles.requestExit / TurretSeats.requestExit / Vehicles.unboard.
    fn request_exit_vehicle(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool;
    fn request_exit_turret(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool;
    fn unboard(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool;
    /// Vehicles.humanTarget(body).
    fn human_target(&mut self, hull: Obj) -> Option<Obj>;
    /// Vehicle.damage(amount).
    fn damage(&mut self, hull: Obj, amount: f64);
    /// pilotInput(unit, request, aim, dt, env, out) (src/flight/ai.js): the
    /// stick. It makes the control's bag `{}` when it has none: the caller
    /// does that in its own state, before the call.
    fn pilot_input(&mut self, control: Obj, request: Option<&Request>, aim: Option<&Vector3>, dt: f64, env: &PilotEnv) -> Stick;

    // ---- calls: navigation (a controller's control and its request)
    /// controller.navControl.attach(unit) / .update(dt) / .request.
    fn nav_attach(&mut self, controller: Option<Obj>, unit: Obj);
    fn nav_update(&mut self, controller: Option<Obj>, dt: f64);
    fn nav_request(&mut self, controller: Option<Obj>) -> Option<Request>;
    /// The map's hints: hints.reserve(hint, controller, now), and a hint's
    /// `reservedAt` written.
    fn reserve_hint(&mut self, hint: Obj, controller: Obj, now: f64);
    fn set_hint_reserved_at(&mut self, hint: Obj, at: f64);

    // ---- calls: the AI's controllers, agents and combat
    /// The seat's input step's analogs into the control's intent.
    fn set_intent_analogs(&mut self, control: Obj, stick: &Stick);
    /// controller.requestObjective(true) / controller.requestExit().
    fn request_objective(&mut self, controller: Obj, exclude: bool);
    fn controller_request_exit(&mut self, controller: Obj);
    /// `controller.reboardAt = t` / `controller.fireA = true` /
    /// `controller.request.valid = false`.
    fn set_reboard_at(&mut self, controller: Obj, t: f64);
    fn fire_a(&mut self, controller: Obj);
    fn invalidate_request(&mut self, controller: Obj);
    /// controller.addVehicleNote({ unit: entry, generation }).
    fn add_vehicle_note(&mut self, controller: Obj, note: Ref);
    /// `character.objective = post`.
    fn set_objective(&mut self, character: Obj, post: Obj);
    /// The agent's `supportHolds` written; the patrol's `expires` and
    /// requested state.
    fn set_support_holds(&mut self, agent: Obj, holds: f64);
    fn set_patrol_expires(&mut self, patrol: Obj, t: f64);
    fn patrol_request_state(&mut self, patrol: Obj, state: f64);
    /// cpCenter(ai, post) / captureRegion(ai, post) as (sizeX, sizeZ).
    fn cp_center(&mut self, post: Obj) -> Vector3;
    fn capture_region(&mut self, post: Obj) -> Option<(f64, f64)>;
    /// An agent's movement helper's `mask` or `radius` (its traversal mask
    /// or radius for its unit; each read attaches the helper's control to
    /// the unit, as navigation's helper's `unit` getter does).
    fn helper_traversal(&mut self, agent: Obj, radius: bool) -> f64;
    /// ai.combat.memoryPoint(agent).
    fn memory_point(&mut self, agent: Obj) -> Option<Vector3>;
    /// selectWeapon(controller, unit, target, switchAllowed, world, out) /
    /// switchWeapon(controller, control, index).
    fn select_weapon(&mut self, controller: Obj, control: Obj, target: Obj, switch_allowed: bool) -> Choice;
    fn switch_weapon(&mut self, controller: Obj, control: Obj, index: usize) -> bool;
    /// The attack patterns' `face(posture)`.
    fn face(&mut self, patterns: Obj, posture: f64);
    /// The agent's tracker: setDelay(seconds, now) / defaultDelay(now).
    fn set_delay(&mut self, agent: Obj, seconds: f64, now: f64);
    fn default_delay(&mut self, agent: Obj, now: f64);
    /// The agent's movement helper: goto(point, { mode: "direct" } when
    /// `direct`, else none), stop(), followPath(points, override,
    /// altitude), land(on).
    fn helper_goto(&mut self, agent: Obj, point: &Vector3, direct: bool);
    fn helper_stop(&mut self, agent: Obj);
    fn helper_follow_path(&mut self, agent: Obj, points: &[[f64; 3]], over: bool, altitude: bool);
    fn helper_land(&mut self, agent: Obj, on: bool);
}
