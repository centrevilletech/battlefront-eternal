//! Whole displayed frames and burst observations exercise their fixed banks.
//! The allocator guard covers Rust-owned state after cold resource admission.
use super::*;
use crate::{
    analytic_effects::Analytic,
    vehicle_effects::Vehicles,
    visual_effects_layout::{
        analytic_arrays as aa, analytic_batch as ab, analytic_frame as af, analytic_spawn as asp,
        jet_frame as j, shield_frame as s, trail_frame as t, vehicle_fx_action as action,
        vehicle_fx_frame as vf, vehicle_fx_input as vi, vehicle_fx_record as vo,
        visual_limits as limits, wake_frame as w,
    },
};
use crate::effects::lightning::tests::{ALLOCS as COUNT, TRACK};
use std::cell::Cell;
fn start() {
    COUNT.with(|n| n.set(0));
    TRACK.with(|t| t.set(true));
}
fn finish() {
    TRACK.with(|t| t.set(false));
    assert_eq!(COUNT.with(Cell::get), 0, "displayed phase allocated");
}

#[test]
fn long_jetpack_and_rolling_frames_are_bounded_and_allocate_nothing() {
    let mut jet = jetpack::Jetpack::new(Vec::new());
    jet.input[j::ENHANCED] = 1.;
    jet.input[j::ACTIVE] = 1.;
    jet.input[j::DT] = 1. / 60.;
    jet.input[j::QUATERNION + 3] = 1.;
    let mut dust = trail::Trail::new("desert");
    dust.input[t::ENHANCED] = 1.;
    dust.input[t::ROLLING] = 1.;
    dust.input[t::GROUNDED] = 1.;
    dust.input[t::VELOCITY] = 10.;
    dust.input[t::DT] = 1. / 60.;
    dust.input[t::FLOOR_HIT] = 1.;
    dust.input[t::FLOOR_NORMAL] = 1.;
    start();
    for frame in 0..10_000 {
        jet.input[j::POSITION] = frame as f64 / 60.;
        jet.frame(&mut || 0.5);
        dust.input[t::POSITION] = frame as f64 / 6.;
        dust.probe();
        dust.roof();
        dust.frame(&mut || 0.5);
    }
    finish();
    for (k, capacity) in [96, 128, 64].iter().enumerate() {
        assert!(jet.wake.input[w::COUNTS + k] <= *capacity as f64);
    }
    assert!(dust.input[t::COUNT] <= 160.);
    assert!(jet.wake.particles.iter().all(|n| n.is_finite()));
    assert!(dust.records.iter().all(|n| n.is_finite()));
    // A pause and an oversized producer count cannot grow the wake or bridge
    // a teleport. Zero-time draws retain already rounded particle births.
    jet.input[j::DT] = 1e20;
    jet.input[j::NOZZLES] = 1e20;
    jet.frame(&mut || 0.5);
    jet.input[j::POSITION] += 100.;
    jet.input[j::ACTIVE] = 0.;
    jet.frame(&mut || 0.5);
    assert!(jet.wake.input[w::COUNTS..w::COUNTS + 3]
        .iter()
        .all(|n| *n == 0.));
}

#[test]
fn paused_absorption_keeps_the_latest_six_hits_without_a_queue() {
    let mut shield = shield::Shield::default();
    shield.input[s::RADIUS] = 1.25;
    shield.input[s::HEIGHT] = 1.25;
    shield.input[s::MAX] = 4000.;
    shield.input[s::HAS_SOURCE] = 1.;
    shield.input[s::SOURCE] = 5.;
    start();
    for amount in 1..100_001 {
        shield.input[s::COMMAND] = 1.;
        shield.input[s::AMOUNT] = amount as f64;
        shield.frame();
    }
    finish();
    assert_eq!(shield.input[s::HIT_INDEX], 100_000.);
    assert_eq!(shield.impacts.len(), 24);
    assert!(shield.powers.iter().all(|n| *n == 1.5));
    assert!(shield.impacts.iter().all(|n| n.is_finite()));
}

#[test]
fn vehicle_bursts_clip_admission_and_keep_sparse_body_identity() {
    let mut phase = Vehicles::default();
    for id in 0..limits::VEHICLES {
        let descriptor = serde_json::from_str(
            r#"{"nodes":1,"damage":[{"effect":1,"start":1,"stop":0,"scale":0.4}]}"#,
        )
        .unwrap();
        assert_eq!(phase.add(descriptor), id);
        let at = id * vi::STRIDE;
        phase.input[at + vi::PRESENT] = 1.;
        phase.input[at + vi::ALIVE] = 1.;
        phase.input[at + vi::HEALTH] = 50.;
        phase.input[at + vi::MAX_HEALTH] = 100.;
        let node = phase.node_start(id) * 16;
        for k in [0, 5, 10, 15] {
            phase.nodes[node + k] = 1.;
        }
        phase.nodes[node + 12] = id as f64;
    }
    assert_eq!(
        phase.add(serde_json::from_str(r#"{"nodes":1}"#).unwrap()),
        usize::MAX
    );
    phase.frame[vf::COUNT] = limits::VEHICLES as f64;
    for k in 0..limits::VEHICLES {
        phase.order[k] = k as u32;
    }
    start();
    phase.frame();
    phase.input[vi::PRESENT] = 0.;
    phase.input[vi::STRIDE + vi::ALIVE] = 0.;
    phase.frame();
    finish();
    let first = phase.output_start(0) * vo::STRIDE;
    let second = phase.output_start(1) * vo::STRIDE;
    let last = phase.output_start(limits::VEHICLES - 1) * vo::STRIDE;
    assert_eq!(phase.output[first + vo::ACTION], action::NONE as f64);
    assert_eq!(phase.output[second + vo::ACTION], action::STOP as f64);
    assert_eq!(
        phase.output[last + vo::POSITION],
        (limits::VEHICLES - 1) as f64
    );
    assert_eq!(phase.output[last + vo::SCALE], 0.4);
}

#[test]
fn analytic_epoch_rebases_and_compacts_without_rewriting_stable_births() {
    let mut phase = Analytic::new(Vec::new());
    let id = phase.add_batch();
    assert_eq!(id, 0);
    phase.input[af::EMITTERS] = f64::MAX;
    phase.emitters[crate::layout::effect::TIME] = 256.5;
    phase.batches[ab::COUNT] = f64::MAX;
    for n in 0..limits::ANALYTIC_PARTICLES {
        let at = n * asp::STRIDE;
        phase.spawns[at + asp::SERIAL] = n as f64 + 1.;
        phase.spawns[at + asp::POSITION] = n as f64;
        phase.spawns[at + asp::BIRTH] = 256.25;
    }
    start();
    phase.frame();
    phase.batches[ab::UNREAD] = f64::INFINITY;
    phase.frame();
    finish();
    assert_eq!(phase.epochs[0], 256.);
    assert_eq!(phase.arrays[aa::META], 0.25);
    assert_eq!(phase.batches[ab::DIRTY], f64::INFINITY);
    assert_eq!(phase.batches[ab::UNREAD], f64::INFINITY);
    phase.batches[ab::COUNT] = 0.;
    phase.frame();
    phase.batches[ab::COUNT] = 1.;
    phase.frame();
    assert_eq!(phase.batches[ab::DIRTY], 0.);
}

#[test]
fn dying_requests_preserve_event_order_and_sample_only_the_chosen_bone() {
    use crate::visual_effects_layout::vehicle_fx_query as q;
    let mut phase = Vehicles::default();
    let content=serde_json::from_str(r#"{"nodes":2,"bones":[1],"dying":{"span":1,"interval":0.1,"effects":[7,8]},"dust":{"entry":{"effect":3},"delay":0}}"#).unwrap();
    assert_eq!(phase.add(content), 0);
    phase.frame[vf::COUNT] = 1.;
    phase.order[0] = 0;
    phase.input[vi::PRESENT] = 1.;
    phase.input[vi::ALIVE] = 1.;
    for node in 0..2 {
        for k in [0, 5, 10, 15] {
            phase.nodes[node * 16 + k] = 1.;
        }
    }
    phase.frame();
    phase.input[vi::ALIVE] = 0.;
    phase.input[vi::DYING] = 0.5;
    start();
    phase.frame();
    let request = phase.frame[vf::PENDING];
    phase.frame[vf::RANDOM] = 0.75;
    phase.frame[vf::RANDOM + 1] = 0.5;
    phase.resume();
    let node = phase.frame[vf::NODE];
    let matrix_request = phase.frame[vf::PENDING];
    // This is the host's raw answer. The selected bone moved after earlier
    // effects were applied, so resolving it before the request would be stale.
    phase.nodes[16 + 12] = 9.;
    phase.complete();
    finish();
    assert_eq!(request, q::RANDOM as f64);
    assert_eq!(matrix_request, q::MATRIX as f64);
    assert_eq!(node, 1.);
    assert_eq!(phase.output[vo::STRIDE + vo::EFFECT], 3.);
    assert_eq!(phase.output[2 * vo::STRIDE + vo::EFFECT], 8.);
    assert_eq!(phase.output[2 * vo::STRIDE + vo::POSITION], 9.);
    assert_eq!(phase.frame[vf::PENDING], q::NONE as f64);
    assert_eq!(phase.frame[vf::CURSOR], 1.);
}
