//! Synthetic jetpack exhaust volume and detached wake tests ported from
//! `tests/jetpack-effects.test.js` and `tests/jetpack-wake.test.js`.
//!
//! These cases exercise `bfe_presentation::unit_effects::jetpack::{Definition,
//! Jetpack, Wake}` across ribcage/authored/mesh nozzle outlets, ignition and
//! cutoff envelopes, frame-rate independent wake interpolation, pool bounds,
//! local light requests, tail bend clamping, and teleport clearing.
//!
//! Coverage from `tests/jetpack-effects.test.js`:
//! - Paired ribcage default nozzle offsets (`[-0.24, -0.21, +/-0.15]`) and
//!   `-X` local exhaust axis rotated by the soldier's quaternion.
//! - Authored `JetExhaust` spawner offset/box midpoints and live mesh outlet
//!   frame overrides (`HAS_PREVIOUS != 0`) with per-nozzle radii.
//! - Smooth throttle wind-down after ignition stops, lingering smoke/sparks,
//!   immediate clearing in Classic mode or on a `> 10m` teleport, and bounded
//!   `96 / 128 / 64` wake pools over long 15s burns.
//! - Active-only local point light requests, frame-rate invariant throttle
//!   envelopes across 30/60/144 FPS, and perpendicular tail bend clamping.
//!
//! Coverage from `tests/jetpack-wake.test.js`:
//! - Sub-frame path interpolation (`fraction = (birth + 1 - phase) / (dt * rate)`)
//!   so moving nozzles fill the path between frames rather than clumping.
//! - Identical birth timestamps and world positions across 30, 60, and 144 FPS.
//! - Sparse ember ratios (`wisps > embers * 3`), low-opacity smoke (`<= 0.12`),
//!   and complete post-cutoff decay.

use bfe_presentation::unit_effects::jetpack::{Definition, Jetpack, Wake};
use bfe_presentation::visual_effects_layout::{
    jet_frame as j, jet_nozzle as n, volume_frame as vf, volume_nozzle as vn, wake_frame as w,
    wake_particle as wp,
};
use three::{Quaternion, Vector3};

/// Builds a baseline jetpack fixture matching `fixture()` in
/// `tests/jetpack-effects.test.js` (body at `(2, 5, 3)`, rotated `+90 deg`
/// around `Z`, velocity `(3, 1, 0)`).
struct JetFixture {
    jet: Jetpack,
    active: bool,
    enhanced: bool,
    position: Vector3,
    quaternion: Quaternion,
    velocity: Vector3,
}

impl JetFixture {
    /// Initializes an active enhanced jetpack with a +90-degree Z body rotation.
    fn new() -> Self {
        let mut q = Quaternion::default();
        q.set_from_axis_angle(&Vector3::new(0.0, 0.0, 1.0), std::f64::consts::FRAC_PI_2);
        Self {
            jet: Jetpack::new(Vec::new()),
            active: true,
            enhanced: true,
            position: Vector3::new(2.0, 5.0, 3.0),
            quaternion: q,
            velocity: Vector3::new(3.0, 1.0, 0.0),
        }
    }

    /// Copies the current fixture state into `jet.input` and advances one frame.
    fn step(&mut self, dt: f64) {
        // Populate body transform, velocity, and mode flags in the input buffer.
        self.jet.input[j::DT] = dt;
        self.jet.input[j::ACTIVE] = if self.active { 1.0 } else { 0.0 };
        self.jet.input[j::ENHANCED] = if self.enhanced { 1.0 } else { 0.0 };
        self.jet.input[j::POSITION] = self.position.x;
        self.jet.input[j::POSITION + 1] = self.position.y;
        self.jet.input[j::POSITION + 2] = self.position.z;
        self.jet.input[j::QUATERNION] = self.quaternion.x;
        self.jet.input[j::QUATERNION + 1] = self.quaternion.y;
        self.jet.input[j::QUATERNION + 2] = self.quaternion.z;
        self.jet.input[j::QUATERNION + 3] = self.quaternion.w;
        self.jet.input[j::VELOCITY] = self.velocity.x;
        self.jet.input[j::VELOCITY + 1] = self.velocity.y;
        self.jet.input[j::VELOCITY + 2] = self.velocity.z;
        // Advance nozzle geometry, exhaust volume, and detached wake particles.
        self.jet.frame(&mut || 0.5);
    }

    /// Runs `seconds` of 60 Hz frames.
    fn run(&mut self, seconds: f64) {
        let steps = (seconds * 60.0).round() as usize;
        for _ in 0..steps {
            self.step(1.0 / 60.0);
        }
    }
}

/// Builds a standalone wake fixture matching `fixture()` in
/// `tests/jetpack-wake.test.js` (single downward nozzle moving at `6 m/s`
/// along `+X`).
struct WakeFixture {
    wake: Wake,
    nozzle_x: f64,
    active: bool,
}

impl WakeFixture {
    /// Creates a single-nozzle wake at full throttle pointing along `-Y`.
    fn new() -> Self {
        let mut wake = Wake::default();
        wake.input[w::THROTTLE] = 1.0;
        wake.input[w::NOZZLES] = 1.0;
        wake.input[w::VELOCITY] = 6.0;
        wake.nozzles[n::DIRECTION + 1] = -1.0;
        wake.nozzles[n::RADIUS] = 0.034;
        Self {
            wake,
            nozzle_x: 0.0,
            active: true,
        }
    }

    /// Advances the standalone wake by `dt` seconds at the current nozzle X.
    fn step(&mut self, dt: f64) {
        // Write frame delta, active flag, and current nozzle X coordinate.
        self.wake.input[w::DT] = dt;
        self.wake.input[w::ACTIVE] = if self.active { 1.0 } else { 0.0 };
        self.wake.nozzles[n::POSITION] = self.nozzle_x;
        self.wake.frame(&mut || 0.5);
    }

    /// Moves the nozzle at `6 m/s` for `seconds` at `rate` FPS.
    fn run(&mut self, seconds: f64, rate: f64) {
        let steps = (seconds * rate).round() as usize;
        for _ in 0..steps {
            self.nozzle_x += 6.0 / rate;
            self.step(1.0 / rate);
        }
    }
}

// Mirrors `tests/jetpack-effects.test.js`:
// "enhanced jets follow the original ribcage nozzle offsets and exhaust axis"
// "authored nozzle positions override defaults without assuming world-down"
// "mesh outlet frames override the legacy emitters and set the visible exhaust width"
#[test]
fn nozzles_follow_ribcage_defaults_authored_definitions_and_mesh_outlet_frames() {
    // Default ribcage outlets rotated by +90 deg Z point straight down (0, -1, 0)
    // at world positions (2.21, 4.76, 3.15) and (2.21, 4.76, 2.85).
    let mut f = JetFixture::new();
    f.step(1.0 / 60.0);
    assert_eq!(f.jet.volume.input[vf::COUNT], 2.0);
    for i in 0..2 {
        // Read each nozzle's world direction and position from the flat nozzle buffer.
        let at = i * n::STRIDE;
        let dir = Vector3::new(
            f.jet.nozzles[at + n::DIRECTION],
            f.jet.nozzles[at + n::DIRECTION + 1],
            f.jet.nozzles[at + n::DIRECTION + 2],
        );
        let pos = Vector3::new(
            f.jet.nozzles[at + n::POSITION],
            f.jet.nozzles[at + n::POSITION + 1],
            f.jet.nozzles[at + n::POSITION + 2],
        );
        assert!(dir.distance_to(&Vector3::new(0.0, -1.0, 0.0)) < 1e-6);
        let expected_z = if i != 0 { 2.85 } else { 3.15 };
        assert!(pos.distance_to(&Vector3::new(2.21, 4.76, expected_z)) < 1e-6);
    }

    // Authored nozzle definitions override defaults without assuming world-down.
    // Offset midpoint (1, 2, 3) + body (4, 5, 6) = (5, 7, 9); box midpoint = (0, 0, -1).
    let defs: Vec<Definition> = serde_json::from_str(
        r#"[{"name": "JetExhaust", "spawner": {"offset": [[1, 1], [2, 2], [3, 3]], "box": [[0, 0], [0, 0], [-1, -1]]}}]"#,
    )
    .unwrap();
    let mut authored = Jetpack::new(defs);
    authored.input[j::ACTIVE] = 1.0;
    authored.input[j::ENHANCED] = 1.0;
    authored.input[j::POSITION..j::POSITION + 3].copy_from_slice(&[4.0, 5.0, 6.0]);
    authored.input[j::QUATERNION + 3] = 1.0;
    authored.frame(&mut || 0.5);
    assert_eq!(authored.input[j::NOZZLES], 1.0);
    assert_eq!(
        &authored.nozzles[n::POSITION..n::POSITION + 3],
        &[5.0, 7.0, 9.0]
    );
    assert_eq!(
        &authored.nozzles[n::DIRECTION..n::DIRECTION + 3],
        &[0.0, 0.0, -1.0]
    );

    // Mesh outlet frames (HAS_PREVIOUS != 0) override legacy emitters and set radius.
    // Populate two explicit mesh nozzle frames with distinct radii (0.034 and 0.032).
    let frames = [
        ([1.0, 2.0, 3.0], [0.0, -1.0, 0.0], 0.034),
        ([4.0, 5.0, 6.0], [1.0, 0.0, 0.0], 0.032),
    ];
    for (i, (pos, dir, radius)) in frames.iter().enumerate() {
        let at = i * n::STRIDE;
        f.jet.nozzles[at + n::POSITION..at + n::POSITION + 3].copy_from_slice(pos);
        f.jet.nozzles[at + n::DIRECTION..at + n::DIRECTION + 3].copy_from_slice(dir);
        f.jet.nozzles[at + n::RADIUS] = *radius;
        f.jet.nozzles[at + n::HAS_PREVIOUS] = 1.0;
    }
    // Verify that `frame` preserves the mesh outlets and forwards their radii to `volume`.
    f.step(1.0 / 60.0);
    for (i, (pos, dir, radius)) in frames.iter().enumerate() {
        let at = i * n::STRIDE;
        assert_eq!(&f.jet.nozzles[at + n::POSITION..at + n::POSITION + 3], pos);
        assert_eq!(
            &f.jet.nozzles[at + n::DIRECTION..at + n::DIRECTION + 3],
            dir
        );
        assert!((f.jet.volume.arrays[i * vn::STRIDE + vn::RADIUS] as f64 - radius).abs() < 1e-8);
    }
}

// Mirrors `tests/jetpack-effects.test.js`:
// "sparks and smoke persist briefly after ignition stops; Classic clears enhancements"
// "long burns keep fixed buffers bounded and zero-time frames do not age particles"
// "teleporting cannot leave a smoke trail connected across the map"
#[test]
fn smoke_and_sparks_persist_after_cutoff_stay_bounded_and_clear_on_classic_or_teleport() {
    // Burn for 0.8s so both smoke (kind 1) and ember sparks (kind 2) are populated.
    let mut f = JetFixture::new();
    f.run(0.8);
    assert!(f.jet.wake.input[w::COUNTS + 1] > 0.0);
    assert!(f.jet.wake.input[w::COUNTS + 2] > 0.0);
    let emitted = f.jet.wake.input[w::EMITTED];

    // Cutoff winds down throttle smoothly while existing smoke lingers then decays.
    f.active = false;
    f.run(0.1);
    assert!(f.jet.input[j::THROTTLE] > 0.0 && f.jet.input[j::THROTTLE] < 0.3);
    assert_eq!(f.jet.wake.input[w::EMITTED], emitted);
    assert!(f.jet.wake.input[w::COUNTS + 1] > 0.0);

    // After 2 seconds all lingering smoke and ember sparks have expired.
    f.run(2.0);
    assert_eq!(f.jet.wake.input[w::COUNTS + 1], 0.0);
    assert_eq!(f.jet.wake.input[w::COUNTS + 2], 0.0);

    // Classic mode clears enhanced volume and wake particles immediately.
    f.active = true;
    f.run(0.5);
    f.enhanced = false;
    f.run(1.0 / 60.0);
    assert_eq!(f.jet.volume.input[vf::SHOWN], 0.0);
    assert_eq!(f.jet.wake.input[w::COUNTS + 1], 0.0);
    assert_eq!(f.jet.wake.input[w::COUNTS + 2], 0.0);

    // Long burn keeps fixed buffers bounded and zero-time frame does not age or emit.
    // Check that the backing Vec<f32> pointer never reallocates over 900 frames.
    let mut long = JetFixture::new();
    let ptr = long.jet.wake.particles.as_ptr();
    long.run(15.0);
    assert!(long.jet.wake.input[w::COUNTS + 1] <= 128.0);
    assert!(long.jet.wake.input[w::COUNTS + 2] <= 64.0);
    assert_eq!(long.jet.wake.particles.as_ptr(), ptr);
    let before = long.jet.wake.particles.clone();
    let emitted_long = long.jet.wake.input[w::EMITTED];
    long.step(0.0);
    assert_eq!(long.jet.wake.particles, before);
    assert_eq!(long.jet.wake.input[w::EMITTED], emitted_long);

    // Teleporting clears smoke and sparks rather than bridging across the map.
    long.position.x += 100.0;
    long.active = false;
    long.step(1.0 / 60.0);
    assert_eq!(long.jet.wake.input[w::COUNTS + 1], 0.0);
    assert_eq!(long.jet.wake.input[w::COUNTS + 2], 0.0);
}

// Mirrors `tests/jetpack-effects.test.js`:
// "enhanced exhaust requests one bounded local light only while burning"
// "ignition and cutoff envelopes are stable across render rates"
// "turning bends the combustion tail while the nozzle remains attached; teleport clears history"
#[test]
fn local_light_render_rate_envelopes_and_tail_bend_match_expected_bounds() {
    // Active enhanced burn emits one local light near the mean nozzle outlet.
    let mut f = JetFixture::new();
    f.position = Vector3::new(0.0, 3.0, 0.0);
    f.quaternion = Quaternion::default();
    f.velocity = Vector3::default();
    f.step(1.0 / 60.0);
    assert!(f.jet.input[j::LIGHT_STRENGTH] > 0.0);
    let light_pos = Vector3::new(
        f.jet.input[j::LIGHT_POSITION],
        f.jet.input[j::LIGHT_POSITION + 1],
        f.jet.input[j::LIGHT_POSITION + 2],
    );
    assert!(light_pos.distance_to(&f.position) < 1.0);

    // Inactive or Classic mode suppresses the local light request.
    f.active = false;
    f.step(1.0 / 60.0);
    assert_eq!(f.jet.input[j::LIGHT_STRENGTH], 0.0);
    f.enhanced = false;
    f.active = true;
    f.step(1.0 / 60.0);
    assert_eq!(f.jet.input[j::LIGHT_STRENGTH], 0.0);

    // Ignition and cutoff throttle envelopes are identical at 30, 60 and 144 FPS.
    // Each trace samples throttle after 0.12s ignition, +0.30s full burn, 0.10s cutoff, +0.30s off.
    let traces: Vec<Vec<f64>> = [30.0f64, 60.0, 144.0]
        .iter()
        .map(|&rate| {
            let mut rf = JetFixture::new();
            let mut trace = Vec::new();
            for (active, duration) in [(true, 0.12f64), (true, 0.30), (false, 0.10), (false, 0.30)]
            {
                rf.active = active;
                let mut left: f64 = duration;
                while left > 1e-8 {
                    let dt = left.min(1.0 / rate);
                    rf.step(dt);
                    left -= dt;
                }
                trace.push(rf.jet.input[j::THROTTLE]);
            }
            // Verify intermediate ignition/cutoff ramps and full rest at zero.
            assert!(trace[0] > 0.85 && trace[0] < 1.0);
            assert!(trace[2] > 0.0 && trace[2] < 0.3);
            assert_eq!(trace[3], 0.0);
            trace
        })
        .collect();
    // All higher-rate traces must match the 30 FPS baseline within 1e-8.
    for trace in &traces[1..] {
        for (a, b) in trace.iter().zip(&traces[0]) {
            assert!((a - b).abs() < 1e-8);
        }
    }

    // Moving sideways bends the tail perpendicular to the exhaust direction; teleport resets it.
    let mut turn = JetFixture::new();
    turn.run(0.3);
    turn.position.x += 0.08;
    turn.step(1.0 / 60.0);
    let bend = Vector3::new(
        turn.jet.nozzles[n::BEND],
        turn.jet.nozzles[n::BEND + 1],
        turn.jet.nozzles[n::BEND + 2],
    );
    let dir = Vector3::new(
        turn.jet.nozzles[n::DIRECTION],
        turn.jet.nozzles[n::DIRECTION + 1],
        turn.jet.nozzles[n::DIRECTION + 2],
    );
    // Tail bend is clamped to MAX_TAIL_BEND (0.14) and orthogonal to exhaust axis.
    assert!(bend.length() > 0.01 && bend.length() <= 0.141);
    assert!(bend.dot(&dir).abs() < 1e-8);
    turn.position.x += 100.0;
    turn.step(1.0 / 60.0);
    let reset_bend = Vector3::new(
        turn.jet.nozzles[n::BEND],
        turn.jet.nozzles[n::BEND + 1],
        turn.jet.nozzles[n::BEND + 2],
    );
    assert!(reset_bend.length() < 1e-8);
}

// Mirrors `tests/jetpack-wake.test.js`:
// "moving nozzle emissions fill the actual between-frame path rather than clumping at the endpoint"
// "emission timestamps and positions are stable at 30, 60 and 144 FPS"
// "wake stops emitting after cutoff, decays completely, and has sparse embers and low-opacity smoke"
// "teleport clears the previous wake and never interpolates across the map"
#[test]
fn wake_interpolates_between_frame_paths_across_frame_rates_and_decays_after_cutoff() {
    // Over a 0.2s step from X = 0 to X = 1.2, 7 wisps spawn evenly at (i+1)/6 m
    // and (i+1)/36 s rather than clumping at X = 1.2.
    let mut f = WakeFixture::new();
    f.step(0.0);
    f.nozzle_x = 1.2;
    f.step(0.2);
    assert_eq!(f.wake.input[w::COUNTS], 7.0);
    for i in 0..7 {
        let at = i * wp::STRIDE;
        let x = f.wake.particles[at + wp::POSITION] as f64;
        let t = f.wake.particles[at + wp::TIME] as f64;
        assert!((x - (i + 1) as f64 / 6.0).abs() < 1e-6);
        assert!((t - (i + 1) as f64 / 36.0).abs() < 1e-6);
    }

    // Emission timestamps and positions match across 30, 60 and 144 FPS.
    // Sort each particle pool by birth time so ring-buffer wraparound order is normalized.
    let results: Vec<Vec<Vec<(f32, f32)>>> = [30.0, 60.0, 144.0]
        .iter()
        .map(|&rate| {
            let mut wf = WakeFixture::new();
            wf.step(0.0);
            wf.run(1.0, rate);
            [0usize, 1, 2]
                .iter()
                .map(|&kind| {
                    // Pool base indices: wisps = 0..96, smoke = 96..224, embers = 224..288.
                    let base = if kind == 0 {
                        0
                    } else if kind == 1 {
                        96
                    } else {
                        224
                    };
                    let count = wf.wake.input[w::COUNTS + kind] as usize;
                    let mut values: Vec<(f32, f32)> = (0..count)
                        .map(|i| {
                            let at = (base + i) * wp::STRIDE;
                            (
                                wf.wake.particles[at + wp::TIME],
                                wf.wake.particles[at + wp::POSITION],
                            )
                        })
                        .collect();
                    values.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                    values
                })
                .collect()
        })
        .collect();
    // Compare 60 FPS and 144 FPS sorted (time, x) pairs against 30 FPS.
    for result in &results[1..] {
        for kind in 0..3 {
            assert_eq!(result[kind].len(), results[0][kind].len());
            for (a, b) in result[kind].iter().zip(&results[0][kind]) {
                assert!((a.0 - b.0).abs() < 1e-6);
                assert!((a.1 - b.1).abs() < 1e-5);
            }
        }
    }

    // Sparse embers, low-opacity smoke, cutoff decay, and teleport clearing.
    // Verify wisps outnumber embers by > 3x and smoke opacity never exceeds 0.12.
    let mut burn = WakeFixture::new();
    burn.run(1.0, 60.0);
    assert!(burn.wake.input[w::COUNTS] > burn.wake.input[w::COUNTS + 2] * 3.0);
    assert!(burn.wake.particles[96 * wp::STRIDE + wp::OPACITY] <= 0.121);
    let emitted = burn.wake.input[w::EMITTED];
    // Turning the wake inactive halts new emissions while existing smoke decays.
    burn.active = false;
    burn.run(0.1, 60.0);
    assert_eq!(burn.wake.input[w::EMITTED], emitted);
    assert!(burn.wake.input[w::COUNTS + 1] > 0.0);
    burn.run(1.5, 60.0);
    assert_eq!(&burn.wake.input[w::COUNTS..w::COUNTS + 3], &[0.0, 0.0, 0.0]);
}
