#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
#[path = "support/movement_trace.rs"]
mod trace;
use std::{fs, path::Path, process::Command};
#[test]
fn native_movement_matches_two_wasm_instances_and_isolated_legacy_processes() {
    if let Ok(lane) = std::env::var("BFE_MOVEMENT_LEGACY_LANE") {
        let lane = lane.parse().unwrap();
        let path = std::env::var("BFE_MOVEMENT_TRACE").unwrap();
        trace::assert_no_legacy_exchange_use(Path::new(&path), lane);
        let n = trace::replay(Path::new(&path), trace::Mode::Legacy, Some(lane));
        eprintln!(
            "isolated legacy movement: {n} exact records; native leaves legacy exchanges untouched"
        );
        return;
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = std::env::temp_dir().join(format!("native-movement-{}.jsonl", std::process::id()));
    assert!(
        Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
            .arg(repo.join("tools/native-movement-twin.mjs"))
            .arg("--out")
            .arg(&path)
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(trace::replay(&path, trace::Mode::Owned, None), 6720);
    assert_eq!(trace::replay(&path, trace::Mode::Stateful, None), 6720);
    for lane in 0..2 {
        assert!(Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "native_movement_matches_two_wasm_instances_and_isolated_legacy_processes",
                "--nocapture"
            ])
            .env("BFE_MOVEMENT_LEGACY_LANE", lane.to_string())
            .env("BFE_MOVEMENT_TRACE", &path)
            .status()
            .unwrap()
            .success());
    }
    fs::remove_file(path).unwrap();
    eprintln!("owned movement: 6720 exact exchange and 6720 exact stateful records, two contexts");
}
