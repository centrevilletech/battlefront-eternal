//! What the goal bots cost per tick at 32, 64 and 128 bots. Run with
//! `cargo test --release -p sim --lib goal_bot_cost -- --ignored --nocapture`.

use super::arena::{Arena, ArenaRules, BotSpec, Op};
use crate::ai::goals::{census, table::StoredArg};
use std::time::Instant;

fn armies(per_team: usize) -> Vec<BotSpec> {
    let mut bots = Vec::new();
    for team in [1.0, 2.0] {
        for i in 0..per_team {
            let x = if team == 1.0 { -60.0 } else { 60.0 };
            let z = (i as f64 - per_team as f64 / 2.0) * 2.5;
            bots.push(BotSpec { team, pos: [x, 0.0, z], yaw: 0.0, hero: false, melee: i % 4 == 3, health: 1.0e9, human: false, mixed: false });
        }
    }
    bots
}

#[test]
#[ignore]
fn goal_bot_cost() {
    let deathmatch = |team| Op::Add { team, kind: "Deathmatch", weight: 1.0, target: StoredArg::None, extra: StoredArg::None };
    for per_team in [16, 32, 64] {
        let mut arena = Arena::build(&armies(per_team), None, 9999, ArenaRules::Goals(vec![deathmatch(1), deathmatch(2)]));
        arena.activate();
        arena.run(120);
        let (pass, scans, ticks) = (census::pass_time(), census::scans(), 1320u32);
        let mut steps = Vec::with_capacity(ticks as usize);
        for _ in 0..ticks {
            let start = Instant::now();
            arena.step_one();
            steps.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        steps.sort_by(f64::total_cmp);
        let mean = steps.iter().sum::<f64>() / f64::from(ticks);
        println!(
            "goal-bot cost: {} bots, goal pass mean {:.4} ms/tick, scans {:.1}/tick, whole tick mean {:.3} median {:.3} p95 {:.3} ms",
            per_team * 2,
            (census::pass_time() - pass).as_secs_f64() * 1000.0 / f64::from(ticks),
            census::scans().wrapping_sub(scans) as f64 / f64::from(ticks),
            mean,
            steps[steps.len() / 2],
            steps[steps.len() * 95 / 100],
        );
    }
}
