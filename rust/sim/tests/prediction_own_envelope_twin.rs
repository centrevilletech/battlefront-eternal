//! Own-player envelopes validated against the original JavaScript reader.
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
fn own_envelope_matches_js_structure_errors_and_wasm_values() {
    let content = match std::env::var("BFE_OWN_ENVELOPE_CORPUS_KIND").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("synthetic") => false,
        Ok("content") => true,
        _ => panic!("invalid own-envelope corpus kind"),
    };
    assert!(
        !content || std::env::var_os("BFE_OWN_ENVELOPE_CORPUS").is_some(),
        "content mode requires an explicit captured corpus"
    );
    let mut child = None;
    let corpus: Box<dyn BufRead> = if let Some(path) = std::env::var_os("BFE_OWN_ENVELOPE_CORPUS") {
        Box::new(BufReader::new(
            std::fs::File::open(path).expect("read own-envelope corpus"),
        ))
    } else {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut process =
            Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
                .arg(root.join("tools/prediction-own-envelope-twin.mjs"))
                .current_dir(root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("run own-envelope oracle");
        let stdout = process.stdout.take().unwrap();
        child = Some(process);
        Box::new(BufReader::new(stdout))
    };
    let mut session = Session::default();
    let mut operations = 0;
    let mut errors = 0;
    let mut successes = 0;
    let mut valid_kinds = HashSet::new();
    let mut error_messages = HashSet::new();
    let mut view_cases = 0;
    for (line, source) in corpus.lines().enumerate() {
        let event = json::parse(&source.unwrap()).unwrap();
        let label = get(&event, "label").as_str().unwrap();
        assert_eq!(get(&event, "op").as_f64(), Some(8.0));
        session.input = words(get(&event, "input"));
        let result = session.run(8);
        if let Some(error) = get(&event, "error").as_str() {
            assert_eq!(result, Err(error), "{label} operation {line}");
            assert!(
                session.output.is_empty(),
                "failed parse retained previous output"
            );
            errors += 1;
            error_messages.insert(error.to_owned());
        } else {
            assert_eq!(result, Ok(()), "{label} operation {line}");
            successes += 1;
            if let Some(kind) = label.strip_suffix(":valid") {
                valid_kinds.insert(kind.to_owned());
            }
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
        if label.starts_with("view-flags:") {
            view_cases += 1;
        }
        operations += 1;
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
        assert!(successes > 100);
        assert_eq!(errors, 0, "live own envelopes must all decode");
    } else {
        assert!(
            operations > 5000
                && successes > 1000
                && errors > 3000
                && view_cases >= 2048
                && error_messages.len() >= 10
        );
        for kind in [
            "infantry",
            "droideka",
            "ground",
            "flight",
            "passenger",
            "turret",
            "recon",
        ] {
            assert!(valid_kinds.contains(kind), "missing {kind} valid envelope");
        }
    }
    eprintln!("own envelopes: {operations} operations, {successes} valid, {errors} exact errors");
}
