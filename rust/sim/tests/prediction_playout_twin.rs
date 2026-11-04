//! Replay owned playout exchanges after live JavaScript state and callback parity.
use bfe_content::{json, Value};
use bfe_sim::prediction::playout::{
    advance, arrive, measure_delay, measure_gap, measurement_key, rendered_view_tick, shown_tick,
    AdvanceInput, AdvanceMode, ArrivalInput, Interval, ResetHead, ResetTail, Session,
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
fn bits(values: &[f64]) -> Vec<u64> {
    values.iter().map(|v| v.to_bits()).collect()
}

#[test]
fn playout_exchanges_match_live_js_state_and_callback_observations() {
    let content = match std::env::var("BFE_PLAYOUT_CORPUS_KIND").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("synthetic") => false,
        Ok("content") => true,
        _ => panic!("invalid playout corpus kind"),
    };
    assert!(
        !content || std::env::var_os("BFE_PLAYOUT_CORPUS").is_some(),
        "content mode requires an explicit captured corpus"
    );
    let mut child = None;
    let corpus: Box<dyn BufRead> = if let Some(path) = std::env::var_os("BFE_PLAYOUT_CORPUS") {
        Box::new(BufReader::new(
            std::fs::File::open(path).expect("read playout corpus"),
        ))
    } else {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut process =
            Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
                .arg(root.join("tools/prediction-playout-twin.mjs"))
                .current_dir(root)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("run playout oracle");
        let stdout = process.stdout.take().unwrap();
        child = Some(process);
        Box::new(BufReader::new(stdout))
    };
    let mut session = Session::default();
    let mut counts = [0usize; 13];
    let mut labels = HashSet::new();
    let mut per_label = HashMap::<String, [usize; 13]>::new();
    for (line, source) in corpus.lines().enumerate() {
        let event = json::parse(&source.unwrap()).unwrap();
        let label = get(&event, "label").as_str().unwrap();
        let op = get(&event, "op").as_f64().expect("numeric operation");
        assert!(op.is_finite() && op >= 0.0 && op.fract() == 0.0 && op < counts.len() as f64);
        let op = op as usize;
        assert!(
            matches!(get(&event, "error"), Value::Null),
            "{label} public playout ABI error"
        );
        session.input = words(get(&event, "input"));
        assert_eq!(session.run(op as u32), Ok(()), "{label} operation {line}");
        let expected = words(get(&event, "output"));
        assert_eq!(
            session.output.len(),
            expected.len(),
            "{label} output length"
        );
        for (word, (actual, expected)) in session.output.iter().zip(expected).enumerate() {
            assert_eq!(
                actual.to_bits(),
                expected.to_bits(),
                "{label} operation {line} word {word}"
            );
        }
        counts[op] += 1;
        labels.insert(label.to_owned());
        per_label.entry(label.to_owned()).or_insert([0; 13])[op] += 1;
    }
    if let Some(mut child) = child {
        assert!(child.wait().unwrap().success(), "playout oracle failed");
    }
    let total = counts.iter().sum::<usize>();
    assert!(
        counts.iter().all(|n| *n > 0),
        "every phase must run: {counts:?}"
    );
    if content {
        assert!(labels.len() >= 20 && total > 1000, "actual client coverage");
        for (label, usage) in per_label {
            for op in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12] {
                assert!(usage[op] > 0, "{label} missing phase {op}");
            }
        }
    } else {
        assert!(
            labels.len() >= 30 && total > 4000,
            "synthetic playout coverage"
        );
        for prefix in [
            "clock:",
            "reset:",
            "arrival:",
            "measure:",
            "advance:",
            "interval:",
            "rendered:",
        ] {
            assert!(
                labels.iter().any(|label| label.starts_with(prefix)),
                "{prefix}"
            );
        }
    }
    eprintln!(
        "playout: {counts:?}, {total} exchanges, {} labels",
        labels.len()
    );
}

#[test]
fn reset_and_arrival_phases_preserve_skipped_writes_and_ring_bytes() {
    assert_eq!(
        bits(&ResetHead::new(-0.0).unwrap().words()),
        bits(&[0.0, 0.0, -0.0, 1.0])
    );
    assert_eq!(
        ResetTail::default().words(),
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0]
    );
    let initial = ArrivalInput {
        hz: 60.0,
        window: 3.0,
        latest: 3.0,
        arrived_at: Some(0.0),
        arrived_tick: 3.0,
        gap_at: 2.0,
        gap_count: 2.0,
        ring_length: 3,
        tick: 9.0,
        now: 0.1,
        interval: 3.0,
    };
    let result = arrive(initial).unwrap();
    // Missing packets count against one stream interval, not the tick jump.
    assert_eq!(
        result.words(),
        [1.0, 9.0, 1.0, 2.0, 4.0, 0.0, 1.0, 3.0, 0.1, 9.0]
    );
    let mut ring = [f64::from_bits(0x7ff0_0000_0000_0001), -0.0, 17.0];
    let before = bits(&ring);
    let write = result.ring_write.unwrap();
    ring[write.index] = write.value;
    assert_eq!(&bits(&ring)[..2], &before[..2]);
    assert_eq!(ring[2], 4.0);

    let duplicate = arrive(ArrivalInput {
        tick: 3.0,
        now: 0.2,
        hz: f64::NAN,
        interval: f64::NAN,
        ..initial
    })
    .unwrap();
    assert!(!duplicate.write_latest && !duplicate.advance_gap && !duplicate.write_count);
    assert!(duplicate.ring_write.is_none());
    assert_eq!(duplicate.arrived_at, 0.2);
    assert_eq!(duplicate.arrived_tick, 3.0);
    let short_ring = arrive(ArrivalInput {
        ring_length: 1,
        ..initial
    })
    .unwrap();
    assert!(short_ring.advance_gap && short_ring.ring_write.is_none());
    assert_eq!(short_ring.gap_at, 0.0);
    assert_eq!(short_ring.gap_count, 3.0);
    let early = arrive(ArrivalInput {
        latest: -0.0,
        arrived_at: None,
        arrived_tick: -0.0,
        tick: 0.0,
        now: -0.0,
        ..initial
    })
    .unwrap();
    assert!(!early.write_latest);
    assert_eq!(early.latest.to_bits(), (-0.0f64).to_bits());
    assert_eq!(early.arrived_at.to_bits(), (-0.0f64).to_bits());
    assert_eq!(early.arrived_tick.to_bits(), 0.0f64.to_bits());
}

#[test]
fn percentile_keys_and_delay_keep_v8_zero_and_boundary_rules() {
    let key = measurement_key(3.0, 2.0, -1.0).unwrap();
    assert!(key.changed);
    assert_eq!(key.key, 12290.0);
    assert!(!measurement_key(3.0, 2.0, key.key).unwrap().changed);
    let mut samples = vec![0.0; 120];
    samples[119] = 1000.0;
    let before = samples.clone();
    assert_eq!(measure_gap(&samples), Ok(0.0));
    assert_eq!(samples, before, "sorting owns its copy");
    samples[114..].fill(1000.0);
    assert_eq!(measure_gap(&samples), Ok(1000.0));
    assert_eq!(measure_gap(&[]), Ok(0.0));
    assert_eq!(measure_gap(&[-0.0]).unwrap().to_bits(), (-0.0f64).to_bits());
    assert_eq!(
        measure_gap(&[0.0, -0.0]).unwrap().to_bits(),
        0.0f64.to_bits()
    );
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(measure_gap(&[value]), Err("unsupported playout arithmetic"));
    }
    assert_eq!(measure_delay(4.0, 1.0, 10.0, 2.0), Ok(2.0));
    assert_eq!(
        measure_delay(-0.0, -0.0, -0.0, 0.0).unwrap().to_bits(),
        (-0.0f64).to_bits()
    );
    assert_eq!(
        measure_delay(f64::MAX, f64::MAX, 0.0, 10.0),
        Err("unsupported playout arithmetic")
    );
}

#[test]
fn advance_modes_preserve_start_snap_trim_wait_and_monotone_order() {
    let input = AdvanceInput {
        latest: 100.0,
        delay: 2.0,
        clock: 98.0,
        started: true,
        hz: 60.0,
        trim: 0.1,
        snap: 12.0,
        dt: 1.0 / 60.0,
        waits: 5.0,
    };
    let run = advance(input).unwrap();
    assert_eq!(run.mode, AdvanceMode::Run);
    assert_eq!(run.clock, 99.0);
    assert_eq!(run.rate, 1.0);
    let wait = advance(AdvanceInput { dt: 0.1, ..input }).unwrap();
    assert_eq!(wait.mode, AdvanceMode::Wait);
    assert_eq!(wait.clock, 100.0);
    assert_eq!(wait.waits, 6.0);
    let start = advance(AdvanceInput {
        started: false,
        latest: 1.0,
        ..input
    })
    .unwrap();
    assert_eq!(start.mode, AdvanceMode::Start);
    assert_eq!(start.clock.to_bits(), 0.0f64.to_bits());
    let idle = advance(AdvanceInput {
        started: false,
        latest: 0.0,
        clock: -0.0,
        ..input
    })
    .unwrap();
    assert_eq!(idle.mode, AdvanceMode::Idle);
    assert_eq!(idle.clock.to_bits(), (-0.0f64).to_bits());
    let snap = advance(AdvanceInput {
        clock: 20.0,
        ..input
    })
    .unwrap();
    assert_eq!(snap.mode, AdvanceMode::Snap);
    assert_eq!(snap.clock, 98.0);
    assert_eq!(snap.rate, 1.0);
    assert_eq!(
        advance(AdvanceInput {
            clock: 86.0,
            ..input
        })
        .unwrap()
        .mode,
        AdvanceMode::Run
    );
    assert_eq!(
        advance(AdvanceInput { dt: -1.0, ..input }).unwrap().clock,
        98.0
    );
    assert_eq!(
        advance(AdvanceInput {
            trim: -0.1,
            ..input
        })
        .unwrap()
        .rate,
        0.9
    );
    assert_eq!(
        advance(AdvanceInput {
            dt: f64::MAX,
            ..input
        })
        .err(),
        Some("unsupported playout arithmetic")
    );
    assert_eq!(
        advance(AdvanceInput { hz: 0.0, ..input }).err(),
        Some("unsupported playout arithmetic")
    );
    assert_eq!(shown_tick(-1.0).unwrap().to_bits(), 0.0f64.to_bits());
}

#[test]
fn interval_nullable_short_circuits_and_reapplication_retain_exact_values() {
    let mut interval = Interval {
        current: None,
        previous: Some(f64::NAN),
    };
    assert_eq!(interval.needs_sample(f64::NAN), Ok(true));
    assert_eq!(interval.alpha(f64::NAN), Ok(1.0));
    interval.apply(-0.0).unwrap();
    assert_eq!(interval.previous, Some(-1.0));
    assert_eq!(interval.current.unwrap().to_bits(), (-0.0f64).to_bits());
    interval.apply(3.0).unwrap();
    assert_eq!(interval.previous.unwrap().to_bits(), (-0.0f64).to_bits());
    assert_eq!(interval.alpha(1.5), Ok(0.5));
    assert_eq!(interval.alpha(-0.0).unwrap().to_bits(), 0.0f64.to_bits());
    assert_eq!(interval.alpha(4.0), Ok(1.0));
    assert_eq!(interval.needs_sample(3.0), Ok(true));
    assert_eq!(interval.needs_sample(2.0), Ok(false));
    let flat = Interval {
        current: Some(2.0),
        previous: Some(2.0),
    };
    assert_eq!(flat.alpha(2.5), Ok(0.5));
    assert_eq!(flat.alpha(1.0).unwrap().to_bits(), 0.0f64.to_bits());
    assert_eq!(flat.alpha(4.0), Ok(1.0));
    let before = bits(&interval.words());
    assert_eq!(
        interval.apply(f64::INFINITY),
        Err("unsupported playout arithmetic")
    );
    assert_eq!(bits(&interval.words()), before);
    interval.reset();
    assert_eq!(interval.words(), [0.0; 4]);
    assert_eq!(rendered_view_tick(None, Some(f64::NAN)), Ok(0.0));
    assert_eq!(rendered_view_tick(Some(3.9), Some(2.9)), Ok(2.0));
    assert_eq!(
        rendered_view_tick(Some(-0.0), Some(-0.0))
            .unwrap()
            .to_bits(),
        0.0f64.to_bits()
    );
}

#[test]
fn exchanges_own_results_and_reuse_safely_after_every_error() {
    let mut session = Session::default();
    session.input = vec![-0.0];
    assert_eq!(session.run(0), Ok(()));
    let kept = session.output.clone();
    for (op, input, error) in [
        (0, vec![], "invalid playout exchange"),
        (0, vec![2.0, 3.0], "invalid playout exchange"),
        (1, vec![1.0], "invalid playout exchange"),
        (4, vec![2.0, 1.0], "invalid playout exchange"),
        (4, vec![-1.0], "invalid playout exchange"),
        (4, vec![f64::INFINITY], "invalid playout exchange"),
        (4, vec![1.0, f64::NAN], "unsupported playout arithmetic"),
        (
            6,
            vec![1.0, 2.0, 0.0, 2.0, 60.0, 0.1, 12.0, 0.1, 0.0],
            "invalid playout exchange",
        ),
        (
            9,
            vec![0.0, 0.0, f64::INFINITY],
            "unsupported playout arithmetic",
        ),
        (
            12,
            vec![1.0, f64::NAN, 1.0, 2.0],
            "unsupported playout arithmetic",
        ),
        (13, vec![], "invalid playout exchange"),
    ] {
        session.input = input;
        assert_eq!(session.run(op), Err(error));
        assert!(session.output.is_empty());
        session.input.clear();
        assert_eq!(session.run(8), Ok(()));
        assert_eq!(session.output, vec![0.0; 4]);
    }
    assert_eq!(bits(&kept), bits(&[0.0, 0.0, -0.0, 1.0]));
    session.input = vec![0.0, f64::NAN, 1.0, f64::NAN, f64::INFINITY];
    assert_eq!(session.run(11), Ok(()));
    assert_eq!(session.output, vec![1.0]);
}
