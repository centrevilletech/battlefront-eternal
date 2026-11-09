// Declarative JSON schema loader (`schema_eval.rs`).
//
// Loads the simulation store and HUD schema definitions from `rust/sim/schema/*.json`
// and `rust/sim/schema-test/*.json` (excluding `*-layout.json` files owned by
// `layouts.rs`), preserving JSON object property declaration order.

use crate::json::JsonValue;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

// Loaded representation of a single `.json` schema file.
pub struct LoadedSchema {
    // Filename stem without the `.json` extension (e.g. `"client"`).
    pub file: String,
    // Parsed schema object preserving property insertion order.
    pub schema: JsonValue,
    // Value of the top-level `"$layoutOnly": true` marker when present.
    pub layout_only: bool,
}

// Returns true when `name` is a simulation store JSON schema file (`*.json`
// excluding presentation/transport `*-layout.json` files).
fn is_sim_json_schema(name: &str) -> bool {
    name.ends_with(".json") && !name.ends_with("-layout.json")
}

// Parses a `.json` simulation schema file, extracting and stripping the top-level
// `"$layoutOnly"` boolean marker when present.
pub fn load_json_schema(source: &str, filename: &str) -> Result<(JsonValue, bool), String> {
    let parsed = JsonValue::parse(source).map_err(|e| format!("{filename}: {e}"))?;
    let JsonValue::Object(entries) = parsed else {
        return Err(format!("{filename}: schema root must be a JSON object"));
    };
    let mut layout_only = false;
    let mut filtered = Vec::with_capacity(entries.len());
    for (k, v) in entries {
        if k == "$layoutOnly" {
            layout_only = v
                .as_bool()
                .ok_or_else(|| format!("{filename}: $layoutOnly must be a boolean"))?;
        } else {
            filtered.push((k, v));
        }
    }
    Ok((JsonValue::Object(filtered), layout_only))
}

// Loads all simulation `.json` schema files under `dir` (excluding `*-layout.json`).
//
// Ordering rule: schema stems are sorted by `<stem>.json` in lexicographical byte
// order (`'-'` 0x2d before `'.'` 0x2e, so `client-timing` sorts before `client`).
// When `include_layouts` is `false`, schemas with `"$layoutOnly": true` are skipped.
pub fn load_schemas(dir: &Path, include_layouts: bool) -> Result<Vec<LoadedSchema>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut stems_set = BTreeSet::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let Some(name) = entry.ok().and_then(|e| e.file_name().into_string().ok()) else {
            continue;
        };
        if is_sim_json_schema(&name) {
            if let Some(stem) = name.strip_suffix(".json") {
                stems_set.insert(stem.to_string());
            }
        }
    }

    let mut stems: Vec<String> = stems_set.into_iter().collect();
    stems.sort_by(|a, b| format!("{a}.json").cmp(&format!("{b}.json")));

    let mut out = Vec::new();
    for stem in stems {
        let json_name = format!("{stem}.json");
        let json_path = dir.join(&json_name);
        let text = fs::read_to_string(&json_path).map_err(|e| format!("{json_name}: {e}"))?;
        let (schema, layout_only) = load_json_schema(&text, &json_name)?;
        if layout_only && !include_layouts {
            continue;
        }
        out.push(LoadedSchema {
            file: stem,
            schema,
            layout_only,
        });
    }
    Ok(out)
}
