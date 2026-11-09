//! The props' and the hulls' identities attachF3 binds (F3Props.prepare over
//! LiveObjects.buildings, F3Hulls.prepare over the fleet), derived from the
//! world the fleet stage built and the content: the registry members the
//! sequence added, the kernel bodies the prop and hull constructors made,
//! and the authored descriptions. Nothing is predicted: a missing receipt
//! refuses, naming it.
use super::super::super::{AdmitError, Stage};
use super::super::{fleet, props, units, Cx};
use crate::gen::{F3BodyName, F3HullSetup, Member, RepairTarget};
use crate::store::Ref;
use crate::world::owner::World;
use bfe_content::{js::to_number, value::Value};

fn error(message: impl Into<String>) -> AdmitError { AdmitError::Content { stage: Stage::Units, message: message.into() } }

/// The store body ref of a kernel body, from the fleet stage's body order.
fn body_of(cx: &Cx, kernel: u32) -> Option<Ref<crate::gen::Body>> {
    cx.ids.bodies.iter().find(|(_, k)| *k == kernel).map(|(body, _)| *body)
}

/// LiveObjects.buildings in order, as F3Props.prepare reads them: each
/// building's host object becomes a Building or Turret ref numbered in
/// first-use order per kind (the bridge's ref tables), its registry target
/// is the TurretTarget TurretSeats.attachUnits added or the ObjectiveTarget
/// registerObjectives added after the turrets (both in the fleet stage), its
/// bodies are the three looks' kernel bodies, and a turret is the only
/// building with a damage hook and a crew (TurretSeats) at construction.
pub fn buildings(world: &mut World, cx: &Cx) -> Result<Vec<props::BuildingBinding>, AdmitError> {
    let Some(data) = cx.battle.props.clone() else { return Ok(Vec::new()) };
    let instances = data.get("instances").and_then(Value::as_array).map(|i| i.to_vec()).unwrap_or_default();
    let sources = props::live_sources(cx.battle);
    let turret_ids: Vec<u32> = world.sim.store.units.list.iter()
        .filter_map(|m| match m { Member::TurretTarget(r) => Some(r.slot), _ => None }).collect();
    let objective_ids: Vec<u32> = world.sim.store.units.list.iter()
        .filter_map(|m| match m { Member::ObjectiveTarget(r) => Some(r.slot), _ => None }).collect();
    let (mut buildings, mut turrets, mut objectives) = (0u32, 0u32, 0usize);
    let mut out = Vec::with_capacity(sources.len());
    for (index, source) in sources.iter().enumerate() {
        let desc = instances.get(*source).ok_or_else(|| error("a live building names no instance"))?;
        let base = desc.get("base").and_then(Value::as_str).unwrap_or("");
        let mut bodies = [None; 3];
        for (look, slot) in bodies.iter_mut().enumerate() {
            *slot = cx.ids.prop_kernels.iter().find(|(i, l, _)| *i == *source && *l as usize == look)
                .and_then(|(_, _, kernel)| body_of(cx, *kernel));
        }
        // Building.team: `desc.team || 0`.
        let team = desc.get("team").filter(|v| v.truthy()).map(|v| to_number(Some(v))).unwrap_or(0.0);
        let mut binding = props::Binding { index: index as u32, target: RepairTarget::Building(Ref::new(buildings, 1)), member: None, bodies,
            generation: None, team, target_team: team, category: 0, kind: f64::NAN, damaged: false, crew: false, trap: None };
        match base {
            "armedbuilding" => {
                let id = *turret_ids.get(turrets as usize).ok_or_else(|| error("a turret has no registry target"))?;
                binding.target = RepairTarget::Turret(Ref::new(turrets, 1));
                binding.member = Some(Member::TurretTarget(Ref::new(id, 0)));
                // TurretTarget: first life, a building target (category 4,
                // KIND.armedBuilding), its team the turret's own.
                binding.generation = Some(1.0); binding.category = 4; binding.kind = 8.0;
                binding.damaged = true; binding.crew = true;
                turrets += 1;
            }
            "commandpost" => {
                // registerObjectives (game.js 1295) added its ObjectiveTarget
                // after the turrets; the fleet stage keeps that order.
                let id = *objective_ids.get(objectives).ok_or_else(|| error("a destructible post has no registry target"))?;
                objectives += 1;
                binding.member = Some(Member::ObjectiveTarget(Ref::new(id, 0)));
                binding.generation = Some(1.0); binding.category = 4; binding.kind = 9.0;
                buildings += 1;
            }
            "trap" => {
                binding.trap = Some(trap(world, cx, desc, &data)?);
                // LiveObjects.attachTraps (game.js teamOfSide): a trap placed
                // with team 0 belongs to its ResetTeam side's team, the first
                // mission team whose first class carries that side's prefix.
                let reset = desc.get("trap").and_then(|t| t.get("resetTeam")).and_then(Value::as_str).filter(|s| !s.is_empty());
                if let (0.0, Some(side)) = (binding.team, reset) {
                    let teams = cx.battle.world.get("mission").and_then(|m| m.get("teams")).and_then(Value::as_object);
                    let team = teams.and_then(|teams| teams.entries().find(|(_, desc)| {
                        let first = desc.get("classes").and_then(|c| c.at(0));
                        let name = first.and_then(|f| if let Value::Array(_) = f { f.at(0) } else { Some(f) }).and_then(Value::as_str);
                        name.is_some_and(|n| n.contains('_') && n.split('_').next().is_some_and(|p| p.to_lowercase() == side))
                    }).map(|(key, _)| to_number(Some(&Value::str(key))))).filter(|t| !t.is_nan()).unwrap_or(0.0);
                    binding.team = team;
                    binding.target_team = team;
                }
                buildings += 1;
            }
            _ => { buildings += 1; }
        }
        out.push(props::BuildingBinding { source: *source, binding });
    }
    Ok(out)
}

fn vec3(value: Option<&Value>) -> [f64; 3] {
    let a = value.and_then(Value::as_array).unwrap_or(&[]);
    std::array::from_fn(|i| a.get(i).map_or(0.0, |v| to_number(Some(v))))
}

/// traps.js Trap's placement facts: the trigger point (TriggerOffset at the
/// trigger node at bind, else in the trap's frame), the death effect's
/// point, the build and trigger clips' lengths, and the pad's sphere where
/// repair weapons, the AI and blasts find the trap (placePad over the pad
/// LiveObjects grounded with padOnGround: a cast 20 m down from above the
/// pad's spot, against the collision world as it stands).
fn trap(world: &mut World, cx: &Cx, desc: &Value, data: &Value) -> Result<props::Trap, AdmitError> {
    use three::{Matrix4, Vector3};
    let t = desc.get("trap").cloned().unwrap_or(Value::Null);
    let placement: [f64; 16] = desc.get("matrix").and_then(Value::as_array).filter(|m| m.len() == 16)
        .map(|m| std::array::from_fn(|i| to_number(m.get(i)))).ok_or_else(|| error("a trap has no placement matrix"))?;
    let matrix = Matrix4 { elements: placement };
    let model = desc.get("model").and_then(Value::as_str).unwrap_or("");
    let clips = cx.content.prop_clips.iter().find(|(name, _)| name == model).and_then(|(_, clips)| clips.as_ref());
    let clip = |key: &str| t.get(key).filter(|v| v.truthy()).and_then(Value::as_str).and_then(|name| clips.and_then(|c| c.get(name))).map(|c| c.frames_f64());
    let [ox, oy, oz] = vec3(t.get("offset"));
    let mut point = Vector3::new(ox, oy, oz);
    // No stock trap names a trigger node; one that does is refused rather
    // than placed in the wrong frame.
    if t.get("node").is_some_and(Value::truthy) { return Err(error("a trap's trigger node is not placed natively yet")); }
    point.apply_matrix4(&matrix);
    let [ex, ey, ez] = vec3(t.get("deathEffectOffset"));
    let mut effect = Vector3::new(ex, ey, ez);
    effect.apply_matrix4(&matrix);
    let mut pad_sphere = crate::gen::Bounds { center: [placement[12], placement[13], placement[14]], radius: 1.0 };
    if let Some(pad) = t.get("pad").filter(|v| v.truthy()) {
        let [ax, ay, az] = vec3(pad.get("at"));
        let mut at = Matrix4::default();
        at.make_translation(ax, ay, az).premultiply(&matrix);
        let kernel = cx.ids.kernel.as_ref().ok_or(AdmitError::Unavailable(Stage::Kernels))?.world;
        let from = Vector3::new(at.elements[12], at.elements[13] + 20.0, at.elements[14]);
        if let Some(hit) = crate::units::native::cast_in(world, kernel as usize, &from, &Vector3::new(0.0, -1.0, 0.0), 60.0) {
            at.elements[13] = from.y - hit.distance;
        }
        let sphere = pad.get("model").and_then(Value::as_str).and_then(|m| data.get("models").and_then(|ms| ms.get(m)))
            .and_then(|m| m.get("sphere")).and_then(Value::as_array).filter(|s| s.len() >= 4);
        match sphere {
            Some(s) => {
                let mut center = Vector3::new(to_number(s.first()), to_number(s.get(1)), to_number(s.get(2)));
                center.apply_matrix4(&at);
                pad_sphere = crate::gen::Bounds { center: [center.x, center.y, center.z], radius: to_number(s.get(3)) };
            }
            None => pad_sphere.center = [at.elements[12], at.elements[13], at.elements[14]],
        }
    }
    Ok(props::Trap { pad_sphere, point: [point.x, point.y, point.z], effect_at: [effect.x, effect.y, effect.z],
        build_frames: clip("build"), trigger_frames: clip("trigger") })
}

/// F3Hulls.prepare over the fleet (Vehicles.vehicles, then F2's prepared
/// variants: every other registered hull in slot order): the registry
/// VehicleTarget of a live hull, the health type and classifier of its
/// class, its blast sphere (the template's rig sphere widened to the hull's
/// half diagonal, else the hull's own centre), the hull's body and its
/// kernel slot, and the whole body list once (F3Sim.bodiesIn publishes it
/// when the collision world's version moved, which the first hull always
/// finds). F1 explosions register at the row.
pub fn hulls(world: &World, cx: &Cx) -> Result<Vec<units::HullBinding>, AdmitError> {
    let Some(data) = cx.battle.vehicle_def.as_ref() else { return Ok(Vec::new()) };
    let store = &world.sim.store;
    let plans = fleet::hulls(data, &cx.battle.world)?;
    let mut refs = store.vehicles.vehicles.clone();
    refs.extend(store.vehicle.iter().map(|(r, _)| r).filter(|r| !store.vehicles.vehicles.contains(r)));
    if plans.len() != refs.len() { return Err(error("the fleet's hulls differ from its plan")); }
    let classes = data.get("classes").ok_or_else(|| error("vehicle pack has no classes"))?;
    let collision = world.fleet.collision.as_ref().ok_or_else(|| error("the fleet has no collision world"))?;
    let targets: Vec<u32> = store.units.list.iter()
        .filter_map(|m| match m { Member::VehicleTarget(r) => Some(r.slot), _ => None }).collect();
    let all: Vec<F3BodyName> = cx.ids.bodies.iter().map(|(body, kernel)| F3BodyName { body: *body, kernel: *kernel }).collect();
    let mut out = Vec::with_capacity(plans.len());
    let mut live = 0usize;
    for (index, (plan, hull)) in plans.iter().zip(&refs).enumerate() {
        let vehicle = store.vehicle.get(*hull).ok_or_else(|| error("a fleet hull is missing from the store"))?;
        if String::from_utf16_lossy(world.sim.strings.units(vehicle.name)) != plan.name {
            return Err(error("the fleet's hull order differs from its plan"));
        }
        let def = classes.get(&plan.name).ok_or_else(|| error(format!("vehicle class {} is missing", plan.name)))?;
        let settings = def.get("settings");
        // F3Hulls HEALTH: person 0, animal 1, droid 2, vehicle 3, building 4.
        let health_type = settings.and_then(|s| s.get("HealthType")).and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("vehicle").to_lowercase();
        let health = match health_type.as_str() { "person" => 0, "animal" => 1, "droid" => 2, "vehicle" => 3, "building" => 4, _ => 0 };
        // VehicleTarget: an animal is category 1, any other hull 3; its kind
        // is the class's classifier (seats.js vehicleKind).
        let category = if health_type == "animal" { 1 } else { 3 };
        let kind = match def.get("base").and_then(Value::as_str).unwrap_or("hover") { "walker" | "commandwalker" => 2.0, "flyer" => 5.0, _ => 6.0 };
        let (member, key) = if plan.prepared { (None, String::new()) } else {
            let id = *targets.get(live).ok_or_else(|| error("a live hull has no registry target"))?;
            live += 1;
            (Some(Member::VehicleTarget(Ref::new(id, 0))), format!("v{}", live - 1))
        };
        // `0.5 * vehicle.size.length()`: THREE's length is a plain square root.
        let radius = 0.5 * (vehicle.size[0] * vehicle.size[0] + vehicle.size[1] * vehicle.size[1] + vehicle.size[2] * vehicle.size[2]).sqrt();
        let model = def.get("model").and_then(Value::as_str).unwrap_or("");
        let sphere = cx.content.vehicle_templates.iter().find(|t| t.name == model).and_then(|t| t.rig.get("sphere")).and_then(Value::as_array);
        let (blast_center, blast_radius) = match sphere {
            Some(s) => ([to_number(s.first()), to_number(s.get(1)), to_number(s.get(2))], to_number(s.get(3)).max(radius)),
            None => (vehicle.center, radius),
        };
        let kernel = collision.kernel_slot(vehicle.body.slot).ok_or_else(|| error("a hull's body is outside the collision world"))? as u32;
        let setup = F3HullSetup { hull: *hull, member, health, category, kind, key: crate::store::StrId(0),
            blast_center, blast_radius, explosion: None, final_explosion: None };
        out.push(units::HullBinding { name: plan.name.clone(), key, setup, body: F3BodyName { body: vehicle.body, kernel },
            changed_bodies: (index == 0).then(|| all.clone()) });
    }
    Ok(out)
}

/// Both receipt tables, when no earlier producer (a partial run's
/// fixtures) supplied them.
pub fn fill(world: &mut World, cx: &mut Cx) -> Result<(), AdmitError> {
    if cx.ids.props.is_empty() { cx.ids.props = buildings(world, cx)?; }
    if cx.ids.hulls.is_empty() { cx.ids.hulls = hulls(world, cx)?; }
    Ok(())
}
