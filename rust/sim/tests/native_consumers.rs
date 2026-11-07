#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
use bfe_sim::abi::context::{Context, Error, ErrorKind, Failure, Record};
use bfe_sim::gen::*;
use std::cell::Cell;
use std::rc::Rc;
macro_rules! record {
    ($ty:ty, $value:expr) => {{
        let value = $value;
        Record::new(<$ty>::WORDS, |r, h| value.put(r, h))
    }};
}
fn event_input(op: WorldEventOperation) -> WorldEventInput {
    WorldEventInput {
        op,
        state: WorldEventState {
            capacity: 4.0,
            seq: 2.0,
            ..Default::default()
        },
    }
}
fn local_input() -> WorldLocalInput {
    WorldLocalInput {
        op: WorldLocalOperation::Tail,
        dt: 1.0 / 60.0,
        ..Default::default()
    }
}
fn network_input() -> WorldNetworkInput {
    WorldNetworkInput {
        tick: 4.0,
        at: 501.0,
        grace: 3600.0,
        respawn: 4.0,
        ..Default::default()
    }
}

#[test]
fn native_input_consumers_keep_colliding_string_ids_and_partial_errors_local() {
    let a = Context::new();
    let b = Context::new();
    let jump = a
        .intern(&"jump".encode_utf16().collect::<Vec<_>>())
        .unwrap();
    let fire = b
        .intern(&"firePrimary".encode_utf16().collect::<Vec<_>>())
        .unwrap();
    assert_eq!(jump, fire);
    let input = IaInput {
        op: IaOperation::Push,
        state: IaState {
            capacity: 4.0,
            ..Default::default()
        },
        entry: Some(IaEntry {
            id: 1.0,
            tick: 1.0,
            generation: 1.0,
            actions: vec![jump, jump],
            ..Default::default()
        }),
        tick: 1.0,
        same_epoch: true,
        same_generation: true,
        payload_valid: true,
        human_valid: true,
        ..Default::default()
    };
    let accepted = a.input_actions(input.clone()).unwrap();
    let rejected = b.input_actions(input).unwrap();
    assert!(accepted.accepted);
    assert_eq!(accepted.state.received, 1.0);
    assert!(!rejected.accepted);
    assert_eq!(rejected.state.received, 0.0);
    assert_eq!(
        b.string(rejected.error.unwrap()).unwrap(),
        "invalid reliable actions"
            .encode_utf16()
            .collect::<Vec<_>>()
    );
    let retained = a.calls().unwrap();
    let first = a
        .input_actions(IaInput {
            op: IaOperation::Take,
            state: accepted.state,
            tick: 1.0,
            generation: 1.0,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(first.actions, vec![jump]);
    assert_eq!(first.state.queue[0].actions, vec![jump]);
    assert_eq!(
        retained.read(IaResult::take).state.queue[0].actions.len(),
        2
    );

    let out = a
        .input_mailbox(ImInput {
            op: ImOperation::Take,
            tick: 2.0,
            generation: 1.0,
            state: ImState {
                capacity: 4.0,
                queue: vec![ImEntry {
                    sequence: 1.0,
                    tick: 1.0,
                    generation: 1.0,
                    actions: vec![jump; 2049],
                    down: 17.0,
                    look_total: Some([-0.0, 12.5]),
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    assert!(out.error.is_some());
    assert!(out.command.is_none());
    assert!(out.state.queue.is_empty());
    assert_eq!(out.state.ack, 1.0);
    assert_eq!(out.state.down, Some(17.0));
    assert_eq!(out.state.look_total, [-0.0, 12.5]);
    let recovered = a
        .input_mailbox(ImInput {
            op: ImOperation::Take,
            state: out.state,
            tick: 3.0,
            generation: 1.0,
            ..Default::default()
        })
        .unwrap();
    assert!(recovered.error.is_none());

    let status = a
        .intern(&"manual".encode_utf16().collect::<Vec<_>>())
        .unwrap();
    let out = a
        .input_fire(IfInput {
            op: IaOperation::Take,
            tick: 2.0,
            generation: 2.0,
            state: IfState {
                generation: Some(1.0),
                buttons: [1.0; 2],
                outcomes: Some(vec![IaOutcome { id: 0.0, status }; 64]),
                queue: vec![IfEdge {
                    id: 1.0,
                    tick: 1.0,
                    generation: 1.0,
                    pressed: true,
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    assert!(out.error.is_some());
    assert!(out.replaced_buttons);
    assert!(!out.replaced_queue);
    assert_eq!(out.state.generation, Some(2.0));
    assert_eq!(out.state.queue.len(), 1);
    assert_eq!(out.state.buttons, [0.0; 2]);
    a.dispose().unwrap();
    b.dispose().unwrap();
    assert_eq!(
        a.input_controls(IcInput::default()).unwrap_err().kind(),
        ErrorKind::Disposed
    );
}

#[test]
fn native_controls_queue_and_axis_use_owned_records_and_recover_after_growth() {
    let cx = Context::new();
    let mut state = IqState {
        capacity: 2.0,
        items_length: 2.0,
        ..Default::default()
    };
    for name in [5.0, 6.0] {
        state = cx
            .input_queue(IqInput {
                op: IqOperation::Add,
                state,
                name,
            })
            .unwrap()
            .state;
    }
    let out = cx
        .input_queue(IqInput {
            op: IqOperation::Add,
            state,
            name: 7.0,
        })
        .unwrap();
    assert!(out.error.is_some());
    assert_eq!(out.state.length, 2.0);
    let saved = cx.calls().unwrap();
    let deleted = cx
        .input_queue(IqInput {
            op: IqOperation::Delete,
            state: out.state,
            name: 6.0,
        })
        .unwrap();
    assert!(deleted.accepted);
    let added = cx
        .input_queue(IqInput {
            op: IqOperation::Add,
            state: deleted.state,
            name: 7.0,
        })
        .unwrap();
    assert!(added.replaced_items);
    assert!(added.error.is_none());
    cx.scratch(&vec![0; 900_000]).unwrap();
    assert!(saved.read(IqResult::take).error.is_some());
    let controls = cx
        .input_controls(IcInput {
            op: IcOperation::Accept,
            epoch: Some(2.0),
            generation: Some(1.0),
            sequence: Some(3.0),
            state: IcState {
                epoch: Some(1.0),
                generation: Some(1.0),
                sequence: Some(4.0),
                down: vec![5.0],
                queue: added.state,
            },
        })
        .unwrap();
    assert!(controls.accepted);
    assert!(controls.state.down.is_empty());
    assert_eq!(controls.state.queue.length, 0.0);
    let rejected = cx
        .input_controls(IcInput {
            op: IcOperation::Accept,
            state: controls.state,
            epoch: Some(2.0),
            generation: Some(1.0),
            sequence: Some(2.0),
        })
        .unwrap();
    assert!(!rejected.accepted);
    let saved = cx.calls().unwrap();
    assert_eq!(
        cx.input_axis(IkInput {
            down: vec![4.0],
            plus: vec![3.0],
            minus: vec![]
        })
        .unwrap(),
        1.0
    );
    assert_eq!(
        cx.input_axis(IkInput {
            down: vec![4.0],
            plus: vec![3.0],
            minus: vec![4.0]
        })
        .unwrap()
        .to_bits(),
        0.0f64.to_bits()
    );
    assert_eq!(cx.calls().unwrap().row, saved.row);
    cx.dispose().unwrap();
}

#[test]
fn native_event_reader_keeps_failed_delivery_cursor_and_nested_partial_state() {
    let cx = Context::new();
    let original = Error::host("");
    let error = original.clone();
    let nested = Rc::new(Cell::new(false));
    let flag = nested.clone();
    cx.register(IMPORT_WORLD_EVENTS, move |cx, record| {
        let mut args = record.read(WorldEventArgs::take);
        assert_eq!(cx.dispose().unwrap_err().kind(), ErrorKind::Busy);
        if args.op == WorldEventCall::Deliver && !flag.replace(true) {
            args.state = cx
                .world_events(WorldEventInput {
                    op: WorldEventOperation::Emit,
                    state: args.state,
                })
                .unwrap();
            cx.scratch(&vec![1; 200_000])?;
            args.state.time = -0.0;
            return Err(Failure {
                error: error.clone(),
                partial: Some(record!(
                    WorldEventReply,
                    WorldEventReply {
                        state: args.state,
                        present: true
                    }
                )),
            });
        }
        Ok(record!(
            WorldEventReply,
            WorldEventReply {
                state: args.state,
                present: true
            }
        ))
    })
    .unwrap();
    let failure = cx
        .world_events(event_input(WorldEventOperation::Read))
        .unwrap_err();
    assert!(failure.error.same(&original));
    assert_eq!(failure.state.cursor, 1.0);
    assert_eq!(failure.state.seq, 3.0);
    assert_eq!(failure.state.time.to_bits(), (-0.0f64).to_bits());
    assert_eq!(cx.calls().unwrap().read(WorldEventState::take).cursor, 1.0);
    let state = cx
        .world_events(WorldEventInput {
            op: WorldEventOperation::Read,
            state: failure.state,
        })
        .unwrap();
    assert_eq!(state.cursor, 3.0);
    cx.dispose().unwrap();
}

#[test]
fn native_world_consumers_missing_handlers_and_unmatched_panics_release_guards() {
    let cx = Context::new();
    assert_eq!(
        cx.world_events(event_input(WorldEventOperation::Emit))
            .unwrap_err()
            .error
            .kind(),
        ErrorKind::MissingHandler
    );
    assert_eq!(
        cx.world_local(local_input()).unwrap_err().error.kind(),
        ErrorKind::MissingHandler
    );
    assert_eq!(
        cx.world_network(network_input()).unwrap_err().error.kind(),
        ErrorKind::MissingHandler
    );
    cx.dispose().unwrap();
    assert_eq!(
        cx.world_events(event_input(WorldEventOperation::Reset))
            .unwrap_err()
            .error
            .kind(),
        ErrorKind::Disposed
    );
}
