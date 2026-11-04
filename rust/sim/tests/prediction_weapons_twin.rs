//! Run the owned prediction corpus checked against JavaScript through the
//! native session too, retaining float bits and operation/session order.
use bfe_content::{json, Value};
use bfe_sim::prediction::weapons::Session;
use std::{collections::HashMap, path::Path, process::Command};

fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or_else(|| panic!("missing {key}"))
}
fn text(v: &Value) -> &str {
    v.as_str().unwrap()
}
fn words(v: &Value) -> Vec<f64> {
    let hex = text(v);
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
fn private_weapon_prediction_matches_the_js_and_wasm_corpus() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let node = std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into());
    let oracle = Command::new(node)
        .arg(root.join("tools/prediction-weapons-twin.mjs"))
        .current_dir(&root)
        .output()
        .expect("run prediction oracle");
    assert!(
        oracle.status.success(),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    let mut sessions: HashMap<String, Session> = HashMap::new();
    let mut counts = [0usize; 4];
    for (line, source) in std::str::from_utf8(&oracle.stdout)
        .unwrap()
        .lines()
        .enumerate()
    {
        let event = json::parse(source).unwrap();
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
    }
    assert!(counts[0] >= 12);
    assert!(counts[1] >= 20);
    assert!(counts[2] >= 850);
    assert!(counts[3] >= 1000);
    eprintln!(
        "prediction weapons: {} sessions, operations {counts:?}",
        sessions.len()
    );
}
