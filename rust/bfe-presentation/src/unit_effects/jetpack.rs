//! JetpackEffects.update, JetpackWake.update and JetpackVolume.update.
//! One frame owns nozzle smoothing, ignition, wake births and volume output.
//!
//! Geometry anchors feed the attached volume; history bends its tail, while
//! detached combustion wisps/smoke travel along their own world-space paths.
//!
//! JetpackEffects owns the attached flame and detached wake as one displayed
//! unit phase. Its local outlet positions and directions are immutable content;
//! mesh-sampled world outlets, when present, override those authored defaults.
//! The first four recognized exhaust definitions retain their authored order.
//! Without any recognized exhaust the original two ribcage outlets are used.
//!
//! Active is a fact from gameplay. Throttle is a visual envelope, with separate
//! ignition and cutoff time constants. A new activation restarts the ignition
//! pulse without restarting either effect clock. Speed adds only the original
//! small power adjustment; it never changes the wake's emission cadence.
//!
//! Each nozzle retains a world tail target. Smoothing that target bends the
//! attached flame while the outlet remains on its mesh. Removing the axial
//! component makes the bend perpendicular to the current exhaust direction.
//! The final bend length is bounded to avoid a wide volume after a sharp turn.
//! Teleporting the body clears both tail history and every detached wake pool.
//!
//! WakePool stores rounded float32 birth times and lifetimes. Expiration reads
//! those stored values, rather than unrounded intermediate doubles, because
//! particles at the edge of their life must survive the same displayed frame.
//! Expired records swap with the final active record in the same pool. Pool
//! order and this reverse removal walk are observable through rendered overlap.
//!
//! Each nozzle has an independent fractional birth phase for each wake kind.
//! A birth lies at its actual fraction through the current frame, along the
//! previous-to-current nozzle path, rather than at the final outlet position.
//! Direction uses the same interpolation fraction and is normalized afterwards.
//! An explicitly supplied previous position can replace retained history only
//! when it lies within the teleport bound. Invalid history never draws a bridge.
//!
//! Combustion, smoke and embers have separate capacities and lifetimes. A full
//! pool refuses a birth before consuming cosmetic random values. Expiration
//! happens after emission, preserving the original full-pool behavior at cutoff.
//! A stopped nozzle clears fractional phases, so restarting cannot emit a saved
//! fraction as an extra particle. Existing detached particles continue to decay.
//!
//! Volume output is an active prefix of at most four nozzles. Power below its
//! visibility threshold hides that prefix but leaves the previous shader facts
//! untouched. Visible power has its original minimum to avoid a collapsed core.
//! The host's requested capacity is bounded before any output array is indexed.
//! Phase offsets are immutable per-nozzle shader facts, not a second clock.
//! The one local light uses the mean of the exhaust points only while active.

//!
//! Frame records distinguish several coordinate spaces:
//! - body position and velocity are world metres and metres per second;
//! - authored outlet position is in the body's local frame;
//! - authored outlet direction is normalized before applying the body turn;
//! - supplied mesh outlet directions are normalized after copying;
//! - retained tail targets and perpendicular bends stay in world coordinates.
//! This distinction avoids applying the body quaternion twice to sampled outlets.
//!
//! The default quaternion is not inferred from direction. The host supplies the
//! body's drawn quaternion directly, preserving roll and every authored outlet.
//! Position applies that quaternion before adding the displayed body position.
//! The flame and its light therefore follow the same frame as the visible mesh.
//! Only detached wake positions interpolate through the previous display interval.
//!
//! State lifetime also differs between the attached and detached effects:
//! - time continues while Enhanced effects are disabled;
//! - disabling clears active particles, throttle and nozzle history;
//! - re-enabling begins with fresh positions rather than an old tail;
//! - cutoff leaves existing detached particles alive until their rounded expiry;
//! - an active-to-inactive transition does not restart the ignition birth clock.
//! Clear does not reset the historical emitted counter used by diagnostics.
//!
//! A particle consumes random values in its original attribute order:
//! speed, downstream distance where applicable, three velocity components,
//! lifetime, radius and noise seed. Embers omit the downstream random draw.
//! Changing that order would alter all later particles even with the same seed.
//! The opacity factor uses throttle at the birth, rather than current throttle.
//! It is retained with the particle so shutdown cannot darken old smoke at once.
//!
//! Fractional emission counts use floor(total + epsilon). The epsilon permits a
//! mathematically complete birth at a display interval boundary after accumulated
//! rounding. It does not accumulate as extra phase: only the unmodified total is
//! used when retaining the fractional remainder for the next frame.
//! Birth fractions clamp to the interval before interpolating any vector.
//!
//! The pools expire after processing all nozzle births for the frame. This
//! retains overlap order when two nozzles share a full pool. A swap removal copies
//! all fourteen float32 values together, including the noise seed and opacity;
//! copying only position would attach another particle's appearance to that slot.
//! Each kind's storage starts after the preceding kind's complete capacity.
//! Count cannot spill across that boundary even with an oversized nozzle input.
//!
//! The attached volume reads positions, directions, bends and outlet radii as
//! separate shader attributes. Those streams are merely alternate views of the
//! native result and have no independent smoothing or emission state.
//! The phase offset is assigned by nozzle ordinal; it is not an effect id or
//! an index into the detached pools. Its large separation decorrelates noise.
//!
//! Light position adds the same small downstream offset at each nozzle before
//! forming the mean. Strength follows retained throttle, but the active flag
//! still gates whether a request is emitted. A decaying attached flame after
//! cutoff therefore does not keep requesting a gameplay point light.

use super::{lerp, put, vector};
use crate::visual_effects_layout::{
    jet_frame as j, jet_local as l, jet_nozzle as n, volume_frame as v, volume_nozzle as z,
    wake_frame as w, wake_particle as p,
};
use three::{Quaternion, Vector3};
const CAP: [usize; 3] = [96, 128, 64];
const RATE: [f64; 3] = [36.0, 15.0, 4.5];
const LIFE: [[f64; 2]; 3] = [[0.16, 0.27], [0.65, 1.05], [0.2, 0.45]];
const OPACITY: [f64; 3] = [0.8, 0.12, 0.75];
const WIDTH: usize = p::STRIDE;
#[derive(Clone, Copy, Default)]
struct Emitter {
    position: Vector3,
    direction: Vector3,
    ready: bool,
    phase: [f64; 3],
}
// Pools store float32 at the original assignment boundary. Expiration must
// read those rounded births/lifetimes, including swap-with-last compaction.
pub struct Wake {
    pub input: Vec<f64>,
    pub nozzles: Vec<f64>,
    pub particles: Vec<f32>,
    emitters: [Emitter; 4],
    counts: [usize; 3],
    time: f64,
    emitted: usize,
}
impl Default for Wake {
    fn default() -> Self {
        Self {
            input: vec![0.; w::STRIDE],
            nozzles: vec![0.; 4 * n::STRIDE],
            particles: vec![0.; 288 * WIDTH],
            emitters: [Emitter::default(); 4],
            counts: [0; 3],
            time: 0.,
            emitted: 0,
        }
    }
}
impl Wake {
    pub fn clear(&mut self) {
        self.counts = [0; 3];
        self.emitters = [Emitter::default(); 4];
        for k in 0..3 {
            self.input[w::COUNTS + k] = 0.;
            self.input[w::DIRTY + k] = 0.;
        }
    }
    // WakePool.emit retains the random draw order: a full pool consumes none.
    fn emit(
        &mut self,
        kind: usize,
        time: f64,
        p: Vector3,
        d: Vector3,
        velocity: Vector3,
        radius: f64,
        throttle: f64,
        random: &mut impl FnMut() -> f64,
    ) -> bool {
        if self.counts[kind] >= CAP[kind] {
            return false;
        }
        let base = if kind == 0 {
            0
        } else if kind == 1 {
            96
        } else {
            224
        };
        let at = (base + self.counts[kind]) * WIDTH;
        self.counts[kind] += 1;
        let speed = if kind == 0 {
            4. + random() * 2.
        } else if kind == 1 {
            1.8 + random() * 1.1
        } else {
            4. + random() * 3.
        };
        let downstream = if kind == 0 {
            0.12 + random() * 0.08
        } else if kind == 1 {
            0.36 + random() * 0.12
        } else {
            0.15
        };
        let spread = if kind == 2 { 0.9 } else { 0.3 };
        let r = &mut self.particles;
        r[at + p::POSITION] = (p.x + d.x * downstream) as f32;
        r[at + p::POSITION + 1] = (p.y + d.y * downstream) as f32;
        r[at + p::POSITION + 2] = (p.z + d.z * downstream) as f32;
        r[at + p::TIME] = time as f32;
        r[at + p::VELOCITY] = (velocity.x * 0.72 + d.x * speed + (random() - 0.5) * spread) as f32;
        r[at + p::VELOCITY + 1] =
            (velocity.y * 0.72 + d.y * speed + (random() - 0.5) * spread) as f32;
        r[at + p::VELOCITY + 2] =
            (velocity.z * 0.72 + d.z * speed + (random() - 0.5) * spread) as f32;
        r[at + p::LIFE] = lerp(LIFE[kind][0], LIFE[kind][1], random()) as f32;
        r[at + p::DIRECTION] = d.x as f32;
        r[at + p::DIRECTION + 1] = d.y as f32;
        r[at + p::DIRECTION + 2] = d.z as f32;
        r[at + p::RADIUS] = (if kind == 0 {
            radius * (1.1 + random())
        } else if kind == 1 {
            radius * (2.0 + random() * 1.1)
        } else {
            0.004 + random() * 0.003
        }) as f32;
        r[at + p::SEED] = (random() * 100.) as f32;
        r[at + p::OPACITY] = (OPACITY[kind] * (0.6 + throttle * 0.4)) as f32;
        self.input[w::DIRTY + kind] = 1.;
        true
    }
    pub fn frame(&mut self, random: &mut impl FnMut() -> f64) {
        if self.input[w::CLEAR] != 0. {
            self.input[w::CLEAR] = 0.;
            self.clear();
            return;
        }
        let dt = self.input[w::DT].clamp(0., 0.25);
        self.time += dt;
        let throttle = self.input[w::THROTTLE].clamp(0., 1.);
        let count = (self.input[w::NOZZLES] as usize).min(4);
        let velocity = vector(&self.input, w::VELOCITY);
        // A teleport invalidates every nozzle's old trail before any birth.
        for i in 0..count {
            if self.emitters[i].ready
                && self.emitters[i]
                    .position
                    .distance_to_squared(&vector(&self.nozzles, i * n::STRIDE + n::POSITION))
                    > 64.
            {
                self.clear();
                break;
            }
        }
        for i in 0..4 {
            if self.input[w::ACTIVE] == 0. || throttle < 0.01 || i >= count {
                self.emitters[i].ready = false;
                self.emitters[i].phase = [0.; 3];
                continue;
            }
            let at = i * n::STRIDE;
            let mut previous = self.emitters[i];
            let position = vector(&self.nozzles, at + n::POSITION);
            let direction = vector(&self.nozzles, at + n::DIRECTION);
            let supplied = vector(&self.nozzles, at + n::PREVIOUS);
            let start = if self.nozzles[at + n::HAS_PREVIOUS] != 0.
                && supplied.distance_to_squared(&position) <= 64.
            {
                supplied
            } else if previous.ready {
                previous.position
            } else {
                position
            };
            let old = if previous.ready {
                previous.direction
            } else {
                direction
            };
            for kind in 0..3 {
                let rate = RATE[kind] * throttle;
                let phase = previous.phase[kind];
                let total = phase + dt * rate;
                let births = (total + 1e-9).floor();
                previous.phase[kind] = (total - births).max(0.);
                for birth in 0..births as usize {
                    let fraction = if dt > 0. {
                        ((birth as f64 + 1. - phase) / (dt * rate)).clamp(0., 1.)
                    } else {
                        1.
                    };
                    let mut p = Vector3::default();
                    p.lerp_vectors(&start, &position, fraction);
                    let mut d = Vector3::default();
                    d.lerp_vectors(&old, &direction, fraction).normalize();
                    let radius = self.nozzles[at + n::RADIUS];
                    if self.emit(
                        kind,
                        self.time - dt + fraction * dt,
                        p,
                        d,
                        velocity,
                        if radius == 0. || radius.is_nan() {
                            0.034
                        } else {
                            radius
                        },
                        throttle,
                        random,
                    ) {
                        self.emitted += 1;
                    }
                }
            }
            previous.position = position;
            previous.direction = direction;
            previous.ready = true;
            self.emitters[i] = previous;
        }
        for kind in 0..3 {
            let base = if kind == 0 {
                0
            } else if kind == 1 {
                96
            } else {
                224
            };
            for index in (0..self.counts[kind]).rev() {
                let at = (base + index) * WIDTH;
                if self.time - (self.particles[at + p::TIME] as f64)
                    < (self.particles[at + p::LIFE] as f64)
                {
                    continue;
                }
                self.counts[kind] -= 1;
                let from = (base + self.counts[kind]) * WIDTH;
                self.particles.copy_within(from..from + WIDTH, at);
                self.input[w::DIRTY + kind] = 1.;
            }
            self.input[w::COUNTS + kind] = self.counts[kind] as f64;
        }
        self.input[w::TIME] = self.time;
        self.input[w::EMITTED] = self.emitted as f64;
    }
}
// The volume is also callable for retained diagnostic scenes; production
// Jetpack owns it together with the wake and never resmooths nozzle facts.
pub struct Volume {
    pub input: Vec<f64>,
    pub nozzles: Vec<f64>,
    pub arrays: Vec<f32>,
}
impl Default for Volume {
    fn default() -> Self {
        let mut input = vec![0.; v::STRIDE];
        input[v::CAPACITY] = 4.;
        Self {
            input,
            nozzles: vec![0.; 4 * n::STRIDE],
            arrays: vec![0.; 4 * z::STRIDE],
        }
    }
}
impl Volume {
    pub fn frame(&mut self) {
        let shown = self.input[v::POWER] > 0.015;
        self.input[v::SHOWN] = shown as u8 as f64;
        let count = if shown {
            (self.input[v::COUNT] as usize)
                .min(4)
                .min(self.input[v::CAPACITY] as usize)
        } else {
            0
        };
        self.input[v::COUNT] = count as f64;
        if !shown {
            return;
        }
        self.input[v::POWER] = self.input[v::POWER].max(0.025);
        for i in 0..count {
            let at = i * n::STRIDE;
            for k in 0..3 {
                self.arrays[i * z::STRIDE + z::POSITION + k] =
                    self.nozzles[at + n::POSITION + k] as f32;
                self.arrays[i * z::STRIDE + z::DIRECTION + k] =
                    self.nozzles[at + n::DIRECTION + k] as f32;
                self.arrays[i * z::STRIDE + z::BEND + k] = self.nozzles[at + n::BEND + k] as f32;
            }
            let radius = self.nozzles[at + n::RADIUS];
            self.arrays[i * z::STRIDE + z::RADIUS] = (if radius == 0. || radius.is_nan() {
                0.034
            } else {
                radius
            }) as f32;
            self.arrays[i * z::STRIDE + z::PHASE] = (i as f64 * 17.31) as f32;
        }
    }
}
pub struct Jetpack {
    pub input: Vec<f64>,
    pub local: Vec<f64>,
    pub nozzles: Vec<f64>,
    pub wake: Wake,
    pub volume: Volume,
    last: Vector3,
    has_position: bool,
    was_active: bool,
    tails: [Vector3; 4],
    ready: [bool; 4],
}
impl Default for Jetpack {
    fn default() -> Self {
        let mut input = vec![0.; j::STRIDE];
        input[j::IGNITION] = 10.;
        Self {
            input,
            local: vec![0.; 4 * l::STRIDE],
            nozzles: vec![0.; 4 * n::STRIDE],
            wake: Wake::default(),
            volume: Volume::default(),
            last: Vector3::default(),
            has_position: false,
            was_active: false,
            tails: [Vector3::default(); 4],
            ready: [false; 4],
        }
    }
}
#[derive(serde::Deserialize, Default)]
#[serde(default)]
pub struct Definition {
    name: String,
    spawner: Spawner,
}
#[derive(serde::Deserialize, Default)]
#[serde(default)]
pub struct Spawner {
    offset: Option<[[f64; 2]; 3]>,
    r#box: Option<[[f64; 2]; 3]>,
}
impl Jetpack {
    // JetpackEffects.constructor admits the authored outlets and their midpoints.
    // Missing exhaust definitions retain the original paired ribcage defaults.
    pub fn new(definitions: Vec<Definition>) -> Self {
        let mut phase = Self::default();
        let mut count = 0;
        for def in &definitions {
            if !["JetExhaust", "JetExhaust_Left", "JetExhaust_Right"]
                .iter()
                .any(|name| def.name.eq_ignore_ascii_case(name))
            {
                continue;
            }
            if count == 4 {
                break;
            }
            phase.outlet(count, Some(&def.spawner));
            count += 1;
        }
        if count == 0 {
            phase.outlet(0, None);
            phase.outlet(1, None);
            count = 2;
        }
        phase.input[j::NOZZLES] = count as f64;
        phase
    }
    fn outlet(&mut self, index: usize, spawner: Option<&Spawner>) {
        let position = spawner
            .and_then(|s| s.offset)
            .map(|p| {
                Vector3::new(
                    (p[0][0] + p[0][1]) * 0.5,
                    (p[1][0] + p[1][1]) * 0.5,
                    (p[2][0] + p[2][1]) * 0.5,
                )
            })
            .unwrap_or(Vector3::new(
                -0.24,
                -0.21,
                if index != 0 { -0.15 } else { 0.15 },
            ));
        let mut direction = spawner
            .and_then(|s| s.r#box)
            .map(|p| {
                Vector3::new(
                    (p[0][0] + p[0][1]) * 0.5,
                    (p[1][0] + p[1][1]) * 0.5,
                    (p[2][0] + p[2][1]) * 0.5,
                )
            })
            .unwrap_or(Vector3::new(-1., 0., 0.));
        direction.normalize();
        put(&mut self.local, index * l::STRIDE + l::POSITION, position);
        put(&mut self.local, index * l::STRIDE + l::DIRECTION, direction);
    }

    pub fn clear(&mut self) {
        self.wake.clear();
        self.volume.input[v::COUNT] = 0.;
        self.volume.input[v::SHOWN] = 0.;
        self.input[j::THROTTLE] = 0.;
        self.was_active = false;
        self.ready = [false; 4];
    }
    pub fn frame(&mut self, random: &mut impl FnMut() -> f64) {
        if self.input[j::CLEAR] != 0. {
            self.input[j::CLEAR] = 0.;
            self.clear();
            return;
        }
        let dt = self.input[j::DT].clamp(0., 0.1);
        self.input[j::TIME] += dt;
        if self.input[j::ENHANCED] == 0. {
            self.clear();
            self.has_position = false;
            self.input[j::LIGHT_STRENGTH] = 0.;
            return;
        }
        let position = vector(&self.input, j::POSITION);
        if self.has_position && self.last.distance_to_squared(&position) > 100. {
            self.clear();
        }
        self.last = position;
        self.has_position = true;
        let active = self.input[j::ACTIVE] != 0.;
        if active && !self.was_active {
            self.input[j::IGNITION] = 0.;
        }
        self.was_active = active;
        self.input[j::IGNITION] += dt;
        let throttle = &mut self.input[j::THROTTLE];
        *throttle += ((active as u8 as f64) - *throttle)
            * (1. - v8math::exp(-dt / if active { 0.045 } else { 0.07 }));
        if !active && *throttle < 0.015 {
            *throttle = 0.;
        }
        let throttle = *throttle;
        let ignition = self.input[j::IGNITION];
        let pulse = 1.
            + 0.14
                * v8math::sin((ignition / 0.16).min(1.) * std::f64::consts::PI)
                * v8math::exp(-ignition * 5.);
        let velocity = vector(&self.input, j::VELOCITY);
        let power = throttle * pulse * (1. + (velocity.length() / 40.).min(0.12));
        self.input[j::POWER] = power;
        let count = (self.input[j::NOZZLES] as usize).min(4);
        let q = j::QUATERNION;
        let quaternion = Quaternion::new(
            self.input[q],
            self.input[q + 1],
            self.input[q + 2],
            self.input[q + 3],
        );
        let mut light = Vector3::default();
        for i in 0..count {
            let at = i * n::STRIDE;
            let mut p = vector(&self.nozzles, at + n::POSITION);
            let mut d = vector(&self.nozzles, at + n::DIRECTION);
            if self.nozzles[at + n::HAS_PREVIOUS] == 0. {
                p = vector(&self.local, i * l::STRIDE + l::POSITION);
                p.apply_quaternion(&quaternion).add(&position);
                d = vector(&self.local, i * l::STRIDE + l::DIRECTION);
                d.apply_quaternion(&quaternion);
            }
            d.normalize();
            put(&mut self.nozzles, at + n::POSITION, p);
            put(&mut self.nozzles, at + n::DIRECTION, d);
            let mut target = p;
            target.add_scaled_vector(&d, 0.58);
            if !self.ready[i] {
                self.tails[i] = target;
                self.ready[i] = true;
            }
            self.tails[i].lerp(&target, 1. - v8math::exp(-dt / 0.055));
            let mut bend = self.tails[i];
            bend.sub(&target);
            bend.add_scaled_vector(&d, -bend.dot(&d))
                .clamp_length(0., 0.14);
            put(&mut self.nozzles, at + n::BEND, bend);
            light.add(&p).add_scaled_vector(&d, 0.12);
        }
        if count > 0 {
            light.multiply_scalar(1. / count as f64);
        }
        put(&mut self.input, j::LIGHT_POSITION, light);
        self.input[j::LIGHT_STRENGTH] = if active { 0.30 * throttle } else { 0. };
        self.volume.input[v::TIME] = self.input[j::TIME];
        self.volume.input[v::POWER] = power;
        self.volume.input[v::COUNT] = count as f64;
        self.volume.input[v::CAPACITY] = count as f64;
        self.volume.nozzles.copy_from_slice(&self.nozzles);
        self.volume.frame();
        self.wake.input[w::DT] = dt;
        self.wake.input[w::THROTTLE] = throttle;
        self.wake.input[w::ACTIVE] = active as u8 as f64;
        self.wake.input[w::NOZZLES] = count as f64;
        put(&mut self.wake.input, w::VELOCITY, velocity);
        self.wake.nozzles.copy_from_slice(&self.nozzles);
        for i in 0..4 {
            self.wake.nozzles[i * n::STRIDE + n::HAS_PREVIOUS] = 0.;
        }
        self.wake.frame(random);
    }
}
