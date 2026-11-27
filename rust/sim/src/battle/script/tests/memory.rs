//! Callback refusal and its effect on the rest of a mission.
use super::*;
use crate::battle::rules::events::{Event, EventQueue, EventRules, Outcome, RuleActions, RuleResult};

const INVENTORY: &[u8] = include_bytes!("../../../../tests/lua/24_inventory.luac");
const RECEIPTS: &[u8] = include_bytes!("../../../../tests/lua/25_receipts.luac");

#[derive(Default)]
struct Ledger {
    scores: [u32; 3],
    result: Option<Outcome>,
}

impl RuleActions for Ledger {
    fn score(&self, team: f64) -> Option<u32> { self.scores.get(team as usize).copied() }
    fn add_team_points(&mut self, team: f64, delta: i32) -> RuleResult<u32> {
        let score = &mut self.scores[team as usize];
        *score = score.saturating_add_signed(delta);
        Ok(*score)
    }
    fn start_timer(&mut self, _: crate::store::StrId, _: f64) -> bool { true }
    fn finish(&mut self, result: Outcome) -> RuleResult<()> {
        self.result = Some(result);
        Ok(())
    }
}

fn inventory() -> (ScriptRules, Ledger) {
    let mut script = ScriptRules::new(probe_chain("inventory", INVENTORY), ScriptRulesConfig::strict(3)).unwrap();
    let mut ledger = Ledger::default();
    script.tick(0., &EventQueue::new(0), &mut ledger);
    assert_eq!(script.host.lifecycle, LifecycleState::Active);
    (script, ledger)
}

fn deliveries(index: u32, timer: Option<crate::store::StrId>) -> EventQueue {
    let mut queue = EventQueue::new(3);
    assert!(queue.push(Event::TicketsChanged { team: 1., count: 19 }));
    if let Some(timer) = timer { assert!(queue.push(Event::TimerElapsed { timer })); }
    assert!(queue.push(Event::MissionSoldierKilled {
        victim: Ref::new(index, 1), victim_team: 1, human: false, hero: false, credit: None, death: Default::default(),
    }));
    queue
}

#[test]
fn a_completed_callback_resets_memory_strikes_and_other_listeners_keep_running() {
    use std::cell::Cell;
    let (mut script, mut ledger) = inventory();
    let visits = Cell::new(0);
    script.vm.register_closure("FileInventory", move |_| {
        let visit = visits.get() + 1;
        visits.set(visit);
        if visit == 3 { Ok(()) } else { Err(ScriptError::MemoryLimit { limit: 1024 }) }
    }).unwrap();
    let timer = script.host.timers[0].rules_id();
    // A refusal ends that tick's dispatch: the deliveries behind it arrive first
    // on the next tick, so team 2's points trail the refused ticks by one tick.
    let team_2 = [0, 1, 13, 13, 14, 15, 17];
    for visit in 1..=7 {
        script.tick(1./60., &deliveries(visit, (visit == 2).then_some(timer)), &mut ledger);
        assert_eq!(ledger.scores[1], visit.min(6) + 2 * u32::from(visit >= 3));
        assert_eq!(ledger.scores[2], team_2[visit as usize - 1], "visit {visit}");
        assert_eq!(script.host.diagnostics.callbacks_dropped, u32::from(visit >= 6));
        assert_eq!(script.host.degraded_deadline, (visit >= 6).then_some(36_007));
        assert!(!script.host.diagnostics.faulted);
    }
    let failures = &script.host.diagnostics.errors;
    let memory = failures.iter().find(|f| f.kind == DiagnosticKind::MemoryLimit).unwrap();
    assert_eq!(memory.occurrences, 5);
    assert!(memory.message.contains("memory limit"));
    assert!(failures.iter().any(|f| f.kind == DiagnosticKind::ListenerDisabled && f.listener_id == memory.listener_id));
    let quiet = EventQueue::new(0);
    for tick in 9..=36_007 {
        script.tick(1.0 / 60.0, &quiet, &mut ledger);
        assert_eq!(ledger.result, (tick == 36_007).then_some(Outcome::Winner(2.0)));
    }
}

#[test]
fn a_death_behind_a_memory_refusal_is_recorded_on_the_tick_it_is_delivered() {
    use crate::units::infantry::contact::{DeathRecord, LifeId};
    use std::cell::Cell;
    let (mut script, mut ledger) = inventory();
    let visits = Cell::new(0);
    script.vm.register_closure("FileInventory", move |_| {
        visits.set(visits.get() + 1);
        if visits.get() == 1 { Err(ScriptError::MemoryLimit { limit: 1024 }) } else { Ok(()) }
    }).unwrap();
    let killed = |slot: u32, tick: u32| {
        let mut queue = EventQueue::new(2);
        assert!(queue.push(Event::TicketsChanged { team: 1., count: 19 }));
        let death = DeathRecord { tick, victim: LifeId { slot, ..Default::default() }, victim_team: 1, ..Default::default() };
        assert!(queue.push(Event::MissionSoldierKilled {
            victim: Ref::new(slot, 1), victim_team: 1, human: false, hero: false, credit: None, death,
        }));
        queue
    };
    // The refusal ends the tick's dispatch before the death reaches the script.
    let first = script.host.tick + 1;
    script.tick(1. / 60., &killed(1, first), &mut ledger);
    assert!(script.host.deaths.is_empty());
    assert_eq!(ledger.scores[2], 0);
    // The next tick delivers it first, still carrying the tick it happened on.
    script.tick(1. / 60., &killed(2, first + 1), &mut ledger);
    let delivered: Vec<_> = script.host.deaths.iter().map(|d| (d.tick, d.victim.slot)).collect();
    assert_eq!(delivered, [(first, 1), (first + 1, 2)]);
    assert_eq!(ledger.scores[2], 2);
    // Each rules tick lists only its own deliveries.
    script.tick(1. / 60., &EventQueue::new(0), &mut ledger);
    assert!(script.host.deaths.is_empty());
}

/// Keep a large live object and leave room for one short string. Ordinary
/// listeners use only their existing Lua values. Filing the inventory needs
/// additional space and must return within the same tick.
fn fill_inventory_heap(script: &mut ScriptRules) {
    let vm = &mut script.vm;
    vm.set_limits(adapter::ScriptLimits { memory_limit: 128 * 1024 * 1024, ..vm.limits() }).unwrap();
    vm.register_closure("FileInventory", |ctx| {
        for item in 0u32..4_000 {
            ctx.vm_mut().intern_bytes(&item.to_le_bytes())?;
        }
        Ok(())
    }).unwrap();
    let table = vm.create_table().unwrap();
    let _root = vm.create_ref(ScriptValue::Table(table)).unwrap();
    for index in 1..=131_072 {
        let number = ScriptValue::Number(index as f32);
        vm.table_raw_set(table, number, number).unwrap();
    }
    vm.intern_bytes(b"stub").unwrap();
    vm.collect_garbage();
    vm.intern_bytes(b"stub").unwrap();
    let limit = vm.memory_bytes();
    vm.collect_garbage();
    vm.set_limits(adapter::ScriptLimits { memory_limit: limit, ..vm.limits() }).unwrap();
}

#[test]
fn memory_pressure_abandons_only_the_offending_listener_within_a_tick() {
    use std::time::{Duration, Instant};
    let (mut script, mut ledger) = inventory();
    fill_inventory_heap(&mut script);
    let timer = script.host.timers[0].rules_id();
    let quiet = EventQueue::new(0);
    let mut worst_tick = Duration::ZERO;
    let mut most_collector_work = 0;
    let mut most_script_work = 0;
    // The ticket listener fails on ticks 2, 3 and 4. Each refusal ends that
    // tick's dispatch, so the deliveries behind it run first on the next tick.
    // The fourth ticket has no subscriber. A separate death listener and a
    // timer still score.
    for tick in 2..=36_004 {
        let events;
        let queue = if tick <= 5 {
            events = deliveries(tick, (tick == 3).then_some(timer));
            &events
        } else {
            &quiet
        };
        let before = script.vm.collector_stats().total_units;
        let start = Instant::now();
        script.tick(1.0 / 60.0, queue, &mut ledger);
        worst_tick = worst_tick.max(start.elapsed());
        let collector_work = script.vm.collector_stats().total_units - before;
        most_collector_work = most_collector_work.max(collector_work);
        assert!(collector_work <= bfe_script::TICK_COLLECTOR_UNITS, "tick {tick} used {collector_work} collector units");
        let script_work = script.vm.work_used();
        most_script_work = most_script_work.max(script_work);
        assert!(script_work <= 30_000, "tick {tick} used {script_work} script units");
        assert_eq!(ledger.scores[1], (tick - 1).min(3), "partial effect at tick {tick}");
        assert_eq!(ledger.scores[2], match tick { 2 => 0, 3 => 1, 4 => 12, _ => 14 }, "later listeners at tick {tick}");
        assert_eq!(script.host.diagnostics.callbacks_dropped, u32::from(tick >= 4));
        assert_eq!(script.host.degraded_deadline, (tick >= 4).then_some(36_004));
        assert_eq!(ledger.result, (tick == 36_004).then_some(Outcome::Winner(2.0)));
        assert!(!script.host.diagnostics.faulted);
    }
    let diag = &script.host.diagnostics;
    assert_eq!(diag.callback_budget_refusals, 3);
    assert_eq!(diag.callbacks_executed, 8);
    let memory = diag.errors.iter().find(|e| e.kind == DiagnosticKind::MemoryLimit).unwrap();
    assert_eq!(memory.occurrences, 3);
    assert!(memory.message.contains("memory limit"));
    assert!(diag.errors.iter().any(|e| e.kind == DiagnosticKind::ListenerDisabled && e.listener_id == memory.listener_id));
    assert!(most_collector_work > 0);
    println!("Memory pressure: worst tick {worst_tick:?}, collector {most_collector_work}, script {most_script_work}, 3 abandoned calls, 1 disabled listener");
    // Wall time means something only for an optimized build. The collector
    // bound above holds in every build.
    if !cfg!(debug_assertions) {
        assert!(worst_tick <= Duration::from_secs_f64(1.0 / 60.0), "longest authority tick was {worst_tick:?}");
    }
}

#[test]
fn quiet_ticks_advance_collection_before_a_listener_needs_memory() {
    let (mut script, mut ledger) = inventory();
    let vm = &mut script.vm;
    let table = vm.create_table().unwrap();
    let root = vm.create_ref(ScriptValue::Table(table)).unwrap();
    for i in 1..=32_768 {
        let key = ScriptValue::Number(i as f32);
        vm.table_raw_set(table, key, key).unwrap();
    }
    vm.set_limits(adapter::ScriptLimits {
        memory_limit: vm.memory_bytes() + 1024,
        ..vm.limits()
    }).unwrap();
    assert!(vm.release_ref(root));

    let quiet = EventQueue::new(0);
    let callbacks = script.host.diagnostics.callbacks_executed;
    let mut collected = 0;
    for _ in 0..60 {
        let before = script.vm.collector_stats().total_units;
        script.tick(1.0 / 60.0, &quiet, &mut ledger);
        let units = script.vm.collector_stats().total_units - before;
        assert!(units <= bfe_script::TICK_COLLECTOR_UNITS, "quiet tick used {units} collector units");
        collected += units;
    }
    assert!(collected > 0, "no collector progress on quiet ticks with a full heap");
    assert_eq!(script.host.diagnostics.callbacks_executed, callbacks);
    assert!(!script.host.diagnostics.faulted);
}

/// The clerk interns short strings that nothing keeps. Each rescue at the hard
/// limit frees the previous batch, until a rescue no longer fits in what is left
/// of the tick's collector balance. The string arena and intern index are grown
/// beforehand, so the refusal is for one more short string and leaves no room.
/// The porter behind it builds a sixteen-slot table for every arrival, which
/// cannot fit in that tick; on the next tick the collection runs first.
#[test]
fn a_listener_behind_a_memory_refusal_allocates_after_the_next_collection() {
    let mut script = ScriptRules::new(probe_chain("receipts", RECEIPTS), ScriptRulesConfig::strict(3)).unwrap();
    let mut ledger = Ledger::default();
    let quiet = EventQueue::new(0);
    script.tick(0., &quiet, &mut ledger);
    assert_eq!(script.host.lifecycle, LifecycleState::Active);
    let vm = &mut script.vm;
    vm.register_closure("FileInventory", |ctx| {
        for item in 0u32..4_000 {
            ctx.vm_mut().intern_bytes(&item.to_le_bytes())?;
        }
        Ok(())
    }).unwrap();
    // A live table whose full collection takes about a sixth of a tick's balance.
    let table = vm.create_table().unwrap();
    let _root = vm.create_ref(ScriptValue::Table(table)).unwrap();
    for index in 1..=8_192 {
        let number = ScriptValue::Number(index as f32);
        vm.table_raw_set(table, number, number).unwrap();
    }
    for item in 0u32..6_000 {
        vm.intern_bytes(&(100_000 + item).to_le_bytes()).unwrap();
    }
    vm.collect_garbage();
    for _ in 2..=5 {
        script.tick(1.0 / 60.0, &quiet, &mut ledger);
    }
    let vm = &mut script.vm;
    let limit = vm.memory_bytes() + 8 * 1024;
    vm.set_limits(adapter::ScriptLimits { memory_limit: limit, ..vm.limits() }).unwrap();

    // Ticks 6 to 10 each bring a ticket change and a death.
    let expected = [(6, 1, 0), (7, 2, 1), (8, 3, 2), (9, 3, 4), (10, 3, 5)];
    for (tick, clerk, porter) in expected {
        let before = script.vm.collector_stats().total_units;
        script.tick(1.0 / 60.0, &deliveries(tick, None), &mut ledger);
        let units = script.vm.collector_stats().total_units - before;
        assert!(units <= bfe_script::TICK_COLLECTOR_UNITS, "tick {tick} used {units} collector units");
        assert_eq!((ledger.scores[1], ledger.scores[2]), (clerk, porter), "points after tick {tick}");
        let room = limit - script.vm.memory_bytes();
        if tick <= 8 {
            assert_eq!(room, 0, "the refusal at tick {tick} left room");
        }
        println!("Receipts tick {tick}: {units} collector units, {room} bytes of room, clerk {clerk}, porter {porter}");
    }
    let diag = &script.host.diagnostics;
    let memory: Vec<_> = diag.errors.iter().filter(|e| e.kind == DiagnosticKind::MemoryLimit).collect();
    assert_eq!(memory.len(), 1, "every memory refusal comes from one listener site");
    assert_eq!(memory[0].occurrences, 3);
    let clerk = memory[0].listener_id;
    assert!(diag.errors.iter().all(|e| e.listener_id.is_none() || e.listener_id == clerk), "the porter was never refused");
    assert!(diag.errors.iter().any(|e| e.kind == DiagnosticKind::ListenerDisabled && e.listener_id == clerk));
    assert_eq!((diag.callback_errors, diag.callback_budget_refusals, diag.callbacks_dropped), (3, 3, 1));
    assert_eq!(script.host.degraded_deadline, Some(36_008));
    for tick in 11..=36_008 {
        script.tick(1.0 / 60.0, &quiet, &mut ledger);
        assert_eq!(ledger.result, (tick == 36_008).then_some(Outcome::Winner(2.0)));
    }
}

#[test]
fn the_script_clock_saturates_at_the_bound_and_the_host_keeps_running() {
    let (mut script, mut ledger) = inventory();
    script.host.tick = u32::MAX - 3;
    for _ in 0..8 {
        script.tick(1. / 60., &EventQueue::new(0), &mut ledger);
    }
    assert_eq!(script.host.tick, u32::MAX, "the host tick saturates");
    assert!(!script.host.diagnostics.faulted, "a tick at the bound is not a fault");
    assert_eq!(script.host.lifecycle, LifecycleState::Active);
    let _ = script.host.published_digest(0, 0);
}

#[test]
fn the_host_diagnostic_counts_saturate_at_their_bound() {
    let (mut script, _) = inventory();
    let diagnostics = &mut script.host.diagnostics;
    diagnostics.refused_writes = u32::MAX;
    diagnostics.setup_refusal(5, "op", crate::battle::rules::objects::RuleError::NotAvailable);
    assert_eq!(diagnostics.refused_writes, u32::MAX);
    diagnostics.setup_refusal(6, "op", crate::battle::rules::objects::RuleError::NotAvailable);
    assert_eq!(diagnostics.refused_writes, u32::MAX);
}
