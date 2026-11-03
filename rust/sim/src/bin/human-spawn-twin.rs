//! Reproduce tools/human-spawn-twin.mjs's trace with the Rust spawn port.
use bfe_content::{json, Value};
use bfe_sim::battle::human::{self, Command, Event, State, World};
use bfe_sim::gen::{HumanAuto, HumanPhase, HumanPick, HumanSpawn};
use bfe_sim::store::{StrId, Strings};

fn bits(n: f64) -> String { format!("{:016x}", n.to_bits()) }
fn flag(b: bool) -> String { if b { "1" } else { "0" }.into() }
fn number(v: &Value) -> f64 {
    v.as_str().map(|s| f64::from_bits(u64::from_str_radix(s, 16).expect("number bits"))).unwrap_or_else(|| v.as_f64().unwrap())
}
fn array<'a>(v: &'a Value, key: &str) -> &'a [Value] { v.get(key).and_then(Value::as_array).unwrap_or(&[]) }
fn as_bool(v: &Value) -> Option<bool> { if let Value::Bool(b) = v { Some(*b) } else { None } }
fn boolean(v: &Value, key: &str) -> bool { v.get(key).and_then(as_bool).unwrap_or(false) }
fn intern(v: &Value, strings: &mut Strings) {
    match v {
        Value::String(s) => { strings.intern_str(s); }
        Value::Array(a) => for v in a.iter() { intern(v, strings); },
        Value::Object(o) => for (_, v) in o.entries() { intern(v, strings); },
        _ => {},
    }
}
fn id(strings: &Strings, text: &str) -> StrId {
    let units: Vec<u16> = text.encode_utf16().collect();
    StrId(strings.list.iter().position(|s| *s == units).expect("interned name") as u32)
}
fn name(strings: &Strings, v: Option<&Value>) -> Option<StrId> { v.and_then(Value::as_str).map(|s| id(strings, s)) }
fn phase(v: &Value) -> HumanPhase {
    match v.as_str().unwrap() {
        "unspawned" => HumanPhase::Unspawned, "alive" => HumanPhase::Alive,
        "dying" => HumanPhase::Dying, "awaiting" => HumanPhase::Awaiting, _ => unreachable!(),
    }
}
fn phase_name(p: HumanPhase) -> String {
    match p { HumanPhase::Unspawned => "unspawned", HumanPhase::Alive => "alive", HumanPhase::Dying => "dying", HumanPhase::Awaiting => "awaiting" }.into()
}
fn pick(strings: &Strings, v: Option<&Value>) -> Option<HumanPick> {
    let v = v.filter(|v| v.as_object().is_some())?;
    Some(HumanPick { class_name: name(strings, v.get("className")).unwrap(), post: name(strings, v.get("post")) })
}
fn auto(strings: &Strings, v: Option<&Value>) -> Option<HumanAuto> {
    let v = v.filter(|v| v.as_object().is_some())?;
    Some(HumanAuto { team: v.get("team").and_then(Value::as_f64).unwrap(), class_name: name(strings, v.get("className")).unwrap(), post: name(strings, v.get("post")) })
}
fn command(strings: &Strings, v: &Value) -> Option<Command> {
    match v.get("kind").and_then(Value::as_str)? {
        "team" => Command::team(v.get("team").and_then(Value::as_f64)?),
        "spawn" => {
            let class = name(strings, v.get("className"))?;
            let post = match v.get("post") {
                None | Some(Value::Null) => None,
                Some(v) => Some(name(strings, Some(v))?),
            };
            Command::spawn(class, post, strings)
        }
        "suicide" => Some(Command::Suicide), "cancel" => Some(Command::Cancel), _ => None,
    }
}

struct Twin<'s> {
    strings: &'s Strings, r: f64, k: f64, trace: Vec<String>,
    team: f64, dead: bool, over: bool, classic: bool, present: bool,
    unit: bool, timer: f64, can: bool, accept: bool, spawn_ok: bool,
}
impl Twin<'_> {
    fn text(&self, n: StrId) -> String { String::from_utf16_lossy(self.strings.units(n)) }
    fn optional(&self, n: Option<StrId>) -> String { n.map(|n| self.text(n)).unwrap_or_else(|| "-".into()) }
    fn line(&mut self, label: &str, parts: Vec<String>) {
        self.trace.push([vec![label.into(), bits(self.r), bits(self.k)], parts].concat().join(" "));
    }
    fn call(&mut self, name: &str, parts: Vec<String>) { self.line("call", [vec![name.into()], parts].concat()); }
    fn state_parts(&self, s: &State) -> Vec<String> {
        vec![phase_name(s.phase), bits(s.team), self.text(s.class_name), bits(s.class_index), self.optional(s.post), flag(s.can_switch_teams), flag(s.over)]
    }
    fn event(&mut self, label: &str, e: &Event) {
        let (name, parts) = match e {
            Event::AwaitingSpawn(s) => ("awaitingSpawn", self.state_parts(s)),
            Event::Spawned { team, class_name, post } => ("spawned", vec![bits(*team), self.text(*class_name), self.optional(*post)]),
            Event::TeamChanged { team, previous } => ("teamChanged", vec![bits(*team), bits(*previous)]),
            Event::PlayerDied { team, class_name } => ("playerDied", vec![bits(*team), self.text(*class_name)]),
        };
        self.line(label, [vec![name.into()], parts].concat());
    }
    fn dump(&mut self, s: &HumanSpawn, events: &[Event]) {
        let state = human::state(s, self);
        let mut parts = self.state_parts(&state);
        parts.extend([flag(s.spawned_once), bits(s.dead_time), flag(s.ended),
            s.auto.as_ref().map(|a| bits(a.team)).unwrap_or_else(|| "-".into()),
            self.optional(s.auto.as_ref().map(|a| a.class_name)), self.optional(s.auto.as_ref().and_then(|a| a.post)),
            self.optional(s.pending.as_ref().map(|p| p.class_name)), self.optional(s.pending.as_ref().and_then(|p| p.post))]);
        self.line("state", parts);
        self.line("world", vec![bits(self.team), flag(self.dead), flag(self.over), flag(self.classic), flag(self.present), flag(self.unit), bits(self.timer), flag(self.can), flag(self.accept), flag(self.spawn_ok)]);
        self.line("queue", vec![bits(events.len() as f64)]);
        for e in events { self.event("queued", e); }
    }
    fn set(&mut self, v: &Value) {
        if let Some(x) = v.get("teamNow").and_then(Value::as_f64) { self.team = x; }
        if let Some(x) = v.get("timer") { self.timer = number(x); }
        for (key, field) in [("deadNow", &mut self.dead), ("overNow", &mut self.over), ("classic", &mut self.classic),
            ("present", &mut self.present), ("unit", &mut self.unit), ("can", &mut self.can), ("accept", &mut self.accept), ("spawnOk", &mut self.spawn_ok)]
        {
            if let Some(x) = v.get(key).and_then(as_bool) { *field = x; }
        }
    }
}
impl World for Twin<'_> {
    fn team(&mut self) -> f64 { self.team }
    fn dead(&mut self) -> bool { self.dead }
    fn over(&mut self) -> bool { self.over }
    fn roster(&mut self, team: f64) -> Vec<StrId> {
        let names: &[&str] = if team == 1.0 { &["a", "b", "c"] } else if team == 2.0 { &["d", "e"] } else { &[] };
        names.iter().map(|s| id(self.strings, s)).collect()
    }
    fn classic(&mut self) -> bool { self.classic }
    fn character_spawn(&mut self) -> Option<(bool, f64)> { self.present.then_some((self.unit, self.timer)) }
    fn can_spawn(&mut self, _: StrId) -> bool { self.can }
    fn set_team(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>, team: f64) {
        self.call("team", vec![bits(team)]);
        let previous = self.team;
        self.team = team;
        human::team_changed(s, events, team, previous, self);
    }
    fn spawn(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>, choice: &HumanPick) {
        self.call("spawn", vec![self.text(choice.class_name), self.optional(choice.post)]);
        if !self.spawn_ok { return; }
        self.dead = false;
        self.unit = true;
        let strings = self.strings;
        human::spawned(s, events, choice.class_name, Some(choice.post.unwrap_or_else(|| id(strings, "cp1"))), strings, self);
    }
    fn ready(&mut self, _: &mut HumanSpawn, _: &mut Vec<Event>, choice: &HumanPick) -> bool {
        self.call("ready", vec![self.text(choice.class_name), self.optional(choice.post)]);
        self.accept
    }
    fn kill(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>) {
        self.call("kill", vec![]);
        self.dead = true;
        self.unit = false;
        self.timer = 4.0;
        human::died(s, events, self);
    }
    fn cancel(&mut self) { self.call("cancel", vec![]); }
    fn rejected(&mut self, class: StrId, team: f64) { self.call("rejected", vec![self.text(class), bits(team)]); }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args.len(), 3, "human-spawn-twin OPS.json TRACE.txt");
    let ops = json::parse(&std::fs::read_to_string(&args[1]).unwrap()).unwrap();
    let mut strings = Strings::default();
    for s in ["", "a", "b", "c", "d", "e", "cp1"] { strings.intern_str(s); }
    intern(&ops, &mut strings);
    let mut out = Vec::new();
    for (r, run) in array(&ops, "runs").iter().enumerate() {
        let config = run.get("config").unwrap();
        let class_name = name(&strings, config.get("className")).unwrap();
        let post = name(&strings, config.get("post"));
        let automatic = boolean(config, "auto").then_some(HumanAuto { team: 1.0, class_name, post });
        let mut s = human::new(class_name, automatic, post);
        let mut events = Vec::new();
        let mut w = Twin { strings: &strings, r: r as f64, k: -1.0, trace: Vec::new(), team: 1.0, dead: true, over: false,
            classic: boolean(config, "classic"), present: true, unit: false, timer: 0.0, can: true, accept: true, spawn_ok: true };
        w.dump(&s, &events);
        for (k, op) in array(run, "ops").iter().enumerate() {
            w.k = k as f64;
            if let Some(set) = op.get("set") { w.set(set); }
            if let Some(switchable) = op.get("switchable").and_then(as_bool) { s.can_switch_teams = switchable; }
            match op.get("kind").and_then(Value::as_str).unwrap() {
                "update" => {
                    let commands: Vec<Command> = array(op, "commands").iter().filter_map(|v| command(&strings, v)).collect();
                    human::update(&mut s, &mut events, number(op.get("dt").unwrap()), &commands, &mut w);
                }
                "spawned" => {
                    w.dead = false; w.unit = true;
                    human::spawned(&mut s, &mut events, name(&strings, op.get("className")).unwrap(), name(&strings, op.get("post")), &strings, &mut w);
                }
                "died" => { w.dead = true; w.unit = false; w.timer = 4.0; human::died(&mut s, &mut events, &mut w); }
                "authority" => {
                    let n = op.get("next").unwrap();
                    let next = human::Authority { class_name: name(&strings, n.get("className")),
                        post: name(&strings, n.get("post")), phase: phase(n.get("phase").unwrap()),
                        spawned_once: boolean(n, "spawnedOnce"), ended: boolean(n, "ended"), dead_time: number(n.get("deadTime").unwrap()),
                        pending: pick(&strings, n.get("pending")), auto: auto(&strings, n.get("auto")) };
                    for event in human::apply_authoritative(&mut s, next, &mut events, &strings, &mut w) { w.event("event", &event); }
                }
                _ => unreachable!(),
            }
            if op.get("flush").and_then(as_bool) != Some(false) {
                for event in human::flush(&mut events) { w.event("event", &event); }
            }
            w.dump(&s, &events);
        }
        out.extend(w.trace);
    }
    std::fs::write(&args[2], out.join("\n") + "\n").unwrap();
}
