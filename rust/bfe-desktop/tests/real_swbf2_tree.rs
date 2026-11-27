//! A real prepared 2005 tree through verification, export, import and relocation.
//!
//! Needs a content folder holding a prepared `swbf2/.content` (`BFE_SWBF2_PREPARED`), the
//! copy of the game it was prepared from (`BFE_SWBF2_GAME_DIR`), the preparation tools that
//! prepared it (`BFE_SWBF2_TOOLS`, for their `content-version.json`) and an empty folder to
//! write into (`BFE_SWBF2_SCRATCH`); it is skipped when they are not set. It only reads the
//! prepared tree, the tools and the copy.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use bfe_content::layout::{family_dir, SWBF2_MARKER_FILE, SWBF2_PREPARE_STATE_FILE};
use bfe_desktop::archive::{
    export_content_for, import_content, import_content_for, relocate_preparation_for, RECEIPT_NAME,
};
use bfe_desktop::prepare::verify_prepared_outputs_for;
use bfe_desktop::swbf2_tree::{self, Swbf2Record};
use bfe_family::GameFamily;

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// The tree's machinery at its top, written out here rather than taken from the exporter:
/// the tools' lock and per-step locks, the marker and the import receipt.
const MACHINERY: [&str; 4] = [".prepare.lock", ".locks", ".bfe-family.json", ".bfe-import.json"];

/// Every regular file under `dir` (relative, `/`-separated) but the machinery.
fn tree_files(dir: &Path) -> BTreeSet<String> {
    fn walk(base: &Path, at: &Path, out: &mut BTreeSet<String>) {
        for entry in fs::read_dir(at).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            let rel = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            assert!(!kind.is_symlink(), "{rel} is a link");
            if at == base && MACHINERY.contains(&rel.as_str()) {
                continue;
            }
            if kind.is_dir() {
                walk(base, &path, out);
            } else {
                out.insert(rel);
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(dir, dir, &mut out);
    out
}

#[test]
#[ignore]
fn a_real_2005_tree_round_trips_byte_for_byte() {
    let (Some(prepared), Some(game), Some(tools_dir), Some(scratch)) = (
        env_path("BFE_SWBF2_PREPARED"),
        env_path("BFE_SWBF2_GAME_DIR"),
        env_path("BFE_SWBF2_TOOLS"),
        env_path("BFE_SWBF2_SCRATCH"),
    ) else {
        eprintln!("skipped: BFE_SWBF2_PREPARED, BFE_SWBF2_GAME_DIR, BFE_SWBF2_TOOLS and BFE_SWBF2_SCRATCH are not set");
        return;
    };
    let platform = if cfg!(windows) { "win32" } else { "linux" };
    let tools = swbf2_tree::swbf2_tools(Some(&tools_dir)).unwrap();
    let install = swbf2_tree::identify_install(&game).unwrap();
    let tree = swbf2_tree::locate(&prepared).unwrap();
    assert_eq!(
        swbf2_tree::inspect(&tree.dir, &tools, Some(&install)).unwrap(),
        Swbf2Record::Complete
    );
    assert_eq!(swbf2_tree::inspect(&tree.dir, &tools, None).unwrap(), Swbf2Record::Complete);
    assert!(verify_prepared_outputs_for(&prepared, &game, GameFamily::Swbf2, platform, Some(&tools_dir)).unwrap());

    // The export lists every file of the tree but its machinery.
    let listed: Vec<_> = swbf2_tree::content_files(&tree.dir).unwrap();
    let names: BTreeSet<String> = listed.iter().map(|f| f.0.clone()).collect();
    assert_eq!(names, tree_files(&tree.dir));
    let bytes: u64 = listed.iter().map(|f| f.2).sum();
    let scripts = names.iter().filter(|n| n.starts_with("scripts/")).count();
    println!("tree: {} files, {bytes} bytes, {scripts} under scripts/", names.len());

    fs::create_dir_all(&scratch).unwrap();
    let archive = scratch.join("tree.bfecontent");
    let summary = export_content_for(&prepared, GameFamily::Swbf2, &archive, Some(&tools_dir)).unwrap();
    assert_eq!(summary.files, names.len());
    assert_eq!(summary.bytes, bytes);
    println!(
        "export: {} files, {} bytes, archive {} bytes, maps {:?}",
        summary.files, summary.bytes, summary.archive_bytes, summary.maps
    );

    // The 2004 import declines it as the released client does.
    assert_eq!(
        import_content(&archive, &scratch.join("imports-2004")).unwrap_err(),
        format!("Not prepared game content: {SWBF2_PREPARE_STATE_FILE}")
    );

    let imported =
        import_content_for(&archive, GameFamily::Swbf2, &scratch.join("imports"), Some(&tools_dir)).unwrap();
    let copy = family_dir(&imported.content_dir, GameFamily::Swbf2);
    assert_eq!(tree_files(&copy), names);
    for (rel, source, _, _) in &listed {
        assert!(
            fs::read(copy.join(rel)).unwrap() == fs::read(source).unwrap(),
            "{rel} differs after the import"
        );
    }
    let machinery: BTreeSet<String> = fs::read_dir(&copy)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.') && n != SWBF2_PREPARE_STATE_FILE)
        .collect();
    assert_eq!(
        machinery,
        BTreeSet::from([SWBF2_MARKER_FILE.to_string(), RECEIPT_NAME.to_string()])
    );
    println!("import: {} files byte for byte", names.len());

    // Moved onto the same copy of the game, it verifies again.
    let moved =
        relocate_preparation_for(&imported.content_dir, GameFamily::Swbf2, &game, Some(&tools_dir)).unwrap();
    assert!(moved.ok, "{moved:?}");
    assert!(verify_prepared_outputs_for(&imported.content_dir, &game, GameFamily::Swbf2, platform, Some(&tools_dir)).unwrap());
    // Only the record's `game` may have changed spelling; every other file is as exported.
    for (rel, source, _, _) in &listed {
        if rel != SWBF2_PREPARE_STATE_FILE {
            assert!(fs::read(copy.join(rel)).unwrap() == fs::read(source).unwrap(), "{rel}");
        }
    }
    println!("relocated and verified");
}
