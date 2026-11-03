//! A battle's content decoded: the model packs and the battle's weapon
//! models (game.js's merge), the bodies and their clip banks, droideka
//! clips, the map's collision, the vehicle pack's templates and the props'
//! models, clips and bodies, the death regions, the AI's route table. Built
//! from a loaded Battle (src/battle.rs) in game.js's order.

use crate::actor::{build_actor, Actor};
use crate::battle::Battle;
use crate::bin_arrays::f32s;
use crate::clips::{keep_last_tracks, load_clips, Clips};
use crate::collision::{json_shape, range_shape, BodyTriangles, MapCollision};
use crate::models::{model_pack, ModelMap};
use crate::regions::{death_regions, BuiltRegion};
use crate::value::{Object, Value};
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
    /// flight/gear.js gearFoot: how far below its origin it stands on its
    /// gear (a craft with a takeoff clip), else None.
    pub gear_foot: Option<f64>,
}

/// A prop instance's collision bodies (props.js `body(range)`), by look.
pub struct PropBodies {
    pub instance: usize,
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
    /// game.js weaponModels: the played side's pack, the other sides'
    /// (first name wins), then the vehicle pack's extras (a headless world
    /// has no turret extras). Values say which pack each came from.
    pub weapon_models: ModelMap,
    pub weapon_model_sources: Vec<(String, String)>,
    /// Bodies by "<side>/<model>".
    pub actors: Vec<(String, Actor)>,
    /// The shared clips each body plays (anims.json on its bones), by body.
    pub body_clips: Vec<(String, Rc<Clips>)>,
    /// Droideka classes' own clips on their bodies, by "<side>/<class>".
    pub walker_clips: Vec<(String, Clips)>,
    pub collision: MapCollision,
    pub vehicle_templates: Vec<VehicleTemplate>,
    pub vehicle_blockers: Option<BodyTriangles>,
    pub prop_bodies: Vec<PropBodies>,
    /// props.js clipsFor: a model's clips, by model, in first-use order.
    pub prop_clips: Vec<(String, Option<Clips>)>,
    pub death_regions: Vec<(Value, BuiltRegion)>,
    /// ai-nav.bin's routing words (nav/data.js routeWords).
    pub route_words: Option<Result<Vec<u32>, String>>,
    /// The placed power-ups a world spawns at the start.
    pub placed_items: Vec<PlacedItem>,
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

/// nav/data.js routeWords(bin, routes).
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
            let pack = model_pack(side.weapons.get("models"), &floats(&side.weapon_buf)?, &indices(&side.weapon_index_buf)?)?;
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
            (Some(props), Some(buf), Some(index)) => Some(model_pack(Some(&parts_records(props.get("models"))), &floats(buf)?, &indices(index)?)?),
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
        // Bodies and their clips (units.js clipsFor: one bank a skeleton).
        let anim_floats = floats(&b.anim_buf)?;
        let mut actors = Vec::new();
        let mut banks: Vec<(String, Rc<Clips>)> = Vec::new();
        let mut body_clips = Vec::new();
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
        // Droideka classes: their own clips on their bodies (units.js buildBody).
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
        let collision = MapCollision::load(&b.coll_buf, &b.coll_index_buf, &b.coll_poly_buf, &b.ground_buf, &b.ground_index_buf)?;
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
            // game.js 1229-1231: present with props_collision.bin; a missing
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
                        prop_bodies.push(PropBodies { instance: i, built: body(desc.get("collision")), destroyed: None, unbuilt: None });
                        clips_for(model)?;
                    }
                    "powerupstation" | "armedbuilding" | "commandpost" => {
                        if base == "commandpost" {
                            clips_for(model)?;
                        }
                        prop_bodies.push(PropBodies {
                            instance: i,
                            built: body(desc.get("collision")),
                            destroyed: body(desc.get("destroyed").and_then(|d| d.get("collision"))),
                            unbuilt: body(desc.get("unbuilt").and_then(|d| d.get("collision"))),
                        });
                    }
                    _ => {}
                }
            }
        }
        // placed-items.js spawnPlacedItems.
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
