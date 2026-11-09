//! The live-object owners' construction (game.js 3128-3160 headless):
//! F4Live (f4-live.js), then F4Battle inside F4Tail (f4-battle.js,
//! f4-tail.js), the AI's activation, and the viewpoint's player binding.
//! Each export runs against the world as the page's constructors ran it,
//! with every answer (handles, descriptors) fed back before the next call.
//! Definitions and labels come from the loaded content; counts, refs and
//! roster facts from the world the earlier stages built.
use super::super::super::{dispatch::{self, Returned}, sequence, AdmitError, Stage};
use super::super::{ai, combat::{self, DefinitionKind}, Call, Calls, Cx};
use super::{stock, streams, Facts};
use crate::gen::*;
use crate::store::{DefId, Ref, StrId};
use crate::world::owner::World;
use bfe_content::{js::to_number, value::Value};

const STAGE: Stage = Stage::LiveTail;
fn error(message: impl Into<String>) -> AdmitError { AdmitError::Content { stage: STAGE, message: message.into() } }
fn block(words: usize, put: impl FnOnce(&mut [f64], &mut Vec<f64>)) -> Call {
    let mut call = Call { row: vec![0.0; words], ..Default::default() };
    put(&mut call.row, &mut call.heap); call
}
fn scalar(args: &[f64]) -> Call { Call { args: args.to_vec(), ..Default::default() } }

/// One export as the page makes it: interns and the argument block into
/// the world, the body, its answer recorded for the owners after it.
fn dispatch(world: &mut World, cx: &mut Cx, calls: &mut Calls, name: &'static str, call: Call) -> Result<Returned, AdmitError> {
    sequence::args(world, &call)?;
    let answer = dispatch::call(world, STAGE, name, &call)?;
    dispatch::record(cx, STAGE, name, answer.clone())?;
    cx.refresh_strings(&world.sim.strings);
    calls.push((STAGE, name, call));
    Ok(answer)
}

/// f4-live.js `text`: `String(value ?? '').trim().replace(/\n/g,' ') || null`.
fn text(value: Option<&Value>) -> Result<Option<String>, AdmitError> {
    let s = match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.to_string(),
        Some(Value::Number(n)) => bfe_content::js::number_to_string(*n),
        Some(Value::Bool(b)) => b.to_string(),
        Some(_) => return Err(error("a label is neither text nor a number")),
    };
    let s = s.trim().replace('\n', " ");
    Ok(if s.is_empty() { None } else { Some(s) })
}
/// `(s.HealthType || 'vehicle').toLowerCase()` and F4Live's
/// `String(def.HealthType ?? '').toLowerCase() === 'vehicle'`.
fn health_type(value: Option<&Value>, fallback: &str) -> String {
    match value {
        Some(Value::String(s)) if !s.is_empty() => s.to_lowercase(),
        Some(Value::Number(n)) if *n != 0.0 && !n.is_nan() => bfe_content::js::number_to_string(*n).to_lowercase(),
        Some(Value::Bool(true)) => "true".into(),
        _ => fallback.to_lowercase(),
    }
}

/// F4Live.classes / F4Tail.weapons: the definitions already sent, by identity.
#[derive(Default)]
struct Sent { classes: Vec<Value>, weapons: Vec<Value> }
impl Sent {
    fn has(list: &[Value], value: &Value) -> bool { list.iter().any(|v| v.same(value)) }
}

/// DefTable.id: the class or weapon handle, registering an unsent one.
fn definition_id(cx: &mut Cx, kind: DefinitionKind, def: &Value) -> Result<DefId, AdmitError> {
    let (id, _) = combat::with_bindings(cx, STAGE, |bindings, _| {
        let id = match kind {
            DefinitionKind::Class => bindings.combat.classes.id(def, STAGE)?,
            _ => bindings.combat.weapons.id(def, STAGE)?,
        };
        Ok((id, Vec::new()))
    })?;
    Ok(id)
}

/// F4Live.prepareClass's row (also F4Tail's): the handle first, then the
/// effects and label interned in the generated field order.
fn unit_class(cx: &mut Cx, def: &Value) -> Result<Call, AdmitError> {
    let id = definition_id(cx, DefinitionKind::Class, def)?;
    let captures = match def.get("CapturePosts") { None | Some(Value::Null) => true, Some(Value::Number(n)) => *n != 0.0, Some(_) => true };
    let vehicle = health_type(def.get("HealthType"), "") == "vehicle";
    // `def.HealthGainEffect || 'Powerup_blue'`: a truthy value is the name.
    let effect = |value: Option<&Value>, fallback: &str| -> Result<String, AdmitError> {
        match value {
            Some(Value::String(s)) if !s.is_empty() => Ok(s.to_string()),
            Some(Value::Number(n)) if *n != 0.0 && !n.is_nan() => Ok(bfe_content::js::number_to_string(*n)),
            Some(v) if v.truthy() => Err(error("a gain effect is neither text nor a number")),
            _ => Ok(fallback.to_string()),
        }
    };
    let health = effect(def.get("HealthGainEffect"), "Powerup_blue")?;
    let ammo = effect(def.get("AmmoGainEffect"), "Powerup_red")?;
    let label = text(def.get("displayName"))?;
    let mut call = Call::default();
    let row = F4UnitClass { def: id, captures, vehicle, health_effect: cx.intern(&mut call, &health),
        ammo_effect: cx.intern(&mut call, &ammo), label: label.map(|l| cx.intern(&mut call, &l)) };
    call.row.resize(F4UnitClass::WORDS, 0.0); row.put(&mut call.row, &mut call.heap);
    Ok(call)
}

/// F4Live.prepareWeapon's row for a weapon definition (weapons.js Weapon:
/// levels are the definition and its charges merged over it, the name its
/// class, the channel its own): ammunition per level, the key, the labels.
fn weapon_class(cx: &mut Cx, def: &Value) -> Result<Call, AdmitError> {
    let id = definition_id(cx, DefinitionKind::Weapon, def)?;
    let levels = combat::weapon_levels(def)?;
    let ammunition = levels.iter().map(|level| to_number(level.get("ammo"))).collect();
    let primary = matches!(def.get("channel"), Some(Value::Number(n)) if *n == 0.0);
    let key = def.get("class").and_then(Value::as_str).map(str::to_string);
    let labels = levels.iter().map(|level| {
        let name = level.get("displayName").filter(|v| !matches!(v, Value::Null)).or_else(|| level.get("label"));
        text(name)
    }).collect::<Result<Vec<_>, _>>()?;
    let mut call = Call::default();
    let row = F4WeaponClass { def: id, ammunition, primary, key: key.map(|k| cx.intern(&mut call, &k)),
        labels: labels.into_iter().map(|l| l.map(|l| cx.intern(&mut call, &l))).collect() };
    call.row.resize(F4WeaponClass::WORDS, 0.0); row.put(&mut call.row, &mut call.heap);
    Ok(call)
}

/// prepareClass: the class once, then every authored weapon once.
fn prepare_class(world: &mut World, cx: &mut Cx, calls: &mut Calls, sent: &mut Sent, def: &Value,
    class_export: &'static str, weapon_export: &'static str) -> Result<(), AdmitError> {
    if Sent::has(&sent.classes, def) { return Ok(()); }
    sent.classes.push(def.clone());
    let call = unit_class(cx, def)?;
    dispatch(world, cx, calls, class_export, call)?;
    for weapon in def.get("weapons").and_then(Value::as_array).unwrap_or(&[]) {
        prepare_weapon(world, cx, calls, sent, weapon, weapon_export)?;
    }
    Ok(())
}
fn prepare_weapon(world: &mut World, cx: &mut Cx, calls: &mut Calls, sent: &mut Sent, def: &Value, export: &'static str) -> Result<(), AdmitError> {
    if Sent::has(&sent.weapons, def) { return Ok(()); }
    sent.weapons.push(def.clone());
    let call = weapon_class(cx, def)?;
    dispatch(world, cx, calls, export, call)?;
    Ok(())
}

/// The hulls as the fleet lists them: the live vehicles, then the prepared
/// variants (every registered hull outside the fleet list, in slot order).
fn hulls(world: &World) -> Vec<Ref<Vehicle>> {
    let store = &world.sim.store;
    let mut out = store.vehicles.vehicles.clone();
    out.extend(store.vehicle.iter().map(|(r, _)| r).filter(|r| !store.vehicles.vehicles.contains(r)));
    out
}
/// The vehicle pack's class of a hull, by the hull's name.
fn hull_class<'a>(world: &World, cx: &'a Cx, hull: Ref<Vehicle>) -> Result<&'a Value, AdmitError> {
    let name = world.sim.store.vehicle.get(hull).map(|v| String::from_utf16_lossy(world.sim.strings.units(v.name)))
        .ok_or_else(|| error("a fleet hull is missing from the store"))?;
    cx.battle.vehicle_def.as_ref().and_then(|v| v.get("classes")).and_then(|c| c.get(&name))
        .ok_or_else(|| error(format!("vehicle class {name} is missing")))
}
fn hull_class_row(cx: &mut Cx, hull: Ref<Vehicle>, def: &Value, vehicle: bool) -> Result<Call, AdmitError> {
    let label = text(def.get("label"))?;
    let mut call = Call::default();
    // The page's Vehicle has no `side`: `vehicle.side ?? null` is null.
    let row = F4HullClass { hull, vehicle, side: None, label: label.map(|l| cx.intern(&mut call, &l)) };
    call.row.resize(F4HullClass::WORDS, 0.0); row.put(&mut call.row, &mut call.heap);
    Ok(call)
}

/// Every side's unit definitions in authored order, each object once: the
/// `definitions` set F4Live builds (the built bodies' and the local class
/// are among them, so their later prepareUnit adds nothing).
fn side_definitions(cx: &Cx) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for side in &cx.battle.sides {
        for (_, def) in side.units.as_object().map(|o| o.entries().collect::<Vec<_>>()).unwrap_or_default() {
            if !out.iter().any(|v| v.same(def)) { out.push(def.clone()); }
        }
    }
    out
}

/// The sole F4Battle construction import (KIND_BATTLE): the authored
/// battle tables the page built with `new Battle(mission)`, carried into
/// the store when no earlier owner wrote them. The roster's Characters
/// stay what the AI's construction committed.
fn ensure_battle(world: &mut World, cx: &Cx) -> Result<(), AdmitError> {
    if !world.sim.store.battle.team_ids.is_empty() { return Ok(()); }
    let characters = world.sim.store.battle.characters.clone();
    world.sim.store.battle = super::initial(cx.battle, characters)?;
    Ok(())
}

/// game.js spawnPosts at the human's first spawn: the player's team's
/// active posts with a spawn path, else every active post with one.
/// Mobile posts have no spawn path, so only the command posts count.
fn spawn_posts(world: &World, cx: &Cx, team: f64) -> Vec<Ref<Post>> {
    let spawns = cx.battle.world.get("spawns");
    let descs = cx.battle.world.get("commandPosts").and_then(Value::as_array).unwrap_or(&[]);
    let with_path = |i: usize| descs.get(i).and_then(|d| d.get("spawnPath")).and_then(Value::as_str)
        .and_then(|p| spawns.and_then(|s| s.get(p))).is_some_and(Value::truthy);
    let posts = &world.sim.store.command_posts.posts;
    let active = |r: &Ref<Post>| world.sim.store.post.get(*r).is_some_and(|p| p.active);
    let own: Vec<Ref<Post>> = posts.iter().enumerate()
        .filter(|(i, r)| world.sim.store.post.get(**r).is_some_and(|p| p.team == team) && active(r) && with_path(*i))
        .map(|(_, r)| *r).collect();
    if !own.is_empty() { return own; }
    posts.iter().enumerate().filter(|(i, r)| active(r) && with_path(*i)).map(|(_, r)| *r).collect()
}

/// The HumanSpawn import (KIND_HUMAN_SPAWN) as createGame left it: the
/// policy's automatic choice, and after `humanSpawn.spawned(UNIT, post)`
/// an alive, once-spawned human at the first spawn post (spawnIndex 0);
/// with the spawn screen he is still unspawned.
fn human_spawn(world: &mut World, cx: &mut Cx) -> Result<(), AdmitError> {
    let policy = cx.battle.policy.clone();
    let mut call = Call::default();
    let class_name = cx.intern(&mut call, &policy.class_name);
    let auto = (!policy.screen).then(|| HumanAuto { team: policy.team, class_name, post: policy.post.as_deref().map(|p| cx.intern(&mut call, p)) });
    let post = if policy.screen { None } else {
        spawn_posts(world, cx, policy.team).first().and_then(|r| world.sim.store.post.get(*r)).map(|p| p.name)
    };
    if !call.strings.is_empty() { return Err(error("the human spawn names a class or post the world never interned")); }
    world.sim.store.human_spawn = HumanSpawn { auto, class_name, post,
        phase: if policy.screen { HumanPhase::Unspawned } else { HumanPhase::Alive },
        can_switch_teams: true, spawned_once: !policy.screen, dead_time: 0.0, pending: None, ended: false };
    Ok(())
}

/// ai_mission_teams's capture table for the roster's teams (ai/owned/entry.rs):
/// `units.teams.capture[team]`, one value per team row.
fn capture_teams(world: &World) -> Vec<f64> {
    world.sim.store.spawning.teams.iter().map(|team| {
        let first = team.relations[1] > 0; let second = team.relations[2] > 0;
        if team.index <= 2.0 { team.index } else if first { if second { 0.0 } else { 1.0 } } else if second { 2.0 } else { team.index }
    }).collect()
}

/// The recorded path points of a post's spawn path (`paths[post.desc.spawnPath] ?? null`).
fn raw_path(cx: &Cx, index: usize) -> Result<Option<Vec<[f64; 4]>>, AdmitError> {
    let descs = cx.battle.world.get("commandPosts").and_then(Value::as_array).unwrap_or(&[]);
    let Some(name) = descs.get(index).and_then(|d| d.get("spawnPath")).and_then(Value::as_str) else { return Ok(None) };
    let Some(path) = cx.battle.world.get("spawns").and_then(|s| s.get(name)).filter(|v| !matches!(v, Value::Null)) else { return Ok(None) };
    path.as_array().ok_or_else(|| error("a spawn path is not an array"))?.iter().map(|p| {
        let p = p.as_array().ok_or_else(|| error("a spawn point is not an array"))?;
        Ok(std::array::from_fn(|i| to_number(p.get(i))))
    }).collect::<Result<Vec<_>, _>>().map(Some)
}

/// One class's grip rows over every animated prop born so far; each row's
/// clip ids register the bank's clips the kernel has not copied yet, right
/// before the row, as its `put` does.
fn grip_class(world: &mut World, cx: &mut Cx, calls: &mut Calls, name: &str, id: DefId, births: &[(F4Handle, Vec<String>)]) -> Result<(), AdmitError> {
    for (handle, grabs) in births {
        let mut registrations = Vec::new();
        let row = super::animated::grip(world, cx, name, id, handle, grabs, &mut registrations)?;
        calls.extend(registrations.into_iter().map(|(_, name, call)| (STAGE, name, call)));
        dispatch(world, cx, calls, "f4_grip_class", block(F4GripClass::WORDS, |r, h| row.put(r, h)))?;
    }
    Ok(())
}

/// `calls` receives every call as it is dispatched, so a refusal leaves the
/// calls before it for the comparison that found it.
pub fn run(world: &mut World, cx: &mut Cx, calls: &mut Calls) -> Result<(), AdmitError> {
    let mut facts: Facts = stock::facts(world, cx)?;
    // preparedPads over the command vehicles' posts (rows/ai/observe.rs):
    // the scratch for a pad's spawn points holds its largest class record.
    let pads = ai::prepared_pads(world, cx)?;
    let points = pads.iter().map(ai::PadFacts::points).max().unwrap_or(0);
    facts.point_capacity = super::point_capacity(0, std::iter::empty())?.max(u32::try_from(points).map_err(|_| error("a pad has too many spawn points"))?);
    cx.ids.battle = Some(facts.clone());

    // F4Live: the setup, every building's handle, the definitions, the
    // hulls, then the first publication.
    let setup = super::live(&facts)?;
    dispatch(world, cx, calls, "f4_live_init", block(F4LiveSetup::WORDS, |r, h| setup.put(r, h)))?;
    for index in 0..crate::units::f3_props::building_count_in(&world.units) {
        dispatch(world, cx, calls, "f4_building_handle", scalar(&[index as f64]))?;
    }
    let mut live = Sent::default();
    for def in side_definitions(cx) { prepare_class(world, cx, calls, &mut live, &def, "f4_unit_class", "f4_weapon_class")?; }
    let fleet = hulls(world);
    for hull in &fleet {
        let def = hull_class(world, cx, *hull)?.clone();
        let vehicle = health_type(def.get("settings").and_then(|s| s.get("HealthType")), "vehicle") == "vehicle";
        let call = hull_class_row(cx, *hull, &def, vehicle)?;
        dispatch(world, cx, calls, "f4_hull_class", call)?;
    }
    // bindAnimated per animated prop (Pose.refresh's three views, then the
    // birth), then prepareGrips for every soldier class with a loaded body,
    // in the sides' table order; the built bodies' and the local class
    // repeat classes already gripped.
    let mut births: Vec<(F4Handle, Vec<String>)> = Vec::new();
    for (index, instance) in super::animated::instances(cx).into_iter().enumerate() {
        let pose = cx.ids.animated_poses.iter().find(|(i, _)| *i == instance).map(|(_, p)| *p)
            .ok_or_else(|| error("an animated prop has no pose from its construction"))?;
        let body = cx.ids.prop_kernels.iter().find(|(i, look, _)| *i == instance && *look == 0)
            .and_then(|(_, _, kernel)| cx.ids.bodies.iter().find(|(_, k)| k == kernel).map(|(body, _)| *body));
        let mut interns = Call::default();
        let setup = super::animated::birth(world, cx, instance, index, pose, body, &mut interns)?;
        let mut call = block(F4AnimationSetup::WORDS, |r, h| setup.put(r, h));
        call.strings = interns.strings;
        let Returned::Row(words) = dispatch(world, cx, calls, "f4_animated_birth", call)? else { return Err(error("f4_animated_birth answered no handle")) };
        births.push((F4Handle::take(&words, &[]), super::animated::grab_clips(cx, instance)));
    }
    if !births.is_empty() {
        let prepared = stock::prepared_classes(cx);
        let mut gripped: Vec<Value> = Vec::new();
        for side in cx.battle.sides.clone() {
            for (name, def) in side.units.as_object().map(|o| o.entries().collect::<Vec<_>>()).unwrap_or_default() {
                if def.get("base").and_then(Value::as_str) != Some("soldier") || !prepared.iter().any(|p| p == name) { continue; }
                if gripped.iter().any(|g| g.same(def)) { continue; }
                gripped.push(def.clone());
                let id = definition_id(cx, DefinitionKind::Class, def)?;
                grip_class(world, cx, calls, name, id, &births)?;
            }
        }
        // prepareUnit for every built body, then the local one: a class not
        // gripped yet (a droideka, whose base the loop above skips) grips
        // with its own animator's clips.
        let viewpoint = cx.ids.registry.iter().find(|(key, _)| key == "viewpoint").map(|(_, id)| *id as u32);
        let mut bodies: Vec<String> = world.sim.store.units.list.iter().filter_map(|m| match m {
            crate::gen::Member::Unit(r) if Some(r.slot) != viewpoint => world.sim.store.unit.get(*r)
                .map(|u| String::from_utf16_lossy(world.sim.strings.units(u.class_name))),
            _ => None,
        }).collect();
        bodies.push(cx.battle.policy.class_name.clone());
        for name in bodies {
            let Some(def) = cx.battle.sides.iter().find_map(|side| side.units.get(&name).filter(|d| d.truthy()).cloned()) else { continue };
            if gripped.iter().any(|g| g.same(&def)) { continue; }
            gripped.push(def.clone());
            let id = definition_id(cx, DefinitionKind::Class, &def)?;
            grip_class(world, cx, calls, &name, id, &births)?;
        }
    }
    dispatch(world, cx, calls, "f4_live_publish", scalar(&[]))?;

    // F4Battle: the roster's Characters and the Battle are imported, then
    // the setup, the root mirror and one mirror per Character.
    ensure_battle(world, cx)?;
    let characters = world.sim.store.battle.characters.len();
    let setup = super::setup(characters)?;
    dispatch(world, cx, calls, "f4_battle_init", block(F4BattleSetup::WORDS, |r, h| setup.put(r, h)))?;
    dispatch(world, cx, calls, "f4_battle_capacity", scalar(&[0.0]))?;
    for i in 0..characters {
        dispatch(world, cx, calls, "f4_character_handle", scalar(&[i as f64]))?;
        dispatch(world, cx, calls, "f4_battle_capacity", scalar(&[1.0]))?;
    }
    dispatch(world, cx, calls, "f4_battle_publish", scalar(&[]))?;

    // F4Tail: posts, spawning and the human spawn are imported; the setup
    // registers the tail's two streams in its field order.
    if world.sim.store.command_posts.posts.is_empty() && cx.battle.world.get("commandPosts").and_then(Value::as_array).is_some_and(|p| !p.is_empty()) {
        return Err(error("the command posts were not constructed before the tail"));
    }
    human_spawn(world, cx)?;
    facts.random = cx.streams.register(streams::WORLD_RANDOM);
    facts.gameplay = cx.streams.register(streams::GAMEPLAY);
    cx.ids.battle = Some(facts.clone());
    let mut call = Call::default();
    let tail = super::tail(cx.battle, cx.options, &facts, &mut |t| cx.intern(&mut call, t))?;
    call.row.resize(F4TailSetup::WORDS, 0.0); tail.put(&mut call.row, &mut call.heap);
    dispatch(world, cx, calls, "f4_tail_init", call)?;
    dispatch(world, cx, calls, "f4_tail_commands", scalar(&[]))?;
    let mut tail_sent = Sent::default();
    for def in live.classes.clone() { prepare_class(world, cx, calls, &mut tail_sent, &def, "f4_tail_unit_class", "f4_tail_weapon_class")?; }
    // The built bodies' and the local arsenal are these same definitions.
    let registered = combat::registered_definitions(cx, DefinitionKind::Weapon)?;
    for hull in &fleet {
        let def = hull_class(world, cx, *hull)?.clone();
        let call = hull_class_row(cx, *hull, &def, false)?;
        dispatch(world, cx, calls, "f4_tail_hull_class", call)?;
        let seats = crate::vehicles::module::f2::firing::seat_weapon_defs_in(world, *hull);
        for seat in seats {
            for weapon in seat {
                let (_, def) = registered.iter().find(|(id, _)| *id == weapon).ok_or_else(|| error("a seat weapon has no registered definition"))?;
                let def = def.clone();
                prepare_weapon(world, cx, calls, &mut tail_sent, &def, "f4_tail_weapon_class")?;
            }
        }
    }
    // LiveObjects.turrets: every armed building with a weapon, in prop order.
    if let Some(props) = cx.battle.props.clone() {
        let models = props.get("models");
        for desc in props.get("instances").and_then(Value::as_array).unwrap_or(&[]) {
            if desc.get("base").and_then(Value::as_str) != Some("armedbuilding") { continue; }
            if !desc.get("model").and_then(Value::as_str).and_then(|m| models.and_then(|ms| ms.get(m))).is_some_and(Value::truthy) { continue; }
            let Some(weapon) = desc.get("weapon").filter(|w| w.truthy()) else { continue };
            prepare_weapon(world, cx, calls, &mut tail_sent, weapon, "f4_tail_weapon_class")?;
        }
    }
    let posts = world.sim.store.command_posts.posts.clone();
    for (i, post) in posts.iter().enumerate() {
        let row = F4PostSetup { post: *post, raw_path: raw_path(cx, i)? };
        dispatch(world, cx, calls, "f4_tail_post", block(F4PostSetup::WORDS, |r, h| row.put(r, h)))?;
    }
    // Every command vehicle's pad, by pad index (f4-tail.js f4_tail_pad).
    for pad in &pads {
        let mut call = Call::default();
        let setup = pad.setup(&mut |text| cx.intern(&mut call, text));
        call.row.resize(F4PadSetup::WORDS, 0.0);
        setup.put(&mut call.row, &mut call.heap);
        dispatch(world, cx, calls, "f4_tail_pad", call)?;
    }
    let prepared = stock::prepared_classes(cx);
    for side in cx.battle.sides.clone() {
        for (name, def) in side.units.as_object().map(|o| o.entries().collect::<Vec<_>>()).unwrap_or_default() {
            if !prepared.iter().any(|p| p == name) { continue; }
            let display = def.get("displayName").filter(|v| !matches!(v, Value::Null));
            let display = display.map(|v| v.as_str().map(str::to_string).ok_or_else(|| error("a hero name is not text"))).transpose()?;
            let mut call = Call::default();
            let row = F4HeroSetup { class_name: cx.intern(&mut call, name), name: display.map(|d| cx.intern(&mut call, &d)) };
            call.row.resize(F4HeroSetup::WORDS, 0.0); row.put(&mut call.row, &mut call.heap);
            dispatch(world, cx, calls, "f4_tail_hero", call)?;
        }
    }
    for (team, value) in capture_teams(world).into_iter().enumerate() {
        let row = F4CaptureTeam { team: team as u32, value };
        dispatch(world, cx, calls, "f4_tail_capture_team", block(F4CaptureTeam::WORDS, |r, h| row.put(r, h)))?;
    }
    dispatch(world, cx, calls, "f4_tail_ready", scalar(&[]))?;
    for (kind, count) in [(F4ObjectKind::Post, posts.len()), (F4ObjectKind::MobilePost, 0), (F4ObjectKind::Human, 1), (F4ObjectKind::Spawning, 1)] {
        for i in 0..count {
            dispatch(world, cx, calls, "f4_tail_handle", scalar(&[kind as u8 as f64, i as f64]))?;
            dispatch(world, cx, calls, "f4_tail_capacity", scalar(&[kind as u8 as f64, i as f64]))?;
        }
    }
    // `ai?.activate?.()`, then the viewpoint's binding (game.js 3156-3158):
    // the gameplay stream for combat, the player's spread registered now,
    // no recon key before any connection is admitted, and the world seed.
    let activation = ai::run_activation(world, cx)?;
    calls.extend(activation);
    let character = facts.character.ok_or_else(|| error("the roster has no human Character"))?;
    let body = facts.local.ok_or_else(|| error("the viewpoint is not registered"))?;
    let binding = F3PlayerBinding { character, body: Some(body), local: true,
        combat_random: cx.streams.register(streams::GAMEPLAY), weapon_random: cx.streams.register(streams::PLAYER_WEAPONS),
        recon_key: None, recon_seed: cx.options.seed };
    if !world.bind_spawn_player(&binding) { return Err(AdmitError::Refused { stage: STAGE, call: "f3_factory_player_bind".into() }); }
    calls.push((STAGE, "f3_factory_player_bind", block(F3PlayerBinding::WORDS, |r, h| binding.put(r, h))));
    cx.returned.push(("f3_factory_player_bind", 1.0));
    cx.ids.battle = Some(facts);
    Ok(())
}
