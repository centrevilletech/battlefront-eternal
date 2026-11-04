//! Native records and rings against the unchanged JavaScript oracle.
//! Set BFE_NODE to the production server's Node executable when running
//! this gate; the oracle uses synthetic records and requires no content.
use bfe_content::{json, Value};
use bfe_sim::gen::{WireCapture, WireSnapshot};
use bfe_sim::store::Strings;
use bfe_sim::wire::{codec, records, ring::SnapshotRing};
use std::{path::Path, process::Command};

fn number(v: &Value) -> f64 { v.as_f64().expect("oracle number") }
fn get<'a>(v: &'a Value, name: &str) -> &'a Value { v.get(name).unwrap_or_else(|| panic!("oracle {name}")) }
fn text(v: &Value) -> &str { v.as_str().expect("oracle string") }
fn array(v: &Value) -> &[Value] { v.as_array().expect("oracle array") }
fn flag(v: &Value) -> bool { matches!(v, Value::Bool(true)) }
fn bytes(v: &Value) -> Vec<u8> {
    let s = text(v);
    assert_eq!(s.len() % 2, 0);
    (0..s.len()).step_by(2).map(|at| u8::from_str_radix(&s[at..at + 2], 16).unwrap()).collect()
}
fn words(v: &Value) -> Vec<f64> {
    bytes(v).chunks_exact(8).map(|b| f64::from_le_bytes(b.try_into().unwrap())).collect()
}
fn integers(v: &Value) -> Vec<i64> { array(v).iter().map(|v| number(v) as i64).collect() }

#[test]
fn native_records_quantizers_and_rings_match_javascript_bytes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let node = std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into());
    let oracle = Command::new(node).arg(root.join("tools/wire-records-twin.mjs")).current_dir(&root)
        .output().expect("run the Node record oracle");
    assert!(oracle.status.success(), "oracle failed: {}", String::from_utf8_lossy(&oracle.stderr));
    let trace = std::str::from_utf8(&oracle.stdout).unwrap();
    let mut strings = Strings::default();
    let mut slots = records::SnapshotSlotTable::default();
    let mut ring = SnapshotRing::new(7).unwrap();
    let mut snapshots: Vec<WireSnapshot> = Vec::new();
    let mut counts = [0usize; 3];
    for (line, source) in trace.lines().enumerate() {
        let op = json::parse(source).unwrap();
        match text(get(&op, "kind")) {
            "quantize" => {
                counts[0] += 1;
                let input = words(get(&op, "input"));
                let p = [input[0], input[1], input[2]];
                let v = records::quantize_velocity([input[3], input[4], input[5]], flag(get(&op, "wide")));
                let q = records::quantize_quaternion([input[6], input[7], input[8], input[9]]);
                let mut actual = records::quantize_position(p, &records::default_bounds()).to_vec();
                actual.push(if v.wide { 1.0 } else { 0.0 }); actual.extend(v.v);
                actual.extend([records::quantize_yaw(p[0]), records::quantize_pitch(p[1]), q.index]); actual.extend(q.v);
                let expected = words(get(&op, "expected"));
                assert_eq!(actual.len(), expected.len());
                for (i, (a, b)) in actual.into_iter().zip(expected).enumerate() {
                    assert!(jsrt::same_value(a, b), "line {line}, number {i}: {a:?} != {b:?}");
                }
            },
            "ring" => {
                counts[1] += 1;
                let mut evicted = Vec::new();
                if flag(get(&op, "clear")) { ring.clear(); }
                else {
                    ring.set_with_eviction(number(get(&op, "tick")) as i64, number(get(&op, "epoch")) as u32,
                        number(get(&op, "value")) as i64, |value, held| {
                            evicted.push((value, held.ticks().to_vec(), held.newest_tick()));
                        }).unwrap();
                }
                assert_eq!(ring.ticks(), integers(get(&op, "ticks")), "line {line}: ring ticks");
                assert_eq!(ring.newest_tick(), number(get(&op, "newest")) as i64, "line {line}: newest");
                assert_eq!(ring.oldest_tick(), number(get(&op, "oldest")) as i64, "line {line}: oldest");
                let expected = array(get(&op, "evicted"));
                assert_eq!(evicted.len(), expected.len());
                for (a, b) in evicted.iter().zip(expected) {
                    assert_eq!(a.0, number(get(b, "value")) as i64);
                    assert_eq!(a.1, integers(get(b, "ticks")));
                    assert_eq!(a.2, number(get(b, "newest")) as i64);
                }
                for row in array(get(&op, "values")) {
                    let row = array(row); let tick = number(&row[0]) as i64; let epoch = number(&row[1]) as u32;
                    assert_eq!(ring.get(tick, Some(epoch)), Some(&(number(&row[2]) as i64)));
                    assert_eq!(ring.get(tick, Some(epoch + 7)), None);
                }
            },
            "capture" => {
                counts[2] += 1;
                for name in array(get(&op, "strings")) { strings.intern_str(text(name)); }
                let capture = WireCapture::take(&words(get(&op, "row")), &words(get(&op, "heap")));
                let snapshot = records::snapshot_from_records(capture.tick, capture.epoch, &capture.records,
                    capture.world, &mut slots, &capture.bounds, snapshots.last(), &strings).unwrap();
                let baseline_tick = number(get(&op, "baseline"));
                let baseline = snapshots.iter().find(|s| s.tick == baseline_tick);
                let parts = codec::encode_entities(&snapshot, baseline, number(get(&op, "interval"))).unwrap();
                let expected: Vec<Vec<u8>> = array(get(&op, "parts")).iter().map(bytes).collect();
                assert_eq!(parts, expected, "capture {} tick {} published bytes", counts[2], snapshot.tick);
                assert_eq!(snapshot.slots.iter().map(|s| s.slot as i64).collect::<Vec<_>>(), integers(get(&op, "order")), "capture slot insertion order");
                snapshots.push(snapshot);
            },
            kind => panic!("unknown oracle operation {kind}"),
        }
    }
    assert_eq!(counts, [600, 600, 240]);
    eprintln!("wire records: {} quantizer cases, {} ring operations, {} captures byte-identical", counts[0], counts[1], counts[2]);
}

#[test]
fn slot_reuse_and_exhaustion_preserve_allocation_order() {
    let mut slots = records::SnapshotSlotTable::default();
    assert_eq!(slots.assign_unit(0.0), Ok(0.0));
    assert_eq!(slots.assign_unit(-0.0), Ok(0.0));
    assert_eq!(slots.assign("item:0".into(), None), Ok(1.0));
    for i in 1..511 { assert_eq!(slots.assign_unit(i as f64), Ok((i + 1) as f64)); }
    assert_eq!(slots.assign_unit(900.0), Err("snapshot slot table full"));
    assert_eq!(slots.slot_for_unit(900.0), -1.0);
    slots.release_unit(10.0); slots.release_unit(20.0); slots.release_unit(20.0);
    assert_eq!(slots.assign_unit(900.0), Ok(21.0));
    assert_eq!(slots.assign_unit(901.0), Ok(11.0));
    assert_eq!(slots.assign_unit(902.0), Err("snapshot slot table full"));
    assert_eq!(slots.slot_for("item:0"), 1.0);
}
