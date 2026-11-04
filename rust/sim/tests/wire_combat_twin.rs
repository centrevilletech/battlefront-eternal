//! The native combat writer consumes the same synthetic ABI records as
//! the live adapter and compares every byte against JavaScript producers.
use bfe_content::{json, Value};
use bfe_sim::{gen::WbEncode, wire::combat};
use std::{path::Path, process::Command};

fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap()
}
fn text(v: &Value) -> &str {
    v.as_str().unwrap()
}
fn bytes(v: &Value) -> Vec<u8> {
    let text = text(v);
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}
fn words(v: &Value) -> Vec<f64> {
    bytes(v)
        .chunks_exact(8)
        .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
        .collect()
}

#[test]
fn native_combat_payloads_match_javascript_bytes_and_failures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let node = std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into());
    let oracle = Command::new(node)
        .arg(root.join("tools/wire-combat-twin.mjs"))
        .current_dir(&root)
        .output()
        .expect("run combat oracle");
    assert!(
        oracle.status.success(),
        "{}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    let mut count = 0;
    let mut failures = 0;
    for line in std::str::from_utf8(&oracle.stdout).unwrap().lines() {
        let value = json::parse(line).unwrap();
        let input = WbEncode::take(&words(get(&value, "row")), &words(get(&value, "heap")));
        let actual = combat::encode(&input);
        if let Some(error) = get(&value, "error").as_str() {
            assert_eq!(actual, Err(error), "{}", text(get(&value, "label")));
            failures += 1;
        } else {
            assert_eq!(
                actual.unwrap(),
                bytes(get(&value, "bytes")),
                "{}",
                text(get(&value, "label"))
            );
        }
        count += 1;
    }
    assert!(count >= 850);
    assert!(failures >= 100);
    eprintln!("wire combat: {count} native byte/validation cases, {failures} expected failures");
}
