use super::{allocations, mix, PublicationSlot, Runtime, CLOCK_START_MS, CLOCK_UNIX_MS};
use bfe_client::local::launch::Prepared;
use bfe_sim::abi::context::Context;
use bfe_sim::world::admit::mission::{MissionPreferences, MissionSelection};
use bfe_sim::world::admit::{Bots, Options as WorldOptions};
use std::path::Path;
use std::time::Instant;

fn run_mission(root: &Path, spec: &str, ticks: usize, seed: u32) {
    let mut parts = spec.split(':');
    let map = parts.next().expect("map").to_string();
    let era = parts.next().expect("era").to_string();
    let mode = parts.next().expect("mode").to_string();
    let side = if era == "cw" { "rep" } else { "all" };
    let space = mode == "ass";

    let selection = MissionSelection {
        family: bfe_family::GameFamily::Swbf2,
        map: map.clone(),
        era: era.clone(),
        mode: mode.clone(),
        seed,
    };
    let options = WorldOptions {
        family: selection.family,
        map: map.clone(),
        mode: mode.clone(),
        side: side.into(),
        seed,
        bots: Bots::read("classic", "medium", "full"),
        ..Default::default()
    };
    let preferences = MissionPreferences {
        bots_per_team: space.then_some(32),
        direct_spawn: space,
        ..Default::default()
    };

    let context = Context::new();
    let (prepared, journal) =
        Prepared::developer_mission_in(context.clone(), root, &options, &selection, preferences)
            .unwrap_or_else(|e| panic!("admit {spec}: {e}"));
    let mut slot = PublicationSlot::new_preallocated();
    let mut runtime = Runtime::new_into_slot(
        *prepared.engine,
        *prepared.admission,
        CLOCK_START_MS,
        CLOCK_UNIX_MS,
        &mut slot,
    )
    .unwrap_or_else(|e| panic!("runtime {spec}: {e}"));

    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    hash = mix(hash, journal.resources.admission_digest.as_bytes());
    let mut times = Vec::with_capacity(ticks);
    let mut steady_allocations = 0u64;
    let print_every = if ticks > 6000 { 3600 } else { 600 };

    for tick in 1..=ticks {
        let before = allocations();
        let started = Instant::now();
        runtime
            .step_into_slot(&mut slot)
            .unwrap_or_else(|e| panic!("step {spec} tick {tick}: {e}"));
        times.push(started.elapsed().as_secs_f64() * 1000.0);
        if tick > 60 {
            steady_allocations += allocations() - before;
        }
        let score = slot.mission_score.expect("mission score");
        assert!(!score.faulted, "mission callback fault on {spec} tick {tick}");

        hash = mix(hash, &slot.bytes);
        for word in slot.block.words.iter().chain(&slot.block.heap) {
            hash = mix(hash, &word.to_bits().to_le_bytes());
        }
        for team in score.teams {
            hash = mix(hash, &team.to_le_bytes());
        }
        hash = mix(hash, &[u8::from(score.over), u8::from(score.finished)]);

        if tick % print_every == 0 || tick == ticks {
            let rules = context.owned(|w| w.mission_digest().unwrap()).unwrap();
            println!(
                "digest2 {spec} tick {tick} pub={hash:016x} rules={rules:016x} score={:?} over={}",
                score.teams, score.over
            );
        }
    }

    let mean = times.iter().sum::<f64>() / times.len() as f64;
    let mut worst: Vec<usize> = (0..times.len()).collect();
    worst.sort_by(|a, b| times[*b].total_cmp(&times[*a]));
    for &at in worst.iter().take(5) {
        println!("worst2 {spec} tick {} {:.3} ms", at + 1, times[at]);
    }
    times.sort_by(f64::total_cmp);
    let p95 = times[(times.len() - 1) * 95 / 100];
    let p99 = times[(times.len() - 1) * 99 / 100];
    let max = times[times.len() - 1];
    println!(
        "cost2 {spec} ticks {ticks} mean {mean:.3} p95 {p95:.3} p99 {p99:.3} max {max:.3} ms; allocations per tick {:.1}",
        steady_allocations as f64 / (ticks.saturating_sub(60).max(1)) as f64
    );
    runtime.close();
}

#[test]
#[ignore = "requires prepared Battlefront II content"]
fn swbf2_world_digest() {
    let Ok(root) = std::env::var("BFE_SWBF2_PREPARED") else {
        println!("skipping swbf2_world_digest: BFE_SWBF2_PREPARED not set");
        return;
    };
    let specs =
        std::env::var("BFE_DIGEST_SWBF2").unwrap_or_else(|_| "tat2:gcw:eli,spa6:cw:ass".into());
    let ticks = std::env::var("BFE_DIGEST_TICKS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);
    let seed = std::env::var("BFE_DIGEST_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    for spec in specs.split(',').filter(|s| !s.is_empty()) {
        run_mission(Path::new(&root), spec, ticks, seed);
    }
}
