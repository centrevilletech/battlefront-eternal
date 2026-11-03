//! The rigs' inputs from the content as rust/content (bfe_content) loads it,
//! the same objects the JavaScript builds them from: a body's bones (the
//! actor's records), its clips, a droideka's guns and their barrel tips, and
//! a hull's rig and seats (its class in the battle's vehicle definitions and
//! its model's template). Behind the crate's `content` feature.

use super::droideka::{Barrel, Gun};
use super::hull::{AimerDesc, BarrelDesc, HullDesc, SeatDesc, WeaponDesc};
use super::skeleton::{BoneDesc, Skeleton};
use super::three::Vector3;
use crate::pose::Clip;
use bfe_content::actor::Actor;
use bfe_content::clips::{Clip as ContentClip, Clips};
use bfe_content::content::VehicleTemplate;
use bfe_content::derived::DroidekaGun;
use bfe_content::js::number_to_string;
use bfe_content::odf::num;
use bfe_content::value::Value;

/// A parent index as the JavaScript tests it (`parent >= 0`): -1 otherwise.
fn parent_index(parent: f64) -> i32 {
    if parent >= 0.0 {
        parent as i32
    } else {
        -1
    }
}

/// A body's bones (the actor's records), for `Skeleton::actor`.
pub fn actor_bones(actor: &Actor) -> Vec<BoneDesc> {
    actor.bones.iter().map(|b| BoneDesc { name: b.name.clone(), parent: parent_index(b.parent), local: b.local }).collect()
}

/// A body's skeleton as buildActor makes it.
pub fn actor_skeleton(actor: &Actor) -> Skeleton {
    Skeleton::actor(&actor_bones(actor))
}

/// A clip as the pose sampler holds it: its frames, rate, root motion,
/// tracks and samples (a copy of its slice of the bank).
pub fn clip(clip: &ContentClip) -> Clip {
    let frames = clip.frames_f64();
    Clip {
        frames: if frames >= 0.0 { frames as usize } else { 0 },
        fps: clip.fps,
        root_motion: clip.root_motion,
        tracks: clip.tracks.iter().map(|&t| if t >= 0.0 { t as i32 } else { -1 }).collect(),
        data: clip.data().to_vec(),
    }
}

/// A droideka's guns and barrels (Droideka.arm): each gun's bone, and a
/// marker at each barrel tip found on its arm's mesh, or the gun's bone
/// where none was found.
pub fn droideka_guns(guns: &[DroidekaGun]) -> Vec<Gun> {
    guns.iter()
        .map(|g| Gun {
            bone: g.bone,
            barrels: match &g.tips {
                Some(tips) => tips.iter().map(|t| Barrel::Marker(Vector3::new(t.x, t.y, t.z))).collect(),
                None => vec![Barrel::GunBone],
            },
        })
        .collect()
}

/// `String(value)` of a key the JavaScript reads as `String(x || '')`: None
/// for a falsy value.
fn name(value: Option<&Value>) -> Option<String> {
    let v = value.filter(|v| v.truthy())?;
    Some(match v {
        Value::String(s) => s.to_string(),
        Value::Number(n) => number_to_string(*n),
        Value::Bool(b) => b.to_string(),
        // An object or array as a name would be "[object Object]" or its
        // joined elements; no class has one.
        _ => return None,
    })
}

/// The seat's pivot key (f2aa4a90) as `vec3(s.f2aa4a90, ZERO)` reads it:
/// three numbers split on white space, else the origin.
fn pivot(value: Option<&Value>) -> Vector3 {
    let text = match value {
        None | Some(Value::Null) => return Vector3::default(),
        Some(Value::String(s)) => s.to_string(),
        Some(Value::Number(n)) => number_to_string(*n),
        Some(Value::Bool(b)) => b.to_string(),
        Some(_) => return Vector3::default(),
    };
    // String(v).trim().split(/\s+/).map(Number): the runs of space split
    // it, and an empty string's one empty piece is no number of three.
    let parts: Vec<f64> = text.trim_matches(bfe_content::js::is_js_space).split(bfe_content::js::is_js_space).filter(|p| !p.is_empty()).map(bfe_content::js::string_to_number).collect();
    if parts.len() >= 3 && parts[..3].iter().all(|x| x.is_finite()) {
        Vector3::new(parts[0], parts[1], parts[2])
    } else {
        Vector3::default()
    }
}

/// The rig's inputs for a vehicle class: its definition in the battle's
/// vehicle file (`data.classes[name]`) and its model's template.
pub fn hull_desc(class: &Value, template: &VehicleTemplate) -> HullDesc {
    let settings = class.get("settings");
    let health_type = settings.and_then(|s| s.get("HealthType")).filter(|v| v.truthy()).and_then(Value::as_str).unwrap_or("vehicle").to_lowercase();
    let base = class.get("base").filter(|v| v.truthy()).and_then(Value::as_str).unwrap_or("hover");
    let rig_bones = bfe_content::derived::rig_bones(&template.rig);
    let (animated, has_clips) = clips_animated(template.clips.as_ref());
    HullDesc {
        model: class.get("model").and_then(Value::as_str).unwrap_or("").to_string(),
        flying: base == "flyer",
        creature: health_type == "animal",
        bones: rig_bones.iter().map(|b| BoneDesc { name: b.name.clone(), parent: parent_index(b.parent), local: b.local.elements }).collect(),
        muzzle_tips: template.muzzle_tips.iter().map(|(n, t)| (n.clone(), Vector3::new(t[0], t[1], t[2]))).collect(),
        animated,
        has_clips,
        seats: class.get("seats").and_then(Value::as_array).unwrap_or(&[]).iter().map(seat_desc).collect(),
    }
}

/// The bones the clips drive (every track at or above 0), and whether there
/// are clips at all (`clips?.size`).
fn clips_animated(clips: Option<&Clips>) -> (Vec<usize>, bool) {
    let Some(clips) = clips else { return (Vec::new(), false) };
    let mut animated: Vec<usize> = Vec::new();
    for c in &clips.list {
        for &t in &c.tracks {
            if t >= 0.0 && !animated.contains(&(t as usize)) {
                animated.push(t as usize);
            }
        }
    }
    animated.sort_unstable();
    (animated, !clips.list.is_empty())
}

fn seat_desc(seat: &Value) -> SeatDesc {
    let s = seat.get("settings");
    let key = |k: &str| s.and_then(|s| s.get(k));
    let pilot_animation = name(key("Pilot9Pose")).or_else(|| name(key("PilotAnimation")));
    SeatDesc {
        turret_node_name: name(key("TurretNodeName")),
        fire_point_name: name(key("FirePointName")),
        pilot_animation,
        nine_pose: key("Pilot9Pose").is_some_and(|v| v.truthy()),
        pilot_position: name(key("PilotPosition")),
        rider_node_hash: name(key("11ce2025")),
        shootable: num(key("2b60e217"), 1.0),
        pivot: pivot(key("f2aa4a90")),
        weapons: seat.get("weapons").and_then(Value::as_array).unwrap_or(&[]).iter().map(weapon_desc).collect(),
    }
}

fn weapon_desc(w: &Value) -> WeaponDesc {
    let def = w.get("def");
    WeaponDesc {
        fire_point_name: name(def.and_then(|d| d.get("FirePointName"))),
        class: def.and_then(|d| d.get("class")).and_then(Value::as_str).unwrap_or("").to_string(),
        aimers: w.get("aimers").and_then(Value::as_array).unwrap_or(&[]).iter().map(aimer_desc).collect(),
    }
}

fn aimer_desc(a: &Value) -> AimerDesc {
    AimerDesc {
        node: name(a.get("node")),
        fire_point_name: name(a.get("FirePointName")),
        hierarchy_level: num(a.get("HierarchyLevel"), 0.0),
        barrels: a
            .get("barrels")
            .and_then(Value::as_array)
            .unwrap_or(&[])
            .iter()
            .map(|b| BarrelDesc {
                barrel_node_name: name(b.get("BarrelNodeName")),
                fire_point_name: name(b.get("FirePointName")),
                length: num(b.get("BarrelLength"), 0.0),
                recoil: b.get("BarrelRecoil").map(|r| num(Some(r), 0.0)),
            })
            .collect(),
    }
}
