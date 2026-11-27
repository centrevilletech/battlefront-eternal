//! gameplay-only fingerprint. Native and browser clients
//! hash their own prepared files; a server's advertised digest never substitutes
//! for missing local content. Text formatting and incidental bone order do not
//! change the agreement that gates spawning.
use bfe_content::{files::Files,json,map::pick_swbf2_mission_strict,mission::{Query,select_mission,utf16_cmp},GameFamily,Value};
use sha2::{Sha256,Digest};
use std::{collections::{BTreeMap,BTreeSet},rc::Rc};
mod scripts;
mod order;
mod geometry;
fn digest(bytes:&[u8])->String{Sha256::digest(bytes).iter().map(|n|format!("{n:02x}")).collect()}
fn string(text:&str,out:&mut String){out.push_str(&serde_json::to_string(text).expect("UTF-8 JSON string"));}
fn canonical(value:&Value,out:&mut String,depth:usize)->Result<(),String>{
    if depth>256{return Err("gameplay content nesting exceeds bounds".into());}
    match value{
        Value::Null=>out.push_str("null"),Value::Bool(v)=>out.push_str(if *v{"true"}else{"false"}),
        Value::Number(v)=>{if !v.is_finite(){return Err("invalid gameplay JSON number".into());}
            if *v==0.&&v.is_sign_negative(){out.push_str("-0");}else{out.push_str(&jsrt::numfmt::number_to_string(*v));}},
        Value::String(v)=>string(v,out),Value::Array(values)=>{out.push('[');for (i,v)in values.iter().enumerate(){if i>0{out.push(',');}canonical(v,out,depth+1)?;}out.push(']');},
        Value::Object(values)=>{let mut keys=values.keys().collect::<Vec<_>>();keys.sort_by(|a,b|utf16_cmp(a,b));out.push('{');
            for (i,key)in keys.iter().enumerate(){if i>0{out.push(',');}string(key,out);out.push(':');canonical(values.get(key).unwrap(),out,depth+1)?;}out.push('}');},
    }Ok(())
}
pub fn canonical_bytes(bytes:&[u8])->Result<Vec<u8>,String>{let value=json::parse_bytes(bytes).map_err(|e|e.to_string())?;serialize(&value)}
fn serialize(value:&Value)->Result<Vec<u8>,String>{let mut out=String::new();canonical(value,&mut out,0)?;Ok(out.into_bytes())}
fn parsed(bytes:&[u8])->Result<Value,String>{json::parse_bytes(bytes).map_err(|e|e.to_string())}
// Animation tracks name their bones in the fingerprint. Importers may rebase
// the global table while leaving every named track and clip byte unchanged.
fn animation_bytes(bytes:&[u8])->Result<Vec<u8>,String>{
    let value=parsed(bytes)?;let bones=value.get("bones").and_then(Value::as_array).ok_or("invalid animation bones")?;
    let mut names=Vec::with_capacity(bones.len());let mut unique=BTreeSet::new();for bone in bones{let name=bone.as_str().ok_or("invalid animation bone")?;if !unique.insert(name){return Err("duplicate animation bone".into());}names.push(name);}
    let clips=value.get("clips").and_then(Value::as_object).ok_or("invalid animation clips")?;let mut transformed=Vec::new();
    for (name,clip)in clips.entries(){let mut object=clip.as_object().ok_or("invalid animation clip")?.clone();let tracks=clip.get("tracks").and_then(Value::as_array).ok_or("invalid animation tracks")?;
        let mut named=Vec::with_capacity(tracks.len());for track in tracks{let n=track.as_f64().ok_or("invalid animation track")?;if n<0.||n.fract()!=0.||n>=names.len()as f64{return Err("invalid animation track".into());}named.push(Value::str(names[n as usize]));}
        object.set(Rc::from("tracks"),Value::Array(Rc::new(named)));transformed.push((Rc::from(name),Value::Object(Rc::new(object))));}
    names.sort_by(|a,b|utf16_cmp(a,b));let mut object=value.as_object().ok_or("invalid animation table")?.clone();
    object.set(Rc::from("bones"),Value::Array(Rc::new(names.into_iter().map(Value::str).collect())));
    object.set(Rc::from("clips"),Value::Object(Rc::new(bfe_content::value::Object::from_written(transformed))));serialize(&Value::Object(Rc::new(object)))
}
// Aircraft takeoff clips are cosmetic for client compatibility, as in the
// browser fingerprint. Shared walker clips remain part of gameplay agreement.
fn vehicle_bytes(bytes:&[u8])->Result<Vec<u8>,String>{
    let value=parsed(bytes)?;let Some(anims)=value.get("animations").and_then(Value::as_object)else{return serialize(&value);};
    let Some(classes)=value.get("classes").and_then(Value::as_object)else{return serialize(&value);};let mut flyers=BTreeMap::new();
    for def in classes.values(){if let Some(model)=def.get("model").and_then(Value::as_str){let only=flyers.entry(model).or_insert(true);*only&=def.get("base").and_then(Value::as_str)==Some("flyer");}}
    let keep=anims.entries().filter(|(model,_)|!flyers.get(model).copied().unwrap_or(false)).map(|(k,v)|(Rc::from(k),v.clone())).collect();
    let mut object=value.as_object().unwrap().clone();object.set(Rc::from("animations"),Value::Object(Rc::new(bfe_content::value::Object::from_written(keep))));serialize(&Value::Object(Rc::new(object)))
}
fn valid_name(name:&str)->bool{!name.is_empty()&&name.len()<=64&&name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')}
pub fn manifest(files:&dyn Files,query:&str)->Result<Vec<(String,Option<String>)>,String>{
    let query=Query::parse(query);let map=if query.get("map").is_empty(){"nab2"}else{query.get("map")};let side=if query.get("side").is_empty(){"rep"}else{query.get("side")};
    if !valid_name(map)||!valid_name(side){return Err("invalid content selection".into());}
    let world_path=format!("/maps/{map}/map.json");let raw=files.read(&world_path).ok_or("missing map content")?;let world=select_mission(&parsed(&raw)?,&query);
    let mission=world.get("mission");let mut sides=BTreeSet::new();sides.insert(side.to_string());
    if let Some(names)=mission.and_then(|m|m.get("sides")).and_then(Value::as_array){for name in names{let name=name.as_str().ok_or("invalid mission side")?;if !valid_name(name){return Err("invalid mission side".into());}sides.insert(name.to_string());}}
    let vehicles=world.get("vehicles").and_then(Value::as_str).unwrap_or("vehicles/vehicles.json");let parts=vehicles.split('/').collect::<Vec<_>>();
    if parts.first()!=Some(&"vehicles")||parts.last()!=Some(&"vehicles.json")||!(parts.len()==2||parts.len()==3&&valid_name(parts[1])){return Err("invalid vehicle pack".into());}
    let mut paths=vec!["/common/common.json".into(),"/common/anims.json".into(),"/common/anims.bin".into()];
    if world.get("family").and_then(Value::as_str) == Some("swbf2") {
        paths.extend(scripts::paths(&world, map)?);
    }
    for name in ["ai-mission.json","ai-nav.json","ai-nav.bin","collision.bin","collision_index.bin","collision_poly.bin","terrain_collision.bin","terrain_collision_index.bin","props.json","props_anims.bin","props_collision.bin","props_collision_index.bin","props_collision_poly.bin",vehicles]{paths.push(format!("/maps/{map}/{name}"));}
    for side in sides{for name in ["units.json","weapons.json","ai.json"]{paths.push(format!("/sides/{side}/{name}"));}}paths.sort();
    let mission_name=mission.and_then(|m|m.get("name")).and_then(Value::as_str).map(str::to_owned);
    let mut result=vec![(world_path,Some(digest(&canonical_bytes(&raw)?))),("mission".into(),mission_name)];let vehicle_path=format!("/maps/{map}/{vehicles}");
    for path in paths{let hash=if let Some(bytes)=files.read(&path){let bytes=if path=="/common/anims.json"{animation_bytes(&bytes)?}else if path==vehicle_path{vehicle_bytes(&bytes)?}else if path.ends_with(".json"){canonical_bytes(&bytes)?}else{bytes.to_vec()};digest(&bytes)}else{"missing".into()};result.push((path,Some(hash)));}
    result.sort_by(|a,b|utf16_cmp(&a.0,&b.0));Ok(result)
}
pub fn hash(files:&dyn Files,query:&str)->Result<String,String>{let manifest=manifest(files,query)?;let bytes=serde_json::to_vec(&manifest).map_err(|e|e.to_string())?;Ok(format!("bfe-content-sha256-{}",digest(&bytes)))}

fn validate_rel_path(rel: &str, label: &str) -> Result<(), String> {
    if rel.is_empty()
        || rel.starts_with('/')
        || rel.contains('\\')
        || rel.contains(':')
        || rel
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        return Err(format!("invalid {label} path"));
    }
    Ok(())
}

fn resolve_swbf2_layer_key(world: &Value, wanted: &str) -> Option<String> {
    let layers = world.get("layers").and_then(Value::as_object)?;
    if layers.has(wanted) {
        return Some(wanted.to_string());
    }
    if let Some(k) = layers.keys().find(|k| k.eq_ignore_ascii_case(wanted)) {
        return Some(k.to_string());
    }
    if let Some(idx_hit) = world
        .get("layerIndex")
        .and_then(Value::as_object)
        .and_then(|idx| {
            idx.get(wanted)
                .or_else(|| {
                    idx.entries()
                        .find(|(k, _)| k.eq_ignore_ascii_case(wanted))
                        .map(|(_, v)| v)
                })
                .and_then(Value::as_str)
        })
    {
        if layers.has(idx_hit) {
            return Some(idx_hit.to_string());
        }
        if let Some(k) = layers.keys().find(|k| k.eq_ignore_ascii_case(idx_hit)) {
            return Some(k.to_string());
        }
    }
    for (k, lv) in layers.entries() {
        if lv
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|n| n.eq_ignore_ascii_case(wanted))
            || lv
                .get("hash")
                .and_then(Value::as_str)
                .is_some_and(|h| h.eq_ignore_ascii_case(wanted))
        {
            return Some(k.to_string());
        }
    }
    None
}

/// Family-aware gameplay manifest.
///
/// For `GameFamily::Swbf1`, delegates directly to [`manifest`] with no changes.
/// For `GameFamily::Swbf2`, hashes the family tag, selected mission, canonical
/// `map.json`, active layer collision and mask streams, base/terrain/prop
/// collision and masks, `props.json`/`props.bin`/`props_anims.bin`, map
/// classes/planning/vehicle pack, common and side gameplay tables
/// (`units.json`, `weapons.json`, `weapons.bin`, `combos.json`, `classes.json`,
/// `ai.json`, `anim_banks.{json,bin}`, referenced actor `.json`/`.bin`), and
/// prepared mission scripts via `scripts::paths`.
pub fn manifest_for_family(
    files: &dyn Files,
    family: GameFamily,
    query: &str,
) -> Result<Vec<(String, Option<String>)>, String> {
    if family == GameFamily::Swbf1 {
        return manifest(files, query);
    }
    let q = Query::parse(query);
    let map = if q.get("map").is_empty() {
        "tat2"
    } else {
        q.get("map")
    };
    if !valid_name(map) {
        return Err("invalid content selection".into());
    }
    let world_path = format!("/maps/{map}/map.json");
    let raw = files.read(&world_path).ok_or("missing map content")?;
    let world = parsed(&raw)?;
    if world.as_object().is_none() {
        return Err("invalid map content".into());
    }
    let mission_name = pick_swbf2_mission_strict(&world, &world_path, None, Some(&q))?;
    let mission_rec = mission_name
        .as_deref()
        .and_then(|m| world.get("missions").and_then(|ms| ms.get(m)));

    let mut paths: BTreeSet<String> = BTreeSet::new();
    let mut raw_paths = BTreeSet::new();
    paths.insert("/common/common.json".into());
    paths.insert("/common/anim_banks.json".into());
    paths.insert("/common/anim_banks.bin".into());

    // Prepared mission scripts (shared mechanism with `fingerprint/scripts.rs`).
    paths.extend(scripts::paths(&world, map)?);

    // Base map gameplay data, collision, masks, classes, planning, and vehicles.
    for name in [
        "collision.bin",
        "collision_index.bin",
        "collision_poly.bin",
        "collision_mask.bin",
        "terrain_collision.bin",
        "terrain_collision_index.bin",
        "props.json",
        "props.bin",
        "props_anims.bin",
        "props_collision.bin",
        "props_collision_index.bin",
        "props_collision_poly.bin",
        "props_collision_mask.bin",
        "classes.json",
        "anim_banks.json",
        "anim_banks.bin",
    ] {
        paths.insert(format!("/maps/{map}/{name}"));
    }
    let planning_file = world
        .get("planningGroups")
        .and_then(|p| p.get("data"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("planning.bin");
    validate_rel_path(planning_file, "planning")?;
    let planning_path = format!("/maps/{map}/{planning_file}");
    raw_paths.insert(planning_path.clone());
    paths.insert(planning_path);

    let vehicles = world
        .get("vehicles")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("vehicles/vehicles.json");
    let vparts = vehicles.split('/').collect::<Vec<_>>();
    if vparts.first() != Some(&"vehicles")
        || vparts.last() != Some(&"vehicles.json")
        || !(vparts.len() == 2 || vparts.len() == 3 && valid_name(vparts[1]))
    {
        return Err("invalid vehicle pack".into());
    }
    paths.insert(format!("/maps/{map}/{vehicles}"));

    let mut active_layers = Vec::new();
    let mut selections = BTreeMap::new();

    // Active layer collision & mask streams.
    if let Some(mlayers) = mission_rec.and_then(|m| m.get("layers")).and_then(Value::as_array) {
        let layers_obj = world.get("layers").and_then(Value::as_object);
        for l in mlayers {
            let wanted = l.as_str().ok_or("invalid mission layer")?;
            let resolved = resolve_swbf2_layer_key(&world, wanted)
                .ok_or_else(|| format!("unknown mission layer '{wanted}'"))?;
            if active_layers.iter().any(|name: &String| name.eq_ignore_ascii_case(&resolved)) { continue; }
            active_layers.push(resolved.clone());
            let declared_coll = layers_obj
                .and_then(|o| o.get(&resolved))
                .and_then(|lv| lv.get("collision"))
                .and_then(Value::as_str);
            let coll_path = if let Some(rel) = declared_coll {
                validate_rel_path(rel, "layer collision")?;
                format!("/maps/{map}/{rel}")
            } else {
                format!("/maps/{map}/layers/{resolved}/collision.bin")
            };
            let stem = coll_path.strip_suffix(".bin").unwrap_or(&coll_path);
            raw_paths.insert(coll_path.clone());
            paths.insert(coll_path.clone());
            paths.insert(format!("{stem}_index.bin"));
            paths.insert(format!("{stem}_poly.bin"));
            paths.insert(format!("{stem}_mask.bin"));
        }
    }

    // Sides, per-side animation banks, combos, classes, and referenced actor definitions.
    let mut sides: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Some(sides_map) = mission_rec.and_then(|m| m.get("sides")).and_then(Value::as_object) {
        for (sname, list) in sides_map.entries() {
            if !valid_name(sname) {
                return Err("invalid mission side".into());
            }
            let pkgs = list
                .as_array()
                .ok_or("invalid mission side packages")?
                .iter()
                .map(|v| v.as_str().map(str::to_string).ok_or("invalid mission package"))
                .collect::<Result<Vec<_>, _>>()?;
            sides.insert(sname.to_string(), pkgs);
        }
    }
    if sides.is_empty() {
        let fallback = if q.get("side").is_empty() {
            "rep"
        } else {
            q.get("side")
        };
        if !valid_name(fallback) {
            return Err("invalid content selection".into());
        }
        sides.insert(fallback.to_ascii_lowercase(), Vec::new());
    }
    let mut geometry = geometry::Geometry::default();
    let props_path = format!("/maps/{map}/props.json");
    let props = files.read(&props_path).as_deref().map(parsed).transpose()?;
    if let Some(items) = props.as_ref().and_then(|p| p.get("items")).and_then(Value::as_array) {
        for entry in items {
            if let Some(item) = entry.get("item") { geometry.item(item); }
        }
    }
    let base_placed = props.as_ref().and_then(|p| p.get("instances")).filter(|v| !v.is_null())
        .or_else(|| world.get("placedObjects"));
    let layer_placed = active_layers.iter().filter_map(|name| world.get("layers")?.get(name)?.get("placedObjects"));
    for entries in base_placed.into_iter().chain(layer_placed) {
        for inst in entries.as_array().unwrap_or(&[]) {
            if let Some(weapon) = inst.get("weapon") { geometry.weapon(weapon); }
        }
    }
    let common_meta = files.read("/common/anim_banks.json").as_deref().map(parsed).transpose()?;
    let common_banks: Vec<_> = order::keys(common_meta.as_ref().and_then(|v| v.get("banks")))
        .into_iter().filter(|k| k.starts_with("human_")).map(str::to_string).collect();
    for (side, pkgs) in &sides {
        for name in [
            "units.json",
            "weapons.json",
            "weapons.bin",
            "combos.json",
            "classes.json",
            "ai.json",
            "anim_banks.json",
            "anim_banks.bin",
        ] {
            paths.insert(format!("/sides/{side}/{name}"));
        }
        raw_paths.insert(format!("/sides/{side}/anim_banks.bin"));
        let units_path = format!("/sides/{side}/units.json");
        if let Some(ubytes) = files.read(&units_path) {
            let uval = parsed(&ubytes)?;
            if let Some(items) = uval.get("items").and_then(Value::as_object) {
                for item in items.values() { geometry.item(item); }
            }
            if let Some(units_obj) = uval.get("units").and_then(Value::as_object) {
                let classes = bfe_content::battle::swbf2_spawn_classes(units_obj, pkgs, &q, |_| {});
                let mut units = Vec::with_capacity(classes.len());
                for name in &classes {
                    let def = bfe_content::battle::materialize_swbf2_unit(units_obj.get(name).unwrap());
                    for weapon in def.get("weapons").and_then(Value::as_array).unwrap_or(&[]) {
                        geometry.weapon(weapon);
                    }
                    if let Some(actor_rel) = def
                        .get("actor")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        validate_rel_path(actor_rel, "actor")?;
                        let stem = actor_rel.strip_suffix(".json").unwrap_or(actor_rel);
                        paths.insert(format!("/sides/{side}/{stem}.json"));
                        paths.insert(format!("/sides/{side}/{stem}.bin"));
                    }
                    units.push((name.clone(), def));
                }
                let bank_path = format!("/sides/{side}/anim_banks.json");
                let banks = files.read(&bank_path).as_deref().map(parsed).transpose()?.unwrap_or(Value::Null);
                selections.insert(units_path, order::units(side, uval.get("items"), &units, &common_banks, &banks)?);
                geometry.units.insert(side.clone(), units);
            }
        }
    }

    let mut side_order = order::keys(mission_rec.and_then(|m| m.get("sides")));
    if side_order.is_empty() { side_order.extend(sides.keys().map(String::as_str)); }
    let mesh_hashes = geometry.hashes(files, &side_order)?;
    let mut result = vec![
        ("family".into(), Some("swbf2".into())),
        (world_path.clone(), Some(if raw_paths.contains(&world_path) {
            digest(&raw)
        } else {
            order::hash(&world, &order::map(&world, mission_rec, &active_layers))?
        })),
        ("mission".into(), mission_name),
    ];
    if !q.get("practiceTarget").is_empty() {
        result.push(("practiceUnit".into(), Some(q.get("unit").to_owned())));
        result.push(("practiceTarget".into(), Some(q.get("practiceTarget").to_owned())));
    }
    paths.remove(&world_path);
    for path in paths {
        let hash = if let Some(bytes) = files.read(&path) {
            if raw_paths.contains(&path) {
                digest(&bytes)
            } else if let Some(hash) = mesh_hashes.get(&path) {
                hash.clone()
            } else if path.ends_with(".json") {
                let value = parsed(&bytes)?;
                let selection = if path.ends_with("/anim_banks.json") && !path.starts_with("/maps/") {
                    Some(order::banks(&value, path.starts_with("/common/")))
                } else if path == props_path {
                    Some(order::props(&value))
                } else {
                    selections.remove(&path)
                };
                match selection {
                    Some(selection) => order::hash(&value, &selection)?,
                    None => digest(&serialize(&value)?),
                }
            } else {
                digest(&bytes)
            }
        } else {
            "missing".into()
        };
        result.push((path, Some(hash)));
    }
    result.sort_by(|a, b| utf16_cmp(&a.0, &b.0));
    Ok(result)
}

/// Family-aware gameplay content fingerprint.
///
/// For `GameFamily::Swbf1`, returns the exact same hash as [`hash`].
pub fn hash_for_family(
    files: &dyn Files,
    family: GameFamily,
    query: &str,
) -> Result<String, String> {
    if family == GameFamily::Swbf1 {
        return hash(files, query);
    }
    FAMILY_DIGESTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let manifest = manifest_for_family(files, family, query)?;
    let bytes = serde_json::to_vec(&manifest).map_err(|e| e.to_string())?;
    Ok(format!("bfe-content-sha256-{}", digest(&bytes)))
}

/// How many Battlefront II content digests this process has begun: a
/// server computes one per battle start, which its tests count.
static FAMILY_DIGESTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// [`hash_for_family`]'s count of Battlefront II digests in this process.
pub fn family_digests() -> usize {
    FAMILY_DIGESTS.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod family_tests {
    use super::*;
    use bfe_content::files::MemFiles;

    const SCRIPT_A: &[u8] = include_bytes!("../../../tests/fixtures/mission-scripts/bobbin.luac");
    const SCRIPT_B: &[u8] = include_bytes!("../../../tests/fixtures/mission-scripts/weft.luac");

    #[test]
    fn swbf1_for_family_matches_released_hash_exactly() {
        let mut files = MemFiles::default();
        files.insert(
            "maps/nab2/map.json",
            br#"{"missions":{"nab2a":{"mission":{"name":"nab2a","sides":["rep"]}}}}"#.to_vec(),
        );
        files.insert("common/common.json", br#"{"a":1}"#.to_vec());
        files.insert("sides/rep/units.json", br#"{"units":{}}"#.to_vec());
        let h_released = hash(&files, "?map=nab2&side=rep").unwrap();
        let h_family = hash_for_family(&files, GameFamily::Swbf1, "?map=nab2&side=rep").unwrap();
        assert_eq!(h_released, h_family);
        assert_eq!(
            manifest(&files, "?map=nab2&side=rep").unwrap(),
            manifest_for_family(&files, GameFamily::Swbf1, "?map=nab2&side=rep").unwrap()
        );
    }

    #[test]
    fn swbf2_fingerprint_covers_anim_banks_layers_combos_vehicles_and_scripts() {
        let sha = digest(SCRIPT_A);
        let script_path = format!("scripts/{sha}.luac");
        let manifest_path = "maps/arena/scripts/cw-ass.json";
        let make_files = || {
            let mut files = MemFiles::default();
            files.insert(
                "maps/arena/map.json",
                format!(
                    r#"{{
                        "family": "swbf2",
                        "vehicles": "vehicles/vehicles.json",
                        "missions": {{
                            "arena_c_ass": {{
                                "layers": ["obj"],
                                "sides": {{"blue": ["pilot"]}},
                                "scriptManifest": "{manifest_path}",
                                "scripts": [{{"name": "arena_c_ass", "file": "{script_path}", "length": {}, "sha256": "{sha}"}}]
                            }}
                        }},
                        "layers": {{
                            "obj": {{"collision": "layers/obj/collision.bin"}}
                        }}
                    }}"#,
                    SCRIPT_A.len()
                )
                .into_bytes(),
            );
            files.insert(manifest_path, br#"{"version":1,"family":"swbf2"}"#.to_vec());
            files.insert(&script_path, SCRIPT_A.to_vec());
            files.insert("maps/arena/layers/obj/collision.bin", vec![1, 2, 3, 4]);
            files.insert("maps/arena/props.bin", vec![0, 0, 128, 63, 0, 0, 0, 0]);
            files.insert("maps/arena/vehicles/vehicles.json", br#"{"version":1,"classes":{}}"#.to_vec());
            files.insert("common/common.json", br#"{"family":"swbf2"}"#.to_vec());
            files.insert("common/anim_banks.json", br#"{"fps":30,"banks":{}}"#.to_vec());
            files.insert("common/anim_banks.bin", vec![0; 16]);
            files.insert(
                "sides/blue/units.json",
                br#"{"units":{"pilot":{"actor":"actors/pilot.json"}}}"#.to_vec(),
            );
            files.insert("sides/blue/actors/pilot.json", br#"{"bones":[]}"#.to_vec());
            files.insert("sides/blue/actors/pilot.bin", vec![0, 0, 128, 63, 0, 0, 0, 0]);
            files.insert("sides/blue/weapons.bin", vec![0, 0, 128, 63, 0, 0, 0, 0]);
            files.insert("sides/blue/combos.json", br#"{"machines":{}}"#.to_vec());
            files.insert("sides/blue/ai.json", br#"{"skill":1}"#.to_vec());
            files.insert("sides/blue/anim_banks.json", br#"{"fps":30,"banks":{"b1":{"clips":[]}}}"#.to_vec());
            files.insert("sides/blue/anim_banks.bin", vec![1, 2, 3, 4]);
            files
        };

        let base_files = make_files();
        let q = "?map=arena&mission=arena_c_ass";
        let base_hash = hash_for_family(&base_files, GameFamily::Swbf2, q).unwrap();
        let rows = manifest_for_family(&base_files, GameFamily::Swbf2, q).unwrap();
        assert!(rows.contains(&("family".into(), Some("swbf2".into()))));
        assert!(rows.iter().any(|(p, _)| p == "/maps/arena/props.bin"));
        assert!(rows.iter().any(|(p, _)| p == "/sides/blue/ai.json"));
        assert!(rows.iter().any(|(p, _)| p == "/sides/blue/weapons.bin"));
        assert!(rows.iter().any(|(p, _)| p == "/sides/blue/anim_banks.json"));
        assert!(rows.iter().any(|(p, _)| p == "/sides/blue/actors/pilot.json"));
        assert!(rows.iter().any(|(p, _)| p == "/sides/blue/actors/pilot.bin"));
        assert!(rows.iter().any(|(p, _)| p == "/maps/arena/layers/obj/collision.bin"));
        assert!(rows.iter().any(|(p, _)| p == &format!("/{script_path}")));

        // 1. Canonical JSON formatting invariance (whitespace and key order).
        let mut reformatted = make_files();
        reformatted.insert(
            "sides/blue/anim_banks.json",
            b"{\n  \"banks\" : { \"b1\" : { \"clips\" : [] } },\n  \"fps\" : 30\n}\n".to_vec(),
        );
        assert_eq!(hash_for_family(&reformatted, GameFamily::Swbf2, q).unwrap(), base_hash);

        // 2. Mutating any gameplay file changes the fingerprint.
        for (path, replacement) in [
            ("maps/arena/props.bin", vec![0, 0, 0, 64, 0, 0, 0, 0]),
            ("sides/blue/ai.json", br#"{"skill":2}"#.to_vec()),
            ("sides/blue/anim_banks.json", br#"{"fps":60,"banks":{"b1":{"clips":[]}}}"#.to_vec()),
            ("sides/blue/anim_banks.bin", vec![9, 9, 9, 9]),
            ("sides/blue/combos.json", br#"{"machines":{"x":1}}"#.to_vec()),
            ("sides/blue/actors/pilot.json", br#"{"bones":[{"name":"r"}]}"#.to_vec()),
            ("maps/arena/layers/obj/collision.bin", vec![5, 6, 7, 8]),
            ("maps/arena/vehicles/vehicles.json", br#"{"version":1,"classes":{"v":1}}"#.to_vec()),
            (manifest_path, br#"{"version":2,"family":"swbf2"}"#.to_vec()),
            (script_path.as_str(), SCRIPT_B.to_vec()),
        ] {
            let mut f = make_files();
            f.insert(path, replacement);
            assert_ne!(
                hash_for_family(&f, GameFamily::Swbf2, q).unwrap(),
                base_hash,
                "mutating {path} must change 2005 fingerprint"
            );
        }

        // 3. Removing an optional file (sides/blue/ai.json) hashes it as "missing" and changes the hash.
        let mut without_ai = make_files();
        without_ai.files.remove("sides/blue/ai.json");
        let missing_rows = manifest_for_family(&without_ai, GameFamily::Swbf2, q).unwrap();
        assert!(missing_rows.contains(&("/sides/blue/ai.json".into(), Some("missing".into()))));
        assert_ne!(hash_for_family(&without_ai, GameFamily::Swbf2, q).unwrap(), base_hash);
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use bfe_content::files::DirFiles;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(1);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let id = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "bfe-fingerprint-{label}-{}-{id}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_synthetic_map(root: &Path) {
        fs::create_dir_all(root.join("maps/hot1")).unwrap();
        fs::write(
            root.join("maps/hot1/map.json"),
            br#"{"name":"hot1","mission":{"name":"hot1i","sides":["imp","all"]}}"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("common")).unwrap();
        fs::write(root.join("common/common.json"), br#"{"version":1}"#).unwrap();
        fs::create_dir_all(root.join("sides/imp")).unwrap();
        fs::write(root.join("sides/imp/units.json"), br#"{"classes":{}}"#).unwrap();
        fs::create_dir_all(root.join("sides/all")).unwrap();
        fs::write(root.join("sides/all/units.json"), br#"{"classes":{}}"#).unwrap();
    }

    #[test]
    fn swbf1_fingerprint_matches_pinned_digest_in_the_flat_folder_and_swbf2_resolves_beneath_it() {
        let tmp = TempDir::new("families");
        write_synthetic_map(tmp.path());

        let swbf1_files = DirFiles::new(tmp.path(), GameFamily::Swbf1);
        assert_eq!(swbf1_files.root, tmp.path());
        let swbf1_manifest = manifest(&swbf1_files, "map=hot1&side=imp").unwrap();
        let swbf1_hash = hash(&swbf1_files, "map=hot1&side=imp").unwrap();
        assert!(!swbf1_manifest.iter().any(|(k, _)| k == "family"));

        // Pinned pre-change 2004 digest for this synthetic map tree.
        assert_eq!(
            swbf1_hash,
            "bfe-content-sha256-a6e9f7a362f99653582c909c991f43a89d0e387246c0937406c93f436c2e5270"
        );
        assert_eq!(
            hash_for_family(&swbf1_files, GameFamily::Swbf1, "map=hot1&side=imp").unwrap(),
            swbf1_hash
        );

        // The 2005 game lives beneath the same root, in its own prepared folder.
        let bf2_root = bfe_content::layout::family_dir(tmp.path(), GameFamily::Swbf2);
        write_synthetic_map(&bf2_root);
        let swbf2_files = DirFiles::new(tmp.path(), GameFamily::Swbf2);
        assert_eq!(swbf2_files.root, bf2_root);
        assert_ne!(swbf2_files.root, swbf1_files.root);
    }
}
