//! The hull phase on the module, behind its switch (src/vehicles-sim.js;
//! off unless a world asks for it): the ground hulls' tick start and their
//! 120 Hz substeps (src/vehicles.js Vehicles.update), run on the hulls as
//! the store holds them, JavaScript writing them in before the phase and
//! syncing its Vehicle objects from the store after it.
//!
//! What the hulls ask of the battle is answered here where the module holds
//! it (native.rs): the collision world's queries from the module's own copy
//! of it, when the battle's collision runs on it (src/ray-kernel.js), and
//! the map's water and death regions. The rest (hooks, explosions, effects,
//! the units, the clock) comes back as the imports of
//! rust/sim/schema/vehicles.mjs, served by the same VehiclesWorld
//! (src/vehicles-world.js) the JavaScript hulls call; so does the
//! collision in a battle whose collision is JavaScript's.
//!
//! As the rounds' module does (rust/sim/src/ordnance/module.rs), the store
//! is reached through short borrows taken from the instance's one `Sim` and
//! never held across an import: a hook JavaScript serves (a hull destroyed,
//! its blast) can reach every hull. Before such a hook the hull whose method
//! calls it goes back into the store and JavaScript syncs its hulls from it;
//! after it JavaScript writes its hulls back and the hull is read again
//! (`current`, `reload`), so a hook sees and changes the hulls as it does
//! when the hulls are JavaScript's. The bodies' flags and places the hulls
//! changed go to JavaScript before a hook and after the phase
//! (`veh_changes`), and JavaScript gives every body's flag again after a
//! hook (`veh_flags`), for the same reason.

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
use crate::abi::sim;
use crate::collision::spheres::Bounds;
use crate::gen::*;
use crate::store::{Ref, StrId};
use std::collections::HashMap;
use std::ptr::addr_of_mut;
use three::{Matrix4, Quaternion, Vector3};

/// vehicles.js FIXED_STEP: a ground hull's substep.
const FIXED_STEP: f64 = 1.0 / 120.0;

static mut CLASSES: Option<HashMap<String, HullClass>> = None;
/// The collision world the hulls query here (none: the battle's collision
/// is JavaScript's, and the imports answer), the map's water and death
/// regions, what JavaScript hands over in numbers (`veh_input`) and the
/// changes it takes back (`veh_changes`).
static mut COLLISION: Option<Collision> = None;
static mut MAP: Option<MapRules> = None;
static mut INPUT: Vec<f64> = Vec::new();
static mut CHANGES: Vec<f64> = Vec::new();
/// The vehicle pack's templates by model and its classes by name
/// (`veh_pack`), each ground hull's pose by its slot in the store with the
/// life it was taken for (posing.rs), what the last phase's poses left for
/// JavaScript, and this phase's clock (the barrels' kick).
static mut TEMPLATES: Option<HashMap<String, VehicleTemplate>> = None;
static mut DEFS: Option<HashMap<String, Value>> = None;
static mut POSES: Vec<Option<Posed>> = Vec::new();
static mut RIDERS: Vec<f64> = Vec::new();

/// A ground hull's pose kept in the module: the life it was taken for, the
/// pose, and its seats' turn rates (seatView's turnRate, the 9-pose's).
struct Posed {
    generation: f64,
    pose: HullPose,
    turn_rates: Vec<f64>,
}
static mut POSED: Vec<f64> = Vec::new();
static mut CLOCK: Option<f64> = None;
static mut BYTES: Vec<u8> = Vec::new();
/// Flight's view of the battle: its environment and the classes' live
/// tunings (given at attach, retuned by command), the registry's entries
/// and the teams' relations this phase, each craft's pilot input, each
/// class's flight keys, the crafts' accumulated time, and the locks the
/// crafts' targeting left for their weapons (out).
static mut FLIGHT_ENV: Option<Environment> = None;
static mut TUNINGS: Option<Tunings> = None;
static mut MEMBERS: Vec<MemberRow> = Vec::new();
static mut RELATIONS: Vec<f64> = Vec::new();
static mut TEAMS: Vec<Team> = Vec::new();
static mut FLIGHT_INPUTS: Vec<Option<(FlightInput, Option<FlightInput>)>> = Vec::new();
static mut FLIGHT_KEYS: Option<HashMap<String, (Config, Option<GearClip>)>> = None;
static mut FLIGHT_ACC: f64 = 0.0;
static mut LOCKS: Vec<f64> = Vec::new();
/// Units' aim points JavaScript gave this phase (by entry), until the
/// registry's rows are written again.
static mut AIM_CACHE: Vec<Option<Vector3>> = Vec::new();

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

// SAFETY (these and `classes`): one thread; each is borrowed for one call
// of the module's own and never across an import.
fn collision_world() -> Option<&'static mut Collision> {
    unsafe { (*addr_of_mut!(COLLISION)).as_mut() }
}

fn map() -> Option<&'static mut MapRules> {
    unsafe { (*addr_of_mut!(MAP)).as_mut() }
}

fn input() -> &'static mut Vec<f64> {
    unsafe { &mut *addr_of_mut!(INPUT) }
}

fn poses() -> &'static mut Vec<Option<Posed>> {
    unsafe { &mut *addr_of_mut!(POSES) }
}

/// The hull classes' keys the phase has been given (`veh_class`), by name.
fn classes() -> &'static mut HashMap<String, HullClass> {
    // SAFETY: one thread; nothing holds a reference across a call out.
    unsafe { (*addr_of_mut!(CLASSES)).get_or_insert_with(HashMap::new) }
}

/// A hull class: its name and its definition (`data.classes[name]`) as JSON,
/// both interned strings, and the turn rate its hulls have (its seats' aim
/// rates fall back on it).
#[no_mangle]
pub extern "C" fn veh_class(name: u32, json: u32, turn_rate: f64) {
    let s = sim();
    let name = String::from_utf16_lossy(s.strings.units(StrId(name)));
    let text = String::from_utf16_lossy(s.strings.units(StrId(json)));
    let def = bfe_content::json::parse(&text).unwrap_or_else(|e| panic!("hull class {name}: {e:?}"));
    classes().insert(name, HullClass::new(&def, turn_rate));
}

/// The map's data the hulls ask about, as JSON (`water`: its cell, offset,
/// grid and the cell numbers its Set holds, or null; `deathRegions` and
/// `mission.deathRegions` as the map has them), and the kill height under
/// the battlefield (game.js killY): the water and the death regions are
/// answered here from then on.
#[no_mangle]
pub extern "C" fn veh_map(json: u32, kill_y: f64) {
    let s = sim();
    let text = String::from_utf16_lossy(s.strings.units(StrId(json)));
    let value = bfe_content::json::parse(&text).unwrap_or_else(|e| panic!("hull phase map: {e:?}"));
    let rules = MapRules::from_json(&value, kill_y);
    unsafe { *addr_of_mut!(MAP) = Some(rules) };
}

/// The battle's collision as the module holds it: the kernel's world
/// (collision.js rayWorld) and the vehicle-only barriers' fixed set (-1
/// none). A world below 0: the battle's collision is JavaScript's, and the
/// imports answer. What it knows of the bodies stays while the world does.
#[no_mangle]
pub extern "C" fn veh_collision(world: i32, blockers: i32) {
    let slot = unsafe { &mut *addr_of_mut!(COLLISION) };
    if world < 0 {
        *slot = None;
        return;
    }
    let blockers = (blockers >= 0).then_some(blockers as usize);
    match slot {
        Some(c) if c.world == world as usize => c.blockers = blockers,
        _ => *slot = Some(Collision::new(world as usize, blockers)),
    }
}

/// Where JavaScript writes each body's enabled flag, by its place in the
/// collision world's order (MAX_BODIES bytes; 0 without a world here).
#[no_mangle]
pub extern "C" fn veh_flags() -> *mut u8 {
    collision_world().map_or(std::ptr::null_mut(), |c| c.flags_ptr())
}

/// Room for `count` numbers JavaScript hands over (veh_bodies, veh_collider).
#[no_mangle]
pub extern "C" fn veh_input(count: u32) -> *mut f64 {
    let v = input();
    if v.len() < count as usize {
        v.resize(count as usize, 0.0);
    }
    v.as_mut_ptr()
}

/// The bodies' numbers: `count` pairs in the input, each a body's number in
/// the store (the bridge's) and its kernel slot.
#[no_mangle]
pub extern "C" fn veh_bodies(count: u32) {
    let Some(c) = collision_world() else { return };
    let v = input();
    for i in 0..count as usize {
        c.name_body(v[2 * i] as u32, v[2 * i + 1] as u32);
    }
}

/// A hull's body: in the input its number in the store, its kernel slot,
/// its bounds in its own frame (min, max), then the gather caches (kernel
/// ids, -1 none) of its `walls` wall probes and of its `springs` springs.
#[no_mangle]
pub extern "C" fn veh_collider(walls: u32, springs: u32) {
    let Some(c) = collision_world() else { return };
    let v = input();
    let (w, n) = (walls as usize, springs as usize);
    let mut local_bounds: Bounds = [0.0; 6];
    local_bounds.copy_from_slice(&v[2..8]);
    let collider = Collider {
        kernel: v[1] as u32,
        local_bounds,
        walls: v[8..8 + w].iter().map(|&x| x as i64).collect(),
        springs: v[8 + w..8 + w + n].iter().map(|&x| x as i64).collect(),
    };
    c.add_collider(v[0] as BodyId, collider);
}

/// The flags and places the hulls changed since the last call, for
/// JavaScript's bodies (Collision::take_changes); returns their address, or
/// 0 without a collision world here.
#[no_mangle]
pub extern "C" fn veh_changes() -> *const f64 {
    let Some(c) = collision_world() else { return std::ptr::null() };
    let out = unsafe { &mut *addr_of_mut!(CHANGES) };
    c.take_changes(crate::collision::state(), out);
    out.as_ptr()
}

/// Room for `count` bytes JavaScript hands over (the vehicle pack).
#[no_mangle]
pub extern "C" fn veh_bytes(count: u32) -> *mut u8 {
    let v = unsafe { &mut *addr_of_mut!(BYTES) };
    v.clear();
    v.resize(count as usize, 0);
    v.as_mut_ptr()
}

/// The vehicle pack, from `veh_bytes`'s room: its JSON (`def_len` bytes:
/// its models, collision, animations and classes), then (from the next
/// multiple of 4) its models' `floats` floats and `indices` indices and its
/// animations' `animations` floats. The hulls' rigs are built from it as
/// the content crate builds them (posing.rs). Returns how many models.
#[no_mangle]
pub extern "C" fn veh_pack(def_len: u32, floats: u32, indices: u32, animations: u32) -> u32 {
    let bytes = unsafe { &*addr_of_mut!(BYTES) };
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
    unsafe {
        *addr_of_mut!(TEMPLATES) = Some(made.into_iter().map(|t| (t.name.clone(), t)).collect());
        *addr_of_mut!(DEFS) = Some(classes);
        // The pack's bytes are not needed again.
        *addr_of_mut!(BYTES) = Vec::new();
    }
    count
}

/// A ground hull's pose as the constructor makes it from its class and
/// model (build.rs), with its seats' turn rates, or None where the pack
/// lacks either.
fn new_pose(name: &str) -> Option<(HullPose, Vec<f64>)> {
    let defs = unsafe { (*addr_of_mut!(DEFS)).as_ref()? };
    let templates = unsafe { (*addr_of_mut!(TEMPLATES)).as_ref()? };
    let def = defs.get(name)?;
    let model = def.get("model").and_then(Value::as_str)?;
    let template = templates.get(model)?;
    let source = super::build::HullSource { name, def, template, slot: 0, slot_matrix: &[0.0; 16] };
    let (hull, extras) = super::build::build(&source, 0, Ref::default(), 0);
    Some((hull.pose?, extras.seat_turn_rates))
}

/// A hull's whole pose state from JavaScript (posing::pull), `count`
/// numbers in the input, for the hull in store slot `slot` on life
/// `generation`: its pose is the module's from now until its next life.
/// Returns 1, or 0 where it cannot be (no pack, or counts that disagree:
/// JavaScript keeps posing it).
#[no_mangle]
pub extern "C" fn veh_pull(slot: u32, generation: f64, count: u32) -> u32 {
    let s = sim();
    let r = s.store.vehicles.vehicles.iter().copied().find(|r| r.slot == slot);
    let Some(r) = r else { return 0 };
    let name = String::from_utf16_lossy(s.strings.units(s.store.vehicle.get(r).expect("a hull the store holds").name));
    let all = poses();
    if all.len() <= slot as usize {
        all.resize_with(slot as usize + 1, || None);
    }
    // The rig is the class's: made once a hull, kept over its lives.
    let made = match all[slot as usize].take() {
        Some(p) => Some((p.pose, p.turn_rates)),
        None => new_pose(&name),
    };
    let Some((mut pose, turn_rates)) = made else { return 0 };
    let ok = posing::pull(&mut pose, &input()[..count as usize]);
    all[slot as usize] = Some(Posed { generation: if ok { generation } else { f64::NAN }, pose, turn_rates });
    ok as u32
}

/// The pose inputs JavaScript changed since the last poses (posing::inputs),
/// `count` numbers in the input: for each hull the module poses, its slot
/// and then its inputs.
#[no_mangle]
pub extern "C" fn veh_pose_inputs(count: u32) {
    let v = &input()[..count as usize];
    let all = poses();
    let mut i = 0;
    while i < v.len() {
        let slot = v[i] as usize;
        i += 1;
        let Some(Some(p)) = all.get_mut(slot) else { panic!("pose inputs for a hull the module does not pose") };
        i += posing::inputs(&mut p.pose, &v[i..]);
    }
}

/// What the last phase's poses left (posing::out), for JavaScript: the
/// number of hulls posed, then for each its slot, how many numbers follow
/// and those numbers. Returns its address.
#[no_mangle]
pub extern "C" fn veh_posed() -> *const f64 {
    unsafe { (*addr_of_mut!(POSED)).as_ptr() }
}

/// Vehicles.syncCrews' part for the hulls the module poses (the crafts'
/// riders stay JavaScript's): for each live posed hull in the battle's
/// order, each seat a bot holds (a lease not a person's), its rider as
/// poseRider places him: the hull slot and seat, the point he rides at
/// (his seat's rider node in the world, else the hull's middle), the
/// seat's aim, the hull's velocity, whether he shows (a rider node), and a
/// 9-pose's pitch and turn (riderSample: ninePose for a turret seat, the
/// pitch alone for the pilot's); JavaScript picks his clip, poses him and
/// counts his tick (src/vehicles.js syncRiderFrom). Returns the list's
/// address: [riders, then 13 numbers each].
#[no_mangle]
pub extern "C" fn veh_riders() -> *const f64 {
    let out = unsafe { &mut *addr_of_mut!(RIDERS) };
    out.clear();
    out.push(0.0);
    let mut count = 0;
    for (i, &r) in hull_refs().iter().enumerate() {
        let v = sim().store.vehicle.get(r).expect("a hull the store holds");
        if !v.alive || v.flying {
            continue;
        }
        let generation = v.target_generation;
        let Some(Some(p)) = poses().get_mut(r.slot as usize) else { continue };
        if p.generation != generation {
            continue;
        }
        let hull = load(r, i);
        for (k, seat) in hull.seats.iter().enumerate() {
            match seat.lease {
                Some(lease) if !lease.human => {}
                _ => continue,
            }
            let rider = p.pose.rig.seats.get(k).and_then(|s| s.rider.as_ref());
            let node = rider.and_then(|r| r.node);
            let point = match node {
                Some(node) => {
                    let mut at = Vector3::default();
                    p.pose.rig.tree.get_world_position(node, &mut at)
                }
                None => {
                    let mut at = hull.position;
                    at.add_scaled_vector(&super::hull::UP, hull.size.y * 0.5);
                    at
                }
            };
            let (pitch, turn) = match rider {
                Some(r) if r.nine_pose && k > 0 => {
                    let yaw_rate = super::hull::or_zero(p.pose.seats[k].yaw_rate);
                    let turn_rate = p.turn_rates.get(k).copied().unwrap_or(f64::NAN);
                    super::pose::nine_pose(seat.pitch_limits, seat.yaw_limits, seat.wrap_yaw, turn_rate, super::hull::angle(seat.aim_yaw - hull.yaw), seat.pitch, yaw_rate)
                }
                Some(r) if r.nine_pose => (-seat.pitch / 0.7, 0.0),
                _ => (0.0, 0.0),
            };
            out.extend_from_slice(&[r.slot as f64, k as f64, point.x, point.y, point.z, seat.aim_yaw, seat.pitch, hull.velocity.x, hull.velocity.y, hull.velocity.z, if node.is_some() { 1.0 } else { 0.0 }, pitch, turn]);
            count += 1;
        }
    }
    out[0] = count as f64;
    out.as_ptr()
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
#[no_mangle]
pub extern "C" fn veh_flight_env(json: u32, max_fly_height: f64) {
    let s = sim();
    let text = String::from_utf16_lossy(s.strings.units(StrId(json)));
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
    unsafe {
        *addr_of_mut!(FLIGHT_ENV) = Some(environment);
        (*addr_of_mut!(TUNINGS)).get_or_insert_with(Tunings::default);
    }
}

/// FLIGHT_CLASSES[class][key] = value (a class's tuning: at attach, and
/// as a world's flightTuning command retunes it); 1 if it took.
#[no_mangle]
pub extern "C" fn veh_flight_tune(class: u32, key: u32, value: f64) -> u32 {
    let s = sim();
    let class = String::from_utf16_lossy(s.strings.units(StrId(class)));
    let key = String::from_utf16_lossy(s.strings.units(StrId(key)));
    let t = unsafe { (*addr_of_mut!(TUNINGS)).get_or_insert_with(Tunings::default) };
    t.retune(&class, &key, value) as u32
}

/// The registry's entries this phase (units.all(), in its order), ten
/// numbers each in the input: kind (0 unit, 1 hull, 2 other), id,
/// generation (NaN: undefined), alive, whether it has a team and the team,
/// a hull's slot in the store, and another's aim point.
#[no_mangle]
pub extern "C" fn veh_members(count: u32) {
    let v = &input()[..count as usize * 10];
    let rows = unsafe { &mut *addr_of_mut!(MEMBERS) };
    rows.clear();
    let cache = unsafe { &mut *addr_of_mut!(AIM_CACHE) };
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

/// The relation of every team among the entries and hulls to every other
/// (Units.relation), `count` teams in the input as (has a team, team)
/// pairs, then the count x count relations, row by row.
#[no_mangle]
pub extern "C" fn veh_relations(count: u32) {
    let n = count as usize;
    let v = &input()[..n * 2 + n * n];
    unsafe {
        *addr_of_mut!(TEAMS) = (0..n).map(|i| team(v[2 * i], v[2 * i + 1])).collect();
        *addr_of_mut!(RELATIONS) = v[n * 2..].to_vec();
    }
}

/// A pilot's input as numbers (src/vehicles-sim.js flightNumbers): 19 each.
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
#[no_mangle]
pub extern "C" fn veh_flight_inputs(count: u32) {
    let v = &input()[..count as usize * 40];
    let all = unsafe { &mut *addr_of_mut!(FLIGHT_INPUTS) };
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

/// The crafts' time left over after the last phase's flight steps.
#[no_mangle]
pub extern "C" fn veh_flight_acc() -> f64 {
    unsafe { *addr_of_mut!(FLIGHT_ACC) }
}

/// What the crafts' targeting left in their weapons this phase: [count,
/// then per craft its slot, the lock's target id, generation, elapsed time
/// and whether it is complete]. Returns its address.
#[no_mangle]
pub extern "C" fn veh_locks() -> *const f64 {
    unsafe { (*addr_of_mut!(LOCKS)).as_ptr() }
}

/// A class's flight keys (flightConfig's own, from its definition) and its
/// takeoff clip (the gear's), from the pack.
fn flight_keys(name: &str) -> (Config, Option<GearClip>) {
    let keys = unsafe { (*addr_of_mut!(FLIGHT_KEYS)).get_or_insert_with(HashMap::new) };
    if let Some(k) = keys.get(name) {
        return k.clone();
    }
    let defs = unsafe { (*addr_of_mut!(DEFS)).as_ref() }.expect("the vehicle pack (flight needs its classes)");
    let def = defs.get(name).unwrap_or_else(|| panic!("hull class {name}: not in the pack"));
    let templates = unsafe { (*addr_of_mut!(TEMPLATES)).as_ref() }.expect("the vehicle pack");
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

/// A craft as the phase works on it: the hull with its flight.
fn load_flyer(r: Ref<Vehicle>, index: usize) -> Hull {
    let mut hull = load(r, index);
    read_flight(&mut hull);
    hull
}

/// A craft's flight from the store.
fn read_flight(hull: &mut Hull) {
    let s = sim();
    let v = s.store.vehicle.get(hull.id).expect("a hull the store holds");
    let name = String::from_utf16_lossy(s.strings.units(v.name));
    let (config, gear) = flight_keys(&name);
    hull.flight = Flight::from_store(v, &s.strings, config, gear).unwrap_or_else(|e| panic!("{name}'s flight: {e}"));
}

/// A hull back into the store, a craft's flight with it.
fn store_hull(hull: &Hull) {
    store(hull);
    if hull.flying {
        let s = sim();
        let v = s.store.vehicle.get_mut(hull.id).expect("a hull the store holds");
        hull.flight.to_store(v, &mut s.strings);
    }
}

/// An idle stick: { forward: 0, right: 0, turn: 0, brake: false }.
fn idle_input() -> FlightInput {
    FlightInput { forward: 0.0, right: 0.0, turn: 0.0, ..FlightInput::default() }
}

/// The battle's hulls, in its list's order.
fn hull_refs() -> Vec<Ref<Vehicle>> {
    sim().store.vehicles.vehicles.clone()
}

/// Hull `index` (its store handle `r`) as the phase works on it.
fn load(r: Ref<Vehicle>, index: usize) -> Hull {
    let s = sim();
    let v = s.store.vehicle.get(r).expect("a hull the store holds");
    let name = String::from_utf16_lossy(s.strings.units(v.name));
    let class = classes().get(&name).unwrap_or_else(|| panic!("hull class {name}: not given to the phase"));
    Hull::from_store(r, v, class, index, &s.strings)
}

/// A hull back into the store.
fn store(hull: &Hull) {
    let s = sim();
    let v = s.store.vehicle.get_mut(hull.id).expect("a hull the store holds");
    hull.to_store(v, &mut s.strings);
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
/// with `poses`, each ground hull the module poses posed for the tick
/// (aimTick, then render where `cosmetic`, as Vehicles.update poses every
/// hull), at the tick's alpha. Returns what is left over. `level`: the
/// water's level this phase (`water.level`), for a map with water;
/// `clock`: the clock barrels kick back on.
#[no_mangle]
pub extern "C" fn veh_ground(dt: f64, acc: f64, flight_acc: f64, level: f64, clock: f64, flags: u32) -> f64 {
    let (poses_on, cosmetic, flight_on, assists) = (flags & 1, flags & 2, flags & 4, flags & 8);
    if let Some(m) = map() {
        m.level = level;
    }
    unsafe { *addr_of_mut!(CLOCK) = Some(clock) };
    let refs = hull_refs();
    let mut world = JsWorld { craft: u32::MAX };
    let locks = unsafe { &mut *addr_of_mut!(LOCKS) };
    locks.clear();
    locks.push(0.0);
    if flight_on != 0 {
        if let Some(env) = unsafe { (*addr_of_mut!(FLIGHT_ENV)).as_mut() } {
            env.assists = assists != 0;
        }
        // The crafts first (Vehicles.update): each live craft's tick start
        // and step, at 1/60 s, as many as the time left over makes.
        let mut flight_acc = flight_acc + dt;
        let mut last: Vec<Option<crate::flight::state::Lock>> = vec![None; refs.len()];
        while flight_acc + 1e-9 >= 1.0 / 60.0 {
            for (i, &r) in refs.iter().enumerate() {
                let v = sim().store.vehicle.get(r).expect("a hull the store holds");
                if !(v.alive && v.flying) {
                    continue;
                }
                let mut hull = load_flyer(r, i);
                hull.capture_tick();
                hull.capture_flight_tick();
                let given = unsafe { (&*addr_of_mut!(FLIGHT_INPUTS)).get(r.slot as usize).copied().flatten() };
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
                store_hull(&hull);
            }
            flight_acc -= 1.0 / 60.0;
        }
        unsafe { *addr_of_mut!(FLIGHT_ACC) = flight_acc };
        for (i, lock) in last.iter().enumerate() {
            if let Some(l) = lock {
                locks.extend_from_slice(&[refs[i].slot as f64, l.target_id, l.generation, l.elapsed, if l.complete { 1.0 } else { 0.0 }]);
                locks[0] += 1.0;
            }
        }
    }
    for (i, &r) in refs.iter().enumerate() {
        let v = sim().store.vehicle.get(r).expect("a hull the store holds");
        if v.alive && !v.flying {
            let mut hull = load(r, i);
            hull.capture_tick();
            store(&hull);
        }
    }
    let mut acc = acc + dt;
    while acc + 1e-9 >= FIXED_STEP {
        for (i, &r) in refs.iter().enumerate() {
            let v = sim().store.vehicle.get(r).expect("a hull the store holds");
            if v.alive && !v.flying {
                let mut hull = load(r, i);
                let input = input_of(&hull);
                hull.tick(&mut world, FIXED_STEP, &input);
                store(&hull);
            }
        }
        acc -= FIXED_STEP;
    }
    let out = unsafe { &mut *addr_of_mut!(POSED) };
    out.clear();
    out.push(0.0);
    if poses_on == 0 {
        return acc;
    }
    let cosmetic = cosmetic != 0;
    for (i, &r) in refs.iter().enumerate() {
        let v = sim().store.vehicle.get(r).expect("a hull the store holds");
        if !v.alive && v.dying > 0.0 {
            let mut hull = load(r, i);
            hull.dying_tick(&mut world, dt);
            store(&hull);
        }
    }
    let alpha = v8math::max(0.0, acc / FIXED_STEP);
    // The ground hulls whose pose is the module's (this life's), in order.
    let mine: Vec<(usize, Ref<Vehicle>)> = refs
        .iter()
        .enumerate()
        .filter(|&(_, &r)| {
            let v = sim().store.vehicle.get(r).expect("a hull the store holds");
            !v.flying && matches!(poses().get(r.slot as usize), Some(Some(p)) if p.generation == v.target_generation)
        })
        .map(|(i, &r)| (i, r))
        .collect();
    let mut posed = vec![false; mine.len()];
    for pass in 0..2 {
        if pass == 1 && !cosmetic {
            break;
        }
        for (k, &(i, r)) in mine.iter().enumerate() {
            let mut hull = load(r, i);
            // The hull holds its pose while it poses (nothing it calls
            // reaches the poses).
            let Posed { generation, pose, turn_rates } = poses()[r.slot as usize].take().expect("the hull's pose");
            hull.pose = Some(pose);
            if hull.alive && !hull.rests(dt) {
                posed[k] = true;
                if pass == 0 {
                    hull.aim_tick(&mut world, alpha, dt);
                } else {
                    hull.render(&mut world, alpha, dt, true);
                }
            }
            poses()[r.slot as usize] = Some(Posed { generation, pose: hull.pose.take().expect("the hull's pose"), turn_rates });
            store(&hull);
        }
    }
    let mut count = 0;
    let mut numbers = Vec::new();
    for (k, &(_, r)) in mine.iter().enumerate() {
        if !posed[k] {
            continue;
        }
        numbers.clear();
        posing::out(&mut poses()[r.slot as usize].as_mut().expect("the hull's pose").pose, &mut numbers);
        out.push(r.slot as f64);
        out.push(numbers.len() as f64);
        out.extend_from_slice(&numbers);
        count += 1;
    }
    out[0] = count as f64;
    acc
}

fn v3(a: &Vector3) -> Vec3 {
    [a.x, a.y, a.z]
}

fn vec(a: &Vec3) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}

fn body_word(body: Option<BodyId>) -> f64 {
    body.map_or(-1.0, |b| b as f64)
}

fn hit_of(h: VehHit) -> Option<Hit> {
    h.found.then(|| Hit { distance: h.distance, point: vec(&h.point), normal: vec(&h.normal), terrain: h.terrain, body: (h.body >= 0.0).then_some(h.body as BodyId) })
}

/// The hull's body (its number in the store), by its place in the list.
fn body_of(hull: usize) -> BodyId {
    let s = sim();
    let r = s.store.vehicles.vehicles[hull];
    s.store.vehicle.get(r).expect("a hull the store holds").body.slot
}

/// The battle: the module's own collision world, water and death regions
/// where it holds them (native.rs), the imports for the rest. `craft`: the
/// craft whose flight steps now (its store slot), for its registry entry.
pub struct JsWorld {
    craft: u32,
}

impl VehicleWorld for JsWorld {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> {
        if let Some(hit) = collision_world().and_then(|c| c.cast(crate::collision::state(), origin, direction, far, ignore)) {
            return hit;
        }
        hit_of(import_veh_cast(&VehRay { origin: v3(origin), direction: v3(direction), far, ignore: body_word(ignore) }))
    }

    fn push_walls(&mut self, hull: usize, probes: &[Vector3], radius: f64, q: &Quaternion, own: Option<&Quaternion>, previous: &Vector3, position: &mut Vector3, velocity: &mut Vector3, terrain: bool) -> bool {
        if let Some(c) = collision_world() {
            if let Some(undo) = c.push_walls(crate::collision::state(), body_of(hull), probes, radius, q, own, previous, position, velocity, terrain) {
                return undo;
            }
        }
        let r = import_veh_push_walls(&VehPush {
            hull: hull as f64,
            probes: probes.iter().map(v3).collect(),
            radius,
            q: [q.x, q.y, q.z, q.w],
            own: own.map(|o| [o.x, o.y, o.z, o.w]),
            previous: v3(previous),
            position: v3(position),
            velocity: v3(velocity),
            terrain,
        });
        *position = vec(&r.position);
        *velocity = vec(&r.velocity);
        r.undo
    }

    fn ground(&mut self, x: f64, z: f64, top: f64) -> Option<Ground> {
        if let Some(c) = collision_world() {
            return c.ground(crate::collision::state(), x, z, top);
        }
        let g = import_veh_ground(&VehColumn { x, z, top });
        g.found.then(|| Ground { height: g.height, normal: vec(&g.normal) })
    }

    fn spring_polygons(&mut self, hull: usize, index: usize, center: &Vector3, radius: f64, out: &mut Vec<Polygon>) {
        if let Some(c) = collision_world() {
            if c.spring_polygons(crate::collision::state(), body_of(hull), index, center, radius, out).is_some() {
                return;
            }
        }
        let r = import_veh_spring_polygons(&VehSpring { hull: hull as f64, index: index as f64, center: v3(center), radius });
        out.clear();
        out.extend(r.polygons.iter().map(|p| Polygon { id: p.id, moving: p.moving, triangles: p.triangles.iter().map(|t| [vec(&t.a), vec(&t.b), vec(&t.c)]).collect() }));
    }

    fn move_body(&mut self, body: BodyId, matrix: &Matrix4) {
        if collision_world().and_then(|c| c.move_body(crate::collision::state(), body, matrix)).is_some() {
            return;
        }
        import_veh_move_body(&VehBody { body: body as f64, matrix: matrix.elements, enabled: false });
    }

    fn enable_body(&mut self, body: BodyId, enabled: bool) {
        if collision_world().and_then(|c| c.enable_body(crate::collision::state(), body, enabled)).is_some() {
            return;
        }
        import_veh_enable_body(&VehBody { body: body as f64, matrix: [0.0; 16], enabled });
    }

    fn body_enabled(&mut self, body: BodyId) -> bool {
        if let Some(on) = collision_world().and_then(|c| c.body_enabled(crate::collision::state(), body)) {
            return on;
        }
        import_veh_body_enabled(&VehBody { body: body as f64, matrix: [0.0; 16], enabled: false }).value
    }

    fn water_over(&mut self, x: f64, z: f64) -> bool {
        if let Some(m) = map() {
            return m.water.as_ref().is_some_and(|w| w.over(x, z));
        }
        import_veh_water_over(&VehColumn { x, z, top: 0.0 }).value
    }

    fn water_level(&mut self) -> f64 {
        if let Some(m) = map() {
            return m.level;
        }
        import_veh_water_level(&VehNothing {}).value
    }

    fn has_water(&mut self) -> bool {
        if let Some(m) = map() {
            return m.water.is_some();
        }
        import_veh_has_water(&VehNothing {}).value
    }

    fn in_death_region(&mut self, position: &Vector3) -> bool {
        if let Some(m) = map() {
            return m.in_death_region(position);
        }
        import_veh_in_death_region(&VehPoint { position: v3(position) }).value
    }

    fn damaged(&mut self, hull: usize, amount: f64, attacker: &OrdnanceOwner) {
        import_veh_damaged(&VehDamaged { hull: hull as f64, amount, attacker: *attacker });
    }

    fn destroyed_crew(&mut self, hull: usize) {
        import_veh_destroyed_crew(&VehHull { hull: hull as f64 });
    }

    fn destroyed(&mut self, hull: usize) {
        import_veh_destroyed(&VehHull { hull: hull as f64 });
    }

    fn respawn_clock(&mut self, hull: usize) {
        import_veh_respawn_clock(&VehHull { hull: hull as f64 });
    }

    fn release_crew(&mut self, hull: usize) {
        import_veh_release_crew(&VehHull { hull: hull as f64 });
    }

    fn explosion(&mut self, hull: usize, final_explosion: bool, position: &Vector3) {
        import_veh_explosion(&VehBlast { hull: hull as f64, r#final: final_explosion, position: v3(position) });
    }

    fn effect(&mut self, name: &str, position: &Vector3) {
        let name = sim().strings.intern_str(name);
        import_veh_effect(&VehEffect { name, position: v3(position) });
    }

    fn raycast_units(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<f64> {
        let h = import_veh_raycast_units(&VehRay { origin: v3(origin), direction: v3(direction), far, ignore: -1.0 });
        h.found.then_some(h.distance)
    }

    fn clock(&mut self) -> f64 {
        if let Some(clock) = unsafe { *addr_of_mut!(CLOCK) } {
            return clock;
        }
        import_veh_clock(&VehNothing {}).value
    }

    fn tow_released(&mut self, _hull: usize, _reason: &str) {
        unreachable!("the tow cables are JavaScript's in this phase")
    }

    fn current(&mut self, hull: &Hull) {
        store_hull(hull);
    }

    fn reload(&mut self, hull: &mut Hull) {
        let s = sim();
        let v = s.store.vehicle.get(hull.id).expect("a hull the store holds");
        hull.load(v, &s.strings);
        if hull.flying {
            read_flight(hull);
        }
    }
}

/// A hull's aim point (VehicleTarget.aimPoint: Vehicle.sphere, the middle
/// of its box as it stands now).
fn hull_point(slot: u32) -> Option<(bool, BodyId, Vector3)> {
    let s = sim();
    let r = s.store.vehicles.vehicles.iter().copied().find(|r| r.slot == slot)?;
    let v = s.store.vehicle.get(r)?;
    let mut m = Matrix4::default();
    m.from_array(&v.matrix, 0);
    let mut point = Vector3::new(v.center[0], v.center[1], v.center[2]);
    point.apply_matrix4(&m);
    Some((v.alive, v.body.slot, point))
}

impl FlightWorld for JsWorld {
    fn environment(&mut self) -> &Environment {
        unsafe { (*addr_of_mut!(FLIGHT_ENV)).as_ref() }.expect("the flight environment (veh_flight_env)")
    }

    fn tuning(&mut self, class: FlightClass) -> Tuning {
        unsafe { (*addr_of_mut!(TUNINGS)).get_or_insert_with(Tunings::default) }.of(class)
    }

    fn member_count(&mut self) -> usize {
        unsafe { (&*addr_of_mut!(MEMBERS)).len() }
    }

    fn member(&mut self, i: usize) -> Member {
        let m = unsafe { (&*addr_of_mut!(MEMBERS))[i] };
        match m.kind {
            MEMBER_UNIT => Member { id: m.id, generation: m.generation, alive: m.alive, team: m.team, own: false, unit: true, body: None, point: Vector3::default() },
            MEMBER_HULL => {
                let (alive, body, point) = hull_point(m.hull).expect("a hull entry's hull");
                Member { id: m.id, generation: m.generation, alive, team: m.team, own: m.hull == self.craft, unit: false, body: Some(body), point }
            }
            MEMBER_ON_HULL => {
                let (_, body, point) = hull_point(m.hull).expect("an entry's hull");
                Member { id: m.id, generation: m.generation, alive: m.alive, team: m.team, own: false, unit: false, body: Some(body), point }
            }
            _ => Member { id: m.id, generation: m.generation, alive: m.alive, team: m.team, own: false, unit: false, body: None, point: m.point },
        }
    }

    fn relation(&mut self, a: Team, b: Team) -> f64 {
        let teams = unsafe { &*addr_of_mut!(TEAMS) };
        let same = |x: &Team, y: &Team| match (x, y) {
            (None, None) => true,
            (Some(x), Some(y)) => x == y || x.is_nan() && y.is_nan(),
            _ => false,
        };
        let (Some(i), Some(j)) = (teams.iter().position(|t| same(t, &a)), teams.iter().position(|t| same(t, &b))) else {
            panic!("a team the relations do not name: {a:?} {b:?}")
        };
        unsafe { (&*addr_of_mut!(RELATIONS))[i * teams.len() + j] }
    }

    fn aim_point(&mut self, i: usize) -> Vector3 {
        let m = unsafe { (&*addr_of_mut!(MEMBERS))[i] };
        match m.kind {
            MEMBER_UNIT => {
                if let Some(Some(p)) = unsafe { (&*addr_of_mut!(AIM_CACHE)).get(i) } {
                    return *p;
                }
                let p = vec(&import_veh_aim_point(&VehHull { hull: i as f64 }).position);
                if let Some(slot) = unsafe { (&mut *addr_of_mut!(AIM_CACHE)).get_mut(i) } {
                    *slot = Some(p);
                }
                p
            }
            MEMBER_HULL | MEMBER_ON_HULL => hull_point(m.hull).expect("a hull entry's hull").2,
            _ => m.point,
        }
    }

    fn prefetch_aim_points(&mut self, members: &[usize]) {
        let rows = unsafe { &*addr_of_mut!(MEMBERS) };
        let cache = unsafe { &*addr_of_mut!(AIM_CACHE) };
        let wanted: Vec<f64> = members.iter().copied().filter(|&i| rows[i].kind == MEMBER_UNIT && cache[i].is_none()).map(|i| i as f64).collect();
        if wanted.is_empty() {
            return;
        }
        let points = import_veh_aim_points(&VehMembers { indices: wanted.clone() }).points;
        let cache = unsafe { &mut *addr_of_mut!(AIM_CACHE) };
        for (k, &i) in wanted.iter().enumerate() {
            cache[i as usize] = Some(vec(&points[k]));
        }
    }

    fn body_is_hull(&mut self, body: BodyId) -> bool {
        let s = sim();
        s.store.vehicles.vehicles.iter().any(|&r| s.store.vehicle.get(r).is_some_and(|v| v.body.slot == body))
    }

    fn system_death_region(&mut self, position: &Vector3) -> bool {
        self.in_death_region(position)
    }
}
