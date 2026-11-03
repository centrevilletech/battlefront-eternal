//! The generator's tests (tools/sim-schema.mjs) on its own schema
//! (rust/sim/schema-test/shelf.mjs, generated into `gen_test`): every type a
//! field can have goes through the sync rows and the replay values and
//! comes back the same. tests/sim-schema.test.js checks the JavaScript the
//! same schema makes, and the rows it writes for a map, word for word, are
//! the ones `a_map_s_words_are_its_entries` expects here.

use crate::gen_test::*;
use crate::store::block::Block;
use crate::store::Strings;

fn person(id: u32) -> Ref<Person> { Ref::new(id, 0) }

/// A shelf of two things that name each other, every type of field set.
fn shelf(strings: &mut Strings) -> Store {
    let (a, b) = (strings.intern_str("a"), strings.intern_str("b"));
    let mut s = Store::default();
    let spot = Spot { at: [1.0, -0.0, 3.0], turn: [0.0, 0.0, 0.7071067811865476, 0.7071067811865476], label: Some(b), mark: Colour::Green, near: Some(person(9)), kept: vec![] };
    let one = s.thing.insert(Thing {
        count: -3, size: u32::MAX, small: 255, light: 0.1f32, mass: -0.0, on: true, r#where: [1.5, 2.5, -3.5], facing: [0.0, 0.0, 0.0, 1.0],
        pair: [1e-300, 5e-324], name: a, colour: Colour::Blue, def: DefId(2), owner: Some(person(4)), holder: Holder::Person(person(9)),
        spot: Nullable::Value(spot.clone()), friends: RefSet { items: vec![person(9), person(4)] }, next: None,
        tags: Some(StrMap { entries: vec![(a, b), (b, a)] }), hidden: 1.25, boxed: 2.5,
        marks: vec![1.5, -0.0, 1e300], probes: vec![[1.0, 2.0, 3.0], [-4.0, 5.0, -6.0]], flags: vec![true, false, true],
        spots: vec![spot.clone(), Spot { mark: Colour::Red, label: None, ..spot.clone() }], holders: vec![Holder::Nobody, Holder::Person(person(4)), Holder::Null],
        matrix: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 5.0, -6.0, 7.5, 1.0], first_size: -2.5,
        shapes: vec![Shape { radius: 0.5, half: Half::F64(0.25) }, Shape { radius: -0.0, half: Half::Vec3([1.0, 2.0, 0.5]) }],
        hit_set: Some(1), body: Body::Wheels(Wheels { count: 4, grip: [0.1, 0.2, 0.3] }),
    });
    let two = s.thing.insert(Thing { holder: Holder::Thing(one), next: Some(one), spot: Nullable::Null, owner: None, name: b, tags: None, holders: vec![Holder::Thing(one)], body: Body::Legs(Legs { stride: 1.5, side: vec![1.0, -1.0] }), ..Default::default() });
    s.thing.get_mut(one).unwrap().next = Some(two);
    s.shelf = Shelf {
        things: vec![one, two],
        by_name: StrMap { entries: vec![(a, vec![two, one]), (b, vec![])] },
        weights: StrMap { entries: vec![(a, 1.5), (b, -0.0)] },
        spots: StrMap { entries: vec![(b, Spot { kept: vec![two], ..spot })] },
        owners: StrMap { entries: vec![(b, person(4))] },
        names: Some(StrMap { entries: vec![(a, b)] }),
    };
    s
}

fn through_rows(from: &mut Store, kind: KindCode, to: &mut Store) {
    let mut block = Block::default();
    assert!(from.export(kind, &mut block, 1));
    assert!(to.import(kind, &block, true));
}

#[test]
fn every_type_goes_through_the_rows_and_comes_back() {
    let mut strings = Strings::default();
    let mut a = shelf(&mut strings);
    let mut b = Store::default();
    through_rows(&mut a, KIND_THING, &mut b);
    through_rows(&mut a, KIND_SHELF, &mut b);
    assert_eq!(b.shelf, a.shelf);
    for &r in &a.shelf.things { assert_eq!(b.thing.get(r), a.thing.get(r)); }
    let one = b.thing.get(a.shelf.things[0]).unwrap();
    assert_eq!((one.mass.to_bits(), one.light, one.size), ((-0.0f64).to_bits(), 0.1f32, u32::MAX));
}

#[test]
fn a_map_s_words_are_its_entries() {
    let mut strings = Strings::default();
    let mut s = shelf(&mut strings);
    let mut block = Block::default();
    s.export(KIND_SHELF, &mut block, 1);
    let w = &block.words;
    let (heap, row) = (w[2] as usize, crate::store::block::HEADER);
    // The shelf's row: slot, generation, things, byName, weights, ...
    let (at, n) = (w[row + 6] as usize, w[row + 7] as usize);
    assert_eq!(n, 2);
    let entries = &w[heap + at..heap + at + 4];
    assert_eq!(entries.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), [0.0, 1.5, 1.0, -0.0].iter().map(|x: &f64| x.to_bits()).collect::<Vec<_>>());
    // byName's entries ("a": [two, one], "b": []): each its key and its
    // list's start and length; the lists after the entries.
    let (at, n) = (w[row + 4] as usize, w[row + 5] as usize);
    assert_eq!(n, 2);
    let e = &w[heap + at..heap + at + 6];
    assert_eq!((e[0], e[2], e[3], e[5]), (0.0, 2.0, 1.0, 0.0));
    let list = e[1] as usize;
    assert_eq!(&w[heap + list..heap + list + 4], &[1.0, 1.0, 0.0, 1.0]);
}

#[test]
fn a_retired_thing_is_exported_once_before_the_live_ones() {
    let mut strings = Strings::default();
    let mut s = shelf(&mut strings);
    let gone = s.shelf.things[1];
    assert!(s.retire(KIND_THING, gone.slot, gone.gen));
    s.shelf.things.pop();
    let mut block = Block::default();
    // The detector's export leaves it; the sync's hands it over.
    s.export(KIND_THING, &mut block, 2);
    assert_eq!((block.words[0], block.words[4]), (2.0, 1.0));
    s.export(KIND_THING, &mut block, 0);
    let stride = block.words[1] as usize;
    assert_eq!((block.words[0], block.words[4]), (2.0, 1.0));
    assert_eq!((block.words[5], block.words[6]), (gone.slot as f64, gone.gen as f64));
    assert_eq!(block.words[5 + stride], s.shelf.things[0].slot as f64);
    s.export(KIND_THING, &mut block, 0);
    assert_eq!((block.words[0], block.words[4]), (1.0, 0.0));
}

/// The test schema's command: its arguments back with `n` doubled, after
/// noting its spot (if any) as an event.
pub fn double(sim: &mut crate::abi::Sim, ask: Ask) -> Ask {
    if let Some(spot) = &ask.spot { spot.emit(&mut sim.events); }
    Ask { n: ask.n * 2.0, ..ask }
}

#[test]
fn a_command_takes_its_arguments_and_gives_its_result_through_the_call_block() {
    let sim = crate::abi::sim();
    let spot = Spot { at: [1.0, 2.0, 3.0], label: None, kept: vec![Ref::new(3, 1)], ..Default::default() };
    let ask = Ask { n: -0.75, who: Some(Ref::new(4, 0)), things: vec![Ref::new(1, 2), Ref::new(0, 1)], spot: Some(spot.clone()) };
    // Written as JavaScript writes arguments (the same words), then run.
    sim.calls.result(Ask::WORDS, |row, heap| ask.put(row, heap));
    sim.events.clear();
    assert!(run_command(sim, COMMAND_DOUBLE));
    let back = { let (row, heap) = sim.calls.args(); Ask::take(row, heap) };
    assert_eq!(back, Ask { n: -1.5, ..ask });
    // The event: its code, its length, the spot's words and its heap.
    let w = unsafe { std::slice::from_raw_parts(sim.events.ptr(), sim.events.len() + 1) };
    assert_eq!((w[1], w[2]), (EVENT_NOTED as f64, (Spot::WORDS + 2) as f64));
    assert_eq!(Spot::take(&w[3..3 + Spot::WORDS], &w[3 + Spot::WORDS..]), spot);
    assert!(!run_command(sim, 99));
    sim.events.clear();
}

#[cfg(feature = "replay")]
#[test]
fn every_type_goes_through_the_replay_values_and_comes_back() {
    use crate::store::replay::Arena;
    let mut first = Replay::default();
    first.store = shelf(&mut first.strings);
    let mut a = Arena::default();
    let value = first.write_value(KIND_SHELF, &mut a);
    let mut second = Replay::default();
    second.read_value(KIND_SHELF, a.view(&value)).unwrap();
    let mut b = Arena::default();
    let again = second.write_value(KIND_SHELF, &mut b);
    assert_eq!(sim_replay::diff(a.view(&value), b.view(&again)), None);
    // The two things still name each other, and the union holds the first.
    let things = &second.store.shelf.things;
    let (one, two) = (second.store.thing.get(things[0]).unwrap(), second.store.thing.get(things[1]).unwrap());
    assert_eq!((one.next, two.next, two.holder), (Some(things[1]), Some(things[0]), Holder::Thing(things[0])));
}
