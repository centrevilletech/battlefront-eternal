//! A missing local authority is reported before opening any display or
//! falling into archive/online parsing. This runs without installed content,
//! including on CI hosts that have no window or audio device.
#[test]
fn local_launch_refuses_at_admission_before_device_creation() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_bfe-client"))
        .args(["--local", "nab2", "--side", "rep", "--no-audio"])
        .env("BFE_CONTENT_DIR", std::env::temp_dir().join("missing-battle-content"))
        .env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.starts_with("bfe-client: "), "{error}");
    if !bfe_sim::world::admit::provided() {
        assert_eq!(error.trim(), "bfe-client: native world API unavailable: world_admit");
    } else {
        assert!(!error.contains("unavailable"), "{error}");
    }
    assert!(!error.contains("EventLoop"), "{error}");
    assert!(!error.contains("archive"), "{error}");
}
