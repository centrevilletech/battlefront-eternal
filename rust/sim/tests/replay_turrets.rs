//! Turret aim/rig replay, using the same TurretSide implementation as the
//! native crews and the live command adapter. No afterstate is served.
#![cfg(feature = "replay")]
use bfe_sim::battle::turrets;
use bfe_sim::gen::{PtInput, PtOperation, PtSettings, Replay};
use sim_replay::{check_num, check_value, check_vec3, replay_corpus, Arena, Corpus, Frame, Node, Outcome, Value};

const METHODS: &[&str] = &["steer", "holdYaw", "viewDirection", "present", "gunWant", "aimGun", "pose", "muzzle", "fireDirection", "eyePoint", "aimOrigin", "seatPoint", "seatTurn", "seatYaw"];
fn replay(frame: &Frame) -> Result<(), String> {
    let mut cx = Replay::default(); let mut arena = Arena::default();
    let input = PtInput::from_value(frame.before().get("input"), &mut cx, "input")?;
    let settings = PtSettings::from_value(frame.before().get("settings"), &mut cx, "settings")?;
    let (op, alias, direction) = (input.op, input.alias, input.direction);
    let result = turrets::run(input, &settings);
    let state = result.state.to_value(&mut cx, &mut arena);
    check_value("turret state", frame.after().get("input").get("state"), arena.view(&state)).map_err(|e| e.to_string())?;
    check_value("turret settings", frame.after().get("settings"), frame.before().get("settings")).map_err(|e| e.to_string())?;
    match op {
        PtOperation::HoldYaw | PtOperation::SeatYaw => check_num("return", frame.result(), result.number),
        PtOperation::Muzzle => {
            if direction { check_vec3("muzzle direction", frame.after().get("muzzleDirection"), result.direction).map_err(|e| e.to_string())?; }
            check_vec3("return", frame.result(), if alias { result.direction } else { result.point })
        }
        PtOperation::ViewDirection | PtOperation::EyePoint | PtOperation::AimOrigin | PtOperation::SeatPoint | PtOperation::FireDirection => check_vec3("return", frame.result(), result.point),
        PtOperation::SeatTurn => {
            let q = arena.quat(result.turn); check_value("return", frame.result(), arena.view(&q))
        }
        PtOperation::GunWant => {
            let pair = arena.object("", vec![("yaw", Value::Num(result.pair[0])), ("pitch", Value::Num(result.pair[1]))]);
            check_value("return", frame.result(), arena.view(&pair))
        }
        _ => check_value("return", frame.result(), arena.view(&Value::Undefined)),
    }.map_err(|e| e.to_string())
}
#[test]
fn turret_math_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return }; let mut total = 0;
    for method in METHODS {
        let result = replay_corpus(&corpus, &format!("Turret.{method}"), |_, frame| match replay(frame) { Ok(()) => Outcome::Same, Err(e) => Outcome::Failed(e) }).unwrap();
        eprintln!("{}", result.summary()); total += result.replayed(); result.assert_clean();
    }
    assert!(total > 0); eprintln!("turret math: {total} exact frames");
}
#[test]
fn turret_math_rejects_state_and_output_mutations() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return }; let mut state = false; let mut output = false;
    for capture in corpus.captures() { let capture = capture.unwrap();
        for method in ["Turret.steer", "Turret.muzzle"] { for frame in capture.frames(method) {
            let frame = frame.unwrap(); replay(&frame).expect("unmodified turret frame");
            if !state {
                let mut changed = frame.clone(); let id = changed.after().get("input").get("state").node_id().unwrap();
                if let Node::Object { fields, .. } = &mut changed.exit.arena.nodes[id as usize] { fields.iter_mut().find(|(k,_)| k == "yaw").unwrap().1 = Value::Num(98765.0); }
                assert!(replay(&changed).is_err(), "wrong aim accepted"); state = true;
            }
            if !output && method == "Turret.muzzle" {
                let mut changed = frame.clone(); let id = changed.exit.root().node_id().unwrap();
                if let Node::Object { fields, .. } = &mut changed.exit.arena.nodes[id as usize] { fields.iter_mut().find(|(k,_)| k == "r").unwrap().1 = Value::Null; }
                assert!(replay(&changed).is_err(), "wrong muzzle accepted"); output = true;
            }
            if state && output { return; }
        } }
    }
    panic!("missing turret mutation coverage: state={state}, output={output}");
}
