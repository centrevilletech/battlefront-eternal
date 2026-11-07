//! Frozen semantic bytes exercise canonical store reads, including identities
//! reused between publications. The expected bytes came from the prior host.
use bfe_sim::{gen::*, store::{DefId, Ref, Strings}, wire::{codec, records,
    shots::Counters, store_capture::{Capture, Turret as TurretSample, World}}};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counted;
thread_local! { static ACTIVE: Cell<bool> = const { Cell::new(false) }; static ALLOCATIONS: Cell<usize> = const { Cell::new(0) }; }
fn allocation() { ACTIVE.with(|active| { if active.get() { ALLOCATIONS.with(|n| n.set(n.get() + 1)); } }); }
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 { allocation(); System.alloc(l) }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 { allocation(); System.alloc_zeroed(l) }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 { allocation(); System.realloc(p, l, n) }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { System.dealloc(p, l) }
}
#[global_allocator] static ALLOCATOR: Counted = Counted;

struct Readers { turret: TurretSample, remote: Option<(u32, Ref<ReconDroid>)> }
impl World for Readers {
    fn walk(&self, _: Ref<Vehicle>) -> (f64, f64) { (1.5, 0.5) }
    fn remote_count(&self) -> usize { 1 }
    fn remote(&self, _: usize) -> Option<(u32, Ref<ReconDroid>)> { self.remote }
    fn turret_count(&self) -> usize { 1 }
    fn turret(&self, _: usize) -> Option<TurretSample> { Some(self.turret) }
}
fn hex(value: &bfe_content::Value) -> Vec<u8> {
    let text = value.as_str().unwrap();
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
}
#[test]
fn canonical_entities_match_frozen_snapshots_and_capture_without_allocating() {
    let mut strings = Strings::default();
    let infantry = strings.intern_str("unit"); let vehicle_class = strings.intern_str("vehicle");
    let built = strings.intern_str("built");
    let setup = WireStoreSetup { classes: vec![WireStoreName { name: infantry, id: 1 }, WireStoreName { name: vehicle_class, id: 2 }],
        items: vec![WireStoreDefinition { definition: 7, id: 7 }], remotes: vec![WireStoreDefinition { definition: 8, id: 8 }],
        item_capacity: 3, remote_capacity: 1, entity_capacity: 256, record_capacity: 512, seat_capacity: 8, ..Default::default() };
    let mut capture = Capture::new(&setup, &strings).unwrap();
    let mut store = Store::default();
    let u = store.unit.insert_at(1, 1, Unit { class_name: infantry, generation: 1., team: 1.,
        soldier: UnitBody::Soldier(Soldier { position: [1., 2., 3.], velocity: [0., -0., 0.], aim_yaw: -0., aim_pitch: 0.3, ..Default::default() }),
        combat: Combat { health: 100., ..Default::default() }, ..Default::default() });
    store.units.list.push(Member::Unit(u));
    let v = store.vehicle.insert(Vehicle { name: vehicle_class, target_id: 10., target_generation: 2.,
        team: Some(2.), alive: true, position: [20., -10., 3.], velocity: [0., 0.2, 31.75], quaternion: [0., 0., 0., 1.],
        health: 500., power: Some(3.5), slot_index: 0., tilt: -0., roll: -0.,
        seats: vec![VehicleSeat { aim_yaw: 0.25, pitch: -0., ..Default::default() }], ..Default::default() });
    store.vehicles.vehicles.push(v);
    let first_item = store.item.insert(Item { def: DefId(7), velocity: [0.1, 0., 0.], up: [0., 1., 0.], yaw: 0.3, ..Default::default() });
    store.items.items.push(first_item);
    let d = store.recon_droid.insert(ReconDroid { class: 8, team: 2., position: [2., 3., 4.], health: 21.5, yaw: 0.2, pitch: -0.5, ..Default::default() });
    let turret_ref = Ref::new(19, 7);
    let mut readers = Readers { turret: TurretSample { reference: turret_ref, id: 20., life: 3., building: 0., state: built,
        team: 2., health: 80., yaw: 0.1, pitch: 0.2, gun_yaw: 0.3, gun_pitch: -0. }, remote: Some((1, d)) };
    let lease = store.seat_lease.insert(SeatLease { vehicle: Some(v), index: 0., ..Default::default() });
    let defs = bfe_sim::combat::defs::WeaponDefs::default(); let shots = Counters::default();
    let expected = bfe_content::json::parse(include_str!("../../../tests/fixtures/wire-store.json")).unwrap();
    let mut slots = records::SnapshotSlotTable::default(); let mut snapshots: Vec<WireSnapshot> = Vec::new();
    let mut retained = bfe_sim::wire::retained::Ring::new(8, 0, 128).unwrap();
    let mut retained_slots = records::SnapshotSlotTable::default();
    let mut encoder = bfe_sim::wire::buffered::Encoder::new(32);
    for tick in 1..=8 {
        match tick {
            2 => { store.unit.get_mut(u).unwrap().removed = true; store.vehicle.get_mut(v).unwrap().alive = false;
                store.item.get_mut(first_item).unwrap().gone = Some(true); store.recon_droid.get_mut(d).unwrap().dead = true; }
            3 => { let unit = store.unit.get_mut(u).unwrap(); unit.removed = false; unit.generation += 1.; unit.seat = Some(lease);
                let hull = store.vehicle.get_mut(v).unwrap(); hull.alive = true; hull.target_generation += 1.;
                // Reuse the physical item slot, with a new slab generation.
                store.item.remove(first_item); let new = store.item.insert(Item { def: DefId(7), velocity: [0.1, 0., 0.], up: [0., 1., 0.], yaw: 0.3, ..Default::default() });
                store.items.items[0] = new; readers.remote = Some((2, d)); store.recon_droid.get_mut(d).unwrap().dead = false; }
            4 => { let unit = store.unit.get_mut(u).unwrap(); unit.seat = None; unit.dead = true;
                if let UnitBody::Soldier(s) = &mut unit.soldier { s.air = Some(Air::Thrown); }
                let item = store.items.items[0]; store.item.get_mut(item).unwrap().position[0] += 0.000001; }
            5 => { readers.remote = None; let unit = store.unit.get_mut(u).unwrap(); unit.dead = false;
                if let UnitBody::Soldier(s) = &mut unit.soldier { s.air = None; }
                unit.seat = Some(lease); let seat = store.seat_lease.get_mut(lease).unwrap(); seat.vehicle = None; seat.turret = Some(turret_ref); }
            6 => { store.unit.get_mut(u).unwrap().removed = true; let hull = store.vehicle.get_mut(v).unwrap(); hull.alive = false; hull.dying = 0.231; }
            7 => { store.vehicle.get_mut(v).unwrap().dying = 0.; let item = store.items.items[0]; store.item.get_mut(item).unwrap().gone = Some(true); }
            8 => { let unit = store.unit.get_mut(u).unwrap(); unit.removed = false; unit.generation += 1.; unit.seat = None; }
            _ => {}
        }
        // A late cold admission may extend definition handles. It cannot
        // reset the protocol item slot that was reused at the previous tick.
        if tick == 4 { capture.rebind(&setup, &strings).unwrap(); }
        // The first capture is included: startup reserved the working set.
        ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
        let result = capture.capture(&store, &defs, &readers, &shots).and_then(|_| retained.capture(tick as f64, 1., capture.records(), std::iter::empty(),
            &mut retained_slots, &records::default_bounds(), &strings));
        ACTIVE.with(|flag| flag.set(false)); result.unwrap();
        assert_eq!(ALLOCATIONS.with(Cell::get), 0, "capture {tick}");
        let snapshot = records::snapshot_from_records(tick as f64, 1., capture.records(), vec![], &mut slots,
            &records::default_bounds(), snapshots.last(), &strings).unwrap();
        let case = &expected.as_array().unwrap()[tick - 1];
        for variant in case.get("variants").unwrap().as_array().unwrap() {
            let baseline = variant.get("baseline").unwrap().as_f64().unwrap();
            let baseline = snapshots.iter().find(|s| s.tick == baseline);
            let frozen: Vec<_> = variant.get("parts").unwrap().as_array().unwrap().iter().map(hex).collect();
            assert_eq!(codec::encode_entities(&snapshot, baseline, 2.).unwrap(), frozen, "snapshot {tick}");
            ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
            let encoded = encoder.encode(&snapshot, baseline, 2.);
            ACTIVE.with(|flag| flag.set(false));
            assert_eq!(encoded.unwrap(), frozen, "buffered snapshot {tick}");
            assert_eq!(ALLOCATIONS.with(Cell::get), 0, "buffered encode {tick}");
            let (current, base) = retained.pair(tick as f64, 1., baseline.map(|b| b.tick)).unwrap();
            ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
            let encoded = encoder.encode(current, base, 2.);
            ACTIVE.with(|flag| flag.set(false));
            assert_eq!(encoded.unwrap(), frozen, "retained snapshot {tick}");
            assert_eq!(ALLOCATIONS.with(Cell::get), 0, "retained encode {tick}");
        }
        snapshots.push(snapshot);
    }
    // Ordinary repeated publications neither grow nested rows nor re-admit
    // absent item lives. This does not measure the allocating legacy encoder.
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    for tick in 9..1209 {
        capture.capture(&store, &defs, &readers, &shots).unwrap();
        retained.capture(tick as f64, 1., capture.records(), std::iter::empty(), &mut retained_slots, &records::default_bounds(), &strings).unwrap();
    }
    ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

#[test]
fn capture_capacity_is_a_recoverable_result_without_allocator_growth() {
    let mut strings = Strings::default(); let class = strings.intern_str("unit"); let built = strings.intern_str("built");
    let setup = WireStoreSetup { classes: vec![WireStoreName { name: class, id: 1 }],
        entity_capacity: 512, record_capacity: 2048, seat_capacity: 1, remote_capacity: 1, ..Default::default() };
    let mut capture = Capture::new(&setup, &strings).unwrap();
    let mut store = Store::default();
    for id in 0..4096 {
        let r = store.unit.insert_at(id, 1, Unit { class_name: class, combat: Combat { health: 100., ..Default::default() }, ..Default::default() });
        store.units.list.push(Member::Unit(r));
    }
    let readers = Readers { turret: TurretSample { reference: Ref::new(0, 1), id: 5000., life: 1., building: 0., state: built,
        team: 0., health: 100., yaw: 0., pitch: 0., gun_yaw: 0., gun_pitch: 0. }, remote: None };
    let defs = bfe_sim::combat::defs::WeaponDefs::default(); let shots = Counters::default();
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    let result = capture.capture(&store, &defs, &readers, &shots);
    ACTIVE.with(|flag| flag.set(false));
    assert_eq!(result, Err("semantic fixed-roster capacity exceeded"));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    store.units.list.truncate(4);
    capture.capture(&store, &defs, &readers, &shots).unwrap();
    assert_eq!(capture.records().len(), 5);
}

#[test]
fn canonical_unit_fields_match_frozen_stagger_order_without_allocations() {
    let mut strings = Strings::default(); let empty = strings.intern_str("");
    let repair = strings.intern_str("repair:雪😀");
    let setup = WireStoreSetup { entity_capacity: 4, record_capacity: 8, ..Default::default() };
    let maps = Capture::new(&setup, &strings).unwrap();
    let mut store = Store::default();
    for id in 1..=3 {
        let character = store.character.insert(Character { roster_slot: Some((36 + id) as f64), ..Default::default() });
        let reference = store.unit.insert_at(id, 1, Unit { generation: 1., hero: id == 1,
            targetable: Some(id != 2), character: Some(character),
            combat: Combat { repair_lock: empty, blow_apart: id == 2, ..Default::default() }, ..Default::default() });
        store.units.list.push(Member::Unit(reference));
    }
    let expected = bfe_content::json::parse(include_str!("../../../tests/fixtures/wire-unit-fields.json")).unwrap();
    let mut fields = bfe_sim::wire::store_world::Units::default();
    let mut stagger = bfe_sim::wire::stagger::Stagger::new();
    let mut stage = bfe_sim::wire::stagger::Stagger::new();
    let defs = bfe_sim::combat::defs::WeaponDefs::default();
    for case in expected.as_array().unwrap() {
        let tick = case.get("tick").unwrap().as_f64().unwrap();
        for member in &store.units.list {
            let Member::Unit(reference) = *member else { continue };
            let unit = store.unit.get_mut(reference).unwrap();
            unit.generation = if tick >= 48. { 2. } else { 1. };
            unit.dead_time = if reference.slot == 2 { tick / 60. } else { 0. };
            if let UnitBody::Soldier(s) = &mut unit.soldier { s.state_time = (tick - 4.) / 60.; }
            unit.combat.repair_lock = if reference.slot == 1 && tick >= 30. && tick < 62. { repair } else { empty };
            unit.removed = reference.slot == 3 && tick >= 50. && tick < 70.;
        }
        ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
        stage.copy_native_from(&stagger).unwrap(); stage.begin();
        let result = fields.take(tick, &store, &strings, &defs, &maps, &mut stage, |r, _| r.slot != 3);
        stage.finish(&[]); std::mem::swap(&mut stage, &mut stagger);
        ACTIVE.with(|flag| flag.set(false)); result.unwrap();
        assert_eq!(ALLOCATIONS.with(Cell::get), 0, "unit fields {tick}");
        let actual = stagger.world(); let frozen = case.get("fields").unwrap().as_array().unwrap();
        assert_eq!(actual.len(), frozen.len(), "field count {tick}");
        for (actual, expected) in actual.iter().zip(frozen) {
            for (name, value) in [("id", actual.id), ("schema", actual.schema), ("generation", actual.generation)] {
                assert_eq!(value, expected.get(name).unwrap().as_f64().unwrap(), "{name} {tick}");
            }
            assert_eq!(actual.payload, hex(expected.get("payload").unwrap()), "unit payload {tick}");
        }
    }
    // Reject an invalid inventory before any definition or byte work. A
    // subsequent valid publication still owns all of its prepared buffers.
    let unit = store.unit.get_mut(Ref::new(1, 1)).unwrap();
    unit.combat.weapons.resize_with(13, Weapon::default);
    let committed = stagger.world();
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    stage.copy_native_from(&stagger).unwrap(); stage.begin();
    let result = fields.take(1000., &store, &strings, &defs, &maps, &mut stage, |_, _| true);
    ACTIVE.with(|flag| flag.set(false));
    assert_eq!(result, Err("Unit weapon count exceeds presentation limit"));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    let after = stagger.world();
    assert_eq!(after.len(), committed.len());
    for (a, b) in after.iter().zip(&committed) { assert_eq!(a.payload, b.payload); }
    store.unit.get_mut(Ref::new(1, 1)).unwrap().combat.weapons.clear();
    fields.take(1000., &store, &strings, &defs, &maps, &mut stagger, |_, _| true).unwrap();
}

struct WorldProps { tick: f64 }
impl bfe_sim::wire::store_world::Props for WorldProps {
    fn building_count(&self) -> usize { 2 }
    fn building(&self, index: usize) -> Option<bfe_sim::wire::store_world::Building> {
        use bfe_sim::wire::store_world::{Building, Station};
        Some(Building { turret: index == 1, state: if self.tick == 49. { 2 } else { 1 },
            health: if self.tick < 12. { f64::INFINITY } else { 50. }, team: if self.tick >= 49. { 2. } else { 1. },
            facing: -0., yaw: 1e-7, station: Some(Station { active_mode: self.tick >= 12., active: self.tick / 60.,
                pulse: self.tick / 100., served_health: self.tick >= 12., served_ammo: false }) })
    }
    fn animation_count(&self) -> usize { 1 }
    fn animation(&self, _: usize) -> Option<[f64; 7]> { Some([1., 3., 1.23445 + self.tick / 600., 91., 3., 0., 0.]) }
    fn trap_count(&self) -> usize { 1 }
    fn trap(&self, _: usize) -> Option<[f64; 3]> { Some([self.tick % 4., self.tick, self.tick % 2.]) }
}
#[test]
fn canonical_world_fields_match_frozen_json_order_and_recycle_without_allocating() {
    let mut strings = Strings::default();
    let names = ["unbuilt", "built", "destroyed", "waiting", "active"].map(|name| strings.intern_str(name));
    let leg = strings.intern(&['r' as u16, 'i' as u16, 'd' as u16, 'e' as u16, 'r' as u16, 0xd800, 0x96ea]);
    let mut store = Store::default();
    let a = store.vehicle.insert(Vehicle { alive: true, target_id: 31., target_generation: 4., tow_target: 1.,
        tow_local: [-0., 1e-7, 1000000000000000100.], tow_leg: leg, tow_angle: 1.234, tow_winding: -5., ..Default::default() });
    let b = store.vehicle.insert(Vehicle { target_id: 63., target_generation: 8., ..Default::default() });
    store.vehicles.vehicles.extend([a, b]);
    let expected = bfe_content::json::parse(include_str!("../../../tests/fixtures/wire-world-fields.json")).unwrap();
    let mut fields = bfe_sim::wire::store_world::WorldFields::default();
    let mut retained = bfe_sim::wire::retained::Ring::new(0, 16, 128).unwrap();
    let mut retained_slots = records::SnapshotSlotTable::default();
    let mut stagger = bfe_sim::wire::stagger::Stagger::new();
    for case in expected.as_array().unwrap() {
        let tick = case.get("tick").unwrap().as_f64().unwrap();
        let hull = store.vehicle.get_mut(a).unwrap(); hull.alive = tick != 49.; hull.target_generation = if tick >= 60. { 5. } else { 4. };
        ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
        stagger.begin();
        let result = fields.take(tick, 2., &store, &strings, &WorldProps { tick }, names, &mut stagger);
        stagger.finish(&[7., 8., 9., 10.]);
        let result = result.and_then(|_| retained.capture(tick, 2., &[], stagger.entries.iter().filter_map(|e| e.field.as_ref()),
            &mut retained_slots, &records::default_bounds(), &strings));
        ACTIVE.with(|flag| flag.set(false)); result.unwrap();
        assert_eq!(ALLOCATIONS.with(Cell::get), 0, "world fields {tick}");
        let actual = &retained.get(tick, 2.).unwrap().world; let frozen = case.get("fields").unwrap().as_array().unwrap();
        assert_eq!(actual.len(), frozen.len(), "world field count {tick}");
        for (actual, expected) in actual.iter().zip(frozen) {
            for (name, value) in [("id", actual.id), ("schema", actual.schema), ("generation", actual.generation)] {
                assert_eq!(value, expected.get(name).unwrap().as_f64().unwrap(), "{name} {tick}");
            }
            assert_eq!(actual.payload, hex(expected.get("payload").unwrap()), "world payload {tick} id {}", actual.id);
        }
    }
    // Repeated retire/rebirth must return native payload storage to its bank.
    // The old bounded-bank leak would exhaust it after a thousand toggles.
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    let mut result = Ok(());
    for step in 0..4096 {
        let tick = 200. + step as f64 * 60.; store.vehicle.get_mut(a).unwrap().alive = step % 2 == 0;
        stagger.begin(); result = fields.take(tick, 2., &store, &strings, &WorldProps { tick }, names, &mut stagger);
        stagger.finish(&[7., 8., 9., 10.]); if result.is_err() { break; }
        result = retained.capture(tick, 2., &[], stagger.entries.iter().filter_map(|e| e.field.as_ref()),
            &mut retained_slots, &records::default_bounds(), &strings); if result.is_err() { break; }
    }
    ACTIVE.with(|flag| flag.set(false)); result.unwrap();
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

#[test]
fn bounded_number_and_world_writers_reject_excess_without_allocator_growth() {
    let mut bytes = [0; 32]; let mut bits = 711982381u64;
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    for _ in 0..10000 {
        bits ^= bits << 13; bits ^= bits >> 7; bits ^= bits << 17;
        let number = f64::from_bits(bits);
        assert!(jsrt::numfmt::json_number_into(number, &mut bytes).is_some());
        assert!(jsrt::numfmt::fixed_four_number(number).is_some());
    }
    ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    let mut strings = Strings::default(); let long = strings.intern_str(&"雪".repeat(4096));
    let mut out = Vec::with_capacity(900);
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    let error = bfe_sim::wire::presentation::world_payload_into(&[4., 1., 3., long.0 as f64], &strings, &mut out);
    ACTIVE.with(|flag| flag.set(false));
    assert_eq!(error, Err(bfe_sim::wire::presentation::Error::WorldLength));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

struct SummaryLinks { first: Ref<Character>, second: Ref<Character> }
impl bfe_sim::wire::store_summaries::Links for SummaryLinks {
    fn get(&self, r: Ref<Character>) -> Option<bfe_sim::wire::store_summaries::Link> {
        use bfe_sim::wire::store_summaries::Link;
        if r == self.first { Some(Link { rtt: 37.5, has_kind: true, relay: true, has_family: true, family_six: true }) }
        else if r == self.second { Some(Link { rtt: f64::INFINITY, has_kind: true, relay: false, has_family: true, family_six: false }) }
        else { None }
    }
}
#[test]
fn canonical_common_fields_match_frozen_roster_and_statistics_chunks_without_allocating() {
    let mut strings = Strings::default();
    let a = strings.intern_str("classA"); let b = strings.intern_str("classB"); let vehicle = strings.intern_str("vehicle");
    let gun = strings.intern_str("gun"); let missing = strings.intern_str("missing-gun");
    let alpha = strings.intern_str("alpha"); let beta = strings.intern_str("beta");
    let mut store = Store::default();
    for i in 0..40 {
        let name = strings.intern_str(&if i == 0 { "fixture-λ🚀-long-雪雪雪雪雪".into() } else { format!("bot-{i}") });
        let r = store.character.insert_at(1000 + i * 3, 7, Character { name: Some(name), human: i < 2, team: (i % 2 + 1) as f64,
            kills: (i * 3000) as f64, team_kills: i as f64 - 1., deaths: i as f64, captures: (i % 3) as f64, ..Default::default() });
        store.battle.characters.push(r);
    }
    for (i, &reference) in store.battle.characters.iter().enumerate() {
        let mut slot = StatSlot { team_kills: i as f64, vehicles_destroyed: 300., spree: 300., headshots: 70000.,
            life_best: 12.3456 + i as f64, camp_best: 1.2345, ..Default::default() };
        for j in 0..40.min(i + 3) { slot.victims.set(store.battle.characters[j], (j + 1) as f64); }
        slot.victims.set(Ref::new(99999, 88), 255.);
        slot.classes = vec![TimeEntry { key: a, time: 1., ..Default::default() }, TimeEntry { key: b, time: 2., ..Default::default() }, TimeEntry { key: a, time: 2., ..Default::default() }];
        slot.weapons = vec![TimeEntry { key: missing, time: 3., ..Default::default() }];
        slot.vehicles = vec![TimeEntry { key: vehicle, time: 5., ..Default::default() }];
        let slots = if i % 2 == 0 { vec![slot, StatSlot::default()] } else { vec![StatSlot::default(), slot] };
        store.character.get_mut(reference).unwrap().record = bfe_sim::store::Nullable::Value(Record { slots, ..Default::default() });
    }
    for (team, value) in [(1., 140.), (2., 170.)] { store.battle.reinforcements.set(team, value); }
    for (team, value) in [(1., 150.), (2., 180.)] { store.battle.start.set(team, value); }
    for (team, value) in [(1., 1.), (2., 0.)] { store.battle.no_post.set(team, value); }
    for (team, value) in [(1., 0.3), (2., 0.6)] { store.battle.bleed.set(team, value); }
    store.battle.losing = true; store.battle.loser = Some(2.); store.battle.over_time = 1e-7;
    store.battle.victory_clock = bfe_sim::store::Nullable::Null; store.battle.countdown = bfe_sim::store::Nullable::Value(3.5);
    let mut post = Post { name: alpha, team: 1., toward: 2., progress: 0.3, neutralize: 0.4, contested: true, active: true, ..Default::default() };
    post.counts.set(2., 3.); post.counts.set(1., 4.);
    let post = store.post.insert(post); let other = store.post.insert(Post { name: beta, ..Default::default() });
    store.command_posts.posts.extend([post, other]);
    let setup = WireStoreSetup { entity_capacity: 256, record_capacity: 512,
        classes: vec![WireStoreName { name: a, id: 1 }, WireStoreName { name: b, id: 3 }, WireStoreName { name: vehicle, id: 5 }],
        weapon_names: vec![WireStoreName { name: gun, id: 2 }], posts: vec![WireStoreName { name: alpha, id: 1 }, WireStoreName { name: beta, id: 2 }], ..Default::default() };
    let maps = Capture::new(&setup, &strings).unwrap();
    let links = SummaryLinks { first: store.battle.characters[0], second: store.battle.characters[2] };
    let mut summaries = bfe_sim::wire::store_summaries::Summaries::default();
    let mut stagger = bfe_sim::wire::stagger::Stagger::new();
    let expected = bfe_content::json::parse(include_str!("../../../tests/fixtures/wire-common-fields.json")).unwrap();
    for case in expected.as_array().unwrap() {
        let tick = case.get("tick").unwrap().as_f64().unwrap();
        store.battle.game_over = tick >= 300.; store.battle.over = tick >= 300.; store.battle.finished = tick >= 360.;
        store.character.get_mut(links.first).unwrap().kills = tick; store.post.get_mut(post).unwrap().progress = tick / 400.;
        ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
        stagger.begin(); let result = summaries.take(tick, 2., &store, &strings, &maps, &links, &mut stagger);
        stagger.finish(&[7., 8., 9., 10.]); ACTIVE.with(|flag| flag.set(false)); result.unwrap();
        assert_eq!(ALLOCATIONS.with(Cell::get), 0, "common fields {tick}");
        let actual = stagger.world(); let frozen = case.get("fields").unwrap().as_array().unwrap();
        assert_eq!(actual.len(), frozen.len(), "common count {tick}");
        for (actual, expected) in actual.iter().zip(frozen) {
            for (name, value) in [("id", actual.id), ("schema", actual.schema), ("generation", actual.generation)] {
                assert_eq!(value, expected.get(name).unwrap().as_f64().unwrap(), "{name} {tick}");
            }
            assert_eq!(actual.payload, hex(expected.get("payload").unwrap()), "common payload {tick} id {}", actual.id);
        }
    }
}

#[test]
fn canonical_own_bodies_and_inventory_match_frozen_checkpoints_without_allocating() {
    use bfe_sim::{combat::defs::{Js, JsObj, WeaponClass, WeaponDefs}, wire::store_own};
    use std::rc::Rc;
    let mut defs = WeaponDefs::default();
    for (id, name, channel, rounds, ammo, spread) in [(0, "rifle", 0., 20., 3., 2.), (1, "grenade", 1., 1., 4., 0.)] {
        let mut fields = vec![("class".into(), Js::Str(name.into())), ("channel".into(), Js::Num(channel)),
            ("RoundsPerClip".into(), Js::Num(rounds)), ("ammo".into(), Js::Num(ammo)),
            ("SpreadThreshold".into(), Js::Num(spread)), ("base".into(), Js::Str(if id == 0 { "cannon" } else { "grenade" }.into()))];
        if id == 0 { fields.extend([("ZoomMin".into(), Js::Num(2.)), ("ZoomMax".into(), Js::Num(4.)), ("ShotDelay".into(), Js::Num(0.2))]); }
        defs.set(DefId(id), WeaponClass::new(Rc::new(JsObj { fields, def: None })));
    }
    let mut strings = Strings::default(); let name = strings.intern_str("trooper");
    let setup = WireStoreSetup { entity_capacity: 256, record_capacity: 512, classes: vec![WireStoreName { name, id: 1 }],
        weapons: vec![WireStoreDefinition { definition: 0, id: 1 }, WireStoreDefinition { definition: 1, id: 2 }], ..Default::default() };
    let maps = Capture::new(&setup, &strings).unwrap(); let mut capture = store_own::Capture::new(&defs);
    let frozen = bfe_content::json::parse(include_str!("../../../tests/fixtures/wire-own-fields.json")).unwrap();
    for case in frozen.as_array().unwrap() {
        let i = case.get("i").unwrap().as_f64().unwrap(); let n = i as usize;
        let soldier = Soldier { posture: Posture::from_u8((n % 3) as u8).unwrap(),
            air: if n % 5 == 0 { None } else { Air::from_u8((n % 5 - 1) as u8) }, grounded: n % 2 == 0,
            dive_from: Posture::Prone, fuel: 0.3, aim_yaw: -0., aim_pitch: 0.2, dive: if n % 3 == 0 { -1. } else { 0. },
            state_time: i / 60., position: [1., 2., 3.], velocity: [-0., 0.2, -3.], normal: [0., 1., 0.],
            settled: [if n % 2 == 0 { 1. } else { -0. }, 2., 3.], ..Default::default() };
        let body = if (8..22).contains(&n) { UnitBody::Droideka(Droideka { mode: (n % 14) as f64, gun: 0.,
            grounded: n % 2 == 0, reverse: n % 3 == 0, dead: n % 4 == 0, aim_yaw: -0., aim_pitch: 0.2, state_time: i / 60.,
            time: 0.3, legs: 0.4, spin: 0.5, roll: 0.6, lean: 0.7, blast_clock: 0.8, look_x: 0.9, look_y: 1.1,
            position: [1., 2., 3.], velocity: [-0., 0.2, -3.], avoidance: [0.1, 0.2, 0.3],
            shield: (n % 2 == 0).then_some(Shield { on: true, value: 33.3, max: 100., ..Default::default() }),
            last_gun: (n % 3 == 0).then_some(LastGun { index: 0., barrel: 1. }), ..Default::default() }) } else { UnitBody::Soldier(soldier) };
        let mut rifle = Weapon::create(DefId(0), &defs, true); rifle.heat = i / 100.; rifle.wait = if n % 2 == 0 { -0. } else { 0.12 };
        let mut grenade = Weapon::create(DefId(1), &defs, true); grenade.reserve = (n % 4) as f64;
        let unit = Unit { generation: 7., team: (n % 2 + 1) as f64, class_name: name, soldier: body, dead_time: i / 120., removed: n % 5 == 0,
            combat: Combat { weapons: vec![rifle, grenade], health: 97., dead: n % 4 == 0, death_time: i / 60.,
                kick: Kick { rate: 0.4, hold: 0.8, offset: -0. }, zoom_step: (n % 3) as f64, first_person: n % 2 == 0,
                gun_underwater: n % 3 == 0, ..Default::default() }, ..Default::default() };
        let facts = if n % 2 == 0 { store_own::Facts { discarded: i, ..Default::default() } }
            else { let mut held = [0; 16]; held[0] = 5; store_own::Facts { held, flags: 2047,
                view: [1.25, 16. / 9., 99., 1.2, 1.35], axes: [0.2, -0.3, 0.4, -0.], buttons: 1 << 11, discarded: i, ..Default::default() } };
        let mut v = Vehicle { target_id: 20., target_generation: 4., flying: n >= 24, position: [10., 20., 30.], velocity: [0.1, -0., 0.3],
            quaternion: [0., 0., 0., 1.], alive: true, sleeping: Some(false), health: 0.1, team: Some(2.), yaw: 0.3, tilt: 0.4, roll: 0.5,
            yaw_rate: 0.6, forward_input: 0.7, right_input: 0.8, tilt_rate: 0.9, roll_rate: 1., support: 1.1,
            altitude: 1.2, throttle: 1.3, airspeed: 1.4, state_time: 1.5, floor_y: f64::NAN, bank: 1.7, power: Some(1.8),
            stick_x: Some(1.9), stick_y: Some(2.), flight_roll_rate: Some(2.1), trick_cooldown: Some(2.2), trick_pending: Some(2.3),
            gear: Some(2.4), landing_message_time: Some(2.5), flight_clock: Some(2.6), flight_assist_id: Some(2.7), flight_lock_id: Some(2.8),
            flight_lock_generation: Some(2.9), flight_lock_timer: Some(3.), flight_lost_los: Some(3.1), trick_time: Some(3.2), trick_direction: Some(3.3),
            flight_start: Some([1., 2., 3.]), flight_end: Some([1., 2., 3.]), flight_trick_right: Some([1., 2., 3.]),
            flight_start_q: Some([0., 0., 0., 1.]), flight_trick_loop_q: Some([0., 0., 0., 1.]), flight_trick_end_q: Some([0., 0., 0., 1.]),
            roll_lock: Some(false), boosting: Some(false), boost_exhausted: Some(false), terrain_warning: Some(false),
            trick_held: Some(false), immelmann_held: Some(true), ..Default::default() };
        if n == 22 { v.floor_y = 0.; }
        let mounted_weapon = Weapon::create(DefId(0), &defs, true);
        let mut remote_rifle = Weapon::create(DefId(0), &defs, true); remote_rifle.heat = 0.2;
        let mut remote_grenade = Weapon::create(DefId(1), &defs, true); remote_grenade.wait = 0.3;
        let droid = ReconDroid { weapons: vec![remote_rifle, remote_grenade], yaw: 0.2, pitch: -0.3, health: 40., countdown: -1.,
            position: [1., 2., 3.], velocity: [0.1, 0.2, 0.3],
            salvo: (n >= 30).then_some(ReconSalvo { strike: 0, left: 2., wait: 0.4, at: [1., 2., 3.], x: [1., 0., 0.], y: [0., 1., 0.], z: [0., 0., 1.] }), ..Default::default() };
        let context = ReconContext { seed: 12345, look_x: 0.1, look_y: -0.2, zoom: n % 2 != 0, first_person: n % 2 != 0,
            camera: ReconCamera { position: [4., 5., 6.], rotation: [0., 0., 0., 1.], fov: 0.75, reach: 0.8, fresh: n % 2 == 0 }, ..Default::default() };
        ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
        capture.mounted.value.clear(); capture.remote.value.clear();
        let schema = if n == 22 { capture.ground(&v, [Some(0.2), Some(-0.), None, Some(0.1), Some(0.3), None]) }
            else if (24..28).contains(&n) { capture.flight(&v, [if n == 25 { 1 } else { 2 }, if n == 26 { 1 } else if n == 27 { 3 } else { 0 }, 1, 0, 5, 0],
                [Some(0.2), Some(-0.), None, Some(0.1), Some(0.3), None]) }
            else { capture.foot(&unit.soldier) };
        let result = schema.and_then(|schema| {
            if n == 23 { capture.mount(2, 20., 4., 1., &[0.27, 0.12, 0.15], 2, &[(&mounted_weapon, 17.)], &defs)?; }
            else if (22..28).contains(&n) { capture.mount(4, 20., 4., 0., &[], 0, &[], &defs)?; }
            if n >= 28 { capture.recon(&droid, &context, &defs, 1.25, (n >= 30).then_some((0, 0, 3.)))?; }
            capture.envelope(&unit, schema, if (22..28).contains(&n) && n != 23 { (20., 4.) } else { (0., 0.) }, 3., 42., &maps, &defs, &facts, (n >= 28).then_some((5., 6., 7.)))
        });
        ACTIVE.with(|flag| flag.set(false)); result.unwrap(); assert_eq!(ALLOCATIONS.with(Cell::get), 0, "own allocation {n}");
        assert_eq!(capture.body.value, hex(case.get("body").unwrap()), "body {n}");
        if let Some(value) = case.get("mounted").filter(|v| !matches!(v, bfe_content::Value::Null)) { assert_eq!(capture.mounted.value, hex(value), "mount {n}"); }
        if n >= 28 { assert_eq!(capture.remote.value, hex(case.get("remote").unwrap()), "remote {n}"); }
        assert_eq!(capture.payload.value, hex(case.get("payload").unwrap()), "envelope {n}");
        // The following valid capture retains all storage after a view fault.
        let mut invalid = facts; invalid.flags |= 32; invalid.view[0] = f64::NAN;
        ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
        let error = capture.envelope(&unit, 6, (0., 0.), 3., 42., &maps, &defs, &invalid, None);
        ACTIVE.with(|flag| flag.set(false)); assert_eq!(error, Err("invalid own-player number")); assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    }
}

#[test]
fn personal_spawn_and_record_fields_match_frozen_bytes_without_allocator_growth() {
    use bfe_sim::wire::{stagger::Stagger, store_summaries::{Summaries, Spawn}};
    let mut strings = Strings::default(); let alpha = strings.intern_str("alpha"); let beta = strings.intern_str("beta");
    let names: Vec<_> = (1..=64).map(|id| strings.intern_str(&format!("class-{id}"))).collect();
    let setup = WireStoreSetup { classes: names.iter().enumerate().map(|(id, &name)| WireStoreName { name, id: id as u32 + 1 }).collect(),
        posts: vec![WireStoreName { name: alpha, id: 1 }, WireStoreName { name: beta, id: 2 }], ..Default::default() };
    let maps = Capture::new(&setup, &strings).unwrap(); let mut store = Store::default();
    let a = store.character.insert_at(2, 1, Character::default());
    let player = store.character.insert_at(31, 9, Character { record: bfe_sim::store::Nullable::Value(Record { slots: vec![
        StatSlot { headshots: 7., deaths: 2., life_best: 12.3456, camp_best: 1.2345, ..Default::default() }, StatSlot::default()], ..Default::default() }), ..Default::default() });
    let b = store.character.insert_at(67, 8, Character::default()); store.battle.characters.extend([a, player, b]);
    let mut summaries = Summaries::default(); let mut stagger = Stagger::own();
    let all_classes: Vec<_> = (1..=64).map(|id| id as f64).collect();
    let frozen = bfe_content::json::parse(include_str!("../../../tests/fixtures/wire-own-summaries.json")).unwrap();
    for case in frozen.as_array().unwrap() {
        let i = case.get("i").unwrap().as_f64().unwrap() as usize;
        let connection = F4ConnectionView { phase: HumanPhase::from_u8((i % 4) as u8).unwrap(), class_name: Some(names[0]),
            post: (i % 2 != 0).then_some(alpha), spawned_once: i > 0, ended: i >= 4, dead_time: i as f64 / 60., ..Default::default() };
        ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
        let result = summaries.own(1., 2., &store, &maps, player, Spawn { connection: &connection,
            identity: if i == 0 { (0., 0.) } else { (20., 3.) }, team: (i % 2 + 1) as f64, slot: if i == 7 { 299 } else { 1 },
            pending: (i % 2 != 0).then_some((3., 2.)), auto: (i % 3 == 0).then_some((2., 4., 0.)),
            posts: if i >= 4 { &[] } else { &[1., 2.] }, classes: if i >= 4 { &[] } else { &all_classes },
        }, &mut stagger);
        ACTIVE.with(|flag| flag.set(false)); result.unwrap(); assert_eq!(ALLOCATIONS.with(Cell::get), 0, "personal allocation {i}");
        let actual = stagger.world(); let expected = case.get("fields").unwrap().as_array().unwrap();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            for (key, value) in [("id", actual.id), ("schema", actual.schema), ("generation", actual.generation)] {
                assert_eq!(value, expected.get(key).unwrap().as_f64().unwrap(), "personal {key} {i}");
            }
            assert_eq!(actual.payload, hex(expected.get("payload").unwrap()), "personal payload {i}");
        }
    }
}

#[test]
fn retained_ring_rejects_oversized_work_and_recovers_without_allocating() {
    let strings = Strings::default(); let mut ring = bfe_sim::wire::retained::Ring::new(1, 1, 128).unwrap();
    let mut slots = records::SnapshotSlotTable::default(); let bounds = records::default_bounds();
    let good = WireField { id: 7., schema: 8., generation: 2., payload: vec![1, 2, 3] };
    let bad = WireField { payload: vec![4; 901], ..Default::default() };
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    for tick in 1..=128 { ring.capture(tick as f64, 2., &[], std::iter::once(&good), &mut slots, &bounds, &strings).unwrap(); }
    let error = ring.capture(129., 2., &[], std::iter::once(&bad), &mut slots, &bounds, &strings);
    let excess = ring.capture(129., 2., &[], [&good, &good].into_iter(), &mut slots, &bounds, &strings);
    let result = ring.capture(129., 2., &[], std::iter::once(&good), &mut slots, &bounds, &strings);
    ACTIVE.with(|flag| flag.set(false)); result.unwrap(); assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    assert_eq!(error, Err("snapshot record exceeds datagram")); assert_eq!(excess, Err("snapshot world capacity exceeded"));
    assert!(ring.get(1., 2.).is_none()); assert!(ring.get(129., 1.).is_none());
    assert_eq!(ring.get(128., 2.).unwrap().world[0].payload, good.payload);
    assert_eq!(ring.get(129., 2.).unwrap().world[0].payload, good.payload);
}

#[test]
fn retained_own_baselines_match_frozen_packets_and_recycle_without_allocating() {
    use bfe_sim::wire::own_ring::Ring;
    let frozen = bfe_content::json::parse(include_str!("../../../tests/fixtures/wire-own-fields.json")).unwrap();
    let mut rings = [Ring::new(128).unwrap(), Ring::new(128).unwrap()];
    let mut correction = codec::Writer(Vec::with_capacity(2200));
    let mut summaries = codec::Writer(Vec::with_capacity(2048));
    let mut packet = codec::Writer(Vec::with_capacity(1100));
    for case in frozen.as_array().unwrap() {
        let i = case.get("i").unwrap().as_f64().unwrap(); let n = i as u8;
        let payload = hex(case.get("payload").unwrap());
        let mut fields = vec![WireField { id: 65536., schema: 3., generation: 3., payload: vec![n, 0, 1, 0, 2, 0, 3, 0, 4] }];
        if n % 4 != 0 { fields.push(WireField { id: 79999., schema: 5., generation: 3., payload: vec![1, n, 2, n, 3, n, 4, n, 5] }); }
        for (held, ring) in rings.iter_mut().enumerate() {
            let baseline = if held == 1 && n != 0 { 99. + i } else { 0. };
            let expected = hex(&case.get("ownPackets").unwrap().as_array().unwrap()[held]);
            ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
            let prepared = ring.write(100. + i, 3., baseline, Some(&payload), &fields).unwrap();
            let (own, base) = ring.prepared(&prepared);
            codec::own_correction_into(&mut correction, 1., 7., 11., own.payload.as_deref().unwrap(), base.and_then(|b| b.payload.as_deref())).unwrap();
            codec::world_section_into(&mut summaries, &own.summaries, base.map_or(&[], |b| &b.summaries)).unwrap();
            codec::own_datagram_into(&mut packet, codec::OwnParts { epoch: 3., tick: 100. + i, interval: 2., baseline,
                ack: i + 4., second: false, count: 1., correction: Some(&correction.0), summaries: Some(&summaries.0), cosmetics: Some(&[254, n]) }).unwrap();
            ACTIVE.with(|flag| flag.set(false));
            assert_eq!(ALLOCATIONS.with(Cell::get), 0, "own packet allocation {n}/{held}");
            assert_eq!(packet.0, expected, "own packet {n}/{held}");
        }
    }
    let payload = [9; 1100]; let oversize = [9; 1101];
    let mut ring = Ring::new(128).unwrap(); ring.reserve(1100).unwrap();
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    for tick in 1..4097 {
        let base = if tick > 128 { tick - 128 } else { 0 } as f64;
        let prepared = ring.write(tick as f64, 1., base, Some(&payload), &[]).unwrap();
        let (_, held) = ring.prepared(&prepared);
        assert_eq!(held.is_some(), tick > 128);
        assert!(!ring.has(tick as f64 - 128., 1.));
        assert!(ring.write(tick as f64 + 1., 1., tick as f64, Some(&oversize), &[]).is_err());
        // Spectators omit a correction; prepareOwn retains the baseline
        // body without adding stale personal fields to the new snapshot.
        ring.write(tick as f64, 1., tick as f64, None, &[]).unwrap();
        assert_eq!(ring.get(tick as f64, 1.).unwrap().payload.as_deref(), Some(payload.as_slice()));
    }
    assert!(codec::own_datagram_into(&mut packet, codec::OwnParts { epoch: 1., tick: 1., interval: 2., baseline: 0., ack: 0.,
        second: false, count: 1., correction: Some(&payload[..1077]), summaries: Some(&[]), cosmetics: None }).is_err());
    ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

#[test]
fn retained_cosmetic_delivery_preserves_order_replacements_and_acknowledgements_without_allocating() {
    use bfe_sim::wire::{cosmetics::{self, Event, Owner, View}, delivery};
    use std::rc::Rc;
    let mut expected = cosmetics::Outbox::new(300.); let mut actual = delivery::Outbox::new(300.);
    let mut serial = 1u64;
    for tick in 1..1600 {
        let events: Vec<_> = (0..if tick % 97 == 0 { 4096 } else { 7 }).map(|i| {
            serial += 1; let schema = [1., 2., 5., 6., 7., 8.][(serial as usize / 3) % 6];
            Rc::new(Event { identity: serial, schema, meta: cosmetics::cosmetic_metadata(schema,
                if i % 4 == 0 { (serial % 89) as f64 } else { serial as f64 }, tick as f64, tick as f64, 0.25),
                view: View { schema, position: Some([if i % 3 == 0 { 900. } else { i as f64 }, 0., 0.]),
                    owner: Owner::number(if i % 7 == 0 { 1. } else { 2. }), speed: 60., direction: Some([-1., 0., 0.]), life: 0.25 },
                bytes: vec![serial as u8; i % 111 + 19] })
        }).collect();
        actual.prepare(events.len()).unwrap();
        expected.offer(events.clone(), Some([0., 0., 0.]), Owner::number(1.));
        ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
        actual.offer(&events, Some([0., 0., 0.]), Owner::number(1.));
        ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0, "cosmetic offer {tick}");
        // Loads can produce events for many ticks without any publication.
        if tick % 137 < 36 { continue; }
        let full = tick % 7 != 0; let spare = if tick % 11 == 0 { 27. } else { 5000. };
        actual.prepare(0).unwrap();
        let packed = expected.pack(tick as f64, 255., 1077., full, spare).unwrap();
        ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
        actual.pack(tick as f64, 255., 1077., full, spare).unwrap();
        ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0, "cosmetic pack {tick}");
        assert_eq!(actual.bytes(), (packed.first.as_slice(), packed.second.as_slice()), "cosmetic bytes {tick}");
        assert_eq!(actual.inspect(), expected.inspect(), "cosmetic state {tick}");
        if tick % 9 == 0 {
            expected.acknowledge(tick as f64 - 2.);
            ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true)); actual.acknowledge(tick as f64 - 2.);
            ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0, "cosmetic ack {tick}");
        }
        if tick % 157 == 0 {
            expected.resend();
            ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true)); actual.resend();
            ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0, "cosmetic resync {tick}");
        }
        assert_eq!(actual.inspect(), expected.inspect(), "cosmetic settled state {tick}");
    }
}

#[test]
fn complete_native_publication_including_first_capture_and_fault_recovery_does_not_allocate() {
    use bfe_sim::wire::{publisher::{self, World}, Runtime, host::Host, journal};
    struct Source { strings: Strings, body: Vec<u8>, fields: Vec<WireField>, tick: f64, fail: bool, burst: usize }
    impl World for Source {
        fn capture(&mut self, tick: f64, epoch: f64, runtime: &mut Runtime) -> codec::Result<()> {
            runtime.capture_canonical(tick, epoch, &[], std::iter::empty(), &self.strings)
        }
        fn actor(&self, peer: u32) -> publisher::Actor {
            publisher::Actor { present: true, spectator: peer == 2, unit: (peer != 2).then_some((1., 7.)), ack: self.tick + 4., ack_tick: self.tick, ..Default::default() }
        }
        fn summaries(&mut self, _: u32, _: f64, out: &mut publisher::Fields) -> codec::Result<()> {
            out.clear(); for f in &self.fields { out.push(f)?; } Ok(())
        }
        fn own(&mut self, _: u32, _: f64) -> codec::Result<publisher::Correction<'_>> {
            if self.fail { return Err("invalid own-player number"); }
            Ok(publisher::Correction { id: 1., generation: 7., schema: 11., payload: &self.body })
        }
        fn events(&mut self, events: &mut journal::Events) {
            events.begin(); let mut payload = [0; 78]; payload[..4].copy_from_slice(&10000u32.to_le_bytes());
            for id in 0..self.burst { events.emit_raw(id as f64, 1., self.tick, 4., &payload, Default::default()); }
        }
        fn status(&self, id: bfe_sim::store::StrId) -> &[u16] { self.strings.units(id) }
    }
    let fixture = bfe_content::json::parse(include_str!("../../../tests/fixtures/wire-own-fields.json")).unwrap();
    let mut source = Source { strings: Strings::default(), body: hex(fixture.as_array().unwrap()[0].get("payload").unwrap()),
        fields: vec![WireField { id: 65536., schema: 3., generation: 3., payload: vec![0, 0, 1, 0, 2, 0, 3, 0, 4] }], tick: 1., fail: false, burst: 0 };
    let mut runtime = Runtime::default(); runtime.reserve_world(0, 0).unwrap();
    let mut host = Host::new(true, f64::INFINITY, f64::INFINITY, 3.);
    host.add(1, 0.); host.add(2, 0.);
    for id in 1..=2 {
        let c = host.client_mut(id); c.ready = true; c.started = true;
        c.publication = Some(publisher::Personal::new(id, std::ptr::null()).unwrap());
    }
    let mut edge_owner = Box::new(bfe_sim::wire::edges::Host::default());
    let status = source.strings.intern(&[0x03bb, 0xd83d, 0xde80, 0xd800]);
    edge_owner.epoch = 3.; edge_owner.generation = 7.; edge_owner.results.push(WeOutcome { id: 12., status, tick: 1. });
    host.client_mut(1).publication.as_mut().unwrap().edges = &*edge_owner;
    let mut publisher = publisher::Publisher::default(); publisher.admit(2);
    let frozen = codec::own_datagram(&WireOwn { epoch: 3., tick: 1., interval: 1., baseline_tick: 0., ack_input: 5., own_parts: 1.,
        correction: Some(WireField { id: 1., generation: 7., schema: 11., payload: source.body.clone() }), summaries: Some(source.fields.clone()), ..Default::default() }).unwrap();
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    unsafe { publisher.run(&mut host, &mut runtime, &mut source, 1., 0., 1000., 0.); }
    ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0, "first full publication");
    assert!(publisher.output.faults.is_empty()); assert_eq!(publisher.output.acks.len(), 2);
    let status = &publisher.output.edges[0].results[0].status;
    assert_eq!(&publisher.bytes[status.offset as usize..(status.offset + status.length) as usize], &[0xbb, 3, 0x3d, 0xd8, 0x80, 0xde, 0, 0xd8]);
    let span = &publisher.output.own[0].span;
    assert_eq!(&publisher.bytes[span.offset as usize..(span.offset + span.length) as usize], frozen);
    // Held and empty baselines, 128-entry eviction, own-only spectator rows,
    // invalid own input and following recovery all use the same native call.
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    for tick in 2..2002 {
        source.tick = tick as f64; source.fail = tick % 71 == 0;
        if tick % 3 == 0 { host.accept_ack(1, tick as f64 - 1., 3.); }
        if tick % 7 == 0 { host.accept_ack(2, tick as f64 - 1., 3.); }
        unsafe { publisher.run(&mut host, &mut runtime, &mut source, tick as f64, tick as f64 * 16., 1000., 0.); }
        assert_eq!(publisher.output.faults.len(), source.fail as usize);
        assert_eq!(publisher.output.own.len(), if source.fail { 1 } else { 2 });
    }
    ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0, "complete retained publication lifecycle");
    // A pause may drain more reliable bytes than the ordinary battle arena.
    // Every emitted reference must still point at its original record, and a
    // repeated batch must reuse the exceptional reservation without allocation.
    source.burst = 4096; source.fail = false;
    unsafe { publisher.run(&mut host, &mut runtime, &mut source, 2002., 32032., 1000., 0.); }
    assert!(publisher.output.faults.is_empty()); assert_eq!(publisher.output.records.len(), 4096);
    for row in &publisher.output.reliable { assert_eq!(row.records.len(), 4096); for &i in &row.records { assert_eq!(publisher.output.records[i as usize].id, i as f64); } }
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    unsafe { publisher.run(&mut host, &mut runtime, &mut source, 2003., 32048., 1000., 0.); }
    ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0, "retained paused reliable journal");
}

#[test]
fn canonical_dynamic_projectiles_match_frozen_samples_and_retirements_without_allocating() {
    use bfe_content::Value;
    use bfe_sim::wire::{journal::Events, store_projectiles};
    fn number(v: &Value) -> f64 { if v.as_str() == Some("-0") { -0. } else { v.as_f64().unwrap() } }
    fn n(row: &Value, field: &str) -> f64 { number(row.get(field).unwrap()) }
    fn vec<const N: usize>(row: &Value, field: &str) -> [f64; N] {
        let a = row.get(field).unwrap().as_array().unwrap(); std::array::from_fn(|i| number(&a[i]))
    }
    let frozen = bfe_content::json::parse(include_str!("../../../tests/fixtures/wire-dynamic-fields.json")).unwrap();
    let mut capture = store_projectiles::Capture::default(); let mut events = Events::default();
    for frame in frozen.as_array().unwrap() {
        let mut store = Store::default(); store.ordnance.grenades.clear(); store.ordnance.rounds.clear(); store.ordnance.beams.clear(); capture.facts.clear();
        for sample in frame.get("samples").unwrap().as_array().unwrap() {
            let slot = n(sample, "slot") as u32; let generation = n(sample, "generation") as u32; let gone = matches!(sample.get("gone"), Some(Value::Bool(true)));
            match n(sample, "type") as u8 {
                0 => { let r = store.grenade.insert_at(slot, generation, Grenade { position: vec(sample, "position"), velocity: vec(sample, "velocity"),
                    quaternion: vec(sample, "quaternion"), fuse: Some(n(sample, "life")), gone: Some(gone), ..Default::default() }); store.ordnance.grenades.push(r); }
                1 => { let r = store.round.insert_at(slot, generation, Round { position: vec(sample, "position"), velocity: vec(sample, "velocity"),
                    quaternion: vec(sample, "quaternion"), life: n(sample, "life"), gone: Some(gone), ..Default::default() }); store.ordnance.rounds.push(r); }
                2 => { let r = store.beam.insert_at(slot, generation, Beam { position: vec(sample, "position"), end: vec(sample, "end"), life: n(sample, "life"), ..Default::default() }); store.ordnance.beams.push(r); }
                _ => unreachable!(),
            }
        }
        for fact in frame.get("facts").unwrap().as_array().unwrap() {
            capture.facts.push(WireDynamicFact { slot: n(fact, "slot") as u32, ref_generation: n(fact, "refGeneration") as u32, r#type: n(fact, "type") as u8,
                id: n(fact, "id"), generation: n(fact, "generation"), launch_tick: n(fact, "launchTick"), definition_id: n(fact, "definitionId"),
                owner_id: n(fact, "ownerId"), owner_generation: n(fact, "ownerGeneration"), fire_id: n(fact, "fireId"), channel: n(fact, "channel"),
                terminal: matches!(fact.get("terminal"), Some(Value::Bool(true))) });
        }
        let tick = n(frame, "tick");
        ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true)); events.begin(); capture.take(tick, &store, &mut events);
        ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0, "dynamic capture {tick}");
        assert_eq!((capture.unknown, capture.malformed), (0, 0));
        let records = frame.get("records").unwrap().as_array().unwrap(); assert_eq!(events.cosmetics().len(), records.len());
        for (event, record) in events.cosmetics().iter().zip(records) {
            assert_eq!(event.schema, n(record, "schema")); assert_eq!(event.bytes[14], 0);
            assert_eq!(u32::from_le_bytes(event.bytes[..4].try_into().unwrap()) as f64, n(record, "id"));
            assert_eq!(&event.bytes[17..], hex(record.get("payload").unwrap()));
        }
        let retired: Vec<_> = frame.get("retired").unwrap().as_array().unwrap().iter().map(|v| number(v) as u32).collect();
        assert_eq!(capture.retired, retired, "retired {tick}");
    }
}

#[test]
fn publication_discovers_preexisting_projectile_lives_without_allocating() {
    use bfe_sim::wire::{journal::Events, store_projectiles::Capture};
    let mut store = Store::default();
    let r = store.grenade.insert_at(3, 2, Grenade { position: [1., -0., 4.], quaternion: [0., 0., 0., 1.], fuse: Some(2.), ..Default::default() });
    store.ordnance.grenades.push(r);
    let mut capture = Capture::default(); let mut events = Events::default(); let mut next_id = 41.;
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    events.begin();
    capture.take_with(17., &store, &mut events, |_| {
        let id = next_id; next_id += 1.;
        Ok(WireDynamicFact { id, generation: 9., definition_id: 7., launch_tick: 17., ..Default::default() })
    });
    assert_eq!(capture.created.len(), 1);
    assert_eq!((capture.created[0].slot, capture.created[0].ref_generation, capture.created[0].id), (3, 2, 41.));
    assert_eq!(events.cosmetics().len(), 1);
    events.begin(); capture.take_with(18., &store, &mut events, |_| panic!("same body was already observed"));
    assert!(capture.created.is_empty()); assert_eq!(events.cosmetics().len(), 1);
    store.ordnance.grenades.clear(); events.begin(); capture.take(19., &store, &mut events);
    assert_eq!(capture.retired.as_slice(), &[41]); assert_eq!(events.cosmetics()[0].schema, 7.);
    ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    assert_eq!(next_id, 42.);
}

#[test]
fn projectile_receipt_history_saturates_without_growing_or_panicking() {
    use bfe_sim::wire::{cosmetics::{self, Event, Owner, View}, delivery};
    use std::rc::Rc;
    let mut outbox = delivery::Outbox::new(300.);
    for tick in 0..1030 {
        // Longer than any authored flight: history cannot expire during this
        // synthetic overload, even though every delivery is acknowledged.
        let events: Vec<_> = (0..32).map(|i| {
            let id = (tick * 32 + i + 1) as f64;
            Rc::new(Event { identity: id as u64, schema: 1., meta: cosmetics::cosmetic_metadata(1., id, 1., 1., 100000.),
                view: View { schema: 1., position: Some([0.; 3]), owner: Owner::number(1.), life: 100000., ..Default::default() },
                bytes: vec![1; 19] })
        }).collect();
        outbox.prepare(events.len()).unwrap();
        ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
        outbox.offer(&events, Some([0.; 3]), Owner::number(1.));
        outbox.pack(tick as f64, 1100., 1100., true, 5000.).unwrap(); outbox.acknowledge(tick as f64);
        ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    }
    assert_eq!(outbox.history_evictions, 1030 * 32 - 32768);
}

#[test]
fn projectile_identity_updates_retain_order_and_retire_only_the_named_life() {
    use bfe_sim::wire::store_projectiles::Capture;
    let mut capture = Capture::default();
    let first = WireDynamicFact { id: 7., slot: 2, ref_generation: 3, definition_id: 9., ..Default::default() };
    let next = WireDynamicFact { id: 8., slot: 2, ref_generation: 4, definition_id: 11., ..Default::default() };
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    capture.begin_updates(); assert!(capture.update(first.clone())); assert!(capture.update(next.clone()));
    capture.begin_updates(); assert_eq!(capture.facts.len(), 2);
    assert!(capture.update(WireDynamicFact { terminal: true, ..first }));
    assert!(capture.facts[0].terminal); assert_eq!(capture.facts[1].id, 8.);
    capture.retired.push(7); capture.begin_updates();
    assert_eq!(capture.facts.len(), 1); assert_eq!(capture.facts[0].ref_generation, 4);
    for i in 0..4095 { assert!(capture.update(WireDynamicFact { id: 100. + i as f64, slot: 100 + i, ref_generation: 1, ..Default::default() })); }
    assert!(!capture.update(WireDynamicFact { id: 999999., slot: 20000, ref_generation: 1, ..Default::default() }));
    capture.retired.extend(capture.facts.iter().map(|f| f.id as u32)); capture.begin_updates(); assert!(capture.facts.is_empty());
    ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

#[test]
fn oversized_side_record_index_preserves_held_frames_and_recovers_without_allocating() {
    use bfe_sim::wire::retained::Ring;
    let strings = Strings::default(); let bounds = records::default_bounds();
    let body = WireRecord { kind: WireRecordKind::Soldier, id: 0., generation: 1., class_id: Some(1.),
        team: Some(1.), health: Some(100.), yaw: Some(0.), pitch: Some(0.), ..Default::default() };
    let valid = [body.clone()];
    let mut burst: Vec<_> = (1..=257).map(|id| WireRecord { kind: WireRecordKind::Seat, id: id as f64, ..Default::default() }).collect();
    burst.push(body);
    let mut slots = records::SnapshotSlotTable::default(); let mut ring = Ring::new(0, 0, 128).unwrap();
    let mut encoder = bfe_sim::wire::buffered::Encoder::new(0);
    ring.capture(1., 1., &valid, [].iter(), &mut slots, &bounds, &strings).unwrap();
    // More unrelated hints than a complete battle can use must reject this
    // snapshot, preserve its held baseline and release scratch for the retry.
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    let overflow = ring.capture(2., 1., &burst, [].iter(), &mut slots, &bounds, &strings);
    let retry = ring.capture(2., 1., &valid, [].iter(), &mut slots, &bounds, &strings);
    let (current, base) = ring.pair(2., 1., Some(1.)).unwrap();
    let encoded = encoder.encode(current, base, 1.);
    ACTIVE.with(|flag| flag.set(false));
    assert_eq!(overflow, Err("snapshot slot table full")); retry.unwrap();
    assert_eq!(encoded.unwrap().len(), 1); assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

#[test]
fn retained_snapshot_storage_grows_at_boundary_and_then_reuses_every_frame() {
    use bfe_sim::wire::retained::Ring;
    let strings = Strings::default(); let mut slots = records::SnapshotSlotTable::default();
    let fields: Vec<_> = (0..192).map(|id| WireField { id: id as f64, schema: 2., generation: 1., payload: vec![id as u8; 900] }).collect();
    let mut ring = Ring::new(0, fields.len(), 128).unwrap();
    let mut encoder = bfe_sim::wire::buffered::Encoder::new(fields.len());
    // This deliberately exceeds the measured 32 KiB frame reservation. The
    // first full retention cycle may grow at the publication boundary; every
    // subsequent capture and held-baseline read must reuse those banks.
    for tick in 1..=129 { ring.capture(tick as f64, 1., &[], fields.iter(), &mut slots, &records::default_bounds(), &strings).unwrap(); }
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|flag| flag.set(true));
    for tick in 130..=400 {
        ring.capture(tick as f64, 1., &[], fields.iter(), &mut slots, &records::default_bounds(), &strings).unwrap();
        let (current, base) = ring.pair(tick as f64, 1., Some((tick - 127) as f64)).unwrap();
        assert_eq!(current.world.len(), fields.len()); assert_eq!(base.unwrap().world[191].payload, fields[191].payload);
        // A protocol-sized source may exceed the prepared multipart bank.
        // The encoder returns a snapshot fault and remains usable for a delta.
        assert_eq!(encoder.encode(current, None, 1.), Err("snapshot needs too many parts"));
        assert_eq!(encoder.encode(current, base, 1.).unwrap().len(), 1);
    }
    ACTIVE.with(|flag| flag.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

#[test]
fn presentation_callbacks_borrow_fields_and_reuse_output_even_on_oversize() {
    use bfe_sim::wire::presentation::{event_payload_into, Error};
    let mut strings = Strings::default();
    let kind = strings.intern_str("kind").0 as f64;
    let blow = strings.intern_str("blowApart").0 as f64;
    let position = strings.intern_str("position").0 as f64;
    let name = strings.intern_str("name").0 as f64;
    let owner = strings.intern_str("ownerId").0 as f64;
    let lone = strings.intern(&[0xd800]).0 as f64;
    let long = strings.intern(&[0; 128]).0 as f64;
    // Independent JSON bytes cover a skipped vector hole, scalar signed zero,
    // a lone surrogate and the appended visual clamp for an absent amount.
    let words = vec![3., blow, f64::NEG_INFINITY, 4., kind, 3., blow,
        position, 4., 3., 1., 1., 7., 1., -0., name, 3., lone, owner, 1., 7.];
    let pressure = vec![3., blow, -50., 3., kind, 3., blow, name, 3., long, name, 3., long];
    let expected = br#"{"kind":"blowApart","position":[1,null,0],"name":"\ud800","ownerId":7,"amount":-50}"#;
    let mut out = Vec::with_capacity(2048);
    ALLOCATIONS.with(|n| n.set(0)); ACTIVE.with(|active| active.set(true));
    for _ in 0..3000 {
        assert_eq!(event_payload_into(&words, &strings, &mut out), Ok(()));
        assert_eq!(out.as_slice(), expected);
        assert_eq!(event_payload_into(&pressure, &strings, &mut out), Err((Error::EventLength, 0)));
    }
    ACTIVE.with(|active| active.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    assert_eq!(out.capacity(), 2048);
}
