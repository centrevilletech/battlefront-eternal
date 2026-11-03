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
static mut POSES: Vec<Option<(f64, HullPose)>> = Vec::new();
static mut POSED: Vec<f64> = Vec::new();
static mut CLOCK: Option<f64> = None;
static mut BYTES: Vec<u8> = Vec::new();

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

fn poses() -> &'static mut Vec<Option<(f64, HullPose)>> {
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
/// model (build.rs), or None where the pack lacks either.
fn new_pose(name: &str) -> Option<HullPose> {
    let defs = unsafe { (*addr_of_mut!(DEFS)).as_ref()? };
    let templates = unsafe { (*addr_of_mut!(TEMPLATES)).as_ref()? };
    let def = defs.get(name)?;
    let model = def.get("model").and_then(Value::as_str)?;
    let template = templates.get(model)?;
    let source = super::build::HullSource { name, def, template, slot: 0, slot_matrix: &[0.0; 16] };
    super::build::build(&source, 0, Ref::default(), 0).0.pose
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
    let pose = match all[slot as usize].take() {
        Some((_, pose)) => Some(pose),
        None => new_pose(&name),
    };
    let Some(mut pose) = pose else { return 0 };
    let ok = posing::pull(&mut pose, &input()[..count as usize]);
    all[slot as usize] = Some((if ok { generation } else { f64::NAN }, pose));
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
        let Some(Some((_, pose))) = all.get_mut(slot) else { panic!("pose inputs for a hull the module does not pose") };
        i += posing::inputs(pose, &v[i..]);
    }
}

/// What the last phase's poses left (posing::out), for JavaScript: the
/// number of hulls posed, then for each its slot, how many numbers follow
/// and those numbers. Returns its address.
#[no_mangle]
pub extern "C" fn veh_posed() -> *const f64 {
    unsafe { (*addr_of_mut!(POSED)).as_ptr() }
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
pub extern "C" fn veh_ground(dt: f64, acc: f64, level: f64, clock: f64, poses_on: u32, cosmetic: u32) -> f64 {
    if let Some(m) = map() {
        m.level = level;
    }
    unsafe { *addr_of_mut!(CLOCK) = Some(clock) };
    let refs = hull_refs();
    let mut world = JsWorld;
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
            !v.flying && matches!(poses().get(r.slot as usize), Some(Some((generation, _))) if *generation == v.target_generation)
        })
        .map(|(i, &r)| (i, r))
        .collect();
    let mut posed = vec![false; mine.len()];
    for pass in 0..2 {
        if pass == 1 && cosmetic == 0 {
            break;
        }
        for (k, &(i, r)) in mine.iter().enumerate() {
            let mut hull = load(r, i);
            // The hull holds its pose while it poses (nothing it calls
            // reaches the poses).
            let (generation, pose) = poses()[r.slot as usize].take().expect("the hull's pose");
            hull.pose = Some(pose);
            if hull.alive && !hull.rests(dt) {
                posed[k] = true;
                if pass == 0 {
                    hull.aim_tick(&mut world, alpha, dt);
                } else {
                    hull.render(&mut world, alpha, dt, true);
                }
            }
            poses()[r.slot as usize] = Some((generation, hull.pose.take().expect("the hull's pose")));
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
        posing::out(&mut poses()[r.slot as usize].as_mut().expect("the hull's pose").1, &mut numbers);
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
/// where it holds them (native.rs), the imports for the rest.
pub struct JsWorld;

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
        store(hull);
    }

    fn reload(&mut self, hull: &mut Hull) {
        let s = sim();
        let v = s.store.vehicle.get(hull.id).expect("a hull the store holds");
        hull.load(v, &s.strings);
    }
}
