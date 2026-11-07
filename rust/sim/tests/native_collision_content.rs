//! Real-map ownership and native BVH construction against the existing JS
//! collision oracle. Optional in source-only checkouts; acceptance requires
//! BFE_CONTENT_DIR and reports a nonzero all-map count.
#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
use bfe_content::{
    bvh::{build, Positions},
    collision::MapCollision,
    install::{install, read_trees},
    js::js_hypot,
};
use bfe_sim::{
    abi::context::{CollisionQuery, Context},
    collision::{CollisionOwner, Ray, MISS},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[test]
fn native_collision_owned_worlds_match_every_content_map() {
    let Some(root) = std::env::var_os("BFE_CONTENT_DIR").map(PathBuf::from) else {
        eprintln!("skipped: no BFE_CONTENT_DIR");
        return;
    };
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut maps: Vec<_> = fs::read_dir(root.join("maps"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|m| root.join("maps").join(m).join("collision.bin").exists())
        .collect();
    maps.sort();
    assert!(!maps.is_empty());
    let out = std::env::temp_dir().join(format!("owned-collision-content-{}", std::process::id()));
    let mut total = 0;
    for pair in maps.chunks(2) {
        let mut worlds = Vec::new();
        for map in pair {
            let file = out.join(format!("{map}.trees"));
            assert!(
                Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
                    .arg(repo.join("tools/collision-trees.mjs"))
                    .args(["--map", map, "--casts", "3000", "--out"])
                    .arg(&file)
                    .current_dir(&repo)
                    .status()
                    .unwrap()
                    .success()
            );
            let (expected_b, expected_t, casts) = read_trees(&fs::read(file).unwrap()).unwrap();
            let read = |name: &str| fs::read(root.join("maps").join(map).join(name)).unwrap();
            let c = MapCollision::load(
                &read("collision.bin"),
                &read("collision_index.bin"),
                &read("collision_poly.bin"),
                &read("terrain_collision.bin"),
                &read("terrain_collision_index.bin"),
            )
            .unwrap();
            let b = build(Positions::F32(&c.positions), &c.indices);
            let t = (!c.terrain_indices.is_empty())
                .then(|| build(Positions::F32(&c.terrain_positions), &c.terrain_indices));
            assert_eq!(b.roots, expected_b.roots, "{map}: building roots");
            assert_eq!(b.indirect, expected_b.indirect, "{map}: building indirect");
            assert_eq!(t.is_some(), expected_t.is_some());
            if let (Some(t), Some(e)) = (&t, expected_t) {
                assert_eq!(t.roots, e.roots, "{map}: terrain roots");
                assert_eq!(t.indirect, e.indirect, "{map}: terrain indirect")
            }
            let cx = Context::new();
            let ids = cx.collision_install(&c, &b, t.as_ref()).unwrap();
            assert_eq!(ids.world, 0);
            let mut owner = Box::new(CollisionOwner::new());
            let owned_ids = install(&mut *owner, &c, &b, t.as_ref()).unwrap();
            assert_eq!(owned_ids.world, ids.world);
            worlds.push((map, cx, ids, casts, owner));
        }
        for i in 0..worlds.iter().map(|w| w.3.len()).max().unwrap() {
            for (map, cx, ids, casts, owner) in &mut worlds {
                let Some(c) = casts.get(i) else { continue };
                let ray = Ray {
                    ox: c.origin[0],
                    oy: c.origin[1],
                    oz: c.origin[2],
                    dx: c.direction[0],
                    dy: c.direction[1],
                    dz: c.direction[2],
                };
                let result = cx
                    .collision_query(
                        CollisionQuery::Cast {
                            world: ids.world,
                            ray,
                            far: c.far,
                            hypot_xz: js_hypot(&[ray.dx, ray.dz]),
                            ignore: -1,
                            static_only: false,
                            moving_only: false,
                        },
                        &[],
                    )
                    .unwrap();
                // Exercise the same owner used by WASM veneers, with equal
                // local ids interleaved across maps and the Context reference.
                unsafe {
                    let io = std::slice::from_raw_parts_mut(owner.io_ptr(), 64);
                    io[..8].copy_from_slice(&[ray.ox, ray.oy, ray.oz, ray.dx, ray.dy, ray.dz, c.far, js_hypot(&[ray.dx, ray.dz])]);
                }
                let count = owner.world_candidates(ids.world, -1, 0, 0);
                assert_eq!(count, 0);
                assert_eq!(owner.world_cast(ids.world, count), c.code, "{map} owner cast {i}");
                if c.code != MISS {
                    let hit = unsafe { std::slice::from_raw_parts(owner.io_ptr().add(10), 7) };
                    assert!(hit.iter().zip(&c.hit).all(|(a, b)| a.to_bits() == b.to_bits()), "{map} owner hit {i}");
                }
                assert_eq!(result.code, c.code, "{map} cast {i}");
                if c.code != MISS {
                    assert_eq!(
                        result.io[10..17]
                            .iter()
                            .map(|v| v.to_bits())
                            .collect::<Vec<_>>(),
                        c.hit.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                        "{map} cast {i}"
                    )
                }
                total += 1;
            }
        }
        for (map, cx, _, casts, _) in worlds {
            cx.dispose().unwrap();
            eprintln!("owned collision {map}: {} exact casts", casts.len())
        }
    }
    fs::remove_dir_all(out).unwrap();
    assert_eq!(total, maps.len() * 3000);
    eprintln!(
        "owned collision content: {} maps, {total} exact interleaved casts",
        maps.len()
    );
}
