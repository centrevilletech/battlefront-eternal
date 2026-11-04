//! Replay the same owned recon continuations checked against JavaScript.
//! Foreign queries and callback mutations arrive as captured resume commands.
use bfe_content::{json, Value};
use bfe_sim::prediction::remotes::Session;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
};

#[test]
fn default_weapon_level_notifications_leave_authoritative_state_and_shots_unchanged() {
    use bfe_sim::{
        combat::{
            defs::{Js, JsObj, WeaponClass, WeaponDefs},
            weapons::{Admission, Aim, Trigger, WeaponDraws},
        },
        gen::{Posture, Weapon},
        store::DefId,
    };
    use std::rc::Rc;

    struct DefaultDraws {
        allow: bool,
        draws: usize,
    }
    impl WeaponDraws for DefaultDraws {
        fn draw(&mut self, _: u32) -> f64 {
            self.draws += 1;
            0.5
        }
        fn legacy(&mut self, weapon: u32) -> f64 {
            self.draw(weapon)
        }
        fn seeded(&mut self, _: f64, _: f64) {}
        fn admit(&mut self, _: u32) -> Admission {
            if self.allow {
                Admission::Yes
            } else {
                Admission::No
            }
        }
    }
    struct Recorded {
        draws: DefaultDraws,
        levels: Vec<(u32, f64)>,
    }
    impl WeaponDraws for Recorded {
        fn draw(&mut self, weapon: u32) -> f64 {
            self.draws.draw(weapon)
        }
        fn legacy(&mut self, weapon: u32) -> f64 {
            self.draws.legacy(weapon)
        }
        fn seeded(&mut self, seed: f64, value: f64) {
            self.draws.seeded(seed, value);
        }
        fn admit(&mut self, weapon: u32) -> Admission {
            self.draws.admit(weapon)
        }
        fn level_written(&mut self, weapon: u32, level: f64) {
            self.levels.push((weapon, level));
        }
    }
    let mut all_writes = Vec::new();
    for (base, charge, allow, updates) in [
        ("blaster", 1.0, true, 2),
        ("grenade", 0.0, true, 2),
        ("grenade", 0.0, false, 1),
    ] {
        let class = WeaponClass::new(Rc::new(JsObj {
            fields: vec![
                ("base".into(), Js::Str(base.into())),
                ("RoundsPerClip".into(), Js::Num(10.0)),
                ("ammo".into(), Js::Num(3.0)),
                ("ShotDelay".into(), Js::Num(0.125)),
                ("InitialSalvoDelay".into(), Js::Num(0.0)),
                ("MaxPressedTime".into(), Js::Num(charge)),
            ],
            def: None,
        }));
        let mut defs = WeaponDefs::default();
        defs.set(DefId(0), class.clone());
        let mut a = Weapon::create(DefId(0), &defs, true);
        a.admit_shot = true;
        let mut b = a.clone();
        let mut plain = DefaultDraws { allow, draws: 0 };
        let mut recorded = Recorded {
            draws: DefaultDraws { allow, draws: 0 },
            levels: Vec::new(),
        };
        let mut actual = Vec::new();
        let mut expected = Vec::new();
        let trigger = Trigger {
            held: true,
            pressed: true,
            ..Trigger::default()
        };
        let aim = Aim {
            stick: 0.0,
            posture: Some(Posture::Stand),
            zoomed: false,
            repair_target: false,
        };
        for _ in 0..updates {
            a.update(
                7,
                &class,
                0.125,
                true,
                &trigger,
                false,
                &aim,
                &mut plain,
                &mut actual,
            );
            b.update(
                7,
                &class,
                0.125,
                true,
                &trigger,
                false,
                &aim,
                &mut recorded,
                &mut expected,
            );
            let mut a_row = [0.0; Weapon::WORDS];
            let mut b_row = [0.0; Weapon::WORDS];
            a.put(&mut a_row, &mut Vec::new());
            b.put(&mut b_row, &mut Vec::new());
            for (a, b) in a_row.into_iter().zip(b_row) {
                assert!(jsrt::same_value(a, b));
            }
            assert_eq!(actual, expected);
            assert_eq!(plain.draws, recorded.draws.draws);
        }
        assert_eq!(a.level, 0.0);
        if charge > 0.0 {
            assert!(a.charging);
            assert_eq!(a.held, 0.25);
            assert!(actual.is_empty());
        } else {
            assert_eq!(a.throw_time, -1.0);
            assert!(!a.hidden);
            assert_eq!(actual.len(), usize::from(allow));
        }
        all_writes.extend(recorded.levels);
    }
    assert_eq!(
        all_writes,
        vec![(7, 0.0); 5],
        "same-level charge, completed throw and refused throw assignments"
    );
}

fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or_else(|| panic!("missing {key}"))
}
fn text(value: &Value) -> &str {
    value.as_str().unwrap()
}
fn words(value: &Value) -> Vec<f64> {
    let hex = text(value);
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
fn private_recon_replay_matches_js_and_wasm_continuations() {
    let mut child = None;
    let corpus: Box<dyn BufRead> =
        if let Some(path) = std::env::var_os("BFE_REMOTE_PREDICTION_CORPUS") {
            Box::new(BufReader::new(
                std::fs::File::open(path).expect("read recon prediction corpus"),
            ))
        } else {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let mut process =
                Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
                    .arg(root.join("tools/prediction-remotes-twin.mjs"))
                    .current_dir(&root)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .expect("run recon oracle");
            let stdout = process.stdout.take().unwrap();
            child = Some(process);
            Box::new(BufReader::new(stdout))
        };
    let mut sessions: HashMap<String, Session> = HashMap::new();
    let mut counts = [0usize; 4];
    let mut yields = 0;
    let mut disposed = 0;
    for (line, source) in corpus.lines().enumerate() {
        let event = json::parse(&source.unwrap()).unwrap();
        if event.get("type").and_then(Value::as_str) == Some("dispose") {
            assert!(sessions.remove(text(get(&event, "session"))).is_some());
            disposed += 1;
            continue;
        }
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
        if matches!(op, 2 | 3) && session.output.first() == Some(&1.0) {
            yields += 1;
        }
    }
    if let Some(child) = child {
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert!(
        disposed >= 10
            && sessions.is_empty()
            && counts[0] >= 10
            && counts[1] >= 10
            && counts[2] >= 1000
            && yields >= 5000
    );
    eprintln!(
        "recon prediction: {} sessions, operations {counts:?}, {yields} yields",
        disposed
    );
}
