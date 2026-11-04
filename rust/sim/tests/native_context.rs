#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
use bfe_sim::abi::context::{Context, Error, ErrorKind, Failure, Record};
use bfe_sim::gen::*;
use bfe_sim::store::block::Block;
use std::cell::Cell;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::rc::Rc;

fn echo(value: f64, note: Option<StrId>, events: u32) -> Record {
    Record::new(Echo::WORDS, |r, h| {
        Echo {
            value,
            at: [-0.0, f64::INFINITY, f64::from_bits(1)],
            note,
            events,
            ..Default::default()
        }
        .put(r, h)
    })
}
fn fill(cx: &Context, count: u32) {
    cx.command(
        COMMAND_FILL,
        Record::new(Fill::WORDS, |r, h| {
            Fill {
                kind: KIND_BOLT as u32,
                count,
            }
            .put(r, h)
        }),
    )
    .unwrap();
}
fn clock(state: WorldClockState, seconds: f64) -> WorldClockInput {
    WorldClockInput {
        op: WorldClockOperation::Advance,
        state,
        seconds,
        limited: false,
    }
}
fn state() -> WorldClockState {
    WorldClockState {
        hz: 60.0,
        dt: 1.0 / 60.0,
        max_steps: 8.0,
        budget: f64::INFINITY,
        max_debt: f64::INFINITY,
        ..Default::default()
    }
}
fn reply(state: WorldClockState, time: f64) -> Record {
    Record::new(WorldClockReply::WORDS, |r, h| {
        WorldClockReply { state, time }.put(r, h)
    })
}
fn identity_clock(cx: &Context) {
    cx.register(IMPORT_WORLD_CLOCK, |_, args| {
        Ok(reply(args.read(WorldClockArgs::take).state, 0.0))
    })
    .unwrap();
}
fn bits(values: &[f64]) -> Vec<u64> {
    values.iter().map(|v| v.to_bits()).collect()
}

#[test]
fn native_contexts_isolate_local_ids_rows_strings_commands_events_and_disposal() {
    let a = Context::new();
    let b = Context::new();
    let aid = a.intern(&[65, 0xd800, 0]).unwrap();
    let bid = b.intern(&[66, 0xdc00, 0]).unwrap();
    assert_eq!(aid, bid);
    for cx in [&a, &b] {
        fill(cx, 1);
        identity_clock(cx);
    }
    let rows = a.export(KIND_BOLT as u32, 1).unwrap();
    assert_eq!(bits(&rows), bits(&b.export(KIND_BOLT as u32, 1).unwrap()));
    let slot = rows[5] as u32;
    let generation = rows[6] as u32;
    // Same entity slots, different full field records. Use generated Store IO.
    for (cx, speed) in [(&a, 12.5), (&b, -7.0)] {
        let mut block = Block::default();
        block.begin(1, Bolt::FULL_STRIDE);
        let (row, heap) = block.row(0);
        row[0] = slot as f64;
        row[1] = generation as f64;
        Bolt {
            speed,
            ..Default::default()
        }
        .put_full(row, heap);
        block.finish();
        cx.import(KIND_BOLT as u32, 1, block).unwrap();
    }
    assert_ne!(
        bits(&a.export(KIND_BOLT as u32, 1).unwrap()),
        bits(&b.export(KIND_BOLT as u32, 1).unwrap())
    );
    for (cx, id, value) in [(&a, aid, 10.0), (&b, bid, 20.0)] {
        cx.register(IMPORT_ASK, move |cx, args| {
            let mut out = args.read(Echo::take);
            assert_eq!(
                cx.string(out.note.unwrap()).unwrap()[0],
                if value == 10.0 { 65 } else { 66 }
            );
            out.value += value;
            Ok(Record::new(Echo::WORDS, |r, h| out.put(r, h)))
        })
        .unwrap();
        let out = cx.command(COMMAND_ECHO, echo(value, Some(id), 2)).unwrap();
        assert_eq!(out.read(Echo::take).value, value + 1.0);
    }
    let saved = a.calls().unwrap();
    let events = a.events().unwrap();
    assert_eq!(a.clock(clock(state(), 0.05)).unwrap().state.tick, 3.0);
    assert_eq!(b.clock(clock(state(), 1.0 / 60.0)).unwrap().state.tick, 1.0);
    assert_eq!(
        a.command(COMMAND_RELAY, echo(1.0, Some(aid), 0))
            .unwrap()
            .read(Echo::take)
            .value,
        11.0
    );
    assert_eq!(
        b.command(COMMAND_RELAY, echo(1.0, Some(bid), 0))
            .unwrap()
            .read(Echo::take)
            .value,
        21.0
    );
    assert_eq!(saved.read(Echo::take).value, 11.0);
    b.clear_events().unwrap();
    assert_eq!(bits(&events), bits(&a.events().unwrap()));
    assert!(a.retire(KIND_BOLT as u32, slot, generation).unwrap());
    assert!(!a.retire(KIND_BOLT as u32, slot, generation).unwrap());
    assert_eq!(a.export(KIND_BOLT as u32, 0).unwrap()[4], 1.0);
    assert_eq!(a.export(KIND_BOLT as u32, 0).unwrap()[4], 0.0);
    assert_eq!(b.export(KIND_BOLT as u32, 1).unwrap()[0], 1.0);
    fill(&a, 1);
    let replacement = a.export(KIND_BOLT as u32, 1).unwrap();
    let last = 5 + (replacement[0] as usize - 1) * replacement[1] as usize;
    assert_eq!(replacement[last], slot as f64);
    assert_ne!(replacement[last + 1], generation as f64);
    assert!(!a.retire(KIND_BOLT as u32, slot, generation).unwrap());
    let stale = a.clone();
    a.dispose().unwrap();
    assert_eq!(stale.calls().unwrap_err().kind(), ErrorKind::Disposed);
    assert_eq!(stale.dispose().unwrap_err().kind(), ErrorKind::Disposed);
    let c = Context::new();
    assert_eq!(c.intern(&[67]).unwrap(), aid);
    assert_eq!(stale.intern(&[90]).unwrap_err().kind(), ErrorKind::Disposed);
    assert_eq!(b.string(bid).unwrap(), vec![66, 0xdc00, 0]);
    assert_eq!(
        b.command(COMMAND_RELAY, echo(2.0, Some(bid), 0))
            .unwrap()
            .read(Echo::take)
            .value,
        22.0
    );
    b.dispose().unwrap();
    c.dispose().unwrap();
}

#[test]
fn native_context_nested_a_b_a_callbacks_grow_buffers_and_keep_outer_records() {
    let a = Context::new();
    let b = Context::new();
    let depth = Rc::new(Cell::new(0));
    let seen = Rc::new(Cell::new(0));
    let nested_a = a.clone();
    b.register(IMPORT_ASK, move |cx, args| {
        assert_eq!(cx.dispose().unwrap_err().kind(), ErrorKind::Busy);
        let out = nested_a.command(COMMAND_RELAY, echo(7.0, None, 0))?;
        assert_eq!(out.read(Echo::take).value, 107.0);
        assert_eq!(args.read(Echo::take).value, 3.0);
        Ok(echo(30.0, None, 0))
    })
    .unwrap();
    let other = b.clone();
    let d = depth.clone();
    let calls = seen.clone();
    a.register(IMPORT_ASK, move |cx, args| {
        calls.set(calls.get() + 1);
        let original = args.read(Echo::take);
        assert_eq!(cx.dispose().unwrap_err().kind(), ErrorKind::Busy);
        if d.get() == 0 {
            d.set(1);
            assert_eq!(
                other
                    .command(COMMAND_RELAY, echo(3.0, None, 0))?
                    .read(Echo::take)
                    .value,
                30.0
            );
            d.set(0);
            assert_eq!(bits(&args.row), bits(&echo(original.value, None, 0).row));
        }
        cx.scratch(&vec![0x31; 200_000])?;
        cx.intern_scratch(100_000)?;
        fill(cx, 400);
        cx.export(KIND_BOLT as u32, 1)?;
        cx.command(COMMAND_ECHO, echo(9.0, None, 3))?;
        Ok(echo(original.value + 100.0, None, 0))
    })
    .unwrap();
    // A different argument shape/heap reenters the same exchange.
    a.register(999, |cx, args| {
        assert_eq!(args.heap.len(), 4000);
        let saved = args.clone();
        cx.command(COMMAND_RELAY, echo(2.0, None, 0))?;
        assert_eq!(bits(&saved.heap), bits(&args.heap));
        Ok(args.clone())
    })
    .unwrap();
    let args = Record {
        row: vec![0.0, 4000.0],
        heap: vec![-0.0; 4000],
    };
    assert_eq!(
        bits(&a.call_import(999, args.clone()).unwrap().heap),
        bits(&args.heap)
    );
    assert_eq!(seen.get(), 2);
    assert_eq!(a.calls().unwrap().read(Echo::take).value, 102.0);
    assert!(a.events().unwrap()[0] > 0.0);
    assert_eq!(b.events().unwrap(), vec![0.0]);
    a.dispose().unwrap();
    b.dispose().unwrap();
}

#[test]
fn native_clock_reentry_preserves_partial_edits_original_errors_and_next_invocation() {
    for message in ["tick failed", ""] {
        let a = Context::new();
        let b = Context::new();
        identity_clock(&b);
        let error = Error::host(message);
        let original = error.clone();
        let nested = Rc::new(Cell::new(false));
        let flag = nested.clone();
        let other = b.clone();
        a.register(IMPORT_WORLD_CLOCK, move |cx, record| {
            let mut args = record.read(WorldClockArgs::take);
            assert_eq!(cx.dispose().unwrap_err().kind(), ErrorKind::Busy);
            if args.tick == 2.0 && !flag.replace(true) {
                assert_eq!(other.clock(clock(state(), 0.05)).unwrap().state.tick, 3.0);
                let inner = cx.clock(clock(state(), 1.0 / 60.0)).unwrap();
                assert_eq!(inner.state.tick, 1.0);
                cx.scratch(&vec![42; 500_000]).unwrap();
                args.state.max_debt = 4.0;
                return Err(Failure {
                    error: error.clone(),
                    partial: Some(reply(args.state, 0.0)),
                });
            }
            Ok(reply(args.state, 0.0))
        })
        .unwrap();
        let failed = a.clock(clock(state(), 0.05)).unwrap_err();
        assert!(failed.error.same(&original));
        assert_eq!(failed.error.message(), message);
        assert_eq!(failed.state.tick, 1.0);
        assert_eq!(failed.state.elapsed, 0.05);
        assert_eq!(failed.state.max_debt, 4.0);
        let published = a.calls().unwrap().read(WorldClockResult::take);
        assert_eq!(published.state, failed.state);
        assert_eq!(
            a.string(published.error.unwrap()).unwrap(),
            message.encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(a.clock(clock(failed.state, 0.0)).unwrap().state.tick, 3.0);
        a.dispose().unwrap();
        b.dispose().unwrap();
    }
}

#[test]
fn native_context_missing_handlers_capacity_and_registration_recover() {
    let cx = Context::new();
    assert_eq!(
        cx.command(COMMAND_RELAY, echo(1.0, None, 0))
            .unwrap_err()
            .error
            .kind(),
        ErrorKind::MissingHandler
    );
    let failed = cx.clock(clock(state(), 0.05)).unwrap_err();
    assert_eq!(failed.error.kind(), ErrorKind::MissingHandler);
    assert_eq!(failed.state.elapsed, 0.05);
    assert_eq!(failed.state.tick, 0.0);
    identity_clock(&cx);
    assert_eq!(cx.clock(clock(failed.state, 0.0)).unwrap().state.tick, 3.0);
    cx.register(IMPORT_ASK, |cx, args| {
        cx.unregister(IMPORT_ASK)?;
        cx.register(IMPORT_ASK, |_, _| Ok(echo(99.0, None, 0)))?;
        Ok(args.clone())
    })
    .unwrap();
    assert_eq!(
        cx.command(COMMAND_RELAY, echo(-0.0, None, 0)).unwrap().row[0].to_bits(),
        (-0.0f64).to_bits()
    );
    assert_eq!(
        cx.command(COMMAND_RELAY, echo(0.0, None, 0)).unwrap().row[0],
        99.0
    );
    assert_eq!(
        cx.call_import(
            IMPORT_ASK,
            Record {
                row: vec![0.0; 8192],
                heap: vec![]
            }
        )
        .unwrap_err()
        .error
        .kind(),
        ErrorKind::ExchangeCapacity
    );
    assert_eq!(
        cx.command(999, Record::default()).unwrap_err().error.kind(),
        ErrorKind::UnsupportedCommand
    );
    assert_eq!(
        cx.command(COMMAND_RELAY, echo(0.0, None, 0)).unwrap().row[0],
        99.0
    );
    cx.dispose().unwrap();
}

#[test]
fn native_clock_unmatched_panics_propagate_payload_and_release_invocation_guards() {
    let cx = Context::new();
    cx.register(IMPORT_WORLD_CLOCK, |_, _| {
        resume_unwind(Box::new(123456u64))
    })
    .unwrap();
    let panic = catch_unwind(AssertUnwindSafe(|| cx.clock(clock(state(), 0.05)))).unwrap_err();
    assert_eq!(*panic.downcast::<u64>().unwrap(), 123456);
    identity_clock(&cx);
    assert_eq!(cx.clock(clock(state(), 0.05)).unwrap().state.tick, 3.0);
    cx.dispose().unwrap();
}

#[test]
fn native_raw_relay_rejects_without_calling_an_extern_c_callback() {
    assert!(!bfe_sim::abi::command_available(COMMAND_RELAY));
    assert!(bfe_sim::gen::cmd_relay(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0).is_null());
}
