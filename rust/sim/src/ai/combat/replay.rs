//! Runs the AI combat services against captured foreign calls. Each frame
//! starts from its own entry state; native state is compared bit for bit.

use super::agent::{CombatAgent, Cover, Goal, Patterns, Phase, Reservation};
use super::aim::{AimCurves, Aimer};
use super::binding::{CombatAi, Event as CombatEvent, EventRef};
use super::enhanced::{profile, EnhancedAimer, EnhancedCombat};
use super::perception::{DangerState, DeferredLos, LosRequest, Perception, PriorityHeap, Response, TargetTracker};
use super::weapons_ai::{Burst, Button, ClusterState, FireControl, Registered};
use super::world::*;
use crate::combat::defs::{Js, JsObj};
use sim_replay::{Call, Capture, Frame, Log};
use sim_replay::value::{Node, V, Value};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use three::Vector3;

#[path = "replay_values.rs"]
mod values;
use values::*;
#[path = "replay_exact.rs"]
mod exact;
use exact::Exact;
#[path = "replay_world.rs"]
mod host;

#[derive(Clone, Debug, Default)]
struct HelperView {
    obj: Obj,
    arrived: bool,
    flying: bool,
    follow: bool,
    terminate: bool,
}

#[derive(Clone, Debug)]
struct AgentView {
    obj: Obj,
    patterns_obj: Option<Obj>,
    controller: Obj,
    tracker: Option<TargetTracker>,
    helper: Option<HelperView>,
    combat: Option<CombatAgent>,
}

#[derive(Clone, Debug, Default)]
struct UnitWrites {
    buttons: Option<[f64; 2]>,
    aim: Option<Vector3>,
    random: Vec<bool>,
    weapons: Vec<Obj>,
}

struct Host<'a> {
    log: Log<'a>,
    read: Read<'a>,
    time: f64,
    difficulty: DifficultyView,
    bound: V<'a>,
    teams: V<'a>,
    units: HashMap<Obj, UnitView>,
    writes: HashMap<Obj, UnitWrites>,
    arsenal_known: HashSet<Obj>,
    ctls: HashMap<Obj, Ctl>,
    agents: HashMap<Obj, AgentView>,
    characters: HashMap<Obj, (Option<Obj>, Option<Obj>, f64)>,
    living: Vec<Obj>,
}

fn equal<T: Exact + std::fmt::Debug>(label: &str, got: &T, want: &T) {
    if let Some(d) = got.difference(want) { panic!("{label}.{d}"); }
}

impl<'a> Host<'a> {
    fn new(capture: &'a Capture, frame: &'a Frame) -> Self {
        let s = frame.before();
        assert_eq!(s.get("contract").opt_num(), Some(2.0), "AI combat capture needs complete entry caches");
        let mut h = Self {
            log: Log::new(frame), read: Read::new(capture), time: s.get("time").num(),
            difficulty: difficulty(s.get("difficulty")), bound: s.get("bound"), teams: s.get("teams"),
            units: HashMap::new(), writes: HashMap::new(), arsenal_known: HashSet::new(), ctls: HashMap::new(), agents: HashMap::new(),
            characters: HashMap::new(), living: Vec::new(),
        };
        h.read.tier = s.get("enhanced").get("tier").opt_num().unwrap_or(2.0);
        for v in s.get("units").items() { h.add_unit(v); }
        for v in s.get("ctls").items() { let c = h.read.ctl(v); h.ctls.insert(c.obj, c); }
        for v in s.get("agents").items() { let a = h.read.agent(v); h.agents.insert(a.obj, a); }
        for v in s.get("characters").items() {
            let id = h.read.obj(v.get("self"));
            let u = h.read.opt_obj(v.get("unit"));
            let r = h.read.opt_obj(v.get("remote"));
            h.characters.insert(id, (u, r, v.get("team").num()));
        }
        for v in s.get("living").items() { let id = h.add_unit(v); h.living.push(id); }
        h
    }

    fn add_unit(&mut self, v: V) -> Obj {
        let u = self.read.unit(v);
        let id = u.obj;
        let has_arsenal_view = v.get("arsenal").str() != Some("omitted");
        // A queried recipient can first appear only as another bot's
        // target. Its arsenal was not readable before this query.
        if self.units.contains_key(&id) && has_arsenal_view && !self.arsenal_known.contains(&id) {
            self.units.get_mut(&id).unwrap().arsenal = u.arsenal.clone();
            let view = self.read.unit_writes(v);
            let writes = self.writes.get_mut(&id).unwrap();
            writes.random = view.random; writes.weapons = view.weapons;
        }
        if let Some(old) = self.units.get(&id) {
            let mut expected = u;
            if v.get("arsenal").str() == Some("omitted") { expected.arsenal = old.arsenal.clone(); }
            equal("repeated unit view", old, &expected);
        } else {
            self.units.insert(id, u);
            let writes = self.read.unit_writes(v);
            self.writes.insert(id, writes);
        }
        if has_arsenal_view { self.arsenal_known.insert(id); }
        id
    }

    fn call(&mut self, name: &str) -> &'a Call { self.log.call(name).unwrap_or_else(|e| panic!("{e}")) }
    fn check_obj(&mut self, v: V, actual: Option<Obj>) { equal("object argument", &actual, &self.read.opt_obj(v)); }
    fn num(&self, v: V, actual: f64) { sim_replay::check_num("number argument", v, actual).unwrap(); }
    fn boolean(&self, v: V, actual: bool) { sim_replay::check_bool("boolean argument", v, actual).unwrap(); }
    fn vector(&self, v: V, actual: &Vector3) { equal("vector argument", actual, &vec(v)); }

    fn check_state(&mut self, cx: &CombatAi, s: V) {
        self.num(s.get("time"), self.time);
        let hooked = cx.enhanced.as_ref().map_or(cx.aim_hooked, |e| e.hooked);
        self.boolean(s.get("aimHooked"), hooked);
        for v in s.get("ctls").items() {
            let want = self.read.ctl(v);
            equal("controller", self.ctls.get(&want.obj).expect("captured controller is present"), &want);
        }
        for v in s.get("agents").items() {
            let want = self.read.agent(v);
            equal("agent", self.agents.get(&want.obj).expect("captured agent is present"), &want);
        }
        for v in s.get("units").items() {
            let mut want = self.read.unit(v);
            let got = self.units.get(&want.obj).expect("captured unit is present");
            if v.get("arsenal").str() == Some("omitted") { want.arsenal = got.arsenal.clone(); }
            equal("unit", got, &want);
            let writes = self.read.unit_writes(v);
            let actual = self.writes.get(&want.obj).unwrap();
            equal("unit buttons", &actual.buttons, &writes.buttons);
            equal("unit aim", &actual.aim, &writes.aim);
            if v.get("arsenal").str() != Some("omitted") { equal("weapon random generator", &actual.random, &writes.random); }
        }
        if !s.get("los").is_undefined() { equal("deferred LOS", &cx.los, &self.read.los(s.get("los"))); }
        assert_eq!(cx.shared.clusters.states.len(), s.get("clusters").items().count(), "cluster membership");
        for v in s.get("clusters").items() {
            let key = self.read.obj(v.get("key"));
            let want = cluster(v);
            equal("cluster", cx.shared.clusters.states.get(&key).expect("captured cluster is present"), &want);
        }
        assert_eq!(cx.reservations.by_hint.len(), s.get("reservations").items().count(), "reservation membership");
        for v in s.get("reservations").items() {
            let key = Read::hint_obj(v.get("index").num());
            let want = Reservation { owner: self.read.opt_obj(v.get("owner")), at: v.get("at").num() };
            equal("reservation", cx.reservations.by_hint.get(&key).expect("captured reservation is present"), &want);
        }
        if let Some(e) = &cx.enhanced {
            if !s.get("skill").is_undefined() {
                let c = self.read.opt_obj(self.log.frame.arg(0)).and_then(|c| self.ctls.get(&c)).and_then(|c| c.character);
                let got = c.and_then(|c| e.skills.get(&c).copied());
                equal("enhanced skill", &got, &s.get("skill").opt_num());
            }
        }
    }
}

/// Replay one service or combat-agent frame. A capture supplies content
/// definitions as well as the frame; no external content directory is read.
pub fn replay_frame(capture: &Capture, frame: &Frame) -> Result<(), String> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(capture, frame)));
    match result {
        Ok(()) => Ok(()),
        Err(panic) => {
            let message = panic.downcast_ref::<String>().map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied()).unwrap_or("unknown panic");
            Err(format!("{}: {message}", frame.place()))
        }
    }
}

fn run(capture: &Capture, f: &Frame) {
    let mut h = Host::new(capture, f);
    let before = f.before();
    let mut cx = CombatAi::new(if before.get("enhanced").is_nullish() { None } else { Some(EnhancedCombat::new(before.get("enhanced").get("tier").num())) });
    cx.aim_hooked = before.get("aimHooked").bool();
    if let Some(e) = &mut cx.enhanced {
        e.hooked = cx.aim_hooked;
        if let Some(skill) = before.get("skill").opt_num() {
            let c = h.read.obj(f.arg(0));
            if let Some(character) = h.ctl(c).character { e.skills.insert(character, skill); }
        }
    }
    if !before.get("los").is_nullish() { cx.los = h.read.los(before.get("los")); }
    for v in before.get("clusters").items() {
        let key = h.read.obj(v.get("key"));
        cx.shared.clusters.states.insert(key, cluster(v));
    }
    for data in before.get("weaponData").items() {
        let data = h.read.js(data);
        let Js::Obj(data) = data else { panic!("AI weapon records are an object") };
        let Js::Obj(data) = data.get("weapons") else { panic!("AI weapon map is an object") };
        for (name, rec) in &data.fields {
            cx.shared.weapons.register(&[(name.to_string(), Registered { ai: rec.get("ai").clone(), levels: match rec.get("levels") {
                Js::Arr(levels) => levels.iter().map(|l| (l.get("at").clone(), l.get("ai").clone())).collect(), _ => Vec::new(),
            } })]);
        }
    }
    for v in before.get("reservations").items() {
        let key = Read::hint_obj(v.get("index").num());
        cx.reservations.by_hint.insert(key, Reservation { owner: h.read.opt_obj(v.get("owner")), at: v.get("at").num() });
    }
    let a = h.read.opt_obj(f.arg(0));
    let b = h.read.opt_obj(f.arg(1));
    let r = f.result();
    if matches!(f.name.as_str(), "aiCombat.attachController" | "aiCombat.detachController" | "aiCombat.rebuildProtected" | "aiCombat.dropLosRequests" | "aiCombat.clearCandidate" | "aiCombat.updateTarget" | "aiCombat.updateTargetSelection" | "aiCombat.gateFire" | "aiCombat.consumeFire" | "aiCombat.ingest" | "UnitCombatAgent.exit") {
        assert!(r.is_undefined(), "void method returned {}", r.show());
    }
    match f.name.as_str() {
        "aiCombat.createPerception" => {
            let c = a.unwrap(); let old = h.ctl(c).perception.clone();
            cx.create_perception(&mut h, c);
            let want = h.read.perception(f.after().get("createdPerception"), h.read.tier);
            equal("created perception", &h.ctl(c).perception, &want);
            h.ctl_mut(c).perception = old;
        }
        "aiCombat.attachController" => cx.attach_controller(&mut h, a.unwrap()),
        "aiCombat.detachController" => cx.detach_controller(&mut h, a.unwrap()),
        "aiCombat.rebuildProtected" => cx.rebuild_protected(&mut h),
        "aiCombat.scan" => { let x = cx.scan(&mut h, a.unwrap()); h.num(r, x); }
        "aiCombat.dropLosRequests" => cx.drop_los_requests(&mut h, a.unwrap()),
        "aiCombat.serviceLos" => { let x = cx.service_los(&mut h, f.arg(1).num()); h.boolean(r, x); }
        "aiCombat.broadcastAcoustic" => { let x = cx.broadcast_acoustic(&mut h, a.unwrap()); h.num(r, x); }
        "aiCombat.clearCandidate" => cx.clear_candidate(&mut h, a.unwrap()),
        "aiCombat.rankTargets" => { let x = cx.rank_targets(&mut h, a.unwrap()); h.num(r, x); }
        "aiCombat.updateTarget" => cx.update_target(&mut h, a.unwrap()),
        "aiCombat.updateTargetSelection" => cx.update_target_selection(&mut h, a.unwrap()),
        "aiCombat.gateFire" => cx.gate_fire(&mut h, a.unwrap()),
        "aiCombat.currentTarget" => { let x = cx.current_target(&mut h, a.unwrap()); h.check_obj(r, x); }
        "aiCombat.memoryPoint" => { let x = cx.memory_point(&mut h, a.unwrap()); equal("return", &x, &opt_vec(r)); }
        "aiCombat.acquiredTarget" => { let x = cx.acquired_target(&mut h, a.unwrap()); h.check_obj(r, x); }
        "aiCombat.canEngage" => { let x = cx.can_engage(&mut h, a.unwrap(), b); h.boolean(r, x); }
        "aiCombat.hasWeapon" => { let x = cx.has_weapon(&h, a.unwrap()); h.boolean(r, x); }
        "aiCombat.sensorRange" => { let x = cx.sensor_range(&mut h, a.unwrap(), b.unwrap()); h.num(r, x); }
        "aiCombat.acousticRange" => { let x = cx.acoustic_range(&h, a.unwrap()); h.num(r, x); }
        "aiCombat.consumeFire" => cx.consume_fire(&mut h, a.unwrap(), b.unwrap(), f.arg(2).num(), !f.arg(3).is_nullish()),
        "aiCombat.aimPoint" => { let x = cx.aim_point(&mut h, a.unwrap(), b.unwrap(), f.arg(2).num()); equal("return", &x, &opt_vec(r)); }
        "aiCombat.leadPoint" => { let x = cx.lead_point(&mut h, a.unwrap(), b.unwrap(), f.arg(2).num()); h.boolean(r,x.is_some()); if let Some(x)=x { h.vector(f.after().get("leadOut"),&x); } }
        "aiCombat.aim" => { let x = cx.aim(&mut h, a.unwrap(), f.arg(1).num()); equal("return", &x, &opt_vec(r)); }
        "aiCombat.ingest" => { let evs = before.get("events").items().map(|v| h.read.event(v)).collect::<Vec<_>>(); cx.ingest(&mut h, &evs); }
        "aiCombat.grenadeUpdate" => {
            let mut s = if before.get("danger").is_nullish() { DangerState { first_contact: f64::INFINITY, last_notify: -1.0 } } else { danger(before.get("danger")) };
            let owner = h.read.opt_obj(f.arg(2));
            let x = cx.grenade_update(&mut h, &mut s, &vec(f.arg(1)), owner, f.arg(3).opt_num().unwrap_or(f64::INFINITY));
            h.num(r, x); equal("grenade danger", &s, &danger(f.after().get("danger")));
        }
        "aiCombat.hazardResponse" | "aiCombat.alertResponse" | "aiCombat.acousticResponse" => {
            let x = match f.name.as_str() {
                "aiCombat.hazardResponse" => cx.hazard_response(&mut h, a.unwrap(), b),
                "aiCombat.alertResponse" => cx.alert_response(&mut h, a.unwrap(), b),
                _ => cx.acoustic_response(&mut h, a.unwrap(), b),
            };
            equal("response", &x, &response(r));
        }
        "aiCombat.createCombatAgent" => {
            let c = a.unwrap();
            let id = h.read.obj(f.after().get("created").get("self"));
            let call = h.call("ai.createHelper"); h.check_obj(call.arg(0), Some(id));
            let helper = h.read.helper(call.extra().get("helper"));
            let g = h.read.goal(before.get("goal"));
            let agent = CombatAgent::new(&h, id, c, cx.init_agent(c), g);
            let patterns_obj = h.read.opt_obj(f.after().get("created").get("patterns").get("self"));
            h.agents.insert(id, AgentView { obj: id, patterns_obj, controller: c, tracker: None, helper, combat: Some(agent) });
            let want = h.read.agent(f.after().get("created"));
            equal("created agent", h.agents.get(&id).unwrap(), &want);
            h.check_obj(r, Some(id));
        }
        name if name.starts_with("UnitCombatAgent.") => {
            let id = h.read.obj(f.receiver());
            let mut agent = h.agents[&id].combat.clone().expect("combat agent");
            match &name[16..] {
                "tick" => { let x = agent.tick(&mut h, &mut cx, f.arg(0).num(), if f.arg(1).num() == 0.0 { Phase::Behave } else { Phase::Rank }); h.boolean(r, x); }
                "exit" => agent.exit(&mut h, &mut cx),
                "acceptsAlert" => { let x = agent.accepts_alert(&mut h, f.arg(0).bool(), &vec(f.arg(1))); h.boolean(r, x); }
                "acceptsHazard" => { let x = agent.accepts_hazard(&h); h.boolean(r, x); }
                "acceptsAcoustic" => { let x = agent.accepts_acoustic(&mut h, &mut cx, a); h.boolean(r, x); }
                "acceptsTarget" => h.boolean(r, agent.accepts_target()),
                "acceptsBoard" => { let x = agent.accepts_board(&mut h, a.unwrap()); h.boolean(r, x); }
                "acceptsRecovery" => h.boolean(r, agent.accepts_recovery()),
                "savesOnInterrupt" => h.boolean(r, agent.saves_on_interrupt()),
                _ => panic!("unknown combat agent method {name}"),
            }
            h.agent_sync(&agent);
        }
        _ => panic!("unhandled AI combat frame {}", f.name),
    }
    h.log.finish().unwrap();
    h.check_state(&cx, f.after());
}
