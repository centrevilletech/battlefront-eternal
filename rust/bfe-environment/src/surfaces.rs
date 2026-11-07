//! Water.update, updateWaders and updateRings, plus OceanSurface.update and
//! Foliage.update's clocks. Resource shells borrow these fixed output blocks.
//! Commands precede advancement, preserving a wake's preceding-frame timestamp.
use crate::layout::{surface as lim, surface_command as cmd, surface_command_kind as kind,
    surface_config as cfg, surface_frame as frame, surface_result as out};
use crate::shore::{Queries, Shore};
use crate::surface_math::Math;

#[derive(Clone, Copy, Default)]
struct Ripple { x: f64, z: f64, age: f64, life: f64, radius: f64, velocity: Option<[f64; 3]> }
#[derive(Clone, Copy, Default)]
struct Wader { id: f64, position: [f64; 3], speed: f64, seen: f64, depth: f64 }

pub struct Phase {
    pub config: Vec<f64>,
    pub frame: Vec<f64>,
    pub commands: Vec<f64>,
    pub result: Vec<f64>,
    pub positions: Vec<f32>,
    pub uvs: Vec<f32>,
    pub alpha: Vec<f32>,
    pub shore: Shore,
    pub storm: crate::storm::Storm,
    ripples: [Ripple; lim::RIPPLES],
    ripple_count: usize,
    waders: Vec<Wader>,
    wader_count: usize,
    order: Vec<usize>,
    time: f64,
    ocean_time: f64,
    foliage_time: f64,
    overflow: u64,
    circle_sin: [f64; lim::SEGMENTS + 1],
    circle_cos: [f64; lim::SEGMENTS + 1],
}
impl Default for Phase { fn default() -> Self { Self::new() } }
impl Phase {
    // Water.ringMesh allocated for 100 rings of 40 segments. The native owner
    // keeps that exact bound and replaces the oldest birth when it is full.
    // Waders are paired by stable contact id, never by a transient list index.
    pub fn new() -> Self {
        let vertices = lim::RIPPLES * lim::SEGMENTS * 4;
        let mut phase = Self {
            config: vec![0.0; cfg::STRIDE], frame: vec![0.0; frame::STRIDE],
            commands: vec![0.0; lim::COMMANDS * cmd::STRIDE], result: vec![0.0; out::STRIDE],
            positions: vec![0.0; vertices * 3], uvs: vec![0.0; vertices * 2], alpha: vec![0.0; vertices],
            storm: crate::storm::Storm::new(), shore: Shore::new(), ripples: [Ripple::default(); lim::RIPPLES], ripple_count: 0,
            waders: vec![Wader::default(); lim::WADERS], wader_count: 0, order: vec![0; lim::WADERS],
            time: 0.0, ocean_time: 0.0, foliage_time: 0.0, overflow: 0,
            circle_sin: [0.0; lim::SEGMENTS + 1], circle_cos: [0.0; lim::SEGMENTS + 1],
        };
        phase.prepare_math::<crate::surface_math::V8>(); phase
    }
    // Water.ripple and soldierStep's contact publication become scalar rows.
    // A paused burst is bounded on both sides. Extra command rows are counted;
    // no count from a host can index past an allocated command block.
    // updateRings uses the same forty arc endpoints for every ring. Resolve
    // their pixel-visible primitives once, with the host's arithmetic, rather
    // than crossing the math boundary for identical angles every frame.
    pub fn prepare_math<M: Math>(&mut self) {
        for i in 0..=lim::SEGMENTS {
            let a = (i as f64 / lim::SEGMENTS as f64) * 2.0 * std::f64::consts::PI;
            self.circle_sin[i] = M::sin(a); self.circle_cos[i] = M::cos(a);
        }
    }
    // Birth commands are consumed in arrival order. Replacing a full ripple
    // table shifts only its small fixed record array, just as Water.ripple
    // discarded its oldest entry before appending a new wake.
    // Contact rows carry the source id because input order can change between
    // ticks when units die, respawn or are hidden by a presentation boundary.
    fn apply<M: Math>(&mut self) {
        let count = (self.frame[frame::COMMANDS] as usize).min(lim::COMMANDS);
        if self.frame[frame::COMMANDS] > lim::COMMANDS as f64 { self.overflow += 1; }
        // The host queue is circular so a long pause cannot cause repeated
        // whole-buffer copies. Clamp its starting row independently of count.
        // Refusal totals are cumulative and remain visible after the burst.
        let start = (self.frame[frame::COMMAND_START] as usize).min(lim::COMMANDS - 1);
        self.overflow = self.overflow.saturating_add(self.frame[frame::COMMAND_DROPPED] as u64);
        for ordinal in 0..count {
            let i = (start + ordinal) % lim::COMMANDS;
            let c = &self.commands[i * cmd::STRIDE..(i + 1) * cmd::STRIDE];
            if c[cmd::KIND] == kind::RIPPLE {
                if self.ripple_count == lim::RIPPLES {
                    self.ripples.copy_within(1..lim::RIPPLES, 0);
                    self.ripple_count -= 1;
                }
                self.ripples[self.ripple_count] = Ripple { x: c[cmd::POSITION], z: c[cmd::POSITION + 2],
                    age: 0.0, life: c[cmd::LIFE], radius: c[cmd::RADIUS],
                    velocity: if c[cmd::HAS_VELOCITY] != 0.0 { Some([c[cmd::VELOCITY], c[cmd::VELOCITY + 1], c[cmd::VELOCITY + 2]]) } else { None } };
                self.ripple_count += 1;
            } else if c[cmd::KIND] == kind::WADER {
                let id = c[cmd::ID];
                if !id.is_finite() || id <= 0.0 { continue; }
                let found = self.waders[..self.wader_count].iter().position(|w| w.id == id);
                let at = if let Some(at) = found { at } else {
                    if self.wader_count == lim::WADERS { self.overflow += 1; continue; }
                    self.wader_count += 1; self.wader_count - 1
                };
                self.waders[at] = Wader { id, position: [c[cmd::POSITION], c[cmd::POSITION + 1], c[cmd::POSITION + 2]],
                    speed: M::hypot(c[cmd::VELOCITY], c[cmd::VELOCITY + 2]), seen: self.time, depth: c[cmd::DEPTH] };

            }
        }
        self.frame[frame::COMMANDS] = 0.0; self.frame[frame::COMMAND_DROPPED] = 0.0;
    }
    // Water.update advances even when its flat surface is not drawn. An ocean
    // has a separate clock because it pauses while its material is disabled.
    // The physics water level arrives as a fact and must not be replaced by
    // this display clock: ring height used WaterState.level in JavaScript.
    pub fn update<M: Math, Q: Queries>(&mut self, queries: &mut Q) {
        self.apply::<M>();
        let dt = self.frame[frame::DT];
        self.time += dt;
        self.foliage_time += dt;
        self.result[out::TIME] = self.time;
        self.result[out::FOLIAGE_TIME] = self.foliage_time;
        self.result[out::WAVE] = if self.config[cfg::BOBBING] != 0.0 { 0.06 * M::sin(2.0 * std::f64::consts::PI * 0.3 * self.time) } else { 0.0 };
        // Water.update's three texture sequences keep separate rates and sizes.
        // A missing sequence leaves its resource shell's blank texture intact.
        for (count, fps, dest) in [(cfg::NORMAL_COUNT, cfg::NORMAL_FPS, out::NORMAL_A),
            (cfg::BUMP_COUNT, cfg::BUMP_FPS, out::BUMP_A), (cfg::MASK_COUNT, cfg::MASK_FPS, out::MASK_A)] {
            let n = self.config[count] as usize;
            if n == 0 { continue; }
            let f = self.config[fps] * self.time;
            let k = f.floor();
            self.result[dest] = k % n as f64;
            self.result[dest + 1] = (k + 1.0) % n as f64;
            self.result[dest + 2] = f - k;
        }
        // Water.update normalizes the negated sun and camera forward vectors.
        // The forward vector is already a camera fact, rather than a JS query
        // that updates its matrix a second time in this phase.
        // Vector3.normalize divides by length or one for a zero vector.
        // Its reciprocal multiply also preserves Three's final rounding.
        // Sun and camera facts are read once, before output publication.
        let sun = frame::SUN;
        let (x, y, z) = (-self.frame[sun], -self.frame[sun + 1], -self.frame[sun + 2]);
        let length = (x * x + y * y + z * z).sqrt();
        let inverse = 1.0 / if length == 0.0 { 1.0 } else { length };
        self.result[out::SUN..out::SUN + 3].copy_from_slice(&[x * inverse, y * inverse, z * inverse]);
        let f = frame::FORWARD;
        let forward_length = (self.frame[f]*self.frame[f] + self.frame[f+1]*self.frame[f+1] + self.frame[f+2]*self.frame[f+2]).sqrt();
        let inverse = 1.0 / if forward_length == 0.0 { 1.0 } else { forward_length };
        self.result[out::YAW] = M::atan2(self.frame[f]*inverse, self.frame[f + 2]*inverse) - std::f64::consts::PI / 2.0;
        self.update_waders::<M>();
        self.rings::<M>(dt);
        if self.frame[frame::OCEAN] != 0.0 { self.ocean_time += dt; }
        self.result[out::OCEAN_TIME] = self.ocean_time;
        // OceanSurface.update called the storm only for the Enhanced material.
        // Disabling it retains the scheduled strike and cosmetic RNG state.
        // A later re-enable therefore continues the same sequence of bolts.
        self.storm.update::<M>(self.ocean_time,[self.frame[frame::EYE],self.frame[frame::EYE+1],self.frame[frame::EYE+2]],self.frame[frame::STORM]!=0.0);
        self.result[out::STORM_FLASH]=self.storm.flash;
        self.result[out::STORM_POSITION..out::STORM_POSITION+3].copy_from_slice(&self.storm.position);
        self.result[out::STORM_COUNT]=self.storm.count as f64;self.result[out::STORM_DIRTY]=f64::from(self.storm.dirty);
        self.result[out::STORM_STRIKES]=self.storm.strikes as f64;
        self.result[out::OCEAN_X] = (self.frame[frame::EYE] / 8.0).floor() * 8.0;
        self.result[out::OCEAN_Z] = (self.frame[frame::EYE + 2] / 8.0).floor() * 8.0;
        // Shore has its own quality-gated clock; water time always advances.
        self.shore.update::<M, Q>(dt, self.frame[frame::EYE], self.frame[frame::EYE + 2],
            self.config[cfg::HEIGHT], self.frame[frame::SHORE_QUALITY] as usize, queries);
        self.result[out::SHORE_TIME] = self.shore.time;
        self.result[out::SHORE_COUNT] = self.shore.count as f64;
        self.result[out::SHORE_DIRTY] = f64::from(self.shore.dirty);
        self.result[out::SHORE_TILES] = self.shore.tiles() as f64;
        self.result[out::OVERFLOW] = (self.overflow + self.shore.overflow) as f64;
    }
    fn update_waders<M: Math>(&mut self) {
        // Water.updateWaders deletes stale contacts in insertion order. Stable
        // ordering matters when two soldiers are equally far from the camera.
        let mut n = 0;
        for i in 0..self.wader_count {
            let w = self.waders[i];
            if self.time - w.seen > 0.25 { continue; }
            self.waders[n] = w; self.order[n] = n; n += 1;
        }
        self.wader_count = n;
        let eye = frame::EYE;
        // Keep the squared three-dimensional distance from updateWaders.
        // Horizontal distance alone changes ties for soldiers on steps.
        // Ordinal ties preserve the Map's surviving insertion order.
        let distance = |i: usize| { let p = self.waders[i].position;
            let x = p[0] - self.frame[eye]; let y = p[1] - self.frame[eye + 1]; let z = p[2] - self.frame[eye + 2]; x*x + y*y + z*z };
        if n > 8 { self.order[..n].sort_unstable_by(|a, b| distance(*a).total_cmp(&distance(*b)).then(a.cmp(b))); }
        for i in 0..8 {
            let at = out::WADERS + i * 4;
            if i < n { let w = self.waders[self.order[i]];
                self.result[at..at + 4].copy_from_slice(&[w.position[0], w.position[2], w.depth, (0.55 + w.speed * 0.12).min(1.0)]);
            } else { self.result[at + 3] = 0.0; }
        }
    }
    fn rings<M: Math>(&mut self, dt: f64) {
        // Water.update's expiry happens before updateRings builds any vertices.
        // Filtering compacts in place, retaining birth order and fixed storage.
        let mut n = 0;
        for i in 0..self.ripple_count {
            let mut r = self.ripples[i]; r.age += dt;
            if r.age < r.life && M::hypot(r.x - self.frame[frame::EYE], r.z - self.frame[frame::EYE + 2]) < 120.0 {
                self.ripples[n] = r; n += 1;
            }
        }
        self.ripple_count = n;
        let mut q = 0;
        let y = self.frame[frame::LEVEL] + 0.01;
        for r in &self.ripples[..n] {
            let f = r.age / r.life;
            let k = if f < 0.15 { f / 0.15 } else { (1.0 - f) / 0.85 };
            let a = (0.25 * r.life * k).min(0.75);
            let radius = r.radius + 1.5 * r.age;
            // The old wake shaped its brightness from full velocity length,
            // but stretched its opening from the horizontal two-argument norm.
            // Those two lengths intentionally remain separate computations.
            let shape = r.velocity.map(|v| {
                let speed = (v[0]*v[0] + v[1]*v[1] + v[2]*v[2]).sqrt();
                let mut base = a * if speed < 1.5 { speed / 1.5 } else { 1.0 };
                let m = M::hypot(v[0], v[2]) / 1.5;
                let g = 4.0 * ((m - 1.0) * (m - 1.0)) + 0.4;
                base *= g / (g + 1.0);
                (base, m, M::atan2(v[2], v[0]))
            });
            for s in 0..lim::SEGMENTS {
                let a0 = (s as f64 / lim::SEGMENTS as f64) * 2.0 * std::f64::consts::PI;
                let a1 = ((s + 1) as f64 / lim::SEGMENTS as f64) * 2.0 * std::f64::consts::PI;
                let mut alpha = a;
                if let Some((base, m, heading)) = shape {
                    let c = M::cos((a0 + a1) / 2.0 - heading);
                    let w = if m <= 1.0 { (1.0 - ((1.0-c)*m)*((1.0-c)*m))*(2.0-m) }
                        else { 1.0 - (1.0-c*m)*(1.0-c*m)/m };
                    alpha = base * w.max(0.0);
                }
                // Invisible segments contribute no vertices or indices.
                // A partially visible V therefore remains a compact prefix,
                // rather than reserving gaps in the renderer's draw range.
                if alpha <= 1.0 / 255.0 { continue; }
                let outer = radius + 0.35; let inner = (radius - 0.35).max(0.0);
                // BufferAttribute.setXYZ/setXY rounded only at final storage.
                // Keep all intermediate arithmetic f64, including arc angles.
                let points = [[r.x + self.circle_cos[s]*outer, y, r.z + self.circle_sin[s]*outer],
                    [r.x + self.circle_cos[s]*inner, y, r.z + self.circle_sin[s]*inner],
                    [r.x + self.circle_cos[s+1]*outer, y, r.z + self.circle_sin[s+1]*outer],
                    [r.x + self.circle_cos[s+1]*inner, y, r.z + self.circle_sin[s+1]*inner]];
                for v in 0..4 {
                    let at = q*4+v;
                    for axis in 0..3 { self.positions[at*3+axis] = points[v][axis] as f32; }
                    self.uvs[at*2] = ((s + usize::from(v >= 2)) as f64 * 0.25) as f32;
                    self.uvs[at*2+1] = (v & 1) as f32;
                    self.alpha[at] = alpha as f32;
                }
                q += 1;
            }
        }
        self.result[out::QUADS] = q as f64;
    }
}
