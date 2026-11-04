#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
use bfe_content::{json, Value};
use bfe_sim::abi::context::{Context, Error, Failure, Record};
use bfe_sim::gen::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::rc::Rc;

fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or_else(|| panic!("missing {key}"))
}
fn words(v: &Value) -> Vec<f64> {
    let hex = v.as_str().unwrap();
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
fn record(v: &Value) -> Record {
    Record {
        row: words(v),
        heap: vec![],
    }
}
fn bits(values: &[f64]) -> Vec<u64> {
    values.iter().map(|v| v.to_bits()).collect()
}
fn same(actual: &[f64], expected: &Value) {
    assert_eq!(bits(actual), bits(&words(expected)));
}

#[test]
fn native_context_clock_and_relay_records_match_two_wasm_instances_and_js() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut child = Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
        .arg(root.join("tools/native-context-twin.mjs"))
        .current_dir(root)
        .stdout(Stdio::piped())
        .spawn()
        .expect("run native context oracle");
    let source = BufReader::new(child.stdout.take().unwrap());
    let contexts = [Context::new(), Context::new()];
    let queues: [Rc<RefCell<VecDeque<Value>>>; 2] =
        std::array::from_fn(|_| Rc::new(RefCell::new(VecDeque::new())));
    let errors = [Error::host("tick failed"), Error::host("")];
    for (cx, queue) in contexts.iter().zip(&queues) {
        for code in [IMPORT_ASK, IMPORT_WORLD_CLOCK] {
            let queue = queue.clone();
            let errors = errors.clone();
            cx.register(code, move |_, args| {
                let expected = queue.borrow_mut().pop_front().expect("unexpected callback");
                assert_eq!(get(&expected, "code").as_f64().unwrap() as u32, code);
                same(&args.row, get(&expected, "args"));
                assert!(args.heap.is_empty());
                if let Some(error) = expected.get("error").and_then(Value::as_str) {
                    let error = errors
                        .iter()
                        .find(|e| e.message() == error)
                        .unwrap()
                        .clone();
                    Err(Failure {
                        error,
                        partial: Some(record(get(&expected, "partial"))),
                    })
                } else {
                    Ok(record(get(&expected, "reply")))
                }
            })
            .unwrap();
        }
    }
    let (mut clocks, mut commands, mut failed, mut init) = (0, 0, 0, 0);
    for (line, text) in source.lines().enumerate() {
        let event = json::parse(&text.unwrap()).unwrap();
        let index = get(&event, "cx").as_f64().unwrap() as usize;
        let cx = &contexts[index];
        let queue = &queues[index];
        if get(&event, "kind").as_str() == Some("init") {
            for s in get(&event, "strings").as_array().unwrap() {
                let units: Vec<u16> = s
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap() as u16)
                    .collect();
                cx.intern(&units).unwrap();
            }
            init += 1;
            continue;
        }
        assert!(
            queue.borrow().is_empty(),
            "unconsumed callbacks before line {line}"
        );
        queue
            .borrow_mut()
            .extend(get(&event, "calls").as_array().unwrap().iter().cloned());
        let input = record(get(&event, "input"));
        match get(&event, "kind").as_str().unwrap() {
            "clock" => {
                let result = cx.clock(input.read(WorldClockInput::take));
                let state = match result {
                    Ok(out) => {
                        same(&cx.calls().unwrap().row, get(&event, "output"));
                        out.state
                    }
                    Err(failure) => {
                        failed += 1;
                        let message = get(&event, "error").as_str().unwrap();
                        assert!(errors
                            .iter()
                            .any(|e| e.message() == message && e.same(&failure.error)));
                        failure.state
                    }
                };
                same(
                    &Record::new(WorldClockState::WORDS, |r, h| state.put(r, h)).row,
                    get(&event, "state"),
                );
                clocks += 1;
            }
            "command" => {
                let code = get(&event, "code").as_f64().unwrap() as u32;
                let out = cx.command(code, input).unwrap();
                same(&out.row, get(&event, "output"));
                same(&cx.events().unwrap(), get(&event, "events"));
                cx.clear_events().unwrap();
                commands += 1;
            }
            other => panic!("unexpected event {other}"),
        }
        assert!(
            queue.borrow().is_empty(),
            "missing callbacks at line {line}"
        );
    }
    assert!(child.wait().unwrap().success());
    assert_eq!((init, clocks, commands, failed), (2, 1460, 144, 4));
    for cx in contexts {
        cx.dispose().unwrap();
    }
}
