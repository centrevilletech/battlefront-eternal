//! Exact correction reads replayed from the same byte payloads checked in JS.
use bfe_content::{json, Value};
use bfe_sim::prediction::corrections::Session;
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
};

fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}"))
}
fn words(value: &Value) -> Vec<f64> {
    let hex = value.as_str().unwrap();
    assert_eq!(hex.len() % 16, 0);
    (0..hex.len())
        .step_by(16)
        .map(|i| {
            let mut bytes = [0; 8];
            for j in 0..8 {
                bytes[j] = u8::from_str_radix(&hex[i + j * 2..i + j * 2 + 2], 16).unwrap();
            }
            f64::from_le_bytes(bytes)
        })
        .collect()
}

#[test]
fn flight_floor_nan_bits_and_owned_results_survive_later_reads() {
    use bfe_sim::prediction::corrections::{decode_compact_flight, decode_flight};
    fn command(bytes: &[u8], metadata: &[f64]) -> Vec<f64> {
        let mut out = vec![metadata.len() as f64];
        out.extend(metadata);
        out.extend([1.0, bytes.len() as f64]);
        out.extend(bytes.iter().map(|n| *n as f64));
        out
    }
    let mut session = Session::default();
    for bits in [
        0x7ff8000000000000u64,
        0x7ff8000000000001,
        0x7ff8123456789abc,
        0xfff8000000001234,
        0x7ff0000000000001,
        0xfff000000000abcd,
    ] {
        let mut full_bytes = vec![0u8; 726];
        full_bytes[128..136].copy_from_slice(&bits.to_le_bytes());
        let full = decode_flight(Some(&full_bytes), 2.0, true, [true; 13], true).unwrap();
        assert_eq!(full.numbers[16].to_bits(), bits);
        assert_eq!(full.words()[16].to_bits(), bits);
        let mut full_metadata = vec![2.0, 1.0];
        full_metadata.extend([1.0; 14]);
        session.input = command(&full_bytes, &full_metadata);
        session.run(6).unwrap();
        assert_eq!(session.output[16].to_bits(), bits);
        let saved_output = session.output.clone();

        // Flying without a trick has one unit quaternion and no travel fields.
        let mut compact_bytes = vec![0u8; 382];
        compact_bytes[0] = 2;
        compact_bytes[110..118].copy_from_slice(&bits.to_le_bytes());
        compact_bytes[310..318].copy_from_slice(&1.0f64.to_le_bytes());
        let compact =
            decode_compact_flight(Some(&compact_bytes), true, [true; 9], [true; 4]).unwrap();
        assert_eq!(compact.numbers[13].to_bits(), bits);
        assert_eq!(compact.words()[19].to_bits(), bits);
        session.input = command(&compact_bytes, &[1.0; 14]);
        session.run(5).unwrap();
        assert_eq!(session.output[19].to_bits(), bits);

        full_bytes.fill(255);
        compact_bytes.fill(255);
        session.input.clear();
        assert!(session.run(0).is_err());
        assert!(session.output.is_empty());
        assert_eq!(saved_output[16].to_bits(), bits);
        assert_eq!(full.numbers[16].to_bits(), bits);
        assert_eq!(compact.numbers[13].to_bits(), bits);
        assert_eq!(full.words()[16].to_bits(), bits);
        assert_eq!(compact.words()[19].to_bits(), bits);
    }
}

#[test]
fn correction_readers_match_js_validation_and_wasm_words() {
    let content = match std::env::var("BFE_CORRECTION_CORPUS_KIND").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("synthetic") => false,
        Ok("content") => true,
        _ => panic!("invalid correction corpus kind"),
    };
    assert!(
        !content || std::env::var_os("BFE_CORRECTION_PREDICTION_CORPUS").is_some(),
        "content mode requires an explicit captured corpus"
    );
    let mut child = None;
    let corpus: Box<dyn BufRead> =
        if let Some(path) = std::env::var_os("BFE_CORRECTION_PREDICTION_CORPUS") {
            Box::new(BufReader::new(
                std::fs::File::open(path).expect("read correction corpus"),
            ))
        } else {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let mut process =
                Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
                    .arg(root.join("tools/prediction-corrections-twin.mjs"))
                    .current_dir(root)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .expect("run correction oracle");
            let stdout = process.stdout.take().unwrap();
            child = Some(process);
            Box::new(BufReader::new(stdout))
        };
    let mut session = Session::default();
    // Restoring a body can also emit nested arsenal operations.
    let mut counts = BTreeMap::<u32, usize>::new();
    let mut failures = 0;
    for (line, source) in corpus.lines().enumerate() {
        let event = json::parse(&source.unwrap()).unwrap();
        let label = get(&event, "label").as_str().unwrap();
        let op = get(&event, "op").as_f64().unwrap() as u32;
        session.input = words(get(&event, "input"));
        let result = session.run(op);
        if let Some(error) = get(&event, "error").as_str() {
            assert_eq!(result, Err(error), "{label} operation {line}");
            assert!(
                session.output.is_empty(),
                "error must clear preceding successful output"
            );
            failures += 1;
        } else {
            assert_eq!(result, Ok(()), "{label} operation {line}");
        }
        let expected = words(get(&event, "output"));
        assert_eq!(
            session.output.len(),
            expected.len(),
            "{label} output length"
        );
        for (word, (actual, expected)) in session.output.iter().zip(expected).enumerate() {
            assert!(
                jsrt::same_value(*actual, expected),
                "{label} operation {line} word {word}: {actual:?} != {expected:?}"
            );
        }
        *counts.entry(op).or_default() += 1;
    }
    if let Some(child) = child {
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    if content {
        assert!(
            counts.get(&4).copied().unwrap_or(0) >= 30
                && counts.get(&5).copied().unwrap_or(0) >= 30
                && counts.get(&6).copied().unwrap_or(0) >= 30
                && counts.values().sum::<usize>() >= 300
        );
        assert_eq!(
            failures, 0,
            "authenticated content corrections must all restore"
        );
    } else {
        assert!(
            (0..8).all(|op| counts.get(&op).is_some_and(|n| *n > 0))
                && counts.values().sum::<usize>() > 2500
                && failures > 1000
        );
    }
    eprintln!("correction reads: operations {counts:?}, {failures} exact errors");
}
