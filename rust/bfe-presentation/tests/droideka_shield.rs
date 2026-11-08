//! Synthetic droideka shield bubble presentation tests ported from
//! `tests/droideka-shield.test.js`.
//!
//! These cases exercise `bfe_presentation::unit_effects::shield::Shield`
//! across Classic activation growth, Enhanced interrupted pop transitions,
//! localized impact absorption, undirected flashes, and body pose/mode updates.
//!
//! Coverage includes:
//! - Classic activation growth, 1.06x peak overshoot at 94% of `FadeIn`,
//!   settling to full radius, and linear `FadeOut` collapse.
//! - Enhanced cubic expansion continuity across interrupted transitions.
//! - Localized 6-slot impact ring buffer and undirected flashes when the
//!   damage source is unknown.
//! - Live mode switching, yaw-rotated `ShieldOffset`, and hiding when rolled
//!   or dead.

use bfe_presentation::unit_effects::shield::Shield;
use bfe_presentation::visual_effects_layout::{shield_command as cmd, shield_frame as f};

/// Builds a baseline shield fixture matching `fixture(mode)` in
/// `tests/droideka-shield.test.js` (`Radius = 1.25`, `Height = 1.25`,
/// `FadeIn = 0.15`, `FadeOut = 0.15`, `ShieldOffset = [0, 1, 0.125]`,
/// `max = 4000`).
fn fixture(enhanced: bool) -> Shield {
    let mut s = Shield::default();
    s.input[f::OFFSET] = 0.0;
    s.input[f::OFFSET + 1] = 1.0;
    s.input[f::OFFSET + 2] = 0.125;
    s.input[f::RADIUS] = 1.25;
    s.input[f::HEIGHT] = 1.25;
    s.input[f::MAX] = 4000.0;
    s.input[f::VALUE] = 4000.0;
    s.input[f::FADE_IN] = 0.15;
    s.input[f::FADE_OUT] = 0.15;
    s.input[f::ENHANCED] = if enhanced { 1.0 } else { 0.0 };
    s
}

/// Advances the shield display state by `dt` seconds.
fn step(s: &mut Shield, dt: f64) {
    s.input[f::COMMAND] = cmd::FRAME as f64;
    s.input[f::DT] = dt;
    s.frame();
}

/// Records an absorbed shield hit with an optional world-space source point.
fn hit(s: &mut Shield, amount: f64, source: Option<[f64; 3]>) {
    s.input[f::COMMAND] = cmd::HIT as f64;
    s.input[f::AMOUNT] = amount;
    if let Some([x, y, z]) = source {
        s.input[f::HAS_SOURCE] = 1.0;
        s.input[f::SOURCE] = x;
        s.input[f::SOURCE + 1] = y;
        s.input[f::SOURCE + 2] = z;
    } else {
        s.input[f::HAS_SOURCE] = 0.0;
    }
    s.frame();
}

// Mirrors `tests/droideka-shield.test.js`:
// "Classic uses native activation growth, overshoot, dimensions and collapse timing"
#[test]
fn classic_uses_native_activation_growth_overshoot_dimensions_and_collapse_timing() {
    let mut s = fixture(false);
    s.input[f::ON] = 1.0;

    // 47% through FadeIn (0.15s) reaches 53% of the 1.06 overshoot curve.
    step(&mut s, 0.15 * 0.47);
    assert!((s.input[f::SCALE] / 1.25 - 0.53).abs() < 1e-6);

    // 94% through FadeIn reaches the 1.06 peak overshoot.
    step(&mut s, 0.15 * 0.47);
    assert!((s.input[f::SCALE] / 1.25 - 1.06).abs() < 1e-6);

    // 100% through FadeIn settles back to full radius (1.25) at ShieldOffset.
    step(&mut s, 0.15 * 0.06);
    assert!((s.input[f::SCALE] - 1.25).abs() < 1e-6);
    assert_eq!(&s.input[f::CENTER..f::CENTER + 3], &[0.0, 1.0, 0.125]);

    // Turning off collapses linearly over FadeOut (0.15s) and hides at zero.
    s.input[f::ON] = 0.0;
    step(&mut s, 0.075);
    assert!((s.input[f::SCALE] - 0.625).abs() < 1e-6);
    step(&mut s, 0.075);
    assert_eq!(s.input[f::VISIBLE], 0.0);
}

// Mirrors `tests/droideka-shield.test.js`:
// "Enhanced pop remains continuous through interrupted activation and resets after collapse"
#[test]
fn enhanced_pop_remains_continuous_through_interrupted_activation_and_resets_after_collapse() {
    let mut s = fixture(true);
    s.input[f::ON] = 1.0;
    step(&mut s, 0.07);
    let mut scale = s.input[f::SCALE];

    // Reversing to off at dt = 0 preserves the current shell scale without a jump.
    s.input[f::ON] = 0.0;
    step(&mut s, 0.0);
    assert!((s.input[f::SCALE] - scale).abs() < 1e-8);

    // Partially collapsing and turning back on at dt = 0 also remains continuous.
    step(&mut s, 0.03);
    scale = s.input[f::SCALE];
    s.input[f::ON] = 1.0;
    step(&mut s, 0.0);
    assert!((s.input[f::SCALE] - scale).abs() < 1e-8);

    // Full activation settles at 1.25; full collapse resets pop to the minimum shell.
    step(&mut s, 1.0);
    assert_eq!(s.input[f::SCALE], 1.25);
    s.input[f::ON] = 0.0;
    step(&mut s, 1.0);
    s.input[f::ON] = 1.0;
    step(&mut s, 0.0);
    assert!(s.input[f::SCALE] < 0.01);
}

// Mirrors `tests/droideka-shield.test.js`:
// "real absorption forwards damage source and keeps a bounded localized impact pool"
#[test]
fn real_absorption_forwards_damage_source_and_keeps_bounded_localized_impact_pool() {
    let mut s = fixture(true);
    s.input[f::ON] = 1.0;
    step(&mut s, 0.3);

    // Hit from +X relative to center (0, 1, 0.125) normalizes to (+1, 0, 0).
    let from = [5.0, 1.0, 0.125];
    s.input[f::VALUE] = 3900.0;
    hit(&mut s, 100.0, Some(from));
    assert_eq!(s.input[f::VALUE], 3900.0);
    assert!(s.impacts[0] > 0.999);
    assert!(s.powers[0] > 0.0);

    // Thirty additional hits wrap in place within the fixed 6-hit (24-float) pool.
    let ptr = s.impacts.as_ptr();
    for _ in 0..30 {
        hit(&mut s, 10.0, Some(from));
    }
    assert_eq!(s.impacts.as_ptr(), ptr);
    assert_eq!(s.impacts.len(), 24);
}

// Mirrors `tests/droideka-shield.test.js`:
// "drain, regeneration and unabsorbed damage cannot fabricate hit animations"
#[test]
fn drain_regeneration_and_unabsorbed_damage_cannot_fabricate_hit_animations() {
    let mut s = fixture(false);
    s.input[f::ON] = 1.0;
    for _ in 0..20 {
        s.input[f::VALUE] -= 10.0;
        step(&mut s, 1.0 / 60.0);
    }
    s.input[f::ON] = 0.0;
    assert_eq!(s.input[f::HIT_INDEX], 0.0);
    assert_eq!(s.input[f::FLASH], 0.0);

    // An absorbed hit with no known source produces an undirected flash only.
    s.input[f::ON] = 1.0;
    hit(&mut s, 100.0, None);
    assert!(s.input[f::FLASH] > 0.0);
    assert_eq!(s.input[f::HIT_INDEX], 0.0);
}

// Mirrors `tests/droideka-shield.test.js`:
// "presentation follows rendered feet, switches modes live and disappears rolled/dead"
#[test]
fn presentation_follows_rendered_feet_switches_modes_live_and_disappears_rolled_or_dead() {
    let mut s = fixture(false);
    s.input[f::ON] = 1.0;
    s.input[f::YAW] = std::f64::consts::FRAC_PI_2;
    s.input[f::POSITION] = 3.0;
    s.input[f::POSITION + 1] = 2.0;
    s.input[f::POSITION + 2] = 4.0;

    // A quarter-turn yaw rotates ShieldOffset [0, 1, 0.125] to [+0.125, +1, 0].
    step(&mut s, 0.3);
    let dx = s.input[f::CENTER] - 3.125;
    let dy = s.input[f::CENTER + 1] - 3.0;
    let dz = s.input[f::CENTER + 2] - 4.0;
    assert!((dx * dx + dy * dy + dz * dz).sqrt() < 1e-8);
    assert_eq!(s.input[f::AURA], 0.0);

    // Switching to Enhanced enables the aura live; rolling or dying hides the bubble.
    s.input[f::ENHANCED] = 1.0;
    step(&mut s, 0.0);
    assert_eq!(s.input[f::AURA], 1.0);

    s.input[f::ROLLED] = 1.0;
    step(&mut s, 1.0);
    assert_eq!(s.input[f::VISIBLE], 0.0);

    s.input[f::ROLLED] = 0.0;
    s.input[f::DEAD] = 1.0;
    step(&mut s, 1.0);
    assert_eq!(s.input[f::VISIBLE], 0.0);
}
