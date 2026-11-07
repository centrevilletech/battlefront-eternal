#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
use bfe_content::{json, Value};
use bfe_sim::abi::context::{Context, Record};
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
        row: words(get(v, "row")),
        heap: words(get(v, "heap")),
    }
}
fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|n| n.to_bits()).collect()
}
fn same(actual: &Record, expected: &Value) {
    let expected = record(expected);
    assert_eq!(bits(&actual.row), bits(&expected.row));
    assert_eq!(bits(&actual.heap), bits(&expected.heap));
}
struct Lane {
    context: Context,
    calls: Rc<RefCell<VecDeque<Value>>>,
}
impl Lane {
    fn new() -> Self {
        let context = Context::new();
        let calls: Rc<RefCell<VecDeque<Value>>> = Rc::default();
        for code in [
            IMPORT_WORLD_EVENTS,
            IMPORT_WORLD_LOCAL,
            IMPORT_WORLD_NETWORK,
        ] {
            let calls = calls.clone();
            context
                .register(code, move |_, args| {
                    let call = calls.borrow_mut().pop_front().expect("unexpected callback");
                    assert_eq!(get(&call, "code").as_f64().unwrap() as u32, code);
                    same(args, get(&call, "input"));
                    Ok(record(get(&call, "output")))
                })
                .unwrap();
        }
        Self { context, calls }
    }
}
#[test]
fn native_world_consumers_match_existing_port_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut child = Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
        .arg(root.join("tools/native-consumers-twin.mjs"))
        .current_dir(root)
        .stdout(Stdio::piped())
        .spawn()
        .expect("run consumer fixtures");
    let reader = BufReader::new(child.stdout.take().unwrap());
    let mut lanes: Vec<Lane> = Vec::new();
    let mut count = 0;
    let mut summary = false;
    for text in reader.lines() {
        let event = json::parse(&text.unwrap())
            .unwrap_or_else(|error| panic!("consumer record {count}: {error:?}"));
        if event.get("summary").is_some() {
            summary = true;
            continue;
        }
        let cx = get(&event, "cx").as_f64().unwrap() as usize;
        while lanes.len() <= cx {
            lanes.push(Lane::new());
        }
        let Lane { context, calls } = &lanes[cx];
        for (i, units) in get(&event, "strings")
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let units: Vec<u16> = units
                .as_array()
                .unwrap()
                .iter()
                .map(|u| u.as_f64().unwrap() as u16)
                .collect();
            assert_eq!(context.intern(&units).unwrap().0 as usize, i);
        }
        assert!(calls.borrow().is_empty());
        calls
            .borrow_mut()
            .extend(get(&event, "calls").as_array().unwrap().iter().cloned());
        let input = record(get(&event, "input"));
        let scalar = match get(&event, "name").as_str().unwrap() {
            "world_events_run" => {
                context
                    .world_events(input.read(WorldEventInput::take))
                    .unwrap();
                None
            }
            "world_local_run" => {
                context
                    .world_local(input.read(WorldLocalInput::take))
                    .unwrap();
                None
            }
            "world_network_run" => Some(
                context
                    .world_network(input.read(WorldNetworkInput::take))
                    .unwrap()
                    .tick,
            ),
            name => panic!("unexpected consumer {name}"),
        };
        if let Some(scalar) = scalar {
            assert_eq!(bits(&[scalar]), bits(&words(get(&event, "scalar"))));
        }
        if let Some(output) = event.get("output").or_else(|| event.get("state")) {
            same(&context.calls().unwrap(), output);
        }
        assert!(calls.borrow().is_empty());
        count += 1;
    }
    assert!(child.wait().unwrap().success());
    assert!(summary);
    assert!(count > 0, "only {count} consumer records");
    assert!(lanes.len() > 2);
    for lane in lanes {
        lane.context.dispose().unwrap();
    }
}
