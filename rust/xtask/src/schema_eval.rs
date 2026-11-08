// Declarative JavaScript and JSON schema loader and exporter (`schema_eval.rs`).
//
// Why this module exists:
// The 40 simulation and boundary schema files in `rust/sim/schema/*.mjs` (plus the
// test schema in `rust/sim/schema-test/shelf.mjs`) are written as declarative ES
// modules (`export default { ... }`). Converting those `.mjs` files to `.json` on
// disk would change the SHA-256 wire compatibility fingerprint computed by
// `host_contract.rs` (`HOST_SCHEMA` hashes every file under `rust/sim/schema`),
// which is a release-boundary decision reserved for the deletion sweep.
// Until the deletion sweep converts `rust/sim/schema/*.mjs` to `.json` via
// `cargo xtask schema --export-json DIR`, `xtask` evaluates `.mjs` files directly
// in Rust without invoking Node.js, and seamlessly reads `<stem>.json` once exported.
//
// Evaluated JavaScript subset:
// 1. Lexical tokens:
//    - ASCII whitespace, line comments (`// ...`), and block comments (`/* ... */`).
//    - Identifiers (`[A-Za-z_$][A-Za-z0-9_$]*`), single- or double-quoted string
//      literals with standard escapes (`\n`, `\r`, `\t`, `\\`, `\'`, `\"`), and
//      decimal or hexadecimal (`0x...`) numeric literals with optional `-` sign
//      and `_` digit separators.
//    - Punctuation: `{`, `}`, `[`, `]`, `(`, `)`, `:`, `,`, `;`, `=`, `=>`, `...`, `?`.
// 2. Top-level module statements:
//    - `const <ident> = <expr>;` binds a top-level constant in the module environment
//      (used for shared field blocks such as `humanFields`, `itemState`, and the
//      `fixed` field helper in `client.mjs`).
//    - `export const layoutOnly = <bool>;` marks layout-only schemas (`hud.mjs`,
//      `input.mjs`, `perf.mjs`, `telemetry.mjs`, `timeline.mjs`, `worker_config.mjs`,
//      etc.) that are skipped by `sim-schema` unless `include_layouts` is set.
//    - `export default <expr>;` defines the module's exported schema object.
// 3. Expressions:
//    - Object literals `{ key: expr, shorthandIdent, ...spreadExpr }` with identifier,
//      string, or non-negative integer keys.
//    - Array literals `[ expr, ... ]` with optional trailing commas.
//    - Primitive literals: strings, numbers, `true`, `false`, `null`, `undefined`.
//    - Identifier references to previously bound top-level `const` declarations.
//    - The `fixed` arrow function helper declaration in `client.mjs`:
//      `const fixed = (type, js) => ({ type, mirror: false, ...(js ? { js } : {}) });`
//      and call expressions `fixed(type)` or `fixed(type, js)`.
//
// Rejected JavaScript constructs:
// - Any top-level statement other than `const`, `export const`, or `export default`
//   (`let`, `var`, `function`, `class`, `import`, `export { ... }`, `if`, `for`,
//   `while`, `switch`, `try`, `return`, or expression statements).
// - Template literals (`` `...` ``), regular expressions, bigints, unary `+`/`!`
//   operators, binary arithmetic/logical/comparison operators, member access (`a.b`
//   or `a[b]`), computed object property keys (`{ [k]: v }`), array spreads,
//   arbitrary arrow functions or closures, method definitions, `new`, `async`/`await`,
//   or calls to any function other than the bound `fixed` helper.
// - Top-level modules without an `export default` object or containing unreferenced
//   `undefined` / function values inside the exported JSON tree.
//
// Byte-identical ordering rule:
// Object literal evaluation preserves exact source declaration order. When an object
// spread (`...spreadExpr`) or duplicate key overwrites an existing key, the key stays
// at its original insertion slot and only its value is updated in place, matching V8's
// `Object` property insertion order for string keys. Properties whose evaluated value
// is `undefined` are omitted, matching `JSON.stringify` / `Object.entries` over the
// conditional spread `...(js ? { js } : {})`.

use crate::json::JsonValue;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

// Loaded representation of a single `.mjs` or `.json` schema file, corresponding to
// one entry returned by `loadSchemas(dir)` in `tools/sim-schema.mjs`.
pub struct LoadedSchema {
    // Filename stem without the `.mjs` or `.json` extension (e.g. `"client"`).
    pub file: String,
    // Evaluated `export default` object preserving V8 property insertion order.
    pub schema: JsonValue,
    // Value of `export const layoutOnly = true` (or `"$layoutOnly": true` in JSON).
    pub layout_only: bool,
}

// Returns true when `name` is a simulation store JSON schema file (`*.json`
// excluding presentation/transport `*-layout.json` files).
fn is_sim_json_schema(name: &str) -> bool {
    name.ends_with(".json") && !name.ends_with("-layout.json")
}

// Parses a `.json` simulation schema file exported by `export_schemas_json`,
// extracting and stripping the top-level `"$layoutOnly"` boolean marker when present
// so the returned `(JsonValue, bool)` matches `eval_schema_module` byte-for-byte.
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

// Serializes an evaluated `(schema, layout_only)` pair into a canonical 2-space
// pretty-printed JSON document with a trailing newline. When `layout_only` is true,
// prepends `"$layoutOnly": true` as the first top-level object property.
pub fn format_schema_json(
    schema: &JsonValue,
    layout_only: bool,
    filename: &str,
) -> Result<String, String> {
    let JsonValue::Object(entries) = schema else {
        return Err(format!("{filename}: schema root must be a JSON object"));
    };
    let mut out_entries = Vec::with_capacity(entries.len() + usize::from(layout_only));
    if layout_only {
        out_entries.push(("$layoutOnly".to_string(), JsonValue::Bool(true)));
    }
    for (k, v) in entries {
        if k != "$layoutOnly" {
            out_entries.push((k.clone(), v.clone()));
        }
    }
    let doc = JsonValue::Object(out_entries);
    Ok(format!("{}\n", doc.to_json_pretty()))
}

// Resolves `dir_arg` passed to `cargo xtask schema --export-json <DIR>` against
// either the repository root (when `root/dir_arg` exists or `dir_arg` begins with
// `rust/`) or the current working directory.
fn resolve_export_dir(root: &Path, dir_arg: &str) -> PathBuf {
    let p = Path::new(dir_arg);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    let from_root = root.join(p);
    if from_root.exists() || dir_arg.starts_with("rust/") || dir_arg.starts_with("rust\\") {
        return from_root;
    }
    p.to_path_buf()
}

// Exports declarative `.mjs` schema modules to `<DIR>/<stem>.json` (`cargo xtask schema --export-json <DIR>`).
//
// Source selection rule:
// - If `<DIR>` already contains `.mjs` files (for example `rust/sim/schema` or
//   `rust/sim/schema-test`), evaluates every `<stem>.mjs` in `<DIR>` and writes
//   `<DIR>/<stem>.json` alongside it.
// - Otherwise, evaluates every `<stem>.mjs` in `rust/sim/schema` and writes
//   `<DIR>/<stem>.json` into `<DIR>` (creating `<DIR>` if needed).
pub fn export_schemas_json(root: &Path, dir_arg: &str) -> Result<(), String> {
    let target_dir = resolve_export_dir(root, dir_arg);
    fs::create_dir_all(&target_dir).map_err(|e| format!("{}: {e}", target_dir.display()))?;

    let has_mjs = fs::read_dir(&target_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .any(|name| name.ends_with(".mjs"));

    let src_dir = if has_mjs {
        target_dir.clone()
    } else {
        root.join("rust/sim/schema")
    };

    let mut mjs_files: Vec<String> = fs::read_dir(&src_dir)
        .map_err(|e| format!("{}: {e}", src_dir.display()))?
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            if name.ends_with(".mjs") {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    mjs_files.sort();

    if mjs_files.is_empty() {
        return Err(format!(
            "no .mjs schema files found in {}",
            src_dir.display()
        ));
    }

    for file in &mjs_files {
        let src_path = src_dir.join(file);
        let text = fs::read_to_string(&src_path).map_err(|e| format!("{file}: {e}"))?;
        let (schema, layout_only) = eval_schema_module(&text, file)?;
        let json_text = format_schema_json(&schema, layout_only, file)?;
        let stem = file.strip_suffix(".mjs").unwrap();
        let dst_path = target_dir.join(format!("{stem}.json"));
        fs::write(&dst_path, json_text).map_err(|e| format!("{}: {e}", dst_path.display()))?;
    }

    println!(
        "exported {} schema files to {}",
        mjs_files.len(),
        target_dir.display()
    );
    Ok(())
}

// Ports `loadSchemas(dir)` from `tools/sim-schema.mjs`, supporting both `.mjs`
// schema modules and `.json` schema files (excluding `*-layout.json`).
//
// Ordering rule: schema stems are sorted by `<stem>.mjs` in lexicographical byte
// order (`Array.prototype.sort()` on UTF-16 code units in Node.js for ASCII
// filenames, so `client-timing.mjs` sorts before `client.mjs`). When both
// `<stem>.json` and `<stem>.mjs` exist, `<stem>.json` takes precedence. When
// `include_layouts` is `false`, schemas with `layoutOnly = true` are skipped.
pub fn load_schemas(dir: &Path, include_layouts: bool) -> Result<Vec<LoadedSchema>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut stems_set = BTreeSet::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let Some(name) = entry.ok().and_then(|e| e.file_name().into_string().ok()) else {
            continue;
        };
        if let Some(stem) = name.strip_suffix(".mjs") {
            stems_set.insert(stem.to_string());
        } else if is_sim_json_schema(&name) {
            if let Some(stem) = name.strip_suffix(".json") {
                stems_set.insert(stem.to_string());
            }
        }
    }

    // Sort by `<stem>.mjs` so `'-'` (0x2d) vs `'.'` (0x2e) ordering matches
    // `fs.readdirSync(dir).sort()` over `.mjs` filenames (`client-timing` before `client`).
    let mut stems: Vec<String> = stems_set.into_iter().collect();
    stems.sort_by(|a, b| format!("{a}.mjs").cmp(&format!("{b}.mjs")));

    let mut out = Vec::new();
    for stem in stems {
        let json_name = format!("{stem}.json");
        let json_path = dir.join(&json_name);
        let (schema, layout_only) = if json_path.exists() {
            let text = fs::read_to_string(&json_path).map_err(|e| format!("{json_name}: {e}"))?;
            load_json_schema(&text, &json_name)?
        } else {
            let mjs_name = format!("{stem}.mjs");
            let mjs_path = dir.join(&mjs_name);
            let text = fs::read_to_string(&mjs_path).map_err(|e| format!("{mjs_name}: {e}"))?;
            eval_schema_module(&text, &mjs_name)?
        };
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

// Internal runtime value produced while evaluating module bindings and expressions.
// `FixedFn` represents the `fixed(type, js)` helper function in `client.mjs`; all
// other bindings evaluate directly to `JsonValue` or `Undefined`.
#[derive(Clone, Debug)]
enum Val {
    Undefined,
    Json(JsonValue),
    FixedFn,
}

impl Val {
    // Converts an evaluated expression value into a concrete `JsonValue`, rejecting
    // leaked `undefined` or function values that cannot appear in a schema tree.
    fn into_json(self, where_msg: &str) -> Result<JsonValue, String> {
        match self {
            Val::Json(j) => Ok(j),
            Val::Undefined => Err(format!("{where_msg}: unexpected undefined in schema value")),
            Val::FixedFn => Err(format!("{where_msg}: unexpected function in schema value")),
        }
    }
}

// Evaluates a single `.mjs` schema source string and returns `(default_export, layout_only)`.
// Used by `load_schemas` for full directory loads and by layout generators (`layouts.rs`)
// that evaluate individual layout-only `.mjs` files (`audio.mjs`, `environment.mjs`, etc.).
pub fn eval_schema_module(source: &str, filename: &str) -> Result<(JsonValue, bool), String> {
    let tokens = tokenize(source, filename)?;
    let mut parser = ModuleParser {
        tokens: &tokens,
        pos: 0,
        filename,
        env: HashMap::new(),
    };
    parser.parse_module()
}

// Lexical token for the declarative JavaScript subset.
#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Num(f64),
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LParen,
    RParen,
    Colon,
    Comma,
    Semi,
    Eq,
    Arrow,
    DotDotDot,
    Question,
}

// Scans `.mjs` source text into a flat token stream, stripping whitespace and
// `//` or `/* ... */` comments and rejecting any unsupported punctuation or syntax.
fn tokenize(src: &str, filename: &str) -> Result<Vec<Tok>, String> {
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // Strip single-line `//` comments through the end of the line.
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Strip multi-line `/* ... */` block comments.
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        match b {
            b'{' => {
                out.push(Tok::LBrace);
                i += 1;
            }
            b'}' => {
                out.push(Tok::RBrace);
                i += 1;
            }
            b'[' => {
                out.push(Tok::LBracket);
                i += 1;
            }
            b']' => {
                out.push(Tok::RBracket);
                i += 1;
            }
            b'(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            b')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            b':' => {
                out.push(Tok::Colon);
                i += 1;
            }
            b',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            b';' => {
                out.push(Tok::Semi);
                i += 1;
            }
            b'?' => {
                out.push(Tok::Question);
                i += 1;
            }
            // Distinguish `=>` (arrow function token) from `=` (binding assignment).
            b'=' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                    out.push(Tok::Arrow);
                    i += 2;
                } else {
                    out.push(Tok::Eq);
                    i += 1;
                }
            }
            // Only the three-dot spread operator `...` is permitted; member access `.` is rejected.
            b'.' => {
                if i + 2 < bytes.len() && bytes[i + 1] == b'.' && bytes[i + 2] == b'.' {
                    out.push(Tok::DotDotDot);
                    i += 3;
                } else {
                    return Err(format!("{filename}: unexpected '.' at byte {i}"));
                }
            }
            // Single- or double-quoted string literals with standard backslash escapes.
            b'\'' | b'"' => {
                let quote = b;
                i += 1;
                let mut s = String::new();
                while i < bytes.len() && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                        if i >= bytes.len() {
                            return Err(format!("{filename}: unterminated string"));
                        }
                        match bytes[i] {
                            b'n' => s.push('\n'),
                            b'r' => s.push('\r'),
                            b't' => s.push('\t'),
                            b'\\' => s.push('\\'),
                            b'\'' => s.push('\''),
                            b'"' => s.push('"'),
                            other => s.push(other as char),
                        }
                        i += 1;
                    } else {
                        let ch = src[i..].chars().next().unwrap();
                        s.push(ch);
                        i += ch.len_utf8();
                    }
                }
                if i >= bytes.len() {
                    return Err(format!("{filename}: unterminated string"));
                }
                i += 1;
                out.push(Tok::Str(s));
            }
            // Numeric literals: optional leading `-`, decimal or `0x` hex, optional `_` separators.
            b'-' | b'0'..=b'9' => {
                let start = i;
                if b == b'-' {
                    i += 1;
                }
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.' || bytes[i] == b'_')
                {
                    i += 1;
                }
                let raw = src[start..i].replace('_', "");
                let num: f64 =
                    if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
                        u64::from_str_radix(hex, 16).map_err(|e| format!("{filename}: {e}"))? as f64
                    } else {
                        raw.parse()
                            .map_err(|e| format!("{filename}: invalid number '{raw}': {e}"))?
                    };
                out.push(Tok::Num(num));
            }
            // ASCII identifiers and keywords (`const`, `export`, `default`, `true`, `false`, `null`).
            _ if b.is_ascii_alphabetic() || b == b'_' || b == b'$' => {
                let start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
                {
                    i += 1;
                }
                out.push(Tok::Ident(src[start..i].to_string()));
            }
            _ => {
                return Err(format!(
                    "{filename}: unexpected character '{}' at byte {i}",
                    b as char
                ))
            }
        }
    }
    Ok(out)
}

// Recursive-descent parser and evaluator over the tokenized `.mjs` module.
struct ModuleParser<'a> {
    tokens: &'a [Tok],
    pos: usize,
    filename: &'a str,
    env: HashMap<String, Val>,
}

impl<'a> ModuleParser<'a> {
    // Returns the current lookahead token without advancing `pos`.
    fn peek(&self) -> Option<&'a Tok> {
        self.tokens.get(self.pos)
    }

    // Consumes and returns the current token, or errors on unexpected EOF.
    fn next(&mut self) -> Result<&'a Tok, String> {
        let t = self
            .tokens
            .get(self.pos)
            .ok_or_else(|| format!("{}: unexpected EOF", self.filename))?;
        self.pos += 1;
        Ok(t)
    }

    // Consumes the next token and verifies that it matches `expected`.
    fn expect(&mut self, expected: &Tok) -> Result<(), String> {
        let t = self.next()?;
        if t != expected {
            return Err(format!(
                "{}: expected {:?}, got {:?}",
                self.filename, expected, t
            ));
        }
        Ok(())
    }

    // Parses top-level `const`, `export const`, and `export default` declarations
    // until EOF, returning the `export default` JSON object and `layoutOnly` flag.
    fn parse_module(&mut self) -> Result<(JsonValue, bool), String> {
        let mut default_export: Option<JsonValue> = None;
        let mut layout_only = false;

        while let Some(tok) = self.peek() {
            match tok {
                Tok::Semi => {
                    self.pos += 1;
                }
                // `const <name> = <expr>;` binds a reusable schema fragment or `fixed` helper.
                Tok::Ident(kw) if kw == "const" => {
                    self.pos += 1;
                    let name = match self.next()? {
                        Tok::Ident(s) => s.clone(),
                        other => {
                            return Err(format!(
                                "{}: expected identifier after const, got {:?}",
                                self.filename, other
                            ))
                        }
                    };
                    self.expect(&Tok::Eq)?;
                    let val = self.parse_expr()?;
                    self.env.insert(name, val);
                    if self.peek() == Some(&Tok::Semi) {
                        self.pos += 1;
                    }
                }
                // `export default <expr>;` or `export const <name> = <expr>;`.
                Tok::Ident(kw) if kw == "export" => {
                    self.pos += 1;
                    match self.next()? {
                        Tok::Ident(s) if s == "default" => {
                            let val = self.parse_expr()?.into_json(self.filename)?;
                            default_export = Some(val);
                            if self.peek() == Some(&Tok::Semi) {
                                self.pos += 1;
                            }
                        }
                        Tok::Ident(s) if s == "const" => {
                            let name = match self.next()? {
                                Tok::Ident(n) => n.clone(),
                                other => {
                                    return Err(format!(
                                        "{}: expected ident, got {:?}",
                                        self.filename, other
                                    ))
                                }
                            };
                            self.expect(&Tok::Eq)?;
                            let val = self.parse_expr()?;
                            if name == "layoutOnly" {
                                if let Val::Json(JsonValue::Bool(b)) = val {
                                    layout_only = b;
                                }
                            }
                            self.env.insert(name, val);
                            if self.peek() == Some(&Tok::Semi) {
                                self.pos += 1;
                            }
                        }
                        other => {
                            return Err(format!("{}: unexpected export {:?}", self.filename, other))
                        }
                    }
                }
                other => {
                    return Err(format!(
                        "{}: unexpected top-level token {:?}",
                        self.filename, other
                    ))
                }
            }
        }

        let schema =
            default_export.ok_or_else(|| format!("{}: missing export default", self.filename))?;
        Ok((schema, layout_only))
    }

    // Evaluates a single value expression: the `fixed` arrow function definition,
    // object or array literal, primitive literal, identifier lookup, or `fixed(...)` call.
    fn parse_expr(&mut self) -> Result<Val, String> {
        // Recognize the `fixed` arrow function helper in `rust/sim/schema/client.mjs`:
        // `(type, js) => ({ type, mirror: false, ...(js ? { js } : {}) })`.
        if self.peek() == Some(&Tok::LParen) {
            let save = self.pos;
            let mut depth = 0;
            let mut cursor = self.pos;
            while cursor < self.tokens.len() {
                match self.tokens[cursor] {
                    Tok::LParen => depth += 1,
                    Tok::RParen => {
                        depth -= 1;
                        if depth == 0 {
                            cursor += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
            if cursor < self.tokens.len() && self.tokens[cursor] == Tok::Arrow {
                // Advance past the arrow function body up to the terminating `;` at depth 0.
                self.pos = cursor + 1;
                let mut d = 0;
                while self.pos < self.tokens.len() {
                    match self.tokens[self.pos] {
                        Tok::LParen | Tok::LBrace | Tok::LBracket => d += 1,
                        Tok::RParen | Tok::RBrace | Tok::RBracket => d -= 1,
                        Tok::Semi if d == 0 => break,
                        _ => {}
                    }
                    self.pos += 1;
                }
                return Ok(Val::FixedFn);
            }
            self.pos = save;
        }

        match self.next()? {
            Tok::LBrace => self.parse_object().map(Val::Json),
            Tok::LBracket => self.parse_array().map(Val::Json),
            Tok::Str(s) => Ok(Val::Json(JsonValue::String(s.clone()))),
            Tok::Num(n) => Ok(Val::Json(JsonValue::Number(*n))),
            Tok::Ident(id) => match id.as_str() {
                "true" => Ok(Val::Json(JsonValue::Bool(true))),
                "false" => Ok(Val::Json(JsonValue::Bool(false))),
                "null" => Ok(Val::Json(JsonValue::Null)),
                "undefined" => Ok(Val::Undefined),
                name => {
                    // Evaluate call expressions `fixed(type)` or `fixed(type, js)`:
                    // produces `{ type, mirror: false }` or `{ type, mirror: false, js }`
                    // with keys in that exact V8 declaration order.
                    if self.peek() == Some(&Tok::LParen) {
                        self.pos += 1;
                        let mut args = Vec::new();
                        while self.peek() != Some(&Tok::RParen) {
                            args.push(self.parse_expr()?);
                            if self.peek() == Some(&Tok::Comma) {
                                self.pos += 1;
                            }
                        }
                        self.expect(&Tok::RParen)?;
                        match self.env.get(name) {
                            Some(Val::FixedFn) => {
                                let ty = args
                                    .first()
                                    .cloned()
                                    .ok_or("fixed() missing type arg")?
                                    .into_json(self.filename)?;
                                let mut fields = vec![
                                    ("type".to_string(), ty),
                                    ("mirror".to_string(), JsonValue::Bool(false)),
                                ];
                                if let Some(Val::Json(js_val)) = args.get(1) {
                                    fields.push(("js".to_string(), js_val.clone()));
                                }
                                Ok(Val::Json(JsonValue::Object(fields)))
                            }
                            _ => Err(format!("{}: unknown function {name}", self.filename)),
                        }
                    } else {
                        self.env
                            .get(name)
                            .cloned()
                            .ok_or_else(|| format!("{}: unknown identifier {name}", self.filename))
                    }
                }
            },
            other => Err(format!(
                "{}: unexpected token in expression: {:?}",
                self.filename, other
            )),
        }
    }

    // Evaluates an object literal `{ ... }` into an ordered `JsonValue::Object`,
    // expanding `...spread` expressions and shorthand properties while updating
    // duplicate keys in place to preserve V8 property insertion order.
    fn parse_object(&mut self) -> Result<JsonValue, String> {
        let mut entries: Vec<(String, JsonValue)> = Vec::new();
        while self.peek() != Some(&Tok::RBrace) {
            if self.peek() == Some(&Tok::DotDotDot) {
                // Object spread (`...ident` or `...{ ... }`): merge properties in order.
                self.pos += 1;
                let spread_val = self.parse_expr()?.into_json(self.filename)?;
                if let JsonValue::Object(spread_entries) = spread_val {
                    for (k, v) in spread_entries {
                        if let Some(existing) = entries.iter_mut().find(|(ek, _)| ek == &k) {
                            existing.1 = v;
                        } else {
                            entries.push((k, v));
                        }
                    }
                }
            } else {
                // Standard `key: value` or shorthand `key` property.
                let key = match self.next()? {
                    Tok::Ident(s) => s.clone(),
                    Tok::Str(s) => s.clone(),
                    Tok::Num(n) => (*n as i64).to_string(),
                    other => {
                        return Err(format!(
                            "{}: expected object key, got {:?}",
                            self.filename, other
                        ))
                    }
                };
                let val = if self.peek() == Some(&Tok::Colon) {
                    self.pos += 1;
                    self.parse_expr()?
                } else {
                    self.env
                        .get(&key)
                        .cloned()
                        .ok_or_else(|| format!("{}: unknown shorthand {key}", self.filename))?
                };
                if let Val::Json(j) = val {
                    if let Some(existing) = entries.iter_mut().find(|(ek, _)| ek == &key) {
                        existing.1 = j;
                    } else {
                        entries.push((key, j));
                    }
                }
            }
            if self.peek() == Some(&Tok::Comma) {
                self.pos += 1;
            }
        }
        self.expect(&Tok::RBrace)?;
        Ok(JsonValue::Object(entries))
    }

    // Evaluates an array literal `[ ... ]` into a `JsonValue::Array`.
    fn parse_array(&mut self) -> Result<JsonValue, String> {
        let mut items = Vec::new();
        while self.peek() != Some(&Tok::RBracket) {
            let val = self.parse_expr()?.into_json(self.filename)?;
            items.push(val);
            if self.peek() == Some(&Tok::Comma) {
                self.pos += 1;
            }
        }
        self.expect(&Tok::RBracket)?;
        Ok(JsonValue::Array(items))
    }
}
