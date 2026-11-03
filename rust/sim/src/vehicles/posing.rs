//! The hulls' poses kept in the module (rust/sim/src/vehicles/module.rs):
//! each hull's rig and pose state (`HullPose`) lives here from its
//! first phase on, posed by aimTick and render (pose.rs) inside the phase,
//! and the battle's JavaScript objects (the Vehicle's walk cycle and
//! visual, its seats' aim targets, its aimers, its rig's nodes) are made to
//! match it after each pose, so every reader there sees what it saw when
//! JavaScript posed them.
//!
//! - `templates`: the vehicle pack's models as the content crate builds
//!   them (src/vehicles.js 1952-1979: the rig, its parts, its clips, its
//!   wheels, muzzle tips and gear foot), from the pack JavaScript loaded.
//! - `pull`: a hull's whole pose state from JavaScript, where the module
//!   meets the hull first and after each spawn (Vehicle.spawn resets it and
//!   poses the hull once): everything a pose reads that is not its class's.
//! - `inputs`: what JavaScript changes between poses each tick: the seats'
//!   frozen aim (latchCrews) and the barrels a seat fired (fireSeat).
//! - `out`: what a pose leaves, for JavaScript's objects.
//!
//! The layouts are flat lists of numbers in the hull's own order (seats,
//! their weapons, each weapon's aimers and muzzles, the rig's nodes as the
//! capture's rigNodes lists them): JavaScript writes and reads them in the
//! same order (src/vehicles-sim.js), and the counts must agree, else the
//! hull is left to JavaScript's pose.

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

/// A hull's whole pose state from JavaScript (`v` as src/vehicles-sim.js
/// pullPose writes it): its age, hover and walk phases, walk blend and
/// visual; per seat its last aim yaw (or none), aim rate and target (or
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

/// How many numbers `inputs` reads for this pose.
pub fn inputs_len(p: &HullPose) -> usize {
    let mut n = 0;
    for s in &p.seats {
        n += 4;
        for w in &s.weapons {
            n += 1 + w.muzzles.len() * 2;
        }
    }
    n
}

/// What JavaScript changed of a hull's pose state since its last pose:
/// per seat the frozen aim its lease's input holds (aiming, aim), per
/// weapon the barrel it fires next and its muzzles' kick times. Returns
/// how many numbers it read.
pub fn inputs(p: &mut HullPose, v: &[f64]) -> usize {
    let mut i = 0;
    for s in &mut p.seats {
        s.aiming = flag(v[i]);
        s.aim = Vector3::new(v[i + 1], v[i + 2], v[i + 3]);
        i += 4;
        for w in &mut s.weapons {
            w.barrel = v[i];
            i += 1;
            for m in &mut w.muzzles {
                m.kick_at = flag(v[i]).then_some(v[i + 1]);
                i += 2;
            }
        }
    }
    i
}

/// What a pose left, for JavaScript (src/vehicles-sim.js poseOut reads it
/// in this order): the age, walk phase and blend, the visual; per seat its
/// last aim yaw (or none), aim rate and target (or none), and its
/// weapons' aimers (turn, limits flag, on-target flag, facing, posed);
/// each rig node's position and quaternion; the root's Euler angles.
pub fn out(p: &mut HullPose, o: &mut Vec<f64>) {
    o.push(p.age);
    o.push(p.walk_phase);
    o.push(p.walk_blend);
    o.extend_from_slice(&[p.visual.x, p.visual.y, p.visual.z]);
    for s in &p.seats {
        match s.last_yaw {
            Some(y) => o.extend_from_slice(&[1.0, y]),
            None => o.extend_from_slice(&[0.0, 0.0]),
        }
        o.push(s.yaw_rate);
        match s.target {
            Some(t) => o.extend_from_slice(&[1.0, t.x, t.y, t.z]),
            None => o.extend_from_slice(&[0.0, 0.0, 0.0, 0.0]),
        }
        for w in &s.weapons {
            for a in &w.aimers {
                let limits = match a.in_limits {
                    None => 2.0,
                    Some(true) => 1.0,
                    Some(false) => 0.0,
                };
                o.extend_from_slice(&[a.yaw, a.pitch, limits, if a.on_target { 1.0 } else { 0.0 }, a.facing.x, a.facing.y, a.facing.z, if a.posed { 1.0 } else { 0.0 }]);
            }
        }
    }
    for k in 0..nodes(p) {
        let n = p.rig.tree.node(k as three::NodeId);
        let q = n.quaternion;
        o.extend_from_slice(&[n.position.x, n.position.y, n.position.z, q.x, q.y, q.z, q.w]);
    }
    let root = p.rig.root;
    let e = p.rig.tree.node_mut(root).rotation();
    o.extend_from_slice(&[e.x, e.y, e.z]);
}
