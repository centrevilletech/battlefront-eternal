#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
use bfe_sim::client::ClientPrediction;
use bfe_sim::collision::{State, Tree, World};
use bfe_sim::gen::{ClientCommand, ClientFrame, ClientResult, Soldier, Weapon};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counter;
thread_local! { static COUNT: Cell<Option<usize>> = const { Cell::new(None) }; }
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|c| { if let Some(n) = c.get() { c.set(Some(n + 1)); } });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) { System.dealloc(p, layout); }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        COUNT.with(|c| { if let Some(n) = c.get() { c.set(Some(n + 1)); } });
        System.realloc(p, layout, size)
    }
}
#[global_allocator] static ALLOCATOR: Counter = Counter;
fn measured<T>(f: impl FnOnce() -> T) -> (T, usize) {
    COUNT.with(|c| c.set(Some(0)));
    let result = f();
    let allocations = COUNT.with(|c| c.replace(None).unwrap());
    (result, allocations)
}
fn input(tick: f64, authority: f64) -> Vec<f64> {
    let mut words = vec![0.0; ClientFrame::WORDS];
    let soldier = Soldier { max_speed: 6.0, max_turn: 3.0, accel: 20.0,
        pitch_limits: [-1.0, 1.0], ..Soldier::default() };
    let actor_at = words.len() as u32;
    words.resize(words.len() + Soldier::WORDS, 0.0);
    soldier.put(&mut words[actor_at as usize..], &mut Vec::new());
    // A compact, grounded stand checkpoint with all finite zero scalars.
    let correction_at = words.len() as u32;
    words.extend([0.0; 124]);
    let weapons_at = words.len() as u32;
    words.push(1.0);
    let weapon_at = words.len();
    words.resize(weapon_at + Weapon::WORDS, 0.0);
    Weapon::default().put(&mut words[weapon_at..], &mut Vec::new());
    words.extend([0.0; 4]);
    let commands_at = words.len() as u32;
    words.resize(words.len() + ClientCommand::WORDS, 0.0);
    ClientCommand { tick, generation: 1.0, sequence: tick, sent: true, ..ClientCommand::default() }
        .put(&mut words[commands_at as usize..], &mut Vec::new());
    ClientFrame { epoch: 1.0, identity: 7.0, generation: 1.0, source: 1.0, authority_tick: authority,
        ignored: -1.0, actor_at, actor_words: Soldier::WORDS as u32,
        correction_at, correction_bytes: 124, weapons_at, weapons_words: (Weapon::WORDS + 5) as u32,
        commands_at, commands_count: 1, mouse_scale: 1.0, ..ClientFrame::default() }
        .put(&mut words, &mut Vec::new());
    words
}

#[test]
fn replay_length_does_not_allocate_in_a_warm_infantry_session() {
    let mut collision = State::default();
    collision.positions.push(bfe_sim::collision::Corners::F32(Vec::new()));
    collision.trees.push(Tree { roots: Vec::new(), positions: 0, index: Vec::new(),
        indexed: true, indirect: Vec::new(), polygons: Vec::new() });
    collision.sets.push(bfe_sim::collision::spheres::FixedSet { tree: 0, bounds: Vec::new(), polygons: Vec::new(), offset: 0.0 });
    collision.worlds.push(World { tree: 0, terrain: None, grid: None, order: Vec::new(),
        version: 0, statics: Some(0), broad: Default::default() });
    let mut session = ClientPrediction::default();
    session.configure_actor(&[0.0], &mut collision).unwrap();
    // The same definition loader used by the page, with one ordinary weapon.
    let mut definition = vec![0.0, 7.0, 3.0];
    for (key, value) in [("base", "cannon"), ("class", "fixture")] {
        definition.push(key.len() as f64);
        definition.extend(key.encode_utf16().map(|v| v as f64));
        definition.extend([5.0, value.len() as f64]);
        definition.extend(value.encode_utf16().map(|v| v as f64));
    }
    definition.push(7.0);
    definition.extend("channel".encode_utf16().map(|v| v as f64));
    definition.extend([4.0, 0.0]);
    session.configure_weapon(&definition).unwrap();
    let short = input(1.0, 0.0); let long = input(120.0, 0.0);
    for _ in 0..4 { session.reset(); session.predict_session(&long, &mut collision).unwrap(); }
    session.reset();
    let (status, one) = measured(|| session.predict_session(&short, &mut collision)); status.unwrap();
    session.reset();
    let (status, many) = measured(|| session.predict_session(&long, &mut collision)); status.unwrap();
    let result = ClientResult::take(&session.output, &[]);
    assert_eq!(result.ticks, 120);
    assert_eq!(one, 0, "warm frame restore must retain its storage");
    assert_eq!(many, one, "allocations must belong to frame restore, not individual replay ticks");
    println!("warm empty-scene infantry: one tick {one} allocations, 120 ticks {many} allocations");
}
