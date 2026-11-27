//! Cold placements::LiveObjects. The shared rig batch builder
//! owns bind transforms and grouping; this host supplies initial looks only.
use crate::{loader::authored_json, map::AdmittedMap, BattleContent};
use bfe_content::{
    collision::MapCollision,
    models::ModelMap,
    world_anim::WorldAnimations,
    GameFamily, Value as Authored,
};
use serde_json::{json, Value};
use std::rc::Rc;

mod colours;
pub use colours::VertexColours;

/// One immutable batch recipe per look, with each batch's vertex colours.
type BatchCache = std::collections::BTreeMap<String, (Vec<Value>, Vec<Option<Rc<VertexColours>>>)>;

// Batch arrays are already in the root bind frame. Placement remains a
// separate matrix, and skeleton rows retain attached-mode rounding rather
// than baking an approximate world-space replacement for a skinned draw.
pub struct Part {
    pub instance:usize,
    pub attachment:Option<String>,
    pub model: String,
    pub source: usize,
    pub geometry_key: String,
    pub colours: Option<Rc<VertexColours>>,
    pub matrix: [f64; 16],
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub uv: Vec<f32>,
    pub index: Vec<u32>,
    pub joints: Vec<u16>,
    pub weights: Vec<f32>,
    pub bones: Vec<f32>,
    pub bind_inverse: [f64; 16],
    pub center: [f64; 3],
    pub radius: f64,
}
pub struct Placements {
    pub parts: Vec<Part>,
    pub deferred: usize,
}
fn array<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("invalid prop placement: {e}"))
}
// LiveObjects selects its initial unbuilt look when available. A built
// power-up station or static building otherwise begins at its authored bind
// pose. Damage, rebuilds and animation commands require a world owner.
pub fn admit(content: &BattleContent) -> Result<Placements, String> {admit_selected(content,0,None)}
/// Admit placements with 2005 map-level world animations evaluated at `time_s`.
pub fn admit_at(content: &BattleContent, time_s: f64) -> Result<Placements, String> {
    if content.family == GameFamily::Swbf1 {
        return admit_selected_swbf1(content, 0, None);
    }
    admit_selected_swbf2(content, 0, None, Some(time_s))
}
/// Fixed scenery includes doors in their authored rest pose.
pub fn admit_bind_poses(content: &BattleContent) -> Result<Placements, String> {
    admit_selected(content, 3, None)
}
/// Direct joins pose armed buildings in the native prop owner instead.
pub fn admit_live(content:&BattleContent)->Result<Placements,String>{admit_selected(content,1,None)}
pub fn admit_turrets(content:&BattleContent)->Result<Placements,String>{admit_selected(content,2,None)}
/// Admit one building look for the state-driven client owner.
pub fn admit_look(content:&BattleContent, instance:usize, look:usize)->Result<Placements,String>{admit_selected(content,3,Some((instance,look)))}
pub fn admit_trap_pad(content:&BattleContent, instance:usize)->Result<Placements,String>{admit_selected(content,3,Some((instance,3)))}
/// Admit placements directly from an `AdmittedMap` without requiring a roster.
pub fn admit_map(map: &AdmittedMap) -> Result<Placements, String> {
    if map.admission.family == GameFamily::Swbf1 {
        return admit_swbf1_from_sources(
            map.admission.props.as_ref(),
            map.props_pack.as_ref(),
            &map.admission.collision,
            0,
            None,
        );
    }
    admit_swbf2_from_sources(
        map.admission.props.as_ref(),
        map.props_pack.as_ref(),
        map.admission.props_attributes_buf.as_deref(),
        &map.admission.collision,
        Some(&map.admission.world_animations),
        None,
        0,
        None,
    )
}
/// Admit placements from an `AdmittedMap` with world animations evaluated at `time_s`.
pub fn admit_map_at(map: &AdmittedMap, time_s: f64) -> Result<Placements, String> {
    if map.admission.family == GameFamily::Swbf1 {
        return admit_swbf1_from_sources(
            map.admission.props.as_ref(),
            map.props_pack.as_ref(),
            &map.admission.collision,
            0,
            None,
        );
    }
    admit_swbf2_from_sources(
        map.admission.props.as_ref(),
        map.props_pack.as_ref(),
        map.admission.props_attributes_buf.as_deref(),
        &map.admission.collision,
        Some(&map.admission.world_animations),
        Some(time_s),
        0,
        None,
    )
}
fn admit_selected(content: &BattleContent,mode:u8, selection:Option<(usize,usize)>) -> Result<Placements, String> {
    // The reader's family decides, never a field of the map's own metadata.
    if content.family == GameFamily::Swbf1 {
        admit_selected_swbf1(content, mode, selection)
    } else {
        admit_selected_swbf2(content, mode, selection, None)
    }
}
fn admit_selected_swbf1(content: &BattleContent,mode:u8, selection:Option<(usize,usize)>) -> Result<Placements, String> {
    admit_swbf1_from_sources(
        content.content.battle.props.as_ref(),
        content.content.props_models.as_ref(),
        &content.content.collision,
        mode,
        selection,
    )
}
// Released 2004 placement path: kept identical to the released build,
// including tolerant deferral of empty or absent pack keys.
fn admit_swbf1_from_sources(
    props_authored: Option<&Authored>,
    pack: Option<&ModelMap>,
    collision: &MapCollision,
    mode: u8,
    selection: Option<(usize, usize)>,
) -> Result<Placements, String> {
    let mut result = Placements {
        parts: vec![],
        deferred: 0,
    };
    let Some(props) = props_authored else {
        return Ok(result);
    };
    let props = json_integers(authored_json(props));
    let Some(instances) = props["instances"].as_array() else {
        return Ok(result);
    };
    let Some(pack) = pack else {
        return Ok(result);
    };
    let mut placements = Vec::with_capacity(instances.len());
    for (instance,desc) in instances.iter().enumerate() {
        if let Some((wanted,look))=selection {
            if instance!=wanted {continue;}
            let mut desc=desc.clone();
            if look==3 {
                let Some(model)=desc["trap"]["pad"]["model"].as_str() else {continue;};
                let matrix=crate::pad::placement(array(&desc["matrix"])?,array(&desc["trap"]["pad"]["at"])?,collision)?;
                desc=json!({"base":"staticpad","model":model,"matrix":matrix});
            } else if look!=0 {
                let variant=if look==1 {"destroyed"} else {"unbuilt"};
                if desc[variant]["model"].as_str().is_none(){continue;}
                desc["model"]=desc[variant]["model"].clone();
            }
            desc["unbuilt"]=Value::Null;
            placements.push((instance,desc));
            continue;
        }
        placements.push((instance,desc.clone()));
        if mode!=1 && desc["base"] == "trap" {
            if let Some(model) = desc["trap"]["pad"]["model"].as_str() {
                let matrix = crate::pad::placement(
                    array(&desc["matrix"])?,
                    array(&desc["trap"]["pad"]["at"])?,
                    collision,
                )?;
                placements.push((instance,json!({"base":"staticpad","model":model,"matrix":matrix})));
            }
        }
    }
    // One immutable recipe per look, shared across every authored placement.
    let mut cache = std::collections::BTreeMap::new();
    for (instance,desc) in &placements {
        let base = desc["base"].as_str().unwrap_or("");
        if mode==1&&matches!(base,"armedbuilding"|"powerupstation"|"commandpost"|"trap")||mode==2&&base!="armedbuilding"{continue;}
        // AnimatedProp.present samples clips even without gameplay. Retain
        // that distinction instead of publishing an incorrect bind-pose door.
        if mode!=3 && matches!(base, "animatedprop" | "door") {
            result.deferred += 1;
            continue;
        }
        // Destroyed geometry is a different model and is not part of the
        // initial visible set. Texture/resource admission follows this chosen
        // look, rather than uploading every dormant wreck in the prop pack.
        let unbuilt=if mode==2{None}else{desc["unbuilt"]["model"].as_str()};
        let name = unbuilt
            .or_else(|| desc["model"].as_str())
            .ok_or("prop has no initial model")?;
        let separate = if base == "armedbuilding" && desc["unbuilt"]["model"].is_null() {
            ["TurretNodeName", "AimerNodeName"]
                .iter()
                .filter_map(|key| desc["settings"][key].as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        let key = format!("{name}:{}", separate.join(","));
        let rig = &props["models"][name];
        let Some(parts) = pack.get(name) else {
            result.deferred += 1;
            continue;
        };
        if rig.is_null() {
            return Err(format!("missing placed rig {name}"));
        }
        push_rig_parts(&mut result, &mut cache, *instance, name, &key, rig, parts, &separate, desc)?;
    }
    Ok(result)
}
/// A destructible placed mesh is drawn by its live building, including the
/// wreck supplied by the class. Other static props keep their admitted batch.
pub fn swbf2_destructible_look(base: &str, has_wreck: bool) -> bool {
    base == "destructablebuilding" || (base == "prop" && has_wreck)
}

fn admit_selected_swbf2(content: &BattleContent,mode:u8, selection:Option<(usize,usize)>, time_s: Option<f64>) -> Result<Placements, String> {
    let swbf2 = content.content.battle.swbf2.as_ref();
    let world_anims = swbf2.map(|s| &s.map_admission.world_animations);
    let props_attributes = (!content.prop_attributes.is_empty())
        .then_some(content.prop_attributes.as_slice())
        .or_else(|| swbf2.and_then(|s| s.map_admission.props_attributes_buf.as_deref()));
    admit_swbf2_from_sources(
        content.content.battle.props.as_ref(),
        content.content.props_models.as_ref(),
        props_attributes,
        &content.content.collision,
        world_anims,
        time_s,
        mode,
        selection,
    )
}
#[allow(clippy::too_many_arguments)]
fn admit_swbf2_from_sources(
    props_authored: Option<&Authored>,
    pack: Option<&ModelMap>,
    props_attributes: Option<&[u8]>,
    collision: &MapCollision,
    world_anims: Option<&WorldAnimations>,
    time_s: Option<f64>,
    mode: u8,
    selection: Option<(usize, usize)>,
) -> Result<Placements, String> {
    let mut result = Placements {
        parts: vec![],
        deferred: 0,
    };
    let Some(props_authored) = props_authored else {
        return Ok(result);
    };
    let props = json_integers(authored_json(props_authored));
    let Some(instances) = props["instances"].as_array() else {
        return Ok(result);
    };
    let animated_matrices: Vec<[f64; 16]> = match (world_anims, time_s) {
        (Some(anims), Some(t)) if !anims.groups.is_empty() && !anims.clips.is_empty() => {
            let objects: Vec<(String, [f64; 16])> = instances
                .iter()
                .map(|desc| {
                    let name = desc["name"].as_str().unwrap_or("").to_string();
                    let mat: [f64; 16] = array(&desc["matrix"]).unwrap_or([
                        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
                        1.0,
                    ]);
                    (name, mat)
                })
                .collect();
            anims.evaluate_transforms(&objects, t)
        }
        _ => Vec::new(),
    };
    let Some(pack) = pack else {
        return Ok(result);
    };
    let mut placements = Vec::with_capacity(if selection.is_some() { 1 } else { instances.len() });
    for (instance,desc) in instances.iter().enumerate() {
        if let Some((wanted,look))=selection {
            if instance!=wanted {continue;}
            let mut desc = desc.clone();
            if let Some(mat) = animated_matrices.get(instance) {
                desc["matrix"] = json!(mat);
            }
            if look==3 {
                let Some(model)=desc["trap"]["pad"]["model"].as_str() else {continue;};
                let matrix=crate::pad::placement(array(&desc["matrix"])?,array(&desc["trap"]["pad"]["at"])?,collision)?;
                desc=json!({"base":"staticpad","model":model,"matrix":matrix});
            } else if look!=0 {
                let variant=if look==1 {"destroyed"} else {"unbuilt"};
                if desc[variant]["model"].as_str().is_none(){continue;}
                desc["model"]=desc[variant]["model"].clone();
            }
            desc["unbuilt"]=Value::Null;
            placements.push((instance,desc));
            continue;
        }
        let mut desc = desc.clone();
        if let Some(mat) = animated_matrices.get(instance) {
            desc["matrix"] = json!(mat);
        }
        placements.push((instance,desc.clone()));
        if mode!=1 && desc["base"] == "trap" {
            if let Some(model) = desc["trap"]["pad"]["model"].as_str() {
                let matrix = crate::pad::placement(
                    array(&desc["matrix"])?,
                    array(&desc["trap"]["pad"]["at"])?,
                    collision,
                )?;
                placements.push((instance,json!({"base":"staticpad","model":model,"matrix":matrix})));
            }
        }
    }
    // One immutable recipe per look, shared across every authored placement.
    let mut cache = BatchCache::new();
    for (instance,desc) in &placements {
        let base = desc["base"].as_str().unwrap_or("");
        if mode == 1 && (matches!(base, "armedbuilding" | "powerupstation" | "commandpost" | "trap")
            || swbf2_destructible_look(base, desc["destroyed"]["model"].is_string()))
            || mode == 2 && base != "armedbuilding" {
            continue;
        }
        // Defer 2005 doors/animated props only when authored prop clips exist;
        // rigid or clip-less doors render at their bind pose until animated clips land.
        if mode!=3 && matches!(base, "animatedprop" | "door") {
            let has_clips = desc["model"]
                .as_str()
                .and_then(|n| {
                    props["animations"].get(n).or_else(|| {
                        props["animations"]
                            .as_object()
                            .and_then(|o| o.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v))
                    })
                })
                .is_some_and(|v| !v.is_null());
            if has_clips {
                result.deferred += 1;
                continue;
            }
        }
        // Logic-only placed objects (such as vehiclespawn pads) carry no model.
        let unbuilt=if mode==2{None}else{desc["unbuilt"]["model"].as_str().filter(|s| !s.is_empty())};
        let Some(name) = unbuilt.or_else(|| desc["model"].as_str().filter(|s| !s.is_empty())) else {
            continue;
        };
        let separate = if base == "armedbuilding" && desc["unbuilt"]["model"].is_null() {
            ["TurretNodeName", "AimerNodeName"]
                .iter()
                .filter_map(|key| desc["settings"][key].as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        // Resolve case-insensitively on 2005 packs where instance model names
        // can differ in case from pack keys.
        let resolved_name = if props["models"].get(name).is_some() {
            name
        } else {
            props["models"]
                .as_object()
                .and_then(|o| o.keys().find(|k| k.eq_ignore_ascii_case(name)).map(String::as_str))
                .unwrap_or(name)
        };
        let key = format!("{resolved_name}:{}", separate.join(","));
        let rig = &props["models"][resolved_name];
        let Some(parts) = pack.get(resolved_name).or_else(|| {
            pack.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(resolved_name))
                .map(|(_, v)| v)
        }) else {
            result.deferred += 1;
            continue;
        };
        if rig.is_null() {
            return Err(format!("missing placed rig {resolved_name}"));
        }
        push_rig_parts_swbf2(
            &mut result,
            &mut cache,
            *instance,
            resolved_name,
            &key,
            rig,
            parts,
            props_attributes,
            &separate,
            desc,
        )?;
    }
    Ok(result)
}
type Swbf2StreamSkin = (Vec<u16>, Option<(Vec<u16>, Vec<f32>)>);

fn json_exact_usize(v: &Value) -> Option<usize> {
    if let Some(u) = v.as_u64() {
        return usize::try_from(u).ok();
    }
    v.as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n <= usize::MAX as f64)
        .map(|n| n as usize)
}

/// Reads optional per-vertex joint (and 3-influence weight) streams for a 2005
/// prop part from `props_attributes.bin`. Returns `Ok(None)` when the part does
/// not declare `vertexStreams.joints`.
fn read_swbf2_part_skin_streams(
    record: &Value,
    vertex_count: usize,
    rig_bone_count: usize,
    props_attributes: Option<&[u8]>,
    model: &str,
) -> Result<Option<Swbf2StreamSkin>, String> {
    let Some(vs) = record.get("vertexStreams").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(joints_off_val) = vs.get("joints").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let attrs = props_attributes.ok_or_else(|| {
        format!("prop rig '{model}': props_attributes.bin missing for vertexStreams.joints")
    })?;
    let influences = match record.get("influencesPerVertex").filter(|v| !v.is_null()) {
        Some(v) => {
            let n = json_exact_usize(v).ok_or_else(|| {
                format!("prop rig '{model}': invalid influencesPerVertex for vertexStreams.joints")
            })?;
            if n != 1 && n != 3 {
                return Err(format!(
                    "prop rig '{model}': unsupported influencesPerVertex {n} (expected 1 or 3)"
                ));
            }
            n
        }
        None => 1,
    };
    let joints_off = json_exact_usize(joints_off_val)
        .ok_or_else(|| format!("prop rig '{model}': invalid vertexStreams.joints byte offset"))?;
    let joint_count = vertex_count
        .checked_mul(influences)
        .ok_or_else(|| format!("prop rig '{model}': vertexStreams.joints count overflow"))?;
    let joint_bytes = joint_count
        .checked_mul(4)
        .ok_or_else(|| format!("prop rig '{model}': vertexStreams.joints byte length overflow"))?;
    let joints_end = joints_off
        .checked_add(joint_bytes)
        .ok_or_else(|| format!("prop rig '{model}': vertexStreams.joints range overflow"))?;
    if joints_end > attrs.len() {
        return Err(format!(
            "prop rig '{model}': vertexStreams.joints range [{joints_off}, {joints_end}) exceeds props_attributes.bin ({} bytes)",
            attrs.len()
        ));
    }
    let mut decoded_joints = Vec::with_capacity(joint_count);
    for (idx, &chunk) in attrs[joints_off..joints_end].as_chunks::<4>().0.iter().enumerate() {
        let j = u32::from_le_bytes(chunk);
        if (j as usize) >= rig_bone_count || j > u32::from(u16::MAX) {
            return Err(format!(
                "prop rig '{model}': vertexStreams.joints[{idx}] index {j} out of range (0..{rig_bone_count})"
            ));
        }
        decoded_joints.push(j as u16);
    }
    if influences == 1 {
        return Ok(Some((decoded_joints, None)));
    }
    let weights_off_val = vs
        .get("weights")
        .filter(|v| !v.is_null())
        .ok_or_else(|| {
            format!("prop rig '{model}': influencesPerVertex 3 requires vertexStreams.weights")
        })?;
    let weights_off = json_exact_usize(weights_off_val)
        .ok_or_else(|| format!("prop rig '{model}': invalid vertexStreams.weights byte offset"))?;
    let weights_end = weights_off
        .checked_add(joint_bytes)
        .ok_or_else(|| format!("prop rig '{model}': vertexStreams.weights range overflow"))?;
    if weights_end > attrs.len() {
        return Err(format!(
            "prop rig '{model}': vertexStreams.weights range [{weights_off}, {weights_end}) exceeds props_attributes.bin ({} bytes)",
            attrs.len()
        ));
    }
    let mut decoded_weights = Vec::with_capacity(joint_count);
    for (vertex, blend) in attrs[weights_off..weights_end].as_chunks::<12>().0.iter().enumerate() {
        let weights = bfe_content::map::swbf2_weight_blend(blend)
            .map_err(|e| format!("prop rig '{model}': vertexStreams.weights at vertex {vertex}: {e}"))?;
        decoded_weights.extend(weights);
    }
    let primary: Vec<u16> = decoded_joints.as_chunks::<3>().0.iter().map(|c| c[0]).collect();
    Ok(Some((primary, Some((decoded_joints, decoded_weights)))))
}

/// 2005 rig batch admission: 2005 `props.bin` part vertices are already stored
/// in model bind space (not bone-local space as in 2004 packs). Per-vertex
/// `vertexStreams.joints` (and `weights` when `influencesPerVertex == 3`) take
/// precedence over rigid `bone`/`bindBone` attachment; supplying explicit
/// `joints` keeps `bfe_pose::rig::construction::batches` from re-applying the
/// bone bind matrix while preserving bone attachment grouping (`separate`) and
/// live skinning indices.
#[allow(clippy::too_many_arguments)]
fn push_rig_parts_swbf2(
    result: &mut Placements,
    cache: &mut BatchCache,
    instance: usize,
    resolved_name: &str,
    key: &str,
    rig: &Value,
    parts: &[bfe_content::models::Part],
    props_attributes: Option<&[u8]>,
    separate: &[String],
    desc: &Value,
) -> Result<(), String> {
    let rig_bones: Vec<String> = rig["bones"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|b| b["name"].as_str().unwrap_or("").to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default();
    if let Some(arr) = rig["bones"].as_array() {
        validate_rig_bones_json(arr, resolved_name)?;
    }
    if !cache.contains_key(key) {
        let mut built_parts = Vec::with_capacity(parts.len());
        for p in parts {
            let mut r = authored_json(&p.record);
            let vertex_count = p.data.len() / 8;
            if r.get("joints").is_none_or(Value::is_null) {
                if let Some((joints, soft)) = read_swbf2_part_skin_streams(
                    &r,
                    vertex_count,
                    rig_bones.len(),
                    props_attributes,
                    resolved_name,
                )? {
                    r["joints"] = json!(joints);
                    if let Some((skin_joints, skin_weights)) = soft {
                        r["skinJoints"] = json!(skin_joints);
                        r["skinWeights"] = json!(skin_weights);
                    }
                } else {
                    let bone_idx = r["bone"]
                        .as_str()
                        .and_then(|name| {
                            let lower = name.to_ascii_lowercase();
                            rig_bones.iter().rposition(|b| *b == lower)
                        })
                        .or_else(|| {
                            r["bindBone"]
                                .as_i64()
                                .filter(|&idx| idx >= 0 && (idx as usize) < rig_bones.len())
                                .map(|idx| idx as usize)
                        })
                        .unwrap_or(rig_bones.len());
                    if r.get("bone").and_then(Value::as_str).is_none_or(str::is_empty)
                        && bone_idx < rig_bones.len()
                    {
                        r["bone"] = json!(rig_bones[bone_idx]);
                    }
                    r["joints"] = json!(vec![bone_idx as u16; vertex_count]);
                }
            }
            r["data"] = json!(p.data);
            r["index"] = json!(p.index);
            built_parts.push(json_integers(r));
        }
        // Parts share a batch only when their resolved surfaces and baked
        // light agree. Each batch's light and tint follow its vertex order.
        let mut surfaces = Vec::new();
        let mut ids = Vec::with_capacity(parts.len());
        for (part, built) in parts.iter().zip(&built_parts) {
            let flags = part.field("flags").and_then(Authored::as_f64).unwrap_or(1.0) as u32;
            let surface = crate::loader::extended_surface(&authored_json(&part.record), flags)
                .map_err(|e| format!("placed model {resolved_name}: {e}"))?;
            let identity = (surface, colours::uses_baked_light(built));
            let id = surfaces.iter().position(|s| *s == identity).unwrap_or_else(|| {
                surfaces.push(identity);
                surfaces.len() - 1
            });
            ids.push(id);
        }
        let recipe = serde_json::to_vec(&json!({"rig":rig,"parts":built_parts,"separate":separate}))
            .map_err(|e| e.to_string())?;
        let bytes = bfe_pose::rig::construction::batches_with_materials(&recipe, &ids)?;
        let batches: Vec<Value> = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let colours = batches.iter().map(|b| {
            let sources: Vec<usize> = array(&b["sources"])?;
            colours::batch(&built_parts, &sources, props_attributes.unwrap_or(&[]))
                .map_err(|e| format!("placed model {resolved_name}: {e}"))
        }).collect::<Result<Vec<_>, _>>()?;
        cache.insert(key.to_string(), (batches, colours));
    }
    push_cached_rig_parts(result, cache, instance, resolved_name, key, rig, desc)
}
fn validate_rig_bones_json(bones: &[Value], model: &str) -> Result<(), String> {
    let len = bones.len();
    let mut parents = Vec::with_capacity(len);
    for (i, bone) in bones.iter().enumerate() {
        let p = bone["parent"]
            .as_f64()
            .ok_or_else(|| format!("prop rig '{model}' bone[{i}]: invalid parent"))?;
        if !p.is_finite() || p.fract() != 0.0 || p < -1.0 || p >= len as f64 {
            return Err(format!(
                "prop rig '{model}' bone[{i}]: parent {p} out of range (-1..{len})"
            ));
        }
        parents.push(p as i32);
        let local = bone["local"]
            .as_array()
            .filter(|m| m.len() == 16)
            .ok_or_else(|| format!("prop rig '{model}' bone[{i}]: expected 16-element local matrix"))?;
        for (j, v) in local.iter().enumerate() {
            let n = v.as_f64().ok_or_else(|| {
                format!("prop rig '{model}' bone[{i}].local[{j}]: expected finite number")
            })?;
            if !n.is_finite() || !(n as f32).is_finite() {
                return Err(format!(
                    "prop rig '{model}' bone[{i}].local[{j}]: non-finite value"
                ));
            }
        }
    }
    // Check acyclicity.
    let mut state = vec![0u8; len]; // 0=unvisited, 1=visiting, 2=done
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
                return Err(format!("prop rig '{model}': skeleton parent cycle at bone[{next}]"));
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
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn push_rig_parts(
    result: &mut Placements,
    cache: &mut BatchCache,
    instance: usize,
    resolved_name: &str,
    key: &str,
    rig: &Value,
    parts: &[bfe_content::models::Part],
    separate: &[String],
    desc: &Value,
) -> Result<(), String> {
        if !cache.contains_key(key) {
            let parts: Vec<_> = parts
                .iter()
                .map(|p| {
                    let mut r = authored_json(&p.record);
                    r["data"] = json!(p.data);
                    r["index"] = json!(p.index);
                    json_integers(r)
                })
                .collect();
            let bytes = bfe_pose::rig::construction::batches(
                &serde_json::to_vec(&json!({"rig":rig,"parts":parts,"separate":separate}))
                    .map_err(|e| e.to_string())?,
            )?;
            cache.insert(
                key.to_string(),
                (serde_json::from_slice(&bytes).map_err(|e| e.to_string())?, Vec::new()),
            );
        }
        push_cached_rig_parts(result, cache, instance, resolved_name, key, rig, desc)
}
fn push_cached_rig_parts(
    result: &mut Placements,
    cache: &BatchCache,
    instance: usize,
    resolved_name: &str,
    key: &str,
    rig: &Value,
    desc: &Value,
) -> Result<(), String> {
        let matrix: [f64; 16] = array(&desc["matrix"])?;
        if matrix.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite prop matrix".into());
        }
        // buildRig decomposes bone locals before Skeleton calculates its
        // inverses. Reuse the pose owner's identical tree construction. Raw
        // locals belong only to vertex packing, never skeleton binding.
        let rig_value = bfe_content::json::parse(&rig.to_string()).map_err(|e| e.to_string())?;
        let hull_desc = bfe_pose::rig::content::hull_records(
            &bfe_content::Value::Null,
            &rig_value,
            &[],
            vec![],
            false,
        );
        let mut skeleton = bfe_pose::rig::hull::HullRig::new(&hull_desc);
        // Skeleton captures inverse bind matrices before the placed root is
        // moved. After placement its bone rows multiply world by those same
        // inverses, matching Skeleton.update's Float32 boneMatrices boundary.
        let inverses: Vec<_> = skeleton
            .bones
            .iter()
            .chain(std::iter::once(&skeleton.static_bone))
            .map(|id| {
                let mut m = skeleton.tree.node(*id).matrix_world;
                m.invert();
                m
            })
            .collect();
        {
            let root = skeleton.tree.node_mut(skeleton.root);
            root.matrix_auto_update = false;
            root.matrix.elements = matrix;
        }
        skeleton.tree.update_matrix_world(skeleton.root, true);
        let mut bones = Vec::with_capacity(inverses.len() * 16);
        for (id, inverse) in skeleton
            .bones
            .iter()
            .chain(std::iter::once(&skeleton.static_bone))
            .zip(&inverses)
        {
            let mut m = three::Matrix4::default();
            m.multiply_matrices(&skeleton.tree.node(*id).matrix_world, inverse);
            bones.extend(m.elements.map(|v| v as f32));
        }
        // SkinnedMesh attached mode also updates bindMatrixInverse from the
        // placed mesh world matrix. Keeping both transforms is intentional:
        // cancelling them algebraically changes shader rounding at edges.
        let (batches, colours) = &cache[key];
        for (i, b) in batches.iter().enumerate() {
            // attachVehicleBatches hangs a separate turret/drum below its bone.
            // Both modelMatrix and its attached-mode inverse must retain that
            // parent, even though skinning cancels it in exact arithmetic.
            let placed = if let Some(name) = b["attach"].as_str() {
                let bone = skeleton
                    .by_name
                    .get(name)
                    .ok_or("missing prop attachment bone")?;
                skeleton
                    .tree
                    .node(skeleton.bones[bone])
                    .matrix_world
                    .elements
            } else {
                matrix
            };
            let mut bind_inverse = three::Matrix4 { elements: placed };
            bind_inverse.invert();
            result.parts.push(Part {
                instance,attachment:b["attach"].as_str().map(str::to_string),
                model: resolved_name.into(),
                colours: colours.get(i).cloned().flatten(),
                source: array(&b["part"])?,
                geometry_key: format!("{key}/{}", b["part"]),
                matrix: placed,
                positions: array(&b["positions"])?,
                normals: array(&b["normals"])?,
                uv: array(&b["uv"])?,
                index: array(&b["indices"])?,
                joints: array(&b["joints"])?,
                weights: array(&b["weights"])?,
                bones: bones.clone(),
                bind_inverse: bind_inverse.elements,
                center: array(&b["center"])?,
                radius: array(&b["radius"])?,
            });
        }
    Ok(())
}

// JSON.stringify emits integral authored numbers without a decimal point.
// Match that cold transport when the shared rig builder reads typed counts
// and joints; matrix/position readers accept these exact integers as f64.
pub(crate) fn json_integers(mut v: Value) -> Value {
    match &mut v {
        Value::Array(a) => {
            for x in a {
                *x = json_integers(std::mem::take(x));
            }
        }
        Value::Object(o) => {
            for x in o.values_mut() {
                *x = json_integers(std::mem::take(x));
            }
        }
        Value::Number(n) => {
            if let Some(x) = n
                .as_f64()
                .filter(|x| x.fract() == 0. && x.abs() <= (1u64 << 53) as f64)
            {
                v = Value::from(x as i64)
            }
        }
        _ => {}
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_content::models::{ModelMap, Part as ModelPart};

    #[test]
    fn swbf2_bone_bound_part_keeps_model_space_vertices_while_swbf1_transforms_them() {
        let props = bfe_content::json::parse(
            r#"{
                "models": {
                    "turret_rig": {
                        "parts": [{"texture": "metal", "flags": 1, "bone": "aimer_y", "bindBone": 1}],
                        "bones": [
                            {"name": "rootnode", "parent": -1, "local": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1]},
                            {"name": "aimer_y", "parent": 0, "local": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,2.5,0,1]}
                        ],
                        "box": [[0,2.0,0],[1,3.0,1]],
                        "sphere": [0.5,2.5,0.5,1.0]
                    }
                },
                "instances": [{
                    "name": "roof_gun_01",
                    "base": "armedbuilding",
                    "model": "turret_rig",
                    "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 10,0,-20,1],
                    "settings": {"AimerNodeName": "aimer_y"}
                }]
            }"#,
        )
        .unwrap();
        let part_record = props
            .get("models")
            .unwrap()
            .get("turret_rig")
            .unwrap()
            .get("parts")
            .unwrap()
            .at(0)
            .unwrap()
            .clone();
        // Raw vertex positions in model space sit around y = 2.5.
        let mut pack = ModelMap::default();
        pack.set(
            "turret_rig",
            std::rc::Rc::new(vec![ModelPart {
                record: part_record,
                data: vec![
                    0.0, 2.5, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
                    1.0, 2.5, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0,
                    0.0, 3.0, 0.5, 0.0, 1.0, 0.0, 0.0, 1.0,
                ],
                index: vec![0, 1, 2],
                sphere_center: bfe_content::three::Vector3::new(0.5, 2.75, 0.25),
                sphere_radius: 1.0,
            }]),
        );
        let collision = MapCollision::load(&[], &[], &[], &[], &[]).unwrap();

        // 2005 path: vertices stay at model space y = 2.5..3.0, bone attachment & joint index preserved.
        let p2 = admit_swbf2_from_sources(Some(&props), Some(&pack), None, &collision, None, None, 0, None).unwrap();
        assert_eq!(p2.parts.len(), 1);
        assert_eq!(p2.parts[0].attachment.as_deref(), Some("aimer_y"));
        assert_eq!(p2.parts[0].positions, vec![0.0, 2.5, 0.0, 1.0, 2.5, 0.0, 0.0, 3.0, 0.5]);
        assert_eq!(p2.parts[0].joints[0], 1);
        assert_eq!(p2.parts[0].joints[4], 1);
        assert_eq!(p2.parts[0].joints[8], 1);

        // Released 2004 path: bone-local vertices are transformed by aimer_y's bind translation (+2.5).
        let p1 = admit_swbf1_from_sources(Some(&props), Some(&pack), &collision, 0, None).unwrap();
        assert_eq!(p1.parts.len(), 1);
        assert_eq!(p1.parts[0].attachment.as_deref(), Some("aimer_y"));
        assert_eq!(p1.parts[0].positions, vec![0.0, 5.0, 0.0, 1.0, 5.0, 0.0, 0.0, 5.5, 0.5]);
    }

    #[test]
    fn swbf2_vertex_stream_joints_and_weights_bind_bones_and_preserve_rest_pose() {
        let make_props = |part_json: &str| {
            bfe_content::json::parse(&format!(
                r#"{{
                    "models": {{
                        "box_gun": {{
                            "parts": [{part_json}],
                            "bones": [
                                {{"name": "root", "parent": -1, "local": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1]}},
                                {{"name": "head", "parent": 0, "local": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,2.0,0,1]}},
                                {{"name": "barrel", "parent": 1, "local": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,1.5,1]}}
                            ],
                            "box": [[0,0,0],[1,2,2]],
                            "sphere": [0.5,1.0,1.0,2.0]
                        }}
                    }},
                    "instances": [{{
                        "name": "gun_01",
                        "base": "armedbuilding",
                        "model": "box_gun",
                        "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 5,0,-10,1]
                    }}]
                }}"#
            ))
            .unwrap()
        };
        let make_pack = |props: &Authored| {
            let rec = props
                .get("models")
                .unwrap()
                .get("box_gun")
                .unwrap()
                .get("parts")
                .unwrap()
                .at(0)
                .unwrap()
                .clone();
            let mut pack = ModelMap::default();
            pack.set(
                "box_gun",
                std::rc::Rc::new(vec![ModelPart {
                    record: rec,
                    data: vec![
                        0.0, 0.5, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
                        0.0, 2.0, 0.2, 0.0, 1.0, 0.0, 1.0, 0.0,
                        0.0, 2.0, 1.7, 0.0, 1.0, 0.0, 0.0, 1.0,
                    ],
                    index: vec![0, 1, 2],
                    sphere_center: bfe_content::three::Vector3::new(0.0, 1.5, 0.6),
                    sphere_radius: 1.5,
                }]),
            );
            pack
        };
        let collision = MapCollision::load(&[], &[], &[], &[], &[]).unwrap();

        for (record, expected) in [
            (r#"{"texture":"t","bone":"head"}"#, 1),
            (r#"{"texture":"t","bindBone":1}"#, 3),
            (r#"{"texture":"t"}"#, 3),
        ] {
            let props = make_props(record);
            let pack = make_pack(&props);
            let out = admit_swbf2_from_sources(Some(&props), Some(&pack), None, &collision, None, None, 0, None).unwrap();
            let (joints, remainder) = out.parts[0].joints.as_chunks::<4>();
            assert_eq!(joints.len(), 3);
            assert!(remainder.is_empty());
            assert!(joints.iter().all(|j| *j == [expected, 0, 0, 0]), "{record}");
        }

        // 1. 1-influence joint stream binds vertices to bones 0, 1, 2 (not static root 3)
        //    and preserves model-space rest positions.
        let props1 = make_props(
            r#"{"texture":"t","flags":1,"bindBone":-1,"vertexStreams":{"joints":0}}"#,
        );
        let pack1 = make_pack(&props1);
        let attrs1: Vec<u8> = [0u32, 1, 2]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let out1 = admit_swbf2_from_sources(
            Some(&props1),
            Some(&pack1),
            Some(&attrs1),
            &collision,
            None,
            None,
            0,
            None,
        )
        .unwrap();
        assert_eq!(out1.parts.len(), 1);
        assert_eq!(out1.parts[0].joints, vec![0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0]);
        assert_eq!(out1.parts[0].weights, vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
        assert_eq!(
            out1.parts[0].positions,
            vec![0.0, 0.5, 0.0, 0.0, 2.0, 0.2, 0.0, 2.0, 1.7]
        );

        // 2. 3-influence joint + weight stream writes all three slots and preserves rest positions.
        let props3 = make_props(
            r#"{"texture":"t","flags":1,"bindBone":-1,"influencesPerVertex":3,"vertexStreams":{"joints":0,"weights":36}}"#,
        );
        let pack3 = make_pack(&props3);
        let mut attrs3: Vec<u8> = [0u32, 1, 2, 1, 2, 0, 2, 1, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        attrs3.extend(
            [1.0f32, 0.0, 0.0, 0.75, 0.25, 0.0, 0.5, 0.3, 0.2]
                .into_iter()
                .flat_map(f32::to_le_bytes),
        );
        let out3 = admit_swbf2_from_sources(
            Some(&props3),
            Some(&pack3),
            Some(&attrs3),
            &collision,
            None,
            None,
            0,
            None,
        )
        .unwrap();
        assert_eq!(out3.parts.len(), 1);
        assert_eq!(out3.parts[0].joints, vec![0, 1, 2, 0, 1, 2, 0, 0, 2, 1, 0, 0]);
        assert_eq!(
            out3.parts[0].weights,
            vec![1.0, 0.0, 0.0, 0.0, 0.75, 0.25, 0.0, 0.0, 0.5, 0.3, 0.2, 0.0]
        );
        assert_eq!(
            out3.parts[0].positions,
            vec![0.0, 0.5, 0.0, 0.0, 2.0, 0.2, 0.0, 2.0, 1.7]
        );

        // 3. Out-of-range joint index, missing attributes buffer, and missing weights error cleanly.
        let bad_attrs: Vec<u8> = [0u32, 99, 1]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let err_oob = admit_swbf2_from_sources(
            Some(&props1),
            Some(&pack1),
            Some(&bad_attrs),
            &collision,
            None,
            None,
            0,
            None,
        )
        .err()
        .unwrap();
        assert!(err_oob.contains("out of range"), "unexpected oob error: {err_oob}");

        let err_missing_buf = admit_swbf2_from_sources(
            Some(&props1),
            Some(&pack1),
            None,
            &collision,
            None,
            None,
            0,
            None,
        )
        .err()
        .unwrap();
        assert!(err_missing_buf.contains("props_attributes.bin"), "unexpected missing buf error: {err_missing_buf}");

        let props3_noweights = make_props(
            r#"{"texture":"t","flags":1,"bindBone":-1,"influencesPerVertex":3,"vertexStreams":{"joints":0}}"#,
        );
        let pack3_nw = make_pack(&props3_noweights);
        let err_nw = admit_swbf2_from_sources(
            Some(&props3_noweights),
            Some(&pack3_nw),
            Some(&attrs3),
            &collision,
            None,
            None,
            0,
            None,
        )
        .err()
        .unwrap();
        assert!(err_nw.contains("weights"), "unexpected missing weights error: {err_nw}");
    }
}
