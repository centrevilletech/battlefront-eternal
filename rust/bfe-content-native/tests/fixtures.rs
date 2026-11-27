//! Synthetic admission fixtures exercise the prepared formats without game data.
use bfe_content_native::{geometry::Geometry, textures::Library};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "bfe-content-native-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("images")).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn vertices(stride: usize, count: usize) -> Vec<u8> {
    (0..stride * count)
        .flat_map(|i| (if i == 0 { -0.0f32 } else { i as f32 }).to_le_bytes())
        .collect()
}
#[test]
fn world_layout_keeps_authored_bits_attributes_and_local_indices() {
    let data = vertices(8, 3);
    let index = [0u32, 1, 2]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let record = json!({"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"stride":8,"attributes":["position","normal","uv"]});
    let geometry = Geometry::world(&record, &data, &index).unwrap();
    assert_eq!(geometry.buffers[0], data);
    assert_eq!(geometry.indices, [0, 1, 2]);
    assert_eq!(
        (geometry.attributes[2].stride, geometry.attributes[2].offset),
        (32, 24)
    );
    assert_eq!(&geometry.buffers[0][..4], &(-0.0f32).to_le_bytes());
    let mut bad = record.clone();
    bad["vertexCount"] = json!(4);
    assert!(Geometry::world(&bad, &data, &index).is_err());
    bad = record;
    bad["attributes"] = json!(["position", "normal", "color"]);
    assert!(Geometry::world(&bad, &data, &index).is_err());
}

#[test]
fn inspect_void_world_composes_layer_and_rigid_placement_without_a_roster() {
    use bfe_content_native::{placed, Loader};
    let dir = Directory::new();
    let map = dir.0.join("maps/sample");
    std::fs::create_dir_all(&map).unwrap();
    let mesh = json!({"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,
                      "stride":3,"attributes":["position"]});
    let matrix = [1,0,0,0, 0,1,0,0, 0,0,1,0, 18,0,-6,1];
    let mut layered_mesh = mesh.clone();
    layered_mesh["flags"] = json!(1);
    layered_mesh["material"] = json!({"blendMode":"alpha","doubleSided":false});
    let layer = json!({"meshes":[layered_mesh],"geom":"extra.bin","index":"extra_index.bin",
                       "placedObjects":[{"model":"FrAmE","matrix":matrix},{"base":"spawn"}]});
    let metadata = json!({"family":"annotation","meshes":[mesh],"layers":{"extra":layer}});
    let part = json!({"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,
                      "material":{"doubleSided":true},"flags":3,"transparent":false});
    let props = json!({"models":{"frame":[part]},"instances":[]});
    std::fs::write(map.join("map.json"), metadata.to_string()).unwrap();
    std::fs::write(map.join("props.json"), props.to_string()).unwrap();
    let base_vertices = vertices(3, 3);
    let mut layer_vertices = base_vertices.clone();
    layer_vertices[..4].copy_from_slice(&40.0f32.to_le_bytes());
    std::fs::write(map.join("geom.bin"), &base_vertices).unwrap();
    std::fs::write(map.join("extra.bin"), &layer_vertices).unwrap();
    std::fs::write(map.join("props.bin"), vertices(8, 3)).unwrap();
    let indices: Vec<_> = [0u32, 1, 2].into_iter().flat_map(u32::to_le_bytes).collect();
    for file in ["index.bin", "extra_index.bin", "props_index.bin"] {
        std::fs::write(map.join(file), &indices).unwrap();
    }
    for file in ["collision.bin", "collision_index.bin", "collision_poly.bin"] {
        std::fs::write(map.join(file), []).unwrap();
    }
    let loader = Loader::from_resolved_root(&dir.0, bfe_content_native::GameFamily::Swbf2);
    let base = loader.inspect("sample", None).unwrap();
    assert_eq!(base.world.len(), 1);
    assert!(placed::admit_bind_poses(&base).unwrap().parts.is_empty());
    let composed = loader.inspect("sample", Some("extra")).unwrap();
    assert!(composed.terrain_bvh.is_none());
    assert_eq!(composed.world.len(), 2);
    assert_eq!(composed.world[0].geometry.buffers[0], base_vertices);
    assert_eq!(composed.world[1].geometry.buffers[0], layer_vertices);
    assert_eq!(composed.world[1].geometry.indices, [0, 1, 2]);
    assert_eq!(composed.world[1].material.blend, bfe_content_native::loader::Blend::Alpha);
    assert!(!composed.world[1].material.double_sided);
    assert!(composed.models[0].material.cutout);
    assert_eq!(composed.models[0].material.blend, bfe_content_native::loader::Blend::Opaque);
    let placements = placed::admit_bind_poses(&composed).unwrap();
    assert_eq!(placements.parts.len(), 1);
    assert_eq!(placements.parts[0].matrix, matrix.map(f64::from));
    assert!(loader.inspect("sample", Some("unknown")).is_err());

    // Identical raw flags and texture do not merge the two different surfaces.
    let mut split = props.clone();
    let mut other = split["models"]["frame"][0].clone();
    other["material"] = json!({"blendMode":"opaque","doubleSided":false});
    split["models"]["frame"].as_array_mut().unwrap().push(other);
    std::fs::write(map.join("props.json"), split.to_string()).unwrap();
    let composed = loader.inspect("sample", Some("extra")).unwrap();
    assert!(composed.models[0].material.double_sided);
    assert!(!composed.models[1].material.double_sided);
    let batches = placed::admit_bind_poses(&composed).unwrap();
    assert_eq!(batches.parts.iter().map(|p| p.source).collect::<Vec<_>>(), [0, 1]);

    for bad_model in [false, true] {
        let mut metadata = metadata.clone();
        let mut props = props.clone();
        let row = if bad_model { &mut props["models"]["frame"][0] } else { &mut metadata["meshes"][0] };
        row["flags"] = json!(129);
        row["material"] = json!({"blendMode":"opaque"});
        std::fs::write(map.join("map.json"), metadata.to_string()).unwrap();
        std::fs::write(map.join("props.json"), props.to_string()).unwrap();
        let error = loader.inspect("sample", Some("extra")).err().expect("contradictory surface was admitted");
        assert!(error.contains("opaque blendMode with additive flag"), "{error}");
    }
}
#[test]
fn model_indices_rebase_once_and_malformed_extents_fail_before_upload() {
    let data = vertices(8, 6);
    let index = [3u32, 4, 5]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let mut record = json!({"vertexOffset":96,"vertexCount":3,"indexOffset":0,"indexCount":3});
    assert_eq!(
        Geometry::model(&record, &data, &index).unwrap().indices,
        [0, 1, 2]
    );
    record["vertexOffset"] = json!(100);
    assert!(Geometry::model(&record, &data, &index).is_err());
    record["vertexOffset"] = json!(96);
    record["indexOffset"] = json!(u64::MAX);
    assert!(Geometry::model(&record, &data, &index).is_err());
}
#[test]
fn texture_library_decodes_png_and_reuses_the_owned_source() {
    let dir = Directory::new();
    let path = dir.0.join("images/pixel.png");
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[80, 40, 20, 128])
            .unwrap();
    }
    std::fs::write(path, &encoded).unwrap();
    let mut library = Library::new(&dir.0, true, 4096, 0).unwrap();
    let a = library.load("images/pixel.png").unwrap();
    let b = library.load("images/pixel.png").unwrap();
    assert_eq!(a, b);
    assert_eq!(library.images.len(), 1);
    assert_eq!(library.images[a].data, [80, 40, 20, 128]);
    assert!(library.read("../outside").is_err());
    assert!(library.read("images/../../outside").is_err());
    assert!(library.read("https://example.invalid/image.png").is_err());
}
#[test]
fn declared_ktx_failure_is_not_retried_as_png() {
    let dir = Directory::new();
    std::fs::write(
        dir.0.join("images/ktx2.json"),
        r#"{"files":{"pixel.png":{"ktx2":"missing.ktx2"}}}"#,
    )
    .unwrap();
    let mut library = Library::new(&dir.0, true, 4096, 0).unwrap();
    let error = library.load("images/pixel.png").unwrap_err();
    assert!(error.contains("missing.ktx2"));
    std::fs::write(dir.0.join("images/ktx2.json"), r#"{"files":[]}"#).unwrap();
    assert!(Library::new(&dir.0, true, 4096, 0)
        .unwrap()
        .load("images/pixel.png")
        .is_err());
}

#[test]
fn compressed_twin_uses_device_policy_and_preserves_authored_mips() {
    use bfe_render::texture_format::Format;
    use sha2::{Digest, Sha256};
    let dir = Directory::new();
    std::fs::write(
        dir.0.join("images/opaque.ktx2"),
        include_bytes!("../../../tests/fixtures/renderer-textures/opaque.ktx2"),
    )
    .unwrap();
    std::fs::write(
        dir.0.join("images/ktx2.json"),
        br#"{"files":{"opaque.png":{"ktx2":"opaque.ktx2"}}}"#,
    )
    .unwrap();
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/renderer-textures/expected.json"
    ))
    .unwrap();
    let row = golden
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["file"] == "opaque.ktx2")
        .unwrap();
    // A PNG request must keep the authored representation even when a
    // compressed twin exists. Missing PNG bytes are not a reason to switch.
    assert!(Library::new(&dir.0, false, 16, 2)
        .unwrap()
        .load("images/opaque.png")
        .is_err());
    for (bits, format, name) in [
        (2, Format::Bc7, "bc7"),
        (2 | 4 | 64, Format::Bc1, "bc1"),
        (0, Format::Rgba, "rgba"),
    ] {
        let mut library = Library::new(&dir.0, true, 16, bits).unwrap();
        let id = library.load("images/opaque.png").unwrap();
        let image = &library.images[id];
        assert_eq!(image.format, format);
        assert_eq!(image.mips.len(), 5);
        assert_eq!(
            format!("{:x}", Sha256::digest(&image.data)),
            row["formats"][name]["hash"].as_str().unwrap()
        );
        assert!(image
            .mips
            .iter()
            .all(|m| m.offset + m.length <= image.data.len()));
    }
}
#[test]
fn unit_content_resolves_aliases_in_clip_and_bank_order() {
    use bfe_content::{actor::build_actor, clips::load_clips, json::parse};
    let actor=build_actor(&parse(r#"{"bones":[{"name":"root","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[]}"#).unwrap(),&[]).unwrap();
    let names = [
        "human_rifle_stand_idle_emote",
        "human_rifle_stand_idle_emote_alert",
        "human_rifle_stand_walkleft",
        "human_rifle_prone_walkleft",
        "human_tool_idle",
    ];
    let mut authored = serde_json::Map::new();
    for name in names {
        authored.insert(
            name.into(),
            json!({"frames":1,"offset":0,"tracks":[],"flags":0,"travel":[0,0,0]}),
        );
    }
    let metadata = parse(&json!({"fps":30,"bones":["root"],"clips":authored}).to_string()).unwrap();
    let clips = load_clips(
        &metadata,
        &std::rc::Rc::from(Vec::<f32>::new()),
        Some(&["root".into()]),
    )
    .unwrap();
    let banks = parse(
        r#"{"banks":{"rifle":{},"tool":{"postureless":{"stand_idle_emote":"human_tool_idle"}}}}"#,
    )
    .unwrap();
    let input = bfe_content_native::units::soldier(&actor, &clips, Some(&banks));
    let id = |name: &str| clips.list.iter().position(|c| c.name == name).unwrap();
    assert_eq!(input.tables.sets[0][0], Some(id(names[0])));
    assert_eq!(input.tables.alert[0], Some(id(names[1])));
    assert_eq!(input.tables.sets[0][7], None);
    assert_eq!(input.tables.sets[2][7], Some(id(names[3])));
    assert_eq!(input.tables.upper[1][id(names[0])], id(names[4]));
    let native = bfe_content_native::units::descriptor(&input);
    assert_eq!(native["bones"][0]["parent"], -1);
    assert_eq!(native["clips"].as_array().unwrap().len(), 5);
}

#[test]
fn effect_content_preserves_shared_definitions_and_distinct_filtered_groups() {
    use bfe_content::{json::parse, Object, Value};
    use std::rc::Rc;
    let emitter=parse(r#"{"geometry":{"type":"particle","texture":"smoke","blend":"alpha"},"transformer":{"position":[],"size":[],"color":[]}}"#).unwrap();
    let group = Value::Array(Rc::new(vec![emitter]));
    let library = Value::Object(Rc::new(Object::from_written(vec![
        (Rc::from("first"), group.clone()),
        (Rc::from("second"), group),
    ])));
    let content = bfe_content_native::effects::Effects::compile(&library).unwrap();
    assert_eq!(content.handles, [("first".into(), 1), ("second".into(), 2)]);
    assert_eq!(
        content.descriptor["definitions"].as_array().unwrap().len(),
        1
    );
    assert_eq!(content.descriptor["definitions"][0]["gpu"], true);
    assert_eq!(content.descriptor["groups"], json!([[], [0], [0]]));
    let filtered = parse(r#"{"bolt":[{"lightning":{}}],"shield":[{"shield":{}}]}"#).unwrap();
    let content = bfe_content_native::effects::Effects::compile(&filtered).unwrap();
    assert_eq!(content.descriptor["groups"], json!([[], [], []]));
    assert_eq!(content.descriptor["definitions"], json!([]));
}

#[test]
fn swbf2_admit_map_and_battle_compose_layers_materials_void_terrain_and_world_anims() {
    use bfe_content::zero_hash;
    use bfe_content_native::{
        loader::{Blend, Loader},
        placed, GameFamily,
    };

    let dir = Directory::new();
    let write = |rel: &str, bytes: &[u8]| {
        let p = dir.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    };
    let f32s = |xs: &[f32]| -> Vec<u8> { xs.iter().flat_map(|x| x.to_le_bytes()).collect() };
    let u32s = |xs: &[u32]| -> Vec<u8> { xs.iter().flat_map(|x| x.to_le_bytes()).collect() };

    let mut png_bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png_bytes, 1, 1);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&[255, 255, 255, 128]).unwrap();
    }
    write("maps/arena/textures/shield.png", &png_bytes);
    write("maps/arena/textures/hull.png", &png_bytes);
    write("maps/arena/textures/hull_bump.png", &png_bytes);

    // Base world mesh (flags=0 + material.doubleSided=true -> two-sided opaque:
    // the descriptor's doubleSided selects culling only, never blending)
    // and layer world mesh in layers/obj/{geom,index}.bin.
    let tri_verts = f32s(&[
        0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
        1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0,
    ]);
    let tri_idx = u32s(&[0, 1, 2]);
    write("maps/arena/geom.bin", &tri_verts);
    write("maps/arena/index.bin", &tri_idx);
    write("maps/arena/layers/obj/geom.bin", &tri_verts);
    write("maps/arena/layers/obj/index.bin", &tri_idx);

    write("maps/arena/collision.bin", &f32s(&[0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 4.0]));
    write("maps/arena/collision_index.bin", &u32s(&[0, 1, 2]));
    write("maps/arena/collision_poly.bin", &u32s(&[0]));
    write("maps/arena/collision_mask.bin", &[0]);
    write("maps/arena/layers/obj/collision.bin", &f32s(&[0.0, -160.0, 0.0, 4.0, -160.0, 0.0, 0.0, -160.0, 4.0]));
    write("maps/arena/layers/obj/collision_index.bin", &u32s(&[0, 1, 2]));
    write("maps/arena/layers/obj/collision_poly.bin", &u32s(&[0]));
    write("maps/arena/layers/obj/collision_mask.bin", &[1]);

    // Rigid prop models: hull, turret, and destroyed wreck.
    let prop_verts = [tri_verts.clone(), tri_verts.clone(), tri_verts.clone()].concat();
    let prop_idx = u32s(&[0, 1, 2, 3, 4, 5, 6, 7, 8]);
    write("maps/arena/props.bin", &prop_verts);
    write("maps/arena/props_index.bin", &prop_idx);
    write("maps/arena/props_collision.bin", &f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]));
    write("maps/arena/props_collision_index.bin", &u32s(&[0, 1, 2]));
    write("maps/arena/props_collision_poly.bin", &u32s(&[0]));
    write("maps/arena/props_collision_mask.bin", &[0]);
    write(
        "maps/arena/props.json",
        br#"{
            "family": "swbf2",
            "models": {
                "ship_hull": [{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":"hull","bump":"hull_bump","flags":1,"material":{"swbf2Flags":16842756,"doubleSided":true,"scroll":[0.1,0.0]}}],
                "ship_wreck": [{"vertexOffset":96,"vertexCount":3,"indexOffset":12,"indexCount":3,"texture":"hull","flags":1}],
                "tur_mesh": [{"vertexOffset":192,"vertexCount":3,"indexOffset":24,"indexCount":3,"texture":"hull","flags":1}]
            },
            "collision": {
                "ship_hull": {"firstTriangle": 0, "triangleCount": 1}
            },
            "instances": [
                {
                    "name": "ship_alpha",
                    "class": "ship_cls",
                    "base": "destructablebuilding",
                    "model": "SHIP_HULL",
                    "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1],
                    "properties": [["DestroyedGeometryName", "SHIP_WRECK"], ["MaxHealth", "750"]]
                },
                {
                    "name": "ship_alpha_tur",
                    "class": "tur_cls",
                    "base": "armedbuilding",
                    "model": "tur_mesh",
                    "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,5,0,1],
                    "properties": []
                },
                {
                    "name": "veh_spawner",
                    "class": "test_vehicle_pad",
                    "base": "vehiclespawn",
                    "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,-120,0,1],
                    "properties": []
                }
            ]
        }"#,
    );

    write(
        "maps/arena/map.json",
        br#"{
            "family": "swbf2",
            "textures": {
                "shield": "textures/shield.png",
                "hull": "textures/hull.png",
                "hull_bump": "textures/hull_bump.png"
            },
            "bounds": {"min": [-500, -500, -500], "max": [500, 500, 500]},
            "meshes": [{
                "vertexOffset": 0, "vertexCount": 3, "indexOffset": 0, "indexCount": 3,
                "stride": 8, "attributes": ["position", "normal", "uv"],
                "texture": "shield", "flags": 0,
                "material": {"swbf2Flags": 16842756, "doubleSided": true, "scroll": [0.2, 0.0]}
            }],
            "lights": [{"name": "sun"}],
            "regions": [],
            "deathRegions": [],
            "paths": [],
            "spawns": {"blue_sp": [[0,-120,0,0]]},
            "commandPosts": [],
            "worldAnimations": {
                "clips": {
                    "move": {
                        "duration": 2.0,
                        "loop": true,
                        "localTranslation": false,
                        "positions": [
                            {"time": 0.0, "value": [0, 0, 0], "type": 0, "tangents": [0,0,0,0,0,0]},
                            {"time": 2.0, "value": [20, 0, 0], "type": 0, "tangents": [0,0,0,0,0,0]}
                        ],
                        "rotations": []
                    }
                },
                "groups": {
                    "g": {"defaultOn": true, "stopWhenControlled": false, "disableHierarchies": false, "bindings": [{"clip": "move", "object": "ship_alpha"}]}
                },
                "hierarchies": [{"parent": "ship_alpha", "children": ["ship_alpha_tur"]}]
            },
            "missions": {
                "arena_m1": {
                    "layers": ["obj"],
                    "sides": {"blue": ["test_trooper"]}
                }
            },
            "layers": {
                "obj": {
                    "geom": "layers/obj/geom.bin",
                    "index": "layers/obj/index.bin",
                    "collision": "layers/obj/collision.bin",
                    "meshes": [{
                        "vertexOffset": 0, "vertexCount": 3, "indexOffset": 0, "indexCount": 3,
                        "stride": 8, "attributes": ["position", "normal", "uv"],
                        "texture": "hull", "flags": 1
                    }],
                    "placedObjects": [],
                    "commandPosts": [{"name":"cp1","team":1,"spawnPath":"blue_sp","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,-120,0,1]}],
                    "spawns": {},
                    "regions": [],
                    "lights": [],
                    "paths": []
                }
            }
        }"#,
    );

    // 1. Typed map admission works WITHOUT common/ or sides/!
    let loader = Loader::detect(&dir.0);
    let admitted = loader
        .admit_map(GameFamily::Swbf2, "arena", Some("arena_m1"), &[])
        .unwrap();
    assert_eq!(admitted.world.len(), 2);
    assert_eq!(admitted.world[0].material.blend, Blend::Opaque);
    assert!(admitted.world[0].material.double_sided);
    assert_eq!(admitted.world[0].material.record["scroll"][0].as_f64(), Some(0.2));
    assert_eq!(admitted.world[0].material.record["scroll"][1].as_f64(), Some(0.0));
    assert!(admitted.terrain_bvh.is_none());
    assert!(admitted.admission.collision.kill_y < -500.0);
    let hull_mesh = admitted
        .prop_models
        .iter()
        .find(|m| m.name == "ship_hull/0")
        .unwrap();
    assert!(hull_mesh.material.bump.is_some());
    assert_eq!(hull_mesh.material.blend, Blend::Opaque);
    assert!(hull_mesh.material.double_sided);
    // Logic-only vehiclespawn skipped cleanly; animated ship and child turret move together.
    let rest = placed::admit_map(&admitted).unwrap();
    assert_eq!(rest.parts.len(), 2);
    assert_eq!(rest.deferred, 0);
    let moved = placed::admit_map_at(&admitted, 1.0).unwrap();
    let ship_part = moved.parts.iter().find(|p| p.instance == 0).unwrap();
    let tur_part = moved.parts.iter().find(|p| p.instance == 1).unwrap();
    assert!((ship_part.matrix[12] - 10.0).abs() < 1e-6);
    assert!((tur_part.matrix[12] - 10.0).abs() < 1e-6);
    assert!((tur_part.matrix[13] - 5.0).abs() < 1e-6);

    // 2. Full battle admission with per-unit bone-CRC clips and `_full` idle fallback.
    write("common/common.json", br#"{"family":"swbf2","textures":{}}"#);
    let idle_crc = zero_hash::crc_hex("human_rifle_stand_idle_emote_full");
    let root_crc = zero_hash::crc_hex("DummyRoot");
    write(
        "common/anim_banks.json",
        format!(
            r#"{{"fps":30,"banks":{{"human_1":{{"clipCount":1,"clips":[{{"crc":"{idle_crc}","flags":0,"frames":2,"bones":1,"offset":0,"tracks":["{root_crc}"]}}]}}}}}}"#
        )
        .as_bytes(),
    );
    write("common/anim_banks.bin", &f32s(&[0.0; 14]));
    write(
        "sides/blue/units.json",
        br#"{
            "textures": {},
            "units": {
                "test_trooper": {
                    "class": "test_trooper",
                    "base": "soldier",
                    "model": "test_trooper_rig",
                    "actor": "actors/test_trooper_rig.json",
                    "properties": [["MaxHealth", "250"]],
                    "weapons": []
                }
            }
        }"#,
    );
    write("sides/blue/weapons.json", br#"{"models":{},"firePoints":{}}"#);
    write("sides/blue/weapons.bin", &[]);
    write("sides/blue/weapons_index.bin", &[]);
    write("sides/blue/anim_banks.json", br#"{"fps":30,"banks":{},"packages":{}}"#);
    write("sides/blue/anim_banks.bin", &[]);
    write(
        "sides/blue/actors/test_trooper_rig.json",
        br#"{"bones":[{"name":"dummyroot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}]}"#,
    );
    write("sides/blue/actors/test_trooper_rig.bin", &f32s(&[0.0; 27]));
    write("sides/blue/actors/test_trooper_rig_index.bin", &u32s(&[0, 1, 2]));

    let battle = loader
        .load_for_family(GameFamily::Swbf2, "arena", "?mission=arena_m1")
        .unwrap();
    assert_eq!(battle.world.len(), 2);
    assert!(battle.terrain_bvh.is_none());
    let defs = battle.defs().unwrap();
    assert!(defs.classes.iter().any(|c| defs.names.text(c.name) == "test_trooper"));
    let unit_input = battle
        .units
        .iter()
        .find(|(k, _)| k == "blue/test_trooper")
        .map(|(_, u)| u)
        .unwrap();
    assert!(unit_input.tables.sets[0][0].is_some());
    assert!(unit_input.tables.alert[0].is_some());
    // Destroyed look resolves canonicalized `ship_wreck`.
    let wreck = placed::admit_look(&battle, 0, 1).unwrap();
    assert_eq!(wreck.parts.len(), 1);
    assert_eq!(wreck.parts[0].model, "ship_wreck");

    // 3. Explicit 2005 material fields (`blendMode`, `alphaCutout`, authoritative
    // `doubleSided`) and prop skeleton hierarchy.
    write(
        "maps/arena/props.json",
        br#"{
            "family": "swbf2",
            "models": {
                "ship_hull": {
                    "parts": [{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":"hull","bump":"hull_bump","flags":4,"material":{"blendMode":"opaque","alphaCutout":0.5,"doubleSided":false,"textureMode":"image","sourceTexture":"hull"}}],
                    "bones": [{"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}]
                }
            },
            "collision": {},
            "instances": [{
                "name": "ship_alpha",
                "class": "ship_cls",
                "base": "destructablebuilding",
                "model": "ship_hull",
                "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1],
                "properties": []
            }]
        }"#,
    );
    let explicit = loader
        .admit_map(GameFamily::Swbf2, "arena", Some("arena_m1"), &[])
        .unwrap();
    let explicit_hull = explicit
        .prop_models
        .iter()
        .find(|m| m.name == "ship_hull/0")
        .unwrap();
    assert_eq!(explicit_hull.material.blend, Blend::Opaque);
    assert!(explicit_hull.material.cutout);
    // Explicit `material.doubleSided: false` overrides legacy `flags: 4` bit.
    assert!(!explicit_hull.material.double_sided);
    assert_eq!(explicit_hull.material.record["blendMode"].as_str(), Some("opaque"));
    assert_eq!(explicit_hull.material.record["alphaCutout"].as_f64(), Some(0.5));
    assert_eq!(explicit_hull.material.record["textureMode"].as_str(), Some("image"));
    let explicit_rig = explicit
        .admission
        .props
        .as_ref()
        .unwrap()
        .get("models")
        .unwrap()
        .get("ship_hull")
        .unwrap();
    assert_eq!(explicit_rig.get("bones").unwrap().as_array().unwrap().len(), 1);
    assert!(explicit_rig.get("box").is_some());
    assert!(explicit_rig.get("sphere").is_some());
}

/// Regression corpus verifying that odd-but-accepted 2004 records retain the
/// exact outputs of the released build (no 2005 material promotion, no prop
/// bump loading, tolerant empty-model deferral, no `_full` clip fallback, and
/// no flat-array prop model acceptance).
#[test]
fn swbf1_odd_inputs_preserve_released_behavior() {
    use bfe_content_native::{
        loader::{Blend, Loader},
        placed,
    };

    let dir = Directory::new();
    let write = |rel: &str, bytes: &[u8]| {
        let path = dir.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    };
    let f32s = |xs: &[f32]| -> Vec<u8> { xs.iter().flat_map(|x| x.to_le_bytes()).collect() };
    let u32s = |xs: &[u32]| -> Vec<u8> { xs.iter().flat_map(|x| x.to_le_bytes()).collect() };

    let mut png_bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png_bytes, 1, 1);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .unwrap()
            .write_image_data(&[200, 180, 160, 255])
            .unwrap();
    }
    write("maps/odd1/textures/wall.png", &png_bytes);
    write("maps/odd1/textures/wall_bump.png", &png_bytes);

    let tri_verts = f32s(&[
        0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
        2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 2.0, 0.0, 1.0, 0.0, 0.0, 1.0,
    ]);
    let tri_idx = u32s(&[0, 1, 2]);
    write("maps/odd1/geom.bin", &tri_verts);
    write("maps/odd1/index.bin", &tri_idx);
    write("maps/odd1/collision.bin", &f32s(&[0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0]));
    write("maps/odd1/collision_index.bin", &tri_idx);
    write("maps/odd1/collision_poly.bin", &u32s(&[0]));
    write("maps/odd1/terrain_collision.bin", &f32s(&[0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0]));
    write("maps/odd1/terrain_collision_index.bin", &tri_idx);
    write("maps/odd1/props.bin", &tri_verts);
    write("maps/odd1/props_index.bin", &tri_idx);

    // Odd 2004 map.json: world[0] carries `doubleSided: true` alongside `flags: 33`.
    // Released 2004 material() ignores `doubleSided` and keeps Opaque / flags=33.
    write(
        "maps/odd1/map.json",
        br#"{
            "textures": {"wall": "textures/wall.png", "wall_bump": "textures/wall_bump.png"},
            "meshes": [{
                "vertexOffset": 0, "vertexCount": 3, "indexOffset": 0, "indexCount": 3,
                "stride": 8, "attributes": ["position", "normal", "uv"],
                "texture": "wall", "flags": 33, "doubleSided": true
            }],
            "commandPosts": [{"name":"cp1","team":1,"spawnPath":"sp1","matrix":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}],
            "spawns": {"sp1": [[0, 1, 0, 0]]},
            "missions": {
                "odd1a": {
                    "mission": {
                        "era": "cw",
                        "sides": ["blue"],
                        "teams": {
                            "1": {"name": "blue", "classes": [["trooper", 0]], "units": 4, "reinforcements": 50},
                            "2": {"name": "blue", "classes": [["trooper", 0]], "units": 4, "reinforcements": 50}
                        }
                    }
                }
            }
        }"#,
    );

    // Odd 2004 props.json:
    // - model `box_rig` has a part with `"bump": "wall_bump"` (released 2004 append_pack leaves bump = None)
    // - instance 0 has `"model": ""` (released 2004 placed::admit defers it rather than erroring)
    write(
        "maps/odd1/props.json",
        br#"{
            "models": {
                "box_rig": {
                    "parts": [{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":"wall","bump":"wall_bump","flags":1}],
                    "bones": [],
                    "box": [[0,0,0],[2,0,2]],
                    "sphere": [1,0,1,2]
                }
            },
            "instances": [
                {"name": "empty_model_inst", "base": "powerupstation", "model": "", "matrix": [1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name": "box_inst", "base": "powerupstation", "model": "box_rig", "matrix": [1,0,0,0,0,1,0,0,0,0,1,0,4,0,0,1]}
            ]
        }"#,
    );

    // Minimal 2004 common/anims/side files, with ONLY a `_full` idle clip to verify
    // released 2004 units::soldier does not apply the 2005 `_full` fallback.
    write("common/common.json", br#"{"textures":{}}"#);
    write("common/effects.json", br#"{"effects":{}}"#);
    write("common/fxmodels.bin", &[]);
    write("common/fxmodels_index.bin", &[]);
    write(
        "common/anims.json",
        br#"{
            "fps": 30,
            "bones": ["DummyRoot"],
            "clips": {
                "human_rifle_stand_idle_emote_full": {"frames": 2, "offset": 0, "tracks": [0], "flags": 0, "travel": [0, 0, 0]}
            }
        }"#,
    );
    write("common/anims.bin", &f32s(&[0.0; 14]));
    write(
        "sides/blue/units.json",
        br#"{
            "textures": {},
            "units": {
                "trooper": {
                    "base": "soldier",
                    "model": "trooper_body",
                    "MaxHealth": 200,
                    "weapons": []
                }
            }
        }"#,
    );
    write("sides/blue/weapons.json", br#"{"models":{},"firePoints":{}}"#);
    write("sides/blue/weapons.bin", &[]);
    write("sides/blue/weapons_index.bin", &[]);
    write(
        "sides/blue/actors/trooper_body.json",
        br#"{"bones":[{"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":""}]}"#,
    );
    write("sides/blue/actors/trooper_body.bin", &f32s(&[0.0; 27]));
    write("sides/blue/actors/trooper_body_index.bin", &tri_idx);

    let loader = Loader::detect(&dir.0);
    let battle = loader.load("odd1", "?map=odd1&side=blue&unit=trooper").unwrap();
    // 1. world[0]: released material() keeps Opaque, double_sided=false, cutout=false, flags=33.
    let m = &battle.world[0].material;
    assert_eq!(m.blend, Blend::Opaque);
    assert!(!m.double_sided);
    assert!(!m.cutout);
    assert_eq!(m.flags, 33);
    // 2. prop models: bump stays None on released 2004 append_pack.
    assert_eq!(battle.models.iter().filter(|x| x.material.bump.is_some()).count(), 0);
    assert!(battle.models.iter().any(|x| x.name == "box_rig/0"));
    // 3. placed::admit: empty model string `""` defers cleanly (1 deferred, 1 placed part).
    let placed = placed::admit(&battle).unwrap();
    assert_eq!(placed.deferred, 1);
    assert_eq!(placed.parts.len(), 1);
    assert_eq!(placed.parts[0].model, "box_rig");
    // 4. units::soldier: `_full` idle clip is NOT used as fallback on 2004.
    let (_, unit_in) = battle.units.iter().find(|(k, _)| k == "blue/trooper_body").unwrap();
    assert!(unit_in.tables.sets[0][0].is_none());
    assert!(unit_in.tables.alert[0].is_none());
    // 2005 soldier_swbf2 on the exact same actor+clips DOES resolve `_full`.
    let swbf2_in = bfe_content_native::units::soldier_swbf2(
        &battle.content.actors[0].1,
        &battle.content.body_clips[0].1,
        None,
        &[],
    );
    assert_eq!(swbf2_in.tables.sets[0][0], Some(0));
    assert_eq!(swbf2_in.tables.alert[0], Some(0));
    // 5. parts_records: a bare-array 2005-style prop model is rejected on 2004
    // (preserving released behavior instead of silently admitting it).
    write(
        "maps/odd1/props.json",
        br#"{
            "models": {
                "flat_array": [{"vertexOffset":0,"vertexCount":3,"indexOffset":0,"indexCount":3,"texture":"wall","flags":1}]
            },
            "instances": []
        }"#,
    );
    let err = loader.load("odd1", "?map=odd1&side=blue&unit=trooper").err().unwrap();
    assert!(err.contains("parts is not an array"), "expected released parts_records error: {err}");
}

fn paper_world() -> Directory {
    let dir = Directory::new();
    write_paper_world(&dir.0);
    dir
}

fn write_paper_world(root: &std::path::Path) {
    use bfe_content::mission::DEFAULT_UNIT;
    let map = root.join("maps/paper");
    let common = root.join("common");
    let side = root.join("sides/rep");
    let actors = side.join("actors");
    for path in [&map, &common, &actors] {
        std::fs::create_dir_all(path).unwrap();
    }
    for (file, data) in [
        (common.join("anims.json"), json!({"bones":[],"clips":{}})),
        (common.join("common.json"), json!({})),
        (common.join("effects.json"), json!({})),
        (side.join("units.json"), json!({"units":{(DEFAULT_UNIT):{"base":"soldier","model":"prism"}}})),
        (side.join("weapons.json"), json!({"models":{}})),
        (actors.join("prism.json"), json!({"bones":[],"parts":[]})),
    ] {
        std::fs::write(file, data.to_string()).unwrap();
    }
    for file in ["collision.bin","collision_index.bin","collision_poly.bin",
                 "terrain_collision.bin","terrain_collision_index.bin"] {
        std::fs::write(map.join(file), []).unwrap();
    }
    for path in [common.join("fxmodels.bin"), common.join("fxmodels_index.bin"), common.join("anims.bin"), side.join("weapons.bin"), side.join("weapons_index.bin"),
                 actors.join("prism.bin"), actors.join("prism_index.bin")] {
        std::fs::write(path, []).unwrap();
    }
    let indices: Vec<_> = [2_u32, 0, 1].into_iter().flat_map(u32::to_le_bytes).collect();
    std::fs::write(map.join("geom.bin"), vertices(3, 3)).unwrap();
    std::fs::write(map.join("index.bin"), indices).unwrap();

}

#[test]
fn map_extensions_cannot_select_the_render_family() {
    use bfe_content::mission::DEFAULT_UNIT;
    use bfe_content_native::Loader;
    let dir = paper_world();
    let map = dir.0.join("maps/paper");
    let loader = Loader::detect(&dir.0);
    let expected: serde_json::Value = serde_json::from_str(include_str!("fixtures/legacy-surface.json")).unwrap();
    let tags = [json!(null),json!(41),json!(["other"]),json!("swbf2"),json!(" SWBF2 ")];
    for tag in tags {
        let metadata = json!({
            "family":tag,"layers":{"test":false},
            "mission":{"sides":["rep"],"era":"cw"},
            "meshes":[{"flags":1,"vertexOffset":0,"vertexCount":3,"indexOffset":0,
                       "indexCount":3,"stride":3,"attributes":["position"],
                       "material":"paper label","extendedLight":{"ignored":true}}]
        });
        std::fs::write(map.join("map.json"), metadata.to_string()).unwrap();
        for content in [loader.load("paper", "map=paper&seed=1").unwrap(),
                        loader.inspect("paper", None).unwrap()] {
            let mesh = &content.world[0];
            assert_eq!(content.world.len(), 1);
            assert_eq!(content.content.battle.unit, DEFAULT_UNIT);
            assert_eq!(json!({
                "flags": mesh.material.flags,
                "indices": mesh.geometry.indices,
                "vertexBytes": mesh.geometry.buffers[0],
            }), expected);
            assert_eq!(content.family, bfe_content_native::GameFamily::Swbf1);
            assert!(!content.static_inspection);
        }
        assert_eq!(loader.inspect("paper", Some("test")).err().as_deref(), Some("world has no selectable layer"));
    }
}

#[test]
fn loaded_battle_validates_original_surface_flags() {
    use bfe_content_native::{GameFamily, Loader};
    let dir = paper_world();
    let loader = Loader::from_resolved_root(&dir.0, GameFamily::Swbf2);
    let mut map = json!({"mission":{"sides":["rep"],"era":"cw"},
        "meshes":[{"flags":129,"vertexOffset":0,"vertexCount":3,"indexOffset":0,
        "indexCount":3,"stride":3,"attributes":["position"],
        "material":{"blendMode":"opaque","doubleSided":false}}]});
    let write = |map: &serde_json::Value| std::fs::write(dir.0.join("maps/paper/map.json"), map.to_string()).unwrap();
    write(&map);
    let error = loader.load("paper", "map=paper&seed=1").err().expect("contradictory surface was admitted");
    assert!(error.contains("opaque blendMode with additive flag"), "{error}");
    map["meshes"][0]["flags"] = json!(1);
    map["meshes"][0]["material"]["alphaCutout"] = json!(0.5);
    write(&map);
    let scene = loader.load("paper", "map=paper&seed=1").unwrap();
    assert!(scene.world[0].material.cutout);
    assert!(!scene.world[0].material.double_sided);
}

#[test]
fn the_selected_root_supplies_the_family_to_both_world_readers() {
    use bfe_content_native::{GameFamily, Loader};
    let parent = Directory::new();
    let root = parent.0.join("swbf2/.content");
    write_paper_world(&root);
    let world = root.join("maps/paper");
    std::fs::write(world.join("props.json"), r#"{"models":{},"instances":[]}"#).unwrap();
    for file in ["props.bin", "props_index.bin"] {
        std::fs::write(world.join(file), []).unwrap();
    }
    let map = json!({"family":"swbf1","mission":{"sides":["rep"],"era":"cw"},
        "meshes":[{"flags":1,"vertexOffset":0,"vertexCount":3,"indexOffset":0,
        "indexCount":3,"stride":3,"attributes":["position"],"material":{"alphaCutout":0.5}}]});
    std::fs::write(world.join("map.json"), map.to_string()).unwrap();
    let load = |reader: Loader| {
        [reader.load("paper", "map=paper&seed=1").unwrap(), reader.inspect("paper", None).unwrap()]
    };
    for content in load(Loader::detect(&root)) {
        assert_eq!(content.family, GameFamily::Swbf1);
        assert!(content.world[0].material.extended.is_none());
    }
    std::fs::write(root.join(".prepare-swbf2.json"), "{}").unwrap();
    for content in load(Loader::detect(&root)) {
        assert_eq!(content.family, GameFamily::Swbf2);
        assert!(content.world[0].material.cutout);
    }
    let (original, bare) = Loader::for_map(&root, "swbf1/paper");
    assert_eq!(bare, "paper");
    for content in load(original) {
        assert_eq!(content.family, GameFamily::Swbf1);
        assert!(content.world[0].material.extended.is_none());
    }
    std::fs::remove_file(root.join(".prepare-swbf2.json")).unwrap();
    let (extended, bare) = Loader::for_map(&root, "swbf2/paper");
    assert_eq!(bare, "paper");
    for content in load(extended) {
        assert_eq!(content.family, GameFamily::Swbf2);
        assert!(content.world[0].material.cutout);
    }
}

#[test]
fn placed_vertex_colours_follow_the_bind_batch_on_both_admission_routes() {
    use bfe_content_native::{placed, GameFamily, Loader};
    let dir = paper_world();
    let map = dir.0.join("maps/paper");
    std::fs::write(map.join("map.json"), json!({"mission":{"era":"cw","sides":["rep"]},
        "meshes":[{"flags":1,"vertexOffset":0,"vertexCount":3,"indexOffset":0,
                   "indexCount":3,"stride":3,"attributes":["position"]}]}).to_string()).unwrap();
    let pose = [1,0,0,0, 0,1,0,0, 0,0,1,0, 2,4,6,1];
    let mut parts = Vec::new();
    for i in 0..3 {
        parts.push(json!({"flags":1, "vertexOffset":i*96, "indexOffset":i*12,
            "vertexCount":3,"indexCount":3,"material":{"swbf2Flags":512}}));
    }
    parts[0]["vertexStreams"] = json!({"lit":100,"tint":148});
    parts[1]["vertexStreams"] = json!({"lit":4,"tint":52});
    parts[1]["material"]["swbf2Flags"] = json!(0);
    parts[2]["vertexStreams"] = json!({"lit":4});
    parts[2]["skinJoints"] = json!(vec![0; 9]);
    parts[2]["skinWeights"] = json!([0.25,0.5,0.25, 0.5,0.25,0.25, 0.125,0.125,0.75]);
    let mut props = json!({"attributesFile":"colours.bin","models":{"tile":{"bones":[],"parts":parts}},
        "instances":[{"model":"tile","base":"door","matrix":pose}]});
    let indices: Vec<_> = (0u32..9).flat_map(u32::to_le_bytes).collect();
    std::fs::write(map.join("props_index.bin"), indices).unwrap();
    std::fs::write(map.join("props.bin"), vertices(8, 9)).unwrap();
    let data: Vec<u8> = (0..49).flat_map(|i| (i as f32 / 64.).to_le_bytes()).collect();
    std::fs::write(map.join("colours.bin"), &data).unwrap();
    std::fs::write(map.join("props.json"), props.to_string()).unwrap();
    let loader = Loader::from_resolved_root(&dir.0, GameFamily::Swbf2);
    for content in [loader.inspect("paper", None).unwrap(), loader.load("paper", "map=paper&seed=1").unwrap()] {
        let placed = placed::admit_look(&content, 0, 0).unwrap();
        assert_eq!(placed.parts.len(), 2);
        let batch = &placed.parts[0];
        assert_eq!(batch.index, [0,1,2,3,4,5]);
        assert_eq!(&batch.weights[12..16], &[0.25,0.5,0.25,0.]);
        let colours = batch.colours.as_ref().unwrap();
        assert_eq!(colours.light, [&data[100..148], &data[4..52]].concat());
        assert_eq!(&colours.tint[..48], &data[148..196]);
        assert_eq!(colours.tint[48..], [1.0f32.to_le_bytes(); 12].concat());
        let tint_only = placed.parts[1].colours.as_ref().unwrap();
        assert!(tint_only.light.is_empty());
        assert_eq!(tint_only.tint, data[52..100]);
    }
    props["models"]["tile"]["parts"][2]["vertexStreams"]["lit"] = json!(196);
    std::fs::write(map.join("props.json"), props.to_string()).unwrap();
    let content = loader.inspect("paper", None).unwrap();
    assert!(placed::admit_bind_poses(&content).err().unwrap().contains("truncated lit"));
    std::fs::remove_file(map.join("colours.bin")).unwrap();
    let released = Loader::from_resolved_root(&dir.0, GameFamily::Swbf1);
    for content in [released.inspect("paper", None).unwrap(), released.load("paper", "map=paper&seed=1").unwrap()] {
        let placed = placed::admit_look(&content, 0, 0).unwrap();
        assert!(placed.parts.iter().all(|p| p.colours.is_none()));
    }
}
