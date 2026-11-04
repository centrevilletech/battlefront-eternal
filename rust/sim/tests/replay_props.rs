//! Native building and supply methods, with callback entry states checked
//! before foreign effects are served. No native-owned state is restored.
#![cfg(feature = "replay")]
use bfe_sim::battle::props::{Building, Identity, Kind, Model, Settings, State, Station, Supply, Target, Weapon, World};
use sim_replay::{check_num, check_value, check_vec3, replay_corpus, Arena, Call, Corpus, Event, Frame, Log, Node, Outcome, Value, V};
use three::{Matrix4, Vector3};

fn point(v: V) -> Vector3 { Vector3::new(v.at(0).num(), v.at(1).num(), v.at(2).num()) }
fn xyz(p: Vector3) -> [f64; 3] { [p.x, p.y, p.z] }
fn read_identity<'a>(v: V<'a>, refs: &mut Vec<V<'a>>) -> Identity {
    match v.value { Value::Null => Identity::Null, Value::Undefined => Identity::Undefined, _ => {
        refs.push(v); Identity::Object(refs.len() - 1)
    } }
}
fn read_building<'a>(v: V<'a>, refs: &mut Vec<V<'a>>) -> Building {
    Building { state: match v.get("state").str().unwrap() { "built" => State::Built, "unbuilt" => State::Unbuilt, "destroyed" => State::Destroyed, s => panic!("building state {s}") },
        health: v.get("health").num(), add_health: v.get("addHealth").num(),
        last_attacker: read_identity(v.get("lastAttacker"), refs), target_generation: v.get("targetGeneration").opt_num() }
}
fn settings(v: V) -> Settings {
    let mut matrix = Matrix4::default();
    matrix.from_array(&v.get("matrix").items().map(|x| x.num()).collect::<Vec<_>>(), 0);
    Settings { kind: match v.get("kind").str() { Some("turret") => Kind::Turret, Some("objective") => Kind::Objective, _ => Kind::Building },
        max_health: v.get("maxHealth").num(), respawn_time: v.get("respawnTime").num(), built_add_health: v.get("builtAddHealth").num(),
        regenerates: v.get("regenerates").bool(), has_destroyed: v.get("destroyed").bool(), has_unbuilt: v.get("unbuilt").bool(),
        has_crew: v.get("crew").bool(), has_explosion: v.get("explosion").bool(), has_damaged: v.get("damaged").bool(), matrix,
        models: std::array::from_fn(|i| { let m = v.get("models").at(i); (!m.is_nullish()).then(|| Model {
            sphere: (!m.get("sphere").is_nullish()).then(|| std::array::from_fn(|i| m.get("sphere").at(i).num())),
            bounds: (!m.get("bounds").is_nullish()).then(|| [point(m.get("bounds").at(0)), point(m.get("bounds").at(1))]),
        }) }) }
}
fn supply(v: V) -> Supply {
    Supply { position: point(v.get("position")), radius: v.get("radius").num(), delay: v.get("delay").num(),
        health: v.get("health").num(), ammo: v.get("ammo").num(), vehicle_health: v.get("vehicleHealth").num(), vehicle_ammo: v.get("vehicleAmmo").num() }
}
fn target(v: V, id: usize) -> Target {
    Target { id, position: point(v.get("position")), vehicle: v.get("vehicle").bool(), alive: v.get("alive").bool(), player: v.get("player").bool(),
        radius: v.get("radius").num(), health: v.get("health").num(), max_health: v.get("maxHealth").num(),
        weapons: v.get("weapons").items().map(|w| Weapon { bottomless: w.get("bottomless").bool(), ammo: w.get("ammo").num(),
            clip: w.get("clip").num(), per_shot: w.get("perShot").num(), reserve: w.get("reserve").num() }).collect() }
}
fn id_value(id: Identity, refs: &[V], arena: &mut Arena) -> Value {
    match id { Identity::Undefined => Value::Undefined, Identity::Null => Value::Null, Identity::Object(i) => arena.copy(refs[i]) }
}
fn building_value(b: &Building, refs: &[V], arena: &mut Arena) -> Value {
    let attacker = id_value(b.last_attacker, refs, arena);
    arena.object("", vec![("state", Value::str(b.state.name())), ("health", Value::Num(b.health)), ("addHealth", Value::Num(b.add_health)),
        ("lastAttacker", attacker), ("targetGeneration", b.target_generation.map(Value::Num).unwrap_or(Value::Null))])
}
fn station_value(s: &Station, refs: &[V], arena: &mut Arena) -> Value {
    let value = building_value(&s.building, refs, arena);
    for (k, v) in [("mode", Value::str(if s.active_mode { "active" } else { "waiting" })), ("active", Value::Num(s.active)), ("pulse", Value::Num(s.pulse)),
        ("servedPlayer", Value::Bool(s.served_player)), ("servedHealth", Value::Bool(s.served_health)), ("servedAmmo", Value::Bool(s.served_ammo))] { arena.assign(&value, k, v); }
    value
}
fn target_value(t: &Target, arena: &mut Arena) -> Value {
    let position = arena.array(xyz(t.position).map(Value::Num).to_vec());
    let weapons: Vec<_> = t.weapons.iter().map(|w| arena.object("", vec![("bottomless", Value::Bool(w.bottomless)), ("ammo", Value::Num(w.ammo)),
        ("clip", Value::Num(w.clip)), ("perShot", Value::Num(w.per_shot)), ("reserve", Value::Num(w.reserve))])).collect();
    let weapons = arena.array(weapons);
    arena.object("", vec![("position", position), ("vehicle", Value::Bool(t.vehicle)), ("alive", Value::Bool(t.alive)), ("player", Value::Bool(t.player)),
        ("radius", Value::Num(t.radius)), ("health", Value::Num(t.health)), ("maxHealth", Value::Num(t.max_health)), ("weapons", weapons)])
}
struct Served<'f> { log: Log<'f>, index: usize, refs: Vec<V<'f>>, failure: Option<String>, callbacks: usize }
impl<'f> Served<'f> {
    fn fail(&mut self, e: impl ToString) { self.failure.get_or_insert_with(|| e.to_string()); }
    fn call(&mut self, method: &str) -> Option<&'f Call> {
        let name = if method == "crew" || method == "sound" { format!("props.{method}") } else { format!("props.{method}:{}", self.index) };
        match self.log.call(&name) { Ok(call) => { self.callbacks += 1; Some(call) }, Err(e) => { self.fail(e); None } }
    }
    fn check_building(&mut self, b: &Building, v: V) {
        let mut arena = Arena::default(); let actual = building_value(b, &self.refs, &mut arena);
        if let Err(e) = check_value("building", v, arena.view(&actual)) { self.fail(e); }
    }
    fn check_station(&mut self, b: &Station, v: V) {
        let mut arena = Arena::default(); let actual = station_value(b, &self.refs, &mut arena);
        if let Err(e) = check_value("station", v, arena.view(&actual)) { self.fail(e); }
    }
    fn check_target(&mut self, t: &Target, v: V) {
        let mut arena = Arena::default(); let actual = target_value(t, &mut arena);
        if let Err(e) = check_value("target", v, arena.view(&actual)) { self.fail(e); }
    }
    fn number(&mut self, v: V, n: f64) { if let Err(e) = check_num("argument", v, n) { self.fail(e); } }
    fn identity(&mut self, v: V, id: Identity) {
        let mut arena = Arena::default(); let actual = id_value(id, &self.refs, &mut arena);
        if let Err(e) = check_value("identity", v, arena.view(&actual)) { self.fail(e); }
    }
    fn callback(&mut self, method: &str, b: &Building) -> Option<&'f Call> {
        let call = self.call(method)?;
        self.check_building(b, call.extra().get("before"));
        // These notifications do not own building state. A callback
        // reentering it must get explicit operations, never an afterstate.
        self.check_building(b, call.extra().get("after"));
        Some(call)
    }
    fn receiver(&mut self, v: V) {
        if let Err(e) = check_value("receiver", v, self.log.frame.enter.root().get("t")) { self.fail(e); }
    }
}
impl World for Served<'_> {
    fn show(&mut self, b: &Building) {
        if let Some(call) = self.call("show") {
            if call.arg(0).str() != Some(b.state.name()) { self.fail("show argument differs"); }
            self.check_building(b, call.extra().get("after"));
        }
    }
    fn crew_destroyed(&mut self, b: &mut Building) { if let Some(call) = self.callback("crew", b) { self.receiver(call.arg(0)); } }
    fn destroyed(&mut self, b: &mut Building) { if let Some(call) = self.callback("onDestroyed", b) { self.receiver(call.arg(0)); } }
    fn explode(&mut self, b: &mut Building, position: Vector3) {
        if let Some(call) = self.callback("onExplode", b) {
            if let Err(e) = check_value("explosion definition", call.arg(0), self.log.frame.before().get("settings").get("explosionDef")) { self.fail(e); }
            if let Err(e) = check_vec3("explosion position", call.arg(1), xyz(position)) { self.fail(e); }
        }
    }
    fn damaged(&mut self, b: &mut Building, amount: f64, attacker: Identity, hit: Identity) {
        if let Some(call) = self.callback("onDamaged", b) {
            self.receiver(call.arg(0)); self.number(call.arg(1), amount); self.identity(call.arg(2), attacker); self.identity(call.arg(3), hit);
        }
    }
    fn served(&mut self, s: &Station, target: &mut Target, gave: u8) {
        if let Some(call) = self.call("onServe") {
            self.number(call.arg(1), gave as f64);
            self.check_station(s, call.extra().get("before")); self.check_station(s, call.extra().get("after"));
            self.check_target(target, call.extra().get("targetBefore")); self.check_target(target, call.extra().get("targetAfter"));
            let before = self.log.frame.before();
            let soldiers: Vec<_> = before.get("soldiers").items().map(|v| v.num() as usize).collect();
            let at = soldiers.iter().position(|&id| id == target.id).expect("served soldier");
            let original = self.log.frame.arg(1);
            let expected = if matches!(original.node(), Some(Node::Array(_))) { original.at(at) } else { original };
            if let Err(e) = check_value("served identity", call.arg(0), expected) { self.fail(e); }
        }
    }
    fn serve_sound(&mut self, _: &Station, first: Option<usize>) {
        if let Some(call) = self.call("sound") {
            if call.arg(0).str() != Some("serve") { self.fail("sound type differs"); }
            self.receiver(call.arg(1));
            if let Some(id) = first {
                let before = self.log.frame.before();
                let at = before.get("soldiers").items().position(|v| v.num() as usize == id).unwrap();
                let original = self.log.frame.arg(1);
                let expected = if matches!(original.node(), Some(Node::Array(_))) { original.at(at) } else { original };
                if let Err(e) = check_value("sound first target", call.arg(2), expected) { self.fail(e); }
            } else if !call.arg(2).is_null() { self.fail("sound expected no soldier"); }
        }
    }
}
fn replay(frame: &Frame) -> Result<usize, String> {
    let before = frame.before();
    let mut refs = Vec::new();
    let mut building = read_building(before.get("building"), &mut refs);
    let settings = settings(before.get("settings"));
    let attacker = if frame.arg(1).is_nullish() { Identity::Null } else { read_identity(frame.arg(1), &mut refs) };
    let hit = if frame.arg(2).is_nullish() { Identity::Null } else { read_identity(frame.arg(2), &mut refs) };
    let mut world = Served { log: Log::new(frame), index: before.get("index").num() as usize, refs, failure: None, callbacks: 0 };
    let mut targets: Vec<_> = before.get("targets").items().enumerate().map(|(i, t)| target(t, i)).collect();
    let soldiers: Vec<_> = before.get("soldiers").items().map(|v| v.num() as usize).collect();
    let vehicles: Vec<_> = before.get("vehicles").items().map(|v| v.num() as usize).collect();
    let mut station = (!before.get("station").is_null()).then(|| {
        let v = before.get("station");
        Station { building: building.clone(), active_mode: v.get("mode").str() == Some("active"), active: v.get("active").num(), pulse: v.get("pulse").num(),
            served_player: v.get("servedPlayer").bool(), served_health: v.get("servedHealth").bool(), served_ammo: v.get("servedAmmo").bool() }
    });
    match frame.name.as_str() {
        "Building.setHealth" => building.set_health(frame.arg(0).num(), &settings, &mut world),
        "Building.damage" => building.damage_base(frame.arg(0).num(), attacker, &settings, &mut world),
        "Turret.damage" => building.damage(frame.arg(0).num(), attacker, hit, &settings, &mut world),
        "Building.regenerate" => building.regenerate(frame.arg(0).num(), &settings, &mut world),
        "PowerupStation.update" => {
            let s = station.as_mut().unwrap();
            s.update(frame.arg(0).num(), &settings, &supply(before.get("supply")), &mut targets, &soldiers, &vehicles, &mut world);
            building = s.building.clone();
        }
        "PowerupStation.serve" => {
            let result = Station::serve(&supply(before.get("supply")), &mut targets[0]);
            world.number(frame.exit.root().get("r"), result as f64);
        }
        _ => return Err(format!("unknown props method {}", frame.name)),
    }
    world.log.finish().map_err(|e| e.0)?;
    world.check_building(&building, frame.after().get("building"));
    if let Some(mut station) = station { station.building = building; world.check_station(&station, frame.after().get("station")); }
    for (i, target) in targets.iter().enumerate() { world.check_target(target, frame.after().get("targets").at(i)); }
    for key in ["settings", "supply", "soldiers", "vehicles"] { check_value(key, frame.after().get(key), before.get(key)).map_err(|e| e.0)?; }
    if let Some(error) = world.failure { Err(error) } else { Ok(world.callbacks) }
}

#[test]
fn props_methods_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let mut frames = 0; let mut callbacks = 0;
    for method in ["Building.setHealth", "Building.damage", "Building.regenerate", "Turret.damage", "PowerupStation.update", "PowerupStation.serve"] {
        let report = replay_corpus(&corpus, method, |_, frame| match replay(frame) {
            Ok(n) => { callbacks += n; Outcome::Same }, Err(e) => Outcome::Failed(e),
        }).unwrap();
        frames += report.replayed(); eprintln!("{}", report.summary()); report.assert_clean();
    }
    assert!(frames > 0, "no props frames");
    assert!(callbacks > 0, "no props callbacks");
    eprintln!("props replay: {frames} exact frames, {callbacks} checked callbacks");
}

#[test]
fn props_replay_rejects_state_and_callback_mutations() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let mut state = false; let mut callback = false; let mut entry = false;
    for capture in corpus.captures() {
        let capture = capture.unwrap();
        for name in ["Building.setHealth", "PowerupStation.update", "Turret.damage"] {
            for frame in capture.frames(name) {
                let frame = frame.unwrap(); replay(&frame).expect("unmodified props frame");
                if !state {
                    let mut changed = frame.clone();
                    let id = changed.after().get("building").node_id().unwrap();
                    if let Node::Object { fields, .. } = &mut changed.exit.arena.nodes[id as usize] {
                        fields.iter_mut().find(|(k, _)| k == "health").unwrap().1 = Value::Num(-1234567.0);
                    }
                    assert!(replay(&changed).is_err(), "wrong building health accepted"); state = true;
                }
                if !callback && !frame.events.is_empty() {
                    let mut changed = frame.clone(); changed.events.remove(0);
                    assert!(replay(&changed).is_err(), "missing callback accepted"); callback = true;
                }
                if !entry {
                    let mut changed = frame.clone();
                    if let Some(Event::Call(call)) = changed.events.iter_mut().find(|e| matches!(e, Event::Call(c) if c.extra().has("before"))) {
                        let id = call.extra().get("before").node_id().unwrap();
                        if let Node::Object { fields, .. } = &mut call.ret.arena.nodes[id as usize] {
                            fields.iter_mut().find(|(k, _)| k == "health").unwrap().1 = Value::Num(-1234567.0);
                        }
                        assert!(replay(&changed).is_err(), "wrong callback entry health accepted"); entry = true;
                    }
                }
                if state && callback && entry { return; }
            }
        }
    }
    panic!("missing mutation coverage: state={state} callback={callback} entry={entry}");
}
