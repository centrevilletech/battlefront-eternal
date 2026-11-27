//! Every way installed rules are disposed of or replaced hands their goal
//! bots' bots back, and a failed installation changes nothing.
//!
//! The disposal paths in the code: the tail reset the unit factory
//! initializer runs (`battle::f4::reset_in`), a new battle tail
//! (`f4_tail_init_in`), a rules selection replacing the installed rules (to
//! Conquest or to other event rules), and the world's own drop, which takes
//! the AI with it. A selection whose preparation fails, or that cannot reach
//! the AI to hand the bots back, is refused and leaves the installed rules.

use super::arena::{Arena, ArenaRules, BotSpec, Op, TestRules};
use crate::ai::control::controller::SourceGoal;
use crate::ai::control::goal_orders::{is_goal_attack, is_goal_base};
use crate::ai::goals::{table::StoredArg, GoalKind, GoalTable};
use crate::battle::rules::{events::EventDriven, ModeParameters, RuleSet};
use crate::battle::{f4, f4_tail};
use bfe_family::GameFamily;

fn deathmatch(team: u8) -> Op { Op::Add { team, kind: "Deathmatch", weight: 1.0, target: StoredArg::None, extra: StoredArg::None } }

fn rifle(team: f64, x: f64, z: f64) -> BotSpec {
    BotSpec { team, pos: [x, 0.0, z], yaw: 0.0, hero: false, melee: false, health: 250.0, human: false, mixed: false }
}

fn arena() -> Arena {
    let mut bots = Vec::new();
    for i in 0..3 {
        bots.push(rifle(1.0, -36.0, i as f64 * 4.0));
        bots.push(rifle(2.0, 36.0, i as f64 * 4.0));
    }
    let mut arena = Arena::build(&bots, None, 99, ArenaRules::Goals(vec![deathmatch(1), deathmatch(2)]));
    arena.activate();
    arena.run(90);
    assert_eq!(tagged(&arena), 6, "every bot carries a goal order");
    arena
}

fn tagged(arena: &Arena) -> usize {
    let goal = |o: &SourceGoal| is_goal_base(o) || is_goal_attack(o);
    (0..6)
        .filter(|&i| {
            let c = arena.controller(i);
            goal(&c.commands.current) || c.commands.queue.iter().any(goal)
        })
        .count()
}

fn released(arena: &Arena) {
    assert_eq!(tagged(arena), 0, "no goal order is left");
    for i in 0..6 {
        let c = arena.controller(i);
        assert!(c.fsm.state == 0.0 && c.agent.is_none(), "bot {i} is freshly bound");
    }
}

fn parameters() -> ModeParameters { ModeParameters { command_limit: 32, score_limit: Some(99), ..Default::default() } }

fn select_goal_rules(arena: &mut Arena) -> bool {
    arena.ops.borrow_mut().extend([deathmatch(1), deathmatch(2)]);
    f4_tail::select_rules(&mut arena.w, GameFamily::Swbf2, "arena", parameters(), Vec::new()).is_ok()
}

#[test]
fn the_factory_reset_hands_the_bots_back_before_a_new_tail_runs() {
    let mut arena = arena();
    // The unit factory initializer resets the battle owner without the AI.
    f4::reset_in(&mut arena.w.battle);
    assert!(arena.bots().is_none());
    // The next tail is the next operation that can reach the AI: the bots
    // come back before it runs a tick.
    arena.factory_reset_and_readmit();
    released(&arena);
    assert!(f4_tail::select_rules(&mut arena.w, GameFamily::Swbf1, "conquest", Default::default(), Vec::new()).is_ok());
    for _ in 0..120 {
        arena.step_one();
        assert_eq!(tagged(&arena), 0, "the old goal orders never run under Conquest");
    }
    // New goal rules take the bots afresh.
    assert!(select_goal_rules(&mut arena));
    arena.run(60);
    assert_eq!(tagged(&arena), 6);
}

#[test]
fn a_new_tail_hands_the_bots_back() {
    let mut arena = arena();
    arena.reinit_tail();
    assert!(arena.bots().is_none());
    released(&arena);
    arena.run(120);
    assert_eq!(tagged(&arena), 0);
}

#[test]
fn replacing_the_rules_hands_the_bots_back_in_both_directions() {
    let mut arena = arena();
    assert!(f4_tail::select_rules(&mut arena.w, GameFamily::Swbf1, "conquest", Default::default(), Vec::new()).is_ok());
    released(&arena);
    arena.run(60);
    assert_eq!(tagged(&arena), 0);
    assert!(select_goal_rules(&mut arena));
    arena.run(60);
    assert_eq!(tagged(&arena), 6);
    // Goal rules replaced by goal rules while the bots run.
    assert!(select_goal_rules(&mut arena));
    released(&arena);
    for i in 0..6 {
        assert!(arena.bots().unwrap().goal_of(i).is_none(), "no bot carries an old goal");
    }
    arena.run(60);
    assert_eq!(tagged(&arena), 6);
}

fn test_rules(arena: &mut Arena) -> TestRules {
    TestRules {
        score_limit: 99,
        table: crate::ai::goals::GoalSlots::with_handles(arena.w.goal_handles()),
        ops: arena.ops.clone(),
        handles: arena.handles.clone(),
        has_table: true,
    }
}

#[test]
fn a_failed_installation_leaves_the_installed_rules_and_their_bots() {
    let mut arena = arena();
    let goals: Vec<_> = (0..6).map(|i| arena.bots().unwrap().goal_of(i).map(|g| g.handle)).collect();
    // A command limit no event queue can hold: the preparation fails.
    let failing = RuleSet::EventDriven(EventDriven::new(test_rules(&mut arena), usize::MAX, 0));
    assert_eq!(f4_tail::install_rules_for_test(&mut arena.w, failing), None);
    assert!(arena.bots().is_some(), "the installed goal bots stay");
    assert_eq!(tagged(&arena), 6, "no order was taken away");
    let after: Vec<_> = (0..6).map(|i| arena.bots().unwrap().goal_of(i).map(|g| g.handle)).collect();
    assert_eq!(goals, after);
    arena.run(60);
    assert_eq!(tagged(&arena), 6, "the installed goal bots still run");
}

#[test]
fn an_installation_that_cannot_reach_the_ai_is_refused() {
    let mut arena = arena();
    let lent = arena.w.ai.take_battle().expect("AI battle");
    arena.ops.borrow_mut().extend([deathmatch(1), deathmatch(2)]);
    assert!(f4_tail::select_rules(&mut arena.w, GameFamily::Swbf2, "arena", parameters(), Vec::new()).is_err());
    arena.w.ai.return_battle(lent);
    arena.ops.borrow_mut().clear();
    assert!(arena.bots().is_some(), "the installed goal bots stay");
    assert_eq!(tagged(&arena), 6);
    arena.run(30);
    assert_eq!(tagged(&arena), 6);
}

#[test]
fn a_kept_handle_never_names_a_goal_of_later_rules() {
    let mut arena = arena();
    let old = arena.handles.borrow()[0].expect("first goal handle");
    assert_eq!(old, 1);
    // Replace the rules: the new table's first goal sits in the same slot at
    // the same revision as the old one did.
    arena.handles.borrow_mut().clear();
    arena.ops.borrow_mut().push(deathmatch(1));
    assert!(f4_tail::select_rules(&mut arena.w, GameFamily::Swbf2, "arena", parameters(), Vec::new()).is_ok());
    arena.run(2);
    let new = arena.handles.borrow()[0].expect("first goal of the new rules");
    assert!(new > 2, "the new rules issue no handle the old ones did: {new}");
    // The script still holds the old token and deletes with it.
    arena.ops.borrow_mut().push(Op::Delete { handle: old });
    arena.run(2);
    let bots = arena.bots().unwrap();
    let kinds: Vec<_> = bots.goals().map(|g| (g.slot, g.handle, g.kind)).collect();
    assert_eq!(kinds, vec![(0, new, GoalKind::Deathmatch)], "the replacement's goal is untouched");
    // Through a factory reset and a new tail too.
    arena.factory_reset_and_readmit();
    arena.handles.borrow_mut().clear();
    arena.ops.borrow_mut().push(deathmatch(2));
    assert!(f4_tail::select_rules(&mut arena.w, GameFamily::Swbf2, "arena", parameters(), Vec::new()).is_ok());
    arena.run(2);
    let latest = arena.handles.borrow()[0].unwrap();
    assert!(latest > new, "handles keep rising across tails: {latest} after {new}");
    let table_len = |arena: &mut Arena| arena.bots().map_or(0, |b| b.goals().count());
    arena.ops.borrow_mut().extend([Op::Delete { handle: old }, Op::Delete { handle: new }]);
    arena.run(2);
    assert_eq!(table_len(&mut arena), 1);
}

#[test]
fn rules_that_stop_giving_a_table_read_as_an_empty_one() {
    let mut arena = arena();
    assert!(crate::ai::goals::NO_GOALS.is_empty());
    // The handler's table goes away: the goal bots keep running on an empty
    // table, so every bot is freed and stands.
    arena.ops.borrow_mut().push(Op::DropTable);
    arena.run(30);
    let bots = arena.bots().expect("the goal bots stay for the installation");
    assert_eq!(bots.goals().count(), 0);
    for i in 0..6 {
        assert!(bots.goal_of(i).is_none());
    }
}

#[test]
fn a_reset_during_an_ai_loan_retires_before_the_next_decision() {
    let mut arena = arena();
    let lent = arena.w.ai.take_battle().unwrap();
    arena.reinit_tail();
    arena.w.ai.return_battle(lent);
    arena.step_one();
    assert_eq!(tagged(&arena), 0);
    for _ in 0..120 {
        arena.step_one();
        assert_eq!(tagged(&arena), 0);
    }
}

#[test]
fn script_preparation_uses_the_same_numbers_even_when_its_table_is_hidden_or_refused() {
    use crate::ai::goals::{GoalSlots, GoalTable};
    let mut arena = arena();
    let old = arena.handles.borrow()[0].unwrap();
    let mut prepared = GoalSlots::with_handles(arena.w.goal_handles());
    let issue = |t: &mut GoalSlots| t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None).unwrap();
    let early = issue(&mut prepared);
    assert!(early > old);
    assert!(!prepared.delete(old));
    let mut failed = test_rules(&mut arena);
    failed.table = prepared;
    let mode = EventDriven::new(failed, usize::MAX, 0);
    assert!(f4_tail::install_rules_for_test(&mut arena.w, RuleSet::EventDriven(mode)).is_none());
    arena.ops.borrow_mut().extend([deathmatch(1), Op::DropTable]);
    arena.step_one();
    let hidden = arena.handles.borrow().last().unwrap().unwrap();
    assert!(hidden > early);
    arena.factory_reset_and_readmit();
    let mut next = GoalSlots::with_handles(arena.w.goal_handles());
    let replacement = issue(&mut next);
    assert!(replacement > hidden);
    assert!(!next.delete(old));
    assert_eq!(next.row(0).handle, replacement);
}

#[test]
fn a_tableless_install_waits_for_the_admitted_character_capacity() {
    let bots = [rifle(1.0, -8.0, 0.0), rifle(2.0, 8.0, 0.0)];
    let mut arena = Arena::build(&bots, None, 99, ArenaRules::NoTable);
    arena.activate();
    let lent = arena.w.ai.take_battle().unwrap();
    assert!(f4_tail::select_rules(&mut arena.w, GameFamily::Swbf2, "arena", parameters(), Vec::new()).is_err());
    arena.w.ai.return_battle(lent);
    assert!(f4_tail::select_rules(&mut arena.w, GameFamily::Swbf2, "arena", parameters(), Vec::new()).is_ok());
}
