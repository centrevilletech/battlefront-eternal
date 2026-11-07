// Frozen synthetic results exercise the complete presentation phases. Cargo supplies
// this build's replay executable, so no installed game data or prior build is
// needed and the assertions cannot silently skip under the Rust test job.
#[test]
fn display_goldens() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = std::process::Command::new("node")
        .arg("--test")
        .arg("--test-reporter=tap")
        .arg("tests/presentation-units.test.js")
        .current_dir(root)
        .env("BFE_UNIT_REPLAY", env!("CARGO_BIN_EXE_unit-replay"))
        .output()
        .expect("Node for display goldens");
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "native presentation golden failed:\n{report}");
    // The native-versus-WASM checks skip only without the executable; here
    // they must all run.
    assert!(report.contains("# skipped 0"), "native presentation checks skipped:\n{report}");
}
