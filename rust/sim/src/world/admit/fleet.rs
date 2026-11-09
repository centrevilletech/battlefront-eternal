//! Stage Fleet as game.js createGame makes it on the host side: `new
//! Vehicles(...)` (src/vehicles.js), then attachF2 (src/sim/wasm/f2-sim.js
//! F2Sim). The owners' row builders (rows/fleet.rs, rows/vehicle_pack.rs,
//! bodies/seats.rs) make the rows; this module makes, inside the owning
//! world, the objects the page made in JavaScript between them: the
//! models' collision shapes and the vehicle blockers, each pad's hull
//! (vehicles/build.rs, the Vehicle constructor) with its moving body, its
//! spawn on its pad (Hull::spawn), the store rows the page imported and the
//! pose values it pulled (vehicle-phase-data.js pullPose).
use super::rows::{fleet, units, Call, Cx};
use super::{bodies, sequence, AdmitError, Rows, Stage};
use crate::collision::spheres::{box_apply_matrix4, Bounds};
use crate::gen::{BeamSource, FfBirth, FfLeg, FfMuzzle, FfSeatBirth, FfWeaponBirth, Member, SeatHullFacts, SpawnSlot, Vehicle, Weapon};
use crate::store::Ref;
use crate::vehicles::build::{base_of, build, local_bounds, HullExtras, HullSource};
use crate::vehicles::hull::Hull;
use crate::vehicles::pose::HullPose;
use crate::vehicles::spawn::{footprints_overlap, SlotDesc};
use crate::vehicles::{posing, BodyId, Ground, Hit, Polygon, SpringPolygons, VehicleWorld};
use crate::world::owner::World;
use bfe_content::bvh::{build as build_bvh, Positions};
use bfe_content::collision::BodyTriangles;
use bfe_content::install::CollisionKernel;
use bfe_content::js::{parse_float_value, string_to_number, to_number};
use bfe_content::Value;
use three::{Matrix4, Quaternion, Vector3};

fn error(message: impl Into<String>) -> AdmitError {
    AdmitError::Content { stage: Stage::Fleet, message: message.into() }
}

/// A hull constructSpawns made: its place in Vehicles.vehicles, its pad,
/// its class, its store row, its kernel body and whether Vehicle.spawn set
/// it down (an overlapping pad leaves it dormant).
pub struct Made {
    pub index: usize,
    pub pad: usize,
    pub name: String,
    pub hull: Ref<Vehicle>,
    /// Its kernel body and the body object's identity in the host's pool.
    pub body: u32,
    pub body_ref: Ref<crate::gen::Body>,
    pub spawned: bool,
    /// A prepared variant (prepareVariants), never spawned at load.
    pub prepared: bool,
    /// Its registry id (Vehicles.attachUnits), 0 for a prepared variant.
    pub target: u32,
    pub template: usize,
    /// Its collider as VehicleContent.collider registered it: the header
    /// and the wall and spring cache ids.
    pub collider: Option<(crate::gen::F2ColliderHeader, Vec<i32>, Vec<i32>)>,
}

/// What `new Vehicles` made beside the hulls: the templates' trees (by
/// template) and the blockers' tree.
struct Shapes {
    trees: Vec<u32>,
    blockers: Option<u32>,
}

/// CollisionWorld as a hull's constructor and spawn see it before attachF2
/// (src/collision.js): casts through the kernel's world directly (rayCast:
/// the bodies' order refreshed, the broad phase, every body enabled as the
/// page's plain flags all are at load), bodies moved by writing their
/// bounds and transforms (moveBody through rays.place). Nothing else is
/// asked of it before the fleet owner exists.
struct ColdWorld<'a> {
    world: &'a mut World,
    kernel: usize,
    /// Each moving body's local bounds (its shape's), for moveBody.
    locals: &'a [(u32, Bounds)],
}

impl VehicleWorld for ColdWorld<'_> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> {
        let c = &mut self.world.collision;
        // rayCandidates: rayBodies first (the kernel's order is the bodies'
        // creation order, none removed yet), then the broad phase, then each
        // candidate's enabled flag (every body is enabled at load).
        let bodies = c.state.bodies.len();
        for (i, slot) in c.world_order_slots(self.kernel as u32, bodies as u32).iter_mut().enumerate() { *slot = i as u32; }
        let ray = crate::collision::Ray { ox: origin.x, oy: origin.y, oz: origin.z, dx: direction.x, dy: direction.y, dz: direction.z };
        let mut candidates = vec![0u32; bodies.max(1)];
        let count = crate::collision::broad::world_candidates_near_into(&mut c.state, self.kernel, &ray, far, ignore.map_or(-1, |b| b as i64), false, false, &mut candidates);
        // rayEnabled: each candidate's `enabled` (a building's current look
        // only; every hull), as the page keeps them in the kernel's flags.
        let flags = self.world.movement.flags_mut();
        let enabled: Vec<u8> = candidates[..count].iter().map(|&k| flags[k as usize]).collect();
        let c = &mut self.world.collision;
        let (code, hit) = crate::collision::world_cast_ray_in(&mut c.state, self.kernel, &ray, far, v8math::hypot(direction.x, direction.z), &candidates[..count], &enabled);
        if code == crate::collision::MISS { return None; }
        Some(Hit { distance: hit.distance, point: Vector3::new(hit.px, hit.py, hit.pz), normal: Vector3::new(hit.nx, hit.ny, hit.nz),
            terrain: code == crate::collision::TERRAIN, body: (code >= 0).then_some(code as BodyId) })
    }
    fn push_walls(&mut self, _hull: usize, _probes: &[Vector3], _radius: f64, _q: &Quaternion, _own: Option<&Quaternion>, _previous: &Vector3, _position: &mut Vector3, _velocity: &mut Vector3, _terrain: bool) -> bool { false }
    fn ground(&mut self, _x: f64, _z: f64, _top: f64) -> Option<Ground> { None }
    fn spring_polygons<'a>(&'a mut self, _hull: usize, _index: usize, _center: &Vector3, _radius: f64, _out: &'a mut Vec<Polygon>) -> SpringPolygons<'a> { SpringPolygons::Native(&[]) }
    fn move_body(&mut self, body: BodyId, matrix: &Matrix4) {
        let Some(&(_, local)) = self.locals.iter().find(|(b, _)| *b == body) else { return };
        let Some(b) = self.world.collision.state.bodies.get_mut(body as usize) else { return };
        let mut inverse = *matrix;
        inverse.invert();
        b.bounds = box_apply_matrix4(&local, &matrix.elements);
        b.matrix = matrix.elements;
        b.inverse = inverse.elements;
    }
    // `body.enabled` is a plain field the kernel reads at each cast; every
    // body is enabled while the fleet is constructed.
    fn enable_body(&mut self, _body: BodyId, _enabled: bool) {}
    fn body_enabled(&mut self, _body: BodyId) -> bool { true }
    fn water_over(&mut self, _x: f64, _z: f64) -> bool { false }
    fn water_level(&mut self) -> f64 { 0.0 }
    fn has_water(&mut self) -> bool { false }
    fn in_death_region(&mut self, _position: &Vector3) -> bool { false }
    fn damaged(&mut self, _hull: usize, _amount: f64, _attacker: &crate::gen::OrdnanceOwner) {}
    fn destroyed_crew(&mut self, _hull: usize) {}
    fn destroyed(&mut self, _hull: usize) {}
    fn explosion(&mut self, _hull: usize, _final_explosion: bool, _position: &Vector3) {}
    fn effect(&mut self, _name: &str, _position: &Vector3) {}
    fn raycast_units(&mut self, _origin: &Vector3, _direction: &Vector3, _far: f64) -> Option<f64> { None }
    fn clock(&mut self) -> f64 { 0.0 }
    fn tow_released(&mut self, _hull: usize, _reason: &str) {}
}

/// collision.js createBodyShape through the ray kernel: the shape's corners
/// and its tree (three-mesh-bvh's build, bfe_content::bvh), and the tree's
/// polygon numbers once a body uses it (ray-kernel.js body). Answers the
/// tree's id.
fn shape_tree(world: &mut World, shape: &BodyTriangles, with_polygons: bool) -> u32 {
    let bvh = build_bvh(Positions::F32(&shape.positions), &shape.indices);
    let positions = world.collision.positions(&shape.positions);
    let tree = world.collision.tree(positions, &shape.indices, &bvh);
    if with_polygons {
        world.collision.tree_polygon_ids(tree, shape.polygons.len() as u32).copy_from_slice(&shape.polygons);
    }
    tree
}

/// vehicle-phase-data.js pullPose for a facade just constructed (and
/// spawned, or not): no age or phases yet, the visual position the spawn
/// copied, every seat without a last aim or target, every aimer at rest
/// (yaw and pitch 0, limits unknown, off target, facing +z, posed), no
/// muzzle kicked, then each rig node's rest transform.
fn pull_values(pose: &HullPose, pad: &[f64; 16], visual: [f64; 3]) -> Vec<f64> {
    // vehicles.js: `this.hoverPhase = (e[12] * .13 + e[14] * .17) % (Math.PI * 2)`.
    let hover_phase = (pad[12] * 0.13 + pad[14] * 0.17) % (std::f64::consts::PI * 2.0);
    let mut out = vec![0.0, hover_phase, 0.0, 0.0, visual[0], visual[1], visual[2]];
    for seat in &pose.seats {
        out.extend([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        for weapon in &seat.weapons {
            out.push(0.0);
            for _ in &weapon.aimers { out.extend([0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 1.0, 1.0]); }
            for _ in &weapon.muzzles { out.extend([0.0, 0.0]); }
        }
    }
    for k in 0..posing::nodes(pose) {
        let n = pose.rig.tree.node(k as three::NodeId);
        out.extend([n.position.x, n.position.y, n.position.z, n.quaternion.x, n.quaternion.y, n.quaternion.z, n.quaternion.w, n.scale.x, n.scale.y, n.scale.z]);
    }
    out
}

/// The stage: `new Vehicles` then attachF2, every call dispatched in the
/// page's order and kept in `rows`.
pub fn construct(world: &mut World, cx: &mut Cx, rows: &mut Rows) -> Result<(), AdmitError> {
    let Some(data) = cx.battle.vehicle_def.clone() else {
        // No vehicle pack: attachF3 installs the map rules (fleet::build).
        let calls = super::rows::build(Stage::Fleet, cx)?;
        sequence::replay(world, Stage::Fleet, &calls, cx)?;
        rows.calls.push((Stage::Fleet, calls));
        return Ok(());
    };
    let kernel = cx.ids.kernel.as_ref().ok_or(AdmitError::Unavailable(Stage::Kernels))?.world as usize;
    construct_prop_bodies(world, cx)?;
    let mut props = Vec::new();
    let posed = prop_poses(world, cx, &mut props);
    rows.calls.push((Stage::Fleet, props));
    posed?;
    let mut gear = Vec::new();
    let landed = gear_feet(world, cx, &mut gear);
    rows.calls.push((Stage::Fleet, gear));
    landed?;
    let mut poses = Vec::new();
    let constructed = construct_vehicles(world, cx, &data, kernel, &mut poses);
    rows.calls.push((Stage::Fleet, poses));
    let (mut made, hulls, extras, shapes) = constructed?;
    // Vehicles.attachUnits: every hull a VehicleTarget in the registry (its
    // aiTarget's id).
    for m in &mut made { m.target = world.sim.store.units.add(|id| Member::VehicleTarget(Ref::new(id, 0))); }
    attach_f2(world, cx, rows, &data, &mut made, &hulls, &extras, &shapes, kernel)?;
    Ok(())
}

/// `new Vehicles(...)`: the templates' shapes, the blockers, the pads, then
/// constructSpawns.
fn construct_vehicles(world: &mut World, cx: &mut Cx, data: &Value, kernel: usize, calls: &mut Vec<(Stage, &'static str, Call)>) -> Result<(Vec<Made>, Vec<Hull>, Vec<HullExtras>, Shapes), AdmitError> {
    let content = cx.content;
    let mut trees = Vec::with_capacity(content.vehicle_templates.len());
    for template in &content.vehicle_templates { trees.push(shape_tree(world, &template.shape, true)); }
    let blockers = content.vehicle_blockers.as_ref().map(|blockers| shape_tree(world, blockers, false));
    let spawns = data.get("spawns").and_then(Value::as_array).ok_or_else(|| error("vehicle pack has no spawns"))?;
    let descs: Vec<SlotDesc> = spawns.iter().map(SlotDesc::of).collect();
    world.sim.store.vehicles.slots = descs.iter().map(|_| SpawnSlot::default()).collect();
    let classes = data.get("classes").ok_or_else(|| error("vehicle pack has no classes"))?;
    let posts = cx.battle.world.get("commandPosts").and_then(Value::as_array).map(|p| p.to_vec()).unwrap_or_default();
    // Each body's polygon numbers start at its serial (collision.js
    // bodySerial), continuing after every body made before the fleet.
    let mut serial = world.collision.state.bodies.len() as f64;
    let mut locals: Vec<(u32, Bounds)> = Vec::new();
    let mut hulls: Vec<Hull> = Vec::new();
    let mut extras: Vec<HullExtras> = Vec::new();
    let mut made: Vec<Made> = Vec::new();
    for (pad, desc) in descs.iter().enumerate() {
        // constructSpawns: the team its post or its own record names, and
        // that team's class; a neutral pad without one manufactures nothing.
        let post = desc.post.as_ref().and_then(|post| posts.iter().find(|p| p.get("name").and_then(Value::as_str).is_some_and(|n| n.to_lowercase() == *post)));
        let team = match post {
            Some(p) => to_number(p.get("team")),
            None => match desc.team {
                Some(t) if t != 0.0 && !t.is_nan() => t,
                _ => if desc.classes.len() == 1 { string_to_number(&desc.classes[0].0) } else { 0.0 },
            },
        };
        let Some(name) = desc.class_for(team).map(str::to_string) else { continue };
        // Vehicles.variant: the pad's hull of the class, made the first time.
        let index = match made.iter().position(|m| m.pad == pad && m.name == name) {
            Some(index) => index,
            None => {
                let def = classes.get(&name).ok_or_else(|| error(format!("vehicle class {name} is missing")))?;
                let model = def.get("model").and_then(Value::as_str).ok_or_else(|| error("vehicle class has no model"))?;
                let (at, template) = content.vehicle_templates.iter().enumerate().find(|(_, t)| t.name == model).ok_or_else(|| error(format!("vehicle model {model} has no template")))?;
                // The Vehicle constructor: a hull that walks on its clips (not
                // a flyer, whose clip only moves its gear) gets its Pose over
                // the rig's bones, rooted at its dummy root or -1 (vehicles.js
                // `new Pose(this.bones.length, shell.records.poseRoot ?? -1)`).
                walking_pose(world, cx, calls, template, def)?;
                // Its moving body in the collision world, placed where its
                // matrix (identity so far) puts it.
                let body = world.collision.body_new(trees[at], 1);
                if body < 0 { return Err(error("the collision world has no room for a hull")); }
                let body = body as u32;
                serial += 1.0;
                world.collision.body_offset(body, serial * 16777216.0);
                world.movement.flags_mut()[body as usize] = 1;
                let (min, max) = local_bounds(&template.shape.positions, &template.shape.indices);
                locals.push((body, [min[0], min[1], min[2], max[0], max[1], max[2]]));
                ColdWorld { world, kernel, locals: &locals }.move_body(body, &Matrix4::default());
                let index = made.len();
                let id = world.sim.store.vehicle.insert(Vehicle::default());
                let (hull, extra) = build(&HullSource { name: &name, def, template, slot: pad, slot_matrix: &desc.matrix }, index, id, body);
                world.sim.store.vehicles.vehicles.push(id);
                hulls.push(hull);
                extras.push(extra);
                made.push(Made { index, pad, name: name.clone(), hull: id, body, body_ref: Ref::default(), spawned: false, prepared: false, target: 0, template: at, collider: None });
                index
            }
        };
        // Another live hull on the pad puts it off a second.
        let blocked = hulls.iter().enumerate().any(|(i, other)| i != index && other.alive
            && footprints_overlap(&hulls[index].center, &hulls[index].size, &desc.matrix, &other.center, &other.size, &other.matrix.elements));
        if blocked {
            world.sim.store.vehicles.slots[pad].remaining = 1.0;
            continue;
        }
        world.sim.store.vehicles.slots[pad].vehicle = Some(made[index].hull);
        let def = classes.get(&name).ok_or_else(|| error(format!("vehicle class {name} is missing")))?;
        hulls[index].spawn(&mut ColdWorld { world, kernel, locals: &locals }, team, &desc.matrix, def, &name);
        // Vehicle.spawn draws only on a page (`system.page.spawned`); the
        // world's facade keeps no drawn flight frame (the module seeds its
        // own pose at veh_pull), so the native spawn's render is undone.
        hulls[index].flight.frame_q = None;
        made[index].spawned = true;
    }
    Ok((made, hulls, extras, Shapes { trees, blockers }))
}

/// The page's imports of the fleet (KIND_VEHICLE, then KIND_VEHICLES) after
/// the pack: every hull's row as the constructor and the spawn left it,
/// each pad's variants by class. The texts they intern (names, in row
/// order) go on the next call, as the recording keeps them.
fn import_rows(world: &mut World, cx: &mut Cx, made: &mut [Made], hulls: &[Hull]) -> Result<Vec<Vec<u16>>, AdmitError> {
    let before = world.sim.strings.list.len();
    for (hull, m) in hulls.iter().zip(made.iter_mut()) {
        let mut row = hull.initial_record(&mut world.sim.strings);
        // A flyer spawned at load carries its flight (initFlight at spawn:
        // its class and speeds, then the craft's own fields), as the native
        // spawn stores it (vehicles/module/f2.rs); their texts precede the
        // name in the row's field order.
        if hull.flying && m.spawned {
            row.flight_class = Some(hull.flight.class);
            row.min_speed = Some(hull.flight.min_speed);
            row.mid_speed = Some(hull.flight.mid_speed);
            row.boost_speed = Some(hull.flight.boost_speed);
            hull.flight.to_store(&mut row, &mut world.sim.strings);
        }
        // The row's name is the facade's (`v.name`).
        row.name = world.sim.strings.intern_str(&m.name);
        // The row names its body object: the pool numbers it at this import.
        m.body_ref = Ref::new(cx.ids.body_pool, 1);
        cx.ids.body_pool += 1;
        row.body = m.body_ref;
        row.target_id = m.target as f64;
        *world.sim.store.vehicle.get_mut(m.hull).ok_or_else(|| error("a made hull left the store"))? = row;
    }
    for m in made {
        let key = world.sim.strings.intern_str(&m.name);
        let variants = &mut world.sim.store.vehicles.slots[m.pad].variants.entries;
        if !variants.iter().any(|(k, _)| *k == key) { variants.push((key, m.hull)); }
    }
    let texts = world.sim.strings.list[before..].to_vec();
    cx.refresh_strings(&world.sim.strings);
    Ok(texts)
}

/// attachF2: the pack, then F2Sim's constructor over the fleet.
#[allow(clippy::too_many_arguments)]
fn attach_f2(world: &mut World, cx: &mut Cx, rows: &mut Rows, data: &Value, made: &mut Vec<Made>, hulls: &[Hull], extras: &[HullExtras], shapes: &Shapes, kernel: usize) -> Result<(), AdmitError> {
    // The pack prefix: map rules, the pack, the flight environment and
    // tuning, then the seat owner (fleet::prefix), with the fleet imported
    // between the tuning and f2_seats_init as F2Sim's constructor does.
    let mut planned_strings = cx.strings.clone();
    let mut prefix = fleet::prefix(cx.battle, &mut planned_strings)?;
    let seats_init = prefix.pop().filter(|export| export.name == "f2_seats_init").ok_or_else(|| error("the pack prefix does not end with f2_seats_init"))?;
    let mut calls = Vec::with_capacity(prefix.len());
    for export in prefix { calls.push(export.in_context(cx, Stage::Fleet)?); }
    sequence::replay(world, Stage::Fleet, &calls, cx)?;
    rows.calls.push((Stage::Fleet, calls));
    // The import's interns are the module's own business (the replay
    // driver drops the host's import interns); the rows carry none.
    let _ = import_rows(world, cx, made, hulls)?;
    let mut calls = vec![seats_init.in_context(cx, Stage::Fleet)?];
    // adopt(v) for every hull in fleet order.
    let order = fleet::hulls(data, &cx.battle.world)?;
    let mut seen = Vec::new();
    for index in 0..made.len() {
        let hull = order.get(index).filter(|h| h.name == made[index].name && !h.prepared).ok_or_else(|| error("the constructed hulls differ from the pack's order"))?;
        adopt(world, cx, rows, data, &made[index], hull, &extras[index], &mut calls, &mut seen, None)?;
    }
    // bodiesIn: the vehicle blockers as a fixed set and the collision world,
    // the bodies' order, every body's identity, then each hull's collider.
    let blockers = match (&cx.content.vehicle_blockers, shapes.blockers) {
        (Some(shape), Some(tree)) => {
            let bounds = bfe_content::collision::triangle_bounds(&shape.positions, &shape.indices);
            let count = shape.indices.len() / 3;
            // collision.js BLOCKER_POLYGONS: the barriers' polygon numbers.
            world.collision.fixed_set(tree, &bounds, &shape.polygons[..count.min(shape.polygons.len())], 1879048192.0) as i32
        }
        _ => -1,
    };
    calls.push(fleet::collision(kernel as i32, blockers).into_row(Stage::Fleet));
    let bodies = body_refs(world, cx, made, kernel);
    calls.push(fleet::bodies(bodies).into_row(Stage::Fleet));
    for index in 0..made.len() { calls.extend(collider(world, cx, &mut made[index], &hulls[index], data)?); }
    // f2_init: the pads' records, the fleet's clock and accumulator at zero.
    let mut planned_strings = cx.strings.clone();
    for export in fleet::init(data, &mut planned_strings)? { calls.push(export.in_context(cx, Stage::Fleet)?); }
    sequence::replay(world, Stage::Fleet, &calls, cx)?;
    rows.calls.push((Stage::Fleet, std::mem::take(&mut calls)));
    attach_turrets(world, cx, rows)?;
    prepare_variants(world, cx, rows, data, made, &order, &mut seen, shapes, kernel)?;
    publish_receipts(cx, kernel, blockers, made)
}

/// The receipts the later owners read of the fleet (what the host's
/// construction handed the bound row builders): the collision world and
/// blocker set, the bodies, each hull with its collider, the vehicle and
/// body receipts by class, and each hull's crew holder.
fn publish_receipts(cx: &mut Cx, kernel: usize, blockers: i32, made: &[Made]) -> Result<(), AdmitError> {
    let mut hulls = Vec::with_capacity(made.len());
    for m in made {
        let (collider, walls, springs) = m.collider.clone().ok_or_else(|| error("a hull has no collider receipt"))?;
        hulls.push(fleet::HullBinding { hull: m.hull, spawned: m.spawned, collider, walls, springs });
        let vehicle = (m.name.clone(), m.hull.slot as f64);
        let body = (m.name.clone(), m.body as f64);
        if !cx.ids.vehicles.contains(&vehicle) { cx.ids.vehicles.push(vehicle); }
        if !cx.ids.vehicle_bodies.contains(&body) { cx.ids.vehicle_bodies.push(body); }
    }
    let bodies = cx.ids.bodies.iter().map(|&(body, kernel)| crate::gen::F2BodyRef { body, kernel }).collect();
    // collision.bodySerial after the fleet: one per body in the world's
    // list (a prepared variant gives its serial back).
    cx.ids.body_serial = cx.ids.bodies.len() as u32;
    cx.ids.fleet = Some(fleet::Bindings { world: kernel as i32, blockers, bodies, hulls });
    fleet::retain_holders(cx)
}

/// F2Sim.adopt(v) for one hull: its class once, its pulled pose, its
/// registration, its seat facts, then F2Firing.adopt.
#[allow(clippy::too_many_arguments)]
fn adopt(world: &mut World, cx: &mut Cx, rows: &mut Rows, data: &Value, m: &Made, hull: &fleet::Hull, extras: &HullExtras, calls: &mut Vec<(Stage, &'static str, Call)>, seen: &mut Vec<String>,
    import: Option<&mut dyn FnMut(&mut World, &mut Cx) -> Result<Vec<Vec<u16>>, AdmitError>>) -> Result<(), AdmitError> {
    let spawns = data.get("spawns").and_then(Value::as_array).ok_or_else(|| error("vehicle pack has no spawns"))?;
    let mut planned_strings = cx.strings.clone();
    for export in fleet::bound_class(data, hull, m.spawned, &mut planned_strings, seen)? {
        calls.push(export.in_context(cx, Stage::Fleet)?);
    }
    // A private facade is imported after its class row (writeSome in
    // adopt): the class call interns its name, the import what is left.
    let mut imported = Vec::new();
    if let Some(import) = import {
        sequence::replay(world, Stage::Fleet, calls, cx)?;
        rows.calls.push((Stage::Fleet, std::mem::take(calls)));
        imported = import(world, cx)?;
    }
    let first = calls.len();
    let row = world.sim.store.vehicle.get(m.hull).ok_or_else(|| error("a made hull left the store"))?.clone();
    let (pose, _) = crate::vehicles::module::new_pose_in(world, &m.name).ok_or_else(|| error(format!("the pack has no rig for {}", m.name)))?;
    let pad = SlotDesc::of(spawns.get(m.pad).ok_or_else(|| error("a made hull's pad is not in the pack"))?).matrix;
    let values = pull_values(&pose, &pad, if m.spawned { row.position } else { [0.0; 3] });
    calls.push((Stage::Fleet, "veh_input", Call { args: vec![values.len() as f64], ..Default::default() }));
    // A hull spawned at load has its pose seeded natively (seed 1).
    calls.push((Stage::Fleet, "veh_pull", Call { args: vec![m.hull.slot as f64, row.target_generation, values.len() as f64, f64::from(m.spawned)],
        scratch: values.iter().flat_map(|v| v.to_le_bytes()).collect(), ..Default::default() }));
    calls.push(fleet::register(m.hull, m.prepared).into_row(Stage::Fleet));
    // A private facade's import interned its texts just before these calls;
    // the driver drops the host's import interns, so none ride on a row.
    let _ = (first, imported);
    let class = cx.defs.vehicles.iter().find(|c| cx.defs.names.text(c.name) == m.name).ok_or_else(|| error(format!("{} is not a vehicle class", m.name)))?;
    let facts = bodies::hull_facts(m.hull, m.index, m.prepared, class);
    let mut seats_adopt = units::call(SeatHullFacts::WORDS, |row, heap| facts.put(row, heap));
    seats_adopt.args = vec![f64::from(m.prepared)];
    calls.push((Stage::Fleet, "f2_seats_adopt", seats_adopt));
    // The pulled pose interns the rig's names natively; the firing rows
    // below are planned against the table after it.
    sequence::replay(world, Stage::Fleet, calls, cx)?;
    rows.calls.push((Stage::Fleet, std::mem::take(calls)));
    // F2Firing.adopt: each seat weapon registered with combat and the
    // ordnance, then the hull's birth row.
    let class_def = data.get("classes").and_then(|c| c.get(&m.name)).ok_or_else(|| error(format!("vehicle class {} is missing", m.name)))?.clone();
    let birth = firing_birth(cx, calls, m, &row, &class_def, extras, &pose)?;
    calls.push((Stage::Fleet, "f2_firing_adopt", birth));
    sequence::replay(world, Stage::Fleet, calls, cx)?;
    rows.calls.push((Stage::Fleet, std::mem::take(calls)));
    Ok(())
}

/// bodiesIn's body list: every body of the collision world in its order
/// (rayBodies), each with its identity in the host's pool: the fleet's at
/// their import, every other body numbered here on first sight.
fn body_refs(world: &mut World, cx: &mut Cx, made: &[Made], kernel: usize) -> Vec<crate::gen::F2BodyRef> {
    let count = world.collision.state.bodies.len();
    let live: Vec<u32> = (0..count as u32).filter(|k| !made.iter().any(|m| m.prepared && m.body == *k)).collect();
    for (i, slot) in world.collision.world_order_slots(kernel as u32, live.len() as u32).iter_mut().enumerate() { *slot = live[i]; }
    let mut refs = Vec::with_capacity(live.len());
    for k in live {
        let body = match made.iter().find(|m| m.body == k) {
            Some(m) => m.body_ref,
            None => match cx.ids.bodies.iter().find(|(_, kernel)| *kernel == k) {
                Some(&(body, _)) => body,
                None => { let body = Ref::new(cx.ids.body_pool, 1); cx.ids.body_pool += 1; body }
            },
        };
        refs.push(crate::gen::F2BodyRef { body, kernel: k });
    }
    cx.ids.bodies = refs.iter().map(|r| (r.body, r.kernel)).collect();
    refs
}

/// VehicleContent.collider(v): a gather cache per wall probe and per
/// spring (collision.js gatherCache, WALL_CACHE_MARGIN), then the header
/// with the body, its kernel slot and its local bounds.
fn collider(world: &mut World, cx: &mut Cx, m: &mut Made, hull: &Hull, data: &Value) -> Result<[(Stage, &'static str, Call); 2], AdmitError> {
    let def = data.get("classes").and_then(|c| c.get(&m.name)).ok_or_else(|| error(format!("vehicle class {} is missing", m.name)))?;
    let springs = def.get("springs").filter(|v| v.truthy()).and_then(Value::as_array).map_or(0, |s| s.len());
    let walls: Vec<i32> = (0..hull.probes.len()).map(|_| world.collision.cache_new(1.5) as i32).collect();
    let springs: Vec<i32> = (0..springs).map(|_| world.collision.cache_new(1.5) as i32).collect();
    let template = &cx.content.vehicle_templates[m.template];
    let (min, max) = local_bounds(&template.shape.positions, &template.shape.indices);
    let header = crate::gen::F2ColliderHeader { body: m.body_ref.slot, kernel: m.body, min, max };
    m.collider = Some((header.clone(), walls.clone(), springs.clone()));
    // The loan sized first (veh_input), then the header and caches in it.
    let words = crate::gen::F2ColliderHeader::WORDS + walls.len() + springs.len();
    Ok([(Stage::Fleet, "veh_input", Call { args: vec![words as f64], ..Default::default() }), fleet::collider(header, &walls, &springs)?.into_row(Stage::Fleet)])
}

/// F2Sim.prepareVariants at attachF3's entry: the map rules again, then
/// every class a pad may still bring, constructed as a private facade (its
/// body made then taken out of the world's list, the serial given back),
/// adopted, given its collider and prepared in the collision world.
#[allow(clippy::too_many_arguments)]
fn prepare_variants(world: &mut World, cx: &mut Cx, rows: &mut Rows, data: &Value, made: &mut Vec<Made>, order: &[fleet::Hull], seen: &mut Vec<String>, shapes: &Shapes, kernel: usize) -> Result<(), AdmitError> {
    let mut calls = Vec::new();
    let mut planned_strings = cx.strings.clone();
    calls.push(fleet::map_call(&cx.battle.world, cx.content.collision.kill_y, &mut planned_strings)?.in_context(cx, Stage::Fleet)?);
    sequence::replay(world, Stage::Fleet, &calls, cx)?;
    rows.calls.push((Stage::Fleet, std::mem::take(&mut calls)));
    let spawns = data.get("spawns").and_then(Value::as_array).ok_or_else(|| error("vehicle pack has no spawns"))?;
    let classes = data.get("classes").ok_or_else(|| error("vehicle pack has no classes"))?;
    let content = cx.content;
    for hull in order.iter().filter(|h| h.prepared) {
        let desc = SlotDesc::of(spawns.get(hull.pad).ok_or_else(|| error("a prepared hull's pad is not in the pack"))?);
        let def = classes.get(&hull.name).ok_or_else(|| error(format!("vehicle class {} is missing", hull.name)))?;
        let model = def.get("model").and_then(Value::as_str).ok_or_else(|| error("vehicle class has no model"))?;
        let (at, template) = content.vehicle_templates.iter().enumerate().find(|(_, t)| t.name == model).ok_or_else(|| error(format!("vehicle model {model} has no template")))?;
        // new Vehicle: its body, offset by the next serial, which the page
        // gives back once the body leaves the world's list.
        let serial = world.collision.state.bodies.len() as f64 - made.iter().filter(|m| m.prepared).count() as f64;
        let mut posed = Vec::new();
        walking_pose(world, cx, &mut posed, template, def)?;
        rows.calls.push((Stage::Fleet, posed));
        let body = world.collision.body_new(shapes.trees[at], 1);
        if body < 0 { return Err(error("the collision world has no room for a hull")); }
        let body = body as u32;
        world.collision.body_offset(body, (serial + 1.0) * 16777216.0);
        world.movement.flags_mut()[body as usize] = 1;
        let (min, max) = local_bounds(&template.shape.positions, &template.shape.indices);
        let locals = [(body, [min[0], min[1], min[2], max[0], max[1], max[2]])];
        ColdWorld { world, kernel, locals: &locals }.move_body(body, &Matrix4::default());
        let index = made.len();
        let id = world.sim.store.vehicle.insert(Vehicle::default());
        let (built, extra) = build(&HullSource { name: &hull.name, def, template, slot: hull.pad, slot_matrix: &desc.matrix }, index, id, body);
        let mut m = Made { index, pad: hull.pad, name: hull.name.clone(), hull: id, body, body_ref: Ref::default(), spawned: false, prepared: true, target: 0, template: at, collider: None };
        // The row names its body object: the pool numbers it at its import.
        m.body_ref = Ref::new(cx.ids.body_pool, 1);
        cx.ids.body_pool += 1;
        made.push(m);
        // rayBodies without it; the slot keeps its variant.
        let _ = body_refs(world, cx, made, kernel);
        let (pad, name, body_ref) = (hull.pad, hull.name.clone(), made[index].body_ref);
        let mut import = |world: &mut World, cx: &mut Cx| -> Result<Vec<Vec<u16>>, AdmitError> {
            let before = world.sim.strings.list.len();
            let key = world.sim.strings.intern_str(&name);
            let mut row = built.initial_record(&mut world.sim.strings);
            row.name = key;
            row.body = body_ref;
            *world.sim.store.vehicle.get_mut(id).ok_or_else(|| error("a made hull left the store"))? = row;
            world.sim.store.vehicles.slots[pad].variants.entries.push((key, id));
            let texts = world.sim.strings.list[before..].to_vec();
            cx.refresh_strings(&world.sim.strings);
            Ok(texts)
        };
        adopt(world, cx, rows, data, &made[index], hull, &extra, &mut calls, seen, Some(&mut import))?;
        calls.extend(collider(world, cx, &mut made[index], &built, data)?);
        let m = &made[index];
        calls.push(fleet::prepare_body(m.body).into_row(Stage::Fleet));
        sequence::replay(world, Stage::Fleet, &calls, cx)?;
        rows.calls.push((Stage::Fleet, std::mem::take(&mut calls)));
    }
    Ok(())
}

/// weapons.js Weapon constructor (`new Weapon(def)`, `player` true) as a
/// store row at birth: level zero, its ammunition and spread floor from the
/// class, every clock at rest.
fn weapon_state(def: &Value, weapon_id: crate::store::DefId) -> Weapon {
    let rounds = to_number(def.get("RoundsPerClip"));
    let bottomless = !(rounds >= 1.0);
    let ammo = to_number(def.get("ammo"));
    let floor = -or_number(to_number(def.get("SpreadThreshold")), 0.0);
    Weapon { def: weapon_id, level: 0.0, bottomless, per_shot: if bottomless { 0.0 } else { 1.0 / (rounds + 0.0001) }, clip: 1.0,
        reserve: if ammo > 0.0 { ammo - 1.0 } else { f64::INFINITY }, floor, kick: floor, spread_scale: 1.0, throw_time: -1.0, charge: 1.0, limit: 1.0,
        lock_target_id: -1.0, lock_generation: -1.0, ..Default::default() }
}

/// `num` of f2-firing.js: parseFloat, or 0 where that is not finite.
fn finite_or_zero(value: Option<&Value>) -> f64 {
    let number = parse_float_value(value);
    if number.is_finite() { number } else { 0.0 }
}

/// JavaScript `a || b` over a number: `a` unless it is 0, -0 or NaN.
fn or_number(value: f64, fallback: f64) -> f64 {
    if value == 0.0 || value.is_nan() { fallback } else { value }
}

/// A string option that is truthy in JavaScript (not absent, not empty).
fn truthy_text(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// The next beam emitter identity, as the host's mirror pool numbers each
/// emitter object the first time a row carries it.
fn emitter(cx: &mut Cx) -> BeamSource {
    let slot = cx.ids.emitters;
    cx.ids.emitters += 1;
    BeamSource::Emitter(Ref::new(slot, 1))
}

/// F2Firing.adopt (src/sim/wasm/f2-firing.js): registers every seat
/// weapon's level-zero class and its ordnance (combat::prepare_vehicle_weapon
/// appends those calls), then builds the FfBirth row: `new Weapon(def)`
/// (src/weapons.js) at birth, the weapon's lock range, cable, channel and
/// AI masks (weaponAI), its rig nodes by name (rigNodes order: the root,
/// the bones, the statics, the markers), its beam emitter and its muzzles'.
fn firing_birth(cx: &mut Cx, calls: &mut Vec<(Stage, &'static str, Call)>, m: &Made, row: &Vehicle, class_def: &Value, extras: &HullExtras, pose: &HullPose) -> Result<Call, AdmitError> {
    let by_name = &pose.rig.by_name;
    // `node(named(name))`: the bone's place in rigNodes, after the root.
    let node_of = |name: Option<&str>| by_name.named(name).map_or(-1.0, |bone| (bone + 1) as f64);
    let seat_values = class_def.get("seats").and_then(Value::as_array).map(|s| s.to_vec()).unwrap_or_default();
    let mut seats = Vec::with_capacity(seat_values.len());
    for (i, seat) in seat_values.iter().enumerate() {
        let settings = seat.get("settings");
        let seat_rig = pose.rig.seats.get(i).ok_or_else(|| error("the rig has fewer seats than the class"))?;
        // The seat's own emitter (`s.emitter`) is one object: the pool
        // numbers it the first time a weapon without a fire node names it.
        let mut seat_emitter: Option<BeamSource> = None;
        let mut weapons = Vec::new();
        for (j, weapon) in seat.get("weapons").and_then(Value::as_array).map(|w| w.to_vec()).unwrap_or_default().iter().enumerate() {
            let authored = weapon.get("def").ok_or_else(|| error("a seat weapon has no definition"))?;
            let ((def, weapon_id, ordnance_id), registration) = super::rows::combat::prepare_vehicle_weapon(cx, authored, Stage::Fleet)?;
            calls.extend(registration);
            let rig = seat_rig.weapons.get(j).ok_or_else(|| error("the rig has fewer weapons than the seat"))?;
            let aimers = weapon.get("aimers").and_then(Value::as_array).map(|a| a.to_vec()).unwrap_or_default();
            let state = weapon_state(&def, weapon_id);
            // weaponAI(def): the class's engagement masks.
            let mut strings = crate::store::Strings::new();
            let words = super::rows::combat::value_words(Some(&def), &mut strings, None);
            let crate::combat::defs::Js::Obj(obj) = crate::combat::module::read_js(&words, &mut 0, &strings) else { return Err(error("a seat weapon is not an object")) };
            let ai = crate::ai::combat::weapons_ai::WeaponAiTable::default().weapon_ai(&obj);
            let ordnance = def.get("ordnance").filter(|v| v.truthy());
            let first_aimer = aimers.first();
            let fallback = truthy_text(first_aimer.and_then(|a| a.get("FirePointName")))
                .or_else(|| truthy_text(first_aimer.and_then(|a| a.get("node"))))
                .or_else(|| truthy_text(def.get("FirePointName")));
            // The weapon's own emitter is its fire node's; a weapon without one
            // fires from the seat's.
            let (beam, emitter_node) = match rig.fire {
                Some(node) => (emitter(cx), node as f64),
                None => (*seat_emitter.get_or_insert_with(|| emitter(cx)), -1.0),
            };
            let muzzles = rig.muzzles.iter().map(|muzzle| FfMuzzle {
                emitter: emitter(cx),
                node: muzzle.fire_node as f64,
                aimer: muzzle.source_aimer.map_or(-1.0, |a| a as f64),
            }).collect();
            weapons.push(FfWeaponBirth {
                state,
                ordnance: ordnance_id,
                tow: ordnance.and_then(|o| o.get("base")).and_then(Value::as_str) == Some("towcable"),
                range: v8math::max(20.0, or_number(to_number(def.get("LockOnRange")), 0.0)),
                cable_length: v8math::max(10.0, or_number(to_number(ordnance.and_then(|o| o.get("CableLength"))), 60.0)),
                channel: match def.get("channel") { Some(Value::Null) | None => j as f64, Some(value) => to_number(Some(value)) },
                relation_mask: ai.relation_mask,
                category_mask: ai.category_mask,
                weapon_node: node_of(def.get("FirePointName").and_then(Value::as_str)),
                fallback_node: node_of(fallback),
                direction_nodes: aimers.iter().map(|a| node_of(a.get("FirePointName").and_then(Value::as_str))).collect(),
                outside: aimers.iter().map(|a| finite_or_zero(a.get("FireOutsideLimits").filter(|v| !v.is_null()).or_else(|| settings.and_then(|s| s.get("FireOutsideLimits"))))).collect(),
                emitter: beam,
                emitter_node,
                emitter_aimer: -1.0,
                muzzles,
            });
        }
        seats.push(FfSeatBirth { weapons });
    }
    let mut texts = Vec::new();
    let legs = extras.legs.iter().map(|leg| {
        let before = cx.strings.list.len();
        let name = cx.strings.intern_str(&leg.name);
        if cx.strings.list.len() > before { texts.push(leg.name.encode_utf16().collect()); }
        let vec3 = |v: &Option<Vector3>| v.as_ref().map(|v| [v.x, v.y, v.z]);
        FfLeg { name, thigh: vec3(&leg.thigh), calf: vec3(&leg.calf), foot: vec3(&leg.foot) }
    }).collect();
    let birth = FfBirth { hull: m.hull, life: row.target_generation, command_walker: base_of(class_def) == "commandwalker", leg_top: extras.leg_top, legs, seats };
    let mut call = units::call(FfBirth::WORDS, |row, heap| birth.put(row, heap));
    call.strings = texts;
    Ok(call)
}

/// LiveObjects' constructor (src/props.js): every placed building's
/// collision bodies, in instance order and look order (built, destroyed,
/// unbuilt), through CollisionWorld.addBody: the props' shared corners
/// once, a tree and a fixed body per look with its polygon numbers and
/// serial, placed at its tree's bounds. Only the building's first look
/// (unbuilt where it has one, else built) is enabled (Building.show).
fn construct_prop_bodies(world: &mut World, cx: &mut Cx) -> Result<(), AdmitError> {
    let content = cx.content;
    let instances = cx.battle.props.as_ref().and_then(|p| p.get("instances")).and_then(Value::as_array).map(|i| i.to_vec()).unwrap_or_default();
    let mut positions: Vec<(*const f32, usize)> = Vec::new();
    let mut serial = world.collision.state.bodies.len() as f64;
    for prop in &content.prop_bodies {
        let desc = instances.get(prop.instance).ok_or_else(|| error("a prop body names no instance"))?;
        let first = if desc.get("unbuilt").is_some_and(|u| u.truthy()) { "unbuilt" } else { "built" };
        for (look, (name, shape)) in [("built", &prop.built), ("destroyed", &prop.destroyed), ("unbuilt", &prop.unbuilt)].into_iter().enumerate() {
            let Some(shape) = shape else { continue };
            // buildBVH over the shared props corners: copied into the kernel
            // the first time (ray-kernel.js positions caches by array).
            let key = std::rc::Rc::as_ptr(&shape.positions) as *const f32;
            let id = match positions.iter().find(|(k, _)| *k == key) {
                Some(&(_, id)) => id,
                None => { let id = world.collision.positions(&shape.positions); positions.push((key, id)); id }
            };
            let bvh = build_bvh(Positions::F32(&shape.positions), &shape.indices);
            let tree = world.collision.tree(id, &shape.indices, &bvh);
            world.collision.tree_polygon_ids(tree, shape.polygons.len() as u32).copy_from_slice(&shape.polygons);
            let body = world.collision.body_new(tree, 0);
            if body < 0 { return Err(error("the collision world has no room for a prop")); }
            serial += 1.0;
            world.collision.body_offset(body as u32, serial * 16777216.0);
            // rays.place(slot, bvh.getBoundingBox(), null, null).
            let (min, max) = local_bounds(&shape.positions, &shape.indices);
            let io = world.collision.io_mut();
            io[20..23].copy_from_slice(&min);
            io[23..26].copy_from_slice(&max);
            world.collision.body_set(body as u32);
            world.movement.flags_mut()[body as usize] = u8::from(name == first);
            cx.ids.prop_kernels.push((prop.instance, look as u8, body as u32));
        }
    }
    Ok(())
}

/// TurretSeats (src/seats.js) after attachF2: every armed building a
/// TurretTarget in the registry (attachUnits), then F2Seats.attachTurrets
/// (src/sim/wasm/f2-seats.js): its weapon class registered with its
/// ordnance, and its birth row from the placed instance (bodies/seats.rs
/// turret_geometry and turret_birth): the shown look's sphere, its built
/// body, its weapon at birth.
fn attach_turrets(world: &mut World, cx: &mut Cx, rows: &mut Rows) -> Result<(), AdmitError> {
    let Some(props) = cx.battle.props.clone() else { return Ok(()) };
    let models = props.get("models").ok_or_else(|| error("props have no models"))?;
    let instances = props.get("instances").and_then(Value::as_array).map(|i| i.to_vec()).unwrap_or_default();
    let turrets: Vec<(usize, Value)> = instances.iter().enumerate()
        .filter(|(_, desc)| desc.get("base").and_then(Value::as_str) == Some("armedbuilding")
            && desc.get("model").and_then(Value::as_str).and_then(|m| models.get(m)).is_some_and(Value::truthy))
        .map(|(i, desc)| (i, desc.clone())).collect();
    for _ in &turrets { world.sim.store.units.add(|id| Member::TurretTarget(Ref::new(id, 0))); }
    // registerObjectives (src/objectives.js, game.js after the turrets): every
    // destructible command post's building an ObjectiveTarget.
    let posts = instances.iter().filter(|desc| desc.get("base").and_then(Value::as_str) == Some("commandpost")
        && desc.get("model").and_then(Value::as_str).and_then(|m| models.get(m)).is_some_and(Value::truthy)).count();
    for _ in 0..posts { world.sim.store.units.add(|id| Member::ObjectiveTarget(Ref::new(id, 0))); }
    let mut calls = Vec::new();
    for (index, (instance, desc)) in turrets.iter().enumerate() {
        let model_name = desc.get("model").and_then(Value::as_str).unwrap_or("");
        let model = models.get(model_name).ok_or_else(|| error(format!("turret model {model_name} is missing")))?;
        let unbuilt = desc.get("unbuilt").filter(|u| u.truthy());
        // Building.show: the unbuilt look first where there is one.
        let built = unbuilt.is_none();
        let (weapon, ordnance, definition) = match desc.get("weapon").filter(|w| w.truthy()) {
            Some(def) => {
                let ((def, weapon_id, ordnance_id), registration) = super::rows::combat::prepare_vehicle_weapon(cx, def, Stage::Fleet)?;
                calls.extend(registration);
                (Some(weapon_state(&def, weapon_id)), ordnance_id, Some(def))
            }
            None => (None, None, None),
        };
        // Turret.sphere: the shown look's model box's centre and half
        // diagonal (an unbuilt look's authored sphere), placed.
        let shown = if built { Some(model) } else { unbuilt.and_then(|u| u.get("model")).and_then(Value::as_str).and_then(|m| models.get(m)) };
        let mut center = Vector3::new(0.0, 1.0, 0.0);
        let mut radius = 1.0;
        let sphere = shown.and_then(|m| m.get("sphere")).filter(|_| !built).and_then(Value::as_array);
        if let Some(sphere) = sphere {
            let n = |i: usize| sphere.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN);
            center.set(n(0), n(1), n(2));
            radius = n(3);
        } else if let Some(boxed) = shown.and_then(|m| m.get("box")).and_then(Value::as_array) {
            let corner = |c: usize, i: usize| boxed.get(c).and_then(|v| v.at(i)).and_then(Value::as_f64).unwrap_or(f64::NAN);
            center.set((corner(0, 0) + corner(1, 0)) / 2.0, (corner(0, 1) + corner(1, 1)) / 2.0, (corner(0, 2) + corner(1, 2)) / 2.0);
            radius = v8math::hypot3(corner(1, 0) - corner(0, 0), corner(1, 1) - corner(0, 1), corner(1, 2) - corner(0, 2)) / 2.0;
        }
        let placement = desc.get("matrix").and_then(Value::as_array).map(|m| std::array::from_fn(|i| m.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN))).unwrap_or([f64::NAN; 16]);
        center.apply_matrix4(&Matrix4 { elements: placement });
        let body = cx.ids.prop_kernels.iter().find(|(i, look, _)| *i == *instance && *look == 0).map(|(_, _, kernel)| *kernel)
            .and_then(|kernel| cx.ids.bodies.iter().find(|(_, k)| *k == kernel).map(|(body, _)| *body));
        let geometry = bodies::turret_geometry(desc, model, index)?;
        // F2Riders.ensureAll visits the stationary turrets after the fleet:
        // its holder, with the turret store's generation-one ref (kept
        // behind the hulls' holders by retain_holders).
        cx.holders.push(bodies::Holder::Turret { turret: Ref::new(index as u32, 1), rider: geometry.rider.clone() });
        // Its registry target is on its first life (TurretTarget, generation 1).
        let binding = bodies::TurretBinding { turret: Ref::new(index as u32, 1), life: 1.0, alive: built, body, center: [center.x, center.y, center.z], radius, ordnance, collide: true };
        let birth = bodies::turret_birth(geometry, binding, weapon, definition.as_ref())?;
        calls.push((Stage::Fleet, "f2_turret_adopt", units::call(crate::gen::F2TurretBirth::WORDS, |row, heap| birth.put(row, heap))));
        sequence::replay(world, Stage::Fleet, &calls, cx)?;
        rows.calls.push((Stage::Fleet, std::mem::take(&mut calls)));
    }
    Ok(())
}

/// One sampling call against the module, kept in `calls` as the page made it.
fn sample(world: &mut World, cx: &mut Cx, calls: &mut Vec<(Stage, &'static str, Call)>, name: &'static str, call: Call) -> Result<super::dispatch::Returned, AdmitError> {
    sequence::args(world, &call)?;
    let answer = super::dispatch::call(world, Stage::Fleet, name, &call)?;
    super::dispatch::record(cx, Stage::Fleet, name, answer.clone())?;
    calls.push((Stage::Fleet, name, call));
    Ok(answer)
}

/// `new Vehicles`' template loop: a model with a takeoff clip finds how far
/// below its origin it stands on its gear (src/flight/gear.js gearFoot) by
/// sampling that clip through the anim kernel: `new Pose(bones, -1)`, the
/// clip registered on its first add (each model's clips are its own
/// objects), `add(clip, 0, 1, false)`, then `apply`. The foot itself is the
/// content's (bfe_content::gear); these calls are what the module saw.
fn gear_feet(world: &mut World, cx: &mut Cx, calls: &mut Vec<(Stage, &'static str, Call)>) -> Result<(), AdmitError> {
    let content = cx.content;
    for template in &content.vehicle_templates {
        let Some(takeoff) = template.clips.as_ref().and_then(|clips| clips.get("takeoff")) else { continue };
        let bones = bfe_content::derived::rig_bones(&template.rig).len();
        // gearFoot's guard: `!clip?.frames || !rig?.bones?.length`.
        if bones == 0 || !takeoff.frames.truthy() { continue; }
        let super::dispatch::Returned::Scalar(pose) = sample(world, cx, calls, "pose_new", Call { args: vec![bones as f64, -1.0], ..Default::default() })? else {
            return Err(error("pose_new refused a gear pose"));
        };
        let clip = super::spawn::register_clip(world, cx, takeoff, &format!("vehicle-template:{}", template.name), calls)?;
        sample(world, cx, calls, "pose_add", Call { args: vec![pose, f64::from(clip), 0.0, 1.0, 0.0], ..Default::default() })?;
        sample(world, cx, calls, "pose_out", Call { args: vec![pose], ..Default::default() })?;
    }
    Ok(())
}

/// The Poses LiveObjects' constructors make, in instance order (props.js
/// ObjectivePost and AnimatedProp, traps.js Trap): `new Pose(bones,
/// max(0, dummy root))` when the model has bones and the clip the kind
/// plays: an ObjectivePost's idle or death clip, any clip of an
/// AnimatedProp (its receipt is kept for the F4 birth, rows/battle/
/// animated.rs), a trap's trigger or build clip.
fn prop_poses(world: &mut World, cx: &mut Cx, calls: &mut Vec<(Stage, &'static str, Call)>) -> Result<(), AdmitError> {
    let Some(props) = cx.battle.props.clone() else { return Ok(()) };
    let models = props.get("models");
    let instances = props.get("instances").and_then(Value::as_array).map(|i| i.to_vec()).unwrap_or_default();
    for (instance, desc) in instances.iter().enumerate() {
        let Some(name) = desc.get("model").and_then(Value::as_str) else { continue };
        let Some(model) = models.and_then(|m| m.get(name)).filter(|m| m.truthy()) else { continue };
        let bones = model.get("bones").and_then(Value::as_array).map(|b| b.to_vec()).unwrap_or_default();
        if bones.is_empty() { continue; }
        let clips = cx.content.prop_clips.iter().find(|(m, _)| m == name).and_then(|(_, clips)| clips.as_ref());
        let has = |key: Option<&Value>| key.and_then(Value::as_str).filter(|k| !k.is_empty()).is_some_and(|k| clips.is_some_and(|c| c.get(k).is_some()));
        let posed = match desc.get("base").and_then(Value::as_str).unwrap_or("") {
            "commandpost" => { let anim = desc.get("animation"); has(anim.and_then(|a| a.get("idle"))) || has(anim.and_then(|a| a.get("death"))) }
            "animatedprop" | "door" => {
                if clips.is_some_and(|c| !c.list.is_empty()) {
                    let (_, made) = super::rows::battle::animated::pose(world, cx, instance)?;
                    calls.extend(made);
                }
                continue;
            }
            "trap" => { let t = desc.get("trap"); has(t.and_then(|t| t.get("trigger"))) || has(t.and_then(|t| t.get("build"))) }
            _ => false,
        };
        if !posed { continue; }
        let root = bones.iter().position(|b| {
            let n = b.get("name").and_then(Value::as_str).unwrap_or("").to_lowercase();
            n == "dummyroot" || n == "dummy_root"
        }).unwrap_or(0);
        sample(world, cx, calls, "pose_new", Call { args: vec![bones.len() as f64, root as f64], ..Default::default() })?;
    }
    Ok(())
}

/// The Vehicle constructor's Pose: a hull that walks on its clips (not a
/// flyer, whose clip only moves its gear) gets one over the rig's bones,
/// rooted at its dummy root or -1 (vehicles.js `new Pose(this.bones.length,
/// shell.records.poseRoot ?? -1)`).
fn walking_pose(world: &mut World, cx: &mut Cx, calls: &mut Vec<(Stage, &'static str, Call)>, template: &bfe_content::content::VehicleTemplate, def: &Value) -> Result<(), AdmitError> {
    if !template.clips.as_ref().is_some_and(|clips| !clips.list.is_empty()) || base_of(def) == "flyer" { return Ok(()); }
    let bones = bfe_content::derived::rig_bones(&template.rig);
    let root = bones.iter().position(|b| b.name.to_lowercase() == "dummyroot").map_or(-1.0, |i| i as f64);
    sample(world, cx, calls, "pose_new", Call { args: vec![bones.len() as f64, root], ..Default::default() })?;
    Ok(())
}
