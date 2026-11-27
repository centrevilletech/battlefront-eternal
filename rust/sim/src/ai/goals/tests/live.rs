//! Goal bots in a live native battle.

use super::arena::{Arena, ArenaRules, BotSpec, BuildingSpec, Op};
use crate::ai::control::controller::{goal, SquadCommand};
use crate::ai::control::squads;
use crate::ai::control::goal_orders::{is_goal_attack, is_goal_base};
use crate::ai::goals::{census, table::StoredArg, GoalKind};
use crate::battle::{f4_tail, rules::ModeParameters};
use bfe_family::GameFamily;

fn deathmatch(team: u8, weight: f32) -> Op {
    Op::Add { team, kind: "Deathmatch", weight, target: StoredArg::None, extra: StoredArg::None }
}

fn rifle(team: f64, x: f64, z: f64, yaw: f64) -> BotSpec {
    BotSpec { team, pos: [x, 0.0, z], yaw, hero: false, melee: false, health: 250.0, human: false, mixed: false }
}

/// A blade fighter of an ordinary class: the 2005 game adds Hero Assault's
/// roster as ordinary classes, and the released targeting never starts a
/// fight between two units flagged as heroes.
fn blade(team: f64, x: f64, z: f64, yaw: f64) -> BotSpec {
    BotSpec { team, pos: [x, 0.0, z], yaw, hero: false, melee: true, health: 400.0, human: false, mixed: false }
}

/// A bot of the neutral team, which has no goals: it stands where it is.
fn bystander(x: f64, z: f64) -> BotSpec {
    BotSpec { team: 0.0, pos: [x, 0.0, z], yaw: 0.0, hero: false, melee: false, health: 250.0, human: false, mixed: false }
}

/// Three rifles per team, 72 m apart, facing away from each other.
fn facing_away() -> Vec<BotSpec> {
    let mut bots = Vec::new();
    for i in 0..3 {
        let z = i as f64 * 4.0 - 4.0;
        bots.push(rifle(1.0, -36.0, z, -std::f64::consts::FRAC_PI_2));
        bots.push(rifle(2.0, 36.0, z, std::f64::consts::FRAC_PI_2));
    }
    bots
}

fn two_deathmatch_teams() -> ArenaRules { ArenaRules::Goals(vec![deathmatch(1, 100.0), deathmatch(2, 100.0)]) }

fn goal_orders(arena: &Arena, bots: usize) -> usize {
    (0..bots)
        .filter(|&i| {
            let c = arena.controller(i);
            let tagged = |o: &crate::ai::control::controller::SourceGoal| is_goal_base(o) || is_goal_attack(o);
            tagged(&c.commands.current) || c.commands.queue.iter().any(tagged)
        })
        .count()
}

#[test]
fn deathmatch_bots_find_each_other_and_fight_to_a_win() {
    let mut arena = Arena::build(&facing_away(), None, 3, two_deathmatch_teams());
    arena.activate();
    let start: Vec<_> = (0..6).map(|i| arena.unit_pos(i)).collect();
    let mut ticks = 0;
    while !arena.w.sim.store.battle.over && ticks < 60 * 90 {
        arena.step_one();
        ticks += 1;
    }
    assert!(arena.w.sim.store.battle.over, "no winner after {ticks} ticks");
    let moved = (0..6).filter(|&i| {
        let (a, b) = (start[i], arena.unit_pos(i));
        ((a[0] - b[0]) * (a[0] - b[0]) + (a[2] - b[2]) * (a[2] - b[2])).sqrt() > 3.0
    });
    assert!(moved.count() >= 4, "the bots walked toward each other");
}

#[test]
fn two_runs_give_identical_bits() {
    let run = || {
        let mut arena = Arena::build(&facing_away(), None, 3, two_deathmatch_teams());
        arena.activate();
        let mut bits = Vec::new();
        for tick in 0..900 {
            arena.step_one();
            if tick % 30 == 0 {
                bits.extend(arena.snapshot_bits());
            }
        }
        bits
    };
    assert_eq!(run(), run());
}

#[test]
fn a_conquest_battle_never_enters_the_goal_bots() {
    let before = census::entries();
    let mut arena = Arena::build(&facing_away(), None, 3, ArenaRules::Conquest);
    arena.activate();
    arena.run(600);
    assert!(arena.bots().is_none());
    assert_eq!(census::entries(), before);
}

#[test]
fn rules_without_a_goal_table_get_no_goal_bots() {
    let before = census::entries();
    let mut arena = Arena::build(&facing_away(), None, 3, ArenaRules::NoTable);
    let family_phase: fn(&mut crate::world::owner::World) -> u32 = crate::ai::owned::entry::native_phase_in::<true>;
    assert!(std::ptr::fn_addr_eq(arena.w.battle.bot_phase, family_phase));
    arena.activate();
    arena.run(300);
    assert!(arena.bots().is_none());
    assert_eq!(census::entries(), before);
}

fn assert_released(arena: &Arena, bots: usize) {
    assert_eq!(goal_orders(arena, bots), 0);
    for i in 0..bots {
        let c = arena.controller(i);
        assert!(c.fsm.state == 0.0 && c.agent.is_none(), "bot {i} is freshly bound");
    }
}

#[test]
fn a_live_tail_reset_hands_every_goal_bot_back_to_the_released_controller() {
    let mut arena = Arena::build(&facing_away(), None, 99, two_deathmatch_teams());
    arena.activate();
    arena.run(90);
    assert!(arena.bots().is_some());
    assert_eq!(goal_orders(&arena, 6), 6);
    arena.reinit_tail();
    assert!(arena.bots().is_none());
    assert_released(&arena, 6);
    let before = census::entries();
    arena.run(120);
    assert_eq!(census::entries(), before, "the old goal bots never run again");
    assert_eq!(goal_orders(&arena, 6), 0);
}

#[test]
fn live_rule_changes_hand_bots_over_in_both_directions() {
    let mut arena = Arena::build(&facing_away(), None, 99, two_deathmatch_teams());
    arena.activate();
    arena.run(90);
    // Goal rules to the 2004 game's Conquest.
    assert!(f4_tail::select_rules(&mut arena.w, GameFamily::Swbf1, "conquest", Default::default(), Vec::new()).is_ok());
    assert!(arena.bots().is_none());
    assert_released(&arena, 6);
    let before = census::entries();
    arena.run(60);
    assert_eq!(census::entries(), before);
    // Conquest to goal rules: a fresh controller takes the bots.
    arena.ops.borrow_mut().extend([deathmatch(1, 1.0), deathmatch(2, 1.0)]);
    let parameters = ModeParameters { command_limit: 32, score_limit: Some(99), ..Default::default() };
    assert!(f4_tail::select_rules(&mut arena.w, GameFamily::Swbf2, "arena", parameters, Vec::new()).is_ok());
    arena.run(60);
    assert_eq!(goal_orders(&arena, 6), 6);
    // A handler replacement while the bots run: the old controller retires.
    arena.ops.borrow_mut().extend([deathmatch(1, 1.0), deathmatch(2, 1.0)]);
    assert!(f4_tail::select_rules(&mut arena.w, GameFamily::Swbf2, "arena", parameters, Vec::new()).is_ok());
    assert_released(&arena, 6);
    for i in 0..6 {
        assert!(arena.bots().unwrap().goal_of(i).is_none(), "no bot carries an old goal");
    }
    arena.run(60);
    assert_eq!(goal_orders(&arena, 6), 6);
}

/// A bot of team 1 holds on a squad order from the player beside it while
/// its team's goal stays or changes; returns the ticks it spent holding.
fn hold_ticks(change_goal: bool) -> u32 {
    let bots = vec![rifle(1.0, -10.0, 0.0, 0.0), bystander(-7.0, 0.0), rifle(2.0, 140.0, 140.0, 0.0)];
    let mut arena = Arena::build(&bots, None, 99, ArenaRules::Goals(vec![deathmatch(1, 1.0)]));
    arena.activate();
    arena.run(30);
    let time = f64::from(arena.tick) / 60.0;
    {
        let c = arena.controller_mut(0);
        c.command_pending = true;
        c.command_record = Some(SquadCommand { source: Some(1), kind: squads::command::HOLD, time, target: None });
    }
    arena.run(10);
    assert_eq!(arena.controller(0).commands.current.kind, goal::HOLD, "the hold started");
    if change_goal {
        let handle = arena.handles.borrow()[0].unwrap();
        let subject = arena.w.sim.store.battle.characters[1];
        arena.ops.borrow_mut().push(Op::Delete { handle });
        arena.ops.borrow_mut().push(Op::Add {
            team: 1, kind: "Follow", weight: 1.0, target: StoredArg::Character(subject), extra: StoredArg::None,
        });
    }
    let mut held = 0;
    for _ in 0..120 {
        arena.step_one();
        held += u32::from(arena.controller(0).commands.current.kind == goal::HOLD);
    }
    if change_goal {
        let goal = arena.bots().unwrap().goal_of(0).map(|g| g.kind);
        assert_eq!(goal, Some(GoalKind::Follow), "the new goal took over the base order");
    }
    held
}

#[test]
fn a_squad_hold_survives_a_change_of_goal() {
    let unchanged = hold_ticks(false);
    let changed = hold_ticks(true);
    assert!(unchanged >= 110, "held {unchanged}");
    assert_eq!(changed, unchanged);
}

/// One hunter west of a wall the length of the arena, `across` enemies just
/// east of it, and one reachable enemy far up the hunter's side.
fn wall_hunt(across: usize) -> f64 {
    let mut bots = vec![rifle(1.0, -4.0, 0.0, 0.0)];
    for i in 0..across {
        bots.push(rifle(2.0, 4.0 + (i % 3) as f64 * 3.0, (i / 3) as f64 * 4.0 - 4.0, 0.0));
    }
    bots.push(rifle(2.0, -4.0, 125.0, 0.0));
    let reachable = bots.len() - 1;
    let mut arena = Arena::build_with_wall(&bots, None, 99, ArenaRules::Goals(vec![deathmatch(1, 1.0)]), true);
    arena.activate();
    let mut closest = f64::INFINITY;
    let (mut first, mut changes, mut last, mut fighting) = (None, 0, None, 0);
    for tick in 0..60 * 120 {
        arena.step_one();
        let d = arena.distance(0, reachable);
        if d < closest {
            closest = d;
        }
        let hunting = arena.bots().unwrap().hunting(0);
        if hunting != last {
            changes += 1;
            last = hunting;
        }
        if hunting == Some(reachable) {
            first.get_or_insert(tick);
        }
        fighting += u32::from(arena.controller(0).commands.current.kind == goal::COMBAT);
        if closest < 40.0 {
            break;
        }
    }
    println!("wall hunt {across}: first reachable pick {first:?}, {changes} target changes, {fighting} fighting ticks, closest {closest:.1} m");
    closest
}

#[test]
fn a_hunter_gives_up_enemies_behind_a_wall_and_reaches_one_it_can() {
    for across in [1, 2, 9, 12] {
        let closest = wall_hunt(across);
        assert!(closest < 40.0, "{across} enemies across the wall: closest approach {closest:.1} m");
    }
}

#[test]
fn settled_guards_search_nothing_and_hunters_search_on_a_schedule() {
    let mut bots = vec![bystander(0.0, 0.0)];
    for i in 0..16 {
        bots.push(rifle(1.0, -20.0 + (i % 4) as f64 * 2.0, -20.0 + (i / 4) as f64 * 2.0, 0.0));
    }
    for i in 0..16 {
        bots.push(rifle(2.0, 130.0, -30.0 + i as f64 * 4.0, 0.0));
    }
    let defend = |subject| Op::Add { team: 1, kind: "Defend", weight: 1.0, target: StoredArg::Character(subject), extra: StoredArg::None };
    let mut arena = Arena::build(&bots, None, 99, ArenaRules::Goals(Vec::new()));
    let subject = arena.w.sim.store.battle.characters[0];
    arena.ops.borrow_mut().push(defend(subject));
    arena.ops.borrow_mut().push(deathmatch(2, 1.0));
    arena.activate();
    arena.run(60 * 10);
    // Team 2 is far away and out of sight: only its hunters search.
    for i in 1..17 {
        assert!(arena.distance(i, 0) < 8.0, "guard {i} reached its subject");
    }
    let scans = census::scans();
    arena.run(600);
    let searched = census::scans().wrapping_sub(scans);
    // 16 hunters each look over at most 16 enemies at most once every 2 s.
    assert!(searched <= 16 * 16 * 6, "searched {searched}");
    assert!(searched > 0);
}

#[test]
fn guards_without_hunters_search_nothing() {
    let mut bots = vec![bystander(0.0, 0.0)];
    for i in 0..16 {
        bots.push(rifle(1.0, -20.0 + (i % 4) as f64 * 2.0, -20.0 + (i / 4) as f64 * 2.0, 0.0));
    }
    bots.push(rifle(2.0, 140.0, 140.0, 0.0));
    let mut arena = Arena::build(&bots, None, 99, ArenaRules::Goals(Vec::new()));
    let subject = arena.w.sim.store.battle.characters[0];
    arena.ops.borrow_mut().push(Op::Add { team: 1, kind: "Defend", weight: 1.0, target: StoredArg::Character(subject), extra: StoredArg::None });
    let scans = census::scans();
    arena.activate();
    arena.run(1200);
    assert_eq!(census::scans(), scans);
}

#[test]
fn destroy_bots_bring_down_a_named_building_and_the_goal_suspends() {
    let bots: Vec<_> = (0..3).map(|i| rifle(1.0, -30.0, i as f64 * 3.0, 0.0)).collect();
    let building = BuildingSpec { name: "relay_mast", team: 2.0, pos: [25.0, 0.0, 0.0], health: 300.0, targetable: true };
    let target = StoredArg::Name(b"Relay_Mast"[..].into());
    let rules = ArenaRules::Goals(vec![Op::Add { team: 1, kind: "Destroy", weight: 5.0, target, extra: StoredArg::None }, deathmatch(1, 1.0)]);
    let mut arena = Arena::build(&bots, Some(building), 99, rules);
    arena.activate();
    let handle = arena.building_handle.clone().unwrap();
    let mut fallen = None;
    for tick in 0..60 * 60 {
        arena.step_one();
        if crate::battle::f4::frame_in(&arena.w.battle, &handle).health <= 0.0 {
            fallen = Some(tick);
            break;
        }
    }
    assert!(fallen.is_some(), "the building never fell");
    arena.run(60);
    let bots = arena.bots().unwrap();
    assert!(!bots.goals().find(|g| g.kind == GoalKind::Destroy).unwrap().active);
    for i in 0..3 {
        assert_eq!(bots.goal_of(i).map(|g| g.kind), Some(GoalKind::Deathmatch));
    }
}

#[test]
fn a_follower_keeps_up_with_a_guard_that_walks_away() {
    let bots = vec![rifle(1.0, 0.0, 0.0, 0.0), rifle(1.0, 0.0, 3.0, 0.0), bystander(60.0, 20.0), rifle(2.0, 140.0, 140.0, 0.0)];
    let mut arena = Arena::build(&bots, None, 99, ArenaRules::Goals(Vec::new()));
    let (leader, subject) = (arena.w.sim.store.battle.characters[0], arena.w.sim.store.battle.characters[2]);
    arena.ops.borrow_mut().push(Op::Add { team: 1, kind: "Defend", weight: 1.0, target: StoredArg::Character(subject), extra: StoredArg::None });
    arena.ops.borrow_mut().push(Op::Add { team: 1, kind: "Follow", weight: 1.0, target: StoredArg::Character(leader), extra: StoredArg::None });
    arena.activate();
    let mut farthest = 0.0f64;
    for tick in 0..60 * 30 {
        arena.step_one();
        if tick > 120 {
            let d = arena.distance(1, 0);
            if d > farthest {
                farthest = d;
            }
        }
    }
    assert!(arena.distance(0, 2) < 8.0, "the guard reached its subject");
    assert!(arena.distance(1, 0) < 10.0, "the follower is with the guard");
    assert!(farthest < 25.0, "the follower kept up: {farthest:.1} m");
}

#[test]
fn weights_seventy_thirty_split_a_live_team_seven_to_three() {
    let mut bots = vec![bystander(0.0, 0.0)];
    for i in 0..10 {
        bots.push(rifle(1.0, i as f64 * 2.0, -10.0, 0.0));
    }
    bots.push(rifle(2.0, 140.0, 140.0, 0.0));
    let mut arena = Arena::build(&bots, None, 99, ArenaRules::Goals(Vec::new()));
    let subject = arena.w.sim.store.battle.characters[0];
    arena.ops.borrow_mut().push(deathmatch(1, 70.0));
    arena.ops.borrow_mut().push(Op::Add { team: 1, kind: "Defend", weight: 30.0, target: StoredArg::Character(subject), extra: StoredArg::None });
    arena.activate();
    arena.run(30);
    let bots = arena.bots().unwrap();
    let kinds: Vec<_> = (1..11).map(|i| bots.goal_of(i).unwrap().kind).collect();
    assert_eq!(kinds.iter().filter(|k| **k == GoalKind::Deathmatch).count(), 7);
    assert_eq!(kinds.iter().filter(|k| **k == GoalKind::Defend).count(), 3);
}

#[test]
fn hero_buttons_change_once_per_tick() {
    let bots = vec![blade(1.0, -2.0, 0.0, std::f64::consts::FRAC_PI_2), blade(2.0, 2.0, 0.0, -std::f64::consts::FRAC_PI_2)];
    let mut arena = Arena::build(&bots, None, 99, two_deathmatch_teams());
    arena.activate();
    let (mut presses, mut previous) = (0, [0u8; 2]);
    for _ in 0..600 {
        arena.step_one();
        for (i, previous) in previous.iter_mut().enumerate() {
            let Some(input) = take(&mut arena, i) else { continue };
            let (pressed, held) = input.combo_bits();
            assert_eq!(pressed & *previous, 0, "a held button is pressed again");
            assert_eq!(pressed & !held, 0);
            presses += pressed.count_ones();
            *previous = held;
        }
    }
    assert!(presses > 0, "the blades attacked");
    if arena.unit_alive(0) {
        let (key, tick) = (life(&arena, 0), sim_tick(&arena) + 1.0);
        let mut frame = arena.bots().unwrap().input(0, &key, tick).unwrap();
        assert!(arena.w.ai_goal_feedback(&frame, true, 0.75, false));
        assert!(arena.bots().unwrap().input(0, &key, tick).unwrap().guarding);
        frame.tick -= 1.;
        assert!(!arena.w.ai_goal_feedback(&frame, false, 0.75, false));
    }
}

#[test]
fn clearing_a_team_frees_its_bots_and_they_stand() {
    let mut arena = Arena::build(&facing_away(), None, 99, two_deathmatch_teams());
    arena.activate();
    arena.run(120);
    arena.ops.borrow_mut().push(Op::Clear { team: 1 });
    arena.run(30);
    // Even bots are team 1.
    for i in 0..6 {
        let goal = arena.bots().unwrap().goal_of(i).map(|g| g.kind);
        assert_eq!(goal.is_some(), i % 2 == 1, "bot {i}: only team 2 keeps a goal");
    }
    let before: Vec<_> = (0..6).step_by(2).map(|i| arena.unit_pos(i)).collect();
    arena.run(180);
    for (k, i) in (0..6).step_by(2).enumerate() {
        let p = arena.unit_pos(i);
        let (dx, dz) = (p[0] - before[k][0], p[2] - before[k][2]);
        let fighting = arena.controller(i).commands.current.kind == goal::COMBAT;
        assert!(fighting || (dx * dx + dz * dz).sqrt() < 1.0, "bot {i} stands without a goal");
    }
}

#[test]
fn a_blade_in_the_second_slot_is_read_as_the_selected_weapon() {
    use crate::ai::goals::melee::FIRE;
    // A secondary gun sits in slot 0 and the primary blade in slot 1.
    let duelist = |team, x, yaw| BotSpec { team, pos: [x, 0.0, 0.0], yaw, hero: false, melee: true, health: 400.0, human: false, mixed: true };
    let bots = vec![duelist(1.0, -1.5, std::f64::consts::FRAC_PI_2), duelist(2.0, 1.5, -std::f64::consts::FRAC_PI_2)];
    let mut arena = Arena::build(&bots, None, 99, two_deathmatch_teams());
    arena.activate();
    let mut presses = 0;
    for _ in 0..600 {
        arena.step_one();
        for i in 0..2 {
            if let Some(input) = take(&mut arena, i) {
                assert_eq!(input.binding.map(|b| (b.slot, b.channel)), Some((1, 0)), "the primary blade in slot 1");
                presses += (input.combo_bits().0 & FIRE).count_ones();
            }
        }
    }
    assert!(presses > 0, "the blade's attacks reached the input record");
}

#[test]
#[ignore]
fn wall_hunt_trace() {
    let mut bots = vec![rifle(1.0, -4.0, 0.0, 0.0)];
    for i in 0..12 {
        bots.push(rifle(2.0, 4.0 + (i % 3) as f64 * 3.0, (i / 3) as f64 * 4.0 - 4.0, 0.0));
    }
    bots.push(rifle(2.0, -4.0, 125.0, 0.0));
    let mut arena = Arena::build_with_wall(&bots, None, 99, ArenaRules::Goals(vec![deathmatch(1, 1.0)]), true);
    arena.activate();
    for tick in 0..1500 {
        arena.step_one();
        if tick % 15 == 0 {
            let p = arena.unit_pos(0);
            let hunting = arena.bots().unwrap().hunting(0);
            let c = arena.controller(0);
            let agent = c.agent.as_ref().map(|a| (a.fsm.state, a.helper_arrived(), a.helper_failed()));
            println!("t{tick} pos ({:.2},{:.2}) hunting {hunting:?} cur {} done {} agent {agent:?}", p[0], p[2], c.commands.current.kind, c.commands.current_done);
        }
    }
}

/// The fixed tick stepped last; the goal bots' samples are for the next one.
fn sim_tick(arena: &Arena) -> f64 { crate::units::f3::tick_in(&arena.w.units) }

fn life(arena: &Arena, i: usize) -> crate::gen::F3UnitKey {
    let r = arena.units[i];
    crate::gen::F3UnitKey::of(r, arena.w.sim.store.unit.get(r).unwrap())
}

/// The sample the combo machine of bot `i` takes at the next soldier phase.
fn take(arena: &mut Arena, i: usize) -> Option<crate::ai::goals::MeleeInput> {
    let (key, tick) = (life(arena, i), sim_tick(arena) + 1.0);
    arena.w.ai_goal_input(key, tick)
}

#[test]
fn melee_input_steps_every_tick_through_the_loss_of_a_goal() {
    let bots = vec![blade(1.0, -2.0, 0.0, std::f64::consts::FRAC_PI_2), blade(2.0, 2.0, 0.0, -std::f64::consts::FRAC_PI_2), blade(0.0, 30.0, 30.0, 0.0)];
    let mut arena = Arena::build(&bots, None, 99, two_deathmatch_teams());
    arena.activate();
    arena.run(60);
    arena.ops.borrow_mut().push(Op::Clear { team: 1 });
    let (mut previous, mut presses, mut samples) = (0u8, 0, 0);
    for _ in 0..240 {
        arena.step_one();
        if !arena.unit_alive(0) {
            break;
        }
        samples += 1;
        let input = take(&mut arena, 0).expect("a living bot's sample every tick");
        assert_eq!(input.tick, sim_tick(&arena) + 1.0, "made for the next tick");
        assert!(take(&mut arena, 0).is_none(), "a sample is given once");
        let (pressed, held) = input.combo_bits();
        assert_eq!(pressed & previous, 0, "no held button is pressed again");
        presses += pressed.count_ones() + held.count_ones();
        previous = held;
        // The bystander has no goal at all and still has its sample.
        assert!(take(&mut arena, 2).is_some());
    }
    assert!(arena.bots().unwrap().goal_of(0).is_none(), "the bot lost its goal");
    assert!(samples >= 30, "a sample on each of {samples} living ticks");
    println!("melee after the goal was lost: {samples} samples, {presses} pressed or held Fire bits");
    // A sample is never read for another tick, or for another life.
    let (next, key, other) = (sim_tick(&arena) + 1.0, life(&arena, 2), life(&arena, 1));
    let bots = arena.bots().unwrap();
    assert!(bots.input(2, &key, next).is_some());
    assert!(bots.input(2, &key, next - 1.0).is_none() && bots.input(2, &key, next + 1.0).is_none());
    assert!(bots.input(2, &other, next).is_none());
    let later = crate::gen::F3UnitKey { life: key.life + 1.0, ..key };
    assert!(bots.input(2, &later, next).is_none(), "another gameplay life of the same unit slot");
}

#[test]
fn a_blade_on_the_secondary_channel_reads_that_channel_trigger() {
    use crate::ai::goals::melee::FIRE;
    // A primary rifle in slot 0 and a blade on the secondary channel in slot 1.
    let lancer = BotSpec { team: 1.0, pos: [0.0, 0.0, 0.0], yaw: 0.0, hero: false, melee: false, health: 400.0, human: false, mixed: true };
    let bots = vec![lancer, rifle(2.0, 140.0, 140.0, 0.0)];
    let mut arena = Arena::build(&bots, None, 99, two_deathmatch_teams());
    arena.activate();
    arena.run(10);
    let body = arena.units[0];
    let mut seen = Vec::new();
    for tick in 0..7 {
        let down = tick < 4;
        arena.step_tail_with(|w| {
            let controls = &mut w.sim.store.unit.get_mut(body).unwrap().controls;
            controls.was = [0.0, f64::from(down)];
            controls.pressed = vec![tick == 5, tick == 6];
            f4_tail::f4_tail_in(w);
        });
        let input = take(&mut arena, 0).expect("sample");
        assert_eq!(input.binding.map(|b| (b.slot, b.channel)), Some((1, 1)), "the blade in slot 1 on the secondary channel");
        seen.push(input.combo_bits());
    }
    let f = FIRE;
    assert_eq!(seen, vec![(f, f), (0, f), (0, f), (0, f), (0, 0), (0, 0), (f, 0)], "its trigger is logical Fire");
}

#[test]
fn a_facing_blade_guards_against_a_firing_rifle() {
    let gunner = rifle(2.0, 12.0, 0.0, -std::f64::consts::FRAC_PI_2);
    let mut arena = Arena::build(&[blade(1.0, 0.0, 0.0, std::f64::consts::FRAC_PI_2), gunner], None, 99, two_deathmatch_teams());
    arena.activate();
    let [defender, attacker] = [arena.units[0], arena.units[1]];
    arena.step_one();
    arena.step_tail_with(|w| {
        let b = w.ai.battle_mut().unwrap();
        let ci = b.runtime.core.controller_of_character(0).unwrap();
        let enemy = w.sim.store.unit.get_mut(attacker).unwrap();
        enemy.controls.was = [1.0, 0.0];
        b.runtime.core.controllers[ci].target = None;
        let guard = w.sim.store.unit.get_mut(defender).unwrap();
        guard.controls.was = [0.0; 2];
        guard.combat.last_attacker = crate::gen::Attacker::Unit(attacker);
        f4_tail::f4_tail_in(w);
    });
    assert!(take(&mut arena, 0).unwrap().block.pressed);
}

fn dueling_blades() -> Arena {
    let mut fighters = [blade(1., 0., 0., 0.), blade(2., 0., 1.5, 3.)];
    for fighter in &mut fighters { fighter.health = 1.0e9; }
    let mut arena = Arena::build(&fighters, None, 99, two_deathmatch_teams());
    arena.activate();
    arena
}

fn duel_sample(arena: &mut Arena, yaw: f64) -> crate::ai::goals::MeleeInput {
    use crate::ai::{control::world::EntRef, owned::entities::Entity};
    let bodies = [arena.units[0], arena.units[1]];
    arena.step_tail_with(|w| {
        let b = w.ai.battle_mut().unwrap();
        let ci = b.runtime.core.controller_of_character(0).unwrap();
        for (body, z, angle) in [(bodies[0], 0., yaw), (bodies[1], 1.5, 3.)] {
            let unit = w.sim.store.unit.get_mut(body).unwrap();
            unit.controls.pressed.fill(false);
            unit.controls.was = [0.; 2];
            let crate::gen::UnitBody::Soldier(s) = &mut unit.soldier else { unreachable!() };
            s.aim_yaw = angle;
            s.position = [0., 0., z];
            s.previous = s.position;
            s.settled = s.position;
        }
        let target = b.state.entities.find(Entity::Unit(bodies[1])).unwrap();
        let ctl = &mut b.runtime.core.controllers[ci];
        ctl.target = Some(EntRef { unit: target, generation: w.sim.store.unit.get(bodies[1]).unwrap().generation });
        ctl.commands.current.kind = goal::COMBAT;
        ctl.commands.current_done = false;
        f4_tail::f4_tail_in(w);
    });
    take(arena, 0).unwrap()
}

#[test]
fn a_blade_under_pressure_saves_energy_and_answers_with_a_strike() {
    let mut arena = dueling_blades();
    let mut attacks = 0;
    let mut late_blocks = 0;
    for t in 0..123 {
        let frame = duel_sample(&mut arena, 0.);
        if t > 10 {
            late_blocks += usize::from(frame.block.pressed);
            attacks += usize::from(frame.fire.pressed);
        }
        assert!(arena.w.ai_goal_feedback(&frame, false, if t < 5 { 100. } else { 30. }, false));
    }
    assert!(attacks >= 3, "attacks with energy held in reserve: {attacks}");
    assert_eq!(late_blocks, 0, "a tired fighter kept asking for guard");
}

#[test]
fn the_fighter_turns_toward_a_target_behind_it_before_starting_its_swing() {
    let mut arena = dueling_blades();
    for _ in 0..35 {
        let frame = duel_sample(&mut arena, std::f64::consts::PI);
        assert!(!frame.fire.held && !frame.fire.pressed);
    }
    assert!(duel_sample(&mut arena, 0.).fire.pressed);
}

#[test]
fn soldier_callback_delivers_each_life_once_and_validates_equipped_feedback() {
    let mut arena = Arena::build(&[blade(1., -3., 0., 1.), blade(2., 3., 0., -1.)], None, 99, two_deathmatch_teams());
    arena.activate();
    arena.run(10);
    let key = life(&arena, 0);
    let tick = sim_tick(&arena) + 1.;
    let observed = arena.bots().unwrap().input(0, &key, tick).unwrap();
    assert!(!arena.w.ai_goal_feedback(&observed, true, 1., false));
    let first = crate::units::f3::with_services_in(&mut arena.w, |w, services| {
        assert!(services.unwrap().ai.is_some());
        let frame = w.ai_goal_input(key.clone(), tick).unwrap();
        assert!(frame.binding.is_some());
        assert!(w.ai_goal_input(key.clone(), tick).is_none());
        frame
    });
    let body = arena.units[0];
    arena.w.sim.store.unit.get_mut(body).unwrap().generation += 1.;
    let fresh = life(&arena, 0);
    assert!(!arena.w.ai_goal_feedback(&first, true, 0., true));
    assert!(arena.w.ai_goal_input(key, tick).is_none());
    let birth = arena.w.ai_goal_input(fresh.clone(), tick).unwrap();
    assert_eq!(birth.combo_bits(), (0, 0));
    assert!(birth.binding.is_some());
    assert!(arena.w.ai_goal_input(fresh.clone(), tick).is_none());
    arena.w.sim.store.unit.get_mut(body).unwrap().combat.weapons.clear();
    assert!(!arena.w.ai_goal_feedback(&birth, true, 0., true));
    arena.w.sim.store.unit.get_mut(body).unwrap().alive = false;
    assert!(arena.w.ai_goal_input(fresh, tick).is_none());
}

#[test]
fn the_soldier_phase_steps_all_living_bots_and_returns_committed_guard() {
    use super::super::melee::MeleeReport;
    use std::{cell::RefCell, rc::Rc};
    let mut arena = Arena::build(&[blade(1., -3., 0., 1.), bystander(40., 20.)], None, 99, two_deathmatch_teams());
    arena.activate();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let calls = seen.clone();
    arena.w.register_bot_melee_driver(Box::new(move |w, input| {
        assert!(w.ai.battle().is_none());
        calls.borrow_mut().push((input.key.slot, input.tick as u32, input.binding.is_some(), input.guarding));
        Ok(input.binding.map(|_| MeleeReport { guarding: true, energy: 0.75, exhausted: false }))
    }));
    arena.run(3);
    let rows = seen.borrow();
    assert_eq!(rows.len(), 6);
    assert!(rows.contains(&(arena.units[0].slot, 1, true, false)));
    assert!(rows.contains(&(arena.units[0].slot, 2, true, true)));
    assert!(rows.contains(&(arena.units[1].slot, 3, false, false)));
}

#[test]
fn holding_the_trigger_when_switching_blades_presses_the_new_binding() {
    use crate::ai::goals::melee::{Binding, BlockMode, MeleeInput, Wanted};
    use crate::store::DefId;
    let mut a = Arena::build(&[rifle(1., 0., 0., 0.)], None, 1, ArenaRules::Conquest);
    a.activate();
    let key = crate::gen::F3UnitKey::of(a.units[0], a.w.sim.store.unit.get(a.units[0]).unwrap());
    let mut frame = MeleeInput::new(1, key, 0);
    for channel in [0, 1] {
        let binding = Some(Binding { slot: channel as usize, weapon: DefId(3), channel });
        frame.step(1. + f64::from(channel), 1. / 60., Wanted { binding, fire: true, ..Wanted::default() }, BlockMode::Toggle);
        assert!(frame.fire.pressed && frame.fire.held);
    }
    frame.step(3., 1. / 60., Wanted::default(), BlockMode::Toggle);
    assert!(frame.fire.released && !frame.fire.held);
}

#[test]
fn event_soldiers_without_an_assignment_table_still_drive_their_blade() {
    let mut roster = facing_away();
    roster[0].melee = true;
    let mut arena = Arena::build(&roster, None, 3, ArenaRules::NoTable);
    arena.activate();
    let key = crate::gen::F3UnitKey::of(arena.units[0], arena.w.sim.store.unit.get(arena.units[0]).unwrap());
    for _ in 0..4 {
        let body = arena.units[0];
        arena.step_tail_with(|w| {
            w.sim.store.unit.get_mut(body).unwrap().controls.was[0] = 1.;
            crate::battle::f4_tail::f4_tail_in(w);
        });
        let tick = f64::from(arena.tick) + 1.;
        let frame = arena.w.ai_goal_input(key.clone(), tick).expect("current soldier frame");
        assert!(frame.binding.is_some() && frame.fire.held);
        assert!(arena.w.ai_goal_input(key.clone(), tick).is_none());
    }
    assert!(arena.bots().is_none());
}

#[test]
fn human_target_aim_miss_sphere_updates_without_a_controller() {
    let mut human = rifle(1.0, -10.0, 0.0, 0.0);
    human.human = true;
    let bot = rifle(2.0, 10.0, 0.0, std::f64::consts::PI);
    let mut arena = Arena::build(&[human, bot], None, 99, two_deathmatch_teams());
    arena.w.sim.store.unit.get_mut(arena.units[0]).unwrap().human = true;
    arena.w.ai.battle_mut().unwrap().state.navigation.set_swbf2(
        Box::new(crate::ai::nav::swbf2::Swbf2Nav::new(None, crate::ai::nav::swbf2::BarrierToggles::default(), 0)),
        16,
        0,
    );
    arena.activate();
    assert!(arena.w.ai.battle().unwrap().runtime.core.controller_of_character(0).is_none());
    arena.run(15);
    let battle = arena.w.ai.battle().unwrap();
    let human_obj = battle.state.entities.find(crate::ai::owned::entities::Entity::Unit(arena.units[0])).unwrap();
    let entry = battle.state.human_miss[(human_obj as u32) as usize];
    assert_eq!(entry.0, human_obj);
    assert!(entry.2.radius > 0.5, "living human target should maintain a non-zero miss sphere: {}", entry.2.radius);
}
