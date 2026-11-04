use bfe_content::{
    bvh::{build, Positions},
    collision::{triangle_bounds, MapCollision, TriangleGrid},
    install::{install, Installed},
    json, Value,
};
use bfe_sim::{
    abi::context::{Context, MovementError},
    collision::{self, spheres, Body, Corners},
    movement::{
        self,
        soldier::{Input, V3},
    },
};
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Owned,
    Stateful,
    Legacy,
}
pub struct Lane {
    cx: Context,
    ids: Option<Installed>,
    order: Vec<u32>,
    pub ticks: usize,
    retained: Option<bfe_sim::abi::context::MovementResult>,
}
impl Lane {
    pub fn new() -> Self {
        Self {
            cx: Context::new(),
            ids: None,
            order: vec![],
            ticks: 0,
            retained: None,
        }
    }
}

fn content(v: &Value) -> MapCollision {
    let b = get(v, "buildings");
    let t = get(v, "terrain");
    let positions = narrow(get(b, "p"));
    let indices = array(get(b, "i"));
    let (terrain, ti) = if matches!(t, Value::Null) {
        (vec![], vec![])
    } else {
        (narrow(get(t, "p")), array(get(t, "i")))
    };
    MapCollision {
        tri_bounds: triangle_bounds(&positions, &indices),
        grid: (!ti.is_empty()).then(|| TriangleGrid::new(&terrain, &ti)),
        positions: positions.into(),
        indices: indices.into(),
        polygons: array(get(b, "polygons")).into(),
        terrain_positions: terrain.into(),
        terrain_indices: ti.into(),
        kill_y: -200.,
    }
}
fn input(v: &[f64; movement::IO_LEN]) -> Input {
    Input {
        forward: v[32],
        right: v[33],
        turn: v[34],
        pitch: v[35],
        jump: v[36] != 0.,
        crouch: v[37] != 0.,
        prone: v[38] != 0.,
        jet_direction: if v[39] != 0. {
            Some(V3::new(v[40], v[41], v[42]))
        } else {
            None
        },
    }
}
pub fn step(lane: &mut Lane, v: &Value, mode: Mode) {
    let legacy = mode == Mode::Legacy;
    match get(v, "op").as_str().unwrap() {
        "header" | "summary" => {}
        "install" => {
            let c = content(v);
            let b = build(Positions::F32(&c.positions), &c.indices);
            let t = (!c.terrain_indices.is_empty())
                .then(|| build(Positions::F32(&c.terrain_positions), &c.terrain_indices));
            let ids = if legacy {
                install(&mut collision::install::Kernel, &c, &b, t.as_ref()).unwrap()
            } else {
                lane.cx.collision_install(&c, &b, t.as_ref()).unwrap()
            };
            assert_eq!(ids.world, 0);
            lane.ids = Some(ids);
        }
        "shape" => {
            let p = corners(v);
            let index = array(get(v, "i"));
            let polygons = array(get(v, "polygons"));
            let id = if legacy {
                legacy_shape(p, &index, &polygons)
            } else {
                lane.cx.collision_shape(p, index, polygons).unwrap()
            };
            assert_eq!(id, num(v, "tree") as u32);
        }
        "body" => {
            let a = floats(get(v, "v"));
            let b = Body {
                tree: num(v, "tree") as usize,
                moving: yes(v, "moving"),
                offset: num(v, "offset"),
                bounds: a[..6].try_into().unwrap(),
                matrix: a[6..22].try_into().unwrap(),
                inverse: a[22..38].try_into().unwrap(),
            };
            let id = if legacy {
                let id = collision::body_new(b.tree as u32, b.moving as u32);
                assert!(id >= 0);
                spheres::body_offset(id as u32, b.offset);
                legacy_io()[20..58].copy_from_slice(&a);
                collision::body_set(id as u32);
                id as u32
            } else {
                lane.cx.collision_body(0, b).unwrap()
            };
            assert_eq!(id, num(v, "id") as u32);
            lane.order.push(id);
        }
        "place" => {
            let a = floats(get(v, "v"));
            let id = num(v, "id") as u32;
            if legacy {
                legacy_io()[20..58].copy_from_slice(&a);
                collision::body_set(id)
            } else {
                lane.cx
                    .collision_place(
                        id,
                        a[..6].try_into().unwrap(),
                        a[6..22].try_into().unwrap(),
                        a[22..38].try_into().unwrap(),
                    )
                    .unwrap()
            }
        }
        "order" => {
            lane.order = array(get(v, "order"));
            if legacy {
                order(0, &lane.order)
            } else {
                lane.cx.collision_order(0, &lane.order).unwrap()
            }
        }
        "new" => {
            let class: [f64; 21] = floats(get(v, "class")).try_into().unwrap();
            let id = if legacy {
                movement_io()[50..71].copy_from_slice(&class);
                movement::mover_new(0)
            } else {
                lane.cx.movement_new(0, class).unwrap()
            };
            assert_eq!(id, num(v, "id") as u32);
        }
        "tick" => {
            let id = num(v, "id") as u32;
            let a: [f64; 80] = floats(get(v, "input")).try_into().unwrap();
            let flags: Vec<u8> = array(get(v, "flags"))
                .into_iter()
                .map(|x| x as u8)
                .collect();
            assert_eq!(flags.len(), lane.order.len());
            let expected = floats(get(v, "expected"));
            if legacy {
                *movement_io() = a;
                unsafe {
                    std::slice::from_raw_parts_mut(movement::movement_flags_ptr(), flags.len())
                        .copy_from_slice(&flags)
                };
                assert_eq!(movement::mover_tick(id), num(v, "code") as u32);
                assert_eq!(
                    bits(movement_io()),
                    bits(&expected),
                    "legacy tick {} mover {id}",
                    lane.ticks
                );
            } else {
                let result = if mode == Mode::Stateful {
                    lane.cx.movement_load(id, a).and_then(|_| {
                        lane.cx
                            .movement_advance(id, input(&a), a[31], flags.clone())
                    })
                } else {
                    lane.cx.movement_tick(id, a, flags.clone())
                };
                if num(v, "code") == 2. {
                    assert!(matches!(result, Err(MovementError::UnknownState)))
                } else {
                    let result = result.unwrap();
                    assert_eq!(
                        bits(&result.io),
                        bits(&expected),
                        "owned tick {} mover {id}",
                        lane.ticks
                    );
                    assert_eq!(result.flags, flags);
                    if lane.retained.is_none() {
                        lane.retained = Some(result)
                    }
                }
            }
            lane.ticks += 1;
        }
        "grow" => {
            if !legacy {
                lane.cx.scratch(&vec![0; 700_000]).unwrap();
            }
        }
        op => panic!("unknown movement trace operation {op}"),
    }
}
fn movement_io() -> &'static mut [f64; 80] {
    unsafe { &mut *movement::movement_io_ptr().cast() }
}
pub fn replay(path: &Path, mode: Mode, only: Option<usize>) -> usize {
    let mut lanes = [Lane::new(), Lane::new()];
    let mut summary = None;
    for (line, text) in BufReader::new(File::open(path).unwrap())
        .lines()
        .enumerate()
    {
        let v = json::parse(&text.unwrap()).unwrap();
        let op = get(&v, "op").as_str().unwrap();
        if op == "header" {
            continue;
        }
        if op == "summary" {
            summary = Some(num(&v, "ticks") as usize);
            continue;
        }
        let cx = num(&v, "cx") as usize;
        if only.is_some_and(|n| n != cx) {
            continue;
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            step(&mut lanes[cx], &v, mode)
        }));
        if let Err(payload) = result {
            eprintln!("{} line {} context {cx}", path.display(), line + 1);
            std::panic::resume_unwind(payload)
        }
    }
    let total = lanes.iter().map(|x| x.ticks).sum();
    assert!(summary.is_some());
    assert!(total > 0);
    if only.is_none() {
        assert_eq!(Some(total), summary)
    }
    for lane in lanes {
        finish(lane, mode)
    }
    total
}
pub fn finish(lane: Lane, mode: Mode) {
    if mode != Mode::Legacy {
        let retained = lane.retained.as_ref().map(|x| bits(&x.io));
        lane.cx.dispose().unwrap();
        assert_eq!(lane.retained.as_ref().map(|x| bits(&x.io)), retained);
    }
}
pub fn interleave(a: &Path, b: &Path) -> usize {
    let mut lanes = [Lane::new(), Lane::new()];
    let mut readers = [
        BufReader::new(File::open(a).unwrap()).lines(),
        BufReader::new(File::open(b).unwrap()).lines(),
    ];
    let mut done = [false; 2];
    while !done.iter().all(|b| *b) {
        for i in 0..2 {
            match readers[i].next() {
                Some(line) => step(
                    &mut lanes[i],
                    &json::parse(&line.unwrap()).unwrap(),
                    Mode::Owned,
                ),
                None => done[i] = true,
            }
        }
    }
    let ticks = lanes.iter().map(|x| x.ticks).sum();
    for lane in lanes {
        finish(lane, Mode::Owned)
    }
    ticks
}
pub fn assert_no_legacy_exchange_use(path: &Path, lane: usize) {
    *movement_io() = [321.5; 80];
    legacy_io().fill(789.25);
    unsafe {
        std::slice::from_raw_parts_mut(movement::movement_flags_ptr(), 65536).fill(73);
        std::slice::from_raw_parts_mut(collision::enabled_ptr(), 65536).fill(87);
        std::slice::from_raw_parts_mut(collision::candidates_ptr(), 65536).fill(9876);
    }
    replay(path, Mode::Owned, Some(lane));
    assert_eq!(*movement_io(), [321.5; 80]);
    assert_eq!(*legacy_io(), [789.25; 64]);
    assert!(copy(movement::movement_flags_ptr(), 65536)
        .iter()
        .all(|&x| x == 73));
    assert!(copy(collision::enabled_ptr(), 65536)
        .iter()
        .all(|&x| x == 87));
    assert!(copy(collision::candidates_ptr(), 65536)
        .iter()
        .all(|&x| x == 9876));
}
fn get<'a>(v: &'a Value, name: &str) -> &'a Value {
    v.get(name).unwrap_or_else(|| panic!("missing {name}"))
}
fn num(v: &Value, name: &str) -> f64 {
    get(v, name).as_f64().unwrap()
}
fn yes(v: &Value, name: &str) -> bool {
    matches!(v.get(name), Some(Value::Bool(true)))
}
fn array(v: &Value) -> Vec<u32> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_f64().unwrap() as u32)
        .collect()
}
fn bytes(v: &Value) -> Vec<u8> {
    let s = v.as_str().unwrap();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
fn floats(v: &Value) -> Vec<f64> {
    bytes(v)
        .chunks_exact(8)
        .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
        .collect()
}
fn narrow(v: &Value) -> Vec<f32> {
    bytes(v)
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}
fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|n| n.to_bits()).collect()
}
fn corners(v: &Value) -> Corners {
    if yes(v, "wide") {
        Corners::F64(floats(get(v, "p")))
    } else {
        Corners::F32(narrow(get(v, "p")))
    }
}
fn legacy_io() -> &'static mut [f64; 64] {
    unsafe { &mut *collision::io_ptr().cast() }
}
fn copy<T: Copy>(pointer: *const T, n: usize) -> Vec<T> {
    unsafe { std::slice::from_raw_parts(pointer, n).to_vec() }
}
fn order(world: u32, values: &[u32]) {
    unsafe {
        std::slice::from_raw_parts_mut(
            collision::world_order(world, values.len() as u32),
            values.len(),
        )
        .copy_from_slice(values);
    }
}
fn legacy_shape(p: Corners, index: &[u32], polygons: &[u32]) -> u32 {
    let id = match p {
        Corners::F32(p) => {
            let id = collision::positions_new(p.len() as u32, 0);
            unsafe {
                std::slice::from_raw_parts_mut(collision::positions_at(id).cast::<f32>(), p.len())
                    .copy_from_slice(&p)
            };
            id
        }
        Corners::F64(p) => {
            let id = collision::positions_new(p.len() as u32, 1);
            unsafe {
                std::slice::from_raw_parts_mut(collision::positions_at(id).cast::<f64>(), p.len())
                    .copy_from_slice(&p)
            };
            id
        }
    };
    let tree = collision::tree_new(id, index.len() as u32, 0);
    unsafe {
        std::slice::from_raw_parts_mut(collision::tree_index(tree), index.len())
            .copy_from_slice(index);
    }
    collision::build::tree_build(tree);
    unsafe {
        std::slice::from_raw_parts_mut(
            spheres::tree_polygons(tree, polygons.len() as u32),
            polygons.len(),
        )
        .copy_from_slice(polygons);
    }
    tree
}
