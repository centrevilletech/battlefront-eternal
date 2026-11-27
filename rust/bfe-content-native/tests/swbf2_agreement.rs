//! Agreement for an invented ground mission with two allied rosters.
use bfe_content::{
    battle::{Battle, Loader},
    files::MemFiles,
    GameFamily, Value,
};
use bfe_content_native::fingerprint::hash_for_family;
use bfe_sim::{
    abi::context::Context,
    world::admit::{Bots, Options},
};

const QUERY: &str = "?map=court&mission=match&side=all";
const IDENTITY: &str = "[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]";
const INFANTRY: &str = r#"[
    ["ControlSpeed","stand 3 2 1"],["ControlSpeed","crouch 2 1 1"],
    ["ControlSpeed","jump 2 1 1"],["ControlSpeed","jet 1 1 1"],
    ["ControlSpeed","sprint 5 2 1"],["ControlSpeed","roll 3 1 1"],
    ["ControlSpeed","tumble 1 2 1"],["SprintAccelerateTime","1"],
    ["EnergyBar","12"],["EnergyRestore","1"],["EnergyRestoreIdle","2"],
    ["EnergyDrainSprint","2"],["EnergyMinSprint","2"],["EnergyCostJump","1"],
    ["EnergyCostRoll","2"],["JumpHeight","1"],["JumpForwardSpeedFactor","1"],
    ["JumpStrafeSpeedFactor","1"],["RollSpeedFactor","1"]
]"#;

fn mesh(stride: usize, extent: f32, uv: f32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for x in [-extent, 0.0, extent] {
        let mut row = vec![0.0f32; stride];
        row[0] = x;
        row[6] = uv;
        bytes.extend(row.iter().flat_map(|n| n.to_le_bytes()));
    }
    bytes
}

fn court(sides: &str) -> MemFiles {
    court_with_second_side(sides, "rep")
}

fn court_with_second_side(sides: &str, second_side: &str) -> MemFiles {
    let mut files = MemFiles::default();
    files.insert("common/common.json", b"{}".to_vec());
    let bounds = r#"{"min":[-20,-10,-20],"max":[20,10,20]}"#;
    files.insert(
        "maps/court/map.json",
        format!(
            r#"{{
        "family":"swbf2","bounds":{bounds},"wireBounds":{bounds},
        "spawns":{{"yard":[[0,1,0,0]]}},
        "commandPosts":[{{"name":"gate","team":1,"spawnPath":"yard","matrix":{IDENTITY}}}],
        "missions":{{"match":{{"layers":[],"sides":{{{sides}}}}}}}
    }}"#
        )
        .into_bytes(),
    );
    let triangle: Vec<_> = [0u32, 1, 2]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    files.insert(
        "maps/court/collision.bin",
        [-10f32, 0., -10., 10., 0., -10., 0., 0., 10.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect(),
    );
    files.insert("maps/court/collision_index.bin", triangle.clone());
    files.insert("maps/court/collision_poly.bin", vec![0; 4]);
    let part = r#"{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3}"#;
    for (side, count) in [("all", 5), (second_side, 4)] {
        let dir = format!("sides/{side}");
        let units = (0..count)
            .map(|i| format!(r#""{side}_{i}":{{"class":"{side}_{i}","base":"soldier","model":"sivlet","actor":"actors/sivlet.json","properties":{INFANTRY},"weapons":[]}}"#))
            .collect::<Vec<_>>()
            .join(",");
        files.insert(&format!("{dir}/units.json"), format!(r#"{{"units":{{{units}}},"items":{{"pebble":{{"class":"pebble","base":"powerupitem","GeometryName":"stone"}}}}}}"#).into_bytes());
        files.insert(&format!("{dir}/actors/sivlet.json"), format!(r#"{{"bones":[{{"name":"DummyRoot","parent":-1,"local":{IDENTITY}}}],"parts":[{part}]}}"#).into_bytes());
        files.insert(&format!("{dir}/actors/sivlet.bin"), mesh(9, 1., 0.));
        files.insert(&format!("{dir}/actors/sivlet_index.bin"), triangle.clone());
        files.insert(
            &format!("{dir}/weapons.json"),
            format!(r#"{{"models":{{"stone":[{part}]}}}}"#).into_bytes(),
        );
        files.insert(&format!("{dir}/weapons.bin"), mesh(8, 1., 0.));
        files.insert(&format!("{dir}/weapons_index.bin"), triangle.clone());
    }
    files
}

fn hash(files: &MemFiles) -> String {
    hash_for_family(files, GameFamily::Swbf2, QUERY).unwrap()
}

#[test]
fn practice_classes_follow_authored_slots_in_admission_and_agreement() {
    let files = court_with_second_side(r#""imp":["imp_2","imp_0"],"all":["all_2","all_0"]"#, "imp");
    let swapped = court_with_second_side(r#""imp":["imp_0","imp_2"],"all":["all_2","all_0"]"#, "imp");
    let query = format!("{QUERY}&unit=all_4&practiceTarget=imp_3");
    let battle =
        Battle::load_for_family(&Loader::new(&files), GameFamily::Swbf2, "court", &query).unwrap();
    let roster = &battle.swbf2.as_ref().unwrap().roster;
    let classes: Vec<_> = roster
        .classes
        .iter()
        .map(|slot| slot.class.as_str())
        .collect();
    assert_eq!(
        classes,
        ["imp_2", "imp_0", "imp_3", "all_2", "all_0", "all_4"]
    );
    let agreement = hash_for_family(&files, GameFamily::Swbf2, &query).unwrap();
    assert_ne!(
        agreement,
        hash_for_family(&swapped, GameFamily::Swbf2, &query).unwrap()
    );
    assert_ne!(agreement, hash(&files));
    let other_target = format!("{QUERY}&unit=all_4&practiceTarget=imp_2");
    assert_ne!(
        agreement,
        hash_for_family(&files, GameFamily::Swbf2, &other_target).unwrap()
    );
}

#[test]
fn agreement_preserves_authored_class_slots_quotas_and_units() {
    let observe = |files: &MemFiles| {
        let battle =
            Battle::load_for_family(&Loader::new(files), GameFamily::Swbf2, "court", QUERY)
                .unwrap();
        let classes = battle
            .mission()
            .unwrap()
            .get("teams")
            .unwrap()
            .get("1")
            .unwrap()
            .get("classes")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|entry| {
                let pair = entry.as_array().unwrap();
                (
                    pair[0].as_str().unwrap().to_owned(),
                    pair[1].as_f64().unwrap() as u32,
                )
            })
            .collect::<Vec<_>>();
        // The admission reads each soldier class's movement numbers from its
        // properties.
        let ctx = Context::new();
        let opts = Options {
            family: GameFamily::Swbf2,
            mode: "match".into(),
            practice_target: None,
            map: "court".into(),
            side: "all".into(),
            unit: None,
            name: None,
            seed: 17,
            bots: Bots::default(),
            enhanced: false,
            heroes: false,
            ..Default::default()
        };
        ctx.world_admit_for_family(files, GameFamily::Swbf2, &opts, Some("match"), true)
            .unwrap();
        let units = ctx.world_unit_states().unwrap();
        ctx.dispose().unwrap();
        (classes, units)
    };
    let first = court(r#""all":[],"rep":[]"#);
    let reformatted = court("  \"all\" : [],\r\n  \"rep\": [ ]");
    let reordered = court(r#""rep":[],"all":[]"#);
    let a = observe(&first);
    assert_eq!(a.0.len(), 9);
    assert!(a.0.iter().any(|(_, q)| *q == 1) && a.0.iter().any(|(_, q)| *q == 2));
    assert!(!a.1.is_empty());
    assert_eq!(hash(&first), hash(&reformatted));
    assert_eq!(a, observe(&reformatted));
    let b = observe(&reordered);
    assert_ne!(a.0[0].0, b.0[0].0);
    assert_ne!(
        a.0.iter().find(|e| e.0 == "all_4"),
        b.0.iter().find(|e| e.0 == "all_4")
    );
    assert_ne!(hash(&first), hash(&reordered));
}

#[test]
fn spawn_fallback_uses_order_only_when_no_post_selects_a_path() {
    let fingerprint = |spawns: &str, posts: &str| {
        let mut files = court(r#""all":[],"rep":[]"#);
        files.insert(
            "maps/court/map.json",
            format!(
                r#"{{"spawns":{{{spawns}}},"commandPosts":[{posts}],
            "missions":{{"match":{{"sides":{{"all":[],"rep":[]}}}}}}}}"#
            )
            .into_bytes(),
        );
        hash(&files)
    };
    let west = r#""west":[[1,0,0,0]],"east":[[2,0,0,0]]"#;
    let east = r#""east":[[2.0,0,0,0]],"west":[[1e0,0,0,0]]"#;
    assert_ne!(fingerprint(west, ""), fingerprint(east, ""));
    let post = r#"{"spawnPath":"east"}"#;
    assert_eq!(fingerprint(west, post), fingerprint(east, post));
}
#[test]
fn meshes_agree_on_uvs_and_distinguish_gameplay_geometry() {
    let sides = r#""all":[],"rep":[]"#;
    let original = hash(&court(sides));
    for (path, stride) in [
        ("sides/all/actors/sivlet.bin", 9),
        ("sides/all/weapons.bin", 8),
    ] {
        let mut files = court(sides);
        files.insert(path, mesh(stride, 1., 0.375));
        assert_eq!(original, hash(&files), "UV in {path}");
        files.insert(path, mesh(stride, 2., 0.));
        assert_ne!(original, hash(&files), "gameplay extent in {path}");
    }
}

#[test]
fn an_item_model_can_come_from_either_playable_side() {
    let mut files = court(r#""all":[],"rep":[]"#);
    let other_seat = "?map=court&mission=match&side=rep";
    assert_eq!(
        hash(&files),
        hash_for_family(&files, GameFamily::Swbf2, other_seat).unwrap()
    );
    let before = hash(&files);
    files.insert("sides/rep/weapons.bin", mesh(8, 3., 0.));
    assert_ne!(hash(&files), before);
    assert_eq!(
        hash(&files),
        hash_for_family(&files, GameFamily::Swbf2, other_seat).unwrap()
    );
}

#[test]
fn only_a_gun_uses_the_height_of_an_actor_vertex() {
    let mut files = court(r#""all":[],"rep":[]"#);
    let path = "sides/all/actors/sivlet.bin";
    let original = mesh(9, 1., 0.);
    let mut raised = original.clone();
    raised[76..80].copy_from_slice(&3f32.to_le_bytes());
    let flat = hash(&files);
    files.insert(path, raised.clone());
    assert_eq!(hash(&files), flat);
    let mut units: serde_json::Value =
        serde_json::from_slice(&files.files["sides/all/units.json"]).unwrap();
    units["units"]["hinged"] = serde_json::json!({"base":"walkerdroid","model":"sivlet","actor":"actors/sivlet.json",
        "weapons":[{"aimers":["pivot"]}]});
    files.insert("sides/all/units.json", serde_json::to_vec(&units).unwrap());
    let meta = "sides/all/actors/sivlet.json";
    let mut actor: serde_json::Value = serde_json::from_slice(&files.files[meta]).unwrap();
    actor["bones"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"name":"pivot","parent":0,
        "local":serde_json::from_str::<serde_json::Value>(IDENTITY).unwrap()}));
    files.insert(meta, serde_json::to_vec(&actor).unwrap());
    let high_tip = hash(&files);
    files.insert(path, original);
    assert_ne!(hash(&files), high_tip);
}
#[test]
fn multibyte_animation_bank_names_load() {
    let mut files = court(r#""all":[],"rep":[]"#);
    let path = "sides/all/units.json";
    let text = std::str::from_utf8(&files.files[path]).unwrap().replace(
        r#""class":"all_0""#,
        r#""class":"all_0","AnimationName":"bleuéx""#,
    );
    files.insert(path, text.into_bytes());
    files.insert(
        "sides/all/anim_banks.json",
        r#"{"banks":{"bleuéx":{"clips":[]}}}"#.as_bytes().to_vec(),
    );
    files.insert("sides/all/anim_banks.bin", Vec::new());
    let battle =
        Battle::load_for_family(&Loader::new(&files), GameFamily::Swbf2, "court", QUERY).unwrap();
    assert!(battle
        .roster_report()
        .unwrap()
        .classes
        .iter()
        .any(|c| c.anim_banks.iter().any(|b| b == "bleuéx")));
}

#[test]
fn only_resolved_animation_alias_order_affects_agreement() {
    let fingerprint = |timing: &str, fallback: &str| {
        let mut files = court(r#""all":[],"rep":[]"#);
        files.insert("sides/all/anim_banks.json", br#"{"banks":{"reel":{"clips":[
            {"crc":"10","frames":2,"offset":0,"tracks":[]},{"crc":"20","frames":2,"offset":0,"tracks":[]}]}}}"#.to_vec());
        files.insert("sides/all/anim_banks.bin", Vec::new());
        let path = "sides/all/units.json";
        let source = std::str::from_utf8(&files.files[path]).unwrap().replacen(
            r#""weapons":[]"#,
            &format!(r#""weapons":[{{"comboBindings":[{{"bank":"reel","clipTiming":{{{timing}}},"clips":{{{fallback}}}}}]}}]"#), 1,
        );
        files.insert(path, source.into_bytes());
        hash(&files)
    };
    let one_first = r#""one":{"crc":"10"},"two":{"crc":"10"}"#;
    let two_first = r#""two":{"crc":"10"},"one":{"crc":"10"}"#;
    assert_eq!(
        fingerprint(one_first, r#""one":[],"two":[]"#),
        fingerprint(one_first, r#""two":[],"one":[]"#)
    );
    assert_ne!(fingerprint(one_first, ""), fingerprint(two_first, ""));
    assert_eq!(
        fingerprint(r#""one":{"crc":"10"},"two":{"crc":"20"}"#, ""),
        fingerprint(r#""two":{"crc":"20"},"one":{"crc":"10"}"#, "")
    );
}

#[test]
fn a_folded_package_uses_its_first_matching_class() {
    let select = |packages: &str, classes: &str| {
        let mut files = court(&format!(r#""all":[{packages}]"#));
        files.insert(
            "sides/all/units.json",
            format!(r#"{{"units":{{{classes}}}}}"#).into_bytes(),
        );
        hash(&files)
    };
    let lower = r#""sivlet":{},"SIVLET":{}"#;
    let upper = r#""SIVLET":{},"sivlet":{}"#;
    assert_ne!(select(r#""Sivlet""#, lower), select(r#""Sivlet""#, upper));
    assert_eq!(select(r#""sivlet""#, lower), select(r#""sivlet""#, upper));
    assert_eq!(select("", lower), select("", upper));
}

#[test]
fn the_resolved_dispenser_item_participates_in_agreement() {
    let fingerprint = |fields: &str| {
        let mut files = court(r#""all":[],"rep":[]"#);
        let path = "sides/all/units.json";
        let source = std::str::from_utf8(&files.files[path]).unwrap().replacen(
            r#""weapons":[]"#,
            &format!(r#""weapons":[{{"base":"dispenser",{fields}}}]"#),
            1,
        );
        files.insert(path, source.into_bytes());
        hash(&files)
    };
    let object = r#""ordnance":{"base":"mine","GeometryName":"stone"}"#;
    assert_ne!(
        fingerprint(&format!(r#""item":null,{object}"#)),
        fingerprint(&format!(r#"{object},"item":null"#))
    );
}

#[test]
fn a_shared_vertex_file_covers_both_gameplay_uses() {
    let mut files = court(r#""all":[],"rep":[]"#);
    let units = "sides/all/units.json";
    let source = std::str::from_utf8(&files.files[units])
        .unwrap()
        .replace("actors/sivlet.json", "weapons.json");
    files.insert(units, source.into_bytes());
    let mut meta: serde_json::Value =
        serde_json::from_slice(&files.files["sides/all/actors/sivlet.json"]).unwrap();
    meta["models"] = serde_json::json!({"stone":[{"vertexOffset":128,"vertexCount":3,"indexOffset":12,"indexCount":3}]});
    files.insert("sides/all/weapons.json", serde_json::to_vec(&meta).unwrap());
    files.insert(
        "sides/all/weapons_index.bin",
        [0u32, 1, 2, 4, 5, 6]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect(),
    );
    let observe = |files: &mut MemFiles, extent| {
        let mut bytes = mesh(9, 1., 0.);
        bytes.resize(128, 0);
        bytes.extend(mesh(8, extent, 0.));
        files.insert("sides/all/weapons.bin", bytes);
        let battle =
            Battle::load_for_family(&Loader::new(files), GameFamily::Swbf2, "court", QUERY)
                .unwrap();
        let content = bfe_content::content::Content::build(battle).unwrap();
        let defs = bfe_content::defs::Defs::build(&content).unwrap();
        (
            hash(files),
            defs.items
                .iter()
                .find(|it| it.key == "all/pebble")
                .unwrap()
                .radius,
        )
    };
    let before = observe(&mut files, 1.);
    let after = observe(&mut files, 2.);
    assert_ne!(before.1, after.1);
    assert_ne!(before.0, after.0);
}

#[test]
fn animation_samples_in_an_actor_file_still_participate() {
    let mut files = court(r#""all":[],"rep":[]"#);
    let units = "sides/all/units.json";
    let source = std::str::from_utf8(&files.files[units]).unwrap().replace(
        r#""actor":"actors/sivlet.json""#,
        r#""actor":"anim_banks.json","AnimationName":"reel""#,
    );
    files.insert(units, source.into_bytes());
    let mut meta: serde_json::Value =
        serde_json::from_slice(&files.files["sides/all/actors/sivlet.json"]).unwrap();
    meta["banks"] = serde_json::json!({"reel":{"clips":[{"crc":"00112233","flags":0,"frames":1,"offset":27,"tracks":[0]}]}});
    files.insert(
        "sides/all/anim_banks.json",
        serde_json::to_vec(&meta).unwrap(),
    );
    files.insert(
        "sides/all/anim_banks_index.bin",
        [0u32, 1, 2]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect(),
    );
    let observe = |files: &mut MemFiles, height: f32| {
        let mut bytes = mesh(9, 1., 0.);
        bytes.extend(
            [0., 0., 0., 1., 0., height, 0.]
                .into_iter()
                .flat_map(f32::to_le_bytes),
        );
        files.insert("sides/all/anim_banks.bin", bytes);
        let battle =
            Battle::load_for_family(&Loader::new(files), GameFamily::Swbf2, "court", QUERY)
                .unwrap();
        let content = bfe_content::content::Content::build(battle).unwrap();
        let clips = &content
            .unit_clips
            .iter()
            .find(|(key, _)| key == "all/all_0")
            .unwrap()
            .1;
        (hash(files), clips.get("00112233").unwrap().data()[5])
    };
    let before = observe(&mut files, 0.);
    let after = observe(&mut files, 1.);
    assert_ne!(before.1, after.1);
    assert_ne!(before.0, after.0);
}
