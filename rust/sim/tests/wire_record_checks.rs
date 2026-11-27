//! A 2005 match's receiver checks every 2005 record before anything of its
//! part is stored or acknowledged, finds the kind, revision and extent of a
//! record it does not know from the carrier alone, and skips what the match
//! marks optional; a 2004 battle's receiver reads every field as released.
use bfe_presentation::{network::{RecordChecks, Session}, network_layout::net_header as h};
use bfe_sim::gen::{WireField, WireSnapshot};
use bfe_sim::movement::soldier::swbf2::{body, ControlSpeed, Tuning};
use bfe_sim::wire::{buffered::Encoder, codec::{own_datagram_into, OwnParts, Writer}, revisions::{self, Checks, Entry, Plan, Supports, Table,
    SUPPORTS, WRITES}, PositionBounds};

fn tuning() -> Tuning {
    let row = ControlSpeed { forward: 1.0, strafe: 1.0, turn: 1.0 };
    Tuning { stand: row, crouch: row, sprint: row, jump: row, jet: row, jetpack: None, roll: row, tumble: row,
        sprint_accelerate_time: 0.4, energy_capacity: 70.0, energy_floor: 0.0, energy_restore: 4.0, energy_restore_idle: 6.0,
        energy_drain_sprint: 9.0, energy_min_sprint: 12.0, energy_cost_jump: 3.0, energy_cost_jump_sprint: None, energy_cost_roll: 11.0,
        jump_height: 1.2, jump_forward_speed_factor: 1.1, jump_strafe_speed_factor: 0.9, roll_speed_factor: 1.7, roll_frames: 25,
        recover_from_tumble: false, getup_front_seconds: 1.0, getup_back_seconds: 1.0 }
}

fn checks(plan: Plan) -> Box<dyn RecordChecks> { Box::new(Checks::new(plan, vec![None, Some(tuning())])) }

fn plan_of(table: &[Entry]) -> Plan { revisions::plan(&Table::of(table), &Supports::of(SUPPORTS)).unwrap() }

// One complete snapshot carrying `fields`, through a receiver with or
// without 2005 checks: whether it completed, and the world fields published.
fn deliver(fields: Vec<WireField>, checks: Option<Box<dyn RecordChecks>>, correction: Option<Vec<u8>>) -> (Result<(), &'static str>, bool, f64) {
    let bounds = PositionBounds::LEGACY;
    let snapshot = WireSnapshot { tick: 3., epoch: 1., slots: vec![], world: fields };
    let mut encoder = Encoder::new(8);
    let parts = encoder.encode(&snapshot, None, 3., bounds.width()).unwrap();
    let mut receiver = Session::new();
    receiver.reserve_receive();
    if let Some(checks) = checks { receiver.check_records(checks); }
    let mut correction_section = Writer::default();
    if let Some(payload) = &correction { bfe_sim::wire::codec::own_correction_into(&mut correction_section, 7., 1., 11., payload, None).unwrap(); }
    let mut own = Writer::default();
    own_datagram_into(&mut own, OwnParts { epoch: 1., tick: 3., interval: 3., baseline: 0., ack: 0., second: false, count: 1.,
        correction: correction.as_ref().map(|_| correction_section.0.as_slice()), summaries: None, cosmetics: None }).unwrap();
    let (mut result, mut records) = (Ok(()), 0.);
    for packet in parts.iter().map(Vec::as_slice).chain(std::iter::once(own.0.as_slice())) {
        if let Err(e) = receiver.receive_with_bounds(packet, &bounds) { result = Err(e); }
        records += receiver.header[h::RECORDS];
    }
    (result, receiver.header[h::ACK_TICK] == 3. && receiver.header[h::COMPLETE] != 0., records)
}

fn field(id: f64, schema: u16, payload: Vec<u8>) -> WireField { WireField { id, schema: f64::from(schema), generation: 1., payload } }

// The packets of one snapshot and its own datagram through `receiver`: the
// first error, and whether the snapshot was acknowledged.
fn feed(receiver: &mut Session, parts: &[Vec<u8>], tick: f64, baseline: f64) -> (Result<(), &'static str>, bool) {
    let bounds = PositionBounds::LEGACY;
    let mut own = Writer::default();
    own_datagram_into(&mut own, OwnParts { epoch: 1., tick, interval: 3., baseline, ack: 0., second: false, count: 1.,
        correction: None, summaries: None, cosmetics: None }).unwrap();
    let mut result = Ok(());
    for packet in parts.iter().map(Vec::as_slice).chain(std::iter::once(own.0.as_slice())) {
        if let Err(e) = receiver.receive_with_bounds(packet, &bounds) { result = Err(e); }
    }
    (result, receiver.header[h::ACK_TICK] == tick && receiver.header[h::COMPLETE] != 0.)
}

// A keyframe of `sent` at tick 3, then `second` at tick 6 written against
// `assumed` as the tick 3 version, through one receiver: whether the
// keyframe was acknowledged, and the second snapshot's result and
// acknowledgement.
fn deliver_pair(sent: Vec<WireField>, assumed: Vec<WireField>, second: Vec<WireField>, checks: Box<dyn RecordChecks>)
    -> (bool, Result<(), &'static str>, bool) {
    let width = PositionBounds::LEGACY.width();
    let at = |tick: f64, world: Vec<WireField>| WireSnapshot { tick, epoch: 1., slots: vec![], world };
    let mut encoder = Encoder::new(8);
    let mut receiver = Session::new();
    receiver.reserve_receive();
    receiver.check_records(checks);
    let parts = encoder.encode(&at(3., sent), None, 3., width).unwrap().to_vec();
    let (keyframe, acknowledged) = feed(&mut receiver, &parts, 3., 0.);
    let parts = encoder.encode(&at(6., second), Some(&at(3., assumed)), 3., width).unwrap().to_vec();
    let (result, delta) = feed(&mut receiver, &parts, 6., 3.);
    (keyframe.is_ok() && acknowledged, result, delta)
}
fn record(revision: u16, body: &[u8]) -> Vec<u8> { [&revision.to_le_bytes()[..], body].concat() }

#[test]
fn a_2005_match_refuses_invalid_and_unnamed_records_a_2004_battle_reads_them_as_released() {
    let display = revisions::field_schema(revisions::SOLDIER_DISPLAY);
    for (schema, payload) in [(display, record(1, &[0xff])), (display, record(2, &[0, 128, 8])), (display, vec![1]),
        (revisions::field_schema(revisions::COMBAT_DISPLAY), record(1, &[1, 2])), (0x0277, record(1, &[]))]
    {
        // As released: any field is published and the snapshot acknowledged.
        let (result, complete, _) = deliver(vec![field(2., schema, payload.clone())], None, None);
        assert!(result.is_ok() && complete, "2004 {schema:#x}");
        // A 2005 match acknowledges nothing of that snapshot.
        let (result, complete, _) = deliver(vec![field(2., schema, payload)], Some(checks(plan_of(WRITES))), None);
        assert!(result.is_err() && !complete, "2005 {schema:#x}: {result:?}");
    }
    // A valid display field is read and acknowledged.
    let (result, complete, records) = deliver(vec![field(100_002., display, record(1, &[0, 128, 8]))], Some(checks(plan_of(WRITES))), None);
    assert!(result.is_ok() && complete && records >= 1.);
}

// A mission's fields are read whole before anything of their snapshot is
// acknowledged: one of another revision, one that does not read, and one of a
// match that does not carry the kind leave it unacknowledged.
#[test]
fn a_mission_field_is_checked_before_its_snapshot_is_acknowledged() {
    use bfe_sim::wire::mission::{write_state, write_system, ClassRoom, Match, State, System, STATE_SCHEMA, SYSTEM_SCHEMA};
    let (mut state, mut room, mut system) = (Vec::new(), Vec::new(), Vec::new());
    write_state(&State::Match(Match { points: [40, 10], ..Default::default() }), &mut state);
    let room_state = State::ClassRoom(ClassRoom { count: [4, 3], available: [0b0101, 0b110] });
    write_state(&room_state, &mut room);
    write_system(&System { index: 2, name: "generator", ..Default::default() }, &mut system);
    for (id, schema, body) in [(0., STATE_SCHEMA, &state), (f64::from(room_state.id()), STATE_SCHEMA, &room), (2., SYSTEM_SCHEMA, &system)] {
        let sent = |payload: Vec<u8>, table: &[Entry]| deliver(vec![field(1., 7, vec![9; 12]), field(id, schema, payload), field(3., 7, vec![8; 12])],
            Some(checks(plan_of(table))), None);
        let (result, complete, records) = sent(record(1, body), WRITES);
        assert!(result.is_ok() && complete && records == 3., "{schema:#x}: {result:?}");
        let mut long = body.clone();
        long.push(0);
        for payload in [record(2, body), record(0, body), record(1, &body[..body.len() - 1]), record(1, &long), record(1, &[]), vec![1], vec![]] {
            let (result, complete, _) = sent(payload.clone(), WRITES);
            assert!(result.is_err() && !complete, "{schema:#x} {payload:?}");
        }
        let without: Vec<Entry> = WRITES.iter().copied().filter(|entry| revisions::field_schema(entry.kind) != schema).collect();
        let (result, complete, _) = sent(record(1, body), &without);
        assert!(result.is_err() && !complete, "{schema:#x}: {result:?}");
    }
}

#[test]
fn a_field_whose_id_does_not_match_its_body_is_refused() {
    use bfe_sim::wire::mission::{write_state, write_system, Match, State, System, STATE_SCHEMA, SYSTEM_SCHEMA};
    let display = revisions::field_schema(revisions::SOLDIER_DISPLAY);
    let (mut state, mut system) = (Vec::new(), Vec::new());
    write_state(&State::Match(Match { points: [40, 10], ..Default::default() }), &mut state);
    write_system(&System { index: 2, name: "generator", ..Default::default() }, &mut system);
    for (id, schema, payload) in [
        (2., display, record(1, &[0, 128, 8])),
        (100_512., display, record(1, &[0, 128, 8])),
        (2., STATE_SCHEMA, record(1, &state)),
        (3., SYSTEM_SCHEMA, record(1, &system)),
    ] {
        let (result, complete, _) = deliver(vec![field(id, schema, payload)], Some(checks(plan_of(WRITES))), None);
        assert!(result.is_err() && !complete, "id={id} schema={schema:#x}: {result:?}");
    }
}

// Optional records of kinds this build does not implement, between two
// ordinary fields, in a match whose table names them.
#[test]
fn optional_records_this_build_does_not_implement_are_skipped_whole() {
    let table = [WRITES[0], WRITES[1], WRITES[2], Entry { kind: revisions::COMBAT_DISPLAY, revision: 1, required: false },
        Entry { kind: 0x77, revision: 9, required: false }];
    let fields = vec![field(1., 7, vec![9; 12]), field(2., revisions::field_schema(revisions::COMBAT_DISPLAY), record(1, &[0xff; 5])),
        field(3., 0x0277, record(9, &[1, 2, 3])), field(4., 7, vec![8; 12])];
    let (result, complete, records) = deliver(fields.clone(), Some(checks(plan_of(&table))), None);
    assert!(result.is_ok() && complete, "{result:?}");
    // Both surrounding fields are published; neither skipped record is.
    let (_, _, all) = deliver(fields, None, None);
    assert_eq!((records, all), (2., 4.));
    // A display field of a revision this build does not read, in a match
    // whose optional display is that revision, is skipped too.
    let newer = [WRITES[0], WRITES[1], Entry { kind: revisions::SOLDIER_DISPLAY, revision: 2, required: false }];
    let (result, complete, records) = deliver(vec![field(2., revisions::field_schema(revisions::SOLDIER_DISPLAY), record(2, &[0xff; 7]))],
        Some(checks(plan_of(&newer))), None);
    assert!(result.is_ok() && complete && records == 0.);
}

// An optional kind this build does not implement, advertised at revision 2,
// between two ordinary fields: a well-formed record of revision 2 is skipped
// and both neighbours are published. A payload without its revision, a
// one-byte header, revision zero or a revision the match never advertised
// leaves the snapshot unacknowledged, whether written whole or as a word
// delta; a delta that keeps the revision word of a held version passes.
#[test]
fn a_skipped_record_shows_the_advertised_revision_before_it_is_passed_over() {
    let table = [WRITES[0], WRITES[1], revisions::written(revisions::SIT_OUT).unwrap(), Entry { kind: 91, revision: 2, required: false }];
    let skipped = revisions::field_schema(91);
    let around = |payload: Vec<u8>| vec![field(1., 7, vec![9; 12]), field(2., skipped, payload), field(3., 7, vec![8; 12])];
    let (result, complete, records) = deliver(around(record(2, &[11])), Some(checks(plan_of(&table))), None);
    assert!(result.is_ok() && complete && records == 2., "{result:?} {records}");
    for payload in [vec![], vec![2], record(0, &[]), record(3, &[]), record(3, &[11])] {
        let (result, complete, _) = deliver(around(payload.clone()), Some(checks(plan_of(&table))), None);
        assert!(result.is_err() && !complete, "{payload:?}: {result:?}");
    }
    // Word deltas of a 64-byte record of revision 2, rebuilt from the
    // acknowledged version and checked whole: a change in its last word keeps
    // the revision word and passes; a change of the revision word to 3 is
    // refused, and so is a delta of a version this receiver never held.
    let long = |revision: u16, last: u8| record(revision, &[[5; 61].as_slice(), &[last]].concat());
    let (keyframe, result, acknowledged) = deliver_pair(around(long(2, 14)), around(long(2, 14)), around(long(2, 15)), checks(plan_of(&table)));
    assert!(keyframe && result.is_ok() && acknowledged, "{result:?}");
    let (keyframe, result, acknowledged) = deliver_pair(around(long(2, 14)), around(long(2, 14)), around(long(3, 14)), checks(plan_of(&table)));
    assert!(keyframe && result.is_err() && !acknowledged, "{result:?}");
    let neighbours = || vec![field(1., 7, vec![9; 12]), field(3., 7, vec![8; 12])];
    let (keyframe, result, acknowledged) = deliver_pair(neighbours(), around(long(2, 14)), around(long(2, 15)), checks(plan_of(&table)));
    assert!(keyframe && result.is_err() && !acknowledged, "{result:?}");
}

// A complete own correction: header, the counted body, the life facts and
// weapon view, an empty arsenal, then the control section.
fn own_payload(class: u16, body_schema: u8, body: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(1u32.to_le_bytes()); b.extend(1u32.to_le_bytes()); b.extend(class.to_le_bytes()); b.push(1); b.push(body_schema);
    b.extend(0u32.to_le_bytes()); b.extend(0u32.to_le_bytes());
    b.extend((body.len() as u16).to_le_bytes()); b.extend_from_slice(body);
    b.extend(100f64.to_le_bytes()); b.push(0); b.extend(0f64.to_le_bytes()); b.extend(0f64.to_le_bytes()); b.push(0);
    for _ in 0..3 { b.extend(0f64.to_le_bytes()); }
    b.push(0); b.extend(1f64.to_le_bytes()); b.push(0); b.extend([255, 255]);
    b.push(0);
    b.extend([0; 16]); b.extend(0u16.to_le_bytes()); b.extend(0u16.to_le_bytes()); b.extend(0u32.to_le_bytes()); b.push(0);
    b
}
// A standing, grounded compact body, then a 2005 suffix.
fn compact(suffix: &[u8]) -> Vec<u8> {
    let mut b = vec![0, 0, 0, 1];
    for n in [0f64; 6].into_iter().chain([1., 2., 3., 0., 0., 0., 0., 1., 0.]) { b.extend(n.to_le_bytes()); }
    b.extend_from_slice(suffix);
    b
}

#[test]
fn a_2005_correction_is_checked_whole_before_it_commits() {
    let idle = [&50f64.to_le_bytes()[..], &[0], &0f64.to_le_bytes(), &0u32.to_le_bytes()].concat();
    let over = [&90f64.to_le_bytes()[..], &[0], &0f64.to_le_bytes(), &0u32.to_le_bytes()].concat();
    let soldier = |suffix: &[u8]| record(1, &compact(suffix));
    let complete = own_payload(1, body::SCHEMA, &soldier(&idle));
    // The envelope the header-only reader accepted: cut before its control section.
    let header_only = complete[..complete.len() - 25].to_vec();
    for (payload, accepted) in [
        (complete.clone(), true),
        (header_only, false),
        // Energy beyond the class's capacity.
        (own_payload(1, body::SCHEMA, &soldier(&over)), false),
        // A class without movement numbers, and no class at all.
        (own_payload(2, body::SCHEMA, &soldier(&idle)), false),
        (own_payload(0, body::SCHEMA, &soldier(&idle)), false),
        // A 2004 soldier body cannot appear in a 2005 match, nor the soldier body's next revision.
        (own_payload(1, 6, &compact(&[])), false),
        (own_payload(1, body::SCHEMA, &record(2, &compact(&idle))), false),
        // A compact body whose flag claims more bytes than the body holds.
        (own_payload(1, body::SCHEMA, &record(1, &[1, 0, 0, 1])), false),
        // A 2005 body of a kind the match does not carry.
        (own_payload(1, revisions::body_schema(revisions::COMBAT), &record(1, &[])), false),
    ] {
        let (result, complete, _) = deliver(vec![], Some(checks(plan_of(WRITES))), Some(payload));
        assert_eq!((result.is_ok(), complete), (accepted, accepted), "{result:?}");
    }
    // A match whose table lacks the soldier body cannot be described at all, so
    // its corrections never meet a plan without it.
    let empty = revisions::Descriptor { mode: "eli".into(), map: "swbf2/kam9".into(), token: "0".repeat(32), records: Table::default() };
    assert!(revisions::Descriptor::read(&empty.json()).is_err());
    let _ = complete;
}

// The outer facts and the payload together: one life of one unit is one
// class, while a late correction of an earlier life or of the unit before a
// respawn is the server's own and is read, not refused.
#[test]
fn a_life_keeps_its_class_and_late_corrections_are_not_refused() {
    let idle = [&50f64.to_le_bytes()[..], &[0], &0f64.to_le_bytes(), &0u32.to_le_bytes()].concat();
    let soldier = |class: u16| own_payload(class, body::SCHEMA, &record(1, &compact(&idle)));
    let checks = Checks::new(plan_of(WRITES), vec![None, Some(tuning()), Some(tuning())]);
    assert!(checks.correction(1., 3., 7, 1, 11, &soldier(1)).is_ok());
    assert!(checks.correction(1., 4., 7, 1, 11, &soldier(2)).is_err());
    assert!(checks.correction(1., 4., 7, 1, 12, &soldier(1)).is_err());
    // A new life, then a late one of the old life, then the unit of a respawn.
    for (tick, unit, life, class) in [(5., 7, 2, 2), (4., 7, 1, 1), (6., 9, 1, 1), (7., 7, 2, 2)] {
        assert!(checks.correction(1., tick, unit, life, 11, &soldier(class)).is_ok(), "{unit} {life} {class}");
    }
    // A new epoch starts afresh: the same unit and life may name another class.
    assert!(checks.correction(2., 1., 7, 2, 11, &soldier(1)).is_ok());
}

// Four own datagrams through one receiver, delivered out of order: unit 7 is
// class 1 in life 1 at tick 3 and class 2 in life 2 at tick 9; a late tick 6
// from life 1 is read without replacing the accepted identity; tick 12 then
// names class 1 for life 2 and is refused before anything of it is published
// or acknowledged.
#[test]
fn a_late_correction_cannot_reset_the_class_of_the_current_life() {
    let idle = [&50f64.to_le_bytes()[..], &[0], &0f64.to_le_bytes(), &0u32.to_le_bytes()].concat();
    let soldier = |class: u16| own_payload(class, body::SCHEMA, &record(1, &compact(&idle)));
    let bounds = PositionBounds::LEGACY;
    let mut receiver = Session::new();
    receiver.reserve_receive();
    receiver.check_records(Box::new(Checks::new(plan_of(WRITES), vec![None, Some(tuning()), Some(tuning())])));
    let mut encoder = Encoder::new(8);
    let mut seen = Vec::new();
    for (tick, life, class) in [(3., 1., 1), (9., 2., 2), (6., 1., 1), (12., 2., 1)] {
        // The tick's shared parts, then its own datagram with the correction.
        let snapshot = WireSnapshot { tick, epoch: 1., slots: vec![], world: vec![] };
        let parts = encoder.encode(&snapshot, None, 3., bounds.width()).unwrap().to_vec();
        let mut section = Writer::default();
        bfe_sim::wire::codec::own_correction_into(&mut section, 7., life, 11., &soldier(class), None).unwrap();
        let mut own = Writer::default();
        own_datagram_into(&mut own, OwnParts { epoch: 1., tick, interval: 3., baseline: 0., ack: 0., second: false, count: 1.,
            correction: Some(section.0.as_slice()), summaries: None, cosmetics: None }).unwrap();
        let mut result = Ok(());
        for part in &parts { result = result.and(receiver.receive_with_bounds(part, &bounds)); }
        let own_result = receiver.receive_with_bounds(&own.0, &bounds);
        seen.push((tick, result.and(own_result).is_ok(), receiver.header[h::RECORDS], receiver.header[h::ACK_TICK]));
    }
    // Published and acknowledged in turn; the late tick 6 publishes nothing
    // and leaves the acknowledgement at 9.
    assert_eq!(seen[..3], [(3., true, 1., 3.), (9., true, 1., 9.), (6., true, 0., 9.)]);
    // Tick 12 is refused: nothing of it published, the acknowledgement still 9.
    assert_eq!(seen[3], (12., false, 0., 9.));
}
