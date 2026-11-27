//! A battle's content decoded: the model packs and the battle's weapon
//! models (game.js's merge), the bodies and their clip banks, droideka
//! clips, the map's collision, the vehicle pack's templates and the props'
//! models, clips and bodies, the death regions, the AI's route table. Built
//! from a loaded Battle (src/battle.rs) in game.js's order.

use crate::actor::{build_actor, Actor};
use crate::battle::Battle;
use crate::bin_arrays::f32s;
use crate::clips::{keep_last_tracks, load_clips, load_clips_by_crc, merge_clips, Clips, CrcBindStats};
use crate::collision::{
    json_shape, range_shape, range_shape_local_swbf2, range_shape_transformed_swbf2,
    BodyTriangles, MapCollision,
};
use crate::models::{model_pack, model_pack_swbf2, ModelMap};
use crate::regions::{death_regions, BuiltRegion};
use crate::value::{Object, Value};
use bfe_family::GameFamily;
use std::collections::HashMap;
use std::rc::Rc;

/// A vehicle class's template (src/vehicles.js 1971-2000, without the
/// presentation batches): its rig record, parts, collision shape, clips
/// (walker banks' repeated joints dropped), wheels and muzzles.
pub struct VehicleTemplate {
    pub name: String,
    pub rig: Value,
    pub parts: crate::models::Model,
    pub shape: BodyTriangles,
    pub clips: Option<Clips>,
    pub wheel_radii: Vec<(String, f64)>,
    pub muzzle_tips: Vec<(String, [f64; 3])>,
    /// gearFoot: how far below its origin it stands on its
    /// gear (a craft with a takeoff clip), else None.
    pub gear_foot: Option<f64>,
}

/// A prop instance's collision bodies (`body(range)`), by look.
pub struct PropBodies {
    pub instance: usize,
    /// True for 2005 props driven by `worldAnimations` groups or hierarchies:
    /// their look shapes stay in model space so the simulation kernel admits
    /// them once as moving bodies rather than baking a fixed world-space copy.
    pub world_animated: bool,
    pub built: Option<BodyTriangles>,
    pub destroyed: Option<BodyTriangles>,
    pub unbuilt: Option<BodyTriangles>,
}

/// A placed power-up as src/placed-items.js spawns it at the battle's
/// start: its item, its place (the matrix's translation), its heading (the
/// forward axis about up, Math.atan2(e[8], e[10])) and team.
pub struct PlacedItem {
    /// Its place in props.json's `items`.
    pub index: usize,
    pub item: Value,
    pub position: crate::three::Vector3,
    pub yaw: f64,
    /// `entry.team || 0`.
    pub team: Value,
}

pub struct Content {
    pub battle: Battle,
    /// Each side's weapons.bin pack, in `sides` order.
    pub weapon_packs: Vec<(String, ModelMap)>,
    pub vehicle_models: Option<ModelMap>,
    pub vehicle_extras: Option<ModelMap>,
    pub props_models: Option<ModelMap>,
    /// weaponModels: the played side's pack, the other sides'
    /// (first name wins), then the vehicle pack's extras (a headless world
    /// has no turret extras). Values say which pack each came from.
    pub weapon_models: ModelMap,
    pub weapon_model_sources: Vec<(String, String)>,
    /// Bodies by "<side>/<model>".
    pub actors: Vec<(String, Actor)>,
    /// The shared clips each body plays (anims.json on its bones), by body.
    pub body_clips: Vec<(String, Rc<Clips>)>,
    /// Per-unit CRC-bound clip banks by `"<side>/<class>"` (2005 only; empty for 2004).
    pub unit_clips: Vec<(String, Rc<Clips>)>,
    /// Per-unit CRC animation bind diagnostics (2005 only; empty for 2004).
    pub unit_anim_stats: Vec<(String, CrcBindStats)>,
    /// Droideka classes' own clips on their bodies, by "<side>/<class>".
    pub walker_clips: Vec<(String, Clips)>,
    pub collision: MapCollision,
    pub vehicle_templates: Vec<VehicleTemplate>,
    pub vehicle_blockers: Option<BodyTriangles>,
    pub prop_bodies: Vec<PropBodies>,
    /// clipsFor: a model's clips, by model, in first-use order.
    pub prop_clips: Vec<(String, Option<Clips>)>,
    pub death_regions: Vec<(Value, BuiltRegion)>,
    /// ai-nav.bin's routing words (routeWords).
    pub route_words: Option<Result<Vec<u32>, String>>,
    /// The placed power-ups a world spawns at the start.
    pub placed_items: Vec<PlacedItem>,
}

/// A 2005 starfighter's or gunship's model as a rig (`sides/<side>/craftrigs.json`,
/// presentation only: read by the presentation's loader, never by a battle's
/// admission) and the takeoff clip its class plays on the rig's bones.
#[derive(Clone, Debug)]
pub struct CraftRig {
    pub class: String,
    pub model: String,
    /// The model's bones, `[{name, parent, local}]`.
    pub bones: Value,
    /// Its parts, each in the space of the bone it hangs from (the record's `bone`).
    pub parts: Vec<crate::models::Part>,
    /// The class's `takeoff` clip on these bones, when its bank has one.
    pub takeoff: Option<crate::clips::Clip>,
}

impl CraftRig {
    /// The bones' names, in their order.
    pub fn bone_names(&self) -> Vec<&str> {
        self.bones
            .as_array()
            .into_iter()
            .flatten()
            .map(|bone| bone.get("name").and_then(Value::as_str).unwrap_or(""))
            .collect()
    }

    /// Its parts grouped by the bone they hang from, in the bones' order, then
    /// the parts hung from no bone it has (`None`); each group is drawn as one
    /// model named by [`CraftRig::group_name`].
    pub fn groups(&self) -> Vec<(Option<usize>, Vec<&crate::models::Part>)> {
        let names = self.bone_names();
        let mut groups: Vec<(Option<usize>, Vec<&crate::models::Part>)> =
            (0..names.len()).map(|i| (Some(i), Vec::new())).collect();
        let mut unboned = Vec::new();
        for part in &self.parts {
            let bone = part.record.get("bone").and_then(Value::as_str);
            match bone.and_then(|b| names.iter().position(|n| n.eq_ignore_ascii_case(b))) {
                Some(i) => groups[i].1.push(part),
                None => unboned.push(part),
            }
        }
        groups.push((None, unboned));
        groups.retain(|(_, parts)| !parts.is_empty());
        groups
    }

    /// The model name a group's parts are registered and drawn under.
    pub fn group_name(&self, bone: Option<usize>) -> String {
        match bone {
            Some(i) => format!("craftrig:{}:{i}", self.model),
            None => format!("craftrig:{}:-", self.model),
        }
    }
}

/// The craft rigs of one 2005 side (`craftrigs.json` and its vertex and index
/// buffers): each flyer class of the side whose model has a rig, with its
/// `takeoff` clip from the side's banks bound to the rig's bones by CRC.
pub fn craft_rigs(battle: &Battle, side: &str, rigs: &Value, buf: &[u8], index: &[u8]) -> Result<Vec<CraftRig>, String> {
    let mut out = Vec::new();
    let Some(units) = battle.sides.iter().find(|s| s.name == side).and_then(|s| s.units.as_object()) else {
        return Ok(out);
    };
    let banks = battle
        .swbf2
        .as_ref()
        .and_then(|s| s.side_anim_banks.iter().find(|(name, _, _)| name == side))
        .map(|(_, meta, bin)| floats(bin).map(|f| (meta, f)))
        .transpose()?;
    let models = rigs.get("models").and_then(Value::as_object);
    let table: Vec<(Rc<str>, Value)> = models
        .into_iter()
        .flat_map(|m| m.entries())
        .map(|(name, rig)| (Rc::from(name), rig.get("parts").cloned().unwrap_or(Value::Null)))
        .collect();
    let pack = model_pack_swbf2(
        Some(&Value::Object(Rc::new(Object::from_written(table)))),
        &floats(buf)?,
        &indices(index)?,
        &format!("sides/{side}/craftrigs.json"),
    )?;
    for (class_name, def) in units.entries() {
        if !matches!(def.get("base").and_then(Value::as_str), Some("flyer" | "commandflyer")) {
            continue;
        }
        let Some(model) = def.get("model").and_then(Value::as_str).map(str::to_ascii_lowercase) else {
            continue;
        };
        let (Some(rig), Some(parts)) = (models.and_then(|m| m.get(&model)), pack.get(&model)) else {
            continue;
        };
        let bones = rig.get("bones").cloned().unwrap_or(Value::Null);
        let names: Vec<String> = bones
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|bone| bone.get("name").and_then(Value::as_str).map(str::to_string))
            .collect();
        let bank = def
            .get("properties")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|p| p.at(0).and_then(Value::as_str).is_some_and(|k| k.eq_ignore_ascii_case("AnimationName")))
            .and_then(|p| p.at(1).and_then(Value::as_str).map(str::to_string));
        let takeoff = match (bank, banks.as_ref()) {
            (Some(bank), Some((meta, bank_floats))) => {
                let names_of = HashMap::from([(crate::zero_hash::crc_hex("takeoff"), vec!["takeoff".to_string()])]);
                load_clips_by_crc(meta, bank_floats, &[bank], &names, &names_of)?
                    .0
                    .list
                    .into_iter()
                    .find(|clip| clip.name == "takeoff")
            }
            _ => None,
        };
        out.push(CraftRig { class: class_name.to_string(), model, bones, parts: parts.as_ref().clone(), takeoff });
    }
    Ok(out)
}

fn floats(bytes: &[u8]) -> Result<Rc<[f32]>, String> {
    Ok(Rc::from(f32s(bytes).map_err(|e| e.to_string())?))
}

fn indices(bytes: &[u8]) -> Result<Vec<u32>, String> {
    crate::bin_arrays::u32s(bytes).map_err(|e| e.to_string())
}

/// `Object.fromEntries(Object.entries(models).map(([k, m]) => [k, m.parts]))`.
fn parts_records(models: Option<&Value>) -> Value {
    let mut out: Vec<(Rc<str>, Value)> = Vec::new();
    if let Some(o) = models.and_then(Value::as_object) {
        for (k, m) in o.entries() {
            out.push((Rc::from(k), m.get("parts").cloned().unwrap_or(Value::Null)));
        }
    }
    Value::Object(Rc::new(Object::from_written(out)))
}

/// 2005 variant of [`parts_records`] that accepts flat part arrays alongside
/// rig objects (`{ parts, bones, ... }`).
fn parts_records_swbf2(models: Option<&Value>) -> Value {
    let mut out: Vec<(Rc<str>, Value)> = Vec::new();
    if let Some(o) = models.and_then(Value::as_object) {
        for (k, m) in o.entries() {
            let parts = if m.as_array().is_some() {
                m.clone()
            } else {
                m.get("parts").cloned().unwrap_or(Value::Null)
            };
            out.push((Rc::from(k), parts));
        }
    }
    Value::Object(Rc::new(Object::from_written(out)))
}

/// routeWords(bin, routes).
pub fn route_words(bin: Option<&[u8]>, routes: Option<&Value>) -> Option<Result<Vec<u32>, String>> {
    let routes = routes.filter(|r| r.truthy())?;
    let Some(bytes) = bin else { return Some(Err("ai-nav: graph without ai-nav.bin".into())) };
    let offset = match routes.get("byteOffset") {
        None | Some(Value::Null) => 0.0,
        Some(v) => crate::js::to_number(Some(v)),
    };
    let count = crate::js::to_number(routes.get("count"));
    if !(offset >= 0.0 && offset + count * 4.0 <= bytes.len() as f64) {
        return Some(Err(format!("ai-nav: ai-nav.bin holds {} bytes, routes need {}", bytes.len(), offset + count * 4.0)));
    }
    let count = count as usize;
    let at = offset as usize;
    Some(Ok((0..count).map(|i| {
        let o = at + i * 4;
        u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]])
    }).collect()))
}

fn bone_names(actor: &Actor) -> Vec<String> {
    actor.bones.iter().map(|b| b.name.clone()).collect()
}

impl Content {
    pub fn build(battle: Battle) -> Result<Content, String> {
        let b = &battle;
        // Weapon packs (game.js 662-666).
        let mut weapon_packs = Vec::new();
        for side in &b.sides {
            let pack = if b.family == GameFamily::Swbf2 {
                model_pack_swbf2(
                    side.weapons.get("models"),
                    &floats(&side.weapon_buf)?,
                    &indices(&side.weapon_index_buf)?,
                    &format!("{}/weapons.json", side.dir),
                )?
            } else {
                model_pack(side.weapons.get("models"), &floats(&side.weapon_buf)?, &indices(&side.weapon_index_buf)?)?
            };
            weapon_packs.push((side.name.clone(), pack));
        }
        let (vehicle_models, vehicle_extras) = match (&b.vehicle_def, &b.vehicle_bufs) {
            (Some(def), Some(bufs)) => (
                Some(model_pack(Some(&parts_records(def.get("models"))), &floats(&bufs.models)?, &indices(&bufs.models_index)?)?),
                Some(model_pack(def.get("extras"), &floats(&bufs.extras)?, &indices(&bufs.extras_index)?)?),
            ),
            _ => (None, None),
        };
        let props_models = match (&b.props, &b.props_buf, &b.props_index_buf) {
            (Some(props), Some(buf), Some(index)) => {
                if b.family == GameFamily::Swbf2 {
                    let records = parts_records_swbf2(props.get("models"));
                    Some(model_pack_swbf2(
                        Some(&records),
                        &floats(buf)?,
                        &indices(index)?,
                        &format!("maps/{}/props.json", b.map),
                    )?)
                } else {
                    let records = parts_records(props.get("models"));
                    Some(model_pack(Some(&records), &floats(buf)?, &indices(index)?)?)
                }
            }
            _ => None,
        };
        let mut weapon_models = ModelMap::default();
        let mut weapon_model_sources: Vec<(String, String)> = Vec::new();
        for (i, (side, pack)) in weapon_packs.iter().enumerate() {
            for (name, parts) in pack.iter() {
                // The played side's pack is the Map itself; the others fill in.
                if i == 0 || !weapon_models.has(name) {
                    if !weapon_models.has(name) {
                        weapon_model_sources.push((name.to_string(), format!("weapons/{side}")));
                    }
                    weapon_models.set(name, parts.clone());
                }
            }
        }
        if let Some(extras) = &vehicle_extras {
            for (name, parts) in extras.iter() {
                if !weapon_models.has(name) {
                    weapon_model_sources.push((name.to_string(), "vehicles/extras".into()));
                    weapon_models.set(name, parts.clone());
                }
            }
        }
        // Bodies and their clips (clipsFor: one bank a skeleton).
        let mut actors = Vec::new();
        let mut body_clips = Vec::new();
        let mut unit_clips = Vec::new();
        let mut unit_anim_stats = Vec::new();
        if let Some(swbf2) = b.swbf2.as_ref() {
            let common_floats = floats(&swbf2.common_anim_bin)?;
            let common_bank_names: Vec<String> = swbf2
                .common_anim_banks
                .get("banks")
                .and_then(Value::as_object)
                .map(|o| o.keys().filter(|k| k.starts_with("human_")).map(str::to_string).collect())
                .unwrap_or_default();
            let mut side_floats: HashMap<&str, (&Value, Rc<[f32]>)> = HashMap::new();
            for (sname, meta, bin) in &swbf2.side_anim_banks {
                side_floats.insert(sname.as_str(), (meta, floats(bin)?));
            }
            for (key, files) in &b.bodies {
                let actor = build_actor(&files.def, &f32s(&files.buf).map_err(|e| e.to_string())?)?;
                let names = bone_names(&actor);
                let common_clips = if crate::battle::has_human_upper_body_bones(&names) {
                    load_clips_by_crc(
                        &swbf2.common_anim_banks,
                        &common_floats,
                        &common_bank_names,
                        &names,
                        &HashMap::new(),
                    )?
                    .0
                } else {
                    Clips::default()
                };
                body_clips.push((key.clone(), Rc::new(common_clips)));
                actors.push((key.clone(), actor));
            }
            // Bind per-unit animation sets (common + unit-specific side banks) by bone CRC.
            for side in &b.sides {
                let Some(units) = side.units.as_object() else { continue };
                for (class_name, def) in units.entries() {
                    let Some(model) = def.get("model").and_then(Value::as_str) else { continue };
                    let body_key = format!("{}/{model}", side.name);
                    let Some(actor_idx) = actors.iter().position(|(k, _)| *k == body_key) else { continue };
                    let unit_key = format!("{}/{class_name}", side.name);
                    let names = bone_names(&actors[actor_idx].1);
                    let empty_names = HashMap::new();
                    let extra_names = swbf2
                        .unit_clip_names
                        .iter()
                        .find(|(k, _)| *k == unit_key)
                        .map(|(_, m)| m)
                        .unwrap_or(&empty_names);
                    let unit_banks = swbf2
                        .unit_anim_banks
                        .iter()
                        .find(|(k, _)| *k == unit_key)
                        .map(|(_, list)| list.as_slice())
                        .unwrap_or(&[]);
                    let common_wanted: Vec<String> = unit_banks
                        .iter()
                        .filter(|bn| {
                            swbf2
                                .common_anim_banks
                                .get("banks")
                                .and_then(Value::as_object)
                                .is_some_and(|o| o.keys().any(|k| k.eq_ignore_ascii_case(bn)))
                        })
                        .cloned()
                        .collect();
                    let (mut merged, mut stats) = load_clips_by_crc(
                        &swbf2.common_anim_banks,
                        &common_floats,
                        &common_wanted,
                        &names,
                        extra_names,
                    )?;
                    if let Some((s_meta, s_fl)) = side_floats.get(side.name.as_str()) {
                        let side_wanted: Vec<String> = unit_banks
                            .iter()
                            .filter(|bn| {
                                s_meta
                                    .get("banks")
                                    .and_then(Value::as_object)
                                    .is_some_and(|o| o.keys().any(|k| k.eq_ignore_ascii_case(bn)))
                            })
                            .cloned()
                            .collect();
                        if !side_wanted.is_empty() {
                            let (overlay, s_stats) =
                                load_clips_by_crc(s_meta, s_fl, &side_wanted, &names, extra_names)?;
                            merged = merge_clips(&merged, &overlay);
                            stats.banks.extend(s_stats.banks);
                            stats.clips += s_stats.clips;
                            stats.matched_tracks += s_stats.matched_tracks;
                            stats.unmatched_tracks += s_stats.unmatched_tracks;
                        }
                    }
                    let rc = Rc::new(merged);
                    // Update the body's clip bank when this unit added side-specific clips.
                    if !stats.banks.is_empty() {
                        body_clips[actor_idx].1 = Rc::clone(&rc);
                    }
                    unit_clips.push((unit_key.clone(), rc));
                    unit_anim_stats.push((unit_key, stats));
                }
            }
        } else {
            let anim_floats = floats(&b.anim_buf)?;
            let mut banks: Vec<(String, Rc<Clips>)> = Vec::new();
            for (key, files) in &b.bodies {
                let actor = build_actor(&files.def, &f32s(&files.buf).map_err(|e| e.to_string())?)?;
                let names = bone_names(&actor);
                let bank_key = names.join(",");
                let bank = match banks.iter().find(|(k, _)| *k == bank_key) {
                    Some((_, bank)) => bank.clone(),
                    None => {
                        let bank = Rc::new(load_clips(&b.anim_meta, &anim_floats, Some(&names))?);
                        banks.push((bank_key, bank.clone()));
                        bank
                    }
                };
                body_clips.push((key.clone(), bank));
                actors.push((key.clone(), actor));
            }
        }
        // Droideka classes: their own clips on their bodies (buildBody).
        let mut walker_clips = Vec::new();
        for side in &b.sides {
            let Some(units) = side.units.as_object() else { continue };
            for (class, def) in units.entries() {
                if def.get("base").and_then(Value::as_str) != Some("walkerdroid") {
                    continue;
                }
                let model = def.get("model").and_then(Value::as_str).unwrap_or("undefined");
                let clips = def.get("clips").and_then(Value::as_str).unwrap_or("undefined");
                let actor = actors.iter().find(|(k, _)| *k == format!("{}/{model}", side.name)).map(|(_, a)| a);
                let walker = b.walkers.iter().find(|(k, _)| *k == format!("{}/{clips}", side.name)).map(|(_, w)| w);
                if let (Some(actor), Some(walker)) = (actor, walker) {
                    let bank = load_clips(&walker.meta, &floats(&walker.buf)?, Some(&bone_names(actor)))?;
                    walker_clips.push((format!("{}/{class}", side.name), bank));
                }
            }
        }
        let collision = match b.swbf2.as_ref() {
            Some(swbf2) => swbf2.map_admission.collision.clone(),
            None => MapCollision::load(&b.coll_buf, &b.coll_index_buf, &b.coll_poly_buf, &b.ground_buf, &b.ground_index_buf)?,
        };
        // The vehicle pack's templates (vehicles.js 1971-2004).
        let mut vehicle_templates = Vec::new();
        let mut vehicle_blockers = None;
        if let (Some(def), Some(models)) = (&b.vehicle_def, &vehicle_models) {
            let animation_floats: Rc<[f32]> = match &b.vehicle_bufs.as_ref().and_then(|v| v.animations.clone()) {
                Some(bytes) => floats(bytes)?,
                None => Rc::from(Vec::new()),
            };
            for (name, rig) in def.get("models").and_then(Value::as_object).map(|o| o.entries().collect::<Vec<_>>()).unwrap_or_default() {
                let col = def.get("collision").and_then(|c| c.get(name)).ok_or_else(|| format!("vehicle {name} has no collision"))?;
                let shape = json_shape(col);
                let parts = models.get(name).cloned().unwrap_or_default();
                let wheel_radii = crate::derived::wheel_radii(&parts);
                let muzzle_tips = crate::derived::vehicle_muzzle_tips(rig, &parts);
                let clips = match def.get("animations").and_then(|a| a.get(name)).filter(|m| m.truthy()) {
                    Some(meta) => {
                        let names: Vec<String> = crate::derived::rig_bones(rig).into_iter().map(|b| b.name).collect();
                        let mut clips = load_clips(meta, &animation_floats, Some(&names))?;
                        keep_last_tracks(&mut clips);
                        Some(clips)
                    }
                    None => None,
                };
                // A flyer with a takeoff clip stands on its gear (vehicles.js 2002-2004).
                let gear_foot = match clips.as_ref().and_then(|c| c.get("takeoff")) {
                    Some(takeoff) => crate::gear::gear_foot(rig, &crate::gear::vehicle_batches(rig, &parts), takeoff),
                    None => None,
                };
                vehicle_templates.push(VehicleTemplate { name: name.to_string(), rig: rig.clone(), parts, shape, clips, wheel_radii, muzzle_tips, gear_foot });
            }
            if let Some(blockers) = def.get("blockers").filter(|x| x.truthy()) {
                let has = blockers.get("indices").and_then(Value::as_array).map(|a| !a.is_empty()).unwrap_or(false);
                if has {
                    vehicle_blockers = Some(json_shape(blockers));
                }
            }
        }
        // The props' bodies (props.js 1486-1539) and clips (clipsFor).
        let mut prop_bodies = Vec::new();
        let mut prop_clips: Vec<(String, Option<Clips>)> = Vec::new();
        if let Some(props) = &b.props {
            // 1229-1231: present with props_collision.bin; a missing
            // index or polygon file is `new Uint32Array(null)`, empty.
            let triangles = match &b.props_coll_buf {
                Some(p) => Some((
                    floats(p)?,
                    match &b.props_coll_index_buf { Some(i) => indices(i)?, None => Vec::new() },
                    match &b.props_coll_poly_buf { Some(g) => indices(g)?, None => Vec::new() },
                )),
                None => None,
            };
            let models = props.get("models");
            let anim_floats: Option<Rc<[f32]>> = match &b.props_anim_buf {
                Some(bytes) => Some(floats(bytes)?),
                None => None,
            };
            let mut clips_for = |model: &str| -> Result<(), String> {
                if prop_clips.iter().any(|(m, _)| m == model) {
                    return Ok(());
                }
                let meta = props.get("animations").and_then(|a| a.get(model)).filter(|m| m.truthy());
                let clips = match (meta, &anim_floats) {
                    (Some(meta), Some(fl)) => {
                        let names: Vec<String> = models
                            .and_then(|m| m.get(model))
                            .and_then(|m| m.get("bones"))
                            .and_then(Value::as_array)
                            .unwrap_or(&[])
                            .iter()
                            .map(|b| b.get("name").and_then(Value::as_str).unwrap_or("").to_string())
                            .collect();
                        Some(load_clips(meta, fl, Some(&names))?)
                    }
                    _ => None,
                };
                prop_clips.push((model.to_string(), clips));
                Ok(())
            };
            if b.family == GameFamily::Swbf2 {
                let prop_masks: &[u8] = b
                    .swbf2
                    .as_ref()
                    .and_then(|s| s.map_admission.props_coll_mask_buf.as_deref())
                    .unwrap_or(&[]);
                if let Some((p, ix, _)) = triangles.as_ref() {
                    if !p.len().is_multiple_of(3) {
                        return Err(format!("maps/{}/props_collision.bin: vertex count is not a multiple of 3 floats", b.map));
                    }
                    if !ix.len().is_multiple_of(3) {
                        return Err(format!("maps/{}/props_collision_index.bin: index count is not a multiple of 3", b.map));
                    }
                }
                for (i, desc) in props
                    .get("instances")
                    .and_then(Value::as_array)
                    .unwrap_or(&[])
                    .iter()
                    .enumerate()
                {
                    let model = desc.get("model").and_then(Value::as_str).unwrap_or("undefined");
                    if models.and_then(|m| m.get(model)).map(Value::truthy) != Some(true) {
                        continue;
                    }
                    let base = desc.get("base").and_then(Value::as_str).unwrap_or("");
                    let inst_name = desc.get("name").and_then(Value::as_str).unwrap_or(model);
                    let world_animated = b.swbf2.as_ref().is_some_and(|s| {
                        s.map_admission.world_animations.animates_object(inst_name)
                    });
                    let matrix_arr = desc
                        .get("matrix")
                        .and_then(Value::as_array)
                        .filter(|m| m.len() >= 16)
                        .ok_or_else(|| format!("maps/{}/props.json: instance '{inst_name}' has no 16-element matrix", b.map))?;
                    let mut matrix = [0.0f64; 16];
                    for (k, slot) in matrix.iter_mut().enumerate() {
                        let v = crate::js::to_number(matrix_arr.get(k));
                        if !v.is_finite() {
                            return Err(format!("maps/{}/props.json: instance '{inst_name}' matrix[{k}] is non-finite", b.map));
                        }
                        *slot = v;
                    }
                    let bake = |look: &str, range: Option<&Value>| -> Result<Option<BodyTriangles>, String> {
                        let Some(range) = range.filter(|r| r.truthy()) else {
                            return Ok(None);
                        };
                        let Some((p, ix, pg)) = triangles.as_ref() else {
                            return Err(format!("maps/{}/props_collision.bin: missing buffer for instance '{inst_name}' ({look})", b.map));
                        };
                        let label = format!("maps/{}/props.json instance '{inst_name}' ({look})", b.map);
                        if world_animated {
                            range_shape_local_swbf2(p, ix, pg, prop_masks, range, &label)
                        } else {
                            range_shape_transformed_swbf2(p, ix, pg, prop_masks, range, &matrix, &label)
                        }
                    };
                    match base {
                        "trap" => {
                            clips_for(model)?;
                        }
                        "animatedprop" | "door" => {
                            prop_bodies.push(PropBodies {
                                instance: i,
                                world_animated,
                                built: bake("built", desc.get("collision"))?,
                                destroyed: None,
                                unbuilt: None,
                            });
                            clips_for(model)?;
                        }
                        "powerupstation" | "armedbuilding" | "commandpost" | "destructablebuilding"
                        | "remoteterminal" | "prop" => {
                            if base == "commandpost" {
                                clips_for(model)?;
                            }
                            prop_bodies.push(PropBodies {
                                instance: i,
                                world_animated,
                                built: bake("built", desc.get("collision"))?,
                                destroyed: bake("destroyed", desc.get("destroyed").and_then(|d| d.get("collision")))?,
                                unbuilt: bake("unbuilt", desc.get("unbuilt").and_then(|d| d.get("collision")))?,
                            });
                        }
                        _ => {}
                    }
                }
            } else {
                for (i, desc) in props.get("instances").and_then(Value::as_array).unwrap_or(&[]).iter().enumerate() {
                    let model = desc.get("model").and_then(Value::as_str).unwrap_or("undefined");
                    if models.and_then(|m| m.get(model)).map(Value::truthy) != Some(true) {
                        continue;
                    }
                    let base = desc.get("base").and_then(Value::as_str).unwrap_or("");
                    let body = |range: Option<&Value>| -> Option<BodyTriangles> {
                        let (p, ix, pg) = triangles.as_ref()?;
                        range_shape(p, ix, pg, range?)
                    };
                    match base {
                        "trap" => {
                            clips_for(model)?;
                        }
                        "animatedprop" | "door" => {
                            prop_bodies.push(PropBodies { instance: i, world_animated: false, built: body(desc.get("collision")), destroyed: None, unbuilt: None });
                            clips_for(model)?;
                        }
                        "powerupstation" | "armedbuilding" | "commandpost" => {
                            if base == "commandpost" {
                                clips_for(model)?;
                            }
                            prop_bodies.push(PropBodies {
                                instance: i,
                                world_animated: false,
                                built: body(desc.get("collision")),
                                destroyed: body(desc.get("destroyed").and_then(|d| d.get("collision"))),
                                unbuilt: body(desc.get("unbuilt").and_then(|d| d.get("collision"))),
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
        // spawnPlacedItems.
        let mut placed_items = Vec::new();
        for (index, entry) in b.props.as_ref().and_then(|p| p.get("items")).and_then(Value::as_array).unwrap_or(&[]).iter().enumerate() {
            let Some(item) = entry.get("item").filter(|i| i.truthy()) else { continue };
            let Some(matrix) = entry.get("matrix").and_then(Value::as_array).filter(|m| m.len() == 16) else { continue };
            let e = |i: usize| crate::js::to_number(matrix.get(i));
            let team = match entry.get("team") {
                Some(t) if t.truthy() => t.clone(),
                _ => Value::Number(0.0),
            };
            placed_items.push(PlacedItem {
                index,
                item: item.clone(),
                position: crate::three::Vector3::new(e(12), e(13), e(14)),
                yaw: crate::js::js_atan2(e(8), e(10)),
                team,
            });
        }
        let death_regions = death_regions(&b.world);
        let route_words = route_words(b.nav_bin.as_deref(), b.nav.as_ref().and_then(|n| n.get("graph")).filter(|g| g.truthy()).and_then(|g| g.get("routes")));
        Ok(Content {
            weapon_packs,
            vehicle_models,
            vehicle_extras,
            props_models,
            weapon_models,
            weapon_model_sources,
            actors,
            body_clips,
            unit_clips,
            unit_anim_stats,
            walker_clips,
            collision,
            vehicle_templates,
            vehicle_blockers,
            prop_bodies,
            prop_clips,
            death_regions,
            route_words,
            placed_items,
            battle,
        })
    }

    pub fn actor(&self, key: &str) -> Option<&Actor> {
        self.actors.iter().find(|(k, _)| k == key).map(|(_, a)| a)
    }
}
