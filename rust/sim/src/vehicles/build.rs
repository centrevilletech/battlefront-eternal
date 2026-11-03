//! A hull made from the battle's content: src/vehicles.js's Vehicle
//! constructor, as Vehicles.variant calls it for a spawn slot and a class.
//! What the constructor works out of the class (src/vehicle-motion.js
//! groundMotion, src/vehicle-water.js waterDamageRule, seatView's limits
//! and turn rates, the aimers' limits and rates), of its model's collision
//! shape (the box three-mesh-bvh's tree gives it, the probes, the radii)
//! and of its rig at rest (P5's HullRig: the turrets, aimers, muzzles and
//! riders; the wheels' axes and a walker's legs), every number as the
//! JavaScript makes it.

use super::class::HullClass;
use super::hull::{Hull, Seat, WaterRule, Wheel, UP};
use super::pose::{Aimer, HullPose, Kick, SeatPose, WeaponPose};
use super::BodyId;
use crate::rig::hull::HullRig;
use bfe_content::clips::Clips;
use bfe_content::content::VehicleTemplate;
use bfe_content::js::string_to_number;
use bfe_content::odf::{class_setting, num, number_first, setting, split_space, to_text};
use bfe_content::value::Value;
use three::{Quaternion, Vector3};
use v8math::{max, min};

const PI: f64 = std::f64::consts::PI;
/// vehicles.js DEG (`Math.PI / 180`).
const DEG: f64 = PI / 180.0;
/// vehicles.js TURRET_PITCH: a turret seat's pitch bound when its section sets none.
const TURRET_PITCH: f64 = 1.5;
/// three-mesh-bvh FLOAT32_EPSILON (`Math.pow(2, -24)`).
const FLOAT32_EPSILON: f64 = 5.960464477539063e-8;

/// `Math.fround`.
fn fround(x: f64) -> f64 {
    x as f32 as f64
}

/// A collision shape's box as src/collision.js createBodyShape keeps it
/// (`localBounds`): three-mesh-bvh's tree over its triangles (indirect, one
/// root) and that root's bounds, `getBoundingBox` of the tree. Each
/// triangle's centre and half extent per axis go into a Float32Array
/// (computeTriangleBounds, the extent widened by FLOAT32_EPSILON of its
/// size), the root's bounds are the least and greatest centre less and
/// plus extent (getBounds) kept as f32 (the node's Float32Array), and the
/// box is those six. (min, max); an empty shape's are +Infinity and
/// -Infinity, an empty Box3.
pub fn local_bounds(positions: &[f32], indices: &[u32]) -> ([f64; 3], [f64; 3]) {
    let corner = |i: usize, axis: usize| -> f64 {
        match indices.get(i) {
            Some(&v) => positions.get(v as usize * 3 + axis).map_or(f64::NAN, |&f| f as f64),
            None => f64::NAN,
        }
    };
    // getTriCount: the index's length over 3; the root's range is its floor.
    let count = indices.len() / 3;
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for tri in 0..count {
        for axis in 0..3 {
            let (a, b, c) = (corner(tri * 3, axis), corner(tri * 3 + 1, axis), corner(tri * 3 + 2, axis));
            let mut low = a;
            if b < low {
                low = b;
            }
            if c < low {
                low = c;
            }
            let mut high = a;
            if b > high {
                high = b;
            }
            if c > high {
                high = c;
            }
            let half = (high - low) / 2.0;
            let center = fround(low + half);
            let extent = fround(half + (low.abs() + half) * FLOAT32_EPSILON);
            let (l, r) = (center - extent, center + extent);
            if l < lo[axis] {
                lo[axis] = l;
            }
            if r > hi[axis] {
                hi[axis] = r;
            }
        }
    }
    (lo.map(fround), hi.map(fround))
}

/// src/vehicle-motion.js groundMotion: top speeds forward, back and
/// sideways, acceleration and deceleration, and the turn rate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    pub speed: f64,
    pub reverse_speed: f64,
    pub strafe_speed: f64,
    pub acceleration: f64,
    pub deceleration: f64,
    pub turn_rate: f64,
}

/// vehicle-motion.js `number(def, name, fallback, read)` over the class's
/// keys (classSetting: the body's, else a later seat's).
fn class_number(def: &Value, name: &str, fallback: f64) -> f64 {
    num(class_setting(Some(def), name), fallback)
}

/// The same over the body section's own keys (`body`).
fn body_number(def: &Value, name: &str, fallback: f64) -> f64 {
    num(setting(def.get("settings"), name), fallback)
}

/// A class's base (`def.base || 'hover'`).
pub fn base_of(def: &Value) -> String {
    match def.get("base").filter(|b| b.truthy()) {
        Some(b) => to_text(Some(b)),
        None => "hover".to_string(),
    }
}

/// vehicle-motion.js gaitSpeed: a walker's pace from its walk clips.
fn gait_speed(clips: Option<&Clips>, animation_speed: f64) -> f64 {
    let left = clips.and_then(|c| c.get("walk_leftfoot_rightfoot"));
    let right = clips.and_then(|c| c.get("walk_rightfoot_leftfoot"));
    // `left?.frames || 0`
    let frames_of = |c: Option<&bfe_content::clips::Clip>| c.map(|c| c.frames_f64()).filter(|&f| !(f == 0.0 || f.is_nan())).unwrap_or(0.0);
    let frames = frames_of(left) + frames_of(right);
    if frames > 0.0 {
        let (left, right) = (left.expect("a walker's left walk clip"), right.expect("a walker's right walk clip"));
        (left.stride + right.stride) * 30.0 / frames * animation_speed
    } else {
        0.0
    }
}

pub fn ground_motion(def: &Value, clips: Option<&Clips>) -> Motion {
    let base = base_of(def);
    if base == "flyer" {
        return Motion::default();
    }
    if base == "walker" || base == "commandwalker" {
        let turn_rate = body_number(def, "MaxTurnSpeed", 1.7);
        let gait = gait_speed(clips, class_number(def, "AnimationSpeed", 1.0));
        let health = class_setting(Some(def), "HealthType").filter(|v| v.truthy());
        let animal = health.is_some_and(|h| to_text(Some(h)).to_lowercase() == "animal");
        let gaited = !(gait == 0.0 || gait.is_nan());
        let speed = if gaited { gait } else { class_number(def, "MaxSpeed", 12.0) };
        let acceleration = class_number(def, "Acceleraton", 8.0) * if animal && gaited { speed } else { 1.0 };
        return Motion {
            speed,
            reverse_speed: speed,
            strafe_speed: 0.0,
            acceleration,
            deceleration: if animal && gaited { acceleration } else { class_number(def, "Deceleration", 10.0) },
            turn_rate,
        };
    }
    Motion {
        speed: class_number(def, "ForwardSpeed", 20.0),
        reverse_speed: class_number(def, "ReverseSpeed", 10.0),
        strafe_speed: class_number(def, "StrafeSpeed", 15.0),
        acceleration: class_number(def, "Acceleration", 10.0),
        deceleration: class_number(def, "Deceleration", 5.0),
        turn_rate: num(setting(def.get("settings"), "TurnRate"), body_number(def, "MaxTurnSpeed", 1.7)),
    }
}

/// vehicle-water.js waterDamageRule over a class's settings.
pub fn water_damage_rule(settings: Option<&Value>) -> Option<WaterRule> {
    let key = |k: &str| settings.and_then(|s| s.get(k));
    let number = |v: Option<&Value>, fallback: f64| {
        let n = number_first(v);
        if n.is_finite() {
            n
        } else {
            fallback
        }
    };
    let amount = number(key("WaterDamageAmount"), 0.0);
    if !(amount > 0.0) {
        return None;
    }
    Some(WaterRule { interval: number(key("WaterDamageInterval"), 1.0), amount })
}

/// vehicles.js `degrees(v)`: a pair of degrees as radians (`vector(v)`:
/// String(v) split on white space, each Number), else None.
fn degrees(v: Option<&Value>) -> Option<[f64; 2]> {
    let v = v.filter(|v| v.truthy())?;
    let text = to_text(Some(v));
    let parts: Vec<f64> = split_space(&text).into_iter().map(string_to_number).collect();
    if parts.len() >= 2 && parts[0].is_finite() && parts[1].is_finite() {
        Some([parts[0] * DEG, parts[1] * DEG])
    } else {
        None
    }
}

/// vehicles.js `vec3(v, fallback)`: three numbers of String(v).trim()
/// split on white space, else the fallback.
fn vec3_key(v: Option<&Value>, fallback: Vector3) -> Vector3 {
    let text = match v {
        None | Some(Value::Null) => return fallback,
        other => to_text(other),
    };
    let trimmed = text.trim_matches(bfe_content::js::is_js_space);
    let parts: Vec<f64> = split_space(trimmed).into_iter().map(string_to_number).collect();
    if parts.len() >= 3 && parts[..3].iter().all(|x| x.is_finite()) {
        Vector3::new(parts[0], parts[1], parts[2])
    } else {
        fallback
    }
}

/// A walker's leg for a tow cable (the constructor's `legs`): its thigh,
/// calf and foot at rest in the hull's frame, where its model has them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TowLeg {
    pub name: String,
    pub thigh: Option<Vector3>,
    pub calf: Option<Vector3>,
    pub foot: Option<Vector3>,
}

/// What the constructor makes beside the hull's tick state and pose: its
/// radius, a walker's legs and the height they hang from, its gear, and
/// each seat's view turn rate (a rider's 9-pose reads it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HullExtras {
    pub radius: f64,
    pub leg_top: f64,
    pub legs: Vec<TowLeg>,
    pub gear_foot: Option<f64>,
    pub seat_turn_rates: Vec<f64>,
}

/// A seat's limits as seatView keeps them: pitch up positive, yaw from the
/// hull's heading (None: free), whether the yaw arc runs round the back.
fn seat_limits(settings: Option<&Value>, index: usize, flying: bool) -> ([f64; 2], Option<[f64; 2]>, bool) {
    let key = |k: &str| settings.and_then(|s| s.get(k));
    if index == 0 {
        // The hover class's default pitch limits are half a radian each way.
        let pitch = if flying { [-PI, PI] } else { degrees(key("PitchLimits")).unwrap_or([-0.5, 0.5]) };
        return (pitch, None, false);
    }
    let pitch = degrees(key("PitchLimits"));
    let pitch_limits = match pitch {
        Some(p) if p[0] > -PI && p[1] < PI => [-p[1], -p[0]],
        _ => [-TURRET_PITCH, TURRET_PITCH],
    };
    let yaw = degrees(key("YawLimits"));
    let wrap = yaw.is_some_and(|y| y[0] > 0.0 && y[0] <= PI && y[1] >= PI && y[1] < 2.0 * PI);
    let yaw_limits = yaw.filter(|y| wrap || y[0] > -PI + 1e-4 && y[1] < PI - 1e-4);
    (pitch_limits, yaw_limits, wrap)
}

/// What a hull is made from: its class (`data.classes[name]`), its model's
/// template, its spawn slot's matrix (elements, column-major).
pub struct HullSource<'a> {
    pub name: &'a str,
    pub def: &'a Value,
    pub template: &'a VehicleTemplate,
    pub slot_matrix: &'a [f64; 16],
}

/// The Vehicle constructor: hull `index` of the battle's list, its store
/// handle `id`, its collision body `body` (collision.addMovingBody's).
pub fn build(src: &HullSource, index: usize, id: crate::store::Ref<crate::gen::Vehicle>, body: BodyId) -> (Hull, HullExtras) {
    let def = src.def;
    let template = src.template;
    let settings = def.get("settings");
    let s = |k: &str| settings.and_then(|s| s.get(k));
    let base = base_of(def);
    let flying = base == "flyer";
    let walking = base == "walker" || base == "commandwalker";
    let mut rig = HullRig::new(&crate::rig::content::hull_desc(def, template));
    let has_pose = rig.pose_root.is_some();
    // The box, its centre and size (Box3 getCenter, getSize: zero for an
    // empty box).
    let (lo, hi) = local_bounds(&template.shape.positions, &template.shape.indices);
    let bounds_min = Vector3::new(lo[0], lo[1], lo[2]);
    let bounds_max = Vector3::new(hi[0], hi[1], hi[2]);
    let empty = hi[0] < lo[0] || hi[1] < lo[1] || hi[2] < lo[2];
    let (center, size) = if empty {
        (Vector3::default(), Vector3::default())
    } else {
        (
            Vector3::new((lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5),
            Vector3::new(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]),
        )
    };
    let radius = max(size.x, size.z) * 0.5;
    let probe_radius = max(0.45, min(size.x * 0.48, 2.6));
    let length = max(0.0, size.z * 0.5 - probe_radius);
    let rows = max(2.0, min(12.0, (size.z / (probe_radius * 1.5)).ceil()));
    let width = max(0.0, size.x * 0.5 - probe_radius);
    let columns: Vec<f64> = if width > probe_radius { vec![-width, 0.0, width] } else { vec![0.0] };
    let mut probes = Vec::new();
    let mut row = 0.0;
    while row <= rows {
        for &x in &columns {
            probes.push(Vector3::new(center.x + x, bounds_min.y + probe_radius + 0.12, center.z - length + row * length * 2.0 / rows));
        }
        row += 1.0;
    }
    let max_health = num(s("MaxHealth"), 1000.0);
    let water_rule = if walking { water_damage_rule(settings) } else { None };
    // `template.rig.sphere?.[3] ?? this.size.y`
    let water_depth = match template.rig.get("sphere").and_then(|sp| sp.as_array()).and_then(|a| a.get(3)) {
        None | Some(Value::Null) => size.y,
        Some(v) => v.as_f64().unwrap_or(f64::NAN),
    };
    let health_type = match s("HealthType").filter(|v| v.truthy()) {
        Some(v) => to_text(Some(v)).to_lowercase(),
        None => "vehicle".to_string(),
    };
    let creature = walking && health_type == "animal";
    let motion = ground_motion(def, template.clips.as_ref());
    let hover = if creature {
        0.0
    } else if walking {
        0.08
    } else {
        num(s("SetAltitude"), 0.5)
    };
    let support_probes = if creature { [-1.0, 0.0, 1.0].iter().map(|&x| Vector3::new(x * size.x * 0.3, 0.0, 0.0)).collect() } else { probes.clone() };
    let class = HullClass::new(def, motion.turn_rate);
    let wheeled = src.name.contains("hailfire");
    let repulsor = !walking && !wheeled;
    let e = src.slot_matrix;
    let hover_phase = (e[12] * 0.13 + e[14] * 0.17) % (PI * 2.0);
    // The wheels: their rims lie in local XZ, local Y their axle; the
    // mirrored left axle spins the other way.
    let mut wheels = Vec::new();
    let mut wheel_bones = Vec::new();
    for name in ["wheel_left", "wheel_right"] {
        let Some(bone) = rig.by_name.get(name).map(|i| rig.bones[i]) else { continue };
        let mut q = Quaternion::default();
        rig.tree.get_world_quaternion(bone, &mut q);
        let mut world_axis = UP;
        world_axis.apply_quaternion(&q);
        let k = v8math::sign(world_axis.x * 1.0 + world_axis.y * 0.0 + world_axis.z * 0.0);
        let k = if k == 0.0 || k.is_nan() { 1.0 } else { k };
        let axis = Vector3::new(0.0 * k, 1.0 * k, 0.0 * k);
        let mut at = Vector3::default();
        rig.tree.get_world_position(bone, &mut at);
        let radius = template.wheel_radii.iter().find(|(n, _)| n == name).map(|&(_, r)| r).filter(|&r| !(r == 0.0 || r.is_nan())).unwrap_or(4.0);
        wheels.push(Wheel { x: at.x, radius, phase: 0.0, previous: 0.0, tick_from: None });
        wheel_bones.push((Some(bone), axis));
    }
    // Each seat: its limits and view (seatView), its aim's rates (aimCrew),
    // its weapons' aimers and muzzles.
    let class_seats = def.get("seats").and_then(Value::as_array).unwrap_or(&[]);
    let mut seats = Vec::with_capacity(class_seats.len());
    let mut seat_poses = Vec::with_capacity(class_seats.len());
    let mut seat_turn_rates = Vec::with_capacity(class_seats.len());
    for (index, seat) in class_seats.iter().enumerate() {
        let ss = seat.get("settings").filter(|v| v.truthy());
        let key = |k: &str| ss.and_then(|s| s.get(k));
        let (pitch_limits, yaw_limits, wrap_yaw) = seat_limits(ss, index, flying);
        let (turn_rate, pitch_rate) = class.seat_rates.get(index).copied().unwrap_or((f64::NAN, f64::NAN));
        seats.push(Seat { turn_rate, pitch_rate, pitch_limits, yaw_limits, wrap_yaw, aim_yaw: 0.0, pitch: 0.0, look_yaw: None, look_pitch: None, lease: None });
        seat_turn_rates.push(num(key("TurnRate"), if index > 0 { 2.0 } else { motion.turn_rate }));
        let rig_seat = &rig.seats[index];
        let weapons = seat
            .get("weapons")
            .and_then(Value::as_array)
            .unwrap_or(&[])
            .iter()
            .enumerate()
            .map(|(w, weapon)| {
                let rig_weapon = &rig_seat.weapons[w];
                let aimers = weapon
                    .get("aimers")
                    .and_then(Value::as_array)
                    .unwrap_or(&[])
                    .iter()
                    .enumerate()
                    .map(|(a, aimer)| {
                        let yaw_limits = degrees(aimer.get("AimerYawLimits")).unwrap_or([-30.0 * DEG, 30.0 * DEG]);
                        Aimer {
                            bone: rig_weapon.aimers[a].bone,
                            yaw: 0.0,
                            pitch: 0.0,
                            in_limits: None,
                            on_target: false,
                            facing: Vector3::new(0.0, 0.0, 1.0),
                            posed: true,
                            owner: rig_weapon.aimers[a].owner,
                            yaw_rate: num(aimer.get("AimerYawRate"), 2.0),
                            pitch_rate: num(aimer.get("AimerPitchRate"), 1.0),
                            pitch_limits: degrees(aimer.get("AimerPitchLimits")).unwrap_or([-45.0 * DEG, 45.0 * DEG]),
                            wrap_yaw: yaw_limits[0] > 0.0 && yaw_limits[0] <= PI && yaw_limits[1] >= PI && yaw_limits[1] < 2.0 * PI,
                            yaw_limits,
                        }
                    })
                    .collect();
                let muzzles = rig_weapon.muzzles.iter().map(|m| Kick { barrel: m.barrel, rest: m.rest, recoil: m.recoil, kick_at: None }).collect();
                WeaponPose { barrel: 0.0, aimers, muzzles }
            })
            .collect();
        seat_poses.push(SeatPose {
            last_yaw: None,
            yaw_rate: 0.0,
            target: None,
            eye: vec3_key(key("EyePointOffset"), Vector3::default()),
            pivot: rig_seat.pivot,
            up: rig_seat.up,
            yaw_factor: num(key("YawTurnFactor"), 1.0),
            pitch_factor: num(key("PitchTurnFactor"), 1.0),
            turret: rig_seat.turret,
            weapons,
            aiming: false,
            aim: Vector3::default(),
        });
    }
    // A walker's legs for the tow cable, at rest in its frame, and the
    // height of the pelvis they hang from.
    let mut legs: Vec<TowLeg> = Vec::new();
    let mut leg_top = bounds_min.y + size.y * 0.6;
    if walking {
        let named: Vec<(String, usize)> = rig.by_name.entries.clone();
        let mut pelvis = Vec::new();
        for (name, bone) in &named {
            if name.ends_with("pelvis") {
                let mut at = Vector3::default();
                rig.tree.get_world_position(rig.bones[*bone], &mut at);
                pelvis.push(at.y);
            }
        }
        if !pelvis.is_empty() {
            leg_top = pelvis.iter().fold(f64::INFINITY, |m, &y| min(m, y));
        }
        for (name, bone) in &named {
            let Some((part, stem)) = ["thigh", "calf", "foot"].iter().find_map(|p| name.strip_suffix(p).map(|stem| (*p, stem))) else { continue };
            let stem = stem.strip_suffix('_').unwrap_or(stem).to_string();
            let mut at = Vector3::default();
            rig.tree.get_world_position(rig.bones[*bone], &mut at);
            let i = match legs.iter().position(|l| l.name == stem) {
                Some(i) => i,
                None => {
                    legs.push(TowLeg { name: stem, ..TowLeg::default() });
                    legs.len() - 1
                }
            };
            match part {
                "thigh" => legs[i].thigh = Some(at),
                "calf" => legs[i].calf = Some(at),
                _ => legs[i].foot = Some(at),
            }
        }
        legs.retain(|l| l.calf.is_some() || l.thigh.is_some());
    }
    let has_legs = def.get("fx").and_then(|f| f.get("legs")).is_some_and(|l| l.truthy());
    let pose = HullPose {
        clips: super::pose::walk_clips(template),
        leg_pairs: super::pose::leg_pairs(def),
        rig,
        visual: Vector3::default(),
        age: 0.0,
        hover_phase,
        walk_phase: 0.0,
        walk_blend: 0.0,
        has_pose,
        legs: has_legs,
        wheel_bones,
        seats: seat_poses,
    };
    let hull = Hull {
        index,
        id,
        flying,
        walking,
        creature,
        wheeled,
        repulsor,
        speed: motion.speed,
        reverse_speed: motion.reverse_speed,
        strafe_speed: motion.strafe_speed,
        acceleration: motion.acceleration,
        deceleration: motion.deceleration,
        turn_rate: motion.turn_rate,
        hover,
        bounds_min,
        bounds_max,
        center,
        size,
        bound_radius: size.length() * 0.5,
        probe_radius,
        probes,
        support_probes,
        max_health,
        water_rule,
        water_depth,
        hover_class: Some(class.hover.clone()),
        legs: if creature { None } else { class.legs.clone() },
        body,
        dying_span: class.dying_span,
        explosion_offset: class.explosion_offset,
        has_explosion: class.has_explosion,
        water_clock: water_rule.map_or(0.0, |r| r.interval),
        quaternion: Quaternion::default(),
        support_normal: UP,
        wheels,
        seats,
        visible: false,
        tow_target: -1.0,
        has_final_explosion: class.has_final_explosion,
        final_offset: class.final_offset,
        pose: Some(pose),
        ..Hull::default()
    };
    let extras = HullExtras { radius, leg_top, legs, gear_foot: if flying { template.gear_foot } else { None }, seat_turn_rates };
    (hull, extras)
}
