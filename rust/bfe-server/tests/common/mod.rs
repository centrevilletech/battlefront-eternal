//! Fixtures shared by the server's test binaries.
#![allow(dead_code)]
use serde_json::json;
use sha2::Digest;
use std::fs;
use std::path::{Path, PathBuf};

/// Synthesizes a minimal but complete prepared game content tree (`maps/nab2/map.json`,
/// `common/anims.json`, `sides/rep/units.json`, `sides/cis/units.json`) so tests can run
/// `bfe_content_native::fingerprint::hash` and `build_entity_registry_descriptor` without
/// requiring an external multi-gigabyte SWBF1 asset directory.
/// Builds a minimal valid BfE content directory (`version.json`, map manifest, mode descriptors,
/// and heightmap/asset stubs) matching the layout checked by `check_content` (`config.rs`, porting
/// `checkContent`) and loaded by `NativeWorldHost`
/// (`world_worker.rs`, porting `packages/bfe-server/src/world-worker.js`).
pub fn minimal_content_dir(dir: &Path) -> PathBuf {
    let root = dir.join("content");
    let map_dir = root.join("maps").join("nab2");
    let common_dir = root.join("common");
    let rep_actors_dir = root.join("sides").join("rep").join("actors");
    let cis_actors_dir = root.join("sides").join("cis").join("actors");
    fs::create_dir_all(&map_dir).unwrap();
    fs::create_dir_all(&common_dir).unwrap();
    fs::create_dir_all(&rep_actors_dir).unwrap();
    fs::create_dir_all(&cis_actors_dir).unwrap();

    fs::write(root.join("maps.json"), br#"[{"id":"nab2"}]"#).unwrap();

    // Minimal `nab2_cw` conquest mission definition with two teams (`rep` and `cis`) and two command posts.
    let map_json = json!({
        "name": "nab2",
        "missions": {
            "nab2_cw": {
                "mission": {
                    "name": "nab2_cw",
                    "era": "cw",
                    "sides": ["rep", "cis"],
                    "teams": {
                        "1": {
                            "name": "rep",
                            "units": 16,
                            "reinforcements": 100,
                            "classes": [
                                ["rep_inf_trooper", 4, 16]
                            ]
                        },
                        "2": {
                            "name": "cis",
                            "units": 16,
                            "reinforcements": 100,
                            "classes": [
                                ["cis_inf_battledroid", 4, 16]
                            ]
                        }
                    },
                    "posts": [
                        {
                            "name": "cp1",
                            "team": 1,
                            "position": [0.0, 0.0, 0.0]
                        },
                        {
                            "name": "cp2",
                            "team": 2,
                            "position": [20.0, 0.0, 0.0]
                        }
                    ]
                },
                "spawnScreen": {},
                "vehicles": null
            }
        }
    });
    fs::write(
        map_dir.join("map.json"),
        serde_json::to_vec(&map_json).unwrap(),
    )
    .unwrap();

    for bin_name in [
        "collision.bin",
        "collision_index.bin",
        "collision_poly.bin",
        "terrain_collision.bin",
        "terrain_collision_index.bin",
    ] {
        fs::write(map_dir.join(bin_name), b"").unwrap();
    }

    let anims_json = json!({
        "fps": 30,
        "bones": ["DummyRoot"],
        "clips": {}
    });
    fs::write(
        common_dir.join("anims.json"),
        serde_json::to_vec(&anims_json).unwrap(),
    )
    .unwrap();
    fs::write(common_dir.join("anims.bin"), b"").unwrap();
    fs::write(common_dir.join("common.json"), b"{}").unwrap();
    fs::write(common_dir.join("effects.json"), b"{}").unwrap();
    fs::write(common_dir.join("fxmodels.bin"), b"").unwrap();
    fs::write(common_dir.join("fxmodels_index.bin"), b"").unwrap();

    let actor_json = json!({
        "bones": [
            {
                "name": "DummyRoot",
                "parent": -1,
                "local": [
                    1.0, 0.0, 0.0, 0.0,
                    0.0, 1.0, 0.0, 0.0,
                    0.0, 0.0, 1.0, 0.0,
                    0.0, 0.0, 0.0, 1.0
                ]
            }
        ],
        "parts": [],
        "attach": "DummyRoot"
    });
    let actor_bytes = serde_json::to_vec(&actor_json).unwrap();

    for (side_name, class_name, actors_dir) in [
        ("rep", "rep_inf_trooper", &rep_actors_dir),
        ("cis", "cis_inf_battledroid", &cis_actors_dir),
    ] {
        let side_dir = root.join("sides").join(side_name);
        let units_json = json!({
            "units": {
                class_name: {
                    "base": "soldier",
                    "model": "trooper",
                    "properties": {
                        "MaxHealth": "300.0",
                        "MaxSpeed": "6.5",
                        "MaxStrafeSpeed": "4.5"
                    },
                    "weapons": []
                }
            },
            "items": {}
        });
        fs::write(
            side_dir.join("units.json"),
            serde_json::to_vec(&units_json).unwrap(),
        )
        .unwrap();
        let weapons_json = json!({
            "weapons": {},
            "ordnance": {},
            "models": {},
            "firePoints": {}
        });
        fs::write(
            side_dir.join("weapons.json"),
            serde_json::to_vec(&weapons_json).unwrap(),
        )
        .unwrap();
        fs::write(side_dir.join("weapons.bin"), b"").unwrap();
        fs::write(side_dir.join("weapons_index.bin"), b"").unwrap();
        fs::write(actors_dir.join("trooper.json"), &actor_bytes).unwrap();
        fs::write(actors_dir.join("trooper.bin"), b"").unwrap();
        fs::write(actors_dir.join("trooper_index.bin"), b"").unwrap();
    }

    root
}

/// An invented 2005 battle, prepared as the 2005 content layout has it: map
/// `yard` on a 2 km square of ground, one mission `yard_duel` (mode `duel`,
/// era `cw`), and one side with two invented soldier classes on a one-bone
/// rig. The side folder is `rep`, which the 2005 loader puts on team 1 (and,
/// alone, on team 2 as well). Both classes carry their movement properties,
/// and the common human bank holds their forward roll clip, 37 frames long.
pub fn invented_battle(dir: &Path) {
    invented_battle_rolling(dir, 37, true);
}

/// [`invented_battle`] with a roll clip of `frames` frames, and with or
/// without the classes' movement properties.
pub fn invented_battle_rolling(dir: &Path, frames: usize, properties: bool) {
    let missions = r#"{"yard_duel":{"mode":"duel","era":"cw","layers":[],"sides":{"rep":[]}}}"#;
    invented_yard(dir, frames, properties, missions, &[("rep", &["blue_trooper", "gold_scout"])]);
}

/// The invented yard with the scripted missions `eras` names (mode `eli`),
/// each running the belfry script (`tests/fixtures/mission-scripts/belfry.lua`):
/// two sides of one class each, `rep_winder` and `cis_winder`.
pub fn invented_mission(dir: &Path, eras: &[&str]) {
    let belfry = include_bytes!("../../../../tests/fixtures/mission-scripts/belfry.luac");
    let mut missions = serde_json::Map::new();
    for era in eras {
        missions.insert(format!("belfry_{era}"), mission_script(dir, era, belfry));
    }
    let missions = serde_json::to_string(&missions).unwrap();
    invented_yard(dir, 37, true, &missions, &[("rep", &["rep_winder"]), ("cis", &["cis_winder"])]);
}

/// [`invented_mission`]'s mission of `era` with another compiled script.
pub fn replace_mission_script(dir: &Path, era: &str, body: &[u8]) {
    let missions = serde_json::to_string(&json!({format!("belfry_{era}"): mission_script(dir, era, body)})).unwrap();
    invented_yard(dir, 37, true, &missions, &[("rep", &["rep_winder"]), ("cis", &["cis_winder"])]);
}

/// Writes one mission's script and its manifest, and gives the map's record
/// of it. The mission's name is the name of the script it starts with.
fn mission_script(dir: &Path, era: &str, body: &[u8]) -> serde_json::Value {
    let name = format!("belfry_{era}");
    let sha = format!("{:x}", sha2::Sha256::digest(body));
    let file = format!("scripts/{sha}.luac");
    let manifest = format!("maps/yard/scripts/{era}-eli.json");
    let scripts = json!([{"id": name, "name": name, "archive": "belfry.lvl", "file": file, "length": body.len(), "sha256": sha}]);
    let put = |path: &str, bytes: &[u8]| {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    };
    put(&file, body);
    put(&manifest, &serde_json::to_vec(&json!({"version": 1, "family": "swbf2", "map": "yard", "era": era, "mode": "eli",
        "entry": name, "chunks": [name], "scripts": scripts})).unwrap());
    json!({"mode": "eli", "era": era, "layers": [], "sides": {"rep": [], "cis": []}, "scriptManifest": manifest, "scripts": scripts})
}

fn invented_yard(dir: &Path, frames: usize, properties: bool, missions: &str, sides: &[(&str, &[&str])]) {
    const IDENTITY: &str = "[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]";
    let floats = |values: &[f32]| values.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
    let words = |values: &[u32]| values.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
    let put = |path: &str, bytes: Vec<u8>| {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    };
    put("common/common.json", br#"{"family":"swbf2"}"#.to_vec());
    put("maps/yard/map.json", format!(r#"{{"family":"swbf2","name":"yard",
        "bounds":{{"min":[-1000,-100,-1000],"max":[1000,400,1000]}},
        "wireBounds":{{"min":[-1000,-100,-1000],"max":[1000,400,1000]}},
        "meshes":[],"lights":[],"regions":[],"deathRegions":[],"paths":[],
        "spawns":{{"yard_gate":[[0,1,0,0]]}},
        "commandPosts":[{{"name":"gate","team":1,"spawnPath":"yard_gate","matrix":{IDENTITY}}},
            {{"name":"far_gate","team":2,"spawnPath":"yard_gate","matrix":{IDENTITY}}}],
        "missions":{missions},
        "layers":{{}}}}"#).into_bytes());
    // One small block of geometry far off, and the ground.
    put("maps/yard/collision.bin", floats(&[500., 0., 500., 502., 0., 500., 501., 2., 502.]));
    put("maps/yard/collision_index.bin", words(&[0, 1, 2]));
    put("maps/yard/collision_poly.bin", words(&[0]));
    put("maps/yard/terrain_collision.bin", floats(&[-1000., 0., -1000., 1000., 0., -1000., 1000., 0., 1000., -1000., 0., 1000.]));
    put("maps/yard/terrain_collision_index.bin", words(&[0, 2, 1, 0, 3, 2]));
    let part = r#"{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3}"#;
    // Invented movement properties, as the 2005 preparation writes a class's
    // inherited properties: one row each, ControlSpeed once per mode.
    let movement = r#"["ControlSpeed","stand 1.0 0.9 1.0"],["ControlSpeed","crouch 0.6 0.5 0.9"],["ControlSpeed","sprint 1.6 0.45 0.6"],
        ["ControlSpeed","jump 0.05 0.05 0.3"],["ControlSpeed","jet 0.5 0.5 0.5"],["ControlSpeed","roll 0.0 0.0 0.25"],
        ["ControlSpeed","tumble 0.0 0.0 0.1"],["SprintAccelerateTime","0.3"],["EnergyBar","50"],["EnergyRestore","5"],
        ["EnergyRestoreIdle","7.5"],["EnergyDrainSprint","20"],["EnergyMinSprint","10"],["EnergyCostJump","1.5"],
        ["EnergyCostRoll","10"],["JumpHeight","1.6"],["JumpForwardSpeedFactor","1.2"],["JumpStrafeSpeedFactor","0.8"],
        ["RollSpeedFactor","1.8"]"#;
    let properties = if properties { movement } else { r#"["MaxHealth","300"]"# };
    let unit = |class: &&str| format!(r#""{class}":{{"class":"{class}","base":"soldier","model":"post_rig","actor":"actors/post_rig.json","properties":[{properties}],"weapons":[]}}"#);
    let mut rig = Vec::new();
    for x in [-1f32, 0., 1.] {
        let mut row = [0f32; 9];
        row[0] = x;
        rig.extend(floats(&row));
    }
    for (side, classes) in sides {
        let units = classes.iter().map(unit).collect::<Vec<_>>().join(",");
        put(&format!("sides/{side}/units.json"), format!(r#"{{"units":{{{units}}}}}"#).into_bytes());
        put(&format!("sides/{side}/actors/post_rig.json"), format!(r#"{{"bones":[{{"name":"DummyRoot","parent":-1,"local":{IDENTITY}}}],"parts":[{part}]}}"#).into_bytes());
        put(&format!("sides/{side}/actors/post_rig.bin"), rig.clone());
        put(&format!("sides/{side}/actors/post_rig_index.bin"), words(&[0, 1, 2]));
    }
    // The common human bank: one forward roll clip on the rig's one bone.
    let crc = bfe_content::zero_hash::crc_hex;
    put("common/anim_banks.json", format!(r#"{{"fps":30,"banks":{{"human_1":{{"clipCount":1,"clips":[{{"crc":"{}","flags":0,
        "frames":{frames},"bones":1,"offset":0,"tracks":["{}"]}}]}}}}}}"#, crc("human_rifle_diveforward"), crc("DummyRoot")).into_bytes());
    put("common/anim_banks.bin", floats(&vec![0.; frames * 7]));
    // What a client's 2005 loader opens beside the battle's data: no world
    // mesh, and an empty weapon pack and bank for each side.
    put("maps/yard/geom.bin", Vec::new());
    put("maps/yard/index.bin", Vec::new());
    for (side, _) in sides {
        put(&format!("sides/{side}/weapons.json"), br#"{"models":{},"firePoints":{}}"#.to_vec());
        put(&format!("sides/{side}/weapons.bin"), Vec::new());
        put(&format!("sides/{side}/weapons_index.bin"), Vec::new());
    }
}
