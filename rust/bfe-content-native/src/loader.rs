//! native asset admission: the canonical content crate owns authored
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
    layout::family_dir,
    GameFamily, Value as Authored,
};
use bfe_render::static_world::{ExtendedBlend, ExtendedRecord, ExtendedSurface};
use serde_json::Value;
use std::{collections::BTreeMap, path::{Path, PathBuf}};

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
    pub extended: Option<ExtendedSurface>,
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
    pub family: crate::GameFamily,
    pub content: Content,
    pub static_inspection: bool,
    pub(crate) prop_attributes: Vec<u8>,
    pub map: Value,
    pub world: Vec<Mesh>,
    pub models: Vec<Mesh>,
    pub sky_flyers: Option<Value>,
    pub actors: Vec<ActorMesh>,
    /// A class's own skin, by "<side>/<class>", for classes whose
    /// OverrideTexture or OverrideTexture2 re-skins a shared body: its body's
    /// material textures in the body's order, with the overrides in place.
    pub skins: Vec<(String, Vec<Option<usize>>)>,
    pub textures: Library,
    pub texture_paths:BTreeMap<String,String>,
    pub effects: Vec<(String, crate::effects::Effects)>,
    /// The mission's visual resources are held outside the content used for network definitions.
    mission_effects: Option<Authored>,
    /// The map's placed effects: each plays its (prefixed) effect group.
    pub environment_instances: Vec<EnvironmentInstance>,
    /// The fountains' basins: centre and radius.
    pub environment_basins: Vec<[f64; 4]>,
    pub first_person: Option<bfe_presentation::unit_content::FirstPersonInput>,
    pub units: Vec<(String, bfe_presentation::unit_content::Input)>,
    pub buildings_bvh: BvhTree,
    pub terrain_bvh: Option<BvhTree>,
    /// The definitions derived from `content`, built once on first use.
    defs: std::cell::OnceCell<std::rc::Rc<bfe_content::defs::Defs>>,
    /// Each 2005 craft class's model as a rig with its takeoff clip (empty for
    /// 2004 and for prepared copies without `craftrigs.json`).
    pub craft_rigs: Vec<bfe_content::content::CraftRig>,
}
impl BattleContent {
    pub fn visual_effects(&self) -> &Authored {
        self.mission_effects.as_ref().unwrap_or(&self.content.battle.effects)
    }
    /// `Defs::build(&self.content)`, built once: a dozen owners read it while
    /// a battle loads, each costing tens of milliseconds on a slow PC.
    pub fn defs(&self) -> Result<std::rc::Rc<bfe_content::defs::Defs>, String> {
        if let Some(defs) = self.defs.get() {
            return Ok(std::rc::Rc::clone(defs));
        }
        let defs = std::rc::Rc::new(bfe_content::defs::Defs::build(&self.content)?);
        let _ = self.defs.set(std::rc::Rc::clone(&defs));
        Ok(defs)
    }
}
pub struct EnvironmentInstance {
    pub effect: String,
    /// Column-major world matrix, as three's Matrix4.fromArray reads it.
    pub matrix: [f64; 16],
    /// The node of its object it hangs from.
    pub hardpoint: String,
}
pub struct Loader {
    root: PathBuf,
    family: crate::GameFamily,
    pub compressed: bool,
    pub max_texture_size: u32,
    pub texture_capabilities: u32,
    /// For a 2005 battle, the generation of the content folder an earlier
    /// reading saw (a join's offered fingerprint): its files are read as that
    /// reading saw them, found or not, and every other file from the folder.
    pub pinned: Option<std::sync::Arc<bfe_content::files::Pinned>>,
}
impl Loader {
    /// Opens the family's own folder beneath a shared content root.
    pub fn new(root: impl Into<PathBuf>, family: crate::GameFamily) -> Self {
        Self::from_resolved_root(family_dir(&root.into(), family), family)
    }

    /// A root that already is one prepared folder: the family is read from the
    /// folder itself (a marked `swbf2/.content` is the 2005 game).
    pub fn detect(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let family = if root.ends_with("swbf2/.content") && root.join(".prepare-swbf2.json").is_file() {
            crate::GameFamily::Swbf2
        } else {
            crate::GameFamily::Swbf1
        };
        Self::from_resolved_root(root, family)
    }

    /// A qualified session or scenery selection names its family explicitly.
    /// A bare id belongs to the selected preparation root.
    pub fn for_map<'a>(root: &Path, id: &'a str) -> (Self, &'a str) {
        let (family, name) = crate::GameFamily::split_id(id);
        let loader = if name == id { Self::detect(root) } else { Self::from_resolved_root(root, family) };
        (loader, name)
    }

    /// Opens a single prepared folder whose physical location is already known.
    /// The caller selects the family together with the content root; map
    /// metadata is interpreted only after that choice.
    pub fn from_resolved_root(root: impl Into<PathBuf>, family: crate::GameFamily) -> Self {
        Self {
            root: root.into(),
            family,
            compressed: true,
            max_texture_size: 16384,
            texture_capabilities: 0,
            pinned: None,
        }
    }
    /// The prepared folder this loader reads, opened for `family`.
    pub(crate) fn files(&self, family: GameFamily) -> DirFiles {
        DirFiles::from_resolved_root(&self.root, family)
    }
    pub(crate) fn root(&self) -> &std::path::Path {
        &self.root
    }
    /// The family selected together with the content root.
    pub fn family(&self) -> crate::GameFamily {
        self.family
    }
    pub fn load_for_family(
        &self,
        family: GameFamily,
        map: &str,
        query: &str,
    ) -> Result<BattleContent, String> {
        match family {
            GameFamily::Swbf1 => self.load(map, query),
            GameFamily::Swbf2 => self.load_swbf2(map, query, None),
        }
    }
    fn library(&self, map: &str) -> Result<Library, String> {
        if map.is_empty() || !map.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err("invalid map name".into());
        }
        Library::new(&self.root, self.compressed, self.max_texture_size, self.texture_capabilities)
    }

    /// Admit the base world and optional visual layer for a developer capture.
    pub fn inspect(&self, map: &str, layer_name: Option<&str>) -> Result<BattleContent, String> {
        if self.family == crate::GameFamily::Swbf1 {
            if layer_name.is_some() { return Err("world has no selectable layer".into()); }
            return self.load(map, &format!("map={map}&seed=1"));
        }
        let textures = self.library(map)?;
        let dir = format!("maps/{map}");
        let mut metadata: Value = serde_json::from_slice(&textures.read(&format!("{dir}/map.json"))?)
            .map_err(|e| e.to_string())?;
        let layer = layer_name.map(|name| metadata["layers"].get(name).cloned()
            .ok_or_else(|| format!("unknown world layer {name}"))).transpose()?;
        let mut vertices = textures.read(&format!("{dir}/geom.bin"))?;
        let mut indices = textures.read(&format!("{dir}/index.bin"))?;
        if let Some(layer) = &layer {
            crate::inspection::append_layer(&mut metadata, layer, &textures, &dir, &mut vertices, &mut indices)?;
        }
        let battle = crate::inspection::scenery(&textures, map, &metadata, layer.as_ref())?;
        self.visuals(Content::build(battle)?, metadata, dir, vertices, indices, textures, true)
    }

    pub fn load(&self, map: &str, query: &str) -> Result<BattleContent, String> {
        let textures = self.library(map)?;
        let files = self.files(self.family);
        // Shared native loaders preserve the game's mission/side precedence,
        // model rebasing, clip remaps and collision arithmetic. Do not duplicate
        // those algorithms in a window-specific resource construction path.
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
        self.visuals(content, metadata, map_path, vertices, indices, textures, false)
    }

    fn visuals(&self, content: Content, metadata: Value, map_path: String, vertices: Vec<u8>,
        indices: Vec<u8>, mut textures: Library, static_inspection: bool) -> Result<BattleContent, String> {
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
            let mut material = material(record.clone(), kind, flags, texture, bump);
            if self.family == crate::GameFamily::Swbf2 {
                material.resolve_extended()?;
                if let Some(name) = material.extended.as_ref().and_then(|s| s.bump.as_deref()) {
                    material.bump = image(&mut textures, &map_table, Some(name))?;
                }
            }
            world.push(Mesh {
                name: record["texture"].as_str().unwrap_or("").into(),
                geometry,
                material,
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
            // buildWorld creates another ground view from the same terrain,
            // using the map's low-resolution image across its whole extent.
            // It belongs to the far scene even though no mesh row names it.
            if kind == MaterialKind::Terrain {
                let low = &metadata["distance"]["lowRes"];
                if let Some(name) = low["texture"].as_str().filter(|name| !name.is_empty()) {
                    let texture = image(&mut textures, &map_table, Some(name))?;
                    let far = far_terrain(world.last().unwrap(), name, &low["scale"], texture);
                    world.push(far);
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
        let mut sky_flyers = None;
        let sky_file = textures.path(&format!("{map_path}/sky/flyers.json"))?;
        if sky_file.exists() {
            let sky = bfe_content::json::parse(
                &std::fs::read_to_string(sky_file).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let sky_textures = table(&format!("{map_path}/sky"), sky.get("textures"));
            sky_flyers = Some(authored_json(&sky));
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
            append_pack_for_family(self.family, &mut models, pack, "", &resolve, &mut textures)?;
        }
        for (family, pack, table) in scoped_packs {
            let scoped = |name: &str| table.get(&name.to_lowercase()).cloned();
            append_pack_for_family(
                self.family, &mut models,
                &pack,
                &format!("{family}/"),
                &scoped,
                &mut textures,
            )?;
        }
        // a class's textures come from its own side first
        // (`side.texture(name) || anyTexture(name)`).
        let (actors, skins) = load_actors_and_skins(&content, &resolve, &mut textures, true)?;
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
                // Its guns turn as the world's do: each aimer's parent bone
                // (the blaster) by the class's aimer.
                let guns = bfe_content::derived::droideka_guns(actor, def.get("weapons"));
                let blasters = guns
                    .iter()
                    .filter_map(|gun| usize::try_from(actor.bones.get(gun.bone)?.parent as i64).ok())
                    .collect();
                // Where each gun's rounds leave, in its blaster's frame: the
                // barrel tips found on its mesh, else its aimer's own place.
                let tips = guns
                    .iter()
                    .map(|gun| match &gun.tips {
                        Some(tips) => tips.iter().map(|t| [t.x, t.y, t.z]).collect(),
                        None => actor.bones.get(gun.bone).map(|b| vec![[b.local[12], b.local[13], b.local[14]]]).unwrap_or_default(),
                    })
                    .collect();
                let a = bfe_pose::rig::droideka_guns::aimer_of(def);
                units.push((
                    class.clone(),
                    crate::units::walker(
                        actor,
                        clips,
                        def.get("BallRadius")
                            .and_then(Authored::as_f64)
                            .unwrap_or(bfe_pose::rig::droideka::BALL_CENTER),
                        blasters,
                        [a.yaw_rate, a.pitch_rate, a.yaw[0], a.yaw[1], a.pitch[0], a.pitch[1]],
                        tips,
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
        // The map's placed effects (torches, smoke, steam, fountains) play
        // through the same player as everything else, under names of their
        // own so a battle effect of the same name keeps its own data.
        let mut environment_instances = Vec::new();
        let environment_basins: Vec<[f64; 4]> = environment
            .as_ref()
            .and_then(|data| data.get("basins").and_then(Authored::as_array))
            .into_iter()
            .flatten()
            .map(|b| {
                let c = b.get("center").and_then(Authored::as_array);
                let at = |i: usize| c.and_then(|c| c.get(i)).and_then(Authored::as_f64).unwrap_or(0.);
                [at(0), at(1), at(2), b.get("radius").and_then(Authored::as_f64).unwrap_or(0.)]
            })
            .collect();
        let environment_library = environment.as_ref().and_then(|data| {
            for item in data.get("instances").and_then(Authored::as_array).into_iter().flatten() {
                let name = item.get("effect").and_then(Authored::as_str);
                let matrix = item.get("matrix").and_then(Authored::as_array);
                if let (Some(name), Some(matrix)) = (name, matrix.filter(|m| m.len() == 16)) {
                    environment_instances.push(EnvironmentInstance {
                        effect: format!("{}{name}", crate::effects::ENVIRONMENT),
                        matrix: std::array::from_fn(|i| matrix[i].as_f64().unwrap_or(0.)),
                        hardpoint: item.get("hardpoint").and_then(Authored::as_str).unwrap_or("").to_string(),
                    });
                }
            }
            crate::effects::prefixed(data.get("effects"), crate::effects::ENVIRONMENT)
        });
        libraries.push(environment_library.as_ref());
        if let Some(data) = &environment {
            any.push(table(&format!("{map_path}/environment"), data.get("textures")));
        }
        let effects = vec![(
            "battle".into(),
            crate::effects::Effects::compile(&crate::effects::merge(libraries))?,
        )];
        let (buildings_bvh, terrain_bvh) = build_collision_bvhs(&content.collision)?;
        let texture_paths = collect_texture_paths(any);
        let prop_attributes = if self.family == crate::GameFamily::Swbf2 {
            content.battle.props.as_ref().and_then(|p| p.get("attributesFile"))
                .map(|file| {
                    let name = file.as_str().ok_or("invalid placed attribute filename")?;
                    textures.read(&format!("{map_path}/{name}"))
                }).transpose()?.unwrap_or_default()
        } else { Vec::new() };
        Ok(BattleContent {
            prop_attributes,
            family: self.family,
            static_inspection,
            content,
            map: metadata,
            world,
            models,
            sky_flyers,
            actors,
            skins,
            textures,
            texture_paths,
            units,
            first_person,
            effects,
            mission_effects: None,
            environment_instances,
            environment_basins,
            buildings_bvh,
            terrain_bvh,
            defs: Default::default(),
            craft_rigs: Vec::new(),
        })
    }
    /// Load display resources with the team, class and spawn policy resolved by the mission authority.
    pub fn load_scripted_mission(&self, map: &str, query: &str, mission: &str) -> Result<BattleContent, String> {
        let resolved = bfe_content::json::parse_bytes(mission.as_bytes()).map_err(|e| e.to_string())?;
        self.load_swbf2(map, query, Some(resolved))
    }

    fn load_swbf2(&self, map: &str, query: &str, mission: Option<bfe_content::Value>) -> Result<BattleContent, String> {
        if map.is_empty() || !map.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err("invalid map name".into());
        }
        let mut textures = Library::new(
            &self.root,
            self.compressed,
            self.max_texture_size,
            self.texture_capabilities,
        )?;
        let files = self.files(GameFamily::Swbf2);
        let files = bfe_content::files::PinnedFiles::new(self.pinned.as_deref(), &files);
        let source = CheckedFiles {
            files: &files,
            library: &textures,
        };
        let mut battle =
            Battle::load_for_family(&BattleLoader::new(&source), GameFamily::Swbf2, map, query)?;
        if let Some(mission) = mission {
            battle.policy = bfe_content::mission::spawn_policy(&battle.query, Some(&mission));
            if battle.policy.class_name != battle.unit {
                return Err("mission scene class differs from the authority".into());
            }
            let mut world = battle.world.as_object().cloned().ok_or("invalid mission world")?;
            world.set("mission".into(), mission);
            battle.world = bfe_content::Value::Object(world.into());
        }
        let content = Content::build(battle)?;
        let admission = &content
            .battle
            .swbf2
            .as_ref()
            .ok_or("2005 battle payload missing")?
            .map_admission;
        let map_path = format!("maps/{map}");
        let mission = admission.mission_name.as_deref().ok_or("missing effect mission id")?;
        if !mission.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')) {
            return Err("invalid effect mission id".into());
        }
        let directory = format!("{map_path}/effects/{mission}");
        let (mission_effects, library) = match crate::effects::swbf2::read(&textures, &directory) {
            Some((pack, library)) => (Some(pack), library),
            None => (None, crate::effects::swbf2::compile(&crate::effects::merge([
                content.battle.effects.get("effects"),
            ]))?),
        };
        let pack = mission_effects.as_ref().unwrap_or(&content.battle.effects);
        let environment_instances = crate::effects::swbf2::instances(pack);
        let effect_models = crate::effects::swbf2::models(&textures, &directory, pack).unwrap_or_else(|e| {
            eprintln!("Skipping mission effect meshes: {e}");
            Default::default()
        });
        let mut effect_textures = table(&directory, pack.get("textures"));
        effect_textures.retain(|name, path| match textures.load(path) {
            Ok(_) => true,
            Err(e) => {
                eprintln!("Skipping mission effect texture {name}: {e}");
                false
            }
        });
        let mut metadata = authored_json(&content.battle.world);
        if let Some(path) = pack.get("spaceDust").and_then(|d| d.get("Texture")).and_then(Authored::as_str).and_then(|name| effect_textures.get(name)) {
            match textures.load(path) {
                Ok(image) => metadata["spaceDustImage"] = image.into(),
                Err(e) => eprintln!("Skipping mission space dust image: {e}"),
            }
        }
        if pack.get("beams").and_then(Authored::as_object).is_some_and(|beams| !beams.is_empty()) {
            match textures.load(&format!("{directory}/beam.png")) {
                Ok(image) => metadata["lightBeamImage"] = image.into(),
                Err(e) => eprintln!("Skipping mission beam image: {e}"),
            }
        }
        let map_table = table(&map_path, content.battle.world.get("textures"));
        let mut any = Vec::new();
        if let Some(side) = content.battle.sides.first() {
            any.push(table(&side.dir, side.units_file.get("textures")));
        }
        any.push(table("common", content.battle.common.get("textures")));
        any.push(map_table.clone());
        for side in content.battle.sides.iter().skip(1) {
            any.push(table(&side.dir, side.units_file.get("textures")));
        }
        let resolve = |name: &str| {
            any.iter()
                .find_map(|t| t.get(&name.to_lowercase()).cloned())
        };
        let world =
            crate::map::build_world_meshes(admission, &metadata, &mut textures, &map_table)?;

        let mut models = Vec::new();
        let mut packs = content
            .weapon_packs
            .iter()
            .map(|(_, pack)| pack)
            .collect::<Vec<_>>();
        packs.extend(content.props_models.iter());
        for pack in packs {
            append_pack_swbf2(&mut models, pack, "", &resolve, &mut textures)?;
        }
        // The craft rigs (presentation only, read here and never by a battle's
        // admission) and their parts, one model a bone (the fleet draws them posed).
        let mut craft_rigs = Vec::new();
        for side in &content.battle.sides {
            let file = |name: &str| textures.read(&format!("sides/{}/{name}", side.name));
            if let (Ok(rigs), Ok(buf), Ok(index)) = (file("craftrigs.json"), file("craftrigs.bin"), file("craftrigs_index.bin")) {
                let rigs = bfe_content::json::parse_bytes(&rigs).map_err(|e| format!("sides/{}/craftrigs.json: {e}", side.name))?;
                craft_rigs.extend(bfe_content::content::craft_rigs(&content.battle, &side.name, &rigs, &buf, &index)?);
            }
        }
        let mut rigs = bfe_content::models::ModelMap::default();
        for rig in &craft_rigs {
            for (bone, parts) in rig.groups() {
                let name = rig.group_name(bone);
                if !rigs.has(&name) {
                    rigs.set(&name, std::rc::Rc::new(parts.into_iter().cloned().collect()));
                }
            }
        }
        append_pack_swbf2(&mut models, &rigs, "", &resolve, &mut textures)?;

        append_pack_swbf2(&mut models, &effect_models, "effects/", &|name| {
            effect_textures.get(&name.to_lowercase()).cloned()
        }, &mut textures)?;

        let (actors, skins) = load_actors_and_skins(&content, &resolve, &mut textures, false)?;

        let swbf2_battle = content
            .battle
            .swbf2
            .as_ref()
            .ok_or("2005 battle payload missing")?;
        let mut units: Vec<(String, bfe_presentation::unit_content::Input)> = content
            .actors
            .iter()
            .filter_map(|(name, actor)| {
                let matching_unit = content.battle.sides.iter().find_map(|side| {
                    let (sname, model) = name.split_once('/')?;
                    if side.name != sname {
                        return None;
                    }
                    side.units.as_object()?.entries().find_map(|(cls, def)| {
                        (def.get("model").and_then(Authored::as_str) == Some(model))
                            .then(|| (format!("{sname}/{cls}"), def))
                    })
                });
                let prefixes = match matching_unit {
                    Some((unit_key, def)) => {
                        let unit_banks = swbf2_battle
                            .unit_anim_banks
                            .iter()
                            .find(|(k, _)| *k == unit_key)
                            .map(|(_, list)| list.as_slice())
                            .unwrap_or(&[]);
                        crate::units::swbf2_slot_prefixes(Some(def), unit_banks)
                    }
                    None => crate::units::swbf2_slot_prefixes(None, &[]),
                };
                content
                    .body_clips
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, clips)| {
                        (
                            name.clone(),
                            crate::units::soldier_swbf2(actor, clips, None, &prefixes),
                        )
                    })
            })
            .collect();
        for (unit_key, clips) in &content.unit_clips {
            let Some((side_name, class_name)) = unit_key.split_once('/') else {
                continue;
            };
            let Some(side) = content.battle.sides.iter().find(|s| s.name == side_name) else {
                continue;
            };
            let Some(def) = side.units.get(class_name) else {
                continue;
            };
            let model = def.get("model").and_then(Authored::as_str).unwrap_or("");
            let actor_key = format!("{side_name}/{model}");
            let unit_banks = swbf2_battle
                .unit_anim_banks
                .iter()
                .find(|(k, _)| k == unit_key)
                .map(|(_, list)| list.as_slice())
                .unwrap_or(&[]);
            let prefixes = crate::units::swbf2_slot_prefixes(Some(def), unit_banks);
            if let Some((_, actor)) = content.actors.iter().find(|(n, _)| n == &actor_key) {
                units.push((
                    unit_key.clone(),
                    crate::units::soldier_swbf2(actor, clips, None, &prefixes),
                ));
            }
        }

        let effects = vec![("battle".into(), library)];

        let (buildings_bvh, terrain_bvh) = build_collision_bvhs(&content.collision)?;
        any.push(effect_textures);
        let texture_paths = collect_texture_paths(any);
        let prop_attributes = swbf2_battle.map_admission.props_attributes_buf.as_deref()
            .map(<[u8]>::to_vec).unwrap_or_default();
        Ok(BattleContent {
            prop_attributes,
            family: GameFamily::Swbf2,
            static_inspection: false,
            content,
            map: metadata,
            world,
            models,
            sky_flyers: None,
            actors,
            skins,
            textures,
            texture_paths,
            units,
            first_person: None,
            effects,
            mission_effects,
            environment_instances,
            environment_basins: Vec::new(),
            buildings_bvh,
            terrain_bvh,
            defs: Default::default(),
            craft_rigs,
        })
    }
}
type SkinOverrides = Vec<(String, Vec<Option<usize>>)>;

fn load_actors_and_skins(
    content: &Content,
    resolve: &impl Fn(&str) -> Option<String>,
    textures: &mut Library,
    warn_missing_skin: bool,
) -> Result<(Vec<ActorMesh>, SkinOverrides), String> {
    let side_tables: Vec<(String, BTreeMap<String, String>)> = content
        .battle
        .sides
        .iter()
        .map(|side| (side.name.clone(), table(&side.dir, side.units_file.get("textures"))))
        .collect();
    let side_resolve = |side: &str, name: &str| {
        side_tables
            .iter()
            .find(|(n, _)| n == side)
            .and_then(|(_, t)| t.get(&name.to_lowercase()).cloned())
            .or_else(|| resolve(name))
    };
    let mut actors = Vec::new();
    for (name, body) in &content.battle.bodies {
        let side = name.split_once('/').map_or("", |(side, _)| side);
        actors.push(actor_mesh(
            name,
            body,
            &|t: &str| side_resolve(side, t),
            textures,
        )?);
    }
    let mut skins = Vec::new();
    for side in &content.battle.sides {
        for (class, def) in side.units.as_object().into_iter().flat_map(|u| u.entries()) {
            let wanted = ["OverrideTexture", "OverrideTexture2"].map(|key| {
                def.get(key)
                    .and_then(Authored::as_str)
                    .filter(|s| !s.is_empty())
            });
            if wanted.iter().all(Option::is_none) {
                continue;
            }
            let model = def.get("model").and_then(Authored::as_str).unwrap_or("");
            let key = format!("{}/{model}", side.name);
            let (Some((_, body)), Some(mesh)) = (
                content.battle.bodies.iter().find(|(n, _)| *n == key),
                actors.iter().find(|a| a.name == key),
            ) else {
                continue;
            };
            let mut skin = mesh.textures.clone();
            for (slot, name) in override_slots(&body.def, wanted) {
                match side_resolve(&side.name, name) {
                    Some(path) => skin[slot] = Some(textures.load(&path)?),
                    None if warn_missing_skin => eprintln!(
                        "native unit admission: class {}/{class} skin {name} has no texture; it keeps its body's",
                        side.name
                    ),
                    None => {}
                }
            }
            if skin != mesh.textures {
                skins.push((format!("{}/{class}", side.name), skin));
            }
        }
    }
    Ok((actors, skins))
}
pub(crate) fn build_collision_bvhs(
    collision: &bfe_content::collision::MapCollision,
) -> Result<(BvhTree, Option<BvhTree>), String> {
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
    Ok((buildings_bvh, terrain_bvh))
}
fn collect_texture_paths(tables: Vec<BTreeMap<String, String>>) -> BTreeMap<String, String> {
    let mut texture_paths = BTreeMap::new();
    for table in tables {
        for (name, path) in table {
            texture_paths.entry(name).or_insert(path);
        }
    }
    texture_paths
}
pub(crate) fn material(
    record: Value,
    kind: MaterialKind,
    flags: u32,
    texture: Option<usize>,
    bump: Option<usize>,
) -> Material {
    Material {
        kind,
        extended: None,
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
/// 2005 material builder: reads explicit `material.{blendMode, alphaCutout,
/// doubleSided, textureMode, sourceTexture}` authoritatively when present
/// (falling back to legacy flag promotion only when explicit blend/cutout
/// fields are absent) and promotes top-level fields binders inspect.
pub fn material_swbf2(
    mut record: Value,
    kind: MaterialKind,
    mut flags: u32,
    texture: Option<usize>,
    bump: Option<usize>,
) -> Material {
    let mut double_sided_legacy = record.get("doubleSided").is_some_and(truth);
    let mut explicit_blend: Option<Blend> = None;
    let mut explicit_cutout: Option<bool> = None;
    let mut explicit_double_sided: Option<bool> = None;
    if let Some(mat) = record.get("material").and_then(Value::as_object).cloned() {
        if let Some(v) = mat.get("doubleSided") {
            let ds = truth(v);
            explicit_double_sided = Some(ds);
            if ds {
                double_sided_legacy = true;
            }
        }
        if let Some(v) = mat.get("alphaCutout") {
            explicit_cutout = Some(match v {
                Value::Null => false,
                Value::Bool(b) => *b,
                Value::Number(n) => n.as_f64().is_some_and(|x| x.is_finite() && x > 0.0),
                _ => false,
            });
        }
        if let Some(b) = mat.get("blendMode").and_then(Value::as_str) {
            explicit_blend = match b.to_ascii_lowercase().as_str() {
                "opaque" => Some(Blend::Opaque),
                "alpha" => Some(Blend::Alpha),
                "additive" => Some(Blend::Additive),
                _ => None,
            };
        }
        if let Some(obj) = record.as_object_mut() {
            for key in [
                "scroll",
                "diffuse",
                "specular",
                "glow",
                "attachedLight",
                "missingTexture",
                "missingBump",
                "swbf2Flags",
                "doubleSided",
                "blendMode",
                "alphaCutout",
                "textureMode",
                "sourceTexture",
            ] {
                if !obj.contains_key(key) {
                    if let Some(val) = mat.get(key) {
                        obj.insert(key.into(), val.clone());
                    }
                }
            }
        }
    }
    if explicit_blend.is_none()
        && explicit_cutout.is_none()
        && double_sided_legacy
        && flags & (2 | 0x80) == 0
    {
        flags |= 4;
    }
    let cutout = explicit_cutout.unwrap_or(flags & 2 != 0);
    let blend = explicit_blend.unwrap_or(if flags & 0x80 != 0 {
        Blend::Additive
    } else if flags & 4 != 0 && !cutout {
        Blend::Alpha
    } else {
        Blend::Opaque
    });
    // When 2005 prepared data provides an explicit `material.doubleSided`
    // boolean, treat it as authoritative rather than OR-ing legacy flags.
    let double_sided = explicit_double_sided.unwrap_or(flags & 0x86 != 0);
    Material {
        kind,
        extended: None,
        blend,
        cutout,
        double_sided,
        far: truth(&record["far"]),
        flags,
        texture,
        bump,
        record,
    }
}
impl Material {
    fn resolve_extended(&mut self) -> Result<(), String> {
        let surface = extended_surface(&self.record, self.flags)?;
        self.apply_extended(surface);
        Ok(())
    }
    /// The resolved 2005 surface decides flags, blending, cutout and culling.
    pub(crate) fn apply_extended(&mut self, surface: ExtendedSurface) {
        self.flags = surface.flags;
        self.blend = match surface.blend_mode {
            ExtendedBlend::Opaque => Blend::Opaque,
            ExtendedBlend::Alpha => Blend::Alpha,
            ExtendedBlend::Additive => Blend::Additive,
        };
        self.cutout = surface.cutout;
        self.double_sided = surface.double_sided;
        self.extended = Some(surface);
    }
}

/// Resolve a 2005 surface from a material record as it was prepared.
/// Geometry counts in model packs use the content reader's f64 numbers.
/// Read only the material's original fields before interpreting any flag.
pub(crate) fn extended_surface(record: &Value, flags: u32) -> Result<ExtendedSurface, String> {
    let mut fields = serde_json::Map::new();
    fields.insert("flags".into(), flags.into());
    for key in ["material", "terrain", "glow", "bump", "scroll", "specular"] {
        if let Some(v) = record.get(key) { fields.insert(key.into(), v.clone()); }
    }
    let descriptor: ExtendedRecord = serde_json::from_value(Value::Object(fields)).map_err(|e| e.to_string())?;
    ExtendedSurface::resolve(&descriptor)
}

/// The 2005 world, prop and effect packs keep the field promotions of
/// [`material_swbf2`] and take their surface from the record as prepared.
pub(crate) fn material_swbf2_extended(
    record: Value,
    kind: MaterialKind,
    flags: u32,
    texture: Option<usize>,
    bump: Option<usize>,
) -> Result<Material, String> {
    let surface = extended_surface(&record, flags)?;
    let mut material = material_swbf2(record, kind, flags, texture, bump);
    material.apply_extended(surface);
    Ok(material)
}

pub(crate) fn truth(value: &Value) -> bool {
    !matches!(value, Value::Null | Value::Bool(false)) && value.as_f64() != Some(0.0)
}
pub(crate) fn table(directory: &str, value: Option<&Authored>) -> BTreeMap<String, String> {
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
pub(crate) fn image(
    library: &mut Library,
    table: &BTreeMap<String, String>,
    name: Option<&str>,
) -> Result<Option<usize>, String> {
    name.and_then(|n| table.get(&n.to_lowercase()))
        .map(|p| library.load(p))
        .transpose()
}
pub(crate) fn validate_triangles(positions: &[f32], indices: &[u32]) -> Result<(), String> {
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
pub(crate) struct CheckedFiles<'a> {
    pub(crate) files: &'a dyn Files,
    pub(crate) library: &'a Library,
}
impl Files for CheckedFiles<'_> {
    fn read(&self, path: &str) -> Option<std::rc::Rc<[u8]>> {
        self.library.path(path).ok()?;
        self.files.read(path)
    }
}

/// textureOverrides: OverrideTexture takes the place of the texture
/// most of the body's vertices use, OverrideTexture2 of the next. Gives the
/// body material slot (textures in first-use order, as actor_mesh numbers
/// them) and the override's name for each override the class has.
fn override_slots<'a>(def: &Authored, wanted: [Option<&'a str>; 2]) -> Vec<(usize, &'a str)> {
    let mut names: Vec<(String, f64)> = Vec::new();
    for part in def.get("parts").and_then(Authored::as_array).into_iter().flatten() {
        let texture = part.get("texture").and_then(Authored::as_str).unwrap_or("");
        let count = part.get("vertexCount").and_then(Authored::as_f64).unwrap_or(0.0);
        match names.iter_mut().find(|(n, _)| n == texture) {
            Some((_, n)) => *n += count,
            None => names.push((texture.to_string(), count)),
        }
    }
    let mut by_use: Vec<usize> = (0..names.len()).collect();
    by_use.sort_by(|a, b| names[*b].1.total_cmp(&names[*a].1));
    wanted
        .into_iter()
        .zip(by_use)
        .filter_map(|(name, slot)| name.map(|name| (slot, name)))
        .collect()
}

/// keeps one vertex stream for all material groups, with four skin
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

/// 2005 variant of [`append_pack`]: resolves optional prop bump maps and builds
/// materials via [`material_swbf2`], leaving the 2004 [`append_pack`] untouched.
pub(crate) fn append_pack_swbf2(
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
            let bump_name = source
                .field("bump")
                .and_then(Authored::as_str)
                .or_else(|| {
                    source
                        .field("material")
                        .and_then(|m| m.get("bump"))
                        .and_then(Authored::as_str)
                });
            let bump = bump_name
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
                material: material_swbf2_extended(
                    record,
                    if flags & 1 != 0 {
                        MaterialKind::Dynamic
                    } else {
                        MaterialKind::Unlit
                    },
                    flags,
                    texture,
                    bump,
                )?,
            });
        }
    }
    Ok(())
}

/// A 2005 model part retains its original nested descriptor even when its
/// geometry came through the shared held, prop, or first-person model pack.
pub fn append_pack_for_family(
    family: crate::GameFamily, out: &mut Vec<Mesh>, pack: &bfe_content::models::ModelMap,
    prefix: &str, resolve: &impl Fn(&str) -> Option<String>, textures: &mut Library,
) -> Result<(), String> {
    let first = out.len();
    append_pack(out, pack, prefix, resolve, textures)?;
    if family == crate::GameFamily::Swbf2 {
        for mesh in &mut out[first..] { mesh.material.resolve_extended()?; }
    }
    Ok(())
}

// The far view keeps authored geometry and baked colour but replaces the
// near layer maps; retaining them would overwrite the low-resolution sampler.
pub(crate) fn far_terrain(near: &Mesh, name: &str, scale: &Value, texture: Option<usize>) -> Mesh {
    let mut record = near.material.record.clone();
    record["far"] = Value::Bool(true);
    record["texture"] = Value::String(name.into());
    record["lowResScale"] = scale.clone();
    if let Some(fields) = record.as_object_mut() {
        for key in ["layers", "detail", "weights"] {
            fields.remove(key);
        }
    }
    let mut material = material(record, MaterialKind::Terrain, near.material.flags, texture, None);
    material.extended = near.material.extended.clone();
    Mesh { name: "far-terrain".into(), geometry: near.geometry.clone(), material }
}

#[cfg(test)]
mod far_terrain_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn far_ground_keeps_the_height_field_and_uses_its_own_map() {
        let record = json!({
            "terrain": true,
            "texture": "near",
            "layers": ["grass", "soil"],
            "detail": "grain",
            "weights": "weights.bin",
            "vertexCount": 3,
            "stride": 3,
            "attributes": ["position"],
            "vertexOffset": 0,
            "indexOffset": 0,
            "indexCount": 3
        });
        let vertices: Vec<u8> = [0f32, 0., 0., 4., 1., 0., 0., 2., 4.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let indices: Vec<u8> = [0u32, 1, 2]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let near = Mesh {
            name: "ground".into(),
            geometry: Geometry::world(&record, &vertices, &indices).unwrap(),
            material: material(record, MaterialKind::Terrain, 1, Some(3), None),
        };
        let far = far_terrain(&near, "overview", &json!(2), Some(7));
        assert!(far.material.far);
        assert_eq!(far.material.kind, MaterialKind::Terrain);
        assert_eq!(far.material.texture, Some(7));
        assert_eq!(far.material.record["lowResScale"], 2);
        assert!(far.material.record.get("layers").is_none());
        assert!(far.material.record.get("detail").is_none());
        assert_eq!(far.geometry.buffers, near.geometry.buffers);
        assert_eq!(far.geometry.indices, near.geometry.indices);
        assert!(!near.material.far);
    }
}
