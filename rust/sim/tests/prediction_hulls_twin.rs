//! The same owned operations and collision replies checked against the
//! JavaScript prediction path, replayed by the native private hull session.
use bfe_content::{json, Value};
use bfe_sim::gen::OrdnanceOwner;
use bfe_sim::prediction::hulls::Session;
use bfe_sim::vehicles::{BodyId, Ground, Hit, Hull, Polygon, VehicleWorld};
use std::{collections::HashMap, path::Path, process::Command};
use three::{Matrix4, Quaternion, Vector3};

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
        .map(|at| {
            let mut bytes = [0; 8];
            for (i, byte) in bytes.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&hex[at + i * 2..at + i * 2 + 2], 16).unwrap();
            }
            f64::from_le_bytes(bytes)
        })
        .collect()
}
fn equal(actual: &[f64], expected: &[f64], place: &str) {
    assert_eq!(actual.len(), expected.len(), "{place} word count");
    for (i, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            jsrt::same_value(actual, expected),
            "{place} word {i}: {actual:?} != {expected:?}"
        );
    }
}

#[test]
fn private_hulls_match_js_and_wasm_with_the_same_queries() {
    let external = std::env::var_os("BFE_HULL_PREDICTION_CORPUS");
    let corpus = if let Some(path) = &external {
        std::fs::read_to_string(path).expect("read owned hull prediction corpus")
    } else {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let oracle = Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
            .arg(root.join("tools/prediction-hulls-twin.mjs"))
            .current_dir(&root)
            .output()
            .expect("run hull prediction oracle");
        assert!(
            oracle.status.success(),
            "{}",
            String::from_utf8_lossy(&oracle.stderr)
        );
        String::from_utf8(oracle.stdout).expect("hull prediction corpus UTF-8")
    };
    let mut sessions: HashMap<String, Session> = HashMap::new();
    let mut counts = [0usize; 3];
    let mut queries = 0usize;
    for (line, source) in corpus.lines().enumerate() {
        let event = json::parse(source).unwrap();
        let session = sessions
            .entry(text(get(&event, "session")).into())
            .or_default();
        let label = format!("{} operation {line}", text(get(&event, "label")));
        let op = get(&event, "op").as_f64().unwrap() as u32;
        let expected_queries = get(&event, "queries").as_array().unwrap();
        let mut at = 0;
        let mut query = |op: u32, args: &[f64]| -> Vec<f64> {
            let q = expected_queries
                .get(at)
                .unwrap_or_else(|| panic!("{label}: unexpected query {at}/{op}"));
            assert_eq!(
                op,
                get(q, "op").as_f64().unwrap() as u32,
                "{label} query {at}"
            );
            equal(
                args,
                &words(get(q, "args")),
                &format!("{label} query {at}/{op}"),
            );
            at += 1;
            words(get(q, "values"))
        };
        session.input = words(get(&event, "input"));
        let result = session.run(op, &mut query);
        assert_eq!(at, expected_queries.len(), "{label}: omitted queries");
        queries += at;
        if let Some(error) = get(&event, "error").as_str() {
            assert_eq!(result, Err(error), "{label}");
        } else {
            assert_eq!(result, Ok(()), "{label}");
        }
        equal(&session.output, &words(get(&event, "output")), &label);
        counts[op as usize] += 1;
    }
    assert!(sessions.len() >= 8);
    assert!(counts[0] >= 8 && counts[1] >= 8 && counts[2] > 1000);
    assert!(queries > 2000);
    eprintln!(
        "prediction hulls: {} sessions, operations {counts:?}, {queries} checked query replies",
        sessions.len()
    );
}

#[derive(Default)]
struct Authority {
    events: Vec<&'static str>,
    presented: Vec<(f64, bool)>,
}

// Intentionally inherits damage_allowed. This represents an authoritative
// world that predates private prediction and must keep normal damage effects.
impl VehicleWorld for Authority {
    fn cast(&mut self, _: &Vector3, _: &Vector3, _: f64, _: Option<BodyId>) -> Option<Hit> {
        panic!("unexpected cast")
    }
    fn push_walls(
        &mut self,
        _: usize,
        _: &[Vector3],
        _: f64,
        _: &Quaternion,
        _: Option<&Quaternion>,
        _: &Vector3,
        _: &mut Vector3,
        _: &mut Vector3,
        _: bool,
    ) -> bool {
        panic!("unexpected wall query")
    }
    fn ground(&mut self, _: f64, _: f64, _: f64) -> Option<Ground> {
        panic!("unexpected ground query")
    }
    fn spring_polygons(&mut self, _: usize, _: usize, _: &Vector3, _: f64, _: &mut Vec<Polygon>) {
        panic!("unexpected spring query")
    }
    fn move_body(&mut self, _: BodyId, _: &Matrix4) {
        panic!("unexpected collider publication")
    }
    fn enable_body(&mut self, body: BodyId, enabled: bool) {
        assert_eq!(body, 7);
        assert!(!enabled);
        self.events.push("body-disabled");
    }
    fn body_enabled(&mut self, _: BodyId) -> bool {
        panic!("unexpected body query")
    }
    fn water_over(&mut self, _: f64, _: f64) -> bool {
        panic!("unexpected water query")
    }
    fn water_level(&mut self) -> f64 {
        panic!("unexpected water query")
    }
    fn has_water(&mut self) -> bool {
        panic!("unexpected water query")
    }
    fn in_death_region(&mut self, _: &Vector3) -> bool {
        panic!("unexpected death region query")
    }
    fn damaged(&mut self, hull: usize, amount: f64, attacker: &OrdnanceOwner) {
        assert_eq!(hull, 3);
        assert!(amount == 25.0 || amount == f64::INFINITY);
        assert_eq!(*attacker, OrdnanceOwner::Null);
        self.events.push("damaged");
    }
    fn destroyed_crew(&mut self, hull: usize) {
        assert_eq!(hull, 3);
        self.events.push("destroyed-crew");
    }
    fn destroyed(&mut self, hull: usize) {
        assert_eq!(hull, 3);
        self.events.push("destroyed");
    }
    fn explosion(&mut self, _: usize, _: bool, _: &Vector3) {
        panic!("unexpected class explosion")
    }
    fn effect(&mut self, name: &str, _: &Vector3) {
        assert_eq!(name, "explosion");
        self.events.push("effect");
    }
    fn raycast_units(&mut self, _: &Vector3, _: &Vector3, _: f64) -> Option<f64> {
        panic!("unexpected unit query")
    }
    fn clock(&mut self) -> f64 {
        panic!("unexpected clock")
    }
    fn tow_released(&mut self, _: usize, _: &str) {
        panic!("unexpected tow callback")
    }
    fn respawn_clock(&mut self, hull: usize) {
        assert_eq!(hull, 3);
        self.events.push("respawn-clock");
    }
    fn release_crew(&mut self, hull: usize) {
        assert_eq!(hull, 3);
        self.events.push("release-crew");
    }
    fn current(&mut self, hull: &Hull) {
        self.presented.push((hull.health, hull.alive));
    }
}

#[test]
fn native_authoritative_world_keeps_default_damage_and_destruction_callbacks() {
    let mut world = Authority::default();
    let mut hull = Hull {
        index: 3,
        body: 7,
        alive: true,
        max_health: 100.0,
        health: 100.0,
        driver: true,
        velocity: Vector3::new(3.0, 4.0, 5.0),
        ..Hull::default()
    };
    assert!(world.damage_allowed());
    hull.damage(&mut world, 25.0, OrdnanceOwner::Null);
    assert_eq!(hull.health, 75.0);
    assert!(hull.alive);
    assert_eq!(world.events, ["damaged"]);
    hull.damage(&mut world, f64::INFINITY, OrdnanceOwner::Null);
    assert_eq!(hull.health, 0.0);
    assert!(!hull.alive);
    assert!(!hull.driver);
    assert_eq!(hull.velocity, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(
        world.presented,
        [(100.0, true), (75.0, true), (0.0, false), (0.0, false)]
    );
    assert_eq!(
        world.events,
        [
            "damaged",
            "damaged",
            "body-disabled",
            "respawn-clock",
            "destroyed-crew",
            "destroyed",
            "release-crew",
            "effect"
        ]
    );
    hull.damage(&mut world, 25.0, OrdnanceOwner::Null);
    assert_eq!(
        world.events.len(),
        8,
        "already destroyed hull has no further effects"
    );
}
