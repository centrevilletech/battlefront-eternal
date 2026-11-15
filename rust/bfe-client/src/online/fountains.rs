//! The Enhanced fountains (fountain-jets.js FountainJets, as
//! environment-effects.js drives it): each fountain in view, out to 90 m,
//! throws its jets on the GPU from fixed seeds and the clock alone. Far to
//! near: the pools' rings and foam, the tier's overflow, the soft spray
//! (drawn at half size into a texture of its own and laid over the frame),
//! the ropes of water, the sharp drops and splashes, the core column, then
//! the near halves of the same. The jets refract the frame drawn so far,
//! copied once just before the first of them. Classic plays the fountains'
//! stock effect instead (map_effects.rs). The basin water's own Enhanced
//! surface (fountain-water.js) and the wading splashes are not drawn here.
use crate::render_bridge::Bridge;
use bfe_content_native::BattleContent;
use bfe_render::{effect_layout as layout, effects as render};

/// One fountain in view: its slot among the map's fountains, where its
/// nozzle is, and the levels of its tier's water and its outer pool.
#[derive(Clone, Copy)]
pub struct Fountain {
    pub slot: usize,
    pub position: [f64; 3],
    pub level: f64,
    pub low: f64,
}

const FOUNTAINS: usize = 5;
// The jets' shared values: time, sources, levels, lows, visible, hasDepth,
// hasScene, sceneResolution, fragScale.
const VALUES: usize = 1 + 20 + 5 + 5 + 5 + 1 + 1 + 2 + 2;
// TIER.rise and TIER.outer: the inner tier is filled to its lip, the outer
// pool sits a little under the basin's authored level.
const RISE: f64 = 0.797;
const OUTER: f64 = -0.05;

// One of FountainJets' meshes: its batch, and whether it is the soft spray
// (drawn into the half-size target, then composited).
struct Mesh {
    batch: u32,
    spray: bool,
}

pub struct Fountains {
    meshes: Vec<Mesh>,
    composite: u32,
    // The frame copied for the jets to refract, and the soft spray's
    // half-size target: source, sample, registered texture.
    behind: (u32, u32, u32),
    spray: (u32, u32, u32),
    sizes: [[i32; 2]; 2],
    at: usize,
    time: f64,
    /// Frames the fountains were drawn.
    pub frames: u64,
}

fn target_desc(size: [i32; 2]) -> String {
    serde_json::json!({"width":size[0],"height":size[1],"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1009,"srgb":false,"renderTarget":true}).to_string()
}

fn texture(content: &mut BattleContent, textures: &[u32], name: &str) -> i32 {
    content
        .texture_paths
        .get(name)
        .cloned()
        .and_then(|path| content.textures.load(&path).ok())
        .and_then(|image| textures.get(image).copied())
        .map_or(-1, |t| t as i32)
}

impl Fountains {
    /// None on a map without fountains.
    pub fn new(content: &mut BattleContent, bridge: &mut Bridge, textures: &[u32], gpu: &mut crate::resources::Resources, at: usize) -> Result<Option<Self>, String> {
        if content.environment_basins.is_empty() || !content.environment_instances.iter().any(|i| i.effect.ends_with("/waterfountain")) {
            return Ok(None);
        }
        // FountainJets.sprayAtlas: the aerated water's normals, height and
        // density, mipmapped.
        let atlas_source = gpu.source(&serde_json::json!({"width":256,"height":128,"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1009,"srgb":false}).to_string())?;
        gpu.source_pixels(atlas_source, &bfe_render::fountain_resources::atlas::<bfe_render::pixel_math::V8>())?;
        let atlas_sample = gpu.sample(
            atlas_source,
            r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1008,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":4,"mipmaps":true,"compare":null}"#,
        )?;
        gpu.upload(atlas_sample)?;
        let atlas = bridge.r.core.add_texture(gpu.texture(atlas_sample)?) as i32;
        let foam = texture(content, textures, "waterfoam1");
        let target = |gpu: &mut crate::resources::Resources, bridge: &mut Bridge, mipmaps: bool| -> Result<(u32, u32, u32), String> {
            let source = gpu.source(&target_desc([1, 1]))?;
            let sample = gpu.sample(
                source,
                &format!(
                    r#"{{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":{},"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":4,"mipmaps":{mipmaps},"compare":null}}"#,
                    if mipmaps { 1008 } else { 1006 }
                ),
            )?;
            gpu.upload(sample)?;
            Ok((source, sample, bridge.r.core.add_texture(gpu.texture(sample)?)))
        };
        let behind = target(gpu, bridge, true)?;
        let spray = target(gpu, bridge, false)?;
        let samplers = [atlas, foam, behind.2 as i32];
        let shadows = bridge.shadow_cascades() > 0;
        // Variant bits: 1 the near half, 2 the tier, groups of particles in
        // bits 2-3 (0 the pools, 1 the soft spray, 2 the sharp drops).
        const TIER: u32 = 2;
        let mut make = |kind: usize, variant: u32| bridge.effect_batch(kind as u32, variant, shadows, -1, 1, -1, &samplers);
        let order = [
            (layout::JET_PARTICLES, TIER, false),
            (layout::JET_FILM, TIER, false),
            (layout::JET_PARTICLES, 1 << 2 | TIER, true),
            (layout::JET_ROPES, TIER, false),
            (layout::JET_PARTICLES, 2 << 2 | TIER, false),
            (layout::JET_CORE, TIER, false),
            (layout::JET_PARTICLES, 2 << 2 | TIER | 1, false),
            (layout::JET_FILM, TIER | 1, false),
            (layout::JET_ROPES, TIER | 1, false),
            (layout::JET_PARTICLES, 1 << 2 | TIER | 1, true),
        ];
        let mut meshes = Vec::new();
        for (kind, variant, spray) in order {
            meshes.push(Mesh { batch: make(kind, variant)?, spray });
        }
        let composite = bridge.effect_batch(layout::JET_COMPOSITE as u32, 0, shadows, -1, 1, -1, &[spray.2 as i32])?;
        let fountains = Self { meshes, composite, behind, spray, sizes: [[0, 0]; 2], at, time: 0., frames: 0 };
        bridge.fx_reserve(fountains.end() as u32);
        Ok(Some(fountains))
    }

    pub fn end(&self) -> usize {
        self.at + VALUES + 3
    }

    fn resize(target: (u32, u32, u32), size: [i32; 2], gpu: &mut crate::resources::Resources, bridge: &mut Bridge) {
        if gpu.source_desc(target.0, &target_desc(size)).is_ok() && gpu.upload(target.1).is_ok() {
            if let Ok(texture) = gpu.texture(target.1) {
                let _ = bridge.r.core.replace_sample_texture(target.2 as usize, texture);
            }
        }
    }

    /// FountainJets.update and its draws for the fountains in view.
    pub fn frame(&mut self, dt: f64, fountains: &[Fountain], screen: [i32; 2], gpu: &mut crate::resources::Resources, bridge: &mut Bridge) {
        self.time += dt.clamp(0., 0.1);
        let shown = fountains.iter().filter(|f| f.slot < FOUNTAINS).count();
        if shown == 0 {
            return;
        }
        let half = [((screen[0] + 1) / 2).max(1), ((screen[1] + 1) / 2).max(1)];
        if self.sizes[0] != screen {
            self.sizes[0] = screen;
            Self::resize(self.behind, screen, gpu, bridge);
        }
        if self.sizes[1] != half {
            self.sizes[1] = half;
            Self::resize(self.spray, half, gpu, bridge);
        }
        let mut v = [0f32; VALUES];
        v[0] = self.time as f32;
        let mut k = 0;
        for f in fountains.iter().filter(|f| f.slot < FOUNTAINS) {
            for i in 0..3 {
                v[1 + f.slot * 4 + i] = f.position[i] as f32;
            }
            v[1 + f.slot * 4 + 3] = 1.;
            v[21 + f.slot] = f.level as f32;
            v[26 + f.slot] = f.low as f32;
            v[31 + k] = f.slot as f32;
            k += 1;
        }
        v[36] = 1.;
        v[37] = 1.;
        v[38] = screen[0] as f32;
        v[39] = screen[1] as f32;
        v[40] = screen[0] as f32 / half[0] as f32;
        v[41] = screen[1] as f32 / half[1] as f32;
        let at = self.at;
        bridge.r.effects.data[at..at + VALUES].copy_from_slice(&v);
        bridge.r.effects.data[at + VALUES..at + VALUES + 3].copy_from_slice(&[half[0] as f32, half[1] as f32, 1.]);
        let mut draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        let mut draw = |bridge: &mut Bridge, values: [f64; 6], order: f64, id: f64| {
            if draws == render::MAX_DRAWS {
                return;
            }
            let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
            row.fill(0.);
            row[..6].copy_from_slice(&values);
            row[render::DRAW_KEY + 1] = order;
            row[render::DRAW_KEY + 5] = id;
            draws += 1;
        };
        // FountainJets.capture: the frame so far, once, before the first jet.
        draw(bridge, [render::KIND_COPY as f64, self.behind.2 as f64, screen[0] as f64, screen[1] as f64, 0., 0.], 8.99, self.behind.2 as f64);
        let room = (1usize << 20) as f64;
        let packed = (half[0] as f64) * 65536. + half[1] as f64;
        for (i, mesh) in self.meshes.iter().enumerate() {
            let order = 9. + i as f64 * 0.05;
            if mesh.spray {
                draw(bridge, [render::KIND_INTO as f64, mesh.batch as f64, shown as f64, at as f64, self.spray.2 as f64, packed], order - 0.01, mesh.batch as f64);
                draw(bridge, [render::KIND_BATCH as f64, self.composite as f64, 1., 0., (at + VALUES) as f64, 1.], order, self.composite as f64 + i as f64 * 1e-3);
            } else {
                draw(bridge, [render::KIND_BATCH as f64, mesh.batch as f64, shown as f64, 0., at as f64, room], order, mesh.batch as f64);
            }
        }
        bridge.r.effects.input[render::IN_DRAWS] = draws as f64;
        self.frames += 1;
    }
}

/// The levels environment-effects.js gives a fountain from its nearest
/// basin: the tier filled to its lip, the outer pool just under the basin.
pub fn levels(content: &BattleContent, position: [f64; 3]) -> (f64, f64) {
    let distance = |b: &[f64; 4]| (b[0] - position[0]).hypot(b[2] - position[2]);
    let water = content
        .environment_basins
        .iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .map_or(position[1] - 0.2, |b| b[1]);
    (water + RISE, water + OUTER)
}
