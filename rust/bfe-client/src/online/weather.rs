//! The map's rain (weather.js, rain-impacts.js, rain-sheets.js) over the
//! shared Weather state of bfe-environment: falling drops placed by the
//! vertex shader from fixed seeds and the rain's clock, cut off under roofs
//! by a small cache of cover heights. rainEffects picks the look: Classic is
//! the map's own density and range; Enhanced is denser and wider, with a
//! ground mist, splashes where drops land and streak sheets in the middle
//! distance.
use super::replay::Collision;
use crate::render_bridge::Bridge;
use bfe_content_native::BattleContent;
use bfe_environment::weather::{self as rain, Impact, Weather};
use bfe_render::{effect_layout as layout, effects as render};

const IMPACTS: usize = 96 * 2;
const MIST: usize = 48;
const SHEETS_SIZE: usize = 256;

// The map's precipitation (world.weather): what both looks start from.
struct Data {
    range: f64,
    count: usize,
    capacity: usize,
    wind: [f64; 2],
    velocity: f64,
    velocity_range: f64,
    width: f64,
    streak: f64,
    color: [f64; 3],
    alpha: [f64; 2],
}

pub struct Rain {
    pub enhanced: bool,
    data: Data,
    state: Weather,
    seeds: Vec<f32>,
    drops: u32,
    mist: u32,
    sheets: u32,
    impacts: u32,
    // The cover cache's texture: its source and sample, and the renderer's
    // id for it.
    cover: (u32, u32, u32),
    cover_size: usize,
    at: usize,
    collision: Collision,
    world: u32,
    water: Option<bfe_sim::units::water::WaterState>,
    /// Frames the rain was drawn.
    pub frames: u64,
}

fn number(v: &serde_json::Value, key: &str, fallback: f64) -> f64 {
    v[key][0].as_f64().or_else(|| v[key].as_f64()).unwrap_or(fallback)
}

impl Rain {
    /// None on a map without rain (no precipitation, or snow's Quads).
    /// `at`: where its floats start in the effect data; it reserves them.
    pub fn new(content: &BattleContent, bridge: &mut Bridge, gpu: &mut crate::resources::Resources, collision: Collision, world: u32, at: usize) -> Result<Option<Self>, String> {
        let weather = &content.map["weather"];
        let p = &weather["precipitation"];
        if !p.is_object() || p["Type"][0].as_str().is_some_and(|t| t.eq_ignore_ascii_case("quads")) {
            return Ok(None);
        }
        let range = number(p, "Range", 12.5).clamp(4., 40.);
        let count = (number(p, "ParticleDensity", 256.).round() as usize).clamp(32, 2048);
        let color = p["Color"].as_array().map_or([216., 220., 228.], |c| std::array::from_fn(|i| c.get(i).and_then(|v| v.as_f64()).unwrap_or(0.)));
        let alpha = p["AlphaMinMax"].as_array().map_or([0.3, 0.45], |a| std::array::from_fn(|i| a.get(i).and_then(|v| v.as_f64()).unwrap_or(0.)));
        // Enhanced draws up to 32 times the map's drops (Weather.capacity).
        let capacity = (count * 32).min(8192);
        let data = Data {
            range,
            count,
            capacity,
            wind: [weather["wind"][0].as_f64().unwrap_or(0.), weather["wind"][1].as_f64().unwrap_or(0.)],
            velocity: number(p, "Velocity", 9.),
            velocity_range: number(p, "VelocityRange", 1.),
            width: number(p, "ParticleSize", 0.02),
            streak: number(p, "StreakLength", 1.),
            color: color.map(|c| c / 255.),
            alpha,
        };
        let mut seeds = vec![0.; capacity * 4];
        rain::seeds(&mut seeds);
        let shadows = bridge.shadow_cascades() > 0;
        // The cover cache: a square of float heights, nearest sampling.
        let cover_size = 32;
        let source = gpu.source(&cover_desc(cover_size))?;
        gpu.source_pixels(source, &vec![0u8; cover_size * cover_size * 4])?;
        let sample = gpu.sample(source, NEAREST)?;
        gpu.upload(sample)?;
        let cover_texture = bridge.r.core.add_texture(gpu.texture(sample)?);
        // The sheets' streak texture, made once.
        let mut streaks = vec![0u8; SHEETS_SIZE * SHEETS_SIZE * 4];
        rain::streaks(&mut streaks);
        let streak_source = gpu.source(
            &serde_json::json!({"width":SHEETS_SIZE,"height":SHEETS_SIZE,"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1009,"srgb":false}).to_string(),
        )?;
        gpu.source_pixels(streak_source, &streaks)?;
        let streak_sample = gpu.sample(
            streak_source,
            r#"{"wrapS":1000,"wrapT":1000,"wrapR":1000,"min":1006,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#,
        )?;
        gpu.upload(streak_sample)?;
        let streak_texture = bridge.r.core.add_texture(gpu.texture(streak_sample)?);
        let cover = cover_texture as i32;
        let drops = bridge.effect_batch(layout::RAIN as u32, 0, shadows, -1, capacity as u32, -1, &[cover])?;
        let mist = bridge.effect_batch(layout::RAIN as u32, 0, shadows, -1, MIST as u32, -1, &[cover])?;
        let impacts = bridge.effect_batch(layout::IMPACTS as u32, 0, shadows, -1, IMPACTS as u32, -1, &[])?;
        let sheets = bridge.effect_batch(layout::RAIN_SHEETS as u32, 0, shadows, -1, 1, -1, &[streak_texture as i32, cover])?;
        let rain = Self {
            enhanced: true,
            state: Weather::new(true, (range * 3.2).min(40.), 32),
            data,
            seeds,
            drops,
            mist,
            sheets,
            impacts,
            cover: (source, sample, cover_texture),
            cover_size,
            at,
            collision,
            world,
            water: super::water::new(content),
            frames: 0,
        };
        bridge.fx_reserve((at + rain.floats()) as u32);
        Ok(Some(rain))
    }

    fn floats(&self) -> usize {
        // Drops and mist: seeds then 23 values; splashes: 8 floats each and
        // one value; sheets: 16 values.
        (self.data.capacity * 4 + 23) + (MIST * 4 + 23) + (IMPACTS * 8 + 1) + 16
    }

    /// The end of its floats in the effect data.
    pub fn end(&self) -> usize {
        self.at + self.floats()
    }

    // Weather.applyMode: Enhanced widens the rain and fills a finer cover
    // cache; either way the cache is filled whole on the next frame.
    fn range(&self) -> f64 {
        if self.enhanced {
            (self.data.range * 3.2).min(40.)
        } else {
            self.data.range
        }
    }

    /// Weather.update once the camera is posed, and its draws.
    /// `camera`: the frame's projection then view matrix (unused beyond
    /// the eye today), `eye` the camera's position.
    pub fn frame(&mut self, dt: f64, eye: [f64; 3], gpu: &mut crate::resources::Resources, bridge: &mut Bridge) {
        let size = if self.enhanced { 32 } else { 16 };
        if size != self.cover_size || (self.state.cover.size != size) {
            self.cover_size = size;
            self.state.set_mode(self.enhanced, self.range(), size);
        }
        let mut frame = [0.; bfe_environment::layout::weather_frame::STRIDE];
        frame[bfe_environment::layout::weather_frame::DT] = dt;
        frame[bfe_environment::layout::weather_frame::POSITION..][..3].copy_from_slice(&eye);
        let mut result = [0.; bfe_environment::layout::weather_result::STRIDE];
        let (collision, world, water) = (&self.collision, self.world, &self.water);
        let down = |x: f64, y: f64, z: f64| {
            let ray = bfe_sim::collision::Ray { ox: x, oy: y, oz: z, dx: 0., dy: -1., dz: 0. };
            collision.borrow_mut().ray_hit(world, ray, 512., -1).map(|(_, h)| h)
        };
        let over = |x: f64, z: f64| water.as_ref().filter(|w| w.over(x, z)).map(|w| w.level());
        let mut height = |x: f64, y: f64, z: f64| {
            let floor = down(x, y, z).map_or(-100000., |h| h.py);
            over(x, z).map_or(floor, |level| floor.max(level))
        };
        let mut cast = |x: f64, y: f64, z: f64| {
            let hit = down(x, y, z);
            let (mut y, normal) = hit.map_or((f64::NEG_INFINITY, [0., 1., 0.]), |h| (h.py, [h.nx, h.ny, h.nz]));
            let level = over(x, z);
            let water = level.is_some_and(|l| l >= y);
            if let Some(level) = level {
                y = y.max(level);
            }
            Impact { y, normal, water }
        };
        self.state.update(&frame, &mut height, &mut cast, &mut result);
        use bfe_environment::layout::weather_result as wr;
        if result[wr::CHANGED] != 0. {
            let cells = self.cover_size * self.cover_size;
            let bytes: Vec<u8> = self.state.cover.heights[..cells].iter().flat_map(|h| h.to_le_bytes()).collect();
            let resized = gpu.source_desc(self.cover.0, &cover_desc(self.cover_size)).is_ok();
            if resized && gpu.source_pixels(self.cover.0, &bytes).is_ok() && gpu.upload(self.cover.1).is_ok() {
                if let Ok(texture) = gpu.texture(self.cover.1) {
                    let _ = bridge.r.core.replace_sample_texture(self.cover.2 as usize, texture);
                }
            }
        }
        if result[wr::COVERED] == 0. {
            return;
        }
        self.frames += 1;
        let (enhanced, range) = (self.enhanced, self.range());
        let count = if enhanced { self.data.capacity } else { self.data.count.min(self.data.capacity) };
        let cell = self.state.cover.cell;
        let d = &self.data;
        let values = |effect: f64| -> [f32; 23] {
            let mut v = [0f32; 23];
            let list = [
                eye[0], eye[1], eye[2],
                result[wr::CAMERA_MOTION], result[wr::CAMERA_MOTION + 1], result[wr::CAMERA_MOTION + 2],
                d.wind[0], d.wind[1], result[wr::RAIN_TIME], range, d.velocity, d.velocity_range, d.width, d.streak,
                enhanced as u8 as f64, effect, cell, self.cover_size as f64, d.color[0], d.color[1], d.color[2], d.alpha[0], d.alpha[1],
            ];
            for (slot, value) in v.iter_mut().zip(list) {
                *slot = value as f32;
            }
            v
        };
        let mut draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        let mut draw = |bridge: &mut Bridge, batch: u32, count: usize, at: usize, order: f64| {
            if draws == render::MAX_DRAWS {
                return;
            }
            let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
            row.fill(0.);
            row[..6].copy_from_slice(&[render::KIND_BATCH as f64, batch as f64, count as f64, 0., at as f64, count as f64]);
            row[render::DRAW_KEY + 1] = order;
            row[render::DRAW_KEY + 5] = batch as f64;
            draws += 1;
        };
        // The drops: their seeds, then the values.
        let drops_at = self.at;
        let data = &mut bridge.r.effects.data;
        data[drops_at..drops_at + count * 4].copy_from_slice(&self.seeds[..count * 4]);
        data[drops_at + count * 4..drops_at + count * 4 + 23].copy_from_slice(&values(0.));
        draw(bridge, self.drops, count, drops_at, 20.);
        if enhanced {
            let mist_at = drops_at + self.data.capacity * 4 + 23;
            let data = &mut bridge.r.effects.data;
            data[mist_at..mist_at + MIST * 4].copy_from_slice(&self.seeds[..MIST * 4]);
            data[mist_at + MIST * 4..mist_at + MIST * 4 + 23].copy_from_slice(&values(1.));
            draw(bridge, self.mist, MIST, mist_at, 20.);
            // The splashes: where and when each landed, its surface's
            // normal, and which of a pair it is (ring or droplet).
            let impacts_at = mist_at + MIST * 4 + 23;
            let impacts = &self.state.impacts;
            let data = &mut bridge.r.effects.data;
            data[impacts_at..impacts_at + IMPACTS * 4].copy_from_slice(&impacts.locations);
            data[impacts_at + IMPACTS * 4..impacts_at + IMPACTS * 7].copy_from_slice(&impacts.normals);
            for i in 0..IMPACTS {
                data[impacts_at + IMPACTS * 7 + i] = (i % 2) as f32;
            }
            data[impacts_at + IMPACTS * 8] = result[wr::IMPACT_TIME] as f32;
            draw(bridge, self.impacts, IMPACTS, impacts_at, 21.);
            // The sheets: one draw of three cylinders around the eye.
            let sheets_at = impacts_at + IMPACTS * 8 + 1;
            let list = [
                eye[0], eye[1], eye[2], result[wr::RAIN_TIME], d.wind[0], d.wind[1], d.color[0], d.color[1], d.color[2],
                d.velocity.mul_add(1.4, 0.).max(6.), 4., 9., 18., 1., cell, self.cover_size as f64,
            ];
            for (k, value) in list.into_iter().enumerate() {
                bridge.r.effects.data[sheets_at + k] = value as f32;
            }
            draw(bridge, self.sheets, 1, sheets_at, 19.);
        }
        bridge.r.effects.input[render::IN_DRAWS] = draws as f64;
    }
}

const NEAREST: &str = r#"{"wrapS":1001,"wrapT":1001,"wrapR":1001,"min":1003,"mag":1003,"anisotropy":1,"flip":false,"premultiply":false,"alignment":4,"mipmaps":false,"compare":null}"#;

// A square of one float per texel (RedFormat, FloatType).
fn cover_desc(size: usize) -> String {
    serde_json::json!({"width":size,"height":size,"depth":1,"target":glow::TEXTURE_2D,"format":1028,"dataType":1015,"srgb":false}).to_string()
}

/// The map's snow (snow.js) over the shared Snow state: flakes in a 5 m
/// box repeated through every grid cell in view, each flake's position and
/// turn in a small float texture, the cells in another, cut off under
/// roofs by the same kind of cover cache as the rain. Snow has one look.
pub struct Snow {
    phase: bfe_environment::SnowPhase,
    count: usize,
    box_limit: usize,
    size: f64,
    tint: [f64; 4],
    batch: u32,
    // Source, sample and renderer id of the flakes', boxes' and cover's
    // textures.
    flakes: (u32, u32, u32),
    boxes: (u32, u32, u32),
    cover: (u32, u32, u32),
    at: usize,
    collision: Collision,
    world: u32,
    water: Option<bfe_sim::units::water::WaterState>,
    /// Frames the snow was drawn.
    pub frames: u64,
}

fn rgba_desc(width: usize, height: usize) -> String {
    serde_json::json!({"width":width,"height":height,"depth":1,"target":glow::TEXTURE_2D,"format":1023,"dataType":1015,"srgb":false}).to_string()
}

fn floats(gpu: &mut crate::resources::Resources, bridge: &mut Bridge, desc: &str, bytes: usize) -> Result<(u32, u32, u32), String> {
    let source = gpu.source(desc)?;
    gpu.source_pixels(source, &vec![0u8; bytes])?;
    let sample = gpu.sample(source, NEAREST)?;
    gpu.upload(sample)?;
    Ok((source, sample, bridge.r.core.add_texture(gpu.texture(sample)?)))
}

fn refresh(gpu: &mut crate::resources::Resources, bridge: &mut Bridge, texture: (u32, u32, u32), values: &[f32]) {
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    if gpu.source_pixels(texture.0, &bytes).is_ok() && gpu.upload(texture.1).is_ok() {
        if let Ok(gl) = gpu.texture(texture.1) {
            let _ = bridge.r.core.replace_sample_texture(texture.2 as usize, gl);
        }
    }
}

impl Snow {
    /// None on a map without snow (precipitation of Type Quads).
    pub fn new(
        content: &mut BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
        gpu: &mut crate::resources::Resources,
        collision: Collision,
        world: u32,
        at: usize,
    ) -> Result<Option<Self>, String> {
        let weather = content.map["weather"].clone();
        let p = &weather["precipitation"];
        if !p["Type"][0].as_str().is_some_and(|t| t.eq_ignore_ascii_case("quads")) {
            return Ok(None);
        }
        let (phase, recipe) = bfe_environment::admission::snow(&weather);
        use bfe_environment::layout::snow_config as sc;
        let count = phase.config[sc::COUNT] as usize;
        let box_limit = phase.config[sc::BOX_LIMIT] as usize;
        let _ = recipe;
        let color = p["Color"].as_array().map_or([192., 192., 192., 160.], |c| std::array::from_fn(|i| c.get(i).and_then(|v| v.as_f64()).unwrap_or(255.)));
        // Its flake texture, from the map's textures; none draws plain quads.
        let map = p["Texture"][0]
            .as_str()
            .and_then(|name| content.texture_paths.get(&name.to_lowercase()).cloned())
            .and_then(|path| content.textures.load(&path).ok())
            .and_then(|image| textures.get(image).copied())
            .map_or(-1, |t| t as i32);
        let shadows = bridge.shadow_cascades() > 0;
        let flakes = floats(gpu, bridge, &rgba_desc(count, 2), count * 2 * 16)?;
        let boxes = floats(gpu, bridge, &rgba_desc(box_limit, 1), box_limit * 16)?;
        let cover = floats(gpu, bridge, &cover_desc(16), 16 * 16 * 4)?;
        let batch = bridge.effect_batch(layout::SNOW as u32, 0, shadows, map, (count * box_limit) as u32, -1, &[flakes.2 as i32, boxes.2 as i32, cover.2 as i32])?;
        bridge.fx_reserve((at + 16) as u32);
        Ok(Some(Self {
            phase,
            count,
            box_limit,
            size: number(p, "ParticleSize", 0.02),
            tint: [color[0] / 255., color[1] / 255., color[2] / 255., color[3] / 255.],
            batch,
            flakes,
            boxes,
            cover,
            at,
            collision,
            world,
            water: super::water::new(content),
            frames: 0,
        }))
    }

    pub fn end(&self) -> usize {
        self.at + 16
    }

    /// Snow.update and its draw. `camera` is the frame's projection, then
    /// view matrix; `fov` and `aspect` its lens.
    pub fn frame(&mut self, dt: f64, eye: [f64; 3], camera: &[f64], gpu: &mut crate::resources::Resources, bridge: &mut Bridge) {
        use bfe_environment::layout::{snow_result as sr, view as v};
        let view = &mut self.phase.view;
        view[v::DT] = dt;
        let mut inverse = three::Matrix4::default();
        inverse.elements.copy_from_slice(&camera[16..32]);
        let mut world = inverse;
        world.invert();
        view[v::CAMERA..v::CAMERA + 16].copy_from_slice(&world.elements);
        view[v::INVERSE..v::INVERSE + 16].copy_from_slice(&inverse.elements);
        view[v::PROJECTION..v::PROJECTION + 16].copy_from_slice(&camera[..16]);
        // three's PerspectiveCamera fov (vertical, degrees) and aspect.
        view[v::FOV] = 2. * (1. / camera[5]).atan().to_degrees();
        view[v::ASPECT] = camera[5] / camera[0];
        view[v::POSITION..v::POSITION + 3].copy_from_slice(&eye);
        let (collision, world_id, water) = (&self.collision, self.world, &self.water);
        let mut height = |x: f64, y: f64, z: f64| {
            let ray = bfe_sim::collision::Ray { ox: x, oy: y, oz: z, dx: 0., dy: -1., dz: 0. };
            let floor = collision.borrow_mut().ray_hit(world_id, ray, 512., -1).map_or(-100000., |(_, h)| h.py);
            water.as_ref().filter(|w| w.over(x, z)).map_or(floor, |w| floor.max(w.level()))
        };
        self.phase.update(&mut height);
        let result = &self.phase.result;
        let Some(snow) = self.phase.snow.as_ref() else {
            return;
        };
        if result[sr::CHANGED] != 0. {
            let cells = snow.cover.size * snow.cover.size;
            refresh(gpu, bridge, self.cover, &snow.cover.heights[..cells]);
        }
        let boxes = (result[sr::BOXES] as usize).min(self.box_limit);
        if result[sr::VISIBLE] == 0. || boxes == 0 {
            return;
        }
        refresh(gpu, bridge, self.flakes, &snow.flake_data);
        refresh(gpu, bridge, self.boxes, &snow.box_data);
        let list = [
            self.count as f64, self.size,
            result[sr::CAMERA_RIGHT], result[sr::CAMERA_RIGHT + 1], result[sr::CAMERA_RIGHT + 2],
            result[sr::CAMERA_UP], result[sr::CAMERA_UP + 1], result[sr::CAMERA_UP + 2],
            self.tint[0], self.tint[1], self.tint[2], self.tint[3],
            snow.cover.cell, snow.cover.size as f64,
        ];
        for (k, value) in list.into_iter().enumerate() {
            bridge.r.effects.data[self.at + k] = value as f32;
        }
        let draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        if draws == render::MAX_DRAWS {
            return;
        }
        let instances = boxes * self.count;
        let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
        row.fill(0.);
        row[..6].copy_from_slice(&[render::KIND_BATCH as f64, self.batch as f64, instances as f64, 0., self.at as f64, instances as f64]);
        row[render::DRAW_KEY + 1] = 20.;
        row[render::DRAW_KEY + 5] = self.batch as f64;
        bridge.r.effects.input[render::IN_DRAWS] = (draws + 1) as f64;
        self.frames += 1;
    }
}
