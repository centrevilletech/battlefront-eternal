use bfe_content::{
    bvh::{build, Positions},
    collision::{triangle_bounds, MapCollision, TriangleGrid},
    install::{install, Installed},
    json, Value,
};
use bfe_sim::{
    abi::context::{Context, MovementError},
    collision::{self, Body, Corners},
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
pub struct Twin {
    cx: Context,
    collision: Box<collision::CollisionOwner>,
    movement: Box<movement::MovementOwner>,
    ids: Option<Installed>,
    order: Vec<u32>,
    pub ticks: usize,
    retained: Option<bfe_sim::abi::context::MovementResult>,
}
impl Twin {
    pub fn new() -> Self {
        Self {
            cx: Context::new(),
            collision: Box::new(collision::CollisionOwner::new()),
            movement: Box::new(movement::MovementOwner::new()),
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
pub fn step(twin: &mut Twin, v: &Value, mode: Mode) {
    let legacy = mode == Mode::Legacy;
    match get(v, "op").as_str().unwrap() {
        "header" | "summary" => {}
        "install" => {
            let c = content(v);
            let b = build(Positions::F32(&c.positions), &c.indices);
            let t = (!c.terrain_indices.is_empty())
                .then(|| build(Positions::F32(&c.terrain_positions), &c.terrain_indices));
            let ids = if legacy {
                install(&mut *twin.collision, &c, &b, t.as_ref()).unwrap()
            } else {
                twin.cx.collision_install(&c, &b, t.as_ref()).unwrap()
            };
            assert_eq!(ids.world, 0);
            twin.ids = Some(ids);
        }
        "shape" => {
            let p = corners(v);
            let index = array(get(v, "i"));
            let polygons = array(get(v, "polygons"));
            let id = if legacy {
                legacy_shape(&mut twin.collision, p, &index, &polygons)
            } else {
                twin.cx.collision_shape(p, index, polygons).unwrap()
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
                let id = twin.collision.body_new(b.tree as u32, b.moving as u32);
                assert!(id >= 0);
                twin.collision.body_offset(id as u32, b.offset);
                twin.collision.io_mut()[20..58].copy_from_slice(&a);
                twin.collision.body_set(id as u32);
                id as u32
            } else {
                twin.cx.collision_body(0, b).unwrap()
            };
            assert_eq!(id, num(v, "id") as u32);
            twin.order.push(id);
        }
        "place" => {
            let a = floats(get(v, "v"));
            let id = num(v, "id") as u32;
            if legacy {
                twin.collision.io_mut()[20..58].copy_from_slice(&a);
                twin.collision.body_set(id)
            } else {
                twin.cx
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
            twin.order = array(get(v, "order"));
            if legacy {
                order(&mut twin.collision, 0, &twin.order)
            } else {
                twin.cx.collision_order(0, &twin.order).unwrap()
            }
        }
        "new" => {
            let class: [f64; 21] = floats(get(v, "class")).try_into().unwrap();
            let id = if legacy {
                twin.movement.io_mut()[50..71].copy_from_slice(&class);
                twin.movement.mover_new(0)
            } else {
                twin.cx.movement_new(0, class).unwrap()
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
            assert_eq!(flags.len(), twin.order.len());
            let expected = floats(get(v, "expected"));
            if legacy {
                *twin.movement.io_mut() = a;
                unsafe {
                    std::slice::from_raw_parts_mut(twin.movement.flags_mut().as_mut_ptr(), flags.len())
                        .copy_from_slice(&flags)
                };
                assert_eq!(twin.movement.tick(&mut twin.collision.state, id), num(v, "code") as u32);
                assert_eq!(
                    bits(twin.movement.io_mut()),
                    bits(&expected),
                    "legacy tick {} mover {id}",
                    twin.ticks
                );
            } else {
                let result = if mode == Mode::Stateful {
                    twin.cx.movement_load(id, a).and_then(|_| {
                        twin.cx
                            .movement_advance(id, input(&a), a[31], flags.clone())
                    })
                } else {
                    twin.cx.movement_tick(id, a, flags.clone())
                };
                if num(v, "code") == 2. {
                    assert!(matches!(result, Err(MovementError::UnknownState)))
                } else {
                    let result = result.unwrap();
                    assert_eq!(
                        bits(&result.io),
                        bits(&expected),
                        "owned tick {} mover {id}",
                        twin.ticks
                    );
                    assert_eq!(result.flags, flags);
                    if twin.retained.is_none() {
                        twin.retained = Some(result)
                    }
                }
            }
            twin.ticks += 1;
        }
        "grow" => {
            if !legacy {
                twin.cx.scratch(&vec![0; 700_000]).unwrap();
            }
        }
        op => panic!("unknown movement trace operation {op}"),
    }
}
pub fn replay(path: &Path, mode: Mode, only: Option<usize>) -> usize {
    let mut twins = [Twin::new(), Twin::new()];
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
            step(&mut twins[cx], &v, mode)
        }));
        if let Err(payload) = result {
            eprintln!("{} line {} context {cx}", path.display(), line + 1);
            std::panic::resume_unwind(payload)
        }
    }
    let total = twins.iter().map(|x| x.ticks).sum();
    assert!(summary.is_some());
    assert!(total > 0);
    if only.is_none() {
        assert_eq!(Some(total), summary)
    }
    for twin in twins {
        finish(twin, mode)
    }
    total
}
pub fn finish(twin: Twin, mode: Mode) {
    if mode != Mode::Legacy {
        let retained = twin.retained.as_ref().map(|x| bits(&x.io));
        twin.cx.dispose().unwrap();
        assert_eq!(twin.retained.as_ref().map(|x| bits(&x.io)), retained);
    }
}
pub fn interleave(a: &Path, b: &Path) -> usize {
    let mut twins = [Twin::new(), Twin::new()];
    let mut readers = [
        BufReader::new(File::open(a).unwrap()).lines(),
        BufReader::new(File::open(b).unwrap()).lines(),
    ];
    let mut done = [false; 2];
    while !done.iter().all(|b| *b) {
        for i in 0..2 {
            match readers[i].next() {
                Some(line) => step(
                    &mut twins[i],
                    &json::parse(&line.unwrap()).unwrap(),
                    Mode::Owned,
                ),
                None => done[i] = true,
            }
        }
    }
    let ticks = twins.iter().map(|x| x.ticks).sum();
    for twin in twins {
        finish(twin, Mode::Owned)
    }
    ticks
}
pub fn assert_no_legacy_exchange_use(path: &Path, index: usize) {
    let mut twin = Twin::new();
    *twin.movement.io_mut() = [321.5; 80];
    twin.collision.io_mut().fill(789.25);
    twin.movement.flags_mut().fill(73);
    unsafe {
        std::slice::from_raw_parts_mut(twin.collision.enabled_ptr(), 65536).fill(87);
        std::slice::from_raw_parts_mut(twin.collision.candidates_ptr(), 65536).fill(9876);
    }
    replay(path, Mode::Owned, Some(index));
    assert_eq!(*twin.movement.io_mut(), [321.5; 80]);
    assert_eq!(*twin.collision.io_mut(), [789.25; 64]);
    assert!(twin.movement.flags_mut().iter().all(|&x| x == 73));
    assert!(copy(twin.collision.enabled_ptr(), 65536).iter().all(|&x| x == 87));
    assert!(copy(twin.collision.candidates_ptr(), 65536).iter().all(|&x| x == 9876));
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
fn copy<T: Copy>(pointer: *const T, n: usize) -> Vec<T> {
    unsafe { std::slice::from_raw_parts(pointer, n).to_vec() }
}
fn order(owner: &mut collision::CollisionOwner, world: u32, values: &[u32]) {
    unsafe {
        std::slice::from_raw_parts_mut(
            owner.world_order_slots(world, values.len() as u32).as_mut_ptr(),
            values.len(),
        )
        .copy_from_slice(values);
    }
}
fn legacy_shape(owner: &mut collision::CollisionOwner, p: Corners, index: &[u32], polygons: &[u32]) -> u32 {
    let id = match p {
        Corners::F32(p) => {
            let id = owner.positions_new(p.len() as u32, 0);
            unsafe {
                std::slice::from_raw_parts_mut(owner.positions_at(id).cast::<f32>(), p.len())
                    .copy_from_slice(&p)
            };
            id
        }
        Corners::F64(p) => {
            let id = owner.positions_new(p.len() as u32, 1);
            unsafe {
                std::slice::from_raw_parts_mut(owner.positions_at(id).cast::<f64>(), p.len())
                    .copy_from_slice(&p)
            };
            id
        }
    };
    let tree = owner.tree_new(id, index.len() as u32, 0);
    unsafe {
        std::slice::from_raw_parts_mut(owner.tree_index(tree), index.len())
            .copy_from_slice(index);
    }
    owner.state.build_tree(tree);
    unsafe {
        std::slice::from_raw_parts_mut(
            owner.tree_polygon_ids(tree, polygons.len() as u32).as_mut_ptr(),
            polygons.len(),
        )
        .copy_from_slice(polygons);
    }
    tree
}
