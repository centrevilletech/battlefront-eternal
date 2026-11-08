//! Synthetic unit-foot plant and cold cycle-height tests ported from
//! `tests/visual-feet.test.js`.
//!
//! These cases exercise `bfe_presentation::feet::{heights, Content, Feet}`
//! directly over synthetic JSON descriptors without requiring game assets.
//!
//! Coverage includes:
//! - Falling foot plant detection, primitive local-turn composition with
//!   displayed bone matrices, and body generation resets.
//! - Cold cycle-height sampling (`heights`) across multi-bone chains with
//!   non-unit parent scales, bind offsets, out-of-chain tracks, and `f32`
//!   rounding.
//! - Registration clamping of oversized authored foot classes to `PER_BODY`.

use bfe_presentation::feet::{heights, Content, Feet};
use bfe_presentation::layout::{
    foot_event as o, foot_frame as f, foot_input as i, foot_limits as lim,
};
use three::{Matrix4, Quaternion, Vector3};

/// Deserializes a synthetic `Content` descriptor from JSON.
fn parse_content(json: &str) -> Content {
    serde_json::from_str(json).expect("valid synthetic foot content json")
}

/// Steps a single foot at detector index `start` at height `y` with the given
/// body `generation` and `matrix`, returning the emitted plant event count.
fn step_single_foot(
    feet: &mut Feet,
    start: usize,
    y: f64,
    generation: f64,
    matrix: &Matrix4,
) -> f64 {
    feet.input[f::DT] = 0.1;
    let at = f::STRIDE + start * i::STRIDE;
    feet.input[at + i::PRESENT] = 1.0;
    feet.input[at + i::GENERATION] = generation;
    feet.input[at + i::ROOT_Y] = 0.25;
    feet.input[at + i::MATRIX..at + i::MATRIX + 16].copy_from_slice(&matrix.elements);
    feet.input[at + i::MATRIX + 13] = y;
    feet.frame();
    feet.output[start * o::STRIDE + o::COUNT]
}

// Mirrors `tests/visual-feet.test.js`:
// "a falling foot plants once, preserves primitive turns, and resets on a new generation"
//
// When `detection` is false and `length` is 0, `Foot` uses fallback descent
// detection: it arms while descending and emits one plant event when the foot
// stops descending, publishing `[point.x, ROOT_Y, point.z]` and the composed
// bone-and-primitive rotation quaternion.
#[test]
fn falling_foot_plants_once_preserves_primitive_turns_and_resets_on_new_generation() {
    let mut feet = Feet::default();
    let content = parse_content(
        r#"{
            "bones": [],
            "clips": [],
            "root": -1,
            "length": 0.0,
            "feet": [{
                "bone": -1,
                "point": [0.0, 0.0, 0.0],
                "turn": [0.0, 0.0, 0.7071067811865475, 0.7071067811865476]
            }],
            "detection": false,
            "threshold": 0.15,
            "hydraulic": 1.0,
            "centred": true,
            "rate": 1.0
        }"#,
    );
    let start = feet.add(content);
    assert_eq!(start, 0);

    // Build a world matrix with a quarter-turn about Y and scale (2, 2, 2).
    // Decomposition must strip the uniform scale before multiplying the
    // authored primitive turn.
    let mut bone_turn = Quaternion::default();
    bone_turn.set_from_axis_angle(&Vector3::new(0.0, 1.0, 0.0), std::f64::consts::FRAC_PI_2);
    let mut matrix = Matrix4::default();
    matrix.compose(
        &Vector3::new(3.0, 2.0, -4.0),
        &bone_turn,
        &Vector3::new(2.0, 2.0, 2.0),
    );

    // Descending from 2.0 to 1.0: still falling, no plant event yet.
    assert_eq!(step_single_foot(&mut feet, start, 2.0, 1.0, &matrix), 0.0);
    assert_eq!(step_single_foot(&mut feet, start, 1.0, 1.0, &matrix), 0.0);

    // Bottoms out at 1.0: emits one plant event at (3, 0.25, -4) with combined turn.
    assert_eq!(step_single_foot(&mut feet, start, 1.0, 1.0, &matrix), 1.0);
    assert_eq!(
        &feet.output[o::POSITION..o::POSITION + 3],
        &[3.0, 0.25, -4.0]
    );
    assert_eq!(feet.output[o::HAS_TURN], 1.0);

    let prim_turn = Quaternion::new(0.0, 0.0, 0.7071067811865475, 0.7071067811865476);
    let mut expected_turn = bone_turn;
    expected_turn.multiply(&prim_turn);
    assert!((feet.output[o::TURN] - expected_turn.x).abs() < 1e-6);
    assert!((feet.output[o::TURN + 1] - expected_turn.y).abs() < 1e-6);
    assert!((feet.output[o::TURN + 2] - expected_turn.z).abs() < 1e-6);
    assert!((feet.output[o::TURN + 3] - expected_turn.w).abs() < 1e-6);

    // Staying at 1.0 or rising does not plant again.
    assert_eq!(step_single_foot(&mut feet, start, 1.0, 1.0, &matrix), 0.0);
    assert_eq!(step_single_foot(&mut feet, start, 1.5, 1.0, &matrix), 0.0);
    assert_eq!(step_single_foot(&mut feet, start, 1.5, 1.0, &matrix), 0.0);

    // A new generation resets descent state and plants on the next bottoming out.
    assert_eq!(step_single_foot(&mut feet, start, 2.0, 2.0, &matrix), 0.0);
    assert_eq!(step_single_foot(&mut feet, start, 1.0, 2.0, &matrix), 0.0);
    assert_eq!(step_single_foot(&mut feet, start, 1.0, 2.0, &matrix), 1.0);
}

// Mirrors `tests/visual-feet.test.js`:
// "cold cycle samples retain scales, bind positions and f32 rounding"
//
// Two-bone chain: bone 0 scales Y by 2, bone 1 has bind Y = 0.5 and foot
// point Y = 0.25. Track 1 (bone 2) is outside the foot's ancestor chain and
// must be ignored when sampling the two 3-frame walk clips.
#[test]
fn cold_cycle_samples_retain_scales_bind_positions_and_f32_rounding() {
    let content = parse_content(
        r#"{
            "bones": [
                {"parent": -1, "scale": [1.0, 2.0, 1.0], "position": [0.0, 0.0, 0.0], "quaternion": [0.0, 0.0, 0.0, 1.0]},
                {"parent": 0, "scale": [1.0, 1.0, 1.0], "position": [0.0, 0.5, 0.0], "quaternion": [0.0, 0.0, 0.0, 1.0]},
                {"parent": -1, "scale": [1.0, 1.0, 1.0], "position": [0.0, 0.0, 0.0], "quaternion": [0.0, 0.0, 0.0, 1.0]}
            ],
            "root": -1,
            "length": 4.0,
            "clips": [
                {
                    "frames": 3,
                    "fps": 1.0,
                    "rootMotion": false,
                    "tracks": [0, 2],
                    "data": [
                        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
                        0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0,
                        0.0, 0.0, 0.0, 1.0, 0.0, 2.0, 0.0,
                        0.0, 0.0, 0.0, 1.0, 0.0, 99.0, 0.0,
                        0.0, 0.0, 0.0, 1.0, 0.0, 99.0, 0.0,
                        0.0, 0.0, 0.0, 1.0, 0.0, 99.0, 0.0
                    ]
                },
                {
                    "frames": 3,
                    "fps": 1.0,
                    "rootMotion": false,
                    "tracks": [0],
                    "data": [
                        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
                        0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0,
                        0.0, 0.0, 0.0, 1.0, 0.0, 2.0, 0.0
                    ]
                }
            ],
            "feet": [{"bone": 1, "point": [0.0, 0.25, 0.0], "turn": null}],
            "detection": true,
            "threshold": 0.15,
            "hydraulic": 0.0,
            "centred": false,
            "rate": 1.0
        }"#,
    );

    // World Y = bone0_y + 2.0 * (0.5 + 0.25) = bone0_y + 1.5 across the 2
    // non-repeated frames per clip.
    assert_eq!(heights(&content), vec![vec![1.5, 2.5, 1.5, 2.5]]);
}

// Mirrors `tests/visual-feet.test.js`:
// "an oversized authored foot class is bounded at registration"
//
// A class declaring 4096 feet is truncated to `lim::PER_BODY` (16) at
// registration so subsequent classes start immediately after and remain
// isolated.
#[test]
fn oversized_authored_foot_class_is_bounded_at_registration() {
    let mut feet = Feet::default();
    let foot_entries: Vec<&str> = (0..4096)
        .map(|_| r#"{"bone": -1, "point": [0.0, 0.0, 0.0], "turn": null}"#)
        .collect();
    let oversized_json = format!(
        r#"{{"bones": [], "clips": [], "root": -1, "length": 0.0, "feet": [{}], "detection": false, "threshold": 0.15, "hydraulic": 0.0, "centred": false, "rate": 1.0}}"#,
        foot_entries.join(",")
    );
    let single_json = r#"{"bones": [], "clips": [], "root": -1, "length": 0.0, "feet": [{"bone": -1, "point": [0.0, 0.0, 0.0], "turn": null}], "detection": false, "threshold": 0.15, "hydraulic": 0.0, "centred": false, "rate": 1.0}"#;

    let first = feet.add(parse_content(&oversized_json));
    let second = feet.add(parse_content(single_json));
    assert_eq!(first, 0);
    assert_eq!(second, lim::PER_BODY);
    assert_eq!(feet.output.len(), (lim::PER_BODY + 1) * o::STRIDE);

    // Stepping the second class works cleanly without being overwritten by the first.
    let matrix = Matrix4::default();
    assert_eq!(step_single_foot(&mut feet, second, 2.0, 1.0, &matrix), 0.0);
    assert_eq!(step_single_foot(&mut feet, second, 1.0, 1.0, &matrix), 0.0);
    assert_eq!(step_single_foot(&mut feet, second, 1.0, 1.0, &matrix), 1.0);
}
