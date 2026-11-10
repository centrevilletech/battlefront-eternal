//! Exports and imports prepared content archives from the command line, in the browser
//! build's archive form: a way to move a content folder between the two builds.
//!
//! `content_archive export <content-dir> <archive>` writes an archive;
//! `content_archive import <archive> <parent-dir>` extracts it into `<parent-dir>/content-<id>`
//! and prints that path. Progress goes to stderr about once a second.
use std::path::Path;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(action), Some(a), Some(b)) = (args.get(1), args.get(2), args.get(3)) else {
        eprintln!(
            "usage: content_archive export <content-dir> <archive> | import <archive> <parent-dir>"
        );
        std::process::exit(2);
    };
    let mut last = Instant::now();
    let mut report = move |phase: &str, file: &str, done: u64, total: u64| {
        if last.elapsed() >= Duration::from_secs(1) {
            last = Instant::now();
            eprintln!("{phase} {file} {done}/{total}");
        }
        Ok::<(), String>(())
    };
    let result = match action.as_str() {
        "export" => bfe_desktop::archive::export_content_with_progress(
            Path::new(a),
            Path::new(b),
            None,
            &mut report,
        )
        .map(|r| {
            format!(
                "{} files, {} bytes, {}",
                r.files,
                r.bytes,
                r.archive_path.display()
            )
        }),
        "import" => bfe_desktop::archive::import_content_with_progress(
            Path::new(a),
            Path::new(b),
            &mut report,
        )
        .map(|r| r.content_dir.display().to_string()),
        _ => Err(format!("unknown action {action}")),
    };
    match result {
        Ok(line) => println!("{line}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
