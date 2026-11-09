//! FirstPersonMotion from src/camera.js. Fixed arrays retain the current
//! segment, velocity and joins; authored keys allocate only at registration.
// The head bob's keys: eye offsets in the camera's frame (x right, y up, z
// back) and their tangents, one per 0.2 s segment.
// The motions whose arms drop and come back up (handsdown, then jump_land
// for the length of the motion).
// One cubic Hermite segment at u in [0, 1], and its slope.
// The first-person camera motions, as the game plays them: a few segments of
// cubic Hermite curves for the eye's offset (Vec and Slope, in the camera's
// frame, m) and the view's turn (Ore and RSlope, Euler x, y, z in rad), each
// segment lasting Time[i] seconds. The globals script sets up the ones for a
// dive to either side, lying down and getting up, and landing; the head bob
// is built in: a 0.8 s cycle lifting the eye 0.1 m twice (0.05 m prone),
// started whenever the soldier moves at 1 m/s or more on his feet with no
// other motion playing, and settled gently when he stops. A motion starts
// from wherever the last one left the eye, at the same velocity.
// keys: { Vec, Slope, Ore, RSlope, Time }.
// Keep velocity continuous even when the new segment has another length.
// The named motion from the globals script, if it has one.
// active: the camera is the soldier's own eyes. The soldier's changes of
// state are followed either way, so none is replayed on switching views.
// The original motion drops the eye a full meter, even after a shallow
// step. Retain a modest impact response for actual jumps and falls.
// Carry leftover frame time into the next segment/motion/bob cycle.

use super::math::*;
use crate::layout::{camera_limits as limit, camera_motion as id};
use serde::Deserialize;
use three::{Quaternion, Vector3};

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Keys {
    #[serde(rename = "Vec")]
    pub position: Vec<[f64; 3]>,
    #[serde(rename = "Slope")]
    pub slope: Vec<[f64; 3]>,
    #[serde(rename = "Ore")]
    pub turn: Vec<[f64; 3]>,
    #[serde(rename = "RSlope")]
    pub turn_slope: Vec<[f64; 3]>,
    #[serde(rename = "Time")]
    pub time: Vec<f64>,
}
#[derive(Default)]
pub struct Motion {
    pub value: [f64; 6],
    slope: [f64; 6],
    start: [f64; 6],
    start_slope: [f64; 6],
    key: usize,
    segment: usize,
    left: f64,
    scale: f64,
    bobbing: bool,
    bob_wanted: bool,
    bob_scale: f64,
    air_time: f64,
    impact: f64,
    was: Option<(usize, usize, f64)>,
    pub cue: f64,
    pub skipped: u64,
}
// Named motions use 0..7; the built-in bob and settle occupy no content row.
const NONE: usize = usize::MAX;
const BOB: usize = limit::MOTIONS;
const SETTLE: usize = BOB + 1;
const BOB_VEC: [[f64; 3]; 4] = [[0.0, 0.1, 0.0], [0.0; 3], [0.0, 0.1, 0.0], [0.0; 3]];
const BOB_SLOPE: [[f64; 3]; 4] = [[0.1, 0.1, 0.0], [0.2, 0.2, 0.0], [0.2, 0.1, 0.0], [0.0; 3]];
impl Motion {
    pub fn new() -> Self {
        Self {
            key: NONE,
            bob_scale: 1.0,
            ..Self::default()
        }
    }
    // FirstPersonMotion's authored motions have a fixed cold registration.
    // Bob and settle are built-in curves, not spare indices in that table.
    // Missing authored keys keep the original no-motion behavior.
    // Malformed arrays can never extend a frame's sampling loop.
    // The shortest available component uses the same zero default as JS.
    fn count(key: usize, keys: &[Keys]) -> usize {
        match key {
            BOB => 4,
            SETTLE => 1,
            _ => keys
                .get(key)
                .map_or(0, |k| k.time.len().min(limit::MOTION_SEGMENTS)),
        }
    }
    fn duration(key: usize, i: usize, keys: &[Keys]) -> f64 {
        match key {
            BOB => 0.2,
            SETTLE => 0.3,
            _ => keys
                .get(key)
                .and_then(|k| k.time.get(i))
                .copied()
                .unwrap_or(0.3),
        }
        .max(1e-4)
    }
    fn component(&self, keys: &[Keys], c: usize, i: usize, slope: bool) -> f64 {
        if self.key == SETTLE {
            return 0.0;
        }
        if self.key == BOB {
            return if c < 3 {
                if slope {
                    BOB_SLOPE[i.min(3)][c] * self.scale
                } else {
                    BOB_VEC[i.min(3)][c] * self.scale
                }
            } else {
                0.0
            };
        }
        let Some(k) = keys.get(self.key) else {
            return 0.0;
        };
        let rows = match (c < 3, slope) {
            (true, false) => &k.position,
            (true, true) => &k.slope,
            (false, false) => &k.turn,
            (false, true) => &k.turn_slope,
        };
        rows.get(i).map_or(0.0, |v| v[c % 3]) * if c < 3 { self.scale } else { 1.0 }
    }
    // FirstPersonMotion.play begins at the current offset and velocity.
    // Interrupting a roll with a stance change must not snap back to zero.
    // The new duration scales the retained derivative at the join.
    // Both position and Euler-turn curves use the same six scalar lanes.
    // Keeping those lanes inline avoids a temporary keyframe object.
    fn play(&mut self, key: usize, scale: f64, keys: &[Keys]) {
        if Self::count(key, keys) == 0 {
            return;
        }
        self.key = key;
        self.scale = scale;
        self.bobbing = key == BOB;
        self.segment = 0;
        self.left = Self::duration(key, 0, keys);
        self.start = self.value;
        for c in 0..6 {
            self.start_slope[c] = self.slope[c] * self.left;
        }
    }
    pub fn reset(&mut self) {
        self.key = NONE;
        self.bobbing = false;
        self.bob_wanted = false;
        self.value.fill(0.0);
        self.slope.fill(0.0);
    }
    fn trigger(&mut self, key: usize, scale: f64, keys: &[Keys]) {
        if Self::count(key, keys) == 0 {
            return;
        }
        self.play(key, scale, keys);
        // RollLeft, RollRight, CrouchToProne and ProneToStand lower the arms.
        if matches!(key, 0 | 1 | 3 | 4) {
            self.cue = keys[key].time.iter().sum();
        }
    }
    // Ports FirstPersonMotion.update in camera.js, before camera selection.
    // Remember every posture, air and dive transition, even in third person.
    // Only the player's own eyes can trigger a new visible motion.
    // Landing strength reads the remembered downward velocity of the body.
    // Short stair drops keep the smaller impact response of the page.
    // An interrupted bob is allowed to settle at its existing derivative.
    // The cue is a scalar duration for the arms' hands-down/landing clips.
    // The host consumes it in the same frame before attachment sampling.
    // CameraMotion.update step across posture, gait, velocity, and authored keys.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        dt: f64,
        posture: usize,
        air: usize,
        dive: f64,
        velocity: Vector3,
        active: bool,
        keys: &[Keys],
    ) {
        self.cue = 0.0;
        let was = self.was;
        self.was = Some((posture, air, dive));
        if air != 0 {
            self.air_time += dt;
            self.impact = self.impact.max(-velocity.y);
        }
        if !active {
            self.reset();
            return;
        }
        if let Some((p, a, d)) = was {
            if dive != 0.0 && d == 0.0 {
                self.trigger(
                    if dive < 0.0 {
                        id::ROLL_LEFT as usize
                    } else {
                        id::ROLL_RIGHT as usize
                    },
                    1.0,
                    keys,
                )
            } else if a != 0 && air == 0 && (a == 1 || self.impact >= 3.0 || self.air_time > 0.35) {
                self.trigger(
                    id::LAND as usize,
                    clamp(self.impact * 0.04, 0.12, 0.35),
                    keys,
                )
            } else if dive == 0.0 && d == 0.0 && air == 0 && a == 0 && p != posture {
                let k = match (p, posture) {
                    (0, 2) => id::STAND_TO_PRONE as usize,
                    (1, 2) => id::CROUCH_TO_PRONE as usize,
                    (2, 0) => id::PRONE_TO_STAND as usize,
                    (1, 0) => id::CROUCH_TO_STAND as usize,
                    _ => NONE,
                };
                self.trigger(k, 1.0, keys);
            }
        }
        if air == 0 {
            self.air_time = 0.0;
            self.impact = 0.0
        }
        let v2 = velocity.x.powi(2) + velocity.z.powi(2);
        self.bob_wanted = air == 0 && dive == 0.0 && v2 >= if self.bobbing { 0.1 } else { 1.0 };
        self.bob_scale = if posture == 2 { 0.5 } else { 1.0 };
        if self.bobbing && !self.bob_wanted {
            self.play(SETTLE, 1.0, keys)
        }
        if self.key == NONE && self.bob_wanted {
            self.play(BOB, self.bob_scale, keys)
        }
        self.step(dt, keys);
    }
    // FirstPersonMotion.step carries time across segment boundaries.
    // A frame can finish a named motion and enter bob or settle immediately.
    // Segment durations have a positive floor, preventing zero-time loops.
    // The iteration limit also bounds an arbitrarily long paused-load dt.
    // Hitting it records a skip instead of growing storage or panicking.
    // Ordinary content finishes long before that limit at every frame rate.
    // Cubic Hermite values and slopes use the original expression order.
    // Slopes are converted back to seconds before the following segment.
    // That conversion is required for continuous joins of unequal lengths.
    fn step(&mut self, dt: f64, keys: &[Keys]) {
        let mut remaining = dt.max(0.0);
        // A broken host clock cannot make a frame loop forever. This permits
        // over thirteen minutes of bob segments before dropping the remainder.
        for _ in 0..limit::MOTION_STEPS {
            if self.key == NONE {
                return;
            }
            let i = self.segment;
            let duration = Self::duration(self.key, i, keys);
            let consumed = remaining.min(self.left);
            self.left -= consumed;
            remaining -= consumed;
            let u = clamp(1.0 - self.left / duration, 0.0, 1.0);
            for c in 0..6 {
                let p0 = if i > 0 {
                    self.component(keys, c, i - 1, false)
                } else {
                    self.start[c]
                };
                let t0 = if i > 0 {
                    self.component(keys, c, i - 1, true) * duration
                        / Self::duration(self.key, i - 1, keys)
                } else {
                    self.start_slope[c]
                };
                let p1 = self.component(keys, c, i, false);
                let t1 = self.component(keys, c, i, true);
                let a = 2.0 * (p0 - p1) + t0 + t1;
                let b = 3.0 * (p1 - p0) - 2.0 * t0 - t1;
                self.value[c] = ((a * u + b) * u + t0) * u + p0;
                self.slope[c] = ((3.0 * a * u + 2.0 * b) * u + t0) / duration;
            }
            if self.left > 1e-10 {
                return;
            }
            if i < Self::count(self.key, keys) - 1 {
                self.segment += 1;
                self.left = Self::duration(self.key, self.segment, keys)
            } else {
                self.key = NONE;
                self.bobbing = false;
                if self.value.iter().any(|v| v.abs() > 1e-4)
                    || self.slope.iter().any(|v| v.abs() > 1e-4)
                {
                    self.play(SETTLE, 1.0, keys)
                } else if self.bob_wanted {
                    self.play(BOB, self.bob_scale, keys)
                }
            }
            if remaining <= 1e-10 {
                return;
            }
        }
        self.reset();
        self.skipped += 1;
    }
    // FirstPersonMotion.offset is measured in the unturned eye's local frame.
    // SoldierCamera applies that translation before the motion's Euler turn.
    // Reversing the two would make a roll rotate its own displacement.
    // The offset is copied as a stack vector, with no published JS curve.
    // Arms receive only the cue duration, not a duplicate motion integrator.
    pub fn offset(&self) -> Vector3 {
        Vector3::new(self.value[0], self.value[1], self.value[2])
    }
    // FirstPersonMotion.turn used ZYX Euler order for the authored Ore keys.
    // Soldier aim remains YXZ, so these two rotations cannot be merged.
    // The product is applied after the eye offset in Soldier::frame.
    // A zero-valued curve still keeps its original quaternion zero signs.
    pub fn turn<M: Math>(&self) -> Quaternion {
        euler::<M>(self.value[3], self.value[4], self.value[5], true)
    }
}
