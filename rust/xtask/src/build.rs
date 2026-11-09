// Code generator driver (`build.rs`) for `cargo xtask build` and `cargo xtask check`.
//
// Runs all schema, layout, and shader catalog generators in `xtask` across the
// native Rust workspace (`sim`, `bfe-render`, `bfe-presentation`, `bfe-environment`,
// `bfe-transport`, `bfe-client`, `bfe-pose`, `audio`, `audio-control`, and
// `audio-decode`). When `--check` is passed (or via `cargo xtask check`),
// verifies that every committed generated Rust and JSON artifact on disk matches
// the generator output byte-for-byte.

use std::path::Path;

use crate::layouts;
use crate::shaders;
use crate::sim_schema_js;

// Supported target group names for `cargo xtask build [--check] [target...]`.
const ARTIFACT_NAMES: &[&str] = &[
    "sim",
    "audio",
    "environment",
    "codec",
    "presentation",
    "renderer",
    "images",
    "textures",
];

// Runs the schema, layout, and shader generators for the selected targets (or
// all targets when none are specified).
pub fn run_build(root: &Path, args: &[String]) -> Result<(), String> {
    let check = args.iter().any(|a| a == "--check");

    layouts::transport_schema(root, check)?;
    shaders::render_shaders(root, check)?;

    let mut names: Vec<String> = Vec::new();
    for arg in args {
        if arg == "--check" {
            continue;
        }
        let mapped = if arg == "rays" || arg == "anim" {
            "sim"
        } else {
            arg.as_str()
        };
        if !names.iter().any(|n| n == mapped) {
            names.push(mapped.to_string());
        }
    }
    if names.iter().any(|n| n == "renderer") {
        for extra in ["textures", "images"] {
            if !names.iter().any(|n| n == extra) {
                names.push(extra.to_string());
            }
        }
    }
    for name in &names {
        if !ARTIFACT_NAMES.contains(&name.as_str()) {
            return Err(format!(
                "usage: cargo xtask build [--check] [{}]",
                ARTIFACT_NAMES.join("] [")
            ));
        }
    }

    let selected: Vec<&str> = if names.is_empty() {
        ARTIFACT_NAMES.to_vec()
    } else {
        names.iter().map(|s| s.as_str()).collect()
    };

    let mut problems: Vec<String> = Vec::new();
    for name in selected {
        if name == "sim" {
            if let Err(e) = sim_schema_js::run_sim_schema(root, check) {
                eprintln!("{e}");
                problems.push("the store's generated code is stale: cargo xtask schema".to_string());
            }
        }
        if name == "sim" || name == "presentation" || name == "renderer" {
            if let Err(e) = layouts::presentation_schema(root, check) {
                eprintln!("{e}");
                return Err("presentation schema is stale".to_string());
            }
        }
        if name == "audio" {
            layouts::audio_schema(root, check)?;
        }
        if name == "environment" {
            if let Err(e) = layouts::environment_schema(root, check) {
                eprintln!("{e}");
                return Err(
                    "The destruction and weather layout is stale: cargo xtask environment-schema"
                        .to_string(),
                );
            }
        }
        if name == "codec" {
            layouts::audio_codec_schema(root, check)?;
        }
    }

    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    if check {
        println!("All generated Rust schemas, layouts, and shader tables match.");
    }
    Ok(())
}
