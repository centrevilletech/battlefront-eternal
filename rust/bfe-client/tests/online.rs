//! Device callbacks and paused input bursts must retain their admitted storage.
use bfe_client::{
    input::Input,
    online::{clock::Clock, index::Index, input::Sender, receiver::Receiver},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
use winit::{event::MouseButton, keyboard::KeyCode};
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|track| {
            if track.get() {
                ALLOCS.with(|count| {
                    count.set(count.get() + 1);
                });
            }
        });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.with(|track| {
            if track.get() {
                ALLOCS.with(|count| {
                    count.set(count.get() + 1);
                });
            }
        });
        System.realloc(ptr, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

#[test]
fn first_input_feedback_and_long_bursts_keep_the_admitted_storage() {
    let mut receiver = Receiver::new(serde_json::json!({}));
    receiver.welcomed = true;
    receiver.complete = true;
    receiver.corrections = 1;
    receiver.own.generation = 1.;
    receiver.own.class = 1;
    receiver.own.alive = true;
    let mut sender = Sender::new(&[(2., [-0.8, 1.1]); 2]);
    let mut input = Input::default();
    let mut index = Index::new(32);
    let mut clock = Clock::default();
    ALLOCS.with(|c| c.set(0));
    TRACK.with(|t| t.set(true));
    for tick in 1..10000 {
        receiver.latest = tick as f64;
        receiver.corrected_sequence = (tick - 1) as f64;
        receiver.message(true, br#"{"type":"input-ack","epoch":1,"generation":1,"ack":4,"serverTick":6}"#, tick as f64 * 1000. / 60.);
        receiver.message(true,br#"{"type":"entity-edge-results","epoch":1,"generation":1,"received":[1,2],"results":[{"id":1,"tick":5,"status":"consumed"}]}"#,0.);
        sender.feedback(&mut receiver);
        // A long paused click burst fills the retry bank before a send. Its
        // oldest edge expires without reserving or throwing on the frame.
        for _ in 0..40 {
            input.mouse(MouseButton::Left, true);
            input.mouse(MouseButton::Left, false);
        }
        input.key(KeyCode::KeyW, true);
        input.motion(2., -1.);
        let (command, look) = input.take(tick, 1);
        sender.tick(&command, &look, &input.take_flight(), &receiver, 100., tick as f64 * 1000. / 60.);
        sender.preview(&receiver);
        clock.arrive(tick as f64, tick as f64 / 60., 2.).unwrap();
        clock.advance(1. / 60.).unwrap();
        for id in 0..32 {
            assert!(index.insert((id, tick), id as usize));
        }
        assert!(!index.insert((33, tick), 33));
        for id in 0..32 {
            assert_eq!(index.remove(&(id, tick)), Some(id as usize));
        }
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert!(sender.refused > 0);
}

#[test]
fn reliable_events_and_full_changed_field_windows_keep_their_reservations() {
    use bfe_client::online::publications::Publications;
    use bfe_presentation::network_layout::{net_kind as k, net_limits as l, net_record as r};
    let mut receiver = Receiver::new(serde_json::json!({}));
    let mut journal = Publications::default();
    let payload = [7u8; l::FIELD_PAYLOAD];
    let mut row = [0.; r::STRIDE];
    row[r::KIND] = k::COMBAT as f64;
    row[r::SCHEMA] = 8.;
    row[r::GENERATION] = 1.;
    // Include the very first delivered event. serde borrows its payload text;
    // the journal's identity and byte reservations already exist at admission.
    ALLOCS.with(|c| c.set(0));
    TRACK.with(|t| t.set(true));
    receiver.message(true,br#"{"type":"gameplay-events","tick":4,"records":[{"kind":"combat","id":1,"generation":1,"tick":4,"schema":8,"payload":"eyJraW5kIjoiZWZmZWN0IiwibmFtZSI6InNwYXJrIn0="}]}"#,0.);
    for id in 1..20000 {
        row[r::ID] = id as f64;
        row[r::TICK] = id as f64;
        journal.push(&row, &payload);
        journal.push(&row, &payload);
    }
    let count = journal.iter().count();
    let first = journal.iter().next().unwrap().row[r::ID];
    let last = journal.iter().last().unwrap().row[r::ID];
    journal.reset();
    journal.push(&row, &payload);
    let restarted = journal.iter().count();
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert!(receiver.fault.is_none());
    assert_eq!(receiver.publications.iter().count(), 1);
    assert_eq!(count, 2048);
    assert_eq!(first, 17952.);
    assert_eq!(last, 19999.);
    assert_eq!(restarted, 1);
    assert!(journal.refused > 0);
}

#[test]
#[ignore = "requires installed battle content"]
fn installed_infantry_replay_includes_the_first_frame_without_allocating() {
    use bfe_client::online::replay::Replay;
    use bfe_sim::{
        combat::defs::WeaponDefs,
        gen::*,
        store::Strings,
        wire::{store_capture, store_own},
    };
    let root = std::env::var_os("BFE_CONTENT_DIR").expect("battle content directory");
    let content = bfe_content_native::Loader::new(root).load("nab2", "map=nab2&side=rep&ai=off&seed=1").unwrap();
    let defs = bfe_content::defs::Defs::build(&content.content).unwrap();
    let class =
        defs.classes.iter().find(|class| class.registry_id.is_some() && class.walker_clips.is_none() && class.body.is_some()).unwrap();
    let class_id = class.registry_id.unwrap() as usize;
    let mut replay = Replay::new(&content).unwrap();
    let mut receiver = Receiver::new(serde_json::json!({}));
    let mut sender = Sender::new(&[(4., [-0.8, 1.1]); 2]);
    let mut strings = Strings::default();
    strings.intern_str("");
    let name = strings.intern_str(defs.names.text(class.name));
    let setup = WireStoreSetup { classes: vec![WireStoreName { name, id: class_id as u32 }], ..Default::default() };
    let maps = store_capture::Capture::new(&setup, &strings).unwrap();
    let weapons = WeaponDefs::default();
    let mut capture = store_own::Capture::new(&weapons);
    let mut body = bfe_sim::units::class::soldier_body(&class.record, 200., class.getup_time, class.dive_time);
    body.position = [-74., -50.045, 70.];
    body.settled = body.position;
    body.previous = body.position;
    body.normal = [0., 1., 0.];
    body.fuel = 1.;
    body.grounded = true;
    let schema = capture.foot(&UnitBody::Soldier(body)).unwrap();
    let unit = Unit { class_name: name, team: 2., alive: true, ..Default::default() };
    capture.envelope(&unit, schema, (0., 0.), 1., 1., &maps, &weapons, &store_own::Facts::default(), None).unwrap();
    receiver.body_length = capture.body.value.len();
    receiver.body_bytes[..receiver.body_length].copy_from_slice(&capture.body.value);
    receiver.checkpoint_length = capture.payload.value.len();
    receiver.checkpoint[..receiver.checkpoint_length].copy_from_slice(&capture.payload.value);
    receiver.own.id = 16.;
    receiver.own.generation = 2.;
    receiver.own.class = class_id;
    receiver.own.body_schema = 6;
    receiver.own.alive = true;
    receiver.own.tick = 100.;
    let command = bfe_client::generated_input::ClientCommand { held: 1, generation: 2., sent: true, ..Default::default() };
    let look = bfe_client::generated_input::ClientLook { x: 2., y: -1. };
    let flight = bfe_client::generated_input::ClientFlightCommand::default();
    ALLOCS.with(|c| c.set(0));
    TRACK.with(|t| t.set(true));
    for tick in 101..1101 {
        receiver.own.tick = tick as f64 - 1.;
        sender.history.acknowledge(tick as f64 - 1.);
        sender.history.push(&command, &look, &flight, tick as f64, 2., tick as f64);
        replay.frame(&receiver, &sender).unwrap();
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert!(replay.ticks >= 1000);
    let body = replay.body.unwrap();
    assert!(body.position.iter().all(|n| n.is_finite()));
    assert_ne!(body.position, [-74., -50.045, 70.]);
}

#[test]
#[ignore = "requires installed battle content"]
fn installed_audio_observations_and_partial_device_frames_keep_admitted_storage() {
    use bfe_client::{
        audio::Status,
        online::{replay::Replay, sound::Sound, sound_data::Prepared},
    };
    let root = std::env::var_os("BFE_CONTENT_DIR").expect("battle content directory");
    let content = bfe_content_native::Loader::new(root).load("nab2", "map=nab2&side=rep&ai=off&seed=1").unwrap();
    let replay = Replay::new(&content).unwrap();
    let mut sound = Sound::new(Prepared::new(&content).unwrap(), replay.collision.clone(), replay.world);
    let mut output = sound.output.take().unwrap();
    let mut receiver = Receiver::new(serde_json::json!({}));
    receiver.own.id = 16.;
    receiver.own.generation = 1.;
    receiver.own.alive = true;
    receiver.own.class = 1;
    let status = Status::default();
    let mut samples = [0f32; 257];
    // Device quanta and window frames deliberately do not align. Identity
    // changes exercise loop retirement inside the measured first window.
    ALLOCS.with(|c| c.set(0));
    TRACK.with(|t| t.set(true));
    for frame in 0..1000 {
        output.fill(&mut samples, &status);
        receiver.own.generation = 1. + (frame / 100) as f64;
        sound.frame(&receiver, &three::Matrix4::default().elements, 1. / 60., &replay, ([100.; 4], false));
    }
    let normal_refused = sound.refused;
    // A device pause can outlive thousands of reliable shots. The audio
    // journal keeps its newest tail and resumes through the same first-window
    // reservation, including the first real firing burst after that pause.
    use bfe_presentation::network_layout::{net_kind as k, net_record as r};
    let mut row = [0.; r::STRIDE];
    row[r::KIND] = k::COMBAT as f64;
    row[r::SCHEMA] = 8.;
    row[r::GENERATION] = 1.;
    let payload = br#"{"kind":"shot","ownerId":16,"ownerGeneration":10,"weaponId":1,"channel":0,"position":[0,0,0]}"#;
    for id in 1..4097 {
        row[r::ID] = id as f64;
        row[r::TICK] = id as f64;
        receiver.publications.push(&row, payload);
        sound.frame(&receiver, &three::Matrix4::default().elements, 1. / 60., &replay, ([100.; 4], false));
        receiver.publications.clear();
    }
    output.fill(&mut samples, &status);
    sound.frame(&receiver, &three::Matrix4::default().elements, 1. / 60., &replay, ([100.; 4], false));
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert!(sound.batches > 500);
    assert_eq!(normal_refused, 0);
    assert!(sound.refused > 0);
    assert_eq!(sound.events, 4096);
}

#[test]
#[ignore = "requires installed battle content"]
fn installed_dynamic_hull_shapes_include_polygon_identities_before_contact() {
    let root = std::env::var_os("BFE_CONTENT_DIR").expect("battle content directory");
    let content = bfe_content_native::Loader::new(root).load("nab2", "map=nab2&side=rep&ai=off&seed=1").unwrap();
    let replay = bfe_client::online::replay::Replay::new(&content).unwrap();
    let collision = replay.collision.borrow();
    let world = &collision.state.worlds[replay.world as usize];
    assert!(world.order.len() > 4);
    for &id in &world.order {
        let body = &collision.state.bodies[id as usize];
        let tree = &collision.state.trees[body.tree];
        if body.moving {
            assert_eq!(tree.polygons.len(), tree.index.len() / 3, "moving shape lost its polygon identities");
        }
    }
}

#[test]
fn server_rejection_text_and_heartbeat_bursts_stay_bounded() {
    let mut receiver = Receiver::new(serde_json::json!({}));
    ALLOCS.with(|c| c.set(0));
    TRACK.with(|t| t.set(true));
    for _ in 0..4096 {
        receiver.message(true, br#"{"type":"ping","time":123.5}"#, 0.);
    }
    for _ in 0..32 {
        assert_eq!(receiver.pong(), Some(&br#"{"type":"pong","time":123.5}"#[..]));
        receiver.pong_sent();
    }
    assert!(receiver.pong().is_none());
    receiver.message(true, br#"{"type":"ping","time":null}"#, 0.);
    assert_eq!(receiver.pong(), Some(&br#"{"type":"pong","time":null}"#[..]));
    receiver.pong_sent();
    receiver.message(true, br#"{"type":"gameplay-error","message":"This battle is full"}"#, 0.);
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert_eq!(receiver.error().as_deref(), Some("This battle is full"));
    let text = "🚀".repeat(1000);
    let message = serde_json::to_vec(&serde_json::json!({"type":"version-error","message":text})).unwrap();
    let mut rejected = Receiver::new(serde_json::json!({}));
    rejected.message(true, &message, 0.);
    assert_eq!(rejected.error().unwrap().chars().count(), 240);
}

#[test]
fn escaped_server_diagnostic_text_survives_json_decoding() {
    let mut receiver = Receiver::new(serde_json::json!({}));
    receiver.message(true, br#"{"type":"gameplay-error","message":"Rejected: \"full\"\nTry again"}"#, 0.);
    assert_eq!(receiver.error().as_deref(), Some("Rejected: \"full\"\nTry again"));
}
