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
use bfe_sim::pose::Clip;
use bfe_sim::rig::droideka::{Barrel, DroidekaRig, Gun, CLIPS};
use bfe_sim::rig::skeleton::{BoneDesc, Skeleton};
use bfe_sim::units::droideka::{DroidekaDef, DroidekaParts, ModeClips};
use std::collections::HashMap;
use std::rc::Rc;
use bfe_sim::rig::three::Vector3;
use bfe_sim::store::replay::{Arena, Node, Value, V};
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
fn unit_tick(capture: &Capture, frame: &Frame, contents: &mut Contents) -> Result<Option<String>, Mismatch> {
    let before = frame.before();
    if before.get("skipped").truthy() {
        return Ok(Some("written in part (its calls only)".into()));
    }
    let walker = before.get("unit").get("walker").truthy();
    let mut parts = None;
    if walker {
        let content = contents.of(capture).map_err(|e| Mismatch(format!("{}: content: {e}", frame.place())))?;
        let Some(content) = content else { return Ok(Some("a droideka's (needs BFE_CONTENT_DIR for its clips and rig)".into())) };
        let u = before.get("unit");
        let (side, class) = (u.get("side").str().unwrap_or(""), u.get("className").str().unwrap_or(""));
        let mut p = droideka_parts(&content, side, class).map_err(|e| Mismatch(format!("{}: {e}", frame.place())))?;
        // The rig's bones as the last tick left them, where an unfold's
        // blend may start from them.
        if let (Some(rig), bones) = (p.rig.as_mut(), before.get("rig").get("bones")) {
            for (i, b) in bones.items().enumerate() {
                let (Some(q), Some(pos)) = (b.get("q").node(), b.get("p").vec3()) else { continue };
                let Node::Quat(q) = q else { continue };
                let node = rig.tree.node_mut(rig.bones[i]);
                node.copy_quaternion(&bfe_sim::rig::three::Quaternion { x: q[0], y: q[1], z: q[2], w: q[3] });
                node.position = bfe_sim::rig::three::Vector3 { x: pos[0], y: pos[1], z: pos[2] };
            }
        }
        parts = Some(p);
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
    unit.tick(id, h, &mut world, parts.as_mut());
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
    if let Some(rig) = parts.as_ref().and_then(|p| p.rig.as_deref()) {
        check_muzzles(frame, after.get("rig").get("muzzles"), rig)?;
    }
    Ok(None)
}


// ---- A droideka's class, clips and hidden gun rig, from the battle's content.

/// The battle's content as the game's headless start loads it (rust/content),
/// once a battle query; None without BFE_CONTENT_DIR.
struct Contents {
    root: Option<std::path::PathBuf>,
    loaded: HashMap<String, Rc<bfe_content::content::Content>>,
}

impl Contents {
    fn new() -> Contents {
        Contents { root: std::env::var_os("BFE_CONTENT_DIR").map(Into::into), loaded: HashMap::new() }
    }

    fn of(&mut self, capture: &Capture) -> Result<Option<Rc<bfe_content::content::Content>>, String> {
        let Some(root) = &self.root else { return Ok(None) };
        let battle = capture.header.root().get("battle");
        let map = battle.get("map").str().ok_or("no map in the capture's header")?.to_string();
        let query = battle.get("query").str().ok_or("no query in the capture's header")?.to_string();
        if let Some(c) = self.loaded.get(&query) {
            return Ok(Some(c.clone()));
        }
        let files = bfe_content::files::DirFiles::new(root);
        let loader = bfe_content::battle::Loader::new(&files);
        let b = bfe_content::battle::Battle::load(&loader, &map, &query)?;
        let content = Rc::new(bfe_content::content::Content::build(b)?);
        self.loaded.insert(query, content.clone());
        Ok(Some(content))
    }
}

/// A content clip as the pose sampler takes one.
fn pose_clip(c: &bfe_content::clips::Clip) -> Clip {
    Clip { frames: c.frames_f64() as usize, fps: c.fps, root_motion: c.root_motion, tracks: c.tracks.iter().map(|&t| t as i32).collect(), data: c.data().to_vec() }
}

/// What a droideka's tick needs besides its state, for a unit of class
/// `class` of side `side` (units.js build: Droideka's constructor and arm).
fn droideka_parts(content: &bfe_content::content::Content, side: &str, class: &str) -> Result<DroidekaParts, String> {
    let def = content.battle.side(side).and_then(|s| s.units.get(class)).ok_or_else(|| format!("no class {side}/{class}"))?;
    let num = |k: &str| def.get(k).filter(|v| !v.is_null()).and_then(|v| v.as_f64());
    let d = DroidekaDef {
        max_pitch_speed: num("MaxPitchSpeed"),
        ball_turn_speed: num("BallTurnSpeed"),
        ball_max_lean: num("BallMaxLean"),
        ball_acceleration: num("BallAcceleration"),
        ball_rolling_friction: num("BallRollingFriction"),
    };
    let clips = content.walker_clips.iter().find(|(k, _)| k == &format!("{side}/{class}")).map(|(_, c)| c).ok_or_else(|| format!("no clips for {side}/{class}"))?;
    let mut by_mode: [Option<Rc<Clip>>; 14] = Default::default();
    for (mode, name) in CLIPS.iter().enumerate() {
        if let Some(name) = name {
            by_mode[mode] = clips.get(name).map(|c| Rc::new(pose_clip(c)));
        }
    }
    let model = def.get("model").and_then(|m| m.as_str()).ok_or("no model")?;
    let actor = content.actor(&format!("{side}/{model}")).ok_or_else(|| format!("no body {side}/{model}"))?;
    let bones: Vec<BoneDesc> = actor.bones.iter().map(|b| BoneDesc { name: b.name.clone(), parent: b.parent as i32, local: b.local }).collect();
    let skeleton = Skeleton::actor(&bones);
    let guns: Vec<Gun> = bfe_content::derived::droideka_guns(actor, def.get("weapons"))
        .into_iter()
        .map(|g| Gun {
            bone: g.bone,
            barrels: match g.tips {
                Some(tips) => tips.into_iter().map(|t| Barrel::Marker(bfe_sim::rig::three::Vector3 { x: t.x, y: t.y, z: t.z })).collect(),
                None => vec![Barrel::GunBone],
            },
        })
        .collect();
    // The roll centre and pivot come from the unit's animator each tick.
    let rig = DroidekaRig::new(&skeleton, &guns, bfe_sim::rig::three::Vector3::default());
    Ok(DroidekaParts { def: d, clips: Rc::new(ModeClips { by_mode }), rig: Some(Box::new(rig)) })
}

/// The rig's barrels' world matrices, as the capture writes them.
fn muzzles(rig: &DroidekaRig) -> Vec<Vec<[f64; 16]>> {
    rig.barrels.iter().map(|b| b.iter().map(|&n| rig.tree.node(n).matrix_world.elements).collect()).collect()
}

fn check_muzzles(frame: &Frame, captured: V, rig: &DroidekaRig) -> Result<(), Mismatch> {
    let mine = muzzles(rig);
    let theirs: Vec<Vec<V>> = captured.items().map(|g| g.items().collect()).collect();
    if theirs.len() != mine.len() || theirs.iter().zip(&mine).any(|(a, b)| a.len() != b.len()) {
        return Err(Mismatch(format!("{}: the rig's barrels: the capture's {}, the port's {:?}", frame.place(), captured.show(), mine.iter().map(|g| g.len()).collect::<Vec<_>>())));
    }
    for (g, (a, b)) in theirs.iter().zip(&mine).enumerate() {
        for (k, (m, e)) in a.iter().zip(b).enumerate() {
            let Some(Node::Mat4(elements)) = m.node() else { return Err(Mismatch(format!("{}: barrel {g}.{k} not a matrix", frame.place()))) };
            for i in 0..16 {
                if elements[i].to_bits() != e[i].to_bits() && !(elements[i].is_nan() && e[i].is_nan()) {
                    return Err(Mismatch(format!("{}: barrel {g}.{k} matrixWorld[{i}]: the capture's {} | the port's {}", frame.place(), elements[i], e[i])));
                }
            }
        }
    }
    Ok(())
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
    let mut contents = Contents::new();
    let report = replay_corpus(&corpus, "Unit.tick", |capture, frame| outcome(unit_tick(capture, frame, &mut contents))).unwrap();
    println!("{}", report.summary());
    report.assert_clean();
}
