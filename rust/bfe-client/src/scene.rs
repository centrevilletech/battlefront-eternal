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
use bfe_render::{core, shader, static_layout as L};
use glow::HasContext;
use std::path::Path;
use three::{CoordinateSystem, Matrix4, Vector3};

pub struct View {
    pub values: [f64; shader::FRAME_FLOATS],
}
pub struct MapCamera {
    pub position: [f64; 3],
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
            yaw: 0.0,
            pitch: -0.4,
            time: 0.0,
            view,
            slots,
        })
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

pub struct Scene {
    bridge: Bridge,
    _resources: Resources,
    pub canvas: Canvas,
    pub admitted: usize,
    pub deferred: usize,
    pub draws: u64,
    size: [i32; 2],
}
impl Scene {
    pub fn load(
        root: &Path,
        map: &str,
        size: [i32; 2],
        mut load: impl FnMut() -> glow::Context,
    ) -> Result<(Self, MapCamera), String> {
        let mut loader = Loader::new(root);
        loader.compressed = false;
        let mut content = loader.load(map, &format!("map={map}&seed=1"))?;
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
        features[L::FEATURES_POINTS] = (ORDNANCE_LIGHTS + posts) as u32;
        let mut used = vec![false; content.textures.images.len()];
        for (i, mesh) in content
            .world
            .iter()
            .enumerate()
            .filter(|(_, m)| inspectable(m))
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
        let canvas = Canvas::new(load(), size)?;
        let mut resources = Resources::new(load());
        resources.canvas = Some(canvas.framebuffer);
        let mut bridge = Bridge::new(load(), Ranges::new(load()));
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
        }
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
        let mut admitted = 0;
        let mut deferred = 0;
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for (mesh_index, (mesh, layer_ids)) in content.world.iter().zip(&layer_ids).enumerate() {
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
            for row in vertices.chunks_exact(record.stride) {
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
            bridge.static_world(
                source,
                false,
                1.0,
                &samplers,
                &features,
                admitted as u32,
                admitted as u32,
                admitted as u32,
            )?;
            admitted += 1;
        }
        // WorldPass.prepareNative seals the opaque draw order after admission.
        // Adding descriptors alone leaves the retained submission list empty.
        bridge.r.world.sort(false);
        if admitted == 0 {
            return Err("map contains no inspectable static world".into());
        }
        let center = std::array::from_fn::<_, 3, _>(|k| (min[k] + max[k]) * 0.5);
        let radius = ((max[0] - min[0]).max(max[2] - min[2])) * 0.5;
        let camera = MapCamera::new(
            [
                center[0],
                center[1] + radius * 0.35,
                center[2] + radius * 0.6,
            ],
            size,
        )?;
        Ok((
            Self {
                bridge,
                _resources: resources,
                canvas,
                admitted,
                deferred,
                draws: 0,
                size,
            },
            camera,
        ))
    }
    pub fn draw(&mut self, view: &View) -> Result<(), String> {
        let gl = &self.bridge.r.core.gl;
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.canvas.framebuffer));
            gl.disable(glow::SCISSOR_TEST);
            gl.viewport(0, 0, self.size[0], self.size[1]);
            gl.color_mask(true, true, true, true);
            gl.depth_mask(true);
            gl.clear_color(0.04, 0.06, 0.09, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
        self.bridge.r.core.input[..shader::FRAME_FLOATS].copy_from_slice(&view.values);
        let stats = self.bridge.frame(0)?;
        self.draws +=
            u64::from(stats.pass_draws[core::PASS_WORLD] + stats.pass_draws[core::PASS_TERRAIN]);
        Ok(())
    }
    pub fn hash(&self) -> Result<String, String> {
        use sha2::{Digest, Sha256};
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
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

// Transparent, sky and far resources retain their full recorded-session path;
// the inspector admits the opaque static world and authored terrain only.
fn inspectable(mesh: &bfe_content_native::loader::Mesh) -> bool {
    mesh.geometry.vertices != 0
        && !mesh.geometry.indices.is_empty()
        && mesh.material.kind != MaterialKind::Sky
        && mesh.material.blend == Blend::Opaque
        && !mesh.material.far
}
