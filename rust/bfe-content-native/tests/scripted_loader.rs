//! A scripted mission's display content is the ordinary 2005 load with the mission's own object:
//! on an invented map the placed objects keep the vertex light and tint their parts carry.
use bfe_content::GameFamily;
use bfe_content_native::{placed, Loader};
use serde_json::json;
use std::{fs, path::PathBuf};

struct Yard(PathBuf);

impl Drop for Yard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A prepared 2005 folder: one map with one door of three parts, a mission with one soldier class.
fn yard(name: &str) -> Yard {
    let root = std::env::temp_dir().join(format!("bfe-scripted-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let map = root.join("maps/yard");
    for dir in ["maps/yard", "common", "sides/alpha/actors", "sides/beta"] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    let write = |path: &str, value: serde_json::Value| fs::write(root.join(path), value.to_string()).unwrap();
    write("maps/yard/map.json", json!({
        "meshes": [], "textures": {}, "family": "swbf2",
        "bounds": {"min": [-2, -2, -2], "max": [2, 2, 2]},
        "missions": {"yard_duel": {"sides": {"alpha": ["alpha_soldier"], "beta": []}}},
        "spawnPoints": []
    }));
    let pose = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 2, 4, 6, 1];
    let parts: Vec<_> = (0..3)
        .map(|i| {
            let mut part = json!({"flags": 1, "vertexOffset": i * 96, "indexOffset": i * 12,
                "vertexCount": 3, "indexCount": 3, "material": {"swbf2Flags": 512}});
            if i != 2 {
                part["vertexStreams"] = json!({"lit": 100 - 96 * i, "tint": 148 - 96 * i});
            }
            part
        })
        .collect();
    write("maps/yard/props.json", json!({"attributesFile": "colours.bin",
        "models": {"tile": {"bones": [], "parts": parts}},
        "instances": [{"model": "tile", "base": "door", "matrix": pose}]}));
    write("common/anims.json", json!({"sets": {}, "clips": {}}));
    write("common/common.json", json!({"textures": {}}));
    write("common/effects.json", json!({"effects": {}, "models": {}}));
    for (side, units) in [("alpha", json!({"alpha_soldier": {"base": "soldier", "model": "alpha_body",
        "actor": "actors/alpha_body.json", "weapons": []}})), ("beta", json!({}))] {
        write(&format!("sides/{side}/units.json"), json!({"units": units, "textures": {}}));
        write(&format!("sides/{side}/weapons.json"), json!({"weapons": {}, "models": {}, "firePoints": {}}));
    }
    write("sides/alpha/actors/alpha_body.json", json!({"bones": [], "parts": []}));
    for bin in ["maps/yard/geom.bin", "maps/yard/index.bin", "maps/yard/collision.bin",
        "maps/yard/collision_index.bin", "maps/yard/collision_poly.bin", "maps/yard/terrain_collision.bin",
        "maps/yard/terrain_collision_index.bin", "common/anims.bin", "common/fxmodels.bin",
        "common/fxmodels_index.bin", "sides/alpha/weapons.bin", "sides/alpha/weapons_index.bin",
        "sides/alpha/actors/alpha_body.bin", "sides/alpha/actors/alpha_body_index.bin",
        "sides/beta/weapons.bin", "sides/beta/weapons_index.bin"] {
        fs::write(root.join(bin), []).unwrap();
    }
    let positions: Vec<u8> = (0..72).flat_map(|i| (i as f32 / 9.).to_le_bytes()).collect();
    fs::write(map.join("props.bin"), positions).unwrap();
    fs::write(map.join("props_index.bin"), (0u32..9).flat_map(u32::to_le_bytes).collect::<Vec<_>>()).unwrap();
    let colours: Vec<u8> = (0..49).flat_map(|i| (i as f32 / 64.).to_le_bytes()).collect();
    fs::write(map.join("colours.bin"), colours).unwrap();
    Yard(root)
}

#[test]
fn a_scripted_mission_keeps_the_placed_colours_of_the_ordinary_load() {
    let yard = yard("colours");
    let loader = Loader::from_resolved_root(&yard.0, GameFamily::Swbf2);
    let query = "?mission=yard_duel&side=alpha&unit=alpha_soldier";
    let mission = json!({"sides": ["alpha", "beta"], "teams": {"1": {"name": "alpha", "classes": [["alpha_soldier", 1]]}}});
    let ordinary = loader.load_for_family(GameFamily::Swbf2, "yard", query).unwrap();
    let scripted = loader.load_scripted_mission("yard", query, &mission.to_string()).unwrap();
    assert!(scripted.content.battle.world.get("mission").and_then(|m| m.get("sides")).is_some());
    let placed = |content| placed::admit(content).unwrap().parts;
    let (a, b) = (placed(&ordinary), placed(&scripted));
    assert_eq!(a.len(), b.len());
    let colours = |parts: &[placed::Part]| {
        parts.iter().map(|p| p.colours.as_ref().map(|c| (c.light.clone(), c.tint.clone()))).collect::<Vec<_>>()
    };
    let seen = colours(&b);
    assert!(seen.iter().any(|c| c.as_ref().is_some_and(|(light, tint)| !light.is_empty() && !tint.is_empty())),
        "the placed parts carry their own light and tint");
    assert_eq!(colours(&a), seen);
    // A mission for another class is refused: the scene must match the authority's soldier.
    let other = json!({"sides": ["alpha", "beta"], "teams": {"1": {"name": "alpha", "classes": [["beta_soldier", 1]]}}});
    let refused = loader.load_scripted_mission("yard", "?mission=yard_duel&side=alpha", &other.to_string());
    assert!(refused.is_err());
}
