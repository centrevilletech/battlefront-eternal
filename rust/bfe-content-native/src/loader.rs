//! game.js's native asset admission: the canonical content crate owns authored
//! definitions and derived collision data; this module adds the visible mesh
//! streams and texture resources the headless content loader never needed.
use crate::{
    geometry::{Attribute, Geometry, Group, Scalar},
    textures::Library,
};
use bfe_content::{
    battle::{Battle, Loader as BattleLoader},
    content::Content,
    files::{DirFiles, Files},
    install::BvhTree,
    Value as Authored,
};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialKind {
    Lit,
    Baked,
    Unlit,
    Dynamic,
    Terrain,
    Sky,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Opaque,
    Alpha,
    Additive,
}
pub struct Material {
    pub kind: MaterialKind,
    pub blend: Blend,
    pub cutout: bool,
    pub double_sided: bool,
    pub flags: u32,
    pub far: bool,
    pub texture: Option<usize>,
    pub bump: Option<usize>,
    // Retain authored values for the pass's load-time uniform binder. Names
    // disappear when that binder builds the native material row.
    pub record: Value,
}
pub struct Mesh {
    pub name: String,
    pub geometry: Geometry,
    pub material: Material,
}
pub struct ActorMesh {
    pub name: String,
    pub geometry: Geometry,
    pub textures: Vec<Option<usize>>,
}
pub struct BattleContent {
    pub content: Content,
    pub map: Value,
    pub world: Vec<Mesh>,
    pub models: Vec<Mesh>,
    pub actors: Vec<ActorMesh>,
    pub textures: Library,
    pub texture_paths:BTreeMap<String,String>,
    pub effects: Vec<(String, crate::effects::Effects)>,
    pub first_person: Option<bfe_presentation::unit_content::FirstPersonInput>,
    pub units: Vec<(String, bfe_presentation::unit_content::Input)>,
    pub buildings_bvh: BvhTree,
    pub terrain_bvh: Option<BvhTree>,
}
pub struct Loader {
    root: PathBuf,
    pub compressed: bool,
    pub max_texture_size: u32,
    pub texture_capabilities: u32,
}
impl Loader {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            compressed: true,
            max_texture_size: 16384,
            texture_capabilities: 0,
        }
    }
    pub fn load(&self, map: &str, query: &str) -> Result<BattleContent, String> {
        if map.is_empty() || !map.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err("invalid map name".into());
        }
        let mut textures = Library::new(
            &self.root,
            self.compressed,
            self.max_texture_size,
            self.texture_capabilities,
        )?;
        // Shared native loaders preserve the game's mission/side precedence,
        // model rebasing, clip remaps and collision arithmetic. Do not duplicate
        // those algorithms in a window-specific resource construction path.
        let files = DirFiles::new(&self.root);
        let source = CheckedFiles {
            files: &files,
            library: &textures,
        };
        let content = Content::build(Battle::load(&BattleLoader::new(&source), map, query)?)?;
        let map_path = format!("maps/{map}");
        let metadata: Value =
            serde_json::from_slice(&textures.read(&format!("{map_path}/map.json"))?)
                .map_err(|e| e.to_string())?;
        let vertices = textures.read(&format!("{map_path}/geom.bin"))?;
        let indices = textures.read(&format!("{map_path}/index.bin"))?;
        let map_table = table(&map_path, content.battle.world.get("textures"));
        let mut any = Vec::new();
        if let Some(side) = content.battle.sides.first() {
            any.push(table(&side.dir, side.units_file.get("textures")));
        }
        any.push(table("common", content.battle.common.get("textures")));
        any.push(map_table.clone());
        any.push(table(
            &content.battle.vehicle_dir,
            content
                .battle
                .vehicle_def
                .as_ref()
                .and_then(|v| v.get("textures")),
        ));
        for side in content.battle.sides.iter().skip(1) {
            any.push(table(&side.dir, side.units_file.get("textures")));
        }
        let resolve = |name: &str| {
            any.iter()
                .find_map(|t| t.get(&name.to_lowercase()).cloned())
        };
        let mut world = Vec::new();
        for record in metadata["meshes"]
            .as_array()
            .ok_or("map has no mesh table")?
        {
            let geometry = Geometry::world(record, &vertices, &indices)?;
            let texture = image(&mut textures, &map_table, record["texture"].as_str())?;
            let bump = image(&mut textures, &map_table, record["bump"].as_str())?;
            if let Some(layers) = record["layers"].as_array() {
                for layer in layers {
                    let _ = image(&mut textures, &map_table, layer.as_str())?;
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
            world.push(Mesh {
                name: record["texture"].as_str().unwrap_or("").into(),
                geometry,
                material: material(record.clone(), kind, flags, texture, bump),
            });
            // Painted terrain weights are byte-normalized attributes, not PNG
            // images. Admitting them here avoids a hidden browser fetch later.
            if let Some(weights) = record["weights"].as_str() {
                let mesh = world.last_mut().unwrap();
                let weights = textures.read(&format!("{map_path}/{weights}"))?;
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
                    mesh.geometry.attributes.push(Attribute {
                        name: name.into(),
                        buffer,
                        size: 4,
                        scalar: Scalar::U8,
                        normalized: true,
                        stride: 8,
                        offset,
                    });
                }
            }
        }
        // Visible-only packs are absent from the headless world's requirements:
        // common/side effects, prop extras, and ships in the far sky. They use
        // modelPack's same byte layout and remain native content, not DOM assets.
        let mut visible_packs = Vec::new();
        let mut scoped_packs = Vec::new();
        let mut extra_pack =
            |directory: &str, base: &str, records: Option<&Authored>| -> Result<(), String> {
                if records
                    .and_then(Authored::as_object)
                    .is_none_or(|o| o.entries().next().is_none())
                {
                    return Ok(());
                }
                let vertex =
                    crate::geometry::floats(&textures.read(&format!("{directory}/{base}.bin"))?)?;
                let index = crate::geometry::words(
                    &textures.read(&format!("{directory}/{base}_index.bin"))?,
                )?;
                visible_packs.push(bfe_content::models::model_pack(records, &vertex, &index)?);
                Ok(())
            };
        extra_pack("common", "fxmodels", content.battle.effects.get("models"))?;
        for side in &content.battle.sides {
            extra_pack(
                side.dir.trim_start_matches('/'),
                "fxmodels",
                side.fx.as_ref().and_then(|v| v.get("models")),
            )?;
        }
        extra_pack(
            &map_path,
            "props_extras",
            content.battle.props.as_ref().and_then(|v| v.get("extras")),
        )?;
        let mut first_person = None;
        if let Some(name) = content
            .battle
            .soldier_def
            .get("firstPerson")
            .and_then(Authored::as_str)
        {
            let folder = content
                .battle
                .sides
                .first()
                .ok_or("missing played side")?
                .dir
                .trim_start_matches('/');
            let base = format!("{folder}/firstperson/{name}");
            let path = textures.path(&format!("{base}.json"))?;
            let anim_path = textures.path("common/anims_fp.json")?;
            if path.exists() && anim_path.exists() {
                let rig = bfe_content::json::parse_bytes(&textures.read(&format!("{base}.json"))?)
                    .map_err(|e| e.to_string())?;
                let model = rig
                    .get("model")
                    .and_then(Authored::as_str)
                    .ok_or("first-person rig has no model")?;
                let records =
                    Authored::Object(std::rc::Rc::new(bfe_content::Object::from_written(vec![(
                        std::rc::Rc::from(model),
                        rig.get("parts")
                            .cloned()
                            .ok_or("first-person rig has no parts")?,
                    )])));
                let vertex = crate::geometry::floats(&textures.read(&format!("{base}.bin"))?)?;
                let index = crate::geometry::words(&textures.read(&format!("{base}_index.bin"))?)?;
                visible_packs.push(bfe_content::models::model_pack(
                    Some(&records),
                    &vertex,
                    &index,
                )?);
                let metadata =
                    bfe_content::json::parse_bytes(&textures.read("common/anims_fp.json")?)
                        .map_err(|e| e.to_string())?;
                let data = std::rc::Rc::from(crate::geometry::floats(
                    &textures.read("common/anims_fp.bin")?,
                )?);
                let bones = rig
                    .get("bones")
                    .and_then(Authored::as_array)
                    .ok_or("first-person bones missing")?
                    .iter()
                    .map(|b| {
                        b.get("name")
                            .and_then(Authored::as_str)
                            .unwrap_or("")
                            .to_string()
                    })
                    .collect::<Vec<_>>();
                let clips = bfe_content::clips::load_clips(&metadata, &data, Some(&bones))?;
                first_person = Some(crate::units::first_person(
                    &rig,
                    &clips,
                    content
                        .battle
                        .soldier_def
                        .get("base")
                        .and_then(Authored::as_str)
                        == Some("walkerdroid"),
                )?);
            }
        }
        let sky_file = textures.path(&format!("{map_path}/sky/flyers.json"))?;
        if sky_file.exists() {
            let sky = bfe_content::json::parse(
                &std::fs::read_to_string(sky_file).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let sky_textures = table(&format!("{map_path}/sky"), sky.get("textures"));
            // Only a declared pack is required. Older prepared maps omit sky
            // flyers entirely, which is an authored absence on both clients.
            let path = textures.path(&format!("{map_path}/sky/flyers.bin"))?;
            if path.exists() {
                let vertices = crate::geometry::floats(
                    &textures.read(&format!("{map_path}/sky/flyers.bin"))?,
                )?;
                let index = crate::geometry::words(
                    &textures.read(&format!("{map_path}/sky/flyers_index.bin"))?,
                )?;
                scoped_packs.push((
                    "sky",
                    bfe_content::models::model_pack(sky.get("models"), &vertices, &index)?,
                    sky_textures,
                ));
            }
        }
        let environment_path = textures.path(&format!("{map_path}/environment/effects.json"))?;
        let environment = if environment_path.exists() {
            Some(
                bfe_content::json::parse_bytes(
                    &textures.read(&format!("{map_path}/environment/effects.json"))?,
                )
                .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        if let Some(data) = &environment {
            if data
                .get("models")
                .and_then(Authored::as_object)
                .is_some_and(|m| m.entries().next().is_some())
            {
                let vertex = crate::geometry::floats(
                    &textures.read(&format!("{map_path}/environment/models.bin"))?,
                )?;
                let index = crate::geometry::words(
                    &textures.read(&format!("{map_path}/environment/models_index.bin"))?,
                )?;
                scoped_packs.push((
                    "environment",
                    bfe_content::models::model_pack(data.get("models"), &vertex, &index)?,
                    table(&format!("{map_path}/environment"), data.get("textures")),
                ));
            }
        }
        let mut models = Vec::new();
        let mut packs = content
            .weapon_packs
            .iter()
            .map(|(_, pack)| pack)
            .collect::<Vec<_>>();
        packs.extend(content.vehicle_models.iter());
        packs.extend(content.vehicle_extras.iter());
        packs.extend(content.props_models.iter());
        packs.extend(visible_packs.iter());
        for pack in packs {
            append_pack(&mut models, pack, "", &resolve, &mut textures)?;
        }
        for (family, pack, table) in scoped_packs {
            let scoped = |name: &str| table.get(&name.to_lowercase()).cloned();
            append_pack(
                &mut models,
                &pack,
                &format!("{family}/"),
                &scoped,
                &mut textures,
            )?;
        }
        let mut actors = Vec::new();
        for (name, body) in &content.battle.bodies {
            actors.push(actor_mesh(name, body, &resolve, &mut textures)?);
        }
        let mut units: Vec<(String, bfe_presentation::unit_content::Input)> = content
            .actors
            .iter()
            .filter_map(|(name, actor)| {
                content
                    .body_clips
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, clips)| {
                        (
                            name.clone(),
                            crate::units::soldier(
                                actor,
                                clips,
                                content.battle.anim_meta.get("sets"),
                            ),
                        )
                    })
            })
            .collect();
        for (class, clips) in &content.walker_clips {
            let Some((side_name, class_name)) = class.split_once('/') else {
                continue;
            };
            let Some(side) = content.battle.sides.iter().find(|s| s.name == side_name) else {
                continue;
            };
            let Some(def) = side.units.get(class_name) else {
                continue;
            };
            let model = def.get("model").and_then(Authored::as_str).unwrap_or("");
            if let Some((_, actor)) = content
                .actors
                .iter()
                .find(|(name, _)| name == &format!("{side_name}/{model}"))
            {
                units.push((
                    class.clone(),
                    crate::units::walker(
                        actor,
                        clips,
                        def.get("BallRadius")
                            .and_then(Authored::as_f64)
                            .unwrap_or(bfe_pose::rig::droideka::BALL_CENTER),
                    ),
                ));
            }
        }
        let mut libraries = vec![
            content.battle.props.as_ref().and_then(|v| v.get("effects")),
            content.battle.effects.get("effects"),
        ];
        for side in content.battle.sides.iter().skip(1) {
            libraries.push(side.fx.as_ref().and_then(|v| v.get("effects")));
        }
        libraries.push(
            content
                .battle
                .side_fx
                .as_ref()
                .and_then(|v| v.get("effects")),
        );
        libraries.push(
            content
                .battle
                .vehicle_def
                .as_ref()
                .and_then(|v| v.get("effects")),
        );
        let mut effects = vec![(
            "battle".into(),
            crate::effects::Effects::compile(&crate::effects::merge(libraries))?,
        )];
        if let Some(data) = environment {
            if let Some(library) = data.get("effects") {
                effects.push((
                    "environment".into(),
                    crate::effects::Effects::compile(library)?,
                ));
            }
        }
        let collision = &content.collision;
        validate_triangles(&collision.positions, &collision.indices)?;
        validate_triangles(&collision.terrain_positions, &collision.terrain_indices)?;
        let buildings_bvh = bfe_content::bvh::build(
            bfe_content::bvh::Positions::F32(&collision.positions),
            &collision.indices,
        );
        let terrain_bvh = (!collision.terrain_indices.is_empty()).then(|| {
            bfe_content::bvh::build(
                bfe_content::bvh::Positions::F32(&collision.terrain_positions),
                &collision.terrain_indices,
            )
        });
        let mut texture_paths=BTreeMap::new();for table in any{for(name,path)in table{texture_paths.entry(name).or_insert(path);}}
        Ok(BattleContent {
            content,
            map: metadata,
            world,
            models,
            actors,
            textures,
            texture_paths,
            units,
            first_person,
            effects,
            buildings_bvh,
            terrain_bvh,
        })
    }
}
fn material(
    record: Value,
    kind: MaterialKind,
    flags: u32,
    texture: Option<usize>,
    bump: Option<usize>,
) -> Material {
    Material {
        kind,
        blend: if flags & 0x80 != 0 {
            Blend::Additive
        } else if flags & 4 != 0 && flags & 2 == 0 {
            Blend::Alpha
        } else {
            Blend::Opaque
        },
        cutout: flags & 2 != 0,
        double_sided: flags & 0x86 != 0,
        far: truth(&record["far"]),
        flags,
        texture,
        bump,
        record,
    }
}
fn truth(value: &Value) -> bool {
    !matches!(value, Value::Null | Value::Bool(false)) && value.as_f64() != Some(0.0)
}
fn table(directory: &str, value: Option<&Authored>) -> BTreeMap<String, String> {
    value
        .and_then(Authored::as_object)
        .map(|v| {
            v.entries()
                .filter_map(|(name, value)| {
                    value.as_str().map(|p| {
                        (
                            name.to_lowercase(),
                            format!("{}/{p}", directory.trim_start_matches('/')),
                        )
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}
fn image(
    library: &mut Library,
    table: &BTreeMap<String, String>,
    name: Option<&str>,
) -> Result<Option<usize>, String> {
    name.and_then(|n| table.get(&n.to_lowercase()))
        .map(|p| library.load(p))
        .transpose()
}
fn validate_triangles(positions: &[f32], indices: &[u32]) -> Result<(), String> {
    if !positions.len().is_multiple_of(3)
        || !indices.len().is_multiple_of(3)
        || indices.iter().any(|&i| i as usize >= positions.len() / 3)
    {
        return Err("collision triangle extent is invalid".into());
    }
    Ok(())
}
// The canonical Files trait intentionally accepts a URL-shaped path. Keep the
// native directory trust boundary outside it, including symlink resolution.
struct CheckedFiles<'a> {
    files: &'a DirFiles,
    library: &'a Library,
}
impl Files for CheckedFiles<'_> {
    fn read(&self, path: &str) -> Option<std::rc::Rc<[u8]>> {
        self.library.path(path).ok()?;
        self.files.read(path)
    }
}

/// actor.js keeps one vertex stream for all material groups, with four skin
/// indices/weights even though this imported actor format binds one bone.
fn actor_mesh(
    name: &str,
    body: &bfe_content::battle::BodyFiles,
    resolve: &impl Fn(&str) -> Option<String>,
    library: &mut Library,
) -> Result<ActorMesh, String> {
    let vertex = crate::geometry::floats(&body.buf)?;
    let indices = crate::geometry::words(&body.index_buf)?;
    let parts = body
        .def
        .get("parts")
        .and_then(Authored::as_array)
        .ok_or("actor has no parts")?;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uv = Vec::new();
    let mut bones = Vec::new();
    let mut weights = Vec::new();
    let mut index = Vec::new();
    let mut groups = Vec::new();
    let mut material_names = Vec::new();
    let mut textures = Vec::new();
    for part in parts {
        let number = |key: &str| -> Result<usize, String> {
            let n = part
                .get(key)
                .and_then(Authored::as_f64)
                .ok_or_else(|| format!("actor has no {key}"))?;
            if n < 0.0 || n.fract() != 0.0 || n > usize::MAX as f64 {
                return Err(format!("invalid actor {key}"));
            }
            Ok(n as usize)
        };
        let first = number("vertexOffset")?;
        if first % 4 != 0 {
            return Err("actor vertex offset is misaligned".into());
        }
        let first = first / 4;
        let count = number("vertexCount")?;
        let end = first
            .checked_add(count.checked_mul(9).ok_or("actor vertex extent overflow")?)
            .ok_or("actor vertex extent overflow")?;
        let source = vertex
            .get(first..end)
            .ok_or("actor vertices exceed storage")?;
        for v in source.as_chunks::<9>().0 {
            positions.extend_from_slice(&v[..3]);
            normals.extend_from_slice(&v[3..6]);
            uv.extend_from_slice(&v[6..8]);
            if !v[8].is_finite() || v[8] < 0.0 || v[8] > u16::MAX as f32 {
                return Err("actor bone index exceeds its format".into());
            }
            bones.extend([v[8] as u16, 0, 0, 0]);
            weights.extend([1.0f32, 0.0, 0.0, 0.0]);
        }
        let texture = part.get("texture").and_then(Authored::as_str).unwrap_or("");
        let material = match material_names.iter().position(|n| n == texture) {
            Some(id) => id,
            None => {
                material_names.push(texture.to_string());
                textures.push(resolve(texture).map(|p| library.load(&p)).transpose()?);
                material_names.len() - 1
            }
        };
        let at = number("indexOffset")?;
        if at % 4 != 0 {
            return Err("actor index offset is misaligned".into());
        }
        let at = at / 4;
        let count = number("indexCount")?;
        let end = at.checked_add(count).ok_or("actor index extent overflow")?;
        groups.push(Group {
            first: index.len(),
            count,
            material,
        });
        index.extend_from_slice(indices.get(at..end).ok_or("actor indices exceed storage")?);
    }
    let vertices = positions.len() / 3;
    let buffers = vec![
        positions.iter().flat_map(|f| f.to_le_bytes()).collect(),
        normals.iter().flat_map(|f| f.to_le_bytes()).collect(),
        uv.iter().flat_map(|f| f.to_le_bytes()).collect(),
        bones.iter().flat_map(|i| i.to_le_bytes()).collect(),
        weights.iter().flat_map(|f| f.to_le_bytes()).collect(),
    ];
    let attributes = [
        ("position", 3, Scalar::Float),
        ("normal", 3, Scalar::Float),
        ("uv", 2, Scalar::Float),
        ("skinIndex", 4, Scalar::U16),
        ("skinWeight", 4, Scalar::Float),
    ]
    .into_iter()
    .enumerate()
    .map(|(buffer, (name, size, scalar))| Attribute {
        name: name.into(),
        buffer,
        size,
        scalar,
        normalized: false,
        stride: size as usize * scalar.bytes(),
        offset: 0,
    })
    .collect();
    let geometry = Geometry {
        buffers,
        attributes,
        indices: index,
        vertices,
        groups,
    };
    geometry.validate()?;
    Ok(ActorMesh {
        name: name.into(),
        geometry,
        textures,
    })
}

pub fn authored_json(value: &Authored) -> Value {
    match value {
        Authored::Null => Value::Null,
        Authored::Bool(v) => Value::Bool(*v),
        Authored::Number(v) => serde_json::json!(v),
        Authored::String(v) => Value::String(v.to_string()),
        Authored::Array(v) => v.iter().map(authored_json).collect(),
        Authored::Object(v) => v
            .entries()
            .map(|(n, v)| (n.into(), authored_json(v)))
            .collect::<serde_json::Map<_, _>>()
            .into(),
    }
}

// modelPack is shared by held weapons, FX, sky ships and environment models.
// A scoped pack keeps its own texture resolver; a same-named common bitmap
// must not replace an environment or sky texture during native admission.
pub fn append_pack(
    out: &mut Vec<Mesh>,
    pack: &bfe_content::models::ModelMap,
    prefix: &str,
    resolve: &impl Fn(&str) -> Option<String>,
    textures: &mut Library,
) -> Result<(), String> {
    for (name, parts) in pack.iter() {
        for (part, source) in parts.iter().enumerate() {
            let texture_name = source.field("texture").and_then(Authored::as_str);
            let texture = texture_name
                .and_then(resolve)
                .map(|p| textures.load(&p))
                .transpose()?;
            let flags = source
                .field("flags")
                .and_then(Authored::as_f64)
                .unwrap_or(1.0) as u32;
            let record = authored_json(&source.record);
            let geometry = Geometry {
                buffers: vec![source.data.iter().flat_map(|f| f.to_le_bytes()).collect()],
                attributes: [("position", 3, 0), ("normal", 3, 12), ("uv", 2, 24)]
                    .into_iter()
                    .map(|(name, size, offset)| Attribute {
                        name: name.into(),
                        buffer: 0,
                        size,
                        scalar: Scalar::Float,
                        normalized: false,
                        stride: 32,
                        offset,
                    })
                    .collect(),
                indices: source.index.clone(),
                vertices: source.data.len() / 8,
                groups: vec![Group {
                    first: 0,
                    count: source.index.len(),
                    material: 0,
                }],
            };
            geometry.validate()?;
            out.push(Mesh {
                name: format!("{prefix}{name}/{part}"),
                geometry,
                material: material(
                    record,
                    if flags & 1 != 0 {
                        MaterialKind::Dynamic
                    } else {
                        MaterialKind::Unlit
                    },
                    flags,
                    texture,
                    None,
                ),
            });
        }
    }
    Ok(())
}
