//! The simulation store's code generator. `sim`'s build script calls
//! [`store_source`] to write the `gen` module into OUT_DIR; xtask uses the
//! same model for the files it still checks in (the generator's test schema,
//! the HUD layout and the client's input layout).
pub mod json;
pub mod schema_eval;
pub mod sim_schema;

use std::path::Path;

/// The `gen` module for the schemas in `schema_dir` as one source file: the
/// index's items with every schema file's module inlined (`mod x { .. }`).
/// The index's own inner attributes and doc comments are left to the
/// including file, which `include!` cannot take.
pub fn store_source(schema_dir: &Path) -> Result<String, String> {
    let schemas = schema_eval::load_schemas(schema_dir, true)?;
    let non_layout: Vec<_> = schemas.into_iter().filter(|s| !s.layout_only).collect();
    let model = sim_schema::build_model(&non_layout, sim_schema::TARGETS[0].clone())?;
    let index = sim_schema::rust_index(&model);
    let mut out = String::with_capacity(4 << 20);
    let mut header = true;
    for line in index.lines() {
        if header && (line.starts_with("//") || line.starts_with("#![")) {
            continue;
        }
        header = false;
        let module = line.strip_prefix("mod ").and_then(|m| m.strip_suffix(';'));
        match module.and_then(|m| model.files.iter().find(|f| sim_schema::snake(f) == m)) {
            Some(file) => {
                out.push_str(line.trim_end_matches(';'));
                out.push_str(" {\n");
                out.push_str(&sim_schema::rust_file(&model, file));
                out.push_str("}\n");
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    Ok(out)
}
