//! Synthetic arc-lightning bolt, chain, body/shield shock, charging muzzle, and
//! classic fallback tests ported from `tests/arc-lightning.test.js`.
//!
//! These cases exercise `bfe_presentation::effects::lightning::{Actor,
//! Collision, Definition, DrawCamera, EmptyCollision, Fixture, Lightning}`
//! over synthetic unit and bone tables without requiring game assets.
//!
//! Coverage includes:
//! - Subdivided channel displacement (`Fixture::Displace`), leader tip growth,
//!   main/child/branch capsule emission, and charge scaling (`charge = 0` vs `1`).
//! - Chained bolt link delay (`+0.05s`), target soldier body shock, skeleton
//!   joint twitch and clean restoration on `end_shock` / `clear`, and first-person
//!   view-arm arc routing (`VIEW_CAPSULES` vs `CAPSULES`).
//! - Droideka ellipsoid shield interception (`on_shield` and `draw_shield`),
//!   charging muzzle crackle while `CHARGING != 0`, wall surface normal spray
//!   vs. max-range air fizzle, Classic ribbon fallback, and `DrawCamera`
//!   pixel-angle scaling.

use bfe_presentation::effects::lightning::{
    Actor, Collision, Definition, DrawCamera, EmptyCollision, Fixture, Lightning,
};
use bfe_presentation::effects::lightning_layout::{
    lightning_bolt as bo, lightning_camera as lc, lightning_capsule as ca, lightning_counts as h,
    lightning_frame as f, lightning_joint as jo, lightning_light as ll, lightning_node as no,
    lightning_shock as so, lightning_spawn as sp, lightning_unit as u,
};
use three::Vector3;

/// Deterministic LCG matching `random()` in `lightning::tests`.
/// Advances `seed` via Numerical Recipes constants and scales to `[0, 1)`.
fn lcg(seed: &mut u32) -> f64 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    *seed as f64 / 4294967296.0
}

/// Returns the stock Dark Trooper arc caster `Definition`.
/// Includes authored width, fade time, spread factor, child strand count,
/// branch parameters, and RGBA colour arrays.
fn caster_definition() -> Definition {
    serde_json::from_str(
        r#"{
            "Width": 0.5,
            "FadeTime": 0.6,
            "SpreadFactor": 0.8,
            "InterpolationSpeed": 5.0,
            "BranchFactor": 0.15,
            "BranchSpreadFactor": 0.8,
            "BranchLength": 4.0,
            "MaxBranches": 3.0,
            "BreakDistance": 1.0,
            "TextureSize": 3.5,
            "ChildBreakDistance": 1.5,
            "ChildTextureSize": 2.0,
            "ChildWidth": 0.4,
            "ChildSpreadFactor": 1.0,
            "NumChildren": 3.0,
            "Color": [130.0, 170.0, 255.0, 250.0],
            "ChildColor": [140.0, 180.0, 255.0, 100.0]
        }"#,
    )
    .unwrap()
}

/// Populates a synthetic alive soldier at unit slot `id` with a 3-bone actor rig.
/// Node 0 is the lower body (`low`), node 1 is the `chest` and first twitch joint,
/// and node 2 is the upper spine/head and second twitch joint.
fn setup_soldier(owner: &mut Lightning, id: usize, pos: [f64; 3], gen: f64) {
    let start = owner.actor(
        id,
        Actor {
            node_start: None,
            parents: vec![-1, 0, 1],
            bones: vec![0, 1, 2],
            low: vec![0],
            chest: Some(1),
            twitch: vec![(1, 0.25), (2, 0.20)],
        },
    );
    // Mark unit slot `id` as alive, listed, combat-capable, soldier, and visible.
    // `GENERATION` tracks respawn identity so stale shocks do not bleed onto a new life.
    let o = id * u::STRIDE;
    owner.units[o..o + u::STRIDE].fill(0.0);
    owner.units[o + u::ALIVE] = 1.0;
    owner.units[o + u::LISTED] = 1.0;
    owner.units[o + u::COMBAT] = 1.0;
    owner.units[o + u::SOLDIER] = 1.0;
    owner.units[o + u::SHOWN] = 1.0;
    owner.units[o + u::BODY_VISIBLE] = 1.0;
    owner.units[o + u::GENERATION] = gen;
    owner.units[o + u::ROOT..o + u::ROOT + 3].copy_from_slice(&pos);
    // Set identity parent world matrix translated to `pos`.
    for diag in [0, 5, 10, 15] {
        owner.units[o + u::PARENT + diag] = 1.0;
    }
    owner.units[o + u::PARENT + 12..o + u::PARENT + 15].copy_from_slice(&pos);

    // Populate the 3 actor nodes with auto-updating local transforms at Y = 0.5, 1.1, 1.5.
    // `Lightning::step` composes these local transforms into world bone positions.
    for (node, y) in [(0, 0.5), (1, 0.6), (2, 0.4)] {
        let no_at = start + node * no::STRIDE;
        owner.nodes[no_at..no_at + no::STRIDE].fill(0.0);
        owner.nodes[no_at + no::MATRIX_AUTO_UPDATE] = 1.0;
        owner.nodes[no_at + no::POSITION + 1] = y;
        owner.nodes[no_at + no::QUATERNION + 3] = 1.0;
        owner.nodes[no_at + no::SCALE..no_at + no::SCALE + 3].copy_from_slice(&[1.0, 1.0, 1.0]);
    }
}

// Mirrors `tests/arc-lightning.test.js`:
// "displaced channels keep their endpoints and monotonic forward order"
// "leader tip advances to the target, then full charge emits more capsules and brighter light than uncharged"
#[test]
fn leader_growth_charge_scaling_and_displace_fixture_match_expected_bounds() {
    let mut owner = Lightning::default();
    let _def = owner.definition(caster_definition());
    let mut seed = 17u32;

    // Verify `Fixture::Displace` produces 65 points (`2^6 + 1`) anchored at `a` and `b`.
    // Midpoint displacement subdivides the segment 6 times while keeping both endpoints exact.
    let json = owner.fixture(
        Fixture::Displace {
            a: [0.0, 1.0, 0.0],
            b: [0.0, 1.0, 20.0],
            depth: 6,
            u: [1.0, 0.0, 0.0],
            w: [0.0, 1.0, 0.0],
            rough: 0.2,
            spread: 0.6,
            seed: 7,
            jag: 11,
            time: 0.3,
            writhe: 2.6,
            coarse: 2,
        },
        &mut || lcg(&mut seed),
        &mut EmptyCollision,
    );
    let pts: Vec<[f64; 3]> = serde_json::from_str(&json).unwrap();
    assert_eq!(pts.len(), 65);
    assert_eq!(pts[0], [0.0, 1.0, 0.0]);
    assert_eq!(pts[64], [0.0, 1.0, 20.0]);

    // Compare uncharged (`charge = 0.0`) vs. fully charged (`charge = 1.0`) after leader arrival.
    // Returns `(capsule_count, first_capsule_glow_radius, first_light_radius)`.
    // Both runs use the same initial PRNG seed (`42`) and 15m bolt path.
    let run_bolt = |charge: f64| -> (u32, f32, f64) {
        let mut l = Lightning::default();
        let d = l.definition(caster_definition());
        let mut s = 42u32;
        l.frame[f::ENHANCED] = 1.0;
        l.frame[f::DT] = 0.01;
        l.spawn[sp::START..sp::START + 3].copy_from_slice(&[0.0, 1.0, 0.0]);
        l.spawn[sp::END..sp::END + 3].copy_from_slice(&[0.0, 1.0, 15.0]);
        l.spawn[sp::CHARGE] = charge;
        l.spawn[sp::FOLLOW_UNIT] = -1.0;
        let id = l.add(d, false, true, &mut || lcg(&mut s), &mut EmptyCollision);

        // At 0.01s (< leader time 0.035 + 0.0014 * 15 = 0.056s), the bolt has not arrived.
        l.step(&mut || lcg(&mut s), &mut EmptyCollision);
        assert_eq!(l.output[id * bo::STRIDE + bo::ARRIVED], 0.0);

        // Advance another 0.06s so the leader arrives at the target and spawns child strands.
        l.frame[f::DT] = 0.06;
        l.step(&mut || lcg(&mut s), &mut EmptyCollision);
        assert_eq!(l.output[id * bo::STRIDE + bo::ARRIVED], 1.0);
        (
            l.counts[h::CAPSULES],
            l.capsules[ca::SIZE + 1],
            l.lights[ll::RADIUS],
        )
    };

    // Full charge increases child strand count (`1 + round(3 * charge)`), glow width, and light reach.
    let (caps_low, glow_low, radius_low) = run_bolt(0.0);
    let (caps_high, glow_high, radius_high) = run_bolt(1.0);
    assert!(caps_high > caps_low);
    assert!(glow_high > glow_low);
    assert!(radius_high > radius_low);
}

// Mirrors `tests/arc-lightning.test.js`:
// "struck soldiers convulse with surface arcs and restore their joints when the shock ends"
// "first-person shock routes arcs to the view-weapon batch instead of world capsules"
#[test]
fn body_shock_twitches_and_restores_joints_and_routes_first_person_to_view_batch() {
    let mut owner = Lightning::default();
    let def = owner.definition(caster_definition());
    let mut seed = 99u32;

    owner.frame[f::ENHANCED] = 1.0;
    owner.frame[f::DT] = 0.06;
    owner.frame[f::UNITS] = 1.0;
    owner.frame[f::ORDER_COUNT] = 1.0;
    owner.unit_order[0] = 0;
    setup_soldier(&mut owner, 0, [0.0, 0.0, 10.0], 1.0);

    // Fire a bolt into soldier 0's chest at `(0, 1.0, 10)` and step past leader arrival,
    // then step once more so `draw_shock` runs on the newly started shock.
    owner.spawn[sp::START..sp::START + 3].copy_from_slice(&[0.0, 1.0, 0.0]);
    owner.spawn[sp::END..sp::END + 3].copy_from_slice(&[0.0, 1.0, 10.0]);
    owner.spawn[sp::CHARGE] = 1.0;
    owner.spawn[sp::FOLLOW_UNIT] = -1.0;
    owner.add(
        def,
        false,
        true,
        &mut || lcg(&mut seed),
        &mut EmptyCollision,
    );
    owner.step(&mut || lcg(&mut seed), &mut EmptyCollision);
    owner.frame[f::DT] = 0.02;
    owner.step(&mut || lcg(&mut seed), &mut EmptyCollision);

    // Soldier 0 is now shocked, overlay is visible, and twitch joint 0 is active (`jo::ON == 1`).
    // The shock also emits surface capsules crawling between the soldier's bones.
    assert_eq!(owner.shocks_output[so::ACTIVE], 1.0);
    assert_eq!(owner.shocks_output[so::SHOW], 1.0);
    assert_eq!(owner.joints[jo::ON], 1.0);
    // Copy the applied twitch quaternion back into `owner.nodes` as the host does.
    // On the next frame, `Lightning::step` detects `applied` and recovers the clean base pose.
    let node1_q = no::STRIDE + no::QUATERNION;
    let applied = [
        owner.joints[jo::QUATERNION],
        owner.joints[jo::QUATERNION + 1],
        owner.joints[jo::QUATERNION + 2],
        owner.joints[jo::QUATERNION + 3],
    ];
    owner.nodes[node1_q..node1_q + 4].copy_from_slice(&applied);

    // Step past the shock's 1.4s lifetime: `end_shock` restores the identity base quaternion.
    owner.frame[f::DT] = 2.0;
    owner.step(&mut || lcg(&mut seed), &mut EmptyCollision);
    assert_eq!(owner.shocks_output[so::ACTIVE], 0.0);
    assert_eq!(&owner.nodes[node1_q..node1_q + 4], &[0.0, 0.0, 0.0, 1.0]);

    // First-person shock (`SHOWN = 0`, `OWN = 1`, `HAS_VIEW = 1`) routes arm arcs to `VIEW_CAPSULES`.
    // World `CAPSULES` stays at 0 because the first-person body mesh is hidden (`SHOWN == 0`).
    // View-arm arcs crawl between `WRIST`, `GRIP`, and `CURRENT_MUZZLE` in camera space.
    owner.units[u::SHOWN] = 0.0;
    owner.units[u::OWN] = 1.0;
    owner.units[u::HAS_CURRENT_MUZZLE] = 1.0;
    owner.units[u::HAS_WRIST] = 1.0;
    owner.units[u::GRIP..u::GRIP + 3].copy_from_slice(&[0.2, 1.3, 10.3]);
    owner.units[u::CURRENT_MUZZLE..u::CURRENT_MUZZLE + 3].copy_from_slice(&[0.2, 1.3, 10.9]);
    owner.units[u::WRIST..u::WRIST + 3].copy_from_slice(&[0.15, 1.25, 10.2]);
    owner.frame[f::HAS_VIEW] = 1.0;
    owner.frame[f::CAMERA + 3] = 1.0;
    owner
        .inject_shock(
            0,
            r#"{"generation":1.0,"age":0.0,"life":1.0,"strength":1.0,"rgb":[0.6,0.8,1.0],"seed":7}"#,
        )
        .unwrap();
    owner.frame[f::DT] = 0.05;
    owner.step(&mut || lcg(&mut seed), &mut EmptyCollision);
    assert!(owner.counts[h::VIEW_CAPSULES] > 0);
    assert_eq!(owner.counts[h::CAPSULES], 0);
}

// Mirrors `tests/arc-lightning.test.js`:
// "droideka shields intercept bolts at their ellipsoid surface and crawl across the bubble"
// "charging weapons crackle at the muzzle, walls spray along the normal, air shots fizzle, and Classic draws ribbons"
#[test]
fn shield_interception_charging_muzzle_wall_and_air_impacts_and_classic_ribbons() {
    let mut owner = Lightning::default();
    let def = owner.definition(caster_definition());
    let mut seed = 123u32;

    owner.frame[f::ENHANCED] = 1.0;
    owner.frame[f::DT] = 0.06;
    owner.frame[f::UNITS] = 1.0;
    owner.frame[f::ORDER_COUNT] = 1.0;
    owner.unit_order[0] = 0;
    setup_soldier(&mut owner, 0, [0.0, 0.0, 10.0], 1.0);

    // Enable an ellipsoid shield of radius `(1.2, 1.2, 1.5)` centered at `(0, 1.0, 10)`.
    // Any bolt aimed at unit 0's center is clamped to the front ellipsoid surface `Z = 10 - 1.5 = 8.5`.
    owner.units[u::SHIELD] = 1.0;
    owner.units[u::CENTER..u::CENTER + 3].copy_from_slice(&[0.0, 1.0, 10.0]);
    owner.units[u::RADIUS..u::RADIUS + 3].copy_from_slice(&[1.2, 1.2, 1.5]);

    owner.spawn[sp::START..sp::START + 3].copy_from_slice(&[0.0, 1.0, 0.0]);
    owner.spawn[sp::END..sp::END + 3].copy_from_slice(&[0.0, 1.0, 10.0]);
    owner.spawn[sp::CHARGE] = 1.0;
    owner.spawn[sp::FOLLOW_UNIT] = -1.0;
    let b_id = owner.add(def, true, true, &mut || lcg(&mut seed), &mut EmptyCollision);
    owner.step(&mut || lcg(&mut seed), &mut EmptyCollision);

    // Bolt terminates on the front of the shield ellipsoid `(0, 1.0, 8.5)` rather than `(0, 1.0, 10)`.
    // `owner.inspect()` also records an active shield shock on unit 0 (`"shields": [0]`).
    assert!((owner.output[b_id * bo::STRIDE + bo::TO + 2] - 8.5).abs() < 1e-6);
    let state: serde_json::Value = serde_json::from_str(&owner.inspect()).unwrap();
    assert_eq!(state["shields"], serde_json::json!([0]));

    // Switching to Classic mode (`ENHANCED = 0`) clears enhanced shocks and draws Classic ribbon vertices.
    // `h::TEXTURE` reports the active definition index for the Classic ribbon mesh material.
    owner.frame[f::ENHANCED] = 0.0;
    owner.frame[f::DT] = 0.02;
    owner.step(&mut || lcg(&mut seed), &mut EmptyCollision);
    assert!(owner.counts[h::VERTICES] > 0 && owner.counts[h::INDICES] > 0);
    assert_eq!(owner.counts[h::TEXTURE], def as u32);

    // Max-range air shot (30m with no target or wall hit) sets `bo::AIR = 1` and emits no sparks;
    // wall hit sets `bo::AIR = 0` and sprays sparks that bounce once on the floor.
    owner.clear();
    owner.frame[f::ENHANCED] = 1.0;
    owner.frame[f::UNITS] = 0.0;
    owner.frame[f::ORDER_COUNT] = 0.0;
    owner.spawn[sp::START..sp::START + 3].copy_from_slice(&[0.0, 1.0, 0.0]);
    owner.spawn[sp::END..sp::END + 3].copy_from_slice(&[0.0, 1.0, 30.0]);
    let air_id = owner.add(
        def,
        false,
        true,
        &mut || lcg(&mut seed),
        &mut EmptyCollision,
    );
    assert_eq!(owner.output[air_id * bo::STRIDE + bo::AIR], 1.0);

    // Synthetic wall at 12m with surface normal `(0, 0, -1)` facing back toward the shooter.
    // When the bolt reaches the wall (`DT = 0.06`), `burst` emits impact sparks along the normal.
    struct WallHit;
    impl Collision for WallHit {
        fn cast(&mut self, _: Vector3, _: Vector3, _: f64) -> Option<(f64, Vector3)> {
            Some((12.0, Vector3::new(0.0, 0.0, -1.0)))
        }
    }
    owner.spawn[sp::END..sp::END + 3].copy_from_slice(&[0.0, 1.0, 12.0]);
    let wall_id = owner.add(def, false, true, &mut || lcg(&mut seed), &mut WallHit);
    assert_eq!(owner.output[wall_id * bo::STRIDE + bo::AIR], 0.0);
    owner.frame[f::DT] = 0.06;
    owner.step(&mut || lcg(&mut seed), &mut WallHit);
    assert!(owner.counts[h::SPARKS] > 0);

    // Also verify `DrawCamera::step` computes `PIXEL_ANGLE` and `NEAR_DEPTH = near * 1.02`.
    // These uniform values keep screen-space capsule thickness and near-plane clipping consistent.
    let mut dc = DrawCamera::default();
    dc.step(70.0, 2.0, 0.1, 1080.0);
    assert!(dc.output[lc::PIXEL_ANGLE] > 0.0 && dc.output[lc::PIXEL_ANGLE] < 0.002);
    assert!((dc.output[lc::NEAR_DEPTH] - 0.102).abs() < 1e-9);
}
