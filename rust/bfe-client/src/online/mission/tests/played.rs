//! A scripted 2005 battle on a real server, played by a client with the
//! battle's content: the mission is an invented one that scores and ends by
//! itself (`tests/fixtures/mission-scripts/belfry.lua`).
use super::super::*;
use crate::generated_input::{ClientAction, ClientCommand, ClientFlightCommand, ClientHeld, ClientLook};
use crate::online::{info, input::Sender, match2005};
use bfe_family::GameFamily;
use bfe_server::{config::ServerConfig, identity::ServerIdentity, local_server::LocalDedicatedServer};
use bfe_sim::battle::script::state::CompactOutcome;
use bfe_sim::wire::revisions::Checks;
use bfe_transport::{connection::Lane, endpoint::Endpoint};
use std::{collections::VecDeque, time::{Duration, Instant}};

#[path = "../../../../../bfe-server/tests/common/mod.rs"]
mod common;

/// The parts of a HUD the observation reads, without one: the player's spawn
/// summary as the HUD takes it from the summaries the server sends him.
#[derive(Default)]
struct Plain(HudOwnSummary);
impl Panel for Plain {
    fn summary(&self) -> &HudOwnSummary { &self.0 }
    fn health(&self, _: usize) -> f64 { 300. }
    fn loadout(&self, _: u32) -> Option<(Loadout, usize)> { None }
}
impl Plain {
    fn read(&mut self, receiver: &Receiver) {
        for field in receiver.publications.iter().filter(|f| f.row[r::KIND] == f64::from(k::SUMMARY) && f.row[r::SCHEMA] == 3. && f.length >= 24) {
            let bytes = &field.bytes[..field.length];
            (self.0.valid, self.0.team, self.0.phase, self.0.ended) = (true, bytes[8], bytes[9], bytes[15] != 0);
            self.0.dead_time = f64::from_le_bytes(bytes[16..24].try_into().unwrap());
        }
    }
}

/// What one player's client saw of the battle.
#[derive(Default)]
struct Seen {
    alive: bool,
    scores: Vec<[u32; 2]>,
    phases: Vec<(bool, bool, bool)>,
    outcome: Option<CompactOutcome>,
    messages: Vec<String>,
    cues: Vec<(bool, String)>,
    due: Vec<u32>,
    objectives: Vec<Vec<(i32, String)>>,
    last: Option<Box<Observation>>,
}

/// A client in `server`'s battle with the content of `root`, driven as the
/// client's loop drives it, from the server's signed answer on.
struct Player {
    endpoint: Box<Endpoint>,
    receiver: Box<Receiver>,
    sender: Box<Sender>,
    replay: Replay,
    ceiling_applied: bool,
    started: bool,
    next: Instant,
    /// What the server sent and when it is let in: `delay` after it came, and
    /// on a rough way some snapshots and corrections later than what follows
    /// them, and some never.
    delay: Duration,
    rough: bool,
    states: u64,
    due: VecDeque<(Instant, bool, Vec<u8>, f64)>,
}

impl Player {
    fn join(server: &LocalDedicatedServer, root: &std::path::Path, key: &std::path::Path, delay: Duration) -> Self {
        let address = format!("127.0.0.1:{}", server.port);
        let (reply, mut selection, mut endpoint) = info::prepare(&address, root, key, "Pilot", None, bfe_test_gate::OPEN).unwrap();
        assert!(!selection.sits_out(), "a client with the map plays it");
        let query = bfe_family::swbf2::content_query(&reply.query, &selection.map).unwrap();
        let textures = crate::scene::TextureOptions { compressed: false, max_size: 4096, capabilities: 0 };
        let folder = GameFamily::Swbf2.content_dir(root);
        let content = crate::scene::Scene::online_content_swbf2(&folder, &selection.map, &query, textures, selection.pinned.take()).unwrap();
        let replay = Replay::new(&content).unwrap();
        let defs = content.defs().unwrap();
        let descriptor = crate::online::replicas::registry_descriptor(&defs.registry, GameFamily::Swbf2);
        let bounds = bfe_sim::wire::bounds::for_battle(GameFamily::Swbf2, &content.content.battle.map_data).unwrap();
        let mut receiver = Box::new(Receiver::with_bounds(descriptor, bounds));
        receiver.clock = crate::online::clock::Clock::online();
        receiver.control_family = GameFamily::Swbf2;
        if let Some(space) = &replay.space {
            let meta = space.meta.clone();
            receiver.space = Some(crate::local::space::History::new(meta.counts));
            receiver.space_online = Some(Box::new(crate::online::space_receiver::OnlineSpace::new(
                meta,
                bounds,
            )));
        }
        let sender = Box::new(Sender::with_inputs(&[], crate::input_family::WireInputs::new(GameFamily::Swbf2)));

        // The signed answer before anything the server sends is read.
        let expected = selection.join.clone().unwrap();
        let mut held = match2005::Held::default();
        let start = Instant::now();
        while !endpoint.ready() {
            endpoint.pump(&mut |lane, text, bytes, now| if text || lane == Lane::State { assert!(held.hold(text, bytes, now)); }).unwrap();
            assert!(start.elapsed() < Duration::from_secs(10), "the signed answer");
            std::thread::sleep(Duration::from_millis(2));
        }
        let joined = match2005::accept(&endpoint.server_hello, &expected).unwrap();
        assert_eq!(joined.role, match2005::Role::Battle);
        receiver.network.borrow_mut().check_records(Box::new(Checks::new(joined.plan.clone(), replay.swbf2_classes.clone())));
        receiver.swbf2_plan = Some(joined.plan.clone());
        for (text, bytes, now) in held.drain() { receiver.message(text, &bytes, now); }
        Self { endpoint, receiver, sender, replay, ceiling_applied: false, started: false, next: Instant::now(), delay, rough: false, states: 0, due: VecDeque::new() }
    }

    /// Takes what has arrived and, every sixtieth of a second, sends the
    /// command given for it; whether this turn sent one.
    fn turn(&mut self, command: impl FnOnce(&Receiver) -> (ClientCommand, ClientLook)) -> bool {
        self.turn_flight(|r| {
            let (cmd, look) = command(r);
            (cmd, look, ClientFlightCommand::default())
        })
    }

    fn turn_flight(
        &mut self,
        command: impl FnOnce(&Receiver) -> (ClientCommand, ClientLook, ClientFlightCommand),
    ) -> bool {
        let Self { endpoint, receiver, sender, replay, ceiling_applied, due, delay, rough, states, .. } = self;
        endpoint.pump(&mut |lane, text, bytes, now| if text || lane == Lane::State {
            let mut late = *delay;
            if !text && *rough {
                *states += 1;
                if *states % 11 == 7 { return; }
                if *states % 5 == 3 { late += Duration::from_millis(45); }
                if *states % 7 == 5 { late += Duration::from_millis(90); }
            }
            due.push_back((Instant::now() + late, text, bytes.to_vec(), now + late.as_secs_f64() * 1000.));
        }).unwrap();
        // In the order they arrive, which on a rough way is not the one they left in.
        due.make_contiguous().sort_by_key(|(at, ..)| *at);
        while due.front().is_some_and(|(at, ..)| *at <= Instant::now()) {
            let (_, text, bytes, now) = due.pop_front().unwrap();
            receiver.message(text, &bytes, now);
        }
        assert_eq!(receiver.error(), None);
        while let Some(bytes) = receiver.pong() {
            if !endpoint.send(Lane::Events, true, bytes).unwrap() { break; }
            receiver.pong_sent();
        }
        if let Some(ceiling) = receiver.flight_ceiling.filter(|_| !*ceiling_applied) {
            replay.set_flight_ceiling(ceiling);
            *ceiling_applied = true;
        }
        if receiver.welcomed && !self.started {
            self.started = endpoint.send(Lane::Events, true, br#"{"type":"call","id":1,"method":"start","args":{}}"#).unwrap();
        }
        std::thread::sleep(Duration::from_millis(1));
        let now = Instant::now();
        if now < self.next { return false; }
        self.next = (self.next + Duration::from_micros(16_667)).max(now);
        sender.feedback(receiver);
        receiver.clock.advance(1. / 60.).unwrap();
        let (command, look, flight) = command(receiver);
        sender.tick(&command, &look, &flight, receiver, 30. + delay.as_secs_f64() * 1000., endpoint.now());
        sender.spawn(receiver);
        sender.send(endpoint, receiver).unwrap();
        endpoint.flush().unwrap();
        true
    }
}

/// Joins `server`'s battle, deploys, walks and watches until the match is
/// finished.
fn play(server: &LocalDedicatedServer, root: &std::path::Path, key: &std::path::Path) -> (u8, Seen) {
    let mut player = Player::join(server, root, key, Duration::ZERO);
    let mut panel = Plain::default();
    let (mut seen, mut team, start) = (Seen::default(), 0, Instant::now());
    while seen.phases.last().is_none_or(|(_, _, finished)| !finished) {
        assert!(start.elapsed() < Duration::from_secs(60), "the match ends: {:?}", seen.scores);
        let walk = ClientCommand { held: 1 << ClientHeld::Forward as u8, ..Default::default() };
        if !player.turn(|_| (walk, ClientLook::default())) { continue; }
        panel.read(&player.receiver);
        player.receiver.observe_mission(&panel, &player.replay);
        observe(&mut player.receiver, &mut seen, &mut team);
        player.receiver.publications.clear();
    }
    (team, seen)
}

fn observe(receiver: &mut Receiver, seen: &mut Seen, team: &mut u8) {
    let Some(view) = receiver.mission_view.as_deref() else { return };
    seen.alive |= receiver.own.alive;
    if receiver.own.alive { *team = receiver.own.team; }
    if seen.scores.last() != Some(&view.score.teams) { seen.scores.push(view.score.teams); }
    let phase = (view.score.over, view.score.game_over, view.score.finished);
    if seen.phases.last() != Some(&phase) { seen.phases.push(phase); }
    seen.outcome = view.score.outcome;
    for notice in &view.notices {
        if let Notice::Message { key, .. } = notice.value {
            let key = format!("{} {key}", notice.sequence);
            if !seen.messages.contains(&key) { seen.messages.push(key); }
        }
        if let Notice::Voice { sound, .. } = notice.value {
            let heard = (true, sound.to_string());
            if !seen.cues.contains(&heard) { seen.cues.push(heard); }
        }
    }
    if let Some((name, due)) = view.timer {
        assert_eq!(name.as_str(), "winch");
        if seen.due.last() != Some(&due) { seen.due.push(due); }
    }
    let objectives: Vec<(i32, String)> = view.objectives.iter().map(|o| (o.team, o.key.to_string())).collect();
    if seen.objectives.last() != Some(&objectives) { seen.objectives.push(objectives); }
    view.copy_into(&mut seen.last);
}

// Two players on opposite teams see the score move and the match end, the
// timer run and the objectives change, and each what the script told everyone
// or his own team, and nothing of the other's.
#[test]
fn playing_clients_see_the_score_move_and_the_match_end() {
    let dir = std::env::temp_dir().join(format!("bfe-client-mission-{}", std::process::id()));
    let root = common::minimal_content_dir(&dir);
    common::invented_mission(&GameFamily::Swbf2.content_dir(&root), &["cw"]);
    let identity = ServerIdentity::load(&dir).unwrap();
    let config = ServerConfig { data: root.to_string_lossy().into_owned(), rotation: vec!["swbf2/yard:cw:eli".into()], ai: "off".into(),
        game_port: Some(0), discovery: false, gate: bfe_test_gate::OPEN, auto_spawn: true, auto_assign_teams: true, ..Default::default() };
    let server = LocalDedicatedServer::start(&config, &identity, |_| false, |_, _, _, _, _, _| {}).unwrap();
    let players = std::thread::scope(|scope| {
        let (server, root) = (&server, &root);
        let players: Vec<_> = ["one", "two"].map(|name| dir.join(format!("{name}.key"))).into_iter()
            .map(|key| scope.spawn(move || play(server, root, &key))).collect();
        players.into_iter().map(|player| player.join().unwrap()).collect::<Vec<_>>()
    });
    let mut teams: Vec<u8> = players.iter().map(|(team, _)| *team).collect();
    teams.sort_unstable();
    assert_eq!(teams, [1, 2]);
    for (team, seen) in players {
        println!("team {team}, scores {:?}, phases {:?}, messages {:?}, cues {:?}, objectives {:?}", seen.scores, seen.phases, seen.messages, seen.cues, seen.objectives);
        assert!(seen.alive, "the client plays a soldier");
        // Every turn of the belfry the client was there for, and the last one.
        assert!(seen.scores.len() >= 4, "{:?}", seen.scores);
        assert!(seen.scores.iter().all(|[blue, gold]| *blue == 4 * gold) && seen.scores.is_sorted());
        assert_eq!(seen.scores.last(), Some(&[200, 50]));
        assert_eq!(seen.outcome, Some(CompactOutcome::Victory(1)));
        assert_eq!((seen.phases.first(), seen.phases.last()), (Some(&(false, false, false)), Some(&(true, true, true))));
        // Its timer was due again after each turn, a second and a half apart.
        assert!(seen.due.len() >= 3 && seen.due.windows(2).all(|pair| pair[1] - pair[0] == 90), "{:?}", seen.due);
        // Both active objectives, then the one the bell did not complete.
        assert_eq!(seen.objectives.first().unwrap(), &[(1, "belfry.wind".to_string()), (0, "belfry.listen".to_string())]);
        assert_eq!(seen.objectives.last().unwrap(), &[(0, "belfry.listen".to_string())]);
        // What the script told everyone reached this player, what it told the
        // other team did not.
        let to = |key: &str| seen.messages.iter().filter(|message| message.ends_with(key)).count();
        assert!(to("belfry.everyone") >= 2, "{:?}", seen.messages);
        assert_eq!(to("belfry.turn") > 0, team == 1);
        assert_eq!(seen.cues.iter().any(|(voice, sound)| *voice && sound == "belfry_creak"), team == 2);
        assert!(seen.cues.iter().all(|(voice, _)| *voice));

        // The rest of the observation: the roster by its registry names, the
        // tickets, what the script shows, and the player's own soldier among
        // the two.
        let last = seen.last.unwrap();
        let teams = &last.admission.teams;
        assert_eq!((teams[0].name.as_str(), teams[0].icon.as_str(), teams[1].name.as_str()), ("blue", "badge_blue", "gold"));
        assert_eq!(teams.iter().map(|t| t.classes.iter().map(|c| c.to_string()).collect::<Vec<_>>()).collect::<Vec<_>>(), [["rep_winder"], ["cis_winder"]]);
        assert_eq!((last.score.target, last.show_points, last.map_hidden, last.posts_hidden), (Some(180), [true, true], false, false));
        assert!(last.reinforcements.iter().zip([50, 100]).all(|(left, most)| left.is_some_and(|left| (0..=most).contains(&left))), "{:?}", last.reinforcements);
        assert_eq!(last.local_character.map(|(_, team)| team), Some(team));
        assert!(last.roster.iter().flatten().any(|p| p.class.is_some()), "roster rows carry replicated class ids");
        assert_eq!(last.class_has_room(1, "rep_winder"), Some(true));
        assert_eq!(last.class_has_room(2, "cis_winder"), Some(true));
        assert_eq!(last.bodies.iter().filter(|body| body.human && body.team == team && body.max_health == 300.).count(), 1);
    }
    drop(server);
    let _ = std::fs::remove_dir_all(&dir);
}

// A 2005 soldier on a connection with a tenth of a second between the server
// and what it says runs, sprints, jumps, strafes, crouches and rolls in this
// client's prediction as the server has him do it, to the bit, well ahead of
// the server's newest correction. While he turns, the server takes two
// commands' looks together and the prediction one by one: a difference of
// millimetres, eased away.
#[test]
fn an_online_soldier_is_predicted_as_the_server_moves_him() { predicted("smooth", false); }

// The same while snapshots and corrections come out of order and one in eleven
// not at all: a correction older than the one applied changes nothing, and the
// prediction from the newest is the server's.
#[test]
fn an_online_soldier_is_predicted_through_lost_and_reordered_corrections() { predicted("rough", true); }

fn predicted(way: &str, rough: bool) {
    let dir = std::env::temp_dir().join(format!("bfe-client-predicted-{way}-{}", std::process::id()));
    let root = common::minimal_content_dir(&dir);
    common::invented_mission(&GameFamily::Swbf2.content_dir(&root), &["cw"]);
    let identity = ServerIdentity::load(&dir).unwrap();
    let config = ServerConfig { data: root.to_string_lossy().into_owned(), rotation: vec!["swbf2/yard:cw:eli".into()], ai: "off".into(),
        game_port: Some(0), discovery: false, gate: bfe_test_gate::OPEN, auto_spawn: true, ..Default::default() };
    let server = LocalDedicatedServer::start(&config, &identity, |_| false, |_, _, _, _, _, _| {}).unwrap();
    let mut player = Player::join(&server, &root, &dir.join("one.key"), Duration::from_millis(100));
    player.rough = rough;
    let start = Instant::now();
    let (mut lived, mut predicted, mut moved, mut lead, mut straight) = (0u32, 0u32, 0f64, 0f64, 0f64);
    let (mut sprinted, mut crouched, mut flew, mut rolled, mut last) = (0, 0, 0, 0, None::<[f64; 3]>);
    while lived < 350 {
        assert!(start.elapsed() < Duration::from_secs(40), "alive for {lived} ticks");
        let press = |action: ClientAction| 1u32 << action as u8;
        let hold = |held: ClientHeld| 1u32 << held as u8;
        let alive = player.receiver.own.alive;
        let sent = player.turn(|_| {
            let mut command = ClientCommand { held: hold(ClientHeld::Forward), ..Default::default() };
            if (30..80).contains(&lived) { command.held |= hold(ClientHeld::Sprint); }
            if (150..190).contains(&lived) { command.held |= hold(ClientHeld::Right); }
            if lived == 100 { command.actions |= press(ClientAction::Jump); }
            if lived == 195 || lived == 235 { command.actions |= press(ClientAction::Crouch); }
            if lived == 245 { command.actions |= press(ClientAction::Roll); }
            (command, ClientLook { x: if lived >= 310 { 12. } else { 0. }, y: 0. })
        });
        if !sent { continue; }
        player.sender.preview(&player.receiver);
        player.replay.frame(&player.receiver, &player.sender);
        player.receiver.publications.clear();
        if !alive { continue; }
        lived += 1;
        if lived == 310 { straight = player.replay.correction_metres; }
        let Some(body) = &player.replay.body else { continue };
        predicted += 1;
        if let Some(last) = last { moved += ((body.position[0] - last[0]).powi(2) + (body.position[2] - last[2]).powi(2)).sqrt(); }
        last = Some(body.position);
        crouched += u32::from(body.posture == bfe_sim::gen::Posture::Crouch);
        flew += u32::from(body.air.is_some());
        let state = player.replay.swbf2_state().expect("a 2005 soldier's own movement");
        sprinted += u32::from(state.sprint_time.is_some());
        rolled += u32::from(state.roll.is_some());
        lead = lead.max(player.replay.predicted_tick() - player.receiver.own.tick);
    }
    let replay = &player.replay;
    println!("{way}: {predicted} of {lived} ticks predicted, up to {lead} ticks ahead, {moved:.1} m moved, {straight} m corrected before he turned and {} m in all, \
        {} snaps, {} commands late; sprinted {sprinted}, crouched {crouched}, in the air {flew}, rolled {rolled}", replay.correction_metres, replay.snaps, player.sender.late);
    assert!(predicted + 12 >= lived, "{predicted} of {lived}");
    assert!(lead >= 6. && moved > 15.);
    assert!(sprinted >= 40 && crouched >= 80 && flew >= 10 && rolled >= 40);
    // A command the server took late or a tick it dropped is a correction no client can foresee.
    if player.sender.late == 0 && player.receiver.server_dropped == 0. {
        assert_eq!(straight, 0.);
        assert!(replay.correction_metres < 0.2, "{} m", replay.correction_metres);
    }
    assert_eq!(replay.snaps, 0);
    drop(server);
    let _ = std::fs::remove_dir_all(&dir);
}

// A player on a rough way deploys when he chooses to, falls, chooses again once
// his own wait is over, and deploys with the wave his view has counted down to
// since he fell. His body is predicted in both lives: what still comes in of
// the life before never moves the one after it.
#[test]
fn a_player_redeploys_with_the_wave_his_view_shows() {
    let dir = std::env::temp_dir().join(format!("bfe-client-redeploys-{}", std::process::id()));
    let root = common::minimal_content_dir(&dir);
    let content = GameFamily::Swbf2.content_dir(&root);
    common::invented_mission(&content, &["cw"]);
    common::replace_mission_script(&content, "cw", include_bytes!("../../../../../../tests/fixtures/mission-scripts/vigil.luac"));
    let identity = ServerIdentity::load(&dir).unwrap();
    let config = ServerConfig { data: root.to_string_lossy().into_owned(), rotation: vec!["swbf2/yard:cw:eli".into()], ai: "off".into(),
        game_port: Some(0), discovery: false, gate: bfe_test_gate::OPEN, auto_assign_teams: true, ..Default::default() };
    let server = LocalDedicatedServer::start(&config, &identity, |_| false, |_, _, _, _, _, _| {}).unwrap();
    let mut player = Player::join(&server, &root, &dir.join("one.key"), Duration::from_millis(40));
    player.rough = true;
    let mut panel = Plain::default();
    let start = Instant::now();
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Stage { Arriving, Deploying, Living(u32), Falling, Down { fell: f64, due: u32 }, Chosen { due: u32 }, Back(u32) }
    let mut stage = Stage::Arriving;
    let mut predicted = 0;
    while !matches!(stage, Stage::Back(60)) {
        assert!(start.elapsed() < Duration::from_secs(60), "{stage:?}");
        let walk = ClientCommand { held: 1 << ClientHeld::Forward as u8, ..Default::default() };
        if !player.turn(|_| (walk, ClientLook::default())) { continue; }
        panel.read(&player.receiver);
        player.receiver.observe_mission(&panel, &player.replay);
        player.sender.preview(&player.receiver);
        player.replay.frame(&player.receiver, &player.sender);
        player.receiver.publications.clear();
        let Player { receiver, sender, replay, .. } = &mut player;
        let Some(view) = receiver.mission_view.as_deref() else { continue };
        let facts = (view.deploy_ready, view.deploying, view.wave);
        let alive = receiver.own.alive;
        predicted += u32::from(replay.body.is_some());
        stage = match stage {
            Stage::Arriving if view.deploy_ready && !alive => {
                // Nothing holds his place back but the battle's opening slot.
                assert!(!view.deploying && view.wave.is_none_or(|due| due <= 36), "{facts:?}");
                sender.request_spawn(receiver, receiver.own.team, receiver.own.class, 0);
                Stage::Deploying
            }
            Stage::Deploying if alive => Stage::Living(0),
            Stage::Living(40) => { sender.request_suicide(receiver); Stage::Falling }
            Stage::Living(ticks) => Stage::Living(ticks + 1),
            Stage::Falling if !alive => match view.wave {
                Some(due) => { assert!(!view.deploy_ready && !view.deploying, "{facts:?}"); Stage::Down { fell: receiver.own.tick, due } }
                None => stage,
            },
            Stage::Down { fell, due } if view.deploy_ready => {
                assert_eq!(facts, (true, false, Some(due)));
                assert!((215. ..290.).contains(&(receiver.own.tick - fell)), "{} ticks down", receiver.own.tick - fell);
                sender.request_spawn(receiver, receiver.own.team, receiver.own.class, 0);
                Stage::Chosen { due }
            }
            Stage::Down { due, .. } => { assert_eq!(facts, (false, false, Some(due))); stage }
            Stage::Chosen { due } if alive => {
                let late = receiver.own.tick - due as f64;
                assert!((0. ..20.).contains(&late), "{late} ticks after his wave");
                predicted = 0;
                Stage::Back(0)
            }
            Stage::Chosen { due } => {
                if view.deploying { assert_eq!(facts, (true, true, Some(due)), "at {}", view.tick); }
                stage
            }
            Stage::Back(ticks) => {
                assert!(alive);
                if ticks > 30 { assert_eq!(facts, (false, false, None)); }
                Stage::Back(ticks + 1)
            }
            _ => stage,
        };
    }
    assert!(predicted >= 50, "predicted on {predicted} of 60 ticks of his second life");
    assert_eq!(player.replay.snaps, 0);
    drop(server);
    let _ = std::fs::remove_dir_all(&dir);
}

// The class room a client sees is the battle's: a human may take any class
// his team lists, whatever its bots hold (the 2005 game limits him only in a
// class with points to unlock, and no prepared class has any), so with bots
// filling both teams' classes every listed class stays open to him on either
// team, alive or down (`Observation::class_room`).
#[test]
fn online_player_sees_every_listed_class_open_on_both_teams() {
    let dir = std::env::temp_dir().join(format!("bfe-client-classroom-{}", std::process::id()));
    let root = common::minimal_content_dir(&dir);
    common::invented_mission(&GameFamily::Swbf2.content_dir(&root), &["cw"]);
    let identity = ServerIdentity::load(&dir).unwrap();
    // belfry.lua sets unit count 4 and max_units 4 for rep_winder (team 1) and
    // cis_winder (team 2); classic bots fill them.
    let config = ServerConfig {
        data: root.to_string_lossy().into_owned(),
        rotation: vec!["swbf2/yard:cw:eli".into()],
        ai: "classic".into(),
        game_port: Some(0),
        discovery: false,
        gate: bfe_test_gate::OPEN,
        auto_spawn: true,
        auto_assign_teams: true,
        ..Default::default()
    };
    let server = LocalDedicatedServer::start(&config, &identity, |_| false, |_, _, _, _, _, _| {}).unwrap();
    let mut player = Player::join(&server, &root, &dir.join("one.key"), Duration::ZERO);
    let mut panel = Plain::default();
    let start = Instant::now();
    let (mut seen_alive, mut seen_down) = (0u32, 0u32);
    // At least 12 s of checks, and up to 60 s on a busy machine, where the server steps slower
    // than real time.
    while start.elapsed() < Duration::from_secs(60)
        && !(start.elapsed() >= Duration::from_secs(12) && seen_alive > 0 && seen_down > 0)
    {
        if !player.turn(|_| (ClientCommand::default(), ClientLook::default())) {
            continue;
        }
        panel.read(&player.receiver);
        player.receiver.observe_mission(&panel, &player.replay);
        player.receiver.publications.clear();
        let Player { receiver, sender, .. } = &mut player;
        let Some(view) = receiver.mission_view.as_deref() else { continue };
        let own_team = if receiver.own.team != 0 { receiver.own.team } else { panel.0.team };
        if own_team != 1 && own_team != 2 {
            continue;
        }
        let rooms = [view.class_has_room(1, "rep_winder"), view.class_has_room(2, "cis_winder")];
        if rooms.contains(&None) {
            continue;
        }
        assert_eq!(rooms, [Some(true); 2], "a listed class closed to a human");
        if receiver.own.alive {
            seen_alive += 1;
            // On a busy machine the bots may not bring him down in time: he ends his life himself, so the
            // classes are also seen while he is down.
            if seen_alive >= 40 && seen_down == 0 {
                sender.request_suicide(receiver);
            }
        } else {
            seen_down += 1;
        }
    }
    assert!(seen_alive > 0 && seen_down > 0, "alive {seen_alive} down {seen_down}");
    drop(server);
    let _ = std::fs::remove_dir_all(&dir);
}

// A real Space pilot on a dedicated server with 100 ms RTT boards a parked
// craft, takes off, and maneuvers through continuous stick and boost inputs
// while client-side tick-scheduled prediction tracks the server's pose with
// sub-5 cm median error and zero snaps.
#[test]
#[ignore = "requires prepared Battlefront II content (BFE_SWBF2_PREPARED)"]
fn space_pilot_prediction_matches_real_server_at_latency() {
    use bfe_sim::space::flight::SpaceFlightState;
    use bfe_sim::wire::space::OwnSpaceBody;
    use std::collections::BTreeMap;

    let prepared = std::path::PathBuf::from(
        std::env::var("BFE_SWBF2_PREPARED").expect("BFE_SWBF2_PREPARED"),
    );
    let root = prepared
        .parent()
        .and_then(|p| p.parent())
        .expect("BFE_SWBF2_PREPARED should be <root>/swbf2/.content")
        .to_path_buf();
    let dir = std::env::temp_dir().join(format!("bfe-client-space-pred-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let identity = ServerIdentity::load(&dir).unwrap();
    let config = ServerConfig {
        data: root.to_string_lossy().into_owned(),
        rotation: vec!["swbf2/spa6:cw:ass".into()],
        ai: "off".into(),
        game_port: Some(0),
        discovery: false,
        gate: bfe_test_gate::OPEN,
        auto_spawn: true,
        auto_assign_teams: true,
        ..Default::default()
    };
    let server = LocalDedicatedServer::start(&config, &identity, |_| false, |_, _, _, _, _, _| {}).unwrap();
    let mut player = Player::join(&server, &root, &dir.join("pilot.key"), Duration::from_millis(50));

    let press = |a: ClientAction| 1u32 << a as u8;
    let hold = |h: ClientHeld| 1u32 << h as u8;
    let fpress = |a: bfe_sim::gen::ClientFlightAction| 1u32 << a as u8;
    let fhold = |h: bfe_sim::gen::ClientFlightHeld| 1u32 << h as u8;
    let mut predicted_by_tick: BTreeMap<u32, [f64; 3]> = BTreeMap::new();
    let mut pure_fly_errors: Vec<f64> = Vec::new();
    let mut last_own_rev = 0u32;
    let mut fly_ticks = 0u32;
    let mut walk_ticks = 0u32;
    let start = Instant::now();

    while fly_ticks < 240 {
        assert!(
            start.elapsed() < Duration::from_secs(45),
            "timed out before completing flight (fly_ticks={fly_ticks}, mounted={}, alive={})",
            player.receiver.space_mounted(),
            player.receiver.own.alive
        );
        let sent = player.turn_flight(|r| {
            let mut cmd = ClientCommand::default();
            let mut look = ClientLook::default();
            let mut fl = ClientFlightCommand { scale: 1.0, ..Default::default() };
            if let Some(OwnSpaceBody::Pilot(own)) = r.space_online.as_deref().and_then(|s| s.own_body.as_ref()) {
                match own.state {
                    0 => {
                        if walk_ticks % 10 == 0 {
                            fl.actions |= fpress(bfe_sim::gen::ClientFlightAction::Takeoff);
                        }
                        walk_ticks += 1;
                    }
                    1 => {
                        fl.held |= fhold(bfe_sim::gen::ClientFlightHeld::Throttle);
                    }
                    _ => {
                        fl.held |= fhold(bfe_sim::gen::ClientFlightHeld::Throttle);
                        // Clear the hangar mouth straight ahead, then climb gently into open space.
                        if fly_ticks >= 70 {
                            let t = f64::from(fly_ticks - 70) / 60.0;
                            if ((fly_ticks - 70) % 90) < 30 {
                                fl.held |= fhold(bfe_sim::gen::ClientFlightHeld::Boost);
                            }
                            fl.pad_axes = [
                                0.45 * (t * 1.4).sin(),
                                -0.35 + 0.15 * (t * 0.9).sin(),
                                0.25 * (t * 0.8).cos(),
                                0.0,
                            ];
                            look.x = 3.0 * (t * 1.1).cos();
                        }
                    }
                }
            } else if r.own.alive {
                walk_ticks += 1;
                let my_pos = r.own.position;
                let my_yaw = r.own.yaw;
                let target = r
                    .space
                    .as_ref()
                    .and_then(|h| h.latest())
                    .and_then(|frame| {
                        frame
                            .hulls
                            .iter()
                            .filter(|h| h.team == r.own.team && h.state == SpaceFlightState::Landed)
                            .min_by(|a, b| {
                                let da = (a.current.position[0] - my_pos[0]).hypot(a.current.position[2] - my_pos[2]);
                                let db = (b.current.position[0] - my_pos[0]).hypot(b.current.position[2] - my_pos[2]);
                                da.total_cmp(&db)
                            })
                            .map(|h| h.current.position)
                    });
                if let Some(tp) = target {
                    let dx = tp[0] - my_pos[0];
                    let dz = tp[2] - my_pos[2];
                    let dist = dx.hypot(dz);
                    let want_yaw = dx.atan2(dz);
                    let mut dyaw = want_yaw - my_yaw;
                    while dyaw > std::f64::consts::PI { dyaw -= std::f64::consts::TAU; }
                    while dyaw < -std::f64::consts::PI { dyaw += std::f64::consts::TAU; }
                    look.x = (-dyaw * 80.0).clamp(-40.0, 40.0);
                    if dist > 3.5 && dyaw.abs() < 0.7 {
                        cmd.held |= hold(ClientHeld::Forward);
                        if dist > 10.0 {
                            cmd.held |= hold(ClientHeld::Sprint);
                        }
                    }
                    if dist < 8.5 && walk_ticks % 45 == 1 {
                        cmd.actions |= press(ClientAction::Use);
                    }
                }
            }
            (cmd, look, fl)
        });
        if !sent {
            continue;
        }
        player.sender.preview(&player.receiver);
        player.replay.frame(&player.receiver, &player.sender);
        player.receiver.publications.clear();

        if let Some(space) = player.receiver.space_online.as_deref() {
            if space.own_revision != last_own_rev {
                last_own_rev = space.own_revision;
                if let Some(OwnSpaceBody::Pilot(own)) = space.own_body.as_ref() {
                    if own.state == 2 {
                        if let Some(&pred_pos) = predicted_by_tick.get(&space.own_ack_tick) {
                            let dx = pred_pos[0] - own.position[0];
                            let dy = pred_pos[1] - own.position[1];
                            let dz = pred_pos[2] - own.position[2];
                            pure_fly_errors.push((dx * dx + dy * dy + dz * dz).sqrt());
                        }
                    }
                }
            }
        }
        if let Some(sp) = player.replay.space.as_deref() {
            if let Some((ptick, ppos, state)) = sp.predicted_pilot_state() {
                if state == SpaceFlightState::Flying {
                    fly_ticks += 1;
                    predicted_by_tick.insert(ptick, ppos);
                    while predicted_by_tick.len() > 256 {
                        predicted_by_tick.pop_first();
                    }
                }
            }
        }
    }

    assert!(pure_fly_errors.len() >= 50, "too few flying samples: {}", pure_fly_errors.len());
    pure_fly_errors.sort_by(f64::total_cmp);
    let median = pure_fly_errors[pure_fly_errors.len() / 2];
    let mean = pure_fly_errors.iter().sum::<f64>() / pure_fly_errors.len() as f64;
    let snaps = player.replay.space.as_deref().map_or(0, |s| s.snaps);
    println!(
        "space prediction (100 ms RTT): samples={}, median={median:.5} m, mean={mean:.5} m, snaps={snaps}, late={}, srv_dropped={}",
        pure_fly_errors.len(),
        player.sender.late,
        player.receiver.server_dropped
    );
    assert_eq!(snaps, 0, "unexpected snap during steady flight");
    assert!(median < 0.05, "median pure flying error {median:.4} m exceeds 0.05 m");
    drop(server);
    let _ = std::fs::remove_dir_all(&dir);
}

// A Hero Assault battle of the prepared 2005 content on a real server, with
// bots, watched by a client with the same content: the heroes' combat
// displays (kind 5) and thrown sabers (kind 6) are world fields the match names
// and its receiver checks and stores, and the powers the heroes use reach the
// client as power events.
#[test]
#[ignore = "requires BFE_SWBF2_PREPARED (a folder's prepared swbf2/.content)"]
fn a_hero_assault_client_reads_the_hero_records_and_power_events() {
    use bfe_sim::wire::revisions::{COMBAT_DISPLAY, THROWN};
    let prepared = std::path::PathBuf::from(std::env::var_os("BFE_SWBF2_PREPARED").unwrap());
    let root = prepared.parent().and_then(std::path::Path::parent).unwrap().to_path_buf();
    let dir = std::env::temp_dir().join(format!("bfe-client-heroes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let identity = ServerIdentity::load(&dir).unwrap();
    let config = ServerConfig { data: root.to_string_lossy().into_owned(), rotation: vec!["swbf2/tat2:gcw:eli".into()], ai: "classic".into(),
        game_port: Some(0), discovery: false, gate: bfe_test_gate::OPEN, auto_spawn: true, auto_assign_teams: true, ..Default::default() };
    let server = LocalDedicatedServer::start(&config, &identity, |_| false, |_, _, _, _, _, _| {}).unwrap();
    let mut player = Player::join(&server, &root, &dir.join("one.key"), Duration::ZERO);
    let plan = player.receiver.swbf2_plan.clone().unwrap();
    assert_eq!((plan.revision(COMBAT_DISPLAY), plan.revision(THROWN)),
        (Ok(bfe_sim::wire::melee::REVISION), Ok(bfe_sim::wire::thrown::REVISION)));
    let (mut displays, mut throws, mut powers, start) = (0u32, 0u32, 0u32, Instant::now());
    while start.elapsed() < Duration::from_secs(120) && (displays == 0 || powers == 0) {
        if !player.turn(|_| (ClientCommand::default(), ClientLook::default())) { continue; }
        for field in player.receiver.publications.iter() {
            let (schema, bytes) = (field.row[r::SCHEMA], &field.bytes[..field.length]);
            if schema == bfe_sim::wire::melee::SCHEMA { displays += 1; }
            if schema == bfe_sim::wire::thrown::SCHEMA { throws += 1; }
            if schema == 8. && bytes.windows(14).any(|w| w == br#""kind":"power""#) { powers += 1; }
        }
        player.receiver.publications.clear();
    }
    println!("hero records online: {displays} combat displays, {throws} thrown sabers, {powers} power events in {:.0} s",
        start.elapsed().as_secs_f64());
    assert!(displays > 0, "no combat display reached the client");
    assert!(powers > 0, "no power event reached the client");
    drop(server);
    let _ = std::fs::remove_dir_all(&dir);
}
