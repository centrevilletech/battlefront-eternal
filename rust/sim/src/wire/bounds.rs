use bfe_content::Value;
use bfe_family::GameFamily;

use super::PositionBounds;

/// The banks a battle's publication reserves: working entity entries per
/// snapshot and world fields per snapshot. A 2004 battle keeps the released
/// reservations; a 2005 battle reserves the wire's whole slot domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capacity {
    pub slots: usize,
    pub fields: usize,
    /// Identities the canonical capture tracks, and its semantic rows.
    pub known: usize,
    pub rows: usize,
    /// Datagrams one shared snapshot may take. Every receiver accepts up to
    /// 256; a 2004 battle keeps the released 32. A snapshot is complete only
    /// when all of them arrived, so this is what the largest keyframe admission
    /// promises may need, far above what a battle's keyframes take.
    pub parts: usize,
}

impl Capacity {
    pub const LEGACY: Self = Self { slots: 256, fields: 192, known: 256, rows: 512, parts: 32 };
    pub const FULL: Self = Self { slots: super::records::SLOT_COUNT, fields: 1024, known: super::records::SLOT_COUNT, rows: 1024, parts: 256 };
    pub fn for_battle(family: GameFamily) -> Self {
        if family == GameFamily::Swbf1 { Self::LEGACY } else { Self::FULL }
    }
}

/// What a battle may field at once, by category, against what its protocol
/// carries. Admission refuses a battle whose totals do not fit, naming them,
/// rather than dropping entities or fields from snapshots later.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Census {
    /// Soldier bodies on the field at once: for every place on the rosters,
    /// the one that fills it and the one its last life fell in, which lies
    /// there until the next is due.
    pub bodies: usize,
    /// Vehicle hulls, placed and spawnable.
    pub hulls: usize,
    /// Items that can be on the ground at once.
    pub items: usize,
    /// Turrets, which hold their own entity slots.
    pub turrets: usize,
    /// Remote droids that can be out at once: one per body when the battle's
    /// weapons include one, none otherwise.
    pub remotes: usize,
    /// World fields the battle publishes at once.
    pub fields: usize,
    /// Ids its capture keeps for good once seen: every body, hull and target
    /// admitted, the hulls still to be born and a body for every place.
    pub identities: usize,
}

impl Census {
    pub fn check(&self, capacity: Capacity) -> Result<(), String> {
        let total = [self.hulls, self.items, self.turrets, self.remotes].into_iter()
            .try_fold(self.bodies, usize::checked_add).unwrap_or(usize::MAX);
        if total > capacity.slots || total > capacity.known {
            return Err(format!("the battle can field {total} entities ({} soldiers, {} vehicles, {} items, {} turrets, {} remote droids); \
                the protocol carries {}", self.bodies, self.hulls, self.items, self.turrets, self.remotes, capacity.slots.min(capacity.known)));
        }
        if self.fields > capacity.fields {
            return Err(format!("the battle publishes {} world fields; the protocol carries {}", self.fields, capacity.fields));
        }
        if self.identities > capacity.known {
            return Err(format!("the battle's bodies, hulls and targets take {} ids; the protocol keeps {}", self.identities, capacity.known));
        }
        Ok(())
    }
}

/// One world field producer of a battle: its schema, how many fields it can
/// publish at once and the largest payload one of them can have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Producer {
    pub schema: u16,
    pub count: usize,
    pub largest: usize,
}

/// The largest payload of a world presentation field written as a JSON array
/// of `numbers` numbers at their longest JSON form (24 bytes), `whole` whole
/// numbers of at most 5 digits, a text of each of `texts` bytes and `flags`
/// booleans.
pub const fn json_field_largest(numbers: usize, whole: usize, texts: &[usize], flags: usize) -> usize {
    let values = numbers + whole + texts.len() + flags;
    let (mut at, mut quoted) = (0, 0);
    while at < texts.len() {
        quoted += texts[at] + 2;
        at += 1;
    }
    2 + values.saturating_sub(1) + numbers * 24 + whole * 5 + quoted + flags * 5
}

/// The largest unit presentation field of a unit with `weapons` weapons: the
/// header, the longest repair identity (`l` or `v` and the index of a building
/// or a hull) and every weapon with all its numbers and its target.
pub const fn unit_field_largest(weapons: usize) -> usize {
    const HEADER: usize = 4 + 1 + 4 + 4 + 2 + 2 + 2 + 1 + 1;
    const REPAIR: usize = 1 + 10;
    const WEAPON: usize = 2 + 1 + 1 + 1 + 7 * 4 + 4 + 2;
    HEADER + REPAIR + weapons * WEAPON
}

/// The common summaries of a battle with `posts` command posts and `players`
/// players at their fullest: the battle's, each post's, the roster in chunks
/// of twelve, and the players' records. A record entry names both of a
/// player's team slots, his favourites and, in each slot, every player he has
/// beaten; whole entries fill chunks of at most 900 bytes, so the entries make
/// as many full chunks as their bytes come to, over as many chunks as it takes
/// when the fewest fit one.
pub fn summaries(posts: usize, players: usize) -> [Producer; 5] {
    const CHUNK: usize = 900;
    let entry = (3 + 2 * (17 + 3 * 6 + 1 + 3 * players.min(255))).min(CHUNK - 1);
    let chunks = players.div_ceil((CHUNK - 1) / entry);
    let full = (players * entry + chunks).div_ceil(CHUNK);
    [
        Producer { schema: 1, count: 1, largest: 1 + 7 * (1 + 4 * 8) + 4 + 1 + 8 + 9 + 9 },
        Producer { schema: 2, count: posts, largest: 2 + 1 + 1 + 8 + 8 + 1 + 1 + 1 + 7 * 3 },
        Producer { schema: 4, count: players.div_ceil(12), largest: 5 + 12 * (1 + 1 + 8 + 2 + 1 + 1 + 32) },
        Producer { schema: 5, count: full, largest: CHUNK },
        Producer { schema: 5, count: chunks - full, largest: 1 },
    ]
}

/// The bytes and parts of a battle's largest keyframe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keyframe {
    /// Encoded records, fields and part headers.
    pub bytes: usize,
    /// The largest indivisible group: a hull with its riders, or one record.
    pub largest: usize,
    /// The most parts the writer can need for it, in any order.
    pub parts: usize,
}

impl Census {
    /// The battle's largest keyframe as a snapshot: every entity at its
    /// largest record (each hull with `seats` seats and as many soldiers
    /// seated in it as it has seats) and every producer's every field at its
    /// largest payload. Record sizes do not depend on the values chosen: every
    /// field the writer writes for a record has a fixed width.
    pub fn largest_snapshot(&self, producers: &[Producer], seats: usize, bounds: &PositionBounds)
        -> Result<crate::gen::WireSnapshot, String>
    {
        use crate::gen::{WireField, WireRecord, WireRecordKind as Kind, WireRecordState, WireSeatAngles};
        let (min, extent) = bounds.padded_axes();
        let corner: [f64; 3] = std::array::from_fn(|a| min[a] + extent[a] * 0.999);
        let mut strings = crate::store::Strings::default();
        let built = strings.intern_str("built");
        let largest = |kind: Kind, id: f64| WireRecord { kind, id, generation: 65_535., class_id: Some(65_535.), team: Some(7.),
            building: 1., position: Some(corner), velocity: Some([-100., 100., -100.]), quaternion: Some([0., 0.6, 0., 0.8]),
            yaw: Some(0.5), pitch: Some(0.2), health: Some(65_535.), posture: 3., charge: Some(0.77), state: WireRecordState::F64(3.),
            weapon: 255., fire_counter: Some(255.), power: 65_535., mode: 3., reverse: true, shield_on: true, time: 99.5, roll: 1.1,
            lean: -0.9, legs: 0.8, shield: 0.66, gun_yaw: 0.5, gun_pitch: 0.2, active: true, definition_id: Some(65_535.),
            up: Some([0.2, 0.9, 0.1]), slot: Some(255.), tilt: 0.3, walk_phase: 0.7, walk_blend: 0.4, ..Default::default() };
        let mut records = Vec::new();
        let mut id = 0.;
        let mut next = || { id += 1.; id };
        let mut hulls = Vec::with_capacity(self.hulls);
        for _ in 0..self.hulls {
            let hull = next();
            hulls.push(hull);
            records.push(largest(Kind::Vehicle, hull));
            let angles = WireSeatAngles { yaw: 0.5, pitch: 0.2, ..Default::default() };
            records.push(WireRecord { kind: Kind::Mount, id: hull, seats: Some(vec![angles; seats.max(1)]), ..Default::default() });
        }
        let mut seated = hulls.iter().flat_map(|&hull| (0..seats).map(move |index| (hull, index)));
        for _ in 0..self.bodies {
            let body = next();
            // A droideka record is 22 bytes larger than a soldier's and uses
            // the same seat attachment, so it bounds every body slot.
            records.push(largest(Kind::Droideka, body));
            if let Some((hull, index)) = seated.next() {
                records.push(WireRecord { kind: Kind::Seat, id: body, vehicle_id: Some(hull), seat_index: Some(index as f64), ..Default::default() });
            }
        }
        for (kind, count) in [(Kind::Item, self.items), (Kind::Turret, self.turrets), (Kind::Remote, self.remotes)] {
            for _ in 0..count {
                let at = next();
                let state = if kind == Kind::Turret { WireRecordState::Str(built) } else { WireRecordState::F64(3.) };
                records.push(WireRecord { state, ..largest(kind, at) });
            }
        }
        let mut world = Vec::with_capacity(producers.iter().map(|p| p.count).sum());
        for (n, producer) in producers.iter().enumerate() {
            for at in 0..producer.count {
                world.push(WireField { id: (1_000_000 * (n + 1) + at) as f64, schema: f64::from(producer.schema), generation: 65_535.,
                    payload: (0..producer.largest).map(|i| (i * 131 + at) as u8 | 0x80).collect() });
            }
        }
        super::records::snapshot_from_records(3., 1., &records, world, &mut super::records::SnapshotSlotTable::default(), bounds, None, &strings)
            .map_err(|error| format!("the battle's largest keyframe cannot be built: {error}"))
    }

    /// The battle's largest keyframe, sized by the battle's own record and
    /// field writers: every entity at its largest record (each hull with
    /// `seats` seats and as many soldiers seated in it as it has seats) and
    /// every producer's every field at its largest payload. The writer fills
    /// parts in order and starts a new part only when the next group does not
    /// fit, so every part but the last holds more than its room less the
    /// largest group: that bounds the parts in any order of the records. A
    /// battle whose largest keyframe can need more parts than its snapshots
    /// may take is refused before it is admitted.
    pub fn check_keyframe(&self, producers: &[Producer], seats: usize, bounds: &PositionBounds, capacity: Capacity)
        -> Result<Keyframe, String>
    {
        use crate::gen::WireClass;
        use super::codec::{Writer, MAX_DATAGRAM};
        // The extended multipart header and the part's two counts.
        const OVERHEAD: usize = 24 + 4;
        let fields: usize = producers.iter().map(|p| p.count).sum();
        if fields > capacity.fields {
            return Err(format!("the battle publishes {fields} world fields; the protocol carries {}", capacity.fields));
        }
        let snapshot = self.largest_snapshot(producers, seats, bounds)?;
        // Each record's bytes as the writer frames it (its slot step, two
        // bytes at most), grouped as the writer groups riders with hulls.
        let width = bounds.width();
        let mut sizes = vec![0usize; snapshot.slots.iter().map(|r| r.slot as usize + 1).max().unwrap_or(0)];
        let mut writer = Writer::default();
        for row in &snapshot.slots {
            writer.0.clear();
            super::codec::record_into(&mut writer, &row.entry, None, width).map_err(|error| format!("the battle's largest keyframe: {error}"))?;
            sizes[row.slot as usize] = writer.0.len() + 2;
        }
        let mut groups: Vec<usize> = Vec::with_capacity(snapshot.slots.len() + fields);
        for row in &snapshot.slots {
            match row.entry.cls {
                WireClass::Vehicle => groups.push(sizes[row.slot as usize] + snapshot.slots.iter()
                    .filter(|r| r.entry.seat_slot == row.slot as f64 && matches!(r.entry.cls, WireClass::Soldier | WireClass::Droideka))
                    .map(|r| sizes[r.slot as usize]).sum::<usize>()),
                WireClass::Soldier | WireClass::Droideka if row.entry.seat_slot >= 0. => {}
                _ => groups.push(sizes[row.slot as usize]),
            }
        }
        for field in &snapshot.world {
            writer.0.clear();
            super::codec::field(&mut writer, field, false, None).map_err(|error| format!("the battle's largest keyframe: {error}"))?;
            groups.push(writer.0.len());
        }
        let total: usize = groups.iter().sum();
        let largest = groups.iter().copied().max().unwrap_or(0);
        let room = MAX_DATAGRAM - OVERHEAD;
        if largest > room { return Err(format!("one record group of the battle ({largest} bytes) does not fit a datagram")); }
        // A part closes only when the next group does not fit, so a closed
        // part holds at least its room less that group plus one byte, and the
        // groups that open parts are distinct: k closed parts need
        // k * (room + 1) less the k largest groups to be at most the total.
        groups.sort_unstable_by(|a, b| b.cmp(a));
        let (mut closed, mut opened) = (0usize, 0usize);
        while let Some(&next) = groups.get(closed) {
            if (closed + 1) * (room + 1) - (opened + next) > total { break; }
            opened += next;
            closed += 1;
        }
        let parts = closed + 1;
        let keyframe = Keyframe { bytes: total + parts * OVERHEAD, largest, parts };
        if parts > capacity.parts {
            return Err(format!("the battle's largest keyframe ({} entities, {fields} world fields, {} bytes) can need {parts} parts; \
                a snapshot takes at most {}", snapshot.slots.len(), keyframe.bytes, capacity.parts));
        }
        Ok(keyframe)
    }
}

/// Both authority and display use the original map metadata and the battle's
/// family. Mission overlays and network packets cannot override the box.
pub fn for_battle(family: GameFamily, map: &Value) -> Result<PositionBounds, &'static str> {
    if family == GameFamily::Swbf1 {
        return Ok(PositionBounds::LEGACY);
    }
    let extents = map.get("wireBounds").ok_or("map is missing wireBounds")?;
    let axis = |key| {
        let [x, y, z] = extents.get(key)?.as_array()? else {
            return None;
        };
        Some([x.as_f64()?, y.as_f64()?, z.as_f64()?])
    };
    axis("min")
        .zip(axis("max"))
        .and_then(|(min, max)| PositionBounds::from_map_extents(min, max))
        .ok_or("invalid map wireBounds")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_2004_battle_keeps_its_banks_and_a_crowded_census_is_refused() {
        assert_eq!(Capacity::for_battle(GameFamily::Swbf1), Capacity { slots: 256, fields: 192, known: 256, rows: 512, parts: 32 });
        assert_eq!(Capacity::for_battle(GameFamily::Swbf2).slots, 512);
        let fits = Census { bodies: 260, hulls: 120, items: 100, turrets: 12, remotes: 0, fields: 700, identities: 512 };
        assert!(fits.check(Capacity::FULL).is_ok());
        assert!(fits.check(Capacity::LEGACY).is_err());
        assert!(Census { fields: 1025, ..fits }.check(Capacity::FULL).is_err());
        assert!(Census { identities: 513, ..fits }.check(Capacity::FULL).unwrap_err().contains("513 ids"));
        assert!(Census { remotes: usize::MAX, ..fits }.check(Capacity::FULL).is_err());
        let error = Census { bodies: 400, hulls: 80, items: 60, ..Census::default() }.check(Capacity::FULL).unwrap_err();
        assert!(error.contains("540 entities") && error.contains("400 soldiers"), "{error}");
    }

    // The sizes admission counts for the common summaries, against what their
    // writers make of a battle whose every name, tally, list and clock is full.
    #[test]
    fn the_common_summaries_never_outgrow_what_admission_counts() {
        use super::super::{stagger::Stagger, store_capture::Capture, store_summaries::{Link, Links, Summaries}};
        use crate::gen::{Character, Post, Record, StatSlot, Store, TimeEntry, WireStoreName, WireStoreSetup};
        use crate::store::{Nullable, Ref, Strings};
        struct Linked;
        impl Links for Linked {
            fn get(&self, _: Ref<Character>) -> Option<Link> { Some(Link { rtt: 80., has_kind: true, relay: true, has_family: true, family_six: true }) }
        }
        for players in [1, 12, 13, 33, 64, 65, 137, 300] {
            const POSTS: usize = 9;
            let (mut strings, mut store) = (Strings::default(), Store::default());
            let [class, weapon] = ["class", "weapon"].map(|name| strings.intern_str(name));
            let long = strings.intern_str(&"é".repeat(32));
            let roster: Vec<_> = (0..players).map(|_| store.character.insert(Character { name: Some(long), team: 2., kills: 9e4, deaths: 9e4, ..Default::default() })).collect();
            let time = |key| vec![TimeEntry { key, label: None, time: 5000. }];
            let mut slot = StatSlot { team_kills: 900., vehicles_destroyed: 900., spree: 900., survivalist: 900., life_kills_best: 900., headshots: 9e4,
                deaths: 900., escapes: 900., life_best: 9e9, camp_best: 9e9, classes: time(class), weapons: time(weapon), vehicles: time(class), ..Default::default() };
            for &victim in &roster { slot.victims.set(victim, 900.); }
            for &player in &roster {
                store.character.get_mut(player).unwrap().record = Nullable::Value(Record { slots: vec![slot.clone(), slot.clone()], ..Default::default() });
            }
            store.battle.characters = roster;
            for team in 1..=7 {
                let team = f64::from(team);
                for table in [&mut store.battle.reinforcements, &mut store.battle.start, &mut store.battle.no_post, &mut store.battle.bleed] { table.set(team, 0.1); }
            }
            (store.battle.victory_clock, store.battle.countdown) = (Nullable::Value(0.1), Nullable::Value(0.1));
            let names: Vec<_> = (0..POSTS).map(|at| strings.intern_str(&format!("post {at}"))).collect();
            for &name in &names {
                let mut post = Post { name, ..Default::default() };
                for team in 1..=7 { post.counts.set(f64::from(team), 900.); }
                let post = store.post.insert(post);
                store.command_posts.posts.push(post);
            }
            let named = |names: &[crate::store::StrId]| names.iter().enumerate().map(|(at, &name)| WireStoreName { id: at as u32 + 1, name }).collect();
            let maps = Capture::new(&WireStoreSetup { classes: named(&[class]), weapon_names: named(&[weapon]), posts: named(&names), ..Default::default() }, &strings).unwrap();
            let (mut writer, mut stage) = (Summaries::default(), Stagger::with_capacity(1024));
            for tick in 0..400 {
                stage.begin();
                writer.take(f64::from(tick), 1., &store, &strings, &maps, &Linked, &mut stage).unwrap();
            }
            let written = stage.world();
            for schema in [1, 2, 4, 5] {
                let sizes: Vec<usize> = written.iter().filter(|f| f.schema == f64::from(schema)).map(|f| f.payload.len()).collect();
                let counted: Vec<Producer> = summaries(POSTS, players).into_iter().filter(|p| p.schema == schema).collect();
                let (count, bytes) = counted.iter().fold((0, 0), |(count, bytes), p| (count + p.count, bytes + p.count * p.largest));
                let largest = counted.iter().map(|p| p.largest).max().unwrap();
                assert!(sizes.len() <= count && sizes.iter().all(|&size| size <= largest) && sizes.iter().sum::<usize>() <= bytes,
                    "{players} players, schema {schema}: {sizes:?} of {count} fields, {largest} bytes at most, {bytes} together");
                // Nothing more is counted than they make, but the records of
                // a roster too long for one entry to fit a chunk.
                if schema != 5 || players <= 137 { assert_eq!(sizes.len(), count, "{players} players, schema {schema}"); }
                if schema != 5 && (schema != 4 || players >= 12) { assert_eq!(sizes.iter().max(), Some(&largest)); }
            }
        }
    }

    // The largest unit field and world presentation records, against their
    // writers with every number at its longest.
    #[test]
    fn unit_and_presentation_fields_reach_the_size_admission_counts() {
        use super::super::presentation::{unit_payload, world_payload_into, UNIT_HEADER, WEAPON_WORDS};
        let mut strings = crate::store::Strings::default();
        let repair = strings.intern_str("v4294967295");
        let long = -1.234_567_890_123_456_7e-300;
        for weapons in [0, 1, 4, 12] {
            let mut words = vec![9.; UNIT_HEADER + weapons * WEAPON_WORDS];
            (words[7], words[8]) = (f64::from(repair.0), weapons as f64);
            assert_eq!(unit_payload(&words, &strings).unwrap().len(), unit_field_largest(weapons));
        }
        let texts = ["destroyed", "waiting", "a leg of the longest name a hull's model may give it, 64 bytes.."];
        let [state, mode, leg] = texts.map(|text| (3., f64::from(strings.intern_str(text).0)));
        let (number, whole, flag) = ((1., long), (1., 65_535.), (2., 0.));
        let mut largest = |values: &[(f64, f64)]| {
            let mut words = vec![4., values.len() as f64];
            for &(kind, value) in values { words.extend([kind, value]); }
            let mut out = Vec::new();
            world_payload_into(&words, &strings, &mut out).unwrap();
            out.len()
        };
        let building = [whole, state, number, number, mode, number, number, number, number, flag, flag];
        assert_eq!(largest(&building), json_field_largest(6, 1, &[texts[0].len(), texts[1].len()], 2));
        assert_eq!(largest(&[whole, number, number, number, number, number, number, number]), json_field_largest(7, 1, &[], 0));
        assert_eq!(largest(&[whole, number, number, number]), json_field_largest(3, 1, &[], 0));
        assert_eq!(largest(&[number, number, number, number, number, number, leg, number, number]), json_field_largest(8, 0, &[texts[2].len()], 0));
    }

    #[test]
    fn map_bounds_use_the_family_and_pad_outwards() {
        let map = bfe_content::json::parse_bytes(
            br#"{"wireBounds":{"min":[-125.5,-5.1,70],"max":[-22.1,10,225.25]}}"#,
        )
        .unwrap();
        let legacy = for_battle(GameFamily::Swbf1, &map).unwrap();
        assert_eq!(legacy, PositionBounds::LEGACY);
        let bounds = for_battle(GameFamily::Swbf2, &map).unwrap();
        assert_eq!(bounds.decode([0.; 3]), [-190., -70., 6.]);
        assert_eq!(bounds.decode([65535.; 3]), [42., 74., 290.]);

        for (json, expected) in [
            (r#"{}"#, "map is missing wireBounds"),
            (r#"{"wireBounds":{"min":[0,0],"max":[0,1,1]}}"#, "invalid map wireBounds"),
            (r#"{"wireBounds":{"min":[0,0,0],"max":[0,1,1]}}"#, "invalid map wireBounds"),
            (r#"{"wireBounds":{"min":[0,0,0],"max":[1,1,65536]}}"#, "invalid map wireBounds"),
        ] {
            let map = bfe_content::json::parse_bytes(json.as_bytes()).unwrap();
            assert_eq!(for_battle(GameFamily::Swbf2, &map), Err(expected));
        }
    }

    // A battle of droidekas must fit inside what admission promises for its
    // body count: droideka records carry 22 more bytes each than soldiers.
    #[test]
    fn droideka_bodies_stay_within_the_admitted_keyframe_bound() {
        use crate::gen::{WireRecord, WireRecordKind as Kind, WireRecordState};
        let bounds = PositionBounds::from_map_extents([-7936.; 3], [7936.; 3]).unwrap();
        let census = Census { bodies: 64, identities: 64, ..Census::default() };
        let bound = census.check_keyframe(&[], 0, &bounds, Capacity::FULL).unwrap();
        let records: Vec<WireRecord> = (1..=64).map(|i| WireRecord {
            kind: Kind::Droideka, id: f64::from(i), generation: 65_535., class_id: Some(65_535.), team: Some(2.),
            position: Some([0., 0., 0.]), velocity: Some([-100., 100., -100.]), yaw: Some(0.5), pitch: Some(0.2),
            health: Some(65_535.), posture: 3., state: WireRecordState::F64(3.), weapon: 255., fire_counter: Some(255.),
            mode: 3., reverse: true, shield_on: true, time: 99.5, roll: 1.1, lean: -0.9, legs: 0.8, shield: 0.66,
            ..Default::default()
        }).collect();
        let snapshot = super::super::records::snapshot_from_records(
            3., 1., &records, vec![], &mut super::super::records::SnapshotSlotTable::default(), &bounds, None, &crate::store::Strings::default()
        ).unwrap();
        let mut encoder = super::super::buffered::Encoder::with_capacity(1024, Capacity::FULL);
        let parts = encoder.encode(&snapshot, None, 3., bounds.width()).unwrap();
        let bytes: usize = parts.iter().map(Vec::len).sum();
        assert!(bytes <= bound.bytes && parts.len() <= bound.parts,
            "droideka keyframe ({bytes} B, {} parts) exceeded admitted bound ({bound:?})", parts.len());
    }
}
