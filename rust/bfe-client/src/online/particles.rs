//! Native particle resources share EffectsPass's transparent list. Both the
//! ordnance owner and effect owner publish into these same retained batches.
use crate::{render_bridge::Bridge, resources::Resources};
use bfe_content_native::BattleContent;
use bfe_render::effects as render;
use serde_json::json;
use std::collections::BTreeMap;
const CAPACITY: usize = 1024;
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
}
impl Prepared {
    pub fn new(content: &mut BattleContent) -> Result<Self, String> {
        let defs = bfe_content::defs::Defs::build(&content.content)?;
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
        let mut prepared = Self { recipes: vec![], keys: BTreeMap::new(), effect_batches: vec![], effect_models: vec![], textures };
        if let Some((_, effects)) = content.effects.iter().find(|(n, _)| n == "battle") {
            for d in effects.descriptor["definitions"].as_array().into_iter().flatten() {
                let name = d["geometry"]["texture"].as_str().unwrap_or("");
                let batch =
                    prepared.textures.get(name).copied().map(|image| {
                        prepared.batch(image, d["gpu"].as_bool().unwrap_or(false), false, d["geometry"]["blend"] == "additive")
                    });
                prepared.effect_batches.push(batch);
                // effects.js: a GEOMETRY particle draws its model, never a quad.
                let model = (d["geometry"]["type"] == "geometry").then(|| d["geometry"]["model"].as_str()).flatten();
                prepared.effect_models.push(model.map(str::to_string));
            }
        }
        Ok(prepared)
    }
    // effects.js batches by geometry layout and blend state as well as map.
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
        let effects = content.effects.iter().find(|(n, _)| n == "battle").map(|(_, e)| e).ok_or("missing effect library")?;
        let bytes = effects.gpu_tracks.iter().flat_map(|n| n.to_le_bytes()).collect::<Vec<_>>();
        let source=resources.source(&json!({"width":effects.gpu_size[0],"height":effects.gpu_size[1],"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1015,"srgb":false}).to_string())?;
        resources.source_pixels(source, &bytes)?;
        let sample=resources.sample(source,r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#)?;
        resources.upload(sample)?;
        let tracks = bridge.r.core.add_texture(resources.texture(sample)?);
        let owner = bridge.fx_add_owner(tracks, effects.gpu_size[1] as f32);
        bridge.r.effects.prepare_owner(&mut bridge.r.core, owner, 1024);
        let mut batches = Vec::new();
        let mut words = 1024 * 12;
        for recipe in self.recipes {
            let key = if recipe.analytic {
                "effects/particle-analytic"
            } else if recipe.bolt {
                "effects/particle-bolt"
            } else {
                "effects/particle-cpu"
            };
            let program = bridge.add_owned_program(key)?;
            let spec = bfe_render::programs::spec(key)?;
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
            bridge.r.effects.prepare_batch(&bridge.r.core, handle, CAPACITY);
            let stride = if recipe.analytic { 18 } else { 21 };
            let at = words;
            words += CAPACITY * stride;
            batches.push(Batch { handle, analytic: recipe.analytic, at, stride, count: 0, order: 0 });
        }
        // game.js glowModel: each model a GEOMETRY definition names, drawn
        // additive and double-sided with its first part's texture, tinted
        // by the particle. One renderer model serves every particle of a
        // part; a model the content lacks draws nothing.
        let shadows = crate::shadows::COUNT > 0;
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
        // game.js hands combat `glowModel("muzzleflash")` for the flash at a
        // held gun's muzzle: the same kind of model, listed last here.
        let flash = Some(FLASH.to_string());
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
            let meshes: Vec<_> = content.models.iter().filter(|m| m.name.starts_with(&prefix)).collect();
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
            batches,
            sequence: 0,
            effect_batches: self.effect_batches,
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
    count: usize,
    // Its place among the batches by first use, from one; zero until then.
    order: u32,
}
impl Batch {
    // particles.js ParticleRenderer.batch makes a batch the first time a
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
    batches: Vec<Batch>,
    sequence: u32,
    pub effect_batches: Vec<Option<usize>>,
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
    pub fn begin(&mut self, bridge: &mut Bridge) {
        for b in &mut self.batches {
            b.count = 0;
        }
        self.model_rows.clear();
        bridge.r.effects.input[render::IN_DRAWS] = 0.;
        bridge.r.effects.input[render::IN_OWNERS] = 0.;
        // Hard-edged until `project` hears of the frame's depth copy.
        bridge.r.effects.input[render::IN_SOFT..render::IN_SOFT + 6].copy_from_slice(&[0., 1., 1., 0.3, 20000., -1.]);
    }
    pub fn quad(&mut self, bridge: &mut Bridge, batch: usize, values: &[f32]) {
        let Some(b) = self.batches.get_mut(batch) else { return };
        if b.analytic || b.count == CAPACITY {
            return;
        }
        let at = b.at + b.count * b.stride;
        bridge.r.effects.data[at..at + 21].copy_from_slice(values);
        b.add(&mut self.sequence);
    }
    pub fn effects(&mut self, bridge: &mut Bridge, owner: &bfe_presentation::Presentation, indices: &[Option<usize>]) {
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
        for row in owner.draws.chunks_exact(d::STRIDE).take(owner.header[h::DRAWS] as usize) {
            let definition = row[d::DEFINITION] as usize;
            // effects.js drawModel: the model at the particle, turned by its
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
            if b.count == CAPACITY {
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
    /// combat.js updateFlashes: the muzzle flash model at `matrix`, in the
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
    /// setting is on (particle-depth.js): the particle and arc shaders then
    /// fade against the scene's depth, which they turn into metres with the
    /// lens's near and far planes.
    pub fn project(&self, bridge: &mut Bridge, camera: &[f64], soft: Option<([i32; 2], u32)>) {
        if let Some((size, depth)) = soft {
            let (near, far) = (camera[14] / (camera[10] - 1.), camera[14] / (camera[10] + 1.));
            bridge.r.effects.input[render::IN_SOFT..render::IN_SOFT + 6]
                .copy_from_slice(&[1., size[0] as f64, size[1] as f64, near, far, depth as f64]);
        }
        for at in 0..self.model_rows.len() {
            let values = &mut bridge.r.effects.data[self.model_at + at * self.fields.length + self.fields.projection..][..16];
            for (slot, value) in values.iter_mut().zip(camera) {
                *slot = *value as f32;
            }
        }
    }
}
