//! Real Endor trap methods, captured with `--specs traps`. The trap starts
//! from its entry state and runs battle::traps; only foreign hull/presentation
//! callbacks are served. No frame-end trap state is applied to the port.
#![cfg(feature = "replay")]

use bfe_sim::battle::traps::{Phase, Settings, Trap, World};
use bfe_sim::store::{StrId, Strings};
use sim_replay::{check_num, check_value, check_vec3, replay_corpus, Arena, Call, Corpus, Event, Frame, Log, Node, Outcome, Value, V};

fn point(v: V) -> [f64; 3] { [v.at(0).num(), v.at(1).num(), v.at(2).num()] }
fn name(v: V, strings: &mut Strings) -> Option<StrId> {
    match v.value {
        Value::Str(s) => Some(strings.intern_str(s)),
        Value::Str16(s) => Some(strings.intern(s)),
        _ => None,
    }
}
fn read_trap(v: V) -> Result<Trap, String> {
    Ok(Trap {
        built: v.get("built").bool(), health: v.get("health").num(),
        phase: match v.get("phase").str() {
            Some("down") => Phase::Down, Some("reset") => Phase::Reset,
            Some("armed") => Phase::Armed, Some("swing") => Phase::Swing,
            _ => return Err(format!("invalid trap phase {}", v.get("phase").show())),
        },
        started: v.get("started").opt_num(), time: v.get("time").num(),
        sprung: v.get("sprung").bool(), tick: v.get("tick").num(), crushed: v.get("crushed").num(),
    })
}
fn settings(v: V, strings: &mut Strings) -> Settings {
    Settings {
        max_health: v.get("maxHealth").num(), reach: v.get("reach").num(), min_radius: v.get("minRadius").num(),
        death_time: v.get("deathTime").num(), build_frames: v.get("buildFrames").opt_num(), trigger_frames: v.get("triggerFrames").opt_num(),
        trigger_team: name(v.get("triggerTeam"), strings), sound: name(v.get("sound"), strings),
        death_effect: name(v.get("deathEffect"), strings), point: point(v.get("point")), effect_at: point(v.get("effectAt")),
    }
}
fn trap_value(t: &Trap, arena: &mut Arena) -> Value {
    let phase = match t.phase { Phase::Down => "down", Phase::Reset => "reset", Phase::Armed => "armed", Phase::Swing => "swing" };
    arena.object("", vec![
        ("built", Value::Bool(t.built)), ("health", Value::Num(t.health)), ("phase", Value::Str(phase.into())),
        ("started", t.started.map(Value::Num).unwrap_or(Value::Null)), ("time", Value::Num(t.time)),
        ("sprung", Value::Bool(t.sprung)), ("tick", Value::Num(t.tick)), ("crushed", Value::Num(t.crushed)),
    ])
}

#[derive(Clone)]
struct Hull<'f> {
    source: V<'f>, alive: bool, health: f64, dying: f64, attacker: Option<V<'f>>, visible: Option<bool>,
    team: f64, position: [f64; 3], radius: f64, sphere: bool,
}
impl<'f> Hull<'f> {
    fn read(v: V<'f>) -> Self {
        Self { source: v, alive: v.get("alive").bool(), health: v.get("health").num(), dying: v.get("dying").num(),
            attacker: (!v.get("lastAttacker").is_null()).then(|| v.get("lastAttacker")), visible: v.get("visible").opt_bool(),
            team: v.get("team").num(), position: point(v.get("position")), radius: v.get("radius").num(), sphere: v.get("sphere").bool() }
    }
    fn value(&self, arena: &mut Arena) -> Value {
        let identity = arena.copy(self.source.get("self"));
        let attacker = self.attacker.map(|v| arena.copy(v)).unwrap_or(Value::Null);
        let position = arena.array(self.position.map(Value::Num).to_vec());
        arena.object("", vec![
            ("self", identity), ("alive", Value::Bool(self.alive)), ("health", Value::Num(self.health)),
            ("dying", Value::Num(self.dying)), ("lastAttacker", attacker),
            ("visible", self.visible.map(Value::Bool).unwrap_or(Value::Null)), ("team", Value::Num(self.team)),
            ("position", position), ("radius", Value::Num(self.radius)), ("sphere", Value::Bool(self.sphere)),
        ])
    }
}

struct Served<'f, 's> {
    log: Log<'f>, strings: &'s Strings, index: usize, hulls: Vec<Hull<'f>>, shown: V<'f>,
    failure: Option<String>, kills: usize,
}
impl<'f> Served<'f, '_> {
    fn fail(&mut self, error: impl ToString) { self.failure.get_or_insert_with(|| error.to_string()); }
    fn call(&mut self, label: &str) -> Option<&'f Call> {
        match self.log.call(label) { Ok(call) => Some(call), Err(e) => { self.fail(e); None } }
    }
    fn number(&mut self, label: &str, value: V, actual: f64) {
        if let Err(e) = check_num(label, value, actual) { self.fail(e); }
    }
    fn text(&mut self, value: V, actual: StrId) {
        let same = match value.value {
            Value::Str(s) => s.encode_utf16().eq(self.strings.units(actual).iter().copied()),
            Value::Str16(s) => s.as_slice() == self.strings.units(actual),
            _ => false,
        };
        if !same { self.fail(format!("text argument differs: {}", value.show())); }
    }
    fn check_hulls(&mut self, expected: V) {
        let mut arena = Arena::default();
        let values = self.hulls.iter().map(|h| h.value(&mut arena)).collect();
        let actual = arena.array(values);
        if let Err(e) = check_value("hulls", expected, arena.view(&actual)) { self.fail(e); }
    }
}
impl World for Served<'_, '_> {
    type Vehicle = usize;
    fn alive(&mut self, vehicle: usize) -> bool { self.hulls[vehicle].alive }
    fn sphere(&mut self, vehicle: usize) -> ([f64; 3], f64) {
        if !self.hulls[vehicle].sphere {
            let h = &self.hulls[vehicle];
            return (h.position, if h.radius != 0.0 && !h.radius.is_nan() { h.radius } else { 0.0 });
        }
        let Some(call) = self.call(&format!("trap.sphere:{vehicle}")) else { return ([0.0; 3], 0.0) };
        // The scratch vector is an output parameter, not a sphere input.
        (call.out(0).vec3().expect("blastSphere output"), call.result().num())
    }
    fn side(&mut self, vehicle: usize) -> Option<StrId> {
        let call = self.call(&format!("trap.side:{}", self.index))?;
        self.number("side team", call.arg(0), self.hulls[vehicle].team);
        match call.result().value {
            Value::Str(s) => self.strings.list.iter().position(|u| s.encode_utf16().eq(u.iter().copied())).map(|i| StrId(i as u32)),
            Value::Str16(s) => self.strings.list.iter().position(|u| u == s).map(|i| StrId(i as u32)),
            _ => None,
        }
    }
    fn clear_attacker(&mut self, vehicle: usize) { self.hulls[vehicle].attacker = None; }
    fn set_health(&mut self, vehicle: usize, health: f64) {
        let Some(call) = self.call(&format!("trap.health:{vehicle}")) else { return };
        self.number("hull setHealth", call.arg(0), health);
        let extra = call.extra();
        // This check proves clear_attacker and previous victims' writes
        // happened before the foreign kill. It cannot be erased by serving
        // the callback's effects (including damage to another hull).
        self.check_hulls(extra.get("before"));
        if !extra.get("trapCalls").is_empty() {
            self.fail("hull callback reentered Trap.setHealth; replay needs its explicit callback operations");
        }
        if let Err(e) = check_value("trap state across hull callback", extra.get("trapBefore"), extra.get("trapAfter")) { self.fail(e); }
        self.hulls = extra.get("after").items().map(Hull::read).collect();
        self.kills += 1;
    }
    fn set_dying(&mut self, vehicle: usize, time: f64) { self.hulls[vehicle].dying = time; }
    fn hide_root(&mut self, vehicle: usize) { if self.hulls[vehicle].visible.is_some() { self.hulls[vehicle].visible = Some(false); } }
    fn show(&mut self, built: bool) {
        let Some(call) = self.call(&format!("trap.show:{}", self.index)) else { return };
        if call.arg(0).str() != Some(if built { "built" } else { "unbuilt" }) { self.fail("show state differs"); }
        self.shown = call.extra().get("shown");
    }
    fn sound(&mut self, name: StrId, at: [f64; 3]) {
        let Some(call) = self.call("trap.sound") else { return };
        if call.arg(0).str() != Some("sound") { self.fail("sound event type differs"); }
        self.text(call.arg(1), name);
        if let Err(e) = check_vec3("sound position", call.arg(2), at) { self.fail(e); }
    }
    fn effect(&mut self, name: StrId, at: [f64; 3]) {
        let Some(call) = self.call(&format!("trap.effect:{}", self.index)) else { return };
        self.text(call.arg(0), name);
        if let Err(e) = check_vec3("effect position", call.arg(1), at) { self.fail(e); }
    }
}

fn replay(frame: &Frame) -> Result<usize, String> {
    let before = frame.before();
    let mut strings = Strings::default();
    let settings = settings(before.get("settings"), &mut strings);
    for call in frame.calls().filter(|c| c.name.starts_with("trap.side:")) { name(call.result(), &mut strings); }
    let mut trap = read_trap(before.get("trap"))?;
    let vehicles: Vec<usize> = before.get("vehicles").items().map(|v| v.num() as usize).collect();
    let mut world = Served { log: Log::new(frame), strings: &strings, index: before.get("index").num() as usize,
        hulls: before.get("hulls").items().map(Hull::read).collect(), shown: before.get("shown"), failure: None, kills: 0 };
    if vehicles.iter().any(|&v| v >= world.hulls.len()) { return Err("trap frame names an unknown hull".into()); }
    match frame.name.as_str() {
        "Trap.step" => trap.step(frame.arg(0).num(), frame.arg(1).num(), &settings, &strings, &vehicles, &mut world),
        "Trap.setHealth" => trap.set_health(frame.arg(0).num(), &settings, &mut world),
        "Trap.crush" => trap.crush(&settings, &strings, &vehicles, &mut world),
        _ => return Err(format!("unknown trap method {}", frame.name)),
    }
    world.log.finish().map_err(|e| e.0)?;
    world.check_hulls(frame.after().get("hulls"));
    if let Some(error) = world.failure { return Err(error); }
    let mut arena = Arena::default();
    let actual = trap_value(&trap, &mut arena);
    check_value("trap", frame.after().get("trap"), arena.view(&actual)).map_err(|e| e.0)?;
    check_value("shown", frame.after().get("shown"), world.shown).map_err(|e| e.0)?;
    check_value("settings", frame.after().get("settings"), before.get("settings")).map_err(|e| e.0)?;
    Ok(world.kills)
}

#[test]
fn trap_methods_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let mut kills = 0;
    for method in ["Trap.step", "Trap.setHealth", "Trap.crush"] {
        let report = replay_corpus(&corpus, method, |_, frame| match replay(frame) {
            Ok(n) => { kills += n; Outcome::Same }, Err(e) => Outcome::Failed(e),
        }).unwrap();
        eprintln!("{}", report.summary());
        assert!(report.replayed() > 0, "no {method} frames in trap corpus");
        report.assert_clean();
    }
    assert!(kills > 0, "trap corpus contains no real hull kills");
    eprintln!("trap replay: {kills} checked hull kill callbacks (nested step/crush frames both count)");
}

#[test]
fn trap_replay_checks_calls_and_state() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let mut checked_call = false;
    let mut checked_state = false;
    for capture in corpus.captures() {
        let capture = capture.unwrap();
        for frame in capture.frames("Trap.crush") {
            let frame = frame.unwrap();
            if !frame.calls().any(|c| c.name.starts_with("trap.health:")) { continue; }
            replay(&frame).expect("clean crush before mutation");
            let mut changed = frame.clone();
            let index = changed.events.iter().position(|e| matches!(e, Event::Call(c) if c.name.starts_with("trap.health:"))).unwrap();
            changed.events.remove(index);
            assert!(replay(&changed).is_err(), "missing hull kill was not detected");
            let mut changed = frame.clone();
            if let Event::Call(call) = &mut changed.events[index] {
                let hull = call.name.strip_prefix("trap.health:").unwrap().parse::<usize>().unwrap();
                let id = call.extra().get("before").at(hull).node_id().unwrap();
                if let Node::Object { fields, .. } = &mut call.ret.arena.nodes[id as usize] {
                    fields.iter_mut().find(|(name, _)| name == "lastAttacker").unwrap().1 = Value::Bool(true);
                }
            }
            assert!(replay(&changed).is_err(), "pre-kill attacker mutation was not detected");
            checked_call = true;
            let mut changed = frame.clone();
            let id = changed.after().get("trap").node_id().unwrap();
            if let Node::Object { fields, .. } = &mut changed.exit.arena.nodes[id as usize] {
                let value = &mut fields.iter_mut().find(|(name, _)| name == "crushed").unwrap().1;
                if let Value::Num(n) = value { *n = f64::from_bits(n.to_bits() + 1); }
            }
            assert!(replay(&changed).is_err(), "one-ulp trap state mutation was not detected");
            checked_state = true;
            break;
        }
        if checked_call && checked_state { break; }
    }
    assert!(checked_call && checked_state, "no real crush for trap mutation checks");
}
