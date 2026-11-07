//! The flight package's functions the battle reaches from outside a craft's
//! own tick, replayed against cases made by the JavaScript itself
//! in the synthetic frozen fixture: a bot pilot's stick and its altitude
//! stage, the assisted shot, a flyer missile's seeker, the transports'
//! spawn points and a craft's class by its name. Each case's calls to the
//! battle are answered from the case (their arguments checked bit for bit)
//! and what comes out is compared to the last bit.
//!
//! Synthetic cases are frozen in the repository, with number bits and
//! undefined preserved. An external BFECAP can extend the same checks through
//! BFE_FLIGHT_CASES; the default CI run always exercises the frozen cases.
#![cfg(feature = "replay")]

use bfe_sim::flight::ai::{Blend, PilotEnv, PilotWorld, Request, Steer};
use bfe_sim::flight::config::{flight_class, Config, Tuning, Tunings, TUNING_KEYS};
use bfe_sim::flight::controls::{flight_input, Bindings, Controls, Pad};
use bfe_sim::flight::spawn::transport_points;
use bfe_sim::flight::state::{Message, Trick};
use bfe_sim::flight::targeting::{navigate_missile, MissileTarget};
use bfe_sim::flight::{Environment, FlightClass, FlightState, FlightWorld, Member, Team};
use bfe_sim::vehicles::{BodyId, Ground, Hit, Hull, Polygon, VehicleWorld};
use sim_replay::{same_number, Arena, Node, Reader, Value, V};
use three::{Matrix4, Quaternion, Vector3};

fn v3(v: V) -> Vector3 {
    Vector3::new(v.at(0).num(), v.at(1).num(), v.at(2).num())
}

fn q4(v: V) -> Quaternion {
    Quaternion::new(v.at(0).num(), v.at(1).num(), v.at(2).num(), v.at(3).num())
}

/// A number as JavaScript arithmetic reads it: undefined NaN, null 0.
fn js_num(v: V) -> f64 {
    match v.value {
        Value::Num(x) => *x,
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        _ => f64::NAN,
    }
}

fn class_of(name: &str) -> FlightClass {
    let i = FlightClass::NAMES.iter().position(|&n| n == name).unwrap_or_else(|| panic!("no class {name}"));
    FlightClass::from_u8(i as u8).unwrap()
}

fn config_of(own: V) -> Config {
    let n = |k: &str| own.get(k).num();
    Config {
        min_speed: n("minSpeed"),
        cruise_speed: n("cruiseSpeed"),
        max_speed: n("maxSpeed"),
        acceleration: n("acceleration"),
        turn_rate: n("turnRate"),
        turn_filter: n("turnFilter"),
        pitch_rate: n("pitchRate"),
        pitch_filter: n("pitchFilter"),
        bank_angle: n("bankAngle"),
        bank_filter: n("bankFilter"),
        level_filter: n("levelFilter"),
        takeoff_height: n("takeoffHeight"),
        takeoff_time: n("takeoffTime"),
        takeoff_speed: n("takeoffSpeed"),
        landed_height: n("landedHeight"),
        landing_time: n("landingTime"),
        landing_speed: n("landingSpeed"),
        boost_speed: n("boostSpeed"),
    }
}

fn same3(a: &Vector3, b: V) -> bool {
    same_number(a.x, b.at(0).num()) && same_number(a.y, b.at(1).num()) && same_number(a.z, b.at(2).num())
}

/// A case's calls, answered in order, their arguments checked.
#[derive(Clone)]
struct Scripted<'c> {
    calls: Vec<V<'c>>,
    next: usize,
    body: BodyId,
    height: bool,
    water: bool,
    rounds: Vec<(bool, f64, Vector3)>,
    error: Option<String>,
    // For the assisted shot: the battle's settings and the target.
    env: Environment,
    target: Option<(bool, Vector3)>,
}

impl<'c> Scripted<'c> {
    fn take(&mut self, kind: &str) -> Option<V<'c>> {
        if self.error.is_some() {
            return None;
        }
        match self.calls.get(self.next) {
            Some(c) if c.get("k").str() == Some(kind) => {
                self.next += 1;
                Some(*c)
            }
            other => {
                self.error = Some(format!("call {}: the port asks {kind}, the case has {}", self.next, other.map_or("nothing".into(), |c| c.show())));
                None
            }
        }
    }

    fn check(&mut self, what: &str, ok: bool, theirs: V, mine: String) {
        if !ok && self.error.is_none() {
            self.error = Some(format!("call {} {what}: the case has {}, the port {mine}", self.next, theirs.show()));
        }
    }

    fn cast_call(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> {
        let c = self.take("cast")?;
        self.check("origin", same3(origin, c.get("origin")), c.get("origin"), format!("{origin:?}"));
        self.check("direction", same3(direction, c.get("direction")), c.get("direction"), format!("{direction:?}"));
        self.check("far", same_number(far, c.get("far").num()), c.get("far"), format!("{far}"));
        let theirs = c.get("ignore").opt_num().map(|x| x as BodyId);
        self.check("ignore", theirs == ignore, c.get("ignore"), format!("{ignore:?}"));
        let h = c.get("hit");
        if h.is_nullish() {
            return None;
        }
        Some(Hit { distance: h.get("distance").num(), point: v3(h.get("point")), normal: v3(h.get("normal")), terrain: h.get("terrain").bool(), body: None })
    }

    fn height_call(&mut self, kind: &str, x: f64, z: f64) -> f64 {
        let Some(c) = self.take(kind) else { return f64::NAN };
        self.check("x", same_number(x, c.get("x").num()), c.get("x"), format!("{x}"));
        self.check("z", same_number(z, c.get("z").num()), c.get("z"), format!("{z}"));
        // A non-number answer is not finite to Number.isFinite.
        match c.get("h").value {
            Value::Num(h) => *h,
            _ => f64::NAN,
        }
    }
}

impl<'c> PilotWorld for Scripted<'c> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> {
        self.cast_call(origin, direction, far, ignore)
    }
    fn terrain(&mut self, x: f64, z: f64) -> Option<f64> {
        if !self.height {
            return None;
        }
        Some(self.height_call("height", x, z))
    }
    fn water(&mut self, x: f64, z: f64) -> Option<f64> {
        if !self.water {
            return None;
        }
        Some(self.height_call("water", x, z))
    }
    fn incoming(&mut self, target_id: f64, position: &Vector3, range: f64) -> bool {
        self.rounds.iter().any(|(dead, id, at)| !dead && *id == target_id && at.distance_to(position) < range)
    }
}

impl<'c> VehicleWorld for Scripted<'c> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> {
        self.cast_call(origin, direction, far, ignore)
    }
    fn push_walls(&mut self, _: usize, _: &[Vector3], _: f64, _: &Quaternion, _: Option<&Quaternion>, _: &Vector3, _: &mut Vector3, _: &mut Vector3, _: bool) -> bool {
        unreachable!()
    }
    fn ground(&mut self, _: f64, _: f64, _: f64) -> Option<Ground> {
        unreachable!()
    }
    fn spring_polygons<'a>(&'a mut self, _: usize, _: usize, _: &Vector3, _: f64, _: &'a mut Vec<Polygon>) -> bfe_sim::vehicles::SpringPolygons<'a> {
        unreachable!()
    }
    fn move_body(&mut self, _: BodyId, _: &Matrix4) {
        unreachable!()
    }
    fn enable_body(&mut self, _: BodyId, _: bool) {
        unreachable!()
    }
    fn body_enabled(&mut self, _: BodyId) -> bool {
        unreachable!()
    }
    fn water_over(&mut self, _: f64, _: f64) -> bool {
        unreachable!()
    }
    fn water_level(&mut self) -> f64 {
        unreachable!()
    }
    fn has_water(&mut self) -> bool {
        unreachable!()
    }
    fn in_death_region(&mut self, _: &Vector3) -> bool {
        unreachable!()
    }
    fn damaged(&mut self, _: usize, _: f64, _: &bfe_sim::gen::OrdnanceOwner) {
        unreachable!()
    }
    fn tow_released(&mut self, _: usize, _: &str) {
        unreachable!()
    }
    fn destroyed_crew(&mut self, _: usize) {
        unreachable!()
    }
    fn destroyed(&mut self, _: usize) {
        unreachable!()
    }
    fn explosion(&mut self, _: usize, _: bool, _: &Vector3) {
        unreachable!()
    }
    fn effect(&mut self, _: &str, _: &Vector3) {
        unreachable!()
    }
    fn raycast_units(&mut self, _: &Vector3, _: &Vector3, _: f64) -> Option<f64> {
        unreachable!()
    }
    fn clock(&mut self) -> f64 {
        unreachable!()
    }
}

impl<'c> FlightWorld for Scripted<'c> {
    fn environment(&mut self) -> &Environment {
        &self.env
    }
    fn tuning(&mut self, class: FlightClass) -> Tuning {
        Tuning::of(class)
    }
    fn member_count(&mut self) -> usize {
        usize::from(self.target.is_some())
    }
    fn member(&mut self, _: usize) -> Member {
        let (alive, point) = self.target.unwrap();
        Member { id: 5.0, generation: Some(1.0), alive, team: None, own: false, unit: false, body: None, point }
    }
    fn relation(&mut self, _: Team, _: Team) -> f64 {
        unreachable!()
    }
    fn aim_point(&mut self, i: usize) -> Vector3 {
        self.member(i).point
    }
    fn body_is_hull(&mut self, _: BodyId) -> bool {
        unreachable!()
    }
    fn system_death_region(&mut self, _: &Vector3) -> bool {
        unreachable!()
    }
}

fn scripted<'c>(case: V<'c>) -> Scripted<'c> {
    Scripted {
        calls: case.get("calls").items().collect(),
        next: 0,
        body: 77,
        height: false,
        water: false,
        rounds: Vec::new(),
        error: None,
        env: Environment::default(),
        target: None,
    }
}

fn pilot(case: V) -> Result<(), String> {
    let input = case.get("input");
    let v = input.get("v");
    let mut w = scripted(case);
    let env_v = input.get("env");
    w.height = env_v.get("height").bool();
    w.water = env_v.get("water").bool();
    w.rounds = input.get("rounds").items().map(|r| (r.get("dead").bool(), r.get("targetId").num(), v3(r.get("position")))).collect();
    let class = class_of(input.get("kind").str().unwrap());
    let mut hull = Hull { position: v3(v.get("position")), quaternion: q4(v.get("quaternion")), body: w.body, ..Hull::default() };
    let f = &mut hull.flight;
    f.class = class;
    f.config = config_of(input.get("own"));
    f.airspeed = v.get("airspeed").num();
    f.radius = js_num(v.get("radius"));
    f.state = Some(match v.get("flightState").str().unwrap() {
        "Landed" => FlightState::Landed,
        "TakingOff" => FlightState::TakingOff,
        "Flying" => FlightState::Flying,
        _ => FlightState::Landing,
    });
    f.landing_reason = Message::from_text(v.get("landingReason").str().unwrap()).unwrap();
    f.grounded = v.get("grounded").bool();
    f.state_time = v.get("stateTime").num();
    f.clock = v.get("flightClock").num();
    f.power = v.get("power").num();
    f.trick = Trick::from_text(v.get("trick").str().unwrap()).unwrap();
    f.terrain_warning = v.get("terrainWarning").bool();
    let steer_v = input.get("steer");
    let steer = Steer { land: steer_v.get("land").bool(), loop_: steer_v.get("loop").bool(), path_altitude: steer_v.get("pathAltitude").bool() };
    let req_v = input.get("request");
    let request = if req_v.is_nullish() {
        None
    } else {
        let blend = req_v.get("blend");
        Some(Request {
            valid: req_v.get("valid").bool(),
            steering_target: Some(v3(req_v.get("steeringTarget"))),
            mode: req_v.get("mode").num(),
            blend: match blend.value {
                Value::Num(x) => Some(*x),
                _ => None,
            },
            throttle: js_num(req_v.get("throttle")),
        })
    };
    let aim = if input.get("aim").is_nullish() { None } else { Some(v3(input.get("aim"))) };
    let env = PilotEnv {
        min_fly_height: env_v.get("minFlyHeight").opt_num().unwrap_or(-200.0),
        max_fly_height: env_v.get("maxFlyHeight").opt_num(),
        has_path: env_v.get("hasPath").bool(),
        space: v.get("space").bool(),
    };
    // Exercise both entry points against the same frozen script: solver
    // callers and the native AI's smaller canonical observation.
    let mut solver_world = w.clone();
    let solver_stick = hull.pilot_input(&mut solver_world, &env, &Tuning::of(class), request.as_ref(), aim.as_ref(), &steer, 7.0);
    let mut row = bfe_sim::gen::Vehicle::default();
    row.flying = true;
    row.flight_class = Some(class);
    row.position = [hull.position.x, hull.position.y, hull.position.z];
    row.quaternion = [hull.quaternion.x, hull.quaternion.y, hull.quaternion.z, hull.quaternion.w];
    row.body = bfe_sim::store::Ref::new(hull.body, 0);
    row.radius = hull.flight.radius;
    let mut strings = bfe_sim::store::Strings::default();
    hull.flight.to_store(&mut row, &mut strings);
    let pilot = bfe_sim::flight::ai::Pilot::from_vehicle(&row, &strings, hull.flight.config)?;
    let stick = pilot.pilot_input(&mut w, &env, &Tuning::of(class), request.as_ref(), aim.as_ref(), &steer, 7.0);
    for (solver, canonical) in [(solver_stick.forward, stick.forward), (solver_stick.right, stick.right),
        (solver_stick.turn, stick.turn), (solver_stick.pitch, stick.pitch), (solver_stick.up, stick.up), (solver_stick.down, stick.down)] {
        if !same_number(solver, canonical) { return Err("canonical pilot and solver pilot numbers differ".into()); }
    }
    if (solver_stick.brake, solver_stick.trick, solver_stick.immelmann, solver_stick.boost)
        != (stick.brake, stick.trick, stick.immelmann, stick.boost)
        || solver_world.error != w.error || solver_world.next != w.next {
        return Err("canonical pilot and solver pilot differ".into());
    }
    if let Some(e) = w.error {
        return Err(e);
    }
    if w.next != w.calls.len() {
        return Err(format!("the port made {} calls, the case {}", w.next, w.calls.len()));
    }
    let out = case.get("out");
    for (key, mine) in [("forward", stick.forward), ("right", stick.right), ("turn", stick.turn), ("pitch", stick.pitch), ("up", stick.up), ("down", stick.down)] {
        if !same_number(out.get(key).num(), mine) {
            return Err(format!("out.{key}: the case has {}, the port {mine}", out.get(key).show()));
        }
    }
    // (The landing path leaves trick, immelmann and boost unset: seatAnalogs,
    // the one caller, reads each through Boolean().)
    for (key, mine) in [("brake", stick.brake), ("trick", stick.trick), ("immelmann", stick.immelmann), ("boost", stick.boost)] {
        if out.get(key).truthy() != mine {
            return Err(format!("out.{key}: the case has {}, the port {mine}", out.get(key).show()));
        }
    }
    Ok(())
}

fn raise(case: V) -> Result<(), String> {
    let input = case.get("input");
    let mut w = scripted(case);
    w.height = input.get("height").bool();
    w.water = input.get("water").bool();
    let class = class_of(input.get("kind").str().unwrap());
    let mut hull = Hull { position: v3(input.get("position")), quaternion: q4(input.get("quaternion")), body: w.body, ..Hull::default() };
    hull.flight.class = class;
    hull.flight.config = config_of(input.get("own"));
    hull.flight.airspeed = input.get("airspeed").num();
    hull.flight.radius = input.get("radius").num();
    let env = PilotEnv { min_fly_height: input.get("minFlyHeight").opt_num().unwrap_or(-200.0), ..PilotEnv::default() };
    let mut point = v3(input.get("point"));
    let blend = match input.get("blend").value {
        Value::Bool(true) => Blend::Ease,
        Value::Bool(false) => Blend::None,
        Value::Num(x) => Blend::Share(*x),
        _ => panic!("a blend that is {}", input.get("blend").show()),
    };
    let obstacle = hull.raise_over_terrain(&mut w, &env, &mut point, blend);
    if let Some(e) = w.error {
        return Err(e);
    }
    if w.next != w.calls.len() {
        return Err(format!("the port made {} calls, the case {}", w.next, w.calls.len()));
    }
    let out = case.get("out");
    if !same3(&point, out.get("point")) {
        return Err(format!("point: the case has {}, the port {point:?}", out.get("point").show()));
    }
    if out.get("obstacle").bool() != obstacle {
        return Err(format!("obstacle: the case has {}, the port {obstacle}", out.get("obstacle").show()));
    }
    Ok(())
}

fn assist(case: V) -> Result<(), String> {
    let input = case.get("input");
    let mut w = scripted(case);
    w.env.assists = input.get("assists").bool();
    let t = input.get("target");
    w.target = if t.is_nullish() { None } else { Some((t.get("alive").bool(), v3(t.get("point")))) };
    let mut hull = Hull::default();
    hull.flight.class = class_of(input.get("kind").str().unwrap());
    hull.flight.assist_id = input.get("assistId").num();
    let mut direction = v3(input.get("direction"));
    hull.assist_shot(&mut w, &mut direction, &v3(input.get("origin")));
    let out = case.get("out");
    if !same3(&direction, out.get("direction")) {
        return Err(format!("direction: the case has {}, the port {direction:?}", out.get("direction").show()));
    }
    Ok(())
}

fn missile(case: V) -> Result<(), String> {
    let input = case.get("input");
    let name = input.get("flightClass").str().filter(|s| !s.is_empty()).unwrap_or("fighter");
    let t = input.get("target");
    let target = MissileTarget {
        position: v3(t.get("position")),
        velocity: if t.get("velocity").is_nullish() { None } else { Some(v3(t.get("velocity"))) },
        roll_rate: {
            let r = js_num(t.get("roll"));
            if r == 0.0 || r.is_nan() {
                0.0
            } else {
                r
            }
        },
    };
    let mut velocity = v3(input.get("velocity"));
    let kept = navigate_missile(&v3(input.get("position")), &mut velocity, &Tuning::of(class_of(name)), &target, input.get("dt").num());
    let out = case.get("out");
    if !same3(&velocity, out.get("velocity")) {
        return Err(format!("velocity: the case has {}, the port {velocity:?}", out.get("velocity").show()));
    }
    let id = if kept { 9.0 } else { -1.0 };
    if !same_number(out.get("targetId").num(), id) {
        return Err(format!("targetId: the case has {}, the port {id}", out.get("targetId").show()));
    }
    Ok(())
}

fn transport(case: V) -> Result<(), String> {
    let input = case.get("input");
    let mut w = scripted(case);
    let hulls: Vec<Hull> = input
        .get("vehicles")
        .items()
        .map(|v| {
            let class = v.get("flightClass").str().map(|s| s.to_string());
            let mut h = Hull {
                alive: v.get("alive").bool(),
                flying: class.is_some(),
                position: v3(v.get("position")),
                quaternion: q4(v.get("quaternion")),
                yaw: v.get("yaw").num(),
                body: v.get("body").num() as BodyId,
                ..Hull::default()
            };
            h.bounds_max.x = v.get("maxX").num();
            if let Some(c) = class {
                h.flight.class = class_of(&c);
            }
            h.flight.state = match v.get("flightState").str() {
                Some("Landed") => Some(FlightState::Landed),
                Some("Flying") => Some(FlightState::Flying),
                _ => None,
            };
            h.flight.team = v.get("team").opt_num();
            h
        })
        .collect();
    let points = transport_points(&mut w, &hulls);
    if let Some(e) = w.error {
        return Err(e);
    }
    if w.next != w.calls.len() {
        return Err(format!("the port made {} calls, the case {}", w.next, w.calls.len()));
    }
    let posts: Vec<V> = case.get("out").get("posts").items().collect();
    if posts.len() != points.len() {
        return Err(format!("posts: the case has {}, the port {}", posts.len(), points.len()));
    }
    for (p, mine) in posts.iter().zip(&points) {
        if p.get("hull").num() as usize != mine.hull || !same_number(p.get("team").num(), mine.team) || !same3(&mine.point, p.get("point")) || !same_number(p.get("yaw").num(), mine.yaw) {
            return Err(format!("post: the case has {}, the port {mine:?}", p.show()));
        }
    }
    Ok(())
}

fn class(case: V) -> Result<(), String> {
    let name = case.get("input").get("name").str().unwrap_or("");
    let mine = flight_class(name);
    let theirs = case.get("out").get("kind").str().unwrap_or("?");
    if FlightClass::NAMES[mine as usize] != theirs {
        return Err(format!("{name:?}: the case has {theirs}, the port {}", FlightClass::NAMES[mine as usize]));
    }
    Ok(())
}

struct CaseControls {
    down: Vec<String>,
    queued: Vec<String>,
}

impl Controls for CaseControls {
    fn down(&self, code: &str) -> bool {
        self.down.iter().any(|c| c == code)
    }
    fn take(&mut self, action: &str) -> bool {
        match self.queued.iter().position(|a| a == action) {
            Some(i) => {
                self.queued.remove(i);
                true
            }
            None => false,
        }
    }
}

struct CaseBindings(Vec<(String, Vec<String>)>);

impl Bindings for CaseBindings {
    fn codes(&self, name: &str) -> &[String] {
        self.0.iter().find(|(n, _)| n == name).map_or(&[], |(_, c)| c.as_slice())
    }
}

fn strings(v: V) -> Vec<String> {
    v.items().map(|s| s.str().unwrap_or("").to_string()).collect()
}

fn keys(case: V) -> Result<(), String> {
    let input = case.get("input");
    let mut controls = CaseControls { down: strings(input.get("down")), queued: strings(input.get("queued")) };
    let bindings = match input.get("bindings").node() {
        Some(sim_replay::Node::Object { fields, .. }) => CaseBindings(fields.iter().map(|(k, v)| (k.clone(), strings(input.get("bindings").arena.view(v)))).collect()),
        _ => return Err("bindings: not an object".into()),
    };
    let p = input.get("pad");
    let pad = if p.is_nullish() {
        None
    } else {
        Some(Pad {
            axes: if p.get("axes").is_nullish() { Vec::new() } else { p.get("axes").items().map(|a| a.opt_num()).collect() },
            buttons: p.get("buttons").items().map(|b| b.bool()).collect(),
        })
    };
    let curve = js_num(input.get("curve"));
    let mine = flight_input(&mut controls, &bindings, pad.as_ref(), curve, input.get("assist").bool());
    let out = case.get("out");
    let k = out.get("keys");
    let num = |key: &str, mine: f64| -> Result<(), String> {
        if same_number(js_num(k.get(key)), mine) {
            Ok(())
        } else {
            Err(format!("{key}: the case has {}, the port {mine}", k.get(key).show()))
        }
    };
    num("forward", mine.forward)?;
    num("right", mine.right)?;
    for (key, value) in [("stickX", mine.stick_x), ("stickY", mine.stick_y)] {
        let theirs = k.get(key);
        match value {
            None if theirs.is_undefined() => {}
            Some(x) if same_number(theirs.opt_num().unwrap_or(f64::NAN), x) && !theirs.is_undefined() => {}
            _ => return Err(format!("{key}: the case has {}, the port {value:?}", theirs.show())),
        }
    }
    for (key, value) in [("boost", mine.boost), ("rollLock", mine.roll_lock), ("takeoff", mine.takeoff), ("trick", mine.trick), ("immelmann", mine.immelmann), ("targetLock", mine.target_lock)] {
        if !matches!(k.get(key).value, Value::Bool(_)) || f64::from(u8::from(k.get(key).bool())) != value {
            return Err(format!("{key}: the case has {}, the port {value}", k.get(key).show()));
        }
    }
    num("responseCurve", mine.response_curve)?;
    let no_assist = matches!(k.get("steeringAssist").value, Value::Bool(false));
    if no_assist != mine.no_assist {
        return Err(format!("steeringAssist: the case has {}, the port no_assist {}", k.get("steeringAssist").show(), mine.no_assist));
    }
    if strings(out.get("queued")) != controls.queued {
        return Err(format!("queued after: the case has {}, the port {:?}", out.get("queued").show(), controls.queued));
    }
    Ok(())
}

/// The declared tunings are Tuning::of's, key by key; a command retunes
/// them as the JavaScript's step does.
fn tuning(case: V) -> Result<(), String> {
    let input = case.get("input");
    let declared = input.get("declared");
    let mut tunings = Tunings::default();
    for (i, name) in FlightClass::NAMES.iter().enumerate() {
        let row = Tuning::of(FlightClass::from_u8(i as u8).unwrap());
        for key in TUNING_KEYS {
            let theirs = declared.get(name).get(key).num();
            if !same_number(theirs, row.get(key).unwrap()) {
                return Err(format!("{name}.{key}: declared {theirs}, Tuning::of {}", row.get(key).unwrap()));
            }
        }
    }
    let command = input.get("command");
    if let Some(sim_replay::Node::Object { fields, .. }) = command.node() {
        for (class, values) in fields {
            let values = command.arena.view(values);
            if let Some(sim_replay::Node::Object { fields, .. }) = values.node() {
                for (key, value) in fields {
                    tunings.retune(class, key, js_num(values.arena.view(value)));
                }
            }
        }
    }
    let out = case.get("out").get("classes");
    for (i, name) in FlightClass::NAMES.iter().enumerate() {
        for key in TUNING_KEYS {
            let (theirs, mine) = (out.get(name).get(key).num(), tunings.0[i].get(key).unwrap());
            if !same_number(theirs, mine) {
                return Err(format!("{name}.{key} after the command: the case has {theirs}, the port {mine}"));
            }
        }
    }
    Ok(())
}

// Tags carry JavaScript values JSON itself cannot represent. All other
// objects keep their authored key order, just like the capture arena.
fn frozen_value(value: &bfe_content::Value, arena: &mut Arena) -> Value {
    use bfe_content::Value as Json;
    match value {
        Json::Null => Value::Null,
        Json::Bool(value) => Value::Bool(*value),
        Json::Number(value) => Value::Num(*value),
        Json::String(value) => Value::Str(value.to_string()),
        Json::Array(values) => {
            let values = values.iter().map(|v| frozen_value(v, arena)).collect();
            arena.array(values)
        }
        Json::Object(object) => {
            if object.get("$undefined").is_some() {
                return Value::Undefined;
            }
            if let Some(Json::String(bits)) = object.get("$f64") {
                assert_eq!(bits.len(), 16, "a number's eight bytes");
                let bytes = std::array::from_fn(|i| u8::from_str_radix(&bits[i * 2..i * 2 + 2], 16).unwrap());
                return Value::Num(f64::from_le_bytes(bytes));
            }
            let fields = object.entries().map(|(k, v)| (k.to_string(), frozen_value(v, arena))).collect();
            let index = arena.nodes.len();
            arena.nodes.push(Node::Object { class: String::new(), fields });
            Value::Node(index as u32)
        }
    }
}

fn check_case(kind: &str, case: V) -> Result<(), String> {
    match kind {
        "pilot" => pilot(case),
        "raise" => raise(case),
        "assist" => assist(case),
        "missile" => missile(case),
        "transport" => transport(case),
        "class" => class(case),
        "keys" => keys(case),
        "tuning" => tuning(case),
        other => Err(format!("no kind {other}")),
    }
}

#[test]
fn flight_functions_match_frozen_cases() {
    let data = bfe_content::json::parse(include_str!("fixtures/flight-functions.json")).expect("synthetic flight cases");
    let cases = match data.get("cases") {
        Some(bfe_content::Value::Array(cases)) => cases,
        _ => panic!("a nonempty case list"),
    };
    assert_eq!(cases.len(), 256, "frozen case count");
    let mut kinds = std::collections::BTreeSet::new();
    for (i, case) in cases.iter().enumerate() {
        let kind = match case.get("kind") {
            Some(bfe_content::Value::String(kind)) => kind.as_ref(),
            _ => panic!("a case kind"),
        };
        kinds.insert(kind);
        let mut arena = Arena::default();
        let value = frozen_value(case.get("value").expect("case values"), &mut arena);
        if let Err(error) = check_case(kind, arena.view(&value)) {
            panic!("case {i} ({kind}): {error}");
        }
    }
    assert_eq!(kinds.len(), 7, "every generated function family");
}

#[test]
fn flight_functions_match_external_cases() {
    let Ok(path) = std::env::var("BFE_FLIGHT_CASES") else { return };
    let bytes = std::fs::read(&path).expect("the cases");
    let mut reader = Reader::new(&bytes).expect("a BFECAP file");
    let mut count = 0;
    while let Some(record) = reader.next_record() {
        let record = record.expect("a record");
        let Some(kind) = record.name.strip_prefix("flight.") else { continue };
        count += 1;
        if let Err(error) = check_case(kind, record.root()) {
            panic!("case {} ({kind}): {error}", record.tick);
        }
    }
    assert!(count > 0, "the external corpus must contain flight cases");
}
