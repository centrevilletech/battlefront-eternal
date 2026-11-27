//! 2005 (`GameFamily::Swbf2`) space battlefield simulation.
//!
//! Lives strictly beside the released 2004 (`GameFamily::Swbf1`) flight,
//! vehicle, and prop paths. A 2004 `World` leaves `world.space` as `None` and
//! never executes any code in this module.
//!
//! Single-owner architecture: canonical `f3_props` / `f4` owns health, alive
//! state, team, collision looks (`[built, destroyed, unbuilt]`), and
//! `Event::ObjectKilled` for every destructible building, turret, powerup
//! station, command post, prop, and remote terminal. `SpaceBattlefield` stores
//! extra 2005-only state (per-object shields, visibility, lock-on eligibility,
//! AI damage floor, auto/remote turrets, remote chair rings, orbiting animation
//! links, death blasts) in `SpaceBuildingAttachment` keyed by canonical
//! `building_index`, and owns indestructible interior proximity doors
//! (`SpaceDoor`), starfighters, pads, hangar bounds, and space ordnance.

pub mod aim;
pub mod bounds;
mod digest;
mod openings;
pub mod combat;
pub mod flight;
pub mod passages;
pub mod pilots;
pub(crate) mod door_animation;
mod door_skin;
pub use door_skin::SpaceDoorSkin;
pub use objects::{SpaceDoor, SpaceDoorSnapshot};
pub mod objects;
pub mod local;
pub mod observations;
pub mod world_anim;

use bfe_wide::Wide;
use self::digest::Fnv;
use crate::battle::props;
use crate::battle::rules::events::{ObjectHandle, ObjectProperty, RuleError, RuleResult};
use crate::collision::spheres::Bounds;
use crate::collision::{CollisionOwner, Hit, Ray, MISS};
use crate::combat::impacts::team_damage;
use crate::gen::{Attacker, OrdnanceOwner, UnitBody};
use crate::units::{f3, f3_props};
use crate::world::admit::rows::Cx;
use crate::world::owner::World;
use bfe_content::content::Content;
use bfe_content::js::to_number;
use bfe_content::Value;
use bounds::{SpaceBounds, SpacePrecisionReport};
use combat::{
    apply_shield_and_hull_damage, plant_boarding_timebomb, steer_guided_rocket,
    steer_homing_velocity, try_fire_fighter_weapon, update_fighter_lock_on, LockCandidate,
    SpaceDamageCategory, SpaceDamageOutcome, SpaceDamagePacket, SpaceExplosionSpec, SpaceOrdnancePool,
    SpaceProjectile, SpaceRng, MAX_SPACE_PROJECTILES, PROVISIONAL_FALLBACK_FIGHTER_HIT_RADIUS,
    PROVISIONAL_GUIDED_TURN_RATE,
};
use flight::{
    attach_vehicle_pack_to_profile, parse_flight_profile, placed_prop_ci, step_vehicle_pads,
    HullMeets, SpaceFighter, SpaceFlightProfile, SpaceFlightState, SpaceLockTargetKind, SpaceOrdnanceKind,
    SpacePilotId, SpacePilotInput, SpaceTrick, SpaceVehiclePad,
};
use objects::{
    aim_turret_toward, build_space_attachments_and_doors, has_turret_los, lead_target_direction, lead_target_point,
    lookup_by_name, step_turret_fire, BuildingCoreFacts, BuiltSpaceObjects,
    SpaceBuildingAttachment, SpaceBuildingSnapshot,
    SpaceSoldierTarget, SpaceTurretPilotType, NO_AUTO_RESPAWN_THRESHOLD,
    PROVISIONAL_TURRET_MUZZLE_PAD, UNREPAIRABLE_MAX_HEALTH_THRESHOLD,
};
use three::Matrix4;
use world_anim::{is_animated_name, AnimatedObject, SpaceWorldAnims};

/// Fixed simulation timestep for space battlefields (60 Hz).
pub const SPACE_FIXED_DT: f64 = 1.0 / 60.0;

/// Minimum preallocated on-foot target slots, also used by constructed battlefields.
const MIN_SOLDIER_TARGET_SLOTS: usize = 128;

/// How far off the surface a round struck its blast goes off (metres).
const BLAST_OFF_SURFACE: f64 = 0.2;

/// Added to a blast's reach before a soldier is passed over unmeasured (metres).
const BLAST_REACH_MARGIN: f64 = 3.0;

/// Pre-allocated per-tick space event capacity (overflow sets explicit
/// `events_overflowed` + `dropped_events`, never drops silently).
pub const MAX_SPACE_EVENTS: usize = 256;

/// Provisional vertical ray origin offset above a vehicle pad when sampling
/// hangar floor elevation (metres).
pub const PROVISIONAL_PAD_FLOOR_PROBE_UP: f64 = 2.0;

/// Provisional downward ray distance when sampling hangar floor elevation
/// below a vehicle pad (metres).
pub const PROVISIONAL_PAD_FLOOR_PROBE_DOWN: f64 = 40.0;

/// Provisional per-pad pre-allocated fighter slot cap (`SpawnCount` upper bound).
pub const PROVISIONAL_PAD_MAX_SPAWN_COUNT: u32 = 8;

/// Admission ceiling across all vehicle pads, including optional layers.
pub const MAX_PREPARED_FIGHTERS: usize = 1024;
pub const PROVISIONAL_PAD_RESPAWN_SECONDS: f64 = 8.0;
pub const PROVISIONAL_PAD_ENEMY_EXPIRY_SECONDS: f64 = 20.0;
pub const PROVISIONAL_PAD_FIELD_EXPIRY_SECONDS: f64 = 40.0;
pub const PROVISIONAL_PAD_DECAY_SECONDS: f64 = 10.0;

/// Queued radial splash detonation processed deterministically inside a tick.
#[derive(Clone, Copy, Debug)]
struct PendingBlast {
    position: [f64; 3],
    blast: SpaceExplosionSpec,
    attacker_unit: Option<SpaceOwner>,
    attacker_team: i32,
    attacker_is_ai: bool,
    emitter: Option<combat::EmittingBody>,
    weapon_index: u8,
    explosion: bool,
}

/// What a soldier's shot did to a craft, until the battlefield's next step.
#[derive(Clone, Copy, Debug)]
struct PendingHit {
    fighter: usize,
    generation: u32,
    packet: SpaceDamagePacket,
}

/// No fewer than the shots soldiers can have in flight at once.
const MAX_PENDING_HITS: usize = 1024;

/// Events emitted by the 2005 space battlefield during a tick (pre-allocated
/// buffer cleared at the start of each tick, zero per-tick allocations).
///
/// Passive `AddShield` recharge does NOT emit `ShieldChanged` (only active
/// damage or explicit shield property mutations do).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpaceEvent {
    FighterTakeoff {
        fighter: usize,
        generation: u32,
        pilot_unit: Option<SpaceOwner>,
    },
    FighterLanded {
        fighter: usize,
        generation: u32,
        pilot_unit: Option<SpaceOwner>,
        zone: usize,
    },
    FighterDestroyed {
        fighter: usize,
        generation: u32,
        team: i32,
        attacker_unit: Option<SpaceOwner>,
    },
    PilotEjected {
        fighter: usize,
        generation: u32,
        pilot_unit: SpaceOwner,
        killed_in_vacuum: bool,
    },
    WeaponFired {
        fighter: Option<usize>,
        building: Option<usize>,
        source_life: f64,
        weapon_index: u8,
        origin: [f64; 3],
        direction: [f64; 3],
        count: u32,
    },
    LockChanged {
        fighter: usize,
        generation: u32,
        weapon_index: u8,
        kind: SpaceLockTargetKind,
        target_index: usize,
        target_generation: u64,
        locked: bool,
    },
    DoorChanged {
        door: usize,
        open: bool,
        locked: bool,
    },
    BuildingDamaged {
        building: usize,
        life: f64,
        hull_loss: f64,
        shield_loss: f64,
        attacker_unit: Option<SpaceOwner>,
    },
    BuildingDestroyed {
        building: usize,
        life: f64,
        team: i32,
        attacker_unit: Option<SpaceOwner>,
    },
    BuildingRespawned {
        building: usize,
        life: f64,
        team: i32,
    },
    ShieldChanged {
        building: usize,
        life: f64,
        cur_shield: f64,
        max_shield: f64,
    },
}

/// Impacts the log keeps per tick; more in one tick are counted, not kept.
pub const SPACE_IMPACT_CAPACITY: usize = 1024;

/// What struck what during the latest tick: direct hits, splash, ship object
/// explosions and the hull contact or other flight loss that destroyed a
/// craft. Kept for tools (a kill feed, friendly fire, fatal contacts); it is
/// not part of the battle's state or its digest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpaceImpact {
    pub cause: SpaceImpactCause,
    /// The craft or ship object whose weapon fired, or whose explosion it was.
    pub emitter: Option<combat::EmittingBody>,
    /// The attacking side; for a flight loss, the craft's own team.
    pub team: i32,
    pub target: SpaceImpactTarget,
    pub hull_loss: f64,
    pub shield_loss: f64,
    pub destroyed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceImpactCause {
    /// A round's direct hit; `weapon` indexes the emitter's weapons.
    Round { weapon: u8 },
    /// Splash from a round's blast.
    Splash,
    /// A destroyed ship object's explosion.
    Explosion,
    /// The craft's hull struck collision body `body` (see `body_owner`).
    Contact { body: i32 },
    /// Another flight loss: the boundary timer or an empty craft decaying.
    Flight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceImpactTarget {
    Fighter { index: usize, generation: u32 },
    Building { index: usize },
    Soldier,
    /// A surface without health (a hull, a door, a shield plane): collision
    /// body `body`, negative when none.
    Surface { body: i32 },
}

/// How many craft and soldiers one cast of a beam goes through.
const MAX_PASSED: usize = 16;

/// What a round's cast meets.
#[derive(Clone, Copy, Debug, PartialEq)]
enum StruckThing {
    Fighter(usize),
    Building(usize),
    Soldier(SpaceSoldierTarget),
    Forcefield,
    /// A surface without health.
    Surface,
}

impl StruckThing {
    /// Whether a beam that passes through goes on behind it.
    fn lets_beams_through(self) -> bool {
        matches!(self, Self::Fighter(_) | Self::Soldier(_))
    }
}

#[derive(Clone, Copy, Debug)]
struct Struck {
    hit: Hit,
    body: Option<u32>,
    thing: StruckThing,
}

impl Struck {
    /// Where the blast of a round that flew along `way` goes off: just off the
    /// surface, on the side the round came from.
    fn off_surface(&self, way: [f64; 3]) -> [f64; 3] {
        let h = &self.hit;
        let off = if h.nx * way[0] + h.ny * way[1] + h.nz * way[2] > 0.0 { -BLAST_OFF_SURFACE } else { BLAST_OFF_SURFACE };
        [h.px + h.nx * off, h.py + h.ny * off, h.pz + h.nz * off]
    }
}

/// A hit of a ray along `dir` on something without a surface of its own.
fn plain_hit(distance: f64, [px, py, pz]: [f64; 3], dir: [f64; 3]) -> Hit {
    Hit { distance, px, py, pz, nx: -dir[0], ny: -dir[1], nz: -dir[2] }
}

/// What a collision body belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceBodyOwner {
    Fighter(usize),
    Building(usize),
    Other,
}

/// Authoritative 2005 space battlefield state owned by `World` when a 2005
/// void-terrain space map is admitted.
pub struct SpaceBattlefield {
    pub bounds: SpaceBounds,
    pub world_anims: SpaceWorldAnims,
    pub profiles: Vec<SpaceFlightProfile>,
    pub pads: Vec<SpaceVehiclePad>,
    pub fighters: Vec<SpaceFighter>,
    /// Space attachments aligned 1-to-1 with canonical `f3_props` buildings.
    pub buildings: Vec<SpaceBuildingAttachment>,
    /// Indestructible interior proximity doors (`base == "door"`, no health).
    pub doors: Vec<SpaceDoor>,
    pub ordnance: SpaceOrdnancePool,
    /// The players the seat driver connects to their craft seats and chairs
    /// (`local`), reserved at admission.
    humans: Vec<local::Connection>,
    /// A local battle's developer start (`World::start_local_space_developer`).
    developer: Option<Box<local::Developer>>,
    pub events: Vec<SpaceEvent>,
    /// True when the most recent tick produced more events than `events.capacity()`.
    pub events_overflowed: bool,
    /// Number of events dropped in the most recent tick due to buffer overflow.
    pub dropped_events: u32,
    pub collision_world: usize,
    pub tick: u32,
    pub elapsed_s: f64,
    rng: SpaceRng,
    building_name_index: Vec<(String, usize)>,
    building_slot_by_index: Vec<Option<usize>>,
    door_name_index: Vec<(String, usize)>,
    pub passage_barriers: Vec<passages::PassageBarrier>,
    navigation_toggle: Option<passages::NavigationToggle>,
    planning_connections: Vec<passages::DoorConnection>,
    planning_toggle: Option<passages::NavigationToggle>,
    body_to_building: Vec<i32>,
    body_to_fighter: Vec<i32>,
    lock_candidates: Vec<LockCandidate>,
    repair_classes: Vec<crate::store::StrId>,
    soldier_cache: Vec<SpaceSoldierTarget>,
    /// A live on-foot target exceeded the reserved unit registry this tick.
    pub soldier_targets_truncated: bool,
    blast_queue: Vec<PendingBlast>,
    pending_hits: Vec<PendingHit>,
    /// Bot pilots, reserved at admission.
    pub pilots: pilots::PilotBots,
    /// This tick's impacts (see `SpaceImpact`), and how many did not fit.
    pub impacts: Vec<SpaceImpact>,
    pub impacts_dropped: u32,
    /// Which collision tree set craft hulls currently expose.
    shown_hulls: HullMeets,
    /// Recently killed crew from enclosed space cockpits kept hidden until despawn.
    hidden_dead_crew: Vec<(u32, u32)>,
    /// The bots' aim error: each craft's sphere of miss as its occupants make
    /// it (by craft index), and the drifting direction of each gun a bot
    /// works (by attachment index).
    craft_spheres: Vec<crate::ai::combat::aim::AimMiss>,
    gun_spheres: Vec<crate::ai::combat::aim::AimMiss>,
    /// The drifting direction of each craft's gunner seats a bot works (by
    /// craft index, then seat 1 to 5).
    seat_spheres: Vec<[crate::ai::combat::aim::AimMiss; 5]>,
}

fn read_matrix(desc: &Value) -> [f64; 16] {
    desc.get("matrix")
        .and_then(Value::as_array)
        .filter(|m| m.len() >= 16)
        .map(|m| {
            std::array::from_fn(|i| {
                let x = to_number(m.get(i));
                if x.is_finite() { x } else { 0.0 }
            })
        })
        .unwrap_or_else(|| Matrix4::default().elements)
}

#[inline]
fn building_core(world: &World, building_index: usize) -> Option<BuildingCoreFacts> {
    f3_props::building_core_in(world, building_index).map(BuildingCoreFacts::from)
}

fn attacker_is_ai(world: &World, attacker: Attacker) -> bool {
    let store = &world.sim.store;
    match attacker {
        Attacker::OwnerSnapshot(r) => store.owner_snapshot.get(r).is_some_and(|o| !o.human),
        Attacker::Unit(u) => store.unit.get(u).is_some_and(|u| !u.human),
        Attacker::Vehicle(v) => store
            .vehicle
            .get(v)
            .and_then(|veh| {
                veh.seats.iter().find_map(|seat| {
                    let lease = store.seat_lease.get(seat.lease?)?;
                    let unit = store.unit.get(lease.unit?)?;
                    Some(!unit.human)
                })
            })
            .unwrap_or(false),
        _ => false,
    }
}

/// Returns true when a 2005 building's `MaxHealth` disables fusion-cutter repair.
#[inline]
pub(crate) fn is_building_unrepairable(max_health: f64) -> bool {
    max_health >= UNREPAIRABLE_MAX_HEALTH_THRESHOLD
}

/// Suppresses automatic post-death regeneration on 2005 buildings whose
/// `RespawnTime` or `MaxHealth` disables auto-respawn.
#[inline]
pub(crate) fn suppress_building_regen(frame: &props::Building, settings: &props::Settings) -> bool {
    !frame.alive()
        && (settings.respawn_time >= NO_AUTO_RESPAWN_THRESHOLD
            || settings.max_health >= UNREPAIRABLE_MAX_HEALTH_THRESHOLD)
}

pub(crate) fn building_ai_floor(world: &World, index: usize, attacker: Attacker) -> f64 {
    if !attacker_is_ai(world, attacker) { return 0.0; }
    world.space.as_ref().and_then(|s| s.building_slot_by_index.get(index).copied().flatten()
        .map(|slot| s.buildings[slot].ai_damage_threshold)).unwrap_or(0.0)
}

/// Handles 2005-only `ObjectProperty` mutations (`MaxShield`, `CurrentShield`,
/// `AddShield`, `Visible`, `LockOn`, `AiDamageThreshold`) delegated from
/// `f3_props::rule_property` when `world.space` is attached.
pub(crate) fn rule_property_in(
    world: &mut World,
    building_index: usize,
    property: ObjectProperty,
) -> RuleResult<()> {
    let Some(space) = world.space.as_deref_mut() else {
        return Err(RuleError::NotAvailable);
    };
    let Some(slot) = space
        .building_slot_by_index
        .get(building_index)
        .copied()
        .flatten()
    else {
        return Err(RuleError::NotAvailable);
    };
    match property {
        ObjectProperty::Visible(v) => {
            space.buildings[slot].visible = v;
            Ok(())
        }
        ObjectProperty::LockOn(v) => {
            space.buildings[slot].lock_on_enabled = v;
            Ok(())
        }
        ObjectProperty::AiDamageThreshold(v) => {
            if !v.is_finite() {
                return Err(RuleError::InvalidValue);
            }
            space.buildings[slot].ai_damage_threshold = v8math::min(1.0, v8math::max(0.0, v));
            Ok(())
        }
        _ => Err(RuleError::NotAvailable),
    }
}

/// Provisional range of the fighter sight ray in metres.
pub const PROVISIONAL_RETICLE_RANGE: f64 = 1024.0;
/// Provisional distance outside the hull where a soldier can reach a seat.
pub const PROVISIONAL_BOARDING_REACH: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpaceSeat { pub fighter: usize, pub generation: u32, pub seat: u8, pub owner: SpaceOwner }
/// The occupied seat's mount and fire-control state at a fixed tick.
pub struct SpaceSeatWeapon<'a> {
    pub spec: &'a flight::SpaceWeaponSpec,
    pub runtime: &'a flight::SpaceWeaponRuntime,
    pub lock: &'a flight::SpaceLockState,
    pub selected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpaceTerminalSeat { pub chair: usize, pub chair_life: f64, pub turret: usize, pub turret_life: f64, pub owner: SpaceOwner }


pub type SpaceOwner = crate::store::Ref<crate::gen::OwnerSnapshot>;

fn resolve_attacker(owner: Option<SpaceOwner>) -> Attacker {
    owner.map_or(Attacker::Null, Attacker::OwnerSnapshot)
}

fn with_space_strike(
    world: &mut World,
    unit: crate::store::Ref<crate::gen::Unit>,
    weapon: Option<crate::units::infantry::contact::Weapon>,
    apply: impl FnOnce(&mut World),
) {
    let previous = weapon.and_then(|weapon| {
        let target = world
            .sim
            .store
            .unit
            .get(unit)
            .map(|u| crate::units::infantry::contact::identity(unit, u))?;
        let infantry = world.units.infantry.as_mut()?;
        Some(std::mem::replace(
            &mut infantry.strike,
            Some(crate::units::infantry::contact::Strike {
                target,
                source: None,
                cause: crate::units::infantry::contact::Cause::Shot { definition: 0 },
                weapon: Some(weapon),
                credit: None,
                reflected: false,
            }),
        ))
    });
    apply(world);
    if let Some(previous) = previous {
        if let Some(infantry) = world.units.infantry.as_mut() {
            infantry.strike = previous;
        }
    }
}

fn apply_space_damage(
    world: &mut World,
    unit: crate::store::Ref<crate::gen::Unit>,
    amount: f64,
    attacker: Attacker,
    weapon: Option<crate::units::infantry::contact::Weapon>,
) {
    with_space_strike(world, unit, weapon, |world| {
        f3::with_services_in(world, |world, services| {
            if let Some(services) = services {
                crate::combat::live::damage_in(world, unit, amount, attacker, services);
            }
        });
    });
}

/// A blast against one soldier, by the rules every other explosion hurts him by.
fn soldier_blast(
    world: &mut World,
    soldier: SpaceSoldierTarget,
    pending: &PendingBlast,
    friendly_fire: bool,
    weapon: Option<crate::units::infantry::contact::Weapon>,
) {
    // Those measure him, but nobody this far off is in reach.
    let reach = v8math::max(pending.blast.inner_radius, pending.blast.outer_radius);
    if dist_3d(pending.position, soldier.center) > reach + soldier.radius + BLAST_REACH_MARGIN { return; }
    let Some(unit) = world.sim.store.unit.get(soldier.unit) else { return; };
    if !unit.alive || unit.combat.dead || unit.generation != soldier.life || !f3::active_in(&world.units) { return; }
    if f3::registered_category_in(&world.units, soldier.unit.slot).is_none() { return; }
    let Some(packet) = SpaceDamagePacket::from_blast(&pending.blast, 1.0, pending.attacker_unit, pending.attacker_team, pending.attacker_is_ai)
        .credited(world) else { return; };
    if friendly_fire_scale(packet.attacker_team, packet.attacker_is_ai, soldier.team, soldier.is_human, friendly_fire) <= 0.0 { return; }
    let blast = &pending.blast;
    let packet = crate::combat::impacts::BlastPacket {
        hit: crate::combat::impacts::HitPacket { damage: blast.damage, scales: blast.scales, shield: blast.shield_scale },
        inner: blast.inner_radius, outer: blast.outer_radius, push: 0.0, push_inner: 0.0, push_outer: 0.0,
    };
    let owner = pending.attacker_unit.map_or(crate::gen::OrdnanceOwner::Null, crate::gen::OrdnanceOwner::OwnerSnapshot);
    with_space_strike(world, soldier.unit, weapon, |world| {
        f3::with_services_in(world, |world, services| {
            if let Some(services) = services {
                crate::combat::live::blast_unit_in(world, soldier.unit, &packet, pending.position, owner, services);
            }
        });
    });
}

fn soldier_hit(
    world: &mut World,
    soldier: SpaceSoldierTarget,
    packet: &SpaceDamagePacket,
    friendly_fire: bool,
    weapon: Option<crate::units::infantry::contact::Weapon>,
) {
    let Some(unit) = world.sim.store.unit.get(soldier.unit) else { return; };
    if !unit.alive || unit.combat.dead || unit.generation != soldier.life || !f3::active_in(&world.units) { return; }
    let Some(packet) = packet.credited(world) else { return; };
    let relation = friendly_fire_scale(packet.attacker_team, packet.attacker_is_ai, soldier.team, soldier.is_human, friendly_fire);
    let Some(category) = f3::registered_category_in(&world.units, soldier.unit.slot) else { return; };
    let category = category as usize;
    let scale = packet.scales.get(category).copied().unwrap_or(0.0);
    let amount = packet.raw_damage * scale * relation;
    if amount > 0.0 {
        apply_space_damage(world, soldier.unit, amount, resolve_attacker(packet.attacker_unit), weapon);
    }
}

impl SpaceDamagePacket {
    fn credited(&self, world: &World) -> Option<Self> {
        if let Some(owner) = self.attacker_unit {
            let row = world.sim.store.owner_snapshot.get(owner)?;
            return Some(Self { attacker_team: row.team as i32, attacker_is_ai: !row.human, ..*self });
        }
        Some(*self)
    }
}

fn place_rider(body: &mut crate::gen::Soldier, position: [f64; 3], velocity: [f64; 3]) {
    body.previous = body.position;
    body.position = position;
    body.settled = position;
    body.velocity = velocity;
}

fn carrier_seat(world: &World, slot: u32, life: f64) -> Option<(&SpaceFighter, u8)> {
    let space = world.space()?;
    let r = world.sim.store.unit.reference(slot)?;
    for f in space.fighters.iter().filter(|f| f.alive && f.occupied()) {
        for (seat, crew) in std::iter::once(f.pilot).chain(f.gunners).enumerate() {
            if let Some(p) = crew {
                if world
                    .sim
                    .store
                    .owner_snapshot
                    .get(p.owner)
                    .is_some_and(|o| o.source_unit == r && o.generation == life)
                {
                    return Some((f, seat as u8));
                }
            }
        }
    }
    None
}

fn carrier(world: &World, slot: u32, life: f64) -> Option<&SpaceFighter> {
    carrier_seat(world, slot, life).map(|(f, _)| f)
}

pub(crate) fn carried(world: &World, slot: u32, life: f64) -> bool {
    carrier(world, slot, life).is_some()
}

/// Where a chair that shows its operator seats him in the world, and the way
/// he faces.
fn chair_seat_world(att: &objects::SpaceBuildingAttachment) -> Option<([f64; 3], f64)> {
    let local = att.terminal.as_ref()?.seat?;
    let m = &att.matrix;
    let at = std::array::from_fn(|k| m[k] * local[0] + m[4 + k] * local[1] + m[8 + k] * local[2] + m[12 + k]);
    Some((at, v8math::atan2(m[8], m[10])))
}

/// The chair that seats soldier `slot` of life `life`, as an operator it shows.
fn operator_chair(world: &World, slot: u32, life: f64) -> Option<([f64; 3], f64)> {
    let space = world.space()?;
    let r = world.sim.store.unit.reference(slot)?;
    space.buildings.iter().find(|att| {
        att.terminal.as_ref().and_then(|t| t.occupant).is_some_and(|p| {
            world.sim.store.owner_snapshot.get(p.owner).is_some_and(|o| o.source_unit == r && o.generation == life)
        })
    }).and_then(chair_seat_world)
}

pub(crate) fn move_mounted(world: &World, slot: u32, unit: &mut crate::gen::Unit) -> bool {
    let Some(space) = world.space() else { return false };
    let Some((f, seat)) = carrier_seat(world, slot, unit.generation) else {
        // A chair's operator stays in its seat.
        let Some((at, yaw)) = operator_chair(world, slot, unit.generation) else { return false };
        let UnitBody::Soldier(body) = &mut unit.soldier else { return false };
        place_rider(body, at, [0.0; 3]);
        body.previous = at;
        body.air = None;
        body.dive = 0.0;
        body.aim_yaw = yaw;
        return true;
    };
    let UnitBody::Soldier(body) = &mut unit.soldier else { return false };
    let pos = f.seat_world_position(&space.profiles[f.profile_index], seat);
    place_rider(body, pos, f.velocity);
    body.previous = pos;
    body.air = None;
    body.dive = 0.0;
    let direction = f.forward();
    body.aim_yaw = v8math::atan2(direction[0], direction[2]);
    body.aim_pitch = v8math::asin(v8math::min(1.0, v8math::max(-1.0, direction[1])));
    true
}

fn living_occupant(world: &World, owner: SpaceOwner) -> Option<crate::store::Ref<crate::gen::Unit>> {
    let unit = crate::combat::owners::resolve(&world.sim.store, owner)?;
    let u = world.sim.store.unit.get(unit)?;
    (u.alive && !u.combat.dead && !u.removed).then_some(unit)
}

fn kill_occupant(
    world: &mut World,
    pilot: SpacePilotId,
    attacker: Attacker,
    weapon: Option<crate::units::infantry::contact::Weapon>,
) {
    let Some(unit) = living_occupant(world, pilot.owner) else { return; };
    apply_space_damage(world, unit, f64::MAX, attacker, weapon);
}

#[inline]
fn friendly_fire_scale(
    attacker_team: i32,
    attacker_is_ai: bool,
    target_team: i32,
    target_is_human: bool,
    friendly_fire: bool,
) -> f64 {
    team_damage(
        f64::from(attacker_team),
        !attacker_is_ai,
        f64::from(target_team),
        target_is_human,
        friendly_fire,
    )
}

fn sample_pad_floor_y(
    collision: &mut CollisionOwner,
    collision_world: usize,
    pad_pos: [f64; 3],
) -> f64 {
    collision
        .ray_hit(
            collision_world as u32,
            Ray {
                ox: pad_pos[0],
                oy: pad_pos[1] + PROVISIONAL_PAD_FLOOR_PROBE_UP,
                oz: pad_pos[2],
                dx: 0.0,
                dy: -1.0,
                dz: 0.0,
            },
            PROVISIONAL_PAD_FLOOR_PROBE_DOWN,
            -1,
        )
        .map_or(pad_pos[1], |(_, hit)| hit.py)
}

fn build_space_bounds(battle: &bfe_content::battle::Battle) -> SpaceBounds {
    let mut bounds = SpaceBounds::from_world_json(&battle.world);
    if let Some(data) = &battle.swbf2 {
        openings::attach(&mut bounds, &data.map_admission.map_data);
    }
    bounds
}

fn build_space_world_anims(
    battle: &bfe_content::battle::Battle,
    prop_kernels: &[(usize, u8, u32)],
    prop_local_bounds: &[(u32, Bounds)],
) -> (SpaceWorldAnims, Vec<(usize, usize)>) {
    let mut animated_objects = Vec::new();
    let mut animated_by_instance = Vec::new();
    let Some(swbf2) = battle.swbf2.as_ref() else {
        return (SpaceWorldAnims::default(), animated_by_instance);
    };
    let anims = &swbf2.map_admission.world_animations;
    let instances = battle
        .props
        .as_ref()
        .and_then(|p| p.get("instances"))
        .and_then(Value::as_array)
        .unwrap_or(&[]);
    for (inst_idx, desc) in instances.iter().enumerate() {
        let name = desc.get("name").and_then(Value::as_str).unwrap_or("");
        if !is_animated_name(anims, name) {
            continue;
        }
        let base_matrix = read_matrix(desc);
        let mut inv = Matrix4 {
            elements: base_matrix,
        };
        inv.invert();

        let mut bodies: [Option<(u32, Bounds)>; 3] = [None, None, None];
        for &(p_inst, look, kernel) in prop_kernels {
            if p_inst == inst_idx && (look as usize) < 3 {
                let local_bounds = prop_local_bounds
                    .iter()
                    .find(|&&(k, _)| k == kernel)
                    .map(|&(_, b)| b)
                    .unwrap_or([-1.0, -1.0, -1.0, 1.0, 1.0, 1.0]);
                bodies[look as usize] = Some((kernel, local_bounds));
            }
        }
        let anim_idx = animated_objects.len();
        animated_by_instance.push((inst_idx, anim_idx));
        animated_objects.push(AnimatedObject {
            name: name.to_string(),
            prop_instance: inst_idx,
            base_matrix,
            base_inverse: inv.elements,
            prev_matrix: base_matrix,
            live_matrix: base_matrix,
            live_inverse: inv.elements,
            bodies,
        });
    }
    (
        SpaceWorldAnims::build(anims, animated_objects),
        animated_by_instance,
    )
}

fn parse_battle_profiles(
    battle: &bfe_content::battle::Battle,
    collision: &mut CollisionOwner,
    mut on_repair_class: impl FnMut(&str),
) -> Vec<SpaceFlightProfile> {
    let vehicle_pack = battle
        .swbf2
        .as_ref()
        .and_then(|s| s.map_admission.vehicle_pack.as_ref());
    let map_classes = battle
        .swbf2
        .as_ref()
        .and_then(|s| s.map_admission.map_classes.as_ref());
    let mut profiles = Vec::new();
    for side in &battle.sides {
        if let Some(units_obj) = side.units.as_object() {
            for (class_name, def) in units_obj.entries() {
                if flight::pilot_repairs(def) {
                    on_repair_class(class_name);
                }
                if let Some(mut profile) = parse_flight_profile(&side.name, class_name, def) {
                    attach_vehicle_pack_to_profile(&mut profile, vehicle_pack, collision);
                    objects::attach_rocket_blasts(&mut profile.weapons, def, map_classes);
                    profiles.push(profile);
                }
            }
        }
    }
    profiles
}

fn parse_vehicle_pads(
    placed: &[bfe_content::map::PlacedObject],
    profiles: &[SpaceFlightProfile],
    collision: &mut CollisionOwner,
    collision_world: usize,
) -> (Vec<SpaceVehiclePad>, Vec<usize>) {
    let mut pads = Vec::new();
    let mut fighter_pad_indices = Vec::new();
    for obj in placed {
        if !obj.base.eq_ignore_ascii_case("vehiclespawn") {
            continue;
        }
        let class_name = [
            ("ClassRepATK", "24f75004"),
            ("ClassRepDEF", "61c30279"),
            ("ClassCISATK", "a2c909c6"),
            ("ClassCISDEF", "9e973303"),
            ("ClassAllATK", "dd29eddc"),
            ("ClassAllDEF", "2a456b61"),
            ("ClassImpATK", "54d2fc8b"),
            ("ClassImpDEF", "07b77f06"),
            ("ClassNeutral", "8cdb433e"),
        ]
        .iter()
        .find_map(|&(plain, hex)| placed_prop_ci(&obj.properties, plain, hex))
        .filter(|s| !s.is_empty());
        let Some(cls) = class_name else { continue };
        let Some(profile_index) = profiles
            .iter()
            .position(|p| p.class_name.eq_ignore_ascii_case(cls))
        else {
            continue;
        };
        let spawn_count = placed_prop_ci(&obj.properties, "SpawnCount", "88923ff7")
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 1.0)
            .map(|v| v as u32)
            .unwrap_or(1);
        let spawn_time = placed_prop_ci(&obj.properties, "SpawnTime", "4e99b371")
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(PROVISIONAL_PAD_RESPAWN_SECONDS);
        let expire_enemy = placed_prop_ci(&obj.properties, "ExpireTimeEnemy", "98fcc969")
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(PROVISIONAL_PAD_ENEMY_EXPIRY_SECONDS);
        let expire_field = placed_prop_ci(&obj.properties, "ExpireTimeField", "937e6a7d")
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(PROVISIONAL_PAD_FIELD_EXPIRY_SECONDS);
        let decay_time = placed_prop_ci(&obj.properties, "DecayTime", "1c098b3c")
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(PROVISIONAL_PAD_DECAY_SECONDS);
        let control_zone = placed_prop_ci(&obj.properties, "ControlZone", "447c6db0")
            .unwrap_or("")
            .to_string();
        let yaw = v8math::atan2(obj.matrix[8], obj.matrix[10]);
        let floor_y = sample_pad_floor_y(collision, collision_world, obj.position);
        let pad_index = pads.len();
        for _ in 0..spawn_count {
            fighter_pad_indices.push(pad_index);
        }
        pads.push(SpaceVehiclePad {
            name: obj.name.clone(),
            team: obj.team,
            profile_index,
            control_zone,
            position: obj.position,
            yaw,
            matrix: obj.matrix,
            spawn_count,
            spawn_time,
            expire_time_enemy: expire_enemy,
            expire_time_field: expire_field,
            decay_time,
            respawn_timer: spawn_time,
            floor_y,
        });
    }
    (pads, fighter_pad_indices)
}

/// Static Space Assault battle metadata built directly from prepared battle content
/// using the exact same admission parsers as [`SpaceBattlefield::build`]. Used by
/// online clients so every profile index, weapon index, turret index, door index,
/// world animation group, and landing zone matches the server 1-for-1.
pub struct SpaceClientData {
    pub bounds: SpaceBounds,
    pub world_anims: SpaceWorldAnims,
    pub profiles: Vec<SpaceFlightProfile>,
    pub pads: Vec<SpaceVehiclePad>,
    pub fighter_pad_indices: Vec<usize>,
    pub fighter_slots: usize,
    pub buildings: Vec<SpaceBuildingAttachment>,
    pub turret_buildings: Vec<usize>,
    pub doors: Vec<SpaceDoor>,
    pub weapon_hashes: Vec<u32>,
}

impl SpaceClientData {
    /// Builds static Space metadata from `battle` and `collision` (`collision_world`)
    /// when `battle` is a 2005 void-terrain space battle. Returns `None` for 2004
    /// battles and 2005 ground battles.
    pub fn from_battle(
        battle: &bfe_content::battle::Battle,
        collision: &mut CollisionOwner,
        collision_world: usize,
    ) -> Option<Self> {
        if battle.family != bfe_content::GameFamily::Swbf2 {
            return None;
        }
        let swbf2 = battle.swbf2.as_ref()?;
        if swbf2.map_admission.has_terrain || swbf2.map_admission.vehicle_pack.is_none() {
            return None;
        }
        let bounds = build_space_bounds(battle);
        let (world_anims, animated_by_instance) = build_space_world_anims(battle, &[], &[]);

        let live_src = crate::world::admit::rows::props::live_sources(battle);
        let building_sources: Vec<(usize, usize, [Option<u32>; 3])> = live_src
            .iter()
            .enumerate()
            .map(|(bidx, &inst)| (bidx, inst, [None; 3]))
            .collect();
        let vehicle_pack = swbf2.map_admission.vehicle_pack.as_ref();
        let BuiltSpaceObjects {
            buildings,
            doors,
            ..
        } = build_space_attachments_and_doors(
            &swbf2.map_admission.placed,
            &building_sources,
            swbf2.map_admission.map_classes.as_ref(),
            vehicle_pack,
            &[],
            &animated_by_instance,
            collision,
        );
        let turret_buildings: Vec<usize> = buildings
            .iter()
            .filter(|b| b.turret.is_some())
            .map(|b| b.building_index)
            .collect();

        let profiles = parse_battle_profiles(battle, collision, |_| {});
        let (pads, fighter_pad_indices) = parse_vehicle_pads(
            &swbf2.map_admission.placed,
            &profiles,
            collision,
            collision_world,
        );
        let fighter_slots = fighter_pad_indices.len();
        let weapon_hashes = crate::wire::space::weapon_hash_table(&profiles, &buildings);
        Some(Self {
            bounds,
            world_anims,
            profiles,
            pads,
            fighter_pad_indices,
            fighter_slots,
            buildings,
            turret_buildings,
            doors,
            weapon_hashes,
        })
    }
}

/// Advances a steered rocket's leaving timer: it starts at
/// GUIDED_OUT_OF_BOUNDS_TIME when the rocket is outside, counts down while it
/// stays there and goes back to -1 inside. True once it has run out.
fn guided_out_of_bounds_step(timer: &mut f64, outside: bool, dt: f64) -> bool {
    if !outside {
        *timer = -1.0;
        return false;
    }
    if !(*timer >= 0.0) {
        *timer = bounds::GUIDED_OUT_OF_BOUNDS_TIME;
    }
    *timer -= dt;
    *timer < 0.0
}

impl SpaceBattlefield {
    /// Returns true ONLY for 2005 (`GameFamily::Swbf2`) void-terrain space
    /// maps (`!content.collision.has_terrain`). Always false for 2004 battles
    /// and for 2005 ground maps.
    pub fn should_attach(content: &Content) -> bool {
        content.battle.family == bfe_content::GameFamily::Swbf2 && !content.collision.has_terrain
    }

    /// Constructs the 2005 space battlefield from admitted content and kernel
    /// body receipts produced during `Stage::Kernels` / `Stage::Fleet` / `Stage::Units`.
    pub(crate) fn build(world: &mut World, content: &Content, cx: &Cx, door_setup: door_animation::DoorSetup) -> Self {
        let collision_world = cx
            .ids
            .kernel
            .as_ref()
            .map(|k| k.world as usize)
            .unwrap_or(0);
        let bounds = build_space_bounds(&content.battle);

        // Bind animated prop instances (orbiting frigates, hierarchy children,
        // and spinning props) to their moving collision bodies.
        let (world_anims, animated_by_instance) = build_space_world_anims(
            &content.battle,
            &cx.ids.prop_kernels,
            &cx.ids.prop_local_bounds,
        );

        // Map every canonical `f3_props` building index (0..N) to its source
        // instance, canonical F4Handle, and collision kernel bodies.
        let building_sources: Vec<(usize, usize, [Option<u32>; 3])> =
            if f3::active_in(&world.units) {
                let live_src = crate::world::admit::rows::props::live_sources(&content.battle);
                let count = f3_props::count_in(&world.units).min(live_src.len());
                (0..count)
                    .map(|bidx| {
                        let kernels = f3_props::building_kernel_bodies_in(world, bidx);
                        (bidx, live_src[bidx], kernels)
                    })
                    .collect()
            } else {
                Vec::new()
            };

        let vehicle_pack = content
            .battle
            .swbf2
            .as_ref()
            .and_then(|s| s.map_admission.vehicle_pack.as_ref());

        let BuiltSpaceObjects {
            mut buildings,
            building_name_index,
            initial_shields,
            mut doors,
            door_name_index,
        } = if let Some(swbf2) = content.battle.swbf2.as_ref() {
            build_space_attachments_and_doors(
                &swbf2.map_admission.placed,
                &building_sources,
                swbf2.map_admission.map_classes.as_ref(),
                vehicle_pack,
                &cx.ids.prop_kernels,
                &animated_by_instance,
                &world.collision,
            )
        } else {
            BuiltSpaceObjects::default()
        };

        for building in &mut buildings {
            building.last_life = Some(f3_props::owner_in(world, building.building_index).life);
            building.sphere = f3_props::model_sphere_in(&world.units, building.building_index);
        }
        door_animation::attach(&mut doors, door_setup.bindings);
        let passage_barriers = content.battle.swbf2.as_ref().map_or_else(Vec::new,
            |data| passages::candidates(&data.map_admission, &doors));
        for (index, shields) in initial_shields { f3_props::install_shields(&mut world.units, index, shields); }

        // Parse 2005 starfighter classes (`flyer` / `commandflyer`) from loaded
        // sides and attach real `vehicles.json` collision trees & fire points.
        let mut repair_classes = Vec::new();
        let profiles = parse_battle_profiles(&content.battle, &mut world.collision, |class_name| {
            repair_classes.push(world.sim.strings.intern_str(class_name));
        });

        // Parse hangar vehicle spawner pads (`base == "vehiclespawn"`) and
        // pre-allocate fighter slots up to each pad's `SpawnCount` cap.
        let (pads, _) = content.battle.swbf2.as_ref().map_or_else(
            || (Vec::new(), Vec::new()),
            |swbf2| {
                parse_vehicle_pads(
                    &swbf2.map_admission.placed,
                    &profiles,
                    &mut world.collision,
                    collision_world,
                )
            },
        );
        let mut fighters = Vec::new();
        let mut added_bodies: Vec<u32> = Vec::new();
        let mut serial = world.collision.state.bodies.len() as f64;
        for (pad_index, pad) in pads.iter().enumerate() {
            let profile = &profiles[pad.profile_index];
            for slot in 0..pad.spawn_count {
                let kernel_body = profile.collision_tree.and_then(|tree| {
                    let b = world.collision.body_new(tree, 1);
                    if b < 0 {
                        return None;
                    }
                    let b = b as u32;
                    serial += 1.0;
                    world.collision.body_offset(b, serial * 16777216.0);
                    added_bodies.push(b);
                    Some(b)
                });
                let id = fighters.len();
                let mut f = SpaceFighter::spawned_on_pad(
                    id,
                    1,
                    pad_index,
                    pad,
                    pad.profile_index,
                    profile,
                    pad.floor_y,
                    None,
                    kernel_body,
                );
                if slot > 0 {
                    f.alive = false;
                    f.health = 0.0;
                    f.mobile_spawn_active = false;
                }
                fighters.push(f);
            }
        }
        if !added_bodies.is_empty() {
            if let Some(w) = world.collision.state.worlds.get_mut(collision_world) {
                w.order.extend_from_slice(&added_bodies);
                w.version += 1;
                // Size the cast index for turret rays against the admitted hulls.
                crate::collision::broad::prepare(&mut world.collision.state, collision_world);
            }
        }
        for f in &fighters {
            f.sync_collision_body(
                &profiles[f.profile_index],
                &mut world.collision,
                collision_world,
                world.movement.flags_mut(),
            );
        }
        name_bodies(world, &fighters);

        // Build O(1) collision body -> owner lookup tables (zero allocations
        // during projectile raycasts).
        let total_bodies = world.collision.state.bodies.len();
        let mut body_to_building = vec![-1i32; total_bodies];
        for att in &buildings {
            for b in att.bodies.into_iter().flatten() {
                if let Some(slot) = body_to_building.get_mut(b as usize) {
                    *slot = att.building_index as i32;
                }
            }
        }
        let mut body_to_fighter = vec![-1i32; total_bodies];
        for (fidx, f) in fighters.iter().enumerate() {
            if let Some(b) = f.kernel_body {
                if let Some(slot) = body_to_fighter.get_mut(b as usize) {
                    *slot = fidx as i32;
                }
            }
        }

        let max_bidx = buildings
            .iter()
            .map(|b| b.building_index + 1)
            .max()
            .unwrap_or(0);
        let mut building_slot_by_index = vec![None; max_bidx];
        for (slot, att) in buildings.iter().enumerate() {
            building_slot_by_index[att.building_index] = Some(slot);
        }

        // Each admitted round and each newly destroyed building can detonate once per drain.
        let blast_capacity = MAX_SPACE_PROJECTILES + buildings.len();
        let lock_cap = fighters.len() + buildings.len() + 16;
        let rng_seed = Wide::from_u32(cx.options.seed).xor(Wide::new(0x51a6_c0de, 0xf00d_cafe));
        let construction = cx.ids.battle.as_ref().and_then(|b| b.construction_limit).unwrap_or(0) as usize;
        let pilots = pilots::PilotBots::build(&bounds, &pads, &fighters, &profiles, &content.battle.world,
            pilots::PROVISIONAL_UNIT_SLOTS.max(construction * 2), buildings.len());
        let (craft_spheres, gun_spheres) = (vec![Default::default(); fighters.len()], vec![Default::default(); buildings.len()]);
        let seat_spheres = vec![Default::default(); fighters.len()];
        Self {
            bounds,
            world_anims,
            profiles,
            pads,
            fighters,
            buildings,
            doors,
            ordnance: SpaceOrdnancePool::new(MAX_SPACE_PROJECTILES),
            humans: Vec::with_capacity(local::MAX_SPACE_HUMANS),
            developer: None,
            events: Vec::with_capacity(MAX_SPACE_EVENTS),
            events_overflowed: false,
            dropped_events: 0,
            collision_world,
            tick: 0,
            elapsed_s: 0.0,
            rng: SpaceRng::new(rng_seed),
            building_name_index,
            building_slot_by_index,
            door_name_index,
            passage_barriers,
            navigation_toggle: None,
            planning_connections: door_setup.connections,
            planning_toggle: None,
            body_to_building,
            body_to_fighter,
            lock_candidates: Vec::with_capacity(lock_cap),
            repair_classes,
            soldier_cache: Vec::with_capacity(MIN_SOLDIER_TARGET_SLOTS.max(
                cx.ids.battle.as_ref().and_then(|b| b.construction_limit).unwrap_or(0) as usize
            )),
            soldier_targets_truncated: false,
            blast_queue: Vec::with_capacity(blast_capacity),
            pending_hits: Vec::with_capacity(MAX_PENDING_HITS),
            pilots,
            impacts: Vec::with_capacity(SPACE_IMPACT_CAPACITY),
            impacts_dropped: 0,
            shown_hulls: HullMeets::Shots,
            hidden_dead_crew: Vec::with_capacity(64),
            craft_spheres,
            gun_spheres,
            seat_spheres,
        }
    }

    /// Constructs a synthetic `SpaceBattlefield` for isolated integration tests.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        bounds: SpaceBounds,
        world_anims: SpaceWorldAnims,
        profiles: Vec<SpaceFlightProfile>,
        pads: Vec<SpaceVehiclePad>,
        fighters: Vec<SpaceFighter>,
        buildings: Vec<SpaceBuildingAttachment>,
        doors: Vec<SpaceDoor>,
        collision_world: usize,
        body_count: usize,
    ) -> Self {
        let building_name_index = objects::build_name_index_from(
            buildings
                .iter()
                .map(|a| (a.building_index, a.name.as_str())),
        );
        let max_bidx = buildings
            .iter()
            .map(|b| b.building_index + 1)
            .max()
            .unwrap_or(0);
        let mut building_slot_by_index = vec![None; max_bidx];
        for (slot, att) in buildings.iter().enumerate() {
            building_slot_by_index[att.building_index] = Some(slot);
        }
        let door_name_index = objects::build_name_index_from(
            doors.iter().enumerate().map(|(i, d)| (i, d.name.as_str())),
        );
        let mut body_to_building = vec![-1i32; body_count];
        for att in &buildings {
            for b in att.bodies.into_iter().flatten() {
                if let Some(slot) = body_to_building.get_mut(b as usize) {
                    *slot = att.building_index as i32;
                }
            }
        }
        let mut body_to_fighter = vec![-1i32; body_count];
        for (fidx, f) in fighters.iter().enumerate() {
            if let Some(b) = f.kernel_body {
                if let Some(slot) = body_to_fighter.get_mut(b as usize) {
                    *slot = fidx as i32;
                }
            }
        }
        // Each admitted round and each newly destroyed building can detonate once per drain.
        let blast_capacity = MAX_SPACE_PROJECTILES + buildings.len();
        let lock_cap = fighters.len() + buildings.len() + 16;
        let (craft_spheres, gun_spheres) = (vec![Default::default(); fighters.len()], vec![Default::default(); buildings.len()]);
        let seat_spheres = vec![Default::default(); fighters.len()];
        Self {
            bounds,
            world_anims,
            profiles,
            pads,
            fighters,
            buildings,
            doors,
            ordnance: SpaceOrdnancePool::new(MAX_SPACE_PROJECTILES),
            humans: Vec::with_capacity(local::MAX_SPACE_HUMANS),
            developer: None,
            events: Vec::with_capacity(MAX_SPACE_EVENTS),
            events_overflowed: false,
            dropped_events: 0,
            collision_world,
            tick: 0,
            elapsed_s: 0.0,
            rng: SpaceRng::new(Wide::new(0x51a6_c0de, 0xf00d_cafe)),
            building_name_index,
            building_slot_by_index,
            door_name_index,
            passage_barriers: Vec::new(),
            navigation_toggle: None,
            planning_connections: Vec::new(),
            planning_toggle: None,
            body_to_building,
            body_to_fighter,
            lock_candidates: Vec::with_capacity(lock_cap),
            repair_classes: Vec::new(),
            soldier_cache: Vec::with_capacity(MIN_SOLDIER_TARGET_SLOTS),
            soldier_targets_truncated: false,
            blast_queue: Vec::with_capacity(blast_capacity),
            pending_hits: Vec::with_capacity(MAX_PENDING_HITS),
            pilots: pilots::PilotBots::default(),
            impacts: Vec::with_capacity(SPACE_IMPACT_CAPACITY),
            impacts_dropped: 0,
            shown_hulls: HullMeets::Shots,
            hidden_dead_crew: Vec::with_capacity(64),
            craft_spheres,
            gun_spheres,
            seat_spheres,
        }
    }

    #[inline]
    fn record_impact(&mut self, impact: SpaceImpact) {
        if self.impacts.len() < self.impacts.capacity() {
            self.impacts.push(impact);
        } else {
            self.impacts_dropped = self.impacts_dropped.saturating_add(1);
        }
    }

    /// The craft or ship object a collision body belongs to.
    pub fn body_owner(&self, body: i32) -> SpaceBodyOwner {
        let Ok(b) = usize::try_from(body) else { return SpaceBodyOwner::Other };
        if let Some(&f) = self.body_to_fighter.get(b) {
            if f >= 0 {
                return SpaceBodyOwner::Fighter(f as usize);
            }
        }
        match self.body_to_building.get(b) {
            Some(&i) if i >= 0 => SpaceBodyOwner::Building(i as usize),
            _ => SpaceBodyOwner::Other,
        }
    }

    #[inline]
    pub fn push_event(&mut self, event: SpaceEvent) {
        if let Some(obs) = &mut self.ordnance.observations {
            if obs.wire_enabled {
                match event {
                    SpaceEvent::FighterDestroyed { fighter, .. } => {
                        if let Some(f) = self.fighters.get(fighter) {
                            obs.record_death(false, f.profile_index as u16, f.position);
                        }
                    }
                    SpaceEvent::BuildingDestroyed { building, .. } => {
                        if let Some(att) = self
                            .building_slot_by_index
                            .get(building)
                            .copied()
                            .flatten()
                            .and_then(|slot| self.buildings.get(slot))
                        {
                            obs.record_death(true, building as u16, att.center);
                        }
                    }
                    _ => {}
                }
            }
        }
        if self.events.len() < self.events.capacity() {
            self.events.push(event);
        } else {
            self.events_overflowed = true;
            self.dropped_events = self.dropped_events.saturating_add(1);
        }
    }

    /// Reports coordinate precision at the farthest reachable battlefield point.
    pub fn precision_report(&self) -> SpacePrecisionReport {
        self.bounds.precision_report()
    }

    /// Returns all events emitted during the most recent tick.
    pub fn events(&self) -> &[SpaceEvent] {
        &self.events
    }

    /// Looks up a canonical `f3_props` building index by authored script name
    /// (case-insensitive, O(log N), zero allocations).
    pub fn building_by_name(&self, name: &str) -> Option<usize> {
        lookup_by_name(&self.building_name_index, name)
    }

    /// Looks up an indestructible proximity door index by authored script name
    /// (case-insensitive, O(log N), zero allocations).
    pub fn door_by_name(&self, name: &str) -> Option<usize> {
        lookup_by_name(&self.door_name_index, name)
    }

    /// Returns a read-only door snapshot.
    pub fn door_snapshot(&self, door_id: usize) -> Option<SpaceDoorSnapshot> {
        self.doors.get(door_id).map(SpaceDoor::snapshot)
    }

    /// Sets the locked flag on an indestructible proximity door and updates its
    /// collision body immediately.
    pub fn set_door_locked(
        &mut self,
        door_id: usize,
        locked: bool,
        collision: &mut CollisionOwner,
        flags: &mut [u8],
    ) -> bool {
        let Some(door) = self.doors.get_mut(door_id) else {
            return false;
        };
        let prev_open = door.door_open;
        let prev_locked = door.locked;
        door.locked = locked;
        if locked && door.door_open {
            door.door_open = false;
        }
        let open = door.door_open;
        door.sync_collision(collision, self.collision_world, flags);
        if prev_open != open || prev_locked != locked {
            self.push_event(SpaceEvent::DoorChanged {
                door: door_id,
                open,
                locked,
            });
        }
        true
    }

    // ---- Fighter & Boarding Helpers ----

    /// Returns the nearest live parked unoccupied fighter of `team` within
    /// `max_distance` metres of `point`.
    pub fn nearest_parked_fighter(
        &self,
        point: [f64; 3],
        team: i32,
        max_distance: f64,
    ) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        let max_sq = max_distance * max_distance;
        for f in &self.fighters {
            if !f.alive
                || f.state != SpaceFlightState::Landed
                || f.pilot.is_some()
                || f.team != team
            {
                continue;
            }
            let dx = f.position[0] - point[0];
            let dy = f.position[1] - point[1];
            let dz = f.position[2] - point[2];
            let dsq = dx * dx + dy * dy + dz * dz;
            if dsq <= max_sq && best.is_none_or(|(_, b)| dsq < b) {
                best = Some((f.id, dsq));
            }
        }
        best.map(|(id, _)| id)
    }

    /// Plants an infantry boarding timebomb inside a ship interior.
    #[allow(clippy::too_many_arguments)]
    pub fn plant_timebomb(
        &mut self,
        position: [f64; 3],
        owner_unit: Option<SpaceOwner>,
        owner_team: i32,
        owner_is_ai: bool,
        fuse_s: f64,
        damage: f64,
        inner_radius: f64,
        outer_radius: f64,
    ) -> Option<usize> {
        plant_boarding_timebomb(
            &mut self.ordnance,
            position,
            owner_unit,
            owner_team,
            owner_is_ai,
            fuse_s,
            damage,
            inner_radius,
            outer_radius,
        )
    }

    /// Emits an infantry boarding bolt/rocket into the space ordnance pool so
    /// interior boarding troops can engage ship systems with real authored
    /// `BuildingScale` and hangar forcefield blocking.
    #[allow(clippy::too_many_arguments)]
    pub fn fire_infantry_shot(
        &mut self,
        origin: [f64; 3],
        direction: [f64; 3],
        speed: f64,
        damage: f64,
        building_scale: f64,
        shield_scale: f64,
        blast_damage: f64,
        blast_radius: f64,
        owner_unit: Option<SpaceOwner>,
        owner_team: i32,
        owner_is_ai: bool,
    ) -> Option<usize> {
        let len = v8math::hypot3(direction[0], direction[1], direction[2]);
        if len < 1e-6 {
            return None;
        }
        let dir = [direction[0] / len, direction[1] / len, direction[2] / len];
        let scales = [1.0, 1.0, 1.0, 1.0, building_scale];
        self.ordnance.spawn(SpaceProjectile {
            generation: 0, previous_position: origin,
            active: true,
            kind: if blast_radius > 0.0 {
                SpaceOrdnanceKind::BombShell
            } else {
                SpaceOrdnanceKind::Bolt
            },
            owner_unit,
            owner_team,
            owner_is_ai,
            from_infantry: true,
            ignore_body: None,
            pass_through: false,
            emitter: None,
            weapon_index: 0,
            position: origin,
            velocity: [dir[0] * speed, dir[1] * speed, dir[2] * speed],
            age: 0.0,
            life_span: 2.0,
            gravity: 0.0,
            turn_rate: 0.0,
            lock_kind: SpaceLockTargetKind::None,
            lock_target: 0,
            lock_generation: 0,
            is_tracer: false,
            base_damage: damage,
            final_damage: None,
            damage_transition_delay: 0.0,
            damage_transition_period: 0.0,
            scales,
            shield_scale,
            guided_pilot_fighter: None,
            guided_damage: 0.0,
            guided_damage_boost: 0.0,
            guided_boost_threshold: 0.8,
            guided_cruise_speed: 0.0,
            guided_boost_speed: 0.0,
            guided_out_of_bounds: -1.0,
            blast: SpaceExplosionSpec { damage: blast_damage, inner_radius: blast_radius * 0.5, outer_radius: blast_radius, scales, shield_scale },
            expire_blast: SpaceExplosionSpec::NONE,
            guided_blast_boost: SpaceExplosionSpec::NONE,
        })
    }

    // ---- Internal Helpers (World-Integrated Single-Owner Paths) ----

    fn damage_building_world(
        &mut self,
        world: &mut World,
        building_index: usize,
        packet: &SpaceDamagePacket,
        ) -> SpaceDamageOutcome {
        let Some(packet) = packet.credited(world) else { return SpaceDamageOutcome::default(); };
        let Some(att_idx) = self
            .building_slot_by_index
            .get(building_index)
            .copied()
            .flatten()
        else {
            return SpaceDamageOutcome::default();
        };
        let Some(before) = building_core(world, building_index) else {
            return SpaceDamageOutcome::default();
        };
        if !before.alive {
            return SpaceDamageOutcome::default();
        }

        // Unified friendly-fire policy (`team_damage` + `f3::friendly_fire_in`).
        let ff_enabled = f3::active_in(&world.units) && f3::friendly_fire_in(&world.units);
        let team_scale = friendly_fire_scale(
            packet.attacker_team,
            packet.attacker_is_ai,
            before.team,
            false,
            ff_enabled,
        );
        if team_scale <= 0.0 {
            return SpaceDamageOutcome::default();
        }
        let scaled_raw = packet.raw_damage * team_scale;
        let cat_idx = (self.buildings[att_idx].damage_category as usize).min(4);
        let cat_scale = v8math::max(0.0, packet.scales[cat_idx]);
        let hull_raw = scaled_raw * cat_scale;
        let shield_damage = scaled_raw * v8math::max(0.0, packet.shield_scale);
        let prior = f3_props::mission_shields(&world.units, building_index).current;
        let floor = if packet.attacker_is_ai { self.buildings[att_idx].ai_damage_threshold } else { 0.0 };
        let attacker = resolve_attacker(packet.attacker_unit);
        f3::with_services_in(world, |w, services| {
            if let Some(services) = services {
                f3_props::space_hit_in(w, building_index, hull_raw, shield_damage, floor, attacker, services);
            }
        });
        let current = f3_props::mission_shields(&world.units, building_index);
        let shield_loss = v8math::min(prior, v8math::max(0.0, shield_damage));
        let shield_broke = shield_loss > 0.0 && current.current == 0.0;
        let after = building_core(world, building_index);
        let hull_loss = after.map_or(0.0, |a| v8math::max(0.0, before.health - a.health));
        let destroyed = after.is_some_and(|a| before.alive && !a.alive);
        let outcome = SpaceDamageOutcome {
            shield_loss,
            hull_loss,
            shield_broke,
            destroyed,
        };
        if after.is_some() {
            if shield_loss > 0.0 {
                let sh = current;
                self.push_event(SpaceEvent::ShieldChanged {
                    building: building_index,
                    life: f3_props::owner_in(world, building_index).life,
                    cur_shield: sh.current,
                    max_shield: sh.maximum,
                });
            }
            if hull_loss > 0.0 || shield_loss > 0.0 {
                self.push_event(SpaceEvent::BuildingDamaged {
                    building: building_index,
                    life: f3_props::owner_in(world, building_index).life,
                    hull_loss,
                    shield_loss,
                    attacker_unit: packet.attacker_unit,
                });
            }
            if destroyed {
                if let Some(ref mut t) = self.buildings[att_idx].turret {
                    t.remote_operator = None;
                    t.pending_remote_fire = false;
                }
                self.push_event(SpaceEvent::BuildingDestroyed {
                    building: building_index,
                    life: f3_props::owner_in(world, building_index).life,
                    team: before.team,
                    attacker_unit: packet.attacker_unit,
                });
                if let Some(exp) = self.buildings[att_idx].death_explosion {
                    if exp.hurts() {
                        self.blast_queue.push(PendingBlast {
                            position: self.buildings[att_idx].center,
                            blast: exp,
                            attacker_unit: packet.attacker_unit,
                            attacker_team: packet.attacker_team,
                            attacker_is_ai: packet.attacker_is_ai,
                            emitter: Some(combat::EmittingBody::Building { index: building_index, life: f3_props::owner_in(world, building_index).life }),
                            weapon_index: 0,
                            explosion: true,
                        });
                    }
                }
            }
        }
        outcome
    }

    fn enqueue_projectile_blast(&mut self, proj: &SpaceProjectile, blast: SpaceExplosionSpec, pos: [f64; 3]) {
        if blast.hurts() {
            self.blast_queue.push(PendingBlast {
                position: pos,
                blast,
                attacker_unit: proj.owner_unit,
                attacker_team: proj.owner_team,
                attacker_is_ai: proj.owner_is_ai,
                emitter: proj.emitter,
                weapon_index: proj.weapon_index,
                explosion: false,
            });
        }
    }

    /// The packet a blast hands a thing of `category` with a bounding sphere,
    /// if it reaches it and nothing fixed but `body`, its own, shields it: the
    /// 2005 game sees terrain and buildings between the blast and the sphere's
    /// centre, and neither craft nor soldiers.
    fn blast_packet(&self, world: &mut World, pending: &PendingBlast, category: SpaceDamageCategory, (center, radius): ([f64; 3], f64), body: Option<u32>) -> Option<SpaceDamagePacket> {
        let blast = &pending.blast;
        let share = blast.share(pending.position, center, radius);
        if share <= 0.0 || (blast.scales[category as usize] <= 0.0 && blast.shield_scale <= 0.0) {
            return None;
        }
        let body = body.and_then(|kernel| world.fleet.collision.as_ref()?.kernel_reference(kernel as usize));
        if crate::units::native::blast_sight_blocked_in(world, self.collision_world, pending.position, center, body) {
            return None;
        }
        Some(SpaceDamagePacket::from_blast(blast, share, pending.attacker_unit, pending.attacker_team, pending.attacker_is_ai))
    }

    fn space_weapon(
        &self,
        emitter: Option<combat::EmittingBody>,
        weapon_index: u8,
    ) -> Option<crate::units::infantry::contact::Weapon> {
        use crate::units::infantry::contact::Weapon;
        match emitter? {
            combat::EmittingBody::Building { index, .. } => self
                .building_slot_by_index
                .get(index)
                .copied()
                .flatten()
                .and_then(|slot| u32::try_from(slot).ok())
                .map(Weapon::SpaceBuilding),
            combat::EmittingBody::Fighter { index, .. } => self.fighters.get(index).and_then(|f| {
                u32::try_from(f.profile_index).ok().map(|profile| {
                    Weapon::SpaceFighter {
                        profile,
                        weapon: weapon_index,
                    }
                })
            }),
        }
    }

    fn apply_blast_world(&mut self, world: &mut World, blast: PendingBlast) {
        let ff_enabled = f3::active_in(&world.units) && f3::friendly_fire_in(&world.units);
        let weapon = self.space_weapon(blast.emitter, blast.weapon_index);
        for fidx in 0..self.fighters.len() {
            if !self.fighters[fidx].alive {
                continue;
            }
            let sphere = self.fighters[fidx].sphere(&self.profiles[self.fighters[fidx].profile_index]);
            let Some(packet) = self.blast_packet(world, &blast, SpaceDamageCategory::Vehicle, sphere, None) else {
                continue;
            };
            let generation = self.fighters[fidx].generation;
            let outcome = self.damage_fighter_with_weapon(
                fidx,
                &packet,
                ff_enabled,
                weapon,
                world,
            );
            self.record_blast(&blast, SpaceImpactTarget::Fighter { index: fidx, generation }, outcome);
        }
        for soldier in self.soldier_cache.iter().copied() {
            soldier_blast(world, soldier, &blast, ff_enabled, weapon);
        }
        for att_idx in 0..self.buildings.len() {
            let bidx = self.buildings[att_idx].building_index;
            let Some(facts) = building_core(world, bidx) else {
                continue;
            };
            if !facts.alive || facts.max_health <= 0.0 {
                continue;
            }
            let att = &self.buildings[att_idx];
            let Some(packet) = self.blast_packet(world, &blast, att.damage_category, att.blast_sphere(), att.active_body(&facts)) else {
                continue;
            };
            let outcome = self.damage_building_world(world, bidx, &packet);
            self.record_blast(&blast, SpaceImpactTarget::Building { index: bidx }, outcome);
        }
    }

    /// Splash that did something is an impact; splash a target shrugged off is not.
    fn record_blast(&mut self, blast: &PendingBlast, target: SpaceImpactTarget, outcome: SpaceDamageOutcome) {
        if outcome.hull_loss > 0.0 || outcome.shield_loss > 0.0 || outcome.destroyed {
            self.record_impact(SpaceImpact {
                cause: if blast.explosion { SpaceImpactCause::Explosion } else { SpaceImpactCause::Splash },
                emitter: blast.emitter,
                team: blast.attacker_team,
                target,
                hull_loss: outcome.hull_loss,
                shield_loss: outcome.shield_loss,
                destroyed: outcome.destroyed,
            });
        }
    }

    fn lose_destroyed_crew(
        &mut self,
        world: &mut World,
        index: usize,
        attacker: Attacker,
        weapon: Option<crate::units::infantry::contact::Weapon>,
    ) {
        let profile_index = self.fighters[index].profile_index;
        let f = &mut self.fighters[index];
        let generation = f.generation;
        let vacuum = flight::PROVISIONAL_SPACE_EJECT_KILLS_PILOT && f.state != SpaceFlightState::Landed
            && self.bounds.landing_zone_at(f.position).is_none();
        let crew = [
            f.pilot.take(),
            f.gunners[0].take(),
            f.gunners[1].take(),
            f.gunners[2].take(),
            f.gunners[3].take(),
            f.gunners[4].take(),
        ];
        for (seat, occupant) in crew.into_iter().enumerate() {
            let Some(occupant) = occupant else { continue };
            let exposed = self.profiles[profile_index].pilot_exposed(seat as u8);
            let victim = living_occupant(world, occupant.owner);
            kill_occupant(world, occupant, attacker, weapon);
            if !exposed {
                if let Some(unit) = victim {
                    // The released rule keeps a destroyed crew out of sight in
                    // every replicated snapshot; the list below is the space
                    // frame's own, for drawing.
                    if f3::active_in(&world.units) {
                        f3::hide_destroyed_crew_in(world, unit);
                    }
                    if let Some(u) = world.sim.store.unit.get(unit) {
                        let key = (unit.slot, u.generation as u32);
                        if !self.hidden_dead_crew.contains(&key) {
                            if self.hidden_dead_crew.len() == self.hidden_dead_crew.capacity() {
                                self.hidden_dead_crew.remove(0);
                            }
                            self.hidden_dead_crew.push(key);
                        }
                    }
                }
            }
            if vacuum {
                self.push_event(SpaceEvent::PilotEjected {
                    fighter: index, generation, pilot_unit: occupant.owner, killed_in_vacuum: true,
                });
            }
        }
    }

    /// Forgets destroyed cockpit crew once their body is gone or their slot
    /// holds another life.
    fn prune_hidden_dead_crew(&mut self, world: &World) {
        self.hidden_dead_crew.retain(|&(slot, life)| {
            world.sim.store.unit.reference(slot).and_then(|r| world.sim.store.unit.get(r)).is_some_and(|u| {
                !u.removed && u.generation as u32 == life && (u.dead || u.combat.dead)
            })
        });
    }

    /// Writes `(unit_slot, generation)` for every soldier currently concealed
    /// inside an enclosed space cockpit (plus destroyed cockpit crew awaiting despawn).
    pub fn hidden_crew_units(&self, world: &World, out: &mut Vec<(u32, u32)>) {
        out.clear();
        for f in self.fighters.iter().filter(|f| f.alive && f.occupied()) {
            let profile = &self.profiles[f.profile_index];
            for (seat, crew) in std::iter::once(f.pilot).chain(f.gunners).enumerate() {
                if profile.pilot_exposed(seat as u8) {
                    continue;
                }
                if let Some(p) = crew {
                    if let Some(snap) = world.sim.store.owner_snapshot.get(p.owner) {
                        out.push((snap.source_unit.slot, snap.generation as u32));
                    }
                }
            }
        }
        for &(slot, gen_id) in &self.hidden_dead_crew {
            if let Some(r) = world.sim.store.unit.reference(slot) {
                if world
                    .sim
                    .store
                    .unit
                    .get(r)
                    .is_some_and(|u| !u.removed && u.generation as u32 == gen_id && (u.dead || u.combat.dead))
                    && !out.contains(&(slot, gen_id))
                {
                    out.push((slot, gen_id));
                }
            }
        }
    }

    /// Applies damage to starfighter `fighter_id` obeying unified friendly-fire
    /// policy and emits `FighterDestroyed` on hull loss to zero.
    pub fn damage_fighter_inner(
        &mut self,
        fighter_id: usize,
        packet: &SpaceDamagePacket,
        friendly_fire: bool,
        world: &mut World,
    ) -> SpaceDamageOutcome {
        self.damage_fighter_with_weapon(fighter_id, packet, friendly_fire, None, world)
    }

    fn damage_fighter_with_weapon(
        &mut self,
        fighter_id: usize,
        packet: &SpaceDamagePacket,
        friendly_fire: bool,
        weapon: Option<crate::units::infantry::contact::Weapon>,
        world: &mut World,
    ) -> SpaceDamageOutcome {
        let Some(packet) = packet.credited(world) else { return SpaceDamageOutcome::default(); };
        let Some(f) = self.fighters.get_mut(fighter_id) else {
            return SpaceDamageOutcome::default();
        };
        if !f.alive {
            return SpaceDamageOutcome::default();
        }
        let target_is_human = f.pilot.is_some_and(|p| p.is_human);
        let team_scale = friendly_fire_scale(
            packet.attacker_team,
            packet.attacker_is_ai,
            f.team,
            target_is_human,
            friendly_fire,
        );
        if team_scale <= 0.0 {
            return SpaceDamageOutcome::default();
        }
        let scaled = SpaceDamagePacket {
            raw_damage: packet.raw_damage * team_scale,
            ..packet
        };
        let mut zero_shield = 0.0;
        let outcome = apply_shield_and_hull_damage(
            &mut zero_shield,
            &mut f.health,
            f.max_health,
            0.0,
            SpaceDamageCategory::Vehicle,
            &scaled,
        );
        if outcome.destroyed {
            let team = f.team;
            let profile_idx = f.profile_index;
            f.alive = false;
            f.health = 0.0;
            f.velocity = [0.0; 3];
            f.airspeed = 0.0;
            f.boosting = false;
            f.trick = SpaceTrick::None;
            f.mobile_spawn_active = false;
            f.sync_collision_body(
                &self.profiles[profile_idx],
                &mut world.collision,
                self.collision_world,
                world.movement.flags_mut(),
            );
            if self.fighters[fighter_id].occupied() {
                let credit = world.mission_credit(resolve_attacker(packet.attacker_unit));
                if let (Some(credit), Some(events)) = (credit, world.battle.rule_events.as_mut().filter(|q| q.mission_events())) {
                    // A flyer on this fleet uses the ordinary occupied-aircraft award.
                    // Team and character come from the attacker's retained launch life.
                    let kind = crate::battle::rules::awards::Destroyed::OccupiedVehicle {
                        victim_kind: self.profiles[profile_idx].mission_aircraft_kind(),
                        killer_kind: self.fighters.iter().find(|other| other.alive && packet.attacker_unit.is_some_and(|owner|
                            std::iter::once(other.pilot).chain(other.gunners).flatten().any(|p| p.owner == owner)))
                            .map_or(0, |other| self.profiles[other.profile_index].mission_aircraft_kind()),
                    };
                    events.push(crate::battle::rules::events::Event::MissionFlyerKilled {
                        slot: fighter_id, generation: self.fighters[fighter_id].generation, team, kind, credit,
                    });
                }
            }
            self.lose_destroyed_crew(world, fighter_id, resolve_attacker(packet.attacker_unit), weapon);
            self.push_event(SpaceEvent::FighterDestroyed {
                fighter: fighter_id,
                    generation: self.fighters[fighter_id].generation,
                team,
                attacker_unit: packet.attacker_unit,
            });
        }
        outcome
    }

    /// Shows `meets` the bodies of every craft's hull that stop it.
    fn show_hulls(&mut self, collision: &mut CollisionOwner, meets: HullMeets) {
        for f in &self.fighters {
            f.show_hull(&self.profiles[f.profile_index], collision, meets);
        }
        self.shown_hulls = meets;
    }

    /// Authoritative fixed-timestep step inside a live `World` (single-owner
    /// canonical `f3_props` integration, zero per-tick allocations).
    fn step_in_world(&mut self, world: &mut World, dt: f64) {
        self.tick = self.tick.saturating_add(1);
        if let Some(obs) = &mut self.ordnance.observations { obs.advance_tick(self.tick); }
        self.elapsed_s = (self.tick as f64) * SPACE_FIXED_DT;
        self.events.clear();
        self.events_overflowed = false;
        self.dropped_events = 0;
        self.impacts.clear();
        self.impacts_dropped = 0;

        let f3_active = f3::active_in(&world.units);
        let ff_enabled = f3_active && f3::friendly_fire_in(&world.units);

        // 2. Advance animated world geometry (orbiting frigates + children) on
        //    integer tick count and mirror live matrices onto canonical `f3_props`.
        self.world_anims.step(dt, &mut world.collision.state);
        for att in &mut self.buildings {
            if let Some(aidx) = att.animated_index {
                if let Some(anim_obj) = self.world_anims.objects.get(aidx) {
                    att.update_from_matrix(anim_obj.live_matrix);
                    if f3_active {
                        f3_props::set_building_matrix_in(
                            world,
                            att.building_index,
                            anim_obj.live_matrix,
                        );
                    }
                }
            }
        }

        // 3. Detect canonical `f3_props` respawns (driven by `f4_live` or repair).
        if f3_active {
            for att_idx in 0..self.buildings.len() {
                let bidx = self.buildings[att_idx].building_index;
                let Some(facts) = building_core(world, bidx) else {
                    continue;
                };
                let life = f3_props::owner_in(world, bidx).life;
                let last = self.buildings[att_idx].last_life;
                if last.is_some_and(|seen| seen != life) && facts.alive {
                    f3_props::restore_shield_after_rebuild(&mut world.units, bidx);
                    self.push_event(SpaceEvent::BuildingRespawned {
                        building: bidx,
                    life: f3_props::owner_in(world, bidx).life,
                        team: facts.team,
                    });
                }
                if !facts.alive {
                    if let Some(ref mut chair) = self.buildings[att_idx].terminal { chair.occupant = None; }
                    if let Some(ref mut t) = self.buildings[att_idx].turret {
                        t.remote_operator = None;
                        t.pending_remote_fire = false;
                    }
                }
                self.buildings[att_idx].last_life = Some(life);
            }
        }

        // 4. Step indestructible interior proximity doors (no health pool).
        for didx in 0..self.doors.len() {
            let mut changed: Option<(bool, bool)> = None;
            {
                let door = &mut self.doors[didx];
                let was_blocked = door.passage_blocked();
                door.previous_fraction = door.open_fraction;
                door.prev_door_open = door.door_open;
                let want_open = if door.locked {
                    false
                } else {
                    let r2 = door.door_trigger_radius * door.door_trigger_radius;
                    self.soldier_cache.iter().any(|s| {
                        let dx = s.position[0] - door.trigger_position[0];
                        let dy = s.position[1] - door.trigger_position[1];
                        let dz = s.position[2] - door.trigger_position[2];
                        dx * dx + dy * dy + dz * dz <= r2
                    })
                };
                let duration = door.opening_clip.as_ref().map_or(objects::PROVISIONAL_DOOR_TRAVEL_SECONDS, |clip|clip.duration);
                let delta = if duration > 0. { dt / duration } else { 1. };
                door.open_fraction = if want_open { v8math::min(1.0, door.open_fraction + delta) } else { v8math::max(0.0, door.open_fraction - delta) };
                if was_blocked != door.passage_blocked() {
                    door.sync_collision(
                        &mut world.collision,
                        self.collision_world,
                        world.movement.flags_mut(),
                    );
                }
                if want_open != door.door_open {
                    door.door_open = want_open;
                    changed = Some((door.door_open, door.locked));
                }
            }
            if let Some((open, locked)) = changed {
                self.push_event(SpaceEvent::DoorChanged {
                    door: didx,
                    open,
                    locked,
                });
            }
        }

        // 5. Populate pre-allocated lock-on candidate list from live fighters
        //    and canonical `f3_props` buildings with `lock_on_enabled == true`.
        self.lock_candidates.clear();
        for f in &self.fighters {
            if !f.alive || self.lock_candidates.len() >= self.lock_candidates.capacity() {
                continue;
            }
            self.lock_candidates.push(LockCandidate {
                kind: SpaceLockTargetKind::Fighter,
                index: f.id,
                generation: u64::from(f.generation),
                team: f.team,
                alive: f.alive,
                evading: matches!(f.trick, SpaceTrick::BarrelLeft | SpaceTrick::BarrelRight),
                position: f.position,
                kernel_body: f.kernel_body,
            });
        }
        if f3_active {
            for att in &self.buildings {
                if self.lock_candidates.len() >= self.lock_candidates.capacity() {
                    break;
                }
                let Some(facts) = building_core(world, att.building_index) else {
                    continue;
                };
                if !facts.alive || !att.lock_on_enabled || facts.team == 0 {
                    continue;
                }
                self.lock_candidates.push(LockCandidate {
                    kind: SpaceLockTargetKind::Object,
                    index: att.building_index,
                    generation: f3_props::owner_in(world,att.building_index).life.to_bits(),
                    team: facts.team,
                    alive: facts.alive,
                    evading: false,
                    position: att.center,
                    kernel_body: att.active_body(&facts),
                });
            }
        }

        // 6. Step starfighters (flight physics, wingtip sweeps, per-weapon
        //    lock-on, spread/barrage/auto-aim firing).
        for idx in 0..self.fighters.len() {
            let profile_idx = self.fighters[idx].profile_index;
            let pad_idx = self.fighters[idx].pad_index;
            let (contacts_before, health_before) = (self.fighters[idx].contacts, self.fighters[idx].health);
            // Its hull sweeps meet the other craft's bodies for craft.
            let sweeps = self.fighters[idx].alive && (self.fighters[idx].state != SpaceFlightState::Landed || self.fighters[idx].pilot.is_some());
            if sweeps {
                self.show_hulls(&mut world.collision, HullMeets::Craft);
            }
            let notice = self.fighters[idx].step_tick(
                dt,
                &self.profiles[profile_idx],
                &self.pads[pad_idx],
                &self.bounds,
                &mut world.collision,
                self.collision_world,
                world.movement.flags_mut(),
            );
            if sweeps {
                self.show_hulls(&mut world.collision, HullMeets::Shots);
            }
            if notice.took_off {
                self.push_event(SpaceEvent::FighterTakeoff {
                    fighter: idx,
                    generation: self.fighters[idx].generation,
                    pilot_unit: self.fighters[idx].pilot.map(|p| p.owner),
                });
            }
            if let Some(zone) = notice.landed_zone {
                self.push_event(SpaceEvent::FighterLanded {
                    fighter: idx,
                    generation: self.fighters[idx].generation,
                    pilot_unit: self.fighters[idx].pilot.map(|p| p.owner),
                    zone,
                });
            }
            if let Some(pilot) = notice.dismounted_pilot {
                if let Some(r) = crate::combat::owners::resolve(&world.sim.store, pilot.owner) {
                    let exit = self.fighters[idx].dismount_position(&self.profiles[profile_idx]);
                    if let Some(u) = world.sim.store.unit.get_mut(r) {
                        if let UnitBody::Soldier(body) = &mut u.soldier { place_rider(body, exit, [0.0;3]); }
                    }
                }
                self.push_event(SpaceEvent::PilotEjected {
                    fighter: idx,
                    generation: self.fighters[idx].generation,
                    pilot_unit: pilot.owner,
                    killed_in_vacuum: false,
                });
            }
            if notice.destroyed {
                let f = &self.fighters[idx];
                let cause = if f.contacts != contacts_before { SpaceImpactCause::Contact { body: f.contact_body } } else { SpaceImpactCause::Flight };
                let impact = SpaceImpact {
                    cause,
                    emitter: None,
                    team: f.team,
                    target: SpaceImpactTarget::Fighter { index: idx, generation: f.generation },
                    hull_loss: health_before,
                    shield_loss: 0.0,
                    destroyed: true,
                };
                self.record_impact(impact);
                self.lose_destroyed_crew(world, idx, Attacker::Null, None);
                self.push_event(SpaceEvent::FighterDestroyed {
                    fighter: idx,
                    generation: self.fighters[idx].generation,
                    team: self.fighters[idx].team,
                    attacker_unit: None,
                });
                continue;
            }

            if self.fighters[idx].alive
                && self.fighters[idx].state == SpaceFlightState::Flying
                && self.fighters[idx].occupied()
            {
                let pos = self.fighters[idx].position;
                let team = self.fighters[idx].team;
                let body = self.fighters[idx].kernel_body;
                for widx in 0..self.profiles[profile_idx].weapons.len() {
                    let specs = &self.profiles[profile_idx].weapons;
                    let spec = &specs[widx];
                    let seat = spec.seat;
                    let input = self.fighters[idx].seat_input(seat);
                    let (selected, held) = self.fighters[idx].weapon_trigger(specs, widx);
                    let fwd = if seat == 0 { self.fighters[idx].forward() }
                        else { input.aim.unwrap_or_else(|| self.fighters[idx].forward()) };
                    let prev = self.fighters[idx].weapon_locks[widx];
                    if spec.lock_on_range > 0.0 && selected {
                        update_fighter_lock_on(&mut self.fighters[idx].weapon_locks[widx], widx as u8,
                            pos, fwd, team, body, spec, dt, held || input.lock_target, &self.lock_candidates,
                            &mut world.collision, self.collision_world);
                    } else { self.fighters[idx].weapon_locks[widx] = Default::default(); }
                    let current = self.fighters[idx].weapon_locks[widx];
                    if selected && seat == 0 && (input.weapon_index.is_some() || spec.lock_on_range > 0.0) {
                        self.fighters[idx].lock = current;
                    }
                    if current.locked != prev.locked || current.kind != prev.kind || current.target_index != prev.target_index || current.target_generation != prev.target_generation {
                        self.push_event(SpaceEvent::LockChanged {fighter: idx, generation: self.fighters[idx].generation, target_generation: current.target_generation, weapon_index: widx as u8, kind: current.kind,
                            target_index: current.target_index, locked: current.locked});
                    }
                    let unmanned = self.fighters[idx].occupant(seat).is_none();
                    let gun = &mut self.fighters[idx].weapons[widx];
                    if unmanned || !selected || (!held && gun.clock.salvos == 0 && !gun.trigger_was_held) {
                        gun.trigger_was_held = false;
                        gun.clock.idle(dt);
                        continue;
                    }
                    let ray = Ray { ox: pos[0], oy: pos[1], oz: pos[2], dx: fwd[0], dy: fwd[1], dz: fwd[2] };
                    let aim_range = PROVISIONAL_RETICLE_RANGE;
                    let dist = world.collision.ray_hit(self.collision_world as u32, ray, aim_range, body.map_or(-1, |i| i as i32))
                        .filter(|(code, _)| *code != MISS).map_or(aim_range, |(_, hit)| hit.distance);
                    let reticle = std::array::from_fn(|axis| pos[axis] + fwd[axis] * dist);
                    let (emitted, origin, direction) = try_fire_fighter_weapon(&mut self.fighters[idx], &self.profiles[profile_idx],
                        widx, dt, held, input.boost, reticle, &self.lock_candidates, &mut self.rng, &mut self.ordnance);
                    if emitted > 0 {
                        self.push_event(SpaceEvent::WeaponFired {
                            fighter: Some(idx),
                            source_life: f64::from(self.fighters[idx].generation),
                            building: None,
                            weapon_index: widx as u8,
                            origin,
                            direction,
                            count: emitted,
                        });
                    }
                }
            } else {
                for gun in &mut self.fighters[idx].weapons {
                    gun.clock.idle(dt);
                }
            }
        }

        // 7. Step hangar vehicle spawner pads.
        step_vehicle_pads(
            dt,
            &mut self.pads,
            &mut self.fighters,
            &self.profiles,
            &self.bounds,
            &mut world.collision,
            self.collision_world,
            world.movement.flags_mut(),
        );

        // The bots' aim error: every occupied craft's sphere of miss and the
        // direction each gun a bot works strays in, before the guns aim.
        let opposing = |team: i32| {
            world.ai.battle().map_or(aim::RELEASED_MEDIUM_SKILL, |b| b.settings.difficulty.borrow().opposing_skill(f64::from(team)))
        };
        let skills = [opposing(1), opposing(2)];
        let skill_of = |team: i32| if team == 2 { skills[1] } else { skills[0] };
        for (f, sphere) in self.fighters.iter().zip(self.craft_spheres.iter_mut()) {
            aim::step_craft_sphere(sphere, f, &self.profiles[f.profile_index], skill_of(f.team), dt, &mut self.rng);
        }
        for (att, sphere) in self.buildings.iter().zip(self.gun_spheres.iter_mut()) {
            let worked = att.turret.as_ref().is_some_and(|t| match t.pilot_type {
                SpaceTurretPilotType::SelfPiloted => true,
                SpaceTurretPilotType::Remote => t.remote_operator.is_some_and(|o| !o.is_human),
                SpaceTurretPilotType::Player => false,
            }) && building_core(world, att.building_index).is_some_and(|b| b.alive);
            aim::step_gun_sphere(sphere, worked, 0.0, dt, &mut self.rng);
        }
        for (f, seats) in self.fighters.iter().zip(self.seat_spheres.iter_mut()) {
            for (gunner, sphere) in f.gunners.iter().zip(seats.iter_mut()) {
                let worked = f.alive && gunner.is_some_and(|g| !g.is_human);
                aim::step_gun_sphere(sphere, worked, 0.0, dt, &mut self.rng);
            }
        }

        // 8. Step self-piloted auto-turrets (with LOS + velocity lead + warm-up)
        //    and manned remote turrets, reading canonical `f3_props` alive/team.
        if f3_active {
            for att_idx in 0..self.buildings.len() {
                let bidx = self.buildings[att_idx].building_index;
                let Some(facts) = building_core(world, bidx) else {
                    continue;
                };
                if !facts.alive {
                    continue;
                }
                let emitter = Some(combat::EmittingBody::Building { index:bidx, life:f3_props::owner_in(world,bidx).life });
                let Some(pilot_type) = self.buildings[att_idx].turret.as_ref().map(|t| t.pilot_type)
                else {
                    continue;
                };
                let t_pos = self.buildings[att_idx].center;
                let t_matrix = self.buildings[att_idx].matrix;
                let t_team = facts.team;
                let t_body = self.buildings[att_idx].active_body(&facts);
                let muzzle_standoff = self.buildings[att_idx].radius + PROVISIONAL_TURRET_MUZZLE_PAD;

                match pilot_type {
                    SpaceTurretPilotType::SelfPiloted => {
                        if let Some(ref mut t) = self.buildings[att_idx].turret {
                            t.prev_yaw = t.yaw;
                            t.prev_pitch = t.pitch;
                        }
                        if t_team == 0 {
                            continue;
                        }
                        let (filter, lead_speed, bubble) = {
                            let t = self.buildings[att_idx].turret.as_ref().unwrap();
                            (t.filter, t.weapon.round_speed(), t.weapon.ai_bubble)
                        };
                        let min_sq = filter.min_range * filter.min_range;
                        let max_sq = filter.max_range * filter.max_range;
                        struct TurretAimTarget {
                            pos: [f64; 3],
                            vel: [f64; 3],
                            dist_sq: f64,
                            craft: Option<usize>,
                        }
                        let mut best: Option<TurretAimTarget> = None;

                        if filter.target_vehicle {
                            for f in &self.fighters {
                                if !f.alive || f.team == 0 || f.team == t_team {
                                    continue;
                                }
                                let dsq = dist_sq(t_pos, f.position);
                                if dsq >= min_sq
                                    && dsq <= max_sq
                                    && best.as_ref().is_none_or(|b| dsq < b.dist_sq)
                                    && has_turret_los(
                                        t_pos,
                                        f.position,
                                        t_body,
                                        f.kernel_body,
                                        &mut world.collision,
                                        self.collision_world,
                                    )
                                {
                                    best = Some(TurretAimTarget {
                                        pos: f.position,
                                        vel: f.velocity,
                                        dist_sq: dsq,
                                        craft: Some(f.id),
                                    });
                                }
                            }
                        }
                        if filter.target_person {
                            for s in &self.soldier_cache {
                                if s.team == 0 || s.team == t_team {
                                    continue;
                                }
                                let dsq = dist_sq(t_pos, s.position);
                                if dsq >= min_sq
                                    && dsq <= max_sq
                                    && best.as_ref().is_none_or(|b| dsq < b.dist_sq)
                                    && has_turret_los(
                                        t_pos,
                                        s.position,
                                        t_body,
                                        None,
                                        &mut world.collision,
                                        self.collision_world,
                                    )
                                {
                                    best = Some(TurretAimTarget {
                                        pos: s.position,
                                        vel: s.velocity,
                                        dist_sq: dsq,
                                        craft: None,
                                    });
                                }
                            }
                        }
                        if filter.target_building {
                            for other in &self.buildings {
                                if other.building_index == bidx {
                                    continue;
                                }
                                let Some(of) = building_core(world, other.building_index) else {
                                    continue;
                                };
                                if !of.alive || !other.lock_on_enabled || of.team == 0 || of.team == t_team {
                                    continue;
                                }
                                let dsq = dist_sq(t_pos, other.center);
                                let obody = other.active_body(&of);
                                if dsq >= min_sq
                                    && dsq <= max_sq
                                    && best.as_ref().is_none_or(|b| dsq < b.dist_sq)
                                    && has_turret_los(
                                        t_pos,
                                        other.center,
                                        t_body,
                                        obody,
                                        &mut world.collision,
                                        self.collision_world,
                                    )
                                {
                                    best = Some(TurretAimTarget {
                                        pos: other.center,
                                        vel: [0.0; 3],
                                        dist_sq: dsq,
                                        craft: None,
                                    });
                                }
                            }
                        }

                        if let Some(target) = best {
                            let target_pos = target.pos;
                            let target_vel = target.vel;
                            // A craft is aimed at off its lead point by its sphere of miss.
                            let aimed = target.craft.and_then(|i| {
                                let lead = lead_target_point(t_pos, target_pos, target_vel, lead_speed)?;
                                let f = &self.fighters[i];
                                let flying = aim::craft_flying(f);
                                let ceiling = if flying { 0.0 } else { aim::offset_ceiling(&mut self.rng) };
                                let point = aim::miss_point(t_pos, lead, self.gun_spheres[att_idx].dir, self.craft_spheres[i].radius,
                                    aim::human_aboard(f), flying, ceiling, bubble);
                                let d = [point[0] - t_pos[0], point[1] - t_pos[1], point[2] - t_pos[2]];
                                let len = v8math::hypot3(d[0], d[1], d[2]);
                                (len > 1e-6).then(|| [d[0] / len, d[1] / len, d[2] / len])
                            });
                            if let Some(dir) = aimed.or_else(|| lead_target_direction(t_pos, target_pos, target_vel, lead_speed)) {
                                let fallback_muzzle = [
                                    t_pos[0] + dir[0] * muzzle_standoff,
                                    t_pos[1] + dir[1] * muzzle_standoff,
                                    t_pos[2] + dir[2] * muzzle_standoff,
                                ];
                                let fired_info = if let Some(ref mut turret) = self.buildings[att_idx].turret {
                                    let (aligned, fire_dir) =
                                        aim_turret_toward(turret, &t_matrix, dir, dt);
                                    step_turret_fire(
                                        turret,
                                        &t_matrix,
                                        fallback_muzzle,
                                        fire_dir,
                                        t_team,
                                        t_body,
                                        emitter,
                                        None,
                                        true,
                                        aligned,
                                        dt,
                                        &mut self.rng,
                                        &mut self.ordnance,
                                    )
                                } else {
                                    (0, fallback_muzzle, dir)
                                };
                                if fired_info.0 > 0 {
                                    self.push_event(SpaceEvent::WeaponFired {
                                        fighter: None,
                                        building: Some(bidx),
                                    source_life: f3_props::owner_in(world, bidx).life,
                                        weapon_index: 0,
                                        origin: fired_info.1,
                                        direction: fired_info.2,
                                        count: fired_info.0,
                                    });
                                }
                            }
                        } else if let Some(turret) = self.buildings[att_idx].turret.as_mut() {
                            // Its trigger is up: what is left of a shot goes out the way it points.
                            let dir = objects::barrel_direction(turret, &t_matrix);
                            let muzzle = std::array::from_fn(|k| t_pos[k] + dir[k] * muzzle_standoff);
                            let (count, origin, direction) = step_turret_fire(turret, &t_matrix, muzzle, dir, t_team, t_body,
                                emitter, None, true, false, dt, &mut self.rng, &mut self.ordnance);
                            if count > 0 {
                                self.push_event(SpaceEvent::WeaponFired {
                                    fighter: None,
                                    building: Some(bidx),
                                    source_life: f3_props::owner_in(world, bidx).life,
                                    weapon_index: 0,
                                    origin,
                                    direction,
                                    count,
                                });
                            }
                        }
                    }
                    SpaceTurretPilotType::Remote => {
                        let fired_info = if let Some(ref mut turret) = self.buildings[att_idx].turret {
                            turret.prev_yaw = turret.yaw;
                            turret.prev_pitch = turret.pitch;
                            let Some(operator) = turret.remote_operator else {
                                // An empty chair's gun runs on with its trigger up, as the
                                // 2005 game's does: its shot delay and heat go on and its
                                // kick falls. What was left of a salvo is dropped.
                                (turret.clock.salvos, turret.clock.wait) = (0, 0.0);
                                step_turret_fire(turret, &t_matrix, t_pos, [0.0, 0.0, 1.0], t_team, t_body,
                                    emitter, None, true, false, dt, &mut self.rng, &mut self.ordnance);
                                continue;
                            };
                            let aim_dir = turret.pending_remote_aim.unwrap_or([0.0, 0.0, 1.0]);
                            let (_, fire_dir) = aim_turret_toward(turret, &t_matrix, aim_dir, dt);
                            let fallback_muzzle = [
                                t_pos[0] + fire_dir[0] * muzzle_standoff,
                                t_pos[1] + fire_dir[1] * muzzle_standoff,
                                t_pos[2] + fire_dir[2] * muzzle_standoff,
                            ];
                            let trigger = turret.pending_remote_fire;
                            step_turret_fire(
                                turret,
                                &t_matrix,
                                fallback_muzzle,
                                fire_dir,
                                operator.team,
                                t_body,
                                emitter,
                                Some(operator.owner),
                                !operator.is_human,
                                trigger,
                                dt,
                                &mut self.rng,
                                &mut self.ordnance,
                            )
                        } else {
                            (0, t_pos, [0.0, 0.0, 1.0])
                        };
                        if fired_info.0 > 0 {
                            self.push_event(SpaceEvent::WeaponFired {
                                fighter: None,
                                building: Some(bidx),
                                    source_life: f3_props::owner_in(world, bidx).life,
                                weapon_index: 0,
                                origin: fired_info.1,
                                direction: fired_info.2,
                                count: fired_info.0,
                            });
                        }
                    }
                    SpaceTurretPilotType::Player => {}
                }
            }
        }

        // What soldiers' shots did to craft since the last step.
        for i in 0..self.pending_hits.len() {
            let hit = self.pending_hits[i];
            if self.fighters[hit.fighter].generation != hit.generation {
                continue;
            }
            let outcome = self.damage_fighter_inner(hit.fighter, &hit.packet, ff_enabled, world);
            self.record_impact(SpaceImpact {
                cause: SpaceImpactCause::Round { weapon: 0 },
                emitter: None,
                team: hit.packet.attacker_team,
                target: SpaceImpactTarget::Fighter { index: hit.fighter, generation: hit.generation },
                hull_loss: outcome.hull_loss,
                shield_loss: outcome.shield_loss,
                destroyed: outcome.destroyed,
            });
        }
        self.pending_hits.clear();

        // 9. Step active ordnance (guided rocket pilot steering, homing
        //    missiles, beams, dynamic impact-time falloff, forcefield blocking
        //    for infantry shots, and boarding timebombs).
        for slot_idx in 0..self.ordnance.slots.len() {
            if !self.ordnance.slots[slot_idx].active {
                continue;
            }
            let mut proj = self.ordnance.slots[slot_idx];
            proj.previous_position = proj.position;
            // The last step is as long as the life left.
            let flown = v8math::min(dt, v8math::max(0.0, proj.life_span - proj.age));
            proj.age += dt;

            // Boarding timebombs sit in place and detonate on fuse expiry.
            if proj.kind == SpaceOrdnanceKind::TimeBomb {
                if proj.age >= proj.life_span {
                    proj.active = false;
                    self.ordnance.slots[slot_idx] = proj;
                    self.enqueue_projectile_blast(&proj, proj.end_blast(false), proj.position);
                } else {
                    self.ordnance.slots[slot_idx] = proj;
                }
                continue;
            }

            // Live pilot steering for active guided rockets (`GuidedMissile = 1`).
            // One whose pilot has left or died goes up where it is.
            if proj.kind == SpaceOrdnanceKind::GuidedRocket {
                if let Some((fidx, generation, seat)) = proj.guided_pilot_fighter {
                    let pilot = self.fighters.get(fidx).filter(|f| f.alive && f.generation == generation && f.occupant(seat).is_some_and(|p| Some(p.owner) == proj.owner_unit));
                    if pilot.is_none() {
                        proj.active = false;
                        self.ordnance.slots[slot_idx] = proj;
                        self.enqueue_projectile_blast(&proj, proj.end_blast(false), proj.position);
                        continue;
                    }
                    if let Some(f) = pilot {
                        let inp = f.seat_input(seat);
                        let turn = if proj.turn_rate > 0.0 {
                            proj.turn_rate
                        } else {
                            PROVISIONAL_GUIDED_TURN_RATE
                        };
                        steer_guided_rocket(
                            &mut proj.velocity,
                            inp.stick_x,
                            inp.stick_y,
                            inp.boost,
                            proj.guided_cruise_speed,
                            proj.guided_boost_speed,
                            turn,
                            dt,
                        );
                    }
                }
            }

            // Homing missiles / lock-guided ordnance steer toward locked target.
            if proj.lock_kind != SpaceLockTargetKind::None && proj.turn_rate > 0.0 {
                let live_target = match proj.lock_kind {
                    SpaceLockTargetKind::Fighter => self.fighters.get(proj.lock_target).and_then(|f| {
                        let evading =
                            matches!(f.trick, SpaceTrick::BarrelLeft | SpaceTrick::BarrelRight);
                        (f.alive && u64::from(f.generation) == proj.lock_generation && !evading)
                            .then_some(f.position)
                    }),
                    SpaceLockTargetKind::Object => {
                        if f3_active {
                            self.building_slot_by_index
                                .get(proj.lock_target)
                                .copied()
                                .flatten()
                                .and_then(|slot| {
                                    let att = &self.buildings[slot];
                                    let facts = building_core(world, att.building_index)?;
                                    (facts.alive && f3_props::owner_in(world,att.building_index).life.to_bits() == proj.lock_generation)
                                        .then_some(att.center)
                                })
                        } else {
                            None
                        }
                    }
                    SpaceLockTargetKind::None => None,
                };
                if let Some(target_pos) = live_target {
                    steer_homing_velocity(
                        proj.position,
                        &mut proj.velocity,
                        target_pos,
                        proj.turn_rate,
                        dt,
                    );
                } else {
                    proj.lock_kind = SpaceLockTargetKind::None;
                }
            }

            if proj.gravity != 0.0 && proj.kind != SpaceOrdnanceKind::Beam {
                proj.velocity[1] -= proj.gravity * flown;
            }

            let step_vec = if proj.kind == SpaceOrdnanceKind::Beam {
                proj.velocity
            } else {
                proj.velocity.map(|v| v * flown)
            };
            let step_len = v8math::hypot3(step_vec[0], step_vec[1], step_vec[2]);
            let mut struck = None;
            if step_len > 1e-6 {
                let dir = step_vec.map(|v| v / step_len);
                let same_life = match proj.emitter {
                    Some(combat::EmittingBody::Fighter { index, generation }) =>
                        self.fighters.get(index).is_some_and(|f| f.generation == generation),
                    Some(combat::EmittingBody::Building { index, life }) =>
                        f3_active && f3_props::owner_in(world, index).life == life,
                    None => true,
                };
                let ignore = proj.ignore_body.filter(|_| same_life);
                let mut ignored = [0u32; MAX_PASSED + 1];
                let mut ignored_count = 0usize;
                if let Some(body) = ignore {
                    ignored[ignored_count] = body;
                    ignored_count += 1;
                }
                struck = self.first_struck(world, &proj, proj.position, dir, step_len, &ignored[..ignored_count], &[]);
                // A beam that passes through hurts each craft and soldier on
                // its way and ends at the first thing that is neither.
                if proj.pass_through {
                    let (mut passed, mut count, mut reach) = ([StruckThing::Surface; MAX_PASSED], 0, step_len);
                    for _ in 0..MAX_PASSED {
                        let Some(through) = struck.filter(|s| s.thing.lets_beams_through()) else { break; };
                        if !passed[..count].contains(&through.thing) {
                            self.strike(world, &proj, &through, ff_enabled);
                            passed[count] = through.thing;
                            count += 1;
                        }
                        if let Some(body) = through.body {
                            if !ignored[..ignored_count].contains(&body) && ignored_count < ignored.len() {
                                ignored[ignored_count] = body;
                                ignored_count += 1;
                            }
                        }
                        reach -= through.hit.distance;
                        let behind = [through.hit.px, through.hit.py, through.hit.pz];
                        struck = self.first_struck(world, &proj, behind, dir, reach, &ignored[..ignored_count], &passed[..count]);
                    }
                    struck = struck.filter(|s| !passed[..count].contains(&s.thing));
                }
            }

            let mut hit_pos = None;
            let mut contact = observations::Contact::default();
            if let Some(struck) = struck {
                if self.ordnance.observations.is_some() {
                    let fighter = match struck.thing { StruckThing::Fighter(f) => Some(f), _ => None };
                    let building = match struck.thing { StruckThing::Building(b) => Some(b), _ => None };
                    hit_pos = Some([struck.hit.px, struck.hit.py, struck.hit.pz]);
                    contact = observations::Contact {
                        normal: (struck.body.is_some() || matches!(struck.thing, StruckThing::Surface))
                            .then_some([struck.hit.nx, struck.hit.ny, struck.hit.nz]),
                        shield: matches!(struck.thing, StruckThing::Forcefield)
                            || building.is_some_and(|i| f3_props::mission_shields(&world.units, i).current > 0.),
                        rigid: struck.body.is_some() || fighter.is_some() || building.is_some(),
                        fighter,
                        building,
                    };
                }
                self.strike(world, &proj, &struck, ff_enabled);
                self.enqueue_projectile_blast(&proj, proj.impact_blast(), struck.off_surface(step_vec));
                proj.active = false;
            } else if proj.kind == SpaceOrdnanceKind::Beam {
                proj.active = false;
            } else {
                proj.position[0] += step_vec[0];
                proj.position[1] += step_vec[1];
                proj.position[2] += step_vec[2];
                // A steered rocket outside the battlefield is lost once
                // GUIDED_OUT_OF_BOUNDS_TIME has run out, as a craft is after its own.
                let lost = proj.kind == SpaceOrdnanceKind::GuidedRocket
                    && proj.guided_pilot_fighter.is_some()
                    && guided_out_of_bounds_step(&mut proj.guided_out_of_bounds, self.bounds.check_boundary(proj.position).outside, dt);
                if proj.age >= proj.life_span || lost {
                    proj.active = false;
                    self.enqueue_projectile_blast(&proj, proj.end_blast(true), proj.position);
                }
            }
            if let Some(obs) = &mut self.ordnance.observations { obs.sample(slot_idx, &proj, hit_pos, contact); }
            self.ordnance.slots[slot_idx] = proj;
        }

        // 10. Drain queued radial splash detonations deterministically.
        self.drain_blasts(world);
    }

    /// What a round's cast of `reach` metres meets first, but for the bodies in
    /// `ignore` and what it has `passed` already.
    #[allow(clippy::too_many_arguments)]
    fn first_struck(&self, world: &mut World, proj: &SpaceProjectile, from: [f64; 3], dir: [f64; 3], reach: f64, ignore: &[u32], passed: &[StruckThing]) -> Option<Struck> {
        let at = |d: f64| -> [f64; 3] { std::array::from_fn(|k| from[k] + dir[k] * d) };
        let mut best_dist = reach;
        let mut struck: Option<Struck> = None;

        // 0) Hangar forcefields block infantry shots while flyers pass free.
        if proj.from_infantry {
            if let Some(point) = self.bounds.forcefield_blocks_segment(from, at(reach)) {
                let distance = dist_3d(from, point);
                if distance <= best_dist {
                    best_dist = distance;
                    struck = Some(Struck { hit: plain_hit(distance, point, dir), body: None, thing: StruckThing::Forcefield });
                }
            }
        }

        // 1) Collision world raycast (capital hulls, systems, doors, fighters).
        let ray = Ray { ox: from[0], oy: from[1], oz: from[2], dx: dir[0], dy: dir[1], dz: dir[2] };
        if let Some((code, hit)) = world.collision.ray_hit_ignoring(self.collision_world as u32, ray, best_dist, ignore) {
            if code != MISS && hit.distance <= best_dist {
                best_dist = hit.distance;
                let body = (code >= 0).then_some(code as u32);
                // Indestructible doors absorb the shot cleanly without taking damage.
                let thing = match self.body_owner(code) {
                    SpaceBodyOwner::Fighter(f) => StruckThing::Fighter(f),
                    SpaceBodyOwner::Building(b) => StruckThing::Building(b),
                    SpaceBodyOwner::Other => StruckThing::Surface,
                };
                struck = Some(Struck { hit, body, thing });
            }
        }

        // 2) Bounding-sphere fallback for live fighters and buildings
        //    lacking a collision body, using the unified FF policy.
        for f in &self.fighters {
            let thing = StruckThing::Fighter(f.id);
            if !f.alive || f.kernel_body.is_some() || passed.contains(&thing) {
                continue;
            }
            if let Some(d) = ray_sphere_hit(from, dir, best_dist, f.position, PROVISIONAL_FALLBACK_FIGHTER_HIT_RADIUS) {
                best_dist = d;
                struck = Some(Struck { hit: plain_hit(d, at(d), dir), body: None, thing });
            }
        }
        if f3::active_in(&world.units) {
            for att in &self.buildings {
                let Some(facts) = building_core(world, att.building_index) else {
                    continue;
                };
                if !facts.alive || !facts.collidable
                    || att.active_body(&facts).is_some()
                    || facts.max_health <= 0.0
                {
                    continue;
                }
                if let Some(d) = ray_sphere_hit(from, dir, best_dist, att.center, att.radius) {
                    best_dist = d;
                    struck = Some(Struck { hit: plain_hit(d, at(d), dir), body: None, thing: StruckThing::Building(att.building_index) });
                }
            }
        }
        let shooter = proj.owner_unit.and_then(|owner| crate::combat::owners::resolve(&world.sim.store, owner));
        for soldier in self.soldier_cache.iter().copied() {
            let thing = StruckThing::Soldier(soldier);
            if Some(soldier.unit) == shooter || passed.contains(&thing) { continue; }
            if let Some(d) = ray_sphere_hit(from, dir, best_dist, soldier.center, soldier.radius) {
                best_dist = d;
                struck = Some(Struck { hit: plain_hit(d, at(d), dir), body: None, thing });
            }
        }
        struck
    }

    /// Deals a round's direct hit to what it struck and records it.
    fn strike(&mut self, world: &mut World, proj: &SpaceProjectile, struck: &Struck, ff_enabled: bool) {
        let direct_packet = proj.impact_damage_packet();
        let weapon = self.space_weapon(proj.emitter, proj.weapon_index);
        let (target, outcome) = match struck.thing {
            StruckThing::Soldier(soldier) => {
                soldier_hit(world, soldier, &direct_packet, ff_enabled, weapon);
                (SpaceImpactTarget::Soldier, SpaceDamageOutcome::default())
            }
            StruckThing::Fighter(fidx) => {
                let generation = self.fighters[fidx].generation;
                let outcome = self.damage_fighter_with_weapon(fidx, &direct_packet, ff_enabled, weapon, world);
                (SpaceImpactTarget::Fighter { index: fidx, generation }, outcome)
            }
            StruckThing::Building(bidx) =>
                (SpaceImpactTarget::Building { index: bidx }, self.damage_building_world(world, bidx, &direct_packet)),
            StruckThing::Forcefield | StruckThing::Surface =>
                (SpaceImpactTarget::Surface { body: struck.body.map_or(-1, |b| b as i32) }, SpaceDamageOutcome::default()),
        };
        self.record_impact(SpaceImpact {
            cause: SpaceImpactCause::Round { weapon: proj.weapon_index },
            emitter: proj.emitter,
            team: proj.owner_team,
            target,
            hull_loss: outcome.hull_loss,
            shield_loss: outcome.shield_loss,
            destroyed: outcome.destroyed,
        });
    }

    fn drain_blasts(&mut self, world: &mut World) {
        let mut cursor = 0usize;
        while cursor < self.blast_queue.len() {
            let blast = self.blast_queue[cursor];
            cursor += 1;
            self.apply_blast_world(world, blast);
        }
        self.blast_queue.clear();
    }

    /// Computes a deterministic 64-bit FNV-1a digest of space battlefield state
    /// (tick, RNG, overflow flags, world animations, pads, fighters, buildings
    /// including canonical `f3_props` health/shields, doors, and ordnance).
    pub fn state_digest(&self, world: Option<&World>) -> Wide {
        let mut m = Fnv::start();
        m.small(self.tick);
        m.word(self.rng.state());
        m.small(self.events_overflowed);
        m.small(self.dropped_events);
        m.count(self.bounds.landing_zones.len());
        m.bits(self.bounds.world_extents);
        let mut hash = self.world_anims.mix_digest(m.finish());
        for p in &self.pads {
            let mut m = Fnv::new(hash);
            m.bits(p.respawn_timer);
            hash = m.finish();
        }
        for f in &self.fighters {
            hash = f.mix_digest(hash);
        }
        for att in &self.buildings {
            let facts = world.and_then(|w| {
                f3::active_in(&w.units)
                    .then(|| building_core(w, att.building_index))
                    .flatten()
            });
            let shields = world.filter(|_| facts.is_some()).map_or_else(Default::default, |w| f3_props::mission_shields(&w.units, att.building_index));
            hash = att.mix_digest(facts, shields, hash);
        }
        for door in &self.doors {
            hash = door.mix_digest(hash);
        }
        for barrier in &self.passage_barriers { hash = barrier.digest(hash); }
        for hit in &self.pending_hits {
            let mut m = Fnv::new(hash);
            m.count(hit.fighter);
            m.bits(hit.packet.raw_damage);
            hash = m.finish();
        }
        hash = self.ordnance.mix_digest(hash);
        hash
    }
}

#[inline]
fn dist_sq(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    dx * dx + dy * dy + dz * dz
}

#[inline]
fn dist_3d(a: [f64; 3], b: [f64; 3]) -> f64 {
    v8math::hypot3(a[0] - b[0], a[1] - b[1], a[2] - b[2])
}

#[inline]
fn ray_sphere_hit(
    origin: [f64; 3],
    dir: [f64; 3],
    max_dist: f64,
    center: [f64; 3],
    radius: f64,
) -> Option<f64> {
    let ocx = origin[0] - center[0];
    let ocy = origin[1] - center[1];
    let ocz = origin[2] - center[2];
    let b = ocx * dir[0] + ocy * dir[1] + ocz * dir[2];
    let c = ocx * ocx + ocy * ocy + ocz * ocz - radius * radius;
    if c <= 0.0 {
        return Some(0.0);
    }
    if b > 0.0 {
        return None;
    }
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    (t >= 0.0 && t <= max_dist).then_some(t)
}

/// Steps the world's 2005 space battlefield layer when present, forwarding live
/// soldier positions and velocities (zero allocations) for interior proximity
/// doors and auto-turrets. Completely skipped (`None`) on every 2004 battle.
/// Runs `step` while craft show their hulls' bodies for `meets` (restoring the
/// previous hull view on exit so nested shot casts inside the soldier phase do
/// not clobber it).
pub(crate) fn hulls_meet<T>(world: &mut World, meets: HullMeets, step: impl FnOnce(&mut World) -> T) -> T {
    let prev = world.space.as_deref().map(|space| space.shown_hulls);
    if let Some(space) = world.space.as_deref_mut() {
        if space.shown_hulls != meets {
            space.show_hulls(&mut world.collision, meets);
        }
    }
    let out = step(world);
    if let (Some(prev), Some(space)) = (prev, world.space.as_deref_mut()) {
        if space.shown_hulls != prev {
            space.show_hulls(&mut world.collision, prev);
        }
    }
    out
}

/// Names each craft's collision body, as admission names every other: a shot
/// or a look that meets a body without a name meets nothing at all.
fn name_bodies(world: &mut World, fighters: &[SpaceFighter]) {
    if let Some(collision) = world.fleet.collision.as_mut() {
        for body in fighters.iter().filter_map(|f| f.kernel_body) {
            // Admission numbers its bodies from 0 up, none above its slot in
            // the kernel, and a craft's slots come after all of theirs.
            debug_assert!(collision.body_reference(body).is_none());
            collision.bind_body(crate::store::Ref::new(body, 1), body);
        }
    }
}

/// The named body of the craft `unit` is aboard.
pub(crate) fn craft_body_in(world: &World, unit: crate::store::Ref<crate::gen::Unit>) -> Option<crate::store::Ref<crate::gen::Body>> {
    let life = world.sim.store.unit.get(unit)?.generation;
    let body = carrier(world, unit.slot, life)?.kernel_body?;
    world.fleet.collision.as_ref()?.kernel_reference(body as usize)
}

/// A soldier's shot or blade struck `body`. What it does to a craft waits for
/// the battlefield's next step: a craft it destroys loses its crew, and their
/// deaths need the services the soldiers' own phases hold meanwhile.
pub(crate) fn struck_in(world: &mut World, body: crate::store::Ref<crate::gen::Body>, packet: crate::combat::impacts::HitPacket, owner: OrdnanceOwner) {
    let Some(kernel) = world.fleet.collision.as_ref().and_then(|c| c.kernel_of(body)) else { return; };
    let Some((fighter, generation)) = world.space().and_then(|space| match space.body_owner(kernel as i32) {
        SpaceBodyOwner::Fighter(f) => Some((f, space.fighters[f].generation)),
        _ => None,
    }) else { return; };
    let (team, human, _) = crate::combat::live::owner_facts_in(&world.sim, &mut world.units, owner);
    let unit = match owner {
        OrdnanceOwner::Unit(unit) => Some(unit),
        OrdnanceOwner::Player => Some(f3::viewpoint_in(&world.units)),
        _ => None,
    };
    let attacker_unit = match (owner, unit.and_then(|u| world.space_unit_key(u))) {
        (OrdnanceOwner::OwnerSnapshot(owner), _) => Some(owner),
        (_, Some(key)) => world.space_owner_of(&key),
        _ => None,
    };
    let packet = SpaceDamagePacket {
        raw_damage: packet.damage,
        scales: packet.scales,
        shield_scale: packet.shield,
        attacker_unit,
        attacker_team: team as i32,
        attacker_is_ai: !human,
    };
    let Some(space) = world.space_mut() else { return; };
    if space.pending_hits.len() < space.pending_hits.capacity() {
        space.pending_hits.push(PendingHit { fighter, generation, packet });
    }
}

pub(crate) fn step_world_in(world: &mut World, dt: f64) {
    local::step(world, dt);
    let Some(mut space) = world.space.take() else {
        return;
    };
    space.soldier_cache.clear();
    space.soldier_targets_truncated = false;
    space.prune_hidden_dead_crew(world);
    if f3::active_in(&world.units) {
        for fighter in &mut space.fighters {
            for seat in std::iter::once(&mut fighter.pilot).chain(fighter.gunners.iter_mut()) {
                if seat.is_some_and(|p| !crate::combat::owners::alive(&world.sim.store, p.owner)) { *seat = None; }
            }
        }
    }
    if f3::active_in(&world.units) {
        for attachment in &mut space.buildings {
            if let Some(t) = &mut attachment.turret {
                if t.remote_operator.is_some_and(|p| !crate::combat::owners::alive(&world.sim.store, p.owner)) {
                    t.remote_operator = None;
                    t.pending_remote_aim = None;
                    t.pending_remote_fire = false;
                    t.pending_remote_spread_scale = None;
                }
            }
            if let Some(t) = &mut attachment.terminal {
                if t.occupant.is_some_and(|p| !crate::combat::owners::alive(&world.sim.store, p.owner)) { t.occupant = None; }
            }
        }
    }
    let cap = space.soldier_cache.capacity();
    for (unit_ref, unit) in world.sim.store.unit.iter() {
        if !unit.alive || unit.combat.dead || unit.removed || unit.seat.is_some() {
            continue;
        }
        let (pos, vel) = match &unit.soldier {
            UnitBody::Soldier(s) => (s.position, s.velocity),
            UnitBody::Droideka(d) => (d.position, d.velocity),
        };
        if space.fighters.iter().any(|fighter| std::iter::once(fighter.pilot).chain(fighter.gunners).flatten()
            .any(|pilot| crate::combat::owners::resolve(&world.sim.store, pilot.owner) == Some(unit_ref))) { continue; }
        if space.soldier_cache.len() == cap {
            space.soldier_targets_truncated = true;
            break;
        }
        space.soldier_cache.push(SpaceSoldierTarget {
            unit: unit_ref,
            life: unit.generation,
            center: unit.bounds.center, radius: unit.bounds.radius,
            team: unit.team as i32,
            is_human: unit.human,
            position: pos,
            velocity: vel,
        });
    }
    space.step_in_world(world, dt);
    space.publish_passages(world);
    world.space = Some(space);
}

impl World {
    // ---- Mission / Rules Seam on World (Single-Owner f3_props + Space) ----

    /// Captures the current unit and character life for a delayed effect. Repeated
    /// launches by the same owner share the immutable native snapshot.
    pub fn space_launch_owner(&mut self, unit: &crate::gen::F3UnitKey) -> Option<SpaceOwner> {
        let u = unit.validate(&self.sim.store).ok()?;
        if !u.alive || u.combat.dead || u.removed { return None; }
        self.space_owner_of(unit)
    }

    /// The same for a life that may be over, whose shot is still in flight.
    fn space_owner_of(&mut self, unit: &crate::gen::F3UnitKey) -> Option<SpaceOwner> {
        let u = unit.validate(&self.sim.store).ok()?;
        if let Some((owner, _)) = self.sim.store.owner_snapshot.iter().find(|(_, s)|
            s.source_unit == unit.reference() && s.generation == unit.life &&
            s.team == u.team && s.character == u.character && s.human == u.human) { return Some(owner); }
        crate::combat::owners::capture(&mut self.sim.store, unit).ok()
    }

    pub fn space_unit_key(&self, unit: crate::store::Ref<crate::gen::Unit>) -> Option<crate::gen::F3UnitKey> {
        Some(crate::gen::F3UnitKey::of(unit, self.sim.store.unit.get(unit)?))
    }

    /// Board a free seat of a landed craft within reach of its hull.
    pub fn space_board_seat(&mut self, unit: &crate::gen::F3UnitKey, fighter: usize, seat: u8) -> Option<SpaceSeat> {
        let sp = self.space.as_ref()?;
        let f = sp.fighters.get(fighter)?;
        let u = unit.validate(&self.sim.store).ok()?;
        if u.seat.is_some() || u.combat.dead || !u.alive || u.removed || !f.alive || f.state != SpaceFlightState::Landed
            || f.team != u.team as i32 || seat >= sp.profiles[f.profile_index].seat_count || f.occupant(seat).is_some() { return None; }
        if sp.fighters.iter().any(|f| std::iter::once(f.pilot).chain(f.gunners).flatten().any(|p|
            crate::combat::owners::resolve(&self.sim.store, p.owner) == Some(unit.reference()))) { return None; }
        let position = self.space_unit_position(unit.reference())?;
        let model = &sp.profiles[f.profile_index];
        let mut local = three::Vector3::new(position[0], position[1], position[2]);
        local.apply_matrix4(&three::Matrix4 { elements: f.inverse });
        if ![local.x, local.y, local.z].iter().enumerate().all(|(axis, value)| {
            let min = model.contact_bounds_min[axis] - PROVISIONAL_BOARDING_REACH;
            let max = model.contact_bounds_max[axis] + PROVISIONAL_BOARDING_REACH;
            (min..=max).contains(value)
        }) {
            return None;
        }
        let (team, is_human, generation) = (u.team as i32, u.human, f.generation);
        let repair_capable = sp.repair_classes.contains(&u.class_name);
        let owner = self.space_launch_owner(unit)?;
        let crew = SpacePilotId {owner, team, is_human, repair_capable};
        let (seat_pos, vel, fwd) = {
            let sp = self.space.as_mut()?;
            let profile = &sp.profiles[sp.fighters[fighter].profile_index];
            let f = &mut sp.fighters[fighter];
            *f.occupant_mut(seat)? = Some(crew);
            (f.seat_world_position(profile, seat), f.velocity, f.forward())
        };
        if let Some(UnitBody::Soldier(body)) = self.sim.store.unit.get_mut(unit.reference()).map(|u| &mut u.soldier) {
            place_rider(body, seat_pos, vel);
            body.previous = seat_pos;
            body.air = None;
            body.dive = 0.0;
            body.aim_yaw = v8math::atan2(fwd[0], fwd[2]);
            body.aim_pitch = v8math::asin(v8math::min(1.0, v8math::max(-1.0, fwd[1])));
        }
        self.nav_walk_stop(unit.reference());
        Some(SpaceSeat { fighter, generation, seat, owner })
    }

    /// The seat a person takes on boarding a landed craft: the earliest seat
    /// that is free or that a bot holds (nobody takes a seat another person
    /// holds).
    pub fn space_person_seat(&self, fighter: usize) -> Option<u8> {
        let space = self.space()?;
        let f = space.fighters.get(fighter)?;
        let count = space.profiles[f.profile_index].seat_count;
        (0..count).find(|&s| f.occupant(s).is_none_or(|p| !p.is_human))
    }

    /// The nearest live landed craft of `team` a person could board.
    pub fn space_nearest_boardable(&self, point: [f64; 3], team: i32) -> Option<usize> {
        let space = self.space()?;
        let mut best: Option<(usize, f64)> = None;
        for f in &space.fighters {
            if !f.alive || f.state != SpaceFlightState::Landed || f.team != team || self.space_person_seat(f.id).is_none() {
                continue;
            }
            let d = dist_sq(point, f.position);
            if best.is_none_or(|(_, b)| d < b) {
                best = Some((f.id, d));
            }
        }
        best.map(|(id, _)| id)
    }

    /// A person boards a landed craft in reach, in the seat
    /// [`World::space_person_seat`] gives. A bot holding that seat moves on to
    /// the next free seat after its own, or gets out.
    pub fn space_board_person(&mut self, unit: &crate::gen::F3UnitKey, fighter: usize) -> Option<SpaceSeat> {
        let seat = self.space_person_seat(fighter)?;
        // Nothing moves unless the person may board (reach, team, a live landed hull).
        let probe = {
            let space = self.space.as_mut()?;
            let f = space.fighters.get_mut(fighter)?;
            f.occupant_mut(seat)?.take()
        };
        let allowed = self.space_board_seat(unit, fighter, seat);
        let Some(boarded) = allowed else {
            let space = self.space.as_mut()?;
            *space.fighters[fighter].occupant_mut(seat)? = probe;
            return None;
        };
        if let Some(bot) = probe {
            let space = self.space.as_mut()?;
            let f = &mut space.fighters[fighter];
            let count = space.profiles[f.profile_index].seat_count;
            let next = (seat + 1..count).find(|&s| f.occupant(s).is_none());
            let generation = f.generation;
            let state = f.state;
            match next {
                Some(to) => {
                    *f.occupant_mut(to)? = Some(bot);
                    let receipt = SpaceSeat { fighter, generation, seat: to, owner: bot.owner };
                    self.space_reseat_bot(bot, Some(receipt), state);
                }
                None => {
                    let exit = f.dismount_position(&space.profiles[f.profile_index]);
                    if let Some(r) = crate::combat::owners::resolve(&self.sim.store, bot.owner) {
                        if let Some(UnitBody::Soldier(s)) = self.sim.store.unit.get_mut(r).map(|u| &mut u.soldier) {
                            place_rider(s, exit, [0.0; 3]);
                        }
                    }
                    self.space_reseat_bot(bot, None, state);
                }
            }
        }
        Some(boarded)
    }

    /// The seat keys: on to the next seat that way (`step` +1 or -1, round
    /// the craft) that no other person holds. A bot there trades places. The
    /// seat left behind loses its controls (a pilot lets go of the stick).
    pub fn space_switch_seat(&mut self, seat: SpaceSeat, step: i8) -> Option<SpaceSeat> {
        let f = self.space_seat_fighter(seat)?;
        let me = f.occupant(seat.seat)?;
        let count = i16::from(self.space()?.profiles[f.profile_index].seat_count);
        let target = (1..count)
            .map(|k| (i16::from(seat.seat) + i16::from(step.signum()) * k).rem_euclid(count) as u8)
            .find(|&s| f.occupant(s).is_none_or(|p| !p.is_human))?;
        let (bot, state) = {
            let space = self.space.as_mut()?;
            let f = &mut space.fighters[seat.fighter];
            let bot = f.occupant_mut(target)?.take();
            *f.occupant_mut(target)? = Some(me);
            *f.occupant_mut(seat.seat)? = bot;
            for s in [seat.seat, target] {
                if s == 0 {
                    f.pending_input = SpacePilotInput::default();
                } else if let Some(input) = f.gunner_inputs.get_mut(usize::from(s) - 1) {
                    *input = SpacePilotInput::default();
                }
            }
            (bot, f.state)
        };
        if let Some(bot) = bot {
            let receipt = SpaceSeat { seat: seat.seat, owner: bot.owner, ..seat };
            self.space_reseat_bot(bot, Some(receipt), state);
        }
        Some(SpaceSeat { seat: target, ..seat })
    }

    /// Hands a bot moved by a person to its new seat (or back to its feet).
    fn space_reseat_bot(&mut self, bot: SpacePilotId, seat: Option<SpaceSeat>, state: SpaceFlightState) {
        let Some(r) = crate::combat::owners::resolve(&self.sim.store, bot.owner) else { return };
        if let Some(space) = self.space.as_mut() {
            space.pilots.reseat(r.slot as usize, seat, state);
        }
    }

    /// Find a soldier's current seat after admission, respawn, or reconnect.
    pub fn space_seat_for_unit(&self, key: &crate::gen::F3UnitKey) -> Option<SpaceSeat> {
        let unit = key.validate(&self.sim.store).ok()?;
        if !unit.alive || unit.removed || unit.combat.dead { return None; }
        for f in &self.space()?.fighters {
            if !f.alive { continue; }
            for (seat, crew) in std::iter::once(f.pilot).chain(f.gunners).enumerate() {
                let Some(crew) = crew else { continue };
                if self.sim.store.owner_snapshot.get(crew.owner).is_some_and(|s|
                    s.source_unit == key.reference() && s.generation == key.life) {
                    return Some(SpaceSeat { fighter: f.id, generation: f.generation, seat: seat as u8, owner: crew.owner });
                }
            }
        }
        None
    }

    /// Read the live hull only while its life and its current occupant match the receipt.
    pub fn space_seat_fighter(&self, seat: SpaceSeat) -> Option<&SpaceFighter> {
        living_occupant(self, seat.owner)?;
        let f = self.space()?.fighters.get(seat.fighter)?;
        (f.alive && f.generation == seat.generation && f.occupant(seat.seat)?.owner == seat.owner).then_some(f)
    }

    pub fn space_seat_weapon(&self, seat: SpaceSeat, index: usize) -> Option<SpaceSeatWeapon<'_>> {
        let f = self.space_seat_fighter(seat)?;
        let weapons = &self.space()?.profiles[f.profile_index].weapons;
        let spec = weapons.get(index)?;
        if spec.seat != seat.seat { return None; }
        Some(SpaceSeatWeapon {
            spec, runtime: f.weapons.get(index)?, lock: f.weapon_locks.get(index)?,
            selected: f.weapon_trigger(weapons, index).0,
        })
    }

    /// Submit inputs only for the seat and craft life the caller boarded.
    pub fn space_control_seat(&mut self, seat: SpaceSeat, input: SpacePilotInput) -> bool {
        if ![input.throttle, input.stick_x, input.stick_y, input.roll].iter().all(|n|n.is_finite() && n.abs() <= 1.)
            || input.aim.is_some_and(|v| v.iter().any(|n| !n.is_finite())) { return false; }
        let Some(f) = self.space_seat_fighter(seat) else { return false; };
        let profile = &self.space().unwrap().profiles[f.profile_index];
        if input.weapon_index.is_some_and(|i| profile.weapons.get(usize::from(i)).is_none_or(|w| w.seat != seat.seat)) {
            return false;
        }
        let limits = profile.seat_aim_limits[usize::from(seat.seat).min(5)];
        let mut input = input;
        // A gunner's aim keeps to his seat's arc, for a person and a bot alike.
        if seat.seat != 0 {
            input.aim = input.aim.map(|a| f.seat_aim_within(limits, a));
        }
        let f = &mut self.space.as_mut().unwrap().fighters[seat.fighter];
        if seat.seat == 0 { f.pending_input = input; } else { f.gunner_inputs[seat.seat as usize - 1] = input; }
        true
    }

    /// Return the same landing decision the pilot's next fresh action press uses.
    pub fn space_landing_permission(&self, seat: SpaceSeat) -> flight::SpaceLandingPermission {
        use flight::SpaceLandingPermission::Unavailable;
        let Some(space) = self.space() else { return Unavailable; };
        if seat.seat != 0 { return Unavailable; }
        let Some(fighter) = self.space_seat_fighter(seat) else { return Unavailable; };
        fighter.landing_at(&space.bounds)
    }

    /// Resolve an admitted player via its connection and current character.
    pub fn space_player_unit(&self, player: u32) -> Option<crate::gen::F3UnitKey> {
        let p = self.input.players.pool.get(player as usize)?;
        let view = crate::battle::f4_connections::wire_view_in(&self.battle, &self.sim.store, p.character.as_ref()?)?;
        self.space_unit_key(view.unit?)
    }

    /// Leave a landed ship at the outer wing on its current deck.
    pub fn space_leave_seat(&mut self, seat: SpaceSeat) -> Option<[f64; 3]> {
        let unit = living_occupant(self, seat.owner)?;
        let space = self.space.as_mut()?;
        let f = space.fighters.get_mut(seat.fighter)?;
        if !f.alive || f.generation != seat.generation || f.state != SpaceFlightState::Landed || f.occupant(seat.seat)?.owner != seat.owner { return None; }
        let p = &space.profiles[f.profile_index];
        let exit = f.dismount_position(p);
        *f.occupant_mut(seat.seat)? = None;
        if seat.seat == 0 { f.pending_input = Default::default(); }
        if let UnitBody::Soldier(s) = &mut self.sim.store.unit.get_mut(unit)?.soldier { place_rider(s, exit, [0.0;3]); }
        Some(exit)
    }

    /// Registers an authored non-auxiliary box region as an active landing
    /// zone (`AddLandingRegion`), sampling its deck floor elevation from live
    /// collision and updating parked fighters inside it.
    pub fn space_add_landing_region(&mut self, name: &str) -> Option<usize> {
        let mut space = self.space.take()?;
        let res = space.bounds.register_landing_region(name);
        if let Some(zone_idx) = res {
            space.bounds.sample_landing_zone_floor(
                zone_idx,
                &mut self.collision,
                space.collision_world,
            );
            for f in &mut space.fighters {
                if f.alive
                    && f.state == SpaceFlightState::Landed
                    && space.bounds.landing_zones[zone_idx].contains(f.position)
                {
                    f.landed_zone = Some(zone_idx);
                }
            }
        }
        self.space = Some(space);
        res
    }

    /// Removes a registered landing region by name.
    pub fn space_remove_landing_region(&mut self, name: &str) -> bool {
        let Some(space) = self.space.as_deref_mut() else {
            return false;
        };
        let removed = space.bounds.remove_landing_region(name);
        if removed {
            for f in &mut space.fighters {
                if f.state == SpaceFlightState::Landed {
                    f.landed_zone = space
                        .bounds
                        .landing_zones
                        .iter()
                        .position(|z| z.contains(f.position));
                }
            }
        }
        removed
    }

    /// Sets the space battlefield's spatial world half-extent (`SetWorldExtents`).
    pub fn space_set_world_extents(&mut self, half_extent: f64) -> bool {
        let Some(space) = self.space.as_deref_mut() else {
            return false;
        };
        space.bounds.set_world_extents(half_extent)
    }

    /// Starts or resumes a named world animation group (`PlayAnimationGroup`).
    pub fn space_play_animation_group(&mut self, name: &str) -> bool {
        let Some(space) = self.space.as_deref_mut() else {
            return false;
        };
        space.world_anims.play_group(name)
    }

    /// Pauses a named world animation group (`PauseAnimationGroup`).
    pub fn space_pause_animation_group(&mut self, name: &str) -> bool {
        let Some(space) = self.space.as_deref_mut() else {
            return false;
        };
        space.world_anims.pause_group(name)
    }

    /// Looks up a canonical `f3_props` building index by script name.
    pub fn space_building_by_name(&self, name: &str) -> Option<usize> {
        self.space.as_deref()?.building_by_name(name)
    }

    /// Returns the canonical `ObjectHandle::Prop(F4Handle)` for `building_index`.
    pub fn space_building_handle(&self, building_index: usize) -> Option<ObjectHandle> {
        let space = self.space.as_deref()?;
        space
            .buildings
            .iter()
            .find(|b| b.building_index == building_index)
            .map(|_| ObjectHandle::Prop(f3_props::owner_in(self, building_index)))
    }

    /// Returns a read-only snapshot combining canonical `f3_props` scalar
    /// state with the space attachment's 2005 state and transform.
    pub fn space_building_snapshot(&self, building_index: usize) -> Option<SpaceBuildingSnapshot> {
        let space = self.space.as_deref()?;
        let slot = space
            .building_slot_by_index
            .get(building_index)
            .copied()
            .flatten()?;
        let att = &space.buildings[slot];
        let facts = building_core(self, building_index)?;
        let shields = f3_props::mission_shields(&self.units, building_index);
        Some(SpaceBuildingSnapshot {
            building_index,
            owner: f3_props::owner_in(self, building_index),
            generation: facts.target_generation,
            alive: facts.alive,
            health: facts.health,
            max_health: facts.max_health,
            cur_shield: shields.current,
            max_shield: shields.maximum,
            add_shield: shields.recharge,
            team: facts.team,
            collidable: facts.collidable,
            visible: att.visible,
            lock_on_enabled: att.lock_on_enabled,
            ai_damage_threshold: att.ai_damage_threshold,
            position: att.position,
        })
    }

    /// Returns `(current, maximum, recharge)` shields for `building_index`,
    /// aligned with the script host's `mission_system_shields` tuple layout.
    pub fn space_building_shields(&self, building_index: usize) -> Option<(f64, f64, f64)> {
        let space = self.space.as_deref()?;
        space.building_slot_by_index.get(building_index).copied().flatten()?;
        let sh = f3_props::mission_shields(&self.units, building_index);
        Some((sh.current, sh.maximum, sh.recharge))
    }

    /// Mutates a property on canonical `f3_props` building `building_index`
    /// through the single canonical owner (`ObjectProperty`), emitting
    /// `SpaceEvent::ShieldChanged` on explicit shield edits.
    pub fn space_set_building_property(
        &mut self,
        building_index: usize,
        prop: ObjectProperty,
    ) -> RuleResult<()> {
        if !f3::active_in(&self.units) || building_index >= f3_props::count_in(&self.units) {
            return Err(RuleError::NotAvailable);
        }
        match prop {
            ObjectProperty::Health(v) if !v.is_finite() || v < 0.0 => {
                return Err(RuleError::InvalidValue)
            }
            ObjectProperty::MaxHealth(v) if !v.is_finite() || v <= 0.0 => {
                return Err(RuleError::InvalidValue)
            }
            ObjectProperty::Team(t)
                if t.fract() != 0.0
                    || !(0.0..crate::battle::spawning::TEAM_COUNT as f64).contains(&t) =>
            {
                return Err(RuleError::InvalidValue)
            }
            ObjectProperty::MaxShield(v) if !v.is_finite() || v < 0.0 => {
                return Err(RuleError::InvalidValue)
            }
            ObjectProperty::CurrentShield(v) | ObjectProperty::AddShield(v) if !v.is_finite() => {
                return Err(RuleError::InvalidValue)
            }
            ObjectProperty::AiDamageThreshold(v) if !v.is_finite() => {
                return Err(RuleError::InvalidValue)
            }
            _ => {}
        }
        if let ObjectProperty::Health(value) = prop {
            if value == 0.0 {
                let owner = f3_props::owner_in(self, building_index);
                crate::battle::f4::set_attacker_in(&mut self.battle, &owner, Attacker::Null);
            }
            f3::with_services_in(self, |w, s| {
                if let Some(services) = s {
                    f3_props::command_hit_in(w, building_index, value, None, None, services);
                }
            });
            return Ok(());
        }
        f3_props::rule_property(self, building_index, prop)
    }

    /// Applies space-weapon damage directly to canonical `f3_props` building
    /// `building_index` (routing through shields, `AIDamageThreshold`,
    /// collision swap, and `Event::ObjectKilled`).
    pub fn space_damage_building(
        &mut self,
        building_index: usize,
        packet: &SpaceDamagePacket,
    ) -> SpaceDamageOutcome {
        let Some(mut space) = self.space.take() else {
            return SpaceDamageOutcome::default();
        };
        let out = space.damage_building_world(self, building_index, packet);
        space.drain_blasts(self);
        self.space = Some(space);
        out
    }

    /// Immediately destroys canonical `f3_props` building `building_index`.
    pub fn space_kill_building(&mut self, building_index: usize, attacker_unit: Option<SpaceOwner>) -> bool {
        let Some(facts) = building_core(self, building_index) else {
            return false;
        };
        if !facts.alive {
            return false;
        }
        let cur_shield = self
            .space_building_shields(building_index)
            .map_or(0.0, |(cur, _, _)| cur);
        let lethal = SpaceDamagePacket {
            raw_damage: facts.max_health + cur_shield + 1.0,
            scales: [1.0; 5],
            shield_scale: 1.0,
            attacker_unit,
            attacker_team: 0,
            attacker_is_ai: false,
        };
        self.space_damage_building(building_index, &lethal).destroyed
    }

    /// Applies fusion cutter repair through canonical `f3_props`, refusing
    /// permanently unrepairable objects (`MaxHealth >= UNREPAIRABLE_MAX_HEALTH_THRESHOLD`)
    /// or enemy repairers.
    pub fn space_repair_building(
        &mut self,
        building_index: usize,
        amount: f64,
        repairer_team: i32,
    ) -> f64 {
        if !f3::active_in(&self.units) || !amount.is_finite() || amount <= 0.0 {
            return 0.0;
        }
        let Some(before) = building_core(self, building_index) else {
            return 0.0;
        };
        if !before.max_health.is_finite()
            || before.max_health <= 0.0
            || is_building_unrepairable(before.max_health)
        {
            return 0.0;
        }
        if before.team != 0 && repairer_team != 0 && before.team != repairer_team {
            return 0.0;
        }
        let next_hp = v8math::min(before.max_health, before.health + amount);
        let gained = v8math::max(0.0, next_hp - before.health);
        if gained <= 0.0 {
            return 0.0;
        }
        f3::with_services_in(self, |w, s| {
            if let Some(services) = s {
                f3_props::space_heal_in(w, building_index, next_hp, services);
            }
        });
        if let Some(after) = building_core(self, building_index) {
            if !before.alive && after.alive {
                let life = f3_props::owner_in(self, building_index).life;
                if let Some(space) = self.space.as_deref_mut() {
                    if let Some(slot) = space
                        .building_slot_by_index
                        .get(building_index)
                        .copied()
                        .flatten()
                    {
                        space.buildings[slot].last_life = Some(life);
                        f3_props::restore_shield_after_rebuild(&mut self.units, building_index);
                    }
                    space.push_event(SpaceEvent::BuildingRespawned {
                        building: building_index,
                        life,
                        team: after.team,
                    });
                }
            }
        }
        gained
    }

    /// Looks up an indestructible proximity door index by script name.
    pub fn space_door_by_name(&self, name: &str) -> Option<usize> {
        self.space.as_deref()?.door_by_name(name)
    }

    /// Returns a read-only snapshot of an indestructible proximity door.
    pub fn space_door_snapshot(&self, door_id: usize) -> Option<SpaceDoorSnapshot> {
        self.space.as_deref()?.door_snapshot(door_id)
    }

    /// Sets the lock flag on proximity door `door_id`.
    pub fn space_set_door_locked(&mut self, door_id: usize, locked: bool) -> bool {
        let Some(mut space) = self.space.take() else {
            return false;
        };
        let ok = space.set_door_locked(door_id, locked, &mut self.collision, self.movement.flags_mut());
        if ok {
            space.publish_passages(self);
        }
        self.space = Some(space);
        ok
    }

    /// Sets the lock flag on a proximity door by script name.
    pub fn space_set_door_locked_by_name(&mut self, name: &str, locked: bool) -> bool {
        let Some(door_id) = self.space_door_by_name(name) else {
            return false;
        };
        self.space_set_door_locked(door_id, locked)
    }

    // ---- Remote Turret Chair Control (Canonical Alive Checks) ----

    /// Occupy a remote chair with a current nearby soldier and retain both building lives.
    pub fn space_board_terminal(&mut self, unit: &crate::gen::F3UnitKey, chair: usize) -> Option<SpaceTerminalSeat> {
        let u = unit.validate(&self.sim.store).ok()?;
        if !u.alive || u.combat.dead || u.removed || u.seat.is_some() || self.space_seat_for_unit(unit).is_some() { return None; }
        let team = u.team as i32;
        let is_human = u.human;
        let pos = self.space_unit_position(unit.reference())?;
        let building = self.space_building_snapshot(chair)?;
        let attachment = self.space()?.buildings.iter().find(|a| a.building_index == chair)?;
        let reach=attachment.radius + PROVISIONAL_BOARDING_REACH;
        if !building.alive || (building.team != 0 && building.team != team)
            || dist_sq(pos, attachment.center) > reach * reach {
            return None;
        }
        let owner = self.space_launch_owner(unit)?;
        if self.space()?.buildings.iter().any(|a| a.terminal.as_ref().and_then(|t| t.occupant).is_some_and(|p|p.owner==owner)) { return None; }
        let pilot = SpacePilotId { owner, team, is_human, repair_capable: false };
        let turret = self.space_enter_remote_terminal(chair, pilot)?;
        self.space_seat_chair_operator(chair, pos, unit.reference());
        Some(SpaceTerminalSeat { chair, chair_life: building.owner.life, turret,
            turret_life: f3_props::owner_in(self, turret).life, owner })
    }

    /// Remembers where a chair's new operator stood and puts his body in the
    /// chair's seat, when the chair shows its operator.
    fn space_seat_chair_operator(&mut self, chair: usize, feet: [f64; 3], unit: crate::store::Ref<crate::gen::Unit>) {
        let Some(att) = self.space.as_deref_mut().and_then(|s| s.buildings.iter_mut().find(|a| a.building_index == chair)) else { return };
        let Some(term) = att.terminal.as_mut() else { return };
        term.feet = Some(feet);
        let Some((at, yaw)) = chair_seat_world(att) else { return };
        if let Some(UnitBody::Soldier(body)) = self.sim.store.unit.get_mut(unit).map(|u| &mut u.soldier) {
            place_rider(body, at, [0.0; 3]);
            body.aim_yaw = yaw;
        }
    }

    /// Where the operator of chair `chair` stands: the point he returns to.
    pub(crate) fn space_terminal_feet(&self, chair: usize) -> Option<[f64; 3]> {
        self.space()?.buildings.iter().find(|b| b.building_index == chair)?.terminal.as_ref()?.feet
    }

    /// Frees the chair of `seat` while its occupant is still the seat's
    /// soldier, whatever became of its gun (destroyed, or rebuilt as another
    /// life): he leaves it.
    pub(crate) fn space_free_terminal(&mut self, seat: SpaceTerminalSeat) -> bool {
        let held = self.space().and_then(|s| s.buildings.iter().find(|b| b.building_index == seat.chair))
            .and_then(|b| b.terminal.as_ref()?.occupant).is_some_and(|p| p.owner == seat.owner);
        held && self.space_exit_remote_terminal(seat.chair)
    }

    pub(crate) fn current_terminal_seat(&self, seat: SpaceTerminalSeat) -> bool {
        if living_occupant(self, seat.owner).is_none() { return false; }
        let Some(space) = self.space() else { return false };
        let occupied = space.buildings.iter().find(|b| b.building_index == seat.chair)
            .and_then(|b| b.terminal.as_ref()).is_some_and(|t| t.remote_turret == Some(seat.turret) &&
                t.occupant.is_some_and(|p| p.owner == seat.owner));
        occupied && self.space_building_snapshot(seat.chair).is_some_and(|v| v.alive && v.owner.life == seat.chair_life)
            && self.space_building_snapshot(seat.turret).is_some_and(|v| v.alive && v.owner.life == seat.turret_life)
    }

    /// A chair's input to its gun: the aim and the trigger.
    pub fn space_control_terminal(&mut self, seat: SpaceTerminalSeat, direction: [f64;3], trigger: bool) -> bool {
        self.space_control_terminal_scaled(seat, direction, trigger, None)
    }

    /// The same with the spread scale a bot's skill hands the gun (a player's
    /// gun takes none).
    pub fn space_control_terminal_scaled(&mut self, seat: SpaceTerminalSeat, direction: [f64;3], trigger: bool, spread_scale: Option<f64>) -> bool {
        self.current_terminal_seat(seat) && direction.iter().all(|n|n.is_finite())
            && self.space_set_remote_turret_input(seat.turret, direction, trigger, spread_scale)
    }

    pub fn space_leave_terminal(&mut self, seat: SpaceTerminalSeat) -> bool {
        self.current_terminal_seat(seat) && self.space_exit_remote_terminal(seat.chair)
    }

    pub fn space_cycle_terminal(&mut self, seat: SpaceTerminalSeat, next: bool) -> Option<SpaceTerminalSeat> {
        if !self.current_terminal_seat(seat) { return None; }
        let (chair,turret) = self.space_cycle_remote_terminal(seat.chair, next)?;
        Some(SpaceTerminalSeat { chair, turret, chair_life:f3_props::owner_in(self,chair).life,
            turret_life:f3_props::owner_in(self,turret).life, owner:seat.owner })
    }

    /// Seats `pilot` into remote turret terminal `terminal_building_index`,
    /// binding them to its `RemoteGameObject` exterior turret if both are
    /// canonically alive and unoccupied.
    fn space_enter_remote_terminal(
        &mut self,
        terminal_building_index: usize,
        pilot: SpacePilotId,
    ) -> Option<usize> {
        if !building_core(self, terminal_building_index)?.alive {
            return None;
        }
        let turret_bidx = {
            let space = self.space.as_deref()?;
            let term_att = space
                .buildings
                .iter()
                .find(|b| b.building_index == terminal_building_index)?;
            let term = term_att.terminal.as_ref()?;
            if term.occupant.is_some() {
                return None;
            }
            term.remote_turret?
        };
        if !building_core(self, turret_bidx)?.alive {
            return None;
        }
        let space = self.space.as_deref_mut()?;
        {
            let t_att = space
                .buildings
                .iter_mut()
                .find(|b| b.building_index == turret_bidx)?;
            let t = t_att.turret.as_mut()?;
            if t.pilot_type != SpaceTurretPilotType::Remote || t.remote_operator.is_some() {
                return None;
            }
            t.remote_operator = Some(pilot);
        }
        if let Some(term_att) = space
            .buildings
            .iter_mut()
            .find(|b| b.building_index == terminal_building_index)
        {
            if let Some(term) = term_att.terminal.as_mut() {
                term.occupant = Some(pilot);
            }
        }
        Some(turret_bidx)
    }

    /// Exits the occupant from `terminal_building_index`, releasing its turret.
    fn space_exit_remote_terminal(&mut self, terminal_building_index: usize) -> bool {
        let Some(space) = self.space.as_deref_mut() else {
            return false;
        };
        let mut left = None;
        let Some(turret_bidx) = space
            .buildings
            .iter_mut()
            .find(|b| b.building_index == terminal_building_index)
            .and_then(|att| {
                let seated = chair_seat_world(att).is_some();
                let term = att.terminal.as_mut()?;
                let pilot = term.occupant.take()?;
                let feet = term.feet.take();
                left = feet.filter(|_| seated).map(|feet| (pilot.owner, feet));
                term.remote_turret
            })
        else {
            return false;
        };
        // A seated operator gets up where he stood when he took the chair.
        if let Some((owner, feet)) = left {
            if let Some(unit) = living_occupant(self, owner) {
                if let Some(UnitBody::Soldier(body)) = self.sim.store.unit.get_mut(unit).map(|u| &mut u.soldier) {
                    place_rider(body, feet, [0.0; 3]);
                }
            }
        }
        let Some(space) = self.space.as_deref_mut() else {
            return false;
        };
        if let Some(t) = space
            .buildings
            .iter_mut()
            .find(|b| b.building_index == turret_bidx)
            .and_then(|att| att.turret.as_mut())
        {
            t.remote_operator = None;
            t.pending_remote_fire = false;
            t.pending_remote_aim = None;
            t.pending_remote_spread_scale = None;
        }
        true
    }

    /// Cycles a seated remote turret operator to the next (`next == true`) or
    /// previous (`next == false`) linked terminal in the ship's chair ring.
    fn space_cycle_remote_terminal(
        &mut self,
        terminal_building_index: usize,
        next: bool,
    ) -> Option<(usize, usize)> {
        let (pilot, start_neighbor) = {
            let space = self.space.as_deref()?;
            let term = space
                .buildings
                .iter()
                .find(|b| b.building_index == terminal_building_index)?
                .terminal
                .as_ref()?;
            let pilot = term.occupant?;
            let neighbor = if next {
                term.next_terminal
            } else {
                term.prev_terminal
            }?;
            (pilot, neighbor)
        };

        let mut cursor = start_neighbor;
        let mut chosen: Option<(usize, usize)> = None;
        for _ in 0..16 {
            if cursor == terminal_building_index {
                break;
            }
            let cand_alive = building_core(self, cursor).is_some_and(|f| f.alive);
            let (cand_unocc, cand_turret, step_next) = {
                let space = self.space.as_deref()?;
                let Some(cand_term) = space
                    .buildings
                    .iter()
                    .find(|b| b.building_index == cursor)
                    .and_then(|b| b.terminal.as_ref())
                else {
                    break;
                };
                (
                    cand_term.occupant.is_none(),
                    cand_term.remote_turret,
                    if next {
                        cand_term.next_terminal
                    } else {
                        cand_term.prev_terminal
                    },
                )
            };
            if cand_alive && cand_unocc {
                if let Some(tid) = cand_turret {
                    let turret_alive = building_core(self, tid).is_some_and(|f| f.alive);
                    let turret_ready = self.space.as_deref().is_some_and(|sp| {
                        sp.buildings
                            .iter()
                            .find(|b| b.building_index == tid)
                            .and_then(|b| b.turret.as_ref())
                            .is_some_and(|t| {
                                t.pilot_type == SpaceTurretPilotType::Remote
                                    && t.remote_operator.is_none()
                            })
                    });
                    if turret_alive && turret_ready {
                        chosen = Some((cursor, tid));
                        break;
                    }
                }
            }
            let Some(nxt) = step_next else {
                break;
            };
            cursor = nxt;
        }

        let (new_terminal, new_turret) = chosen?;
        // He moves to the chair he now holds: its seat, or its middle when it
        // has none, so the chair he leaves is never where he stands.
        let feet = self.space().and_then(|s| s.buildings.iter().find(|b| b.building_index == new_terminal))
            .map(|b| chair_seat_world(b).map_or(b.center, |(at, _)| at));
        self.space_exit_remote_terminal(terminal_building_index);
        let bound = self.space_enter_remote_terminal(new_terminal, pilot)?;
        debug_assert_eq!(bound, new_turret);
        if let (Some(feet), Some(unit)) = (feet, living_occupant(self, pilot.owner)) {
            self.space_seat_chair_operator(new_terminal, feet, unit);
        }
        Some((new_terminal, new_turret))
    }

    /// Sets per-tick aim direction and fire trigger on a remote-operated turret.
    fn space_set_remote_turret_input(
        &mut self,
        turret_building_index: usize,
        aim_dir: [f64; 3],
        fire: bool,
        spread_scale: Option<f64>,
    ) -> bool {
        let Some(t) = self
            .space
            .as_deref_mut()
            .and_then(|s| {
                s.buildings
                    .iter_mut()
                    .find(|b| b.building_index == turret_building_index)
            })
            .and_then(|att| att.turret.as_mut())
        else {
            return false;
        };
        if t.remote_operator.is_none() {
            return false;
        }
        let len = v8math::hypot3(aim_dir[0], aim_dir[1], aim_dir[2]);
        if len > 1e-6 {
            t.pending_remote_aim = Some([aim_dir[0] / len, aim_dir[1] / len, aim_dir[2] / len]);
        }
        t.pending_remote_fire = fire;
        t.pending_remote_spread_scale = spread_scale;
        true
    }

    /// Computes a deterministic 64-bit digest covering both `SpaceBattlefield`
    /// state and canonical `f3_props` building state.
    pub fn space_state_digest(&self) -> Option<Wide> {
        Some(self.space.as_deref()?.state_digest(Some(self)))
    }

    /// Casts a ray against the active 2005 space collision world.
    pub fn space_ray_hit(
        &mut self,
        ray: Ray,
        far: f64,
        ignore: i32,
    ) -> Option<(i32, crate::collision::Hit)> {
        let world_id = self.space.as_deref()?.collision_world as u32;
        self.collision.ray_hit(world_id, ray, far, ignore)
    }

    /// Reads the live world-space AABB of collision kernel body `body`.
    pub fn space_body_bounds(&self, body: u32) -> Option<[f64; 6]> {
        self.collision
            .state
            .bodies
            .get(body as usize)
            .map(|b| b.bounds)
    }

    /// Reads the live enabled flag of collision kernel body `body`.
    pub fn space_body_enabled(&mut self, body: u32) -> Option<bool> {
        let world_id = self.space.as_deref().map_or(0, |s| s.collision_world);
        let order = self
            .collision
            .state
            .worlds
            .get(world_id)
            .and_then(|w| w.order.iter().position(|&s| s == body));
        let idx = order.unwrap_or(body as usize);
        self.movement.flags_mut().get(idx).map(|&f| f != 0)
    }

    /// Appends snapshots of all live on-foot units in the world into `out`.
    pub fn space_live_soldiers(&self, out: &mut Vec<SpaceSoldierTarget>) {
        out.clear();
        for (unit_ref, unit) in self.sim.store.unit.iter() {
            if unit.combat.dead || unit.removed || unit.seat.is_some() {
                continue;
            }
            let (pos, vel) = match &unit.soldier {
                UnitBody::Soldier(s) => (s.position, s.velocity),
                UnitBody::Droideka(d) => (d.position, d.velocity),
            };
            out.push(SpaceSoldierTarget {
                unit: unit_ref,
            life: unit.generation,
            center: unit.bounds.center, radius: unit.bounds.radius,
                team: unit.team as i32,
                is_human: unit.human,
                position: pos,
                velocity: vel,
            });
        }
    }

    /// Returns `(prev_matrix, live_matrix)` for an animated prop instance
    /// (such as an orbiting frigate) so drawing interpolates on the single
    /// authoritative 60 Hz simulation clock.
    pub fn space_animated_transform_pair(
        &self,
        prop_instance: usize,
    ) -> Option<([f64; 16], [f64; 16])> {
        self.space
            .as_deref()?
            .world_anims
            .transform_pair_for_instance(prop_instance)
    }

    /// Returns all animated space objects updated by the simulation tick.
    pub fn space_animated_objects(&self) -> &[AnimatedObject] {
        self.space
            .as_deref()
            .map_or(&[], |s| s.world_anims.objects.as_slice())
    }

    /// Steps only the 2005 space layer (`step_world_in`) on this `World`.
    /// Used by allocation-counting benchmarks to isolate space-layer heap
    /// traffic on a live admitted world; full battles advance via `World::step`.
    pub fn space_step_tick(&mut self, dt: f64) -> bool {
        if self.space.is_none() {
            return false;
        }
        step_world_in(self, dt);
        if f3::active_in(&self.units) { f3_props::advance_mission_shields(&mut self.units, dt); }
        true
    }

    /// Returns a live canonical on-foot unit reference whose intent is driven
    /// through foreign unit rows (preferring the registered viewpoint unit).
    pub fn space_viewpoint_unit(&self) -> Option<crate::store::Ref<crate::gen::Unit>> {
        if f3::active_in(&self.units) {
            if let Some(vp) = f3::registered_viewpoint_in(&self.units) {
                if !f3::native_intent_owned_in(&self.units, vp)
                    && self
                        .sim
                        .store
                        .unit
                        .get(vp)
                        .is_some_and(|u| u.alive && !u.combat.dead && !u.removed && u.seat.is_none())
                {
                    return Some(vp);
                }
            }
        }
        self.sim
            .store
            .unit
            .iter()
            .find(|(r, u)| {
                u.alive
                    && !u.combat.dead
                    && !u.removed
                    && u.seat.is_none()
                    && (!f3::active_in(&self.units) || !f3::native_intent_owned_in(&self.units, *r))
            })
            .map(|(r, _)| r)
    }

    /// Reads the live world position of canonical unit `unit`.
    pub fn space_unit_position(&self, unit: crate::store::Ref<crate::gen::Unit>) -> Option<[f64; 3]> {
        let u = self.sim.store.unit.get(unit)?;
        Some(match &u.soldier {
            UnitBody::Soldier(s) => s.position,
            UnitBody::Droideka(d) => d.position,
        })
    }

    /// Applies movement and firing intent to a canonical
    /// `World` unit so `World::step` drives it across real ship collision decks.
    #[allow(clippy::too_many_arguments)]
    pub fn space_drive_unit(
        &mut self,
        unit: crate::store::Ref<crate::gen::Unit>,
        yaw: Option<f64>,
        forward: f64,
        right: f64,
        fire_primary: bool,
        aim: Option<[f64; 3]>,
    ) -> Option<[f64; 3]> {
        self.space_drive_unit_weapons(unit,yaw,forward,right,[fire_primary,false],aim)
    }

    /// Apply the native primary and secondary soldier triggers during a boarding action.
    /// An absolute yaw writes the soldier's aim yaw. An aim vector uses the
    /// native bot aim override. Connected players also receive trigger and
    /// movement edges through their input bank.
    #[allow(clippy::too_many_arguments)]
    pub fn space_drive_unit_weapons(
        &mut self,
        unit: crate::store::Ref<crate::gen::Unit>,
        yaw: Option<f64>,
        forward: f64,
        right: f64,
        fire: [bool;2],
        aim: Option<[f64; 3]>,
    ) -> Option<[f64; 3]> {
        let (is_network_human, generation, pos) = {
            let u = self.sim.store.unit.get_mut(unit)?;
            if u.combat.dead || u.removed {
                return None;
            }
            if let Some(y) = yaw {
                match &mut u.soldier {
                    UnitBody::Soldier(s) => s.aim_yaw = y,
                    UnitBody::Droideka(d) => d.aim_yaw = y,
                }
            }
            u.intent.forward = forward;
            u.intent.right = right;
            u.intent.turn = 0.0;
            u.intent.buttons = fire.map(f64::from);
            u.intent.aim = aim;
            let pos = match &u.soldier {
                UnitBody::Soldier(s) => s.position,
                UnitBody::Droideka(d) => d.position,
            };
            (u.network_human == Some(true), u.generation, pos)
        };
        if self.input.players.active && is_network_human {
            let mut matched = None;
            for i in 0..self.input.players.order.len() {
                let key = self.input.players.order[i];
                let handle = self
                    .input
                    .players
                    .pool
                    .get(key)
                    .and_then(|p| p.character.clone());
                let is_match = handle.as_ref().is_some_and(|h| {
                    crate::battle::f4_connections::wire_view_in(
                        &self.battle,
                        &self.sim.store,
                        h,
                    )
                    .and_then(|v| v.unit)
                        == Some(unit)
                });
                if is_match {
                    matched = Some(key);
                    break;
                }
            }
            if let Some(key) = matched {
                let mut down = [crate::store::StrId(0); 4];
                let mut n = 0usize;
                if let Some(names) = self.input.players.names.as_ref() {
                    if forward > 0.5 {
                        down[n] = names.forward[0][0];
                        n += 1;
                    } else if forward < -0.5 {
                        down[n] = names.forward[1][0];
                        n += 1;
                    }
                    if right > 0.5 {
                        down[n] = names.right[0][0];
                        n += 1;
                    } else if right < -0.5 {
                        down[n] = names.right[1][0];
                        n += 1;
                    }
                    if fire[0] {
                        down[n] = names.fire_primary[0];
                        n += 1;
                    }
                    if fire[1] { down[n]=names.fire_secondary[0];n+=1; }
                }
                let tick = self.space().map_or(1., |s| f64::from(s.tick) + 1.);
                if let Some(p) = self.input.players.pool.get_mut(key) {
                    let epoch = v8math::max(p.controls.epoch, 1.0);
                    let seq = p.controls.sequence + 1.0;
                    let _ = p.controls.accept(epoch, seq, generation, &down[..n], &[]);
                    if p.reliable_fire {
                        for (channel, pressed) in fire.into_iter().enumerate() {
                            let state = &mut p.fire.state;
                            let previous = state.queue.iter().rev()
                                .find(|edge| edge.button == channel as f64 && edge.generation == generation)
                                .map_or(state.buttons[channel] > 0., |edge| edge.pressed);
                            if previous == pressed { continue; }
                            let edge = crate::gen::IfEdge { id: state.received + 1., generation, tick,
                                button: channel as f64, pressed, ..Default::default() };
                            if !crate::input::fire::push(state, Some(edge), true, true, true, tick) { return None; }
                        }
                    }
                    p.heard(tick * 1000. * SPACE_FIXED_DT);
                }
            }
        }
        Some(pos)
    }

    /// Prepares the unit frame handle needed by `World::step` in headless tests.
    pub fn prepare_units_step(&mut self) -> Option<u32> {
        self.prepare_foreign_unit_rows()
    }

    /// Installs a synthetic `SpaceBattlefield` built against this world's
    /// collision owner and movement flags (used by integration tests).
    #[doc(hidden)]
    pub fn space_install_synthetic(
        &mut self,
        build: impl FnOnce(&mut CollisionOwner, &mut [u8]) -> SpaceBattlefield,
    ) {
        let bf = build(&mut self.collision, self.movement.flags_mut());
        self.space = Some(Box::new(bf));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::battle::rules::events::{Event, EventQueue, EventRules, NamedObject, RuleActions};
    use crate::gen::*;
    use crate::space::bounds::SpaceBounds;
    use crate::space::flight::{install_bounds_box_tree, SpaceWeaponSpec};
    use crate::space::objects::{SpaceTerminalState, SpaceTurretState, TurretTargetFilter};
    use crate::store::Ref;
    use bfe_content::bvh::{build as build_bvh, Positions};
    use bfe_content::install::CollisionKernel;
    use std::cell::RefCell;
    use std::rc::Rc;
    #[test]
    fn a_steered_rocket_outside_is_lost_once_its_own_leaving_time_runs_out() {
        // Invented steps of a quarter second: the timer starts at 13/6 s outside.
        let mut timer = -1.0;
        let mut lost_at = None;
        for step in 1..=12 {
            if guided_out_of_bounds_step(&mut timer, true, 0.25) {
                lost_at = Some(step);
                break;
            }
        }
        assert_eq!(lost_at, Some(9), "2.1667 s at 0.25 s a step");
        let mut back = -1.0;
        assert!(!guided_out_of_bounds_step(&mut back, true, 0.25));
        assert!(!guided_out_of_bounds_step(&mut back, false, 0.25));
        assert_eq!(back, -1.0, "back inside, the time starts again");
    }


    pub(crate) fn make_synthetic_f3_world() -> (Box<World>, usize) {
        let mut w = World::new();
        let empty_bvh = build_bvh(Positions::F32(&[]), &[]);
        let empty_pos = w.collision.positions(&[]);
        let empty_tree = w.collision.tree(empty_pos, &[], &empty_bvh);
        let coll_world = w.collision.world_new(empty_tree, -1, -1);
        w.fleet.collision = Some(crate::vehicles::native::Collision::new(
            coll_world as usize,
            None,
        ));
        w.units.map_world_bounds = Some(crate::units::damage::MapWorldBounds {
            min: [-2000.0, -1800.0, -2000.0],
            max: [2000.0, 1800.0, 2000.0],
        });
        f3::f3_init_in(&mut w, coll_world, f64::NAN, 0);
        w.sim.calls.result(2, |row, _| row.copy_from_slice(&[7.0, 0.0]));
        crate::combat::module::cmb_class_in(&mut w, 0);
        w.sim.calls.result(F3UnitItems::WORDS, |r, h| F3UnitItems::default().put(r, h));
        crate::units::f3_items::f3_unit_items_in(&mut w);
        w.sim.store.battle = crate::battle::rules::new_battle(Default::default(), 1.0, 2.0);
        (w, coll_world as usize)
    }

    pub(crate) fn add_synthetic_box(
        w: &mut World,
        coll_world: usize,
        min: [f64; 3],
        max: [f64; 3],
        enabled: bool,
    ) -> (Ref<Body>, u32) {
        let tree = install_bounds_box_tree(&mut w.collision, min, max);
        let body = w.collision.body_new(tree, 0) as u32;
        w.collision
            .body_offset(body, (body as f64 + 1.0) * 16777216.0);
        let io = w.collision.io_mut();
        io[20..23].copy_from_slice(&min);
        io[23..26].copy_from_slice(&max);
        w.collision.body_set(body);
        w.collision.state.worlds[coll_world].order.push(body);
        w.collision.state.worlds[coll_world].version += 1;
        let order = w.collision.state.worlds[coll_world].order.len() - 1;
        w.movement.flags_mut()[order] = u8::from(enabled);
        w.collision.enable_body(body, enabled);
        let slot = Ref::<Body>::new(body, 1);
        let coll = w.fleet.collision.as_mut().unwrap();
        assert!(coll.bind_body(slot, body));
        (slot, body)
    }

    pub(crate) fn add_synthetic_building(
        w: &mut World,
        index: u32,
        team: f64,
        max_health: f64,
        pos: [f64; 3],
        bodies: [Option<Ref<Body>>; 3],
        has_destroyed: bool,
    ) -> F4Handle {
        let mut matrix = Matrix4::default().elements;
        matrix[12] = pos[0];
        matrix[13] = pos[1];
        matrix[14] = pos[2];
        let b_ref = Ref::<Building>::new(index, 1);
        let settings = PbSettings {
            kind: PbKind::Building,
            max_health,
            respawn_time: 999_999.0,
            built_add_health: 0.0,
            regenerates: false,
            has_destroyed,
            has_unbuilt: false,
            has_explosion: false,
            matrix,
            models: vec![
                Some(PbModel {
                    sphere: None,
                    low: Some([-2.0, -2.0, -2.0]),
                    high: Some([2.0, 2.0, 2.0]),
                }),
                has_destroyed.then_some(PbModel {
                    sphere: None,
                    low: Some([-2.0, -2.0, -2.0]),
                    high: Some([2.0, 0.5, 2.0]),
                }),
                None,
            ],
        };
        let state = F3BuildingState {
            index,
            state: PbStateName::Built,
            health: max_health,
            add_health: 0.0,
            attacker: Attacker::Undefined,
            generation: Some(1.0),
            team,
            target_team: team,
            gunner: None,
            trap_mode: false,
            trap_state: TrState::default(),
        };
        let live = F4BuildingBirth {
            settings: settings.clone(),
            state: PbStation {
                building: PbState {
                    state: PbStateName::Built,
                    health: max_health,
                    add_health: 0.0,
                    last_attacker: PbIdentity::default(),
                    target_generation: Some(1.0),
                },
                mode: PbMode::Waiting,
                ..Default::default()
            },
            supply: None,
            crew: false,
            damaged: false,
            attacker: Attacker::Undefined,
        };
        let key = w.sim.strings.intern_str(&format!("l{index}"));
        let setup = F3BuildingSetup {
            index,
            target: RepairTarget::Building(b_ref),
            member: None,
            key,
            settings,
            state,
            live,
            trap: None,
            bodies: bodies.to_vec(),
            explosion: None,
            damaged: false,
            category: 0,
            kind: 0.0,
            trap_sphere: None,
        };
        w.sim
            .calls
            .result(F3BuildingSetup::WORDS, |row, heap| setup.put(row, heap));
        f3_props::f3_building_in(w);
        f3_props::owner_in(w, index as usize)
    }

    pub(crate) fn prepare_synthetic_tail(w: &mut World) {
        w.sim
            .calls
            .result(F4LiveSetup::WORDS, |r, h| F4LiveSetup::default().put(r, h));
        crate::battle::f4_live::f4_live_init_in(w);
        w.sim
            .calls
            .result(F4BattleSetup::WORDS, |r, h| F4BattleSetup::default().put(r, h));
        crate::battle::f4_core::f4_battle_init_in(w);
        let setup = F4TailSetup {
            boundary: BfConfig::default(),
            registry_capacity: w.sim.store.units.list.len() as u32,
            ..Default::default()
        };
        w.sim
            .calls
            .result(F4TailSetup::WORDS, |r, h| setup.put(r, h));
        crate::battle::f4_tail::f4_tail_init_in(w);
        w.sim.calls.result(F4UnitClass::WORDS, |r, h| {
            F4UnitClass {
                captures: true,
                ..Default::default()
            }
            .put(r, h)
        });
        crate::battle::f4_tail::f4_tail_unit_class_in(w);
    }

    /// A prepared vehicle pack with one invented craft: a hull both soldiers
    /// and shots meet, and beside it one box each for ordnance, flyers,
    /// soldiers, and vehicles, buildings and terrain, 10 m apart along x.
    pub(crate) fn invented_hull_pack() -> bfe_content::vehicle_pack::Swbf2VehiclePack {
        let boxed = |name: &str, x: f64, mask: u32| {
            let p: Vec<String> = [[-1, -1, -1], [1, -1, -1], [1, 1, -1], [-1, 1, -1], [-1, -1, 1], [1, -1, 1], [1, 1, 1], [-1, 1, 1]]
                .iter()
                .map(|c| format!("[{},{},{}]", c[0], c[1], c[2]))
                .collect();
            format!(
                r#"{{"name":"{name}","parent":"root","joint":0,"kind":4,"size":[1,1,1],"mask":{mask},"matrix":[1,0,0,0,0,1,0,0,0,0,1,0,{x},0,0,1],"positions":[{}],"indices":[0,2,1,0,3,2,4,5,6,4,6,7,0,1,5,0,5,4,2,3,7,2,7,6,0,4,7,0,7,3,1,2,6,1,6,5]}}"#,
                p.join(",")
            )
        };
        let bodies = [("hull", 0.0, 17), ("shot", 10.0, 16), ("wing", 20.0, 32), ("foot", 30.0, 1), ("bump", 40.0, 14)].map(|(n, x, m)| boxed(n, x, m)).join(",");
        let pack = format!(
            r#"{{"family":"swbf2","version":1,"classes":{{"test_craft":{{"base":"flyer","model":"test_hull","collisionModel":"test_hull"}}}},"models":{{"test_hull":{{"skeleton":[{{"name":"root","parent":null,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],"matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}}],"meshes":[],"primitives":[{bodies}]}}}}}}"#
        );
        bfe_content::vehicle_pack::Swbf2VehiclePack::from_value(&bfe_content::json::parse(&pack).unwrap(), "test/vehicles.json").unwrap()
    }

    #[test]
    fn a_beam_that_passes_through_hurts_each_craft_and_soldier_on_its_way_once_and_ends_at_a_wall() {
        // Two enemy craft, one of the beam's own side between them and an enemy
        // soldier along the beam before the wall, an enemy craft behind it.
        let beam = SpaceProjectile { kind: SpaceOrdnanceKind::Beam, velocity: [0.0, 0.0, 300.0], life_span: 0.5, base_damage: 23.0, ..Default::default() };
        let lost = |bodies: bool, pass_through: bool| {
            let craft = [(2, [0.0, 0.0, 20.0]), (1, [0.5, 0.0, 40.0]), (2, [0.0, 0.5, 60.0]), (2, [0.0, 0.0, 130.0])];
            let mut w = round_scene(SpaceFlightProfile::default(), &craft, bodies, |_| vec![]);
            let soldier = add_synthetic_soldier(&mut w, 2.0, [0.0, 0.0, 75.0]);
            let health = w.space().unwrap().fighters[0].health;
            let struck = let_fly(&mut w, SpaceProjectile { pass_through, ..beam }).join(" ");
            let mut lost: Vec<f64> = w.space().unwrap().fighters.iter().map(|f| health - f.health).collect();
            lost.push(117.0 - w.sim.store.unit.get(soldier).unwrap().combat.health);
            (lost, struck)
        };
        for bodies in [false, true] {
            assert_eq!(lost(bodies, true), (vec![23.0, 0.0, 23.0, 0.0, 23.0], "craft craft craft soldier surface".to_string()));
            assert_eq!(lost(bodies, false), (vec![23.0, 0.0, 0.0, 0.0, 0.0], "craft".to_string()));
        }
    }

    #[test]
    fn a_passing_beam_keeps_ignoring_its_firer_and_every_craft_already_passed() {
        let beam = SpaceProjectile { kind: SpaceOrdnanceKind::Beam, pass_through: true, velocity: [0.0, 0.0, 300.0], life_span: 0.5, base_damage: 31.0, ..Default::default() };
        let enable_ff = |w: &mut World| {
            w.sim.calls.result(crate::gen::F3ImpactRules::WORDS, |r, h| crate::gen::F3ImpactRules { friendly_fire: true }.put(r, h));
            f3::f3_impact_rules_in(w);
        };

        // 1) A ship turret fires past a close soldier at an enemy craft beyond its own body.
        let mut turret_body = 0u32;
        let mut turret_life = 0.0f64;
        let mut w = round_scene(SpaceFlightProfile::default(), &[(2, [0.0, 0.0, 40.0])], true, |w| {
            let cw = f3::collision_world_in(&w.units);
            let (bref, body) = add_synthetic_box(w, cw, [-2.0, -2.0, -1.0], [2.0, 2.0, 10.0], true);
            turret_body = body;
            turret_life = add_synthetic_building(w, 0, 1.0, 2300.0, [0.0, 0.0, 4.5], [Some(bref), None, None], false).life;
            let mut matrix = Matrix4::default().elements;
            matrix[12..15].copy_from_slice(&[0.0, 0.0, 4.5]);
            vec![SpaceBuildingAttachment {
                building_index: 0, instance: 0, name: "turret0".into(), class: "turret".into(),
                base: "armedbuilding".into(), bodies: [Some(body), None, None], animated_index: None,
                matrix, position: [0.0, 0.0, 4.5], center: [0.0, 0.0, 4.5], local_center_offset: [0.0; 3],
                radius: 5.0, sphere: None, visible: true, lock_on_enabled: false, ai_damage_threshold: 0.0,
                damage_category: SpaceDamageCategory::Building, death_explosion: None, last_life: None,
                turret: None, terminal: None,
            }]
        });
        enable_ff(&mut w);
        let soldier = add_synthetic_soldier(&mut w, 2.0, [0.0, 0.0, 3.0]);
        let craft_hp = w.space().unwrap().fighters[0].health;
        let struck = let_fly(&mut w, SpaceProjectile {
            emitter: Some(combat::EmittingBody::Building { index: 0, life: turret_life }),
            ignore_body: Some(turret_body),
            ..beam
        });
        assert_eq!(struck, ["soldier", "craft", "surface"]);
        assert_eq!(2300.0 - w.space_building_snapshot(0).unwrap().health, 0.0);
        assert_eq!(117.0 - w.sim.store.unit.get(soldier).unwrap().combat.health, 31.0);
        assert_eq!(craft_hp - w.space().unwrap().fighters[0].health, 31.0);

        // 2) A gunship fires past a close soldier through its own body toward an enemy craft.
        let mut w = round_scene(SpaceFlightProfile::default(), &[(1, [0.0, 0.0, 5.0]), (2, [0.0, 0.0, 40.0])], true, |_| vec![]);
        enable_ff(&mut w);
        let soldier = add_synthetic_soldier(&mut w, 2.0, [0.0, 0.0, 1.5]);
        let own_gen = w.space().unwrap().fighters[0].generation;
        let own_body = w.space().unwrap().fighters[0].kernel_body;
        let own_hp = w.space().unwrap().fighters[0].health;
        let enemy_hp = w.space().unwrap().fighters[1].health;
        let struck = let_fly(&mut w, SpaceProjectile {
            emitter: Some(combat::EmittingBody::Fighter { index: 0, generation: own_gen }),
            ignore_body: own_body,
            ..beam
        });
        assert_eq!(struck, ["soldier", "craft", "surface"]);
        assert_eq!(own_hp - w.space().unwrap().fighters[0].health, 0.0);
        assert_eq!(117.0 - w.sim.store.unit.get(soldier).unwrap().combat.health, 31.0);
        assert_eq!(enemy_hp - w.space().unwrap().fighters[1].health, 31.0);

        // 3) Two overlapping enemy craft in front of a third craft and a wall.
        let mut w = round_scene(SpaceFlightProfile::default(), &[(2, [0.0, 0.0, 20.0]), (2, [0.0, 0.0, 20.0]), (2, [0.0, 0.0, 50.0])], true, |_| vec![]);
        let hp = w.space().unwrap().fighters[0].health;
        let struck = let_fly(&mut w, beam);
        let lost: Vec<f64> = w.space().unwrap().fighters.iter().map(|f| hp - f.health).collect();
        assert_eq!(struck, ["craft", "craft", "craft", "surface"]);
        assert_eq!(lost, vec![31.0, 31.0, 31.0]);
    }

    #[test]
    fn soldiers_and_other_craft_meet_a_crafts_own_bodies_for_them() {
        // A craft of the invented pack on its pad, and a small one in the air.
        let (mut w, world_id) = make_synthetic_f3_world();
        let mut hull = SpaceFlightProfile { class_name: "test_craft".into(), ..Default::default() };
        attach_vehicle_pack_to_profile(&mut hull, Some(&invented_hull_pack()), &mut w.collision);
        let mut small = SpaceFlightProfile { contact_bounds_min: [-0.4; 3], contact_bounds_max: [0.4; 3], ..Default::default() };
        small.collision_tree = Some(install_bounds_box_tree(&mut w.collision, small.contact_bounds_min, small.contact_bounds_max));
        let profiles = vec![hull, small];
        let pad = SpaceVehiclePad {
            name: "apron".into(), team: 1, profile_index: 0, control_zone: String::new(),
            position: [0.0; 3], yaw: 0.0, matrix: Matrix4::default().elements,
            spawn_count: 1, spawn_time: 1e6, expire_time_enemy: 1e6, expire_time_field: 1e6,
            decay_time: 1e6, respawn_timer: 1e6, floor_y: 0.0,
        };
        let pads = vec![pad.clone(), SpaceVehiclePad { profile_index: 1, ..pad }];
        let fighters: Vec<SpaceFighter> = (0..2)
            .map(|i| {
                let body = w.collision.body_new(profiles[i].collision_tree.unwrap(), 1) as u32;
                w.collision.state.worlds[world_id].order.push(body);
                w.collision.state.worlds[world_id].version += 1;
                let f = SpaceFighter::spawned_on_pad(i, 1, i, &pads[i], i, &profiles[i], 0.0, None, Some(body));
                f.sync_collision_body(&profiles[i], &mut w.collision, world_id, w.movement.flags_mut());
                f
            })
            .collect();
        let bodies = w.collision.state.bodies.len();
        let home = fighters[0].position;
        w.space = Some(Box::new(SpaceBattlefield::from_parts(
            SpaceBounds::from_world_json(&Value::Null), SpaceWorldAnims::default(),
            profiles, pads, fighters, vec![], vec![], world_id, bodies,
        )));
        let names = ["hull", "shot", "wing", "foot", "bump"];
        let at = |i: usize, z: f64| [home[0] + 10.0 * i as f64, home[1], home[2] + z];
        // Which of the five boxes stops a craft drifting at them.
        let mut stops = Vec::new();
        for (i, name) in names.into_iter().enumerate() {
            let f = &mut w.space_mut().unwrap().fighters[1];
            (f.alive, f.state, f.position, f.velocity) = (true, SpaceFlightState::Flying, at(i, -20.0), [0.0, 0.0, 90.0]);
            (f.health, f.contacts) = (f.max_health, 0);
            for _ in 0..40 {
                assert!(w.space_step_tick(SPACE_FIXED_DT));
            }
            if w.space().unwrap().fighters[1].contacts > 0 {
                stops.push(name);
            }
        }
        assert_eq!(stops, ["bump"]);
        w.space_mut().unwrap().fighters[1].position = [0.0, 0.0, 900.0];
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        // Which of them a soldier's walk, tested waist high, meets.
        let floor = |i: usize, z: f64| [at(i, z)[0], home[1] - 1.0, at(i, z)[2]];
        let met: Vec<&str> = (0..5).filter(|&i| w.space_walk_blocker(floor(i, -8.0), floor(i, 8.0), -1).is_some()).map(|i| names[i]).collect();
        assert_eq!(met, ["hull", "foot"]);
    }

    #[test]
    fn infantry_lock_sight_and_online_bolt_casts_during_soldier_step_meet_shot_hulls() {
        use crate::combat::targeting::Targets as _;
        let (mut w, world_id) = make_synthetic_f3_world();
        let mut hull = SpaceFlightProfile { class_name: "test_craft".into(), ..Default::default() };
        attach_vehicle_pack_to_profile(&mut hull, Some(&invented_hull_pack()), &mut w.collision);
        let pad = SpaceVehiclePad {
            name: "apron".into(), team: 1, profile_index: 0, control_zone: String::new(),
            position: [0.0; 3], yaw: 0.0, matrix: Matrix4::default().elements,
            spawn_count: 1, spawn_time: 1e6, expire_time_enemy: 1e6, expire_time_field: 1e6,
            decay_time: 1e6, respawn_timer: 1e6, floor_y: 0.0,
        };
        let body = w.collision.body_new(hull.collision_tree.unwrap(), 1) as u32;
        w.collision.state.worlds[world_id].order.push(body);
        w.collision.state.worlds[world_id].version += 1;
        let f = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &hull, 0.0, None, Some(body));
        f.sync_collision_body(&hull, &mut w.collision, world_id, w.movement.flags_mut());
        let bodies = w.collision.state.bodies.len();
        let home = f.position;
        w.space = Some(Box::new(SpaceBattlefield::from_parts(
            SpaceBounds::from_world_json(&Value::Null), SpaceWorldAnims::default(),
            vec![hull], vec![pad], vec![f], vec![], vec![], world_id, bodies,
        )));
        name_craft(&mut w);
        let target = add_synthetic_soldier(&mut w, 2.0, [0.0, 0.0, 8.0]);
        let names = ["hull", "shot", "wing", "foot", "bump"];
        let at = |i: usize, z: f64| [home[0] + 10.0 * i as f64, home[1], home[2] + z];

        for (flying, expected_shots) in [(false, vec!["hull", "shot"]), (true, vec!["hull", "shot", "wing"])] {
            w.space_mut().unwrap().fighters[0].state = if flying { SpaceFlightState::Flying } else { SpaceFlightState::Landed };
            hulls_meet(&mut w, HullMeets::Soldiers, |w| {
                // Infantry lock-on sight tests shot hulls even inside the soldier phase.
                let sight_blocked: Vec<&str> = (0..5).filter(|&i| {
                    let from = three::Vector3::new(at(i, -8.0)[0], at(i, -8.0)[1], at(i, -8.0)[2]);
                    let to = three::Vector3::new(at(i, 8.0)[0], at(i, 8.0)[1], at(i, 8.0)[2]);
                    !crate::units::f3_sensors::Sensors { w }.sight(&from, &to, Member::Unit(target))
                }).map(|i| names[i]).collect();
                assert_eq!(sight_blocked, expected_shots, "flying={flying}");

                // Moving and full ordnance casts (used by rewound online bolts) test shot hulls too.
                for moving in [false, true] {
                    let hit: Vec<&str> = (0..5).filter(|&i| {
                        crate::units::native::ordnance_cast_in(w, world_id, at(i, -8.0), [0.0, 0.0, 1.0], 16.0, None, false, moving).is_some()
                    }).map(|i| names[i]).collect();
                    assert_eq!(hit, expected_shots, "flying={flying} moving={moving}");
                }

                // Soldier hulls stay restored for the rest of the soldier phase.
                let ray_hit_soldiers: Vec<&str> = (0..5).filter(|&i| {
                    let p = at(i, -8.0);
                    let ray = Ray { ox: p[0], oy: p[1], oz: p[2], dx: 0.0, dy: 0.0, dz: 1.0 };
                    w.collision.ray_hit(world_id as u32, ray, 16.0, -1).is_some()
                }).map(|i| names[i]).collect();
                assert_eq!(ray_hit_soldiers, ["hull", "foot"], "flying={flying}");
            });
        }
    }

    /// Names the craft's bodies as the battlefield's build does.
    fn name_craft(w: &mut World) {
        let space = w.space.take().unwrap();
        name_bodies(w, &space.fighters);
        w.space = Some(space);
    }

    #[test]
    fn a_craft_in_the_way_stops_a_soldiers_shot_and_a_bots_sight() {
        // A parked craft 18 m ahead, the wall 100 m ahead behind it.
        let mut w = round_scene(SpaceFlightProfile::default(), &[(1, [0.0, 0.0, 20.0])], true, |_| vec![]);
        name_craft(&mut w);
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        w.space_mut().unwrap().fighters[0].state = SpaceFlightState::Landed;
        let world_id = f3::collision_world_in(&w.units);
        let body = w.space().unwrap().fighters[0].kernel_body.unwrap() as usize;
        // A shot's cast, past the craft too, and the one a bolt makes each tick
        // for what moves.
        let mut shot = |z: f64, moving: bool| {
            crate::units::native::ordnance_cast_in(&mut w, world_id, [0.0, 0.0, z], [0.0, 0.0, 1.0], 300.0, None, false, moving)
                .map(|hit| (hit.distance, hit.body.is_some()))
        };
        assert_eq!((shot(0.0, false), shot(0.0, true)), (Some((18.0, true)), Some((18.0, true))));
        assert_eq!((shot(30.0, false), shot(30.0, true)), (Some((70.0, true)), None));
        // What a bot's line of sight is blocked by: the craft, not an open way.
        use crate::ai::owned::sight::{World as _, F3};
        let (ahead, up) = (three::Vector3::new(0.0, 0.0, 1.0), three::Vector3::new(0.0, 1.0, 0.0));
        let from = three::Vector3::new(0.0, 0.0, 0.0);
        let struck = w.fleet.collision.as_ref().unwrap().kernel_reference(body);
        assert!(struck.is_some());
        assert_eq!(F3.cast(&mut w, &from, &ahead, 60.0, None), Some(struck));
        assert_eq!(F3.cast(&mut w, &from, &up, 60.0, None), None);
    }

    #[test]
    fn a_bot_aboard_a_craft_sees_past_its_own_craft_and_its_targets_and_no_other() {
        use crate::ai::owned::{entities::{Entities, Entity}, registry::Queries, seats::Controls, sight::{Sight, F3}};
        // A craft flown by each of two bots, 40 m apart, and an empty one that
        // is between them or well clear. The bots' own places do not matter.
        let sees = |empty: [f64; 3], from: f64, looker: bool| {
            let mut w = round_scene(SpaceFlightProfile::default(), &[(1, [0.0, 0.0, 20.0]), (2, [0.0, 0.0, 60.0]), (2, empty)], true, |_| vec![]);
            name_craft(&mut w);
            let (mut entities, controls, queries) = (Entities::new(16), Controls::new(4), Queries::new(16, 2));
            let bots = [0, 1].map(|i| add_synthetic_soldier(&mut w, 1.0 + i as f64, [0.0, 900.0, 40.0 * i as f64]));
            for (i, bot) in bots.into_iter().enumerate() {
                let key = w.space_unit_key(bot).unwrap();
                let owner = w.space_launch_owner(&key).unwrap();
                w.space_mut().unwrap().fighters[i].pilot = Some(SpacePilotId { owner, team: 1 + i as i32, is_human: false, repair_capable: false });
            }
            assert!(w.space_step_tick(SPACE_FIXED_DT));
            let [a, b] = bots.map(|bot| entities.register(Entity::Unit(bot)));
            let (from, to) = (three::Vector3::new(0.0, 0.0, from), three::Vector3::new(0.0, 0.0, 60.0));
            Sight::new(16).los(&mut w, &entities, &controls, &queries, &mut F3, &from, &to, looker.then_some(a), Some(b))
        };
        // From its seat a bot sees the other at its controls, but not past the
        // empty craft; someone on foot does not see through the first craft.
        assert!(sees([0.0, 300.0, 40.0], 20.0, true));
        assert!(!sees([0.0, 0.0, 40.0], 20.0, true));
        assert!(!sees([0.0, 300.0, 40.0], 0.0, false));
    }

    #[test]
    fn a_bot_aboard_a_craft_has_clear_sky_above_its_own_craft_and_not_under_another() {
        use crate::ai::owned::sight::{clear_above, F3};
        let check = |above: [f64; 3], seat: Option<usize>| {
            let mut w = round_scene(SpaceFlightProfile::default(), &[(1, [0.0, 0.0, 20.0]), (2, above)], true, |_| vec![]);
            name_craft(&mut w);
            let bot = add_synthetic_soldier(&mut w, 1.0, [0.0, 900.0, 0.0]);
            let life = w.sim.store.unit.get(bot).unwrap().generation;
            let handle = AiLifeHandle { kind: u32::from(KIND_UNIT), slot: bot.slot, generation: bot.gen, life };
            if let Some(slot) = seat {
                let key = w.space_unit_key(bot).unwrap();
                let owner = w.space_launch_owner(&key).unwrap();
                let crew = SpacePilotId { owner, team: 1, is_human: false, repair_capable: false };
                if slot == 0 {
                    w.space_mut().unwrap().fighters[0].pilot = Some(crew);
                } else {
                    w.space_mut().unwrap().fighters[0].gunners[slot - 1] = Some(crew);
                }
            }
            assert!(w.space_step_tick(SPACE_FIXED_DT));
            let from = three::Vector3::new(0.0, 0.0, 20.0);
            clear_above(&mut w, &mut F3, &from, &handle)
        };
        assert!(check([0.0, 300.0, 20.0], Some(0)));
        assert!(check([0.0, 300.0, 20.0], Some(1)));
        assert!(!check([0.0, 15.0, 20.0], Some(0)));
        assert!(!check([0.0, 300.0, 20.0], None));
    }

    #[test]
    fn a_soldiers_shot_hurts_the_craft_it_strikes_and_a_lost_craft_loses_its_crew() {
        // An enemy craft with a soldier at its controls, a friend's, and a wall.
        let mut w = round_scene(SpaceFlightProfile::default(), &[(2, [0.0, 0.0, 20.0]), (1, [40.0, 0.0, 20.0])], true, |_| vec![]);
        name_craft(&mut w);
        let shooter = add_synthetic_soldier(&mut w, 1.0, [0.0, 0.0, -20.0]);
        let pilot = add_synthetic_soldier(&mut w, 2.0, [0.0, 0.0, 20.0]);
        prepare_synthetic_tail(&mut w);
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        let key = w.space_unit_key(pilot).unwrap();
        let owner = w.space_launch_owner(&key).unwrap();
        let space = w.space_mut().unwrap();
        space.fighters[0].pilot = Some(SpacePilotId { owner, team: 2, is_human: false, repair_capable: false });
        let health = space.fighters[0].health;
        let bodies = [0, 1].map(|i| space.fighters[i].kernel_body.unwrap() as usize);
        let names = bodies.map(|b| w.fleet.collision.as_ref().unwrap().kernel_reference(b).unwrap());
        let shoot = |w: &mut World, name, damage: f64| {
            let packet = crate::combat::impacts::HitPacket { damage, scales: [1.0, 1.0, 1.0, 0.35, 1.0], shield: 1.0 };
            f3::with_services_in(w, |w, services| {
                crate::combat::live::hit_body_packet_in(w, name, packet, crate::gen::OrdnanceOwner::Unit(shooter), None, services.unwrap());
            });
            assert!(w.space_step_tick(SPACE_FIXED_DT));
            let craft = &w.space().unwrap().fighters;
            (health - craft[0].health, health - craft[1].health)
        };
        // By the round's scale for vehicles, at the next step; a friend's craft
        // nothing with friendly fire off.
        assert_eq!(shoot(&mut w, names[0], 60.0), (60.0 * 0.35, 0.0));
        assert_eq!(shoot(&mut w, names[1], 60.0), (60.0 * 0.35, 0.0));
        assert!(!w.sim.store.unit.get(pilot).unwrap().combat.dead);
        // A shot that destroys it kills the crew, to the shooter's credit.
        shoot(&mut w, names[0], health * 10.0);
        let space = w.space().unwrap();
        assert!(!space.fighters[0].alive && space.fighters[0].pilot.is_none());
        let credited = space.events().iter().find_map(|e| match e {
            SpaceEvent::FighterDestroyed { fighter: 0, attacker_unit, .. } => *attacker_unit,
            _ => None,
        });
        assert_eq!(credited.and_then(|owner| crate::combat::owners::resolve(&w.sim.store, owner)), Some(shooter));
        assert!(w.sim.store.unit.get(pilot).unwrap().combat.dead);
        assert!(crate::units::f3::active_in(&w.units));
        // The craft's cockpit is enclosed: its dead pilot stays out of sight,
        // in every replicated snapshot and in the space frame, until his body
        // is gone.
        let hidden = |w: &World| {
            let mut out = Vec::new();
            w.space().unwrap().hidden_crew_units(w, &mut out);
            out
        };
        let life = w.sim.store.unit.get(pilot).unwrap().generation as u32;
        let dead = w.sim.store.unit.get(pilot).unwrap();
        assert!(!crate::units::f3::wire_visible_in(&w.units, &w.fleet, &w.sim.store, pilot, dead));
        assert_eq!(hidden(&w), vec![(pilot.slot, life)]);
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        assert_eq!(w.space().unwrap().hidden_dead_crew, vec![(pilot.slot, life)]);
        w.sim.store.unit.get_mut(pilot).unwrap().removed = true;
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        assert!(w.space().unwrap().hidden_dead_crew.is_empty());
        assert!(hidden(&w).is_empty());
    }

    #[test]
    fn boarding_checks_team_hull_and_life_and_exterior_hull_loss_kills_the_crew() {
        let (mut w, world_id) = make_synthetic_f3_world();
        // As a live battle does, the unit service holds a place for each body.
        f3::reserve_native_slots_in(&mut w, 8);
        let profile = SpaceFlightProfile {
            contact_bounds_min: [-8.0, -1.0, -2.0],
            contact_bounds_max: [8.0, 1.0, 2.0],
            ..Default::default()
        };
        let pad = SpaceVehiclePad {
            name: "apron".into(), team: 1, profile_index: 0, control_zone: String::new(),
            position: [0.0; 3], yaw: std::f64::consts::FRAC_PI_2, matrix: Matrix4::default().elements,
            spawn_count: 1, spawn_time: 6.0, expire_time_enemy: 8.0, expire_time_field: 11.0,
            decay_time: 3.0, respawn_timer: 0.0, floor_y: 0.0,
        };
        let fighter = SpaceFighter::spawned_on_pad(0, 3, 0, &pad, 0, &profile, 0.0, None, None);
        w.space = Some(Box::new(SpaceBattlefield::from_parts(
            SpaceBounds::from_world_json(&Value::Null), SpaceWorldAnims::default(),
            vec![profile], vec![pad], vec![fighter], vec![], vec![], world_id, 0,
        )));
        let mut soldier = crate::units::class::soldier_body(&Value::Null, 100.0, 0.0, 0.0);
        soldier.position = [0.0, 1.0, 8.0];
        let mut unit = Unit {
            alive: true, human: true, team: 2.0, soldier: UnitBody::Soldier(soldier),
            ..Default::default()
        };
        unit.combat.health = 100.0;
        unit.combat.max_health = 100.0;
        let key = f3::with_owner_rows(&mut w, |store, owner| owner.build(store, unit));
        let unit = key.reference();
        prepare_synthetic_tail(&mut w);
        assert!(crate::battle::f4_core::owns_lifecycle_in(&mut w.battle));
        assert!(w.space_board_seat(&key, 0, 0).is_none());
        let rider = w.sim.store.unit.get_mut(unit).unwrap();
        rider.team = 1.0;
        let UnitBody::Soldier(body) = &mut rider.soldier else { unreachable!() };
        body.position = [8.0, 1.0, 0.0];
        assert!(w.space_board_seat(&key, 0, 0).is_none());
        let UnitBody::Soldier(body) = &mut w.sim.store.unit.get_mut(unit).unwrap().soldier else { unreachable!() };
        body.position = [0.0, 1.0, 8.0];
        let seat = w.space_board_seat(&key, 0, 0).unwrap();
        assert_eq!(w.space_seat_for_unit(&key), Some(seat));
        assert!(w.space_seat_fighter(seat).is_some());
        assert!(w.space_seat_weapon(seat, usize::MAX).is_none());
        assert!(!w.space_control_seat(seat, SpacePilotInput { weapon_index: Some(u8::MAX), ..Default::default() }));
        let stale = SpaceSeat { generation: seat.generation - 1, ..seat };
        assert!(!w.space_control_seat(stale, Default::default()));
        assert!(w.space_leave_seat(stale).is_none());
        let mut space = w.space.take().unwrap();
        space.fighters[0].state = SpaceFlightState::Flying;
        let hit = SpaceDamagePacket {
            raw_damage: space.fighters[0].max_health, scales: [1.0; 5], shield_scale: 1.0,
            attacker_unit: None, attacker_team: 2, attacker_is_ai: false,
        };
        assert!(space.damage_fighter_inner(0, &hit, true, &mut w).destroyed);
        assert!(w.sim.store.unit.get(unit).unwrap().combat.dead);
        assert_eq!(space.events().iter().filter(|e| matches!(e,
            SpaceEvent::PilotEjected { pilot_unit, killed_in_vacuum: true, .. } if *pilot_unit == seat.owner
        )).count(), 1);
        w.space = Some(space);
        assert!(w.space_seat_for_unit(&key).is_none());
        assert!(!w.space_control_seat(seat, Default::default()));
    }

    /// A team 1 round with `TEST_BLAST` let fly at the wall, where its blast
    /// goes off 0.2 m before it, at z = 99.8.
    fn burst_at_the_wall(w: &mut World) {
        let round = SpaceProjectile { position: [0.0, 0.0, 92.0], velocity: [0.0, 0.0, 610.0], life_span: 2.0, blast: TEST_BLAST, ..Default::default() };
        assert_eq!(let_fly(w, round), ["surface"]);
    }

    #[test]
    fn a_rounds_blast_reaches_craft_by_their_spheres_and_not_through_a_wall() {
        // Craft whose sphere's centre (1 m ahead of their own) lies 8 m and 14 m
        // beside the blast, one behind the wall and one of the round's own side.
        // Less half their 6 m radius, the two are 5 m and 11 m from it.
        let profile = SpaceFlightProfile { sphere: [0.0, 0.0, 1.0, 6.0], ..Default::default() };
        let craft = [(2, [8.0, 0.0, 98.8]), (2, [0.0, 14.0, 98.8]), (2, [0.0, 0.0, 107.0]), (1, [-8.0, 0.0, 98.8])];
        let mut w = round_scene(profile, &craft, false, |_| vec![]);
        let health = w.space().unwrap().fighters[0].health;
        burst_at_the_wall(&mut w);
        let lost: Vec<f64> = w.space().unwrap().fighters.iter().map(|f| health - f.health).collect();
        assert!((lost[0] - 80.0 * 15.0 / 16.0).abs() < 1e-6 && (lost[1] - 80.0 * 9.0 / 16.0).abs() < 1e-6, "{lost:?}");
        assert_eq!((lost[2], lost[3]), (0.0, 0.0));
        // The two it hurt are impacts of the round's splash.
        let splash = w.space().unwrap().impacts.iter().filter(|i| i.cause == SpaceImpactCause::Splash).count();
        assert_eq!(splash, 2);
    }

    #[test]
    fn a_rounds_blast_among_big_objects_reaches_only_those_whose_middle_it_is_near() {
        // Enemy objects beside the blast: one of 60 m whose surface it touches
        // (its centre 55 m off: 25 m less half the radius), one of 12 m whose
        // centre is 15 m off (9 m), and one of 12 m behind the wall. Nothing of
        // them is in the way of anything.
        let at = [([55.0, 0.0, 99.8], 60.0), ([0.0, -15.0, 99.8], 12.0), ([0.0, 0.0, 109.0], 12.0)];
        let mut w = round_scene(SpaceFlightProfile::default(), &[], false, |w| {
            (0..3).map(|i| SpaceBuildingAttachment { radius: 0.1, sphere: Some([0.0, 0.0, 0.0, at[i].1]), ..round_object(w, i, 2.0, at[i].0, 0.1) }).collect()
        });
        burst_at_the_wall(&mut w);
        let lost: Vec<f64> = (0..3).map(|i| 2300.0 - w.space_building_snapshot(i).unwrap().health).collect();
        assert!((lost[1] - 0.5 * 80.0 * 11.0 / 16.0).abs() < 1e-6, "{lost:?}");
        assert_eq!((lost[0], lost[2]), (0.0, 0.0));
    }

    #[test]
    fn a_rounds_blast_hurts_the_soldiers_it_reaches_and_a_wall_shields_them() {
        let mut w = round_scene(SpaceFlightProfile::default(), &[], false, |_| vec![]);
        let near = add_synthetic_soldier(&mut w, 2.0, [7.0, 0.0, 99.0]);
        let behind = add_synthetic_soldier(&mut w, 2.0, [0.0, 0.0, 104.0]);
        let friend = add_synthetic_soldier(&mut w, 1.0, [-7.0, 0.0, 99.0]);
        let far = add_synthetic_soldier(&mut w, 2.0, [0.0, 30.0, 60.0]);
        burst_at_the_wall(&mut w);
        let health = |r| w.sim.store.unit.get(r).unwrap().combat.health;
        assert!(health(near) < 117.0 - 0.6 * 80.0 && health(near) > 117.0 - 80.0, "{}", health(near));
        assert_eq!((health(behind), health(friend), health(far)), (117.0, 117.0, 117.0));
    }

    #[test]
    fn an_inner_radius_only_blast_hurts_craft_and_soldiers_within_it() {
        let blast = SpaceExplosionSpec { damage: 50.0, inner_radius: 12.0, outer_radius: 0.0, scales: [1.0; 5], shield_scale: 1.0 };
        let mut w = round_scene(SpaceFlightProfile::default(), &[(2, [8.0, 0.0, 99.8]), (2, [25.0, 0.0, 99.8])], false, |_| vec![]);
        let soldier = add_synthetic_soldier(&mut w, 2.0, [0.0, 8.0, 99.0]);
        let craft_hp = w.space().unwrap().fighters[0].health;
        let round = SpaceProjectile { position: [0.0, 0.0, 92.0], velocity: [0.0, 0.0, 610.0], life_span: 2.0, blast, ..Default::default() };
        assert_eq!(let_fly(&mut w, round), ["surface"]);
        assert_eq!(craft_hp - w.space().unwrap().fighters[0].health, 50.0);
        assert_eq!(craft_hp - w.space().unwrap().fighters[1].health, 0.0);
        assert_eq!(117.0 - w.sim.store.unit.get(soldier).unwrap().combat.health, 50.0);
    }

    #[test]
    fn event_buffer_overflow_sets_flag_and_counts_dropped_events() {
        let mut bf = SpaceBattlefield::from_parts(
            SpaceBounds::from_world_json(&Value::Null),
            SpaceWorldAnims::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            0,
            0,
        );
        let cap = bf.events.capacity();
        assert_eq!(cap, MAX_SPACE_EVENTS);
        for i in 0..(cap + 7) {
            bf.push_event(SpaceEvent::FighterTakeoff {
                fighter: i,
                generation: 1,
                pilot_unit: None,
            });
        }
        assert_eq!(bf.events.len(), cap);
        assert!(bf.events_overflowed);
        assert_eq!(bf.dropped_events, 7);
    }

    #[test]
    fn friendly_fire_policy_is_consistent_for_disabled_and_enabled_modes() {
        assert_eq!(friendly_fire_scale(1, false, 1, false, false), 0.0);
        assert!(friendly_fire_scale(1, false, 1, false, true) > 0.0);
        assert_eq!(friendly_fire_scale(1, false, 2, false, false), 1.0);
        assert_eq!(friendly_fire_scale(1, true, 2, true, false), 1.0);
    }

    #[test]
    fn impacts_name_the_emitter_weapon_target_and_outcome() {
        let (mut w, coll_world) = make_synthetic_f3_world();
        let (core_ref, core_body) = add_synthetic_box(&mut w, coll_world, [8.0, 0.0, -2.0], [12.0, 4.0, 2.0], true);
        let (_, wall_body) = add_synthetic_box(&mut w, coll_world, [-12.0, 0.0, -2.0], [-8.0, 4.0, 2.0], true);
        add_synthetic_building(&mut w, 0, 2.0, 430.0, [10.0, 2.0, 0.0], [Some(core_ref), None, None], false);
        let att = SpaceBuildingAttachment {
            building_index: 0,
            instance: 0,
            name: "core".into(),
            class: "core_class".into(),
            base: "destructablebuilding".into(),
            bodies: [Some(core_body), None, None],
            animated_index: None,
            matrix: { let mut m = Matrix4::default().elements; m[12] = 10.0; m[13] = 2.0; m },
            position: [10.0, 2.0, 0.0],
            center: [10.0, 2.0, 0.0],
            local_center_offset: [0.0; 3],
            radius: 3.0,
            sphere: None,
            visible: true,
            lock_on_enabled: false,
            ai_damage_threshold: 0.0,
            damage_category: SpaceDamageCategory::Building,
            death_explosion: None,
            last_life: None,
            turret: None,
            terminal: None,
        };
        let bf = SpaceBattlefield::from_parts(
            SpaceBounds::from_world_json(&Value::Null), SpaceWorldAnims::default(),
            Vec::new(), Vec::new(), Vec::new(), vec![att], Vec::new(), coll_world, w.collision.state.bodies.len(),
        );
        w.space = Some(Box::new(bf));
        prepare_synthetic_tail(&mut w);
        // Craft 3's second weapon fires at the ship object, its first at a bare wall.
        let shooter = combat::EmittingBody::Fighter { index: 3, generation: 7 };
        for (dx, weapon) in [(1.0, 1u8), (-1.0, 0u8)] {
            w.space_mut().unwrap().ordnance.spawn(SpaceProjectile {
                active: true,
                owner_team: 1,
                emitter: Some(shooter),
                weapon_index: weapon,
                position: [0.0, 2.0, 0.0],
                velocity: [610.0 * dx, 0.0, 0.0],
                life_span: 2.0,
                base_damage: 47.0,
                ..Default::default()
            });
        }
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        let space = w.space().unwrap();
        assert_eq!(space.impacts_dropped, 0);
        let core = space.impacts.iter().find(|i| i.target == SpaceImpactTarget::Building { index: 0 }).expect("the round on the ship object");
        assert_eq!((core.cause, core.emitter, core.team), (SpaceImpactCause::Round { weapon: 1 }, Some(shooter), 1));
        assert_eq!(core.hull_loss, 47.0);
        assert!(!core.destroyed);
        let wall = space.impacts.iter().find(|i| i.cause == SpaceImpactCause::Round { weapon: 0 }).expect("the round on the wall");
        assert_eq!(wall.target, SpaceImpactTarget::Surface { body: wall_body as i32 });
        assert_eq!(space.body_owner(core_body as i32), SpaceBodyOwner::Building(0));
        assert_eq!(space.body_owner(wall_body as i32), SpaceBodyOwner::Other);
        assert_eq!(space.body_owner(-1), SpaceBodyOwner::Other);
        // The log is per tick.
        assert!(w.space_step_tick(SPACE_FIXED_DT));
        assert!(w.space().unwrap().impacts.is_empty());
    }

    /// Ship object `index` of `team` at `at`, met as a sphere of `radius`.
    fn round_object(w: &mut World, index: usize, team: f64, at: [f64; 3], radius: f64) -> SpaceBuildingAttachment {
        add_synthetic_building(w, index as u32, team, 2300.0, at, [None; 3], false);
        let mut matrix = Matrix4::default().elements;
        matrix[12..15].copy_from_slice(&at);
        SpaceBuildingAttachment {
            building_index: index,
            instance: index,
            name: format!("object{index}"),
            class: "object".into(),
            base: "destructablebuilding".into(),
            bodies: [None; 3],
            animated_index: None,
            matrix,
            position: at,
            center: at,
            local_center_offset: [0.0; 3],
            radius,
            sphere: None,
            visible: true,
            lock_on_enabled: false,
            ai_damage_threshold: 0.0,
            damage_category: SpaceDamageCategory::Building,
            death_explosion: None,
            last_life: None,
            turret: None,
            terminal: None,
        }
    }

    /// A wall across z = 100 (2 m wide and high), empty craft (team, place) in
    /// flight, with bodies of their own (boxes 4 m wide) or without, and ship
    /// objects. Friendly fire is off.
    fn round_scene(mut profile: SpaceFlightProfile, craft: &[(i32, [f64; 3])], bodies: bool, objects: impl FnOnce(&mut World) -> Vec<SpaceBuildingAttachment>) -> Box<World> {
        let (mut w, coll_world) = make_synthetic_f3_world();
        w.sim.calls.result(crate::gen::F3ImpactRules::WORDS, |r, h| crate::gen::F3ImpactRules { friendly_fire: false }.put(r, h));
        f3::f3_impact_rules_in(&mut w);
        f3::reserve_native_slots_in(&mut w, 8);
        add_synthetic_box(&mut w, coll_world, [-1.0, -1.0, 100.0], [1.0, 1.0, 101.0], true);
        let buildings = objects(&mut w);
        profile.collision_tree = bodies.then(|| install_bounds_box_tree(&mut w.collision, [-2.0; 3], [2.0; 3]));
        let pad = SpaceVehiclePad {
            name: "apron".into(), team: 1, profile_index: 0, control_zone: String::new(),
            position: [0.0; 3], yaw: 0.0, matrix: Matrix4::default().elements,
            spawn_count: 1, spawn_time: 1e6, expire_time_enemy: 1e6, expire_time_field: 1e6,
            decay_time: 1e6, respawn_timer: 1e6, floor_y: 0.0,
        };
        let pads: Vec<SpaceVehiclePad> = craft.iter().map(|(team, _)| SpaceVehiclePad { team: *team, ..pad.clone() }).collect();
        let fighters = pads.iter().enumerate().map(|(i, p)| {
            let body = profile.collision_tree.map(|tree| {
                let body = w.collision.body_new(tree, 1) as u32;
                w.collision.state.worlds[coll_world].order.push(body);
                w.collision.state.worlds[coll_world].version += 1;
                body
            });
            let mut f = SpaceFighter::spawned_on_pad(i, 1, i, p, 0, &profile, 0.0, None, body);
            (f.state, f.position, f.velocity) = (SpaceFlightState::Flying, craft[i].1, [0.0; 3]);
            f.sync_collision_body(&profile, &mut w.collision, coll_world, w.movement.flags_mut());
            f
        }).collect();
        w.space = Some(Box::new(SpaceBattlefield::from_parts(
            SpaceBounds::from_world_json(&Value::Null), SpaceWorldAnims::default(),
            vec![profile], pads, fighters, buildings, vec![], coll_world, w.collision.state.bodies.len(),
        )));
        prepare_synthetic_tail(&mut w);
        w
    }

    /// A soldier of `team` whose middle is at `at`.
    fn add_synthetic_soldier(w: &mut World, team: f64, at: [f64; 3]) -> crate::store::Ref<Unit> {
        let mut body = crate::units::class::soldier_body(&Value::Null, 117.0, 0.0, 0.0);
        body.position = [at[0], at[1] - 0.9, at[2]];
        let mut unit = Unit { alive: true, team, soldier: UnitBody::Soldier(body), ..Default::default() };
        (unit.combat.health, unit.combat.max_health) = (117.0, 117.0);
        (unit.bounds.center, unit.bounds.radius) = (at, 0.9);
        f3::with_owner_rows(w, |store, owner| owner.build(store, unit)).reference()
    }

    /// Lets a team 1 round fly until it is gone, and what it struck.
    fn let_fly(w: &mut World, round: SpaceProjectile) -> Vec<&'static str> {
        w.space_mut().unwrap().ordnance.spawn(SpaceProjectile { active: true, owner_team: 1, ..round });
        let mut struck = Vec::new();
        for _ in 0..120 {
            assert!(w.space_step_tick(SPACE_FIXED_DT));
            struck.extend(w.space().unwrap().impacts.iter().filter(|i| matches!(i.cause, SpaceImpactCause::Round { .. })).map(|i| match i.target {
                SpaceImpactTarget::Fighter { .. } => "craft",
                SpaceImpactTarget::Soldier => "soldier",
                SpaceImpactTarget::Building { .. } => "object",
                SpaceImpactTarget::Surface { .. } => "surface",
            }));
            if w.space().unwrap().ordnance.active_count() == 0 {
                break;
            }
        }
        assert_eq!(w.space().unwrap().ordnance.active_count(), 0);
        struck
    }

    const TEST_BLAST: SpaceExplosionSpec = SpaceExplosionSpec {
        damage: 80.0, inner_radius: 4.0, outer_radius: 20.0, scales: [1.0, 1.0, 1.0, 1.0, 0.5], shield_scale: 1.0,
    };

    #[test]
    fn a_round_strikes_the_first_thing_on_its_way_and_sets_off_its_blast_there() {
        // An enemy craft, soldier and ship object before the wall, whichever
        // of them there are, and an enemy craft beside where the round meets it.
        let round = SpaceProjectile { velocity: [0.0, 0.0, 610.0], life_span: 2.0, base_damage: 23.0, blast: TEST_BLAST, ..Default::default() };
        let strike = |bodies: bool, craft: bool, soldier: bool, object: bool| {
            let ahead = if craft { [0.0, 0.0, 20.0] } else { [0.0, 300.0, 20.0] };
            let mut w = round_scene(SpaceFlightProfile::default(), &[(2, ahead), (2, [8.0, 0.0, 99.0])], bodies, |w| {
                if object { vec![round_object(w, 0, 2.0, [0.0, 0.0, 75.0], 5.0)] } else { vec![] }
            });
            let soldier = soldier.then(|| add_synthetic_soldier(&mut w, 2.0, [0.0, 0.0, 45.0]));
            let health = w.space().unwrap().fighters[0].health;
            let struck = let_fly(&mut w, round).join(" ");
            let craft = &w.space().unwrap().fighters;
            let hurt = [
                health - craft[0].health,
                soldier.map_or(0.0, |s| 117.0 - w.sim.store.unit.get(s).unwrap().combat.health),
                if object { 2300.0 - w.space_building_snapshot(0).unwrap().health } else { 0.0 },
                health - craft[1].health,
            ];
            (struck, hurt.map(|h| h > 0.0))
        };
        for bodies in [false, true] {
            assert_eq!(strike(bodies, true, true, true), ("craft".into(), [true, false, false, false]));
            assert_eq!(strike(bodies, false, true, true), ("soldier".into(), [false, true, false, false]));
            assert_eq!(strike(bodies, false, false, true), ("object".into(), [false, false, true, false]));
            assert_eq!(strike(bodies, false, false, false), ("surface".into(), [false, false, false, true]));
        }
    }

    #[test]
    fn building_damage_uses_the_same_human_flag_on_the_body_and_its_launch() {
        let mut w = World::new();
        let pilot = w.sim.store.unit.insert(Unit { human: true, alive: true, ..Default::default() });
        let key = F3UnitKey::of(pilot, w.sim.store.unit.get(pilot).unwrap());
        let launch = w.space_launch_owner(&key).unwrap();
        assert!(!attacker_is_ai(&w, Attacker::Unit(pilot)));
        assert!(!attacker_is_ai(&w, Attacker::OwnerSnapshot(launch)));
        w.sim.store.unit.get_mut(pilot).unwrap().human = false;
        assert!(attacker_is_ai(&w, Attacker::Unit(pilot)));
        assert!(!attacker_is_ai(&w, Attacker::OwnerSnapshot(launch)));
        let bot_launch = w.space_launch_owner(&key).unwrap();
        assert!(attacker_is_ai(&w, Attacker::OwnerSnapshot(bot_launch)));
    }

    #[test]
    fn single_owner_f3_props_shields_clamp_no_passive_event_and_emits_object_killed() {
        struct Recorder(Rc<RefCell<Vec<Event>>>, Rc<RefCell<bool>>);
        impl EventRules for Recorder {
            fn tick(&mut self, _: f64, batch: &EventQueue, actions: &mut dyn RuleActions) {
                self.0.borrow_mut().extend_from_slice(batch.events());
                if let Ok(handle) = actions.find_object("synth_core") {
                    if actions
                        .set_property(&handle, ObjectProperty::LockOn(true))
                        .is_ok()
                    {
                        *self.1.borrow_mut() = true;
                    }
                }
            }
        }

        let (mut w, coll_world) = make_synthetic_f3_world();
        let (built_ref, built_k) = add_synthetic_box(&mut w, coll_world, [8.0, 0.0, -2.0], [12.0, 4.0, 2.0], true);
        let (dest_ref, dest_k) = add_synthetic_box(&mut w, coll_world, [8.0, 0.0, -2.0], [12.0, 1.0, 2.0], false);
        let owner = add_synthetic_building(
            &mut w,
            0,
            2.0,
            400.0,
            [10.0, 2.0, 0.0],
            [Some(built_ref), Some(dest_ref), None],
            true,
        );

        let att = SpaceBuildingAttachment {
            building_index: 0,
            instance: 0,
            name: "synth_core".into(),
            class: "synth_reactor".into(),
            base: "destructablebuilding".into(),
            bodies: [Some(built_k), Some(dest_k), None],
            animated_index: None,
            matrix: {
                let mut m = Matrix4::default().elements;
                m[12] = 10.0;
                m[13] = 2.0;
                m
            },
            position: [10.0, 2.0, 0.0],
            center: [10.0, 2.0, 0.0],
            local_center_offset: [0.0; 3],
            radius: 3.0,
            sphere: None,

            visible: true,
            lock_on_enabled: false,
            ai_damage_threshold: 0.0,
            damage_category: SpaceDamageCategory::Building,
            death_explosion: None,
            last_life: None,
            turret: None,
            terminal: None,
        };
        let bf = SpaceBattlefield::from_parts(
            SpaceBounds::from_world_json(&Value::Null),
            SpaceWorldAnims::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![att],
            Vec::new(),
            coll_world,
            w.collision.state.bodies.len(),
        );
        w.space = Some(Box::new(bf));

        // Attach EventRules handler through the public rule factory seam (no edits to battle/).
        prepare_synthetic_tail(&mut w);
        let captured = Rc::new(RefCell::new(Vec::<Event>::new()));
        let prop_set = Rc::new(RefCell::new(false));
        let cap_c = Rc::clone(&captured);
        let prop_c = Rc::clone(&prop_set);
        w.register_rule_factory(move |family, mode, _params| {
            if family == bfe_family::GameFamily::Swbf2 && mode == "synth_space" {
                Ok(Box::new(Recorder(Rc::clone(&cap_c), Rc::clone(&prop_c))))
            } else {
                Err(RuleError::NotAvailable)
            }
        });
        let named = vec![NamedObject {
            object: ObjectHandle::Prop(owner.clone()),
            name: Some("synth_core".into()),
            class: Some("synth_reactor".into()),
        }];
        crate::battle::f4_tail::select_rules(
            &mut w,
            bfe_family::GameFamily::Swbf2,
            "synth_space",
            crate::battle::rules::ModeParameters {
                command_limit: 16,
                timer_limit: 1,
                score_limit: None,
                time_limit: None,
            },
            named,
        )
        .expect("select synthetic space rules");

        // A complete ordnance batch can strike one canonical building in one tick.
        for _ in 0..MAX_SPACE_PROJECTILES {
            w.space_mut().unwrap().plant_timebomb(
                [10.0, 2.0, 0.0], None, 1, false, SPACE_FIXED_DT, 0.25, 1.0, 2.0,
            ).unwrap();
        }
        for _ in 0..7 { assert!(w.space_step_tick(SPACE_FIXED_DT)); }
        assert_eq!(w.space_building_snapshot(0).unwrap().health, 400.0 - MAX_SPACE_PROJECTILES as f64 * 0.25);
        w.space_set_building_property(0, ObjectProperty::Health(400.0)).unwrap();

        w.space_set_building_property(0, ObjectProperty::CurrentShield(200.0)).unwrap();
        w.space_set_building_property(0, ObjectProperty::MaxShield(200.0)).unwrap();
        assert_eq!(w.space_building_shields(0), Some((200.0, 200.0, 0.0)));
        w.space_set_building_property(0, ObjectProperty::CurrentShield(100.0)).unwrap();
        w.space_set_building_property(0, ObjectProperty::AddShield(20.0)).unwrap();

        // Passive AddShield recharge must advance canonical shields WITHOUT
        // flooding SpaceEvent::ShieldChanged.
        for _ in 0..30 {
            assert!(w.space_step_tick(SPACE_FIXED_DT));
            assert!(
                !w.space()
                    .unwrap()
                    .events()
                    .iter()
                    .any(|e| matches!(e, SpaceEvent::ShieldChanged { .. })),
                "passive AddShield recharge must not emit ShieldChanged"
            );
        }
        let snap1 = w.space_building_snapshot(0).unwrap();
        assert!(snap1.cur_shield > 109.0 && snap1.cur_shield <= 200.0);

        let character = w.sim.store.character.insert(crate::gen::Character { team: 1.0, human: true, ..Default::default() });
        let unit = w.sim.store.unit.insert(crate::gen::Unit { alive: true, human: true, team: 1.0, character: Some(character), ..Default::default() });
        let key = w.space_unit_key(unit).unwrap();
        let owner_at_launch = w.space_launch_owner(&key).unwrap();
        w.space_mut().unwrap().plant_timebomb([9.0,1.0,0.0], Some(owner_at_launch), 1, false, 0.5, 900.0, 6.0, 12.0).unwrap();
        w.sim.store.unit.remove(unit).unwrap();
        let other = w.sim.store.character.insert(crate::gen::Character { team: 2.0, ..Default::default() });
        let replacement = w.sim.store.unit.insert(crate::gen::Unit { alive: true, team: 2.0, character: Some(other), ..Default::default() });
        assert_eq!(replacement.slot, unit.slot);
        assert_ne!(replacement.gen, unit.gen);
        assert!(w.space_launch_owner(&key).is_none());
        for _ in 0..35 {
            assert!(w.space_step_tick(SPACE_FIXED_DT));
        }
        let tail_in = F4TailInput { dt: SPACE_FIXED_DT, server: true, ..Default::default() };
        let bank = crate::battle::f4_live::tail_input_bank_mut_in(&mut w).unwrap();
        tail_in.put(bank, &mut Vec::new());
        w.tick.running = true;
        crate::battle::f4_tail::f4_tail_in(&mut w);
        w.tick.running = false;

        let snap_dead = w.space_building_snapshot(0).unwrap();
        assert!(!snap_dead.alive);
        assert_eq!(snap_dead.health, 0.0);
        assert_eq!(snap_dead.cur_shield, 0.0);
        assert_eq!(w.space_body_enabled(built_k), Some(false));
        assert_eq!(w.space_body_enabled(dest_k), Some(true));
        let recorded = captured.borrow();
        let mut kills = recorded.iter().filter_map(|event| match event {
            Event::ObjectKilled { object, killer } if *object == ObjectHandle::Prop(owner.clone()) => Some(*killer),
            _ => None,
        });
        assert_eq!(kills.next(), Some(Some(character)));
        assert_eq!(kills.next(), None);
        assert!(w.space_repair_building(0, 400.0, 2) > 0.0);
        let rebuilt = w.space_building_snapshot(0).unwrap();
        assert_ne!(rebuilt.owner.life, owner.life);
        assert_eq!(w.space_building_handle(0), Some(ObjectHandle::Prop(rebuilt.owner)));
        assert!(*prop_set.borrow(), "RuleActions::set_property(LockOn) must route into space attachment");
        assert!(w.space_building_snapshot(0).unwrap().lock_on_enabled);
    }

    /// What an empty enemy craft at `at` loses to a team 1 round let fly.
    fn lost_to_round(at: [f64; 3], round: SpaceProjectile) -> f64 {
        let mut w = round_scene(SpaceFlightProfile::default(), &[(2, at), (1, [0.0, 0.0, -900.0])], true, |_| vec![]);
        let health = w.space().unwrap().fighters[0].health;
        let_fly(&mut w, round);
        health - w.space().unwrap().fighters[0].health
    }

    #[test]
    fn a_round_bursts_at_the_end_of_its_life_by_the_blast_its_class_names_for_it_or_vanishes() {
        // 0.41 s at 400 m/s: 164 m, 24.6 ticks. The craft is 3 m beside its end.
        let at = [3.0, 30.0, 164.0];
        let round = SpaceProjectile { position: [0.0, 30.0, 0.0], velocity: [0.0, 0.0, 400.0], life_span: 0.41, ..Default::default() };
        assert_eq!(lost_to_round(at, SpaceProjectile { expire_blast: TEST_BLAST, ..round }), 80.0);
        assert_eq!(lost_to_round(at, SpaceProjectile { blast: TEST_BLAST, ..round }), 0.0);
        // A round that lives 93 m does not reach the wall at 100 m, though its
        // last tick would; one that strikes it does not burst by the other blast.
        let at = [3.0, 0.0, 99.8];
        let round = SpaceProjectile { velocity: [0.0, 0.0, 610.0], life_span: 0.1525, ..Default::default() };
        assert_eq!(lost_to_round(at, SpaceProjectile { blast: TEST_BLAST, ..round }), 0.0);
        assert_eq!(lost_to_round(at, SpaceProjectile { life_span: 0.3, expire_blast: TEST_BLAST, ..round }), 0.0);
        assert_eq!(lost_to_round(at, SpaceProjectile { life_span: 0.3, blast: TEST_BLAST, ..round }), 80.0);
    }

    #[test]
    fn a_guided_rocket_goes_up_at_the_end_of_its_life_and_when_its_pilot_is_gone() {
        let at = [3.0, 30.0, 164.0];
        let critical = SpaceExplosionSpec { damage: 400.0, ..TEST_BLAST };
        let rocket = SpaceProjectile {
            kind: SpaceOrdnanceKind::GuidedRocket, position: [0.0, 30.0, 0.0], velocity: [0.0, 0.0, 400.0], life_span: 0.41,
            guided_boost_speed: 400.0, guided_boost_threshold: 0.8, blast: TEST_BLAST, guided_blast_boost: critical, ..Default::default()
        };
        // Past its boost threshold it goes up by the critical blast, short of
        // it by the plain one.
        assert_eq!(lost_to_round(at, rocket), 400.0);
        assert_eq!(lost_to_round(at, SpaceProjectile { guided_boost_speed: 600.0, ..rocket }), 80.0);
        // One whose pilot's seat is empty goes up where it is, by the plain one.
        let orphan = SpaceProjectile { position: [0.0, 30.0, 164.0], life_span: 8.0, guided_pilot_fighter: Some((1, 1, 0)), ..rocket };
        assert_eq!(lost_to_round(at, orphan), 80.0);
    }

    #[test]
    fn a_deselected_craft_weapon_drops_the_rest_of_its_shot() {
        // Two pilot weapons: weapon 0 fires four salvos 0.25 s apart; weapon 1
        // is a second gun on the same seat. Pulling weapon 0 for one tick then
        // staying on it finishes all four salvos; switching to weapon 1 drops
        // the three that remain.
        let w0 = SpaceWeaponSpec {
            salvo_count: 4, shots_per_salvo: 1, salvo_delay: 0.25, salvo_delay_max: 0.25,
            shot_delay: 0.5, velocity: 400.0, life_span: 1.0, max_damage: 10.0, seat: 0, channel: 0,
            ..Default::default()
        };
        let w1 = SpaceWeaponSpec { salvo_count: 1, shot_delay: 0.5, seat: 0, channel: 1, ..w0.clone() };
        let run = |switch_to: Option<u8>, initial_delay: f64| -> u32 {
            let profile = SpaceFlightProfile {
                weapons: vec![SpaceWeaponSpec { initial_salvo_delay: initial_delay, ..w0.clone() }, w1.clone()],
                ..Default::default()
            };
            let mut w = round_scene(profile, &[(1, [0.0, 0.0, -50.0])], false, |_| vec![]);
            let pilot = add_synthetic_soldier(&mut w, 1.0, [0.0, 900.0, 0.0]);
            let key = w.space_unit_key(pilot).unwrap();
            let owner = w.space_launch_owner(&key).unwrap();
            let f = &mut w.space_mut().unwrap().fighters[0];
            f.pilot = Some(SpacePilotId { owner, team: 1, is_human: true, repair_capable: false });
            f.pending_input = SpacePilotInput { weapon_index: Some(0), fire_primary: true, ..Default::default() };
            assert!(w.space_step_tick(SPACE_FIXED_DT));
            let f = &mut w.space_mut().unwrap().fighters[0];
            f.pending_input = SpacePilotInput { weapon_index: switch_to, fire_primary: false, ..Default::default() };
            for _ in 0..90 {
                assert!(w.space_step_tick(SPACE_FIXED_DT));
            }
            w.space().unwrap().fighters[0].weapons[0].shots_fired
        };
        assert_eq!(run(Some(0), 0.0), 4);
        assert_eq!(run(Some(1), 0.0), 1);
        assert_eq!(run(Some(1), 0.2), 0);
    }

    /// A gun of team 1 that fires on its own at a craft of team 2 flying
    /// about 290 m away, ridden by a human or by a bot: how far, in radians, the
    /// gun's barrel lies off the craft's exact lead after a second.
    fn gun_lies_off_the_lead(human: bool) -> f64 {
        let profile = SpaceFlightProfile { bounds_min: [-6.0, -2.0, -6.0], bounds_max: [6.0, 2.0, 6.0], max_speed: 15.0, mid_speed: 10.0, ..Default::default() };
        let gun = |w: &mut World| {
            add_synthetic_building(w, 0, 1.0, 900.0, [0.0; 3], [None; 3], false);
            vec![SpaceBuildingAttachment {
                building_index: 0, instance: 0, name: "gun".into(), class: "gun".into(), base: "armedbuilding".into(),
                bodies: [None; 3], animated_index: None, matrix: Matrix4::default().elements, position: [0.0; 3],
                center: [0.0; 3], local_center_offset: [0.0; 3], radius: 2.0, sphere: None, visible: true,
                lock_on_enabled: false, ai_damage_threshold: 0.0, damage_category: SpaceDamageCategory::Building,
                death_explosion: None, last_life: None, terminal: None,
                turret: Some(SpaceTurretState {
                    pilot_type: SpaceTurretPilotType::SelfPiloted,
                    weapon: SpaceWeaponSpec {
                        shot_delay: 0.5, velocity: 700.0, life_span: 1.0, max_damage: 1.0,
                        ai_bubble: aim::MissBubble { size: 0.5, divider: 60.0, clamp: 4.0 },
                        ..Default::default()
                    },
                    filter: TurretTargetFilter { target_person: false, target_vehicle: true, target_building: false, min_range: 5.0, max_range: 800.0 },
                    yaw: 0.0, pitch: 0.0, prev_yaw: 0.0, prev_pitch: 0.0,
                    yaw_min: -std::f64::consts::PI, yaw_max: std::f64::consts::PI, pitch_min: -1.5, pitch_max: 1.5,
                    turn_rate: 60.0, pitch_rate: 60.0,
                    clock: Default::default(), barrage_remaining: 0, barrage_timer: 0.0, trigger_was_held: false,
                    warm_up: 0.0, heat: 0.0, overheated: false, shots_fired: 0, muzzle_offsets: Vec::new(),
                    remote_operator: None, pending_remote_aim: None, pending_remote_fire: false,
                    pending_remote_spread_scale: None, kick: Default::default(),
                }),
            }]
        };
        let mut w = round_scene(profile, &[(2, [-260.0, 20.0, 120.0])], false, gun);
        let rider = add_synthetic_soldier(&mut w, 2.0, [0.0, 900.0, 0.0]);
        let key = w.space_unit_key(rider).unwrap();
        let owner = w.space_launch_owner(&key).unwrap();
        w.space_mut().unwrap().fighters[0].pilot = Some(SpacePilotId { owner, team: 2, is_human: human, repair_capable: false });
        for _ in 0..60 {
            let f = &mut w.space_mut().unwrap().fighters[0];
            f.pending_input = SpacePilotInput { throttle: 1.0, ..Default::default() };
            assert!(w.space_step_tick(SPACE_FIXED_DT));
        }
        let space = w.space().unwrap();
        let (f, gun) = (&space.fighters[0], &space.buildings[0]);
        let turret = gun.turret.as_ref().unwrap();
        let barrel = objects::barrel_direction(turret, &gun.matrix);
        let exact = lead_target_direction(gun.center, f.position, f.velocity, turret.weapon.round_speed()).unwrap();
        v8math::acos((barrel[0] * exact[0] + barrel[1] * exact[1] + barrel[2] * exact[2]).clamp(-1.0, 1.0))
    }

    #[test]
    fn a_ship_gun_aims_off_a_crafts_lead_by_the_crafts_sphere_of_miss_and_less_when_bots_ride_it() {
        let human = gun_lies_off_the_lead(true);
        let bot = gun_lies_off_the_lead(false);
        // A craft 12 m square hands its sphere 4.2 m, doubled as a diameter
        // and again as a flyer, half of that for this weapon, four times at
        // 290 m: at least 0.4 of evasion puts the barrel metres off the point
        // ahead of the craft.
        assert!(human > 0.01, "the gun leads a human's craft exactly ({human} rad)");
        assert!((bot / human - aim::BOT_TARGET_SHARE).abs() < 0.05, "bot {bot} rad against human {human} rad");
    }

    #[test]
    fn remote_turret_ring_and_team_zero_read_canonical_f3_props() {
        let (mut w, coll_world) = make_synthetic_f3_world();
        let o0 = add_synthetic_building(&mut w, 0, 1.0, 200.0, [0.0; 3], [None; 3], false);
        let o1 = add_synthetic_building(&mut w, 1, 1.0, 200.0, [20.0, 0.0, 0.0], [None; 3], false);
        let o2 = add_synthetic_building(&mut w, 2, 1.0, 200.0, [-20.0, 0.0, 0.0], [None; 3], false);
        let o3 = add_synthetic_building(&mut w, 3, 1.0, 500.0, [0.0, 0.0, -10.0], [None; 3], false);
        let o4 = add_synthetic_building(&mut w, 4, 1.0, 500.0, [4.0, 0.0, -10.0], [None; 3], false);

        let make_tur = |idx: usize, name: &str, _owner: F4Handle, pt: SpaceTurretPilotType, pos: [f64; 3]| {
            let mut m = Matrix4::default().elements;
            m[12] = pos[0];
            m[13] = pos[1];
            m[14] = pos[2];
            SpaceBuildingAttachment {
                building_index: idx,
                instance: idx,
                name: name.into(),
                class: "synth_tur".into(),
                base: "armedbuilding".into(),
                bodies: [None; 3],
                animated_index: None,
                matrix: m,
                position: pos,
                center: pos,
                local_center_offset: [0.0; 3],
                radius: 2.0,
                sphere: None,

                visible: true,
                lock_on_enabled: false,
                ai_damage_threshold: 0.0,
                damage_category: SpaceDamageCategory::Building,
                death_explosion: None,
                last_life: None,
                turret: Some(SpaceTurretState {
                    pilot_type: pt,
                    weapon: SpaceWeaponSpec {
                        shot_delay: 0.2,
                        velocity: 600.0,
                        life_span: 1.0,
                        max_damage: 100.0,
                        ..Default::default()
                    },
                    filter: TurretTargetFilter {
                        target_person: true,
                        target_vehicle: true,
                        target_building: false,
                        min_range: 5.0,
                        max_range: 500.0,
                    },
                    yaw: 0.0,
                    pitch: 0.0,
                    prev_yaw: 0.0,
                    prev_pitch: 0.0,
                    yaw_min: -std::f64::consts::PI,
                    yaw_max: std::f64::consts::PI,
                    pitch_min: -0.5,
                    pitch_max: 0.8,
                    turn_rate: 4.0,
                    pitch_rate: 4.0,
                    clock: Default::default(),
                    barrage_remaining: 0,
                    barrage_timer: 0.0,
                    trigger_was_held: false,
                    warm_up: 0.0,
                    heat: 0.0,
                    overheated: false,
                    shots_fired: 0,
                    muzzle_offsets: Vec::new(),
                    remote_operator: None,
                    pending_remote_aim: None,
                    pending_remote_fire: false,
                    pending_remote_spread_scale: None,
                    kick: Default::default(),
                }),
                terminal: None,
            }
        };
        let make_chair = |idx: usize, name: &str, _owner: F4Handle, rem: usize, nxt: usize| SpaceBuildingAttachment {
            building_index: idx,
            instance: idx,
            name: name.into(),
            class: "synth_chair".into(),
            base: "remoteterminal".into(),
            bodies: [None; 3],
            animated_index: None,
            matrix: Matrix4::default().elements,
            position: [0.0; 3],
            center: [0.0; 3],
            local_center_offset: [0.0; 3],
            radius: 1.5,
            sphere: None,

            visible: true,
            lock_on_enabled: false,
            ai_damage_threshold: 0.0,
            damage_category: SpaceDamageCategory::Building,
            death_explosion: None,
            last_life: None,
            turret: None,
            terminal: Some(SpaceTerminalState {
                remote_object_name: String::new(),
                next_terminal_name: String::new(),
                prev_terminal_name: String::new(),
                remote_turret: Some(rem),
                next_terminal: Some(nxt),
                prev_terminal: Some(nxt),
                occupant: None,
                seat: None,
                feet: None,
            }),
        };

        let bf = SpaceBattlefield::from_parts(
            SpaceBounds::from_world_json(&Value::Null),
            SpaceWorldAnims::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![
                make_tur(0, "auto_t", o0, SpaceTurretPilotType::SelfPiloted, [0.0; 3]),
                make_tur(1, "rem_a", o1, SpaceTurretPilotType::Remote, [20.0, 0.0, 0.0]),
                make_tur(2, "rem_b", o2, SpaceTurretPilotType::Remote, [-20.0, 0.0, 0.0]),
                make_chair(3, "chair_a", o3, 1, 4),
                make_chair(4, "chair_b", o4, 2, 3),
            ],
            Vec::new(),
            coll_world,
            w.collision.state.bodies.len(),
        );
        w.space = Some(Box::new(bf));

        // Add one canonical enemy soldier in range so auto-turret engages.
        let enemy = w.sim.store.unit.insert(Unit {
            alive: true,
            team: 2.0,
            soldier: UnitBody::Soldier(crate::units::class::soldier_body(
                &bfe_content::Value::Null,
                100.0,
                0.0,
                0.0,
            )),
            ..Default::default()
        });
        if let UnitBody::Soldier(ref mut s) = w.sim.store.unit.get_mut(enemy).unwrap().soldier {
            s.position = [0.0, 0.0, 80.0];
        }

        for _ in 0..30 {
            assert!(w.space_step_tick(SPACE_FIXED_DT));
        }
        let shots = w.space().unwrap().buildings[0].turret.as_ref().unwrap().shots_fired;
        assert!(shots > 0);

        // Team = 0 via canonical ObjectProperty silences auto-turret immediately.
        w.space_set_building_property(0, ObjectProperty::Team(0.0)).unwrap();
        for _ in 0..30 {
            assert!(w.space_step_tick(SPACE_FIXED_DT));
        }
        assert_eq!(w.space().unwrap().buildings[0].turret.as_ref().unwrap().shots_fired, shots);

        // Remote turret chair ring cycling.
        let marine = w.sim.store.unit.insert(crate::gen::Unit {
            generation: 4., alive: true, human: true, team: 1.,
            soldier: UnitBody::Soldier(crate::units::class::soldier_body(&Value::Null, 100.,0.,0.)),
            ..Default::default()
        });
        // A player finds a chair by the same rule: a neutral chair of a neutral gun takes
        // either side, and one with a team, or whose gun has one, takes its own.
        let finds = |w: &World| [1, 2].map(|side| local::terminal::nearby(w, side, [0.0; 3]).is_some());
        for (gun, chair, admits) in [(0.0, 0.0, [true, true]), (2.0, 0.0, [false, true]), (0.0, 2.0, [false, true]), (1.0, 0.0, [true, false]), (1.0, 1.0, [true, false])] {
            for (index, team) in [(1, gun), (2, gun), (3, chair), (4, chair)] {
                w.space_set_building_property(index, ObjectProperty::Team(team)).unwrap();
            }
            assert_eq!(finds(&w), admits, "guns of team {gun}, chairs of team {chair}");
        }
        let key = w.space_unit_key(marine).unwrap();
        let seat = w.space_board_terminal(&key, 3).unwrap();
        assert_eq!(seat.turret, 1);
        assert!(w.space_control_terminal(seat, [0.,0.,1.], true));
        for _ in 0..10 { assert!(w.space_step_tick(SPACE_FIXED_DT)); }
        assert!(w.space().unwrap().buildings[1].turret.as_ref().unwrap().shots_fired > 0);
        let next = w.space_cycle_terminal(seat, true).unwrap();
        assert_eq!((next.chair,next.turret),(4,2));
        assert!(!w.space_control_terminal(seat, [0.,0.,1.], true));
        assert!(w.space_control_terminal(next, [0.,0.,1.], true));
        for _ in 0..10 { assert!(w.space_step_tick(SPACE_FIXED_DT)); }
        assert!(w.space().unwrap().buildings[2].turret.as_ref().unwrap().shots_fired > 0);
        assert!(w.space_leave_terminal(next));
        assert!(!w.space_control_terminal(next, [0.,0.,1.], true));
    }

    #[test]
    fn parse_vehicle_pads_maps_every_fighter_slot_to_its_own_pad() {
        let mut p0 = SpaceFlightProfile::default();
        p0.class_name = "alpha_fighter".into();
        let mut p1 = SpaceFlightProfile::default();
        p1.class_name = "beta_bomber".into();
        let profiles = vec![p0, p1];

        let make_pad = |index: usize, name: &str, cls: &str, count: &str, pos: [f64; 3]| {
            bfe_content::map::PlacedObject {
                index,
                layer: None,
                name: name.into(),
                class: "vehiclespawn".into(),
                base: "vehiclespawn".into(),
                model: None,
                door_animation: None,
                destroyed_model: None,
                unbuilt_model: None,
                matrix: Matrix4::default().elements,
                position: pos,
                team: 1,
                radius: None,
                collision: None,
                destroyed_collision: None,
                unbuilt_collision: None,
                visible: false,
                properties: vec![
                    ("ClassRepATK".into(), cls.into()),
                    ("SpawnCount".into(), count.into()),
                    ("SpawnTime".into(), "12.5".into()),
                ],
                settings: Value::Null,
                raw: Value::Null,
            }
        };
        let placed = vec![
            make_pad(0, "pad_left", "alpha_fighter", "2", [-10.0, 2.0, 5.0]),
            make_pad(1, "pad_right", "alpha_fighter", "3", [10.0, 2.0, 5.0]),
            make_pad(2, "pad_bomber", "beta_bomber", "1", [0.0, 2.0, 20.0]),
        ];

        let mut collision = CollisionOwner::new();
        let (pads, fighter_pad_indices) = parse_vehicle_pads(&placed, &profiles, &mut collision, 0);
        assert_eq!(pads.len(), 3);
        assert_eq!(fighter_pad_indices, vec![0, 0, 1, 1, 1, 2]);
        assert_eq!(pads[0].position, [-10.0, 2.0, 5.0]);
        assert_eq!(pads[1].position, [10.0, 2.0, 5.0]);
        assert_eq!(pads[2].profile_index, 1);
        assert!((pads[0].spawn_time - 12.5).abs() < 1e-9);
    }
}
