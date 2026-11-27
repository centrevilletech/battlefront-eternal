//! Deterministic fuzzing of every reader the 2005 records added: record
//! tables, the match descriptor and request, checkpoint parts, the soldier
//! body suffix, the display field, a class's movement-number words and the
//! mission's state, systems, notices and roster. Each reader sees tens of
//! thousands of generated inputs, most of them near valid ones. None may panic, and whatever one accepts must satisfy the
//! format's rules and, where the format allows, read back as written. A
//! record of an optional kind the client skips is skipped only at the kind
//! and revision the match advertises, on every carrier.
use bfe_presentation::{network::{RecordChecks, Session}, network_layout::net_header as h};
use bfe_sim::gen::{WireField, WireSnapshot};
use bfe_sim::movement::soldier::{swbf2::{body, words, ControlSpeed, Jetpack, RollDirection, Tuning}, Jet};
use bfe_sim::battle::script::state::CompactOutcome;
use bfe_sim::wire::{buffered::Encoder, checkpoint, codec::{own_datagram_into, OwnParts, Writer}, mission, PositionBounds,
    revisions::{self, Checks, Descriptor, Entry, Plan, Request, Supports, Table}};
use bfe_wide::Wide;
use serde_json::{json, Value};
use std::{cell::Cell, rc::Rc};

struct Rng(Wide);
impl Rng {
    fn new(seed: u32) -> Self { Self(Wide::from_u32(seed)) }
    fn next(&mut self) -> Wide {
        self.0 = self.0.xor(self.0.shl(13));
        self.0 = self.0.xor(self.0.shr(7));
        self.0 = self.0.xor(self.0.shl(17));
        self.0
    }
    fn below(&mut self, n: u32) -> u32 { self.next().rem(n) }
    fn chance(&mut self, one_in: u32) -> bool { self.below(one_in) == 0 }
    fn byte(&mut self) -> u8 { self.next().low() as u8 }
    // A number from a mix of ordinary, edge and special values.
    fn number(&mut self) -> f64 {
        match self.below(12) {
            0 => f64::NAN,
            1 => f64::INFINITY,
            2 => f64::NEG_INFINITY,
            3 => -0.0,
            4 => 0.0,
            5 => f64::MAX,
            6 => -1.0,
            7 => self.next().to_f64_bits(),
            _ => (self.below(2_000_000) as f64 - 1_000_000.0) / 1000.0,
        }
    }
}

fn tuning(r: &mut Rng) -> Tuning {
    let row = |r: &mut Rng| ControlSpeed { forward: r.below(30) as f64 / 10.0, strafe: r.below(30) as f64 / 10.0, turn: r.below(30) as f64 / 10.0 };
    let jetpack = match r.below(3) {
        0 => None,
        k => Some(Jetpack { physics: Jet { jump: 2.0, push: 5.0, recharge: 0.3, cost: 0.2, initial: 0.1, min_border: 0.2, hover: r.chance(2) },
            acceleration: 9.0, energy_cost: if k == 1 { 0.0 } else { r.below(20) as f64 }, force_jump: r.chance(2) }),
    };
    let capacity = 20.0 + r.below(100) as f64;
    Tuning { stand: row(r), crouch: row(r), sprint: row(r), jump: row(r), jet: row(r), jetpack, roll: row(r), tumble: row(r),
        sprint_accelerate_time: r.below(100) as f64 / 100.0, energy_capacity: capacity, energy_floor: -(r.below(30) as f64),
        energy_restore: 4.0, energy_restore_idle: 6.0, energy_drain_sprint: 9.0, energy_min_sprint: r.below(capacity as u32) as f64,
        energy_cost_jump: 3.0, energy_cost_jump_sprint: r.chance(2).then_some(5.0), energy_cost_roll: 11.0, jump_height: 1.2,
        jump_forward_speed_factor: 1.1, jump_strafe_speed_factor: 0.9, roll_speed_factor: 1.7, roll_frames: r.below(40) as u32,
        recover_from_tumble: r.chance(2), getup_front_seconds: 1.0 + r.below(10) as f64 / 10.0, getup_back_seconds: 1.0 }
}

fn mutate(r: &mut Rng, bytes: &mut Vec<u8>) {
    for _ in 0..1 + r.below(3) {
        match r.below(5) {
            0 if !bytes.is_empty() => { let at = r.below(bytes.len() as u32) as usize; bytes[at] ^= 1 << r.below(8); }
            1 if !bytes.is_empty() => { let at = r.below(bytes.len() as u32) as usize; bytes[at] = r.byte(); }
            2 => bytes.push(r.byte()),
            3 if !bytes.is_empty() => { bytes.pop(); }
            _ => { let n = bytes.len(); bytes.truncate(r.below(n as u32 + 1) as usize); }
        }
    }
}

#[test]
fn soldier_suffix_reader() {
    let mut r = Rng::new(0x5eed_0001);
    let (mut accepted, mut cases) = (0, 0);
    for _ in 0..50_000 {
        let t = tuning(&mut r);
        let b = body::Body { grounded: r.chance(4) || r.chance(2), air: if r.chance(3) { r.below(5) as u8 } else { 0 },
            posture: r.below(4) as u8, lock: if r.chance(3) { r.number() } else { 0.0 }, diving: r.chance(8) };
        // A suffix near a valid one: energy, moves, the timers its moves name,
        // the landing lock and jump count, and the parts its flags name.
        let mut bytes = Vec::new();
        let energy = if r.chance(3) { r.number() } else { t.energy_floor + (t.energy_capacity - t.energy_floor) * r.below(1001) as f64 / 1000.0 };
        bytes.extend(energy.to_le_bytes());
        let moves = if r.chance(4) { r.byte() } else { [0, 1, 2, 4, 6, 0x20, 0x22, 0x21, 0x40, 0x80, 0xc3][r.below(11) as usize] };
        bytes.push(moves);
        let small = |r: &mut Rng| if r.chance(3) { r.number() } else { r.below(100) as f64 / 100.0 };
        if moves & 1 != 0 { bytes.extend(small(&mut r).to_le_bytes()); }
        if moves & 6 != 0 { bytes.extend(small(&mut r).to_le_bytes()); }
        bytes.extend(small(&mut r).to_le_bytes());
        bytes.extend((r.next().low()).to_le_bytes());
        if moves & 0x80 != 0 { bytes.extend(small(&mut r).to_le_bytes()); }
        if moves & 0x40 != 0 {
            bytes.extend([if r.chance(4) { r.byte() } else { 1 + r.below(5) as u8 }, if r.chance(4) { r.byte() } else { r.below(8) as u8 },
                if r.chance(4) { r.byte() } else { r.below(4) as u8 }]);
            for _ in 0..6 { bytes.extend(small(&mut r).to_le_bytes()); }
        }
        if r.chance(4) { mutate(&mut r, &mut bytes); }
        cases += 1;
        let Ok(state) = body::read_suffix(&bytes, &t, b) else { continue };
        accepted += 1;
        // What the reader accepts obeys every rule of the format ...
        assert!(state.energy.is_finite() && (t.energy_floor..=t.energy_capacity).contains(&state.energy));
        assert!(b.posture != 2 && !b.diving);
        if let Some(time) = state.sprint_time { assert!((0.0..=t.sprint_accelerate_time).contains(&time)); }
        if let Some(roll) = state.roll { assert!(t.roll_frames > 0 && (0.0..=t.roll_seconds()).contains(&roll.elapsed)); }
        assert!(state.land_cooldown.is_finite() && state.land_cooldown >= 0.0);
        assert!(state.unsupported_time.is_none_or(|time| time.is_finite() && time >= 0.0));
        if let Some(tumble) = state.tumble { assert!(tumble.fly_dir <= 3 && tumble.elapsed >= 0.0 && tumble.air_time >= 0.0 && tumble.angle.is_finite()); }
        // ... and the writer gives back the very bytes.
        let (out, length) = body::suffix(&state).unwrap();
        assert_eq!(&out[..length], &bytes[..], "{bytes:?}");
    }
    println!("suffix: {accepted} of {cases} accepted");
    assert!(accepted > 1000);
}

#[test]
fn display_field_reader() {
    let mut r = Rng::new(0x5eed_0002);
    let (mut accepted, mut cases) = (0, 0);
    for _ in 0..50_000 {
        let mut bytes = vec![r.byte(), r.byte(), if r.chance(3) { r.byte() } else { [0, 1, 2, 4, 6, 8, 9, 10][r.below(8) as usize] }];
        if r.chance(2) { for _ in 0..6 { bytes.push(r.byte()); } }
        if r.chance(4) { mutate(&mut r, &mut bytes); }
        cases += 1;
        let Ok(display) = body::read_display(&bytes) else { continue };
        accepted += 1;
        assert!((0.0..=1.0).contains(&display.energy));
        assert_eq!(display.roll.is_some(), bytes.len() == 9);
        assert!(!(display.sprint && (display.roll.is_some() || display.exhausted)));
        if let Some((direction, _, _)) = display.roll { assert!(matches!(direction, RollDirection::Forward | RollDirection::Left | RollDirection::Right)); }
        assert_eq!(bytes[2] & !0b1111, 0);
    }
    println!("display: {accepted} of {cases} accepted");
    assert!(accepted > 1000);
}

#[test]
fn class_number_words() {
    let mut r = Rng::new(0x5eed_0003);
    let (mut accepted, mut cases) = (0, 0);
    for _ in 0..50_000 {
        let t = tuning(&mut r);
        let mut w: Vec<f64> = words::put(&t).to_vec();
        for _ in 0..r.below(3) {
            match r.below(4) {
                0 => { let at = r.below(w.len() as u32) as usize; w[at] = r.number(); }
                1 => { let at = r.below(w.len() as u32) as usize; w[at] = f64::from(r.below(3) as u8); }
                2 => { w.pop(); }
                _ => w.push(r.number()),
            }
        }
        cases += 1;
        let Some(back) = words::take(&w) else { continue };
        accepted += 1;
        assert!(back.validate().is_ok());
        let again = words::put(&back);
        assert!(again.iter().zip(&w).all(|(a, b)| a.to_bits() == b.to_bits()) && again.len() == w.len(), "{w:?}");
    }
    println!("words: {accepted} of {cases} accepted");
    assert!(accepted > 1000);
}

// Whole checkpoints delivered shuffled, with losses and duplicates, each
// with its own correction accepted before, among or after its parts, or
// lost, must commit exactly as sent and only with that correction; damaged
// parts, offered to a second assembly (the transport authenticates every
// datagram, so reassembly never has to catch damage), may be refused or
// ignored but never panic or yield more than a checkpoint's bytes.
#[test]
fn checkpoint_parts() {
    let mut r = Rng::new(0x5eed_0004);
    let (mut assembly, mut damaged) = (checkpoint::Assembly::default(), checkpoint::Assembly::default());
    let (mut committed, mut expected, mut refused, mut offered) = (0, 0, 0, 0);
    let mut parts = Vec::new();
    for round in 0..20_000u32 {
        // Epochs and ticks only move forward; the unit and its life change now and then.
        let anchor = checkpoint::Anchor { epoch: (round / 500) as u16, tick: round * 3 + r.below(3) as u32, subject: 7 + round / 900,
            life: 1 + round / 300 };
        let identity = checkpoint::Identity { anchor, kind: revisions::COMBAT, revision: 1 };
        let len = 1 + r.below(checkpoint::MAX_BYTES as u32 + 40) as usize;
        // One identity is one state: its bytes follow from the identity and length.
        let payload: Vec<u8> = (0..len).map(|i| (i as u8).wrapping_mul(13) ^ anchor.tick as u8 ^ (anchor.life as u8) << 4).collect();
        if checkpoint::split(identity, &payload, &mut parts).is_err() {
            assert!(len > checkpoint::MAX_BYTES);
            continue;
        }
        let mut order: Vec<usize> = (0..parts.len()).collect();
        for i in (1..order.len()).rev() { let j = r.below(i as u32 + 1) as usize; order.swap(i, j); }
        // Where among the parts the correction is accepted, if it arrives at all.
        let accept_at = (!r.chance(10)).then(|| r.below(order.len() as u32 + 1) as usize);
        let (mut done, mut lost) = (None, false);
        for (n, &at) in order.iter().enumerate() {
            if accept_at == Some(n) {
                done = done.or(assembly.accept(anchor));
                let _ = damaged.accept(anchor);
            }
            if r.chance(25) { lost = true; continue; }
            for _ in 0..if r.chance(20) { 2 } else { 1 } {
                offered += 1;
                match assembly.offer(&parts[at], revisions::COMBAT, 1) {
                    Ok(Some(id)) => done = Some(id),
                    Ok(None) => {}
                    Err(_) => refused += 1,
                }
            }
            let mut part = parts[at].clone();
            mutate(&mut r, &mut part);
            if let Ok(Some(_)) = damaged.offer(&part, revisions::COMBAT, 1) {
                assert!(damaged.complete().is_some_and(|(_, b)| !b.is_empty() && b.len() <= checkpoint::MAX_BYTES));
            }
        }
        if accept_at == Some(order.len()) { done = done.or(assembly.accept(anchor)); }
        expected += usize::from(accept_at.is_some() && !lost);
        if let Some(id) = done {
            assert_eq!(id, identity);
            assert_eq!(assembly.complete().unwrap(), (identity, &payload[..]));
            committed += 1;
        }
        let mut junk: Vec<u8> = (0..r.below(1200)).map(|_| r.byte()).collect();
        if r.chance(2) && junk.len() >= 4 { junk[..4].copy_from_slice(&checkpoint::MAGIC.to_le_bytes()); }
        let _ = damaged.offer(&junk, revisions::COMBAT, 1);
    }
    println!("checkpoint: {committed} committed from {offered} parts, {refused} refused, {} superseded, {} stale",
        assembly.superseded, assembly.stale);
    // Every checkpoint whose correction was accepted and whose parts all
    // arrived commits, and no other; valid parts are never refused.
    assert_eq!((committed, refused), (expected, 0));
    assert!(committed > 10_000);
}

// A JSON value near the shape a reader expects: sometimes the valid value,
// sometimes with a member replaced, removed, duplicated or added.
fn near(r: &mut Rng, valid: &Value) -> Value {
    let mut v = valid.clone();
    for _ in 0..r.below(3) {
        match &mut v {
            Value::Object(map) if !map.is_empty() => {
                let keys: Vec<String> = map.keys().cloned().collect();
                let key = keys[r.below(keys.len() as u32) as usize].clone();
                match r.below(4) {
                    0 => { map.remove(&key); }
                    1 => { map.insert(key, odd(r)); }
                    2 => { map.insert(format!("x{}", r.below(9)), odd(r)); }
                    _ => { let inner = map[&key].clone(); map.insert(key, near(r, &inner)); }
                }
            }
            Value::Array(items) if !items.is_empty() => {
                let at = r.below(items.len() as u32) as usize;
                match r.below(4) {
                    0 => { items.remove(at); }
                    1 => items[at] = odd(r),
                    2 => { let copy = items[at].clone(); items.push(copy); }
                    _ => { let inner = items[at].clone(); items[at] = near(r, &inner); }
                }
            }
            _ => v = odd(r),
        }
    }
    v
}

fn odd(r: &mut Rng) -> Value {
    match r.below(12) {
        0 => Value::Null,
        1 => json!(true),
        2 => json!(-1),
        3 => json!(65536),
        4 => json!(1.5),
        5 => json!(u64::MAX),
        6 => json!("SWBF2"),
        7 => json!("swbf2/"),
        8 => json!("a".repeat(65)),
        9 => json!([]),
        10 => json!({}),
        _ => json!(r.below(300)),
    }
}

#[test]
fn match_descriptor_and_request_readers() {
    let mut r = Rng::new(0x5eed_0005);
    let token = "00112233445566778899aabbccddeeff";
    let descriptor = Descriptor { mode: "eli".into(), map: "swbf2/kam9".into(), token: token.into(), records: Table::of(revisions::WRITES) };
    let request = Request { mode: "ass".into(), map: "swbf2/spa_9".into(), token: token.into(), supports: Supports::of(revisions::SUPPORTS) };
    let (valid_d, valid_r) = (descriptor.json(), request.json());
    let (mut kept, mut cases) = (0, 0);
    for _ in 0..20_000 {
        let d = near(&mut r, &valid_d);
        let q = near(&mut r, &valid_r);
        cases += 2;
        if let Ok(read) = Descriptor::read(&d) {
            kept += 1;
            assert_eq!(Descriptor::read(&read.json()).unwrap(), read);
            assert!(read.map.starts_with("swbf2/") && revisions::match_token(&read.token));
            assert!(read.records.entries.len() <= revisions::MAX_KINDS && read.records.entries.iter().all(|e| e.revision >= 1));
        }
        if let Ok(read) = Request::read(&q) {
            kept += 1;
            assert_eq!(Request::read(&read.json()).unwrap(), read);
            assert!(read.supports.kinds.iter().all(|s| 1 <= s.min && s.min <= s.max));
        }
        // The tables alone, as random and as mutated bytes, and what a plan of them makes.
        let bytes: Vec<u8> = (0..r.below(40)).map(|_| r.byte()).collect();
        let table = Table::read(&base64(&bytes)).or_else(|_| Table::read(valid_d["records"].as_str().unwrap()));
        let mut support_bytes = Vec::new();
        for _ in 0..r.below(6) { support_bytes.push(1 + r.below(30) as u8); for _ in 0..4 { support_bytes.push(r.byte()); } }
        if let (Ok(table), Ok(supports)) = (table, Supports::read(&base64(&support_bytes))) {
            match revisions::plan(&table, &supports) {
                Ok(plan) => assert_eq!(plan.kinds.len(), table.entries.len()),
                Err(unreadable) => assert!(table.entries.iter().any(|e| e.required && e.kind == unreadable.kind)),
            }
        }
    }
    println!("match readers: {kept} of {cases} accepted");
    assert!(kept > 1000);
}

const TEXTS: [&str; 6] = ["", "k", "level.yard.objectives.1", "ä bell", "winch", "a longer name for a timer"];
fn word<'a>(r: &mut Rng, long: &'a str, empty: bool) -> &'a str {
    match r.below(8) { 0 => long, k => TEXTS[(k as usize - 1).max(usize::from(!empty)).min(5)] }
}

// A mission state a writer could hold.
fn state<'a>(r: &mut Rng, long: &'a str) -> mission::State<'a> {
    match r.below(8) {
        0 => mission::State::Match(mission::Match { over: r.chance(2), game_over: r.chance(2), finished: r.chance(2), faulted: r.chance(5),
            points_shown: [r.chance(2), r.chance(2)], minimap_hidden: r.chance(3), posts_hidden: r.chance(3),
            outcome: match r.below(5) { 0 => Some(CompactOutcome::Victory(1 + r.below(7) as i32)), 1 => Some(CompactOutcome::Defeat(1 + r.below(7) as i32)),
                2 => Some(CompactOutcome::Draw), 3 => Some(CompactOutcome::FaultedDraw), _ => None },
            target: r.chance(2).then(|| r.next().low()), points: [r.next().low(), r.below(200) as u32],
            reinforcements: [r.chance(2).then(|| r.next().low()), r.chance(2).then(|| r.below(300) as u32)] }),
        1 => {
            let shown = !r.chance(3);
            let due = (shown && r.chance(2)).then(|| r.next().low());
            mission::State::Timer(mission::Timer { shown, due, left: if shown && due.is_none() { r.next().low() } else { 0 },
                name: if shown { word(r, long, true) } else { "" } })
        }
        2 => {
            let mut rows: [Option<i32>; mission::PERSONAL_SLOTS] = std::array::from_fn(|_| r.chance(3).then(|| r.next().low() as i32));
            if rows.iter().all(Option::is_none) { rows[0] = Some(0); }
            mission::State::Personal(mission::Personal { field: r.below(mission::PERSONAL_FIELDS as u32) as u8, rows })
        }
        3 => mission::State::Deployments(mission::Deployments { field: r.below(mission::DEPLOYMENT_FIELDS as u32) as u8,
            rows: std::array::from_fn(|_| r.chance(4).then(|| mission::Deployment { due: r.next().low(), chosen: r.chance(2) })) }),
        4 => {
            let mut rows: [Option<u16>; mission::PERSONAL_SLOTS] = std::array::from_fn(|_| r.chance(3).then(|| 1 + r.below(65_535) as u16));
            if rows.iter().all(Option::is_none) { rows[0] = Some(1); }
            mission::State::Classes(mission::Classes { field: r.below(mission::CLASSES_FIELDS as u32) as u8, rows })
        }
        5 => {
            let cap = bfe_sim::battle::spawning::CLASS_CAPACITY as u32;
            let count = [r.below(cap + 1) as u8, r.below(cap + 1) as u8];
            let available = std::array::from_fn(|t| {
                let n = u32::from(count[t]);
                if n == 0 { 0 } else { (r.next().low() as u16) & ((1u16 << n) - 1) }
            });
            mission::State::ClassRoom(mission::ClassRoom { count, available })
        }
        6 => mission::State::Leaving(mission::Leaving { field: r.below(mission::LEAVING_FIELDS as u32) as u8,
            rows: std::array::from_fn(|_| r.chance(4).then(|| r.next().low())) }),
        _ => {
            let count = 1 + r.below(mission::OBJECTIVE_ROWS as u32) as usize;
            let rows = std::array::from_fn(|at| (at < count).then(|| mission::Objective { active: r.chance(2), completed: r.chance(2),
                team: if r.chance(9) { mission::NOBODY } else { r.below(8) as u8 }, key: word(r, long, false) }));
            mission::State::Objectives(mission::Objectives { field: r.below(mission::OBJECTIVE_FIELDS as u32) as u8, rows })
        }
    }
}

// Whatever the mission's field readers accept writes back byte for byte, names
// the field it belongs in and stays within what the keyframe counts for it.
#[test]
fn mission_field_readers() {
    let mut r = Rng::new(0x5eed_000b);
    let long = "m".repeat(mission::MAX_TEXT);
    let pair = |r: &mut Rng| r.chance(2).then(|| [r.below(90_000) as f32 / 7.0, r.below(90_000) as f32 / 7.0]);
    let (mut kept, mut cases, mut whole) = ([0; 2], 0, 0);
    for _ in 0..60_000 {
        cases += 1;
        let (valid_state, mut bytes) = (state(&mut r, &long), Vec::new());
        mission::write_state(&valid_state, &mut bytes);
        assert_eq!(mission::read_state(&bytes), Ok(valid_state));
        let most = match valid_state {
            mission::State::Match(_) => mission::MATCH_BYTES, mission::State::Timer(_) => mission::TIMER_BYTES,
            mission::State::Personal(_) => mission::PERSONAL_BYTES, mission::State::Objectives(_) => mission::OBJECTIVES_BYTES,
            mission::State::Deployments(_) => mission::DEPLOYMENTS_BYTES, mission::State::Classes(_) => mission::CLASSES_BYTES,
            mission::State::ClassRoom(_) => mission::CLASS_ROOM_BYTES, mission::State::Leaving(_) => mission::LEAVING_BYTES,
        };
        assert!(bytes.len() + 2 <= most && most <= 900);
        mutate(&mut r, &mut bytes);
        if let Ok(read) = mission::read_state(&bytes) {
            kept[0] += 1;
            let mut again = Vec::new();
            mission::write_state(&read, &mut again);
            assert_eq!(again, bytes);
            assert!(mission::cleared(read.id()).is_some(), "{:#x}", read.id());
        }
        let system = mission::System { index: r.below(mission::SYSTEMS as u32) as u8, alive: r.chance(2), marker: r.chance(2), team: r.below(8) as u8,
            placed: match r.below(3) { 0 => mission::Placed::Elsewhere, 1 => mission::Placed::Building(r.next().low() as u16), _ => mission::Placed::Hull(r.next().low() as u16) },
            points: r.next().low() as i32, health: pair(&mut r), shield: pair(&mut r), hud: pair(&mut r), name: word(&mut r, &long, false) };
        bytes.clear();
        mission::write_system(&system, &mut bytes);
        assert_eq!(mission::read_system(&bytes), Ok(system));
        assert!(bytes.len() + 2 <= mission::SYSTEM_BYTES);
        mutate(&mut r, &mut bytes);
        if let Ok(read) = mission::read_system(&bytes) {
            kept[1] += 1;
            let mut again = Vec::new();
            mission::write_system(&read, &mut again);
            assert_eq!(again, bytes);
        }
        // Bytes that are nothing at all.
        let noise: Vec<u8> = (0..r.below(80)).map(|_| r.byte()).collect();
        whole += usize::from(mission::read_state(&noise).is_ok()) + usize::from(mission::read_system(&noise).is_ok());
    }
    println!("mission fields: {kept:?} of {cases} damaged accepted, {whole} of noise");
    assert!(kept.iter().all(|kept| *kept > 2_000), "{kept:?}");
}

// Whatever the reliable readers accept says the same when it is written again.
#[test]
fn mission_notice_and_roster_readers() {
    let mut r = Rng::new(0x5eed_000c);
    let actor = |r: &mut Rng| mission::Actor { slot: r.next().low() as u16, human: r.chance(2), class: r.below(40) as u16 };
    let (mut kept, mut cases) = ([0; 2], 0);
    for _ in 0..20_000 {
        cases += 1;
        let text = TEXTS[1 + r.below(5) as usize].to_string();
        let tick = r.next().low();
        let cue = match r.below(4) {
            0 => mission::Cue::Text(text), 1 => mission::Cue::Hint(text), 2 => mission::Cue::Voice(text),
            _ => {
                let killer = r.chance(2).then(|| actor(&mut r));
                let gun = (killer.is_none() && r.chance(2)).then(|| TEXTS[r.below(6) as usize].to_string());
                mission::Cue::Kill { killer, victim: actor(&mut r), weapon: r.next().low() as u16, gun }
            }
        };
        let notice = mission::Notice { sequence: 1 + r.below(u32::MAX) as u32, tick, until: tick.wrapping_add(r.below(301) as u32), cue };
        assert_eq!(mission::Notice::read(&notice.body()), Ok(notice.clone()));
        let body = near(&mut r, &notice.body());
        if let Ok(read) = mission::Notice::read(&body) {
            kept[0] += 1;
            assert_eq!(read.body(), body);
            assert!(read.sequence >= 1 && read.until.wrapping_sub(read.tick) <= mission::NOTICE_TICKS);
        }
        let team = |r: &mut Rng| mission::Team { name: TEXTS[r.below(6) as usize].into(), icons: (0..r.below(5)).map(|_| "badge".to_string()).collect(),
            classes: (0..r.below(13)).map(|_| 1 + r.below(30) as u16).collect() };
        let roster = mission::Roster { teams: [team(&mut r), team(&mut r)] };
        assert_eq!(mission::Roster::read(&roster.body(), |id| id <= 30), Ok(roster.clone()));
        let body = near(&mut r, &roster.body());
        if let Ok(read) = mission::Roster::read(&body, |id| id <= 30) {
            kept[1] += 1;
            assert_eq!(read.body(), body);
            assert!(read.teams.iter().flat_map(|team| &team.classes).all(|id| (1..=30).contains(id)));
        }
    }
    println!("mission records: {kept:?} of {cases} accepted");
    assert!(kept.iter().all(|kept| *kept > 5_000), "{kept:?}");
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 { if i <= chunk.len() { out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize])); } else { out.push('='); } }
    }
    out
}

#[test]
fn records_sections() {
    let mut r = Rng::new(0x5eed_0006);
    let plan = revisions::plan(&Table::of(revisions::WRITES), &Supports::of(revisions::SUPPORTS)).unwrap();
    let (mut kept, mut cases) = (0, 0);
    for _ in 0..50_000 {
        let mut bytes = Vec::new();
        let records: Vec<(u8, u16, Vec<u8>)> = (0..r.below(4)).map(|_| (1 + r.below(40) as u8, 1 + r.below(3) as u16,
            (0..r.below(20)).map(|_| r.byte()).collect())).collect();
        let sections: Vec<revisions::Section<'_>> = records.iter().map(|(kind, revision, body)| revisions::Section { kind: *kind, revision: *revision, body }).collect();
        revisions::write_section(&sections, &mut bytes).unwrap();
        if r.chance(3) { mutate(&mut r, &mut bytes); }
        cases += 1;
        let Ok((read, used)) = revisions::read_section(&bytes) else { continue };
        kept += 1;
        assert!(used <= bytes.len() && read.len() <= revisions::SECTION_RECORDS);
        for record in &read { let _ = revisions::sectioned(&plan, record); }
    }
    println!("sections: {kept} of {cases} accepted");
    assert!(kept > 1000);
}

// A 2005 match whose table advertises optional kind 91, which this build
// does not implement, at revision 2.
fn skipping_plan() -> Plan {
    let table = Table::of(&[revisions::WRITES[0], revisions::WRITES[1], revisions::written(revisions::SIT_OUT).unwrap(), Entry { kind: 91, revision: 2, required: false }]);
    revisions::plan(&table, &Supports::of(revisions::SUPPORTS)).unwrap()
}
// A record's kind: mostly the skipped one, sometimes a read one or any.
fn some_kind(r: &mut Rng) -> u8 { match r.below(4) { 0 => r.byte(), 1 => revisions::INPUT, _ => 91 } }
// A record's revision: mostly zero to four, sometimes any.
fn some_revision(r: &mut Rng) -> u16 { match r.below(6) { 0 => r.next().low() as u16, k => (k - 1) as u16 } }

#[test]
fn skipped_section_records() {
    let mut r = Rng::new(0x5eed_0007);
    let plan = skipping_plan();
    let (mut skipped, mut cases) = (0, 0);
    for _ in 0..50_000 {
        let count = 1 + r.below(3) as u8;
        let mut bytes = vec![count];
        for _ in 0..count {
            let (kind, revision) = (some_kind(&mut r), some_revision(&mut r));
            let body: Vec<u8> = (0..r.below(12)).map(|_| r.byte()).collect();
            let length = if r.chance(10) { r.below(16) as u16 } else { body.len() as u16 };
            bytes.push(kind);
            bytes.extend(revision.to_le_bytes());
            bytes.extend(length.to_le_bytes());
            bytes.extend(&body);
        }
        if r.chance(3) { mutate(&mut r, &mut bytes); }
        cases += 1;
        let Ok((records, used)) = revisions::read_section(&bytes) else { continue };
        assert!(used <= bytes.len());
        for record in &records {
            assert!(record.kind != 0 && record.revision != 0);
            if revisions::sectioned(&plan, record) == Ok(None) {
                assert_eq!((record.kind, record.revision), (91, 2));
                skipped += 1;
            }
        }
    }
    println!("section records: {skipped} skipped in {cases} sections");
    assert!(skipped > 3000);
}

#[test]
fn skipped_reliable_records() {
    let mut r = Rng::new(0x5eed_0008);
    let plan = skipping_plan();
    let (mut skipped, mut cases) = (0, 0);
    for _ in 0..50_000 {
        let valid = json!({"type": "record", "kind": some_kind(&mut r), "revision": some_revision(&mut r), "body": {"x": r.below(9)}});
        let message = if r.chance(2) { near(&mut r, &valid) } else { valid };
        cases += 1;
        if revisions::recorded(&plan, &message) == Ok(None) {
            let object = message.as_object().unwrap();
            assert!(object.len() == 4 && object.contains_key("body"));
            assert_eq!((object["type"].as_str(), object["kind"].as_u64(), object["revision"].as_u64()), (Some("record"), Some(91), Some(2)));
            skipped += 1;
        }
    }
    println!("reliable records: {skipped} skipped of {cases}");
    assert!(skipped > 2000);
}

#[test]
fn skipped_checkpoint_parts() {
    let mut r = Rng::new(0x5eed_0009);
    let plan = skipping_plan();
    let mut parts = Vec::new();
    let (mut skipped, mut cases) = (0, 0);
    for round in 0..20_000u32 {
        let (kind, revision) = (some_kind(&mut r).max(1), some_revision(&mut r).max(1));
        let anchor = checkpoint::Anchor { epoch: 1, tick: round, subject: 7, life: 1 };
        let payload: Vec<u8> = (0..1 + r.below(3000)).map(|_| r.byte()).collect();
        checkpoint::split(checkpoint::Identity { anchor, kind, revision }, &payload, &mut parts).unwrap();
        for part in &parts {
            let mut part = part.clone();
            let (damaged, truncated) = (r.chance(2), r.chance(6));
            if truncated { let n = part.len() as u32; part.truncate(r.below(n) as usize); } else if damaged { mutate(&mut r, &mut part); }
            cases += 1;
            // The receiver's decision: the part's whole header, then the plan.
            let decision = checkpoint::identity(&part).map(|id| (id, plan.skips(id.kind, id.revision)));
            if truncated { assert!(decision.is_err()); }
            if !damaged && !truncated { assert_eq!(decision.unwrap().1 == Ok(true), (kind, revision) == (91, 2)); }
            if let Ok((id, Ok(true))) = decision {
                assert_eq!((id.kind, id.revision), (91, 2));
                skipped += 1;
            }
        }
    }
    println!("checkpoint parts: {skipped} skipped of {cases}");
    assert!(skipped > 1000);
}

// A 2005 match's record checks, noting whether any refused.
struct Watched { checks: Checks, refused: Rc<Cell<bool>> }
impl Watched {
    fn note<T>(&self, result: Result<T, &'static str>) -> Result<T, &'static str> {
        if result.is_err() { self.refused.set(true); }
        result
    }
}
impl RecordChecks for Watched {
    fn reads(&self, schema: u16) -> Result<bool, &'static str> { self.note(self.checks.reads(schema)) }
    fn skipped(&self, schema: u16, payload: &[u8]) -> Result<(), &'static str> { self.note(self.checks.skipped(schema, payload)) }
    fn field(&self, schema: u16, id: u32, payload: &[u8]) -> Result<(), &'static str> { self.note(self.checks.field(schema, id, payload)) }
    fn correction(&self, epoch: f64, tick: f64, id: u32, generation: u32, schema: u16, payload: &[u8]) -> Result<(), &'static str> {
        self.note(self.checks.correction(epoch, tick, id, generation, schema, payload))
    }
}

// The parts of one snapshot and its own datagram: whether it was acknowledged.
fn feed(receiver: &mut Session, parts: &[Vec<u8>], tick: f64, baseline: f64) -> bool {
    let bounds = PositionBounds::LEGACY;
    let mut own = Writer::default();
    own_datagram_into(&mut own, OwnParts { epoch: 1., tick, interval: 3., baseline, ack: 0., second: false, count: 1.,
        correction: None, summaries: None, cosmetics: None }).unwrap();
    for packet in parts.iter().map(Vec::as_slice).chain(std::iter::once(own.0.as_slice())) {
        let _ = receiver.receive_with_bounds(packet, &bounds);
    }
    receiver.header[h::ACK_TICK] == tick && receiver.header[h::COMPLETE] != 0.
}

// Snapshots through the real receiver: a skipped field between two ordinary
// ones, written whole in a keyframe, then changed in place (a word delta),
// replaced or removed against it. Undamaged, a snapshot is acknowledged
// exactly when the skipped record shows revision 2; damaged or truncated, it
// is never acknowledged after a refused check.
#[test]
fn skipped_snapshot_fields() {
    let mut r = Rng::new(0x5eed_000a);
    let plan = skipping_plan();
    let width = PositionBounds::LEGACY.width();
    let schema = f64::from(revisions::field_schema(91));
    let valid = |p: &[u8]| p.len() >= 2 && p[..2] == 2u16.to_le_bytes();
    let world = |skipped: Option<&Vec<u8>>| {
        let mut world = vec![WireField { id: 1., schema: 7., generation: 1., payload: vec![9; 12] }];
        world.extend(skipped.map(|p| WireField { id: 2., schema, generation: 1., payload: p.clone() }));
        world.push(WireField { id: 3., schema: 7., generation: 1., payload: vec![8; 12] });
        world
    };
    let payload = |r: &mut Rng, len: usize| {
        let mut p: Vec<u8> = (0..len).map(|_| r.byte()).collect();
        if len >= 2 && !r.chance(4) { p[..2].copy_from_slice(&2u16.to_le_bytes()); }
        p
    };
    let damage = |r: &mut Rng, parts: &mut Vec<Vec<u8>>| {
        let at = r.below(parts.len() as u32) as usize;
        if r.chance(3) { let n = parts[at].len() as u32; parts[at].truncate(r.below(n) as usize); } else { mutate(r, &mut parts[at]); }
    };
    let (mut acknowledged, mut cases) = (0, 0);
    for _ in 0..4_000 {
        let refused = Rc::new(Cell::new(false));
        let mut receiver = Session::new();
        receiver.reserve_receive();
        receiver.check_records(Box::new(Watched { checks: Checks::new(plan.clone(), vec![]), refused: refused.clone() }));
        let mut encoder = Encoder::new(8);
        let len = r.below(48) as usize;
        let first = payload(&mut r, len);
        let base = WireSnapshot { tick: 3., epoch: 1., slots: vec![], world: world(Some(&first)) };
        let mut parts = encoder.encode(&base, None, 3., width).unwrap().to_vec();
        let damaged = r.chance(4);
        if damaged { damage(&mut r, &mut parts); }
        let keyframe = feed(&mut receiver, &parts, 3., 0.);
        cases += 1;
        if !damaged { assert_eq!(keyframe, valid(&first), "{first:?}"); }
        assert!(!(keyframe && refused.get()));
        acknowledged += usize::from(keyframe);
        if !keyframe || damaged { continue; }
        let second = match r.below(4) {
            0 => None,
            1 => { let len = r.below(48) as usize; Some(payload(&mut r, len)) }
            _ => {
                let mut p = first.clone();
                for _ in 0..1 + r.below(3) { let at = r.below(p.len() as u32) as usize; p[at] = r.byte(); }
                Some(p)
            }
        };
        let next = WireSnapshot { tick: 6., epoch: 1., slots: vec![], world: world(second.as_ref()) };
        let mut parts = encoder.encode(&next, Some(&base), 3., width).unwrap().to_vec();
        let damaged = r.chance(4);
        if damaged { damage(&mut r, &mut parts); }
        refused.set(false);
        let delta = feed(&mut receiver, &parts, 6., 3.);
        cases += 1;
        if !damaged { assert_eq!(delta, second.as_ref().is_none_or(|p| valid(p)), "{first:?} then {second:?}"); }
        assert!(!(delta && refused.get()));
        acknowledged += usize::from(delta);
    }
    println!("snapshots: {acknowledged} acknowledged of {cases}");
    assert!(acknowledged > 2000);
}

#[test]
fn space_wire_readers_and_checks() {
    use bfe_sim::space::flight::{SpaceFlightProfile, SpaceFighter, SpacePilotInput, SpaceVehiclePad};
    use bfe_sim::space::local::PilotControls;
    use bfe_sim::wire::revisions::{plan, Supports, Table, SUPPORTS, WRITES_SPACE};
    use bfe_sim::wire::space::{
        read_fighter_field, read_frigates, read_ordnance_field, read_own_body,
        read_pilot_entries_into, read_props_field, write_fighter_chunk, write_frigates,
        write_guided, write_own_chair, write_own_pilot, write_pilot_entries, write_seats,
        GuidedSnap, OwnChairState, PilotEntry, SeatEntry, FIGHTER_SCHEMA, FRIGATES_SCHEMA,
        MAX_PILOT_ENTRIES, ORDNANCE_SCHEMA, OWN_BODY_SCHEMA, PROPS_SCHEMA, REVISION,
    };

    let mut r = Rng::new(0x5eed_5a60);
    let space_plan = plan(&Table::of(WRITES_SPACE), &Supports::of(SUPPORTS)).unwrap();
    let checks = Checks::new(space_plan, vec![None, None]);
    let bounds = PositionBounds::from_map_extents([-4000.0; 3], [4000.0; 3]).unwrap();

    // 1. PILOT rev 1 reader (30,000 cases: valid, mutated, random).
    let (mut ok_pilot, mut pilot_cases) = (0usize, 0usize);
    let mut scratch = [PilotEntry::default(); MAX_PILOT_ENTRIES];
    for _ in 0..30_000 {
        let mut bytes = Vec::new();
        if r.chance(4) {
            let len = r.below(240) as usize;
            bytes.extend((0..len).map(|_| r.byte()));
        } else {
            let n = 1 + r.below(MAX_PILOT_ENTRIES as u32) as usize;
            let entries: Vec<PilotEntry> = (0..n)
                .map(|i| PilotEntry {
                    sequence: (i as u32 + 1) * 3,
                    controls: PilotControls {
                        mouse: [
                            (r.below(200) as f64 - 100.0) / 100.0,
                            (r.below(200) as f64 - 100.0) / 100.0,
                        ],
                        turret_look: [
                            (r.below(200) as f64 - 100.0) / 50.0,
                            (r.below(200) as f64 - 100.0) / 50.0,
                        ],
                        turret_turn: (r.below(3) as f64) - 1.0,
                        cycle_weapon: (r.below(3) as i8) - 1,
                        terminal_cycle: (r.below(3) as i8) - 1,
                        use_pressed: r.chance(2),
                        flight: SpacePilotInput {
                            throttle: (r.below(201) as f64 - 100.0) / 100.0,
                            stick_x: (r.below(201) as f64 - 100.0) / 100.0,
                            stick_y: (r.below(201) as f64 - 100.0) / 100.0,
                            roll: (r.below(201) as f64 - 100.0) / 100.0,
                            boost: r.chance(2),
                            eject_held: r.chance(4),
                            takeoff_or_land: r.chance(4),
                            fire_primary: r.chance(2),
                            fire_secondary: r.chance(2),
                            lock_target: r.chance(3),
                            trick_roll: (r.below(3) as i8) - 1,
                            trick_flip: r.below(3) as i8,
                            ..Default::default()
                        },
                    },
                })
                .collect();
            write_pilot_entries(&entries, &mut bytes).unwrap();
            if r.chance(2) {
                mutate(&mut r, &mut bytes);
            }
        }
        pilot_cases += 1;
        if let Ok(count) = read_pilot_entries_into(&bytes, &mut scratch) {
            assert!((1..=MAX_PILOT_ENTRIES).contains(&count));
            for e in &scratch[..count] {
                assert!(e.controls.mouse[0].is_finite() && e.controls.mouse[1].is_finite());
                assert!(e.controls.turret_look[0].is_finite() && e.controls.turret_look[1].is_finite());
                assert!(e.controls.flight.throttle.is_finite());
                assert!((-1..=1).contains(&e.controls.cycle_weapon));
                assert!((-1..=1).contains(&e.controls.terminal_cycle));
                assert!((-1..=1).contains(&e.controls.flight.trick_roll));
                assert!((0..=2).contains(&e.controls.flight.trick_flip));
            }
            ok_pilot += 1;
        }
    }
    println!("space pilot: {ok_pilot} of {pilot_cases} accepted");
    assert!(ok_pilot > 10_000);

    // 2. Shared Space world fields (0x0211, 0x0213, 0x0214, 0x0215) via both direct readers & Checks::field (40,000 cases).
    let mut profile = SpaceFlightProfile::default();
    profile.class_name = "test_fighter".into();
    profile.max_health = 1000.0;
    let pad = SpaceVehiclePad {
        name: "p0".into(),
        team: 1,
        profile_index: 0,
        control_zone: String::new(),
        position: [10.0, 2.0, -30.0],
        yaw: 0.0,
        matrix: [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 10., 2., -30., 1.],
        spawn_count: 1,
        spawn_time: 8.0,
        expire_time_enemy: 20.0,
        expire_time_field: 40.0,
        decay_time: 10.0,
        respawn_timer: 8.0,
        floor_y: 0.0,
    };
    let fighters: Vec<SpaceFighter> = (0..8)
        .map(|i| SpaceFighter::spawned_on_pad(i, 1 + i as u32, 0, &pad, 0, &profile, 0.0, None, None))
        .collect();

    let (mut ok_fields, mut field_cases) = (0usize, 0usize);
    for _ in 0..40_000 {
        let mut raw = Vec::new();
        let (schema, id) = match r.below(7) {
            0 => {
                raw.extend(REVISION.to_le_bytes());
                write_fighter_chunk(0, &fighters, &bounds, &mut raw).unwrap();
                (FIGHTER_SCHEMA, 0u32)
            }
            1 => {
                raw.extend(REVISION.to_le_bytes());
                let seats = [
                    SeatEntry { unit_slot: 1, is_chair: false, seat: 0, mount_index: 0 },
                    SeatEntry { unit_slot: 2, is_chair: true, seat: 0, mount_index: 1 },
                ];
                write_seats(&seats, &mut raw).unwrap();
                (FIGHTER_SCHEMA, 256u32)
            }
            2 => {
                raw.extend(REVISION.to_le_bytes());
                write_frigates(&[(1, 10), (2, 42), (0, 0)], &mut raw).unwrap();
                (FRIGATES_SCHEMA, 0u32)
            }
            3 => {
                raw.extend(REVISION.to_le_bytes());
                let g = [GuidedSnap {
                    slot: 3,
                    serial: 99,
                    weapon_idx: 2,
                    pos: [100, 200, 300],
                    vel: [10, -5, 40],
                }];
                write_guided(&g, &mut raw).unwrap();
                (ORDNANCE_SCHEMA, 0u32)
            }
            4 => {
                raw.extend(REVISION.to_le_bytes());
                // tag=1, 1 launch, 1 impact, 1 death
                raw.extend([1u8, 1, 1, 1]);
                // launch: serial u16, weapon_idx u8, flags u8, origin 3*u24, vel 3*i16
                let l_off = r.below(6) as u8;
                let l_flags = l_off | (u8::from(r.chance(2)) << 3) | (1 << 4);
                raw.extend(7u16.to_le_bytes());
                raw.extend([1u8, l_flags]);
                for c in [1000u32, 2000, 3000] {
                    raw.extend_from_slice(&c.to_le_bytes()[..3]);
                }
                for v in [100i16, 0, -50] {
                    raw.extend(v.to_le_bytes());
                }
                // impact: serial u16, weapon_idx u8, flags u8, pos 3*u24, optional oct_normal [i8; 2]
                let has_norm = r.chance(2);
                let i_off = r.below(6) as u8;
                let flags = i_off
                    | (u8::from(r.chance(2)) << 3)
                    | (u8::from(r.chance(2)) << 4)
                    | (u8::from(has_norm) << 5)
                    | (u8::from(r.chance(2)) << 6);
                raw.extend(7u16.to_le_bytes());
                raw.extend([1u8, flags]);
                for c in [1200u32, 2000, 2900] {
                    raw.extend_from_slice(&c.to_le_bytes()[..3]);
                }
                if has_norm {
                    raw.extend([0u8, 127u8]);
                }
                // death: class_idx u16, flags u8, pos 3*u24
                raw.extend(0u16.to_le_bytes());
                raw.push(u8::from(r.chance(2)));
                for c in [500u32, 500, 500] {
                    raw.extend_from_slice(&c.to_le_bytes()[..3]);
                }
                (ORDNANCE_SCHEMA, 1u32)
            }
            5 => {
                raw.extend(REVISION.to_le_bytes());
                // tag=0, 2 turrets, each [i16; 2]
                raw.extend([0u8, 2]);
                for [y, p] in [[100i16, -50], [200, 300]] {
                    raw.extend(y.to_le_bytes());
                    raw.extend(p.to_le_bytes());
                }
                (PROPS_SCHEMA, 0u32)
            }
            _ => {
                raw.extend(REVISION.to_le_bytes());
                // tag=1, 3 doors, each (open u8, flags u8)
                raw.extend([1u8, 3, 0, 0, 128, 2, 255, 3]);
                (PROPS_SCHEMA, 1u32)
            }
        };
        let mut id = id;
        if r.chance(2) {
            mutate(&mut r, &mut raw);
        }
        if r.chance(12) {
            id = r.below(512) as u32;
        }
        field_cases += 1;
        let check_res = checks.field(schema, id, &raw);
        if check_res.is_ok() {
            ok_fields += 1;
            if !raw.is_empty() {
                let (rev, body) = revisions::split_revision(&raw).unwrap();
                assert_eq!(rev, REVISION);
                match schema {
                    FIGHTER_SCHEMA => assert!(read_fighter_field(id, body).is_ok()),
                    FRIGATES_SCHEMA => assert!(read_frigates(id, body).is_ok()),
                    ORDNANCE_SCHEMA => assert!(read_ordnance_field(id, body).is_ok()),
                    PROPS_SCHEMA => assert!(read_props_field(id, body).is_ok()),
                    _ => unreachable!(),
                }
            }
        }
    }
    println!("space fields: {ok_fields} of {field_cases} accepted");
    assert!(ok_fields > 15_000);

    // 3. Own Space body (0x91) via both read_own_body and Checks::correction (20,000 cases).
    let (mut ok_own, mut own_cases) = (0usize, 0usize);
    for _ in 0..20_000 {
        let mut body_with_rev = Vec::new();
        if r.chance(2) {
            let f = &fighters[0];
            write_own_pilot(
                f,
                &profile,
                if r.chance(4) { 1 } else { 0 },
                r.chance(2).then_some(0),
                [0.1, -0.2],
                r.chance(2),
                [0.3, -0.1],
                r.chance(2),
                &bfe_sim::space::local::DoubleTapDetector::default(),
                &mut body_with_rev,
            )
            .unwrap();
        } else {
            let chair = OwnChairState {
                chair_building: 10,
                turret_building: 11,
                aim: [0.2, -0.1],
                feet: [-439.0, 0.5, -1274.0],
                turret_yaw: 0.2,
                turret_pitch: -0.1,
                cooldown: 0.0,
                heat: 12.5,
                overheated: false,
                shots_fired: 4,
            };
            write_own_chair(&chair, &mut body_with_rev).unwrap();
        }
        if r.chance(2) {
            mutate(&mut r, &mut body_with_rev);
        }
        // Build a complete own-player envelope carrying body_schema = 0x91.
        let mut correction = Vec::with_capacity(body_with_rev.len() + 128);
        correction.extend(1u32.to_le_bytes()); // epoch
        correction.extend(1u32.to_le_bytes()); // content_epoch
        correction.extend(1u16.to_le_bytes()); // class_id (present in checks.classes)
        correction.push(1); // team
        correction.push(OWN_BODY_SCHEMA); // body_schema
        correction.extend(0u32.to_le_bytes()); // vehicle_id
        correction.extend(0u32.to_le_bytes()); // vehicle_generation
        correction.extend((body_with_rev.len().min(u16::MAX as usize) as u16).to_le_bytes());
        correction.extend_from_slice(&body_with_rev);
        correction.extend(100.0f64.to_le_bytes()); // health
        correction.push(0); // dead
        correction.extend(0.0f64.to_le_bytes()); // death_time
        correction.extend(0.0f64.to_le_bytes()); // unit_dead_time
        correction.push(0); // removed
        for _ in 0..3 {
            correction.extend(0.0f64.to_le_bytes()); // kick[3]
        }
        correction.push(0); // zoom_step
        correction.extend(1.0f64.to_le_bytes()); // zoom
        correction.push(0); // first
        correction.extend([0u8, 0u8]); // selected[2]
        correction.push(0); // weapon_count
        correction.extend([0u8; 16]); // held_bits
        correction.extend(0u16.to_le_bytes()); // flags
        correction.extend(0u16.to_le_bytes()); // binding_profile_id
        correction.extend(0u32.to_le_bytes()); // discarded_input
        correction.push(0); // control_mode
        own_cases += 1;
        let check_res = checks.correction(1.0, 10.0, 1, 1, 11, &correction);
        if check_res.is_ok() {
            ok_own += 1;
            let (rev, body) = revisions::split_revision(&body_with_rev).unwrap();
            assert_eq!(rev, REVISION);
            assert!(read_own_body(body).is_ok());
        }
    }
    println!("space own 0x91: {ok_own} of {own_cases} accepted");
    assert!(ok_own > 8_000);
}
