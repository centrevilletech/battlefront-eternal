//! Visual admission for a prepared layered world. Only the fixed-camera
//! inspector calls this path. It never constructs a simulation or roster.
use crate::textures::Library;
use bfe_content::{battle::Battle, json::parse_bytes, mission::{spawn_policy, Query}, Value as Authored};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::rc::Rc;

fn authored(value: &Value) -> Result<Authored, String> {
    parse_bytes(&serde_json::to_vec(value).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Layer mesh offsets start at zero in their own two buffers. World indices
/// are local to each mesh, so only the byte offsets need rebasing.
pub(super) fn append_layer(
    map: &mut Value, layer: &Value, files: &Library, dir: &str,
    vertices: &mut Vec<u8>, indices: &mut Vec<u8>,
) -> Result<(), String> {
    let meshes = layer["meshes"].as_array().ok_or("layer has no mesh table")?;
    if meshes.is_empty() { return Ok(()); }
    let read = |key: &str| {
        let file = layer[key].as_str().ok_or_else(|| format!("missing layer {key} file"))?;
        files.read(&format!("{dir}/{file}"))
    };
    let vertex_at = vertices.len() as u64;
    let index_at = indices.len() as u64;
    vertices.extend(read("geom")?);
    indices.extend(read("index")?);
    let base = map["meshes"].as_array_mut().ok_or("map has no mesh table")?;
    for mesh in meshes {
        let mut mesh = mesh.clone();
        for (key, start) in [("vertexOffset", vertex_at), ("indexOffset", index_at)] {
            mesh[key] = mesh[key].as_u64().and_then(|v| v.checked_add(start))
                .ok_or_else(|| format!("invalid layer {key}"))?.into();
        }
        base.push(mesh);
    }
    Ok(())
}

pub(super) fn scenery(
    files: &Library, map_name: &str, map: &Value, layer: Option<&Value>,
) -> Result<Battle, String> {
    let dir = format!("maps/{map_name}");
    let read = |file: &str| files.read(&format!("{dir}/{file}")).map(Rc::<[u8]>::from);
    let world = authored(map)?;
    let mut props: Value = serde_json::from_slice(&read("props.json")?).map_err(|e| e.to_string())?;
    // The exported rigid parts are already in model space. An empty bone list
    // uses the existing static bone and applies the instance matrix exactly once.
    let models = props["models"].as_object_mut().ok_or("props have no model table")?;
    for model in models.values_mut() {
        if model.is_array() {
            *model = json!({ "parts": model.take(), "bones": [] });
        }
    }
    let model_names: BTreeSet<_> = models.keys().cloned().collect();
    let instances = props["instances"].as_array_mut().ok_or("props have no instance table")?;
    if let Some(layer) = layer {
        instances.extend_from_slice(layer["placedObjects"].as_array().ok_or("layer has no placements")?);
    }
    // Spawn markers and other logic objects have no visible model.
    instances.retain(|p| p["model"].as_str().is_some_and(|name| !name.is_empty()));
    // Placed class properties can retain their original spelling while pack
    // keys are lowercase. Keep an exact match when the pack provides one.
    let resolve_model = |desc: &mut Value| {
        if let Some(Value::String(name)) = desc.get_mut("model") {
            if !model_names.contains(name) {
                let lower = name.to_ascii_lowercase();
                if model_names.contains(&lower) { *name = lower; }
            }
        }
    };
    for instance in instances {
        resolve_model(instance);
        for key in ["unbuilt", "destroyed"] {
            if let Some(look) = instance.get_mut(key) { resolve_model(look); }
        }
    }
    let has_ground = map["meshes"].as_array().is_some_and(|meshes|
        meshes.iter().any(|m| m["terrain"].as_bool() == Some(true)));
    let (ground_buf, ground_index_buf) = if has_ground {
        (read("terrain_collision.bin")?, read("terrain_collision_index.bin")?)
    } else { Default::default() };
    let query = Query::default();
    Ok(Battle {
        family: bfe_family::GameFamily::Swbf2, swbf2: None,
        map: map_name.into(), query_text: String::new(), policy: spawn_policy(&query, None), query,
        map_data: world.clone(), world, common: Authored::Null, effects: Authored::Null,
        props: Some(authored(&props)?), props_buf: Some(read("props.bin")?),
        props_index_buf: Some(read("props_index.bin")?),
        coll_buf: read("collision.bin")?, coll_index_buf: read("collision_index.bin")?,
        coll_poly_buf: read("collision_poly.bin")?, ground_buf, ground_index_buf,
        props_coll_buf: None, props_coll_index_buf: None, props_coll_poly_buf: None,
        props_anim_buf: None, side: String::new(), unit: String::new(), soldier_def: Authored::Null,
        anim_meta: Authored::Null, anim_buf: Rc::default(), side_fx: None,
        vehicle_dir: String::new(), vehicle_def: None, vehicle_bufs: None, sides: Vec::new(),
        bodies: Vec::new(), walkers: Vec::new(), nav: None, ai_mission: None,
        class_data: Vec::new(), nav_bin: None, fetched: Default::default(),
    })
}
