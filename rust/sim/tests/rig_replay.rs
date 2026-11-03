//! The rigs against recorded battles: tools/sim-rig/record.mjs records, in
//! a world-hash battle under the production server's Node, every rider pose
//! call, every droideka gun rig sample and gun read, and every hull rig as
//! built and as posed; this replays each recording in order and compares
//! every number by its bits.
//!
//!   BFE_RIG_CORPUS=DIR cargo test -p sim --features replay --test rig_replay
//!
//! DIR holds the recordings (*.rigcap). Without BFE_RIG_CORPUS the test
//! passes without replaying. The transcendental functions are rust/v8math's;
//! each call is also checked against what V8 answered in the recording
//! (rig::math's oracle), and a different answer or a call the JavaScript
//! never made fails the test.
#![cfg(feature = "replay")]

use bfe_sim::pose::Clip;
use bfe_sim::rig::droideka::{Barrel, DroidekaClips, DroidekaRig, DroidekaState, Gun};
use bfe_sim::rig::hull::{AimerDesc, BarrelDesc, HullDesc, HullRig, SeatDesc, WeaponDesc};
use bfe_sim::rig::math::oracle::{self, Oracle};
use bfe_sim::rig::rider::{rider_drop_point, rider_hit_shapes, rider_points, HitShape, RiderPoints, RiderSample, RiderScratch, RiderSkeleton, ShapeKind, ShapePart};
use bfe_sim::rig::skeleton::{BoneDesc, Skeleton};
use bfe_sim::rig::three::{Matrix4, Quaternion, Vector3};
use sim_replay::{Reader, Record, Typed, V};
use std::collections::HashMap;

fn floats(v: V) -> Vec<f64> {
    match v.typed() {
        Some(Typed::F64(x)) => x.clone(),
        other => panic!("expected a Float64Array, got {:?}", other.map(|t| t.class())),
    }
}
fn ints(v: V) -> Vec<i32> {
    match v.typed() {
        Some(Typed::I32(x)) => x.clone(),
        other => panic!("expected an Int32Array, got {:?}", other.map(|t| t.class())),
    }
}
fn opt_str(v: V) -> Option<String> {
    v.str().map(str::to_string)
}
fn v3(a: &[f64]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}
fn q4(a: &[f64]) -> Quaternion {
    Quaternion::new(a[0], a[1], a[2], a[3])
}

/// The first value two runs of numbers differ in (bits; NaN equals NaN).
fn differ(want: &[f64], got: &[f64]) -> Option<String> {
    if want.len() != got.len() {
        return Some(format!("{} values recorded, {} made", want.len(), got.len()));
    }
    for (k, (w, g)) in want.iter().zip(got).enumerate() {
        if w.to_bits() != g.to_bits() && !(w.is_nan() && g.is_nan()) {
            return Some(format!("value {k}: JavaScript {w:e} ({:#018x}), Rust {g:e} ({:#018x})", w.to_bits(), g.to_bits()));
        }
    }
    None
}

// riderPoints' out, as record.mjs pointsOf writes it.
fn points_of(p: &RiderPoints, root: bool) -> Vec<f64> {
    let mut out = Vec::with_capacity(53);
    for v in [p.pelvis, p.neck, p.head] {
        out.extend([v.x, v.y, v.z]);
    }
    for pair in [p.hips, p.knees, p.ankles, p.shoulders, p.elbows, p.hands] {
        out.extend([pair[0].x, pair[0].y, pair[0].z, pair[1].x, pair[1].y, pair[1].z]);
    }
    out.push(if p.arms { 1.0 } else { 0.0 });
    if root {
        out.extend([p.root.x, p.root.y, p.root.z, p.root_turn.x, p.root_turn.y, p.root_turn.z, p.root_turn.w]);
    } else {
        out.extend([f64::NAN; 7]);
    }
    out
}
fn points_from(a: &[f64]) -> RiderPoints {
    let pair = |k: usize| [v3(&a[k..]), v3(&a[k + 3..])];
    RiderPoints {
        pelvis: v3(a),
        neck: v3(&a[3..]),
        head: v3(&a[6..]),
        hips: pair(9),
        knees: pair(15),
        ankles: pair(21),
        shoulders: pair(27),
        elbows: pair(33),
        hands: pair(39),
        arms: a[45] != 0.0,
        root: v3(&a[46..]),
        root_turn: q4(&a[49..]),
    }
}
// Hit shapes, as record.mjs shapesOf writes them.
fn shapes_from(a: &[f64]) -> Vec<HitShape> {
    a.chunks_exact(12)
        .map(|s| HitShape {
            kind: if s[0] == 1.0 { ShapeKind::Capsule } else { ShapeKind::Sphere },
            part: if s[1] == 1.0 { ShapePart::Head } else { ShapePart::Body },
            center: v3(&s[2..]),
            radius: s[5],
            half: s[6],
            half_is_size: s[7] != 0.0,
            half_size: v3(&s[8..]),
            yaw: s[11],
        })
        .collect()
}
fn shapes_of(shapes: &[HitShape]) -> Vec<f64> {
    let mut out = Vec::new();
    for s in shapes {
        out.extend([
            if s.kind == ShapeKind::Capsule { 1.0 } else { 0.0 },
            if s.part == ShapePart::Head { 1.0 } else { 0.0 },
            s.center.x,
            s.center.y,
            s.center.z,
            s.radius,
            if s.half_is_size { f64::NAN } else { s.half },
            if s.half_is_size { 1.0 } else { 0.0 },
            s.half_size.x,
            s.half_size.y,
            s.half_size.z,
            s.yaw,
        ]);
    }
    out
}

struct Body {
    rider: RiderSkeleton,
    scratch: RiderScratch,
    bind_q: Vec<Quaternion>,
    bind_p: Vec<Vector3>,
}

struct Droideka {
    rig: DroidekaRig,
    clips: [i32; 14],
}

#[derive(Default)]
struct Tally {
    checked: HashMap<&'static str, u64>,
    failures: Vec<String>,
}

impl Tally {
    fn check(&mut self, what: &'static str, at: &str, outcome: Option<String>) {
        *self.checked.entry(what).or_default() += 1;
        if let Some(why) = outcome {
            if self.failures.len() < 40 {
                self.failures.push(format!("{what} {at}: {why}"));
            }
        }
    }
}

struct Replay {
    bodies: HashMap<String, Body>,
    clips: HashMap<i32, Clip>,
    droidekas: HashMap<i32, Droideka>,
    hulls: HashMap<i32, HullRig>,
}

fn clip_of(r: V) -> Clip {
    let data = match r.get("data").typed() {
        Some(Typed::F32(x)) => x.clone(),
        other => panic!("clip data {:?}", other.map(|t| t.class())),
    };
    Clip { frames: r.get("frames").num() as usize, fps: r.get("fps").num(), root_motion: r.get("rootMotion").bool(), tracks: ints(r.get("tracks")), data }
}

fn hull_desc(d: V) -> HullDesc {
    HullDesc {
        model: d.get("model").str().unwrap_or("").to_string(),
        flying: d.get("flying").bool(),
        creature: d.get("creature").bool(),
        bones: d
            .get("bones")
            .items()
            .map(|b| BoneDesc { name: b.get("name").str().unwrap().to_string(), parent: b.get("parent").num() as i32, local: floats(b.get("local")).try_into().unwrap() })
            .collect(),
        muzzle_tips: d.get("muzzleTips").items().map(|t| (t.get("name").str().unwrap().to_string(), v3(&floats(t.get("tip"))))).collect(),
        animated: ints(d.get("animated")).into_iter().map(|j| j as usize).collect(),
        has_clips: d.get("hasClips").bool(),
        seats: d
            .get("seats")
            .items()
            .map(|s| SeatDesc {
                turret_node_name: opt_str(s.get("turretNodeName")),
                fire_point_name: opt_str(s.get("firePointName")),
                pilot_animation: opt_str(s.get("pilotAnimation")),
                nine_pose: s.get("ninePose").bool(),
                pilot_position: opt_str(s.get("pilotPosition")),
                rider_node_hash: opt_str(s.get("riderNodeHash")),
                shootable: s.get("shootable").num(),
                pivot: v3(&floats(s.get("pivot"))),
                weapons: s
                    .get("weapons")
                    .items()
                    .map(|w| WeaponDesc {
                        fire_point_name: opt_str(w.get("firePointName")),
                        class: w.get("class").str().unwrap_or("").to_string(),
                        aimers: w
                            .get("aimers")
                            .items()
                            .map(|a| AimerDesc {
                                node: opt_str(a.get("node")),
                                fire_point_name: opt_str(a.get("firePointName")),
                                hierarchy_level: a.get("hierarchyLevel").num(),
                                barrels: a
                                    .get("barrels")
                                    .items()
                                    .map(|b| BarrelDesc {
                                        barrel_node_name: opt_str(b.get("barrelNodeName")),
                                        fire_point_name: opt_str(b.get("firePointName")),
                                        length: b.get("length").num(),
                                        recoil: b.get("recoil").opt_num(),
                                    })
                                    .collect(),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// The hull's rig compared with what the JavaScript built.
fn compare_hull(rig: &mut HullRig, r: V) -> Option<String> {
    let kinds = ints(r.get("kinds"));
    let parents = ints(r.get("parents"));
    let n = rig.tree.nodes.len();
    if kinds.len() != n {
        return Some(format!("{} nodes in the JavaScript's rig, {} in Rust's", kinds.len(), n));
    }
    let kind_of = |id: usize| {
        if id == rig.root as usize {
            0
        } else if rig.bones.contains(&(id as u32)) {
            1
        } else if id == rig.static_bone as usize {
            2
        } else {
            3
        }
    };
    for id in 0..n {
        if kind_of(id) != kinds[id] {
            return Some(format!("node {id} is of kind {} in Rust, {} in JavaScript", kind_of(id), kinds[id]));
        }
        let parent = rig.tree.node(id as u32).parent.map_or(-1, |p| p as i32);
        if parent != parents[id] {
            return Some(format!("node {id}'s parent is {parent} in Rust, {} in JavaScript", parents[id]));
        }
    }
    let locals: Vec<f64> = rig
        .tree
        .nodes
        .iter()
        .flat_map(|o| [o.position.x, o.position.y, o.position.z, o.quaternion.x, o.quaternion.y, o.quaternion.z, o.quaternion.w, o.scale.x, o.scale.y, o.scale.z])
        .collect();
    if let Some(d) = differ(&floats(r.get("locals")), &locals) {
        return Some(format!("rest locals: {d}"));
    }
    let worlds: Vec<f64> = rig.tree.nodes.iter().flat_map(|o| o.matrix_world.elements).collect();
    if let Some(d) = differ(&floats(r.get("worlds")), &worlds) {
        return Some(format!("rest world matrices: {d}"));
    }
    let ix = |id: Option<u32>| id.map_or(-1.0, |i| i as f64);
    for (s, seat) in r.get("seats").items().enumerate() {
        let mine = &rig.seats[s];
        let mut want = vec![seat.get("turret").num()];
        want.extend(floats(seat.get("pivot")));
        want.extend(floats(seat.get("up")));
        let mut got = vec![ix(mine.turret), mine.pivot.x, mine.pivot.y, mine.pivot.z, mine.up.x, mine.up.y, mine.up.z];
        let rider = seat.get("rider");
        match (&mine.rider, rider.is_null()) {
            (None, true) => {}
            (Some(m), false) => {
                if m.anim != rider.get("anim").str().unwrap() || m.clip != rider.get("clip").str().unwrap() {
                    return Some(format!("seat {s}'s rider clip: {} in Rust, {} in JavaScript", m.clip, rider.get("clip").str().unwrap()));
                }
                want.extend([rider.get("ninePose").bool() as u8 as f64, rider.get("node").num(), rider.get("shootable").bool() as u8 as f64]);
                got.extend([m.nine_pose as u8 as f64, ix(m.node), m.shootable as u8 as f64]);
            }
            _ => return Some(format!("seat {s}'s rider: Rust {}, JavaScript {}", mine.rider.is_some(), !rider.is_null())),
        }
        for (w, weapon) in seat.get("weapons").items().enumerate() {
            let mw = &mine.weapons[w];
            want.push(weapon.get("fire").num());
            got.push(ix(mw.fire));
            for (a, aimer) in weapon.get("aimers").items().enumerate() {
                want.extend([aimer.get("bone").num(), aimer.get("owner").num()]);
                got.extend([ix(mw.aimers[a].bone), mw.aimers[a].owner as f64]);
            }
            let muzzles: Vec<V> = weapon.get("muzzles").items().collect();
            want.push(muzzles.len() as f64);
            got.push(mw.muzzles.len() as f64);
            for (m, muzzle) in muzzles.iter().enumerate() {
                let Some(mm) = mw.muzzles.get(m) else { break };
                want.extend([muzzle.get("fireNode").num(), muzzle.get("sourceAimer").num(), muzzle.get("barrel").num(), muzzle.get("recoil").opt_num().unwrap_or(f64::NAN)]);
                got.extend([mm.fire_node as f64, mm.source_aimer.map_or(-1.0, |a| a as f64), ix(mm.barrel), mm.recoil.unwrap_or(f64::NAN)]);
                let rest = muzzle.get("rest");
                if rest.is_null() {
                    want.extend([f64::NAN; 3]);
                } else {
                    want.extend(floats(rest));
                }
                match mm.rest {
                    Some(p) => got.extend([p.x, p.y, p.z]),
                    None => got.extend([f64::NAN; 3]),
                }
            }
        }
        if let Some(d) = differ(&want, &got) {
            return Some(format!("seat {s} (turret, pivot, up, rider, weapons' fire nodes, aimers and muzzles): {d}\n  JavaScript {want:?}\n  Rust {got:?}"));
        }
    }
    if rig.aimed_by_pose != r.get("aimedByPose").bool() {
        return Some(format!("aimedByPose: Rust {}, JavaScript {}", rig.aimed_by_pose, r.get("aimedByPose").bool()));
    }
    None
}

fn replay(name: &str, records: &[Record], tally: &mut Tally) {
    let mut s = Replay { bodies: HashMap::new(), clips: HashMap::new(), droidekas: HashMap::new(), hulls: HashMap::new() };
    for record in records {
        let r = record.root();
        let at = format!("{name} tick {}", record.tick);
        match record.name.as_str() {
            "rig.skeleton" => {
                let key = r.get("key").str().unwrap().to_string();
                let descs: Vec<BoneDesc> = r
                    .get("bones")
                    .items()
                    .map(|b| BoneDesc { name: b.get("name").str().unwrap().to_string(), parent: b.get("parent").num() as i32, local: floats(b.get("local")).try_into().unwrap() })
                    .collect();
                let skeleton = Skeleton::actor(&descs);
                let bind: Vec<f64> = skeleton
                    .bind
                    .iter()
                    .flat_map(|b| [b.position.x, b.position.y, b.position.z, b.quaternion.x, b.quaternion.y, b.quaternion.z, b.quaternion.w, b.scale.x, b.scale.y, b.scale.z])
                    .collect();
                tally.check("skeleton bind pose", &format!("{at} {key}"), differ(&floats(r.get("bind")), &bind));
                let rider = RiderSkeleton::new(&skeleton);
                let scratch = RiderScratch::new(&rider);
                let bind_q = skeleton.bind.iter().map(|b| b.quaternion).collect();
                let bind_p = skeleton.bind.iter().map(|b| b.position).collect();
                s.bodies.insert(key, Body { rider, scratch, bind_q, bind_p });
            }
            "rig.clip" => {
                s.clips.insert(r.get("id").num() as i32, clip_of(r));
            }
            "rig.rider" => {
                let body = s.bodies.get_mut(r.get("skeleton").str().unwrap()).expect("the rider's skeleton");
                let clip = &s.clips[&(r.get("clip").num() as i32)];
                let (bone_q, bone_p) = if r.get("posed").is_null() {
                    (body.bind_q.clone(), body.bind_p.clone())
                } else {
                    let posed = floats(r.get("posed"));
                    (posed.chunks_exact(7).map(q4).collect(), posed.chunks_exact(7).map(|c| v3(&c[4..])).collect())
                };
                let time = r.get("time");
                let sample = RiderSample {
                    clip,
                    pitch: r.get("pitch").opt_num().unwrap_or(f64::NAN),
                    turn: r.get("turn").opt_num().unwrap_or(f64::NAN),
                    time: if time.is_nullish() { None } else { Some(time.num()) },
                };
                assert!(r.get("shoulders").bool(), "a rider's points without shoulders");
                let root = r.get("root").bool();
                assert_eq!(root, r.get("rootTurn").bool(), "root without rootTurn");
                let mut out = points_from(&floats(r.get("before")));
                let result = rider_points(&body.rider, &bone_q, &bone_p, &sample, &mut body.scratch, &mut out, root);
                let outcome = if result != r.get("result").bool() {
                    Some(format!("riderPoints answered {result} in Rust, {} in JavaScript", r.get("result").bool()))
                } else {
                    differ(&floats(r.get("after")), &points_of(&out, root))
                };
                tally.check("riderPoints", &at, outcome);
            }
            "rig.hitShapes" => {
                let points = points_from(&floats(r.get("points")));
                let mut frame = Matrix4::default();
                frame.from_array(&floats(r.get("frame")), 0);
                let scale = r.get("scale").opt_num();
                let mut shapes = shapes_from(&floats(r.get("before")));
                let count = rider_hit_shapes(&points, &frame, scale, &mut shapes);
                let outcome = if count as f64 != r.get("count").num() {
                    Some(format!("{count} shapes in Rust, {} in JavaScript", r.get("count").num()))
                } else {
                    differ(&floats(r.get("after")), &shapes_of(&shapes))
                };
                tally.check("riderHitShapes", &at, outcome);
            }
            "rig.drop" => {
                let points = points_from(&floats(r.get("points")));
                let mut out = Vector3::default();
                rider_drop_point(&points, &mut out);
                tally.check("riderDropPoint", &at, differ(&floats(r.get("out")), &[out.x, out.y, out.z]));
            }
            "rig.droideka.arm" => {
                let k = r.get("skeleton");
                let names: Vec<String> = k.get("names").items().map(|n| n.str().unwrap().to_string()).collect();
                let bind: Vec<(Vector3, Quaternion, Vector3)> = floats(k.get("bind")).chunks_exact(10).map(|c| (v3(c), q4(&c[3..]), v3(&c[7..]))).collect();
                let skeleton = Skeleton::from_bind(names, ints(k.get("parents")), &bind, false);
                let guns: Vec<Gun> = r
                    .get("guns")
                    .items()
                    .map(|g| Gun {
                        bone: g.get("bone").num() as usize,
                        barrels: g.get("barrels").items().map(|b| if b.is_null() { Barrel::GunBone } else { Barrel::Marker(v3(&floats(b))) }).collect(),
                    })
                    .collect();
                let rig = DroidekaRig::new(&skeleton, &guns, v3(&floats(r.get("rollCenter"))));
                let clips: [i32; 14] = ints(r.get("clips")).try_into().unwrap();
                s.droidekas.insert(r.get("id").num() as i32, Droideka { rig, clips });
            }
            "rig.droideka.reset" => {
                let d = s.droidekas.get_mut(&(r.get("id").num() as i32)).unwrap();
                d.rig.reset(r.get("yaw").num(), r.get("roll").num());
            }
            "rig.droideka.sample" => {
                let d = s.droidekas.get_mut(&(r.get("id").num() as i32)).unwrap();
                let st = floats(r.get("state"));
                let state = DroidekaState {
                    mode: st[0] as i32,
                    time: st[1],
                    dead: st[2] != 0.0,
                    legs: st[3],
                    aim_pitch: st[4],
                    aim_yaw: st[5],
                    roll: st[6],
                    lean: st[7],
                    speed: st[8],
                    position: v3(&st[9..]),
                };
                let mut by_mode = [None; 14];
                for (m, &id) in d.clips.iter().enumerate() {
                    if id >= 0 {
                        by_mode[m] = s.clips.get(&id);
                    }
                }
                let clips = DroidekaClips { by_mode };
                d.rig.sample_muzzles(r.get("dt").num(), &state, &clips);
                let mut barrels = Vec::new();
                for gun in &d.rig.barrels {
                    for &b in gun {
                        let e = &d.rig.tree.node(b).matrix_world.elements;
                        barrels.extend([e[12], e[13], e[14]]);
                    }
                }
                let mut outcome = differ(&floats(r.get("barrels")), &barrels).map(|d| format!("barrels: {d}"));
                if outcome.is_none() && r.has("bones") {
                    let bones: Vec<f64> = d
                        .rig
                        .bones
                        .iter()
                        .flat_map(|&b| {
                            let o = d.rig.tree.node(b);
                            [o.position.x, o.position.y, o.position.z, o.quaternion.x, o.quaternion.y, o.quaternion.z, o.quaternion.w, o.scale.x, o.scale.y, o.scale.z]
                        })
                        .collect();
                    outcome = differ(&floats(r.get("bones")), &bones).map(|d| format!("bones: {d}"));
                    let a = &d.rig.animator;
                    let mine = [a.mode as f64, a.went_up_rolled as u8 as f64, a.transition, a.roll_origin, a.facing, a.legs];
                    outcome = outcome.or_else(|| differ(&floats(r.get("animator")), &mine).map(|d| format!("animator: {d}")));
                }
                tally.check("droideka sampleMuzzles", &format!("{at} droideka {}", r.get("id").num()), outcome);
            }
            "rig.droideka.gun" => {
                let d = s.droidekas.get_mut(&(r.get("id").num() as i32)).unwrap();
                let (index, barrel) = (r.get("index").num(), r.get("barrel").num());
                let got = if index < 0.0 || barrel < 0.0 { None } else { d.rig.gun_at(index as usize, barrel as usize) };
                let want = r.get("result");
                let outcome = match (got, want.is_null()) {
                    (None, true) => None,
                    (Some(p), false) => differ(&floats(want), &[p.x, p.y, p.z]),
                    _ => Some(format!("gunAt answered {} in Rust, {} in JavaScript", got.is_some(), !want.is_null())),
                };
                tally.check("droideka gunAt", &at, outcome);
            }
            "rig.hull" => {
                let desc = hull_desc(r.get("desc"));
                let mut rig = HullRig::new(&desc);
                let outcome = compare_hull(&mut rig, r.get("rig"));
                tally.check("hull rig", &format!("{at} {}", r.get("class").str().unwrap_or("?")), outcome);
                s.hulls.insert(r.get("id").num() as i32, rig);
            }
            "rig.hull.pose" => {
                let rig = s.hulls.get_mut(&(r.get("id").num() as i32)).unwrap();
                let locals = floats(r.get("locals"));
                let outcome = if locals.len() != rig.tree.nodes.len() * 10 {
                    Some(format!("{} nodes posed, {} in the rig", locals.len() / 10, rig.tree.nodes.len()))
                } else {
                    for (i, c) in locals.chunks_exact(10).enumerate() {
                        let node = rig.tree.node_mut(i as u32);
                        node.position = v3(c);
                        node.quaternion = q4(&c[3..]);
                        node.scale = v3(&c[7..]);
                    }
                    rig.tree.update_matrix_world(rig.root, true);
                    let worlds: Vec<f64> = rig.tree.nodes.iter().flat_map(|o| o.matrix_world.elements).collect();
                    differ(&floats(r.get("worlds")), &worlds)
                };
                tally.check("hull pose world matrices", &at, outcome);
            }
            _ => {}
        }
    }
}

#[test]
fn rigs_match_recorded_battles() {
    let Ok(dir) = std::env::var("BFE_RIG_CORPUS") else {
        eprintln!("BFE_RIG_CORPUS unset: nothing replayed");
        return;
    };
    let mut files: Vec<_> = std::fs::read_dir(&dir).expect("the corpus").filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "rigcap")).collect();
    files.sort();
    assert!(!files.is_empty(), "no recordings in {dir}");
    let mut tally = Tally::default();
    let (mut answered, mut misses, mut mismatches) = (0u64, Vec::new(), Vec::new());
    for file in &files {
        let bytes = std::fs::read(file).expect("a recording");
        let mut reader = Reader::new(&bytes).expect("a BFECAP file");
        let mut records = Vec::new();
        while let Some(r) = reader.next_record() {
            records.push(r.expect("a record"));
        }
        let mut table = Oracle::new();
        for r in &records {
            if r.name == "rig.oracle" {
                table.insert_table(&floats(r.root().get("table")));
            }
        }
        let name = file.file_stem().unwrap().to_string_lossy().to_string();
        let ((), table) = oracle::with(table, || replay(&name, &records, &mut tally));
        answered += table.answered;
        misses.extend(table.misses.iter().map(|&(f, a, b)| format!("{name}: {}({a:e}, {b:e})", f.name())));
        mismatches.extend(table.mismatches.iter().map(|&(f, a, b, v8, mine)| format!("{name}: {}({a:e}, {b:e}): V8 {v8:e}, v8math {mine:e}", f.name())));
    }
    let mut kinds: Vec<_> = tally.checked.iter().collect();
    kinds.sort();
    eprintln!(
        "{} recordings: {}; transcendental calls: {answered} as V8 answered, {} misses, {} mismatches",
        files.len(),
        kinds.iter().map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(", "),
        misses.len(),
        mismatches.len()
    );
    for m in misses.iter().take(5) {
        eprintln!("miss: {m}");
    }
    for m in mismatches.iter().take(5) {
        eprintln!("mismatch: {m}");
    }
    assert!(tally.failures.is_empty(), "{} differences:\n{}", tally.failures.len(), tally.failures.join("\n"));
    assert!(misses.is_empty(), "{} transcendental calls the JavaScript never made", misses.len());
    assert!(mismatches.is_empty(), "{} transcendental answers not V8's", mismatches.len());
}
