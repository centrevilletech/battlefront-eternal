//! Replay numeric exchanges captured after live helper and callback parity.
use bfe_content::{json, Value};
use bfe_sim::prediction::reconcile::Session;
use std::{
    collections::HashSet,
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
fn timing_and_window_exchanges_match_live_js_observations() {
    let content = match std::env::var("BFE_RECONCILE_CORPUS_KIND").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("synthetic") => false,
        Ok("content") => true,
        _ => panic!("invalid reconciliation corpus kind"),
    };
    assert!(
        !content || std::env::var_os("BFE_RECONCILE_CORPUS").is_some(),
        "content mode requires an explicit captured corpus"
    );
    let mut child = None;
    let corpus: Box<dyn BufRead> = if let Some(path) = std::env::var_os("BFE_RECONCILE_CORPUS") {
        Box::new(BufReader::new(
            std::fs::File::open(path).expect("read reconciliation corpus"),
        ))
    } else {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut process =
            Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
                .arg(root.join("tools/prediction-reconcile-twin.mjs"))
                .current_dir(root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("run reconciliation oracle");
        let stdout = process.stdout.take().unwrap();
        child = Some(process);
        Box::new(BufReader::new(stdout))
    };
    let mut session = Session::default();
    let mut counts = [0usize; 16];
    let mut labels = HashSet::new();
    for (line, source) in corpus.lines().enumerate() {
        let event = json::parse(&source.unwrap()).unwrap();
        let label = get(&event, "label").as_str().unwrap();
        let op = get(&event, "op").as_f64().unwrap() as u32;
        assert!((op as usize) < counts.len());
        session.input = words(get(&event, "input"));
        assert_eq!(get(&event, "error").as_str(), None, "{label} ABI failure");
        assert_eq!(session.run(op), Ok(()), "{label} operation {line}");
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
        labels.insert(label.to_owned());
    }
    if let Some(child) = child {
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let operations = counts.iter().sum::<usize>();
    if content {
        assert!(operations > 100 && counts[8] > 10);
        assert!(counts[1] > 10 && counts[10] + counts[11] > 10);
    } else {
        assert!(operations > 5000 && counts.iter().all(|n| *n > 0));
        assert!(labels.len() >= 50);
        for prefix in ["estimator:", "lead:", "smoother:", "window:", "retain:"] {
            assert!(labels.iter().any(|s| s.starts_with(prefix)), "{prefix}");
        }
    }
    eprintln!("reconciliation: operations {counts:?}, {operations} total");
}

#[test]
fn owned_results_survive_reuse_and_invalid_exchange_clears_output() {
    let mut session = Session::default();
    session.input = vec![10.0, 4.0, 9.0, 10.0, 11.0, 12.0];
    assert_eq!(session.run(14), Ok(()));
    let kept = session.output.clone();
    assert_eq!(kept, vec![2.0, 2.0, 3.0]);
    session.input = vec![0.0, -0.0, 0.0];
    assert_eq!(session.run(12), Ok(()));
    assert_eq!(session.output[0].to_bits(), 0.0f64.to_bits());
    for input in [vec![], vec![1.0, 1.0], vec![1.0, f64::INFINITY]] {
        session.input = input;
        assert_eq!(session.run(14), Err("invalid reconciliation exchange"));
        assert!(session.output.is_empty());
    }
    session.input = vec![1.0, 1.0, f64::NAN];
    assert_eq!(session.run(7), Err("invalid reconciliation exchange"));
    assert!(session.output.is_empty());
    session.input.clear();
    assert_eq!(session.run(0), Ok(()));
    assert_eq!(kept, vec![2.0, 2.0, 3.0]);
}
