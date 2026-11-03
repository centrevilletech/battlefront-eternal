//! A hull's pose as the replay harness's captures hold it (feature
//! `replay`): tools/sim-capture/specs.mjs `poseState` writes it, field by
//! field (the rig's nodes are not the store's yet), and [`pose_from_value`]
//! and [`pose_to_value`] read and write the same shape, so `sim_replay::diff`
//! compares a replayed pose with the capture's to the last bit. A hull's
//! tick state is the store's (rust/sim/schema/vehicles.mjs) and goes through
//! the generated code.

use super::hull::{Hull, Lease, Seat, Wheel};
use three::{Quaternion, Vector3};
use sim_replay::{Arena, Node, Value, V};

fn vec3(v: V) -> Vector3 {
    let a = v.vec3().unwrap_or_else(|| panic!("not a Vector3: {}", v.show()));
    Vector3::new(a[0], a[1], a[2])
}

fn quat(v: V) -> Quaternion {
    match v.node() {
        Some(Node::Quat(q)) => Quaternion::new(q[0], q[1], q[2], q[3]),
        _ => panic!("not a Quaternion: {}", v.show()),
    }
}

fn nums<const N: usize>(v: V) -> [f64; N] {
    let mut out = [0.0; N];
    for (i, x) in out.iter_mut().enumerate() {
        *x = v.at(i).num();
    }
    out
}

fn opt_num(v: V) -> Option<f64> {
    if v.is_null() {
        None
    } else {
        Some(v.num())
    }
}

/// A body handle's serial.
pub fn body_of(v: V) -> u32 {
    v.handle_num("body").unwrap_or_else(|| panic!("not a body: {}", v.show())) as u32
}

fn num(x: f64) -> Value {
    Value::Num(x)
}

fn opt(x: Option<f64>) -> Value {
    x.map_or(Value::Null, Value::Num)
}

fn v3(a: &mut Arena, v: &Vector3) -> Value {
    a.vec3([v.x, v.y, v.z])
}

fn list(a: &mut Arena, xs: &[f64]) -> Value {
    let items = xs.iter().map(|&x| Value::Num(x)).collect();
    a.array(items)
}

fn opt_vec3(v: V) -> Option<Vector3> {
    if v.is_null() {
        None
    } else {
        Some(vec3(v))
    }
}

fn node(v: V) -> Option<three::NodeId> {
    v.opt_num().map(|n| n as three::NodeId)
}

/// A hull from a pose frame's capture (`poseState`), its rig built from its
/// class (`rig`) and posed as captured: each node's position, quaternion
/// and scale (the rig's world matrices are worked out where read).
pub fn pose_from_value(v: V, index: usize, mut rig: crate::rig::hull::HullRig) -> Hull {
    use super::pose::{Aimer, HullPose, Kick, SeatPose, WeaponPose};
    let locals: Vec<f64> = if v.get("rig").is_null() { Vec::new() } else { v.get("rig").items().map(|x| x.num()).collect() };
    for (i, c) in locals.chunks_exact(10).enumerate() {
        let n = rig.tree.node_mut(i as three::NodeId);
        n.position = Vector3::new(c[0], c[1], c[2]);
        n.set_quaternion(c[3], c[4], c[5], c[6]);
        n.scale = Vector3::new(c[7], c[8], c[9]);
    }
    let seats: Vec<SeatPose> = v
        .get("seats")
        .items()
        .map(|s| {
            let lease = s.get("lease");
            SeatPose {
                last_yaw: opt_num(s.get("lastYaw")),
                yaw_rate: s.get("yawRate").num(),
                target: opt_vec3(s.get("target")),
                eye: vec3(s.get("eye")),
                pivot: vec3(s.get("pivot")),
                up: vec3(s.get("up")),
                yaw_factor: s.get("yawFactor").num(),
                pitch_factor: s.get("pitchFactor").num(),
                turret: node(s.get("turret")),
                aiming: !lease.is_null() && lease.get("aiming").bool(),
                aim: if lease.is_null() { Vector3::default() } else { vec3(lease.get("aim")) },
                weapons: s
                    .get("weapons")
                    .items()
                    .map(|w| WeaponPose {
                        barrel: w.get("barrel").num(),
                        aimers: w
                            .get("aimers")
                            .items()
                            .map(|a| Aimer {
                                bone: node(a.get("bone")),
                                yaw: a.get("yaw").num(),
                                pitch: a.get("pitch").num(),
                                in_limits: if a.get("inLimits").is_undefined() { None } else { Some(a.get("inLimits").bool()) },
                                on_target: a.get("onTarget").bool(),
                                facing: vec3(a.get("facing")),
                                posed: a.get("posed").bool(),
                                owner: a.get("owner").num() as i32,
                                yaw_rate: a.get("yawRate").num(),
                                pitch_rate: a.get("pitchRate").num(),
                                yaw_limits: nums::<2>(a.get("yawLimits")),
                                pitch_limits: nums::<2>(a.get("pitchLimits")),
                                wrap_yaw: a.get("wrapYaw").bool(),
                            })
                            .collect(),
                        muzzles: w
                            .get("muzzles")
                            .items()
                            .map(|m| Kick { barrel: node(m.get("barrel")), rest: opt_vec3(m.get("rest")), recoil: opt_num(m.get("recoil")), kick_at: opt_num(m.get("kickAt")) })
                            .collect(),
                    })
                    .collect(),
            }
        })
        .collect();
    let sleeping = if v.get("sleeping").is_undefined() { None } else { Some(v.get("sleeping").bool()) };
    Hull {
        index,
        flying: v.get("flying").bool(),
        creature: v.get("creature").bool(),
        repulsor: v.get("repulsor").bool(),
        max_health: v.get("maxHealth").num(),
        body: body_of(v.get("body")),
        position: vec3(v.get("position")),
        previous: vec3(v.get("previous")),
        velocity: vec3(v.get("velocity")),
        yaw: v.get("yaw").num(),
        previous_yaw: v.get("previousYaw").num(),
        tilt: v.get("tilt").num(),
        previous_tilt: v.get("previousTilt").num(),
        roll: v.get("roll").num(),
        previous_roll: v.get("previousRoll").num(),
        alive: v.get("alive").bool(),
        sleeping,
        health: v.get("health").num(),
        quaternion: quat(v.get("quaternion")),
        leg_side: if v.get("legSide").is_null() { None } else { Some(v.get("legSide").items().map(|x| x.num()).collect()) },
        seat_index: v.get("seatIndex").num() as usize,
        driver: v.get("driver").bool(),
        wheels: v.get("wheels").items().map(|w| Wheel { x: 0.0, radius: 0.0, phase: w.get("phase").num(), previous: w.get("previous").num() }).collect(),
        seats: v
            .get("seats")
            .items()
            .map(|s| Seat {
                aim_yaw: s.get("aimYaw").num(),
                pitch: s.get("pitch").num(),
                lease: if s.get("lease").is_null() {
                    None
                } else {
                    let l = s.get("lease");
                    Some(Lease { human: l.get("human").bool(), network_human: l.get("networkHuman").bool(), turn: 0.0, pitch: 0.0 })
                },
                ..Seat::default()
            })
            .collect(),
        pose: Some(HullPose {
            wheel_bones: v.get("wheels").items().map(|w| (node(w.get("bone")), vec3(w.get("axis")))).collect(),
            rig,
            visual: vec3(v.get("visual")),
            age: v.get("age").num(),
            hover_phase: v.get("hoverPhase").num(),
            walk_phase: v.get("walkPhase").num(),
            walk_blend: v.get("walkBlend").num(),
            has_pose: v.get("pose").bool(),
            legs: v.get("legs").bool(),
            seats,
            clips: None,
            leg_pairs: None,
        }),
        ..Hull::default()
    }
}

/// The hull as `poseState` writes it; `posed`: whether the frame posed its
/// rig (its nodes, and at the end their world matrices, are written then).
pub fn pose_to_value(a: &mut Arena, h: &mut Hull, name: &str, posed: bool) -> Value {
    let p = h.pose.as_mut().expect("a posed hull");
    let n = p.rig.tree.nodes.len() - 1; // all but the scene
    let (rig, world) = if posed {
        let mut locals = Vec::with_capacity(n * 10);
        let mut worlds = Vec::with_capacity(n * 16);
        for i in 0..n {
            let node = p.rig.tree.node(i as three::NodeId);
            let (pp, q, s) = (node.position, node.quaternion, node.scale);
            locals.extend_from_slice(&[pp.x, pp.y, pp.z, q.x, q.y, q.z, q.w, s.x, s.y, s.z]);
            worlds.extend_from_slice(&node.matrix_world.elements);
        }
        (list(a, &locals), list(a, &worlds))
    } else {
        (Value::Null, Value::Null)
    };
    let p = h.pose.as_ref().unwrap();
    let node_value = |n: Option<three::NodeId>| n.map_or(Value::Null, |n| Value::Num(n as f64));
    let wheels = h
        .wheels
        .iter()
        .zip(&p.wheel_bones)
        .map(|(w, (bone, axis))| {
            let axis = v3(a, axis);
            a.object("", vec![("bone", node_value(*bone)), ("axis", axis), ("phase", num(w.phase)), ("previous", num(w.previous))])
        })
        .collect();
    let wheels = a.array(wheels);
    let seats = h
        .seats
        .iter()
        .zip(&p.seats)
        .map(|(s, sp)| {
            let target = match sp.target {
                None => Value::Null,
                Some(t) => v3(a, &t),
            };
            let lease = match s.lease {
                None => Value::Null,
                Some(l) => {
                    let aim = v3(a, &sp.aim);
                    a.object("", vec![("human", Value::Bool(l.human)), ("networkHuman", Value::Bool(l.network_human)), ("aiming", Value::Bool(sp.aiming)), ("aim", aim)])
                }
            };
            let (eye, pivot, up) = (v3(a, &sp.eye), v3(a, &sp.pivot), v3(a, &sp.up));
            let weapons = sp
                .weapons
                .iter()
                .map(|w| {
                    let aimers = w
                        .aimers
                        .iter()
                        .map(|m| {
                            let facing = v3(a, &m.facing);
                            let (yl, pl) = (list(a, &m.yaw_limits), list(a, &m.pitch_limits));
                            a.object(
                                "",
                                vec![
                                    ("bone", node_value(m.bone)),
                                    ("yaw", num(m.yaw)),
                                    ("pitch", num(m.pitch)),
                                    ("inLimits", m.in_limits.map_or(Value::Undefined, Value::Bool)),
                                    ("onTarget", Value::Bool(m.on_target)),
                                    ("facing", facing),
                                    ("posed", Value::Bool(m.posed)),
                                    ("owner", num(m.owner as f64)),
                                    ("yawRate", num(m.yaw_rate)),
                                    ("pitchRate", num(m.pitch_rate)),
                                    ("yawLimits", yl),
                                    ("pitchLimits", pl),
                                    ("wrapYaw", Value::Bool(m.wrap_yaw)),
                                ],
                            )
                        })
                        .collect();
                    let aimers = a.array(aimers);
                    let muzzles = w
                        .muzzles
                        .iter()
                        .map(|m| {
                            let rest = match m.rest {
                                None => Value::Null,
                                Some(r) => v3(a, &r),
                            };
                            a.object("", vec![("barrel", node_value(m.barrel)), ("rest", rest), ("recoil", opt(m.recoil)), ("kickAt", opt(m.kick_at))])
                        })
                        .collect();
                    let muzzles = a.array(muzzles);
                    a.object("", vec![("barrel", num(w.barrel)), ("aimers", aimers), ("muzzles", muzzles)])
                })
                .collect();
            let weapons = a.array(weapons);
            a.object(
                "",
                vec![
                    ("aimYaw", num(s.aim_yaw)),
                    ("pitch", num(s.pitch)),
                    ("lastYaw", opt(sp.last_yaw)),
                    ("yawRate", num(sp.yaw_rate)),
                    ("target", target),
                    ("lease", lease),
                    ("eye", eye),
                    ("pivot", pivot),
                    ("up", up),
                    ("yawFactor", num(sp.yaw_factor)),
                    ("pitchFactor", num(sp.pitch_factor)),
                    ("turret", node_value(sp.turret)),
                    ("weapons", weapons),
                ],
            )
        })
        .collect();
    let seats = a.array(seats);
    let (position, previous, visual, velocity) = (v3(a, &h.position), v3(a, &h.previous), v3(a, &p.visual), v3(a, &h.velocity));
    let leg_side = match &h.leg_side {
        None => Value::Null,
        Some(s) => list(a, s),
    };
    let quaternion = a.quat([h.quaternion.x, h.quaternion.y, h.quaternion.z, h.quaternion.w]);
    a.object(
        "",
        vec![
            ("name", Value::str(name)),
            ("alive", Value::Bool(h.alive)),
            ("sleeping", h.sleeping.map_or(Value::Undefined, Value::Bool)),
            ("repulsor", Value::Bool(h.repulsor)),
            ("flying", Value::Bool(h.flying)),
            ("creature", Value::Bool(h.creature)),
            ("health", num(h.health)),
            ("maxHealth", num(h.max_health)),
            ("pose", Value::Bool(p.has_pose)),
            ("legs", Value::Bool(p.legs)),
            ("body", Value::handle("body", Value::Num(h.body as f64))),
            ("position", position),
            ("previous", previous),
            ("visual", visual),
            ("velocity", velocity),
            ("yaw", num(h.yaw)),
            ("previousYaw", num(h.previous_yaw)),
            ("tilt", num(h.tilt)),
            ("previousTilt", num(h.previous_tilt)),
            ("roll", num(h.roll)),
            ("previousRoll", num(h.previous_roll)),
            ("age", num(p.age)),
            ("hoverPhase", num(p.hover_phase)),
            ("walkPhase", num(p.walk_phase)),
            ("walkBlend", num(p.walk_blend)),
            ("legSide", leg_side),
            ("quaternion", quaternion),
            ("seatIndex", num(h.seat_index as f64)),
            ("driver", Value::Bool(h.driver)),
            ("wheels", wheels),
            ("seats", seats),
            ("rig", rig),
            ("world", world),
        ],
    )
}
