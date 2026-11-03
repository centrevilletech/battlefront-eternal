//! The collision world made from this crate's content in the ray kernel
//! (src/install.rs, with the JavaScript's trees) against the JavaScript's
//! CollisionWorld, on the game's own files: tools/collision-trees.mjs writes
//! each map's trees and a few thousand casts its CollisionWorld answers, and
//! the installed world must answer every one the same, bit for bit. Without
//! the files (BFE_CONTENT_DIR) it passes having checked nothing.
//!
//!   cargo test -p content --features rays --test collision
//!
//! BFE_NODE names the Node to run; BFE_CONTENT_MAP limits it to one map.
#![cfg(feature = "rays")]

use bfe_content::collision::MapCollision;
use bfe_content::install::{install, read_trees};
use bfe_content::js::js_hypot;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn installed_world_casts_as_the_javascript() {
    let Some(root) = std::env::var_os("BFE_CONTENT_DIR").map(PathBuf::from).filter(|r| r.join("maps").is_dir()) else {
        eprintln!("skipped: no game files (BFE_CONTENT_DIR)");
        return;
    };
    let only_map = std::env::var("BFE_CONTENT_MAP").ok();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let node = std::env::var("BFE_NODE").unwrap_or_else(|_| "node".into());
    let out = std::env::temp_dir().join(format!("collision-trees-test-{}", std::process::id()));
    let mut maps: Vec<String> = std::fs::read_dir(root.join("maps"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|m| root.join("maps").join(m).join("collision.bin").exists() && only_map.as_ref().is_none_or(|o| o == m))
        .collect();
    maps.sort();
    assert!(!maps.is_empty(), "no maps in the content");
    let mut differing = Vec::new();
    let mut total = 0;
    for map in &maps {
        let file = out.join(format!("{map}.trees"));
        let status = Command::new(&node)
            .arg(repo.join("tools/collision-trees.mjs"))
            .args(["--map", map, "--casts", "3000", "--out"])
            .arg(&file)
            .env("BFE_CONTENT_DIR", &root)
            .current_dir(&repo)
            .status()
            .expect("node runs tools/collision-trees.mjs");
        assert!(status.success(), "tools/collision-trees.mjs failed on {map}");
        let (buildings, terrain, casts) = read_trees(&std::fs::read(&file).unwrap()).unwrap();
        // The map's collision as this crate loads it (game.js 429-436).
        let read = |name: &str| std::fs::read(root.join("maps").join(map).join(name)).unwrap();
        let collision = MapCollision::load(
            &read("collision.bin"),
            &read("collision_index.bin"),
            &read("collision_poly.bin"),
            &read("terrain_collision.bin"),
            &read("terrain_collision_index.bin"),
        )
        .unwrap();
        let ids = install(&collision, &buildings, terrain.as_ref()).unwrap();
        for (i, cast) in casts.iter().enumerate() {
            let ray = rays::Ray { ox: cast.origin[0], oy: cast.origin[1], oz: cast.origin[2], dx: cast.direction[0], dy: cast.direction[1], dz: cast.direction[2] };
            let hypot = js_hypot(&[cast.direction[0], cast.direction[2]]);
            let (code, hit) = rays::world_cast_ray(rays::state(), ids.world as usize, &ray, cast.far, hypot, 0);
            let got = [hit.distance, hit.px, hit.py, hit.pz, hit.nx, hit.ny, hit.nz];
            let same = code == cast.code && (code == rays::MISS || got.iter().zip(cast.hit.iter()).all(|(a, b)| a.to_bits() == b.to_bits()));
            if !same {
                differing.push(format!("{map} cast {i}: rust {code} {got:?}, js {} {:?}", cast.code, cast.hit));
            }
        }
        let hits = casts.iter().filter(|c| c.code != rays::MISS).count();
        let terrain_hits = casts.iter().filter(|c| c.code == rays::TERRAIN).count();
        eprintln!("{map}: {} casts ({hits} hits, {terrain_hits} on the terrain)", casts.len());
        total += casts.len();
    }
    let _ = std::fs::remove_dir_all(&out);
    assert!(differing.is_empty(), "{} casts differ:\n{}", differing.len(), differing.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
    eprintln!("{total} casts on {} maps identical", maps.len());
}
