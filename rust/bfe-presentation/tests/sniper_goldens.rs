//! Golden parity tests for sniper flight, vapour, headshot flourishes, and
//! moving first-person eye muzzles ported from `tests/sniper-presentation.test.js`.
//!
//! These cases exercise `bfe_presentation::sniper::Sniper` directly against
//! the frozen golden buffers in `tests/fixtures/presentation/sniper-goldens.json`
//! and `tests/fixtures/presentation/sniper-eye-goldens.json`.
//!
//! Coverage includes:
//! - Full third-person flight, deferred impact seed reservation, duplicate
//!   headshot suppression (`distance < 1.5m`), surface sparks, embers, bursts,
//!   light requests (`ADD` and `FLASH`), and LCG RNG state (`sniper-goldens.json`).
//! - First-person eye-owned shot (`distance(start, eye) < 2.5m`) tracking a
//!   moving camera and barrel, retaining camera-local coordinates across a
//!   frame with no muzzle handle (`index == 2`), and keeping smoke anchored to
//!   the fired point (`sniper-eye-goldens.json`).
//! - Bit-for-bit float32 equality of the published `streaks` and `flares`
//!   instance arrays and exact 32-bit unsigned LCG seed progression across all
//!   recorded frame timestamps.

use bfe_presentation::sniper::Sniper;
use bfe_presentation::sniper_layout::{
    sniper_command as c, sniper_counts as h, sniper_event as e, sniper_event_kind as ek,
    sniper_flare as sf, sniper_frame as f, sniper_kind as k, sniper_streak as ss,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use three::{Euler, EulerOrder, Matrix4, Quaternion, Vector3};

/// One frame entry in `sniper-goldens.json` or `sniper-eye-goldens.json`.
/// Captures the exact float32 GPU streak/flare instance buffers, light commands,
/// deferred impact callback seeds (`plays`), and post-frame LCG seed.
#[derive(Deserialize)]
struct GoldenFrame {
    t: f64,
    streaks: Vec<f32>,
    flares: Vec<f32>,
    lights: Vec<serde_json::Value>,
    #[serde(default)]
    plays: Vec<f64>,
    seed: u32,
}

/// Resolves a fixture path under `tests/fixtures/presentation/`.
/// Allows running `cargo test -p bfe-presentation --test sniper_goldens` from
/// either the workspace root or the crate directory.
fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/presentation")
        .join(name)
}

/// Steps the 32-bit LCG (`Math.imul(seed, 1664525) + 1013904223 >>> 0`) and
/// returns the normalized `[0, 1)` sample.
/// Matches `SniperTrace`'s deterministic visual effect generator in `src/sniper-trace.js`.
fn next_random(seed: &mut u32) -> f64 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    *seed as f64 / 4294967296.0
}

/// Compares two `serde_json::Value` trees while treating integer and floating
/// JSON numbers with identical `f64` values as equal (`1` vs `1.0`).
/// JSON serialization in Node omits `.0` on whole numbers, whereas `serde_json::json!`
/// preserves `f64` typing on `Number`.
fn json_eq(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    match (a, b) {
        (serde_json::Value::Number(na), serde_json::Value::Number(nb)) => {
            na.as_f64() == nb.as_f64()
        }
        (serde_json::Value::Array(aa), serde_json::Value::Array(ab)) => {
            aa.len() == ab.len() && aa.iter().zip(ab.iter()).all(|(x, y)| json_eq(x, y))
        }
        (serde_json::Value::Object(oa), serde_json::Value::Object(ob)) => {
            oa.len() == ob.len()
                && oa
                    .iter()
                    .all(|(k, va)| ob.get(k).is_some_and(|vb| json_eq(va, vb)))
        }
        _ => a == b,
    }
}

/// Asserts that two slices of `serde_json::Value` match under `json_eq`.
/// Reports the exact element index and both JSON values on any mismatch.
fn assert_json_slice_eq(left: &[serde_json::Value], right: &[serde_json::Value], label: &str) {
    assert_eq!(left.len(), right.len(), "{label}: length mismatch");
    for (i, (a, b)) in left.iter().zip(right.iter()).enumerate() {
        assert!(json_eq(a, b), "{label}[{i}]: left={a:?} right={b:?}");
    }
}

/// Drains `sniper.events` for the current phase into `lights` and `plays`,
/// matching `SniperTrace.apply()` in `src/sniper-trace.js`.
/// Handles `RELEASE` (cancelling a callback token), `CALLBACK` (running a
/// deferred impact effect with its pre-drawn seed), `LIGHT` (`add`), and
/// `FLASH` (`flash`).
fn drain_events(
    sniper: &Sniper,
    callbacks: &mut HashMap<u32, bool>,
    lights: &mut Vec<serde_json::Value>,
    plays: &mut Vec<f64>,
) {
    let count = sniper.counts[h::EVENTS] as usize;
    for i in 0..count {
        let at = i * e::STRIDE;
        let kind = sniper.events[at + e::KIND] as u32;
        // Release events retire deferred impact tokens without firing their callback.
        if kind == ek::RELEASE {
            let token = sniper.events[at + e::TOKEN] as u32;
            callbacks.remove(&token);
            continue;
        }
        // Callback events invoke the deferred impact handler once when its token is live.
        if kind == ek::CALLBACK {
            let token = sniper.events[at + e::TOKEN] as u32;
            if callbacks.remove(&token).is_some() {
                let raw_seed = sniper.events[at + e::SEED];
                if raw_seed >= 0.0 {
                    plays.push(raw_seed / 4294967296.0);
                }
            }
            continue;
        }
        // Format light position and RGB array to compare against JSON goldens.
        let pos = serde_json::json!([
            sniper.events[at + e::POSITION],
            sniper.events[at + e::POSITION + 1],
            sniper.events[at + e::POSITION + 2],
        ]);
        let rgb = serde_json::json!([
            sniper.events[at + e::COLOR],
            sniper.events[at + e::COLOR + 1],
            sniper.events[at + e::COLOR + 2],
        ]);
        let reach = sniper.events[at + e::REACH];
        // Distinguish one-frame `add` lights (`ek::LIGHT`) from timed `flash` lights.
        if kind == ek::LIGHT {
            lights.push(serde_json::json!(["add", pos, rgb, reach]));
        } else {
            let time = sniper.events[at + e::TIME];
            lights.push(serde_json::json!(["flash", pos, rgb, reach, time]));
        }
    }
}

// Mirrors `tests/sniper-presentation.test.js`:
// "native sniper phases publish the frozen flight and impact buffers"
#[test]
fn native_sniper_phases_publish_frozen_flight_and_impact_buffers() {
    let raw = std::fs::read_to_string(fixture_path("sniper-goldens.json")).unwrap();
    let golden: Vec<GoldenFrame> = serde_json::from_str(&raw).unwrap();

    // Start from the post-constructor LCG seed recorded in the JS test.
    // The JS constructor draws one initial seed during `new SniperTrace()`.
    let mut seed: u32 = 1278274502;
    let mut sniper = Sniper::default();
    let mut callbacks: HashMap<u32, bool> = HashMap::new();
    let mut next_token: u32 = 0;
    let mut plays: Vec<f64> = Vec::new();

    // Synthetic shot parameters: start (2, 1.6, 3), direction normalize(0.2, 0.1, 1), reach 300.
    // The bolt terminates 40m downrange at `head = start + 40 * dir`.
    let start = Vector3::new(2.0, 1.6, 3.0);
    let mut dir = Vector3::new(0.2, 0.1, 1.0);
    dir.normalize();
    let mut head = start;
    head.add_scaled_vector(&dir, 40.0);

    for expected in &golden {
        let t = expected.t;
        let mut lights: Vec<serde_json::Value> = Vec::new();
        let mut cmd_count = 0usize;

        // Clear command slots and write `TOUCH` while t < 10.1.
        // On frames after t == 10.0, `ENDED` is set to 1.0 to report impact at `head`.
        if t < 10.1 {
            let at = f::STRIDE + cmd_count * c::STRIDE;
            sniper.input[at..at + c::STRIDE].fill(0.0);
            sniper.input[at + c::KIND] = k::TOUCH as f64;
            sniper.input[at + c::SOURCE] = 1.0;
            sniper.input[at + c::START..at + c::START + 3]
                .copy_from_slice(&[start.x, start.y, start.z]);
            sniper.input[at + c::HEAD..at + c::HEAD + 3].copy_from_slice(&[head.x, head.y, head.z]);
            sniper.input[at + c::DIRECTION..at + c::DIRECTION + 3]
                .copy_from_slice(&[dir.x, dir.y, dir.z]);
            sniper.input[at + c::ORIGIN..at + c::ORIGIN + 3]
                .copy_from_slice(&[start.x, start.y, start.z]);
            sniper.input[at + c::COLOR..at + c::COLOR + 3].copy_from_slice(&[224.0, 224.0, 160.0]);
            sniper.input[at + c::REACH] = 2000.0 * 0.15;
            sniper.input[at + c::ENDED] = if t > 10.0 { 1.0 } else { 0.0 };
            cmd_count += 1;
        }

        // At t == 10.04, queue one deferred impact callback and two nearby headshots.
        // The second headshot is only 0.173m away (< 1.5m deduplication radius) and is ignored.
        if t == 10.04 {
            next_token += 1;
            callbacks.insert(next_token, true);
            let at = f::STRIDE + cmd_count * c::STRIDE;
            sniper.input[at..at + c::STRIDE].fill(0.0);
            sniper.input[at + c::KIND] = k::DEFER as f64;
            sniper.input[at + c::TOKEN] = next_token as f64;
            sniper.input[at + c::RECORD] = -1.0;
            sniper.input[at + c::SEED_EFFECT] = 1.0;
            sniper.input[at + c::POINT..at + c::POINT + 3]
                .copy_from_slice(&[head.x, head.y, head.z]);
            cmd_count += 1;

            // First headshot report at `head`, followed by a duplicate 0.1m away.
            // Only the first headshot spawns a crimson mist burst and flash.
            for offset in [0.0, 0.1] {
                let at = f::STRIDE + cmd_count * c::STRIDE;
                sniper.input[at..at + c::STRIDE].fill(0.0);
                sniper.input[at + c::KIND] = k::HEADSHOT as f64;
                sniper.input[at + c::POINT..at + c::POINT + 3].copy_from_slice(&[
                    head.x + offset,
                    head.y + offset,
                    head.z + offset,
                ]);
                cmd_count += 1;
            }
        }

        // Run `collect` when commands or active records exist, then run `frame`.
        // Camera is placed at `(0, 1e6, 0)` so this shot is treated as third-person (`!own`).
        if cmd_count > 0 || sniper.counts[h::RECORDS] > 0 {
            sniper.input[f::TIME] = t;
            sniper.input[f::HAS_CAMERA] = 0.0;
            sniper.input[f::HAS_MUZZLE] = 0.0;
            sniper.input[f::EYE..f::EYE + 3].copy_from_slice(&[0.0, 1e6, 0.0]);
            sniper.input[f::BEGIN] = 1.0;
            sniper.input[f::COMMANDS] = cmd_count as f64;
            sniper.collect(&mut || next_random(&mut seed));
            drain_events(&sniper, &mut callbacks, &mut lights, &mut plays);
        }

        // Advance the visual frame phase to emit streak quads, flares, and light events.
        sniper.input[f::TIME] = t;
        sniper.input[f::HAS_CAMERA] = 0.0;
        sniper.input[f::HAS_MUZZLE] = 0.0;
        sniper.input[f::EYE..f::EYE + 3].copy_from_slice(&[0.0, 1e6, 0.0]);
        sniper.frame(&mut || next_random(&mut seed));
        drain_events(&sniper, &mut callbacks, &mut lights, &mut plays);

        // Verify bit-for-bit streak floats, flare floats, light events, plays, and LCG seed.
        let streak_len = sniper.counts[h::STREAKS] as usize * ss::STRIDE;
        let flare_len = sniper.counts[h::FLARES] as usize * sf::STRIDE;
        assert_eq!(
            &sniper.streaks[..streak_len],
            &expected.streaks[..],
            "streaks at {t}"
        );
        assert_eq!(
            &sniper.flares[..flare_len],
            &expected.flares[..],
            "flares at {t}"
        );
        assert_json_slice_eq(&lights, &expected.lights, &format!("lights at {t}"));
        assert_eq!(plays, expected.plays, "plays at {t}");
        assert_eq!(seed, expected.seed, "seed at {t}");
    }
}

// Mirrors `tests/sniper-presentation.test.js`:
// "native eye-view traces retain the frozen moving muzzle buffers"
#[test]
fn native_eye_view_traces_retain_frozen_moving_muzzle_buffers() {
    let raw = std::fs::read_to_string(fixture_path("sniper-eye-goldens.json")).unwrap();
    let golden: Vec<GoldenFrame> = serde_json::from_str(&raw).unwrap();

    // Start from the post-constructor LCG seed recorded in the JS eye-view test.
    let mut seed: u32 = 717482109;
    let mut sniper = Sniper::default();
    let mut callbacks: HashMap<u32, bool> = HashMap::new();
    let mut plays: Vec<f64> = Vec::new();

    // Eye-view shot starts at `(0.05, 1.5, 1.0)` within 2.5m of the camera eye `(0, 1.6, 0)`.
    // Because `distance(start, eye) < 2.5m`, `Sniper` marks the record `own = true`.
    let start = Vector3::new(0.05, 1.5, 1.0);
    let dir = Vector3::new(0.0, 0.0, 1.0);
    let head = Vector3::new(0.0, 1.5, 40.0);

    for (index, expected) in golden.iter().enumerate() {
        // Compute camera world and inverse matrices matching `THREE.PerspectiveCamera`.
        // The camera translates and yaws slightly on every frame while the bolt is live.
        let cam_pos = Vector3::new(index as f64 * 0.07, 1.6, index as f64 * 0.03);
        let mut cam_q = Quaternion::default();
        cam_q.set_from_euler(&Euler::new(0.0, index as f64 * 0.01, 0.0, EulerOrder::XYZ));
        let mut cam_world = Matrix4::default();
        cam_world.compose(&cam_pos, &cam_q, &Vector3::new(1.0, 1.0, 1.0));
        let mut cam_inv = cam_world;
        cam_inv.invert();

        // Transform the local muzzle offset `(0.05, -0.1, 0.8)` by `camera.matrixWorld`.
        // Frame index 2 omits the muzzle handle (`has_muzzle = false`) to test camera-local retention.
        let mut muzzle = Vector3::new(0.05, -0.1, 0.8);
        muzzle.apply_matrix4(&cam_world);
        let has_muzzle = index != 2;

        let t = expected.t;
        let mut lights: Vec<serde_json::Value> = Vec::new();
        let mut cmd_count = 0usize;

        // Touch the bolt record while `t < 10.1`, marking `ENDED = 1.0` once `t > 10.0`.
        // Blue CIS sniper beam colour `[30, 120, 255]` with reach `300m`.
        if t < 10.1 {
            let at = f::STRIDE;
            sniper.input[at..at + c::STRIDE].fill(0.0);
            sniper.input[at + c::KIND] = k::TOUCH as f64;
            sniper.input[at + c::SOURCE] = 1.0;
            sniper.input[at + c::START..at + c::START + 3]
                .copy_from_slice(&[start.x, start.y, start.z]);
            sniper.input[at + c::HEAD..at + c::HEAD + 3].copy_from_slice(&[head.x, head.y, head.z]);
            sniper.input[at + c::DIRECTION..at + c::DIRECTION + 3]
                .copy_from_slice(&[dir.x, dir.y, dir.z]);
            sniper.input[at + c::ORIGIN..at + c::ORIGIN + 3]
                .copy_from_slice(&[start.x, start.y, start.z]);
            sniper.input[at + c::COLOR..at + c::COLOR + 3].copy_from_slice(&[30.0, 120.0, 255.0]);
            sniper.input[at + c::REACH] = 2000.0 * 0.15;
            sniper.input[at + c::ENDED] = if t > 10.0 { 1.0 } else { 0.0 };
            cmd_count = 1;
        }

        // Helper closure to populate the camera and muzzle facts on `sniper.input`.
        // Copies both `CAMERA` (world matrix) and `INVERSE` (view matrix) when active.
        let write_facts = |s: &mut Sniper| {
            let active = cmd_count > 0 || s.counts[h::RECORDS] > 0;
            s.input[f::TIME] = t;
            s.input[f::HAS_CAMERA] = if active { 1.0 } else { 0.0 };
            s.input[f::HAS_MUZZLE] = if active && has_muzzle { 1.0 } else { 0.0 };
            if active {
                s.input[f::EYE..f::EYE + 3].copy_from_slice(&cam_world.elements[12..15]);
                s.input[f::CAMERA..f::CAMERA + 16].copy_from_slice(&cam_world.elements);
                s.input[f::INVERSE..f::INVERSE + 16].copy_from_slice(&cam_inv.elements);
            } else {
                s.input[f::EYE..f::EYE + 3].copy_from_slice(&[0.0, 1e6, 0.0]);
            }
            if active && has_muzzle {
                s.input[f::MUZZLE..f::MUZZLE + 3].copy_from_slice(&[muzzle.x, muzzle.y, muzzle.z]);
            }
        };

        // Run `collect` when commands or active records exist, then run `frame`.
        // Even on frame index 2 (`has_muzzle == false`), `Sniper` projects the retained
        // camera-local muzzle offset through the updated `CAMERA` matrix.
        if cmd_count > 0 || sniper.counts[h::RECORDS] > 0 {
            write_facts(&mut sniper);
            sniper.input[f::BEGIN] = 1.0;
            sniper.input[f::COMMANDS] = cmd_count as f64;
            sniper.collect(&mut || next_random(&mut seed));
            drain_events(&sniper, &mut callbacks, &mut lights, &mut plays);
        }

        write_facts(&mut sniper);
        sniper.frame(&mut || next_random(&mut seed));
        drain_events(&sniper, &mut callbacks, &mut lights, &mut plays);

        // Verify bit-for-bit eye-view streaks, flares, lights, and seed.
        let streak_len = sniper.counts[h::STREAKS] as usize * ss::STRIDE;
        let flare_len = sniper.counts[h::FLARES] as usize * sf::STRIDE;
        assert_eq!(
            &sniper.streaks[..streak_len],
            &expected.streaks[..],
            "eye streaks at {t}"
        );
        assert_eq!(
            &sniper.flares[..flare_len],
            &expected.flares[..],
            "eye flares at {t}"
        );
        assert_json_slice_eq(&lights, &expected.lights, &format!("eye lights at {t}"));
        assert_eq!(seed, expected.seed, "eye seed at {t}");
    }
}
