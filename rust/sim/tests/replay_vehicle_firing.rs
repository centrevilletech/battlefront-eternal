//! Real Vehicle geometry frames. Only the foreign clock return is served;
//! the native core computes barrel/kick changes and every matrix/vector.
#![cfg(feature = "replay")]
use bfe_sim::gen::{Replay, VgState};
use bfe_sim::rig::hull::{AimerRig, ByName, HullRig, Muzzle, SeatRig, WeaponRig};
use bfe_sim::vehicles::firing::{self, WeaponGeometry};
use bfe_sim::vehicles::hull::{Hull, Lease, Seat};
use bfe_sim::vehicles::pose::{Aimer, HullPose, Kick, SeatPose, WeaponPose};
use sim_replay::{
    check_bool, check_num, check_value, check_vec3, replay_corpus, Arena, Corpus, Frame, Log,
    Outcome, V,
};
use three::{Matrix4, NodeId, Quaternion, TransformNode, TransformTree, Vector3};

fn vector(v: [f64; 3]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2])
}
fn read(v: V) -> Vector3 {
    vector(v.vec3().expect("a geometry vector"))
}
fn optional(v: V) -> Option<Vector3> {
    (!v.is_nullish()).then(|| read(v))
}
fn array(v: Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}
fn node(n: f64) -> Option<NodeId> {
    (n >= 0.0).then_some(n as NodeId)
}
fn word(n: Option<usize>) -> f64 {
    n.map_or(-1.0, |n| n as f64)
}
fn next(n: f64) -> f64 {
    f64::from_bits(n.to_bits() + 1)
}

fn hull(state: &VgState, parents: V) -> Hull {
    let mut tree = TransformTree::new();
    for r in &state.nodes {
        let mut n = TransformNode::placed(vector(r.position), vector(r.scale));
        n.set_quaternion(
            r.quaternion[0],
            r.quaternion[1],
            r.quaternion[2],
            r.quaternion[3],
        );
        n.matrix.elements = r.matrix;
        n.matrix_world.elements = r.matrix_world;
        n.matrix_auto_update = r.auto;
        n.matrix_world_auto_update = r.world_auto;
        n.matrix_world_needs_update = r.needs;
        tree.add_node(n);
    }
    for (i, p) in parents.items().enumerate() {
        if let Some(parent) = node(p.num()) {
            tree.add_child(parent, i as NodeId);
        }
    }
    let rig = HullRig {
        scene: state.nodes.len() as NodeId - 1,
        tree,
        root: 0,
        bones: Vec::new(),
        static_bone: 0,
        markers: Vec::new(),
        by_name: ByName::default(),
        bind_q: Vec::new(),
        bind_p: Vec::new(),
        pose_root: None,
        aimed_by_pose: false,
        seats: state
            .seats
            .iter()
            .map(|s| SeatRig {
                turret: node(s.turret),
                pivot: Vector3::default(),
                up: Vector3::new(0.0, 1.0, 0.0),
                rider: None,
                weapons: s
                    .weapons
                    .iter()
                    .map(|w| WeaponRig {
                        fire: None,
                        aimers: w
                            .aimers
                            .iter()
                            .map(|a| AimerRig {
                                bone: node(a.bone),
                                owner: -1,
                            })
                            .collect(),
                        muzzles: w
                            .muzzles
                            .iter()
                            .map(|m| Muzzle {
                                fire_node: node(m.node).expect("a real muzzle fire node"),
                                source_aimer: node(m.aimer).map(|n| n as usize),
                                barrel: node(m.barrel),
                                recoil: None,
                                rest: None,
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    };
    let pose = HullPose {
        work: Default::default(),
        rig,
        visual: Vector3::default(),
        age: 0.0,
        hover_phase: 0.0,
        walk_phase: 0.0,
        walk_blend: 0.0,
        has_pose: false,
        legs: false,
        wheel_bones: Vec::new(),
        clips: None,
        leg_pairs: None,
        seats: state
            .seats
            .iter()
            .map(|s| SeatPose {
                target: s.target.map(vector),
                turret: node(s.turret),
                aiming: s.aiming,
                aim: vector(s.aim),
                weapons: s
                    .weapons
                    .iter()
                    .map(|w| WeaponPose {
                        barrel: w.barrel,
                        aimers: w
                            .aimers
                            .iter()
                            .map(|a| Aimer {
                                bone: node(a.bone),
                                facing: vector(a.facing),
                                in_limits: a.in_limits,
                                on_target: a.on_target,
                                posed: a.posed,
                                ..Aimer::default()
                            })
                            .collect(),
                        muzzles: w
                            .muzzles
                            .iter()
                            .map(|m| Kick {
                                barrel: node(m.barrel),
                                kick_at: m.kick_at,
                                ..Kick::default()
                            })
                            .collect(),
                    })
                    .collect(),
                ..SeatPose::default()
            })
            .collect(),
    };
    Hull {
        flying: state.flying,
        center: vector(state.center),
        size: vector(state.size),
        matrix: Matrix4 {
            elements: state.matrix,
        },
        quaternion: Quaternion::new(
            state.quaternion[0],
            state.quaternion[1],
            state.quaternion[2],
            state.quaternion[3],
        ),
        seat_index: state.seat_index as usize,
        seats: state
            .seats
            .iter()
            .map(|s| Seat {
                aim_yaw: s.yaw,
                pitch: s.pitch,
                lease: s.leased.then_some(Lease {
                    human: s.human,
                    ..Lease::default()
                }),
                ..Seat::default()
            })
            .collect(),
        pose: Some(pose),
        ..Hull::default()
    }
}

fn publish(hull: &Hull, state: &mut VgState) {
    let p = hull.pose.as_ref().unwrap();
    for (s, sp) in state.seats.iter_mut().zip(&p.seats) {
        for (w, wp) in s.weapons.iter_mut().zip(&sp.weapons) {
            w.barrel = wp.barrel;
            for (m, mp) in w.muzzles.iter_mut().zip(&wp.muzzles) {
                m.kick_at = mp.kick_at;
            }
        }
    }
    for (r, n) in state.nodes.iter_mut().zip(&p.rig.tree.nodes) {
        r.matrix = n.matrix.elements;
        r.matrix_world = n.matrix_world.elements;
        r.needs = n.matrix_world_needs_update;
    }
}

#[derive(Clone, Copy)]
enum Mutation {
    None,
    Barrel,
    Clock,
}
fn replay(frame: &Frame, mutation: Mutation) -> Result<(), String> {
    let b = frame.before();
    let after = frame.after();
    let mut cx = Replay::default();
    let mut state = VgState::from_value(b.get("state"), &mut cx, "geometry")?;
    let mut h = hull(&state, b.get("parents"));
    let (seat, weapon) = (b.get("seat").num() as usize, b.get("weapon").num() as usize);
    let aimer = node(b.get("aimer").num()).map(|n| n as usize);
    let keys = state.seats[seat]
        .weapons
        .get(weapon)
        .map(|w| WeaponGeometry {
            weapon_node: node(w.weapon_node),
            fallback_node: node(w.fallback_node),
            direction_nodes: w.aimers.iter().map(|a| node(a.direction_node)).collect(),
            outside_limits: w.aimers.iter().map(|a| a.outside).collect(),
        })
        .unwrap_or_default();
    let mut out = read(b.get("out"));
    let mut normal = read(b.get("normal"));
    let mut beam = read(b.get("beamFrom"));
    let from = read(b.get("from"));
    let target = optional(b.get("target"));
    let fallback = optional(b.get("fallback"));
    let mut muzzle = firing::Muzzle::default();
    let mut flag = false;
    let mut log = Log::new(frame);
    let mut error = None;
    if matches!(mutation, Mutation::Barrel) {
        h.pose.as_mut().unwrap().seats[seat].weapons[weapon].barrel += 1.0;
    }
    let mut clock = |h: &mut Hull| {
        let call = match log.call("geometry.clock") {
            Ok(c) => c,
            Err(e) => {
                error.get_or_insert_with(|| e.to_string());
                return 0.0;
            }
        };
        if call.args().len() != 0 {
            error.get_or_insert_with(|| "clock arguments".into());
        }
        let visible = call
            .call
            .root()
            .get("s")
            .get("hulls")
            .at(b.get("hull").num() as usize);
        for (i, s) in h.pose.as_ref().unwrap().seats.iter().enumerate() {
            for (j, w) in s.weapons.iter().enumerate() {
                let expected = visible.at(i).at(j);
                if let Err(e) = check_num("clock barrel", expected.get("barrel"), w.barrel) {
                    error.get_or_insert_with(|| e.to_string());
                }
                for (k, m) in w.muzzles.iter().enumerate() {
                    let got = expected.get("kicks").at(k);
                    if let Some(time) = m.kick_at {
                        if let Err(e) = check_num("clock kick", got, time) {
                            error.get_or_insert_with(|| e.to_string());
                        }
                    } else if !got.is_undefined() {
                        error.get_or_insert_with(|| "clock kick must be undefined".into());
                    }
                }
            }
        }
        let time = call.result().num();
        if matches!(mutation, Mutation::Clock) {
            next(time)
        } else {
            time
        }
    };
    let method = b.get("method").str().unwrap();
    match method {
        "muzzleOf" => muzzle = h.muzzle_of(seat, weapon, &keys, &mut clock, &mut out),
        "muzzleOfSeat" => muzzle = h.muzzle_of_seat(seat, weapon, &keys, &mut clock, &mut out),
        "heldByLimits" => flag = h.held_by_limits(seat, weapon, aimer, &keys),
        "reticleRay" => out = h.reticle_ray(seat, &from),
        "botAim" => out = h.bot_aim(seat),
        "nose" => out = h.fire_nose(),
        "shotDirection" => {
            out = h.shot_direction(
                seat,
                weapon,
                aimer,
                &keys,
                &from,
                target.as_ref(),
                fallback.as_ref(),
            )
        }
        "beamDirection" => {
            out = h.beam_direction(
                seat,
                weapon,
                node(b.get("node").num()),
                aimer,
                &keys,
                &mut beam,
            )
        }
        "spread" => firing::spread(
            b.get("yaw").num(),
            b.get("pitch").num(),
            &mut out,
            &mut normal,
        ),
        "towHeld" => flag = firing::tow_held(b.get("tow").bool(), b.get("towTarget").num()),
        _ => return Err(format!("unknown geometry {method}")),
    }
    drop(clock);
    log.finish().map_err(|e| e.to_string())?;
    if let Some(error) = error {
        return Err(error);
    }
    let check = |r: Result<(), sim_replay::Mismatch>| r.map_err(|e| e.to_string());
    check(check_vec3("out", after.get("out"), array(out)))?;
    check(check_vec3("normal", after.get("normal"), array(normal)))?;
    check(check_vec3("beamFrom", after.get("beamFrom"), array(beam)))?;
    match method {
        "muzzleOf" | "muzzleOfSeat" => {
            check(check_num(
                "shot node",
                after.get("shotNode"),
                word(muzzle.node.map(|n| n as usize)),
            ))?;
            let emitters = b.get("emitters");
            let emitted = if let Some(k) = muzzle.emitter {
                emitters.get("muzzles").at(k)
            } else if state.seats[seat].weapons[weapon].muzzles.is_empty() {
                emitters.get("first")
            } else {
                emitters.get("weapon")
            };
            check(check_num(
                "shot emitter",
                after.get("shotEmitter"),
                emitted.num(),
            ))?;
            if let Some(aimer) = muzzle.aimer {
                check(check_value(
                    "returned aimer",
                    frame.result(),
                    after.get("aimers").at(aimer),
                ))?;
            } else if !frame.result().is_null() {
                return Err("returned aimer is not null".into());
            }
        }
        "heldByLimits" | "towHeld" => check(check_bool("return", frame.result(), flag))?,
        "spread" => {
            if !frame.result().is_undefined() {
                return Err("spread return".into());
            }
        }
        _ => check(check_vec3("return", frame.result(), array(out)))?,
    }
    publish(&h, &mut state);
    let mut arena = Arena::default();
    let result = state.to_value(&mut cx, &mut arena);
    check(check_value(
        "geometry state",
        after.get("state"),
        arena.view(&result),
    ))?;
    Ok(())
}
fn report(method: &str, mutation: Mutation) -> Option<sim_replay::Report> {
    let corpus = Corpus::from_env().unwrap()?;
    Some(
        replay_corpus(&corpus, method, |_, frame| match replay(frame, mutation) {
            Ok(()) => Outcome::Same,
            Err(e) => Outcome::Failed(e),
        })
        .unwrap(),
    )
}
#[test]
fn real_vehicle_firing_geometry_replays_exactly() {
    let mut count = 0;
    for name in [
        "muzzleOf",
        "muzzleOfSeat",
        "heldByLimits",
        "reticleRay",
        "botAim",
        "nose",
        "shotDirection",
        "beamDirection",
        "spread",
        "towHeld",
    ] {
        let Some(report) = report(&format!("Vehicle.{name}"), Mutation::None) else {
            return;
        };
        println!("{}", report.summary());
        count += report.replayed();
        report.assert_clean();
    }
    assert!(
        count > 0,
        "the corpus contains actual vehicle geometry calls"
    );
}
#[test]
fn vehicle_geometry_replay_detects_changed_barrel_and_clock() {
    for mutation in [Mutation::Barrel, Mutation::Clock] {
        let Some(report) = report("Vehicle.muzzleOf", mutation) else {
            return;
        };
        println!(
            "geometry mutation: {} of {} frames differ",
            report.failures.len(),
            report.replayed()
        );
        assert!(!report.failures.is_empty(), "mutation exercised");
    }
}
