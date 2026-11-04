//! Exact trace twin for tools/vehicle-fire-geometry-twin.mjs.
use bfe_content::{json, Value};
use bfe_sim::rig::hull::{AimerRig, ByName, HullRig, Muzzle, SeatRig, WeaponRig};
use bfe_sim::vehicles::firing::{self, WeaponGeometry};
use bfe_sim::vehicles::hull::{Hull, Lease, Seat};
use bfe_sim::vehicles::pose::{Aimer, HullPose, Kick, SeatPose, WeaponPose};
use three::{NodeId, TransformNode, TransformTree, Vector3};

fn bits(n: f64) -> String {
    format!("{:016x}", n.to_bits())
}
fn flag(b: bool) -> String {
    if b { "1" } else { "0" }.into()
}
fn optional(n: Option<f64>) -> String {
    n.map(bits).unwrap_or_else(|| "-".into())
}
fn number(v: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(v.as_str().expect("number bits"), 16).unwrap())
}
fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).expect(key)
}
fn array(v: &Value) -> &[Value] {
    v.as_array().expect("array")
}
fn truth(v: &Value) -> bool {
    matches!(v, Value::Bool(true))
}
fn maybe(v: &Value) -> Option<f64> {
    (!matches!(v, Value::Null)).then(|| number(v))
}
fn point(v: &Value) -> Vector3 {
    let a = array(v);
    Vector3::new(number(&a[0]), number(&a[1]), number(&a[2]))
}
fn maybe_point(v: &Value) -> Option<Vector3> {
    (!matches!(v, Value::Null)).then(|| point(v))
}
fn vector(v: Vector3) -> [String; 3] {
    [bits(v.x), bits(v.y), bits(v.z)]
}

fn hull(c: &Value) -> Hull {
    let mut tree = TransformTree::new();
    let nodes = array(get(c, "nodes"));
    for raw in nodes {
        let a = array(raw);
        let mut node = TransformNode::placed(
            Vector3::new(number(&a[1]), number(&a[2]), number(&a[3])),
            Vector3::new(number(&a[8]), number(&a[9]), number(&a[10])),
        );
        node.set_quaternion(number(&a[4]), number(&a[5]), number(&a[6]), number(&a[7]));
        tree.add_node(node);
    }
    for (i, raw) in nodes.iter().enumerate() {
        let parent = number(&array(raw)[0]);
        if parent >= 0.0 {
            tree.add_child(parent as NodeId, i as NodeId);
        }
    }
    tree.update_matrix_world(6, true);
    let matrix = tree.node(0).matrix_world;
    let quaternion = tree.node(0).quaternion;
    let count = number(get(c, "count")) as usize;
    let bone = truth(get(c, "bone")).then_some(1);
    let turret = truth(get(c, "turret")).then_some(1);
    let rig = HullRig {
        tree,
        root: 0,
        bones: vec![1, 2, 4],
        static_bone: 0,
        markers: vec![3, 5],
        scene: 6,
        by_name: ByName::default(),
        bind_q: Vec::new(),
        bind_p: Vec::new(),
        pose_root: None,
        aimed_by_pose: false,
        seats: vec![SeatRig {
            turret,
            pivot: Vector3::default(),
            up: Vector3::new(0.0, 1.0, 0.0),
            rider: None,
            weapons: vec![WeaponRig {
                aimers: vec![AimerRig { bone, owner: -1 }],
                fire: None,
                muzzles: (0..count)
                    .map(|k| Muzzle {
                        fire_node: if k == 0 { 3 } else { 5 },
                        source_aimer: Some(0),
                        barrel: Some(if k == 0 { 2 } else { 4 }),
                        recoil: None,
                        rest: None,
                    })
                    .collect(),
            }],
        }],
    };
    let pose = HullPose {
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
        seats: vec![SeatPose {
            target: maybe_point(get(c, "target")),
            aiming: truth(get(c, "aiming")),
            aim: point(get(c, "aim")),
            turret,
            up: Vector3::new(0.0, 1.0, 0.0),
            weapons: vec![WeaponPose {
                barrel: number(get(c, "barrel")),
                aimers: vec![Aimer {
                    bone,
                    facing: point(get(c, "facing")),
                    on_target: truth(get(c, "onTarget")),
                    posed: truth(get(c, "posed")),
                    in_limits: match get(c, "limits") {
                        Value::Bool(b) => Some(*b),
                        _ => None,
                    },
                    ..Aimer::default()
                }],
                muzzles: (0..count)
                    .map(|k| Kick {
                        barrel: Some(if k == 0 { 2 } else { 4 }),
                        kick_at: maybe(&array(get(c, "kicks"))[k]),
                        ..Kick::default()
                    })
                    .collect(),
            }],
            ..SeatPose::default()
        }],
    };
    Hull {
        matrix,
        quaternion,
        center: point(get(c, "center")),
        size: point(get(c, "size")),
        flying: truth(get(c, "flying")),
        seats: vec![Seat {
            aim_yaw: number(get(c, "aimYaw")),
            pitch: number(get(c, "pitch")),
            lease: truth(get(c, "lease")).then(|| Lease {
                human: truth(get(c, "human")),
                ..Lease::default()
            }),
            ..Seat::default()
        }],
        pose: Some(pose),
        ..Hull::default()
    }
}

struct Fixture {
    k: usize,
    lines: Vec<String>,
    keys: WeaponGeometry,
    clock: f64,
    mutate: bool,
    nested: bool,
    depth: usize,
    shot_node: Option<NodeId>,
    emitter: Option<usize>,
    has_emitter: bool,
}

impl Fixture {
    fn line(&mut self, kind: &str, parts: impl IntoIterator<Item = String>) {
        let mut line = vec![kind.into(), bits(self.k as f64)];
        line.extend(parts);
        self.lines.push(line.join(" "));
    }
    fn remember(&mut self, result: firing::Muzzle) {
        self.shot_node = result.node;
        self.emitter = result.emitter;
        self.has_emitter = true;
    }
    fn node(&self) -> String {
        bits(self.shot_node.map_or(-1.0, |n| n as f64))
    }
    fn emitter(&self) -> String {
        bits(if self.has_emitter {
            self.emitter.map_or(-1.0, |n| n as f64)
        } else {
            -2.0
        })
    }
    fn clock(&mut self, hull: &mut Hull) -> f64 {
        let w = &hull.pose.as_ref().unwrap().seats[0].weapons[0];
        let mut fields = vec![bits(w.barrel)];
        fields.extend(w.muzzles.iter().map(|m| optional(m.kick_at)));
        self.line("clock", fields);
        if self.nested && self.depth == 0 {
            self.depth += 1;
            let mut out = Vector3::default();
            let keys = self.keys.clone();
            let result = hull.muzzle_of(0, 0, &keys, |h| self.clock(h), &mut out);
            self.remember(result);
            let mut fields = vector(out).to_vec();
            fields.extend([
                bits(hull.pose.as_ref().unwrap().seats[0].weapons[0].barrel),
                self.node(),
                self.emitter(),
            ]);
            self.line("nested", fields);
            self.depth -= 1;
        }
        if self.mutate {
            let p = hull.pose.as_mut().unwrap();
            p.seats[0].weapons[0].barrel += 0.25;
            p.rig.tree.node_mut(1).position.x += 0.125;
        }
        self.clock
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "vehicle-fire-geometry-twin OPS TRACE");
    let runs = json::parse(&std::fs::read_to_string(&args[1]).expect("operations read"))
        .expect("operations JSON");
    let mut lines = Vec::new();
    for (k, c) in array(&runs).iter().enumerate() {
        let mut h = hull(c);
        let keys = WeaponGeometry {
            weapon_node: truth(get(c, "weaponNode")).then_some(5),
            fallback_node: if truth(get(c, "directionNode")) {
                Some(4)
            } else if truth(get(c, "firstNode")) {
                Some(3)
            } else {
                truth(get(c, "weaponNode")).then_some(5)
            },
            direction_nodes: vec![truth(get(c, "directionNode")).then_some(4)],
            outside_limits: vec![number(get(c, "outside"))],
        };
        let mut f = Fixture {
            k,
            lines: Vec::new(),
            keys: keys.clone(),
            clock: number(get(c, "clock")),
            mutate: truth(get(c, "mutate")),
            nested: truth(get(c, "nested")),
            depth: 0,
            shot_node: None,
            emitter: None,
            has_emitter: false,
        };
        let mut out = point(get(c, "initial"));
        let mut normal = point(get(c, "normal"));
        let mut beam_from = point(get(c, "beamFrom"));
        let from = point(get(c, "from"));
        let aimer = truth(get(c, "useAimer")).then_some(0);
        let target = maybe_point(get(c, "target"));
        let fallback = maybe_point(get(c, "fallback"));
        let method = get(c, "method").as_str().unwrap();
        let mut returned = false;
        let mut answer = false;
        match method {
            "muzzleOf" | "muzzleOfSeat" => {
                let r = if method == "muzzleOf" {
                    h.muzzle_of(0, 0, &keys, |h| f.clock(h), &mut out)
                } else {
                    h.muzzle_of_seat(0, 0, &keys, |h| f.clock(h), &mut out)
                };
                returned = r.aimer.is_some();
                f.remember(r);
            }
            "heldByLimits" => answer = h.held_by_limits(0, 0, aimer, &keys),
            "reticleRay" => out = h.reticle_ray(0, &from),
            "botAim" => out = h.bot_aim(0),
            "nose" => out = h.fire_nose(),
            "shotDirection" => {
                out = h.shot_direction(
                    0,
                    0,
                    aimer,
                    &keys,
                    &from,
                    target.as_ref(),
                    fallback.as_ref(),
                )
            }
            "beamDirection" => {
                out = h.beam_direction(
                    0,
                    0,
                    truth(get(c, "beamNode")).then_some(3),
                    aimer,
                    &keys,
                    &mut beam_from,
                )
            }
            "spread" => firing::spread(
                number(get(c, "yaw")),
                number(get(c, "spreadPitch")),
                &mut out,
                &mut normal,
            ),
            "towHeld" => {
                answer = firing::tow_held(truth(get(c, "tow")), number(get(c, "towTarget")))
            }
            _ => panic!("unknown method {method}"),
        }
        let p = h.pose.as_ref().unwrap();
        let w = &p.seats[0].weapons[0];
        let mut fields = vec![method.to_owned(), flag(answer), flag(returned)];
        fields.extend(vector(out));
        fields.extend(vector(normal));
        fields.extend(vector(beam_from));
        fields.push(bits(w.barrel));
        fields.extend(w.muzzles.iter().map(|m| optional(m.kick_at)));
        fields.extend([f.node(), f.emitter()]);
        fields.extend(
            p.rig
                .tree
                .nodes
                .iter()
                .flat_map(|n| n.matrix_world.elements.map(bits)),
        );
        f.line("state", fields);
        lines.extend(f.lines);
    }
    std::fs::write(&args[2], format!("{}\n", lines.join("\n"))).expect("trace written");
}
