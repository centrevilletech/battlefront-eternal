//! Exact correction reads replayed from the same byte payloads checked in JS.
use bfe_content::{json, Value};
use bfe_sim::prediction::corrections::Session;
use std::{
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
    let mut counts = [0usize; 8];
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
        counts[op as usize] += 1;
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
            counts[4] >= 30
                && counts[5] >= 30
                && counts[6] >= 30
                && counts.iter().sum::<usize>() >= 300
        );
        assert_eq!(
            failures, 0,
            "authenticated content corrections must all restore"
        );
    } else {
        assert!(
            counts.iter().all(|n| *n > 0) && counts.iter().sum::<usize>() > 2500 && failures > 1000
        );
    }
    eprintln!("correction reads: operations {counts:?}, {failures} exact errors");
}
