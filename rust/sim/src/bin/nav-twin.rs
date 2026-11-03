//! The Rust side of the navigation twin gate: replays the operations
//! tools/nav-twin.mjs recorded against the JavaScript (planner
//! requests and services, executor ticks, graph and barrier queries,
//! cursor routes, analogs, goto helpers, hints) on the Rust port and
//! writes the same trace, one JSON line an operation, every f64 as
//! its bit pattern. A byte-for-byte equal trace is the gate.
//!
//!   nav-twin NAV.json NAV.bin|- OPS.json TRACE.out
//!
//! The fake world the goto helpers run against is the tools' world:
//! its units table resolves follow targets and leaders, its time is
//! the operations' clock, and its clear-above answers are scripted by
//! operation (the JavaScript side scripts the same answers).

use bfe_content::json;
use bfe_content::Value;
use bfe_sim::ai::nav::analogs::Intent;
use bfe_sim::ai::nav::barriers;
use bfe_sim::ai::nav::cursor::GraphCursor;
use bfe_sim::ai::nav::goto::{FlyGoto, GotoMode, GotoWorld, GroundGoto, Resolved};
use bfe_sim::ai::nav::hints;
use bfe_sim::ai::nav::jet_hints;
use bfe_sim::ai::nav::math::Vec3;
use bfe_sim::ai::nav::units::{Air, ClassDef, JetView, NavUnit, Posture, SizeType, SoldierView, UnitRef};
use bfe_sim::ai::nav::{Nav, PlannerOptions};
use bfe_sim::ai::random::ClassicRandom;
use std::collections::HashMap;
use std::io::Write;

fn num(v: Option<&Value>) -> f64 {
    v.and_then(|v| v.as_f64()).unwrap_or(f64::NAN)
}
fn opt_num(v: Option<&Value>) -> Option<f64> {
    v.and_then(|v| v.as_f64())
}
fn boolean(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Bool(true)))
}
fn vec3(v: Option<&Value>) -> Vec3 {
    Vec3::new(num(v.and_then(|v| v.at(0))), num(v.and_then(|v| v.at(1))), num(v.and_then(|v| v.at(2))))
}
fn opt_vec3(v: Option<&Value>) -> Option<Vec3> {
    v.filter(|v| !v.is_null()).map(|v| vec3(Some(v)))
}

fn bits(x: f64) -> String {
    format!("\"x{:016x}\"", x.to_bits())
}
fn v3(v: Vec3) -> String {
    format!("[{},{},{}]", bits(v.x), bits(v.y), bits(v.z))
}
fn opt_index(v: Option<usize>) -> String {
    v.map(|i| i.to_string()).unwrap_or_else(|| "null".into())
}

fn unit_of(v: &Value) -> NavUnit {
    let class_def = v.get("classDef").filter(|d| !d.is_null()).map(|d| {
        let ai_size_type = match d.get("AISizeType") {
            Some(Value::String(s)) => SizeType::Text(s.to_string()),
            Some(v) => match v.as_f64() {
                Some(n) => SizeType::Number(n),
                None => SizeType::Missing,
            },
            None => SizeType::Missing,
        };
        let bounds = d.get("bounds").and_then(|b| b.as_array()).filter(|b| b.len() == 2).map(|b| {
            let row = |i: usize| {
                let r = b[i].as_array();
                [
                    r.and_then(|r| r.first()).map(|x| num(Some(x))).unwrap_or(f64::NAN),
                    r.and_then(|r| r.get(1)).map(|x| num(Some(x))).unwrap_or(f64::NAN),
                    r.and_then(|r| r.get(2)).map(|x| num(Some(x))).unwrap_or(f64::NAN),
                ]
            };
            [row(0), row(1)]
        });
        ClassDef {
            ai_size_type,
            bounds,
            max_speed: opt_num(d.get("MaxSpeed")),
            max_strafe_speed: opt_num(d.get("MaxStrafeSpeed")),
            max_turn_speed: opt_num(d.get("MaxTurnSpeed")),
            max_pitch_speed: opt_num(d.get("MaxPitchSpeed")),
            forward_speed: opt_num(d.get("ForwardSpeed")),
            turn_rate: opt_num(d.get("TurnRate")),
            jet_fuel_min_border: opt_num(d.get("JetFuelMinBorder")),
            jet_fuel_recharge_rate: opt_num(d.get("JetFuelRechargeRate")),
        }
    });
    let soldier = v.get("soldier").filter(|s| !s.is_null()).map(|s| SoldierView {
        aim_yaw: opt_num(s.get("aimYaw")),
        aim_pitch: opt_num(s.get("aimPitch")),
        max_speed: opt_num(s.get("maxSpeed")),
        max_strafe: opt_num(s.get("maxStrafe")),
        max_turn: opt_num(s.get("maxTurn")),
        max_pitch: opt_num(s.get("maxPitch")),
        avoidance: opt_vec3(s.get("avoidance")),
        fuel: num(s.get("fuel")),
        jet: s.get("jet").filter(|j| !j.is_null()).map(|j| JetView { min_border: opt_num(j.get("minBorder")), recharge: opt_num(j.get("recharge")) }),
        air: match s.get("air").and_then(|a| a.as_str()) {
            None => Air::None,
            Some("jump") => Air::Jump,
            Some("jet") => Air::Jet,
            Some(_) => Air::Other,
        },
        posture: match s.get("posture").and_then(|p| p.as_str()) {
            Some("crouch") => Posture::Crouch,
            Some("prone") => Posture::Prone,
            Some("stand") => Posture::Stand,
            _ => Posture::Other,
        },
        lock: num(s.get("lock")),
        dive: boolean(s.get("dive")),
    });
    NavUnit {
        id: num(v.get("id")) as u64,
        kind: num(v.get("kind")),
        position: vec3(v.get("position")),
        velocity: vec3(v.get("velocity")),
        generation: opt_num(v.get("generation")),
        alive_false: matches!(v.get("alive"), Some(Value::Bool(false))),
        nav_mask: opt_num(v.get("navMask")),
        nav_radius: opt_num(v.get("navRadius")),
        class_def,
        soldier,
        forward: opt_vec3(v.get("forward")),
        origin: opt_vec3(v.get("origin")),
    }
}

struct World {
    time: f64,
    a: ClassicRandom,
    b: ClassicRandom,
    units: Vec<NavUnit>,
    clear_above: Vec<bool>,
    remembered: HashMap<u64, Vec3>,
    far: f64,
}

impl GotoWorld for World {
    fn time(&self) -> f64 {
        self.time
    }
    fn random_a(&mut self) -> &mut ClassicRandom {
        &mut self.a
    }
    fn random_b(&mut self) -> &mut ClassicRandom {
        &mut self.b
    }
    fn resolve(&mut self, r: UnitRef) -> Option<Resolved> {
        bfe_sim::ai::nav::goto::resolve_ref(r, self.units.iter().find(|u| u.id == r.unit))
    }
    fn leader(&mut self, r: UnitRef) -> Option<NavUnit> {
        // control.js:259: alive (truthy, so a missing alive does not
        // count) and of the ref's generation. The twin's units always
        // carry alive, so alive_false being false is alive here.
        let u = self.units.iter().find(|u| u.id == r.unit)?;
        if u.alive_false {
            return None;
        }
        match r.generation {
            Some(g) if u.generation != Some(g) => None,
            _ => Some(u.clone()),
        }
    }
    fn remembered(&mut self, target: u64) -> Option<Vec3> {
        self.remembered.get(&target).copied()
    }
    fn clear_above(&mut self, _p: Vec3, _unit: &NavUnit) -> bool {
        if self.clear_above.is_empty() {
            true
        } else {
            self.clear_above.remove(0)
        }
    }
    fn follow_far(&mut self) -> f64 {
        self.far
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (nav_json_path, nav_bin_path, ops_path, out_path) = (&args[0], &args[1], &args[2], &args[3]);
    let nav_json = json::parse_bytes(&std::fs::read(nav_json_path).expect("nav json")).expect("parse nav json");
    let bin = if nav_bin_path == "-" { None } else { Some(std::fs::read(nav_bin_path).expect("nav bin")) };
    let ops = json::parse_bytes(&std::fs::read(ops_path).expect("ops")).expect("parse ops");
    let mut nav = Nav::new(&nav_json, bin.as_deref(), None, None, PlannerOptions::default()).expect("load nav");
    let mut world = World {
        time: 0.0,
        a: ClassicRandom::from_state(0),
        b: ClassicRandom::from_state(0),
        units: ops.get("units").and_then(|u| u.as_array()).map(|u| u.iter().map(unit_of).collect()).unwrap_or_default(),
        clear_above: Vec::new(),
        remembered: HashMap::new(),
        far: 3.0,
    };
    let mut controls: HashMap<u64, usize> = HashMap::new();
    let mut battle = Battle::default();
    let mut cursors: HashMap<String, GraphCursor> = HashMap::new();
    let mut grounds: HashMap<u64, GroundGoto> = HashMap::new();
    let mut flys: HashMap<u64, FlyGoto> = HashMap::new();
    let control_of = |nav: &mut Nav, controls: &mut HashMap<u64, usize>, unit: u64| *controls.entry(unit).or_insert_with(|| nav.new_control());
    let mut out = std::io::BufWriter::new(std::fs::File::create(out_path).expect("trace"));
    let op_list = ops.get("ops").and_then(|o| o.as_array()).expect("ops list");
    for (index, op) in op_list.iter().enumerate() {
        let name = op.get("op").and_then(|v| v.as_str()).unwrap_or("");
        let line = run(&mut nav, &mut world, &mut controls, &mut cursors, &mut grounds, &mut flys, &mut battle, &control_of, op);
        writeln!(out, "[{index},\"{name}\",{line}]").expect("write trace");
    }
}

type ControlOf = dyn Fn(&mut Nav, &mut HashMap<u64, usize>, u64) -> usize;

/// Battle-replay state: owner tokens in first-seen order and each
/// owner's last published fingerprint, mirroring the recorder's.
#[derive(Default)]
struct Battle {
    owners: Vec<u64>,
    control_of_owner: HashMap<u64, usize>,
    fingerprints: HashMap<u64, (Option<usize>, bool, u64)>,
    path_marks: HashMap<usize, u64>,
    next_mark: u64,
}

impl Battle {
    fn owner(&mut self, nav: &mut Nav, id: u64) -> usize {
        if let Some(c) = self.control_of_owner.get(&id) {
            return *c;
        }
        let c = nav.new_control();
        self.owners.push(id);
        self.control_of_owner.insert(id, c);
        self.fingerprints.insert(id, (None, false, 0));
        c
    }

    fn events(&mut self, nav: &Nav) -> String {
        let mut out = Vec::new();
        for i in 0..self.owners.len() {
            let id = self.owners[i];
            let c = &nav.controls[self.control_of_owner[&id]];
            // Path identity: the store's path id, marked on first sight
            // so a release-and-republish reads as a change only when
            // the slot's path (or ready) actually changed. The mark is
            // per path id; the recorder compares object identity, and
            // both change exactly when publish put another path there.
            let mark = match c.published {
                Some(p) => {
                    if !self.path_marks.contains_key(&p) {
                        self.next_mark += 1;
                        self.path_marks.insert(p, self.next_mark);
                    }
                    self.path_marks[&p]
                }
                None => 0,
            };
            let now = (c.published, c.ready, mark);
            let prev = self.fingerprints[&id];
            if (prev.0, prev.1) != (now.0, now.1) || prev.2 != now.2 {
                self.fingerprints.insert(id, now);
                let nodes = match c.published {
                    Some(p) => {
                        let nodes: Vec<String> = nav.store.paths[p].nodes.iter().map(|n| format!("[{},{}]", v3(Vec3::new(n.x, n.y, n.z)), n.flag)).collect();
                        format!("[{}]", nodes.join(","))
                    }
                    None => "null".into(),
                };
                out.push(format!("[{id},{},{}]", c.ready, nodes));
            }
        }
        format!("[{}]", out.join(","))
    }
}

fn battle_run(nav: &mut Nav, battle: &mut Battle, op: &Value) -> String {
    let name = op.get("op").and_then(|v| v.as_str()).unwrap_or("");
    if let Some(pools) = op.get("pools").and_then(|p| p.as_array()) {
        if pools.len() == 2 {
            nav.store.pools.paths.used = num(pools.first());
            nav.store.pools.nodes.used = num(pools.get(1));
        }
    }
    let result = match name {
        "bRequest" => {
            let control = battle.owner(nav, num(op.get("owner")) as u64);
            nav.controls[control].serial = num(op.get("serial"));
            nav.controls[control].segments = num(op.get("segments")) as u32;
            let ok = nav.planner_request(control, vec3(op.get("start")), vec3(op.get("end")), num(op.get("radius")), num(op.get("mask")) as u32, num(op.get("priority")));
            ok.to_string()
        }
        "bService" => {
            nav.service(num(op.get("budget")));
            bits(nav.served)
        }
        "bCancel" => {
            let control = battle.owner(nav, num(op.get("owner")) as u64);
            nav.planner_cancel(control);
            "true".into()
        }
        "bPromote" => {
            let control = battle.owner(nav, num(op.get("owner")) as u64);
            nav.planner_promote(control);
            "true".into()
        }
        "bSerial" => {
            let control = battle.owner(nav, num(op.get("owner")) as u64);
            nav.controls[control].serial = num(op.get("serial"));
            nav.controls[control].disposed = boolean(op.get("disposed"));
            bits(num(op.get("serial")))
        }
        "bPublish" => {
            let control = battle.owner(nav, num(op.get("owner")) as u64);
            let path = match op.get("path").and_then(|p| p.as_array()) {
                Some(points) => {
                    let p = nav.store.make();
                    for pt in points {
                        let v = vec3(Some(pt));
                        nav.store.append(p, bfe_sim::ai::nav::planner::PathNode { x: v.x, y: v.y, z: v.z, flag: 0 });
                    }
                    Some(p)
                }
                None => None,
            };
            nav.control_publish(control, path, boolean(op.get("ready")));
            "true".into()
        }
        "bConsume" => {
            let control = battle.owner(nav, num(op.get("owner")) as u64);
            let c = &nav.controls[control];
            let (ready, success) = if !c.ready {
                (false, false)
            } else if c.published.is_none() {
                (true, false)
            } else {
                (true, true)
            };
            if success {
                let c = &mut nav.controls[control];
                c.published = None;
                c.ready = false;
            }
            format!("[{ready},{success}]")
        }
        _ => "\"unknown\"".into(),
    };
    let events = battle.events(nav);
    format!("[{result},{events}]")
}

#[allow(clippy::too_many_arguments)]
fn run(
    nav: &mut Nav,
    world: &mut World,
    controls: &mut HashMap<u64, usize>,
    cursors: &mut HashMap<String, GraphCursor>,
    grounds: &mut HashMap<u64, GroundGoto>,
    flys: &mut HashMap<u64, FlyGoto>,
    battle: &mut Battle,
    control_of: &ControlOf,
    op: &Value,
) -> String {
    if op.get("op").and_then(|v| v.as_str()).map(|n| n.starts_with('b')).unwrap_or(false) && op.get("op").and_then(|v| v.as_str()) != Some("barrierUnused") {
        let n = op.get("op").and_then(|v| v.as_str()).unwrap_or("");
        if n.len() > 1 && n.as_bytes()[1].is_ascii_uppercase() {
            return battle_run(nav, battle, op);
        }
    }
    let name = op.get("op").and_then(|v| v.as_str()).unwrap_or("");
    let unit_at = |world: &World, op: &Value| -> usize {
        let id = num(op.get("unit")) as u64;
        world.units.iter().position(|u| u.id == id).expect("op names a unit")
    };
    // The JavaScript runner's controlOf attaches the unit (a spawn does
    // the same in the game), so control operations attach first here.
    match name {
        "request" | "cancel" | "consume" | "install" | "update" | "setLeader" | "recordTrail" => {
            let i = unit_at(world, op);
            let control = control_of(nav, controls, world.units[i].id);
            let unit = world.units[i].clone();
            nav.control_attach(control, Some(&unit));
        }
        _ => {}
    }
    match name {
        "seed" => {
            world.a = ClassicRandom::new(num(op.get("a")));
            world.b = ClassicRandom::new(num(op.get("b")));
            world.time = num(op.get("time"));
            world.far = opt_num(op.get("far")).unwrap_or(3.0);
            format!("[{},{}]", world.a.seed(), world.b.seed())
        }
        "time" => {
            world.time = num(op.get("time"));
            bits(world.time)
        }
        "rngNext" => format!("{}", world.a.next()),
        "rngUniform" => bits(world.a.uniform()),
        "rngInt" => bits(world.a.int(num(op.get("lo")), num(op.get("hi")))),
        "rngRange" => bits(world.a.range(num(op.get("lo")), num(op.get("hi")))),
        "rngMod" => bits(world.a.modulo(num(op.get("n")))),
        "rngNextB" => format!("{}", world.b.next()),
        "setUnit" => {
            let i = unit_at(world, op);
            let u = &mut world.units[i];
            if let Some(p) = op.get("position") {
                u.position = vec3(Some(p));
            }
            if let Some(v) = op.get("velocity") {
                u.velocity = vec3(Some(v));
            }
            if let Some(a) = op.get("avoidance") {
                if let Some(s) = u.soldier.as_mut() {
                    s.avoidance = opt_vec3(Some(a));
                }
            }
            if let Some(f) = op.get("fuel") {
                if let Some(s) = u.soldier.as_mut() {
                    s.fuel = num(Some(f));
                }
            }
            v3(u.position)
        }
        "pointClear" => {
            let (clear, barrier) = nav.barriers.point_clear(vec3(op.get("p")), num(op.get("r")), num(op.get("mask")) as u32);
            format!("[{clear},{}]", opt_index(barrier))
        }
        "segmentBlocked" => format!("{}", nav.barriers.segment_blocked(vec3(op.get("s")), vec3(op.get("e")), num(op.get("r")), num(op.get("mask")) as u32, None)),
        "nearest" => {
            let (any, barrier, d) = nav.barriers.nearest(vec3(op.get("s")), vec3(op.get("e")), num(op.get("r")), num(op.get("mask")) as u32, None);
            format!("[{any},{},{}]", opt_index(barrier), bits(d))
        }
        "obstruct" => {
            let mut e = vec3(op.get("e"));
            let mut normal = Vec3::default();
            let any = nav.barriers.obstruct(vec3(op.get("s")), &mut e, num(op.get("r")), num(op.get("mask")) as u32, None, Some(&mut normal));
            format!("[{any},{},{}]", v3(e), v3(normal))
        }
        "correct" => {
            let mut p = vec3(op.get("p"));
            let ok = nav.barriers.correct(&mut p, num(op.get("r")), num(op.get("mask")) as u32);
            format!("[{ok},{}]", v3(p))
        }
        "detour" => {
            let b = nav.barriers.list[num(op.get("barrier")) as usize].clone();
            let (mut l, mut r) = (Vec3::default(), Vec3::default());
            let found = barriers::detour(&b, vec3(op.get("p")), vec3(op.get("q")), num(op.get("r")), &mut l, &mut r);
            format!("[{found},{},{}]", v3(l), v3(r))
        }
        "inside" => {
            let b = &nav.barriers.list[num(op.get("barrier")) as usize];
            let p = vec3(op.get("p"));
            format!("{}", barriers::inside(b, p.x, p.y, p.z, num(op.get("r"))))
        }
        "ejection" => {
            let b = &nav.barriers.list[num(op.get("barrier")) as usize];
            let (score, out) = barriers::ejection(b, vec3(op.get("p")), num(op.get("r")), num(op.get("i")) as usize);
            format!("[{},{}]", bits(score), v3(out))
        }
        "attach" => {
            let g = nav.graph.as_ref().expect("graph op without a graph");
            let a = g.attach(vec3(op.get("p")), num(op.get("mask")) as u32);
            format!("[{},{},{}]", a.kind, opt_index(a.hub), opt_index(a.arc))
        }
        "select" => {
            let prev = op.get("prev").and_then(|v| v.as_f64()).map(|v| v as usize);
            let g = nav.graph.as_mut().expect("graph op without a graph");
            let picked = g.select(prev, num(op.get("from")) as usize, num(op.get("to")) as usize, num(op.get("mask")) as u32, &mut world.a);
            format!("[{},{}]", opt_index(picked), world.a.seed())
        }
        "nearestArc" => {
            let g = nav.graph.as_ref().expect("graph op without a graph");
            opt_index(g.nearest_arc(vec3(op.get("p")), num(op.get("mask")) as u32))
        }
        "corridorGap" => {
            let g = nav.graph.as_ref().expect("graph op without a graph");
            bits(g.corridor_gap(num(op.get("arc")) as usize, vec3(op.get("p"))))
        }
        "cursorInit" => {
            let id = op.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut cursor = GraphCursor::new();
            let g = nav.graph.as_mut().expect("cursor without a graph");
            let ok = cursor.init(g, vec3(op.get("start")), vec3(op.get("end")), num(op.get("mask")) as u32, num(op.get("clearance")), &mut world.a);
            let line = cursor_line(&cursor, world.a.seed());
            cursors.insert(id, cursor);
            format!("[{ok},{line}]")
        }
        "cursorAdvance" => {
            let id = op.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let mut cursor = cursors.remove(id).expect("cursor");
            let g = nav.graph.as_mut().expect("cursor without a graph");
            let ok = cursor.advance(g, &mut world.a);
            let line = cursor_line(&cursor, world.a.seed());
            cursors.insert(id.to_string(), cursor);
            format!("[{ok},{line}]")
        }
        "cursorRetry" => {
            let id = op.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let mut cursor = cursors.remove(id).expect("cursor");
            let Nav { graph, barriers, .. } = nav;
            let g = graph.as_ref().expect("cursor without a graph");
            let made = cursor.retry(g, barriers, num(op.get("radius")), &mut world.a);
            let line = cursor_line(&cursor, world.a.seed());
            cursors.insert(id.to_string(), cursor);
            format!("[{made},{line}]")
        }
        "request" => {
            let unit = num(op.get("unit")) as u64;
            let control = control_of(nav, controls, unit);
            if let Some(seg) = opt_num(op.get("segments")) {
                nav.controls[control].segments = seg as u32;
            }
            let ok = nav.planner_request(control, vec3(op.get("start")), vec3(op.get("end")), num(op.get("radius")), num(op.get("mask")) as u32, num(op.get("priority")));
            format!("[{ok},{}]", control_line(nav, control))
        }
        "service" => {
            let times = opt_num(op.get("times")).unwrap_or(1.0) as usize;
            for _ in 0..times {
                nav.service(num(op.get("budget")));
            }
            format!(
                "[{},{},{},{}]",
                nav.planner_idle(),
                bits(nav.served),
                bits(nav.store.pools.paths.used),
                bits(nav.store.pools.nodes.used)
            )
        }
        "cancel" => {
            let control = control_of(nav, controls, num(op.get("unit")) as u64);
            nav.planner_cancel(control);
            control_line(nav, control)
        }
        "consume" => {
            let i = unit_at(world, op);
            let control = control_of(nav, controls, world.units[i].id);
            let mut unit = world.units[i].clone();
            let (ready, success) = nav.control_consume(control, num(op.get("mode")), &mut unit);
            world.units[i] = unit;
            format!("[{ready},{success},{}]", control_line(nav, control))
        }
        "install" => {
            let i = unit_at(world, op);
            let control = control_of(nav, controls, world.units[i].id);
            let path = nav.store.make();
            for p in op.get("points").and_then(|p| p.as_array()).map(|p| p.iter()).into_iter().flatten() {
                let p = vec3(Some(p));
                nav.store.append(path, bfe_sim::ai::nav::planner::PathNode { x: p.x, y: p.y, z: p.z, flag: 0 });
            }
            let mut unit = world.units[i].clone();
            nav.control_install(control, path, true, false, &mut unit);
            world.units[i] = unit;
            control_line(nav, control)
        }
        "update" => {
            let i = unit_at(world, op);
            let control = control_of(nav, controls, world.units[i].id);
            let mut unit = world.units[i].clone();
            nav.control_update(control, num(op.get("dt")), &mut unit, world);
            world.units[i] = unit;
            control_line(nav, control)
        }
        "navInput" => {
            let i = unit_at(world, op);
            let control = control_of(nav, controls, world.units[i].id);
            let aim = opt_vec3(op.get("aim"));
            let mut unit = world.units[i].clone();
            let intent = nav.navigation_input(control, &mut unit, num(op.get("dt")), aim, boolean(op.get("facing")), world);
            world.units[i] = unit;
            intent_line(&intent)
        }
        "setLeader" => {
            let control = control_of(nav, controls, num(op.get("unit")) as u64);
            let leader = opt_num(op.get("leader")).map(|id| UnitRef { unit: id as u64, generation: Some(1.0) });
            nav.control_set_leader(control, leader);
            control_line(nav, control)
        }
        "recordTrail" => {
            let i = unit_at(world, op);
            let control = control_of(nav, controls, world.units[i].id);
            let mut unit = world.units[i].clone();
            nav.control_record_trail(control, boolean(op.get("on")), &mut unit);
            world.units[i] = unit;
            control_line(nav, control)
        }
        "goto" => {
            let i = unit_at(world, op);
            let id = world.units[i].id;
            let control = control_of(nav, controls, id);
            let mut helper = grounds.remove(&id).unwrap_or_else(|| GroundGoto::new(control));
            helper.control = control;
            let mode = match op.get("mode").and_then(|m| m.as_str()) {
                Some("direct") => GotoMode::Direct,
                Some("straight") => GotoMode::Straight,
                _ => GotoMode::Route,
            };
            let mut unit = world.units[i].clone();
            nav.ground_goto(&mut helper, &mut unit, world, vec3(op.get("point")), mode);
            world.units[i] = unit;
            let line = goto_line(nav, &helper);
            grounds.insert(id, helper);
            line
        }
        "gotoTick" => {
            let i = unit_at(world, op);
            let id = world.units[i].id;
            let mut helper = grounds.remove(&id).expect("goto helper");
            let mut unit = world.units[i].clone();
            nav.ground_tick(&mut helper, &mut unit, world, num(op.get("dt")));
            world.units[i] = unit;
            let line = goto_line(nav, &helper);
            grounds.insert(id, helper);
            line
        }
        "gotoStop" => {
            let i = unit_at(world, op);
            let id = world.units[i].id;
            let mut helper = grounds.remove(&id).expect("goto helper");
            let mut unit = world.units[i].clone();
            nav.ground_stop(&mut helper, &mut unit, world);
            world.units[i] = unit;
            let line = goto_line(nav, &helper);
            grounds.insert(id, helper);
            line
        }
        "follow" => {
            let i = unit_at(world, op);
            let id = world.units[i].id;
            let control = control_of(nav, controls, id);
            let mut helper = grounds.remove(&id).unwrap_or_else(|| GroundGoto::new(control));
            let target = opt_num(op.get("target")).map(|t| UnitRef { unit: t as u64, generation: Some(1.0) });
            let mut unit = world.units[i].clone();
            nav.ground_follow(&mut helper, &mut unit, world, target, num(op.get("distance")), boolean(op.get("remembered")));
            world.units[i] = unit;
            let line = goto_line(nav, &helper);
            grounds.insert(id, helper);
            line
        }
        "remember" => {
            world.remembered.insert(num(op.get("target")) as u64, vec3(op.get("point")));
            "true".into()
        }
        "clearAbove" => {
            world.clear_above = op.get("answers").and_then(|a| a.as_array()).map(|a| a.iter().map(|v| boolean(Some(v))).collect()).unwrap_or_default();
            format!("{}", world.clear_above.len())
        }
        "flyGoto" => {
            let i = unit_at(world, op);
            let id = world.units[i].id;
            let control = control_of(nav, controls, id);
            let mut helper = flys.remove(&id).unwrap_or_else(|| FlyGoto::new(control));
            let mode = match op.get("mode").and_then(|m| m.as_str()) {
                Some("direct") => GotoMode::Direct,
                Some("straight") => GotoMode::Straight,
                _ => GotoMode::Route,
            };
            let mut unit = world.units[i].clone();
            nav.fly_goto(&mut helper, &mut unit, vec3(op.get("point")), mode);
            world.units[i] = unit;
            let line = format!("[{},{},{},{}]", bits(helper.state), v3(helper.destination), helper.failed, control_line(nav, control));
            flys.insert(id, helper);
            line
        }
        "flyTick" => {
            let i = unit_at(world, op);
            let id = world.units[i].id;
            let mut helper = flys.remove(&id).expect("fly helper");
            let mut unit = world.units[i].clone();
            nav.fly_tick(&mut helper, &mut unit);
            world.units[i] = unit;
            let line = format!("[{},{},{}]", bits(helper.state), nav.fly_arrived(&helper), control_line(nav, helper.control));
            flys.insert(id, helper);
            line
        }
        "queryHints" => {
            let found = nav.query_hints(num(op.get("mask")) as u32, vec3(op.get("center")), num(op.get("half")), opt_num(op.get("cap")).unwrap_or(f64::INFINITY), opt_num(op.get("owner")).map(|o| o as u64), num(op.get("now")));
            format!("[{}]", found.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","))
        }
        "markHint" => {
            nav.mark_hint(num(op.get("hint")) as usize, Some(1), num(op.get("now")));
            let h = &nav.hints.list[num(op.get("hint")) as usize];
            bits(h.reserved_at)
        }
        "pickStance" => bits(hints::pick_stance(num(op.get("mask")) as i32, &mut world.a)),
        "pickSecondary" => bits(hints::pick_secondary(num(op.get("choices")) as i32, &mut world.a)),
        "jetFind" => {
            let i = unit_at(world, op);
            let unit = &world.units[i];
            let found = nav.query_hints(8, unit.position, 40.0, 30.0, Some(unit.id), world.time);
            let candidates: Vec<(usize, Vec3)> = found.iter().map(|&h| (h, nav.hints.list[h].position)).collect();
            let position = unit.position;
            let picked = jet_hints::find_hint(&candidates, position, &mut world.a);
            format!("[{},{}]", opt_index(picked), world.a.seed())
        }
        "outside" => format!("{}", nav.outside(vec3(op.get("p")))),
        "hintsDump" => {
            let lines: Vec<String> = nav
                .hints
                .list
                .iter()
                .map(|h| {
                    format!(
                        "[{},{},{},{},{},{},{},{},{},{},{},{},{}]",
                        h.index,
                        h.source,
                        h.copy,
                        h.bit,
                        v3(h.position),
                        v3(h.forward),
                        v3(h.right),
                        h.primary_stance,
                        h.secondary_stance,
                        h.secondary_choices,
                        bits(h.mode),
                        bits(h.radius),
                        opt_index(h.link)
                    )
                })
                .collect();
            format!("[{}]", lines.join(","))
        }
        "graphDump" => {
            let g = nav.graph.as_ref().expect("graph");
            let hubs: Vec<String> = g.hubs.iter().map(|h| format!("[{},{},{}]", v3(h.pos()), bits(h.radius), h.mask)).collect();
            format!("[{}]", hubs.join(","))
        }
        _ => "\"unknown\"".into(),
    }
}

fn cursor_line(c: &GraphCursor, seed: u32) -> String {
    format!(
        "[{},{},{},{},{},{}]",
        v3(c.waypoint),
        opt_index(c.current),
        opt_index(c.primary),
        opt_index(c.secondary),
        opt_index(c.previous),
        seed
    )
}

fn control_line(nav: &Nav, control: usize) -> String {
    let c = &nav.controls[control];
    let published = match c.published {
        Some(p) => {
            let nodes: Vec<String> = nav.store.paths[p].nodes.iter().map(|n| format!("[{},{}]", v3(Vec3::new(n.x, n.y, n.z)), n.flag)).collect();
            format!("[{}]", nodes.join(","))
        }
        None => "null".into(),
    };
    let executor = match &c.executor {
        Some(e) => format!(
            "[{},{},{},{},{},{},{},{}]",
            e.index,
            v3(e.waypoint),
            v3(e.prev),
            v3(e.direction),
            bits(e.mode),
            bits(e.status),
            e.complete,
            nav.store.paths[e.path].nodes.len()
        ),
        None => "null".into(),
    };
    let r = &c.request;
    format!(
        "[{},{},{},{},{},{},{}]",
        bits(c.serial),
        c.segments,
        c.ready,
        published,
        executor,
        format!("[{},{},{},{},{},{}]", r.valid, v3(r.steering_target), v3(r.waypoint), v3(r.segment_direction), bits(r.mode), bits(r.lower)),
        bits(c.stranded_for)
    )
}

fn goto_line(nav: &Nav, helper: &GroundGoto) -> String {
    format!(
        "[{},{},{},{},{},{}]",
        bits(helper.state),
        v3(helper.destination),
        helper.failed,
        nav.ground_arrived(helper),
        bits(helper.jet_deadline),
        control_line(nav, helper.control)
    )
}

fn intent_line(intent: &Intent) -> String {
    let jet = match intent.jet_direction {
        None => "null".to_string(),
        Some(None) => "\"null\"".to_string(),
        Some(Some(v)) => v3(v),
    };
    format!(
        "[{},{},{},{},{},{},{},{}]",
        bits(intent.forward),
        bits(intent.right),
        bits(intent.turn),
        bits(intent.pitch),
        intent.jump,
        intent.crouch,
        intent.prone,
        jet
    )
}
