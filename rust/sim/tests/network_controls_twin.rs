#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
use bfe_content::{json, Value};
use bfe_sim::gen::{WorldControlsArgs, WorldControlsCall, WorldControlsReply};
use bfe_sim::world::network_controls::{self, Host};
use std::collections::VecDeque;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::path::Path;
use std::process::Command;

fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}"))
}
fn words(value: &Value) -> Vec<f64> {
    let text = value.as_str().expect("encoded words");
    assert_eq!(text.len() % 16, 0);
    (0..text.len()).step_by(16).map(|start| {
        let bytes = std::array::from_fn(|i| u8::from_str_radix(&text[start + i * 2..start + i * 2 + 2], 16).unwrap());
        f64::from_le_bytes(bytes)
    }).collect()
}
fn bits(values: &[f64]) -> Vec<u64> { values.iter().map(|v| v.to_bits()).collect() }
fn boolean(value: &Value) -> bool {
    match value { Value::Bool(flag) => *flag, _ => panic!("expected Boolean") }
}
struct RecordedFailure;
struct Replay { calls: VecDeque<Value>, count: usize }
impl Host for Replay {
    fn call(&mut self, op: WorldControlsCall, index: f64, value: f64, other: f64) -> WorldControlsReply {
        let expected = self.calls.pop_front().expect("unexpected native control callback");
        let mut row = [0.0; WorldControlsArgs::WORDS];
        WorldControlsArgs { op, index, value, other }.put(&mut row, &mut vec![]);
        assert_eq!(bits(&row), bits(&words(get(&expected, "args"))), "callback {} {op:?}", self.count);
        self.count += 1;
        if expected.get("threw").map(boolean).unwrap_or(false) { resume_unwind(Box::new(RecordedFailure)); }
        WorldControlsReply::take(&words(get(&expected, "reply")), &[])
    }
}

#[test]
fn native_network_controls_match_source_verified_callback_records() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let name = format!("bfe-network-controls-{}-{}.jsonl", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    let corpus = std::env::temp_dir().join(name);
    let output = Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
        .args(["--test", "--test-reporter=dot", "tests/world-network-controls-sim.test.js"])
        .env("BFE_NETWORK_CONTROLS_CORPUS", &corpus).current_dir(root).output().expect("run controls oracle");
    assert!(output.status.success(), "controls oracle failed:\n{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    let text = std::fs::read_to_string(&corpus).expect("controls corpus");
    std::fs::remove_file(corpus.with_extension("jsonl.source.json")).unwrap();
    std::fs::remove_file(corpus).unwrap();
    let (mut cases, mut callbacks, mut stops, mut failed) = (0, 0, 0, 0);
    for (index, line) in text.lines().enumerate() {
        let record = json::parse(line).unwrap();
        let stop = boolean(get(&record, "stop"));
        let mut replay = Replay { calls: get(&record, "calls").as_array().unwrap().iter().cloned().collect(), count: 0 };
        let result = catch_unwind(AssertUnwindSafe(|| {
            if stop { network_controls::stop(&mut replay); }
            else { network_controls::apply(words(get(&record, "tick"))[0], &mut replay); }
        }));
        let threw = match result {
            Ok(()) => false,
            Err(error) if error.is::<RecordedFailure>() => true,
            Err(error) => resume_unwind(error),
        };
        assert_eq!(threw, boolean(get(&record, "threw")), "case {index}");
        assert!(replay.calls.is_empty(), "missing callbacks in case {index}");
        cases += 1; callbacks += replay.count;
        if stop { stops += 1; }
        if threw { failed += 1; }
    }
    assert!(cases >= 100 && callbacks >= 5000 && stops >= 4 && failed >= 10,
        "incomplete corpus: {cases} cases, {callbacks} callbacks, {stops} stops, {failed} failures");
    println!("native controls: {cases} cases, {callbacks} callbacks, {stops} stops, {failed} failures exact");
}
