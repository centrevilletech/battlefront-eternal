//! The combat package's replays (`cargo test -p sim --features replay
//! --test replay_combat`, BFE_REPLAY_CORPUS a directory of captures made
//! with `tools/sim-capture.mjs --specs combat`): each frame (a soldier's
//! combat update, the rounds a droideka's guns owe, damage, a blast, a new
//! life) starts from the unit and combat the JavaScript started from, runs
//! the Rust port, serves every call across combat's boundary
//! (src/combat-world.js, the hidden gun rig, the shield's hook) and every
//! draw from the capture, checking what the port asks, and compares the
//! unit and combat it ends with, bit for bit.
//!
//! A served call that can change the unit or his combat (a round that blows
//! up beside him, a death, a link of the arc) applies the capture's unit
//! and combat as it left them, on the fields it may change: before it does,
//! every other field must already be the JavaScript's.
//!
//! Without a corpus the tests pass without replaying (CI has no game data).
#![cfg(feature = "replay")]

use bfe_sim::combat::combat::{self as cb, CombatDefs, Edge, Edges};
use bfe_sim::combat::defs::{DefTable, Js, JsObj, WeaponClass};
use bfe_sim::combat::weapons::{Admission, Shot, ShotKind, WeaponDraws};
use bfe_sim::combat::world::{ArcTarget, CastHit, CombatWorld, DamageInfo, Droid, LockPick, Presentation};
use bfe_sim::gen::{Attacker, Combat, RepairTarget, Replay, Unit, Weapon};
use bfe_sim::store::replay::{Arena, Value, V};
use bfe_sim::store::StrId;
use sim_replay::{check_num, check_vec3, diff, replay_corpus, Capture, Call, Corpus, Frame, Log, Mismatch, Outcome};
use std::collections::HashMap;
use std::rc::Rc;
use three::Vector3;

fn vec3(v: V) -> Option<Vector3> {
    v.vec3().map(|a| Vector3 { x: a[0], y: a[1], z: a[2] })
}

fn arr(v: &Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

// ---- Content: definitions from a capture's def records.

/// The battle's content as the frames name it, built once a capture.
#[derive(Default)]
struct Content {
    defs: CombatDefs,
    /// Definitions converted, by their capture key.
    objects: HashMap<String, Rc<JsObj>>,
}

impl Content {
    /// A definition (a def handle) as a JavaScript object; `table` the
    /// table a weapon class or a unit class registers in.
    fn def(&mut self, capture: &Capture, cx: &mut Replay, handle: V, table: DefTable) -> Rc<JsObj> {
        let key = handle.handle().and_then(|(_, k)| k.str().map(str::to_string)).unwrap_or_default();
        let found = self.objects.get(&key).cloned();
        let obj = match found {
            Some(o) => o,
            None => {
                let record = capture.def_of(handle).expect("a definition the capture holds");
                let mut obj = JsObj::default();
                for (k, v) in record.fields() {
                    let value = self.js(capture, cx, v, k);
                    obj.fields.push((k.into(), value));
                }
                let o = Rc::new(obj);
                self.objects.insert(key.clone(), o.clone());
                o
            }
        };
        match table {
            DefTable::Weapon => {
                let id = cx.def_in("weapon", handle, "weapon").expect("a weapon definition");
                if !self.defs.weapons.has(id) {
                    self.defs.weapons.set(id, WeaponClass::new(obj.clone()));
                }
                let mut tagged = (*obj).clone();
                tagged.def = Some((DefTable::Weapon, id));
                Rc::new(tagged)
            }
            DefTable::UnitClass => {
                let id = cx.def_in("unitClass", handle, "classDef").expect("a unit class");
                if self.defs.classes.get(id.0 as usize).map_or(true, |c| c.is_none()) {
                    self.defs.set_class(id, obj.clone());
                }
                obj
            }
            _ => obj,
        }
    }

    /// A field's value; definitions it names converted in turn (a class's
    /// weapons as weapon classes).
    fn js(&mut self, capture: &Capture, cx: &mut Replay, v: V, key: &str) -> Js {
        if let Some((kind, _)) = v.handle() {
            if kind == "def" {
                let table = if key == "weapons" { DefTable::Weapon } else { DefTable::Other };
                return Js::Obj(self.def(capture, cx, v, table));
            }
            return Js::Null;
        }
        if v.is_undefined() {
            return Js::Undefined;
        }
        if v.is_null() {
            return Js::Null;
        }
        if let Some(b) = v.opt_bool() {
            return Js::Bool(b);
        }
        if let Some(x) = v.opt_num() {
            return Js::Num(x);
        }
        if let Some(s) = v.str() {
            return Js::Str(s.into());
        }
        if v.class() == Some("Array") || v.node().is_some_and(|n| matches!(n, bfe_sim::store::replay::Node::Array(_))) {
            let items: Vec<Js> = v.items().map(|x| self.js(capture, cx, x, key)).collect();
            return Js::Arr(Rc::new(items));
        }
        let mut obj = JsObj::default();
        for (k, x) in v.fields() {
            let value = self.js(capture, cx, x, k);
            obj.fields.push((k.into(), value));
        }
        Js::Obj(Rc::new(obj))
    }
}

// ---- Values.

/// A value copied with every handle of `from` kind renamed `to` (the
/// capture writer names a building an `object:Building`; the schema's kind
/// is `building`).
fn renamed(arena: &mut Arena, v: V) -> Value {
    if let Some((kind, key)) = v.handle() {
        if kind == "object:Building" {
            let k = arena.copy(key);
            return Value::handle("building", k);
        }
        return arena.copy(v);
    }
    if let Some(bfe_sim::store::replay::Node::Object { class, .. }) = v.node() {
        if class.is_empty() {
            let fields: Vec<(String, V)> = v.fields().map(|(k, x)| (k.to_string(), x)).collect();
            let mut out = Vec::with_capacity(fields.len());
            for (k, x) in fields {
                let c = renamed(arena, x);
                out.push((k, c));
            }
            let refs: Vec<(&str, Value)> = out.iter().map(|(k, x)| (k.as_str(), x.clone())).collect();
            return arena.object("", refs);
        }
    }
    if let Some(bfe_sim::store::replay::Node::Array(_)) = v.node() {
        let items: Vec<Value> = v.items().map(|x| renamed(arena, x)).collect::<Vec<_>>();
        return arena.array(items);
    }
    arena.copy(v)
}

/// An object's value without some of its keys, copied into `arena`.
fn without(arena: &mut Arena, v: V, keys: &[&str]) -> Value {
    let fields: Vec<(String, V)> = v.fields().filter(|(k, _)| !keys.contains(k)).map(|(k, x)| (k.to_string(), x)).collect();
    let mut out = Vec::with_capacity(fields.len());
    for (k, x) in fields {
        let c = renamed(arena, x);
        out.push((k, c));
    }
    let refs: Vec<(&str, Value)> = out.iter().map(|(k, x)| (k.as_str(), x.clone())).collect();
    arena.object("", refs)
}

/// The unit's value as the store's exporter writes it.
fn unit_value(cx: &mut Replay, arena: &mut Arena, id: u32, unit: &Unit) -> Value {
    let r = bfe_sim::store::Ref::<Unit>::new(id, 0);
    *cx.store.unit.get_mut(r).expect("the unit is in the store") = unit.clone();
    Unit::value_out(r, cx, arena)
}

/// The unit's combat (its `combat` field), from the combat the replay runs.
fn sync_unit(u: &mut Unit, c: &Combat) {
    u.combat = c.clone();
}

/// Compares the port's unit and combat with captured ones (`unit`,
/// `combat` values), leaving out the keys named.
fn compare(cx: &mut Replay, id: u32, unit: &Unit, combat: &Combat, captured: V, skip_unit: &[&str], skip_combat: &[&str]) -> Result<(), String> {
    let mut arena = Arena::default();
    let mut u = unit.clone();
    sync_unit(&mut u, combat);
    let mine_unit = unit_value(cx, &mut arena, id, &u);
    let mine_combat = combat.to_value(cx, &mut arena);
    // (Its combat is compared on its own, below.)
    let mut skip_u = vec!["$id", "combat"];
    skip_u.extend_from_slice(skip_unit);
    let mut theirs = Arena::default();
    let tu = without(&mut theirs, captured.get("unit"), &skip_u);
    let tc = without(&mut theirs, captured.get("unit").get("combat"), skip_combat);
    let mut mine = Arena::default();
    let mu = without(&mut mine, arena.view(&mine_unit), &skip_u);
    let mc = without(&mut mine, arena.view(&mine_combat), skip_combat);
    if let Some(d) = diff(theirs.view(&tu), mine.view(&mu)) {
        return Err(format!("unit {d} (capture | port)"));
    }
    if let Some(d) = diff(theirs.view(&tc), mine.view(&mc)) {
        return Err(format!("combat {d} (capture | port)"));
    }
    Ok(())
}

/// A frame's unit and combat, read into the replay's store.
fn read_state(capture: &Capture, content: &mut Content, cx: &mut Replay, state: V) -> Result<(u32, Unit, Combat, Edges), String> {
    let mut arena = Arena::default();
    let uv = renamed(&mut arena, state.get("unit"));
    let r = Unit::value_in(arena.view(&uv), cx, "unit")?;
    let unit = cx.store.unit.get(r).cloned().unwrap();
    content.def(capture, cx, state.get("unit").get("classDef"), DefTable::UnitClass);
    for w in state.get("unit").get("combat").get("weapons").items() {
        content.def(capture, cx, w.get("def"), DefTable::Weapon);
    }
    let cv = renamed(&mut arena, state.get("unit").get("combat"));
    let combat = Combat::from_value(arena.view(&cv), cx, "combat")?;
    let edge = |e: V| Edge { button: e.get("button").num(), id: e.get("id").opt_num(), late: e.get("late").opt_num(), aim_yaw: e.get("aimYaw").opt_num(), aim_pitch: e.get("aimPitch").opt_num() };
    let edges = Edges {
        shots: if state.get("shots").is_nullish() { None } else { Some(state.get("shots").items().map(edge).collect()) },
        shot: if state.get("shot").is_nullish() { None } else { Some(edge(state.get("shot"))) },
    };
    Ok((r.slot, unit, combat, edges))
}

// ---- The world, from a frame's log.

struct World<'a, 'f> {
    frame: &'f Frame,
    log: Log<'f>,
    cx: &'a mut Replay,
    id: u32,
    error: Option<String>,
    /// A mutation check's nudge: the frame's first draw one ulp up.
    nudge_draw: bool,
}

/// What a mutation check changes in every frame it replays.
#[derive(Clone, Copy, PartialEq)]
enum Mutation {
    None,
    /// The tick's length one ulp longer.
    Dt,
    /// The frame's first random number another (1 - it).
    Draw,
}

fn up_one_ulp(x: f64) -> f64 {
    f64::from_bits(x.to_bits() + 1)
}

// What each call may change of the unit (P10's names) and of combat.
// A death unseats (src/boarding.js leaveKilled), which moves the unit's
// hit shapes' tick on (tickCount).
const HURT_UNIT: &[&str] = &["alive", "dead", "deadTime", "character", "controller", "soldier", "seat", "tickCount", "targetable"];
const HURT_COMBAT: &[&str] = &["health", "dead", "deathTime", "lastAttacker", "blowApart", "zoomStep", "mounted", "aimOverride"];
const DIED_UNIT: &[&str] = &["alive", "dead", "deadTime", "character", "controller", "soldier", "seat", "tickCount", "targetable"];
const DIED_COMBAT: &[&str] = &["mounted", "aimOverride"];
const REMOTE_UNIT: &[&str] = &["remotesActive"];
const BODY_UNIT: &[&str] = &["soldier"];

impl<'a, 'f> World<'a, 'f> {
    fn fail(&mut self, what: String) {
        if self.error.is_none() {
            self.error = Some(format!("{}: {what}", self.frame.place()));
        }
    }

    fn call(&mut self, name: &str) -> Option<&'f Call> {
        match self.log.call(name) {
            Ok(c) => Some(c),
            Err(e) => {
                self.fail(e.0);
                None
            }
        }
    }

    fn check(&mut self, r: Result<(), Mismatch>) {
        if let Err(e) = r {
            self.fail(e.0);
        }
    }

    /// The unit and combat as the call left them, on the fields it may
    /// change; every other field must already be the JavaScript's.
    fn after(&mut self, call: &'f Call, unit: &mut Unit, combat: &mut Combat, unit_keys: &[&str], combat_keys: &[&str]) {
        let x = call.extra();
        if x.get("skipped").truthy() || x.is_nullish() {
            return;
        }
        if let Err(e) = compare(self.cx, self.id, unit, combat, x, unit_keys, combat_keys) {
            return self.fail(format!("before {}: {e}", call.name));
        }
        let mut arena = Arena::default();
        let uv = renamed(&mut arena, x.get("unit"));
        let cv = renamed(&mut arena, x.get("unit").get("combat"));
        let captured_unit = match Unit::value_in(arena.view(&uv), self.cx, "after") {
            Ok(r) => self.cx.store.unit.get(r).cloned().unwrap(),
            Err(e) => return self.fail(format!("{}: {e}", call.name)),
        };
        let captured_combat = match Combat::from_value(arena.view(&cv), self.cx, "after") {
            Ok(c) => c,
            Err(e) => return self.fail(format!("{}: {e}", call.name)),
        };
        for key in unit_keys {
            match *key {
                "alive" => unit.alive = captured_unit.alive,
                "dead" => unit.dead = captured_unit.dead,
                "deadTime" => unit.dead_time = captured_unit.dead_time,
                "character" => unit.character = captured_unit.character,
                "controller" => unit.controller = captured_unit.controller,
                "soldier" => unit.soldier = captured_unit.soldier.clone(),
                "seat" => unit.seat = captured_unit.seat,
                "remotesActive" => unit.remotes_active = captured_unit.remotes_active,
                "tickCount" => unit.tick_count = captured_unit.tick_count,
                "targetable" => unit.targetable = captured_unit.targetable,
                other => panic!("no unit field {other}"),
            }
        }
        for key in combat_keys {
            match *key {
                "health" => combat.health = captured_combat.health,
                "dead" => combat.dead = captured_combat.dead,
                "deathTime" => combat.death_time = captured_combat.death_time,
                "lastAttacker" => combat.last_attacker = captured_combat.last_attacker,
                "blowApart" => combat.blow_apart = captured_combat.blow_apart,
                "zoomStep" => combat.zoom_step = captured_combat.zoom_step,
                "mounted" => combat.mounted = captured_combat.mounted,
                "aimOverride" => combat.aim_override = captured_combat.aim_override,
                other => panic!("no combat field {other}"),
            }
        }
    }

    fn attacker_value(&mut self, a: Attacker) -> (Arena, Value) {
        let mut arena = Arena::default();
        let v = a.to_value(self.cx, &mut arena);
        (arena, v)
    }

    fn check_shot(&mut self, what: &str, captured: V, shot: &Shot) {
        match shot.kind {
            ShotKind::Round { yaw, pitch, charge, target_id, target_generation, .. } => {
                let r = check_num(&format!("{what} yaw"), captured.get("yaw"), yaw)
                    .and_then(|_| check_num(&format!("{what} pitch"), captured.get("pitch"), pitch))
                    .and_then(|_| check_num(&format!("{what} charge"), captured.get("charge"), charge))
                    .and_then(|_| check_num(&format!("{what} targetId"), captured.get("targetId"), target_id))
                    .and_then(|_| check_num(&format!("{what} targetGeneration"), captured.get("targetGeneration"), target_generation));
                self.check(r);
            }
            ShotKind::Repair => {
                if !captured.get("repair").truthy() {
                    self.fail(format!("{what}: the port's is a repair tick"));
                }
            }
            ShotKind::MeleeStart { swing } => self.check(check_num(&format!("{what} swing"), captured.get("swing"), swing)),
            ShotKind::MeleeHit | ShotKind::MeleeEnd => {}
        }
    }

    /// The next draw, of any generator but a press's or Math.random's: a
    /// ClassicRandom's uniform() from the next() the capture logged.
    fn any_draw(&mut self) -> f64 {
        let label = match self.log.peek() {
            Some(sim_replay::Event::Draw(d)) if !d.stream.starts_with("seeded:") && d.stream != "Math.random" => d.stream.clone(),
            _ => String::from("a weapon's generator"),
        };
        match self.log.draw(&label) {
            Ok(v) => self.nudged(uniform(v)),
            Err(e) => {
                self.fail(e.0);
                0.5
            }
        }
    }

    fn nudged(&mut self, v: f64) -> f64 {
        if std::mem::take(&mut self.nudge_draw) {
            1.0 - v
        } else {
            v
        }
    }
}

/// ClassicRandom.uniform() (src/ai/random.js:36) from the next() it drew.
fn uniform(next: f64) -> f64 {
    (jsrt::conv::to_uint32(next) >> 9) as f64 / 8388608.0
}

impl WeaponDraws for World<'_, '_> {
    fn draw(&mut self, _weapon: u32) -> f64 {
        self.any_draw()
    }

    fn legacy(&mut self, _weapon: u32) -> f64 {
        match self.log.draw("Math.random") {
            Ok(v) => self.nudged(v),
            Err(e) => {
                self.fail(e.0);
                0.5
            }
        }
    }

    fn seeded(&mut self, seed: f64, value: f64) {
        match self.log.draw(&format!("seeded:{}", seed)) {
            Ok(v) if v.to_bits() == value.to_bits() => {}
            Ok(v) => self.fail(format!("a seeded draw: the capture has {v}, the port {value}")),
            Err(e) => self.fail(e.0),
        }
    }

    fn admit(&mut self, _weapon: u32) -> Admission {
        let Some(call) = self.call("world.admitRemote") else { return Admission::Yes };
        let r = call.result();
        if r.opt_bool() == Some(true) {
            Admission::Yes
        } else if r.opt_bool() == Some(false) {
            Admission::No
        } else {
            Admission::Token(r.get("id").num())
        }
    }
}

impl CombatWorld for World<'_, '_> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, static_only: bool) -> Option<CastHit> {
        let call = self.call("world.cast")?;
        let r = check_vec3("cast origin", call.arg(0), arr(origin))
            .and_then(|_| check_vec3("cast direction", call.arg(1), arr(direction)))
            .and_then(|_| check_num("cast far", call.arg(2), far));
        self.check(r);
        if static_only != call.arg(5).truthy() {
            self.fail("cast staticOnly".into());
        }
        let r = call.result();
        if r.is_nullish() {
            return None;
        }
        Some(CastHit { distance: r.get("distance").num(), point: vec3(r.get("point")).unwrap_or_default(), normal: vec3(r.get("normal")).unwrap_or_default() })
    }

    fn remote_of(&mut self, _weapon: u32) -> Option<Droid> {
        let call = self.call("world.remoteOf")?;
        if call.result().is_nullish() {
            None
        } else {
            Some(Droid(0))
        }
    }

    fn remote_enter(&mut self, _droid: Droid, unit: &mut Unit, combat: &mut Combat) {
        if let Some(call) = self.call("world.remoteEnter") {
            self.after(call, unit, combat, REMOTE_UNIT, &[]);
        }
    }

    fn remote_kill(&mut self, _droid: Droid, unit: &mut Unit, combat: &mut Combat) {
        if let Some(call) = self.call("world.remoteKill") {
            self.after(call, unit, combat, HURT_UNIT, HURT_COMBAT);
        }
    }

    fn remote_launch(&mut self, _weapon: u32, origin: &Vector3, velocity: &Vector3, yaw: f64, team: f64, _admission: Option<f64>, unit: &mut Unit, combat: &mut Combat) {
        let Some(call) = self.call("world.remoteLaunch") else { return };
        let r = check_vec3("launch origin", call.arg(1), arr(origin))
            .and_then(|_| check_vec3("launch velocity", call.arg(2), arr(velocity)))
            .and_then(|_| check_num("launch yaw", call.arg(3), yaw))
            .and_then(|_| check_num("launch team", call.arg(4), team));
        self.check(r);
        self.after(call, unit, combat, REMOTE_UNIT, &[]);
    }

    fn lock_on(&mut self, eye: &Vector3, dir: &Vector3, _weapon: u32, _tool: &Weapon, current: Option<RepairTarget>, held: bool, team: f64) -> Option<RepairTarget> {
        let call = self.call("world.lockOn")?;
        let r = check_vec3("lockOn eye", call.arg(0), arr(eye))
            .and_then(|_| check_vec3("lockOn dir", call.arg(1), arr(dir)))
            .and_then(|_| check_num("lockOn team", call.arg(5), team));
        self.check(r);
        if call.arg(4).truthy() != held {
            self.fail("lockOn held".into());
        }
        let mut arena = Arena::default();
        let mine = match current {
            Some(t) => t.to_value(self.cx, &mut arena),
            None => Value::Null,
        };
        let theirs_v = renamed(&mut arena, call.arg(3));
        let theirs_null = call.arg(3).is_nullish();
        if theirs_null != current.is_none() || (!theirs_null && diff(arena.view(&theirs_v), arena.view(&mine)).is_some()) {
            self.fail("lockOn current target".into());
        }
        let result = renamed(&mut arena, call.result());
        if call.result().is_nullish() {
            return None;
        }
        match RepairTarget::from_value(arena.view(&result), self.cx, "lockOn") {
            Ok(t) => Some(t),
            Err(e) => {
                self.fail(e);
                None
            }
        }
    }

    fn lock_key(&mut self, _target: Option<RepairTarget>) -> StrId {
        let Some(call) = self.call("world.lockKey") else { return StrId(0) };
        let key = call.result().str().unwrap_or("").to_string();
        self.cx.strings.intern_str(&key)
    }

    fn repair(&mut self, _weapon: u32, _target: Option<RepairTarget>, team: f64, unit: &mut Unit, combat: &mut Combat) -> bool {
        let Some(call) = self.call("world.repair") else { return false };
        self.check(check_num("repair team", call.arg(2), team));
        self.after(call, unit, combat, &[], &[]);
        call.result().truthy()
    }

    fn lock_pick(&mut self, _weapon: u32, _launcher: &Weapon, origin: &Vector3, dir: &Vector3) -> LockPick {
        let Some(call) = self.call("world.lockPick") else { return LockPick::None };
        let r = check_vec3("lockPick origin", call.arg(1), arr(origin)).and_then(|_| check_vec3("lockPick dir", call.arg(2), arr(dir)));
        self.check(r);
        let result = call.result();
        if result.is_undefined() {
            return LockPick::Nothing;
        }
        if result.is_null() {
            return LockPick::None;
        }
        let Some(id) = result.get("unit").handle_num("unit") else {
            self.fail("lockPick: not a unit".into());
            return LockPick::None;
        };
        // The unit's life as the frame found it (its update's `lives`).
        let life = self.frame.before().get("lives").items().find(|u| u.get("id").opt_num() == Some(id));
        match life {
            Some(l) => LockPick::Unit { id, generation: l.get("generation").num(), alive: l.get("alive").truthy() },
            None => {
                self.fail(format!("lockPick: unit {id} not among the frame's lives"));
                LockPick::None
            }
        }
    }

    fn fire_round(&mut self, from: &Vector3, dir: &Vector3, shot: &Shot, bind: bool, _selected: Option<(f64, f64)>, unit: &mut Unit, combat: &mut Combat) {
        let Some(call) = self.call("world.fireRound") else { return };
        let r = check_vec3("fire from", call.arg(1), arr(from)).and_then(|_| check_vec3("fire dir", call.arg(2), arr(dir)));
        self.check(r);
        self.check_shot("fire shot", call.arg(3), shot);
        if call.arg(4).truthy() != bind {
            self.fail("fire bind".into());
        }
        self.after(call, unit, combat, HURT_UNIT, HURT_COMBAT);
    }

    fn dispense(&mut self, _weapon: u32, most: f64, origin: &Vector3, velocity: &Vector3, yaw: f64, team: f64, unit: &mut Unit, combat: &mut Combat) -> bool {
        let Some(call) = self.call("world.dispense") else { return false };
        let r = check_num("dispense most", call.arg(1), most)
            .and_then(|_| check_vec3("dispense origin", call.arg(3), arr(origin)))
            .and_then(|_| check_vec3("dispense velocity", call.arg(4), arr(velocity)))
            .and_then(|_| check_num("dispense yaw", call.arg(5), yaw))
            .and_then(|_| check_num("dispense team", call.arg(7), team));
        self.check(r);
        self.after(call, unit, combat, &[], &[]);
        call.result().truthy()
    }

    fn melee_strike(&mut self, unit: &mut Unit, combat: &mut Combat) {
        if let Some(call) = self.call("world.meleeStrike") {
            self.after(call, unit, combat, HURT_UNIT, HURT_COMBAT);
        }
    }

    fn arc_targets(&mut self) -> Vec<ArcTarget> {
        let Some(call) = self.call("world.arcTargets") else { return Vec::new() };
        call.result()
            .items()
            .map(|t| ArcTarget { origin: vec3(t.get("origin")).unwrap_or_default(), center: vec3(t.get("center")), radius: t.get("radius").opt_num() })
            .collect()
    }

    fn hurt_arc_target(&mut self, index: usize, amount: f64, from: &Vector3, unit: &mut Unit, combat: &mut Combat) {
        let Some(call) = self.call("world.hurtArcTarget") else { return };
        let r = check_num("arc index", call.arg(0), index as f64).and_then(|_| check_num("arc damage", call.arg(1), amount)).and_then(|_| check_vec3("arc from", call.arg(2), arr(from)));
        self.check(r);
        self.after(call, unit, combat, HURT_UNIT, HURT_COMBAT);
    }

    fn has_lightning(&mut self, _name: &str) -> bool {
        let Some(call) = self.call("world.lightning") else { return false };
        !call.result().is_nullish()
    }

    fn fired(&mut self, shot: &Shot, unit: &mut Unit, combat: &mut Combat) {
        let Some(call) = self.call("world.fired") else { return };
        self.check_shot("fired", call.arg(0), shot);
        self.after(call, unit, combat, &[], &[]);
    }

    fn damaged(&mut self, amount: f64, attacker: Attacker, info: &DamageInfo, source: Option<Vector3>, unit: &mut Unit, combat: &mut Combat) {
        let Some(call) = self.call("world.damaged") else { return };
        self.check(check_num("damaged amount", call.arg(0), amount));
        let (arena, mine) = self.attacker_value(attacker);
        let mut theirs = Arena::default();
        let t = renamed(&mut theirs, call.arg(1));
        if diff(theirs.view(&t), arena.view(&mine)).is_some() {
            self.fail(format!("damaged attacker: the capture has {}, the port {}", call.arg(1).show(), arena.view(&mine).show()));
        }
        let i = call.arg(2);
        match source {
            Some(s) => self.check(check_vec3("damaged source", i.get("source"), arr(&s))),
            None => {
                if !i.get("source").is_nullish() {
                    self.fail("damaged source: the port has none".into());
                }
            }
        }
        if info.explosion {
            let r = check_num("damaged info amount", i.get("amount"), info.amount.unwrap_or(f64::NAN))
                .and_then(|_| check_num("damaged info typed", i.get("typed"), info.typed.unwrap_or(f64::NAN)))
                .and_then(|_| check_num("damaged info generic", i.get("generic"), info.generic.unwrap_or(f64::NAN)));
            self.check(r);
            if i.get("kind").str() != Some("explosion") {
                self.fail("damaged info kind".into());
            }
        }
        self.after(call, unit, combat, &[], &[]);
    }

    fn died(&mut self, unit: &mut Unit, combat: &mut Combat) {
        if let Some(call) = self.call("world.died") {
            self.after(call, unit, combat, DIED_UNIT, DIED_COMBAT);
        }
    }

    fn blown_apart(&mut self, health: f64, unit: &mut Unit, combat: &mut Combat) {
        if let Some(call) = self.call("world.blownApart") {
            self.check(check_num("blownApart health", call.arg(0), health));
            self.after(call, unit, combat, HURT_UNIT, HURT_COMBAT);
        }
    }

    fn respawn(&mut self, unit: &mut Unit, combat: &mut Combat) {
        if let Some(call) = self.call("world.respawn") {
            self.after(call, unit, combat, &[], &[]);
        }
    }

    fn presentation(&mut self, event: &Presentation) {
        let Some(call) = self.call("world.presentation") else { return };
        let e = call.arg(0);
        match event {
            Presentation::WeaponAction { action, bank, posture, prepare } => {
                let ok = e.get("kind").str() == Some("weaponAction")
                    && e.get("action").str() == Some(action.as_str())
                    && e.get("bank").str() == Some(bank.as_str())
                    && e.get("posture").str() == *posture
                    && e.get("prepare").truthy() == *prepare;
                if !ok {
                    self.fail(format!("presentation: the capture has {}, the port {event:?}", e.show()));
                }
            }
            Presentation::EndWeaponAction { action } => {
                if e.get("kind").str() != Some("endWeaponAction") || e.get("action").str() != *action {
                    self.fail(format!("presentation: the capture has {}, the port {event:?}", e.show()));
                }
            }
            Presentation::Lightning { name, from, to, charge, channel } => {
                let pos = |v: V| -> Option<[f64; 3]> { Some([v.at(0).opt_num()?, v.at(1).opt_num()?, v.at(2).opt_num()?]) };
                let same = |a: Option<[f64; 3]>, b: &Vector3| a.is_some_and(|a| a.iter().zip(arr(b)).all(|(x, y)| x.to_bits() == y.to_bits()));
                let ok = e.get("kind").str() == Some("lightning")
                    && e.get("name").str() == Some(name.as_str())
                    && same(pos(e.get("from")), from)
                    && same(pos(e.get("to")), to)
                    && e.get("charge").opt_num().map(f64::to_bits) == Some(charge.to_bits())
                    && e.get("channel").opt_num() == *channel;
                if !ok {
                    self.fail(format!("presentation: the capture has {}, the port {event:?}", e.show()));
                }
            }
        }
    }

    fn next_gun(&mut self, unit: &mut Unit, combat: &mut Combat) -> Option<Vector3> {
        let call = self.call("body.nextGun")?;
        let r = vec3(call.result());
        self.after(call, unit, combat, BODY_UNIT, &[]);
        r
    }

    fn gun_at(&mut self, selected: (f64, f64)) -> Option<Vector3> {
        let call = self.call("body.gunAt")?;
        let s = call.arg(1);
        let r = check_num("gunAt index", s.get("index"), selected.0).and_then(|_| check_num("gunAt barrel", s.get("barrel"), selected.1));
        self.check(r);
        vec3(call.result())
    }

    fn shield_hit(&mut self, taken: f64, _left: f64, _source: Option<Vector3>, _explosion: bool) {
        let Some(call) = self.call("units.shieldHit") else { return };
        self.check(check_num("shieldHit amount", call.arg(1), taken));
    }

    fn draw_combat(&mut self, generator: bool) -> f64 {
        if generator {
            match self.log.draw("stream:gameplay") {
                Ok(v) => self.nudged(uniform(v)),
                Err(e) => {
                    self.fail(e.0);
                    0.5
                }
            }
        } else {
            self.legacy(0)
        }
    }

    fn text(&self, id: StrId) -> String {
        String::from_utf16_lossy(self.cx.strings.units(id))
    }
}

// ---- The frames.

fn attacker_in(cx: &mut Replay, v: V) -> Result<Attacker, String> {
    let mut arena = Arena::default();
    let r = renamed(&mut arena, v);
    Attacker::from_value(arena.view(&r), cx, "attacker")
}

fn combat_frame(capture: &Capture, frame: &Frame, content: &mut Content, cx: &mut Replay, mutation: Mutation) -> Result<Option<String>, Mismatch> {
    let before = frame.before();
    if before.get("skipped").truthy() {
        return Ok(Some("written in part".into()));
    }
    cx.reset();
    let (id, mut unit, mut combat, edges) = read_state(capture, content, cx, before).map_err(|e| Mismatch(format!("{}: reading the state: {e}", frame.place())))?;
    let defs = std::mem::take(&mut content.defs);
    let mut world = World { frame, log: Log::new(frame), cx, id, error: None, nudge_draw: mutation == Mutation::Draw };
    let method = frame.name.rsplit('.').next().unwrap_or("");
    match method {
        "update" => {
            let dt = frame.arg(0).num();
            let dt = if mutation == Mutation::Dt { up_one_ulp(dt) } else { dt };
            let stick = frame.arg(2).num();
            cb::update(&mut combat, id, &mut unit, &defs, dt, stick, &edges, &mut world);
        }
        "flushMuzzles" => cb::flush_muzzles(&mut combat, &mut unit, &defs, &edges, &mut world),
        "damage" => {
            let amount = frame.arg(0).num();
            let on_shield = if frame.arg(1).is_undefined() { amount } else { frame.arg(1).num() };
            let source = vec3(frame.arg(2));
            let attacker = if frame.arg(3).is_undefined() { Attacker::Null } else { attacker_in(world.cx, frame.arg(3)).map_err(Mismatch)? };
            let i = frame.arg(4);
            let info = DamageInfo {
                explosion: i.get("kind").str() == Some("explosion"),
                amount: i.get("amount").opt_num(),
                typed: i.get("typed").opt_num(),
                generic: i.get("generic").opt_num(),
                known: i.get("known").opt_bool(),
                point: None,
            };
            cb::damage_with(&mut combat, &mut unit, &defs, amount, on_shield, source, attacker, &info, &mut world);
        }
        "explode" => {
            let explosion = content.js(capture, world.cx, frame.arg(0), "explosion");
            let position = vec3(frame.arg(1)).unwrap_or_default();
            let owner = if frame.arg(2).is_undefined() { Attacker::Null } else { attacker_in(world.cx, frame.arg(2)).map_err(Mismatch)? };
            let scale = if frame.arg(3).is_undefined() { 1.0 } else { frame.arg(3).num() };
            cb::explode(&mut combat, &mut unit, &defs, &explosion, &position, owner, scale, &mut world);
        }
        "reset" => cb::reset(&mut combat, &unit, &defs),
        other => return Err(Mismatch(format!("no frame {other}"))),
    }
    content.defs = defs;
    if let Some(e) = world.error.take() {
        return Err(Mismatch(e));
    }
    world.log.finish()?;
    let cx = world.cx;
    compare(cx, id, &unit, &combat, frame.after(), &[], &[]).map_err(|e| Mismatch(format!("{}: at the end: {e}", frame.place())))?;
    Ok(None)
}

fn outcome(r: Result<Option<String>, Mismatch>) -> Outcome {
    match r {
        Ok(None) => Outcome::Same,
        Ok(Some(why)) => Outcome::Skipped(why),
        Err(e) => Outcome::Failed(e.0),
    }
}

fn replay_with(name: &str, mutation: Mutation) -> Option<sim_replay::Report> {
    let corpus = Corpus::from_env().unwrap()?;
    let mut content: HashMap<String, (Content, Replay)> = HashMap::new();
    let report = replay_corpus(&corpus, name, |capture, frame| {
        let (content, cx) = content.entry(capture.label()).or_insert_with(|| (Content::default(), Replay::new(Default::default())));
        outcome(combat_frame(capture, frame, content, cx, mutation))
    })
    .unwrap();
    Some(report)
}

fn replay(name: &str) {
    let Some(report) = replay_with(name, Mutation::None) else { return };
    println!("{}", report.summary());
    report.assert_clean();
}

/// A mutation check: the port with one thing nudged must fail frames.
fn mutated(name: &str, mutation: Mutation, what: &str) {
    let Some(report) = replay_with(name, mutation) else { return };
    println!("{what}: {} of {} replayed frames differ", report.failures.len(), report.replayed());
    assert!(!report.failures.is_empty(), "{what}: no frame noticed");
}

#[test]
fn a_tick_one_ulp_longer_shows() {
    mutated("PlayerCombat.update", Mutation::Dt, "updates, dt + 1 ulp");
}

#[test]
fn another_draw_shows() {
    mutated("PlayerCombat.update", Mutation::Draw, "updates, first draw 1 - it");
    mutated("PlayerCombat.damage", Mutation::Draw, "damage, first draw 1 - it");
}

#[test]
fn combat_updates_replay_as_captured() {
    replay("PlayerCombat.update");
}

#[test]
fn droideka_muzzles_replay_as_captured() {
    replay("PlayerCombat.flushMuzzles");
}

#[test]
fn damage_replays_as_captured() {
    replay("PlayerCombat.damage");
}

#[test]
fn blasts_replay_as_captured() {
    replay("PlayerCombat.explode");
}

#[test]
fn new_lives_replay_as_captured() {
    replay("PlayerCombat.reset");
}

/// What the corpus's replayed frames exercise: their calls across the
/// boundary by name, presentation events by kind, and draws by stream
/// (`cargo test ... -- --ignored corpus_coverage`).
#[test]
#[ignore]
fn corpus_coverage() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    for name in ["PlayerCombat.update", "PlayerCombat.flushMuzzles", "PlayerCombat.damage", "PlayerCombat.explode", "PlayerCombat.reset"] {
        for capture in corpus.captures() {
            let capture = capture.unwrap();
            for frame in capture.frames(name) {
                let frame = frame.unwrap();
                if frame.before().get("skipped").truthy() {
                    continue;
                }
                *counts.entry(format!("{name} frames")).or_default() += 1;
                for event in &frame.events {
                    let key = match event {
                        sim_replay::Event::Call(c) if c.name == "world.presentation" => format!("{name} presentation {}", c.arg(0).get("kind").str().unwrap_or("?")),
                        sim_replay::Event::Call(c) if c.name == "world.lockPick" => {
                            let r = c.result();
                            format!("{name} lockPick {}", if r.is_undefined() { "nothing" } else if r.is_null() { "none" } else { "unit" })
                        }
                        sim_replay::Event::Call(c) if c.name == "world.fired" => {
                            let s = c.arg(0);
                            let kind = if s.get("repair").truthy() { "repair" } else if s.get("meleeStart").truthy() { "melee" } else if s.get("targetId").num() >= 0.0 { "guided" } else { "round" };
                            format!("{name} fired {kind}")
                        }
                        sim_replay::Event::Call(c) => format!("{name} call {}", c.name),
                        sim_replay::Event::Draw(d) => format!("{name} draw {}", if d.stream.starts_with("seeded:") { "seeded" } else { d.stream.as_str() }),
                    };
                    *counts.entry(key).or_default() += 1;
                }
            }
        }
    }
    for (k, n) in &counts {
        println!("{n:>9} {k}");
    }
}
