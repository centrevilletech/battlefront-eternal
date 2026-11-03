//! The store's state read from and written as the replay harness's values
//! (`cargo test -p sim --features replay`): what the store writes it reads
//! back the same, and writing that again gives the same values, every
//! number by its bits, every handle by its key, every entity met twice one
//! object.
#![cfg(feature = "replay")]

use bfe_sim::gen::*;
use bfe_sim::store::replay::{Arena, Value};

fn handle(kind: &str, key: f64) -> Value { Value::handle(kind, Value::Num(key)) }

// A store's ordnance with every kind of value its schema allows, made by
// reading handles in (as a capture names units, bodies and definitions).
fn store(cx: &mut Replay) {
    let a = Arena::default();
    let views: Vec<Value> = vec![handle("unit", 4.0), handle("body", 7.0), handle("vehicle", 2.0), handle("ownerSnapshot", 0.0), handle("emitter", 3.0)];
    let unit = cx.ref_in(KIND_UNIT, a.view(&views[0]), "u").unwrap();
    let body = cx.ref_in(KIND_BODY, a.view(&views[1]), "b").unwrap();
    let vehicle = cx.ref_in(KIND_VEHICLE, a.view(&views[2]), "v").unwrap();
    let snapshot = cx.ref_in(KIND_OWNER_SNAPSHOT, a.view(&views[3]), "s").unwrap();
    let emitter = cx.ref_in(KIND_EMITTER, a.view(&views[4]), "e").unwrap();
    let rifle = Value::handle("def", Value::str("rep_weap_inf_rifle_ord"));
    let rifle2 = Value::handle("def", Value::str("rep_weap_inf_rifle_ord#2"));
    let def = cx.def_in("ordnance", a.view(&rifle), "d").unwrap();
    let def2 = cx.def_in("ordnance", a.view(&rifle2), "d").unwrap();
    assert_eq!((unit.slot, def.0, def2.0), (4, 0, 1));
    let fighter = cx.strings.intern_str("fighter");
    let s = &mut cx.store;
    let b1 = s.bolt.insert(Bolt {
        def, owner: OrdnanceOwner::Unit(unit), network_fire_id: 17.0, ignore_body: None, position: [1.5, -0.0, 2.0],
        direction: [0.0, 0.0, 1.0], speed: 150.0, life: 0.8, offset: Some([0.1, 0.2, 0.3]), merge: 8.0,
        impact: Some(Impact { point: [3.0, 4.0, 5.0], normal: [0.0, 1.0, 0.0], terrain: true, body: Nullable::Value(body), unit: None, part: Nullable::Undefined }),
        previous_flown: Some(0.25), ..Default::default()
    });
    let b2 = s.bolt.insert(Bolt {
        def: def2, owner: OrdnanceOwner::Player, position: [f64::NAN, f64::INFINITY, f64::NEG_INFINITY], direction: [-0.0, 1e-300, 5e-324],
        life: -0.0, ignore_body: Some(body),
        impact: Some(Impact { point: [1.0, 2.0, 3.0], normal: [0.0, 0.0, -1.0], terrain: false, body: Nullable::Null, unit: Some(unit), part: Nullable::Value(HitPart::Head) }),
        ..Default::default()
    });
    let beam = s.beam.insert(Beam {
        history_tick: Some(1204.5), def, owner: OrdnanceOwner::Vehicle(vehicle), source: BeamSource::Emitter(emitter), local_origin: Some([0.0, 0.5, 2.0]),
        damage: Some(-0.0), ..Default::default()
    });
    let grenade = s.grenade.insert(Grenade {
        def, owner: OrdnanceOwner::OwnerSnapshot(snapshot), fuse: None, quaternion: [0.0, 0.7071067811865476, 0.0, 0.7071067811865476],
        sinking: Some(true), touched: None, gone: Some(false), ..Default::default()
    });
    let round = s.round.insert(Round {
        kind: RoundKind::Fatray, def, owner: OrdnanceOwner::Null, phase: [0.25, 6.0], struck: Some(RefSet { items: vec![unit, Ref::new(9, 0)] }),
        flight_class: Some(fighter), target_id: Some(-1.0), ..Default::default()
    });
    s.ordnance = Ordnance { bolts: vec![b1, b2], beams: vec![beam], grenades: vec![grenade], rounds: vec![round], start: [0.5, -0.0, 2.0] };
}

#[test]
fn the_store_reads_back_what_it_writes() {
    let mut first = Replay::default();
    store(&mut first);
    let mut a = Arena::default();
    let value = first.write_value(KIND_ORDNANCE, &mut a);

    // Read into another store (with handles and definitions met afresh)
    // and written again: the same values.
    let mut second = Replay::default();
    second.read_value(KIND_ORDNANCE, a.view(&value)).unwrap();
    let mut b = Arena::default();
    let again = second.write_value(KIND_ORDNANCE, &mut b);
    assert_eq!(sim_replay::diff(a.view(&value), b.view(&again)), None);

    // Field by field, the entities are the ones written (NaN aside, which
    // PartialEq never finds equal: its bits are checked by the diff above).
    let bolts = &second.store.ordnance.bolts;
    assert_eq!(bolts.len(), 2);
    let one = second.store.bolt.get(bolts[0]).unwrap();
    assert_eq!(one, first.store.bolt.get(first.store.ordnance.bolts[0]).unwrap());
    let two = second.store.bolt.get(bolts[1]).unwrap();
    assert!(two.position[0].is_nan() && two.life.to_bits() == (-0.0f64).to_bits());
    let round = second.store.round.get(second.store.ordnance.rounds[0]).unwrap();
    assert_eq!(second.strings.units(round.flight_class.unwrap()), "fighter".encode_utf16().collect::<Vec<_>>());
    assert_eq!(round.struck.as_ref().unwrap().items.iter().map(|r| r.slot).collect::<Vec<_>>(), vec![4, 9]);
}

#[test]
fn a_value_its_schema_does_not_allow_is_named_by_its_path() {
    let mut cx = Replay::default();
    store(&mut cx);
    let mut a = Arena::default();
    let value = cx.write_value(KIND_ORDNANCE, &mut a);
    // The first bolt's life made a string.
    let bolts = match &a.nodes[a.view(&value).get("bolts").node_id().unwrap() as usize] { sim_replay::Node::Array(items) => items.clone(), _ => unreachable!() };
    let bolt = bolts[0].clone();
    let id = match bolt { Value::Node(id) => id as usize, _ => unreachable!() };
    if let sim_replay::Node::Object { fields, .. } = &mut a.nodes[id] {
        fields.iter_mut().find(|(k, _)| k == "life").unwrap().1 = Value::str("soon");
    }
    let mut other = Replay::default();
    let error = other.read_value(KIND_ORDNANCE, a.view(&value)).unwrap_err();
    assert!(error.starts_with("ordnance.bolts[0].life: not a number"), "{error}");
}

// A battle's rounds as tools/sim-store-check.mjs exported them
// (--store-values FILE, in the replay harness's format): every sampled
// tick's state read into the store and written back is the same, field for
// field and handle for handle. Without BFE_SIM_STORE_VALUES there is nothing
// to read and the test says so.
#[test]
fn a_battle_s_rounds_come_back_from_the_store_as_they_went_in() {
    let Ok(path) = std::env::var("BFE_SIM_STORE_VALUES") else {
        eprintln!("BFE_SIM_STORE_VALUES names no file: not checked");
        return;
    };
    let bytes = std::fs::read(&path).expect("the values file");
    let mut reader = sim_replay::Reader::new(&bytes).expect("a capture");
    let mut cx = Replay::default();
    let (mut frames, mut rounds) = (0, 0);
    while let Some(record) = reader.next_record() {
        let record = record.expect("a record");
        if record.name != "ordnance" { continue; }
        cx.store = Store::default();
        cx.next_record();
        cx.read_value(KIND_ORDNANCE, record.root()).unwrap_or_else(|e| panic!("tick {}: {e}", record.tick));
        let mut a = Arena::default();
        let back = cx.write_value(KIND_ORDNANCE, &mut a);
        if let Some(d) = sim_replay::diff(record.root(), a.view(&back)) { panic!("tick {}: {d}", record.tick); }
        frames += 1;
        let o = &cx.store.ordnance;
        rounds += o.bolts.len() + o.beams.len() + o.grenades.len() + o.rounds.len();
    }
    eprintln!("{path}: {frames} ticks, {rounds} rounds read into the store and back");
    assert!(frames > 0, "no ordnance frames in {path}");
}
