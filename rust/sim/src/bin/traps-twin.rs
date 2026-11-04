//! Reproduce tools/traps-twin.mjs's trace with the native trap simulation.
use bfe_content::{json, Value};
use bfe_sim::battle::traps::{Phase, Settings, Trap, World};
use bfe_sim::store::{StrId, Strings};

fn bits(n: f64) -> String { format!("{:016x}", n.to_bits()) }
fn flag(b: bool) -> String { if b { "1" } else { "0" }.into() }
fn optional(v: Option<f64>) -> String { v.map(bits).unwrap_or_else(|| "-".into()) }
fn boolean(v: Option<bool>) -> String { v.map(flag).unwrap_or_else(|| "-".into()) }
fn number(v: &Value) -> f64 {
    v.as_str().map(|s| f64::from_bits(u64::from_str_radix(s, 16).expect("number bits"))).unwrap_or_else(|| v.as_f64().expect("number"))
}
fn get<'a>(v: &'a Value, key: &str) -> &'a Value { v.get(key).expect(key) }
fn array(v: &Value) -> &[Value] { v.as_array().expect("array") }
fn truth(v: &Value) -> bool { matches!(v, Value::Bool(true)) }
fn point(v: &Value) -> [f64; 3] { let a = array(v); [number(&a[0]), number(&a[1]), number(&a[2])] }
fn phase(v: &str) -> Phase {
    match v { "down" => Phase::Down, "reset" => Phase::Reset, "armed" => Phase::Armed, "swing" => Phase::Swing, _ => panic!("phase: {v}") }
}
fn phase_name(v: Phase) -> String {
    match v { Phase::Down => "down", Phase::Reset => "reset", Phase::Armed => "armed", Phase::Swing => "swing" }.into()
}
fn maybe(v: &Value) -> Option<f64> { (!matches!(v, Value::Null)).then(|| number(v)) }
fn name(v: &Value, key: &str, strings: &mut Strings) -> Option<StrId> { get(v, key).as_str().map(|s| strings.intern_str(s)) }

struct Hull {
    alive: bool,
    team: f64,
    position: [f64; 3],
    radius: f64,
    up: f64,
    dying: f64,
    attacker: Option<bool>,
    visible: Option<bool>,
    sphere: bool,
    cascade: Option<usize>,
}

impl Hull {
    fn read(v: &Value) -> Self {
        Self {
            alive: truth(get(v, "alive")), team: number(get(v, "team")), position: point(get(v, "position")),
            radius: number(get(v, "radius")), up: number(get(v, "up")), dying: number(get(v, "dying")),
            attacker: Some(true), visible: truth(get(v, "root")).then_some(true), sphere: truth(get(v, "sphere")),
            cascade: get(v, "cascade").as_f64().map(|n| n as usize),
        }
    }
}

struct Fixture<'a> {
    r: usize,
    k: isize,
    hulls: Vec<Hull>,
    built: bool,
    lines: Vec<String>,
    strings: &'a Strings,
    sides: [StrId; 3],
}

impl Fixture<'_> {
    fn line(&mut self, kind: &str, parts: impl IntoIterator<Item = String>) {
        let mut line = vec![kind.to_owned(), bits(self.r as f64), bits(self.k as f64)];
        line.extend(parts);
        self.lines.push(line.join(" "));
    }
    fn call(&mut self, parts: impl IntoIterator<Item = String>) { self.line("call", parts); }
    fn text(&self, name: StrId) -> String { String::from_utf16_lossy(self.strings.units(name)) }
    fn dump(&mut self, trap: &Trap) {
        let mut state = vec![flag(trap.built), bits(trap.health), phase_name(trap.phase), optional(trap.started),
            bits(trap.time), flag(trap.sprung), bits(trap.tick), bits(trap.crushed)];
        state.extend(trap.state_record().map(bits));
        self.line("state", state);
        self.line("bodies", [flag(self.built), flag(!self.built)]);
        for i in 0..self.hulls.len() {
            let h = &self.hulls[i];
            let mut fields = vec![bits(i as f64), flag(h.alive), bits(h.dying), boolean(h.attacker), boolean(h.visible)];
            fields.extend(h.position.map(bits));
            self.line("hull", fields);
        }
    }
}

impl World for Fixture<'_> {
    type Vehicle = usize;
    fn alive(&mut self, i: usize) -> bool { self.hulls[i].alive }
    fn sphere(&mut self, i: usize) -> ([f64; 3], f64) {
        let sphere = self.hulls[i].sphere;
        if sphere { self.call(["sphere".into(), bits(i as f64)]); }
        let h = &self.hulls[i];
        let mut p = h.position;
        if sphere { p[1] += h.up; }
        let radius = if sphere || (h.radius != 0.0 && !h.radius.is_nan()) { h.radius } else { 0.0 };
        (p, radius)
    }
    fn side(&mut self, i: usize) -> Option<StrId> {
        let team = self.hulls[i].team;
        self.call(["side".into(), bits(team)]);
        if team >= 1.0 && team <= 3.0 && team.fract() == 0.0 { Some(self.sides[team as usize - 1]) } else { None }
    }
    fn clear_attacker(&mut self, _: &mut Trap, i: usize) {
        self.hulls[i].attacker = None;
        self.call(["attacker".into(), bits(i as f64)]);
    }
    fn set_health(&mut self, _: &mut Trap, i: usize, health: f64) {
        self.call(["health".into(), bits(i as f64), bits(health)]);
        if !(health > 0.0) { self.hulls[i].alive = false; self.hulls[i].dying = 3.0; }
        if let Some(other) = self.hulls[i].cascade { self.hulls[other].alive = false; self.hulls[other].position[0] += 10000.0; }
    }
    fn set_dying(&mut self, _: &mut Trap, i: usize, time: f64) {
        self.hulls[i].dying = time;
        self.call(["dying".into(), bits(i as f64), bits(time)]);
    }
    fn hide_root(&mut self, _: &mut Trap, i: usize) {
        if self.hulls[i].visible.is_some() {
            self.hulls[i].visible = Some(false);
            self.call(["visible".into(), bits(i as f64), flag(false)]);
        }
    }
    fn show(&mut self, _: &mut Trap, built: bool) { self.built = built; self.call(["show".into(), flag(built)]); }
    fn sound(&mut self, _: &mut Trap, name: StrId, at: [f64; 3]) {
        let mut parts = vec!["sound".into(), self.text(name)]; parts.extend(at.map(bits)); self.call(parts);
    }
    fn effect(&mut self, _: &mut Trap, name: StrId, at: [f64; 3]) {
        let mut parts = vec!["effect".into(), self.text(name)]; parts.extend(at.map(bits)); self.call(parts);
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "traps-twin OPS TRACE");
    let runs = json::parse(&std::fs::read_to_string(&args[1]).expect("operations read")).expect("operations JSON");
    let mut lines = Vec::new();
    for (r, run) in array(&runs).iter().enumerate() {
        let c = get(run, "config");
        let mut strings = Strings::default();
        let sides = ["all", "imp", "ewk"].map(|s| strings.intern_str(s));
        let settings = Settings {
            max_health: number(get(c, "maxHealth")), reach: number(get(c, "reach")), min_radius: number(get(c, "minRadius")),
            death_time: number(get(c, "deathTime")), build_frames: maybe(get(c, "buildFrames")), trigger_frames: maybe(get(c, "triggerFrames")),
            trigger_team: name(c, "triggerTeam", &mut strings), sound: name(c, "sound", &mut strings), death_effect: name(c, "deathEffect", &mut strings),
            point: point(get(c, "point")), effect_at: point(get(c, "effectAt")),
        };
        let mut trap = Trap::default();
        let mut world = Fixture { r, k: -1, hulls: Vec::new(), built: false, lines: Vec::new(), strings: &strings, sides };
        world.dump(&trap);
        for (k, op) in array(get(run, "ops")).iter().enumerate() {
            world.k = k as isize;
            let fleet: Vec<_> = (0..world.hulls.len()).collect();
            match get(op, "kind").as_str().expect("kind") {
                "health" => trap.set_health(number(get(op, "value")), &settings, &mut world),
                "step" => trap.step(number(get(op, "dt")), number(get(op, "tick")), &settings, &strings, &fleet, &mut world),
                "begin" => trap.begin(phase(get(op, "phase").as_str().unwrap()), maybe(get(op, "tick"))),
                "crush" => trap.crush(&settings, &strings, &fleet, &mut world),
                "hulls" => world.hulls = array(get(op, "values")).iter().map(Hull::read).collect(),
                "catches" => for i in fleet {
                    let caught = trap.catches(&settings, i, &mut world);
                    world.line("catches", [bits(i as f64), flag(caught)]);
                },
                kind => panic!("operation: {kind}"),
            }
            world.dump(&trap);
        }
        lines.extend(world.lines);
    }
    std::fs::write(&args[2], format!("{}\n", lines.join("\n"))).expect("trace written");
}
