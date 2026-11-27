//! The goal bots' orders against the released controller's queue: pending
//! interrupts keep their turn through an objective publication, a full queue
//! defers a goal attack, and a withdrawn goal attack ends at once.

use super::arena::{Arena, ArenaRules, BotSpec, BuildingSpec, Op};
use crate::ai::control::controller::{goal, SourceGoal};
use crate::ai::control::goal_orders::is_goal_attack;
use crate::ai::goals::{table::StoredArg, GoalKind};
use crate::ai::nav::math::Vec3;
use crate::battle::f4_tail;

fn rifle(team: f64, x: f64, z: f64) -> BotSpec {
    BotSpec { team, pos: [x, 0.0, z], yaw: 0.0, hero: false, melee: false, health: 1.0e9, human: false, mixed: false }
}

fn numbered(n: f64) -> SourceGoal {
    let mut order = SourceGoal::new(goal::HOLD);
    order.point = Some(Vec3::new(n, 0.0, 0.0));
    order
}

fn numbers(arena: &Arena) -> Vec<f64> {
    arena.controller(0).commands.queue.iter().filter(|o| o.kind == goal::HOLD).filter_map(|o| o.point.map(|p| p.x)).collect()
}

fn deathmatch(team: u8) -> Op { Op::Add { team, kind: "Deathmatch", weight: 1.0, target: StoredArg::None, extra: StoredArg::None } }

#[test]
fn pending_interrupts_keep_their_turn_through_an_objective_publication() {
    let bots = vec![rifle(1.0, -10.0, 0.0), rifle(2.0, 140.0, 140.0)];
    let mut arena = Arena::build(&bots, None, 99, ArenaRules::Goals(vec![deathmatch(1)]));
    arena.activate();
    arena.run(30);
    for n in 0..3 {
        assert!(arena.controller_mut(0).commands.append(numbered(100.0 + f64::from(n))));
    }
    // Publication can follow immediately after an interrupt is accepted.
    assert_eq!(numbers(&arena), vec![100.0, 101.0, 102.0]);
    // The released objective flow publishes a post before the next tail: its
    // overwrite clears the queue before the goal bots see it.
    arena.step_tail_with(|w| {
        let core = &mut w.ai.battle_mut().unwrap().runtime.core;
        let ci = core.controller_of_character(0).unwrap();
        core.publish_objective(ci, 0);
        f4_tail::f4_tail_in(w);
    });
    assert_eq!(numbers(&arena), vec![100.0, 101.0, 102.0], "the cleared interrupts are back, in order");
    let queue = &arena.controller(0).commands.queue;
    assert_eq!(queue.len(), 4);
    assert!(crate::ai::control::goal_orders::is_goal_base(&queue[3]), "the goal's base order replaced the published one in its place");
}

fn destroy_mast() -> Op {
    Op::Add { team: 1, kind: "Destroy", weight: 1.0, target: StoredArg::Name(b"mast"[..].into()), extra: StoredArg::None }
}

fn mast_arena() -> Arena {
    let bots = vec![rifle(1.0, -10.0, 0.0), rifle(0.0, -60.0, 60.0), rifle(2.0, 140.0, 140.0)];
    let mast = BuildingSpec { name: "mast", team: 2.0, pos: [5.0, 0.0, 0.0], health: 1.0e9, targetable: true };
    let mut arena = Arena::build(&bots, Some(mast), 99, ArenaRules::Goals(vec![deathmatch(1)]));
    arena.activate();
    arena.run(30);
    arena
}

#[test]
fn a_full_queue_defers_a_goal_attack_and_drops_no_accepted_order() {
    let mut arena = mast_arena();
    for n in 0..5 {
        assert!(arena.controller_mut(0).commands.append(numbered(100.0 + f64::from(n))));
    }
    // The bot's goal changes to destroying a mast in reach.
    let handle = arena.handles.borrow()[0].unwrap();
    arena.ops.borrow_mut().extend([Op::Delete { handle }, destroy_mast()]);
    for _ in 0..360 {
        arena.step_one();
        assert_eq!(numbers(&arena), vec![100.0, 101.0, 102.0, 103.0, 104.0], "every accepted order keeps its place");
        let c = arena.controller(0);
        assert!(!is_goal_attack(&c.commands.current) && !c.commands.queue.iter().any(is_goal_attack), "no attack pushes one out");
    }
    assert_eq!(arena.bots().unwrap().goal_of(0).map(|g| g.kind), Some(GoalKind::Destroy));
    // Room opens: the attack goes in ahead of the pending orders, which follow it.
    arena.controller_mut(0).commands.queue.truncate(2);
    let mut started = false;
    for _ in 0..60 {
        arena.step_one();
        let c = arena.controller(0);
        if is_goal_attack(&c.commands.current) || c.commands.queue.iter().any(is_goal_attack) {
            started = true;
            break;
        }
    }
    assert!(started, "the deferred attack starts once there is room");
    assert_eq!(numbers(&arena), vec![100.0, 101.0]);
}

#[test]
fn a_withdrawn_goal_attack_leaves_the_queue_at_once() {
    let mut arena = mast_arena();
    let handle = arena.handles.borrow()[0].unwrap();
    arena.ops.borrow_mut().extend([Op::Delete { handle }, destroy_mast()]);
    let mut pending = false;
    for _ in 0..240 {
        arena.step_one();
        if arena.controller(0).commands.queue.iter().any(is_goal_attack) {
            pending = true;
            break;
        }
    }
    assert!(pending, "the bot queued an attack on the mast");
    // The goal becomes following the bystander.
    let destroy = arena.handles.borrow()[1].unwrap();
    let subject = arena.w.sim.store.battle.characters[1];
    arena.ops.borrow_mut().extend([
        Op::Delete { handle: destroy },
        Op::Add { team: 1, kind: "Follow", weight: 1.0, target: StoredArg::Character(subject), extra: StoredArg::None },
    ]);
    arena.step_tail_with(|w| {
        let core = &mut w.ai.battle_mut().unwrap().runtime.core;
        let ci = core.controller_of_character(0).unwrap();
        let q = &mut core.controllers[ci].commands;
        if !is_goal_attack(&q.current) { assert!(is_goal_attack(&q.pop_front())); }
        f4_tail::f4_tail_in(w);
    });
    let c = arena.controller(0);
    assert!(!is_goal_attack(&c.commands.current) && !c.commands.queue.iter().any(is_goal_attack), "no withdrawn attack is left");
    arena.run(30);
    assert_eq!(arena.bots().unwrap().goal_of(0).map(|g| g.kind), Some(GoalKind::Follow));
}

#[test]
#[ignore]
fn mast_trace() {
    let mut arena = mast_arena();
    let handle = arena.handles.borrow()[0].unwrap();
    arena.ops.borrow_mut().extend([Op::Delete { handle }, destroy_mast()]);
    for tick in 0..120 {
        arena.step_tail_with(|w| {
            if tick % 10 == 0 {
                let core = &w.ai.battle().unwrap().runtime.core;
                let c = &core.controllers[core.controller_of_character(0).unwrap()];
                println!("t{tick} before tail: cur {} done {} latch {} fsm {} agent {:?} queue {:?}", c.commands.current.kind, c.commands.current_done, c.commands.latch, c.fsm.state, c.agent.as_ref().map(|a| a.fsm.state), c.commands.queue.iter().map(|o| o.kind).collect::<Vec<_>>());
            }
            f4_tail::f4_tail_in(w);
        });
        if tick % 10 == 0 {
            let bots = arena.bots().unwrap();
            let goal = bots.goal_of(0).map(|g| (g.kind, g.slot, g.active));
            let subject = goal.and_then(|g| bots.subject(g.1));
            let target = goal.and_then(|g| bots.target(g.1).cloned());
            let c = arena.controller(0);
            println!("t{tick} goal {goal:?} target {target:?} subject {subject:?} cur {} ref_b {:?} done {} latch {} queue {:?}", c.commands.current.kind, c.commands.current.ref_b, c.commands.current_done, c.commands.latch, c.commands.queue.iter().map(|o| o.kind).collect::<Vec<_>>());
        }
    }
}

#[test]
fn an_unassigned_table_restores_released_publication_after_a_loaned_reset() {
    let bots = [rifle(1.0, -10.0, 0.0), rifle(2.0, 140.0, 140.0)];
    let mut arena = Arena::build(&bots, None, 99, ArenaRules::Goals(Vec::new()));
    arena.activate();
    assert!(arena.bots().unwrap().steered().is_none());
    let lent = arena.w.ai.take_battle().unwrap();
    arena.reinit_tail();
    assert_eq!(crate::ai::owned::entry::ai_world_phase_in(&mut arena.w), 0);
    arena.w.ai.return_battle(lent);
    arena.step_one();
    assert!(arena.controller_mut(0).commands.append(numbered(77.0)));
    let core = &mut arena.w.ai.battle_mut().unwrap().runtime.core;
    let ci = core.controller_of_character(0).unwrap();
    core.publish_objective(ci, 0);
    assert!(numbers(&arena).is_empty(), "Conquest publication clears a queued interrupt");
}
