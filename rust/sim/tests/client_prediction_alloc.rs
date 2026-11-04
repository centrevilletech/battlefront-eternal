#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
use bfe_sim::client::ClientPrediction;
use bfe_sim::collision::{State, Tree, World};
use bfe_sim::gen::{ClientCommand, ClientFrame, ClientResult, ClientMount, ClientAction, Droideka, Melee, Soldier, Weapon};
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
fn input(tick: f64, authority: f64) -> Vec<f64> { input_body(tick, authority, false) }
fn input_body(tick: f64, authority: f64, droid: bool) -> Vec<f64> {
    let mut words = vec![0.0; ClientFrame::WORDS];
    let soldier = Soldier { max_speed: 6.0, max_turn: 3.0, accel: 20.0,
        pitch_limits: [-1.0, 1.0], ..Soldier::default() };
    let actor_at = words.len() as u32;
    let actor_words = if droid { Droideka::WORDS } else { Soldier::WORDS };
    words.resize(words.len() + actor_words, 0.0);
    if droid {
        Droideka { max_turn: 3.0, speed: 6.0, ball_radius: 0.7, max_spin: 10.0,
            max_ball_slope: 0.5, pitch_limits: [-1.0, 1.0], ..Droideka::default() }
            .put(&mut words[actor_at as usize..], &mut Vec::new());
    } else { soldier.put(&mut words[actor_at as usize..], &mut Vec::new()); }
    // A compact, grounded stand checkpoint with all finite zero scalars.
    let correction_at = words.len() as u32;
    let correction_bytes: u32 = if droid { 170 } else { 124 };
    words.resize(words.len() + correction_bytes.div_ceil(4) as usize, 0.0);
    let weapons_at = words.len() as u32;
    words.push(1.0);
    let weapon_at = words.len();
    words.resize(weapon_at + Weapon::WORDS, 0.0);
    Weapon { clip: 1000.0, per_shot: 1.0, throw_time: -1.0, random: true,
        spread_scale: 1.0, sniper_scale: 1.0, ..Weapon::default() }.put(&mut words[weapon_at..], &mut Vec::new());
    words.extend([0.0; 4]);
    let commands_at = words.len() as u32;
    words.resize(words.len() + ClientCommand::WORDS, 0.0);
    ClientCommand { tick, generation: 1.0, sequence: tick, sent: true, ..ClientCommand::default() }
        .put(&mut words[commands_at as usize..], &mut Vec::new());
    ClientFrame { epoch: 1.0, identity: 7.0, generation: 1.0, source: 1.0, authority_tick: authority,
        ignored: -1.0, actor_at, actor_words: actor_words as u32,
        correction_at, correction_bytes, weapons_at, weapons_words: (Weapon::WORDS + 5) as u32,
        commands_at, commands_count: 1, mouse_scale: 1.0, ..ClientFrame::default() }
        .put(&mut words, &mut Vec::new());
    words
}

#[test]
fn replay_length_does_not_allocate_in_a_warm_infantry_session() {
    let mut collision = empty_collision();
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

fn empty_collision() -> State {
    let mut collision = State::default();
    collision.positions.push(bfe_sim::collision::Corners::F32(Vec::new()));
    collision.trees.push(Tree { roots: Vec::new(), positions: 0, index: Vec::new(),
        indexed: true, indirect: Vec::new(), polygons: Vec::new() });
    collision.sets.push(bfe_sim::collision::spheres::FixedSet { tree: 0, bounds: Vec::new(), polygons: Vec::new(), offset: 0.0 });
    collision.worlds.push(World { tree: 0, terrain: None, grid: None, order: Vec::new(),
        version: 0, statics: Some(0), broad: Default::default() });
    collision
}

#[test]
fn warm_droideka_and_firing_mounted_replay_do_not_allocate_per_tick() {
    let mut collision = empty_collision();
    let mut session = ClientPrediction::default();
    let mut setup = vec![0.0; 25]; setup[0] = 1.0;
    session.configure_actor(&setup, &mut collision).unwrap();
    let mut definition = vec![0.0, 7.0, 3.0];
    for (key, value) in [("base", "cannon"), ("class", "fixture")] {
        definition.push(key.len() as f64);
        definition.extend(key.encode_utf16().map(|v| v as f64));
        definition.extend([5.0, value.len() as f64]);
        definition.extend(value.encode_utf16().map(|v| v as f64));
    }
    definition.push(7.0); definition.extend("channel".encode_utf16().map(|v| v as f64));
    definition.extend([4.0, 0.0]);
    session.configure_weapon(&definition).unwrap();
    let short = input_body(1.0, 0.0, true); let long = input_body(120.0, 0.0, true);
    check_allocations("droideka", &mut session, &mut collision, &short, &long);
    session.configure_actor(&[0.0], &mut collision).unwrap();
    let short = mounted_input(1.0); let long = mounted_input(120.0);
    check_allocations("firing mounted", &mut session, &mut collision, &short, &long);
}
fn mounted_input(tick: f64) -> Vec<f64> {
    let mut words = input(tick, 0.0);
    let mut frame = ClientFrame::take(&words, &[]);
    let count = frame.weapons_at as usize + 1 + Weapon::WORDS + 3;
    words[count] = 1.0; words.insert(count + 1, 0.0);
    frame.commands_at += 1; frame.weapons_words += 1;
    ClientCommand { tick, generation: 1.0, sequence: tick, sent: true,
        held: bfe_sim::client::PRIMARY, actions: 1 << ClientAction::FirePrimary as u8, ..ClientCommand::default() }
        .put(&mut words[frame.commands_at as usize..], &mut Vec::new());
    frame.mode = 1; frame.mounts_at = words.len() as u32; frame.mounts_count = 1;
    words.resize(words.len() + ClientMount::WORDS, 0.0);
    ClientMount { advance: true, tow: false }.put(&mut words[frame.mounts_at as usize..], &mut Vec::new());
    frame.put(&mut words, &mut Vec::new()); words
}
fn check_allocations(name: &str, session: &mut ClientPrediction, collision: &mut State, short: &[f64], long: &[f64]) {
    for _ in 0..4 { session.reset(); session.predict_session(long, collision).unwrap(); }
    session.reset();
    let (status, one) = measured(|| session.predict_session(short, collision)); status.unwrap();
    session.reset();
    let (status, many) = measured(|| session.predict_session(long, collision)); status.unwrap();
    assert_eq!(ClientResult::take(&session.output, &[]).ticks, 120);
    assert_eq!(one, 0, "{name} warm frame"); assert_eq!(many, 0, "{name} warm replay ticks");
    println!("warm {name}: one tick {one} allocations, 120 ticks {many} allocations");
}

#[test]
fn warm_grounded_session_reuses_collision_and_replay_storage() {
    let mut collision = floor_collision();
    let mut session = ClientPrediction::default();
    session.configure_actor(&[0.0], &mut collision).unwrap();
    let mut definition = vec![0.0, 7.0, 3.0];
    for (key, value) in [("base", "cannon"), ("class", "fixture")] {
        definition.push(key.len() as f64); definition.extend(key.encode_utf16().map(|v| v as f64));
        definition.extend([5.0, value.len() as f64]); definition.extend(value.encode_utf16().map(|v| v as f64));
    }
    definition.push(7.0); definition.extend("channel".encode_utf16().map(|v| v as f64)); definition.extend([4.0, 0.0]);
    session.configure_weapon(&definition).unwrap();
    let short = input(1.0, 0.0); let long = input(120.0, 0.0);
    check_allocations("grounded infantry", &mut session, &mut collision, &short, &long);
    let result = ClientResult::take(&session.output, &[]);
    let body = Soldier::take(&session.output[result.actor_at as usize..], &[]);
    assert!(body.grounded, "the warm collision fixture must exercise real floor contacts");
}

fn floor_collision() -> State {
    let mut collision = empty_collision();
    let positions = vec![-100.0, 0.0, -100.0, 100.0, 0.0, -100.0, 100.0, 0.0, 100.0, -100.0, 0.0, 100.0];
    let index = vec![0, 2, 1, 0, 3, 2];
    let tree = bfe_content::bvh::build(bfe_content::bvh::Positions::F32(&positions), &index);
    collision.positions[0] = bfe_sim::collision::Corners::F32(positions);
    collision.trees[0] = Tree { roots: tree.roots, positions: 0, index, indexed: true,
        indirect: tree.indirect, polygons: vec![0, 0] };
    collision.sets[0].bounds = vec![-100.0,0.0,-100.0,100.0,0.0,100.0,-100.0,0.0,-100.0,100.0,0.0,100.0];
    collision.sets[0].polygons = vec![0, 0];
    collision
}

#[test]
fn warm_melee_replay_retains_swing_metadata() {
    let mut collision = floor_collision();
    let mut session = ClientPrediction::default();
    session.configure_actor(&[0.0], &mut collision).unwrap();
    let mut definition = vec![0.0, 7.0, 3.0];
    for (key, value) in [("base", "melee"), ("class", "fixture")] {
        definition.push(key.len() as f64); definition.extend(key.encode_utf16().map(|v| v as f64));
        definition.extend([5.0, value.len() as f64]); definition.extend(value.encode_utf16().map(|v| v as f64));
    }
    definition.push(7.0); definition.extend("channel".encode_utf16().map(|v| v as f64)); definition.extend([4.0, 0.0]);
    session.configure_weapon(&definition).unwrap();
    let short = melee_input(1.0); let long = melee_input(120.0);
    check_allocations("grounded melee", &mut session, &mut collision, &short, &long);
    let result = ClientResult::take(&session.output, &[]);
    let weapon = Weapon::take(&session.output[result.weapons_at as usize + 1..], &[]);
    assert!(weapon.melee.unwrap().time > 0.0, "the measured swing must have advanced");
}
fn melee_input(tick: f64) -> Vec<f64> {
    let mut words = input(tick, 0.0);
    let mut frame = ClientFrame::take(&words, &[]);
    let at = frame.weapons_at as usize + 1;
    let mut weapon = Weapon::take(&words[at..], &[]);
    weapon.melee = Some(Melee { time: -1.0, index: 0.0, active: false });
    weapon.put(&mut words[at..], &mut Vec::new());
    let at = frame.commands_at as usize;
    ClientCommand { tick: 1.0, generation: 1.0, sequence: 1.0, sent: true,
        held: bfe_sim::client::PRIMARY, actions: 1 << ClientAction::FirePrimary as u8, ..ClientCommand::default() }
        .put(&mut words[at..], &mut Vec::new());
    if tick > 1.0 {
        let at = words.len(); words.resize(at + ClientCommand::WORDS, 0.0);
        ClientCommand { tick, generation: 1.0, sequence: tick, sent: true,
            held: bfe_sim::client::PRIMARY, ..ClientCommand::default() }.put(&mut words[at..], &mut Vec::new());
        frame.commands_count = 2;
    }
    frame.put(&mut words, &mut Vec::new()); words
}
