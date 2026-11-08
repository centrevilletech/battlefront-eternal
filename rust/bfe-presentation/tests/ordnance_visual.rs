//! Synthetic ordnance bolt, beam, projectile, and light-ranking tests ported
//! from `tests/ordnance-visual.test.js` and `tests/ordnance-presentation.test.js`.
//!
//! These cases exercise `bfe_presentation::ordnance::Ordnance` and
//! `bfe_presentation::lights::Lights` over synthetic records without requiring
//! game assets.
//!
//! Coverage from `tests/ordnance-visual.test.js`:
//! - Bolt tail convergence from the displayed muzzle offset `(0.3, -0.2, 0)`
//!   back onto the gameplay ray over `MERGE` (`8m`), tilting the streak axis.
//! - First-draw bolt span extension back to the muzzle when a fast bolt has
//!   already flown `3.5m` before its first rendered frame (`LaserLength = 1.2`).
//! - Mounted / far barrel detachment (`distance^2 > 400`) clamping the muzzle
//!   offset to zero so a bolt never stretches across the map.
//! - Beam drawing (`kind::BEAM`) pushing one streak quad at the far endpoint
//!   and one source light without mutating subsequent bolt axes.
//! - Default `GlowLength = 3 * LaserLength` (`3.6m` vs. `1.2m` core) and
//!   stopped sniper `FadeOutTime` thinning (`0.9 * fade + 0.1`) and fading.
//!
//! Coverage from `tests/ordnance-presentation.test.js`:
//! - `Lights` pool ranking: keeping the strongest eight lights by
//!   `(r + g + b) * scale * radius`, preserving emitter order on ties, and
//!   decaying timed flashes back to zero once their duration elapses.

use bfe_presentation::lights::Lights;
use bfe_presentation::ordnance::Ordnance;
use bfe_presentation::ordnance_layout::{
    call_kind as ck, ordnance_call as cl, ordnance_flag as fl, ordnance_frame as fr,
    ordnance_header as hd, ordnance_kind as kind, ordnance_limits as lim, ordnance_record as rc,
    ordnance_style as st, VERSION,
};
use bfe_presentation::page_layout::{
    light_command as lc, light_frame as lf, light_header as lh, light_kind as lk,
    light_output as lo,
};

/// Registers a blaster bolt and beam style on `Ordnance` at `index`.
/// Populates both Classic quad batch handles (`CORE_BATCH`, `GLOW_BATCH`,
/// `BEAM_BATCH`) and Enhanced volumetric bolt parameters (`LOOK_*`).
fn register_style(
    o: &mut Ordnance,
    index: usize,
    laser_width: f64,
    laser_length: f64,
    glow_length: f64,
    fade_out: f64,
) {
    let s = &mut o.style_input;
    s.fill(0.0);
    s[st::CORE_BATCH] = 0.0;
    s[st::GLOW_BATCH] = 1.0;
    s[st::BEAM_BATCH] = 2.0;
    s[st::LASER_WIDTH] = laser_width;
    s[st::LASER_LENGTH] = laser_length;
    s[st::GLOW_LENGTH] = glow_length;
    s[st::GLOW_RAW] = glow_length;
    s[st::FADE_OUT_TIME] = fade_out;
    s[st::LIGHT_RADIUS] = 4.0;
    s[st::LIGHT_COLOR..st::LIGHT_COLOR + 3].copy_from_slice(&[1.0, 0.2, 0.2]);
    s[st::GLOW_COLOR..st::GLOW_COLOR + 3].copy_from_slice(&[1.0, 0.2, 0.2]);
    s[st::LOOK_COLOR..st::LOOK_COLOR + 3].copy_from_slice(&[1.0, 0.2, 0.2]);
    s[st::LOOK_GLOW] = laser_width * 1.4;
    s[st::LOOK_CORE] = laser_width * 0.45;
    s[st::LOOK_LENGTH] = laser_length;
    s[st::LOOK_BRIGHTNESS] = 1.8;
    s[st::LOOK_LIGHT_RADIUS] = 4.0;
    o.set_style(index);
}

// Mirrors `tests/ordnance-visual.test.js`:
// "a bolt's tail meets the muzzle it was drawn from, then lines up with its path"
// "a bolt first drawn several metres out reaches back to the muzzle once, then keeps its length"
// "a bolt whose shooter has mounted or moved away is drawn on its ray, not from the vehicle"
#[test]
fn bolt_tail_converges_from_muzzle_reaches_back_on_first_draw_and_detaches_when_far() {
    let mut o = Ordnance::new();
    register_style(&mut o, 0, 0.1, 1.2, 3.6, 0.0);

    // Helper closure to write a single bolt record into `o.input` and run `present()`.
    // Supports both Enhanced volumetric bolts (`blaster_arrays`) and Classic quads (`quad_arrays`).
    let draw_bolt = |o: &mut Ordnance,
                     key: f64,
                     flown: f64,
                     drawn: f64,
                     flags: u32,
                     muzzle: [f64; 3],
                     offset: [f64; 3],
                     enhanced: bool| {
        o.input[fr::VERSION] = VERSION as f64;
        o.input[fr::ENHANCED_BOLTS] = if enhanced { 1.0 } else { 0.0 };
        o.input[fr::ENHANCED_SNIPER] = 0.0;
        o.input[fr::RECORDS] = 1.0;
        o.input[fr::NET_COMMANDS] = 0.0;
        let at = fr::STRIDE;
        o.input[at..at + rc::STRIDE].fill(0.0);
        o.input[at + rc::KIND] = kind::BOLT as f64;
        o.input[at + rc::KEY] = key;
        o.input[at + rc::STYLE] = 0.0;
        o.input[at + rc::FLAGS] = flags as f64;
        o.input[at + rc::ALPHA] = 1.0;
        o.input[at + rc::FLOWN] = flown;
        o.input[at + rc::DRAWN] = drawn;
        o.input[at + rc::FADE] = 1.0;
        o.input[at + rc::SPEED] = 100.0;
        o.input[at + rc::LIFE] = 1.0;
        o.input[at + rc::MERGE] = 8.0;
        o.input[at + rc::POSITION..at + rc::POSITION + 3].copy_from_slice(&[0.0, 0.0, flown]);
        o.input[at + rc::DIRECTION..at + rc::DIRECTION + 3].copy_from_slice(&[0.0, 0.0, 1.0]);
        o.input[at + rc::MUZZLE..at + rc::MUZZLE + 3].copy_from_slice(&muzzle);
        o.input[at + rc::OFFSET..at + rc::OFFSET + 3].copy_from_slice(&offset);
        o.present();
    };

    // Case 1: Muzzle at (0.3, -0.2, 0), bolt 2m out with drawn length 1.5m.
    // Head sits at 3/4 of the muzzle offset; tail sits at the muzzle (0.3, -0.2, 0.0).
    draw_bolt(
        &mut o,
        1.0,
        2.0,
        1.5,
        1 << fl::RESOLVE_BOLT,
        [0.3, -0.2, 0.0],
        [0.0; 3],
        true,
    );
    let [head, tail, _, _] = o.blaster_arrays();
    assert!((head[0] - 0.3 * 0.75).abs() < 1e-6);
    assert!((head[1] - (-0.2 * 0.75)).abs() < 1e-6);
    assert!((head[2] - 2.0).abs() < 1e-6);
    // At 2m out with span 2m (first draw stretches to flown=2.0), tail reaches (0.3, -0.2, 0.0).
    assert!((tail[0] - 0.3).abs() < 1e-6);
    assert!((tail[1] - (-0.2)).abs() < 1e-6);
    assert!(tail[2].abs() < 1e-6);

    // At 12m out (past 8m MERGE), the bolt is straight on the +Z ray with length 1.2m.
    draw_bolt(
        &mut o,
        1.0,
        12.0,
        1.5,
        1 << fl::HAS_OFFSET,
        [0.0; 3],
        [0.3, -0.2, 0.0],
        true,
    );
    let [head, tail, _, _] = o.blaster_arrays();
    assert!(head[0].abs() < 1e-6 && head[1].abs() < 1e-6 && (head[2] - 12.0).abs() < 1e-6);
    assert!(tail[0].abs() < 1e-6 && tail[1].abs() < 1e-6 && (tail[2] - 10.8).abs() < 1e-6);

    // Case 2: First draw at 3.5m out stretches 3.5m back to the muzzle once, then 1.2m next frame.
    // This prevents a high-velocity bolt from leaving a gap between the gun barrel and its tail.
    draw_bolt(
        &mut o,
        2.0,
        3.5,
        1.2,
        1 << fl::RESOLVE_BOLT,
        [0.3, -0.2, 0.0],
        [0.0; 3],
        false,
    );
    let [pos, _, shape, axis, _, _] = o.quad_arrays();
    let len1 = shape[2] as f64;
    let tail_x = pos[0] as f64 - axis[0] as f64 * len1;
    let tail_y = pos[1] as f64 - axis[1] as f64 * len1;
    let tail_z = pos[2] as f64 - axis[2] as f64 * len1;
    assert!((tail_x - 0.3).abs() < 1e-5);
    assert!((tail_y - (-0.2)).abs() < 1e-5);
    assert!(tail_z.abs() < 1e-5);

    // Second frame of key 2.0 at 5.0m out uses the class's 1.2m LaserLength.
    draw_bolt(
        &mut o,
        2.0,
        5.0,
        1.2,
        1 << fl::HAS_OFFSET,
        [0.0; 3],
        [0.3, -0.2, 0.0],
        false,
    );
    let [_, _, shape, _, _, _] = o.quad_arrays();
    assert!((shape[2] as f64 - 1.2).abs() < 0.02);

    // Case 3: Shooter muzzle 50m away (> 20m threshold) is ignored and clamped to zero offset.
    // This guards against a shooter mounting a distant vehicle right after firing on foot.
    draw_bolt(
        &mut o,
        3.0,
        1.0,
        1.0,
        1 << fl::RESOLVE_BOLT,
        [50.0, 0.0, 40.0],
        [0.0; 3],
        false,
    );
    let [pos, _, _, axis, _, _] = o.quad_arrays();
    assert_eq!(&pos[0..3], &[0.0, 0.0, 1.0]);
    assert_eq!(&axis[0..3], &[0.0, 0.0, 1.0]);
}

// Mirrors `tests/ordnance-visual.test.js`:
// "beams draw without a ReferenceError and without mutating the caller's vectors"
// "a bolt without GlowLength defaults its glow to three times its core and tapers both ends"
// "a stopped sniper bolt thins and fades over FadeOutTime instead of vanishing on impact"
#[test]
fn beams_glow_defaults_and_stopped_sniper_fade_out_match_classic_rules() {
    let mut o = Ordnance::new();
    // Style 0: LaserWidth = 0.2, LaserLength = 1.2, GlowLength = 3.6 (3x core), FadeOutTime = 0.2.
    register_style(&mut o, 0, 0.2, 1.2, 3.6, 0.2);

    // Draw a Classic beam from (1, 2, 3) to (1, 2, 13) along +Z with life = 0.04 (fade = 0.5).
    // The beam quad is anchored at `END` and extends backward along `DIRECTION` by `distance(start, end)`.
    o.input[fr::VERSION] = VERSION as f64;
    o.input[fr::ENHANCED_BOLTS] = 0.0;
    o.input[fr::RECORDS] = 1.0;
    let at = fr::STRIDE;
    o.input[at..at + rc::STRIDE].fill(0.0);
    o.input[at + rc::KIND] = kind::BEAM as f64;
    o.input[at + rc::STYLE] = 0.0;
    o.input[at + rc::LIFE] = 0.04;
    o.input[at + rc::POSITION..at + rc::POSITION + 3].copy_from_slice(&[1.0, 2.0, 3.0]);
    o.input[at + rc::END..at + rc::END + 3].copy_from_slice(&[1.0, 2.0, 13.0]);
    o.input[at + rc::DIRECTION..at + rc::DIRECTION + 3].copy_from_slice(&[0.0, 0.0, 1.0]);
    o.present();

    // Beam batch (handle 2) receives 1 quad at end (1, 2, 13) of length 10 and alpha 0.5.
    assert_eq!(o.counts()[2], 1);
    let [pos, color, shape, _, _, _] = o.quad_arrays();
    let b2 = 2 * lim::QUADS_PER_HANDLE;
    assert_eq!(&pos[b2 * 3..b2 * 3 + 3], &[1.0, 2.0, 13.0]);
    assert!((color[b2 * 4 + 3] - 0.5).abs() < 1e-6);
    assert!((shape[b2 * 4 + 2] - 10.0).abs() < 1e-6);

    // Draw a Classic bolt 5m out: core (handle 0) has length 1.2 and taper 0.5;
    // glow (handle 1) has 2 quads of length 3.6 and taper 0.8.
    o.input[at..at + rc::STRIDE].fill(0.0);
    o.input[at + rc::KIND] = kind::BOLT as f64;
    o.input[at + rc::KEY] = 10.0;
    o.input[at + rc::STYLE] = 0.0;
    o.input[at + rc::ALPHA] = 1.0;
    o.input[at + rc::FLOWN] = 5.0;
    o.input[at + rc::DRAWN] = 3.6;
    o.input[at + rc::FADE] = 1.0;
    o.input[at + rc::SPEED] = 100.0;
    o.input[at + rc::MERGE] = 8.0;
    o.input[at + rc::POSITION + 2] = 5.0;
    o.input[at + rc::DIRECTION + 2] = 1.0;
    // Present twice so `first_draw` is false on the second frame.
    // On the second presentation, `core_span` equals `LaserLength` (1.2) and `glow_span` equals 3.6.
    o.present();
    o.present();

    assert_eq!(o.counts()[0], 1);
    assert_eq!(o.counts()[1], 2);
    let [_, _, shape, _, axis2, _] = o.quad_arrays();
    let g1 = lim::QUADS_PER_HANDLE;
    assert!((shape[2] - 1.2).abs() < 1e-6);
    assert!((shape[g1 * 4 + 2] - 3.6).abs() < 1e-6);
    assert!((axis2[0] - 0.5).abs() < 1e-6);
    assert!((axis2[g1 * 3] - 0.8).abs() < 1e-6);

    // Stopped sniper bolt with fade = 0.5 thins to `0.9 * 0.5 + 0.1 = 0.55` of its full width
    // and keeps its light half a step behind the impact point (`5.0 - (100 / 60) / 2`).
    let full_core_width = shape[0];
    o.input[at + rc::ENDED] = 1.0;
    o.input[at + rc::FADE] = 0.5;
    o.present();
    let [_, color, shape, _, _, _] = o.quad_arrays();
    assert!((shape[0] - full_core_width * 0.55).abs() < 1e-6);
    assert!((color[3] - 0.5).abs() < 1e-6);
    assert_eq!(o.header[hd::CALLS], 1.0);
    assert_eq!(o.calls()[cl::KIND], ck::LIGHT as f64);
    assert!((o.calls()[cl::A + 2] - (5.0 - (100.0 / 60.0) / 2.0)).abs() < 1e-6);
}

// Mirrors `tests/ordnance-presentation.test.js`:
// "LightPool ranks the strongest eight lights and fades flashes"
#[test]
fn light_pool_ranks_eight_strongest_lights_and_decays_flashes() {
    let mut lights = Lights::new(8);
    lights.input[lf::DT] = 0.05;
    lights.input[lf::COMMANDS] = 10.0;
    lights.commands.fill(0.0);

    // Command 0: a strong timed flash at Z = 2 (radius 6, duration 0.2, serial 1).
    // Timed flashes persist across frames until their remaining duration reaches 0.
    lights.commands[lc::KIND] = lk::FLASH as f64;
    lights.commands[lc::POSITION + 2] = 2.0;
    lights.commands[lc::RGB..lc::RGB + 3].copy_from_slice(&[1.0, 1.0, 1.0]);
    lights.commands[lc::RADIUS] = 6.0;
    lights.commands[lc::DURATION] = 0.2;
    lights.commands[lc::SERIAL] = 1.0;

    // Commands 1..10: 9 one-frame ADD requests with decreasing radius (9.0 down to 1.0).
    // Each ADD candidate is scored by `(r + g + b) * scale * radius`.
    for i in 0..9 {
        let at = (i + 1) * lc::STRIDE;
        lights.commands[at + lc::KIND] = lk::ADD as f64;
        lights.commands[at + lc::POSITION + 2] = 10.0 + i as f64;
        lights.commands[at + lc::RGB..at + lc::RGB + 3].copy_from_slice(&[1.0, 0.5, 0.5]);
        lights.commands[at + lc::RADIUS] = 9.0 - i as f64;
        lights.commands[at + lc::SCALE] = 1.0;
    }

    // Step 1: 8 strongest lights are selected; the weakest 2 (radii 2.0 and 1.0) are dropped.
    // Output row 0 holds the strongest candidate (Z = 10.0, radius 9.0, score = 18.0).
    lights.frame();
    assert_eq!(lights.header[lh::COUNT], 8.0);
    assert_eq!(lights.header[lh::TIMED], 1.0);
    assert_eq!(lights.output[lo::POSITION + 2], 10.0);

    // Step 2: advance by 0.2s with no new commands so the timed flash expires completely.
    // Both `COUNT` and `TIMED` return to 0.0 once the flash finishes its 0.2s decay.
    lights.input[lf::DT] = 0.2;
    lights.input[lf::COMMANDS] = 0.0;
    lights.frame();
    assert_eq!(lights.header[lh::COUNT], 0.0);
    assert_eq!(lights.header[lh::TIMED], 0.0);
}
