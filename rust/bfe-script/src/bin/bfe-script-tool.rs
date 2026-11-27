//! CLI tool for inspecting Battlefront II (2005) `.lvl` script archives and
//! recording mission initialization manifests as JSON.
//!
//! Usage:
//!
//! ```text
//! bfe-script-tool list <archive.lvl>
//! bfe-script-tool manifest [--global NAME=VALUE]... [--answer NAME=VALUE]... <mission.lvl> <script>
//! ```
//!
//! `--global` sets a global before the chunk runs; `--answer` makes a host call return
//! a value (recorded, not listed as unresolved). VALUE is `nil`, `true`, `false`, a
//! number, or any other text as a string.

#![forbid(unsafe_code)]

use std::env;
use std::fs::File;
use std::io::Read;
use std::process;

use bfe_script::archive::{ArchiveLimits, ScriptArchive};
use bfe_script::recorder::{
    record_manifest_with_config, ProfileAnswer, RecordedValue, RecorderLimits, RecorderOptions,
};

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  bfe-script-tool list <archive.lvl>");
    eprintln!(
        "  bfe-script-tool manifest [--global NAME=VALUE]... [--answer NAME=VALUE]... <mission.lvl> <script>"
    );
}

/// `NAME=VALUE` with VALUE `nil`, `true`, `false`, a number, or text.
fn parse_answer(arg: &str) -> Result<(String, RecordedValue), String> {
    let (name, value) = arg
        .split_once('=')
        .ok_or_else(|| format!("expected NAME=VALUE, got '{arg}'"))?;
    if name.is_empty() {
        return Err(format!("empty name in '{arg}'"));
    }
    let value = match value {
        "nil" => RecordedValue::Nil,
        "true" => RecordedValue::Bool(true),
        "false" => RecordedValue::Bool(false),
        v => match v.parse::<f32>() {
            Ok(n) if v.chars().any(|c| c.is_ascii_digit()) => RecordedValue::Number(n),
            _ => RecordedValue::String(v.to_string()),
        },
    };
    Ok((name.to_string(), value))
}

fn read_bounded_file(path: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| format!("failed to open '{path}': {e}"))?;
    if let Ok(meta) = file.metadata() {
        if meta.len() > max_bytes as u64 {
            return Err(format!(
                "file '{path}' is {} bytes, exceeding limit ({max_bytes} bytes)",
                meta.len()
            ));
        }
    }
    let mut data = Vec::new();
    file.take((max_bytes as u64).saturating_add(1))
        .read_to_end(&mut data)
        .map_err(|e| format!("failed to read '{path}': {e}"))?;
    if data.len() > max_bytes {
        return Err(format!(
            "file '{path}' exceeded limit ({max_bytes} bytes)"
        ));
    }
    Ok(data)
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return Err("missing subcommand".to_string());
    }
    let archive_limits = ArchiveLimits::default();
    let recorder_limits = RecorderLimits::default();
    match args[0].as_str() {
        "list" => {
            if args.len() != 2 {
                print_usage();
                return Err("`list` expects exactly one argument: <archive.lvl>".to_string());
            }
            let data = read_bounded_file(&args[1], archive_limits.max_input_bytes)?;
            let archive = ScriptArchive::parse_with_limits(&data, archive_limits)
                .map_err(|e| format!("failed to parse '{}': {e}", args[1]))?;
            for script in archive.scripts() {
                println!("{}", script.name);
            }
            Ok(())
        }
        "manifest" => {
            let mut options = RecorderOptions::default();
            let mut positional = Vec::new();
            let mut it = args[1..].iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--global" | "--answer" => {
                        let spec = it
                            .next()
                            .ok_or_else(|| format!("`{a}` expects NAME=VALUE"))?;
                        let (name, value) = parse_answer(spec)?;
                        options.answers.push(if a == "--global" {
                            ProfileAnswer::Global { name, value }
                        } else {
                            ProfileAnswer::Call { name, value }
                        });
                    }
                    _ => positional.push(a.clone()),
                }
            }
            if positional.len() != 2 {
                print_usage();
                return Err(
                    "`manifest` expects two arguments: <mission.lvl> <script>".to_string()
                );
            }
            let data = read_bounded_file(&positional[0], archive_limits.max_input_bytes)?;
            let archive = ScriptArchive::parse_with_limits(&data, archive_limits)
                .map_err(|e| format!("failed to parse '{}': {e}", positional[0]))?;
            let manifest = record_manifest_with_config(&archive, &positional[1], options)
                .map_err(|e| format!("failed to record '{}': {e}", positional[1]))?;
            let json = manifest
                .to_json_pretty_bounded(recorder_limits.max_output_bytes)
                .map_err(|e| format!("failed to serialize manifest for '{}': {e}", positional[1]))?;
            print!("{json}");
            Ok(())
        }
        other => {
            print_usage();
            Err(format!("unknown subcommand '{other}'"))
        }
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        process::exit(1);
    }
}
