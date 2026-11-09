//! game.js 3110-3126 (headless), between createWorldSim and F4Live: the
//! viewpoint joins F3 and is registered, the factory reserves the registry's
//! future id bound, and inactive construction stock is prepared for every
//! class the roster can field. Everything here runs against the world the
//! earlier stages built: the counts come from the registry, the roster and
//! the bodies already adopted, never from a prediction.
use super::super::super::{spawn, AdmitError, Stage};
use super::super::{Calls, Cx};
use super::{streams, Facts};
use crate::gen::{F3ColdStage, Member};
use crate::world::owner::World;
use bfe_content::{js::to_number, value::Value};


fn error(message: impl Into<String>) -> AdmitError { AdmitError::Content { stage: Stage::World, message: message.into() } }

/// The classes UnitFactory.prepare loaded by the time F4 reads
/// `factory.prepared(name)`: the fielded armies' classes (the player's and
/// the enemy's teams, each team's hero with ?heroes, the locals' that have
/// bodies; game.js createGame's stock), the two armies' playable rosters
/// (game.js 3116) and the player's own class. A class without a soldier or
/// droideka body never prepared. Membership only: callers keep their own order.
pub fn prepared_classes(cx: &Cx) -> Vec<String> {
    let battle = cx.battle;
    let mission = battle.world.get("mission");
    let teams = mission.and_then(|m| m.get("teams")).and_then(Value::as_object);
    let mut names: Vec<String> = vec![battle.policy.class_name.clone()];
    let mut add = |name: &str| if !names.iter().any(|n| n == name) { names.push(name.to_string()); };
    for (_, team) in teams.map(|t| t.entries().collect::<Vec<_>>()).unwrap_or_default() {
        for entry in team.get("classes").and_then(Value::as_array).unwrap_or(&[]) {
            if let Some(name) = entry.at(0).and_then(Value::as_str) { add(name); }
        }
        if cx.options.heroes { if let Some(hero) = team.get("hero").and_then(Value::as_str) { add(hero); } }
    }
    for team in 1..=2 {
        for name in bfe_content::mission::playable_classes(teams.and_then(|t| t.get(&team.to_string()))) { add(&name); }
    }
    names.retain(|name| super::super::super::bodies::prepare::parts(cx, name).is_ok());
    // UnitFactory.prepared tests the loaded *model* (`body:<side>/<model>`),
    // so every class of the same side sharing a prepared class's model
    // counts as prepared too, whether or not anything fielded it.
    let model_of = |name: &str| -> Option<String> {
        let side = battle.sides.iter().find(|side| side.units.get(name).is_some_and(Value::truthy))?;
        let model = side.units.get(name)?.get("model").and_then(Value::as_str).unwrap_or("undefined");
        Some(format!("{}/{model}", side.name))
    };
    let loaded: Vec<String> = names.iter().filter_map(|name| model_of(name)).collect();
    let mut prepared = Vec::new();
    for side in &battle.sides {
        for (name, _) in side.units.as_object().map(|o| o.entries().collect::<Vec<_>>()).unwrap_or_default() {
            if model_of(name).is_some_and(|model| loaded.contains(&model)) && !prepared.iter().any(|p| p == name) { prepared.push(name.to_string()); }
        }
    }
    prepared
}

/// UnitFactory.prepared(className) for one class.
pub fn prepared(cx: &Cx, class_name: &str) -> bool { prepared_classes(cx).iter().any(|n| n == class_name) }

/// The registry's viewpoint slot (game.js `units.add(new Unit({human:true}))`,
/// recorded by the sequence before attachF3Combat).
fn viewpoint_slot(cx: &Cx) -> Result<u32, AdmitError> {
    let id = cx.ids.registry.iter().find(|(key, _)| key == "viewpoint").map(|(_, id)| *id)
        .ok_or_else(|| error("the viewpoint has no registry id"))?;
    if !id.is_finite() || id < 0.0 || id.fract() != 0.0 || id > u32::MAX as f64 { return Err(error("the viewpoint registry id is not a slot")); }
    Ok(id as u32)
}

/// The factory facts the F4 owners and the session read, as the world
/// stands now. `built` is UnitFactory.built: every registry Unit but the
/// viewpoint (warm corpses included, prepared stock never listed).
pub fn facts(world: &World, cx: &mut Cx) -> Result<Facts, AdmitError> {
    let store = &world.sim.store;
    let viewpoint = viewpoint_slot(cx)?;
    let built = store.units.list.iter().filter(|m| matches!(m, Member::Unit(r) if r.slot != viewpoint)).count();
    let fleet = store.vehicles.vehicles.len();
    let prepared = store.vehicle.len().checked_sub(fleet).ok_or_else(|| error("more live hulls than vehicles"))?;
    let characters = store.battle.characters.clone();
    let character = characters.iter().copied().find(|r| store.character.get(*r).is_some_and(|c| c.human));
    let previous = cx.ids.battle.take().unwrap_or_default();
    Ok(Facts {
        local: crate::units::f3::registered_viewpoint_in(&world.units).or(previous.local),
        character, characters,
        next_id: jsrt::to_uint32(store.units.next_id),
        built: u32::try_from(built).map_err(|_| error("too many built bodies"))?,
        construction_limit: previous.construction_limit,
        fleet: u32::try_from(fleet).map_err(|_| error("too many hulls"))?,
        prepared: u32::try_from(prepared).map_err(|_| error("too many prepared hulls"))?,
        point_capacity: previous.point_capacity,
        // game.js `let spawnIndex = 0`: respawn keeps index 0 at admission.
        spawn_index: 0.0,
        random: previous.random,
        // configure registered the gameplay stream at attachF3's end; this
        // answers that id (a new one here means the units stage did not).
        gameplay: cx.streams.register(streams::GAMEPLAY),
    })
}

/// game.js's PlayerBodies before attachF3Combat: the player's page body,
/// `new Soldier(world, def)` (presentation/body.js constructSoldier): the
/// class record as JSON.stringify spells it, through body_construct, whose
/// answer row is the body's state (its jet recharge among it).
pub fn construct_viewpoint_body(world: &mut World, cx: &mut Cx, calls: &mut Calls) -> Result<(), AdmitError> {
    let class_name = cx.battle.policy.class_name.clone();
    let parts = super::super::super::bodies::prepare::parts(cx, &class_name)?;
    // The same page body UnitFactory.build makes: a soldier's body module,
    // or a droideka's animator and muzzle samples (stock.rs page_body).
    let mut made = Vec::new();
    super::super::super::stock::page_body(world, cx, &parts, &class_name, &mut made)?;
    calls.extend(made.into_iter().map(|(_, name, call)| (Stage::Combat, name, call)));
    Ok(())
}

/// game.js 1060-1063 (`if (policy.auto) respawn(pick, !wanted)`): the
/// player's first spawn point, the first post his team owns with a spawn
/// path (else any post with one), at a point the gameplay stream picks.
fn spawn_point(cx: &Cx) -> Result<([f64; 3], f64), AdmitError> {
    let world = &cx.battle.world;
    let spawns = world.get("spawns");
    let descs = world.get("commandPosts").and_then(Value::as_array).unwrap_or(&[]);
    let path_of = |desc: &Value| desc.get("spawnPath").and_then(Value::as_str).and_then(|p| spawns.and_then(|s| s.get(p))).filter(|v| v.truthy()).cloned();
    // Posts start active; only their team and their path select them.
    let own: Vec<Value> = descs.iter().filter(|d| to_number(d.get("team")) == cx.battle.policy.team).filter_map(|d| path_of(d)).collect();
    let choices: Vec<Value> = if own.is_empty() { descs.iter().filter_map(|d| path_of(d)).collect() } else { own };
    let points: Vec<[f64; 4]> = match choices.first().or_else(|| spawns.and_then(Value::as_object).and_then(|o| o.entries().next()).map(|(_, v)| v)) {
        Some(path) => path.as_array().ok_or_else(|| error("a spawn path is not an array"))?.iter()
            .map(|p| { let p = p.as_array().unwrap_or(&[]); std::array::from_fn(|i| to_number(p.get(i))) }).collect(),
        None => vec![[0.0, 10.0, 0.0, 0.0]],
    };
    // `points[Math.floor(gameplayRandom() * points.length)]`: the battle's
    // gameplay stream's first draw (streams.draw("gameplay"), RandomStreams(SEED)).
    let mut streams = jsrt::rng::RandomStreams::new(f64::from(cx.options.seed));
    let index = (streams.get("gameplay").uniform() * points.len() as f64).floor();
    let point = points.get(index as usize).ok_or_else(|| error("the spawn path has no point for the draw"))?;
    Ok(([point[0], point[1], point[2]], point[3]))
}

/// placePlayer's `soldier.spawn(x, y, z, yaw)` (presentation/body.js
/// spawnSoldier): the collision world's body order refreshed (rayBodies:
/// no bodies yet), then body_spawn with the point and the body's own jet
/// recharge; its answer is what the viewpoint's Unit import carries.
pub fn spawn_viewpoint_body(world: &mut World, cx: &mut Cx, calls: &mut Calls) -> Result<(), AdmitError> {
    use crate::gen::{BodySpawn, DroidekaSpawn, Soldier};
    let kernel = cx.ids.kernel.as_ref().ok_or(AdmitError::Unavailable(Stage::Kernels))?.world as u32;
    let (position, yaw) = spawn_point(cx)?;
    let class_name = cx.battle.policy.class_name.clone();
    let walker = super::super::super::bodies::prepare::parts(cx, &class_name)?.walker.is_some();
    // spawnSoldier / spawnDroideka write the spawn row first, then refresh
    // the kernel's body order (rayBodies: world_order over the bodies so
    // far) and spawn; the row therefore travels with world_order, as the
    // recorder saw it.
    let (export, mut order) = (if walker { "body_spawn_droideka" } else { "body_spawn" },
        super::super::Call { args: vec![kernel as f64, world.collision.state.bodies.len() as f64], ..Default::default() });
    order.scratch = (0..world.collision.state.bodies.len() as u32).flat_map(|k| k.to_le_bytes()).collect();
    if walker {
        // Droideka.arm gave it the class's shield weapon before the spawn
        // (`weapons.find(w => w.base === "shield")`, MaxShield || 0).
        let def = cx.battle.sides.iter().find_map(|side| side.units.get(&class_name)).cloned().unwrap_or(Value::Null);
        let shield = def.get("weapons").and_then(Value::as_array).unwrap_or(&[]).iter()
            .find(|w| w.get("base").and_then(Value::as_str) == Some("shield")).cloned();
        let shield_max = shield.as_ref().map(|s| s.get("MaxShield").filter(|v| v.truthy()).map(|v| to_number(Some(v))).unwrap_or(0.0)).unwrap_or(0.0);
        let input = DroidekaSpawn { position, yaw, has_shield: shield.is_some(), shield_max };
        order.row = vec![0.0; DroidekaSpawn::WORDS];
        input.put(&mut order.row, &mut order.heap);
    } else {
        // body_construct answers its block: the header, then the soldier row.
        let header = crate::store::block::HEADER;
        let recharge = cx.returned_row("body_construct").filter(|row| row.len() >= header + Soldier::WORDS).map(|row| Soldier::take(&row[header..], &[]).jet.recharge)
            .ok_or_else(|| error("the viewpoint body was not constructed before its spawn"))?;
        let input = BodySpawn { position, yaw, recharge };
        order.row = vec![0.0; BodySpawn::WORDS];
        input.put(&mut order.row, &mut order.heap);
    }
    super::super::super::sequence::args(world, &order)?;
    let answer = super::super::super::dispatch::call(world, Stage::Combat, "world_order", &order)?;
    super::super::super::dispatch::record(cx, Stage::Combat, "world_order", answer)?;
    calls.push((Stage::Combat, "world_order", order));
    let call = super::super::Call { args: vec![kernel as f64], ..Default::default() };
    let answer = super::super::super::dispatch::call(world, Stage::Combat, export, &call)?;
    super::super::super::dispatch::record(cx, Stage::Combat, export, answer)?;
    calls.push((Stage::Combat, export, call));
    Ok(())
}

/// `calls` receives every call as it is made and `stages` the typed stages of
/// the prepared stock (Rows::cold_stages), even when a later body refuses.
pub fn run(world: &mut World, cx: &mut Cx, calls: &mut Calls, stages: &mut Vec<F3ColdStage>) -> Result<(), AdmitError> {
    cx.ids.battle = Some(facts(world, cx)?);
    let slot = viewpoint_slot(cx)?;
    // Every call here is the world stage's, including the clip registrations
    // the shared helpers label as the spawn piece's.
    calls.extend(spawn::adopt_viewpoint(world, cx, slot, Stage::World)?.into_iter().map(|(_, name, call)| (Stage::World, name, call)));
    // `unitsSim.construction.reserve(units.nextId + 512)`: the future mixed
    // registry id bound, hull births included.
    let limit = jsrt::to_uint32(world.sim.store.units.next_id).checked_add(512).ok_or_else(|| error("registry bound overflows"))?;
    world.reserve_spawn_bodies(limit);
    calls.push((Stage::World, "f3_factory_reserve", super::super::Call { args: vec![limit as f64], ..Default::default() }));
    if let Some(facts) = &mut cx.ids.battle { facts.construction_limit = Some(limit); }
    // `for (const names of rosters.values()) await factory.prepare(name)`:
    // the asset check for every class the two armies can field.
    let mission = cx.battle.world.get("mission");
    for team in 1..=2 {
        let names = bfe_content::mission::playable_classes(mission.and_then(|m| m.get("teams")).and_then(|t| t.get(&team.to_string())));
        for name in &names { spawn::load(cx, name)?; }
    }
    // Every prepared class of every side gets stock for its possible
    // population: max(2, 2 * the roster's count for it - its warm bodies).
    let mut plan = Vec::new();
    for side in &cx.battle.sides {
        let Some(units) = side.units.as_object() else { continue };
        for (name, _) in units.entries() {
            if !prepared(cx, name) { continue; }
            let mut population = 0.0;
            for team in &world.sim.store.spawning.teams {
                if team.classes.iter().any(|class| String::from_utf16_lossy(world.sim.strings.units(class.name)) == name) {
                    population += team.unit_count;
                }
            }
            let warm = world.sim.store.units.list.iter().filter(|m| match m {
                Member::Unit(r) if r.slot != slot => world.sim.store.unit.get(*r)
                    .is_some_and(|u| String::from_utf16_lossy(world.sim.strings.units(u.class_name)) == name),
                _ => false,
            }).count() as f64;
            let count = (population * 2.0 - warm).max(2.0);
            plan.push((name.to_string(), count));
        }
    }
    for (name, count) in plan {
        let mut made = 0.0;
        while made < count {
            let prepared = spawn::prepare(world, cx, &name)?;
            stages.push(prepared.stage);
            calls.extend(prepared.calls.into_iter().map(|(_, name, call)| (Stage::World, name, call)));
            made += 1.0;
        }
    }
    cx.ids.battle = Some(facts(world, cx)?);
    Ok(())
}
