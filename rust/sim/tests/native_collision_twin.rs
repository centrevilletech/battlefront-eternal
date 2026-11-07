#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
use bfe_content::{
    bvh::{build, Positions},
    collision::{triangle_bounds, MapCollision, TriangleGrid},
    install::{install, Installed},
    json, Value,
};
use bfe_sim::{
    abi::context::{CollisionQuery, CollisionResult, Context},
    collision::{self, Body, Corners, Ray},
};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
};

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
fn content(v: &Value) -> MapCollision {
    let b = get(v, "buildings");
    let t = get(v, "terrain");
    let positions = narrow(get(b, "p"));
    let indices = array(get(b, "i"));
    let terrain = narrow(get(t, "p"));
    let ti = array(get(t, "i"));
    MapCollision {
        tri_bounds: triangle_bounds(&positions, &indices),
        grid: Some(TriangleGrid::new(&terrain, &ti)),
        positions: positions.into(),
        indices: indices.into(),
        polygons: array(get(b, "polygons")).into(),
        terrain_positions: terrain.into(),
        terrain_indices: ti.into(),
        kill_y: -110.,
    }
}
fn query(v: &Value, ids: Installed) -> CollisionQuery {
    let n = floats(get(v, "v"));
    let ray = || Ray {
        ox: n[0],
        oy: n[1],
        oz: n[2],
        dx: n[3],
        dy: n[4],
        dz: n[5],
    };
    match get(v, "name").as_str().unwrap() {
        "cast" => CollisionQuery::Cast {
            world: ids.world,
            ray: ray(),
            far: n[6],
            hypot_xz: n[7],
            ignore: num(v, "ignore") as i32,
            static_only: yes(v, "staticOnly"),
            moving_only: yes(v, "movingOnly"),
        },
        "gather" => CollisionQuery::Gather {
            world: ids.world,
            center: n[..3].try_into().unwrap(),
            radius: n[3],
            ignore: num(v, "ignore") as i32,
            cache: (num(v, "cache") >= 0.).then(|| num(v, "cache") as u32),
            blockers: (num(v, "blockers") >= 0.).then(|| num(v, "blockers") as u32),
        },
        "overlap" => CollisionQuery::Overlap {
            world: ids.world,
            center: n[..3].try_into().unwrap(),
            radius: n[3],
            ignore: num(v, "ignore") as i32,
        },
        "tree" => CollisionQuery::Tree {
            tree: ids.buildings,
            ray: ray(),
            near: n[8],
            far: n[6],
        },
        "grid" => CollisionQuery::Grid {
            grid: ids.grid as u32,
            ray: ray(),
            far: n[6],
        },
        "near" => CollisionQuery::TerrainNear {
            grid: ids.grid as u32,
            center: n[..3].try_into().unwrap(),
            radius: n[3],
        },
        _ => panic!("unknown query"),
    }
}
// The isolated exchange replay owns the same leaf banks as the WASM facade.
// Explicit loans keep it independent from each Context's validated entry path.
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
fn legacy_query(owner: &mut collision::CollisionOwner, v: &Value, ids: Installed, flags: &[u8]) -> CollisionResult {
    let values = floats(get(v, "v"));
    owner.io_mut()[..values.len()].copy_from_slice(&values);
    let mut count = 0;
    let enabled = |owner: &mut collision::CollisionOwner, n: usize| {
        let candidates = copy(owner.candidates_ptr(), n);
        for (c, &place) in candidates.iter().enumerate() {
            unsafe { *owner.enabled_ptr().add(c) = flags[place as usize] }
        }
    };
    let name = get(v, "name").as_str().unwrap();
    let code = match name {
        "cast" => {
            count = owner.world_candidates(
                ids.world,
                num(v, "ignore") as i32,
                yes(v, "staticOnly") as u32,
                yes(v, "movingOnly") as u32,
            ) as usize;
            enabled(owner, count);
            if yes(v, "movingOnly") {
                owner.world_cast_moving(ids.world, count as u32)
            } else {
                owner.world_cast(ids.world, count as u32)
            }
        }
        "gather" => {
            count = owner.gather_begin(
                ids.world,
                num(v, "cache") as i32,
                num(v, "blockers") as i32,
                num(v, "ignore") as i32,
            ) as usize;
            enabled(owner, count);
            owner.gather_end(ids.world, count as u32) as i32
        }
        "overlap" => {
            let n = owner.overlap_begin(ids.world, num(v, "ignore") as i32);
            if n < 0 {
                1
            } else {
                count = n as usize;
                enabled(owner, count);
                owner.overlap_end(ids.world, count as u32) as i32
            }
        }
        "tree" => owner.tree_ray(ids.buildings) as i32,
        "grid" => owner.grid_ray(ids.grid as u32) as i32,
        "near" => owner.terrain_near_ray(ids.grid as u32) as i32,
        _ => panic!("unknown legacy query"),
    };
    let io = *owner.io_mut();
    if name == "gather" || name == "near" { assert_eq!(io[58], 0.0); }
    CollisionResult {
        code,
        io,
        candidates: copy(owner.candidates_ptr(), count),
        enabled: copy(owner.enabled_ptr(), count),
        words: if name == "gather" {
            owner.gather_words().to_vec()
        } else {
            vec![]
        },
        near: if name == "near" {
            owner.terrain_triangles().to_vec()
        } else {
            vec![]
        },
    }
}
struct Lane {
    context: Context,
    exchange: Box<collision::CollisionOwner>,
    ids: Option<Installed>,
    order: Vec<u32>,
    retained: Option<CollisionResult>,
}
impl Lane {
    fn new() -> Self {
        Self {
            context: Context::new(),
            exchange: Box::new(collision::CollisionOwner::new()),
            ids: None,
            order: vec![],
            retained: None,
        }
    }
}
fn replay(path: &Path, legacy: Option<usize>) {
    let mut lanes = [Lane::new(), Lane::new()];
    let mut queries = 0;
    let mut summary = false;
    for (line, text) in BufReader::new(File::open(path).unwrap())
        .lines()
        .enumerate()
    {
        let text = text.unwrap();
        let v = json::parse(&text).unwrap();
        let op = get(&v, "op").as_str().unwrap();
        if op == "summary" {
            assert_eq!(num(&v, "contexts"), 2.);
            assert!(num(&v, "queries") > 3800.);
            summary = true;
            continue;
        }
        let cx = num(&v, "cx") as usize;
        if legacy.is_some_and(|n| n != cx) {
            continue;
        }
        let lane = &mut lanes[cx];
        let is_legacy = legacy.is_some();
        match op {
            "install" => {
                let c = content(&v);
                let b = build(Positions::F32(&c.positions), &c.indices);
                let t = build(Positions::F32(&c.terrain_positions), &c.terrain_indices);
                let ids = if is_legacy {
                    install(&mut *lane.exchange, &c, &b, Some(&t)).unwrap()
                } else {
                    lane.context.collision_install(&c, &b, Some(&t)).unwrap()
                };
                assert_eq!(
                    [
                        ids.world,
                        ids.grid as u32,
                        ids.buildings,
                        ids.terrain as u32,
                        ids.statics
                    ]
                    .as_slice(),
                    array(get(&v, "ids"))
                );
                let cache = if is_legacy {
                    lane.exchange.cache_new(1.5)
                } else {
                    lane.context.collision_cache(ids.world, 1.5).unwrap()
                };
                assert_eq!(cache, num(&v, "cache") as u32);
                lane.ids = Some(ids);
            }
            "shape" => {
                let p = corners(&v);
                let i = array(get(&v, "i"));
                let polygons = array(get(&v, "polygons"));
                let tree = if is_legacy {
                    legacy_shape(&mut lane.exchange, p, &i, &polygons)
                } else {
                    lane.context.collision_shape(p, i, polygons).unwrap()
                };
                assert_eq!(tree, num(&v, "tree") as u32, "shape at {line}");
            }
            "set" => {
                let tree = num(&v, "tree") as u32;
                let bounds = narrow(get(&v, "bounds"));
                let polys = array(get(&v, "polygons"));
                let offset = num(&v, "offset");
                let id = if is_legacy {
                    let id = lane.exchange.set_new(tree, (bounds.len() / 6) as u32, offset);
                    unsafe {
                        std::slice::from_raw_parts_mut(lane.exchange.set_bounds(id), bounds.len())
                            .copy_from_slice(&bounds);
                        std::slice::from_raw_parts_mut(lane.exchange.set_polygons(id), polys.len())
                            .copy_from_slice(&polys)
                    };
                    id
                } else {
                    lane.context
                        .collision_fixed_set(tree, &bounds, &polys, offset)
                        .unwrap()
                };
                assert_eq!(id, num(&v, "id") as u32);
            }
            "body" => {
                let values = floats(get(&v, "v"));
                let body = Body {
                    tree: num(&v, "tree") as usize,
                    moving: yes(&v, "moving"),
                    offset: num(&v, "offset"),
                    bounds: values[..6].try_into().unwrap(),
                    matrix: values[6..22].try_into().unwrap(),
                    inverse: values[22..38].try_into().unwrap(),
                };
                let world = lane.ids.unwrap().world;
                let id = if is_legacy {
                    let id = lane.exchange.body_new(body.tree as u32, body.moving as u32);
                    assert!(id >= 0);
                    lane.exchange.body_offset(id as u32, body.offset);
                    lane.exchange.io_mut()[20..58].copy_from_slice(&values);
                    lane.exchange.body_set(id as u32);
                    id as u32
                } else {
                    lane.context.collision_body(world, body).unwrap()
                };
                assert_eq!(id, num(&v, "id") as u32);
                lane.order.push(id);
                if is_legacy {
                    order(&mut lane.exchange, world, &lane.order)
                }
            }
            "place" => {
                let values = floats(get(&v, "v"));
                let id = num(&v, "id") as u32;
                if is_legacy {
                    lane.exchange.io_mut()[20..58].copy_from_slice(&values);
                    lane.exchange.body_set(id)
                } else {
                    lane.context
                        .collision_place(
                            id,
                            values[..6].try_into().unwrap(),
                            values[6..22].try_into().unwrap(),
                            values[22..38].try_into().unwrap(),
                        )
                        .unwrap()
                }
            }
            "remove" => {
                let id = num(&v, "id") as u32;
                let at = lane.order.iter().position(|&n| n == id).unwrap();
                lane.order.remove(at);
                if is_legacy {
                    order(&mut lane.exchange, lane.ids.unwrap().world, &lane.order)
                } else {
                    assert!(lane
                        .context
                        .collision_remove(lane.ids.unwrap().world, id)
                        .unwrap())
                }
            }
            "order" => {
                lane.order = array(get(&v, "order"));
                if is_legacy {
                    order(&mut lane.exchange, lane.ids.unwrap().world, &lane.order)
                } else {
                    lane.context
                        .collision_order(lane.ids.unwrap().world, &lane.order)
                        .unwrap()
                }
            }
            "grow" => {
                if !is_legacy {
                    lane.context
                        .scratch(&vec![0; num(&v, "bytes") as usize])
                        .unwrap();
                }
            }
            "query" => {
                let flags: Vec<u8> = array(get(&v, "flags")).iter().map(|&n| n as u8).collect();
                let actual = if is_legacy {
                    legacy_query(&mut lane.exchange, &v, lane.ids.unwrap(), &flags)
                } else {
                    lane.context
                        .collision_query(query(&v, lane.ids.unwrap()), &flags)
                        .unwrap()
                };
                let expected = get(&v, "expected");
                assert_eq!(actual.code, num(expected, "code") as i32, "code at {line}");
                assert_eq!(
                    bits(&actual.io[10..17]),
                    bits(&floats(get(expected, "hit"))),
                    "hit at {line}"
                );
                assert_eq!(
                    actual.candidates,
                    array(get(expected, "candidates")),
                    "candidates at {line}"
                );
                assert_eq!(
                    actual.enabled,
                    array(get(expected, "enabled"))
                        .iter()
                        .map(|&n| n as u8)
                        .collect::<Vec<_>>(),
                    "flags at {line}"
                );
                if let Some(words) = expected.get("words") {
                    assert_eq!(
                        bits(&actual.words),
                        bits(&floats(words)),
                        "gather at {line}"
                    )
                }
                if let Some(near) = expected.get("near") {
                    assert_eq!(actual.near, array(near), "near at {line}")
                }
                if queries == 7 && !is_legacy {
                    lane.retained = Some(actual)
                }
                queries += 1;
            }
            _ => panic!("unknown operation {op}"),
        }
    }
    assert!(summary);
    assert!(queries > 1900);
    for lane in lanes {
        lane.context.dispose().unwrap();
        if let Some(saved) = lane.retained {
            assert_eq!(saved.io.len(), 64);
        }
    }
    eprintln!(
        "collision {}: {queries} exact query records",
        if legacy.is_some() {
            "isolated legacy native"
        } else {
            "owned contexts"
        }
    );
}

#[test]
fn native_collision_matches_two_wasm_instances_and_isolated_legacy_processes() {
    if let Some(lane) = std::env::var_os("BFE_COLLISION_LEGACY_LANE") {
        let trace = std::env::var_os("BFE_COLLISION_TRACE").unwrap();
        replay(
            Path::new(&trace),
            Some(lane.to_str().unwrap().parse().unwrap()),
        );
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let trace = std::env::temp_dir().join(format!("native-collision-{}.jsonl", std::process::id()));
    let status = Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
        .arg(root.join("tools/native-collision-twin.mjs"))
        .current_dir(&root)
        .stdout(Stdio::from(File::create(&trace).unwrap()))
        .status()
        .unwrap();
    assert!(status.success(), "collision oracle failed");
    replay(&trace, None);
    for lane in 0..2 {
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "native_collision_matches_two_wasm_instances_and_isolated_legacy_processes",
                "--nocapture",
            ])
            .env("BFE_COLLISION_LEGACY_LANE", lane.to_string())
            .env("BFE_COLLISION_TRACE", &trace)
            .status()
            .unwrap();
        assert!(status.success(), "legacy native lane {lane}");
    }
    fs::remove_file(trace).unwrap();
}
