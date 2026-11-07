//! Removed capture layouts fail before a window or audio device is opened.
#[test]
fn old_near_capture_is_rejected_without_a_display() {
    let dir = std::env::temp_dir().join(format!("native-client-capture-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("calls.json"), b"{}").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_bfe-client"))
        .arg(&dir)
        .arg(&dir)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("version-2 session.json"), "{error}");
    assert!(error.contains("bfe-native"), "{error}");
}
