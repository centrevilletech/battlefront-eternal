//! Synthetic droideka rolling dust trail tests ported from
//! `tests/droideka-trail.test.js`.
//!
//! These cases exercise `bfe_presentation::unit_effects::trail::Trail`
//! across outdoor paved vs. terrain opacity, cached floor/roof probes,
//! suppression conditions, indoor decay, speed/turning emission rates,
//! fixed-buffer bounds, and teleport clearing.
//!
//! Coverage includes:
//! - Cached 80ms / 0.5m floor-and-roof ray queries (`probe` and `roof`),
//!   with lighter opacity on paved surfaces (`0.42`) than terrain (`0.70`).
//! - Complete emission suppression under roof, missing ground, water, air,
//!   death, stopped velocity (`<= 2.2 m/s`), or unfolded posture.
//! - Natural particle decay after rolling indoors, and immediate clearing
//!   when switching to Classic mode or teleporting (`distance^2 > 100`).
//! - Speed and cross-product turning rate scaling within the 160-record pool.

use bfe_presentation::unit_effects::trail::Trail;
use bfe_presentation::visual_effects_layout::{trail_frame as f, trail_particle as p};

/// Synthetic environment and soldier state matching `fixture()` in
/// `tests/droideka-trail.test.js`.
struct Fixture {
    trail: Trail,
    roof: bool,
    floor: bool,
    terrain: bool,
    enhanced: bool,
    water: bool,
    casts: usize,
    rolling: bool,
    dead: bool,
    air: bool,
    grounded: bool,
    position: [f64; 3],
    velocity: [f64; 3],
}

impl Fixture {
    /// Creates a rolling droideka moving at `10 m/s` along `+X` over dry,
    /// outdoor, paved ground in Enhanced mode.
    fn new() -> Self {
        Self {
            trail: Trail::new("tat"),
            roof: false,
            floor: true,
            terrain: false,
            enhanced: true,
            water: false,
            casts: 0,
            rolling: true,
            dead: false,
            air: false,
            grounded: true,
            position: [0.0, 0.0, 0.0],
            velocity: [10.0, 0.0, 0.0],
        }
    }

    /// Advances the soldier position and steps `Trail` by `dt` seconds,
    /// answering floor and roof ray requests when `probe()` asks for them.
    fn step(&mut self, dt: f64, advance_pos: bool) {
        if advance_pos {
            self.position[0] += self.velocity[0] * dt;
            self.position[1] += self.velocity[1] * dt;
            self.position[2] += self.velocity[2] * dt;
        }
        // Populate the flat input frame buffer before running the probe pass.
        self.trail.input[f::DT] = dt;
        self.trail.input[f::ENHANCED] = if self.enhanced { 1.0 } else { 0.0 };
        self.trail.input[f::POSITION..f::POSITION + 3].copy_from_slice(&self.position);
        self.trail.input[f::VELOCITY..f::VELOCITY + 3].copy_from_slice(&self.velocity);
        self.trail.input[f::ROLLING] = if self.rolling { 1.0 } else { 0.0 };
        self.trail.input[f::DEAD] = if self.dead { 1.0 } else { 0.0 };
        self.trail.input[f::AIR] = if self.air { 1.0 } else { 0.0 };
        self.trail.input[f::GROUNDED] = if self.grounded { 1.0 } else { 0.0 };
        self.trail.input[f::WET] = if self.water { 1.0 } else { 0.0 };
        self.trail.input[f::WATER_LEVEL] = 0.0;

        // First query stage: check whether a new floor cast is needed.
        self.trail.probe();
        if self.trail.input[f::PROBE] != 0.0 {
            self.casts += 1;
            if self.floor {
                self.trail.input[f::FLOOR_HIT] = 1.0;
                self.trail.input[f::FLOOR_NORMAL] = 1.0;
                self.trail.input[f::FLOOR] = 0.0;
                self.trail.input[f::TERRAIN] = if self.terrain { 1.0 } else { 0.0 };
            } else {
                self.trail.input[f::FLOOR_HIT] = 0.0;
                self.trail.input[f::FLOOR_NORMAL] = 0.0;
            }
            // Second query stage: only cast upward for a roof if the floor hit.
            self.trail.roof();
            if self.trail.input[f::ROOF_PROBE] != 0.0 {
                self.casts += 1;
                self.trail.input[f::ROOF] = if self.roof { 1.0 } else { 0.0 };
            }
        }
        // Advance particle lifetimes and emit new dust records using a fixed 0.5 sample.
        self.trail.frame(&mut || 0.5);
    }

    /// Runs `seconds` of 60 Hz frames.
    fn run(&mut self, seconds: f64) {
        let steps = (seconds * 60.0).round() as usize;
        for _ in 0..steps {
            self.step(1.0 / 60.0, true);
        }
    }
}

// Mirrors `tests/droideka-trail.test.js`:
// "outdoor paved courtyards emit a lighter trail than terrain, with cached probes"
#[test]
fn outdoor_paved_courtyards_emit_lighter_trail_than_terrain_with_cached_probes() {
    let mut paved = Fixture::new();
    let mut terrain = Fixture::new();
    terrain.terrain = true;

    paved.run(1.0);
    terrain.run(1.0);

    // Both surfaces emit the same particle count with cached ray probes (< 60 casts).
    assert!(paved.trail.input[f::COUNT] > 40.0);
    assert!(
        paved.casts < 60,
        "expected cached rays, saw {}",
        paved.casts
    );
    assert_eq!(paved.trail.input[f::COUNT], terrain.trail.input[f::COUNT]);
    // Opacity (offset 10 in each 12-float record) is lighter on paved ground than terrain.
    assert!(paved.trail.records[p::OPACITY] < terrain.trail.records[p::OPACITY]);
}

// Mirrors `tests/droideka-trail.test.js`:
// "roof, missing ground, water, air, death, stopped and unfolded prevent emission"
#[test]
fn roof_missing_ground_water_air_death_stopped_and_unfolded_prevent_emission() {
    // Each disqualifying state must prevent any dust particle from spawning.
    for condition in [
        "roof", "floor", "water", "air", "dead", "stopped", "unfolded",
    ] {
        let mut f = Fixture::new();
        match condition {
            "roof" => f.roof = true,
            "floor" => f.floor = false,
            "water" => f.water = true,
            "air" => f.air = true,
            "dead" => f.dead = true,
            "stopped" => f.velocity = [0.0, 0.0, 0.0],
            "unfolded" => f.rolling = false,
            _ => unreachable!(),
        }
        f.run(1.0);
        assert_eq!(f.trail.input[f::EMITTED], 0.0, "{condition}");
    }
}

// Mirrors `tests/droideka-trail.test.js`:
// "outdoor particles decay after entering a building; Classic clears immediately"
#[test]
fn outdoor_particles_decay_after_entering_building_and_classic_clears_immediately() {
    let mut f = Fixture::new();
    f.run(1.0);
    let emitted = f.trail.input[f::EMITTED];

    // Enter a roofed building: emission stops while existing particles decay.
    f.roof = true;
    f.position[0] += 1.0;
    f.run(0.5);
    assert_eq!(f.trail.input[f::EMITTED], emitted);
    assert!(f.trail.input[f::COUNT] > 0.0);

    // After 2 seconds indoors all previously emitted dust has expired.
    f.run(2.0);
    assert_eq!(f.trail.input[f::COUNT], 0.0);

    // Leaving cover resumes emission; switching to Classic clears immediately.
    // Re-enabling Enhanced mode allows new dust particles to spawn again.
    f.roof = false;
    f.run(1.0);
    assert!(f.trail.input[f::COUNT] > 0.0);
    f.enhanced = false;
    f.run(1.0 / 60.0);
    assert_eq!(f.trail.input[f::COUNT], 0.0);
    f.enhanced = true;
    f.run(0.5);
    assert!(f.trail.input[f::COUNT] > 0.0);
}

// Mirrors `tests/droideka-trail.test.js`:
// "speed and turning strengthen the trail while its pool and buffers remain bounded"
#[test]
fn speed_and_turning_strengthen_trail_while_pool_and_buffers_remain_bounded() {
    let mut slow = Fixture::new();
    let mut fast = Fixture::new();
    let mut turning = Fixture::new();

    // Compare slow (3 m/s), fast straight (10 m/s), and fast turning (10 m/s).
    slow.velocity = [3.0, 0.0, 0.0];
    slow.run(1.0);
    fast.run(1.0);
    for i in 0..60 {
        let angle = i as f64 / 10.0;
        turning.velocity = [angle.cos() * 10.0, 0.0, angle.sin() * 10.0];
        turning.run(1.0 / 60.0);
    }

    assert!(fast.trail.input[f::EMITTED] > slow.trail.input[f::EMITTED]);
    assert!(turning.trail.input[f::EMITTED] > fast.trail.input[f::EMITTED]);

    // Sustained emission never reallocates or exceeds the 160-particle cap.
    let ptr = fast.trail.records.as_ptr();
    fast.run(15.0);
    assert!(fast.trail.input[f::COUNT] <= 160.0);
    assert_eq!(fast.trail.records.as_ptr(), ptr);
    assert!(fast.trail.records.iter().all(|n| n.is_finite()));
}

// Mirrors `tests/droideka-trail.test.js`:
// "teleporting clears the old wake and zero-time draws cannot emit"
#[test]
fn teleporting_clears_old_wake_and_zero_time_draws_cannot_emit() {
    let mut f = Fixture::new();
    f.run(1.0);
    let emitted = f.trail.input[f::EMITTED];

    // Zero dt draw must not emit additional dust particles.
    f.step(0.0, false);
    assert_eq!(f.trail.input[f::EMITTED], emitted);

    // Jumping 100 m in one frame clears the active dust pool.
    // Any step exceeding 10 m squared distance triggers the teleport reset.
    f.position[0] += 100.0;
    f.velocity = [0.0, 0.0, 0.0];
    f.step(1.0 / 60.0, false);
    assert_eq!(f.trail.input[f::COUNT], 0.0);
}
