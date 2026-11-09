//! The fielded classes' corpse stock: game.js createGame, headless, after
//! bindWeaponPolicyContent and before createNativeAI. For every team the
//! mission fields and each of its classes in authored order, the host
//! builds `Math.ceil(count * 1.5)` bodies (UnitFactory.build: the page
//! body, the registry id, the combat facade, F3's adoption, the crews'
//! rider rows, game.js instrument's random bindings) and releases each to
//! the pool at once (F3Sim.release), so the dead can respawn while their
//! bodies are still on the field. Every body here is warm: it has its
//! registry id, unlike the cold stock F3Factory.prepare stages (spawn.rs).
use super::dispatch::Returned;
use super::rows::{combat, units, Call, Calls, Cx};
use super::{bodies, AdmitError, Stage};
use crate::gen::*;
use crate::world::owner::World;
use bfe_content::{js::to_number, Value};

fn error(message: impl Into<String>) -> AdmitError { AdmitError::Content { stage: Stage::Ai, message: message.into() } }
fn refused(call: &str) -> AdmitError { AdmitError::Refused { stage: Stage::Ai, call: call.into() } }

/// Whether UnitFactory.prepare succeeds for the class: a soldier or
/// droideka with a prepared body on some side (UnitFactory.sideOf).
fn prepared(cx: &Cx, class_name: &str) -> bool { bodies::prepare::parts(cx, class_name).is_ok() }

/// game.js (AI_MODE !== "off"): the armies' classes (the player's and the
/// enemy's teams, plus each team's hero with ?heroes) must prepare; the
/// locals' may fail and then field nobody. Every class that prepared is
/// what createNativeAI later reads as `factory.prepared(name)`: those
/// names are published on the AI receipts, in the sides' table order
/// (ai-world-load.js walks `factory.sides` to collect them).
pub fn build(world: &mut World, cx: &mut Cx) -> Result<Calls, AdmitError> {
    let battle = cx.battle;
    let mission = battle.world.get("mission");
    let teams = mission.and_then(|m| m.get("teams")).and_then(Value::as_object);
    let own = battle.policy.team;
    let foe = teams.and_then(|teams| teams.entries().find(|(key, _)| to_number(Some(&Value::str(key))) != own))
        .map(|(key, _)| to_number(Some(&Value::str(key)))).unwrap_or(2.0);
    // classesFor(team): the mission's [className, count] pairs some side has.
    let classes_for = |team: f64| -> Vec<(String, f64)> {
        let key = bfe_content::js::number_to_string(team);
        teams.and_then(|teams| teams.get(&key)).and_then(|t| t.get("classes")).and_then(Value::as_array).unwrap_or(&[]).iter()
            .filter_map(|entry| {
                let name = entry.at(0).and_then(Value::as_str)?;
                battle.sides.iter().any(|side| side.units.get(name).is_some()).then(|| (name.to_string(), to_number(entry.at(1))))
            }).collect()
    };
    let fielded: Vec<f64> = teams.map(|teams| teams.entries().map(|(key, _)| to_number(Some(&Value::str(key)))).collect()).unwrap_or_default();
    let mut armies: Vec<String> = Vec::new();
    for team in [own, foe] {
        for (name, _) in classes_for(team) { if !armies.contains(&name) { armies.push(name); } }
    }
    if cx.options.heroes {
        for (_, team) in teams.map(|t| t.entries().collect::<Vec<_>>()).unwrap_or_default() {
            if let Some(hero) = team.get("hero").and_then(Value::as_str).filter(|hero| battle.sides.iter().any(|side| side.units.get(hero).is_some())) {
                if !armies.iter().any(|name| name == hero) { armies.push(hero.to_string()); }
            }
        }
    }
    let mut locals: Vec<String> = Vec::new();
    for team in &fielded {
        for (name, _) in classes_for(*team) { if !armies.contains(&name) && !locals.contains(&name) { locals.push(name); } }
    }
    for name in &armies {
        if !prepared(cx, name) { return Err(error(format!("army class {name} has no infantry body"))); }
    }
    // UnitFactory.prepared(name) asks whether the class's model body is
    // loaded, not whether that class was prepared: every class of a side
    // sharing a prepared class's model counts, in the sides' table order.
    let mut loaded: Vec<(String, String)> = Vec::new();
    for name in armies.iter().chain(locals.iter()).filter(|name| prepared(cx, name)) {
        if let Some((side, def)) = battle.sides.iter().find_map(|side| side.units.get(name).map(|def| (side, def))) {
            let model = def.get("model").and_then(Value::as_str).unwrap_or("undefined").to_string();
            if !loaded.contains(&(side.name.clone(), model.clone())) { loaded.push((side.name.clone(), model)); }
        }
    }
    let mut ready: Vec<String> = Vec::new();
    for side in &battle.sides {
        if let Some(table) = side.units.as_object() {
            for (name, def) in table.entries() {
                let model = def.get("model").and_then(Value::as_str).unwrap_or("undefined");
                if loaded.iter().any(|(s, m)| s == &side.name && m == model) { ready.push(name.to_string()); }
            }
        }
    }
    let mut receipts = cx.ids.ai.take().unwrap_or_default();
    receipts.prepared = ready;
    cx.ids.ai = Some(receipts);
    let mut calls = Vec::new();
    for team in &fielded {
        for (name, count) in classes_for(*team) {
            if !prepared(cx, &name) { continue; }
            // Half again as many as the roster: Math.ceil(count * 1.5).
            let bodies = (count * 1.5).ceil();
            let mut built = 0.0;
            while built < bodies {
                calls.extend(body(world, cx, &name)?);
                built += 1.0;
            }
        }
    }
    Ok(calls)
}

/// One warm body: UnitFactory.build, then F3's adoption, game.js
/// instrument and the pool release, in the host's call order.
fn body(world: &mut World, cx: &mut Cx, class_name: &str) -> Result<Calls, AdmitError> {
    cx.refresh_strings(&world.sim.strings);
    let holders = std::mem::take(&mut cx.holders);
    let result = construct(world, cx, class_name, &holders);
    cx.holders = holders;
    result
}

/// Dispatches one call against the world and keeps it with the stage's
/// calls, answering what the body returned.
fn dispatch(world: &mut World, cx: &mut Cx, calls: &mut Calls, name: &'static str, call: Call) -> Result<Returned, AdmitError> {
    super::sequence::args(world, &call)?;
    let answer = super::dispatch::call(world, Stage::Ai, name, &call)?;
    super::dispatch::record(cx, Stage::Ai, name, answer.clone())?;
    calls.push((Stage::Ai, name, call));
    cx.refresh_strings(&world.sim.strings);
    Ok(answer)
}

/// UnitFactory.build's page body (units.js buildBody), shared by the warm
/// stock here and the prepared stock (spawn.rs): a droideka's animator and
/// muzzle samples over per-body clip copies, or a soldier's body module
/// constructed from the class record as JSON.stringify spells it.
pub(crate) fn page_body(world: &mut World, cx: &mut Cx, parts: &bodies::prepare::Parts, class_name: &str, calls: &mut Calls) -> Result<(), AdmitError> {
    let bank = parts.walker.unwrap_or(parts.shared);
    // A droideka's clips are loaded for each body (units.js walkerData,
    // loadClips per buildBody): nothing is shared between bodies, and the
    // constructor's samples are registered apart from the animator's bank.
    // Prepared stock reuses one construction slot, so the factory's cookie
    // tells its bodies apart where the registry id cannot.
    let sample_key = format!("droideka-sample:{class_name}:{}:{}", world.sim.store.units.next_id,
        crate::units::f3_construction::cookies_so_far_in(&world.units));
    if parts.walker.is_some() {
        // buildBody for a droideka: DroidekaAnimator's constructor makes a
        // Pose over the actor's bones (root: DummyRoot), samples the roll
        // clip at frame 0 and applies it to find the shell's axle, then
        // Droideka.arm's muzzle sampler makes a second Pose, clears it and
        // applies the idle clip. The accumulator views the page takes
        // (pose_q, pose_t, pose_w) are reads of module memory, not calls
        // the sequence makes.
        let root = parts.actor.bone_by_lower_name("dummyroot").map_or(-1.0, |i| i as f64);
        let bones = parts.actor.bones.len() as f64;
        let Returned::Scalar(axle) = dispatch(world, cx, calls, "pose_new", Call { args: vec![bones, root], ..Default::default() })? else { return Err(refused("pose_new")); };
        if parts.actor.bone_by_lower_name("center_box").is_some() {
            if let Some(clip) = bank.get("roll") {
                let id = super::spawn::register_clip(world, cx, clip, &sample_key, calls)?;
                dispatch(world, cx, calls, "pose_add", Call { args: vec![axle, f64::from(id), 0.0, 1.0, 0.0], ..Default::default() })?;
                dispatch(world, cx, calls, "pose_out", Call { args: vec![axle], ..Default::default() })?;
            }
        }
        dispatch(world, cx, calls, "pose_clear", Call { args: vec![axle], ..Default::default() })?;
        let Returned::Scalar(muzzle) = dispatch(world, cx, calls, "pose_new", Call { args: vec![bones, root], ..Default::default() })? else { return Err(refused("pose_new")); };
        dispatch(world, cx, calls, "pose_clear", Call { args: vec![muzzle], ..Default::default() })?;
        if let Some(clip) = bank.get("idle") {
            let id = super::spawn::register_clip(world, cx, clip, &sample_key, calls)?;
            dispatch(world, cx, calls, "pose_add", Call { args: vec![muzzle, f64::from(id), 0.0, 1.0, 0.0], ..Default::default() })?;
            dispatch(world, cx, calls, "pose_out", Call { args: vec![muzzle], ..Default::default() })?;
        }
    } else {
        // buildBody for a soldier: the page body module constructs the
        // actor's rig from the class record as JSON.stringify spells it.
        let json = bfe_content::js::json_stringify(Some(parts.def));
        let construct = Call { args: vec![0.0, json.len() as f64], scratch: json.into_bytes(), ..Default::default() };
        if dispatch(world, cx, calls, "body_construct", construct)? == Returned::Refused { return Err(refused("body_construct")); }
    }
    Ok(())
}

fn construct(world: &mut World, cx: &mut Cx, class_name: &str, holders: &[bodies::Holder]) -> Result<Calls, AdmitError> {
    let gameplay = cx.ids.battle.as_ref().ok_or(AdmitError::Unavailable(Stage::LiveTail))?.gameplay;
    let parts = bodies::prepare::parts(cx, class_name)?;
    let mut calls = Vec::new();
    let bank = parts.walker.unwrap_or(parts.shared);
    let cookie = crate::units::f3_construction::prepared_count_in(&world.units) as u32;
    let mut clip_ids = Vec::with_capacity(bank.list.len());
    page_body(world, cx, &parts, class_name, &mut calls)?;
    // Units.add: the registry's next id, generation 0, before the combat
    // facade attaches (its cmb_new takes that id).
    let source = world.sim.store.units.next_id;
    if !source.is_finite() || source < 0. || source.fract() != 0. || source > u32::MAX as f64 { return Err(refused("unit registry id")); }
    let key = F3UnitKey { slot: source as u32, generation: 0, life: 0. };
    let added = dispatch(world, cx, &mut calls, "f3_registry_add", Call { args: vec![f64::from(KIND_UNIT), 0.0], ..Default::default() })?;
    if added != Returned::Scalar(source) { return Err(refused("f3_registry_add")); }
    // Soldiers share one animator bank per bone-name set (UnitFactory.clipsFor); a droideka's is its body's own.
    let bank_key = if parts.walker.is_some() { format!("{}:{}", super::spawn::clip_bank_key(&parts, cookie), key.slot) } else { super::spawn::clip_bank_key(&parts, cookie) };
    // CombatFacade.attach: the class's weapons and class (first body of the
    // class), cmb_new, then the six kept settings.
    let combat_calls = combat::attach_unit(cx, key.slot, parts.def, false, Stage::Ai)?;
    let mut interns = Call::default();
    let bodies::Body { unit, adoption, skeleton, combat, muzzles } = bodies::prepare::body(cx, &parts, class_name, key.clone(), false, &mut interns)?;
    if world.sim.store.unit.reference(key.slot).is_some() { return Err(refused("unit slot")); }
    // The host imports the Unit after the facade's settings; the native
    // combat row lives on the unit, so the slab row exists before cmb_new.
    world.sim.store.unit.insert_at(key.slot, key.generation, unit);
    for (_, name, call) in combat_calls {
        if dispatch(world, cx, &mut calls, name, call)? == Returned::Refused { return Err(refused(name)); }
    }
    // F3Sim.adopt: the skeleton with the body's interns, the animator bank
    // (shared soldier clips by bone-name bank identity), services, adoption.
    let mut skeleton_call = units::call(F3BodySkeleton::WORDS, |r, h| skeleton.put(r, h));
    skeleton_call.strings = interns.strings;
    if dispatch(world, cx, &mut calls, "f3_body_skeleton", skeleton_call)? == Returned::Scalar(0.0) { return Err(refused("f3_body_skeleton")); }
    for clip in &bank.list {
        if !clip_ids.iter().any(|(name, _)| *name == clip.name) {
            clip_ids.push((clip.name.as_str(), super::spawn::register_clip(world, cx, clip, &bank_key, &mut calls)?));
        }
    }
    let droideka_class = if parts.walker.is_some() {
        let path = cx.ids.defs.iter().find(|(key, id)| key.starts_with("class:") && *id == f64::from(combat.class.0))
            .map(|(key, _)| key.trim_start_matches("class:").to_string());
        let name = format!("droideka:{}", path.ok_or_else(|| error("droideka has no combat class identity"))?);
        if let Some((_, id)) = cx.ids.classes.iter().find(|(key, _)| key == &name) { Some(*id as u32) } else {
            let row = units::droideka_class(parts.def, |name| clip_ids.iter().find(|(key, _)| *key == name).map(|(_, id)| *id));
            let call = units::call(F3DroidekaClass::WORDS, |r, h| row.put(r, h));
            let Returned::Scalar(id) = dispatch(world, cx, &mut calls, "units_droideka_class", call)? else { return Err(refused("units_droideka_class")); };
            cx.ids.classes.push((name, id)); Some(id as u32)
        }
    } else { None };
    // Droideka.arm attaches the walker's animator and muzzle rig as soon as
    // the class row exists, before F3.prepareWeapons runs the services.
    if let Some(class) = droideka_class {
        dispatch(world, cx, &mut calls, "units_droideka_attach", Call { args: vec![f64::from(key.slot), f64::from(class)], ..Default::default() })?;
        if let Some(muzzles) = &muzzles {
            dispatch(world, cx, &mut calls, "units_muzzle_attach", Call { args: vec![f64::from(key.slot), f64::from(class)], row: muzzles.words.clone(), ..Default::default() })?;
        }
    }
    let services = units::prepare_unit_services(cx, parts.class, key.clone())?;
    super::spawn::apply_construction_calls(world, cx, &services)?;
    calls.extend(services);
    let adopt = units::call(F3Adoption::WORDS, |r, h| adoption.put(r, h));
    if dispatch(world, cx, &mut calls, "f3_adopt", adopt)? == Returned::Scalar(0.0) { return Err(refused("f3_adopt")); }
    // F3Items.flush after adoption, then F2Riders.ensure for every holder.
    let dispensers = units::dispenser_calls(cx, parts.class, key.clone())?;
    super::spawn::apply_construction_calls(world, cx, &dispensers)?;
    calls.extend(dispensers);
    let scale = world.sim.store.unit.get(key.reference()).map(|u| u.root_scale).ok_or_else(|| refused("unit slot"))?;
    for holder in holders {
        let rider = bodies::rider_birth(key.reference(), key.life, scale, bank, holder,
            |clip| clip_ids.iter().find(|(name, _)| *name == clip.name).map_or(0, |(_, id)| *id));
        dispatch(world, cx, &mut calls, "f2_rider_birth", units::call(F2RiderBirth::WORDS, |r, h| rider.put(r, h)))?;
    }
    // game.js instrument: `unit.combat.random = gameplayRandom` sets the
    // flag and binds the stream; UnitFactory.release then returns the body
    // to the pool (F3Sim.release: poolRelease).
    for (action, field, value) in [(F3Action::CombatField, F3CombatField::Random as u32, 1.), (F3Action::RandomBinding, 0, f64::from(gameplay)), (F3Action::PoolRelease, 0, 0.)] {
        let change = F3Change { key: key.clone(), action, field, value, vector: cx.player.command_vector,
            on_shield: cx.player.on_shield, attacker: Attacker::Null, ..Default::default() };
        if dispatch(world, cx, &mut calls, "f3_command", units::call(F3Change::WORDS, |r, h| change.put(r, h)))? == Returned::Scalar(0.0) { return Err(refused("f3_command")); }
    }
    Ok(calls)
}
