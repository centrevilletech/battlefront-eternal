//! Counting-allocator benchmark and regression test for common VM tick paths.
//!
//! Uses the shared counting `#[global_allocator]` of `bfe-test-alloc` to verify
//! host heap allocations and requested bytes during warmed `call_into` ticks
//! (`return x + 1`, 1-arg `pcall`, 8-element `table.sort` with a Lua comparator,
//! and `string.gfind` over 64/128/256-byte strings).

use bfe_script::{Value, Vm};

#[global_allocator]
static GLOBAL: bfe_test_alloc::Counting = bfe_test_alloc::Counting::new();

fn measure<F: FnOnce()>(f: F) -> (u32, u32) {
    let totals = bfe_test_alloc::measure_all(f).1;
    (totals.allocations, totals.bytes)
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/lua/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"))
}

#[test]
fn tick_path_allocations() {
    let bytecode = fixture_bytes("15_alloc_bench.luac");
    let mut vm: Vm = Vm::new_with_stdlib();
    vm.run_chunk(&mut (), &bytecode).unwrap();

    let mut out = Vec::with_capacity(8);

    // 1. 1,000 warmed `return x + 1`
    let f_add = vm.get_global("bench_add").unwrap();
    for _ in 0..16 {
        vm.call_into(&mut (), f_add, &[Value::Number(1.0)], &mut out)
            .unwrap();
    }
    let (add_allocs, add_bytes) = measure(|| {
        for _ in 0..1000 {
            vm.call_into(&mut (), f_add, &[Value::Number(41.0)], &mut out)
                .unwrap();
        }
    });

    // 2. 1,000 warmed 1-argument `pcall`
    let f_pcall = vm.get_global("bench_pcall").unwrap();
    for _ in 0..16 {
        vm.call_into(&mut (), f_pcall, &[Value::Number(1.0)], &mut out)
            .unwrap();
    }
    let (pcall_allocs, pcall_bytes) = measure(|| {
        for _ in 0..1000 {
            vm.call_into(&mut (), f_pcall, &[Value::Number(41.0)], &mut out)
                .unwrap();
        }
    });

    // 3. 1,000 warmed 8-element `table.sort` with Lua comparator
    let f_sort = vm.get_global("bench_sort8").unwrap();
    for _ in 0..16 {
        vm.call_into(&mut (), f_sort, &[], &mut out).unwrap();
    }
    let (sort_allocs, sort_bytes) = measure(|| {
        for _ in 0..1000 {
            vm.call_into(&mut (), f_sort, &[], &mut out).unwrap();
        }
    });

    // 4. 10 `gfind` scans for "." on strings of length 64, 128, 256
    let f_gfind = vm.get_global("bench_gfind").unwrap();
    let s64 = Value::String(vm.intern_str(&"a".repeat(64)).unwrap());
    let s128 = Value::String(vm.intern_str(&"a".repeat(128)).unwrap());
    let s256 = Value::String(vm.intern_str(&"a".repeat(256)).unwrap());
    vm.call_into(&mut (), f_gfind, &[s64], &mut out).unwrap();
    let (g64_allocs, g64_bytes) = measure(|| {
        for _ in 0..10 {
            vm.call_into(&mut (), f_gfind, &[s64], &mut out).unwrap();
        }
    });
    let (g128_allocs, g128_bytes) = measure(|| {
        for _ in 0..10 {
            vm.call_into(&mut (), f_gfind, &[s128], &mut out).unwrap();
        }
    });
    let (g256_allocs, g256_bytes) = measure(|| {
        for _ in 0..10 {
            vm.call_into(&mut (), f_gfind, &[s256], &mut out).unwrap();
        }
    });

    // 5. 1,000 warmed `"level.x." .. n` whose result string already exists: the
    //    number is formatted on the stack and the concatenation buffer is reused.
    let f_concat = vm.get_global("bench_concat").unwrap();
    let existing = vm.intern_str("level.x.17").unwrap();
    let keep = vm.create_ref(Value::String(existing)).unwrap();
    for _ in 0..16 {
        vm.call_into(&mut (), f_concat, &[Value::Number(17.0)], &mut out)
            .unwrap();
    }
    let (concat_allocs, concat_bytes) = measure(|| {
        for _ in 0..1000 {
            vm.call_into(&mut (), f_concat, &[Value::Number(17.0)], &mut out)
                .unwrap();
        }
    });
    assert!(matches!(out[0], Value::String(s) if s == existing));
    assert!(vm.release_ref(keep));

    // 6. A listener creating a capturing closure on every call: the host's window of
    //    18 callbacks, and 1,000 more, after warm-up.
    let f_capture = vm.get_global("bench_capture").unwrap();
    for i in 0..5_000 {
        vm.call_into(&mut (), f_capture, &[Value::Number(i as f32)], &mut out)
            .unwrap();
    }
    let (capture18_allocs, capture18_bytes) = measure(|| {
        for i in 0..18 {
            vm.call_into(&mut (), f_capture, &[Value::Number(i as f32)], &mut out)
                .unwrap();
        }
    });
    let (capture1000_allocs, _) = measure(|| {
        for i in 0..1_000 {
            vm.call_into(&mut (), f_capture, &[Value::Number(i as f32)], &mut out)
                .unwrap();
        }
    });
    assert_eq!(
        capture18_allocs, 0,
        "18 warmed callbacks creating a capturing closure must not allocate ({capture18_bytes} bytes)"
    );
    assert_eq!(capture1000_allocs, 0, "1,000 more such callbacks must not allocate");

    for tick in 0..18 {
        vm.begin_tick(tick).unwrap();
        vm.call_into(&mut (), f_capture, &[Value::Number(3.)], &mut out)
            .unwrap();
    }
    let (ticks, bytes) = measure(|| {
        for tick in 18..36 {
            vm.begin_tick(tick).unwrap();
            vm.call_into(&mut (), f_capture, &[Value::Number(3.)], &mut out)
                .unwrap();
        }
    });
    assert_eq!(
        ticks, 0,
        "18 warmed simulation ticks allocated {bytes} bytes"
    );

    let (digest_allocs, _) = measure(|| {
        std::hint::black_box(vm.state_digest());
    });
    assert_eq!(digest_allocs, 0, "state_digest must not allocate");
    assert_eq!(concat_allocs, 0, "warmed concat of an existing string must not allocate ({concat_bytes} bytes)");

    eprintln!(
        "ALLOC_BENCH add=({add_allocs},{add_bytes}) pcall=({pcall_allocs},{pcall_bytes}) sort8=({sort_allocs},{sort_bytes}) gfind64=({g64_allocs},{g64_bytes}) gfind128=({g128_allocs},{g128_bytes}) gfind256=({g256_allocs},{g256_bytes})"
    );

    assert_eq!(add_allocs, 0, "warmed return x+1 must not allocate");
    assert_eq!(pcall_allocs, 0, "warmed 1-arg pcall must not allocate");
    assert_eq!(
        sort_allocs, 0,
        "warmed 8-element table.sort with Lua comparator must not allocate"
    );
    assert_eq!(
        g64_allocs, g128_allocs,
        "gfind allocations must not grow with input length"
    );
    assert_eq!(
        g128_allocs, g256_allocs,
        "gfind allocations must not grow with input length"
    );
    assert_eq!(
        g256_bytes, 0,
        "warmed gfind over interned matches must not copy input or pattern"
    );
}
