use bfe_native::validate_capture;
use serde_json::json;

#[test]
fn rejects_truncated_misaligned_and_empty_capture_frames() {
    let mut capture = json!({"blobs": [[0, 8], [8, 4]], "textures": [],
        "frames": [{"records": 0, "input": 0, "bones": 1}], "calls": []});
    assert!(validate_capture(&capture, &[0; 12]).is_ok());
    assert!(validate_capture(&capture, &[0; 11]).is_err());
    capture["blobs"][0][1] = json!(7);
    assert!(validate_capture(&capture, &[0; 12]).is_err());
    capture["frames"] = json!([]);
    assert!(validate_capture(&capture, &[0; 12]).is_err());
}

// Synthetic registration order covers every unchecked owner lookup used by
// the near replay. No GL context or prepared game data participates in this
// admission test, so corrupt files fail equally on headless and window hosts.
fn scene() -> (serde_json::Value, Vec<u8>) {
    let mut data = Vec::new();
    let mut blobs = Vec::new();
    let mut blob = |bytes: Vec<u8>| {
        let id = blobs.len();
        blobs.push(json!([data.len(), bytes.len()]));
        data.extend(bytes);
        id
    };
    let bounds = blob(
        [0., 0., 0., 1., -1., -1., -1., 1., 1., 1.]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect(),
    );
    let vertices = blob(
        [0., 0., 0., 1., 0., 0., 0., 1., 0.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect(),
    );
    let index = blob(
        [0u32, 1, 2]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect(),
    );
    let uniform = blob(1f32.to_le_bytes().to_vec());
    let input = blob(0f64.to_le_bytes().to_vec());
    let bones = blob(vec![]);
    let texture = blob(vec![255; 4]);
    let meta = json!({"blobs":blobs,"textures":[{"width":1,"height":1,"blob":texture}],"frames":[{"records":0,"input":input,"bones":bones}],"calls":[
        ["add_texture",[],0],["add_program",[0,"vertex","fragment","position:0","colour"],0],
        ["add_geometry",[],0],["geometry_buffer",[0,{"blob":vertices}],0],
        ["geometry_pointer",[0,0,0,3,glow::FLOAT,false,0,0],null],
        ["geometry_index",[0,{"blob":index},glow::UNSIGNED_INT],null],
        ["geometry_group",[0,0,3],null],["add_world",[0,0,0,8,{"blob":bounds}],0],
        ["world_texture",[0,0,0],null],["world_uniform",[0,"colour",{"blob":uniform},false],null],
        ["world_cells",[0,{"blob":vertices},{"blob":index},48],null],["sort",[null],null]
    ]});
    (meta, data)
}
#[test]
fn malformed_owner_ids_and_lengths_fail_before_registration() {
    let (valid, bytes) = scene();
    validate_capture(&valid, &bytes).unwrap();
    for (call, arg, value, label) in [
        (3, 0, json!(1), "geometry id"),
        (4, 1, json!(1), "buffer id"),
        (7, 0, json!(9), "world geometry id"),
        (7, 1, json!(9), "world program id"),
        (8, 0, json!(1), "world id"),
        (8, 2, json!(1), "world texture id"),
        (8, 2, json!(-1), "unsigned integer"),
        (8, 2, json!(0.5), "unsigned integer"),
        (4, 7, json!(1024), "attribute extent"),
        (6, 2, json!(4), "index buffer"),
    ] {
        let mut meta = valid.clone();
        meta["calls"][call][1][arg] = value;
        let result = std::panic::catch_unwind(|| validate_capture(&meta, &bytes));
        let error = result
            .expect("malformed admission must not panic")
            .unwrap_err();
        assert!(error.contains(label), "{error}");
    }
    // JSON floating-point metadata must not pass admission and then reach
    // the replay's exact integer readers during window resume.
    for field in ["input", "bones", "records"] {
        let mut meta = valid.clone();
        meta["frames"][0][field] = json!(0.0);
        assert!(validate_capture(&meta, &bytes).is_err());
    }
    let mut meta = valid.clone();
    meta["textures"][0]["width"] = json!(1.0);
    assert!(validate_capture(&meta, &bytes).is_err());
    let mut meta = valid.clone();
    meta["blobs"][0][1] = json!(72);
    assert!(validate_capture(&meta, &bytes)
        .unwrap_err()
        .contains("ten finite"));
    let mut data = bytes.clone();
    data[..8].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(validate_capture(&valid, &data).is_err());
    let mut data = bytes.clone();
    data[116..120].copy_from_slice(&99u32.to_le_bytes());
    assert!(validate_capture(&valid, &data)
        .unwrap_err()
        .contains("cell index"));
    let mut meta = valid.clone();
    meta["blobs"][3][1] = json!(0);
    assert!(validate_capture(&meta, &bytes)
        .unwrap_err()
        .contains("uniform"));
    let mut meta = valid.clone();
    meta["frames"][0]["records"] = json!(1);
    assert!(validate_capture(&meta, &bytes)
        .unwrap_err()
        .contains("renderer storage"));
}
#[test]
fn malformed_frame_references_and_bone_ranges_are_rejected() {
    use bfe_render::instances as i;
    let (mut meta, mut bytes) = scene();
    let at = bytes.len();
    let mut input = vec![0f64; i::INPUT_LEN];
    let r = i::IN_RECORDS;
    input[r + i::REC_KIND] = 2.;
    input[r + i::REC_PROGRAM] = 0.;
    input[r + i::REC_GEOMETRY] = 0.;
    for g in 0..4 {
        input[r + i::REC_TEXTURES + g] = -1.;
    }
    bytes.extend(input.iter().flat_map(|v| v.to_le_bytes()));
    meta["blobs"]
        .as_array_mut()
        .unwrap()
        .push(json!([at, input.len() * 8]));
    meta["frames"][0]["input"] = json!(7);
    meta["frames"][0]["records"] = json!(1);
    validate_capture(&meta, &bytes).unwrap();
    for (field, value, label) in [
        (i::REC_PROGRAM, 4., "program id"),
        (i::REC_GEOMETRY, 4., "geometry id"),
        (i::REC_TEXTURES, 4., "texture id"),
        (i::REC_GROUPS, 2., "group count"),
    ] {
        let mut data = bytes.clone();
        let at = at + (r + field) * 8;
        data[at..at + 8].copy_from_slice(&f64::to_le_bytes(value));
        assert!(validate_capture(&meta, &data).unwrap_err().contains(label));
    }
    let mut data = bytes;
    let kind = at + (r + i::REC_KIND) * 8;
    data[kind..kind + 8].copy_from_slice(&1f64.to_le_bytes());
    let count = at + (r + i::REC_BONES) * 8;
    data[count..count + 8].copy_from_slice(&1f64.to_le_bytes());
    assert!(validate_capture(&meta, &data)
        .unwrap_err()
        .contains("bone range"));
}
