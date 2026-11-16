//! The Enhanced fountains (fountain-jets.js FountainJets, as
//! environment-effects.js drives it): each fountain in view, out to 90 m,
//! throws its jets on the GPU from fixed seeds and the clock alone. Far to
//! near: the pools' rings and foam, the tier's overflow, the soft spray
//! (drawn at half size into a texture of its own and laid over the frame),
//! the ropes of water, the sharp drops and splashes, the core column, then
//! the near halves of the same. The jets refract the frame drawn so far,
//! copied once just before the first of them. Classic plays the fountains'
//! stock effect instead (map_effects.rs). The basins' Enhanced water
//! (FountainWater) is drawn here too: the tier filled to its
//! lip and the outer pool, reading a copy of the opaque scene for what shows
//! through, with foam, caustics, the plume's shadow and reflection, and rings
//! where someone wades (the environment's wakes).
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
// The outer pool's disc: sides, and its lift over the stock basin water.
const POOL_SIDES: usize = 32;
const POOL_LIFT: f64 = 0.01;

// One of FountainJets' meshes: its batch, and whether it is the soft spray
// (drawn into the half-size target, then composited).
struct Mesh {
    batch: u32,
    spray: bool,
}

// FountainWater: the scene copy it reads, its two models (the outer pool's
// disc, then the tier) with their centres, and where its values go.
struct Basins {
    copy: crate::water_host::SceneCopy,
    size: [i32; 2],
    models: [(u32, [f64; 3]); 2],
    centres: [[f64; 4]; FOUNTAINS],
    foam: bool,
    at: usize,
    matrices: usize,
    // Drawn this frame: the frame copies the opaque scene for it.
    copying: bool,
    // FountainSpray: its batch and seeds (eight wakes of 24 splashes).
    spray: u32,
    seeds: Vec<f32>,
}
// FountainWater's values (effect_layout FOUNTAIN_WATER).
const BASIN_VALUES: usize = 100;
// FountainSpray: 8 wakes x 24 splashes, then sprayTime, sources, wakes,
// motion and hasDepth.
const SPLASHES: usize = 8 * 24;
const SPRAY_VALUES: usize = 1 + 20 + 32 + 32 + 1;

pub struct Fountains {
    basins: Option<Basins>,
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
        // FountainWater: the basins (the map's first five), the tier's pools
        // filled to their lips, and the outer pools as discs at the basins'
        // level over the stock water, both reading the copied scene.
        let copy = crate::water_host::scene_copy(gpu, bridge, [1, 1])?;
        let mut centres = [[1e6, 1e6, 1e6, 0.]; FOUNTAINS];
        for (slot, b) in centres.iter_mut().zip(&content.environment_basins) {
            *slot = *b;
        }
        let flat: Vec<f64> = content.environment_basins.iter().take(FOUNTAINS).flatten().copied().collect();
        let samplers = [-1, copy.1[2] as i32, copy.2[2] as i32, foam];
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        for b in content.environment_basins.iter().take(FOUNTAINS) {
            let base = vertices.len() as u32 / 3;
            vertices.extend([b[0] as f32, (b[1] + POOL_LIFT) as f32, b[2] as f32]);
            for k in 0..=POOL_SIDES {
                let a = k as f64 / POOL_SIDES as f64 * std::f64::consts::TAU;
                vertices.extend([(b[0] + a.cos() * b[3]) as f32, (b[1] + POOL_LIFT) as f32, (b[2] + a.sin() * b[3]) as f32]);
            }
            for k in 1..=POOL_SIDES as u32 {
                indices.extend([base, base + k + 1, base + k]);
            }
        }
        let model = |r: Vec<f64>| (r[0] as u32, [r[1], r[2], r[3]]);
        let outer = model(bridge.effect_basin(false, &flat, &vertices, &indices, true, shadows, &samplers)?);
        let tier = model(bridge.effect_basin(true, &flat, &[], &[], false, shadows, &samplers)?);
        let spray_batch = bridge.effect_batch(layout::SPRAY as u32, 0, shadows, -1, SPLASHES as u32, -1, &[])?;
        // FountainSpray's seeds: the wake, then three draws of its stream.
        let mut state: u32 = 137;
        let mut random = || {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            state as f64 / 4294967296.
        };
        let mut seeds = Vec::with_capacity(SPLASHES * 4);
        for i in 0..SPLASHES {
            seeds.push((i / 24) as f32);
            for _ in 0..3 {
                seeds.push(random() as f32);
            }
        }
        let matrices = bfe_render::effects::MAX_DRAWS
            - 2 * super::shields::BUBBLES - super::holograms::MATRICES - super::blades::MATRICES - 2;
        let basins = Basins { copy, size: [1, 1], models: [outer, tier], centres, foam: foam >= 0, at: at + VALUES + 3, matrices, copying: false, spray: spray_batch, seeds };
        let fountains = Self { basins: Some(basins), meshes, composite, behind, spray, sizes: [[0, 0]; 2], at, time: 0., frames: 0 };
        bridge.fx_reserve(fountains.end() as u32);
        Ok(Some(fountains))
    }

    /// The basins' scene copy this frame (its target and size), when they
    /// are drawn: the frame copies the opaque scene's colour and depth into
    /// it before the transparent draws.
    pub fn scene_copy(&self) -> Option<(u32, [i32; 2])> {
        self.basins.as_ref().filter(|b| b.copying).map(|b| (b.copy.0, b.size))
    }
    pub fn end(&self) -> usize {
        self.at + VALUES + 3 + BASIN_VALUES + SPLASHES * 4 + SPRAY_VALUES
    }

    fn resize(target: (u32, u32, u32), size: [i32; 2], gpu: &mut crate::resources::Resources, bridge: &mut Bridge) {
        if gpu.source_desc(target.0, &target_desc(size)).is_ok() && gpu.upload(target.1).is_ok() {
            if let Ok(texture) = gpu.texture(target.1) {
                let _ = bridge.r.core.replace_sample_texture(target.2 as usize, texture);
            }
        }
    }

    /// FountainJets.update and its draws for the fountains in view.
    pub fn frame(&mut self, dt: f64, fountains: &[Fountain], wakes: &super::map_effects::Wakes, camera: &[f64], screen: [i32; 2], gpu: &mut crate::resources::Resources, bridge: &mut Bridge) {
        self.time += dt.clamp(0., 0.1);
        let shown = fountains.iter().filter(|f| f.slot < FOUNTAINS).count();
        if let Some(basins) = &mut self.basins {
            basins.copying = false;
        }
        if shown == 0 {
            return;
        }
        if let Some(basins) = &mut self.basins {
            Self::basins_frame(basins, wakes, camera, screen, gpu, bridge);
            Self::spray_frame(basins, fountains, wakes, bridge);
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

impl Fountains {
    /// FountainSpray.update: short splashes where a character wades into a
    /// visible fountain's pools, at render order 11.
    fn spray_frame(basins: &Basins, fountains: &[Fountain], wakes: &super::map_effects::Wakes, bridge: &mut Bridge) {
        let at = basins.at + BASIN_VALUES;
        let data = &mut bridge.r.effects.data;
        data[at..at + SPLASHES * 4].copy_from_slice(&basins.seeds);
        let v = &mut data[at + SPLASHES * 4..at + SPLASHES * 4 + SPRAY_VALUES];
        v.fill(0.);
        v[0] = wakes.time as f32;
        for f in fountains.iter().filter(|f| f.slot < FOUNTAINS) {
            for k in 0..3 {
                v[1 + f.slot * 4 + k] = f.position[k] as f32;
            }
            v[1 + f.slot * 4 + 3] = 1.;
        }
        for k in 0..8 {
            for i in 0..4 {
                v[21 + k * 4 + i] = wakes.points[k][i] as f32;
                v[53 + k * 4 + i] = wakes.motion[k][i] as f32;
            }
        }
        v[SPRAY_VALUES - 1] = 1.;
        let draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        if draws < render::MAX_DRAWS {
            let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
            row.fill(0.);
            row[..6].copy_from_slice(&[render::KIND_BATCH as f64, f64::from(basins.spray), SPLASHES as f64, 0., at as f64, SPLASHES as f64]);
            row[render::DRAW_KEY + 1] = 11.;
            row[render::DRAW_KEY + 5] = f64::from(basins.spray);
            bridge.r.effects.input[render::IN_DRAWS] = (draws + 1) as f64;
        }
    }
    /// FountainWater.update and capture, and the basins' draws.
    fn basins_frame(basins: &mut Basins, wakes: &super::map_effects::Wakes, camera: &[f64], screen: [i32; 2], gpu: &mut crate::resources::Resources, bridge: &mut Bridge) {
        use bfe_render::{cameras::layout as cl, shader};
        if basins.size != screen && crate::water_host::scene_copy_resize(gpu, bridge, &basins.copy, screen).is_ok() {
            basins.size = screen;
        }
        basins.copying = true;
        let member = |name: &str| shader::FRAME_MEMBERS.iter().position(|(n, _)| *n == name).map(|i| shader::FRAME_OFFSETS[i]);
        let frame = |name: &str, k: usize| member(name).and_then(|at| camera.get(at + k).copied()).unwrap_or(0.);
        let lens = &bridge.r.cameras.input[cl::input::LENS..cl::input::LENS + cl::lens::LEN];
        let range = [lens[cl::lens::NEAR], lens[cl::lens::FAR]];
        let fields = layout::PRODUCERS[layout::FOUNTAIN_WATER].1;
        let data = &mut bridge.r.effects.data;
        let mut put = |name: &str, value: &[f64]| {
            if let Some((offset, size)) = bfe_render::water_resources::field(fields, name) {
                for (to, from) in data[basins.at + offset..basins.at + offset + size].iter_mut().zip(value) {
                    *to = *from as f32;
                }
            }
        };
        put("waterTime", &[wakes.time]);
        // A bright overcast stand-in for the sky, from the ambient top.
        put("sky", &[(frame("ambientTop", 0) * 1.4 + 0.15).min(1.), (frame("ambientTop", 1) * 1.4 + 0.16).min(1.), (frame("ambientTop", 2) * 1.4 + 0.2).min(1.)]);
        put("waterResolution", &[f64::from(screen[0]), f64::from(screen[1])]);
        put("cameraRange", &range);
        put("waterSunDirection", &[frame("sunDirection", 0), frame("sunDirection", 1), frame("sunDirection", 2)]);
        put("waterSunColor", &[frame("sunColor", 0), frame("sunColor", 1), frame("sunColor", 2)]);
        put("wakes", wakes.points.as_flattened());
        put("motion", wakes.motion.as_flattened());
        put("basins", basins.centres.as_flattened());
        put("hasTier", &[1.]);
        put("hasFoam", &[f64::from(u8::from(basins.foam))]);
        const IDENTITY: [f64; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
        let mut draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        for (k, (model, centre)) in basins.models.iter().enumerate() {
            if draws == render::MAX_DRAWS {
                break;
            }
            let matrix = (basins.matrices + k) * 16;
            bridge.r.effects.matrices[matrix..matrix + 16].copy_from_slice(&IDENTITY);
            let entry = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
            entry.fill(0.);
            entry[..4].copy_from_slice(&[render::KIND_MODEL as f64, f64::from(*model), matrix as f64, basins.at as f64]);
            // The outer pool where the stock basin water was (-17), the tier over it.
            entry[render::DRAW_KEY + 1] = if k == 0 { -17. } else { -16.9 };
            entry[render::DRAW_KEY + 2..render::DRAW_KEY + 5].copy_from_slice(centre);
            entry[render::DRAW_KEY + 5] = f64::from(*model);
            draws += 1;
        }
        bridge.r.effects.input[render::IN_DRAWS] = draws as f64;
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
