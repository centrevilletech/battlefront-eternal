// Frozen synthetic results exercise the complete presentation phases. Cargo supplies
// this build's replay executable, so no installed game data or prior build is
// needed and the assertions cannot silently skip under the Rust test job.
//
// `support/v8math_preload.mjs` installs non-FMA fdlibm `Math.sin` and `Math.cos`
// matching `v8math` and x86_64 V8 (`src/base/ieee754.cc`), avoiding the 1-ULP
// FMA contraction in official `linux-arm64` Node 24 binaries when synthesizing
// the 600-frame JavaScript test inputs in `tests/presentation-units.test.js`.
#[test]
fn display_goldens() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../..");
    let preload = manifest_dir.join("tests/support/v8math_preload.mjs");
    let output = std::process::Command::new("node")
        .arg("--import")
        .arg(&preload)
        .arg("--test")
        .arg("--test-reporter=tap")
        .arg("tests/presentation-units.test.js")
        .current_dir(root)
        .env("BFE_UNIT_REPLAY", env!("CARGO_BIN_EXE_unit-replay"))
        .output()
        .expect("Node for display goldens");
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "native presentation golden failed:\n{report}"
    );
    // The native-versus-WASM checks skip only without the executable; here
    // they must all run.
    assert!(
        report.contains("# skipped 0"),
        "native presentation checks skipped:\n{report}"
    );
}
