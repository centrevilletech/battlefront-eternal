//! A hull as the replay harness's captures hold it (feature `replay`):
//! tools/sim-capture/specs.mjs `hullState` writes it, field by field, and
//! [`hull_from_value`] and [`hull_to_value`] read and write the same shape,
//! so `sim_replay::diff` compares a replayed hull with the capture's to the
//! last bit. Until the store's generator holds a hull's nested lists, this
//! hand-written pair stands in for the generated code.

use super::hull::{HoverClass, Hull, Lease, Legs, Seat, Spring, WaterRule, Wheel};
use three::{Matrix4, Quaternion, Vector3};
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

/// A hull from the capture's `hullState` value; `index` is its place in the battle's list.
pub fn hull_from_value(v: V, index: usize) -> Hull {
    let hover_class = if v.get("hoverClass").is_null() {
        None
    } else {
        let h = v.get("hoverClass");
        Some(HoverClass {
            altitude: h.get("altitude").num(),
            gravity_scale: h.get("gravityScale").num(),
            lift_spring: h.get("liftSpring").num(),
            lift_damp: h.get("liftDamp").num(),
            traction: h.get("traction").num(),
            level_spring: h.get("levelSpring").num(),
            level_damp: h.get("levelDamp").num(),
            velocity_spring: h.get("velocitySpring").num(),
            velocity_damp: h.get("velocityDamp").num(),
            omega_x_spring: h.get("omegaXSpring").num(),
            omega_x_damp: h.get("omegaXDamp").num(),
            omega_z_spring: h.get("omegaZSpring").num(),
            omega_z_damp: h.get("omegaZDamp").num(),
            springs: h
                .get("springs")
                .items()
                .map(|s| Spring { at: vec3(s.get("at")), radius: s.get("radius").num(), length: s.get("length").num(), factors: nums::<6>(s.get("factors")) })
                .collect(),
        })
    };
    let legs = if v.get("legs").is_null() {
        None
    } else {
        let l = v.get("legs");
        Some(Legs {
            ray_length: l.get("rayLength").num(),
            orient_roll: l.get("orientRoll").bool(),
            width: l.get("width").num(),
            pairs: l.get("pairs").items().map(|p| [nums::<3>(p.get("left")), nums::<3>(p.get("right"))]).collect(),
        })
    };
    let water_rule = if v.get("waterRule").is_null() {
        None
    } else {
        Some(WaterRule { interval: v.get("waterRule").get("interval").num(), amount: v.get("waterRule").get("amount").num() })
    };
    let mut matrix = Matrix4::default();
    let elements: Vec<f64> = v.get("matrix").items().map(|x| x.num()).collect();
    matrix.from_array(&elements, 0);
    let sleeping = if v.get("sleeping").is_undefined() { None } else { Some(v.get("sleeping").bool()) };
    Hull {
        index,
        flying: v.get("flying").bool(),
        walking: v.get("walking").bool(),
        creature: v.get("creature").bool(),
        wheeled: v.get("wheeled").bool(),
        repulsor: v.get("repulsor").bool(),
        speed: v.get("speed").num(),
        reverse_speed: v.get("reverseSpeed").num(),
        strafe_speed: v.get("strafeSpeed").num(),
        acceleration: v.get("acceleration").num(),
        deceleration: v.get("deceleration").num(),
        turn_rate: v.get("turnRate").num(),
        hover: v.get("hover").num(),
        bounds_min: vec3(v.get("boundsMin")),
        bounds_max: vec3(v.get("boundsMax")),
        center: vec3(v.get("center")),
        size: vec3(v.get("size")),
        bound_radius: v.get("boundRadius").num(),
        probe_radius: v.get("probeRadius").num(),
        probes: v.get("probes").items().map(vec3).collect(),
        support_probes: v.get("supportProbes").items().map(vec3).collect(),
        max_health: v.get("maxHealth").num(),
        water_rule,
        water_depth: v.get("waterDepth").num(),
        hover_class,
        legs,
        body: body_of(v.get("body")),
        dying_span: v.get("dyingSpan").num(),
        explosion_offset: if v.get("explosionOffset").is_null() { None } else { Some(nums::<3>(v.get("explosionOffset"))) },
        has_explosion: v.get("hasExplosion").bool(),
        position: vec3(v.get("position")),
        previous: vec3(v.get("previous")),
        velocity: vec3(v.get("velocity")),
        yaw: v.get("yaw").num(),
        previous_yaw: v.get("previousYaw").num(),
        tilt: v.get("tilt").num(),
        roll: v.get("roll").num(),
        previous_tilt: v.get("previousTilt").num(),
        previous_roll: v.get("previousRoll").num(),
        yaw_rate: v.get("yawRate").num(),
        tilt_rate: v.get("tiltRate").num(),
        roll_rate: v.get("rollRate").num(),
        support: v.get("support").num(),
        forward_input: v.get("forwardInput").num(),
        right_input: v.get("rightInput").num(),
        alive: v.get("alive").bool(),
        sleeping,
        health: v.get("health").num(),
        dying: v.get("dying").num(),
        water_clock: v.get("waterClock").num(),
        quaternion: quat(v.get("quaternion")),
        matrix,
        item_center: vec3(v.get("itemCenter")),
        support_normal: vec3(v.get("supportNormal")),
        leg_side: if v.get("legSide").is_null() { None } else { Some(v.get("legSide").items().map(|x| x.num()).collect()) },
        seat_index: v.get("seatIndex").num() as usize,
        driver: v.get("driver").bool(),
        wheels: v
            .get("wheels")
            .items()
            .map(|w| Wheel { x: w.get("x").num(), radius: w.get("radius").num(), phase: w.get("phase").num(), previous: w.get("previous").num() })
            .collect(),
        seats: v
            .get("seats")
            .items()
            .map(|s| Seat {
                turn_rate: s.get("turnRate").num(),
                pitch_rate: s.get("pitchRate").num(),
                pitch_limits: nums::<2>(s.get("pitchLimits")),
                yaw_limits: if s.get("yawLimits").is_null() { None } else { Some(nums::<2>(s.get("yawLimits"))) },
                wrap_yaw: s.get("wrapYaw").bool(),
                aim_yaw: s.get("aimYaw").num(),
                pitch: s.get("pitch").num(),
                look_yaw: opt_num(s.get("lookYaw")),
                look_pitch: opt_num(s.get("lookPitch")),
                lease: if s.get("lease").is_null() {
                    None
                } else {
                    let l = s.get("lease");
                    Some(Lease { human: l.get("human").bool(), network_human: l.get("networkHuman").bool(), turn: l.get("turn").num(), pitch: l.get("pitch").num() })
                },
            })
            .collect(),
        pose: None,
    }
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

/// The hull as `hullState` writes it (its name, which the port does not
/// keep, given back as captured).
pub fn hull_to_value(a: &mut Arena, h: &Hull, name: &str) -> Value {
    let hover_class = match &h.hover_class {
        None => Value::Null,
        Some(c) => {
            let springs = c
                .springs
                .iter()
                .map(|s| {
                    let at = v3(a, &s.at);
                    let factors = list(a, &s.factors);
                    a.object("", vec![("at", at), ("radius", num(s.radius)), ("length", num(s.length)), ("factors", factors)])
                })
                .collect();
            let springs = a.array(springs);
            a.object(
                "",
                vec![
                    ("altitude", num(c.altitude)),
                    ("gravityScale", num(c.gravity_scale)),
                    ("liftSpring", num(c.lift_spring)),
                    ("liftDamp", num(c.lift_damp)),
                    ("traction", num(c.traction)),
                    ("levelSpring", num(c.level_spring)),
                    ("levelDamp", num(c.level_damp)),
                    ("velocitySpring", num(c.velocity_spring)),
                    ("velocityDamp", num(c.velocity_damp)),
                    ("omegaXSpring", num(c.omega_x_spring)),
                    ("omegaXDamp", num(c.omega_x_damp)),
                    ("omegaZSpring", num(c.omega_z_spring)),
                    ("omegaZDamp", num(c.omega_z_damp)),
                    ("springs", springs),
                ],
            )
        }
    };
    let legs = match &h.legs {
        None => Value::Null,
        Some(l) => {
            let pairs = l
                .pairs
                .iter()
                .map(|p| {
                    let left = list(a, &p[0]);
                    let right = list(a, &p[1]);
                    a.object("", vec![("left", left), ("right", right)])
                })
                .collect();
            let pairs = a.array(pairs);
            a.object("", vec![("rayLength", num(l.ray_length)), ("orientRoll", Value::Bool(l.orient_roll)), ("width", num(l.width)), ("pairs", pairs)])
        }
    };
    let water_rule = match h.water_rule {
        None => Value::Null,
        Some(r) => a.object("", vec![("interval", num(r.interval)), ("amount", num(r.amount))]),
    };
    let wheels = h
        .wheels
        .iter()
        .map(|w| a.object("", vec![("x", num(w.x)), ("radius", num(w.radius)), ("phase", num(w.phase)), ("previous", num(w.previous))]))
        .collect();
    let wheels = a.array(wheels);
    let seats = h
        .seats
        .iter()
        .map(|s| {
            let pitch_limits = list(a, &s.pitch_limits);
            let yaw_limits = match s.yaw_limits {
                None => Value::Null,
                Some(l) => list(a, &l),
            };
            let lease = match s.lease {
                None => Value::Null,
                Some(l) => a.object("", vec![("human", Value::Bool(l.human)), ("networkHuman", Value::Bool(l.network_human)), ("turn", num(l.turn)), ("pitch", num(l.pitch))]),
            };
            a.object(
                "",
                vec![
                    ("turnRate", num(s.turn_rate)),
                    ("pitchRate", num(s.pitch_rate)),
                    ("pitchLimits", pitch_limits),
                    ("yawLimits", yaw_limits),
                    ("wrapYaw", Value::Bool(s.wrap_yaw)),
                    ("aimYaw", num(s.aim_yaw)),
                    ("pitch", num(s.pitch)),
                    ("lookYaw", opt(s.look_yaw)),
                    ("lookPitch", opt(s.look_pitch)),
                    ("lease", lease),
                ],
            )
        })
        .collect();
    let seats = a.array(seats);
    let probes = h.probes.iter().map(|p| v3(a, p)).collect();
    let probes = a.array(probes);
    let support_probes = h.support_probes.iter().map(|p| v3(a, p)).collect();
    let support_probes = a.array(support_probes);
    let bounds_min = v3(a, &h.bounds_min);
    let bounds_max = v3(a, &h.bounds_max);
    let center = v3(a, &h.center);
    let size = v3(a, &h.size);
    let position = v3(a, &h.position);
    let previous = v3(a, &h.previous);
    let velocity = v3(a, &h.velocity);
    let quaternion = a.quat([h.quaternion.x, h.quaternion.y, h.quaternion.z, h.quaternion.w]);
    let matrix = list(a, &h.matrix.elements);
    let item_center = v3(a, &h.item_center);
    let support_normal = v3(a, &h.support_normal);
    let leg_side = match &h.leg_side {
        None => Value::Null,
        Some(s) => list(a, s),
    };
    let explosion_offset = match &h.explosion_offset {
        None => Value::Null,
        Some(o) => list(a, o),
    };
    a.object(
        "",
        vec![
            ("name", Value::str(name)),
            ("flying", Value::Bool(h.flying)),
            ("walking", Value::Bool(h.walking)),
            ("creature", Value::Bool(h.creature)),
            ("wheeled", Value::Bool(h.wheeled)),
            ("repulsor", Value::Bool(h.repulsor)),
            ("speed", num(h.speed)),
            ("reverseSpeed", num(h.reverse_speed)),
            ("strafeSpeed", num(h.strafe_speed)),
            ("acceleration", num(h.acceleration)),
            ("deceleration", num(h.deceleration)),
            ("turnRate", num(h.turn_rate)),
            ("hover", num(h.hover)),
            ("boundsMin", bounds_min),
            ("boundsMax", bounds_max),
            ("center", center),
            ("size", size),
            ("boundRadius", num(h.bound_radius)),
            ("probeRadius", num(h.probe_radius)),
            ("probes", probes),
            ("supportProbes", support_probes),
            ("maxHealth", num(h.max_health)),
            ("waterRule", water_rule),
            ("waterDepth", num(h.water_depth)),
            ("hoverClass", hover_class),
            ("legs", legs),
            ("body", Value::handle("body", Value::Num(h.body as f64))),
            ("dyingSpan", num(h.dying_span)),
            ("explosionOffset", explosion_offset),
            ("hasExplosion", Value::Bool(h.has_explosion)),
            ("position", position),
            ("previous", previous),
            ("velocity", velocity),
            ("yaw", num(h.yaw)),
            ("previousYaw", num(h.previous_yaw)),
            ("tilt", num(h.tilt)),
            ("roll", num(h.roll)),
            ("previousTilt", num(h.previous_tilt)),
            ("previousRoll", num(h.previous_roll)),
            ("yawRate", num(h.yaw_rate)),
            ("tiltRate", num(h.tilt_rate)),
            ("rollRate", num(h.roll_rate)),
            ("support", num(h.support)),
            ("forwardInput", num(h.forward_input)),
            ("rightInput", num(h.right_input)),
            ("alive", Value::Bool(h.alive)),
            ("sleeping", h.sleeping.map_or(Value::Undefined, Value::Bool)),
            ("health", num(h.health)),
            ("dying", num(h.dying)),
            ("waterClock", num(h.water_clock)),
            ("quaternion", quaternion),
            ("matrix", matrix),
            ("itemCenter", item_center),
            ("supportNormal", support_normal),
            ("legSide", leg_side),
            ("seatIndex", num(h.seat_index as f64)),
            ("driver", Value::Bool(h.driver)),
            ("wheels", wheels),
            ("seats", seats),
        ],
    )
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
