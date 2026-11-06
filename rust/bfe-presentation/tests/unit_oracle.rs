// Frozen synthetic results exercise the complete presentation phases. Cargo supplies
// this build's replay executable, so no installed game data or prior build is
// needed and the assertions cannot silently skip under the Rust test job.
#[test]
fn display_goldens() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let status = std::process::Command::new("node")
        .arg("--test")
        .arg("tests/presentation-units.test.js")
        .current_dir(root)
        .env("BFE_UNIT_REPLAY", env!("CARGO_BIN_EXE_unit-replay"))
        .status()
        .expect("Node for display goldens");
    assert!(status.success(), "native presentation golden failed");
}
