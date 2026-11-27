//! Size shared packets at the 512-slot wire limit, independently of the live
//! capture bank's current 256-entry reservation.
mod crowded;
use bfe_sim::{
    gen::{WireClass, WireMount, WireRecord, WireRecordKind as Kind, WireRecordState, WireSeatAngles, WireSnapshot},
    store::Strings,
    wire::{buffered::Encoder, codec, records::{snapshot_from_records, SnapshotSlotTable, SLOT_COUNT}, PositionBounds},
};

fn mixed_snapshot(bounds: PositionBounds, tick: u32) -> WireSnapshot {
    let mut records = Vec::new();
    for slot in 0..SLOT_COUNT {
        let kind = match slot % 8 {
            0 | 1 => Kind::Vehicle,
            2..=5 => Kind::Soldier,
            6 => Kind::Item,
            _ => Kind::Remote,
        };
        let id = (slot + 1) as f64;
        let x = slot as f64 - 256. + f64::from(tick - 3) / 3.;
        records.push(WireRecord { kind, id, generation: 1., class_id: Some(1.), team: Some(1.),
            health: Some(100.), state: WireRecordState::F64(1.), active: true, yaw: Some(0.), pitch: Some(0.),
            position: Some([x, 10., 300.]), velocity: Some([20., 0., 0.]),
            quaternion: Some([0., 0., 0., 1.]), ..Default::default() });
        if kind == Kind::Vehicle {
            records.push(WireRecord { kind: Kind::Mount, id, seats: Some(vec![WireSeatAngles::default()]),
                ..Default::default() });
        } else if matches!(slot % 8, 2 | 3) {
            records.push(WireRecord { kind: Kind::Seat, id, vehicle_id: Some(id - 2.), ..Default::default() });
        }
    }
    snapshot_from_records(tick.into(), 1., &records, vec![], &mut SnapshotSlotTable::default(),
        &bounds, None, &Strings::default()).unwrap()
}

fn size(parts: &[Vec<u8>]) -> (usize, usize) {
    assert!(parts.iter().all(|part| part.len() <= codec::MAX_DATAGRAM));
    (parts.iter().map(Vec::len).sum(), parts.len())
}

#[test]
fn full_slot_position_budget() {
    let wide = PositionBounds::from_map_extents([-7936.; 3], [7936.; 3]).unwrap();
    let mut encoder = Encoder::new(0);
    for (bounds, expected_key, expected_delta) in [
        (wide, (23_305, 22), (6_322, 7)),
        (PositionBounds::LEGACY, (21_769, 22), (3_176, 4)),
    ] {
        let base = mixed_snapshot(bounds, 3);
        let moved = mixed_snapshot(bounds, 6);
        let key = size(encoder.encode(&base, None, 3., bounds.width()).unwrap());
        let delta = size(encoder.encode(&moved, Some(&base), 3., bounds.width()).unwrap());
        assert_eq!((key, delta), (expected_key, expected_delta));
        println!("{:?} mixed key {key:?}, one-metre delta {delta:?}", bounds.width());
    }

    // A legal entity mix can exceed the retained ceiling before world fields.
    let mut crowded = mixed_snapshot(wide, 3);
    let hull = crowded.slots.iter().find(|row| row.entry.cls == WireClass::Vehicle).unwrap().entry.clone();
    for row in &mut crowded.slots {
        let id = row.entry.entity_id;
        row.entry = hull.clone();
        row.entry.entity_id = id;
        row.entry.mount = WireMount { seats: vec![[0., 0.]; 16], ..Default::default() };
    }
    let required = size(&codec::encode_entities(&crowded, None, 3., wide.width()).unwrap());
    assert_eq!(required, (59_509, 57));
    assert_eq!(encoder.encode(&crowded, None, 3., wide.width()), Err("snapshot needs too many parts"));
    println!("wide key with 512 sixteen-seat hulls requires {required:?}");
}

// A crowded battle at the 2005 capacity from the generator built on the
// protocol's limits (tests/crowded): its keyframe and a delta in which every
// world field changes, at two field counts, and the most world fields one
// keyframe of that battle carries within a 2005 snapshot's 256 parts.
#[test]
fn a_crowded_battle_fits_the_retained_ceiling_up_to_its_field_limit() {
    let bounds = crowded::bounds();
    let (sizes, ceiling) = budget(crowded::MIX, bounds);
    println!("crowded battle {:?}: {sizes:?}; a keyframe carries at most {ceiling} world fields", bounds.width());
    assert_eq!(sizes, [(64, (13_262, 13), (6_949, 7)), (286, (30_876, 30), (24_539, 24))]);
    // A 2005 snapshot may take the 256 parts every receiver reads, so one
    // keyframe of this battle carries the whole 1,024-field domain; at the
    // released 32 parts it would carry at most 319 fields.
    assert_eq!(ceiling, 1024);
}

// The same budget for a census kept outside the tree: BFE_CENSUS names a file
// of seven whole numbers (hulls, crewed hulls, soldiers, seated soldiers,
// items, remotes, world fields). Without it there is nothing to check.
#[test]
fn a_private_census_fits_the_retained_ceiling() {
    let Ok(path) = std::env::var("BFE_CENSUS") else { return };
    let text = std::fs::read_to_string(path).unwrap();
    let n: Vec<u32> = text.split_whitespace().map(|word| word.parse().unwrap()).collect();
    let [hulls, crewed, soldiers, seated, items, remotes, fields] = n[..] else { panic!("seven numbers") };
    let mix = crowded::Mix { hulls, crewed, soldiers, seated, items, remotes };
    let (sizes, ceiling) = budget(mix, crowded::bounds());
    let fields = fields as usize;
    let base = crowded::snapshot_of(mix, crowded::bounds(), 1, fields, true);
    let moved = crowded::snapshot_of(mix, crowded::bounds(), 2, fields, true);
    let mut encoder = bfe_sim::wire::buffered::Encoder::with_capacity(512, bfe_sim::wire::bounds::Capacity::FULL);
    let key = size(encoder.encode(&base, None, 3., crowded::bounds().width()).unwrap());
    let delta = size(encoder.encode(&moved, Some(&base), 3., crowded::bounds().width()).unwrap());
    println!("census: {fields} fields key {key:?}, changing delta {delta:?}; generated sizes {sizes:?}; ceiling {ceiling} fields");
    assert!(fields <= ceiling);
}

// A mix's keyframe and changing delta at 64 and 286 world fields, and the
// most fields whose keyframe still fits.
fn budget(mix: crowded::Mix, bounds: PositionBounds) -> (Vec<(usize, (usize, usize), (usize, usize))>, usize) {
    use bfe_sim::wire::bounds::Capacity;
    let mut encoder = Encoder::with_capacity(512, Capacity::FULL);
    let mut sizes = Vec::new();
    for fields in [64, 286] {
        let base = crowded::snapshot_of(mix, bounds, 1, fields, true);
        let moved = crowded::snapshot_of(mix, bounds, 2, fields, true);
        let key = size(encoder.encode(&base, None, 3., bounds.width()).unwrap());
        let delta = size(encoder.encode(&moved, Some(&base), 3., bounds.width()).unwrap());
        sizes.push((fields, key, delta));
    }
    // The largest count whose keyframe fits: the encoder refuses every larger one.
    let fits = |encoder: &mut Encoder, fields: usize| encoder.encode(&crowded::snapshot_of(mix, bounds, 1, fields, false), None, 3., bounds.width()).is_ok();
    let (mut lo, mut hi) = (0, Capacity::FULL.fields + 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if fits(&mut encoder, mid) { lo = mid } else { hi = mid }
    }
    (sizes, lo)
}

// The admission check sizes a battle's largest keyframe with the writer's own
// record and field writers and bounds its parts in any order of the records.
// A count of the battle alone once admitted generated fields that the writer
// then could not encode. The same battle, each field family at its largest
// generated size, is admitted at 64 and 286 fields; a battle several times
// larger, whose worst keyframe passes what every receiver reads, is refused.
// Whenever a battle is admitted, the writer encodes the generated keyframe
// within the bound.
#[test]
fn admission_refuses_a_keyframe_the_writer_cannot_encode() {
    use bfe_sim::wire::bounds::{Capacity, Census, Producer};
    let bounds = crowded::bounds();
    let m = crowded::MIX;
    let census = |fields: usize| Census { bodies: m.soldiers as usize, hulls: m.hulls as usize, items: m.items as usize, turrets: 0,
        remotes: m.remotes as usize, fields, identities: 0 };
    let producers = |fields: usize| -> Vec<Producer> {
        let plan = crowded::field_plan(fields);
        let count = |schema: u16| plan.iter().filter(|(s, _, _)| *s == schema).count();
        vec![Producer { schema: 6, count: count(6), largest: 87 }, Producer { schema: 0x0203, count: count(0x0203), largest: 11 },
            Producer { schema: 7, count: count(7), largest: 83 }, Producer { schema: 4, count: count(4), largest: 159 }]
    };
    let mut encoder = Encoder::with_capacity(1024, Capacity::FULL);
    for fields in [64, 286] {
        let keyframe = census(fields).check_keyframe(&producers(fields), 4, &bounds, Capacity::FULL).unwrap();
        let written = size(encoder.encode(&crowded::snapshot_of(m, bounds, 1, fields, false), None, 3., bounds.width()).unwrap());
        println!("{fields} fields: largest keyframe {keyframe:?}; generated keyframe {written:?}");
        assert!(written.0 <= keyframe.bytes && written.1 <= keyframe.parts, "{written:?} {keyframe:?}");
    }
    // A battle several times this size still cannot be promised: at the
    // whole slot domain of soldiers and their remotes its worst keyframe
    // passes the 256 parts every receiver reads, and the writer refuses the
    // generated keyframe of the same census.
    let huge = Census { bodies: 512, hulls: 0, items: 0, turrets: 0, remotes: 0, fields: 1024, identities: 512 };
    let huge_producers = [Producer { schema: 6, count: 512, largest: bfe_sim::wire::bounds::unit_field_largest(12) },
        Producer { schema: 0x0203, count: 512, largest: 11 }];
    let refused = huge.check_keyframe(&huge_producers, 4, &bounds, Capacity::FULL).unwrap_err();
    assert!(refused.contains("a snapshot takes at most 256"), "{refused}");
    let generated = huge.largest_snapshot(&huge_producers, 4, &bounds).unwrap();
    assert_eq!(encoder.encode(&generated, None, 3., bounds.width()).err(), Some("snapshot needs too many parts"));
}

// An invented space battle, with each 2005 field at the largest size its
// draft gives (the producers do not write them yet): 58 soldiers with their
// unit and display fields, 30 two-seat fighters, 52 turret slots, 144 space
// objects, the animation clock and the guided ordnance. At these largest sizes
// admission promises the whole battle, all 512 ordnance slots included, within
// the parts every receiver reads, in any order of its records.
#[test]
fn an_invented_space_battle_counts_against_the_keyframe_budget() {
    use bfe_sim::wire::bounds::{unit_field_largest, Capacity, Census, Producer};
    let bounds = crowded::bounds();
    let census = Census { bodies: 58, hulls: 30, items: 0, turrets: 52, remotes: 0, fields: 0, identities: 140 };
    let producers = |ordnance: usize| [
        Producer { schema: 6, count: 58, largest: unit_field_largest(4) },
        Producer { schema: 0x0203, count: 58, largest: 11 },
        // Fighter: revision, flight, energy, two heats, bank, lock, target kind, target, life, progress.
        Producer { schema: 0x0211, count: 30, largest: 2 + 1 + 2 + 4 + 2 + 1 + 1 + 4 + 4 + 2 },
        // Space object: revision, team, flags, look, health, maximum, shield, maximum, recharge.
        Producer { schema: 0x0212, count: 144, largest: 2 + 3 + 5 * 4 },
        // Animation clock: revision, count, then 32 groups of state and origin.
        Producer { schema: 0x0213, count: 1, largest: 2 + 1 + 32 * 5 },
        // Guided ordnance: revision, kind, shooter and life, target kind, target and life, position code, velocity.
        Producer { schema: 0x0214, count: ordnance, largest: 2 + 1 + 8 + 1 + 8 + 8 + 6 },
    ];
    let snapshot = census.largest_snapshot(&producers(512), 2, &bounds).unwrap();
    let written = size(Encoder::with_capacity(1024, Capacity::FULL).encode(&snapshot, None, 3., bounds.width()).unwrap());
    let admitted = census.check_keyframe(&producers(512), 2, &bounds, Capacity::FULL).unwrap();
    let most = (0..=512).rev().find(|&n| census.check_keyframe(&producers(n), 2, &bounds, Capacity::FULL).is_ok()).unwrap();
    println!("invented space battle: writer {written:?} with 512 ordnance; admitted with {most} ordnance: {admitted:?}");
    assert!(written.1 <= admitted.parts && admitted.parts <= Capacity::FULL.parts);
    assert_eq!(most, 512);
}

// The crowded battle above as a scripted battle of 64 places: its 286 fields,
// the common summaries of a full roster and eight posts, and a mission at the
// most its records carry. Admission promises it, and the writer encodes it
// within the bound; the mission's own share is said beside it.
#[test]
fn a_mission_at_its_fullest_fits_beside_a_crowded_battle() {
    use bfe_sim::wire::{bounds::{summaries, Capacity, Census, Producer}, mission};
    let bounds = crowded::bounds();
    let m = crowded::MIX;
    let census = Census { bodies: m.soldiers as usize, hulls: m.hulls as usize, items: m.items as usize, turrets: 0, remotes: m.remotes as usize,
        fields: 0, identities: 300 };
    let plan = crowded::field_plan(286);
    let count = |schema: u16| plan.iter().filter(|(s, _, _)| *s == schema).count();
    let battle = [Producer { schema: 6, count: count(6), largest: 87 }, Producer { schema: 0x0203, count: count(0x0203), largest: 11 },
        Producer { schema: 7, count: count(7), largest: 83 }];
    let state = |count, largest| Producer { schema: mission::STATE_SCHEMA, count, largest };
    let records = [state(1, mission::MATCH_BYTES), state(1, mission::TIMER_BYTES), state(64usize.div_ceil(mission::PERSONAL_SLOTS), mission::PERSONAL_BYTES),
        state(64usize.div_ceil(mission::PERSONAL_SLOTS), mission::DEPLOYMENTS_BYTES),
        state(64usize.div_ceil(mission::PERSONAL_SLOTS), mission::CLASSES_BYTES),
        state(mission::OBJECTIVE_FIELDS, mission::OBJECTIVES_BYTES), Producer { schema: mission::SYSTEM_SCHEMA, count: mission::SYSTEMS, largest: mission::SYSTEM_BYTES }];
    let mut encoder = Encoder::with_capacity(1024, Capacity::FULL);
    let mut keyframe = |producers: &[Producer]| {
        let bound = census.check_keyframe(producers, 4, &bounds, Capacity::FULL).unwrap();
        let written = size(encoder.encode(&census.largest_snapshot(producers, 4, &bounds).unwrap(), None, 3., bounds.width()).unwrap());
        assert!(written.0 <= bound.bytes && written.1 <= bound.parts, "{written:?} {bound:?}");
        (written, bound.parts)
    };
    let unscripted = [&battle[..], &summaries(8, 64)].concat();
    let scripted = [&unscripted[..], &records].concat();
    let sizes = [keyframe(&unscripted), keyframe(&scripted)];
    println!("largest keyframes of the crowded battle without and with a mission, as written and bounded: {sizes:?}");
    assert_eq!(sizes, [((64_474, 68), 102), ((76_926, 81), 122)]);
}
