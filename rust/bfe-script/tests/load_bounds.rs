//! Loading a chunk stays inside the memory the VM has left and, inside a call, inside
//! the call's remaining budget: the decoded prototypes and the verifier's scratch are
//! charged at their real sizes before they are allocated.

use std::rc::Rc;

use bfe_script::opcode::{OpCode, MAXSTACK, POS_A, POS_B, POS_BX, POS_C};
use bfe_script::{ScriptError, Value, Vm, VmLimits};

#[global_allocator]
static GLOBAL: bfe_test_alloc::Counting = bfe_test_alloc::Counting::new();

/// Run `f` and return the most bytes it held live at once beyond what it started with.
fn peak_of<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let (out, peak, _) = measure(f);
    (out, peak)
}

/// Run `f` and return its peak and its net change of live bytes.
fn measure<T>(f: impl FnOnce() -> T) -> (T, usize, isize) {
    let (out, peak, net) = bfe_test_alloc::measure_thread_peak(f);
    (
        out,
        usize::try_from(peak).unwrap_or(usize::MAX),
        isize::try_from(net).unwrap_or(isize::MAX),
    )
}

fn abc(op: OpCode, a: u32, b: u32, c: u32) -> u32 {
    (op as u32) | (c << POS_C) | (b << POS_B) | (a << POS_A)
}

fn abx(op: OpCode, a: u32, bx: u32) -> u32 {
    (op as u32) | (bx << POS_BX) | (a << POS_A)
}

/// The compiled form of `local n=1` followed by `statements` lines of `n=n+1` and
/// `return n`, as the reference compiler writes it (without line information).
fn straight_line_chunk(statements: usize) -> Vec<u8> {
    numbered_chunk(statements, 1)
}

fn numbered_chunk(statements: usize, constants: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"\x1bLua");
    out.extend_from_slice(&[0x50, 1, 4, 4, 4, 6, 8, 9, 9, 4]);
    out.extend_from_slice(&31_415_926.0f32.to_bits().to_le_bytes());
    let name = b"@straight.lua\0";
    out.extend_from_slice(&(name.len() as u32).to_le_bytes());
    out.extend_from_slice(name);
    out.extend_from_slice(&0i32.to_le_bytes()); // line defined
    out.extend_from_slice(&[0, 0, 0, 2]); // nups, params, vararg, max stack
    out.extend_from_slice(&0i32.to_le_bytes()); // line info
    let code_len = statements + 3;
    out.extend_from_slice(&1i32.to_le_bytes()); // one local, `n`
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(b"n\0");
    out.extend_from_slice(&1i32.to_le_bytes());
    out.extend_from_slice(&(code_len as i32).to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes()); // upvalue names
    out.extend_from_slice(&constants.to_le_bytes());
    for i in 1..=constants {
        out.push(3);
        out.extend_from_slice(&(i as f32).to_bits().to_le_bytes());
    }
    out.extend_from_slice(&0i32.to_le_bytes()); // no child functions
    out.extend_from_slice(&(code_len as i32).to_le_bytes());
    out.extend_from_slice(&abx(OpCode::LoadK, 0, 0).to_le_bytes());
    let add = abc(OpCode::Add, 0, 0, MAXSTACK as u32);
    for _ in 0..statements {
        out.extend_from_slice(&add.to_le_bytes());
    }
    out.extend_from_slice(&abc(OpCode::Return, 0, 2, 0).to_le_bytes());
    out.extend_from_slice(&abc(OpCode::Return, 0, 1, 0).to_le_bytes());
    out
}

/// The peak additional allocation includes the decoded program and verifier scratch.
#[test]
fn loading_stays_within_the_memory_left() {
    let chunk = straight_line_chunk(32_768);
    // What the loaded chunk holds, measured under a generous limit.
    let mut probe = Vm::<()>::with_limits(VmLimits::default()).unwrap();
    let before = probe.memory_bytes();
    probe.load_chunk_bytes(&chunk).unwrap();
    let loaded = probe.memory_bytes() - before;
    for (room_wanted, fits) in [(4usize << 20, true), (loaded + 65_536, false), (loaded / 2, false)] {
        let mut limits = VmLimits::default();
        let base = Vm::<()>::with_limits(limits).unwrap().memory_bytes();
        limits.memory_limit = base + room_wanted;
        let mut vm = Vm::<()>::with_limits(limits).unwrap();
        let room = limits.memory_limit - vm.memory_bytes();
        let (res, peak) = peak_of(|| vm.load_chunk_bytes(&chunk));
        println!("room {room}: loaded {loaded}, peak {peak}, ok {}", res.is_ok());
        assert!(peak <= room, "peak {peak} bytes with {room} bytes of room");
        match res {
            Ok(cl) => {
                assert!(fits);
                assert!(vm.memory_bytes() <= limits.memory_limit);
                let out = vm.call(&mut (), Value::Closure(cl), &[]).unwrap();
                assert!(matches!(out[..], [Value::Number(n)] if n == 32_769.0));
            }
            Err(e) => {
                assert!(!fits, "{e}");
                assert!(matches!(e, ScriptError::MemoryLimit { .. }), "{e}");
            }
        }
    }
}

/// A chunk a native function loads inside a call is the call's own work: with the
/// tick budget the 32,768-statement chunk is refused before the decoder goes past the
/// budget, and with the load budget it loads and reports what it used.
#[test]
fn a_native_include_is_limited_by_the_call_budget() {
    let chunk = Rc::new(straight_line_chunk(32_768));
    let mut vm = Vm::new_with_stdlib();
    let bytes = Rc::clone(&chunk);
    let include = vm
        .create_native_closure(move |ctx| {
            ctx.vm_mut().load_chunk_bytes(&bytes)?;
            Ok(())
        })
        .unwrap();
    vm.set_instruction_budget(VmLimits::TICK_BUDGET);
    let err = vm.call(&mut (), Value::NativeFn(include), &[]).unwrap_err();
    assert!(
        matches!(err, ScriptError::InstructionLimit { limit } if limit == VmLimits::TICK_BUDGET),
        "{err}"
    );
    assert!(vm.work_used() <= VmLimits::TICK_BUDGET);
    vm.set_instruction_budget(VmLimits::LOAD_BUDGET);
    vm.call(&mut (), Value::NativeFn(include), &[]).unwrap();
    let used = vm.work_used();
    println!("include of {} bytes used {used} units", chunk.len());
    // Decoding reads every byte (sixteen per unit) and the verifier visits every
    // instruction in three passes (four steps per unit).
    assert!(used >= chunk.len() as u32 / 16, "{used}");
    assert!(used > VmLimits::TICK_BUDGET);
}

/// Lowering the limits shrinks the reserved stack, frame and call trace buffers with
/// the reservation, raising them grows both, and reserving userdata room charges it:
/// the reported figure follows the real heap (within the allocator's block rounding
/// the accounting adds).
#[test]
fn limits_and_reservations_follow_the_real_heap() {
    let mut vm = Vm::<()>::new_with_stdlib();
    let roomy = vm.limits();
    let mut small = roomy;
    small.max_stack_slots = 1_024;
    small.max_call_depth = 256;
    for limits in [small, roomy] {
        let before = vm.memory_bytes() as isize;
        let (res, _, real) = measure(|| vm.set_limits(limits));
        res.unwrap();
        let charged = vm.memory_bytes() as isize - before;
        println!("set_limits: charged {charged}, real {real}");
        assert!(charged >= real && charged - real <= 3 * 32, "{charged} vs {real}");
    }
    let before = vm.memory_bytes() as isize;
    let (res, _, real) = measure(|| vm.reserve_userdata(512));
    res.unwrap();
    let charged = vm.memory_bytes() as isize - before;
    println!("reserve_userdata: charged {charged}, real {real}");
    assert!(real > 0 && charged >= real && charged - real <= 32, "{charged} vs {real}");
}

#[test]
fn userdata_reservation_reclaims_unreachable_storage_before_failing() {
    let mut vm = Vm::<()>::with_limits(VmLimits {
        max_stack_slots: 128,
        max_call_depth: 8,
        ..VmLimits::default()
    })
    .unwrap();
    vm.intern_bytes(&[17; 24 * 1024]).unwrap();
    let cap = vm.memory_bytes() + 4096;
    vm.set_limits(VmLimits {
        memory_limit: cap,
        ..vm.limits()
    })
    .unwrap();
    let before = vm.collector_stats();
    vm.reserve_userdata(256).unwrap();
    assert_eq!(
        vm.collector_stats().full_collections - before.full_collections,
        1
    );
    assert!(vm.memory_bytes() < cap);
}

#[test]
fn exchanging_stack_and_frame_capacity_fits_the_peak() {
    let limits = VmLimits {
        max_stack_slots: 2048,
        max_call_depth: 4096,
        memory_limit: 600_000,
        ..VmLimits::default()
    };
    let (vm, _, initial) = measure(|| Vm::<()>::with_limits(limits).unwrap());
    let mut vm = vm;
    let limits = VmLimits {
        max_stack_slots: 32768,
        max_call_depth: 128,
        ..limits
    };
    let (result, peak) = peak_of(|| vm.set_limits(limits));
    result.unwrap();
    assert!(
        initial as usize + peak <= limits.memory_limit,
        "initial {initial}, additional peak {peak}"
    );
    let bytes = vm.memory_bytes();
    for slots in [usize::MAX, usize::MAX / 2, limits.memory_limit] {
        assert!(vm
            .set_limits(VmLimits {
                max_stack_slots: slots,
                ..limits
            })
            .is_err());
        assert_eq!(vm.memory_bytes(), bytes);
        assert_eq!(vm.limits().max_stack_slots, limits.max_stack_slots);
    }
    assert!(vm.create_table().is_ok());
}

#[test]
fn native_include_pays_once_when_verification_reclaims_memory() {
    fn run(chunk: Rc<Vec<u8>>, pressure: bool, budget: u32) -> u32 {
        let mut vm = Vm::<()>::with_limits(VmLimits {
            max_stack_slots: 256,
            max_call_depth: 16,
            ..VmLimits::default()
        })
        .unwrap();
        let include = vm
            .create_native_closure(move |ctx| {
                ctx.vm_mut().load_chunk_bytes(&chunk)?;
                Ok(())
            })
            .unwrap();
        vm.create_ref(Value::NativeFn(include)).unwrap();
        if pressure {
            vm.intern_bytes(&[41; 65_536]).unwrap();
            vm.set_limits(VmLimits {
                memory_limit: vm.memory_bytes() + 8192,
                ..vm.limits()
            })
            .unwrap();
        }
        vm.set_instruction_budget(budget);
        let before = vm.collector_stats().forced_collections;
        vm.call(&mut (), Value::NativeFn(include), &[]).unwrap();
        if pressure {
            assert!(vm.collector_stats().forced_collections > before);
        }
        vm.work_used()
    }
    let chunk = Rc::new(straight_line_chunk(1024));
    let ordinary = run(chunk.clone(), false, VmLimits::LOAD_BUDGET);
    assert_eq!(run(chunk, true, ordinary), ordinary);
}

#[test]
fn predecoded_number_pool_is_reserved_before_instantiation() {
    let bytes = numbered_chunk(2, 2048);
    let proto = bfe_script::load_chunk(&bytes).unwrap();
    let mut vm = Vm::<()>::with_limits(VmLimits {
        max_stack_slots: 128,
        max_call_depth: 8,
        memory_limit: 32_768,
        ..VmLimits::default()
    })
    .unwrap();
    let t = vm.create_table().unwrap();
    vm.create_ref(Value::Table(t)).unwrap();
    for i in 1..=48 {
        vm.table_raw_set(t, Value::Number(i as f32), Value::Number(i as f32))
            .unwrap();
    }
    let room = vm.limits().memory_limit - vm.memory_bytes();
    let (result, peak) = peak_of(|| vm.load_proto(proto.clone()));
    assert!(matches!(result, Err(ScriptError::MemoryLimit { .. })));
    assert!(peak <= room, "additional peak {peak} with {room} available");
}

#[test]
fn nested_replacement_buffers_remain_charged_during_callbacks() {
    use bfe_script::CallCtx;
    struct Sample {
        gsub: Value,
        args: [Value; 4],
        depth: usize,
        bytes: [usize; 4],
    }
    fn replace(ctx: &mut CallCtx<'_, Sample>) -> Result<(), ScriptError> {
        let depth = ctx.host().depth;
        let bytes = ctx.vm().memory_bytes();
        ctx.host_mut().bytes[depth] = bytes;
        if depth + 1 < 4 {
            let (gsub, args) = (ctx.host().gsub, ctx.host().args);
            ctx.host_mut().depth += 1;
            let res = ctx.call_into(gsub, &args, &mut Vec::new());
            ctx.host_mut().depth -= 1;
            res?;
        }
        ctx.push_return(ctx.arg(0))
    }
    let mut vm = Vm::<Sample>::new_with_stdlib();
    let Value::Table(lib) = vm.get_global("string").unwrap() else {
        panic!()
    };
    let gsub = vm.table_get_str(lib, "gsub").unwrap();
    let (replace, _root) = vm.create_rooted_native_fn(replace).unwrap();
    let subject = Value::String(vm.intern_bytes(&[b'x'; 8192]).unwrap());
    let pattern = Value::String(vm.intern_str("^.").unwrap());
    let args = [
        subject,
        pattern,
        Value::NativeFn(replace),
        Value::Number(1.),
    ];
    let mut host = Sample {
        gsub,
        args,
        depth: 0,
        bytes: [0; 4],
    };
    vm.call(&mut host, gsub, &args).unwrap();
    let before = vm.memory_bytes();
    vm.call(&mut host, gsub, &args).unwrap();
    for pair in host.bytes.windows(2) {
        assert!(pair[1] >= pair[0] + 8192, "{:?}", host.bytes);
    }
    assert_eq!(vm.memory_bytes(), before);
    vm.set_limits(VmLimits {
        memory_limit: before + 24000,
        ..vm.limits()
    })
    .unwrap();
    assert!(matches!(
        vm.call(&mut host, gsub, &args),
        Err(ScriptError::MemoryLimit { .. })
    ));
    assert!(vm.memory_bytes() <= before);
    assert_eq!(host.depth, 0);
}

#[test]
fn growing_a_table_reserves_the_replacement_beside_the_old_array() {
    let mut vm = Vm::new();
    let t = vm.create_table().unwrap();
    vm.create_ref(Value::Table(t)).unwrap();
    for i in 1..=64 {
        vm.table_raw_set(t, Value::Number(i as f32), Value::Bool(true))
            .unwrap();
    }
    vm.collect_garbage();
    vm.set_limits(VmLimits {
        memory_limit: vm.memory_bytes() + 1536,
        ..vm.limits()
    })
    .unwrap();
    let key = Value::Number(65.);
    assert!(matches!(
        vm.table_raw_set(t, key, key),
        Err(ScriptError::MemoryLimit { .. })
    ));
    assert!(vm.table_raw_get(t, key).unwrap().is_nil());
    assert!(matches!(
        vm.table_raw_get(t, Value::Number(64.)).unwrap(),
        Value::Bool(true)
    ));
}

#[test]
fn native_closure_payload_is_reserved_before_it_is_boxed() {
    let mut vm = Vm::new();
    let limit = vm.memory_bytes() + 2048;
    vm.set_limits(VmLimits {
        memory_limit: limit,
        ..vm.limits()
    })
    .unwrap();
    let data = [17u8; 8192];
    let (result, peak) = peak_of(|| {
        vm.create_native_closure(move |ctx| {
            ctx.push_return(Value::Number(std::hint::black_box(&data)[0] as f32))
        })
    });
    assert!(matches!(result, Err(ScriptError::MemoryLimit { .. })));
    assert!(peak <= 2048);
}

#[path = "common/archive.rs"]
mod synthetic_archive;

#[test]
fn recorder_includes_use_the_enclosing_call_budget() {
    let entry = include_bytes!("lua/09_recorder_entry.luac");
    let helper = numbered_chunk(1, 2048);
    let bytes = synthetic_archive::build_synthetic_lvl(&[
        ("entry", entry),
        ("09_recorder_helper", &helper),
    ]);
    let archive = bfe_script::ScriptArchive::parse(&bytes).unwrap();
    let limits = VmLimits {
        instruction_budget: 500,
        ..VmLimits::default()
    };
    assert!(matches!(
        bfe_script::record_manifest_with_limits(&archive, "entry", limits),
        Err(ScriptError::InstructionLimit { limit: 500 })
    ));
    bfe_script::record_manifest(&archive, "entry").unwrap();
}

#[test]
fn large_captures_require_room_before_allocating_their_buffer() {
    let mut vm = Vm::new();
    let maker = vm
        .run_chunk(&mut (), include_bytes!("lua/29_capture.luac"))
        .unwrap()[0];
    vm.create_ref(maker).unwrap();
    let args = [2., 3., 5., 7., 11.].map(Value::Number);
    let mut result = Vec::with_capacity(1);
    vm.call_into(&mut (), maker, &args, &mut result).unwrap();
    let value = vm.call(&mut (), result[0], &[]).unwrap();
    assert!(matches!(value[..], [Value::Number(50.)]));
    vm.collect_garbage();
    vm.set_limits(VmLimits {
        memory_limit: vm.memory_bytes(),
        ..vm.limits()
    })
    .unwrap();
    let (r, peak) = peak_of(|| vm.call_into(&mut (), maker, &args, &mut result));
    assert!(matches!(r, Err(ScriptError::MemoryLimit { .. })));
    assert_eq!(peak, 0, "capture allocation before refusal");
}

#[test]
fn diagnostic_function_visits_are_admitted_together() {
    fn run(visit: bool, budget: u32) -> (Result<(), ScriptError>, u32, usize) {
        let mut vm = Vm::new();
        let calls = Rc::new(std::cell::Cell::new(0usize));
        let seen = calls.clone();
        let f = vm
            .create_native_closure(move |ctx| {
                let bytes = include_bytes!("lua/29_capture.luac");
                if visit {
                    ctx.vm_mut()
                        .load_chunk_with_functions(bytes, &mut |_, _| seen.set(seen.get() + 1))?;
                } else {
                    ctx.vm_mut().load_chunk_bytes(bytes)?;
                }
                Ok(())
            })
            .unwrap();
        vm.set_instruction_budget(budget);
        let result = vm.call_into(&mut (), Value::NativeFn(f), &[], &mut Vec::new());
        (result, vm.work_used(), calls.get())
    }
    let (plain, budget, _) = run(false, VmLimits::LOAD_BUDGET);
    plain.unwrap();
    let (short, _, visits) = run(true, budget);
    assert!(matches!(short, Err(ScriptError::InstructionLimit { .. })));
    assert_eq!(visits, 0);
    let (complete, used, visits) = run(true, budget + 3);
    complete.unwrap();
    assert_eq!((used, visits), (budget + 3, 3));
}

#[test]
fn discovering_an_empty_archive_does_not_construct_a_second_vm() {
    let bytes = synthetic_archive::build_synthetic_lvl(&[]);
    let archive = bfe_script::ScriptArchive::parse(&bytes).unwrap();
    let (scan, peak) = peak_of(|| bfe_script::scan_archive(&archive, Default::default()));
    assert!(scan.host_functions.is_empty());
    assert_eq!(peak, 0);
}

#[test]
fn displaying_a_lua_error_reserves_the_encoded_diagnostic() {
    let mut vm = Vm::new_with_stdlib();
    let error = vm.get_global("error").unwrap();
    let text = Value::String(vm.intern_bytes(&[0xff; 1024]).unwrap());
    vm.create_ref(text).unwrap();
    vm.collect_garbage();
    vm.set_limits(VmLimits {
        memory_limit: vm.memory_bytes() + 2048,
        ..vm.limits()
    })
    .unwrap();
    let (r, peak) = peak_of(|| vm.call(&mut (), error, &[text, Value::Number(0.)]));
    assert!(matches!(r, Err(ScriptError::MemoryLimit { .. })));
    assert!(peak <= 2048);
}
