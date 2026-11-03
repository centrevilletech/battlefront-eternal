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

use bfe_sim::gen::{Replay, Unit, UnitBody};
use bfe_sim::pose::Clip;
use bfe_sim::rig::droideka::{Barrel, DroidekaRig, Gun, CLIPS};
use bfe_sim::rig::skeleton::{BoneDesc, Skeleton};
use bfe_sim::units::droideka::{DroidekaDef, DroidekaParts, ModeClips};
use std::collections::HashMap;
use std::rc::Rc;
use three::Vector3;
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
            "aimOverride" => unit.aim_override = from.aim_override,
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
const RESET: &[&str] = &["combatDead", "aimOverride", "mounted", "pivot", "gunUnderwater"];

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

    fn combat_reset(&mut self, _id: u32, unit: &mut Unit) {
        let Some(call) = self.call("combat.reset") else { return };
        self.after(call, unit, RESET);
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
                node.copy_quaternion(&three::Quaternion { x: q[0], y: q[1], z: q[2], w: q[3] });
                node.position = three::Vector3 { x: pos[0], y: pos[1], z: pos[2] };
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
                Some(tips) => tips.into_iter().map(|t| Barrel::Marker(three::Vector3 { x: t.x, y: t.y, z: t.z })).collect(),
                None => vec![Barrel::GunBone],
            },
        })
        .collect();
    // The roll centre and pivot come from the unit's animator each tick.
    let rig = DroidekaRig::new(&skeleton, &guns, three::Vector3::default());
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

// ---- The registry's queries (units.raycast, units.query) and the separation.

use bfe_sim::gen::{Droideka, Half, Held, Member, Posture, ShapePart, ShapeType, Soldier, Units};
use bfe_sim::store::{Nullable, Ref, Slab};
use bfe_sim::units::registry::{Exclude, UnitHit};
use bfe_sim::units::separate::{separate, Pushers, Scope};

fn num_or(v: V, d: f64) -> f64 {
    v.opt_num().unwrap_or(d)
}

fn shape_of(v: V) -> Result<HitShape, String> {
    let center = v.get("center").vec3().ok_or("a shape without a centre")?;
    let half_size = v.get("halfSize").vec3().ok_or("a shape without halfSize")?;
    // `half` is a number, or the vector halfSize is (the same object).
    let half = match v.get("half").vec3() {
        Some(h) => Half::Vec3(h),
        None => Half::F64(v.get("half").num()),
    };
    Ok(HitShape {
        r#type: ShapeType::NAMES.iter().position(|n| Some(*n) == v.get("type").str()).and_then(|i| ShapeType::from_u8(i as u8)).ok_or("a shape of no known type")?,
        part: if v.get("part").str() == Some("head") { ShapePart::Head } else { ShapePart::Body },
        center,
        radius: v.get("radius").num(),
        half,
        yaw: v.get("yaw").num(),
        half_size,
    })
}

/// A unit as a query reads it (the capture's hitValue), in the store's form.
fn hit_unit(v: V) -> Result<(u32, Unit), String> {
    let id = v.get("id").num() as u32;
    let posture = match v.get("posture").str() {
        Some("crouch") => Posture::Crouch,
        Some("prone") => Posture::Prone,
        _ => Posture::Stand,
    };
    let air = match v.get("air").str() {
        Some("jump") => Some(bfe_sim::gen::Air::Jump),
        Some("fall") => Some(bfe_sim::gen::Air::Fall),
        Some("thrown") => Some(bfe_sim::gen::Air::Thrown),
        Some("jet") => Some(bfe_sim::gen::Air::Jet),
        _ => None,
    };
    let position = v.get("position").vec3().ok_or("no position")?;
    let walker = v.get("walker").truthy();
    let soldier = if walker {
        UnitBody::Droideka(Droideka { position, aim_yaw: v.get("aimYaw").num(), mode: if v.get("rolled").truthy() { 12.0 } else { 0.0 }, ball_radius: v.get("ballRadius").num(), ..Default::default() })
    } else {
        UnitBody::Soldier(Soldier { position, aim_yaw: v.get("aimYaw").num(), posture, dive: v.get("dive").num(), air, ..Default::default() })
    };
    let shapes = v.get("shapes").items().map(shape_of).collect::<Result<Vec<_>, _>>()?;
    let rider_hits = if v.get("riderHits").is_nullish() { None } else { Some(v.get("riderHits").items().map(shape_of).collect::<Result<Vec<_>, _>>()?) };
    let unit = Unit {
        alive: v.get("alive").truthy(),
        dead: v.get("dead").truthy(),
        removed: v.get("removed").truthy(),
        walker,
        root_scale: v.get("rootScale").num(),
        seat: if v.get("seat").truthy() { Some(Ref::new(0, 0)) } else { None },
        held: if v.get("held").truthy() { Nullable::Value(Held { prop: Ref::new(0, 0), since: 0.0, frame: Nullable::Value([0.0; 16]), time: None }) } else { Nullable::Undefined },
        tick_count: v.get("tickCount").num(),
        spheres_tick: v.get("spheresTick").num(),
        shape_count: v.get("shapeCount").num(),
        bounds: Bounds { center: v.get("bounds").get("center").vec3().ok_or("no bounds")?, radius: v.get("bounds").get("radius").num() },
        shapes,
        rider_hits,
        soldier,
        ..Default::default()
    };
    // Which array its hit shapes are follows from their count (units.mjs).
    let rider = v.get("hitSet").str() == Some("riderHits");
    if rider != unit.rider_set() && !(unit.shape_count == 0.0) {
        return Err(format!("unit {id}: hitSet {} with {} shapes, which its count does not tell", v.get("hitSet").show(), unit.shape_count));
    }
    Ok((id, unit))
}

/// The registry's members and units as a query found them.
fn registry_of(members: V) -> Result<(Units, Slab<Unit>, HashMap<u32, V>), String> {
    let mut registry = Units::default();
    let mut units = Slab::default();
    let mut targets = HashMap::new();
    for m in members.items() {
        if !m.get("unit").is_nullish() {
            let (id, unit) = hit_unit(m.get("unit"))?;
            units.insert_at(id, 0, unit);
            registry.list.push(Member::Unit(Ref::new(id, 0)));
        } else {
            let t = m.get("target");
            let id = t.get("id").num() as u32;
            registry.list.push(match t.get("target").str() {
                Some("vehicle") => Member::VehicleTarget(Ref::new(id, 0)),
                Some("turret") => Member::TurretTarget(Ref::new(id, 0)),
                _ => Member::ObjectiveTarget(Ref::new(id, 0)),
            });
            targets.insert(id, t);
        }
    }
    Ok((registry, units, targets))
}

/// A query's or raycast's world: seats' exposure, riders' and held bodies'
/// shapes (as the frame's end has them: written once a tick by the call
/// that made them), and targets' spheres, served in order.
struct QueryWorld<'f> {
    frame: &'f Frame,
    log: Log<'f>,
    before: HashMap<u32, V<'f>>,
    after: HashMap<u32, V<'f>>,
    targets: HashMap<u32, V<'f>>,
    error: Option<Mismatch>,
}

impl QueryWorld<'_> {
    fn fail(&mut self, what: String) {
        if self.error.is_none() {
            self.error = Some(Mismatch(format!("{}: {what}", self.frame.place())));
        }
    }

    fn shapes_after(&mut self, id: u32, shapes: &mut [HitShape]) -> usize {
        let Some(a) = self.after.get(&id).copied() else { return 0 };
        for (i, s) in a.get("riderHits").items().enumerate().take(shapes.len()) {
            match shape_of(s) {
                Ok(s) => shapes[i] = s,
                Err(e) => self.fail(e),
            }
        }
        if a.get("hitSet").str() == Some("riderHits") { a.get("shapeCount").num() as usize } else { 0 }
    }
}

impl UnitsWorld for QueryWorld<'_> {
    fn cast(&mut self, _o: &Vector3, _d: &Vector3, _f: f64) -> Option<CastHit> {
        self.fail("a cast in a query".into());
        None
    }
    fn gather(&mut self, _id: u32, _c: &Vector3, _r: f64, _cache: usize, _out: &mut Vec<GatherPolygon>) {
        self.fail("a gather in a query".into());
    }
    fn seat_exposed(&mut self, id: u32, _unit: &Unit) -> bool {
        self.before.get(&id).is_some_and(|u| u.get("seatExposed").truthy())
    }
    fn rider_shapes(&mut self, id: u32, _unit: &Unit, shapes: &mut [HitShape]) -> usize {
        self.shapes_after(id, shapes)
    }
    fn held_shapes(&mut self, id: u32, _unit: &Unit, shapes: &mut [HitShape]) -> usize {
        self.shapes_after(id, shapes)
    }
    fn target_alive(&mut self, id: u32) -> bool {
        self.targets.get(&id).is_some_and(|t| t.get("alive").truthy())
    }
    fn target_dead(&mut self, id: u32) -> bool {
        self.targets.get(&id).is_some_and(|t| t.get("dead").truthy())
    }
    fn target_sphere(&mut self, id: u32) -> Bounds {
        let Some(t) = self.targets.get(&id).copied() else { return Bounds::default() };
        let label = match t.get("target").str() {
            Some("vehicle") => "vehicle.sphere",
            Some("turret") => "turret.sphere",
            _ => "building.sphere",
        };
        let call = match self.log.call(label) {
            Ok(c) => c,
            Err(e) => {
                self.fail(e.0);
                return Bounds::default();
            }
        };
        let center = call.out(0).vec3().unwrap_or([f64::NAN; 3]);
        let radius = if label == "vehicle.sphere" { t.get("boundRadius").num() } else { call.result().num() };
        Bounds { center, radius }
    }
    fn move_soldier(&mut self, _id: u32, _unit: &mut Unit, _h: f64) {}
    fn damage(&mut self, _id: u32, _unit: &mut Unit, _amount: f64) {}
    fn aim(&mut self, _id: u32, _unit: &mut Unit, _h: f64) {}
    fn has_remote_step(&mut self, _id: u32, _unit: &Unit) -> bool {
        false
    }
    fn remote_step(&mut self, _id: u32, _unit: &mut Unit, _h: f64, _active: bool) {}
    fn combat_reset(&mut self, _id: u32, _unit: &mut Unit) {}
    fn combat_update(&mut self, _id: u32, _unit: &mut Unit, _h: f64, _stick: f64) {}
    fn flush_muzzles(&mut self, _id: u32, _unit: &mut Unit) {}
    fn clear_charge(&mut self, _id: u32, _unit: &mut Unit) {}
    fn death_blast(&mut self, _id: u32, _unit: &mut Unit) {}
    fn blow_apart(&mut self, _id: u32, _unit: &mut Unit, _health: f64) {}
    fn let_go_of_character(&mut self, _id: u32, _unit: &mut Unit) {}
    fn release(&mut self, _id: u32, _unit: &Unit) {}
}

/// The member a handle in a query's arguments or answer names: the
/// capture names every member of the registry, targets too, by a `unit`
/// handle with its registry id.
fn member_of(v: V, targets: &HashMap<u32, V>) -> Option<Member> {
    let id = v.handle_num("unit")? as u32;
    Some(match targets.get(&id).map(|t| t.get("target").str()) {
        None => Member::Unit(Ref::new(id, 0)),
        Some(Some("vehicle")) => Member::VehicleTarget(Ref::new(id, 0)),
        Some(Some("turret")) => Member::TurretTarget(Ref::new(id, 0)),
        Some(_) => Member::ObjectiveTarget(Ref::new(id, 0)),
    })
}

/// Each unit's hit shapes as the frame left them, against the capture's.
fn check_hits(frame: &Frame, units: &Slab<Unit>, after: &HashMap<u32, V>) -> Result<(), Mismatch> {
    for (&id, a) in after {
        let Some(u) = units.get(Ref::new(id, 0)) else { continue };
        let fail = |what: String| Mismatch(format!("{}: unit {id} {what}", frame.place()));
        for (k, mine) in [("tickCount", u.tick_count), ("spheresTick", u.spheres_tick), ("shapeCount", u.shape_count), ("bounds.radius", u.bounds.radius)] {
            let theirs = if k == "bounds.radius" { a.get("bounds").get("radius") } else { a.get(k) };
            check_num(k, theirs, mine).map_err(|e| fail(e.0))?;
        }
        check_vec3("bounds.center", a.get("bounds").get("center"), u.bounds.center).map_err(|e| fail(e.0))?;
        let hits: Vec<V> = if u.rider_set() { a.get("riderHits").items().collect() } else { a.get("shapes").items().collect() };
        if (a.get("hitSet").str() == Some("riderHits")) != u.rider_set() {
            return Err(fail(format!("hitSet {} | rider set {}", a.get("hitSet").show(), u.rider_set())));
        }
        for (i, (theirs, mine)) in hits.iter().zip(u.hit_set()).enumerate().take(u.shape_count as usize) {
            let t = shape_of(*theirs).map_err(fail)?;
            if t != *mine && !(t.center.iter().zip(mine.center).all(|(x, y)| x.to_bits() == y.to_bits())) {
                return Err(fail(format!("shape {i}: the capture's {t:?} | the port's {mine:?}")));
            }
            if format!("{t:?}") != format!("{mine:?}") {
                return Err(fail(format!("shape {i}: the capture's {t:?} | the port's {mine:?}")));
            }
        }
    }
    Ok(())
}

/// A registry's units' values by id.
fn by_id(members: V<'_>) -> HashMap<u32, V<'_>> {
    members.items().filter(|m| !m.get("unit").is_nullish()).map(|m| (m.get("unit").get("id").num() as u32, m.get("unit"))).collect()
}

fn registry_frame(frame: &Frame, raycast: bool) -> Result<Option<String>, Mismatch> {
    let before = frame.before();
    if before.get("skipped").truthy() {
        return Ok(Some("written in part (one of many a tick)".into()));
    }
    let place = |e: String| Mismatch(format!("{}: {e}", frame.place()));
    let (registry, mut units, targets) = registry_of(before.get("members")).map_err(place)?;
    let after_members = frame.after().get("members");
    let mut world = QueryWorld { frame, log: Log::new(frame), before: by_id(before.get("members")), after: by_id(after_members), targets: targets.clone(), error: None };
    // The caller's filter: its answers as captured, each asked about the
    // same member.
    let answers: Vec<(V, bool)> = {
        let f: Vec<V> = frame.after().get("filtered").items().collect();
        f.chunks(2).map(|p| (p[0], p[1].truthy())).collect()
    };
    let mut asked = 0usize;
    let mut filter_error: Option<String> = None;
    let mut filter = |m: Member| -> bool {
        let Some((who, answer)) = answers.get(asked).copied() else {
            filter_error.get_or_insert_with(|| format!("the filter asked {} times, the capture {}", asked + 1, answers.len()));
            return false;
        };
        asked += 1;
        if member_of(who, &targets) != Some(m) {
            filter_error.get_or_insert_with(|| format!("the filter asked about {m:?}, the capture about {}", who.show()));
        }
        answer
    };
    let has_filter = frame.arg(if raycast { 5 } else { 3 }).function().is_some();
    if raycast {
        let (origin, direction) = (vec3(frame.arg(0)).ok_or_else(|| place("no origin".into()))?, vec3(frame.arg(1)).ok_or_else(|| place("no direction".into()))?);
        let far = frame.arg(2).num();
        let ex = frame.arg(3);
        let many: Vec<Member> = ex.items().filter_map(|x| member_of(x, &targets)).collect();
        let exclude = if ex.is_nullish() {
            Exclude::Nobody
        } else if ex.node().is_some() && ex.handle().is_none() {
            Exclude::Many(&many)
        } else {
            member_of(ex, &targets).map_or(Exclude::Nobody, Exclude::One)
        };
        let riders = frame.arg(6).truthy();
        let hit: Option<UnitHit> = registry.raycast(&mut units, &origin, &direction, far, exclude, if has_filter { Some(&mut filter) } else { None }, riders, &mut world);
        if let Some(e) = filter_error.take() {
            return Err(place(e));
        }
        if let Some(e) = world.error.take() {
            return Err(e);
        }
        let r = frame.result();
        match (r.is_nullish(), hit) {
            (true, None) => {}
            (false, Some(h)) => {
                if r.get("unit").handle_num("unit") != Some(h.unit as f64) {
                    return Err(place(format!("hit unit: the capture's {} | the port's {}", r.get("unit").show(), h.unit)));
                }
                check_num("hit distance", r.get("distance"), h.distance).map_err(|e| place(e.0))?;
                check_vec3("hit point", r.get("point"), [h.point.x, h.point.y, h.point.z]).map_err(|e| place(e.0))?;
                check_vec3("hit normal", r.get("normal"), [h.normal.x, h.normal.y, h.normal.z]).map_err(|e| place(e.0))?;
                if r.get("part").str() != Some(ShapePart::NAMES[h.part as usize]) {
                    return Err(place(format!("hit part: the capture's {} | the port's {}", r.get("part").show(), ShapePart::NAMES[h.part as usize])));
                }
            }
            _ => return Err(place(format!("hit: the capture's {} | the port's {hit:?}", r.show()))),
        }
    } else {
        let center = vec3(frame.arg(0)).ok_or_else(|| place("no centre".into()))?;
        let radius = frame.arg(1).num();
        let mut out = Vec::new();
        registry.query(&mut units, &center, radius, &mut out, if has_filter { Some(&mut filter) } else { None }, &mut world);
        if let Some(e) = filter_error.take() {
            return Err(place(e));
        }
        if let Some(e) = world.error.take() {
            return Err(e);
        }
        let theirs: Vec<Option<Member>> = frame.result().items().map(|x| member_of(x, &targets)).collect();
        let mine: Vec<Option<Member>> = out.iter().map(|m| Some(*m)).collect();
        if theirs != mine {
            return Err(place(format!("the query's answer: the capture's {theirs:?} | the port's {mine:?}")));
        }
    }
    if asked != answers.len() {
        return Err(place(format!("the filter asked {asked} times, the capture {}", answers.len())));
    }
    world.log.finish()?;
    let after = by_id(after_members);
    check_hits(frame, &units, &after)?;
    Ok(None)
}

fn separate_frame(frame: &Frame) -> Result<Option<String>, Mismatch> {
    let before = frame.before();
    let place = |e: String| Mismatch(format!("{}: {e}", frame.place()));
    let mut registry = Units::default();
    let mut units: Slab<Unit> = Slab::default();
    for m in before.get("members").items() {
        let id = m.get("id").num() as u32;
        if !m.get("unit").truthy() {
            registry.list.push(Member::ObjectiveTarget(Ref::new(id, 0)));
            continue;
        }
        let position = m.get("position").vec3().ok_or_else(|| place("no position".into()))?;
        let walker = m.get("walker").truthy();
        let soldier = if walker {
            UnitBody::Droideka(Droideka { position, ..Default::default() })
        } else {
            let posture = match m.get("posture").str() {
                Some("crouch") => Posture::Crouch,
                Some("prone") => Posture::Prone,
                _ => Posture::Stand,
            };
            let air = if m.get("air").truthy() { Some(bfe_sim::gen::Air::Jump) } else { None };
            UnitBody::Soldier(Soldier { position, posture, dive: num_or(m.get("dive"), 0.0), air, ..Default::default() })
        };
        let unit = Unit {
            seat: if m.get("seat").truthy() { Some(Ref::new(0, 0)) } else { None },
            held: if m.get("held").truthy() { Nullable::Null.or_held() } else { Nullable::Undefined },
            removed: m.get("removed").truthy(),
            alive: m.get("alive").truthy(),
            dead: m.get("dead").truthy(),
            walker,
            tick_count: m.get("tickCount").num(),
            soldier,
            ..Default::default()
        };
        units.insert_at(id, 0, unit);
        registry.list.push(Member::Unit(Ref::new(id, 0)));
    }
    let camera = before.get("camera").vec3().unwrap_or([0.0; 3]);
    let scope = Scope {
        player: Some(before.get("player").num() as u32),
        player_out: before.get("driver").truthy() || before.get("gunner").truthy() || before.get("playerDead").truthy(),
        global: before.get("global").truthy(),
        camera: Vector3 { x: camera[0], y: camera[1], z: camera[2] },
    };
    separate(&registry, &mut units, &scope, &mut Pushers::default());
    for m in frame.after().get("members").items() {
        if !m.get("unit").truthy() {
            continue;
        }
        let id = m.get("id").num() as u32;
        let u = units.get(Ref::new(id, 0)).unwrap();
        let p = u.position();
        check_vec3(&format!("unit {id} position"), m.get("position"), [p.x, p.y, p.z]).map_err(|e| place(e.0))?;
        check_num(&format!("unit {id} tickCount"), m.get("tickCount"), u.tick_count).map_err(|e| place(e.0))?;
    }
    Ok(None)
}

trait OrHeld {
    fn or_held(self) -> Nullable<Held>;
}
impl OrHeld for Nullable<Held> {
    fn or_held(self) -> Nullable<Held> {
        Nullable::Value(Held { prop: Ref::new(0, 0), since: 0.0, frame: Nullable::Undefined, time: None })
    }
}

#[test]
fn registry_queries_replay_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    for (name, raycast) in [("units.raycast", true), ("units.query", false)] {
        let report = replay_corpus(&corpus, name, |_capture, frame| outcome(registry_frame(frame, raycast))).unwrap();
        println!("{}", report.summary());
        report.assert_clean();
    }
}

#[test]
fn soldiers_are_pushed_apart_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let report = replay_corpus(&corpus, "separate", |_capture, frame| outcome(separate_frame(frame))).unwrap();
    println!("{}", report.summary());
    report.assert_clean();
}

// ---- A unit's spawn, and what combat and the network ask of a body.

/// The unit a frame's state names, read into a fresh store.
fn unit_in(frame: &Frame, v: V, cx: &mut Replay) -> Result<(u32, Unit), Mismatch> {
    let r = Unit::value_in(v, cx, "unit").map_err(|e| Mismatch(format!("{}: reading the unit: {e}", frame.place())))?;
    Ok((r.slot, cx.store.unit.get(r).cloned().unwrap()))
}

/// The unit at the frame's end against the capture's.
fn check_unit(frame: &Frame, cx: &mut Replay, id: u32, unit: &Unit, captured: V) -> Result<(), Mismatch> {
    let mut arena = Arena::default();
    let mine = unit_value(cx, &mut arena, id, unit);
    let mut a = Arena::default();
    let theirs = without(&mut a, captured, &[]);
    match diff(a.view(&theirs), arena.view(&mine)) {
        None => Ok(()),
        Some(d) => Err(Mismatch(format!("{}: unit {d} (capture | port)", frame.place()))),
    }
}

fn spawn_frame(frame: &Frame) -> Result<Option<String>, Mismatch> {
    let before = frame.before();
    let mut cx = Replay::default();
    let (id, mut unit) = unit_in(frame, before.get("unit"), &mut cx)?;
    let (x, y, z, yaw) = (frame.arg(0).num(), frame.arg(1).num(), frame.arg(2).num(), frame.arg(3).num());
    let mut world = TickWorld { frame, log: Log::new(frame), cx: &mut cx, arena: Arena::default(), id, state: before, character_has: false, pool: Vec::new(), error: None };
    unit.spawn(id, x, y, z, yaw, &mut world);
    if let Some(e) = world.error.take() {
        return Err(e);
    }
    world.log.finish()?;
    check_unit(frame, world.cx, id, &unit, frame.after().get("unit"))?;
    Ok(None)
}

fn droideka_of(unit: &mut Unit) -> Option<&mut bfe_sim::gen::Droideka> {
    match &mut unit.soldier {
        UnitBody::Droideka(d) => Some(d),
        UnitBody::Soldier(_) => None,
    }
}

fn body_frame(capture: &Capture, frame: &Frame, contents: &mut Contents) -> Result<Option<String>, Mismatch> {
    let before = frame.before();
    if before.get("unit").is_nullish() {
        return Ok(Some("a body no unit has".into()));
    }
    let place = |e: String| Mismatch(format!("{}: {e}", frame.place()));
    let mut cx = Replay::default();
    let (id, mut unit) = unit_in(frame, before.get("unit"), &mut cx)?;
    let mut log = Log::new(frame);
    match frame.name.as_str() {
        "Droideka.absorb" => {
            let (amount, on_shield) = (frame.arg(0).num(), if frame.arg(1).is_undefined() { frame.arg(0).num() } else { frame.arg(1).num() });
            let d = droideka_of(&mut unit).ok_or_else(|| place("not a droideka".into()))?;
            let mut error = None;
            let left = d.absorb(amount, on_shield, |_, taken| {
                let r = log.call("units.shieldHit").and_then(|c| {
                    if c.arg(0).handle_num("unit") != Some(id as f64) {
                        return Err(Mismatch(format!("units.shieldHit: the capture's unit {}", c.arg(0).show())));
                    }
                    check_num("units.shieldHit amount", c.arg(1), taken)
                });
                if let Err(e) = r {
                    error.get_or_insert(e);
                }
            });
            if let Some(e) = error {
                return Err(e);
            }
            check_num("absorb's answer", frame.result(), left).map_err(|e| place(e.0))?;
        }
        "Droideka.toggleShield" => droideka_of(&mut unit).ok_or_else(|| place("not a droideka".into()))?.toggle_shield(),
        "Droideka.look" => {
            let d = droideka_of(&mut unit).ok_or_else(|| place("not a droideka".into()))?;
            d.look(frame.arg(0).num(), frame.arg(1).num(), num_or(frame.arg(2), 1.0), frame.arg(3).truthy());
        }
        "Soldier.lookRaw" => {
            let UnitBody::Soldier(s) = &mut unit.soldier else { return Err(place("not a soldier".into())) };
            s.look_raw(frame.arg(0).num(), frame.arg(1).num(), num_or(frame.arg(2), 1.0), frame.arg(3).truthy());
        }
        "Droideka.nextGun" => {
            let content = contents.of(capture).map_err(place)?;
            let Some(content) = content else { return Ok(Some("needs BFE_CONTENT_DIR for the gun rig".into())) };
            let u = before.get("unit");
            let (side, class) = (u.get("side").str().unwrap_or(""), u.get("className").str().unwrap_or(""));
            let mut parts = droideka_parts(&content, side, class).map_err(place)?;
            let rig = parts.rig.as_mut().unwrap();
            // The rig as this tick's sampleMuzzles left it: each barrel's
            // world matrix is what nextGun reads.
            let d = droideka_of(&mut unit).ok_or_else(|| place("not a droideka".into()))?;
            let guns_before = (d.gun, d.last_gun.clone());
            let _ = guns_before;
            let mine = d.next_gun(rig).is_some();
            let sel = d.last_gun.clone().ok_or_else(|| place("no gun chosen".into()))?;
            let m = before.get("muzzles").at(sel.index as usize).at(sel.barrel as usize);
            let Some(Node::Mat4(e)) = m.node() else { return Err(place(format!("no barrel {sel:?} in the capture"))) };
            if frame.result().is_nullish() == mine {
                return Err(place(format!("nextGun's answer: the capture's {}, the port {}", frame.result().show(), if mine { "a point" } else { "none" })));
            }
            check_vec3("nextGun's point", frame.result(), [e[12], e[13], e[14]]).map_err(|e| place(e.0))?;
        }
        other => return Ok(Some(format!("no replay for {other}"))),
    }
    log.finish()?;
    check_unit(frame, &mut cx, id, &unit, frame.after().get("unit"))?;
    Ok(None)
}

#[test]
fn spawns_and_bodies_replay_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let report = replay_corpus(&corpus, "Unit.spawn", |_capture, frame| outcome(spawn_frame(frame))).unwrap();
    println!("{}", report.summary());
    report.assert_clean();
    let mut contents = Contents::new();
    for name in ["Droideka.absorb", "Droideka.toggleShield", "Droideka.nextGun", "Droideka.look", "Soldier.lookRaw"] {
        let report = replay_corpus(&corpus, name, |capture, frame| outcome(body_frame(capture, frame, &mut contents))).unwrap();
        println!("{}", report.summary());
        report.assert_clean();
    }
}

// ---- A soldier's tick in the water.

use bfe_sim::gen::WaterContact;
use bfe_sim::units::water::{WaterEffects, WaterState};

fn contact_of(v: V) -> WaterContact {
    let n = |k: &str| v.get(k).opt_num();
    WaterContact { gun_under: v.get("gunUnder").opt_bool(), under: n("under"), wake_clock: n("wakeClock"), splash_clock: n("splashClock") }
}

struct Splashes(Vec<Vector3>);
impl WaterEffects for Splashes {
    fn ripple(&mut self, _at: &Vector3, _life: f64, _radius: f64, _velocity: &Vector3) {}
    fn splash(&mut self, at: &Vector3) {
        self.0.push(*at);
    }
}

fn water_frame(frame: &Frame) -> Result<Option<String>, Mismatch> {
    let before = frame.before();
    let place = |e: String| Mismatch(format!("{}: {e}", frame.place()));
    let water = WaterState {
        height: before.get("height").num(),
        cell: 1.0,
        offset: [0.0, 0.0],
        grid: 1.0,
        cells: Default::default(),
        bobbing: false,
        simulation_time: 0.0,
        physics_wave: before.get("physicsWave").num(),
    };
    let mut contact = contact_of(before.get("contact"));
    let v = |i: usize| vec3(frame.arg(i)).ok_or_else(|| place(format!("argument {i} is no vector")));
    let (feet, head, gun, velocity) = (v(2)?, v(3)?, v(4)?, v(5)?);
    let mut log = Log::new(frame);
    let mut error: Option<Mismatch> = None;
    let mut over = |x: f64, z: f64| -> bool {
        match log.call("water.over").and_then(|c| check_num("over x", c.arg(0), x).and_then(|_| check_num("over z", c.arg(1), z)).map(|_| c.result().truthy())) {
            Ok(b) => b,
            Err(e) => {
                error.get_or_insert(e);
                false
            }
        }
    };
    let mut splashes = Splashes(Vec::new());
    let damage = water.soldier_step_over(frame.arg(0).num(), &mut contact, &feet, &head, &gun, &velocity, &mut over, &mut splashes);
    if let Some(e) = error {
        return Err(e);
    }
    log.finish()?;
    check_num("damage", frame.result(), damage).map_err(|e| place(e.0))?;
    let after = frame.after();
    let theirs = contact_of(after.get("contact"));
    let same = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(x), Some(y)) => x.to_bits() == y.to_bits(),
        (None, None) => true,
        _ => false,
    };
    if theirs.gun_under != contact.gun_under || !same(theirs.under, contact.under) || !same(theirs.wake_clock, contact.wake_clock) || !same(theirs.splash_clock, contact.splash_clock) {
        return Err(place(format!("contact: the capture's {theirs:?} | the port's {contact:?}")));
    }
    let recorded: Vec<V> = after.get("splashes").items().collect();
    if recorded.len() != splashes.0.len() {
        return Err(place(format!("splashes: the capture's {} | the port's {}", recorded.len(), splashes.0.len())));
    }
    for (r, s) in recorded.iter().zip(&splashes.0) {
        check_vec3("splash", *r, [s.x, s.y, s.z]).map_err(|e| place(e.0))?;
    }
    Ok(None)
}

#[test]
fn soldiers_in_the_water_replay_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let report = replay_corpus(&corpus, "WaterState.soldierStep", |_capture, frame| outcome(water_frame(frame))).unwrap();
    println!("{}", report.summary());
    report.assert_clean();
}

// ---- Each body's class numbers, made from its class as the factory makes
// them, against every unit a capture wrote in full.

fn class_frame(capture: &Capture, frame: &Frame, contents: &mut Contents) -> Result<Option<String>, Mismatch> {
    let before = frame.before();
    if before.get("skipped").truthy() {
        return Ok(Some("written in part".into()));
    }
    let place = |e: String| Mismatch(format!("{}: {e}", frame.place()));
    let Some(content) = contents.of(capture).map_err(place)? else { return Ok(Some("needs BFE_CONTENT_DIR".into())) };
    let mut cx = Replay::default();
    let (_id, unit) = unit_in(frame, before.get("unit"), &mut cx)?;
    let u = before.get("unit");
    let (side, class) = (u.get("side").str().unwrap_or(""), u.get("className").str().unwrap_or(""));
    let def = content.battle.side(side).and_then(|s| s.units.get(class)).ok_or_else(|| place(format!("no class {side}/{class}")))?;
    let ceiling = content.battle.mission().and_then(|m| m.get("maxFlyHeight")).and_then(|v| v.as_f64()).unwrap_or(200.0);
    let model = def.get("model").and_then(|m| m.as_str()).unwrap_or("");
    let actor = content.actor(&format!("{side}/{model}"));
    match &unit.soldier {
        UnitBody::Soldier(s) => {
            // buildBody's get-up and dive times, from the body's clips.
            let clips = content.body_clips.iter().find(|(k, _)| k == &format!("{side}/{model}")).map(|(_, c)| c.clone());
            let duration = |name: &str| clips.as_ref().and_then(|c| c.get(name)).map(|c| c.duration);
            let getup = duration("human_rifle_prone_getup_stand").unwrap_or(0.0);
            let dive = bfe_content::DIVE_LENGTH * duration("human_rifle_diveforward").unwrap_or(0.0);
            let mine = bfe_sim::units::class::soldier_body(def, ceiling, getup, dive);
            let pairs = [
                ("maxSpeed", s.max_speed, mine.max_speed),
                ("maxStrafe", s.max_strafe, mine.max_strafe),
                ("maxJump", s.max_jump, mine.max_jump),
                ("maxTurn", s.max_turn, mine.max_turn),
                ("maxPitch", s.max_pitch, mine.max_pitch),
                ("accel", s.accel, mine.accel),
                ("pitchLimits[0]", s.pitch_limits[0], mine.pitch_limits[0]),
                ("pitchLimits[1]", s.pitch_limits[1], mine.pitch_limits[1]),
                ("jet.jump", s.jet.jump, mine.jet.jump),
                ("jet.push", s.jet.push, mine.jet.push),
                ("jet.recharge", s.jet.recharge, mine.jet.recharge),
                ("jet.cost", s.jet.cost, mine.jet.cost),
                ("jet.initial", s.jet.initial, mine.jet.initial),
                ("jet.minBorder", s.jet.min_border, mine.jet.min_border),
                ("maxFlyHeight", s.max_fly_height, mine.max_fly_height),
                ("getupTime", s.getup_time, mine.getup_time),
                ("diveTime", s.dive_time, mine.dive_time),
            ];
            for (k, theirs, ours) in pairs {
                if theirs.to_bits() != ours.to_bits() {
                    return Err(place(format!("{class}.{k}: the capture's {theirs} | the port's {ours}")));
                }
            }
            if s.jet.hover != mine.jet.hover || s.collision_rule != mine.collision_rule {
                return Err(place(format!("{class}: jet.hover or the collision rule: the capture's {:?} {:?} | the port's {:?} {:?}", s.jet.hover, s.collision_rule, mine.jet.hover, mine.collision_rule)));
            }
        }
        UnitBody::Droideka(d) => {
            let root = actor.and_then(|a| a.bones.iter().position(|b| b.name.to_lowercase() == "dummyroot")).map_or(-1.0, |i| i as f64);
            let mine = bfe_sim::units::class::droideka_body(def, root, ceiling);
            let pairs = [
                ("maxTurn", d.max_turn, mine.max_turn),
                ("pitchLimits[0]", d.pitch_limits[0], mine.pitch_limits[0]),
                ("pitchLimits[1]", d.pitch_limits[1], mine.pitch_limits[1]),
                ("speed", d.speed, mine.speed),
                ("turnThreshold", d.turn_threshold, mine.turn_threshold),
                ("ballRadius", d.ball_radius, mine.ball_radius),
                ("maxSpin", d.max_spin, mine.max_spin),
                ("maxBallSlope", d.max_ball_slope, mine.max_ball_slope),
                ("eyeHeight", d.eye_height, mine.eye_height),
                ("rootIndex", d.root_index, mine.root_index),
                ("maxFlyHeight", d.max_fly_height, mine.max_fly_height),
            ];
            for (k, theirs, ours) in pairs {
                if theirs.to_bits() != ours.to_bits() {
                    return Err(place(format!("{class}.{k}: the capture's {theirs} | the port's {ours}")));
                }
            }
            if d.collision_rule != mine.collision_rule {
                return Err(place(format!("{class}: the collision rule: the capture's {:?} | the port's {:?}", d.collision_rule, mine.collision_rule)));
            }
        }
    }
    Ok(None)
}

#[test]
fn bodies_are_made_from_their_classes_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let mut contents = Contents::new();
    let report = replay_corpus(&corpus, "Unit.tick", |capture, frame| outcome(class_frame(capture, frame, &mut contents))).unwrap();
    println!("class numbers: {}", report.summary());
    report.assert_clean();
}

// ---- The registry's team questions.

use bfe_sim::units::registry::TeamTable;

fn teams_of(v: V) -> Option<TeamTable> {
    if v.is_nullish() {
        return None;
    }
    let nums = |x: V| -> Vec<f64> { x.items().map(|n| n.opt_num().unwrap_or(f64::NAN)).collect() };
    Some(TeamTable {
        relations: v.get("relations").items().map(nums).collect(),
        factions: v.get("factions").items().map(|n| n.opt_num()).collect(),
        capture: v.get("capture").items().map(|n| n.opt_num()).collect(),
    })
}

fn team_frame(frame: &Frame) -> Result<Option<String>, Mismatch> {
    let before = frame.before();
    if before.get("skipped").truthy() {
        return Ok(Some("written in part (one of many a tick)".into()));
    }
    let place = |e: String| Mismatch(format!("{}: {e}", frame.place()));
    let mut strings = bfe_sim::store::Strings::default();
    let mut registry = Units::default();
    for e in before.get("alliances").items() {
        let key = strings.intern_str(e.at(0).str().ok_or_else(|| place("an alliance's key is no string".into()))?);
        registry.alliances.set(key, e.at(1).num());
    }
    let teams = teams_of(before.get("teams"));
    let a = frame.arg(0).num();
    let result = frame.result();
    match frame.name.as_str() {
        "units.relation" => check_num("relation", result, registry.relation(a, frame.arg(1).num(), teams.as_ref(), &mut strings)),
        "units.captureTeam" => check_num("captureTeam", result, registry.capture_team(a, teams.as_ref())),
        "units.locals" => bfe_sim::store::replay::boolean(result, "locals").map_err(Mismatch).and_then(|b| if b == registry.locals(a, teams.as_ref()) { Ok(()) } else { Err(Mismatch(format!("locals: the capture's {b}"))) }),
        other => return Ok(Some(format!("no replay for {other}"))),
    }
    .map_err(|e| place(e.0))?;
    Ok(None)
}

#[test]
fn team_questions_replay_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    for name in ["units.relation", "units.locals", "units.captureTeam"] {
        let report = replay_corpus(&corpus, name, |_capture, frame| outcome(team_frame(frame))).unwrap();
        println!("{}", report.summary());
        report.assert_clean();
    }
}
