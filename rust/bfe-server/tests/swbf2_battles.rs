//! 2005 battles through the server. Each test hands the open gate of the
//! test-only crate to the worlds and servers it starts; nothing else in the
//! process sees it.
use bfe_server::udp_peer::INPUT_MAGIC;
use bfe_server::world_worker::{decode_entity_input, NativeWorldHost};
use bfe_test_gate::OPEN;
use std::fs;
use std::path::PathBuf;

fn temp_dir(prefix: &str) -> PathBuf {
    let bytes = bfe_transport::crypto::random::<8>().unwrap_or([0x11; 8]);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let dir = std::env::temp_dir().join(format!("bfe-server-test-{prefix}-{hex}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The world fields of one shared snapshot part: id, schema and full payload.
fn snapshot_world_fields(part: &[u8]) -> Vec<(u32, u16, Vec<u8>)> {
    assert_eq!(&part[..4], b"BNS1");
    fn read<'a>(part: &'a [u8], at: &mut usize, n: usize) -> &'a [u8] {
        *at += n;
        &part[*at - n..*at]
    }
    let mut at = if part[15] == 0 { 24 } else { 20 };
    let length = u16::from_le_bytes(read(part, &mut at, 2).try_into().unwrap()) as usize;
    let end = at + length;
    let (mut count, mut shift) = (0usize, 0);
    loop {
        let byte = read(part, &mut at, 1)[0];
        count |= usize::from(byte & 127) << shift;
        shift += 7;
        if byte & 128 == 0 {
            break;
        }
    }
    let mut out = Vec::new();
    for _ in 0..count {
        let id = u32::from_le_bytes(read(part, &mut at, 4).try_into().unwrap());
        let schema = u16::from_le_bytes(read(part, &mut at, 2).try_into().unwrap());
        read(part, &mut at, 4);
        let mode = read(part, &mut at, 1)[0];
        if mode == 1 {
            continue;
        }
        let length = u16::from_le_bytes(read(part, &mut at, 2).try_into().unwrap()) as usize;
        let payload = read(part, &mut at, length).to_vec();
        if mode == 0 {
            out.push((id, schema, payload));
        }
    }
    assert!(at <= end);
    out
}

/// The full own-correction payload of an own datagram, if it carries one.
fn own_correction(datagram: &[u8]) -> Option<Vec<u8>> {
    assert_eq!(&datagram[..4], b"BNO1");
    if datagram[20] & 1 == 0 {
        return None;
    }
    let section = &datagram[23..];
    // id, generation and the envelope schema; then a whole payload (mode 0).
    assert_eq!(u16::from_le_bytes([section[8], section[9]]), 11);
    if section[10] != 0 {
        return None;
    }
    let length = u16::from_le_bytes([section[11], section[12]]) as usize;
    Some(section[13..13 + length].to_vec())
}

/// One 2005 input packet for `tick` of a life: the codes held, and `edges` as
/// (id, kind, payload).
fn input_packet(generation: u32, tick: u32, held: &[usize], edges: &[(u32, u8, Vec<u8>)]) -> Vec<u8> {
    input_packet_looking(generation, tick, held, 0.0, edges)
}

/// The same packet with the player's running total of horizontal mouse counts.
fn input_packet_looking(generation: u32, tick: u32, held: &[usize], look_x: f64, edges: &[(u32, u8, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = INPUT_MAGIC.to_le_bytes().to_vec();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&generation.to_le_bytes());
    bytes.extend_from_slice(&[1, 1, edges.len() as u8, 0]);
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    bytes.extend_from_slice(&tick.to_le_bytes());
    bytes.extend_from_slice(&tick.to_le_bytes());
    bytes.push(0);
    let mut mask = [0u8; 16];
    for &held in held {
        mask[held / 8] |= 1 << (held % 8);
    }
    bytes.extend_from_slice(&mask);
    bytes.extend_from_slice(&look_x.to_le_bytes());
    bytes.extend_from_slice(&0f64.to_le_bytes());
    for (id, kind, payload) in edges {
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&tick.to_le_bytes());
        bytes.push(*kind);
        bytes.extend_from_slice(payload);
    }
    bytes
}

/// A 2005 battle end to end through the server's own world host: an invented
/// battle in the 2005 content layout, read by the 2005 loader under its
/// approved fingerprint, its classes given invented movement numbers (prepared
/// content does not carry them yet). The player's
/// 2005 packets (sprint held, then a roll pressed) pass the 2005 decoder, the
/// authority steps them, every own correction carries the soldier body 0x82 that the
/// client's strict reader accepts, and another player sees the same sprint and
/// roll in the shared display field 0x0203. The released decoder refuses the
/// same roll packet.
#[test]
fn a_2005_battle_sprints_and_rolls_through_the_server() {
    use bfe_family::GameFamily;
    use bfe_sim::movement::soldier::swbf2::{body, ControlSpeed, Tuning};
    use bfe_sim::prediction::corrections::{decode_compact_soldier_fixed, decode_own_player_header};
    use bfe_server::world_worker::decode_entity_input_for;
    use bfe_transport::layout::{SWBF2_INPUT_ACTIONS, SWBF2_INPUT_CODES};
    let root = temp_dir("invented-battle");
    common::invented_battle(&root);
    let query = "?map=yard&side=rep&ai=off&seed=1&mode=duel&era=cw";
    // The admission reads each soldier class's movement numbers from the
    // content: its properties and its roll clip's 37 frames.
    let row = |forward, strafe, turn| ControlSpeed { forward, strafe, turn };
    let tuning = Tuning {
        stand: row(1.0, 0.9, 1.0), crouch: row(0.6, 0.5, 0.9), sprint: row(1.6, 0.45, 0.6),
        jump: row(0.05, 0.05, 0.3), jet: row(0.5, 0.5, 0.5), jetpack: None, roll: row(0.0, 0.0, 0.25),
        tumble: row(0.0, 0.0, 0.1), sprint_accelerate_time: 0.3, energy_capacity: 50.0, energy_floor: 0.0, energy_restore: 5.0,
        energy_restore_idle: 7.5, energy_drain_sprint: 20.0, energy_min_sprint: 10.0, energy_cost_jump: 1.5,
        energy_cost_jump_sprint: None, energy_cost_roll: 10.0, jump_height: 1.6, jump_forward_speed_factor: 1.2,
        jump_strafe_speed_factor: 0.8, roll_speed_factor: 1.8, roll_frames: 37, recover_from_tumble: false,
        getup_front_seconds: bfe_sim::movement::soldier::swbf2::DEFAULT_GETUP_FRONT_SECONDS,
        getup_back_seconds: bfe_sim::movement::soldier::swbf2::DEFAULT_GETUP_BACK_SECONDS,
    };
    let mut host = NativeWorldHost::load_for(&root, query, 1, GameFamily::Swbf2, OPEN).unwrap();
    assert!(host.world_admitted);
    assert_eq!(host.family, GameFamily::Swbf2);
    let words = |t: &Tuning| bfe_sim::movement::soldier::swbf2::words::put(t).map(f64::to_bits);
    let admitted: Vec<_> = host.swbf2_classes().iter().map(|(class, t)| (class.clone(), words(t))).collect();
    assert_eq!(admitted, [("blue_trooper".to_string(), words(&tuning)), ("gold_scout".to_string(), words(&tuning))]);
    let classes = host.registry_descriptor["classes"].as_array().unwrap().clone();
    let welcome = host.add_player("human", "Human", false, false, false).unwrap();
    host.add_player("watcher", "Watcher", false, false, false).unwrap();
    let class = classes.iter().find(|entry| entry[1].as_str() == Some(welcome.class_name.as_str())).unwrap()[0]
        .as_u64()
        .unwrap() as u16;
    let code = |name: &str| SWBF2_INPUT_CODES.iter().position(|c| *c == name).unwrap();
    let (forward, sprint) = (code("KeyW"), code("ShiftLeft"));
    let roll = SWBF2_INPUT_ACTIONS.iter().position(|a| *a == "roll").unwrap() as u8;
    let tick_ms = 1000. / 60.;
    let mut now = 1000.;
    host.handle_start("human", now);
    host.handle_start("watcher", now);
    let spawn = [class.to_le_bytes(), 0u16.to_le_bytes()].concat();
    host.handle_input("human", &input_packet(welcome.generation, host.tick + 1, &[], &[(1, 3, spawn)]), now);
    let mut generation = None;
    for _ in 0..120 {
        now += tick_ms;
        host.step(now, (1_700_000_000_000.0 + now) as _).unwrap();
        generation = unit_rows(&host.context).unwrap().into_iter()
            .find(|(id, _, unit)| *id == welcome.unit_id && unit.alive)
            .map(|(_, _, unit)| unit.generation as u32);
        if generation.is_some() {
            break;
        }
    }
    let generation = generation.expect("the player spawns");
    // Run and sprint for a second and a half, run on, roll forward, run on.
    let (mut corrections, mut displays, mut unit_fields) = (Vec::new(), Vec::new(), 0);
    let mut roll_packet = None;
    for step in 0..300u32 {
        let tick = host.tick + 1;
        let held: &[usize] = if step < 90 { &[forward, sprint] } else { &[forward] };
        let edges = if step == 150 { vec![(2, 0, vec![roll])] } else { Vec::new() };
        let bytes = input_packet(generation, tick, held, &edges);
        if step == 150 {
            roll_packet = Some(bytes.clone());
        }
        host.handle_input("human", &bytes, now);
        now += tick_ms;
        let publication = host.step(now, (1_700_000_000_000.0 + now) as _).unwrap().unwrap();
        for own in publication.own_packets.iter().filter(|p| p.peer_id == "human") {
            let Some(payload) = own_correction(&own.bytes) else { continue };
            let head = decode_own_player_header(&payload).unwrap();
            if head.dead != 0 || head.removed != 0 {
                continue;
            }
            assert_eq!(head.body_schema, body::SCHEMA);
            let bytes = body::revision_body(head.body.get(&payload).unwrap()).unwrap();
            let split = body::compact_len(bytes).unwrap();
            let compact = decode_compact_soldier_fixed(Some(&bytes[..split]), &[true; 6]).unwrap();
            let state = body::read_suffix(&bytes[split..], &tuning, body::Body::of(&compact)).unwrap();
            corrections.push((step, state));
        }
        for group in publication.shared.iter().filter(|g| g.clients.iter().any(|c| c == "watcher")) {
            for part in &group.parts {
                for (id, schema, payload) in snapshot_world_fields(part) {
                    unit_fields += usize::from(id == 100_000 + welcome.unit_id && schema == 6);
                    if id == 100_000 + welcome.unit_id && schema == body::DISPLAY_SCHEMA {
                        let (revision, display) = bfe_sim::wire::revisions::split_revision(&payload).unwrap();
                        assert_eq!(revision, body::REVISION);
                        displays.push((step, body::read_display(display).unwrap()));
                    }
                }
            }
        }
    }
    let roll_packet = roll_packet.unwrap();
    assert!(decode_entity_input_for(&roll_packet, GameFamily::Swbf2).is_ok());
    assert_eq!(decode_entity_input_for(&roll_packet, GameFamily::Swbf1).err(), Some("unknown action"));
    assert_eq!(decode_entity_input(&roll_packet).err(), Some("unknown action"));
    // Sprinting drains energy; running on restores it; the roll costs it.
    let sprinting: Vec<_> = corrections.iter().filter(|(step, _)| *step < 90).collect();
    assert!(sprinting.iter().any(|(_, s)| s.sprint_time.is_some()));
    assert!(sprinting.last().unwrap().1.energy < tuning.energy_capacity - 10.);
    let before = corrections.iter().rfind(|(step, _)| (120..150).contains(step)).unwrap().1.energy;
    let rolling: Vec<_> = corrections.iter().filter(|(_, s)| s.roll.is_some()).collect();
    assert!(!rolling.is_empty(), "the roll reaches the authority and its own correction");
    assert!(rolling.iter().all(|(step, _)| (150..210).contains(step)));
    assert!(rolling[0].1.energy < before);
    assert!(corrections.iter().filter(|(step, _)| *step > 260).all(|(_, s)| s.roll.is_none() && s.sprint_time.is_none()));
    // Another player sees the same sprint and roll.
    assert!(displays.iter().any(|(step, d)| *step < 90 && d.sprint));
    let shown = displays.iter().find_map(|(_, d)| d.roll).expect("the roll reaches another player");
    assert!((host.tick - 300..host.tick).contains(&shown.1));
    println!("2005 battle: {} own corrections ({} rolling), {} display fields, {unit_fields} unit fields", corrections.len(), rolling.len(), displays.len());
    // His body leaves with him, and both of its fields with it: what a player
    // who joins later must receive whole holds neither.
    assert!(unit_fields > 0);
    host.remove_player("human");
    host.add_player("late", "Late", false, false, false).unwrap();
    host.handle_start("late", now);
    let mut held = Vec::new();
    for _ in 0..90 {
        now += tick_ms;
        let publication = host.step(now, (1_700_000_000_000.0 + now) as _).unwrap().unwrap();
        for group in publication.shared.iter().filter(|g| g.clients.iter().any(|c| c == "late")) {
            held.extend(group.parts.iter().flat_map(|part| snapshot_world_fields(part)).map(|(id, schema, _)| (id, schema)));
        }
    }
    assert!(held.iter().any(|(_, schema)| *schema == 1), "the joiner is sent whole snapshots");
    assert!(held.iter().all(|(id, _)| *id != 100_000 + welcome.unit_id), "{held:?}");
    host.remove_player("late");
    host.remove_player("watcher");
    // Finished and empty, it restarts as the same 2005 battle in a fresh
    // epoch: the same game, content fingerprint and class numbers.
    let fingerprint = host.fingerprint.clone();
    host.ended = true;
    assert!(host.restart_finished_empty().unwrap());
    assert_eq!(host.swbf2_classes().iter().map(|(class, t)| (class.clone(), words(t))).collect::<Vec<_>>(), admitted);
    assert_eq!((host.family, host.epoch, host.fingerprint.as_str(), host.world_admitted), (GameFamily::Swbf2, 2, fingerprint.as_str(), true));
    let again = host.add_player("again", "Again", false, false, false).unwrap();
    assert_eq!(again.class_name, welcome.class_name);
    host.remove_player("again");
    // One bound of the map changed under the same selection. The
    // reconstructed world is refused and the accepted one stays; with the
    // content back, the battle restarts again.
    let map_json = root.join("maps").join("yard").join("map.json");
    let accepted = fs::read_to_string(&map_json).unwrap();
    fs::write(&map_json, accepted.replacen("[1000,400,1000]", "[1001,400,1000]", 1)).unwrap();
    host.ended = true;
    assert_eq!(host.restart_finished_empty().unwrap_err(), "the battle's content changed: it cannot restart as the same battle");
    assert_eq!((host.epoch, host.fingerprint.as_str()), (2, fingerprint.as_str()));
    fs::write(&map_json, accepted).unwrap();
    assert!(host.restart_finished_empty().unwrap());
    assert_eq!((host.epoch, host.fingerprint.as_str()), (3, fingerprint.as_str()));
    // The built gate refuses the same battle before any content is read.
    assert_eq!(NativeWorldHost::load_for(&root, query, 1, GameFamily::Swbf2, bfe_family::Gate::BUILT).err().as_deref(),
        Some("Battlefront II (2005) is not available in this build"));
    let _ = fs::remove_dir_all(&root);
}

/// A 2005 soldier's look through the server: the mouse counts of the player's
/// packets turn him by the row of the state he is in (walking, sprinting,
/// rolling), and the client's replay of the server's own correction, its real
/// bytes, names the same row for the next tick, so the frames drawn between two
/// ticks turn him as the next tick does.
#[test]
fn a_2005_soldiers_look_is_turned_by_his_state_and_his_clients_replay_names_the_same_row() {
    use bfe_family::GameFamily;
    use bfe_sim::client::ClientPrediction;
    use bfe_sim::collision::CollisionOwner;
    use bfe_sim::gen::{ClientFrame, Soldier, UnitBody};
    use bfe_sim::movement::soldier::swbf2::body;
    use bfe_sim::prediction::corrections::decode_own_player_header;
    use bfe_transport::layout::{SWBF2_INPUT_ACTIONS, SWBF2_INPUT_CODES};
    const LOOK: f64 = 2.0;
    let root = temp_dir("look-battle");
    common::invented_battle(&root);
    let query = "?map=yard&side=rep&ai=off&seed=1&mode=duel&era=cw";
    let mut host = NativeWorldHost::load_for(&root, query, 1, GameFamily::Swbf2, OPEN).unwrap();
    let welcome = host.add_player("human", "Human", false, false, false).unwrap();
    let tuning = host.swbf2_classes().iter().find(|(class, _)| *class == welcome.class_name).unwrap().1;
    let classes = host.registry_descriptor["classes"].as_array().unwrap().clone();
    let class = classes.iter().find(|entry| entry[1].as_str() == Some(welcome.class_name.as_str())).unwrap()[0].as_u64().unwrap() as u16;
    let code = |name: &str| SWBF2_INPUT_CODES.iter().position(|c| *c == name).unwrap();
    let (forward, sprint) = (code("KeyW"), code("ShiftLeft"));
    let roll = SWBF2_INPUT_ACTIONS.iter().position(|a| *a == "roll").unwrap() as u8;
    let tick_ms = 1000. / 60.;
    let mut now = 1000.;
    host.handle_start("human", now);
    let spawn = [class.to_le_bytes(), 0u16.to_le_bytes()].concat();
    host.handle_input("human", &input_packet(welcome.generation, host.tick + 1, &[], &[(1, 3, spawn)]), now);
    let mut generation = None;
    for _ in 0..120 {
        now += tick_ms;
        host.step(now, (1_700_000_000_000.0 + now) as _).unwrap();
        generation = unit_rows(&host.context).unwrap().into_iter()
            .find(|(id, _, unit)| *id == welcome.unit_id && unit.alive)
            .map(|(_, _, unit)| unit.generation as u32);
        if generation.is_some() {
            break;
        }
    }
    let generation = generation.expect("the player spawns");
    // Counts every tick; sprint through ticks 0 to 89, walk on, one roll at 150.
    let mut history: Vec<(Soldier, Option<Vec<u8>>)> = Vec::new();
    for step in 0..300u32 {
        let held: &[usize] = if step < 90 { &[forward, sprint] } else { &[forward] };
        let edges = if step == 150 { vec![(2, 0, vec![roll])] } else { Vec::new() };
        let bytes = input_packet_looking(generation, host.tick + 1, held, LOOK * f64::from(step + 1), &edges);
        host.handle_input("human", &bytes, now);
        now += tick_ms;
        let publication = host.step(now, (1_700_000_000_000.0 + now) as _).unwrap().unwrap();
        let UnitBody::Soldier(soldier) = unit_rows(&host.context).unwrap().into_iter()
            .find(|(id, _, unit)| *id == welcome.unit_id && unit.alive).unwrap().2.soldier else { panic!("a soldier") };
        let mut correction = None;
        for own in publication.own_packets.iter().filter(|p| p.peer_id == "human") {
            let Some(payload) = own_correction(&own.bytes) else { continue };
            let head = decode_own_player_header(&payload).unwrap();
            if head.dead == 0 && head.removed == 0 {
                assert_eq!(head.body_schema, body::SCHEMA);
                correction = Some(head.body.get(&payload).unwrap().to_vec());
            }
        }
        history.push((soldier, correction));
    }
    // The client's side: restore the correction's bytes in a replay that runs no command and read the
    // look scale it gives the next tick's counts.
    let mut collision = CollisionOwner::new();
    let world = collision.world_new(0, -1, -1);
    // One prediction per correction: a client that restores a correction is one that has none yet.
    let mut replayed_scale = |tick: usize, soldier: &Soldier, bytes: &[u8]| {
        let mut prediction = ClientPrediction::default();
        prediction.configure_actor_for_family(&[0.], GameFamily::Swbf2).unwrap();
        prediction.configure_swbf2(tuning).unwrap();
        let mut words = vec![0.; ClientFrame::WORDS + Soldier::WORDS];
        soldier.put(&mut words[ClientFrame::WORDS..], &mut Vec::new());
        let correction_at = words.len() as u32;
        for chunk in bytes.chunks(4) {
            let mut padded = [0; 4];
            padded[..chunk.len()].copy_from_slice(chunk);
            words.push(u32::from_le_bytes(padded).into());
        }
        let weapons_at = words.len() as u32;
        words.extend([0., 0., 0., 0., 0.]);
        let frame = ClientFrame { epoch: 1., identity: 1., generation: 1., source: 1., movement: true, world,
            ignored: -1., authority_tick: tick as f64, actor_at: ClientFrame::WORDS as u32, actor_words: Soldier::WORDS as u32,
            correction_at, correction_bytes: bytes.len() as u32, weapons_at, weapons_words: 5, mouse_scale: 1.,
            ..Default::default() };
        frame.put(&mut words, &mut Vec::new());
        prediction.predict_session(&words, &mut collision.state).unwrap();
        prediction.swbf2_look_scale().expect("a replayed 2005 soldier has a look scale")
    };
    let turn = |t: usize| {
        let mut d = history[t].0.aim_yaw - history[t - 1].0.aim_yaw;
        while d > std::f64::consts::PI { d -= 2. * std::f64::consts::PI; }
        while d < -std::f64::consts::PI { d += 2. * std::f64::consts::PI; }
        d
    };
    // What the walking soldier's counts turn him by in a tick.
    let standing = turn(130);
    assert!(standing.abs() > 1e-6, "the counts turn the walking soldier");
    let mut seen: Vec<f64> = Vec::new();
    let mut checked = 0;
    for (t, (soldier, correction)) in history.iter().enumerate().take(299).skip(2) {
        let Some(bytes) = correction else { continue };
        let scale = replayed_scale(t, soldier, bytes);
        assert!((turn(t + 1) - standing * scale).abs() < 1e-9,
            "tick {t}: the server turned him {} for the counts, the replay's scale {scale} gives {}", turn(t + 1), standing * scale);
        checked += 1;
        if !seen.iter().any(|s| (s - scale).abs() < 1e-12) {
            seen.push(scale);
        }
    }
    println!("2005 look: {checked} corrections replayed, the scales they name: {seen:?}");
    assert!(checked > 80, "the server's corrections reach the replay on every third tick: {checked}");
    for row in [1.0, tuning.sprint.turn, tuning.roll.turn] {
        assert!(seen.iter().any(|s| (s - row).abs() < 1e-12), "no tick of the run had the scale {row}: {seen:?}");
    }
    let _ = fs::remove_dir_all(&root);
}

/// A scripted 2005 battle (a mode the script host runs) is admitted through
/// the server's own loader from the content its digest covers, runs its
/// mission to the mission's own end, and restarts as the same battle: the same
/// query, gate, digest and mission, in a fresh epoch. A map with the mode in
/// both eras runs the mission of the battle's era.
#[test]
fn a_scripted_battle_runs_the_mission_its_digest_covers() {
    use bfe_content::files::DirFiles;
    use bfe_content_native::fingerprint::hash_for_family;
    use bfe_family::{Availability, Gate, GameFamily};
    use std::sync::atomic::{AtomicBool, Ordering};
    static SHUT: AtomicBool = AtomicBool::new(false);
    struct Door;
    impl Availability for Door {
        fn plays(&self, _: GameFamily) -> bool { !SHUT.load(Ordering::SeqCst) }
        fn name(&self) -> &'static str { "a door" }
    }
    const DOOR: Gate = Gate::new(&Door);

    let root = temp_dir("scripted");
    let content = GameFamily::Swbf2.content_dir(&root);
    common::invented_mission(&content, &["cw", "gcw"]);
    let files = DirFiles::from_resolved_root(&content, GameFamily::Swbf2);
    let mut digests = Vec::new();
    for (side, era) in [("rep", "cw"), ("all", "gcw")] {
        let query = format!("?map=swbf2/yard&side={side}&ai=off&seed=1&mode=eli&match=0b");
        let mut host = NativeWorldHost::load_battle(&root, GameFamily::Swbf2, &query, 1, DOOR).unwrap();
        assert!(host.world_admitted);
        let selected = host.mission_selection().unwrap().clone();
        assert_eq!((selected.map.as_str(), selected.era.as_str(), selected.mode.as_str()), ("yard", era, "eli"));
        let (_, content_query) = bfe_server::config::swbf2_content(&root, &query).unwrap();
        let digest = hash_for_family(&files, GameFamily::Swbf2, &content_query).unwrap();
        assert_eq!((host.query.as_str(), host.fingerprint.as_str()), (query.as_str(), digest.as_str()));
        digests.push(digest.clone());

        // The belfry's fifth turn, 7.5 seconds in, rings its bell; the battle
        // is finished nine seconds later.
        let mut now = 1000.;
        let mut ended = None;
        for tick in 1..=1100u32 {
            now += 1000. / 60.;
            host.step(now, (1_700_000_000_000.0 + now) as _).unwrap();
            if host.ended {
                ended = Some((tick, host.winner));
                break;
            }
        }
        let (tick, winner) = ended.expect("the mission ends by its own script");
        assert!((985..=995).contains(&tick) && winner == Some(1), "tick {tick}, winner {winner:?}");

        // A shut gate keeps the finished battle; opened, the same battle
        // restarts, plays and ends again.
        SHUT.store(true, Ordering::SeqCst);
        assert_eq!(host.restart_finished_empty().unwrap_err(), "Battlefront II (2005) is not available in this build");
        assert_eq!(host.epoch, 1);
        SHUT.store(false, Ordering::SeqCst);
        assert!(host.restart_finished_empty().unwrap());
        assert_eq!((host.epoch, host.query.as_str(), host.fingerprint.as_str(), host.ended), (2, query.as_str(), digest.as_str(), false));
        assert_eq!(host.mission_selection(), Some(&selected));
        for _ in 0..1100 {
            now += 1000. / 60.;
            host.step(now, (1_700_000_000_000.0 + now) as _).unwrap();
        }
        assert!(host.ended && host.winner == Some(1));

        // Content that changed under the battle does not restart as it.
        let map_json = content.join("maps/yard/map.json");
        let accepted = fs::read_to_string(&map_json).unwrap();
        fs::write(&map_json, accepted.replacen("[1000,400,1000]", "[1001,400,1000]", 1)).unwrap();
        assert_eq!(host.restart_finished_empty().unwrap_err(), "the battle's content changed: it cannot restart as the same battle");
        assert_eq!(host.epoch, 2);
        fs::write(&map_json, accepted).unwrap();
    }
    assert_ne!(digests[0], digests[1], "each era's digest covers its own mission");

    // A map whose digest covers another mission than the one its scripts run
    // is refused: here a mission of the same mode and era sorts first.
    let map_json = content.join("maps/yard/map.json");
    let accepted = fs::read_to_string(&map_json).unwrap();
    fs::write(&map_json, accepted.replacen("\"missions\":{", "\"missions\":{\"aisle_cw\":{\"mode\":\"eli\",\"era\":\"cw\",\"layers\":[],\"sides\":{\"rep\":[],\"cis\":[]}},", 1)).unwrap();
    let query = "?map=swbf2/yard&side=rep&ai=off&seed=1&mode=eli";
    assert_eq!(NativeWorldHost::load_battle(&root, GameFamily::Swbf2, query, 1, DOOR).err().as_deref(),
        Some("the cw eli mission of yard is belfry_cw, but the battle's content digest covers aisle_cw"));
    let _ = fs::remove_dir_all(&root);
}

/// A scripted battle's one player, a tick at a time.
struct Lone {
    host: NativeWorldHost,
    now: f64,
    life: u32,
    edge: u32,
}
/// A tick, the player's living body (its unit and team) and what the wire says
/// of the body he waits for.
type Tick = (u32, Option<(u32, u8)>, Option<bfe_sim::wire::mission::Deployment>);
impl Lone {
    fn turn(&mut self, edges: Vec<(u8, Vec<u8>)>) -> Tick {
        use bfe_sim::wire::mission::{read_state, State, STATE_SCHEMA};
        let edges: Vec<_> = edges.into_iter().map(|(kind, payload)| { self.edge += 1; (self.edge, kind, payload) }).collect();
        self.host.handle_input("one", &input_packet(self.life, self.host.tick + 1, &[], &edges), self.now);
        self.now += 1000. / 60.;
        let publication = self.host.step(self.now, (1_700_000_000_000.0 + self.now) as _).unwrap().unwrap();
        assert!(publication.faults.is_empty(), "{:?}", publication.faults);
        let mut waits = None;
        for part in publication.shared.iter().filter(|g| g.clients.iter().any(|c| c == "one")).flat_map(|g| &g.parts) {
            for (id, _, payload) in snapshot_world_fields(part).into_iter().filter(|(_, schema, _)| *schema == STATE_SCHEMA) {
                let state = read_state(&payload[2..]).unwrap();
                assert_eq!(state.id(), id);
                if let State::Deployments(deployments) = state {
                    assert!(deployments.rows.iter().flatten().count() <= 1, "one player is in the battle");
                    waits = waits.or(deployments.rows.iter().flatten().next().copied());
                }
            }
        }
        let body = unit_rows(&self.host.context).unwrap().into_iter().find(|(_, _, unit)| unit.human && unit.alive);
        // A new life counts its edges from one again, as a client does.
        if let Some((_, _, unit)) = &body {
            if self.life != unit.generation as u32 { (self.life, self.edge) = (unit.generation as u32, 0); }
        }
        (self.host.tick, body.map(|(id, _, unit)| (id, unit.team as u8)), waits)
    }
    /// Ticks in which he does nothing: what he waits for at the last of them
    /// that says so.
    fn idle(&mut self, ticks: u32) -> Option<bfe_sim::wire::mission::Deployment> {
        let mut last = None;
        for _ in 0..ticks {
            let (_, body, waits) = self.turn(vec![]);
            assert_eq!(body, None, "nobody deploys by himself");
            last = waits.or(last);
        }
        last
    }
}

/// A scripted battle deploys its players as its mission does: what a player
/// chooses waits for the spawn wave after his own wait, he may choose again
/// meanwhile and must after changing teams, and the mission state on the wire
/// says the tick he deploys on.
#[test]
fn a_scripted_battle_deploys_its_players_with_its_waves() {
    use bfe_family::GameFamily;
    use bfe_sim::wire::mission::Deployment;
    let root = temp_dir("deploys");
    let content = GameFamily::Swbf2.content_dir(&root);
    common::invented_mission(&content, &["cw"]);
    common::replace_mission_script(&content, "cw", include_bytes!("../../../tests/fixtures/mission-scripts/vigil.luac"));
    let mut host = NativeWorldHost::load_battle(&root, GameFamily::Swbf2, "?map=swbf2/yard&side=rep&ai=off&seed=1&mode=eli", 1, OPEN).unwrap();
    let welcome = host.add_player("one", "One", false, false, true).unwrap();
    host.handle_start("one", 1000.);
    let classes = host.registry_descriptor["classes"].as_array().unwrap().clone();
    let choice = |class: &str| {
        let id = classes.iter().find(|entry| entry[1].as_str() == Some(class)).unwrap()[0].as_u64().unwrap() as u16;
        (3u8, [id.to_le_bytes(), 0u16.to_le_bytes()].concat())
    };
    let mut player = Lone { host, now: 1000., life: welcome.generation, edge: 0 };
    // He waits for nothing but his own choice: his place's wave has opened.
    assert_eq!(player.idle(40), Some(Deployment { due: 0, chosen: false }));
    let mut deployed = player.turn(vec![choice(&welcome.class_name)]);
    for _ in 0..20 {
        if deployed.1.is_none() { deployed = player.turn(vec![]); }
    }
    let (_, team) = deployed.1.expect("his choice deploys him");
    assert_eq!(team, welcome.team);

    for redeploy in 0..2 {
        // He falls; the wave after his four seconds is 15 seconds after the last.
        assert_eq!((0..40).filter_map(|_| player.turn(vec![]).2).next(), None, "nobody waits");
        let mut fallen = player.turn(vec![(4, vec![])]);
        for _ in 0..4 {
            if fallen.1.is_some() { fallen = player.turn(vec![]); }
        }
        assert_eq!(fallen.1, None);
        let waits = player.idle(30).expect("he waits");
        assert!(!waits.chosen && waits.due > fallen.0 + 240 && waits.due <= fallen.0 + 240 + 900 + 30, "{waits:?} after {}", fallen.0);
        // A choice before his own wait is over is no choice.
        player.turn(vec![choice(&welcome.class_name)]);
        assert_eq!(player.idle(60), Some(waits));
        assert_eq!(player.idle(160), Some(waits));
        player.turn(vec![choice(&welcome.class_name)]);
        assert_eq!(player.idle(20), Some(Deployment { chosen: true, ..waits }));
        let mut class = welcome.class_name.clone();
        if redeploy == 1 {
            // Across to the other team he has chosen nothing.
            player.turn(vec![(2, vec![3 - team])]);
            assert_eq!(player.idle(20), Some(waits));
            class = if class == "rep_winder" { "cis_winder".into() } else { "rep_winder".into() };
        }
        player.turn(vec![choice(&class)]);
        assert_eq!(player.idle(20), Some(Deployment { chosen: true, ..waits }));
        let mut back = player.turn(vec![]);
        while back.1.is_none() && back.0 < waits.due + 60 { back = player.turn(vec![]); }
        assert_eq!((back.0, back.1.map(|(_, team)| team)), (waits.due, Some(if redeploy == 1 { 3 - team } else { team })));
    }
    let _ = fs::remove_dir_all(&root);
}

/// A scripted battle takes every player its teams have room for: each one's
/// body steps from the tick it spawns on, however many bodies the battle was
/// admitted with, and a player past the last is refused alone.
#[test]
fn a_scripted_battle_takes_a_full_house_of_players() {
    use bfe_family::GameFamily;
    let root = temp_dir("full-house");
    let content = GameFamily::Swbf2.content_dir(&root);
    common::invented_mission(&content, &["cw"]);
    common::replace_mission_script(&content, "cw", include_bytes!("../../../tests/fixtures/mission-scripts/vigil.luac"));
    let mut host = NativeWorldHost::load_battle(&root, GameFamily::Swbf2, "?map=swbf2/yard&side=rep&ai=off&seed=1&mode=eli", 1, OPEN).unwrap();
    let mut now = 1000.;
    let (mut joined, mut refused) = (0usize, None);
    while refused.is_none() && joined < 80 {
        let id = format!("{joined:064x}");
        match host.add_player(&id, "Player", false, true, false) {
            Ok(_) => { host.handle_start(&id, now); joined += 1; }
            Err(why) => refused = Some(why),
        }
        for _ in 0..3 {
            now += 1000. / 60.;
            host.step(now, (1_700_000_000_000.0 + now) as _).unwrap();
        }
    }
    // A side's players deploy one after another, half a second apart.
    let (mut alive, mut ticks) = (0, 0);
    while alive < joined && ticks < 32 * 30 {
        now += 1000. / 60.;
        assert!(host.step(now, (1_700_000_000_000.0 + now) as _).unwrap().unwrap().faults.is_empty());
        alive = unit_rows(&host.context).unwrap().iter().filter(|(_, _, unit)| unit.human && unit.alive).count();
        ticks += 1;
    }
    println!("{joined} players joined, {alive} alive after {ticks} ticks, then {refused:?}");
    assert!(joined >= 32 && refused.is_some(), "{joined} {refused:?}");
    assert_eq!(alive, joined, "every player deploys");
    for _ in 0..120 {
        now += 1000. / 60.;
        assert!(host.step(now, (1_700_000_000_000.0 + now) as _).unwrap().unwrap().faults.is_empty());
    }
    let _ = fs::remove_dir_all(&root);
}

/// A mission whose script fails as it starts ends its battle as a draw, parks
/// its world and takes no further joins; the service schedules what follows.
#[test]
fn a_scripted_battle_that_fails_to_start_parks() {
    use bfe_family::GameFamily;
    let root = temp_dir("scripted-park");
    let content = GameFamily::Swbf2.content_dir(&root);
    common::invented_mission(&content, &["cw"]);
    common::replace_mission_script(&content, "cw", include_bytes!("../../../tests/fixtures/mission-scripts/snag.luac"));
    let query = "?map=swbf2/yard&side=rep&ai=off&seed=1&mode=eli";
    let mut host = NativeWorldHost::load_battle(&root, GameFamily::Swbf2, query, 1, OPEN).unwrap();
    let mut now = 1000.;
    for _ in 0..4 {
        now += 1000. / 60.;
        host.step(now, (1_700_000_000_000.0 + now) as _).unwrap();
    }
    assert_eq!((host.ended, host.winner), (true, Some(0)));
    let stopped = host.tick;
    for _ in 0..60 {
        now += 1000. / 60.;
        assert!(host.step(now, (1_700_000_000_000.0 + now) as _).unwrap().is_none());
    }
    assert_eq!(host.tick, stopped);
    assert_eq!(host.add_player("late", "Late", false, false, false).err().as_deref(), Some("The mission failed to start."));
    assert!(!host.restart_finished_empty().unwrap());
    let _ = fs::remove_dir_all(&root);
}

/// A 2005 world whose admission is refused is not loaded: the refusal names
/// the class and what its content lacks. With content that gives every class
/// its numbers, the world loads.
#[test]
fn a_refused_2005_admission_is_the_load_error() {
    use bfe_family::GameFamily;
    let root = temp_dir("refused-admission");
    let query = "?map=yard&side=rep&ai=off&seed=1&mode=duel&era=cw";
    common::invented_battle_rolling(&root, 25, false);
    assert_eq!(NativeWorldHost::load_for(&root, query, 1, GameFamily::Swbf2, OPEN).err().as_deref(),
        Some("Session: the Battlefront II class blue_trooper has no movement numbers: missing ControlSpeed"));
    common::invented_battle_rolling(&root, 25, true);
    assert!(NativeWorldHost::load_for(&root, query, 1, GameFamily::Swbf2, OPEN).unwrap().world_admitted);
    let _ = fs::remove_dir_all(&root);
}

/// The classes' movement numbers both peers use come from the content the
/// match's digest covers. Changing the classes' forward roll clip from 25 to
/// 26 frames changes the digest the client's hello carries and the server
/// compares, and the numbers the server admits.
#[test]
fn a_roll_clip_of_another_length_changes_the_digest_the_peers_compare() {
    use bfe_content::files::DirFiles;
    use bfe_family::GameFamily;
    let query = "?map=yard&side=rep&ai=off&seed=1&mode=duel&era=cw";
    let mut seen = Vec::new();
    for frames in [25, 26] {
        let root = temp_dir("roll-clip");
        common::invented_battle_rolling(&root, frames, true);
        let host = NativeWorldHost::load_for(&root, query, 1, GameFamily::Swbf2, OPEN).unwrap();
        let hello = bfe_content_native::fingerprint::hash_for_family(&DirFiles::from_resolved_root(&root, GameFamily::Swbf2), GameFamily::Swbf2, query).unwrap();
        assert_eq!(hello, host.fingerprint, "the client's digest of the same content is the server's");
        let rolls: Vec<_> = host.swbf2_classes().iter().map(|(class, t)| (class.clone(), t.roll_frames)).collect();
        assert_eq!(rolls, [("blue_trooper".to_string(), frames as u32), ("gold_scout".to_string(), frames as u32)]);
        seen.push(host.fingerprint.clone());
        let _ = fs::remove_dir_all(&root);
    }
    assert_ne!(seen[0], seen[1]);
}

/// A 2005 class's jetpack and its body's acceleration come from its
/// properties by the one rule local play uses, and the digest the peers
/// compare covers them: an invented jet class (energy powered, hovering, its
/// own jet acceleration, body acceleration 90) is admitted with that jetpack
/// and spawns with that acceleration; a jet class without them has none; one
/// jet number changed changes the digest.
#[test]
fn a_jet_class_takes_its_jetpack_and_acceleration_from_the_content_the_digest_covers() {
    use bfe_content::files::DirFiles;
    use bfe_family::GameFamily;

    let query = "?map=yard&side=rep&ai=off&seed=1&mode=duel&era=cw";
    let mut seen = Vec::new();
    for push in ["8", "9"] {
        let root = temp_dir("jet-class");
        common::invented_battle(&root);
        let units = root.join("sides/rep/units.json");
        let mut doc: serde_json::Value = serde_json::from_slice(&fs::read(&units).unwrap()).unwrap();
        let properties = doc["units"]["blue_trooper"]["properties"].as_array_mut().unwrap();
        for (key, value) in [("JetJump", "5"), ("JetPush", push), ("JetType", "hover"), ("JetEnergyDrain", "12"),
            ("JetFuelCost", "0.2"), ("JetAcceleration", "40"), ("Acceleration", "90")] {
            properties.push(serde_json::json!([key, value]));
        }
        fs::write(&units, serde_json::to_vec(&doc).unwrap()).unwrap();
        let mut host = NativeWorldHost::load_for(&root, query, 1, GameFamily::Swbf2, OPEN).unwrap();
        let hello = bfe_content_native::fingerprint::hash_for_family(&DirFiles::from_resolved_root(&root, GameFamily::Swbf2), GameFamily::Swbf2, query).unwrap();
        assert_eq!(hello, host.fingerprint, "the client's digest of the same content is the server's");
        let classes = host.swbf2_classes().to_vec();
        assert_eq!((classes[0].0.as_str(), classes[1].0.as_str()), ("blue_trooper", "gold_scout"));
        let pack = classes[0].1.jetpack.expect("the jet class's jetpack");
        assert_eq!((pack.physics.jump, pack.physics.push, pack.physics.cost, pack.physics.hover, pack.acceleration),
            (5., push.parse::<f64>().unwrap(), 0.2, true, 40.));
        assert_eq!((pack.energy_cost, pack.force_jump), (12., false));
        assert!(classes[1].1.jetpack.is_none());
        // The jet class's soldier spawns with its body acceleration.
        let descriptor = host.registry_descriptor["classes"].as_array().unwrap().clone();
        let welcome = host.add_player("human", "Human", false, false, false).unwrap();
        let jet = descriptor.iter().find(|entry| entry[1].as_str() == Some("blue_trooper")).unwrap()[0].as_u64().unwrap() as u16;
        let mut now = 1000.;
        host.handle_start("human", now);
        let mut spawn = INPUT_MAGIC.to_le_bytes().to_vec();
        spawn.extend_from_slice(&1u32.to_le_bytes());
        spawn.extend_from_slice(&welcome.generation.to_le_bytes());
        spawn.extend_from_slice(&[1, 1, 1, 0]);
        spawn.extend_from_slice(&32u16.to_le_bytes());
        spawn.extend_from_slice(&0u16.to_le_bytes());
        spawn.extend_from_slice(&1.0f64.to_le_bytes());
        spawn.extend_from_slice(&(host.tick + 1).to_le_bytes());
        spawn.extend_from_slice(&(host.tick + 1).to_le_bytes());
        spawn.push(0);
        spawn.extend_from_slice(&[0; 16]);
        spawn.extend_from_slice(&0f64.to_le_bytes());
        spawn.extend_from_slice(&0f64.to_le_bytes());
        spawn.extend_from_slice(&1u32.to_le_bytes());
        spawn.extend_from_slice(&(host.tick + 1).to_le_bytes());
        spawn.push(3);
        spawn.extend_from_slice(&[jet.to_le_bytes(), 0u16.to_le_bytes()].concat());
        host.handle_input("human", &spawn, now);
        let mut accel = None;
        for _ in 0..120 {
            now += 1000. / 60.;
            host.step(now, (1_700_000_000_000.0 + now) as _).unwrap();
            accel = unit_rows(&host.context).unwrap().into_iter().find(|(id, _, unit)| *id == welcome.unit_id && unit.alive)
                .and_then(|(_, _, unit)| match unit.soldier { bfe_sim::gen::UnitBody::Soldier(soldier) => Some(soldier.accel), _ => None });
            if accel.is_some() { break; }
        }
        assert_eq!(accel, Some(90.), "the jet class's soldier takes its body acceleration");
        seen.push(host.fingerprint.clone());
        let _ = fs::remove_dir_all(&root);
    }
    assert_ne!(seen[0], seen[1], "the digest covers the jet numbers");
}

/// The store's unit rows (`KIND_UNIT`, full): slot, store generation, row.
fn unit_rows(context: &bfe_sim::abi::context::Context) -> Result<Vec<(u32, u32, bfe_sim::gen::Unit)>, String> {
    use bfe_sim::gen::BlockHeader as B;
    let units = context.export(u32::from(bfe_sim::gen::KIND_UNIT), 1).map_err(|e| e.message().to_string())?;
    let stride = units[B::STRIDE] as usize;
    let rows = units[B::ROWS] as usize;
    let heap_at = units[B::HEAP] as usize;
    let heap = &units[heap_at..heap_at + units[B::HEAP_WORDS] as usize];
    let mut out = Vec::with_capacity(rows);
    for i in 0..rows {
        let row = &units[B::WORDS + i * stride..B::WORDS + (i + 1) * stride];
        let mut unit = bfe_sim::gen::Unit::default();
        unit.take_full(row, heap);
        out.push((row[0] as u32, row[1] as u32, unit));
    }
    Ok(out)
}

mod common;

/// What one join saw: the server's signed answer, the reliable messages, the
/// snapshots completed through the receiver and any record it refused.
struct Seen {
    answer: serde_json::Value,
    events: Vec<serde_json::Value>,
    complete: usize,
    refused: Vec<String>,
    closed: Option<(u64, String)>,
}

fn info_of(address: std::net::SocketAddr) -> (Vec<u8>, serde_json::Value) {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_secs(2)).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
    stream.write_all(b"GET /info HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n").unwrap();
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).unwrap();
    let at = answer.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let body = answer[at..].to_vec();
    let value = serde_json::from_slice(&body).unwrap();
    (body, value)
}

/// Joins with `hello`, starts, and reads for `seconds`: state datagrams go
/// through `receiver` once the registry names the classes, `checks` builds
/// the 2005 record checks from the registry and the signed answer.
fn join(address: std::net::SocketAddr, server: [u8; 32], hello: &[u8], seconds: u64,
    mut checks: impl FnMut(&serde_json::Value, &serde_json::Value) -> Option<Box<dyn bfe_presentation::network::RecordChecks>>,
    bounds: bfe_sim::wire::PositionBounds) -> Seen
{
    use bfe_presentation::network_layout::net_header as h;
    use bfe_transport::{connection::Lane, crypto::Identity, endpoint::Endpoint};
    use std::time::{Duration, Instant};
    let mut endpoint = Endpoint::connect(&address.to_string(), Identity::generate().unwrap(), server, hello).unwrap();
    let (mut events, mut state): (Vec<serde_json::Value>, Vec<Vec<u8>>) = (Vec::new(), Vec::new());
    fn take(lane: Lane, text: bool, bytes: &[u8], events: &mut Vec<serde_json::Value>, state: &mut Vec<Vec<u8>>) {
        if text { if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) { events.push(value); } }
        else if lane == Lane::State { state.push(bytes.to_vec()); }
    }
    let established = endpoint.establish(Duration::from_secs(3), &mut |lane, text, bytes, _| take(lane, text, bytes, &mut events, &mut state));
    let answer = serde_json::from_slice(&endpoint.server_hello).unwrap_or(serde_json::Value::Null);
    let mut seen = Seen { answer, events: Vec::new(), complete: 0, refused: Vec::new(), closed: None };
    if established.is_err() {
        seen.closed = endpoint.connection.as_ref().and_then(|c| c.closed).map(|c| (c.code, String::from_utf8_lossy(c.reason()).into_owned()));
        return seen;
    }
    let mut receiver = bfe_presentation::network::Session::new();
    receiver.reserve_receive();
    let (mut started, mut ready) = (false, false);
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(seconds) {
        if endpoint.pump(&mut |lane, text, bytes, _| take(lane, text, bytes, &mut events, &mut state)).is_err() { break; }
        if let Some(closed) = endpoint.connection.as_ref().and_then(|c| c.closed) {
            seen.closed = Some((closed.code, String::from_utf8_lossy(closed.reason()).into_owned()));
            break;
        }
        if !ready {
            if let Some(registry) = events.iter().find(|e| e["type"] == "entity-registry") {
                if let Some(checks) = checks(&registry["registry"], &seen.answer) { receiver.check_records(checks); }
                ready = true;
            }
        }
        if ready && !started && events.iter().any(|e| e["type"] == "welcome") {
            started = true;
            let call = serde_json::to_vec(&serde_json::json!({"type": "call", "id": 1, "method": "start", "args": {"name": "Tester"}})).unwrap();
            endpoint.send(Lane::Events, true, &call).unwrap();
            endpoint.flush().unwrap();
        }
        if ready {
            for bytes in state.drain(..) {
                match receiver.receive_with_bounds(&bytes, &bounds) {
                    Err(error) => seen.refused.push(error.to_string()),
                    Ok(()) => seen.complete += usize::from(receiver.header[h::COMPLETE] != 0. && receiver.header[h::VALID] != 0.),
                }
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    seen.events = events;
    seen
}

/// A 2005 match end to end through a dedicated server, then a rotation to a
/// 2004 battle and back. Discovery reads the fresh `/info` reply through the
/// client's selection; the 2005 hello asks for the match it names with the
/// record revisions this build implements and the digest the client computes
/// of its own copy; the signed answer names the match and its record table;
/// every 2005 record of the snapshot stream is checked against the match's
/// plan before it is acknowledged. The server announces and hands over to a
/// 2004 battle, which the client joins with the released hello, then to a
/// new 2005 match, which it joins afresh; the old match's hello is refused
/// as changed.
#[test]
fn a_2005_match_end_to_end_with_a_rotation_to_2004_and_back() {
    use bfe_content::files::DirFiles;
    use bfe_family::GameFamily;
    use bfe_online::declared::Statements;
    use bfe_server::{config::ServerConfig, identity::ServerIdentity, local_server::LocalDedicatedServer};
    use bfe_sim::wire::revisions::{self, Checks, Descriptor, Request, Supports, SUPPORTS};
    use bfe_transport::layout::network_contract as contract;
    use serde_json::{json, Value};
    let dir = temp_dir("match-2005");
    let root = common::minimal_content_dir(&dir);
    common::invented_battle(&GameFamily::Swbf2.content_dir(&root));
    // The client's numbers, read from its own copy of the content by the
    // rule the server's admission uses.
    let numbers = {
        let folder = GameFamily::Swbf2.content_dir(&root);
        let files = DirFiles::from_resolved_root(&folder, GameFamily::Swbf2);
        let battle = bfe_content::battle::Battle::load_for_family(&bfe_content::battle::Loader::new(&files), GameFamily::Swbf2, "yard",
            "?map=yard&side=rep&mode=duel&era=cw").unwrap();
        let content = bfe_content::content::Content::build(battle).unwrap();
        let defs = bfe_content::defs::Defs::build(&content).unwrap();
        bfe_sim::movement::soldier::swbf2::classes::of_battle(&content, &defs).into_iter()
            .map(|(class, tuning)| (class, tuning.unwrap())).collect::<Vec<_>>()
    };
    assert_eq!(numbers.iter().map(|(class, t)| (class.as_str(), t.roll_frames)).collect::<Vec<_>>(), [("blue_trooper", 37), ("gold_scout", 37)]);
    let config = |entry: &str| ServerConfig { data: root.to_string_lossy().into_owned(), rotation: vec![entry.into()], game_port: Some(0),
        discovery: false, gate: OPEN, ..Default::default() };
    let (swbf2, swbf1) = (config("swbf2/yard:cw:duel"), config("swbf1/nab2:rep:conquest"));
    let identity = ServerIdentity::load(&dir).unwrap();
    let box_2005 = bfe_sim::wire::PositionBounds::from_map_extents([-1000., -100., -1000.], [1000., 400., 1000.]).unwrap();
    // The checks a 2005 client installs: the plan of the signed answer and
    // each registered class's movement numbers.
    let checks_2005 = |registry: &Value, answer: &Value| -> Option<Box<dyn bfe_presentation::network::RecordChecks>> {
        let descriptor = Descriptor::read(&answer["battle"]).ok()?;
        let plan = revisions::plan(&descriptor.records, &Supports::of(SUPPORTS)).ok()?;
        let rows = registry["classes"].as_array()?;
        let mut classes = vec![None; rows.len() + 1];
        for row in rows {
            let (id, name) = (row[0].as_u64()? as usize, row[1].as_str()?);
            if let Some((_, t)) = numbers.iter().find(|(class, _)| class == name) { *classes.get_mut(id)? = Some(*t); }
        }
        Some(Box::new(Checks::new(plan, classes)))
    };
    // Discovery and the 2005 hello for the match the fresh reply names.
    let hello_2005 = |address| {
        let (body, info) = info_of(address);
        let query = info["query"].as_str().unwrap().to_string();
        assert_eq!(Statements::read(&body).family(&query, None).unwrap(), (GameFamily::Swbf2, true), "{info}");
        let content_query = bfe_family::swbf2::content_query(&query, "yard").unwrap();
        let digest = bfe_content_native::fingerprint::hash_for_family(&DirFiles::from_resolved_root(GameFamily::Swbf2.content_dir(&root), GameFamily::Swbf2), GameFamily::Swbf2,
            &content_query).unwrap();
        assert_eq!(info["fingerprint"], digest.as_str(), "the server advertises the digest the client computes");
        let token = bfe_content::mission::Query::parse(&query).get("match").to_string();
        let request = Request { mode: "duel".into(), map: "swbf2/yard".into(), token: token.clone(), supports: Supports::of(SUPPORTS) };
        let hello = json!({"protocol": contract::PROTOCOL, "profile": contract::PROFILE, "simulation": contract::SIMULATION,
            "content": digest, "name": "Tester", "battle": request.json()});
        (serde_json::to_vec(&hello).unwrap(), request, digest)
    };
    let first = LocalDedicatedServer::start(&swbf2, &identity, |_| false, |_, _, _, _, _, _| {}).unwrap();
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], first.port));
    let (hello, request, digest) = hello_2005(address);
    let seen = join(address, identity.inner.id, &hello, 4, checks_2005, box_2005);
    let descriptor = Descriptor::read(&seen.answer["battle"]).unwrap();
    assert!(request.names(&descriptor) && seen.answer["fingerprint"] == digest.as_str() && seen.answer["content"] == digest.as_str());
    assert_eq!(descriptor.records, revisions::Table::of(revisions::WRITES));
    assert!(seen.refused.is_empty() && seen.complete >= 5, "{} complete, refused {:?}", seen.complete, seen.refused);
    // The welcome names the epoch's flight ceiling: the mission names none,
    // so its soldiers were admitted with 200 m.
    let welcome = seen.events.iter().find(|e| e["type"] == "welcome").unwrap();
    assert_eq!(welcome["flightCeiling"].as_f64(), Some(200.));
    println!("2005 match: {} snapshots complete, {} reliable messages", seen.complete, seen.events.len());
    // The announcement of a 2004 battle, then the handover to it.
    let second_token = "1111111111111111111111111111111a";
    let mut announced = None;
    {
        use bfe_transport::{crypto::Identity, endpoint::Endpoint};
        let mut watcher = Endpoint::connect(&address.to_string(), Identity::generate().unwrap(), identity.inner.id, &hello).unwrap();
        let mut events: Vec<Value> = Vec::new();
        fn keep(text: bool, bytes: &[u8], events: &mut Vec<Value>) {
            if text { if let Ok(v) = serde_json::from_slice::<Value>(bytes) { events.push(v); } }
        }
        watcher.establish(std::time::Duration::from_secs(3), &mut |_, text, bytes, _| keep(text, bytes, &mut events)).unwrap();
        first.match_rotate(&format!("?map=nab2&side=rep&match={second_token}"), 1500);
        let start = std::time::Instant::now();
        while announced.is_none() && start.elapsed() < std::time::Duration::from_secs(3) {
            watcher.pump(&mut |_, text, bytes, _| keep(text, bytes, &mut events)).unwrap();
            announced = events.iter().find(|e| e["type"] == "match").cloned();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    let announced = announced.expect("the next battle is announced to the match's players");
    assert!(announced["query"].as_str().unwrap().contains(second_token));
    let handed = first.hand_over();
    let second = LocalDedicatedServer::start_with_match_token(&swbf1, &identity, |_| false, |_, _, _, _, _, _| {}, Some(second_token), Some(handed)).unwrap();
    // The service settles the announcement once its registration names the new battle.
    second.udp.lock().unwrap().pending_match = None;
    let (body, info) = info_of(address);
    let query = info["query"].as_str().unwrap().to_string();
    assert!(query.contains(second_token) && query.contains("map=nab2"), "{info}");
    assert_eq!(Statements::read(&body).game(&query, None).unwrap().0, GameFamily::Swbf1);
    // The released hello for the 2004 battle, with the released digest.
    let released = bfe_content_native::fingerprint::hash(&DirFiles::new(&root, GameFamily::Swbf1), &query).unwrap();
    let hello_2004 = serde_json::to_vec(&json!({"name": "Tester", "protocol": contract::PROTOCOL, "profile": contract::PROFILE,
        "simulation": contract::SIMULATION, "content": released})).unwrap();
    let seen = join(address, identity.inner.id, &hello_2004, 5, |_, _| None, bfe_sim::wire::PositionBounds::LEGACY);
    let types: Vec<&str> = seen.events.iter().filter_map(|e| e["type"].as_str()).collect();
    assert!(seen.answer.get("battle").is_none() && seen.closed.is_none(), "{:?} {types:?}", seen.closed);
    assert!(types.contains(&"welcome") && types.contains(&"entity-registry"), "{types:?} {}", seen.answer);
    assert!(seen.events.iter().all(|e| e.get("flightCeiling").is_none()), "a 2004 welcome is the released one");
    // The 2005 match's old hello meets a 2004 battle: the released admission
    // refuses its content as it refuses any other, and the client's probe of
    // /info then finds the battle changed and follows it.
    let stale = join(address, identity.inner.id, &hello, 1, |_, _| None, box_2005);
    assert_eq!(stale.closed.as_ref().map(|(code, _)| *code), Some(3), "{:?}", stale.closed);
    // Back to a new 2005 match.
    let third_token = "2222222222222222222222222222222b";
    second.match_rotate(&format!("?map=swbf2/yard&side=rep&match={third_token}"), 1500);
    let handed = second.hand_over();
    let third = LocalDedicatedServer::start_with_match_token(&swbf2, &identity, |_| false, |_, _, _, _, _, _| {}, Some(third_token), Some(handed)).unwrap();
    third.udp.lock().unwrap().pending_match = None;
    // The original match's hello names a match that ended: refused as changed.
    let stale = join(address, identity.inner.id, &hello, 1, |_, _| None, box_2005);
    assert_eq!(stale.closed.as_ref().map(|(code, reason)| (*code, reason.as_str())), Some((4, "match-changed")), "{:?}", stale.closed);
    let (hello, request, digest) = hello_2005(address);
    assert_eq!(request.token, third_token);
    let seen = join(address, identity.inner.id, &hello, 3, checks_2005, box_2005);
    assert!(request.names(&Descriptor::read(&seen.answer["battle"]).unwrap()) && seen.answer["fingerprint"] == digest.as_str());
    assert!(seen.refused.is_empty() && seen.complete >= 3, "{} complete, refused {:?}", seen.complete, seen.refused);
    drop(third);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn failed_add_player_rolls_back_admitted_player_mailbox_and_edges() {
    use bfe_family::GameFamily;
    let root = temp_dir("add-player-rollback");
    common::invented_battle(&root);
    let query = "?map=yard&side=rep&ai=off&seed=1&mode=duel&era=cw";
    let mut host = NativeWorldHost::load_for(&root, query, 1, GameFamily::Swbf2, OPEN).unwrap();

    // Fail after world_network_join, wire_edges_create and run_host_op(0, ...)
    // by pointing publication_session at an absent session so wire_publish_bind
    // refuses it. Repeating 70 times proves none of the player, roster slot,
    // mailbox, edges bank or host client leaks.
    let real_pub = host.publication_session;
    host.publication_session = 999_999;
    for attempt in 0..70 {
        let peer = format!("fail-bind-{attempt}");
        let err = host
            .add_player(&peer, "FailBind", false, false, false)
            .expect_err("wire_publish_bind with invalid session must fail add_player");
        assert!(
            err.contains("publication bind refused"),
            "attempt {attempt} leaked resources instead of rolling back: {err}"
        );
        assert!(host.mailbox_by_native.is_empty() && host.player_by_native.is_empty() && host.edges_by_native.is_empty());
    }
    host.publication_session = real_pub;

    // After all rolled-back failures, a normal join still succeeds cleanly.
    let ok = host.add_player("survivor", "Survivor", false, false, false).unwrap();
    assert!(ok.team == 1 || ok.team == 2);
    drop(host);
    let _ = fs::remove_dir_all(&root);
}
