//! Cold placements from props.js::LiveObjects. The shared rig batch builder
//! owns bind transforms and grouping; this host supplies initial looks only.
use crate::{loader::authored_json, BattleContent};
use serde_json::{json, Value};

// Batch arrays are already in the root bind frame. Placement remains a
// separate matrix, and skeleton rows retain attached-mode rounding rather
// than baking an approximate world-space replacement for a skinned draw.
pub struct Part {
    pub model: String,
    pub source: usize,
    pub geometry_key: String,
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
pub fn admit(content: &BattleContent) -> Result<Placements, String> {
    let mut result = Placements {
        parts: vec![],
        deferred: 0,
    };
    let Some(props) = &content.content.battle.props else {
        return Ok(result);
    };
    let props = json_integers(authored_json(props));
    let Some(instances) = props["instances"].as_array() else {
        return Ok(result);
    };
    let Some(pack) = &content.content.props_models else {
        return Ok(result);
    };
    let mut placements = Vec::with_capacity(instances.len());
    for desc in instances {
        placements.push(desc.clone());
        if desc["base"] == "trap" {
            if let Some(model) = desc["trap"]["pad"]["model"].as_str() {
                let matrix = crate::pad::placement(
                    array(&desc["matrix"])?,
                    array(&desc["trap"]["pad"]["at"])?,
                    &content.content.collision,
                )?;
                placements.push(json!({"base":"staticpad","model":model,"matrix":matrix}));
            }
        }
    }
    // One immutable recipe per look, shared across every authored placement.
    let mut cache = std::collections::BTreeMap::<String, Value>::new();
    for desc in &placements {
        let base = desc["base"].as_str().unwrap_or("");
        // AnimatedProp.present samples clips even without gameplay. Retain
        // that distinction instead of publishing an incorrect bind-pose door.
        if matches!(base, "animatedprop" | "door") {
            result.deferred += 1;
            continue;
        }
        // Destroyed geometry is a different model and is not part of the
        // initial visible set. Texture/resource admission follows this chosen
        // look, rather than uploading every dormant wreck in the prop pack.
        let name = desc["unbuilt"]["model"]
            .as_str()
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
        if !cache.contains_key(&key) {
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
                key.clone(),
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
            );
        }
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
        for b in cache[&key].as_array().ok_or("invalid rig batches")? {
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
                model: name.into(),
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
    }
    Ok(result)
}

// JSON.stringify emits integral authored numbers without a decimal point.
// Match that cold transport when the shared rig builder reads typed counts
// and joints; matrix/position readers accept these exact integers as f64.
fn json_integers(mut v: Value) -> Value {
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
