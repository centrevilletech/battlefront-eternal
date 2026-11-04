//! Replay owned cursor exchanges after JavaScript callback and alias parity.
use bfe_content::{json, Value};
use bfe_sim::prediction::replay::{
    Cursor, FrameInput, MatchingCommand, Phase, ScanEntry, SentCommand, Session, Slot, ViewBinding,
};
use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
};

fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}"))
}

fn words(value: &Value) -> Vec<f64> {
    let hex = value.as_str().expect("hexadecimal f64 words");
    assert_eq!(hex.len() % 16, 0);
    (0..hex.len())
        .step_by(16)
        .map(|i| {
            let mut bytes = [0; 8];
            for (j, byte) in bytes.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&hex[i + j * 2..i + j * 2 + 2], 16).unwrap();
            }
            f64::from_le_bytes(bytes)
        })
        .collect()
}

fn integer(value: &Value) -> u64 {
    let value = value.as_f64().expect("integer");
    assert!(
        value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= 9007199254740991.0
    );
    value as u64
}

#[test]
fn private_cursor_exchanges_match_owned_js_frames() {
    let content = match std::env::var("BFE_REPLAY_CORPUS_KIND").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("synthetic") => false,
        Ok("content") => true,
        _ => panic!("invalid replay corpus kind"),
    };
    assert!(
        !content || std::env::var_os("BFE_REPLAY_CORPUS").is_some(),
        "content mode requires an explicit captured corpus"
    );
    let mut child = None;
    let corpus: Box<dyn BufRead> = if let Some(path) = std::env::var_os("BFE_REPLAY_CORPUS") {
        Box::new(BufReader::new(
            std::fs::File::open(path).expect("read replay corpus"),
        ))
    } else {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut process =
            Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
                .arg(root.join("tools/prediction-replay-twin.mjs"))
                .current_dir(root)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("run replay oracle");
        let stdout = process.stdout.take().unwrap();
        child = Some(process);
        Box::new(BufReader::new(stdout))
    };
    let mut owners = HashMap::<u64, Session>::new();
    let mut seen = HashSet::new();
    let mut labels = HashSet::new();
    let mut counts = [0usize; 4];
    let mut created = 0;
    let mut dropped = 0;
    let mut peak = 0;
    let mut early = 0;
    for (line, source) in corpus.lines().enumerate() {
        let event = json::parse(&source.unwrap()).unwrap();
        let label = get(&event, "label").as_str().unwrap();
        let serial = integer(get(&event, "session"));
        assert!(serial > 0, "{label} missing owner serial");
        assert!(
            matches!(get(&event, "error"), Value::Null),
            "{label} public replay ABI error"
        );
        let input = words(get(&event, "input"));
        let expected = words(get(&event, "output"));
        if let Some(operation) = get(&event, "op").as_str() {
            assert!(
                input.is_empty() && expected.is_empty(),
                "{label} lifecycle payload"
            );
            match operation {
                "create" => {
                    assert!(seen.insert(serial), "{label} reused owner serial {serial}");
                    assert!(owners.insert(serial, Session::default()).is_none());
                    created += 1;
                    peak = peak.max(owners.len());
                }
                "drop" => {
                    let owner = owners.remove(&serial).expect("drop a live owner");
                    if owner
                        .cursor
                        .as_ref()
                        .is_some_and(|cursor| cursor.position().phase != Phase::Done)
                    {
                        early += 1;
                    }
                    dropped += 1;
                }
                _ => panic!("{label} unknown lifecycle operation {operation}"),
            }
        } else {
            let op = integer(get(&event, "op"));
            assert!(op < counts.len() as u64);
            let owner = owners.get_mut(&serial).expect("operation on a live owner");
            owner.input = input;
            assert_eq!(
                owner.run(op as u32),
                Ok(()),
                "{label} owner {serial} operation {line}"
            );
            assert_eq!(owner.output.len(), expected.len(), "{label} output length");
            for (word, (actual, expected)) in owner.output.iter().zip(expected).enumerate() {
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "{label} owner {serial} operation {line} word {word}"
                );
            }
            counts[op as usize] += 1;
        }
        labels.insert(label.to_owned());
    }
    if let Some(mut child) = child {
        assert!(child.wait().unwrap().success(), "replay oracle failed");
    }
    assert!(owners.is_empty(), "all replay owners must be dropped");
    assert_eq!(created, dropped);
    if content {
        assert!(created >= 10 && counts[0] >= 10 && counts[1] >= 100 && counts[2] >= 100);
    } else {
        assert!(created >= 40 && counts[0] >= 40 && counts[1] > 500 && counts[2] > 500);
        assert!(
            labels.len() >= 40 && peak > 1,
            "stateful nested owners must be exercised"
        );
        assert!(early > 0, "throw or fallback must close a live prefix");
    }
    eprintln!(
        "replay: {counts:?}, {created} owners, peak {peak}, {early} early drops, {} labels",
        labels.len()
    );
}

fn scan(generation: f64, tick: f64) -> ScanEntry {
    ScanEntry {
        generation,
        tick: Some(tick),
    }
}

fn binding(field: u8, value: u64) -> ViewBinding {
    ViewBinding { field, value }
}

#[test]
fn fixed_endpoint_and_live_index_keep_unsent_tokens_and_view_order() {
    let mut cursor =
        Cursor::begin(0.0, 7.0, &[scan(7.0, 1.0), scan(7.0, 2.0), scan(7.0, 4.0)]).unwrap();
    let first = cursor
        .frame(FrameInput {
            pending_length: 3,
            down: 10,
            view: vec![binding(5, 20), binding(0, 21)],
            slots: vec![
                Slot::Matching(MatchingCommand {
                    tick: 1.0,
                    generation: 7.0,
                    sequence: 30,
                    actions: vec![40, 41],
                    sent: None,
                }),
                Slot::Blocker { tick: 2.0 },
            ],
        })
        .unwrap();
    let owned = first.words();
    assert!(first.any && !first.last);
    assert_eq!(first.position.index, 1);
    assert_eq!(first.down, 10);
    assert_eq!(first.actions, [40, 41]);
    assert!(first.look.is_empty());
    assert_eq!(first.sequences, [30]);
    assert_eq!(cursor.advance().unwrap().tick, 2.0);

    // A callback shifted the old first element and replaced the slot at the
    // retained index. The old second element now lies before that index.
    let second = cursor
        .frame(FrameInput {
            pending_length: 2,
            down: 100,
            view: vec![binding(5, 200), binding(0, 210)],
            slots: vec![Slot::Matching(MatchingCommand {
                tick: 2.0,
                generation: 7.0,
                sequence: 31,
                actions: vec![42],
                sent: Some(SentCommand {
                    down: 110,
                    look: vec![50, 50],
                    view: vec![binding(0, 220), binding(3, 230)],
                }),
            })],
        })
        .unwrap();
    assert_eq!(second.position.index, 2);
    assert_eq!(second.sequences, [31]);
    assert_eq!(second.down, 110);
    assert_eq!(second.look, [50, 50]);
    assert_eq!(
        second.view,
        [binding(5, 200), binding(0, 220), binding(3, 230)]
    );
    assert_eq!(cursor.advance().unwrap().tick, 3.0);

    // New later commands are visible, but cannot extend the initial last=4.
    let third = cursor
        .frame(FrameInput {
            pending_length: 4,
            down: 1,
            view: vec![binding(6, 0)],
            slots: vec![
                Slot::Foreign {
                    tick: 3.0,
                    generation: 8.0,
                },
                Slot::Blocker { tick: 1000.0 },
            ],
        })
        .unwrap();
    assert!(!third.any && third.actions.is_empty() && third.sequences.is_empty());
    assert_eq!(third.position.index, 3);
    assert_eq!(
        third.view,
        [binding(6, 0)],
        "present undefined token remains a view entry"
    );
    cursor.advance().unwrap();
    let fourth = cursor
        .frame(FrameInput {
            pending_length: 4,
            down: 1,
            view: vec![],
            slots: vec![Slot::Blocker { tick: 1000.0 }],
        })
        .unwrap();
    assert!(fourth.last && !fourth.any);
    let done = cursor.advance().unwrap();
    assert_eq!(done.last, 4.0);
    assert_eq!(done.tick, 5.0);
    assert_eq!(done.phase, Phase::Done);
    assert_eq!(
        first.words(),
        owned,
        "later token arenas cannot alter an owned frame"
    );
}

#[test]
fn initial_scan_keeps_late_fractional_and_zero_return_rules() {
    let none = Cursor::begin(
        -0.0,
        3.0,
        &[ScanEntry {
            generation: 4.0,
            tick: None,
        }],
    )
    .unwrap()
    .position();
    assert!(!none.mine && none.phase == Phase::Done);
    assert_eq!(none.last.to_bits(), (-0.0f64).to_bits());
    let fractional = Cursor::begin(0.5, 3.0, &[scan(3.0, 1.0)])
        .unwrap()
        .position();
    assert!(fractional.mine, "the host still copies initialView");
    assert_eq!(fractional.last, 1.0);
    assert_eq!(fractional.tick, 1.5);
    assert_eq!(fractional.phase, Phase::Done);
    let late = Cursor::begin(10.0, 3.0, &[scan(3.0, 2.0)])
        .unwrap()
        .position();
    assert_eq!(
        (late.last, late.tick, late.phase),
        (11.0, 11.0, Phase::Ready)
    );

    let mut fractional = Cursor::begin(0.0, 3.0, &[scan(3.0, 2.25)]).unwrap();
    for tick in [1.0, 2.0] {
        let frame = fractional
            .frame(FrameInput {
                pending_length: 1,
                down: 0,
                view: vec![],
                slots: vec![Slot::Blocker { tick: 2.25 }],
            })
            .unwrap();
        assert_eq!(frame.position.tick, tick);
        assert!(
            !frame.last,
            "last flag uses equality, not the final loop iteration"
        );
        fractional.advance().unwrap();
    }
    assert_eq!(fractional.position().phase, Phase::Done);
    assert_eq!(fractional.position().last, 2.25);
}

#[test]
fn unsupported_arithmetic_is_rejected_without_clamping() {
    for from in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        4503599627370496.0,
        -4503599627370496.0,
    ] {
        assert_eq!(
            Cursor::begin(from, 1.0, &[]).unwrap_err(),
            "unsupported replay arithmetic"
        );
    }
    assert_eq!(
        Cursor::begin(0.0, f64::NAN, &[]).unwrap_err(),
        "unsupported replay arithmetic"
    );
    assert_eq!(
        Cursor::begin(0.0, 1.0, &[scan(1.0, f64::INFINITY)]).unwrap_err(),
        "unsupported replay arithmetic"
    );
    let foreign = Cursor::begin(0.0, 1.0, &[scan(2.0, f64::NAN)]).unwrap();
    assert!(
        !foreign.position().mine,
        "foreign ticks are not read by Begin"
    );
    let mut boundary = Cursor::begin(4503599627370495.5, 1.0, &[scan(1.0, 0.0)]).unwrap();
    let first = boundary.position();
    assert_eq!(first.tick, 4503599627370496.0);
    assert_eq!(first.last, first.tick);
    boundary
        .frame(FrameInput {
            pending_length: 0,
            down: 0,
            view: vec![],
            slots: vec![],
        })
        .unwrap();
    assert_eq!(boundary.advance().unwrap().phase, Phase::Done);
}

#[test]
fn malformed_frames_clear_output_without_consuming_the_cursor() {
    let mut session = Session::default();
    session.input = vec![0.0, 7.0, 1.0, 7.0, 1.0, 2.0];
    session.run(0).unwrap();
    let begin = session.output.clone();
    let position = session.cursor.as_ref().unwrap().position();
    for input in [
        vec![],
        vec![1.0, 0.0, 0.0, 0.0],                // missing reachable slot
        vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0],      // blocker is already due
        vec![1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 7.0], // foreign row matches
        vec![1.0, 0.0, 0.0, 1.0, 0.0, f64::NAN],
        vec![1.0, f64::INFINITY, 0.0, 0.0],
        vec![1.0, 0.0, 2.0, 0.0, 1.0, 0.0, 2.0, 0.0], // duplicate view key
        vec![1.0, 0.0, 0.0, 1.0, 2.0, 1.0, 7.0, 30.0, 0.0], // missing sent flag
        vec![1.0, 0.0, 0.0, 1.0, 0.0, 2.0, 99.0],     // trailing word
    ] {
        session.input = input;
        assert!(session.run(1).is_err());
        assert!(session.output.is_empty());
        assert_eq!(session.cursor.as_ref().unwrap().position(), position);
    }
    session.input = vec![1.0, 0.0, 0.0, 1.0, 0.0, 2.0];
    session.run(1).unwrap();
    let frame = session.output.clone();
    assert_eq!(session.run(1), Err("invalid replay phase"));
    assert!(session.output.is_empty());
    session.input.clear();
    session.run(3).unwrap();
    assert_eq!(session.output[4], Phase::AwaitApply as u8 as f64);
    session.run(2).unwrap();
    assert_eq!(session.output[3], 2.0);
    assert_eq!(begin, [1.0, 2.0, 0.0, 1.0, 1.0]);
    assert_eq!(frame[3], 1.0);
}

#[test]
fn invalid_later_slot_and_view_updates_roll_back_the_whole_frame() {
    let mut cursor = Cursor::begin(0.0, 1.0, &[scan(1.0, 1.0)]).unwrap();
    let before = cursor.position();
    let sent = |view| {
        Slot::Matching(MatchingCommand {
            tick: 1.0,
            generation: 1.0,
            sequence: 5,
            actions: vec![6],
            sent: Some(SentCommand {
                down: 7,
                look: vec![8],
                view,
            }),
        })
    };
    let invalid = FrameInput {
        pending_length: 2,
        down: 0,
        view: vec![],
        slots: vec![
            sent(vec![]),
            Slot::Foreign {
                tick: 1.0,
                generation: 1.0,
            },
        ],
    };
    assert_eq!(
        cursor.frame(invalid).unwrap_err(),
        "invalid replay exchange"
    );
    assert_eq!(cursor.position(), before);
    for view in [
        vec![binding(2, 1), binding(1, 2)],
        vec![binding(7, 1)],
        vec![binding(0, 9007199254740992)],
    ] {
        assert_eq!(
            cursor
                .frame(FrameInput {
                    pending_length: 1,
                    down: 0,
                    view: vec![],
                    slots: vec![sent(view)]
                })
                .unwrap_err(),
            "invalid replay exchange"
        );
        assert_eq!(cursor.position(), before);
    }
    let kept = cursor
        .frame(FrameInput {
            pending_length: 1,
            down: 0,
            view: vec![binding(6, 0)],
            slots: vec![sent(vec![binding(0, 0), binding(3, 9)])],
        })
        .unwrap();
    assert_eq!(kept.view, [binding(6, 0), binding(0, 0), binding(3, 9)]);
    assert_eq!(cursor.advance().unwrap().phase, Phase::Done);
}
