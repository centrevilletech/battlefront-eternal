use bfe_native::validate_capture;
use serde_json::json;

#[test]
fn rejects_truncated_misaligned_and_empty_capture_frames() {
    let mut capture = json!({"blobs": [[0, 8], [8, 4]], "textures": [],
        "frames": [{"records": 0, "input": 0, "bones": 1}], "calls": []});
    assert!(validate_capture(&capture, 12).is_ok());
    assert!(validate_capture(&capture, 11).is_err());
    capture["blobs"][0][1] = json!(7);
    assert!(validate_capture(&capture, 12).is_err());
    capture["frames"] = json!([]);
    assert!(validate_capture(&capture, 12).is_err());
}
