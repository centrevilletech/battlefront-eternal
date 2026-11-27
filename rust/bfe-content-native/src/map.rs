//! Shared typed map admission for server, client, and inspector.
//!
//! Loads base and mission-selected layer geometry, materials, textures,
//! placed props, lights, regions, paths, spawns, command posts, world
//! animations, and collision BVHs without requiring a side/unit roster.
use crate::{
    geometry::Geometry,
    loader::{
        append_pack_swbf2, authored_json, build_collision_bvhs, far_terrain, image,
        material, material_swbf2_extended, table, truth, CheckedFiles, Loader, MaterialKind, Mesh,
    },
    textures::Library,
};
use bfe_content::{
    install::BvhTree,
    map::{MapAdmission, MapCounts},
    models::{model_pack_swbf2, ModelMap},
    GameFamily,
};
use serde_json::Value;
use std::{collections::BTreeMap, rc::Rc};

/// Visual and collision assets admitted for a map and its active layers,
/// independent of any side/unit roster.
pub struct AdmittedMap {
    pub admission: Rc<MapAdmission>,
    /// Composed map JSON (base + active layers).
    pub map: Value,
    /// Composed world meshes across the base map and every active layer.
    pub world: Vec<Mesh>,
    /// Packed visual prop models from `props.bin` / `props_index.bin`.
    pub prop_models: Vec<Mesh>,
    /// Decoded prop model records for placement rig construction.
    pub props_pack: Option<ModelMap>,
    pub textures: Library,
    pub texture_paths: BTreeMap<String, String>,
    pub buildings_bvh: BvhTree,
    /// `None` on void-terrain maps (such as 2005 space maps).
    pub terrain_bvh: Option<BvhTree>,
}

impl AdmittedMap {
    pub fn counts(&self) -> MapCounts {
        self.admission.counts()
    }
}

/// Builds composed world meshes from every `MeshSource` in `admission` (base map
/// followed by each active layer in mission order).
pub(crate) fn build_world_meshes(
    admission: &MapAdmission,
    composed_map_json: &Value,
    textures: &mut Library,
    map_table: &BTreeMap<String, String>,
) -> Result<Vec<Mesh>, String> {
    let mut world = Vec::new();
    for source in &admission.mesh_sources {
        let vertices = textures.read(&source.geom_path)?;
        let indices = textures.read(&source.index_path)?;
        let source_dir = source
            .geom_path
            .rsplit_once('/')
            .map_or("", |(dir, _)| dir);
        for authored in &source.meshes {
            let record = crate::placed::json_integers(authored_json(authored));
            let geometry = Geometry::world(&record, &vertices, &indices)?;
            let texture = image(textures, map_table, record["texture"].as_str())?;
            let bump_name = if admission.family == GameFamily::Swbf2 {
                record["bump"]
                    .as_str()
                    .or_else(|| record["material"]["bump"].as_str())
            } else {
                record["bump"].as_str()
            };
            let bump = image(textures, map_table, bump_name)?;
            if let Some(layers) = record["layers"].as_array() {
                for layer in layers {
                    let _ = image(textures, map_table, layer.as_str())?;
                }
            }
            let flags = record["flags"].as_u64().unwrap_or(0) as u32;
            let kind = if truth(&record["sky"]) {
                MaterialKind::Sky
            } else if truth(&record["terrain"]) {
                MaterialKind::Terrain
            } else if record["colorKind"].as_str() == Some("lit") || flags & 0x40 != 0 {
                MaterialKind::Baked
            } else if flags & 1 == 0 {
                MaterialKind::Unlit
            } else if truth(&record["dynamic"]) {
                MaterialKind::Dynamic
            } else {
                MaterialKind::Lit
            };
            let mat = if admission.family == GameFamily::Swbf2 {
                let mut mat = material_swbf2_extended(record.clone(), kind, flags, texture, bump)?;
                if let Some(name) = mat.extended.as_ref().and_then(|s| s.bump.as_deref()) {
                    mat.bump = image(textures, map_table, Some(name))?;
                }
                mat
            } else {
                material(record.clone(), kind, flags, texture, bump)
            };
            world.push(Mesh {
                name: record["texture"].as_str().unwrap_or("").into(),
                geometry,
                material: mat,
            });
            if let Some(weights) = record["weights"].as_str() {
                let mesh = world.last_mut().unwrap();
                let weight_path = if source_dir.is_empty() {
                    weights.to_string()
                } else {
                    format!("{source_dir}/{weights}")
                };
                let weights = textures.read(&weight_path)?;
                if weights.len()
                    != mesh
                        .geometry
                        .vertices
                        .checked_mul(8)
                        .ok_or("terrain weight extent overflow")?
                {
                    return Err("terrain weight count differs from geometry".into());
                }
                let buffer = mesh.geometry.buffers.len();
                mesh.geometry.buffers.push(weights);
                for (name, offset) in [("weights0", 0), ("weights1", 4)] {
                    mesh.geometry.attributes.push(crate::geometry::Attribute {
                        name: name.into(),
                        buffer,
                        size: 4,
                        scalar: crate::geometry::Scalar::U8,
                        normalized: true,
                        stride: 8,
                        offset,
                    });
                }
            }
            if kind == MaterialKind::Terrain {
                let low = &composed_map_json["distance"]["lowRes"];
                if let Some(name) = low["texture"].as_str().filter(|name| !name.is_empty()) {
                    let texture = image(textures, map_table, Some(name))?;
                    let far = far_terrain(world.last().unwrap(), name, &low["scale"], texture);
                    world.push(far);
                }
            }
        }
    }
    Ok(world)
}

impl Loader {
    /// Admits a 2005 map and its active layers without loading any side or unit
    /// roster. Shared by the inspector, client, and server.
    ///
    /// 2004 content stays on [`Loader::load`] without a duplicate reader.
    pub fn admit_map(
        &self,
        family: GameFamily,
        map: &str,
        mission: Option<&str>,
        layers: &[&str],
    ) -> Result<AdmittedMap, String> {
        if family != GameFamily::Swbf2 {
            return Err("Loader::admit_map is 2005-only; use Loader::load for 2004 content".into());
        }
        if map.is_empty() || !map.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err("invalid map name".into());
        }
        let mut textures = Library::new(
            self.root(),
            self.compressed,
            self.max_texture_size,
            self.texture_capabilities,
        )?;
        let files = self.files(family);
        let source = CheckedFiles {
            files: &files,
            library: &textures,
        };
        let admission = Rc::new(MapAdmission::load(&source, family, map, mission, layers)?);
        let map_path = format!("maps/{map}");
        let map_table = table(&map_path, admission.world.get("textures"));
        let composed_json = crate::placed::json_integers(authored_json(&admission.world));
        let world = build_world_meshes(&admission, &composed_json, &mut textures, &map_table)?;

        let mut props_pack = None;
        let mut prop_models = Vec::new();
        if let (Some(props), Some(buf), Some(idx_buf)) = (
            admission.props.as_ref(),
            admission.props_buf.as_ref(),
            admission.props_index_buf.as_ref(),
        ) {
            if !buf.is_empty() && !idx_buf.is_empty() {
                let vertex = crate::geometry::floats(buf)?;
                let index = crate::geometry::words(idx_buf)?;
                let records = prop_parts_records(props.get("models"), family);
                let pack = model_pack_swbf2(
                    records.as_ref(),
                    &vertex,
                    &index,
                    &format!("{map_path}/props.json"),
                )?;
                let resolve = |name: &str| map_table.get(&name.to_lowercase()).cloned();
                append_pack_swbf2(&mut prop_models, &pack, "", &resolve, &mut textures)?;
                props_pack = Some(pack);
            }
        }

        let (buildings_bvh, terrain_bvh) = build_collision_bvhs(&admission.collision)?;

        Ok(AdmittedMap {
            admission,
            map: composed_json,
            world,
            prop_models,
            props_pack,
            textures,
            texture_paths: map_table,
            buildings_bvh,
            terrain_bvh,
        })
    }
}

fn prop_parts_records(models: Option<&bfe_content::Value>, family: GameFamily) -> Option<bfe_content::Value> {
    let obj = models?.as_object()?;
    let entries: Vec<(Rc<str>, bfe_content::Value)> = obj
        .entries()
        .filter_map(|(k, v)| {
            if family == GameFamily::Swbf2 && v.as_array().is_some() {
                Some((Rc::from(k), v.clone()))
            } else {
                v.get("parts")
                    .filter(|p| p.as_array().is_some())
                    .map(|p| (Rc::from(k), p.clone()))
            }
        })
        .collect();
    Some(bfe_content::Value::Object(Rc::new(
        bfe_content::Object::from_written(entries),
    )))
}
