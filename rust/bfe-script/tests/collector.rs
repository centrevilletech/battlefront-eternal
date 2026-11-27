//! Collector work is separate from script work. A simulation tick shares one
//! collector balance across all callbacks and reservations.

use bfe_wide::Wide;
use std::time::{Duration, Instant};

use bfe_script::{CallCtx, ScriptError, Value, Vm, VmLimits};

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/lua/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"))
}

/// Largest share of work an incremental step is given (`GC_MAX_STEP_WORK`).
const MAX_STEP_UNITS: u32 = 8192;

/// A VM with the periodic workload loaded and `setup(entries)` run under the load budget.
fn periodic_vm(entries: u32) -> Vm {
    let mut vm = Vm::new_with_stdlib();
    vm.run_chunk(&mut (), &fixture_bytes("27_periodic.luac"))
        .expect("workload loads");
    let setup = vm.get_global("setup").unwrap();
    vm.call(&mut (), setup, &[Value::Number(entries as f32)])
        .expect("setup fits the load budget");
    vm
}

struct PeriodicRun {
    refusals: usize,
    max_call: Duration,
}

/// Call `step(i)` `calls` times with `budget` each, counting refusals.
fn run_periodic(vm: &mut Vm, calls: u32, budget: u32) -> PeriodicRun {
    vm.set_instruction_budget(budget);
    let step = vm.get_global("step").unwrap();
    let mut out = Vec::with_capacity(4);
    let mut refusals = 0;
    let mut max_call = Duration::ZERO;
    for i in 0..calls {
        let t = Instant::now();
        let res = vm.call_into(&mut (), step, &[Value::Number(i as f32)], &mut out);
        max_call = max_call.max(t.elapsed());
        match res {
            Ok(()) => assert!(matches!(out[..], [Value::Number(n)] if n == 1.0)),
            Err(_) => refusals += 1,
        }
    }
    PeriodicRun { refusals, max_call }
}

/// The same callback must keep its result throughout collection at both table sizes.
#[test]
fn routine_callbacks_are_never_refused_for_collection() {
    for entries in [32_768u32, 131_072] {
        let mut vm = periodic_vm(entries);
        let before = vm.collector_stats();
        let run = run_periodic(&mut vm, 100_000, VmLimits::TICK_BUDGET);
        let stats = vm.collector_stats();
        println!(
            "{entries} entries: {} refusals, {} steps, {} cycles, largest step {} units, \
             {} deferred, slowest call {:?}, {} bytes",
            run.refusals,
            stats.steps - before.steps,
            stats.cycles - before.cycles,
            stats.max_step_units,
            stats.deferred_steps,
            run.max_call,
            vm.memory_bytes(),
        );
        assert_eq!(run.refusals, 0, "{entries} entries");
        assert!(stats.cycles > before.cycles, "cycles completed during the run");
        assert!(stats.max_step_units <= MAX_STEP_UNITS, "{stats:?}");
        assert_eq!(stats.deferred_steps, 0, "the tick allowance covers every step");
        assert_eq!(stats.forced_collections, 0, "the incremental cycles keep up");
        // Each callback's own work is a handful of instructions.
        assert!(vm.work_used() < 32, "own work {}", vm.work_used());
    }
}

/// With an allowance far too small for the collector's share, steps wait instead of
/// refusing: the calls all succeed, the collector falls behind, and memory pressure
/// (the defined backstop) collects in full when needed.
#[test]
fn a_tight_allowance_defers_collection_instead_of_refusing() {
    let mut vm = periodic_vm(32_768);
    let run = run_periodic(&mut vm, 100_000, 24);
    let stats = vm.collector_stats();
    println!("tight allowance: {stats:?}, slowest call {:?}", run.max_call);
    assert_eq!(run.refusals, 0);
    assert!(stats.deferred_steps > 0, "{stats:?}");
    assert!(vm.memory_bytes() <= vm.limits().memory_limit);
}

/// A call whose own work exceeds the budget is still refused, whatever the collector
/// has or has not done.
#[test]
fn a_call_over_its_own_budget_is_still_refused() {
    let mut vm = periodic_vm(1_024);
    vm.set_instruction_budget(3);
    let step = vm.get_global("step").unwrap();
    let err = vm.call(&mut (), step, &[Value::Number(1.0)]).unwrap_err();
    assert!(matches!(err, ScriptError::InstructionLimit { limit: 3 }), "{err}");
}

/// `Vm::collect_garbage` from a native function never uses the call's own allowance:
/// with a budget of three or twelve units the call succeeds and its own work is the
/// same as without the request.
#[test]
fn collect_garbage_from_a_native_never_uses_the_call_allowance() {
    fn collect(ctx: &mut CallCtx<'_, ()>) -> Result<(), ScriptError> {
        ctx.vm_mut().collect_garbage();
        Ok(())
    }
    fn nothing(_: &mut CallCtx<'_, ()>) -> Result<(), ScriptError> {
        Ok(())
    }
    for budget in [3u32, 12] {
        let mut used = Vec::new();
        for f in [nothing as fn(&mut CallCtx<'_, ()>) -> Result<(), ScriptError>, collect] {
            let mut vm = periodic_vm(4_096);
            let native = vm.create_native_fn(f).unwrap();
            vm.set_instruction_budget(budget);
            vm.call(&mut (), Value::NativeFn(native), &[])
                .unwrap_or_else(|e| panic!("budget {budget}: {e}"));
            used.push(vm.work_used());
        }
        assert_eq!(used[0], used[1], "budget {budget}");
        // The script's own `collectgarbage()` is collector work too.
        let mut vm = periodic_vm(4_096);
        let cg = vm.get_global("collectgarbage").unwrap();
        vm.set_instruction_budget(budget);
        vm.call(&mut (), cg, &[]).unwrap_or_else(|e| panic!("budget {budget}: {e}"));
    }
}

/// A full collection requested inside a call that cannot afford it waits, and runs
/// at the earliest step that can: between calls, when the host next allocates.
#[test]
fn a_deferred_full_collection_runs_between_calls() {
    let mut vm = periodic_vm(32_768);
    let cg = vm.get_global("collectgarbage").unwrap();
    vm.set_instruction_budget(VmLimits::TICK_BUDGET);
    let before = vm.collector_stats();
    vm.call(&mut (), cg, &[]).unwrap();
    let mid = vm.collector_stats();
    assert_eq!(mid.full_collections, before.full_collections);
    assert_eq!(mid.deferred_requests, before.deferred_requests + 1);
    vm.intern_str("between calls").unwrap();
    let after = vm.collector_stats();
    assert_eq!(after.full_collections, before.full_collections + 1);
    // With a heap that small requests run at once even inside a tick.
    let mut small = periodic_vm(16);
    let cg = small.get_global("collectgarbage").unwrap();
    small.set_instruction_budget(VmLimits::TICK_BUDGET);
    let before = small.collector_stats();
    small.call(&mut (), cg, &[]).unwrap();
    assert_eq!(small.collector_stats().full_collections, before.full_collections + 1);
}

/// A dead table with a large key index is destroyed a block at a time: no step does
/// more than its share, and the memory comes back.
#[test]
fn a_large_dead_index_is_destroyed_in_pieces() {
    let mut vm = periodic_vm(16);
    let build = vm.get_global("build_index").unwrap();
    let t = vm.call(&mut (), build, &[Value::Number(30_000.0)]).unwrap();
    assert!(matches!(t[0], Value::Table(_)));
    drop(t);
    let built = vm.memory_bytes();
    let churn = vm.get_global("churn").unwrap();
    vm.set_instruction_budget(VmLimits::TICK_BUDGET);
    let mut out = Vec::new();
    let mut i = 0u32;
    let start = vm.collector_stats();
    while vm.collector_stats().cycles < start.cycles + 3 {
        vm.call_into(&mut (), churn, &[Value::Number(i as f32)], &mut out)
            .unwrap();
        i += 1;
        assert!(i < 2_000_000, "cycles complete");
    }
    let stats = vm.collector_stats();
    println!(
        "index destroyed after {i} calls: {stats:?}, {} bytes, {built} built",
        vm.memory_bytes()
    );
    assert!(stats.max_step_units <= MAX_STEP_UNITS, "{stats:?}");
    assert_eq!(stats.forced_collections, start.forced_collections);
    assert_eq!(stats.full_collections, start.full_collections);
    // After a full collection the index (30,000 keys at 72 bytes each, 2.2 MB) is
    // released with its entries and key strings; what stays is arena slots, which are
    // reused but never returned.
    vm.set_instruction_budget(VmLimits::LOAD_BUDGET);
    vm.collect_garbage();
    assert!(
        vm.memory_bytes() < built - 2_000_000,
        "{} of {built}",
        vm.memory_bytes()
    );
}

#[test]
fn cached_lookup_fits_the_same_budget_during_sweep() {
    fn read(ctx: &mut CallCtx<'_, [u32; 32]>) -> Result<(), ScriptError> {
        ctx.charge_work(32)?;
        assert_eq!(ctx.host().iter().sum::<u32>(), 528);
        ctx.vm_mut().intern_str("field")?;
        Ok(())
    }
    let mut vm = Vm::with_limits(VmLimits::default()).unwrap();
    vm.open_stdlib().unwrap();
    let key = vm.intern_str("field").unwrap();
    vm.create_ref(Value::String(key)).unwrap();
    let reader = Value::NativeFn(vm.register_fn("read", read).unwrap());
    let mut fields = std::array::from_fn(|i| i as u32 + 1);
    let mut out = Vec::new();
    vm.set_instruction_budget(35);
    let mut steps = 0;
    for i in 0..100_000 {
        vm.intern_str(&format!("entry{i}")).unwrap();
        let before = vm.collector_stats().steps;
        vm.call_into(&mut fields, reader, &[], &mut out).unwrap();
        assert_eq!(vm.work_used(), 35);
        steps += vm.collector_stats().steps - before;
    }
    assert!(steps > 0);
}

#[test]
fn between_call_digest_distinguishes_a_pending_collection() {
    fn request(ctx: &mut CallCtx<'_, bool>) -> Result<(), ScriptError> {
        if *ctx.host() {
            ctx.vm_mut().collect_garbage();
        }
        Ok(())
    }
    fn sample(mut requested: bool) -> (Wide, bool) {
        let mut vm = Vm::<bool>::new_with_stdlib();
        let f = vm.register_fn("maintain", request).unwrap();
        let table = Value::Table(vm.create_table().unwrap());
        vm.set_instruction_budget(3);
        vm.call(&mut requested, Value::NativeFn(f), &[]).unwrap();
        assert_eq!(vm.work_used(), 3);
        let digest = vm.state_digest();
        vm.intern_str("another event").unwrap();
        (digest, vm.is_valid_value(table))
    }
    let a = sample(false);
    let b = sample(true);
    assert!(a.1 && !b.1);
    assert_ne!(a.0, b.0);
    assert_eq!(b, sample(true));
}

#[test]
fn a_large_registry_is_scanned_across_bounded_steps() {
    let mut vm = periodic_vm(32_768);
    for n in 0..12_000 {
        vm.create_ref(Value::Number(n as f32)).unwrap();
    }
    let before = vm.collector_stats().cycles;
    let run = run_periodic(&mut vm, 100_000, VmLimits::TICK_BUDGET);
    let stats = vm.collector_stats();
    assert_eq!(run.refusals, 0);
    assert!(stats.cycles > before);
    assert!(stats.max_step_units <= 8192, "{stats:?}");
}

#[test]
fn failed_callback_leaves_effects_and_later_ticks_reclaim_space() {
    use bfe_script::TICK_COLLECTOR_UNITS;
    let mut vm = periodic_vm(131_072);
    vm.set_global("checkpoint", Value::Number(0.)).unwrap();
    let (f, _root) = vm
        .create_rooted_native_fn(|ctx| {
            let vm = ctx.vm_mut();
            vm.set_global("checkpoint", Value::Number(1.))?;
            vm.intern_bytes(&[19; 4096])?;
            vm.set_global("checkpoint", Value::Number(2.))
        })
        .unwrap();
    let scratch = vm.intern_bytes(&[41; 6144]).unwrap();
    let held = vm.create_ref(Value::String(scratch)).unwrap();
    vm.collect_garbage();
    vm.set_limits(VmLimits {
        memory_limit: vm.memory_bytes() + 256,
        ..vm.limits()
    })
    .unwrap();
    vm.release_ref(held);
    vm.set_instruction_budget(VmLimits::TICK_BUDGET);
    let mut start = vm.collector_stats().total_units;
    vm.begin_tick(1).unwrap();
    let error = vm.call(&mut (), Value::NativeFn(f), &[]).unwrap_err();
    assert!(
        matches!(error, ScriptError::MemoryLimit { .. }),
        "{error:?}"
    );
    assert!(matches!(
        vm.get_global("checkpoint").unwrap(),
        Value::Number(1.)
    ));
    assert!(vm.collector_stats().total_units - start <= TICK_COLLECTOR_UNITS);
    let digest = vm.state_digest();
    vm.begin_tick(1).unwrap();
    assert!(vm.begin_tick(0).is_err());
    assert_eq!(digest, vm.state_digest());
    for tick in 2..=100 {
        start = vm.collector_stats().total_units;
        vm.begin_tick(tick).unwrap();
        let result = vm.call(&mut (), Value::NativeFn(f), &[]);
        assert!(vm.collector_stats().total_units - start <= TICK_COLLECTOR_UNITS);
        if result.is_ok() {
            assert!(matches!(
                vm.get_global("checkpoint").unwrap(),
                Value::Number(2.)
            ));
            assert!(vm.collector_stats().max_step_units <= MAX_STEP_UNITS);
            println!(
                "recovered at tick {tick}, digest {:016x}",
                vm.state_digest()
            );
            return;
        }
        assert!(matches!(result, Err(ScriptError::MemoryLimit { .. })));
    }
    panic!("garbage still occupied released space");
}

#[test]
fn garbage_churn_stays_within_memory_and_tick_work() {
    use bfe_script::TICK_COLLECTOR_UNITS;
    let mut vm = Vm::with_limits(VmLimits {
        memory_limit: 4 << 20,
        ..VmLimits::default()
    })
    .unwrap();
    vm.open_stdlib().unwrap();
    vm.run_chunk(&mut (), include_bytes!("lua/22_hostile.luac"))
        .unwrap();
    let f = vm.get_global("garbage_churn").unwrap();
    vm.set_instruction_budget(VmLimits::TICK_BUDGET);
    let (mut most, mut peak) = (0, 0);
    for tick in 0..128 {
        let start = vm.collector_stats().total_units;
        vm.begin_tick(tick).unwrap();
        let result = vm.call(&mut (), f, &[]);
        assert!(
            matches!(result, Err(ScriptError::InstructionLimit { .. })),
            "tick {tick}: {result:?}"
        );
        assert_eq!(vm.work_used(), VmLimits::TICK_BUDGET);
        most = most.max(vm.collector_stats().total_units - start);
        peak = peak.max(vm.memory_bytes());
        assert!(most <= TICK_COLLECTOR_UNITS);
        assert!(peak < 4 << 20);
    }
    assert!(vm.collector_stats().cycles > 1);
    println!(
        "128 churn ticks, peak {peak} bytes, largest {most} units, digest {:016x}",
        vm.state_digest()
    );
}

#[test]
fn a_spent_collector_balance_still_admits_allocations_that_fit() {
    use bfe_script::TICK_COLLECTOR_UNITS;
    let mut vm = periodic_vm(131_072);
    let (f, _) = vm
        .create_rooted_native_fn(|ctx| {
            assert!(ctx.vm_mut().begin_tick(12).is_err());
            ctx.vm_mut().intern_bytes(b"new in this tick")?;
            ctx.push_return(Value::Bool(true))
        })
        .unwrap();
    let before = vm.collector_stats().total_units;
    vm.begin_tick(11).unwrap();
    vm.collect_garbage();
    let used = vm.collector_stats().total_units;
    assert!((TICK_COLLECTOR_UNITS - 4..=TICK_COLLECTOR_UNITS).contains(&(used - before)));
    vm.begin_tick(11).unwrap();
    let out = vm.call(&mut (), Value::NativeFn(f), &[]).unwrap();
    assert!(matches!(&out[..], [Value::Bool(true)]));
    assert_eq!(vm.collector_stats().total_units, used);
    vm.begin_tick(12).unwrap();
    assert!(vm.collector_stats().total_units > used);
}

#[test]
fn inline_capture_traffic_paces_collection_in_reserved_slots() {
    let mut vm = Vm::with_limits(VmLimits {
        memory_limit: 3 << 20,
        ..VmLimits::default()
    })
    .unwrap();
    vm.run_chunk(&mut (), include_bytes!("lua/15_alloc_bench.luac"))
        .unwrap();
    let capture = vm.get_global("bench_capture").unwrap();
    let mut out = Vec::new();
    let before = vm.collector_stats();
    for tick in 0..2_000 {
        vm.begin_tick(tick).unwrap();
        for _ in 0..24 {
            vm.call_into(&mut (), capture, &[Value::Number(3.)], &mut out)
                .unwrap();
        }
    }
    let stats = vm.collector_stats();
    assert!(stats.cycles > before.cycles + 2);
    assert_eq!(stats.forced_collections, before.forced_collections);
}

/// A busy mission whose live table is half of its memory limit. Every tick runs 24
/// callbacks that each build a capturing closure, 8 that each build two new strings
/// and a table, and 4 that each format a new marker. Collection is paced only by
/// `begin_tick`: no memory rescue runs, no allocation is refused, the heap stays
/// below the limit, and no tick does more than its collector balance.
#[test]
fn a_healthy_heavy_allocation_mission_never_reaches_the_limit() {
    use bfe_script::TICK_COLLECTOR_UNITS;
    let mut vm = periodic_vm(131_072);
    vm.run_chunk(&mut (), &fixture_bytes("15_alloc_bench.luac"))
        .unwrap();
    vm.collect_garbage();
    let live = vm.memory_bytes();
    let limit = 2 * live;
    vm.set_limits(VmLimits {
        memory_limit: limit,
        ..vm.limits()
    })
    .unwrap();
    let capture = vm.get_global("bench_capture").unwrap();
    let churn = vm.get_global("churn").unwrap();
    let step = vm.get_global("step").unwrap();
    let mut out = Vec::with_capacity(4);
    let before = vm.collector_stats();
    let (mut most, mut peak) = (0, 0);
    for tick in 0..20_000u32 {
        let start = vm.collector_stats().total_units;
        vm.begin_tick(tick).unwrap();
        vm.set_instruction_budget(VmLimits::TICK_BUDGET);
        for i in 0..24u32 {
            vm.call_into(&mut (), capture, &[Value::Number(i as f32)], &mut out)
                .unwrap();
        }
        for i in 0..8u32 {
            let n = Value::Number((tick * 8 + i) as f32);
            vm.call_into(&mut (), churn, &[n], &mut out).unwrap();
        }
        for i in 0..4u32 {
            let n = Value::Number((tick * 4 + i) as f32);
            vm.call_into(&mut (), step, &[n], &mut out).unwrap();
        }
        most = most.max(vm.collector_stats().total_units - start);
        peak = peak.max(vm.memory_bytes());
    }
    let stats = vm.collector_stats();
    assert_eq!(stats.forced_collections, before.forced_collections);
    assert!(stats.cycles > before.cycles + 100, "{stats:?}");
    assert!(most <= TICK_COLLECTOR_UNITS);
    // Garbage never takes more than half of the room the live data leaves.
    assert!(peak - live < (limit - live) / 2, "peak {peak} of {limit}");
    println!(
        "healthy mission: live {live} of {limit}, peak {peak}, {} cycles, largest tick {most} units",
        stats.cycles - before.cycles
    );
    // The schedule and the heap it leaves are the same on every build and architecture.
    assert_eq!(vm.state_digest(), Wide::new(0xd2ea_70e4, 0x1cfe_b74e));
}

/// Full collections clear weak tables in slices while the script reads, copies,
/// moves, deletes and re-weakens their entries between ticks, with ample room and
/// under memory pressure. Every value the script can still reach stays intact, and
/// after a complete full collection a weak table holds only strongly reachable values.
#[test]
fn weak_tables_cleared_across_ticks_keep_every_reachable_value() {
    for room in [96 << 10, 12 << 10] {
        let mut vm = Vm::with_limits(VmLimits {
            memory_limit: 64 << 20,
            ..VmLimits::default()
        })
        .unwrap();
        vm.open_stdlib().unwrap();
        vm.run_chunk(&mut (), &fixture_bytes("30_weak_paced.luac"))
            .unwrap();
        let setup = vm.get_global("setup").unwrap();
        vm.call(&mut (), setup, &[Value::Number(2000.)]).unwrap();
        vm.collect_garbage();
        vm.set_limits(VmLimits {
            memory_limit: vm.memory_bytes() + room,
            ..vm.limits()
        })
        .unwrap();
        let poke = vm.get_global("poke").unwrap();
        let verify = vm.get_global("verify").unwrap();
        let before = vm.collector_stats();
        let mut refusals = 0;
        for tick in 0..1_500u32 {
            vm.begin_tick(tick).unwrap();
            vm.set_instruction_budget(VmLimits::TICK_BUDGET);
            let args = [Value::Number(60.), Value::Number(2000.)];
            match vm.call(&mut (), poke, &args) {
                Ok(_) => {}
                Err(ScriptError::MemoryLimit { .. }) => refusals += 1,
                Err(e) => panic!("tick {tick}: {e}"),
            }
            if tick % 8 == 7 {
                vm.set_instruction_budget(VmLimits::LOAD_BUDGET);
                match vm.call(&mut (), verify, &[]) {
                    Ok(_) | Err(ScriptError::MemoryLimit { .. }) => {}
                    Err(e) => panic!("tick {tick}: {e}"),
                }
            }
        }
        // Finish one more full collection, on ticks alone if the request needs them.
        let full = vm.collector_stats().full_collections;
        vm.collect_garbage();
        let mut tick = 1_500;
        while vm.collector_stats().full_collections == full {
            vm.begin_tick(tick).unwrap();
            tick += 1;
            assert!(tick < 3_500, "the last full collection did not finish");
        }
        vm.set_instruction_budget(VmLimits::LOAD_BUDGET);
        vm.call(&mut (), verify, &[]).unwrap();
        let settled = vm.get_global("settled").unwrap();
        let stray = vm.call(&mut (), settled, &[]).unwrap();
        assert!(matches!(stray[..], [Value::Number(n)] if n == 0.), "{stray:?}");
        let stats = vm.collector_stats();
        assert!(stats.full_collections - before.full_collections > 100, "{stats:?}");
        if room == 12 << 10 {
            assert!(refusals > 0);
            assert!(stats.forced_collections > before.forced_collections);
        }
        println!(
            "weak tables, room {room}: {} full collections, {} forced, {refusals} refused calls",
            stats.full_collections - before.full_collections,
            stats.forced_collections - before.forced_collections
        );
    }
}
