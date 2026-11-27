//! Direct dispatch through the same authority that owns the simulation tick.
use bfe_wide::Fnv;
use super::*;
use bfe_content::files::{DirFiles, Files};
use std::{cell::RefCell, rc::Rc, time::{Duration, Instant}};

#[derive(Default)]
struct Trace {
    in_vm: Option<u32>,
    last_return: Option<u32>,
    entries: u32,
    returns: u32,
    samples: Option<Vec<(u32, Duration)>>,
}

thread_local! {
    static TRACE: RefCell<Option<Trace>> = const { RefCell::new(None) };
}

pub(in crate::battle::script) fn enter_vm(tick: u32) {
    TRACE.with_borrow_mut(|trace| {
        if let Some(trace) = trace {
            assert_eq!(trace.in_vm.replace(tick), None);
            trace.entries += 1;
        }
    });
}

pub(in crate::battle::script) fn leave_vm(tick: u32) {
    TRACE.with_borrow_mut(|trace| {
        if let Some(trace) = trace {
            assert_eq!(trace.in_vm.take(), Some(tick));
            trace.last_return = Some(tick);
            trace.returns += 1;
        }
    });
}

fn check_return(tick: u32, ran_callbacks: bool) {
    TRACE.with_borrow(|trace| {
        let trace = trace.as_ref().unwrap();
        assert_eq!(trace.in_vm, None, "VM dispatch still on stack at authority tick {tick}");
        assert_eq!(trace.entries, trace.returns);
        if ran_callbacks {
            assert_eq!(trace.last_return, Some(tick), "callback did not return on the authority thread");
        }
    });
}

/// Collector work of the mission VM, sampled after each authority step.
struct CollectorTicks {
    units: u32,
    tick: u32,
    most: u32,
}

impl CollectorTicks {
    fn new(world: &Root, tick: u32) -> Self {
        Self { units: world.mission_state().unwrap().0.vm.collector_stats().total_units, tick, most: 0 }
    }

    fn step(&mut self, world: &Root, tick: u32) {
        let units = world.mission_state().unwrap().0.vm.collector_stats().total_units;
        let spent = units - self.units;
        let ticks = tick - self.tick;
        assert!(spent <= ticks.saturating_mul(bfe_script::TICK_COLLECTOR_UNITS), "{spent} collector units over {ticks} ticks ending at tick {tick}");
        if ticks == 1 { self.most = self.most.max(spent); }
        (self.units, self.tick) = (units, tick);
    }
}

fn assert_no_memory_refusals(world: &Root) {
    let rules = world.mission_state().unwrap().0;
    assert!(!rules.host().diagnostics.errors.iter().any(|e| e.kind == DiagnosticKind::MemoryLimit),
        "a stock mission reached the memory limit");
}

fn assert_healthy(telemetry: &ScriptTelemetryHandle) {
    let state = telemetry.read();
    assert_eq!((state.callback_budget_refusals, state.callbacks_dropped, state.callback_errors, state.refused_writes), (0, 0, 0, 0));
    assert_eq!((state.queue_overflows, state.refused_commands), (0, 0));
    assert!(!state.faulted);
}

#[test]
#[ignore = "requires prepared 2005 content and a full sustained-fire replay"]
fn direct_space_callbacks_and_scores_for_300000_ticks() {
    TRACE.replace(Some(Trace::default()));
    let (mut world, _, telemetry) = prepared_world(space_mission(), 616, 8);
    tick_world(&mut world, 0.0);
    let sections = capital_sections(&world);
    let mut previous = telemetry.read().callbacks_executed;
    let mut collector = CollectorTicks::new(&world, telemetry.read().tick);
    for tick in 1..=300_000 {
        for (_, slot) in sections.iter().flatten() {
            damage_building(&mut world, *slot, 2.0, Some(Attacker::Null));
        }
        tick_world(&mut world, 1.0 / 60.0);
        let state = telemetry.read();
        check_return(state.tick, state.callbacks_executed != previous);
        collector.step(&world, state.tick);
        previous = state.callbacks_executed;
        assert_eq!(state.scores[1..3], [0, 0], "score changed at tick {tick}");
        assert!(!world.sim.store.battle.over, "early result at tick {tick}");
    }
    assert_healthy(&telemetry);
    assert_no_memory_refusals(&world);
    assert_eq!(previous, 4_131_110);
    let trace = TRACE.take().unwrap();
    assert!(trace.returns > 0);
    println!("Space: 300000 ticks, {previous} callbacks, {} synchronous dispatches, scores 0-0, at most {} collector units in a tick",
        trace.returns, collector.most);
}

struct SharedSpawn {
    files: DirFiles,
    path: String,
    bytes: Rc<[u8]>,
}

impl Files for SharedSpawn {
    fn read(&self, path: &str) -> Option<Rc<[u8]>> {
        if path.trim_start_matches('/') == self.path {
            Some(Rc::clone(&self.bytes))
        } else {
            self.files.read(path)
        }
    }
}

// Let the admitted heroes meet at one post. All combatants, weapons, terrain and
// the post's spawn positions are supplied by the selected prepared mission.
fn shared_spawn(mission: FlatMission) -> SharedSpawn {
    let files = DirFiles::from_resolved_root(prepared_root(), bfe_family::GameFamily::Swbf2);
    let path = format!("maps/{}/map.json", mission.map);
    let mut map: serde_json::Value = serde_json::from_slice(&files.read(&path).unwrap()).unwrap();
    let layers = map["missions"][mission.entry]["layers"].as_array().unwrap();
    let posts = layers.iter().flat_map(|layer| {
        map["layers"][layer.as_str().unwrap()]["commandPosts"].as_array().into_iter().flatten()
    });
    let paths: Vec<_> = posts.filter_map(|post| {
        let team = post["team"].as_u64()?;
        (team == 1 || team == 2).then(|| (team, post["spawnPath"].as_str().unwrap().to_owned()))
    }).collect();
    let (_, shared) = paths.iter().find(|(team, _)| *team == 1).unwrap();
    let nodes = map["spawns"][shared].clone();
    assert!(nodes.as_array().is_some_and(|nodes| !nodes.is_empty()));
    for (_, path) in paths {
        if let Some(spawn) = map["spawns"].get_mut(&path) { *spawn = nodes.clone(); }
        for layer in map["layers"].as_object_mut().unwrap().values_mut() {
            if let Some(spawn) = layer["spawns"].get_mut(&path) { *spawn = nodes.clone(); }
        }
    }
    SharedSpawn { files, path, bytes: serde_json::to_vec(&map).unwrap().into() }
}

#[test]
#[ignore = "requires prepared 2005 mission content in BFE_SWBF2_PREPARED and a full Hero Assault battle"]
fn direct_hero_assault_seed_911() {
    const MAX_TICKS: u32 = 120_000;
    let mission = FlatMission { map: "tat2", era: "gcw", mode: "eli", entry: "tat2g_eli", prepared: true };
    let files = shared_spawn(mission);
    TRACE.replace(Some(Trace::default()));
    let (mut world, admitted, telemetry) = prepared_world_from(&files, mission, 911, 16);
    let target = world.event_rules().unwrap().score_limit().expect("Hero Assault score limit");
    assert_eq!(target, 180);
    let mut clock = WorldClockState { hz: 60.0, max_steps: 4.0, max_debt: 8.0, budget: f64::INFINITY, ..Default::default() };
    let mut last_scores = [0, 0];
    let mut history = Fnv::new();
    let mut changes = 0;
    let mut callbacks = 0;
    let mut reached_tick = None;
    let mut first_winner = None;
    let mut finish_tick = None;
    let mut collector = CollectorTicks::new(&world, telemetry.read().tick);
    for tick in 1..=MAX_TICKS {
        step_combat(&mut world, admitted.units, tick, &mut clock);
        let state = telemetry.read();
        check_return(state.tick, state.callbacks_executed != callbacks);
        collector.step(&world, state.tick);
        callbacks = state.callbacks_executed;
        let scores = [state.scores[1], state.scores[2]];
        let mut kills = [0.0; 3];
        for (_, character) in world.sim.store.character.iter() {
            if let Some(count) = kills.get_mut(character.team as usize) {
                *count += character.kills - character.team_kills;
            }
        }
        assert_eq!(scores.map(f64::from), kills[1..3], "score and enemy kills at tick {tick}");
        if scores != last_scores {
            for n in [tick, scores[0], scores[1]] {
                history.bytes(&n.to_le_bytes());
            }
            last_scores = scores;
            changes += 1;
        }
        if reached_tick.is_none() && (scores[0] >= target || scores[1] >= target) {
            reached_tick = Some(tick);
            first_winner = match (scores[0] >= target, scores[1] >= target) {
                (true, false) => Some(1usize),
                (false, true) => Some(2usize),
                _ => None,
            };
        }
        if world.sim.store.battle.over {
            assert!(reached_tick.is_some(), "early battle result at tick {tick} with scores {scores:?}");
            finish_tick = Some(tick);
            break;
        }
    }
    let finish = finish_tick.expect("Hero Assault match must reach MissionVictory");
    let reached = reached_tick.expect("a team must reach the score limit");
    assert!(finish > reached, "victory delay must separate target tick {reached} from result tick {finish}");
    assert_healthy(&telemetry);
    assert_no_memory_refusals(&world);
    let loser = match world.sim.store.battle.loser {
        Some(1.0) => 1usize,
        Some(2.0) => 2usize,
        other => panic!("finished Hero Assault must name team 1 or 2 as loser, got {other:?}"),
    };
    let winner = 3 - loser;
    assert!(
        last_scores[winner - 1] >= target,
        "winner {winner} finished below score target {target}: {last_scores:?}"
    );
    if let Some(first) = first_winner {
        assert_eq!(winner, first, "team {first} reached {target} first, but team {loser} lost");
    }
    assert_eq!(
        world.mission_state().unwrap().0.host().outcome,
        Some(BattleOutcome::Victory(winner as i32))
    );
    assert!(changes > 0);
    assert!(
        callbacks >= last_scores[0] + last_scores[1] + 1,
        "expected callbacks for scored points plus victory timer (got {callbacks} for {last_scores:?})"
    );
    let trace = TRACE.take().unwrap();
    assert_eq!(trace.entries, trace.returns);
    assert_eq!(trace.in_vm, None);
    assert!(trace.returns > 0);
    let history = history.finish();
    println!("Hero Assault: {}-{} loser={loser} at tick {finish} (target at {reached}), {callbacks} callbacks, {} synchronous dispatches, at most {} collector units in a tick, changes={changes} history={history:016x}",
        last_scores[0], last_scores[1], trace.returns, collector.most);
}

pub(in crate::battle::script) fn dispatch_start() -> Option<Instant> {
    TRACE.with_borrow(|trace| trace.as_ref()?.samples.as_ref().map(|_| Instant::now()))
}

pub(in crate::battle::script) fn measure_dispatch(start: Option<Instant>, work: u32) {
    if let Some(start) = start {
        let elapsed = start.elapsed();
        TRACE.with_borrow_mut(|trace| {
            trace.as_mut().unwrap().samples.as_mut().unwrap().push((work, elapsed));
        });
    }
}

#[test]
#[ignore = "requires prepared 2005 mission content in BFE_SWBF2_PREPARED and an idle native release build"]
fn ordinary_gameplay_dispatches_fit_352_microseconds() {
    assert!(!cfg!(debug_assertions), "measure the optimized authority");
    let streams: [(&str, usize, fn()); 4] = [
        ("hero bots", 8, owner_hero_bots_win_and_loss),
        ("busy hero combat", 38, retail_tat2g_eli_live_combat_busy_win_and_determinism),
        ("space victories", 220, prepared_space_both_sides_reach_the_target_deterministically),
        ("sustained shield fire", 605, prepared_downed_capital_shield_under_sustained_fire),
    ];
    let mut largest = Duration::ZERO;
    for (name, min_dispatches, play) in streams {
        let mut best: Vec<(u32, Duration)> = Vec::new();
        for replay in 0..7 {
            TRACE.replace(Some(Trace { samples: Some(Vec::with_capacity(min_dispatches)), ..Default::default() }));
            play();
            let trace = TRACE.take().unwrap();
            assert_eq!(trace.entries, trace.returns);
            assert_eq!(trace.in_vm, None);
            let samples = trace.samples.unwrap();
            assert_eq!(samples.len() as u32, trace.returns, "{name} replay {replay}");
            if replay == 0 {
                assert!(
                    samples.len() >= min_dispatches,
                    "{name} recorded only {} dispatches (expected at least {min_dispatches})",
                    samples.len()
                );
                assert!(samples.iter().any(|&(work, _)| work > 0), "{name} recorded no script work");
                best = samples;
            } else {
                assert_eq!(
                    samples.len(),
                    best.len(),
                    "dispatch count changed in {name} replay {replay}"
                );
                for ((work, elapsed), (new_work, new_elapsed)) in best.iter_mut().zip(samples) {
                    assert_eq!(*work, new_work, "dispatch work changed in {name} replay {replay}");
                    *elapsed = (*elapsed).min(new_elapsed);
                }
            }
        }
        let maximum = best.iter().map(|&(_, time)| time).max().unwrap();
        println!("{name}: {} dispatches, slowest seven-replay minimum {maximum:?}", best.len());
        largest = largest.max(maximum);
    }
    assert!(largest <= Duration::from_micros(352), "largest ordinary gameplay dispatch was {largest:?}");
}
