//! The units package's replays (`cargo test -p sim --features replay
//! --test replay_units`, BFE_REPLAY_CORPUS a directory of captures made with
//! `tools/sim-capture.mjs --specs units`): each frame starts from the state
//! the JavaScript started from, runs the Rust port, serves every call across
//! the units' boundary from the capture (checking what the port asks), and
//! compares the state it ends with, bit for bit.
//!
//! A served call's effect on the unit is the capture's (the unit as the call
//! left it), and only on the fields that call is known to change: before it
//! is applied, every other field must already be what the JavaScript had,
//! so a served call cannot hide a mistake of the port's before it.
//!
//! Without a corpus the tests pass without replaying (CI has no game data).
#![cfg(feature = "replay")]

use bfe_sim::gen::{Replay, Unit};
use bfe_sim::rig::three::Vector3;
use bfe_sim::store::replay::{Arena, Value, V};
use bfe_sim::units::shapes::{Bounds, HitShape};
use bfe_sim::units::world::{CastHit, GatherPolygon, UnitsWorld};
use sim_replay::{check_num, check_vec3, diff, replay_corpus, Capture, Corpus, Frame, Log, Mismatch, Outcome};

fn vec3(v: V) -> Option<Vector3> {
    v.vec3().map(|a| Vector3 { x: a[0], y: a[1], z: a[2] })
}

/// An object's value without some of its keys, copied into `arena`.
fn without(arena: &mut Arena, v: V, keys: &[&str]) -> Value {
    let fields: Vec<(String, V)> = v.fields().filter(|(k, _)| !keys.contains(k)).map(|(k, x)| (k.to_string(), x)).collect();
    let mut out = Vec::with_capacity(fields.len());
    for (k, x) in fields {
        let c = arena.copy(x);
        out.push((k, c));
    }
    let refs: Vec<(&str, Value)> = out.iter().map(|(k, x)| (k.as_str(), x.clone())).collect();
    arena.object("", refs)
}

/// The unit's value as the capture writes it (`$id` first, then its fields).
fn unit_value(cx: &mut Replay, arena: &mut Arena, id: u32, unit: &Unit) -> Value {
    let r = bfe_sim::store::Ref::<Unit>::new(id, 0);
    *cx.store.unit.get_mut(r).expect("the unit is in the store") = unit.clone();
    Unit::value_out(r, cx, arena)
}

/// Copies the fields `touched` names from `from` into `unit`.
fn copy_fields(unit: &mut Unit, from: &Unit, touched: &[&str]) {
    for key in touched {
        match *key {
            "soldier" => unit.soldier = from.soldier.clone(),
            "intent" => unit.intent = from.intent.clone(),
            "controls" => unit.controls = from.controls.clone(),
            "input" => unit.input = from.input.clone(),
            "combatDead" => unit.combat_dead = from.combat_dead,
            "alive" => unit.alive = from.alive,
            "dead" => unit.dead = from.dead,
            "deadTime" => unit.dead_time = from.dead_time,
            "removed" => unit.removed = from.removed,
            "character" => unit.character = from.character,
            "controller" => unit.controller = from.controller,
            "seat" => unit.seat = from.seat,
            "held" => unit.held = from.held.clone(),
            "mounted" => unit.mounted = from.mounted,
            "aimOverride" => unit.aim_override = from.aim_override.clone(),
            "remotesActive" => unit.remotes_active = from.remotes_active,
            "pivot" => unit.pivot = from.pivot,
            "gunUnderwater" => unit.gun_underwater = from.gun_underwater,
            "waterContact" => unit.water_contact = from.water_contact.clone(),
            "tickCount" => unit.tick_count = from.tick_count,
            "team" => unit.team = from.team,
            "generation" => unit.generation = from.generation,
            other => panic!("no field {other} to copy"),
        }
    }
}

// What each call across a unit's tick may change of the unit. The aimer
// writes the intent's aim in place, and combat's aimOverride is that same
// vector in the JavaScript, so it moves with it; combat's update sets its
// aim pivot.
const MOVE: &[&str] = &["soldier"];
const DAMAGE: &[&str] = &["combatDead", "alive", "dead", "deadTime", "character", "soldier", "seat", "mounted"];
const AIM: &[&str] = &["intent", "soldier", "aimOverride"];
const UPDATE: &[&str] = &["intent", "controls", "soldier", "combatDead", "alive", "dead", "deadTime", "character", "seat", "mounted", "pivot"];
const MUZZLES: &[&str] = &["soldier"];
const NOTHING: &[&str] = &[];
const BLOW_APART: &[&str] = &["soldier", "combatDead", "alive", "dead", "deadTime", "character"];
const REMOTE: &[&str] = &["intent", "controls", "soldier"];

/// A unit's tick served from its frame.
struct TickWorld<'a, 'f> {
    frame: &'f Frame,
    log: Log<'f>,
    cx: &'a mut Replay,
    arena: Arena,
    id: u32,
    /// The frame's state as it began, and its supplements.
    state: V<'f>,
    /// Whether the Character the tick began with still has this unit.
    character_has: bool,
    /// The class's free bodies.
    pool: Vec<f64>,
    error: Option<Mismatch>,
}

impl<'a, 'f> TickWorld<'a, 'f> {
    fn fail(&mut self, what: String) {
        if self.error.is_none() {
            self.error = Some(Mismatch(format!("{}: {what}", self.frame.place())));
        }
    }

    /// The next call, which must be `name`.
    fn call(&mut self, name: &str) -> Option<&'f sim_replay::Call> {
        match self.log.call(name) {
            Ok(c) => Some(c),
            Err(e) => {
                if self.error.is_none() {
                    self.error = Some(e);
                }
                None
            }
        }
    }

    /// The unit as the call left it, on the fields it may change; every
    /// other field must already be the JavaScript's.
    fn after(&mut self, call: &'f sim_replay::Call, unit: &mut Unit, touched: &[&str]) {
        let after = call.extra().get("unit");
        if after.is_nullish() {
            return self.fail(format!("{}: no unit after the call", call.name));
        }
        let mine = unit_value(self.cx, &mut self.arena, self.id, unit);
        let mut skip = vec!["$id"];
        skip.extend_from_slice(touched);
        let a = without(&mut self.arena, after, &skip);
        let mine_view = self.arena.view(&mine);
        let b_value = {
            let mut scratch = Arena::default();
            let v = without(&mut scratch, mine_view, &skip);
            (scratch, v)
        };
        if let Some(d) = diff(self.arena.view(&a), b_value.0.view(&b_value.1)) {
            return self.fail(format!("before {}: {d} (capture | port)", call.name));
        }
        match Unit::from_value(after, self.cx, "after") {
            Ok(captured) => copy_fields(unit, &captured, touched),
            Err(e) => self.fail(format!("{}: {e}", call.name)),
        }
    }
}

impl UnitsWorld for TickWorld<'_, '_> {
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64) -> Option<CastHit> {
        let call = self.call("collision.cast")?;
        let checks = check_vec3("cast origin", call.arg(0), [origin.x, origin.y, origin.z])
            .and_then(|_| check_vec3("cast direction", call.arg(1), [direction.x, direction.y, direction.z]))
            .and_then(|_| check_num("cast far", call.arg(2), far));
        if let Err(e) = checks {
            self.fail(e.0);
            return None;
        }
        let r = call.result();
        if r.is_nullish() {
            return None;
        }
        Some(CastHit {
            point: vec3(r.get("point")).unwrap_or_default(),
            normal: vec3(r.get("normal")).unwrap_or_default(),
            distance: r.get("distance").num(),
            terrain: r.get("terrain").truthy(),
        })
    }

    fn gather(&mut self, _id: u32, center: &Vector3, radius: f64, cache: usize, out: &mut Vec<GatherPolygon>) {
        out.clear();
        let Some(call) = self.call("collision.gather") else { return };
        let checks = check_vec3("gather center", call.arg(0), [center.x, center.y, center.z]).and_then(|_| check_num("gather radius", call.arg(1), radius));
        if let Err(e) = checks {
            return self.fail(e.0);
        }
        // The cache's number: a droideka's base's first, then its ball's (as
        // `gatherCaches ??= [base, ball]` made them), numbered by the capture
        // in the order it met them.
        let _ = cache;
        for p in call.result().items() {
            let triangles = p
                .get("triangles")
                .items()
                .map(|t| [vec3(t.get("a")).unwrap_or_default(), vec3(t.get("b")).unwrap_or_default(), vec3(t.get("c")).unwrap_or_default()])
                .collect();
            out.push(GatherPolygon { triangles, moving: p.get("moving").truthy() });
        }
    }

    fn seat_exposed(&mut self, _id: u32, _unit: &Unit) -> bool {
        self.fail("a seat's exposure asked in a tick".into());
        false
    }
    fn rider_shapes(&mut self, _id: u32, _unit: &Unit, _shapes: &mut [HitShape]) -> usize {
        self.fail("a rider's shapes asked in a tick".into());
        0
    }
    fn held_shapes(&mut self, _id: u32, _unit: &Unit, _shapes: &mut [HitShape]) -> usize {
        self.fail("a held body's shapes asked in a tick".into());
        0
    }
    fn target_alive(&mut self, _id: u32) -> bool {
        self.fail("a target asked in a tick".into());
        false
    }
    fn target_dead(&mut self, _id: u32) -> bool {
        false
    }
    fn target_sphere(&mut self, _id: u32) -> Bounds {
        Bounds::default()
    }

    fn move_soldier(&mut self, _id: u32, unit: &mut Unit, h: f64) {
        let Some(call) = self.call("soldier.tick") else { return };
        // soldier.tick(input, h): the input as the unit handed it.
        let input = call.arg(0);
        let i = &unit.input;
        let mine_input = [
            ("forward", Value::Num(i.forward)),
            ("right", Value::Num(i.right)),
            ("turn", Value::Num(i.turn)),
            ("pitch", Value::Num(i.pitch)),
            ("jump", Value::Bool(i.jump)),
            ("crouch", Value::Bool(i.crouch)),
            ("prone", Value::Bool(i.prone)),
            ("use", Value::Bool(i.r#use)),
        ];
        for (k, v) in mine_input {
            let a = Arena::default();
            let mine = a.view(&v);
            if let Some(d) = diff(input.get(k), mine) {
                return self.fail(format!("soldier.tick input.{k}: {d}"));
            }
        }
        if let Err(e) = check_num("soldier.tick h", call.arg(1), h) {
            return self.fail(e.0);
        }
        self.after(call, unit, MOVE);
    }

    fn damage(&mut self, _id: u32, unit: &mut Unit, amount: f64) {
        let Some(call) = self.call("combat.damage") else { return };
        if let Err(e) = check_num("combat.damage amount", call.arg(0), amount) {
            return self.fail(e.0);
        }
        self.after(call, unit, DAMAGE);
    }

    fn aim(&mut self, id: u32, unit: &mut Unit, h: f64) {
        let Some(call) = self.call("ai.aim") else { return };
        if call.arg(0).handle_num("unit") != Some(id as f64) {
            return self.fail(format!("ai.aim: the capture's unit {}", call.arg(0).show()));
        }
        if let Err(e) = check_num("ai.aim h", call.arg(1), h) {
            return self.fail(e.0);
        }
        self.after(call, unit, AIM);
    }

    fn has_remote_step(&mut self, _id: u32, _unit: &Unit) -> bool {
        self.state.get("remoteStep").truthy()
    }

    fn remote_step(&mut self, _id: u32, unit: &mut Unit, h: f64, active: bool) {
        let Some(call) = self.call("remotes.step") else { return };
        // remoteWorld.step(player, tick, dt, input, controls).
        if let Err(e) = check_num("remotes.step dt", call.arg(2), h) {
            return self.fail(e.0);
        }
        if call.arg(3).is_null() == active {
            return self.fail(format!("remotes.step: the capture's input {}, the port {}", call.arg(3).show(), if active { "the intent" } else { "null" }));
        }
        self.after(call, unit, REMOTE);
    }

    fn combat_update(&mut self, _id: u32, unit: &mut Unit, h: f64, stick: f64) {
        let Some(call) = self.call("combat.update") else { return };
        let checks = check_num("combat.update h", call.arg(0), h).and_then(|_| check_num("combat.update stick", call.arg(2), stick));
        if let Err(e) = checks {
            return self.fail(e.0);
        }
        self.after(call, unit, UPDATE);
    }

    fn flush_muzzles(&mut self, _id: u32, unit: &mut Unit) {
        let Some(call) = self.call("combat.flushMuzzles") else { return };
        self.after(call, unit, MUZZLES);
    }

    fn clear_charge(&mut self, _id: u32, unit: &mut Unit) {
        let Some(call) = self.call("combat.clearCharge") else { return };
        self.after(call, unit, NOTHING);
    }

    fn death_blast(&mut self, id: u32, unit: &mut Unit) {
        let Some(call) = self.call("units.deathBlast") else { return };
        if call.arg(0).handle_num("unit") != Some(id as f64) {
            return self.fail(format!("units.deathBlast: the capture's unit {}", call.arg(0).show()));
        }
        self.after(call, unit, NOTHING);
    }

    fn blow_apart(&mut self, id: u32, unit: &mut Unit, health: f64) {
        let Some(call) = self.call("units.blowApart") else { return };
        if call.arg(0).handle_num("unit") != Some(id as f64) {
            return self.fail(format!("units.blowApart: the capture's unit {}", call.arg(0).show()));
        }
        if let Err(e) = check_num("units.blowApart health", call.arg(1), health) {
            return self.fail(e.0);
        }
        self.after(call, unit, BLOW_APART);
    }

    fn let_go_of_character(&mut self, _id: u32, unit: &mut Unit) {
        if unit.character.is_some() {
            self.character_has = false;
        }
    }

    fn release(&mut self, id: u32, _unit: &Unit) {
        if !self.pool.contains(&(id as f64)) {
            self.pool.push(id as f64);
        }
    }
}

/// The ids of a list of unit handles (a pool).
fn ids(v: V) -> Option<Vec<f64>> {
    if v.is_nullish() {
        return None;
    }
    Some(v.items().filter_map(|x| x.handle_num("unit")).collect())
}

/// One unit's tick (Unit.tick), replayed.
fn unit_tick(_capture: &Capture, frame: &Frame) -> Result<Option<String>, Mismatch> {
    let before = frame.before();
    if before.get("skipped").truthy() {
        return Ok(Some("written in part (its calls only)".into()));
    }
    if before.get("unit").get("walker").truthy() {
        return Ok(Some("a droideka's (needs its clips and rig)".into()));
    }
    let h = frame.arg(0).num();
    let mut cx = Replay::default();
    let unit_v = before.get("unit");
    let r = Unit::value_in(unit_v, &mut cx, "unit").map_err(|e| Mismatch(format!("{}: reading the unit: {e}", frame.place())))?;
    let id = r.slot;
    let mut unit = cx.store.unit.get(r).cloned().unwrap();
    // The Character's hold on him: whether the one the tick began with still names him.
    let character_has = before.get("characterUnit").handle_num("unit") == Some(id as f64);
    let pool = ids(before.get("pool")).unwrap_or_default();
    let mut world = TickWorld { frame, log: Log::new(frame), cx: &mut cx, arena: Arena::default(), id, state: before, character_has, pool, error: None };
    unit.tick(id, h, &mut world, None);
    if let Some(e) = world.error.take() {
        return Err(e);
    }
    world.log.finish()?;
    // The state it ends with: the unit, its Character's hold and the pool.
    let after = frame.after();
    let mine = unit_value(world.cx, &mut world.arena, id, &unit);
    let mut a = Arena::default();
    let theirs = without(&mut a, after.get("unit"), &[]);
    if let Some(d) = diff(a.view(&theirs), world.arena.view(&mine)) {
        return Err(Mismatch(format!("{}: unit {d} (capture | port)", frame.place())));
    }
    let character_now = after.get("characterUnit").handle_num("unit") == Some(id as f64);
    if character_now != world.character_has {
        return Err(Mismatch(format!("{}: its Character's unit: the capture's {}, the port {}", frame.place(), after.get("characterUnit").show(), world.character_has)));
    }
    if ids(after.get("pool")) != ids(before.get("pool")).map(|_| world.pool.clone()) {
        return Err(Mismatch(format!("{}: the pool: the capture's {}, the port {:?}", frame.place(), after.get("pool").show(), world.pool)));
    }
    Ok(None)
}

fn outcome(r: Result<Option<String>, Mismatch>) -> Outcome {
    match r {
        Ok(None) => Outcome::Same,
        Ok(Some(why)) => Outcome::Skipped(why),
        Err(e) => Outcome::Failed(e.0),
    }
}

#[test]
fn unit_ticks_replay_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let report = replay_corpus(&corpus, "Unit.tick", |capture, frame| outcome(unit_tick(capture, frame))).unwrap();
    println!("{}", report.summary());
    report.assert_clean();
}
