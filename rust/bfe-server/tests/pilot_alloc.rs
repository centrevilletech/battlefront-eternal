//! Verifies that carrying a Space `PILOT` rev 1 record section on a `BNI1`
//! input packet adds zero heap allocations to the server decoder and host
//! input admission path compared with an identical packet without records.
use bfe_family::GameFamily;
use bfe_server::world_worker::{decode_entity_input_with_records, NativeWorldHost};
use bfe_sim::space::flight::SpacePilotInput;
use bfe_sim::space::local::PilotControls;
use bfe_sim::wire::revisions::{
    plan, write_section, Section, Supports, Table, PILOT, SUPPORTS, WRITES_SPACE,
};
use bfe_sim::wire::space::{write_pilot_entries, PilotEntry, REVISION};
use bfe_test_gate::OPEN;
use std::path::PathBuf;

#[global_allocator]
static ALLOCATOR: bfe_test_alloc::Counting = bfe_test_alloc::Counting::new();

/// `f`'s result and the allocations this thread made while it ran.
fn measure<R>(f: impl FnOnce() -> R) -> (R, u32) {
    bfe_test_alloc::measure_thread(f)
}

fn build_input_packet(
    epoch: u32,
    generation: u32,
    sequence: u32,
    tick: u32,
    pilot: Option<PilotControls>,
) -> Vec<u8> {
    let mut p = Vec::with_capacity(128);
    let mut pilot_body = Vec::new();
    let mut records = Vec::new();
    if let Some(controls) = pilot {
        write_pilot_entries(&[PilotEntry { sequence, controls }], &mut pilot_body).unwrap();
        write_section(
            &[Section {
                kind: PILOT,
                revision: REVISION,
                body: &pilot_body,
            }],
            &mut records,
        )
        .unwrap();
    }
    p.extend_from_slice(&0x3149_4e42u32.to_le_bytes()); // BNI1
    p.extend_from_slice(&epoch.to_le_bytes());
    p.extend_from_slice(&generation.to_le_bytes());
    p.push(1); // nv
    p.push(1); // nc
    p.push(0); // ne
    p.push(if records.is_empty() { 0 } else { 2 }); // flags: bit 1 = records section
    p.extend_from_slice(&0u16.to_le_bytes()); // view flags
    p.extend_from_slice(&0u16.to_le_bytes()); // view reserved
    p.extend_from_slice(&sequence.to_le_bytes());
    p.extend_from_slice(&tick.to_le_bytes());
    p.push(0); // view index
    p.extend_from_slice(&[0u8; 16]); // held codes
    p.extend_from_slice(&0.0f64.to_le_bytes());
    p.extend_from_slice(&0.0f64.to_le_bytes());
    p.extend_from_slice(&records);
    p
}

#[test]
fn pilot_record_adds_zero_allocations_in_server_decoder() {
    let space_plan = plan(&Table::of(WRITES_SPACE), &Supports::of(SUPPORTS)).unwrap();
    let controls = PilotControls {
        mouse: [0.25, -0.15],
        flight: SpacePilotInput {
            throttle: 0.9,
            stick_x: 0.3,
            boost: true,
            fire_primary: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let without = build_input_packet(1, 1, 5, 10, None);
    let with_pilot = build_input_packet(1, 1, 5, 10, Some(controls));

    // Warm once before counting.
    assert_eq!(
        decode_entity_input_with_records(&with_pilot, GameFamily::Swbf2, Some(&space_plan))
            .unwrap()
            .pilot_entries()
            .len(),
        1
    );

    let (_, allocs_without) = measure(|| {
        for _ in 0..500 {
            let d =
                decode_entity_input_with_records(&without, GameFamily::Swbf2, Some(&space_plan))
                    .unwrap();
            assert!(d.pilot_entries().is_empty());
        }
    });
    let (_, allocs_with) = measure(|| {
        for _ in 0..500 {
            let d =
                decode_entity_input_with_records(&with_pilot, GameFamily::Swbf2, Some(&space_plan))
                    .unwrap();
            assert_eq!(d.pilot_entries().len(), 1);
        }
    });
    assert_eq!(
        allocs_with, allocs_without,
        "PILOT section added {} extra heap allocations across 500 packets",
        allocs_with.saturating_sub(allocs_without)
    );
}

#[test]
#[ignore = "requires prepared SWBF2 spa6 content (BFE_SWBF2_PREPARED)"]
fn server_handle_input_with_pilot_record_adds_zero_extra_allocations() {
    let root = PathBuf::from(
        std::env::var("BFE_SWBF2_PREPARED").expect("BFE_SWBF2_PREPARED"),
    );
    let query = "?map=spa6&era=cw&mode=ass&side=rep&seed=41";
    let mut host = NativeWorldHost::load_for(&root, query, 1, GameFamily::Swbf2, OPEN).unwrap();
    let space_plan = plan(&Table::of(WRITES_SPACE), &Supports::of(SUPPORTS)).unwrap();
    let welcome = host.add_player("pilot", "Pilot", false, false, false).unwrap();
    host.set_plan("pilot", Some(space_plan));
    host.handle_start("pilot", 0.0);
    let mut now = 0.0f64;
    let mut wall = 1_700_000_000_000;
    for _ in 0..30 {
        now += 1000.0 / 60.0;
        wall += 17;
        host.step(now, wall).unwrap();
    }

    let controls = PilotControls {
        mouse: [0.2, -0.1],
        flight: SpacePilotInput {
            throttle: 0.75,
            stick_x: 0.25,
            ..Default::default()
        },
        ..Default::default()
    };

    // Pre-build both packet streams outside the measured section so each packet
    // arrives at `tick = host.tick + 1` (realistic 60 Hz cadence, queue depth 1).
    let n = 60usize;
    let warm_n = 10usize;
    let start_tick = host.tick;
    let mut pkts_warm = Vec::with_capacity(warm_n);
    let mut pkts_plain = Vec::with_capacity(n);
    let mut pkts_pilot = Vec::with_capacity(n);
    let mut seq = 1u32;
    for i in 0..warm_n {
        pkts_warm.push(build_input_packet(
            host.epoch,
            welcome.generation,
            seq,
            start_tick + i as u32 + 1,
            (i % 2 == 1).then_some(controls),
        ));
        seq += 1;
    }
    for i in 0..n {
        pkts_plain.push(build_input_packet(
            host.epoch,
            welcome.generation,
            seq,
            start_tick + (warm_n + i) as u32 + 1,
            None,
        ));
        seq += 1;
    }
    for i in 0..n {
        pkts_pilot.push(build_input_packet(
            host.epoch,
            welcome.generation,
            seq,
            start_tick + (warm_n + n + i) as u32 + 1,
            Some(controls),
        ));
        seq += 1;
    }

    for pkt in &pkts_warm {
        host.handle_input("pilot", pkt, now);
        now += 1000.0 / 60.0;
        wall += 17;
        host.step(now, wall).unwrap();
    }

    let mut allocs_plain = 0u32;
    for pkt in &pkts_plain {
        let (_, a) = measure(|| host.handle_input("pilot", pkt, now));
        allocs_plain += a;
        now += 1000.0 / 60.0;
        wall += 17;
        host.step(now, wall).unwrap();
    }
    let mut allocs_pilot = 0u32;
    for pkt in &pkts_pilot {
        let (_, a) = measure(|| host.handle_input("pilot", pkt, now));
        allocs_pilot += a;
        now += 1000.0 / 60.0;
        wall += 17;
        host.step(now, wall).unwrap();
    }
    println!("handle_input over {n} ticks: plain={allocs_plain} allocs, pilot={allocs_pilot} allocs");
    assert_eq!(
        allocs_pilot, allocs_plain,
        "PILOT packets added {} extra allocations on NativeWorldHost::handle_input",
        allocs_pilot.saturating_sub(allocs_plain)
    );
}
