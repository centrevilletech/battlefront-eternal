//! Retained hull construction shared by F2 and native replay fixtures.
//! The pack supplies immutable rig/clip templates; vehicle-phase-data.js
//! pullPose supplies a new facade life's initial pose once, at adoption.
//! Native aimTick and render then keep the rig and pose in Rust. Compact
//! generated publications expose only world-readable scalar and geometry
//! samples; no whole rig or rider body is handed back to JavaScript.
//!
//! Node order matches rigNodes: root, bones, static bones and markers. A
//! count mismatch rejects adoption instead of selecting a JavaScript pose.

use super::pose::HullPose;
use bfe_content::clips::{keep_last_tracks, load_clips};
use bfe_content::content::VehicleTemplate;
use bfe_content::value::{Object, Value};
use std::rc::Rc;
use three::Vector3;

/// `Object.fromEntries(Object.entries(models).map(([k, m]) => [k, m.parts]))`.
fn parts_records(models: Option<&Value>) -> Value {
    let mut out: Vec<(Rc<str>, Value)> = Vec::new();
    if let Some(o) = models.and_then(Value::as_object) {
        for (k, m) in o.entries() {
            out.push((Rc::from(k), m.get("parts").cloned().unwrap_or(Value::Null)));
        }
    }
    Value::Object(Rc::new(Object::from_written(out)))
}

/// The pack's templates by model name (vehicles.js 1952-1979, as the
/// content crate's battle loader makes them): `def` the pack's JSON (its
/// models, collision and animations), its models' floats and indices, and
/// its animations' floats.
pub fn templates(def: &Value, floats: &[f32], indices: &[u32], animations: Rc<[f32]>) -> Result<Vec<VehicleTemplate>, String> {
    let models = bfe_content::models::model_pack(Some(&parts_records(def.get("models"))), floats, indices)?;
    let mut out = Vec::new();
    for (name, rig) in def.get("models").and_then(Value::as_object).map(|o| o.entries().collect::<Vec<_>>()).unwrap_or_default() {
        let col = def.get("collision").and_then(|c| c.get(name)).ok_or_else(|| format!("vehicle {name} has no collision"))?;
        let shape = bfe_content::collision::json_shape(col);
        let parts = models.get(name).cloned().unwrap_or_default();
        let wheel_radii = bfe_content::derived::wheel_radii(&parts);
        let muzzle_tips = bfe_content::derived::vehicle_muzzle_tips(rig, &parts);
        let clips = match def.get("animations").and_then(|a| a.get(name)).filter(|m| m.truthy()) {
            Some(meta) => {
                let names: Vec<String> = bfe_content::derived::rig_bones(rig).into_iter().map(|b| b.name).collect();
                let mut clips = load_clips(meta, &animations, Some(&names))?;
                keep_last_tracks(&mut clips);
                Some(clips)
            }
            None => None,
        };
        let gear_foot = match clips.as_ref().and_then(|c| c.get("takeoff")) {
            Some(takeoff) => bfe_content::gear::gear_foot(rig, &bfe_content::gear::vehicle_batches(rig, &parts), takeoff),
            None => None,
        };
        out.push(VehicleTemplate { name: name.to_string(), rig: rig.clone(), parts, shape, clips, wheel_radii, muzzle_tips, gear_foot });
    }
    Ok(out)
}

/// How many numbers `pull` reads for this pose.
pub fn pull_len(p: &HullPose) -> usize {
    let mut n = 7;
    for s in &p.seats {
        n += 2 + 1 + 4;
        for w in &s.weapons {
            n += 1 + w.aimers.len() * 8 + w.muzzles.len() * 2;
        }
    }
    n + nodes(p) * 10
}

/// The rig's nodes JavaScript has (all but the scene, which is made last).
pub fn nodes(p: &HullPose) -> usize {
    p.rig.tree.nodes.len() - 1
}

fn flag(x: f64) -> bool {
    x != 0.0
}

/// A new facade life's pose (vehicle-phase-data.js pullPose): age, hover and
/// walk phases, walk blend and visual; per seat its last aim yaw (or none), aim rate and target (or
/// none), per weapon the barrel it fires next, its aimers' turn, limits
/// flag, on-target flag, facing and whether they posed their node, and its
/// muzzles' kick time (or none); then each rig node's position, quaternion
/// and scale. False where the counts do not agree.
pub fn pull(p: &mut HullPose, v: &[f64]) -> bool {
    if v.len() != pull_len(p) {
        return false;
    }
    let mut i = 0;
    let mut next = || {
        let x = v[i];
        i += 1;
        x
    };
    p.age = next();
    p.hover_phase = next();
    p.walk_phase = next();
    p.walk_blend = next();
    p.visual = Vector3::new(next(), next(), next());
    for s in &mut p.seats {
        let (has, yaw) = (flag(next()), next());
        s.last_yaw = has.then_some(yaw);
        s.yaw_rate = next();
        let (has, x, y, z) = (flag(next()), next(), next(), next());
        s.target = has.then(|| Vector3::new(x, y, z));
        for w in &mut s.weapons {
            w.barrel = next();
            for a in &mut w.aimers {
                a.yaw = next();
                a.pitch = next();
                let limits = next();
                a.in_limits = if limits == 2.0 { None } else { Some(flag(limits)) };
                a.on_target = flag(next());
                a.facing = Vector3::new(next(), next(), next());
                a.posed = flag(next());
            }
            for m in &mut w.muzzles {
                let (has, at) = (flag(next()), next());
                m.kick_at = has.then_some(at);
            }
        }
    }
    for k in 0..nodes(p) {
        let n = p.rig.tree.node_mut(k as three::NodeId);
        n.position = Vector3::new(next(), next(), next());
        let (x, y, z, w) = (next(), next(), next(), next());
        n.set_quaternion(x, y, z, w);
        n.scale = Vector3::new(next(), next(), next());
    }
    true
}
