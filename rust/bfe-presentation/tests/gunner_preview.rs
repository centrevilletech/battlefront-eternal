//! Turret gunner preview, drum/gun pose, and reticle convergence tests ported
//! from `tests/gunner-preview.test.js`.
//!
//! These cases exercise `bfe_presentation::network::preview::Phase`,
//! `bfe_presentation::camera_effects::Gunner`, `bfe_presentation::turrets::Turret`,
//! and `bfe_presentation::cameras::Phase` (`TURRET_ACTIVE`) against the golden
//! turret view values in `tests/fixtures/turret-view-golden.json`.
//!
//! Coverage includes:
//! - `network::preview::Phase` steering the drum ahead across queued commands
//!   and loose mouse looks (`turret-view-golden.json` `state`), plus alpha
//!   interpolation back through the 60 Hz tick.
//! - `Turret::present` posing the turret drum and gun barrel across `camera`,
//!   `preview`, `pose`, and `bot` golden states (`turret-view-golden.json` `poses`).
//! - `Gunner` and `cameras::Phase` in third-person chase and first-person eye
//!   views, plus `Gunner::wall` and `Gunner::resolve` reticle convergence.

use bfe_presentation::camera_effects::Gunner;
use bfe_presentation::cameras::{math::V8, Cast, Content, Phase};
use bfe_presentation::layout::{
    self, camera_frame as cf, camera_output as co, turret_presentation_input as ti,
};
use bfe_presentation::network::preview::Phase as PreviewPhase;
use bfe_presentation::network_layout::{
    net_limits as lim, net_preview_command as pc, net_preview_frame as pf, net_preview_look as pl,
    net_preview_output as po,
};
use bfe_presentation::scene::{Input as SceneInput, Node as SceneNode};
use bfe_presentation::turrets::{Descriptor as TurretDescriptor, Turret};
use bfe_presentation::visual_effects_layout::gunner_frame as g;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use three::{Matrix4, Quaternion, Vector3};

const DEG: f64 = std::f64::consts::PI / 180.0;

/// Deserialized schema of `tests/fixtures/turret-view-golden.json`.
/// Holds both the 4-tick servo preview state (`state`) and the 4 golden
/// drum/gun quaternion pairs (`poses`).
#[derive(Deserialize)]
struct TurretGolden {
    state: StateGolden,
    poses: HashMap<String, PosePairGolden>,
}

/// Golden turret servo state after replaying the 4 queued mouse commands.
/// Values are recorded in radians (`yaw`, `pitch`) and rad/s (`yaw_vel`, `pitch_vel`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StateGolden {
    yaw: f64,
    pitch: f64,
    yaw_vel: f64,
    pitch_vel: f64,
}

/// Golden `[x, y, z, w]` local quaternions for a turret's `drum` and `gun` nodes.
/// Comparing via `Quaternion::angle_to` handles sign-equivalent quaternion covers.
#[derive(Deserialize)]
struct PosePairGolden {
    drum: [f64; 4],
    gun: [f64; 4],
}

/// Unobstructed raycast world for `cameras::Phase::frame`.
/// Always returns `None` so third-person turret cameras sit at their full chase offset.
struct NoWall;
impl Cast for NoWall {
    fn cast(&mut self, _: Vector3, _: Vector3, _: f64, _: bool) -> Option<f64> {
        None
    }
}

/// Loads `tests/fixtures/turret-view-golden.json`.
/// This fixture captures the exact float outputs of the reference turret camera and servo.
fn load_golden() -> TurretGolden {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/turret-view-golden.json");
    let raw = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&raw).unwrap()
}

/// Builds a synthetic `Turret` matching `turret()` in `tests/gunner-preview.test.js`
/// at `(10, -2, 5)` facing `0.3 rad` around `+Y`.
/// Node 0 is the building root, node 1 is `drum` at `(0, 0.6, 0)`, and node 2
/// is `gun` at `(0, 0.8, 0.6)`.
fn make_turret() -> (Turret, Vec<f64>, Matrix4) {
    let mut root_q = Quaternion::default();
    root_q.set_from_axis_angle(&Vector3::new(0.0, 1.0, 0.0), 0.3);
    let mut root_mat = Matrix4::default();
    root_mat.compose(
        &Vector3::new(10.0, -2.0, 5.0),
        &root_q,
        &Vector3::new(1.0, 1.0, 1.0),
    );
    let id_mat = Matrix4::default().elements;
    let mut out = Vec::new();
    // Register the 3-node hierarchy (`root -> drum -> gun`) with identity parent/gun binds.
    // `PitchTurnFactor = 0.0` so the drum only yaws while the gun pitches.
    // `YawTurnFactor = 1.0` so the drum receives the full yaw angle.
    let turret = Turret::new(
        TurretDescriptor {
            scene: SceneInput {
                nodes: vec![
                    SceneNode {
                        parent: -1,
                        pose: [
                            10.0, -2.0, 5.0, root_q.x, root_q.y, root_q.z, root_q.w, 1.0, 1.0, 1.0,
                        ],
                        matrix: id_mat,
                        auto: true,
                        skinned: false,
                    },
                    SceneNode {
                        parent: 0,
                        pose: [0.0, 0.6, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                        matrix: id_mat,
                        auto: true,
                        skinned: false,
                    },
                    SceneNode {
                        parent: 1,
                        pose: [0.0, 0.8, 0.6, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                        matrix: id_mat,
                        auto: true,
                        skinned: false,
                    },
                ],
                bones: Vec::new(),
                parent: None,
            },
            seat: 0,
            source_shown: false,
            turn_rate: 0.0,
            drum: 1,
            gun: 2,
            parent_turn: Some([0.0, 0.0, 0.0, 1.0]),
            gun_bind: Some([0.0, 0.0, 0.0, 1.0]),
            up: [0.0, 1.0, 0.0],
            yaw_factor: 1.0,
            pitch_factor: 0.0,
        },
        &mut out,
    );
    (turret, out, root_mat)
}

// Mirrors `tests/gunner-preview.test.js`:
// "previewSteer runs the drum ahead exactly as the world steers it, without touching the turret"
// "preview sets the yaw and pitch shown over the world aim, back through the tick by alpha"
#[test]
fn preview_steer_and_gunner_alpha_unwind_match_golden_state() {
    let golden = load_golden();
    let mut prev = PreviewPhase::new();

    // Base turret servo parameters from `turret()` in `tests/gunner-preview.test.js`:
    // `MaxTurnSpeed = 2.85`, `TurnFilter = 10.0`, `PitchLimits = [-45, 45]`, `facing = 0.3`.
    // These match the stock laser turret's first-order servo damper and pitch clamp.
    prev.frame[pf::ENABLED] = 1.0;
    prev.frame[pf::TURRET_ACTIVE] = 1.0;
    prev.frame[pf::TURN_RATE] = 2.85;
    prev.frame[pf::FILTER] = 10.0;
    prev.frame[pf::TURRET_PITCH_LOW] = -45.0 * DEG;
    prev.frame[pf::TURRET_PITCH_HIGH] = 45.0 * DEG;
    prev.frame[pf::FACING] = 0.3;
    prev.frame[pf::SCALE] = 0.5;
    prev.frame[pf::INVERT] = 0.0;
    prev.frame[pf::ALPHA] = 1.0;
    prev.frame[pf::INPUT_ALPHA] = 1.0;
    prev.frame[pf::PRESENT_ALPHA] = 1.0;

    // Queue the 4 pending commands:
    // 0: [[6, 0], [2, 1]], 1: [[0, -4]], 2: [], 3: [[8, 0]] with scale 0.25, invert true.
    // Even an empty command (command 2) steps the turret's velocity filter by 1/60s.
    // Command 3 overrides sensitivity scale (`0.25`) and Y-inversion (`1.0`).
    let cmd_looks: [&[[f64; 2]]; 4] = [
        &[[6.0, 0.0], [2.0, 1.0]],
        &[[0.0, -4.0]],
        &[],
        &[[8.0, 0.0]],
    ];
    let mut look_cursor = 0usize;
    prev.frame[pf::COMMANDS] = 4.0;
    prev.frame[pf::HAS_LOOSE] = 0.0;
    for (idx, samples) in cmd_looks.iter().enumerate() {
        let cat = idx * pc::STRIDE;
        prev.commands[cat..cat + pc::STRIDE].fill(0.0);
        prev.commands[cat + pc::LOOK_AT] = look_cursor as f64;
        prev.commands[cat + pc::LOOK_COUNT] = samples.len() as f64;
        prev.commands[cat + pc::HAS_SCALE] = 1.0;
        prev.commands[cat + pc::SCALE] = if idx == 3 { 0.25 } else { 0.5 };
        prev.commands[cat + pc::HAS_INVERT] = 1.0;
        prev.commands[cat + pc::INVERT] = if idx == 3 { 1.0 } else { 0.0 };
        for &[lx, ly] in *samples {
            let lat = look_cursor * pl::STRIDE;
            prev.looks[lat + pl::X] = lx;
            prev.looks[lat + pl::Y] = ly;
            look_cursor += 1;
        }
    }

    // Verify all 4 servo state outputs match `turret-view-golden.json` `state` within 1e-12.
    // This confirms `preview::Phase` runs the exact same servo math as `stepTurretAim`.
    prev.present();
    assert!((prev.output[po::TURRET_YAW] - golden.state.yaw).abs() < 1e-12);
    assert!((prev.output[po::TURRET_PITCH] - golden.state.pitch).abs() < 1e-12);
    assert!((prev.output[po::TURRET_YAW_VEL] - golden.state.yaw_vel).abs() < 1e-12);
    assert!((prev.output[po::TURRET_PITCH_VEL] - golden.state.pitch_vel).abs() < 1e-12);

    // Loose looks `[[6, 0]]` count as one more tick, matching a 5th queued command.
    // Loose looks are stored at the tail of `prev.looks` (`PREVIEW_LOOKS - count`).
    // This lets sub-tick mouse input steer the local turret view before the next 60 Hz seal.
    prev.frame[pf::HAS_LOOSE] = 1.0;
    prev.frame[pf::LOOKS] = 1.0;
    let tail_at = (lim::PREVIEW_LOOKS - 1) * pl::STRIDE;
    prev.looks[tail_at + pl::X] = 6.0;
    prev.looks[tail_at + pl::Y] = 0.0;
    prev.present();
    let yaw_with_loose = prev.output[po::TURRET_YAW];

    // Promoting that loose look into a 5th queued command produces the exact same yaw.
    // Consequently, sealing a command tick never causes a visual snap in the gunner view.
    prev.frame[pf::HAS_LOOSE] = 0.0;
    prev.frame[pf::LOOKS] = 0.0;
    prev.frame[pf::COMMANDS] = 5.0;
    let cat4 = 4 * pc::STRIDE;
    prev.commands[cat4..cat4 + pc::STRIDE].fill(0.0);
    prev.commands[cat4 + pc::LOOK_AT] = look_cursor as f64;
    prev.commands[cat4 + pc::LOOK_COUNT] = 1.0;
    prev.commands[cat4 + pc::HAS_SCALE] = 1.0;
    prev.commands[cat4 + pc::SCALE] = 0.5;
    let lat4 = look_cursor * pl::STRIDE;
    prev.looks[lat4 + pl::X] = 6.0;
    prev.looks[lat4 + pl::Y] = 0.0;
    prev.present();
    assert!((yaw_with_loose - prev.output[po::TURRET_YAW]).abs() < 1e-12);

    // Alpha unwind: with yaw=0.2, pitch=0.1, yawVel=0.6, pitchVel=-0.3 and no commands,
    // alpha=1 gives 0 preview offset and alpha=0 unwinds one 60 Hz tick.
    // At `PRESENT_ALPHA = 0.0`, the offset equals `-vel * (1 / 60)`.
    prev.frame[pf::COMMANDS] = 0.0;
    prev.frame[pf::TURRET_YAW] = 0.2;
    prev.frame[pf::TURRET_PITCH] = 0.1;
    prev.frame[pf::YAW_VEL] = 0.6;
    prev.frame[pf::PITCH_VEL] = -0.3;
    prev.frame[pf::PRESENT_ALPHA] = 1.0;
    prev.present();
    assert!(prev.output[po::TURRET_PREVIEW_YAW].abs() < 1e-12);
    assert!(prev.output[po::TURRET_PREVIEW_PITCH].abs() < 1e-12);

    // Unwinding by a full tick (`PRESENT_ALPHA = 0.0`) shifts yaw by `-0.01` and pitch by `+0.005`.
    prev.frame[pf::PRESENT_ALPHA] = 0.0;
    prev.present();
    assert!((prev.output[po::TURRET_PREVIEW_YAW] + 0.6 / 60.0).abs() < 1e-12);
    assert!((prev.output[po::TURRET_PREVIEW_PITCH] - 0.3 / 60.0).abs() < 1e-12);
}

// Mirrors `tests/gunner-preview.test.js`:
// "the page camera follows a turret it was never entered on, and poses its drum and gun"
// "first person looks from the eye down the aim, the drum and gun hidden from inside the turret, and comes back"
// "poseTurret keeps the recorded drum and gun angles"
// "the native prop frame poses every built turret from its recorded angles"
#[test]
fn turret_poses_camera_views_and_reticle_convergence_match_golden_values() {
    let golden = load_golden();
    let (mut turret, mut out, root_mat) = make_turret();

    // Helper closure to pose `turret` with `(yaw, gun_yaw, gun_pitch)` and compare to `golden.poses[key]`.
    // Reads the local quaternions written onto the `Scene` tree's drum (1) and gun (2) nodes.
    // Angular error between the computed quaternion and golden quaternion must be `< 1e-6 rad`.
    let mut check_pose = |key: &str, yaw: f64, gun_yaw: f64, gun_pitch: f64| {
        let mut t_in = [0.0; ti::STRIDE];
        t_in[ti::YAW] = yaw;
        t_in[ti::GUN_YAW] = gun_yaw;
        t_in[ti::GUN_PITCH] = gun_pitch;
        turret.present(&t_in, &mut out);
        let drum_q = turret.scene.tree.node(turret.scene.nodes[1]).quaternion;
        let gun_q = turret.scene.tree.node(turret.scene.nodes[2]).quaternion;
        let want = &golden.poses[key];
        let want_drum = Quaternion::new(want.drum[0], want.drum[1], want.drum[2], want.drum[3]);
        let want_gun = Quaternion::new(want.gun[0], want.gun[1], want.gun[2], want.gun[3]);
        assert!(drum_q.angle_to(&want_drum) < 1e-6, "{key} drum");
        assert!(gun_q.angle_to(&want_gun) < 1e-6, "{key} gun");
    };

    // Check all 4 golden pose snapshots (`camera`, `preview`, `pose`, and `bot`).
    // `camera` and `pose` include non-zero `gun_yaw` and `gun_pitch`; `preview` and `bot` are yaw-only.
    // Each call updates the turret scene tree in place and writes 13 pose floats into `out`.
    check_pose("camera", -0.5, 0.05, -0.1);
    check_pose("preview", -0.5 + 0.3, 0.0, 0.0);
    check_pose("pose", 1.1, 0.08, 0.3);
    check_pose("bot", 0.7, 0.0, 0.0);

    // First-person and third-person turret camera placement for `yaw = 0.4, pitch = -0.2`
    // (`Gunner` takes world yaw `facing + yaw = 0.3 + 0.4`).
    // `EyePointOffset = (0, 1.5, 1)`, `TrackCenter = (0, 1.5, -2)`, `TrackOffset = (0, 0.5, 1.5)`.
    // `TiltValue = 5 deg` tilts the third-person chase look slightly downward.
    let mut gunner = Gunner::default();
    gunner.input[g::ACTIVE] = 1.0;
    gunner.input[g::FACING] = 0.3;
    gunner.input[g::YAW] = 0.3 + 0.4;
    gunner.input[g::PITCH] = -0.2;
    gunner.input[g::FIRST] = 1.0;
    gunner.input[g::ROOT..g::ROOT + 16].copy_from_slice(&root_mat.elements);
    gunner.input[g::PIVOT..g::PIVOT + 3].copy_from_slice(&[0.0, 0.6, 0.0]);
    gunner.input[g::UP..g::UP + 3].copy_from_slice(&[0.0, 1.0, 0.0]);
    gunner.input[g::EYE_OFFSET..g::EYE_OFFSET + 3].copy_from_slice(&[0.0, 1.5, 1.0]);
    gunner.input[g::CENTER_OFFSET..g::CENTER_OFFSET + 3].copy_from_slice(&[0.0, 1.5, -2.0]);
    gunner.input[g::TRACK_OFFSET..g::TRACK_OFFSET + 3].copy_from_slice(&[0.0, 0.5, 1.5]);
    gunner.input[g::TILT] = 5.0 * DEG;
    gunner.frame();

    // Compute the analytical world eye position from the drum pivot `(10, -1.4, 5)`,
    // shifted `1.0m` along `aim` and `1.5m` along the pitched aim frame's `up`.
    // In first-person mode, `Gunner::frame` outputs this exact eye position into `g::EYE`.
    let aim = Vector3::new(
        gunner.input[g::DIRECTION],
        gunner.input[g::DIRECTION + 1],
        gunner.input[g::DIRECTION + 2],
    );
    let yaw: f64 = 0.3 + 0.4;
    let pitch: f64 = -0.2;
    let up = Vector3::new(
        -yaw.sin() * pitch.sin(),
        pitch.cos(),
        -yaw.cos() * pitch.sin(),
    );
    let mut expected_eye = Vector3::new(10.0, -1.4, 5.0);
    expected_eye.add_scaled_vector(&aim, 1.0);
    expected_eye.add_scaled_vector(&up, 1.5);

    // Populate `cameras::Phase` for turret mode (`TURRET_ACTIVE = 1.0`).
    // Feeds the canonical `DIRECTION` and `EYE` vectors computed by `Gunner::frame`.
    // `TURRET_RATES` are set to `30.0` so the camera tracks without lag on the initial frame.
    let mut phase = Phase::new(Content::default());
    let a = &mut phase.input;
    a[cf::VERSION] = layout::VERSION as f64;
    a[cf::DT] = 1.0 / 60.0;
    a[cf::CAMERA_QUATERNION + 3] = 1.0;
    a[cf::CAMERA_UP + 1] = 1.0;
    a[cf::CAMERA_FOV] = 48.0;
    a[cf::CAMERA_ASPECT] = 16.0 / 9.0;
    a[cf::CAMERA_NEAR] = 0.1;
    a[cf::CAMERA_FAR] = 1000.0;
    a[cf::TURRET_ACTIVE] = 1.0;
    a[cf::TURRET_ID] = 1.0;
    a[cf::TURRET_ROOT..cf::TURRET_ROOT + 16].copy_from_slice(&root_mat.elements);
    a[cf::TURRET_UP..cf::TURRET_UP + 3].copy_from_slice(&[0.0, 1.0, 0.0]);
    a[cf::TURRET_CANONICAL_DIRECTION] = 1.0;
    a[cf::TURRET_DIRECTION..cf::TURRET_DIRECTION + 3].copy_from_slice(&[aim.x, aim.y, aim.z]);
    a[cf::TURRET_CANONICAL_EYE] = 1.0;
    a[cf::TURRET_EYE..cf::TURRET_EYE + 3].copy_from_slice(&[
        gunner.input[g::EYE],
        gunner.input[g::EYE + 1],
        gunner.input[g::EYE + 2],
    ]);
    a[cf::TURRET_CENTER..cf::TURRET_CENTER + 3].copy_from_slice(&[0.0, 1.5, -2.0]);
    a[cf::TURRET_OFFSET..cf::TURRET_OFFSET + 3].copy_from_slice(&[0.0, 0.5, 1.5]);
    a[cf::TURRET_PIVOT..cf::TURRET_PIVOT + 3].copy_from_slice(&[0.0, 0.6, 0.0]);
    a[cf::TURRET_TILT] = 5.0 * DEG;
    a[cf::TURRET_FIRST] = 48.0 * DEG;
    a[cf::TURRET_THIRD] = 48.0 * DEG;
    a[cf::TURRET_RATES..cf::TURRET_RATES + 6].fill(30.0);

    // First-person camera sits at `EyePointOffset` in the aim frame and looks along `aim`.
    // Applying `cam_q` to `(0, 0, -1)` recovers `aim` within `1e-6 rad`.
    phase.input[cf::FIRST_PERSON] = 1.0;
    phase.frame::<V8, _>(&mut NoWall);
    let cam_pos = Vector3::new(
        phase.output[co::POSITION],
        phase.output[co::POSITION + 1],
        phase.output[co::POSITION + 2],
    );
    let cam_q = Quaternion::new(
        phase.output[co::QUATERNION],
        phase.output[co::QUATERNION + 1],
        phase.output[co::QUATERNION + 2],
        phase.output[co::QUATERNION + 3],
    );
    let mut look = Vector3::new(0.0, 0.0, -1.0);
    look.apply_quaternion(&cam_q);
    assert!(look.angle_to(&aim) < 1e-6);
    assert!(cam_pos.distance_to(&expected_eye) < 1e-6);

    // Third-person chase camera sits near the turret (`< 5m` from `(10, -2, 5)`).
    // Toggling `FIRST_PERSON` back to `0.0` switches cleanly from the eye offset to the chase rig.
    phase.input[cf::FIRST_PERSON] = 0.0;
    phase.frame::<V8, _>(&mut NoWall);
    let chase_pos = Vector3::new(
        phase.output[co::POSITION],
        phase.output[co::POSITION + 1],
        phase.output[co::POSITION + 2],
    );
    assert!(chase_pos.distance_to(&Vector3::new(10.0, -2.0, 5.0)) < 5.0);

    // Reticle convergence from camera `(1, 2, 0)` to wall (`30m`) or closer unit (`12m`).
    // When both `WALL` and `UNIT` report hits, `Gunner::resolve` converges on `UNIT_DISTANCE`.
    // The resulting `RETICLE` unit vector points from `CAMERA` toward `ORIGIN + DIRECTION * 12`.
    // This ensures the third-person reticle lines up with the closer enemy unit hit.
    gunner.input[g::CAMERA..g::CAMERA + 3].copy_from_slice(&[1.0, 2.0, 0.0]);
    gunner.input[g::WALL] = 1.0;
    gunner.input[g::WALL_DISTANCE] = 30.0;
    gunner.wall();
    gunner.input[g::UNIT] = 1.0;
    gunner.input[g::UNIT_DISTANCE] = 12.0;
    gunner.resolve();
    let mut want_reticle = Vector3::new(
        gunner.input[g::ORIGIN] + gunner.input[g::DIRECTION] * 12.0 - 1.0,
        gunner.input[g::ORIGIN + 1] + gunner.input[g::DIRECTION + 1] * 12.0 - 2.0,
        gunner.input[g::ORIGIN + 2] + gunner.input[g::DIRECTION + 2] * 12.0,
    );
    want_reticle.normalize();
    for c in 0..3 {
        let comp = [want_reticle.x, want_reticle.y, want_reticle.z][c];
        assert!((gunner.input[g::RETICLE + c] - comp).abs() < 1e-9);
    }
}
