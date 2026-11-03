//! The hull phase on the module, behind its switch (src/vehicles-sim.js;
//! off unless a world asks for it): the ground hulls' tick start and their
//! 120 Hz substeps (src/vehicles.js Vehicles.update), run on the hulls as
//! the store holds them, JavaScript writing them in before the phase and
//! syncing its Vehicle objects from the store after it. What the hulls ask
//! of the battle comes back as the imports of rust/sim/schema/vehicles.mjs,
//! served by the same VehiclesWorld (src/vehicles-world.js) the JavaScript
//! hulls call.
//!
//! As the rounds' module does (rust/sim/src/ordnance/module.rs), the store
//! is reached through short borrows taken from the instance's one `Sim` and
//! never held across an import: a hook JavaScript serves (a hull destroyed,
//! its blast) can reach every hull. Before such a hook the hull whose method
//! calls it goes back into the store and JavaScript syncs its hulls from it;
//! after it JavaScript writes its hulls back and the hull is read again
//! (`current`, `reload`), so a hook sees and changes the hulls as it does
//! when the hulls are JavaScript's.

use super::class::HullClass;
use super::hull::{Hull, Input};
use super::{BodyId, Ground, Hit, Polygon, VehicleWorld};
use crate::abi::sim;
use crate::gen::*;
use crate::store::{Ref, StrId};
use std::collections::HashMap;
use std::ptr::addr_of_mut;
use three::{Matrix4, Quaternion, Vector3};

/// vehicles.js FIXED_STEP: a ground hull's substep.
const FIXED_STEP: f64 = 1.0 / 120.0;

static mut CLASSES: Option<HashMap<String, HullClass>> = None;

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
/// seat 0's frozen input or none. Returns what is left over.
#[no_mangle]
pub extern "C" fn veh_ground(dt: f64, acc: f64) -> f64 {
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

/// The battle as the module's imports give it.
pub struct JsWorld;

impl VehicleWorld for JsWorld {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> {
        hit_of(import_veh_cast(&VehRay { origin: v3(origin), direction: v3(direction), far, ignore: body_word(ignore) }))
    }

    fn push_walls(&mut self, hull: usize, probes: &[Vector3], radius: f64, q: &Quaternion, own: Option<&Quaternion>, previous: &Vector3, position: &mut Vector3, velocity: &mut Vector3, terrain: bool) -> bool {
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
        let g = import_veh_ground(&VehColumn { x, z, top });
        g.found.then(|| Ground { height: g.height, normal: vec(&g.normal) })
    }

    fn spring_polygons(&mut self, hull: usize, index: usize, center: &Vector3, radius: f64, out: &mut Vec<Polygon>) {
        let r = import_veh_spring_polygons(&VehSpring { hull: hull as f64, index: index as f64, center: v3(center), radius });
        out.clear();
        out.extend(r.polygons.iter().map(|p| Polygon { id: p.id, moving: p.moving, triangles: p.triangles.iter().map(|t| [vec(&t.a), vec(&t.b), vec(&t.c)]).collect() }));
    }

    fn move_body(&mut self, body: BodyId, matrix: &Matrix4) {
        import_veh_move_body(&VehBody { body: body as f64, matrix: matrix.elements, enabled: false });
    }

    fn enable_body(&mut self, body: BodyId, enabled: bool) {
        import_veh_enable_body(&VehBody { body: body as f64, matrix: [0.0; 16], enabled });
    }

    fn body_enabled(&mut self, body: BodyId) -> bool {
        import_veh_body_enabled(&VehBody { body: body as f64, matrix: [0.0; 16], enabled: false }).value
    }

    fn water_over(&mut self, x: f64, z: f64) -> bool {
        import_veh_water_over(&VehColumn { x, z, top: 0.0 }).value
    }

    fn water_level(&mut self) -> f64 {
        import_veh_water_level(&VehNothing {}).value
    }

    fn has_water(&mut self) -> bool {
        import_veh_has_water(&VehNothing {}).value
    }

    fn in_death_region(&mut self, position: &Vector3) -> bool {
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
