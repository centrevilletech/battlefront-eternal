//! Goal table, assignment, order and button rules on plain data.

use super::assign::{Assignments, GoalSpec};
use super::melee::{Binding, BlockMode, MeleeInput, Wanted, FIRE, FIRE_SECONDARY};
use super::regions::{region_center, GoalRegions};
use super::table::{GoalArg, GoalKind, GoalSlots, GoalTable, StoredArg, GOAL_SLOTS, MAX_GOAL_HANDLE};
use crate::ai::control::controller::{goal, CommandQueue, SourceGoal};
use crate::ai::control::goal_orders::{is_goal_base, publish_base, retire_orders, set_base, Base, Change};
use crate::ai::nav::math::Vec3;

pub(crate) mod arena;
mod bits;
mod cost;
mod disposal;
mod fights;
mod hunt;
mod live;
mod orders;
mod subjects;

fn name(text: &str) -> StoredArg { StoredArg::Name(text.as_bytes().into()) }

fn specs(table: &GoalSlots) -> Vec<GoalSpec> {
    (0..table.len())
        .map(|i| {
            let r = table.row(i);
            GoalSpec { slot: r.slot, handle: r.handle, team: r.team, kind: r.kind, weight: r.weight }
        })
        .collect()
}

#[test]
fn the_table_accepts_teams_zero_to_seven_and_positive_weights_only() {
    let mut t = GoalSlots::new();
    assert!(t.add(0, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None).is_some());
    assert!(t.add(7, b"deathMATCH", 0.25, StoredArg::None, StoredArg::None).is_some());
    for weight in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(t.add(1, b"Deathmatch", weight, StoredArg::None, StoredArg::None), None, "weight {weight}");
    }
    assert_eq!(t.add(8, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None), None);
    assert_eq!(t.add(1, b"Hunt", 1.0, StoredArg::None, StoredArg::None), None);
    assert_eq!(t.len(), 2);
    // The weight is kept exactly.
    assert_eq!(t.row(1).weight, 0.25);
}

#[test]
fn twenty_slots_are_shared_by_every_team() {
    let mut t = GoalSlots::new();
    let mut accepted = 0;
    for team in 1..=7u8 {
        for _ in 0..64 {
            accepted += usize::from(t.add(team, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None).is_some());
        }
    }
    assert_eq!(accepted, GOAL_SLOTS);
    assert_eq!(t.len(), GOAL_SLOTS);
}

#[test]
fn targets_are_required_only_where_the_kind_needs_one() {
    let mut t = GoalSlots::new();
    for kind in [&b"Destroy"[..], b"Defend", b"Follow", b"CTFOffense", b"CTFDefense"] {
        assert_eq!(t.add(1, kind, 1.0, StoredArg::None, StoredArg::None), None);
    }
    assert!(t.add(1, b"Conquest", 1.0, StoredArg::None, StoredArg::None).is_some());
    // An offense goal may name only its area; the flag is optional.
    assert!(t.add(2, b"CTFOffense", 1.0, name("north_gate"), StoredArg::None).is_some());
    assert!(t.add(2, b"CTFOffense", 1.0, name("north_gate"), name("blue_flag")).is_some());
    assert_eq!(t.row(1).target, GoalArg::Name(b"north_gate"));
    assert_eq!(t.row(1).extra, GoalArg::None);
    assert_eq!(t.row(2).kind, GoalKind::CtfOffense);
}

#[test]
fn a_new_goal_takes_the_first_free_slot_and_handles_are_never_reused() {
    let mut t = GoalSlots::new();
    let a = t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None).unwrap();
    let b = t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None).unwrap();
    let before = t.revision();
    assert!(t.delete(a));
    assert!(!t.delete(a));
    assert!(t.revision() > before);
    let c = t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None).unwrap();
    assert!(c != a && c != b);
    assert_eq!((t.row(0).slot, t.row(0).handle), (0, c));
    assert_eq!((t.row(1).slot, t.row(1).handle), (1, b));
    t.clear(2);
    assert_eq!(t.len(), 2);
    t.clear(1);
    assert!(t.is_empty());
}

fn assign_all(a: &mut Assignments, characters: std::ops::Range<usize>, team: u8) -> Vec<u8> {
    characters.map(|c| a.assign(c, team).unwrap().slot).collect()
}

#[test]
fn fractional_weights_split_bots_by_the_lowest_assigned_to_weight_ratio() {
    let mut t = GoalSlots::new();
    t.add(1, b"Deathmatch", 0.1, StoredArg::None, StoredArg::None);
    t.add(1, b"Deathmatch", 0.9, StoredArg::None, StoredArg::None);
    let mut a = Assignments::default();
    a.reserve(10);
    a.sync(specs(&t));
    let slots = assign_all(&mut a, 0..10, 1);
    assert_eq!(slots.iter().filter(|s| **s == 0).count(), 1);
    assert_eq!(slots.iter().filter(|s| **s == 1).count(), 9);
    // The earlier slot wins the opening tie (0 / 0.1 against 0 / 0.9).
    assert_eq!(slots[0], 0);
}

#[test]
fn seventy_thirty_gives_seven_and_three() {
    let mut t = GoalSlots::new();
    t.add(3, b"Deathmatch", 70.0, StoredArg::None, StoredArg::None);
    t.add(3, b"Defend", 30.0, name("tower"), StoredArg::None);
    let mut a = Assignments::default();
    a.reserve(10);
    a.sync(specs(&t));
    let slots = assign_all(&mut a, 0..10, 3);
    assert_eq!(slots.iter().filter(|s| **s == 0).count(), 7);
    assert_eq!(slots.iter().filter(|s| **s == 1).count(), 3);
}

#[test]
fn a_reused_free_slot_comes_first_in_a_tie() {
    let mut t = GoalSlots::new();
    let first = t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None).unwrap();
    t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None);
    t.delete(first);
    t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None);
    let mut a = Assignments::default();
    a.reserve(2);
    a.sync(specs(&t));
    assert_eq!(a.assign(0, 1).unwrap().slot, 0);
    assert_eq!(a.assign(1, 1).unwrap().slot, 1);
}

#[test]
fn a_new_goal_never_takes_a_bot_from_a_valid_goal() {
    let mut t = GoalSlots::new();
    t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None);
    let mut a = Assignments::default();
    a.reserve(10);
    a.sync(specs(&t));
    assign_all(&mut a, 0..10, 1);
    t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None);
    a.sync(specs(&t));
    // Every bot keeps its goal; the new goal fills as bots become free.
    assert!(assign_all(&mut a, 0..10, 1).iter().all(|s| *s == 0));
    a.release(4);
    assert_eq!(a.assign(4, 1).unwrap().slot, 1);
}

#[test]
fn suspended_goals_free_their_bots_and_resume_without_taking_others() {
    let mut t = GoalSlots::new();
    t.add(2, b"Destroy", 1.0, name("generator"), StoredArg::None);
    t.add(2, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None);
    let mut a = Assignments::default();
    a.reserve(4);
    a.sync(specs(&t));
    assert_eq!(assign_all(&mut a, 0..4, 2), vec![0, 1, 0, 1]);
    a.set_active(0, false);
    assert_eq!(a.goal(0).unwrap().assigned, 0);
    assert_eq!(assign_all(&mut a, 0..4, 2), vec![1, 1, 1, 1]);
    a.set_active(0, true);
    assert_eq!(assign_all(&mut a, 0..4, 2), vec![1, 1, 1, 1]);
    a.release(3);
    assert_eq!(a.assign(3, 2).unwrap().slot, 0);
}

#[test]
fn wrong_team_and_deleted_goals_release_their_bots() {
    let mut t = GoalSlots::new();
    let one = t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None).unwrap();
    t.add(2, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None);
    let mut a = Assignments::default();
    a.reserve(2);
    a.sync(specs(&t));
    assert_eq!(a.assign(0, 1).unwrap().slot, 0);
    // The bot came back on team 2.
    assert_eq!(a.assign(0, 2).unwrap().slot, 1);
    assert_eq!(a.goal(0).unwrap().assigned, 0);
    assert_eq!(a.assign(1, 1).unwrap().slot, 0);
    t.delete(one);
    a.sync(specs(&t));
    assert_eq!(a.of(1), None);
    assert_eq!(a.assign(1, 1), None);
    assert_eq!(a.of(0).unwrap().slot, 1);
}

#[test]
fn the_writer_refuses_what_the_script_host_refuses() {
    use crate::gen::Character;
    use crate::store::Ref;
    let someone = StoredArg::Character(Ref::<Character>::new(3, 1));
    let mut t = GoalSlots::new();
    // A follower follows a character or an object, never a bare name.
    assert_eq!(t.add(1, b"Follow", 1.0, name("leader"), StoredArg::None), None);
    assert!(t.add(1, b"Follow", 1.0, someone.clone(), StoredArg::None).is_some());
    // A flag offense names its area; a character is no area.
    assert_eq!(t.add(1, b"CTFOffense", 1.0, someone.clone(), StoredArg::None), None);
    assert!(t.add(1, b"CTFOffense", 1.0, name("gate"), StoredArg::None).is_some());
    assert!(t.add(1, b"CTFOffense", 1.0, name("gate"), name("flag")).is_some());
    // Every other kind that needs a target takes any.
    assert!(t.add(1, b"Defend", 1.0, someone, StoredArg::None).is_some());
    assert!(t.add(1, b"Destroy", 1.0, name("mast"), StoredArg::None).is_some());
}

#[test]
fn a_bound_table_issues_no_handle_below_its_first_and_stops_at_the_last_exact_number() {
    let mut t = GoalSlots::new();
    t.bind(100);
    assert_eq!(t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None), Some(100));
    t.bind(50);
    assert_eq!(t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None), Some(101));
    assert_eq!(t.next_handle(), 102);
    t.clear(1);
    t.bind(MAX_GOAL_HANDLE - 1);
    assert_eq!(t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None), Some(MAX_GOAL_HANDLE - 1));
    assert_eq!(t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None), Some(MAX_GOAL_HANDLE));
    assert_eq!(t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None), None, "no exact handle is left");
    assert_eq!(t.len(), 2);
}

/// One pass over a wave as the goal bots make it: every departure first,
/// then every vacancy.
fn wave(a: &mut Assignments, teams: &[Option<u8>]) {
    for (c, team) in teams.iter().enumerate() {
        a.keep(c, *team);
    }
    for (c, team) in teams.iter().enumerate() {
        if let Some(team) = team {
            a.fill(c, *team);
        }
    }
}

#[test]
fn a_wave_frees_its_departures_before_it_fills_vacancies() {
    let mut t = GoalSlots::new();
    t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None);
    t.add(1, b"Deathmatch", 1.0, StoredArg::None, StoredArg::None);
    let mut a = Assignments::default();
    a.reserve(10);
    a.sync(specs(&t));
    // Only a high-index bot carries goal A.
    let mut teams = vec![None; 10];
    teams[9] = Some(1);
    wave(&mut a, &teams);
    assert_eq!(a.of(9).unwrap().slot, 0);
    // On the next tick a low-index bot joins as the high-index bot leaves:
    // both goals are vacant, so the earlier slot wins.
    teams[9] = None;
    teams[1] = Some(1);
    wave(&mut a, &teams);
    assert_eq!(a.of(9), None);
    assert_eq!(a.of(1).unwrap().slot, 0);
    assert_eq!((a.goal(0).unwrap().assigned, a.goal(1).unwrap().assigned), (1, 0));
    // A bot whose team leaves the table's range loses its link and count.
    teams[1] = None;
    wave(&mut a, &teams);
    assert_eq!(a.of(1), None);
    assert_eq!(a.goal(0).unwrap().assigned, 0);
}

fn hold() -> SourceGoal { SourceGoal::new(goal::HOLD) }
fn numbered(n: f64) -> SourceGoal {
    let mut order = hold();
    order.point = Some(Vec3::new(n, 0.0, 0.0));
    order
}

#[test]
fn a_bound_bot_runs_its_base_order_next() {
    let mut q = CommandQueue::new();
    assert_eq!(set_base(&mut q, Base::Wait), Change::Next);
    assert!(q.needs_next_goal());
    assert!(is_goal_base(&q.pop_front()));
    q.latch = false;
    assert_eq!(set_base(&mut q, Base::Wait), Change::Kept);
}

#[test]
fn a_pending_hold_keeps_its_turn_when_the_base_order_changes() {
    let mut q = CommandQueue::new();
    set_base(&mut q, Base::Wait);
    q.pop_front();
    q.latch = false;
    // The released interrupt pass pushes a squad hold, saving the base order.
    let saved = q.current_copy();
    assert!(q.prepend_two(saved, hold()));
    assert_eq!(set_base(&mut q, Base::Wait), Change::Kept);
    // A Conquest goal replaces the saved base order in place.
    assert_eq!(set_base(&mut q, Base::Objective(3)), Change::Replaced);
    assert_eq!(q.queue[0].kind, goal::HOLD);
    assert_eq!((q.queue[1].kind, q.queue[1].post), (goal::OBJECTIVE, Some(3)));
    assert_eq!(q.pop_front().kind, goal::HOLD);
    q.latch = false;
    assert_eq!(set_base(&mut q, Base::Objective(3)), Change::Kept);
    q.current_done = true;
    assert_eq!(q.pop_front().post, Some(3));
}

#[test]
fn a_running_interrupt_without_a_saved_base_returns_to_the_new_base() {
    let mut q = CommandQueue::new();
    q.prepend_one(SourceGoal::combat(1.0, None, None));
    q.pop_front();
    q.latch = false;
    assert_eq!(set_base(&mut q, Base::Wait), Change::Queued);
    assert_eq!(q.current.kind, goal::COMBAT);
    q.current_done = true;
    assert!(q.needs_next_goal());
    assert!(is_goal_base(&q.pop_front()));
}

#[test]
fn a_published_objective_is_a_base_order_and_is_replaced_in_place() {
    let mut q = CommandQueue::new();
    q.overwrite(SourceGoal::objective(Some(1)));
    assert_eq!(set_base(&mut q, Base::Objective(1)), Change::Kept);
    assert_eq!(set_base(&mut q, Base::Wait), Change::Replaced);
    assert!(is_goal_base(&q.queue[0]));
}

#[test]
fn objective_publication_keeps_new_and_duplicate_interrupts_in_a_full_queue() {
    let mut q = CommandQueue::new();
    for n in [7., 8., 5., 5., 5.] { assert!(q.append(numbered(n))); }
    publish_base(&mut q, 1);
    publish_base(&mut q, 2);
    for n in [7., 8., 5., 5., 5.] { assert_eq!(q.pop_front().point.unwrap().x, n); }
    assert_eq!(q.pop_front().post, Some(2));
    assert!(q.queue.is_empty());
}

#[test]
fn a_whole_command_history_keeps_every_interrupt() {
    // Base, a fight, a squad hold pushed during the fight (a fight is not
    // saved), a goal change, the hold ending: every order runs once, in order.
    let mut q = CommandQueue::new();
    let mut ran = Vec::new();
    let step = |q: &mut CommandQueue, ran: &mut Vec<f64>| {
        if q.needs_next_goal() {
            ran.push(q.pop_front().kind);
            q.latch = false;
        }
    };
    set_base(&mut q, Base::Wait);
    step(&mut q, &mut ran);
    let saved = q.current_copy();
    q.prepend_two(saved, SourceGoal::combat(1.0, None, None));
    step(&mut q, &mut ran);
    q.prepend_one(hold());
    assert_eq!(set_base(&mut q, Base::Objective(5)), Change::Replaced);
    step(&mut q, &mut ran);
    q.current_done = true;
    step(&mut q, &mut ran);
    q.current_done = true;
    step(&mut q, &mut ran);
    assert_eq!(ran, vec![goal::WAIT, goal::COMBAT, goal::HOLD, goal::OBJECTIVE]);
    assert_eq!(q.current.post, Some(5));
}

#[test]
fn a_goal_attack_waits_for_room_and_a_withdrawn_one_hands_over_at_once() {
    use crate::ai::control::goal_orders::{cancel_attacks, is_goal_attack, start_attack, Attack};
    use crate::ai::control::world::EntRef;
    let target = EntRef { unit: 9, generation: 1.0 };
    let mut q = CommandQueue::new();
    set_base(&mut q, Base::Wait);
    q.pop_front();
    q.latch = false;
    for n in 0..4 {
        assert!(q.append(numbered(100.0 + f64::from(n))));
    }
    // The attack and its saved base order need two places: it waits.
    assert_eq!(start_attack(&mut q, target), Attack::Deferred);
    assert_eq!(q.queue.len(), 4);
    q.queue.truncate(3);
    assert_eq!(start_attack(&mut q, target), Attack::Started);
    assert_eq!(start_attack(&mut q, target), Attack::Underway);
    assert_eq!(q.queue.len(), 5, "nothing accepted was pushed out");
    // The attack runs; the goal changes.
    assert!(is_goal_attack(&q.pop_front()));
    q.latch = false;
    cancel_attacks(&mut q);
    assert!(q.needs_next_goal(), "the running attack hands over at the next service");
    assert!(is_goal_base(&q.pop_front()), "to the saved base order");
    let held: Vec<_> = q.queue.iter().filter_map(|o| o.point.map(|p| p.x)).collect();
    assert_eq!(held, vec![100.0, 101.0, 102.0], "the pending orders keep their turn");
    // A running attack with nothing queued behind it returns to a base order.
    let mut q = CommandQueue::new();
    q.prepend_one(SourceGoal::new(goal::WAIT));
    q.pop_front();
    q.latch = false;
    assert_eq!(start_attack(&mut q, target), Attack::Started);
    q.pop_front();
    q.latch = false;
    q.queue.clear();
    cancel_attacks(&mut q);
    assert!(is_goal_base(&q.pop_front()));
}

#[test]
fn retiring_removes_only_the_goal_orders() {
    let mut q = CommandQueue::new();
    set_base(&mut q, Base::Wait);
    q.pop_front();
    q.latch = false;
    let saved = q.current_copy();
    q.prepend_two(saved, hold());
    q.pop_front();
    assert!(retire_orders(&mut q));
    assert_eq!(q.current.kind, goal::HOLD);
    assert!(q.queue.iter().all(|o| !is_goal_base(o)));
    assert!(!retire_orders(&mut q), "nothing left to remove");
}

const DT: f64 = 1.0 / 60.0;

fn key(life: f64) -> crate::gen::F3UnitKey { crate::gen::F3UnitKey { slot: 4, generation: 2, life } }

fn blade_on(channel: u8, fire: bool, block: bool) -> Wanted {
    let binding = Binding { slot: usize::from(channel), weapon: crate::store::DefId(7), channel };
    Wanted { binding: Some(binding), fire, block, forward: 0.0, right: 0.0 }
}

#[test]
fn one_sustained_press_is_one_press_and_a_repeat_step_changes_nothing() {
    let mut m = MeleeInput::new(1, key(1.0), 3);
    for tick in 0..3 {
        m.step(f64::from(tick), DT, blade_on(0, true, false), BlockMode::Toggle);
        m.step(f64::from(tick), DT, blade_on(0, false, false), BlockMode::Toggle);
        assert!(m.fire.held);
        assert_eq!(m.fire.pressed, tick == 0);
        assert_eq!(m.combo_bits(), (if tick == 0 { FIRE } else { 0 }, FIRE));
    }
    m.step(3.0, DT, blade_on(0, false, false), BlockMode::Toggle);
    assert!(m.fire.released && !m.fire.held);
    assert_eq!(m.combo_bits(), (0, 0));
    assert!(m.is_for(&key(1.0), 3.0) && !m.is_for(&key(2.0), 3.0) && !m.is_for(&key(1.0), 2.0));
}

#[test]
fn a_blade_on_the_secondary_channel_feeds_logical_fire_and_still_blocks_on_the_dedicated_control() {
    let mut m = MeleeInput::new(1, key(1.0), 3);
    m.step(0.0, DT, blade_on(1, true, false), BlockMode::Toggle);
    assert_eq!(m.combo_bits(), (FIRE, FIRE), "the secondary channel's trigger is logical Fire");
    m.step(1.0, DT, blade_on(1, false, false), BlockMode::Toggle);
    let mut presses = 0;
    for tick in 20..40 {
        m.step(f64::from(tick), DT, blade_on(1, false, true), BlockMode::Toggle);
        presses += u32::from(m.combo_bits().0 & FIRE_SECONDARY != 0);
        if m.block.pressed { m.sync_guard(true); }
    }
    assert_eq!(presses, 1, "one dedicated block press");
    assert!(m.guarding);
}

#[test]
fn losing_the_weapon_or_mounting_releases_what_was_held_and_a_new_binding_starts_idle() {
    let mut m = MeleeInput::new(1, key(1.0), 3);
    m.step(0.0, DT, blade_on(0, true, false), BlockMode::Toggle);
    m.step(1.0, DT, Wanted::default(), BlockMode::Toggle);
    assert!(m.fire.released);
    assert_eq!(m.combo_bits(), (0, 0));
    // A raised block on one binding is not believed on another.
    for tick in 10..12 {
        m.step(f64::from(tick), DT, blade_on(0, false, true), BlockMode::Toggle);
        if m.block.pressed { m.sync_guard(true); }
    }
    assert!(m.guarding);
    m.step(12.0, DT, blade_on(1, false, false), BlockMode::Toggle);
    assert!(!m.guarding);
}

#[test]
fn the_default_block_is_one_press_up_and_one_press_down() {
    let mut m = MeleeInput::new(1, key(1.0), 3);
    let mut presses = 0;
    for tick in 0..10 {
        m.step(f64::from(tick), DT, blade_on(0, false, true), BlockMode::Toggle);
        presses += u32::from(m.block.pressed);
        if m.block.pressed { assert!(!m.guarding); m.sync_guard(true); }
        assert!(m.guarding);
    }
    assert_eq!(presses, 1);
    assert!(!m.block.held, "the button comes up after its press");
    // Lowering waits for the spacing after the raising press.
    for tick in 10..40 {
        m.step(f64::from(tick), DT, blade_on(0, false, false), BlockMode::Toggle);
        if m.block.pressed {
            assert!(tick >= 18, "a press {tick} ticks after the last");
            presses += 1;
            m.sync_guard(false);
        }
    }
    assert_eq!(presses, 2);
    assert!(!m.guarding);
    // The combo machine reports the guard still raised (it held the press, or
    // dropped it): the bot does not press again within the retry time, then
    // makes one fresh press.
    m.sync_guard(true);
    let mut again = Vec::new();
    for tick in 40..140 {
        m.step(f64::from(tick), DT, blade_on(0, false, false), BlockMode::Toggle);
        m.sync_guard(true);
        if m.block.pressed {
            again.push(tick);
        }
    }
    assert!(again.iter().all(|t| *t >= 18 + 60), "no retry within a second: {again:?}");
    assert!(again.windows(2).all(|w| w[1] - w[0] >= 60), "retries a second apart: {again:?}");
    assert!(!again.is_empty());
}

#[test]
fn an_attack_changes_the_guard_belief_only_through_the_committed_state() {
    let mut m = MeleeInput::new(1, key(1.0), 3);
    m.step(0.0, DT, blade_on(0, false, true), BlockMode::Toggle);
    assert!(!m.guarding);
    m.sync_guard(true);
    m.step(1.0, DT, blade_on(0, true, true), BlockMode::Toggle);
    assert_eq!(m.combo_bits().0, FIRE);
    assert!(m.guarding, "the attack alone says nothing about the guard");
    // The machine reports that the attack's transition won and broke guard.
    m.sync_guard(false);
    assert!(!m.guarding);
}

#[test]
fn hold_to_block_lasts_while_wanted() {
    let mut m = MeleeInput::new(1, key(1.0), 3);
    for tick in 0..5 {
        m.step(f64::from(tick), DT, blade_on(0, false, true), BlockMode::Hold);
        assert_eq!(m.block.pressed, tick == 0);
    }
    assert_eq!(m.combo_bits(), (0, FIRE_SECONDARY));
    m.step(5.0, DT, blade_on(0, false, false), BlockMode::Hold);
    assert!(m.block.released);
}

#[test]
fn hold_to_block_represses_when_committed_guard_dropped_while_still_wanted() {
    let mut m = MeleeInput::new(1, key(1.0), 3);
    m.step(0.0, DT, blade_on(0, false, true), BlockMode::Hold);
    assert!(m.block.pressed && m.block.held);
    m.consumed = true;
    assert!(m.report(true, 100.0, false));
    // While the combo machine reports active guard, hold never pulses a release.
    for tick in 1..40 {
        m.step(f64::from(tick), DT, blade_on(0, false, true), BlockMode::Hold);
        assert!(!m.block.pressed && m.block.held);
        m.consumed = true;
        assert!(m.report(true, 100.0, false));
    }
    // Once guard is broken (for example after a stagger or jump), hold re-presses
    // cleanly so the combo machine re-enters guard.
    let mut re_pressed = false;
    for tick in 40..70 {
        m.step(f64::from(tick), DT, blade_on(0, false, true), BlockMode::Hold);
        if m.block.pressed {
            re_pressed = true;
            break;
        }
        m.consumed = true;
        assert!(m.report(false, 100.0, false));
    }
    assert!(re_pressed, "hold mode emits a fresh press to re-enter guard");
}

fn value(json: &str) -> bfe_content::value::Value { bfe_content::json::parse(json).expect("test json") }

#[test]
fn regions_read_the_prepared_matrix_the_older_transform_and_a_centre() {
    let matrix = value(r#"{"name":"Gate","matrix":[1,0,0,0, 0,1,0,0, 0,0,1,0, 10.5,2.25,-3.0,1]}"#);
    assert_eq!(region_center(&matrix), Some(Vec3::new(10.5, 2.25, -3.0)));
    let xfrm = value(r#"{"name":"yard","xfrm":[1,0,0, 0,1,0, 0,0,1, 4.0,1.0,6.0]}"#);
    assert_eq!(region_center(&xfrm), Some(Vec3::new(4.0, 1.0, -6.0)));
    let center = value(r#"{"name":"well","center":[1,2,3]}"#);
    assert_eq!(region_center(&center), Some(Vec3::new(1.0, 2.0, 3.0)));
    let regions = GoalRegions::read(&value(&format!(
        "[{},{},{},{}]",
        r#"{"name":"Gate","matrix":[1,0,0,0, 0,1,0,0, 0,0,1,0, 10.5,2.25,-3.0,1]}"#,
        r#"{"name":"yard","xfrm":[1,0,0, 0,1,0, 0,0,1, 4.0,1.0,6.0]}"#,
        r#"{"name":"twice","center":[0,0,0]}"#,
        r#"{"name":"TWICE","center":[1,1,1]}"#
    )));
    assert_eq!(regions.find(b"gate"), Some(Vec3::new(10.5, 2.25, -3.0)));
    assert_eq!(regions.find(b"YARD"), Some(Vec3::new(4.0, 1.0, -6.0)));
    assert_eq!(regions.find(b"twice"), None, "an ambiguous name is never found");
    assert_eq!(regions.find(b"absent"), None);
}

#[test]
fn every_name_form_finds_the_same_mission_object() {
    use crate::battle::rules::events::ObjectHandle;
    use crate::battle::rules::objects::{MissionObjects, NamedObject};
    use crate::gen::SpawnPost;
    use crate::store::Ref;
    let names = ["north", "\u{e000}", "\u{10000}", "Zeta", "alpha"];
    let rows = names
        .iter()
        .enumerate()
        .map(|(i, n)| NamedObject {
            object: ObjectHandle::Post(SpawnPost::Post(Ref::new(i as u32, 1))),
            name: Some((*n).to_owned()),
            class: None,
        })
        .collect();
    let objects = MissionObjects::new(rows);
    for (i, n) in names.iter().enumerate() {
        let found = objects.find_bytes(n.as_bytes()).expect("every admitted name");
        assert_eq!(*found, ObjectHandle::Post(SpawnPost::Post(Ref::new(i as u32, 1))));
    }
    assert!(objects.find_bytes(b"NORTH").is_ok());
    assert!(objects.find_bytes(b"\xff").is_err());
}

/// Every named region of a prepared 2005 map (`BFE_SWBF2_CONTENT`, the
/// family root) resolves by name to its matrix translation.
#[test]
fn prepared_2005_regions_resolve_by_name() {
    let Ok(root) = std::env::var("BFE_SWBF2_CONTENT") else { return };
    for map in ["tat2", "spa6"] {
        let path = std::path::Path::new(&root).join("maps").join(map).join("map.json");
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let json = bfe_content::json::parse(&text).expect("prepared map");
        let Some(records) = json.get("regions").and_then(|r| r.as_array()) else { continue };
        let regions = GoalRegions::read(json.get("regions").unwrap());
        let mut found = 0;
        for record in records {
            let name = record.get("name").and_then(|n| n.as_str()).expect("named region");
            let m = record.get("matrix").and_then(|m| m.as_array()).expect("prepared matrix");
            let at = |i: usize| m[i].as_f64().unwrap();
            let unique = records.iter().filter(|r| r.get("name").and_then(|n| n.as_str()).is_some_and(|n| n.eq_ignore_ascii_case(name))).count() == 1;
            if unique {
                assert_eq!(regions.find(name.to_ascii_uppercase().as_bytes()), Some(Vec3::new(at(12), at(13), at(14))), "{map} {name}");
                found += 1;
            }
        }
        println!("{map}: {found} of {} regions resolve by name", records.len());
        assert!(found > 0);
    }
}

#[test]
fn a_guard_request_waits_for_the_committed_mover_to_recover() {
    let mut input = MeleeInput::new(5, key(1.), 0);
    input.step(1., DT, blade_on(0, false, true), BlockMode::Toggle);
    assert!(input.block.pressed && !input.guarding);
    input.consumed = true;
    assert!(input.report(false, 0., true));
    for tick in 2..80 {
        input.step(f64::from(tick), DT, blade_on(0, false, true), BlockMode::Toggle);
        assert!(!input.block.pressed);
    }
    input.consumed = true;
    assert!(input.report(false, 1., false));
    input.step(80., DT, blade_on(0, false, true), BlockMode::Toggle);
    assert!(input.block.pressed && !input.guarding);
}

#[test]
fn entering_striking_distance_presses_fire_without_waiting_for_a_world_tick_phase() {
    use super::melee::{Binding, SwingClock};
    use crate::{ai::control::world::EntRef, store::DefId};
    let enemy = EntRef { unit: 7, generation: 2. };
    let blade = Binding { slot: 2, weapon: DefId(4), channel: 1 };
    let mut clock = SwingClock::default();
    for t in 0..17 { assert!(!clock.pulse(t, None)); }
    for t in 17..60 { assert_eq!(clock.pulse(t, Some((enemy, blade))), (t - 17) % 18 < 4); }
    assert!(!clock.pulse(60, None));
    assert!(clock.pulse(61, Some((enemy, blade))));
    for t in 62..67 { clock.pulse(t, Some((enemy, blade))); }
    assert!(clock.pulse(67, Some((EntRef { generation: 3., ..enemy }, blade))));
}
