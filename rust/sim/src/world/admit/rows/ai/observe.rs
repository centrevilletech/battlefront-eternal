//! What createNativeAI reads from the host's objects before building its
//! rows (ai-world-load.js): the registry's next id, the fleet's hulls
//! (`[...vehicles, ...f2.prepared]`), the posts (`posts.all()`, after
//! attachPostAI and attachVehicles) and the pads. Observed on the world the
//! earlier owners constructed and kept on the context as Receipts. The
//! random stream registry's ids and states are the host's F3Random facts:
//! they come on the receipts from the stage before, not from here.
use super::super::{fleet, Cx};
use super::{attach_post_ai, error, mission_calls, HullReceipt, PostReceipt};
use crate::gen::{AiNavPost, SpawnPost};
use crate::world::admit::AdmitError;
use crate::world::owner::World;
use bfe_content::Value;

pub fn observe(world: &mut World, cx: &mut Cx) -> Result<(), AdmitError> {
    let mut receipts = cx.ids.ai.take().unwrap_or_default();
    let battle = cx.battle;
    // setupRows: `world.units.nextId` sizes the registry capacity.
    receipts.setup.next_unit_id = jsrt::to_uint32(world.sim.store.units.next_id);
    // hullFacts reads the constructed Vehicle: its local bounds, MaxHealth,
    // speed and turn rate. F3Hulls.prepare visited the same two lists in
    // the same order, so the hull bindings give the refs.
    receipts.hulls.clear();
    let plans = match battle.vehicle_def.as_ref() { Some(data) => fleet::hulls(data, &battle.world)?, None => Vec::new() };
    if plans.len() != cx.ids.hulls.len() { return Err(error("AI hull receipts need every F3 hull binding")); }
    for (plan, binding) in plans.iter().zip(&cx.ids.hulls) {
        let row = world.sim.store.vehicle.get(binding.setup.hull).ok_or_else(|| error("AI hull has no vehicle row"))?;
        receipts.hulls.push(HullReceipt {
            hull: binding.setup.hull, name: plan.name.clone(),
            bounds: [row.bounds_min[0], row.bounds_min[1], row.bounds_min[2], row.bounds_max[0], row.bounds_max[1], row.bounds_max[2]],
            max_health: row.max_health, speed: row.speed, turn_rate: row.turn_rate, prepared: plan.prepared, pad: plan.pad,
        });
    }
    // posts.all(): the static command posts (each with attachPostAI's
    // record over the map's description) then the mobile ones. Navigation
    // uses only the static posts.
    let (mission, _) = mission_calls(battle);
    let descs = battle.world.get("commandPosts").and_then(Value::as_array).unwrap_or(&[]);
    let spawns = battle.world.get("spawns");
    receipts.posts.clear();
    receipts.setup.navigation_posts.clear();
    let posts = world.sim.store.command_posts.posts.clone();
    for (index, reference) in posts.iter().enumerate() {
        let post = world.sim.store.post.get(*reference).ok_or_else(|| error("AI post has no store row"))?;
        let desc = descs.get(index).ok_or_else(|| error("AI post has no authored description"))?;
        let name = Value::str(&String::from_utf16_lossy(world.sim.strings.units(post.name)));
        let ai = attach_post_ai(desc, Some(&name), post.position, index, mission.as_ref(), spawns)?;
        receipts.setup.navigation_posts.push(AiNavPost { post: SpawnPost::Post(*reference), name: post.name, position: post.position });
        receipts.posts.push(PostReceipt { reference: SpawnPost::Post(*reference), building: post.building, name: Some(name), desc: desc.clone(), ai });
    }
    // posts.attachVehicles: the command vehicles' posts (src/mobile-posts.js
    // MobilePost: a desc and an ai record of its own, no building, no
    // capture), after the placed posts; then preparedPads (f4-tail.js) over
    // the same pads.
    let text = |id: crate::store::StrId| String::from_utf16_lossy(world.sim.strings.units(id));
    let mobile: Vec<_> = world.sim.store.mobile_post.iter().map(|(reference, post)| (reference, post.clone())).collect();
    for (reference, post) in &mobile {
        let name = Value::str(&text(post.name));
        let desc = object(vec![("name", Value::str(&text(post.desc.name))), ("displayName", Value::str(&text(post.desc.display_name))),
            ("valueBleed", Value::Number(post.desc.value_bleed)), ("mobile", Value::Bool(post.desc.mobile))]);
        let ai = object(vec![("order", Value::Null), ("atk", numbers(&post.ai.atk)), ("def", numbers(&post.ai.def)), ("bans", Value::Number(0.0)),
            ("valueBleed", Value::Number(post.ai.value_bleed)), ("spawnPath", Value::Null), ("allyPath", Value::Null), ("allyCount", Value::Number(0.0)),
            // MobilePost.ai: capture null and radius 0 are constants of the
            // record; its yaw getter is the post's.
            ("classSpawnPoints", Value::Number(post.ai.class_spawn_points)), ("capture", Value::Null), ("radius", Value::Number(0.0)), ("yaw", Value::Number(post.yaw))]);
        receipts.posts.push(PostReceipt { reference: SpawnPost::MobilePost(*reference), building: None, name: Some(name), desc, ai });
    }
    receipts.pads = prepared_pads(world, cx)?;
    cx.ids.ai = Some(receipts);
    Ok(())
}

fn object(entries: Vec<(&str, Value)>) -> Value {
    Value::Object(std::rc::Rc::new(bfe_content::value::Object::from_written(entries.into_iter().map(|(k, v)| (std::rc::Rc::from(k), v)).collect())))
}
fn numbers(values: &[f64]) -> Value { Value::Array(std::rc::Rc::new(values.iter().map(|v| Value::Number(*v)).collect())) }

/// preparedPads(fleet, posts): for every mobile post, the hulls its pad's
/// slot constructed (slot.variants, in construction order: the fleet's plan
/// for that pad) and the pad's live vehicle, each as padHull reads it from
/// the constructed Vehicle and the mission's commandClasses record.
/// Shared by the AI roster (createNativeAI) and F4Tail (`pads ??=
/// preparedPads(fleet, posts)`), which call it over the same constructed
/// hulls before any tick.
pub fn prepared_pads(world: &World, cx: &Cx) -> Result<Vec<super::PadFacts>, AdmitError> {
    let battle = cx.battle;
    let plans = match battle.vehicle_def.as_ref() { Some(data) => fleet::hulls(data, &battle.world)?, None => Vec::new() };
    let mobile: Vec<_> = world.sim.store.mobile_post.iter().map(|(reference, post)| (reference, post.clone())).collect();
    let (mission, _) = mission_calls(battle);
    let classes = mission.as_ref().and_then(|m| m.get("commandClasses")).cloned();
    let data = battle.vehicle_def.as_ref();
    let text = |id: crate::store::StrId| String::from_utf16_lossy(world.sim.strings.units(id));
    let mut out = Vec::new();
    for (_, post) in mobile {
        let index = post.index as usize;
        let mut variants = Vec::new();
        let mut vehicle = None;
        for (i, plan) in plans.iter().enumerate().filter(|(_, plan)| plan.pad == index) {
            let binding = cx.ids.hulls.get(i).ok_or_else(|| error("pad hull has no F3 binding"))?;
            let row = world.sim.store.vehicle.get(binding.setup.hull).ok_or_else(|| error("pad hull has no vehicle row"))?;
            let def = data.and_then(|d| d.get("classes")).and_then(|c| c.get(&plan.name));
            let settings = def.and_then(|d| d.get("settings"));
            let record = classes.as_ref().and_then(|c| c.get(&plan.name)).filter(|v| v.truthy()).map(|r| crate::gen::PsClass {
                atk: r.get("values").and_then(|v| v.get("atk")).and_then(Value::as_array).map(|a| a.iter().map(|v| bfe_content::js::to_number(Some(v))).collect()),
                def: r.get("values").and_then(|v| v.get("def")).and_then(Value::as_array).map(|a| a.iter().map(|v| bfe_content::js::to_number(Some(v))).collect()),
                value_bleed: r.get("valueBleed").filter(|v| !v.is_null()).map(|v| bfe_content::js::to_number(Some(v))),
                points: r.get("spawnPoints").and_then(Value::as_array).map(|a| a.iter().map(|p| std::array::from_fn(|k| p.at(k).filter(|v| !v.is_null()).map_or(0.0, |v| bfe_content::js::to_number(Some(v))))).collect()).unwrap_or_default(),
            });
            // Vehicle.spawn(team) gives a spawned hull its pad's team; a
            // prepared variant never spawned has none (`v.team | 0`).
            // v.controllingTeam is a seated lease's team, else the hull's;
            // no seat is leased at admission.
            let pad_team = data.and_then(|d| d.get("spawns")).and_then(|s| s.at(index)).and_then(|pad| pad.get("team")).map_or(0.0, |v| bfe_content::js::to_number(Some(v)));
            let team = if plan.prepared { 0.0 } else { f64::from(jsrt::to_int32(pad_team)) };
            let label = def.and_then(|d| d.get("label")).filter(|v| v.truthy()).and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| plan.name.clone());
            let facts = super::PadHullFacts {
                vehicle: binding.setup.hull, alive: row.alive,
                command: matches!(def.and_then(|d| d.get("base")).and_then(Value::as_str), Some("commandwalker" | "commandhover")),
                // v.aiTarget.generation: the VehicleTarget's own life counter.
                generation: row.target_generation,
                team, controlling_team: team, seated: false, yaw: row.yaw, name: text(row.name), label,
                radius: (row.size[0] * row.size[0] + row.size[1] * row.size[1] + row.size[2] * row.size[2]).sqrt() / 2.0,
                speed: row.speed,
                value_bleed: super::parse_int(settings.and_then(|s| s.get("ValueBleed"))),
                record,
            };
            if !plan.prepared && vehicle.is_none() { vehicle = Some(facts.clone()); }
            variants.push(facts);
        }
        out.push(super::PadFacts { index: post.index as u32, variants, vehicle });
    }
    out.sort_by_key(|pad| pad.index);
    Ok(out)
}
