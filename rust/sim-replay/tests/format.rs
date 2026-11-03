// The capture format, end to end: tools/sim-capture/fixture.mjs writes a
// made-up capture with every kind of value and record through the same
// writer the captures use, and this reads it back exactly. Needs Node (as
// `node`, or NODE); without it the test says so and passes.
use sim_replay::{check_value, check_vec3, diff, Arena, Capture, Event, Node, Phase, Typed, Value, PHASES};
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture() -> Option<Capture> {
    let out = std::env::temp_dir().join(format!("sim-replay-fixture-{}.bfecap", std::process::id()));
    let node = std::env::var("NODE").unwrap_or_else(|_| "node".into());
    let status = Command::new(&node).arg(root().join("tools/sim-capture/fixture.mjs")).arg(&out).current_dir(root()).status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!("fixture.mjs failed: {s}"),
        Err(e) => {
            eprintln!("no Node to write the fixture with ({node}: {e}): not checked");
            return None;
        }
    }
    let capture = Capture::open_any_v8(&out).expect("the fixture reads");
    std::fs::remove_file(&out).ok();
    Some(capture)
}

fn bits(x: f64) -> u64 {
    x.to_bits()
}

#[test]
fn every_value_comes_back_exactly() {
    let Some(capture) = fixture() else { return };
    assert_eq!(capture.header.root().get("format").num(), 1.0);
    assert_eq!(capture.battle().get("map").str(), Some("test"));
    let def = capture.def("test_ord").expect("the definition");
    assert_eq!(def.get("base").str(), Some("bolt"));
    assert_eq!(def.get("Velocity").num(), 150.0);
    assert_eq!(def.get("LaserLength").num(), 0.5);

    let frames: Vec<_> = capture.frames("test.frame").collect::<Result<_, _>>().unwrap();
    assert_eq!(frames.len(), 1);
    let frame = &frames[0];
    assert_eq!((frame.id, frame.parent, frame.tick), (0, None, 2));
    let a = |i: usize| frame.arg(i);
    let numbers = [0.0, -0.0, 0.1, -1.0, 4503599627370495.0, -4503599627370495.0, 4503599627370496.0, 9007199254740994.0];
    for (i, n) in numbers.iter().enumerate() {
        assert_eq!(bits(a(i).num()), bits(*n), "number {i}");
    }
    assert!(a(8).num().is_nan());
    assert_eq!(a(9).num(), f64::INFINITY);
    assert_eq!(a(10).num(), f64::NEG_INFINITY);
    assert_eq!(bits(a(11).num()), bits(5e-324));
    assert_eq!(a(12).str(), Some("plain"));
    assert_eq!(a(13).str(), Some("déjà"));
    assert!(matches!(a(14).value, Value::Str16(units) if units == &[0xd800]));
    assert_eq!((a(15).bool(), a(16).bool()), (true, false));
    assert!(a(17).is_null() && a(18).is_undefined());
    // One Vector3 met twice is one node, and the list holds that same node.
    assert_eq!(a(19).node_id(), a(20).node_id());
    assert_eq!(a(21).at(1).node_id(), a(19).node_id());
    let v = a(19).vec3().unwrap();
    assert_eq!((v[0], bits(v[1]), v[2]), (1.5, bits(-0.0), -2.25));
    assert!(matches!(a(22).node(), Some(Node::Quat(q)) if q == &[0.5, 0.5, 0.5, 0.5]));
    assert!(matches!(a(23).node(), Some(Node::Mat4(m)) if m[12] == 1.0 && m[13] == 2.0 && m[14] == 3.0 && m[0] == 1.0));
    assert!(matches!(a(24).node(), Some(Node::Euler(e, order)) if e == &[0.1, 0.2, 0.3] && order == "YXZ"));
    assert!(matches!(a(25).node(), Some(Node::Box3(b)) if b == &[-1.0, -2.0, -3.0, 1.0, 2.0, 3.0]));
    assert!(matches!(a(26).typed(), Some(Typed::F32(f)) if f[0] == 0.1f32 && f[1].to_bits() == (-0.0f32).to_bits()));
    assert!(matches!(a(27).typed(), Some(Typed::U16(u)) if u == &[65535]));
    assert!(matches!(a(28).typed(), Some(Typed::I32(i)) if i == &[-5]));
    let entries: Vec<_> = a(29).entries().collect();
    assert_eq!(entries[0].0.str(), Some("k"));
    assert_eq!(entries[0].1.node_id(), a(21).node_id(), "the Map holds the same list");
    assert_eq!((entries[1].0.num(), entries[1].1.str()), (2.0, Some("two")));
    assert_eq!(a(30).at(0).node_id(), a(21).node_id(), "the Set holds the same list");
    assert_eq!(a(30).at(1).num(), 3.0);
    assert_eq!(a(31).handle_num("unit"), Some(3.0));
    assert_eq!(a(32).handle_str("def"), Some("test_ord"));
    assert_eq!(capture.def_of(a(32)).unwrap().get("class").str(), Some("test_ord"));
    assert!(matches!(a(33).value, Value::Omitted(why) if why == "getter"));
    assert_eq!(a(34).function(), Some("named"));
    // Integer-like keys first, ascending, then the rest in order: as JavaScript lists them.
    assert_eq!(a(35).fields().map(|(k, _)| k).collect::<Vec<_>>(), ["2", "10", "z", "a"]);
}

#[test]
fn frames_hold_their_calls_and_draws_in_order() {
    let Some(capture) = fixture() else { return };
    let frame = capture.frames("test.frame").next().unwrap().unwrap();
    let world = frame.unit(3.0).expect("unit 3 in the world record");
    assert_eq!(world.get("generation").num(), 2.0);
    assert_eq!(world.get("classDef").handle_str("def"), Some("rep_inf_test"));
    let kinds: Vec<String> = frame
        .events
        .iter()
        .map(|e| match e {
            Event::Draw(d) => format!("draw {} {}", d.stream, d.value),
            Event::Call(c) => format!("call {} (inside: {})", c.name, c.inner.iter().map(|d| format!("{} {}", d.stream, d.value)).collect::<Vec<_>>().join(", ")),
        })
        .collect();
    assert_eq!(kinds, ["draw A 4294967295", "draw stream:gameplay 12", "call units.raycast (inside: B 7)", "draw Math.random 0.25", "draw seeded:99 0.5"]);
    let call = frame.calls().next().unwrap();
    check_vec3("origin", call.arg(0), [0.0, 1.0, 0.0]).unwrap();
    assert_eq!(call.arg(1).num(), 25.5);
    assert!(call.arg(2).get("unit").is_null(), "the out parameter as it went in");
    assert_eq!(call.result().get("unit").handle_num("unit"), Some(3.0));
    assert_eq!(call.result().get("distance").num(), 12.25);
    assert_eq!(call.out(2).node_id(), call.result().node_id(), "the result is the out parameter");
    assert!(call.extra().get("striking").get("owner").is_null());
    assert_eq!(frame.result().str(), Some("done"));

    // The log serves them in order and says where a port parts from them.
    let mut log = sim_replay::Log::new(&frame);
    assert_eq!(log.draw("A").unwrap(), 4294967295.0);
    assert!(log.call("units.raycast").is_err(), "a draw comes first");
    assert_eq!(log.draw("stream:gameplay").unwrap(), 12.0);
    log.call("units.raycast").unwrap();
    assert!(log.finish().is_err());
    assert_eq!(log.draw("Math.random").unwrap(), 0.25);
    assert_eq!(log.draw("seeded:").unwrap(), 0.5);
    log.finish().unwrap();

    let inner = capture.frames("test.inner").next().unwrap().unwrap();
    assert_eq!((inner.id, inner.parent), (1, Some(0)));
    assert_eq!(inner.after().get("n").num(), 2.0);
    let ticks: Vec<_> = capture.ticks().collect::<Result<_, _>>().unwrap();
    assert_eq!(ticks.len(), 1);
    assert_eq!(ticks[0].root().get("line").str(), Some("2 a b c"));
}

#[test]
fn diff_names_the_first_difference() {
    let Some(capture) = fixture() else { return };
    let frame = capture.frames("test.frame").next().unwrap().unwrap();
    let d = diff(frame.before(), frame.after()).expect("the list grew");
    assert_eq!(d.path, ".list.length");
    assert!(diff(frame.before().get("hit"), frame.before().get("hit")).is_none());
    // A port's own value, built in its arena, against the capture's.
    let mut arena = Arena::default();
    let list = arena.array(vec![Value::Num(1.0), Value::Num(2.0), Value::Num(3.0)]);
    check_value("list", frame.after().get("list"), arena.view(&list)).unwrap();
    let wrong = arena.array(vec![Value::Num(1.0), Value::Num(2.0), Value::Num(-0.0)]);
    let e = check_value("list", frame.after().get("list"), arena.view(&wrong)).unwrap_err();
    assert!(e.0.contains("[2]") && e.0.contains("-0"), "{e}");
}

#[test]
fn the_phases_are_the_captures() {
    let js = std::fs::read_to_string(root().join("tools/sim-capture/phases.mjs")).unwrap();
    let named: Vec<(String, String)> = js
        .lines()
        .filter_map(|l| l.trim().strip_prefix("['"))
        .map(|rest| {
            let mut parts = rest.split('\'');
            let name = parts.next().unwrap().to_string();
            parts.next();
            (name, parts.next().unwrap().to_string())
        })
        .collect();
    let mine: Vec<(String, String)> = PHASES.iter().map(|(p, n)| (n.to_string(), format!("{p:?}"))).collect();
    assert_eq!(named, mine);
    assert_eq!(Phase::from_name("unitTick"), Some(Phase::UnitTick));
    assert_eq!(Phase::Ordnance.name(), "ordnance");
}
