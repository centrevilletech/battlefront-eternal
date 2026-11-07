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
