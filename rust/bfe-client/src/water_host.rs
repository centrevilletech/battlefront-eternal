//! The terrain water's host (Water, src/water.js, and the mirror part of
//! BattleFrame, src/rust-render/frame.js, in the page this client replaces).
//!
//! The renderer already owns the water passes: the tint pass over a copy of
//! the opaque scene, the reflection pass and the low textured pass, each for
//! the near scene and for the far scene (`effect_water`), the reflected far
//! scene (the frame plan's mirror views) and the scene copy the passes read.
//! What stays here is what the page owned: which settings pick which passes,
//! the per-frame facts every pass reads (camera, clocks, sampler identities),
//! the mirrored camera, and the retained rows that publish them.
use crate::{render_bridge::Bridge, resources::Resources};
use bfe_environment::{
    layout::{
        environment_recipe as recipe_kind, surface_config as cfg, surface_frame as frame,
        surface_result as out, water_draw_view as draw, water_view as policy_in,
        water_view_result as policy_out,
    },
    surface_math::V8,
    surface_policy::Policy,
    surfaces::Phase,
    water_draw::Draws,
};
use bfe_render::{
    cameras::layout as cl,
    effect_layout as kinds,
    effects::{self as fx, DRAW_KEY},
    frame_plan::layout::{facts as f, state as s},
    shader, water_resources as w,
};
use serde_json::{json, Value};
use three::{Matrix4, Quaternion};

/// The page's WATER QUALITY and OCEAN AND SKY choices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub quality: String,
    pub ocean: String,
}
impl Default for Settings {
    // The page's defaults (DEFAULTS in src/settings.js).
    fn default() -> Self {
        Self { quality: "high".into(), ocean: "enhanced".into() }
    }
}

/// Images the water reads, by index in the map's texture library.
#[derive(Default)]
pub struct Images {
    pub main: Option<usize>,
    pub normals: Vec<usize>,
    pub bumps: Vec<usize>,
    pub masks: Vec<usize>,
    pub foam: Option<usize>,
}
impl Images {
    pub fn all(&self) -> impl Iterator<Item = usize> + '_ {
        self.main
            .into_iter()
            .chain(self.normals.iter().copied())
            .chain(self.bumps.iter().copied())
            .chain(self.masks.iter().copied())
            .chain(self.foam)
    }
}

/// Water.frames: a name prefix and a frame count; frames the map does not
/// have are left out, as there.
pub fn images(data: &Value, mut load: impl FnMut(&str) -> Result<Option<usize>, String>) -> Result<Images, String> {
    let recipe = bfe_environment::content::recipe(recipe_kind::WATER as u32, data);
    let mut frames = |key: &str| -> Result<Vec<usize>, String> {
        let mut found = Vec::new();
        for name in recipe["frames"][key]["names"].as_array().into_iter().flatten() {
            found.extend(load(name.as_str().unwrap_or(""))?);
        }
        Ok(found)
    };
    let normals = frames("NormalMapTextures")?;
    let bumps = frames("BumpMapTextures")?;
    let masks = frames("SpecularMaskTextures")?;
    let look = &data["look"];
    let main = match look["MainTexture"][0].as_str() {
        Some(name) => load(name)?,
        None => None,
    };
    let foam = match look["FoamTexture"][0].as_str() {
        Some(name) => load(name)?,
        None => None,
    };
    Ok(Images { main, normals, bumps, masks, foam })
}

// Draw rows 0-2 are the near scene's passes, 3-5 the far scene's, each
// tint, reflection, textured (Draws, bfe-environment).
const PASSES: [usize; 3] = [kinds::WATER_TINT, kinds::WATER_REFLECTION, kinds::WATER_TEXTURED];
// Water.surfaceAnchor's render orders: after the opaque scene, ahead of
// everything else that blends.
const ORDER: [f64; 3] = [-20., -19., -20.];
// The rows of the effect matrices the near passes take, ahead of the shields'.
const MATRICES: usize = 8;
const RESERVED_MATRICES: usize = crate::online::shields::BUBBLES + crate::online::holograms::MATRICES;
// The far camera's near plane (Graphics.farCamera).
const FAR_NEAR: f64 = 2.;
// The mirror's target before the first frame's size is known (REFLECTION_SIZE).
const MIRROR_SIZE: [i32; 2] = [512, 256];
const IDENTITY: [f64; 16] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

struct Near {
    model: u32,
    center: [f64; 3],
    // Where its values start in the host's span of the effect data.
    at: usize,
}

// A render target with a colour image the passes sample, as the core knows it.
#[derive(Clone, Copy)]
struct Target {
    target: u32,
    source: u32,
    sample: u32,
    texture: u32,
    size: [i32; 2],
}
struct Surface {
    colour: Target,
    depth_source: u32,
    depth_sample: u32,
    depth_texture: u32,
}

const NEAREST: &str = r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#;
const LINEAR: &str = r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1006,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#;

fn image(size: [i32; 2], format: u32, data_type: u32) -> String {
    json!({"width":size[0],"height":size[1],"depth":1,"target":glow::TEXTURE_2D,"format":format,"dataType":data_type,"srgb":false,"renderTarget":true}).to_string()
}
fn target_desc(size: [i32; 2], colour: u32, depth: Option<u32>) -> String {
    json!({"width":size[0],"height":size[1],"color":colour,"depth":depth,"depthBuffer":depth.is_none(),"stencil":depth.is_some(),"samples":0,"resolveDepth":false,"resolveStencil":false,"cube":false}).to_string()
}

// A render target with an RGBA8 colour image (a WebGLRenderTarget), and its
// depth: a buffer, or a texture the passes read.
fn target(resources: &mut Resources, bridge: &mut Bridge, size: [i32; 2], with_depth_texture: bool) -> Result<(Target, Option<(u32, u32, u32)>), String> {
    let source = resources.source(&image(size, 1023, 1009))?;
    let sample = resources.sample(source, LINEAR)?;
    let depth = if with_depth_texture {
        // The canvas holds depth and stencil together, and a copy between
        // framebuffers needs the same storage on both sides.
        let ds = resources.source(&image(size, 1027, 1020))?;
        let dsample = resources.sample(ds, NEAREST)?;
        Some((ds, dsample))
    } else {
        None
    };
    let id = resources.target(&target_desc(size, sample, depth.map(|d| d.1)))?;
    resources.upload(sample)?;
    let texture = bridge.r.core.add_texture(resources.texture(sample)?);
    let depth = match depth {
        Some((ds, dsample)) => {
            resources.upload(dsample)?;
            Some((ds, dsample, bridge.r.core.add_texture(resources.texture(dsample)?)))
        }
        None => None,
    };
    Ok((Target { target: id, source, sample, texture, size }, depth))
}

// The neutral normal and the black specular mask the page kept as
// DataTextures: one texel, nearest, clamped.
fn fallback(resources: &mut Resources, bridge: &mut Bridge, pixels: [u8; 4]) -> Result<u32, String> {
    let source = resources.source(&json!({"width":1,"height":1,"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1009,"srgb":false}).to_string())?;
    resources.source_pixels(source, &pixels)?;
    let sample = resources.sample(source, NEAREST)?;
    resources.upload(sample)?;
    Ok(bridge.r.core.add_texture(resources.texture(sample)?))
}

pub(crate) fn camera_range(p: &[f64]) -> [f64; 2] {
    [p[14] / (p[10] - 1.), p[14] / (p[10] + 1.)]
}

fn member(name: &str) -> usize {
    shader::FRAME_OFFSETS[shader::FRAME_MEMBERS.iter().position(|(n, _)| *n == name).expect("frame catalog member")]
}

/// What the water asks the frame to publish this frame.
pub struct Plan {
    /// Whether the reflected far scene is drawn anew this frame.
    pub mirror: bool,
    /// Whether the sky layers are mirrored first.
    pub sky: bool,
    /// The mirrored views' frame blocks: the sky's, with the plain
    /// projection, and the scenery's, with the one clipped at the surface.
    pub plain: Vec<f64>,
    pub clipped: Vec<f64>,
    /// The mirror's target and size, for its views' bindings.
    pub target: u32,
    /// Ultra draws near geometry into the final mirror, over the far image.
    pub near: Option<(u32, u32)>,
    pub size: [i32; 2],
    /// The scene copy's target (Enhanced water), and whether a pass reads
    /// it before the transparent list.
    pub surface: Option<u32>,
    pub copy: bool,
    /// The screen passes of an eye under the surface: Low's tint over the
    /// finished frame, Enhanced's fog over the opaque and transparent scene.
    pub underwater: bool,
    pub fog: bool,
    /// The sea's surface the near phase draws, when the sea replaces the flat
    /// surface, and whether its sky capture is due this frame.
    pub sea: Option<usize>,
    pub probe: bool,
}

pub struct WaterHost {
    data: Value,
    state: bfe_sim::units::water::WaterState,
    far_reach: f64,
    far_water: f64,
    has_ocean: bool,
    phase: Box<Phase>,
    // Each wading soldier's wake clock, by unit id (WaterState.soldierStep).
    wakes: std::collections::HashMap<u32, f64>,
    policy: Box<Policy>,
    draws: Box<Draws>,
    mode: Value,
    has_bumps: bool,
    tint: [f64; 3],
    near: [Option<Near>; 3],
    far: [Option<usize>; 3],
    mirror: Target,
    mirror_far: Option<Target>,
    surface: Option<Surface>,
    canvas: [i32; 2],
    reflected: bool,
    // The effect-list rows this host appended last frame: where they began
    // and where the list ended, so a frame drawn again without the list
    // having been rebuilt does not stack a second copy.
    appended: Option<(usize, usize)>,
    // The sea (OceanSurface), where the map has one, and where its spray's
    // values start in the host's span of the effect data.
    ocean: Option<crate::ocean_host::Ocean>,
    ocean_at: usize,
    // WaterShore: the two lapping batches (foam, splash), where their values
    // start in the host's span of the effect data, the shore quality (0 off,
    // 1 High, 2 Ultra), how many of their patches the renderer holds, and
    // the query scratch.
    shore: Option<[u32; 2]>,
    shore_at: usize,
    shore_quality: f64,
    shore_held: [usize; 2],
    gathered: bfe_sim::collision::spheres::Gathered,
    terrain: Vec<u32>,
    // Water.underwater and Water.underwaterFog: the screen programs, and the
    // colour the Low tint shows (the look's UnderwaterColor).
    screens: [u32; 2],
    underwater: [f64; 4],
    pub plan: Plan,
    floats: usize,
}

// WaterShore's patches a batch holds, and the floats a batch takes in the
// effect data: a patch's point (4) and phase (1), then its 54 values.
const SHORE_PATCHES: usize = 384;
const SHORE_BATCH: usize = SHORE_PATCHES * 5 + 54;
impl WaterHost {
    /// Water.constructor, for the map's water. `textures` are the core's
    /// texture ids by library index; `far_slot` is the first free slot of the
    /// far input, and moves past the slots the far passes take.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bridge: &mut Bridge,
        resources: &mut Resources,
        data: &Value,
        images: &Images,
        textures: &[u32],
        far_slot: &mut usize,
        far_reach: f64,
        size: [i32; 2],
        shadows: bool,
        sea: (u32, bool),
    ) -> Result<Self, String> {
        let look = &data["look"];
        let recipe = bfe_environment::content::recipe(recipe_kind::WATER as u32, data);
        let ocean = recipe["ocean"].as_bool().unwrap_or(false);
        let mut phase = Box::new(Phase::new(true));
        phase.config[cfg::HEIGHT] = data["height"].as_f64().unwrap_or(0.);
        phase.config[cfg::BOBBING] = f64::from(look["OscillationEnable"][0].as_f64().unwrap_or(1.) > 0.);
        // Water.frames' counts and rates, for the frames that exist.
        for (key, count, fps, have) in [
            ("NormalMapTextures", cfg::NORMAL_COUNT, cfg::NORMAL_FPS, images.normals.len()),
            ("BumpMapTextures", cfg::BUMP_COUNT, cfg::BUMP_FPS, images.bumps.len()),
            ("SpecularMaskTextures", cfg::MASK_COUNT, cfg::MASK_FPS, images.masks.len()),
        ] {
            phase.config[count] = have as f64;
            phase.config[fps] = if have == 0 { 0. } else { look[key][2].as_f64().unwrap_or(0.) };
        }
        let mut policy = Box::new(Policy::default());
        policy.configure(data);
        // The authored boxes of the visibility queries, once: the water's
        // own, then each cell's (Water.bounds, Water.cellBounds).
        let mut boxes: Vec<f64> = recipe["bounds"].as_array().into_iter().flatten().flat_map(|c| c.as_array().into_iter().flatten().map(|v| v.as_f64().unwrap_or(0.))).collect();
        for cell in recipe["cells"].as_array().into_iter().flatten() {
            boxes.extend(cell.as_array().into_iter().flatten().map(|v| v.as_f64().unwrap_or(0.)));
        }
        bridge.camera_mirror_bounds_frame(&boxes);
        let mut draws = Box::new(Draws::default());
        let id = |i: &usize| textures[*i] as f64;
        let normal = fallback(resources, bridge, [128, 128, 255, 255])?;
        let mask = fallback(resources, bridge, [0, 0, 0, 255])?;
        draws.samples(&json!({
            "normal": images.normals.iter().map(id).collect::<Vec<_>>(),
            "bump": images.bumps.iter().map(id).collect::<Vec<_>>(),
            "mask": images.masks.iter().map(id).collect::<Vec<_>>(),
            "main": images.main.as_ref().map_or(-1., id),
            "normalFallback": normal,
            "maskFallback": mask,
        }));
        let source = bridge.water_source(&data.to_string(), ocean)?[0] as u32;
        let (mirror, _) = target(resources, bridge, MIRROR_SIZE, false)?;
        let mut host = Self {
            data: data.clone(),
            state: crate::online::water::from_data(data),
            far_reach,
            far_water: look["FarSceneRange"][0].as_f64().unwrap_or(0.),
            has_ocean: ocean,
            phase,
            wakes: std::collections::HashMap::new(),
            policy,
            draws,
            mode: Value::Null,
            has_bumps: !images.bumps.is_empty(),
            tint: std::array::from_fn(|i| recipe["tint"][i].as_f64().unwrap_or(1.)),
            near: [None, None, None],
            far: [None; 3],
            mirror,
            mirror_far: None,
            surface: None,
            canvas: size,
            reflected: false,
            appended: None,
            ocean: None,
            ocean_at: 0,
            shore: None,
            shore_at: 0,
            shore_quality: 0.,
            shore_held: [0; 2],
            gathered: Default::default(),
            terrain: Vec::new(),
            screens: [bridge.add_owned_program("effects/water-underwater-tint-s0")?, bridge.add_owned_program("effects/water-underwater-fog-s0")?],
            underwater: std::array::from_fn(|i| recipe["underwater"][i].as_f64().unwrap_or(0.)),
            plan: Plan { mirror: false, sky: false, plain: vec![0.; shader::FRAME_FLOATS], clipped: vec![0.; shader::FRAME_FLOATS], target: mirror.target, near: None, size: MIRROR_SIZE, surface: None, copy: false, underwater: false, fog: false, sea: None, probe: false },
            floats: 0,
        };
        // Water.surfaceAnchor for each pass, near and far. A map without a
        // main texture has no low pass.
        for (k, kind) in PASSES.iter().enumerate() {
            if *kind == kinds::WATER_TEXTURED && images.main.is_none() {
                continue;
            }
            let facts = bridge.effect_water(source, *kind as u32, false, shadows, 0, (k + 1) as u32)?;
            host.draws.values(k).copy_from_slice(&facts[4..]);
            host.near[k] = Some(Near { model: facts[0] as u32, center: [facts[1], facts[2], facts[3]], at: host.floats });
            host.floats += facts.len() - 4;
            let slot = *far_slot;
            let facts = bridge.effect_water(source, *kind as u32, true, false, slot as u32, (k + 4) as u32)?;
            host.draws.values(3 + k).copy_from_slice(&facts[4..]);
            *far_slot += 17 + facts.len() - 4;
            host.far[k] = Some(slot);
        }
        // OceanSurface, for a map the sea may replace the surface of. `sea` is
        // the water occupancy texture and whether the map storms (a weather of
        // anything but quads).
        if ocean {
            let mut floats = 0;
            let map = |image: Option<usize>| image.map(|i| textures[i]);
            let sea = crate::ocean_host::Ocean::new(bridge, source, data, sea.1, far_slot, sea.0, map(images.main), map(images.foam), shadows, &mut floats)?;
            host.ocean_at = host.floats;
            host.floats += sea.spray_floats();
            host.ocean = Some(sea);
        }
        // WaterShore's two batches: the foam and the splash along the waterline.
        let mut shore = [0; 2];
        for batch in &mut shore {
            *batch = bridge.effect_batch(kinds::WATER_SHORE as u32, 0, shadows, -1, SHORE_PATCHES as u32, -1, &[])?;
        }
        host.shore = Some(shore);
        host.shore_at = host.floats;
        host.floats += 2 * SHORE_BATCH;
        Ok(host)
    }

    /// Leave every far item of the water out of a packet: the flat passes' and the sea's.
    pub fn hide_all_far(&self, far: &mut [f64]) {
        self.hide_far(far);
        for slot in self.ocean.iter().flat_map(|o| o.slots()) {
            far[slot] = 0.;
        }
    }

    /// A mirrored far scene leaves the far passes out (they draw from the
    /// eye's far camera only): hide them, keeping what each showed.
    pub fn hide_far(&self, far: &mut [f64]) -> [f64; 3] {
        std::array::from_fn(|k| self.far[k].map_or(0., |at| std::mem::replace(&mut far[at], 0.)))
    }
    pub fn show_far(&self, far: &mut [f64], shown: [f64; 3]) {
        for (k, value) in shown.into_iter().enumerate() {
            if let Some(at) = self.far[k] {
                far[at] = value;
            }
        }
    }

    /// The floats of the effect data the near passes' values take.
    pub fn data_floats(&self) -> usize {
        self.floats
    }

    /// Whether the settings leave the flat surface to draw (an ocean replaces it).
    pub fn flat(&self) -> bool {
        !self.mode.is_null() && !self.mode["ocean"].as_bool().unwrap_or(false)
    }
    fn enhanced(&self) -> bool {
        self.mode["enhanced"].as_bool().unwrap_or(false)
    }
    fn quality(&self, key: &str) -> f64 {
        self.mode["quality"][key].as_f64().unwrap_or(0.)
    }
    // Water.lowPass.visible: the textured pass, where the map has one.
    fn low(&self) -> bool {
        self.near[2].is_some() && self.mode["low"].as_bool().unwrap_or(false)
    }

    /// Water.applyQuality: the passes the settings choose.
    pub fn set_settings(&mut self, bridge: &mut Bridge, resources: &mut Resources, settings: &Settings) -> Result<(), String> {
        self.mode = self.draws.mode(&json!({
            "quality": settings.quality,
            "oceanEffects": settings.ocean,
            "hasOcean": self.has_ocean,
            "oceanAlways": self.data["look"]["OceanEnable"][0].as_f64().unwrap_or(0.) > 0.,
            "low": self.near[2].is_some(),
        }));
        self.shore_quality = match settings.quality.as_str() {
            "ultra" => 2.,
            "high" => 1.,
            _ => 0.,
        };
        let sea = settings.ocean == "enhanced";
        if let Some(ocean) = &mut self.ocean {
            ocean.set_enhanced(bridge, resources, sea)?;
        }
        // Enhanced water reads a copy of the opaque colour and depth, made
        // once per frame before the first pass that reads it.
        if self.enhanced() && self.surface.is_none() && self.flat() {
            self.surface = Some(self.surface_target(resources, bridge)?);
        }
        if self.mode["planar"].as_bool().unwrap_or(false) && self.mirror_far.is_none() {
            self.mirror_far = Some(target(resources, bridge, MIRROR_SIZE, false)?.0);
        }
        Ok(())
    }

    /// Omit this host's surfaces from the mirrored near scene.
    pub fn draws_before_water(&self, count: usize) -> usize {
        self.appended.filter(|(_, end)| *end == count).map_or(count, |(start, _)| start)
    }

    fn surface_target(&self, resources: &mut Resources, bridge: &mut Bridge) -> Result<Surface, String> {
        let (colour, depth) = target(resources, bridge, self.canvas, true)?;
        let (depth_source, depth_sample, depth_texture) = depth.ok_or("scene copy without depth")?;
        Ok(Surface { colour, depth_source, depth_sample, depth_texture })
    }

    pub fn storm_light(&mut self, light: Option<crate::storm_host::Light>) {
        if let Some(ocean) = &mut self.ocean { ocean.storm_light = light; }
    }

    pub fn ocean(&self) -> Option<&crate::ocean_host::Ocean> {
        self.ocean.as_ref()
    }

    /// The window changed size: the scene copy follows it.
    pub fn resize(&mut self, bridge: &mut Bridge, resources: &mut Resources, size: [i32; 2]) -> Result<(), String> {
        self.canvas = size;
        if let Some(surface) = &mut self.surface {
            resources.source_desc(surface.colour.source, &image(size, 1023, 1009))?;
            resources.source_desc(surface.depth_source, &image(size, 1027, 1020))?;
            resources.resize_target(surface.colour.target, &target_desc(size, surface.colour.sample, Some(surface.depth_sample)))?;
            surface.colour.size = size;
        }
        let _ = bridge;
        Ok(())
    }

    /// WaterState.soldierStep for the soldiers on foot (unit id, feet,
    /// velocity): a wet one is a wader for the surface's contact foam and
    /// rings, and leaves a wake ring every 1 / (0.3 v^2 + 4) s while it moves.
    pub fn soldiers(&mut self, dt: f64, soldiers: &[(u32, [f64; 3], [f64; 3])]) {
        use bfe_environment::layout::{surface as lim, surface_command as cmd, surface_command_kind as kind};
        const WAKE_LIFE: f64 = 2.;
        const WAKE_RADIUS: f64 = 0.25;
        let level = self.state.level();
        let mut n = 0;
        let mut push = |phase: &mut Phase, row: [f64; cmd::STRIDE]| {
            if n < lim::COMMANDS {
                phase.commands[n * cmd::STRIDE..(n + 1) * cmd::STRIDE].copy_from_slice(&row);
                n += 1;
            }
        };
        let mut seen = Vec::with_capacity(soldiers.len());
        for &(id, feet, velocity) in soldiers {
            if !(level > feet[1] && self.state.over(feet[0], feet[2])) {
                continue;
            }
            seen.push(id);
            let mut row = [0.; cmd::STRIDE];
            row[cmd::KIND] = kind::WADER;
            row[cmd::ID] = id as f64 + 1.;
            row[cmd::POSITION..cmd::POSITION + 3].copy_from_slice(&feet);
            row[cmd::VELOCITY..cmd::VELOCITY + 3].copy_from_slice(&velocity);
            row[cmd::DEPTH] = level - feet[1];
            push(&mut self.phase, row);
            let speed2 = velocity.iter().map(|v| v * v).sum::<f64>();
            let clock = self.wakes.entry(id).or_insert(0.);
            *clock += dt;
            let interval = 1. / (0.3 * speed2 + 4.);
            if *clock > interval && speed2 > 0. {
                *clock = (*clock - interval).min(interval);
                let mut row = [0.; cmd::STRIDE];
                row[cmd::KIND] = kind::RIPPLE;
                for k in 0..3 {
                    row[cmd::POSITION + k] = feet[k] + velocity[k] * 0.03;
                }
                row[cmd::VELOCITY..cmd::VELOCITY + 3].copy_from_slice(&velocity);
                row[cmd::HAS_VELOCITY] = 1.;
                row[cmd::LIFE] = WAKE_LIFE;
                row[cmd::RADIUS] = WAKE_RADIUS;
                push(&mut self.phase, row);
            }
        }
        self.wakes.retain(|id, _| seen.contains(id));
        self.phase.frame[frame::COMMAND_START] = 0.;
        self.phase.frame[frame::COMMANDS] = n as f64;
    }

    // Water.update, updateWaders and updateRings for this frame.
    fn step(&mut self, view: &[f64], dt: f64, collision: Option<(&crate::online::replay::Collision, u32)>) {
        let sea = !self.flat();
        let f = &mut self.phase.frame;
        f[frame::DT] = dt;
        let eye = member("cameraPosition");
        f[frame::EYE..frame::EYE + 3].copy_from_slice(&view[eye..eye + 3]);
        // The camera looks down the negated third row of the view matrix.
        let v = &view[16..32];
        f[frame::FORWARD..frame::FORWARD + 3].copy_from_slice(&[-v[2], -v[6], -v[10]]);
        let sun = member("sunDirection");
        f[frame::SUN..frame::SUN + 3].copy_from_slice(&view[sun..sun + 3]);
        f[frame::LEVEL] = self.state.level();
        // OceanSurface.update's clock runs while its material is drawn.
        f[frame::OCEAN] = f64::from(sea);
        f[frame::SHORE_QUALITY] = self.shore_quality;
        let Self { phase, state, gathered, terrain, .. } = self;
        match collision {
            Some((collision, world)) => phase.update::<V8, _>(&mut Shore { collision, world: world as usize, state, gathered, terrain }),
            None => phase.update::<V8, _>(&mut NoShore),
        }
        self.state.step(dt);
    }

    // The main camera as the camera owner takes it (PageCameras): pose,
    // lens and matrices, from the frame block's view and projection.
    pub(crate) fn camera(bridge: &mut Bridge, view: &[f64], far_reach: f64, planar: bool, level: f64, fade: [f64; 2], water_far: f64) {
        let mut world = Matrix4::default();
        world.elements.copy_from_slice(&view[16..32]);
        world.invert();
        let mut quaternion = Quaternion::default();
        quaternion.set_from_rotation_matrix(&world);
        let mut inverse = Matrix4::default();
        inverse.elements.copy_from_slice(&view[0..16]);
        inverse.invert();
        let p = &view[0..16];
        // The lens a perspective matrix came from: vertical field of view,
        // aspect and the planes.
        let [near, far] = camera_range(p);
        let fov = 2. * (1. / p[5]).atan().to_degrees();
        let input = &mut bridge.r.cameras.input;
        input[cl::input::POSITION..cl::input::POSITION + 3].copy_from_slice(&[world.elements[12], world.elements[13], world.elements[14]]);
        input[cl::input::QUATERNION..cl::input::QUATERNION + 4].copy_from_slice(&[quaternion.x, quaternion.y, quaternion.z, quaternion.w]);
        input[cl::input::SCALE..cl::input::SCALE + 3].copy_from_slice(&[1., 1., 1.]);
        input[cl::input::UP..cl::input::UP + 3].copy_from_slice(&[0., 1., 0.]);
        let lens = &mut input[cl::input::LENS..cl::input::LENS + cl::lens::LEN];
        lens.fill(0.);
        lens[cl::lens::FOV] = fov;
        lens[cl::lens::ASPECT] = p[5] / p[0];
        lens[cl::lens::NEAR] = near;
        lens[cl::lens::FAR] = far;
        lens[cl::lens::ZOOM] = 1.;
        lens[cl::lens::FILM_GAUGE] = 35.;
        input[cl::input::PUBLISHED] = 1.;
        let at = cl::input::MATRICES;
        input[at + cl::source::WORLD..at + cl::source::WORLD + 16].copy_from_slice(&world.elements);
        input[at + cl::source::INVERSE..at + cl::source::INVERSE + 16].copy_from_slice(&view[16..32]);
        input[at + cl::source::PROJECTION..at + cl::source::PROJECTION + 16].copy_from_slice(p);
        input[at + cl::source::PROJECTION_INVERSE..at + cl::source::PROJECTION_INVERSE + 16].copy_from_slice(&inverse.elements);
        // The far camera's range: near plane 2, out to the far scene's reach.
        input[cl::input::FAR_RANGE..cl::input::FAR_RANGE + 2].copy_from_slice(&[FAR_NEAR, far_reach]);
        input[cl::input::WATER_LEVEL] = level;
        input[cl::input::WATER_FADE..cl::input::WATER_FADE + 2].copy_from_slice(&fade);
        input[cl::input::WATER_REACH] = water_far;
        input[cl::input::FAR_REACH] = far_reach;
        input[cl::input::NEAR_REFLECTION] = f64::from(planar);
        bridge.camera_main_frame();
    }

    /// One frame of the water: its clocks and facts, the rows of every pass
    /// it draws (appended to the effect list at `data_at` in the effect
    /// data, the shields' room ending there), and what the frame must
    /// publish for them: Water.update, Water.reflect and Water.prepareSurface.
    pub fn frame(&mut self, bridge: &mut Bridge, resources: &mut Resources, view: &[f64], dt: f64, data_at: usize, collision: Option<(&crate::online::replay::Collision, u32)>) -> Result<(), String> {
        self.plan.mirror = false;
        self.plan.near = None;
        self.plan.sky = false;
        self.plan.copy = false;
        self.plan.underwater = false;
        self.plan.fog = false;
        self.plan.sea = None;
        self.plan.probe = false;
        self.plan.surface = self.surface.as_ref().filter(|_| self.enhanced() && self.flat()).map(|s| s.colour.target);
        if self.mode.is_null() {
            return Ok(());
        }
        if !self.flat() {
            self.appended = None;
            return self.sea(bridge, view, dt, data_at, collision);
        }
        self.step(view, dt, collision);
        let eye = member("cameraPosition");
        let fade = member("fade");
        let planar = self.mode["planar"].as_bool().unwrap_or(false);
        Self::camera(bridge, view, self.far_reach, planar, self.state.level(), [view[fade], view[fade + 1]], self.far_water);

        // Water.surfacePolicy: the view's range, target size and the
        // uniforms every pass shares.
        let (scale, max_size) = (self.quality("scale"), self.quality("maxSize"));
        let p = &mut self.policy.input;
        p[policy_in::FAR] = self.far_reach;
        p[policy_in::FADE..policy_in::FADE + 2].copy_from_slice(&view[fade..fade + 2]);
        p[policy_in::AUTHORED_FAR] = self.far_reach;
        p[policy_in::WATER_FAR] = self.far_water;
        let input = &bridge.r.cameras.input;
        p[policy_in::NEAR] = input[cl::input::LENS + cl::lens::NEAR];
        p[policy_in::FOV] = input[cl::input::LENS + cl::lens::FOV];
        p[policy_in::ASPECT] = input[cl::input::LENS + cl::lens::ASPECT];
        p[policy_in::INVERSE..policy_in::INVERSE + 16].copy_from_slice(&view[16..32]);
        p[policy_in::PROJECTION..policy_in::PROJECTION + 16].copy_from_slice(&view[0..16]);
        p[policy_in::SCALE] = scale;
        p[policy_in::MAX_SIZE] = max_size;
        p[policy_in::SIZE] = f64::from(self.canvas[0]);
        p[policy_in::SIZE + 1] = f64::from(self.canvas[1]);
        p[policy_in::LEVEL] = self.state.level();
        p[policy_in::EYE_Y] = view[eye + 1];
        p[policy_in::HAS_BUMPS] = f64::from(self.has_bumps);
        p[policy_in::PLANAR] = f64::from(planar);
        self.policy.update::<V8>();
        let policy = self.policy.draw_values(view[eye + 1], self.far_reach);
        let size = [self.policy.result[policy_out::DIMENSIONS] as i32, self.policy.result[policy_out::DIMENSIONS + 1] as i32];

        // Water.reflect: the water in view, and the mirror's cadence.
        bridge.camera_mirror_visibility_frame();
        let visible = bridge.r.cameras.mirror[cl::mirror::VISIBLE] != 0.;
        let far_visible = bridge.r.cameras.mirror[cl::mirror::FAR_VISIBLE] != 0.;
        let forward = &bridge.r.cameras.output[cl::role::MAIN * cl::camera::LEN + cl::camera::DIRECTION..][..3];
        let forward = [forward[0], forward[1], forward[2]];
        let facts = &mut bridge.r.frames.plan.facts;
        facts[f::MIRROR_ENABLED] = 1.;
        facts[f::MIRROR_VISIBLE] = f64::from(visible);
        facts[f::MIRROR_FAR_VISIBLE] = f64::from(far_visible);
        facts[f::MIRROR_LOW] = f64::from(self.low());
        facts[f::MIRROR_NEAR] = f64::from(planar);
        facts[f::MIRROR_LEVEL] = self.state.level();
        facts[f::MIRROR_SIZE] = f64::from(size[0]);
        facts[f::MIRROR_SIZE + 1] = f64::from(size[1]);
        facts[f::MIRROR_EYE..f::MIRROR_EYE + 3].copy_from_slice(&view[eye..eye + 3]);
        facts[f::MIRROR_FORWARD..f::MIRROR_FORWARD + 3].copy_from_slice(&forward);
        facts[f::WATER_SCALE] = self.quality("scale");
        facts[f::WATER_OVER] = f64::from(self.state.over(view[eye], view[eye + 2]));
        facts[f::WATER_LEVEL] = self.state.level();
        facts[f::WATER_EYE_Y] = view[eye + 1];
        facts[f::WATER_OCEAN] = 0.;
        bridge.r.frames.plan.select();
        let state = &bridge.r.frames.plan.state;
        self.plan.underwater = state[s::UNDERWATER] != 0.;
        self.plan.fog = state[s::FOG] != 0. && self.surface.is_some();
        let eligible = state[s::MIRROR_ELIGIBLE] != 0.;
        let held = state[s::MIRROR_HELD] != 0.;
        // A mirror seen from on or below the surface has no reflection to
        // show: its pass stays out, and so does one with a refused clip.
        let mut shown = eligible && held;
        if eligible && !held {
            bridge.r.cameras.input[cl::input::NEAR_REFLECTION] = f64::from(planar);
            bridge.camera_reflect_frame();
            if bridge.r.cameras.mirror[cl::mirror::VALID] != 0. {
                bridge.r.frames.plan.facts[f::MIRROR_CLIP_VALID] = 1.;
                self.mirror_views(bridge, view);
                self.plan.mirror = true;
                self.plan.sky = true;
                shown = true;
            }
        }
        self.reflected = shown;
        // Water.resizeReflection: a new size is a new image; a held one
        // never changes size.
        if self.plan.mirror && size != self.mirror.size {
            resources.source_desc(self.mirror.source, &image(size, 1023, 1009))?;
            resources.resize_target(self.mirror.target, &target_desc(size, self.mirror.sample, None))?;
            self.mirror.size = size;
        }
        self.plan.target = self.mirror.target;
        if planar {
            if let Some(far) = &mut self.mirror_far {
                if self.plan.mirror && size != far.size {
                    resources.source_desc(far.source, &image(size, 1023, 1009))?;
                    resources.resize_target(far.target, &target_desc(size, far.sample, None))?;
                    far.size = size;
                }
                self.plan.target = far.target;
                self.plan.near = Some((self.mirror.target, far.texture));
            }
        }
        self.plan.size = self.mirror.size;
        self.rows(bridge, view, &policy, data_at, [visible, far_visible]);
        Ok(())
    }

    // The sea's frame: its surfaces' placement and values, the sky capture
    // an Enhanced sea that does not storm is due, and what an eye under
    // the surface sees. The flat surface and its mirror stand down.
    fn sea(&mut self, bridge: &mut Bridge, view: &[f64], dt: f64, data_at: usize, collision: Option<(&crate::online::replay::Collision, u32)>) -> Result<(), String> {
        self.step(view, dt, collision);
        let eye = member("cameraPosition");
        let fade = member("fade");
        Self::camera(bridge, view, self.far_reach, false, self.state.level(), [view[fade], view[fade + 1]], self.far_water);
        let Some(ocean) = &mut self.ocean else {
            return Ok(());
        };
        ocean.frame(bridge, view, &self.phase.result, data_at + self.ocean_at);
        self.plan.sea = Some(ocean.near_slot());
        let time = self.phase.result[out::OCEAN_TIME];
        let facts = &mut bridge.r.frames.plan.facts;
        // SkyProbe.update: the capture's cadence is the plan's.
        facts[f::PROBE_ENABLED] = f64::from(ocean.probing());
        facts[f::PROBE_TIME] = time;
        facts[f::PROBE_GENERATION] = 1.;
        facts[f::PROBE_EYE..f::PROBE_EYE + 3].copy_from_slice(&view[eye..eye + 3]);
        facts[f::WATER_SCALE] = self.mode["quality"]["scale"].as_f64().unwrap_or(0.);
        facts[f::WATER_OVER] = f64::from(self.state.over(view[eye], view[eye + 2]));
        facts[f::WATER_LEVEL] = self.state.level();
        facts[f::WATER_EYE_Y] = view[eye + 1];
        facts[f::WATER_OCEAN] = 1.;
        bridge.r.frames.plan.select();
        let state = &bridge.r.frames.plan.state;
        self.plan.probe = state[s::PROBE_CAPTURE] != 0.;
        self.plan.underwater = state[s::UNDERWATER] != 0.;
        Ok(())
    }

    // Water.prepareSurface for each pass that draws this frame: its values
    // from the frame's facts, into the near scene's effect list or the far
    // scene's input.
    fn rows(&mut self, bridge: &mut Bridge, view: &[f64], policy: &[f64], data_at: usize, seen: [bool; 2]) {
        let eye = member("cameraPosition");
        let surface = self.mode["surface"].as_bool().unwrap_or(false);
        let shown = [
            [seen[0] && surface, seen[0] && surface && self.reflected, seen[0] && self.low()],
            [seen[1] && surface, seen[1] && surface && self.reflected, seen[1] && self.low()],
        ];
        // The scene copy before the first pass that reads it, when it does.
        self.plan.copy = self.plan.surface.is_some() && shown[0][0];
        let first = bridge.r.effects.input[fx::IN_DRAWS] as usize;
        let mut first = first;
        if let Some((before, after)) = self.appended.take() {
            // Models and meshes number separately: a row is ours by kind and id.
            let row = &bridge.r.effects.input[fx::DRAW_AT + before * fx::DRAW_STRIDE..][..2];
            let owned = if row[0] == fx::KIND_MODEL as f64 {
                self.near.iter().flatten().any(|n| f64::from(n.model) == row[1])
            } else {
                row[0] == fx::KIND_BATCH as f64 && self.shore.is_some_and(|meshes| meshes.iter().any(|mesh| f64::from(*mesh) == row[1]))
            };
            if first == after && owned {
                first = before;
                bridge.r.effects.input[fx::IN_DRAWS] = before as f64;
            }
        }
        let wave = self.phase.result[out::WAVE];
        let mut world = IDENTITY;
        world[13] = wave;
        let reflection = &bridge.r.cameras.mirror[cl::mirror::REFLECTION..cl::mirror::REFLECTION + 16];
        let projection: [f64; 16] = view[0..16].try_into().unwrap_or(IDENTITY);
        let lens = &bridge.r.cameras.input[cl::input::LENS..cl::input::LENS + cl::lens::LEN];
        let range = [lens[cl::lens::NEAR], lens[cl::lens::FAR]];
        let camera = &bridge.r.cameras.input[cl::input::MATRICES + cl::source::WORLD..][..16];
        let (sun, fog, fog_range) = (member("sunColor"), member("fogColor"), member("fogRange"));
        let scene = self.surface.as_ref().map_or([-1., -1.], |s| [f64::from(s.colour.texture), f64::from(s.depth_texture)]);
        for far in [false, true] {
            let d = &mut self.draws.input;
            d[draw::PROJECTION..draw::PROJECTION + 16].copy_from_slice(&projection);
            d[draw::INVERSE..draw::INVERSE + 16].copy_from_slice(&view[16..32]);
            d[draw::CAMERA..draw::CAMERA + 16].copy_from_slice(camera);
            d[draw::CAMERA_RANGE..draw::CAMERA_RANGE + 2].copy_from_slice(&if far { [FAR_NEAR, self.far_reach] } else { range });
            d[draw::SUN_COLOR..draw::SUN_COLOR + 3].copy_from_slice(&view[sun..sun + 3]);
            d[draw::REFLECTION_MATRIX..draw::REFLECTION_MATRIX + 16].copy_from_slice(reflection);
            d[draw::TINT..draw::TINT + 3].copy_from_slice(&self.tint);
            d[draw::RESOLUTION..draw::RESOLUTION + 2].copy_from_slice(&[f64::from(self.canvas[0]), f64::from(self.canvas[1])]);
            d[draw::SCENE..draw::SCENE + 2].copy_from_slice(&scene);
            d[draw::REFLECTION] = f64::from(self.mirror.texture);
            d[draw::FOG_COLOR..draw::FOG_COLOR + 3].copy_from_slice(&view[fog..fog + 3]);
            d[draw::FOG_RANGE..draw::FOG_RANGE + 2].copy_from_slice(&view[fog_range..fog_range + 2]);
            d[draw::EYE_Y] = view[eye + 1];
            d[draw::FAR] = self.far_reach;
            for k in 0..3 {
                let slot = 3 * usize::from(far) + k;
                if far {
                    let Some(at) = self.far[k] else { continue };
                    let input = &mut bridge.r.far.input;
                    input[at] = f64::from(shown[1][k]);
                    if !shown[1][k] {
                        continue;
                    }
                    self.draws.prepare(slot, &self.phase.result, policy);
                    input[at + 1..at + 17].copy_from_slice(&world);
                    let row = self.draws.values(slot);
                    input[at + 17..at + 17 + row.len()].copy_from_slice(row);
                } else {
                    let Some(near) = &self.near[k] else { continue };
                    let effects = &mut bridge.r.effects;
                    let draws = effects.input[fx::IN_DRAWS] as usize;
                    if !shown[0][k] || draws == fx::MAX_DRAWS {
                        continue;
                    }
                    self.draws.prepare(slot, &self.phase.result, policy);
                    let row = self.draws.values(slot);
                    let at = data_at + near.at;
                    for (to, from) in effects.data[at..at + row.len()].iter_mut().zip(row.iter()) {
                        *to = *from as f32;
                    }
                    // The group lifts by the bob; the sort key is the
                    // sphere's centre through that matrix.
                    let matrix = (fx::MAX_DRAWS - RESERVED_MATRICES - MATRICES + k) * 16;
                    effects.matrices[matrix..matrix + 16].copy_from_slice(&world);
                    let entry = &mut effects.input[fx::DRAW_AT + draws * fx::DRAW_STRIDE..][..fx::DRAW_STRIDE];
                    entry.fill(0.);
                    entry[..4].copy_from_slice(&[fx::KIND_MODEL as f64, f64::from(near.model), matrix as f64, at as f64]);
                    entry[DRAW_KEY + 1] = ORDER[k];
                    entry[DRAW_KEY + 2..DRAW_KEY + 5].copy_from_slice(&[near.center[0], near.center[1] + wave, near.center[2]]);
                    entry[DRAW_KEY + 5] = f64::from(near.model);
                    effects.input[fx::IN_DRAWS] = (draws + 1) as f64;
                }
            }
        }
        // WaterShore.publishFrame: the lapping foam and splash along the
        // waterline, while the water is in view at High or Ultra.
        if let Some(batches) = self.shore.filter(|_| self.shore_quality > 0. && seen[0]) {
            let result = &self.phase.result;
            let count = (result[out::SHORE_COUNT] as usize).min(SHORE_PATCHES);
            let dirty = result[out::SHORE_DIRTY] != 0.;
            let time = result[out::SHORE_TIME];
            let fields = kinds::PRODUCERS[kinds::WATER_SHORE].1;
            for (k, handle) in batches.into_iter().enumerate() {
                let effects = &mut bridge.r.effects;
                let draws = effects.input[fx::IN_DRAWS] as usize;
                if count == 0 || draws == fx::MAX_DRAWS {
                    continue;
                }
                // The batch keeps the patches it was given: send them again
                // only when the shore moved, or it holds another number.
                let from = if dirty || self.shore_held[k] != count { 0 } else { count };
                self.shore_held[k] = count;
                let start = data_at + self.shore_at + k * SHORE_BATCH;
                let mut at = start;
                let shore = &self.phase.shore;
                for (source, size) in [(&shore.points, 4), (&shore.phases, 1)] {
                    effects.data[at..at + (count - from) * size].copy_from_slice(&source[from * size..count * size]);
                    at += (count - from) * size;
                }
                let mut put = |name: &str, value: &[f64]| {
                    if let Some((offset, size)) = w::field(fields, name) {
                        for (to, from) in effects.data[at + offset..at + offset + size].iter_mut().zip(value) {
                            *to = *from as f32;
                        }
                    }
                };
                put("time", &[time]);
                put("height", &[self.phase.config[cfg::HEIGHT]]);
                put("splash", &[k as f64]);
                put("modelMatrix", &world);
                put("projectionMatrix", &view[0..16]);
                put("viewMatrix", &view[16..32]);
                put("cameraPosition", &view[member("cameraPosition")..member("cameraPosition") + 3]);
                let entry = &mut effects.input[fx::DRAW_AT + draws * fx::DRAW_STRIDE..][..fx::DRAW_STRIDE];
                entry.fill(0.);
                entry[..6].copy_from_slice(&[fx::KIND_BATCH as f64, f64::from(handle), count as f64, from as f64, start as f64, SHORE_PATCHES as f64]);
                entry[DRAW_KEY + 1] = -17.;
                entry[DRAW_KEY + 2..DRAW_KEY + 5].copy_from_slice(&[0., wave, 0.]);
                entry[DRAW_KEY + 5] = f64::from(handle);
                effects.input[fx::IN_DRAWS] = (draws + 1) as f64;
            }
        }
        let after = bridge.r.effects.input[fx::IN_DRAWS] as usize;
        self.appended = (after > first).then_some((first, after));
    }

    /// The one step of an underwater screen pass, written into the screen
    /// input as the page's ScreenPass wrote it (post.rs): the Low tint's
    /// blended colour, or the Enhanced fog's reads of the scene copy.
    pub fn screen(&self, bridge: &mut Bridge, fog: bool, time: f64) {
        use bfe_render::post as post;
        let step = &mut bridge.r.post.input[..post::STEP];
        step.fill(0.);
        let range = &bridge.r.cameras.input[cl::input::LENS..];
        let range = [range[cl::lens::NEAR], range[cl::lens::FAR]];
        // Each uniform: its type and up to four values.
        let mut rows = [(0u32, [0.; 4], 0usize); 5];
        let program = if fog {
            let scene = self.surface.as_ref().map_or([-1., -1.], |s| [f64::from(s.colour.texture), f64::from(s.depth_texture)]);
            rows = [
                (2, [range[0], range[1], 0., 0.], 2),
                (3, [self.tint[0], self.tint[1], self.tint[2], 0.], 3),
                (1, [time, 0., 0., 0.], 1),
                (post::TYPE_SAMPLER, [scene[0], 0., 0., 0.], 1),
                (post::TYPE_SAMPLER, [scene[1], 0., 0., 0.], 1),
            ];
            self.screens[1]
        } else {
            rows[0] = (4, self.underwater, 4);
            self.screens[0]
        };
        let uniforms = &rows[..if fog { 5 } else { 1 }];
        step[post::STEP_PROGRAM] = f64::from(program);
        step[post::STEP_TARGET] = post::TARGET_BOUND;
        step[post::STEP_WIDTH] = f64::from(self.canvas[0]);
        step[post::STEP_HEIGHT] = f64::from(self.canvas[1]);
        step[post::STEP_UNIFORMS] = uniforms.len() as f64;
        for (i, (kind, values, count)) in uniforms.iter().enumerate() {
            let at = post::STEP_FIRST + i * post::UNIFORM;
            step[at] = i as f64;
            step[at + 1] = f64::from(*kind);
            step[at + 2..at + 2 + count].copy_from_slice(&values[..*count]);
        }
        // The Low tint blends over the frame; the fog replaces it.
        let state = &mut bridge.r.post.states[..post::STATE];
        state.fill(0.);
        if !fog {
            state[post::STATE_FLAGS] = f64::from(post::STATE_BLEND);
            state[post::STATE_EQUATIONS..post::STATE_EQUATIONS + 2].copy_from_slice(&[32774., 32774.]);
            state[post::STATE_FACTORS..post::STATE_FACTORS + 4].copy_from_slice(&[770., 771., 1., 771.]);
        }
    }

    /// The water's clock, for the screen passes.
    pub fn time(&self) -> f64 {
        self.phase.result[out::TIME]
    }

    // The frame blocks of the mirrored views: the main frame's with the
    // mirrored camera's projection, view and position.
    fn mirror_views(&mut self, bridge: &Bridge, view: &[f64]) {
        let cameras = &bridge.r.cameras;
        for (block, role) in [(&mut self.plan.plain, cl::role::MIRROR_PLAIN), (&mut self.plan.clipped, cl::role::MIRROR)] {
            block.copy_from_slice(view);
            let row = &cameras.output[role * cl::camera::LEN..(role + 1) * cl::camera::LEN];
            block[0..16].copy_from_slice(&row[cl::camera::PROJECTION..cl::camera::PROJECTION + 16]);
            block[16..32].copy_from_slice(&row[cl::camera::INVERSE..cl::camera::INVERSE + 16]);
            block[32..35].copy_from_slice(&row[cl::camera::POSITION..cl::camera::POSITION + 3]);
        }
    }
}

// WaterShore's collision queries (collect and its exposed probe): the
// battle's static triangles and the terrain's about a tile, and a downward
// cast to learn whether a point lies over open water.
struct Shore<'a> {
    collision: &'a crate::online::replay::Collision,
    world: usize,
    state: &'a bfe_sim::units::water::WaterState,
    gathered: &'a mut bfe_sim::collision::spheres::Gathered,
    terrain: &'a mut Vec<u32>,
}
impl bfe_environment::shore::Queries for Shore<'_> {
    fn triangles(&mut self, x: f64, z: f64, height: f64, output: &mut [f64]) -> usize {
        use bfe_sim::collision::spheres;
        // A tile is 8 m square: gather about its centre.
        let (tx, tz) = (x * 8., z * 8.);
        let center = [tx + 4., height, tz + 4.];
        let mut owner = self.collision.borrow_mut();
        let state = &mut owner.state;
        spheres::gather_fixed(state, self.world, center, 4., None, None, self.gathered);
        let stride = bfe_environment::layout::shore_triangle::STRIDE;
        let mut count = 0;
        let mut put = |output: &mut [f64], a: [f64; 3], b: [f64; 3], c: [f64; 3]| {
            let at = count * stride;
            if at + stride <= output.len() {
                output[at..at + 3].copy_from_slice(&a);
                output[at + 3..at + 6].copy_from_slice(&b);
                output[at + 6..at + 9].copy_from_slice(&c);
            }
            count += 1;
        };
        for polygon in self.gathered.list() {
            for t in &polygon.tris {
                put(output, t.a, t.b, t.c);
            }
        }
        // The terrain's triangles that cross the tile's slab of the surface.
        if let Some(grid) = state.worlds[self.world].grid {
            spheres::terrain_near(state, grid, center, 5.7, self.terrain);
            for &t in self.terrain.iter() {
                let (a, b, c) = spheres::terrain_triangle(state, grid, t as usize);
                let (lo, hi) = (a[1].min(b[1]).min(c[1]), a[1].max(b[1]).max(c[1]));
                let (x0, x1) = (a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]));
                let (z0, z1) = (a[2].min(b[2]).min(c[2]), a[2].max(b[2]).max(c[2]));
                if lo <= height + 0.1 && hi >= height - 0.1 && x1 >= tx && x0 <= tx + 8. && z1 >= tz && z0 <= tz + 8. {
                    put(output, a, b, c);
                }
            }
        }
        count
    }
    // host_shore_exposed: the highest surface under a point 12 m above the
    // water within 30 m, against the water's own level less 8 cm.
    fn exposed(&mut self, x: f64, z: f64, height: f64) -> bool {
        if !self.state.over(x, z) {
            return false;
        }
        let ray = bfe_sim::collision::Ray { ox: x, oy: height + 12., oz: z, dx: 0., dy: -1., dz: 0. };
        let ground = match self.collision.borrow_mut().ray_hit(self.world as u32, ray, 30., -1) {
            Some((_, hit)) => height + 12. - hit.distance,
            None => f64::NEG_INFINITY,
        };
        ground < height - 0.08
    }
}

// Maps and views without a battle's collision lap no shore.
struct NoShore;
impl bfe_environment::shore::Queries for NoShore {
    fn triangles(&mut self, _: f64, _: f64, _: f64, _: &mut [f64]) -> usize {
        0
    }
    fn exposed(&mut self, _: f64, _: f64, _: f64) -> bool {
        false
    }
}
