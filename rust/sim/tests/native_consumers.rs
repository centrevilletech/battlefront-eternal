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

    let mut strings = bfe_sim::store::Strings::default();
    let mut mailbox = bfe_sim::input::owner::Mailbox::new(1., 4., Vec::new(), &mut strings).unwrap();
    mailbox.state = ImState { capacity: 4., ..Default::default() };
    let request = ImInput {
            op: ImOperation::Take,
            tick: 2.0,
            generation: 1.0,
            state: ImState {
                capacity: 4.0,
                // Each admitted entry fits; a paused burst is split across
                // takes before it can exceed the reserved action batch.
                queue: vec![ImEntry {
                    sequence: 1.0,
                    tick: 1.0,
                    generation: 1.0,
                    actions: vec![jump; 2048],
                    down: 17.0,
                    look_total: Some([-0.0, 12.5]),
                    ..Default::default()
                }, ImEntry {
                    sequence: 2.0,
                    tick: 1.0,
                    generation: 1.0,
                    actions: vec![jump],
                    down: 34.0,
                    look_total: Some([-0.0, 25.0]),
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        };
    mailbox.state = request.state.clone();
    let out = mailbox.run(request, &mut strings);
    assert!(out.error.is_none());
    assert_eq!(out.command.as_ref().unwrap().actions, vec![jump; 2048]);
    assert_eq!(mailbox.state.queue.len(), 1);
    assert_eq!(mailbox.state.queue[0].sequence, 2.0);
    assert_eq!(mailbox.state.ack, 1.0);
    assert_eq!(mailbox.state.down, Some(17.0));
    assert_eq!(mailbox.state.look_total, [-0.0, 12.5]);
    let recovered = mailbox.run(ImInput {
            op: ImOperation::Take,
            tick: 3.0,
            generation: 1.0,
            ..Default::default()
        }, &mut strings);
    assert!(recovered.error.is_none());
    assert_eq!(recovered.command.unwrap().actions, vec![jump]);
    assert!(mailbox.state.queue.is_empty());
    assert_eq!(mailbox.state.ack, 2.0);
    assert_eq!(mailbox.state.down, Some(34.0));
    assert_eq!(mailbox.state.look_total, [-0.0, 25.0]);

    // Native admission rejects an oversized record before queue or held state
    // can change. Retained malformed rows still retire once during a take.
    let mut mailbox = bfe_sim::input::owner::Mailbox::new(1., 4., Vec::new(), &mut strings).unwrap();
    let entry = ImEntry {
        sequence: 1.0, tick: 1.0, generation: 1.0,
        actions: vec![jump; 2049], down: 17.0,
        look_total: Some([-0.0, 12.5]), ..Default::default()
    };
    let oversized = mailbox.run(ImInput {
        op: ImOperation::Push, entry: Some(entry.clone()), ..Default::default()
    }, &mut strings);
    assert!(oversized.error.is_some());
    assert!(oversized.command.is_none());
    assert!(mailbox.state.queue.is_empty());
    assert_eq!(mailbox.state.ack, 0.0);
    assert_eq!(mailbox.state.down, None);
    assert_eq!(mailbox.state.look_total, [0.0, 0.0]);

    // Corrupt retained data must not stall the next tick or grow its command
    // scratch. Keep the original watermarks while omitting the oversized data.
    mailbox.state.queue.push(entry);
    let mut retired = mailbox.run(ImInput {
        op: ImOperation::Take, tick: 2.0, generation: 1.0, ..Default::default()
    }, &mut strings);
    assert!(retired.error.is_none());
    let command = retired.command.as_ref().unwrap();
    assert!(command.actions.is_empty() && command.look.is_empty()
        && command.human.is_empty() && command.fire.is_empty());
    assert!(mailbox.state.queue.is_empty());
    assert_eq!(mailbox.state.ack, 1.0);
    assert_eq!(mailbox.state.down, Some(17.0));
    assert_eq!(mailbox.state.look_total, [-0.0, 12.5]);
    mailbox.recycle(&mut retired);
    let recovered = mailbox.run(ImInput {
        op: ImOperation::Take, tick: 3.0, generation: 1.0, ..Default::default()
    }, &mut strings);
    assert!(recovered.error.is_none());
    assert!(recovered.command.as_ref().unwrap().actions.is_empty());
    assert!(mailbox.state.queue.is_empty());
    assert_eq!(mailbox.state.ack, 1.0);

    // InputMailbox.take defers the next valid command before exceeding its
    // fixed scratch. Both commands must survive a paused burst, in order.
    let mut strings=bfe_sim::store::Strings::default();
    let mut mailbox=bfe_sim::input::owner::Mailbox::new(1.,4.,Vec::new(),&mut strings).unwrap();
    mailbox.state.queue.push(ImEntry {sequence:1.,tick:1.,generation:1.,actions:vec![jump;1025],
        down:17.,look_total:Some([-0.,12.5]),..Default::default()});
    mailbox.state.queue.push(ImEntry {sequence:2.,tick:2.,generation:1.,actions:vec![jump;1025],
        down:19.,look_total:Some([3.,13.5]),..Default::default()});
    let out=mailbox.run(ImInput {op:ImOperation::Take,tick:2.,generation:1.,..Default::default()},&mut strings);
    assert!(out.error.is_none());
    assert_eq!(out.command.as_ref().unwrap().actions,vec![jump;1025]);
    assert_eq!(mailbox.state.queue.len(),1);assert_eq!(mailbox.state.queue[0].sequence,2.);
    assert_eq!(mailbox.state.ack,1.);assert_eq!(mailbox.state.down,Some(17.));
    assert_eq!(mailbox.state.look_total,[-0.,12.5]);
    let recovered=mailbox.run(ImInput {op:ImOperation::Take,tick:3.,generation:1.,..Default::default()},&mut strings);
    assert!(recovered.error.is_none());
    assert!(mailbox.state.queue.is_empty());assert_eq!(mailbox.state.ack,2.);assert_eq!(mailbox.state.down,Some(19.));
    let command=recovered.command.unwrap();
    assert_eq!(command.actions,vec![jump;1025]);
    assert_eq!(command.look[0].point,[3.,1.]);

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
    // A full receipt backlog drops the receipt; the take still finishes.
    assert!(out.error.is_none());
    assert!(out.replaced_buttons);
    assert!(out.replaced_queue);
    assert_eq!(out.state.generation, Some(2.0));
    assert!(out.state.queue.is_empty());
    assert_eq!(out.state.outcomes.as_ref().unwrap().len(), 64);
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
    cx.dispose().unwrap();
    assert_eq!(
        cx.world_events(event_input(WorldEventOperation::Reset))
            .unwrap_err()
            .error
            .kind(),
        ErrorKind::Disposed
    );
}

#[test]
fn retained_mailbox_delivers_owned_choices_and_edges_once_to_native_players() {
    use bfe_sim::input::owner::Mailbox;
    use bfe_sim::store::Strings;
    let mut strings = Strings::default();
    let key = strings.intern_str("KeyW");
    let jump = strings.intern_str("jump");
    let class = strings.intern_str("soldier");
    let mut mailbox = Mailbox::new(1., 4., vec![key], &mut strings).unwrap();
    let choices = vec![
        HsCommand { kind: HsCommandKind::Team, team: 2., ..Default::default() },
        HsCommand { kind: HsCommandKind::Spawn, class_name: Some(class), ..Default::default() },
    ];
    let entry = ImEntry {
        identity: 1., sequence: 20., tick: 5., generation: 1., down: 1.,
        codes: vec![key], actions: vec![jump], human: choices.clone(),
        look_total: Some([12., -6.]),
        fire: vec![ImFire { id: 9., button: 1., pressed: true }],
        ..Default::default()
    };
    let out = mailbox.run(ImInput {
        op: ImOperation::Push, entry: Some(entry), epoch_matches: true,
        ..Default::default()
    }, &mut strings);
    assert!(out.accepted);
    assert!(mailbox.take_command(4., 1., &mut strings));
    assert!(mailbox.taken().human.is_empty());
    assert_eq!(mailbox.state.ack, 0.);
    assert!(mailbox.take_command(5., 1., &mut strings));
    assert_eq!(mailbox.state.ack, 20.);
    assert_eq!(mailbox.down(), &[key]);
    assert_eq!(mailbox.taken().human, choices);
    assert_eq!(mailbox.taken().actions, vec![jump]);
    assert_eq!(mailbox.taken().look[0].point, [12., -6.]);
    assert_eq!(mailbox.taken().fire[0].id, 9.);
    assert!(mailbox.take_command(6., 1., &mut strings));
    assert!(mailbox.taken().human.is_empty());
    assert!(mailbox.taken().actions.is_empty());
    assert!(mailbox.taken().look.is_empty());
    assert!(mailbox.taken().fire.is_empty());
    assert_eq!(mailbox.down(), &[key]);
    mailbox.clear_command(&mut strings);
    assert!(mailbox.down().is_empty());
    assert_eq!(mailbox.state.ack, 20.);
}

#[test]
fn native_player_admission_refuses_excess_and_never_revives_a_retired_handle() {
    use bfe_sim::players::owner::{Player, Players, PLAYER_CAPACITY};
    use bfe_sim::store::Strings;
    let mut strings = Strings::default();
    let mut players = Players::new();
    players.activate(&mut strings, 256);
    let first = players.admit(Player::new(1, 1000.)).unwrap();
    for mailbox in 2..=PLAYER_CAPACITY {
        assert!(players.admit(Player::new(mailbox, 1000.)).is_some());
    }
    // Repeated excess admissions leave the accepted prefix and its order.
    for mailbox in 0..4096 {
        assert!(players.admit(Player::new(mailbox, 1000.)).is_none());
    }
    assert_eq!(players.order.len(), PLAYER_CAPACITY);
    assert!(players.remove(first));
    let replacement = players.admit(Player::new(5000, 1001.)).unwrap();
    assert_ne!(replacement, first);
    assert!(players.pool.get(first).is_none());
    assert_eq!(players.pool.get(replacement).unwrap().mailbox, 5000);
    assert_eq!(players.order.last(), Some(&replacement));
}
