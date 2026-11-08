//! DroidekaTrail.update/probe: cached outdoor tests and bounded rolling dust.
//! Query rounds cross only floor/roof facts; Rust owns cadence and ray origin.
//!

//!
//! The trail clock is separate from world time and clamps displayed dt. Pauses
//! cannot create an unbounded burst when the next frame finally arrives.
//! Rolling, grounded, alive and dry are independent gameplay facts. Being near
//! the water's surface suppresses dust even if a floor ray finds solid ground.
//!
//! The floor/roof probe is cached by both time and horizontal travel. Rust
//! requests its floor origin before the host calls collision. A valid upward
//! floor normal supplies the cached floor height and terrain flag; only then is
//! the roof origin constructed. A miss or steep floor leaves outdoor false.
//! The roof query preserves the original eighty-metre open-sky test.
//!
//! Moving into cover stops new births while old dust continues to age. A mode
//! change to Classic clears the active prefix immediately, rather than letting
//! Enhanced dust remain visible. Teleports clear the old pool and fractional
//! birth carry before a new path can be drawn across the map.
//!
//! Emission strength depends on horizontal speed and change of direction.
//! The cross term measures turning without wrapping an angle. Birth carry keeps
//! fractional counts between frames; a full pool discards the integral excess
//! rather than retaining a delayed explosion of dust after particles expire.
//!
//! New particles interpolate their birth points along the previous drawn path.
//! Their vertical position comes from the cached floor, so stair correction does
//! not make a dust ribbon float at the body's smoothed height. Terrain and paved
//! surfaces share timing but retain different opacity. The rounded records feed
//! three four-float GPU streams without a browser motion implementation.

//!
//! The generated trail record groups birth, drift and shape facts into the
//! same twelve float32 values the renderer consumes. Position and drift remain
//! world-space; shader age comes from the independent trail clock minus birth.
//! Swapping an expired record preserves all three groups as one identity.
//!
//! Cached terrain is meaningful only after a valid floor result. A roof miss
//! cannot by itself establish outdoor ground. Rejecting the floor first also
//! preserves the original number and ordering of collision service calls.
//!
//! Speed uses hypot on x/z, while travel and teleport checks use squared vector
//! distance. Replacing one with the other can change a threshold at its edge.
//! The turning term similarly keeps its cross-product expression and original
//! minimum denominator instead of deriving an angular velocity with atan2.

use super::{lerp, put, vector};
use crate::visual_effects_layout::{trail_frame as f, trail_particle as p};
use three::Vector3;
pub struct Trail {
    pub input: Vec<f64>,
    pub records: Vec<f32>,
    pub rays: Vec<f64>,
    previous: Vector3,
    probe_position: Vector3,
    probe_at: f64,
    ready: bool,
    outdoor: bool,
    terrain: bool,
    floor: f64,
    carry: f64,
    last_x: f64,
    last_z: f64,
    count: usize,
}
impl Default for Trail {
    fn default() -> Self {
        Self {
            input: vec![0.; f::STRIDE],
            records: vec![0.; 160 * p::STRIDE],
            rays: vec![0.; 3],
            previous: Vector3::default(),
            probe_position: Vector3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            probe_at: f64::NEG_INFINITY,
            ready: false,
            outdoor: false,
            terrain: false,
            floor: 0.,
            carry: 0.,
            last_x: 0.,
            last_z: 0.,
            count: 0,
        }
    }
}
impl Trail {
    // DroidekaTrail.dustTint is authored map policy, admitted with the pool.
    pub fn new(name: &str) -> Self {
        let mut trail = Self::default();
        let name = name.to_ascii_lowercase();
        let tint = if name.contains("hot") {
            [0.78, 0.83, 0.86]
        } else if ["tat", "geo"].iter().any(|n| name.contains(n)) {
            [0.66, 0.46, 0.28]
        } else if ["end", "kas", "yav"].iter().any(|n| name.contains(n)) {
            [0.43, 0.37, 0.25]
        } else if ["bes", "kam"].iter().any(|n| name.contains(n)) {
            [0.59, 0.63, 0.65]
        } else {
            [0.68, 0.61, 0.45]
        };
        trail.input[f::TINT..f::TINT + 3].copy_from_slice(&tint);
        trail
    }

    pub fn clear(&mut self) {
        self.count = 0;
        self.carry = 0.;
        self.input[f::COUNT] = 0.;
    }
    fn rolling(&self) -> bool {
        self.input[f::ROLLING] != 0.
            && self.input[f::DEAD] == 0.
            && self.input[f::AIR] == 0.
            && self.input[f::GROUNDED] != 0.
            && v8math::hypot(self.input[f::VELOCITY], self.input[f::VELOCITY + 2]) > 2.2
    }
    // Probe request is computed before the host reads collision. A fresh floor
    // answer supplies the second ray origin, retaining floor-before-roof order.
    pub fn probe(&mut self) {
        let dt = self.input[f::DT].clamp(0., 0.1);
        let time = self.input[f::TIME] + dt;
        let position = vector(&self.input, f::POSITION);
        let needed = self.input[f::ENHANCED] != 0.
            && self.rolling()
            && (time - self.probe_at > 0.08
                || self.probe_position.distance_to_squared(&position) > 0.25);
        self.input[f::PROBE] = needed as u8 as f64;
        let mut origin = position;
        origin.y += 0.6;
        put(&mut self.rays, 0, origin);
    }
    pub fn roof(&mut self) {
        self.input[f::ROOF_PROBE] =
            (self.input[f::FLOOR_HIT] != 0. && self.input[f::FLOOR_NORMAL] >= 0.45) as u8 as f64;
        let position = vector(&self.input, f::POSITION);
        put(
            &mut self.rays,
            0,
            Vector3::new(position.x, self.input[f::FLOOR] + 0.25, position.z),
        );
    }
    pub fn frame(&mut self, random: &mut impl FnMut() -> f64) {
        if self.input[f::CLEAR] != 0. {
            self.input[f::CLEAR] = 0.;
            self.clear();
            return;
        }
        let dt = self.input[f::DT].clamp(0., 0.1);
        self.input[f::TIME] += dt;
        let time = self.input[f::TIME];
        if self.input[f::ENHANCED] == 0. {
            self.clear();
            self.ready = false;
            return;
        }
        let position = vector(&self.input, f::POSITION);
        let jumped = self.ready && self.previous.distance_to_squared(&position) > 100.;
        if jumped {
            self.clear();
        }
        for i in (0..self.count).rev() {
            let at = i * p::STRIDE;
            if time - self.records[at + p::TIME] as f64 >= self.records[at + p::LIFE] as f64 {
                self.count -= 1;
                self.records.copy_within(
                    self.count * p::STRIDE..self.count * p::STRIDE + p::STRIDE,
                    at,
                );
            }
        }
        let rolling = self.rolling();
        if self.input[f::PROBE] != 0. {
            self.probe_at = time;
            self.probe_position = position;
            self.outdoor = false;
            if self.input[f::FLOOR_HIT] != 0. && self.input[f::FLOOR_NORMAL] >= 0.45 {
                self.floor = self.input[f::FLOOR];
                self.terrain = self.input[f::TERRAIN] != 0.;
                self.outdoor = self.input[f::ROOF] == 0.;
            }
        }
        let vx = self.input[f::VELOCITY];
        let vz = self.input[f::VELOCITY + 2];
        let speed = v8math::hypot(vx, vz);
        let wet = self.input[f::WET] != 0. && position.y < self.input[f::WATER_LEVEL] + 0.12;
        if rolling && self.outdoor && !wet && dt > 0. {
            let dx = vx / speed;
            let dz = vz / speed;
            let turn = if self.ready {
                ((dx * self.last_z - dz * self.last_x).abs() / (dt * 2.).max(0.01)).min(1.)
            } else {
                0.
            };
            let strength = ((speed - 2.2) / 8.).clamp(0., 1.);
            self.carry += dt * (18. + strength * 38. + turn * 22.);
            let spawn = (self.carry.floor() as usize).min(160 - self.count);
            self.carry -= self.carry.floor();
            for k in 0..spawn {
                let at = self.count * p::STRIDE;
                self.count += 1;
                let fraction = (k + 1) as f64 / spawn.max(1) as f64;
                let spread = (random() - 0.5) * (0.55 + turn * 0.4);
                let r = &mut self.records;
                r[at + p::POSITION] = ((if self.ready && !jumped {
                    lerp(self.previous.x, position.x, fraction)
                } else {
                    position.x
                }) - dx * 0.4
                    + dz * spread) as f32;
                r[at + p::POSITION + 1] = (self.floor + 0.14 + random() * 0.12) as f32;
                r[at + p::POSITION + 2] = ((if self.ready && !jumped {
                    lerp(self.previous.z, position.z, fraction)
                } else {
                    position.z
                }) - dz * 0.4
                    - dx * spread) as f32;
                r[at + p::TIME] = time as f32;
                r[at + p::VELOCITY] = (-dx * (0.4 + strength * 0.8) + dz * spread * 2.) as f32;
                r[at + p::VELOCITY + 1] = (0.1 + random() * 0.25) as f32;
                r[at + p::VELOCITY + 2] = (-dz * (0.4 + strength * 0.8) - dx * spread * 2.) as f32;
                r[at + p::LIFE] = (1.2 + random() * 0.8) as f32;
                r[at + p::RADIUS] = (0.3 + strength * 0.2 + random() * 0.17) as f32;
                r[at + p::SEED] = (random() * 30.) as f32;
                r[at + p::OPACITY] = ((if self.terrain { 0.7 } else { 0.42 })
                    * (0.55 + strength * 0.45 + turn * 0.25))
                    as f32;
                r[at + p::TERRAIN] = self.terrain as u8 as f32;
                self.input[f::EMITTED] += 1.;
            }
            self.last_x = dx;
            self.last_z = dz;
        } else {
            self.carry = 0.;
        }
        self.previous = position;
        self.ready = true;
        self.input[f::COUNT] = self.count as f64;
    }
}
