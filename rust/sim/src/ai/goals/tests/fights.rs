//! Do goal bots fight? Hits landed by each side's classes (credited to the
//! attacker from the units' damage notices), deaths, the earliest hit, and
//! quiet stretches of a life (stationary without an emitted shot or a hostile hit) on the
//! invented arena. Run with `cargo test --release -p sim --lib goal_fights --
//! --ignored --nocapture`.

use super::arena::{Arena, ArenaRules, BotSpec, Op};
use crate::ai::goals::table::StoredArg;

fn deathmatch(team: u8) -> Op { Op::Add { team, kind: "Deathmatch", weight: 1.0, target: StoredArg::None, extra: StoredArg::None } }

fn bot(team: f64, x: f64, z: f64, yaw: f64, melee: bool) -> BotSpec {
    BotSpec { team, pos: [x, 0.0, z], yaw, hero: false, melee, health: 250.0, human: false, mixed: false }
}

enum Activity {
    Shot(usize),
    Hit(usize),
    Death(usize),
}

fn life_index(lives: &[crate::gen::F3UnitKey], slot: u32, life: f64) -> Option<usize> {
    lives.iter().position(|k| k.slot == slot && k.life.to_bits() == life.to_bits())
}

fn activity(arena: &Arena, offset: &mut usize, lives: &[crate::gen::F3UnitKey]) -> Vec<Activity> {
    use crate::gen::*;
    let words = arena.w.sim.events.snapshot();
    let end = 1 + words[0] as usize;
    let mut at = 1 + *offset;
    let mut out = Vec::new();
    while at + 2 <= end {
        let (code, length) = (words[at] as u32, words[at + 1] as usize);
        let record = &words[at + 2..at + 2 + length];
        match code {
            EVENT_F3_SHOT => {
                let (row, heap) = record.split_at(F3ShotNotice::WORDS);
                let shot = F3ShotNotice::take(row, heap);
                if let Some(i) = lives.iter().position(|k| *k == shot.key) { out.push(Activity::Shot(i)); }
            }
            EVENT_F3_DAMAGE => {
                let (row, heap) = record.split_at(F3DamageNotice::WORDS);
                let hit = F3DamageNotice::take(row, heap);
                if hit.has_attacker_ref && hit.amount > 0. && hit.attacker_team != hit.team {
                    if let Some(i) = life_index(lives, hit.attacker_ref.id as u32, hit.attacker_ref.generation) {
                        out.push(Activity::Hit(i));
                    }
                }
            }
            EVENT_F3_DEATH => {
                let (row, heap) = record.split_at(F3DeathNotice::WORDS);
                let death = F3DeathNotice::take(row, heap);
                if let Some(i) = lives.iter().position(|k| *k == death.key) { out.push(Activity::Death(i)); }
            }
            _ => {}
        }
        at += 2 + length;
    }
    *offset = at - 1;
    out
}

#[test]
fn damage_from_a_previous_life_never_credits_a_reused_slot() {
    let lives = [crate::gen::F3UnitKey { slot: 7, generation: 1, life: 9. }];
    assert_eq!(life_index(&lives, 7, 8.), None);
    assert_eq!(life_index(&lives, 7, 9.), Some(0));
}

fn report(name: &str, bots: &[BotSpec], limit: u32, seconds: u32) {
    let mut arena = Arena::build(bots, None, limit, ArenaRules::Goals(vec![deathmatch(1), deathmatch(2)]));
    arena.activate();
    let n = bots.len();
    let mut lives: Vec<_> = arena.units.iter().map(|&u| crate::gen::F3UnitKey::of(u, arena.w.sim.store.unit.get(u).unwrap())).collect();
    let mut continuous = vec![true; n];
    // [team 1 rifle, team 1 blade, team 2 rifle, team 2 blade]
    let cell = |i: usize| (if bots[i].team == 1.0 { 0 } else { 2 }) + usize::from(bots[i].melee);
    let (mut deaths, mut hits) = ([0u32; 4], [0u32; 4]);
    let mut alive = vec![true; n];
    let (mut first, mut quiet, mut longest) = (None, 0u32, [0u32; 2]);
    let mut still = vec![0u32; n];
    let mut acted = vec![false; n];
    let mut last: Vec<_> = (0..n).map(|i| arena.unit_pos(i)).collect();
    let mut offset = 0;
    let mut ticks = 0;
    while ticks < seconds * 60 && !arena.w.sim.store.battle.over {
        arena.step_one();
        ticks += 1;
        for event in activity(&arena, &mut offset, &lives) {
            match event {
                Activity::Shot(i) => acted[i] = true,
                Activity::Hit(i) => {
                    hits[cell(i)] += 1;
                    acted[i] = true;
                    first.get_or_insert(ticks);
                }
                Activity::Death(i) => {
                    deaths[cell(i)] += 1;
                    continuous[i] = false;
                    still[i] = 0;
                }
            }
        }
        for i in 0..n {
            let body = arena.units[i];
            let unit = arena.w.sim.store.unit.get(body).unwrap();
            let key = crate::gen::F3UnitKey::of(body, unit);
            alive[i] = arena.unit_alive(i);
            continuous[i] &= alive[i] && key == lives[i];
            lives[i] = key;
        }
        if ticks % 60 == 0 {
            for i in 0..n {
                let p = arena.unit_pos(i);
                let (dx, dz) = (p[0] - last[i][0], p[2] - last[i][2]);
                let moved = (dx * dx + dz * dz).sqrt();
                // Stationary for a second of this life without an emitted shot or hostile damage.
                still[i] = if continuous[i] && moved < 0.5 && !acted[i] { still[i] + 1 } else { 0 };
                if still[i] == 6 {
                    quiet += 1;
                }
                let side = usize::from(bots[i].team != 1.0);
                longest[side] = longest[side].max(still[i]);
                last[i] = p;
                acted[i] = false;
                continuous[i] = alive[i];
            }
        }
    }
    let minutes = f64::from(ticks) / 3600.0;
    let per = |v: u32| format!("{:.1}", f64::from(v) / minutes);
    println!(
        "goal-fight {name}: {:.1} s, first hit {:.2} s, winner {:?}; hits landed/min t1 rifle {} blade {}, t2 rifle {} blade {}; deaths/min t1 rifle {} blade {}, t2 rifle {} blade {}; quiet over 5 s: {quiet}, longest t1 {} s t2 {} s",
        f64::from(ticks) / 60.0,
        first.map_or(f64::NAN, |t| f64::from(t) / 60.0),
        arena.w.sim.store.battle.over.then_some(arena.w.sim.store.battle.loser.map(|l| 3.0 - l)),
        per(hits[0]), per(hits[1]), per(hits[2]), per(hits[3]),
        per(deaths[0]), per(deaths[1]), per(deaths[2]), per(deaths[3]),
        longest[0], longest[1],
    );
}

#[test]
#[ignore]
fn goal_fights() {
    let mut mixed = Vec::new();
    for i in 0..6 {
        let z = i as f64 * 3.0 - 7.5;
        mixed.push(bot(1.0, -36.0, z, -std::f64::consts::FRAC_PI_2, i % 2 == 1));
        mixed.push(bot(2.0, 36.0, z, std::f64::consts::FRAC_PI_2, i % 2 == 1));
    }
    report("flat mixed 6v6", &mixed, 6, 180);
    // The same roster in the same order and places, teams exchanged.
    let swapped: Vec<_> = mixed.iter().map(|b| BotSpec { team: 3.0 - b.team, ..*b }).collect();
    report("flat mixed 6v6, teams exchanged", &swapped, 6, 180);
    let rifles: Vec<_> = (0..8).map(|i| bot(1.0 + (i % 2) as f64, if i % 2 == 0 { -40.0 } else { 40.0 }, i as f64 * 3.0 - 10.0, 0.0, false)).collect();
    report("flat rifles 4v4", &rifles, 4, 180);
    let blades: Vec<_> = (0..8).map(|i| bot(1.0 + (i % 2) as f64, if i % 2 == 0 { -15.0 } else { 15.0 }, i as f64 * 3.0 - 10.0, 0.0, true)).collect();
    report("flat blades 4v4", &blades, 4, 180);
    let mut big = Vec::new();
    for i in 0..16 {
        big.push(bot(1.0, -70.0, i as f64 * 4.0 - 30.0, 0.0, i % 4 == 3));
        big.push(bot(2.0, 70.0, i as f64 * 4.0 - 30.0, 0.0, i % 4 == 3));
    }
    report("flat 16v16, quarter blades", &big, 12, 300);
}
