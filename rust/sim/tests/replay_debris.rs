//! Actual Classic debris motion. Geometry and particle calls are served
//! with checked inputs; no piece afterstate is copied into native Debris.
#![cfg(feature = "replay")]
use bfe_sim::battle::debris::{Bounds, Debris, Hit, Launch, Piece, World};
use bfe_sim::gen::{DbChunk, DbState, Replay};
use bfe_sim::store::StrId;
use sim_replay::{check_bool, check_num, check_value, check_vec3, replay_corpus, Arena, Call, Corpus, Event, Frame, Log, Node, Outcome, V, Value};
use std::collections::BTreeMap;
use three::{Matrix4, Quaternion, Vector3};

fn vec(v: V) -> Vector3 { let a = v.vec3().expect("captured vector"); Vector3::new(a[0], a[1], a[2]) }
fn xyz(v: Vector3) -> [f64; 3] { [v.x, v.y, v.z] }
fn matrix(v: V) -> Matrix4 {
    let Some(Node::Mat4(a)) = v.node() else { panic!("captured matrix") }; let mut m = Matrix4::default(); m.from_array(a, 0); m
}
fn entry<'a>(c: &'a Call) -> V<'a> { c.call.root().get("s") }
struct Served<'a> {
    log: Log<'a>, cx: Replay, failure: Option<String>, children: BTreeMap<u32, Vec<Vector3>>, bodies: V<'a>, calls: BTreeMap<String, usize>,
}
impl<'a> Served<'a> {
    fn check(&mut self, r: Result<(), impl ToString>) { if let Err(e) = r { self.failure.get_or_insert_with(|| e.to_string()); } }
    fn call(&mut self, name: &str) -> Option<&'a Call> {
        match self.log.call(&format!("debris.{name}")) {
            Ok(c) => { *self.calls.entry(name.into()).or_default() += 1; Some(c) },
            Err(e) => { self.failure.get_or_insert_with(|| e.to_string()); None },
        }
    }
    fn mesh(&mut self, expected: V, id: u32, position: Vector3, turn: Quaternion) {
        self.check(check_num("mesh id", expected.get("id"), id as f64));
        self.check(check_vec3("mesh position", expected.get("position"), xyz(position)));
        let mut arena = Arena::default(); let q = arena.quat([turn.x, turn.y, turn.z, turn.w]);
        self.check(check_value("mesh quaternion", expected.get("quaternion"), arena.view(&q)));
        self.check(check_vec3("mesh scale", expected.get("scale"), [1.0; 3]));
        let actual = self.children.get(&id).expect("mesh children").clone();
        if expected.get("children").len() != actual.len() { self.failure.get_or_insert("mesh child count".into()); }
        for (i, p) in actual.iter().enumerate() { self.check(check_vec3("mesh child translation", expected.get("children").at(i), xyz(*p))); }
    }
    fn prepared_mesh(&mut self, name: &str, source: V) -> Option<&'a Call> {
        let c = self.call(name)?;
        self.check(check_value("initial geometry transform", entry(c).get("mesh"), source)); Some(c)
    }
    fn spawn(&mut self, debris: &mut Debris, spawn: V) -> Result<(), String> {
        for input in spawn.get("chunks").items() {
            let model = if input.get("named").bool() {
                let Some(c) = self.call("model") else { break };
                self.check(check_value("model name", c.arg(0), input.get("name"))); Some(c)
            } else { None };
            if !model.is_some_and(|c| c.extra().get("found").bool()) || input.get("bone").is_null() { continue; }
            let Some(make) = self.call("make") else { break };
            self.check(check_num("model factory", make.arg(0), model.unwrap().extra().get("factory").num()));
            let mesh = make.extra().get("mesh"); let id = mesh.get("id").num() as u32;
            self.children.insert(id, mesh.get("children").items().map(vec).collect());
            let center = spawn.get("center").bool();
            let bounds = if center {
                let Some(c) = self.prepared_mesh("matrix", mesh) else { break };
                self.check(check_bool("initial matrix force", c.arg(0), true));
                let Some(c) = self.prepared_mesh("bounds", mesh) else { break };
                self.check(check_value("initial bounds mesh", c.arg(0), make.result()));
                Bounds { min: vec(c.extra().get("low")), max: vec(c.extra().get("high")) }
            } else { Bounds { min: Vector3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), max: Vector3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY) } };
            let Some(c) = self.call("worldMatrix") else { break };
            self.check(check_bool("bone update parents", c.arg(0), true)); self.check(check_bool("bone update children", c.arg(1), false));
            self.check(check_num("selected bone", c.extra().get("bone"), input.get("bone").num()));
            let launch = Launch { id, chunk: DbChunk::from_value(input.get("chunk"), &mut self.cx, "chunk")?.into(), matrix: matrix(c.extra().get("matrix")), bounds,
                origin: vec(spawn.get("origin")), velocity: vec(spawn.get("velocity")), health: spawn.get("health").num(),
                ignore: spawn.get("ignore").opt_num().map(|n| n as u32), center, floor: spawn.get("floor").num() };
            debris.launch(launch, self);
        }
        Ok(())
    }
}
impl World for Served<'_> {
    fn add(&mut self, id: u32, p: &Vector3, q: &Quaternion, shift: Option<&Vector3>) {
        if let Some(s) = shift { for p in self.children.get_mut(&id).unwrap() { p.sub(s); } }
        if let Some(c) = self.call("add") { self.mesh(entry(c).get("mesh"), id, *p, *q); }
    }
    fn remove(&mut self, p: &Piece) { if let Some(c) = self.call("remove") { self.mesh(entry(c).get("mesh"), p.id, p.position, p.quaternion); } }
    fn cast(&mut self, start: &Vector3, ray: &Vector3, far: f64, ignore: Option<u32>) -> Option<Hit> {
        let c = self.call("cast")?;
        self.check(check_vec3("cast origin", c.arg(0), xyz(*start))); self.check(check_vec3("cast direction", c.arg(1), xyz(*ray)));
        self.check(check_num("cast length", c.arg(2), far));
        if let Some(id) = ignore { self.check(check_value("cast ignored body", c.arg(4), self.bodies.at(id as usize))); }
        else { self.check(check_value("cast ignored body", c.arg(4), Arena::default().view(&Value::Null))); }
        (!c.result().is_nullish()).then(|| Hit { point: vec(c.result().get("point")), normal: vec(c.result().get("normal")) })
    }
    fn low(&mut self, p: &Piece, q: &Quaternion) -> f64 {
        let Some(c) = self.call("matrix") else { return 0.0 };
        self.mesh(entry(c).get("mesh"), p.id, p.position, *q); self.check(check_bool("settling matrix force", c.arg(0), true));
        let Some(c) = self.call("bounds") else { return 0.0 };
        self.mesh(entry(c).get("mesh"), p.id, p.position, *q); vec(c.extra().get("low")).y
    }
    fn play(&mut self, _p: &Piece, name: StrId, p: &Vector3, velocity: Option<&Vector3>, _impact: bool) -> Option<u32> {
        let c = self.call("play")?; let text = self.cx.str_out(name); let arena = Arena::default();
        self.check(check_value("effect name", c.arg(0), arena.view(&text))); self.check(check_vec3("effect position", c.arg(1), xyz(*p)));
        if let Some(v) = velocity {
            self.check(check_value("trail normal", c.arg(2), arena.view(&Value::Null))); self.check(check_vec3("effect velocity", c.arg(3), xyz(*v)));
            if c.args().len() != 4 { self.failure.get_or_insert("trail argument count".into()); }
        } else if c.args().len() != 2 { self.failure.get_or_insert("smoke/impact argument count".into()); }
        c.extra().get("effect").opt_num().map(|n| n as u32)
    }
    fn move_effect(&mut self, p: &Piece, id: u32, velocity: bool) {
        if let Some(c) = self.call("move") {
            self.check(check_num("moved effect", entry(c).get("effect"), id as f64)); self.check(check_vec3("effect move position", c.arg(0), xyz(p.position)));
            if velocity {
                self.check(check_value("effect move turn", c.arg(1), Arena::default().view(&Value::Null))); self.check(check_vec3("effect move velocity", c.arg(2), xyz(p.velocity)));
                if c.args().len() != 3 { self.failure.get_or_insert("trail move arguments".into()); }
            } else if c.args().len() != 1 { self.failure.get_or_insert("smoke move arguments".into()); }
        }
    }
    fn stop_effect(&mut self, id: u32) {
        if let Some(c) = self.call("stop") { self.check(check_num("stopped effect", entry(c).get("effect"), id as f64)); }
    }
}
fn replay(frame: &Frame) -> Result<BTreeMap<String, usize>, String> {
    let mut cx = Replay::default(); let mut debris: Debris = DbState::from_value(frame.before().get("state"), &mut cx, "state")?.into();
    let children = frame.before().get("meshes").items().map(|m| (m.get("id").num() as u32, m.get("children").items().map(vec).collect())).collect();
    let mut world = Served { log: Log::new(frame), cx, failure: None, children, bodies: frame.before().get("bodies"), calls: BTreeMap::new() };
    match frame.name.as_str() {
        "Debris.blowApart" => world.spawn(&mut debris, frame.before().get("spawn"))?,
        "Debris.update" => debris.update(frame.before().get("dt").num(), &mut world),
        "Debris.bounce" => debris.bounce(frame.before().get("id").num() as u32, &mut world),
        "Debris.rest" => debris.rest(frame.before().get("id").num() as u32, frame.before().get("floor").num(), &mut world),
        "Debris.clear" => debris.clear(&mut world),
        _ => return Err("unknown debris method".into()),
    }
    if let Some(e) = world.failure { return Err(e); }
    world.log.finish().map_err(|e| e.to_string())?;
    let mut arena = Arena::default(); let state = DbState::from(&debris).to_value(&mut world.cx, &mut arena);
    check_value("debris state", frame.after().get("state"), arena.view(&state)).map_err(|e| e.to_string())?;
    check_value("return", frame.result(), arena.view(&Value::Undefined)).map_err(|e| e.to_string())?;
    Ok(world.calls)
}
const METHODS: &[&str] = &["blowApart", "update", "bounce", "rest", "clear"];
#[test]
fn debris_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return }; let mut total = 0; let mut calls = BTreeMap::<String, usize>::new();
    for method in METHODS {
        let result = replay_corpus(&corpus, &format!("Debris.{method}"), |_, frame| match replay(frame) {
            Ok(counts) => { for (k, n) in counts { *calls.entry(k).or_default() += n; } Outcome::Same }, Err(e) => Outcome::Failed(e),
        }).unwrap();
        eprintln!("{}", result.summary()); total += result.replayed(); result.assert_clean();
    }
    assert!(total > 0); eprintln!("debris: {total} exact frames; callbacks {calls:?}");
}
#[test]
fn debris_rejects_state_cast_and_geometry_mutations() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return }; let (mut state, mut cast, mut geometry) = (false, false, false);
    for capture in corpus.captures() { let capture = capture.unwrap();
        for frame in capture.frames("Debris.update") { let frame = frame.unwrap(); replay(&frame).expect("unmodified debris frame");
            if !state && frame.after().get("state").get("pieces").len() > 0 {
                let mut changed = frame.clone(); let id = changed.after().get("state").get("pieces").at(0).get("position").node_id().unwrap();
                if let Node::Vec3(v) = &mut changed.exit.arena.nodes[id as usize] { v[0] += 123.0; }
                assert!(replay(&changed).is_err(), "wrong debris position accepted"); state = true;
            }
            for (i, event) in frame.events.iter().enumerate() { let Event::Call(c) = event else { continue };
                if !cast && c.name == "debris.cast" {
                    let mut changed = frame.clone(); let Event::Call(c) = &mut changed.events[i] else { unreachable!() };
                    let id = c.args().node_id().unwrap(); if let Node::Array(a) = &mut c.call.arena.nodes[id as usize] { a[2] = Value::Num(123456.0); }
                    assert!(replay(&changed).is_err(), "wrong debris cast accepted"); cast = true;
                }
                if !geometry && c.name == "debris.bounds" {
                    let mut changed = frame.clone(); let Event::Call(c) = &mut changed.events[i] else { unreachable!() };
                    let id = entry(c).get("mesh").get("quaternion").node_id().unwrap();
                    if let Node::Quat(q) = &mut c.call.arena.nodes[id as usize] { q[1] += 1.0; }
                    assert!(replay(&changed).is_err(), "wrong settling transform accepted"); geometry = true;
                }
            }
            if state && cast && geometry { return; }
        }
    }
    panic!("missing debris mutation coverage: state={state}, cast={cast}, geometry={geometry}");
}
