//! What the AI's perception and combat code reads of the battle and asks of
//! it (src/ai/classic/perception.js, combat.js, aim.js, weapons-ai.js,
//! src/ai/enhanced/aim.js).
//!
//! The JavaScript reaches live objects: units (soldiers and droidekas, the
//! seat controls bots drive, and the registry's entries for hulls, turrets
//! and buildings), their weapons, the controllers and agents of the control
//! core, the navigation's hints and helpers, the vehicle AI's hooks. The
//! Rust names each by an [`Obj`], the caller's token for the object
//! (compared where the JavaScript compares objects with `===`), and reads
//! it through the views below. A field the JavaScript reads with `?.` or
//! `??` is an `Option` where it can be missing; a field that comes straight
//! from content (a class's or weapon's definition) is the JavaScript value
//! itself ([`Js`]), read with JavaScript's own rules.
//!
//! [`World`] is every call across the boundary, labelled as the capture
//! labels them (tools/sim-capture/ai-combat.mjs). What other packages own
//! and compute lazily (a unit's hit-shape sphere, its eye and aim, the
//! registry's broadphase, a sight line through the collision world) is a
//! call; plain fields are views. The control core's controller fields this
//! package reads and writes are [`Ctl`], which the control core holds per
//! controller; an agent's target tracker is this package's
//! ([`super::perception::TargetTracker`]), held on every agent.

use crate::ai::{owned::scratch::List,control::world::SharedRows};
use std::rc::Rc;
use three::Vector3;

use crate::combat::defs::{Js, JsObj};

pub use super::perception::Perception;

/// A JavaScript object this package names (a unit, a control, a registry
/// entry, a controller, an agent, a Character, a hint, a post, a hull).
pub type Obj = u64;

/// `{ unit, generation }`: a weak reference (perception.js:323 `refOf`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ref {
    pub obj: Obj,
    pub generation: f64,
}

/// A unit's broadphase sphere (`unit.sphere`: its lazy hit shapes'
/// bounds for a soldier, the hull's, turret's or building's for an entry).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sphere {
    pub center: Vector3,
    pub radius: f64,
}

/// Perception's scalar observations borrow the current body without copying
/// an entire combat view. Replay worlds retain their original unit reads.
#[derive(Clone,Copy,Debug,Default,PartialEq)]
pub struct UnitScalars {pub generation:f64,pub kind:f64,pub team:f64,pub alive:bool,pub position:Vector3,
    pub hero:bool,pub human:bool,pub intent:bool,pub intent_buttons:bool,pub targetable_false:bool}
impl UnitScalars {
    pub fn of(unit:&UnitView)->Self {Self {generation:unit.generation,kind:unit.kind,team:unit.team,alive:unit.alive,position:unit.position,
        hero:unit.hero,human:unit.human,intent:unit.intent,intent_buttons:unit.intent_buttons,targetable_false:unit.targetable_false}}
}

/// targetPriority observes only classification and tow ownership. It does
/// not need the candidate's arsenal, posture or complete lease frame.
#[derive(Clone,Copy,Debug,Default,PartialEq)]
pub struct PriorityView {pub kind:f64,pub building_post:Option<Obj>,pub pilot:bool,pub vehicle:Option<Obj>,pub tow_on:bool,pub towed:Option<Obj>}
impl PriorityView {
    pub fn of(u:&UnitView)->Self {Self {kind:u.kind,building_post:u.building_post,pilot:u.pilot,vehicle:u.vehicle,tow_on:u.tow_on,towed:u.towed}}
}
/// Keep raw health presence until validTarget reaches its numeric test.
#[derive(Clone,Debug,Default,PartialEq)]
pub struct TargetStatus {pub alive:bool,pub targetable_false:bool,pub combat_max_health:Js,pub class_max_health:Js}
impl TargetStatus {
    pub fn of(u:&UnitView)->Self {Self {alive:u.alive,targetable_false:u.targetable_false,combat_max_health:u.combat_max_health.clone(),
        class_max_health:u.class.as_ref().map(|c|c.max_health.clone()).unwrap_or_default()}}
}

/// A class's fields this package reads (`unit.classDef`), as the
/// JavaScript values they are (undefined where the class lacks one).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassView {
    pub fields:Rc<ClassFields>,
    /// Native load-time classifications; captures retain the raw source fields.
    pub prepared_role:Option<f64>,
    pub prepared_category:Option<f64>,
}
/// Immutable classDef values are shared by every scalar combat observation.
/// Prepared classifications stay on the small view because registry kind can
/// change category without changing any authored class field.
#[derive(Clone,Debug,Default,PartialEq)]
pub struct ClassFields {
    pub forward_speed: Js,
    pub max_speed: Js,
    pub max_health: Js,
    pub health_type: Js,
    pub jet_jump: Js,
    pub jet_fuel_min_border: Js,
    pub unit_type: Js,
    pub role: Js,
    /// `classDef.ai?.role`.
    pub ai_role: Js,
}
impl std::ops::Deref for ClassView {
    type Target=ClassFields;
    fn deref(&self)->&ClassFields {&self.fields}
}

/// A droideka's shield (`soldier.shield`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShieldView {
    pub on: bool,
    pub max: Option<f64>,
    pub value: Option<f64>,
}

/// A body's fields this package reads (`unit.soldier`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoldierView {
    /// `soldier.air` is truthy.
    pub air: bool,
    /// `soldier.posture` (a string for soldiers: stand, crouch, prone).
    pub posture: Js,
    pub shield: Option<ShieldView>,
    /// `soldier.jet` is there, and its `jump` and `minBorder`.
    pub jet: bool,
    pub jet_jump: Js,
    pub jet_min_border: Js,
    pub fuel: Js,
    /// `soldier.maxSpeed` (the Enhanced aim's top speed).
    pub max_speed: Js,
}

/// A seat lease as the alerts read it (src/seats.js SeatLease).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LeaseView {
    pub obj: Obj,
    pub human: bool,
    pub released: bool,
    /// `lease.live === false`.
    pub live_false: bool,
    /// `lease.controllable?.controller` (with `?.notifyShot`: a controller).
    pub controllable_controller: Option<Obj>,
    /// `lease.unit?.controller`.
    pub unit_controller: Option<Obj>,
}

/// One weapon of an arsenal (src/weapons.js Weapon), as this package reads
/// it, and the AI settings it writes on it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WeaponView {
    /// `weapon.def`: the class at its current level.
    pub def: Rc<JsObj>,
    /// `weapon.levels` (None: missing).
    pub levels: Option<Rc<Vec<Rc<JsObj>>>>,
    pub channel: Js,
    pub clip: f64,
    pub reserve: f64,
    /// `weapon.rounds` (the class's RoundsPerClip as written).
    pub rounds: Js,
    pub bottomless: bool,
    pub heat: f64,
    pub overheated: bool,
    pub charging: bool,
    pub held: f64,
    pub wait: f64,
    /// `weapon.limit` (None: undefined).
    pub limit: Option<f64>,
    /// The `reloading` and `busy` getters.
    pub reloading: bool,
    pub busy: bool,
    // What the AI writes.
    pub spread_scale: f64,
    /// `weapon.sniperScale` (undefined reads as 0 where it is read).
    pub sniper_scale: Option<f64>,
    pub yaw_spread_override: Option<f64>,
    pub kick: f64,
    pub floor: f64,
    /// The Enhanced aim's mark (src/ai/enhanced/aim.js playerSpread).
    pub enhanced_floor: bool,
}

/// The AI-owned settings on an actual weapon. Native edits touch only these
/// fields, never a copied arsenal or unit body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponSettings {
    pub spread_scale:f64, pub sniper_scale:Option<f64>, pub yaw_spread_override:Option<f64>,
    pub kick:f64, pub floor:f64, pub enhanced_floor:bool,
}
impl WeaponSettings {
    pub fn of(w:&WeaponView)->Self { Self { spread_scale:w.spread_scale,sniper_scale:w.sniper_scale,
        yaw_spread_override:w.yaw_spread_override,kick:w.kick,floor:w.floor,enhanced_floor:w.enhanced_floor } }
    pub fn player_spread(&mut self,def:&JsObj) {
        self.spread_scale=1.;self.sniper_scale=Some(0.);
        if !self.enhanced_floor {
            self.enhanced_floor=true;
            let threshold=def.get("SpreadThreshold");
            let floor=-(if threshold.truthy(){threshold.num()}else{0.});
            if floor<self.floor {self.floor=floor;self.kick=v8math::min(self.kick,floor);}
        }
    }
    pub fn apply(self,w:&mut WeaponView) {
        w.spread_scale=self.spread_scale;w.sniper_scale=self.sniper_scale;w.yaw_spread_override=self.yaw_spread_override;
        w.kick=self.kick;w.floor=self.floor;w.enhanced_floor=self.enhanced_floor;
    }
}

/// Which arsenal a unit holds: a soldier's (src/weapons.js Arsenal) or a
/// seat control's (src/seats.js seatArsenal), which select differently.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArsenalKind {
    #[default]
    Soldier,
    Seat,
}

/// `unit.combat.arsenal`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArsenalView {
    pub kind: ArsenalKind,
    pub weapons: Vec<WeaponView>,
    /// `channels[c]`: the weapons' indices in each channel's list.
    pub channels: Vec<Vec<usize>>,
    pub selected: Vec<f64>,
}

impl ArsenalView {
    /// `arsenal.current(channel)` (weapons.js:511; a seat's, seats.js:563).
    pub fn current(&self, channel: f64) -> Option<usize> {
        match self.kind {
            ArsenalKind::Soldier => {
                // channels[channel][selected[channel]] || null. A channel
                // outside the list throws in JavaScript; no caller asks.
                let c = index_of(channel)?;
                let list = self.channels.get(c).expect("arsenal.current: a channel the arsenal has");
                let s = self.selected.get(c).copied().unwrap_or(f64::NAN);
                index_of(s).and_then(|i| list.get(i).copied())
            }
            ArsenalKind::Seat => {
                // channels[channel ?? 0]?.[0] || null
                let c = index_of(channel)?;
                self.channels.get(c).and_then(|l| l.first().copied())
            }
        }
    }
}

/// A JavaScript number used as an array index (a non-negative integer).
pub fn index_of(x: f64) -> Option<usize> {
    if x >= 0.0 && x.fract() == 0.0 && x < 4294967295.0 {
        Some(x as usize)
    } else {
        None
    }
}

/// A unit, a seat control or a registry entry, as this package reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnitView {
    pub obj: Obj,
    /// Registry id; cached repair targets are plain objects without one.
    pub id: Option<f64>,
    pub generation: f64,
    pub alive: bool,
    pub team: f64,
    pub kind: f64,
    pub human: bool,
    pub hero: bool,
    /// `unit.trap` is truthy.
    pub trap: bool,
    /// `unit.targetable === false`.
    pub targetable_false: bool,
    /// `unit.collisionCategory` (undefined on every unit today).
    pub collision_category: Option<f64>,
    /// `unit.category` when it is an integer (entries keep one).
    pub category: Option<f64>,
    pub position: Vector3,
    /// `unit.velocity` (None: undefined).
    pub velocity: Option<Vector3>,
    pub class: Option<ClassView>,
    /// `unit.combat?.maxHealth` and `unit.maxHealth`.
    pub combat_max_health: Js,
    pub max_health: Js,
    pub soldier: Option<SoldierView>,
    /// `unit.combat?.arsenal` (None: no combat or no arsenal).
    pub arsenal: Option<ArsenalView>,
    /// `typeof unit.origin === "function"`.
    pub has_origin: bool,
    pub controller: Option<Obj>,
    pub character: Option<Obj>,
    /// `unit.pilot` (a control in a hull's pilot seat).
    pub pilot: bool,
    /// `unit.vehicle` (a control's or a hull entry's hull).
    pub vehicle: Option<Obj>,
    /// The hull's tow cable target as targetPriority reads it:
    /// `hull.towTarget >= 0` and `hull.system?.vehicles?.[hull.towTarget]`.
    pub tow_on: bool,
    pub towed: Option<Obj>,
    /// `unit.turret` (a control's or a turret entry's turret).
    pub turret: Option<Obj>,
    /// `unit.building?.post` (a destructible post's building entry).
    pub building_post: Option<Obj>,
    /// `unit.seat` (the lease a soldier holds, or a control's own).
    pub seat: Option<LeaseView>,
    /// `unit.lease` is there (a control).
    pub has_lease: bool,
    /// The leases of `unit.turret ?? unit.vehicle` (a hull's seats' in
    /// order, or the turret's one), None per empty seat.
    pub body_leases: Option<Vec<Option<LeaseView>>>,
    /// `unit.intent` is there, and its buttons.
    pub intent: bool,
    pub intent_buttons: bool,
}

impl UnitView {
    /// Scalar facts at one observation. Arsenal and body leases are native
    /// services with their own current reads; copying this view never copies
    /// either table or evaluates lazy geometry.
    pub fn observation(&self)->Self {
        Self {
            obj:self.obj.clone(),
            id:self.id.clone(),
            generation:self.generation.clone(),
            alive:self.alive.clone(),
            team:self.team.clone(),
            kind:self.kind.clone(),
            human:self.human.clone(),
            hero:self.hero.clone(),
            trap:self.trap.clone(),
            targetable_false:self.targetable_false.clone(),
            collision_category:self.collision_category.clone(),
            category:self.category.clone(),
            position:self.position.clone(),
            velocity:self.velocity.clone(),
            class:self.class.clone(),
            combat_max_health:self.combat_max_health.clone(),
            max_health:self.max_health.clone(),
            soldier:self.soldier.clone(),
            arsenal:None,
            has_origin:self.has_origin.clone(),
            controller:self.controller.clone(),
            character:self.character.clone(),
            pilot:self.pilot.clone(),
            vehicle:self.vehicle.clone(),
            tow_on:self.tow_on.clone(),
            towed:self.towed.clone(),
            turret:self.turret.clone(),
            building_post:self.building_post.clone(),
            seat:self.seat.clone(),
            has_lease:self.has_lease.clone(),
            body_leases:None,
            intent:self.intent.clone(),
            intent_buttons:self.intent_buttons.clone(),
        }
    }
}

/// A cover hint as combat reads it (src/ai/classic/nav/hints.js records).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HintView {
    pub obj: Obj,
    pub position: Vector3,
    pub forward: Vector3,
    /// `primaryMask`, `secondaryMask`, `sideMask` (None: undefined).
    pub primary_mask: Option<f64>,
    pub secondary_mask: Option<f64>,
    pub side_mask: Option<f64>,
    /// `hint.siblings` (the ring's other hints, in order).
    pub siblings: SharedRows<Obj>,
}

/// `world.difficulty` as the combat code reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DifficultyView {
    pub present: bool,
    pub enemy_skill: Option<f64>,
    /// `difficulty.hard` when it is set (a boolean), else `tier === 3`.
    pub hard: Option<bool>,
    pub tier: Option<f64>,
    /// `difficulty.dangerRow?.()`.
    pub danger_row: Option<(f64, f64)>,
}

/// The movement request kinds combat asks of a helper's goto (goto.js
/// options.mode: absent is a route).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GotoMode {
    Route,
    Direct,
}

/// The side's aim curves (the controller's `difficulty`, or the world's
/// for its team): A and D for the filter, B and S for the spreads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Curves {
    pub a: f64,
    pub d: f64,
    /// `curves.B` / `curves.S` (None: undefined).
    pub b: Option<f64>,
    pub s: Option<f64>,
}

/// A targeted-shot or damage alert on a controller (controller.js:419).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShotAlert {
    pub pending: bool,
    pub known: bool,
    pub point: Vector3,
    /// The source (a Character, or a unit where the attacker was not one).
    pub source: Option<Obj>,
}

/// A danger alert on a controller (controller.js:418).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DangerAlert {
    pub pending: bool,
    pub point: Vector3,
}

/// A jet request (controller.js:399, the controller's or its control's).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct JetRequest {
    pub pending: bool,
    pub vector: Vector3,
}

/// A controller (src/ai/classic/controller.js UnitController) as this
/// package reads and writes it: the control core holds one per controller.
/// `perception` is this package's own (created at each bind).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ctl {
    /// Native Core state for charge policy callbacks.
    pub controller_state:f64,
    pub obj: Obj,
    pub unit: Option<Obj>,
    pub generation: f64,
    pub character: Option<Obj>,
    /// `controller.objective` (its Character's objective post).
    pub objective: Option<Obj>,
    pub perception: Option<Perception>,
    /// The firing target (`controller.target`, set by setTarget).
    pub target: Option<Ref>,
    pub fire_a: bool,
    pub fire_b: bool,
    pub roll: f64,
    /// `controller.rangeScale` (None: undefined).
    pub range_scale: Option<f64>,
    pub aim_state: f64,
    pub aim_point: Vector3,
    pub aim_target: Option<Ref>,
    pub stance: f64,
    /// The navigation control's stance, when the controller has one.
    pub nav_stance: Option<f64>,
    pub facing: bool,
    pub wanted_mode: f64,
    /// `controller.dodge` (undefined until combat sets it).
    pub dodge: Option<f64>,
    /// `controller.alertAnim` (undefined until combat sets it).
    pub alert_anim: Option<bool>,
    pub shot: ShotAlert,
    pub danger: DangerAlert,
    pub acoustic: Vec<Ref>,
    /// `controller.difficulty` (the curves its team had at its bind).
    pub difficulty: Option<Curves>,
    /// The jet request requestJet writes: the navigation control's when
    /// it has one, else the controller's own.
    pub jet: JetRequest,
}

impl Ctl {
    /// controller.js:381 `lookAt`.
    pub fn look_at(&mut self, point: &Vector3) {
        self.aim_state = 1.0;
        self.aim_point = *point;
    }

    /// controller.js:390 `lookAlongPath`.
    pub fn look_along_path(&mut self) {
        self.aim_state = 2.0;
    }

    /// controller.js:396 `setStance`.
    pub fn set_stance(&mut self, stance: f64) {
        self.stance = stance;
        if self.nav_stance.is_some() {
            self.nav_stance = Some(stance);
        }
    }

    /// controller.js:386 `aimAt(ref)`.
    pub fn aim_at(&mut self, r: Option<Ref>) {
        self.aim_state = 0.0;
        self.aim_target = r;
    }

    /// controller.js:439 `notifyShot`.
    pub fn notify_shot(&mut self, source: Option<Obj>, known: bool, point: Option<Vector3>) {
        let Some(source) = source else { return };
        self.shot.pending = true;
        self.shot.source = Some(source);
        if known {
            if let Some(p) = point {
                self.shot.known = true;
                self.shot.point = p;
            }
        }
    }

    /// controller.js:432 `notifyDanger`.
    pub fn notify_danger(&mut self, point: &Vector3) {
        self.danger.pending = true;
        self.danger.point = *point;
    }

    /// controller.js:449 `addAcoustic` (20 at most).
    pub fn add_acoustic(&mut self, r: Ref) {
        if self.acoustic.len() < 20 {
            self.acoustic.push(r);
        }
    }
}

/// The vehicle AI's dispatch answer for a walker, flyer or hover
/// (src/ai/classic/vehicles/patterns.js through world.vehicles.dispatch):
/// what it wrote into the dispatcher's `out`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Engagement {
    pub pattern: f64,
    pub probability: f64,
    pub aux: bool,
    pub action: f64,
    pub in_band: bool,
    /// The chosen weapon's index in the unit's arsenal.
    pub weapon: Option<usize>,
}

/// A query's filter (the closures the JavaScript hands the registry's
/// query): evaluated by the world for each living member its broadphase
/// keeps, with [`QueryFilter::keeps`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QueryFilter {
    /// `u !== except && relation(team, u.team) < 0` (scan, acoustic).
    Hostile { except: Obj, team: f64 },
    /// `u !== except && relation(team, u.team) !== 0` (the cluster scan).
    NotNeutral { except: Obj, team: f64 },
    /// `u !== except && relation(team, u.team) >= 0` (protected humans).
    AllyOrNeutral { except: Obj, team: f64 },
    /// Core queryAllies excludes neutral members before its first-thirty cap.
    Ally {except:Obj,team:f64},
    /// `u !== except` (grenade danger; `except` may be nobody).
    Except { except: Option<Obj> },
    /// `u !== a && u !== b` (boarding's hostile check).
    ExceptTwo { a: Obj, b: Option<Obj> },
}

impl QueryFilter {
    /// The filter's answer for member `u` of team `team`, given the
    /// registry's relation.
    pub fn keeps(&self, u: Obj, team: f64, relation: &mut dyn FnMut(f64, f64) -> f64) -> bool {
        match *self {
            QueryFilter::Hostile { except, team: t } => u != except && relation(t, team) < 0.0,
            QueryFilter::NotNeutral { except, team: t } => u != except && relation(t, team) != 0.0,
            QueryFilter::Ally {except,team:t}=>u!=except&&relation(t,team)>0.,
            QueryFilter::AllyOrNeutral { except, team: t } => u != except && relation(t, team) >= 0.0,
            QueryFilter::Except { except } => Some(u) != except,
            QueryFilter::ExceptTwo { a, b } => u != a && Some(u) != b,
        }
    }
}

/// The draws: the AI's shared generator A, the game's B, and the Enhanced
/// aim's own (C), each a ClassicRandom (src/ai/random.js) the world keeps;
/// `int` and `range` are its methods over the uniform drawn.
pub trait Draws {
    fn uniform_a(&mut self) -> f64;
    fn uniform_b(&mut self) -> f64;
    fn uniform_c(&mut self) -> f64;
}

/// random.js:41 `int(lo, hi)` on generator `which` (no draw when hi <= lo).
pub fn draw_int(d: &mut (impl Draws + ?Sized), which: Gen, lo: f64, hi: f64) -> f64 {
    if !(hi > lo) {
        return lo;
    }
    let u = uniform(d, which);
    v8math::trunc(v8math::fround(v8math::fround(u * v8math::fround(hi - lo + 1.0)) + lo))
}

/// random.js:49 `range(lo, hi)`.
pub fn draw_range(d: &mut (impl Draws + ?Sized), which: Gen, lo: f64, hi: f64) -> f64 {
    let u = uniform(d, which);
    v8math::fround(v8math::fround(lo) + v8math::fround(u * v8math::fround(hi - lo)))
}

/// Which generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gen {
    A,
    B,
    C,
}

/// A uniform from generator `which`.
pub fn uniform(d: &mut (impl Draws + ?Sized), which: Gen) -> f64 {
    match which {
        Gen::A => d.uniform_a(),
        Gen::B => d.uniform_b(),
        Gen::C => d.uniform_c(),
    }
}

/// Perception's scan and hearing passes observe no class, lease or weapon
/// snapshot. Keep their source read as one compact value over the body owner.
#[derive(Clone,Copy,Debug,Default,PartialEq)]
pub struct UnitSensor {
    pub generation:f64,pub kind:f64,pub team:f64,pub human:bool,
    pub position:Vector3,pub velocity:Option<Vector3>,pub controller:Option<Obj>,
    pub collision_category:Option<f64>,
}
impl UnitSensor {
    pub fn of(u:&UnitView)->Self {Self {generation:u.generation,kind:u.kind,team:u.team,human:u.human,
        position:u.position,velocity:u.velocity,controller:u.controller,collision_category:u.collision_category}}
}
/// boundRadius treats an airborne soldier and every unrecognized posture as
/// standing; its fallback sphere remains an observation at the original point.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum BoundPosture {Stand,Crouch,Prone}
impl BoundPosture {
    pub fn of(u:&UnitView)->Option<Self> {
        if u.kind!=0. {return None;}
        let s=u.soldier.as_ref()?;
        Some(if s.air {Self::Stand}else{match s.posture.str() {Some("crouch")=>Self::Crouch,Some("prone")=>Self::Prone,_=>Self::Stand}})
    }
}

/// What the AI's perception and combat code asks of the battle.
pub trait World: Draws {
    // -- Clock and settings.
    /// `world.time ?? 0` (simulation time in seconds).
    fn time(&self) -> f64;
    fn difficulty(&self) -> DifficultyView;
    /// `world.mission?.stayInTurrets` (truthy) and `noJetObstacleTest`.
    fn mission_stay_in_turrets(&self) -> bool;
    fn mission_no_jet_obstacle_test(&self) -> bool;
    /// The control core's `ai.stayInTurrets()` (its mission controls).
    fn ai_stay_in_turrets(&mut self) -> bool;

    // -- Units.
    fn controller_objective(&self,c:Obj)->Option<Obj> {self.ctl(c).objective}
    fn unit(&self, u: Obj) -> UnitView;
    fn unit_sensor(&self,u:Obj)->UnitSensor {UnitSensor::of(&self.unit(u))}
    /// expandedRange observes kind and the authored class speed together.
    fn unit_weapon_motion(&self,u:Obj)->(f64,f64) {let view=self.unit(u);(view.kind,super::weapons_ai::forward_speed(&view))}
    fn unit_bound_posture(&self,u:Obj)->Option<BoundPosture> {BoundPosture::of(&self.unit(u))}
    fn unit_scalars(&self,u:Obj)->UnitScalars {UnitScalars::of(&self.unit(u))}
    /// aim.leadPoint and selectedWeapon observe these before any service call.
    fn unit_velocity(&self,u:Obj)->Option<Vector3> {self.unit(u).velocity}
    fn unit_controller(&self,u:Obj)->Option<Obj> {self.unit(u).controller}
    fn unit_life(&self,u:Obj)->(bool,f64) {let u=self.unit(u);(u.alive,u.generation)}
    fn unit_priority(&self,u:Obj)->PriorityView {PriorityView::of(&self.unit(u))}
    fn unit_target_status(&self,u:Obj)->TargetStatus {TargetStatus::of(&self.unit(u))}
    fn unit_has_origin(&self,u:Obj)->bool {self.unit(u).has_origin}
    /// consumeFire tests only the presence of the intent button pair.
    fn unit_has_buttons(&self,u:Obj)->bool {let u=self.unit(u);u.intent&&u.intent_buttons}
    fn has_arsenal(&self,u:Obj)->bool;
    fn weapon_count(&self,u:Obj)->usize;
    fn current_weapon(&self,u:Obj,channel:f64)->Option<usize>;
    fn weapon(&self,u:Obj,index:usize)->Option<WeaponView>;
    /// A captured weapon observation precedes burstRequest's early return.
    /// Native owners can omit unused runtime fields for an idle request.
    fn weapon_for_burst(&self,u:Obj,index:usize,_active:bool)->Option<WeaponView> {Some(self.weapon(u,index).expect("arsenal member"))}
    /// chargeWeapon classifies level zero, independently of the charged level.
    fn weapon_chargeable(&self,u:Obj,index:usize)->bool {
        super::weapons_ai::charge_weapon(&self.weapon(u,index).expect("arsenal member"))
    }
    /// Definition-only observations do not need clocks, ammunition or spread.
    fn weapon_definition(&self,u:Obj,index:usize)->Option<Rc<JsObj>> {self.weapon(u,index).map(|weapon|weapon.def)}
    /// maxExpandedRange visits charge levels in their original order.
    fn visit_weapon_levels(&self,u:Obj,index:usize,visit:&mut dyn FnMut(&JsObj)) {
        let weapon=self.weapon(u,index).expect("arsenal member");
        match &weapon.levels {Some(levels)=>for level in levels.iter() {visit(level)},None=>visit(&weapon.def)}
    }
    fn body_lease_count(&self,u:Obj)->usize;
    fn body_lease(&self,u:Obj,index:usize)->Option<LeaseView>;
    /// prepareSpread clears and optionally restores yaw as separate writes.
    fn set_weapon_yaw(&mut self,u:Obj,index:usize,value:Option<f64>) {self.edit_weapon(u,index,|w|w.yaw_spread_override=value);}
    fn weapon_settings(&self,u:Obj,index:usize)->WeaponSettings;
    fn edit_weapon<R>(&mut self,u:Obj,index:usize,edit:impl FnOnce(&mut WeaponSettings)->R)->R;
    /// `unit.origin(out)`: where it looks from.
    fn origin(&mut self, u: Obj) -> Vector3;
    /// `unit.forward(out)`: its aim direction.
    fn forward(&mut self, u: Obj) -> Vector3;
    /// `unit.aimPoint(out)`: the point others aim at.
    fn aim_point(&mut self, u: Obj) -> Vector3;
    /// `unit.sphere` (a getter; lazy for a soldier).
    fn sphere(&mut self, u: Obj) -> Sphere;
    /// Writes `unit.intent.buttons[0..2]` (consumeFire).
    fn set_intent_buttons(&mut self, u: Obj, b0: f64, b1: f64);
    /// Writes `unit.intent.aim` (the aim services).
    fn set_intent_aim(&mut self, u: Obj, aim: Option<Vector3>);
    /// `arsenal.select(weapon)` for the weapon at `index` (a soldier's:
    /// refused while the channel's weapon is busy; a seat's: always).
    fn arsenal_select(&mut self, u: Obj, index: usize) -> bool;
    /// useClassicRandom: the unit's weapons draw their spread from
    /// generator A from now on (weapons-ai.js:999).
    fn use_classic_random(&mut self, u: Obj);

    // -- The registry.
    /// A scoped result buffer reserved at world construction.
    fn object_buffer(&self) -> List<Obj>;
    /// `units.query(center, radius, out, filter)`: living members by their
    /// broadphase sphere, in registry order, filtered.
    fn query(&mut self, center: &Vector3, radius: f64, filter: QueryFilter) -> List<Obj>;
    /// `units.relation(a, b)`.
    fn relation(&mut self, a: f64, b: f64) -> f64;
    /// `units.living()`: every living member, in registry order.
    fn living(&mut self) -> List<Obj>;
    /// game.js `los(from, to, a, b)`: the segment is clear.
    fn los(&mut self, from: &Vector3, to: &Vector3, a: Option<Obj>, b: Option<Obj>) -> bool;

    // -- Controllers and agents (the control core's).
    /// The control core's `ai.curvesFor(team)` (its difficulty's curves).
    fn curves_for(&mut self, team: f64) -> Option<Curves>;
    /// A Character's team (the damage alert's source team).
    fn character_team(&self, character: Obj) -> f64;
    fn fresh_perception(&mut self,c:Obj)->super::perception::Perception;
    fn ctl(&self, c: Obj) -> &Ctl;
    fn ctl_mut(&mut self, c: Obj) -> &mut Ctl;
    /// Native controls receive the aliased navigation write immediately.
    fn set_stance(&mut self,c:Obj,stance:f64) {self.ctl_mut(c).set_stance(stance);}
    /// An agent's controller.
    fn agent_controller(&self, agent: Obj) -> Obj;
    /// An agent's target tracker (every agent has one, from initAgent).
    fn tracker(&mut self, agent: Obj) -> Option<&mut super::perception::TargetTracker>;
    /// A unit's or control's controller has the alert intake
    /// (`controller?.addAcoustic`, `?.notifyShot`, `?.notifyDanger`):
    /// every UnitController does.
    fn is_controller(&self, c: Obj) -> bool;
    /// The active agent's policies (agents.js and its subclasses), as the
    /// responses ask them (`agent?.acceptsX?.(...)`).
    fn agent_accepts_alert(&mut self, agent: Obj, known: bool, point: &Vector3) -> bool;
    fn agent_accepts_hazard(&mut self, agent: Obj) -> bool;
    fn agent_accepts_acoustic(&mut self, agent: Obj, source: Obj) -> bool;

    // -- The vehicle AI's hooks (world.vehicles, ai.vehicles).
    /// `world.vehicles?.sharedTarget?.(controller) ?? null`.
    fn shared_target(&mut self, controller: Obj) -> Option<Obj>;
    /// `world.vehicles.dispatch` is bound (walkers, flyers, hovers).
    fn has_vehicle_dispatch(&self) -> bool;
    fn vehicle_dispatch(&mut self, shared: &mut super::weapons_ai::Shared, controller: Obj, unit: Obj, target: Obj, switch_allowed: bool) -> Engagement;
    /// `ai.vehicles?.mountReach ?? world.vehicles?.mountReach` is bound,
    /// and its answer.
    fn has_mount_reach(&self) -> bool;
    fn mount_reach(&mut self, unit: Obj, point: &Vector3) -> bool;
    /// `ai.vehicles?.currentControllable?.(character)` (None: not bound or
    /// none; `bound` says which).
    fn current_controllable(&mut self, character: Obj) -> (bool, Option<Obj>);
    /// A Character's `remote` and `unit` (sourceUnit's fallbacks).
    fn character_remote(&self, character: Obj) -> Option<Obj>;
    fn character_unit(&self, character: Obj) -> Option<Obj>;
    /// Whether an alert source is itself a unit (`source.alive !==
    /// undefined && source.position`) rather than a Character.
    fn is_unit(&self, o: Obj) -> bool;
    /// The attack patterns' vehicle hooks (world.vehicles enterPattern,
    /// exitPattern, tickPattern, sidestep): they read and write the
    /// patterns (`agent`) as the vehicle AI does.
    fn enter_pattern(&mut self, agent: Obj, state: f64);
    fn exit_pattern(&mut self, agent: Obj, state: f64);
    fn tick_pattern(&mut self, agent: Obj, state: f64, dt: f64);
    fn sidestep(&mut self, agent: Obj);

    // -- Navigation (world.nav, ai.nav).
    /// The cover hints: `world.nav.hints.query(4, center, half, 30)`, else
    /// `ai.nav.queryHints({ mask: 4, center, halfExtent, cap: 30 })`.
    fn cover_hints(&mut self, center: &Vector3, half: f64) -> List<HintView>;
    /// The jet hints service (world.nav.jetHints), when there is one.
    fn has_jet_hints(&self) -> bool;
    fn jet_hints_excluded(&mut self, controller: Obj) -> bool;
    fn jet_hints_find(&mut self, controller: Obj) -> Option<Obj>;
    fn jet_hints_enter(&mut self, controller: Obj, hint: Option<Obj>);
    fn jet_hints_exit(&mut self, controller: Obj, hint: Option<Obj>);
    /// `tick(...) ?? true`.
    fn jet_hints_tick(&mut self, controller: Obj, hint: Option<Obj>, dt: f64) -> bool;
    /// `nav?.pointClear?.(point, unit)`, `nav?.segmentBlocked?.(...)`,
    /// `nav?.directClear?.(unit, point)`, `nav?.inDangerRegion?.(unit)`:
    /// None where the navigation has no such method (ClassicNav has none
    /// of them: quirks R1c).
    fn nav_point_clear(&mut self, point: &Vector3, unit: Obj) -> Option<bool>;
    fn nav_segment_blocked(&mut self, from: &Vector3, to: &Vector3, unit: Obj) -> Option<bool>;
    fn nav_direct_clear(&mut self, unit: Obj, point: &Vector3) -> Option<bool>;
    fn nav_in_danger_region(&mut self, unit: Obj) -> Option<bool>;

    // -- The agent's base (the control core's Agent) and its helper.
    /// The combat agent as it stands, handed to the world before a call
    /// that can read it (the interrupts ask its policies and its tracker;
    /// the vehicle AI's pattern hooks read and write its patterns and
    /// tracker), and taken back after (`agent_resync`): the world keeps the
    /// agent while it is not running.
    fn agent_sync(&mut self, agent: &super::agent::CombatAgent);
    fn agent_resync(&mut self, agent: &mut super::agent::CombatAgent);
    /// `this.interrupts?.tick?.()`: an interrupt took the agent over.
    // A native callback reborrows this invocation's services, including LOS;
    // it must not reacquire a globally borrowed combat owner.
    fn interrupts_tick(&mut self, agent: Obj,combat:&mut super::binding::CombatAi) -> bool;
    /// Agent.acceptsBoard's base policy (boardPolicy).
    fn agent_board_policy(&mut self, agent: Obj, candidate: Obj) -> bool;
    /// The agent's movement helper is there, `arrived`, `flying`, and has
    /// a `follow` method.
    fn has_helper(&self, agent: Obj) -> bool;
    fn helper_arrived(&mut self, agent: Obj) -> bool;
    fn helper_flying(&mut self, agent: Obj) -> bool;
    fn helper_has_follow(&self, agent: Obj) -> bool;
    fn helper_goto(&mut self, agent: Obj, point: &Vector3, mode: Option<GotoMode>);
    fn helper_stop(&mut self, agent: Obj);
    fn helper_tick(&mut self, agent: Obj, dt: f64);
    /// `helper.follow(target, { mode: 0, follow: 1 })`.
    fn helper_follow(&mut self, agent: Obj, target: Obj);
    /// Agent.exit's helper end (`helper.terminate ?? helper.stop`).
    fn helper_terminate(&mut self, agent: Obj);
    /// `this.controller.requestJet?.(vector)`.
    fn request_jet(&mut self, controller: Obj, vector: &Vector3);

    // -- Squads.
    /// squads.js emitVo(ai, unit, { event, property }).
    fn emit_vo(&mut self, unit: Obj, event: &str, property: &str);
}
