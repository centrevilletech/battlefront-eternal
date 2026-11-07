//! The native world API's shape, its bodies and its refusals. The
//! function-pointer coercions fail to compile when a frozen signature in
//! abi/context_step.rs moves; the runtime checks pin what each entry answers
//! (a body's result, or Unavailable while the body has not landed, never a
//! made-up value) and the lifetime rules every body keeps: Disposed after
//! dispose, Poisoned after an escaped panic, dispose still succeeding on a
//! poisoned context.
#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
use bfe_sim::abi::context::{Context, Error, ErrorKind};
use bfe_sim::abi::context_step::{bank, provided, Fault, Provided, Publication};
use bfe_sim::gen::*;
use bfe_sim::store::block::Block;
use serde_json::Value;

#[test]
fn native_world_api_signatures_are_frozen() {
    let _: fn(&Context, WorldTickSetup) -> Result<(), Error> = Context::world_step_init;
    let _: fn(&Context, WorldTickInput) -> Result<WorldTickOutput, Error> = Context::world_step;
    let _: fn(&Context, u32, &[f64]) -> Result<Publication, Error> = Context::world_publish;
    let _: fn(&Context, u32, &[f64], &mut Publication) -> Result<(), Error> = Context::world_publish_into;
    let _: fn(&Context, u32, &mut Vec<f64>) -> Result<usize, Error> = Context::read_bank;
    let _: fn(&Context, u32, &[f64]) -> Result<Block, Fault> = Context::run_operation;
    let _: fn(&Context, u32, &[f64], &mut Block) -> Result<(), Fault> = Context::run_operation_into;
    let _: fn(&Context, &Value) -> Result<WorldTickSetup, Error> = Context::world_replay_admit;
    let _: fn(&Context, &Value) -> Result<(), Error> = Context::world_replay_prepare;
    let _: fn(&Context, &Value, &[WorldTickOutput]) -> Result<Value, Error> = Context::world_replay_publish;
    let _: fn(&Context) -> usize = Context::world_replay_live_handles;
    let _: fn(&Context) -> Result<(), Error> = Context::world_replay_panic;
    let _: fn(&Publication) -> WirePublicationOutput = Publication::output;
    let _: fn() -> Provided = provided;
    // What a host may advertise today: everything but the tick and the replay.
    assert_eq!(provided(), Provided { step: true, publication: true, banks: true, operations: true, replay: false });
    // The bank numbers are part of the table too: the first six as declared
    // with the skeleton, the rest added behind them.
    assert_eq!(
        [bank::F3_PUBLICATION, bank::F2_DELTA, bank::F4_PHASE, bank::AI_BOUNDARY, bank::WIRE_CENSUS, bank::EVENTS],
        [1, 2, 3, 4, 5, 6]
    );
    assert_eq!(bank::ALL.iter().copied().collect::<Vec<_>>(), (1..=28).collect::<Vec<_>>());
}

fn tick() -> WorldTickInput {
    WorldTickInput {
        dt: 1.0 / 60.0,
        tick: 1.0,
        clock: WorldClockState { hz: 60.0, dt: 1.0 / 60.0, ..Default::default() },
        ..Default::default()
    }
}

fn kind_of<T>(result: Result<T, Error>) -> ErrorKind {
    match result {
        Ok(_) => panic!("an entry answered where a refusal was due"),
        Err(error) => error.kind(),
    }
}

fn fault_kind(fault: Fault) -> ErrorKind {
    match fault {
        Fault::Context(error) => error.kind(),
        other => panic!("run_operation answered {other:?} where a context refusal was due"),
    }
}

/// Every entry's refusal on a context that can no longer be used.
fn refusals(context: &Context) -> Vec<ErrorKind> {
    let mut words = Vec::new();
    vec![
        kind_of(context.world_step_init(WorldTickSetup::default())),
        kind_of(context.world_step(tick())),
        kind_of(context.world_publish(1, &[0.0; WireWorldFacts::WORDS])),
        kind_of(context.read_bank(bank::EVENTS, &mut words)),
        fault_kind(context.run_operation(OPERATION_ECHO, &[0.0; Echo::WORDS]).err().unwrap()),
        kind_of(context.world_replay_admit(&Value::Null)),
        kind_of(context.world_replay_prepare(&Value::Null)),
        kind_of(context.world_replay_publish(&Value::Null, &[])),
        kind_of(context.world_replay_panic()),
    ]
}

#[test]
fn entries_without_a_body_fail_unavailable_and_name_themselves() {
    let context = Context::new();
    for (entry, result) in [
        ("world_replay_admit", context.world_replay_admit(&Value::Null).map(|_| ())),
        ("world_replay_prepare", context.world_replay_prepare(&Value::Null)),
        ("world_replay_publish", context.world_replay_publish(&Value::Null, &[]).map(|_| ())),
    ] {
        let error = result.err().unwrap();
        assert_eq!((error.kind(), error.message()), (ErrorKind::Unavailable, entry));
    }
    // The refusals leave the context usable, like the WASM module's null.
    assert!(context.intern(&[65]).is_ok());
    assert_eq!(context.world_replay_live_handles(), 1);
    context.dispose().unwrap();
    assert_eq!(context.world_replay_live_handles(), 0);
    assert!(refusals(&context).iter().all(|kind| *kind == ErrorKind::Disposed));
    assert_eq!(context.dispose().unwrap_err().kind(), ErrorKind::Disposed);
}

#[test]
fn an_escaped_panic_poisons_every_entry_but_disposal() {
    let context = Context::new();
    let alias = context.clone();
    assert_eq!(context.world_replay_panic().unwrap_err().kind(), ErrorKind::Poisoned);
    assert!(refusals(&alias).iter().all(|kind| *kind == ErrorKind::Poisoned));
    assert_eq!(alias.intern(&[65]).unwrap_err().kind(), ErrorKind::Poisoned);
    assert_eq!(context.events().unwrap_err().kind(), ErrorKind::Poisoned);
    // Poisoning released the owning borrow: the panic unwound through it.
    assert_eq!(context.world_replay_live_handles(), 1);
    context.dispose().unwrap();
    assert_eq!(alias.world_replay_live_handles(), 0);
    assert_eq!(alias.world_step(tick()).unwrap_err().kind(), ErrorKind::Disposed);
}

#[test]
fn step_setup_binds_once_and_publication_needs_a_session() {
    let context = Context::new();
    context.world_step_init(WorldTickSetup { water: true, water_time: 2.0, water_height: 1.5, water_bob: true }).unwrap();
    context.world_step_init(WorldTickSetup::default()).unwrap();
    // No unit service is prepared: the tick is refused, not run, and the
    // context stays usable.
    let output = context.world_step(tick()).unwrap();
    assert!(!output.accepted);
    assert_eq!(output.clock.tick, 0.0);
    assert!(context.intern(&[66]).is_ok());
    // No wire session has been admitted: the session key names nothing.
    let error = context.world_publish(1, &[0.0; WireWorldFacts::WORDS]).err().unwrap();
    assert_eq!((error.kind(), error.message()), (ErrorKind::InvalidKind, "no such publication session"));
    let mut retained = Publication::default();
    assert_eq!(context.world_publish_into(1, &[0.0; WireWorldFacts::WORDS], &mut retained).unwrap_err().kind(), ErrorKind::InvalidKind);
    context.dispose().unwrap();
}

#[test]
fn operations_run_the_command_bodies_over_argument_rows() {
    let context = Context::new();
    let note = context.intern(&[72, 105]).unwrap();
    let mut row = vec![0.0; Echo::WORDS];
    Echo { value: 41.0, at: [-0.0, f64::INFINITY, f64::from_bits(1)], count: 0, note: Some(note), events: 2 }
        .put(&mut row, &mut Vec::new());
    let block = context.run_operation(OPERATION_ECHO, &row).unwrap();
    let (out, heap) = block.read(0);
    let echo = Echo::take(out, heap);
    assert_eq!(echo.value, 42.0);
    assert_eq!(echo.at.map(f64::to_bits), [(-0.0f64).to_bits(), f64::INFINITY.to_bits(), 1]);
    assert_eq!(echo.note, Some(note));
    // The retained path reuses the host's block: same words, no new buffer.
    let mut retained = Block::default();
    retained.words.reserve(64);
    let (buffer, capacity) = (retained.words.as_ptr(), retained.words.capacity());
    context.run_operation_into(OPERATION_ECHO, &row, &mut retained).unwrap();
    assert_eq!(retained.words.iter().map(|w| w.to_bits()).collect::<Vec<_>>(), block.words.iter().map(|w| w.to_bits()).collect::<Vec<_>>());
    assert_eq!((retained.words.as_ptr(), retained.words.capacity()), (buffer, capacity));
    // The two echoed events wait in the events bank until the host clears it.
    let mut words = Vec::new();
    assert!(context.read_bank(bank::EVENTS, &mut words).unwrap() > 1);
    assert_eq!(words, context.events().unwrap());
    context.clear_events().unwrap();
    assert_eq!(context.read_bank(bank::EVENTS, &mut words).unwrap(), 1);
    // A command without a result answers with an empty block, not a refusal.
    let mut fill = vec![0.0; Fill::WORDS];
    Fill { kind: KIND_BOLT as u32, count: 3 }.put(&mut fill, &mut Vec::new());
    assert!(context.run_operation(OPERATION_FILL, &fill).unwrap().words.is_empty());
    assert_eq!(context.export(KIND_BOLT as u32, 1).unwrap()[BlockHeader::ROWS], 3.0);
    // Relay asks the host back and is not an operation; a wrong row is refused
    // before any body runs.
    assert!(!OPERATION_NAMES.contains(&"relay"));
    assert!(matches!(context.run_operation(9999, &[]), Err(Fault::UnknownOperation(9999))));
    assert!(matches!(
        context.run_operation(OPERATION_ECHO, &row[1..]),
        Err(Fault::Arguments { expected: Echo::WORDS, given }) if given == Echo::WORDS - 1
    ));
    // Export operations take their scalars, then the record the WebAssembly
    // host writes into the call block. A mailbox that does not exist refuses
    // the run (the export answers null) and ignores the view.
    let shape = operation(OPERATION_INPUT_MAILBOX_OWNED_RUN).unwrap();
    assert_eq!((shape.scalars, shape.record, shape.answers), (1, InputMailboxCall::WORDS, true));
    let mut run = vec![0.0; 1 + InputMailboxCall::WORDS];
    run[0] = 7.0;
    assert!(matches!(context.run_operation(OPERATION_INPUT_MAILBOX_OWNED_RUN, &run), Err(Fault::Refused)));
    assert!(matches!(
        context.run_operation(OPERATION_INPUT_MAILBOX_OWNED_RUN, &run[..1]),
        Err(Fault::Arguments { expected, given: 1 }) if expected == 1 + InputMailboxCall::WORDS
    ));
    let view = vec![7.0; 1 + InputViewUpdate::WORDS];
    assert!(context.run_operation(OPERATION_INPUT_MAILBOX_VIEW, &view).unwrap().words.is_empty());
    // A host operation on an absent wire host answers NaN, as the export does.
    let host = context.run_operation(OPERATION_WIRE_HOST_OP, &[3.0, 4.0, 1.0, 0.0, 0.0]).unwrap();
    assert!(host.read(0).0[0].is_nan());
    context.dispose().unwrap();
}

#[test]
fn unprepared_banks_read_empty_and_unknown_banks_are_refused() {
    let context = Context::new();
    let mut words = vec![7.0; 3];
    for bank in bank::ALL {
        // Only the always-present banks have words before any phase is
        // prepared: the events bank's header, the hulls' descriptor and the
        // census, which the wire owner keeps from its construction.
        let count = context.read_bank(bank, &mut words).unwrap();
        assert_eq!(count, words.len(), "bank {bank}");
        let expected = match bank {
            bank::EVENTS => 1,
            bank::F2_PHASE => F2Phase::WORDS,
            bank::WIRE_CENSUS => WireCensus::WORDS,
            _ => 0,
        };
        assert_eq!(count, expected, "bank {bank}");
    }
    let error = context.read_bank(29, &mut words).err().unwrap();
    assert_eq!((error.kind(), error.message()), (ErrorKind::InvalidKind, "no such bank"));
    context.dispose().unwrap();
}
