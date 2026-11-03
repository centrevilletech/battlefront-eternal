//! The Rust loaders against the JavaScript's, on the game's own files: with
//! BFE_CONTENT_DIR set, tools/content-dump.mjs dumps every battle of every
//! map (each side it fields played first) from the JavaScript loaders, and
//! this crate's dump of each must be the same text, line for line. Without
//! the files (CI has none) the test passes having checked nothing, as the
//! repository's other content tests skip.
//!
//! BFE_NODE names the Node that runs the JavaScript (the dedicated server's
//! Node 24.22 is the reference; default `node`); BFE_CONTENT_MAP limits the
//! run to one map.

use bfe_content::battle::Loader;
use bfe_content::battle_dump::{dump_planned, PayloadCache};
use bfe_content::catalog::plan;
use bfe_content::files::DirFiles;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn rust_loaders_match_the_javascript() {
    let Some(root) = std::env::var_os("BFE_CONTENT_DIR").map(PathBuf::from).filter(|r| r.join("maps").is_dir()) else {
        eprintln!("skipped: no game files (BFE_CONTENT_DIR)");
        return;
    };
    let only_map = std::env::var("BFE_CONTENT_MAP").ok();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::env::temp_dir().join(format!("content-dump-test-{}", std::process::id()));
    let js = out.join("js");
    let node = std::env::var("BFE_NODE").unwrap_or_else(|_| "node".into());
    let mut command = Command::new(&node);
    command.arg(repo.join("tools/content-dump.mjs")).arg("--out").arg(&js).env("BFE_CONTENT_DIR", &root).current_dir(&repo);
    if let Some(map) = &only_map {
        command.arg("--map").arg(map);
    }
    let status = command.status().expect("node runs tools/content-dump.mjs");
    assert!(status.success(), "tools/content-dump.mjs failed");

    let list = plan(&root, only_map.as_deref(), None, None);
    assert!(!list.is_empty(), "no battles in the content");
    let files = DirFiles::new(&root);
    let loader = Loader::new(&files);
    let mut cache = PayloadCache::default();
    let mut differing = Vec::new();
    for p in &list {
        let rust = dump_planned(&loader, p, &mut cache, false);
        let javascript = std::fs::read_to_string(js.join(format!("{}.txt", p.name()))).unwrap_or_default();
        if rust != javascript {
            let line = rust.lines().zip(javascript.lines()).position(|(a, b)| a != b).unwrap_or_else(|| rust.lines().count().min(javascript.lines().count()));
            differing.push(format!(
                "{}: line {}\n  rust: {}\n  js:   {}",
                p.name(),
                line + 1,
                rust.lines().nth(line).unwrap_or("(end)"),
                javascript.lines().nth(line).unwrap_or("(end)")
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&out);
    assert!(differing.is_empty(), "{} of {} battles differ:\n{}", differing.len(), list.len(), differing.join("\n"));
    eprintln!("{} battles identical", list.len());
}
