//! Replay the same owned recon continuations checked against JavaScript.
//! Foreign queries and callback mutations arrive as captured resume commands.
use bfe_content::{json, Value};
use bfe_sim::prediction::remotes::Session;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
};

fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}"))
}
fn text(value: &Value) -> &str {
    value.as_str().unwrap()
}
fn words(value: &Value) -> Vec<f64> {
    let hex = text(value);
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
fn private_recon_replay_matches_js_and_wasm_continuations() {
    let mut child = None;
    let corpus: Box<dyn BufRead> =
        if let Some(path) = std::env::var_os("BFE_REMOTE_PREDICTION_CORPUS") {
            Box::new(BufReader::new(
                std::fs::File::open(path).expect("read recon prediction corpus"),
            ))
        } else {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let mut process =
                Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
                    .arg(root.join("tools/prediction-remotes-twin.mjs"))
                    .current_dir(&root)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .expect("run recon oracle");
            let stdout = process.stdout.take().unwrap();
            child = Some(process);
            Box::new(BufReader::new(stdout))
        };
    let mut sessions: HashMap<String, Session> = HashMap::new();
    let mut counts = [0usize; 4];
    let mut yields = 0;
    for (line, source) in corpus.lines().enumerate() {
        let event = json::parse(&source.unwrap()).unwrap();
        let session = sessions
            .entry(text(get(&event, "session")).into())
            .or_default();
        let op = get(&event, "op").as_f64().unwrap() as u32;
        session.input = words(get(&event, "input"));
        let result = session.run(op);
        let label = text(get(&event, "label"));
        if let Some(error) = get(&event, "error").as_str() {
            assert_eq!(result, Err(error), "{label} operation {line}");
        } else {
            assert_eq!(result, Ok(()), "{label} operation {line}");
        }
        let expected = words(get(&event, "output"));
        assert_eq!(
            session.output.len(),
            expected.len(),
            "{label} operation {line} output length"
        );
        for (word, (actual, expected)) in session.output.iter().zip(expected).enumerate() {
            assert!(
                jsrt::same_value(*actual, expected),
                "{label} operation {line} word {word}: {actual:?} != {expected:?}"
            );
        }
        counts[op as usize] += 1;
        if matches!(op, 2 | 3) && session.output.first() == Some(&1.0) {
            yields += 1;
        }
    }
    if let Some(child) = child {
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert!(
        sessions.len() >= 10
            && counts[0] >= 10
            && counts[1] >= 10
            && counts[2] >= 1000
            && yields >= 5000
    );
    eprintln!(
        "recon prediction: {} sessions, operations {counts:?}, {yields} yields",
        sessions.len()
    );
}
