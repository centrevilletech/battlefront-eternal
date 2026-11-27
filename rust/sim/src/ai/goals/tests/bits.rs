//! Per-tick bits of goal battles (positions, health, orders, hunted enemies
//! and melee samples), hashed for comparing architectures. Run
//! with `cargo test -p sim --lib goal_bits_digest -- --ignored --nocapture`
//! natively and under an ARM64 emulator; the printed lines must match.

use bfe_wide::{Fnv, Wide};
use super::arena::{Arena, ArenaRules, BotSpec, Op};
use crate::ai::goals::table::StoredArg;

fn deathmatch(team: u8) -> Op { Op::Add { team, kind: "Deathmatch", weight: 1.0, target: StoredArg::None, extra: StoredArg::None } }

fn bot(team: f64, x: f64, z: f64, yaw: f64, melee: bool) -> BotSpec {
    BotSpec { team, pos: [x, 0.0, z], yaw, hero: false, melee, health: 250.0, human: false, mixed: false }
}

fn digest(name: &str, mut arena: Arena, ticks: u32) {
    arena.activate();
    let (mut hash, mut words, mut over) = (Fnv::new(), 0u32, None);
    let mut mix = |hash: &mut Fnv, word: Wide| {
        hash.word(word);
        words += 1;
    };
    for tick in 0..ticks {
        arena.step_one();
        for i in 0..arena.units.len() {
            for p in arena.unit_pos(i) {
                mix(&mut hash, Wide::from_f64_bits(p));
            }
            mix(&mut hash, Wide::from_f64_bits(arena.unit_health(i)));
            mix(&mut hash, Wide::from_u32(u32::from(arena.unit_alive(i))));
            mix(&mut hash, Wide::from_f64_bits(arena.controller(i).commands.current.kind));
            let hunting = arena.bots().and_then(|b| b.hunting(i)).map_or(Wide::MAX, |t| Wide::from_u32(t as u32));
            mix(&mut hash, hunting);
            let r = arena.units[i];
            let key = crate::gen::F3UnitKey::of(r, arena.w.sim.store.unit.get(r).unwrap());
            let next = crate::units::f3::tick_in(&arena.w.units) + 1.0;
            let sample = arena.bots().and_then(|b| b.input(i, &key, next)).map_or(Wide::MAX, |m| {
                let (pressed, held) = m.combo_bits();
                Wide::from_u32(u32::from(pressed) | u32::from(held) << 8 | u32::from(m.guarding) << 16)
            });
            mix(&mut hash, sample);
        }
        if over.is_none() && arena.w.sim.store.battle.over {
            over = Some(tick);
        }
    }
    println!("goal-bits {name}: {:016x} over {words} words, battle over at {over:?}", hash.finish());
}

#[test]
#[ignore]
fn goal_bits_digest() {
    let mut mixed = Vec::new();
    for i in 0..6 {
        let z = i as f64 * 3.0 - 7.5;
        mixed.push(bot(1.0, -36.0, z, -std::f64::consts::FRAC_PI_2, i % 2 == 1));
        mixed.push(bot(2.0, 36.0, z, std::f64::consts::FRAC_PI_2, i % 2 == 1));
    }
    digest("flat-mixed", Arena::build(&mixed, None, 4, ArenaRules::Goals(vec![deathmatch(1), deathmatch(2)])), 3600);
    let blades: Vec<_> = (0..8).map(|i| bot(1.0 + (i % 2) as f64, if i % 2 == 0 { -10.0 } else { 10.0 }, i as f64 * 2.0 - 7.0, 0.0, true)).collect();
    digest("blades", Arena::build(&blades, None, 3, ArenaRules::Goals(vec![deathmatch(1), deathmatch(2)])), 3600);
    let mut wall = vec![bot(1.0, -4.0, 0.0, 0.0, false)];
    for i in 0..9 {
        wall.push(bot(2.0, 4.0 + (i % 3) as f64 * 3.0, (i / 3) as f64 * 4.0 - 4.0, 0.0, false));
    }
    wall.push(bot(2.0, -4.0, 125.0, 0.0, false));
    digest("wall", Arena::build_with_wall(&wall, None, 99, ArenaRules::Goals(vec![deathmatch(1)]), true), 3600);
}
