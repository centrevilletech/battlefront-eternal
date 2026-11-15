//! The Enhanced steam of the maps' vents (environment-effects.js with
//! EnvironmentVolume and SteamWisps): each vent in view, out to 90 m, is a
//! ray-marched box of rolling vapour drawn at half the screen's size into a
//! texture of its own, then laid over the scene by a depth-aware composite;
//! sparse wisps curl over it on maps without fountains. The player's wakes
//! (map_effects.rs Wakes) part both. Classic plays the vents' stock effect
//! instead (map_effects.rs).
use crate::render_bridge::Bridge;
use bfe_content_native::BattleContent;
use bfe_render::{effect_layout as layout, effects as render};

/// One vent in view: where it is, the way it blows, its box and kind
/// (0 a grate, 1 a pipe).
#[derive(Clone, Copy)]
pub struct Volume {
    pub position: [f64; 3],
    pub direction: [f64; 3],
    pub extent: [f64; 3],
    pub kind: f64,
}

const VOLUMES: usize = 32;
const WISP_SOURCES: usize = 9;
const WISPS: usize = WISP_SOURCES * 16;
// The volume's values: time, resolution, hasDepth, 8 wakes, 8 motions,
// the reflection's matrix and whether there is one.
const VOLUME_VALUES: usize = 1 + 2 + 1 + 32 + 32 + 16 + 1;
const WISP_VALUES: usize = 1 + 36 + 36 + 32 + 32 + 1;

pub struct Steam {
    volume: u32,
    composite: u32,
    wisps: Option<u32>,
    wisp_seeds: Vec<f32>,
    // The half-size target: its source, sample and registered texture, and
    // its size now.
    target: (u32, u32, u32),
    size: [i32; 2],
    at: usize,
    time: f64,
    /// Frames the steam was drawn.
    pub frames: u64,
}

impl Steam {
    /// None on a map without steam vents. `at`: where its floats start.
    pub fn new(content: &mut BattleContent, bridge: &mut Bridge, textures: &[u32], gpu: &mut crate::resources::Resources, at: usize) -> Result<Option<Self>, String> {
        let steam = content.environment_instances.iter().any(|i| is_steam(&i.effect));
        if !steam {
            return Ok(None);
        }
        let fountains = content.environment_instances.iter().any(|i| i.effect.ends_with("/waterfountain"));
        let shadows = bridge.shadow_cascades() > 0;
        // EnvironmentVolume.noiseTexture: 32 cubed bytes, repeating, linear.
        let noise_source = gpu.source(
            &serde_json::json!({"width":32,"height":32,"depth":32,"target":glow::TEXTURE_3D,"format":1028,"dataType":1009,"srgb":false}).to_string(),
        )?;
        gpu.source_pixels(noise_source, &bfe_render::effect_resources::steam_noise())?;
        let noise_sample = gpu.sample(
            noise_source,
            r#"{"wrapS":1000,"wrapT":1000,"wrapR":1000,"min":1006,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#,
        )?;
        gpu.upload(noise_sample)?;
        let noise = bridge.r.core.add_texture(gpu.texture(noise_sample)?);
        // Its target, sized on the first frame.
        let source = gpu.source(&target_desc([1, 1]))?;
        let sample = gpu.sample(
            source,
            r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":4,"mipmaps":false,"compare":null}"#,
        )?;
        gpu.upload(sample)?;
        let texture = bridge.r.core.add_texture(gpu.texture(sample)?);
        let volume = bridge.effect_batch(layout::VOLUME as u32, 0, shadows, -1, VOLUMES as u32, -1, &[noise as i32, -1])?;
        let composite = bridge.effect_batch(layout::VOLUME_COMPOSITE as u32, 0, shadows, -1, 1, -1, &[texture as i32])?;
        // SteamWisps (thicksmoke2), on maps without fountains.
        let smoke = content
            .texture_paths
            .get("thicksmoke2")
            .cloned()
            .and_then(|path| content.textures.load(&path).ok())
            .and_then(|image| textures.get(image).copied());
        let wisps = match (fountains, smoke) {
            (false, Some(smoke)) => Some(bridge.effect_batch(layout::WISPS as u32, 0, shadows, smoke as i32, WISPS as u32, -1, &[])?),
            _ => None,
        };
        // SteamWisps' seeds: the source, then three draws of its stream.
        let mut state: u32 = 317;
        let mut random = || {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            state as f64 / 4294967296.
        };
        let mut wisp_seeds = Vec::with_capacity(WISPS * 4);
        for i in 0..WISPS {
            wisp_seeds.push((i / 16) as f32);
            for _ in 0..3 {
                wisp_seeds.push(random() as f32);
            }
        }
        let steam = Self { volume, composite, wisps, wisp_seeds, target: (source, sample, texture), size: [0, 0], at, time: 0., frames: 0 };
        bridge.fx_reserve(steam.end() as u32);
        Ok(Some(steam))
    }

    pub fn end(&self) -> usize {
        self.at + VOLUMES * 10 + VOLUME_VALUES + 3 + WISPS * 4 + WISP_VALUES
    }

    /// EnvironmentVolume.update/renderVolumes and SteamWisps.update.
    /// `screen` is the frame's size; `depth` whether the scene's depth is
    /// there to read (the composite and the volume fade against it).
    pub fn frame(&mut self, dt: f64, volumes: &[Volume], wakes: &super::map_effects::Wakes, screen: [i32; 2], depth: bool, gpu: &mut crate::resources::Resources, bridge: &mut Bridge) {
        self.time += dt.clamp(0., 0.1);
        if volumes.is_empty() || !depth {
            return;
        }
        let size = [((screen[0] as f64) * 0.5).ceil().max(1.) as i32, ((screen[1] as f64) * 0.5).ceil().max(1.) as i32];
        if size != self.size {
            self.size = size;
            if gpu.source_desc(self.target.0, &target_desc(size)).is_ok() && gpu.upload(self.target.1).is_ok() {
                if let Ok(texture) = gpu.texture(self.target.1) {
                    let _ = bridge.r.core.replace_sample_texture(self.target.2 as usize, texture);
                }
            }
        }
        let count = volumes.len().min(VOLUMES);
        let data = &mut bridge.r.effects.data;
        let at = self.at;
        for (i, v) in volumes.iter().take(count).enumerate() {
            for k in 0..3 {
                data[at + i * 3 + k] = v.position[k] as f32;
                data[at + count * 3 + i * 3 + k] = v.direction[k] as f32;
                data[at + count * 6 + i * 3 + k] = v.extent[k] as f32;
            }
            data[at + count * 9 + i] = v.kind as f32;
        }
        let values = at + count * 10;
        let mut v = [0f32; VOLUME_VALUES];
        v[0] = self.time as f32;
        v[1] = size[0] as f32;
        v[2] = size[1] as f32;
        v[3] = 1.;
        // The player's wakes, their times moved onto this clock.
        let wake = |v: &mut [f32], at: usize, self_time: f64| {
            for k in 0..8 {
                let p = wakes.points[k];
                let time = if p[3] < -99. { -100. } else { self_time - (wakes.time - p[3]) };
                v[at + k * 4..at + k * 4 + 4].copy_from_slice(&[p[0] as f32, p[1] as f32, p[2] as f32, time as f32]);
                let m = wakes.motion[k];
                v[at + 32 + k * 4..at + 32 + k * 4 + 4].copy_from_slice(&[m[0] as f32, m[1] as f32, m[2] as f32, m[3] as f32]);
            }
        };
        wake(&mut v, 4, self.time);
        data[values..values + VOLUME_VALUES].copy_from_slice(&v);
        let composite_at = values + VOLUME_VALUES;
        data[composite_at..composite_at + 3].copy_from_slice(&[size[0] as f32, size[1] as f32, 1.]);
        let mut draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        let mut draw = |bridge: &mut Bridge, row_values: [f64; 6], order: f64, id: f64| {
            if draws == render::MAX_DRAWS {
                return;
            }
            let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
            row.fill(0.);
            row[..6].copy_from_slice(&row_values);
            row[render::DRAW_KEY + 1] = order;
            row[render::DRAW_KEY + 5] = id;
            draws += 1;
        };
        // The volumes into their target, just before the composite (render
        // order 10) reads it.
        let packed = (size[0] as f64) * 65536. + size[1] as f64;
        draw(bridge, [render::KIND_INTO as f64, self.volume as f64, count as f64, at as f64, self.target.2 as f64, packed], 9.5, self.volume as f64);
        draw(bridge, [render::KIND_BATCH as f64, self.composite as f64, 1., 0., composite_at as f64, 1.], 10., self.composite as f64);
        // SteamWisps: the first nine vents in view, render order 11.
        if let Some(wisps) = self.wisps {
            let wisps_at = composite_at + 3;
            let data = &mut bridge.r.effects.data;
            data[wisps_at..wisps_at + WISPS * 4].copy_from_slice(&self.wisp_seeds);
            let w = wisps_at + WISPS * 4;
            let mut v = [0f32; WISP_VALUES];
            v[0] = self.time as f32;
            for i in 0..WISP_SOURCES {
                if let Some(vent) = volumes.get(i) {
                    for k in 0..3 {
                        v[1 + i * 4 + k] = vent.position[k] as f32;
                        v[37 + i * 4 + k] = vent.direction[k] as f32;
                    }
                    v[1 + i * 4 + 3] = 1.;
                    v[37 + i * 4 + 3] = vent.kind as f32;
                }
            }
            wake(&mut v, 73, self.time);
            v[WISP_VALUES - 1] = 1.;
            data[w..w + WISP_VALUES].copy_from_slice(&v);
            draw(bridge, [render::KIND_BATCH as f64, wisps as f64, WISPS as f64, 0., wisps_at as f64, WISPS as f64], 11., wisps as f64);
        }
        bridge.r.effects.input[render::IN_DRAWS] = draws as f64;
        self.frames += 1;
    }
}

/// environment-effects.js ENHANCED, less the fountains: the vents with an
/// Enhanced form.
pub fn is_steam(effect: &str) -> bool {
    effect.ends_with("/prop_steamgrate") || effect.ends_with("/prop_steampipe")
}

fn target_desc(size: [i32; 2]) -> String {
    serde_json::json!({"width":size[0],"height":size[1],"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1009,"srgb":false,"renderTarget":true}).to_string()
}
