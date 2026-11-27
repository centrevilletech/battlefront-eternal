//! Goal bots allocate nothing once a battle is warm: the battle tail, where
//! they run, is counted with this test binary's allocator.

use super::phase::{ALLOCATIONS, COUNTING};
use crate::ai::goals::tests::arena::{Arena, ArenaRules, BotSpec, Op};
use crate::ai::goals::table::StoredArg;
use crate::battle::f4_tail;
use std::cell::Cell;

fn armies(per_team: usize) -> Vec<BotSpec> {
    let mut bots = Vec::new();
    for team in [1.0, 2.0] {
        for i in 0..per_team {
            let x = if team == 1.0 { -40.0 } else { 40.0 };
            let z = (i as f64 - per_team as f64 / 2.0) * 3.0;
            bots.push(BotSpec { team, pos: [x, 0.0, z], yaw: 0.0, hero: false, melee: i % 4 == 3, health: 1.0e9, human: false, mixed: false });
        }
    }
    bots
}

fn tail_allocations(rules: ArenaRules) -> usize {
    let mut arena = Arena::build(&armies(16), None, 9999, rules);
    arena.activate();
    arena.run(600);
    let mut counted = 0;
    for _ in 0..600 {
        let before = ALLOCATIONS.with(Cell::get);
        arena.step_tail_with(|w| {
            COUNTING.with(|c| c.set(true));
            f4_tail::f4_tail_in(w);
            COUNTING.with(|c| c.set(false));
        });
        counted += ALLOCATIONS.with(Cell::get) - before;
    }
    counted
}

#[test]
fn thirty_two_goal_bots_allocate_nothing_in_warm_ticks() {
    let deathmatch = |team| Op::Add { team, kind: "Deathmatch", weight: 1.0, target: StoredArg::None, extra: StoredArg::None };
    let with_goals = tail_allocations(ArenaRules::Goals(vec![deathmatch(1), deathmatch(2)]));
    let without = tail_allocations(ArenaRules::NoTable);
    assert_eq!((with_goals, without), (0, 0));
}

/// Allocations of the battle tail in each of passes one to six, with only
/// half the admitted roster shown to passes one to three.
fn first_passes(rules: ArenaRules, team_size: usize) -> Vec<usize> {
    let mut arena = Arena::build(&armies(team_size), None, 9999, rules);
    arena.activate();
    let full = arena.w.ai.battle().unwrap().runtime.core.character_count;
    arena.w.ai.battle_mut().unwrap().runtime.core.character_count = full / 2;
    f4_tail::select_rules(&mut arena.w, bfe_family::GameFamily::Swbf2, "arena", Default::default(), vec![]).unwrap();
    arena.w.ai.battle_mut().unwrap().runtime.core.character_count = full;
    let mut counts = Vec::new();
    for pass in 0..6 {
        let before = ALLOCATIONS.with(Cell::get);
        arena.step_tail_with(|w| {
            let core = &mut w.ai.battle_mut().unwrap().runtime.core;
            let full = core.character_count;
            if pass < 3 {
                core.character_count = full / 2;
            }
            COUNTING.with(|c| c.set(true));
            f4_tail::f4_tail_in(w);
            COUNTING.with(|c| c.set(false));
            w.ai.battle_mut().unwrap().runtime.core.character_count = full;
        });
        counts.push(ALLOCATIONS.with(Cell::get) - before);
    }
    counts
}

#[test]
fn goal_bots_reserve_at_installation_and_allocate_in_no_pass() {
    let deathmatch = |team| Op::Add { team, kind: "Deathmatch", weight: 1.0, target: StoredArg::None, extra: StoredArg::None };
    for size in [16, 32] {
        let with_goals = first_passes(ArenaRules::Goals(vec![deathmatch(1), deathmatch(2)]), size);
        let without = first_passes(ArenaRules::NoTable, size);
        assert_eq!(with_goals, [0; 6]);
        assert_eq!(with_goals, without);
    }
}

#[test]
fn deathmatch_with_planning_graph_allocates_nothing_in_warm_ticks() {
    use crate::ai::nav::swbf2::{
        graph::{Swbf2ArcInput, Swbf2HubInput},
        BarrierToggles, Swbf2Graph, Swbf2Nav,
    };
    let deathmatch = |team| Op::Add { team, kind: "Deathmatch", weight: 1.0, target: StoredArg::None, extra: StoredArg::None };
    let mut arena = Arena::build(&armies(16), None, 9999, ArenaRules::Goals(vec![deathmatch(1), deathmatch(2)]));
    let hubs = vec![
        Swbf2HubInput { name: Some("h0".into()), position: [-40.0, 0.0, 0.0], radius: 6.0, slots: vec![0.0, 1.0], weight_counts: [0; 5], weights: Vec::new() },
        Swbf2HubInput { name: Some("h1".into()), position: [0.0, 0.0, 30.0], radius: 6.0, slots: vec![0.0, 2.0], weight_counts: [0; 5], weights: Vec::new() },
        Swbf2HubInput { name: Some("h2".into()), position: [40.0, 0.0, 0.0], radius: 6.0, slots: vec![1.0], weight_counts: [0; 5], weights: Vec::new() },
    ];
    let arcs = vec![
        Swbf2ArcInput { name: None, start: 0.0, end: 1.0, filter_flags: 1, type_flags: 0 },
        Swbf2ArcInput { name: None, start: 1.0, end: 2.0, filter_flags: 1, type_flags: 0 },
    ];
    let graph = Swbf2Graph::new(1, &hubs, &arcs, None);
    let swbf2 = Swbf2Nav::new(Some(graph), BarrierToggles::default(), 0);
    arena.w.ai.battle_mut().unwrap().state.navigation.set_swbf2(Box::new(swbf2), 64, 0);
    arena.activate();
    arena.run(600);
    let mut counted = 0;
    let mut roaming_bot_ticks = 0usize;
    for _ in 0..1800 {
        let before = ALLOCATIONS.with(Cell::get);
        arena.step_tail_with(|w| {
            COUNTING.with(|c| c.set(true));
            f4_tail::f4_tail_in(w);
            COUNTING.with(|c| c.set(false));
        });
        counted += ALLOCATIONS.with(Cell::get) - before;
        roaming_bot_ticks += (0..32).filter(|&i| arena.bots().unwrap().test_roam_dest(i).is_some()).count();
    }
    assert!(roaming_bot_ticks > 10_000, "expected active 2005 Deathmatch roaming, got {roaming_bot_ticks} bot-ticks");
    assert_eq!(counted, 0, "warm 2005 Deathmatch ticks with planning graph must not allocate");
}
