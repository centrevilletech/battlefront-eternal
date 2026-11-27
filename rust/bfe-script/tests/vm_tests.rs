//! Differential, safety, GC, determinism, host-interface, and recording-host tests for `bfe-script`.

use bfe_wide::Wide;
use std::collections::BTreeMap;

use bfe_script::{
    archive_host_functions, load_chunk, load_chunk_with_limits, record_manifest,
    record_manifest_with_config, scan_archive, ProfileAnswer,
    ArchiveError, ArchiveLimits, ChunkLimits, ImportProfile, LoadError, Manifest, RecordedValue,
    RecorderLimits, RecorderOptions, ScriptArchive, ScriptError, ScriptRng, Value, ValueRef, Vm,
    VmLimits, CANONICAL_NAN_BITS, MANIFEST_SCHEMA_VERSION,
};

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/lua/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"))
}

fn fixture_text(name: &str) -> String {
    let path = format!("{}/tests/lua/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture {path}: {e}"))
}

// ---------------------------------------------------------------------------
// 1. Differential checks against the reference Lua 5.0.3 (float) interpreter
// ---------------------------------------------------------------------------

fn run_differential(base: &str) {
    let bytecode = fixture_bytes(&format!("{base}.luac"));
    let expected = fixture_text(&format!("{base}.expected.txt"));

    let mut vm = Vm::new_with_stdlib();
    vm.capture_output();
    vm.run_chunk(&mut (), &bytecode)
        .unwrap_or_else(|e| panic!("{base} failed in bfe-script VM: {e}"));
    let actual = String::from_utf8(vm.take_output()).expect("test output is UTF-8");
    assert_eq!(actual, expected, "output mismatch for {base}");
}

#[test]
fn diff_01_arith_and_coercion() {
    run_differential("01_arith_and_coercion");
}

#[test]
fn diff_02_control_and_loops() {
    run_differential("02_control_and_loops");
}

#[test]
fn diff_03_closures_upvalues_varargs() {
    run_differential("03_closures_upvalues_varargs");
}

#[test]
fn diff_04_tables_and_metatables() {
    run_differential("04_tables_and_metatables");
}

#[test]
fn diff_05_strings_and_patterns() {
    run_differential("05_strings_and_patterns");
}

#[test]
fn diff_06_math_and_errors() {
    run_differential("06_math_and_errors");
}

#[test]
fn diff_10_multiret_adjust() {
    run_differential("10_multiret_adjust");
}

#[test]
fn diff_11_gc_rooting() {
    run_differential("11_gc_rooting");
}

#[test]
fn diff_12_stdlib_semantics() {
    run_differential("12_stdlib_semantics");
}

#[test]
fn diff_13_weak_and_gc() {
    run_differential("13_weak_and_gc");
}

#[test]
fn diff_14_errors_and_frames() {
    run_differential("14_errors_and_frames");
}

#[test]
fn diff_17_chunk_names() {
    run_differential("17_chunk_names");
}

#[test]
fn diff_19_integer_arguments() {
    run_differential("19_integer_arguments");
}

/// `setfenv`/`getfenv` levels that convert to 0: only an argument that is exactly 0
/// replaces the globals; `0.5`, `-0.5`, `"0.75"` select the running native.
#[test]
fn diff_24_environment_levels() {
    run_differential("24_environment_levels");
}

#[test]
fn diff_26_weak_mode() {
    run_differential("26_weak_mode");
}

/// `work_used` reports the last call's work measured from the budget that call
/// started with, even after the host changes the budget for the next call.
#[test]
fn work_used_is_measured_from_the_calls_own_budget() {
    let mut vm = Vm::new_with_stdlib();
    vm.run_chunk(&mut (), &fixture_bytes("27_periodic.luac"))
        .unwrap();
    let step = vm.get_global("step").unwrap();
    vm.set_instruction_budget(VmLimits::LOAD_BUDGET);
    vm.call(&mut (), step, &[Value::Number(1.0)]).unwrap();
    let first = vm.work_used();
    assert!(first > 0 && first < 64, "{first}");
    vm.set_instruction_budget(VmLimits::TICK_BUDGET);
    assert_eq!(vm.work_used(), first);
    vm.call(&mut (), step, &[Value::Number(2.0)]).unwrap();
    let second = vm.work_used();
    assert!(second > 0 && second < 64, "{second}");
    vm.set_instruction_budget(VmLimits::LOAD_BUDGET);
    assert_eq!(vm.work_used(), second);
}

#[test]
fn refusal_names_the_budget_the_call_started_with() {
    fn change_next(ctx: &mut bfe_script::CallCtx<'_, ()>) -> Result<(), ScriptError> {
        ctx.vm_mut().set_instruction_budget(500);
        ctx.charge_work(100)
    }
    let mut vm = Vm::new_with_stdlib();
    let f = vm.register_fn("change_next", change_next).unwrap();
    vm.set_instruction_budget(20);
    assert!(matches!(
        vm.call(&mut (), Value::NativeFn(f), &[]),
        Err(ScriptError::InstructionLimit { limit: 20 })
    ));
    assert_eq!(vm.work_used(), 20);
    vm.call(&mut (), Value::NativeFn(f), &[]).unwrap();
}

/// Work units are 32-bit: a call that spends nearly the largest budget succeeds and reports its
/// work, one that asks for more is refused with the budget named, and nothing wraps at the bound.
#[test]
fn work_at_the_largest_budget_stops_at_the_bound() {
    fn nearly(ctx: &mut bfe_script::CallCtx<'_, ()>) -> Result<(), ScriptError> {
        ctx.charge_work(u32::MAX - 1000)
    }
    fn past(ctx: &mut bfe_script::CallCtx<'_, ()>) -> Result<(), ScriptError> {
        ctx.charge_work(u32::MAX - 1000)?;
        ctx.charge_work(2000)
    }
    let mut vm = Vm::new_with_stdlib();
    let nearly = vm.register_fn("nearly", nearly).unwrap();
    let past = vm.register_fn("past", past).unwrap();
    vm.set_instruction_budget(u32::MAX);
    vm.call(&mut (), Value::NativeFn(nearly), &[]).unwrap();
    assert!(vm.work_used() >= u32::MAX - 1000 && vm.work_used() < u32::MAX, "{}", vm.work_used());
    vm.set_instruction_budget(u32::MAX);
    assert!(matches!(
        vm.call(&mut (), Value::NativeFn(past), &[]),
        Err(ScriptError::InstructionLimit { limit: u32::MAX })
    ));
    assert_eq!(vm.work_used(), u32::MAX);
}

#[test]
fn discarded_native_handles_are_reported_without_interrupting_collection() {
    fn noop(_: &mut bfe_script::CallCtx<'_, ()>) -> Result<(), ScriptError> {
        Ok(())
    }
    let mut vm = Vm::new_with_stdlib();
    let handle = vm.create_native_fn(noop).unwrap();
    let before = vm.collector_stats();
    let (persistent, root) = vm.create_rooted_native_fn(noop).unwrap();
    vm.collect_garbage();
    assert_eq!(
        vm.collector_stats().discarded_host_natives,
        before.discarded_host_natives + 1
    );
    assert!(matches!(
        vm.call(&mut (), Value::NativeFn(handle), &[]),
        Err(ScriptError::InvalidHandle(_))
    ));
    vm.call(&mut (), Value::NativeFn(persistent), &[]).unwrap();
    assert!(vm.release_ref(root));
}

#[test]
fn pinning_keeps_the_incoming_object_alive_during_registry_growth() {
    let mut vm = Vm::<()>::new();
    vm.intern_bytes(&[42; 8192]).unwrap();
    let object = Value::Table(vm.create_table().unwrap());
    vm.set_limits(VmLimits {
        memory_limit: vm.memory_bytes(),
        ..vm.limits()
    })
    .unwrap();
    let before = vm.collector_stats();
    let root = vm.create_ref(object).unwrap();
    assert!(vm.collector_stats().full_collections > before.full_collections);
    assert!(vm.is_valid_value(vm.get_ref(root).unwrap()));
    vm.collect_garbage();
    assert!(vm.is_valid_value(object));
}

/// The same function rooted with `create_ref`, passed into a call or stored in a
/// global survives collection, and a temporary one the VM saw may be collected.
#[test]
fn rooted_host_natives_survive_collection() {
    fn one(ctx: &mut bfe_script::CallCtx<'_, ()>) -> Result<(), ScriptError> {
        ctx.push_return(Value::Number(1.0))
    }
    let mut vm = Vm::new_with_stdlib();
    let rooted = vm.create_native_fn(one).unwrap();
    let r = vm.create_ref(Value::NativeFn(rooted)).unwrap();
    let temporary = vm.create_native_fn(one).unwrap();
    vm.call(&mut (), Value::NativeFn(temporary), &[]).unwrap();
    vm.register_fn("one", one).unwrap();
    vm.collect_garbage();
    let f = vm.get_ref(r).unwrap();
    let out = vm.call(&mut (), f, &[]).unwrap();
    assert!(matches!(out[..], [Value::Number(n)] if n == 1.0));
    assert!(vm.call(&mut (), Value::NativeFn(temporary), &[]).is_err());
}

/// Work that used to be charged after it was done, or not at all, is charged before it
/// is done in proportion to its size: moving a tail call's arguments, opening and
/// closing upvalues, inserting a `__call` metamethod, copying a long name into an error
/// message. Each pair of fixture functions differs only by that work.
#[test]
fn late_work_is_charged_in_proportion() {
    let mut vm = Vm::new_with_stdlib();
    vm.run_chunk(&mut (), &fixture_bytes("28_late_work.luac"))
        .unwrap();
    let mut used = |name: &str, arg: &str| -> u32 {
        let func = vm.get_global(name).unwrap();
        let args = if arg.is_empty() {
            Vec::new()
        } else {
            vec![vm.get_global(arg).unwrap()]
        };
        vm.call(&mut (), func, &args).unwrap();
        vm.work_used()
    };
    // 241 values move down, 30 units, plus the frame's setup.
    let (tail, notail) = (used("tail", "MANY"), used("notail", "MANY"));
    assert!(tail >= notail + 25, "tail call {tail}, plain call {notail}");
    // Eight cells opened (one unit each) and closed (a quarter each).
    let closes = |vm: &mut Vm, n: f32| -> u32 {
        let func = vm.get_global("closes").unwrap();
        vm.call(&mut (), func, &[Value::Number(n)]).unwrap();
        vm.work_used()
    };
    let (with_cells, without) = (closes(&mut vm, 1.0), closes(&mut vm, 0.0));
    assert!(with_cells >= without + 10, "{with_cells} with cells, {without} without");
    // 241 values move up to insert the metamethod.
    let mut used = |name: &str, arg: &str| -> u32 {
        let func = vm.get_global(name).unwrap();
        let args = vec![vm.get_global(arg).unwrap()];
        vm.call(&mut (), func, &args).unwrap();
        vm.work_used()
    };
    let (meta, direct) = (used("meta_call", "MANY"), used("direct_call", "MANY"));
    assert!(meta >= direct + 25, "metamethod call {meta}, direct call {direct}");
    // A 4,095-byte name is read, copied into the message with its position and
    // interned, each charged by length.
    let mut used = |name: &str| -> u32 {
        let func = vm.get_global(name).unwrap();
        let out = vm.call(&mut (), func, &[]).unwrap();
        assert!(matches!(out[0], Value::Bool(false)));
        vm.work_used()
    };
    let (long, short) = (used("long_name"), used("short_name"));
    assert!(long >= short + 700, "long name {long}, short name {short}");
}

/// Every NaN a script can make, and every non-NaN exceptional result, in release
/// builds as well as debug: the printed values match the reference and every NaN
/// left in VM state has the canonical bits.
#[test]
fn diff_21_exceptional_arithmetic_has_canonical_nan_bits() {
    run_differential("21_exceptional_arithmetic");
    let mut vm: Vm = Vm::new_with_stdlib();
    vm.capture_output();
    vm.run_chunk(&mut (), &fixture_bytes("21_exceptional_arithmetic.luac"))
        .unwrap();
    let results = vm.get_global("results").unwrap().as_table().unwrap();
    let n = vm.table_getn(results).unwrap();
    assert_eq!(n, 97);
    let mut nans = Vec::new();
    for i in 1..=n {
        let v = vm.table_raw_get_int(results, i).unwrap().as_number().unwrap();
        if v.is_nan() {
            nans.push(i);
            assert_eq!(v.to_bits(), CANONICAL_NAN_BITS, "result {i}");
        }
    }
    assert_eq!(nans.len(), 37, "NaN results at {nans:?}");

    // Host paths: a native function returning a NaN with a payload and the sign
    // bit set, and such a NaN passed in as an argument, both arrive canonical.
    fn make_nan(ctx: &mut bfe_script::CallCtx<'_, ()>) -> Result<(), ScriptError> {
        ctx.push_return(Value::Number(f32::from_bits(0xFFC0_BEEF)))
    }
    let f = vm.register_fn("make_nan", make_nan).unwrap();
    let out = vm.call(&mut (), Value::NativeFn(f), &[]).unwrap();
    assert_eq!(out[0].as_number().unwrap().to_bits(), CANONICAL_NAN_BITS);
    let t = vm.create_table().unwrap();
    let table_lib = vm.get_global("table").unwrap().as_table().unwrap();
    let insert = vm.table_get_str(table_lib, "insert").unwrap();
    vm.call(
        &mut (),
        insert,
        &[Value::Table(t), Value::Number(f32::from_bits(0x7FA0_0001))],
    )
    .unwrap();
    assert_eq!(
        vm.table_raw_get_int(t, 1).unwrap().as_number().unwrap().to_bits(),
        CANONICAL_NAN_BITS
    );
}

/// Replace the root source name of a compiled chunk (children inherit it).
fn chunk_with_source(chunk: &[u8], source: &[u8]) -> Vec<u8> {
    let n = u32::from_le_bytes(chunk[18..22].try_into().unwrap()) as usize;
    let mut out = chunk[..18].to_vec();
    out.extend_from_slice(&((source.len() + 1) as u32).to_le_bytes());
    out.extend_from_slice(source);
    out.push(0);
    out.extend_from_slice(&chunk[22 + n..]);
    out
}

#[test]
fn error_locations_cut_long_multibyte_chunk_names_by_bytes() {
    // Expected names follow Lua 5.0.3 `luaO_chunkid`, which cuts by bytes (checked
    // against the reference interpreter): `@` keeps the last 52 bytes after "...",
    // `=` its leading 59 bytes, a literal source its first line up to 43 bytes.
    let chunk = fixture_bytes("17_chunk_names.luac");
    let euro = "\u{20ac}".as_bytes();
    let at_euro = [b"@".as_slice(), &euro.repeat(30), b".lua"].concat();
    let at_euro_id = [b"...".as_slice(), &euro.repeat(16), b".lua"].concat();
    let eq_euro = [b"=".as_slice(), &euro.repeat(25)].concat();
    let eq_euro_id = euro.repeat(25)[..59].to_vec();
    let lit_euro = euro.repeat(30);
    let lit_euro_id = [b"[string \"".as_slice(), &lit_euro[..43], b"...\"]"].concat();
    let lit_nl = b"xxxxxxxxxx\nsecond line".to_vec();
    let lit_nl_id = b"[string \"xxxxxxxxxx...\"]".to_vec();
    let at_latin1 = [b"@".as_slice(), &[0xE9u8; 70]].concat();
    let at_latin1_id = [b"...".as_slice(), &[0xE9u8; 52]].concat();
    let cases = [
        (at_euro, at_euro_id),
        (eq_euro, eq_euro_id),
        (lit_euro, lit_euro_id),
        (lit_nl, lit_nl_id),
        (at_latin1, at_latin1_id),
    ];
    for (source, id) in cases {
        let mut vm = Vm::new_with_stdlib();
        vm.capture_output();
        vm.run_chunk(&mut (), &chunk_with_source(&chunk, &source))
            .unwrap();
        let mut want = Vec::new();
        for (line, msg) in [
            (3, "first"),
            (5, "attempt to index local `t' (a nil value)"),
            (7, "attempt to perform arithmetic on a table value"),
        ] {
            want.extend_from_slice(&id);
            want.extend_from_slice(format!(":{line}: {msg}\n").as_bytes());
        }
        want.extend_from_slice(b"bad argument #1 to `?' (string expected, got no value)\n");
        assert_eq!(
            vm.take_output(),
            want,
            "source {:?}",
            String::from_utf8_lossy(&source)
        );
    }
}

// ---------------------------------------------------------------------------
// 2. Chunk loader robustness (never panics on corrupt or hostile inputs)
// ---------------------------------------------------------------------------

#[test]
fn loader_rejects_bad_headers_and_stock_double_chunks() {
    assert!(matches!(load_chunk(&[]), Err(LoadError::UnexpectedEof { .. })));
    assert!(matches!(load_chunk(b"not lua"), Err(LoadError::BadSignature)));

    let valid = fixture_bytes("01_arith_and_coercion.luac");
    assert!(load_chunk(&valid).is_ok());

    // Accepts retail BODY single trailing 0x00 byte, rejects other trailing bytes.
    let mut with_nul = valid.clone();
    with_nul.push(0);
    assert!(load_chunk(&with_nul).is_ok());

    let mut with_garbage = valid.clone();
    with_garbage.push(0x42);
    assert!(matches!(
        load_chunk(&with_garbage),
        Err(LoadError::TrailingBytes(1))
    ));

    let mut with_two_nuls = valid.clone();
    with_two_nuls.extend_from_slice(&[0, 0]);
    assert!(matches!(
        load_chunk(&with_two_nuls),
        Err(LoadError::TrailingBytes(2))
    ));

    // Wrong version (Lua 5.1 = 0x51).
    let mut bad_ver = valid.clone();
    bad_ver[4] = 0x51;
    assert_eq!(load_chunk(&bad_ver).unwrap_err(), LoadError::UnsupportedVersion(0x51));

    // Big-endian header.
    let mut big_end = valid.clone();
    big_end[5] = 0;
    assert_eq!(load_chunk(&big_end).unwrap_err(), LoadError::UnsupportedEndianness(0));

    // Stock Lua 5.0 double chunk (sizeof(lua_Number) = 8 instead of 4).
    let mut double_hdr = valid.clone();
    double_hdr[13] = 8;
    assert_eq!(
        load_chunk(&double_hdr).unwrap_err(),
        LoadError::HeaderMismatch {
            field: "sizeof(lua_Number)",
            expected: 4,
            found: 8
        }
    );

    // Bad test number.
    let mut bad_test = valid.clone();
    bad_test[14..18].copy_from_slice(&0.0f32.to_le_bytes());
    assert!(matches!(load_chunk(&bad_test), Err(LoadError::BadTestNumber(_))));

    // Truncated at every single prefix length must return Err, never panic.
    for cut in 0..valid.len() {
        assert!(
            load_chunk(&valid[..cut]).is_err(),
            "truncated chunk at byte {cut} unexpectedly succeeded"
        );
    }

    // Hostile huge count field must error cleanly without OOM.
    let mut huge_count = valid[..18].to_vec();
    huge_count.extend_from_slice(&0u32.to_le_bytes()); // source = null
    huge_count.extend_from_slice(&0i32.to_le_bytes()); // line_defined
    huge_count.extend_from_slice(&[0, 0, 0, 2]); // nups, numparams, is_vararg, maxstack
    huge_count.extend_from_slice(&0x7FFF_FFFFi32.to_le_bytes()); // bogus lineinfo count
    assert!(matches!(
        load_chunk(&huge_count),
        Err(LoadError::UnexpectedEof { .. })
    ));
}

#[test]
fn archive_reader_rejects_invalid_headers() {
    assert_eq!(ScriptArchive::parse(&[]).unwrap_err(), ArchiveError::InvalidHeader);
    assert_eq!(
        ScriptArchive::parse(b"NOPE\x00\x00\x00\x00").unwrap_err(),
        ArchiveError::InvalidHeader
    );
    // Truncated ucfb body length.
    let mut trunc = Vec::new();
    trunc.extend_from_slice(b"ucfb");
    trunc.extend_from_slice(&1000u32.to_le_bytes());
    trunc.extend_from_slice(&[0u8; 8]);
    assert_eq!(
        ScriptArchive::parse(&trunc).unwrap_err(),
        ArchiveError::TruncatedChunk { offset: 0 }
    );
}

#[test]
fn cross_arch_bit_identity_for_v8math_nan_and_default_rng() {
    let mut vm: Vm = Vm::new_with_stdlib();
    let math_tbl = vm.get_global("math").unwrap().as_table().unwrap();

    let call1 = |vm: &mut Vm, name: &str, arg: f32| -> u32 {
        let f = vm.table_get_str(math_tbl, name).unwrap();
        vm.call(&mut (), f, &[Value::Number(arg)]).unwrap()[0]
            .as_number()
            .unwrap()
            .to_bits()
    };
    let call2 = |vm: &mut Vm, name: &str, a: f32, b: f32| -> u32 {
        let f = vm.table_get_str(math_tbl, name).unwrap();
        vm.call(&mut (), f, &[Value::Number(a), Value::Number(b)])
            .unwrap()[0]
            .as_number()
            .unwrap()
            .to_bits()
    };

    // Literal IEEE-754 f32 reference bit patterns (identical on x86-64 and ARM64).
    assert_eq!(call1(&mut vm, "sin", 1.0), 0x3F57_6AA4);
    assert_eq!(call1(&mut vm, "cos", 1.0), 0x3F0A_5140);
    assert_eq!(call1(&mut vm, "tan", 0.75), 0x3F6E_7D1B);
    assert_eq!(call1(&mut vm, "exp", 2.5), 0x4142_EB7F);
    assert_eq!(call1(&mut vm, "log", 10.0), 0x4013_5D8E);
    assert_eq!(call2(&mut vm, "pow", 3.5, 1.25), 0x4099_310F);
    assert_eq!(call2(&mut vm, "atan2", 1.5, -2.5), 0x4026_799F);

    // Every NaN entering VM state (hardware sqrt, division 0/0, host-supplied non-canonical NaNs)
    // must observe exact canonical quiet NaN bits `0x7FC0_0000`.
    assert_eq!(CANONICAL_NAN_BITS, 0x7FC0_0000);
    assert_eq!(call1(&mut vm, "sqrt", -0.1), CANONICAL_NAN_BITS);
    assert_eq!(call1(&mut vm, "acos", 2.0), CANONICAL_NAN_BITS);
    assert_eq!(call2(&mut vm, "mod", 1.0, 0.0), CANONICAL_NAN_BITS);
    assert_eq!(
        call1(&mut vm, "abs", f32::from_bits(0xFFC0_1234)),
        CANONICAL_NAN_BITS
    );

    // Host-injected non-canonical NaNs stored in tables/refs are canonicalized on entry.
    let neg_nan = f32::from_bits(0xFFC0_0000);
    let payload_nan = f32::from_bits(0xFFC0_BEEF);
    let t = vm.create_table().unwrap();
    vm.table_raw_set(t, Value::Number(1.0), Value::Number(neg_nan))
        .unwrap();
    vm.table_raw_set(t, Value::Number(2.0), Value::Number(payload_nan))
        .unwrap();
    assert_eq!(
        vm.table_raw_get_int(t, 1).unwrap().as_number().unwrap().to_bits(),
        CANONICAL_NAN_BITS
    );
    assert_eq!(
        vm.table_raw_get_int(t, 2).unwrap().as_number().unwrap().to_bits(),
        CANONICAL_NAN_BITS
    );
    let r = vm.create_ref(Value::Number(payload_nan)).unwrap();
    assert_eq!(
        vm.get_ref(r).unwrap().as_number().unwrap().to_bits(),
        CANONICAL_NAN_BITS
    );

    // Verify Lua bytecode OP_DIV (0.0 / 0.0), OP_SUB (inf - inf), and OP_UNM (-NaN) produce
    // exact canonical NaN bits `0x7FC0_0000` (x86-64 hardware division yields `0xFFC0_0000`).
    let mut nan_chunk = Vec::new();
    nan_chunk.extend_from_slice(&bfe_script::chunk::LUA_SIGNATURE);
    nan_chunk.extend_from_slice(&[0x50, 1, 4, 4, 4, 6, 8, 9, 9, 4]);
    nan_chunk.extend_from_slice(&bfe_script::chunk::TEST_NUMBER.to_le_bytes());
    nan_chunk.extend_from_slice(&0u32.to_le_bytes()); // source = null
    nan_chunk.extend_from_slice(&0i32.to_le_bytes()); // line_defined
    nan_chunk.extend_from_slice(&[0, 0, 0, 4]); // nups=0, num_params=0, is_vararg=0, max_stack=4
    nan_chunk.extend_from_slice(&0i32.to_le_bytes()); // line_info
    nan_chunk.extend_from_slice(&0i32.to_le_bytes()); // local_vars
    nan_chunk.extend_from_slice(&0i32.to_le_bytes()); // upvalue_names
    nan_chunk.extend_from_slice(&2i32.to_le_bytes()); // 2 constants: 0.0, +inf
    nan_chunk.push(3);
    nan_chunk.extend_from_slice(&0.0f32.to_le_bytes());
    nan_chunk.push(3);
    nan_chunk.extend_from_slice(&f32::INFINITY.to_le_bytes());
    nan_chunk.extend_from_slice(&0i32.to_le_bytes()); // protos
    // R0 = 0.0 / 0.0; R1 = inf - inf; R2 = -R0; return R0, R1, R2
    let enc_abc = |op: u32, a: u32, b: u32, c: u32| -> u32 {
        op | (c << 6) | (b << 15) | (a << 24)
    };
    let k0 = 128u32;
    let k1 = 129u32;
    let code = [
        enc_abc(15, 0, k0, k0), // OP_DIV R0 = K0 / K0
        enc_abc(13, 1, k1, k1), // OP_SUB R1 = K1 - K1
        enc_abc(17, 2, 0, 0),   // OP_UNM R2 = -R0
        enc_abc(27, 0, 4, 0),   // OP_RETURN R0..R2 (B=4 => 3 returns)
    ];
    nan_chunk.extend_from_slice(&(code.len() as i32).to_le_bytes());
    for ins in code {
        nan_chunk.extend_from_slice(&ins.to_le_bytes());
    }
    let nan_res = vm.run_chunk(&mut (), &nan_chunk).unwrap();
    assert_eq!(nan_res.len(), 3);
    assert_eq!(nan_res[0].as_number().unwrap().to_bits(), CANONICAL_NAN_BITS);
    assert_eq!(nan_res[1].as_number().unwrap().to_bits(), CANONICAL_NAN_BITS);
    assert_eq!(nan_res[2].as_number().unwrap().to_bits(), CANONICAL_NAN_BITS);

    // Canonical NaN string formatting ignores hardware NaN sign bit differences.
    let tostring_fn = vm.get_global("tostring").unwrap();
    let pos_nan = f32::from_bits(CANONICAL_NAN_BITS);
    let s1 = vm.call(&mut (), tostring_fn, &[Value::Number(pos_nan)]).unwrap()[0]
        .as_string()
        .unwrap();
    let s2 = vm.call(&mut (), tostring_fn, &[Value::Number(neg_nan)]).unwrap()[0]
        .as_string()
        .unwrap();
    assert_eq!(vm.string_str(s1), Some("nan"));
    assert_eq!(vm.string_str(s2), Some("nan"));

    // DefaultRng produces literal deterministic sequence for seed 12345.
    let seed_fn = vm.table_get_str(math_tbl, "randomseed").unwrap();
    let rand_fn = vm.table_get_str(math_tbl, "random").unwrap();
    vm.call(&mut (), seed_fn, &[Value::Number(12345.0)]).unwrap();
    let r1 = vm.call(&mut (), rand_fn, &[]).unwrap()[0].as_number().unwrap().to_bits();
    let r2 = vm.call(&mut (), rand_fn, &[]).unwrap()[0].as_number().unwrap().to_bits();
    let r3 = vm.call(&mut (), rand_fn, &[]).unwrap()[0].as_number().unwrap().to_bits();
    assert_eq!(r1, 0x3F33_6D47);
    assert_eq!(r2, 0x3D87_C4A8);
    assert_eq!(r3, 0x3E8E_23EA);
    vm.call(&mut (), seed_fn, &[Value::Number(12345.0)]).unwrap();
    let r1_again = vm.call(&mut (), rand_fn, &[]).unwrap()[0].as_number().unwrap().to_bits();
    let r2_again = vm.call(&mut (), rand_fn, &[]).unwrap()[0].as_number().unwrap().to_bits();
    assert_eq!((r1, r2), (r1_again, r2_again));
}

// ---------------------------------------------------------------------------
// 3. VM safety limits (instruction budget, stack overflow, memory limit) & GC
// ---------------------------------------------------------------------------

#[test]
fn vm_enforces_instruction_stack_and_memory_limits_even_inside_pcall() {
    let bytecode = fixture_bytes("07_limits_and_gc.luac");

    // Instruction limit cannot be caught by pcall.
    let mut vm: Vm = Vm::with_limits(VmLimits {
        instruction_budget: 2_000,
        ..VmLimits::default()
    })
    .unwrap();
    vm.open_stdlib().unwrap();
    vm.run_chunk(&mut (), &bytecode).unwrap();

    let f_loop = vm.get_global("infinite_loop").unwrap();
    assert!(matches!(
        vm.call(&mut (), f_loop, &[]),
        Err(ScriptError::InstructionLimit { limit: 2_000 })
    ));
    let f_loop_pcall = vm.get_global("infinite_loop_pcall").unwrap();
    assert!(matches!(
        vm.call(&mut (), f_loop_pcall, &[]),
        Err(ScriptError::InstructionLimit { limit: 2_000 })
    ));

    // Call depth limit cannot be caught by pcall.
    let mut vm: Vm = Vm::with_limits(VmLimits {
        max_call_depth: 32,
        ..VmLimits::default()
    })
    .unwrap();
    vm.open_stdlib().unwrap();
    vm.run_chunk(&mut (), &bytecode).unwrap();

    let f_rec = vm.get_global("deep_recurse").unwrap();
    assert!(matches!(
        vm.call(&mut (), f_rec, &[Value::Number(0.0)]),
        Err(ScriptError::StackOverflow { limit: 32 })
    ));
    let f_rec_pcall = vm.get_global("deep_recurse_pcall").unwrap();
    assert!(matches!(
        vm.call(&mut (), f_rec_pcall, &[]),
        Err(ScriptError::StackOverflow { limit: 32 })
    ));

    // Memory limit cannot be caught by pcall.
    let mut vm: Vm = Vm::with_limits(VmLimits {
        memory_limit: 128 * 1024,
        max_stack_slots: 1024,
        max_call_depth: 200,
        ..VmLimits::default()
    })
    .unwrap();
    vm.open_stdlib().unwrap();
    vm.run_chunk(&mut (), &bytecode).unwrap();

    let f_mem = vm.get_global("memory_bomb").unwrap();
    assert!(matches!(
        vm.call(&mut (), f_mem, &[]),
        Err(ScriptError::MemoryLimit { .. })
    ));
    let f_mem_pcall = vm.get_global("memory_bomb_pcall").unwrap();
    assert!(matches!(
        vm.call(&mut (), f_mem_pcall, &[]),
        Err(ScriptError::MemoryLimit { .. })
    ));
}

#[test]
fn gc_collects_cyclic_tables_and_closures_under_tight_memory_limit() {
    let bytecode = fixture_bytes("07_limits_and_gc.luac");
    let mut vm: Vm = Vm::with_limits(VmLimits {
        memory_limit: 256 * 1024,
        max_stack_slots: 1024,
        max_call_depth: 200,
        ..VmLimits::default()
    })
    .unwrap();
    vm.open_stdlib().unwrap();
    vm.run_chunk(&mut (), &bytecode).unwrap();

    // 4000 cyclic table+closure groups would exceed 2 MiB without GC,
    // yet fit cleanly in 256 KiB because mark-and-sweep reclaims cycles.
    let f_churn = vm.get_global("cycle_churn").unwrap();
    let res = vm.call(&mut (), f_churn, &[Value::Number(4000.0)]).unwrap();
    // sum_{i=1}^{4000} (2*i) = 4000 * 4001 = 16004000 (exact in f32!)
    assert_eq!(res[0].as_number(), Some(16_004_000.0));
}

// ---------------------------------------------------------------------------
// 4. Host interface: native functions, tables, ValueRef pinning, table order, RNG
// ---------------------------------------------------------------------------

struct TestHost {
    callbacks: BTreeMap<String, ValueRef>,
    factor: f32,
}

struct FixedStepRng {
    step: u32,
}

impl ScriptRng for FixedStepRng {
    fn seed(&mut self, seed: i32) {
        self.step = seed as u32;
    }

    fn next_unit(&mut self) -> f32 {
        self.step = self.step.wrapping_add(1);
        ((self.step % 10) as f32) / 10.0
    }
}

#[test]
fn host_interface_callbacks_tables_deterministic_order_and_rng() {
    let bytecode = fixture_bytes("08_host_interop.luac");
    let mut host = TestHost {
        callbacks: BTreeMap::new(),
        factor: 3.0,
    };
    let mut vm: Vm<TestHost> = Vm::with_limits(VmLimits::default()).unwrap();
    vm.open_stdlib().unwrap();
    vm.set_rng(Box::new(FixedStepRng { step: 0 }));

    vm.register_fn("host_register_callback", |ctx| {
        let name_id = ctx.check_string_id(0)?;
        let name = ctx.vm().string_latin1(name_id).expect("live string arg");
        let cb = ctx.check_function(1)?;
        let vref = ctx.vm_mut().create_ref(cb)?;
        ctx.host_mut().callbacks.insert(name, vref);
        Ok(())
    })
    .unwrap();

    vm.register_table(
        "host_lib",
        &[("triple", |ctx| {
            let x = ctx.check_number(0)?;
            let f = ctx.host().factor;
            ctx.push_return(Value::Number(x * f))
        })],
    )
    .unwrap();

    vm.run_chunk(&mut host, &bytecode).unwrap();

    // Calling Lua function from Rust and reading multiple return values into a reusable Vec.
    let sum_diff = vm.get_global("sum_and_diff").unwrap();
    let mut out = Vec::with_capacity(4);
    vm.call_into(
        &mut host,
        sum_diff,
        &[Value::Number(10.0), Value::Number(3.0)],
        &mut out,
    )
    .unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].as_number(), Some(13.0));
    assert_eq!(out[1].as_number(), Some(7.0));

    // Calling registered native table function from Lua.
    let call_lib = vm.get_global("call_host_lib").unwrap();
    vm.call_into(&mut host, call_lib, &[Value::Number(7.0)], &mut out)
        .unwrap();
    assert_eq!(out[0].as_number(), Some(21.0));

    // Register a closure callback, clear its global factory, force GC, and call via ValueRef!
    let install = vm.get_global("install_callback").unwrap();
    vm.call_into(&mut host, install, &[], &mut out).unwrap();
    vm.set_global("install_callback", Value::Nil).unwrap();
    vm.collect_garbage();

    let cb_ref = *host.callbacks.get("on_tick").expect("callback registered");
    let cb_val = vm.get_ref(cb_ref).expect("live ValueRef");
    vm.call_into(&mut host, cb_val, &[Value::Number(5.0)], &mut out)
        .unwrap();
    assert_eq!(out[0].as_number(), Some(105.0));
    vm.call_into(&mut host, cb_val, &[Value::Number(12.0)], &mut out)
        .unwrap();
    assert_eq!(out[0].as_number(), Some(117.0));

    assert!(vm.release_ref(cb_ref));
    assert!(!vm.release_ref(cb_ref));
    assert!(vm.get_ref(cb_ref).is_none());
    vm.collect_garbage();

    // Deterministic table order: array keys 1..=3 ascending, then map keys in insertion order.
    let build_tbl = vm.get_global("build_ordered_table").unwrap();
    vm.call_into(&mut host, build_tbl, &[], &mut out).unwrap();
    let tid = out[0].as_table().expect("table returned");
    assert_eq!(
        vm.string_str(
            vm.table_raw_get_int(tid, 1)
                .unwrap()
                .as_string()
                .unwrap()
        ),
        Some("one")
    );
    assert_eq!(vm.table_get_str(tid, "alpha").unwrap().as_number(), Some(2.0));

    let ordered_fn = vm.get_global("ordered_keys").unwrap();
    vm.call_into(&mut host, ordered_fn, &[Value::Table(tid)], &mut out)
        .unwrap();
    let order_str = vm.string_str(out[0].as_string().unwrap()).unwrap();
    assert_eq!(
        order_str,
        "1=one,2=two,3=three,zeta=1,alpha=2,middle=3,0=zero"
    );

    // Host-supplied deterministic ScriptRng hook.
    let roll = vm.get_global("roll_random").unwrap();
    vm.call_into(&mut host, roll, &[Value::Number(0.0)], &mut out)
        .unwrap();
    assert_eq!(out[0].as_number(), Some(0.1)); // step 1 -> 0.1
    assert_eq!(out[1].as_number(), Some(3.0)); // step 2 -> 0.2 * 10 = 2 -> +1 = 3
    assert_eq!(out[2].as_number(), Some(8.0)); // step 3 -> floor(0.3 * 11) + 5 = 8
}

// ---------------------------------------------------------------------------
// 5. Synthetic UCFB `.lvl` archive + Recording Host JSON contract test
// ---------------------------------------------------------------------------

#[path = "common/archive.rs"]
mod synthetic_archive;
use synthetic_archive::{build_synthetic_lvl, write_ucfb_chunk};

#[test]
fn recording_host_runs_setup_phases_and_emits_stable_json() {
    let entry_luac = fixture_bytes("09_recorder_entry.luac");
    let helper_luac = fixture_bytes("09_recorder_helper.luac");
    let lvl = build_synthetic_lvl(&[
        ("09_recorder_entry", &entry_luac),
        ("09_recorder_helper", &helper_luac),
    ]);

    let archive = ScriptArchive::parse(&lvl).unwrap();
    assert_eq!(archive.scripts().len(), 2);

    let manifest: Manifest = record_manifest(&archive, "09_recorder_entry").unwrap();
    assert_eq!(manifest.schema, MANIFEST_SCHEMA_VERSION);
    assert_eq!(manifest.profile, "pc-fresh");
    assert_eq!(manifest.script, "09_recorder_entry");
    assert!(manifest.unresolved_queries.is_empty());
    assert_eq!(
        manifest.loaded_scripts,
        vec![
            "09_recorder_entry",
            "09_recorder_helper",
            "09_recorder_helper"
        ]
    );
    assert_eq!(manifest.phases, vec!["chunk", "ScriptPreInit", "ScriptInit"]);

    // Verify ScriptPostLoad and OptionalUndefinedHook were never called.
    assert!(!manifest
        .calls
        .iter()
        .any(|c| c.name == "HostShouldNotRun" || c.name == "OptionalUndefinedHook"));

    // Verify exact call order and argument shapes.
    let names: Vec<&str> = manifest.calls.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "ScriptCB_DoFile",
            "ScriptCB_DoFile",
            "HostSetExtents",
            "HostLoadArchive",
            "HostAddItem",
            "HostAddItem",
            "HostRegister",
        ]
    );

    assert_eq!(manifest.calls[2].phase, "ScriptPreInit");
    assert_eq!(manifest.calls[2].args, vec![RecordedValue::Number(2000.0)]);

    assert_eq!(
        manifest.calls[4].args,
        vec![
            RecordedValue::Number(1.0),
            RecordedValue::String("class_a".to_string()),
            RecordedValue::Number(4.0),
        ]
    );

    // JSON parses cleanly and preserves lossless `$bfe` markers and stable key order.
    let json_text = manifest.to_json_pretty();
    let parsed: serde_json::Value =
        serde_json::from_str(&json_text).expect("manifest JSON must be valid");
    assert_eq!(parsed["schema"], MANIFEST_SCHEMA_VERSION);
    assert_eq!(parsed["profile"], "pc-fresh");
    assert_eq!(parsed["unresolved_queries"].as_array().unwrap().len(), 0);
    // The guarded hook stayed nil and is listed as probed; the include guard is
    // assigned by the helper and is not.
    assert_eq!(manifest.probed_globals, vec!["OptionalUndefinedHook"]);
    assert_eq!(parsed["probed_globals"], serde_json::json!(["OptionalUndefinedHook"]));
    assert_eq!(parsed["calls"][6]["name"], "HostRegister");
    assert_eq!(parsed["calls"][6]["args"][0]["flag"], true);
    assert_eq!(parsed["calls"][6]["args"][0]["cb"]["$bfe"], "function");
    assert_eq!(
        parsed["calls"][6]["args"][0]["self_ref"],
        serde_json::json!({"$bfe": "cycle", "up": 1})
    );
    assert_eq!(parsed["calls"][6]["args"][1], serde_json::Value::Null);
    assert_eq!(parsed["calls"][6]["args"][2], false);
    assert_eq!(parsed["calls"][6]["args"][3], 1.5);
    assert_eq!(parsed["calls"][6]["args"][4]["$bfe"], "function");
}

#[test]
fn recorder_profile_callable_bindings_and_lossless_schema_v2() {
    let entry_luac = fixture_bytes("16_recorder_v2.luac");
    let lib_luac = fixture_bytes("16_recorder_lib.luac");
    let lvl = build_synthetic_lvl(&[
        ("16_recorder_v2", &entry_luac),
        ("16_recorder_lib", &lib_luac),
    ]);
    let archive = ScriptArchive::parse(&lvl).unwrap();

    // 1. With `OptionalAPI` registered in profile options: `if OptionalAPI then` runs;
    //    the preparation step supplies a global and two answered queries.
    let opts = RecorderOptions {
        profile: ImportProfile::PcFresh,
        extra_host_functions: vec!["OptionalAPI".to_string()],
        answers: vec![
            ProfileAnswer::Global {
                name: "gPlatform".to_string(),
                value: RecordedValue::String("PC".to_string()),
            },
            ProfileAnswer::Call {
                name: "QueryPlatform".to_string(),
                value: RecordedValue::String("PC".to_string()),
            },
            ProfileAnswer::Call {
                name: "QuerySavedSetup".to_string(),
                value: RecordedValue::Bool(false),
            },
        ],
        ..RecorderOptions::default()
    };
    let manifest = record_manifest_with_config(&archive, "16_recorder_v2", opts).unwrap();
    assert_eq!(manifest.schema, MANIFEST_SCHEMA_VERSION);
    assert_eq!(manifest.profile, "pc-fresh");
    assert_eq!(manifest.probed_globals, vec!["AbsentOptionalHook"]);

    let call_names: Vec<&str> = manifest.calls.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        call_names,
        vec![
            "GuardedHostCall",
            "OptionalAPI",
            "TableStoredAPI",
            "QueryPlatform",
            "QuerySavedSetup",
            "RecordPlatform",
            "OpenAudioStream",
            "OpenAudioStream",
            "AudioStreamAppendSegments",
            "AudioStreamAppendSegments",
            "CreateTimer",
            "RecordTimerResult",
            "Record",
        ]
    );
    // AbsentOptionalHook is only called behind a test anywhere in the archive: it
    // stayed nil.
    assert!(!call_names.contains(&"AbsentOptionalHook"));
    // The supplied global and answers reached the script.
    assert_eq!(
        manifest.calls[5].args,
        vec![
            RecordedValue::String("PC".to_string()),
            RecordedValue::String("PC".to_string()),
            RecordedValue::Bool(false),
        ]
    );
    // Sequential deterministic audio stream handles (1 and 2) propagated cleanly.
    assert_eq!(
        manifest.calls[8].args,
        vec![
            RecordedValue::String("seg_a".to_string()),
            RecordedValue::Number(1.0),
        ]
    );
    assert_eq!(
        manifest.calls[9].args,
        vec![
            RecordedValue::String("seg_b".to_string()),
            RecordedValue::Number(2.0),
        ]
    );
    // Unresolved query surfaced with exact index and arity.
    assert_eq!(manifest.unresolved_queries.len(), 1);
    assert_eq!(manifest.unresolved_queries[0].name, "CreateTimer");
    assert_eq!(manifest.unresolved_queries[0].call_index, 10);
    assert_eq!(manifest.unresolved_queries[0].expected_results, Some(1));

    // Verify lossless Schema v2 JSON representation via an ordinary JSON parser.
    let json_text = manifest.to_json_pretty();
    let parsed: serde_json::Value = serde_json::from_str(&json_text).unwrap();
    let rec_args = parsed["calls"][12]["args"].as_array().unwrap();
    // Mixed-key table preserves all 4 entries without duplicate JSON keys.
    let mixed = &rec_args[0][0][0];
    assert_eq!(mixed["$bfe"], "table");
    let entries = mixed["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0], serde_json::json!([1, "n"]));
    assert_eq!(entries[1], serde_json::json!(["1", "s"]));
    assert_eq!(entries[2], serde_json::json!([true, "b"]));
    assert_eq!(entries[3], serde_json::json!(["true", "s2"]));
    // Negative zero is distinct from +0.
    assert_eq!(
        rec_args[1],
        serde_json::json!({"$bfe": "number", "special": "-0"})
    );
    // Raw non-UTF8 byte 0xE9 is distinct from UTF-8 "é" (0xC3 0xA9).
    assert_eq!(
        rec_args[2],
        serde_json::json!({"$bfe": "bytes", "hex": "e9"})
    );
    assert_eq!(rec_args[3], serde_json::json!("é"));
    // Ordinary Lua table containing reserved `"$bfe"` key is wrapped losslessly.
    assert_eq!(rec_args[4]["$bfe"], "table");
    assert_eq!(
        rec_args[4]["entries"],
        serde_json::json!([["$bfe", "bytes"], ["hex", "e9"]])
    );
    // Special floats nan, inf, -inf use `$bfe` number wrapper.
    assert_eq!(
        rec_args[5],
        serde_json::json!({"$bfe": "number", "special": "nan"})
    );
    assert_eq!(
        rec_args[6],
        serde_json::json!({"$bfe": "number", "special": "inf"})
    );
    assert_eq!(
        rec_args[7],
        serde_json::json!({"$bfe": "number", "special": "-inf"})
    );

    // 2. Without `OptionalAPI` in options, `if OptionalAPI then` sees nil while
    //    `TableStoredAPI` (untested, placed in array literal) still calls cleanly.
    let default_manifest = record_manifest(&archive, "16_recorder_v2").unwrap();
    let default_names: Vec<&str> = default_manifest
        .calls
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert!(!default_names.contains(&"OptionalAPI"));
    assert!(default_names.contains(&"TableStoredAPI"));
    assert!(default_names.contains(&"GuardedHostCall"));
    assert_eq!(
        default_manifest.probed_globals,
        vec!["AbsentOptionalHook", "OptionalAPI"]
    );
    // Nothing answers the two queries now: both are reported, and the global is nil.
    let unresolved: Vec<&str> = default_manifest
        .unresolved_queries
        .iter()
        .map(|q| q.name.as_str())
        .collect();
    assert_eq!(unresolved, vec!["QueryPlatform", "QuerySavedSetup", "CreateTimer"]);
    let record_platform = default_manifest
        .calls
        .iter()
        .find(|c| c.name == "RecordPlatform")
        .unwrap();
    assert_eq!(
        record_platform.args,
        vec![RecordedValue::Nil, RecordedValue::Nil, RecordedValue::Nil]
    );

    // 3. The derived names: unguarded calls anywhere in the archive, never assigned,
    //    not part of the standard library (the profile's own names are bound first).
    let derived = archive_host_functions(&archive, ChunkLimits::default());
    assert!(derived.contains(&"GuardedHostCall".to_string()));
    assert!(derived.contains(&"TableStoredAPI".to_string()));
    assert!(!derived.contains(&"AbsentOptionalHook".to_string()));
    assert!(!derived.contains(&"OptionalAPI".to_string()));
    assert!(!derived.contains(&"ScriptInit".to_string()));
    assert!(!derived.contains(&"UnusedHelper".to_string()));
    assert!(!derived.iter().any(|n| n == "string" || n == "print" || n == "rawget"));
}

/// A member that fails to decode is reported by the scan and in every manifest
/// recorded from the archive, instead of silently dropping its calls from the
/// derived bindings; a scan made once can be reused for each entry.
#[test]
fn recorder_reports_archive_members_it_could_not_scan() {
    let good = fixture_bytes("20_recorder_probes.luac");
    let lvl = build_synthetic_lvl(&[("good", &good), ("incomplete", b"\x1bLua")]);
    let archive = ScriptArchive::parse(&lvl).unwrap();
    let scan = scan_archive(&archive, ChunkLimits::default());
    assert_eq!(scan.skipped.len(), 1);
    assert_eq!(scan.skipped[0].0, "incomplete");
    let opts = RecorderOptions {
        scan: Some(std::rc::Rc::new(scan.clone())),
        ..RecorderOptions::default()
    };
    let manifest = record_manifest_with_config(&archive, "good", opts).unwrap();
    assert_eq!(manifest.skipped_scripts, scan.skipped);
    let json: serde_json::Value = serde_json::from_str(&manifest.to_json_pretty()).unwrap();
    assert_eq!(json["skipped_scripts"][0]["script"], "incomplete");
    assert!(record_manifest(&archive, "incomplete").is_err());
}

#[test]
fn recorder_sees_probes_calls_through_tables_and_g_and_marks_cycle_distance() {
    let luac = fixture_bytes("20_recorder_probes.luac");
    let lvl = build_synthetic_lvl(&[("20_recorder_probes", &luac)]);
    let archive = ScriptArchive::parse(&lvl).unwrap();
    assert_eq!(
        archive_host_functions(&archive, ChunkLimits::default()),
        vec!["FieldStored", "Record", "ViaG"]
    );
    let manifest = record_manifest(&archive, "20_recorder_probes").unwrap();
    let names: Vec<&str> = manifest.calls.iter().map(|c| c.name.as_str()).collect();
    // Calls through a table field and through `_G` are recorded; functions the
    // script assigns through `_G` or `rawset` are its own and record nothing.
    assert_eq!(names, vec!["FieldStored", "ViaG", "Record", "Record"]);
    assert_eq!(manifest.calls[0].args, vec![RecordedValue::Number(3.0)]);
    assert_eq!(
        manifest.probed_globals,
        vec!["OptionalApi", "RawProbe", "TestedViaG"]
    );
    let cycle = |up| RecordedValue::Cycle { up };
    assert_eq!(
        manifest.calls[2].args,
        vec![RecordedValue::Object(vec![(
            "inner".to_string(),
            RecordedValue::Object(vec![
                ("back".to_string(), cycle(2)),
                ("self".to_string(), cycle(1)),
            ])
        )])]
    );
    // A snapshot is a tree: a shared subtable is written at each place.
    let pair = RecordedValue::Array(vec![RecordedValue::Number(1.0), RecordedValue::Number(2.0)]);
    assert_eq!(
        manifest.calls[3].args,
        vec![RecordedValue::Array(vec![pair.clone(), pair])]
    );
    let json = manifest.to_json_pretty();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed["calls"][2]["args"][0]["inner"]["back"],
        serde_json::json!({"$bfe": "cycle", "up": 2})
    );
}

#[test]
fn stale_and_foreign_vm_handles_are_rejected_without_panicking() {
    let mut vm1: Vm = Vm::new_with_stdlib();
    let mut vm2: Vm = Vm::new_with_stdlib();

    // Allocate unrooted objects in vm1, then collect them and reuse their slots.
    let stale_str = vm1.intern_str("ephemeral_string_for_gc").unwrap();
    let stale_tbl = vm1.create_table().unwrap();
    let stale_ud = vm1.create_userdata(7, Wide::from_u32(42), None).unwrap();
    let stale_cl = vm1
        .load_chunk_bytes(&fixture_bytes("01_arith_and_coercion.luac"))
        .unwrap();
    let live_ref = vm1.create_ref(Value::Table(stale_tbl)).unwrap();
    assert!(vm1.release_ref(live_ref));

    vm1.collect_garbage();

    // Allocate fresh objects to force slot reuse in every arena.
    let fresh_str = vm1.intern_str("replacement_string").unwrap();
    let fresh_tbl = vm1.create_table().unwrap();
    let fresh_ud = vm1.create_userdata(9, Wide::from_u32(99), None).unwrap();
    let fresh_cl = vm1
        .load_chunk_bytes(&fixture_bytes("01_arith_and_coercion.luac"))
        .unwrap();
    assert!(vm1.string_str(fresh_str) == Some("replacement_string"));
    assert!(vm1.userdata_info(fresh_ud) == Some((9, Wide::from_u32(99))));
    assert!(vm1.is_valid_value(Value::Table(fresh_tbl)));
    assert!(vm1.is_valid_value(Value::Closure(fresh_cl)));

    // Stale handles must NOT alias the reused slots and must NOT panic!
    assert_eq!(vm1.string_bytes(stale_str), None);
    assert_eq!(vm1.string_str(stale_str), None);
    assert_eq!(vm1.string_latin1(stale_str), None);
    assert_eq!(vm1.userdata_info(stale_ud), None);
    assert!(matches!(
        vm1.table_raw_get(stale_tbl, Value::Number(1.0)),
        Err(ScriptError::InvalidHandle("table"))
    ));
    assert!(matches!(
        vm1.table_get_str(stale_tbl, "k"),
        Err(ScriptError::InvalidHandle("table"))
    ));
    assert!(matches!(
        vm1.table_raw_set(stale_tbl, Value::Number(1.0), Value::Number(2.0)),
        Err(ScriptError::InvalidHandle("table"))
    ));
    assert!(matches!(
        vm1.call(&mut (), Value::Closure(stale_cl), &[]),
        Err(ScriptError::InvalidHandle("closure"))
    ));
    assert!(matches!(
        vm1.create_ref(Value::Table(stale_tbl)),
        Err(ScriptError::InvalidHandle("table"))
    ));
    assert!(vm1.get_ref(live_ref).is_none());

    // Foreign-VM live handles from vm2 must be rejected by vm1.
    let foreign_str = vm2.intern_str("from_vm2").unwrap();
    let foreign_tbl = vm2.create_table().unwrap();
    let foreign_ud = vm2.create_userdata(1, Wide::from_u32(2), None).unwrap();
    let foreign_ref = vm2.create_ref(Value::Table(foreign_tbl)).unwrap();
    let foreign_fn = vm2.get_global("print").unwrap();

    assert_eq!(vm1.string_bytes(foreign_str), None);
    assert_eq!(vm1.userdata_info(foreign_ud), None);
    assert!(vm1.get_ref(foreign_ref).is_none());
    assert!(!vm1.release_ref(foreign_ref));
    assert!(matches!(
        vm1.table_raw_get(foreign_tbl, Value::Number(1.0)),
        Err(ScriptError::InvalidHandle("table"))
    ));
    assert!(matches!(
        vm1.table_raw_set(fresh_tbl, Value::String(foreign_str), Value::Number(1.0)),
        Err(ScriptError::InvalidHandle("string"))
    ));
    assert!(matches!(
        vm1.call(&mut (), foreign_fn, &[]),
        Err(ScriptError::InvalidHandle("native function"))
    ));
}

/// Debug builds need the documented 4 MiB stack on every thread that runs scripts. Test
/// threads get it from `RUST_MIN_STACK` in `.cargo/config.toml` at the repository root,
/// which Cargo also finds when the CI scripts start there with `--manifest-path`.
#[cfg(debug_assertions)]
#[test]
fn debug_test_threads_get_the_documented_stack() {
    let configured = std::env::var("RUST_MIN_STACK")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    assert!(configured >= 4 << 20, "RUST_MIN_STACK is {configured}, not 4 MiB");
    // Recurse until 3 MiB of this thread's stack is in use (measured from the
    // addresses of locals), more than Rust's default 2 MiB thread stack holds.
    fn deep(top: usize, target: usize) -> usize {
        let block = std::hint::black_box([1u8; 8 * 1024]);
        let used = top.abs_diff(block.as_ptr() as usize);
        if used >= target {
            used
        } else {
            deep(top, target) + usize::from(block[used % block.len()])
        }
    }
    let marker = std::hint::black_box([0u8; 16]);
    let used = deep(marker.as_ptr() as usize, 3 << 20);
    assert!(used >= 3 << 20);
}

fn digest_after_fixtures() -> Wide {
    let mut vm: Vm = Vm::new_with_stdlib();
    vm.capture_output();
    for name in [
        "04_tables_and_metatables.luac",
        "13_weak_and_gc.luac",
        "21_exceptional_arithmetic.luac",
    ] {
        vm.run_chunk(&mut (), &fixture_bytes(name)).unwrap();
    }
    vm.collect_garbage();
    vm.state_digest()
}

/// Two functions that differ only in a parameter name give different error messages,
/// so their states must digest differently (the digest covers debug names and ranges).
#[test]
fn state_digest_covers_debug_names() {
    let run = |fixture: &str| {
        let chunk = chunk_with_source(&fixture_bytes(fixture), b"@case.lua");
        let mut vm: Vm = Vm::new_with_stdlib();
        vm.run_chunk(&mut (), &chunk).unwrap();
        let digest = vm.state_digest();
        let f = vm.get_global("score").unwrap();
        let err = vm.call(&mut (), f, &[Value::Bool(false)]).unwrap_err();
        (digest, err.to_string())
    };
    let (a, ea) = run("25_digest_alpha.luac");
    let (b, eb) = run("25_digest_omega.luac");
    assert!(ea.contains("local `alpha'") && eb.contains("local `omega'"), "{ea} / {eb}");
    assert_ne!(a, b);
}

/// The whole deterministic VM state after the same programs is the same in two VMs,
/// and equals a fixed value on every architecture and build (if a change to the heap
/// layout, accounting or collector legitimately changes it, update the literal).
#[test]
fn state_digest_is_reproducible_and_architecture_independent() {
    let a = digest_after_fixtures();
    let b = digest_after_fixtures();
    assert_eq!(a, b);
    let mut vm: Vm = Vm::new_with_stdlib();
    let before = vm.state_digest();
    vm.set_global("x", Value::Number(1.0)).unwrap();
    assert_ne!(before, vm.state_digest());
    eprintln!("state digest {a:#018x}");
    assert_eq!(a, STATE_DIGEST);
}

const STATE_DIGEST: Wide = Wide::new(0x70d4_1c33, 0xdb1f_b4cb);

/// Natives that call natives (`pcall(pcall, ...)`, `__tostring = tostring`) stop at
/// the re-entry limit on a 2 MiB thread instead of overflowing the native stack,
/// in debug and release builds alike; a shallow chain still runs to completion.
#[test]
fn native_to_native_nesting_counts_toward_the_reentry_limit() {
    let bytecode = fixture_bytes("23_native_nesting.luac");
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let mut vm: Vm = Vm::new_with_stdlib();
            vm.run_chunk(&mut (), &bytecode).unwrap();
            for name in ["pcall_chain", "tostring_loop", "xpcall_chain"] {
                let f = vm.get_global(name).unwrap();
                let res = vm.call(&mut (), f, &[]);
                assert!(
                    matches!(res, Err(ScriptError::StackOverflow { limit: 64 })),
                    "{name}: {res:?}"
                );
            }
            let f = vm.get_global("shallow_chain").unwrap();
            let out = vm.call(&mut (), f, &[]).unwrap();
            assert!(matches!(out[0], Value::Bool(true)));
        })
        .unwrap()
        .join()
        .unwrap();
}

/// Native functions that hand the VM another VM's handle, or a handle kept across a
/// collection, get `InvalidHandle` from `push_return`, `call_into`, `pcall_into` and
/// `set_upvalue` instead of a panic or a silent alias.
#[test]
fn native_side_paths_reject_foreign_and_stale_handles() {
    #[derive(Default)]
    struct Holder {
        held: Option<Value>,
    }
    let mut vm2: Vm = Vm::new_with_stdlib();
    let foreign_tbl = Value::Table(vm2.create_table().unwrap());
    let foreign_fn = vm2.get_global("print").unwrap();

    let mut vm: Vm<Holder> = Vm::with_limits(VmLimits::default()).unwrap();
    vm.open_stdlib().unwrap();
    fn give_back(ctx: &mut bfe_script::CallCtx<'_, Holder>) -> Result<(), ScriptError> {
        let v = ctx.host().held.unwrap();
        ctx.push_return(v)
    }
    fn call_held(ctx: &mut bfe_script::CallCtx<'_, Holder>) -> Result<(), ScriptError> {
        let v = ctx.host().held.unwrap();
        let mut out = Vec::new();
        ctx.call_into(v, &[], &mut out)
    }
    fn pcall_held(ctx: &mut bfe_script::CallCtx<'_, Holder>) -> Result<(), ScriptError> {
        let v = ctx.host().held.unwrap();
        let mut out = Vec::new();
        ctx.pcall_into(Value::Nil, &[v], &mut out).map(|_| ())
    }
    fn store_held(ctx: &mut bfe_script::CallCtx<'_, Holder>) -> Result<(), ScriptError> {
        let v = ctx.host().held.unwrap();
        ctx.set_upvalue(0, v)
    }
    let give = Value::NativeFn(vm.create_native_fn(give_back).unwrap());
    let call = Value::NativeFn(vm.create_native_fn(call_held).unwrap());
    let pcall = Value::NativeFn(vm.create_native_fn(pcall_held).unwrap());
    let store = Value::NativeFn(
        vm.create_native_fn_with_upvals(store_held, vec![Value::Nil])
            .unwrap(),
    );
    let own = vm.create_table().unwrap();
    let r = vm.create_ref(Value::Table(own)).unwrap();
    let pins: Vec<_> = [give, call, pcall, store]
        .into_iter()
        .map(|f| vm.create_ref(f).unwrap())
        .collect();

    let mut host = Holder::default();
    let invalid = |res: Result<Vec<Value>, ScriptError>| matches!(res, Err(ScriptError::InvalidHandle(_)));
    for held in [foreign_tbl, foreign_fn] {
        host.held = Some(held);
        assert!(invalid(vm.call(&mut host, give, &[])), "push_return {held:?}");
        assert!(invalid(vm.call(&mut host, call, &[])), "call_into {held:?}");
        assert!(invalid(vm.call(&mut host, pcall, &[])), "pcall_into {held:?}");
        assert!(invalid(vm.call(&mut host, store, &[])), "set_upvalue {held:?}");
    }

    // A table this VM collected (its slot reused) is rejected the same way.
    let stale = Value::Table(vm.create_table().unwrap());
    vm.collect_garbage();
    let _reuse = vm.create_table().unwrap();
    host.held = Some(stale);
    assert!(invalid(vm.call(&mut host, give, &[])));
    assert!(invalid(vm.call(&mut host, store, &[])));

    // The VM still works afterwards.
    host.held = vm.get_ref(r);
    assert_eq!(vm.call(&mut host, give, &[]).unwrap().len(), 1);
    for p in pins {
        assert!(vm.release_ref(p));
    }
}

#[test]
fn host_table_get_str_roots_table_when_key_interning_triggers_gc() {
    let mut vm: Vm = Vm::with_limits(VmLimits {
        memory_limit: 256 * 1024,
        max_stack_slots: 1024,
        max_call_depth: 200,
        ..VmLimits::default()
    })
    .unwrap();
    // Create an unrooted table and insert a value under a key.
    let tbl = vm.create_table().unwrap();
    vm.table_set_str(tbl, "answer", Value::Number(42.0)).unwrap();
    // Allocate a large enough key lookup to cross the GC threshold while `tbl`
    // is not stored in `_G` or a `ValueRef`.
    let big_key = "k".repeat(80 * 1024);
    let miss = vm.table_get_str(tbl, &big_key).unwrap();
    assert!(miss.is_nil());
    // `tbl` must still be live and intact after GC ran during `table_get_str`!
    let hit = vm.table_get_str(tbl, "answer").unwrap();
    assert_eq!(hit.as_number(), Some(42.0));
}

#[test]
fn gfind_native_closures_are_collected_and_memory_stays_flat() {
    // The ledger charges each arena's buffer at its capacity, and while a buffer
    // doubles the old and new ones coexist, so the limit leaves room for that.
    let mut vm: Vm = Vm::with_limits(VmLimits {
        memory_limit: 256 * 1024,
        max_stack_slots: 1024,
        max_call_depth: 200,
        ..VmLimits::default()
    })
    .unwrap();
    vm.open_stdlib().unwrap();

    let str_tbl = vm.get_global("string").unwrap().as_table().unwrap();
    let gfind_fn = vm.table_get_str(str_tbl, "gfind").unwrap();
    let pat = vm.intern_str("%a+").unwrap();
    let pat_ref = vm.create_ref(Value::String(pat)).unwrap();

    // Baseline memory after a collection.
    vm.collect_garbage();
    let baseline = vm.memory_bytes();

    // Create, invoke once, and drop 5,000 `string.gfind` iterators over distinct
    // dynamically-created strings (total > 1 MiB without GC, under a 256 KiB limit),
    // twice. Freed slots are reused, so the second round must not grow the heap.
    let mut stale_iter = None;
    let mut out = Vec::new();
    let mut after_round = Vec::new();
    for round in 0..2 {
        for i in 0..5_000 {
            let s = format!("token_{round}_{i}_payload_bytes_for_gc_reclamation");
            let sid = vm.intern_str(&s).unwrap();
            vm.call_into(
                &mut (),
                gfind_fn,
                &[Value::String(sid), vm.get_ref(pat_ref).unwrap()],
                &mut out,
            )
            .unwrap();
            let iter_val = out[0];
            if round == 0 && i == 0 {
                stale_iter = Some(iter_val);
            }
            // Step the iterator once, then drop it.
            vm.call_into(&mut (), iter_val, &[], &mut out).unwrap();
            assert_eq!(
                vm.string_str(out[0].as_string().unwrap()),
                Some("token")
            );
        }
        vm.collect_garbage();
        after_round.push(vm.memory_bytes());
    }
    // What stays after round one is arena capacity (reused, never returned).
    assert!(
        after_round[1] <= after_round[0] && after_round[0] <= baseline + 128 * 1024,
        "memory leaked across dropped gfind iterators: baseline={baseline}, rounds={after_round:?}"
    );
    // The dropped first iterator must now be rejected as a stale handle.
    assert!(matches!(
        vm.call(&mut (), stale_iter.unwrap(), &[]),
        Err(ScriptError::InvalidHandle("native function"))
    ));
}

#[test]
fn lua_and_native_functions_have_distinct_tostring_uids() {
    let mut vm: Vm = Vm::new_with_stdlib();
    let cl = vm
        .load_chunk_bytes(&fixture_bytes("01_arith_and_coercion.luac"))
        .unwrap();
    let cl_str = vm.default_tostring(Value::Closure(cl)).unwrap();

    // Check against every stdlib native function in _G (including getmetatable).
    let g_entries = vm.table_entries(vm.globals()).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    assert!(seen.insert(cl_str.clone()));
    for (_, val) in g_entries {
        if matches!(val, Value::NativeFn(_)) {
            let s = vm.default_tostring(val).unwrap();
            assert_ne!(s, cl_str, "Lua closure collided with native function tostring");
            assert!(seen.insert(s), "duplicate native function tostring UID");
        }
    }
}

#[test]
fn table_growth_is_transactional_and_never_exceeds_memory_limit() {
    let mut vm: Vm = Vm::with_limits(VmLimits {
        memory_limit: 32 * 1024,
        max_stack_slots: 128,
        max_call_depth: 16,
        ..VmLimits::default()
    })
    .unwrap();
    let tbl = vm.create_table().unwrap();
    vm.set_global("t", Value::Table(tbl)).unwrap();

    // Grow the table until the next map insertion exceeds `memory_limit`.
    let mut failed_key = None;
    for i in 1..=2000 {
        let key = Value::Number((i as f32) + 0.5);
        match vm.table_raw_set(tbl, key, Value::Number(i as f32)) {
            Ok(()) => {
                assert!(
                    vm.memory_bytes() <= 32 * 1024,
                    "live memory {} exceeded cap 32768",
                    vm.memory_bytes()
                );
            }
            Err(ScriptError::MemoryLimit { limit: 32768 }) => {
                failed_key = Some(key);
                break;
            }
            Err(other) => panic!("unexpected error: {other}"),
        }
    }
    let key = failed_key.expect("table should eventually hit the 32 KiB memory limit");
    // The failed write must NOT have mutated the table, and `memory_bytes` must stay within cap!
    assert!(vm.memory_bytes() <= 32 * 1024);
    assert!(vm.table_raw_get(tbl, key).unwrap().is_nil());
    // Retrying the exact same write must still fail with MemoryLimit.
    assert!(matches!(
        vm.table_raw_set(tbl, key, Value::Number(999.0)),
        Err(ScriptError::MemoryLimit { limit: 32768 })
    ));
    assert!(vm.table_raw_get(tbl, key).unwrap().is_nil());
}

#[test]
fn with_limits_and_set_limits_reject_unusable_configurations() {
    assert!(matches!(
        Vm::<()>::with_limits(VmLimits {
            memory_limit: 0,
            ..VmLimits::default()
        }),
        Err(ScriptError::InvalidConfig(_))
    ));
    assert!(matches!(
        Vm::<()>::with_limits(VmLimits {
            memory_limit: 128,
            ..VmLimits::default()
        }),
        Err(ScriptError::InvalidConfig(_))
    ));
    assert!(matches!(
        Vm::<()>::with_limits(VmLimits {
            max_call_depth: 0,
            ..VmLimits::default()
        }),
        Err(ScriptError::InvalidConfig(_))
    ));
    assert!(matches!(
        Vm::<()>::with_limits(VmLimits {
            max_stack_slots: 16,
            ..VmLimits::default()
        }),
        Err(ScriptError::InvalidConfig(_))
    ));

    let mut vm: Vm = Vm::new_with_stdlib();
    let live = vm.memory_bytes();
    assert!(live > 1024);
    // Reject setting memory_limit below live bytes, leaving old limits intact.
    let orig = vm.limits();
    assert!(matches!(
        vm.set_limits(VmLimits {
            memory_limit: live - 1,
            ..orig
        }),
        Err(ScriptError::InvalidConfig(_))
    ));
    assert_eq!(vm.limits(), orig);
}

#[test]
fn loaded_protos_are_charged_to_memory_limit_and_reclaimed_on_gc() {
    let bytecode = fixture_bytes("04_tables_and_metatables.luac");
    let mut vm: Vm = Vm::with_limits(VmLimits {
        memory_limit: 256 * 1024,
        max_stack_slots: 1024,
        max_call_depth: 200,
        ..VmLimits::default()
    })
    .unwrap();
    vm.collect_garbage();
    let baseline = vm.memory_bytes();

    // Load a chunk and pin its closure; memory must increase by prototype + constants + closure.
    let cl = vm.load_chunk_bytes(&bytecode).unwrap();
    let r = vm.create_ref(Value::Closure(cl)).unwrap();
    let loaded_mem = vm.memory_bytes();
    assert!(
        loaded_mem > baseline + 1024,
        "LoadedProto bytes should be charged (baseline={baseline}, loaded={loaded_mem})"
    );

    // Unpin and collect; the prototypes and their constants are reclaimed (their arena
    // slots stay charged and are reused by the next load).
    assert!(vm.release_ref(r));
    vm.collect_garbage();
    let after_first = vm.memory_bytes();
    assert!(after_first < loaded_mem);
    let cl = vm.load_chunk_bytes(&bytecode).unwrap();
    let r = vm.create_ref(Value::Closure(cl)).unwrap();
    assert_eq!(vm.memory_bytes(), loaded_mem);
    assert!(vm.release_ref(r));
    vm.collect_garbage();
    assert_eq!(vm.memory_bytes(), after_first);

    // Tight cap that fails mid-load in `load_proto` must not leak temp roots.
    let parsed_proto = load_chunk(&bytecode).unwrap();
    vm.set_limits(VmLimits {
        memory_limit: after_first + 512,
        ..vm.limits()
    })
    .unwrap();
    assert!(matches!(
        vm.load_proto(parsed_proto),
        Err(ScriptError::MemoryLimit { .. })
    ));
    vm.collect_garbage();
    assert_eq!(vm.memory_bytes(), after_first);
}

#[test]
fn native_work_budget_bounds_backtracking_rep_gsub_and_table_ops_even_inside_pcall() {
    let mut vm: Vm = Vm::with_limits(VmLimits {
        instruction_budget: 5_000,
        memory_limit: 128 * 1024,
        max_stack_slots: 1024,
        max_call_depth: 200,
    })
    .unwrap();
    vm.open_stdlib().unwrap();

    let pcall = vm.get_global("pcall").unwrap();
    let str_tbl = vm.get_global("string").unwrap().as_table().unwrap();
    let tab_tbl = vm.get_global("table").unwrap().as_table().unwrap();
    let find_fn = vm.table_get_str(str_tbl, "find").unwrap();
    let rep_fn = vm.table_get_str(str_tbl, "rep").unwrap();
    let gsub_fn = vm.table_get_str(str_tbl, "gsub").unwrap();
    let insert_fn = vm.table_get_str(tab_tbl, "insert").unwrap();
    let setn_fn = vm.table_get_str(tab_tbl, "setn").unwrap();

    // 1. Catastrophic backtracking in `string.find` hits InstructionLimit even inside `pcall`!
    let subject = vm.intern_str(&"a".repeat(40)).unwrap();
    let redos_pat = vm.intern_str("a*a*a*a*a*a*b").unwrap();
    let res = vm.call(
        &mut (),
        pcall,
        &[find_fn, Value::String(subject), Value::String(redos_pat)],
    );
    assert!(
        matches!(res, Err(ScriptError::InstructionLimit { limit: 5_000 })),
        "expected non-catchable InstructionLimit for backtracking, got {res:?}"
    );

    // 2. Excessively deep nested pattern hits non-catchable StackOverflow even inside `pcall`!
    let deep_pat = vm.intern_str(&format!("{}{}", "a?".repeat(210), "z")).unwrap();
    let short_s = vm.intern_str(&"a".repeat(210)).unwrap();
    vm.set_limits(VmLimits {
        instruction_budget: 1_000_000,
        ..vm.limits()
    })
    .unwrap();
    let res = vm.call(
        &mut (),
        pcall,
        &[find_fn, Value::String(short_s), Value::String(deep_pat)],
    );
    assert!(
        matches!(res, Err(ScriptError::StackOverflow { .. })),
        "expected non-catchable StackOverflow for deep pattern, got {res:?}"
    );

    // 3. `string.rep` hits InstructionLimit under small work budget even inside `pcall`.
    vm.set_limits(VmLimits {
        instruction_budget: 500,
        ..vm.limits()
    })
    .unwrap();
    let one_char = vm.intern_str("x").unwrap();
    let res = vm.call(
        &mut (),
        pcall,
        &[rep_fn, Value::String(one_char), Value::Number(10_000.0)],
    );
    assert!(matches!(
        res,
        Err(ScriptError::InstructionLimit { limit: 500 })
    ));

    // 4. `string.gsub` memory expansion hits MemoryLimit BEFORE exceeding cap even inside `pcall`.
    vm.set_limits(VmLimits {
        instruction_budget: 1_000_000,
        memory_limit: 128 * 1024,
        ..vm.limits()
    })
    .unwrap();
    let big_in = vm.intern_str(&"a".repeat(1000)).unwrap();
    let pat_a = vm.intern_str("a").unwrap();
    let big_repl = vm.intern_str(&"b".repeat(500)).unwrap();
    let res = vm.call(
        &mut (),
        pcall,
        &[
            gsub_fn,
            Value::String(big_in),
            Value::String(pat_a),
            Value::String(big_repl),
        ],
    );
    assert!(matches!(res, Err(ScriptError::MemoryLimit { .. })));

    // 5. `table.insert` with extreme `n` uses checked arithmetic and respects InstructionLimit.
    let tbl = vm.create_table().unwrap();
    vm.set_global("big_tbl", Value::Table(tbl)).unwrap();
    vm.call(
        &mut (),
        setn_fn,
        // The largest `f32` below 2^31: larger values convert to `INT_MIN` as C does.
        &[Value::Table(tbl), Value::Number(2_147_483_520.0)],
    )
    .unwrap();
    vm.set_limits(VmLimits {
        instruction_budget: 1_000,
        ..vm.limits()
    })
    .unwrap();
    let res = vm.call(
        &mut (),
        pcall,
        &[
            insert_fn,
            Value::Table(tbl),
            Value::Number(1.0),
            Value::Number(42.0),
        ],
    );
    // Either hits checked overflow or InstructionLimit (never panics or hangs).
    assert!(
        matches!(res, Err(ScriptError::InstructionLimit { .. }))
            || matches!(res, Ok(ref v) if v.first().and_then(|x| x.as_bool()) == Some(false)),
        "unexpected result for extreme table.insert: {res:?}"
    );
}

#[test]
fn chunk_archive_and_recorder_enforce_cumulative_and_malformed_limits() {
    // 1. ChunkLimits enforces input byte cap, decoded byte cap, and proto count cap.
    let valid = fixture_bytes("03_closures_upvalues_varargs.luac");
    assert!(matches!(
        load_chunk_with_limits(
            &valid,
            ChunkLimits {
                max_input_bytes: 32,
                ..ChunkLimits::default()
            }
        ),
        Err(LoadError::InputTooLarge { .. })
    ));
    assert!(matches!(
        load_chunk_with_limits(
            &valid,
            ChunkLimits {
                max_decoded_bytes: 256,
                ..ChunkLimits::default()
            }
        ),
        Err(LoadError::DecodedSizeLimit { .. })
    ));
    assert!(matches!(
        load_chunk_with_limits(
            &valid,
            ChunkLimits {
                max_protos: 1,
                ..ChunkLimits::default()
            }
        ),
        Err(LoadError::TooManyProtos { limit: 1 })
    ));

    // Inherited source names are shared across child prototypes (same Rc pointer).
    let root = load_chunk(&valid).unwrap();
    assert!(!root.protos().is_empty());
    let root_src = root.source().expect("root has source");
    let child_src = root.protos()[0].source().expect("child inherits source");
    assert_eq!(root_src.as_ptr(), child_src.as_ptr());

    // 2. ArchiveLimits rejects overly nested `lvl_` containers and malformed interior tags.
    let mut nested = Vec::new();
    for _ in 0..40 {
        let mut lvl_body = vec![0u8; 8];
        lvl_body.extend_from_slice(&nested);
        let mut next = Vec::new();
        write_ucfb_chunk(&mut next, b"lvl_", &lvl_body);
        nested = next;
    }
    let mut deep_archive = Vec::new();
    write_ucfb_chunk(&mut deep_archive, b"ucfb", &nested);
    assert!(matches!(
        ScriptArchive::parse(&deep_archive),
        Err(ArchiveError::DepthLimit { limit: 32 })
    ));

    // Malformed non-printable interior tag must error instead of silently truncating.
    let entry_luac = fixture_bytes("09_recorder_entry.luac");
    let mut bad_body = Vec::new();
    let mut scr_body = Vec::new();
    write_ucfb_chunk(&mut scr_body, b"NAME", b"ok\0");
    write_ucfb_chunk(&mut scr_body, b"INFO", &[1u8]);
    let mut b_nul = entry_luac.clone();
    b_nul.push(0);
    write_ucfb_chunk(&mut scr_body, b"BODY", &b_nul);
    write_ucfb_chunk(&mut bad_body, b"scr_", &scr_body);
    // Append an invalid non-printable chunk tag after the leading script.
    bad_body.extend_from_slice(&[0xFF, 0xFE, 0x01, 0x02, 0, 0, 0, 0]);
    let mut bad_archive = Vec::new();
    write_ucfb_chunk(&mut bad_archive, b"ucfb", &bad_body);
    assert!(matches!(
        ScriptArchive::parse(&bad_archive),
        Err(ArchiveError::MalformedChunk { .. })
    ));

    // Trailing all-zero padding inside container is still accepted.
    let mut padded_body = Vec::new();
    write_ucfb_chunk(&mut padded_body, b"scr_", &scr_body);
    padded_body.extend_from_slice(&[0u8; 12]);
    let mut padded_archive = Vec::new();
    write_ucfb_chunk(&mut padded_archive, b"ucfb", &padded_body);
    assert_eq!(ScriptArchive::parse(&padded_archive).unwrap().scripts().len(), 1);

    // Input size limit on ArchiveLimits.
    assert!(matches!(
        ScriptArchive::parse_with_limits(
            &padded_archive,
            ArchiveLimits {
                max_input_bytes: 16,
                ..ArchiveLimits::default()
            }
        ),
        Err(ArchiveError::InputTooLarge { .. })
    ));

    // 3. Recorder snapshot bounds shared-DAG exponential blowup cleanly!
    let mut vm: Vm = Vm::new();
    let mut cur = vm.create_table().unwrap();
    vm.set_global("dag", Value::Table(cur)).unwrap();
    // 20-level binary DAG has only 21 tables in memory, but 2^20 = 1,048,576 tree nodes!
    for _ in 0..20 {
        let next = vm.create_table().unwrap();
        vm.table_raw_set(next, Value::Number(1.0), Value::Table(cur))
            .unwrap();
        vm.table_raw_set(next, Value::Number(2.0), Value::Table(cur))
            .unwrap();
        vm.set_global("dag", Value::Table(next)).unwrap();
        cur = next;
    }
    let mut nodes = 0usize;
    let mut str_bytes = 0usize;
    let dag_res = bfe_script::recorder::snapshot_value_bounded(
        &vm,
        Value::Table(cur),
        RecorderLimits {
            max_nodes: 2_000,
            ..RecorderLimits::default()
        },
        &mut nodes,
        &mut str_bytes,
    );
    assert!(matches!(
        dag_res,
        Err(ScriptError::ResourceLimit("recorder node limit exceeded"))
    ));
    assert!(nodes <= 2_001);

    // Output byte limit on JSON serialization.
    let lvl = build_synthetic_lvl(&[
        ("09_recorder_entry", &entry_luac),
        ("09_recorder_helper", &fixture_bytes("09_recorder_helper.luac")),
    ]);
    let archive = ScriptArchive::parse(&lvl).unwrap();
    let manifest = record_manifest(&archive, "09_recorder_entry").unwrap();
    assert!(matches!(
        manifest.to_json_pretty_bounded(64),
        Err(ScriptError::ResourceLimit(_))
    ));
}

#[test]
fn table_delete_and_reinsert_order_is_independent_of_intervening_gc() {
    let run_probe = |do_gc: bool| -> String {
        let mut vm: Vm = Vm::new_with_stdlib();
        let t = vm.create_table().unwrap();
        let t_ref = vm.create_ref(Value::Table(t)).unwrap();

        // Insert "xy" then "z", delete "xy".
        let k_xy = vm.intern_str("xy").unwrap();
        let k_z = vm.intern_str("z").unwrap();
        let z_ref = vm.create_ref(Value::String(k_z)).unwrap();
        vm.table_raw_set(t, Value::String(k_xy), Value::Number(1.0))
            .unwrap();
        vm.table_raw_set(t, Value::String(k_z), Value::Number(1.0))
            .unwrap();
        vm.table_raw_set(t, Value::String(k_xy), Value::Nil)
            .unwrap();

        // Optionally collect so "xy" is swept from the string arena + table tombstone.
        if do_gc {
            vm.collect_garbage();
        }

        // Reinsert "xy" and check which key `next(t, nil)` yields first.
        let k_xy_again = vm.intern_str("xy").unwrap();
        let t_live = vm.get_ref(t_ref).unwrap().as_table().unwrap();
        vm.table_raw_set(t_live, Value::String(k_xy_again), Value::Number(1.0))
            .unwrap();
        let _ = vm.get_ref(z_ref).unwrap();

        let (first_k, _) = vm.table_next(t_live, Value::Nil).unwrap().unwrap();
        let (second_k, _) = vm.table_next(t_live, first_k).unwrap().unwrap();
        assert!(vm.table_next(t_live, second_k).unwrap().is_none());
        format!(
            "{},{}",
            vm.string_str(first_k.as_string().unwrap()).unwrap(),
            vm.string_str(second_k.as_string().unwrap()).unwrap()
        )
    };

    // Both without GC and with GC must yield `"z,xy"` (reinsertion appends at fresh position).
    assert_eq!(run_probe(false), "z,xy");
    assert_eq!(run_probe(true), "z,xy");

    // Verify deleting a key during ongoing `next` traversal still continues cleanly.
    let mut vm: Vm = Vm::new_with_stdlib();
    let t = vm.create_table().unwrap();
    vm.table_set_str(t, "a", Value::Number(1.0)).unwrap();
    vm.table_set_str(t, "b", Value::Number(2.0)).unwrap();
    let (k1, _) = vm.table_next(t, Value::Nil).unwrap().unwrap();
    assert_eq!(vm.string_str(k1.as_string().unwrap()), Some("a"));
    vm.table_raw_set(t, k1, Value::Nil).unwrap();
    let (k2, _) = vm.table_next(t, k1).unwrap().unwrap();
    assert_eq!(vm.string_str(k2.as_string().unwrap()), Some("b"));

    // Verify `collectgarbage(1234)` sets the GC threshold reported by `gcinfo()`.
    let cg_fn = vm.get_global("collectgarbage").unwrap();
    let gi_fn = vm.get_global("gcinfo").unwrap();
    vm.call(&mut (), cg_fn, &[Value::Number(1234.0)]).unwrap();
    let gi = vm.call(&mut (), gi_fn, &[]).unwrap();
    assert_eq!(gi[1].as_number(), Some(1234.0));
}

#[test]
fn userdata_gc_metamethods_never_run_and_weak_entries_clear_in_full_collections() {
    // Script finalizers are not run (no stock mission uses `__gc`): no order, nesting,
    // depth or error of a finalizer can reach the host call that collects.
    let mut vm: Vm = Vm::with_limits(VmLimits::default()).unwrap();
    vm.open_stdlib().unwrap();
    vm.register_fn("NewUD", |ctx| {
        let id = Wide::from_i32(ctx.opt_int(0, 0)?);
        let mt = ctx.vm_mut().create_table()?;
        let ud = ctx.vm_mut().create_userdata(1, id, Some(mt))?;
        ctx.push_return(Value::Userdata(ud))
    })
    .unwrap();
    // A native `__gc` is not called either.
    let calls = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let seen = std::rc::Rc::clone(&calls);
    let native_gc = vm
        .create_native_closure(move |_ctx| {
            seen.set(seen.get() + 1);
            Ok(())
        })
        .unwrap();
    let mt = vm.create_table().unwrap();
    vm.table_set_str(mt, "__gc", Value::NativeFn(native_gc)).unwrap();
    let u = vm.create_userdata(2, Wide::from_u32(7), Some(mt)).unwrap();

    vm.capture_output();
    vm.run_chunk(&mut (), &fixture_bytes("18_gc_without_finalizers.luac"))
        .unwrap();
    assert_eq!(
        String::from_utf8(vm.take_output()).unwrap(),
        "finalizers run\t0\nweak value\ttrue\tstrings stay\nweak key\ttrue\t2\ttrue\n"
    );
    vm.collect_garbage();
    vm.collect_garbage();
    assert_eq!(calls.get(), 0);
    // The unreferenced userdata are gone.
    assert!(vm.userdata_info(u).is_none());
}

#[test]
fn native_reentry_depth_is_bounded_and_trailing_percent_pattern_reports_clean_error() {
    // 1. Trailing `%` in pattern returns clean Lua 5.0 error text (without C printf typo).
    let mut vm: Vm = Vm::new_with_stdlib();
    let str_tbl = vm.get_global("string").unwrap().as_table().unwrap();
    let find_fn = vm.table_get_str(str_tbl, "find").unwrap();
    let pcall_fn = vm.get_global("pcall").unwrap();
    let subj = vm.intern_str("a").unwrap();
    let pat = vm.intern_str("%").unwrap();
    let out = vm
        .call(
            &mut (),
            pcall_fn,
            &[find_fn, Value::String(subj), Value::String(pat)],
        )
        .unwrap();
    assert_eq!(out[0].as_bool(), Some(false));
    let msg = vm.string_str(out[1].as_string().unwrap()).unwrap();
    assert_eq!(msg, "malformed pattern (ends with `%')");

    // 2. Direct Lua recursion runs up to `max_call_depth` without hitting native re-entry cap.
    let rec_chunk = fixture_bytes("07_limits_and_gc.luac");
    let mut vm2: Vm = Vm::with_limits(VmLimits {
        max_call_depth: 500,
        ..VmLimits::default()
    })
    .unwrap();
    vm2.open_stdlib().unwrap();
    vm2.run_chunk(&mut (), &rec_chunk).unwrap();
    let deep_recurse = vm2.get_global("deep_recurse").unwrap();
    let res = vm2.call(&mut (), deep_recurse, &[Value::Number(0.0)]);
    assert!(matches!(res, Err(ScriptError::StackOverflow { limit: 500 })));

    // 3. Deep native VM re-entry (native -> Lua -> native -> Lua via `pcall`) hits the
    //    conservative native re-entry cap (64) even when `max_call_depth` is 500, and
    //    propagates out of `pcall` as a non-catchable StackOverflow.
    //    Minimal valid Lua 5.0.3 chunk: `return Bounce()` (`GETGLOBAL 0 0; CALL 0 1 1; RETURN 0 1`).
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&[0x1b, b'L', b'u', b'a', 0x50, 1, 4, 4, 4, 6, 8, 9, 9, 4]);
    chunk.extend_from_slice(&31415926.0f32.to_le_bytes());
    let src_name = b"@reentry.lua\0";
    chunk.extend_from_slice(&(src_name.len() as u32).to_le_bytes());
    chunk.extend_from_slice(src_name);
    chunk.extend_from_slice(&0u32.to_le_bytes()); // lineDefined
    chunk.extend_from_slice(&[0, 0, 0, 2]); // nups, numparams, is_vararg, maxstacksize
    chunk.extend_from_slice(&0u32.to_le_bytes()); // lineinfo
    chunk.extend_from_slice(&0u32.to_le_bytes()); // locvars
    chunk.extend_from_slice(&0u32.to_le_bytes()); // upvalues
    chunk.extend_from_slice(&1u32.to_le_bytes()); // 1 constant: "Bounce"
    chunk.push(4); // LUA_TSTRING
    let cname = b"Bounce\0";
    chunk.extend_from_slice(&(cname.len() as u32).to_le_bytes());
    chunk.extend_from_slice(cname);
    chunk.extend_from_slice(&0u32.to_le_bytes()); // 0 child protos
    // Instructions (6/8/9/9: op | C<<6 | B<<15 | A<<24, Bx = B<<9 | C -> Bx<<6):
    // OP_GETGLOBAL (5) A=0 Bx=0; OP_CALL (25) A=0 B=1 C=1; OP_RETURN (27) A=0 B=1
    let ins0: u32 = 5;
    let ins1: u32 = 25 | (1 << 6) | (1 << 15);
    let ins2: u32 = 27 | (1 << 15);
    chunk.extend_from_slice(&3u32.to_le_bytes());
    for ins in [ins0, ins1, ins2] {
        chunk.extend_from_slice(&ins.to_le_bytes());
    }

    let mut vm3: Vm = Vm::with_limits(VmLimits {
        max_call_depth: 500,
        ..VmLimits::default()
    })
    .unwrap();
    vm3.open_stdlib().unwrap();
    let cl = vm3.load_chunk_bytes(&chunk).unwrap();
    vm3.set_global("THUNK", Value::Closure(cl)).unwrap();
    vm3.register_fn("Bounce", |ctx| {
        let pcall = ctx.vm_mut().get_global("pcall")?;
        let thunk = ctx.vm_mut().get_global("THUNK")?;
        let mut out = Vec::new();
        ctx.call_into(pcall, &[thunk], &mut out)
    })
    .unwrap();
    let res = vm3.call(&mut (), Value::Closure(cl), &[]);
    assert!(
        matches!(res, Err(ScriptError::StackOverflow { limit: 64 })),
        "expected non-catchable StackOverflow at native re-entry cap 64, got {res:?}"
    );
}

#[test]
fn function_keys_identify_failed_instructions() {
    let mut vm = Vm::new_with_stdlib();
    let mut keys = Vec::new();
    let bytes = fixture_bytes("25_digest_alpha.luac");
    let entry = vm
        .load_chunk_with_functions(&bytes, &mut |key, ordinal| keys.push((key, ordinal)))
        .unwrap();
    assert_eq!(keys.iter().map(|p| p.1).collect::<Vec<_>>(), [0, 1]);
    vm.call(&mut (), Value::Closure(entry), &[]).unwrap();
    let score = vm.get_global("score").unwrap();
    vm.call(&mut (), score, &[Value::Bool(false)]).unwrap_err();
    let site = vm.failure_site().unwrap();
    assert_eq!(site.function, keys[1].0);
    assert_eq!(site.instruction, 0);
    vm.call(&mut (), score, &[Value::Number(2.0)]).unwrap();
    assert!(vm.failure_site().is_none());
    let count = keys.len();
    assert!(vm
        .load_chunk_with_functions(&bytes[..12], &mut |k, i| keys.push((k, i)))
        .is_err());
    assert_eq!(keys.len(), count);
}
