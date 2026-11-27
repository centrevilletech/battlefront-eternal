//! 2005 (`GameFamily::Swbf2`) starfighter flight model, hangar spawners,
//! takeoff/landing, tricks, pilot passive repair, multi-point hull collision,
//! and mobile transport spawns.
//!
//! Lives beside the released 2004 flight code (`crate::flight`) and runs only
//! for 2005 space battlefields. Fixed 60 Hz deterministic arithmetic using
//! `v8math` transcendentals only, with zero per-tick allocations.

use bfe_wide::Wide;
use super::combat::SpaceExplosionSpec;
use super::digest::Fnv;
use super::bounds::{SpaceBounds, OUT_OF_BOUNDS_TIME};
use crate::collision::spheres::{box_apply_matrix4, Bounds};
use crate::collision::{CollisionOwner, Hit, Ray, MISS};
use bfe_content::bvh::{build as build_bvh, Positions};
use bfe_content::install::CollisionKernel;
use bfe_content::js::to_number;
use bfe_content::vehicle_pack::{
    Swbf2CollisionGeometry, Swbf2VehicleModel, Swbf2VehiclePack, COLLISION_MASK_BUILDING,
    COLLISION_MASK_FLYER, COLLISION_MASK_ORDNANCE, COLLISION_MASK_SOLDIER,
};
use bfe_content::Value;
use three::{Matrix4, Quaternion, Vector3};

const PI: f64 = std::f64::consts::PI;
const TAU: f64 = std::f64::consts::TAU;
/// Degrees to radians, for the 2005 weapon angles authored in degrees.
const DEG: f64 = PI / 180.0;

/// Provisional linear drag applied to unoccupied drifting fighters in space
/// (1/s). Zero gravity applies in open space.
pub const PROVISIONAL_PILOTLESS_DRAG: f64 = 0.08;

/// Provisional rule for crew forced out of a destroyed airborne hull in open space.
pub const PROVISIONAL_SPACE_EJECT_KILLS_PILOT: bool = true;

/// Provisional minimum spacing around a hangar pad required to spawn a new
/// starfighter without overlapping an existing parked hull (metres).
pub const PROVISIONAL_PAD_CLEAR_RADIUS: f64 = 8.0;

/// Provisional trick cooldown between consecutive maneuvers (seconds).
pub const PROVISIONAL_TRICK_COOLDOWN: f64 = 0.5;

/// A landing craft sets down once its up axis is within this cosine of
/// straight up, as in the 2005 game.
pub const LANDING_LEVEL_COS: f64 = 0.9375;

/// Inside a landing region the 2005 game caps MinSpeed, MidSpeed and MaxSpeed
/// at these (m/s), MinSpeed after taking this share of it, and slows a craft
/// this many times faster than it speeds up.
pub const LANDING_REGION_SPEEDS: [f64; 3] = [10.0, 20.0, 60.0];
pub const LANDING_REGION_MIN_SPEED_SHARE: f64 = 0.2;
pub const LANDING_REGION_BRAKE: f64 = 3.0;
/// How far a craft taking off or landing looks straight down for the deck under it.
pub const DECK_RAY_REACH: f64 = 1024.0;

/// Provisional lethal nose-first collision speed threshold (m/s).
pub const PROVISIONAL_LETHAL_IMPACT_SPEED: f64 = 85.0;

/// Provisional vertical landing gear clearance above the hangar floor (metres).
pub const PROVISIONAL_GEAR_CLEARANCE_Y: f64 = 0.15;

/// Provisional settling tolerance when transitioning from `TakingOff` or
/// `Landing` (metres).
pub const PROVISIONAL_LANDING_SETTLE_EPS: f64 = 0.05;

/// Provisional hull collision damage scale multiplier applied to
/// `(impact_speed - collision_threshold) * collision_scale`.
pub const PROVISIONAL_COLLISION_DAMAGE_FACTOR: f64 = 10.0;

/// Provisional forward step past a hangar forcefield plane during flyer sweep
/// raycasts (metres).
pub const PROVISIONAL_FORCEFIELD_STEP_PAD: f64 = 0.5;

/// Provisional minimum lateral dismount offset beside a landed fighter (metres).
pub const PROVISIONAL_DISMOUNT_MIN_OFFSET: f64 = 2.5;

/// Provisional wingtip clearance added to lateral dismount offset (metres).
pub const PROVISIONAL_DISMOUNT_WING_PAD: f64 = 1.5;

/// Provisional radial spread for fallback mobile spawn points when no explicit
/// `SpawnPointLocation` offsets are authored (metres).
pub const PROVISIONAL_MOBILE_SPAWN_SPREAD: f64 = 2.5;

/// Provisional minimum forward hull sweep probe pad (metres).
pub const PROVISIONAL_MIN_SWEEP_PROBE: f64 = 0.5;

/// Provisional minimum fallback muzzle forward offset when a weapon has no
/// authored vehicle-pack fire points (metres).
pub const PROVISIONAL_DEFAULT_MUZZLE_PAD: f64 = 1.0;

/// Default visual bank angle when a starfighter class omits `BankAngle`
/// (radians; visual lean only, does not steer heading).
pub const PROVISIONAL_DEFAULT_BANK_ANGLE: f64 = 0.5;

/// Default visual bank filter rate when a starfighter class omits `BankFilter`
/// (1/s).
pub const PROVISIONAL_DEFAULT_BANK_FILTER: f64 = 2.0;

/// Default roll rate filter when a starfighter class omits `RollRateFilter`
/// (1/s).
pub const PROVISIONAL_DEFAULT_ROLL_RATE_FILTER: f64 = 4.0;

/// Default momentum filter when a starfighter class omits `MomentumFilter`
/// (1/s).
pub const PROVISIONAL_DEFAULT_MOMENTUM_FILTER: f64 = 8.0;

/// Default deceleration slew rate when a starfighter class omits
/// `TurnFilterDecel` or `PitchFilterDecel` (rad/s^2).
pub const PROVISIONAL_DEFAULT_FILTER_DECEL: f64 = 10.0;

/// Provisional fallback cruise-speed fraction of `BoostSpeed` for guided
/// rockets when `MaxSpeed` is omitted.
pub const PROVISIONAL_GUIDED_CRUISE_FRAC: f64 = 0.75;

/// Provisional fallback lifespan for guided rockets when `LifeSpan` is omitted (s).
pub const PROVISIONAL_GUIDED_DEFAULT_LIFE_S: f64 = 8.0;

/// Four-state 2005 flyer lifecycle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum SpaceFlightState {
    #[default]
    Landed = 0,
    TakingOff = 1,
    Flying = 2,
    Landing = 3,
}

/// What meets a craft's hull. Each body of the pack is marked with what it
/// collides with, and the 2005 game picks a craft's bodies by what meets it:
/// shots the ordnance bodies, and the flyer bodies too while the craft flies;
/// soldiers the soldier bodies; another craft the building bodies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HullMeets {
    Shots,
    Soldiers,
    Craft,
}

/// The fixed-step landing decision used by the pilot action and battle display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpaceLandingPermission {
    Unavailable,
    Maneuver,
    OutsideRegion,
    NoFloor,
    Allowed { zone: usize, floor_y: f64 },
}

/// Active aerobatic maneuver on a 2005 starfighter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum SpaceTrick {
    #[default]
    None = 0,
    BarrelLeft = 1,
    BarrelRight = 2,
    Loop = 3,
    Immelmann = 4,
}

impl SpaceTrick {
    #[inline]
    pub fn active(self) -> bool {
        self != Self::None
    }
}

/// Pilot identity seated in a 2005 starfighter or turret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpacePilotId {
    /// Immutable character, source life and team captured at boarding.
    pub owner: super::SpaceOwner,
    pub team: i32,
    pub is_human: bool,
    /// True when the pilot class has passive vehicle repair capability.
    pub repair_capable: bool,
}

/// Per-tick pilot control input for a 2005 starfighter.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpacePilotInput {
    /// Throttle in `[-1.0, 1.0]` (-1 min speed, 0 cruise/mid, +1 max speed).
    pub throttle: f64,
    /// Yaw stick in `[-1.0, 1.0]` (+right, -left).
    pub stick_x: f64,
    /// Pitch stick in `[-1.0, 1.0]` (+nose up, -nose down).
    pub stick_y: f64,
    /// Roll command in `[-1.0, 1.0]`.
    pub roll: f64,
    /// Hold roll lock (remaps lateral stick into roll and suppresses yaw).
    pub roll_lock: bool,
    /// Hold afterburner / boost (also boosts active guided rocket).
    pub boost: bool,
    /// Edge-triggered takeoff / land request.
    pub takeoff_or_land: bool,
    /// Single-tap barrel roll direction: -1 left, 0 none, +1 right.
    pub trick_roll: i8,
    /// Double-tap vertical trick: 0 none, 1 loop, 2 Immelmann.
    pub trick_flip: i8,
    /// Hold to eject while airborne (or instant exit when landed).
    pub eject_held: bool,
    /// Fire primary weapon channel (0).
    pub fire_primary: bool,
    /// Fire secondary weapon channel (1).
    pub fire_secondary: bool,
    /// Request or cycle missile lock-on.
    pub lock_target: bool,
    /// Profile weapon index selected in this seat. Without a selection each fire
    /// channel uses its first mounted weapon.
    pub weapon_index: Option<u8>,
    /// World-space gunner sight direction. The pilot uses the hull attitude.
    pub aim: Option<[f64; 3]>,
    /// The spread scale a bot's skill hands the seat's guns: it multiplies the
    /// kick each salvo adds. `None` is 1; a player's guns take no scale.
    pub spread_scale: Option<f64>,
}

/// Parsed weapon / ordnance / explosion descriptor carried by a starfighter or
/// ship turret class.
#[derive(Clone, Debug)]
pub struct SpaceWeaponSpec {
    pub name: String,
    pub hud_tag: String,
    pub channel: u8,
    pub seat: u8,
    pub ordnance_kind: SpaceOrdnanceKind,
    pub shot_delay: f64,
    pub salvo_count: u32,
    /// Rounds each salvo fires at once (`ShotsPerSalvo`), each from the next
    /// fire point with its own spread draw and its own share of the clip.
    pub shots_per_salvo: u32,
    /// The time from one salvo of a shot to the next: a random one between the
    /// two values of `SalvoDelay` (one value is both).
    pub salvo_delay: f64,
    pub salvo_delay_max: f64,
    pub initial_salvo_delay: f64,
    pub barrage_min: u32,
    pub barrage_max: u32,
    pub barrage_delay: f64,
    /// The spread each shot draws, in radians: it leaves up to `pitch_spread`
    /// above or below its aim and up to `yaw_spread` to either side, each
    /// as the spread times the difference of two uniform draws (see
    /// `combat::apply_weapon_spread`). Heat does not change it.
    pub pitch_spread: f64,
    pub yaw_spread: f64,
    /// The kick each salvo adds to both spreads, in radians (`SpreadPerShot`,
    /// or the older `KickSpread`), up to `spread_limit` (`SpreadLimit`; a gun
    /// without one gathers none).
    pub spread_per_shot: f64,
    pub spread_limit: f64,
    /// How fast the kick falls, in radians a second (`SpreadRecoverRate`): to
    /// zero for a bot and to minus `spread_threshold` (`SpreadThreshold`) for a
    /// player, so a player's first salvos after a rest gather no spread.
    pub spread_recover_rate: f64,
    pub spread_threshold: f64,
    /// Seconds after the gun stops in which its kick does not fall
    /// (`CoolDownTime`).
    pub cool_down_time: f64,
    pub auto_aim_size: f64,
    pub rounds_per_clip: u32,
    pub reload_time: f64,
    pub heat_per_shot: f64,
    pub heat_recover_rate: f64,
    pub heat_threshold: f64,
    pub warm_up_time: f64,
    pub secondary_tracer_period: u32,
    pub lock_on_range: f64,
    pub lock_on_angle_deg: f64,
    pub lock_off_angle_deg: f64,
    pub lock_time: f64,
    pub target_lock_max_distance: f64,
    pub target_lock_max_distance_lose: f64,
    /// How far off a bot uses it (`MaxRange`).
    pub max_range: f64,
    /// Its rounds' speed; a beam's length, which it covers at once.
    pub velocity: f64,
    pub life_span: f64,
    pub gravity: f64,
    pub turn_rate: f64,
    pub waver_turn: f64,
    /// Whether a beam goes on behind the craft and soldiers it strikes.
    pub pass_through: bool,
    pub max_damage: f64,
    pub final_damage: Option<f64>,
    pub damage_transition_delay: f64,
    pub damage_transition_period: f64,
    pub person_scale: f64,
    pub animal_scale: f64,
    pub droid_scale: f64,
    pub vehicle_scale: f64,
    pub building_scale: f64,
    pub shield_scale: f64,
    pub guided_missile: bool,
    pub guided_damage: f64,
    pub guided_damage_boost: f64,
    pub guided_boost_threshold: f64,
    pub guided_boost_speed: f64,
    /// The blast its rounds set off where they strike.
    pub blast: SpaceExplosionSpec,
    /// The one they set off at the end of their life.
    pub expire_blast: SpaceExplosionSpec,
    /// The one a guided rocket sets off instead past its boost threshold.
    pub guided_blast_boost: SpaceExplosionSpec,
    /// Model-space weapon muzzle offsets resolved from `vehicles.json`.
    pub muzzles: Vec<[f64; 3]>,
    /// What a bot may use the weapon for: the authored `MinRange` and
    /// `OptimalRange` (metres; an absent optimum reads as half of `MaxRange`)
    /// and the `AITargetPerson`, `AITargetVehicle` and `AITargetBuilding` flags.
    pub ai_min_range: f64,
    pub ai_optimal_range: f64,
    pub ai_targets_person: bool,
    pub ai_targets_vehicle: bool,
    pub ai_targets_building: bool,
    /// How far a bot's aim strays with this weapon: `AIBubbleSizeMultiplier`
    /// scales the target's sphere of miss, which grows with the distance over
    /// `AIBubbleScaleDistDivider` up to `AIBubbleScaleClamp` times.
    pub ai_bubble: super::aim::MissBubble,
}

/// The weapon class keeps `AutoAimSize` times this: the cone, in radians,
/// within which a round is pulled onto a target.
pub const AUTO_AIM_SCALE: f64 = 0.072;

/// Fallback values for properties absent from a prepared definition.
pub const PROVISIONAL_WEAPON_DEFAULTS: SpaceWeaponSpec = SpaceWeaponSpec {
            name: String::new(),
            hud_tag: String::new(),
            channel: 0,
            seat: 0,
            ordnance_kind: SpaceOrdnanceKind::Bolt,
            shot_delay: 0.15,
            salvo_count: 1,
            shots_per_salvo: 1,
            salvo_delay: 0.0,
            salvo_delay_max: 0.0,
            initial_salvo_delay: 0.0,
            barrage_min: 0,
            barrage_max: 0,
            barrage_delay: 0.0,
            pitch_spread: 0.0,
            yaw_spread: 0.0,
            spread_per_shot: 0.0,
            spread_limit: 0.0,
            spread_recover_rate: 0.0,
            spread_threshold: 0.0,
            cool_down_time: 0.0,
            auto_aim_size: 0.0,
            rounds_per_clip: 0,
            reload_time: 0.0,
            heat_per_shot: 0.0,
            heat_recover_rate: 0.25,
            heat_threshold: 0.5,
            warm_up_time: 0.0,
            secondary_tracer_period: 0,
            lock_on_range: 0.0,
            lock_on_angle_deg: 6.0,
            lock_off_angle_deg: 10.0,
            lock_time: 0.5,
            target_lock_max_distance: 2000.0,
            target_lock_max_distance_lose: 2500.0,
            max_range: 1500.0,
            velocity: 1400.0,
            life_span: 1.5,
            gravity: 0.0,
            turn_rate: 0.0,
            waver_turn: 0.0,
            pass_through: false,
            max_damage: 250.0,
            final_damage: None,
            damage_transition_delay: 0.0,
            damage_transition_period: 0.0,
            person_scale: 1.0,
            animal_scale: 1.0,
            droid_scale: 1.0,
            vehicle_scale: 1.0,
            building_scale: 1.0,
            shield_scale: 1.0,
            guided_missile: false,
            guided_damage: 1500.0,
            guided_damage_boost: 5000.0,
            guided_boost_threshold: 0.8,
            guided_boost_speed: 225.0,
            blast: SpaceExplosionSpec::NONE,
            expire_blast: SpaceExplosionSpec::NONE,
            guided_blast_boost: SpaceExplosionSpec::NONE,
            muzzles: Vec::new(),
            ai_min_range: 0.0,
            ai_optimal_range: 0.0,
            ai_targets_person: PROVISIONAL_AI_TARGETS[0],
            ai_targets_vehicle: PROVISIONAL_AI_TARGETS[1],
            ai_targets_building: PROVISIONAL_AI_TARGETS[2],
            ai_bubble: super::aim::RELEASED_MISS_BUBBLE,
};

/// Person, vehicle and building use of a weapon whose definition sets no
/// `AITarget*` flag: the same split as the released game's default bot
/// category mask (people and vehicles, no buildings).
pub const PROVISIONAL_AI_TARGETS: [bool; 3] = [true, true, false];

impl Default for SpaceWeaponSpec {
    fn default() -> Self { PROVISIONAL_WEAPON_DEFAULTS }
}

impl SpaceWeaponSpec {
    /// The speed its target is led by and its rounds fall at: none for a beam,
    /// which strikes at once.
    pub fn round_speed(&self) -> f64 {
        if self.ordnance_kind == SpaceOrdnanceKind::Beam { 0.0 } else { self.velocity }
    }

    /// From the beginning of one shot of a held trigger to the next.
    pub fn shot_time(&self) -> f64 {
        let salvos = f64::from(self.salvo_count.max(1) - 1) * 0.5 * (self.salvo_delay + self.salvo_delay_max);
        self.initial_salvo_delay + salvos + self.shot_delay
    }

    /// How far its rounds get: a beam's length, else their speed over their life.
    pub fn round_reach(&self) -> f64 {
        if self.ordnance_kind == SpaceOrdnanceKind::Beam { self.velocity } else { self.velocity * self.life_span }
    }

    /// Whether a tracer goes out with its `round`th round, counted from 0: with
    /// the first and then every `SecondaryOrdnancePeriod`th after it.
    pub fn tracer(&self, round: u32) -> bool {
        self.secondary_tracer_period > 0 && round.is_multiple_of(self.secondary_tracer_period)
    }
}

// Lower limits used when integrating incomplete or unusually small authored values.
pub const PROVISIONAL_MIN_SHOT_INTERVAL: f64 = 0.02;
/// The 2005 game turns a weapon's `MaxSpread` into the spread each shot
/// draws: 0.025 of it, in radians, in pitch and 0.05 of it in yaw. It reads
/// no `MinSpread`, and heat does not widen the spread.
pub const MAX_SPREAD_PITCH: f64 = 0.025;
pub const MAX_SPREAD_YAW: f64 = 0.05;
pub const PROVISIONAL_MIN_LOCK_ANGLE: f64 = 0.5;
pub const PROVISIONAL_MIN_LOCK_DISTANCE: f64 = 100.0;
pub const PROVISIONAL_MIN_WEAPON_RANGE: f64 = 50.0;
pub const PROVISIONAL_MIN_ROUND_SPEED: f64 = 10.0;
pub const PROVISIONAL_MIN_ROUND_LIFE: f64 = 0.05;
pub const PROVISIONAL_MIN_GUIDED_BOOST_SPEED: f64 = 50.0;
pub const PROVISIONAL_MIN_GUIDED_SPEED: f64 = 20.0;
pub const PROVISIONAL_MIN_GUIDED_LIFE: f64 = 2.0;
pub const PROVISIONAL_MIN_CONTROL_RATE: f64 = 0.05;
pub const PROVISIONAL_MIN_CONTROL_FILTER: f64 = 0.5;
pub const PROVISIONAL_MIN_GEAR_TIME: f64 = 0.1;
pub const PROVISIONAL_MIN_GEAR_HEIGHT: f64 = 0.5;
pub const PROVISIONAL_MIN_FLYER_SPEED: f64 = 1.0;
pub const PROVISIONAL_MIN_ENERGY_CAPACITY: f64 = 1.0;
pub const PROVISIONAL_MIN_HULL_CAPACITY: f64 = 1.0;
pub const PROVISIONAL_MIN_FLYER_ACCELERATION: f64 = 1.0;

/// Gear extension at which pressing the action again can interrupt a landing.
pub const PROVISIONAL_LANDING_CANCEL_GEAR: f64 = 0.75;
/// A beam's length when its ordnance gives no `Range`.
pub const PROVISIONAL_BEAM_RANGE: f64 = 2000.0;

/// Authored ordnance archetype for 2005 space weapons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum SpaceOrdnanceKind {
    #[default]
    Bolt = 0,
    Missile = 1,
    BombShell = 2,
    Beam = 3,
    GuidedRocket = 4,
    TimeBomb = 5,
    RepairBeam = 6,
}

/// Immutable per-class flight and durability profile parsed from a 2005
/// starfighter unit definition (`base == "flyer" | "commandflyer"`).
#[derive(Clone, Debug)]
pub struct SpaceFlightProfile {
    pub class_name: String,
    pub side: String,
    pub is_command_flyer: bool,
    /// Authored aircraft category used by the script host for an occupied kill.
    pub transport: bool,
    pub max_health: f64,
    pub min_speed: f64,
    pub mid_speed: f64,
    pub max_speed: f64,
    pub boost_speed: f64,
    pub acceleration: f64,
    pub boost_acceleration: f64,
    pub turn_rate: f64,
    pub pitch_rate: f64,
    pub roll_rate: f64,
    pub turn_filter: f64,
    pub turn_filter_decel: f64,
    pub pitch_filter: f64,
    pub pitch_filter_decel: f64,
    pub turn_buildup_multiplier: f64,
    pub pitch_buildup_multiplier: f64,
    pub roll_rate_filter: f64,
    pub momentum_filter: f64,
    pub bank_angle: f64,
    pub bank_filter: f64,
    pub level_filter: f64,
    pub level_filter_landing: f64,
    pub takeoff_height: f64,
    pub takeoff_time: f64,
    pub takeoff_speed: f64,
    pub landing_time: f64,
    pub landing_speed: f64,
    pub energy_bar: f64,
    pub energy_auto_restore: f64,
    pub energy_boost_drain: f64,
    pub energy_trick_single: f64,
    pub energy_trick_double: f64,
    pub trick_roll_speed: f64,
    pub trick_flip_speed: f64,
    pub trick_strafe_speed: f64,
    pub pilot_skill_repair_scale: f64,
    pub time_required_to_eject: f64,
    pub collision_scale: f64,
    pub collision_threshold: f64,
    pub spawn_point_count: u32,
    /// All authored `SpawnPointLocation` local offsets on a mobile transport.
    pub spawn_point_offsets: Vec<[f64; 3]>,
    pub seat_count: u8,
    /// Whether each seat exposes its seated soldier outside the hull
    /// (`PilotAnimation` authored and `IsPilotExposed != 0`).
    pub seat_pilot_exposed: [bool; 6],
    /// Local-space cockpit/seat origin (`PilotPosition`) per seat.
    pub seat_pilot_position: [[f64; 3]; 6],
    /// Each seat's aim limits in radians from the hull's nose, as the seat's
    /// section authors them: yaw (positive to the left) min and max, then
    /// pitch (positive down) min and max.
    pub seat_aim_limits: [[f64; 4]; 6],
    /// Local bounds `[min, max]` of all its collision bodies.
    pub bounds_min: [f64; 3],
    pub bounds_max: [f64; 3],
    /// The model's bounding sphere (local centre and radius), which blasts are
    /// measured to: the prepared `sphere`, else the one around its box.
    pub sphere: [f64; 4],
    /// Hull-contact local bounds used for wingtip/fuselage sweeps.
    pub contact_bounds_min: [f64; 3],
    pub contact_bounds_max: [f64; 3],
    /// Count of collision meshes imported from `vehicles.json`.
    pub collision_mesh_count: usize,
    /// Count of collision primitives imported from `vehicles.json`.
    pub collision_primitive_count: usize,
    /// True when `collision_tree` was built from `vehicles.json` rather than
    /// the fallback 12-triangle box.
    pub hull_has_vehicle_pack: bool,
    /// Collision kernel BVH tree index shared by all instances of this class:
    /// the bodies shots meet.
    pub collision_tree: Option<u32>,
    /// The bodies shots meet in flight, those soldiers meet and those other
    /// craft meet (`None`: the same).
    pub flight_tree: Option<u32>,
    pub soldier_tree: Option<u32>,
    pub craft_tree: Option<u32>,
    pub weapons: Vec<SpaceWeaponSpec>,
}

/// Fallback values for properties absent from a prepared definition.
pub const PROVISIONAL_FLIGHT_DEFAULTS: SpaceFlightProfile = SpaceFlightProfile {
            class_name: String::new(),
            side: String::new(),
            is_command_flyer: false,
            transport: false,
            max_health: 2000.0,
            min_speed: 35.0,
            mid_speed: 70.0,
            max_speed: 95.0,
            boost_speed: 150.0,
            acceleration: 40.0,
            boost_acceleration: 80.0,
            turn_rate: 1.0,
            pitch_rate: 1.25,
            roll_rate: 3.15,
            turn_filter: 6.0,
            turn_filter_decel: PROVISIONAL_DEFAULT_FILTER_DECEL,
            pitch_filter: 6.0,
            pitch_filter_decel: PROVISIONAL_DEFAULT_FILTER_DECEL,
            turn_buildup_multiplier: 1.0,
            pitch_buildup_multiplier: 1.0,
            roll_rate_filter: PROVISIONAL_DEFAULT_ROLL_RATE_FILTER,
            momentum_filter: PROVISIONAL_DEFAULT_MOMENTUM_FILTER,
            bank_angle: PROVISIONAL_DEFAULT_BANK_ANGLE,
            bank_filter: PROVISIONAL_DEFAULT_BANK_FILTER,
            // The 2005 game's own default.
            level_filter: 3.0,
            level_filter_landing: 5.0,
            // The 2005 game's own defaults for take-off and landing.
            takeoff_height: 5.0,
            takeoff_time: 5.0,
            takeoff_speed: 3.0,
            landing_time: 5.0,
            landing_speed: 3.0,
            energy_bar: 100.0,
            energy_auto_restore: 10.0,
            energy_boost_drain: 20.0,
            energy_trick_single: 15.0,
            energy_trick_double: 30.0,
            trick_roll_speed: 6.0,
            trick_flip_speed: 6.0,
            trick_strafe_speed: 30.0,
            pilot_skill_repair_scale: 0.02,
            time_required_to_eject: 2.5,
            collision_scale: 1.5,
            collision_threshold: 5.0,
            spawn_point_count: 0,
            spawn_point_offsets: Vec::new(),
            seat_count: 1,
            seat_pilot_exposed: [false; 6],
            seat_pilot_position: [[0.0; 3]; 6],
            seat_aim_limits: [DEFAULT_SEAT_AIM_LIMITS; 6],
            bounds_min: [-3.0, -2.0, -4.0],
            bounds_max: [3.0, 2.0, 4.0],
            sphere: [0.0, 0.0, 0.0, 5.385164807134504],
            contact_bounds_min: [-3.0, -2.0, -4.0],
            contact_bounds_max: [3.0, 2.0, 4.0],
            collision_mesh_count: 0,
            collision_primitive_count: 0,
            hull_has_vehicle_pack: false,
            collision_tree: None,
            flight_tree: None,
            soldier_tree: None,
            craft_tree: None,
            weapons: Vec::new(),
};

impl SpaceFlightProfile {
    pub(super) fn mission_aircraft_kind(&self) -> u8 {
        if self.transport { 5 } else { 4 }
    }

    pub fn pilot_exposed(&self, seat: u8) -> bool {
        self.seat_pilot_exposed.get(seat as usize).copied().unwrap_or(false)
    }

    pub fn pilot_local_position(&self, seat: u8) -> [f64; 3] {
        self.seat_pilot_position.get(seat as usize).copied().unwrap_or([0.0; 3])
    }
}

impl Default for SpaceFlightProfile {
    fn default() -> Self { PROVISIONAL_FLIGHT_DEFAULTS }
}

/// Live weapon firing state stored inline per weapon slot on a starfighter.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpaceWeaponRuntime {
    pub clock: super::combat::SpaceShotClock,
    pub barrage_remaining: u32,
    pub barrage_timer: f64,
    pub trigger_was_held: bool,
    pub ammo_in_clip: u32,
    pub reload_timer: f64,
    pub heat: f64,
    pub overheated: bool,
    pub warm_up: f64,
    pub shots_fired: u32,
    pub kick: super::combat::SpaceKick,
}

/// Target lock state held by a starfighter's fire-control system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum SpaceLockTargetKind {
    #[default]
    None = 0,
    Fighter = 1,
    Object = 2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpaceLockState {
    pub weapon_index: u8,
    pub kind: SpaceLockTargetKind,
    pub target_index: usize,
    pub target_generation: u64,
    pub progress: f64,
    pub locked: bool,
    pub lost_los_time: f64,
}

/// Hangar vehicle spawner pad (`com_item_vehicle_spawn`).
#[derive(Clone, Debug)]
pub struct SpaceVehiclePad {
    pub name: String,
    pub team: i32,
    pub profile_index: usize,
    pub control_zone: String,
    pub position: [f64; 3],
    pub yaw: f64,
    pub matrix: [f64; 16],
    pub spawn_count: u32,
    pub spawn_time: f64,
    pub expire_time_enemy: f64,
    pub expire_time_field: f64,
    pub decay_time: f64,
    pub respawn_timer: f64,
    /// Sampled deck elevation below this pad (independent of `landing_zones`).
    pub floor_y: f64,
}

/// One live (or dormant/destroyed) starfighter hull in a 2005 space battle.
#[derive(Clone, Debug)]
pub struct SpaceFighter {
    pub id: usize,
    pub generation: u32,
    pub pad_index: usize,
    pub profile_index: usize,
    pub team: i32,
    pub alive: bool,
    pub health: f64,
    pub max_health: f64,
    pub state: SpaceFlightState,
    pub state_time: f64,
    pub gear: f64,
    pub prev_gear: f64,
    pub position: [f64; 3],
    pub previous_position: [f64; 3],
    pub velocity: [f64; 3],
    pub quaternion: [f64; 4],
    pub previous_quaternion: [f64; 4],
    pub airspeed: f64,
    pub yaw_rate: f64,
    pub pitch_rate: f64,
    pub roll_rate: f64,
    pub roll_state: f64,
    pub level_timer: f64,
    pub yaw_buildup: f64,
    pub pitch_buildup: f64,
    pub bank: f64,
    pub prev_bank: f64,
    pub energy: f64,
    pub boosting: bool,
    pub boost_exhausted: bool,
    pub trick: SpaceTrick,
    pub trick_progress: f64,
    pub trick_cooldown: f64,
    pub trick_axis_right: [f64; 3],
    pub action_held: bool,
    pub eject_timer: f64,
    /// Seconds left before a crewed craft outside the boundary is destroyed;
    /// negative while it is inside or empty.
    pub oob_timer: f64,
    pub oob_warning: bool,
    pub empty_timer: f64,
    pub pilot: Option<SpacePilotId>,
    pub gunners: [Option<SpacePilotId>; 5],
    pub gunner_inputs: [SpacePilotInput; 5],
    pub landed_zone: Option<usize>,
    pub mobile_spawn_active: bool,
    pub kernel_body: Option<u32>,
    pub matrix: [f64; 16],
    pub inverse: [f64; 16],
    pub weapons: [SpaceWeaponRuntime; 5],
    pub lock: SpaceLockState,
    pub weapon_locks: [SpaceLockState; 5],
    pub pending_input: SpacePilotInput,
    /// Hull contacts with solid collision over this craft's life, the
    /// surface normal of the latest one and the collision body it struck
    /// (`SpaceBattlefield::body_owner` names it; negative: no body).
    pub contacts: u32,
    pub contact_normal: [f64; 3],
    pub contact_body: i32,
}

/// Outcome flags returned from stepping one starfighter tick so the outer
/// `SpaceBattlefield` can emit events without borrowing conflicts.
#[derive(Clone, Copy, Debug, Default)]
pub struct FighterStepNotice {
    pub took_off: bool,
    pub landed_zone: Option<usize>,
    pub destroyed: bool,
    pub dismounted_pilot: Option<SpacePilotId>,
    pub fire_primary: bool,
    pub fire_secondary: bool,
    pub lock_request: bool,
}

fn value_as_f64(v: &Value) -> f64 {
    match v {
        Value::Number(x) => *x,
        Value::String(s) => s
            .split_whitespace()
            .next()
            .and_then(|tok| tok.parse::<f64>().ok())
            .unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

fn authored_property<'a>(def: &'a Value, name: &str) -> Option<&'a Value> {
    def.get(name).or_else(||def.get("properties")?.as_array()?.iter().rev().find_map(|pair| {
        pair.at(0)?.as_str()?.eq_ignore_ascii_case(name).then(||pair.at(1)).flatten()
    }))
}

fn numeric_count(value: f64, minimum: usize, maximum: usize) -> bool {
    value.is_finite() && value.fract() == 0.0 && value >= minimum as f64 && value <= maximum as f64
}

fn validate_weapon_counts(weapon: &Value, name: &str) -> Result<(), String> {
    use bfe_content::vehicle_pack::MAX_SALVO;
    for (field, limit) in [("SalvoCount", MAX_SALVO), ("ShotsPerSalvo", MAX_SALVO), ("BarrageMin", u16::MAX as usize),
        ("BarrageMax", u16::MAX as usize), ("SecondaryOrdnancePeriod", u32::MAX as usize),
        ("RoundsPerClip", u32::MAX as usize)] {
        if let Some(v) = authored_property(weapon,field) {
            let number = v.as_f64().or_else(||v.as_str()?.trim().parse().ok());
            // A clip of -1 denotes an unlimited magazine.
            let unlimited_clip = field == "RoundsPerClip" && number == Some(-1.0);
            if !unlimited_clip && !number.is_some_and(|n| numeric_count(n,0,limit)) {
                return Err(format!("{name}: invalid {field}"));
            }
        }
    }
    Ok(())
}

/// A definition's `(key, value)` property pairs in authored order.
fn property_pairs(obj: &Value) -> impl Iterator<Item = (&str, &Value)> {
    obj.get("properties")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|pair| Some((pair.at(0)?.as_str()?, pair.at(1)?)))
}

/// The spread (pitch, yaw) in radians that a 2005 gun's shots draw, from its
/// properties in authored order, as the 2005 game reads them: `PitchSpread`
/// and `YawSpread` in degrees, and `MaxSpread` setting both at 0.025 and 0.05
/// of its value, so the later of the keys wins on each axis. A value that is
/// not a number leaves the axis as it was.
fn authored_spread<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a Value)>) -> (f64, f64) {
    let (mut pitch, mut yaw) = (0.0, 0.0);
    for (key, value) in pairs {
        let n = value_as_f64(value);
        if !n.is_finite() {
            continue;
        }
        if key.eq_ignore_ascii_case("PitchSpread") {
            pitch = n * DEG;
        } else if key.eq_ignore_ascii_case("YawSpread") {
            yaw = n * DEG;
        } else if key.eq_ignore_ascii_case("MaxSpread") {
            pitch = MAX_SPREAD_PITCH * n;
            yaw = MAX_SPREAD_YAW * n;
        }
    }
    (pitch, yaw)
}

/// The keys naming the blast a round sets off where it strikes.
pub const IMPACT_BLASTS: [&str; 3] = ["ExplosionName", "Explosion", "ExplosionImpact"];
/// The key naming the one it sets off at the end of its life.
pub const EXPIRE_BLAST: &str = "ExplosionExpire";

/// A blast class's record as the 2005 game reads it, key by key in authored
/// order: `Damage`, or `MaxDamage`; `DamageRadiusInner`; `DamageRadiusOuter`,
/// or `DamageRadius` (which leaves the inner radius alone); `HealthScale` for
/// people, animals and droids, `ArmorScale` for vehicles and buildings, or a
/// scale of their own; and `ShieldScale`. Scales are 1 and the rest 0 unless
/// given.
pub fn parse_explosion(record: &Value) -> SpaceExplosionSpec {
    let mut blast = SpaceExplosionSpec { scales: [1.0; 5], shield_scale: 1.0, ..SpaceExplosionSpec::NONE };
    for (key, value) in property_pairs(record) {
        let n = value_as_f64(value);
        if !n.is_finite() {
            continue;
        }
        if key.eq_ignore_ascii_case("Damage") || key.eq_ignore_ascii_case("MaxDamage") {
            blast.damage = n;
        } else if key.eq_ignore_ascii_case("DamageRadiusInner") {
            blast.inner_radius = n;
        } else if key.eq_ignore_ascii_case("DamageRadiusOuter") || key.eq_ignore_ascii_case("DamageRadius") {
            blast.outer_radius = n;
        } else if key.eq_ignore_ascii_case("HealthScale") {
            blast.scales[..3].fill(n);
        } else if key.eq_ignore_ascii_case("ArmorScale") {
            blast.scales[3..].fill(n);
        } else if key.eq_ignore_ascii_case("PersonScale") {
            blast.scales[0] = n;
        } else if key.eq_ignore_ascii_case("AnimalScale") {
            blast.scales[1] = n;
        } else if key.eq_ignore_ascii_case("DroidScale") {
            blast.scales[2] = n;
        } else if key.eq_ignore_ascii_case("VehicleScale") {
            blast.scales[3] = n;
        } else if key.eq_ignore_ascii_case("BuildingScale") {
            blast.scales[4] = n;
        } else if key.eq_ignore_ascii_case("ShieldScale") {
            blast.shield_scale = n;
        }
    }
    blast
}

/// The blast an ordnance names by one of `keys`, the last given: its record is
/// nested under that key. A beam sets off none.
pub fn named_blast(ordnance: &Value, keys: &[&str]) -> SpaceExplosionSpec {
    if ordnance.get("base").and_then(Value::as_str).is_some_and(|b| b.eq_ignore_ascii_case("beam")) {
        return SpaceExplosionSpec::NONE;
    }
    let named = property_pairs(ordnance)
        .filter(|(key, _)| keys.iter().any(|k| key.eq_ignore_ascii_case(k)))
        .last();
    let record = named.and_then(|(key, _)| {
        ordnance.get(key).or_else(|| {
            ordnance.as_object()?.entries().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v)
        })
    });
    record.map(parse_explosion).unwrap_or_default()
}

/// The kick a 2005 gun reads from its properties in authored order, as the
/// 2005 game does: `[per salvo, recovery a second, threshold, limit]` in
/// radians, from `SpreadPerShot` (or the older `KickSpread`),
/// `SpreadRecoverRate`, `SpreadThreshold` and `SpreadLimit` in degrees. The
/// older `SpreadRecover` sets the recovery to its value times the kick a salvo
/// adds, and the limit to that recovery over the shot delay as read so far
/// (`shot_delay` before any `ShotDelay`).
fn authored_kick<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a Value)>, shot_delay: f64) -> [f64; 4] {
    let [mut per_salvo, mut recover, mut threshold, mut limit] = [0.0; 4];
    let mut delay = shot_delay;
    for (key, value) in pairs {
        let n = value_as_f64(value);
        if !n.is_finite() {
            continue;
        }
        if key.eq_ignore_ascii_case("SpreadPerShot") || key.eq_ignore_ascii_case("KickSpread") {
            per_salvo = n * DEG;
        } else if key.eq_ignore_ascii_case("SpreadRecoverRate") {
            recover = n * DEG;
        } else if key.eq_ignore_ascii_case("SpreadThreshold") {
            threshold = n * DEG;
        } else if key.eq_ignore_ascii_case("SpreadLimit") {
            limit = n * DEG;
        } else if key.eq_ignore_ascii_case("ShotDelay") {
            delay = n;
        } else if key.eq_ignore_ascii_case("SpreadRecover") {
            recover = per_salvo * n;
            if delay > 0.0 {
                limit = recover / delay;
            }
        }
    }
    [per_salvo, recover, threshold, limit]
}

fn prop_f64(obj: &Value, key: &str, fallback: f64) -> f64 {
    if let Some(v) = obj.get(key) {
        let n = value_as_f64(v);
        if n.is_finite() {
            return n;
        }
    }
    if let Some(props) = obj.get("properties").and_then(Value::as_array) {
        for pair in props.iter().rev() {
            if pair
                .at(0)
                .and_then(Value::as_str)
                .is_some_and(|k| k.eq_ignore_ascii_case(key))
            {
                if let Some(val) = pair.at(1) {
                    let n = value_as_f64(val);
                    if n.is_finite() {
                        return n;
                    }
                }
            }
        }
    }
    fallback
}

/// A property of one or two numbers: one stands for both, none for zeros.
fn prop_pair(obj: &Value, key: &str) -> (f64, f64) {
    let first = prop_f64(obj, key, 0.0);
    let second = authored_property(obj, key).and_then(Value::as_str)
        .and_then(|text| text.split_whitespace().nth(1)?.parse::<f64>().ok())
        .filter(|n| n.is_finite());
    (first, second.unwrap_or(first))
}

/// Visits every `(key, value)` pair belonging to the flight chassis (`BODY`)
/// section in source order, ignoring gunner/turret seat sections (`TURRET1`,
/// `TURRET2`, ...).
///
/// Prefers explicit `vehicleSections` (`kind == "chassis"`) emitted by the
/// 2005 pack importer; falls back cleanly to scanning flat `properties` while
/// skipping non-`BODY` `FlyerSection` (`4cbe283c`) and `TurretSection` blocks.
fn for_each_chassis_prop<'a>(def: &'a Value, mut visit: impl FnMut(&'a str, &'a Value)) {
    if let Some(sections) = def.get("vehicleSections").and_then(Value::as_array) {
        if !sections.is_empty() {
            for sec in sections {
                let kind = sec.get("kind").and_then(Value::as_str).unwrap_or("");
                if !kind.eq_ignore_ascii_case("chassis") {
                    continue;
                }
                if let Some(props) = sec.get("properties").and_then(Value::as_array) {
                    for pair in props {
                        if let (Some(k), Some(v)) = (pair.at(0).and_then(Value::as_str), pair.at(1))
                        {
                            visit(k, v);
                        }
                    }
                }
            }
            return;
        }
    }
    if let Some(props) = def.get("properties").and_then(Value::as_array) {
        let mut in_seat = false;
        for pair in props {
            let Some(k) = pair.at(0).and_then(Value::as_str) else {
                continue;
            };
            let Some(v) = pair.at(1) else {
                continue;
            };
            if k.eq_ignore_ascii_case("FlyerSection") || k.eq_ignore_ascii_case("4cbe283c") {
                in_seat = !v
                    .as_str()
                    .is_some_and(|s| s.trim().eq_ignore_ascii_case("BODY"));
                continue;
            }
            if k.eq_ignore_ascii_case("TurretSection") {
                in_seat = true;
                continue;
            }
            if !in_seat {
                visit(k, v);
            }
        }
    }
}

fn chassis_prop_f64(def: &Value, key: &str, fallback: f64) -> f64 {
    // Always read chassis-scoped `vehicleSections` (`kind == "chassis"`) before
    // any flattened top-level property, because `materialize_swbf2_unit` flattens
    // later turret seat sections onto top-level keys (last-wins).
    let mut found: Option<f64> = None;
    for_each_chassis_prop(def, |k, v| {
        if k.eq_ignore_ascii_case(key) {
            let n = value_as_f64(v);
            if n.is_finite() {
                found = Some(n);
            }
        }
    });
    if let Some(n) = found {
        return n;
    }
    if let Some(v) = def.get(key) {
        let n = value_as_f64(v);
        if n.is_finite() {
            return n;
        }
    }
    fallback
}

fn chassis_prop_all_strs<'a>(def: &'a Value, key: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    for_each_chassis_prop(def, |k, v| {
        if k.eq_ignore_ascii_case(key) {
            if let Some(s) = v.as_str() {
                out.push(s);
            }
        }
    });
    if out.is_empty() {
        if let Some(s) = def.get(key).and_then(Value::as_str) {
            out.push(s);
        }
    }
    out
}

/// Resolves a placed-object ODF property by plain name or case-folded FNV-1a
/// hex key (8 lowercase hex digits).
pub fn placed_prop_ci<'a>(pairs: &'a [(String, String)], plain: &str, fnv_hex: &str) -> Option<&'a str> {
    for (k, v) in pairs.iter().rev() {
        if k.eq_ignore_ascii_case(plain) || (!fnv_hex.is_empty() && k.eq_ignore_ascii_case(fnv_hex)) {
            return Some(v.as_str());
        }
    }
    None
}

fn parse_vec3_str(text: &str, fallback: [f64; 3]) -> [f64; 3] {
    let mut parts = text.split_whitespace();
    let x = parts.next().and_then(|s| s.parse::<f64>().ok()).filter(|v| v.is_finite());
    let y = parts.next().and_then(|s| s.parse::<f64>().ok()).filter(|v| v.is_finite());
    let z = parts.next().and_then(|s| s.parse::<f64>().ok()).filter(|v| v.is_finite());
    match (x, y, z) {
        (Some(x), Some(y), Some(z)) => [x, y, z],
        _ => fallback,
    }
}

/// Parses a single starfighter weapon record from a 2005 unit's `weapons` list.
pub fn parse_space_weapon(w: &Value, index: usize) -> SpaceWeaponSpec {
    let mut spec = SpaceWeaponSpec::default();
    spec.name = w
        .get("class")
        .or_else(|| w.get("name"))
        .or_else(|| w.get("weapon"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    spec.hud_tag = authored_property(w, "HUDTag")
        .or_else(|| authored_property(w, "IconTexture"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    spec.channel = w
        .get("channel")
        .and_then(Value::as_f64)
        .map(|n| n as u8)
        .unwrap_or(index as u8);
    spec.seat = w
        .get("seat")
        .and_then(Value::as_f64)
        .map(|n| n as u8)
        .unwrap_or(if spec.channel >= 2 { 1 } else { 0 });

    spec.shot_delay = v8math::max(PROVISIONAL_MIN_SHOT_INTERVAL, prop_f64(w, "ShotDelay", spec.shot_delay));
    spec.salvo_count = v8math::max(1.0, prop_f64(w, "SalvoCount", 1.0)) as u32;
    spec.shots_per_salvo = v8math::max(1.0, prop_f64(w, "ShotsPerSalvo", 1.0)) as u32;
    let (first, second) = prop_pair(w, "SalvoDelay");
    spec.salvo_delay = v8math::max(0.0, first);
    spec.salvo_delay_max = v8math::max(spec.salvo_delay, second);
    spec.initial_salvo_delay = v8math::max(0.0, prop_f64(w, "InitialSalvoDelay", 0.0));
    spec.barrage_min = v8math::max(0.0, prop_f64(w, "BarrageMin", 0.0)) as u32;
    spec.barrage_max = v8math::max(
        f64::from(spec.barrage_min),
        prop_f64(w, "BarrageMax", f64::from(spec.barrage_min)),
    ) as u32;
    spec.barrage_delay = v8math::max(0.0, prop_f64(w, "BarrageDelay", 0.0));
    let direct = ["MaxSpread", "PitchSpread", "YawSpread"].into_iter().filter_map(|k| Some((k, w.get(k)?)));
    (spec.pitch_spread, spec.yaw_spread) = authored_spread(property_pairs(w).chain(direct));
    [spec.spread_per_shot, spec.spread_recover_rate, spec.spread_threshold, spec.spread_limit] =
        authored_kick(property_pairs(w), spec.shot_delay);
    spec.cool_down_time = v8math::max(0.0, prop_f64(w, "CoolDownTime", 0.0));
    spec.auto_aim_size = v8math::max(0.0, prop_f64(w, "AutoAimSize", 0.0)) * AUTO_AIM_SCALE;
    spec.rounds_per_clip = v8math::max(0.0, prop_f64(w, "RoundsPerClip", 0.0)) as u32;
    spec.reload_time = v8math::max(0.0, prop_f64(w, "ReloadTime", 0.0));
    spec.heat_per_shot = v8math::max(0.0, prop_f64(w, "HeatPerShot", 0.0));
    spec.heat_recover_rate = v8math::max(0.0, prop_f64(w, "HeatRecoverRate", PROVISIONAL_WEAPON_DEFAULTS.heat_recover_rate));
    spec.heat_threshold = v8math::min(1.0, v8math::max(0.0, prop_f64(w, "HeatThreshold", PROVISIONAL_WEAPON_DEFAULTS.heat_threshold)));
    spec.warm_up_time = v8math::max(0.0, prop_f64(w, "WarmUpTime", 0.0));
    spec.secondary_tracer_period = v8math::max(0.0, prop_f64(w, "SecondaryOrdnancePeriod", 0.0)) as u32;
    spec.lock_on_range = v8math::max(0.0, prop_f64(w, "LockOnRange", 0.0));
    spec.lock_on_angle_deg = v8math::max(PROVISIONAL_MIN_LOCK_ANGLE, prop_f64(w, "LockOnAngle", PROVISIONAL_WEAPON_DEFAULTS.lock_on_angle_deg));
    spec.lock_off_angle_deg = v8math::max(spec.lock_on_angle_deg, prop_f64(w, "LockOffAngle", PROVISIONAL_WEAPON_DEFAULTS.lock_off_angle_deg));
    spec.lock_time = v8math::max(0.0, prop_f64(w, "LockTime", PROVISIONAL_WEAPON_DEFAULTS.lock_time));
    spec.target_lock_max_distance = v8math::max(PROVISIONAL_MIN_LOCK_DISTANCE, prop_f64(w, "TargetLockMaxDistance", PROVISIONAL_WEAPON_DEFAULTS.target_lock_max_distance));
    spec.target_lock_max_distance_lose = v8math::max(
        spec.target_lock_max_distance,
        prop_f64(w, "TargetLockMaxDistanceLose", PROVISIONAL_WEAPON_DEFAULTS.target_lock_max_distance_lose),
    );
    spec.max_range = v8math::max(PROVISIONAL_MIN_WEAPON_RANGE, prop_f64(w, "MaxRange", PROVISIONAL_WEAPON_DEFAULTS.max_range));
    spec.ai_min_range = v8math::min(spec.max_range, v8math::max(0.0, prop_f64(w, "MinRange", 0.0)));
    spec.ai_optimal_range = v8math::min(spec.max_range, v8math::max(spec.ai_min_range, prop_f64(w, "OptimalRange", spec.max_range * 0.5)));
    let flag = |key: &str, fallback: bool| prop_f64(w, key, if fallback { 1.0 } else { 0.0 }) != 0.0;
    spec.ai_targets_person = flag("AITargetPerson", PROVISIONAL_AI_TARGETS[0]);
    spec.ai_targets_vehicle = flag("AITargetVehicle", PROVISIONAL_AI_TARGETS[1]);
    spec.ai_targets_building = flag("AITargetBuilding", PROVISIONAL_AI_TARGETS[2]);
    let bubble = super::aim::RELEASED_MISS_BUBBLE;
    spec.ai_bubble = super::aim::MissBubble {
        size: prop_f64(w, "AIBubbleSizeMultiplier", bubble.size),
        divider: prop_f64(w, "AIBubbleScaleDistDivider", bubble.divider),
        clamp: prop_f64(w, "AIBubbleScaleClamp", bubble.clamp),
    };

    if let Some(ord) = w.get("ordnance").filter(|v| v.as_object().is_some()) {
        let base = ord.get("base").and_then(Value::as_str).unwrap_or("bolt");
        let guided = prop_f64(ord, "GuidedMissile", 0.0) != 0.0;
        spec.guided_missile = guided;
        spec.ordnance_kind = if guided || base == "flyer" {
            SpaceOrdnanceKind::GuidedRocket
        } else if base == "missile" {
            SpaceOrdnanceKind::Missile
        } else if base == "shell" {
            SpaceOrdnanceKind::BombShell
        } else if base == "beam" {
            SpaceOrdnanceKind::Beam
        } else {
            SpaceOrdnanceKind::Bolt
        };
        spec.velocity = v8math::max(
            PROVISIONAL_MIN_ROUND_SPEED,
            if base == "beam" {
                prop_f64(ord, "Range", PROVISIONAL_BEAM_RANGE)
            } else {
                prop_f64(ord, "Velocity", PROVISIONAL_WEAPON_DEFAULTS.velocity)
            },
        );
        spec.life_span = v8math::max(PROVISIONAL_MIN_ROUND_LIFE, prop_f64(ord, "LifeSpan", PROVISIONAL_WEAPON_DEFAULTS.life_span));
        spec.expire_blast = named_blast(ord, &[EXPIRE_BLAST]);
        spec.gravity = prop_f64(ord, "Gravity", 0.0);
        spec.turn_rate = v8math::max(0.0, prop_f64(ord, "TurnRate", 0.0));
        spec.waver_turn = v8math::max(0.0, prop_f64(ord, "WaverTurn", 0.0));
        spec.pass_through = base == "beam" && prop_f64(ord, "PassThrough", 0.0) != 0.0;
        spec.max_damage = v8math::max(0.0, prop_f64(ord, "MaxDamage", prop_f64(ord, "Damage", PROVISIONAL_WEAPON_DEFAULTS.max_damage)));
        let fin = prop_f64(ord, "DamageFinalDamage", f64::NAN);
        spec.final_damage = fin.is_finite().then_some(fin);
        spec.damage_transition_delay = v8math::max(0.0, prop_f64(ord, "DamageTransitionDelay", 0.0));
        spec.damage_transition_period = v8math::max(0.0, prop_f64(ord, "DamageTransitionPeriod", 0.0));
        spec.person_scale = prop_f64(ord, "PersonScale", 1.0);
        spec.animal_scale = prop_f64(ord, "AnimalScale", 1.0);
        spec.droid_scale = prop_f64(ord, "DroidScale", 1.0);
        spec.vehicle_scale = prop_f64(ord, "VehicleScale", 1.0);
        spec.building_scale = prop_f64(ord, "BuildingScale", 1.0);
        spec.shield_scale = prop_f64(ord, "ShieldScale", 1.0);
        if guided {
            spec.guided_damage = v8math::max(0.0, prop_f64(ord, "GuidedMissileDamage", PROVISIONAL_WEAPON_DEFAULTS.guided_damage));
            spec.guided_damage_boost = v8math::max(
                spec.guided_damage,
                prop_f64(ord, "GuidedMissileDamageBoost", PROVISIONAL_WEAPON_DEFAULTS.guided_damage_boost),
            );
            spec.guided_boost_threshold = v8math::min(
                1.0,
                v8math::max(0.0, prop_f64(ord, "GuidedMissileBoostExplosionSpeedThreshold", PROVISIONAL_WEAPON_DEFAULTS.guided_boost_threshold)),
            );
            spec.guided_boost_speed = v8math::max(PROVISIONAL_MIN_GUIDED_BOOST_SPEED, prop_f64(ord, "BoostSpeed", PROVISIONAL_WEAPON_DEFAULTS.guided_boost_speed));
            let cruise = prop_f64(ord, "MaxSpeed", spec.guided_boost_speed * PROVISIONAL_GUIDED_CRUISE_FRAC);
            spec.velocity = v8math::max(PROVISIONAL_MIN_GUIDED_SPEED, v8math::min(spec.guided_boost_speed, cruise));
            spec.life_span = v8math::max(PROVISIONAL_MIN_GUIDED_LIFE, prop_f64(ord, "LifeSpan", PROVISIONAL_GUIDED_DEFAULT_LIFE_S));
        }
        // `ExplosionName`, `Explosion` and `ExplosionImpact` are one key to the
        // game, and the last one given counts.
        spec.blast = named_blast(ord, &IMPACT_BLASTS);
    }
    spec
}

/// Parses a 2005 starfighter class (`flyer` or `commandflyer`) from `units.json`,
/// strictly scoping hull flight tuning to chassis (`BODY`) sections so gunner
/// turret seat rates never overwrite gunship hull rates.
/// Validate bounded simulation loops before constructing unit, fleet or collision owners.
pub(crate) fn validate_combat_content(content: &bfe_content::content::Content) -> Result<(), String> {
    use bfe_content::vehicle_pack::{MAX_SEATS, MAX_WEAPONS};
    if let Some(data) = &content.battle.swbf2 {
        super::passages::validate(&data.map_admission)?;
        super::openings::validate(&data.map_admission.map_data)?;
        let mut slots = 0usize;
        for pad in data.map_admission.placed.iter().filter(|p|p.base.eq_ignore_ascii_case("vehiclespawn")) {
            let raw=placed_prop_ci(&pad.properties,"SpawnCount","88923ff7").unwrap_or("1");
            let n=raw.trim().parse::<f64>().ok().filter(|&n|numeric_count(n,1,super::PROVISIONAL_PAD_MAX_SPAWN_COUNT as usize))
                .ok_or_else(||format!("{}: invalid SpawnCount",pad.name))?;
            slots+=n as usize;
            if slots>super::MAX_PREPARED_FIGHTERS { return Err("too many space fighter slots".into()); }
        }
    }
    for side in &content.battle.sides {
        let Some(classes) = side.units.as_object() else { continue; };
        for (name, def) in classes.entries() {
            let base = def.get("base").and_then(Value::as_str).unwrap_or("");
            if !base.eq_ignore_ascii_case("flyer") && !base.eq_ignore_ascii_case("commandflyer") { continue; }
            let mut spawn_points=None;
            for_each_chassis_prop(def,|key,value| {
                if key.eq_ignore_ascii_case("SpawnPointCount") { spawn_points=Some(value); }
            });
            if spawn_points.is_some_and(|v| !numeric_count(value_as_f64(v),0,u16::MAX as usize)) {
                return Err(format!("{name}: invalid SpawnPointCount"));
            }
            if let Some(value) = def.get("weapons") {
                let weapons = value.as_array().ok_or_else(|| format!("{name}: invalid weapon list"))?;
                if weapons.len() > MAX_WEAPONS { return Err(format!("{name}: too many fighter weapons")); }
                for w in weapons {
                    for (field, limit) in [("seat", MAX_SEATS-1), ("channel", MAX_WEAPONS-1)] {
                        if w.get(field).is_some_and(|v| !v.as_f64().is_some_and(|n| numeric_count(n, 0, limit))) {
                            return Err(format!("{name}: invalid weapon {field}"));
                        }
                    }
                    validate_weapon_counts(w,name)?;
                }
            }
            if let Some(value) = def.get("vehicleSections") {
                let sections = value.as_array().ok_or_else(|| format!("{name}: invalid section list"))?;
                if sections.len() > 4096 || sections.iter().filter(|s| s.get("kind").and_then(Value::as_str)
                    .is_some_and(|k| k.eq_ignore_ascii_case("seat"))).count() >= MAX_SEATS { return Err(format!("{name}: too many seats")); }
            }
        }
    }
    if let Some(table) = content.battle.swbf2.as_ref().and_then(|b| b.map_admission.map_classes.as_ref())
        .and_then(|table| table.get("wpnc")).and_then(Value::as_object) {
        for (name, w) in table.entries() {
            validate_weapon_counts(w,name)?;
        }
    }
    Ok(())
}

fn section_pilot_exposed<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a Value)>) -> bool {
    let mut has_anim = false;
    let mut exposed = false;
    for (k, v) in pairs {
        if k.eq_ignore_ascii_case("PilotAnimation") {
            has_anim = v.as_str().is_some_and(|s| !s.trim().is_empty());
        } else if k.eq_ignore_ascii_case("IsPilotExposed") {
            let n = value_as_f64(v);
            if n.is_finite() {
                exposed = n != 0.0;
            }
        }
    }
    has_anim && exposed
}

fn parse_seat_pilot_exposed(def: &Value) -> [bool; 6] {
    let mut out = [false; 6];
    if let Some(sections) = def.get("vehicleSections").and_then(Value::as_array) {
        if !sections.is_empty() {
            let mut chassis_anim = false;
            let mut chassis_exposed = false;
            let mut seat_idx = 1usize;
            for sec in sections {
                let kind = sec.get("kind").and_then(Value::as_str).unwrap_or("");
                let props = sec.get("properties").and_then(Value::as_array);
                let pairs = props.into_iter().flatten().filter_map(|pair| {
                    Some((pair.at(0)?.as_str()?, pair.at(1)?))
                });
                if kind.eq_ignore_ascii_case("chassis") {
                    for (k, v) in pairs {
                        if k.eq_ignore_ascii_case("PilotAnimation") {
                            chassis_anim = v.as_str().is_some_and(|s| !s.trim().is_empty());
                        } else if k.eq_ignore_ascii_case("IsPilotExposed") {
                            let n = value_as_f64(v);
                            if n.is_finite() {
                                chassis_exposed = n != 0.0;
                            }
                        }
                    }
                } else if kind.eq_ignore_ascii_case("seat") && seat_idx < out.len() {
                    out[seat_idx] = section_pilot_exposed(pairs);
                    seat_idx += 1;
                }
            }
            out[0] = chassis_anim && chassis_exposed;
            return out;
        }
    }
    let mut pairs = Vec::new();
    for_each_chassis_prop(def, |k, v| pairs.push((k, v)));
    out[0] = section_pilot_exposed(pairs);
    out
}

/// How far outside its arc a point may lie and still count as one a gunner
/// can bear on (radians).
pub const PROVISIONAL_SEAT_BEAR_CONE: f64 = 0.05;

/// A seat's aim limits when its section sets none: a turret turns freely
/// (ten radians each way) and pitches a radian and a half either way.
pub const DEFAULT_SEAT_AIM_LIMITS: [f64; 4] = [-10.0, 10.0, -1.5, 1.5];

/// Every weapon a class's sections name, in source order, with the seat its
/// section is (the chassis 0, then each seat section in turn) and its fire
/// channel there: `WeaponSection` n names the n-th weapon of the section
/// (channel n - 1, the primary attack first), and a weapon named before any
/// `WeaponSection` is the section's first.
fn section_weapon_seats(def: &Value) -> Vec<(&str, u8, u8)> {
    let mut out = Vec::new();
    let Some(sections) = def.get("vehicleSections").and_then(Value::as_array) else { return out };
    let mut seat = 0u8;
    for sec in sections {
        let kind = sec.get("kind").and_then(Value::as_str).unwrap_or("");
        let this_seat = if kind.eq_ignore_ascii_case("chassis") {
            0
        } else if kind.eq_ignore_ascii_case("seat") {
            seat = seat.saturating_add(1);
            seat
        } else {
            continue;
        };
        let mut channel = 0u8;
        for pair in sec.get("properties").and_then(Value::as_array).into_iter().flatten() {
            let (Some(k), Some(v)) = (pair.at(0).and_then(Value::as_str), pair.at(1)) else { continue };
            if k.eq_ignore_ascii_case("WeaponSection") {
                let n = value_as_f64(v);
                if n.is_finite() && n >= 1.0 && n < 9.0 {
                    channel = n as u8 - 1;
                }
            } else if k.eq_ignore_ascii_case("WeaponName") {
                if let Some(name) = v.as_str() {
                    out.push((name, this_seat, channel));
                }
            }
        }
    }
    out
}

/// Each seat section's `YawLimits` and `PitchLimits` (degrees in the data).
fn section_aim_limits(def: &Value) -> [[f64; 4]; 6] {
    let mut out = [DEFAULT_SEAT_AIM_LIMITS; 6];
    let Some(sections) = def.get("vehicleSections").and_then(Value::as_array) else { return out };
    let mut seat = 0usize;
    for sec in sections {
        let kind = sec.get("kind").and_then(Value::as_str).unwrap_or("");
        if !kind.eq_ignore_ascii_case("seat") {
            continue;
        }
        seat += 1;
        let Some(limits) = out.get_mut(seat) else { break };
        for pair in sec.get("properties").and_then(Value::as_array).into_iter().flatten() {
            let (Some(k), Some(v)) = (pair.at(0).and_then(Value::as_str), pair.at(1).and_then(Value::as_str)) else { continue };
            let at = if k.eq_ignore_ascii_case("YawLimits") {
                0
            } else if k.eq_ignore_ascii_case("PitchLimits") {
                2
            } else {
                continue;
            };
            let mut it = v.split_whitespace().filter_map(|s| s.parse::<f64>().ok()).filter(|n| n.is_finite());
            if let (Some(lo), Some(hi)) = (it.next(), it.next()) {
                let (lo, hi) = (v8math::min(lo, hi), v8math::max(lo, hi));
                limits[at] = lo * PI / 180.0;
                limits[at + 1] = hi * PI / 180.0;
            }
        }
    }
    out
}

pub fn parse_flight_profile(side: &str, class_name: &str, def: &Value) -> Option<SpaceFlightProfile> {
    let base = def.get("base").and_then(Value::as_str).unwrap_or("");
    if base != "flyer" && base != "commandflyer" {
        return None;
    }
    let mut p = SpaceFlightProfile {
        class_name: class_name.to_string(),
        side: side.to_string(),
        is_command_flyer: base == "commandflyer",
        transport: chassis_prop_all_strs(def, "VehicleType").iter().any(|v| v.eq_ignore_ascii_case("transport")),
        seat_pilot_exposed: parse_seat_pilot_exposed(def),
        seat_aim_limits: section_aim_limits(def),
        ..Default::default()
    };
    p.max_health = v8math::max(PROVISIONAL_MIN_HULL_CAPACITY, chassis_prop_f64(def, "MaxHealth", p.max_health));
    p.min_speed = v8math::max(PROVISIONAL_MIN_FLYER_SPEED, chassis_prop_f64(def, "MinSpeed", p.min_speed));
    p.mid_speed = v8math::max(p.min_speed, chassis_prop_f64(def, "MidSpeed", p.mid_speed));
    p.max_speed = v8math::max(p.mid_speed, chassis_prop_f64(def, "MaxSpeed", p.max_speed));
    p.boost_speed = v8math::max(p.max_speed, chassis_prop_f64(def, "BoostSpeed", p.max_speed));
    let accel = chassis_prop_f64(def, "Acceleraton", chassis_prop_f64(def, "Acceleration", p.acceleration));
    p.acceleration = v8math::max(PROVISIONAL_MIN_FLYER_ACCELERATION, accel);
    p.boost_acceleration = v8math::max(
        p.acceleration,
        chassis_prop_f64(def, "BoostAcceleration", p.acceleration),
    );
    p.turn_rate = v8math::max(PROVISIONAL_MIN_CONTROL_RATE, chassis_prop_f64(def, "TurnRate", p.turn_rate));
    p.pitch_rate = v8math::max(PROVISIONAL_MIN_CONTROL_RATE, chassis_prop_f64(def, "PitchRate", p.pitch_rate));
    p.roll_rate = v8math::max(PROVISIONAL_MIN_CONTROL_RATE, chassis_prop_f64(def, "RollRate", p.roll_rate));
    p.turn_filter = v8math::max(PROVISIONAL_MIN_CONTROL_FILTER, chassis_prop_f64(def, "TurnFilter", p.turn_filter));
    p.turn_filter_decel = v8math::max(
        PROVISIONAL_MIN_CONTROL_FILTER,
        chassis_prop_f64(def, "TurnFilterDecel", PROVISIONAL_DEFAULT_FILTER_DECEL),
    );
    p.pitch_filter = v8math::max(PROVISIONAL_MIN_CONTROL_FILTER, chassis_prop_f64(def, "PitchFilter", p.pitch_filter));
    p.pitch_filter_decel = v8math::max(
        PROVISIONAL_MIN_CONTROL_FILTER,
        chassis_prop_f64(def, "PitchFilterDecel", PROVISIONAL_DEFAULT_FILTER_DECEL),
    );
    p.turn_buildup_multiplier = v8math::max(
        0.1,
        chassis_prop_f64(def, "TurnBuildupMultiplier", p.turn_buildup_multiplier),
    );
    p.pitch_buildup_multiplier = v8math::max(
        0.1,
        chassis_prop_f64(def, "PitchBuildupMultiplier", p.pitch_buildup_multiplier),
    );
    p.roll_rate_filter = v8math::max(
        PROVISIONAL_MIN_CONTROL_FILTER,
        chassis_prop_f64(def, "RollRateFilter", p.roll_rate_filter),
    );
    p.momentum_filter = v8math::max(
        PROVISIONAL_MIN_CONTROL_FILTER,
        chassis_prop_f64(def, "MomentumFilter", p.momentum_filter),
    );
    p.bank_angle = v8math::max(0.0, chassis_prop_f64(def, "BankAngle", p.bank_angle));
    p.bank_filter = v8math::max(PROVISIONAL_MIN_CONTROL_FILTER, chassis_prop_f64(def, "BankFilter", p.bank_filter));
    p.level_filter = v8math::max(0.0, chassis_prop_f64(def, "LevelFilter", p.level_filter));
    p.level_filter_landing = v8math::max(
        p.level_filter,
        chassis_prop_f64(def, "LevelFilterLanding", p.level_filter_landing),
    );
    p.takeoff_height = v8math::max(PROVISIONAL_MIN_GEAR_HEIGHT, chassis_prop_f64(def, "TakeoffHeight", p.takeoff_height));
    p.takeoff_time = v8math::max(PROVISIONAL_MIN_GEAR_TIME, chassis_prop_f64(def, "TakeoffTime", p.takeoff_time));
    p.takeoff_speed = v8math::max(PROVISIONAL_MIN_FLYER_SPEED, chassis_prop_f64(def, "TakeoffSpeed", p.takeoff_speed));
    p.landing_time = v8math::max(PROVISIONAL_MIN_GEAR_TIME, chassis_prop_f64(def, "LandingTime", p.landing_time));
    p.landing_speed = v8math::max(PROVISIONAL_MIN_FLYER_SPEED, chassis_prop_f64(def, "LandingSpeed", p.landing_speed));
    p.energy_bar = v8math::max(PROVISIONAL_MIN_ENERGY_CAPACITY, chassis_prop_f64(def, "EnergyBar", p.energy_bar));
    p.energy_auto_restore = v8math::max(0.0, chassis_prop_f64(def, "EnergyAutoRestore", p.energy_auto_restore));
    p.energy_boost_drain = v8math::max(0.0, chassis_prop_f64(def, "EnergyBoostDrain", p.energy_boost_drain));
    p.energy_trick_single = v8math::max(
        0.0,
        chassis_prop_f64(def, "EnergyTrickDrainSingleTap", p.energy_trick_single),
    );
    p.energy_trick_double = v8math::max(
        0.0,
        chassis_prop_f64(def, "EnergyTrickDrainDoubleTap", p.energy_trick_double),
    );
    p.trick_roll_speed = v8math::max(0.0, chassis_prop_f64(def, "TrickRollSpeed", p.trick_roll_speed));
    p.trick_flip_speed = v8math::max(0.0, chassis_prop_f64(def, "TrickFlipSpeed", p.trick_flip_speed));
    p.trick_strafe_speed = v8math::max(
        0.0,
        chassis_prop_f64(def, "TrickSideRollStrafeSpeed", p.trick_strafe_speed),
    );
    p.pilot_skill_repair_scale = v8math::max(
        0.0,
        chassis_prop_f64(def, "PilotSkillRepairScale", p.pilot_skill_repair_scale),
    );
    p.time_required_to_eject = v8math::max(
        0.0,
        chassis_prop_f64(def, "TimeRequiredToEject", p.time_required_to_eject),
    );
    p.collision_scale = v8math::max(0.0, chassis_prop_f64(def, "CollisionScale", p.collision_scale));
    p.collision_threshold = v8math::max(
        0.1,
        chassis_prop_f64(def, "CollisionThreshold", p.collision_threshold),
    );
    p.spawn_point_count = v8math::max(0.0, chassis_prop_f64(def, "SpawnPointCount", 0.0)) as u32;
    for loc in chassis_prop_all_strs(def, "SpawnPointLocation") {
        p.spawn_point_offsets.push(parse_vec3_str(loc, [0.0; 3]));
    }
    if let Some(bounds) = def.get("bounds").and_then(Value::as_array) {
        if bounds.len() >= 2 {
            if let (Some(lo), Some(hi)) = (bounds[0].as_array(), bounds[1].as_array()) {
                if lo.len() >= 3 && hi.len() >= 3 {
                    let bmin = [to_number(lo.first()), to_number(lo.get(1)), to_number(lo.get(2))];
                    let bmax = [to_number(hi.first()), to_number(hi.get(1)), to_number(hi.get(2))];
                    if bmin.iter().chain(bmax.iter()).all(|v| v.is_finite()) {
                        p.bounds_min = bmin;
                        p.bounds_max = bmax;
                        p.contact_bounds_min = bmin;
                        p.contact_bounds_max = bmax;
                        let mid = |k: usize| 0.5 * (bmin[k] + bmax[k]);
                        let half = 0.5 * v8math::hypot3(bmax[0] - bmin[0], bmax[1] - bmin[1], bmax[2] - bmin[2]);
                        p.sphere = [mid(0), mid(1), mid(2), half];
                    }
                }
            }
        }
    }
    if let Some(sphere) = def.get("sphere").and_then(Value::as_array).filter(|s| s.len() >= 4) {
        let sphere: [f64; 4] = std::array::from_fn(|k| to_number(sphere.get(k)));
        if sphere.iter().all(|n| n.is_finite()) && sphere[3] >= 0.0 {
            p.sphere = sphere;
        }
    }
    if let Some(weapons) = def.get("weapons").and_then(Value::as_array) {
        if weapons.len() > bfe_content::vehicle_pack::MAX_WEAPONS { return None; }
        let mut max_seat = 0u8;
        let mut named = section_weapon_seats(def);
        for (idx, w) in weapons.iter().enumerate() {
            let mut spec = parse_space_weapon(w, idx);
            // The seat sections name their own weapons: a weapon goes to the
            // section that names it, on the channel its WeaponSection gives.
            if w.get("seat").is_none() {
                if let Some(at) = named.iter().position(|(name, _, _)| name.eq_ignore_ascii_case(&spec.name)) {
                    let (_, seat, channel) = named.remove(at);
                    spec.seat = seat;
                    spec.channel = channel;
                }
            }
            if usize::from(spec.seat) >= bfe_content::vehicle_pack::MAX_SEATS { return None; }
            if spec.seat > max_seat {
                max_seat = spec.seat;
            }
            p.weapons.push(spec);
        }
        let section_seats = def
            .get("vehicleSections")
            .and_then(Value::as_array)
            .map(|secs| {
                let seat_secs = secs
                    .iter()
                    .filter(|s| {
                        s.get("kind")
                            .and_then(Value::as_str)
                            .is_some_and(|k| k.eq_ignore_ascii_case("seat"))
                    })
                    .count();
                u8::try_from(1 + seat_secs).ok()
            })
            .unwrap_or(Some(1))?;
        p.seat_count = if p.is_command_flyer {
            (max_seat + 1).max(section_seats).max(2)
        } else {
            (max_seat + 1).max(section_seats).max(1)
        };
    }
    Some(p)
}

/// Builds a 12-triangle axis-aligned box BVH tree in `collision` from a
/// starfighter's local bounds. Used only when `vehicles.json` does not supply
/// authored collision geometry for a class.
pub fn install_bounds_box_tree(
    collision: &mut CollisionOwner,
    bmin: [f64; 3],
    bmax: [f64; 3],
) -> u32 {
    let (x0, y0, z0) = (bmin[0] as f32, bmin[1] as f32, bmin[2] as f32);
    let (x1, y1, z1) = (bmax[0] as f32, bmax[1] as f32, bmax[2] as f32);
    let positions: [f32; 24] = [
        x0, y0, z0, x1, y0, z0, x1, y1, z0, x0, y1, z0,
        x0, y0, z1, x1, y0, z1, x1, y1, z1, x0, y1, z1,
    ];
    let indices: [u32; 36] = [
        0, 2, 1, 0, 3, 2,
        4, 5, 6, 4, 6, 7,
        0, 1, 5, 0, 5, 4,
        2, 3, 7, 2, 7, 6,
        0, 4, 7, 0, 7, 3,
        1, 2, 6, 1, 6, 5,
    ];
    let pos_id = collision.positions(&positions);
    let bvh = build_bvh(Positions::F32(&positions), &indices);
    let tree = collision.tree(pos_id, &indices, &bvh);
    let polys = collision.tree_polygon_ids(tree, 12);
    for (i, slot) in polys.iter_mut().enumerate() {
        *slot = (i / 2) as u32;
    }
    tree
}

/// Builds a collision BVH tree from those of a 2005 vehicle pack model's meshes
/// and transformed collision primitives that `mask` marks (handling
/// primitive-only fighters cleanly).
pub fn install_vehicle_pack_tree(
    collision: &mut CollisionOwner,
    model: &Swbf2VehicleModel,
    mask: u32,
) -> Option<(u32, Swbf2CollisionGeometry)> {
    let geom = model.build_collision_geometry(mask);
    if geom.positions.is_empty() || geom.indices.len() < 3 {
        return None;
    }
    let pos_id = collision.positions(&geom.positions);
    let bvh = build_bvh(Positions::F32(&geom.positions), &geom.indices);
    let tree = collision.tree(pos_id, &geom.indices, &bvh);
    let tri_count = geom.indices.len() / 3;
    let polys = collision.tree_polygon_ids(tree, tri_count as u32);
    for (i, slot) in polys.iter_mut().enumerate() {
        *slot = geom.polygons.get(i).copied().unwrap_or(i as u32);
    }
    Some((tree, geom))
}

/// Attaches real collision geometry, hull contact bounds, and named weapon
/// fire points from `maps/<map>/vehicles/vehicles.json` to `profile`. Falls
/// back to a bounds-box BVH only when the vehicle pack omits this class.
pub fn attach_vehicle_pack_to_profile(
    profile: &mut SpaceFlightProfile,
    vehicle_pack: Option<&Swbf2VehiclePack>,
    collision: &mut CollisionOwner,
) {
    if let Some(pack) = vehicle_pack {
        if let Some(cls) = pack.class(&profile.class_name) {
            for spec in &mut profile.weapons {
                let muzzles = cls.weapon_muzzles(&spec.name, Some(spec.seat as usize));
                if !muzzles.is_empty() {
                    spec.muzzles = muzzles;
                }
            }
            for (seat_idx, seat) in cls.seats.iter().enumerate().take(profile.seat_pilot_position.len()) {
                if let Some(pos) = seat
                    .points
                    .iter()
                    .filter_map(|&idx| cls.points.get(idx))
                    .find(|pt| pt.property.eq_ignore_ascii_case("PilotPosition"))
                    .and_then(|pt| pt.model_position())
                {
                    profile.seat_pilot_position[seat_idx] = pos;
                }
            }
            if profile.seat_pilot_position[0] == [0.0; 3] {
                if let Some(pos) = cls
                    .points
                    .iter()
                    .find(|pt| pt.property.eq_ignore_ascii_case("PilotPosition"))
                    .and_then(|pt| pt.model_position())
                {
                    profile.seat_pilot_position[0] = pos;
                }
            }
        }
        if let Some(model) = pack.collision_model_for(&profile.class_name) {
            if let Some((tree, geom)) = install_vehicle_pack_tree(collision, model, COLLISION_MASK_ORDNANCE) {
                let (mut lo, mut hi) = (geom.bounds_min, geom.bounds_max);
                let mut marked = |mask: u32| {
                    let (tree, geom) = install_vehicle_pack_tree(collision, model, mask)?;
                    lo = std::array::from_fn(|k| v8math::min(lo[k], geom.bounds_min[k]));
                    hi = std::array::from_fn(|k| v8math::max(hi[k], geom.bounds_max[k]));
                    Some(tree)
                };
                profile.flight_tree = marked(COLLISION_MASK_ORDNANCE | COLLISION_MASK_FLYER);
                profile.soldier_tree = marked(COLLISION_MASK_SOLDIER);
                profile.craft_tree = marked(COLLISION_MASK_BUILDING);
                profile.collision_tree = Some(tree);
                profile.bounds_min = lo;
                profile.bounds_max = hi;
                if let Some((cmin, cmax)) = model.hull_contact_bounds() {
                    profile.contact_bounds_min = cmin;
                    profile.contact_bounds_max = cmax;
                } else {
                    profile.contact_bounds_min = geom.bounds_min;
                    profile.contact_bounds_max = geom.bounds_max;
                }
                profile.collision_mesh_count = geom.mesh_count;
                profile.collision_primitive_count = geom.primitive_count;
                profile.hull_has_vehicle_pack = true;
                return;
            }
        }
    }
    let tree = install_bounds_box_tree(collision, profile.bounds_min, profile.bounds_max);
    profile.collision_tree = Some(tree);
    profile.hull_has_vehicle_pack = false;
}

pub(crate) fn quat_from_axis_angle(axis: [f64; 3], angle: f64) -> Quaternion {
    let half = angle * 0.5;
    let s = v8math::sin(half);
    let c = v8math::cos(half);
    Quaternion {
        x: axis[0] * s,
        y: axis[1] * s,
        z: axis[2] * s,
        w: c,
    }
}

fn quat_from_yaw(yaw: f64) -> [f64; 4] {
    let q = quat_from_axis_angle([0.0, 1.0, 0.0], yaw);
    [q.x, q.y, q.z, q.w]
}

fn to_q(q: [f64; 4]) -> Quaternion {
    Quaternion { x: q[0], y: q[1], z: q[2], w: q[3] }
}

fn from_q(q: &Quaternion) -> [f64; 4] {
    [q.x, q.y, q.z, q.w]
}

pub(crate) fn rotate_vec(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let mut vec = Vector3::new(v[0], v[1], v[2]);
    let quat = to_q(q);
    vec.apply_quaternion(&quat);
    [vec.x, vec.y, vec.z]
}

fn level_quat(q: [f64; 4]) -> [f64; 4] {
    let fwd = rotate_vec(q, [0.0, 0.0, 1.0]);
    let yaw = v8math::atan2(fwd[0], fwd[2]);
    quat_from_yaw(yaw)
}

/// An axis's rate for a stick deflection: below 0.7 of full stick the smaller
/// of TurnRate and PitchRate, blending to the axis's own `rate` at full stick.
fn effective_rate(p: &SpaceFlightProfile, stick: f64, rate: f64) -> f64 {
    let base = v8math::min(p.turn_rate, p.pitch_rate);
    if stick.abs() >= 0.7 {
        base + (rate - base) * v8math::min(1.0, (stick.abs() - 0.7) / 0.3)
    } else {
        base
    }
}

/// MinSpeed, MidSpeed and MaxSpeed, as the 2005 game caps them inside a
/// landing region: MinSpeed to a fifth and at most 10, MidSpeed at most 20,
/// MaxSpeed at most 60.
fn speeds(p: &SpaceFlightProfile, in_region: bool) -> [f64; 3] {
    if in_region {
        [
            v8math::min(p.min_speed * LANDING_REGION_MIN_SPEED_SHARE, LANDING_REGION_SPEEDS[0]),
            v8math::min(p.mid_speed, LANDING_REGION_SPEEDS[1]),
            v8math::min(p.max_speed, LANDING_REGION_SPEEDS[2]),
        ]
    } else {
        [p.min_speed, p.mid_speed, p.max_speed]
    }
}

/// The speed the throttle asks for: MidSpeed at rest, MaxSpeed held forward,
/// MinSpeed held back.
fn throttle_target_speed(p: &SpaceFlightProfile, throttle: f64, in_region: bool) -> f64 {
    let [min, mid, max] = speeds(p, in_region);
    let t = v8math::min(1.0, v8math::max(-1.0, throttle));
    if t >= 0.0 {
        mid + (max - mid) * t
    } else {
        mid + (mid - min) * t
    }
}

/// One step of the speed toward `target`: up by `step`, down by `step`, or
/// three times that inside a landing region, as in the 2005 game.
fn speed_toward(speed: f64, target: f64, step: f64, in_region: bool) -> f64 {
    let down = if in_region { LANDING_REGION_BRAKE } else { 1.0 } * step;
    v8math::min(speed + step, v8math::max(speed - down, target))
}

/// Casts a single ray stepping past up to 3 hangar forcefield slabs
/// (`UseVCollForFlyers = 1`). Returns the first solid blocking hit, if any.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cast_flyer_ray(
    origin: [f64; 3],
    dir: [f64; 3],
    max_dist: f64,
    ignore: i32,
    bounds: &SpaceBounds,
    collision: &mut CollisionOwner,
    collision_world: usize,
) -> Option<(i32, Hit, f64)> {
    let mut ray_origin = origin;
    let mut remaining = max_dist;
    let mut travelled = 0.0;
    for _ in 0..3 {
        if remaining <= 1e-6 {
            break;
        }
        let Some((code, h)) = collision.ray_hit(
            collision_world as u32,
            Ray {
                ox: ray_origin[0],
                oy: ray_origin[1],
                oz: ray_origin[2],
                dx: dir[0],
                dy: dir[1],
                dz: dir[2],
            },
            remaining,
            ignore,
        ) else {
            break;
        };
        if code == MISS {
            break;
        }
        if bounds.is_forcefield_hit(code, [h.px, h.py, h.pz], [h.nx, h.ny, h.nz]) {
            let advance = h.distance + PROVISIONAL_FORCEFIELD_STEP_PAD;
            if advance >= remaining {
                break;
            }
            ray_origin[0] += dir[0] * advance;
            ray_origin[1] += dir[1] * advance;
            ray_origin[2] += dir[2] * advance;
            travelled += advance;
            remaining -= advance;
            continue;
        }
        return Some((code, h, travelled + h.distance));
    }
    None
}

/// Multi-point hull sweep testing nose, left/right wingtips, dorsal fin, and
/// (when airborne) ventral keel along displacement `disp`. Prevents wide
/// wingspans from clipping through hangar doorways or capital hulls during
/// `Flying`, `TakingOff`, and `Landing`. Zero heap allocations.
#[allow(clippy::too_many_arguments)]
fn sweep_hull_displacement(
    position: [f64; 3],
    quaternion: [f64; 4],
    disp: [f64; 3],
    profile: &SpaceFlightProfile,
    include_ventral: bool,
    ignore: i32,
    bounds: &SpaceBounds,
    collision: &mut CollisionOwner,
    collision_world: usize,
) -> Option<(Hit, f64, i32)> {
    let dist = v8math::hypot3(disp[0], disp[1], disp[2]);
    if dist <= 1e-6 {
        return None;
    }
    let dir = [disp[0] / dist, disp[1] / dist, disp[2] / dist];
    let lo = profile.contact_bounds_min;
    let hi = profile.contact_bounds_max;
    let mid_y = 0.5 * (lo[1] + hi[1]);
    let mid_z = 0.5 * (lo[2] + hi[2]);

    // Fixed stack array of hull probe points in model space.
    let probes: [([f64; 3], f64); 5] = [
        ([0.0, mid_y, hi[2]], PROVISIONAL_MIN_SWEEP_PROBE),
        ([lo[0], mid_y, mid_z], PROVISIONAL_MIN_SWEEP_PROBE),
        ([hi[0], mid_y, mid_z], PROVISIONAL_MIN_SWEEP_PROBE),
        ([0.0, hi[1], mid_z], PROVISIONAL_MIN_SWEEP_PROBE),
        ([0.0, lo[1], mid_z], PROVISIONAL_MIN_SWEEP_PROBE),
    ];
    let count = if include_ventral { 5 } else { 4 };

    let mut earliest: Option<(Hit, f64, i32)> = None;
    for &(local_pt, pad) in &probes[..count] {
        let world_off = rotate_vec(quaternion, local_pt);
        let probe_origin = [
            position[0] + world_off[0],
            position[1] + world_off[1],
            position[2] + world_off[2],
        ];
        // Cast from hull center outward to the probe point if moving, or sweep
        // from the probe point along `disp`.
        let max_d = dist + pad;
        if let Some((code, h, total_d)) =
            cast_flyer_ray(probe_origin, dir, max_d, ignore, bounds, collision, collision_world)
        {
            let safe_d = v8math::max(0.0, total_d - pad);
            if earliest.is_none_or(|(_, best_safe, _)| safe_d < best_safe) {
                earliest = Some((h, safe_d, code));
            }
        }
    }
    earliest
}

impl SpaceFighter {
    pub fn seat_world_position(&self, profile: &SpaceFlightProfile, seat: u8) -> [f64; 3] {
        let off = rotate_vec(self.quaternion, profile.pilot_local_position(seat));
        [
            self.position[0] + off[0],
            self.position[1] + off[1],
            self.position[2] + off[2],
        ]
    }

    pub fn dismount_position(&self, profile: &SpaceFlightProfile) -> [f64; 3] {
        let right = self.right();
        let offset = v8math::max(PROVISIONAL_DISMOUNT_MIN_OFFSET, profile.contact_bounds_max[0] + PROVISIONAL_DISMOUNT_WING_PAD);
        [self.position[0] + right[0]*offset, self.position[1] + profile.contact_bounds_min[1], self.position[2] + right[2]*offset]
    }
    pub fn occupant(&self, seat: u8) -> Option<SpacePilotId> {
        if seat == 0 { self.pilot } else { self.gunners.get(seat as usize - 1).copied().flatten() }
    }
    pub fn occupant_mut(&mut self, seat: u8) -> Option<&mut Option<SpacePilotId>> {
        if seat == 0 { Some(&mut self.pilot) } else { self.gunners.get_mut(seat as usize - 1) }
    }
    /// Shared weapon-mount selection rule: an explicit `selected_weapon` selects
    /// only that mount; otherwise each channel's earliest mount for `seat` is active.
    pub fn weapon_selected(
        weapons: &[SpaceWeaponSpec],
        seat: u8,
        selected_weapon: Option<u8>,
        index: usize,
    ) -> bool {
        let Some(spec) = weapons.get(index) else {
            return false;
        };
        if spec.seat != seat {
            return false;
        }
        match selected_weapon {
            Some(selected) => usize::from(selected) == index,
            None => !weapons[..index]
                .iter()
                .any(|w| w.seat == seat && w.channel == spec.channel),
        }
    }

    /// Returns the lock state of the active selected weapon for `seat`, or
    /// `SpaceLockState::default()` when no selected mount holds a lock.
    pub fn selected_lock(
        &self,
        weapons: &[SpaceWeaponSpec],
        seat: u8,
        selected_weapon: Option<u8>,
    ) -> SpaceLockState {
        let mut active = SpaceLockState::default();
        for (i, lock) in self
            .weapon_locks
            .iter()
            .take(weapons.len())
            .enumerate()
        {
            if Self::weapon_selected(weapons, seat, selected_weapon, i)
                && lock.kind != SpaceLockTargetKind::None
            {
                active = *lock;
            }
        }
        active
    }

    pub(super) fn weapon_trigger(&self, weapons: &[SpaceWeaponSpec], index: usize) -> (bool,bool) {
        let Some(spec)=weapons.get(index) else {return (false,false)};
        if self.occupant(spec.seat).is_none() {return (false,false)}
        let input=self.seat_input(spec.seat);
        let selected=Self::weapon_selected(weapons, spec.seat, input.weapon_index, index);
        let fire=if input.weapon_index.is_some() {input.fire_primary || input.fire_secondary}
            else if spec.channel==0 {input.fire_primary} else {input.fire_secondary};
        (selected,selected && fire)
    }

    /// Holds a gunner's world aim `dir` to his seat's limits (`limits` as
    /// [`SpaceFlightProfile::seat_aim_limits`] keeps them): yaw from the nose,
    /// positive to the left, and pitch, positive down. A yaw arc of a whole
    /// turn or more turns freely.
    pub fn seat_aim_within(&self, limits: [f64; 4], dir: [f64; 3]) -> [f64; 3] {
        let (f, l, u) = (self.forward(), self.right(), self.up());
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let free_yaw = limits[1] - limits[0] >= 2.0 * PI;
        let yaw = v8math::atan2(dot(dir, l), dot(dir, f));
        let yaw = if free_yaw { yaw } else { v8math::min(limits[1], v8math::max(limits[0], yaw)) };
        let down = -v8math::asin(v8math::min(1.0, v8math::max(-1.0, dot(dir, u))));
        let down = v8math::min(limits[3], v8math::max(limits[2], down));
        let (cy, sy, cp, sp) = (v8math::cos(yaw), v8math::sin(yaw), v8math::cos(down), v8math::sin(down));
        std::array::from_fn(|i| f[i] * cy * cp + l[i] * sy * cp - u[i] * sp)
    }

    /// Whether a gunner in a seat with `limits` can bear on `point`.
    /// Turning while taking off or landing: the stick's pitch counts by the
    /// lift and its yaw by the height over TakeoffHeight (`reach`); while the
    /// lift is low the nose is drawn level and the turn damped, and the wings
    /// are levelled, all at LevelFilterLanding. The rates follow their targets
    /// at PitchFilter and TurnFilter.
    fn lifting_attitude(&mut self, profile: &SpaceFlightProfile, input: &SpacePilotInput, lift: f64, reach: f64, dt: f64) {
        let sx = v8math::min(1.0, v8math::max(-1.0, input.stick_x));
        let sy = v8math::min(1.0, v8math::max(-1.0, input.stick_y));
        let fwd = self.forward();
        let left = self.right();
        let level = profile.level_filter_landing;
        self.pitch_rate *= v8math::max(0.0, 1.0 - (1.0 - lift) * level * dt);
        self.pitch_rate += fwd[1] * level * 0.5 * (1.0 - lift);
        let pitch_target = -sy * effective_rate(profile, sy, profile.pitch_rate) * lift;
        self.pitch_rate += (pitch_target - self.pitch_rate) * v8math::min(1.0, profile.pitch_filter * dt);
        let yaw_target = -sx * effective_rate(profile, sx, profile.turn_rate) * reach;
        self.yaw_rate += (yaw_target - self.yaw_rate) * v8math::min(1.0, profile.turn_filter * dt);
        self.roll_rate = -left[1] * level;
        let (dp, dy, dr) = (self.pitch_rate * dt, self.yaw_rate * dt, self.roll_rate * dt);
        let ang = v8math::hypot3(dp, dy, dr);
        if ang > 1e-12 {
            let dq = quat_from_axis_angle([dp / ang, dy / ang, dr / ang], ang);
            let mut q = to_q(self.quaternion);
            q.multiply(&dq).normalize();
            self.quaternion = from_q(&q);
        }
    }

    /// The pitch stick after the fly-height rule: within two seconds of the
    /// battlefield's floor or ceiling at the present climb, the stick is
    /// drawn toward the input that would level the nose in one tick of
    /// `pitch_step` (the pitch rate times the tick), by one minus the square
    /// of that time over two seconds, and wholly once the limit is reached.
    /// Nothing is drawn while the nose already points away from the limit.
    pub fn fly_height_stick(&self, stick: f64, bounds: &SpaceBounds, pitch_step: f64) -> f64 {
        let climb = self.velocity[1];
        let (to_limit, away) = if climb > 0.0 {
            ((bounds.max[1] - self.position[1]) * 0.5 / climb, -1.0)
        } else if climb < 0.0 {
            ((bounds.min[1] - self.position[1]) * 0.5 / climb, 1.0)
        } else {
            return stick;
        };
        let reach = v8math::min(1.0, v8math::max(0.0, to_limit));
        if !(reach < 1.0) || !(pitch_step > 0.0) {
            return stick;
        }
        let fwd = self.forward();
        let mut nose = fwd[1] * away;
        let forward_speed = self.velocity[0] * fwd[0] + self.velocity[1] * fwd[1] + self.velocity[2] * fwd[2];
        if forward_speed * nose > 0.0 {
            return stick;
        }
        if self.up()[1] * away > 0.0 {
            nose = -nose;
        }
        let level = v8math::min(1.0, v8math::max(-1.0, nose / pitch_step));
        (stick - level) * reach * reach + level
    }

    pub fn seat_can_bear(&self, limits: [f64; 4], point: [f64; 3]) -> bool {
        let d = [point[0] - self.position[0], point[1] - self.position[1], point[2] - self.position[2]];
        let n = v8math::hypot3(d[0], d[1], d[2]);
        if n <= 1e-9 {
            return true;
        }
        let d = [d[0] / n, d[1] / n, d[2] / n];
        let held = self.seat_aim_within(limits, d);
        held[0] * d[0] + held[1] * d[1] + held[2] * d[2] >= v8math::cos(PROVISIONAL_SEAT_BEAR_CONE)
    }

    pub fn seat_input(&self, seat: u8) -> SpacePilotInput {
        if seat == 0 { self.pending_input } else { self.gunner_inputs.get(seat as usize - 1).copied().unwrap_or_default() }
    }

    /// Creates a newly parked fighter on `pad` using `profile`.
    #[allow(clippy::too_many_arguments)]
    pub fn spawned_on_pad(
        id: usize,
        generation: u32,
        pad_index: usize,
        pad: &SpaceVehiclePad,
        profile_index: usize,
        profile: &SpaceFlightProfile,
        floor_y: f64,
        landed_zone: Option<usize>,
        kernel_body: Option<u32>,
    ) -> Self {
        let settled_y = floor_y - profile.contact_bounds_min[1] + PROVISIONAL_GEAR_CLEARANCE_Y;
        let position = [pad.position[0], settled_y, pad.position[2]];
        let quaternion = quat_from_yaw(pad.yaw);
        let mut weapons = [SpaceWeaponRuntime::default(); 5];
        for (i, w) in profile.weapons.iter().take(5).enumerate() {
            weapons[i].ammo_in_clip = w.rounds_per_clip;
        }
        let mut f = Self {
            id,
            generation,
            pad_index,
            profile_index,
            team: pad.team,
            alive: true,
            health: profile.max_health,
            max_health: profile.max_health,
            state: SpaceFlightState::Landed,
            state_time: 0.0,
            gear: 1.0,
            prev_gear: 1.0,
            position,
            previous_position: position,
            velocity: [0.0; 3],
            quaternion,
            previous_quaternion: quaternion,
            airspeed: 0.0,
            yaw_rate: 0.0,
            pitch_rate: 0.0,
            roll_rate: 0.0,
            roll_state: 0.0,
            level_timer: 15.0,
            yaw_buildup: 0.0,
            pitch_buildup: 0.0,
            bank: 0.0,
            prev_bank: 0.0,
            energy: profile.energy_bar,
            boosting: false,
            boost_exhausted: false,
            trick: SpaceTrick::None,
            trick_progress: 0.0,
            trick_cooldown: 0.0,
            trick_axis_right: [1.0, 0.0, 0.0],
            action_held: false,
            eject_timer: 0.0,
            oob_timer: -1.0,
            oob_warning: false,
            empty_timer: 0.0,
            pilot: None,
            gunners: [None; 5],
            gunner_inputs: [SpacePilotInput::default(); 5],
            landed_zone,
            mobile_spawn_active: false,
            kernel_body,
            matrix: Matrix4::default().elements,
            inverse: Matrix4::default().elements,
            weapons,
            lock: SpaceLockState::default(),
            weapon_locks: [SpaceLockState::default(); 5],
            pending_input: SpacePilotInput::default(),
            contacts: 0,
            contact_normal: [0.0; 3],
            contact_body: -1,
        };
        f.recompute_matrix();
        f
    }

    /// Its model's bounding sphere where it is: centre and radius.
    pub fn sphere(&self, profile: &SpaceFlightProfile) -> ([f64; 3], f64) {
        let [x, y, z, radius] = profile.sphere;
        let off = rotate_vec(self.quaternion, [x, y, z]);
        (std::array::from_fn(|k| self.position[k] + off[k]), radius)
    }

    pub fn forward(&self) -> [f64; 3] {
        rotate_vec(self.quaternion, [0.0, 0.0, 1.0])
    }

    pub fn right(&self) -> [f64; 3] {
        rotate_vec(self.quaternion, [1.0, 0.0, 0.0])
    }

    pub fn up(&self) -> [f64; 3] {
        rotate_vec(self.quaternion, [0.0, 1.0, 0.0])
    }

    fn landing_permission(&self, bounds: &SpaceBounds, zone: Option<usize>) -> SpaceLandingPermission {
        use SpaceLandingPermission::*;
        if !self.alive || self.pilot.is_none() ||
            !matches!(self.state, SpaceFlightState::Flying | SpaceFlightState::TakingOff) {
            return Unavailable;
        }
        if self.trick.active() { return Maneuver; }
        let Some(zone) = zone else { return OutsideRegion; };
        match bounds.landing_zones[zone].floor_y {
            Some(floor_y) => Allowed { zone, floor_y },
            None => NoFloor,
        }
    }

    /// Re-evaluate the pilot action against the current fixed-tick pose and registered decks.
    pub fn landing_at(&self, bounds: &SpaceBounds) -> SpaceLandingPermission {
        let zone = bounds.landing_zones.iter().position(|z| z.contains(self.position));
        self.landing_permission(bounds, zone)
    }

    pub fn occupied(&self) -> bool {
        self.pilot.is_some() || self.gunners.iter().any(Option::is_some)
    }

    /// True while performing an evasive barrel roll (breaks incoming missile locks).
    pub fn evading_missiles(&self) -> bool {
        matches!(self.trick, SpaceTrick::BarrelLeft | SpaceTrick::BarrelRight)
    }

    pub fn recompute_matrix(&mut self) {
        let mut m = Matrix4::default();
        let pos = Vector3::new(self.position[0], self.position[1], self.position[2]);
        let q = to_q(self.quaternion);
        m.compose(&pos, &q, &Vector3::new(1.0, 1.0, 1.0));
        let mut inv = m;
        inv.invert();
        self.matrix = m.elements;
        self.inverse = inv.elements;
    }

    /// Shows `meets` the bodies of this craft's hull that stop it, until
    /// another call or the next sync (which shows shots theirs).
    pub fn show_hull(&self, profile: &SpaceFlightProfile, collision: &mut CollisionOwner, meets: HullMeets) {
        let shown = match meets {
            HullMeets::Shots if self.state == SpaceFlightState::Flying => profile.flight_tree,
            HullMeets::Shots => None,
            HullMeets::Soldiers => profile.soldier_tree,
            HullMeets::Craft => profile.craft_tree,
        };
        if let (Some(kernel), Some(tree)) = (self.kernel_body, shown.or(profile.collision_tree)) {
            if let Some(body) = collision.state.bodies.get_mut(kernel as usize) {
                body.tree = tree as usize;
            }
        }
    }

    /// Syncs this fighter's moving collision body in `collision`, showing
    /// shots their bodies (zero allocation, never bumps `fixed_version`).
    pub fn sync_collision_body(
        &self,
        profile: &SpaceFlightProfile,
        collision: &mut CollisionOwner,
        collision_world: usize,
        flags: &mut [u8],
    ) {
        let Some(kernel) = self.kernel_body else {
            return;
        };
        let local: Bounds = [
            profile.bounds_min[0],
            profile.bounds_min[1],
            profile.bounds_min[2],
            profile.bounds_max[0],
            profile.bounds_max[1],
            profile.bounds_max[2],
        ];
        if let Some(body) = collision.state.bodies.get_mut(kernel as usize) {
            body.bounds = box_apply_matrix4(&local, &self.matrix);
            body.matrix = self.matrix;
            body.inverse = self.inverse;
        }
        self.show_hull(profile, collision, HullMeets::Shots);
        let enabled = self.alive;
        collision.enable_body(kernel, enabled);
        if let Some(order) = collision
            .state
            .worlds
            .get(collision_world)
            .and_then(|w| w.order.iter().position(|&s| s == kernel))
        {
            if let Some(slot) = flags.get_mut(order) {
                *slot = u8::from(enabled);
            }
        } else if let Some(slot) = flags.get_mut(kernel as usize) {
            *slot = u8::from(enabled);
        }
    }

    /// Returns world-space mobile spawn coordinates when this `commandflyer`
    /// is landed inside a hangar landing zone, cycling across all authored
    /// `SpawnPointLocation` offsets.
    pub fn mobile_spawn_point(&self, profile: &SpaceFlightProfile, slot: u32) -> Option<[f64; 3]> {
        if !self.alive || !self.mobile_spawn_active || profile.spawn_point_count == 0 {
            return None;
        }
        let local = if !profile.spawn_point_offsets.is_empty() {
            profile.spawn_point_offsets[(slot as usize) % profile.spawn_point_offsets.len()]
        } else {
            let angle = (slot % profile.spawn_point_count) as f64
                * (TAU / profile.spawn_point_count as f64);
            [
                v8math::cos(angle) * PROVISIONAL_MOBILE_SPAWN_SPREAD,
                0.0,
                v8math::sin(angle) * PROVISIONAL_MOBILE_SPAWN_SPREAD,
            ]
        };
        let off = rotate_vec(self.quaternion, local);
        Some([
            self.position[0] + off[0],
            self.position[1] + off[1],
            self.position[2] + off[2],
        ])
    }

    /// Applies direct or splash damage to this starfighter hull. Returns true
    /// if this hit destroyed the hull.
    pub fn apply_damage(&mut self, amount: f64) -> bool {
        if !self.alive || !amount.is_finite() || amount <= 0.0 {
            return false;
        }
        self.health = v8math::max(0.0, self.health - amount);
        if self.health <= 0.0 {
            self.alive = false;
            self.mobile_spawn_active = false;
            self.velocity = [0.0; 3];
            self.airspeed = 0.0;
            self.boosting = false;
            self.trick = SpaceTrick::None;
            true
        } else {
            false
        }
    }

    /// Applies hull contact damage and normal slide after a multi-point sweep
    /// hits solid geometry.
    fn apply_hull_contact(&mut self, h: Hit, safe_d: f64, body: i32, disp: [f64; 3], profile: &SpaceFlightProfile) -> bool {
        self.contacts = self.contacts.wrapping_add(1);
        self.contact_normal = [h.nx, h.ny, h.nz];
        self.contact_body = body;
        let impact_speed = -(self.velocity[0] * h.nx + self.velocity[1] * h.ny + self.velocity[2] * h.nz);
        let impact_speed = v8math::max(0.0, impact_speed);
        let mut destroyed = false;
        if impact_speed > profile.collision_threshold {
            let dmg = if impact_speed >= PROVISIONAL_LETHAL_IMPACT_SPEED {
                self.max_health
            } else {
                (impact_speed - profile.collision_threshold)
                    * profile.collision_scale
                    * PROVISIONAL_COLLISION_DAMAGE_FACTOR
            };
            if self.apply_damage(dmg) {
                destroyed = true;
            }
        }
        if self.alive {
            let vn = self.velocity[0] * h.nx + self.velocity[1] * h.ny + self.velocity[2] * h.nz;
            if vn < 0.0 {
                self.velocity[0] -= h.nx * vn;
                self.velocity[1] -= h.ny * vn;
                self.velocity[2] -= h.nz * vn;
            }
            let dist = v8math::hypot3(disp[0], disp[1], disp[2]);
            if dist > 1e-6 {
                let frac = v8math::min(1.0, safe_d / dist);
                self.position[0] += disp[0] * frac;
                self.position[1] += disp[1] * frac;
                self.position[2] += disp[2] * frac;
            }
        }
        destroyed
    }

    /// Advances only craft orientation by its current angular velocity for one tick.
    /// Used by gunner-seat prediction so predicted world-space aim follows a turning
    /// craft across round-trip latency without simulating pilot stick input.
    pub fn extrapolate_attitude(&mut self, dt: f64) {
        if !self.alive || self.state != SpaceFlightState::Flying {
            return;
        }
        let (dp, dy, dr) = (self.pitch_rate * dt, self.yaw_rate * dt, self.roll_rate * dt);
        let ang = v8math::hypot3(dp, dy, dr);
        if ang > 1e-12 {
            let dq = quat_from_axis_angle([dp / ang, dy / ang, dr / ang], ang);
            let mut q = to_q(self.quaternion);
            q.multiply(&dq).normalize();
            self.quaternion = from_q(&q);
        }
    }

    /// Advances this starfighter by one fixed 60 Hz tick. Zero allocations.
    #[allow(clippy::too_many_arguments)]
    pub fn step_tick(
        &mut self,
        dt: f64,
        profile: &SpaceFlightProfile,
        pad: &SpaceVehiclePad,
        bounds: &SpaceBounds,
        collision: &mut CollisionOwner,
        collision_world: usize,
        flags: &mut [u8],
    ) -> FighterStepNotice {
        let mut notice = FighterStepNotice::default();
        if !self.alive {
            return notice;
        }
        self.previous_position = self.position;
        self.previous_quaternion = self.quaternion;
        self.prev_bank = self.bank;
        self.prev_gear = self.gear;
        self.state_time += dt;
        self.trick_cooldown = v8math::max(0.0, self.trick_cooldown - dt);

        // Update weapon timers (barrage, reload, heat).
        for (i, spec) in profile.weapons.iter().take(5).enumerate() {
            let held = self.weapon_trigger(&profile.weapons, i).1;
            let player = self.occupant(spec.seat).is_some_and(|p| p.is_human);
            let rt = &mut self.weapons[i];
            if rt.barrage_timer > 0.0 {
                rt.barrage_timer = v8math::max(0.0, rt.barrage_timer - dt);
            }
            if rt.reload_timer > 0.0 {
                rt.reload_timer = v8math::max(0.0, rt.reload_timer - dt);
                if rt.reload_timer <= 0.0 && spec.rounds_per_clip > 0 {
                    rt.ammo_in_clip = spec.rounds_per_clip;
                }
            }
            if rt.heat > 0.0 {
                rt.heat = v8math::max(0.0, rt.heat - spec.heat_recover_rate * dt);
                if rt.overheated && rt.heat <= spec.heat_threshold {
                    rt.overheated = false;
                }
            }
            let stopped = rt.reload_timer > 0.0 || rt.overheated;
            rt.kick.tick(spec, held, rt.warm_up, rt.clock.done(), stopped, player, dt);
        }

        let input = if self.pilot.is_some() {
            self.pending_input
        } else {
            SpacePilotInput::default()
        };

        // The provisional repair rate interprets PilotSkillRepairScale as a
        // fraction of maximum hull per second while a repair-capable pilot is seated.
        if let Some(pilot) = self.pilot {
            if pilot.repair_capable && profile.pilot_skill_repair_scale > 0.0 && self.health < self.max_health {
                let heal = profile.pilot_skill_repair_scale * self.max_health * dt;
                self.health = v8math::min(self.max_health, self.health + heal);
            }
        }

        // Ejection / hangar exit handling.
        if let Some(pilot) = self.pilot {
            if input.eject_held {
                if self.state == SpaceFlightState::Landed {
                    self.pilot = None;
                    self.eject_timer = 0.0;
                    notice.dismounted_pilot = Some(pilot);
                } else {
                    self.eject_timer += dt;
                    // Exterior holds keep the rider aboard. Longer holds
                    // extrapolate the observed 3.2 seconds of open flight.
                    if self.eject_timer >= profile.time_required_to_eject && bounds.landing_zone_at(self.position).is_some() {
                        self.pilot = None;
                        self.eject_timer = 0.0;
                        notice.dismounted_pilot = Some(pilot);
                    }
                }
            } else {
                self.eject_timer = 0.0;
            }
        }

        // Current landing zone containment.
        let zone_idx = bounds
            .landing_zones
            .iter()
            .position(|z| z.contains(self.position));

        // Edge-triggered takeoff / landing transitions.
        let action_pressed = input.takeoff_or_land && !self.action_held;
        self.action_held = input.takeoff_or_land;
        if self.pilot.is_some() && action_pressed {
            match self.state {
                SpaceFlightState::Landed => {
                    self.state = SpaceFlightState::TakingOff;
                    self.state_time = 0.0;
                    self.mobile_spawn_active = false;
                    self.landed_zone = None;
                    notice.took_off = true;
                }
                SpaceFlightState::Flying | SpaceFlightState::TakingOff => {
                    if matches!(self.landing_permission(bounds, zone_idx), SpaceLandingPermission::Allowed { .. }) {
                        self.state = SpaceFlightState::Landing;
                        self.state_time = 0.0;
                    }
                }
                SpaceFlightState::Landing if self.gear > PROVISIONAL_LANDING_CANCEL_GEAR => {
                    self.state = SpaceFlightState::TakingOff;
                    self.state_time = 0.0;
                    notice.took_off = true;
                }
                _ => {}
            }
        }

        self.boosting = false;
        let ignore = self.kernel_body.map_or(-1, |k| k as i32);

        match self.state {
            SpaceFlightState::Landed => {
                self.velocity = [0.0; 3];
                self.airspeed = 0.0;
                self.yaw_rate = 0.0;
                self.pitch_rate = 0.0;
                self.roll_rate = 0.0;
                self.roll_state = 0.0;
                self.level_timer = 15.0;
                self.yaw_buildup = 0.0;
                self.pitch_buildup = 0.0;
                self.bank = 0.0;
                self.gear = 1.0;
                self.landed_zone = zone_idx;
                self.mobile_spawn_active = profile.is_command_flyer
                    && profile.spawn_point_count > 0
                    && zone_idx.is_some();
            }
            SpaceFlightState::TakingOff | SpaceFlightState::Landing => {
                // Lifting off or settling, as the 2005 game flies it: the motion
                // blends from straight up (or down) at a speed that grows with the
                // height over TakeoffHeight into flight, by the lift (1 - gear).
                let landing = self.state == SpaceFlightState::Landing;
                self.roll_state = 0.0;
                self.level_timer = 15.0;
                self.yaw_buildup = 0.0;
                self.pitch_buildup = 0.0;
                // The deck under the craft, by a ray straight down as the game finds it;
                // the region's or the pad's floor only where the ray finds nothing.
                let deck_y = cast_flyer_ray(
                    self.position,
                    [0.0, -1.0, 0.0],
                    DECK_RAY_REACH,
                    ignore,
                    bounds,
                    collision,
                    collision_world,
                )
                .map(|(_, hit, _)| hit.py)
                .filter(|y| y.is_finite())
                .or_else(|| zone_idx.and_then(|idx| bounds.landing_zones[idx].floor_y))
                .unwrap_or(pad.floor_y);
                // Where the craft rests: its hull's underside on the deck.
                let rest = -profile.contact_bounds_min[1] + PROVISIONAL_GEAR_CLEARANCE_Y;
                let settled_y = deck_y + rest;
                let height = self.position[1] - deck_y;
                if landing {
                    self.gear = v8math::min(1.0, self.gear + dt / profile.landing_time);
                } else if height > profile.takeoff_height + rest {
                    // The lift rises only once clear of the deck by TakeoffHeight.
                    self.gear = v8math::max(0.0, self.gear - dt / profile.takeoff_time);
                }
                let lift = 1.0 - self.gear;
                let reach = v8math::min(1.0, v8math::max(0.0, height / profile.takeoff_height));
                // The speed along the nose follows the throttle as in flight; the
                // lift scales how much of it moves the craft. As in flight, the speed it
                // starts from each tick is the craft's own motion along its nose, so
                // while the lift is partial the speed stays low and the craft rises and
                // settles nearly over its pad.
                let nose = self.forward();
                self.airspeed = self.velocity[0] * nose[0] + self.velocity[1] * nose[1] + self.velocity[2] * nose[2];
                let in_region = zone_idx.is_some();
                let target = throttle_target_speed(profile, input.throttle, in_region);
                self.airspeed = speed_toward(self.airspeed, target, profile.acceleration * dt, in_region);
                self.lifting_attitude(profile, &input, lift, reach, dt);
                let vertical = (0.875 * reach + 0.125)
                    * if landing { -profile.landing_speed } else { profile.takeoff_speed };
                let fwd = self.forward();
                self.velocity = [
                    fwd[0] * self.airspeed * lift,
                    vertical + (fwd[1] * self.airspeed - vertical) * lift,
                    fwd[2] * self.airspeed * lift,
                ];
                let mut disp = [self.velocity[0] * dt, self.velocity[1] * dt, self.velocity[2] * dt];
                if self.position[1] + disp[1] < settled_y {
                    disp[1] = v8math::min(0.0, settled_y - self.position[1]);
                }
                if let Some((h, safe_d, body)) = sweep_hull_displacement(
                    self.position,
                    self.quaternion,
                    disp,
                    profile,
                    false,
                    ignore,
                    bounds,
                    collision,
                    collision_world,
                ) {
                    if self.apply_hull_contact(h, safe_d, body, disp, profile) {
                        notice.destroyed = true;
                    }
                } else {
                    self.position[0] += disp[0];
                    self.position[1] += disp[1];
                    self.position[2] += disp[2];
                }
                if !self.alive {
                    // Lost in contact: nothing more to settle.
                } else if !landing && self.gear <= 0.0 {
                    self.gear = 0.0;
                    self.state = SpaceFlightState::Flying;
                    self.state_time = 0.0;
                } else if landing
                    && self.position[1] - settled_y <= PROVISIONAL_LANDING_SETTLE_EPS
                    && self.up()[1] > LANDING_LEVEL_COS
                {
                    // Down on the deck and near enough level.
                    self.position[1] = settled_y;
                    self.quaternion = level_quat(self.quaternion);
                    self.velocity = [0.0; 3];
                    self.airspeed = 0.0;
                    self.gear = 1.0;
                    self.state = SpaceFlightState::Landed;
                    self.state_time = 0.0;
                    self.landed_zone = zone_idx;
                    self.mobile_spawn_active = profile.is_command_flyer
                        && profile.spawn_point_count > 0
                        && zone_idx.is_some();
                    notice.landed_zone = zone_idx;
                }
            }
            SpaceFlightState::Flying => {
                if self.pilot.is_none() {
                    // Zero-gravity pilotless drift in open space.
                    let damp = v8math::max(0.0, 1.0 - PROVISIONAL_PILOTLESS_DRAG * dt);
                    self.velocity[0] *= damp;
                    self.velocity[1] *= damp;
                    self.velocity[2] *= damp;
                    self.airspeed = v8math::hypot3(self.velocity[0], self.velocity[1], self.velocity[2]);
                } else {
                    // Boost & energy management.
                    if !input.boost {
                        self.boost_exhausted = false;
                    }
                    let want_boost = input.boost && input.throttle > 0.0
                        && self.energy > 0.0
                        && !self.boost_exhausted
                        && !self.trick.active();
                    if want_boost {
                        self.boosting = true;
                        self.energy = v8math::max(0.0, self.energy - profile.energy_boost_drain * dt);
                        if self.energy <= 0.0 {
                            self.boost_exhausted = true;
                        }
                    }

                    // Tricks (disabled when EnergyTrickDrainSingleTap == 0).
                    let tricks_enabled = profile.energy_trick_single > 0.0
                        && (profile.trick_roll_speed > 0.0 || profile.trick_flip_speed > 0.0);
                    if tricks_enabled && !self.trick.active() && self.trick_cooldown <= 0.0 {
                        if input.trick_flip != 0
                            && profile.trick_flip_speed > 0.0
                            && self.energy >= profile.energy_trick_double
                        {
                            self.energy = v8math::max(0.0, self.energy - profile.energy_trick_double);
                            self.trick = if input.trick_flip == 2 {
                                SpaceTrick::Immelmann
                            } else {
                                SpaceTrick::Loop
                            };
                            self.trick_progress = 0.0;
                            self.trick_axis_right = self.right();
                        } else if input.trick_roll != 0
                            && profile.trick_roll_speed > 0.0
                            && self.energy >= profile.energy_trick_single
                        {
                            self.energy = v8math::max(0.0, self.energy - profile.energy_trick_single);
                            self.trick = if input.trick_roll < 0 {
                                SpaceTrick::BarrelLeft
                            } else {
                                SpaceTrick::BarrelRight
                            };
                            self.trick_progress = 0.0;
                            self.trick_axis_right = self.right();
                        }
                    }

                    // Speed integration.
                    let in_region = zone_idx.is_some();
                    let target_speed = if self.boosting {
                        v8math::max(profile.max_speed, profile.boost_speed)
                    } else {
                        throttle_target_speed(profile, input.throttle, in_region)
                    };
                    let accel_rate = if self.boosting {
                        profile.boost_acceleration
                    } else {
                        profile.acceleration
                    };
                    self.airspeed = speed_toward(self.airspeed, target_speed, accel_rate * dt, in_region);

                    // Attitude & trick motion.
                    let mut strafe = [0.0; 3];
                    match self.trick {
                        SpaceTrick::BarrelLeft | SpaceTrick::BarrelRight => {
                            let dir = if self.trick == SpaceTrick::BarrelRight { 1.0 } else { -1.0 };
                            let d_angle = profile.trick_roll_speed * dt;
                            let dq = quat_from_axis_angle([0.0, 0.0, 1.0], dir * d_angle);
                            let mut q = to_q(self.quaternion);
                            q.multiply(&dq).normalize();
                            self.quaternion = from_q(&q);
                            self.trick_progress += d_angle;
                            let r = self.trick_axis_right;
                            strafe = [
                                r[0] * dir * profile.trick_strafe_speed,
                                r[1] * dir * profile.trick_strafe_speed,
                                r[2] * dir * profile.trick_strafe_speed,
                            ];
                            if self.trick_progress >= TAU {
                                self.trick = SpaceTrick::None;
                                self.trick_progress = 0.0;
                                self.trick_cooldown = PROVISIONAL_TRICK_COOLDOWN;
                            }
                        }
                        SpaceTrick::Loop => {
                            let d_angle = profile.trick_flip_speed * dt;
                            let dq = quat_from_axis_angle([1.0, 0.0, 0.0], -d_angle);
                            let mut q = to_q(self.quaternion);
                            q.multiply(&dq).normalize();
                            self.quaternion = from_q(&q);
                            self.trick_progress += d_angle;
                            if self.trick_progress >= TAU {
                                self.trick = SpaceTrick::None;
                                self.trick_progress = 0.0;
                                self.trick_cooldown = PROVISIONAL_TRICK_COOLDOWN;
                            }
                        }
                        SpaceTrick::Immelmann => {
                            let d_angle = profile.trick_flip_speed * dt;
                            let mut q = to_q(self.quaternion);
                            if self.trick_progress < PI {
                                let step = v8math::min(d_angle, PI - self.trick_progress);
                                let dq = quat_from_axis_angle([1.0, 0.0, 0.0], -step);
                                q.multiply(&dq).normalize();
                                self.trick_progress += step;
                            } else {
                                let step = v8math::min(d_angle, TAU - self.trick_progress);
                                let dq = quat_from_axis_angle([0.0, 0.0, 1.0], step);
                                q.multiply(&dq).normalize();
                                self.trick_progress += step;
                            }
                            self.quaternion = from_q(&q);
                            if self.trick_progress >= TAU - 1e-6 {
                                self.trick = SpaceTrick::None;
                                self.trick_progress = 0.0;
                                self.trick_cooldown = PROVISIONAL_TRICK_COOLDOWN;
                            }
                        }
                        SpaceTrick::None => {
                            let raw_sx = v8math::min(1.0, v8math::max(-1.0, input.stick_x));
                            let sy = v8math::min(1.0, v8math::max(-1.0, input.stick_y));
                            let raw_roll = if input.roll_lock {
                                raw_sx
                            } else {
                                v8math::min(1.0, v8math::max(-1.0, input.roll))
                            };
                            let sx = if input.roll_lock { 0.0 } else { raw_sx };

                            // Roll deadzone (0.3 / 0.7) and exponential filter (RollRateFilter).
                            let roll_cmd = if raw_roll.abs() >= 0.3 {
                                let sign = if raw_roll > 0.0 { 1.0 } else { -1.0 };
                                v8math::min(1.0, v8math::max(-1.0, (raw_roll - sign * 0.3) / 0.7))
                            } else {
                                0.0
                            };
                            self.roll_state += (roll_cmd - self.roll_state)
                                * v8math::min(1.0, profile.roll_rate_filter * dt);
                            if self.roll_state.abs() <= 1e-4 {
                                self.roll_state = 0.0;
                            }
                            if roll_cmd != 0.0 {
                                self.level_timer = 0.0;
                            }

                            // 14-second auto-level delay after manual roll: holds bank indefinitely
                            // until yaw input (> 0.3) starts the timer, and resets while actively
                            // steering (|yaw| > 0.1 or |pitch| > 0.1).
                            if sx.abs() > 0.3 && self.level_timer == 0.0 {
                                self.level_timer = 0.01;
                            }
                            if self.level_timer > 0.0 {
                                self.level_timer = v8math::min(15.0, self.level_timer + dt);
                                if self.level_timer < 14.0 && (sx.abs() > 0.1 || sy.abs() > 0.1) {
                                    self.level_timer = 0.01;
                                }
                            }
                            let auto_level = if self.level_timer >= 14.0 {
                                self.roll_state = 0.0;
                                let left = self.right();
                                -profile.level_filter * left[1]
                            } else {
                                0.0
                            };
                            self.roll_rate = self.roll_state * profile.roll_rate + auto_level;

                            // Effective axis rates blend from min(TurnRate, PitchRate) across 0.7..1.0.
                            let base_rate = v8math::min(profile.turn_rate, profile.pitch_rate);
                            let eff_turn = if sx.abs() >= 0.7 {
                                base_rate
                                    + (profile.turn_rate - base_rate)
                                        * v8math::min(1.0, (sx.abs() - 0.7) / 0.3)
                            } else {
                                base_rate
                            };
                            let eff_pitch = if sy.abs() >= 0.7 {
                                base_rate
                                    + (profile.pitch_rate - base_rate)
                                        * v8math::min(1.0, (sy.abs() - 0.7) / 0.3)
                            } else {
                                base_rate
                            };

                            // Boost reduces turn/pitch authority from 1.0 down to 0.7 (0.5 for a
                            // command flyer) across [MaxSpeed, BoostSpeed] and resets buildup timers.
                            let boost_scale = if self.airspeed > profile.max_speed
                                && profile.boost_speed > profile.max_speed
                            {
                                let bt = v8math::min(
                                    1.0,
                                    v8math::max(
                                        0.0,
                                        (self.airspeed - profile.max_speed)
                                            / (profile.boost_speed - profile.max_speed),
                                    ),
                                );
                                self.yaw_buildup = 0.0;
                                self.pitch_buildup = 0.0;
                                let floor = if profile.is_command_flyer { 0.5 } else { 0.7 };
                                1.0 - (1.0 - floor) * bt
                            } else {
                                if sx * sx + sy * sy >= 0.9 {
                                    self.yaw_buildup = v8math::min(1.0, self.yaw_buildup + dt);
                                    self.pitch_buildup = v8math::min(1.0, self.pitch_buildup + dt);
                                } else {
                                    self.yaw_buildup = 0.0;
                                    self.pitch_buildup = 0.0;
                                }
                                1.0
                            };
                            let buildup_mult = |timer: f64, mult: f64| {
                                if timer > 0.5 {
                                    1.0 + (mult - 1.0) * (timer - 0.5) * 2.0
                                } else {
                                    1.0
                                }
                            };
                            let ty = -sx
                                * eff_turn
                                * boost_scale
                                * buildup_mult(self.yaw_buildup, profile.turn_buildup_multiplier);
                            let tp = -self.fly_height_stick(sy, bounds, profile.pitch_rate * dt)
                                * eff_pitch
                                * boost_scale
                                * buildup_mult(self.pitch_buildup, profile.pitch_buildup_multiplier);

                            // Linear angular-rate slew with separate acceleration and deceleration filters.
                            let linear_slew = |cur: f64, target: f64, accel: f64, decel: f64| {
                                let accelerating =
                                    (0.0 <= cur && cur <= target) || (target <= cur && cur <= 0.0);
                                let step = (if accelerating { accel } else { decel }) * dt;
                                let diff = target - cur;
                                if diff.abs() <= step {
                                    target
                                } else if diff > 0.0 {
                                    cur + step
                                } else {
                                    cur - step
                                }
                            };
                            self.yaw_rate = linear_slew(
                                self.yaw_rate,
                                ty,
                                profile.turn_filter,
                                profile.turn_filter_decel,
                            );
                            self.pitch_rate = linear_slew(
                                self.pitch_rate,
                                tp,
                                profile.pitch_filter,
                                profile.pitch_filter_decel,
                            );

                            let (dp, dy, dr) = (self.pitch_rate * dt, self.yaw_rate * dt, self.roll_rate * dt);
                            let ang = v8math::hypot3(dp, dy, dr);
                            if ang > 1e-12 {
                                let dq = quat_from_axis_angle([dp / ang, dy / ang, dr / ang], ang);
                                let mut q = to_q(self.quaternion);
                                q.multiply(&dq).normalize();
                                self.quaternion = from_q(&q);
                            }
                        }
                    }

                    let fwd = self.forward();
                    let desired = [
                        fwd[0] * self.airspeed + strafe[0],
                        fwd[1] * self.airspeed + strafe[1],
                        fwd[2] * self.airspeed + strafe[2],
                    ];
                    let blend = v8math::min(1.0, v8math::max(0.0, profile.momentum_filter * dt));
                    let mut vel = [
                        self.velocity[0] + (desired[0] - self.velocity[0]) * blend,
                        self.velocity[1] + (desired[1] - self.velocity[1]) * blend,
                        self.velocity[2] + (desired[2] - self.velocity[2]) * blend,
                    ];
                    if !self.trick.active() && self.airspeed > 1e-6 {
                        let proj = vel[0] * fwd[0] + vel[1] * fwd[1] + vel[2] * fwd[2];
                        if proj > 1e-6 && (proj - self.airspeed).abs() > 1e-6 {
                            let scale = self.airspeed / proj;
                            vel[0] *= scale;
                            vel[1] *= scale;
                            vel[2] *= scale;
                        }
                    }
                    self.velocity = vel;

                    // The drawn hull leans into a turn by BankAngle times the forward speed
                    // times the yaw rate over the top speed, eased at BankFilter; the 2005 game
                    // neither clamps this lean nor scales it by TurnRate.
                    let top_speed = v8math::max(v8math::max(profile.max_speed, profile.boost_speed), 1e-6);
                    let forward_speed = vel[0] * fwd[0] + vel[1] * fwd[1] + vel[2] * fwd[2];
                    let target_bank = -profile.bank_angle * forward_speed * self.yaw_rate / top_speed;
                    self.bank += (target_bank - self.bank) * v8math::min(1.0, profile.bank_filter * dt);
                }

                // Multi-point wingtip + nose + dorsal + ventral hull sweep
                // against world collision (ignoring own body & hangar forcefields).
                let disp = [
                    self.velocity[0] * dt,
                    self.velocity[1] * dt,
                    self.velocity[2] * dt,
                ];
                if let Some((h, safe_d, body)) = sweep_hull_displacement(
                    self.position,
                    self.quaternion,
                    disp,
                    profile,
                    true,
                    ignore,
                    bounds,
                    collision,
                    collision_world,
                ) {
                    if self.apply_hull_contact(h, safe_d, body, disp, profile) {
                        notice.destroyed = true;
                    }
                } else {
                    self.position[0] += disp[0];
                    self.position[1] += disp[1];
                    self.position[2] += disp[2];
                }

                // Nothing turns a craft back at the boundary: a crewed craft
                // outside it is destroyed once OUT_OF_BOUNDS_TIME has run out,
                // and the time starts again whenever it is back inside.
                let bcheck = bounds.check_boundary(self.position);
                self.oob_warning = bcheck.warning;
                if bcheck.outside && (self.occupied() || self.oob_timer >= 0.0) {
                    if !(self.oob_timer >= 0.0) {
                        self.oob_timer = OUT_OF_BOUNDS_TIME;
                    }
                    self.oob_timer -= dt;
                    if self.oob_timer < 0.0 && self.apply_damage(self.max_health) {
                        notice.destroyed = true;
                    }
                } else {
                    self.oob_timer = -1.0;
                }

                // Weapon fire & lock-on requests while flying with pilot.
                if self.alive && self.pilot.is_some() {
                    notice.fire_primary = input.fire_primary;
                    notice.fire_secondary = input.fire_secondary;
                    notice.lock_request = input.lock_target;
                }
            }
        }

        // Recharge energy when not boosting or executing a trick.
        if self.alive && !self.boosting && !self.trick.active() {
            self.energy = v8math::min(profile.energy_bar, self.energy + profile.energy_auto_restore * dt);
        }

        // Abandonment expiry timer when unoccupied away from home pad.
        if self.alive && !self.occupied() {
            let at_home = (self.position[0] - pad.position[0]).abs() < PROVISIONAL_PAD_CLEAR_RADIUS
                && (self.position[2] - pad.position[2]).abs() < PROVISIONAL_PAD_CLEAR_RADIUS
                && self.state == SpaceFlightState::Landed;
            if !at_home {
                self.empty_timer += dt;
                let in_enemy_hangar = zone_idx.is_some_and(|idx| {
                    let zteam = bounds.landing_zones[idx].team;
                    zteam != 0 && zteam != self.team
                });
                let expire = if in_enemy_hangar {
                    pad.expire_time_enemy
                } else {
                    pad.expire_time_field
                };
                if expire > 0.0 && self.empty_timer > expire {
                    let decay_rate = if pad.decay_time > 0.0 {
                        self.max_health / pad.decay_time
                    } else {
                        self.max_health
                    };
                    if self.apply_damage(decay_rate * dt) {
                        notice.destroyed = true;
                    }
                }
            } else {
                self.empty_timer = 0.0;
            }
        } else {
            self.empty_timer = 0.0;
        }

        self.recompute_matrix();
        self.sync_collision_body(profile, collision, collision_world, flags);
        notice
    }

    /// Mixes complete authoritative fighter state into an FNV-1a digest.
    pub fn mix_digest(&self, hash: Wide) -> Wide {
        let mut m = Fnv::new(hash);
        m.small(self.generation);
        m.small(self.alive);
        m.small(self.state as u8);
        m.signed(self.team);
        m.bits(self.health);
        m.bits(self.energy);
        m.bits(self.airspeed);
        m.bits(self.gear);
        m.bits(self.prev_gear);
        m.bits(self.bank);
        m.bits(self.prev_bank);
        m.small(self.trick as u8);
        m.bits(self.trick_progress);
        m.bits(self.trick_cooldown);
        m.bits(self.oob_timer);
        m.bits(self.empty_timer);
        m.small(self.mobile_spawn_active);
        m.option_count(self.landed_zone);
        for value in [
            self.state_time,
            self.yaw_rate,
            self.pitch_rate,
            self.roll_rate,
            self.roll_state,
            self.level_timer,
            self.yaw_buildup,
            self.pitch_buildup,
            self.eject_timer,
        ] {
            m.bits(value);
        }
        for flag in [self.boosting, self.boost_exhausted, self.action_held, self.oob_warning] {
            m.small(flag);
        }
        for (crew, input) in std::iter::once((self.pilot, self.pending_input)).chain(self.gunners.into_iter().zip(self.gunner_inputs)) {
            m.small(crew.is_some());
            if let Some(p) = crew {
                m.small(p.owner.slot); m.small(p.owner.gen);
                m.signed(p.team); m.small(p.is_human); m.small(p.repair_capable);
            }
            for value in [input.throttle, input.stick_x, input.stick_y, input.roll] { m.bits(value); }
            for flag in [input.roll_lock, input.boost, input.takeoff_or_land, input.eject_held, input.fire_primary, input.fire_secondary, input.lock_target] {
                m.small(flag);
            }
            m.signed(i32::from(input.trick_roll)); m.signed(i32::from(input.trick_flip));
            m.option_small(input.weapon_index);
            m.small(input.aim.is_some());
            if let Some(aim) = input.aim { for value in aim { m.bits(value); } }
        }
        for value in self.previous_position.into_iter().chain(self.position).chain(self.velocity)
            .chain(self.previous_quaternion).chain(self.quaternion).chain(self.trick_axis_right) { m.bits(value); }
        for lock in std::iter::once(self.lock).chain(self.weapon_locks) {
            m.small(lock.weapon_index); m.small(lock.kind as u8);
            m.count(lock.target_index); m.le(lock.target_generation.to_le_bytes());
            m.bits(lock.progress); m.small(lock.locked); m.bits(lock.lost_los_time);
        }
        for w in &self.weapons {
            m.bits(w.clock.cooldown);
            m.small(w.clock.salvos);
            m.bits(w.clock.wait);
            m.bits(w.barrage_timer); m.small(w.trigger_was_held);
            m.small(w.ammo_in_clip);
            m.bits(w.reload_timer);
            m.bits(w.heat);
            m.small(w.overheated);
            m.bits(w.warm_up);
            m.small(w.shots_fired);
            m.small(w.barrage_remaining);
            m.bits(w.kick.value);
            m.small(w.kick.spun);
            m.bits(w.kick.cool_down);
        }
        m.finish()
    }
}

/// Checks whether a pad's footprint is clear of all live fighters.
pub fn pad_is_clear(pad: &SpaceVehiclePad, fighters: &[SpaceFighter]) -> bool {
    let r2 = PROVISIONAL_PAD_CLEAR_RADIUS * PROVISIONAL_PAD_CLEAR_RADIUS;
    !fighters.iter().any(|f| {
        if !f.alive {
            return false;
        }
        let dx = f.position[0] - pad.position[0];
        let dy = f.position[1] - pad.position[1];
        let dz = f.position[2] - pad.position[2];
        dx * dx + dy * dy + dz * dz < r2
    })
}

/// Steps all hangar vehicle spawn pads, respawning replacement fighters into
/// pre-allocated dormant slots when `spawn_time` elapses and active count is
/// below `spawn_count`. Allocates nothing.
#[allow(clippy::too_many_arguments)]
pub fn step_vehicle_pads(
    dt: f64,
    pads: &mut [SpaceVehiclePad],
    fighters: &mut [SpaceFighter],
    profiles: &[SpaceFlightProfile],
    bounds: &SpaceBounds,
    collision: &mut CollisionOwner,
    collision_world: usize,
    flags: &mut [u8],
) {
    for (pad_idx, pad) in pads.iter_mut().enumerate() {
        let active_for_pad = fighters
            .iter()
            .filter(|f| f.pad_index == pad_idx && f.alive)
            .count() as u32;
        if active_for_pad >= pad.spawn_count || !pad_is_clear(pad, fighters) {
            pad.respawn_timer = pad.spawn_time;
            continue;
        }
        pad.respawn_timer = v8math::max(0.0, pad.respawn_timer - dt);
        if pad.respawn_timer > 0.0 {
            continue;
        }
        // Reuse a pre-allocated dormant slot belonging to this pad.
        let Some(slot_idx) = fighters
            .iter()
            .position(|f| f.pad_index == pad_idx && !f.alive)
        else {
            continue;
        };
        let profile = &profiles[pad.profile_index];
        let zone_idx = bounds
            .landing_zones
            .iter()
            .position(|z| z.contains(pad.position));
        let floor_y = zone_idx
            .and_then(|idx| bounds.landing_zones[idx].floor_y)
            .unwrap_or(pad.floor_y);
        let next_gen = fighters[slot_idx].generation.wrapping_add(1);
        let kernel = fighters[slot_idx].kernel_body;
        fighters[slot_idx] = SpaceFighter::spawned_on_pad(
            slot_idx,
            next_gen,
            pad_idx,
            pad,
            pad.profile_index,
            profile,
            floor_y,
            zone_idx,
            kernel,
        );
        fighters[slot_idx].sync_collision_body(profile, collision, collision_world, flags);
        pad.respawn_timer = pad.spawn_time;
    }
}

pub(super) fn pilot_repairs(def: &Value) -> bool {
    prop_f64(def, "PilotSkillRepairScale", 0.0) > 0.0 || chassis_prop_all_strs(def, "UnitType").iter().any(|s| s.eq_ignore_ascii_case("pilot"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::bounds::OrientedRegion;
    use bfe_content::json::parse;

    fn synthetic_world() -> (CollisionOwner, usize, SpaceBounds, Vec<u8>) {
        let mut coll = CollisionOwner::new();
        let empty_bvh = build_bvh(Positions::F32(&[]), &[]);
        let empty_pos = coll.positions(&[]);
        let empty_tree = coll.tree(empty_pos, &[], &empty_bvh);
        let world_id = coll.world_new(empty_tree, -1, -1) as usize;
        let mut bounds = SpaceBounds {
            min: [-2000.0, -1600.0, -2000.0],
            max: [2000.0, 1600.0, 2000.0],
            center: [0.0, 0.0, 0.0],
            world_extents: 2000.0,
            boundary_polygon: vec![
                [-2000.0, -2000.0],
                [2000.0, -2000.0],
                [2000.0, 2000.0],
                [-2000.0, 2000.0],
            ],
            authored_regions: vec![],
            landing_zones: vec![OrientedRegion::from_matrix(
                "hangar_north".into(),
                1,
                Matrix4::default().elements,
                [80.0, 20.0, 80.0],
                0.0,
            )],
            forcefields: vec![],
        };
        bounds.add_landing_region(OrientedRegion::from_matrix(
            "hangar_south".into(),
            2,
            [
                1.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, 900.0, 1.0,
            ],
            [80.0, 20.0, 80.0],
            0.0,
        ));
        let flags = vec![0u8; 64];
        (coll, world_id, bounds, flags)
    }

    fn synthetic_fighter_profile() -> SpaceFlightProfile {
        SpaceFlightProfile {
            class_name: "synthetic_interceptor".into(),
            side: "blue".into(),
            is_command_flyer: false,
            max_health: 1600.0,
            min_speed: 30.0,
            mid_speed: 75.0,
            max_speed: 110.0,
            boost_speed: 165.0,
            acceleration: 45.0,
            boost_acceleration: 85.0,
            turn_rate: 1.2,
            pitch_rate: 1.2,
            roll_rate: 3.0,
            energy_bar: 90.0,
            energy_auto_restore: 10.0,
            energy_boost_drain: 20.0,
            energy_trick_single: 15.0,
            energy_trick_double: 30.0,
            trick_roll_speed: 6.0,
            trick_flip_speed: 8.0,
            trick_strafe_speed: 25.0,
            pilot_skill_repair_scale: 0.025,
            time_required_to_eject: 2.0,
            ..Default::default()
        }
    }

    #[test]
    fn a_blast_class_is_read_key_by_key_the_later_key_winning() {
        let read = |properties: &str| {
            let b = parse_explosion(&bfe_content::json::parse(&format!(r#"{{"class":"test_burst","base":"explosion","properties":[{properties}]}}"#)).unwrap());
            (b.damage, b.inner_radius, b.outer_radius, b.scales, b.shield_scale)
        };
        // `DamageRadius` is the outer radius alone, and a scale not given is 1.
        assert_eq!(read(r#"["Damage","43"],["DamageRadius","11.5"]"#), (43.0, 0.0, 11.5, [1.0; 5], 1.0));
        assert_eq!(
            read(r#"["Damage","43"],["DamageRadiusInner","2.5"],["DamageRadiusOuter","9.5"],["HealthScale","0.85"],["DroidScale","1.15"],["ArmorScale","0.35"],["BuildingScale","0.55"],["ShieldScale","0.65"]"#),
            (43.0, 2.5, 9.5, [0.85, 0.85, 1.15, 0.35, 0.55], 0.65)
        );
        // A group's scale after a scale of its own replaces it, as `MaxDamage`
        // after `Damage` and either radius after the other do.
        assert_eq!(
            read(r#"["Damage","43"],["VehicleScale","0.15"],["ArmorScale","0.75"],["DamageRadiusOuter","9.5"],["DamageRadius","7.5"],["MaxDamage","61"]"#),
            (61.0, 0.0, 7.5, [1.0, 1.0, 1.0, 0.75, 0.75], 1.0)
        );
    }

    #[test]
    fn prepared_weapon_counts_require_representable_whole_values() {
        let valid = parse(r#"{"properties":[["SalvoCount","3.0"],["BarrageMax","7"]]}"#).unwrap();
        assert!(validate_weapon_counts(&valid,"fixture").is_ok());
        assert!(validate_weapon_counts(&parse(r#"{"RoundsPerClip":-1}"#).unwrap(),"fixture").is_ok());
        for value in ["4.5","-1","NaN","9007199254740992","2oops"] {
            let raw = format!(r#"{{"properties":[["BarrageMax","{value}"]]}}"#);
            assert!(validate_weapon_counts(&parse(&raw).unwrap(),"fixture").is_err());
        }
        assert!(validate_weapon_counts(&parse(r#"{"SalvoCount":129}"#).unwrap(),"fixture").is_err());
    }

    #[test]
    fn pilot_visibility_and_cockpit_position_follow_authored_seat_properties() {
        let enclosed = parse(r#"{
            "base":"flyer",
            "vehicleSections":[
                {"kind":"chassis","name":"BODY","properties":[["PilotAnimation","seated_pose"],["IsPilotExposed","0"]]}
            ]
        }"#).unwrap();
        let p = parse_flight_profile("amber", "glider_alpha", &enclosed).unwrap();
        assert!(!p.pilot_exposed(0));

        let omitted_anim = parse(r#"{
            "base":"flyer",
            "vehicleSections":[
                {"kind":"chassis","name":"BODY","properties":[["IsPilotExposed","1"]]}
            ]
        }"#).unwrap();
        let p_no_anim = parse_flight_profile("amber", "glider_no_anim", &omitted_anim).unwrap();
        assert!(!p_no_anim.pilot_exposed(0));

        let mixed = parse(r#"{
            "base":"commandflyer",
            "vehicleSections":[
                {"kind":"chassis","name":"BODY","properties":[["PilotAnimation","seated_pose"],["IsPilotExposed","0"]]},
                {"kind":"seat","name":"TURRET1","properties":[["PilotAnimation","gun_pose"],["IsPilotExposed","1"]]},
                {"kind":"seat","name":"TURRET2","properties":[["PilotAnimation","gun_pose"],["IsPilotExposed","0"]]}
            ]
        }"#).unwrap();
        let p_mixed = parse_flight_profile("amber", "gunboat_beta", &mixed).unwrap();
        assert!(!p_mixed.pilot_exposed(0));
        assert!(p_mixed.pilot_exposed(1));
        assert!(!p_mixed.pilot_exposed(2));

        let mut p_seat = p;
        p_seat.seat_pilot_position[0] = [0.0, -0.25, -0.75];
        let pad = SpaceVehiclePad {
            name: "pad_cockpit".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_a".into(),
            position: [10.0, 2.0, -5.0],
            yaw: PI * 0.5,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let fighter = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &p_seat, 0.0, Some(0), None);
        let cockpit = fighter.seat_world_position(&p_seat, 0);
        assert!((cockpit[0] - (fighter.position[0] - 0.75)).abs() < 1e-6);
        assert!((cockpit[1] - (fighter.position[1] - 0.25)).abs() < 1e-6);
        assert!((cockpit[2] - fighter.position[2]).abs() < 1e-6);
    }

    #[test]
    fn gunship_chassis_flight_rates_ignore_turret_seat_overrides() {
        let raw = parse(r#"{
            "base":"commandflyer",
            "vehicleSections":[
                {"kind":"chassis","name":"BODY","properties":[["TurnRate","0.83"],["PitchRate","1.17"],["SpawnPointLocation","2 4 1 0"]]},
                {"kind":"seat","name":"lookout","properties":[["TurnRate","2.19"],["PitchRate","1.52"]]}
            ],
            "TurnRate":2.19,"PitchRate":1.52
        }"#).unwrap();
        let p = parse_flight_profile("violet", "shuttle", &raw).unwrap();
        assert_eq!((p.turn_rate, p.pitch_rate), (0.83,1.17));
        assert_eq!(p.spawn_point_offsets, [[2.0,4.0,1.0]]);
    }

    #[test]
    fn wingtip_sweep_detects_hangar_doorway_contact_during_flight_and_takeoff() {
        let (mut coll, world_id, bounds, mut flags) = synthetic_world();
        // Place a solid pillar at x in [5.5, 9.0], y in [-10, 20], z in [15, 25].
        // A wide-winged fighter (wingspan +/- 8 m) flying straight down z = 0
        // has a clear nose path at x = 0, but its right wingtip clips the pillar.
        let pillar_tree = install_bounds_box_tree(&mut coll, [5.5, -10.0, 15.0], [9.0, 20.0, 25.0]);
        let pillar_body = coll.body_new(pillar_tree, 0) as u32;
        let io = coll.io_mut();
        io[20..23].copy_from_slice(&[5.5, -10.0, 15.0]);
        io[23..26].copy_from_slice(&[9.0, 20.0, 25.0]);
        coll.body_set(pillar_body);
        coll.state.worlds[world_id].order.push(pillar_body);
        coll.state.worlds[world_id].version += 1;
        flags[pillar_body as usize] = 1;
        coll.enable_body(pillar_body, true);

        let mut profile = synthetic_fighter_profile();
        profile.contact_bounds_min = [-8.0, -2.0, -4.0];
        profile.contact_bounds_max = [8.0, 2.0, 4.0];
        profile.bounds_min = profile.contact_bounds_min;
        profile.bounds_max = profile.contact_bounds_max;
        let ftree = install_bounds_box_tree(&mut coll, profile.bounds_min, profile.bounds_max);
        profile.collision_tree = Some(ftree);
        let fbody = coll.body_new(ftree, 1) as u32;
        coll.state.worlds[world_id].order.push(fbody);
        coll.state.worlds[world_id].version += 1;

        let pad = SpaceVehiclePad {
            name: "pad_wide".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0, 0.0, 0.0],
            yaw: 0.0,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let mut fighter = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &profile, 0.0, Some(0), Some(fbody));
        fighter.pilot = Some(SpacePilotId { owner: crate::store::Ref::new(1, 1), team: 1, is_human: true, repair_capable: false });
        fighter.state = SpaceFlightState::Flying;
        fighter.position = [0.0, 5.0, 5.0];
        fighter.airspeed = 60.0;
        fighter.velocity = [0.0, 0.0, 60.0];
        fighter.pending_input = SpacePilotInput { throttle: 0.0, ..Default::default() };

        let hp_before = fighter.health;
        for _ in 0..20 {
            fighter.step_tick(1.0 / 60.0, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        assert!(
            fighter.health < hp_before || fighter.position[2] < 16.0,
            "wide wingtip must collide with doorway pillar (hp {} vs {}, z={})",
            fighter.health,
            hp_before,
            fighter.position[2]
        );
        // The contact names the body the wingtip struck.
        assert!(fighter.contacts > 0);
        assert_eq!(fighter.contact_body, pillar_body as i32);
    }

    #[test]
    fn a_craft_that_lifts_off_and_lands_again_at_once_stays_near_its_pad() {
        // Take-off key, landing key 1.25 s later with the throttle at rest: the
        // speed it integrates starts each tick from its own motion along the nose,
        // which the partial lift keeps small.
        let (mut coll, world_id, bounds, mut flags) = synthetic_world();
        let mut profile = SpaceFlightProfile {
            takeoff_height: 2.0,
            takeoff_speed: 10.0,
            takeoff_time: 1.0,
            landing_speed: 10.0,
            landing_time: 1.0,
            ..synthetic_fighter_profile()
        };
        let tree = install_bounds_box_tree(&mut coll, profile.bounds_min, profile.bounds_max);
        profile.collision_tree = Some(tree);
        let body = coll.body_new(tree, 1) as u32;
        coll.state.worlds[world_id].order.push(body);
        coll.state.worlds[world_id].version += 1;
        let pad = SpaceVehiclePad {
            name: "pad_hold".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0, 0.0, 0.0],
            yaw: 0.0,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let mut fighter = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &profile, 0.0, Some(0), Some(body));
        fighter.pilot = Some(SpacePilotId { owner: crate::store::Ref::new(1, 1), team: 1, is_human: true, repair_capable: false });
        let start = fighter.position;
        let mut lifted = false;
        for tick in 0..600 {
            let key = tick == 0 || tick == 75;
            fighter.pending_input = SpacePilotInput { takeoff_or_land: key, ..Default::default() };
            fighter.step_tick(1.0 / 60.0, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
            lifted |= fighter.gear < 0.9;
            if tick > 75 && fighter.state == SpaceFlightState::Landed {
                break;
            }
        }
        assert!(lifted);
        assert_eq!(fighter.state, SpaceFlightState::Landed);
        let moved = v8math::hypot(fighter.position[0] - start[0], fighter.position[2] - start[2]);
        assert!(moved < 6.0, "rests {moved} m from its pad");
    }

    #[test]
    fn a_landing_craft_rests_on_the_deck_under_it_not_on_the_regions_recorded_floor() {
        let (mut coll, world_id, bounds, mut flags) = synthetic_world();
        // The region records its floor at 0; the deck under this spot is half a metre lower.
        let deck_tree = install_bounds_box_tree(&mut coll, [-20.0, -3.0, -20.0], [20.0, -0.5, 20.0]);
        let deck = coll.body_new(deck_tree, 0) as u32;
        let io = coll.io_mut();
        io[20..23].copy_from_slice(&[-20.0, -3.0, -20.0]);
        io[23..26].copy_from_slice(&[20.0, -0.5, 20.0]);
        coll.body_set(deck);
        coll.state.worlds[world_id].order.push(deck);
        coll.state.worlds[world_id].version += 1;
        flags[deck as usize] = 1;
        coll.enable_body(deck, true);
        let mut profile = synthetic_fighter_profile();
        let tree = install_bounds_box_tree(&mut coll, profile.bounds_min, profile.bounds_max);
        profile.collision_tree = Some(tree);
        let body = coll.body_new(tree, 1) as u32;
        coll.state.worlds[world_id].order.push(body);
        coll.state.worlds[world_id].version += 1;
        let pad = SpaceVehiclePad {
            name: "pad_low".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0, 0.0, 0.0],
            yaw: 0.0,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let mut fighter = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &profile, 0.0, Some(0), Some(body));
        fighter.pilot = Some(SpacePilotId { owner: crate::store::Ref::new(1, 1), team: 1, is_human: true, repair_capable: false });
        fighter.state = SpaceFlightState::Landing;
        fighter.position = [0.0, 6.0, 0.0];
        fighter.gear = 0.5;
        let rest = -profile.contact_bounds_min[1] + PROVISIONAL_GEAR_CLEARANCE_Y;
        for _ in 0..600 {
            fighter.step_tick(1.0 / 60.0, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
            if fighter.state == SpaceFlightState::Landed {
                break;
            }
        }
        assert_eq!(fighter.state, SpaceFlightState::Landed);
        assert!((fighter.position[1] - (-0.5 + rest)).abs() < 1e-6, "rests at {}", fighter.position[1]);
    }

    #[test]
    fn fighter_takes_off_boosts_rolls_repairs_gates_landing_speed_and_lands() {
        let (mut coll, world_id, bounds, mut flags) = synthetic_world();
        let mut profile = SpaceFlightProfile {
            takeoff_height: 2.0,
            takeoff_speed: 10.0,
            takeoff_time: 1.0,
            landing_speed: 10.0,
            landing_time: 1.0,
            ..synthetic_fighter_profile()
        };
        let tree = install_bounds_box_tree(&mut coll, profile.bounds_min, profile.bounds_max);
        profile.collision_tree = Some(tree);
        let body = coll.body_new(tree, 1) as u32;
        coll.state.worlds[world_id].order.push(body);
        coll.state.worlds[world_id].version += 1;

        let pad = SpaceVehiclePad {
            name: "pad_alpha".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0, 0.0, 0.0],
            yaw: 0.0,
            matrix: Matrix4::default().elements,
            spawn_count: 2,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let mut fighter = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &profile, 0.0, Some(0), Some(body));
        fighter.pilot = Some(SpacePilotId { owner: crate::store::Ref::new(1, 1), team: 1, is_human: true, repair_capable: true });
        fighter.health = 1000.0;

        // Trigger takeoff and verify took_off notice.
        fighter.pending_input = SpacePilotInput { takeoff_or_land: true, ..Default::default() };
        let n0 = fighter.step_tick(1.0 / 60.0, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        assert!(n0.took_off);
        assert_eq!(fighter.state, SpaceFlightState::TakingOff);

        // The lift rises only once TakeoffHeight clear of the resting height;
        // the take-off is done once it is whole, TakeoffTime later.
        fighter.pending_input = SpacePilotInput { throttle: 0.5, ..Default::default() };
        let rest = fighter.position[1];
        let mut clear = None;
        for tick in 0u32..150 {
            fighter.step_tick(1.0 / 60.0, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
            if fighter.position[1] - rest <= profile.takeoff_height {
                assert_eq!(fighter.gear, 1.0, "lift before clearing the deck at tick {tick}");
            } else if clear.is_none() {
                clear = Some(tick);
            }
            if fighter.state == SpaceFlightState::Flying {
                let clear = clear.expect("cleared the deck");
                assert!((tick - clear).abs_diff(60) <= 2, "lift took {} ticks", tick - clear);
                break;
            }
        }
        assert_eq!(fighter.state, SpaceFlightState::Flying);
        assert!(fighter.health > 1030.0, "expected passive repair, got {}", fighter.health);

        // Boost past MaxSpeed toward BoostSpeed, draining EnergyBar.
        fighter.pending_input = SpacePilotInput { throttle: 1.0, boost: true, ..Default::default() };
        for _ in 0..90 {
            fighter.step_tick(1.0 / 60.0, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        assert!(fighter.airspeed > profile.max_speed + 20.0);
        assert!(fighter.energy < profile.energy_bar - 20.0);

        // Execute barrel roll trick and verify missile-evade flag + energy spend.
        let energy_before = fighter.energy;
        fighter.pending_input = SpacePilotInput { throttle: 0.0, trick_roll: 1, ..Default::default() };
        fighter.step_tick(1.0 / 60.0, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        assert!(fighter.evading_missiles());
        assert!(fighter.energy < energy_before - 10.0);

        // Finish barrel roll.
        fighter.pending_input = SpacePilotInput::default();
        for _ in 0..90 {
            fighter.step_tick(1.0 / 60.0, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        assert!(!fighter.trick.active());

        // Outside every landing region the key does nothing.
        fighter.position = [0.0, 8.0, 500.0];
        fighter.pending_input = SpacePilotInput { takeoff_or_land: true, ..Default::default() };
        assert_eq!(fighter.landing_at(&bounds), SpaceLandingPermission::OutsideRegion);
        fighter.step_tick(1.0 / 60.0, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        assert_eq!(fighter.state, SpaceFlightState::Flying);

        // Inside one, speed is no bar (the game has none): fast in the hangar is allowed.
        fighter.position = [0.0, 8.0, 900.0];
        fighter.airspeed = 80.0;
        assert!(matches!(fighter.landing_at(&bounds), SpaceLandingPermission::Allowed { .. }));

        // A registered region with an empty deck ray cannot authorize a press.
        let mut empty_deck = bounds.clone();
        empty_deck.sample_landing_zone_floor(1, &mut coll, world_id);
        fighter.airspeed = 40.0;
        fighter.action_held = false;
        assert_eq!(fighter.landing_at(&empty_deck), SpaceLandingPermission::NoFloor);
        fighter.step_tick(1.0 / 60.0, &profile, &pad, &empty_deck, &mut coll, world_id, &mut flags);
        assert_eq!(fighter.state, SpaceFlightState::Flying);

        // Land inside hangar_south (z = 900).
        fighter.pending_input = SpacePilotInput::default();
        fighter.step_tick(1.0 / 60.0, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        fighter.position = [0.0, 8.0, 900.0];
        fighter.airspeed = 40.0;
        fighter.pending_input = SpacePilotInput { takeoff_or_land: true, ..Default::default() };
        assert_eq!(fighter.landing_at(&bounds), SpaceLandingPermission::Allowed { zone: 1, floor_y: 0.0 });
        let mut landed = None;
        for _ in 0..300 {
            let n = fighter.step_tick(1.0 / 60.0, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
            fighter.pending_input.takeoff_or_land = false;
            if n.landed_zone.is_some() {
                landed = n.landed_zone;
                break;
            }
        }
        assert_eq!(fighter.state, SpaceFlightState::Landed);
        assert_eq!(landed, Some(1));
    }

    #[test]
    fn shots_meet_a_crafts_ordnance_bodies_and_in_flight_its_flyer_bodies_too() {
        let (mut coll, world_id, _, mut flags) = synthetic_world();
        let mut profile = SpaceFlightProfile { class_name: "test_craft".into(), ..synthetic_fighter_profile() };
        attach_vehicle_pack_to_profile(&mut profile, Some(&crate::space::tests::invented_hull_pack()), &mut coll);
        let body = coll.body_new(profile.collision_tree.unwrap(), 1) as u32;
        coll.state.worlds[world_id].order.push(body);
        coll.state.worlds[world_id].version += 1;
        let pad = SpaceVehiclePad {
            name: "pad_bodies".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0; 3],
            yaw: 0.0,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let mut craft = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &profile, 0.0, Some(0), Some(body));
        // Which of the five boxes a ray down on each meets.
        let mut struck = |craft: &SpaceFighter| -> Vec<&str> {
            craft.sync_collision_body(&profile, &mut coll, world_id, &mut flags);
            (0..5)
                .filter(|&i| {
                    let ray = Ray { ox: craft.position[0] + 10.0 * i as f64, oy: craft.position[1] + 9.0, oz: craft.position[2], dx: 0.0, dy: -1.0, dz: 0.0 };
                    coll.ray_hit(world_id as u32, ray, 18.0, -1).is_some_and(|(code, _)| code == body as i32)
                })
                .map(|i| ["hull", "shot", "wing", "foot", "bump"][i])
                .collect()
        };
        for (state, meets) in [
            (SpaceFlightState::Landed, vec!["hull", "shot"]),
            (SpaceFlightState::TakingOff, vec!["hull", "shot"]),
            (SpaceFlightState::Flying, vec!["hull", "shot", "wing"]),
            (SpaceFlightState::Landing, vec!["hull", "shot"]),
        ] {
            craft.state = state;
            assert_eq!(struck(&craft), meets, "{state:?}");
        }
    }

    /// A craft on a pad carrying one invented gun read from `json`, crewed by a
    /// player (`human`) or a bot, with a reticle 1 km along its nose.
    fn test_gunship(json: &str, human: bool) -> (SpaceFighter, SpaceFlightProfile, [f64; 3]) {
        let mut profile = synthetic_fighter_profile();
        profile.weapons = vec![parse_space_weapon(&bfe_content::json::parse(json).unwrap(), 0)];
        let pad = SpaceVehiclePad {
            name: "pad_test".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0, 0.0, 900.0],
            yaw: 0.4,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let mut ship = SpaceFighter::spawned_on_pad(3, 7, 0, &pad, 0, &profile, 0., Some(0), None);
        ship.pilot = Some(SpacePilotId { owner: crate::store::Ref::new(7, 3), team: 1, is_human: human, repair_capable: false });
        let fwd = ship.forward();
        let reticle = [ship.position[0] + fwd[0] * 1000.0, ship.position[1] + fwd[1] * 1000.0, ship.position[2] + fwd[2] * 1000.0];
        (ship, profile, reticle)
    }

    /// A crewed craft on a pad with one invented gun, run tick by tick as the
    /// battle runs it: the flight step, then the trigger.
    struct KickRig {
        ship: SpaceFighter,
        profile: SpaceFlightProfile,
        pad: SpaceVehiclePad,
        world: (CollisionOwner, usize, SpaceBounds, Vec<u8>),
        rng: crate::space::combat::SpaceRng,
    }

    impl KickRig {
        fn new(json: &str, human: bool) -> Box<Self> {
            let mut profile = synthetic_fighter_profile();
            profile.weapons = vec![parse_space_weapon(&bfe_content::json::parse(json).unwrap(), 0)];
            let pad = SpaceVehiclePad {
                name: "pad_kick".into(),
                team: 1,
                profile_index: 0,
                control_zone: "cp_north".into(),
                position: [0.0, 0.0, 900.0],
                yaw: 0.0,
                matrix: Matrix4::default().elements,
                spawn_count: 1,
                spawn_time: 5.0,
                expire_time_enemy: 15.0,
                expire_time_field: 30.0,
                decay_time: 8.0,
                respawn_timer: 5.0,
                floor_y: 0.0,
            };
            let mut ship = SpaceFighter::spawned_on_pad(3, 7, 0, &pad, 0, &profile, 0., Some(0), None);
            ship.pilot = Some(SpacePilotId { owner: crate::store::Ref::new(7, 3), team: 1, is_human: human, repair_capable: false });
            Box::new(KickRig { ship, profile, pad, world: synthetic_world(), rng: crate::space::combat::SpaceRng::new(Wide::from_u32(43)) })
        }

        /// `ticks` ticks with the trigger held or not; each round's direction,
        /// and the nose it left from.
        fn run(&mut self, ticks: u32, held: bool) -> Vec<([f64; 3], [f64; 3])> {
            let dt = 1.0 / 60.0;
            let mut shots = Vec::new();
            for _ in 0..ticks {
                self.ship.pending_input.fire_primary = held;
                let (coll, world, bounds, flags) = &mut self.world;
                self.ship.step_tick(dt, &self.profile, &self.pad, bounds, coll, *world, flags);
                let rt = &self.ship.weapons[0];
                if !held && rt.clock.salvos == 0 && !rt.trigger_was_held {
                    self.ship.weapons[0].clock.idle(dt);
                    continue;
                }
                let fwd = self.ship.forward();
                let reticle = [self.ship.position[0] + fwd[0] * 1000.0, self.ship.position[1] + fwd[1] * 1000.0, self.ship.position[2] + fwd[2] * 1000.0];
                let mut rounds = crate::space::combat::SpaceOrdnancePool::new(4);
                let (n, _, d) = crate::space::combat::try_fire_fighter_weapon(&mut self.ship, &self.profile, 0, dt, held, false, reticle, &[], &mut self.rng, &mut rounds);
                if n > 0 {
                    shots.push((d, fwd));
                }
            }
            shots
        }
    }

    #[test]
    fn a_weapons_rounds_burst_at_the_end_of_their_life_only_by_the_class_named_for_that() {
        let spec = |base: &str, names: &str| {
            let json = format!(r#"{{"class":"test_bomb","properties":[],"ordnance":{{"class":"test_shot","base":"{base}","properties":[{names}],
                "ExplosionName":{{"class":"test_burst","base":"explosion","properties":[["Damage","370"],["DamageRadius","16.35"]]}},
                "ExplosionExpire":{{"class":"test_flak","base":"explosion","properties":[["Damage","29"],["DamageRadiusInner","3.4"],["DamageRadiusOuter","21.5"],["ArmorScale","0.55"]]}}}}}}"#);
            parse_space_weapon(&bfe_content::json::parse(&json).unwrap(), 0)
        };
        let both = r#"["ExplosionName","test_burst"],["ExplosionExpire","test_flak"]"#;
        for base in ["bolt", "missile", "shell"] {
            let round = spec(base, both);
            assert_eq!((round.expire_blast.damage, round.expire_blast.outer_radius), (29.0, 21.5));
            assert_eq!(round.expire_blast.scales, [1.0, 1.0, 1.0, 0.55, 0.55]);
        }
        assert!(!spec("missile", r#"["ExplosionName","test_burst"]"#).expire_blast.hurts());
        assert_eq!(spec("beam", both).expire_blast, SpaceExplosionSpec::NONE);
    }

    #[test]
    fn a_barrage_paces_a_bots_trigger_and_never_a_players() {
        // An invented gun whose bots let go for 0.9 s after every fifth salvo;
        // 3 s of held trigger. A player fires as with no barrage at all.
        let gun = |barrage: &str| format!(r#"{{"class":"test_barrage_gun","properties":[["ShotDelay","0.11"]{barrage}]}}"#);
        let paced = gun(r#",["BarrageMin","5"],["BarrageMax","5"],["BarrageDelay","0.9"]"#);
        let salvos = |json: &str, human: bool| KickRig::new(json, human).run(180, true).len();
        assert_eq!(salvos(&paced, true), salvos(&gun(""), true));
        assert_eq!((salvos(&paced, true), salvos(&paced, false)), (28, 14));
    }

    /// The ticks, of `ticks` with the trigger held, in which `json`'s gun lets a salvo go.
    fn salvo_ticks(json: &str, ticks: usize) -> Vec<usize> {
        let mut rig = KickRig::new(json, true);
        (0..ticks).filter(|_| !rig.run(1, true).is_empty()).collect()
    }

    #[test]
    fn a_craft_guns_shot_delay_counts_from_the_tick_after_its_last_salvo() {
        // An invented gun: five salvos 0.057 s apart, the first 0.13 s into
        // the shot, and a shot delay of 0.31 s. The salvos go at 0.13 to
        // 0.358 s; the delay runs from the next tick, 22/60 s, and the held
        // trigger begins the next shot at 0.6767 s, with its own 0.13 s.
        let json = r#"{"class":"test_salvo_gun","properties":[["SalvoCount","5"],["SalvoDelay","0.057"],["InitialSalvoDelay","0.13"],["ShotDelay","0.31"]]}"#;
        assert_eq!(salvo_ticks(json, 95), [7, 11, 14, 18, 21, 48, 51, 55, 58, 62, 89, 92]);
    }

    #[test]
    fn a_salvo_delay_of_two_values_is_a_random_time_between_them() {
        // An invented gun whose salvos go 0.045 to 0.21 s apart, and the same
        // one with the first value alone; shots are 0.93 s apart.
        let gaps = |delay: &str| -> Vec<usize> {
            let json = format!(r#"{{"class":"test_salvo_gun","properties":[["SalvoCount","5"],["SalvoDelay","{delay}"],["ShotDelay","0.93"]]}}"#);
            let ticks = salvo_ticks(&json, 1800);
            ticks.windows(2).map(|t| t[1] - t[0]).filter(|gap| *gap < 30).collect()
        };
        let random = gaps("0.045 0.21");
        assert!(random.len() > 60 && random.iter().all(|gap| (2..=13).contains(gap)), "{random:?}");
        assert!(random.iter().any(|gap| *gap <= 5) && random.iter().any(|gap| *gap >= 10), "{random:?}");
        assert!(gaps("0.045").iter().all(|gap| (2..=3).contains(gap)));
        // The first value may be 0.
        let random = gaps("0 0.21");
        assert!(random.iter().all(|gap| *gap <= 13) && random.iter().any(|gap| *gap >= 10), "{random:?}");
    }

    #[test]
    fn a_gun_keeps_its_rate_whatever_the_tick() {
        // An invented gun's 0.047 s between shots are 2.82 ticks: 213 shots in
        // ten seconds, not one every third tick.
        let json = r#"{"class":"test_rate_gun","properties":[["ShotDelay","0.047"]]}"#;
        assert_eq!(salvo_ticks(json, 600).len(), 213);
    }

    #[test]
    fn a_bots_kicking_gun_spreads_wider_salvo_by_salvo_up_to_its_limit() {
        // An invented gun with no spread of its own whose salvos kick 0.6
        // degrees each, up to 2.1.
        let json = r#"{"class":"test_kick_gun","properties":[["SpreadPerShot","0.6"],["SpreadLimit","2.1"],["SpreadRecoverRate","0.9"],["SpreadThreshold","1.7"],["ShotDelay","0.11"]]}"#;
        let mut rig = KickRig::new(json, false);
        let shots = rig.run(150, true);
        assert!(shots.len() > 15, "{}", shots.len());
        let deg = PI / 180.0;
        let off = |(d, f): &([f64; 3], [f64; 3])| v8math::acos(v8math::min(1.0, d[0] * f[0] + d[1] * f[1] + d[2] * f[2]));
        // The first salvo flies along the nose; the next ones stray by the
        // kick gathered so far; none strays past the limit on both axes.
        assert_eq!(shots[0].0, shots[0].1);
        assert!(off(&shots[1]) > 0.0 && off(&shots[1]) <= 0.6 * deg * std::f64::consts::SQRT_2 + 1e-12);
        assert!(shots.iter().all(|s| off(s) <= 2.1 * deg * std::f64::consts::SQRT_2 + 1e-12));
        assert!(shots[5..].iter().any(|s| off(s) > 1.3 * deg), "the kick reaches its limit");
    }

    #[test]
    fn a_players_kick_rests_below_zero_so_the_first_salvos_fly_true() {
        // The same invented gun. After a rest a player's kick sits at minus
        // the threshold (1.7 degrees). Each salvo adds 0.6 and the 7 ticks to
        // the next take 0.105 away, so four salvos fly true and the fifth
        // strays; a bot's kick rests at zero, so its second salvo strays.
        let json = r#"{"class":"test_kick_gun","properties":[["SpreadPerShot","0.6"],["SpreadLimit","2.1"],["SpreadRecoverRate","0.9"],["SpreadThreshold","1.7"],["ShotDelay","0.11"]]}"#;
        let mut player = KickRig::new(json, true);
        assert!(player.run(180, false).is_empty());
        let shots = player.run(40, true);
        assert!(shots.len() >= 5);
        assert!(shots[..4].iter().all(|(d, f)| d == f));
        assert_ne!(shots[4].0, shots[4].1);
        let mut bot = KickRig::new(json, false);
        bot.run(180, false);
        let shots = bot.run(30, true);
        assert_eq!(shots[0].0, shots[0].1);
        assert_ne!(shots[1].0, shots[1].1);
    }

    #[test]
    fn a_kick_holds_through_the_cool_down_and_only_then_falls() {
        // An invented gun that cools for 1.1 s after it stops: the kick gathered
        // in a burst (2.1 degrees) has not fallen away 3.0 s after the trigger
        // is let go, though 2.3 s of recovery would have taken it, and is gone
        // by 3.8 s.
        let json = r#"{"class":"test_cool_gun","properties":[["SpreadPerShot","0.6"],["SpreadLimit","2.1"],["SpreadRecoverRate","0.9"],["ShotDelay","0.11"],["CoolDownTime","1.1"]]}"#;
        for (rest, true_after) in [(180u32, false), (228, true)] {
            let mut rig = KickRig::new(json, false);
            assert!(rig.run(40, true).len() >= 5);
            rig.run(rest, false);
            let next = rig.run(1, true);
            assert_eq!(next.len(), 1);
            assert_eq!(next[0].0 == next[0].1, true_after, "after {rest} ticks at rest");
        }
    }

    #[test]
    fn a_kick_falls_through_an_overheat_or_a_reload_with_the_trigger_held() {
        // Invented guns that stop by themselves, one by overheating after five
        // shots, one by emptying a clip of three, and cool for 1.3 s after they
        // stop. Until they can fire again it makes no difference whether the
        // trigger is held or let go: the kick holds for the cool-down and then
        // falls 0.9 degrees a second.
        let kick = r#"["SpreadPerShot","0.6"],["SpreadLimit","2.1"],["SpreadRecoverRate","0.9"],["ShotDelay","0.11"],["CoolDownTime","1.3"]"#;
        let hot = format!(r#"{{"class":"test_hot_gun","properties":[{kick},["HeatPerShot","0.23"],["HeatRecoverRate","0.17"],["HeatThreshold","0.29"]]}}"#);
        let clip = format!(r#"{{"class":"test_clip_gun","properties":[{kick},["RoundsPerClip","3"],["ReloadTime","4.3"]]}}"#);
        let stopped = |rig: &KickRig| rig.ship.weapons[0].overheated || rig.ship.weapons[0].reload_timer > 0.0;
        for json in [hot, clip] {
            let (mut held, mut let_go) = (KickRig::new(&json, false), KickRig::new(&json, false));
            while !stopped(&held) {
                held.run(1, true);
                let_go.run(1, true);
            }
            let start = held.ship.weapons[0].kick.value;
            for tick in 0..180 {
                held.run(1, true);
                let_go.run(1, false);
                assert_eq!(held.ship.weapons[0].kick.value, let_go.ship.weapons[0].kick.value, "tick {tick}, {json}");
            }
            // Three seconds on, still stopped: the cool-down took 1.3 s of them
            // and the fall has taken at least a degree of the rest.
            assert!(stopped(&held), "{json}");
            assert!(held.ship.weapons[0].kick.value < start - PI / 180.0, "{start} {json}");
        }
    }

    #[test]
    fn a_salvo_cut_short_by_an_empty_clip_still_kicks() {
        use crate::space::combat::{try_fire_fighter_weapon, SpaceOrdnancePool, SpaceRng};
        // An invented gun whose shot is five salvos of two rounds, all at once,
        // and whose clip holds fewer. A clip of four ends exactly after the
        // second salvo: the third finds it empty, releases nothing and kicks all
        // the same. A clip of five leaves the third salvo one round. Three
        // salvos kick either way. The game does this: it tests the clip before
        // each round and adds the kick after the salvo, whether or not the empty
        // clip let a round of it go. No stock craft or ship weapon shows it, as
        // none has both a clip and a kick.
        let deg = PI / 180.0;
        for (clip, rounds) in [(4, 4), (5, 5)] {
            let json = format!(r#"{{"class":"test_clip_gun","properties":[["SalvoCount","5"],["ShotsPerSalvo","2"],["SalvoDelay","0"],["RoundsPerClip","{clip}"],["ReloadTime","4.3"],["SpreadPerShot","0.5"],["SpreadLimit","7.3"]]}}"#);
            let (mut ship, profile, reticle) = test_gunship(&json, false);
            let mut rng = SpaceRng::new(Wide::from_u32(5));
            let mut pool = SpaceOrdnancePool::new(16);
            let (n, _, _) = try_fire_fighter_weapon(&mut ship, &profile, 0, 1.0 / 60.0, true, false, reticle, &[], &mut rng, &mut pool);
            assert_eq!(n, rounds, "clip {clip}");
            assert!((ship.weapons[0].kick.value - 3.0 * 0.5 * deg).abs() < 1e-12, "clip {clip}: {}", ship.weapons[0].kick.value / deg);
        }
    }

    #[test]
    fn a_clip_emptied_mid_shot_waits_for_the_shot_to_end_and_never_restarts_reload() {
        // An invented gun whose shot has three salvos 0.5 s apart, clip of one,
        // reload of 2.0 s. Salvo 1 empties the clip on tick 1; the reload waits
        // until salvo 2 cuts the shot at 0.5 s, then counts down once without
        // restarting.
        let json = r#"{"class":"test_midshot_clip","properties":[["SalvoCount","3"],["ShotsPerSalvo","1"],["SalvoDelay","0.5"],["RoundsPerClip","1"],["ReloadTime","2.0"],["ShotDelay","0.2"]]}"#;
        let mut rig = KickRig::new(json, true);
        let first = rig.run(1, true);
        assert_eq!(first.len(), 1);
        assert_eq!(rig.ship.weapons[0].ammo_in_clip, 0);
        assert_eq!(rig.ship.weapons[0].reload_timer, 0.0, "reload does not start mid-shot");
        for tick in 2..=30 {
            assert!(rig.run(1, false).is_empty(), "tick {tick}");
            assert_eq!(rig.ship.weapons[0].reload_timer, 0.0, "tick {tick}");
        }
        // Tick 31: salvo 2 finds the clip empty, cuts the shot, and starts one reload.
        assert!(rig.run(1, false).is_empty());
        assert_eq!(rig.ship.weapons[0].clock.salvos, 0);
        let started = rig.ship.weapons[0].reload_timer;
        assert!((started - 2.0).abs() < 1e-9, "started={started}");
        let mut prev = started;
        for tick in 32..=152 {
            assert!(rig.run(1, false).is_empty(), "tick {tick}");
            let cur = rig.ship.weapons[0].reload_timer;
            assert!(cur <= prev, "reload timer jumped up on tick {tick}: {prev} -> {cur}");
            prev = cur;
        }
        assert_eq!(rig.ship.weapons[0].reload_timer, 0.0);
        assert_eq!(rig.ship.weapons[0].ammo_in_clip, 1);

        // Even when ReloadTime is shorter than SalvoDelay, the clip does not
        // refill mid-shot before salvo 2 cuts the shot.
        let short = r#"{"class":"test_short_reload","properties":[["SalvoCount","3"],["ShotsPerSalvo","1"],["SalvoDelay","0.5"],["RoundsPerClip","1"],["ReloadTime","0.25"],["ShotDelay","0.2"]]}"#;
        let mut rig = KickRig::new(short, true);
        assert_eq!(rig.run(1, true).len(), 1);
        assert!(rig.run(35, false).is_empty(), "must not reload and fire salvo 2 mid-shot");
        assert_eq!(rig.ship.weapons[0].shots_fired, 1);
        // After the 0.25 s reload finishes following the cut salvo, clip refills.
        assert!(rig.run(20, false).is_empty());
        assert_eq!(rig.ship.weapons[0].ammo_in_clip, 1);
    }

    #[test]
    fn a_seats_spread_scale_multiplies_the_kick_each_salvo_adds() {
        // The same invented gun, never falling back, fired by two bots, one of
        // whose seats hands its gun a scale of 2.5: every round after the first
        // strays exactly 2.5 times as far on each axis.
        let json = r#"{"class":"test_kick_gun","properties":[["SpreadPerShot","0.2"],["SpreadLimit","30"],["ShotDelay","0.11"]]}"#;
        let mut plain = KickRig::new(json, false);
        let mut scaled = KickRig::new(json, false);
        scaled.ship.pending_input.spread_scale = Some(2.5);
        let (a, b) = (plain.run(60, true), scaled.run(60, true));
        assert_eq!(a.len(), b.len());
        assert!(a.len() > 5);
        let fwd = plain.ship.forward();
        let up = rotate_vec(plain.ship.quaternion, [0.0, 1.0, 0.0]);
        let right = [up[1] * fwd[2] - up[2] * fwd[1], up[2] * fwd[0] - up[0] * fwd[2], up[0] * fwd[1] - up[1] * fwd[0]];
        let dot = |p: [f64; 3], q: [f64; 3]| p[0] * q[0] + p[1] * q[1] + p[2] * q[2];
        let axes = |d: [f64; 3]| (v8math::asin(dot(d, up)), v8math::atan2(dot(d, right), dot(d, fwd)));
        for ((da, _), (db, _)) in a.iter().zip(&b).skip(1) {
            let ((pa, ya), (pb, yb)) = (axes(*da), axes(*db));
            assert!((pb - 2.5 * pa).abs() < 1e-9 && (yb - 2.5 * ya).abs() < 1e-9, "{pa} {pb} {ya} {yb}");
        }
        // A player's gun takes no scale from its seat.
        let mut player = KickRig::new(json, true);
        player.ship.pending_input.spread_scale = Some(2.5);
        assert_eq!(player.run(60, true), a);
    }

    #[test]
    fn a_guns_kick_reads_degrees_its_older_names_and_its_cool_down() {
        let spec = |props: &str| parse_space_weapon(&bfe_content::json::parse(&format!(r#"{{"class":"test_gun","properties":{props}}}"#)).unwrap(), 0);
        let deg = PI / 180.0;
        let a = spec(r#"[["KickSpread","0.3"],["SpreadRecoverRate","1.1"],["SpreadThreshold","0.95"],["SpreadLimit","1.6"],["CoolDownTime","0.7"]]"#);
        assert_eq!([a.spread_per_shot, a.spread_recover_rate, a.spread_threshold, a.spread_limit, a.cool_down_time], [0.3 * deg, 1.1 * deg, 0.95 * deg, 1.6 * deg, 0.7]);
        // The older SpreadRecover: recovery = its value times the salvo's kick,
        // limit = that over the shot delay read so far.
        let b = spec(r#"[["SpreadPerShot","0.4"],["ShotDelay","0.23"],["SpreadRecover","3.0"]]"#);
        assert_eq!(b.spread_recover_rate, 0.4 * deg * 3.0);
        assert_eq!(b.spread_limit, 0.4 * deg * 3.0 / 0.23);
        // No SpreadLimit: no kick at all.
        assert_eq!(spec(r#"[["SpreadPerShot","0.4"]]"#).spread_limit, 0.0);
    }

    #[test]
    fn a_craft_weapons_rounds_burst_by_the_blast_class_their_ordnance_names() {
        // Invented weapons whose prepared record holds the named blast's. The
        // keys are read in order, the later winning.
        let spec = |base: &str, names: &str, blasts: &str| {
            let json = format!(r#"{{"class":"test_bomb","properties":[],"ordnance":{{"class":"test_shot","base":"{base}","properties":[["MaxDamage","61"],["VehicleScale","1.65"],{names}],{blasts}}}}}"#);
            parse_space_weapon(&bfe_content::json::parse(&json).unwrap(), 0)
        };
        let burst = |name: &str, properties: &str| format!(r#""{name}":{{"class":"test_burst","base":"explosion","properties":[{properties}]}}"#);
        let full = burst("ExplosionName", r#"["Damage","370"],["DamageRadiusInner","3.4"],["DamageRadiusOuter","16.35"],["VehicleScale","0.35"],["HealthScale","0.85"],["DroidScale","1.15"],["BuildingScale","0.55"],["ShieldScale","0.65"]"#);
        let named = r#"["ExplosionName","test_burst"]"#;
        let blast = spec("missile", named, &full).blast;
        assert_eq!((blast.damage, blast.inner_radius, blast.outer_radius), (370.0, 3.4, 16.35));
        assert_eq!((blast.scales, blast.shield_scale), ([0.85, 0.85, 1.15, 0.35, 0.55], 0.65));
        // `DamageRadius` is the outer radius alone, a missing scale is 1 (not
        // the round's own), and `ArmorScale` after `VehicleScale` replaces it.
        let blast = spec("shell", named, &burst("ExplosionName", r#"["Damage","43"],["DamageRadius","11.5"],["VehicleScale","0.35"],["ArmorScale","0.75"]"#)).blast;
        assert_eq!((blast.damage, blast.inner_radius, blast.outer_radius, blast.scales, blast.shield_scale), (43.0, 0.0, 11.5, [1.0, 1.0, 1.0, 0.75, 0.75], 1.0));
        // The last of the keys that name an impact blast counts.
        let two = format!("{full},{}", burst("ExplosionImpact", r#"["Damage","29"],["DamageRadius","4.5"]"#));
        assert_eq!(spec("bolt", r#"["ExplosionName","test_burst"],["ExplosionImpact","test_burst"]"#, &two).blast.damage, 29.0);
        assert_eq!(spec("bolt", r#"["ExplosionImpact","test_burst"],["ExplosionName","test_burst"]"#, &two).blast.damage, 370.0);
        // A beam strikes without one, and a round naming none has none.
        assert_eq!(spec("beam", named, &full).blast, SpaceExplosionSpec::NONE);
        assert!(!spec("bolt", r#"["LifeSpan","1.3"]"#, &full).blast.hurts());
    }

    #[test]
    fn blast_keys_ignore_case_and_an_inner_radius_alone_hurts() {
        let lower = bfe_content::json::parse(
            r#"{"class":"test_lower","base":"explosion","properties":[
                ["damage","75"],["damageradiusinner","6.5"],["damageradiusouter","14.0"],
                ["healthscale","0.6"],["armorscale","0.4"],["droidscale","1.2"],["buildingscale","0.3"],["shieldscale","0.8"]
            ]}"#,
        ).unwrap();
        let b = parse_explosion(&lower);
        assert_eq!((b.damage, b.inner_radius, b.outer_radius), (75.0, 6.5, 14.0));
        assert_eq!((b.scales, b.shield_scale), ([0.6, 0.6, 1.2, 0.4, 0.3], 0.8));

        // Inner radius alone hurts within that radius and nowhere beyond it.
        let inner_only = bfe_content::json::parse(
            r#"{"class":"test_inner","base":"explosion","properties":[["maxdamage","95"],["damageradiusinner","8.0"]]}"#,
        ).unwrap();
        let ib = parse_explosion(&inner_only);
        assert!(ib.hurts(), "a blast with only an inner radius must hurt");
        assert_eq!(ib.share([0.0; 3], [5.0, 0.0, 0.0], 0.0), 1.0);
        assert_eq!(ib.share([0.0; 3], [9.0, 0.0, 0.0], 0.0), 0.0);

        // Case-insensitive blast lookup on ordnance, both matching and canonical nested keys.
        let ord_same = bfe_content::json::parse(
            r#"{"class":"test_shot","base":"bolt","properties":[["explosionname","test_inner"]],
                "explosionname":{"class":"test_inner","base":"explosion","properties":[["damage","44"],["damageradius","9.0"]]}}"#,
        ).unwrap();
        assert_eq!(named_blast(&ord_same, &IMPACT_BLASTS).damage, 44.0);
        let ord_canon = bfe_content::json::parse(
            r#"{"class":"test_shot","base":"bolt","properties":[["explosionimpact","test_inner"]],
                "ExplosionImpact":{"class":"test_inner","base":"explosion","properties":[["damage","55"],["damageradiusinner","7.0"]]}}"#,
        ).unwrap();
        let cb = named_blast(&ord_canon, &IMPACT_BLASTS);
        assert_eq!((cb.damage, cb.inner_radius, cb.hurts()), (55.0, 7.0, true));
        let beam_upper = bfe_content::json::parse(
            r#"{"class":"test_beam","base":"BEAM","properties":[["explosionname","test_inner"]],
                "ExplosionName":{"class":"test_inner","base":"explosion","properties":[["damage","55"],["damageradius","7.0"]]}}"#,
        ).unwrap();
        assert_eq!(named_blast(&beam_upper, &IMPACT_BLASTS), SpaceExplosionSpec::NONE);
    }

    #[test]
    fn a_craft_is_as_big_to_a_blast_as_its_models_sphere() {
        let profile = |extra: &str| {
            let json = format!(r#"{{"base":"flyer","bounds":[[-3,-1,-2],[3,5,7]]{extra}}}"#);
            parse_flight_profile("side", "test_craft", &bfe_content::json::parse(&json).unwrap()).unwrap()
        };
        assert_eq!(profile(r#","sphere":[0.5,2,2.5,4.25]"#).sphere, [0.5, 2.0, 2.5, 4.25]);
        // Content prepared without it: the sphere around the model's box.
        assert_eq!(profile("").sphere, [0.0, 2.0, 2.5, 0.5 * 153.0f64.sqrt()]);
    }

    #[test]
    fn a_craft_guns_pitch_and_yaw_spreads_are_degrees_and_the_later_key_wins() {
        let spread = |props: &str| {
            let w = bfe_content::json::parse(&format!(r#"{{"class":"test_gun","properties":{props}}}"#)).unwrap();
            let spec = parse_space_weapon(&w, 0);
            (spec.pitch_spread, spec.yaw_spread)
        };
        let deg = std::f64::consts::PI / 180.0;
        assert_eq!(spread(r#"[["PitchSpread","0.7"],["YawSpread","1.9"]]"#), (0.7 * deg, 1.9 * deg));
        // MaxSpread writes both axes; a later PitchSpread replaces the pitch only.
        assert_eq!(spread(r#"[["MaxSpread","0.42"],["PitchSpread","0.8"]]"#), (0.8 * deg, 0.05 * 0.42));
        assert_eq!(spread(r#"[["PitchSpread","0.8"],["MaxSpread","0.42"]]"#), (0.025 * 0.42, 0.05 * 0.42));
    }

    #[test]
    fn a_craft_gun_with_a_wide_yaw_spread_fans_its_shots_sideways() {
        use crate::space::combat::{try_fire_fighter_weapon, SpaceOrdnancePool, SpaceRng};
        // An invented gun spreading 2.3 degrees to the side and 0.43 up or down.
        let json = r#"{"class":"test_fan_gun","properties":[["YawSpread","2.3"],["PitchSpread","0.43"]]}"#;
        let (mut ship, profile, reticle) = test_gunship(json, false);
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let fwd = ship.forward();
        let up = rotate_vec(ship.quaternion, [0.0, 1.0, 0.0]);
        let right = [up[1] * fwd[2] - up[2] * fwd[1], up[2] * fwd[0] - up[0] * fwd[2], up[0] * fwd[1] - up[1] * fwd[0]];
        let deg = std::f64::consts::PI / 180.0;
        let mut rng = SpaceRng::new(Wide::from_u32(17));
        let (mut widest_pitch, mut widest_yaw) = (0.0f64, 0.0f64);
        for _ in 0..300 {
            ship.weapons[0].clock = Default::default();
            let mut rounds = SpaceOrdnancePool::new(2);
            let (n, _, d) = try_fire_fighter_weapon(&mut ship, &profile, 0, 1.0 / 60.0, true, false, reticle, &[], &mut rng, &mut rounds);
            assert_eq!(n, 1);
            let pitch = v8math::asin(dot(d, up)).abs();
            let yaw = v8math::atan2(dot(d, right), dot(d, fwd)).abs();
            assert!(pitch <= 0.43 * deg + 1e-12 && yaw <= 2.3 * deg + 1e-12, "pitch {pitch} yaw {yaw}");
            widest_pitch = v8math::max(widest_pitch, pitch);
            widest_yaw = v8math::max(widest_yaw, yaw);
        }
        assert!(widest_yaw > 0.7 * 2.3 * deg && widest_pitch > 0.7 * 0.43 * deg, "{widest_pitch} {widest_yaw}");
    }

    #[test]
    fn a_tracer_goes_out_with_a_craft_guns_first_round_and_every_period_after() {
        use crate::space::combat::{try_fire_fighter_weapon, SpaceOrdnancePool, SpaceRng};
        // An invented gun of one salvo of fourteen rounds, a tracer to every four.
        let json = r#"{"class":"test_tracer_gun","properties":[["SalvoCount","1"],["ShotsPerSalvo","14"],["SecondaryOrdnancePeriod","4"]]}"#;
        let (mut ship, profile, reticle) = test_gunship(json, true);
        let mut rng = SpaceRng::new(Wide::from_u32(5));
        let mut rounds = SpaceOrdnancePool::new(16);
        try_fire_fighter_weapon(&mut ship, &profile, 0, 1.0 / 60.0, true, false, reticle, &[], &mut rng, &mut rounds);
        let tracers: Vec<usize> = (0..14).filter(|&i| rounds.slots[i].is_tracer).collect();
        assert_eq!((rounds.active_count(), tracers), (14, vec![0, 4, 8, 12]));
    }

    #[test]
    fn a_craft_gun_fires_every_round_of_each_salvo() {
        use crate::space::combat::{try_fire_fighter_weapon, SpaceOrdnancePool, SpaceRng};
        // An invented gun: two salvos a shot, three rounds each, none between
        // them, and a clip of seven.
        let json = r#"{"class":"test_salvo_gun","properties":[["SalvoCount","2"],["ShotsPerSalvo","3"],["SalvoDelay","0"],["RoundsPerClip","7"],["ReloadTime","1.9"]]}"#;
        let (mut ship, profile, reticle) = test_gunship(json, true);
        let mut rng = SpaceRng::new(Wide::from_u32(5));
        let mut rounds = SpaceOrdnancePool::new(16);
        let (n, _, _) = try_fire_fighter_weapon(&mut ship, &profile, 0, 1.0 / 60.0, true, false, reticle, &[], &mut rng, &mut rounds);
        assert_eq!(n, 6, "two salvos of three rounds");
        assert_eq!(ship.weapons[0].ammo_in_clip, 1);
        // The next shot fires the clip's last round and the reload begins.
        ship.weapons[0].clock = Default::default();
        let (n, _, _) = try_fire_fighter_weapon(&mut ship, &profile, 0, 1.0 / 60.0, true, false, reticle, &[], &mut rng, &mut rounds);
        assert_eq!(n, 1);
        assert!(ship.weapons[0].reload_timer > 0.0);
    }

    #[test]
    fn a_craft_gun_spreads_its_shots_by_max_spread_on_each_axis_whatever_its_heat() {
        use crate::space::combat::{try_fire_fighter_weapon, SpaceOrdnancePool, SpaceRng};
        // An invented gun. The 2005 game draws each shot within 0.025 of
        // MaxSpread in pitch and 0.05 of it in yaw, in the craft's frame,
        // reads no MinSpread, and spreads a hot gun no wider than a cold one.
        let json = r#"{"class":"test_spread_gun","properties":[["MinSpread","0.13"],["MaxSpread","0.37"]]}"#;
        let mut profile = synthetic_fighter_profile();
        profile.weapons = vec![parse_space_weapon(&bfe_content::json::parse(json).unwrap(), 0)];
        let pad = SpaceVehiclePad {
            name: "pad_spread".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0, 0.0, 900.0],
            yaw: 0.6,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let mut ship = SpaceFighter::spawned_on_pad(3, 7, 0, &pad, 0, &profile, 0., Some(0), None);
        ship.pilot = Some(SpacePilotId { owner: crate::store::Ref::new(7, 3), team: 1, is_human: true, repair_capable: false });
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let fwd = ship.forward();
        let up = rotate_vec(ship.quaternion, [0.0, 1.0, 0.0]);
        let right = [up[1] * fwd[2] - up[2] * fwd[1], up[2] * fwd[0] - up[0] * fwd[2], up[0] * fwd[1] - up[1] * fwd[0]];
        let reticle = [ship.position[0] + fwd[0] * 1000.0, ship.position[1] + fwd[1] * 1000.0, ship.position[2] + fwd[2] * 1000.0];
        let (pitch_max, yaw_max) = (0.025 * 0.37, 0.05 * 0.37);
        let mut rng = SpaceRng::new(Wide::from_u32(91));
        for heat in [0.0, 0.95] {
            let (mut widest_pitch, mut widest_yaw) = (0.0f64, 0.0f64);
            for _ in 0..300 {
                let rt = &mut ship.weapons[0];
                (rt.clock, rt.heat, rt.overheated) = (Default::default(), heat, false);
                let mut rounds = SpaceOrdnancePool::new(2);
                let (n, _, d) = try_fire_fighter_weapon(&mut ship, &profile, 0, 1.0 / 60.0, true, false, reticle, &[], &mut rng, &mut rounds);
                assert_eq!(n, 1);
                let pitch = v8math::asin(dot(d, up)).abs();
                let yaw = v8math::atan2(dot(d, right), dot(d, fwd)).abs();
                assert!(pitch <= pitch_max + 1e-12 && yaw <= yaw_max + 1e-12, "heat {heat}: pitch {pitch} yaw {yaw}");
                widest_pitch = v8math::max(widest_pitch, pitch);
                widest_yaw = v8math::max(widest_yaw, yaw);
            }
            assert!(widest_pitch > 0.7 * pitch_max && widest_yaw > 0.7 * yaw_max, "heat {heat}: {widest_pitch} {widest_yaw}");
        }
    }

    #[test]
    fn gunner_selection_sight_weapon_lock_and_credit_are_independent() {
        use crate::space::combat::{try_fire_fighter_weapon,SpaceOrdnancePool,SpaceRng};
        let mut transport=synthetic_fighter_profile();
        transport.seat_count=2;
        transport.weapons=vec![SpaceWeaponSpec::default(),
            SpaceWeaponSpec{seat:1,channel:1,..Default::default()},
            SpaceWeaponSpec{seat:1,channel:1,ordnance_kind:SpaceOrdnanceKind::Missile,..Default::default()}];
        let pad = SpaceVehiclePad {
            name: "pad_trans".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0, 0.0, 900.0],
            yaw: 0.0,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let mut ship=SpaceFighter::spawned_on_pad(3,7,0,&pad,0,&transport,0.,Some(0),None);
        let crew=|id|SpacePilotId{owner:crate::store::Ref::new(id,3),team:1,is_human:true,repair_capable:false};
        ship.pilot=Some(crew(7));
        ship.pending_input=SpacePilotInput{fire_secondary:true,..Default::default()};
        assert_eq!(ship.weapon_trigger(&transport.weapons,2),(false,false));
        ship.gunners[0]=Some(crew(11));
        ship.gunner_inputs[0]=SpacePilotInput{weapon_index:Some(2),fire_primary:true,aim:Some([1.,0.,0.]),..Default::default()};
        assert_eq!(ship.weapon_trigger(&transport.weapons,0),(true,false));
        assert_eq!(ship.weapon_trigger(&transport.weapons,1),(false,false));
        assert_eq!(ship.weapon_trigger(&transport.weapons,2),(true,true));
        ship.weapon_locks[2]=SpaceLockState{kind:SpaceLockTargetKind::Fighter,target_index:8,target_generation:12,locked:true,..Default::default()};
        let reticle=[ship.position[0]+500.,ship.position[1],ship.position[2]];
        let mut rounds=SpaceOrdnancePool::new(3);
        assert_eq!(try_fire_fighter_weapon(&mut ship,&transport,2,1./60.,true,false,reticle,&[],&mut SpaceRng::new(Wide::from_u32(32)),&mut rounds).0,1);
        let round=rounds.slots.iter().find(|r|r.active).unwrap();
        assert_eq!(round.owner_unit,Some(crew(11).owner));
        assert!(round.velocity[0]>0. && round.velocity[2].abs()<round.velocity[0]*0.05);
        assert_eq!((round.lock_target,round.lock_generation),(8,12));
    }

    #[test]
    fn command_transport_disables_tricks_and_cycles_all_spawn_offsets() {
        let (mut coll, world_id, bounds, mut flags) = synthetic_world();
        let mut transport = synthetic_fighter_profile();
        transport.class_name = "synthetic_transport".into();
        transport.is_command_flyer = true;
        transport.energy_trick_single = 0.0;
        transport.energy_trick_double = 0.0;
        transport.trick_roll_speed = 0.0;
        transport.trick_flip_speed = 0.0;
        transport.spawn_point_count = 6;
        transport.spawn_point_offsets = vec![
            [-6.0, -3.0, 8.0],
            [6.0, -3.0, 8.0],
            [-7.0, -3.0, 0.0],
            [7.0, -3.0, 0.0],
            [-6.0, -3.0, -8.0],
            [6.0, -3.0, -8.0],
        ];

        let pad = SpaceVehiclePad {
            name: "pad_trans".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0, 0.0, 900.0],
            yaw: 0.0,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let mut ship = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &transport, 0.0, Some(1), None);
        ship.pilot = Some(SpacePilotId { owner: crate::store::Ref::new(2, 1), team: 1, is_human: true, repair_capable: false });
        ship.step_tick(1.0 / 60.0, &transport, &pad, &bounds, &mut coll, world_id, &mut flags);
        assert!(ship.mobile_spawn_active);
        let p0 = ship.mobile_spawn_point(&transport, 0).unwrap();
        let p1 = ship.mobile_spawn_point(&transport, 1).unwrap();
        let p5 = ship.mobile_spawn_point(&transport, 5).unwrap();
        assert!((p0[0] - (-6.0)).abs() < 1e-6);
        assert!((p1[0] - 6.0).abs() < 1e-6);
        assert!((p5[2] - 892.0).abs() < 1e-6);

        ship.state = SpaceFlightState::Flying;
        ship.pending_input = SpacePilotInput { trick_roll: 1, trick_flip: 1, ..Default::default() };
        ship.step_tick(1.0 / 60.0, &transport, &pad, &bounds, &mut coll, world_id, &mut flags);
        assert!(!ship.trick.active());
    }

    #[test]
    fn hangar_pad_respawns_after_departure_and_pilotless_drifts_in_zero_gravity() {
        let (mut coll, world_id, bounds, mut flags) = synthetic_world();
        let profile = synthetic_fighter_profile();
        let pad = SpaceVehiclePad {
            name: "pad_beta".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0, 0.0, 0.0],
            yaw: 0.0,
            matrix: Matrix4::default().elements,
            spawn_count: 2,
            spawn_time: 1.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 1.0,
            floor_y: 0.0,
        };
        let mut pads = vec![pad];
        let f0 = SpaceFighter::spawned_on_pad(0, 1, 0, &pads[0], 0, &profile, 0.0, Some(0), None);
        let mut f1 = SpaceFighter::spawned_on_pad(1, 1, 0, &pads[0], 0, &profile, 0.0, Some(0), None);
        f1.alive = false;
        let mut fighters = vec![f0, f1];
        let profiles = vec![profile];

        fighters[0].state = SpaceFlightState::Flying;
        fighters[0].position = [200.0, -400.0, 200.0];
        fighters[0].velocity = [0.0, 0.0, 50.0];
        for _ in 0..90 {
            fighters[0].step_tick(1.0 / 60.0, &profiles[0], &pads[0], &bounds, &mut coll, world_id, &mut flags);
            step_vehicle_pads(1.0 / 60.0, &mut pads, &mut fighters, &profiles, &bounds, &mut coll, world_id, &mut flags);
        }
        assert!(fighters[0].alive);
        assert!((fighters[0].position[1] - (-400.0)).abs() < 1e-9);
        assert!(fighters[1].alive);
        assert_eq!(fighters[1].state, SpaceFlightState::Landed);
    }

    #[test]
    fn parse_space_weapon_reads_hud_tag_with_icon_texture_fallback() {
        let mut tagged = bfe_content::Object::new();
        tagged.set("class".into(), Value::str("spindle_gun"));
        tagged.set(
            "properties".into(),
            Value::Array(std::rc::Rc::new(vec![
                Value::Array(std::rc::Rc::new(vec![Value::str("IconTexture"), Value::str("icon_backup")])),
                Value::Array(std::rc::Rc::new(vec![Value::str("HUDTag"), Value::str("icon_spindle")])),
            ])),
        );
        assert_eq!(
            parse_space_weapon(&Value::Object(std::rc::Rc::new(tagged)), 0).hud_tag,
            "icon_spindle"
        );

        let mut fallback = bfe_content::Object::new();
        fallback.set("class".into(), Value::str("lantern_gun"));
        fallback.set(
            "properties".into(),
            Value::Array(std::rc::Rc::new(vec![
                Value::Array(std::rc::Rc::new(vec![Value::str("IconTexture"), Value::str("icon_lantern")])),
            ])),
        );
        assert_eq!(
            parse_space_weapon(&Value::Object(std::rc::Rc::new(fallback)), 0).hud_tag,
            "icon_lantern"
        );
    }

    #[test]
    fn roll_deadzone_filter_roll_lock_and_fourteen_second_auto_level_hold_bank() {
        let (mut coll, world_id, bounds, mut flags) = synthetic_world();
        let mut profile = synthetic_fighter_profile();
        profile.roll_rate = 3.0;
        profile.roll_rate_filter = 4.0;
        profile.level_filter = 2.0;
        let pad = SpaceVehiclePad {
            name: "pad_roll".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0; 3],
            yaw: 0.0,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let mut ship = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &profile, 0.0, None, None);
        ship.pilot = Some(SpacePilotId { owner: crate::store::Ref::new(1, 1), team: 1, is_human: true, repair_capable: false });
        ship.state = SpaceFlightState::Flying;
        ship.airspeed = profile.mid_speed;
        ship.velocity = [0.0, 0.0, profile.mid_speed];
        let dt = 1.0 / 60.0;

        // Roll input inside 0.3 deadzone produces no roll.
        ship.pending_input = SpacePilotInput { roll: 0.25, ..Default::default() };
        for _ in 0..30 {
            ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        assert!(ship.right()[1].abs() < 1e-9);

        // Full right roll banks right wing down (left axis +X lifts, right()[1] > 0).
        ship.pending_input = SpacePilotInput { roll: 1.0, ..Default::default() };
        for _ in 0..30 {
            ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        let banked_after_roll = ship.right()[1];
        assert!(banked_after_roll > 0.5, "expected sustained roll, right.y={banked_after_roll}");
        assert_eq!(ship.level_timer, 0.0);

        // Releasing roll holds the banked attitude indefinitely (5 seconds later, still banked).
        ship.pending_input = SpacePilotInput::default();
        for _ in 0..300 {
            ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        assert!(
            ship.right()[1] > 0.5,
            "craft must hold roll after release, right.y={}",
            ship.right()[1]
        );

        // A yaw nudge (> 0.3) starts the 14 s level timer, but bank stays held through 10 s.
        ship.pending_input = SpacePilotInput { stick_x: 0.5, ..Default::default() };
        ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        ship.pending_input = SpacePilotInput::default();
        for _ in 0..600 {
            ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        assert!(ship.level_timer > 9.0 && ship.level_timer < 14.0);
        let bank_at_10s = ship.right()[1];
        assert!(bank_at_10s > 0.4, "must not auto-level before 14 s: {bank_at_10s}");

        // After 14 s total + 3 s of leveling, wings return near level.
        for _ in 0..420 {
            ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        assert!(ship.right()[1].abs() < 0.05, "expected level after 14 s delay, got {}", ship.right()[1]);

        // RollLock remaps stick_x into roll while suppressing yaw.
        ship.quaternion = quat_from_yaw(0.0);
        ship.yaw_rate = 0.0;
        ship.pending_input = SpacePilotInput { stick_x: 1.0, roll_lock: true, ..Default::default() };
        for _ in 0..30 {
            ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        assert!(ship.right()[1] > 0.5, "RollLock must roll from stick_x");
        assert!(ship.yaw_rate.abs() < 1e-9, "RollLock must suppress yaw");
    }

    #[test]
    fn linear_slew_boost_turn_reduction_momentum_drift_and_inward_bank() {
        let (mut coll, world_id, bounds, mut flags) = synthetic_world();
        let mut profile = synthetic_fighter_profile();
        profile.turn_rate = 1.0;
        profile.pitch_rate = 1.4;
        profile.turn_filter = 4.0;
        profile.turn_filter_decel = 12.0;
        profile.momentum_filter = 6.0;
        profile.bank_angle = 0.5;
        profile.bank_filter = 2.0;
        let pad = SpaceVehiclePad {
            name: "pad_phys".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0; 3],
            yaw: 0.0,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let mut ship = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &profile, 0.0, None, None);
        ship.pilot = Some(SpacePilotId { owner: crate::store::Ref::new(1, 1), team: 1, is_human: true, repair_capable: false });
        ship.state = SpaceFlightState::Flying;
        // In open space, away from the hangar's landing region.
        ship.position = [0.0, 0.0, 400.0];
        ship.airspeed = profile.max_speed;
        ship.velocity = [0.0, 0.0, profile.max_speed];
        let dt = 1.0 / 60.0;

        // First tick of full right yaw slews linearly by TurnFilter * dt = 4/60.
        ship.pending_input = SpacePilotInput { stick_x: 1.0, throttle: 1.0, ..Default::default() };
        ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        assert!((ship.yaw_rate - (-4.0 * dt)).abs() < 1e-9, "yaw_rate={}", ship.yaw_rate);

        // Hold for 30 ticks: yaw reaches -1.0, visual bank leans INTO right turn (bank > 0),
        // and velocity keeps lateral drift while preserving forward airspeed projection.
        for _ in 0..30 {
            ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        assert!((ship.yaw_rate - (-1.0)).abs() < 1e-6);
        assert!(ship.bank > 0.15, "right turn must bank right wing down (bank > 0), got {}", ship.bank);
        let fwd = ship.forward();
        let proj = ship.velocity[0] * fwd[0] + ship.velocity[1] * fwd[1] + ship.velocity[2] * fwd[2];
        assert!((proj - ship.airspeed).abs() < 1e-5, "forward projection must match airspeed");
        let speed = v8math::hypot3(ship.velocity[0], ship.velocity[1], ship.velocity[2]);
        assert!(speed > ship.airspeed + 0.1, "momentum filter must produce lateral drift");

        // Releasing stick decelerates linearly at TurnFilterDecel * dt = 12/60 (3x faster than accel).
        let before_release = ship.yaw_rate;
        ship.pending_input = SpacePilotInput { throttle: 1.0, ..Default::default() };
        ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        assert!(
            ((ship.yaw_rate - before_release) - 12.0 * dt).abs() < 1e-6,
            "decel step {} != {}",
            ship.yaw_rate - before_release,
            12.0 * dt
        );

        // At full boost speed, max yaw rate reduces by 30% (1.0 -> 0.7).
        ship.airspeed = profile.boost_speed;
        ship.velocity = [fwd[0] * profile.boost_speed, fwd[1] * profile.boost_speed, fwd[2] * profile.boost_speed];
        ship.pending_input = SpacePilotInput { stick_x: 1.0, throttle: 1.0, boost: true, ..Default::default() };
        for _ in 0..40 {
            ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        assert!(
            (ship.yaw_rate.abs() - 0.7).abs() < 0.02,
            "boost turn rate should scale to 0.7, got {}",
            ship.yaw_rate
        );

        // A command flyer keeps only half its rate at full boost.
        profile.is_command_flyer = true;
        for _ in 0..40 {
            ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        assert!(
            (ship.yaw_rate.abs() - 0.5).abs() < 0.02,
            "a command flyer's boost turn rate should scale to 0.5, got {}",
            ship.yaw_rate
        );
    }

    #[test]
    fn each_seat_section_owns_the_weapons_it_names_on_its_own_channels() {
        let raw = parse(r#"{
            "base":"commandflyer",
            "weapons":[
                {"class":"moth_gun","channel":0},
                {"class":"moth_pod","channel":1},
                {"class":"moth_spear","channel":2},
                {"class":"moth_ball","channel":2},
                {"class":"moth_ball","channel":2}
            ],
            "vehicleSections":[
                {"kind":"chassis","name":"BODY","properties":[["WeaponName","moth_gun"],["WeaponSection","2"],["WeaponName","moth_pod"]]},
                {"kind":"seat","name":"POST_A","properties":[["WeaponSection","1"],["WeaponName","moth_spear"],["YawLimits","-0 0"],["PitchLimits","0 0"]]},
                {"kind":"seat","name":"POST_B","properties":[["YawLimits","-90.0 0.0"],["PitchLimits","-20.0 45.0"],["WeaponName","moth_ball"]]},
                {"kind":"seat","name":"POST_C","properties":[["WeaponName","moth_ball"]]},
                {"kind":"seat","name":"POST_D","properties":[["YawLimits","-195.0 195.0"]]}
            ]
        }"#).unwrap();
        let p = parse_flight_profile("amber", "moth_carrier", &raw).unwrap();
        let seats: Vec<(u8, u8)> = p.weapons.iter().map(|w| (w.seat, w.channel)).collect();
        assert_eq!(seats, [(0, 0), (0, 1), (1, 0), (2, 0), (3, 0)]);
        assert_eq!(p.seat_count, 5);
        assert_eq!(p.seat_aim_limits[1], [0.0, 0.0, 0.0, 0.0]);
        let d = |x: f64| x * PI / 180.0;
        assert_eq!(p.seat_aim_limits[2], [d(-90.0), 0.0, d(-20.0), d(45.0)]);
        assert_eq!(p.seat_aim_limits[3], DEFAULT_SEAT_AIM_LIMITS);
        assert_eq!(p.seat_aim_limits[4][..2], [d(-195.0), d(195.0)]);
    }

    #[test]
    fn a_gunners_aim_keeps_to_his_seats_arc() {
        let profile = synthetic_fighter_profile();
        let pad = SpaceVehiclePad {
            name: "pad_arc".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0; 3],
            yaw: 0.0,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let ship = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &profile, 0.0, None, None);
        let d = |x: f64| x * PI / 180.0;
        // The left side only (the hull's +X), from 30 degrees up to 45 down.
        let limits = [0.0, d(90.0), d(-30.0), d(45.0)];
        let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9);
        assert!(near(ship.seat_aim_within(limits, [1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]));
        assert!(near(ship.seat_aim_within(limits, [-1.0, 0.0, 0.0]), [0.0, 0.0, 1.0]), "right is held to the nose");
        let up = ship.seat_aim_within(limits, [0.0, 1.0, 0.0]);
        assert!((up[1] - d(30.0).sin()).abs() < 1e-9, "straight up is held 30 degrees up: {up:?}");
        assert!(ship.seat_can_bear(limits, [50.0, 0.0, 10.0]));
        assert!(!ship.seat_can_bear(limits, [-50.0, 0.0, 10.0]));
        // A whole turn of yaw turns freely.
        assert!(near(ship.seat_aim_within([-PI, PI, -1.5, 1.5], [0.0, 0.0, -1.0]), [0.0, 0.0, -1.0]));
    }

    #[test]
    fn drawn_lean_is_forward_speed_times_yaw_rate_over_top_speed_without_clamp() {
        let (mut coll, world_id, bounds, mut flags) = synthetic_world();
        let mut profile = synthetic_fighter_profile();
        profile.turn_rate = 2.0;
        profile.turn_filter = 1000.0;
        profile.max_speed = 100.0;
        profile.boost_speed = 0.0;
        profile.mid_speed = 90.0;
        profile.bank_angle = 0.4;
        profile.bank_filter = 1000.0;
        let pad = SpaceVehiclePad {
            name: "pad_lean".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0; 3],
            yaw: 0.0,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let mut ship = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &profile, 0.0, None, None);
        ship.pilot = Some(SpacePilotId { owner: crate::store::Ref::new(1, 1), team: 1, is_human: true, repair_capable: false });
        ship.state = SpaceFlightState::Flying;
        ship.airspeed = 90.0;
        ship.velocity = [0.0, 0.0, 90.0];
        ship.pending_input = SpacePilotInput { stick_x: -1.0, ..Default::default() };
        ship.step_tick(1.0 / 60.0, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        let fwd = ship.forward();
        let forward_speed = ship.velocity[0] * fwd[0] + ship.velocity[1] * fwd[1] + ship.velocity[2] * fwd[2];
        assert!((ship.yaw_rate - 2.0).abs() < 1e-9, "yaw_rate={}", ship.yaw_rate);
        // 0.4 * 90 * 2 / 100 = 0.72, beyond BankAngle and not divided by TurnRate.
        let want = -0.4 * forward_speed * ship.yaw_rate / 100.0;
        assert!((ship.bank - want).abs() < 1e-9, "bank={} want={want}", ship.bank);
        assert!(ship.bank < -0.7, "bank={}", ship.bank);
    }

    /// A crewed craft flying at `position` along `quaternion` at `speed`, stick centred.
    fn crewed_craft(profile: &SpaceFlightProfile, position: [f64; 3], quaternion: [f64; 4], speed: f64) -> (SpaceFighter, SpaceVehiclePad) {
        let pad = SpaceVehiclePad {
            name: "pad_edge".into(),
            team: 1,
            profile_index: 0,
            control_zone: "cp_north".into(),
            position: [0.0; 3],
            yaw: 0.0,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let mut ship = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, profile, 0.0, None, None);
        ship.pilot = Some(SpacePilotId { owner: crate::store::Ref::new(1, 1), team: 1, is_human: true, repair_capable: false });
        ship.state = SpaceFlightState::Flying;
        ship.position = position;
        ship.quaternion = quaternion;
        ship.airspeed = speed;
        let fwd = ship.forward();
        ship.velocity = [fwd[0] * speed, fwd[1] * speed, fwd[2] * speed];
        ship.pending_input = SpacePilotInput::default();
        (ship, pad)
    }

    #[test]
    fn inside_a_landing_region_speeds_are_capped_and_braking_is_three_times_harder() {
        let p = SpaceFlightProfile { min_speed: 35.0, mid_speed: 70.0, max_speed: 95.0, ..synthetic_fighter_profile() };
        let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9);
        assert!(near(speeds(&p, true), [7.0, 20.0, 60.0]));
        assert!(near(speeds(&p, false), [35.0, 70.0, 95.0]));
        // The throttle at rest asks for the (capped) MidSpeed, held back for MinSpeed.
        assert!((throttle_target_speed(&p, 0.0, true) - 20.0).abs() < 1e-9);
        assert!((throttle_target_speed(&p, -1.0, true) - 7.0).abs() < 1e-9);
        assert!((throttle_target_speed(&p, 1.0, false) - 95.0).abs() < 1e-9);
        // Slowing is three times faster inside, speeding up is not.
        assert_eq!(speed_toward(50.0, 0.0, 1.0, true), 47.0);
        assert_eq!(speed_toward(50.0, 0.0, 1.0, false), 49.0);
        assert_eq!(speed_toward(10.0, 20.0, 1.0, true), 11.0);
    }

    #[test]
    fn nothing_turns_a_craft_at_the_edge_and_outside_it_is_lost_after_six_and_a_half_seconds() {
        let (mut coll, world_id, bounds, mut flags) = synthetic_world();
        let profile = synthetic_fighter_profile();
        let dt = 1.0 / 60.0;
        // 60 m inside the edge, heading 45 degrees out of it, stick centred.
        let (mut ship, pad) = crewed_craft(&profile, [1940.0, 0.0, 0.0], quat_from_yaw(PI * 0.25), profile.min_speed);
        let inward = |ship: &SpaceFighter| -ship.forward()[0];
        let start = inward(&ship);
        assert!((start + std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9, "heading out: {start}");
        let mut outside = 0;
        let mut lost = None;
        for tick in 0..1200 {
            let notice = ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
            assert!(ship.yaw_rate == 0.0 && ship.pitch_rate == 0.0, "turned at tick {tick}");
            assert!((inward(&ship) - start).abs() < 1e-9, "heading moved at tick {tick}");
            if ship.position[0] > 2000.0 {
                outside += 1;
            }
            if notice.destroyed {
                lost = Some(outside);
                break;
            }
            assert!(ship.alive);
        }
        let lost = lost.expect("lost outside");
        assert!((390..=391).contains(&lost), "lost after {lost} ticks outside");

        // Coming back inside starts the time again.
        let (mut ship, pad) = crewed_craft(&profile, [2100.0, 0.0, 0.0], quat_from_yaw(PI * 0.5), profile.min_speed);
        for _ in 0..300 {
            ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        ship.position = [0.0; 3];
        ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        assert!(ship.oob_timer < 0.0);
        ship.position = [2100.0, 0.0, 0.0];
        for _ in 0..300 {
            ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        assert!(ship.alive, "the time ran from the start again");

        // Over the ceiling is not outside, and an empty craft outside is left alone.
        let (mut high, pad) = crewed_craft(&profile, [0.0, 1700.0, 0.0], quat_from_yaw(0.0), profile.min_speed);
        let (mut empty, _) = crewed_craft(&profile, [2100.0, 0.0, 0.0], quat_from_yaw(PI * 0.5), profile.min_speed);
        empty.pilot = None;
        for _ in 0..600 {
            high.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
            empty.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
        }
        assert!(high.alive && empty.alive);
        assert!(high.oob_timer < 0.0 && empty.oob_timer < 0.0);
    }

    #[test]
    fn a_craft_climbing_at_the_ceiling_has_its_nose_drawn_level() {
        let (mut coll, world_id, bounds, mut flags) = synthetic_world();
        let profile = synthetic_fighter_profile();
        let dt = 1.0 / 60.0;
        let step = profile.pitch_rate * dt;
        // Nose up 30 degrees (a turn about the left axis by a negative angle).
        let up30 = |at: [f64; 3]| {
            let q = quat_from_axis_angle([1.0, 0.0, 0.0], -PI / 6.0);
            crewed_craft(&profile, at, [q.x, q.y, q.z, q.w], 90.0)
        };
        let (far, _) = up30([0.0, 0.0, 0.0]);
        assert!((far.forward()[1] - 0.5).abs() < 1e-9);
        // More than two seconds from the ceiling: the stick is the pilot's.
        assert_eq!(far.fly_height_stick(0.4, &bounds, step), 0.4);
        // At the ceiling the stick is wholly the input that levels the nose:
        // nose down, which is a negative stick.
        let (at, _) = up30([0.0, bounds.max[1], 0.0]);
        assert_eq!(at.fly_height_stick(0.4, &bounds, step), -1.0);
        // Halfway into the two seconds the pilot keeps a quarter of his stick.
        let half = bounds.max[1] - 45.0 * 2.0 * 0.5;
        let (mid, _) = up30([0.0, half, 0.0]);
        assert!((mid.fly_height_stick(0.4, &bounds, step) - (0.4 * 0.25 - 0.75)).abs() < 1e-9);
        // Upside down the same leveling is the other way on the stick.
        let mut flipped = at.clone();
        let roll = quat_from_axis_angle([0.0, 0.0, 1.0], PI);
        let mut q = to_q(flipped.quaternion);
        q.multiply(&roll).normalize();
        flipped.quaternion = from_q(&q);
        assert!(flipped.up()[1] < -0.8 && flipped.forward()[1] > 0.49);
        assert_eq!(flipped.fly_height_stick(0.4, &bounds, step), 1.0);
        // A nose already pointing away from the ceiling is left alone.
        let (mut diving, _) = up30([0.0, bounds.max[1], 0.0]);
        diving.velocity[1] = 10.0;
        let q = quat_from_axis_angle([1.0, 0.0, 0.0], PI / 6.0);
        diving.quaternion = [q.x, q.y, q.z, q.w];
        assert_eq!(diving.fly_height_stick(0.4, &bounds, step), 0.4);

        // Flown with the stick centred, the craft levels off at the ceiling.
        let (mut ship, pad) = up30([0.0, bounds.max[1] - 120.0, 0.0]);
        let mut top: f64 = 0.0;
        for _ in 0..600 {
            ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
            top = top.max(ship.position[1]);
        }
        assert!(ship.forward()[1].abs() < 0.01, "nose {}", ship.forward()[1]);
        assert!(top < bounds.max[1], "climbed to {top}");
    }

    /// Response curves of every prepared flyer class to steps of the stick,
    /// roll, throttle and boost, one CSV row per tick. Reads the prepared side
    /// files named in `BFE_FLIGHT_UNITS` (comma separated) and writes
    /// `BFE_FLIGHT_TRACE`; fails when they are not set.
    #[test]
    #[ignore]
    fn flight_response_probe() {
        let (Ok(files), Ok(out)) = (std::env::var("BFE_FLIGHT_UNITS"), std::env::var("BFE_FLIGHT_TRACE")) else {
            panic!("the probe needs BFE_FLIGHT_UNITS (prepared side files, comma separated) and BFE_FLIGHT_TRACE (the CSV to write)");
        };
        use std::io::Write;
        let mut w = std::io::BufWriter::new(std::fs::File::create(out).unwrap());
        writeln!(w, "class,scenario,tick,sx,sy,roll,lock,throttle,boost,airspeed,v_left,v_up,v_fwd,fx,fy,fz,ux,uy,uz,yaw_rate,pitch_rate,roll_rate,w_pitch,w_yaw,w_roll,bank").unwrap();
        type Step = (u32, SpacePilotInput);
        let input = |sx: f64, sy: f64, roll: f64, lock: bool, throttle: f64, boost: bool| {
            let mut i = SpacePilotInput { stick_x: sx, stick_y: sy, roll, throttle, boost, ..Default::default() };
            i.roll_lock = lock;
            i
        };
        let idle = input(0.0, 0.0, 0.0, false, 0.0, false);
        let scenarios: Vec<(&str, f64, Vec<Step>)> = vec![
            ("pitch", 0.0, vec![(30, idle), (120, input(0.0, 1.0, 0.0, false, 0.0, false)), (90, idle)]),
            ("pitch_half", 0.0, vec![(30, idle), (120, input(0.0, 0.5, 0.0, false, 0.0, false)), (90, idle)]),
            ("yaw", 0.0, vec![(30, idle), (120, input(1.0, 0.0, 0.0, false, 0.0, false)), (90, idle)]),
            ("yaw_half", 0.0, vec![(30, idle), (120, input(0.5, 0.0, 0.0, false, 0.0, false)), (90, idle)]),
            ("roll", 0.0, vec![(30, idle), (60, input(0.0, 0.0, 1.0, false, 0.0, false)), (240, idle)]),
            ("roll_then_pitch", 0.0, vec![(30, idle), (30, input(0.0, 0.0, 1.0, false, 0.0, false)), (90, input(0.0, 1.0, 0.0, false, 0.0, false)), (60, idle)]),
            ("roll_then_yaw", 0.0, vec![(30, idle), (30, input(0.0, 0.0, 1.0, false, 0.0, false)), (30, idle), (60, input(1.0, 0.0, 0.0, false, 0.0, false)), (900, idle)]),
            ("roll_lock", 0.0, vec![(30, idle), (60, input(1.0, 0.0, 0.0, true, 0.0, false)), (120, idle)]),
            ("throttle", 0.0, vec![(30, idle), (180, input(0.0, 0.0, 0.0, false, 1.0, false)), (180, idle), (180, input(0.0, 0.0, 0.0, false, -1.0, false)), (120, idle)]),
            ("boost", 0.0, vec![(30, idle), (180, input(0.0, 0.0, 0.0, false, 1.0, true)), (180, idle)]),
            ("from_rest", -1.0, vec![(240, input(0.0, 0.0, 0.0, false, 1.0, false))]),
        ];
        let (mut coll, world_id, bounds, mut flags) = synthetic_world();
        let pad = SpaceVehiclePad {
            name: "probe_pad".into(),
            team: 1,
            profile_index: 0,
            control_zone: "probe".into(),
            position: [0.0, 400.0, -1500.0],
            yaw: 0.0,
            matrix: Matrix4::default().elements,
            spawn_count: 1,
            spawn_time: 5.0,
            expire_time_enemy: 15.0,
            expire_time_field: 30.0,
            decay_time: 8.0,
            respawn_timer: 5.0,
            floor_y: 0.0,
        };
        let dt = 1.0 / 60.0;
        for path in files.split(',') {
            let text = std::fs::read_to_string(path).unwrap();
            let units = parse(&text).unwrap();
            let Some(units) = units.get("units").and_then(Value::as_object) else { continue };
            let mut names: Vec<_> = units.entries().map(|(k, v)| (k.to_string(), v)).collect();
            names.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, def) in names {
                let Some(profile) = parse_flight_profile("probe", &name, def) else { continue };
                for (scenario, start_speed, steps) in &scenarios {
                    let mut ship = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &profile, 0.0, None, None);
                    ship.pilot = Some(SpacePilotId { owner: crate::store::Ref::new(1, 1), team: 1, is_human: true, repair_capable: false });
                    ship.state = SpaceFlightState::Flying;
                    ship.position = pad.position;
                    ship.airspeed = if *start_speed < 0.0 { 0.0 } else { profile.mid_speed };
                    ship.velocity = [0.0, 0.0, ship.airspeed];
                    let mut tick = 0u32;
                    for (count, step) in steps {
                        for _ in 0..*count {
                            let before = ship.quaternion;
                            ship.pending_input = *step;
                            ship.step_tick(dt, &profile, &pad, &bounds, &mut coll, world_id, &mut flags);
                            tick += 1;
                            let (f, u, l) = (ship.forward(), ship.up(), ship.right());
                            let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
                            // Body rates from the attitude change over the tick (local x, y, z).
                            let mut q0 = to_q(before);
                            q0.invert();
                            let mut d = q0;
                            d.multiply(&to_q(ship.quaternion));
                            let (s, c) = (v8math::hypot3(d.x, d.y, d.z), d.w);
                            let angle = 2.0 * v8math::atan2(s, c);
                            let k = if s > 1e-12 { angle / s / dt } else { 0.0 };
                            let v = ship.velocity;
                            writeln!(
                                w,
                                "{name},{scenario},{tick},{},{},{},{},{},{},{:.4},{:.4},{:.4},{:.4},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5}",
                                step.stick_x, step.stick_y, step.roll, u8::from(step.roll_lock), step.throttle, u8::from(step.boost),
                                ship.airspeed, dot(v, l), dot(v, u), dot(v, f), f[0], f[1], f[2], u[0], u[1], u[2],
                                ship.yaw_rate, ship.pitch_rate, ship.roll_rate, d.x * k, d.y * k, d.z * k, ship.bank
                            )
                            .unwrap();
                        }
                    }
                }
            }
        }
    }
}
