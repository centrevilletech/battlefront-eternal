//! Typed map admission, separate from any side or unit roster.
//!
//! Server, client, and inspector share this boundary to load a map's authored
//! base world and compose a mission's active layer(s) in one deterministic
//! order: geometry, collision with per-triangle masks, true void terrain
//! (without stand-in files), visible and logic-only placed objects, damage
//! looks, paths, regions, lights, objectives/command posts, spawns, and world
//! animations. All cross-references (models, collision keys, spawn paths,
//! regions) are canonicalized here once so downstream consumers never chase
//! case mismatches.

pub mod door;

use crate::bin_arrays::{f32s, u32s};
use crate::collision::{CollisionLayerBytes, MapCollision};
use crate::files::Files;
use crate::json;
use crate::mission::{utf16_cmp, Query};
use crate::models::{model_pack_swbf2, Part};
use crate::three::{Box3, Matrix4};
use crate::value::{Object, Value};
use crate::vehicle_pack::Swbf2VehiclePack;
use crate::world_anim::WorldAnimations;
use bfe_family::GameFamily;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

/// One world geometry stream (either the base map or a composed layer).
#[derive(Clone, Debug)]
pub struct MapMeshSource {
    /// `None` for the base world, `Some(layer_name)` for an active layer.
    pub layer: Option<String>,
    /// Content-relative path to the vertex stream (e.g. `maps/spa6/geom.bin`).
    pub geom_path: String,
    /// Content-relative path to the index stream (e.g. `maps/spa6/index.bin`).
    pub index_path: String,
    /// Authored mesh records belonging to this stream, in order.
    pub meshes: Vec<Value>,
}

/// A placed object from the base world or a composed layer (visible or logic-only).
#[derive(Clone, Debug)]
pub struct PlacedObject {
    pub index: usize,
    pub layer: Option<String>,
    pub name: String,
    pub class: String,
    pub base: String,
    pub model: Option<String>,
    pub door_animation: Option<door::DoorAnimation>,
    pub destroyed_model: Option<String>,
    pub unbuilt_model: Option<String>,
    pub matrix: [f64; 16],
    pub position: [f64; 3],
    pub team: i32,
    pub radius: Option<f64>,
    pub collision: Option<(u32, u32)>,
    pub destroyed_collision: Option<(u32, u32)>,
    pub unbuilt_collision: Option<(u32, u32)>,
    pub visible: bool,
    pub properties: Vec<(String, String)>,
    pub settings: Value,
    pub raw: Value,
}

/// Axis-aligned 3D world bounds (`map.json.{bounds, wireBounds, renderBounds}`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldBounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// Summary census of an admitted map composition.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapCounts {
    pub world_meshes: usize,
    pub collision_triangles: usize,
    pub terrain_triangles: usize,
    pub placed_total: usize,
    pub placed_visible: usize,
    pub placed_logic_only: usize,
    pub placed_by_base: BTreeMap<String, usize>,
    pub lights: usize,
    pub regions: usize,
    pub gameplay_regions: usize,
    pub auxiliary_regions: usize,
    pub death_regions: usize,
    pub paths: usize,
    pub spawn_paths: usize,
    pub spawn_points: usize,
    pub command_posts: usize,
    pub prop_skeletons: usize,
    pub prop_multi_bone_skeletons: usize,
    pub world_anim_clips: usize,
    pub world_anim_groups: usize,
    pub world_anim_hierarchies: usize,
}

/// Shared admitted map state, independent of any side/unit roster.
pub struct MapAdmission {
    pub family: GameFamily,
    pub map: String,
    pub mission_name: Option<String>,
    pub active_layers: Vec<String>,
    pub has_terrain: bool,
    /// Authored world bounds (`map.json.bounds`, `wireBounds`, `renderBounds`).
    pub bounds: Option<WorldBounds>,
    pub wire_bounds: Option<WorldBounds>,
    pub render_bounds: Option<WorldBounds>,
    /// Raw `map.json` as parsed from disk.
    pub map_data: Value,
    /// Composed `map.json` view with active layer data merged in order and
    /// references canonicalized.
    pub world: Value,
    /// Geometry streams in composition order (base, then active layers).
    pub mesh_sources: Vec<MapMeshSource>,
    /// Composed static collision (base + active layers) and optional terrain.
    pub collision: MapCollision,
    /// Normalized `props.json` (canonical rig objects with attached `skeletons`
    /// bind hierarchies, composed base + layer instances, resolved collision
    /// ranges and damage looks).
    pub props: Option<Value>,
    pub props_buf: Option<Rc<[u8]>>,
    pub props_index_buf: Option<Rc<[u8]>>,
    /// Relative filename of `props.json.attributesFile` (e.g. `props_attributes.bin`), when present.
    pub props_attributes_file: Option<String>,
    /// Raw bytes of `props_attributes.bin` (prop tint/lit/tangent/joint/weight streams), when present.
    pub props_attributes_buf: Option<Rc<[u8]>>,
    pub props_coll_buf: Option<Rc<[u8]>>,
    pub props_coll_index_buf: Option<Rc<[u8]>>,
    pub props_coll_poly_buf: Option<Rc<[u8]>>,
    pub props_coll_mask_buf: Option<Rc<[u8]>>,
    pub props_anim_buf: Option<Rc<[u8]>>,
    /// Typed placed objects (both visible and logic-only), in composition order.
    pub placed: Vec<PlacedObject>,
    pub world_animations: WorldAnimations,
    pub lights: Vec<Value>,
    pub global_lights: Value,
    pub regions: Vec<Value>,
    pub death_regions: Vec<Value>,
    pub paths: Vec<Value>,
    pub spawns: Value,
    pub command_posts: Vec<Value>,
    /// Map-local animation banks (`maps/<map>/anim_banks.json` + `.bin`), when present.
    pub map_anim_banks: Option<Value>,
    pub map_anim_bin: Option<Rc<[u8]>>,
    /// Map-local class definitions (`maps/<map>/classes.json`), when present.
    pub map_classes: Option<Value>,
    pub vehicle_pack: Option<Swbf2VehiclePack>,
    /// Raw bytes of `maps/<map>/<planningGroups.data>` (e.g. `planning.bin`),
    /// when a 2005 map declares dynamic planning-group delta blocks; always
    /// `None` for 2004 maps.
    pub planning_bin: Option<Rc<[u8]>>,
    /// Case-insensitive model key map (lowercase -> exact key in `props.models`).
    pub prop_model_canonical: HashMap<String, String>,
    /// References that required case-folding canonicalization `(authored, canonical)`.
    pub canonicalized_refs: Vec<(String, String)>,
}

/// Converts a single ODF scalar string into a typed JSON value when it is a
/// clean finite number, keeping non-numeric strings and multi-token vectors as
/// strings.
pub fn parse_odf_scalar(text: &str) -> Value {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Value::str(text);
    }
    if !trimmed.contains(char::is_whitespace) {
        if let Ok(n) = trimmed.parse::<f64>() {
            if n.is_finite() {
                return Value::Number(n);
            }
        }
    }
    Value::str(text)
}

/// Parses an ordered `[[key, value], ...]` ODF property pair list into a flat
/// JSON object (last scalar wins, numeric strings converted to JSON numbers).
pub fn materialize_odf_properties(pairs: &[(String, String)]) -> Object {
    let mut written: Vec<(Rc<str>, Value)> = Vec::with_capacity(pairs.len());
    for (k, v) in pairs {
        written.push((Rc::from(k.as_str()), parse_odf_scalar(v)));
    }
    Object::from_written(written)
}

/// Extracts `[[key, value], ...]` pairs from a 2005 `"properties"` JSON array.
pub fn read_property_pairs(value: Option<&Value>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(list) = value.and_then(Value::as_array) else {
        return out;
    };
    for pair in list {
        let Some(arr) = pair.as_array() else { continue };
        let Some(k) = arr.first().and_then(Value::as_str) else { continue };
        let v = match arr.get(1) {
            Some(Value::String(s)) => s.to_string(),
            Some(Value::Number(n)) => crate::js::number_to_string(*n),
            Some(Value::Bool(b)) => b.to_string(),
            _ => continue,
        };
        out.push((k.to_string(), v));
    }
    out
}

fn read_json(files: &dyn Files, path: &str) -> Result<Value, String> {
    let bytes = files.read(path).ok_or_else(|| format!("404 {path}"))?;
    json::parse_bytes(&bytes).map_err(|e| format!("{path}: {e}"))
}

/// Strict optional JSON reader for 2005 content: an absent file returns
/// `Ok(None)`, while a file that exists but fails to parse returns an `Err`
/// naming the file path.
pub(crate) fn optional_json_strict(files: &dyn Files, path: &str) -> Result<Option<Value>, String> {
    let Some(bytes) = files.read(path) else {
        return Ok(None);
    };
    json::parse_bytes(&bytes)
        .map(Some)
        .map_err(|e| format!("{path}: {e}"))
}

pub(crate) fn validate_rel_path(rel: &str, source: &str) -> Result<(), String> {
    if rel.is_empty()
        || rel.starts_with('/')
        || rel.contains('\\')
        || rel.contains(':')
        || rel.split('/').any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        return Err(format!("{source}: invalid relative path '{rel}'"));
    }
    Ok(())
}

fn read_matrix16_strict(value: Option<&Value>, label: &str) -> Result<[f64; 16], String> {
    let Some(val) = value.filter(|v| !v.is_null()) else {
        return Ok(Matrix4::identity().elements);
    };
    let arr = val
        .as_array()
        .filter(|a| a.len() == 16)
        .ok_or_else(|| format!("{label}: expected 16-element matrix"))?;
    let mut m = [0.0; 16];
    for (i, slot) in m.iter_mut().enumerate() {
        let v = arr
            .get(i)
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("{label}: non-numeric matrix element at index {i}"))?;
        if !v.is_finite() || !(v as f32).is_finite() {
            return Err(format!("{label}: non-finite matrix element at index {i}"));
        }
        *slot = v;
    }
    Ok(m)
}

fn read_vec3_strict(value: Option<&Value>, fallback: [f64; 3], label: &str) -> Result<[f64; 3], String> {
    let Some(val) = value.filter(|v| !v.is_null()) else {
        return Ok(fallback);
    };
    let arr = val
        .as_array()
        .filter(|a| a.len() == 3)
        .ok_or_else(|| format!("{label}: expected 3-element vector"))?;
    let mut out = [0.0; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        let v = arr
            .get(i)
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("{label}: non-numeric vector component {i}"))?;
        if !v.is_finite() || !(v as f32).is_finite() {
            return Err(format!("{label}: non-finite vector component {i}"));
        }
        *slot = v;
    }
    Ok(out)
}

fn parse_bounds_strict(value: Option<&Value>, label: &str) -> Result<Option<WorldBounds>, String> {
    let Some(val) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let obj = val
        .as_object()
        .ok_or_else(|| format!("{label}: bounds must be an object with min/max"))?;
    let min = read_vec3_strict(obj.get("min"), [0.0; 3], &format!("{label}.min"))?;
    let max = read_vec3_strict(obj.get("max"), [0.0; 3], &format!("{label}.max"))?;
    Ok(Some(WorldBounds { min, max }))
}

/// Validates a 2005 skeleton array (`props.json.skeletons[*]`, `models[*].bones`,
/// or `actors/*.json.bones`): every bone must have a valid parent index (`-1`
/// or `0..len`), acyclic parent links, and a 16-element finite `f32`-safe local
/// matrix.
pub fn validate_skeleton_strict(bones_val: &Value, label: &str) -> Result<usize, String> {
    let bones = bones_val
        .as_array()
        .ok_or_else(|| format!("{label}: expected skeleton bone array"))?;
    let len = bones.len();
    let mut parents = Vec::with_capacity(len);
    for (i, bone) in bones.iter().enumerate() {
        let obj = bone
            .as_object()
            .ok_or_else(|| format!("{label}[{i}]: expected bone object"))?;
        let p = obj
            .get("parent")
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("{label}[{i}]: missing or non-numeric parent"))?;
        if !p.is_finite() || p.fract() != 0.0 || p < -1.0 || p >= len as f64 {
            return Err(format!(
                "{label}[{i}]: parent {p} out of range (-1..{len})"
            ));
        }
        parents.push(p as i32);
        let local = obj
            .get("local")
            .filter(|v| !v.is_null())
            .ok_or_else(|| format!("{label}[{i}]: missing local matrix"))?;
        read_matrix16_strict(Some(local), &format!("{label}[{i}].local"))?;
    }
    let mut state = vec![0u8; len]; // 0 = unvisited, 1 = visiting, 2 = done
    for start in 0..len {
        let mut cur = start;
        while state[cur] == 0 {
            state[cur] = 1;
            let p = parents[cur];
            if p < 0 {
                break;
            }
            let next = p as usize;
            if state[next] == 1 {
                return Err(format!("{label}: skeleton parent cycle at bone[{next}]"));
            }
            cur = next;
        }
        let mut cur = start;
        while state[cur] == 1 {
            state[cur] = 2;
            let p = parents[cur];
            if p < 0 {
                break;
            }
            cur = p as usize;
        }
    }
    Ok(len)
}

fn optional_array_strict<'a>(obj: &'a Object, key: &str, label: &str) -> Result<&'a [Value], String> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(&[]),
        Some(v) => v
            .as_array()
            .ok_or_else(|| format!("{label}.{key}: expected JSON array")),
    }
}

fn optional_object_strict<'a>(
    obj: &'a Object,
    key: &str,
    label: &str,
) -> Result<Option<&'a Object>, String> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_object()
            .map(Some)
            .ok_or_else(|| format!("{label}.{key}: expected JSON object")),
    }
}

fn validate_spawns_strict(spawns: &Object, label: &str) -> Result<(), String> {
    for (name, val) in spawns.entries() {
        let pts = val
            .as_array()
            .ok_or_else(|| format!("{label}.{name}: expected spawn point array"))?;
        for (i, pt) in pts.iter().enumerate() {
            let arr = pt
                .as_array()
                .filter(|a| a.len() >= 3)
                .ok_or_else(|| {
                    format!("{label}.{name}[{i}]: expected spawn node with at least 3 numbers")
                })?;
            for (j, elem) in arr.iter().enumerate() {
                let n = elem
                    .as_f64()
                    .ok_or_else(|| format!("{label}.{name}[{i}][{j}]: expected finite number"))?;
                if !n.is_finite() || !(n as f32).is_finite() {
                    return Err(format!("{label}.{name}[{i}][{j}]: non-finite number"));
                }
            }
        }
    }
    Ok(())
}

fn validate_regions_strict(regions: &[Value], label: &str) -> Result<(), String> {
    for (i, reg) in regions.iter().enumerate() {
        let obj = reg
            .as_object()
            .ok_or_else(|| format!("{label}[{i}]: expected region object"))?;
        let rname = obj.get("name").and_then(Value::as_str).unwrap_or("(unnamed)");
        if let Some(sz) = obj.get("size").filter(|v| !v.is_null()) {
            let size = read_vec3_strict(Some(sz), [0.0; 3], &format!("{label}[{i}] '{rname}'.size"))?;
            if size.iter().any(|n| *n < 0.0) { return Err(format!("{label}[{i}]: negative region size")); }
        }
        if let Some(m) = obj.get("matrix").filter(|v| !v.is_null()) {
            let elements = read_matrix16_strict(Some(m), &format!("{label}[{i}] '{rname}'.matrix"))?;
            let det = (Matrix4 { elements }).determinant();
            if det == 0.0 || !det.is_finite() { return Err(format!("{label}[{i}]: singular region transform")); }
        }
    }
    Ok(())
}

/// Decode one authored blend. Allow the accumulated rounding of three f32
/// weights without changing the values sent to the renderer.
pub fn swbf2_weight_blend(bytes: &[u8; 12]) -> Result<[f32; 3], &'static str> {
    let (words, _) = bytes.as_chunks::<4>();
    let weights: [f32; 3] = std::array::from_fn(|i| f32::from_le_bytes(words[i]));
    if weights.iter().any(|w| !w.is_finite()) {
        return Err("non-finite weight");
    }
    if weights.iter().any(|&w| w < 0.0) {
        return Err("negative weight");
    }
    if (weights.into_iter().map(f64::from).sum::<f64>() - 1.0).abs() > 1e-5 {
        return Err("weights must sum to one within 0.00001");
    }
    Ok(weights)
}

fn validate_part_bind_and_streams(
    part: &Value,
    skel_len: usize,
    attr_bytes: Option<&[u8]>,
    label: &str,
) -> Result<(), String> {
    let Some(obj) = part.as_object() else {
        return Err(format!("{label}: expected part object"));
    };
    if let Some(bb) = obj.get("bindBone").filter(|v| !v.is_null()) {
        let idx = bb
            .as_f64()
            .ok_or_else(|| format!("{label}.bindBone: expected integer"))?;
        if !idx.is_finite() || idx.fract() != 0.0 || idx < -1.0 || idx >= skel_len as f64 {
            return Err(format!(
                "{label}.bindBone: index {idx} out of range (-1..{skel_len})"
            ));
        }
    }
    if let Some(vs_val) = obj.get("vertexStreams").filter(|v| !v.is_null()) {
        let vs = vs_val
            .as_object()
            .ok_or_else(|| format!("{label}.vertexStreams: expected JSON object"))?;
        if !vs.is_empty() {
            let attrs = attr_bytes.ok_or_else(|| {
                format!("{label}.vertexStreams: props_attributes.bin missing for vertex streams")
            })?;
            let total = attrs.len();
            let vc = obj
                .get("vertexCount")
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n <= usize::MAX as f64)
                .ok_or_else(|| format!("{label}: invalid vertexCount for vertexStreams"))?
                as usize;
            let influences = match obj.get("influencesPerVertex").filter(|v| !v.is_null()) {
                Some(v) => {
                    let n = v
                        .as_f64()
                        .ok_or_else(|| format!("{label}.influencesPerVertex: expected integer"))?;
                    if !n.is_finite() || n.fract() != 0.0 || !(1.0..=4.0).contains(&n) {
                        return Err(format!("{label}.influencesPerVertex: invalid value {n}"));
                    }
                    n as usize
                }
                None => 1,
            };
            for (stream, off_val) in vs.entries() {
                if off_val.is_null() {
                    continue;
                }
                let off_f = off_val
                    .as_f64()
                    .ok_or_else(|| format!("{label}.vertexStreams.{stream}: expected byte offset"))?;
                if !off_f.is_finite()
                    || off_f < 0.0
                    || off_f.fract() != 0.0
                    || off_f > usize::MAX as f64
                {
                    return Err(format!(
                        "{label}.vertexStreams.{stream}: invalid offset {off_f}"
                    ));
                }
                let off = off_f as usize;
                let stride = match stream {
                    "tint" | "lit" => 16usize,
                    "tangent" | "binormal" => 12usize,
                    "joints" | "weights" => 4 * influences,
                    _ => 4usize,
                };
                let bytes = vc
                    .checked_mul(stride)
                    .ok_or_else(|| format!("{label}.vertexStreams.{stream}: byte length overflow"))?;
                let end = off
                    .checked_add(bytes)
                    .ok_or_else(|| format!("{label}.vertexStreams.{stream}: range overflow"))?;
                if end > total {
                    return Err(format!(
                        "{label}.vertexStreams.{stream}: range [{off}, {end}) exceeds props_attributes.bin ({total} bytes)"
                    ));
                }
                if stream == "joints" {
                    if influences != 1 && influences != 3 {
                        return Err(format!(
                            "{label}.vertexStreams.joints: unsupported influencesPerVertex {influences} (expected 1 or 3)"
                        ));
                    }
                    if influences == 3
                        && vs.get("weights").is_none_or(Value::is_null)
                    {
                        return Err(format!(
                            "{label}.vertexStreams.joints: influencesPerVertex 3 requires vertexStreams.weights"
                        ));
                    }
                    for (entry_idx, &chunk) in attrs[off..end].as_chunks::<4>().0.iter().enumerate() {
                        let j = u32::from_le_bytes(chunk);
                        if (j as usize) >= skel_len || j > u32::from(u16::MAX) {
                            return Err(format!(
                                "{label}.vertexStreams.joints[{entry_idx}]: joint index {j} out of range (0..{skel_len})"
                            ));
                        }
                    }
                } else if stream == "weights" && influences == 3 {
                    for (vertex, blend) in attrs[off..end].as_chunks::<12>().0.iter().enumerate() {
                        swbf2_weight_blend(blend)
                            .map_err(|e| format!("{label}.vertexStreams.weights at vertex {vertex}: {e}"))?;
                    }
                } else if stream == "weights" {
                    for (entry_idx, &chunk) in attrs[off..end].as_chunks::<4>().0.iter().enumerate() {
                        let w = f32::from_le_bytes(chunk);
                        if !w.is_finite() {
                            return Err(format!(
                                "{label}.vertexStreams.weights[{entry_idx}]: non-finite weight"
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// True for non-gameplay auxiliary regions (sound streams/spaces/triggers,
/// lighting shadow volumes, and map bounds boxes).
pub fn is_auxiliary_region(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower == "mapbounds"
        || lower.starts_with("soundstream")
        || lower.starts_with("soundspace")
        || lower.starts_with("soundtrigger")
        || lower.starts_with("shadow ")
        || lower == "shadow"
}

/// Selects a 2005 mission name from `map.json.missions` matching an explicit
/// name or query (`mission=`, `mode=`, `era=`), else the initial mission in
/// sorted name order.
pub fn pick_swbf2_mission(map_data: &Value, requested: Option<&str>, query: Option<&Query>) -> Option<String> {
    pick_swbf2_mission_strict(map_data, "map.json", requested, query).ok().flatten()
}

fn mission_matches_mode(name: &str, rec: &Value, mode: &str) -> bool {
    if let Some(m) = rec.get("mode").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        return m.eq_ignore_ascii_case(mode);
    }
    let suffix = format!("_{mode}");
    name.to_ascii_lowercase().ends_with(&suffix)
}

fn mission_matches_era(name: &str, rec: &Value, era: &str) -> bool {
    if let Some(e) = rec.get("era").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        return e.eq_ignore_ascii_case(era);
    }
    let lower = name.to_ascii_lowercase();
    let Some((head, _)) = lower.rsplit_once('_') else {
        return false;
    };
    match era {
        "gcw" => head.ends_with('g'),
        "cw" => head.ends_with('c'),
        _ => false,
    }
}

pub fn pick_swbf2_mission_strict(
    map_data: &Value,
    map_path: &str,
    requested: Option<&str>,
    query: Option<&Query>,
) -> Result<Option<String>, String> {
    let missions_val = map_data.get("missions");
    let Some(missions) = (match missions_val {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_object()
                .ok_or_else(|| format!("{map_path}.missions: expected JSON object"))?,
        ),
    }) else {
        if let Some(req) = requested.filter(|s| !s.is_empty()) {
            return Err(format!("{map_path}: unknown mission '{req}' (no missions table)"));
        }
        if let Some(wanted) = query.map(|q| q.get("mission")).filter(|s| !s.is_empty()) {
            return Err(format!("{map_path}: unknown mission '{wanted}' (no missions table)"));
        }
        if let Some(q) = query {
            let mode = q.get("mode");
            let era = q.get("era");
            if !mode.is_empty() || !era.is_empty() {
                return Err(format!(
                    "{map_path}: no mission matching mode='{mode}' era='{era}' (no missions table)"
                ));
            }
        }
        return Ok(None);
    };
    let mut names: Vec<String> = missions.keys().map(str::to_string).collect();
    names.sort_by(|a, b| utf16_cmp(a, b));
    if let Some(req) = requested.filter(|s| !s.is_empty()) {
        if let Some(hit) = names.iter().find(|n| n.eq_ignore_ascii_case(req)) {
            return Ok(Some(hit.clone()));
        }
        return Err(format!("{map_path}: unknown mission '{req}'"));
    }
    if let Some(q) = query {
        let wanted = q.get("mission");
        if !wanted.is_empty() {
            if let Some(hit) = names.iter().find(|n| n.eq_ignore_ascii_case(wanted)) {
                return Ok(Some(hit.clone()));
            }
            return Err(format!("{map_path}: unknown mission '{wanted}'"));
        }
        let mode = q.get("mode").to_ascii_lowercase();
        let era = q.get("era").to_ascii_lowercase();
        if !mode.is_empty() || !era.is_empty() {
            if let Some(hit) = names.iter().find(|n| {
                let rec = missions.get(n).unwrap_or(&Value::Null);
                let ok_mode = mode.is_empty() || mission_matches_mode(n, rec, &mode);
                let ok_era = era.is_empty() || mission_matches_era(n, rec, &era);
                ok_mode && ok_era
            }) {
                return Ok(Some(hit.clone()));
            }
            return Err(format!(
                "{map_path}: no mission matching mode='{mode}' era='{era}'"
            ));
        }
    }
    Ok(names.into_iter().next())
}

fn compute_model_bounds(parts: &[Part]) -> (Value, Value) {
    let mut bounds = Box3::default();
    let mut count = 0usize;
    for part in parts {
        let n = part.count();
        for i in 0..n {
            let p = part.position(i);
            if p.x.is_finite() && p.y.is_finite() && p.z.is_finite() {
                bounds.expand_by_point(&p);
                count += 1;
            }
        }
    }
    if count == 0 || bounds.is_empty() {
        let zero = Value::Array(Rc::new(vec![Value::Number(0.0); 3]));
        let box_val = Value::Array(Rc::new(vec![zero.clone(), zero]));
        let sphere_val = Value::Array(Rc::new(vec![
            Value::Number(0.0),
            Value::Number(0.0),
            Value::Number(0.0),
            Value::Number(1.0),
        ]));
        return (box_val, sphere_val);
    }
    let center = bounds.center();
    let mut max_r2 = 0.0f64;
    for part in parts {
        let n = part.count();
        for i in 0..n {
            let p = part.position(i);
            if p.x.is_finite() && p.y.is_finite() && p.z.is_finite() {
                let d2 = center.distance_to_squared(&p);
                if d2 > max_r2 {
                    max_r2 = d2;
                }
            }
        }
    }
    let lo = Value::Array(Rc::new(vec![
        Value::Number(bounds.min.x),
        Value::Number(bounds.min.y),
        Value::Number(bounds.min.z),
    ]));
    let hi = Value::Array(Rc::new(vec![
        Value::Number(bounds.max.x),
        Value::Number(bounds.max.y),
        Value::Number(bounds.max.z),
    ]));
    let box_val = Value::Array(Rc::new(vec![lo, hi]));
    let sphere_val = Value::Array(Rc::new(vec![
        Value::Number(center.x),
        Value::Number(center.y),
        Value::Number(center.z),
        Value::Number(max_r2.sqrt()),
    ]));
    (box_val, sphere_val)
}

fn collision_range_strict(entry: &Value, label: &str) -> Result<(u32, u32), String> {
    let (first_v, count_v) = if let Some(arr) = entry.as_array() {
        if arr.len() != 2 {
            return Err(format!("{label}: collision range array must have 2 elements"));
        }
        (arr.first(), arr.get(1))
    } else if let Some(obj) = entry.as_object() {
        (obj.get("firstTriangle"), obj.get("triangleCount"))
    } else {
        return Err(format!("{label}: collision range must be an object or 2-element array"));
    };
    let first = first_v
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("{label}: missing or non-numeric collision firstTriangle"))?;
    let count = count_v
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("{label}: missing or non-numeric collision triangleCount"))?;
    if !first.is_finite()
        || !count.is_finite()
        || first.fract() != 0.0
        || count.fract() != 0.0
        || first < 0.0
        || count <= 0.0
        || first > f64::from(u32::MAX)
        || count > f64::from(u32::MAX)
    {
        return Err(format!("{label}: invalid collision range ({first}, {count})"));
    }
    Ok((first as u32, count as u32))
}

fn range_value(range: (u32, u32)) -> Value {
    Value::Array(Rc::new(vec![
        Value::Number(f64::from(range.0)),
        Value::Number(f64::from(range.1)),
    ]))
}

fn starts_deathregion(name: &str) -> bool {
    name.len() >= 11 && name.as_bytes()[..11].eq_ignore_ascii_case(b"deathregion")
}

fn canonicalize_key(
    authored: &str,
    exact_has: impl Fn(&str) -> bool,
    lower_map: &HashMap<String, String>,
    log: &mut Vec<(String, String)>,
) -> Option<String> {
    if authored.is_empty() {
        return None;
    }
    if exact_has(authored) {
        return Some(authored.to_string());
    }
    let lower = authored.to_ascii_lowercase();
    let canonical = lower_map.get(&lower)?;
    if canonical != authored {
        let entry = (authored.to_string(), canonical.clone());
        if !log.contains(&entry) {
            log.push(entry);
        }
    }
    Some(canonical.clone())
}

impl MapAdmission {
    /// Admits a 2005 map independently of any side or unit roster.
    ///
    /// Composes the base world and the selected mission's layers (or
    /// `explicit_layers` when non-empty) in order, handles void terrain
    /// without stand-in files, validates schemas and numeric extents
    /// strictly, and canonicalizes all cross-references at this boundary.
    ///
    /// 2004 content stays on [`crate::battle::Battle::load`] without a
    /// duplicate reader.
    pub fn load(
        files: &dyn Files,
        family: GameFamily,
        map: &str,
        mission: Option<&str>,
        explicit_layers: &[&str],
    ) -> Result<MapAdmission, String> {
        Self::load_inner(files, family, map, mission, None, explicit_layers)
    }

    /// Admits a 2005 map using a query string (`?mission=...&mode=...&era=...`).
    pub fn load_for_query(
        files: &dyn Files,
        family: GameFamily,
        map: &str,
        query: &Query,
    ) -> Result<MapAdmission, String> {
        Self::load_inner(files, family, map, None, Some(query), &[])
    }

    fn load_inner(
        files: &dyn Files,
        family: GameFamily,
        map: &str,
        mission: Option<&str>,
        query: Option<&Query>,
        explicit_layers: &[&str],
    ) -> Result<MapAdmission, String> {
        if family != GameFamily::Swbf2 {
            return Err("MapAdmission is 2005-only; use Battle::load for 2004 content".into());
        }
        let map_dir = format!("/maps/{map}");
        let map_rel = format!("maps/{map}");
        let map_json_path = format!("{map_dir}/map.json");
        let map_data = read_json(files, &map_json_path)?;
        let map_obj = map_data
            .as_object()
            .ok_or_else(|| format!("{map_json_path}: expected JSON object"))?;

        let bounds = parse_bounds_strict(map_obj.get("bounds"), &format!("{map_json_path}.bounds"))?;
        let wire_bounds = parse_bounds_strict(
            map_obj.get("wireBounds"),
            &format!("{map_json_path}.wireBounds"),
        )?;
        let render_bounds = parse_bounds_strict(
            map_obj.get("renderBounds"),
            &format!("{map_json_path}.renderBounds"),
        )?;

        let mission_name = pick_swbf2_mission_strict(&map_data, &map_json_path, mission, query)?;
        let layers_obj = optional_object_strict(map_obj, "layers", &map_json_path)?;
        let layer_index_obj = optional_object_strict(map_obj, "layerIndex", &map_json_path)?;
        if let Some(idx) = layer_index_obj {
            for (hk, hv) in idx.entries() {
                let target = hv.as_str().ok_or_else(|| {
                    format!("{map_json_path}.layerIndex.{hk}: expected layer name string")
                })?;
                let exists = layers_obj.is_some_and(|o| {
                    o.has(target) || o.keys().any(|k| k.eq_ignore_ascii_case(target))
                });
                if !exists {
                    return Err(format!(
                        "{map_json_path}.layerIndex.{hk}: target layer '{target}' is absent from layers"
                    ));
                }
            }
        }
        let mut active_layers: Vec<String> = Vec::new();
        let mut push_layer = |wanted: &str| {
            let resolved = layers_obj
                .and_then(|o| {
                    if o.has(wanted) {
                        return Some(wanted.to_string());
                    }
                    if let Some(k) = o.keys().find(|k| k.eq_ignore_ascii_case(wanted)) {
                        return Some(k.to_string());
                    }
                    // Look up by `layerIndex[hash] -> layer_key`.
                    if let Some(idx_hit) = layer_index_obj.and_then(|idx| {
                        idx.get(wanted)
                            .or_else(|| {
                                idx.entries()
                                    .find(|(k, _)| k.eq_ignore_ascii_case(wanted))
                                    .map(|(_, v)| v)
                            })
                            .and_then(Value::as_str)
                    }) {
                        if o.has(idx_hit) {
                            return Some(idx_hit.to_string());
                        }
                        if let Some(k) = o.keys().find(|k| k.eq_ignore_ascii_case(idx_hit)) {
                            return Some(k.to_string());
                        }
                    }
                    // Fallback: match against each layer record's `name` or `hash` field.
                    for (k, lv) in o.entries() {
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
                })
                .unwrap_or_else(|| wanted.to_string());
            if !active_layers.iter().any(|l| l.eq_ignore_ascii_case(&resolved)) {
                active_layers.push(resolved);
            }
        };
        if !explicit_layers.is_empty() {
            for &name in explicit_layers {
                push_layer(name);
            }
        } else if let Some(ref mname) = mission_name {
            if let Some(mrec) = map_obj.get("missions").and_then(|m| m.get(mname)) {
                let mobj = mrec.as_object().ok_or_else(|| {
                    format!("{map_json_path}: mission '{mname}' must be a JSON object")
                })?;
                let mlayers = optional_array_strict(
                    mobj,
                    "layers",
                    &format!("{map_json_path}.missions.{mname}"),
                )?;
                for (i, l) in mlayers.iter().enumerate() {
                    let name = l.as_str().ok_or_else(|| {
                        format!(
                            "{map_json_path}.missions.{mname}.layers[{i}]: expected layer name string"
                        )
                    })?;
                    push_layer(name);
                }
            }
        }

        // 1. World mesh streams: base first, then each active layer in order.
        let mut mesh_sources = Vec::new();
        let base_meshes: Vec<Value> = optional_array_strict(map_obj, "meshes", &map_json_path)?.to_vec();
        mesh_sources.push(MapMeshSource {
            layer: None,
            geom_path: format!("{map_rel}/geom.bin"),
            index_path: format!("{map_rel}/index.bin"),
            meshes: base_meshes,
        });
        for lname in &active_layers {
            let Some(lval) = layers_obj.and_then(|o| o.get(lname)) else {
                return Err(format!("{map_json_path}: unknown layer '{lname}'"));
            };
            let lobj = lval.as_object().ok_or_else(|| {
                format!("{map_json_path}: layer '{lname}' must be a JSON object")
            })?;
            let layer_label = format!("{map_json_path}.layers.{lname}");
            let lmeshes: Vec<Value> = optional_array_strict(lobj, "meshes", &layer_label)?.to_vec();
            if let Some(g) = lobj.get("geom").filter(|v| !v.is_null()) {
                let gs = g
                    .as_str()
                    .ok_or_else(|| format!("{layer_label}.geom: expected path string"))?;
                validate_rel_path(gs, &format!("{map_json_path}: layer '{lname}'.geom"))?;
            }
            if let Some(ix) = lobj.get("index").filter(|v| !v.is_null()) {
                let ixs = ix
                    .as_str()
                    .ok_or_else(|| format!("{layer_label}.index: expected path string"))?;
                validate_rel_path(ixs, &format!("{map_json_path}: layer '{lname}'.index"))?;
            }
            if !lmeshes.is_empty() {
                let geom_rel = lobj
                    .get("geom")
                    .and_then(Value::as_str)
                    .map(|g| format!("{map_rel}/{g}"))
                    .unwrap_or_else(|| format!("{map_rel}/layers/{lname}/geom.bin"));
                let index_rel = lobj
                    .get("index")
                    .and_then(Value::as_str)
                    .map(|ix| format!("{map_rel}/{ix}"))
                    .unwrap_or_else(|| format!("{map_rel}/layers/{lname}/index.bin"));
                mesh_sources.push(MapMeshSource {
                    layer: Some(lname.clone()),
                    geom_path: geom_rel,
                    index_path: index_rel,
                    meshes: lmeshes,
                });
            }
        }

        // 2. Composed collision + per-triangle masks + true void terrain.
        struct OwnedCollisionLayer {
            positions: Rc<[u8]>,
            indices: Rc<[u8]>,
            polygons: Rc<[u8]>,
            masks: Option<Rc<[u8]>>,
        }
        let mut owned_layers: Vec<OwnedCollisionLayer> = Vec::new();
        {
            let c_path = format!("{map_dir}/collision.bin");
            let ci_path = format!("{map_dir}/collision_index.bin");
            let cp_path = format!("{map_dir}/collision_poly.bin");
            let (c, ci, cp) = (files.read(&c_path), files.read(&ci_path), files.read(&cp_path));
            match (c, ci, cp) {
                (Some(c), Some(ci), Some(cp)) => {
                    let cm = files.read(&format!("{map_dir}/collision_mask.bin"));
                    owned_layers.push(OwnedCollisionLayer {
                        positions: c,
                        indices: ci,
                        polygons: cp,
                        masks: cm,
                    });
                }
                (None, None, None) => {}
                (c, ci, _) => {
                    let missing = if c.is_none() {
                        c_path
                    } else if ci.is_none() {
                        ci_path
                    } else {
                        cp_path
                    };
                    return Err(format!("404 {missing}"));
                }
            }
        }
        for lname in &active_layers {
            let Some(lobj) = layers_obj.and_then(|o| o.get(lname)) else { continue };
            let declared_coll = lobj.get("collision").and_then(Value::as_str);
            if let Some(rel) = declared_coll {
                validate_rel_path(rel, &format!("{map_json_path}: layer '{lname}'.collision"))?;
            }
            let coll_path = declared_coll
                .map(|p| format!("{map_dir}/{p}"))
                .unwrap_or_else(|| format!("{map_dir}/layers/{lname}/collision.bin"));
            let stem = coll_path.strip_suffix(".bin").unwrap_or(&coll_path);
            let idx_path = format!("{stem}_index.bin");
            let poly_path = format!("{stem}_poly.bin");
            let mask_path = format!("{stem}_mask.bin");
            match (files.read(&coll_path), files.read(&idx_path), files.read(&poly_path)) {
                (Some(c), Some(ci), Some(cp)) => {
                    let cm = files.read(&mask_path);
                    owned_layers.push(OwnedCollisionLayer {
                        positions: c,
                        indices: ci,
                        polygons: cp,
                        masks: cm,
                    });
                }
                (None, None, None) if declared_coll.is_none() => {}
                (c, ci, _) => {
                    let missing = if c.is_none() {
                        coll_path
                    } else if ci.is_none() {
                        idx_path
                    } else {
                        poly_path
                    };
                    return Err(format!(
                        "{map_json_path}: layer '{lname}' declared collision file missing: {missing}"
                    ));
                }
            }
        }
        let layer_refs: Vec<CollisionLayerBytes<'_>> = owned_layers
            .iter()
            .map(|l| CollisionLayerBytes {
                positions: &l.positions,
                indices: &l.indices,
                polygons: &l.polygons,
                masks: l.masks.as_deref(),
            })
            .collect();
        let terrain_files = match (
            files.read(&format!("{map_dir}/terrain_collision.bin")),
            files.read(&format!("{map_dir}/terrain_collision_index.bin")),
        ) {
            (None, None) => None,
            (Some(g), Some(gi)) => match (g.is_empty(), gi.is_empty()) {
                (false, false) => Some((g, gi)),
                (true, true) => None,
                (true, false) => {
                    return Err(format!(
                        "{map_dir}/terrain_collision.bin is empty while terrain_collision_index.bin is non-empty"
                    ));
                }
                (false, true) => {
                    return Err(format!(
                        "{map_dir}/terrain_collision_index.bin is empty while terrain_collision.bin is non-empty"
                    ));
                }
            },
            (Some(_), None) => {
                return Err(format!("404 {map_dir}/terrain_collision_index.bin"));
            }
            (None, Some(_)) => {
                return Err(format!("404 {map_dir}/terrain_collision.bin"));
            }
        };
        if terrain_files.is_none() && bounds.is_none() {
            return Err(format!(
                "{map_json_path}: map has no terrain and is missing required bounds"
            ));
        }
        let min_bound_y = bounds.map(|b| b.min[1]);
        let collision = MapCollision::load_composed(
            &layer_refs,
            terrain_files.as_ref().map(|(g, gi)| (g.as_ref(), gi.as_ref())),
            min_bound_y,
        )
        .map_err(|e| format!("{map_dir}: {e}"))?;
        let has_terrain = collision.has_terrain;

        // 3. Props, skeleton attachment, rigid part normalization, placed objects, canonicalization.
        let props_json_path = format!("{map_dir}/props.json");
        let raw_props = optional_json_strict(files, &props_json_path)?;
        let raw_props_obj = match raw_props.as_ref() {
            Some(v) => Some(
                v.as_object()
                    .ok_or_else(|| format!("{props_json_path}: expected JSON object"))?,
            ),
            None => None,
        };
        let props_buf = if raw_props.is_some() {
            files.read(&format!("{map_dir}/props.bin"))
        } else {
            None
        };
        let props_index_buf = if raw_props.is_some() {
            files.read(&format!("{map_dir}/props_index.bin"))
        } else {
            None
        };
        let props_attributes_file = raw_props_obj
            .and_then(|p| p.get("attributesFile"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let props_attributes_buf = if let Some(ref attr_file) = props_attributes_file {
            validate_rel_path(attr_file, &format!("{props_json_path}.attributesFile"))?;
            let path = format!("{map_dir}/{attr_file}");
            let bytes = files
                .read(&path)
                .ok_or_else(|| format!("{props_json_path}: missing attributesFile {path}"))?;
            Some(bytes)
        } else {
            None
        };
        let props_coll_buf = files.read(&format!("{map_dir}/props_collision.bin"));
        let props_coll_index_buf = files.read(&format!("{map_dir}/props_collision_index.bin"));
        let props_coll_poly_buf = files.read(&format!("{map_dir}/props_collision_poly.bin"));
        let props_coll_mask_buf = files.read(&format!("{map_dir}/props_collision_mask.bin"));
        let props_anim_buf = files.read(&format!("{map_dir}/props_anims.bin"));
        let map_anim_banks_path = format!("{map_dir}/anim_banks.json");
        let map_anim_banks = optional_json_strict(files, &map_anim_banks_path)?;
        if let Some(ref ab) = map_anim_banks {
            if ab.as_object().is_none() {
                return Err(format!("{map_anim_banks_path}: expected JSON object"));
            }
        }
        let map_anim_bin = if map_anim_banks.is_some() {
            files.read(&format!("{map_dir}/anim_banks.bin"))
        } else {
            None
        };
        let map_classes_path = format!("{map_dir}/classes.json");
        let map_classes = optional_json_strict(files, &map_classes_path)?;
        if let Some(ref mc) = map_classes {
            if mc.as_object().is_none() {
                return Err(format!("{map_classes_path}: expected JSON object"));
            }
        }
        let planning_bin = if let Some(rel) = map_data
            .get("planningGroups")
            .and_then(|pg| pg.get("data"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            validate_rel_path(rel, &format!("{map_json_path}.planningGroups.data"))?;
            let path = format!("{map_dir}/{rel}");
            let bytes = files
                .read(&path)
                .ok_or_else(|| format!("{map_json_path}: missing planningGroups.data {path}"))?;
            Some(bytes)
        } else {
            None
        };

        let vehicle_pack = {
            let rel = match map_data.get("vehicles").and_then(Value::as_str) {
                Some(r) if !r.is_empty() => {
                    validate_rel_path(r, &format!("{map_rel}.vehicles"))?;
                    r.to_string()
                }
                _ => "vehicles/vehicles.json".to_string(),
            };
            let full = format!("{map_dir}/{rel}");
            match files.read(&full) {
                Some(bytes) => {
                    if bytes.len() > crate::vehicle_pack::MAX_FILE_BYTES { return Err(format!("{full}: vehicle pack too large")); }
                    let raw = json::parse_bytes(&bytes).map_err(|e| format!("{full}: {e}"))?;
                    Some(Swbf2VehiclePack::from_value(&raw, &full)?)
                }
                None if map_obj.get("vehicles").and_then(Value::as_str).is_some() =>
                    return Err(format!("{full}: missing declared vehicle pack")),
                None => None,
            }
        };

        let mut canonicalized_refs: Vec<(String, String)> = Vec::new();
        let mut prop_model_canonical: HashMap<String, String> = HashMap::new();
        let mut prop_coll_canonical: HashMap<String, String> = HashMap::new();
        let mut prop_coll_ranges: HashMap<String, (u32, u32)> = HashMap::new();
        let mut prop_skel_by_lower: HashMap<String, (Value, usize)> = HashMap::new();

        // Build normalized props.models (attaching `props.json.skeletons[model]`
        // when present and deriving geometric box + sphere bounds).
        let mut normalized_models_obj = Object::new();
        let raw_coll_obj = match raw_props_obj {
            Some(p) => optional_object_strict(p, "collision", &props_json_path)?,
            None => None,
        };
        let prop_coll_tri_count: Option<(usize, usize)> = match (
            props_coll_buf.as_ref(),
            props_coll_index_buf.as_ref(),
            props_coll_poly_buf.as_ref(),
        ) {
            (Some(vb), Some(ib), Some(pb)) => {
                let fl = f32s(vb).map_err(|e| format!("{map_dir}/props_collision.bin: {e}"))?;
                let ix = u32s(ib).map_err(|e| format!("{map_dir}/props_collision_index.bin: {e}"))?;
                let pg = u32s(pb).map_err(|e| format!("{map_dir}/props_collision_poly.bin: {e}"))?;
                if !fl.len().is_multiple_of(3) {
                    return Err(format!("{map_dir}/props_collision.bin: vertex count is not a multiple of 3 floats"));
                }
                if !ix.len().is_multiple_of(3) {
                    return Err(format!("{map_dir}/props_collision_index.bin: index count is not a multiple of 3"));
                }
                Some((ix.len() / 3, pg.len()))
            }
            (None, None, None) => None,
            (vb, ib, _) => {
                let missing = if vb.is_none() {
                    format!("{map_dir}/props_collision.bin")
                } else if ib.is_none() {
                    format!("{map_dir}/props_collision_index.bin")
                } else {
                    format!("{map_dir}/props_collision_poly.bin")
                };
                return Err(format!("404 {missing}"));
            }
        };
        if let Some(coll_map) = raw_coll_obj {
            for (k, entry) in coll_map.entries() {
                prop_coll_canonical.entry(k.to_ascii_lowercase()).or_insert_with(|| k.to_string());
                let (first, count) =
                    collision_range_strict(entry, &format!("{props_json_path}.collision.{k}"))?;
                if let Some((max_tris, max_polys)) = prop_coll_tri_count {
                    let end = first.checked_add(count).ok_or_else(|| {
                        format!("{props_json_path}: collision range overflow for model '{k}'")
                    })? as usize;
                    if end > max_tris {
                        return Err(format!(
                            "{props_json_path}: collision range [{first}, {end}) for model '{k}' exceeds buffer ({max_tris} triangles)"
                        ));
                    }
                    if end > max_polys {
                        return Err(format!(
                            "{props_json_path}: collision polygon buffer has {max_polys} entries, need {end} for model '{k}'"
                        ));
                    }
                }
                prop_coll_ranges.insert(k.to_string(), (first, count));
            }
        }
        let raw_skels = match raw_props_obj {
            Some(p) => optional_object_strict(p, "skeletons", &props_json_path)?,
            None => None,
        };
        if let Some(skels) = raw_skels {
            for (k, v) in skels.entries() {
                let slen = validate_skeleton_strict(v, &format!("{props_json_path}.skeletons.{k}"))?;
                prop_skel_by_lower
                    .entry(k.to_ascii_lowercase())
                    .or_insert_with(|| (v.clone(), slen));
            }
        }
        let raw_models = match raw_props_obj {
            Some(p) => optional_object_strict(p, "models", &props_json_path)?,
            None => None,
        };
        let attr_bytes = props_attributes_buf.as_deref();
        if let Some(raw_models) = raw_models {
            // Decode prop parts strictly once to validate buffers and derive box + sphere bounds.
            let decoded_pack = match (&props_buf, &props_index_buf) {
                (Some(vb), Some(ib)) => {
                    let mut parts_entries: Vec<(Rc<str>, Value)> = Vec::new();
                    for (name, rec) in raw_models.entries() {
                        let parts = if rec.as_array().is_some() {
                            rec.clone()
                        } else if let Some(obj) = rec.as_object() {
                            obj.get("parts")
                                .filter(|p| p.as_array().is_some())
                                .cloned()
                                .ok_or_else(|| {
                                    format!("{props_json_path}.models.{name}: missing parts array")
                                })?
                        } else {
                            return Err(format!(
                                "{props_json_path}.models.{name}: expected parts array or rig object"
                            ));
                        };
                        parts_entries.push((Rc::from(name), parts));
                    }
                    let parts_val = Value::Object(Rc::new(Object::from_written(parts_entries)));
                    let fl = f32s(vb).map_err(|e| format!("{map_dir}/props.bin: {e}"))?;
                    let ix = u32s(ib).map_err(|e| format!("{map_dir}/props_index.bin: {e}"))?;
                    Some(model_pack_swbf2(Some(&parts_val), &fl, &ix, &props_json_path)?)
                }
                _ if !raw_models.is_empty() => {
                    let missing = if props_buf.is_none() {
                        format!("{map_dir}/props.bin")
                    } else {
                        format!("{map_dir}/props_index.bin")
                    };
                    return Err(format!("404 {missing}"));
                }
                _ => None,
            };

            for (name, rec) in raw_models.entries() {
                prop_model_canonical
                    .entry(name.to_ascii_lowercase())
                    .or_insert_with(|| name.to_string());
                let attached_bones = raw_skels
                    .and_then(|s| s.get(name))
                    .and_then(|v| v.as_array().map(|a| (v.clone(), a.len())))
                    .or_else(|| prop_skel_by_lower.get(&name.to_ascii_lowercase()).cloned());
                if let Some(parts_arr) = rec.as_array() {
                    let (bones_val, skel_len) = attached_bones
                        .unwrap_or_else(|| (Value::Array(Rc::new(Vec::new())), 0));
                    for (pi, part) in parts_arr.iter().enumerate() {
                        validate_part_bind_and_streams(
                            part,
                            skel_len,
                            attr_bytes,
                            &format!("{props_json_path}.models.{name}[{pi}]"),
                        )?;
                    }
                    let (box_val, sphere_val) = decoded_pack
                        .as_ref()
                        .and_then(|pack| pack.get(name))
                        .map(|parts| compute_model_bounds(parts))
                        .unwrap_or_else(|| compute_model_bounds(&[]));
                    let is_rigid = skel_len == 0;
                    let rig = Object::from_written(vec![
                        (Rc::from("parts"), rec.clone()),
                        (Rc::from("bones"), bones_val),
                        (Rc::from("box"), box_val),
                        (Rc::from("sphere"), sphere_val),
                        (Rc::from("rigid"), Value::Bool(is_rigid)),
                    ]);
                    normalized_models_obj.set(Rc::from(name), Value::Object(Rc::new(rig)));
                } else if let Some(mut obj) = rec.as_object().cloned() {
                    let skel_len = if let Some(existing_bones) = obj
                        .get("bones")
                        .filter(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()))
                    {
                        validate_skeleton_strict(
                            existing_bones,
                            &format!("{props_json_path}.models.{name}.bones"),
                        )?
                    } else if let Some((bones_val, slen)) = attached_bones {
                        obj.set(Rc::from("bones"), bones_val);
                        slen
                    } else {
                        if !obj.has("bones") {
                            obj.set(Rc::from("bones"), Value::Array(Rc::new(Vec::new())));
                        }
                        0
                    };
                    if let Some(parts_arr) = obj.get("parts").and_then(Value::as_array) {
                        for (pi, part) in parts_arr.iter().enumerate() {
                            validate_part_bind_and_streams(
                                part,
                                skel_len,
                                attr_bytes,
                                &format!("{props_json_path}.models.{name}.parts[{pi}]"),
                            )?;
                        }
                    }
                    if !obj.has("box") || !obj.has("sphere") {
                        let (box_val, sphere_val) = decoded_pack
                            .as_ref()
                            .and_then(|pack| pack.get(name))
                            .map(|parts| compute_model_bounds(parts))
                            .unwrap_or_else(|| compute_model_bounds(&[]));
                        if !obj.has("box") {
                            obj.set(Rc::from("box"), box_val);
                        }
                        if !obj.has("sphere") {
                            obj.set(Rc::from("sphere"), sphere_val);
                        }
                    }
                    normalized_models_obj.set(Rc::from(name), Value::Object(Rc::new(obj)));
                } else {
                    return Err(format!(
                        "{props_json_path}.models.{name}: expected parts array or rig object"
                    ));
                }
            }
        }

        // Compose placed objects in deterministic order: base first, then active layers.
        let mut raw_placed: Vec<(Option<String>, Value)> = Vec::new();
        let base_placed = if let Some(pobj) = raw_props_obj {
            if pobj.get("instances").is_some_and(|v| !v.is_null()) {
                optional_array_strict(pobj, "instances", &props_json_path)?
            } else {
                optional_array_strict(map_obj, "placedObjects", &map_json_path)?
            }
        } else {
            optional_array_strict(map_obj, "placedObjects", &map_json_path)?
        };
        for obj in base_placed {
            raw_placed.push((None, obj.clone()));
        }
        for lname in &active_layers {
            let Some(lobj) = layers_obj.and_then(|o| o.get(lname)).and_then(Value::as_object) else {
                continue;
            };
            let list = optional_array_strict(
                lobj,
                "placedObjects",
                &format!("{map_json_path}.layers.{lname}"),
            )?;
            for obj in list {
                raw_placed.push((Some(lname.clone()), obj.clone()));
            }
        }

        let class_defs = map_data.get("classes").and_then(Value::as_object);
        let mut placed = Vec::with_capacity(raw_placed.len());
        let mut normalized_instances: Vec<Value> = Vec::with_capacity(raw_placed.len());

        for (index, (layer, raw)) in raw_placed.into_iter().enumerate() {
            let name = raw
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let class = raw
                .get("class")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let base = raw
                .get("base")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let inst_label = format!(
                "{}: placed[{index}] '{}'",
                if layer.is_some() { &map_json_path } else { &props_json_path },
                if name.is_empty() { &class } else { &name }
            );
            let matrix = read_matrix16_strict(raw.get("matrix"), &format!("{inst_label}.matrix"))?;
            let position = read_vec3_strict(
                raw.get("position"),
                [matrix[12], matrix[13], matrix[14]],
                &format!("{inst_label}.position"),
            )?;

            // Merge class ODF properties with instance properties (instance wins).
            let mut merged_pairs: Vec<(String, String)> = Vec::new();
            let cls_rec = class_defs.and_then(|c| {
                c.get(&class)
                    .or_else(|| c.get(&class.to_ascii_lowercase()))
            });
            if let Some(crec) = cls_rec {
                merged_pairs.extend(read_property_pairs(crec.get("properties")));
            }
            let inst_pairs = read_property_pairs(raw.get("properties"));
            merged_pairs.extend(inst_pairs.clone());
            let settings_obj = materialize_odf_properties(&merged_pairs);
            let settings = Value::Object(Rc::new(settings_obj.clone()));

            let prop_ci = |wanted: &str| -> Option<String> {
                merged_pairs
                    .iter()
                    .rev()
                    .find(|(k, _)| k.eq_ignore_ascii_case(wanted))
                    .map(|(_, v)| v.clone())
            };

            let authored_model = raw
                .get("model")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or_else(|| prop_ci("GeometryName").filter(|s| !s.is_empty()));
            let canonical_model = match authored_model.as_deref() {
                Some(m) => {
                    let hit = canonicalize_key(
                        m,
                        |k| normalized_models_obj.has(k),
                        &prop_model_canonical,
                        &mut canonicalized_refs,
                    );
                    if hit.is_none() {
                        return Err(format!(
                            "{inst_label}: model '{m}' is absent from {props_json_path}"
                        ));
                    }
                    hit
                }
                None => None,
            };
            let visible = canonical_model.is_some();
            let model_for_record = canonical_model.clone().or(authored_model);

            let resolve_look = |prop_key: &str,
                                log: &mut Vec<(String, String)>|
             -> (Option<String>, Option<(u32, u32)>) {
                let Some(authored) = prop_ci(prop_key).filter(|s| !s.is_empty()) else {
                    return (None, None);
                };
                let canon_mod = canonicalize_key(
                    &authored,
                    |k| normalized_models_obj.has(k),
                    &prop_model_canonical,
                    log,
                );
                let coll_key = canonicalize_key(
                    canon_mod.as_deref().unwrap_or(&authored),
                    |k| raw_coll_obj.is_some_and(|c| c.has(k)),
                    &prop_coll_canonical,
                    log,
                );
                let coll = coll_key
                    .as_deref()
                    .and_then(|k| prop_coll_ranges.get(k).copied());
                (canon_mod, coll)
            };

            let built_collision = match raw.get("collision").filter(|v| !v.is_null()) {
                Some(entry) => Some(collision_range_strict(
                    entry,
                    &format!("{inst_label}.collision"),
                )?),
                None => {
                    let key = canonical_model.as_deref().or(model_for_record.as_deref());
                    key.and_then(|k| {
                        let ckey = canonicalize_key(
                            k,
                            |ck| raw_coll_obj.is_some_and(|c| c.has(ck)),
                            &prop_coll_canonical,
                            &mut canonicalized_refs,
                        )?;
                        prop_coll_ranges.get(&ckey).copied()
                    })
                }
            };
            let (destroyed_model, destroyed_collision) =
                resolve_look("DestroyedGeometryName", &mut canonicalized_refs);
            let (unbuilt_model, unbuilt_collision) =
                resolve_look("UnbuiltGeometryName", &mut canonicalized_refs);

            let team = prop_ci("Team")
                .and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|n| n.is_finite())
                .map(|n| n as i32)
                .or_else(|| raw.get("team").and_then(Value::as_f64).map(|n| n as i32))
                .unwrap_or(0);
            let radius = prop_ci("Radius")
                .and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|n| n.is_finite() && *n > 0.0)
                .or_else(|| raw.get("radius").and_then(Value::as_f64).filter(|n| *n > 0.0));

            // Enriched instance object compatible with Content::build, Defs::build,
            // F3Props, and bfe-content-native::placed.
            let mut inst_obj = raw.as_object().cloned().unwrap_or_default();
            if let Some(ref m) = model_for_record {
                inst_obj.set(Rc::from("model"), Value::str(m));
            }
            inst_obj.set(Rc::from("settings"), settings.clone());
            inst_obj.set(Rc::from("team"), Value::Number(f64::from(team)));
            if let Some(r) = radius {
                inst_obj.set(Rc::from("radius"), Value::Number(r));
            }
            if let Some(range) = built_collision {
                inst_obj.set(Rc::from("collision"), range_value(range));
            }
            if destroyed_model.is_some() || destroyed_collision.is_some() {
                let mut d = Object::new();
                if let Some(ref dm) = destroyed_model {
                    d.set(Rc::from("model"), Value::str(dm));
                }
                if let Some(dr) = destroyed_collision {
                    d.set(Rc::from("collision"), range_value(dr));
                }
                inst_obj.set(Rc::from("destroyed"), Value::Object(Rc::new(d)));
            }
            if unbuilt_model.is_some() || unbuilt_collision.is_some() {
                let mut u = Object::new();
                if let Some(ref um) = unbuilt_model {
                    u.set(Rc::from("model"), Value::str(um));
                }
                if let Some(ur) = unbuilt_collision {
                    u.set(Rc::from("collision"), range_value(ur));
                }
                inst_obj.set(Rc::from("unbuilt"), Value::Object(Rc::new(u)));
            }
            if base == "commandpost" && !name.is_empty() && !inst_obj.has("post") {
                inst_obj.set(Rc::from("post"), Value::str(&name));
            }
            if let Some(ref l) = layer {
                inst_obj.set(Rc::from("layer"), Value::str(l));
            }
            let enriched = Value::Object(Rc::new(inst_obj));
            normalized_instances.push(enriched);

            let door_animation = door::for_instance(family, &base, &class, map_classes.as_ref(), &merged_pairs)
                .map_err(|e| format!("{inst_label}: {e}"))?;
            placed.push(PlacedObject {
                door_animation,
                index,
                layer,
                name,
                class,
                base,
                model: canonical_model,
                destroyed_model,
                unbuilt_model,
                matrix,
                position,
                team,
                radius,
                collision: built_collision,
                destroyed_collision,
                unbuilt_collision,
                visible,
                properties: inst_pairs,
                settings,
                raw,
            });
        }

        let props = raw_props.map(|raw| {
            let mut obj = raw.as_object().cloned().unwrap_or_default();
            obj.set(Rc::from("models"), Value::Object(Rc::new(normalized_models_obj)));
            obj.set(Rc::from("instances"), Value::Array(Rc::new(normalized_instances)));
            Value::Object(Rc::new(obj))
        });

        // 4. Compose lights, regions, deathRegions, paths, spawns, commandPosts,
        //    and worldAnimations in base-then-active-layer order.
        let mut lights: Vec<Value> = optional_array_strict(map_obj, "lights", &map_json_path)?.to_vec();
        let global_lights = match map_obj.get("globalLights") {
            None | Some(Value::Null) => Value::Object(Rc::new(Object::new())),
            Some(v) if v.as_object().is_some() => v.clone(),
            Some(_) => return Err(format!("{map_json_path}.globalLights: expected JSON object")),
        };
        let mut regions: Vec<Value> = optional_array_strict(map_obj, "regions", &map_json_path)?.to_vec();
        let mut death_regions: Vec<Value> =
            optional_array_strict(map_obj, "deathRegions", &map_json_path)?.to_vec();
        let mut paths: Vec<Value> = optional_array_strict(map_obj, "paths", &map_json_path)?.to_vec();
        let mut spawns_obj = optional_object_strict(map_obj, "spawns", &map_json_path)?
            .cloned()
            .unwrap_or_default();
        validate_spawns_strict(&spawns_obj, &format!("{map_json_path}.spawns"))?;
        let mut raw_cps: Vec<Value> =
            optional_array_strict(map_obj, "commandPosts", &map_json_path)?.to_vec();
        let mut world_animations = WorldAnimations::from_value_strict(
            map_obj.get("worldAnimations"),
            &format!("{map_json_path}.worldAnimations"),
        )?;

        for lname in &active_layers {
            let Some(lobj) = layers_obj.and_then(|o| o.get(lname)).and_then(Value::as_object) else {
                continue;
            };
            let layer_label = format!("{map_json_path}.layers.{lname}");
            lights.extend(optional_array_strict(lobj, "lights", &layer_label)?.iter().cloned());
            regions.extend(optional_array_strict(lobj, "regions", &layer_label)?.iter().cloned());
            paths.extend(optional_array_strict(lobj, "paths", &layer_label)?.iter().cloned());
            if let Some(sp) = optional_object_strict(lobj, "spawns", &layer_label)? {
                validate_spawns_strict(sp, &format!("{layer_label}.spawns"))?;
                for (k, v) in sp.entries() {
                    spawns_obj.set(Rc::from(k), v.clone());
                }
            }
            raw_cps.extend(
                optional_array_strict(lobj, "commandPosts", &layer_label)?
                    .iter()
                    .cloned(),
            );
            world_animations.merge(WorldAnimations::from_value_strict(
                lobj.get("worldAnimations"),
                &format!("{map_json_path}: layer '{lname}'.worldAnimations"),
            )?);
        }
        world_animations.validate_hierarchies(&format!("{map_json_path}.worldAnimations"))?;
        for g in &world_animations.groups {
            let mut has_placement = false;
            for binding in &g.bindings {
                if !world_animations.clips.iter().any(|c| c.name.eq_ignore_ascii_case(&binding.clip)) {
                    return Err(format!("{map_json_path}: unknown animation clip {}", binding.clip));
                }
                let known = placed.iter().any(|p| p.name.eq_ignore_ascii_case(&binding.object)) ||
                    layers_obj.is_some_and(|layers| layers.entries().any(|(_,layer)|
                        layer.get("placedObjects").and_then(Value::as_array).is_some_and(|objects| objects.iter().any(|p|
                            p.get("name").and_then(Value::as_str).is_some_and(|n| n.eq_ignore_ascii_case(&binding.object))))));
                has_placement |= known;
            }
            // A group may list alternative placements across mission layers. A
            // group with no subject anywhere cannot animate any admitted map.
            if !g.bindings.is_empty() && !has_placement {
                return Err(format!("{map_json_path}: animation group {} has no placed subject", g.name));
            }
        }
        validate_regions_strict(&regions, &format!("{map_json_path}.regions"))?;
        validate_regions_strict(&death_regions, &format!("{map_json_path}.deathRegions"))?;

        // Promote any composed `deathregion*` region into `deathRegions` (deduplicated).
        let mut seen_death: Vec<String> = death_regions
            .iter()
            .filter_map(|r| r.get("name").and_then(Value::as_str).map(str::to_ascii_lowercase))
            .collect();
        for reg in &regions {
            if let Some(rname) = reg.get("name").and_then(Value::as_str) {
                let lower = rname.to_ascii_lowercase();
                if starts_deathregion(rname) && !seen_death.contains(&lower) {
                    seen_death.push(lower);
                    death_regions.push(reg.clone());
                }
            }
        }

        // Build case-insensitive lookup maps for spawns and regions so command
        // post references canonicalize at this boundary.
        let mut spawn_lower: HashMap<String, String> = HashMap::new();
        for k in spawns_obj.keys() {
            spawn_lower.entry(k.to_ascii_lowercase()).or_insert_with(|| k.to_string());
        }
        let mut region_by_lower: HashMap<String, Value> = HashMap::new();
        for reg in &regions {
            if let Some(rname) = reg.get("name").and_then(Value::as_str) {
                region_by_lower.entry(rname.to_ascii_lowercase()).or_insert_with(|| reg.clone());
            }
        }

        let mut cp_spawn_keys: Vec<String> = Vec::new();
        let mut command_posts = Vec::with_capacity(raw_cps.len());
        for (cp_idx, cp) in raw_cps.into_iter().enumerate() {
            let mut cp_obj = cp
                .as_object()
                .ok_or_else(|| format!("{map_json_path}.commandPosts[{cp_idx}]: expected JSON object"))?
                .clone();
            let cp_name = cp_obj.get("name").and_then(Value::as_str).unwrap_or("(unnamed)").to_string();
            if let Some(m) = cp_obj.get("matrix").filter(|v| !v.is_null()) {
                read_matrix16_strict(
                    Some(m),
                    &format!("{map_json_path}.commandPosts[{cp_idx}] '{cp_name}'.matrix"),
                )?;
            }
            if let Some(sp) = cp.get("spawnPath").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                let Some(canon) = canonicalize_key(
                    sp,
                    |k| spawns_obj.has(k),
                    &spawn_lower,
                    &mut canonicalized_refs,
                ) else {
                    return Err(format!(
                        "{map_json_path}: command post '{cp_name}' references missing spawnPath '{sp}'"
                    ));
                };
                cp_obj.set(Rc::from("spawnPath"), Value::str(&canon));
                if !cp_spawn_keys.iter().any(|k| k == &canon) {
                    cp_spawn_keys.push(canon);
                }
            }
            // Resolve named captureRegion -> `capture` region object for simulation consumers.
            if cp_obj.get("capture").is_none_or(Value::is_null) {
                if let Some(cap_name) = cp
                    .get("captureRegion")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    let Some(reg) = region_by_lower.get(&cap_name.to_ascii_lowercase()) else {
                        return Err(format!(
                            "{map_json_path}: command post '{cp_name}' references missing captureRegion '{cap_name}'"
                        ));
                    };
                    cp_obj.set(Rc::from("capture"), reg.clone());
                }
            }
            // Simulation expects `killRegion` to be either Null or a region object,
            // whereas 2005 stores a region name string. Resolve to the region object
            // (preserving the name under `killRegionName`).
            if let Some(kill_name) = cp
                .get("killRegion")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                cp_obj.set(Rc::from("killRegionName"), Value::str(kill_name));
                let Some(reg) = region_by_lower.get(&kill_name.to_ascii_lowercase()) else {
                    return Err(format!(
                        "{map_json_path}: command post '{cp_name}' references missing killRegion '{kill_name}'"
                    ));
                };
                cp_obj.set(Rc::from("killRegion"), reg.clone());
            }
            command_posts.push(Value::Object(Rc::new(cp_obj)));
        }

        // 2005 `map.json.spawns` lists every path on the map (including AI
        // formation paths and camera rails). Keep all authored paths in `paths`
        // and restrict `world.spawns` to spawn paths actually named by composed
        // command posts so simulation fallback spawns never pick non-spawn paths.
        let filtered_spawns_obj = if !cp_spawn_keys.is_empty() {
            let mut only_cp = Object::new();
            for key in &cp_spawn_keys {
                if let Some(val) = spawns_obj.get(key) {
                    only_cp.set(Rc::from(key.as_str()), val.clone());
                }
            }
            only_cp
        } else {
            spawns_obj
        };
        let spawns = Value::Object(Rc::new(filtered_spawns_obj));

        // Build composed `world` JSON view.
        let mut world_obj = map_data.as_object().cloned().unwrap_or_default();
        world_obj.set(Rc::from("lights"), Value::Array(Rc::new(lights.clone())));
        world_obj.set(Rc::from("regions"), Value::Array(Rc::new(regions.clone())));
        world_obj.set(Rc::from("deathRegions"), Value::Array(Rc::new(death_regions.clone())));
        world_obj.set(Rc::from("paths"), Value::Array(Rc::new(paths.clone())));
        world_obj.set(Rc::from("spawns"), spawns.clone());
        world_obj.set(Rc::from("commandPosts"), Value::Array(Rc::new(command_posts.clone())));
        world_obj.set(
            Rc::from("activeLayers"),
            Value::Array(Rc::new(active_layers.iter().map(|s| Value::str(s)).collect())),
        );
        let world = Value::Object(Rc::new(world_obj));

        Ok(MapAdmission {
            family,
            map: map.to_string(),
            mission_name,
            active_layers,
            has_terrain,
            bounds,
            wire_bounds,
            render_bounds,
            map_data,
            world,
            mesh_sources,
            collision,
            props,
            props_buf,
            props_index_buf,
            props_attributes_file,
            props_attributes_buf,
            props_coll_buf,
            props_coll_index_buf,
            props_coll_poly_buf,
            props_coll_mask_buf,
            props_anim_buf,
            placed,
            world_animations,
            lights,
            global_lights,
            regions,
            death_regions,
            paths,
            spawns,
            command_posts,
            map_anim_banks,
            map_anim_bin,
            map_classes,
            vehicle_pack,
            planning_bin,
            prop_model_canonical,
            canonicalized_refs,
        })
    }

    /// Evaluates world matrices for every placed object at time `time_s`,
    /// applying active world animation groups and hierarchy propagation.
    pub fn placed_transforms_at(&self, time_s: f64) -> Vec<[f64; 16]> {
        let base: Vec<(String, [f64; 16])> = self
            .placed
            .iter()
            .map(|p| (p.name.clone(), p.matrix))
            .collect();
        self.world_animations.evaluate_transforms(&base, time_s)
    }

    /// Returns a structured census of the admitted map composition.
    pub fn counts(&self) -> MapCounts {
        let mut placed_by_base: BTreeMap<String, usize> = BTreeMap::new();
        let mut placed_visible = 0;
        let mut placed_logic_only = 0;
        for obj in &self.placed {
            let key = if obj.base.is_empty() {
                "(none)".to_string()
            } else {
                obj.base.clone()
            };
            *placed_by_base.entry(key).or_insert(0) += 1;
            if obj.visible {
                placed_visible += 1;
            } else {
                placed_logic_only += 1;
            }
        }
        let mut spawn_paths = 0;
        let mut spawn_points = 0;
        if let Some(sp) = self.spawns.as_object() {
            for (_, pts) in sp.entries() {
                spawn_paths += 1;
                if let Some(arr) = pts.as_array() {
                    spawn_points += arr.len();
                }
            }
        }
        let mut auxiliary_regions = 0;
        for reg in &self.regions {
            let name = reg.get("name").and_then(Value::as_str).unwrap_or("");
            if is_auxiliary_region(name) {
                auxiliary_regions += 1;
            }
        }
        let gameplay_regions = self.regions.len().saturating_sub(auxiliary_regions);
        let mut prop_skeletons = 0;
        let mut prop_multi_bone_skeletons = 0;
        if let Some(models) = self
            .props
            .as_ref()
            .and_then(|p| p.get("models"))
            .and_then(Value::as_object)
        {
            for (_, rec) in models.entries() {
                if let Some(bones) = rec.get("bones").and_then(Value::as_array) {
                    if !bones.is_empty() {
                        prop_skeletons += 1;
                        if bones.len() > 1 {
                            prop_multi_bone_skeletons += 1;
                        }
                    }
                }
            }
        }
        MapCounts {
            world_meshes: self.mesh_sources.iter().map(|s| s.meshes.len()).sum(),
            collision_triangles: self.collision.indices.len() / 3,
            terrain_triangles: self.collision.terrain_indices.len() / 3,
            placed_total: self.placed.len(),
            placed_visible,
            placed_logic_only,
            placed_by_base,
            lights: self.lights.len(),
            regions: self.regions.len(),
            gameplay_regions,
            auxiliary_regions,
            death_regions: self.death_regions.len(),
            paths: self.paths.len(),
            spawn_paths,
            spawn_points,
            command_posts: self.command_posts.len(),
            prop_skeletons,
            prop_multi_bone_skeletons,
            world_anim_clips: self.world_animations.clips.len(),
            world_anim_groups: self.world_animations.groups.len(),
            world_anim_hierarchies: self.world_animations.hierarchies.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::mask;
    use crate::files::MemFiles;

    fn f32_bytes(xs: &[f32]) -> Vec<u8> {
        xs.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn u32_bytes(xs: &[u32]) -> Vec<u8> {
        xs.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    #[test]
    fn swbf2_map_admission_composes_layers_void_terrain_rigid_props_and_case_keys() {
        let mut files = MemFiles::default();
        // Base collision: 1 triangle at y = -200, mask = SOLDIER.
        files.insert("maps/void1/collision.bin", f32_bytes(&[0.0, -200.0, 0.0, 2.0, -200.0, 0.0, 0.0, -200.0, 2.0]));
        files.insert("maps/void1/collision_index.bin", u32_bytes(&[0, 1, 2]));
        files.insert("maps/void1/collision_poly.bin", u32_bytes(&[0]));
        files.insert("maps/void1/collision_mask.bin", vec![mask::SOLDIER]);
        // Layer collision: 1 triangle at y = -220, mask = FLYER.
        files.insert("maps/void1/layers/obj/collision.bin", f32_bytes(&[5.0, -220.0, 0.0, 7.0, -220.0, 0.0, 5.0, -220.0, 2.0]));
        files.insert("maps/void1/layers/obj/collision_index.bin", u32_bytes(&[0, 1, 2]));
        files.insert("maps/void1/layers/obj/collision_poly.bin", u32_bytes(&[0]));
        files.insert("maps/void1/layers/obj/collision_mask.bin", vec![mask::FLYER]);
        // Rigid prop geometry: 1 triangle (3 vertices * 8 floats).
        files.insert(
            "maps/void1/props.bin",
            f32_bytes(&[
                -1.0, 0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0,
                 1.0, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0, 0.0,
                 0.0, 2.0,  1.0, 0.0, 1.0, 0.0, 0.5, 1.0,
            ]),
        );
        files.insert("maps/void1/props_index.bin", u32_bytes(&[0, 1, 2]));
        files.insert("maps/void1/props_collision.bin", f32_bytes(&[-1.0, 0.0, -1.0, 1.0, 0.0, -1.0, 0.0, 2.0, 1.0]));
        files.insert("maps/void1/props_collision_index.bin", u32_bytes(&[0, 1, 2]));
        files.insert("maps/void1/props_collision_poly.bin", u32_bytes(&[0]));
        files.insert("maps/void1/props_collision_mask.bin", vec![mask::BUILDING]);
        files.insert("maps/void1/props_attributes.bin", vec![1, 2, 3, 4]);

        let map_json = r#"{
            "family": "swbf2",
            "name": "void1",
            "bounds": {"min": [-500.0, -400.0, -500.0], "max": [500.0, 400.0, 500.0]},
            "meshes": [{"texture": "base_hull", "flags": 1, "stride": 8, "attributes": ["position","normal","uv"], "vertexOffset": 0, "vertexCount": 3, "indexOffset": 0, "indexCount": 3}],
            "lights": [{"name": "sun", "type": "directional"}],
            "regions": [
                {"name": "hangar_cap", "type": "box", "matrix": [1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1], "size": [10,10,10]},
                {"name": "soundstream ambient 1.0", "type": "sphere", "matrix": [1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1], "size": [5,5,5]}
            ],
            "deathRegions": [],
            "paths": [{"name": "base_path", "nodes": []}],
            "spawns": {"base_spawn": [[0, -198, 0, 0]]},
            "commandPosts": [],
            "classes": {
                "sys_gen": {
                    "base": "destructablebuilding",
                    "properties": [
                        ["MaxHealth", "5000.0"],
                        ["GeometryName", "Ship_Gen_SG"],
                        ["DestroyedGeometryName", "Ship_Gen_SG"]
                    ]
                },
                "veh_pad": {"base": "vehiclespawn", "properties": []}
            },
            "worldAnimations": {
                "clips": {
                    "drift": {
                        "duration": 4.0, "loop": true, "localTranslation": false,
                        "positions": [{"time": 0.0, "value": [0,0,0], "type": 1}, {"time": 4.0, "value": [0,0,8], "type": 1}],
                        "rotations": [{"time": 0.0, "value": [0,0,0], "type": 1}, {"time": 4.0, "value": [0,0,0], "type": 1}]
                    }
                },
                "groups": {
                    "g1": {"defaultOn": true, "stopWhenControlled": false, "disableHierarchies": false, "bindings": [{"clip": "drift", "object": "gen1"}]}
                },
                "hierarchies": [{"parent": "gen1", "children": ["pad1"]}]
            },
            "missions": {
                "void1_m1": {"layers": ["aa11bb22"], "sides": {"blue": ["test_trooper"]}}
            },
            "layerIndex": {"aa11bb22": "obj"},
            "layers": {
                "obj": {
                    "hash": "aa11bb22",
                    "name": "obj",
                    "geom": "layers/obj/geom.bin",
                    "index": "layers/obj/index.bin",
                    "collision": "layers/obj/collision.bin",
                    "meshes": [{"texture": "layer_shield", "flags": 0, "material": {"doubleSided": true, "swbf2Flags": 16842756}, "stride": 8, "attributes": ["position","normal","uv"], "vertexOffset": 0, "vertexCount": 3, "indexOffset": 0, "indexCount": 3}],
                    "lights": [{"name": "hangar_fill", "type": "point"}],
                    "regions": [{"name": "deathregion_a", "type": "cylinder", "matrix": [1,0,0,0,0,1,0,0,0,0,1,0,0,-200,0,1], "size": [5,5,5]}],
                    "paths": [],
                    "spawns": {"Obj_Spawn": [[10, -218, 0, 1.5]]},
                    "commandPosts": [{"name": "cp_obj", "team": 1, "spawnPath": "obj_spawn", "captureRegion": "HANGAR_CAP", "killRegion": "deathregion_a", "matrix": [1,0,0,0,0,1,0,0,0,0,1,0,10,-218,0,1]}],
                    "placedObjects": [
                        {"name": "pad1", "class": "veh_pad", "base": "vehiclespawn", "matrix": [1,0,0,0,0,1,0,0,0,0,1,0,1,2,3,1], "properties": [["Team", "1"]]}
                    ]
                }
            }
        }"#;
        let props_json = r#"{
            "family": "swbf2",
            "attributesFile": "props_attributes.bin",
            "models": {
                "ship_gen_sg": [{"texture": "gen", "vertexOffset": 0, "vertexCount": 3, "indexOffset": 0, "indexCount": 3, "flags": 1, "bindBone": 0, "material": {"flags": 1, "swbf2Flags": 1}}]
            },
            "skeletons": {
                "ship_gen_sg": [
                    {"name": "root", "parent": -1, "local": [1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                    {"name": "core", "parent": 0, "local": [1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}
                ]
            },
            "collision": {
                "ship_gen_sg": {"firstTriangle": 0, "triangleCount": 1}
            },
            "instances": [
                {"name": "gen1", "class": "sys_gen", "base": "destructablebuilding", "model": "Ship_Gen_SG", "matrix": [1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1], "properties": [["Team", "2"]]}
            ]
        }"#;
        files.insert("maps/void1/map.json", map_json.as_bytes().to_vec());
        files.insert("maps/void1/props.json", props_json.as_bytes().to_vec());

        let admitted = MapAdmission::load(&files, GameFamily::Swbf2, "void1", Some("void1_m1"), &[]).unwrap();
        assert!(!admitted.has_terrain);
        assert!(admitted.collision.grid.is_none());
        assert!(admitted.collision.kill_y <= -500.0);
        assert_eq!(admitted.bounds, Some(WorldBounds { min: [-500.0, -400.0, -500.0], max: [500.0, 400.0, 500.0] }));
        assert_eq!(admitted.active_layers, vec!["obj"]);
        assert_eq!(admitted.props_attributes_file.as_deref(), Some("props_attributes.bin"));
        assert_eq!(admitted.props_attributes_buf.as_deref(), Some(&[1u8, 2, 3, 4][..]));
        assert_eq!(admitted.mesh_sources.len(), 2);
        assert_eq!(admitted.mesh_sources[0].layer, None);
        assert_eq!(admitted.mesh_sources[1].layer.as_deref(), Some("obj"));
        assert_eq!(admitted.collision.indices.len() / 3, 2);
        assert_eq!(&*admitted.collision.masks, &[mask::SOLDIER, mask::FLYER]);

        // Case canonicalization resolved `Ship_Gen_SG` -> `ship_gen_sg` and `obj_spawn` -> `Obj_Spawn`.
        assert_eq!(admitted.placed.len(), 2);
        assert_eq!(admitted.placed[0].model.as_deref(), Some("ship_gen_sg"));
        assert_eq!(admitted.placed[0].destroyed_model.as_deref(), Some("ship_gen_sg"));
        assert_eq!(admitted.placed[0].collision, Some((0, 1)));
        assert_eq!(admitted.placed[0].team, 2);
        assert!(admitted.placed[0].visible);
        assert!(!admitted.placed[1].visible);
        assert_eq!(admitted.placed[1].layer.as_deref(), Some("obj"));
        assert!(admitted.canonicalized_refs.iter().any(|(a, c)| a == "Ship_Gen_SG" && c == "ship_gen_sg"));
        assert!(admitted.canonicalized_refs.iter().any(|(a, c)| a == "obj_spawn" && c == "Obj_Spawn"));

        // Normalized rig in props.models attaches `skeletons` bones + computed bounds.
        let rig = admitted.props.as_ref().unwrap().get("models").unwrap().get("ship_gen_sg").unwrap();
        assert_eq!(rig.get("bones").unwrap().as_array().unwrap().len(), 2);
        assert_eq!(rig.get("parts").unwrap().as_array().unwrap().len(), 1);
        assert!(rig.get("box").unwrap().as_array().is_some());
        assert!(rig.get("sphere").unwrap().as_array().is_some());

        // Layer deathregion promoted, CP captureRegion and killRegion resolved,
        // and `spawns` retains only CP-named spawn paths.
        assert_eq!(admitted.death_regions.len(), 1);
        assert_eq!(admitted.command_posts.len(), 1);
        assert_eq!(admitted.command_posts[0].get("spawnPath").unwrap().as_str(), Some("Obj_Spawn"));
        assert!(admitted.command_posts[0].get("capture").unwrap().as_object().is_some());
        assert!(admitted.command_posts[0].get("killRegion").unwrap().as_object().is_some());
        assert!(admitted.spawns.get("Obj_Spawn").is_some());
        assert!(admitted.spawns.get("base_spawn").is_none());

        // World animation advances parent `gen1` and child `pad1` together.
        let xforms = admitted.placed_transforms_at(2.0);
        assert!((xforms[0][14] - 4.0).abs() < 1e-9);
        assert!((xforms[1][14] - 7.0).abs() < 1e-9);

        let counts = admitted.counts();
        assert_eq!(counts.world_meshes, 2);
        assert_eq!(counts.collision_triangles, 2);
        assert_eq!(counts.placed_total, 2);
        assert_eq!(counts.placed_visible, 1);
        assert_eq!(counts.placed_logic_only, 1);
        assert_eq!(counts.lights, 2);
        assert_eq!(counts.regions, 3);
        assert_eq!(counts.gameplay_regions, 2);
        assert_eq!(counts.auxiliary_regions, 1);
        assert_eq!(counts.death_regions, 1);
        assert_eq!(counts.spawn_paths, 1);
        assert_eq!(counts.spawn_points, 1);
        assert_eq!(counts.prop_skeletons, 1);
        assert_eq!(counts.prop_multi_bone_skeletons, 1);
        assert_eq!(counts.command_posts, 1);
    }

    #[test]
    fn swbf2_map_admission_rejects_malformed_inputs_without_silent_skips() {
        let base_files = || {
            let mut files = MemFiles::default();
            files.insert("maps/m1/collision.bin", f32_bytes(&[0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 4.0]));
            files.insert("maps/m1/collision_index.bin", u32_bytes(&[0, 1, 2]));
            files.insert("maps/m1/collision_poly.bin", u32_bytes(&[0]));
            files.insert("maps/m1/props.bin", f32_bytes(&[0.0; 24]));
            files.insert("maps/m1/props_index.bin", u32_bytes(&[0, 1, 2]));
            files.insert("maps/m1/props_collision.bin", f32_bytes(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]));
            files.insert("maps/m1/props_collision_index.bin", u32_bytes(&[0, 1, 2]));
            files.insert("maps/m1/props_collision_poly.bin", u32_bytes(&[0]));
            files.insert(
                "maps/m1/props.json",
                br#"{
                    "family": "swbf2",
                    "models": {"box": [{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}]},
                    "collision": {"box": {"firstTriangle": 0, "triangleCount": 1}},
                    "instances": [{"name":"b1","class":"c1","base":"destructablebuilding","model":"box","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}]
                }"#.to_vec(),
            );
            files.insert(
                "maps/m1/map.json",
                br#"{
                    "family": "swbf2",
                    "bounds": {"min": [-50, -50, -50], "max": [50, 50, 50]},
                    "meshes": [],
                    "lights": [],
                    "regions": [{"name": "zone_alpha", "type": "box", "matrix": [1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1], "size": [5,5,5]}],
                    "deathRegions": [],
                    "paths": [],
                    "spawns": {"sp1": [[0, 1, 0, 0]]},
                    "commandPosts": [{"name": "cp1", "team": 1, "spawnPath": "sp1", "captureRegion": "zone_alpha"}],
                    "missions": {"m1_a": {"layers": [], "sides": {}}},
                    "layers": {}
                }"#.to_vec(),
            );
            files
        };

        // Baseline valid map succeeds.
        assert!(MapAdmission::load(&base_files(), GameFamily::Swbf2, "m1", Some("m1_a"), &[]).is_ok());

        // 1. Malformed props.json must error naming props.json (not silently return None).
        let mut f = base_files();
        f.insert("maps/m1/props.json", b"{ broken json".to_vec());
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("props.json"), "expected props.json in error: {err}");

        // 2. Unknown requested mission must error (never silently fall back).
        let f = base_files();
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_typo"), &[]).err().unwrap();
        assert!(err.contains("m1_typo"), "expected mission name in error: {err}");

        // 3. Declared layer collision file absent must error naming the missing file.
        let mut f = base_files();
        f.insert(
            "maps/m1/map.json",
            br#"{
                "family": "swbf2", "bounds": {"min": [-50, -50, -50], "max": [50, 50, 50]},
                "meshes": [], "lights": [], "regions": [], "deathRegions": [],
                "paths": [], "spawns": {}, "commandPosts": [],
                "missions": {"m1_a": {"layers": ["l1"], "sides": {}}},
                "layers": {"l1": {"collision": "layers/l1/collision.bin", "meshes": []}}
            }"#.to_vec(),
        );
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("collision.bin"), "expected collision.bin in error: {err}");

        // 4. Non-finite placed matrix (1e400 -> Infinity) must error.
        let mut f = base_files();
        f.insert(
            "maps/m1/props.json",
            br#"{
                "family": "swbf2",
                "models": {"box": [{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}]},
                "collision": {},
                "instances": [{"name":"b1","class":"c1","base":"destructablebuilding","model":"box","matrix":[1e400,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}]
            }"#.to_vec(),
        );
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("non-finite"), "expected non-finite in error: {err}");

        // 5. Non-finite bounds must error.
        let mut f = base_files();
        f.insert(
            "maps/m1/map.json",
            br#"{
                "family": "swbf2", "bounds": {"min": [0, 1e400, 0], "max": [10, 10, 10]},
                "meshes": [], "lights": [], "regions": [], "deathRegions": [],
                "paths": [], "spawns": {}, "commandPosts": [],
                "missions": {"m1_a": {"layers": [], "sides": {}}}, "layers": {}
            }"#.to_vec(),
        );
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("non-finite"), "expected non-finite in error: {err}");

        // 6. Instance naming an absent model must error.
        let mut f = base_files();
        f.insert(
            "maps/m1/props.json",
            br#"{
                "family": "swbf2",
                "models": {"box": [{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}]},
                "collision": {},
                "instances": [{"name":"b1","class":"c1","base":"destructablebuilding","model":"ghost_model","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}]
            }"#.to_vec(),
        );
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("ghost_model"), "expected model name in error: {err}");

        // 7. Out-of-range prop collision triangle range must error.
        let mut f = base_files();
        f.insert(
            "maps/m1/props.json",
            br#"{
                "family": "swbf2",
                "models": {"box": [{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}]},
                "collision": {"box": {"firstTriangle": 0, "triangleCount": 99}},
                "instances": [{"name":"b1","class":"c1","base":"destructablebuilding","model":"box","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}]
            }"#.to_vec(),
        );
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("exceeds buffer"), "expected range error: {err}");

        // 8. Dangling captureRegion on a command post must error.
        let mut f = base_files();
        f.insert(
            "maps/m1/map.json",
            br#"{
                "family": "swbf2", "bounds": {"min": [-50, -50, -50], "max": [50, 50, 50]},
                "meshes": [], "lights": [], "regions": [], "deathRegions": [],
                "paths": [], "spawns": {"sp1": [[0,1,0,0]]},
                "commandPosts": [{"name":"cp1","team":1,"spawnPath":"sp1","captureRegion":"missing_cap"}],
                "missions": {"m1_a": {"layers": [], "sides": {}}}, "layers": {}
            }"#.to_vec(),
        );
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("missing_cap"), "expected region name in error: {err}");

        // 9. Path traversal in layer collision path must error.
        let mut f = base_files();
        f.insert(
            "maps/m1/map.json",
            br#"{
                "family": "swbf2", "bounds": {"min": [-50, -50, -50], "max": [50, 50, 50]},
                "meshes": [], "lights": [], "regions": [], "deathRegions": [],
                "paths": [], "spawns": {}, "commandPosts": [],
                "missions": {"m1_a": {"layers": ["l1"], "sides": {}}},
                "layers": {"l1": {"collision": "../escape.bin", "meshes": []}}
            }"#.to_vec(),
        );
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("invalid relative path"), "expected traversal error: {err}");

        // 10. Non-finite vertex in collision.bin must error.
        let mut f = base_files();
        f.insert("maps/m1/collision.bin", f32_bytes(&[f32::NAN, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 4.0]));
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("non-finite"), "expected non-finite collision error: {err}");

        // 11. Skeleton parent out of range, cycle, and short local matrix must error (never panic).
        for (tag, skel, needle) in [
            (
                "parent_oob",
                r#"[{"name":"r","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},{"name":"a","parent":999,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}]"#,
                "out of range",
            ),
            (
                "cycle",
                r#"[{"name":"a","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},{"name":"b","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}]"#,
                "cycle",
            ),
            (
                "short_local",
                r#"[{"name":"r","parent":-1,"local":[1,0,0]}]"#,
                "16-element matrix",
            ),
        ] {
            let mut f = base_files();
            f.insert(
                "maps/m1/props.json",
                format!(
                    r#"{{"family":"swbf2","models":{{"box":[{{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}}]}},"skeletons":{{"box":{skel}}},"collision":{{}},"instances":[]}}"#
                )
                .into_bytes(),
            );
            let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[])
                .err()
                .unwrap_or_else(|| panic!("expected error for skeleton {tag}"));
            assert!(err.contains(needle), "{tag}: unexpected error {err}");
        }

        // 12. Spawn node with string NaN or fewer than 3 coordinates must error.
        for (tag, spawns) in [
            ("nan_string", r#"{"sp1":[["NaN",0,0,0]]}"#),
            ("short_node", r#"{"sp1":[[1.0]]}"#),
        ] {
            let mut f = base_files();
            f.insert(
                "maps/m1/map.json",
                format!(
                    r#"{{"family":"swbf2","bounds":{{"min":[-50,-50,-50],"max":[50,50,50]}},"meshes":[],"lights":[],"regions":[],"deathRegions":[],"paths":[],"spawns":{spawns},"commandPosts":[],"missions":{{"m1_a":{{"layers":[],"sides":{{}}}}}},"layers":{{}}}}"#
                )
                .into_bytes(),
            );
            let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[])
                .err()
                .unwrap_or_else(|| panic!("expected error for spawn {tag}"));
            assert!(err.contains("spawns"), "{tag}: unexpected error {err}");
        }

        // 13. Command post with non-finite (1e400) matrix must error.
        let mut f = base_files();
        f.insert(
            "maps/m1/map.json",
            br#"{"family":"swbf2","bounds":{"min":[-50,-50,-50],"max":[50,50,50]},"meshes":[],"lights":[],"regions":[],"deathRegions":[],"paths":[],"spawns":{"sp1":[[0,1,0,0]]},"commandPosts":[{"name":"cp1","team":1,"spawnPath":"sp1","matrix":[1e400,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"missions":{"m1_a":{"layers":[],"sides":{}}},"layers":{}}"#.to_vec(),
        );
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("commandPosts"), "expected commandPosts error: {err}");

        // 14. Region size of wrong type must error.
        let mut f = base_files();
        f.insert(
            "maps/m1/map.json",
            br#"{"family":"swbf2","bounds":{"min":[-50,-50,-50],"max":[50,50,50]},"meshes":[],"lights":[],"regions":[{"name":"r1","type":"box","size":"big"}],"deathRegions":[],"paths":[],"spawns":{},"commandPosts":[],"missions":{"m1_a":{"layers":[],"sides":{}}},"layers":{}}"#.to_vec(),
        );
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("regions"), "expected regions error: {err}");

        // 15. Wrong types on mission layers, lights, layer meshes, and classes.json must error.
        let mut f = base_files();
        f.insert(
            "maps/m1/map.json",
            br#"{"family":"swbf2","bounds":{"min":[-50,-50,-50],"max":[50,50,50]},"meshes":[],"lights":[],"regions":[],"deathRegions":[],"paths":[],"spawns":{},"commandPosts":[],"missions":{"m1_a":{"layers":5,"sides":{}}},"layers":{}}"#.to_vec(),
        );
        assert!(MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).is_err());

        let mut f = base_files();
        f.insert(
            "maps/m1/map.json",
            br#"{"family":"swbf2","bounds":{"min":[-50,-50,-50],"max":[50,50,50]},"meshes":[],"lights":{},"regions":[],"deathRegions":[],"paths":[],"spawns":{},"commandPosts":[],"missions":{"m1_a":{"layers":[],"sides":{}}},"layers":{}}"#.to_vec(),
        );
        assert!(MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).is_err());

        let mut f = base_files();
        f.insert(
            "maps/m1/map.json",
            br#"{"family":"swbf2","bounds":{"min":[-50,-50,-50],"max":[50,50,50]},"meshes":[],"lights":[],"regions":[],"deathRegions":[],"paths":[],"spawns":{},"commandPosts":[],"missions":{"m1_a":{"layers":["l1"],"sides":{}}},"layers":{"l1":{"meshes":{}}}}"#.to_vec(),
        );
        assert!(MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).is_err());

        let mut f = base_files();
        f.insert("maps/m1/classes.json", b"[]".to_vec());
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("classes.json"), "expected classes.json error: {err}");

        // 16. Terrain pair with one side empty must error instead of silently turning void.
        let mut f = base_files();
        f.insert("maps/m1/terrain_collision.bin", Vec::new());
        f.insert("maps/m1/terrain_collision_index.bin", u32_bytes(&[0, 1, 2]));
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("terrain_collision.bin"), "expected terrain pair error: {err}");

        // 17. Negative or string prop collision range must error.
        for bad_coll in [r#"{"firstTriangle":-5,"triangleCount":2}"#, r#""bad""#] {
            let mut f = base_files();
            f.insert(
                "maps/m1/props.json",
                format!(
                    r#"{{"family":"swbf2","models":{{"box":[{{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}}]}},"collision":{{"box":{bad_coll}}},"instances":[]}}"#
                )
                .into_bytes(),
            );
            assert!(
                MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).is_err(),
                "expected error for collision range {bad_coll}"
            );
        }

        // 18. Matrix with 1e300 overflows f32 and must error.
        let mut f = base_files();
        f.insert(
            "maps/m1/props.json",
            br#"{"family":"swbf2","models":{"box":[{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}]},"collision":{},"instances":[{"name":"b1","class":"c1","base":"destructablebuilding","model":"box","matrix":[1e300,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}]}"#.to_vec(),
        );
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("non-finite"), "expected f32 overflow error: {err}");

        // 19. Prop part vertexCount / indexOffset overflow, bindBone OOB, and vertexStreams OOB must error.
        for (tag, part) in [
            ("vcount_2p32", r#"{"vertexOffset":0,"vertexCount":4294967296,"indexOffset":0,"indexCount":3,"texture":""}"#),
            ("ioff_huge", r#"{"vertexOffset":0,"vertexCount":3,"indexOffset":1000000000000000,"indexCount":3,"texture":""}"#),
            ("bindbone_oob", r#"{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":"","bindBone":99}"#),
            ("vstreams_oob", r#"{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":"","vertexStreams":{"tint":999999}}"#),
        ] {
            let mut f = base_files();
            f.insert("maps/m1/props_attributes.bin", vec![0u8; 8]);
            f.insert(
                "maps/m1/props.json",
                format!(
                    r#"{{"family":"swbf2","attributesFile":"props_attributes.bin","models":{{"box":[{part}]}},"collision":{{}},"instances":[]}}"#
                )
                .into_bytes(),
            );
            assert!(
                MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).is_err(),
                "expected error for prop part case {tag}"
            );
        }

        // 19b. Joint stream index out of skeleton range and non-finite weight stream must error.
        {
            let mut f = base_files();
            f.insert("maps/m1/props_attributes.bin", u32_bytes(&[0, 5, 0]));
            f.insert(
                "maps/m1/props.json",
                br#"{"family":"swbf2","attributesFile":"props_attributes.bin","skeletons":{"box":[{"name":"r","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}]},"models":{"box":[{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":"","vertexStreams":{"joints":0}}]},"collision":{},"instances":[]}"#.to_vec(),
            );
            let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
            assert!(err.contains("joint index 5 out of range"), "expected joint OOB error: {err}");
        }
        {
            let mut f = base_files();
            let mut attrs = u32_bytes(&[0, 0, 0, 0, 0, 0, 0, 0, 0]);
            attrs.extend_from_slice(&f32_bytes(&[1.0, 0.0, 0.0, f32::NAN, 0.0, 0.0, 1.0, 0.0, 0.0]));
            f.insert("maps/m1/props_attributes.bin", attrs);
            f.insert(
                "maps/m1/props.json",
                br#"{"family":"swbf2","attributesFile":"props_attributes.bin","skeletons":{"box":[{"name":"r","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}]},"models":{"box":[{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":"","influencesPerVertex":3,"vertexStreams":{"joints":0,"weights":36}}]},"collision":{},"instances":[]}"#.to_vec(),
            );
            let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
            assert!(err.contains("non-finite weight"), "expected weight error: {err}");
        }

        // 20. World animation hierarchy cycle must error.
        let mut f = base_files();
        f.insert(
            "maps/m1/map.json",
            br#"{"family":"swbf2","bounds":{"min":[-50,-50,-50],"max":[50,50,50]},"meshes":[],"lights":[],"regions":[],"deathRegions":[],"paths":[],"spawns":{},"commandPosts":[],"worldAnimations":{"clips":{},"groups":{},"hierarchies":[{"parent":"a","children":["b"]},{"parent":"b","children":["a"]}]},"missions":{"m1_a":{"layers":[],"sides":{}}},"layers":{}}"#.to_vec(),
        );
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("cycle"), "expected hierarchy cycle error: {err}");

        // 21. Void map (no terrain) missing bounds must error, while a terrain map missing bounds succeeds.
        let no_bounds_map = br#"{"family":"swbf2","meshes":[],"lights":[],"regions":[],"deathRegions":[],"paths":[],"spawns":{},"commandPosts":[],"missions":{"m1_a":{"layers":[],"sides":{}}},"layers":{}}"#;
        let mut f = base_files();
        f.insert("maps/m1/map.json", no_bounds_map.to_vec());
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).err().unwrap();
        assert!(err.contains("missing required bounds"), "expected missing bounds error: {err}");

        let mut f_terrain = base_files();
        f_terrain.insert("maps/m1/map.json", no_bounds_map.to_vec());
        f_terrain.insert(
            "maps/m1/terrain_collision.bin",
            f32_bytes(&[0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 4.0, 4.0, 0.0, 4.0]),
        );
        f_terrain.insert(
            "maps/m1/terrain_collision_index.bin",
            u32_bytes(&[0, 1, 2, 1, 3, 2]),
        );
        assert!(
            MapAdmission::load(&f_terrain, GameFamily::Swbf2, "m1", Some("m1_a"), &[]).is_ok(),
            "terrain map without bounds should still admit"
        );
    }

    #[test]
    fn three_weights_form_a_nonnegative_unit_blend() {
        let part = json::parse(r#"{"vertexCount":2,"influencesPerVertex":3,"vertexStreams":{"joints":0,"weights":24}}"#).unwrap();
        for (weights, valid) in [
            ([0.0, 0.0, 0.0], false), ([0.25, 0.25, 0.0], false),
            ([1.0, 1.0, 0.0], false), ([-0.25, 0.25, 1.0], false),
            ([f32::NAN, 0.0, 1.0], false), ([f32::INFINITY, 0.0, 0.0], false),
            ([0.0, 0.0, 1.00002], false), ([0.0, 0.0, 1.000005], true),
            ([1.0 / 3.0; 3], true), ([0.25, 0.25, 0.5], true),
        ] {
            let mut bytes = u32_bytes(&[0, 1, 2, 0, 1, 2]);
            bytes.extend(f32_bytes(&[0.0, 1.0, 0.0]));
            bytes.extend(f32_bytes(&weights));
            let result = validate_part_bind_and_streams(&part, 3, Some(&bytes), "mesh");
            assert_eq!(result.is_ok(), valid, "{weights:?} gave {result:?}");
        }
    }

    #[test]
    fn swbf2_mission_selection_matches_mode_and_era_and_errors_on_miss() {
        let map_data = json::parse(
            r#"{
                "family": "swbf2",
                "missions": {
                    "arena_c_con": {"layers": [], "sides": {}},
                    "arena_g_con": {"layers": [], "sides": {}},
                    "arena_g_eli": {"layers": [], "sides": {}}
                }
            }"#,
        )
        .unwrap();

        let q_gcw_con = Query::parse("?mode=con&era=gcw");
        assert_eq!(
            pick_swbf2_mission_strict(&map_data, "map.json", None, Some(&q_gcw_con)).unwrap().as_deref(),
            Some("arena_g_con")
        );
        let q_cw_con = Query::parse("?mode=con&era=cw");
        assert_eq!(
            pick_swbf2_mission_strict(&map_data, "map.json", None, Some(&q_cw_con)).unwrap().as_deref(),
            Some("arena_c_con")
        );
        let q_miss_mode = Query::parse("?mode=ctf&era=gcw");
        assert!(pick_swbf2_mission_strict(&map_data, "map.json", None, Some(&q_miss_mode)).is_err());
        let q_miss_era = Query::parse("?mode=eli&era=cw");
        assert!(pick_swbf2_mission_strict(&map_data, "map.json", None, Some(&q_miss_era)).is_err());
    }

    #[test]
    fn map_admission_is_swbf2_only() {
        let files = MemFiles::default();
        let err = MapAdmission::load(&files, GameFamily::Swbf1, "m1", None, &[])
            .err()
            .unwrap();
        assert!(err.contains("2005-only"), "expected 2005-only error: {err}");
    }

    #[test]
    fn swbf2_planning_bin_loads_when_declared_and_errors_when_missing() {
        let mut f = MemFiles::default();
        f.insert("maps/plan_map/planning.bin", vec![0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]);
        f.insert(
            "maps/plan_map/map.json",
            br#"{
                "family": "swbf2", "bounds": {"min":[-12,-8,-9],"max":[9,17,11]}, "meshes": [], "lights": [], "regions": [], "deathRegions": [],
                "paths": [], "spawns": {}, "commandPosts": [],
                "planningGroups": {"groupCounts": [1], "connectionHashes": ["00000001"], "data": "planning.bin", "blocks": []},
                "missions": {"pm_a": {"layers": [], "sides": {}}}, "layers": {}
            }"#.to_vec(),
        );
        let ok = MapAdmission::load(&f, GameFamily::Swbf2, "plan_map", Some("pm_a"), &[]).unwrap();
        assert_eq!(ok.planning_bin.as_deref(), Some(&[0x78u8, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01][..]));

        let mut f = MemFiles::default();
        f.insert(
            "maps/plan_map/map.json",
            br#"{
                "family": "swbf2", "bounds": {"min":[-12,-8,-9],"max":[9,17,11]}, "meshes": [], "lights": [], "regions": [], "deathRegions": [],
                "paths": [], "spawns": {}, "commandPosts": [],
                "planningGroups": {"groupCounts": [1], "connectionHashes": [], "data": "planning.bin", "blocks": []},
                "missions": {"pm_a": {"layers": [], "sides": {}}}, "layers": {}
            }"#.to_vec(),
        );
        let err = MapAdmission::load(&f, GameFamily::Swbf2, "plan_map", Some("pm_a"), &[]).err().unwrap();
        assert!(err.contains("planning.bin"), "expected planning.bin in error: {err}");
    }
}
