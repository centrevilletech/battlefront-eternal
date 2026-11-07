//! What the AI control core asks of the rest of the battle: the
//! `aiWorld` object game.js builds and the service bags classic/index.js
//! binds. The JavaScript reaches everything through those bags with
//! `?.`, so a missing service is a silent no-op. This trait describes
//! those bindings, each method present exactly
//! where the JavaScript's bag has it; `Bind` says which optional parts
//! exist at all. In the replay tests a capture serves it; at the flip
//! the other Rust packages implement it (perception/combat/aim: the AI
//! combat package; vehicle services and the mounted policies: the
//! vehicle AI package; navigation: nav/, already in this crate).
//!
//! Views contain only the scalar facts an algorithm observes. Native hosts
//! resolve them from canonical stores at each call, retaining class metadata
//! once at load; captures supply the same views from their recorded tables.
//! Unit identities carry a storage generation. Weak references also carry
//! the source object's gameplay life; post and Character indices retain their
//! authored/roster order.

use crate::ai::owned::scratch::{List,Inline};
use super::super::nav::math::Vec3;
use super::super::nav::units::SizeType;
use super::super::random::ClassicRandom;
use crate::gen::Intent;
use std::rc::Rc;

/// A native identity for a body, seat control, registry entry or repair
/// wrapper. Capture readers map recorded objects into this same key space.
/// The storage generation is retained on wasm32 as well.
pub type UKey = u64;

/// A weak entity reference (controller.js:343 `refOf`): the entity's
/// table index and the life of it the holder meant.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EntRef {
    pub unit: UKey,
    pub generation: f64,
}

/// Damage attribution names a Character when one is available, else the
/// attacking unit itself. These tables have independent indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertSource {
    Character(usize),
    Unit(UKey),
}

/// A soldier body's fields the core reads (units.js Unit.soldier for a
/// soldier or a droideka; a seat controllable has none).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SoldierSnap {
    pub aim_yaw: Option<f64>,
    pub aim_pitch: Option<f64>,
    pub max_speed: Option<f64>,
    pub max_strafe: Option<f64>,
    pub max_turn: Option<f64>,
    pub max_pitch: Option<f64>,
    pub posture: Option<crate::gen::Posture>,
    /// A droideka's mode (12 is the ball; scheduler's droideka writes).
    pub mode: Option<f64>,
    pub rolled: Option<bool>,
}

/// A class's fields the core reads (unit.classDef), raw: the class
/// record of content for a body, a seat's constructed view for a seat
/// controllable, a building's settings for its target.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassSnap {
    /// `role` when it is an integer (ai.json), as controller.js roleOf
    /// reads it; else the UnitType string decides.
    pub role: Option<f64>,
    pub unit_type: Option<String>,
    pub ai_size_type: SizeType,
    pub prepared_travel_mask:Option<f64>,
    pub prepared_role:Option<f64>,
    pub prepared_health_type:Option<f64>,
    pub bounds: Option<[[f64; 3]; 2]>,
    pub max_speed: Option<f64>,
    pub max_strafe_speed: Option<f64>,
    pub strafe_speed: Option<f64>,
    pub max_turn_speed: Option<f64>,
    pub max_pitch_speed: Option<f64>,
    pub forward_speed: Option<f64>,
    pub turn_rate: Option<f64>,
    pub pitch_rate: Option<f64>,
    pub max_health: Option<f64>,
    /// HealthType as written: a number, a name, or missing (person).
    pub health_type_num: Option<f64>,
    pub health_type_name: Option<String>,
    pub base: Option<String>,
}

/// One weapon as the class jobs read it (weapons.js Weapon + its def):
/// whether it is the selected one is the arsenal's `selected` on the
/// unit snapshot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WeaponSnap {
    pub base: Option<String>,
    pub clip: Option<f64>,
    pub reserve: Option<f64>,
    pub rounds: Option<f64>,
    pub bottomless: bool,
    pub def_ammo: Option<f64>,
    pub def_class: Option<Rc<str>>,
    pub max_items: Option<f64>,
    pub item_base: Option<String>,
    pub item_class: Option<String>,
    pub item_life_span: Option<f64>,
    pub item_life_missing: bool,
    /// Service quantities of the weapon's def and of its item def
    /// (classes.js quantity() reads one of the two by base).
    pub qty_def: QtySnap,
    pub qty_item: QtySnap,
    /// Items of this dispenser's class the owner has about, not gone
    /// (items.js ownedBy), for the MaxItems test.
    pub owned_count: f64,
}

/// A def's service quantities as classes.js quantity() reads them:
/// def.service[kind][type] when that array exists, else
/// def[TypeHealth]/def[TypeAmmo] (def[Type] itself for building
/// health, type 5 and 6).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QtySnap {
    pub svc_health: Option<[Option<f64>; 7]>,
    pub svc_ammo: Option<[Option<f64>; 7]>,
    pub named_health: [Option<f64>; 7],
    pub named_ammo: [Option<f64>; 7],
    pub bare: [Option<f64>; 7],
}

/// Immutable facts can be shared by value observations. Native observations
/// leave these empty and query their canonical policy services instead.
#[derive(Clone,Debug,PartialEq)]
pub enum SharedRows<T> { Empty, Rows(Rc<[T]>) }
impl<T> Default for SharedRows<T> {fn default()->Self {Self::Empty}}
impl<T> std::ops::Deref for SharedRows<T> {
    type Target=[T];fn deref(&self)->&[T] {match self {Self::Empty=>&[],Self::Rows(rows)=>rows}}
}
impl<T> From<Vec<T>> for SharedRows<T> {
    fn from(rows:Vec<T>)->Self {if rows.is_empty(){Self::Empty}else{Self::Rows(rows.into())}}
}
impl<T> FromIterator<T> for SharedRows<T> {
    fn from_iter<I:IntoIterator<Item=T>>(iter:I)->Self {Vec::from_iter(iter).into()}
}

/// A unit (or target, or seat controllable) as the core sees it in one
/// phase. `intent` is the live intent the input step writes (a unit's
/// own; a seat controllable's seat intent).
#[derive(Clone, Debug, PartialEq)]
pub struct AiUnit {
    pub key: UKey,
    /// The JavaScript's `unit.id` (a seat controllable carries its
    /// occupant's, seats.js:587).
    pub id: f64,
    pub generation: f64,
    pub kind: f64,
    pub team: f64,
    pub alive: bool,
    pub removed: bool,
    pub human: bool,
    pub hero: bool,
    pub targetable: Option<bool>,
    pub position: Vec3,
    pub velocity: Option<Vec3>,
    pub soldier: Option<SoldierSnap>,
    pub class_def: Option<Rc<ClassSnap>>,
    pub class_name: Option<Rc<str>>,
    /// A runtime health type takes precedence over the class definition
    /// (repair wrappers distinguish a building from its rebuild state).
    pub health_type_num: Option<f64>,
    pub health_type_name: Option<Rc<str>>,
    pub weapons: SharedRows<WeaponSnap>,
    /// The arsenal's selected weapon index pair as weapons-ai reads it
    /// is the combat package's; classes only compare the selected
    /// weapon object, which its service answers.
    pub combat_health: Option<f64>,
    pub health: Option<f64>,
    pub combat_max_health: Option<f64>,
    pub max_health: Option<f64>,
    pub sphere_radius: Option<f64>,
    pub radius: Option<f64>,
    pub nav_radius: Option<f64>,
    pub nav_mask: Option<f64>,
    pub seat: bool,
    /// unit.seat.vehicle's position, when seated in a vehicle.
    pub seat_vehicle_pos: Option<Vec3>,
    pub has_lease: bool,
    pub lease_is_self: bool,
    pub is_seat_unit: bool,
    pub has_turret: bool,
    pub mine: bool,
    /// The Character whose body this is (classes.js's human test).
    pub character: Option<usize>,
    pub human_piloted: bool,
    pub intent: Intent,
    /// `unit.forward(out)` / `origin(out)` / `aimPoint(out)` when the
    /// unit has them (units compute them; a replay serves them).
    pub forward: Option<Vec3>,
    pub origin: Option<Vec3>,
    pub aim_point: Option<Vec3>,
    /// A building wrapper's wrapped building health fields are read
    /// through health/max_health above, as its getters give them.
    pub building_alive: Option<bool>,
    /// Repair wrappers can stand for a building or vehicle body. A
    /// weapon's held repair target may name that body directly.
    pub repair_aliases: SharedRows<UKey>,
}

impl AiUnit {
    /// The same unit as navigation sees it (nav/units.rs NavUnit), for
    /// the travel masks and radii the core computes and for a live
    /// helper host. Identity token: the table index.
    pub fn nav_unit(&self) -> super::super::nav::units::NavUnit {
        use super::super::nav::units::{ClassDef, NavUnit, Posture as NavPosture, SoldierView};
        let mut u = NavUnit::new(self.key as u64, self.kind, self.position);
        u.velocity = self.velocity.unwrap_or_default();
        u.generation = Some(self.generation);
        u.alive_false = !self.alive;
        u.nav_mask = self.nav_mask;
        u.nav_radius = self.nav_radius;
        u.class_def = self.class_def.as_ref().map(|c| ClassDef {
            ai_size_type: c.ai_size_type.clone(),
            bounds: c.bounds,
            max_speed: c.max_speed,
            max_strafe_speed: c.max_strafe_speed,
            max_turn_speed: c.max_turn_speed,
            max_pitch_speed: c.max_pitch_speed,
            forward_speed: c.forward_speed,
            turn_rate: c.turn_rate,
            jet_fuel_min_border: None,
            jet_fuel_recharge_rate: None,
        });
        u.soldier = self.soldier.map(|s| SoldierView {
            aim_yaw: s.aim_yaw,
            aim_pitch: s.aim_pitch,
            max_speed: s.max_speed,
            max_strafe: s.max_strafe,
            max_turn: s.max_turn,
            max_pitch: s.max_pitch,
            posture: match s.posture {
                Some(crate::gen::Posture::Stand) => NavPosture::Stand,
                Some(crate::gen::Posture::Crouch) => NavPosture::Crouch,
                Some(crate::gen::Posture::Prone) => NavPosture::Prone,
                None => NavPosture::Stand,
            },
            ..SoldierView::default()
        });
        u.forward = self.forward;
        u.origin = self.origin;
        u
    }
}

impl Default for AiUnit {
    fn default() -> Self {
        AiUnit {
            key: 0, id: 0.0, generation: 0.0, kind: 0.0, team: 0.0,
            alive: false, removed: false, human: false, hero: false,
            targetable: None, position: Vec3::default(), velocity: None,
            soldier: None, class_def: None, class_name: None, health_type_num: None, health_type_name: None, weapons: SharedRows::Empty,
            combat_health: None, health: None, combat_max_health: None, max_health: None,
            sphere_radius: None, radius: None, nav_radius: None, nav_mask: None,
            seat: false, seat_vehicle_pos: None, has_lease: false, lease_is_self: false, is_seat_unit: false, character: None, human_piloted: false,
            has_turret: false, mine: false, intent: Intent::default(),
            forward: None, origin: None, aim_point: None, building_alive: None, repair_aliases: SharedRows::Empty,
        }
    }
}

/// Common UnitController scalar reads need neither a class nor an intent
/// snapshot. Native worlds borrow these values from the current body; replay
/// worlds keep the same unit observation through the default trait method.
#[derive(Clone,Copy,Debug,Default,PartialEq)]
pub struct UnitScalars {pub generation:f64,pub kind:f64,pub team:f64,pub alive:bool,pub position:Vec3,pub has_lease:bool,pub lease_is_self:bool,pub hero:bool,pub human:bool}
impl UnitScalars {
    pub fn of(unit:&AiUnit)->Self {Self {generation:unit.generation,kind:unit.kind,team:unit.team,alive:unit.alive,position:unit.position,has_lease:unit.has_lease,lease_is_self:unit.lease_is_self,hero:unit.hero,human:unit.human}}
}
/// LOD observes motion and the source identity parity at one current read.
/// It does not need intent, class policy, health or held/rider geometry.
#[derive(Clone,Copy,Debug,Default,PartialEq)]
pub struct UnitMotion {pub id:f64,pub position:Vec3,pub velocity:Option<Vec3>}
impl UnitMotion {
    pub fn of(unit:&AiUnit)->Self {Self {id:unit.id,position:unit.position,velocity:unit.velocity}}
}

/// A command post's built capture region (regions.js buildRegion), as
/// the post keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionSnap {
    pub kind: RegionKind,
    pub half: [f64; 3],
    pub radius: f64,
    pub center: Vec3,
    pub inverse: [f64; 16],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionKind {
    Sphere,
    Box,
    Cylinder,
    Other,
}

/// A post's AI properties (goals.js postAI): the mission record joined
/// at load, or the defaults said from the map alone.
#[derive(Clone, Debug, PartialEq)]
pub struct PostAi {
    pub order: Option<f64>,
    pub atk: [f64; 6],
    pub def: [f64; 6],
    pub bans: f64,
    pub value_bleed: f64,
    pub spawn_present: Option<bool>,
    pub ally_present: Option<bool>,
    pub ally_count: f64,
    pub class_spawn_points: f64,
    pub capture: Option<CaptureSnap>,
    pub radius: f64,
    pub yaw: f64,
}

impl Default for PostAi {
    fn default()->Self {Self {order:None,atk:[100.;6],def:[100.;6],bans:0.,value_bleed:100.,
        spawn_present:None,ally_present:None,ally_count:0.,class_spawn_points:0.,capture:None,radius:0.,yaw:0.}}
}

/// The capture part of a post's AI record (goals.js captureOf): where
/// its centre is and the built size the offset spans.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureSnap {
    pub center: Vec3,
    pub half: [f64; 3],
}

/// A command post as the core reads it (commandposts.js Post,
/// mobile-posts.js MobilePost). `region` is what `post.contains`
/// tests; `capture` inside `ai` is the mission's region the patrol
/// geometry reads (patrol.js captureRegion prefers the population's).
#[derive(Clone, Debug, PartialEq)]
pub struct PostSnap {
    pub index: usize,
    pub name: Option<Rc<str>>,
    pub team: f64,
    pub active: bool,
    pub position: Vec3,
    pub building_alive: Option<bool>,
    pub neutralize: f64,
    pub radius: f64,
    pub speed: f64,
    pub ai: PostAi,
    pub region: Option<RegionSnap>,
    /// The map capture block patrol.js falls back to (post.desc.capture
    /// or post.capture), raw as captureRegion reads it.
    pub desc_capture: Option<Rc<RawCapture>>,
    pub desc_spawn_path: bool,
    pub desc_spawn_points: f64,
    pub desc_ally_path: bool,
}

/// A raw capture block (a map region or a built one) as patrol.js
/// captureRegion reads it: a center array or a matrix, a size, a
/// built half when the object already is a built region.
#[derive(Clone, Debug, PartialEq)]
pub struct RawCapture {
    pub center: Option<[f64; 3]>,
    pub matrix: Option<Vec<f64>>,
    pub size: Option<[f64; 3]>,
    pub half: Option<[f64; 3]>,
}

/// A roster Character as the core reads and writes it (battle.js
/// Character): its body and its objective are table indices.
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterSnap {
    pub index: usize,
    pub team: f64,
    pub human: bool,
    pub unit: Option<UKey>,
    pub objective: Option<usize>,
}

/// One player's view for the level-of-detail gate (scheduler.js
/// cameraAccepts reads matrixWorld's elements and these): the
/// simulation camera offline (camera.js, its matrix as game.js left
/// it), the network views online (network-views.js).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraView {
    pub elements: [f64; 16],
    pub fov: f64,
    pub aspect: f64,
    pub near: f64,
    pub far: f64,
    pub zoom: f64,
}

/// What is bound (classic/index.js bind() + the vehicle adapter): the
/// presence the JavaScript's `?.` sees. In a battle everything here is
/// bound; the degraded shapes are the tests'.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bind {
    /// Navigation: goto helpers exist (else DirectHelper), the planner
    /// and hint services answer, navigationInput/analogs are bound.
    pub nav: bool,
    /// The AI combat services (perception, aim, weapons, combat agent).
    pub combat: bool,
    /// The combat agent class exists (else the missing stand-in).
    pub combat_agent: bool,
    /// Population's requestObjective (goals bound through spawning).
    pub population: bool,
    /// The vehicle AI services and its board/unboard agents.
    pub vehicles: bool,
    /// The class jobs (snipe/mine behavior helpers replace the bases).
    pub classes: bool,
    /// A second generator exists (else B's draws share A's stream).
    pub random_b: bool,
    /// The Enhanced AI (agents, objectives, difficulty flavor).
    pub enhanced: bool,
    /// world.mineGroundHeight exists.
    pub mine_ground: bool,
    /// adaptClassicAI ran (mounted controllers follow their control).
    pub vehicle_adapter: bool,
}

impl Default for Bind {
    fn default() -> Self {
        Bind { nav: true, combat: true, combat_agent: true, population: true, vehicles: true, classes: true, random_b: true, enhanced: false, mine_ground: true, vehicle_adapter: true }
    }
}

/// The draws, A and B (classic/index.js:226): every draw the core makes
/// goes through here, in the JavaScript's order, so a replay serves
/// them in order and a live run draws its own generators.
pub trait Draws {
    fn a_uniform(&mut self) -> f64;
    fn b_uniform(&mut self) -> f64;
    fn a_int(&mut self, lo: f64, hi: f64) -> f64 {
        // int() draws only when hi > lo (random.js:42).
        if !(hi > lo) {
            return lo;
        }
        int_of(self.a_uniform(), lo, hi)
    }
    fn a_range(&mut self, lo: f64, hi: f64) -> f64 {
        range_of(self.a_uniform(), lo, hi)
    }
    fn b_range(&mut self, lo: f64, hi: f64) -> f64 {
        range_of(self.b_uniform(), lo, hi)
    }
}

/// ClassicRandom.int on an already-drawn uniform (random.js:41). A
/// draw only happens when hi > lo; callers that must not draw spell
/// the test themselves (they all go through a_int, which draws first
/// like the JavaScript's uniform() call inside int()... see note:
/// int() returns lo without drawing when hi <= lo, so a_int must not
/// draw then either. Implementations override a_int for that; the
/// default here is only the arithmetic.
pub fn int_of(u: f64, lo: f64, hi: f64) -> f64 {
    if !(hi > lo) {
        return lo;
    }
    v8math::trunc(v8math::fround(v8math::fround(u * v8math::fround(hi - lo + 1.0)) + lo))
}

/// ClassicRandom.range on an already-drawn uniform (random.js:49).
pub fn range_of(u: f64, lo: f64, hi: f64) -> f64 {
    v8math::fround(v8math::fround(lo) + v8math::fround(u * v8math::fround(hi - lo)))
}

/// The live draws: the two generators themselves.
pub struct LiveDraws {
    pub a: ClassicRandom,
    pub b: ClassicRandom,
}

impl Draws for LiveDraws {
    fn a_uniform(&mut self) -> f64 {
        self.a.uniform()
    }
    fn b_uniform(&mut self) -> f64 {
        self.b.uniform()
    }
    fn a_int(&mut self, lo: f64, hi: f64) -> f64 {
        self.a.int(lo, hi)
    }
    fn a_range(&mut self, lo: f64, hi: f64) -> f64 {
        self.a.range(lo, hi)
    }
    fn b_range(&mut self, lo: f64, hi: f64) -> f64 {
        self.b.range(lo, hi)
    }
}

/// Controller fields the AI combat services write (perception's fire
/// controls, weapons-ai's target setting, combat's aim upkeep): read
/// back after those calls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CombatWrites {
    pub fire_a: bool,
    pub fire_b: bool,
    pub target: Option<EntRef>,
    pub aim_state: f64,
    pub aim_point: Vec3,
    pub stance: f64,
    pub facing: bool,
    pub wanted_mode: f64,
    pub dodge: Option<f64>,
    pub alert_anim: Option<bool>,
}

/// The controller fields combat reads before a service. Perception and
/// trackers stay in combat's own tables; feeding this view never resets
/// their heaps, aim filters, fire timers or target memory.
#[derive(Clone)]
pub struct CombatInput<'a> {
    /// The service runs before this immutable loan ends. Reading canonical
    /// fields here avoids constructing and copying a second controller row.
    pub source:&'a super::controller::Controller,
    pub helper_kind:Option<HelperKind>,
    pub helper_has_path:bool,
    pub policy:super::policy::Input,
    pub objective:Option<usize>,
    pub nav_stance:Option<f64>,
}
impl<'a> CombatInput<'a> {
    #[inline(always)]
    pub fn read(c:&'a super::controller::Controller,objective:Option<usize>)->Self {
        Self {source:c,helper_kind:c.agent.as_ref().and_then(|a|a.helper.kind),
            helper_has_path:c.agent.as_ref().is_some_and(|a|a.helper_has_path()),
            policy:super::policy::Input::read(c),objective,
            nav_stance:c.nav_control.as_ref().map(|n|n.stance)}
    }
}

/// One controller's alert state after combat's ingest.
#[derive(Clone, Debug, PartialEq)]
pub struct AlertState {
    pub controller: usize,
    pub shot_pending: bool,
    pub shot_known: bool,
    pub shot_point: Vec3,
    pub shot_source: Option<AlertSource>,
    pub danger_pending: bool,
    pub danger_point: Vec3,
    pub acoustic: Inline<EntRef,20>,
    pub dodge: Option<f64>,
    pub alert_anim: Option<bool>,
}

/// A combat response (perception.js hazard/alert/acousticResponse):
/// the goal mode and point, whether the mount is left instead, whether
/// the soldier shouts that it heard an enemy.
#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub mode: f64,
    pub point: Option<Vec3>,
    pub target: Option<EntRef>,
    pub exit: bool,
    pub alert: bool,
}

/// P16's response services call P15 acceptance policies. These are the
/// only policy fields those calls may change, including a null response.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResponseState {
    pub interrupts: super::agents::Interrupts,
    pub enhanced: Option<super::controller::EnhancedMemory>,
    pub secondary_wait_until: f64,
}

impl ResponseState {
    pub fn read(c: &super::controller::Controller) -> Self {
        Self {
            interrupts: c.agent.as_ref().map(|a| a.interrupts).unwrap_or_default(),
            enhanced: c.enhanced.as_ref().map(super::controller::EnhancedMemory::share), secondary_wait_until: c.secondary_wait_until,
        }
    }

    pub fn apply(self, c: &mut super::controller::Controller) {
        if let Some(agent) = &mut c.agent { agent.interrupts = self.interrupts; }
        c.enhanced = self.enhanced;
        c.secondary_wait_until = self.secondary_wait_until;
    }
}

/// A repair job (classes.js findRepairJob): its target and which need
/// ("health" or "ammo").
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JobRef {
    pub target: EntRef,
    pub ammo: bool,
}

/// Mounted patrol can ask Population for objectives, including for
/// other AI crew. Apply each publication through the native core.
#[derive(Clone, Debug, PartialEq)]
pub struct MountedPatrol {
    pub patrol: super::patrol::Patrol,
    pub publications: Vec<(usize, Option<usize>)>,
    pub support_holds: Option<f64>,
}

/// What one helper call left observable (the goto helpers' arrived /
/// failed / status / hasPath, read as properties between calls).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HelperObs {
    pub arrived: bool,
    pub failed: bool,
    pub status: f64,
    pub has_path: bool,
}

/// What the navigation control shows between calls (control.js
/// NavControl's status, executor, sidestep side, stance and jet; its
/// request is the controller's own MoveRequest).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlObs {
    pub status: f64,
    pub has_executor: bool,
    pub avoid_side: f64,
    pub stance: f64,
    pub jet_pending: bool,
    pub jet_vector: Vec3,
    /// The control's movement request (the controller's own, aliased).
    pub request: super::controller::MoveRequest,
}

impl Default for ControlObs {
    fn default() -> Self {
        ControlObs { status: 0.0, has_executor: false, avoid_side: 0.0, stance: 0.0, jet_pending: false, jet_vector: Vec3::default(), request: Default::default() }
    }
}

/// Which movement helper an agent holds (classic/index.js
/// createHelper): the ground or flyer goto helper, a vehicle helper by
/// name, or the degraded straight-line helper (port in agents.rs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelperKind {
    Ground,
    Fly,
    Direct,
    Vehicle,
    /// StationaryHelper has no navigation owner or disposal method.
    Stationary,
}

/// Which route a goto asks for (goto.js options.mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GotoMode {
    Route,
    Direct,
    Straight,
}

/// The mounted policy of a seat controller (vehicles/controllers.js).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    None,
    Mobile,
    Stationary,
    Fly,
}

/// A hint as patrol and the class jobs read it (nav/hints.js records
/// through patrol.js's adapters). `post` is the hint's command post by
/// table index (its authored attachment; the runtime hint's is always
/// absent, quirks R1d).
#[derive(Clone, Debug, PartialEq)]
pub struct HintSnap {
    pub bit: f64,
    pub position: Vec3,
    pub forward: Vec3,
    pub primary_stance: f64,
    pub post: Option<usize>,
    pub mode: f64,
    pub radius: f64,
    pub reserved_at: Option<f64>,
    pub linked: SharedRows<usize>,
}

/// An item lying about, as the class jobs read them (items.js).
#[derive(Clone, Debug, PartialEq)]
pub struct ItemSnap {
    pub gone: bool,
    pub mine: bool,
    pub def_base: Option<Rc<str>>,
    pub def_class: Option<Rc<str>>,
    pub position: Vec3,
    pub radius: Option<f64>,
    pub team: f64,
    pub owner: Option<UKey>,
    pub qty: QtySnap,
}

/// Character state belongs to the battle owner. Control retains only roster
/// indices and its own controllers, never a second mutable Character table.
pub trait Characters {
    fn character(&self,index:usize)->CharacterSnap;
    /// Controller binding observes only the current body, not the objective.
    fn character_unit(&self,index:usize)->Option<UKey> {self.character(index).unit}
    fn character_objective(&self,index:usize)->Option<usize> {self.character(index).objective}
    fn character_set_objective(&mut self,index:usize,post:Option<usize>);
}

/// The rest of the battle, as the core asks for it. Controller and
/// agent arguments are the controller's index in the core's roster;
/// the agent an argument names is that controller's current agent.
/// Methods the JavaScript calls with `?.` on a possibly-missing bag
/// entry only run when `Bind` says the bag is there.
pub trait AiWorld: Characters {
    fn link_character(&mut self,_character:usize,_controller:usize,_bound:bool) {}
    /// Native identities exist for every agent, including a missing combat
    /// capability. Helper creation has completed before this notification.
    fn agent_created(&mut self,_controller:usize,_helper:super::agents::Helper) {}
    fn agent_disposed(&mut self,_controller:usize,_helper:super::agents::Helper) {}
    /// Native subclass instances own these virtual decisions. Replay worlds
    /// retain their recorded service behavior when no concrete agent exists.
    fn agent_target_override(&mut self,_controller:usize,_target:UKey)->Option<bool> {None}
    fn agent_board_override(&mut self,_controller:usize,_candidate:UKey)->Option<bool> {None}
    // -- the phase's tables and clocks.
    fn time(&self) -> f64;
    fn paused(&self) -> bool;
    fn unit(&self, key: UKey) -> AiUnit;
    /// Liveness neither inspects policy nor samples held/lease geometry.
    /// Capture hosts retain the original full observation at this point.
    fn unit_live(&self,key:UKey,generation:f64)->bool {
        let unit=self.unit(key);unit.alive&&unit.generation==generation
    }
    fn unit_scalars(&self,key:UKey)->UnitScalars {UnitScalars::of(&self.unit(key))}
    fn unit_motion(&self,key:UKey)->UnitMotion {UnitMotion::of(&self.unit(key))}
    fn unit_is_seat_unit(&self,key:UKey)->bool {self.unit(key).is_seat_unit}
    /// UnitController.bind/unbind writes the canonical body's controller at
    /// that lifecycle point. An unbind clears only this controller's link.
    fn link_controller(&mut self,_unit:UKey,_controller:usize,_bound:bool) {}
    fn intent_command(&mut self,key:UKey,command:crate::ai::owned::intent::Command);
    fn units_len(&self) -> usize;
    fn unit_key(&self,index:usize)->UKey;
    fn unit_sphere_radius(&mut self,key:UKey)->Option<f64>;
    fn unit_forward(&mut self,key:UKey)->Option<Vec3>;
    fn unit_aim_point(&mut self,key:UKey)->Option<Vec3>;
    fn unit_has_origin_forward(&self,key:UKey)->bool;
    fn post(&self, index: usize) -> PostSnap;
    fn objective_post(&self,index:usize)->super::goals::ObjectivePost<'_>;
    fn posts_len(&self) -> usize;
    fn post_name_id(&self,index:usize)->u32;
    fn cameras(&self) -> &[CameraView];
    fn network_views(&self) -> bool;
    fn lod_disabled_world(&self) -> bool;
    /// The registry's query (registry.js Units.query): living members
    /// by their broadphase sphere, in registry order, the caller's
    /// filter applied as it goes.
    fn unit_buffer(&self)->List<UKey>;
    fn predicate_buffer(&self)->List<(UKey,bool)>;
    fn index_buffer(&self)->List<usize>;
    fn weight_buffer(&self)->List<f64>;
    fn objective_buffer(&self)->List<Option<usize>>;
    fn recruit_buffer(&self)->List<(f64,usize)>;
    fn sort_recruits(&mut self,rows:&mut Vec<(f64,usize)>);

    fn units_query(&mut self, center: Vec3, radius: f64, filter: &mut dyn FnMut(&AiUnit) -> bool) -> List<UKey>;
    /// Repair candidates including destroyed buildings
    /// (src/ai/services.js createServiceCandidates), filtered to the
    /// radius as it filters them; the building wrappers are table
    /// entries of their own.
    fn service_candidates(&mut self, center: Vec3, radius: f64) -> List<UKey>;
    fn items_len(&self)->usize;
    fn item(&self,index:usize)->ItemSnap;
    /// A Character's crew (ai/index.js objectiveCrew): its vehicle's
    /// driver and occupants as character indices, when seated.
    fn crew(&mut self, character: usize) -> Option<CrewSnap>;
    fn crew_occupants(&self,character:usize)->List<usize>;
    /// objectiveVehicleMask uses navigation's integer/trimmed size rules,
    /// rather than the goal classifier's deliberately narrower string rules.
    fn objective_vehicle_mask(&self,vehicle:UKey)->f64 {
        crate::ai::nav::units::traversal_mask(&self.unit(vehicle).nav_unit()) as f64
    }
    fn tickets(&self, team: f64) -> Option<f64>;
    fn reinforcements(&self, team: f64) -> Option<f64>;
    fn mine_ground_height(&mut self, point: Vec3) -> f64;
    /// The simulation journal's voice records (squads.js emitVo):
    /// fields are (event, property, command kind or -1, context,
    /// positional, notice) as each call site spells them.
    fn emit_vo(&mut self, unit: UKey, event: &str, property: &str, command: f64, context: Option<f64>, positional: Option<bool>, notice: Option<bool>);

    // -- AI combat services (classic/index.js combatAdapter +
    // combat.js classicBinding; the combat package's).
    /// A live native host keeps the control and combat projections of
    /// this controller current before entering combat. Boundary replays
    /// already supply their entry state and need no additional action.
    /// Captured hosts receive the Character objective in the service view.
    /// A native service can observe the canonical post at its actual reader.
    fn combat_objective(&self,character:usize)->Option<usize> {self.character_objective(character)}
    fn combat_feed(&mut self, _controller: usize, _input: &CombatInput) {}
    /// Replay hosts observe each feed. A canonical host already owns the
    /// acoustic notes and protected-human lists those sensor passes mutate;
    /// they read no controller requests and need no repeated roster snapshot.
    /// Native journals signal whether any event can change controller alerts.
    /// A replay pass retains its captured preparation and observation order.
    /// Tracker, queue and skip services own no fields from CombatInput.
    /// Captured hosts can still prepare their source observation as usual.
    fn combat_prepare_retained(&mut self,core:&super::Core,controller:usize) {core.feed_combat(self,controller);}
    fn combat_prepare_query(&mut self,core:&super::Core,controller:usize) {core.feed_combat(self,controller);}
    fn combat_prepare_fire(&mut self,core:&super::Core,controller:usize)->bool {core.feed_combat(self,controller);true}
    fn combat_fire_requests(&mut self,controller:usize)->(bool,bool) {let w=self.combat_writes(controller);(w.fire_a,w.fire_b)}
    /// Captured services preserve their full write-back contract. The native
    /// aim-point query is pure and can leave unrelated Core fields untouched.
    fn combat_prepare_aim_query(&mut self,core:&super::Core,controller:usize)->bool {core.feed_combat(self,controller);true}
    fn combat_prepare_ingest(&mut self,core:&super::Core)->bool {core.feed_all_combat(self);true}
    fn combat_prepare_sensors(&mut self, core: &super::Core) { core.feed_all_combat(self); }
    /// Replay worlds keep the complete captured feed sequence. Canonical LOS
    /// prepares only queued owners, before checking their current life.
    fn combat_prepare_los(&mut self,core:&super::Core) {core.feed_all_combat(self);}
    /// Native policy callbacks observe controller state changes immediately.
    fn controller_state_changed(&mut self,_controller:usize,_state:f64) {}
    /// Native navigation shares these writes at the source setter, before a
    /// later helper query can observe the control again.
    fn nav_stance_changed(&mut self,_controller:usize,_stance:f64) {}
    fn nav_jet_requested(&mut self,_controller:usize,_vector:Vec3) {}
    fn combat_create_perception(&mut self, controller: usize);
    fn combat_attach_controller(&mut self, controller: usize);
    fn combat_detach_controller(&mut self, controller: usize);
    fn combat_rebuild_protected(&mut self);
    fn combat_ingest(&mut self) -> List<AlertState>;
    fn combat_init_agent(&mut self, controller: usize);
    fn combat_scan(&mut self, controller: usize);
    fn combat_drop_los_requests(&mut self, controller: usize);
    fn combat_service_los(&mut self, budget: f64) -> bool;
    fn combat_broadcast_acoustic(&mut self, unit: UKey) -> List<(usize, EntRef)>;
    fn combat_clear_candidate(&mut self, controller: usize);
    fn combat_rank_targets(&mut self, controller: usize);
    fn combat_update_target(&mut self, controller: usize);
    fn combat_update_target_selection(&mut self, controller: usize);
    fn combat_gate_fire(&mut self, controller: usize);
    fn combat_current_target(&mut self, controller: usize) -> Option<UKey>;
    fn combat_memory_point(&mut self, controller: usize) -> Option<Vec3>;
    fn combat_acquired_target(&mut self, controller: usize) -> Option<UKey>;
    fn combat_can_engage(&mut self, controller: usize, target: UKey) -> bool;
    fn combat_has_weapon(&mut self, controller: usize) -> bool;
    fn combat_sensor_range(&mut self, controller: usize, target: UKey) -> f64;
    fn combat_acoustic_range(&mut self, controller: usize) -> f64;
    fn combat_consume_fire(&mut self, controller: usize, step: f64);
    fn combat_aim_point(&mut self, controller: usize, dt: f64) -> Option<Vec3>;
    fn combat_lead_point(&mut self, controller: usize, target: UKey, dt: f64) -> Option<Vec3>;
    fn combat_mark_skip(&mut self, controller: usize, skipped: bool);
    fn combat_hazard_response(&mut self, controller: usize, state: &mut ResponseState) -> Option<Response>;
    fn combat_alert_response(&mut self, controller: usize, state: &mut ResponseState) -> Option<Response>;
    fn combat_acoustic_response(&mut self, controller: usize, state: &mut ResponseState) -> Option<Response>;
    /// weapons-ai as the class jobs call it (selectedWeapon,
    /// switchWeapon, effectiveRange, firingCone): weapon indices in
    /// the unit's snapshot list.
    fn weapons_selected(&mut self, controller: usize) -> Option<usize>;
    fn weapons_switch(&mut self, controller: usize, index: usize);
    /// arsenal.select directly (classes.js chooseWeapon's other arm).
    fn weapons_select(&mut self, controller: usize, index: usize);
    fn weapons_effective_range(&mut self, controller: usize, weapon: usize) -> f64;
    fn weapons_firing_cone(&mut self, controller: usize, point: Vec3) -> bool;
    /// unit.combat's repair target (weapons'; classes.js:384 reads it
    /// before the controller's own target).
    fn combat_repair_target(&mut self, unit: UKey) -> Option<UKey>;

    // Native class policies can read the real arsenal; captured worlds keep
    // the same defaults over their recorded weapon facts.
    fn classes_needs(&self,unit:UKey,ammo:bool)->bool { super::classes::needs(&self.unit(unit),ammo) }
    fn classes_dispenser_available(&self,unit:UKey,index:usize)->bool {
        let unit=self.unit(unit);let weapon=&unit.weapons[index];
        if super::classes::remaining_ammo(weapon)==0. {return false;}
        let max=weapon.max_items.unwrap_or(0.);max==0.||max.is_nan()||weapon.owned_count<max
    }
    fn classes_mine_weapon(&self,unit:UKey)->Option<usize> {
        let row=self.unit(unit);
        (0..row.weapons.len()).find(|&i| {let w=&row.weapons[i];
            w.base.as_deref()==Some("dispenser")&&w.item_base.as_deref()==Some("mine")
                &&w.item_life_missing&&self.classes_dispenser_available(unit,i)})
    }
    fn classes_service_weapon(&self,unit:UKey,health_type:f64,ammo:bool)->Option<usize> {
        let row=self.unit(unit);
        (0..row.weapons.len()).find(|&i| {let w=&row.weapons[i];
            let quantity=if w.base.as_deref()==Some("repair") {Some(&w.qty_def)}
                else if w.base.as_deref()==Some("dispenser")&&self.classes_dispenser_available(unit,i) {Some(&w.qty_item)}else{None};
            quantity.is_some_and(|q|super::classes::quantity(q,health_type,ammo)>0.)})
    }
    fn classes_repair_weapon(&self,unit:UKey,index:usize)->bool {
        self.unit(unit).weapons.get(index).and_then(|w|w.base.as_deref())==Some("repair")
    }
    fn repair_matches(&self,target:UKey,current:UKey)->bool {target==current||self.unit(target).repair_aliases.contains(&current)}

    // -- Vehicle AI services (classic/index.js VEHICLE_SERVICES; the
    // vehicle AI package's, with its mounted policies).
    fn vehicles_notify(&mut self) -> List<(usize, EntRef)>;
    fn vehicles_request_exit(&mut self,core:&mut super::Core,controller:usize);
    fn vehicles_boarded(&mut self, controller: usize) -> bool;
    fn vehicles_boarded_vehicle(&mut self, controller: usize) -> Option<UKey>;
    fn vehicles_boarded_vehicle_of(&mut self, unit: UKey) -> Option<UKey>;
    fn vehicles_boarded_vehicle_of_character(&mut self, character: usize) -> Option<UKey>;
    fn vehicles_current_controllable(&mut self, character: usize) -> Option<UKey>;
    fn vehicles_is_mounted_turret(&mut self, unit: UKey) -> bool;
    fn vehicles_empty_seats(&mut self, vehicle: UKey) -> f64;
    fn vehicles_crew_controllers(&mut self, vehicle: UKey, except: usize) -> List<usize>;
    fn vehicles_seats_full(&mut self, vehicle: UKey) -> bool;
    fn vehicles_pending_reservations(&mut self, vehicle: UKey, team: f64) -> bool;
    fn vehicles_low_tickets(&mut self, team: f64) -> bool;
    fn vehicles_pilot_keeps_seat(&mut self, controller: usize) -> bool;
    fn vehicles_alerts_blocked(&mut self, controller: usize) -> bool;
    fn vehicles_mount_reach(&mut self, unit: UKey, point: Vec3) -> bool;
    fn vehicles_refuses_hold(&mut self, controller: usize) -> bool;
    fn vehicles_can_enter(&mut self, controller: usize, candidate: UKey) -> Option<bool>;
    fn vehicles_free_seats(&mut self, candidate: UKey, team: f64, want: f64) -> f64;
    fn vehicles_reserve(&mut self, candidate: UKey, unit: UKey, want: f64);
    fn vehicles_reserve_seat(&mut self, controller: usize, vehicle: UKey) -> bool;
    fn vehicles_target_reserved(&mut self, target: UKey, team: f64) -> bool;
    fn vehicles_reserve_target(&mut self, target: UKey, unit: UKey);
    fn vehicles_is_at_at(&mut self, unit: UKey) -> bool;
    fn vehicles_support_point(&mut self, unit: UKey, post: usize) -> Option<Vec3>;
    fn vehicles_board_policy(&mut self, controller: usize, candidate: UKey) -> Option<bool>;
    fn vehicles_board_tick(&mut self,core:&mut super::Core, controller: usize, dt: f64);
    /// The mounted patrol returns its state and explicit Population
    /// publications. `objectives` is the characters' current objective
    /// table, checked before any callback writes are applied. None means
    /// the hook is absent (patrol.js then asks out), not a false return.
    fn vehicles_patrol_mounted(&mut self,core:&mut super::Core, controller: usize, patrol: super::patrol::Patrol, shared: &mut ForeignShared, objectives: &[Option<usize>], support_holds: Option<f64>) -> Option<MountedPatrol>;
    /// controllerPolicy(unit) and its bind/controller/agent effects.
    fn vehicles_policy_of(&mut self, unit: UKey) -> Policy;
    fn vehicles_policy_home_facing(&mut self, policy: Policy, unit: UKey) -> Option<bool>;
    fn vehicles_policy_agent(&mut self, policy: Policy, state: f64) -> bool;

    // -- Agents this package does not own (combat agent: AI combat;
    // board, unboard and the flyer agent: vehicle AI). Their state
    // machines' entries, exits and behavior run through here; their
    // fields never cross.
    fn foreign_agent_create(&mut self,core:&mut super::Core, controller: usize, kind: ForeignKind, shared: &ForeignShared) -> (ForeignTick, Option<ControlObs>);
    /// Agent.tick(dt, phase) whole: its state machine's round, ending
    /// in the state and pending state it reports.
    fn foreign_agent_tick(&mut self,core:&mut super::Core, controller: usize, dt: f64, phase: f64, shared: &ForeignShared) -> ForeignTick;
    /// Agent.exit(): dispose follows separately, as on a local agent.
    fn foreign_agent_exit(&mut self,core:&mut super::Core, controller: usize, shared: &ForeignShared) -> (Option<ForeignShared>, Vec<CoreEffect>);

    // -- Movement helpers and the navigation control (navigation's;
    // the direct helper is the core's own and never reaches here).
    fn helper_request_invalidated(&mut self, _controller: usize) {}
    fn helper_create(&mut self, controller: usize) -> (HelperKind, HelperObs, Option<ControlObs>);
    fn helper_goto(&mut self, controller: usize, point: Vec3, mode: GotoMode) -> HelperObs;
    fn helper_stop(&mut self, controller: usize) -> HelperObs;
    fn helper_terminate(&mut self, controller: usize) -> HelperObs;
    fn helper_tick(&mut self, controller: usize, dt: f64) -> HelperObs;
    fn helper_destination(&mut self, controller: usize) -> Vec3;
    fn helper_install_path(&mut self, controller: usize, points: &[Vec3]) -> HelperObs;
    fn helper_follow(&mut self, controller: usize, target: EntRef) -> HelperObs;
    fn helper_follow_trail(&mut self, controller: usize, on: bool) -> HelperObs;
    fn helper_hold_path(&mut self, controller: usize, on: bool) -> HelperObs;
    fn helper_set_leader(&mut self, controller: usize, leader: Option<EntRef>) -> HelperObs;
    fn helper_dispose(&mut self, controller: usize);
    fn nav_control_ensure(&mut self, controller: usize) -> ControlObs;
    fn nav_control_reset(&mut self, controller: usize);
    fn nav_control_obs(&mut self, controller: usize) -> ControlObs;

    // -- Navigation services (the ai.nav bag).
    fn nav_planner_idle(&mut self) -> bool;
    fn nav_service(&mut self, budget: f64);
    fn nav_point_clear(&mut self, point: Vec3, radius: f64, mask: f64) -> bool;
    fn nav_clip_segment(&mut self, from: Vec3, to: Vec3, radius: f64, mask: f64) -> (bool, Vec3);
    fn nav_in_danger_region(&mut self, point: Vec3, pad: f64) -> bool;
    fn nav_query_hints(&mut self, mask: f64, center: Vec3, half_extent: f64, cap: f64, requester: Option<UKey>) -> List<usize>;
    fn nav_hints_len(&self) -> usize;
    fn nav_hint(&self, hint: usize) -> HintSnap;
    fn nav_hint_available(&mut self, hint: usize) -> bool;
    fn nav_mark_hint(&mut self, hint: usize, unit: Option<UKey>, prefer_world: bool);
    fn nav_hint_set_reserved(&mut self, hint: usize, at: f64);
    /// controller.perception's presence the class jobs test.
    fn perception_flags(&mut self, controller: usize) -> (bool, bool);
    /// The controller fields the combat services may have written.
    fn combat_writes(&mut self, controller: usize) -> CombatWrites;
    /// world.network && world.secondaryWorldActive.
    fn network_secondary_active(&self) -> bool;
    /// nav/analogs.js navigationInput (executor step, posture/jet,
    /// analogs into the intent) and analogs alone.
    fn nav_navigation_input(&mut self, _core:&mut crate::ai::control::Core, controller: usize, step: f64, aim: Option<Vec3>,facing:bool) -> (Option<HelperObs>, Vec<CoreEffect>);
    fn nav_analogs(&mut self, _core:&mut crate::ai::control::Core, controller: usize, aim: Option<Vec3>, step: f64,facing:bool) -> ([f64; 4], Vec<CoreEffect>);
}

/// A seated Character's crew (ai/index.js:24 objectiveCrew).
#[derive(Clone, Debug, PartialEq)]
pub struct CrewSnap {
    pub vehicle: UKey,
    pub is_driver: bool,
    pub driver: Option<usize>,
    pub occupants: Vec<usize>,
}

/// The mission's team table the registry's relation reads
/// (registry.js:74, spawning.js teamTable).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TeamTable {
    pub relations: Vec<Option<Vec<f64>>>,
    pub factions: Vec<f64>,
    pub alliances: Vec<((f64, f64), f64)>,
}

impl TeamTable {
    /// registry.js relation(a, b).
    pub fn relation(&self, a: f64, b: f64) -> f64 {
        if a == b {
            return if a != 0.0 && !a.is_nan() { 1.0 } else { 0.0 };
        }
        if a == 0.0 || a.is_nan() || b == 0.0 || b.is_nan() {
            return 0.0;
        }
        let key = if a < b { (a, b) } else { (b, a) };
        for (k, v) in &self.alliances {
            if *k == key {
                return *v;
            }
        }
        let row = if a >= 0.0 { self.relations.get(a as usize) } else { None };
        match row {
            Some(Some(row)) if b >= 0.0 => row.get(b as usize).copied().unwrap_or(-1.0),
            _ => -1.0,
        }
    }
    pub fn faction(&self, team: f64) -> f64 {
        if team >= 0.0 { self.factions.get(team as usize).copied().unwrap_or(0.0) } else { 0.0 }
    }
}

/// Which foreign agent a controller state builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForeignKind {
    Combat,
    Board,
    Unboard,
    Fly,
}

/// A foreign agent's machine after a create or a tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ForeignTick {
    /// The running object finished after a callback replaced its binding.
    pub replaced:bool,
    pub state: f64,
    pub pending: f64,
    pub helper: super::agents::Helper,
    pub interrupts: super::agents::Interrupts,
    pub shared: Option<ForeignShared>,
    pub effects: Vec<CoreEffect>,
}

/// P15-owned writes from mounted steering and the foreign flyer: an
/// objective publication runs P15's own queue update, while a saved post
/// on disembark only changes the Character. Every write checks its input
/// before applying; no controller or character snapshot is restored.
#[derive(Clone, Debug, PartialEq)]
pub enum CoreEffect {
    Publish { character: usize, before: Option<usize>, after: Option<usize> },
    Objective { character: usize, before: Option<usize>, after: Option<usize> },
    Reboard { controller: usize, before: f64, after: f64 },
}

/// Controller fields a foreign agent can change through shared agent
/// operations, in addition to CombatWrites and the navigation control.
/// Hosts receive the current values and return them after the call.
#[derive(Clone, Debug, PartialEq)]
pub struct ForeignShared {
    pub commands: super::controller::CommandQueue,
    pub last_command: Option<super::controller::SquadCommand>,
    pub pending: f64,
    pub published: bool,
    pub aim_target: Option<EntRef>,
    pub leader: Option<EntRef>,
    pub reboard_at: f64,
    pub secondary_wait_until: f64,
    pub exit_request: bool,
    pub jet_pending: bool,
    pub jet_vector: Vec3,
    pub enhanced: Option<super::controller::EnhancedMemory>,
}

impl ForeignShared {
    pub fn read(c: &super::controller::Controller) -> Self {
        Self {
            commands: c.commands.clone(), last_command: c.last_command,
            pending: c.fsm.pending, published: c.published,
            aim_target: c.aim_target, leader: c.leader,
            reboard_at: c.reboard_at, secondary_wait_until: c.secondary_wait_until,
            exit_request: c.exit_request, jet_pending: c.jet_pending, jet_vector: c.jet_vector,
            enhanced: c.enhanced.as_ref().map(super::controller::EnhancedMemory::share),
        }
    }

    pub fn apply(self, c: &mut super::controller::Controller) {
        c.commands = self.commands;
        c.last_command = self.last_command;
        c.fsm.pending = self.pending;
        c.published = self.published;
        c.aim_target = self.aim_target;
        c.leader = self.leader;
        c.reboard_at = self.reboard_at;
        c.secondary_wait_until = self.secondary_wait_until;
        c.exit_request = self.exit_request;
        c.jet_pending = self.jet_pending;
        c.jet_vector = self.jet_vector;
        c.enhanced = self.enhanced;
    }
}
