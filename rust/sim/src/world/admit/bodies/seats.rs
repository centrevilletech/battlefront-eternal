//! Crew holder construction (f2-seats.js adopt/attachTurrets and
//! props.js Turret). Authored geometry is resolved once; a body birth then
//! binds only its retained sampler to these already admitted holders.
use super::{error, Rider};
use crate::{gen::*, store::{DefId, Ref}, world::admit::{rows::units, AdmitError}};
use bfe_content::{defs::VehicleClassDef, odf, Value};
use three::{Matrix4, Quaternion, TransformNode, TransformTree, Vector3};
const DEG: f64 = std::f64::consts::PI / 180.0;

/// F2Seats.adopt reads the seat's settings and constructed weapon count,
/// including empty seats. Prepared hulls always publish index zero.
pub fn hull_facts(vehicle: Ref<Vehicle>, index: usize, prepared: bool, class: &VehicleClassDef) -> SeatHullFacts {
    SeatHullFacts { vehicle, index: if prepared { 0.0 } else { index as f64 },
        hover: class.record.get("base").and_then(Value::as_str) == Some("hover"),
        animal: class.record.get("settings").and_then(|s| s.get("HealthType")).and_then(Value::as_str).is_some_and(|s| s.to_lowercase() == "animal"),
        turret_nodes: class.seats.iter().map(|s| s.record.get("settings").and_then(|s| s.get("TurretNodeName")).is_some_and(Value::truthy)).collect(),
        weapons: class.seats.iter().map(|s| s.weapons.len() as f64).collect(), ..Default::default() }
}

/// Turret's already-built model and camera/aim geometry, before any crew
/// operates it. Numeric identities and mutable building life are separate.
#[derive(Clone, Debug)]
pub struct TurretGeometry {
    pub settings: PtSettings,
    pub state: PtState,
    pub rider: Option<Rider>,
    pub shootable: bool,
}

/// buildRig and Turret's constructor from the placed instance and model.
/// This does not apply Turret.present: attachTurrets receives the bind rig,
/// even when a drum is authored with a half-turn of its own.
pub fn turret_geometry(instance: &Value, model: &Value, index: usize) -> Result<TurretGeometry, AdmitError> {
    let settings = instance.get("settings").ok_or_else(|| error("turret has no settings"))?;
    let records = model.get("bones").and_then(Value::as_array).ok_or_else(|| error("turret has no bones"))?;
    let placement = matrix(instance.get("matrix"))?;
    let mut tree = TransformTree::new();
    let root = tree.add_node(TransformNode::default());
    let mut names = Vec::new(); let mut bones = Vec::new(); let mut parents = Vec::new();
    for bone in records {
        let mut node = TransformNode::default(); node.decompose_from(&Matrix4 { elements: matrix(bone.get("local"))? });
        bones.push(tree.add_node(node));
        names.push(bone.get("name").and_then(Value::as_str).unwrap_or("").to_lowercase());
        let p = bfe_content::js::to_number(bone.get("parent"));
        if p >= 0.0 && (p.fract() != 0.0 || p >= records.len() as f64) { return Err(error("turret has an invalid bone parent")); }
        parents.push(if p >= 0.0 { p as i32 } else { -1 });
    }
    // Reject cycles before following parents in TransformTree's world walk.
    for start in 0..parents.len() {
        let mut p = start as i32; let mut visits = 0;
        while p >= 0 { visits += 1; if visits > parents.len() { return Err(error("turret has a cyclic rig")); } p = parents[p as usize]; }
    }
    for (i, &p) in parents.iter().enumerate() { tree.add_child(if p < 0 { root } else { bones[p as usize] }, bones[i]); }
    // LiveObjects.look hangs the placed root under the scene. Keeping its
    // identity parent preserves Three's signed-zero multiplication at bind.
    let scene = tree.add_node(TransformNode::default()); tree.add_child(scene,root);
    let node = tree.node_mut(root); node.matrix.elements = placement; node.matrix_auto_update = false;
    tree.update_matrix_world(root, true);
    // Object.fromEntries's duplicate names keep their last bone.
    let named = |name: &str| names.iter().rposition(|n| n == &name.to_lowercase());
    let key = |name| settings.get(name);
    let bone = |key_name| key(key_name).filter(|v| v.truthy()).map(|v| odf::to_text(Some(v))).and_then(|s| named(&s));
    let drum = bone("TurretNodeName"); let gun = bone("AimerNodeName"); let seat = bone("PilotPosition");
    let fire = key("FirePointName").filter(|v| v.truthy());
    let fire = match fire { Some(Value::Array(a)) => a.iter().map(|v| odf::to_text(Some(v))).collect::<Vec<_>>(),
        Some(v) => vec![odf::to_text(Some(v))], None => vec!["hp_fire".into()] };
    let mut fire_points = fire.iter().filter_map(|n| named(n)).collect::<Vec<_>>();
    if fire_points.is_empty() {
        if let Some(start) = gun.or(drum) { descendant_markers(start, &names, &bones, &tree, &mut fire_points); }
    }
    let mut root_turn = Quaternion::default(); tree.get_world_quaternion(root, &mut root_turn); root_turn.invert();
    let mut pivot = Vector3::default(); let mut up = Vector3::new(0., 1., 0.);
    if let Some(drum) = drum {
        tree.get_world_position(bones[drum], &mut pivot);
        let mut inverse = tree.node(root).matrix_world; inverse.invert(); pivot.apply_matrix4(&inverse);
        let mut turn = Quaternion::default(); tree.get_world_quaternion(bones[drum], &mut turn);
        let mut local = root_turn; local.multiply(&turn); up.apply_quaternion(&local).normalize();
    } else { pivot.from_array(&odf::scan_vector(key("f2aa4a90")).unwrap_or([0.;3]), 0); }
    let parent_turn = drum.and_then(|i| tree.node(bones[i]).parent).map(|parent| {
        let mut q = Quaternion::default(); tree.get_world_quaternion(parent, &mut q);
        let mut local = root_turn; local.multiply(&q).invert(); q4(local)
    });
    let gun_bind = gun.map(|i| q4(tree.node(bones[i]).quaternion));
    let pair = |name, fallback| odf::pair(key(name)).map(|p| [p[0] * DEG, p[1] * DEG]).unwrap_or(fallback);
    let pitch = pair("PitchLimits", [-45.*DEG, 45.*DEG]); let pitch_limits = [-pitch[1], -pitch[0]];
    let yaw = odf::pair(key("YawLimits")).map(|p| [p[0] * DEG, p[1] * DEG]);
    let wrap = yaw.is_some_and(|p| p[0] > 0. && p[0] <= std::f64::consts::PI && p[1] >= std::f64::consts::PI && p[1] < 2.*std::f64::consts::PI);
    let yaw_limits = yaw.filter(|p| wrap || p[0] > -std::f64::consts::PI + 1e-4 && p[1] < std::f64::consts::PI - 1e-4);
    let gun_pitch = pair("AimerPitchLimits", [-45.*DEG, 45.*DEG]);
    // EyePointOffset seeds center in authored property order. scanVector is
    // intentionally permissive for the dish turret's malformed eye string.
    let (mut eye, mut center, mut offset) = ([0.;3], [0.;3], [0.;3]);
    if let Some(object) = settings.as_object() { for (name, value) in object.entries() {
        if !matches!(name, "EyePointOffset" | "TrackCenter" | "TrackOffset") { continue; }
        let Some(v) = odf::scan_vector(Some(value)) else { continue; };
        match name { "EyePointOffset" => { eye = v; center = v; }, "TrackCenter" => center = v, _ => offset = v }
    }}
    let num = |name, fallback| odf::num(key(name), fallback);
    let authored = PtSettings { facing: v8math::atan2(placement[8], placement[10]),
        position: [placement[12], placement[13], placement[14]], matrix: placement, pitch_limits, yaw_limits, wrap_yaw: wrap,
        gun_pitch_limits: [v8math::min(gun_pitch[0], pitch_limits[0]), v8math::max(gun_pitch[1], pitch_limits[1])],
        gun_yaw_limits: pair("AimerYawLimits", [-30.*DEG,30.*DEG]), turn_rate: num("MaxTurnSpeed",2.), filter: num("TurnFilter",10.),
        gun_pitch_rate: num("AimerPitchRate",1.), gun_yaw_rate: num("AimerYawRate",2.), yaw_factor: num("YawTurnFactor",1.), pitch_factor: num("PitchTurnFactor",1.),
        camera_eye: eye, camera_center: center, camera_offset: offset, camera_tilt: num("TiltValue",0.)*DEG,
        pivot: v3(pivot), up: v3(up), drum_parent_turn: parent_turn, gun_bind,
        root: tree.node(root).matrix.elements, parents: parents.iter().map(|&p| p as f64).collect(),
        drum: drum.map(|i| i as f64), gun: gun.map(|i| i as f64), seat: seat.map(|i| i as f64), fire_points: fire_points.iter().map(|&i| i as f64).collect() };
    let state = PtState { index: index as f64, root_world: tree.node(root).matrix_world.elements,
        bones: bones.iter().map(|&id| { let b = tree.node(id); PaBone { p: v3(b.position), q: q4(b.quaternion), s: v3(b.scale), world: b.matrix_world.elements } }).collect(), ..Default::default() };
    let anim = key("Pilot9Pose").filter(|v| v.truthy()).or_else(|| key("PilotAnimation")).filter(|v| v.truthy());
    let rider = anim.map(|v| Rider { clip: Some(format!("human_{}", odf::to_text(Some(v)).to_lowercase())), anim: None,
        nine_pose: key("Pilot9Pose").is_some_and(Value::truthy) });
    Ok(TurretGeometry { settings: authored, state, rider, shootable: num("2b60e217",1.) != 0. })
}

/// F2Seats.attachTurrets' identities and the Building.sphere observation at
/// this construction point. Built/pad geometry and its collision body come
/// from the building owner, whose current look can differ from the gun rig.
#[derive(Clone, Debug)]
pub struct TurretBinding {
    pub turret: Ref<Turret>, pub life: f64, pub alive: bool, pub body: Option<Ref<Body>>,
    pub center: Vec3, pub radius: f64, pub ordnance: Option<DefId>, pub collide: bool,
}
pub fn turret_birth(geometry: TurretGeometry, binding: TurretBinding, weapon: Option<Weapon>, definition: Option<&Value>) -> Result<F2TurretBirth, AdmitError> {
    let levels = definition.map(units::weapon_levels).transpose()?.unwrap_or_default();
    let locks = levels.iter().map(|def| { let lock = units::lock_rule(def); F2TurretLockLevel {
        range: lock.range, on: lock.on_angle, off: lock.off_angle, relation_mask: lock.relation_mask, category_mask: lock.category_mask,
    }}).collect();
    let missile_lock = definition.is_some_and(|d| d.get("base").and_then(Value::as_str) == Some("launcher")
        && d.get("ordnance").and_then(|o| o.get("base")).and_then(Value::as_str) == Some("missile"));
    let channel = definition.map_or(0., |d| match d.get("channel") { None | Some(Value::Null) => 0., v => bfe_content::js::to_number(v) });
    Ok(F2TurretBirth { life: binding.life, timed: geometry.rider.as_ref().is_some_and(|r| !r.nine_pose), locks,
        settings: geometry.settings, turret: PcTurret { state: geometry.state, turret: binding.turret, alive: binding.alive,
            center: binding.center, radius: binding.radius, attacker: Attacker::Null,
            weapon: weapon.is_some(), missile_lock, body: binding.body, rider: geometry.rider.is_some(),
            shootable: geometry.rider.is_some() && geometry.shootable, ..Default::default() },
        weapon, ordnance: binding.ordnance, channel, collide: binding.collide })
}
fn matrix(value: Option<&Value>) -> Result<[f64;16], AdmitError> {
    let values = value.and_then(Value::as_array).filter(|a| a.len() == 16).ok_or_else(|| error("turret matrix must contain sixteen numbers"))?;
    let mut out = [0.;16];
    for (at, value) in values.iter().enumerate() { out[at] = bfe_content::js::to_number(Some(value)); if !out[at].is_finite() { return Err(error("turret has a nonfinite matrix")); } }
    Ok(out)
}
fn descendant_markers(index: usize, names: &[String], bones: &[u32], tree: &TransformTree, out: &mut Vec<usize>) {
    let name = &names[index];
    let marker = names.iter().rposition(|n| n == name) == Some(index) && name.strip_prefix("hp_").is_some_and(|n| n.starts_with("fire") || n.starts_with("gun") && !n.starts_with("gunner")
        || n.starts_with("cannon") || n.starts_with("laser") || n.starts_with("turret"));
    if marker { out.push(index); return; }
    for &child in &tree.node(bones[index]).children { if let Some(i) = bones.iter().position(|&id| id == child) { descendant_markers(i,names,bones,tree,out); } }
}
fn v3(v: Vector3) -> Vec3 { [v.x,v.y,v.z] }
fn q4(q: Quaternion) -> Quat { [q.x,q.y,q.z,q.w] }
