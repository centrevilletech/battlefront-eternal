//! Native particle resources share EffectsPass's transparent list. Both the
//! ordnance owner and effect owner publish into these same retained batches.
use crate::{render_bridge::Bridge, resources::Resources};
use bfe_content_native::BattleContent;
use bfe_render::effects as render;
use serde_json::json;
use std::collections::BTreeMap;
mod refraction;
mod trails;
#[cfg(all(test, target_os = "linux"))]
mod gl_tests;
const CAPACITY: usize = 1024;
// All particles in a Battlefront II mission may share one material.
const MISSION_CAPACITY: usize = 4096;
// The 2004 game's drop shadow under a soldier: a blurred standing figure, head at the image's top row.
const DROP_SHADOW_TEXTURE: &str = "player_shadow";
// The figure fills the image from about 12% to 88% of its height; its feet are at 88%.
const DROP_SHADOW_FEET: f64 = 0.88;
const DROP_SHADOW_FIGURE: f64 = 0.76;
const DROP_SHADOW_OPACITY: f32 = 0.6;
// Model particles drawn in one frame, each part of a model counting one.
const MODEL_DRAWS: usize = 256;
// A particle recipe names existing renderer programs and one texture.
// CPU effects, analytic effects and ordnance quads have distinct attribute
// layouts even when they name the same image. The complete key below keeps
// those material/resource identities separate without live string lookup.
pub struct Recipe {
    pub image: usize,
    pub analytic: bool,
    pub bolt: bool,
    pub additive: bool,
}
pub struct Prepared {
    pub recipes: Vec<Recipe>,
    keys: BTreeMap<(usize, bool, bool, bool), usize>,
    pub effect_batches: Vec<Option<usize>>,
    // The model each GEOMETRY definition draws its particles with.
    pub effect_models: Vec<Option<String>>,
    pub textures: BTreeMap<String, usize>,
    // The batch of the soldiers' drop shadows (`player_shadow`), when the content has it.
    pub drop_shadow: Option<usize>,
}
impl Prepared {
    pub fn new(content: &mut BattleContent) -> Result<Self, String> {
        let mut prepared = Self::new_shared(content)?;
        if content.content.battle.family == bfe_content::GameFamily::Swbf2 {
            super::combat::swbf2::load_round_textures(content, &mut prepared)?;
        }
        Ok(prepared)
    }
    fn new_shared(content: &mut BattleContent) -> Result<Self, String> {
        let defs = content.defs()?;
        let mut names = vec!["lightglow".to_string()];
        for (_, d) in &defs.registry.combat {
            if let Some(name) = d.get("LaserTexture").and_then(bfe_content::Value::as_str) {
                names.push(name.into());
            }
        }
        // The arc caster's ribbons draw with a texture of their own.
        names.extend(super::lightning::definitions(content).into_iter().map(|(_, _, texture)| texture));
        // A droideka's shield bubble may carry a field texture.
        names.extend(super::shields::textures(content));
        // The command posts' holograms: their flare and the logos' texture.
        names.extend(super::holograms::textures(content));
        // Lightsabre blades: each melee weapon's texture and the glow.
        names.extend(super::blades::textures(content));
        // The Enhanced steam's wisps (steam.rs).
        names.push("thicksmoke2".into());
        // The Enhanced fountains' foam (fountains.rs).
        names.push("waterfoam1".into());
        // The soldiers' drop shadows while the sun casts none (drop_shadows).
        names.push(DROP_SHADOW_TEXTURE.into());
        // The map's snowflake, which the snow draws (weather.rs).
        if let Some(name) = content.map["weather"]["precipitation"]["Texture"][0].as_str() {
            names.push(name.into());
        }
        for (_, effects) in &content.effects {
            for d in effects.descriptor["definitions"].as_array().into_iter().flatten() {
                if let Some(name) = d["geometry"]["texture"].as_str() {
                    names.push(name.into());
                }
            }
        }
        // Resolve every effect and ordnance texture while loading. Optional
        // absent textures retain a missing handle; a displayed event never
        // starts file I/O or constructs a texture. Shared images use one
        // TextureLibrary entry and one subsequent GPU upload.
        let mut textures = BTreeMap::new();
        for name in names {
            if textures.contains_key(&name) {
                continue;
            }
            if let Some(path) = content.texture_paths.get(&name.to_lowercase()) {
                textures.insert(name, content.textures.load(path)?);
            }
        }
        let mut prepared = Self { recipes: vec![], keys: BTreeMap::new(), effect_batches: vec![], effect_models: vec![], textures, drop_shadow: None };
        prepared.drop_shadow = prepared.textures.get(DROP_SHADOW_TEXTURE).copied().map(|image| prepared.batch(image, false, false, false));
        if let Some((_, effects)) = content.effects.iter().find(|(n, _)| n == "battle") {
            for d in effects.descriptor["definitions"].as_array().into_iter().flatten() {
                let name = d["geometry"]["texture"].as_str().unwrap_or("");
                let batch =
                    prepared.textures.get(name).copied().map(|image| {
                        prepared.batch(image, d["gpu"].as_bool().unwrap_or(false), false, d["geometry"]["blend"] == "additive")
                    });
                prepared.effect_batches.push(batch);
                // a GEOMETRY particle draws its model, never a quad.
                let model = (d["geometry"]["type"] == "geometry").then(|| d["geometry"]["model"].as_str()).flatten();
                prepared.effect_models.push(model.map(str::to_string));
            }
        }
        Ok(prepared)
    }
    // batches by geometry layout and blend state as well as map.
    // Deduplicating only the image would pair analytic tracks with CPU quads,
    // or additive bolts with alpha particles. This lookup is cold admission;
    // runtime publications carry the resulting fixed batch handle.
    pub fn batch(&mut self, image: usize, analytic: bool, bolt: bool, additive: bool) -> usize {
        let key = (image, analytic, bolt, additive);
        if let Some(id) = self.keys.get(&key) {
            return *id;
        }
        let id = self.recipes.len();
        self.keys.insert(key, id);
        self.recipes.push(Recipe { image, analytic, bolt, additive });
        id
    }
    pub fn ordnance(&mut self, name: &str) -> Option<usize> {
        self.textures.get(name).copied().map(|image| self.batch(image, false, true, true))
    }
    pub fn admit(
        self,
        content: &BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
        resources: &mut Resources,
    ) -> Result<Particles, String> {
        let materials = (content.content.battle.family == bfe_content::GameFamily::Swbf2)
            .then(|| refraction::materials(content, &self));
        let mut particles = self.admit_shared(content, bridge, textures, resources)?;
        if let Some(materials) = materials.filter(|v| !v.is_empty()) {
            particles.refraction = Some(refraction::Refraction::new(&materials, &mut particles, textures, resources, bridge)?);
        }
        if content.content.battle.family == bfe_content::GameFamily::Swbf2 {
            if let Some((_, library)) = content.effects.iter().find(|(name, _)| name == "battle") {
                let defs = library.descriptor["definitions"].as_array().unwrap();
                if defs.iter().any(|d| d["geometry"]["type"] == "trail") {
                    particles.trails = defs.iter().map(|d| d["geometry"]["type"] == "trail").collect();
                }
            }
        }
        Ok(particles)
    }
    fn admit_shared(
        self,
        content: &BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
        resources: &mut Resources,
    ) -> Result<Particles, String> {
        let capacity = if content.content.battle.family == bfe_content::GameFamily::Swbf2 {
            MISSION_CAPACITY
        } else {
            CAPACITY
        };
        let shadow_cascades = bridge.shadow_cascades();
        let effects = content.effects.iter().find(|(n, _)| n == "battle").map(|(_, e)| e).ok_or("missing effect library")?;
        let bytes = effects.gpu_tracks.iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<_>>();
        let source=resources.source(&json!({"width":effects.gpu_size[0],"height":effects.gpu_size[1],"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1015,"srgb":false}).to_string())?;
        resources.source_pixels(source, &bytes)?;
        let sample=resources.sample(source,r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
        resources.upload(sample)?;
        resources.release_pixels(source)?;
        let tracks = bridge.r.core.add_texture(resources.texture(sample)?);
        let owner = bridge.fx_add_owner(tracks, effects.gpu_size[1] as f32);
        bridge.r.effects.prepare_owner(&mut bridge.r.core, owner, 1024);
        let mut batches = Vec::new();
        let mut words = 1024 * 12;
        let swbf2 = content.content.battle.family == bfe_content::GameFamily::Swbf2;
        let (mut swbf2_cpu, mut swbf2_analytic) = (None, None);
        for recipe in self.recipes {
            let key = if recipe.analytic {
                "effects/particle-analytic"
            } else if recipe.bolt {
                "effects/particle-bolt"
            } else {
                "effects/particle-cpu"
            };
            let spec = bfe_render::programs::spec(key)?;
            let program = if swbf2 && !recipe.bolt {
                let slot = if recipe.analytic { &mut swbf2_analytic } else { &mut swbf2_cpu };
                match *slot {
                    Some(id) => id,
                    None => {
                        let id = swbf2_particle_program(bridge, spec)?;
                        *slot = Some(id);
                        id
                    }
                }
            } else {
                bridge.add_owned_program(key)?
            };
            let fields = if recipe.analytic {
                vec![("spawnPosition", 3), ("spawnVelocity", 3), ("spawnColor", 4), ("spawnShape", 4), ("spawnMeta", 4)]
            } else {
                vec![("iPosition", 3), ("iColor", 4), ("iShape", 4), ("iAxis", 3), ("iAxis2", 3), ("iFrame", 4)]
            };
            let mut attributes = Vec::new();
            for (name, size) in fields {
                attributes.extend([spec.attributes.iter().find(|(n, _)| *n == name).ok_or("missing particle attribute")?.1, size]);
            }
            let corner = spec.attributes.iter().find(|(n, _)| *n == "corner").ok_or("missing particle corner")?.1;
            let blend = [
                1,
                glow::FUNC_ADD,
                glow::FUNC_ADD,
                glow::SRC_ALPHA,
                if recipe.additive { glow::ONE } else { glow::ONE_MINUS_SRC_ALPHA },
                glow::ONE,
                if recipe.additive { glow::ONE } else { glow::ONE_MINUS_SRC_ALPHA },
            ];
            let handle = bridge.fx_add_batch(
                program,
                textures[recipe.image] as i32,
                &blend,
                corner,
                &attributes,
                if recipe.analytic { owner as i32 } else { -1 },
            );
            bridge.fx_batch_interleaved(handle);
            bridge.r.effects.prepare_batch(&bridge.r.core, handle, capacity);
            let stride = if recipe.analytic { 18 } else { 21 };
            let at = words;
            words += capacity * stride;
            batches.push(Batch { handle, analytic: recipe.analytic, at, stride, capacity, count: 0, order: 0 });
        }
        // glowModel: each model a GEOMETRY definition names, drawn
        // additive and double-sided with its first part's texture, tinted
        // by the particle. One renderer model serves every particle of a
        // part; a model the content lacks draws nothing.
        let shadows = shadow_cascades > 0;
        let mut fields = GlowFields::default();
        for &(name, size) in bfe_render::effect_layout::PRODUCERS[bfe_render::effect_layout::ACTOR_GLOW].1 {
            match name {
                "projectionMatrix" => fields.projection = fields.length,
                "diffuse" => fields.diffuse = fields.length,
                "opacity" => fields.opacity = fields.length,
                _ => {}
            }
            fields.length += size as usize;
        }
        let mut glows: Vec<(String, Glow)> = Vec::new();
        let mut effect_glows = Vec::new();
        // hands combat `glowModel("muzzleflash")` for the flash at a
        // held gun's muzzle: the same kind of model, listed last here.
        let flash = match content.content.battle.family {
            bfe_content::GameFamily::Swbf2 => content.visual_effects().get("muzzleModel")
                .and_then(bfe_content::Value::as_str).map(str::to_owned),
            bfe_content::GameFamily::Swbf1 => Some(FLASH.to_string()),
        };
        for name in self.effect_models.iter().chain([&flash]) {
            let Some(name) = name else {
                effect_glows.push(None);
                continue;
            };
            if let Some(known) = glows.iter().position(|(known, _)| known == name) {
                effect_glows.push(Some(known));
                continue;
            }
            let prefix = format!("{name}/");
            let mut meshes: Vec<_> = content.models.iter().filter(|m| m.name.starts_with(&prefix)).collect();
            // A map effect's model comes from the map's environment pack.
            if meshes.is_empty() {
                let prefix = format!("environment/{name}/");
                meshes = content.models.iter().filter(|m| m.name.starts_with(&prefix)).collect();
            }
            let Some(first) = meshes.first() else {
                effect_glows.push(None);
                continue;
            };
            let texture = first.material.texture.map_or(-1, |t| textures[t] as i32);
            let policy = bfe_render::actor_effects::Policy::new(bfe_render::effect_layout::ACTOR_GLOW, shadows, false, texture, &Default::default())?;
            if policy.defaults.len() != fields.length {
                return Err("glow model values differ from the catalog".into());
            }
            let mut parts = Vec::new();
            for mesh in meshes {
                let model = bridge.fx_add_model(texture, &policy.blend, 1, glow::LEQUAL, &policy.values, &bounds(&mesh.geometry));
                for (key, side) in &policy.keys {
                    let program = bridge.add_owned_program(key)?;
                    let geometry = mesh.geometry.register(&mut bridge.r, key)?;
                    bridge.fx_model_pass(model, program, *side, geometry);
                }
                parts.push(model);
            }
            glows.push((name.clone(), Glow { parts, values: policy.defaults.iter().map(|v| *v as f32).collect() }));
            effect_glows.push(Some(glows.len() - 1));
        }
        let flash = effect_glows.pop().flatten();
        let model_at = words;
        words += MODEL_DRAWS * fields.length;
        bridge.fx_reserve(words as u32);
        Ok(Particles {
            refraction: None,
            trails: Vec::new(),
            batches,
            sequence: 0,
            effect_batches: self.effect_batches,
            drop_shadow: self.drop_shadow,
            glows: glows.into_iter().map(|(_, glow)| glow).collect(),
            effect_glows,
            flash,
            fields,
            model_at,
            model_rows: Vec::with_capacity(MODEL_DRAWS),
            owner,
            words,
            epochs: vec![0.; 1024],
        })
    }
}
// The sphere three would cull a part by: its box's middle and the farthest
// vertex from it. Positions lead each 32-byte vertex.
fn bounds(geometry: &bfe_content_native::geometry::Geometry) -> [f64; 4] {
    let points: Vec<[f64; 3]> = geometry.buffers[0]
        .chunks_exact(32)
        .map(|v| std::array::from_fn(|axis| f32::from_le_bytes(v[axis * 4..axis * 4 + 4].try_into().unwrap()) as f64))
        .collect();
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in &points {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    let center: [f64; 3] = std::array::from_fn(|axis| if points.is_empty() { 0. } else { (low[axis] + high[axis]) / 2. });
    let radius = points.iter().map(|p| (0..3).map(|axis| (p[axis] - center[axis]).powi(2)).sum::<f64>().sqrt()).fold(0., f64::max);
    [center[0], center[1], center[2], radius]
}
// The model game.js draws at a held gun's muzzle when it fires.
const FLASH: &str = "muzzleflash";
// A model's renderer models, one a part, and the material values they
// start from.
struct Glow {
    parts: Vec<u32>,
    values: Vec<f32>,
}
// Where the live values sit in a glow model's material values.
#[derive(Default)]
struct GlowFields {
    projection: usize,
    diffuse: usize,
    opacity: usize,
    length: usize,
}
struct Batch {
    handle: u32,
    analytic: bool,
    at: usize,
    stride: usize,
    capacity: usize,
    count: usize,
    // Its place among the batches by first use, from one; zero until then.
    order: u32,
}
impl Batch {
    fn full(&self, dropped: &mut u32) -> bool {
        if self.count < self.capacity {
            return false;
        }
        if self.capacity == MISSION_CAPACITY {
            *dropped = dropped.saturating_add(1);
        }
        true
    }
    // ParticleRenderer.batch makes a batch the first time a
    // particle asks for it, and three draws transparent meshes that tie on
    // render order and depth in the order they were made. So a batch first
    // used earlier in the battle draws under one first used later: the
    // dirt an impact throws up sits under the blast's glow, not over it.
    fn add(&mut self, sequence: &mut u32) {
        if self.order == 0 {
            *sequence += 1;
            self.order = *sequence;
        }
        self.count += 1;
    }
}
pub struct Particles {
    pub(in crate::online) refraction: Option<refraction::Refraction>,
    batches: Vec<Batch>,
    trails: Vec<bool>,
    sequence: u32,
    pub effect_batches: Vec<Option<usize>>,
    drop_shadow: Option<usize>,
    glows: Vec<Glow>,
    effect_glows: Vec<Option<usize>>,
    // The muzzle flash's glow model, when the content has one.
    flash: Option<usize>,
    fields: GlowFields,
    // Where the frame's model values start in the effect data, and the
    // frame's model draws: renderer model, then the particle's position.
    model_at: usize,
    model_rows: Vec<(u32, [f64; 3])>,
    owner: u32,
    pub words: usize,
    epochs: Vec<f64>,
}
impl Particles {
    pub(crate) async fn warm_mission(&self, bridge: &mut Bridge, target: glow::Framebuffer, second_queue: bool) -> Result<(), String> {
        if self.batches.is_empty() { return Ok(()); }
        // The scene may have spent this display frame admitting models and shadows.
        crate::loading_work::idle().await;
        // The second driver queue (a context sharing the window's) only for a Battlefront II battle: once one has
        // existed the NVIDIA driver runs the window context's calls through its multi-context layer for the rest of
        // the battle, about 0.1 ms a frame on a Kamino battle, and a 2004 battle had none before.
        if second_queue {
            crate::particle_worker::precache(bridge, self.batches.iter().map(|b| b.handle)).await?;
        }
        let batches = self.batches.iter().map(|b| (b.handle, b.at));
        bridge.r.effects.warm_particle_batches(&mut bridge.r.core, target, batches, crate::loading_work::idle).await
    }

    pub fn begin(&mut self, bridge: &mut Bridge) {
        for b in &mut self.batches {
            b.count = 0;
        }
        self.model_rows.clear();
        // The draws that follow the first person arms start again too.
        bridge.r.view.input[bfe_render::view::IN_DRAWS] = 0.;
        bridge.r.effects.input[render::IN_DRAWS] = 0.;
        bridge.r.effects.input[render::IN_OWNERS] = 0.;
        // Hard-edged until `project` hears of the frame's depth copy.
        bridge.r.effects.input[render::IN_SOFT..render::IN_SOFT + 6].copy_from_slice(&[0., 1., 1., 0.3, 20000., -1.]);
    }
    /// Without sun shadows, a drop shadow under each soldier standing at `feet`, as the 2004 game drew one: the
    /// figure image laid flat on the ground with its feet at his and its head away from the sun, as long as a
    /// 1.8 m figure's shadow at the sun's height (between 1.2 m and 2.6 m). `sun` is the sun's direction toward
    /// the scene, unit length.
    pub fn drop_shadows(&mut self, bridge: &mut Bridge, feet: &[[f64; 3]], sun: [f64; 3]) {
        let Some(batch) = self.drop_shadow else { return };
        if feet.is_empty() || bridge.shadow_cascades() > 0 {
            return;
        }
        for values in drop_shadow_quads(feet, sun) {
            self.quad(bridge, batch, &values);
        }
    }
    pub fn quad(&mut self, bridge: &mut Bridge, batch: usize, values: &[f32]) {
        let Some(b) = self.batches.get_mut(batch) else { return };
        if b.analytic || b.full(&mut bridge.r.effects.dropped_particle_rows) {
            return;
        }
        let at = b.at + b.count * b.stride;
        bridge.r.effects.data[at..at + 21].copy_from_slice(values);
        b.add(&mut self.sequence);
    }
    pub fn effects(&mut self, bridge: &mut Bridge, owner: &bfe_presentation::Presentation, indices: &[Option<usize>]) {
        if self.trails.is_empty() {
            self.write_effects::<false>(bridge, owner, indices);
        } else {
            self.write_effects::<true>(bridge, owner, indices);
        }
    }
    fn write_effects<const TRAILS: bool>(&mut self, bridge: &mut Bridge, owner: &bfe_presentation::Presentation, indices: &[Option<usize>]) {
        use bfe_presentation::layout::{draw as d, draw_kind as kind, effect as e, header as h};
        let count = (owner.header[h::EFFECTS] as usize).min(1024);
        for (i, row) in owner.effect_output.chunks_exact(e::STRIDE).take(count).enumerate() {
            let at = i * 12;
            let x = &mut bridge.r.effects.data[at..at + 12];
            for k in 0..3 {
                x[k] = row[e::POSITION + k] as f32;
            }
            x[3] = row[e::TINT] as f32;
            for k in 0..4 {
                x[4 + k] = row[e::QUATERNION + k] as f32;
            }
            let epoch = (row[e::TIME] / 256.).floor() * 256.;
            self.epochs[i] = epoch;
            x[8] = (row[e::TIME] - epoch) as f32;
            x[9] = row[e::SCALE] as f32;
        }
        if count > 0 {
            bridge.r.effects.input[render::IN_OWNERS] = 1.;
            bridge.r.effects.input[render::OWNER_AT..render::OWNER_AT + 4].copy_from_slice(&[self.owner as f64, 1024., count as f64, 0.]);
        }
        let mut previous = None;
        for row in owner.draws.chunks_exact(d::STRIDE).take(owner.header[h::DRAWS] as usize) {
            let definition = row[d::DEFINITION] as usize;
            if TRAILS {
                if self.trails.get(definition) == Some(&true) {
                    let point = trails::Point::read(row);
                    if let (Some(tail), Some(batch)) = (previous.replace(point), self.effect_batches[definition]) {
                        if let Some(quad) = point.connect(tail, row) {
                            self.quad(bridge, batch, &quad);
                        }
                    }
                    continue;
                }
                previous = None;
            }
            // drawModel: the model at the particle, turned by its
            // quaternion (the owner has already spun it about its own up),
            // scaled by its size, in its colour and opacity.
            if row[d::KIND] == kind::MODEL as f64 {
                let Some(glow) = self.effect_glows.get(definition).copied().flatten() else {
                    continue;
                };
                let mut matrix = three::Matrix4::default();
                matrix.compose(
                    &three::Vector3::new(row[d::POSITION], row[d::POSITION + 1], row[d::POSITION + 2]),
                    &three::Quaternion::new(row[d::TURN], row[d::TURN + 1], row[d::TURN + 2], row[d::TURN + 3]),
                    &three::Vector3::new(row[d::SIZE], row[d::SIZE], row[d::SIZE]),
                );
                self.model(bridge, glow, &matrix.elements, &row[d::COLOR..d::COLOR + 3], row[d::COLOR + 3]);
                continue;
            }
            let Some(batch) = self.effect_batches.get(definition).copied().flatten() else {
                continue;
            };
            let b = &mut self.batches[batch];
            if b.full(&mut bridge.r.effects.dropped_particle_rows) {
                continue;
            }
            let at = b.at + b.count * b.stride;
            let x = &mut bridge.r.effects.data[at..at + b.stride];
            if row[d::KIND] == kind::ANALYTIC as f64 {
                let slot = row[d::EFFECT] as usize;
                if slot >= count {
                    continue;
                }
                for k in 0..3 {
                    x[k] = row[d::POSITION + k] as f32;
                    x[3 + k] = row[d::VELOCITY + k] as f32;
                }
                for k in 0..4 {
                    x[6 + k] = row[d::COLOR + k] as f32;
                }
                x[10] = row[d::SIZE] as f32;
                x[11] = row[d::ROTATION] as f32;
                x[12] = row[d::SPIN] as f32;
                x[13] = 0.;
                x[14] = (row[d::BIRTH] - self.epochs[slot]) as f32;
                x[15] = row[d::LIFE] as f32;
                x[16] = slot as f32;
                x[17] = indices.get(definition).copied().flatten().unwrap_or(0) as f32;
            } else {
                for k in 0..3 {
                    x[k] = row[d::POSITION + k] as f32;
                    x[11 + k] = row[d::AXIS + k] as f32;
                    x[14 + k] = row[d::AXIS2 + k] as f32;
                }
                for k in 0..4 {
                    x[3 + k] = row[d::COLOR + k] as f32;
                    x[17 + k] = row[d::FRAME + k] as f32;
                }
                x[7] = row[d::SIZE] as f32;
                x[8] = row[d::ROTATION] as f32;
                x[9] = row[d::SHAPE] as f32;
                x[10] = row[d::LENGTH] as f32;
            }
            b.add(&mut self.sequence);
        }
    }
    // One glow model of the frame: each of its parts at `matrix`, in a
    // colour and an opacity. Returns the model rows it took, none once the
    // frame's rows are spent.
    fn model(&mut self, bridge: &mut Bridge, glow: usize, matrix: &[f64; 16], color: &[f64], opacity: f64) -> std::ops::Range<usize> {
        let first = self.model_rows.len();
        let glow = &self.glows[glow];
        for part in &glow.parts {
            let at = self.model_rows.len();
            if at == MODEL_DRAWS {
                break;
            }
            bridge.r.effects.matrices[at * 16..at * 16 + 16].copy_from_slice(matrix);
            let values = &mut bridge.r.effects.data[self.model_at + at * self.fields.length..][..self.fields.length];
            values.copy_from_slice(&glow.values);
            for k in 0..3 {
                values[self.fields.diffuse + k] = color[k] as f32;
            }
            values[self.fields.opacity] = opacity as f32;
            self.model_rows.push((*part, [matrix[12], matrix[13], matrix[14]]));
        }
        first..self.model_rows.len()
    }
    // The draw row of one model row, sorted by where the model is, as three
    // sorts a transparent mesh.
    fn model_draw(&self, bridge: &mut Bridge, draw: usize, at: usize) {
        let (model, position) = &self.model_rows[at];
        let row = &mut bridge.r.effects.input[render::DRAW_AT + draw * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
        row.fill(0.);
        row[..4].copy_from_slice(&[render::KIND_MODEL as f64, *model as f64, (at * 16) as f64, (self.model_at + at * self.fields.length) as f64]);
        row[render::DRAW_KEY + 2..render::DRAW_KEY + 5].copy_from_slice(position);
        row[render::DRAW_KEY + 5] = *model as f64;
    }
    /// updateFlashes: the muzzle flash model at `matrix`, in the
    /// weapon's flash colour. It is asked for after the frame's bodies are
    /// posed, when the particle draws are already listed, so it adds its
    /// own. False when there is no such model or no room left for it.
    pub fn flash(&mut self, bridge: &mut Bridge, matrix: &[f64; 16], color: &[f64], opacity: f64) -> bool {
        let Some(glow) = self.flash else {
            return false;
        };
        let rows = self.model(bridge, glow, matrix, color, opacity);
        let mut draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        let drawn = !rows.is_empty();
        for at in rows {
            if draws == render::MAX_DRAWS {
                break;
            }
            self.model_draw(bridge, draws, at);
            draws += 1;
        }
        bridge.r.effects.input[render::IN_DRAWS] = draws as f64;
        drawn
    }
    /// FirstPersonView.flash: the muzzle flash on the first person arms'
    /// gun. It is drawn with the arms, after them (the view pass's own
    /// draws), so the gun does not cover it as it covers the world's
    /// effects. False when there is no such model or no room for it.
    pub fn flash_view(&mut self, bridge: &mut Bridge, matrix: &[f64; 16], color: &[f64], opacity: f64) -> bool {
        use bfe_render::view;
        let Some(glow) = self.flash else {
            return false;
        };
        let rows = self.model(bridge, glow, matrix, color, opacity);
        let drawn = !rows.is_empty();
        for at in rows {
            let draws = bridge.r.view.input[view::IN_DRAWS] as usize;
            if draws == view::MAX_DRAWS {
                break;
            }
            let (model, _) = self.model_rows[at];
            let row = &mut bridge.r.view.input[view::IN_FIRST_DRAW + draws * view::DRAW..][..view::DRAW];
            row.fill(0.);
            row[view::D_PROGRAM] = bfe_render::actor_layout::kind::VIEW_MODEL as f64;
            row[view::D_GEOMETRY] = model as f64;
            row[view::D_RANGE] = (at * 16) as f64;
            row[view::D_RANGE + 1] = (self.model_at + at * self.fields.length) as f64;
            bridge.r.view.input[view::IN_DRAWS] = (draws + 1) as f64;
        }
        drawn
    }
    pub fn finish(&self, bridge: &mut Bridge) {
        let mut draws = 0;
        for b in &self.batches {
            if b.count == 0 || draws == render::MAX_DRAWS {
                continue;
            }
            let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
            row.fill(0.);
            row[..6].copy_from_slice(&[render::KIND_BATCH as f64, b.handle as f64, b.count as f64, 0., b.at as f64, b.count as f64]);
            row[render::DRAW_KEY + 1] = 10.;
            row[render::DRAW_KEY + 5] = b.order as f64;
            draws += 1;
        }
        for at in 0..self.model_rows.len() {
            if draws == render::MAX_DRAWS {
                break;
            }
            self.model_draw(bridge, draws, at);
            draws += 1;
        }
        bridge.r.effects.input[render::IN_DRAWS] = draws as f64;
    }
    /// The frame's camera for the model particles: their material reads the
    /// projection matrix (`camera`'s first sixteen values), which is known
    /// only once the frame's view is chosen.
    /// `soft` is the frame's size and its depth copy when the softParticles
    /// setting is on: the particle and arc shaders then
    /// fade against the scene's depth, which they turn into metres with the
    /// lens's near and far planes.
    /// The scene's depth goes over whatever the setting: the steam needs it.
    pub fn project(&self, bridge: &mut Bridge, camera: &[f64], soft: Option<(bool, [i32; 2], u32)>) {
        if let Some((on, size, depth)) = soft {
            let (near, far) = (camera[14] / (camera[10] - 1.), camera[14] / (camera[10] + 1.));
            bridge.r.effects.input[render::IN_SOFT..render::IN_SOFT + 6]
                .copy_from_slice(&[on as u8 as f64, size[0] as f64, size[1] as f64, near, far, depth as f64]);
        }
        for at in 0..self.model_rows.len() {
            let values = &mut bridge.r.effects.data[self.model_at + at * self.fields.length + self.fields.projection..][..16];
            for (slot, value) in values.iter_mut().zip(camera) {
                *slot = *value as f32;
            }
        }
    }
}

/// The flat quads (particle-cpu's shape 2: position, two half axes) of drop shadows at `feet`, lifted 4 cm off the
/// ground they stand on.
fn drop_shadow_quads(feet: &[[f64; 3]], sun: [f64; 3]) -> impl Iterator<Item = [f32; 21]> + '_ {
    let flat = (sun[0] * sun[0] + sun[2] * sun[2]).sqrt();
    let away = if flat > 1e-3 { [sun[0] / flat, sun[2] / flat] } else { [0., 1.] };
    let length = if flat > 1e-3 && sun[1] < 0. { (1.8 * flat / -sun[1]).clamp(1.2, 2.6) } else { 1.2 };
    let side = length / DROP_SHADOW_FIGURE;
    let half = (side / 2.) as f32;
    let centre = (DROP_SHADOW_FEET - 0.5) * side;
    feet.iter().map(move |p| {
        let mut q = [0f32; 21];
        q[0] = (p[0] + away[0] * centre) as f32;
        q[1] = (p[1] + 0.04) as f32;
        q[2] = (p[2] + away[1] * centre) as f32;
        q[3..7].copy_from_slice(&[1., 1., 1., DROP_SHADOW_OPACITY]);
        q[7..11].copy_from_slice(&[1., 0., 0., 2.]);
        // First axis across the figure, second from its head toward its feet (the image's rows grow downward).
        q[11..14].copy_from_slice(&[-away[1] as f32 * half, 0., away[0] as f32 * half]);
        q[14..17].copy_from_slice(&[-away[0] as f32 * half, 0., -away[1] as f32 * half]);
        q[17..21].copy_from_slice(&[0., 0., 1., 1.]);
        q
    })
}

#[cfg(test)]
mod drop_shadow_tests {
    #[test]
    fn a_drop_shadow_lies_flat_with_its_feet_at_the_soldier_and_its_head_away_from_the_sun() {
        // Sun 45 degrees up, shining toward +x: the shadow runs toward +x and is 1.8 m long.
        let sun = [std::f64::consts::FRAC_1_SQRT_2, -std::f64::consts::FRAC_1_SQRT_2, 0.];
        let q: Vec<_> = super::drop_shadow_quads(&[[10., 2., -5.]], sun).collect();
        let q = q[0];
        assert_eq!(q[10], 2., "flat quad");
        assert!((q[1] - 2.04).abs() < 1e-5);
        let side = 1.8 / super::DROP_SHADOW_FIGURE;
        // The feet row (image row 88%, corner y = 0.76) lands on the soldier's feet.
        let feet_x = q[0] as f64 + q[14] as f64 * (2. * super::DROP_SHADOW_FEET - 1.);
        assert!((feet_x - 10.).abs() < 1e-4, "{feet_x}");
        // The head end lies down-sun (+x), the axes are horizontal and the quad is the figure's side long.
        assert!(q[14] < 0. && q[15] == 0. && q[12] == 0.);
        assert!(((q[14].abs() * 2.) as f64 - side).abs() < 1e-4);
        // A sun overhead still leaves a short shadow, and no direction is NaN.
        let q: Vec<_> = super::drop_shadow_quads(&[[0., 0., 0.]], [0., -1., 0.]).collect();
        assert!(q[0].iter().all(|v| v.is_finite()));
    }
}

// Battlefront II culls camera-facing particle sprites inside 1 m of the eye
// and fades their opacity linearly across one full sprite width past that
// plane (`2.0 * size`, since `size` is the quad's half-extent in the shader).
pub(super) fn swbf2_near_fade_vertex(vertex: &str) -> Result<String, String> {
    const ANCHOR: &str = "      mv = modelViewMatrix * vec4(iPosition, 1.0);";
    if vertex.matches(ANCHOR).count() != 1 {
        return Err("particle vertex shader missing camera-facing billboard anchor".into());
    }
    Ok(vertex.replacen(
        ANCHOR,
        "      mv = modelViewMatrix * vec4(iPosition, 1.0);\n\
         float nearDepth = -mv.z - 1.0;\n\
         if (nearDepth <= 0.0) {\n\
           gl_Position = vec4(2.0, 2.0, 2.0, 1.0);\n\
           return;\n\
         }\n\
         float nearSpan = 2.0 * size;\n\
         if (nearSpan > 0.0 && nearDepth < nearSpan) {\n\
           vColor.a *= nearDepth / nearSpan;\n\
         }",
        1,
    ))
}

fn swbf2_particle_program(
    bridge: &mut Bridge,
    spec: &bfe_render::programs::Spec,
) -> Result<u32, String> {
    let (vertex, fragment) = bfe_render::programs::sources(spec);
    let vertex = swbf2_near_fade_vertex(&vertex)?;
    let attributes: Vec<_> = spec.attributes.iter().map(|(n, l)| ((*n).to_owned(), *l)).collect();
    let uniforms: Vec<_> = spec.uniforms.iter().map(|(n, _)| (*n).to_owned()).collect();
    bridge.r.core.add_program_lamps(spec.kind, &vertex, &fragment, &attributes, &uniforms, spec.lamps)
}

#[cfg(test)]
fn near_eye_alpha(depth: f32, half_size: f32) -> f32 {
    let near = depth - 1.0;
    if near <= 0.0 {
        0.0
    } else {
        let span = 2.0 * half_size;
        if span > 0.0 && near < span {
            near / span
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod batch_capacity_tests {
    use super::*;

    #[test]
    fn one_material_accepts_the_whole_mission_pool_and_counts_excess_rows() {
        let mut batch = Batch { handle: 0, analytic: false, at: 0, stride: 21,
            capacity: MISSION_CAPACITY, count: 0, order: 0 };
        let (mut sequence, mut dropped) = (0, 0);
        for _ in 0..MISSION_CAPACITY + 3 {
            if !batch.full(&mut dropped) {
                batch.add(&mut sequence);
            }
        }
        assert_eq!(batch.count, 4096);
        assert_eq!(dropped, 3);
        assert_eq!(sequence, 1);
    }

    #[test]
    fn swbf2_near_fade_rewrites_cpu_and_analytic_particle_shaders() {
        for key in ["effects/particle-cpu", "effects/particle-analytic"] {
            let spec = bfe_render::programs::spec(key).unwrap();
            let (vertex, _) = bfe_render::programs::sources(spec);
            let rewritten = swbf2_near_fade_vertex(&vertex).unwrap();
            assert!(rewritten.contains("float nearDepth = -mv.z - 1.0;"), "{key}");
            assert!(rewritten.contains("float nearSpan = 2.0 * size;"), "{key}");
            bfe_render::shader::rewrite_lamps(&rewritten, bfe_render::shader::Stage::Vertex, spec.kind, spec.lamps)
                .unwrap();
        }
        assert_eq!(near_eye_alpha(0.4, 3.5), 0.0);
        assert_eq!(near_eye_alpha(1.0, 3.5), 0.0);
        assert!((near_eye_alpha(4.5, 3.5) - 0.5).abs() < 1e-6);
        assert_eq!(near_eye_alpha(8.0, 3.5), 1.0);
        assert_eq!(near_eye_alpha(15.0, 3.5), 1.0);
    }

    /// What a lowered stage declares: (inputs, outputs, uniforms) as (type, name) pairs. Lines of
    /// the form `[flat] [precision] type name;` after the qualifier; `varying` is an output in
    /// the vertex stage and an input in the fragment stage, as the shader prefixes define it.
    fn interface(glsl: &str, vertex: bool) -> (Vec<(String, String)>, Vec<(String, String)>, Vec<(String, String)>) {
        let (mut ins, mut outs, mut uniforms) = (Vec::new(), Vec::new(), Vec::new());
        for line in glsl.lines() {
            let line = line.trim().trim_end_matches(';');
            let mut words = line.split_whitespace().filter(|w| !matches!(*w, "flat" | "smooth" | "highp" | "mediump" | "lowp"));
            let (Some(qualifier), Some(ty), Some(name), None) = (words.next(), words.next(), words.next(), words.next()) else {
                continue;
            };
            let pair = (ty.to_string(), name.to_string());
            match qualifier {
                "in" => ins.push(pair),
                "out" => outs.push(pair),
                "varying" if vertex => outs.push(pair),
                "varying" => ins.push(pair),
                "uniform" => uniforms.push(pair),
                _ => {}
            }
        }
        (ins, outs, uniforms)
    }

    /// Lower one stage the way the shader loader does and check what a driver would refuse at
    /// once: the header, a `main`, balanced delimiters, no uniform declared twice.
    fn lower_stage(label: &str, src: &str, vertex: bool, kind: bfe_render::shader::Kind, lamps: bool) -> String {
        use bfe_render::shader::{rewrite_lamps, to_desktop_glsl, Stage};
        let stage = if vertex { Stage::Vertex } else { Stage::Fragment };
        let rewritten = rewrite_lamps(src, stage, kind, lamps).unwrap_or_else(|e| panic!("{label}: rewrite_lamps failed: {e}"));
        let glsl = to_desktop_glsl(&rewritten, false);
        assert!(glsl.starts_with("#version 330 core\n"), "{label}: missing desktop GLSL header");
        assert!(glsl.contains("void main()"), "{label}: missing main entry point");
        let (mut braces, mut parens) = (0i32, 0i32);
        for ch in glsl.chars() {
            match ch {
                '{' => braces += 1,
                '}' => braces -= 1,
                '(' => parens += 1,
                ')' => parens -= 1,
                _ => {}
            }
            assert!(braces >= 0 && parens >= 0, "{label}: unbalanced delimiter");
        }
        assert_eq!((braces, parens), (0, 0), "{label}: unclosed delimiter");
        let (_, _, uniforms) = interface(&glsl, vertex);
        for (i, (_, name)) in uniforms.iter().enumerate() {
            assert!(uniforms[..i].iter().all(|(_, n)| n != name), "{label}: uniform {name} declared twice");
        }
        glsl
    }

    /// A shader program links only when every input of the fragment stage is an output of the vertex
    /// stage with the same type and a uniform both stages name has one type.
    fn assert_stages_link(label: &str, vertex: &str, fragment: &str) {
        let (_, vertex_outs, vertex_uniforms) = interface(vertex, true);
        let (fragment_ins, _, fragment_uniforms) = interface(fragment, false);
        assert!(!fragment_ins.is_empty(), "{label}: the fragment stage reads nothing from the vertex stage");
        for (ty, name) in &fragment_ins {
            assert!(vertex_outs.contains(&(ty.clone(), name.clone())), "{label}: fragment input {ty} {name} has no matching vertex output");
        }
        for (ty, name) in &fragment_uniforms {
            if let Some((other, _)) = vertex_uniforms.iter().find(|(_, n)| n == name) {
                assert_eq!(other, ty, "{label}: uniform {name} has two types");
            }
        }
    }

    #[test]
    fn pixel_tested_shader_sources_lower_and_link_without_gl() {
        use bfe_render::shader::Kind;
        let cpu_spec = bfe_render::programs::spec("effects/particle-cpu").unwrap();
        let (cpu_vs, _) = bfe_render::programs::sources(cpu_spec);
        let refract_vs = refraction::refraction_vertex(&cpu_vs).unwrap();
        let refract_vs_glsl = lower_stage("refraction/vs", &refract_vs, true, Kind::Effect, false);
        // The refraction fragment stage is a source of its own, without the catalog's prefix.
        let refract_fs_glsl = lower_stage("refraction/fs", include_str!("particles/refraction.frag"), false, Kind::Effect, false);
        assert_stages_link("refraction", &refract_vs_glsl, &refract_fs_glsl);
        assert!(refract_vs_glsl.contains("uniform vec2 blurSettings;"));
        assert!(refract_vs_glsl.contains("refractShift = offset * (2.0 * blurSettings.x);"));
        assert!(refract_fs_glsl.contains("uniform sampler2D sceneColor;"));

        for key in ["effects/particle-cpu", "effects/particle-analytic"] {
            let spec = bfe_render::programs::spec(key).unwrap();
            let (vs_src, fs_src) = bfe_render::programs::sources(spec);
            let near_vs = swbf2_near_fade_vertex(&vs_src).unwrap();
            let vs = lower_stage(&format!("{key}/vs"), &near_vs, true, spec.kind, spec.lamps);
            let fs = lower_stage(&format!("{key}/fs"), &fs_src, false, spec.kind, spec.lamps);
            assert_stages_link(key, &vs, &fs);
            assert!(vs.contains("float nearDepth = -mv.z - 1.0;"), "{key}");
            assert!(vs.contains("float nearSpan = 2.0 * size;"), "{key}");
        }
    }
}
