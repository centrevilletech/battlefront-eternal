use super::*;
use bfe_sim::battle::script::state::CompactOutcome;
use serde_json::json;

mod pair;
mod played;

fn mission() -> Mission {
    let roster = wire::Roster { teams: [
        wire::Team { name: "blue".into(), icons: vec!["badge".into(), "small".into()], classes: vec![7, 3] },
        wire::Team { name: "gold".into(), icons: vec![], classes: vec![4] },
    ] };
    Mission::new(&roster, |id| Some(format!("class_{id}")).filter(|_| id != 9))
}

fn field(state: wire::State<'_>) -> (u32, Vec<u8>) {
    let mut payload = wire::REVISION.to_le_bytes().to_vec();
    wire::write_state(&state, &mut payload);
    (state.id(), payload)
}

fn system(system: &wire::System<'_>) -> Vec<u8> {
    let mut payload = wire::REVISION.to_le_bytes().to_vec();
    wire::write_system(system, &mut payload);
    payload
}

// Everything a field says is stored as said, the newest of each, and an epoch
// takes all of it but the roster.
#[test]
fn installed_fields_are_the_newest_of_each() {
    let mut mission = mission();
    assert_eq!(mission.admission.teams[0].classes.iter().map(|c| c.to_string()).collect::<Vec<_>>(), ["class_7", "class_3"]);
    assert_eq!((mission.admission.teams[0].icon.as_str(), mission.admission.teams[1].name.as_str()), ("badge", "gold"));
    assert_eq!(mission.score(5), None);
    let install = |mission: &mut Mission, state| {
        let (id, payload) = field(state);
        mission.install(wire::STATE_SCHEMA, id, &payload).unwrap();
    };
    let state = wire::Match { points: [40, 10], target: Some(180), ..Default::default() };
    install(&mut mission, wire::State::Match(state));
    assert_eq!(mission.score(5).map(|score| (score.script_tick, score.teams, score.target, score.outcome)), Some((5, [40, 10], Some(180), None)));
    install(&mut mission, wire::State::Match(wire::Match { points: [200, 50], over: true, outcome: Some(CompactOutcome::Victory(1)), ..state }));
    let score = mission.score(9).unwrap();
    assert_eq!((score.teams, score.outcome, score.over), ([200, 50], Some(CompactOutcome::Victory(1)), true));
    install(&mut mission, wire::State::Timer(wire::Timer { shown: true, due: Some(900), left: 0, name: "winch" }));
    assert_eq!(mission.timer, Some(MissionTimer::Running { name: label("winch"), due: 900 }));
    // A timer that stands still keeps its remaining ticks; one not shown clears it.
    install(&mut mission, wire::State::Timer(wire::Timer { shown: true, due: None, left: 60, name: "winch" }));
    assert_eq!(mission.timer, Some(MissionTimer::Paused { name: label("winch"), left: 60 }));
    install(&mut mission, wire::State::Timer(wire::Timer::default()));
    assert!(mission.timer.is_none());
    let mut rows = [None; wire::PERSONAL_SLOTS];
    (rows[1], rows[30]) = (Some(4), Some(-2));
    install(&mut mission, wire::State::Personal(wire::Personal { field: 1, rows }));
    assert_eq!((mission.personal[33], mission.personal[62], mission.personal[1]), (Some(4), Some(-2), None));
    let mut rows = [None; wire::OBJECTIVE_ROWS];
    rows[0] = Some(wire::Objective { active: true, completed: false, team: 2, key: "belfry.guard" });
    install(&mut mission, wire::State::Objectives(wire::Objectives { field: 1, rows }));
    let goal = mission.goals[8].unwrap();
    assert_eq!((goal.active, goal.team, goal.key.as_str()), (true, 2, "belfry.guard"));
    let mut rows = [None; wire::PERSONAL_SLOTS];
    rows[2] = Some(wire::Deployment { due: 900, chosen: true });
    install(&mut mission, wire::State::Deployments(wire::Deployments { field: 1, rows }));
    assert_eq!((mission.deployments[34], mission.deployments[2]), (rows[2], None));
    install(&mut mission, wire::State::Deployments(wire::Deployments { field: 1, ..Default::default() }));
    assert_eq!(mission.deployments[34], None, "nobody waits there now");
    install(&mut mission, wire::State::Deployments(wire::Deployments { field: 1, rows }));
    let mut class_rows = [None; wire::PERSONAL_SLOTS];
    class_rows[5] = Some(7);
    install(&mut mission, wire::State::Classes(wire::Classes { field: 1, rows: class_rows }));
    assert_eq!((mission.classes[37], mission.classes[5]), (Some(7), None));
    let class_room = wire::ClassRoom { count: [2, 1], available: [0b01, 0b0] };
    install(&mut mission, wire::State::ClassRoom(class_room));
    assert_eq!(mission.class_room, Some(class_room));
    let generator = wire::System { index: 3, alive: true, marker: true, team: 2, health: Some([40., 90.]), name: "generator", ..Default::default() };
    mission.install(wire::SYSTEM_SCHEMA, 3, &system(&generator)).unwrap();
    assert_eq!(mission.systems[3].map(|s| (s.name.to_string(), s.state.health)), Some(("generator".to_string(), Some([40., 90.]))));
    mission.reset();
    assert!(mission.score(1).is_none() && mission.timer.is_none() && mission.systems[3].is_none() && mission.goals[8].is_none());
    assert!(mission.personal[33].is_none() && mission.deployments[34].is_none() && mission.classes[37].is_none() && mission.class_room.is_none());
    assert_eq!(mission.admission.teams[0].name, "blue");
}

// A record of another revision, one that does not read, or one that lies at
// another id than its own is refused before anything of it is stored.
#[test]
fn a_field_that_is_not_its_own_is_refused() {
    let mut mission = mission();
    let (id, payload) = field(wire::State::Match(wire::Match { points: [1, 2], ..Default::default() }));
    assert!(mission.install(wire::STATE_SCHEMA, id + 1, &payload).is_err());
    let mut later = payload.clone();
    later[0] += 1;
    assert!(mission.install(wire::STATE_SCHEMA, id, &later).is_err());
    assert!(mission.install(wire::STATE_SCHEMA, id, &payload[..payload.len() - 1]).is_err());
    assert!(mission.install(wire::STATE_SCHEMA, id, &[1]).is_err());
    assert!(mission.install(wire::SYSTEM_SCHEMA, id, &payload).is_err());
    assert!(mission.install(wire::SYSTEM_SCHEMA, 4, &system(&wire::System { index: 5, name: "x", ..Default::default() })).is_err());
    assert!(mission.score(1).is_none() && mission.systems.iter().all(Option::is_none));
    assert!(Mission::carries(wire::STATE_SCHEMA) && Mission::carries(wire::SYSTEM_SCHEMA) && !Mission::carries(0x0203));
}

// Notices come once each, in order, and the ones kept do not grow without end.
#[test]
fn notices_come_once_and_in_order() {
    let mut mission = mission();
    let body = |sequence: u64, key: &str, value: serde_json::Value| json!({"sequence": sequence, "tick": 10, "until": 310, key: value});
    let weapon = |id: u16| (id == 12).then(|| "yard_carbine".to_string());
    mission.notice(&body(1, "text", json!("belfry.turn")), weapon).unwrap();
    mission.notice(&body(4, "kill", json!([[2, false, 7], [0, true, 0], 12])), weapon).unwrap();
    mission.notice(&body(5, "voice", json!("creak")), weapon).unwrap();
    for stale in [5, 2] {
        assert!(mission.notice(&body(stale, "text", json!("again")), weapon).is_err());
    }
    assert!(mission.notice(&json!({"sequence": 9}), weapon).is_err());
    assert_eq!((mission.notices.len(), mission.sequence), (3, 5));
    let Notice::Message { team: None, key } = mission.notices[0].value else { panic!("a message") };
    assert_eq!((key.as_str(), mission.notices[0].until), ("belfry.turn", 310));
    // A kill with a killer names no weapon, as a local view's does.
    let Notice::Kill { killer: Some(killer), victim, weapon: with } = mission.notices[1].value else { panic!("a kill") };
    assert_eq!((killer.slot, killer.class, victim.human, victim.class, with.as_str()), (2, Some(7), true, None, ""));
    assert_eq!(mission.notices[2].value, Notice::voice(None, "creak"));
    for sequence in 6..200 {
        mission.notice(&body(sequence, ["text", "hint", "voice"][sequence as usize % 3], json!("more")), weapon).unwrap();
    }
    assert_eq!(mission.notices.len(), NOTICES);
}

// A field the authority removes clears what it held: an emptied chunk, a
// finished objective block and a system it stops naming leave no stale state.
#[test]
fn removed_fields_clear_what_they_held() {
    let mut mission = mission();
    let install = |mission: &mut Mission, state| {
        let (id, payload) = field(state);
        mission.install(wire::STATE_SCHEMA, id, &payload).unwrap();
        id
    };
    install(&mut mission, wire::State::Match(wire::Match { points: [3, 4], ..Default::default() }));
    let mut rows = [None; wire::PERSONAL_SLOTS];
    rows[0] = Some(9);
    let personal = install(&mut mission, wire::State::Personal(wire::Personal { field: 0, rows }));
    let mut class_rows = [None; wire::PERSONAL_SLOTS];
    class_rows[0] = Some(3);
    let classes = install(&mut mission, wire::State::Classes(wire::Classes { field: 0, rows: class_rows }));
    let class_room = install(&mut mission, wire::State::ClassRoom(wire::ClassRoom { count: [2, 1], available: [0b10, 0b1] }));
    let mut goals = [None; wire::OBJECTIVE_ROWS];
    goals[0] = Some(wire::Objective { active: true, completed: false, team: 1, key: "belfry.guard" });
    let objectives = install(&mut mission, wire::State::Objectives(wire::Objectives { field: 0, rows: goals }));
    let generator = wire::System { index: 2, name: "generator", ..Default::default() };
    mission.install(wire::SYSTEM_SCHEMA, 2, &system(&generator)).unwrap();
    assert!(mission.score(1).is_some() && mission.personal[0] == Some(9) && mission.classes[0] == Some(3) && mission.class_room.is_some() && mission.goals[0].is_some() && mission.systems[2].is_some());
    mission.remove(wire::STATE_SCHEMA, personal).unwrap();
    mission.remove(wire::STATE_SCHEMA, classes).unwrap();
    mission.remove(wire::STATE_SCHEMA, class_room).unwrap();
    mission.remove(wire::STATE_SCHEMA, objectives).unwrap();
    mission.remove(wire::SYSTEM_SCHEMA, 2).unwrap();
    assert!(mission.personal[0].is_none() && mission.classes[0].is_none() && mission.class_room.is_none() && mission.goals[0].is_none() && mission.systems[2].is_none());
    assert!(mission.remove(wire::STATE_SCHEMA, 0x0900).is_err());
    assert!(mission.remove(wire::SYSTEM_SCHEMA, 32).is_err());
    assert!(mission.remove(wire::STATE_SCHEMA, 0).is_ok() && mission.score(1).is_none());
}

// A snapshot soldier id is read from the network: tracking its last pose must
// never size storage by that id. An id no battle can hold is shown untracked.
#[test]
fn a_soldier_id_past_the_battles_bodies_is_shown_untracked() {
    let mut mission = mission();
    let now = Pose { position: [3., 4.], yaw: 1., health: 90., energy: 0.5 };
    let far = LifeId { slot: 8_388_608, slab: 0, generation: 1 };
    assert_eq!(mission.track_pose(far, 10, now).position, now.position);
    assert!(mission.poses.len() <= 512, "pose storage grew to a wire id: {}", mission.poses.len());
    let life = LifeId { slot: 7, slab: 0, generation: 2 };
    assert_eq!(mission.track_pose(life, 10, now).position, now.position);
    let moved = Pose { position: [9., 4.], ..now };
    assert_eq!(mission.track_pose(life, 11, moved).position, now.position, "the pose a tick earlier");
    assert_eq!(mission.track_pose(life, 12, moved).position, moved.position);
}

// A script's message, hint and voice-over are the same notices whether the
// battle is a local world's (from its notifications) or the authority's (from
// the records it sends), and the battle's sound plays each voice-over once.
#[test]
fn hints_and_voice_overs_are_the_same_notices_replicated_and_local() {
    use crate::local::mission_view::{script_notice, VoicesHeard};
    use bfe_sim::battle::script::state::ScriptNotification as Script;
    let mut mission = mission();
    let body = |sequence: u64, until: u32, key: &str, value: &str| json!({"sequence": sequence, "tick": 10, "until": until, key: value});
    mission.notice(&body(1, 50, "voice", "expired_vo"), |_| None).unwrap();
    mission.notice(&body(2, 200, "text", "belfry.turn"), |_| None).unwrap();
    mission.notice(&body(3, 220, "hint", "belfry.hint"), |_| None).unwrap();
    mission.notice(&body(4, 220, "voice", "belfry_creak"), |_| None).unwrap();
    let replicated: Vec<Notice> = mission.notices.iter().map(|notice| notice.value).collect();
    let local: Vec<Notice> = [
        Script::BroadcastVoiceOver { sound_id: "expired_vo".into(), team: None },
        Script::ShowMessageText { key: "belfry.turn".into(), team: None },
        Script::AddMissionHint { key: "belfry.hint".to_string() },
        Script::BroadcastVoiceOver { sound_id: "belfry_creak".into(), team: None },
    ].iter().filter_map(script_notice).collect();
    assert_eq!(replicated, local);
    // Words the wire could not carry, and what is no cue, make no notice.
    assert_eq!(script_notice(&Script::AddMissionHint { key: String::new() }), None);
    assert_eq!(script_notice(&Script::BroadcastVoiceOver { sound_id: "x".repeat(97).into(), team: Some(2) }), None);
    assert_eq!(script_notice(&Script::DisableSmallMapMiniMap), None);

    // Seen from tick 60 on, as many times as the screens are drawn: the expired one never plays,
    // the other once, and a later one when it comes.
    let admission = std::sync::Arc::new(Admission { teams: Default::default() });
    let score = MissionScore { script_tick: 0, teams: [0; 2], target: None, outcome: None, over: false, game_over: false, finished: false, faulted: false };
    let mut view = Observation::new(admission, score, 4);
    view.tick = 60;
    view.notices = mission.notices.iter().copied().collect();
    let mut heard = VoicesHeard::default();
    let mut played = Vec::new();
    for _ in 0..3 {
        heard.take(&view, |sound, team| played.push((sound.to_owned(), team)));
    }
    assert_eq!(played, [("belfry_creak".to_string(), None)]);
    mission.notice(&body(5, 300, "voice", "belfry_toll"), |_| None).unwrap();
    view.notices = mission.notices.iter().copied().collect();
    view.tick = 61;
    heard.take(&view, |sound, team| played.push((sound.to_owned(), team)));
    assert_eq!(played.len(), 2);
    assert_eq!(played[1].0, "belfry_toll");
    // The clock going back is another battle, whose notices count from one
    // again (this one has had its fields).
    let (id, payload) = field(wire::State::Leaving(wire::Leaving { field: 0, rows: [None; wire::PERSONAL_SLOTS] }));
    mission.install(wire::STATE_SCHEMA, id, &payload).unwrap();
    mission.reset();
    mission.notice(&body(1, 90, "voice", "again"), |_| None).unwrap();
    view.notices = mission.notices.iter().copied().collect();
    view.tick = 5;
    heard.take(&view, |sound, team| played.push((sound.to_owned(), team)));
    assert_eq!(played.last().map(|(sound, _)| sound.as_str()), Some("again"));
}

#[test]
fn reset_allows_notices_from_sequence_one_in_the_next_epoch() {
    let mut mission = mission();
    let body = |sequence: u64| json!({"sequence": sequence, "tick": 10, "until": 310, "text": "belfry.turn"});
    // An epoch that has had its fields.
    let (id, payload) = field(wire::State::Leaving(wire::Leaving { field: 0, rows: [None; wire::PERSONAL_SLOTS] }));
    mission.install(wire::STATE_SCHEMA, id, &payload).unwrap();
    mission.notice(&body(1), |_| None).unwrap();
    mission.notice(&body(5), |_| None).unwrap();
    assert_eq!(mission.sequence, 5);
    mission.reset();
    assert_eq!((mission.sequence, mission.notices.len()), (0, 0));
    mission.notice(&body(1), |_| None).expect("new epoch starts notice sequence at 1");
    assert_eq!(mission.sequence, 1);
}

// A battle's welcome brings the notices still showing (its opening hint), and
// its first state starts the epoch after them: they stay, in sequence.
#[test]
fn the_notices_that_come_with_the_welcome_outlast_the_start_of_its_epoch() {
    let mut mission = mission();
    mission.notice(&json!({"sequence": 1, "tick": 1, "until": 301, "hint": "belfry.hint"}), |_| None).unwrap();
    mission.reset();
    assert_eq!((mission.sequence, mission.notices.len()), (1, 1));
    assert!(mission.notice(&json!({"sequence": 1, "tick": 2, "until": 302, "text": "again"}), |_| None).is_err(), "still in sequence");
    mission.notice(&json!({"sequence": 2, "tick": 3, "until": 303, "text": "belfry.turn"}), |_| None).unwrap();
    assert_eq!(mission.notices.len(), 2);
}

#[test]
fn paused_timer_preserves_remaining_time_across_ticks() {
    let mut mission = mission();
    let (id, payload) = field(wire::State::Timer(wire::Timer { shown: true, due: None, left: 180, name: "winch" }));
    mission.install(wire::STATE_SCHEMA, id, &payload).unwrap();
    assert_eq!(mission.timer, Some(MissionTimer::Paused { name: label("winch"), left: 180 }));
    // A paused timer shows nothing on the HUD, as a local one does, during
    // play or once the match is over.
    assert_eq!(mission.observed_timer(false), None);
    assert_eq!(mission.observed_timer(true), None);
    // Hiding the timer clears it.
    let (id, payload) = field(wire::State::Timer(wire::Timer::default()));
    mission.install(wire::STATE_SCHEMA, id, &payload).unwrap();
    assert!(mission.timer.is_none());
    assert_eq!(mission.observed_timer(false), None);
}

#[test]
fn class_room_feeds_observation_for_both_teams_and_defaults_open_when_absent() {
    let mut mission = mission();
    let score = MissionScore {
        script_tick: 0,
        teams: [0, 0],
        target: None,
        outcome: None,
        over: false,
        game_over: false,
        finished: false,
        faulted: false,
    };
    let mut obs = Observation::new(mission.admission.clone(), score, 4);
    // Before any ClassRoom field arrives, every admitted class defaults open.
    mission.write_class_room(&mut obs.class_room);
    assert_eq!(obs.class_has_room(1, "class_7"), Some(true));
    assert_eq!(obs.class_has_room(1, "class_3"), Some(true));
    assert_eq!(obs.class_has_room(2, "class_4"), Some(true));
    assert_eq!(obs.class_has_room(1, "unknown"), None);

    // Team 1 has class_7 open and class_3 full; Team 2 has class_4 full.
    let (id, payload) = field(wire::State::ClassRoom(wire::ClassRoom {
        count: [2, 1],
        available: [0b01, 0b0],
    }));
    mission.install(wire::STATE_SCHEMA, id, &payload).unwrap();
    mission.write_class_room(&mut obs.class_room);
    assert_eq!(obs.class_has_room(1, "class_7"), Some(true));
    assert_eq!(obs.class_has_room(1, "class_3"), Some(false));
    assert_eq!(obs.class_has_room(2, "class_4"), Some(false));

    // When all classes on team 1 fill and team 2 opens, the observation reflects both.
    let (id, payload) = field(wire::State::ClassRoom(wire::ClassRoom {
        count: [2, 1],
        available: [0b00, 0b1],
    }));
    mission.install(wire::STATE_SCHEMA, id, &payload).unwrap();
    mission.write_class_room(&mut obs.class_room);
    assert_eq!(obs.class_has_room(1, "class_7"), Some(false));
    assert_eq!(obs.class_has_room(1, "class_3"), Some(false));
    assert_eq!(obs.class_has_room(2, "class_4"), Some(true));

    // Removing the field resets availability to open.
    mission.remove(wire::STATE_SCHEMA, id).unwrap();
    mission.write_class_room(&mut obs.class_room);
    assert_eq!(obs.class_has_room(1, "class_7"), Some(true));
    assert_eq!(obs.class_has_room(1, "class_3"), Some(true));
    assert_eq!(obs.class_has_room(2, "class_4"), Some(true));
}

// The timer a script shows is the same on the HUD whether the battle is a local
// world's or the authority's replicated one: both make it with `hud_timer`
// from the same facts (due while it runs, match over); a stopped timer shows
// nothing in either.
#[test]
fn a_timer_shows_the_same_replicated_and_local() {
    use crate::local::mission_view::hud_timer;
    // (due, ticks left, match over, the deadline the HUD counts to)
    let cases = [
        (Some(900), 0, false, Some(900)),
        (None, 180, false, None),
        (None, 0, false, None),
        (Some(900), 0, true, None),
        (None, 180, true, None),
    ];
    for (due, left, over, shown) in cases {
        let mut replicated = mission();
        let timer = wire::Timer { shown: true, due, left, name: "winch" };
        let (id, payload) = field(wire::State::Timer(timer));
        replicated.install(wire::STATE_SCHEMA, id, &payload).unwrap();
        let local = hud_timer(label("winch"), due, over);
        assert_eq!(replicated.observed_timer(over), local, "{due:?} {left} {over}");
        assert_eq!(local.map(|(_, deadline)| deadline), shown, "{due:?} {left} {over}");
    }
}

// Online, a player is shown leaving the battlefield while he lives and either
// the craft he sits in runs its out-of-bounds timer (his own Space body, any
// seat: a gunner's craft is his too) or the authority says his soldier's
// countdown; a countdown that is no time is no countdown.
#[test]
fn an_online_player_leaves_by_his_crafts_timer_or_his_soldiers_countdown_while_he_lives() {
    use bfe_sim::wire::space::{OwnPilotState, OwnSpaceBody};
    let craft = |oob_timer: f64, seat: u8| OwnSpaceBody::Pilot(OwnPilotState {
        is_gunner: seat != 0, fighter_id: 1, fighter_gen: 1, seat, profile: 0, state: 0, trick: 0, boosting: false,
        boost_exhausted: false, action_held: false, oob_warning: oob_timer >= 0., can_land: false, mobile_spawn: false,
        boarding_use: false, selected_weapon: None, position: [0.; 3], velocity: [0.; 3], quaternion: [0., 0., 0., 1.],
        airspeed: 0., yaw_rate: 0., pitch_rate: 0., roll_rate: 0., bank: 0., gear: 0., state_time: 0., energy: 0.,
        health: 1., trick_progress: 0., trick_cooldown: 0., eject_timer: 0., oob_timer, stick: [0.; 2], roll_state: 0.,
        level_timer: 0., yaw_buildup: 0., pitch_buildup: 0., taps: Default::default(), trick_axis_right: [1., 0., 0.],
        gunner_angles: [0.; 2], weapon_count: 0, weapons: Default::default(), lock: None,
    });
    assert!(!leaving(true, None, None));
    assert!(!leaving(true, None, Some(&craft(-1., 0))), "inside: the timer is not running");
    assert!(leaving(true, None, Some(&craft(0., 0))), "the timer starts at zero");
    assert!(leaving(true, None, Some(&craft(4.5, 2))), "a gunner leaves with his craft");
    assert!(!leaving(false, None, Some(&craft(4.5, 0))), "a dead player leaves nothing");
    assert!(leaving(true, Some(3.), None));
    assert!(!leaving(false, Some(3.), None));
    let mut mission = mission();
    for nothing in [f32::NAN, f32::INFINITY, -0.5] {
        mission.set_own_leaving(Some(nothing));
        assert_eq!(mission.own_leaving, None, "{nothing}");
    }
    mission.set_own_leaving(Some(0.));
    assert_eq!(mission.own_leaving, Some(0.));
    mission.reset();
    assert_eq!(mission.own_leaving, None, "a new epoch starts on the battlefield");
}

// Online, the player's hit marker is the authority's damage record naming his
// life the attacker of another: marked on the tick of the step that made it
// (the record's own is its capture's, one before), not for a record of
// another's hit or of his own wound, gone half a second on and with a new
// life.
#[test]
fn an_online_player_marks_his_hits_from_the_authoritys_damage_records() {
    use bfe_presentation::network_layout::net_limits;
    let record = |tick: f64, target: (u32, u32), attacker: (u32, u32)| {
        let mut bytes = ((tick + 18.) as u32).to_le_bytes().to_vec();
        for n in [target.0, target.1, attacker.0, attacker.1] { bytes.extend_from_slice(&n.to_le_bytes()); }
        for n in [1., 2., 3.] { bytes.extend_from_slice(&f64::to_le_bytes(n)); }
        bytes.extend_from_slice(&2i32.to_le_bytes());
        bytes.extend_from_slice(&1i32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        for _ in 0..3 { bytes.extend_from_slice(&0f64.to_le_bytes()); }
        let mut row = [0.; r::STRIDE];
        (row[r::KIND], row[r::SCHEMA], row[r::TICK]) = (f64::from(k::COMBAT), 3., tick);
        (row, bytes)
    };
    assert!(record(0., (0, 0), (0, 0)).1.len() <= net_limits::FIELD_PAYLOAD);
    let mut receiver = Receiver::new(serde_json::Value::Null);
    (receiver.own.id, receiver.own.generation, receiver.own.alive) = (7., 2., true);
    let mut mission = mission();
    let mut see = |receiver: &mut Receiver, records: &[([f64; r::STRIDE], Vec<u8>)], tick: u32| {
        for (row, bytes) in records { receiver.publications.push(row, bytes); }
        mission.mark_hits(receiver, tick);
        receiver.publications.clear();
        mission.hit
    };
    assert_eq!(see(&mut receiver, &[record(100., (9, 1), (4, 1)), record(100., (7, 2), (9, 1))], 100), None, "another's hit, his own wound");
    assert_eq!(see(&mut receiver, &[record(103., (9, 1), (7, 2))], 103), Some((104, false)), "on the step's tick");
    assert_eq!(see(&mut receiver, &[], 134), Some((104, false)));
    assert_eq!(see(&mut receiver, &[], 135), None, "half a second on");
    assert_eq!(see(&mut receiver, &[record(140., (9, 1), (7, 1))], 140), None, "an earlier life's");
    assert_eq!(see(&mut receiver, &[record(141., (9, 1), (7, 2))], 141), Some((142, false)));
    receiver.own.generation = 3.;
    assert_eq!(see(&mut receiver, &[], 143), None, "a new life starts unmarked");
}

// His own soldier's leaving warning comes from the authority's leaving state
// for his roster slot: the seconds left at the tick shown, none for another
// slot's or once the state clears it.
#[test]
fn an_online_soldier_leaves_by_his_roster_slots_leaving_state() {
    let mut mission = mission();
    let mut receiver = Receiver::new(serde_json::Value::Null);
    receiver.roster.local = Some(33);
    let mut rows = [None; wire::PERSONAL_SLOTS];
    rows[1] = Some(700);
    let (id, payload) = field(wire::State::Leaving(wire::Leaving { field: 1, rows }));
    mission.install(wire::STATE_SCHEMA, id, &payload).unwrap();
    assert_eq!(mission.own_soldier_leaving(&receiver, 400), Some(5.));
    assert_eq!(mission.own_soldier_leaving(&receiver, 760), Some(0.), "still outside when the countdown has run out");
    receiver.roster.local = Some(32);
    assert_eq!(mission.own_soldier_leaving(&receiver, 400), None, "another's");
    receiver.roster.local = Some(33);
    mission.install(wire::STATE_SCHEMA, id, &field(wire::State::Leaving(wire::Leaving { field: 1, rows: [None; wire::PERSONAL_SLOTS] })).1).unwrap();
    assert_eq!(mission.own_soldier_leaving(&receiver, 400), None, "back inside");
}
