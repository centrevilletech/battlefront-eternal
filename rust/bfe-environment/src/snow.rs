//! Snow (src/snow.js): a world's Precipitation of Type("Quads") as the 2004
//! engine draws it. `count` flakes fill a 5 m box and the box repeats over
//! the world's 5 m grid out to `range` in view; the flakes keep their place
//! in the world as the camera moves through them. The flakes move here, and
//! each frame the boxes in view are chosen, nearest last, for one instanced
//! draw to repeat them over.

use crate::common::{sort_descending, Camera, EngineRandom, Frustum};
use crate::cover::Cover;
use crate::layout::{snow_config as sc, snow_result as sr};
use jsrt::conv::{js_hypot3, js_max, js_min};
use three::{deg_to_rad, Vector3};

pub const BOX: f64 = crate::layout::snow::BOX as f64;
/// Boxes found in view at once. The view reaches a few thousand cells at
/// most; a find past this is dropped.
// Boxes found in view at once. The view reaches a few thousand cells at most;
// a find past this is dropped, never written out of bounds.
const FOUND: usize = 32768;

/// The wind (Wind): its velocity eases from one target to the next, a new
/// target every 1 / rate seconds, each the authored velocity give or take
/// the range.
// The wind (the world's Effect("Wind")): its velocity eases from one target to
// the next, a new target every 1/rate seconds, each the authored velocity give
// or take its range. Its random stream is the engine's, seeded 5.
struct Wind {
    base: [f64; 2],
    range: f64,
    rate: f64,
    random: EngineRandom,
    from: [f64; 2],
    to: [f64; 2],
    t: f64,
}

impl Wind {
    fn new(base: [f64; 2], range: f64, rate: f64) -> Wind {
        let mut wind = Wind { base, range, rate, random: EngineRandom::new(5.0), from: base, to: base, t: 0.0 };
        wind.to = wind.target();
        wind
    }

    // The next gust's velocity: the authored one, give or take the range.
    fn target(&mut self) -> [f64; 2] {
        if !(self.range > 0.0) {
            return self.base;
        }
        [self.base[0] + (self.random.float() * 2.0 - 1.0) * self.range, self.base[1] + (self.random.float() * 2.0 - 1.0) * self.range]
    }

    // Eases from one target to the next; the time within the gust is its share.
    fn update(&mut self, dt: f64) -> [f64; 2] {
        if self.rate > 0.0 {
            self.t += self.rate * dt;
            while self.t >= 1.0 {
                self.t -= 1.0;
                self.from = self.to;
                self.to = self.target();
            }
        }
        let t = if self.rate > 0.0 { self.t } else { 0.0 };
        [self.from[0] + (self.to[0] - self.from[0]) * t, self.from[1] + (self.to[1] - self.from[1]) * t]
    }
}

/// The flakes of one box (SnowFlakes): positions in a unit box, each with
/// its wander, alpha, spin (turns a second) and turn (0..1), as 32-bit
/// floats.
// The flakes of one box: positions in a unit box, each with its wander, alpha,
// spin (turns a second) and turn (0..1), as 32-bit floats as the page kept them
// (the stores round alike).
struct Flakes {
    count: usize,
    fall: f64,
    wander: f64,
    alpha: [f64; 2],
    spin: [f64; 2],
    random: EngineRandom,
    position: Vec<f32>,
    velocity: Vec<f32>,
    alphas: Vec<f32>,
    spins: Vec<f32>,
    turns: Vec<f32>,
    ease: f64,
}

impl Flakes {
    // Each flake starts at a random place with its own alpha, spin, turn and a
    // wander velocity in a random direction, VelocityRange fast.
    fn new(c: &[f64]) -> Flakes {
        let count = c[sc::COUNT] as usize;
        let mut flakes = Flakes {
            count,
            fall: c[sc::FALL],
            wander: c[sc::WANDER],
            alpha: [c[sc::ALPHA_MIN], c[sc::ALPHA_MAX]],
            spin: [c[sc::SPIN_BASE], c[sc::SPIN_RANGE]],
            random: EngineRandom::new(3.0),
            position: vec![0.0; count * 3],
            velocity: vec![0.0; count * 3],
            alphas: vec![0.0; count],
            spins: vec![0.0; count],
            turns: vec![0.0; count],
            ease: 0.0,
        };
        let (alpha_min, alpha_max) = (flakes.alpha[0], flakes.alpha[1]);
        for i in 0..count {
            for k in 0..3 {
                flakes.position[i * 3 + k] = flakes.random.float() as f32;
            }
            flakes.alphas[i] = (alpha_min + flakes.random.float() * (alpha_max - alpha_min)) as f32;
            flakes.spins[i] = (flakes.spin[0] + (flakes.random.float() - 0.5) * flakes.spin[1]) as f32;
            flakes.turns[i] = flakes.random.float() as f32;
            // A random direction, VelocityRange fast.
            let z = flakes.random.float() * 2.0 - 1.0;
            let a = flakes.random.float() * std::f64::consts::PI * 2.0;
            let r = (1.0 - z * z).sqrt();
            flakes.velocity[i * 3] = (v8math::cos(a) * r * flakes.wander) as f32;
            flakes.velocity[i * 3 + 1] = (z * flakes.wander) as f32;
            flakes.velocity[i * 3 + 2] = (v8math::sin(a) * r * flakes.wander) as f32;
        }
        flakes
    }

    /// The engine's half-second easing: wander toward (±1, ±1, ±1) x
    /// VelocityRange, alpha toward an end of its range, spin toward
    /// Rotation ± half RotationRange (retarget).
    // The engine's half-second easing: wander toward (±1, ±1, ±1) x VelocityRange,
    // alpha toward an end of its range, spin toward Rotation ± half
    // RotationRange; the draws are made flake by flake.
    fn retarget(&mut self, t: f64) {
        for i in 0..self.count {
            if self.wander > 0.0 {
                for k in 0..3 {
                    let v = self.velocity[i * 3 + k] as f64;
                    let pick = (self.random.int() & 1) as f64;
                    self.velocity[i * 3 + k] = (v + ((pick * 2.0 - 1.0) * self.wander - v) * t) as f32;
                }
            }
            if self.alpha[1] > self.alpha[0] {
                let pick = (self.random.int() & 1) as f64;
                let a = self.alphas[i] as f64;
                self.alphas[i] = (a + (self.alpha[0] + pick * (self.alpha[1] - self.alpha[0]) - a) * t) as f32;
            }
            if self.spin[1] > 0.0 {
                let pick = (self.random.int() & 1) as f64;
                let s = self.spins[i] as f64;
                self.spins[i] = (s + (self.spin[0] + (pick - 0.5) * self.spin[1] - s) * t) as f32;
            }
        }
    }

    // Every half second (an accumulator that fills at 0.4 a second) the flakes
    // ease a fifth of the way toward new random targets; between, they fall with
    // the wind and their wander, and wrap in their box.
    fn update(&mut self, dt: f64, wind: [f64; 2]) {
        self.ease += 0.4 * dt;
        if self.ease > 0.2 {
            self.ease = js_min(self.ease, 1.0);
            self.retarget(self.ease);
            self.ease -= 0.2;
        }
        let step = dt / BOX;
        let push = [wind[0] * step, -self.fall * step, wind[1] * step];
        for i in 0..self.count * 3 {
            let value = self.position[i] as f64 + push[i % 3] + self.velocity[i] as f64 * step;
            self.position[i] = (value - value.floor()) as f32;
        }
        for i in 0..self.count {
            let turn = self.turns[i] as f64 + self.spins[i] as f64 * dt;
            self.turns[i] = (turn - turn.floor()) as f32;
        }
    }
}

// The snow: the flakes, the wind, the cover cache, and the data the shaders
// read (a two-row texture of flakes and a one-row texture of boxes in view).
pub struct Snow {
    flakes: Flakes,
    wind: Wind,
    range: f64,
    box_limit: usize,
    pub cover: Cover,
    initial: bool,
    /// The flakes as the shader reads them: two rows of `count` texels, the
    /// first the position and alpha, the second the turn.
    pub flake_data: Vec<f32>,
    /// The boxes in view: x, y, z, fade.
    pub box_data: Vec<f32>,
    found: Vec<[f64; 5]>,
    keys: Vec<f64>,
    order: Vec<u32>,
    scratch: Vec<u32>,
}

impl Snow {
    pub fn new(c: &[f64]) -> Snow {
        let flakes = Flakes::new(c);
        let count = flakes.count;
        let range = c[sc::RANGE];
        let box_limit = c[sc::BOX_LIMIT] as usize;
        Snow {
            flakes,
            wind: Wind::new([c[sc::WIND], c[sc::WIND + 1]], c[sc::GUST_RANGE], c[sc::GUST_RATE]),
            range,
            box_limit,
            cover: Cover::new(range, 16),
            initial: true,
            flake_data: vec![0.0; count * 2 * 4],
            box_data: vec![0.0; box_limit * 4],
            found: vec![[0.0; 5]; FOUND],
            keys: vec![0.0; FOUND],
            order: vec![0; FOUND],
            scratch: vec![0; FOUND],
        }
    }

    /// The boxes in view: each 5 m cell of the world grid whose centre lies
    /// within `far` along the view and whose box the view frustum touches,
    /// farthest first, with its fade (1, falling to 0 over the last third
    /// of the range). Over the limit, the nearest stay (snowBoxes).
    // The boxes in view: each 5 m cell of the world grid whose centre lies within
    // the range along the view and whose box the frustum touches, farthest first
    // with its fade (1, falling to 0 over the last third of the range). Over the
    // limit, the nearest boxes stay.
    fn boxes(&mut self, camera: &Camera, frustum: &Frustum) -> usize {
        let far = self.range;
        let e = &camera.world.elements;
        let (ex, ey, ez) = (e[12], e[13], e[14]);
        let length = js_hypot3(e[8], e[9], e[10]);
        let length = if length == 0.0 || length.is_nan() { 1.0 } else { length };
        let (fx, fy, fz) = (-e[8] / length, -e[9] / length, -e[10] / length);
        // The frustum's corners at `far` lie this far from the eye at most.
        // The frustum's corners at the range lie this far from the eye at most.
        let tan_y = v8math::tan(deg_to_rad(camera.fov_or(90.0)) / 2.0);
        let tan_x = tan_y * camera.aspect_or_1();
        let reach = far * (1.0 + tan_x * tan_x + tan_y * tan_y).sqrt() + BOX;
        let cells = |center: f64| ((center - reach) / BOX).floor() as i64..=((center + reach) / BOX).floor() as i64;
        let fade_from = far * 0.66;
        let half = BOX / 2.0;
        let mut found = 0;
        for x in cells(ex) {
            for y in cells(ey) {
                for z in cells(ez) {
                    let (cx, cy, cz) = ((x as f64 + 0.5) * BOX, (y as f64 + 0.5) * BOX, (z as f64 + 0.5) * BOX);
                    let depth = (cx - ex) * fx + (cy - ey) * fy + (cz - ez) * fz;
                    if depth >= far {
                        continue;
                    }
                    let min = Vector3::new(cx - half, cy - half, cz - half);
                    let max = Vector3::new(cx + half, cy + half, cz + half);
                    if !frustum.intersects_box(&min, &max) || found == FOUND {
                        continue;
                    }
                    self.found[found] = [depth, cx, cy, cz, if depth > fade_from { (far - depth) / (far - fade_from) } else { 1.0 }];
                    self.keys[found] = depth;
                    self.order[found] = found as u32;
                    found += 1;
                }
            }
        }
        sort_descending(&self.keys[..found], &mut self.order[..found], &mut self.scratch[..found]);
        let count = usize::min(found, self.box_limit);
        for (slot, i) in (found - count..found).enumerate() {
            let b = &self.found[self.order[i] as usize];
            self.box_data[slot * 4..slot * 4 + 4].copy_from_slice(&[b[1] as f32, b[2] as f32, b[3] as f32, b[4] as f32]);
        }
        count
    }

    /// The frame (Snow.update). `cast` answers the cover cache's cell
    /// queries; `position` is the camera's position the cache follows.
    // The frame: the flakes and the wind step, the cover cache refreshes (all of
    // it the first time, a budget of cells after), the flakes' texels are written,
    // and the boxes in view are chosen.
    pub fn update(&mut self, camera: &Camera, position: [f64; 3], cast: &mut dyn FnMut(f64, f64, f64) -> f64, result: &mut [f64]) {
        let dt = js_min(js_max(camera.dt, 0.0), 0.25);
        let wind = self.wind.update(dt);
        self.flakes.update(dt, wind);
        let e = &camera.world.elements;
        let mut right = Vector3::new(e[0], e[1], e[2]);
        right.normalize();
        let mut up = Vector3::new(e[4], e[5], e[6]);
        up.normalize();
        let budget = if self.initial { self.cover.cells() } else { 96 };
        let (covered, changed) = self.cover.update(position, budget, cast);
        self.initial = false;
        let n = self.flakes.count;
        for i in 0..n {
            let f = &self.flakes;
            self.flake_data[i * 4..i * 4 + 3].copy_from_slice(&f.position[i * 3..i * 3 + 3]);
            self.flake_data[i * 4 + 3] = f.alphas[i];
            self.flake_data[(n + i) * 4] = f.turns[i];
        }
        let mut frustum = Frustum::default();
        camera.frustum(&mut frustum);
        let boxes = self.boxes(camera, &frustum);
        result[sr::BOXES] = boxes as f64;
        result[sr::INSTANCES] = (boxes * n) as f64;
        result[sr::COVERED] = f64::from(covered);
        result[sr::VISIBLE] = f64::from(covered && boxes > 0);
        result[sr::CHANGED] = f64::from(changed);
        result[sr::QUERIES] = self.cover.queries as f64;
        result[sr::CAMERA_RIGHT..sr::CAMERA_RIGHT + 3].copy_from_slice(&[right.x, right.y, right.z]);
        result[sr::CAMERA_UP..sr::CAMERA_UP + 3].copy_from_slice(&[up.x, up.y, up.z]);
    }
}
