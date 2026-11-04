//! LiveObjects queries replay with argument-exact target/collision calls.
//! Lifecycle callbacks belong to the separately gated target modules.
#![cfg(feature = "replay")]
use bfe_sim::battle::live::{self, Damage, Definition, Health, Hit, Out, Queries, Target, World};
use bfe_sim::battle::props::{Identity, State};
use bfe_sim::ordnance::defs::{ExplosionDef, Field};
use sim_replay::{check_num, check_value, check_vec3, replay_corpus, Arena, Call, Corpus, Event, Frame, Log, Node, Outcome, V, Value};
use three::{Matrix4, Vector3};

fn vec(v: V) -> Vector3 { let p = v.vec3().unwrap_or_else(|| [v.at(0).num(), v.at(1).num(), v.at(2).num()]); Vector3::new(p[0], p[1], p[2]) }
fn xyz(v: Vector3) -> [f64; 3] { [v.x, v.y, v.z] }
fn field(v: V) -> Field { match v.value {
    Value::Undefined => Field::Undefined, Value::Null => Field::Null, Value::Bool(b) => Field::Bool(*b), Value::Num(n) => Field::Num(*n),
    Value::Str(s) => Field::Str(s.clone()), _ => panic!("live definition primitive: {}", v.show()),
} }
fn definition(v: V) -> Definition {
    Definition { repair: std::array::from_fn(|i| field(v.get("repair").at(i))), target_vehicle: field(v.get("targetVehicle")), target_neutral: field(v.get("targetNeutral")),
        target_enemy: field(v.get("targetEnemy")), target_friendly: field(v.get("targetFriendly")), range: field(v.get("range")), angle: field(v.get("angle")), shield_scale: field(v.get("shieldScale")),
        explosion: ExplosionDef { damage: field(v.get("damage")), damage_radius_inner: field(v.get("inner")), damage_radius_outer: field(v.get("outer")), scales: std::array::from_fn(|i| field(v.get("scales").at(i))) } }
}
fn target<'a>(v: V<'a>, bodies: &mut Vec<V<'a>>) -> Target {
    let body = (!v.get("body").is_nullish()).then(|| { bodies.push(v.get("body")); bodies.len() - 1 });
    let mut matrix = Matrix4::default(); matrix.from_array(&v.get("matrix").items().map(|v| v.num()).collect::<Vec<_>>(), 0);
    Target { alive: v.get("alive").bool(), state: match v.get("state").str() { Some("built") => Some(State::Built), Some("unbuilt") => Some(State::Unbuilt), Some("destroyed") => Some(State::Destroyed), _ => None },
        health_type: match v.get("healthType").str() { Some("building") => Some(Health::Building), Some("vehicle") => Some(Health::Vehicle), Some("rebuild") => Some(Health::Rebuild), Some("build") => Some(Health::Build), _ => None },
        health: v.get("health").num(), max_health: v.get("maxHealth").num(), team: v.get("team").num(), body, gunner: v.get("gunner").bool(), position: vec(v.get("position")), matrix }
}
fn value(v: Field, arena: &mut Arena) -> Value { match v {
    Field::Undefined => Value::Undefined, Field::Null => Value::Null, Field::Bool(v) => Value::Bool(v), Field::Num(v) => Value::Num(v), Field::Str(v) => Value::str(&v),
    _ => arena.object("", vec![]),
} }
struct Served<'f> {
    log: Log<'f>, targets: V<'f>, bodies: Vec<V<'f>>, owner: Option<V<'f>>, hit: V<'f>, failure: Option<String>, callbacks: usize,
}
impl<'f> Served<'f> {
    fn fail(&mut self, why: impl ToString) { self.failure.get_or_insert_with(|| why.to_string()); }
    fn call(&mut self, name: String) -> Option<&'f Call> { match self.log.call(&name) { Ok(c) => { self.callbacks += 1; Some(c) }, Err(e) => { self.fail(e); None } } }
    fn number(&mut self, expected: V, n: f64) { if let Err(e) = check_num("live argument", expected, n) { self.fail(e); } }
    fn point(&mut self, expected: V, p: Vector3) { if let Err(e) = check_vec3("live point", expected, xyz(p)) { self.fail(e); } }
    fn object(&mut self, expected: V, actual: V) { if let Err(e) = check_value("live identity", expected, actual) { self.fail(e); } }
    fn update(&mut self, c: &'f Call) { self.targets = c.extra().get("targets"); }
    fn effect(&mut self, q: &mut Queries, c: &'f Call) {
        for (key, point) in [("center", q.center), ("toward", q.toward)] {
            let expected = c.extra().get("before").get(key);
            let mut arena = Arena::default(); let actual = arena.array(xyz(point).map(Value::Num).to_vec());
            if let Err(e) = check_value("query callback entry", expected, arena.view(&actual)) { self.fail(e); }
        }
        // A lifecycle callback can reenter another query (an explosion's
        // blast). Only these shared scratch vectors travel back; native
        // entry geometry above is checked before applying that effect.
        q.center = vec(c.extra().get("after").get("center")); q.toward = vec(c.extra().get("after").get("toward"));
        if c.extra().has("targets") { self.targets = c.extra().get("targets"); }
    }
}
impl World for Served<'_> {
    fn target(&mut self, index: usize) -> Target { target(self.targets.at(index), &mut self.bodies) }
    fn sphere(&mut self, index: usize, blast: bool, point: Vector3) -> (Vector3, f64) {
        let method = if blast { "blastSphere" } else { "sphere" };
        if let Some(c) = self.call(format!("live.{method}:{index}")) {
            self.point(c.arg(0), point); self.update(c); (vec(c.out(0)), c.result().num())
        } else { (point, 0.0) }
    }
    fn cast(&mut self, origin: Vector3, direction: Vector3, far: f64, ignore: Option<usize>, static_only: bool, out: Out) -> Option<Hit> {
        let c = self.call("live.cast".into())?;
        self.point(c.arg(0), origin); self.point(c.arg(1), direction); self.number(c.arg(2), far);
        if let Some(index) = ignore { self.object(c.arg(4), self.bodies[index]); }
        else if matches!(out, Out::Sight) { if !c.arg(4).is_null() { self.fail("blast body null"); } }
        else if !c.arg(4).is_undefined() { self.fail("lock cast has no ignore body"); }
        if static_only { if c.arg(5).opt_bool() != Some(true) { self.fail("blast cast static flag"); } }
        else if !c.arg(5).is_undefined() { self.fail("lock cast has no static flag"); }
        if matches!(out, Out::Lock) { self.object(c.arg(3), self.hit); self.hit = c.out(3); }
        (!c.result().is_nullish()).then(|| Hit { distance: c.result().get("distance").num(), owner: c.extra().get("owner").opt_num().filter(|n| *n >= 0.0).map(|n| n as usize) })
    }
    fn team_scale(&mut self, q: &mut Queries, index: usize) -> f64 {
        if let Some(c) = self.call("live.teamScale".into()) { self.object(c.arg(1), self.targets.at(index).get("self")); self.effect(q, c); c.result().num() } else { 0.0 }
    }
    fn damage(&mut self, q: &mut Queries, index: usize, amount: f64, _owner: Identity, info: Option<Damage>) {
        if let Some(c) = self.call(format!("live.damage:{index}")) {
            self.number(c.arg(0), amount);
            if let Some(owner) = self.owner { self.object(c.arg(1), owner); } else if !c.arg(1).is_null() { self.fail("default owner null"); }
            if let Some(info) = info {
                if let Some(direction) = info.direction { self.point(c.arg(2).get("direction"), direction); } else if !c.arg(2).get("direction").is_null() { self.fail("damage direction null"); }
                self.number(c.arg(2).get("amount"), info.amount); self.number(c.arg(2).get("typed"), info.typed);
                let mut arena = Arena::default(); let generic = value(info.generic, &mut arena);
                if let Err(e) = check_value("generic damage scale", c.arg(2).get("generic"), arena.view(&generic)) { self.fail(e); }
            } else if !c.arg(2).is_undefined() { self.fail("blast damage has no hit record"); }
            self.effect(q, c); self.update(c);
        }
    }
    fn set_health(&mut self, q: &mut Queries, index: usize, health: Field) {
        if let Some(c) = self.call(format!("live.setHealth:{index}")) {
            let mut arena = Arena::default(); let health = value(health, &mut arena);
            if let Err(e) = check_value("repair health", c.arg(0), arena.view(&health)) { self.fail(e); }
            self.effect(q, c); self.update(c);
        }
    }
    fn may_enter(&mut self, q: &mut Queries, index: usize) -> bool {
        if let Some(c) = self.call("live.may".into()) { self.object(c.arg(1), self.targets.at(index).get("self")); self.effect(q, c); c.result().truthy() } else { false }
    }
}
fn replay(frame: &Frame) -> Result<usize, String> {
    let before = frame.before(); let args = frame.args(); let def = definition(before.get("definition"));
    let mut q = Queries { center: vec(before.get("center")), toward: vec(before.get("toward")) };
    let mut arena = Arena::default();
    let owner = (!args.at(2).is_undefined()).then(|| args.at(2));
    let mut world = Served { log: Log::new(frame), targets: before.get("targets"), bodies: vec![], owner, hit: before.get("hit"), failure: None, callbacks: 0 };
    let list = |name: &str| before.get(name).items().map(|n| n.num() as usize).collect::<Vec<_>>();
    let one = before.get("one").num(); let one = (one >= 0.0).then_some(one as usize);
    let team = |at: usize| if args.at(at).is_undefined() { before.get("playerTeam").num() } else { args.at(at).num() };
    let collision = before.get("collision").bool(); let mut result = Value::Undefined;
    match frame.name.as_str() {
        "LiveObjects.blast" => q.blast(&list("buildings"), &def.explosion, vec(args.at(1)), Identity::Null, args.at(3).truthy(), collision, &mut world),
        "LiveObjects.struck" => q.struck(if before.get("building").bool() { one } else { None }, &def, Identity::Null, args.at(3).truthy(), (!args.at(4).is_nullish()).then(|| vec(args.at(4))), &mut world),
        "LiveObjects.repair" => result = Value::Bool(q.repair(one, &def, team(2), &mut world)),
        "LiveObjects.repairs" => result = Value::Bool(live::repairs(&def, &world.target(one.expect("repair target")), team(2))),
        "LiveObjects.lockOn" => {
            let candidates = [list("buildings"), list("vehicles")].concat();
            let answer = q.lock_on(&candidates, vec(args.at(0)), vec(args.at(1)), &def, one, args.at(4).truthy(), team(5), collision, &mut world);
            result = answer.map(|i| arena.copy(world.targets.at(i).get("self"))).unwrap_or(Value::Null);
        }
        "LiveObjects.enterable" => {
            let answer = q.enterable(&list("turrets"), vec(args.at(0)), vec(args.at(1)), args.at(2).truthy(), &mut world);
            result = answer.map(|i| arena.copy(world.targets.at(i).get("self"))).unwrap_or(Value::Null);
        }
        _ => return Err("live method".into()),
    }
    if let Some(e) = world.failure { return Err(e); }
    world.log.finish().map_err(|e| e.to_string())?;
    for (key, point) in [("center", q.center), ("toward", q.toward)] {
        let array = arena.array(xyz(point).map(Value::Num).to_vec());
        check_value(key, frame.after().get(key), arena.view(&array)).map_err(|e| e.to_string())?;
    }
    check_value("collision output", frame.after().get("hit"), world.hit).map_err(|e| e.to_string())?;
    check_value("foreign target output", frame.after().get("targets"), world.targets).map_err(|e| e.to_string())?;
    check_value("return", frame.result(), arena.view(&result)).map_err(|e| e.to_string())?;
    Ok(world.callbacks)
}
#[test]
fn live_queries_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let mut total = 0; let mut callbacks = 0;
    for method in ["LiveObjects.blast", "LiveObjects.struck", "LiveObjects.repairs", "LiveObjects.repair", "LiveObjects.lockOn", "LiveObjects.enterable"] {
        let result = replay_corpus(&corpus, method, |_, frame| match replay(frame) { Ok(n) => { callbacks += n; Outcome::Same }, Err(e) => Outcome::Failed(e) }).unwrap();
        eprintln!("{}", result.summary()); total += result.replayed(); result.assert_clean();
    }
    assert!(total > 0); eprintln!("live queries: {total} exact frames, {callbacks} checked callbacks");
}

#[test]
fn live_queries_reject_selection_geometry_and_callback_mutations() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let mut selection = false; let mut geometry = false; let mut callback = false; let mut entry = false;
    for capture in corpus.captures() {
        let capture = capture.unwrap();
        for method in ["LiveObjects.lockOn", "LiveObjects.blast", "LiveObjects.repair", "LiveObjects.enterable"] {
            for frame in capture.frames(method) {
                let frame = frame.unwrap(); replay(&frame).expect("unmodified live frame");
                if !selection && method == "LiveObjects.lockOn" {
                    let mut changed = frame.clone();
                    let id = changed.exit.root().node_id().unwrap();
                    if let Node::Object { fields, .. } = &mut changed.exit.arena.nodes[id as usize] {
                        fields.iter_mut().find(|(k, _)| k == "r").unwrap().1 = Value::Num(-44.0);
                    }
                    assert!(replay(&changed).is_err(), "wrong selected target accepted"); selection = true;
                }
                if !geometry {
                    let mut changed = frame.clone(); let id = changed.after().get("toward").node_id().unwrap();
                    if let Node::Array(values) = &mut changed.exit.arena.nodes[id as usize] { values[0] = Value::Num(1000000.0); }
                    assert!(replay(&changed).is_err(), "wrong query geometry accepted"); geometry = true;
                }
                if !callback {
                    let mut changed = frame.clone();
                    if let Some(Event::Call(call)) = changed.events.iter_mut().find(|e| matches!(e, Event::Call(c) if c.name == "live.cast")) {
                        let id = call.args().node_id().unwrap();
                        if let Node::Array(values) = &mut call.call.arena.nodes[id as usize] { values[2] = Value::Num(-7.0); }
                        assert!(replay(&changed).is_err(), "wrong cast extent accepted"); callback = true;
                    }
                }
                if !entry {
                    let mut changed = frame.clone();
                    if let Some(Event::Call(call)) = changed.events.iter_mut().find(|e| matches!(e, Event::Call(c) if c.extra().has("before"))) {
                        let id = call.extra().get("before").get("center").node_id().unwrap();
                        if let Node::Array(values) = &mut call.ret.arena.nodes[id as usize] { values[0] = Value::Num(-12345.0); }
                        assert!(replay(&changed).is_err(), "wrong callback entry geometry accepted"); entry = true;
                    }
                }
                if selection && geometry && callback && entry { return; }
            }
        }
    }
    panic!("missing live mutation coverage: selection={selection} geometry={geometry} callback={callback} entry={entry}");
}
