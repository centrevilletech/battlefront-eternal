//! The rain (src/weather.js, src/rain-impacts.js, src/rain-sheets.js): the
//! state the drops, their mist, the splashes where they land and the mid
//! distance sheets share. The drops' positions are worked out by the vertex
//! shader from their seeds and the rain's clock, so the CPU keeps the clock,
//! the camera's motion, the cover cache, the splashes' records and which of
//! the draws show.

use crate::cover::Cover;
use crate::layout::{rain, weather_frame as wf, weather_result as wr};
use jsrt::conv::{js_min, js_round};
use three::Vector3;

const COUNT: usize = rain::IMPACTS;
const LIFE_RATE: f64 = 90.0;

/// What the page's collision world answers for a splash's cast: the height
/// of what a cast straight down first hits (or the water's level over it
/// if higher, or -infinity for nothing), its normal, and whether it is water.
// What the page's collision world answers for a splash's cast: the height of
// what a cast straight down first hits (or the water's level over it if higher,
// or -infinity for nothing), its normal, and whether it is water.
pub struct Impact {
    pub y: f64,
    pub normal: [f64; 3],
    pub water: bool,
}

/// The splashes (RainImpacts): rings and droplets where drops land, in a
/// ring of COUNT records, a ring and a droplet each.
// The splashes: rings and droplets where drops land, in a ring of 96 records
// (a ring and a droplet each), the oldest overwritten.
pub struct Impacts {
    time: f64,
    acc: f64,
    pub next: usize,
    pub queries: usize,
    random_state: u32,
    pub locations: Vec<f32>,
    pub normals: Vec<f32>,
    pub shown: bool,
    pub changed: bool,
}

impl Impacts {
    fn new() -> Impacts {
        let mut locations = vec![0.0f32; COUNT * 2 * 4];
        let mut normals = vec![0.0f32; COUNT * 2 * 3];
        for i in 0..COUNT * 2 {
            locations[i * 4 + 3] = -100.0;
            normals[i * 3 + 1] = 1.0;
        }
        Impacts { time: 0.0, acc: 0.0, next: 0, queries: 0, random_state: 19, locations, normals, shown: false, changed: false }
    }

    fn random(&mut self) -> f64 {
        self.random_state = self.random_state.wrapping_mul(1664525).wrapping_add(1013904223);
        self.random_state as f64 / 4294967296.0
    }

    // Up to four casts a frame from random places around the eye, 128 m above;
    // only those that land on something level (or water) within range of the
    // eye's height make a splash.
    fn update(&mut self, dt: f64, eye: [f64; 3], enabled: bool, cast: &mut dyn FnMut(f64, f64, f64) -> Impact) {
        self.time += dt;
        self.shown = enabled;
        self.queries = 0;
        self.changed = false;
        if !enabled {
            self.acc = 0.0;
            return;
        }
        self.acc = js_min(4.0, self.acc + dt * LIFE_RATE);
        while self.acc >= 1.0 {
            self.acc -= 1.0;
            self.queries += 1;
            let radius = self.random().sqrt() * 11.0;
            let angle = self.random() * std::f64::consts::PI * 2.0;
            let (x, y, z) = (eye[0] + v8math::cos(angle) * radius, eye[1] + 128.0, eye[2] + v8math::sin(angle) * radius);
            let hit = cast(x, y, z);
            let h = hit.y;
            if !h.is_finite() || h > eye[1] + 1.0 || h < eye[1] - 12.0 || (!hit.water && hit.normal[1] < 0.6) {
                continue;
            }
            for kind in 0..2 {
                let index = self.next * 2 + kind;
                self.locations[index * 4..index * 4 + 4].copy_from_slice(&[x as f32, h as f32, z as f32, self.time as f32]);
                let n = if hit.water { [0.0, 1.0, 0.0] } else { hit.normal };
                self.normals[index * 3..index * 3 + 3].copy_from_slice(&[n[0] as f32, n[1] as f32, n[2] as f32]);
            }
            self.next = (self.next + 1) % COUNT;
            self.changed = true;
        }
    }
}

pub struct Weather {
    enhanced: bool,
    pub cover: Cover,
    pub impacts: Impacts,
    rain_time: f64,
    previous: [f64; 3],
    motion: [f64; 3],
    initial: bool,
}

impl Weather {
    pub fn new(enhanced: bool, range: f64, size: usize) -> Weather {
        Weather { enhanced, cover: Cover::with_capacity(range, size, 32), impacts: Impacts::new(), rain_time: 0.0, previous: [0.0; 3], motion: [0.0; 3], initial: true }
    }

    /// Classic or Enhanced rain, live (applyMode): a cover cache of the new
    /// range and size, filled whole in the next frame.
    // Classic or Enhanced rain, live: a cover cache of the new range and size,
    // filled whole in the next frame. Nothing is allocated, so no view moves.
    pub fn set_mode(&mut self, enhanced: bool, range: f64, size: usize) {
        self.enhanced = enhanced;
        self.cover.reset(range, size);
        self.initial = true;
    }

    /// The frame (Weather.update). `height` answers the cover cache's cells,
    /// `cast` the splashes' casts.
    // The frame: the rain's clock, the camera's motion (clamped at 30 m/s, and none
    // after a jump or on the first frame), the cover cache, the splashes, and which
    // draws show.
    pub fn update(&mut self, frame: &[f64], height: &mut dyn FnMut(f64, f64, f64) -> f64, cast: &mut dyn FnMut(f64, f64, f64) -> Impact, result: &mut [f64]) {
        let dt = frame[wf::DT];
        let at = [frame[wf::POSITION], frame[wf::POSITION + 1], frame[wf::POSITION + 2]];
        self.rain_time += dt;
        let (p, q) = (Vector3::new(at[0], at[1], at[2]), Vector3::new(self.previous[0], self.previous[1], self.previous[2]));
        if !self.initial && dt > 0.0 && q.distance_to_squared(&p) < 100.0 {
            let mut m = Vector3::default();
            m.sub_vectors(&p, &q).divide_scalar(dt).clamp_length(0.0, 30.0);
            self.motion = [m.x, m.y, m.z];
        } else {
            self.motion = [0.0; 3];
        }
        self.previous = at;
        let budget = if self.initial { self.cover.cells() } else { 96 };
        let (covered, changed) = self.cover.update(at, budget, height);
        let visible = covered;
        self.impacts.update(dt, at, visible && self.enhanced, cast);
        self.initial = false;
        result[wr::RAIN_TIME] = self.rain_time;
        result[wr::EYE..wr::EYE + 3].copy_from_slice(&at);
        result[wr::CAMERA_MOTION..wr::CAMERA_MOTION + 3].copy_from_slice(&self.motion);
        result[wr::COVERED] = f64::from(visible);
        result[wr::CHANGED] = f64::from(changed);
        result[wr::COVER_QUERIES] = self.cover.queries as f64;
        result[wr::MIST_VISIBLE] = f64::from(visible && self.enhanced);
        result[wr::SHEETS_VISIBLE] = f64::from(visible && self.enhanced);
        result[wr::IMPACTS_SHOWN] = f64::from(self.impacts.shown);
        result[wr::IMPACTS_CHANGED] = f64::from(self.impacts.changed);
        result[wr::IMPACTS_NEXT] = self.impacts.next as f64;
        result[wr::IMPACTS_QUERIES] = self.impacts.queries as f64;
        result[wr::IMPACT_TIME] = self.impacts.time;
    }
}

/// The seeds of `count` drops, four numbers each, from a fixed stream (the
/// instanced geometry's `seed` attribute).
// The seeds of the drops, four numbers each, from a fixed stream: the shader
// places each drop from its seed and the rain's clock, so this is made once.
pub fn seeds(out: &mut [f32]) {
    let mut state: u32 = 7351;
    for v in out.iter_mut() {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        *v = (state as f64 / 4294967296.0) as f32;
    }
}

/// A tileable sheet of falling streaks, thin, varied in length and strength,
/// pre-blurred across so they stay soft when minified (streakTexture); RGBA,
/// alpha the streaks.
// A tileable sheet of falling streaks, thin, varied in length and strength,
// pre-blurred across so they stay soft when minified.
pub fn streaks(data: &mut [u8]) {
    let size = rain::STREAK_SIZE;
    data.fill(0);
    let mut random = EngineRandomLcg(2741);
    for _ in 0..rain::STREAK_COUNT {
        let x = random.next() * size as f64;
        let y0 = random.next() * size as f64;
        let length = 6.0 + random.next() * random.next() * 44.0;
        let strength = 0.35 + random.next() * 0.65;
        let mut k = 0.0;
        while k < length {
            let y = ((y0 + k).floor() as i64).rem_euclid(size as i64) as usize;
            let fade = v8math::sin(std::f64::consts::PI * (k + 0.5) / length) * strength;
            for dx in -1i64..=1 {
                let px = (((x.floor() as i64 + dx) % size as i64) + size as i64) as usize % size;
                let w = if dx == 0 { 1.0 } else { 0.28 };
                let at = (y * size + px) * 4;
                // A streak's last texel can sum below zero; the page's Uint8Array wrapped
                // such a value modulo 256 (so a few texels stay bright), and so does this.
                data[at + 3] = (js_min(255.0, data[at + 3] as f64 + js_round(255.0 * fade * w)) as i64 & 0xff) as u8;
            }
            k += 1.0;
        }
    }
    for i in (0..data.len()).step_by(4) {
        data[i] = 255;
        data[i + 1] = 255;
        data[i + 2] = 255;
    }
}

struct EngineRandomLcg(u32);
impl EngineRandomLcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0 as f64 / 4294967296.0
    }
}
