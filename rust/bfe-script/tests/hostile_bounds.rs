//! Hostile programs (`tests/lua/22_hostile.lua`) under a 4 MiB memory limit: each
//! one stops at its work budget or the memory limit, release builds stop within a
//! wall-time bound for the recommended budgets, and the heap the whole process
//! allocates while a program runs stays within twice the memory limit.
//!
//! This file is its own test binary with a counting allocator and a single test,
//! so nothing else allocates while a program is measured.

use std::time::{Duration, Instant};

use bfe_script::{ScriptError, Value, Vm, VmLimits};
use bfe_test_alloc::{live_bytes, peak_bytes, reset_peak, Tracking};

#[global_allocator]
static ALLOCATOR: Tracking = Tracking::new();

const MEMORY_LIMIT: usize = 4 << 20;

const PROGRAMS: &[&str] = &[
    "find_plain",
    "class_scan",
    "backtrack",
    "backref",
    "sort_comparator",
    "index_chain",
    "tombstones",
    "deep_recursion",
    "garbage_churn",
    "grow_tables",
    "grow_strings",
    "grow_closures",
    "double_string",
    "rep_parts",
    "gsub_expand",
    "print_flood",
    "pointer_chase",
    "plain_find_long_pattern",
    "find_long_pattern",
    "gfind_long_pattern",
    "gsub_long_pattern",
    "format_precision",
    "concat_long_separator",
    "error_long_message",
    "deep_level_lookup",
    "weak_mode_scan",
];

fn fixture() -> Vec<u8> {
    let path = format!("{}/tests/lua/22_hostile.luac", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"))
}

struct Run {
    outcome: Result<(), ScriptError>,
    elapsed: Duration,
    work: u32,
    heap_growth: usize,
}

fn run(chunk: &[u8], name: &str, budget: u32) -> Run {
    let base = live_bytes();
    reset_peak();
    let mut vm: Vm = Vm::with_limits(VmLimits {
        instruction_budget: budget,
        memory_limit: MEMORY_LIMIT,
        ..VmLimits::default()
    })
    .unwrap();
    vm.open_stdlib().unwrap();
    vm.capture_output();
    vm.run_chunk(&mut (), chunk).unwrap();
    let f = vm.get_global(name).unwrap();
    assert!(matches!(f, Value::Closure(_)), "{name} is not defined");
    let start = Instant::now();
    let outcome = vm.call(&mut (), f, &[]).map(|_| ());
    let elapsed = start.elapsed();
    let work = vm.work_used();
    drop(vm);
    // The peak is never below the base it was reset to, so its difference wraps correctly; the live
    // reading is a gauge that can fall below the base (memory freed from before the run), so it saturates.
    let heap_growth = usize::try_from(peak_bytes().wrapping_sub(base)).unwrap_or(usize::MAX);
    let left = live_bytes().saturating_sub(base);
    assert!(left < 64 * 1024, "{name}: {left} bytes still allocated after the VM was dropped");
    Run {
        outcome,
        elapsed,
        work,
        heap_growth,
    }
}

#[test]
fn hostile_programs_stop_within_budget_time_and_memory() {
    let chunk = fixture();
    // Debug builds run far slower per unit; they check outcomes and memory with a
    // smaller budget that still reaches the memory limit.
    let release = !cfg!(debug_assertions);
    let budget = if release { VmLimits::LOAD_BUDGET } else { 1_000_000 };
    let mut report = Vec::new();
    for &name in PROGRAMS {
        let r = run(&chunk, name, budget);
        let stop = match &r.outcome {
            Err(ScriptError::InstructionLimit { .. }) => {
                assert!(r.work >= budget, "{name}");
                "budget"
            }
            Err(ScriptError::MemoryLimit { .. }) => "memory",
            other => panic!("{name}: expected the budget or the memory limit to stop it, got {other:?}"),
        };
        assert!(r.work <= budget, "{name}: used {} units of {budget}", r.work);
        assert!(
            r.heap_growth <= MEMORY_LIMIT,
            "{name}: process heap grew {} bytes under a {MEMORY_LIMIT}-byte limit",
            r.heap_growth
        );
        let mut tick_time = Duration::ZERO;
        if release {
            assert!(r.elapsed < Duration::from_secs(3), "{name}: {:?} for {} units", r.elapsed, r.work);
            let tick = run(&chunk, name, VmLimits::TICK_BUDGET);
            assert!(
                tick.work <= VmLimits::TICK_BUDGET,
                "{name}: tick work {}",
                tick.work
            );
            assert!(
                tick.heap_growth <= MEMORY_LIMIT,
                "{name}: tick peak {}",
                tick.heap_growth
            );
            assert!(
                tick.elapsed < Duration::from_millis(50),
                "{name}: {:?} for a tick budget",
                tick.elapsed
            );
            tick_time = tick.elapsed;
        }
        report.push(format!(
            "{name:16} {stop:7} {:>9} units {:>8.1} ns/unit heap {:>5.2}x limit, {:?} for a tick budget",
            r.work,
            r.elapsed.as_nanos() as f64 / r.work.max(1) as f64,
            r.heap_growth as f64 / MEMORY_LIMIT as f64,
            tick_time,
        ));
    }
    eprintln!("{}", report.join("\n"));
}
