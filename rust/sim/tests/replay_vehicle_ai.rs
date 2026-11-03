// The vehicle AI (src/ai/classic/vehicles/*) replayed against every frame of
// the corpus BFE_REPLAY_CORPUS names (tools/sim-capture.mjs --specs
// vehicleAI): each frame starts from the views its capture recorded
// (tools/sim-capture/vehicle-ai.mjs: the hulls, turrets, leases, units,
// controllers and agents it reads, the service's own state), runs the Rust
// port (rust/sim/src/ai/vehicles), serves every call across its boundary
// from the capture with its arguments checked bit for bit and every draw of
// generator A from the capture's, and compares what it returns and what it
// writes (its own state, the steering bags and intents it keeps on the
// controls, the hulls' decay, the controllers', agents' and patterns'
// fields it sets) with the capture's. Navigation's graph and barriers come
// from the map's content (BFE_CONTENT_DIR). Without a corpus the test
// passes without replaying.
#![cfg(feature = "replay")]

use std::collections::{HashMap, HashSet};

use bfe_content::json;
use bfe_sim::ai::nav::{Nav, PlannerOptions};
use bfe_sim::ai::vehicles::agents::{AgentCx, AgentHost, BoardAgent, FlyAgent, Fsm, UnBoardAgent};
use bfe_sim::ai::vehicles::controllers::{MountedAgent, Policy};
use bfe_sim::ai::vehicles::fly::{self, FlyHost};
use bfe_sim::ai::vehicles::patterns::{AttackRun, PatternsView};
use bfe_sim::ai::vehicles::random::Draws;
use bfe_sim::ai::vehicles::reservations::Reservation;
use bfe_sim::ai::vehicles::service::{AnalogOptions, Decay, PatrolView, StationaryHelper, VehicleAI};
use bfe_sim::ai::vehicles::world::*;
use bfe_sim::vehicles::{BodyId, Hit};
use bfe_sim::ai::vehicles::replay::*;
use sim_replay::value::show_number;
use sim_replay::{Call, Capture, Corpus, Frame, Log, Node, Outcome, Report, V};
use three::Vector3;

/// A frame's start, as views; and its world (the calls, the draws, what
/// the port writes).
struct Rw<'f, 'n> {
    log: Log<'f>,
    ids: Ids,
    error: Option<String>,
    now: f64,
    settings: Settings,
    units: HashMap<Obj, UnitView>,
    hulls: HashMap<Obj, HullView>,
    hull_order: Vec<Obj>,
    turrets: HashMap<Obj, TurretView>,
    turret_order: Vec<Obj>,
    leases: HashMap<Obj, LeaseView>,
    controllers: HashMap<Obj, ControllerView>,
    characters: HashMap<Obj, (Option<Obj>, Option<Obj>)>,
    posts: HashMap<Obj, PostView>,
    post_order: Vec<Obj>,
    weapons: HashMap<Obj, Vec<WeaponView>>,
    reinforcements: Vec<(f64, Option<f64>)>,
    human_units: Vec<Obj>,
    nav: Option<&'n mut Nav>,
    has_planner: bool,
    has_graph: bool,
    steer: HashMap<Obj, Option<Steer>>,
    intents: HashMap<Obj, Option<SeatIntent>>,
    decay: HashMap<Obj, Option<Decay>>,
    // What the port writes of other packages' objects.
    reboard_at: HashMap<Obj, f64>,
    fire_a: HashSet<Obj>,
    request_invalid: HashSet<Obj>,
    objectives: HashMap<Obj, Obj>,
    support_holds: HashMap<Obj, f64>,
    patrol: Option<(Option<f64>, Option<f64>)>,
    intent_analogs: HashMap<Obj, Stick>,
    // The frame's agent and its helper (as the frame found them).
    agent: Option<Obj>,
    helper_kind: Option<String>,
    helper_arrived: bool,
    helper_failed: bool,
    fly_fields: Option<(Option<Vector3>, Option<(bool, f64, f64)>)>,
    // The controller of the frame's agent or helper.
    fly_controller: Option<Obj>,
    // The controllers' fields the vehicle AI writes, as the frame found them.
    ctl_fields: HashMap<Obj, CtlFields>,
    // supportPoint's helper: its mask, radius and unit.
    helper_mask: Option<(f64, f64, Option<Obj>)>,
    // The movement request the frame was handed (a control's).
    request: Option<Request>,
    request_handle: Option<String>,
    hints: HashMap<Obj, HintView>,
}

impl<'f, 'n> Rw<'f, 'n> {
    fn new(_capture: &'f Capture, frame: &'f Frame, state: V<'f>, nav: Option<&'n mut Nav>) -> Rw<'f, 'n> {
        let mut w = Rw {
            log: Log::new(frame),
            ids: Ids::default(),
            error: None,
            now: num_or_nan(state.get("time")),
            settings: Settings::default(),
            units: HashMap::new(),
            hulls: HashMap::new(),
            hull_order: Vec::new(),
            turrets: HashMap::new(),
            turret_order: Vec::new(),
            leases: HashMap::new(),
            controllers: HashMap::new(),
            characters: HashMap::new(),
            posts: HashMap::new(),
            post_order: Vec::new(),
            weapons: HashMap::new(),
            reinforcements: Vec::new(),
            human_units: Vec::new(),
            nav,
            has_planner: false,
            has_graph: false,
            steer: HashMap::new(),
            intents: HashMap::new(),
            decay: HashMap::new(),
            reboard_at: HashMap::new(),
            fire_a: HashSet::new(),
            request_invalid: HashSet::new(),
            objectives: HashMap::new(),
            support_holds: HashMap::new(),
            patrol: None,
            intent_analogs: HashMap::new(),
            agent: None,
            helper_kind: None,
            helper_arrived: false,
            helper_failed: false,
            fly_fields: None,
            fly_controller: None,
            ctl_fields: HashMap::new(),
            helper_mask: None,
            request: None,
            request_handle: None,
            hints: HashMap::new(),
        };
        w.load(state);
        w
    }

    fn obj(&mut self, v: V) -> Option<Obj> {
        self.ids.of(v)
    }

    fn load(&mut self, s: V<'f>) {
        let settings = s.get("settings");
        let controls = settings.get("controls");
        let fly = controls.get("flyHeight");
        let dense = controls.get("dense");
        let dense_value = if dense.is_nullish() { dense } else { let x = dense.get("value"); if x.is_nullish() { dense } else { x } };
        self.settings = Settings {
            min_fly_height: opt_num(fly.get("min")).unwrap_or(-200.0),
            max_fly_height: opt_num(fly.get("max")).unwrap_or(200.0),
            notify_radius: opt_num(controls.get("vehicleNotifyRadius")).unwrap_or(50.0),
            dense: dense_value.truthy(),
            stay_in_turrets: controls.get("stayInTurrets").truthy(),
            cycle: num_or_nan(settings.get("cycle")),
        };
        self.has_planner = settings.get("hasPlanner").truthy();
        self.has_graph = settings.get("hasGraph").truthy();
        let r = settings.get("reinforcements");
        for team in [1.0, 2.0] {
            let v = match r.node() {
                Some(Node::Array(_)) => r.at(team as usize),
                _ => r.get(&show_number(team)),
            };
            self.reinforcements.push((team, v.opt_num()));
        }
        for u in settings.get("humanUnits").items() {
            if let Some(o) = self.obj(u) {
                self.human_units.push(o);
            }
        }
        for p in settings.get("posts").items() {
            let o = self.obj(p.get("self")).expect("post");
            self.post_order.push(o);
            self.posts.insert(o, PostView { obj: o, position: vec3(p.get("position")), team: num_or_nan(p.get("team")) });
        }
        for h in settings.get("hulls").items() {
            let o = self.obj(h).expect("hull");
            self.hull_order.push(o);
        }
        for t in settings.get("turrets").items() {
            let o = self.obj(t).expect("turret");
            self.turret_order.push(o);
        }
        for h in s.get("hulls").items() {
            let o = self.obj(h.get("self")).expect("hull");
            let seats = h
                .get("seats")
                .items()
                .map(|seat| SeatView {
                    lease: self.ids.of(seat.get("lease")),
                    weapons: num_or_nan(seat.get("weapons")) as usize,
                    yaw_limits: seat.get("yawLimits").str().map(String::from),
                    pitch_limits: seat.get("pitchLimits").str().map(String::from),
                })
                .collect();
            let decay = h.get("decay");
            self.decay.insert(o, if decay.is_nullish() { None } else { Some(Decay { idle: num_or_nan(decay.get("idle")), sample: num_or_nan(decay.get("sample")) }) });
            let view = HullView {
                obj: o,
                entry: self.ids.of(h.get("entry")).expect("hull entry"),
                alive: h.get("alive").truthy(),
                name: h.get("name").str().unwrap_or("").to_string(),
                flying: h.get("flying").truthy(),
                walking: h.get("walking").truthy(),
                grounded: h.get("grounded").truthy(),
                flight_state: h.get("flightState").str().map(String::from),
                gear: num_or_nan(h.get("gear")),
                speed: num_or_nan(h.get("speed")),
                max_speed_setting: parse_float(h.get("maxSpeed")),
                forward_speed_setting: parse_float(h.get("forwardSpeed")),
                label: h.get("label").str().map(String::from),
                position: vec3(h.get("position")),
                velocity: vec3(h.get("velocity")),
                yaw: num_or_nan(h.get("yaw")),
                quaternion: quat(h.get("quaternion")),
                matrix: mat4(h.get("matrix")),
                bounds_min: vec3(h.get("boundsMin")),
                bounds_max: vec3(h.get("boundsMax")),
                bound_radius: num_or_nan(h.get("boundRadius")),
                size: vec3(h.get("size")),
                body: body_of(h.get("body")),
                seats,
                occupants: num_or_nan(h.get("occupants")),
                driver: h.get("driver").truthy(),
                free_seats: num_or_nan(h.get("freeSeats")),
                occupied: h.get("occupied").truthy(),
                controlling_team: num_or_nan(h.get("controllingTeam")),
                tow_target: num_or_nan(h.get("towTarget")),
                slot_post: h.get("slotPost").truthy(),
                decay_time: opt_num(h.get("decayTime")),
                max_health: num_or_nan(h.get("maxHealth")),
                own_system: h.get("ownSystem").truthy(),
            };
            self.hulls.insert(o, view);
        }
        for t in s.get("turrets").items() {
            let o = self.obj(t.get("self")).expect("turret");
            let limits = |v: V| if v.len() >= 2 { Some([num_or_nan(v.at(0)), num_or_nan(v.at(1))]) } else { None };
            let view = TurretView {
                obj: o,
                entry: self.ids.of(t.get("entry")),
                alive: t.get("alive").truthy(),
                gunner: t.get("gunner").truthy(),
                lease: self.ids.of(t.get("lease")),
                facing: num_or_nan(t.get("facing")),
                yaw_limits_setting: t.get("yawLimitsSetting").str().map(String::from),
                pitch_limits_setting: t.get("pitchLimitsSetting").str().map(String::from),
                pitch_limits: limits(t.get("pitchLimits")),
                yaw_limits: if t.get("yawLimits").is_nullish() { None } else { limits(t.get("yawLimits")).or(Some([f64::NAN, f64::NAN])) },
                has_hold_yaw: t.get("holdYaw").truthy(),
            };
            self.turrets.insert(o, view);
        }
        for l in s.get("leases").items() {
            let o = self.obj(l.get("self")).expect("lease");
            let view = LeaseView {
                obj: o,
                released: l.get("released").truthy(),
                human: l.get("human").truthy(),
                live: l.get("live").truthy(),
                unit: self.ids.of(l.get("unit")),
                controllable: self.ids.of(l.get("controllable")),
                vehicle: self.ids.of(l.get("vehicle")),
                turret: self.ids.of(l.get("turret")),
                index: num_or_nan(l.get("index")),
                team: num_or_nan(l.get("team")),
            };
            self.leases.insert(o, view);
        }
        for u in s.get("units").items() {
            let o = self.obj(u.get("self")).expect("unit");
            let body_yaw = opt_num(u.get("bodyYaw")).or_else(|| opt_num(u.get("aimYaw"))).unwrap_or(0.0);
            let view = UnitView {
                obj: o,
                kind: num_or_nan(u.get("kind")),
                id: num_or_nan(u.get("id")),
                generation: num_or_nan(u.get("generation")),
                alive: u.get("alive").truthy(),
                team: num_or_nan(u.get("team")),
                position: vec3_of(u.get("position")).unwrap_or_default(),
                velocity: vec3_of(u.get("velocity")).unwrap_or_default(),
                body_yaw,
                class: class_view(u.get("classDef")),
                class_name: u.get("className").str().map(String::from),
                soldier: u.get("soldier").truthy(),
                soldier_max_speed: opt_num(u.get("soldierMaxSpeed")),
                walker: u.get("walker").truthy(),
                human: u.get("human").truthy(),
                network_human: u.get("networkHuman").truthy(),
                targetable: u.get("targetable").truthy(),
                seat: self.ids.of(u.get("seat")),
                lease: self.ids.of(u.get("lease")),
                vehicle: self.ids.of(u.get("vehicle")),
                turret: self.ids.of(u.get("turret")),
                controller: self.ids.of(u.get("controller")),
                character: self.ids.of(u.get("character")),
                up_y: opt_num(u.get("upY")),
                own_label: u.get("ownLabel").str().map(String::from),
                own_name: u.get("ownName").str().map(String::from),
            };
            let steer = steer_of(&mut self.ids, u.get("steer"));
            self.steer.insert(o, steer);
            self.intents.insert(o, intent_of(u.get("intent")));
            if u.has("weapons") && !u.get("weapons").is_undefined() {
                let list = u.get("weapons").items().map(|w| WeaponView { base: w.get("base").str().map(String::from), channel: num_or_nan(w.get("channel")) }).collect();
                self.weapons.insert(o, list);
            }
            self.units.insert(o, view);
        }
        for c in s.get("controllers").items() {
            let o = self.obj(c.get("self")).expect("controller");
            let view = ControllerView {
                obj: o,
                unit: self.ids.of(c.get("unit")),
                nav_control: c.get("navControl").truthy(),
                executor: c.get("executor").truthy(),
                agent: self.ids.of(c.get("agent")),
                executor_status: num_or_nan(c.get("executorStatus")),
                target: c.get("target").truthy(),
                helper: c.get("helper").truthy(),
                helper_has_path: c.get("helperHasPath").truthy(),
                helper_state: opt_num(c.get("helperState")),
                helper_destination: vec3_of(c.get("helperDestination")),
                is_unit_controller: c.get("isUnitController").truthy(),
                fire_channel: opt_num(c.get("fireChannel")),
            };
            self.controllers.insert(o, view);
            self.ctl_fields.insert(o, CtlFields::of(c));
        }
        for ch in s.get("characters").items() {
            let o = self.obj(ch.get("self")).expect("character");
            let unit = self.ids.of(ch.get("unit"));
            let controller = self.ids.of(ch.get("controller"));
            self.characters.insert(o, (unit, controller));
        }
        let agent = s.get("agent");
        if !agent.is_nullish() {
            self.agent = self.ids.of(agent.get("self"));
            self.fly_controller = self.ids.of(agent.get("controller"));
            let helper = agent.get("helper");
            if !helper.is_nullish() {
                self.helper_kind = helper.get("kind").str().map(String::from);
                self.helper_arrived = helper.get("arrived").truthy();
                self.helper_failed = helper.get("failed").truthy();
            }
        }
        let helper = s.get("helper");
        if !helper.is_nullish() {
            self.helper_kind = helper.get("kind").str().map(String::from);
            self.fly_controller = self.ids.of(helper.get("controller"));
            self.helper_arrived = helper.get("arrived").truthy();
            self.helper_failed = helper.get("failed").truthy();
        }
        let request = s.get("request");
        if !request.is_nullish() {
            self.request = Some(request_of(request));
        }
        let hm = s.get("helperMask");
        if !hm.is_nullish() {
            let unit = self.ids.of(hm.get("unit"));
            self.helper_mask = Some((num_or_nan(hm.get("mask")), num_or_nan(hm.get("radius")), unit));
        }
    }

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

    fn check_num(&mut self, what: &str, v: V, mine: f64) {
        if !v.opt_num().is_some_and(|x| same(x, mine)) {
            self.fail(format!("{what}: the capture has {}, the port {}", v.show(), show_number(mine)));
        }
    }
    fn check_bool(&mut self, what: &str, v: V, mine: bool) {
        if v.opt_bool() != Some(mine) {
            self.fail(format!("{what}: the capture has {}, the port {mine}", v.show()));
        }
    }
    fn check_vec(&mut self, what: &str, v: V, mine: &Vector3) {
        if !vec3_of(v).is_some_and(|x| same_vec(&x, mine)) {
            self.fail(format!("{what}: the capture has {}, the port ({}, {}, {})", v.show(), show_number(mine.x), show_number(mine.y), show_number(mine.z)));
        }
    }
    fn check_obj(&mut self, what: &str, v: V, mine: Option<Obj>) {
        let theirs = if v.is_nullish() { None } else { self.ids.of(v) };
        if theirs != mine {
            let m = mine.map(|o| self.ids.name(o)).unwrap_or_else(|| "none".into());
            self.fail(format!("{what}: the capture has {}, the port {m}", v.show()));
        }
    }
    fn hit_of(&mut self, v: V) -> Option<Hit> {
        if v.is_nullish() || v.opt_bool() == Some(false) {
            return None;
        }
        Some(Hit {
            distance: num_or_nan(v.get("distance")),
            point: vec3_of(v.get("point")).unwrap_or_default(),
            normal: vec3_of(v.get("normal")).unwrap_or_default(),
            terrain: v.get("terrain").truthy(),
            body: body_of(v.get("body")),
        })
    }
    fn exit_options(&mut self, what: &str, v: V, mine: &ExitOptions) {
        match mine.post {
            None => {
                if v.has("post") {
                    self.fail(format!("{what}.post: the capture has {}, the port none", v.get("post").show()));
                }
            }
            Some(post) => self.check_obj(&format!("{what}.post"), v.get("post"), post),
        }
        for (k, mine) in [("promote", mine.promote), ("placement", mine.placement)] {
            let theirs = if v.has(k) { v.get(k).opt_bool() } else { None };
            if theirs != mine {
                self.fail(format!("{what}.{k}: the capture has {}, the port {mine:?}", v.get(k).show()));
            }
        }
    }
}

/// A controller's fields the vehicle AI writes.
#[derive(Clone, Debug, PartialEq)]
struct CtlFields {
    reboard_at: f64,
    fire_a: bool,
    request_valid: Option<bool>,
    policy: Option<String>,
    policy_adapted: bool,
    home_facing: Option<bool>,
}

impl CtlFields {
    fn of(c: V) -> CtlFields {
        CtlFields {
            reboard_at: num_or_nan(c.get("reboardAt")),
            fire_a: c.get("fireA").truthy(),
            request_valid: c.get("requestValid").opt_bool(),
            policy: c.get("policy").str().map(String::from),
            policy_adapted: c.get("policyAdapted").truthy(),
            home_facing: c.get("homeFacing").opt_bool(),
        }
    }
    fn same(&self, o: &CtlFields) -> bool {
        same(self.reboard_at, o.reboard_at)
            && self.fire_a == o.fire_a
            && self.request_valid == o.request_valid
            && self.policy == o.policy
            && self.policy_adapted == o.policy_adapted
            && self.home_facing == o.home_facing
    }
}

impl Draws for Rw<'_, '_> {
    fn next_a(&mut self) -> u32 {
        if self.error.is_some() {
            return 0;
        }
        match self.log.draw("A") {
            Ok(x) => x as u32,
            Err(e) => {
                self.error = Some(e.0);
                0
            }
        }
    }
}

impl World for Rw<'_, '_> {
    fn now(&self) -> f64 {
        self.now
    }
    fn settings(&self) -> &Settings {
        &self.settings
    }
    fn unit(&self, u: Obj) -> UnitView {
        self.units.get(&u).cloned().unwrap_or_else(|| panic!("no view of unit {}", self.ids.name(u)))
    }
    fn hull(&self, h: Obj) -> HullView {
        self.hulls.get(&h).cloned().unwrap_or_else(|| panic!("no view of hull {}", self.ids.name(h)))
    }
    fn turret(&self, t: Obj) -> TurretView {
        self.turrets.get(&t).cloned().unwrap_or_else(|| panic!("no view of turret {}", self.ids.name(t)))
    }
    fn lease(&self, l: Obj) -> LeaseView {
        self.leases.get(&l).cloned().unwrap_or_else(|| panic!("no view of lease {}", self.ids.name(l)))
    }
    fn controller(&self, c: Obj) -> ControllerView {
        self.controllers.get(&c).cloned().unwrap_or_else(|| panic!("no view of controller {}", self.ids.name(c)))
    }
    fn post(&self, p: Obj) -> PostView {
        *self.posts.get(&p).unwrap_or_else(|| panic!("no view of post {}", self.ids.name(p)))
    }
    fn character_unit(&self, c: Obj) -> Option<Obj> {
        self.characters.get(&c).unwrap_or_else(|| panic!("no view of character {}", self.ids.name(c))).0
    }
    fn character_controller(&self, c: Obj) -> Option<Obj> {
        self.characters.get(&c).unwrap_or_else(|| panic!("no view of character {}", self.ids.name(c))).1
    }
    fn hulls(&self) -> Vec<Obj> {
        self.hull_order.clone()
    }
    fn turrets(&self) -> Vec<Obj> {
        self.turret_order.clone()
    }
    fn weapons(&self, control: Obj) -> Option<Vec<WeaponView>> {
        self.weapons.get(&control).cloned()
    }
    fn reinforcements(&self, team: f64) -> Option<f64> {
        self.reinforcements.iter().find(|(t, _)| *t == team).and_then(|(_, v)| *v)
    }
    fn human_units(&mut self) -> Vec<Obj> {
        self.human_units.clone()
    }
    fn has_planner(&self) -> bool {
        self.has_planner
    }
    fn has_graph(&self) -> bool {
        self.has_graph
    }
    fn nav(&mut self) -> Option<&mut Nav> {
        self.nav.as_deref_mut()
    }
    fn support_holds(&self, agent: Obj) -> Option<f64> {
        self.support_holds.get(&agent).copied()
    }
    fn hint_link(&self, _hint: Obj) -> Option<Obj> {
        None
    }
    fn steer(&mut self, control: Obj) -> &mut Option<Steer> {
        self.steer.entry(control).or_insert(None)
    }
    fn intent(&mut self, control: Obj) -> Option<&mut SeatIntent> {
        self.intents.get_mut(&control).and_then(|i| i.as_mut())
    }
    fn decay(&mut self, hull: Obj) -> &mut Option<Decay> {
        self.decay.entry(hull).or_insert(None)
    }

    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> {
        let call = self.call("collision.cast")?;
        self.check_vec("cast origin", call.arg(0), origin);
        self.check_vec("cast direction", call.arg(1), direction);
        self.check_num("cast far", call.arg(2), far);
        let theirs = body_of(call.arg(4));
        if theirs != ignore {
            self.fail(format!("cast ignore: the capture has {}, the port {ignore:?}", call.arg(4).show()));
        }
        self.hit_of(call.result())
    }
    fn query(&mut self, center: &Vector3, radius: f64, _filter: QueryFilter) -> Vec<Obj> {
        let Some(call) = self.call("units.query") else { return Vec::new() };
        self.check_vec("query center", call.arg(0), center);
        self.check_num("query radius", call.arg(1), radius);
        let found: Vec<V> = call.result().items().collect();
        found.into_iter().filter_map(|u| self.ids.of(u)).collect()
    }
    fn relation(&mut self, a: f64, b: f64) -> f64 {
        let Some(call) = self.call("world.relation") else { return 0.0 };
        self.check_num("relation a", call.arg(0), a);
        self.check_num("relation b", call.arg(1), b);
        num_or_nan(call.result())
    }
    fn los(&mut self, a: &Vector3, b: &Vector3) -> Option<bool> {
        let call = self.call("world.los")?;
        self.check_vec("los a", call.arg(0), a);
        self.check_vec("los b", call.arg(1), b);
        Some(call.result().truthy())
    }
    fn sphere(&mut self, x: Obj) -> Sphere {
        let Some(call) = self.call("port.sphere") else { return Sphere::default() };
        self.check_obj("sphere of", call.arg(0), Some(x));
        let r = call.result();
        Sphere { center: vec3_of(r.get("center")).unwrap_or_default(), radius: num_or_nan(r.get("radius")) }
    }
    fn origin(&mut self, _control: Obj) -> Vector3 {
        let Some(call) = self.call("seat.origin") else { return Vector3::default() };
        vec3_of(call.result()).unwrap_or_default()
    }
    fn forward(&mut self, _control: Obj) -> Vector3 {
        let Some(call) = self.call("seat.forward") else { return Vector3::default() };
        vec3_of(call.result()).unwrap_or_default()
    }
    fn mount_origin(&mut self, origin: Origin) -> Vector3 {
        match origin {
            Origin::Lease(_) => self.call("lease.origin").and_then(|c| vec3_of(c.result())).unwrap_or_default(),
            Origin::TurretPivot(t) => {
                let Some(call) = self.call("port.turretPivot") else { return Vector3::default() };
                self.check_obj("turretPivot of", call.arg(0), Some(t));
                vec3_of(call.result()).unwrap_or_default()
            }
            Origin::SeatOrigin(_, i) => {
                let Some(call) = self.call("vehicle.seatOrigin") else { return Vector3::default() };
                self.check_num("seatOrigin index", call.arg(0), i as f64);
                vec3_of(call.result()).unwrap_or_default()
            }
        }
    }
    fn hold_yaw(&mut self, _turret: Obj, a: f64) -> f64 {
        let Some(call) = self.call("turret.holdYaw") else { return 0.0 };
        self.check_num("holdYaw yaw", call.arg(0), a);
        num_or_nan(call.result())
    }
    fn eligible_seat(&mut self, unit: Obj, hull: Obj) -> f64 {
        let Some(call) = self.call("vehicles.eligibleSeat") else { return -1.0 };
        self.check_obj("eligibleSeat unit", call.arg(0), Some(unit));
        self.check_obj("eligibleSeat hull", call.arg(1), Some(hull));
        num_or_nan(call.result())
    }
    fn first_free_seat(&mut self, _hull: Obj) -> f64 {
        self.call("vehicle.firstFreeSeat").map(|c| num_or_nan(c.result())).unwrap_or(-1.0)
    }
    fn board(&mut self, unit: Obj, hull: Obj) -> bool {
        let Some(call) = self.call("vehicles.board") else { return false };
        self.check_obj("board unit", call.arg(0), Some(unit));
        self.check_obj("board hull", call.arg(1), Some(hull));
        call.result().truthy()
    }
    fn man(&mut self, unit: Obj, turret: Obj) -> bool {
        let Some(call) = self.call("turrets.man") else { return false };
        self.check_obj("man unit", call.arg(0), Some(unit));
        self.check_obj("man turret", call.arg(1), Some(turret));
        call.result().truthy()
    }
    fn request_exit_vehicle(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool {
        let Some(call) = self.call("vehicles.requestExit") else { return false };
        self.check_obj("requestExit unit", call.arg(0), unit);
        self.exit_options("requestExit options", call.arg(1), &options);
        call.result().truthy()
    }
    fn request_exit_turret(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool {
        let Some(call) = self.call("turrets.requestExit") else { return false };
        self.check_obj("turret requestExit unit", call.arg(0), unit);
        self.exit_options("turret requestExit options", call.arg(1), &options);
        call.result().truthy()
    }
    fn unboard(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool {
        let Some(call) = self.call("vehicles.unboard") else { return false };
        self.check_obj("unboard unit", call.arg(0), unit);
        self.exit_options("unboard options", call.arg(1), &options);
        call.result().truthy()
    }
    fn human_target(&mut self, hull: Obj) -> Option<Obj> {
        let call = self.call("vehicles.humanTarget")?;
        self.check_obj("humanTarget hull", call.arg(0), Some(hull));
        let r = call.result();
        if r.is_nullish() {
            None
        } else {
            self.ids.of(r)
        }
    }
    fn damage(&mut self, _hull: Obj, amount: f64) {
        if let Some(call) = self.call("vehicle.damage") {
            self.check_num("damage", call.arg(0), amount);
        }
    }
    fn pilot_input(&mut self, control: Obj, request: Option<&Request>, aim: Option<&Vector3>, dt: f64, env: &PilotEnv) -> Stick {
        let Some(call) = self.call("port.pilotInput") else { return Stick::default() };
        self.check_obj("pilot unit", call.arg(0), Some(control));
        let r = call.arg(1);
        match (r.handle().is_some(), request) {
            (true, _) => {
                let theirs = Ids::key(r);
                if theirs != self.request_handle {
                    self.fail(format!("pilot request: the capture has {}, the port {:?}", r.show(), self.request_handle));
                }
            }
            (false, Some(mine)) => {
                let theirs = request_of(r);
                if theirs.valid != mine.valid
                    || !same_vec(&theirs.steering_target, &mine.steering_target)
                    || !same(theirs.mode, mine.mode)
                    || !same_opt(theirs.blend, mine.blend)
                    || !same_opt(theirs.throttle, mine.throttle)
                {
                    self.fail(format!("pilot request: the capture has {}, the port {mine:?}", r.show()));
                }
            }
            (false, None) => {
                if !r.is_nullish() {
                    self.fail(format!("pilot request: the capture has {}, the port none", r.show()));
                }
            }
        }
        match aim {
            Some(a) => self.check_vec("pilot aim", call.arg(2), a),
            None => {
                if !call.arg(2).is_nullish() {
                    self.fail(format!("pilot aim: the capture has {}, the port none", call.arg(2).show()));
                }
            }
        }
        self.check_num("pilot dt", call.arg(3), dt);
        let e = call.arg(4);
        self.check_bool("pilot env.hasPath", e.get("hasPath"), env.has_path);
        self.check_num("pilot env.minFlyHeight", e.get("minFlyHeight"), env.min_fly_height);
        self.check_num("pilot env.maxFlyHeight", e.get("maxFlyHeight"), env.max_fly_height);
        let o = call.out(5);
        Stick {
            forward: num_or_nan(o.get("forward")),
            right: num_or_nan(o.get("right")),
            turn: num_or_nan(o.get("turn")),
            pitch: num_or_nan(o.get("pitch")),
            up: num_or_nan(o.get("up")),
            down: num_or_nan(o.get("down")),
            brake: o.get("brake").truthy(),
            trick: o.get("trick").truthy(),
            immelmann: o.get("immelmann").truthy(),
            boost: o.get("boost").truthy(),
        }
    }
    fn nav_attach(&mut self, _controller: Option<Obj>, unit: Obj) {
        if let Some(call) = self.call("navControl.attach") {
            self.check_obj("attach unit", call.arg(0), Some(unit));
        }
    }
    fn nav_update(&mut self, _controller: Option<Obj>, dt: f64) {
        if let Some(call) = self.call("navControl.update") {
            self.check_num("update dt", call.arg(0), dt);
        }
    }
    fn nav_request(&mut self, _controller: Option<Obj>) -> Option<Request> {
        self.request
    }
    fn reserve_hint(&mut self, _hint: Obj, controller: Obj, now: f64) {
        if let Some(call) = self.call("hints.reserve") {
            self.check_obj("hint owner", call.arg(1), Some(controller));
            self.check_num("hint now", call.arg(2), now);
        }
    }
    fn set_hint_reserved_at(&mut self, _hint: Obj, _at: f64) {}
    fn set_intent_analogs(&mut self, control: Obj, stick: &Stick) {
        self.intent_analogs.insert(control, *stick);
    }
    fn request_objective(&mut self, controller: Obj, exclude: bool) {
        let _ = controller;
        if let Some(call) = self.call("controller.requestObjective") {
            self.check_bool("requestObjective exclude", call.arg(0), exclude);
        }
    }
    fn controller_request_exit(&mut self, _controller: Obj) {
        self.call("controller.requestExit");
    }
    fn set_reboard_at(&mut self, controller: Obj, t: f64) {
        self.reboard_at.insert(controller, t);
    }
    fn fire_a(&mut self, controller: Obj) {
        self.fire_a.insert(controller);
    }
    fn invalidate_request(&mut self, controller: Obj) {
        self.request_invalid.insert(controller);
    }
    fn add_vehicle_note(&mut self, _controller: Obj, note: Ref) {
        if let Some(call) = self.call("controller.addVehicleNote") {
            let a = call.arg(0);
            self.check_obj("note unit", a.get("unit"), Some(note.obj));
            self.check_num("note generation", a.get("generation"), note.generation);
        }
    }
    fn set_objective(&mut self, character: Obj, post: Obj) {
        self.objectives.insert(character, post);
    }
    fn set_support_holds(&mut self, agent: Obj, holds: f64) {
        self.support_holds.insert(agent, holds);
    }
    fn set_patrol_expires(&mut self, _patrol: Obj, t: f64) {
        let p = self.patrol.get_or_insert((None, None));
        p.0 = Some(t);
    }
    fn patrol_request_state(&mut self, _patrol: Obj, state: f64) {
        let p = self.patrol.get_or_insert((None, None));
        p.1 = Some(state);
    }
    fn cp_center(&mut self, post: Obj) -> Vector3 {
        let Some(call) = self.call("port.cpCenter") else { return Vector3::default() };
        self.check_obj("cpCenter post", call.arg(1), Some(post));
        vec3_of(call.result()).unwrap_or_default()
    }
    fn capture_region(&mut self, post: Obj) -> Option<(f64, f64)> {
        let call = self.call("port.captureRegion")?;
        self.check_obj("captureRegion post", call.arg(1), Some(post));
        let r = call.result();
        if r.is_nullish() {
            None
        } else {
            Some((opt_num(r.get("sizeX")).unwrap_or(3.0), opt_num(r.get("sizeZ")).unwrap_or(3.0)))
        }
    }
    fn helper_traversal(&mut self, _agent: Obj, radius: bool) -> f64 {
        let (mask, r, unit) = self.helper_mask.unwrap_or((f64::NAN, f64::NAN, None));
        if let Some(call) = self.call("navControl.attach") {
            self.check_obj("attach unit", call.arg(0), unit);
        }
        if radius {
            r
        } else {
            mask
        }
    }
    fn memory_point(&mut self, _agent: Obj) -> Option<Vector3> {
        let call = self.call("combat.memoryPoint")?;
        vec3_of(call.result())
    }
    fn select_weapon(&mut self, controller: Obj, control: Obj, target: Obj, switch_allowed: bool) -> Choice {
        let Some(call) = self.call("port.selectWeapon") else { return Choice::default() };
        self.check_obj("selectWeapon controller", call.arg(0), Some(controller));
        self.check_obj("selectWeapon unit", call.arg(1), Some(control));
        self.check_obj("selectWeapon target", call.arg(2), Some(target));
        self.check_bool("selectWeapon switchAllowed", call.arg(3), switch_allowed);
        let o = call.out(4);
        let weapon = o.get("weapon");
        Choice {
            ok: o.get("ok").truthy(),
            action: num_or_nan(o.get("action")),
            in_band: o.get("inBand").truthy(),
            weapon: if weapon.is_nullish() { None } else { self.ids.of(weapon) },
        }
    }
    fn switch_weapon(&mut self, controller: Obj, control: Obj, index: usize) -> bool {
        let Some(call) = self.call("port.switchWeapon") else { return false };
        self.check_obj("switchWeapon controller", call.arg(0), Some(controller));
        self.check_obj("switchWeapon unit", call.arg(1), Some(control));
        self.check_num("switchWeapon index", call.arg(2), index as f64);
        call.result().truthy()
    }
    fn face(&mut self, _patterns: Obj, posture: f64) {
        if let Some(call) = self.call("patterns.face") {
            self.check_num("face posture", call.arg(0), posture);
        }
    }
    fn set_delay(&mut self, _agent: Obj, seconds: f64, now: f64) {
        if let Some(call) = self.call("tracker.setDelay") {
            self.check_num("setDelay seconds", call.arg(0), seconds);
            self.check_num("setDelay now", call.arg(1), now);
        }
    }
    fn default_delay(&mut self, _agent: Obj, now: f64) {
        if let Some(call) = self.call("tracker.defaultDelay") {
            self.check_num("defaultDelay now", call.arg(0), now);
        }
    }
    fn helper_goto(&mut self, agent: Obj, point: &Vector3, direct: bool) {
        match self.helper_kind.as_deref() {
            Some("fly") => fly::goto(self, agent, point, direct),
            Some("stationary") => {
                let controller = self.agent_controller(agent);
                StationaryHelper::default().goto(self, controller);
            }
            _ => {
                let Some(call) = self.call("groundHelper.goto") else { return };
                self.check_vec("goto point", call.arg(0), point);
                let mode = call.arg(1).get("mode");
                let theirs = mode.str() == Some("direct");
                if theirs != direct || (!direct && !mode.is_undefined()) {
                    self.fail(format!("goto mode: the capture has {}, the port direct={direct}", call.arg(1).show()));
                }
            }
        }
    }
    fn helper_stop(&mut self, agent: Obj) {
        match self.helper_kind.as_deref() {
            Some("fly") => fly::stop(self, agent),
            Some("stationary") => {
                let controller = self.agent_controller(agent);
                StationaryHelper::default().stop(self, controller);
            }
            _ => {
                self.call("groundHelper.stop");
            }
        }
    }
    fn helper_follow_path(&mut self, agent: Obj, points: &[[f64; 3]], over: bool, altitude: bool) {
        if self.helper_kind.as_deref() == Some("fly") {
            fly::follow_path(self, agent, points, over, altitude);
        }
    }
    fn helper_land(&mut self, agent: Obj, on: bool) {
        if self.helper_kind.as_deref() == Some("fly") {
            fly::land(self, agent, on);
        }
    }
}

impl Rw<'_, '_> {
    /// The controller of the frame's agent (or the helper's).
    fn agent_controller(&self, _agent: Obj) -> Obj {
        self.fly_controller.expect("the agent's controller")
    }
}

impl AgentHost for Rw<'_, '_> {
    fn interrupts_tick(&mut self, _agent: Obj) -> bool {
        self.call("interrupts.tick").is_some_and(|c| c.result().truthy())
    }
    fn update_combat_target(&mut self, _agent: Obj) {
        self.call("agent.updateCombatTarget");
    }
    fn aim(&mut self, _agent: Obj, stance: f64) {
        if let Some(call) = self.call("agent.aim") {
            self.check_num("aim stance", call.arg(0), stance);
        }
    }
    fn tick_helper(&mut self, _agent: Obj, dt: f64) {
        if let Some(call) = self.call("agent.tickHelper") {
            self.check_num("tickHelper dt", call.arg(0), dt);
        }
    }
    fn can_engage(&mut self, _agent: Obj, target: Obj) -> bool {
        let Some(call) = self.call("agent.canEngage") else { return false };
        self.check_obj("canEngage target", call.arg(0), Some(target));
        call.result().truthy()
    }
    fn has_helper(&self, _agent: Obj) -> bool {
        self.helper_kind.is_some()
    }
    fn helper_arrived(&mut self, _agent: Obj) -> bool {
        self.helper_arrived
    }
    fn helper_failed(&mut self, _agent: Obj) -> bool {
        self.helper_failed
    }
    fn posts(&self) -> Vec<Obj> {
        self.post_order.clone()
    }
    fn query_hints(&mut self, center: &Vector3, requester: Obj, now: f64) -> Option<Vec<HintView>> {
        let call = self.call("nav.queryHints")?;
        let q = call.arg(0);
        self.check_vec("queryHints center", q.get("center"), center);
        self.check_obj("queryHints requester", q.get("requester"), Some(requester));
        self.check_num("queryHints now", q.get("now"), now);
        let mut out = Vec::new();
        for h in call.result().items() {
            let name = h.get("name").str().unwrap_or("?").to_string();
            let obj = self.ids.named(format!("hint:{name}"));
            let post = h.get("commandPost");
            let view = HintView {
                obj,
                position: vec3_of(h.get("position")).unwrap_or_default(),
                forward: vec3_of(h.get("forward")).unwrap_or_default(),
                up: vec3_of(h.get("up")),
                command_post: if post.is_nullish() { None } else { self.ids.of(post) },
            };
            self.hints.insert(obj, view);
            out.push(view);
        }
        Some(out)
    }
}

impl FlyHost for Rw<'_, '_> {
    fn fly_unit(&mut self, _helper: Obj) -> Option<Obj> {
        // The helper's `this.unit`: its controller's unit, attached.
        let controller = self.fly_controller?;
        let unit = self.controllers.get(&controller).and_then(|c| c.unit)?;
        if let Some(call) = self.call("navControl.attach") {
            self.check_obj("attach unit", call.arg(0), Some(unit));
        }
        Some(unit)
    }
    fn fly_super_goto(&mut self, _helper: Obj, point: &Vector3, direct: bool) {
        let Some(call) = self.call("flyHelper.goto") else { return };
        self.check_vec("fly goto point", call.arg(0), point);
        let mode = call.arg(1).get("mode");
        if (mode.str() == Some("direct")) != direct {
            self.fail(format!("fly goto mode: the capture has {}, the port direct={direct}", call.arg(1).show()));
        }
    }
    fn fly_super_stop(&mut self, _helper: Obj) {
        self.call("flyHelper.stop");
    }
    fn fly_has_planner(&self, _helper: Obj) -> bool {
        self.has_planner
    }
    fn fly_planner_cancel(&mut self, _helper: Obj) {
        self.call("planner.cancel");
    }
    fn fly_path_new(&mut self, _helper: Obj) -> u64 {
        self.call("planner.path").and_then(|c| self.ids.of(c.result())).unwrap_or(0)
    }
    fn fly_path_prepend(&mut self, _path: u64, x: f64, y: f64, z: f64) -> bool {
        let Some(call) = self.call("path.prepend") else { return false };
        self.check_num("prepend x", call.arg(0), x);
        self.check_num("prepend y", call.arg(1), y);
        self.check_num("prepend z", call.arg(2), z);
        call.result().truthy()
    }
    fn fly_path_release(&mut self, _path: u64) {
        self.call("path.release");
    }
    fn fly_install(&mut self, _helper: Obj, path: u64, over: bool, altitude: bool) {
        let Some(call) = self.call("navControl.install") else { return };
        self.check_obj("install path", call.arg(0), Some(path));
        self.check_bool("install override", call.arg(1), over);
        self.check_bool("install altitude", call.arg(2), altitude);
    }
    fn fly_set_destination(&mut self, _helper: Obj, destination: &Vector3) {
        let f = self.fly_fields.get_or_insert((None, None));
        f.0 = Some(*destination);
    }
    fn fly_set_followed(&mut self, _helper: Obj, failed: bool, state: f64, pending: f64) {
        let f = self.fly_fields.get_or_insert((None, None));
        f.1 = Some((failed, state, pending));
    }
}

// ---------------------------------------------------------------- the service's own state

fn service_of(w: &mut Rw, s: V) -> VehicleAI {
    let mut vai = VehicleAI::new();
    for r in s.get("reservations").items() {
        let c = r.get("candidate");
        let o = r.get("owner");
        vai.reservations.records.push(Reservation {
            candidate: Ref { obj: w.ids.of(c.get("unit")).expect("candidate"), generation: num_or_nan(c.get("generation")) },
            owner: Ref { obj: w.ids.of(o.get("unit")).expect("owner"), generation: num_or_nan(o.get("generation")) },
            time: num_or_nan(r.get("time")),
            kind: num_or_nan(r.get("type")),
        });
    }
    vai.notifier.counter = s.get("counter").opt_num();
    let run = s.get("run");
    vai.run = AttackRun { a: run.get("A").opt_num(), b: run.get("B").opt_num() };
    let cache = s.get("supportCache");
    if !cache.is_nullish() {
        for entry in cache.items() {
            let post = w.ids.of(entry.at(0)).expect("post");
            let mut keys = Vec::new();
            for k in entry.at(1).items() {
                let hubs = k
                    .at(1)
                    .items()
                    .map(|h| bfe_sim::ai::vehicles::support::SupportHub { hub: num_or_nan(h.get("hub")) as usize, distance: num_or_nan(h.get("distance")) })
                    .collect();
                keys.push((k.at(0).str().unwrap_or("").to_string(), hubs));
            }
            vai.support_cache.push((post, keys));
        }
    }
    vai
}

/// The service's padLeft, from the hulls' and turrets' views.
fn pads_of(w: &mut Rw, s: V, vai: &mut VehicleAI) {
    for h in s.get("hulls").items().chain(s.get("turrets").items()) {
        if let Some(t) = h.get("padLeft").opt_num() {
            let o = w.ids.of(h.get("self")).expect("body");
            vai.pad_left.push((o, t));
        }
    }
}

fn compare_service(w: &mut Rw, vai: &VehicleAI, s: V) {
    let records: Vec<V> = s.get("reservations").items().collect();
    if records.len() != vai.reservations.records.len() {
        w.fail(format!("reservations: the capture has {}, the port {}", records.len(), vai.reservations.records.len()));
        return;
    }
    for (i, (r, mine)) in records.iter().zip(vai.reservations.records.clone()).enumerate() {
        let c = r.get("candidate");
        let o = r.get("owner");
        w.check_obj(&format!("reservation {i} candidate"), c.get("unit"), Some(mine.candidate.obj));
        w.check_num(&format!("reservation {i} candidate generation"), c.get("generation"), mine.candidate.generation);
        w.check_obj(&format!("reservation {i} owner"), o.get("unit"), Some(mine.owner.obj));
        w.check_num(&format!("reservation {i} owner generation"), o.get("generation"), mine.owner.generation);
        w.check_num(&format!("reservation {i} time"), r.get("time"), mine.time);
        w.check_num(&format!("reservation {i} type"), r.get("type"), mine.kind);
    }
    let counter = s.get("counter");
    if counter.opt_num().map(f64::to_bits) != vai.notifier.counter.map(f64::to_bits) {
        w.fail(format!("notifier counter: the capture has {}, the port {:?}", counter.show(), vai.notifier.counter));
    }
    let run = s.get("run");
    if run.get("A").opt_num().map(f64::to_bits) != vai.run.a.map(f64::to_bits) || run.get("B").opt_num().map(f64::to_bits) != vai.run.b.map(f64::to_bits) {
        w.fail(format!("attack run: the capture has {}, the port {:?}", run.show(), vai.run));
    }
    let cache = s.get("supportCache");
    if !cache.is_nullish() {
        let theirs: usize = cache.items().map(|e| e.at(1).len()).sum();
        let mine: usize = vai.support_cache.iter().map(|(_, k)| k.len()).sum();
        if theirs != mine {
            w.fail(format!("support cache: the capture has {theirs} entries, the port {mine}"));
        }
        for entry in cache.items() {
            let post = w.ids.of(entry.at(0)).expect("post");
            for k in entry.at(1).items() {
                let key = k.at(0).str().unwrap_or("");
                let hubs: Vec<(f64, f64)> = k.at(1).items().map(|h| (num_or_nan(h.get("hub")), num_or_nan(h.get("distance")))).collect();
                let found = vai.support_cache.iter().find(|(p, _)| *p == post).and_then(|(_, ks)| ks.iter().find(|(kk, _)| kk == key));
                match found {
                    None => w.fail(format!("support cache: the port has no {key}")),
                    Some((_, list)) => {
                        let mine: Vec<(f64, f64)> = list.iter().map(|h| (h.hub as f64, h.distance)).collect();
                        if mine.len() != hubs.len() || mine.iter().zip(&hubs).any(|(a, b)| !same(a.0, b.0) || !same(a.1, b.1)) {
                            w.fail(format!("support cache {key}: the capture has {hubs:?}, the port {mine:?}"));
                        }
                    }
                }
            }
        }
    }
}

/// Everything the port writes, against the frame's end.
fn compare_exit(w: &mut Rw, vai: &VehicleAI, after: V) {
    if w.error.is_some() {
        return;
    }
    compare_service(w, vai, after.get("service"));
    // The hulls' decay and pads (the hulls the frame took in).
    for h in after.get("hulls").items() {
        let o = w.ids.of(h.get("self")).expect("hull");
        let d = h.get("decay");
        let theirs = if d.is_nullish() { None } else { Some(Decay { idle: num_or_nan(d.get("idle")), sample: num_or_nan(d.get("sample")) }) };
        let mine = w.decay.get(&o).copied().flatten();
        let ok = match (theirs, mine) {
            (None, None) => true,
            (Some(a), Some(b)) => same(a.idle, b.idle) && same(a.sample, b.sample),
            _ => false,
        };
        if !ok {
            w.fail(format!("decay of {}: the capture has {theirs:?}, the port {mine:?}", w.ids.name(o)));
        }
        let pad = h.get("padLeft").opt_num();
        let mine = vai.pad_left.iter().find(|(b, _)| *b == o).map(|(_, t)| *t);
        if pad.map(f64::to_bits) != mine.map(f64::to_bits) {
            w.fail(format!("padLeft of {}: the capture has {pad:?}, the port {mine:?}", w.ids.name(o)));
        }
    }
    // The controls' steering bags and intents.
    for c in after.get("controls").items() {
        let o = w.ids.of(c.get("self")).expect("control");
        // Only the controls the frame had in view.
        if !w.units.contains_key(&o) {
            continue;
        }
        let theirs = steer_of(&mut w.ids, c.get("steer"));
        let mine = w.steer.get(&o).copied().flatten();
        if let Some(d) = steer_diff(&w.ids, &mine, &theirs) {
            w.fail(format!("{}: {d}", w.ids.name(o)));
        }
        let i = c.get("intent");
        if let Some(mine) = w.intents.get(&o).copied().flatten() {
            let theirs = intent_of(i).unwrap_or_default();
            if !(same(mine.up, theirs.up) && same(mine.down, theirs.down) && mine.brake == theirs.brake && mine.trick == theirs.trick && mine.immelmann == theirs.immelmann && mine.boost == theirs.boost) {
                w.fail(format!("{} intent: the capture has {theirs:?}, the port {mine:?}", w.ids.name(o)));
            }
        }
        if let Some(stick) = w.intent_analogs.get(&o).copied() {
            for (k, mine) in [("forward", stick.forward), ("right", stick.right), ("turn", stick.turn), ("pitch", stick.pitch)] {
                let v = i.get(k);
                if !v.opt_num().is_some_and(|x| same(x, mine)) {
                    w.fail(format!("{} intent.{k}: the capture has {}, the port {}", w.ids.name(o), v.show(), show_number(mine)));
                }
            }
        }
    }
    // The controllers' fields it writes.
    for c in after.get("controllers").items() {
        let o = w.ids.of(c.get("self")).expect("controller");
        let theirs = CtlFields::of(c);
        let Some(mut mine) = w.ctl_fields.get(&o).cloned() else { continue };
        if let Some(t) = w.reboard_at.get(&o) {
            mine.reboard_at = *t;
        }
        if w.fire_a.contains(&o) {
            mine.fire_a = true;
        }
        // The request's validity is navigation's to write too (a helper's
        // stop writes the request a controller shares with its control):
        // compared only where the port wrote it.
        if w.request_invalid.contains(&o) {
            mine.request_valid = Some(false);
        } else {
            mine.request_valid = theirs.request_valid;
        }
        if !mine.same(&theirs) {
            w.fail(format!("controller {}: the capture has {theirs:?}, the port {mine:?}", w.ids.name(o)));
        }
    }
    for ch in after.get("characters").items() {
        let o = w.ids.of(ch.get("self")).expect("character");
        if let Some(post) = w.objectives.get(&o).copied() {
            w.check_obj("objective", ch.get("objective"), Some(post));
        }
    }
    if let Some((expires, pending)) = w.patrol {
        let p = after.get("patrol");
        if let Some(t) = expires {
            w.check_num("patrol expires", p.get("expires"), t);
        }
        if let Some(s) = pending {
            w.check_num("patrol pending", p.get("pending"), s);
        }
    }
    let agent = after.get("agent");
    for (a, holds) in w.support_holds.clone() {
        if Some(a) == w.agent {
            w.check_num("supportHolds", agent.get("supportHolds"), holds);
        }
    }
    if let Some((destination, followed)) = w.fly_fields {
        let h = if agent.is_nullish() { after.get("helper") } else { agent.get("helper") };
        if let Some(d) = destination {
            w.check_vec("helper destination", h.get("destination"), &d);
        }
        if let Some((failed, state, pending)) = followed {
            w.check_bool("helper failed", h.get("failed"), failed);
            w.check_num("helper state", h.get("state"), state);
            w.check_num("helper pending", h.get("pending"), pending);
        }
    }
}

fn patterns_view(w: &mut Rw, p: V) -> PatternsView {
    let agent = p.get("agent");
    let tracker = agent.get("tracker");
    let helper = agent.get("helper");
    let control = {
        let c = w.ids.of(p.get("controller")).expect("patterns' controller");
        w.controllers.get(&c).and_then(|c| c.unit).expect("the patterns' control")
    };
    PatternsView {
        obj: w.ids.of(p.get("self")).expect("patterns"),
        agent: w.ids.of(agent.get("self")).expect("patterns' agent"),
        controller: w.ids.of(p.get("controller")).expect("patterns' controller"),
        control,
        tracker_point: if tracker.is_nullish() { None } else { vec3_of(tracker.get("point")) },
        fresh_target: if tracker.is_nullish() { None } else { w.ids.of(tracker.get("freshTarget")) },
        target: if tracker.is_nullish() { None } else { w.ids.of(tracker.get("target")) },
        helper: !helper.is_nullish(),
        helper_arrived: helper.get("arrived").truthy(),
    }
}

fn opt_obj(w: &mut Rw, v: V) -> Option<Obj> {
    if v.is_nullish() {
        None
    } else {
        w.ids.of(v)
    }
}

fn check_result_obj(w: &mut Rw, frame: &Frame, mine: Option<Obj>) {
    w.check_obj("result", frame.result(), mine);
}

fn check_result_bool(w: &mut Rw, frame: &Frame, mine: bool) {
    let r = frame.result();
    if r.truthy() != mine || (!r.is_nullish() && r.opt_bool().is_none() && r.opt_num().is_none()) && r.opt_bool() != Some(mine) {
        w.fail(format!("result: the capture has {}, the port {mine}", r.show()));
    }
}

fn check_result_num(w: &mut Rw, frame: &Frame, mine: f64) {
    w.check_num("result", frame.result(), mine);
}

fn hint_of(w: &mut Rw, h: V) -> Option<HintView> {
    if h.is_nullish() {
        return None;
    }
    let name = h.get("name").str().unwrap_or("?").to_string();
    let obj = w.ids.named(format!("hint:{name}"));
    let post = h.get("commandPost");
    Some(HintView {
        obj,
        position: vec3_of(h.get("position")).unwrap_or_default(),
        forward: vec3_of(h.get("forward")).unwrap_or_default(),
        up: vec3_of(h.get("up")),
        command_post: if post.is_nullish() { None } else { w.ids.of(post) },
    })
}

// ---------------------------------------------------------------- the frames

/// The child analogs frames of seatInput frames, by their parent's id.
type Children = HashMap<u32, Frame>;

fn replay(capture: &Capture, frame: &Frame, nav: Option<&mut Nav>, children: &Children) -> Outcome {
    let name = frame.name.strip_prefix("vehicleAI.").unwrap_or(&frame.name).trim_start_matches("frame:");
    // seatInput's views are its analogs child's (after the executor's step).
    let child = if name == "seatInput" { children.get(&frame.id) } else { None };
    if name == "seatInput" && child.is_none() {
        return Outcome::Skipped("a seatInput frame without its analogs frame".into());
    }
    let state = child.map(|c| c.before()).unwrap_or(frame.before());
    let mut w = Rw::new(capture, frame, state, nav);
    if let Some(c) = child {
        // The control's request (the child's argument, a handle).
        w.request_handle = Ids::key(c.arg(1));
    } else if name == "analogs" {
        w.request_handle = Ids::key(frame.arg(1));
    }
    let mut vai = service_of(&mut w, state.get("service"));
    pads_of(&mut w, state, &mut vai);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&mut w, &mut vai, frame, name)));
    if let Err(p) = outcome {
        let what = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
        return Outcome::Failed(w.error.clone().unwrap_or_else(|| format!("{}: panicked: {what}", frame.place())));
    }
    if w.error.is_none() {
        if let Err(e) = w.log.finish() {
            w.error = Some(e.0);
        }
    }
    compare_exit(&mut w, &vai, frame.after());
    match w.error {
        None => Outcome::Same,
        Some(e) => Outcome::Failed(e),
    }
}

fn run(w: &mut Rw, vai: &mut VehicleAI, frame: &Frame, name: &str) {
    let a = |i: usize| frame.arg(i);
    match name {
        "analogs" => {
            let unit = w.ids.of(a(0)).expect("unit");
            let request = w.request;
            let aim = vec3_of(a(2));
            let options = a(5);
            let controller = opt_obj(w, options.get("controller"));
            let out = vai.analogs(w, unit, request.as_ref(), aim.as_ref(), num_or_nan(a(3)), AnalogOptions { controller, stepped: options.get("stepped").truthy() });
            let o = frame.result();
            for (k, mine) in [("forward", out.forward), ("right", out.right), ("turn", out.turn), ("pitch", out.pitch)] {
                w.check_num(&format!("analogs.{k}"), o.get(k), mine);
            }
        }
        "seatInput" => {
            let unit = w.ids.of(a(1)).expect("unit");
            let aim = vec3_of(a(4));
            vai.seat_input(w, unit, num_or_nan(a(3)), aim.as_ref());
        }
        "updateDecay" => vai.update_decay(w, num_or_nan(a(0))),
        "notify" => {
            let told = vai.notify(w);
            check_result_num(w, frame, told);
        }
        "dispatch" => {
            let controller = w.ids.of(a(0)).expect("controller");
            let unit = w.ids.of(a(1)).expect("unit");
            let target = w.ids.of(a(2)).expect("target");
            let out = vai.dispatch(w, controller, unit, target, a(3).truthy());
            let o = frame.result();
            w.check_num("dispatch.pattern", o.get("pattern"), out.pattern);
            w.check_num("dispatch.probability", o.get("probability"), out.probability);
            w.check_bool("dispatch.aux", o.get("aux"), out.aux);
            w.check_num("dispatch.action", o.get("action"), out.action);
            w.check_bool("dispatch.inBand", o.get("inBand"), out.in_band);
            let weapon = o.get("weapon");
            w.check_obj("dispatch.weapon", weapon, out.weapon);
        }
        "enterPattern" | "tickPattern" | "exitPattern" | "sidestep" => {
            let p = frame.before().get("patterns");
            let view = patterns_view(w, p);
            let mut st = pattern_state_of(p.get("vehicle"));
            match name {
                "enterPattern" => vai.enter_pattern(w, &view, &mut st, num_or_nan(a(1))),
                "tickPattern" => vai.tick_pattern(w, &view, &mut st, num_or_nan(a(1))),
                "exitPattern" => vai.exit_pattern(w, &view, num_or_nan(a(1))),
                _ => {
                    let moved = vai.sidestep(w, &view);
                    check_result_bool(w, frame, moved);
                }
            }
            let theirs = pattern_state_of(frame.after().get("patterns").get("vehicle"));
            let same_state = match (&st, &theirs) {
                (None, None) => true,
                (Some(x), Some(y)) => {
                    x.toggle == y.toggle
                        && same_vec(&x.entry, &y.entry)
                        && same_vec(&x.destination, &y.destination)
                        && same(x.best, y.best)
                        && same(x.progress_at, y.progress_at)
                        && x.fallback == y.fallback
                        && x.loop_ == y.loop_
                        && x.inverted == y.inverted
                        && x.flip_a == y.flip_a
                        && x.flip_b == y.flip_b
                }
                _ => false,
            };
            if !same_state {
                w.fail(format!("patterns.vehicle: the capture has {theirs:?}, the port {st:?}"));
            }
        }
        "reserveSeat" => {
            let controller = opt_obj(w, a(0));
            let candidate = opt_obj(w, a(1));
            let ok = vai.reserve_seat(w, controller, candidate);
            check_result_bool(w, frame, ok);
        }
        "holdReservation" => {
            let controller = opt_obj(w, a(0));
            let candidate = w.ids.of(a(1)).expect("candidate");
            let ok = vai.hold_reservation(w, controller, candidate);
            check_result_bool(w, frame, ok);
        }
        "boardNow" => {
            let controller = opt_obj(w, a(0));
            let candidate = w.ids.of(a(1)).expect("candidate");
            let ok = vai.board_now(w, controller, candidate);
            check_result_bool(w, frame, ok);
        }
        "boardPathClear" => {
            let controller = opt_obj(w, a(0));
            let candidate = w.ids.of(a(1)).expect("candidate");
            let ok = vai.board_path_clear(w, controller, candidate);
            check_result_bool(w, frame, ok);
        }
        "patrolMounted" => {
            let p = frame.before().get("patrol");
            let agent = p.get("agent");
            let view = PatrolView {
                obj: w.ids.of(p.get("self")).expect("patrol"),
                agent: w.ids.of(agent.get("self")).expect("patrol's agent"),
                controller: w.ids.of(agent.get("controller")).expect("patrol's controller"),
                post: opt_obj(w, p.get("post")),
                support: agent.get("support").truthy(),
                helper: !agent.get("helper").is_nullish(),
            };
            if let Some(h) = agent.get("supportHolds").opt_num() {
                w.support_holds.insert(view.agent, h);
            }
            w.agent = Some(view.agent);
            let kept = vai.patrol_mounted(w, &view);
            check_result_bool(w, frame, kept);
        }
        "supportPoint" => {
            let unit = opt_obj(w, a(0));
            let post = opt_obj(w, a(1));
            let helper = opt_obj(w, a(2));
            let point = vai.support_point(w, unit, post, helper);
            match point {
                None => {
                    if !frame.result().is_nullish() {
                        w.fail(format!("supportPoint: the capture has {}, the port none", frame.result().show()));
                    }
                }
                Some(p) => w.check_vec("supportPoint", frame.result(), &p),
            }
        }
        "requestExit" => {
            let controller = opt_obj(w, a(0));
            let post = opt_obj(w, a(1));
            let ok = vai.request_exit(w, controller, post);
            check_result_bool(w, frame, ok);
        }
        "abandonPad" => {
            let controller = w.ids.of(a(0)).expect("controller");
            let n = vai.abandon_pad(w, controller);
            check_result_num(w, frame, n);
        }
        "disembarkPassengers" => {
            let controller = opt_obj(w, a(0));
            let post = opt_obj(w, a(1));
            let n = vai.disembark_passengers(w, controller, post);
            check_result_num(w, frame, n);
        }
        "destroyStuck" => {
            let controller = opt_obj(w, a(0));
            vai.destroy_stuck(w, controller);
        }
        "freeSeats" => {
            let candidate = w.ids.of(a(0)).expect("candidate");
            let kind = a(2).opt_num().unwrap_or(1.0);
            let n = vai.free_seats(w, candidate, num_or_nan(a(1)), kind);
            check_result_num(w, frame, n);
        }
        "reserve" => {
            let candidate = w.ids.of(a(0)).expect("candidate");
            let unit = w.ids.of(a(1)).expect("unit");
            let kind = a(2).opt_num().unwrap_or(1.0);
            vai.reserve(w, candidate, unit, kind);
        }
        "currentControllable" => {
            let character = opt_obj(w, a(0));
            let c = vai.current_controllable(w, character);
            check_result_obj(w, frame, c);
        }
        "boarded" => {
            let controller = opt_obj(w, a(0));
            let b = vai.boarded(w, controller);
            check_result_bool(w, frame, b);
        }
        "lowTickets" => {
            let b = vai.low_tickets(w, num_or_nan(a(0)));
            check_result_bool(w, frame, b);
        }
        "boardedVehicle" => {
            let controller = opt_obj(w, a(0));
            let e = vai.boarded_vehicle(w, controller);
            check_result_obj(w, frame, e);
        }
        "boardedVehicleOf" => {
            let unit = opt_obj(w, a(0));
            let e = vai.boarded_vehicle_of(w, unit);
            check_result_obj(w, frame, e);
        }
        "boardedVehicleOfCharacter" => {
            let character = opt_obj(w, a(0));
            let e = vai.boarded_vehicle_of_character(w, character);
            check_result_obj(w, frame, e);
        }
        "isMountedTurret" => {
            let sender = opt_obj(w, a(0));
            let b = vai.is_mounted_turret(w, sender);
            check_result_bool(w, frame, b);
        }
        "emptySeats" => {
            let sender = opt_obj(w, a(0));
            let n = vai.empty_seats(w, sender);
            check_result_num(w, frame, n);
        }
        "crewControllers" => {
            let entry = opt_obj(w, a(0));
            let source = opt_obj(w, a(1));
            let list = vai.crew_controllers(w, entry, source);
            let theirs: Vec<V> = frame.result().items().collect();
            if theirs.len() != list.len() {
                w.fail(format!("crewControllers: the capture has {}, the port {}", theirs.len(), list.len()));
            } else {
                for (t, m) in theirs.into_iter().zip(list) {
                    w.check_obj("crew controller", t, Some(m));
                }
            }
        }
        "seatsFull" => {
            let entry = w.ids.of(a(0)).expect("entry");
            let b = vai.seats_full(w, entry);
            check_result_bool(w, frame, b);
        }
        "pendingReservations" => {
            let entry = w.ids.of(a(0)).expect("entry");
            let b = vai.pending_reservations(w, entry, num_or_nan(a(1)));
            check_result_bool(w, frame, b);
        }
        "targetReserved" => {
            let target = w.ids.of(a(0)).expect("target");
            let b = vai.target_reserved(w, target, num_or_nan(a(1)));
            check_result_bool(w, frame, b);
        }
        "reserveTarget" => {
            let target = w.ids.of(a(0)).expect("target");
            let unit = w.ids.of(a(1)).expect("unit");
            vai.reserve_target(w, target, unit);
        }
        "mountReach" => {
            let unit = opt_obj(w, a(0));
            let b = vai.mount_reach(w, unit, &vec3(a(1)));
            check_result_bool(w, frame, b);
        }
        "sharedTarget" => {
            let controller = opt_obj(w, a(0));
            let t = vai.shared_target(w, controller);
            check_result_obj(w, frame, t);
        }
        "unreachableFromMount" => {
            let controller = opt_obj(w, a(0));
            let b = vai.unreachable_from_mount(w, controller, &vec3(a(1)));
            check_result_bool(w, frame, b);
        }
        "createHelper" => {
            let agent = frame.before().get("agent");
            let controller = opt_obj(w, agent.get("controller"));
            let kind = vai.create_helper(w, controller);
            let r = frame.result();
            let theirs = match r.class() {
                Some("StationaryHelper") => HelperKind::Stationary,
                Some("FlyGotoHelper") => HelperKind::Fly,
                _ if r.is_nullish() => HelperKind::None,
                _ => match r.handle().map(|(k, _)| k) {
                    Some("object:StationaryHelper") => HelperKind::Stationary,
                    Some("object:FlyGotoHelper") => HelperKind::Fly,
                    _ => HelperKind::None,
                },
            };
            if theirs != kind {
                w.fail(format!("createHelper: the capture has {}, the port {kind:?}", r.show()));
            }
        }
        "mountedState" => {
            let controller = w.ids.of(a(0)).expect("controller");
            let policy = policy_of(w, frame.before(), controller);
            let s = VehicleAI::mounted_state(policy, num_or_nan(a(1)));
            check_result_num(w, frame, s);
        }
        "agentFor" => {
            let controller = w.ids.of(a(1)).expect("controller");
            let policy = policy_of(w, frame.before(), controller);
            let mine = VehicleAI::agent_for(policy, num_or_nan(a(0)));
            let r = frame.result();
            let theirs = if r.is_nullish() { None } else { Some(MountedAgent::Fly) };
            if theirs != mine {
                w.fail(format!("agentFor: the capture has {}, the port {mine:?}", r.show()));
            }
        }
        "adaptController" => {
            let controller = w.ids.of(a(0)).expect("controller");
            let unit = opt_obj(w, a(1));
            let (policy, home) = vai.adapt_controller(w, unit);
            let mut fields = w.ctl_fields.get(&controller).cloned().expect("the controller's fields");
            fields.policy = policy.map(|p| p.name().to_string());
            if let Some(h) = home {
                fields.home_facing = Some(h);
            }
            fields.policy_adapted = true;
            w.ctl_fields.insert(controller, fields);
        }
        "canEnter" => {
            let controller = opt_obj(w, a(0));
            let candidate = w.ids.of(a(1)).expect("candidate");
            let b = vai.can_enter(w, controller, candidate);
            check_result_bool(w, frame, b);
        }
        "boardPolicy" => {
            let agent = w.ids.of(a(0)).expect("agent");
            let controller = w.ids.of(frame.before().get("agent").get("controller")).expect("agent's controller");
            let candidate = w.ids.of(a(1)).expect("candidate");
            let b = vai.board_policy(w, agent, controller, candidate);
            check_result_bool(w, frame, b);
        }
        "isAtAt" => {
            let unit = w.ids.of(a(0)).expect("unit");
            let b = vai.is_at_at(w, unit);
            check_result_bool(w, frame, b);
        }
        "holdHint" | "closeHint" => {}
        _ => {}
    }
}

fn policy_of(w: &mut Rw, before: V, controller: Obj) -> Option<Policy> {
    let _ = before;
    match w.ctl_fields.get(&controller).and_then(|c| c.policy.clone()).as_deref() {
        Some("UnitMobileController") => Some(Policy::Mobile),
        Some("UnitStationaryController") => Some(Policy::Stationary),
        Some("UnitFlyController") => Some(Policy::Fly),
        _ => None,
    }
}

/// The agents' and helpers' frames.
fn replay_agent(capture: &Capture, frame: &Frame, nav: Option<&mut Nav>) -> Outcome {
    let state = frame.before();
    let mut w = Rw::new(capture, frame, state, nav);
    let mut vai = service_of(&mut w, state.get("service"));
    pads_of(&mut w, state, &mut vai);
    let agent = state.get("agent");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_agent(&mut w, &mut vai, frame, agent)));
    if let Err(p) = outcome {
        let what = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
        return Outcome::Failed(w.error.clone().unwrap_or_else(|| format!("{}: panicked: {what}", frame.place())));
    }
    if w.error.is_none() {
        if let Err(e) = w.log.finish() {
            w.error = Some(e.0);
        }
    }
    compare_exit(&mut w, &vai, frame.after());
    match w.error {
        None => Outcome::Same,
        Some(e) => Outcome::Failed(e),
    }
}

fn run_agent(w: &mut Rw, vai: &mut VehicleAI, frame: &Frame, agent: V) {
    let (label, method) = frame.name.split_once('.').expect("label.method");
    if label == "flyHelperP14" || label == "stationaryHelper" {
        let helper = w.ids.of(frame.receiver()).expect("helper");
        let a = |i: usize| frame.arg(i);
        match (label, method) {
            ("flyHelperP14", "followPath") => {
                let points: Vec<[f64; 3]> = a(0).items().map(|p| [num_or_nan(p.at(0)), num_or_nan(p.at(1)), num_or_nan(p.at(2))]).collect();
                let ok = fly::follow_path(w, helper, &points, a(1).truthy(), a(2).truthy());
                check_result_bool(w, frame, ok);
            }
            ("flyHelperP14", "goto") => {
                let direct = a(1).get("mode").str() == Some("direct") || a(1).get("mode").str() == Some("straight");
                fly::goto(w, helper, &vec3(a(0)), direct);
            }
            ("flyHelperP14", "stop") => fly::stop(w, helper),
            ("flyHelperP14", "land") => fly::land(w, helper, if a(0).is_undefined() { true } else { a(0).truthy() }),
            ("stationaryHelper", "goto") | ("stationaryHelper", "stop") => {
                let controller = w.fly_controller.expect("the helper's controller");
                StationaryHelper::default().goto(w, controller);
            }
            _ => {}
        }
        return;
    }
    let agent_obj = w.ids.of(agent.get("self")).expect("agent");
    let cx = AgentCx { agent: agent_obj, controller: w.ids.of(agent.get("controller")).expect("agent's controller"), unit: opt_obj(w, agent.get("unit")) };
    let mut fsm = Fsm { state: num_or_nan(agent.get("state")), pending: num_or_nan(agent.get("pending")) };
    let dt = frame.arg(0).opt_num().unwrap_or(f64::NAN);
    match label {
        "boardAgent" => {
            let v = agent.get("vehicle");
            let vehicle = if v.is_nullish() { None } else { Some(Ref { obj: w.ids.of(v.get("unit")).expect("vehicle"), generation: num_or_nan(v.get("generation")) }) };
            let me = BoardAgent { vehicle };
            match method {
                "onEnter" => me.on_enter(w, vai, &mut fsm, &cx),
                "behave" => me.behave(w, vai, &mut fsm, &cx, dt),
                "contested" => {
                    let b = me.contested(w, vai, &cx);
                    check_result_bool(w, frame, b);
                }
                _ => {}
            }
        }
        "unboardAgent" => {
            let me = UnBoardAgent;
            match method {
                "onEnter" => me.on_enter(w, &fsm, &cx),
                "behave" => me.behave(w, vai, &mut fsm, &cx, dt),
                _ => {}
            }
        }
        "flyAgent" => {
            let hint = hint_of(w, agent.get("hint"));
            let mut me = FlyAgent {
                hint,
                search_at: num_or_nan(agent.get("searchAt")),
                flag: agent.get("flag").truthy(),
                deadline: num_or_nan(agent.get("deadline")),
                post: opt_obj(w, agent.get("post")),
            };
            match method {
                "onEnter" => me.on_enter(w, vai, &mut fsm, &cx),
                "behave" => me.behave(w, vai, &mut fsm, &cx, dt),
                "onExit" => me.on_exit(w, &fsm, &cx),
                "acceptsTarget" => {
                    let target = w.ids.of(frame.arg(0)).expect("target");
                    let b = me.accepts_target(w, &fsm, &cx, target);
                    check_result_bool(w, frame, b);
                }
                _ => {}
            }
            let after = frame.after().get("agent");
            w.check_num("flyAgent.searchAt", after.get("searchAt"), me.search_at);
            w.check_bool("flyAgent.flag", after.get("flag"), me.flag);
            w.check_num("flyAgent.deadline", after.get("deadline"), me.deadline);
            let post = after.get("post");
            w.check_obj("flyAgent.post", post, me.post);
            let hint = after.get("hint");
            if hint.is_nullish() != me.hint.is_none() {
                w.fail(format!("flyAgent.hint: the capture has {}, the port {:?}", hint.show(), me.hint));
            }
        }
        _ => {}
    }
    let after = frame.after().get("agent");
    w.check_num("agent.pending", after.get("pending"), fsm.pending);
}

// ---------------------------------------------------------------- the test

const SERVICE_FRAMES: &[&str] = &[
    "vehicleAI.analogs", "vehicleAI.seatInput", "vehicleAI.updateDecay", "vehicleAI.notify", "vehicleAI.frame:dispatch",
    "vehicleAI.enterPattern", "vehicleAI.tickPattern", "vehicleAI.exitPattern", "vehicleAI.sidestep", "vehicleAI.reserveSeat",
    "vehicleAI.holdReservation", "vehicleAI.boardNow", "vehicleAI.boardPathClear", "vehicleAI.patrolMounted", "vehicleAI.supportPoint",
    "vehicleAI.requestExit", "vehicleAI.abandonPad", "vehicleAI.disembarkPassengers", "vehicleAI.destroyStuck", "vehicleAI.freeSeats",
    "vehicleAI.reserve", "vehicleAI.frame:currentControllable", "vehicleAI.frame:boarded", "vehicleAI.frame:lowTickets",
    "vehicleAI.frame:boardedVehicle", "vehicleAI.frame:boardedVehicleOf", "vehicleAI.frame:boardedVehicleOfCharacter",
    "vehicleAI.frame:isMountedTurret", "vehicleAI.frame:emptySeats", "vehicleAI.frame:crewControllers", "vehicleAI.frame:seatsFull",
    "vehicleAI.frame:pendingReservations", "vehicleAI.frame:targetReserved", "vehicleAI.frame:reserveTarget", "vehicleAI.frame:mountReach",
    "vehicleAI.frame:sharedTarget", "vehicleAI.frame:unreachableFromMount", "vehicleAI.frame:createHelper", "vehicleAI.frame:mountedState",
    "vehicleAI.frame:agentFor", "vehicleAI.frame:adaptController", "vehicleAI.frame:canEnter", "vehicleAI.frame:boardPolicy",
    "vehicleAI.frame:isAtAt",
];
const AGENT_FRAMES: &[&str] = &[
    "boardAgent.onEnter", "boardAgent.behave", "boardAgent.contested", "unboardAgent.onEnter", "unboardAgent.behave",
    "flyAgent.onEnter", "flyAgent.behave", "flyAgent.onExit", "flyAgent.acceptsTarget", "flyHelperP14.followPath",
    "flyHelperP14.goto", "flyHelperP14.stop", "flyHelperP14.land", "stationaryHelper.goto", "stationaryHelper.stop",
];

/// The map's navigation, as the battle loaded it (ai-nav.json + .bin).
fn load_nav(capture: &Capture) -> Option<Nav> {
    let dir = std::env::var_os("BFE_CONTENT_DIR")?;
    let map = capture.battle().get("map").str()?.to_string();
    let base = std::path::Path::new(&dir).join("maps").join(&map);
    let json = json::parse_bytes(&std::fs::read(base.join("ai-nav.json")).ok()?).ok()?;
    let bin = json.get("bin").and_then(|b| b.get("file")).and_then(|f| f.as_str()).and_then(|f| std::fs::read(base.join(f)).ok());
    Nav::new(&json, bin.as_deref(), None, None, PlannerOptions::default()).ok()
}

fn replay_all(only: Option<&str>) -> Option<Vec<Report>> {
    let corpus = Corpus::from_env().expect("corpus")?;
    let mut reports: Vec<Report> = SERVICE_FRAMES.iter().chain(AGENT_FRAMES).filter(|n| only.is_none_or(|o| n.contains(o))).map(|n| Report::new(n)).collect();
    for capture in corpus.captures() {
        let capture = capture.expect("capture");
        let mut nav = load_nav(&capture);
        // seatInput's analogs children, by parent.
        let mut children: Children = HashMap::new();
        if reports.iter().any(|r| r.name == "vehicleAI.seatInput") {
            for f in capture.frames("vehicleAI.analogs") {
                let f = f.expect("frame");
                if let Some(p) = f.parent {
                    children.insert(p, f);
                }
            }
        }
        for report in reports.iter_mut() {
            report.captures += 1;
            let agentish = AGENT_FRAMES.contains(&report.name.as_str());
            for frame in capture.frames(&report.name.clone()) {
                let frame = frame.expect("frame");
                let outcome = if agentish { replay_agent(&capture, &frame, nav.as_mut()) } else { replay(&capture, &frame, nav.as_mut(), &children) };
                report.add(&capture, &frame, outcome);
            }
        }
    }
    Some(reports)
}

#[test]
fn vehicle_ai_as_captured() {
    // A frame whose views lack something the port reads panics; its replay
    // fails with the message, and the run goes on quietly.
    std::panic::set_hook(Box::new(|_| {}));
    let only = std::env::var("BFE_REPLAY_ONLY").ok();
    let Some(reports) = replay_all(only.as_deref()) else { return };
    let mut failed = false;
    for r in &reports {
        if r.replayed() + r.skipped.values().sum::<usize>() == 0 {
            continue;
        }
        println!("{}", r.summary());
        failed |= !r.failures.is_empty();
    }
    let total: usize = reports.iter().map(|r| r.same).sum();
    let bad: usize = reports.iter().map(|r| r.failures.len()).sum();
    println!("vehicle AI: {total} frames the same, {bad} different");
    assert!(!failed, "frames differ");
}
