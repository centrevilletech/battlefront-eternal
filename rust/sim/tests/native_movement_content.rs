//! Acceptance supplies a closed source/content-authenticated battle corpus.
#![cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
#[path = "support/movement_trace.rs"]
mod trace;
use std::{fs, path::Path, process::Command};
#[test]
fn native_movement_matches_authenticated_battle_corpus() {
    if let Ok(file) = std::env::var("BFE_MOVEMENT_LEGACY_FILE") {
        let n = trace::replay(Path::new(&file), trace::Mode::Legacy, None);
        eprintln!("legacy battle movement: {n} exact records");
        return;
    }
    let Some(root) = std::env::var_os("BFE_NATIVE_MOVEMENT_CORPUS") else {
        eprintln!("skipped: no BFE_NATIVE_MOVEMENT_CORPUS");
        return;
    };
    let root = Path::new(&root);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "bfemove"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 17);
    for file in &files {
        assert!(
            Command::new(std::env::var_os("BFE_NODE").unwrap_or_else(|| "node".into()))
                .arg(repo.join("tools/native-movement-capture.mjs"))
                .arg("--verify")
                .arg(format!("{}.json", file.display()))
                .status()
                .unwrap()
                .success()
        );
    }
    let mut total = 0;
    for pair in files.chunks(2) {
        total += if pair.len() == 2 {
            trace::interleave(&pair[0], &pair[1])
        } else {
            trace::replay(&pair[0], trace::Mode::Owned, None)
        }
    }
    for file in files {
        assert!(Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "native_movement_matches_authenticated_battle_corpus",
                "--nocapture"
            ])
            .env("BFE_MOVEMENT_LEGACY_FILE", file)
            .status()
            .unwrap()
            .success());
    }
    eprintln!("owned authenticated movement corpus: 17 maps, {total} exact interleaved records");
    assert!(total >= 10000);
}
