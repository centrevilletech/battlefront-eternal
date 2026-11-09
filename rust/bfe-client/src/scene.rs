//! Native scene admission for inspecting prepared maps before a live World
//! exists. Asset work ends before the first frame; the free camera publishes
//! generated commands through Host and the renderer borrows one fixed frame.
use crate::{
    canvas::Canvas,
    generated_input::*,
    host::World,
    input::ViewActions,
    render_bridge::Bridge,
    resources::{Ranges, Resources},
};
use bfe_content_native::{
    loader::{Blend, MaterialKind},
    Loader,
};
use bfe_render::{shader, static_layout as L};
use glow::HasContext;
use std::path::Path;
use three::{CoordinateSystem, Matrix4, Vector3};

// Visibility is independent of execution order. Component probes retain
// the same semantic role bank as the normal interactive scene.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Component {
    All,
    Props,
    Foliage,
    Water,
    Sky,
}
#[derive(Clone, Copy)]
pub struct View {
    pub values: [f64; shader::FRAME_FLOATS],
}
pub struct MapCamera {
    pub position: [f64; 3],
    pub first_person: bool,
    yaw: f64,
    pitch: f64,
    time: f64,
    view: View,
    slots: [usize; 3],
}
impl MapCamera {
    pub fn new(position: [f64; 3], size: [i32; 2]) -> Result<Self, String> {
        if position.iter().any(|v| !v.is_finite()) || size.iter().any(|n| !(1..=8192).contains(n)) {
            return Err("invalid native camera".into());
        }
        let mut view = View {
            values: [0.0; shader::FRAME_FLOATS],
        };
        let mut projection = Matrix4::default();
        let top = 0.3 * (60.0f64.to_radians() * 0.5).tan();
        let right = top * size[0] as f64 / size[1] as f64;
        projection.make_perspective(
            -right,
            right,
            top,
            -top,
            0.3,
            20000.0,
            CoordinateSystem::WebGL,
        );
        set(&mut view, "projectionMatrix", &projection.elements);
        // The inspector has an explicit neutral environment. Recorded battles
        // retain their authored lighting/post chain through the session host.
        for (name, values) in [
            ("sunDirection", &[0.4, 0.8, 0.4][..]),
            ("sunColor", &[0.6, 0.6, 0.6]),
            ("ambient", &[0.5, 0.5, 0.5]),
            ("ambientTop", &[0.5, 0.5, 0.5]),
            ("ambientBottom", &[0.3, 0.3, 0.3]),
            ("characterAmbient", &[0.5, 0.5, 0.5]),
            ("aaSamples", &[1.0]),
            ("farAaSamples", &[1.0]),
            ("bump", &[1.0]),
            ("terrainShade", &[1.0]),
            ("resolution", &[size[0] as f64, size[1] as f64]),
            ("fogRange", &[19999.0, 20000.0]),
            ("fade", &[0.0, 20000.0]),
        ] {
            set(&mut view, name, values);
        }
        let slots = ["viewMatrix", "cameraPosition", "time"].map(|name| {
            shader::FRAME_OFFSETS[shader::FRAME_MEMBERS
                .iter()
                .position(|(n, _)| *n == name)
                .expect("frame catalog member")]
        });
        Ok(Self {
            position,
            first_person: false,
            yaw: 0.0,
            pitch: -0.4,
            time: 0.0,
            view,
            slots,
        })
    }
    pub fn aim(&self) -> [f64; 2] {
        [self.yaw - std::f64::consts::PI, self.pitch]
    }
    pub fn snapshot(&self)->&View{&self.view}
    pub fn aspect(&self)->f64{let projection=&self.view.values[..16];projection[5]/projection[0]}
    /// A camera phase has already selected and obstructed this view. Publish
    /// its exact matrices instead of deriving them again from yaw and pitch.
    pub fn accept_camera(&mut self,output:&[f64]){use bfe_presentation::layout::camera_output as o;self.position.copy_from_slice(&output[o::POSITION..o::POSITION+3]);self.view.values[..16].copy_from_slice(&output[o::PROJECTION..o::PROJECTION+16]);self.view.values[16..32].copy_from_slice(&output[o::INVERSE..o::INVERSE+16]);let at=self.slots[1];self.view.values[at..at+3].copy_from_slice(&self.position);}
    pub fn follow(&mut self,position:[f64;3],yaw:f64,pitch:f64,dt:f64){
        self.position=position;self.yaw=yaw+std::f64::consts::PI;self.pitch=pitch;self.time+=dt;
    }

}
impl World for MapCamera {
    type Frame = View;
    fn step(
        &mut self,
        c: &ClientCommand,
        look: &ClientLook,
        _: &ClientFlightCommand,
        _: &ViewActions,
        dt: f64,
    ) -> Result<(), String> {
        // Inspection never invents a soldier, generation or weapon authority.
        // Relative pixels use the same device mapping as a future live World.
        let safe = |v: f64| {
            if v.is_finite() {
                v.clamp(-1e6, 1e6)
            } else {
                0.0
            }
        };
        self.yaw = (self.yaw - safe(look.x) * 0.002).rem_euclid(std::f64::consts::TAU);
        self.pitch = (self.pitch - safe(look.y) * 0.002).clamp(-1.55, 1.55);
        let held = |key: ClientHeld| f64::from(u8::from(c.held & (1 << (key as u8)) != 0));
        let forward = held(ClientHeld::Forward) - held(ClientHeld::Backward);
        let right = held(ClientHeld::Right) - held(ClientHeld::Left);
        let norm = (forward * forward + right * right).sqrt().max(1.0);
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.134)
        } else {
            0.0
        };
        let speed = 24.0 * dt / norm;
        self.position[0] += (-self.yaw.sin() * forward + self.yaw.cos() * right) * speed;
        self.position[1] += self.pitch.sin() * forward * speed + held(ClientHeld::Space) * speed;
        self.position[2] += (-self.yaw.cos() * forward - self.yaw.sin() * right) * speed;
        self.time += dt;
        Ok(())
    }
    fn present(&mut self, _: f64) -> &View {
        let eye = Vector3::new(self.position[0], self.position[1], self.position[2]);
        let target = Vector3::new(
            eye.x - self.yaw.sin() * self.pitch.cos(),
            eye.y + self.pitch.sin(),
            eye.z - self.yaw.cos() * self.pitch.cos(),
        );
        let mut matrix = Matrix4::default();
        matrix.look_at(&eye, &target, &Vector3::new(0.0, 1.0, 0.0));
        matrix.set_position_v3(&eye).invert();
        let [view, eye, time] = self.slots;
        self.view.values[view..view + 16].copy_from_slice(&matrix.elements);
        self.view.values[eye..eye + 3].copy_from_slice(&self.position);
        self.view.values[time] = self.time;
        &self.view
    }
}
// Names and offsets belong to the renderer's frame catalog. No client copy of
// a shader or input schema is maintained alongside this admission boundary.
fn set(view: &mut View, name: &str, values: &[f64]) {
    if let Some(i) = shader::FRAME_MEMBERS.iter().position(|(n, _)| *n == name) {
        let at = shader::FRAME_OFFSETS[i];
        view.values[at..at + values.len()].copy_from_slice(values);
    }
}

/// A sealed, exclusively owned graph of decoded CPU assets. Its constructors
/// accept filesystem paths, never an existing Rc or a renderer resource.
/// No reference into this graph escapes before open_prepared consumes it.
pub(crate) struct Prepared {
    spawn_objectives: [Vec<String>; 2],
    spawn_vehicles: std::collections::HashMap<String, [Vec<String>; 2]>,
    menu_grid: Option<u32>,
    menu_models: Vec<crate::menu_models::Model>,
    spawn_shots: Vec<bfe_battle_ui::spawn::CameraShot>,
    content: bfe_content_native::BattleContent,
    layer_ids: Vec<Vec<usize>>,
    details: Vec<Option<usize>>,
    foliage: Vec<bfe_content_native::foliage::Class>,
    foliage_images: Vec<usize>,
    placed: bfe_content_native::placed::Placements,
    water_main: Option<usize>,
    water_mask: Option<usize>,
    water_foam: Option<usize>,
    features: [u32; L::FEATURES_LENGTH],
    prepared_views: Option<crate::online::first_person::Prepared>,
    prepared_particles: Option<crate::online::particles::Prepared>,
    prepared_hud: Option<crate::online::hud::Prepared>,
    prepared_sound: Option<crate::online::sound_data::Prepared>,
    used: Vec<bool>,
    live: bool,
}
// SAFETY: Rc aliases exist only inside this sealed asset graph. Loader and
// preparation temporaries are dropped before construction returns. There are
// no GL objects, borrowed data, thread-local caches or callbacks in this type.
// Moving its sole owner between threads cannot race a reference count; once
// consumed by open_prepared, its ordinary Rc-bearing outputs are !Send again.
unsafe impl Send for Prepared {}

pub struct Scene {
    pub spawn_objectives: [Vec<String>;2],
    pub spawn_vehicles: std::collections::HashMap<String,[Vec<String>;2]>,
    pub menu_grid: Option<u32>,
    pub menu_models: Vec<crate::menu_models::Model>,
    pub spawn_shots: Vec<bfe_battle_ui::spawn::CameraShot>,
    pub bridge: Box<Bridge>,
    pub resources: Resources,
    far_target: u32,
    shadows: Option<crate::shadows::Shadows>,
    image_step: [f64; bfe_render::post::STEP],
    image_state: [f64; bfe_render::post::STATE],
    pub canvas: Canvas,
    pub admitted: usize,
    pub deferred: usize,
    pub draws: u64,
    pub size: [i32; 2],
    static_content: crate::static_scene::Admission,
    far_projection: [f64; 16],
    far_world_slots: Vec<usize>,
    near_world_ids: Vec<u32>,
    pub online: Option<Box<crate::online::replicas::Replicas>>,
}
impl Scene {
    pub fn load(
        root: &Path,
        map: &str,
        size: [i32; 2],
        load: impl FnMut() -> glow::Context,
    ) -> Result<(Self, MapCamera), String> {
        Self::load_with_random(root, map, size, None, load)
    }
    /// Recorded random draws are admission facts, shared with a browser view.
    /// Ordinary inspection uses one deterministic host stream instead.
    pub fn load_with_random(
        root: &Path,
        map: &str,
        size: [i32; 2],
        random_values: Option<&[f64]>,
        load: impl FnMut() -> glow::Context,
    ) -> Result<(Self, MapCamera), String> {
        if random_values.is_some_and(|v| {
            v.len() > 4_194_304 || v.iter().any(|x| !x.is_finite() || !(0.0..1.0).contains(x))
        }) {
            return Err("invalid foliage random admission stream".into());
        }
        let mut loader = Loader::new(root);
        loader.compressed = false;
        let content = loader.load(map, &format!("map={map}&seed=1"))?;
        Self::admit(content,map,size,random_values,false,load)
    }
    /// Online joins admit the advertised mission from local files. The server
    /// cannot supply asset paths or executable class definitions at runtime.
    pub fn load_online(
        root: &Path,
        query: &str,
        size: [i32; 2],
        load: impl FnMut() -> glow::Context,
    ) -> Result<(Self, MapCamera), String> {
        let prepared = Self::prepare_online(root, query)?;
        crate::loading_work::finish(Self::open_prepared(prepared, size, load))
    }
    pub(crate) fn prepare_online(root: &Path, query: &str) -> Result<Prepared, String> {
        let selection = bfe_content::mission::Query::parse(query);
        let map = if selection.get("map").is_empty() {
            "nab2"
        } else {
            selection.get("map")
        };
        let mut loader = Loader::new(root);
        loader.compressed = false;
        Self::prepare_scene(loader.load(map, query)?, map, None, true)
    }
    fn admit(
        content: bfe_content_native::BattleContent,
        map: &str,
        size: [i32; 2],
        random_values: Option<&[f64]>,
        live: bool,
        load: impl FnMut() -> glow::Context,
    ) -> Result<(Self, MapCamera), String> {
        let prepared = Self::prepare_scene(content, map, random_values, live)?;
        crate::loading_work::finish(Self::open_prepared(prepared, size, load))
    }
    fn prepare_scene(
        mut content: bfe_content_native::BattleContent,
        map: &str,
        random_values: Option<&[f64]>,
        live: bool,
    ) -> Result<Prepared, String> {
        let spawn_screen = content.content.battle.world.get("spawnScreen");
        let strings = |value: Option<&bfe_content::Value>| {
            value
                .and_then(bfe_content::Value::as_array)
                .map_or_else(Vec::new, |rows| {
                    rows.iter()
                        .filter_map(bfe_content::Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
        };
        let spawn_objectives = std::array::from_fn(|i| {
            strings(
                spawn_screen
                    .and_then(|s| s.get("teams"))
                    .and_then(|t| t.get(&(i + 1).to_string()))
                    .and_then(|t| t.get("objectives")),
            )
        });
        let spawn_vehicles = spawn_screen
            .and_then(|s| s.get("postVehicles"))
            .and_then(bfe_content::Value::as_object)
            .map_or_else(std::collections::HashMap::new, |posts| {
                posts
                    .entries()
                    .map(|(name, teams)| {
                        (
                            name.to_string(),
                            std::array::from_fn(|i| strings(teams.get(&(i + 1).to_string()))),
                        )
                    })
                    .collect()
            });
        let menu_grid = if live {
            content
                .texture_paths
                .get("shell_cube")
                .map(|p| content.textures.load(p))
                .transpose()?
                .map(|id| id as u32)
        } else {
            None
        };
        let menu_models = if live {
            crate::menu_models::prepare(&mut content)?
        } else {
            Vec::new()
        };
        let spawn_shots = content
            .content
            .battle
            .world
            .get("spawnScreen")
            .and_then(|s| s.get("cameraShots"))
            .and_then(bfe_content::Value::as_array)
            .map_or_else(Vec::new, |shots| {
                shots
                    .iter()
                    .filter_map(|s| {
                        let p = s.get("position")?.as_array()?;
                        let q = s.get("quaternion")?.as_array()?;
                        if p.len() != 3 || q.len() != 4 {
                            return None;
                        }
                        Some(bfe_battle_ui::spawn::CameraShot {
                            position: [p[0].as_f64()?, p[1].as_f64()?, p[2].as_f64()?],
                            quaternion: [
                                q[0].as_f64()?,
                                q[1].as_f64()?,
                                q[2].as_f64()?,
                                q[3].as_f64()?,
                            ],
                        })
                    })
                    .collect()
            });
        // Resolve terrain sampler identities through the authored map table,
        // before GPU admission freezes the texture bank and frame storage.
        let mut layers = std::collections::BTreeMap::new();
        if let Some(table) = content
            .content
            .battle
            .world
            .get("textures")
            .and_then(|v| v.as_object())
        {
            for (name, path) in table.entries() {
                if let Some(path) = path.as_str() {
                    layers.insert(name.to_lowercase(), format!("maps/{map}/{path}"));
                }
            }
        }
        let mut details = Vec::with_capacity(content.world.len());
        let mut layer_ids = Vec::with_capacity(content.world.len());
        for mesh in &content.world {
            let mut ids = Vec::new();
            if let Some(names) = mesh.material.record["layers"].as_array() {
                for name in names {
                    let name = name.as_str().ok_or("invalid terrain sampler name")?;
                    let path = layers
                        .get(&name.to_lowercase())
                        .ok_or("missing terrain sampler")?;
                    ids.push(content.textures.load(path)?);
                }
            }
            layer_ids.push(ids);
            details.push(
                if let Some(name) = mesh.material.record["detail"].as_str() {
                    let path = layers
                        .get(&name.to_lowercase())
                        .ok_or("missing terrain detail sampler")?;
                    Some(content.textures.load(path)?)
                } else {
                    None
                },
            );
        }
        let mut sun = Vector3::new(0., -1., 0.);
        if let Ok(v) =
            serde_json::from_value::<[f64; 3]>(content.map["lighting"]["sunDirection"].clone())
        {
            sun.set(v[0], v[1], v[2]).normalize();
        }
        let order = content
            .content
            .battle
            .world
            .get("foliage")
            .and_then(|v| v.get("classes"))
            .and_then(|v| v.as_object())
            .map(|o| o.entries().map(|(n, _)| n.to_owned()).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut seeded = bfe_content_native::foliage::Seeded::new(1);
        let mut random_at = 0;
        let mut random = || {
            if let Some(values) = random_values {
                let value = values.get(random_at).copied().unwrap_or(f64::NAN);
                random_at += 1;
                value
            } else {
                seeded.next()
            }
        };
        let foliage =
            bfe_content_native::foliage::admit(&content.map["foliage"], &order, sun, &mut random)?;
        let mut foliage_images = Vec::new();
        for class in &foliage {
            let path = layers
                .get(&class.texture.to_lowercase())
                .ok_or_else(|| format!("missing foliage texture {}", class.texture))?;
            foliage_images.push(content.textures.load(path)?);
        }
        let placed = if live{bfe_content_native::placed::admit_live(&content)?}else{bfe_content_native::placed::admit(&content)?};
        let mut water_image = |name: &str| -> Result<Option<usize>, String> {
            layers
                .get(&name.to_lowercase())
                .map(|path| content.textures.load(path))
                .transpose()
        };
        let water_main = if let Some(name) = content.map["water"]["look"]["MainTexture"][0].as_str()
        {
            water_image(name)?
        } else {
            None
        };
        let water_mask =
            if let Some(name) = content.map["water"]["look"]["SpecularMaskTextures"][0].as_str() {
                water_image(&format!("{name}0"))?
            } else {
                None
            };
        let water_foam = if let Some(name) = content.map["water"]["look"]["FoamTexture"][0].as_str()
        {
            water_image(name)?
        } else {
            None
        };
        // game.js constructs eight ordnance light shells and each visible post hologram.
        // Preserve that cold catalog identity; this neutral inspection frame
        // publishes zero active lights until a native World supplies them.
        const ORDNANCE_LIGHTS: usize = 8;
        let world = &content.content.battle.world;
        let posts = if world.get("hologram").is_some_and(|v| v.truthy()) {
            world
                .get("commandPosts")
                .and_then(|v| v.as_array())
                .map_or(0, |posts| {
                    posts
                        .iter()
                        .filter(|post| post.get("holoOdf").is_none_or(|v| v.truthy()))
                        .count()
                })
        } else {
            0
        };
        let mut features = [0; L::FEATURES_LENGTH];
        features[L::FEATURES_SHADOWS] = if live { crate::shadows::COUNT as u32 } else { 0 };
        features[L::FEATURES_CSM] = live as u32;
        features[L::FEATURES_POINTS] = (ORDNANCE_LIGHTS + posts) as u32;
        let prepared_views = if live {
            Some(crate::online::first_person::Prepared::new(&mut content)?)
        } else {
            None
        };
        let prepared_particles = if live {
            Some(crate::online::particles::Prepared::new(&mut content)?)
        } else {
            None
        };
        let prepared_hud = if live {
            Some(crate::online::hud::Prepared::new(&mut content)?)
        } else {
            None
        };
        let prepared_sound = if live {
            Some(crate::online::sound_data::Prepared::new(&content)?)
        } else {
            None
        };
        let mut used = vec![false; content.textures.images.len()];
        for (i, mesh) in content
            .world
            .iter()
            .enumerate()
            .filter(|(_, m)| inspectable(m) || m.material.kind == MaterialKind::Sky)
        {
            for id in mesh
                .material
                .texture
                .into_iter()
                .chain(mesh.material.bump)
                .chain(details[i])
                .chain(layer_ids[i].iter().copied())
            {
                *used.get_mut(id).ok_or("invalid map image id")? = true;
            }
        }
        for id in foliage_images
            .iter()
            .copied()
            .chain(water_main)
            .chain(water_mask)
            .chain(water_foam)
        {
            used[id] = true;
        }
        for part in &placed.parts {
            let mesh = content
                .models
                .iter()
                .find(|m| m.name == format!("{}/{}", part.model, part.source))
                .ok_or("missing placed model resource")?;
            if let Some(id) = mesh.material.texture {
                used[id] = true;
            }
        }
        if live {
            used.fill(true);
        }
        Ok(Prepared {
            spawn_objectives,
            spawn_vehicles,
            menu_grid,
            menu_models,
            spawn_shots,
            content,
            layer_ids,
            details,
            foliage,
            foliage_images,
            placed,
            water_main,
            water_mask,
            water_foam,
            features,
            prepared_views,
            prepared_particles,
            prepared_hud,
            prepared_sound,
            used,
            live,
        })
    }
    pub(crate) async fn open_prepared(
        prepared: Prepared,
        size: [i32; 2],
        mut load: impl FnMut() -> glow::Context,
    ) -> Result<(Self, MapCamera), String> {
        let Prepared {
            spawn_objectives,
            spawn_vehicles,
            menu_grid,
            mut menu_models,
            spawn_shots,
            mut content,
            layer_ids,
            details,
            foliage,
            foliage_images,
            placed,
            water_main,
            water_mask,
            water_foam,
            features,
            prepared_views,
            prepared_particles,
            prepared_hud,
            prepared_sound,
            used,
            live,
        } = prepared;
        crate::loading_work::frame().await;
        let canvas = Canvas::new(load(), size)?;
        let mut resources = Resources::new(load());
        resources.canvas = Some(canvas.framebuffer);
        let mut bridge = Bridge::new_boxed(load(), Ranges::new(load()));
        bridge.r.core.canvas_framebuffer = Some(canvas.framebuffer);
        let sampling = r#"{"wrapS":1000,"wrapT":1000,"wrapR":1000,"min":1008,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":true,"compare":null}"#;
        let mut textures = vec![u32::MAX; content.textures.images.len()];
        for (i, image) in content.textures.images.iter().enumerate() {
            // Battle admission has already decoded side resources. Inspection
            // uploads only the images used by the admitted world descriptors.
            if !used[i] {
                continue;
            }
            if image.format != bfe_render::texture_format::Format::Rgba {
                return Err("inspection expects decoded RGBA assets".into());
            }
            let source=resources.source(&serde_json::json!({"width":image.width,"height":image.height,"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1009,"srgb":false}).to_string())?;
            resources.source_pixels(source, &image.data)?;
            let sample = resources.sample(source, sampling)?;
            resources.upload(sample)?;
            textures[i] = bridge.r.core.add_texture(resources.texture(sample)?);
            if i % 8 == 7 {
                crate::loading_work::frame().await;
                bridge.r.core.state.reset();
            }
        }
        let menu_grid=menu_grid.map(|id|textures[id as usize]);
        for model in &mut menu_models {model.texture=textures[model.texture as usize];}
        let empty = resources.world_fallback(false)?;
        let empty = bridge.r.core.add_texture(resources.texture(empty)?);
        let neutral = resources.world_fallback(true)?;
        let neutral = bridge.r.core.add_texture(resources.texture(neutral)?);
        // buildWorld attaches canopy footprints before native source admission.
        // Zero state rows retain the authored billboard LOD in this inspector;
        // the existing tree descriptor still owns grouping and mask geometry.
        let canopy = &content.map["treeCanopies"];
        let canopy_texture =
            if let Some(instances) = canopy["instances"].as_array().filter(|v| !v.is_empty()) {
                if instances.len() > 65536 {
                    return Err("native canopy state capacity".into());
                }
                let state = resources
                    .canopy_state(instances.len() as u32, &vec![0; instances.len() * 16])?;
                Some(bridge.r.core.add_texture(resources.texture(state)?))
            } else {
                None
            };
        let mut ocean_mask = empty;
        if content.map["water"]["look"]["OceanEnable"][0]
            .as_f64()
            .unwrap_or(0.)
            > 0.
        {
            let data = &content.map["water"];
            let grid = data["grid"]
                .as_u64()
                .filter(|n| (1..=4096).contains(n))
                .ok_or("invalid ocean mask grid")? as usize;
            let mut pixels = vec![0u8; grid * grid];
            for cell in data["cells"].as_array().ok_or("missing ocean mask cells")? {
                let x = cell[0]
                    .as_u64()
                    .filter(|v| *v < grid as u64)
                    .ok_or("invalid ocean mask column")? as usize;
                let z = cell[1]
                    .as_u64()
                    .filter(|v| *v < grid as u64)
                    .ok_or("invalid ocean mask row")? as usize;
                pixels[z * grid + x] = 255;
            }
            let source=resources.source(&serde_json::json!({"width":grid,"height":grid,"depth":1,"target":glow::TEXTURE_2D,"format":1028,"dataType":1009,"srgb":false}).to_string())?;
            resources.source_pixels(source, &pixels)?;
            let sample=resources.sample(source,r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
            resources.upload(sample)?;
            ocean_mask = bridge.r.core.add_texture(resources.texture(sample)?);
        }
        let mut static_content =
            crate::static_scene::props(&mut bridge, &content, &textures, placed, if live { crate::shadows::COUNT } else { 0 })?;
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        for (class, image) in foliage.iter().zip(foliage_images) {
            bridge.foliage_class(
                textures[image],
                class.grass,
                live,
                &class.definition.to_string(),
                &class.cards,
                &class.rows,
                &class.bounds,
                &[1., 0., 0., 0., 1., 0., 0., 0., 1.],
            )?;
            static_content.foliage += 1;
        }
        if let Some(main) = water_main {
            static_content.water = crate::static_scene::water(
                &mut bridge,
                &content.map["water"],
                textures[main],
                water_mask.map_or(u32::MAX, |i| textures[i]),
                empty,
                water_foam.map_or(textures[main], |i| textures[i]),
                ocean_mask,
            )?;
        } else if !content.map["water"].is_null() {
            static_content.deferred += 1;
        }
        let mut far_slot = bfe_render::far::FAR_SLOTS;
        let mut far_world_slots = Vec::new();
        let mut near_world_ids = Vec::new();
        let mut admitted = 0;
        let mut deferred = 0;
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for (mesh_index, (mesh, layer_ids)) in content.world.iter().zip(&layer_ids).enumerate() {
            if mesh_index % 8 == 0 {
                crate::loading_work::frame().await;
                bridge.r.core.state.reset();
            }
            if mesh.material.kind == MaterialKind::Sky {
                let sky = crate::static_scene::sky(&mut bridge, mesh, &textures, far_slot, empty)?;
                far_slot = crate::static_scene::sky_end(&sky);
                static_content.skies.push(sky);
                admitted += 1;
                continue;
            }
            if !inspectable(mesh) {
                deferred += 1;
                continue;
            }
            let raw = &mesh.geometry.buffers[0];
            let vertices = raw
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect::<Vec<_>>();
            let mut authored = mesh.material.record.clone();
            let name = mesh.name.to_ascii_lowercase();
            if canopy_texture.is_some() && (name.contains("treebillboard") || name.contains("bush"))
            {
                authored["treeCanopies"] = canopy.clone();
            }
            let record: bfe_render::static_world::Record =
                serde_json::from_value(authored.clone()).map_err(|e| e.to_string())?;
            for row in vertices
                .chunks_exact(record.stride)
                .filter(|_| !mesh.material.far)
            {
                for k in 0..3 {
                    min[k] = min[k].min(row[k] as f64);
                    max[k] = max[k].max(row[k] as f64);
                }
            }
            let source = bridge.static_source(
                &authored.to_string(),
                vertices,
                mesh.geometry.indices.clone(),
            )?;
            if live && !mesh.material.far {
                bridge.static_depth_add(source, mesh.material.texture.map_or(-1, |id| textures[id] as i32))?;
            }
            if let Some(weights) = mesh.geometry.buffers.get(1) {
                bridge.static_source_weights(source, weights)?;
            }
            let mut samplers = [-1; L::SAMPLERS_LENGTH];
            samplers[L::SAMPLERS_EMPTY] = empty as i32;
            samplers[L::SAMPLERS_LAYER1] = neutral as i32;
            samplers[L::SAMPLERS_DETAIL] = neutral as i32;
            if let Some(id) = canopy_texture {
                samplers[L::SAMPLERS_TREE] = id as i32;
            }
            if let Some(id) = mesh.material.texture {
                samplers[L::SAMPLERS_MAP] = textures[id] as i32;
            }
            if let Some(id) = mesh.material.bump {
                samplers[L::SAMPLERS_BUMP] = textures[id] as i32;
            }
            if mesh.material.kind == MaterialKind::Terrain {
                samplers[L::SAMPLERS_BUMP] = -1;
                if let Some(id) = layer_ids.first() {
                    samplers[L::SAMPLERS_MAP] = textures[*id] as i32;
                    samplers[L::SAMPLERS_LAYER1] = textures[*layer_ids.get(1).unwrap_or(id)] as i32;
                }
                if let Some(id) = details[mesh_index] {
                    samplers[L::SAMPLERS_DETAIL] = textures[id] as i32;
                }
            }
            for (i, id) in layer_ids.iter().take(8).enumerate() {
                samplers[L::SAMPLERS_LAYERS + i] = textures[*id] as i32;
            }
            let world_id = bridge.static_world(
                source,
                mesh.material.far,
                if mesh.material.far {
                    mesh.material.record["lowResScale"].as_f64().unwrap_or(1.)
                } else {
                    1.
                },
                &samplers,
                if mesh.material.far {
                    &[0; L::FEATURES_LENGTH]
                } else {
                    &features
                },
                admitted as u32,
                far_slot as u32,
                admitted as u32,
            )?;
            if !mesh.material.far {
                near_world_ids.push(world_id);
            }
            if mesh.material.far {
                far_world_slots.push(far_slot);
                bridge.r.far.input[far_slot] = 1.;
                far_slot += 2;
            }
            admitted += 1;
        }
        // Graphics.farTarget and NearPass.writeImage retain separate images:
        // the shared schedule clears near depth/color before the far image's
        // depth-tested composite. A canvas alias cannot preserve that order.
        let source=resources.source(&serde_json::json!({"width":size[0],"height":size[1],"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1009,"srgb":false,"renderTarget":true}).to_string())?;
        let far_sample=resources.sample(source,r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1006,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
        let far_target=resources.target(&serde_json::json!({"width":size[0],"height":size[1],"color":far_sample,"depth":null,"depthBuffer":true,"stencil":false,"samples":0,"resolveDepth":false,"resolveStencil":false,"cube":false}).to_string())?;
        let far_texture = bridge.r.core.add_texture(resources.texture(far_sample)?);
        bridge.set_far_map(far_texture);
        let image_program = bridge.add_owned_program("effects/far-image")?;
        use bfe_render::post;
        let mut image_step = [0.; post::STEP];
        image_step[post::STEP_PROGRAM] = image_program as f64;
        image_step[post::STEP_TARGET] = post::TARGET_BOUND;
        image_step[post::STEP_WIDTH] = size[0] as f64;
        image_step[post::STEP_HEIGHT] = size[1] as f64;
        image_step[post::STEP_UNIFORMS] = 1.;
        image_step[post::STEP_FIRST + 1] = post::TYPE_SAMPLER as f64;
        image_step[post::STEP_FIRST + 2] = far_texture as f64;
        let mut image_state = [0.; post::STATE];
        image_state[post::STATE_FLAGS] = post::STATE_DEPTH_TEST as f64;
        image_state[post::STATE_DEPTH_FUNC] = glow::LEQUAL as f64;
        // Inspection has not admitted post-processing programs. Missing ids
        // must stay outside the live program bank, never alias world shader 0.
        use bfe_render::post_chain::layout::{facts as pf, program as pp};
        bridge.r.post_chain.input[pf::PROGRAMS..pf::PROGRAMS + pp::COUNT].fill(u32::MAX as f64);
        // WorldPass.prepareNative seals the opaque draw order after admission.
        // Adding descriptors alone leaves the retained submission list empty.
        bridge.r.world.sort(false);
        bridge.r.far.prepare_frame_capacity();
        bridge.r.foliage.prepare_frame_caches();
        if admitted == 0 {
            return Err("map contains no inspectable static world".into());
        }
        let center = std::array::from_fn::<_, 3, _>(|k| (min[k] + max[k]) * 0.5);
        let radius = ((max[0] - min[0]).max(max[2] - min[2])) * 0.5;
        let mut camera = MapCamera::new(
            [
                center[0],
                center[1] + radius * 0.35,
                center[2] + radius * 0.6,
            ],
            size,
        )?;
        environment(&mut camera.view, &content.map);
        if live {
            // Graphics.applyDistance uses the authored near fade, including
            // the foliage cutoff. The inspector's wide diagnostic lens is
            // not a battle view-distance setting.
            let range = &content.map["distance"]["nearScene"];
            let end = range[3].as_f64().filter(|v| v.is_finite() && *v > 0.3).unwrap_or(500.);
            let start = range[1].as_f64().filter(|v| v.is_finite() && *v >= 0. && *v < end).unwrap_or(300.);
            set(&mut camera.view, "fade", &[start, end]);
            let mut projection = Matrix4::default();
            let top = 0.3 * (60.0f64.to_radians() * 0.5).tan();
            let right = top * size[0] as f64 / size[1] as f64;
            projection.make_perspective(-right, right, top, -top, 0.3, end, CoordinateSystem::WebGL);
            set(&mut camera.view, "projectionMatrix", &projection.elements);
        }
        let mut projection = Matrix4::default();
        let top = 2. * (60.0f64.to_radians() * 0.5).tan();
        let right = top * size[0] as f64 / size[1] as f64;
        let reach = content.map["distance"]["farScene"][1]
            .as_f64()
            .unwrap_or(3000.);
        projection.make_perspective(-right, right, top, -top, 2., reach, CoordinateSystem::WebGL);
        deferred += static_content.deferred;
        eprintln!(
            "native static admission: props={}, foliage={}, water={}, sky={}",
            static_content.props,
            static_content.foliage,
            usize::from(static_content.water.is_some()),
            static_content.skies.len()
        );
        crate::loading_work::frame().await;
        bridge.r.core.state.reset();
        let online = if live {
            Some(Box::new(
                crate::online::replicas::Replicas::new(
                    &mut content,
                    &mut bridge,
                    &textures,
                    prepared_views.ok_or("missing first-person resources")?,
                    prepared_hud.ok_or("missing battle HUD resources")?,
                    prepared_particles.ok_or("missing battle particle resources")?,
                    prepared_sound.ok_or("missing battle audio resources")?,
                    &mut resources,
                    empty,
                    size,
                )
                .await?,
            ))
        } else {
            None
        };
        let bones = static_content.records.iter()
            .map(|row| row[bfe_render::instances::REC_BONES] as usize)
            .chain([online.as_ref().map_or(1, |resources| resources.max_bones())])
            .max()
            .unwrap_or(1)
            .max(1);
        let shadows = if live {
            Some(crate::shadows::Shadows::new(&mut bridge, &mut resources, bones).await?)
        } else {
            None
        };
        bridge.frame_reserve_frame();
        Ok((
            Self {
                online,
                bridge,
                resources,
                far_target,
                shadows,
                image_step,
                image_state,
                canvas,
                spawn_shots,
                menu_models,
                menu_grid,
                spawn_objectives,
                spawn_vehicles,
                admitted,
                deferred,
                draws: 0,
                size,
                static_content,
                far_projection: projection.elements,
                far_world_slots,
                near_world_ids,
            },
            camera,
        ))
    }
    /// A direct join publishes all live owners together before the renderer
    /// borrows their frame banks. The window supplies only its elapsed clock.
    // Presentation borrows an already decoded session. Keeping this boundary
    // separate from socket pumping also lets recorded network sessions exercise
    // the complete native frame without constructing a transport or a World.
    pub fn present_network(&mut self, receiver:&crate::online::receiver::Receiver,
        replay:&crate::online::replay::Replay, history:&crate::online::replay::History,
        camera:&mut MapCamera,dt:f64) {
        if let Some(resources)=&mut self.online {
            let mut world=Matrix4::default();
            world.elements.copy_from_slice(&camera.snapshot().values[16..32]);world.invert();
            resources.frame(receiver,receiver.clock.shown,dt.min(8./60.),camera.position,
                replay,history,&mut self.bridge);
            let cue = resources.cameras.frame(receiver,replay,&mut resources.hulls,&mut resources.turrets,camera,dt);
            world.elements.copy_from_slice(&camera.snapshot().values[16..32]);world.invert();
            resources.first_person.enabled = resources.cameras.eye_view;
            resources.finish_frame(receiver, replay, &world.elements, cue);
            let reticle = resources.reticle_point(receiver);
            resources.hud.aim(&mut self.bridge, camera.snapshot(), reticle);
            resources.sound.bleed = [resources.hud.teams[1][3], resources.hud.teams[2][3]];
            resources.sound.frame(receiver,&world.elements,dt,replay,resources.hud.audio_facts());
        }
    }
    pub fn online_frame(&mut self,client:&mut crate::online::client::Client,dt:f64){
        self.present_network(&client.receiver,&client.replay,&client.sender.history,&mut client.camera,dt);
        client.receiver.publications.clear();
    }
    pub fn draw(&mut self, view: &View) -> Result<(), String> {
        self.draw_selected(view, Component::All)
    }
    fn draw_selected(&mut self, view: &View, component: Component) -> Result<(), String> {
        use bfe_render::frame_plan::layout::{
            binding as b, facts as f, kind as k, meta as m, role as r, stats as t,
        };
        let mut frame_view = *view;
        let view = &mut frame_view;
        self.bridge.frame_begin_frame();
        self.bridge.r.frames.plan.facts.fill(0.);
        self.bridge.r.frames.plan.facts[f::MAIN] = 1.;
        if self.online.is_some(){self.bridge.r.frames.plan.facts[f::HUD_KINDS]=k::HUD as f64;}
        // Target rows are numeric facts, not a host-selected draw sequence.
        // Renderer::battle_frame owns binds, clears, composites and the tail.
        for role in [r::FAR, r::NEAR, r::IMAGE, r::TRANSPARENT, r::HUD0] {
            let row = &mut self.bridge.r.frames.bindings[role * b::LEN..(role + 1) * b::LEN];
            row.fill(0.);
            row[b::TARGET] = if role == r::FAR {
                self.far_target as f64
            } else {
                -1.
            };
            row[b::SAMPLE] = -1.;
            row[b::WIDTH] = self.size[0] as f64;
            row[b::HEIGHT] = self.size[1] as f64;
            row[b::SCISSOR + 2] = self.size[0] as f64;
            row[b::SCISSOR + 3] = self.size[1] as f64;
            row[b::CLEAR..b::CLEAR + 4].copy_from_slice(&[0.04, 0.06, 0.09, 1.]);
            self.bridge.r.frames.meta[role * m::LEN + m::BOUND] = 1.;
        }
        self.bridge.r.core.input[..shader::FRAME_FLOATS].copy_from_slice(&view.values);
        crate::static_scene::update(&mut self.static_content, &mut self.bridge, &view.values);
        if component == Component::All {
            self.bridge.r.core.input[..16].copy_from_slice(&self.far_projection);
        }
        for id in &self.near_world_ids {
            self.bridge
                .r
                .world
                .world_shown(*id, component == Component::All);
        }
        for sky in &self.static_content.skies {
            self.bridge.r.far.input[sky.slot] =
                if matches!(component, Component::All | Component::Sky) {
                    1.
                } else {
                    0.
                };
        }
        for slot in &self.far_world_slots {
            self.bridge.r.far.input[*slot] = if component == Component::All { 1. } else { 0. };
        }
        self.bridge.r.far.input[bfe_render::far::FAR_CLEAR..bfe_render::far::FAR_CLEAR + 4]
            .copy_from_slice(&[0.04, 0.06, 0.09, 1.]);
        self.bridge.r.far.input[bfe_render::far::FAR_DRAW] = 1.;
        self.publish(r::FAR, k::FAR, 0, 0, 0);
        self.bridge.r.core.input[..shader::FRAME_FLOATS].copy_from_slice(&view.values);
        let mut records = if matches!(component, Component::All | Component::Props) {
            let room=if component==Component::All{self.online.as_ref().map_or(0,|r|r.record_budget().min(bfe_render::instances::MAX_RECORDS))}else{0};
            self.static_content.select_limit(&mut self.bridge,bfe_render::instances::MAX_RECORDS-room)
        } else {
            0
        };
        if matches!(component, Component::All | Component::Foliage) {
            self.prepare_foliage(view);
        } else {
            self.bridge.foliage_clear_frame();
        }
        let water_shown = self.bridge.r.water.input[bfe_render::water::S_SHOWN];
        if !matches!(component, Component::All | Component::Water) {
            self.bridge.r.water.input[bfe_render::water::S_SHOWN] = 0.;
        }
        let mut bones = self.bridge.r.core.input[bfe_render::instances::IN_RECORDS..]
            .chunks_exact(bfe_render::instances::REC_STRIDE)
            .take(records)
            .map(|row| {
                (row[bfe_render::instances::REC_BONE_AT] + row[bfe_render::instances::REC_BONES])
                    as usize
            })
            .max()
            .unwrap_or(0);
        if component==Component::All{if let Some(online)=&self.online{(records,bones)=online.append(&mut self.bridge,records,bones);}}
        if let Some(shadows) = &mut self.shadows { shadows.publish(&mut self.bridge, view, records)?; }
        self.bridge.r.core.input[..shader::FRAME_FLOATS].copy_from_slice(&view.values);
        self.publish(r::NEAR, k::SCENE, records, bones, 0);
        self.bridge.r.post.input[..bfe_render::post::STEP].copy_from_slice(&self.image_step);
        self.bridge.r.post.states[..bfe_render::post::STATE].copy_from_slice(&self.image_state);
        self.publish(r::IMAGE, k::SCREEN, 0, 0, 1);
        if let Some(online)=&self.online{self.bridge.r.frames.meta[r::TRANSPARENT*m::LEN+m::FX_WORDS]=online.combat.particles.words as f64;}
        self.publish(r::TRANSPARENT, k::TRANSPARENT, records, bones, 0);
        if let Some(online)=&self.online{let (view_records,view_bones)=online.first_person.append(&online.units,&mut self.bridge);if view_records!=0{let mut projection=Matrix4::default();let top=0.3*(std::f64::consts::PI/6.).tan();let right=top*self.size[0]as f64/self.size[1]as f64;projection.make_perspective(-right,right,top,-top,0.3,10.,CoordinateSystem::WebGL);self.bridge.r.core.input[..16].copy_from_slice(&projection.elements);self.bridge.r.frames.plan.facts[f::VIEW_SHOWN]=1.;self.publish(r::VIEW,k::VIEW,view_records,view_bones,0);}}
        if self.online.is_some(){self.publish(r::HUD0,k::HUD,0,0,0);}
        let success = self.bridge.battle_frame(&mut self.resources);
        self.bridge.r.water.input[bfe_render::water::S_SHOWN] = water_shown;
        if !success {
            return Err(self
                .bridge
                .frame_error_frame()
                .unwrap_or("native map frame failed".into()));
        }
        self.draws += self
            .bridge
            .r
            .frames
            .stats
            .chunks_exact(t::LEN)
            .map(|row| row[t::PASS_DRAWS..].iter().sum::<f64>() as u64)
            .sum::<u64>();
        Ok(())
    }
    fn publish(&mut self, role: usize, kind: usize, records: usize, bones: usize, steps: usize) {
        use bfe_render::frame_plan::layout::meta as m;
        let row = &mut self.bridge.r.frames.meta[role * m::LEN..(role + 1) * m::LEN];
        row[m::KIND] = kind as f64;
        row[m::RECORDS] = records as f64;
        row[m::BONES] = bones as f64;
        row[m::STEPS] = steps as f64;
        // FrameBank clips a full packet and retains its fault marker, just
        // as the browser producer does. Capacity never terminates the window.
        self.bridge.frame_publish_frame(role as u32);
    }
    fn prepare_foliage(&mut self, view: &View) {
        let input = &mut self.bridge.r.foliage.input;
        input[bfe_render::foliage::IN_PROJECTION..bfe_render::foliage::IN_PROJECTION + 16]
            .copy_from_slice(&view.values[..16]);
        input[bfe_render::foliage::IN_VIEW..bfe_render::foliage::IN_VIEW + 16]
            .copy_from_slice(&view.values[16..32]);
        let mut world = Matrix4::default();
        world.elements.copy_from_slice(&view.values[16..32]);
        world.invert();
        input[bfe_render::foliage::IN_WORLD..bfe_render::foliage::IN_WORLD + 16]
            .copy_from_slice(&world.elements);
        input[bfe_render::foliage::IN_EYE..bfe_render::foliage::IN_EYE + 3]
            .copy_from_slice(&view.values[32..35]);
        input[bfe_render::foliage::IN_END] = view.values[bfe_render::cameras::layout::shader_frame::FADE + 1];
        input[bfe_render::foliage::IN_TIME] = view.values[35];
        self.bridge.foliage_frame();
    }
    /// Probe visibility uses the same packets and battle_frame as the window.
    /// Only admitted object prefixes change; no alternate pass order exists.
    pub fn draw_component(&mut self, view: &View, name: &str) -> Result<(), String> {
        let component = match name {
            "props" => Component::Props,
            "foliage" => Component::Foliage,
            "water" => Component::Water,
            "sky" => Component::Sky,
            _ => return Err("unknown static component".into()),
        };
        self.draw_selected(view, component)
    }
    pub fn frame_trace(&self) -> &[f64] {
        use bfe_render::frame_plan::layout::command as c;
        &self.bridge.r.frames.plan.trace[..self.bridge.r.frames.plan.count * c::LEN]
    }
    /// Cold diagnostics for the isolated-view tool; never read by the frame loop.
    pub fn bindings(&self) -> serde_json::Value {
        use bfe_render::{instances as I, water as W};
        let rigs=self.static_content.records.iter().map(|r|{let start=r[I::REC_BONE_AT] as usize*16;let len=r[I::REC_BONES] as usize*16;serde_json::json!({"world":&r[I::REC_WORLD..I::REC_WORLD+16],"bind":r[I::REC_BIND_INVERSE..I::REC_BIND_INVERSE+16].iter().map(|v|*v as f32 as f64).collect::<Vec<_>>(),"bones":self.static_content.bones[start..start+len].iter().map(|v|*v as f64).collect::<Vec<_>>()})}).collect::<Vec<_>>();
        let mut uniforms = serde_json::Map::new();
        if let Some(w) = &self.static_content.water {
            if let Ok(spec) = bfe_render::programs::spec(w.key) {
                for (i, (name, ty)) in spec.uniforms.iter().enumerate() {
                    if *ty >= 32 {
                        continue;
                    }
                    let at = W::S_FIRST + i * bfe_render::post::UNIFORM + 2;
                    let len = (*ty as usize).min(16);
                    uniforms.insert(
                        name.to_string(),
                        serde_json::json!(self.bridge.r.water.input[at..at + len]
                            .iter()
                            .map(|v| *v as f32 as f64)
                            .collect::<Vec<_>>()),
                    );
                }
            }
        }
        serde_json::json!({"rigs":rigs,"uniforms":uniforms})
    }
    pub fn hash(&self) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        Ok(format!("{:x}", Sha256::digest(self.pixels()?)))
    }
    pub fn pixels(&self) -> Result<Vec<u8>, String> {
        let gl = &self.bridge.r.core.gl;
        let mut bytes = vec![0; self.size[0] as usize * self.size[1] as usize * 4];
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.canvas.framebuffer));
            gl.read_pixels(
                0,
                0,
                self.size[0],
                self.size[1],
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut bytes)),
            );
            let error = gl.get_error();
            if error != glow::NO_ERROR {
                return Err(format!("native scene GL error {error}"));
            }
        }
        Ok(bytes)
    }
}

// Blended world buckets need their own authored transparent descriptors.
// Sky, opaque far meshes and the independent static passes are admitted above.
fn inspectable(mesh: &bfe_content_native::loader::Mesh) -> bool {
    mesh.geometry.vertices != 0
        && !mesh.geometry.indices.is_empty()
        && mesh.material.kind != MaterialKind::Sky
        && mesh.material.blend == Blend::Opaque
}

// materials.js::setLighting/setFog read only authored map content. The inspector
// uses these values without fabricating point lights, shadow facts or actors.
fn environment(view: &mut View, map: &serde_json::Value) {
    let l = &map["lighting"];
    for (uniform, key) in [
        ("ambient", "ambient"),
        ("ambientTop", "top"),
        ("ambientBottom", "bottom"),
        ("characterAmbient", "character"),
        ("sunColor", "sunColor"),
    ] {
        let color = if uniform == "characterAmbient" && l[key].is_null() {
            &l["ambient"]
        } else {
            &l[key]
        };
        let values = std::array::from_fn::<_, 3, _>(|i| color[i].as_f64().unwrap_or(128.) / 255.);
        set(view, uniform, &values);
    }
    let mut sun = Vector3::new(0., -1., 0.);
    if let Ok(v) = serde_json::from_value::<[f64; 3]>(l["sunDirection"].clone()) {
        sun.set(v[0], v[1], v[2]).normalize();
    }
    set(view, "sunDirection", &[sun.x, sun.y, sun.z]);
    set(view, "shadowStrength", &[1.]);
    let low = &map["distance"]["lowRes"];
    if let Ok(fog) = serde_json::from_value::<[f64; 3]>(low["fog"].clone()) {
        set(view, "fogColor", &fog.map(|v| v / 255.));
    }
    let range = serde_json::from_value::<[f64; 2]>(low["fogRange"].clone())
        .ok()
        .filter(|r| !low["fog"].is_null() && r[0] >= 0. && r[0] < r[1])
        .unwrap_or([1e9, 2e9]);
    set(view, "fogRange", &range);
}
