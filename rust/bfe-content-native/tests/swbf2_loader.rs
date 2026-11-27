//! Verify `Loader::new(root, GameFamily::Swbf2).load(...)` resolves
//! the 2005 family directory once (never double-nesting `swbf2/swbf2`).

use std::fs;
use std::path::PathBuf;

use bfe_content::layout::family_dir;
use bfe_content_native::loader::Loader;
use bfe_family::GameFamily;

#[test]
fn loader_resolves_swbf2_once_without_double_nesting() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bfe-native-loader-swbf2-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);

    // Populate a single-resolved 2005 content tree with no symlink and no double nesting.
    let bf2 = family_dir(&root, GameFamily::Swbf2);
    fs::create_dir_all(bf2.join("maps/testmap1")).unwrap();
    fs::create_dir_all(bf2.join("common")).unwrap();
    fs::create_dir_all(bf2.join("sides/alpha/actors")).unwrap();
    fs::create_dir_all(bf2.join("sides/beta")).unwrap();

    fs::write(
        bf2.join("maps/testmap1/map.json"),
        r#"{
            "meshes": [],
            "textures": {},
            "mission": {
                "teams": [
                    { "side": "alpha", "classes": ["alpha_soldier"] },
                    { "side": "beta", "classes": ["beta_soldier"] }
                ]
            },
            "spawnPoints": []
        }"#,
    )
    .unwrap();
    for bin in [
        "maps/testmap1/geom.bin",
        "maps/testmap1/index.bin",
        "maps/testmap1/collision.bin",
        "maps/testmap1/collision_index.bin",
        "maps/testmap1/collision_poly.bin",
        "maps/testmap1/terrain_collision.bin",
        "maps/testmap1/terrain_collision_index.bin",
        "common/anims.bin",
        "common/fxmodels.bin",
        "common/fxmodels_index.bin",
        "sides/alpha/weapons.bin",
        "sides/alpha/weapons_index.bin",
        "sides/alpha/actors/alpha_body.bin",
        "sides/alpha/actors/alpha_body_index.bin",
        "sides/beta/weapons.bin",
        "sides/beta/weapons_index.bin",
    ] {
        fs::write(bf2.join(bin), []).unwrap();
    }
    fs::write(bf2.join("common/anims.json"), r#"{"sets":{},"clips":{}}"#).unwrap();
    fs::write(bf2.join("common/common.json"), r#"{"textures":{}}"#).unwrap();
    fs::write(bf2.join("common/effects.json"), r#"{"effects":{},"models":{}}"#).unwrap();
    fs::write(
        bf2.join("sides/alpha/units.json"),
        r#"{"units":{"alpha_soldier":{"model":"alpha_body","weapons":[]}},"textures":{}}"#,
    )
    .unwrap();
    fs::write(
        bf2.join("sides/alpha/weapons.json"),
        r#"{"weapons":{},"models":{},"firePoints":{}}"#,
    )
    .unwrap();
    fs::write(
        bf2.join("sides/alpha/actors/alpha_body.json"),
        r#"{"bones":[],"parts":[]}"#,
    )
    .unwrap();
    fs::write(
        bf2.join("sides/beta/units.json"),
        r#"{"units":{},"textures":{}}"#,
    )
    .unwrap();
    fs::write(
        bf2.join("sides/beta/weapons.json"),
        r#"{"weapons":{},"models":{},"firePoints":{}}"#,
    )
    .unwrap();

    // Ensure there is no double-nested path.
    assert!(!family_dir(&bf2, GameFamily::Swbf2).exists());

    let loader = Loader::new(PathBuf::from(&root), GameFamily::Swbf2);
    let result = loader.load("testmap1", "?map=testmap1&side=alpha&unit=alpha_soldier");
    let _ = fs::remove_dir_all(&root);

    let loaded = result.expect("Loader::new(root, Swbf2).load must read single-nested family_dir");
    assert_eq!(loaded.content.battle.map, "testmap1");
}
