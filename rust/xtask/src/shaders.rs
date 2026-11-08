// Renderer layout and shader catalog generator (`shaders.rs`).
//
// Why this module exists:
// Ports `tools/render-shaders.mjs` (`generate`) and the nine renderer boundary
// layout generators it invokes before scanning shader manifests:
// 1. `actor_layout`              -> `tools/render-actor-layout.mjs` (`generate`)
// 2. `interface_resource_layout` -> `tools/render-interface-resource-layout.mjs` (`generate`)
// 3. `line_layout`               -> `tools/render-line-layout.mjs` (`generate`)
// 4. `interface_layout`          -> `tools/render-interface-layout.mjs` (`generate`)
// 5. `static_layout`             -> `tools/render-static-layout.mjs` (`generate`)
// 6. `effect_layout`             -> `tools/render-effect-layout.mjs` (`generate`)
// 7. `post_layout`               -> `tools/render-post-layout.mjs` (`generate`)
// 8. `frame_layout`              -> `tools/render-frame-layout.mjs` (`generate`)
// 9. `camera_layout`             -> `tools/render-camera-layout.mjs` (`generate`)
//
// Byte-identical formatting and ordering rules across the renderer generators:
// - `constant(s)` ports `s.replace(/[A-Z]/g, c => '_' + c).toUpperCase()`, which
//   prefixes `_` before every uppercase letter (including index 0 if uppercase).
// - Pretty vs. compact JSON in generated JS modules matches each original script:
//   `actor-layout.js`, `interface-resource-layout.js`, `line-layout.js`,
//   `interface-layout.js`, `static-layout.js`, `effect-layout.js` (`EFFECT` and
//   `EFFECT_TARGET`), and `src/render-programs.js` use 2-space `to_json_pretty()`;
//   `EFFECT_INPUTS`, `post-layout.js`, `frame-layout.js`, `camera-layout.js`,
//   `actor-selectors.json`, and `world-selectors.json` use single-line
//   `to_json_compact()`.
// - Spacing around `:` and `=` in generated Rust `pub const` items is preserved per
//   file (e.g. `pub const {key}:{ty}={v};` without spaces in `actor_layout.rs`,
//   `interface_resource_layout.rs`, and `line_layout.rs`).
// - V8 property insertion order is preserved when mutating existing keys in place
//   (notably `facts.LEN` in `post_layout` and `shadowTwin` in `render_shaders`).

use crate::json::{quote_str, JsonValue};
use crate::layouts::write_or_check;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

// Ports `const upper = s => s.replace(/[A-Z]/g, c => '_' + c).toUpperCase()` (also
// named `constant` in `render-post-layout.mjs`, `render-frame-layout.mjs`, and
// `render-camera-layout.mjs`).
//
// Formatting rule: unlike `upper_camel` in `layouts.rs`, this unconditionally
// inserts `_` before every ASCII uppercase character.
fn constant(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c);
        } else {
            out.push(c.to_ascii_uppercase());
        }
    }
    out
}

// Ports `name.replace(/[A-Z]/g, c => '_' + c.toLowerCase())` used for Rust module
// names in `tools/render-actor-layout.mjs` and `tools/render-interface-resource-layout.mjs`.
fn snake_mod(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

// Reads and parses a JSON file relative to the repository root `root`.
fn read_json(root: &Path, rel: &str) -> Result<JsonValue, String> {
    let text = fs::read_to_string(root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
    JsonValue::parse(&text).map_err(|e| format!("{rel}: {e}"))
}

// Ports `generate(check)` from `tools/render-actor-layout.mjs`.
//
// Why this generator exists:
// Actor appearance and source handles cross a generated numeric boundary between
// `src/rust-render/actor-layout.js` and `rust/bfe-render/src/actor_layout.rs`
// defined by `rust/bfe-render/actor-layout.json`.
//
// Ordering and formatting rules:
// - Sections `"limit"`, `"kind"`, and `"crowd"` emit direct values (`size`) without
//   a trailing `LEN`; all other sections pack running offsets `at` and append `LEN`.
// - Negative values (such as sentinel handle constants) emit `:i32=` in Rust;
//   non-negative values emit `:usize=`, with no spaces around `:` or `=` and no
//   space before `{` in `export const ACTOR={};`.
fn actor_layout(root: &Path, check: bool) -> Result<(), String> {
    let schema = read_json(root, "rust/bfe-render/actor-layout.json")?;
    let mut layout = Vec::new();
    let mut rust = Vec::new();
    for (name, fields_val) in schema.as_object().ok_or("actor-layout must be object")? {
        let mut at: i64 = 0;
        let mut row = Vec::new();
        let is_direct = name == "limit" || name == "kind" || name == "crowd";
        for (key, size_val) in fields_val.as_object().ok_or("fields must be object")? {
            let size = size_val.as_i64().ok_or("size must be int")?;
            let val = if is_direct { size } else { at };
            row.push((constant(key), JsonValue::Number(val as f64)));
            if !is_direct {
                at += size;
            }
        }
        if !is_direct {
            row.push(("LEN".to_string(), JsonValue::Number(at as f64)));
        }
        rust.push(format!("pub mod {} {{", snake_mod(name)));
        for (key, value) in &row {
            let v = value.as_i64().unwrap();
            let ty = if v < 0 { "i32" } else { "usize" };
            rust.push(format!("    pub const {key}:{ty}={v};"));
        }
        rust.push("}".to_string());
        layout.push((constant(name), JsonValue::Object(row)));
    }
    let notice = "// Generated by tools/render-actor-layout.mjs; edit actor-layout.json.\n";
    let js = format!("{notice}export const ACTOR={};\n", JsonValue::Object(layout).to_json_pretty());
    let rs = format!("{notice}{}\n", rust.join("\n"));
    write_or_check(root, "src/rust-render/actor-layout.js", &js, check, "Stale src/rust-render/actor-layout.js")?;
    write_or_check(root, "rust/bfe-render/src/actor_layout.rs", &rs, check, "Stale rust/bfe-render/src/actor_layout.rs")?;
    Ok(())
}

// Ports `generate(check)` from `tools/render-interface-resource-layout.mjs`.
//
// Why this generator exists:
// Interface appearance, stream, and source handles share one numeric boundary
// (`src/rust-render/interface-resource-layout.js` and
// `rust/bfe-render/src/interface_resource_layout.rs`) generated from
// `rust/bfe-render/interface-resource-layout.json`.
//
// Ordering and formatting rules:
// - Sections `"limit"` and `"kind"` store direct constants; all other sections pack
//   running offsets and append `LEN`.
// - Rust constants use compact `pub const {key}:{ty}={v};` formatting (`i32` when
//   negative, `usize` otherwise), and JS uses `export const UI_RESOURCE=<pretty>;`.
fn interface_resource_layout(root: &Path, check: bool) -> Result<(), String> {
    let schema = read_json(root, "rust/bfe-render/interface-resource-layout.json")?;
    let mut layout = Vec::new();
    let mut rust = Vec::new();
    for (name, fields_val) in schema.as_object().ok_or("interface-resource-layout must be object")? {
        let mut at: i64 = 0;
        let mut row = Vec::new();
        let is_direct = name == "limit" || name == "kind";
        for (key, size_val) in fields_val.as_object().ok_or("fields must be object")? {
            let size = size_val.as_i64().ok_or("size must be int")?;
            let val = if is_direct { size } else { at };
            row.push((constant(key), JsonValue::Number(val as f64)));
            if !is_direct {
                at += size;
            }
        }
        if !is_direct {
            row.push(("LEN".to_string(), JsonValue::Number(at as f64)));
        }
        rust.push(format!("pub mod {} {{", snake_mod(name)));
        for (key, value) in &row {
            let v = value.as_i64().unwrap();
            let ty = if v < 0 { "i32" } else { "usize" };
            rust.push(format!("    pub const {key}:{ty}={v};"));
        }
        rust.push("}".to_string());
        layout.push((constant(name), JsonValue::Object(row)));
    }
    let notice = "// Generated by tools/render-interface-resource-layout.mjs; edit interface-resource-layout.json.\n";
    let js = format!("{notice}export const UI_RESOURCE={};\n", JsonValue::Object(layout).to_json_pretty());
    let rs = format!("{notice}{}\n", rust.join("\n"));
    write_or_check(root, "src/rust-render/interface-resource-layout.js", &js, check, "Stale src/rust-render/interface-resource-layout.js")?;
    write_or_check(root, "rust/bfe-render/src/interface_resource_layout.rs", &rs, check, "Stale rust/bfe-render/src/interface_resource_layout.rs")?;
    Ok(())
}

// Ports `generate(check)` from `tools/render-line-layout.mjs`.
//
// Why this generator exists:
// Line publication carries live positions and placement across a shared numeric
// layout (`src/rust-render/line-layout.js` and `rust/bfe-render/src/line_layout.rs`)
// while GPU policy stays native.
//
// Ordering and formatting rules:
// - `"limit"` and `"kind"` sections emit direct values; other sections pack offsets
//   and append `LEN`.
// - Rust module names use `name` verbatim (`pub mod {name}`) and compact
//   `pub const {key}:usize={v};` lines.
fn line_layout(root: &Path, check: bool) -> Result<(), String> {
    let schema = read_json(root, "rust/bfe-render/line-layout.json")?;
    let mut layout = Vec::new();
    let mut rust = Vec::new();
    for (name, fields_val) in schema.as_object().ok_or("line-layout must be object")? {
        let mut at: i64 = 0;
        let mut row = Vec::new();
        let is_direct = name == "limit" || name == "kind";
        for (key, size_val) in fields_val.as_object().ok_or("fields must be object")? {
            let size = size_val.as_i64().ok_or("size must be int")?;
            let val = if is_direct { size } else { at };
            row.push((constant(key), JsonValue::Number(val as f64)));
            if !is_direct {
                at += size;
            }
        }
        if !is_direct {
            row.push(("LEN".to_string(), JsonValue::Number(at as f64)));
        }
        rust.push(format!("pub mod {name} {{"));
        for (key, value) in &row {
            let v = value.as_i64().unwrap();
            rust.push(format!("    pub const {key}:usize={v};"));
        }
        rust.push("}".to_string());
        layout.push((constant(name), JsonValue::Object(row)));
    }
    let notice = "// Generated by tools/render-line-layout.mjs; edit line-layout.json.\n";
    let js = format!("{notice}export const LINE={};\n", JsonValue::Object(layout).to_json_pretty());
    let rs = format!("{notice}{}\n", rust.join("\n"));
    write_or_check(root, "src/rust-render/line-layout.js", &js, check, "Stale src/rust-render/line-layout.js")?;
    write_or_check(root, "rust/bfe-render/src/line_layout.rs", &rs, check, "Stale rust/bfe-render/src/line_layout.rs")?;
    Ok(())
}

// Ports `generate(check)` from `tools/render-interface-layout.mjs`.
//
// Why this generator exists:
// One declaration (`rust/bfe-render/interface-layout.json`) owns the menu/spawn UI
// draw record on both sides of WASM (`src/rust-render/interface-layout.js` and
// `rust/bfe-render/src/interface_layout.rs`).
//
// Ordering and formatting rules:
// - Emits `MAX_DRAWS`, `MAX_UNIFORMS`, and `UNIFORM` first, followed by sequential
//   field indices `0..fields.length`, and finally `STRIDE = VALUES + MAX_UNIFORMS * UNIFORM`.
fn interface_layout(root: &Path, check: bool) -> Result<(), String> {
    let schema = read_json(root, "rust/bfe-render/interface-layout.json")?;
    let max_draws = schema.get("maxDraws").and_then(|v| v.as_usize()).ok_or("missing maxDraws")?;
    let max_uniforms = schema.get("maxUniforms").and_then(|v| v.as_usize()).ok_or("missing maxUniforms")?;
    let uniform_size = schema.get("uniformSize").and_then(|v| v.as_usize()).ok_or("missing uniformSize")?;
    let fields = schema.get("fields").and_then(|v| v.as_array()).ok_or("missing fields")?;

    let mut constants = vec![
        ("MAX_DRAWS".to_string(), JsonValue::Number(max_draws as f64)),
        ("MAX_UNIFORMS".to_string(), JsonValue::Number(max_uniforms as f64)),
        ("UNIFORM".to_string(), JsonValue::Number(uniform_size as f64)),
    ];
    let mut values_idx = 0usize;
    for (i, f) in fields.iter().enumerate() {
        let key = constant(f.as_str().ok_or("field must be string")?);
        if key == "VALUES" {
            values_idx = i;
        }
        constants.push((key, JsonValue::Number(i as f64)));
    }
    let stride = values_idx + max_uniforms * uniform_size;
    constants.push(("STRIDE".to_string(), JsonValue::Number(stride as f64)));

    let notice = "// Generated by tools/render-interface-layout.mjs; edit interface-layout.json instead.\n";
    let js = format!("{notice}export const UI = {};\n", JsonValue::Object(constants.clone()).to_json_pretty());
    let rs_lines: Vec<String> = constants.iter().map(|(k, v)| format!("pub const {k}: usize = {};", v.as_usize().unwrap())).collect();
    let rs = format!("{notice}{}\n", rs_lines.join("\n"));
    write_or_check(root, "src/rust-render/interface-layout.js", &js, check, "Stale src/rust-render/interface-layout.js")?;
    write_or_check(root, "rust/bfe-render/src/interface_layout.rs", &rs, check, "Stale rust/bfe-render/src/interface_layout.rs")?;
    Ok(())
}

// Ports `generate(check)` from `tools/render-static-layout.mjs`.
//
// Why this generator exists:
// Cold static-resource transport shares one generated layout between
// `src/rust-render/static-layout.js` and `rust/bfe-render/src/static_layout.rs`
// from `rust/bfe-render/static-layout.json`.
//
// Ordering and formatting rules:
// - JS receives nested objects (`STATIC.<GROUP>.<FIELD>`) formatted with `to_json_pretty()`.
// - Rust flattens the two levels into `pub const <GROUP>_<FIELD>: usize = <val>;`.
fn static_layout(root: &Path, check: bool) -> Result<(), String> {
    let schema = read_json(root, "rust/bfe-render/static-layout.json")?;
    let mut js_entries = Vec::new();
    let mut rs_lines = Vec::new();
    for (name, fields_val) in schema.as_object().ok_or("static-layout must be object")? {
        let mut inner = Vec::new();
        for (key, value) in fields_val.as_object().ok_or("fields must be object")? {
            inner.push((constant(key), value.clone()));
            rs_lines.push(format!("pub const {}_{}: usize = {};", constant(name), constant(key), value.as_usize().unwrap()));
        }
        js_entries.push((constant(name), JsonValue::Object(inner)));
    }
    let notice = "// Generated by tools/render-static-layout.mjs; edit static-layout.json instead.\n";
    let js = format!("{notice}export const STATIC = {};\n", JsonValue::Object(js_entries).to_json_pretty());
    let rs = format!("{notice}{}\n", rs_lines.join("\n"));
    write_or_check(root, "src/rust-render/static-layout.js", &js, check, "Stale src/rust-render/static-layout.js")?;
    write_or_check(root, "rust/bfe-render/src/static_layout.rs", &rs, check, "Stale rust/bfe-render/src/static_layout.rs")?;
    Ok(())
}

// Ports `generate(check)` from `tools/render-effect-layout.mjs`.
//
// Why this generator exists:
// Effect producer rows form a data boundary (`src/rust-render/effect-layout.js`,
// `rust/bfe-render/src/effect_layout.rs`, and `rust/bfe-environment/src/effect_layout.rs`)
// independent of GPU descriptors, combining `rust/bfe-render/effect-layout.json` and
// `rust/bfe-render/effect-targets.json`.
//
// Ordering and formatting rules:
// - Each effect entry starts with `ID: id`, followed by packed `values` offsets,
//   optional `CAPACITY` (only when `row.capacity` is truthy), `LENGTH: at`, and
//   `STRIDE` (sum of `row.attributes` widths).
// - In Rust, `pub const <NAME>: usize = <id>;` has spaces around `:` and `=`, whereas
//   optional `pub const <NAME>_CAPACITY:usize=<cap>;` has no spaces around `:` or `=`.
// - `rust/bfe-environment/src/effect_layout.rs` receives a `pub mod <snake_name>` per
//   effect containing `pub const <KEY>: usize = <value>;` for every field in `fields`.
// - `EFFECT` and `EFFECT_TARGET` use `to_json_pretty()`, while `EFFECT_INPUTS` uses
//   single-line `to_json_compact()`.
fn effect_layout(root: &Path, check: bool) -> Result<(), String> {
    let rows = read_json(root, "rust/bfe-render/effect-layout.json")?;
    let targets = read_json(root, "rust/bfe-render/effect-targets.json")?;
    let notice = "// Generated by tools/render-effect-layout.mjs; edit effect-layout.json instead.\n";
    let mut layouts = Vec::new();
    let mut rust = Vec::new();
    let mut environment = Vec::new();
    let mut producers = Vec::new();
    let mut effect_inputs = Vec::new();

    for (id, (name, row)) in rows.as_object().ok_or("effect-layout must be object")?.iter().enumerate() {
        let mut at: usize = 0;
        let mut fields = vec![("ID".to_string(), JsonValue::Number(id as f64))];
        let values = row.get("values").and_then(|v| v.as_object()).ok_or("missing values")?;
        for (key, size_val) in values {
            let size = size_val.as_usize().ok_or("size must be int")?;
            fields.push((constant(key), JsonValue::Number(at as f64)));
            at += size;
        }
        if let Some(cap) = row.get("capacity").and_then(|v| v.as_usize()) {
            fields.push(("CAPACITY".to_string(), JsonValue::Number(cap as f64)));
        }
        fields.push(("LENGTH".to_string(), JsonValue::Number(at as f64)));
        let attrs = row.get("attributes").and_then(|v| v.as_object()).ok_or("missing attributes")?;
        let stride: usize = attrs.iter().map(|(_, v)| v.as_usize().unwrap()).sum();
        fields.push(("STRIDE".to_string(), JsonValue::Number(stride as f64)));
        let env_lines: Vec<String> = fields.iter().map(|(key, value)| format!("    pub const {key}: usize = {};", value.as_usize().unwrap())).collect();
        environment.push(format!("pub mod {} {{\n{}\n}}", constant(name).to_lowercase(), env_lines.join("\n")));
        layouts.push((constant(name), JsonValue::Object(fields)));

        // Emit `PRODUCERS` attribute and value width slices without interior spaces.
        let pairs = |entries: &[(String, JsonValue)]| -> String {
            entries.iter().map(|(n, s)| format!("({},{})", quote_str(n), s.as_usize().unwrap())).collect::<Vec<_>>().join(",")
        };
        producers.push(format!("    (&[{}],&[{}]),", pairs(attrs), pairs(values)));
        rust.push(format!("pub const {}: usize = {id};", constant(name)));
        if let Some(cap) = row.get("capacity").and_then(|v| v.as_usize()) {
            rust.push(format!("pub const {}_CAPACITY:usize={cap};", constant(name)));
        }

        let attr_entries: Vec<JsonValue> = attrs.iter().map(|(k, v)| JsonValue::Array(vec![JsonValue::String(k.clone()), v.clone()])).collect();
        effect_inputs.push(JsonValue::Array(attr_entries));
    }

    let target_entries: Vec<(String, JsonValue)> = targets.as_object().ok_or("targets must be object")?.iter().map(|(k, v)| (constant(k), v.clone())).collect();
    let target_js = format!("export const EFFECT_TARGET = {};\n", JsonValue::Object(target_entries).to_json_pretty());
    let target_rust: String = targets.as_object().unwrap().iter().map(|(k, v)| format!("pub const TARGET_{}: usize = {};\n", constant(k), v.as_usize().unwrap())).collect();

    let js = format!(
        "{notice}export const EFFECT = {};\n{target_js}export const EFFECT_INPUTS = {};\n",
        JsonValue::Object(layouts).to_json_pretty(),
        JsonValue::Array(effect_inputs).to_json_compact()
    );
    let rs = format!(
        "{notice}{}\npub const PRODUCERS: &[(&[(&str,u32)],&[(&str,u32)])] = &[\n{}\n];\n{target_rust}",
        rust.join("\n"),
        producers.join("\n")
    );
    let env_rs = format!("{notice}{}\n", environment.join("\n"));
    write_or_check(root, "src/rust-render/effect-layout.js", &js, check, "Stale src/rust-render/effect-layout.js")?;
    write_or_check(root, "rust/bfe-render/src/effect_layout.rs", &rs, check, "Stale rust/bfe-render/src/effect_layout.rs")?;
    write_or_check(root, "rust/bfe-environment/src/effect_layout.rs", &env_rs, check, "Stale rust/bfe-environment/src/effect_layout.rs")?;
    Ok(())
}

// Ports `generate(check)` from `tools/render-post-layout.mjs`.
//
// Why this generator exists:
// Retained post-processing facts, temporal state, and post-chain shader uniform
// slots share one declared WASM layout (`src/rust-render/post-layout.js` and
// `rust/bfe-render/src/post_chain_layout.rs`) derived from
// `rust/bfe-render/post-chain-layout.json` and `rust/bfe-render/shaders/post-hud/manifest.json`.
//
// Ordering and formatting rules:
// - `offsets(fields)` packs `[name, size]` pairs and appends `LEN: at`.
// - In `facts`, `offsets(schema.facts)` creates `LEN` first, then `RESOURCES` and
//   `PROGRAMS` are appended after `LEN`, and `facts.LEN` is reassigned in place.
//   In V8, reassigning an existing property keeps `LEN` at its original slot before
//   `RESOURCES` and `PROGRAMS`, which we preserve by mutating the `LEN` entry in place.
// - Cross-checks each program key in `schema.programs` against `post-hud/manifest.json`
//   to emit `TYPES: [&[u32]; N]` and per-program uniform slot constants in `pub mod uniform`.
fn post_layout(root: &Path, check: bool) -> Result<(), String> {
    let schema = read_json(root, "rust/bfe-render/post-chain-layout.json")?;
    if schema.get("version").and_then(|v| v.as_usize()) != Some(1) {
        return Err("Unsupported post layout".into());
    }
    // Ports `const offsets = fields => ...` from `tools/render-post-layout.mjs`.
    let offsets = |fields_val: &JsonValue| -> Result<Vec<(String, JsonValue)>, String> {
        let mut out = Vec::new();
        let mut at: usize = 0;
        for item in fields_val.as_array().ok_or("fields must be array")? {
            let pair = item.as_array().ok_or("field entry must be pair")?;
            let name = pair[0].as_str().ok_or("name must be string")?;
            let size = pair[1].as_usize().ok_or("size must be int")?;
            out.push((constant(name), JsonValue::Number(at as f64)));
            at += size;
        }
        out.push(("LEN".to_string(), JsonValue::Number(at as f64)));
        Ok(out)
    };

    // Pack resource target ordinals, per-resource field indices, `STRIDE`, and `COUNT`.
    let res_list = schema.get("resources").and_then(|v| v.as_array()).ok_or("missing resources")?;
    let res_fields = schema.get("resourceFields").and_then(|v| v.as_array()).ok_or("missing resourceFields")?;
    let mut resources = Vec::new();
    for (i, r) in res_list.iter().enumerate() {
        resources.push((constant(r.as_str().unwrap()), JsonValue::Number(i as f64)));
    }
    for (i, rf) in res_fields.iter().enumerate() {
        let key = constant(rf.as_str().unwrap());
        if let Some(existing) = resources.iter_mut().find(|(k, _)| k == &key) {
            existing.1 = JsonValue::Number(i as f64);
        } else {
            resources.push((key, JsonValue::Number(i as f64)));
        }
    }
    let res_stride = res_fields.len();
    let res_count = res_list.len();
    resources.push(("STRIDE".to_string(), JsonValue::Number(res_stride as f64)));
    resources.push(("COUNT".to_string(), JsonValue::Number(res_count as f64)));

    // Preserve V8 property order on `facts`: `LEN` is created by `offsets()`, then
    // `RESOURCES` and `PROGRAMS` are appended, and `LEN` is updated in place.
    let mut facts = offsets(schema.get("facts").ok_or("missing facts")?)?;
    let state = offsets(schema.get("state").ok_or("missing state")?)?;
    let facts_len = facts.iter().find(|(k, _)| k == "LEN").unwrap().1.as_usize().unwrap();
    let facts_resources = facts_len;
    let programs_obj = schema.get("programs").and_then(|v| v.as_object()).ok_or("missing programs")?;
    let facts_programs = facts_resources + res_count * res_stride;
    let new_facts_len = facts_programs + programs_obj.len();
    facts.push(("RESOURCES".to_string(), JsonValue::Number(facts_resources as f64)));
    facts.push(("PROGRAMS".to_string(), JsonValue::Number(facts_programs as f64)));
    facts.iter_mut().find(|(k, _)| k == "LEN").unwrap().1 = JsonValue::Number(new_facts_len as f64);

    let mut programs = Vec::new();
    let mut keys = Vec::new();
    for (i, (name, key_val)) in programs_obj.iter().enumerate() {
        programs.push((constant(name), JsonValue::Number(i as f64)));
        keys.push(key_val.clone());
    }
    programs.push(("COUNT".to_string(), JsonValue::Number(keys.len() as f64)));

    // Resolve each post-chain program's uniform declarations from `post-hud/manifest.json`.
    let manifest = read_json(root, "rust/bfe-render/shaders/post-hud/manifest.json")?;
    let manifest_progs = manifest.get("programs").and_then(|v| v.as_array()).ok_or("missing programs in manifest")?;
    let mut declarations = Vec::new();
    for key_val in &keys {
        let key = key_val.as_str().unwrap();
        let row = manifest_progs
            .iter()
            .find(|p| p.get("key").and_then(|k| k.as_str()) == Some(key))
            .ok_or_else(|| format!("Missing post program {key}"))?;
        let uniforms = row.get("uniforms").and_then(|u| u.as_object()).ok_or("missing uniforms")?;
        declarations.push(uniforms.to_vec());
    }

    let map_const_obj = |val: &JsonValue| -> Vec<(String, JsonValue)> {
        val.as_object().unwrap().iter().map(|(k, v)| (constant(k), v.clone())).collect()
    };

    let modules = vec![
        ("facts", facts),
        ("state", state),
        ("resource", resources),
        ("program", programs),
        ("mode", map_const_obj(schema.get("modes").unwrap())),
        ("aa", map_const_obj(schema.get("aa").unwrap())),
    ];

    let notice = "// Generated by tools/render-post-layout.mjs; edit post-chain-layout.json instead.\n";
    let mut js = notice.to_string();
    let mut rs = notice.to_string();
    for (name, values) in &modules {
        js.push_str(&format!("export const {} = {};\n", name.to_uppercase(), JsonValue::Object(values.clone()).to_json_compact()));
        let lines: Vec<String> = values.iter().map(|(k, v)| format!("    pub const {k}: usize = {};", v.as_usize().unwrap())).collect();
        rs.push_str(&format!("pub mod {name} {{\n{}\n}}\n", lines.join("\n")));
    }
    js.push_str(&format!("export const POST_KEYS = {};\n", JsonValue::Array(keys.clone()).to_json_compact()));

    // Emit `TYPES` uniform-type slices and `pub mod uniform` slot constants for Rust.
    let types: Vec<String> = declarations
        .iter()
        .map(|ds| {
            let inner: Vec<String> = ds.iter().map(|(_, ty)| ty.as_usize().unwrap_or_else(|| ty.get("type").unwrap().as_usize().unwrap()).to_string()).collect();
            format!("&[{}]", inner.join(", "))
        })
        .collect();
    rs.push_str(&format!("pub const TYPES: [&[u32]; {}] = [{}];\n", keys.len(), types.join(", ")));
    rs.push_str("pub mod uniform {\n");
    for ((prog_name, _), decls) in programs_obj.iter().zip(declarations.iter()) {
        let lines: Vec<String> = decls.iter().enumerate().map(|(slot, (u_name, _))| format!("    pub const {}: usize = {slot};", constant(u_name))).collect();
        rs.push_str(&format!("pub mod {} {{\n{}\n}}\n", constant(prog_name).to_lowercase(), lines.join("\n")));
    }
    rs.push_str("}\n");

    write_or_check(root, "src/rust-render/post-layout.js", &js, check, "Stale src/rust-render/post-layout.js")?;
    write_or_check(root, "rust/bfe-render/src/post_chain_layout.rs", &rs, check, "Stale rust/bfe-render/src/post_chain_layout.rs")?;
    Ok(())
}

// Ports `generate(check)` from `tools/render-frame-layout.mjs`.
//
// Why this generator exists:
// Semantic view facts and native WebGL operation traces share one declared frame
// boundary (`src/rust-render/frame-layout.js` and `rust/bfe-render/src/frame_layout.rs`)
// generated from `rust/bfe-render/frame-layout.json`.
//
// Ordering and formatting rules:
// - `fields(rows)` packs `[name, count]` pairs into sequential offsets and appends `LEN`.
// - `enumerate(rows)` assigns `0..N` ordinals; `role` appends `COUNT: roles.length`.
// - All 13 module groups are emitted in fixed declaration order using compact JSON in JS.
fn frame_layout(root: &Path, check: bool) -> Result<(), String> {
    let schema = read_json(root, "rust/bfe-render/frame-layout.json")?;
    if schema.get("version").and_then(|v| v.as_usize()) != Some(1) {
        return Err("Unsupported frame layout".into());
    }
    // Ports `const fields = rows => ...` from `tools/render-frame-layout.mjs`.
    let fields = |rows: &JsonValue| -> Vec<(String, JsonValue)> {
        let mut at: usize = 0;
        let mut result = Vec::new();
        for item in rows.as_array().unwrap() {
            let pair = item.as_array().unwrap();
            let name = pair[0].as_str().unwrap();
            let count = pair[1].as_usize().unwrap();
            result.push((constant(name), JsonValue::Number(at as f64)));
            at += count;
        }
        result.push(("LEN".to_string(), JsonValue::Number(at as f64)));
        result
    };
    // Ports `const enumerate = rows => Object.fromEntries(rows.map((name, i) => [constant(name), i]))`.
    let enumerate = |rows: &JsonValue| -> Vec<(String, JsonValue)> {
        rows.as_array().unwrap().iter().enumerate().map(|(i, v)| (constant(v.as_str().unwrap()), JsonValue::Number(i as f64))).collect()
    };

    let ui_stages = schema.get("uiStages").unwrap();
    let ui_cap: Vec<(String, JsonValue)> = ui_stages.as_array().unwrap().iter().map(|item| {
        let pair = item.as_array().unwrap();
        (constant(pair[0].as_str().unwrap()), pair[1].clone())
    }).collect();
    let roles_arr = schema.get("roles").unwrap();
    let mut role = enumerate(roles_arr);
    role.push(("COUNT".to_string(), JsonValue::Number(roles_arr.as_array().unwrap().len() as f64)));
    let limit: Vec<(String, JsonValue)> = schema.get("limits").unwrap().as_object().unwrap().iter().map(|(k, v)| (constant(k), v.clone())).collect();

    let modules = vec![
        ("facts", fields(schema.get("facts").unwrap())),
        ("state", fields(schema.get("state").unwrap())),
        ("binding", fields(schema.get("binding").unwrap())),
        ("command", fields(schema.get("command").unwrap())),
        ("meta", fields(schema.get("meta").unwrap())),
        ("line", fields(schema.get("line").unwrap())),
        ("stats", fields(schema.get("stats").unwrap())),
        ("ui", fields(ui_stages)),
        ("ui_capacity", ui_cap),
        ("kind", enumerate(schema.get("kinds").unwrap())),
        ("operation", enumerate(schema.get("operations").unwrap())),
        ("role", role),
        ("limit", limit),
    ];

    let notice = "// Generated by tools/render-frame-layout.mjs; edit frame-layout.json instead.\n";
    let mut js = notice.to_string();
    let mut rs = notice.to_string();
    for (k, v) in modules {
        js.push_str(&format!("export const {} = {};\n", k.to_uppercase(), JsonValue::Object(v.clone()).to_json_compact()));
        let lines: Vec<String> = v.iter().map(|(n, x)| format!("    pub const {n}: usize = {};", x.as_usize().unwrap())).collect();
        rs.push_str(&format!("pub mod {k} {{\n{}\n}}\n", lines.join("\n")));
    }

    write_or_check(root, "src/rust-render/frame-layout.js", &js, check, "Stale src/rust-render/frame-layout.js")?;
    write_or_check(root, "rust/bfe-render/src/frame_layout.rs", &rs, check, "Stale rust/bfe-render/src/frame_layout.rs")?;
    Ok(())
}

// Ports `generate(check)` from `tools/render-camera-layout.mjs`.
//
// Why this generator exists:
// Camera, cascade, light, mirror, lens, sky, shader-frame, history-pose, placement,
// uniform, point-change, depth, and unit-view records share one declared boundary
// (`src/rust-render/camera-layout.js` and `rust/bfe-render/src/camera_layout.rs`)
// generated from `rust/bfe-render/camera-layout.json`.
//
// Ordering and formatting rules:
// - The 20 packed record groups (`input` .. `unitView`) are emitted first,
//   followed by `history_receipt`, `light_command`, `light_flag`, `role`, and `limit`.
// - Module names pass through `constant(k)` (`LIGHT_INPUT` in JS, `light_input` in Rust).
fn camera_layout(root: &Path, check: bool) -> Result<(), String> {
    let schema = read_json(root, "rust/bfe-render/camera-layout.json")?;
    if schema.get("version").and_then(|v| v.as_usize()) != Some(1) {
        return Err("Unsupported camera layout".into());
    }
    let fields = |rows: &JsonValue| -> Vec<(String, JsonValue)> {
        let mut at: usize = 0;
        let mut result = Vec::new();
        for item in rows.as_array().unwrap() {
            let pair = item.as_array().unwrap();
            let name = pair[0].as_str().unwrap();
            let count = pair[1].as_usize().unwrap();
            result.push((constant(name), JsonValue::Number(at as f64)));
            at += count;
        }
        result.push(("LEN".to_string(), JsonValue::Number(at as f64)));
        result
    };
    let enumerate = |rows: &JsonValue| -> Vec<(String, JsonValue)> {
        rows.as_array().unwrap().iter().enumerate().map(|(i, v)| (constant(v.as_str().unwrap()), JsonValue::Number(i as f64))).collect()
    };

    let mut modules = Vec::new();
    for name in [
        "input", "camera", "cascade", "light", "pack", "mirror", "lens", "source", "sky",
        "lightInput", "lightOutput", "shaderFrame", "historyPose", "placement",
        "uniformInput", "uniformOutput", "pointChange", "depthKey", "depthMarker", "unitView",
    ] {
        modules.push((name.to_string(), fields(schema.get(name).unwrap())));
    }
    let history_receipt: Vec<(String, JsonValue)> = schema.get("historyReceipts").unwrap().as_object().unwrap().iter().map(|(k, v)| (constant(k), v.clone())).collect();
    modules.push(("history_receipt".to_string(), history_receipt));
    modules.push(("light_command".to_string(), enumerate(schema.get("lightCommands").unwrap())));
    let light_flag: Vec<(String, JsonValue)> = schema.get("lightFlags").unwrap().as_object().unwrap().iter().map(|(k, v)| (constant(k), v.clone())).collect();
    modules.push(("light_flag".to_string(), light_flag));
    let roles_arr = schema.get("roles").unwrap();
    let mut role = enumerate(roles_arr);
    role.push(("COUNT".to_string(), JsonValue::Number(roles_arr.as_array().unwrap().len() as f64)));
    modules.push(("role".to_string(), role));
    let limit: Vec<(String, JsonValue)> = schema.get("limits").unwrap().as_object().unwrap().iter().map(|(k, v)| (constant(k), v.clone())).collect();
    modules.push(("limit".to_string(), limit));

    let notice = "// Generated by tools/render-camera-layout.mjs; edit camera-layout.json instead.\n";
    let mut js = notice.to_string();
    let mut rs = notice.to_string();
    for (k, v) in modules {
        js.push_str(&format!("export const {} = {};\n", constant(&k), JsonValue::Object(v.clone()).to_json_compact()));
        let lines: Vec<String> = v.iter().map(|(n, x)| format!("    pub const {n}: usize = {};", x.as_usize().unwrap())).collect();
        rs.push_str(&format!("pub mod {} {{\n{}\n}}\n", constant(&k).to_lowercase(), lines.join("\n")));
    }

    write_or_check(root, "src/rust-render/camera-layout.js", &js, check, "Stale src/rust-render/camera-layout.js")?;
    write_or_check(root, "rust/bfe-render/src/camera_layout.rs", &rs, check, "Stale rust/bfe-render/src/camera_layout.rs")?;
    Ok(())
}

// Ports `generate(check)` from `tools/render-shaders.mjs`.
//
// Why this generator exists:
// Runs all nine renderer layout generators above, then scans every shader family
// directory under `rust/bfe-render/shaders/<family>/manifest.json` to emit:
// - `rust/bfe-render/src/program_warmup.rs`
// - `rust/bfe-render/src/actor-selectors.json`
// - `rust/bfe-render/src/world-selectors.json`
// - `src/render-programs.js`
// - `rust/bfe-render/src/program_catalog.rs`
//
// Ordering and formatting rules:
// - Shader family directories are sorted lexicographically (`readdirSync(base).sort()`).
// - A shader's `vertex` or `fragment` path may be a single string or an array of chunk
//   paths concatenated in order (`parts` / `read_parts`).
// - A shadowed twin (`p.shadowOf`) reuses its unshadowed base's vertex and fragment
//   sources with `#define USE_SHADOWMAP` and `#define SHADOWMAP_TYPE_PCF_SOFT` spliced
//   in at `twin.vertexLine` and `twin.fragmentLine` (`with_shadow`), prefixed with
//   `vertexPrefix` / `fragmentPrefix`.
// - The per-program SHA-256 `hash` digests compact `JSON.stringify` of
//   `{ vertex, fragment, attributes, uniforms, kind, source, lamps }` in that exact
//   7-key order.
// - Each `sources[p.key]` object records `attributes`, `uniforms`, `hash`, `kind`,
//   `source`, `lamps`, optional `derived`, optional `shadowOf`, optional `material`,
//   and optional `selector` + `inputs`. After all programs in all families are added,
//   the `twins` pass appends `shadowTwin` to each base program's object so `shadowTwin`
//   appears last in V8 property order.
// - `program_warmup.rs` emits `WarmSpec` entries for `actors/`, `effects/`, and
//   `utility/` in manifest order, followed by the post-chain programs from
//   `rust/bfe-render/post-chain-layout.json` (`family:4`).
// - `selector_json` builds `{ key, ...p.selector }` objects for `actors/` and `world/`:
//   if `p.selector` contains a `"key"` property, it overwrites the initial `"key"` slot
//   in place just as V8 object spread `{ key, ...p.selector }` does.
pub fn render_shaders(root: &Path, check: bool) -> Result<(), String> {
    actor_layout(root, check)?;
    interface_resource_layout(root, check)?;
    line_layout(root, check)?;
    interface_layout(root, check)?;
    static_layout(root, check)?;
    effect_layout(root, check)?;
    post_layout(root, check)?;
    frame_layout(root, check)?;
    camera_layout(root, check)?;

    let base = root.join("rust/bfe-render/shaders");
    let mut families: Vec<String> = fs::read_dir(&base)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().and_then(|e| e.file_name().into_string().ok()))
        .collect();
    families.sort();

    let mut sources: Vec<(String, JsonValue)> = Vec::new();
    let mut rows: Vec<String> = Vec::new();
    let mut twins: Vec<(String, String)> = Vec::new();

    for family in families {
        let fam_dir = base.join(&family);
        let manifest_text = fs::read_to_string(fam_dir.join("manifest.json")).map_err(|e| e.to_string())?;
        let manifest = JsonValue::parse(&manifest_text)?;
        if manifest.get("version").and_then(|v| v.as_usize()) != Some(1) {
            return Err("Unsupported shader manifest".into());
        }
        let programs = manifest.get("programs").and_then(|v| v.as_array()).ok_or("missing programs")?;
        let mut entries: HashMap<String, &JsonValue> = HashMap::new();
        for q in programs {
            if let Some(k) = q.get("key").and_then(|v| v.as_str()) {
                entries.insert(k.to_string(), q);
            }
        }

        // A source path may be an ordered array of chunk files (the actor family's
        // shared chunks) concatenated in order.
        let parts = |path_val: &JsonValue| -> Vec<String> {
            if let Some(arr) = path_val.as_array() {
                arr.iter().map(|v| v.as_str().unwrap().to_string()).collect()
            } else {
                vec![path_val.as_str().unwrap().to_string()]
            }
        };

        let read_parts = |path_val: &JsonValue| -> Result<String, String> {
            let mut joined = String::new();
            for file in parts(path_val) {
                let s = fs::read_to_string(fam_dir.join(&file)).map_err(|e| e.to_string())?;
                joined.push_str(&s);
            }
            Ok(joined)
        };

        for p in programs {
            let key = p.get("key").and_then(|v| v.as_str()).ok_or("missing program key")?;
            if !key.starts_with(&format!("{family}/")) || sources.iter().any(|(k, _)| k == key) {
                return Err(format!("Invalid shader key {key}"));
            }
            // A shadowed twin stores no source of its own: it is its unshadowed base with
            // the two shadow defines inserted at the recorded line, as Three.js emits them.
            let twin = p.get("shadowOf");
            let own = if let Some(t) = twin {
                let t_key = t.get("key").and_then(|v| v.as_str()).ok_or("missing shadowOf.key")?;
                entries.get(t_key).copied().ok_or_else(|| format!("Invalid shadow base for {key}"))?
            } else {
                p
            };
            if own.get("vertex").is_none() || own.get("shadowOf").is_some() {
                return Err(format!("Invalid shadow base for {key}"));
            }
            let with_shadow = |text: String, line: Option<usize>| -> String {
                if let Some(idx) = line {
                    let mut lines: Vec<&str> = text.split('\n').collect();
                    lines.splice(idx..idx, ["#define USE_SHADOWMAP", "#define SHADOWMAP_TYPE_PCF_SOFT"]);
                    lines.join("\n")
                } else {
                    text
                }
            };
            let v_line = twin.and_then(|t| t.get("vertexLine")).and_then(|v| v.as_usize());
            let f_line = twin.and_then(|t| t.get("fragmentLine")).and_then(|v| v.as_usize());
            let v_prefix = p.get("vertexPrefix").and_then(|v| v.as_str()).unwrap_or("");
            let f_prefix = p.get("fragmentPrefix").and_then(|v| v.as_str()).unwrap_or("");
            let vertex = format!("{v_prefix}{}", with_shadow(read_parts(own.get("vertex").unwrap())?, v_line));
            let fragment = format!("{f_prefix}{}", with_shadow(read_parts(own.get("fragment").unwrap())?, f_line));

            let kind = p.get("kind").and_then(|v| v.as_str()).unwrap_or("Plain");
            let source = p.get("source").and_then(|v| v.as_str()).unwrap_or("body");
            let lamps = p.get("lamps").and_then(|v| v.as_bool()).unwrap_or(false);

            let attrs_obj = p.get("attributes").and_then(|v| v.as_object()).ok_or("missing attributes")?;
            let attributes_json: Vec<JsonValue> = attrs_obj.iter().map(|(n, v)| JsonValue::Array(vec![JsonValue::String(n.clone()), v.clone()])).collect();

            // Normalize each uniform entry (`name: type` or `name: { type, size }`)
            // into `{ name, type, size }` objects and `(name, ty, size)` tuples.
            let uniforms_obj = p.get("uniforms").and_then(|v| v.as_object()).ok_or("missing uniforms")?;
            let mut uniforms_list: Vec<(String, usize, usize)> = Vec::new();
            let mut uniforms_json: Vec<JsonValue> = Vec::new();
            for (name, value) in uniforms_obj {
                let (ty, size) = if let Some(n) = value.as_usize() {
                    (n, 1usize)
                } else {
                    (
                        value.get("type").and_then(|v| v.as_usize()).ok_or("invalid uniform type")?,
                        value.get("size").and_then(|v| v.as_usize()).ok_or("invalid uniform size")?,
                    )
                };
                uniforms_list.push((name.clone(), ty, size));
                uniforms_json.push(JsonValue::Object(vec![
                    ("name".to_string(), JsonValue::String(name.clone())),
                    ("type".to_string(), JsonValue::Number(ty as f64)),
                    ("size".to_string(), JsonValue::Number(size as f64)),
                ]));
            }

            // Digest the 7-field program payload in V8 insertion order.
            let hash_payload = JsonValue::Object(vec![
                ("vertex".to_string(), JsonValue::String(vertex)),
                ("fragment".to_string(), JsonValue::String(fragment)),
                ("attributes".to_string(), JsonValue::Array(attributes_json.clone())),
                ("uniforms".to_string(), JsonValue::Array(uniforms_json.clone())),
                ("kind".to_string(), JsonValue::String(kind.to_string())),
                ("source".to_string(), JsonValue::String(source.to_string())),
                ("lamps".to_string(), JsonValue::Bool(lamps)),
            ]);
            let mut hasher = Sha256::new();
            hasher.update(hash_payload.to_json_compact().as_bytes());
            let hash = format!("{:x}", hasher.finalize());

            // The page picks a shadowed program through its base's shadowTwin, never by name.
            if let Some(t) = twin {
                let t_key = t.get("key").and_then(|v| v.as_str()).unwrap();
                twins.push((t_key.to_string(), key.to_string()));
            }

            let mut entry_fields = vec![
                ("attributes".to_string(), JsonValue::Array(attributes_json)),
                ("uniforms".to_string(), JsonValue::Array(uniforms_json)),
                ("hash".to_string(), JsonValue::String(hash)),
                ("kind".to_string(), JsonValue::String(kind.to_string())),
                ("source".to_string(), JsonValue::String(source.to_string())),
                ("lamps".to_string(), JsonValue::Bool(lamps)),
            ];
            if p.get("derived").and_then(|v| v.as_bool()) == Some(true) {
                entry_fields.push(("derived".to_string(), JsonValue::Bool(true)));
            }
            if let Some(t) = twin {
                let t_key = t.get("key").and_then(|v| v.as_str()).unwrap();
                entry_fields.push(("shadowOf".to_string(), JsonValue::String(t_key.to_string())));
            }
            if let Some(mat) = p.get("material") {
                entry_fields.push(("material".to_string(), mat.clone()));
            }
            if let Some(sel) = p.get("selector") {
                entry_fields.push(("selector".to_string(), sel.clone()));
                if let Some(inp) = p.get("inputs") {
                    entry_fields.push(("inputs".to_string(), inp.clone()));
                }
            }
            sources.push((key.to_string(), JsonValue::Object(entry_fields)));

            // Build the corresponding `Spec` entry for `rust/bfe-render/src/program_catalog.rs`.
            let attr_pairs: Vec<String> = attrs_obj.iter().map(|(n, v)| format!("({}, {})", quote_str(n), v.as_usize().unwrap())).collect();
            let uniform_pairs: Vec<String> = uniforms_list.iter().map(|(n, ty, _)| format!("({}, {ty})", quote_str(n))).collect();
            let size_pairs: Vec<String> = uniforms_list.iter().filter(|(_, _, sz)| *sz > 1).map(|(n, _, sz)| format!("({}, {sz})", quote_str(n))).collect();
            let files_rs = |path_val: &JsonValue| -> String {
                let inc: Vec<String> = parts(path_val).into_iter().map(|file| format!("include_str!(\"../shaders/{family}/{file}\")")).collect();
                format!("&[{}]", inc.join(", "))
            };
            let shadow_at = if let (Some(vl), Some(fl)) = (v_line, f_line) {
                format!("Some(({vl}, {fl}))")
            } else {
                "None".to_string()
            };
            rows.push(format!(
                "    Spec {{ key: {}, vertex: {}, fragment: {}, vertex_prefix: {}, fragment_prefix: {}, kind: Kind::{kind}, expanded: {}, lamps: {lamps}, shadow_at: {shadow_at}, attributes: &[{}], uniforms: &[{}], uniform_sizes: &[{}] }},",
                quote_str(key),
                files_rs(own.get("vertex").unwrap()),
                files_rs(own.get("fragment").unwrap()),
                quote_str(v_prefix),
                quote_str(f_prefix),
                source == "expanded",
                attr_pairs.join(", "),
                uniform_pairs.join(", "),
                size_pairs.join(", ")
            ));
        }
    }

    // Attach `shadowTwin` to each base program after all family manifests have been loaded.
    for (from, key) in twins {
        let entry = sources.iter_mut().find(|(k, _)| k == &from).ok_or_else(|| format!("Invalid shadow twin for {key}"))?;
        if let JsonValue::Object(ref mut fields) = entry.1 {
            if fields.iter().any(|(k, _)| k == "shadowTwin") {
                return Err(format!("Invalid shadow twin for {key}"));
            }
            fields.push(("shadowTwin".to_string(), JsonValue::String(key)));
        }
    }

    // Loading policy comes from the same finite catalog as shader admission.
    // Preserve manifest order so prewarming cannot change registration identity.
    let mut warm: Vec<String> = Vec::new();
    for (key, p) in &sources {
        let actor = key.starts_with("actors/");
        let effect = key.starts_with("effects/");
        let utility = key.starts_with("utility/");
        if !(actor || effect || utility) {
            continue;
        }
        let q = p.get("selector");
        let selected = actor && q.is_some();
        let family_num = if actor { 1 } else if effect { 2 } else { 3 };
        let cascades: i64 = if selected {
            q.and_then(|s| s.get("cascades")).and_then(|v| v.as_i64()).unwrap_or(-1)
        } else {
            -1
        };
        let lamps_num: i64 = if selected {
            i64::from(q.and_then(|s| s.get("lamps")).and_then(|v| v.as_bool()).unwrap_or(false))
        } else {
            -1
        };
        let dynamic: bool = if selected {
            q.and_then(|s| s.get("defines"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter().any(|pair| {
                        pair.as_array()
                            .and_then(|items| items.first())
                            .and_then(|n| n.as_str())
                            == Some("DYNAMIC")
                    })
                })
                .unwrap_or(false)
        } else {
            false
        };
        let derived: bool = if selected {
            p.get("derived").and_then(|v| v.as_bool()).unwrap_or(false)
        } else {
            false
        };
        let has_shadow_of = p.get("shadowOf").is_some();
        let has_shadow_twin = p.get("shadowTwin").is_some();
        let shadow: i64 = if effect && (has_shadow_of || has_shadow_twin) {
            i64::from(has_shadow_of)
        } else {
            -1
        };
        let csm: i64 = if effect {
            if let Some(csm_val) = q.and_then(|s| s.get("csmChunks")) {
                match csm_val {
                    JsonValue::Bool(b) => i64::from(*b),
                    JsonValue::Number(n) => *n as i64,
                    _ => -1,
                }
            } else {
                -1
            }
        } else {
            -1
        };
        warm.push(format!(
            "    WarmSpec {{ key:{}, family:{family_num}, cascades:{cascades}, lamps:{lamps_num}, dynamic:{dynamic}, derived:{derived}, shadow:{shadow}, csm:{csm} }},",
            quote_str(key)
        ));
    }
    let post_chain = read_json(root, "rust/bfe-render/post-chain-layout.json")?;
    for (_, key_val) in post_chain.get("programs").and_then(|v| v.as_object()).ok_or("missing post-chain programs")? {
        let key = key_val.as_str().ok_or("post-chain program key must be string")?;
        warm.push(format!(
            "    WarmSpec {{key:{},family:4,cascades:-1,lamps:-1,dynamic:false,derived:false,shadow:-1,csm:-1}},",
            quote_str(key)
        ));
    }

    // Build compact selector tables (`actor-selectors.json` and `world-selectors.json`).
    let selector_json = |prefix: &str| -> String {
        let mut items = Vec::new();
        for (key, p) in &sources {
            if key.starts_with(prefix) {
                if let Some(sel) = p.get("selector").and_then(|s| s.as_object()) {
                    let mut obj = vec![("key".to_string(), JsonValue::String(key.clone()))];
                    for (sk, sv) in sel {
                        if let Some(existing) = obj.iter_mut().find(|(k, _)| k == sk) {
                            existing.1 = sv.clone();
                        } else {
                            obj.push((sk.clone(), sv.clone()));
                        }
                    }
                    items.push(JsonValue::Object(obj));
                }
            }
        }
        format!("{}\n", JsonValue::Array(items).to_json_compact())
    };

    let program_warmup = format!(
        "// Generated by tools/render-shaders.mjs; edit shader manifests instead.\nuse super::WarmSpec;\npub static PROGRAMS: &[WarmSpec] = &[\n{}\n];\n",
        warm.join("\n")
    );
    let actor_selectors = selector_json("actors/");
    let world_selectors = selector_json("world/");
    let render_programs = format!(
        "// Generated by tools/render-shaders.mjs; edit rust/bfe-render/shaders instead.\nexport const programLayouts = {};\n",
        JsonValue::Object(sources).to_json_pretty()
    );
    let program_catalog = format!(
        "// Generated by tools/render-shaders.mjs; edit the shader manifests instead.\nuse super::Spec;\nuse crate::shader::Kind;\npub static PROGRAMS: &[Spec] = &[\n{}\n];\n",
        rows.join("\n")
    );

    for (path, text) in [
        ("rust/bfe-render/src/program_warmup.rs", &program_warmup),
        ("rust/bfe-render/src/actor-selectors.json", &actor_selectors),
        ("rust/bfe-render/src/world-selectors.json", &world_selectors),
        ("src/render-programs.js", &render_programs),
        ("rust/bfe-render/src/program_catalog.rs", &program_catalog),
    ] {
        write_or_check(root, path, text, check, &format!("Stale {path}"))?;
    }
    Ok(())
}
