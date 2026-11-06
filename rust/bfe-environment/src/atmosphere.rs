//! The map's haze (src/atmosphere.js): the FogCloud band of big soft squares
//! around the camera and the dusteffect clouds placed through the map, as
//! the 2004 engine simulates them. Each texture is one batch of at most
//! CAPACITY squares gathered each frame, sorted back to front, for the
//! renderer to draw as one instanced draw.
//!
//! Terrain heights under the fog come from the page's collision world (the
//! only thing here the phase asks of the host): each grid corner is asked
//! once and blended between.

use crate::common::{sort_descending, Camera, EngineRandom, Frustum};
use crate::layout::{atmosphere_config as ac, atmosphere_stats as st, dust_config as dc, square_batch as sb, squares};
use jsrt::conv::{js_max, js_min};
use three::{deg_to_rad, Vector3};

// The most squares a texture's batch draws.
const CAPACITY: usize = squares::CAPACITY;
const SQRT_HALF: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// JavaScript's `%` and the wrapping it is used for (wrap).
fn wrap(value: f64, period: f64) -> f64 {
    let m = value % period;
    if m < 0.0 { m + period } else { m }
}

/// The FogCloud's definition (map.json atmosphere.fogCloud).
// The FogCloud's definition: a band of big soft squares around the camera,
// sized, spaced and scrolled by the map's numbers.
pub struct Fog {
    pub near: f64,
    pub far: f64,
    pub cell: f64,
    pub size: f64,
    pub density: f64,
    pub velocity: [f64; 2],
    pub rotation: f64,
    pub alpha: f64,
    pub height: f64,
}

/// Terrain heights on a grid of corners, each asked of the collision world
/// once (GroundHeights): the FogCloud stands hundreds of squares a frame on
/// the ground, and its points move a few centimetres a frame. An open table
/// of the same size and the same clear-when-full rule as the Map it ports.
// Terrain heights on a grid of corners: the fog stands hundreds of squares
// a frame on the ground and its points move a few centimetres a frame, so
// each corner is asked once and blended between.
pub struct GroundHeights {
    cell: f64,
    limit: usize,
    keys: Vec<i64>,
    heights: Vec<f64>,
    count: usize,
}

const EMPTY: i64 = i64::MIN;

impl GroundHeights {
    pub fn new(cell: f64, limit: usize) -> GroundHeights {
        // A table twice the limit, open addressed, so it is never more than half
        // full and a lookup is a step or two.
        let slots = (limit * 2).next_power_of_two();
        GroundHeights { cell, limit, keys: vec![EMPTY; slots], heights: vec![0.0; slots], count: 0 }
    }

    // A corner's height, asked of the page the first time. The table is full at
    // the limit and then cleared whole (as the Map it replaces was), which keeps
    // the lookup bounded.
    fn corner(&mut self, ix: i64, iz: i64, ground: &mut dyn FnMut(f64, f64) -> f64) -> f64 {
        // A corner's key: its two numbers in one, as the page's Map had them.
        let key = ix * 1048576 + iz;
        let mask = self.keys.len() - 1;
        let mut at = ((key as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as usize & mask;
        while self.keys[at] != EMPTY {
            if self.keys[at] == key {
                return self.heights[at];
            }
            at = (at + 1) & mask;
        }
        if self.count >= self.limit {
            self.keys.fill(EMPTY);
            self.count = 0;
            at = ((key as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as usize & mask;
        }
        // NaN: no ground there.
        // The page's collision world answers (NaN for no ground there).
        let height = ground(ix as f64 * self.cell, iz as f64 * self.cell);
        self.keys[at] = key;
        self.heights[at] = height;
        self.count += 1;
        height
    }

    /// The terrain's height at x, z, blended between its four corners; at an
    /// edge or a cut, between those that have ground; none when there is
    /// none (NaN).
    // The height at a point: blended between its four corners, or between those
    // that have ground at an edge or a cut; none (NaN) where there is none.
    pub fn at(&mut self, x: f64, z: f64, ground: &mut dyn FnMut(f64, f64) -> f64) -> f64 {
        let fx = x / self.cell;
        let fz = z / self.cell;
        let (ix, iz) = (fx.floor(), fz.floor());
        let (u, v) = (fx - ix, fz - iz);
        let (ix, iz) = (ix as i64, iz as i64);
        let a = self.corner(ix, iz, ground);
        let b = self.corner(ix + 1, iz, ground);
        let c = self.corner(ix, iz + 1, ground);
        let d = self.corner(ix + 1, iz + 1, ground);
        if !a.is_nan() && !b.is_nan() && !c.is_nan() && !d.is_nan() {
            return (a * (1.0 - u) + b * u) * (1.0 - v) + (c * (1.0 - u) + d * u) * v;
        }
        let (mut sum, mut weight) = (0.0, 0.0);
        for (h, w) in [(a, (1.0 - u) * (1.0 - v)), (b, u * (1.0 - v)), (c, (1.0 - u) * v), (d, u * v)] {
            if !h.is_nan() {
                sum += h * w;
                weight += w;
            }
        }
        if weight > 0.0 { sum / weight } else { f64::NAN }
    }
}

/// One dusteffect class's particles, shared by every object of the class
/// (DustCloud). Each is [x, y, z, vx, vy, vz, size, life left, age, alpha]
/// in the object's frame, kept as 32-bit floats as the page kept them.
// One dusteffect class's particles, shared by every object of the class. Their
// state is 32-bit floats, as the page kept them, so the stores round alike.
pub struct Dust {
    def: Vec<f64>,
    random: EngineRandom,
    particles: Vec<[f32; 10]>,
    instances: usize,
    first_instance: usize,
}

impl Dust {
    fn new(def: &[f64], seed: f64, first_instance: usize) -> Dust {
        // A class's particles all start full: lives part spent, every particle at its
        // class's alpha.
        let count = def[dc::COUNT] as usize;
        let mut dust = Dust {
            def: def.to_vec(),
            random: EngineRandom::new(seed),
            particles: vec![[0.0; 10]; count],
            instances: def[dc::INSTANCES] as usize,
            first_instance,
        };
        for i in 0..count {
            let mut p = [0.0f32; 10];
            dust.spawn(&mut p);
            // A cloud starts full: lives part spent, every particle at its alpha.
            p[7] = (p[7] as f64 * dust.random.float()) as f32;
            p[9] = dust.def[dc::ALPHA] as f32;
            dust.particles[i] = p;
        }
        dust
    }

    fn between(&mut self, a: f64, b: f64) -> f64 {
        a + self.random.float() * (b - a)
    }

    // A particle starts again in the class's box with its own velocity, size
    // and life, drawn in a fixed order.
    fn spawn(&mut self, p: &mut [f32; 10]) {
        let d = &self.def;
        let (min_pos, max_pos, min_vel, max_vel) = ([d[dc::MIN_POS], d[dc::MIN_POS + 1], d[dc::MIN_POS + 2]], [d[dc::MAX_POS], d[dc::MAX_POS + 1], d[dc::MAX_POS + 2]], [d[dc::MIN_VEL], d[dc::MIN_VEL + 1], d[dc::MIN_VEL + 2]], [d[dc::MAX_VEL], d[dc::MAX_VEL + 1], d[dc::MAX_VEL + 2]]);
        let (min_size, max_size, min_life, max_life) = (d[dc::MIN_SIZE], d[dc::MAX_SIZE], d[dc::MIN_LIFETIME], d[dc::MAX_LIFETIME]);
        p[2] = self.between(min_pos[2], max_pos[2]) as f32;
        p[1] = self.between(min_pos[1], max_pos[1]) as f32;
        p[0] = self.between(min_pos[0], max_pos[0]) as f32;
        p[9] = 0.0;
        p[8] = 0.0;
        p[5] = self.between(min_vel[2], max_vel[2]) as f32;
        p[4] = self.between(min_vel[1], max_vel[1]) as f32;
        p[3] = self.between(min_vel[0], max_vel[0]) as f32;
        p[6] = self.between(min_size, max_size) as f32;
        p[7] = self.between(min_life, max_life) as f32;
    }

    /// Ages and moves each particle; a spent one, or one past the radius
    /// fade, starts again (DustCloud.update).
    // Ages and moves each particle; a spent one, or one past the radius fade,
    // starts again. Alpha rises at one a second to the class's, and in its last
    // second is the class's x life left x that.
    fn update(&mut self, dt: f64) {
        let (alpha, fade_min, fade_max) = (self.def[dc::ALPHA], self.def[dc::FADE_MIN], self.def[dc::FADE_MAX]);
        for i in 0..self.particles.len() {
            let mut p = self.particles[i];
            p[8] = (p[8] as f64 + dt) as f32;
            p[7] = (p[7] as f64 - dt) as f32;
            if p[7] < 0.0 {
                self.spawn(&mut p);
                self.particles[i] = p;
                continue;
            }
            p[0] = (p[0] as f64 + p[3] as f64 * dt) as f32;
            p[1] = (p[1] as f64 + p[4] as f64 * dt) as f32;
            p[2] = (p[2] as f64 + p[5] as f64 * dt) as f32;
            // In at 1 a second to the class's alpha; its last second, Alpha x
            // life left x that (the engine's own rule, which steps down there).
            let rise = js_min(p[8] as f64, alpha);
            p[9] = (if (p[7] as f64) < 1.0 { alpha * p[7] as f64 * rise } else { rise }) as f32;
            if fade_min > 0.0 {
                let r2 = p[0] as f64 * p[0] as f64 + p[2] as f64 * p[2] as f64;
                if r2 > fade_min * fade_min {
                    if r2 > fade_max * fade_max {
                        self.spawn(&mut p);
                        self.particles[i] = p;
                        continue;
                    }
                    p[9] = (p[9] as f64 * (1.0 - (r2.sqrt() - fade_min) / (fade_max - fade_min))) as f32;
                }
            }
            self.particles[i] = p;
        }
    }

    /// How strongly an object of the class draws, its position's view depth
    /// given: the class's radius on top, out past MaxDistance, fading in
    /// over the 200 m before it (and over 100 m beyond a MinDistance). 0
    /// when not drawn (dustFade).
    // How strongly an object of the class draws, from its depth: the class's radius
    // on top, nothing past its far distance, fading in over 200 m before it.
    fn fade(&self, depth: f64) -> f64 {
        let (min, max, radius) = (self.def[dc::MIN_DISTANCE], self.def[dc::MAX_DISTANCE], self.def[dc::RADIUS]);
        let reach = depth + radius;
        if reach < min || reach > max {
            return 0.0;
        }
        if min > 0.0 && reach < min + 100.0 {
            return js_max(0.0, (reach - min) * 0.01);
        }
        js_min((max - reach) * 0.005, 1.0)
    }
}

/// One texture's squares (SquareBatch): gathered with their depth, then
/// written far to near into the records the renderer reads.
// One texture's squares: gathered with their depth, then written far to near so
// the soft squares blend over one another in order. The sort allocates
// nothing.
pub struct Batch {
    items: Vec<[f64; 7]>,
    used: usize,
    order: Vec<u32>,
    scratch: Vec<u32>,
    keys: Vec<f64>,
    pub center: Vec<f32>,
    pub spin: Vec<f32>,
    pub count: usize,
}

impl Batch {
    fn new() -> Batch {
        Batch {
            items: vec![[0.0; 7]; CAPACITY],
            used: 0,
            order: vec![0; CAPACITY],
            scratch: vec![0; CAPACITY],
            keys: vec![0.0; CAPACITY],
            center: vec![0.0; CAPACITY * 4],
            spin: vec![0.0; CAPACITY * 2],
            count: 0,
        }
    }

    fn push(&mut self, depth: f64, x: f64, y: f64, z: f64, size: f64, angle: f64, alpha: f64) {
        if self.used == CAPACITY || !(alpha > 0.001) {
            return;
        }
        self.items[self.used] = [depth, x, y, z, size, angle, alpha];
        self.used += 1;
    }

    /// Far to near, so the soft squares blend over one another in order.
    // Far to near; equal depths keep the order they were gathered in.
    fn end(&mut self) {
        let n = self.used;
        for i in 0..n {
            self.order[i] = i as u32;
            self.keys[i] = self.items[i][0];
        }
        sort_descending(&self.keys[..n], &mut self.order[..n], &mut self.scratch[..n]);
        for i in 0..n {
            let item = &self.items[self.order[i] as usize];
            self.center[i * 4] = item[1] as f32;
            self.center[i * 4 + 1] = item[2] as f32;
            self.center[i * 4 + 2] = item[3] as f32;
            self.center[i * 4 + 3] = item[4] as f32;
            self.spin[i * 2] = item[5] as f32;
            self.spin[i * 2 + 1] = item[6] as f32;
        }
        self.count = n;
    }
}

pub struct Atmosphere {
    time: f64,
    fog: Option<Fog>,
    heights: GroundHeights,
    dust: Vec<Dust>,
    instances: Vec<f64>,
    pub batches: Vec<Batch>,
}

impl Atmosphere {
    /// `config` is the AtmosphereConfig record then a DustConfig record per
    /// class; `instances` the objects' positions, three numbers each, class
    /// after class.
    pub fn new(config: &[f64], instances: &[f64]) -> Atmosphere {
        let fog = (config[ac::HAS_FOG] != 0.0).then(|| Fog {
            near: config[ac::NEAR],
            far: config[ac::FAR],
            cell: config[ac::CELL],
            size: config[ac::SIZE],
            density: config[ac::DENSITY],
            velocity: [config[ac::VELOCITY], config[ac::VELOCITY + 1]],
            rotation: config[ac::ROTATION],
            alpha: config[ac::ALPHA],
            height: config[ac::HEIGHT],
        });
        let classes = config[ac::CLASSES] as usize;
        let mut dust = Vec::with_capacity(classes);
        let mut first = 0;
        for k in 0..classes {
            let at = ac::STRIDE + k * dc::STRIDE;
            let def = &config[at..at + dc::STRIDE];
            dust.push(Dust::new(def, def[dc::SEED], first));
            first += def[dc::INSTANCES] as usize;
        }
        let batches = (0..usize::from(fog.is_some()) + classes).map(|_| Batch::new()).collect();
        Atmosphere { time: 0.0, fog, heights: GroundHeights::new(8.0, 65536), dust, instances: instances.to_vec(), batches }
    }

    /// The FogCloud squares in view at `time` seconds (fogCloudParticles):
    /// every 256 m cell holds the same `density` points, drawn once from a
    /// fixed seed, scrolled by the wind and stood `height` over the ground.
    /// A point draws inside the box around the view's slab from `range`
    /// near to far, its alpha 1 - t^2 of the colour's, t running -1..1 as
    /// the squared distance runs between the range's squares.
    #[allow(clippy::too_many_arguments)]
    // The FogCloud squares in view: every cell holds the same points, drawn once
    // from a fixed seed, scrolled by the wind, and stood a height over the ground.
    // A point draws inside the box around the view's slab from the near range to
    // the far, its alpha falling off with the squared distance's place between.
    fn fog_squares(fog: &Fog, time: f64, eye: &Vector3, right: &Vector3, up: &Vector3, forward: &Vector3, tan_x: f64, tan_y: f64, heights: &mut GroundHeights, ground: &mut dyn FnMut(f64, f64) -> f64, batch: &mut Batch) -> usize {
        let (near, far, cell, size) = (fog.near, fog.far, fog.cell, fog.size);
        if !(far > near) || !(fog.density > 0.0) {
            return 0;
        }
        // A view direction's corners are normalized (the box around the view's slab
        // is the bounds of the slab's eight corners, padded by the square's reach).
        let norm = 1.0 / (1.0 + tan_x * tan_x + tan_y * tan_y).sqrt();
        let (mut min_x, mut min_y, mut min_z) = (f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let (mut max_x, mut max_y, mut max_z) = (f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut take = |dx: f64, dy: f64, dz: f64, distance: f64| {
            let (x, y, z) = (eye.x + dx * distance, eye.y + dy * distance, eye.z + dz * distance);
            if x < min_x { min_x = x; }
            if x > max_x { max_x = x; }
            if y < min_y { min_y = y; }
            if y > max_y { max_y = y; }
            if z < min_z { min_z = z; }
            if z > max_z { max_z = z; }
        };
        take(forward.x, forward.y, forward.z, near);
        take(forward.x, forward.y, forward.z, far);
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                let dx = (forward.x + right.x * sx * tan_x + up.x * sy * tan_y) * norm;
                let dy = (forward.y + right.y * sx * tan_x + up.y * sy * tan_y) * norm;
                let dz = (forward.z + right.z * sx * tan_x + up.z * sy * tan_y) * norm;
                take(dx, dy, dz, near);
                take(dx, dy, dz, far);
            }
        }
        let pad = size * SQRT_HALF;
        min_x -= pad; min_y -= pad; min_z -= pad;
        max_x += pad; max_y += pad; max_z += pad;
        let (x0, x1) = ((min_x / cell).floor() as i64, (max_x / cell).floor() as i64);
        let (z0, z1) = ((min_z / cell).floor() as i64, (max_z / cell).floor() as i64);
        // The wind scrolls the pattern; the cells repeat, so only the wrapped offset
        // matters.
        let offset_x = wrap(fog.velocity[0] * time, cell);
        let offset_z = wrap(fog.velocity[1] * time, cell);
        let spin = wrap(fog.rotation * time + 1.0, 2.0) - 1.0;
        let mid = (near * near + far * far) * 0.5;
        let scale = 2.0 / (far * far - near * near);
        // The same seed every frame: the pattern does not shimmer.
        let mut random = EngineRandom::new(1.0);
        let mut count = 0;
        let mut i = 0.0;
        while i < fog.density {
            i += 1.0;
            let mut x = random.float() * cell + offset_x;
            let mut z = random.float() * cell + offset_z;
            let phase = random.float() * 2.0;
            let turn = if random.int() & 0x10 != 0 { -1.0 } else { 1.0 };
            if x > cell { x -= cell; }
            if z > cell { z -= cell; }
            let mut angle = turn * spin + phase;
            if angle > 1.0 { angle -= 2.0; }
            for cx in x0..=x1 {
                let px = cx as f64 * cell + x;
                if px < min_x || px >= max_x { continue; }
                for cz in z0..=z1 {
                    let pz = cz as f64 * cell + z;
                    if pz < min_z || pz >= max_z { continue; }
                    let height = heights.at(px, pz, ground);
                    if height.is_nan() { continue; }
                    let py = height + fog.height;
                    if py < min_y || py >= max_y { continue; }
                    let (dx, dy, dz) = (px - eye.x, py - eye.y, pz - eye.z);
                    // Where the point's squared distance lies between the range's squares, -1
                    // to 1; its alpha is 1 - t squared of the colour's.
                    let t = (dx * dx + dy * dy + dz * dz - mid) * scale;
                    if t * t >= 1.0 { continue; }
                    let depth = (px - eye.x) * forward.x + (py - eye.y) * forward.y + (pz - eye.z) * forward.z;
                    batch.push(depth, px, py, pz, size, angle * std::f64::consts::PI, (1.0 - t * t) * fog.alpha);
                    count += 1;
                }
            }
        }
        count
    }

    /// The frame (Atmosphere.update): the clouds step, and the squares in
    /// view are gathered into each batch. `result` takes each batch's count
    /// and camera axes (SquareBatch records) after the AtmosphereStats.
    // The frame: the clouds step, then the squares in view are gathered into each
    // batch. Dust objects are culled as the engine culls any object, by a sphere
    // of the class's radius against the view.
    pub fn update(&mut self, camera: &Camera, ground: &mut dyn FnMut(f64, f64) -> f64, result: &mut [f64]) {
        // A frame is counted for at most a quarter second.
        let dt = js_min(js_max(camera.dt, 0.0), 0.25);
        self.time += dt;
        let e = &camera.world.elements;
        let eye = Vector3::new(e[12], e[13], e[14]);
        let mut right = Vector3::new(e[0], e[1], e[2]);
        right.normalize();
        let mut up = Vector3::new(e[4], e[5], e[6]);
        up.normalize();
        let mut forward = Vector3::new(-e[8], -e[9], -e[10]);
        forward.normalize();
        let tan_y = v8math::tan(deg_to_rad(camera.fov_or(60.0)) * 0.5);
        let tan_x = tan_y * camera.aspect_or_1();
        // Copies are culled as the engine culls any object: by a sphere of
        // the class's radius around them, against the view.
        // Dust objects are culled as the engine culls any object.
        let mut frustum = Frustum::default();
        camera.frustum(&mut frustum);
        for batch in self.batches.iter_mut() {
            batch.used = 0;
        }
        let (mut fog_count, mut dust_count, mut objects) = (0usize, 0usize, 0usize);
        let mut b = 0;
        let stats = st::STRIDE;
        if let Some(fog) = &self.fog {
            let r = &mut result[stats + b * sb::STRIDE..];
            r[sb::AXIS_U..sb::AXIS_U + 3].copy_from_slice(&[right.x, right.y, right.z]);
            r[sb::AXIS_V..sb::AXIS_V + 3].copy_from_slice(&[up.x, up.y, up.z]);
            fog_count = Self::fog_squares(fog, self.time, &eye, &right, &up, &forward, tan_x, tan_y, &mut self.heights, ground, &mut self.batches[b]);
            b += 1;
        }
        for dust in self.dust.iter_mut() {
            dust.update(dt);
            let scale = dust.def[dc::HEIGHT_SCALE];
            // The camera's axes in the objects' frame: the world's, the height
            // scaled. u runs down the square, v across, as the engine maps them.
            let r = &mut result[stats + b * sb::STRIDE..];
            r[sb::AXIS_U..sb::AXIS_U + 3].copy_from_slice(&[-up.x, -up.y * scale, -up.z]);
            r[sb::AXIS_V..sb::AXIS_V + 3].copy_from_slice(&[right.x, right.y * scale, right.z]);
            let radius = dust.def[dc::RADIUS];
            for k in 0..dust.instances {
                let at = (dust.first_instance + k) * 3;
                let at = Vector3::new(self.instances[at], self.instances[at + 1], self.instances[at + 2]);
                let fade = dust.fade((at.x - eye.x) * forward.x + (at.y - eye.y) * forward.y + (at.z - eye.z) * forward.z);
                if fade <= 0.0 || !frustum.intersects_sphere(&at, radius) {
                    continue;
                }
                objects += 1;
                for p in dust.particles.iter() {
                    if !(p[9] > 0.0) {
                        continue;
                    }
                    let x = at.x + p[0] as f64;
                    let y = at.y + p[1] as f64 * scale;
                    let z = at.z + p[2] as f64;
                    let depth = (x - eye.x) * forward.x + (y - eye.y) * forward.y + (z - eye.z) * forward.z;
                    self.batches[b].push(depth, x, y, z, p[6] as f64, 0.0, p[9] as f64 * fade);
                    dust_count += 1;
                }
            }
            b += 1;
        }
        result[st::FOG] = fog_count as f64;
        result[st::DUST] = dust_count as f64;
        result[st::OBJECTS] = objects as f64;
        for (k, batch) in self.batches.iter_mut().enumerate() {
            batch.end();
            result[stats + k * sb::STRIDE + sb::COUNT] = batch.count as f64;
        }
    }
}
