//! Synthetic vehicle thrust, damage, destruction, dying-walker, and death-pose
//! presentation tests ported from `tests/vehicle-effects.test.js` and
//! `tests/walker-dying.test.js`.
//!
//! These cases exercise `bfe_presentation::vehicle_effects::Vehicles` and
//! `bfe_presentation::vehicles::Vehicles` over synthetic descriptors and rigs
//! without requiring game assets.
//!
//! Coverage from `tests/vehicle-effects.test.js`:
//! - Thrust scale ramp (`thrustScale`) from `MinSpeed + ScaleStart * (MaxSpeed - MinSpeed)`
//!   up to `MaxSpeed`, clamped between `minScale` and `maxScale`.
//! - Health band checks (`damageBurns`) with exclusive lower bound (`stop`)
//!   and inclusive upper bound (`start`).
//! - Flight-state gating (`TakingOff`, `Flying`, `Landing` vs. `Landed`),
//!   node-attached damage plumes with root fallback for missing nodes,
//!   inherited velocity share, and scale.
//! - Dying-walker periodic random node explosions (`query::RANDOM` ->
//!   `query::MATRIX`), delayed `DeathDustEffect` while dying, final chunk
//!   release (`BLOW_APART`, `FALLEN`), and `bodyCarrier` inverse-bind matrix
//!   composition so chunks spawn from the fallen body pose.
//!
//! Coverage from `tests/walker-dying.test.js`:
//! - Posing `death01` from the remaining `dying` clock at `AnimationSpeed`,
//!   holding the last frame at clip end.
//! - Smoothstepped `DEATH_BLEND` transition from the last pose drawn alive
//!   rather than snapping to frame 0 of `death01`.

use bfe_presentation::layout::{
    vehicle_entry as ve, vehicle_frame as vf, vehicle_mode as vmode, vehicle_node as vn,
    VERSION as LAYOUT_VERSION,
};
use bfe_presentation::vehicle_effects::{Descriptor, Vehicles as VehicleEffects};
use bfe_presentation::vehicles::{BoneInput, ClipRef, RigInput, Vehicles as VehiclePoser};
use bfe_presentation::visual_effects_layout::{
    vehicle_fx_action as action, vehicle_fx_flight as flight, vehicle_fx_frame as fr,
    vehicle_fx_input as vi, vehicle_fx_inspect as ti, vehicle_fx_node as fnode,
    vehicle_fx_query as query, vehicle_fx_record as vo, vehicle_fx_status as vs,
};
use three::{Matrix4, Quaternion, Vector3};

/// Writes a 4x4 translation matrix at node `node` for body `id`.
fn set_node_translation(fx: &mut VehicleEffects, id: usize, node: usize, x: f64, y: f64, z: f64) {
    let at = (fx.node_start(id) + node) * fnode::STRIDE;
    let mut m = Matrix4::default();
    m.make_translation(x, y, z);
    fx.nodes[at..at + 16].copy_from_slice(&m.elements);
}

/// Steps `fx` for a single body `id` and marks newly played persistent handles
/// as present for the next frame.
fn step_single(fx: &mut VehicleEffects, id: usize, persistent: usize) {
    fx.frame[fr::COUNT] = 1.0;
    fx.order[0] = id as u32;
    fx.frame();
    // Mirror the host handle lifecycle by updating persistent handle status flags.
    let out_base = fx.output_start(id);
    for k in 0..persistent {
        let act = fx.output[(out_base + k) * vo::STRIDE + vo::ACTION] as u32;
        let st = (out_base + k) * vs::STRIDE;
        if act == action::PLAY || act == action::MOVE {
            fx.status[st + vs::PRESENT] = 1.0;
            fx.status[st + vs::DONE] = 0.0;
        } else if act == action::STOP {
            fx.status[st + vs::PRESENT] = 0.0;
        }
    }
}

// Mirrors `tests/vehicle-effects.test.js`:
// "thrust scale rests at its minimum, ramps from ScaleStart of the way to MaxSpeed and stops at its maximum"
// "a damage effect burns at most its start share of MaxHealth and above its stop share"
#[test]
fn thrust_scale_and_damage_burn_bands_match_authored_thresholds() {
    let mut fx = VehicleEffects::default();
    // Evaluate thrust scale across rest, ramp start, midpoint, MaxSpeed, and boost.
    let check_thrust = |fx: &mut VehicleEffects, speed: f64, scale_start: f64| -> f64 {
        fx.inspection[ti::SPEED] = speed;
        fx.inspection[ti::MIN_SPEED] = 20.0;
        fx.inspection[ti::MAX_SPEED] = 72.0;
        fx.inspection[ti::SCALE_START] = scale_start;
        fx.inspection[ti::MIN_SCALE] = 0.5;
        fx.inspection[ti::MAX_SCALE] = 1.4;
        fx.inspect();
        fx.inspection[ti::SCALE]
    };

    // ScaleStart = 0.55 begins ramping at 20 + (72 - 20) * 0.55 = 48.6 m/s.
    let start = 20.0 + (72.0 - 20.0) * 0.55;
    assert_eq!(check_thrust(&mut fx, 0.0, 0.55), 0.5);
    assert_eq!(check_thrust(&mut fx, start, 0.55), 0.5);
    assert!((check_thrust(&mut fx, (start + 72.0) / 2.0, 0.55) - 0.95).abs() < 1e-9);
    assert_eq!(check_thrust(&mut fx, 72.0, 0.55), 1.4);
    assert_eq!(check_thrust(&mut fx, 150.0, 0.55), 1.4);
    // A ramp that starts at MaxSpeed (scaleStart = 1) jumps to maxScale above it.
    assert_eq!(check_thrust(&mut fx, 80.0, 1.0), 1.4);

    // Damage burns strictly above stop * max and at or below start * max.
    let check_burns = |fx: &mut VehicleEffects, health: f64, start_share: f64, stop_share: f64| {
        fx.inspection[ti::HEALTH] = health;
        fx.inspection[ti::MAX_HEALTH] = 100.0;
        fx.inspection[ti::START] = start_share;
        fx.inspection[ti::STOP] = stop_share;
        fx.inspect();
        fx.inspection[ti::BURNS] != 0.0
    };
    // Full health (100), exactly at stop (20), and dead (0) do not burn.
    assert!(!check_burns(&mut fx, 100.0, 0.6, 0.2));
    assert!(check_burns(&mut fx, 60.0, 0.6, 0.2));
    assert!(check_burns(&mut fx, 21.0, 0.6, 0.2));
    assert!(!check_burns(&mut fx, 20.0, 0.6, 0.2));
    assert!(!check_burns(&mut fx, 0.0, 0.3, 0.0));
}

// Mirrors `tests/vehicle-effects.test.js`:
// "thrust burns from takeoff to touchdown, at its attach point, scaled by speed"
// "damage effects follow the health bands at their nodes, with their scale and inherited velocity"
#[test]
fn thrust_and_damage_effects_follow_flight_states_nodes_and_health_bands() {
    // Register a synthetic craft with two wingtip thrust nozzles and two damage bands.
    // Damage entry 0 attaches to node 1; damage entry 1 uses out-of-range node 99 (root fallback).
    let mut fx = VehicleEffects::default();
    let desc: Descriptor = serde_json::from_str(
        r#"{
            "nodes": 2,
            "speed": [20.0, 72.0],
            "thrust": [
                {"effect": 1, "node": 0, "offset": [0.7, -0.3, -2.4], "minScale": 0.5, "maxScale": 1.4, "scaleStart": 0.55},
                {"effect": 1, "node": 0, "offset": [-0.7, -0.3, -2.4], "minScale": 0.5, "maxScale": 1.4, "scaleStart": 0.55}
            ],
            "damage": [
                {"effect": 2, "node": 1, "offset": [0.0, 0.0, 0.0], "start": 0.6, "stop": 0.2, "scale": 1.0, "inherit": 0.75},
                {"effect": 3, "node": 99, "offset": [0.0, 0.0, 1.0], "start": 0.3, "stop": 0.0, "scale": 0.4, "inherit": 0.0}
            ]
        }"#,
    )
    .unwrap();
    let id = fx.add(desc);
    set_node_translation(&mut fx, id, 0, 10.0, 0.0, 0.0);
    set_node_translation(&mut fx, id, 1, 10.0, 1.0, 0.0);

    // Initialize the vehicle alive on the landing pad at full health.
    let at = id * vi::STRIDE;
    fx.input[at + vi::PRESENT] = 1.0;
    fx.input[at + vi::GENERATION] = 1.0;
    fx.input[at + vi::ALIVE] = 1.0;
    fx.input[at + vi::HEALTH] = 100.0;
    fx.input[at + vi::MAX_HEALTH] = 100.0;
    fx.input[at + vi::FLYING] = 1.0;
    fx.input[at + vi::FLIGHT_STATE] = flight::LANDED as f64;

    // Landed on the pad: thrust does not burn.
    // Both persistent thrust records emit action::STOP.
    step_single(&mut fx, id, 5);
    assert_eq!(fx.output[vo::ACTION], action::STOP as f64);

    // TakingOff: both thrust plumes start at their attach points at minScale (0.5).
    // Right wingtip nozzle is at node 0 ([10, 0, 0]) + offset ([0.7, -0.3, -2.4]).
    fx.input[at + vi::FLIGHT_STATE] = flight::TAKING_OFF as f64;
    step_single(&mut fx, id, 5);
    assert_eq!(fx.output[vo::ACTION], action::PLAY as f64);
    assert_eq!(fx.output[vo::STRIDE + vo::ACTION], action::PLAY as f64);
    assert!((fx.output[vo::POSITION] - 10.7).abs() < 1e-6);
    assert!((fx.output[vo::POSITION + 1] - (-0.3)).abs() < 1e-6);
    assert!((fx.output[vo::POSITION + 2] - (-2.4)).abs() < 1e-6);
    assert_eq!(fx.output[vo::SCALE], 0.5);

    // Flying at MaxSpeed (72): same plumes move and scale to maxScale (1.4).
    // Because the handles were marked PRESENT, action transitions from PLAY to MOVE.
    fx.input[at + vi::FLIGHT_STATE] = flight::FLYING as f64;
    fx.input[at + vi::VELOCITY + 2] = 72.0;
    step_single(&mut fx, id, 5);
    assert_eq!(fx.output[vo::ACTION], action::MOVE as f64);
    assert_eq!(fx.output[vo::SCALE], 1.4);

    // Landing keeps burning; Landed stops both thrust plumes.
    // Touchdown transitions from MOVE back to STOP on the next frame.
    fx.input[at + vi::FLIGHT_STATE] = flight::LANDING as f64;
    step_single(&mut fx, id, 5);
    assert_eq!(fx.output[vo::ACTION], action::MOVE as f64);
    fx.input[at + vi::FLIGHT_STATE] = flight::LANDED as f64;
    step_single(&mut fx, id, 5);
    assert_eq!(fx.output[vo::ACTION], action::STOP as f64);

    // Damage bands: at health 50, first damage entry plays at node 1 with inherit 0.75.
    // Second damage entry (start 0.3) remains stopped above 30 HP.
    let d0 = 2 * vo::STRIDE;
    let d1 = 3 * vo::STRIDE;
    fx.input[at + vi::HEALTH] = 50.0;
    step_single(&mut fx, id, 5);
    assert_eq!(fx.output[d0 + vo::ACTION], action::PLAY as f64);
    assert_eq!(
        &fx.output[d0 + vo::POSITION..d0 + vo::POSITION + 3],
        &[10.0, 1.0, 0.0]
    );
    assert_eq!(fx.output[d0 + vo::INHERIT], 0.75);
    assert_eq!(fx.output[d1 + vo::ACTION], action::STOP as f64);

    // At health 25, missing node 99 falls back to root (node 0) + offset [0, 0, 1].
    fx.input[at + vi::HEALTH] = 25.0;
    step_single(&mut fx, id, 5);
    assert_eq!(fx.output[d0 + vo::ACTION], action::MOVE as f64);
    assert_eq!(fx.output[d1 + vo::ACTION], action::PLAY as f64);
    assert_eq!(
        &fx.output[d1 + vo::POSITION..d1 + vo::POSITION + 3],
        &[10.0, 0.0, 1.0]
    );
    assert_eq!(fx.output[d1 + vo::SCALE], 0.4);

    // At health 10, first damage entry stops while second continues burning.
    fx.input[at + vi::HEALTH] = 10.0;
    step_single(&mut fx, id, 5);
    assert_eq!(fx.output[d0 + vo::ACTION], action::STOP as f64);
    assert_eq!(fx.output[d1 + vo::ACTION], action::MOVE as f64);
}

// Mirrors `tests/vehicle-effects.test.js`:
// "a destroyed hull stops its effects, throws its chunks and raises its death dust after the delay"
// "a dying walker pops its dying explosions at random nodes, raises its dust only while dying, and flies apart when it is gone"
// "a dead walker's pieces start where its fallen body lies, never under the ground"
#[test]
fn destruction_dying_walker_explosions_dust_and_fallen_body_carrier_follow_clock() {
    // Register a walker with a 4m tall body bone, 0.7s dust delay, and 0.4s dying explosions.
    let mut fx = VehicleEffects::default();
    let desc: Descriptor = serde_json::from_str(
        r#"{
            "nodes": 2,
            "bones": [0, 1],
            "body": 1,
            "bind": [
                {"position": [0.0, 0.0, 0.0], "quaternion": [0.0, 0.0, 0.0, 1.0], "scale": [1.0, 1.0, 1.0]},
                {"position": [0.0, 4.0, 0.0], "quaternion": [0.0, 0.0, 0.0, 1.0], "scale": [1.0, 1.0, 1.0]}
            ],
            "dust": {"entry": {"effect": 10, "node": 0, "offset": [0.0, 2.0, 0.0]}, "delay": 0.7},
            "dying": {"span": 1.0, "interval": 0.4, "effects": [20, 21], "final": []}
        }"#,
    )
    .unwrap();
    let id = fx.add(desc);
    set_node_translation(&mut fx, id, 0, 10.0, 0.0, 0.0);
    set_node_translation(&mut fx, id, 1, 10.0, 4.0, 0.0);

    // Standing at bind: carrier matrix equals the root world matrix (translation [10, 0, 0]).
    fx.carrier(id);
    assert_eq!(fx.input[vi::CARRIER_VALID], 1.0);
    let mut carrier = Matrix4::default();
    carrier
        .elements
        .copy_from_slice(&fx.input[vi::CARRIER..vi::CARRIER + 16]);
    let mut pt = Vector3::new(0.0, 4.0, 0.0);
    pt.apply_matrix4(&carrier);
    assert!(pt.distance_to(&Vector3::new(10.0, 4.0, 0.0)) < 1e-6);

    // Body falls to Y = 1.5 with a 0.2 rad roll: a point at bind (0, 4, 0) moves to (10, 1.5, 0).
    // This verifies `world * inverse(bind)` places debris chunks at the fallen body height.
    let mut fallen_m = Matrix4::default();
    let mut q = Quaternion::default();
    q.set_from_axis_angle(&Vector3::new(0.0, 0.0, 1.0), 0.2);
    fallen_m.compose(
        &Vector3::new(10.0, 1.5, 0.0),
        &q,
        &Vector3::new(1.0, 1.0, 1.0),
    );
    fx.nodes[16..32].copy_from_slice(&fallen_m.elements);
    fx.carrier(id);
    carrier
        .elements
        .copy_from_slice(&fx.input[vi::CARRIER..vi::CARRIER + 16]);
    let mut fallen_pt = Vector3::new(0.0, 4.0, 0.0);
    fallen_pt.apply_matrix4(&carrier);
    assert!(fallen_pt.distance_to(&Vector3::new(10.0, 1.5, 0.0)) < 1e-6);

    // Restore node 1 to (10, 1, 0) for the dying explosion node check.
    // Mark the hull alive for one baseline frame before starting the dying countdown.
    set_node_translation(&mut fx, id, 1, 10.0, 1.0, 0.0);
    fx.input[vi::PRESENT] = 1.0;
    fx.input[vi::GENERATION] = 1.0;
    fx.input[vi::ALIVE] = 1.0;
    step_single(&mut fx, id, 1);

    // Destroyed with dying = 1.0: stays whole while dying, no explosions at 0.7 remaining.
    fx.input[vi::ALIVE] = 0.0;
    fx.input[vi::DYING] = 1.0;
    step_single(&mut fx, id, 1);
    assert_eq!(fx.input[vi::BLOW_APART], 0.0);
    fx.input[vi::DYING] = 0.7;
    step_single(&mut fx, id, 1);
    assert_eq!(fx.frame[fr::PENDING], query::NONE as f64);

    // At dying = 0.55 (elapsed 0.45 > interval 0.4): requests random pick then matrix.
    // Picks [0.9, 0.99] select effect index 1 (21) and bone index 1 (at [10, 1, 0]).
    fx.input[vi::DYING] = 0.55;
    step_single(&mut fx, id, 1);
    assert_eq!(fx.frame[fr::PENDING], query::RANDOM as f64);
    fx.frame[fr::RANDOM] = 0.9;
    fx.frame[fr::RANDOM + 1] = 0.99;
    fx.resume();
    assert_eq!(fx.frame[fr::PENDING], query::MATRIX as f64);
    assert_eq!(fx.frame[fr::NODE], 1.0);
    fx.complete();
    let exp_row = 2 * vo::STRIDE;
    assert_eq!(fx.output[exp_row + vo::EFFECT], 21.0);
    assert_eq!(
        &fx.output[exp_row + vo::POSITION..exp_row + vo::POSITION + 3],
        &[10.0, 1.0, 0.0]
    );

    // At dying = 0.2 (elapsed 0.8 > delay 0.7 and > 2 * 0.4): raises dust and pops second explosion.
    // Dust is published before the random explosion request suspends the frame.
    fx.input[vi::DYING] = 0.2;
    step_single(&mut fx, id, 1);
    assert_eq!(fx.frame[fr::PENDING], query::RANDOM as f64);
    fx.frame[fr::RANDOM] = 0.1;
    fx.frame[fr::RANDOM + 1] = 0.1;
    fx.resume();
    fx.complete();
    let dust_row = vo::STRIDE;
    assert_eq!(fx.output[dust_row + vo::EFFECT], 10.0);
    assert_eq!(
        &fx.output[dust_row + vo::POSITION..dust_row + vo::POSITION + 3],
        &[10.0, 2.0, 0.0]
    );
    assert_eq!(fx.output[exp_row + vo::EFFECT], 20.0);

    // At dying = 0.0: flies apart with FALLEN = 1.0.
    // Both BLOW_APART and FALLEN flags are raised for chunk release.
    fx.input[vi::DYING] = 0.0;
    step_single(&mut fx, id, 1);
    assert_eq!(fx.input[vi::BLOW_APART], 1.0);
    assert_eq!(fx.input[vi::FALLEN], 1.0);
}

// Mirrors `tests/walker-dying.test.js`:
// "whoever draws it poses the death clip from the dying clock, holding its last frame"
// "the death clip takes over from the pose it was drawn in alive, not from a snap to its first frame"
#[test]
fn dying_walker_poses_death_clip_from_dying_clock_and_blends_from_last_alive_pose() {
    // 31-frame clip played at 2x speed (AnimationSpeed = 2.0) spans 0.5167s.
    const FRAMES: usize = 31;
    let span = FRAMES as f64 / 30.0 / 2.0;
    let x_axis = Vector3::new(1.0, 0.0, 0.0);

    // Build synthetic 31-frame death01 clip tipping bone 1 forward by 90 degrees.
    // Track 0 is the root bone (identity); track 1 is bone_body rotating around +X.
    let mut floats = Vec::with_capacity(2 * FRAMES * 7);
    for track in [0, 1] {
        for f in 0..FRAMES {
            let mut q = Quaternion::default();
            let angle = if track == 1 {
                (f as f64 / (FRAMES - 1) as f64) * std::f64::consts::FRAC_PI_2
            } else {
                0.0
            };
            q.set_from_axis_angle(&x_axis, angle);
            // Each keyframe packs [qx, qy, qz, qw, px, py, pz] as 7 f32s.
            floats.extend_from_slice(&[
                q.x as f32,
                q.y as f32,
                q.z as f32,
                q.w as f32,
                0.0,
                if track == 1 { 3.0 } else { 0.0 },
                0.0,
            ]);
        }
    }

    // Idle clip holds bone 1 leaning back by -0.4 rad to test smooth death take-over.
    // Without smooth death take-over, frame 0 of `death01` would snap bone 1 to 0.0 rad.
    let mut back_q = Quaternion::default();
    back_q.set_from_axis_angle(&x_axis, -0.4);
    let idle_floats = vec![
        // Track 0: identity root keyframe.
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
        // Track 1: backward-leaning body keyframe at Y = 3.0.
        back_q.x as f32,
        back_q.y as f32,
        back_q.z as f32,
        back_q.w as f32,
        0.0,
        3.0,
        0.0,
    ];

    // Register the two-bone walker rig with both `idle` and `death01` clips.
    // Both bones start with identity bind rotations and bone 1 sits at Y = 3.0.
    let mut poser = VehiclePoser::new();
    let death_handle = poser.load_clip(FRAMES, 30.0, false, vec![0, 1], floats);
    let idle_handle = poser.load_clip(1, 30.0, false, vec![0, 1], idle_floats);
    let rig = poser.add_rig(RigInput {
        source_height: 0.0,
        source_animal: false,
        bones: vec![
            // Root bone (index 0) at the origin.
            BoneInput {
                parent: -1,
                position: [0.0, 0.0, 0.0],
                quaternion: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, 1.0, 1.0],
                bind: [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            },
            // Body bone (index 1) 3 meters above the root bone.
            BoneInput {
                parent: 0,
                position: [0.0, 3.0, 0.0],
                quaternion: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, 1.0, 1.0],
                bind: [0.0, 0.0, 0.0, 1.0, 0.0, 3.0, 0.0],
            },
        ],
        yxz: true,
        has_pose: true,
        pose_root: 0,
        flying: false,
        creature: false,
        // Single-frame idle clip used when `mode == vmode::ALIVE`.
        idle: Some(ClipRef {
            handle: idle_handle,
            duration: 1.0 / 30.0,
        }),
        left: None,
        right: None,
        lift_left: None,
        lift_right: None,
        // 31-frame death01 clip used when `mode == vmode::DYING`.
        death: Some(ClipRef {
            handle: death_handle,
            duration: FRAMES as f64 / 30.0,
        }),
        takeoff: None,
        gear_root: -1,
        flight: None,
        legs: None,
        seats: Vec::new(),
        wheels: Vec::new(),
        nodes: Vec::new(),
        death_span: span,
    });

    // Lookup stream input and output offsets for slot 0 of the registered rig.
    let sin = poser.slot_input(rig, 0);
    let sout = poser.slot_output(rig, 0);

    // Helper that poses the walker in `mode` with remaining `dying` seconds and
    // returns the tilt angle (in radians) of `bone_body` (node index 2: root=0, bone0=1, bone1=2).
    let mut pose_and_tilt = |mode: u32, dying: f64| -> f64 {
        poser.input[vf::VERSION] = LAYOUT_VERSION as f64;
        poser.input[vf::ENTRIES] = 1.0;
        let at = vf::STRIDE;
        poser.input[at + ve::RIG] = rig as f64;
        poser.input[at + ve::INPUT] = sin as f64;
        poser.input[at + ve::OUTPUT] = sout as f64;
        poser.input[at + ve::MODE] = mode as f64;
        poser.input[at + ve::ALPHA] = 1.0;
        poser.input[at + ve::DT] = 1.0 / 60.0;
        poser.input[at + ve::FRAME_DT] = 1.0 / 60.0;
        poser.input[at + ve::DYING] = dying;
        poser.place();
        poser.pose();

        // Node 2 (bone_body) local matrix starts at `eo::STRIDE + 2 * vn::STRIDE`.
        let node_at =
            sout + bfe_presentation::layout::vehicle_entry_output::STRIDE + 2 * vn::STRIDE;
        let mut m = Matrix4::default();
        m.elements
            .copy_from_slice(&poser.stream_out[node_at..node_at + 16]);
        let mut p = Vector3::default();
        let mut q = Quaternion::default();
        let mut s = Vector3::default();
        m.decompose(&mut p, &mut q, &mut s);
        // Extract rotation magnitude in radians from the decomposed quaternion.
        2.0 * v8math::atan2(v8math::hypot(q.x, q.z), q.w)
    };

    // Drawn alive in its -0.4 rad backward lean, then destroyed at dying = span:
    // starts from its alive pose (0.4 rad magnitude), not snapped to 0.
    let alive_tilt = pose_and_tilt(vmode::ALIVE, 0.0);
    assert!((alive_tilt - 0.4).abs() < 1e-6);
    let start_death_tilt = pose_and_tilt(vmode::DYING, span);
    assert!((start_death_tilt - 0.4).abs() < 1e-6);

    // One-sixteenth into the span: smoothly blending from the alive lean into the clip.
    let mid_blend_tilt = pose_and_tilt(vmode::DYING, span - span / 16.0);
    assert!(mid_blend_tilt < 0.4 && mid_blend_tilt > 0.0);

    // Halfway through the span: past the 1/8th blend window, matches frame 15.5 of 30.
    // The expected tilt is halfway to 90 degrees (+PI/4 radians).
    let half_tilt = pose_and_tilt(vmode::DYING, span / 2.0);
    let expected_half = (FRAMES as f64 / 2.0 / (FRAMES - 1) as f64) * std::f64::consts::FRAC_PI_2;
    assert!((half_tilt - expected_half).abs() < 0.01);

    // Near the end of the span: holds the final 90-degree tipped pose.
    // Because frame 30 is reached at half the span (AnimationSpeed = 2), the clip holds.
    let end_tilt = pose_and_tilt(vmode::DYING, 0.01);
    assert!((end_tilt - std::f64::consts::FRAC_PI_2).abs() < 0.04);
}
