// A craft's flight (src/flight/, Vehicle.fly), replayed against every
// flyer.tick, flyer.aimTick and flyer.render frame of the corpus
// BFE_REPLAY_CORPUS names (tools/sim-capture.mjs --specs flight). Each frame
// starts from the craft as captured (its store export, or its pose for the
// pose frames); its calls to the rest of the battle (the collision world,
// the water, the death regions, the battle's hooks, a unit's aim point) are
// served from the capture with every argument checked bit for bit, and what
// targeting reads of the registry from the frame's state (again after any
// call that changed it); the craft it ends with, and what its targeting
// left in its weapons, are compared with the capture's. A ground hull's
// frames are the hulls package's (tests/replay_vehicles.rs): skipped here.
// Without a corpus the tests pass without replaying.
#![cfg(feature = "replay")]

use bfe_sim::flight::config::{Config, Tuning, Tunings, TUNING_KEYS};
use bfe_sim::flight::model::FlightInput;
use bfe_sim::flight::state::GearClip;
use bfe_sim::flight::{Environment, Flight, FlightClass, FlightWorld, Member, Team, Zone};
use bfe_sim::gen::{OrdnanceOwner, Replay, Vehicle};
use bfe_sim::vehicles::class::HullClass;
use bfe_sim::vehicles::replay::{body_of, pose_from_value, pose_to_value};
use bfe_sim::vehicles::{BodyId, Ground, Hit, Hull, Polygon, VehicleWorld};
use sim_replay::{check_bool, check_num, check_vec3, diff, replay_corpus, same_number, Arena, Call, Corpus, Log, Node, Outcome, Value, V};
use std::collections::{BTreeMap, HashMap};
use three::{Matrix4, Quaternion, Vector3};

fn vec3(v: V) -> Vector3 {
    let a = v.vec3().unwrap_or_else(|| panic!("not a Vector3: {}", v.show()));
    Vector3::new(a[0], a[1], a[2])
}

fn vec(v: &Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn nums3(v: V) -> [f64; 3] {
    [v.at(0).num(), v.at(1).num(), v.at(2).num()]
}

/// A JavaScript value as a number the way flight reads its input: a
/// number as is, a boolean as 0 or 1, anything else (undefined) as NaN.
fn js_num(v: V) -> f64 {
    match v.value {
        Value::Num(x) => *x,
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::Undefined => f64::NAN,
        // null is 0 to `||` and to arithmetic alike.
        Value::Null => 0.0,
        _ => panic!("an input value that is no number or boolean: {}", v.show()),
    }
}

/// An input object as flight reads it.
fn flight_input(v: V) -> FlightInput {
    let stick = |key: &str| {
        let s = v.get(key);
        if s.is_nullish() {
            None
        } else {
            Some(js_num(s))
        }
    };
    FlightInput {
        forward: js_num(v.get("forward")),
        right: js_num(v.get("right")),
        turn: js_num(v.get("turn")),
        pitch: js_num(v.get("pitch")),
        up: js_num(v.get("up")),
        down: js_num(v.get("down")),
        takeoff: js_num(v.get("takeoff")),
        stick_x: stick("stickX"),
        stick_y: stick("stickY"),
        boost: js_num(v.get("boost")),
        trick: js_num(v.get("trick")),
        immelmann: js_num(v.get("immelmann")),
        roll_lock: js_num(v.get("rollLock")),
        target_lock: js_num(v.get("targetLock")),
        bot: js_num(v.get("bot")),
        response_curve: js_num(v.get("responseCurve")),
        no_assist: matches!(v.get("steeringAssist").value, Value::Bool(false)),
    }
}

fn team_of(v: V) -> Team {
    if v.is_nullish() {
        None
    } else {
        Some(v.num())
    }
}

fn members_of(v: V) -> Vec<Member> {
    v.items()
        .map(|t| Member {
            id: t.get("id").num(),
            generation: t.get("generation").opt_num(),
            alive: t.get("alive").truthy(),
            team: team_of(t.get("team")),
            own: t.get("own").bool(),
            unit: t.get("unit").bool(),
            body: if t.get("body").is_nullish() { None } else { Some(body_of(t.get("body"))) },
            point: if t.get("point").is_nullish() { Vector3::default() } else { vec3(t.get("point")) },
        })
        .collect()
}

fn relations_of(v: V) -> Vec<(Team, f64)> {
    v.items().map(|p| (team_of(p.at(0)), p.at(1).num())).collect()
}

fn environment_of(world: V) -> Environment {
    let env = world.get("env");
    let bounds = env.get("bounds");
    Environment {
        ceiling: env.get("ceiling").opt_num(),
        max_fly_height: world.get("maxFlyHeight").num(),
        bounds: if bounds.is_nullish() { None } else { Some((nums3(bounds.get("min")), nums3(bounds.get("max")))) },
        space: env.get("space").truthy(),
        landing_zones: if env.get("landingZones").is_nullish() {
            None
        } else {
            Some(
                env.get("landingZones")
                    .items()
                    .map(|z| {
                        let min = nums3(z.get("min"));
                        Zone { min, max: nums3(z.get("max")), floor_y: z.get("floorY").opt_num().unwrap_or(min[1]) }
                    })
                    .collect(),
            )
        },
        assists: world.get("assists").bool(),
    }
}

fn tunings_of(world: V) -> Tunings {
    let mut t = Tunings::default();
    let tuning = world.get("tuning");
    if !tuning.is_nullish() {
        // (The classes the frame reads: a craft's own, a missile's.)
        for (i, name) in FlightClass::NAMES.iter().enumerate() {
            let c = tuning.get(name);
            if c.is_nullish() {
                continue;
            }
            let mut row = Tuning::of(FlightClass::from_u8(i as u8).unwrap());
            for key in TUNING_KEYS {
                let x = c.get(key).num();
                // Set whatever the capture holds (a retune is checked by
                // retune's own rules where it was made).
                if !row.retune(key, x) {
                    assert!(same_number(row.get(key).unwrap(), x), "a tuning the port cannot hold: {name}.{key} = {x}");
                }
            }
            t.0[i] = row;
        }
    }
    t
}

// A frame's calls, served from its capture, and what targeting reads.
struct Served<'f> {
    log: Log<'f>,
    hull: usize,
    water: Option<f64>,
    env: Environment,
    tunings: Tunings,
    members: Vec<Member>,
    relations: Vec<(Team, f64)>,
    /// The team the relations are of (the craft's).
    team: Team,
    /// The last aim point asked or read (targeting.js's scratch `point`),
    /// which the next unit's aimPoint is handed.
    point: Option<Vector3>,
    error: Option<String>,
}

impl<'f> Served<'f> {
    fn fail(&mut self, what: String) {
        if self.error.is_none() {
            self.error = Some(format!("{}: {what}", self.log.frame.place()));
        }
    }

    fn call(&mut self, name: &str) -> Option<&'f Call> {
        if self.error.is_some() {
            return None;
        }
        match self.log.call(name) {
            Ok(call) => Some(call),
            Err(e) => {
                self.error = Some(e.0);
                None
            }
        }
    }

    fn check(&mut self, result: Result<(), sim_replay::Mismatch>) {
        if let Err(e) = result {
            self.fail(e.0);
        }
    }

    fn check_hull(&mut self, what: &str, v: V) {
        if v.handle_num("vehicle") != Some(self.hull as f64) {
            self.fail(format!("{what}: the capture has {}, the port hull {}", v.show(), self.hull));
        }
    }

    fn check_body(&mut self, what: &str, v: V, mine: Option<BodyId>) {
        let theirs = if v.is_nullish() { None } else { Some(body_of(v)) };
        if theirs != mine {
            self.fail(format!("{what}: the capture has {}, the port {mine:?}", v.show()));
        }
    }

    /// A call that can change the registry: what it left, as its extra holds it.
    fn changed(&mut self, call: &Call) {
        let x = call.extra();
        if !x.get("targets").is_nullish() {
            self.members = members_of(x.get("targets"));
            self.relations = relations_of(x.get("relations"));
        }
    }
}

impl<'f> VehicleWorld for Served<'f> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> {
        let call = self.call("collision.cast")?;
        self.check(check_vec3("cast origin", call.arg(0), vec(origin)));
        self.check(check_vec3("cast direction", call.arg(1), vec(direction)));
        self.check(check_num("cast far", call.arg(2), far));
        self.check_body("cast ignoreBody", call.arg(4), ignore);
        let r = call.result();
        if r.is_null() {
            return None;
        }
        let body = r.get("body");
        Some(Hit {
            distance: r.get("distance").num(),
            point: vec3(r.get("point")),
            normal: vec3(r.get("normal")),
            terrain: r.get("terrain").truthy(),
            body: if body.is_nullish() { None } else { Some(body_of(body)) },
        })
    }

    fn push_walls(&mut self, _: usize, _: &[Vector3], _: f64, _: &Quaternion, _: Option<&Quaternion>, _: &Vector3, _: &mut Vector3, _: &mut Vector3, _: bool) -> bool {
        self.fail("a craft pushed against walls".into());
        false
    }

    fn ground(&mut self, _: f64, _: f64, _: f64) -> Option<Ground> {
        self.fail("a craft asked for the ground".into());
        None
    }

    fn spring_polygons<'a>(&'a mut self, _: usize, _: usize, _: &Vector3, _: f64, out: &'a mut Vec<Polygon>) -> bfe_sim::vehicles::SpringPolygons<'a> {
        out.clear();
        self.fail("a craft asked for its springs' polygons".into());
        bfe_sim::vehicles::SpringPolygons::Recorded(out)
    }

    fn move_body(&mut self, body: BodyId, matrix: &Matrix4) {
        let Some(call) = self.call("collision.moveBody") else { return };
        self.check_body("moveBody body", call.arg(0), Some(body));
        match call.arg(1).node() {
            Some(Node::Mat4(e)) if e.iter().zip(matrix.elements.iter()).all(|(a, b)| same_number(*a, *b)) => {}
            _ => self.fail(format!("moveBody matrix: the capture has {}, the port {:?}", call.arg(1).show(), matrix.elements)),
        }
    }

    fn body_enabled(&mut self, body: BodyId) -> bool {
        let Some(call) = self.call("world.bodyEnabled") else { return false };
        self.check_body("bodyEnabled body", call.arg(0), Some(body));
        call.result().truthy()
    }

    fn enable_body(&mut self, body: BodyId, enabled: bool) {
        let Some(call) = self.call("world.enableBody") else { return };
        self.check_body("enableBody body", call.arg(0), Some(body));
        self.check(check_bool("enableBody enabled", call.arg(1), enabled));
    }

    fn water_over(&mut self, x: f64, z: f64) -> bool {
        if self.water.is_none() {
            return false;
        }
        let Some(call) = self.call("water.over") else { return false };
        self.check(check_num("water.over x", call.arg(0), x));
        self.check(check_num("water.over z", call.arg(1), z));
        call.result().truthy()
    }

    fn water_level(&mut self) -> f64 {
        self.water.expect("the water's level, on a map without water")
    }

    fn has_water(&mut self) -> bool {
        self.water.is_some()
    }

    fn in_death_region(&mut self, position: &Vector3) -> bool {
        let Some(call) = self.call("world.inDeathRegion") else { return false };
        self.check(check_vec3("inDeathRegion position", call.arg(0), vec(position)));
        call.result().truthy()
    }

    fn damaged(&mut self, hull: usize, amount: f64, attacker: &OrdnanceOwner) {
        let Some(call) = self.call("world.damaged") else { return };
        debug_assert_eq!(hull, self.hull);
        self.check_hull("damaged hull", call.arg(0));
        self.check(check_num("damaged amount", call.arg(1), amount));
        if !call.arg(2).is_nullish() || *attacker != OrdnanceOwner::Null {
            self.fail(format!("damaged attacker: the capture has {}, the port {attacker:?}", call.arg(2).show()));
        }
        self.changed(call);
    }

    fn tow_released(&mut self, hull: usize, reason: &str) {
        // A craft's tick lets no cable go (updateTethers does).
        self.fail(format!("tow released: hull {hull} ({reason}) in a craft's tick"));
    }

    fn destroyed_crew(&mut self, hull: usize) {
        let Some(call) = self.call("vehicles.destroyedCrew") else { return };
        debug_assert_eq!(hull, self.hull);
        self.check_hull("destroyedCrew hull", call.arg(0));
        self.changed(call);
    }

    fn destroyed(&mut self, hull: usize) {
        let Some(call) = self.call("world.destroyed") else { return };
        debug_assert_eq!(hull, self.hull);
        self.check_hull("destroyed hull", call.arg(0));
        self.changed(call);
    }

    fn explosion(&mut self, hull: usize, _final_explosion: bool, position: &Vector3) {
        let Some(call) = self.call("world.explosion") else { return };
        debug_assert_eq!(hull, self.hull);
        if call.arg(0).handle_str("def").is_none() {
            self.fail(format!("explosion def: the capture has {}", call.arg(0).show()));
        }
        self.check(check_vec3("explosion position", call.arg(1), vec(position)));
        self.check_hull("explosion owner", call.arg(2));
        self.changed(call);
    }

    fn raycast_units(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<f64> {
        let call = self.call("units.raycast")?;
        self.check(check_vec3("units.raycast origin", call.arg(0), vec(origin)));
        self.check(check_vec3("units.raycast direction", call.arg(1), vec(direction)));
        self.check(check_num("units.raycast far", call.arg(2), far));
        let r = call.result();
        if r.is_nullish() {
            None
        } else {
            Some(r.get("distance").num())
        }
    }

    fn clock(&mut self) -> f64 {
        match self.call("world.clock") {
            Some(call) => call.result().num(),
            None => 0.0,
        }
    }

    fn effect(&mut self, name: &str, position: &Vector3) {
        let Some(call) = self.call("world.effect") else { return };
        if call.arg(0).str() != Some(name) {
            self.fail(format!("effect name: the capture has {}, the port {name}", call.arg(0).show()));
        }
        self.check(check_vec3("effect position", call.arg(1), vec(position)));
    }
}

impl<'f> FlightWorld for Served<'f> {
    fn environment(&mut self) -> &Environment {
        &self.env
    }

    fn tuning(&mut self, class: FlightClass) -> Tuning {
        self.tunings.of(class)
    }

    fn member_count(&mut self) -> usize {
        self.members.len()
    }

    fn member(&mut self, i: usize) -> Member {
        self.members[i]
    }

    fn member_by_id(&mut self, id: f64) -> Option<usize> {
        self.members.iter().position(|m| m.id == id || m.id.is_nan() && id.is_nan())
    }

    fn relation(&mut self, a: Team, b: Team) -> f64 {
        let same = |x: Team, y: Team| match (x, y) {
            (None, None) => true,
            (Some(p), Some(q)) => same_number(p, q),
            _ => false,
        };
        if !same(a, self.team) {
            self.fail(format!("relation of {a:?}: the capture's relations are {:?}'s", self.team));
            return 0.0;
        }
        match self.relations.iter().find(|(t, _)| same(*t, b)) {
            Some(&(_, r)) => r,
            None => {
                self.fail(format!("relation to {b:?}: not among the capture's"));
                0.0
            }
        }
    }

    fn aim_point(&mut self, i: usize) -> Vector3 {
        let m = self.members[i];
        if !m.unit {
            self.point = Some(m.point);
            return m.point;
        }
        let Some(call) = self.call("unit.aimPoint") else { return Vector3::default() };
        // It is handed the scratch point as the last aim point left it.
        if let Some(p) = self.point {
            self.check(check_vec3("aimPoint out (the last aim point)", call.arg(0), vec(&p)));
        }
        let r = vec3(call.result());
        self.point = Some(r);
        r
    }

    fn body_is_hull(&mut self, body: BodyId) -> bool {
        self.members.iter().any(|m| m.body == Some(body))
    }

    fn system_death_region(&mut self, position: &Vector3) -> bool {
        let Some(call) = self.call("vehicles.inDeathRegion") else { return false };
        self.check(check_vec3("inDeathRegion position", call.arg(0), vec(position)));
        call.result().truthy()
    }
}

/// The battle's content as rust/content loads it (BFE_CONTENT_DIR), as
/// world-hash asked for the capture's battle.
fn content_of(header: V) -> Option<bfe_content::content::Content> {
    use bfe_content::battle::{Battle, Loader};
    use bfe_content::files::DirFiles;
    let root = std::env::var_os("BFE_CONTENT_DIR")?;
    let b = header.get("battle");
    let (map, side, seed) = (b.get("map").str()?.to_string(), b.get("side").str()?.to_string(), b.get("seed").num());
    let ai = b.get("ai").str().unwrap_or("classic").to_string();
    let era = if side == "rep" || side == "cis" { "cw" } else { "gcw" };
    let query = format!("?map={map}&side={side}&era={era}&ai={ai}&seed={seed}&rays=js&movement=js");
    let files = DirFiles::new(std::path::Path::new(&root));
    let loader = Loader::new(&files);
    let battle = Battle::load(&loader, &map, &query).expect("the battle's content");
    Some(bfe_content::content::Content::build(battle).expect("the battle's content"))
}

fn class_def<'c>(content: &'c bfe_content::content::Content, class: &str) -> Option<&'c bfe_content::value::Value> {
    content.battle.vehicle_def.as_ref()?.get("classes")?.get(class)
}

fn template_of<'c>(content: &'c bfe_content::content::Content, def: &bfe_content::value::Value) -> Option<&'c bfe_content::content::VehicleTemplate> {
    let model = def.get("model").and_then(|m| m.as_str())?;
    content.vehicle_templates.iter().find(|t| t.name == model)
}

/// A class's takeoff clip (`clips.get('takeoff')`), its length and the
/// first bone named DummyRoot (any case).
fn gear_clip(content: &bfe_content::content::Content, class: &str) -> Option<GearClip> {
    let def = class_def(content, class)?;
    let template = template_of(content, def)?;
    let c = template.clips.as_ref()?.get("takeoff")?;
    let desc = bfe_sim::rig::content::hull_desc(def, template);
    let root = desc.bones.iter().position(|b| b.name.eq_ignore_ascii_case("dummyroot")).map_or(-1, |i| i as i32);
    Some(GearClip { clip: std::rc::Rc::new(bfe_sim::rig::content::clip(c)), duration: c.duration, root })
}

/// What a frame's state says the craft's weapons hold of its lock, and
/// whether each is `lock` (or what they held before, where targeting did
/// not run).
fn check_locks(after: V, before: V, lock: Option<bfe_sim::flight::state::Lock>) -> Result<(), String> {
    for (s, seat) in after.items().enumerate() {
        for (k, w) in seat.items().enumerate() {
            let at = format!("locks[{s}][{k}]");
            match lock {
                Some(l) => {
                    let pairs = [("lockTargetId", l.target_id), ("lockGeneration", l.generation), ("lockElapsed", l.elapsed)];
                    for (key, mine) in pairs {
                        if !same_number(w.get(key).num(), mine) {
                            return Err(format!("{at}.{key}: the capture has {}, the port {mine}", w.get(key).show()));
                        }
                    }
                    if w.get("lockComplete").bool() != l.complete {
                        return Err(format!("{at}.lockComplete: the capture has {}, the port {}", w.get("lockComplete").show(), l.complete));
                    }
                }
                None => {
                    if let Some(d) = diff(before.at(s).at(k), w) {
                        return Err(format!("{at} changed without targeting: {d}"));
                    }
                }
            }
        }
    }
    Ok(())
}

/// A craft's own configuration as the capture holds it (flightConfig's own
/// keys, in their order), where it does, against the port's.
fn check_config(theirs: V, mine: &Config) -> Result<(), String> {
    if theirs.is_nullish() {
        return Ok(());
    }
    let pairs = [
        ("minSpeed", mine.min_speed),
        ("cruiseSpeed", mine.cruise_speed),
        ("maxSpeed", mine.max_speed),
        ("acceleration", mine.acceleration),
        ("turnRate", mine.turn_rate),
        ("turnFilter", mine.turn_filter),
        ("pitchRate", mine.pitch_rate),
        ("pitchFilter", mine.pitch_filter),
        ("bankAngle", mine.bank_angle),
        ("bankFilter", mine.bank_filter),
        ("levelFilter", mine.level_filter),
        ("takeoffHeight", mine.takeoff_height),
        ("takeoffTime", mine.takeoff_time),
        ("takeoffSpeed", mine.takeoff_speed),
        ("landedHeight", mine.landed_height),
        ("landingTime", mine.landing_time),
        ("landingSpeed", mine.landing_speed),
        ("boostSpeed", mine.boost_speed),
    ];
    let keys: Vec<String> = match theirs.node() {
        Some(Node::Object { fields, .. }) => fields.iter().map(|(k, _)| k.clone()).collect(),
        _ => return Err(format!("config: not an object: {}", theirs.show())),
    };
    if keys.len() != pairs.len() || keys.iter().zip(pairs.iter()).any(|(k, (name, _))| k != name) {
        return Err(format!("config: the capture's keys are {keys:?}"));
    }
    for (name, value) in pairs {
        if !same_number(theirs.get(name).num(), value) {
            return Err(format!("config.{name}: the capture has {}, the port {value}", theirs.get(name).show()));
        }
    }
    Ok(())
}

// What a replayed tick ran: (frames, of which the same) by path.
fn path_of(hull: &Hull) -> &'static str {
    use bfe_sim::flight::FlightState::*;
    if !hull.alive || hull.sleeping == Some(true) && !hull.occupied() {
        return "gone or asleep (no step)";
    }
    match hull.flight.state {
        Some(Flying) if !hull.occupied() => "pilotless",
        Some(Landed) => "landed",
        Some(TakingOff) => "taking off",
        Some(Landing) => "landing",
        Some(Flying) => "flying",
        _ => "other",
    }
}

type Contents = Option<(String, bfe_content::content::Content)>;

fn load<'c>(content: &'c mut Contents, label: &str, header: V) -> &'c bfe_content::content::Content {
    if content.as_ref().map(|(l, _)| l != label).unwrap_or(true) {
        *content = Some((label.to_string(), content_of(header).expect("the capture's battle")));
    }
    &content.as_ref().unwrap().1
}

/// What a replayed tick exercised beyond its path (the corpus's coverage).
fn features(frame: &sim_replay::Frame, network: bool) -> Vec<String> {
    let (b, a) = (frame.before().get("hull"), frame.after().get("hull"));
    let mut out = Vec::new();
    let text = |v: V, key: &str| v.get(key).str().unwrap_or("").to_string();
    let (from, to) = (text(b, "flightState"), text(a, "flightState"));
    if from != to {
        out.push(format!("state {from} -> {to}"));
    }
    if !text(a, "trick").is_empty() || !text(b, "trick").is_empty() {
        out.push(format!("trick {}", if text(b, "trick").is_empty() { text(a, "trick") } else { text(b, "trick") }));
    }
    for key in ["boosting", "terrainWarning", "boostExhausted", "rollLock"] {
        if a.get(key).truthy() {
            out.push(key.to_string());
        }
    }
    let warning = text(a, "flightWarning");
    if !warning.is_empty() {
        out.push(format!("warning {warning}"));
    }
    let lock = text(a, "flightLockState");
    if !lock.is_empty() {
        out.push(format!("lock {lock}"));
    }
    if a.get("flightAssistId").num() >= 0.0 {
        out.push("assist target".into());
    }
    for c in frame.calls() {
        if ["world.damaged", "world.destroyed", "world.explosion", "vehicles.inDeathRegion", "water.over"].contains(&c.name.as_str()) {
            out.push(format!("call {}", c.name));
        }
    }
    if network {
        out.push("network pilot".into());
    }
    out.sort();
    out.dedup();
    out
}

/// Replays one flyer.tick frame; the path it took and its outcome.
fn replay_tick(content: &bfe_content::content::Content, classes: &mut HashMap<String, HullClass>, frame: &sim_replay::Frame, seen: &mut BTreeMap<String, (usize, usize)>) -> (&'static str, Outcome) {
    let before = frame.before();
    if before.get("skipped").truthy() {
        return ("ground hull", Outcome::Skipped("a ground hull's tick (the hulls package's)".into()));
    }
    let Some(index) = frame.receiver().handle_num("vehicle") else {
        return ("?", Outcome::Failed(format!("{}: no hull receiver ({})", frame.place(), frame.receiver().show())));
    };
    let index = index as usize;
    let water = before.get("water");
    let water = if water.is_nullish() { None } else { Some(water.get("level").num()) };
    let mut cx = Replay::default();
    let r = match Vehicle::value_in(before.get("hull"), &mut cx, "hull") {
        Ok(r) => r,
        Err(e) => return ("?", Outcome::Failed(format!("{}: {e}", frame.place()))),
    };
    let stored = cx.store.vehicle.get(r).unwrap().clone();
    let name = before.get("hull").get("name").str().unwrap_or("?").to_string();
    let Some(def) = class_def(content, &name) else {
        return ("?", Outcome::Failed(format!("{}: no class {name} in the content", frame.place())));
    };
    let class = classes.entry(name.clone()).or_insert_with(|| HullClass::new(def, stored.turn_rate));
    let mut hull = Hull::from_store(r, &stored, class, index);
    hull.body = body_of(before.get("hull").get("body"));
    hull.flight = match Flight::from_store(&stored, &cx.strings, Config::of(def), None) {
        Ok(f) => f,
        Err(e) => return ("?", Outcome::Failed(format!("{}: {e}", frame.place()))),
    };
    // The class definition's keys as the JavaScript read them (flightConfig's own).
    if let Err(e) = check_config(before.get("config"), &hull.flight.config) {
        return ("?", Outcome::Failed(format!("{}: {e}", frame.place())));
    }
    let path = path_of(&hull);
    let idle = before.get("idle").truthy();
    let world = before.get("world");
    let mut served = Served {
        log: Log::new(frame),
        hull: index,
        water,
        env: if idle { Environment::default() } else { environment_of(world) },
        tunings: if idle { Tunings::default() } else { tunings_of(world) },
        members: if idle { Vec::new() } else { members_of(before.get("targets")) },
        relations: if idle { Vec::new() } else { relations_of(before.get("relations")) },
        team: hull.flight.team,
        point: None,
        error: None,
    };
    let dt = frame.arg(0).num();
    let input = flight_input(frame.arg(1));
    let pilot = before.get("pilot");
    let network = if !idle && pilot.get("networkHuman").bool() && pilot.get("flight").node().is_some() { Some(flight_input(pilot.get("flight"))) } else { None };
    hull.flyer_tick(&mut served, dt, &input, network.as_ref());
    let exercised = if idle { Vec::new() } else { features(frame, network.is_some()) };
    let tally = |seen: &mut BTreeMap<String, (usize, usize)>, same: bool| {
        for f in &exercised {
            let t = seen.entry(f.clone()).or_default();
            t.0 += 1;
            if same {
                t.1 += 1;
            }
        }
    };
    if let Some(e) = served.error {
        tally(seen, false);
        return (path, Outcome::Failed(e));
    }
    if let Err(e) = served.log.finish() {
        tally(seen, false);
        return (path, Outcome::Failed(e.0));
    }
    {
        let v = cx.store.vehicle.get_mut(r).unwrap();
        hull.to_store(v, &mut cx.strings);
        hull.flight.to_store(v, &mut cx.strings);
    }
    let mut arena = Arena::default();
    let mine = Vehicle::value_out(r, &mut cx, &mut arena);
    let after = frame.after();
    if let Some(d) = diff(after.get("hull"), arena.view(&mine)) {
        tally(seen, false);
        return (path, Outcome::Failed(format!("{}: hull {d} (capture | port)", frame.place())));
    }
    if !idle {
        if let Err(e) = check_locks(after.get("locks"), before.get("locks"), hull.flight.lock) {
            tally(seen, false);
            return (path, Outcome::Failed(format!("{}: {e}", frame.place())));
        }
    }
    tally(seen, true);
    (path, Outcome::Same)
}

#[test]
fn flyers_tick_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    if std::env::var_os("BFE_CONTENT_DIR").is_none() {
        eprintln!("flyers_tick_as_captured: no BFE_CONTENT_DIR (the crafts' classes are the content's): not replayed");
        return;
    }
    let mut paths: BTreeMap<&'static str, (usize, usize)> = Default::default();
    let mut seen: BTreeMap<String, (usize, usize)> = Default::default();
    let mut content: Contents = None;
    let mut classes: HashMap<String, HullClass> = Default::default();
    let mut label_seen = String::new();
    let report = replay_corpus(&corpus, "flyer.tick", |capture, frame| {
        let label = capture.label();
        if label != label_seen {
            classes.clear();
            label_seen = label.clone();
        }
        let c = load(&mut content, &label, capture.header.root());
        let (path, outcome) = replay_tick(c, &mut classes, frame, &mut seen);
        let tally = paths.entry(path).or_default();
        tally.0 += 1;
        if matches!(outcome, Outcome::Same) {
            tally.1 += 1;
        }
        outcome
    })
    .unwrap();
    eprintln!("{}", report.summary());
    for (path, (frames, same)) in &paths {
        eprintln!("  {path}: {frames} frames, {same} the same");
    }
    eprintln!("  exercised:");
    for (what, (frames, same)) in &seen {
        eprintln!("    {what}: {frames} frames, {same} the same");
    }
    report.assert_clean();
}

type Built = bfe_sim::rig::hull::HullRig;

/// A class's rig, built as the Vehicle constructor builds it.
fn rig_of(content: &bfe_content::content::Content, class: &str) -> Option<Built> {
    let def = class_def(content, class)?;
    let template = template_of(content, def)?;
    Some(bfe_sim::rig::hull::HullRig::new(&bfe_sim::rig::content::hull_desc(def, template)))
}

fn quat_of(v: V) -> Option<Quaternion> {
    match v.node() {
        Some(Node::Quat(q)) => Some(Quaternion::new(q[0], q[1], q[2], q[3])),
        _ => None,
    }
}

#[test]
fn flyers_pose_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    if std::env::var_os("BFE_CONTENT_DIR").is_none() {
        eprintln!("flyers_pose_as_captured: no BFE_CONTENT_DIR (the rigs are built from the content): not replayed");
        return;
    }
    for name in ["flyer.aimTick", "flyer.render"] {
        let mut content: Contents = None;
        let mut rigs: HashMap<String, Option<(Built, Option<GearClip>)>> = Default::default();
        let mut label_seen = String::new();
        let mut posed_same = 0usize;
        // What the same frames exercised: a crewed turret seat, the gear
        // between down and stowed, a render that is no tick's (a spawn's).
        let (mut turrets, mut gear_moving, mut untimed) = (0usize, 0usize, 0usize);
        let report = replay_corpus(&corpus, name, |capture, frame| {
            let label = capture.label();
            if label != label_seen {
                rigs.clear();
                label_seen = label.clone();
            }
            let before = frame.before();
            if before.get("skipped").truthy() {
                return Outcome::Skipped("a ground hull's pose (the hulls package's)".into());
            }
            let hull_v = before.get("hull");
            if !hull_v.get("alive").bool() {
                // A craft that is gone is not posed: nothing changes.
                return match diff(hull_v, frame.after().get("hull")) {
                    None => Outcome::Same,
                    Some(d) => Outcome::Failed(format!("{}: a gone craft's pose changed: {d}", frame.place())),
                };
            }
            let c = load(&mut content, &label, capture.header.root());
            let class = hull_v.get("name").str().unwrap_or("?").to_string();
            let Some((rig, gear)) = rigs.entry(class.clone()).or_insert_with(|| rig_of(c, &class).map(|r| (r, gear_clip(c, &class)))).clone() else {
                return Outcome::Failed(format!("{}: no rig for {class} in the content", frame.place()));
            };
            let Some(index) = frame.receiver().handle_num("vehicle") else {
                return Outcome::Failed(format!("{}: no hull receiver", frame.place()));
            };
            let mut hull = pose_from_value(hull_v, index as usize, rig);
            // The world matrices it starts with, where the frame reads them
            // (a world's render after aimTick).
            if !hull_v.get("world").is_nullish() {
                let worlds: Vec<f64> = hull_v.get("world").items().map(|x| x.num()).collect();
                let rig = &mut hull.pose.as_mut().unwrap().rig;
                for (i, m) in worlds.chunks_exact(16).enumerate() {
                    rig.tree.node_mut(i as three::NodeId).matrix_world.elements.copy_from_slice(m);
                }
            }
            hull.flight.gear = hull_v.get("gear").num();
            hull.flight.previous_q = quat_of(hull_v.get("flightPreviousQ")).unwrap_or(hull.quaternion);
            hull.flight.frame_q = quat_of(hull_v.get("flightFrameQ"));
            hull.flight.gear_clip = gear;
            let (alpha, dt) = (frame.arg(0).num(), frame.arg(1).num());
            let mut served = Served {
                log: Log::new(frame),
                hull: index as usize,
                water: None,
                env: Environment::default(),
                tunings: Tunings::default(),
                members: Vec::new(),
                relations: Vec::new(),
                team: None,
                point: None,
                error: None,
            };
            if name == "flyer.aimTick" {
                hull.aim_tick(&mut served, alpha, dt);
            } else {
                hull.render(&mut served, alpha, dt, frame.arg(2).truthy());
            }
            if let Some(e) = served.error {
                return Outcome::Failed(e);
            }
            if let Err(e) = served.log.finish() {
                return Outcome::Failed(e.0);
            }
            let mut arena = Arena::default();
            let mine = pose_to_value(&mut arena, &mut hull, &class, true);
            let gear = Value::Num(hull.flight.gear);
            arena.assign(&mine, "gear", gear);
            let prev = hull.flight.previous_q;
            let prev = arena.quat([prev.x, prev.y, prev.z, prev.w]);
            arena.assign(&mine, "flightPreviousQ", prev);
            let frame_q = match hull.flight.frame_q {
                Some(q) => arena.quat([q.x, q.y, q.z, q.w]),
                None => Value::Null,
            };
            arena.assign(&mine, "flightFrameQ", frame_q);
            match diff(frame.after().get("hull"), arena.view(&mine)) {
                None => {
                    posed_same += 1;
                    if hull_v.get("seats").items().skip(1).any(|s| !s.get("lease").is_nullish()) {
                        turrets += 1;
                    }
                    let g = hull_v.get("gear").num();
                    if g > 0.0 && g < 1.0 {
                        gear_moving += 1;
                    }
                    if name == "flyer.render" && !frame.arg(2).truthy() {
                        untimed += 1;
                    }
                    Outcome::Same
                }
                Some(d) => Outcome::Failed(format!("{}: hull {d} (capture | port)", frame.place())),
            }
        })
        .unwrap();
        eprintln!("{}\n  posed: {posed_same} of the same frames ({turrets} with a crewed turret seat, {gear_moving} with the gear moving, {untimed} a spawn's)", report.summary());
        report.assert_clean();
    }
}
