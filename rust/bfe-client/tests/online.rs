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
#[ignore = "requires installed battle content"]
fn installed_dynamic_hull_shapes_include_polygon_identities_before_contact() {
    let root = std::env::var_os("BFE_CONTENT_DIR").expect("battle content directory");
    let content = bfe_content_native::Loader::new(root, bfe_family::GameFamily::Swbf1).load("nab2", "map=nab2&side=rep&ai=off&seed=1").unwrap();
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

#[test]
fn space_beam_events_and_reposing_allocate_nothing_per_frame() {
    let mut harness = bfe_client::online::combat::swbf2::space::SpaceBeamHarness::new(32);
    let first = harness.step(0);
    ALLOCS.with(|c| c.set(0));
    TRACK.with(|t| t.set(true));
    let mut last = first;
    for step in 1..120 {
        last = harness.step(step);
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(
        ALLOCS.with(Cell::get),
        0,
        "steady-state space beam events and per-frame re-posing must not allocate"
    );
    assert_ne!(first.0, last.0, "fighter beam origin must move across frames");
    assert_ne!(first.1, last.1, "fighter beam endpoint must sweep across frames");
    assert_ne!(first.2, last.2, "turret beam origin must track rotating barrel across frames");
    assert_ne!(first.3, last.3, "turret beam endpoint must sweep across frames");
}

#[test]
fn online_space_receive_publish_and_predict_allocate_nothing() {
    use bfe_client::{
        generated_input::{ClientCommand, ClientFlightCommand, ClientLook},
        input_family::WireInputs,
        local::space::History,
        online::space_receiver::{OnlineSpace, SpaceClientMeta, SpacePredictor},
    };
    use bfe_content::install::CollisionKernel;
    use bfe_family::GameFamily;
    use bfe_presentation::network::RecordChecks;
    use bfe_sim::{
        collision::CollisionOwner,
        space::{
            bounds::SpaceBounds,
            combat::{SpaceDamageCategory, SpaceKick, SpaceShotClock},
            flight::{
                SpaceFighter, SpaceFlightProfile, SpaceFlightState, SpaceVehiclePad,
                SpaceWeaponSpec,
            },
            objects::{
                SpaceBuildingAttachment, SpaceDoor, SpaceTurretPilotType, SpaceTurretState,
                TurretTargetFilter,
            },
            world_anim::SpaceWorldAnims,
            SpaceClientData,
        },
        wire::{
            revisions::{plan, Checks, Supports, Table, SUPPORTS, WRITES_SPACE},
            space::{
                encode_pos_u24, encode_turret_angle, write_fighter_chunk, write_frigates,
                write_guided, write_own_chair, write_own_pilot, write_seats, GuidedSnap,
                OwnChairState, SeatEntry, FIGHTER_SCHEMA, FRIGATES_SCHEMA, ORDNANCE_SCHEMA,
                PROPS_SCHEMA, REVISION, SEATS_FIELD_ID,
            },
            PositionBounds,
        },
    };
    use std::{cell::RefCell, rc::Rc};

    let mut profile = SpaceFlightProfile::default();
    profile.class_name = "zero_alloc_craft".into();
    profile.max_health = 1000.0;
    profile.max_speed = 105.0;
    profile.boost_speed = 170.0;
    profile.min_speed = 35.0;
    profile.acceleration = 50.0;
    profile.boost_acceleration = 80.0;
    profile.energy_bar = 100.0;
    profile.energy_auto_restore = 10.0;
    profile.energy_boost_drain = 15.0;
    profile.turn_rate = 1.25;
    profile.pitch_rate = 1.25;
    profile.roll_rate = 2.0;
    profile.weapons.push(SpaceWeaponSpec {
        name: "laser_main".into(),
        seat: 0,
        channel: 0,
        ..Default::default()
    });
    let pad = SpaceVehiclePad {
        name: "pad0".into(),
        team: 1,
        profile_index: 0,
        control_zone: String::new(),
        position: [0.0; 3],
        yaw: 0.0,
        matrix: [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ],
        spawn_count: 1,
        spawn_time: 8.0,
        expire_time_enemy: 20.0,
        expire_time_field: 40.0,
        decay_time: 10.0,
        respawn_timer: 8.0,
        floor_y: 0.0,
    };
    let turret = SpaceTurretState {
        pilot_type: SpaceTurretPilotType::Remote,
        weapon: SpaceWeaponSpec::default(),
        filter: TurretTargetFilter::default(),
        yaw: 0.0,
        pitch: 0.0,
        prev_yaw: 0.0,
        prev_pitch: 0.0,
        yaw_min: -1.5,
        yaw_max: 1.5,
        pitch_min: -0.6,
        pitch_max: 0.8,
        turn_rate: 1.2,
        pitch_rate: 1.0,
        clock: SpaceShotClock::default(),
        barrage_remaining: 0,
        barrage_timer: 0.0,
        warm_up: 0.0,
        heat: 0.0,
        overheated: false,
        shots_fired: 0,
        muzzle_offsets: Vec::new(),
        remote_operator: None,
        pending_remote_aim: None,
        pending_remote_fire: false,
        pending_remote_spread_scale: None,
        kick: SpaceKick::default(),
        trigger_was_held: false,
    };
    let bld = SpaceBuildingAttachment {
        building_index: 7,
        name: "turret0".into(),
        class: "turret_class".into(),
        base: "armedbuilding".into(),
        instance: 7,
        bodies: [None; 3],
        animated_index: None,
        matrix: [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 10., 5., -20., 1.,
        ],
        position: [10., 5., -20.],
        center: [10., 5., -20.],
        local_center_offset: [0., 1., 0.],
        radius: 3.0,
        sphere: None,
        visible: true,
        lock_on_enabled: false,
        ai_damage_threshold: 0.0,
        damage_category: SpaceDamageCategory::Building,
        death_explosion: None,
        last_life: None,
        turret: Some(turret),
        terminal: None,
    };
    let door = SpaceDoor {
        id: 0,
        instance: 12,
        name: "door0".into(),
        class: "door_class".into(),
        body: None,
        matrix: [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ],
        position: [0.; 3],
        center: [0.; 3],
        door_trigger_radius: 4.0,
        trigger_position: [0.; 3],
        opening_clip: None,
        door_open: false,
        prev_door_open: false,
        previous_fraction: 0.0,
        open_fraction: 0.0,
        locked: false,
        collidable: true,
        visible: true,
    };
    let mut sbounds = SpaceBounds::from_world_json(&bfe_content::Value::Null);
    sbounds.min = [-4000.0; 3];
    sbounds.max = [4000.0; 3];
    let data = SpaceClientData {
        bounds: sbounds,
        world_anims: SpaceWorldAnims::default(),
        profiles: vec![profile.clone()],
        pads: vec![pad.clone()],
        fighter_pad_indices: vec![0],
        fighter_slots: 1,
        buildings: vec![bld],
        turret_buildings: vec![7],
        doors: vec![door],
        weapon_hashes: vec![0x1122_3344],
    };
    let meta = SpaceClientMeta::from_client_data(data, None);
    let bounds = PositionBounds::default();
    let online = OnlineSpace::new(meta.clone(), bounds);
    let mut history = History::new(meta.counts);

    let mut col = Box::new(CollisionOwner::new());
    let empty_bvh = bfe_content::bvh::build(bfe_content::bvh::Positions::F32(&[]), &[]);
    let empty_pos = col.positions(&[]);
    let empty_tree = col.tree(empty_pos, &[], &empty_bvh);
    let world = col.world_new(empty_tree, -1, -1) as usize;
    let mut predictor = SpacePredictor::new(meta.clone(), Rc::new(RefCell::new(col)), world);

    // Pre-build all Space wire payloads outside the measured loop.
    let mut fighter = SpaceFighter::spawned_on_pad(0, 1, 0, &pad, 0, &profile, 0.0, None, None);
    fighter.state = SpaceFlightState::Flying;
    fighter.airspeed = 105.0;
    fighter.velocity = [0.0, 0.0, 105.0];
    let mut chunk_payload = REVISION.to_le_bytes().to_vec();
    write_fighter_chunk(0, &[fighter.clone()], &bounds, &mut chunk_payload).unwrap();
    let mut seats_payload = REVISION.to_le_bytes().to_vec();
    write_seats(
        &[SeatEntry {
            unit_slot: 3,
            is_chair: false,
            seat: 0,
            mount_index: 0,
        }],
        &mut seats_payload,
    )
    .unwrap();
    let mut frigates_payload = REVISION.to_le_bytes().to_vec();
    write_frigates(&[], &mut frigates_payload).unwrap();
    let mut guided_payload = REVISION.to_le_bytes().to_vec();
    write_guided(
        &[GuidedSnap {
            slot: 0,
            serial: 9,
            weapon_idx: 0,
            pos: encode_pos_u24(&bounds, [10.0, 20.0, 30.0]),
            vel: [0, 0, 800],
        }],
        &mut guided_payload,
    )
    .unwrap();
    let mut events_payload = REVISION.to_le_bytes().to_vec();
    events_payload.extend([1u8, 1, 1, 1]);
    events_payload.extend(11u16.to_le_bytes());
    events_payload.extend([0u8, 1u8 << 4]);
    events_payload.extend([0u8; 9]);
    events_payload.extend([0i16, 0, 800].into_iter().flat_map(i16::to_le_bytes));
    events_payload.extend(11u16.to_le_bytes());
    events_payload.extend([0u8, 1u8 << 6]);
    events_payload.extend([0u8; 9]);
    events_payload.extend(0u16.to_le_bytes());
    events_payload.push(0u8);
    events_payload.extend([0u8; 9]);
    let mut turrets_payload = REVISION.to_le_bytes().to_vec();
    turrets_payload.extend([0u8, 1]);
    turrets_payload.extend(encode_turret_angle(0.25).to_le_bytes());
    turrets_payload.extend(encode_turret_angle(-0.1).to_le_bytes());
    let mut doors_payload = REVISION.to_le_bytes().to_vec();
    doors_payload.extend([1u8, 1, 64, 2]);
    let mut own_pilot_payload = Vec::new();
    write_own_pilot(
        &fighter,
        &profile,
        0,
        Some(0),
        [0.1, -0.05],
        false,
        [0.0; 2],
        false,
        &bfe_sim::space::local::DoubleTapDetector::default(),
        &mut own_pilot_payload,
    )
    .unwrap();
    let mut own_chair_payload = Vec::new();
    write_own_chair(
        &OwnChairState {
            chair_building: 6,
            turret_building: 7,
            aim: [0.1, 0.0],
            feet: [0.0; 3],
            turret_yaw: 0.1,
            turret_pitch: 0.0,
            cooldown: 0.0,
            heat: 0.0,
            overheated: false,
            shots_fired: 0,
        },
        &mut own_chair_payload,
    )
    .unwrap();

    let space_plan = plan(&Table::of(WRITES_SPACE), &Supports::of(SUPPORTS)).unwrap();
    let checks = Checks::new(space_plan, vec![None; 4]);
    let mut receiver = Receiver::new(serde_json::json!({}));
    receiver.control_family = GameFamily::Swbf2;
    receiver.own.team = 1;
    receiver.own.generation = 1.0;
    receiver.own.alive = true;
    receiver.space_online = Some(Box::new(online));
    let mut sender = Sender::with_inputs(&[], WireInputs::new(GameFamily::Swbf2));
    sender.history.push(
        &ClientCommand::default(),
        &ClientLook::default(),
        &ClientFlightCommand::default(),
        12.0,
        1.0,
        2.0,
    );
    sender.history.mark_space_sent(2);

    // Warm one iteration before measuring.
    {
        let space = receiver.space_online.as_deref_mut().unwrap();
        space.receive_field(FIGHTER_SCHEMA, 0, &chunk_payload).unwrap();
        space.receive_field(FIGHTER_SCHEMA, SEATS_FIELD_ID, &seats_payload).unwrap();
        space.receive_field(FRIGATES_SCHEMA, 0, &frigates_payload).unwrap();
        space.receive_field(ORDNANCE_SCHEMA, 0, &guided_payload).unwrap();
        space.receive_field(ORDNANCE_SCHEMA, 1, &events_payload).unwrap();
        space.receive_field(PROPS_SCHEMA, 0, &turrets_payload).unwrap();
        space.receive_field(PROPS_SCHEMA, 1, &doors_payload).unwrap();
        space.receive_own(&own_pilot_payload, 1, 10).unwrap();
        space.publish_snapshot_headless(10, 1, &mut history);
    }
    predictor.frame(&receiver, &sender);
    predictor.present(0.5, 1.0 / 60.0);

    ALLOCS.with(|c| c.set(0));
    TRACK.with(|t| t.set(true));
    for step in 1..=20u32 {
        let tick = 10 + step * 3;
        checks.field(FIGHTER_SCHEMA, 0, &chunk_payload).unwrap();
        checks.field(FIGHTER_SCHEMA, SEATS_FIELD_ID, &seats_payload).unwrap();
        checks.field(FRIGATES_SCHEMA, 0, &frigates_payload).unwrap();
        checks.field(ORDNANCE_SCHEMA, 0, &guided_payload).unwrap();
        checks.field(ORDNANCE_SCHEMA, 1, &events_payload).unwrap();
        checks.field(PROPS_SCHEMA, 0, &turrets_payload).unwrap();
        checks.field(PROPS_SCHEMA, 1, &doors_payload).unwrap();
        {
            let space = receiver.space_online.as_deref_mut().unwrap();
            space.receive_field(FIGHTER_SCHEMA, 0, &chunk_payload).unwrap();
            space.receive_field(FIGHTER_SCHEMA, SEATS_FIELD_ID, &seats_payload).unwrap();
            space.receive_field(FRIGATES_SCHEMA, 0, &frigates_payload).unwrap();
            space.receive_field(ORDNANCE_SCHEMA, 0, &guided_payload).unwrap();
            space.receive_field(ORDNANCE_SCHEMA, 1, &events_payload).unwrap();
            space.receive_field(PROPS_SCHEMA, 0, &turrets_payload).unwrap();
            space.receive_field(PROPS_SCHEMA, 1, &doors_payload).unwrap();
            if step % 2 == 0 {
                space.receive_own(&own_pilot_payload, 1, tick).unwrap();
            } else {
                space.receive_own(&own_chair_payload, 1, tick).unwrap();
            }
            space.publish_snapshot_headless(tick, 1, &mut history);
        }
        predictor.frame(&receiver, &sender);
        predictor.present(0.5, 1.0 / 60.0);
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(
        ALLOCS.with(Cell::get),
        0,
        "steady Space receive_field/receive_own/publish_snapshot/predict must allocate nothing"
    );
}
