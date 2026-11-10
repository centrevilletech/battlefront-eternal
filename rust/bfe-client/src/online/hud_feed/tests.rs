use super::*;

// Commands and texts in the order the feed raised them.
#[derive(Default)]
struct Recorder {
    events: Vec<[f64; 5]>,
    texts: Vec<String>,
}
impl Sink for Recorder {
    fn event(&mut self, kind: u32, a: f64, b: f64, c: f64, d: f64) {
        self.events.push([kind as f64, a, b, c, d]);
    }
    fn text(&mut self, value: &str) -> u32 {
        self.texts.push(value.to_uppercase());
        self.texts.len() as u32
    }
}

fn tables() -> Tables {
    Tables {
        labels: vec![String::new(), "Republic Trooper".into(), String::new()],
        soldier_shots: HashMap::from([((1, 5), 0.2)]),
        seat_shots: HashMap::from([((3, 1, 9), 0.5)]),
        turret_shots: HashMap::from([(4, 0.75)]),
        posts: vec![None, None, None, Some("AT-AT".into())],
    }
}

// Team 1 and team 2 are enemies, and team 3 is a locals team.
fn teams() -> Teams {
    let mut relations = [[-1.; 8]; 8];
    for (team, row) in relations.iter_mut().enumerate() {
        row[0] = 0.;
        row[team] = 1.;
    }
    relations[0].fill(0.);
    Teams::fixture(relations, [false, false, false, true, false, false, false, false])
}

fn unit(id: u32, kind: u32, source: usize, team: f64) -> Unit {
    Unit { id, generation: 2, kind, source, team, radar: id as usize + 10 }
}

// The page's own unit is 7 (life 2) on team 1, facing yaw 0.
fn own() -> Own {
    Own { id: 7, generation: 2, team: 1, yaw: 0. }
}

fn world(id: u32) -> Option<Unit> {
    match id {
        7 => Some(unit(7, k::SOLDIER, 1, 1.)),
        8 => Some(unit(8, k::SOLDIER, 1, 2.)),
        9 => Some(unit(9, k::SOLDIER, 2, 1.)),
        20 => Some(unit(20, k::VEHICLE, 3, 2.)),
        21 => Some(unit(21, k::TURRET, 4, 2.)),
        _ => None,
    }
}

// A roster of the human "Rex" in slot 0 and the bot "Cody" in slot 1, with
// the local player's slot named by his spawn summary.
fn receiver() -> Receiver {
    let mut receiver = Receiver::new(serde_json::Value::Null);
    let mut roster = vec![2, 0, 0, 0, 2];
    for (human, name) in [(1, "Rex"), (0, "Cody")] {
        roster.extend([1, human, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 0, name.len() as u8]);
        roster.extend(name.bytes());
    }
    assert_eq!(receiver.roster.receive(4, 70000, &roster), Some(()));
    let mut spawn = vec![0; 24];
    spawn.extend([0, 0, 0, 0, 0]);
    assert_eq!(receiver.roster.receive(3, 65536, &spawn), Some(()));
    receiver
}

// encodeFeedback's layout, written by hand as the server writes it.
fn record(flags: u16, expiry: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for n in [expiry, 7, 2, 9, 3] {
        out.extend(n.to_le_bytes());
    }
    for n in [10., 1., -4.] {
        out.extend(f64::to_le_bytes(n));
    }
    out.extend(2i32.to_le_bytes());
    out.extend((-1i32).to_le_bytes());
    out.extend(flags.to_le_bytes());
    for n in [13., 1., 0.] {
        out.extend(f64::to_le_bytes(n));
    }
    out
}

fn feedback(target: (u32, u32), attacker: (u32, u32), team: i32, attacker_team: i32, source: Option<[f64; 3]>) -> Feedback {
    Feedback { target, attacker, position: [0.; 3], team, attacker_team, source }
}

#[test]
fn feedback_decodes_identities_teams_and_optional_source() {
    let event = decode(&record(1, 118), 100.).unwrap();
    assert_eq!(event.target, (7, 2));
    assert_eq!(event.attacker, (9, 3));
    assert_eq!(event.position, [10., 1., -4.]);
    assert_eq!((event.team, event.attacker_team), (2, -1));
    assert_eq!(event.source, Some([13., 1., 0.]));
    assert_eq!(decode(&record(0, 118), 100.).unwrap().source, None);
    // A wrong expiry, unknown flag or short record is refused whole.
    assert!(decode(&record(1, 117), 100.).is_none());
    assert!(decode(&record(2, 118), 100.).is_none());
    assert!(decode(&record(1, 118)[..77], 100.).is_none());
}

#[test]
fn damage_direction_matches_the_browser_convention() {
    // Facing yaw 0, a source straight ahead (+z) is 0 and one at -x is to
    // the right: the formula's sign, not a rotated copy of it.
    assert_eq!(damage_direction(0., 5., 0.), 0.);
    assert!((damage_direction(-5., 0., 0.) - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!((damage_direction(0., -5., 0.).abs() - std::f64::consts::PI).abs() < 1e-12);
    let yaw = std::f64::consts::FRAC_PI_2;
    assert!(damage_direction(5., 0., yaw).abs() < 1e-12);
}

#[test]
fn damage_dealt_marks_a_hit_and_damage_taken_points_at_its_source() {
    let (mut feed, mut hud, receiver, teams) = (Feed::new(tables()), Recorder::default(), receiver(), teams());
    // His own round on an enemy: the enemy ring. No arc, he was not the target.
    feed.present(&mut hud, &own(), &world, &receiver, &teams, false, &feedback((8, 2), (7, 2), 2, 1, Some([0., 0., 5.])));
    assert_eq!(hud.events, [[C::HIT as f64, 2., 0., 0., 0.]]);
    // Another life of his id is not him, and neither is "no attacker".
    feed.present(&mut hud, &own(), &world, &receiver, &teams, false, &feedback((8, 2), (7, 1), 2, 1, None));
    feed.present(&mut hud, &Own { id: 0, ..own() }, &world, &receiver, &teams, false, &feedback((8, 2), (0, 2), 2, -1, None));
    assert_eq!(hud.events.len(), 1);
    // Hurt from his left with a known source: the arc, and no hit ring.
    feed.present(&mut hud, &own(), &world, &receiver, &teams, false, &feedback((7, 2), (8, 2), 1, 2, Some([5., 0., 0.])));
    assert_eq!(hud.events[1][0], C::DAMAGE as f64);
    assert!((hud.events[1][1] + std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    // Without a source there is nothing to point at.
    feed.present(&mut hud, &own(), &world, &receiver, &teams, false, &feedback((7, 2), (8, 2), 1, 2, None));
    assert_eq!(hud.events.len(), 2);
    // Hurting himself raises both, the hit as a friendly one.
    feed.present(&mut hud, &own(), &world, &receiver, &teams, false, &feedback((7, 2), (7, 2), 1, 1, Some([0., 0., 5.])));
    assert_eq!(hud.events[2], [C::HIT as f64, 1., 0., 0., 0.]);
    assert_eq!(hud.events[3], [C::DAMAGE as f64, 0., 0., 0., 0.]);
}

#[test]
fn kill_lines_name_characters_and_only_show_for_humans_and_heroes() {
    let (mut feed, mut hud, receiver, teams) = (Feed::new(tables()), Recorder::default(), receiver(), teams());
    let state = |id: u32, slot: u16, flags: u8| {
        let mut bytes = vec![0; 21];
        bytes[..4].copy_from_slice(&id.to_le_bytes());
        bytes[4] = flags;
        bytes[UNIT_STATE_SLOT..UNIT_STATE_SLOT + 2].copy_from_slice(&slot.to_le_bytes());
        let mut row = [0.; r::STRIDE];
        row[r::ID] = UNIT_STATE_ID + id as f64;
        row[r::GENERATION] = 2.;
        (row, bytes)
    };
    // Two bots: no line.
    feed.present(&mut hud, &own(), &world, &receiver, &teams, true, &feedback((9, 2), (8, 2), 1, 2, None));
    assert!(hud.events.is_empty());
    // The same kill once the killer is a hero: bots by their class labels,
    // the one without a label as "Soldier", in the enemy's colour.
    let (row, bytes) = state(8, NO_SLOT, 1);
    feed.unit_state(&row, &bytes, &own());
    feed.present(&mut hud, &own(), &world, &receiver, &teams, true, &feedback((9, 2), (8, 2), 1, 2, None));
    assert_eq!(hud.texts, ["REPUBLIC TROOPER", "SOLDIER"]);
    assert_eq!(hud.events, [[C::KILL as f64, 3., 1., 2., 2.]]);
    // The local player killed by a named bot of the enemy.
    let (row, bytes) = state(8, 1, 0);
    feed.unit_state(&row, &bytes, &own());
    feed.present(&mut hud, &own(), &world, &receiver, &teams, true, &feedback((7, 2), (8, 2), 1, 2, None));
    assert_eq!(hud.texts[2..], ["CODY", "REX"]);
    assert_eq!(hud.events[1], [C::KILL as f64, 3., 3., 4., 2.]);
    // A teammate, his own hand and no attacker at all: the other three
    // kinds, reusing the names already interned.
    feed.present(&mut hud, &own(), &world, &receiver, &teams, true, &feedback((7, 2), (9, 2), 1, 1, None));
    feed.present(&mut hud, &own(), &world, &receiver, &teams, true, &feedback((7, 2), (7, 2), 1, 1, None));
    feed.present(&mut hud, &own(), &world, &receiver, &teams, true, &feedback((7, 2), (0, 0), 1, -1, None));
    assert_eq!(hud.events[2], [C::KILL as f64, 2., 2., 4., 1.]);
    assert_eq!(hud.events[3], [C::KILL as f64, 1., 4., 4., 1.]);
    assert_eq!(hud.events[4], [C::KILL as f64, 0., 0., 4., 0.]);
    assert_eq!(hud.texts.len(), 4);
    // A victim this page does not have, or another life of him, says nothing.
    feed.present(&mut hud, &own(), &world, &receiver, &teams, true, &feedback((99, 2), (7, 2), 2, 1, None));
    feed.present(&mut hud, &own(), &world, &receiver, &teams, true, &feedback((8, 5), (7, 2), 2, 1, None));
    assert_eq!(hud.events.len(), 5);
    // A state for another life of a unit does not make it a hero.
    let (mut row, bytes) = state(9, NO_SLOT, 1);
    row[r::GENERATION] = 1.;
    feed.unit_state(&row, &bytes, &own());
    assert!(feed.state(&world(9).unwrap()).is_none());
}

#[test]
fn a_local_player_without_a_roster_name_is_player_one() {
    let (mut feed, mut hud, teams) = (Feed::new(tables()), Recorder::default(), teams());
    let receiver = Receiver::new(serde_json::Value::Null);
    feed.present(&mut hud, &own(), &world, &receiver, &teams, true, &feedback((7, 2), (0, 0), 1, -1, None));
    assert_eq!(hud.texts, ["PLAYER 1"]);
}

#[test]
fn shots_flash_the_mount_or_the_shooter_for_that_weapon_s_delay() {
    let (feed, mut hud) = (Feed::new(tables()), Recorder::default());
    let shot = |owner: u32, mount: u32, seat: usize, weapon: u32| Shot {
        owner_id: owner,
        owner_generation: 2,
        mount_id: mount,
        mount_generation: 2,
        seat,
        weapon_id: weapon,
    };
    // On foot: his own weapon of that name.
    assert!(feed.shot(&mut hud, &shot(8, 0, 0, 5), &world));
    assert_eq!(hud.events, [[C::SHOT as f64, 18., 2., 0.2, 0.]]);
    // From a seat the vehicle flashes, for the seat's weapon; a turret
    // flashes for its one weapon whatever the event names.
    assert!(feed.shot(&mut hud, &shot(8, 20, 1, 9), &world));
    assert!(feed.shot(&mut hud, &shot(8, 21, 0, 77), &world));
    assert_eq!(hud.events[1], [C::SHOT as f64, 30., 2., 0.5, 0.]);
    assert_eq!(hud.events[2], [C::SHOT as f64, 31., 2., 0.75, 0.]);
    // A seat without that weapon falls back to the rider's own.
    assert!(feed.shot(&mut hud, &shot(8, 20, 0, 5), &world));
    assert_eq!(hud.events[3], [C::SHOT as f64, 30., 2., 0.2, 0.]);
    // An unknown unit, another life or an unknown weapon waits.
    assert!(!feed.shot(&mut hud, &shot(99, 0, 0, 5), &world));
    assert!(!feed.shot(&mut hud, &shot(8, 99, 0, 5), &world));
    assert!(!feed.shot(&mut hud, &Shot { owner_generation: 1, ..shot(8, 0, 0, 5) }, &world));
    assert!(!feed.shot(&mut hud, &shot(8, 0, 0, 6), &world));
    assert_eq!(hud.events.len(), 4);
}

#[test]
fn the_score_is_the_local_character_s_or_the_first_human_s() {
    let mut receiver = Receiver::new(serde_json::Value::Null);
    assert_eq!(score(&receiver.roster), [0.; 4]);
    // A bot with a score, then a human with another: kills, team kills,
    // deaths and captures as the roster carries them.
    let mut roster = vec![2, 0, 0, 0, 2];
    roster.extend([1, 0, 9, 0, 0, 0, 9, 0, 9, 0, 255, 255, 0, 0]);
    roster.extend([1, 1, 7, 0, 1, 0, 3, 0, 4, 0, 255, 255, 0, 0]);
    assert_eq!(receiver.roster.receive(4, 70000, &roster), Some(()));
    assert_eq!(score(&receiver.roster), [7., 1., 3., 4.]);
    // His spawn summary names slot 0: that Character, human or not.
    let mut spawn = vec![0; 24];
    spawn.extend([0, 0, 0, 0, 0]);
    assert_eq!(receiver.roster.receive(3, 65536, &spawn), Some(()));
    assert_eq!(score(&receiver.roster), [9., 0., 9., 9.]);
}

#[test]
fn post_summaries_announce_captures_losses_and_destroyed_posts() {
    // The first summary only records the post; the same owner says nothing.
    assert_eq!(post_change(None, 1, true, false), None);
    assert_eq!(post_change(Some((1, true)), 1, true, false), None);
    // Changing hands names the new owner; going neutral names the old one.
    assert_eq!(post_change(Some((1, true)), 2, true, false), Some(("game.infomsg.captured", 2)));
    assert_eq!(post_change(Some((0, true)), 2, true, false), Some(("game.infomsg.captured", 2)));
    assert_eq!(post_change(Some((1, true)), 0, true, false), Some(("game.infomsg.lost", 1)));
    // A destructible post destroyed is lost by its owner, once.
    assert_eq!(post_change(Some((2, true)), 2, false, true), Some(("game.infomsg.lost", 2)));
    assert_eq!(post_change(Some((2, false)), 2, false, true), None);
    assert_eq!(post_change(Some((2, true)), 2, false, false), None);
    assert_eq!(fill(Some("%s CAPTURED THE %s"), &["REPUBLIC", "PLAZA"]), "REPUBLIC CAPTURED THE PLAZA");
    assert_eq!(fill(Some("%s LOST THE %s"), &["", "PLAZA"]), " LOST THE PLAZA");
    assert_eq!(fill(None, &["REPUBLIC", "PLAZA"]), "REPUBLIC PLAZA");
}

#[test]
fn messages_and_the_player_s_own_squad_orders_reach_the_hud() {
    let (mut feed, mut hud) = (Feed::new(tables()), Recorder::default());
    feed.message(&mut hud, "Republic captured the Plaza", 1.);
    feed.message(&mut hud, "Republic captured the Plaza", 2.);
    assert_eq!(hud.texts, ["REPUBLIC CAPTURED THE PLAZA"]);
    assert_eq!(hud.events, [[C::MESSAGE as f64, 1., 1., 0., 0.], [C::MESSAGE as f64, 1., 2., 0., 0.]]);
    assert_eq!(["moveout", "hold", "follow", "atease", "charge"].map(squad_order), [Some(0.), Some(1.), Some(2.), Some(3.), None]);
    // A squad event of his own unit lights its line; another unit's does not.
    let mut receiver = receiver();
    receiver.corrections = 1;
    receiver.own.id = 7.;
    receiver.own.generation = 2.;
    let mut push = |id: f64, json: &str| {
        let mut row = [0.; r::STRIDE];
        row[r::KIND] = k::COMBAT as f64;
        row[r::ID] = id;
        row[r::GENERATION] = 1.;
        row[r::SCHEMA] = 8.;
        receiver.publications.push(&row, json.as_bytes());
    };
    push(1., r#"{"kind":"squad","ownerId":7,"ownerGeneration":2,"label":"follow"}"#);
    push(2., r#"{"kind":"squad","ownerId":8,"ownerGeneration":2,"label":"hold"}"#);
    push(3., r#"{"kind":"squad","ownerId":7,"ownerGeneration":1,"label":"hold"}"#);
    feed.frame(&mut hud, &receiver, &teams());
    assert_eq!(hud.events[2..], [[C::SQUAD as f64, 2., 0., 0., 0.]]);
}

#[test]
fn a_command_vehicle_that_stops_living_loses_its_post_once() {
    let mut feed = Feed::new(tables());
    let mut receiver = Receiver::new(serde_json::Value::Null);
    // The receiver's own record stands in for a hull: a living vehicle of
    // the command class 3, on team 2.
    receiver.corrections = 1;
    receiver.own.id = 40.;
    let fact = &mut receiver.own_fact;
    fact[r::KIND] = k::VEHICLE as f64;
    fact[r::ID] = 40.;
    fact[r::GENERATION] = 5.;
    fact[r::CLASS_ID] = 3.;
    fact[r::TEAM] = 2.;
    fact[r::HEALTH] = 900.;
    assert_eq!(feed.lost_posts(&receiver).1, 0);
    assert_eq!(feed.lost_posts(&receiver).1, 0);
    // Burning out, it is no post any more: lost by the team it had.
    receiver.own_fact[r::STATE] = DYING;
    assert_eq!(feed.lost_posts(&receiver), ([(2, 3), (0, 0), (0, 0), (0, 0)], 1));
    assert_eq!(feed.lost_posts(&receiver).1, 0);
    assert_eq!((feed.post_label(3), feed.post_label(1)), ("AT-AT", ""));
    // A vehicle of another class is never a post, and a new battle forgets
    // the old one's hulls without announcing them.
    receiver.own_fact[r::STATE] = 0.;
    receiver.own_fact[r::CLASS_ID] = 1.;
    assert_eq!(feed.lost_posts(&receiver).1, 0);
    receiver.own_fact[r::CLASS_ID] = 3.;
    assert_eq!(feed.lost_posts(&receiver).1, 0);
    receiver.epoch = 2.;
    receiver.own_fact[r::HEALTH] = 0.;
    assert_eq!(feed.lost_posts(&receiver).1, 0);
}

#[test]
fn the_own_soldier_s_repair_lock_building_health_and_vehicle_slots_are_kept() {
    assert_eq!(repair_lock(b"l3"), Some(RepairLock::Building(3)));
    assert_eq!(repair_lock(b"v12"), Some(RepairLock::Vehicle(12)));
    assert_eq!([repair_lock(b""), repair_lock(b"x1"), repair_lock(b"l")], [None; 3]);
    let (mut feed, mut hud, teams) = (Feed::new(tables()), Recorder::default(), teams());
    let mut receiver = Receiver::new(serde_json::Value::Null);
    receiver.corrections = 1;
    receiver.own.id = 7.;
    receiver.own.generation = 2.;
    let mut push = |kind: u32, id: f64, generation: f64, schema: f64, slot: f64, bytes: &[u8]| {
        let mut row = [0.; r::STRIDE];
        row[r::KIND] = kind as f64;
        row[r::ID] = id;
        row[r::GENERATION] = generation;
        row[r::SCHEMA] = schema;
        row[r::SLOT] = slot;
        receiver.publications.push(&row, bytes);
    };
    // His own unit state with the key "l3": id, flags, two clocks, no roster
    // slot, two selected weapons, no weapons, then the key's length and text.
    let mut state = vec![7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 0, 2];
    state.extend(b"l3");
    push(k::SUMMARY, UNIT_STATE_ID + 7., 2., 6., 0., &state);
    // Another unit's lock is not his; building 3 at 450; slot 1 holds hull 40.
    let mut other = state.clone();
    other[0] = 8;
    other[22] = b'9';
    push(k::SUMMARY, UNIT_STATE_ID + 8., 2., 6., 0., &other);
    push(k::SUMMARY, BUILDING_STATE_ID + 3., 1., 7., 0., br#"[3,"built",450,2,null,0,0,0,0,false,false]"#);
    push(k::SUMMARY, BUILDING_STATE_ID + 4., 1., 7., 0., br#"[4,"built",null,0,"idle",0,0,0,0,false,false]"#);
    push(k::MOUNT, 40., 5., 0., 1., &[]);
    push(k::MOUNT, 41., 5., 0., -1., &[]);
    feed.frame(&mut hud, &receiver, &teams);
    assert_eq!(feed.repair(&receiver), Some(RepairLock::Building(3)));
    assert_eq!((feed.building_health(3), feed.building_health(4), feed.building_health(5)), (Some(450.), Some(f64::INFINITY), None));
    assert_eq!((feed.vehicle(1), feed.vehicle(0)), (Some((40, 5)), None));
    // A new life of his has no lock until its own state says so.
    receiver.own.generation = 3.;
    assert_eq!(feed.repair(&receiver), None);
}

#[test]
fn relations_and_kill_kinds_follow_the_browser() {
    assert_eq!(kill_kind(None, unit(1, k::SOLDIER, 0, 1.)), 0.);
    assert_eq!(kill_kind(Some(unit(1, k::SOLDIER, 0, 1.)), unit(1, k::SOLDIER, 0, 1.)), 1.);
    assert_eq!(kill_kind(Some(unit(2, k::SOLDIER, 0, 1.)), unit(1, k::SOLDIER, 0, 1.)), 2.);
    assert_eq!(kill_kind(Some(unit(2, k::SOLDIER, 0, 2.)), unit(1, k::SOLDIER, 0, 1.)), 3.);
    assert_eq!([relation_code(1.), relation_code(-1.), relation_code(0.)], [1., 2., 0.]);
    assert_eq!((viewer(-1), index(-1)), (u8::MAX, usize::MAX));
}

#[test]
fn the_model_takes_uppercase_names() {
    let mut hud = HudModel::new();
    let mut texts = HashMap::new();
    let first = text(&mut texts, &mut hud, "CT-7567 Rex");
    assert_eq!(text(&mut texts, &mut hud, "CT-7567 Rex"), first);
    assert_eq!(hud.texts[first as usize], "CT-7567 REX".encode_utf16().collect::<Vec<_>>());
    assert_ne!(text(&mut texts, &mut hud, SOLDIER), first);
}
