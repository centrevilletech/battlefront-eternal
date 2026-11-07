//! Vehicles.update owns hull physics, retained rigs and collision placements.
//! F2 keeps mutable hulls in the generated fleet slab and lends reserved working
//! rows to the solvers. Native unit/combat services share that same fleet rather
//! than importing bodies or recreating collision queries during a tick.
//!
//! Vehicle.damage commits before crew death, Character credit and immediate
//! explosions. The current hull is reloaded after recursive work, so a nested
//! blast cannot be lost when the outer movement step resumes. The combined
//! world span passes its existing service borrow through these hooks.

pub mod f2;

use super::class::HullClass;
use super::hull::{Hull, Input};
use super::native::{Collider, Collision, MapRules};
use super::pose::HullPose;
use super::posing;
use crate::flight::model::FlightInput;
use crate::flight::state::GearClip;
use crate::flight::config::Tunings;
use crate::flight::{Config, Environment, Flight, FlightClass, FlightWorld, Member, Team, Tuning, Zone};
use bfe_content::content::VehicleTemplate;
use bfe_content::value::Value;
use super::{BodyId, Ground, Hit, Polygon, VehicleWorld};
use crate::gen::*;
use crate::store::{Ref, StrId};
use std::collections::HashMap;
use three::{Matrix4, Quaternion, Vector3};

/// vehicles.js FIXED_STEP: a ground hull's substep.
const FIXED_STEP: f64 = 1.0 / 120.0;

/// A ground hull's pose kept in the module: the life it was taken for, the
/// pose, and its seats' turn rates (seatView's turnRate, the 9-pose's).
struct Posed {
    generation: f64,
    pose: HullPose,
    turn_rates: Vec<f64>,
}
/// Vehicles owns cold definitions, retained rigs and each independent vehicle-phase bank.
/// Canonical bodies stay in Store. Entry services project only the field they
/// need, so a committed hull can be observed by a nested crew death without
/// borrowing its caller's scratch or replacing another publication bank.
pub(crate) struct Fleet {
    classes: Option<HashMap<String, HullClass>>,
    // VehiclesWorld collision, map rules and lifecycle exchange are separate
    // from retained class/pose banks and from each independent flight clock.
    pub(crate) collision: Option<Collision>,
    pub(crate) map: Option<MapRules>,
    input: Vec<f64>,
    changes: Vec<f64>,
    templates: Option<HashMap<String, VehicleTemplate>>,
    defs: Option<HashMap<String, Value>>,
    poses: Vec<Option<Posed>>,
    clock: Option<f64>,
    bytes: Vec<u8>,
    flight_env: Option<Environment>,
    tunings: Option<Tunings>,
    // FlightWorld receives ordered registry facts; aim points stay lazy until
    // targeting asks, and replacing registry facts invalidates only the cache.
    members: Vec<MemberRow>,
    relations: Vec<f64>,
    teams: Vec<Team>,
    flight_inputs: Vec<Option<(FlightInput, Option<FlightInput>)>>,
    flight_keys: Option<HashMap<String, (Config, Option<GearClip>)>>,
    flight_acc: f64,
    locks: Vec<f64>,
    aim_cache: Vec<Option<Vector3>>,
    aim_wanted: Vec<f64>,
    phase_refs: Vec<Ref<Vehicle>>,
    phase_mine: Vec<(usize, Ref<Vehicle>)>,
    phase_posed: Vec<bool>,
    phase_locks: Vec<Option<crate::flight::state::Lock>>,
    scheduler: f2::Scheduler,
    seats: Option<f2::live_seats::State>,
    firing: Option<f2::firing::State>,
    riders: f2::riders::Riders,
    geometry: f2::geometry::Geometry,
}
impl Fleet {
    pub(crate) const fn new() -> Self {
        Self {
            classes: None,
            collision: None,
            map: None,
            input: Vec::new(),
            changes: Vec::new(),
            templates: None,
            defs: None,
            poses: Vec::new(),
            clock: None,
            bytes: Vec::new(),
            flight_env: None,
            tunings: None,
            members: Vec::new(),
            relations: Vec::new(),
            teams: Vec::new(),
            flight_inputs: Vec::new(),
            flight_keys: None,
            flight_acc: 0.0,
            locks: Vec::new(),
            aim_cache: Vec::new(),
            aim_wanted: Vec::new(),
            phase_refs: Vec::new(),
            phase_mine: Vec::new(),
            phase_posed: Vec::new(),
            phase_locks: Vec::new(),
            scheduler: f2::Scheduler::new(), seats: None, firing: None,
            riders: f2::riders::Riders::new(), geometry: f2::geometry::Geometry::new(),
        }
    }
}
impl Fleet {
    /// Borrow the canonical crew owner while no detached seat solver is active.
    pub(crate) fn seats_owner(&mut self) -> &mut crate::vehicles::f2_seats::Seats {
        f2::live_seats::seats_owner(self)
    }
}

// A detached solver row keeps its scratch while callbacks reborrow the
// battle. Ordinary helpers borrow disjoint fleet banks directly.

/// A registry entry as JavaScript gives it each phase: a unit's (its aim
/// point asked when wanted), a hull's (alive, aim point and body read from
/// the hull as it is when asked) or another's (a turret's, a building's:
/// its aim point as it stands).
#[derive(Clone, Copy, Debug)]
struct MemberRow {
    kind: u8,
    id: f64,
    generation: Option<f64>,
    alive: bool,
    team: Team,
    hull: u32,
    point: Vector3,
}

const MEMBER_UNIT: u8 = 0;
const MEMBER_HULL: u8 = 1;
/// An entry that stands for a hull without being its target (its aim point
/// and body the hull's).
const MEMBER_ON_HULL: u8 = 3;

/// Borrow the collision service beside its immutable geometry and flag banks.
pub(crate) fn collision_owner(fleet:&mut Fleet)->Option<&mut Collision> {fleet.collision.as_mut()}
pub(crate) fn map_ref(fleet:&Fleet)->Option<&MapRules> {fleet.map.as_ref()}

// Spatial readers borrow their prepared bank for the duration of a query.
pub(crate) fn collision_world_in(world_ctx: &mut crate::world::owner::World) -> Option<&mut Collision> {
    world_ctx.fleet.collision.as_mut()
}

pub(crate) fn map_in(world_ctx: &mut crate::world::owner::World) -> Option<&mut MapRules> {
    world_ctx.fleet.map.as_mut()
}

/// Pilot steering and the fleet observe the same immutable flight facts.
pub(crate) fn flight_environment_in(world_ctx: &mut crate::world::owner::World)->&Environment {
    world_ctx.fleet.flight_env.as_ref().expect("the flight environment (veh_flight_env)")
}

// navigateMissile and the hull phase share FLIGHT_CLASSES' live tunings.
// Copy its numeric row; F1 does not create or cache a second tuning table.
pub(crate) fn flight_tuning_in(world_ctx: &mut crate::world::owner::World, class: FlightClass) -> Tuning { Fleet::flight_tuning(&mut world_ctx.fleet.tunings, class) }

/// EntityWorldCapture's mount sample reads the retained walk clocks without
/// updating a rig or publishing a geometry row back through JavaScript.
pub(crate) fn wire_walk_in(fleet:&Fleet, reference: Ref<Vehicle>) -> (f64, f64) { Fleet::wire_walk(&fleet.poses, reference) }
/// captureGroundCorrection observes aim history after the completed pose
/// pass. Reading it must not call aimSeats or sample the rig a second time.
pub(crate) fn wire_seat_history_in(fleet:&Fleet, reference: Ref<Vehicle>, index: usize) -> Option<(f64, Option<f64>)> { Fleet::wire_seat_history(&fleet.poses, reference, index) }

/// The hull classes' keys the phase has been given (`veh_class`), by name.

/// A hull class: its name and its definition (`data.classes[name]`) as JSON,
/// both interned strings, and the turn rate its hulls have (its seats' aim
/// rates fall back on it).

pub(crate) fn veh_class_in(world_ctx: &mut crate::world::owner::World, name: u32, json: u32, turn_rate: f64) { Fleet::veh_class(&world_ctx.sim.strings, &mut world_ctx.fleet.classes, name, json, turn_rate) }

/// The map's data the hulls ask about, as JSON (`water`: its cell, offset,
/// grid and the cell numbers its Set holds, or null; `deathRegions` and
/// `mission.deathRegions` as the map has them), and the kill height under
/// the battlefield (game.js killY): the water and the death regions are
/// answered here from then on.

pub(crate) fn veh_map_in(world_ctx: &mut crate::world::owner::World, json: u32, kill_y: f64) { Fleet::veh_map(&world_ctx.sim.strings, &mut world_ctx.fleet.map, json, kill_y) }

/// The battle's collision as the module holds it: the kernel's world
/// (collision.js rayWorld) and the vehicle-only barriers' fixed set (-1
/// none). The world is required; body mappings stay with its lifetime.

pub(crate) fn veh_collision_in(world_ctx: &mut crate::world::owner::World, world: i32, blockers: i32) { Fleet::veh_collision(&mut world_ctx.fleet.collision, world, blockers); #[cfg(target_arch = "wasm32")]
    if let Some(c) = world_ctx.fleet.collision.as_mut() { c.use_world_flags(); } }

/// Where JavaScript writes each body's enabled flag, by its place in the
/// collision world's order (MAX_BODIES bytes; 0 without a world here).

pub(crate) fn veh_flags_in(world_ctx: &mut crate::world::owner::World) -> Option<&mut [u8]> {
    if world_ctx.fleet.collision.is_some() { Some(&mut world_ctx.movement.flags_mut()[..]) } else { None }
}

/// Room for `count` numbers JavaScript hands over (veh_bodies, veh_collider).

pub(crate) fn veh_input_in(world_ctx: &mut crate::world::owner::World, count: u32) -> Option<&mut [f64]> { Fleet::veh_input(&mut world_ctx.fleet.input, count); Some(&mut world_ctx.fleet.input) }

/// The bodies' numbers: `count` pairs in the input, each a body's number in
/// the store (the bridge's) and its kernel slot.

pub(crate) fn veh_bodies_in(world_ctx: &mut crate::world::owner::World, count: u32) {
    let Some(c) = world_ctx.fleet.collision.as_mut() else { return };
    let v = &mut world_ctx.fleet.input;
    for i in 0..count as usize {
        c.name_body(v[2 * i] as u32, v[2 * i + 1] as u32);
    }
}

/// A hull's body: in the input its number in the store, its kernel slot,
/// its bounds in its own frame (min, max), then the gather caches (kernel
/// ids, -1 none) of its `walls` wall probes and of its `springs` springs.

pub(crate) fn veh_collider_in(world_ctx: &mut crate::world::owner::World, walls: u32, springs: u32) {
    let Some(c) = world_ctx.fleet.collision.as_mut() else { return };
    let v = &mut world_ctx.fleet.input;
    let (w, n) = (walls as usize, springs as usize);
    let header = F2ColliderHeader::take(v, &[]);
    let collider = Collider {
        kernel: header.kernel,
        local_bounds: [header.min[0],header.min[1],header.min[2],header.max[0],header.max[1],header.max[2]],
        walls: v[F2ColliderHeader::WORDS..F2ColliderHeader::WORDS + w].iter().map(|&x| x as i64).collect(),
        springs: v[F2ColliderHeader::WORDS + w..F2ColliderHeader::WORDS + w + n].iter().map(|&x| x as i64).collect(),
    };
    c.add_collider(header.body, collider);
    if f2::active_in(&world_ctx.fleet) { prepare_collision_in(world_ctx); }
}

// CollisionWorld construction reserves the largest changed-body publication.
// A phase can disable every hull or move every body before its next mirror
// barrier; first damage must not grow this output reservation.
pub(crate) fn prepare_collision_in(world_ctx: &mut crate::world::owner::World) {
    let Some(c) = world_ctx.fleet.collision.as_mut() else { return };
    let state = &mut world_ctx.collision.state;
    c.prepare(state);
    let words = F2BodyHeader::WORDS + (state.worlds[c.world].order.len() + state.worlds[c.world].prepared.len())
        * (F2BodyToggle::WORDS + F2BodyPlaced::WORDS);
    let changes = &mut world_ctx.fleet.changes;
    changes.reserve(words.saturating_sub(changes.len()));
}

/// The flags and places the hulls changed since the last call, for
/// JavaScript's bodies (Collision::take_changes); returns their address, or
/// 0 without a collision world here.

pub(crate) fn veh_changes_in(world_ctx: &mut crate::world::owner::World) -> Option<&[f64]> {
    let Some(c) = world_ctx.fleet.collision.as_mut() else { return None };
    let out = &mut world_ctx.fleet.changes;
    c.take_changes_in(&mut world_ctx.collision.state, world_ctx.movement.flags_mut(), out);
    Some(&out[..])
}

/// Room for `count` bytes JavaScript hands over (the vehicle pack).

pub(crate) fn veh_bytes_in(world_ctx: &mut crate::world::owner::World, count: u32) -> Option<&mut [u8]> { Fleet::veh_bytes(&mut world_ctx.fleet.bytes, count); Some(&mut world_ctx.fleet.bytes) }

/// The vehicle pack, from `veh_bytes`'s room: its JSON (`def_len` bytes:
/// its models, collision, animations and classes), then (from the next
/// multiple of 4) its models' `floats` floats and `indices` indices and its
/// animations' `animations` floats. The hulls' rigs are built from it as
/// the content crate builds them (posing.rs). Returns how many models.

pub(crate) fn veh_pack_in(world_ctx: &mut crate::world::owner::World, def_len: u32, floats: u32, indices: u32, animations: u32) -> u32 {
    let bytes = &world_ctx.fleet.bytes;
    let def = bfe_content::json::parse_bytes(&bytes[..def_len as usize]).unwrap_or_else(|e| panic!("vehicle pack: {e:?}"));
    let mut at = (def_len as usize).div_ceil(4) * 4;
    let mut words = |n: u32| {
        let out: Vec<[u8; 4]> = bytes[at..at + n as usize * 4].chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
        at += n as usize * 4;
        out
    };
    let model_floats: Vec<f32> = words(floats).into_iter().map(f32::from_le_bytes).collect();
    let model_indices: Vec<u32> = words(indices).into_iter().map(u32::from_le_bytes).collect();
    let anim: std::rc::Rc<[f32]> = words(animations).into_iter().map(f32::from_le_bytes).collect();
    let made = posing::templates(&def, &model_floats, &model_indices, anim).unwrap_or_else(|e| panic!("vehicle pack: {e}"));
    let count = made.len() as u32;
    let classes: HashMap<String, Value> = def.get("classes").and_then(Value::as_object).map(|o| o.entries().map(|(k, v)| (k.to_string(), v.clone())).collect()).unwrap_or_default();
    {
        world_ctx.fleet.templates = Some(made.into_iter().map(|t| (t.name.clone(), t)).collect());
        world_ctx.fleet.defs = Some(classes);
        // The pack's bytes are not needed again.
        world_ctx.fleet.bytes = Vec::new();
    }
    count
}

/// A hull's pose as the constructor makes it from its class and
/// model (build.rs), with its seats' turn rates, or None where the pack
/// lacks either.
fn new_pose_in(world_ctx: &mut crate::world::owner::World, name: &str) -> Option<(HullPose, Vec<f64>)> { Fleet::new_pose(&mut world_ctx.fleet.defs, &mut world_ctx.fleet.templates, name) }

/// A hull's whole pose state from JavaScript (posing::pull), `count`
/// numbers in the input, for the hull in store slot `slot` on life
/// `generation`: its pose is the module's from now until its next life.
/// `seed`: the world spawned this hull before its pose was the module's (a
/// world's fleet at load), so the module seeds its spawn pose itself
/// (Hull::seed) over the pulled state. Returns 1 on adoption; 0 rejects a
/// missing pack or mismatched node count.

pub(crate) fn veh_pull_in(world_ctx: &mut crate::world::owner::World, slot: u32, generation: f64, count: u32, seed: u32) -> u32 {
    let s = &mut world_ctx.sim;
    // Vehicle construction may prepare a private pad variant before it
    // joins fleet order. The slab already owns its generation-qualified row.
    let Some((r, _)) = s.store.vehicle.at(slot) else { return 0 };
    let name = String::from_utf16_lossy(s.strings.units(s.store.vehicle.get(r).expect("a hull the store holds").name));

    if world_ctx.fleet.poses.len() <= slot as usize {
        world_ctx.fleet.poses.resize_with(slot as usize + 1, || None);
    }
    // The rig is the class's: made once a hull, kept over its lives.
    let made = match world_ctx.fleet.poses[slot as usize].take() {
        Some(p) => Some((p.pose, p.turn_rates)),
        None => new_pose_in(world_ctx, &name),
    };
    let Some((mut pose, turn_rates)) = made else { return 0 };
    let ok = posing::pull(&mut pose, &world_ctx.fleet.input[..count as usize]);
    let mut pose = if ok && seed != 0 { seed_pose_in(world_ctx, r, pose) } else { pose };
    if f2::active_in(&world_ctx.fleet) { pose.rig.tree.update_matrix_world(pose.rig.root, true); }
    world_ctx.fleet.poses[slot as usize] = Some(Posed { generation: if ok { generation } else { f64::NAN }, pose, turn_rates });
    ok as u32
}

/// A spawned hull's seed pose (Hull::seed) over its pulled pose: the hull as
/// the store holds it after its spawn, its class's and, for a craft, its
/// flight's keys. No barrel has fired before a hull's first life. A class
/// the phase was not given keeps the pulled pose.
fn seed_pose_in(world_ctx: &mut crate::world::owner::World, r: Ref<Vehicle>, pose: HullPose) -> HullPose {
    let s = &mut world_ctx.sim;
    let v = s.store.vehicle.get(r).expect("a hull the store holds");
    let name = String::from_utf16_lossy(s.strings.units(v.name));
    let Some(class) = world_ctx.fleet.classes.get_or_insert_with(HashMap::new).get(&name) else { return pose };
    let index = s.store.vehicles.vehicles.iter().position(|&h| h == r).unwrap_or(0);
    let mut hull = Hull::from_store(r, v, class, index);
    if hull.flying {
        read_flight_in(world_ctx, &mut hull);
    }
    hull.pose = Some(pose);
    hull.seed(0.0);
    hull.pose.take().expect("the seeded pose")
}

fn opt(flag: f64, x: f64) -> Option<f64> {
    (flag != 0.0).then_some(x)
}

fn team(flag: f64, x: f64) -> Team {
    opt(flag, x)
}

/// The battle's flight environment as JSON (Vehicles.flightEnvironment:
/// ceiling, bounds, space, landingZones) and its fly height; the assists
/// come with each phase.

pub(crate) fn veh_flight_env_in(world_ctx: &mut crate::world::owner::World, json: u32, max_fly_height: f64) { Fleet::veh_flight_env(&world_ctx.sim.strings, &mut world_ctx.fleet.flight_env, &mut world_ctx.fleet.tunings, json, max_fly_height) }

/// FLIGHT_CLASSES[class][key] = value (a class's tuning: at attach, and
/// as a world's flightTuning command retunes it); 1 if it took.

pub(crate) fn veh_flight_tune_in(world_ctx: &mut crate::world::owner::World, class: u32, key: u32, value: f64) -> u32 { Fleet::veh_flight_tune(&world_ctx.sim.strings, &mut world_ctx.fleet.tunings, class, key, value) }

/// The registry's entries this phase (units.all(), in its order), ten
/// numbers each in the input: kind (0 unit, 1 hull, 2 other), id,
/// generation (NaN: undefined), alive, whether it has a team and the team,
/// a hull's slot in the store, and another's aim point.

pub(crate) fn veh_members_in(world_ctx: &mut crate::world::owner::World, count: u32) { Fleet::veh_members(&mut world_ctx.fleet.input, &mut world_ctx.fleet.members, &mut world_ctx.fleet.aim_cache, count) }

/// The relation of every team among the entries and hulls to every other
/// (Units.relation), `count` teams in the input as (has a team, team)
/// pairs, then the count x count relations, row by row.

pub(crate) fn veh_relations_in(world_ctx: &mut crate::world::owner::World, count: u32) { Fleet::veh_relations(&mut world_ctx.fleet.input, &mut world_ctx.fleet.teams, &mut world_ctx.fleet.relations, count) }

/// Lifecycle flight input layout, also used by independent native fixtures.
fn flight_input(v: &[f64]) -> FlightInput {
    FlightInput {
        forward: v[0],
        right: v[1],
        turn: v[2],
        pitch: v[3],
        up: v[4],
        down: v[5],
        takeoff: v[6],
        stick_x: opt(v[7], v[8]),
        stick_y: opt(v[9], v[10]),
        boost: v[11],
        trick: v[12],
        immelmann: v[13],
        roll_lock: v[14],
        target_lock: v[15],
        bot: v[16],
        response_curve: v[17],
        no_assist: v[18] != 0.0,
    }
}

/// Each craft's pilot input this phase, 40 numbers each in the input: its
/// slot in the store, whether a network pilot's keys follow, seat 0's
/// frozen input (19) and the network pilot's keys (19).

pub(crate) fn veh_flight_inputs_in(world_ctx: &mut crate::world::owner::World, count: u32) { Fleet::veh_flight_inputs(&mut world_ctx.fleet.input, &mut world_ctx.fleet.flight_inputs, count) }

/// The crafts' time left over after the last phase's flight steps.

pub(crate) fn veh_flight_acc_in(world_ctx: &mut crate::world::owner::World) -> f64 {
    world_ctx.fleet.flight_acc
}

/// What the crafts' targeting left in their weapons this phase: [count,
/// then per craft its slot, the lock's target id, generation, elapsed time
/// and whether it is complete]. Returns its address.

pub(crate) fn veh_locks_in(world_ctx: &mut crate::world::owner::World) -> Option<&[f64]> {
    Some(&world_ctx.fleet.locks[..])
}

/// A class's flight keys (flightConfig's own, from its definition) and its
/// takeoff clip (the gear's), from the pack.

/// predictionVehicle resolved immutable content once per source. The client
/// gets an independent hull, row, strings and query view; it never takes a
/// working row out of the authoritative phase's reentrant pool.
pub(crate) fn prediction_hull_in(world_ctx: &mut crate::world::owner::World, r: Ref<Vehicle>) -> Option<crate::client::hulls::Session> {
    let s = &mut world_ctx.sim;
    // F2 normally interns these enum messages at fleet construction. A page
    // has only resource rows, so resolve the same IDs before cloning strings.
    // Published message handles must remain valid in the shared bridge table.
    let _ = crate::flight::store::FlightTextIds::new(&mut s.strings);
    let v = s.store.vehicle.get(r)?;
    let name = String::from_utf16_lossy(s.strings.units(v.name));
    let class = world_ctx.fleet.classes.get_or_insert_with(HashMap::new).get(&name)?;
    let mut hull = Hull::from_store(r, v, class, 0);
    let (config, gear) = if f2::active_in(&world_ctx.fleet) { f2::flight_keys_for_in(&world_ctx.fleet, r) } else { Fleet::flight_keys(&mut world_ctx.fleet.flight_keys, &mut world_ctx.fleet.defs, &mut world_ctx.fleet.templates, &name) };
    hull.flight = Flight::from_store(v, &s.strings, config, gear).ok()?;
    hull.pose = Some(world_ctx.fleet.poses.get(r.slot as usize)?.as_ref()?.pose.clone());
    Some(crate::client::hulls::Session::new(hull, v.clone(), s.strings.clone(),
        world_ctx.fleet.collision.as_mut()?.prediction_view(), world_ctx.fleet.map.as_mut()?.clone(),
        world_ctx.fleet.flight_env.as_ref()?.clone(),
        *world_ctx.fleet.tunings.as_ref()?))
}

/// A craft as the phase works on it: the hull with its flight.
fn load_flyer_in(world_ctx: &mut crate::world::owner::World, r: Ref<Vehicle>, index: usize) -> Hull {
    let mut hull = load_in(world_ctx, r, index);
    read_flight_in(world_ctx, &mut hull);
    hull
}

/// A craft's flight from the store.
pub(crate) fn read_flight_in(world_ctx: &mut crate::world::owner::World, hull: &mut Hull) {
    let s = &mut world_ctx.sim;
    let v = s.store.vehicle.get(hull.id).expect("a hull the store holds");
    let (config, gear) = if f2::active_in(&world_ctx.fleet) { f2::flight_keys_for_in(&world_ctx.fleet, hull.id) } else {
        Fleet::flight_keys(&mut world_ctx.fleet.flight_keys, &mut world_ctx.fleet.defs, &mut world_ctx.fleet.templates, &String::from_utf16_lossy(s.strings.units(v.name)))
    };
    hull.flight = Flight::from_store(v, &s.strings, config, gear).expect("registered hull flight");
}

/// A hull back into the store, a craft's flight with it.
pub(crate) fn store_hull_in(world_ctx: &mut crate::world::owner::World, hull: &Hull) {
    store_in(world_ctx, hull);
    if hull.flying {
        if f2::active_in(&world_ctx.fleet) { f2::store_flight_in(world_ctx, hull); } else {
            let s = &mut world_ctx.sim;
            let v = s.store.vehicle.get_mut(hull.id).expect("a hull the store holds");
            hull.flight.to_store(v, &mut s.strings);
        }
    }
}

/// An idle stick: { forward: 0, right: 0, turn: 0, brake: false }.
fn idle_input() -> FlightInput {
    FlightInput { forward: 0.0, right: 0.0, turn: 0.0, ..FlightInput::default() }
}

/// Hull `index` (its store handle `r`) as the phase works on it.
fn load_in(world_ctx: &mut crate::world::owner::World, r: Ref<Vehicle>, index: usize) -> Hull {
    if f2::active_in(&world_ctx.fleet) { return f2::take_hull_in(world_ctx, r, index); }
    let s = &mut world_ctx.sim;
    let v = s.store.vehicle.get(r).expect("a hull the store holds");
    let name = String::from_utf16_lossy(s.strings.units(v.name));
    let class = world_ctx.fleet.classes.get_or_insert_with(HashMap::new).get(&name).unwrap_or_else(|| panic!("hull class {name}: not given to the phase"));
    Hull::from_store(r, v, class, index)
}

/// A hull back into the store.
fn store_in(world_ctx: &mut crate::world::owner::World, hull: &Hull) {
    let s = &mut world_ctx.sim;
    let v = s.store.vehicle.get_mut(hull.id).expect("a hull the store holds");
    if f2::active_in(&world_ctx.fleet) { f2::prepare_leg_store_in(&mut world_ctx.fleet, hull,v); }
    hull.to_store(v, &mut s.strings);
    if f2::active_in(&world_ctx.fleet) { f2::stored_in(&mut world_ctx.fleet, hull); }
}

/// What seat 0's bot (a lease that is not a person's) holds as the tick
/// reads it, else an idle stick (a world drives no hull of its own player).
fn input_of(hull: &Hull) -> Input {
    match hull.seats.first().and_then(|s| s.lease) {
        Some(lease) if !lease.human => Input { forward: lease.forward, right: lease.right, turn: lease.turn, brake: lease.brake },
        _ => Input::default(),
    }
}

/// Vehicles.update's ground part: each live ground hull's tick start
/// (captureTick), then the 120 Hz substeps the time left over `acc` and
/// this update's `dt` make, every live ground hull ticking in each, its
/// seat 0's frozen input or none; then each destroyed hull's dying, and,
/// with `poses`, each hull the module poses posed for the tick
/// (aimTick, then render where `cosmetic`, as Vehicles.update poses every
/// hull), at the tick's alpha. Returns what is left over. `level`: the
/// water's level this phase (`water.level`), for a map with water;
fn recycle_hull_in(world_ctx: &mut crate::world::owner::World, hull: Hull) { if f2::active_in(&world_ctx.fleet) { f2::recycle_in(world_ctx, hull); } }

/// `clock`: the clock barrels kick back on.

pub(crate) fn veh_ground_in(world_ctx: &mut crate::world::owner::World, dt: f64, acc: f64, flight_acc: f64, level: f64, clock: f64, flags: u32) -> f64 {
    ground_in(world_ctx, dt, acc, flight_acc, level, clock, flags, None)
}

// Vehicles.update reuses its caller's authority through substeps and wreck
// explosions. A coordinated world span must not reacquire that authority.
pub(crate) fn ground_in(world_ctx: &mut crate::world::owner::World, dt: f64, acc: f64, flight_acc: f64, level: f64, clock: f64, flags: u32,
    services: Option<crate::units::f3::WorldServices<'_>>) -> f64 {
    let (poses_on, cosmetic, flight_on, assists) = (flags & 1, flags & 2, flags & 4, flags & 8);
    if let Some(m) = world_ctx.fleet.map.as_mut() {
        m.level = level;
    }
    world_ctx.fleet.clock = Some(clock);
    let mut refs = std::mem::take(&mut world_ctx.fleet.phase_refs);
    refs.clear(); refs.extend_from_slice(&world_ctx.sim.store.vehicles.vehicles);
    let mut world = JsWorld::with_services_in(world_ctx, u32::MAX, services);
    world.world_ctx.fleet.locks.clear();
    world.world_ctx.fleet.locks.resize(F2LockHeader::WORDS,0.0);
    if f2::active_in(&world.world_ctx.fleet) {
        for (i, &r) in refs.iter().enumerate() {
            let v = world.world_ctx.sim.store.vehicle.get(r).expect("F2 hull");
            if v.alive && !v.flying {
                let mut hull = load_in(world.world_ctx, r, i);
                hull.capture_tick(); store_in(world.world_ctx, &hull); recycle_hull_in(world.world_ctx, hull);
            }
        }
    }
    if flight_on != 0 {
        if let Some(env) = world.world_ctx.fleet.flight_env.as_mut() {
            env.assists = assists != 0;
        }
        // The crafts first (Vehicles.update): each live craft's tick start
        // and step, at 1/60 s, as many as the time left over makes.
        let mut flight_acc = flight_acc + dt;
        let mut last = std::mem::take(&mut world.world_ctx.fleet.phase_locks);
        last.resize(refs.len(), None); last.fill(None);
        while flight_acc + 1e-9 >= 1.0 / 60.0 {
            for (i, &r) in refs.iter().enumerate() {
                let v = world.world_ctx.sim.store.vehicle.get(r).expect("a hull the store holds");
                if !(v.alive && v.flying) {
                    continue;
                }
                let mut hull = load_flyer_in(world.world_ctx, r, i);
                hull.capture_tick();
                hull.capture_flight_tick();
                let given = world.world_ctx.fleet.flight_inputs.get(r.slot as usize).copied().flatten();
                // What Vehicles.update hands the tick: seat 0's frozen
                // input where a bot holds it, else an idle stick; a network
                // pilot's keys where his lease holds seat 0.
                let input = match given {
                    Some((seat, _)) if hull.crewed(0) => seat,
                    _ => idle_input(),
                };
                let network = match given {
                    Some((_, keys)) if hull.seats.first().and_then(|s| s.lease).is_some_and(|l| l.network_human) => keys,
                    _ => None,
                };
                world.craft = r.slot;
                hull.flight.lock = None;
                hull.flyer_tick(&mut world, 1.0 / 60.0, &input, network.as_ref());
                world.craft = u32::MAX;
                if let Some(lock) = hull.flight.lock {
                    last[i] = Some(lock);
                }
                store_hull_in(world.world_ctx, &hull); recycle_hull_in(world.world_ctx, hull);
            }
            flight_acc -= 1.0 / 60.0;
        }
        world.world_ctx.fleet.flight_acc = flight_acc;
        for (i, lock) in last.iter().enumerate() {
            if let Some(l) = lock {
                let locks = &mut world.world_ctx.fleet.locks;
                let at = locks.len(); locks.resize(at + F2Lock::WORDS, 0.0);
                F2Lock { hull: refs[i].slot, id: l.target_id, generation: l.generation, elapsed: l.elapsed, complete: l.complete }.put(&mut locks[at..], &mut Vec::new());
                locks[0] += 1.0;
            }
        }
        { world.world_ctx.fleet.phase_locks = last; }
    }
    for (i, &r) in refs.iter().enumerate() {
        let v = world.world_ctx.sim.store.vehicle.get(r).expect("a hull the store holds");
        if !f2::active_in(&world.world_ctx.fleet) && v.alive && !v.flying {
            let mut hull = load_in(world.world_ctx, r, i);
            hull.capture_tick();
            store_in(world.world_ctx, &hull); recycle_hull_in(world.world_ctx, hull);
        }
    }
    let mut acc = acc + dt;
    while acc + 1e-9 >= FIXED_STEP {
        for (i, &r) in refs.iter().enumerate() {
            let v = world.world_ctx.sim.store.vehicle.get(r).expect("a hull the store holds");
            if v.alive && !v.flying {
                let mut hull = load_in(world.world_ctx, r, i);
                let input = input_of(&hull);
                hull.tick(&mut world, FIXED_STEP, &input);
                store_in(world.world_ctx, &hull); recycle_hull_in(world.world_ctx, hull);
            }
        }
        acc -= FIXED_STEP;
    }
    if poses_on == 0 {
        { world.world_ctx.fleet.phase_refs = refs; }
        return acc;
    }
    let cosmetic = cosmetic != 0;
    for (i, &r) in refs.iter().enumerate() {
        let v = world.world_ctx.sim.store.vehicle.get(r).expect("a hull the store holds");
        if !v.alive && v.dying > 0.0 {
            let mut hull = load_in(world.world_ctx, r, i);
            hull.dying_tick(&mut world, dt);
            store_in(world.world_ctx, &hull); recycle_hull_in(world.world_ctx, hull);
        }
    }
    let alpha = v8math::max(0.0, acc / FIXED_STEP);
    // The hulls whose pose is the module's (this life's), in order.
    // A craft needs its flight state and gear clip as well as its rig.
    let mut mine = std::mem::take(&mut world.world_ctx.fleet.phase_mine);
    mine.clear();
    for (i, &r) in refs.iter().enumerate() {
        let v = world.world_ctx.sim.store.vehicle.get(r).expect("a hull the store holds");
        if (!v.flying || flight_on != 0) && matches!(world.world_ctx.fleet.poses.get(r.slot as usize), Some(Some(p)) if p.generation == v.target_generation) { mine.push((i, r)); }
    }
    let mut posed = std::mem::take(&mut world.world_ctx.fleet.phase_posed);
    posed.resize(mine.len(), false); posed.fill(false);
    for pass in 0..2 {
        if pass == 1 && !cosmetic {
            break;
        }
        for (k, &(i, r)) in mine.iter().enumerate() {
            let mut hull = load_in(world.world_ctx, r, i);
            if hull.flying {
                read_flight_in(world.world_ctx, &mut hull);
            }
            // The hull holds its pose while it poses (nothing it calls
            // reaches the poses).
            let Posed { generation, pose, turn_rates } = world.world_ctx.fleet.poses[r.slot as usize].take().expect("the hull's pose");
            hull.pose = Some(pose);
            if hull.alive && !hull.rests(dt) {
                posed[k] = true;
                let alpha = if hull.flying { 1.0 } else { alpha };
                if pass == 0 {
                    hull.aim_tick(&mut world, alpha, dt);
                } else {
                    hull.render(&mut world, alpha, dt, true);
                }
            }
            world.world_ctx.fleet.poses[r.slot as usize] = Some(Posed { generation, pose: hull.pose.take().expect("the hull's pose"), turn_rates });
            store_hull_in(world.world_ctx, &hull); recycle_hull_in(world.world_ctx, hull);
        }
    }
    // Render leaves native poses in place. Mark only hulls that actually
    // posed; the generated delta publication omits unchanged mirror words.
    for (k, &(_, r)) in mine.iter().enumerate() { if posed[k] { f2::mark_in(&mut world.world_ctx.fleet, r); } }
    { world.world_ctx.fleet.phase_refs = refs; world.world_ctx.fleet.phase_mine = mine; world.world_ctx.fleet.phase_posed = posed; }
    acc
}

fn v3(a: &Vector3) -> Vec3 {
    [a.x, a.y, a.z]
}

fn vec(a: &Vec3) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}

/// The battle: the module's own collision world, water and death regions
/// where it holds them (native.rs), the imports for the rest. `craft`: the
/// craft whose flight steps now (its store slot), for its registry entry.

pub struct JsWorld<'w, 'a> {
    world_ctx: &'w mut crate::world::owner::World,
    craft: u32,
    hull: Option<Ref<Vehicle>>,
    services: Option<crate::units::f3::WorldServices<'a>>,
}
impl<'w, 'a> JsWorld<'w, 'a> {
    // Native spatial readers need no mutable world authority. Damage-capable
    // callers lend the existing borrow explicitly instead of acquiring one here.
    pub fn new_in(world_ctx: &'w mut crate::world::owner::World, craft: u32) -> Self { Self::with_services_in(world_ctx, craft, None) }
    pub(crate) fn with_services_in(world_ctx: &'w mut crate::world::owner::World, craft: u32, services: Option<crate::units::f3::WorldServices<'a>>) -> Self {
            Self { world_ctx, craft, hull: None, services }
    }
}

impl VehicleWorld for JsWorld<'_, '_> {
    // VehiclesWorld's spatial queries share the battle's native kernel. No
    // query round-trip can interleave a stale JavaScript collider mirror.
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> { let world_ctx = &mut *self.world_ctx;
            world_ctx.fleet.collision.as_mut().expect("the vehicle collision world")
                .cast_in(&mut world_ctx.collision.state, world_ctx.movement.flags_mut(), origin, direction, far, ignore)?
    }

    fn push_walls(&mut self, hull: usize, probes: &[Vector3], radius: f64, q: &Quaternion, own: Option<&Quaternion>, previous: &Vector3, position: &mut Vector3, velocity: &mut Vector3, terrain: bool) -> bool { let world_ctx = &mut *self.world_ctx;
        // VehiclesWorld.pushWalls can outlive a retired hull's registration.
        let Some(&reference) = world_ctx.sim.store.vehicles.vehicles.get(hull) else { return false };
        let Some(vehicle) = world_ctx.sim.store.vehicle.get(reference) else { return false };
        let Some(collision) = world_ctx.fleet.collision.as_mut() else { return false };
        let Some(undo) = collision.push_walls_in(&mut world_ctx.collision.state, world_ctx.movement.flags_mut(), vehicle.body.slot,
            probes, radius, q, own, previous, position, velocity, terrain) else { return false };
        undo
    }

    fn ground(&mut self, x: f64, z: f64, top: f64) -> Option<Ground> { let world_ctx = &mut *self.world_ctx;
            world_ctx.fleet.collision.as_mut().expect("the vehicle collision world").ground(&mut world_ctx.collision.state, x, z, top)
    }

    fn spring_polygons<'a>(&'a mut self, hull: usize, index: usize, center: &Vector3, radius: f64, _out: &'a mut Vec<Polygon>) -> super::SpringPolygons<'a> { let world_ctx = &mut *self.world_ctx;
        let _allocation = f2::allocation::nested(f2::allocation::Stage::Polygons);
        let Some(&reference) = world_ctx.sim.store.vehicles.vehicles.get(hull) else { return super::SpringPolygons::Native(&[]) };
        let Some(vehicle) = world_ctx.sim.store.vehicle.get(reference) else { return super::SpringPolygons::Native(&[]) };
        let Some(collision) = world_ctx.fleet.collision.as_mut() else { return super::SpringPolygons::Native(&[]) };
        let Some(polygons) = collision.spring_polygons_in(&mut world_ctx.collision.state, world_ctx.movement.flags_mut(), vehicle.body.slot,
            index, center, radius) else { return super::SpringPolygons::Native(&[]) };
        super::SpringPolygons::Native(polygons)
    }

    fn move_body(&mut self, body: BodyId, matrix: &Matrix4) { let world_ctx = &mut *self.world_ctx;
        let Some(collision) = world_ctx.fleet.collision.as_mut() else { return };
        let Some(()) = collision.move_body(&mut world_ctx.collision.state, body, matrix) else { return };
    }

    fn enable_body(&mut self, body: BodyId, enabled: bool) { let world_ctx = &mut *self.world_ctx;
        let Some(collision) = world_ctx.fleet.collision.as_mut() else { return };
        let Some(()) = collision.enable_body_in(&mut world_ctx.collision.state, world_ctx.movement.flags_mut(), body, enabled) else { return };
    }

    fn body_enabled(&mut self, body: BodyId) -> bool { let world_ctx = &mut *self.world_ctx;
        let Some(collision) = world_ctx.fleet.collision.as_mut() else { return false };
        let Some(enabled) = collision.body_enabled_in(&mut world_ctx.collision.state, world_ctx.movement.flags_mut(), body) else { return false };
        enabled
    }

    fn water_over(&mut self, x: f64, z: f64) -> bool { let world_ctx = &mut *self.world_ctx;
            world_ctx.fleet.map.as_mut().expect("the vehicle map rules").water.as_ref().is_some_and(|w| w.over(x, z))
    }

    fn water_level(&mut self) -> f64 { let world_ctx = &mut *self.world_ctx; world_ctx.fleet.map.as_mut().expect("the vehicle map rules").level }

    fn has_water(&mut self) -> bool { let world_ctx = &mut *self.world_ctx; world_ctx.fleet.map.as_mut().expect("the vehicle map rules").water.is_some() }

    fn in_death_region(&mut self, position: &Vector3) -> bool { let world_ctx = &mut *self.world_ctx;
            world_ctx.fleet.map.as_mut().expect("the vehicle map rules").in_death_region(position)
    }

    fn damaged(&mut self, hull: usize, amount: f64, attacker: &OrdnanceOwner) { let world_ctx = &mut *self.world_ctx;
            if let Some(services) = &mut self.services {
                use crate::vehicles::damage::HullDamageWorld;
                let reference=f2::hull_reference_in(world_ctx,hull);
                crate::units::f3_hulls::HullDamage::new(world_ctx, reference, services.reborrow()).damaged_hull(hull, amount, attacker);
                return;
            }
            if f2::active_in(&world_ctx.fleet) { f2::publish_in(world_ctx); }
            import_veh_damaged(&VehDamaged { hull: hull as f64, amount, attacker: *attacker });
    }

    fn destroyed_crew(&mut self, hull: usize) { let world_ctx = &mut *self.world_ctx;
            if let Some(services) = &mut self.services {
                use crate::vehicles::damage::HullDamageWorld;
                let reference=f2::hull_reference_in(world_ctx,hull);
                crate::units::f3_hulls::HullDamage::new(world_ctx, reference, services.reborrow()).destroy_hull_crew(hull);
                return;
            }
            f2::live_seats::destroyed_in(world_ctx, hull);
    }

    fn destroyed(&mut self, hull: usize) { let world_ctx = &mut *self.world_ctx;
            if let Some(services) = &mut self.services {
                use crate::vehicles::damage::HullDamageWorld;
                let reference=f2::hull_reference_in(world_ctx,hull);
                crate::units::f3_hulls::HullDamage::new(world_ctx, reference, services.reborrow()).destroyed_hull(hull);
                return;
            }
            if f2::active_in(&world_ctx.fleet) { f2::publish_in(world_ctx); }
            import_veh_destroyed(&VehHull { hull: hull as f64 });
    }

    fn respawn_clock(&mut self, hull: usize) { let world_ctx = &mut *self.world_ctx;
            f2::respawn_clock_in(world_ctx, hull);
    }

    fn release_crew(&mut self, hull: usize) { let world_ctx = &mut *self.world_ctx;
            if let Some(services) = &mut self.services {
                use crate::vehicles::damage::HullDamageWorld;
                let reference=f2::hull_reference_in(world_ctx,hull);
                crate::units::f3_hulls::HullDamage::new(world_ctx, reference, services.reborrow()).release_hull_crew(hull);
                return;
            }
            f2::live_seats::release_in(world_ctx, hull);
    }

    fn explosion(&mut self, hull: usize, final_explosion: bool, position: &Vector3) { let world_ctx = &mut *self.world_ctx;
            if let Some(services) = &mut self.services {
                use crate::vehicles::damage::HullDamageWorld;
                let reference=f2::hull_reference_in(world_ctx,hull);
                crate::units::f3_hulls::HullDamage::new(world_ctx, reference, services.reborrow()).explode_hull(hull, final_explosion, position);
                return;
            }
            if f2::active_in(&world_ctx.fleet) { f2::publish_in(world_ctx); }
            import_veh_explosion(&VehBlast { hull: hull as f64, r#final: final_explosion, position: v3(position) });
    }

    fn effect(&mut self, name: &str, position: &Vector3) { let world_ctx = &mut *self.world_ctx;
            if let Some(services) = &mut self.services {
                use crate::vehicles::damage::HullDamageWorld;
                crate::units::f3_hulls::HullDamage::new(world_ctx,self.hull.expect("committed wreck"),services.reborrow()).hull_effect(name, position);
                return;
            }
            let name = world_ctx.sim.strings.intern_str(name);
            import_veh_effect(&VehEffect { name, position: v3(position) });
    }

    fn raycast_units(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<f64> {
            if crate::units::f3::active_in(&self.world_ctx.units) {
                return crate::units::f3_fleet::raycast_in(self.world_ctx, origin, direction, far, None);
            }
            let h = import_veh_raycast_units(&VehRay { origin: v3(origin), direction: v3(direction), far, ignore: -1.0 });
            h.found.then_some(h.distance)
    }

    fn clock(&mut self) -> f64 { let world_ctx = &mut *self.world_ctx;
            world_ctx.fleet.clock.expect("the vehicle phase clock")
    }

    fn tow_released(&mut self, _hull: usize, _reason: &str) {
            // Hull cleanup never releases a cable; the native tow phase does.
            unreachable!("tow release belongs to the mounted weapon owner")
    }

    fn current(&mut self, hull: &Hull) { let world_ctx = &mut *self.world_ctx;
            self.hull = Some(hull.id);
            // Vehicle.setHealth may enter native crew work before its next hook.
            // Publish at the actual foreign call so no unread block is replaced.
            store_hull_in(world_ctx, hull);
    }

    fn reload(&mut self, hull: &mut Hull) { let world_ctx = &mut *self.world_ctx;
            let s = &mut world_ctx.sim;
            let v = s.store.vehicle.get(hull.id).expect("a hull the store holds");
            hull.load(v);
            if hull.flying {
                read_flight_in(world_ctx, hull);
            }
    }
}

/// A hull's aim point (VehicleTarget.aimPoint: Vehicle.sphere, the middle
/// of its box as it stands now).
fn hull_point_in(world_ctx: &mut crate::world::owner::World, slot: u32) -> Option<(bool, BodyId, Vector3)> {
    let s = &mut world_ctx.sim;
    let r = s.store.vehicles.vehicles.iter().copied().find(|r| r.slot == slot)?;
    let v = s.store.vehicle.get(r)?;
    let mut m = Matrix4::default();
    m.from_array(&v.matrix, 0);
    let mut point = Vector3::new(v.center[0], v.center[1], v.center[2]);
    point.apply_matrix4(&m);
    Some((v.alive, v.body.slot, point))
}

impl FlightWorld for JsWorld<'_, '_> {
    fn environment(&mut self) -> &Environment { let world_ctx = &mut *self.world_ctx;
            world_ctx.fleet.flight_env.as_ref().expect("the flight environment (veh_flight_env)")
    }

    fn tuning(&mut self, class: FlightClass) -> Tuning { let world_ctx = &mut *self.world_ctx;
            flight_tuning_in(world_ctx, class)
    }

    fn member_count(&mut self) -> usize { let world_ctx = &mut *self.world_ctx;
            if crate::units::f3::active_in(&world_ctx.units) { return crate::units::f3_fleet::count_in(&world_ctx.sim); }
            world_ctx.fleet.members.len()
    }

    fn member(&mut self, i: usize) -> Member { let world_ctx = &mut *self.world_ctx;
            if crate::units::f3::active_in(&world_ctx.units) { return crate::units::f3_fleet::flight_in(world_ctx, i, self.craft); }
            let m = world_ctx.fleet.members[i];
            match m.kind {
                MEMBER_UNIT => Member { id: m.id, generation: m.generation, alive: m.alive, team: m.team, own: false, unit: true, body: None, point: Vector3::default() },
                MEMBER_HULL => {
                    let (alive, body, point) = hull_point_in(world_ctx, m.hull).expect("a hull entry's hull");
                    Member { id: m.id, generation: m.generation, alive, team: m.team, own: m.hull == self.craft, unit: false, body: Some(body), point }
                }
                MEMBER_ON_HULL => {
                    let (_, body, point) = hull_point_in(world_ctx, m.hull).expect("an entry's hull");
                    Member { id: m.id, generation: m.generation, alive: m.alive, team: m.team, own: false, unit: false, body: Some(body), point }
                }
                _ => Member { id: m.id, generation: m.generation, alive: m.alive, team: m.team, own: false, unit: false, body: None, point: m.point },
            }
    }

    fn relation(&mut self, a: Team, b: Team) -> f64 { let world_ctx = &mut *self.world_ctx;
            if crate::units::f3::active_in(&world_ctx.units) { return crate::units::f3::relation_in(&world_ctx.units, a.unwrap_or(0.0),b.unwrap_or(0.0)); }
            let teams = &world_ctx.fleet.teams;
            let same = |x: &Team, y: &Team| match (x, y) {
                (None, None) => true,
                (Some(x), Some(y)) => x == y || x.is_nan() && y.is_nan(),
                _ => false,
            };
            let (Some(i), Some(j)) = (teams.iter().position(|t| same(t, &a)), teams.iter().position(|t| same(t, &b))) else {
                panic!("a team the relations do not name: {a:?} {b:?}")
            };
            world_ctx.fleet.relations[i * teams.len() + j]
    }

    fn aim_point(&mut self, i: usize) -> Vector3 { let world_ctx = &mut *self.world_ctx;
            if crate::units::f3::active_in(&world_ctx.units) {
                let member = crate::units::f3_fleet::key_in(&world_ctx.sim, i);
                if matches!(member, crate::gen::Member::Unit(_)) {
                    if let Some(Some(point)) = world_ctx.fleet.aim_cache.get(i) { return *point; }
                    let point = crate::units::f3_fleet::aim_in(world_ctx, member);
                    { world_ctx.fleet.aim_cache[i] = Some(point); }
                    return point;
                }
                return crate::units::f3_fleet::aim_in(world_ctx, member);
            }
            let m = world_ctx.fleet.members[i];
            match m.kind {
                MEMBER_UNIT => {
                    if let Some(Some(p)) = world_ctx.fleet.aim_cache.get(i) {
                        return *p;
                    }
                    let p = vec(&import_veh_aim_point(&VehHull { hull: i as f64 }).position);
                    if let Some(slot) = world_ctx.fleet.aim_cache.get_mut(i) {
                        *slot = Some(p);
                    }
                    p
                }
                MEMBER_HULL | MEMBER_ON_HULL => hull_point_in(world_ctx, m.hull).expect("a hull entry's hull").2,
                _ => m.point,
            }
    }

    fn prefetch_aim_points(&mut self, members: &[usize]) {
            if crate::units::f3::active_in(&self.world_ctx.units) {
                // Flight targeting observes each requested Unit once this phase.
                // Preserve the original bulk request order and retained aim cache.
                for &index in members {
                    if matches!(crate::units::f3_fleet::key_in(&self.world_ctx.sim,index), crate::gen::Member::Unit(_)) { self.aim_point(index); }
                }
                return;
            }
            let world_ctx = &mut *self.world_ctx;
            let rows = &world_ctx.fleet.members;
            let cache = &world_ctx.fleet.aim_cache;
            let mut args = VehMembers { indices: std::mem::take(&mut world_ctx.fleet.aim_wanted) };
            args.indices.clear();
            args.indices.extend(members.iter().copied().filter(|&i| rows[i].kind == MEMBER_UNIT && cache[i].is_none()).map(|i| i as f64));
            if !args.indices.is_empty() {
                import_veh_aim_points_read(&args, |row, heap| {
                    let cache = &mut world_ctx.fleet.aim_cache;
                    for (&i, point) in args.indices.iter().zip(VehPoints::points_rows(row, heap)) { cache[i as usize] = Some(Vector3::new(point[0], point[1], point[2])); }
                });
            }
            { world_ctx.fleet.aim_wanted = args.indices; }
    }

    fn body_is_hull(&mut self, body: BodyId) -> bool { let world_ctx = &mut *self.world_ctx;
            let s = &mut world_ctx.sim;
            s.store.vehicles.vehicles.iter().any(|&r| s.store.vehicle.get(r).is_some_and(|v| v.body.slot == body))
    }

    fn system_death_region(&mut self, position: &Vector3) -> bool {
            self.in_death_region(position)
    }
}

impl Fleet {
    fn veh_class(strings: &crate::store::Strings, classes: &mut Option<HashMap<String, HullClass>>, name: u32, json: u32, turn_rate: f64) {

        let name = String::from_utf16_lossy(strings.units(StrId(name)));
        let text = String::from_utf16_lossy(strings.units(StrId(json)));
        let def = bfe_content::json::parse(&text).unwrap_or_else(|e| panic!("hull class {name}: {e:?}"));
        classes.get_or_insert_with(HashMap::new).insert(name, HullClass::new(&def, turn_rate));
    }
    fn veh_map(strings: &crate::store::Strings, map: &mut Option<MapRules>, json: u32, kill_y: f64) {

        let text = String::from_utf16_lossy(strings.units(StrId(json)));
        let value = bfe_content::json::parse(&text).unwrap_or_else(|e| panic!("hull phase map: {e:?}"));
        let rules = MapRules::from_json(&value, kill_y);
        *map = Some(rules);
    }
    fn veh_collision(collision: &mut Option<Collision>, world: i32, blockers: i32) {
        let slot = collision;
        assert!(world >= 0, "vehicles require native collision");
        let blockers = (blockers >= 0).then_some(blockers as usize);
        match slot {
            Some(c) if c.world == world as usize => c.blockers = blockers,
            _ => *slot = Some(Collision::new(world as usize, blockers)),
        }
    }
    fn veh_input(input: &mut Vec<f64>, count: u32) -> *mut f64 {
        let v = input;
        if v.len() < count as usize {
            v.resize(count as usize, 0.0);
        }
        v.as_mut_ptr()
    }
    fn veh_bytes(bytes: &mut Vec<u8>, count: u32) -> *mut u8 {
        let v = bytes;
        v.clear();
        v.resize(count as usize, 0);
        v.as_mut_ptr()
    }
    fn veh_flight_env(strings: &crate::store::Strings, flight_env: &mut Option<Environment>, tunings: &mut Option<Tunings>, json: u32, max_fly_height: f64) {

        let text = String::from_utf16_lossy(strings.units(StrId(json)));
        let env = bfe_content::json::parse(&text).unwrap_or_else(|e| panic!("flight environment: {e:?}"));
        let nums3 = |v: Option<&Value>| -> [f64; 3] { [0, 1, 2].map(|k| v.and_then(|a| a.at(k)).and_then(Value::as_f64).unwrap_or(f64::NAN)) };
        let bounds = env.get("bounds").filter(|b| !matches!(b, Value::Null)).map(|b| (nums3(b.get("min")), nums3(b.get("max"))));
        let landing_zones = env.get("landingZones").filter(|z| !matches!(z, Value::Null)).map(|zones| {
            zones
                .as_array()
                .unwrap_or(&[])
                .iter()
                .map(|z| {
                    let (min, max) = (nums3(z.get("min")), nums3(z.get("max")));
                    let floor_y = match z.get("floorY") {
                        None | Some(Value::Null) => min[1],
                        Some(v) => v.as_f64().unwrap_or(f64::NAN),
                    };
                    Zone { min, max, floor_y }
                })
                .collect()
        });
        let environment = Environment {
            ceiling: env.get("ceiling").and_then(Value::as_f64),
            max_fly_height,
            bounds,
            space: env.get("space").is_some_and(Value::truthy),
            landing_zones,
            assists: false,
        };
        {
            *flight_env = Some(environment);
            tunings.get_or_insert_with(Tunings::default);
        }
    }
    fn veh_flight_tune(strings: &crate::store::Strings, tunings: &mut Option<Tunings>, class: u32, key: u32, value: f64) -> u32 {

        let class = String::from_utf16_lossy(strings.units(StrId(class)));
        let key = String::from_utf16_lossy(strings.units(StrId(key)));
        let t = tunings.get_or_insert_with(Tunings::default);
        t.retune(&class, &key, value) as u32
    }
    fn veh_members(input: &mut Vec<f64>, members: &mut Vec<MemberRow>, aim_cache: &mut Vec<Option<Vector3>>, count: u32) {
        let v = &input[..count as usize * 10];
        let rows = members;
        rows.clear();
        let cache = aim_cache;
        cache.clear();
        cache.resize(count as usize, None);
        for m in v.chunks_exact(10) {
            rows.push(MemberRow {
                kind: m[0] as u8,
                id: m[1],
                generation: (!m[2].is_nan()).then_some(m[2]),
                alive: m[3] != 0.0,
                team: team(m[4], m[5]),
                hull: m[6] as u32,
                point: Vector3::new(m[7], m[8], m[9]),
            });
        }
    }
    fn veh_relations(input: &mut Vec<f64>, teams: &mut Vec<Team>, relations: &mut Vec<f64>, count: u32) {
        let n = count as usize;
        let v = &input[..n * 2 + n * n];
        {
            let teams = teams; teams.clear(); teams.extend((0..n).map(|i| team(v[2 * i], v[2 * i + 1])));
            let relations = relations; relations.clear(); relations.extend_from_slice(&v[n * 2..]);
        }
    }
    fn veh_flight_inputs(input: &mut Vec<f64>, flight_inputs: &mut Vec<Option<(FlightInput, Option<FlightInput>)>>, count: u32) {
        let v = &input[..count as usize * 40];
        let all = flight_inputs;
        for x in all.iter_mut() {
            *x = None;
        }
        for f in v.chunks_exact(40) {
            let slot = f[0] as usize;
            if all.len() <= slot {
                all.resize(slot + 1, None);
            }
            all[slot] = Some((flight_input(&f[2..21]), (f[1] != 0.0).then(|| flight_input(&f[21..40]))));
        }
    }
    fn new_pose(defs: &mut Option<HashMap<String, Value>>, templates: &mut Option<HashMap<String, VehicleTemplate>>, name: &str) -> Option<(HullPose, Vec<f64>)> {
        let defs = defs.as_ref()?;
        let templates = templates.as_ref()?;
        let def = defs.get(name)?;
        let model = def.get("model").and_then(Value::as_str)?;
        let template = templates.get(model)?;
        let source = super::build::HullSource { name, def, template, slot: 0, slot_matrix: &[0.0; 16] };
        let (hull, extras) = super::build::build(&source, 0, Ref::default(), 0);
        Some((hull.pose?, extras.seat_turn_rates))
    }
    fn flight_keys(flight_keys: &mut Option<HashMap<String, (Config, Option<GearClip>)>>, defs: &mut Option<HashMap<String, Value>>, templates: &mut Option<HashMap<String, VehicleTemplate>>, name: &str) -> (Config, Option<GearClip>) {
        let keys = flight_keys.get_or_insert_with(HashMap::new);
        if let Some(k) = keys.get(name) {
            return k.clone();
        }
        let defs = defs.as_ref().expect("the vehicle pack (flight needs its classes)");
        let def = defs.get(name).unwrap_or_else(|| panic!("hull class {name}: not in the pack"));
        let templates = templates.as_ref().expect("the vehicle pack");
        let template = def.get("model").and_then(Value::as_str).and_then(|m| templates.get(m));
        let gear = template.and_then(|t| {
            let c = t.clips.as_ref()?.get("takeoff")?;
            let desc = crate::rig::content::hull_desc(def, t);
            let root = desc.bones.iter().position(|b| b.name.eq_ignore_ascii_case("dummyroot")).map_or(-1, |i| i as i32);
            Some(GearClip { clip: std::rc::Rc::new(crate::rig::content::clip(c)), duration: c.duration, root })
        });
        let k = (Config::of(def), gear);
        keys.insert(name.to_string(), k.clone());
        k
    }
    fn flight_tuning(tunings: &mut Option<Tunings>, class: FlightClass) -> Tuning {
        tunings.get_or_insert_with(Tunings::default).of(class)
    }
    fn wire_walk(poses: &[Option<Posed>], reference: Ref<Vehicle>) -> (f64, f64) {
        poses.get(reference.slot as usize).and_then(Option::as_ref)
            .map_or((0., 0.), |row| (row.pose.walk_phase, row.pose.walk_blend))
    }
    fn wire_seat_history(poses: &[Option<Posed>], reference: Ref<Vehicle>, index: usize) -> Option<(f64, Option<f64>)> {
        let row = poses.get(reference.slot as usize)?.as_ref()?;
        let seat = row.pose.seats.get(index)?; Some((seat.yaw_rate, seat.last_yaw))
    }
}

// facade shims: callers outside the fleet bindings still enter through WASM.
#[cfg(target_arch = "wasm32")]
pub(crate) fn collision_world() -> Option<&'static mut Collision> { collision_world_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn map() -> Option<&'static mut MapRules> { map_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn flight_tuning(class: FlightClass) -> Tuning { flight_tuning_in(crate::abi::world(), class) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn wire_walk(reference: Ref<Vehicle>) -> (f64, f64) { wire_walk_in(&crate::abi::world().fleet, reference) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn wire_seat_history(reference: Ref<Vehicle>, index: usize) -> Option<(f64, Option<f64>)> { wire_seat_history_in(&crate::abi::world().fleet, reference, index) }

#[cfg(target_arch = "wasm32")]
pub(crate) fn prediction_hull(r: Ref<Vehicle>) -> Option<crate::client::hulls::Session> { prediction_hull_in(crate::abi::world(), r) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn read_flight(hull: &mut Hull) { read_flight_in(crate::abi::world(), hull) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn store_hull(hull: &Hull) { store_hull_in(crate::abi::world(), hull) }

impl<'a> JsWorld<'static, 'a> {
    // facade shims: external physical callers still enter through WASM.
    #[cfg(target_arch = "wasm32")]
    pub fn new(craft: u32) -> Self { Self::new_in(crate::abi::world(), craft) }
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn veh_class(name: u32, json: u32, turn_rate: f64) { veh_class_in(crate::abi::facade::world(), name, json, turn_rate) }
    #[no_mangle]
    pub extern "C" fn veh_map(json: u32, kill_y: f64) { veh_map_in(crate::abi::facade::world(), json, kill_y) }
    #[no_mangle]
    pub extern "C" fn veh_collision(world: i32, blockers: i32) { veh_collision_in(crate::abi::facade::world(), world, blockers) }
    #[no_mangle]
    pub extern "C" fn veh_flags() -> *mut u8 { veh_flags_in(crate::abi::facade::cold()).map_or(std::ptr::null_mut(), |b| b.as_mut_ptr()) }
    #[no_mangle]
    pub extern "C" fn veh_input(count: u32) -> *mut f64 { veh_input_in(crate::abi::facade::world(), count).map_or(std::ptr::null_mut(), |b| b.as_mut_ptr()) }
    #[no_mangle]
    pub extern "C" fn veh_bodies(count: u32) { veh_bodies_in(crate::abi::facade::world(), count) }
    #[no_mangle]
    pub extern "C" fn veh_collider(walls: u32, springs: u32) { veh_collider_in(crate::abi::facade::world(), walls, springs) }
    #[no_mangle]
    pub extern "C" fn veh_changes() -> *const f64 { veh_changes_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn veh_bytes(count: u32) -> *mut u8 { veh_bytes_in(crate::abi::facade::world(), count).map_or(std::ptr::null_mut(), |b| b.as_mut_ptr()) }
    #[no_mangle]
    pub extern "C" fn veh_pack(def_len: u32, floats: u32, indices: u32, animations: u32) -> u32 { veh_pack_in(crate::abi::facade::world(), def_len, floats, indices, animations) }
    #[no_mangle]
    pub extern "C" fn veh_pull(slot: u32, generation: f64, count: u32, seed: u32) -> u32 { veh_pull_in(crate::abi::facade::world(), slot, generation, count, seed) }
    #[no_mangle]
    pub extern "C" fn veh_flight_env(json: u32, max_fly_height: f64) { veh_flight_env_in(crate::abi::facade::world(), json, max_fly_height) }
    #[no_mangle]
    pub extern "C" fn veh_flight_tune(class: u32, key: u32, value: f64) -> u32 { veh_flight_tune_in(crate::abi::facade::world(), class, key, value) }
    #[no_mangle]
    pub extern "C" fn veh_members(count: u32) { veh_members_in(crate::abi::facade::world(), count) }
    #[no_mangle]
    pub extern "C" fn veh_relations(count: u32) { veh_relations_in(crate::abi::facade::world(), count) }
    #[no_mangle]
    pub extern "C" fn veh_flight_inputs(count: u32) { veh_flight_inputs_in(crate::abi::facade::world(), count) }
    #[no_mangle]
    pub extern "C" fn veh_flight_acc() -> f64 { veh_flight_acc_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn veh_locks() -> *const f64 { veh_locks_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn veh_ground(dt: f64, acc: f64, flight_acc: f64, level: f64, clock: f64, flags: u32) -> f64 { veh_ground_in(crate::abi::facade::world(), dt, acc, flight_acc, level, clock, flags) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{veh_class, veh_map, veh_collision, veh_flags, veh_input, veh_bodies, veh_collider, veh_changes, veh_bytes, veh_pack, veh_pull, veh_flight_env, veh_flight_tune, veh_members, veh_relations, veh_flight_inputs, veh_flight_acc, veh_locks, veh_ground};

#[cfg(test)]
mod robustness_tests {
    use super::*;

    #[test]
    fn hull_queries_skip_retired_and_refused_bodies() {
        let mut owner = crate::world::owner::World::new();
        owner.fleet.collision = Some(Collision::new(0, None));
        // The list may still name a retired hull until the enclosing tick ends.
        owner.sim.store.vehicles.vehicles.push(Ref::new(0, 1));
        let mut world = JsWorld::new_in(&mut owner, 0);
        let mut position = Vector3::new(1.0, 2.0, 3.0);
        let mut velocity = Vector3::new(4.0, 5.0, 6.0);
        assert!(!world.push_walls(0, &[], 1.0, &Quaternion::default(), None,
            &Vector3::default(), &mut position, &mut velocity, false));
        assert_eq!(position, Vector3::new(1.0, 2.0, 3.0));
        assert_eq!(velocity, Vector3::new(4.0, 5.0, 6.0));
        assert!(matches!(world.spring_polygons(0, 0, &position, 1.0, &mut Vec::new()), super::super::SpringPolygons::Native(p) if p.is_empty()));
        world.move_body(u32::MAX, &Matrix4::default());
        world.enable_body(u32::MAX, true);
        assert!(!world.body_enabled(u32::MAX));
        assert!(world.cast(&position, &velocity, 1.0, Some(u32::MAX)).is_none());
    }
}
