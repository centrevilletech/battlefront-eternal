use super::*;

fn round(state: State<'_>) -> Vec<u8> {
    let mut body = Vec::new();
    write_state(&state, &mut body);
    assert_eq!(read_state(&body), Ok(state));
    // A byte missing or left over refuses the record.
    assert!(read_state(&body[..body.len() - 1]).is_err());
    body.push(0);
    assert!(read_state(&body).is_err());
    body.pop();
    body
}

#[test]
fn match_state_reads_back_as_written() {
    let full = Match { over: true, game_over: true, finished: false, faulted: true, points_shown: [true, false], minimap_hidden: true,
        posts_hidden: false, outcome: Some(CompactOutcome::Victory(2)), target: Some(180), points: [u32::MAX, 7], reinforcements: [Some(0), None] };
    let body = round(State::Match(full));
    assert_eq!(body.len() + 2, MATCH_BYTES);
    for outcome in [None, Some(CompactOutcome::Defeat(1)), Some(CompactOutcome::Draw), Some(CompactOutcome::FaultedDraw)] {
        round(State::Match(Match { outcome, target: None, ..full }));
    }
    // Unknown flags, a result without its team or with one it does not have,
    // and values of what is absent.
    for (at, value) in [(2, 0x08), (3, 5), (3, 1), (4, 1), (5, 1), (17, 1), (21, 1)] {
        let mut body = Vec::new();
        write_state(&State::Match(Match { outcome: None, ..Default::default() }), &mut body);
        body[at] = value;
        assert!(read_state(&body).is_err(), "byte {at}");
    }
}

#[test]
fn the_timer_says_nothing_it_does_not_show() {
    let name = "n".repeat(MAX_TEXT);
    let body = round(State::Timer(Timer { shown: true, due: Some(u32::MAX), left: 0, name: &name }));
    assert_eq!(body.len() + 2, TIMER_BYTES);
    round(State::Timer(Timer { shown: true, due: None, left: 90, name: "winch" }));
    round(State::Timer(Timer::default()));
    for (flags, due, left, name) in [(0, 0, 0, "x"), (0, 0, 1, ""), (0, 1, 0, ""), (2, 0, 0, ""), (3, 5, 1, "x"), (1, 5, 0, "x"), (4, 0, 0, "")] {
        let mut body = vec![1, flags];
        body.extend(u32::to_le_bytes(due));
        body.extend(u32::to_le_bytes(left));
        text(name, &mut body);
        assert!(read_state(&body).is_err(), "{flags} {due} {left} {name:?}");
    }
}

#[test]
fn personal_points_and_objectives_keep_their_places() {
    let mut rows = [None; PERSONAL_SLOTS];
    (rows[0], rows[9], rows[31]) = (Some(0), Some(-250), Some(i32::MAX));
    round(State::Personal(Personal { field: 15, rows }));
    let body = round(State::Personal(Personal { field: 0, rows: [Some(-1); PERSONAL_SLOTS] }));
    assert_eq!(body.len() + 2, PERSONAL_BYTES);
    let mut none = vec![2, 0];
    none.extend([0; 4]);
    assert!(read_state(&none).is_err(), "a field of no points is not sent");
    assert!(read_state(&[2, 16, 1, 0, 0, 0, 0, 0, 0, 0]).is_err());

    let key = "k".repeat(MAX_TEXT);
    let row = Objective { active: true, completed: true, team: 7, key: &key };
    let body = round(State::Objectives(Objectives { field: 7, rows: [Some(row); OBJECTIVE_ROWS] }));
    assert_eq!(body.len() + 2, OBJECTIVES_BYTES);
    let mut rows = [None; OBJECTIVE_ROWS];
    rows[0] = Some(Objective { active: true, completed: false, team: EVERYONE, key: "a.first" });
    rows[1] = Some(Objective { active: false, completed: true, team: NOBODY, key: "ä" });
    assert_eq!(State::Objectives(Objectives { field: 3, rows }).id(), 0x303);
    round(State::Objectives(Objectives { field: 3, rows }));
    // A field or count out of range, an unknown flag, a team no player can be
    // on, an empty key and bytes that are no text.
    for body in [&[3, 8, 1, 0, 0, 1, b'k'][..], &[3, 0, 0], &[3, 0, 9], &[3, 0, 1, 4, 0, 1, b'k'], &[3, 0, 1, 0, 8, 1, b'k'],
        &[3, 0, 1, 0, 0, 0], &[3, 0, 1, 0, 0, 1, 0xff], &[4], &[5], &[6], &[7], &[]] {
        assert!(read_state(body).is_err(), "{body:?}");
    }
}

#[test]
fn deployments_keep_their_places() {
    let mut rows = [None; PERSONAL_SLOTS];
    (rows[0], rows[17], rows[31]) = (Some(Deployment { due: 0, chosen: true }), Some(Deployment { due: 900, chosen: false }), Some(Deployment { due: u32::MAX, chosen: true }));
    assert_eq!(State::Deployments(Deployments { field: 15, rows }).id(), 0x40f);
    round(State::Deployments(Deployments { field: 15, rows }));
    let body = round(State::Deployments(Deployments { field: 0, rows: [Some(Deployment { due: 7, chosen: false }); PERSONAL_SLOTS] }));
    assert_eq!(body.len() + 2, DEPLOYMENTS_BYTES);
    // Nobody waiting is said; a field past the last, and a choice of nobody.
    assert_eq!(round(State::Deployments(Deployments::default())).len(), 10);
    for body in [[4, 16, 0, 0, 0, 0, 0, 0, 0, 0], [4, 0, 0, 0, 0, 0, 1, 0, 0, 0], [4, 0, 1, 0, 0, 0, 0, 0, 0, 0]] {
        assert!(read_state(&body).is_err(), "{body:?}");
    }
}

// A player outside the battlefield keeps his slot's place and his countdown's
// end; nobody leaving is said too; a field past the last is refused.
#[test]
fn leaving_players_keep_their_places() {
    let mut rows = [None; PERSONAL_SLOTS];
    (rows[0], rows[19], rows[31]) = (Some(0), Some(18_000), Some(u32::MAX));
    assert_eq!(State::Leaving(Leaving { field: 2, rows }).id(), 0x702);
    assert_eq!(cleared(0x702), Some(Cleared::Leaving(2)));
    round(State::Leaving(Leaving { field: 2, rows }));
    let body = round(State::Leaving(Leaving { field: 0, rows: [Some(7); PERSONAL_SLOTS] }));
    assert_eq!(body.len() + 2, LEAVING_BYTES);
    assert_eq!(round(State::Leaving(Leaving::default())).len(), 6);
    for body in [&[7, 16, 0, 0, 0, 0][..], &[7, 0, 1, 0, 0, 0], &[7, 0, 0, 0, 0]] {
        assert!(read_state(body).is_err(), "{body:?}");
    }
}

#[test]
fn classes_keep_their_places() {
    let mut rows = [None; PERSONAL_SLOTS];
    (rows[0], rows[12], rows[31]) = (Some(1), Some(7), Some(u16::MAX));
    assert_eq!(State::Classes(Classes { field: 15, rows }).id(), 0x50f);
    round(State::Classes(Classes { field: 15, rows }));
    let body = round(State::Classes(Classes { field: 0, rows: [Some(3); PERSONAL_SLOTS] }));
    assert_eq!(body.len() + 2, CLASSES_BYTES);
    // Empty mask, field past the last, or class id 0 are refused.
    for body in [&[5, 0, 0, 0, 0, 0][..], &[5, 16, 1, 0, 0, 0, 1, 0], &[5, 0, 1, 0, 0, 0, 0, 0]] {
        assert!(read_state(body).is_err(), "{body:?}");
    }
    assert_eq!(cleared(0x50f), Some(Cleared::Classes(15)));
    assert_eq!(cleared(0x510), None);
}

#[test]
fn class_room_keeps_team_order_and_refuses_out_of_bounds_bits() {
    let cap = crate::battle::spawning::CLASS_CAPACITY as u8;
    let all = (1u16 << cap) - 1;
    let full = ClassRoom { count: [cap, cap], available: [all, 0b1010_0101_0011] };
    assert_eq!(State::ClassRoom(full).id(), 0x600);
    let body = round(State::ClassRoom(full));
    assert_eq!(body.len() + 2, CLASS_ROOM_BYTES);
    round(State::ClassRoom(ClassRoom::default()));
    round(State::ClassRoom(ClassRoom { count: [4, 0], available: [0b0101, 0] }));
    // Count past CLASS_CAPACITY or a bit set at or past count is refused.
    for body in [
        [TAG_CLASS_ROOM, cap + 1, 0, 0, 0, 0, 0],
        [TAG_CLASS_ROOM, 0, cap + 1, 0, 0, 0, 0],
        [TAG_CLASS_ROOM, 0, 0, 1, 0, 0, 0],
        [TAG_CLASS_ROOM, 3, 2, 0b1000, 0, 0, 0],
        [TAG_CLASS_ROOM, 3, 2, 0, 0, 0b0100, 0],
    ] {
        assert!(read_state(&body).is_err(), "{body:?}");
    }
    assert_eq!(cleared(0x600), Some(Cleared::ClassRoom));
    assert_eq!(cleared(0x601), None);

    // Bank detects class availability changes as deltas without reallocating.
    let mut bank = Bank::default();
    let mut one = |available| {
        bank.used = 0;
        bank.state(State::ClassRoom(ClassRoom { count: [4, 4], available }));
        (bank.offers()[0].state, bank.offers()[0].payload.as_ptr())
    };
    let (s0, p0) = one([0b1111, 0b1111]);
    let (s1, p1) = one([0b1111, 0b1111]);
    let (s2, p2) = one([0b1101, 0b1111]);
    let (s3, p3) = one([0b1101, 0b0111]);
    assert_eq!((s0, p0), (s1, p1));
    assert_eq!(p0, p2);
    assert_eq!(p0, p3);
    assert_ne!(s0, s2);
    assert_ne!(s2, s3);
}

#[test]
fn a_system_reads_back_as_written() {
    let name = "s".repeat(MAX_TEXT);
    let full = System { index: 31, alive: true, marker: true, team: 2, placed: Placed::Building(519), points: -20, health: Some([0., 900.5]),
        shield: Some([4000., 4000.]), hud: Some([0.25, 0.75]), name: &name };
    let mut body = Vec::new();
    write_system(&full, &mut body);
    assert_eq!(body.len() + 2, SYSTEM_BYTES);
    assert_eq!(read_system(&body), Ok(full));
    for (at, value) in [(0, 32), (1, 0x20), (2, 8), (3, 3), (10, 0), (14, 0), (22, 0), (30, 0)] {
        let mut odd = body.clone();
        odd[at] = value;
        if at >= 10 { odd[at..at + 4].copy_from_slice(&f32::NAN.to_le_bytes()); }
        assert!(read_system(&odd).is_err(), "byte {at}");
    }
    let bare = System { index: 0, name: "x", placed: Placed::Hull(9), ..Default::default() };
    body.clear();
    write_system(&bare, &mut body);
    assert_eq!(read_system(&body), Ok(bare));
    // A value of something it says is unknown, and a place nothing is at.
    let mut odd = body.clone();
    odd[10] = 1;
    assert!(read_system(&odd).is_err());
    let mut odd = body.clone();
    odd[3] = 0;
    assert!(read_system(&odd).is_err());
    body.push(0);
    assert!(read_system(&body).is_err());
}

#[test]
fn notices_read_back_as_written() {
    let victim = Actor { slot: 3, human: true, class: 12 };
    for cue in [Cue::Text("belfry.turn".into()), Cue::Hint("belfry.hint".into()), Cue::Voice("creak".into()),
        Cue::Kill { killer: None, victim, weapon: 0, gun: None }, Cue::Kill { killer: Some(Actor { slot: 0, human: false, class: 0 }), victim, weapon: 900, gun: None },
        Cue::Kill { killer: None, victim, weapon: 0, gun: Some("belfry_gun".into()) }, Cue::Kill { killer: None, victim, weapon: 0, gun: Some(String::new()) }] {
        let notice = Notice { sequence: 8, tick: 400, until: 700, cue };
        assert_eq!(Notice::read(&notice.body()), Ok(notice));
    }
    for body in [json!({}), json!([]), json!({"sequence":0,"tick":1,"until":2,"text":"a"}), json!({"sequence":1,"tick":1,"until":302,"text":"a"}),
        json!({"sequence":1,"tick":2,"until":1,"text":"a"}), json!({"sequence":1,"tick":1,"until":2,"text":""}),
        json!({"sequence":1,"tick":1,"until":2,"text":"a","hint":"b"}), json!({"sequence":1,"tick":1,"until":2}),
        json!({"sequence":1,"tick":1,"until":2,"shout":"a"}), json!({"sequence":1.5,"tick":1,"until":2,"text":"a"}),
        json!({"sequence":1,"tick":1,"until":2,"kill":[null,[1,true],3]}), json!({"sequence":1,"tick":1,"until":2,"kill":[[1,true,2],null,3]}),
        json!({"sequence":1,"tick":1,"until":2,"kill":[null,[70000,true,2],3]}), json!({"sequence":1,"tick":1,"until":2,"kill":[null,[1,1,2],3]}),
        json!({"sequence":1,"tick":1,"until":2,"text":"k".repeat(MAX_TEXT + 1)}), json!({"sequence":1u64 << 53,"tick":1,"until":2,"text":"a"})] {
        assert!(Notice::read(&body).is_err(), "{body}");
    }
}

#[test]
fn the_roster_names_classes_of_the_matched_registry() {
    let roster = Roster { teams: [Team { name: "blue".into(), icons: vec!["badge".into()], classes: vec![4, 9, 4] }, Team::default()] };
    assert_eq!(Roster::read(&roster.body(), |id| id == 4 || id == 9), Ok(roster.clone()));
    assert!(Roster::read(&roster.body(), |id| id == 4).is_err());
    let team = |classes: Value| json!({"name": "", "icons": [], "classes": classes});
    for body in [json!({"teams": []}), json!({"teams": [team(json!([])), team(json!([])), team(json!([]))]}),
        json!({"teams": [team(json!([0])), team(json!([]))]}), json!({"teams": [team(json!(vec![4; 13])), team(json!([]))]}),
        json!({"teams": [team(json!(["4"])), team(json!([]))]}), json!({"teams": [{"name": "", "icons": []}, team(json!([]))]}),
        json!({"teams": [{"name": "", "icons": [""], "classes": []}, team(json!([]))]}),
        json!({"teams": [{"name": "n".repeat(MAX_TEXT + 1), "icons": [], "classes": []}, team(json!([]))]}),
        json!({"teams": [{"name": "", "icons": vec!["i"; 5], "classes": []}, team(json!([]))]}),
        json!({"teams": [team(json!([])), team(json!([]))], "more": 1})] {
        assert!(Roster::read(&body, |_| true).is_err(), "{body}");
    }
}

#[test]
fn a_bank_keeps_its_storage_and_says_when_a_field_changed() {
    let mut bank = Bank::default();
    let mut one = |points| { bank.used = 0; bank.state(State::Match(Match { points, ..Default::default() })); (bank.offers()[0].state, bank.offers()[0].payload.as_ptr()) };
    let (first, at) = one([1, 2]);
    let (same, again) = one([1, 2]);
    let (changed, still) = one([1, 3]);
    assert_eq!((first, at), (same, again));
    assert_eq!(at, still);
    assert_ne!(first, changed);
}
