//! Integration tests against a local Battlefront II (2005) installation.
//!
//! Reads the install root from `BFE_SWBF2_DIR` (skipped only if unset; panics
//! if `BFE_SWBF2_DIR` is set to an invalid path). No retail game files or
//! extracted script contents are included in the repository.

use std::path::{Path, PathBuf};

use bfe_script::{
    load_chunk, record_manifest, RecordedValue, ScriptArchive, Vm, MANIFEST_SCHEMA_VERSION,
};

fn resolve_lvl_dir() -> Option<PathBuf> {
    let root = std::env::var_os("BFE_SWBF2_DIR")?;
    let base = PathBuf::from(&root);
    let candidates = [
        base.join("GameData/data/_lvl_pc"),
        base.join("GameData/Data/_LVL_PC"),
        base.join("data/_lvl_pc"),
        base.clone(),
    ];
    let found = candidates
        .into_iter()
        .find(|p| p.join("mission.lvl").is_file());
    Some(found.unwrap_or_else(|| {
        panic!(
            "BFE_SWBF2_DIR is set to {:?}, but mission.lvl was not found under any expected _lvl_pc path",
            base
        )
    }))
}

fn read_archive(dir: &Path, file: &str) -> Vec<u8> {
    let path = dir.join(file);
    std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

#[test]
#[ignore = "requires BFE_SWBF2_DIR pointing to a local Battlefront II (2005) install"]
fn load_every_script_in_swbf2_archives() {
    let Some(lvl_dir) = resolve_lvl_dir() else {
        eprintln!("BFE_SWBF2_DIR not set; skipping retail script load test");
        return;
    };

    // Check each archive independently against its expected non-empty script set.
    let expected_archives = [
        ("mission.lvl", 100usize),
        ("common.lvl", 20usize),
        ("ingame.lvl", 5usize),
        ("shell.lvl", 50usize),
    ];
    let mut total_loaded = 0usize;
    for (archive_name, min_scripts) in expected_archives {
        let data = read_archive(&lvl_dir, archive_name);
        let archive = ScriptArchive::parse(&data)
            .unwrap_or_else(|e| panic!("failed to parse {archive_name}: {e}"));
        let count = archive.scripts().len();
        assert!(
            count >= min_scripts,
            "{archive_name} expected at least {min_scripts} scripts, got {count}"
        );
        let mut vm: Vm = Vm::new_with_stdlib();
        for script in archive.scripts() {
            let proto = load_chunk(script.body).unwrap_or_else(|e| {
                panic!("load_chunk failed on {archive_name}:{}: {e}", script.name)
            });
            vm.load_proto(proto).unwrap_or_else(|e| {
                panic!(
                    "Vm::load_proto failed on {archive_name}:{}: {e}",
                    script.name
                )
            });
            total_loaded += 1;
        }
        eprintln!("{archive_name}: loaded {count} scripts");
    }
    assert!(
        total_loaded >= 250,
        "expected >= 250 total scripts across all 4 archives, got {total_loaded}"
    );
}

/// Optional directory of expected manifests (`<script>.json`, as printed by
/// `bfe-script-tool manifest mission.lvl <script>`) made from the same install.
fn expected_manifest_dir() -> Option<PathBuf> {
    std::env::var_os("BFE_SWBF2_MANIFESTS").map(PathBuf::from)
}

#[test]
#[ignore = "requires BFE_SWBF2_DIR pointing to a local Battlefront II (2005) install"]
fn record_every_mission_manifest() {
    let Some(lvl_dir) = resolve_lvl_dir() else {
        eprintln!("BFE_SWBF2_DIR not set; skipping retail manifest test");
        return;
    };
    let expected_dir = expected_manifest_dir();

    let data = read_archive(&lvl_dir, "mission.lvl");
    let archive = ScriptArchive::parse(&data).expect("parse mission.lvl");

    let mut recorded = 0usize;
    let mut compared = 0usize;
    for script in archive.scripts() {
        let name = &script.name;
        let manifest = record_manifest(&archive, name)
            .unwrap_or_else(|e| panic!("record_manifest failed for {name}: {e}"));
        let repeat = record_manifest(&archive, name)
            .unwrap_or_else(|e| panic!("repeat record_manifest failed for {name}: {e}"));
        assert_eq!(manifest, repeat, "{name} recording must be deterministic");
        assert_eq!(manifest.schema, MANIFEST_SCHEMA_VERSION);
        assert_eq!(manifest.profile, "pc-fresh");
        assert_eq!(&manifest.script, name);
        assert_eq!(manifest.phases[0], "chunk");

        // Every stream handle appended to came from an earlier `OpenAudioStream`
        // call of the same recording.
        let mut open_streams = 0usize;
        for call in &manifest.calls {
            if call.name == "OpenAudioStream" {
                open_streams += 1;
            } else if call.name == "AudioStreamAppendSegments" {
                match call.args.last() {
                    Some(RecordedValue::Number(n)) if *n >= 1.0 && (*n as usize) <= open_streams => {}
                    other => panic!("{name}: appended to stream handle {other:?} with {open_streams} open"),
                }
            }
        }
        for query in &manifest.unresolved_queries {
            assert!(query.call_index < manifest.calls.len());
            assert_eq!(manifest.calls[query.call_index].name, query.name);
        }

        let json = manifest.to_json_pretty();
        assert_eq!(json, repeat.to_json_pretty(), "{name} JSON must be byte-identical");
        let parsed: serde_json::Value = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("{name} emitted invalid JSON: {e}"));
        assert_eq!(parsed["schema"], MANIFEST_SCHEMA_VERSION);
        assert_eq!(parsed["calls"].as_array().unwrap().len(), manifest.calls.len());

        if let Some(dir) = &expected_dir {
            let path = dir.join(format!("{name}.json"));
            let expected = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("missing expected manifest {}: {e}", path.display()));
            assert!(
                json == expected,
                "{name}: manifest differs from {} (ordered calls and arguments)",
                path.display()
            );
            compared += 1;
        }
        recorded += 1;
    }
    assert!(recorded >= 100, "expected at least 100 mission scripts, got {recorded}");
    eprintln!("mission.lvl: recorded {recorded} scripts, compared {compared} with expected manifests");
}
