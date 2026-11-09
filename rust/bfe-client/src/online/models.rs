//! Immutable model recipes for held weapons, items and projectile meshes.
//! The shared rig batch builder owns packing and bounds. A live view lends
//! only its published matrix to the existing instance and skin banks.
use crate::render_bridge::Bridge;
use bfe_content_native::{
    loader::authored_json,
    placed::{Part, Placements},
    BattleContent,
};
use bfe_render::instances as draw;
use serde_json::{json, Value};
use three::Matrix4;
pub struct Model {
    pub rows: Vec<[f64; draw::REC_STRIDE]>,
    pub radius: f64,
}
fn array<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T, String> {
    serde_json::from_value(v.clone()).map_err(|e| e.to_string())
}
impl Model {
    pub fn new(content: &BattleContent, bridge: &mut Bridge, textures: &[u32], name: &str, cascades: usize) -> Result<Option<Self>, String> {
        let Some(parts) = content.content.weapon_models.get(name) else {
            return Ok(None);
        };
        let parts: Vec<_> = parts
            .iter()
            .map(|p| {
                let mut r = authored_json(&p.record);
                r["data"] = json!(p.data);
                r["index"] = json!(p.index);
                r
            })
            .collect();
        let bytes = bfe_pose::rig::construction::batches(
            &serde_json::to_vec(&super::hulls::integers(json!({"rig":{"bones":[]},"parts":parts,"separate":[]})))
                .map_err(|e| e.to_string())?,
        )?;
        let batches: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let mut placed = Placements { parts: vec![], deferred: 0 };
        let mut radius: f64 = 0.;
        for b in batches.as_array().ok_or("invalid rigid model batches")? {
            let r: f64 = array(&b["radius"])?;
            radius = radius.max(r);
            placed.parts.push(Part {
                instance: 0,
                attachment: None,
                model: name.into(),
                source: array(&b["part"])?,
                geometry_key: format!("{name}/{}", b["part"]),
                matrix: Matrix4::default().elements,
                positions: array(&b["positions"])?,
                normals: array(&b["normals"])?,
                uv: array(&b["uv"])?,
                index: array(&b["indices"])?,
                joints: array(&b["joints"])?,
                weights: array(&b["weights"])?,
                bones: vec![0.; 16],
                bind_inverse: Matrix4::default().elements,
                center: array(&b["center"])?,
                radius: r,
            });
        }
        let admitted = crate::static_scene::props(bridge, content, textures, placed, cascades)?;
        Ok(Some(Self { rows: admitted.records, radius }))
    }
    // A one-bone rigid model uses the same attached skin convention as all
    // other instance rows. No geometry or material is constructed at draw.
    pub fn append(&self, bridge: &mut Bridge, matrix: &[f64; 16], mut records: usize, mut bones: usize) -> (usize, usize) {
        let mut inverse = Matrix4 { elements: *matrix };
        inverse.invert();
        let identity = Matrix4::default().elements;
        for template in &self.rows {
            if records == draw::MAX_RECORDS || bones == draw::MAX_BONES {
                continue;
            }
            let mut row = *template;
            row[draw::REC_WORLD..draw::REC_WORLD + 16].copy_from_slice(matrix);
            row[draw::REC_BIND_INVERSE..draw::REC_BIND_INVERSE + 16].copy_from_slice(&inverse.elements);
            row[draw::REC_BONES] = 1.;
            row[draw::REC_BONE_AT] = bones as f64;
            for (i, v) in matrix.iter().enumerate() {
                bridge.r.core.bones[bones * 16 + i] = *v as f32;
            }
            row[draw::REC_BIND..draw::REC_BIND + 16].copy_from_slice(&identity);
            let at = draw::IN_RECORDS + records * draw::REC_STRIDE;
            bridge.r.core.input[at..at + draw::REC_STRIDE].copy_from_slice(&row);
            records += 1;
            bones += 1;
        }
        (records, bones)
    }
}
impl Model {
    /// Rigid parts of the first-person rig use the same admission as held
    /// meshes. Read their original interleaved bytes without repacking JSON.
    pub fn part(content: &BattleContent, bridge: &mut Bridge, textures: &[u32], index: usize) -> Result<Self, String> {
        let mesh = &content.models[index];
        let (model, source) = mesh.name.rsplit_once('/').ok_or("invalid model part name")?;
        let source = source.parse::<usize>().map_err(|e| e.to_string())?;
        let attribute = |name: &str| -> Result<Vec<f32>, String> {
            let a = mesh.geometry.attributes.iter().find(|a| a.name == name).ok_or("missing rigid attribute")?;
            if a.scalar != bfe_content_native::geometry::Scalar::Float {
                return Err("rigid attribute is not float".into());
            }
            let mut out = Vec::new();
            let stride = if a.stride == 0 { a.size as usize * 4 } else { a.stride };
            let bytes = &mesh.geometry.buffers[a.buffer];
            for vertex in 0..mesh.geometry.vertices {
                for k in 0..a.size as usize {
                    let at = a.offset + vertex * stride + k * 4;
                    let b = bytes.get(at..at + 4).ok_or("rigid attribute exceeds buffer")?;
                    out.push(f32::from_le_bytes(b.try_into().unwrap()));
                }
            }
            Ok(out)
        };
        let count = mesh.geometry.vertices;
        let mut joints = vec![0u16; count * 4];
        let mut weights = vec![0.; count * 4];
        for i in 0..count {
            joints[i * 4] = 0;
            weights[i * 4] = 1.;
        }
        let part = Part {
            instance: 0,
            attachment: None,
            model: model.into(),
            source,
            geometry_key: mesh.name.clone(),
            matrix: Matrix4::default().elements,
            positions: attribute("position")?,
            normals: attribute("normal")?,
            uv: attribute("uv")?,
            index: mesh.geometry.indices.clone(),
            joints,
            weights,
            bones: vec![0.; 16],
            bind_inverse: Matrix4::default().elements,
            center: [0.; 3],
            radius: 2.,
        };
        let admitted = crate::static_scene::props(bridge, content, textures, Placements { parts: vec![part], deferred: 0 }, 0)?;
        Ok(Self { rows: admitted.records, radius: 2. })
    }
}
