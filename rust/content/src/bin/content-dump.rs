//! The Rust side of the content dump: every battle of every map, each
//! fielded side played first, written as tools/content-dump.mjs writes the
//! JavaScript's (src/dump.rs for the format).
//!
//!   content-dump [--map nab2] [--mission nab2c] [--side rep] --out DIR
//!
//! The content is BFE_CONTENT_DIR (or public/ beside the repository's rust/).

use bfe_content::battle::Loader;
use bfe_content::battle_dump::{dump_planned, PayloadCache};
use bfe_content::catalog::plan;
use bfe_content::files::DirFiles;
use std::path::PathBuf;

fn option(args: &[String], name: &str) -> Option<String> {
    let flag = format!("--{name}");
    args.iter().position(|a| *a == flag).and_then(|i| args.get(i + 1)).cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = PathBuf::from(std::env::var("BFE_CONTENT_DIR").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../public").to_string()
    }));
    let out = PathBuf::from(option(&args, "out").unwrap_or_else(|| std::env::temp_dir().join("content-dump-rust").to_string_lossy().into_owned()));
    std::fs::create_dir_all(&out).expect("output folder");
    let full = args.iter().any(|a| a == "--full");
    if args.iter().any(|a| a == "--inline") {
        bfe_content::dump::INLINE_ALL.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let list = plan(&root, option(&args, "map").as_deref(), option(&args, "mission").as_deref(), option(&args, "side").as_deref());
    let files = DirFiles::new(&root);
    let loader = Loader::new(&files);
    let mut cache = PayloadCache::default();
    let started = std::time::Instant::now();
    for p in &list {
        let text = dump_planned(&loader, p, &mut cache, full);
        std::fs::write(out.join(format!("{}.txt", p.name())), text).expect("dump written");
    }
    println!("{} battles in {:.1} s -> {}", list.len(), started.elapsed().as_secs_f64(), out.display());
}
