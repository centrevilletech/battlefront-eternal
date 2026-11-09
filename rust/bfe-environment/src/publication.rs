//! Numeric draw publication formerly assembled by the page's effect classes.
//! Values are retained at admission and borrowed directly by upload glue.
//! The native phase selects modes, visibility, texture-frame slots and counts;
//! browser identity caches never decide those presentation rules.
use crate::effect_layout::{
    fountain_water as fountain, rain as rain_row, rain_sheets as sheets, snow as snow_row, square,
    water_shore as shore,
};
use crate::layout::{
    atmosphere_stats as ast, environment_draw as draw, environment_result as er,
    page_frame as frame, sky_publication as sky_row, sky_result, snow_result as sr,
    square_batch as sb, weather_mode as mode, weather_result as wr,
};
use serde_json::{json, Value};
fn number(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or(0.)
}
fn copy(out: &mut [f64], at: usize, v: &Value) {
    for (i, n) in v.as_array().into_iter().flatten().enumerate() {
        if at + i >= out.len() {
            break;
        }
        out[at + i] = n.as_f64().unwrap_or(0.);
    }
}
// Generated layouts describe these retained blocks to both the environment
// phase and the renderer. Their addresses remain stable after cold admission.
// Memory growth replaces browser views, never these numerical publications.
pub struct Publication {
    empty: [f64; crate::effect_layout::water_reflection::LENGTH],
    pub fountain_models: Vec<[f64; crate::layout::fountain_model::STRIDE]>,
    original_models: Vec<[f64; crate::layout::fountain_model::STRIDE]>,
    fountain_enabled: bool,
    pub input: [f64; frame::STRIDE],
    pub weather_mode: [f64; mode::STRIDE],
    rain: [f64; rain_row::LENGTH],
    mist: [f64; rain_row::LENGTH],
    sheets: [f64; sheets::LENGTH],
    snow: [f64; snow_row::LENGTH],
    squares: Vec<[f64; square::LENGTH]>,
    pub fountain: [f64; fountain::LENGTH],
    pub shore: [[f64; shore::LENGTH]; 2],
    pub sky: Vec<[f64; sky_row::STRIDE]>,
    weather_range: [f64; 2],
    weather_count: f64,
    weather_capacity: f64,
    stepped: bool,
    motion_frame: f64,
    pub still: f64,
}
impl Default for Publication {
    fn default() -> Self {
        Self::new()
    }
}
impl Publication {
    // Before its first motion render, a new sky has consumed no observation.
    // The negative frame sentinel preserves the renderer frame-zero case.
    pub fn new() -> Self {
        Self {
            empty: [0.; crate::effect_layout::water_reflection::LENGTH],
            fountain_models: vec![],
            original_models: vec![],
            fountain_enabled: false,
            input: [0.; frame::STRIDE],
            weather_mode: [0.; mode::STRIDE],
            rain: [0.; rain_row::LENGTH],
            mist: [0.; rain_row::LENGTH],
            sheets: [0.; sheets::LENGTH],
            snow: [0.; snow_row::LENGTH],
            squares: vec![],
            fountain: [0.; fountain::LENGTH],
            shore: [[0.; shore::LENGTH]; 2],
            sky: vec![],
            weather_range: [0.; 2],
            weather_count: 0.,
            weather_capacity: 0.,
            stepped: false,
            motion_frame: -1.,
            still: 1.,
        }
    }
    // Weather.applyMode chooses the count and cache shape in this owner. All
    // mode storage was reserved at load, so repeated changes do not allocate.
    pub fn weather(&mut self, data: &Value, enhanced: bool) -> Value {
        let r = crate::content::recipe(crate::layout::environment_recipe::WEATHER as u32, data);
        // Mode changes retain immutable seed capacity and both authored reaches.
        // They must not restart the drop or splash random streams.
        self.weather_range = [number(&r, "range"), number(&r, "enhancedRange")];
        self.weather_count = number(&r, "count");
        self.weather_capacity = number(&r, "capacity");
        for (key, at) in [
            ("wind", rain_row::WIND),
            ("alpha", rain_row::ALPHA),
            ("color", rain_row::COLOR),
        ] {
            copy(&mut self.rain, at, &r[key]);
        }
        for (key, at) in [
            ("velocity", rain_row::VELOCITY),
            ("velocityRange", rain_row::VELOCITY_RANGE),
            ("width", rain_row::WIDTH),
            ("streak", rain_row::STREAK),
        ] {
            self.rain[at] = number(&r, key);
        }
        self.sheets[sheets::SPEED] = number(&r, "sheetSpeed");
        self.sheets[sheets::RADII..sheets::RADII + 3].copy_from_slice(&[4., 9., 18.]);
        self.sheets[sheets::STRENGTH] = 1.;
        self.mode(enhanced);
        r
    }
    // Cover::reset includes an eight-metre guard beyond the visible range.
    // Both shader addressing and collision cells must use that exact width.
    // Only cold mode changes select these values; ordinary frames reuse them.
    pub fn mode(&mut self, enhanced: bool) {
        let size = if enhanced { 32. } else { 16. };
        let range = self.weather_range[usize::from(enhanced)];
        let cell = (range * 2. + 8.) / size;
        self.weather_mode = [
            f64::from(enhanced),
            range,
            if enhanced {
                self.weather_capacity
            } else {
                self.weather_count
            },
            self.weather_capacity,
            size,
            cell,
        ];
        let r = &mut self.rain;
        r[rain_row::ENHANCED] = f64::from(enhanced);
        r[rain_row::RANGE] = range;
        r[rain_row::COVER_CELL] = cell;
        r[rain_row::COVER_SIZE] = size;
        // Mist differs only by its effect selector. Complete its retained row
        // now, so a paused draw immediately after a setting change is valid.
        self.mist = *r;
        self.mist[rain_row::EFFECT] = 1.;
        self.sheets[sheets::COVER_CELL] = cell;
        self.sheets[sheets::COVER_SIZE] = size;
        self.sheets[sheets::WIND..sheets::WIND + 2]
            .copy_from_slice(&r[rain_row::WIND..rain_row::WIND + 2]);
        self.sheets[sheets::COLOR..sheets::COLOR + 3]
            .copy_from_slice(&r[rain_row::COLOR..rain_row::COLOR + 3]);
    }
    // RainSheets.publishFrame now shares these native clock/eye records. No
    // JavaScript loop reconstructs the sheet uniforms on every frame.
    pub fn weather_frame(&mut self, r: &[f64]) {
        self.rain[rain_row::RAIN_TIME] = r[wr::RAIN_TIME];
        self.rain[rain_row::EYE..rain_row::EYE + 3].copy_from_slice(&r[wr::EYE..wr::EYE + 3]);
        self.rain[rain_row::CAMERA_MOTION..rain_row::CAMERA_MOTION + 3]
            .copy_from_slice(&r[wr::CAMERA_MOTION..wr::CAMERA_MOTION + 3]);
        self.mist = self.rain;
        self.mist[rain_row::EFFECT] = 1.;
        self.sheets[sheets::RAIN_TIME] = r[wr::RAIN_TIME];
        self.sheets[sheets::EYE..sheets::EYE + 3].copy_from_slice(&r[wr::EYE..wr::EYE + 3]);
    }
    // Snow starts with world right/up before the first camera observation.
    // Its later normalized basis is published with the repeated native boxes.
    // Flake size, tint and cache dimensions stay fixed for this admission.
    pub fn snow(&mut self, r: &Value, cell: f64) {
        let v = &mut self.snow;
        v[snow_row::FLAKE_COUNT] = number(r, "count");
        v[snow_row::SIZE] = number(r, "size");
        copy(v, snow_row::TINT, &r["tint"]);
        v[snow_row::CAMERA_RIGHT] = 1.;
        v[snow_row::CAMERA_UP + 1] = 1.;
        v[snow_row::COVER_SIZE] = number(r, "coverSize");
        v[snow_row::COVER_CELL] = cell;
    }
    // These are the same axes used by the native repeated-box placement.
    // Deriving another camera basis on the page would separate the two draws.
    pub fn snow_frame(&mut self, r: &[f64]) {
        self.snow[snow_row::CAMERA_RIGHT..snow_row::CAMERA_RIGHT + 3]
            .copy_from_slice(&r[sr::CAMERA_RIGHT..sr::CAMERA_RIGHT + 3]);
        self.snow[snow_row::CAMERA_UP..snow_row::CAMERA_UP + 3]
            .copy_from_slice(&r[sr::CAMERA_UP..sr::CAMERA_UP + 3]);
    }
    // SquareBatch starts with camera-aligned axes and its authored tint.
    // Subsequent axes come from the exact phase batch that produced its rows.
    pub fn atmosphere(&mut self, r: &Value) {
        self.squares = r["batches"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|b| {
                let mut v = [0.; square::LENGTH];
                copy(&mut v, square::TINT, &b["tint"]);
                v[square::AXIS_U] = 1.;
                v[square::AXIS_V + 1] = 1.;
                v
            })
            .collect();
    }
    // Batch order is fixed: optional fog first, then authored dust classes.
    // Dust can flatten its axes independently of fog, so each texture needs
    // the exact basis produced together with its sorted centre/spin records.
    pub fn atmosphere_frame(&mut self, r: &[f64]) {
        for (k, v) in self.squares.iter_mut().enumerate() {
            let at = ast::STRIDE + k * sb::STRIDE;
            v[square::AXIS_U..square::AXIS_U + 3]
                .copy_from_slice(&r[at + sb::AXIS_U..at + sb::AXIS_U + 3]);
            v[square::AXIS_V..square::AXIS_V + 3]
                .copy_from_slice(&r[at + sb::AXIS_V..at + sb::AXIS_V + 3]);
        }
    }
    // FountainWater's five basin slots and optional tier count are compiled
    // once, including the far-away sentinel for absent authored basins.
    pub fn fountain(&mut self, data: &Value) -> Value {
        let basins = data["basins"].as_array();
        let count = basins.map_or(0, Vec::len).min(5);
        // Five basin slots are part of the shader contract. Unused slots have
        // zero radius and distant centres, avoiding false pools at the origin.
        for i in 0..5 {
            let b = basins.and_then(|v| v.get(i));
            let at = fountain::BASINS + i * 4;
            for k in 0..3 {
                self.fountain[at + k] = b.and_then(|v| v["center"][k].as_f64()).unwrap_or(1e6);
            }
            self.fountain[at + 3] = b.and_then(|v| v["radius"].as_f64()).unwrap_or(0.);
        }
        self.fountain[fountain::HAS_TIER] = f64::from(data["tier"].as_bool().unwrap_or(false));
        self.fountain[fountain::HAS_FOAM] = f64::from(data["foam"].as_bool().unwrap_or(false));
        let sky = crate::content::recipe(
            crate::layout::environment_recipe::FOUNTAIN_SKY as u32,
            &data["sky"],
        );
        copy(&mut self.fountain, fountain::SKY, &sky);
        json!({"count":count,"textures":basins.into_iter().flatten().map(|v|v["texture"].clone()).collect::<Vec<_>>()})
    }
    // FountainWater.setEnabled preserves the authored appearance in Classic
    // and chooses the enhanced replacement order in this native owner.
    // Resource identity remains external, paired with these stable row ids.
    // Capture authored appearance before enabling replacement anchors.
    // Registration alone may allocate. Live changes rewrite existing rows,
    // paired with the host models by their stable admission positions.
    pub fn fountain_models(&mut self, data: &Value) {
        self.original_models = data
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| {
                [
                    f64::from(row["visible"].as_bool().unwrap_or(false)),
                    row["order"].as_f64().unwrap_or(0.),
                ]
            })
            .collect();
        self.fountain_models = self.original_models.clone();
    }
    // Equal settings do no work. Disposing a never-enabled fountain must
    // leave its original surface appearance and draw order untouched.
    pub fn fountain_mode(&mut self, enabled: bool) -> bool {
        if self.fountain_enabled == enabled {
            return false;
        }
        self.fountain_enabled = enabled;
        for (v, original) in self.fountain_models.iter_mut().zip(&self.original_models) {
            v[crate::layout::fountain_model::VISIBLE] = if enabled {
                0.
            } else {
                original[crate::layout::fountain_model::VISIBLE]
            };
            v[crate::layout::fountain_model::ORDER] = if enabled {
                -17.
            } else {
                original[crate::layout::fountain_model::ORDER]
            };
        }
        true
    }
    // Wakes have already advanced in EnvironmentPhase. Fountain shaders read
    // that same timestamp and the same point/velocity arrays as nearby steam.
    // The page therefore owns neither a second clock nor a wake afterstate.
    pub fn fountain_frame(&mut self, p: &crate::EnvironmentPhase) {
        let v = &mut self.fountain;
        v[fountain::WATER_TIME] = p.result[er::TIME];
        v[fountain::SKY..fountain::SKY + 3].copy_from_slice(&p.result[er::SKY..er::SKY + 3]);
        v[fountain::WATER_SUN_COLOR..fountain::WATER_SUN_COLOR + 3]
            .copy_from_slice(&self.input[frame::SUN_COLOR..frame::SUN_COLOR + 3]);
        v[fountain::WATER_SUN_DIRECTION..fountain::WATER_SUN_DIRECTION + 3]
            .copy_from_slice(&self.input[frame::SUN_DIRECTION..frame::SUN_DIRECTION + 3]);
        v[fountain::WAKES..fountain::WAKES + 32].copy_from_slice(&p.wakes.points);
        v[fountain::MOTION..fountain::MOTION + 32].copy_from_slice(&p.wakes.motion);
    }
    // A paused render consumes the preceding motion observation once per
    // renderer frame. SkyFlyers.motionFrame retains no host timing state.
    // All model parts in one renderer frame share a still flag. Consume the
    // stepped bit once, rather than treating later parts as paused draws.
    pub fn sky_motion(&mut self, frame: f64) -> f64 {
        if frame != self.motion_frame {
            self.motion_frame = frame;
            self.still = f64::from(!self.stepped);
            self.stepped = false;
        }
        self.still
    }
    pub fn sky_frame(&mut self, p: &crate::SkyPhase) {
        // Motion observes the same complete advancement as ship placement.
        // Zero-dt layout calls retain a preceding unconsumed flight step.
        self.stepped |= p.frame[crate::layout::sky_frame::DT] > 0.;
        for (k, v) in self.sky.iter_mut().enumerate() {
            let at = k * sky_result::STRIDE;
            let on = p.result[at + sky_result::ON] != 0.;
            v[sky_row::SOLID_VISIBLE] = f64::from(on && p.result[at + sky_result::SOLIDS] > 0.);
            v[sky_row::FADE_VISIBLE] = f64::from(on && p.result[at + sky_result::FADINGS] > 0.);
            v[sky_row::MOTION_VISIBLE] = f64::from(on);
            // Hidden browser draws used to retain their preceding counts/reach.
            // Preserve those observable resource facts in the native record.
            if on {
                v[sky_row::REACH] = p.frame[crate::layout::sky_frame::FAR];
                v[sky_row::SOLID_COUNT] = p.result[at + sky_result::SOLIDS];
                v[sky_row::FADING_COUNT] = p.result[at + sky_result::FADINGS];
            }
        }
    }
    // WaterShore.publishFrame owns both native instance counts and lapping
    // time. The renderer borrows these rows for the wash and splash draws.
    pub fn shore_frame(&mut self, p: &crate::surfaces::Phase) {
        for (k, v) in self.shore.iter_mut().enumerate() {
            v[shore::TIME] = p.result[crate::layout::surface_result::SHORE_TIME];
            v[shore::HEIGHT] = p.config[crate::layout::surface_config::HEIGHT];
            v[shore::SPLASH] = k as f64;
        }
    }
    // Only generated producer blocks cross this export. Invalid optional
    // square ids return a zero row; shore ids clamp to their two draws.
    // Neither case can grow a bounded bank or panic while a frame uploads.
    pub fn values(&mut self, kind: u32, index: usize) -> &mut [f64] {
        if kind == draw::RAIN as u32 {
            return &mut self.rain;
        }
        if kind == draw::MIST as u32 {
            return &mut self.mist;
        }
        if kind == draw::SHEETS as u32 {
            return &mut self.sheets;
        }
        if kind == draw::SNOW as u32 {
            return &mut self.snow;
        }
        if kind == draw::FOUNTAIN as u32 {
            return &mut self.fountain;
        }
        if kind == draw::SHORE as u32 {
            return &mut self.shore[index.min(1)];
        }
        if kind == draw::SQUARE as u32 {
            if let Some(v) = self.squares.get_mut(index) {
                return v;
            }
        }
        &mut self.empty
    }
}
