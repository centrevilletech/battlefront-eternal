//! Rebuild the captured collision geometry and replay the same private actor
//! operations checked against JavaScript, without a JavaScript runtime.
use bfe_content::bvh::{build, Positions};
use bfe_content::{json, Value};
use bfe_sim::{
    collision::{self, Body, Corners, Grid, Tree, World},
    prediction::actors::Session,
};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
};

fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or_else(|| panic!("missing {key}"))
}
fn list(v: &Value) -> &[Value] {
    v.as_array().unwrap()
}
fn num(v: &Value) -> f64 {
    v.as_f64().unwrap()
}
fn id(v: &Value, key: &str) -> usize {
    num(get(v, key)) as usize
}
fn text(v: &Value) -> &str {
    v.as_str().unwrap()
}
fn bytes(v: &Value) -> Vec<u8> {
    let value = text(v);
    assert_eq!(value.len() % 2, 0);
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
        .collect()
}
fn f64s(v: &Value) -> Vec<f64> {
    bytes(v)
        .chunks_exact(8)
        .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
        .collect()
}
fn f32s(v: &Value) -> Vec<f32> {
    bytes(v)
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}
fn u32s(v: &Value) -> Vec<u32> {
    bytes(v)
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}
fn tree() -> Tree {
    Tree {
        roots: Vec::new(),
        positions: 0,
        index: Vec::new(),
        indexed: true,
        indirect: Vec::new(),
        polygons: Vec::new(),
    }
}
fn body() -> Body {
    Body {
        tree: 0,
        moving: false,
        offset: 0.0,
        bounds: [0.0; 6],
        matrix: [0.0; 16],
        inverse: [0.0; 16],
    }
}
fn world() -> World {
    World {
        tree: 0,
        terrain: None,
        grid: None,
        order: Vec::new(),
        version: 1,
        statics: None,
        broad: Default::default(),
    }
}
fn grid() -> Grid {
    Grid {
        positions: 0,
        index: Vec::new(),
        cells: Vec::new(),
        start: Vec::new(),
        stamp: Vec::new(),
        serial: 0,
        min_x: 0.0,
        min_z: 0.0,
        max_x: 0.0,
        max_z: 0.0,
        size: 1.0,
        nx: 0.0,
        nz: 0.0,
    }
}
fn grow<T>(values: &mut Vec<T>, len: usize, create: impl FnMut() -> T) {
    if values.len() < len {
        values.resize_with(len, create);
    }
}
fn install(g: &Value, state: &mut collision::State) {
    for p in list(get(g, "positions")) {
        let at = id(p, "id");
        grow(&mut state.positions, at + 1, || Corners::F32(Vec::new()));
        state.positions[at] = if get(p, "wide").truthy() {
            Corners::F64(f64s(get(p, "data")))
        } else {
            Corners::F32(f32s(get(p, "data")))
        };
    }
    for t in list(get(g, "trees")) {
        let at = id(t, "id");
        grow(&mut state.trees, at + 1, tree);
        let position = id(t, "positions");
        let index = u32s(get(t, "indices"));
        let packed = build(
            match &state.positions[position] {
                Corners::F32(v) => Positions::F32(v),
                Corners::F64(v) => Positions::F64(v),
            },
            &index,
        );
        state.trees[at] = Tree {
            roots: packed.roots,
            indirect: packed.indirect,
            positions: position,
            index,
            indexed: true,
            polygons: u32s(get(t, "polygons")),
        };
    }
    for s in list(get(g, "sets")) {
        let at = id(s, "id");
        grow(&mut state.sets, at + 1, || collision::spheres::FixedSet {
            tree: 0,
            bounds: Vec::new(),
            polygons: Vec::new(),
            offset: 0.0,
        });
        state.sets[at] = collision::spheres::FixedSet {
            tree: id(s, "tree"),
            bounds: f32s(get(s, "bounds")),
            polygons: u32s(get(s, "polygons")),
            offset: num(get(s, "offset")),
        };
    }
    for b in list(get(g, "bodies")) {
        let at = id(b, "id");
        grow(&mut state.bodies, at + 1, body);
        state.bodies[at] = Body {
            tree: id(b, "tree"),
            moving: get(b, "moving").truthy(),
            offset: num(get(b, "offset")),
            ..body()
        };
    }
    for g in list(get(g, "grids")) {
        let at = id(g, "id");
        let n = f64s(get(g, "numbers"));
        let index = u32s(get(g, "indices"));
        grow(&mut state.grids, at + 1, grid);
        state.grids[at] = Grid {
            positions: id(g, "positions"),
            stamp: vec![0; index.len() / 3],
            index,
            cells: u32s(get(g, "cells")),
            start: u32s(get(g, "start")),
            serial: 0,
            min_x: n[0],
            min_z: n[1],
            max_x: n[2],
            max_z: n[3],
            size: n[4],
            nx: n[5],
            nz: n[6],
        };
    }
    let w = get(g, "world");
    let at = id(w, "id");
    grow(&mut state.worlds, at + 1, world);
    state.worlds[at] = World {
        tree: id(w, "tree"),
        terrain: get(w, "terrain").as_f64().map(|n| n as usize),
        grid: get(w, "grid").as_f64().map(|n| n as usize),
        order: list(get(w, "order"))
            .iter()
            .map(|n| num(n) as u32)
            .collect(),
        statics: Some(id(w, "statics")),
        ..world()
    };
}
fn equal(actual: &[f64], expected: &[f64], place: &str) {
    assert_eq!(actual.len(), expected.len(), "{place} output length");
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        assert!(jsrt::same_value(a, e), "{place} word {i}: {a:?} != {e:?}");
    }
}

#[test]
fn private_actor_sessions_match_js_and_wasm_on_owned_geometry() {
    let mut child = None;
    let corpus: Box<dyn BufRead> =
        if let Some(path) = std::env::var_os("BFE_ACTOR_PREDICTION_CORPUS") {
            Box::new(BufReader::new(
                std::fs::File::open(path).expect("read actor corpus"),
            ))
        } else {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let mut process =
                Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
                    .arg(root.join("tools/prediction-actors-twin.mjs"))
                    .current_dir(&root)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .expect("run actor oracle");
            let stdout = process.stdout.take().unwrap();
            child = Some(process);
            Box::new(BufReader::new(stdout))
        };
    let mut sessions: HashMap<String, Session> = HashMap::new();
    let mut collision = collision::State::default();
    let mut worlds = 0;
    let mut steps = 0;
    for (line, source) in corpus.lines().enumerate() {
        let event = json::parse(&source.unwrap()).unwrap();
        if text(get(&event, "type")) == "collision" {
            install(get(&event, "geometry"), &mut collision);
            worlds += 1;
            continue;
        }
        for b in list(get(&event, "bodies")) {
            let body = &mut collision.bodies[id(b, "id")];
            let v = f64s(get(b, "words"));
            body.bounds.copy_from_slice(&v[..6]);
            body.matrix.copy_from_slice(&v[6..22]);
            body.inverse.copy_from_slice(&v[22..38]);
        }
        collision.fixed_version += 1;
        let session = sessions
            .entry(text(get(&event, "session")).into())
            .or_default();
        session.input = f64s(get(&event, "input"));
        let op = id(&event, "op") as u32;
        let result = session.run(op, &mut collision);
        let label = format!("{} line {line}", text(get(&event, "label")));
        if let Some(error) = get(&event, "error").as_str() {
            assert_eq!(result, Err(error), "{label}");
        } else {
            assert_eq!(result, Ok(()), "{label}");
        }
        equal(&session.output, &f64s(get(&event, "output")), &label);
        if op == 1 {
            steps += 1;
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
    assert!(worlds >= 2 && sessions.len() >= 2 && steps >= 1000);
    eprintln!(
        "actor prediction: {worlds} collision worlds, {} sessions, {steps} exact steps",
        sessions.len()
    );
}
