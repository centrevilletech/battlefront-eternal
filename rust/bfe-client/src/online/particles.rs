//! Native particle resources share EffectsPass's transparent list. Both the
//! ordnance owner and effect owner publish into these same retained batches.
use crate::{render_bridge::Bridge, resources::Resources};
use bfe_content_native::BattleContent;
use bfe_render::effects as render;
use serde_json::json;
use std::collections::BTreeMap;
const CAPACITY: usize = 1024;
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
        let mut prepared = Self { recipes: vec![], keys: BTreeMap::new(), effect_batches: vec![], textures };
        if let Some((_, effects)) = content.effects.iter().find(|(n, _)| n == "battle") {
            for d in effects.descriptor["definitions"].as_array().into_iter().flatten() {
                let name = d["geometry"]["texture"].as_str().unwrap_or("");
                let batch =
                    prepared.textures.get(name).copied().map(|image| {
                        prepared.batch(image, d["gpu"].as_bool().unwrap_or(false), false, d["geometry"]["blend"] == "additive")
                    });
                prepared.effect_batches.push(batch);
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
            batches.push(Batch { handle, analytic: recipe.analytic, at, stride, count: 0 });
        }
        bridge.fx_reserve(words as u32);
        Ok(Particles { batches, effect_batches: self.effect_batches, owner, words, epochs: vec![0.; 1024] })
    }
}
struct Batch {
    handle: u32,
    analytic: bool,
    at: usize,
    stride: usize,
    count: usize,
}
pub struct Particles {
    batches: Vec<Batch>,
    pub effect_batches: Vec<Option<usize>>,
    owner: u32,
    pub words: usize,
    epochs: Vec<f64>,
}
impl Particles {
    pub fn begin(&mut self, bridge: &mut Bridge) {
        for b in &mut self.batches {
            b.count = 0;
        }
        bridge.r.effects.input[render::IN_DRAWS] = 0.;
        bridge.r.effects.input[render::IN_OWNERS] = 0.;
        bridge.r.effects.input[render::IN_SOFT..render::IN_SOFT + 6].copy_from_slice(&[0., 1., 1., 0.3, 20000., -1.]);
    }
    pub fn quad(&mut self, bridge: &mut Bridge, batch: usize, values: &[f32]) {
        let Some(b) = self.batches.get_mut(batch) else { return };
        if b.analytic || b.count == CAPACITY {
            return;
        }
        let at = b.at + b.count * b.stride;
        bridge.r.effects.data[at..at + 21].copy_from_slice(values);
        b.count += 1;
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
            b.count += 1;
        }
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
            row[render::DRAW_KEY + 5] = b.handle as f64;
            draws += 1;
        }
        bridge.r.effects.input[render::IN_DRAWS] = draws as f64;
    }
}
