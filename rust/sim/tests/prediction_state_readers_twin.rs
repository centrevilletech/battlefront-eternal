//! Replay the parser phases already checked through live JavaScript helpers.
use bfe_content::{json, Value};
use bfe_sim::prediction::corrections::Session;
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
fn arsenal_and_recon_phases_match_js_callbacks_and_native_words() {
    let content = match std::env::var("BFE_STATE_READERS_CORPUS_KIND").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("synthetic") => false,
        Ok("content") => true,
        _ => panic!("invalid state-reader corpus kind"),
    };
    assert!(
        !content || std::env::var_os("BFE_STATE_READERS_CORPUS").is_some(),
        "content mode requires an explicit captured corpus"
    );
    let mut child = None;
    let corpus: Box<dyn BufRead> = if let Some(path) = std::env::var_os("BFE_STATE_READERS_CORPUS")
    {
        Box::new(BufReader::new(
            std::fs::File::open(path).expect("read state-reader corpus"),
        ))
    } else {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut process =
            Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
                .arg(root.join("tools/prediction-state-readers-twin.mjs"))
                .current_dir(root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("run state-reader oracle");
        let stdout = process.stdout.take().unwrap();
        child = Some(process);
        Box::new(BufReader::new(stdout))
    };
    let mut session = Session::default();
    let mut counts = [0usize; 19];
    let mut failures = 0;
    let mut errors = HashSet::new();
    let mut labels = HashSet::new();
    for (line, source) in corpus.lines().enumerate() {
        let event = json::parse(&source.unwrap()).unwrap();
        let label = get(&event, "label").as_str().unwrap();
        let op = get(&event, "op").as_f64().unwrap() as u32;
        assert!((op as usize) < counts.len());
        session.input = words(get(&event, "input"));
        let result = session.run(op);
        if let Some(error) = get(&event, "error").as_str() {
            assert_eq!(result, Err(error), "{label} operation {line}");
            failures += 1;
            errors.insert(error.to_owned());
        } else {
            assert_eq!(result, Ok(()), "{label} operation {line}");
        }
        // Expansion failures can deliberately retain partially copied bytes.
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
        assert!(operations > 100 && counts[9..].iter().sum::<usize>() > 50);
        assert_eq!(failures, 0, "authenticated state readers must all validate");
    } else {
        assert!(
            operations > 2500
                && counts[9..].iter().all(|n| *n > 0)
                && failures > 100
                && errors.len() >= 8
        );
        assert!(
            labels.iter().any(|s| s.starts_with("arsenal:"))
                && labels.iter().any(|s| s.starts_with("recon:"))
        );
    }
    eprintln!("state-reader phases: operations {counts:?}, {failures} exact errors");
}
