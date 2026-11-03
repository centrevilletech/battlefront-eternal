//! Numbers the JavaScript derives from models at load, made here from the
//! same data the same way: src/muzzles.js (barrelTips, weaponMuzzle,
//! vehicleMuzzleTips), src/items.js radiusOf (an item's radius and pickup
//! centre), src/droideka.js arm (its barrels' tips), src/vehicles.js (wheel
//! radii).

use crate::actor::Actor;
use crate::js::{js_max, js_min, to_number};
use crate::models::{Model, Part};
use crate::three::{Box3, Matrix4, Vector3};
use crate::value::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

fn axis(v: &Vector3, a: Axis) -> f64 {
    match a {
        Axis::X => v.x,
        Axis::Y => v.y,
        Axis::Z => v.z,
    }
}

fn set_axis(v: &mut Vector3, a: Axis, x: f64) {
    match a {
        Axis::X => v.x = x,
        Axis::Y => v.y = x,
        Axis::Z => v.z = x,
    }
}

/// src/muzzles.js barrelTips(points, forward, sign, across): the end rings
/// of a rig's tubes, one tip per half across.
pub fn barrel_tips(points: &[Vector3], forward: Axis, sign: f64, across: Axis) -> Vec<Vector3> {
    if points.is_empty() {
        return Vec::new();
    }
    let mut front = f64::NEG_INFINITY;
    let mut back = f64::INFINITY;
    for p in points {
        let depth = axis(p, forward) * sign;
        front = js_max(front, depth);
        back = js_min(back, depth);
    }
    let tolerance = js_max(0.003, (front - back) * 0.005);
    let ends: Vec<&Vector3> = points.iter().filter(|p| axis(p, forward) * sign >= front - tolerance).collect();
    let bounds = Box3::from_points(ends.iter().copied());
    let middle = (axis(&bounds.min, across) + axis(&bounds.max, across)) / 2.0;
    let mut halves = [Box3::default(), Box3::default()];
    for p in &ends {
        halves[if axis(p, across) > middle { 1 } else { 0 }].expand_by_point(p);
    }
    halves
        .iter()
        .filter(|b| !b.is_empty())
        .map(|b| {
            let mut tip = b.center();
            set_axis(&mut tip, forward, sign * (front + 0.002));
            tip
        })
        .collect()
}

/// What weaponMuzzle returns: the authored point as it was (any value, or
/// undefined), or one found from the mesh.
#[derive(Clone, Debug)]
pub enum Muzzle {
    Authored(Option<Value>),
    Found([f64; 3]),
}

/// src/muzzles.js weaponMuzzle(parts, authored, def): a held gun's muzzle
/// in its model's frame.
pub fn weapon_muzzle(parts: Option<&[Part]>, authored: Option<&Value>, def: Option<&Value>) -> Muzzle {
    let kept = || Muzzle::Authored(authored.cloned());
    let ordnance = def.and_then(|d| d.get("ordnance")).map(Value::truthy).unwrap_or(false);
    let grenade = def.and_then(|d| d.get("base")).and_then(Value::as_str) == Some("grenade");
    if !ordnance || grenade {
        return kept();
    }
    let mut points: Vec<Vector3> = Vec::new();
    for part in parts.unwrap_or(&[]) {
        for i in 0..part.count() {
            points.push(part.position(i));
        }
    }
    if points.is_empty() {
        return kept();
    }
    let bounds = Box3::from_points(points.iter());
    if bounds.max.z - bounds.min.z < 0.05 {
        return kept();
    }
    // An authored point already resolved against the mesh stays.
    if let Some(a) = authored {
        if a.truthy() {
            let z = match a {
                Value::Array(items) => to_number(items.get(2)),
                // authored[2] of a string is its third character; of
                // anything else, undefined.
                Value::String(s) => s.encode_utf16().nth(2).map(|u| crate::js::string_to_number(&String::from_utf16_lossy(&[u]))).unwrap_or(f64::NAN),
                _ => f64::NAN,
            };
            if z > 0.05 {
                return kept();
            }
        }
    }
    let front: Vec<&Vector3> = points.iter().filter(|p| p.z >= bounds.max.z - 0.003).collect();
    let mut tip = Box3::from_points(front.iter().copied()).center();
    tip.z = bounds.max.z;
    Muzzle::Found(tip.to_array())
}

fn num(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => *n,
        Some(other) => to_number(Some(other)),
        None => f64::NAN,
    }
}

/// A rig's bone records (vehicles.json / props.json models[name].bones).
pub struct RigBone {
    pub name: String,
    pub parent: f64,
    pub local: Matrix4,
}

pub fn rig_bones(rig: &Value) -> Vec<RigBone> {
    rig.get("bones")
        .and_then(Value::as_array)
        .unwrap_or(&[])
        .iter()
        .map(|b| {
            let local_v = b.get("local").and_then(Value::as_array).unwrap_or(&[]);
            let mut local = [f64::NAN; 16];
            for (i, slot) in local.iter_mut().enumerate() {
                *slot = local_v.get(i).map(|v| num(Some(v))).unwrap_or(f64::NAN);
            }
            RigBone { name: b.get("name").and_then(Value::as_str).unwrap_or("").to_string(), parent: num(b.get("parent")), local: Matrix4::from_array(&local) }
        })
        .collect()
}

/// Each bone's world matrix as `resolve` in vehicleMuzzleTips makes it:
/// the parent's (a fresh identity at a root) times the bone's local.
pub fn rig_world(bones: &[RigBone]) -> Vec<Matrix4> {
    fn resolve(i: usize, bones: &[RigBone], world: &mut Vec<Option<Matrix4>>) -> Matrix4 {
        if let Some(m) = world[i] {
            return m;
        }
        let parent = bones[i].parent;
        let base = if parent >= 0.0 { resolve(parent as usize, bones, world) } else { Matrix4::identity() };
        let m = Matrix4::multiply_matrices(&base, &bones[i].local);
        world[i] = Some(m);
        m
    }
    let mut world: Vec<Option<Matrix4>> = vec![None; bones.len()];
    (0..bones.len()).map(|i| resolve(i, bones, &mut world)).collect()
}

/// src/muzzles.js vehicleMuzzleTips(rig, parts): bone name (lower case) ->
/// muzzle, in the rig's bone order.
pub fn vehicle_muzzle_tips(rig: &Value, parts: &[Part]) -> Vec<(String, [f64; 3])> {
    let bones = rig_bones(rig);
    let world = rig_world(&bones);
    let inverse: Vec<Matrix4> = world.iter().map(Matrix4::inverted).collect();
    // points: Map(lower name -> vertices); a repeated name keeps its place
    // and starts over.
    let mut points: Vec<(String, Vec<Vector3>)> = Vec::new();
    for b in &bones {
        let name = b.name.to_lowercase();
        match points.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = Vec::new(),
            None => points.push((name, Vec::new())),
        }
    }
    for part in parts {
        let joints = part.field("joints");
        for i in 0..part.count() {
            // part.joints?.[i]
            let joint = match joints {
                Some(Value::Array(list)) => list.get(i),
                _ => None,
            };
            let name = match joint {
                Some(j) => {
                    let index = match j {
                        Value::Number(n) if n.fract() == 0.0 && *n >= 0.0 && (*n as usize) < bones.len() => Some(*n as usize),
                        _ => None,
                    };
                    index.map(|k| bones[k].name.to_lowercase())
                }
                None => part.bone().map(str::to_lowercase),
            };
            let Some(name) = name else { continue };
            let Some(slot) = points.iter_mut().position(|(n, _)| *n == name) else { continue };
            let mut p = part.position(i);
            if let Some(Value::Number(j)) = joint {
                p.apply_matrix4(&inverse[*j as usize]);
            }
            points[slot].1.push(p);
        }
    }
    let mut tips = Vec::new();
    for (name, vertices) in points {
        if vertices.is_empty() {
            continue;
        }
        let bounds = Box3::from_points(vertices.iter());
        if bounds.max.z - bounds.min.z < 0.05 {
            continue;
        }
        let mouth: Vec<&Vector3> = vertices.iter().filter(|p| p.z >= bounds.max.z - 0.003).collect();
        let mut center = Box3::from_points(mouth.iter().copied()).center();
        center.z = bounds.max.z;
        tips.push((name, center.to_array()));
    }
    tips
}

/// src/vehicles.js 1973-1983: each wheel bone's radius, half the larger of
/// its parts' joint bounds across x and z.
pub fn wheel_radii(parts: &[Part]) -> Vec<(String, f64)> {
    let mut bounds: Vec<(String, Box3)> = Vec::new();
    for part in parts {
        let bone = part.bone().map(str::to_lowercase);
        let Some(bone) = bone.filter(|b| b == "wheel_left" || b == "wheel_right") else { continue };
        let b = part.bounding_box();
        match bounds.iter_mut().find(|(n, _)| *n == bone) {
            Some(slot) => {
                slot.1.union(&b);
            }
            None => {
                let mut fresh = Box3::default();
                fresh.union(&b);
                bounds.push((bone, fresh));
            }
        }
    }
    bounds.into_iter().map(|(bone, b)| (bone, js_max(b.max.x - b.min.x, b.max.z - b.min.z) * 0.5)).collect()
}

/// src/items.js: an item's radius and pickup centre. With a model (the
/// item's GeometryName in the battle's weapon models), the largest of its
/// parts' bounding spheres (the first of equals), its centre kept; a model
/// of no parts gives 0.3 and no centre; no model, 0.3 and no centre.
pub fn item_radius(model: Option<&Model>) -> (f64, Option<Vector3>) {
    let Some(parts) = model else { return (0.3, None) };
    let mut best: Option<&Part> = None;
    for part in parts.iter() {
        if best.is_none_or(|b| part.sphere_radius > b.sphere_radius) {
            best = Some(part);
        }
    }
    match best {
        Some(p) => (p.sphere_radius, Some(p.sphere_center)),
        None => (0.3, None),
    }
}

/// A droideka's guns (src/droideka.js arm): each aimer's bone, and the
/// barrels' tips in the gun's parent bone's frame (None: the gun itself).
#[derive(Clone, Debug)]
pub struct DroidekaGun {
    pub bone: usize,
    pub tips: Option<Vec<Vector3>>,
}

/// droideka.js arm(weapons): the blaster is the first weapon with aimers.
pub fn droideka_guns(actor: &Actor, weapons: Option<&Value>) -> Vec<DroidekaGun> {
    let list = weapons.and_then(Value::as_array).unwrap_or(&[]);
    let blaster = list.iter().find(|w| match w.get("aimers") {
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::String(s)) => !s.is_empty(),
        _ => false,
    });
    let aimers = blaster.and_then(|b| b.get("aimers")).and_then(Value::as_array).unwrap_or(&[]);
    let mut guns = Vec::new();
    for name in aimers {
        let Some(name) = name.as_str() else { continue };
        let Some(bone) = actor.bone_by_lower_name(name) else { continue };
        guns.push(bone);
    }
    // The body's tree: a bone's parent object (bones[0]'s is the body).
    let parent_index = |i: usize| -> Option<usize> {
        if i == 0 {
            return None;
        }
        let p = actor.bones[i].parent;
        if p >= 0.0 {
            Some(p as usize)
        } else {
            None
        }
    };
    guns.into_iter()
        .map(|gun| {
            let Some(index) = parent_index(gun) else { return DroidekaGun { bone: gun, tips: None } };
            let to_bone = Matrix4::multiply_matrices(&actor.bones[index].inverse, &actor.bind_matrix);
            let mut points = Vec::new();
            for i in 0..actor.vertex_count() {
                if actor.skin_index[i * 4] as usize == index {
                    let mut p = actor.position(i);
                    p.apply_matrix4(&to_bone);
                    points.push(p);
                }
            }
            let tips = barrel_tips(&points, Axis::X, 1.0, Axis::Z);
            DroidekaGun { bone: gun, tips: if tips.is_empty() { None } else { Some(tips) } }
        })
        .collect()
}
