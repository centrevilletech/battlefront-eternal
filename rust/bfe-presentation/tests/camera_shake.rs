//! Synthetic camera shake and soldier chase camera tests ported from
//! `tests/camera-shake.test.js` and `tests/camera.test.js`.
//!
//! These cases exercise `bfe_presentation::cameras::Phase` and
//! `bfe_presentation::cameras::shake::Shake` over synthetic camera commands,
//! frame rates, and wall raycasts without requiring game assets.
//!
//! Coverage from `tests/camera-shake.test.js`:
//! - Explosion shake falloff (`distance - 0.5 * radius`) between
//!   `ShakeRadiusInner` (`3m`) and `ShakeRadiusOuter` (`8m`), ignoring blasts
//!   closer than `1e-4m` or beyond `outer + 0.5 * radius`.
//! - PC split-screen double admission (`2` shakes per explosion) capped at `4`
//!   active slots, summing before linear decay (`dt * amount / length`).
//! - Cubic random axis turns (`(2r - 1)^3 * 0.1 * sum`) when `dt > 0` and alive,
//!   and zero random draws when paused (`dt == 0`) or in death camera mode.
//!
//! Coverage from `tests/camera.test.js`:
//! - Responsive chase camera orbiting and running at `30, 60, 144, 240 Hz`
//!   with zero screen-space pivot drift (`|ndc.x| < 1e-6`, `|ndc.y| < 1e-6`).
//! - Immediate boom retraction on wall obstruction and exponential release
//!   (`1 - exp(-6 * dt)`) once the wall clears.

use bfe_presentation::cameras::{math::V8, Cast, Content, Phase};
use bfe_presentation::layout::{
    self, camera_command as c, camera_command_kind as k, camera_frame as f, camera_output as o,
};
use three::{Matrix4, Quaternion, Vector3};

/// Synthetic raycast and deterministic random provider for `Phase::frame`.
/// Records the number of random draws consumed during each camera step.
struct SyntheticWorld {
    wall_distance: Option<f64>,
    random_value: f64,
    random_draws: usize,
}

impl SyntheticWorld {
    /// Creates an unobstructed world that returns `random_value` on each draw.
    fn new(random_value: f64) -> Self {
        Self {
            wall_distance: None,
            random_value,
            random_draws: 0,
        }
    }
}

impl Cast for SyntheticWorld {
    fn random(&mut self) -> f64 {
        self.random_draws += 1;
        self.random_value
    }

    fn cast(
        &mut self,
        _origin: Vector3,
        _direction: Vector3,
        far: f64,
        _static_only: bool,
    ) -> Option<f64> {
        self.wall_distance.filter(|&d| d <= far)
    }
}

/// Initializes a baseline soldier camera `Phase` at `(0, 0, 0)` with 16:9 aspect.
/// Sets default 60 Hz step, responsive chase mode, and identity initial orientation.
fn baseline_phase() -> Phase {
    let mut p = Phase::new(Content::default());
    let a = &mut p.input;
    a[f::VERSION] = layout::VERSION as f64;
    a[f::DT] = 1.0 / 60.0;
    a[f::ZOOM] = 1.0;
    a[f::RESPONSIVE] = 1.0;
    a[f::CAMERA_QUATERNION + 3] = 1.0;
    a[f::CAMERA_UP + 1] = 1.0;
    a[f::CAMERA_FOV] = 65.0;
    a[f::CAMERA_ASPECT] = 16.0 / 9.0;
    a[f::CAMERA_NEAR] = 0.1;
    a[f::CAMERA_FAR] = 1000.0;
    a[f::DEATH_HFOV] = 1.0;
    p
}

// Mirrors `tests/camera-shake.test.js`:
// "the camera phase measures explosion falloff at the followed origin less half its sphere"
// "explosion shake has two admissions, four slots, and sums before decaying"
#[test]
fn explosion_falloff_two_admission_cap_and_linear_decay_match_2004_rules() {
    let mut phase = baseline_phase();
    let mut world = SyntheticWorld::new(0.5);

    // Helper that fires an explosion at `distance` along +X against a target of `radius`
    // and returns half of `SHAKE_TOTAL` on a zero-decay step (`1e-9s`) to read `share`.
    let probe_share = |distance: f64, radius: f64| -> f64 {
        let mut p = baseline_phase();
        let mut w = SyntheticWorld::new(0.5);
        p.input[f::DT] = 1e-9;
        p.input[f::COMMANDS] = 1.0;
        let cmd = &mut p.commands[..c::STRIDE];
        cmd.fill(0.0);
        cmd[c::KIND] = k::EXPLOSION as f64;
        cmd[c::TARGET] = distance;
        cmd[c::RADIUS] = radius;
        cmd[c::AMOUNT] = 0.5;
        cmd[c::LENGTH] = 1.0;
        cmd[c::INNER] = 3.0;
        cmd[c::OUTER] = 8.0;
        p.frame::<V8, _>(&mut w);
        // Each explosion admits 2 shakes of `amount * share` (2 * 0.5 * share = share).
        p.output[o::SHAKE_TOTAL]
    };

    // Inside inner radius (2m with radius 2m -> d = 1m < 3m): full share (1.0).
    assert!((probe_share(2.0, 2.0) - 1.0).abs() < 1e-6);
    // Midpoint (5.5m with radius 0m -> d = 5.5m): half share (0.5).
    assert!((probe_share(5.5, 0.0) - 0.5).abs() < 1e-6);
    // Midpoint shifted by half of a 2m target radius (6.5m - 1.0m = 5.5m): half share (0.5).
    assert!((probe_share(6.5, 2.0) - 0.5).abs() < 1e-6);
    // At or beyond outer radius after subtracting half target radius: 0.0.
    assert_eq!(probe_share(9.0, 2.0), 0.0);
    // Exactly at the blast origin (distance <= 0.0001m): ignored (0.0).
    assert_eq!(probe_share(0.0, 2.0), 0.0);

    // Queue 3 explosions in one frame: each tries to add 2 shakes, capped at 4 slots total.
    phase.input[f::COMMANDS] = 3.0;
    for (row, amount) in [0.4, 0.2, 1.0].iter().enumerate() {
        let at = row * c::STRIDE;
        let cmd = &mut phase.commands[at..at + c::STRIDE];
        cmd.fill(0.0);
        cmd[c::KIND] = k::EXPLOSION as f64;
        cmd[c::TARGET] = 1.0;
        cmd[c::RADIUS] = 1.0;
        cmd[c::AMOUNT] = *amount;
        cmd[c::LENGTH] = 1.0;
        cmd[c::INNER] = 3.0;
        cmd[c::OUTER] = 8.0;
    }

    // First 0.25s step: sums [0.4, 0.4, 0.2, 0.2] = 1.2 before decaying by 25%;
    // the third explosion's two shakes were refused (`REFUSED == 2.0`).
    phase.input[f::DT] = 0.25;
    phase.frame::<V8, _>(&mut world);
    assert!((phase.output[o::SHAKE_TOTAL] - 1.2).abs() < 1e-9);
    assert_eq!(phase.output[o::REFUSED], 2.0);

    // Next three 0.25s steps without new commands decay linearly by 0.3 per step: 0.9, 0.6, 0.3.
    phase.input[f::COMMANDS] = 0.0;
    for expected in [0.9, 0.6, 0.3] {
        phase.frame::<V8, _>(&mut world);
        assert!((phase.output[o::SHAKE_TOTAL] - expected).abs() < 1e-9);
    }

    // Once all 4 shakes have decayed through their 1.0s lifetime, the next step has 0 total.
    phase.frame::<V8, _>(&mut world);
    assert!(phase.output[o::SHAKE_TOTAL].abs() < 1e-12);
}

// Mirrors `tests/camera-shake.test.js`:
// "paused and death views consume no random turns while live views turn on all three axes"
#[test]
fn shake_turns_view_by_cubic_random_draw_and_skips_turns_when_paused_or_dead() {
    let mut phase = baseline_phase();
    let mut world = SyntheticWorld::new(1.0);

    // Baseline frame with no shake establishes the unshaken quaternion.
    phase.input[f::DT] = 0.1;
    phase.frame::<V8, _>(&mut world);
    let base_q = Quaternion::new(
        phase.output[o::QUATERNION],
        phase.output[o::QUATERNION + 1],
        phase.output[o::QUATERNION + 2],
        phase.output[o::QUATERNION + 3],
    );

    // Admit a single shake of amount 0.5, length 1.0 (`ADD_SHAKE`).
    phase.input[f::COMMANDS] = 1.0;
    phase.commands[..c::STRIDE].fill(0.0);
    phase.commands[c::KIND] = k::ADD_SHAKE as f64;
    phase.commands[c::AMOUNT] = 0.5;
    phase.commands[c::LENGTH] = 1.0;

    // Paused (`DT == 0`): consumes 0 random draws and does not rotate the view.
    world.random_draws = 0;
    phase.input[f::DT] = 0.0;
    phase.frame::<V8, _>(&mut world);
    assert_eq!(world.random_draws, 0);
    assert_eq!(phase.output[o::QUATERNION + 3], base_q.w);

    // Active step (`DT == 0.1`) with `r = 1.0`: consumes 3 random draws and rotates the view,
    // while `PRE_QUATERNION` remains unshaken (`base_q`).
    phase.input[f::COMMANDS] = 0.0;
    phase.input[f::DT] = 0.1;
    phase.frame::<V8, _>(&mut world);
    assert_eq!(world.random_draws, 3);
    assert!((phase.output[o::PRE_QUATERNION + 3] - base_q.w).abs() < 1e-9);
    let shaken_q = Quaternion::new(
        phase.output[o::QUATERNION],
        phase.output[o::QUATERNION + 1],
        phase.output[o::QUATERNION + 2],
        phase.output[o::QUATERNION + 3],
    );
    assert!(base_q.angle_to(&shaken_q) > 0.05);

    // Death camera (`DEATH_ACTIVE = 1`): decays the remaining shake without drawing random numbers.
    world.random_draws = 0;
    phase.input[f::DEATH_ACTIVE] = 1.0;
    phase.frame::<V8, _>(&mut world);
    assert_eq!(world.random_draws, 0);
    assert!((phase.output[o::SHAKE_TOTAL] - 0.45).abs() < 1e-9);
}

// Mirrors `tests/camera.test.js`:
// "responsive chase view follows mouse aim and an unobstructed orbit on the same frame"
// "native chase retracts against a wall and releases continuously"
#[test]
fn responsive_chase_camera_locks_pivot_across_frame_rates_and_retracts_at_walls() {
    // At 30, 60, 144, and 240 Hz, the soldier's TrackCenter (feet + [0, 1.8, 0])
    // stays dead center on screen (`ndc.x == 0, ndc.y == 0`) while running and orbiting.
    for rate in [30.0, 60.0, 144.0, 240.0] {
        let mut phase = baseline_phase();
        let mut world = SyntheticWorld::new(0.5);
        let dt = 1.0 / rate;
        let steps = (rate * 0.5) as usize;
        for i in 0..steps {
            let t = (i + 1) as f64 * dt;
            let feet = [t * 6.0, 0.0, (t * 3.0).sin() * 2.0];
            phase.input[f::DT] = dt;
            phase.input[f::FEET..f::FEET + 3].copy_from_slice(&feet);
            phase.input[f::YAW] = t * 4.0;
            phase.input[f::PITCH] = (t * 5.0).sin() * 0.4;
            phase.frame::<V8, _>(&mut world);

            // Project world pivot `feet + (0, 1.8, 0)` through `INVERSE` then `PROJECTION`.
            let mut inv = Matrix4::default();
            inv.elements
                .copy_from_slice(&phase.output[o::INVERSE..o::INVERSE + 16]);
            let mut proj = Matrix4::default();
            proj.elements
                .copy_from_slice(&phase.output[o::PROJECTION..o::PROJECTION + 16]);
            let mut ndc = Vector3::new(feet[0], feet[1] + 1.8, feet[2]);
            ndc.apply_matrix4(&inv);
            ndc.apply_matrix4(&proj);
            assert!(
                ndc.x.abs() < 1e-6,
                "rate {rate} step {i}: ndc.x = {}",
                ndc.x
            );
            assert!(
                ndc.y.abs() < 1e-6,
                "rate {rate} step {i}: ndc.y = {}",
                ndc.y
            );
        }
    }

    // Wall retraction and smooth exponential release (`1 - exp(-6 * dt)`).
    let mut phase = baseline_phase();
    let mut world = SyntheticWorld::new(0.5);
    let pivot = Vector3::new(0.0, 1.8, 0.0);

    // Unobstructed first frame places the camera at full boom length (~3.5m from pivot).
    phase.frame::<V8, _>(&mut world);
    let full_dist = Vector3::new(
        phase.output[o::POSITION],
        phase.output[o::POSITION + 1],
        phase.output[o::POSITION + 2],
    )
    .distance_to(&pivot);
    assert!((full_dist - 3.5).abs() < 1e-6);

    // Wall at 1.4m pulls the boom in immediately on the very next frame (< 1.4m).
    world.wall_distance = Some(1.4);
    phase.frame::<V8, _>(&mut world);
    let pulled_dist = Vector3::new(
        phase.output[o::POSITION],
        phase.output[o::POSITION + 1],
        phase.output[o::POSITION + 2],
    )
    .distance_to(&pivot);
    assert!(pulled_dist < 1.4 && pulled_dist > 1.2);

    // Removing the wall eases the boom back out smoothly rather than snapping to 3.5m.
    world.wall_distance = None;
    phase.frame::<V8, _>(&mut world);
    let eased_dist = Vector3::new(
        phase.output[o::POSITION],
        phase.output[o::POSITION + 1],
        phase.output[o::POSITION + 2],
    )
    .distance_to(&pivot);
    assert!(eased_dist > pulled_dist && eased_dist < full_dist);
}
