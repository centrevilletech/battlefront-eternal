//! The sea (OceanSurface and EnhancedOcean, src/ocean-surface.js and
//! in the client this client replaces).
//!
//! A map whose water look asks for an ocean, or whose water reaches every edge
//! of its grid while OCEAN AND SKY is Enhanced, draws the sea in place of the
//! flat surface. The renderer owns its meshes, programs and look constants
//! (`effect_water`); what stays here is what the client owned: which of its
//! near and far surfaces draw, their per-frame values, the crest spray, and
//! the cube capture of the sky an Enhanced sea reflects.
use crate::{render_bridge::Bridge, resources::Resources};
use bfe_environment::layout::surface_result as out;
use bfe_render::{
    effect_layout as kinds,
    effects::{self as fx, DRAW_KEY},
    shader, water_resources as w,
};
use serde_json::{json, Value};

const IDENTITY: [f64; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
// EnhancedOcean's crest spray: its render order, and how many crests there are.
const SPRAY_ORDER: f64 = 19.;
const SPRAY_CAPACITY: usize = 384;
// SkyProbe: the cube's face size, and its first generation.
const PROBE_SIZE: i32 = 128;

// One of the sea's surfaces as the far pass holds it: its input slot and the
// values it reads, which keep the look's constants between frames.
struct Item {
    slot: usize,
    fields: &'static [(&'static str, u32)],
    values: Vec<f64>,
}
impl Item {
    fn set(&mut self, name: &str, value: &[f64]) {
        if let Some((at, size)) = w::field(self.fields, name) {
            let n = value.len().min(size);
            self.values[at..at + n].copy_from_slice(&value[..n]);
        }
    }
}

/// SkyProbe: the map's sky in a cube around the eye, which an Enhanced sea
/// that does not storm reflects and fades into.
#[derive(Clone, Copy)]
pub struct Probe {
    pub target: u32,
    pub sample: u32,
    pub texture: u32,
    pub size: i32,
}

pub struct Ocean {
    height: f64,
    stormy: bool,
    // The classic surface, then the Enhanced one, near scene and far scene.
    near: [Item; 2],
    far: [Item; 2],
    haze: Option<Item>,
    spray: Option<(u32, Vec<f64>, &'static [(&'static str, u32)])>,
    pub probe: Option<Probe>,
    mask: f64,
    water_map: f64,
    foam_map: f64,
    pub enhanced: bool,
    time: f64,
    pub storm_light: Option<crate::storm_host::Light>,
}

// Texture.createTarget's cube target: six RGBA8 faces with depth, a mip
// chain, linear sampling (SkyProbe's WebGLCubeRenderTarget).
pub(crate) fn cube(resources: &mut Resources, bridge: &mut Bridge, size: i32) -> Result<Probe, String> {
    let source = resources.source(&json!({"width":size,"height":size,"depth":1,"target":glow::TEXTURE_CUBE_MAP,"format":1023,"dataType":1009,"srgb":false,"renderTarget":true}).to_string())?;
    let sample = resources.sample(source, r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1008,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":4,"mipmaps":true,"compare":null}"#)?;
    let target = resources.target(&json!({"width":size,"height":size,"color":sample,"depth":null,"depthBuffer":true,"stencil":false,"samples":0,"resolveDepth":true,"resolveStencil":true,"cube":true}).to_string())?;
    resources.upload(sample)?;
    let texture = bridge.r.core.add_texture(resources.texture(sample)?);
    Ok(Probe { target, sample, texture, size })
}

impl Ocean {
    /// OceanSurface.constructor: the surfaces of the classic and the Enhanced
    /// sea for the near and the far scene, the haze of a storm's far sea and
    /// the crest spray. `mask` is the water occupancy texture, `images` the
    /// water's and foam's, as the core knows them (-1: none).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bridge: &mut Bridge,
        source: u32,
        data: &Value,
        stormy: bool,
        far_slot: &mut usize,
        mask: u32,
        water_map: Option<u32>,
        foam_map: Option<u32>,
        shadows: bool,
        floats: &mut usize,
    ) -> Result<Self, String> {
        let enhanced_kind = if stormy { kinds::OCEAN_STORM } else { kinds::OCEAN };
        let mut item = |bridge: &mut Bridge, kind: usize, near: bool| -> Result<Item, String> {
            let slot = *far_slot;
            // The near sea draws with the world's shadows, the far one without.
            let facts = bridge.effect_water(source, kind as u32, true, shadows && near, slot as u32, (slot + 100) as u32)?;
            *far_slot += 17 + facts.len() - 4;
            Ok(Item { slot, fields: w::fields(kind)?, values: facts[4..].to_vec() })
        };
        let near = [item(bridge, kinds::OCEAN_CLASSIC, true)?, item(bridge, enhanced_kind, true)?];
        let far = [item(bridge, kinds::OCEAN_CLASSIC, false)?, item(bridge, enhanced_kind, false)?];
        let haze = if stormy { Some(item(bridge, kinds::OCEAN_HAZE, false)?) } else { None };
        // EnhancedOcean.batch: the crest spray, a fixed batch of seeded crests.
        let spray_kind = if stormy { kinds::OCEAN_SPRAY_STORM } else { kinds::OCEAN_SPRAY };
        let batch = bridge.effect_batch(spray_kind as u32, 0, shadows, -1, SPRAY_CAPACITY as u32, -1, &[mask as i32])?;
        let defaults = bridge.water_values(source, spray_kind as u32)?;
        let fields = w::fields(spray_kind)?;
        *floats += defaults.len();
        Ok(Self {
            height: data["height"].as_f64().unwrap_or(0.),
            stormy,
            near,
            far,
            haze,
            spray: Some((batch, defaults, fields)),
            probe: None,
            mask: f64::from(mask),
            water_map: water_map.map_or(-1., f64::from),
            foam_map: foam_map.or(water_map).map_or(-1., f64::from),
            enhanced: false,
            time: 0.,
            storm_light: None,
        })
    }

    /// Floats of the effect data the spray's values take.
    pub fn spray_floats(&self) -> usize {
        self.spray.as_ref().map_or(0, |s| s.1.len())
    }

    /// OceanSurface.applyMode: the Enhanced sea draws, with its sky capture
    /// where it does not storm, or the classic one.
    pub fn set_enhanced(&mut self, bridge: &mut Bridge, resources: &mut Resources, enhanced: bool) -> Result<(), String> {
        self.enhanced = enhanced;
        if enhanced && !self.stormy && self.probe.is_none() {
            self.probe = Some(cube(resources, bridge, PROBE_SIZE)?);
        }
        Ok(())
    }

    /// Whether the sky capture is wanted (SkyProbe.enabled).
    pub fn probing(&self) -> bool {
        self.enhanced && self.probe.is_some()
    }
    pub fn slots(&self) -> impl Iterator<Item = usize> + '_ {
        self.near.iter().chain(self.far.iter()).chain(self.haze.iter()).map(|i| i.slot)
    }
    /// The surface the near phase draws now.
    pub fn near_slot(&self) -> usize {
        self.near[usize::from(self.enhanced)].slot
    }

    /// OceanSurface.publishFrame and prepareSource: the surfaces' placement
    /// and values for this frame, spray appended to the effect list.
    pub fn frame(&mut self, bridge: &mut Bridge, view: &[f64], result: &[f64], spray_at: usize) {
        self.time = result[out::OCEAN_TIME];
        // The sea follows the eye on a grid of its own (OceanSurface.update).
        let mut matrix = IDENTITY;
        matrix[12] = result[out::OCEAN_X];
        matrix[13] = self.height;
        matrix[14] = result[out::OCEAN_Z];
        let frame = |name: &str| {
            let i = shader::FRAME_MEMBERS.iter().position(|(n, _)| *n == name).expect("frame catalog member");
            (shader::FRAME_OFFSETS[i], shader::FRAME_MEMBERS[i].1)
        };
        let probe = self.probe.as_ref().map_or(-1., |p| f64::from(p.texture));
        let (mask, water_map, foam_map, time, enhanced) = (self.mask, self.water_map, self.foam_map, self.time, self.enhanced);
        // Near and far each hold the classic and the Enhanced surface; the far
        // scene's Enhanced storm adds its haze. Only far surfaces raise their
        // flag here: the near scene's draw in the near phase, which raises
        // its own for its packet.
        let slots = [(0, true), (1, true), (0, false), (1, false), (2, false)];
        for (which, (index, near)) in slots.into_iter().enumerate() {
            let item = match which {
                0 | 1 => &mut self.near[index],
                2 | 3 => &mut self.far[index],
                _ => match &mut self.haze {
                    Some(haze) => haze,
                    None => continue,
                },
            };
            let shown = match which {
                0 | 2 => !enhanced,
                _ => enhanced,
            };
            item.set("time", &[time]);
            for name in ["sunDirection", "sunColor", "fogColor", "ambient"] {
                let (at, _) = frame(name);
                item.set(name, &view[at..at + 3]);
            }
            let (at, _) = frame("fogRange");
            item.set("fogRange", &view[at..at + 2]);
            item.set("mask", &[mask]);
            item.set("waterMap", &[water_map]);
            item.set("foamMap", &[foam_map]);
            item.set("skyProbe", &[probe]);
            item.set("cloudMap", &[self.storm_light.map_or(-1., |l| l.texture as f64)]);
            item.set("cloudReady", &[self.storm_light.is_some() as u8 as f64]);
            item.set("lightningFlash", &[self.storm_light.map_or(0., |l| l.flash)]);
            item.set("lightningPosition", &self.storm_light.map_or([0.; 3], |l| l.position));
            let input = &mut bridge.r.far.input;
            input[item.slot] = f64::from(shown && !near);
            input[item.slot + 1..item.slot + 17].copy_from_slice(&matrix);
            input[item.slot + 17..item.slot + 17 + item.values.len()].copy_from_slice(&item.values);
        }
        // EnhancedOcean's spray draws with the effects, only on the Enhanced sea.
        if let Some((batch, values, fields)) = &mut self.spray {
            let effects = &mut bridge.r.effects;
            let draws = effects.input[fx::IN_DRAWS] as usize;
            if self.enhanced && draws < fx::MAX_DRAWS {
                let mut set = |name: &str, value: &[f64]| {
                    if let Some((at, size)) = w::field(fields, name) {
                        for (to, from) in values[at..at + size].iter_mut().zip(value) {
                            *to = *from;
                        }
                    }
                };
                let (p, v, c) = (frame("projectionMatrix").0, frame("viewMatrix").0, frame("cameraPosition").0);
                set("projectionMatrix", &view[p..p + 16]);
                set("viewMatrix", &view[v..v + 16]);
                set("cameraPosition", &view[c..c + 3]);
                set("time", &[self.time]);
                set("eye", &[view[c], view[c + 2]]);
                set("seaLevel", &[self.height]);
                for (to, from) in effects.data[spray_at..spray_at + values.len()].iter_mut().zip(values.iter()) {
                    *to = *from as f32;
                }
                let entry = &mut effects.input[fx::DRAW_AT + draws * fx::DRAW_STRIDE..][..fx::DRAW_STRIDE];
                entry.fill(0.);
                entry[..6].copy_from_slice(&[fx::KIND_BATCH as f64, f64::from(*batch), SPRAY_CAPACITY as f64, 0., spray_at as f64, SPRAY_CAPACITY as f64]);
                entry[DRAW_KEY + 1] = SPRAY_ORDER;
                entry[DRAW_KEY + 2..DRAW_KEY + 5].copy_from_slice(&[matrix[12], matrix[13], matrix[14]]);
                entry[DRAW_KEY + 5] = f64::from(*batch);
                effects.input[fx::IN_DRAWS] = (draws + 1) as f64;
            }
        }
    }
}
