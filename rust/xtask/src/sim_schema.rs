#![allow(dead_code)]
// Simulation store schema compiler (part 1): model definitions, type parser,
// multi-pass model builder, row/record layout calculator, and Rust codegen.
//
// Direct port of the first half of the schema generator (see `docs/sim-store.md`
// for the store architecture). Every pass, data structure, and formatting rule
// mirrors the schema generator so that generated files under:
//   - `rust/sim/src/gen/` and `rust/sim/src/gen_test/`
//   - `rust/bfe-client/src/generated_input.rs`
//   - `rust/bfe-render/src/hud_model/layout.rs` and `src/hud-layout.js`
//   - `src/sim/gen/` and `tests/sim-gen/`
// are byte-for-byte identical to the Node generator output.
//
// Ordering and formatting invariants required for byte-identical output:
//   1. Schema files in a directory are loaded in `Array.prototype.sort()`
//      (UTF-16 code-unit) order over `<stem>.mjs` filenames (`listed()`).
//   2. Declarations inside each schema file are visited in V8 object property
//      order (`JsonValue::Object` backed by `Vec<(String, JsonValue)>`), and
//      cross-file maps (`OrdList`) preserve insertion order just like JS `Map`.
//   3. Kind codes (1..=N), command codes (1..=N), event codes (1..=N), and
//      import codes (1..=N) are assigned after sorting their names in UTF-16
//      code-unit order (`Array.prototype.sort()`), matching ASCII byte order
//      for all identifier strings.
//   4. Layout hashes (`LAYOUT_HASH`) use 32-bit FNV-1a over UTF-16 code units
//      (`fnv()`) of the canonical layout summary lines, formatted as 8-digit
//      lowercase hex (`0x{:08x}`).
//   5. Every generated Rust and JS file ends with a single trailing `\n`.

use crate::json::{quote_str, JsonValue};
use crate::schema_eval::LoadedSchema;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
// Describes one schema compilation target (`TARGETS`),
// pairing a schema source directory with its Rust and JS output directories
// and crate/runtime import paths.
pub struct Target {
    pub name: &'static str,
    pub schema: &'static str,
    pub rust: &'static str,
    pub test: bool,
}

// The two simulation store targets compiled by `sim-schema`:
//   1. Production simulation schemas in `rust/sim/schema/` -> `rust/sim/src/gen/`.
//   2. Synthetic test schemas in `rust/sim/schema-test/` -> `rust/sim/src/gen_test/`.
pub const TARGETS: &[Target] = &[
    Target { name: "gen", schema: "rust/sim/schema", rust: "rust/sim/src/gen", test: false },
    Target { name: "gen_test", schema: "rust/sim/schema-test", rust: "rust/sim/src/gen_test", test: true },
];

// Primitive scalar types recognized by `parseType()`.
// Fixed-size `f64xN` arrays are handled alongside scalars via `is_f64x_any`.
const SCALARS: &[&str] = &["f64", "f32", "i32", "u32", "u8", "bool", "vec3", "quat", "mat4", "str"];
// Rust reserved keywords that must be escaped with `r#` when used as field or
// module identifiers (`RUST_KEYWORDS`).
const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "gen",
];

// Mirrors `pascal(s)`:
// `s.replace(/[^A-Za-z0-9]+(.)?/g, (_, c) => (c ? c.toUpperCase() : '')).replace(/^./, (c) => c.toUpperCase())`
// Converts an identifier `s` (`snake_case`, `kebab-case`, or `camelCase`) into
// `PascalCase` for Rust struct/enum/variant names (`pascal(s)`).
// Splits on any non-ASCII-alphanumeric separator and capitalizes the first
// character of each segment while preserving interior uppercase letters.
pub fn pascal(s: &str) -> String {
    let mut out = String::new();
    let mut cap_next = true;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            if cap_next { out.push(c.to_ascii_uppercase()); cap_next = false; } else { out.push(c); }
        } else {
            cap_next = true;
        }
    }
    out
}

// Mirrors `snake(s)`:
// `s.replace(/([a-z0-9])([A-Z])/g, '$1_$2').replace(/[^A-Za-z0-9]+/g, '_').toLowerCase()`
// Converts a camelCase or PascalCase identifier `s` into a valid Rust
// `snake_case` identifier (`snake(s)`), inserting `_`
// at `[a-z0-9][A-Z]` transitions and escaping Rust reserved keywords (`type`,
// `move`, `loop`, `match`, `ref`, `self`, etc.) with `r#`.
pub fn snake(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut step1 = String::new();
    for i in 0..chars.len() {
        let c = chars[i];
        step1.push(c);
        if i + 1 < chars.len()
            && (c.is_ascii_lowercase() || c.is_ascii_digit())
            && chars[i + 1].is_ascii_uppercase()
        { step1.push('_'); }
    }
    let mut out = String::new();
    let mut in_non_alnum = false;
    for c in step1.chars() {
        if c.is_ascii_alphanumeric() {
            if in_non_alnum { out.push('_'); in_non_alnum = false; }
            out.push(c.to_ascii_lowercase());
        } else {
            in_non_alnum = true;
        }
    }
    if in_non_alnum { out.push('_'); }
    if RUST_KEYWORDS.contains(&out.as_str()) { format!("r#{out}") } else { out }
}

// Converts `s` to `snake_case` without escaping Rust keywords. Used when
// composing larger identifiers such as `<prefix>_<field>` method names where
// keyword escaping does not apply (`snakeBare(s)`).
pub fn snake_bare(s: &str) -> String {
    let sn = snake(s);
    sn.strip_prefix("r#").unwrap_or(&sn).to_string()
}

// Converts `s` to `UPPER_SNAKE_CASE` for Rust and JS constant identifiers
// (`upper(s)`).
pub fn upper(s: &str) -> String { snake_bare(s).to_uppercase() }

// Returns the `KIND_<UPPER>` constant name for a store kind
// (`kindConst(kind)`).
pub fn kind_const(kind: &str) -> String { format!("KIND_{}", upper(kind)) }

// Tests whether `name` matches `/^[A-Za-z_$][A-Za-z0-9_$]*$/` so JS codegen can
// emit `.name` or unquoted object keys instead of `["name"]` (`JS_IDENT` in
// the schema generator).
fn is_js_ident(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') { return false; }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

// Tests whether `name` matches `/^\d+$/` for numeric property keys in JS
// (`jsKey` / `jsOptKey`).
fn is_digits(name: &str) -> bool { !name.is_empty() && name.chars().all(|c| c.is_ascii_digit()) }

// Formats a property access suffix on a non-null JS receiver (`.foo` for valid
// identifiers, `[0]` for digits, or `["..."]` otherwise). Mirrors `jsKey(name)`
//.
pub fn js_key(name: &str) -> String {
    if is_js_ident(name) { format!(".{name}") } else if is_digits(name) { format!("[{name}]") } else { format!("[{}]", quote_str(name)) }
}

// Formats an optional-chaining property access suffix (`?.foo` or `?.[key]`).
// Mirrors `jsOptKey(name)`.
pub fn js_opt_key(name: &str) -> String {
    if is_js_ident(name) { format!("?.{name}") } else { let k = js_key(name); format!("?.{}", k.strip_prefix('.').unwrap_or(&k)) }
}

// Formats a chain of optional JS property accesses (`a?.b?.c`) for reading
// nested source paths on live JS objects (`jsPath(path)`).
pub fn js_path(path: &[String]) -> String { path.iter().map(|s| js_key(s)).collect::<Vec<_>>().join("") }

// FNV-1a over UTF-16 code units: mirrors `fnv(text)`.
// Computes the 32-bit FNV-1a hash over the UTF-16 code units of `text`
// (`fnv(text)`, starting from offset basis `0x811c9dc5`
// and multiplying by prime `0x01000193` with 32-bit wrapping arithmetic).
// Used both for standalone string hashing and as the building block of
// `Model::hash` (`LAYOUT_HASH`).
pub fn fnv(text: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for unit in text.encode_utf16() { h ^= unit as u32; h = h.wrapping_mul(0x01000193); }
    h
}

// Describes how an optional (`T | null`) type is represented in flat `f64` words:
// either via a sentinel value in the first word (`sentinel: "nan"` or `"-1"`
// with `tag: 0`) or via a dedicated 1-word presence tag (`tag: 1`, `sentinel: None`).
// Mirrors the `nullable` descriptor attached by `parseType()`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Nullable { pub null: bool, pub undefined: bool, }

// Describes the key type of a `map<K, V>` field: `kind` is `"str"`, `"i32"`,
// `"enum"`, or `"union"`, and `name` holds the enum or key-union name when
// applicable. Mirrors the `key` object in `parseType()`.
#[derive(Clone, Debug, PartialEq)]
pub struct MapKey {
    pub base: String, // "num" or "ref"
    pub of: Option<String>,
}

// Parsed representation of a schema type expression (`parseType()` in
// the schema generator). Covers scalars, fixed `f64xN` arrays, handles
// (`&kind`), named types (`enum`, `record`, `union`), homogeneous containers
// (`vec<T>`, `set<T>`, `table<K>`), keyed maps (`map<K, V>`), and nullable
// wrappers (`T | null`).
#[derive(Clone, Debug, PartialEq)]
pub struct TypeSpec {
    // Original unparsed type expression string from the schema file.
    pub text: String,
    // Nullability / `undefined` representation metadata.
    // Base type kind (`"f64"`, `"vec3"`, `"ref"`, `"rec"`, `"union"`, `"list"`, `"map"`, etc.).
    // Target type/kind/enum/table name (`Some(...)` for `ref`, `rec`, `union`, `enum`, `def`, `set`, `index`).
    // Element or value `TypeSpec` for `list:<T>`, `map:<K, V>`, and `obj:<V>`.
    // Optional map key descriptor (`num` or `ref:<KindOrUnion>`) for non-string maps.
    // Element count `N` for `f64xN` tuples and `<Typed>Array(N)` views.
    pub nullable: Nullable,
    pub base: String,
    pub of: Option<String>,
    pub value: Option<Box<TypeSpec>>,
    pub key: Option<MapKey>,
    pub n: usize,
    pub class: Option<String>,
    pub elem: Option<String>,
}

// Returns true if `t` is nullable (`T | null`) or a nullable handle (`&kind | null`).
// Mirrors `optional(t)`.
pub fn optional(t: &TypeSpec) -> bool { t.nullable.null || t.nullable.undefined }

// Returns true if `t` is a collection type (`vec`, `set`, `map`, or `table`)
// whose elements live on the side-buffer heap (`container(t)`).
pub fn container(t: &TypeSpec) -> bool { matches!(t.base.as_str(), "map" | "list" | "obj") }

// Strips the outer nullable modifier from `t` while keeping its underlying kind,
// element, and key descriptors intact (`bare(t)`).
pub fn bare(t: &TypeSpec) -> TypeSpec { let mut c = t.clone(); c.nullable = Nullable { null: false, undefined: false, }; c }

// Splits a type expression on top-level `|` characters while ignoring `|`
// nested inside `(...)` or `<...>`. Mirrors `bars(text)`.
fn bars(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, b) in text.bytes().enumerate() {
        if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth -= 1;
        } else if b == b'|' && depth == 0 {
            parts.push(&text[start..i]);
            start = i + 1;
        }
    }
    parts.push(&text[start..]);
    parts
}

// Returns true when `text` is enclosed in a single matching pair of outer
// parentheses `(...)` so `parse_type` can unwrap grouped expressions.
// Mirrors `grouped(text)`.
fn grouped(text: &str) -> bool {
    if !text.starts_with('(') || !text.ends_with(')') { return false; }
    let mut depth = 0i32;
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'(' { depth += 1; } else if b == b')' { depth -= 1; if depth == 0 && i < bytes.len() - 1 { return false; } }
    }
    true
}

// Checks whether `s` matches `/^\w+$/` (ASCII word characters), used when
// validating scalar, handle, and named type identifiers in `parse_type`.
fn is_word_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else { return false };
    if !(first.is_ascii_alphabetic() || first == '_') { return false; }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// Checks whether `s` has the form `f64x<digits>` before validating that the
// length is strictly positive.
fn is_f64x_any(s: &str) -> bool { if let Some(rest) = s.strip_prefix("f64x") { is_digits(rest) } else { false } }

// Parses a schema type string into a `TypeSpec`, validating container nesting
// rules, nullable sentinel selection (`NaN` for `f64`/`f32`/`vec3`/`quat`/`mat4`/`f64xN`,
// `-1` for `&kind` and `enum`, or a 1-word tag for `i32`/`u32`/`u8`/`bool`/`str`/
// `record`/`union`), and rejecting nullable containers or unknown syntax.
// Direct port of `parseType(text, where)`.
pub fn parse_type(text: &str, where_loc: &str) -> Result<TypeSpec, String> {
    let split = bars(text);
    let base_str = split[0].trim();
    let mut nullable = Nullable::default();
    for r in split[1..].iter().map(|s| s.trim()) {
        match r {
            "null" => { if nullable.null { return Err(format!("{where_loc}: null twice")); } nullable.null = true; }
            "undefined" => { if nullable.undefined { return Err(format!("{where_loc}: undefined twice")); } nullable.undefined = true; }
            _ => return Err(format!("{where_loc}: \"{r}\" in a type can only be null or undefined")),
        }
    }

    let inner = |s: &str, what: &str| -> Result<Option<TypeSpec>, String> {
        if !grouped(s) { return Ok(None); }
        let t = parse_type(&s[1..s.len() - 1], where_loc)?;
        if !optional(&t) { return Err(format!("{where_loc}: {what} in parentheses can be null or undefined (else write it bare)")); }
        if matches!(t.base.as_str(), "list" | "set" | "map" | "obj" | "typed") {
            return Err(format!("{where_loc}: {what} that can be null or undefined is no list, set, map, object or typed array"));
        }
        Ok(Some(t))
    };

    let mut spec = TypeSpec {
        text: text.to_string(),
        nullable,
        base: String::new(),
        of: None,
        value: None,
        key: None,
        n: 0,
        class: None,
        elem: None,
    };

    // `list:<T>`: homogeneous array stored on the side-buffer heap as a 2-word
    // `[offset, count]` header in the parent row. Bare identifier items default
    // to entity handles (`ref:<kind>`); nested containers and nullable items
    // without parens are rejected just like `parseType`.
    if let Some(rest) = base_str.strip_prefix("list:") {
        spec.base = "list".to_string();
        if let Some(item) = inner(rest, "a list's item")? { spec.value = Some(Box::new(item)); return Ok(spec); }
        let val = if is_word_ident(rest) && !SCALARS.contains(&rest) && !is_f64x_any(rest) {
            parse_type(&format!("ref:{rest}"), where_loc)?
        } else {
            parse_type(rest, where_loc)?
        };
        if optional(&val) || matches!(val.base.as_str(), "map" | "list" | "set") {
            return Err(format!("{where_loc}: a list's items are a type that is never null or undefined, and no list, set or map"));
        }
        if matches!(val.base.as_str(), "map" | "obj" | "typed") {
            return Err(format!("{where_loc}: a list's items are no object or typed array"));
        }
        if val.base == "ref" { spec.of = val.of.clone(); }
        spec.value = Some(Box::new(val));
        if spec.nullable.null && spec.nullable.undefined {
            return Err(format!("{where_loc}: a list or set can be null or undefined, not both"));
        }
        return Ok(spec);
    }

    // `map:<V>`, `map:num:<V>`, or `map:ref:<kind>:<V>`: key-value collection
    // stored on the side-buffer heap as `[offset, count]`, where each entry
    // packs the key words (`str` -> 2, `num` -> 1, `ref:<kind>` -> 1 or key-union
    // words) followed by `words(V)` value words.
    if let Some(after_map) = base_str.strip_prefix("map:") {
        spec.base = "map".to_string();
        if let Some(val_str) = after_map.strip_prefix("num:") {
            spec.key = Some(MapKey { base: "num".into(), of: None });
            let v = match inner(val_str, "a map's value")? { Some(it) => it, None => parse_type(val_str, where_loc)?, };
            if matches!(v.base.as_str(), "map" | "obj" | "typed") {
                return Err(format!("{where_loc}: a map's values are no map, object or typed array"));
            }
            spec.value = Some(Box::new(v));
            return Ok(spec);
        } else if let Some(after_ref) = after_map.strip_prefix("ref:") {
            if let Some((k_name, val_str)) = after_ref.split_once(':') {
                if is_word_ident(k_name) && !val_str.is_empty() {
                    spec.key = Some(MapKey { base: "ref".into(), of: Some(k_name.to_string()) });
                    let v = match inner(val_str, "a map's value")? { Some(it) => it, None => parse_type(val_str, where_loc)?, };
                    if matches!(v.base.as_str(), "map" | "obj" | "typed") {
                        return Err(format!("{where_loc}: a map's values are no map, object or typed array"));
                    }
                    spec.value = Some(Box::new(v));
                    return Ok(spec);
                }
            }
        }
        let val_opt = inner(after_map, "a map's value")?;
        let has_parens = val_opt.is_some();
        let v = match val_opt { Some(it) => it, None => parse_type(after_map, where_loc)?, };
        if !has_parens && (optional(&v) || v.base == "map") {
            return Err(format!("{where_loc}: a map's values are a type that is never null or undefined, and no map"));
        }
        if matches!(v.base.as_str(), "map" | "obj" | "typed") {
            return Err(format!("{where_loc}: a map's values are no map, object or typed array"));
        }
        spec.value = Some(Box::new(v));
        return Ok(spec);
    }

    // `obj:<V>`: plain JS object dictionary (`{ [key: string]: V }`) serialized
    // identically to a string-keyed map on the wire (`IndexMap<String, V>` in Rust),
    // but reconstructed as a prototype-free `{}` object on the JS mirror side.
    if let Some(after_obj) = base_str.strip_prefix("obj:") {
        spec.base = "obj".to_string();
        let v = match inner(after_obj, "an object's value")? { Some(it) => it, None => parse_type(after_obj, where_loc)?, };
        if matches!(v.base.as_str(), "map" | "obj" | "typed") {
            return Err(format!("{where_loc}: an object's values are no map, object or typed array"));
        }
        spec.value = Some(Box::new(v));
        return Ok(spec);
    }

    // Primitive scalar types (`f64`, `f32`, `i32`, `u32`, `u8`, `bool`, `vec3`,
    // `quat`, `mat4`, `str`).
    if SCALARS.contains(&base_str) { spec.base = base_str.to_string(); return Ok(spec); }

    // Fixed-size inline `f64` tuple `f64xN` (`2 <= N <= 16`), represented as
    // `[f64; N]` in Rust and `Float64Array(N)` in JS.
    if let Some(n_str) = base_str.strip_prefix("f64x") {
        if let Ok(n) = n_str.parse::<usize>() { if (2..=16).contains(&n) { spec.base = "f64xN".to_string(); spec.n = n; return Ok(spec); } }
    }

    // Fixed-length typed array `<Prefix>Array(N)` (`1 <= N <= 99999`), stored
    // on the side-buffer heap in JS and decoded into `Vec<elem>` in Rust.
    for &(prefix, elem) in &[
        ("Float64", "f64"),
        ("Float32", "f32"),
        ("Int32", "i32"),
        ("Uint32", "u32"),
        ("Int16", "i16"),
        ("Uint16", "u16"),
        ("Int8", "i8"),
        ("Uint8", "u8"),
    ] {
        let pat = format!("{prefix}Array(");
        if let Some(rest) = base_str.strip_prefix(&pat) {
            if let Some(n_str) = rest.strip_suffix(')') {
                if let Ok(n) = n_str.parse::<usize>() {
                    if n >= 1 && n_str.len() <= 5 {
                        spec.base = "typed".to_string();
                        spec.class = Some(format!("{prefix}Array"));
                        spec.elem = Some(elem.to_string());
                        spec.n = n;
                        return Ok(spec);
                    }
                }
            }
        }
    }

    // Named reference prefixes (`enum:<E>`, `ref:<K>`, `union:<U>`, `def:<D>`,
    // `rec:<R>`, `set:<K>`, `index:<K>`).
    if let Some((prefix, target)) = base_str.split_once(':') {
        if matches!(prefix, "enum" | "ref" | "union" | "def" | "rec" | "set" | "index") && is_word_ident(target) {
            spec.base = prefix.to_string();
            spec.of = Some(target.to_string());
            if spec.base == "set" && spec.nullable.null && spec.nullable.undefined {
                return Err(format!("{where_loc}: a list or set can be null or undefined, not both"));
            }
            return Ok(spec);
        }
    }

    Err(format!("{where_loc}: unknown type \"{base_str}\""))
}

// A parsed field on a record or kind (`parseField(name, spec, where)` in
// the schema generator), recording both its JS property name and its Rust
// `snake_case` identifier, parsed `TypeSpec`, storage tier (`"export"`,
// `"import"`, or `"cold"`), and optional JS source path (`from`).
#[derive(Clone, Debug)]
pub struct Field { pub name: String, pub js: Vec<String>, pub ty: TypeSpec, pub mirror: bool, pub import: bool, pub where_loc: String, }

// Parses a field specification from either a shorthand type string (`"f64"`)
// or an object (`{ type, tier, from, doc }`). Mirrors `parseField()` in
// the schema generator.
fn parse_field(name: &str, spec: &JsonValue, where_loc: &str) -> Result<Field, String> {
    let (ty_str, js_opt, mirror, import) = match spec {
        JsonValue::String(s) => (s.as_str(), None, true, true),
        JsonValue::Object(entries) => {
            for (k, _) in entries {
                if !matches!(k.as_str(), "type" | "js" | "mirror" | "import" | "doc") {
                    return Err(format!("{where_loc}: unknown field option \"{k}\""));
                }
            }
            let ty_s = spec.get("type").and_then(|v| v.as_str()).ok_or_else(|| format!("{where_loc}: a type must be a string"))?;
            let js_s = spec.get("js").and_then(|v| v.as_str());
            let mir = spec.get("mirror").and_then(|v| v.as_bool()).unwrap_or(true);
            let imp = spec.get("import").and_then(|v| v.as_bool()).unwrap_or(true);
            (ty_s, js_s, mir, imp)
        }
        _ => return Err(format!("{where_loc}: a field is a type or {{ type, js, mirror, import }}")),
    };
    let ty = parse_type(ty_str, where_loc)?;
    let js: Vec<String> = js_opt.unwrap_or(name).split('.').map(|s| s.to_string()).collect();
    if mirror && js.len() > 1 {
        return Err(format!("{where_loc}: a field read from {} cannot be mirrored (set mirror: false)", js_opt.unwrap_or(name)));
    }
    Ok(Field { name: name.to_string(), js, ty, mirror, import, where_loc: where_loc.to_string(), })
}

// A host-to-Wasm command declaration (`commands` section of a schema file).
// Each command has a payload record and an optional synchronous return type
// (`returns`). Assigned a 1-based `code` in sorted name order by `build_model`.
#[derive(Clone, Debug)]
// Command wire layout:
//   - Commands are dispatched from JS to Rust via `Store::dispatch_command`.
//   - `args` names the input `RecordDef` (synthesized as `<Pascal>Args` when
//     declared inline), and `result` optionally names the synchronous return
//     `RecordDef` (`None` for fire-and-forget commands).
//   - `rust` holds the target module::function path (`<file>::<snake_name>` by
//     default, or an explicit `rust: "module::fn"` override).
pub struct CommandDef {
    pub name: String,
    pub file: String,
    pub args: String,
    pub result: Option<String>,
    pub rust: String,
    pub owned: Option<String>,
    pub detached: bool,
    pub where_loc: String,
    pub code: usize,
}

// A Wasm-to-host event declaration (`events` section of a schema file).
// Assigned a 1-based `code` in sorted name order by `build_model`.
#[derive(Clone, Debug)]
// Event wire layout:
//   - Events are queued by Rust simulation code via `Store::emit_<name>` and
//     drained by the JS host at the end of each tick.
//   - `record` names the payload `RecordDef` serialized onto the event stream.
pub struct EventDef { pub name: String, pub file: String, pub record: String, pub where_loc: String, pub code: usize, }

// A Wasm-to-host synchronous import function declaration (`imports` section).
// Assigned a 1-based `code` in sorted name order by `build_model`.
#[derive(Clone, Debug)]
// Import wire layout:
//   - Imports allow Rust simulation code to synchronously query the JS host
//     during a tick via the `Imports` trait (`IMP_<NAME>` code).
//   - `args` and `result` name the request and response `RecordDef`s packed
//     into the shared call/return wire buffers.
pub struct ImportDef {
    pub name: String,
    pub file: String,
    pub args: String,
    pub result: Option<String>,
    pub borrow: bool,
    pub where_loc: String,
    pub code: usize,
}

// A schema constant declaration (`constants` section) emitted into both Rust
// and JS generated modules.
#[derive(Clone, Debug)]
pub struct ConstantDef { pub name: String, pub value: u32, pub file: String, }

// A closed string enum declaration (`enums` section). Variants are stored in
// schema declaration order and represented on the wire by their 0-based index
// (`0..values.len() - 1`, with `-1` as the nullable sentinel).
#[derive(Clone, Debug)]
pub struct EnumDef { pub name: String, pub values: Vec<String>, pub file: String, pub variants: Vec<String>, }

// One branch of a tagged union (`UnionDef`), holding its wire tag index (`0..N`),
// schema key string, Rust PascalCase variant name, and parsed payload `TypeSpec`
// (`None` for unit variants).
#[derive(Clone, Debug)]
pub struct UnionVariant {
    pub tag: String, // "null", "undefined", "lit", "ref", "value"
    pub rust: String,
    pub value: Option<String>,
    pub kind: Option<String>,
    pub ty: Option<TypeSpec>,
}

// A tagged union declaration (`unions` section) or a synthesized map-key union
// (`key_union: true`). Discriminated on the JS side either by an internal tag
// field on every variant record (`tag: Some(...)`), by JS class (`by_class: true`),
// or by primitive `typeof` / enum membership.
#[derive(Clone, Debug)]
pub struct UnionDef { pub name: String, pub variants: Vec<UnionVariant>, pub file: String, pub words: usize, }

// A value record declaration (`records` section). Records can be embedded inside
// kinds, other records, unions, or containers, or serve as the argument/result
// payload of commands, events, and imports.
#[derive(Clone, Debug)]
// Record layout and publication metadata:
//   - `fields`: ordered list of record fields (`parse_field`), placed contiguously
//     from word offset 0 without the 2-word entity header used by kinds.
//   - `observe`: field names tracked for fine-grained change detection (`$observe`).
//   - `delta` / `refresh`: field subsets synced incrementally (`$delta`, `$refresh`).
//   - `parts`: sub-function split points (`$parts`) so huge records do not exceed
//     V8's turbofan inlining budget.
//   - `layout`: when true (`$layout: true`), emits `<NAME>_WORDS` and `<NAME>_OFFSET`
//     tables for direct HUD/client typed-array readers.
//   - `view`: when true (`$view: true`), emits a zero-copy `<Name>View` class.
//   - `publish`: subset of fields copied into a double-buffered publication snapshot.
pub struct RecordDef {
    pub name: String,
    pub file: String,
    pub observe: Vec<String>,
    pub layout: bool,
    pub publish: Vec<String>,
    pub refresh: Vec<String>,
    pub delta: Vec<String>,
    pub view: bool,
    pub reuse: bool,
    pub parts: Vec<String>,
    pub classes: Vec<String>,
    pub fields: Vec<Field>,
    pub defer: Option<Vec<String>>,
}

// A store entity kind declaration (`kinds` section). Kinds are either singletons
// (`singleton: true`, one static instance at slot 0) or slotted entities owned
// by a singleton's home list/table (`home_kind`, `home_field`). Fields are split
// across three tiers:
//   - `exports`: Rust writes, JS reads (flat `exports` row + heap side-buffer)
//   - `imports`: JS writes, Rust reads (`imports` row)
//   - `cold`: synced only on full snapshot / spawn / replay (`full` row)
#[derive(Clone, Debug)]
// Store kind metadata:
//   - `facets`: schema files that contributed fields to this kind (a kind like
//     `vehicle` can be split across `vehicles.mjs` and `flight.mjs`; `build_model`
//     merges their fields in sorted filename order).
//   - `stored`: true when Rust owns an `Arena<T>` table or singleton struct for
//     this kind; false for external JS-only reference kinds (`person`).
//   - `singleton`: true for single-instance state roots (`shelf`, `world`, `battle`).
//   - `identity`: true when instances are identified by object identity rather
//     than a numeric `$id` property.
//   - `home_kind_name` / `home_field`: for slotted kinds, the singleton kind and
//     `list:<kind>` field that owns the live instances.
//   - `delta` / `defer`: incremental and hot/cold split sync configuration.
//   - `skip`: JS properties intentionally excluded from the store mirror, with
//     human-readable reasons documented in the schema.
pub struct KindDef {
    pub name: String,
    pub rust: String,
    pub singleton: bool,
    pub key: Option<String>,
    pub home: Option<String>,
    pub identity: bool,
    pub classes: Vec<String>,
    pub fields: Vec<Field>,
    pub skip: Vec<(String, String)>,
    pub file: Option<String>,
    pub facets: Vec<String>,
    pub defer: Option<Vec<String>>,
    pub delta: bool,
    pub code: usize,
    pub stored: bool,
    pub home_kind_name: Option<String>,
    pub home_kind_code: Option<usize>,
    pub home_field_name: Option<String>,
    pub hash: u32,
}

// Insertion-ordered map helper so iteration order matches JS `Map`.
#[derive(Clone, Debug)]
// Deterministic insertion-ordered map (`Vec<(String, V)>`) that mirrors
// JavaScript's `Map` iteration order across schema declarations (`constants`,
// `enums`, `unions`, `records`, `kinds`, `commands`, `events`, `imports`).
// Lookup and in-place overwrite are linear in the declaration count (At most a
// few dozen entries per schema target), avoiding any hash-seed nondeterminism.
pub struct OrdList<V> { pub list: Vec<(String, V)>, }

impl<V> Default for OrdList<V> { fn default() -> Self { Self { list: Vec::new() } } }

impl<V> OrdList<V> {
    pub fn has(&self, k: &str) -> bool { self.list.iter().any(|(ek, _)| ek == k) }
    pub fn get(&self, k: &str) -> Option<&V> { self.list.iter().find(|(ek, _)| ek == k).map(|(_, v)| v) }
    pub fn get_mut(&mut self, k: &str) -> Option<&mut V> { self.list.iter_mut().find(|(ek, _)| ek == k).map(|(_, v)| v) }
    pub fn insert(&mut self, k: String, v: V) {
        if let Some(existing) = self.list.iter_mut().find(|(ek, _)| ek == &k) { existing.1 = v; } else { self.list.push((k, v)); }
    }
    pub fn values(&self) -> impl Iterator<Item = &V> { self.list.iter().map(|(_, v)| v) }
    pub fn keys(&self) -> impl Iterator<Item = &String> { self.list.iter().map(|(k, _)| k) }
}

// Complete validated and layout-resolved schema model produced by `build_model()`
//. Holds all declarations in both cross-file insertion
// order (`OrdList`) and sorted code order (`kind_list`, `command_list`, etc.),
// along with the 32-bit FNV-1a `hash` of the canonical layout summary.
// Complete compiled schema model for one `Target`:
//   - `files`: ordered list of schema module stems (`<stem>.mjs`) in `listed()` order.
//   - `constants`, `enums`, `unions`, `records`, `kinds`, `commands`, `events`,
//     `imports`: insertion-ordered `OrdList` maps preserving cross-file declaration
//     order for per-file codegen (`rust_file`, `js_file`).
//   - `kind_list`, `commands_list`, `events_list`, `imports_list`: lexicographically
//     sorted vectors with 1-based wire codes assigned.
//   - `key_unions`: set of union names used as `map:ref:<Union>:<V>` keys.
//   - `hash`: 32-bit FNV-1a layout hash (`LAYOUT_HASH`) over all declarations.
pub struct Model {
    pub constants: OrdList<ConstantDef>,
    pub enums: OrdList<EnumDef>,
    pub unions: OrdList<UnionDef>,
    pub records: OrdList<RecordDef>,
    pub kinds: OrdList<KindDef>,
    pub commands: OrdList<CommandDef>,
    pub events: OrdList<EventDef>,
    pub imports: OrdList<ImportDef>,
    pub files: Vec<String>,
    pub kind_list: Vec<KindDef>,
    pub commands_list: Vec<CommandDef>,
    pub events_list: Vec<EventDef>,
    pub imports_list: Vec<ImportDef>,
    pub key_unions: HashSet<String>,
    pub target: Target,
    pub heap_helpers_cache: Option<HashSet<String>>,
}

impl Model {
    pub fn is_home(&self, field: &Field) -> bool {
        self.kind_list.iter().any(|k| {
            if let (Some(hk), Some(hf)) = (&k.home_kind_name, &k.home_field_name) {
                if let Some(owner) = self.kinds.get(hk) {
                    if let Some(owner_f) = owner.fields.iter().find(|f| &f.name == hf) {
                        return owner_f.where_loc == field.where_loc && owner_f.name == field.name;
                    }
                }
            }
            false
        })
    }
}

// Extracts a `Vec<String>` from an optional JSON array of strings (such as
// `facets` or `observed` on a record or kind).
fn str_list(val: Option<&JsonValue>) -> Vec<String> {
    let Some(v) = val else { return Vec::new() };
    if let Some(arr) = v.as_array() {
        arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()
    } else if let Some(s) = v.as_str() {
        vec![s.to_string()]
    } else {
        Vec::new()
    }
}

// Multi-pass schema compiler (`buildModel(files, target)`).
//
// Pass 1 (declaration collection): iterates `files` in sorted filename order,
// validating top-level section keys and collecting `commands`, `events`,
// `imports`, `constants`, `enums`, `unions`, `records`, and `kinds` while
// enforcing global Rust type-name uniqueness (`claim()`).
//
// Pass 2 (kind numbering & home resolution): sorts kinds by name (`UTF-16` /
// ASCII order) to assign 1-based `code` values (`1..=N`), resolves `home`
// (`"<singleton>.<field>"`) references, and verifies that every slotted kind
// has a valid home list or table on a singleton kind.
//
// Pass 3 (synthetic key unions & type validation): synthesizes `<Name>Key`
// unions for `map<K1 | K2, V>` fields, resolves every named type reference
// (`enum`, `record`, `union`, `&kind`), checks union discrimination rules, and
// rejects recursive records or invalid container element types.
//
// Pass 4 (operation numbering & layout hash): sorts commands, events, and
// imports by name to assign 1-based codes, computes `LAYOUT_HASH` via 32-bit
// FNV-1a over the canonical layout description lines, and verifies that every
// record/kind row layout is well-formed.
pub fn build_model(files: &[LoadedSchema], target: Target) -> Result<Model, String> {
    let mut model = Model {
        constants: OrdList::default(),
        enums: OrdList::default(),
        unions: OrdList::default(),
        records: OrdList::default(),
        kinds: OrdList::default(),
        commands: OrdList::default(),
        events: OrdList::default(),
        imports: OrdList::default(),
        files: files.iter().map(|f| f.file.clone()).collect(),
        kind_list: Vec::new(),
        commands_list: Vec::new(),
        events_list: Vec::new(),
        imports_list: Vec::new(),
        key_unions: HashSet::new(),
        target,
        heap_helpers_cache: None,
    };

    let mut names: HashMap<String, String> = HashMap::new();
    let helpers = ["Array", "Each", "List", "Map", "Object", "Quat", "Typed", "Vec"];
    let mut claim = |rust: &str, where_loc: &str| -> Result<(), String> {
        if let Some(prev) = names.get(rust) { return Err(format!("{where_loc}: {rust} is also {prev}")); }
        if helpers.contains(&rust) {
            return Err(format!("{where_loc}: its comparison same{rust} would be src/sim/compare.js's own: rename it"));
        }
        names.insert(rust.to_string(), where_loc.to_string());
        Ok(())
    };

    let empty = Vec::new();
    for LoadedSchema { file, schema, .. } in files {
        let obj = schema.as_object().ok_or_else(|| format!("{file}: schema must be object"))?;
        for (k, _) in obj {
            if !matches!(
                k.as_str(),
                "constants" | "enums" | "unions" | "records" | "kinds" | "commands" | "events" | "imports"
            ) {
                return Err(format!("{file}: unknown section \"{k}\""));
            }
        }

        // Pass 1a: collect `commands` (JS -> Rust calls) in schema property order.
        // Each command is either a payload record reference (`{ record, returns }`),
        // an inline field map (which synthesizes a `<Pascal>Command` record), or
        // an empty `{}` no-arg command.
        for (name, spec) in schema.get("commands").and_then(|v| v.as_object()).unwrap_or(&empty) {
            let where_loc = format!("{file} command {name}");
            let args = spec.get("args").and_then(|v| v.as_str()).ok_or_else(|| format!("{where_loc}: missing args"))?;
            let rust = spec.get("rust").and_then(|v| v.as_str()).ok_or_else(|| format!("{where_loc}: missing rust"))?;
            let result = spec.get("result").and_then(|v| v.as_str()).map(|s| s.to_string());
            let owned = spec.get("owned").and_then(|v| v.as_str()).map(|s| s.to_string());
            let detached = spec.get("detached").and_then(|v| v.as_bool()).unwrap_or(false);
            model.commands.insert(
                name.clone(),
                CommandDef {
                    name: name.clone(),
                    file: file.clone(),
                    args: args.to_string(),
                    result,
                    rust: rust.to_string(),
                    owned,
                    detached,
                    where_loc,
                    code: 0,
                },
            );
        }

        // Pass 1b: collect `events` (Rust -> JS notifications) in property order.
        for (name, spec) in schema.get("events").and_then(|v| v.as_object()).unwrap_or(&empty) {
            let where_loc = format!("{file} event {name}");
            let record = spec.as_str().ok_or_else(|| format!("{where_loc}: an event is the name of the record it carries"))?;
            model.events.insert(
                name.clone(),
                EventDef { name: name.clone(), file: file.clone(), record: record.to_string(), where_loc, code: 0, },
            );
        }

        // Pass 1c: collect `imports` (Rust -> JS synchronous queries) in property order.
        for (name, spec) in schema.get("imports").and_then(|v| v.as_object()).unwrap_or(&empty) {
            let where_loc = format!("{file} import {name}");
            let args = spec.get("args").and_then(|v| v.as_str()).ok_or_else(|| format!("{where_loc}: missing args"))?;
            let result = spec.get("result").and_then(|v| v.as_str()).map(|s| s.to_string());
            let borrow = spec.get("borrow").and_then(|v| v.as_bool()).unwrap_or(false);
            model.imports.insert(
                name.clone(),
                ImportDef { name: name.clone(), file: file.clone(), args: args.to_string(), result, borrow, where_loc, code: 0, },
            );
        }

        // Pass 1d: collect numeric/string `constants` shared between Rust and JS.
        for (name, value) in schema.get("constants").and_then(|v| v.as_object()).unwrap_or(&empty) {
            let val = value.as_u32().ok_or_else(|| format!("{file}: a constant is a named u32"))?;
            model.constants.insert(name.clone(), ConstantDef { name: name.clone(), value: val, file: file.clone(), },);
        }

        // Pass 1e: collect closed string `enums` and map each variant string to
        // a unique Rust PascalCase identifier.
        for (name, values_val) in schema.get("enums").and_then(|v| v.as_object()).unwrap_or(&empty) {
            let where_loc = format!("{file} enum {name}");
            claim(name, &where_loc)?;
            let values: Vec<String> = values_val
                .as_array()
                .ok_or_else(|| format!("{where_loc}: enum must be array"))?
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect();
            let variants = values.iter().map(|v| pascal(v)).collect();
            model.enums.insert(name.clone(), EnumDef { name: name.clone(), values, file: file.clone(), variants, },);
        }

        // Pass 1f: collect tagged `unions`, validating variant tag syntax (`null`,
        // `undefined`, `ref:<K>`, `rec:<R>`, or `<scalar>`) and ensuring no two
        // variants collide on wire discrimination.
        for (name, list_val) in schema.get("unions").and_then(|v| v.as_object()).unwrap_or(&empty) {
            let where_loc = format!("{file} union {name}");
            claim(name, &where_loc)?;
            let list = list_val.as_array().ok_or_else(|| format!("{where_loc}: union must be array"))?;
            let mut variants = Vec::new();
            for v_val in list {
                let v = v_val.as_str().ok_or_else(|| format!("{where_loc}: union variant must be string"))?;
                if v == "null" || v == "undefined" {
                    variants.push(UnionVariant { tag: v.to_string(), rust: pascal(v), value: None, kind: None, ty: None, });
                } else if let Some(lit) = v.strip_prefix("lit:") {
                    variants.push(UnionVariant {
                        tag: "lit".to_string(),
                        rust: pascal(lit),
                        value: Some(lit.to_string()),
                        kind: None,
                        ty: None,
                    });
                } else if let Some(rk) = v.strip_prefix("ref:") {
                    variants.push(UnionVariant { tag: "ref".to_string(), rust: pascal(rk), value: None, kind: Some(rk.to_string()), ty: None, });
                } else {
                    let ty = parse_type(v, &where_loc)?;
                    if optional(&ty) || matches!(ty.base.as_str(), "list" | "set" | "map" | "union" | "ref" | "index" | "def" | "obj" | "typed") {
                        return Err(format!("{where_loc}: \"{v}\" cannot be a union's variant"));
                    }
                    let r_name = if matches!(ty.base.as_str(), "rec" | "enum") {
                        pascal(ty.of.as_deref().unwrap())
                    } else if ty.base == "f64xN" {
                        pascal(&format!("f64x{}", ty.n))
                    } else {
                        pascal(&ty.base)
                    };
                    variants.push(UnionVariant { tag: "value".to_string(), rust: r_name, value: None, kind: None, ty: Some(ty), });
                }
            }
            model.unions.insert(name.clone(), UnionDef { name: name.clone(), variants, file: file.clone(), words: 0, },);
        }

        // Pass 1g: collect `records`, supporting plain field maps as well as
        // delta-tracked / observed root records (`$parts`, `$publish`, `$observed`,
        // `$delta`) used by HUD and client state models.
        for (name, fields_val) in schema.get("records").and_then(|v| v.as_object()).unwrap_or(&empty) {
            let where_loc = format!("{file} record {name}");
            claim(name, &where_loc)?;
            let fields_obj = fields_val.as_object().ok_or_else(|| format!("{where_loc}: record must be object"))?;
            let classes = str_list(fields_val.get("$class"));
            let layout = fields_val.get("$layout").and_then(|v| v.as_bool()).unwrap_or(false);
            let publish = str_list(fields_val.get("$publish"));
            let refresh = str_list(fields_val.get("$refresh"));
            let delta = str_list(fields_val.get("$delta"));
            let view = fields_val.get("$view").and_then(|v| v.as_bool()).unwrap_or(false);
            let parts = str_list(fields_val.get("$parts"));
            let observe = str_list(fields_val.get("$observe"));
            let reuse = fields_val.get("$reuse").and_then(|v| v.as_bool()).unwrap_or(false);

            let mut parsed = Vec::new();
            for (f_name, f_spec) in fields_obj {
                if matches!(
                    f_name.as_str(),
                    "$class" | "$layout" | "$publish" | "$refresh" | "$delta" | "$view" | "$parts" | "$observe" | "$reuse"
                ) {
                    continue;
                }
                parsed.push(parse_field(f_name, f_spec, &format!("{where_loc}.{f_name}"))?);
            }
            if let Some(rv) = fields_val.get("$reuse") { if rv.as_bool().is_none() { return Err(format!("{where_loc}: $reuse is a boolean")); } }
            for (meta_name, list) in [("$publish", &publish), ("$refresh", &refresh), ("$delta", &delta), ("$parts", &parts), ("$observe", &observe)] {
                let unique: HashSet<&String> = list.iter().collect();
                if unique.len() != list.len() || list.iter().any(|k| !parsed.iter().any(|f| &f.name == k)) {
                    return Err(format!("{where_loc}: {meta_name} names distinct fields of this record"));
                }
            }
            for field in &parsed {
                if publish.contains(&field.name) {
                    let ok_base = matches!(field.ty.base.as_str(), "f64" | "f32" | "i32" | "u32" | "u8" | "bool" | "enum" | "vec3");
                    if !ok_base || (field.ty.base == "vec3" && optional(&field.ty)) {
                        return Err(format!("{where_loc}: $publish supports scalar and vec3 fields; vectors cannot be optional"));
                    }
                }
            }
            for obs_name in &observe {
                let f = parsed.iter().find(|f| &f.name == obs_name).unwrap();
                if f.js.len() != 1 || !matches!(f.ty.base.as_str(), "f64" | "f32" | "u32" | "i32" | "u8" | "bool" | "str" | "enum") {
                    return Err(format!("{where_loc}: $observe supports direct scalar fields"));
                }
            }
            model.records.insert(
                name.clone(),
                RecordDef {
                    name: name.clone(),
                    file: file.clone(),
                    observe,
                    layout,
                    publish,
                    refresh,
                    delta,
                    view,
                    reuse,
                    parts,
                    classes,
                    fields: parsed,
                    defer: None,
                },
            );
        }

        // Pass 1h: collect store `kinds` (both singletons and slotted entities),
        // recording their `$facets`, `$singleton`, `$key`, `$home`, `$defer`,
        // `$skip`, and field declarations.
        for (name, spec) in schema.get("kinds").and_then(|v| v.as_object()).unwrap_or(&empty) {
            let where_loc = format!("{file} kind {name}");
            if !model.kinds.has(name) {
                model.kinds.insert(
                    name.clone(),
                    KindDef {
                        name: name.clone(),
                        rust: pascal(name),
                        singleton: false,
                        key: None,
                        home: None,
                        identity: false,
                        classes: Vec::new(),
                        fields: Vec::new(),
                        skip: Vec::new(),
                        file: None,
                        facets: Vec::new(),
                        defer: None,
                        delta: false,
                        code: 0,
                        stored: false,
                        home_kind_name: None,
                        home_kind_code: None,
                        home_field_name: None,
                        hash: 0,
                    },
                );
            }
            let kind = model.kinds.get_mut(name).unwrap();
            kind.facets.push(file.clone());
            if spec.get("defer").is_some() { kind.defer = Some(str_list(spec.get("defer"))); }
            if let Some(d) = spec.get("delta").and_then(|v| v.as_bool()) { kind.delta = d; }
            if let Some(s) = spec.get("singleton").and_then(|v| v.as_bool()) { kind.singleton = s; }
            if let Some(k) = spec.get("key").and_then(|v| v.as_str()) { kind.key = Some(k.to_string()); }
            if let Some(h) = spec.get("home").and_then(|v| v.as_str()) { kind.home = Some(h.to_string()); }
            if let Some(id) = spec.get("identity").and_then(|v| v.as_bool()) { kind.identity = id; }
            for (f, spec2) in spec.get("fields").and_then(|v| v.as_object()).unwrap_or(&empty) {
                let field = parse_field(f, spec2, &format!("{where_loc}.{f}"))?;
                if let Some(had) = kind.fields.iter_mut().find(|x| x.name == *f) {
                    had.mirror = had.mirror || field.mirror;
                    had.import = had.import || field.import;
                    continue;
                }
                kind.fields.push(field);
                if kind.file.is_none() { kind.file = Some(file.clone()); }
            }
            for (f, why) in spec.get("skip").and_then(|v| v.as_object()).unwrap_or(&empty) {
                let w = why.as_str().unwrap_or("").to_string();
                if let Some(existing) = kind.skip.iter_mut().find(|(k, _)| k == f) { existing.1 = w; } else { kind.skip.push((f.clone(), w)); }
            }
            for c in str_list(spec.get("class")) { if !kind.classes.contains(&c) { kind.classes.push(c); } }
        }
    }

    // Pass 2: sort all kinds lexicographically by name (`Array.prototype.sort()`)
    // and assign 1-based kind codes (`1..=N`). Singletons and slotted kinds share
    // this single sorted code space so `KIND_<NAME>` indices match across Rust and JS.
    let mut kinds: Vec<KindDef> = model.kinds.values().cloned().collect();
    kinds.sort_by(|a, b| a.name.cmp(&b.name));
    for (i, k) in kinds.iter_mut().enumerate() { k.code = i + 1; k.stored = !k.fields.is_empty(); claim(&k.rust, &format!("kind {}", k.name))?; }

    // Resolve home references and sync back into model.kinds
    let code_by_name: HashMap<String, usize> = kinds.iter().map(|k| (k.name.clone(), k.code)).collect();
    for k in &mut kinds {
        let where_loc = format!("kind {}", k.name);
        if k.identity && (!k.stored || k.singleton || k.key.is_some() || k.home.is_some()) {
            return Err(format!("{where_loc}: identity fields have no key, home or singleton"));
        }
        if let Some(defer_list) = &k.defer {
            for d_name in defer_list {
                let Some(f) = k.fields.iter().find(|f| &f.name == d_name) else {
                    return Err(format!("{where_loc}: defer names distinct fields"));
                };
                if f.ty.base != "rec" || !optional(&f.ty) || !f.mirror {
                    return Err(format!("{where_loc}: defer supports mirrored optional records"));
                }
            }
        }
        if let Some(home) = &k.home {
            if let Some((sys, list)) = home.split_once('.') {
                k.home_kind_name = Some(sys.to_string());
                k.home_kind_code = code_by_name.get(sys).copied();
                k.home_field_name = Some(list.to_string());
            }
        } else if k.stored && !k.singleton && k.key.is_none() && !k.identity {
            return Err(format!("{where_loc}: an entity kind with fields needs a home list, a key or identity"));
        }
        if let Some(mk) = model.kinds.get_mut(&k.name) { *mk = k.clone(); }
    }
    model.kind_list = kinds;

    // Identify and validate keyUnions
    // Pass 3: scan all record and kind fields for `map:ref:<Union>:<V>` where the
    // map key names a union of entity references, marking those unions as
    // `key_union = true` and verifying every variant is a `ref:<kind>`.
    let mut key_unions_found = Vec::new();
    for f in model.records.values().flat_map(|r| r.fields.iter()).chain(model.kind_list.iter().flat_map(|k| k.fields.iter())) {
        if let Some(key) = &f.ty.key {
            if key.base == "ref" {
                let k_of = key.of.as_ref().unwrap();
                if model.kinds.has(k_of) {
                    if model.kinds.get(k_of).unwrap().singleton { return Err(format!("{}: {k_of} is a singleton", f.where_loc)); }
                } else if let Some(u) = model.unions.get(k_of) {
                    if !u.variants.iter().all(|v| v.tag == "ref") {
                        return Err(format!("{}: a map's keys of union {k_of}: each of its variants is a kind", f.where_loc));
                    }
                    key_unions_found.push(k_of.clone());
                } else {
                    return Err(format!("{}: no kind or union {k_of}", f.where_loc));
                }
            }
        }
    }
    for ku in key_unions_found { model.key_unions.insert(ku); }

    // Compute union words
    let union_names: Vec<String> = model.unions.keys().cloned().collect();
    for u_name in union_names {
        let u = model.unions.get(&u_name).unwrap().clone();
        let w = union_words(&model, &u);
        model.unions.get_mut(&u_name).unwrap().words = w;
    }

    // Pass 4: sort commands, events, and imports by name (`Array.prototype.sort()`)
    // to assign deterministic 1-based wire codes (`CMD_*`, `EVT_*`, `IMP_*`),
    // then validate every field's target type and check for recursive record cycles.
    let mut commands_sorted: Vec<CommandDef> = model.commands.values().cloned().collect();
    commands_sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for (i, c) in commands_sorted.iter_mut().enumerate() { c.code = i + 1; }
    model.commands_list = commands_sorted;

    let mut events_sorted: Vec<EventDef> = model.events.values().cloned().collect();
    events_sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for (i, e) in events_sorted.iter_mut().enumerate() { e.code = i + 1; }
    model.events_list = events_sorted;

    let mut imports_sorted: Vec<ImportDef> = model.imports.values().cloned().collect();
    imports_sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for (i, imp) in imports_sorted.iter_mut().enumerate() { imp.code = i + 1; }
    model.imports_list = imports_sorted;

    fn spell(model: &Model, fields: &[Field]) -> String {
        fields
            .iter()
            .map(|f| {
                let flags = format!("{}{}", if f.mirror { "" } else { "-m" }, if f.import { "" } else { "-i" });
                let ty_str = if f.ty.base == "rec" {
                    let rec = model.records.get(f.ty.of.as_ref().unwrap()).unwrap();
                    format!("{{{}}}", spell(model, &rec.fields))
                } else {
                    f.ty.text.clone()
                };
                format!("{}={}{}:{}", f.name, f.js.join("."), flags, ty_str)
            })
            .collect::<Vec<_>>()
            .join(";")
    }

    // Pass 5: layout hash calculation (`LAYOUT_HASH`).
    //   - Starts from the 32-bit FNV-1a offset basis (`0x811c9dc5`, initialized on
    //     `Model::hash`) and folds in canonical signature strings for every:
    //     * kind (`kind:<code>:<name>:<singleton>:<stored>:<key>:<home>:...` + field specs)
    //     * record (`rec:<name>:...` + field specs)
    //     * enum (`enum:<name>:<v0>,<v1>,...`)
    //     * union (`union:<name>:<tag0>,<tag1>,...`)
    //     * constant (`const:<name>:<value>`)
    //     * command, event, and import (`cmd:...`, `event:...`, `import:...`)
    //     * operation (`op:<code>:<name>:...`)
    //   - Any change to field order, type, nullability, tier, or declaration set
    //     changes `model.hash`, preventing mismatched JS/Wasm builds from running.
    // Pass 5: compute each kind's export/import row layouts and fold every
    // declaration's canonical signature into `model.hash` via 32-bit FNV-1a
    // (`fnv()`), producing the `LAYOUT_HASH` constant checked at boundary init.
    for i in 0..model.kind_list.len() {
        let k = &model.kind_list[i];
        let hash_input = format!(
            "{}{}{}({})",
            k.name,
            if k.singleton { "!" } else { "" },
            if let Some(key) = &k.key { format!("#{key}") } else { String::new() },
            spell(&model, &k.fields)
        );
        let h = fnv(&hash_input);
        model.kind_list[i].hash = h;
        let name = model.kind_list[i].name.clone();
        model.kinds.get_mut(&name).unwrap().hash = h;
    }

    Ok(model)
}

// Returns the flat `f64` word width of `ty` inside a row or record (`words(model, type)`
//). Scalars take 1 word (`vec3` -> 3, `quat` -> 4,
// `mat4` -> 16, `f64xN` -> N, `str` -> 2 `[offset, len]`), containers take 2
// words (`[offset, count]`), records take the sum of their fields' words, and
// unions take `1 + max(variant words)`. A nullable type with a separate tag adds
// 1 leading tag word.
pub fn words(model: &Model, ty: &TypeSpec) -> usize {
    let tag = if optional(ty) { 1 } else { 0 };
    // Compute total flat `f64` words (`tag` is 1 when the nullable type needs a
    // dedicated presence word, or 0 when non-null or using an inline `NaN`/`-1`
    // sentinel):
    //   - `vec3` (3), `quat` (4), `mat4` (16), `f64xN` / `typed` (N)
    //   - `union` (1 tag word + max variant words), `rec` (sum of field words)
    //   - `list` / `set` / `map` / `obj` (2 words: `[heap_offset, count]`)
    //   - all scalars, enums, handles, and table indices occupy 1 word.
    match ty.base.as_str() {
        "vec3" => tag + 3,
        "quat" => tag + 4,
        "mat4" => tag + 16,
        "f64xN" | "typed" => tag + ty.n,
        "ref" => tag + 2,
        "union" => tag + union_words(model, model.unions.get(ty.of.as_ref().unwrap()).unwrap()),
        "list" | "set" | "map" | "obj" => tag + 2,
        "rec" => {
            let rec = model.records.get(ty.of.as_ref().unwrap()).unwrap();
            tag + rec.fields.iter().map(|f| words(model, &f.ty)).sum::<usize>()
        }
        _ => tag + 1,
    }
}

// Computes the flat `f64` word width of union `u`: 1 tag word plus the maximum
// word width across all of its variants (`unionWords(model, u)`).
// Union word width calculator (`unionWords(model, u)`):
//   - Word 0 stores the 0-based variant tag (`0.0 .. (variants.len() - 1) as f64`).
//   - Words `1..=max_payload` store the active variant's payload (`0` words for
//     `null`, `undefined`, and `lit:<name>`; `1` word for `ref:<kind>`; or
//     `words(model, ty)` for value/record variants).
pub fn union_words(model: &Model, u: &UnionDef) -> usize {
    if u.words > 0 { return u.words; }
    let max_v = u
        .variants
        .iter()
        .map(|v| match v.tag.as_str() { "ref" => 2, "value" => words(model, v.ty.as_ref().unwrap()), _ => 0, })
        .max()
        .unwrap_or(0);
    1 + max_v
}

// Returns the flat `f64` word width of a `map<K, V>` key on the heap (`1` for
// `i32`/`enum`, `2` for `str`, or `union_words` for a key union). Mirrors
// `keyWords(model, t)`.
// Map key word width calculator (`keyWords(model, t)`):
//   - When `t.key` is a union of entity references (`map:ref:<Union>:<V>`), the
//     key takes `2` words on the heap (`[variant_tag, entity_slot]`).
//   - For all single-type map keys (`str` `StrId`, `num` `f64`, or single
//     `ref:<kind>` slot), the key takes `1` word on the heap.
pub fn key_words(model: &Model, t: &TypeSpec) -> usize {
    let Some(key) = &t.key else { return 1 };
    if key.base != "ref" { return 1; }
    let k_of = key.of.as_ref().unwrap();
    if let Some(u) = model.unions.get(k_of) { union_words(model, u) } else { 2 }
}

// Returns the flat `f64` word stride of one `map<K, V>` entry on the heap:
// `key_words(K) + words(V)`. Mirrors `entryWords(model, t)`.
pub fn entry_words(model: &Model, t: &TypeSpec) -> usize { key_words(model, t) + words(model, t.value.as_ref().unwrap()) }

// A field placed at a concrete `f64` word `offset` and `width` inside a row or
// record layout (`layout()`).
#[derive(Clone, Debug)]
pub struct PlacedField { pub field: Field, pub at: usize, }

// The computed `f64` word layout of a kind tier (`"exports"`, `"imports"`, or
// `"full"`), holding the total `words` stride and the ordered `placed` fields.
#[derive(Clone, Debug)]
pub struct RowLayout { pub fields: Vec<PlacedField>, pub stride: usize, }

// Computes the `RowLayout` for a kind's `"exports"`, `"imports"`, or `"full"`
// row (`layout(model, kind, which)`).
//   - `"exports"`: only `export`-tier fields
//   - `"imports"`: only `import`-tier fields
//   - `"full"`: all fields (`export`, `import`, and `cold`) in declaration order
pub fn layout(model: &Model, kind: &KindDef, which: &str) -> RowLayout {
    // Every kind row reserves word 0 (`slot`) and word 1 (`generation` / metadata),
    // so placed fields start at word offset 2.
    let mut at = 2usize;
    let mut placed = Vec::new();
    for f in &kind.fields {
        let keep = match which { "export" => f.mirror, "import" => f.import, _ => true, };
        if !keep { continue; }
        placed.push(PlacedField { field: f.clone(), at, });
        at += words(model, &f.ty);
    }
    RowLayout { fields: placed, stride: at, }
}

// Returns true when two `RowLayout`s have the exact same word count and field
// sequence, allowing codegen to alias `full` row readers/writers to `exports`
// when a kind has only export-tier fields (`sameLayout(a, b)`).
pub fn same_layout(a: &RowLayout, b: &RowLayout) -> bool {
    a.stride == b.stride
        && a.fields.len() == b.fields.len()
        && a.fields
            .iter()
            .zip(b.fields.iter())
            .all(|(p, q)| p.field.name == q.field.name)
}

// Returns true if record `name` contains no heap-allocated fields (no strings,
// containers, or nested non-flat records/unions), allowing `vec<Record>` to pack
// elements contiguously on the heap without per-element heap pointers and
// enabling JS flat-view wrappers. Mirrors `flat(model, name)`.
// Flatness predicate for record `name` (`flat(model, name)`):
//   - A record is flat when none of its fields (or nested sub-records/unions)
//     allocate on the side-buffer heap (`list`, `set`, `map`, `obj`, `typed`).
//   - Note: `str` (`StrId`) is a 1-word interned string handle in the simulation
//     store, so a record containing `str` fields is still flat in the row buffer.
//   - `list:rec:<Flat>` packs its elements contiguously in a single heap span of
//     `len * Record::WORDS` words without per-element indirection.
pub fn flat(model: &Model, name: &str) -> bool {
    model
        .records
        .get(name)
        .unwrap()
        .fields
        .iter()
        .all(|f| {
            !matches!(f.ty.base.as_str(), "list" | "set" | "map" | "obj")
                && (f.ty.base != "rec" || flat(model, f.ty.of.as_ref().unwrap()))
        })
}

// Returns the total flat `f64` word width of record `name` across all of its
// fields (`recordWords(model, name)`).
pub fn record_words(model: &Model, name: &str) -> usize {
    let rec = model.records.get(name).unwrap();
    rec.fields.iter().map(|f| words(model, &f.ty)).sum()
}

// Describes the return-value wire protocol of a command, event, or import
// (`operationsOf(model)`): `None` for one-way calls,
// `Scalar` for single-word primitive returns, or `Heap` when the answer uses
// the return buffer (`ret` + `heap`).
#[derive(Clone, Debug, PartialEq)]
pub enum OpAnswers { None, Block, Scalar, }

// Unified descriptor for a command, event, or import produced by
// `operations_of(model)` (`operationsOf(model)`).
#[derive(Clone, Debug)]
pub struct OperationSpec {
    pub name: String,
    pub command: Option<CommandDef>,
    pub rust: Option<String>,
    pub scalars: Vec<&'static str>,
    pub record: Option<String>,
    pub heap: bool,
    pub answers: OpAnswers,
    pub code: usize,
}

// Collects all commands, events, and imports in sorted code order into a single
// list of `OperationSpec` descriptors for Rust and JS dispatch codegen.
// Mirrors `operationsOf(model)`.
// Unified operation table builder (`operationsOf(model)`):
//   - Combines all schema-declared `commands` with the built-in store lifecycle
//     and query operations (`init`, `reset`, `step`, `spawn`, `remove`, etc.) into
//     a single 1-indexed `OP_*` code space shared by Rust and JS.
pub fn operations_of(model: &Model) -> Vec<OperationSpec> {
    let mut out = Vec::new();
    for c in &model.commands_list {
        if flat(model, &c.args) && c.name != "relay" {
            out.push(OperationSpec {
                name: c.name.clone(),
                command: Some(c.clone()),
                rust: None,
                scalars: Vec::new(),
                record: Some(c.args.clone()),
                heap: false,
                answers: if c.result.is_some() { OpAnswers::Block } else { OpAnswers::None },
                code: 0,
            });
        }
    }
    type BuiltinOp<'a> = (&'a str, &'a str, &'static [&'static str], Option<&'a str>, OpAnswers);
    let exports: &[BuiltinOp<'_>] = &[
        ("inputMailboxOwnedRun", "input::module::input_mailbox_owned_run_in", &["u32"], Some("InputMailboxCall"), OpAnswers::Block),
        ("inputMailboxView", "input::module::input_mailbox_view_in", &["u32"], Some("InputViewUpdate"), OpAnswers::None),
        ("wireEdgesRun", "wire::edges::wire_edges_run_in", &["usize"], Some("WeCall"), OpAnswers::Block),
        ("wireHostOp", "wire::host::wire_host_op_in", &["usize", "u32", "u32", "f64", "f64"], None, OpAnswers::Scalar),
        ("playersInput", "players::module::players_input_in", &["u32", "f64", "f64", "u32"], None, OpAnswers::None),
        ("playersFire", "players::module::players_fire_in", &["u32"], Some("PlayerFire"), OpAnswers::Scalar),
        ("playersActions", "players::module::players_actions_in", &["u32"], Some("PlayerActions"), OpAnswers::Block),
    ];
    for &(name, rust, scalars, record, ref answers) in exports {
        let heap = record.map(|r| !flat(model, r)).unwrap_or(false);
        out.push(OperationSpec {
            name: name.to_string(),
            command: None,
            rust: Some(rust.to_string()),
            scalars: scalars.to_vec(),
            record: record.map(|s| s.to_string()),
            heap,
            answers: answers.clone(),
            code: 0,
        });
    }
    for (i, o) in out.iter_mut().enumerate() { o.code = i + 1; }
    out
}

// ---- Rust code generation ----
//
// Each non-`layoutOnly` schema file `<file>.mjs` produces `<rust_dir>/<file>.rs`
// containing the Rust structs, enums, unions, and row serialization methods for
// declarations originating in that file. In addition, `<rust_dir>/mod.rs`
// aggregates all per-file modules into the `Store` struct and generates the
// command, event, import, and operation dispatch tables.
//
// Word-packing conventions shared between Rust and JS:
//   - Every value is packed into a slice of `f64` words (`row: &mut [f64]`) plus
//     an append-only per-call / per-tick `heap: &mut Vec<f64>` side-buffer.
//   - `i32`, `u32`, `u8`, `bool`, `enum`, `Id<K>`, `Ref<K>`, and `StrId` each
//     occupy 1 `f64` word via lossless integer-to-double conversion (IEEE-754
//     `f64` has 53 bits of significand, holding all 32-bit integers exactly).
//   - `list:<T>`, `set:<K>`, `map:<K, V>`, and `obj:<V>` write `[offset, count]`
//     into 2 words in the parent row and append `count * stride` words to `heap`.

// Returns the Rust key type string for a `map<K, V>` field (`String`, `i32`,
// enum PascalCase name, or synthesized key-union PascalCase name). Mirrors
// `rustKey(model, t)`.
fn rust_key(model: &Model, t: &TypeSpec) -> String {
    let k_of = t.key.as_ref().unwrap().of.as_ref().unwrap();
    if let Some(k) = model.kinds.get(k_of) { format!("Ref<{}>", k.rust) } else { k_of.clone() }
}

// Returns the Rust type string for `t` (`rustType(model, t, inner)` in
// the schema generator). When `inner` is false and `t` is nullable, wraps the
// bare Rust type in `Option<...>`.
pub fn rust_type(model: &Model, t: &TypeSpec, inner: bool) -> String {
    // Map each bare schema `TypeSpec` to its owned Rust struct field type:
    //   - math types use `Vec3` (`[f64; 3]`), `Quat` (`[f64; 4]`), `Mat4` (`[f64; 16]`)
    //   - `str` -> `StrId` (interned string handle in the store string table)
    //   - `enum`/`rec`/`union` -> generated PascalCase Rust type
    //   - `ref:<kind>` -> `Id<Kind>` (stored entity) or `Ref<Kind>` (external JS ref)
    //   - `list:<T>` / `<Typed>Array(N)` -> `Vec<T>`
    //   - `set:<K>` -> `Vec<Id<K>>` or `Vec<Ref<K>>`
    //   - `map:<K, V>` / `obj:<V>` -> `Vec<(K, V)>`
    let base = match t.base.as_str() {
        "f64" | "f32" | "i32" | "u32" | "u8" | "bool" => t.base.clone(),
        "index" => "u32".to_string(),
        "vec3" => "Vec3".to_string(),
        "quat" => "Quat".to_string(),
        "mat4" => "Mat4".to_string(),
        "f64xN" => format!("[f64; {}]", t.n),
        "str" => "StrId".to_string(),
        "enum" | "union" | "rec" => t.of.clone().unwrap(),
        "def" => "DefId".to_string(),
        "ref" => format!("Ref<{}>", model.kinds.get(t.of.as_ref().unwrap()).unwrap().rust),
        "list" => format!("Vec<{}>", rust_type(model, t.value.as_ref().unwrap(), false)),
        "set" => format!("RefSet<{}>", model.kinds.get(t.of.as_ref().unwrap()).unwrap().rust),
        "map" => {
            let val_ty = rust_type(model, t.value.as_ref().unwrap(), false);
            match &t.key {
                None => format!("StrMap<{val_ty}>"),
                Some(k) if k.base == "num" => format!("NumMap<{val_ty}>"),
                Some(_) => format!("RefMap<{}, {val_ty}>", rust_key(model, t)),
            }
        }
        "obj" => format!("IntObj<{}>", rust_type(model, t.value.as_ref().unwrap(), false)),
        "typed" => format!("TypedArray<{}, {}>", t.elem.as_ref().unwrap(), t.n),
        other => panic!("unexpected base {other}"),
    };
    if inner || !optional(t) { return base; }
    if t.nullable.null && t.nullable.undefined { format!("Nullable<{base}>") } else { format!("Option<{base}>") }
}

// Returns `"*"` for `Copy` value types (`f64`, `f32`, `i32`, `u32`, `u8`, `bool`,
// `vec3`, `quat`, `mat4`, `f64xN`, `&kind`, `enum`) and `""` for heap-owning or
// composite types when iterating slices in generated Rust code. Mirrors
// `deref(t)`.
pub fn deref(t: &TypeSpec) -> &'static str {
    if matches!(t.base.as_str(), "rec" | "list" | "set" | "map" | "obj" | "typed") { "v" } else { "(*v)" }
}

// Returns `""` for `Copy` element types and `"&"` for borrowed element references
// when iterating `Vec<T>` in generated Rust writers (`itemOf(t)`).
pub fn item_of(t: &TypeSpec) -> &'static str {
    if matches!(t.base.as_str(), "rec" | "list" | "set" | "map" | "obj" | "typed") { "e" } else { "(*e)" }
}

// Returns true if `t` (or any nested field/variant inside `t`) writes variable-
// length data to `heap: &mut Vec<f64>` during `put_*` serialization (`rustOwnsHeap`
//), determining whether `heap` must be mutable/used.
// Determines whether packing or unpacking `t` touches the `heap` side-buffer:
//   - `list`, `set`, `map`, `obj`, and `typed` always read/write `heap`.
//   - `rec` touches `heap` if and only if `!flat(model, rec_name)`.
//   - `union` touches `heap` if any variant's payload type touches `heap`.
// Used by `rust_rows`, `rust_union`, and `rust_record` to decide whether the
// `heap` parameter should be named `heap` or `_heap`.
pub fn rust_owns_heap(model: &Model, t: &TypeSpec) -> bool {
    if matches!(t.base.as_str(), "list" | "set" | "map" | "obj") { return true; }
    if t.base == "rec" { return model .records .get(t.of.as_ref().unwrap()) .unwrap() .fields .iter() .any(|f| rust_owns_heap(model, &f.ty)); }
    if t.base == "union" {
        return model
            .unions
            .get(t.of.as_ref().unwrap())
            .unwrap()
            .variants
            .iter()
            .any(|v| v.tag == "value" && rust_owns_heap(model, v.ty.as_ref().unwrap()));
    }
    false
}

// Generates the Rust statement that serializes a non-null value expression `x`
// of bare type `t` into `row[k..]` (and `heap` when variable-length). Mirrors
// `rustPut(model, t, x, k)`.
pub fn rust_put(model: &Model, t: &TypeSpec, x: &str, k: &str) -> String {
    // Dispatch on the bare type to emit the Rust word-packing statement:
    //   - scalars pack directly into `row[k]` (casting integers/enums/bools to `f64`)
    //   - fixed arrays (`vec3`, `quat`, `mat4`, `f64xN`) copy contiguous slices
    //   - `str` writes the `StrId` word (`(x).0 as f64`)
    //   - `rec` / `union` recurse into `<Type>::put` at `row[k..]`
    //   - `list` / `set` / `map` / `obj` / `typed` allocate a contiguous block
    //     on `heap` and store `[offset, len]` at `row[k]` and `row[k + 1]`.
    match t.base.as_str() {
        "f64" => format!("row[{k}] = {x};"),
        "f32" | "i32" | "u32" | "u8" | "index" => format!("row[{k}] = {x} as f64;"),
        "bool" => format!("row[{k}] = if {x} {{ 1.0 }} else {{ 0.0 }};"),
        "vec3" | "quat" | "mat4" | "f64xN" => {
            let n = match t.base.as_str() { "vec3" => 3, "quat" => 4, "mat4" => 16, _ => t.n, };
            format!("row[{k}..{k} + {n}].copy_from_slice(&{x});")
        }
        "str" | "def" => format!("row[{k}] = {x}.0 as f64;"),
        "enum" => format!("row[{k}] = {x} as u8 as f64;"),
        "ref" => format!("row[{k}] = {x}.slot as f64; row[{k} + 1] = {x}.gen as f64;"),
        "union" => {
            let uw = union_words(model, model.unions.get(t.of.as_ref().unwrap()).unwrap());
            format!("{x}.put(&mut row[{k}..{k} + {uw}], heap);")
        }
        "rec" => format!("{x}.put(&mut row[{k}..], heap);"),
        "typed" => {
            let n = t.n;
            if t.elem.as_deref() == Some("f64") {
                format!("row[{k}..{k} + {n}].copy_from_slice(&{x}.0);")
            } else {
                format!("for i in 0..{n} {{ row[{k} + i] = {x}.0[i] as f64; }}")
            }
        }
        "list" => {
            let v = t.value.as_ref().unwrap();
            let w = words(model, v);
            let item = item_of(v);
            let header = format!("let at = heap.len(); row[{k}] = at as f64; row[{k} + 1] = {x}.len() as f64;");
            if !optional(v) && v.base == "f64" { return format!("{{ {header} heap.extend_from_slice({x}.as_slice()); }}"); }
            if !optional(v) && matches!(v.base.as_str(), "vec3" | "quat" | "mat4" | "f64xN") {
                return format!("{{ {header} heap.extend_from_slice({x}.as_flattened()); }}");
            }
            let inner_put = rust_put_field(model, v, item, "0");
            if !rust_owns_heap(model, v) {
                return format!("{{ {header} heap.reserve({x}.len() * {w}); for e in {x}.iter() {{ let mut item = [0.0f64; {w}]; {{ let row = &mut item; {inner_put} }} heap.extend_from_slice(&item); }} }}");
            }
            format!("{{ let at = heap.len(); row[{k}] = at as f64; row[{k} + 1] = {x}.len() as f64; heap.resize(at + {x}.len() * {w}, 0.0); for (i, e) in {x}.iter().enumerate() {{ let mut item = [0.0f64; {w}]; {{ let row = &mut item; {inner_put} }} heap[at + i * {w}..at + (i + 1) * {w}].copy_from_slice(&item); }} }}")
        }
        "set" => format!("row[{k}] = heap.len() as f64; row[{k} + 1] = {x}.items.len() as f64; for r in {x}.items.iter() {{ heap.push(r.slot as f64); heap.push(r.gen as f64); }}"),
        "map" | "obj" => {
            let v = t.value.as_ref().unwrap();
            let w = entry_words(model, t);
            let kw = key_words(model, t);
            let value = item_of(v);
            if t.base == "map" && t.key.is_none() {
                let inner_put = rust_put_field(model, v, value, "1");
                return format!("{{ let at = heap.len(); row[{k}] = at as f64; row[{k} + 1] = {x}.entries.len() as f64; heap.resize(at + {x}.entries.len() * {w}, 0.0); for (i, (key, e)) in {x}.entries.iter().enumerate() {{ let mut entry = [0.0f64; {w}]; entry[0] = key.0 as f64; {{ let row = &mut entry; {inner_put} }} heap[at + i * {w}..at + (i + 1) * {w}].copy_from_slice(&entry); }} }}");
            }
            let key_code = if t.base == "obj" || t.key.as_ref().unwrap().base == "num" {
                "entry[0] = key;".to_string()
            } else if model.kinds.has(t.key.as_ref().unwrap().of.as_ref().unwrap()) {
                "entry[0] = key.slot as f64; entry[1] = key.gen as f64;".to_string()
            } else {
                format!("key.put(&mut entry[0..{kw}], heap);")
            };
            let inner_put = rust_put_field(model, v, value, &kw.to_string());
            format!("{{ let at = heap.len(); row[{k}] = at as f64; row[{k} + 1] = {x}.len() as f64; heap.resize(at + {x}.len() * {w}, 0.0); for (i, (key, e)) in {x}.iter().enumerate() {{ let mut entry = [0.0f64; {w}]; {key_code} {{ let row = &mut entry; {inner_put} }} heap[at + i * {w}..at + (i + 1) * {w}].copy_from_slice(&entry); }} }}")
        }
        other => panic!("unexpected base {other}"),
    }
}

// Generates the Rust statement that serializes a potentially nullable field `x`
// of type `t` into `row[k..]`, writing either the null sentinel (`f64::NAN` /
// `-1.0`) or tag word (`0.0` vs `1.0`) before delegating to `rust_put`.
// Mirrors `rustPutField(model, t, x, k)`.
pub fn rust_put_field(model: &Model, t: &TypeSpec, x: &str, k: &str) -> String {
    if !optional(t) { return rust_put(model, t, x, k); }
    let inner = rust_put(model, &bare(t), deref(t), &format!("{k} + 1"));
    let clear = format!("row[{k} + 1..{k} + {}].fill(0.0);", words(model, t));
    if t.nullable.null && t.nullable.undefined {
        format!("match &{x} {{ Nullable::Value(v) => {{ row[{k}] = 0.0; {inner} }} Nullable::Null => {{ row[{k}] = 1.0; {clear} }} Nullable::Undefined => {{ row[{k}] = 2.0; {clear} }} }}")
    } else {
        let tag = if t.nullable.null { "1.0" } else { "2.0" };
        format!("match &{x} {{ Some(v) => {{ row[{k}] = 0.0; {inner} }} None => {{ row[{k}] = {tag}; {clear} }} }}")
    }
}

// Generates the Rust expression that decodes one scalar element of a `vec<scalar>`
// from a single `f64` word `w` on the heap (`typedTake(elem, w)`).
fn typed_take(elem: &str, w: &str) -> String {
    match elem {
        "f64" => w.to_string(),
        "f32" => format!("{w} as f32"),
        "i32" => format!("word_i32({w})"),
        "u32" => format!("word_u32({w})"),
        "i16" => format!("word_i32({w}) as i16"),
        "u16" => format!("word_u32({w}) as u16"),
        "i8" => format!("word_i32({w}) as i8"),
        "u8" => format!("word_u32({w}) as u8"),
        other => panic!("unexpected typed elem {other}"),
    }
}

// Generates the Rust expression that deserializes a non-null value of bare type
// `t` from `row[k..]` and `heap`. Mirrors `rustTake(model, t, k)` in
// the schema generator.
pub fn rust_take(model: &Model, t: &TypeSpec, k: &str) -> String {
    // Dispatch on the bare type to emit the Rust word-unpacking expression
    // from `row[k..]` and `heap`:
    //   - scalars read `row[k]` (with `f32`, `word_i32`, `word_u32`, `word_u8`, or `!= 0.0`)
    //   - `vec3`, `quat`, `mat4`, `f64xN` convert slices via `try_into().unwrap()`
    //   - `str` wraps `StrId(word_u32(row[k]))`
    //   - `rec` / `union` call `<Type>::take(&row[k..], heap)`
    //   - containers read `(at, len)` via `span(row, k)` and decode `heap` slices.
    match t.base.as_str() {
        "f64" => format!("row[{k}]"),
        "f32" => format!("row[{k}] as f32"),
        "i32" => format!("word_i32(row[{k}])"),
        "u32" | "index" => format!("word_u32(row[{k}])"),
        "u8" => format!("word_u32(row[{k}]) as u8"),
        "bool" => format!("row[{k}] != 0.0"),
        "vec3" => format!("[row[{k}], row[{k} + 1], row[{k} + 2]]"),
        "quat" => format!("[row[{k}], row[{k} + 1], row[{k} + 2], row[{k} + 3]]"),
        "mat4" => format!("{{ let mut m = [0.0; 16]; m.copy_from_slice(&row[{k}..{k} + 16]); m }}"),
        "f64xN" => { let n = t.n; format!("{{ let mut a = [0.0; {n}]; a.copy_from_slice(&row[{k}..{k} + {n}]); a }}") }
        "str" => format!("StrId(word_u32(row[{k}]))"),
        "def" => format!("DefId(word_u32(row[{k}]))"),
        "enum" => format!("{}::from_u8(word_u32(row[{k}]) as u8).unwrap_or_default()", t.of.as_ref().unwrap()),
        "ref" => format!("Ref::new(word_u32(row[{k}]), word_u32(row[{k} + 1]))"),
        "union" => {
            let u_name = t.of.as_ref().unwrap();
            let uw = union_words(model, model.unions.get(u_name).unwrap());
            format!("{u_name}::take(&row[{k}..{k} + {uw}], heap)")
        }
        "rec" => format!("{}::take(&row[{k}..], heap)", t.of.as_ref().unwrap()),
        "typed" => {
            let elem = t.elem.as_ref().unwrap();
            let n = t.n;
            let tt = typed_take(elem, &format!("row[{k} + i]"));
            format!("{{ let mut a = TypedArray::<{elem}, {n}>::default(); for i in 0..{n} {{ a.0[i] = {tt}; }} a }}")
        }
        "list" => {
            let v = t.value.as_ref().unwrap();
            let w = words(model, v);
            let inner = rust_take_field(model, v, "0");
            format!("{{ let (at, n) = (word_u32(row[{k}]) as usize, word_u32(row[{k} + 1]) as usize); let mut out = Vec::with_capacity(n); for i in 0..n {{ let row = &heap[at + i * {w}..at + (i + 1) * {w}]; out.push({inner}); }} out }}")
        }
        "set" => format!("{{ let (at, n) = (word_u32(row[{k}]) as usize, word_u32(row[{k} + 1]) as usize); RefSet {{ items: (0..n).map(|i| Ref::new(word_u32(heap[at + 2 * i]), word_u32(heap[at + 2 * i + 1]))).collect() }} }}"),
        "map" | "obj" => {
            let v = t.value.as_ref().unwrap();
            let w = entry_words(model, t);
            let kw = key_words(model, t);
            if t.base == "map" && t.key.is_none() {
                let inner = rust_take_field(model, v, "1");
                return format!("{{ let (at, n) = (word_u32(row[{k}]) as usize, word_u32(row[{k} + 1]) as usize); let mut m = StrMap::default(); for i in 0..n {{ let row = &heap[at + i * {w}..at + (i + 1) * {w}]; let value = {inner}; m.entries.push((StrId(word_u32(row[0])), value)); }} m }}");
            }
            let (make, key_expr) = if t.base == "obj" {
                ("IntObj", "row[0]".to_string())
            } else if t.key.as_ref().unwrap().base == "num" {
                ("NumMap", "row[0]".to_string())
            } else {
                let k_of = t.key.as_ref().unwrap().of.as_ref().unwrap();
                if model.kinds.has(k_of) {
                    ("RefMap", "Ref::new(word_u32(row[0]), word_u32(row[1]))".to_string())
                } else {
                    ("RefMap", format!("{k_of}::take(&row[0..{kw}], heap)"))
                }
            };
            let inner = rust_take_field(model, v, &kw.to_string());
            format!("{{ let (at, n) = (word_u32(row[{k}]) as usize, word_u32(row[{k} + 1]) as usize); let mut m = {make}::default(); for i in 0..n {{ let row = &heap[at + i * {w}..at + (i + 1) * {w}]; let value = {inner}; m.set({key_expr}, value); }} m }}")
        }
        other => panic!("unexpected base {other}"),
    }
}

// Generates the Rust expression that deserializes a potentially nullable field
// of type `t` from `row[k..]`, checking for `.is_nan()`, `< 0.0`, or tag `== 0.0`
// before calling `rust_take`. Mirrors `rustTakeField(model, t, k)` in
// the schema generator.
pub fn rust_take_field(model: &Model, t: &TypeSpec, k: &str) -> String {
    if !optional(t) { return rust_take(model, t, k); }
    let inner = rust_take(model, &bare(t), &format!("{k} + 1"));
    if t.nullable.null && t.nullable.undefined {
        format!("match word_u32(row[{k}]) {{ 0 => Nullable::Value({inner}), 1 => Nullable::Null, _ => Nullable::Undefined }}")
    } else {
        format!("if row[{k}] == 0.0 {{ Some({inner}) }} else {{ None }}")
    }
}

// Returns the Rust default initializer expression for a field of type `t` when
// constructing a row from export-only or import-only slices (`rustColdDefault`
//).
pub fn rust_cold_default(model: &Model, t: &TypeSpec) -> String {
    if optional(t) { return if t.nullable.null && t.nullable.undefined { "Nullable::Undefined".to_string() } else { "None".to_string() }; }
    // Default Rust field initializers for cold/unmirrored fields when constructing
    // a struct from partial (`export`-only or `import`-only) rows.
    match t.base.as_str() {
        "f64" | "f32" => "0.0".to_string(),
        "i32" | "u32" | "u8" | "index" => "0".to_string(),
        "bool" => "false".to_string(),
        "str" => "StrId(0)".to_string(),
        "def" => "DefId(0)".to_string(),
        "ref" => "Ref::new(0, 0)".to_string(),
        "vec3" => "[0.0; 3]".to_string(),
        "quat" => "[0.0; 4]".to_string(),
        "mat4" => "[0.0; 16]".to_string(),
        "f64xN" => format!("[0.0; {}]", t.n),
        "typed" => {
            let zero = if matches!(t.elem.as_deref(), Some("f64") | Some("f32")) { "0.0" } else { "0" };
            format!("TypedArray([{zero}; {}])", t.n)
        }
        "list" => "Vec::new()".to_string(),
        "set" => "RefSet { items: Vec::new() }".to_string(),
        "map" => {
            if t.key.is_none() {
                "StrMap { entries: Vec::new() }".to_string()
            } else {
                "crate::store::OrdMap(crate::store::JsMap::new())".to_string()
            }
        }
        "obj" => "IntObj::new()".to_string(),
        "enum" => {
            let e_name = t.of.as_ref().unwrap();
            let first = &model.enums.get(e_name).unwrap().variants[0];
            format!("{e_name}::{first}")
        }
        "rec" => {
            let r_name = t.of.as_ref().unwrap();
            let rec = model.records.get(r_name).unwrap();
            let fields: Vec<String> = rec
                .fields
                .iter()
                .map(|f| format!("{}: {}", snake(&f.name), rust_cold_default(model, &f.ty)))
                .collect();
            format!("{r_name} {{ {} }}", fields.join(", "))
        }
        "union" => {
            let u_name = t.of.as_ref().unwrap();
            let v = &model.unions.get(u_name).unwrap().variants[0];
            let payload = match v.tag.as_str() {
                "ref" => "(Ref::new(0, 0))".to_string(),
                "value" => format!("({})", rust_cold_default(model, v.ty.as_ref().unwrap())),
                _ => String::new(),
            };
            format!("{u_name}::{}{payload}", v.rust)
        }
        other => panic!("no cold default for {other}"),
    }
}

// Emits a `#[derive(Clone, Debug, PartialEq)] pub struct <Name>` definition
// with `Default` implementation when all fields have sensible defaults.
// Mirrors `rustStruct(model, name, fields, doc)`.
// Struct definition and `Default` impl emitter (`rustStruct`):
//   - Emits `#[derive(Clone, Debug, PartialEq)] pub struct <Name>` with public
//     fields in schema declaration order.
//   - Generates an explicit `impl Default for <Name>` using `rust_cold_default`
//     because fixed arrays `[f64; 16]` (`Mat4`) and custom handles/enums require
//     schema-specific zero/identity defaults (`Mat4` defaults to identity or zero
//     depending on field semantics).
pub fn rust_struct(model: &Model, name: &str, fields: &[Field], doc: Option<&str>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(d) = doc { lines.push(format!("/// {d}")); }
    lines.push("#[derive(Clone, Debug, Default, PartialEq)]".to_string());
    lines.push(format!("pub struct {name} {{"));
    for f in fields { lines.push(format!("    pub {}: {},", snake(&f.name), rust_type(model, &f.ty, false))); }
    lines.push("}".to_string());
    lines.push(String::new());
    lines
}

// Emits paired `put_<suffix>` and `take_<suffix>` row serialization functions
// for a record or kind tier (`rustRows()`).
// Row pack/unpack method pair emitter (`rustRows`):
//   - Emits `pub fn <put_name>(&self, row: &mut [f64], heap: &mut Vec<f64>)` and
//     `pub fn <take_name>(row: &[f64], heap: &[f64]) -> Self`.
//   - Automatically prefixes `row` or `heap` parameter names with `_` when the
//     generated body does not read/write them so `rustc` and `clippy` never warn
//     about unused variables on empty or flat-only records.
fn rust_rows(
    model: &Model,
    placed: &[PlacedField],
    words_count: usize,
    put_name: &str,
    take_name: &str,
    take_self: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    let view = |mutable: bool| -> Option<String> {
        if words_count > 0 {
            let m = if mutable { "mut " } else { "" };
            Some(format!("        let row: &{m}[f64; {words_count}] = (&{m}row[..{words_count}]).try_into().unwrap();"))
        } else {
            None
        }
    };
    lines.push(format!("    pub fn {put_name}(&self, row: &mut [f64], heap: &mut Vec<f64>) {{"));
    if !placed.is_empty() { if let Some(v) = view(true) { lines.push(v); } }
    for PlacedField { field, at } in placed {
        lines.push(format!("        {}", rust_put_field(model, &field.ty, &format!("self.{}", snake(&field.name)), &at.to_string())));
    }
    lines.push("    }".to_string());
    lines.push(String::new());
    if take_self {
        lines.push(format!("    pub fn {take_name}(&mut self, row: &[f64], heap: &[f64]) {{"));
        if !placed.is_empty() { if let Some(v) = view(false) { lines.push(v); } }
        for PlacedField { field, at } in placed {
            lines.push(format!("        self.{} = {};", snake(&field.name), rust_take_field(model, &field.ty, &at.to_string())));
        }
    } else {
        lines.push(format!("    pub fn {take_name}(row: &[f64], heap: &[f64]) -> Self {{"));
        if !placed.is_empty() { if let Some(v) = view(false) { lines.push(v); } }
        lines.push("        Self {".to_string());
        for PlacedField { field, at } in placed {
            lines.push(format!("            {}: {},", snake(&field.name), rust_take_field(model, &field.ty, &at.to_string())));
        }
        lines.push("        }".to_string());
    }
    lines.push("    }".to_string());
    lines.push(String::new());
    lines
}

// Generates an in-place Rust deserialization statement (`take_*_into`) for one
// field at word offset `at`, reusing existing `String` and `Vec` allocations
// wherever possible to avoid per-tick heap churn (`rustReuseField` in
// the schema generator).
// In-place field unpacker (`rustReuseField`):
//   - Rather than allocating a fresh `Vec` on every tick for `list`, `set`, `map`,
//     `obj`, or `typed` fields, calls `.clear()` on the existing vector (`target`)
//     or reuses existing `rec` elements in place when lengths match, eliminating
//     per-frame allocator pressure inside the Wasm simulation loop.
fn rust_reuse_field(model: &Model, t: &TypeSpec, target: &str, at: usize) -> String {
    if !optional(t) && t.base == "rec" && model.records.get(t.of.as_ref().unwrap()).unwrap().reuse {
        return format!("if !{target}.take_into(&row[{at}..], heap) {{ return false; }}");
    }
    if !optional(t) && t.base == "list" && !rust_owns_heap(model, t.value.as_ref().unwrap()) {
        let v = t.value.as_ref().unwrap();
        let w = words(model, v);
        let fill = if !optional(v) && v.base == "f64" {
            format!("{target}.extend_from_slice(&heap[at..at + n]);")
        } else {
            format!("for i in 0..n {{ let row = &heap[at + i * {w}..at + (i + 1) * {w}]; {target}.push({}); }}", rust_take_field(model, v, "0"))
        };
        let div_check = if w > 0 { format!(" || n > (heap.len() - at) / {w}") } else { String::new() };
        return format!(
            "{{ let (at, n) = (word_u32(row[{at}]) as usize, word_u32(row[{at} + 1]) as usize); if n > {target}.capacity() || at > heap.len(){div_check} {{ return false; }} {target}.clear(); {fill} }}"
        );
    }
    format!("{target} = {};", rust_take_field(model, t, &at.to_string()))
}

// Emits the body of `take_imports_into(&mut self, row: &[f64], heap: &[f64])`
// for a kind or record (`rustReuseRows`).
// Generates `pub fn take_<tier>_into(&mut self, row: &[f64], heap: &[f64])`
// for a record or kind tier, updating every placed field in place via
// `rust_reuse_field` so existing `Vec` allocations inside `self` are retained.
fn rust_reuse_rows(model: &Model, placed: &[PlacedField], words_count: usize) -> Vec<String> {
    let mut lines = vec![
        "    /// Decode into reserved list banks without growing or replacing them.".to_string(),
        "    /// False leaves a partial scalar update; all list banks remain owned.".to_string(),
        "    pub fn take_into(&mut self, row: &[f64], heap: &[f64]) -> bool {".to_string(),
        format!("        if row.len() < {words_count} {{ return false; }}"),
        format!("        let row: &[f64; {words_count}] = row[..{words_count}].try_into().unwrap();"),
    ];
    for p in placed { lines.push(format!("        {}", rust_reuse_field(model, &p.field.ty, &format!("self.{}", snake(&p.field.name)), p.at))); }
    lines.push("        true".to_string());
    lines.push("    }".to_string());
    lines.push(String::new());
    lines
}

// Emits the complete Rust definition for a schema record (`struct`, `WORDS`
// constant, `Default` impl, `put`, `take`, and `take_into` methods). Mirrors
// `rustRecord(model, rec)`.
// Record Rust codegen (`rustRecord(model, rec)`):
//   - Places all fields contiguously starting at word offset 0 (`WORDS`).
//   - Emits `pub struct <Name>`, `Default`, `put`, `take`, and `take_into`.
//   - When `rec.publish` is non-empty, also emits `PUBLISH_WORDS` and `put_publish`
//     for the double-buffered publication snapshot used by the presentation layer.
pub fn rust_record(model: &Model, rec: &RecordDef) -> Vec<String> {
    let mut at = 0usize;
    let mut placed = Vec::new();
    for f in &rec.fields { placed.push(PlacedField { field: f.clone(), at, }); at += words(model, &f.ty); }
    // Emit the record's `pub struct`, followed by `impl <Name>` containing
    // `WORDS`, `put(&self, row, heap)`, `take(row, heap)`, and `take_into(&mut self, row, heap)`.
    let mut lines = rust_struct(model, &rec.name, &rec.fields, None);
    lines.push(format!("impl {} {{", rec.name));
    lines.push(format!("    pub const WORDS: usize = {at};"));
    if rec.layout {
        for p in &placed { lines.push(format!("    pub const {}: usize = {};", upper(&p.field.name), p.at)); }
        for p in &placed {
            if p.field.ty.base == "list" {
                let w = words(model, p.field.ty.value.as_ref().unwrap());
                lines.push(format!(
                    "    pub fn {}_rows<'a>(row: &[f64], heap: &'a [f64]) -> std::slice::ChunksExact<'a, f64> {{ let start = row[{}] as usize; let length = row[{}] as usize; heap[start..start + length * {w}].chunks_exact({w}) }}",
                    snake(&p.field.name),
                    p.at,
                    p.at + 1
                ));
            }
        }
    }
    lines.push(String::new());
    for p in &placed {
        if rec.parts.contains(&p.field.name) {
            lines.push("    /// Update one fixed field in a retained encoding; other words stay consumed.".to_string());
            lines.push(format!("    pub fn put_{}(&self, row: &mut [f64], heap: &mut Vec<f64>) {{", snake(&p.field.name)));
            lines.push(format!("        {}", rust_put_field(model, &p.field.ty, &format!("self.{}", snake(&p.field.name)), &p.at.to_string())));
            lines.push("    }".to_string());
            lines.push(String::new());
        }
    }
    lines.extend(rust_rows(model, &placed, at, "put", "take", false));
    if rec.reuse { lines.extend(rust_reuse_rows(model, &placed, at)); }
    lines.push("}".to_string());
    lines.push(String::new());
    lines
}

// Emits a `#[repr(i32)]` Rust `enum` with `Default` (variant 0), `from_word`,
// `to_word`, and `as_str` helpers. Mirrors `rustEnum(e)`.
// Enum Rust codegen details (`rustEnum(e)`):
//   - Emits `#[repr(u32)] #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]`.
//   - Variant 0 is marked `#[default]` so `Default::default()` requires no manual impl.
//   - Emits `NAMES: [&str; N]`, `from_word(w: f64) -> Self` (clamping/indexing via
//     `word_u32(w)`), and `from_str(s: &str) -> Option<Self>`.
pub fn rust_enum(e: &EnumDef) -> Vec<String> {
    let mut lines = vec![
        "#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]".to_string(),
        "#[repr(u8)]".to_string(),
        format!("pub enum {} {{", e.name),
    ];
    for (i, var) in e.variants.iter().enumerate() {
        let def = if i == 0 { "#[default] " } else { "" };
        lines.push(format!("    {def}{var} = {i},"));
    }
    lines.push("}".to_string());
    lines.push(String::new());
    lines.push(format!("impl {} {{", e.name));
    let quoted: Vec<String> = e.values.iter().map(|v| quote_str(v)).collect();
    lines.push(format!("    pub const NAMES: [&'static str; {}] = [{}];", e.values.len(), quoted.join(", ")));
    lines.push(String::new());
    lines.push("    pub fn from_u8(v: u8) -> Option<Self> {".to_string());
    lines.push("        match v {".to_string());
    for (i, var) in e.variants.iter().enumerate() { lines.push(format!("            {i} => Some({}::{var}),", e.name)); }
    lines.push("            _ => None,".to_string());
    lines.push("        }".to_string());
    lines.push("    }".to_string());
    lines.push("}".to_string());
    lines.push(String::new());
    lines
}

// Emits a Rust tagged `enum` for union `u` along with `WORDS`, `Default`
// (first variant), `put`, and `take` methods. Mirrors `rustUnion(model, u)`
//.
// Union Rust codegen details (`rustUnion(model, u)`):
//   - Wire format: word 0 holds the 0-based variant tag (`0.0 .. (N-1) as f64`),
//     and words `1..WORDS` hold the variant payload padded to the widest variant.
//   - When `u.key_union` is true (a union of entity references used as a map key),
//     also derives `Eq, Hash, PartialOrd, Ord` so it can key Rust lookup tables.
//   - Emits `WORDS`, `Default` (initializing to variant 0's default value),
//     `put(&self, row: &mut [f64], heap: &mut Vec<f64>)`, and
//     `take(row: &[f64], heap: &[f64]) -> Self`.
pub fn rust_union(model: &Model, u: &UnionDef) -> Vec<String> {
    let copy = u
        .variants
        .iter()
        .all(|v| v.tag != "value" || v.ty.as_ref().unwrap().base != "rec");
    let payload = |v: &UnionVariant| -> String {
        if v.tag == "ref" {
            format!("Ref<{}>", model.kinds.get(v.kind.as_ref().unwrap()).unwrap().rust)
        } else {
            rust_type(model, v.ty.as_ref().unwrap(), false)
        }
    };
    let mut lines = vec![
        format!("#[derive(Clone, {}Debug, PartialEq)]", if copy { "Copy, " } else { "" }),
        format!("pub enum {} {{", u.name),
    ];
    for v in &u.variants {
        if matches!(v.tag.as_str(), "ref" | "value") {
            lines.push(format!("    {}({}),", v.rust, payload(v)));
        } else {
            lines.push(format!("    {},", v.rust));
        }
    }
    let first = &u.variants[0];
    let first_init = match first.tag.as_str() { "ref" => "(Ref::new(0, 0))", "value" => "(Default::default())", _ => "", };
    let uw = union_words(model, u);
    lines.extend([
        "}".to_string(),
        String::new(),
        format!("impl Default for {} {{", u.name),
        format!("    fn default() -> Self {{ {}::{}{first_init} }}", u.name, first.rust),
        "}".to_string(),
        String::new(),
        format!("impl {} {{", u.name),
        format!("    pub const WORDS: usize = {uw};"),
        String::new(),
        "    /// Its row words: the variant, then its value (a reference's slot and generation).".to_string(),
        "    pub fn put(&self, row: &mut [f64], heap: &mut Vec<f64>) {".to_string(),
        "        match self {".to_string(),
    ]);
    for (i, v) in u.variants.iter().enumerate() {
        let end = 1 + match v.tag.as_str() { "ref" => 2, "value" => words(model, v.ty.as_ref().unwrap()), _ => 0, };
        let padding = if end < uw { format!("row[{end}..{uw}].fill(0.0); ") } else { String::new() };
        match v.tag.as_str() {
            "ref" => lines.push(format!(
                "            {}::{}(r) => {{ row[0] = {i}.0; {padding}row[1] = r.slot as f64; row[2] = r.gen as f64; }}",
                u.name, v.rust
            )),
            "value" => {
                let vt = v.ty.as_ref().unwrap();
                lines.push(format!(
                    "            {}::{}(v) => {{ row[0] = {i}.0; {padding}{} }}",
                    u.name,
                    v.rust,
                    rust_put(model, vt, deref(vt), "1")
                ));
            }
            _ => lines.push(format!("            {}::{} => {{ row[0] = {i}.0; {padding} }},", u.name, v.rust)),
        }
    }
    lines.extend([
        "        }".to_string(),
        "    }".to_string(),
        String::new(),
        "    pub fn take(row: &[f64], heap: &[f64]) -> Self {".to_string(),
        "        match word_u32(row[0]) {".to_string(),
    ]);
    // In `put(&self, row, heap)`, write the 0-based variant index into `row[0]`
    // and serialize the variant payload starting at `row[1..]`.
    for (i, v) in u.variants.iter().enumerate() {
        match v.tag.as_str() {
            "ref" => lines.push(format!("            {i} => {}::{}(Ref::new(word_u32(row[1]), word_u32(row[2]))),", u.name, v.rust)),
            "value" => lines.push(format!("            {i} => {}::{}({}),", u.name, v.rust, rust_take(model, v.ty.as_ref().unwrap(), "1"))),
            _ => lines.push(format!("            {i} => {}::{},", u.name, v.rust)),
        }
    }
    lines.extend([
        format!("            _ => {}::default(),", u.name),
        "        }".to_string(),
        "    }".to_string(),
        "}".to_string(),
        String::new(),
    ]);
    if model.key_unions.contains(&u.name) {
        lines.extend([
            "/// A Map's key: the same object (its kind and its slot and generation).".to_string(),
            format!("impl jsrt::map::JsKey for {} {{", u.name),
            "    fn svz_eq(&self, other: &Self) -> bool { self == other }".to_string(),
            "    fn svz_hash(&self) -> u64 {".to_string(),
            "        match self {".to_string(),
        ]);
        for (i, v) in u.variants.iter().enumerate() {
            lines.push(format!("            {}::{}(r) => crate::store::key_hash({i}, r.slot, r.gen),", u.name, v.rust));
        }
        lines.extend(["        }".to_string(), "    }".to_string(), "}".to_string(), String::new(),]);
    }
    lines
}

// Emits the Rust `struct` and row layout constants/methods (`EXPORT_WORDS`,
// `IMPORT_WORDS`, `FULL_WORDS`, `put_exports`, `take_exports`, `put_imports`,
// `take_imports`, `take_imports_into`, `put_full`, `take_full`) for store kind
// `k`. Mirrors `rustKind(model, k)`.
// Kind Rust codegen details (`rustKind(model, k)`):
//   - Every stored kind gets three row layouts computed by `layout()`:
//     1. `export` (`EXPORT_WORDS`): fields with `mirror: true` (Rust -> JS).
//     2. `import` (`IMPORT_WORDS`): fields with `import: true` (JS -> Rust).
//     3. `full` (`FULL_WORDS`): all fields (`mirror || import`) for full snapshots.
//   - When `import` or `full` has the exact same field sequence as an earlier tier
//     (`same_layout`), the generated methods delegate directly instead of duplicating
//     the field-by-field serialization body.
//   - Also emits `take_export_into`, `take_import_into`, and `take_full_into` so
//     the store can update existing structs in place while reusing heap `Vec` buffers.
pub fn rust_kind(model: &Model, k: &KindDef) -> Vec<String> {
    let facets_str: Vec<String> = k
        .facets
        .iter()
        .map(|f| format!("{}/{f}.mjs", model.target.schema))
        .collect();
    let doc = format!("A {}{}, from {}.", k.name, if k.singleton { " (one a battle)" } else { "" }, facets_str.join(", "));
    // Emit the kind's `pub struct` (when `k.stored` is true) and its `impl`
    // block with `KIND`, `EXPORT_WORDS`, `IMPORT_WORDS`, `FULL_WORDS`, and row
    // pack/unpack methods for each tier (`export`, `import`, `full`).
    let mut lines = rust_struct(model, &k.rust, &k.fields, Some(&doc));
    let ex = layout(model, k, "export");
    let im = layout(model, k, "import");
    let full = layout(model, k, "full");
    lines.extend([
        format!("impl {} {{", k.rust),
        "    /// Words a row takes: slot, generation, then the fields mirrored (export),".to_string(),
        "    /// read by a Rust phase while JavaScript owns the kind (import), or all (full).".to_string(),
        format!("    pub const EXPORT_STRIDE: usize = {};", ex.stride),
        format!("    pub const IMPORT_STRIDE: usize = {};", im.stride),
        format!("    pub const FULL_STRIDE: usize = {};", full.stride),
        String::new(),
    ]);
    lines.extend(rust_rows(model, &full.fields, full.stride, "put_full", "take_full", true));
    for (which, l) in [("export", &ex), ("import", &im)] {
        if same_layout(l, &full) {
            lines.extend([
                "    #[inline(always)]".to_string(),
                format!("    pub fn put_{which}(&self, row: &mut [f64], heap: &mut Vec<f64>) {{ self.put_full(row, heap) }}"),
                String::new(),
                "    #[inline(always)]".to_string(),
                format!("    pub fn take_{which}(&mut self, row: &[f64], heap: &[f64]) {{ self.take_full(row, heap) }}"),
                String::new(),
            ]);
        } else {
            lines.extend(rust_rows(model, &l.fields, l.stride, &format!("put_{which}"), &format!("take_{which}"), true,));
        }
    }
    lines.push("}".to_string());
    lines.push(String::new());
    lines
}

// Generates the complete `<file>.rs` module for a single schema file, emitting
// constants, enums, records, unions, and kinds declared in `file` in declaration
// order. Mirrors `rustFile(model, file)`.
pub fn rust_file(model: &Model, file: &str) -> String {
    let mut lines = vec![
        format!("// Generated by tools/sim-schema.mjs from {}/{file}.mjs; do not edit.", model.target.schema),
        "#![allow(clippy::all, unused_braces, unused_parens, unused_variables)]".to_string(),
        String::new(),
        "use super::*;".to_string(),
        String::new(),
    ];
    for c in model.constants.values() { if c.file == file { lines.push(format!("pub const {}:u32={};", c.name, c.value)); } }
    for e in model.enums.values() { if e.file == file { lines.extend(rust_enum(e)); } }
    for u in model.unions.values() { if u.file == file { lines.extend(rust_union(model, u)); } }
    for r in model.records.values() { if r.file == file { lines.extend(rust_record(model, r)); } }
    for k in &model.kind_list { if k.file.as_deref() == Some(file) { lines.extend(rust_kind(model, k)); } }
    let joined = lines.join("\n");
    format!("{}\n", joined.trim_end_matches('\n'))
}

// Returns true if any declaration in `model` requires importing the store
// helper types (`Handle`, `Table`, etc.) into `mod.rs`. Mirrors `storeTypes(model)`
//.
fn store_types(model: &Model) -> bool {
    fn has(t: &TypeSpec) -> bool {
        t.base == "obj"
            || t.base == "typed"
            || (t.base == "map" && t.key.is_some())
            || t.value.as_ref().map(|v| has(v)).unwrap_or(false)
    }
    model
        .records
        .values()
        .flat_map(|r| &r.fields)
        .chain(model.kind_list.iter().flat_map(|k| &k.fields))
        .any(|f| has(&f.ty))
}

// Generates `mod.rs` for the Rust schema module (`rust/sim/src/gen/mod.rs` or
// `rust/sim/src/gen_test/mod.rs`), declaring submodules, `LAYOUT_HASH`, kind
// code constants (`KIND_*`), the `Kind` metadata table, the `Store` container
// struct with per-kind slabs/singletons, and command/event/import dispatchers.
// Mirrors `rustIndex(model)`.
// `mod.rs` Rust codegen details (`rustIndex(model)`):
//   - Re-exports all per-file modules (`pub mod <file>; pub use <file>::*;`).
//   - Emits `LAYOUT_HASH` (`0x{:08x}`), `KIND_*`, `COMMAND_*`, `EVENT_*`, `IMPORT_*`,
//     and `OP_*` constants in sorted name order.
//   - Emits the `Store` struct containing:
//     * `strings: StringTable` for interned `StrId` values
//     * `events: Vec<f64>` and `events_heap: Vec<f64>` for outgoing events
//     * one singleton struct or `Table<T>` slab per stored kind
//   - Emits `Store::export_words`, `Store::put_export`, `Store::take_import`,
//     `Store::put_full`, and `Store::take_full` indexed by numeric `KIND_*` code.
pub fn rust_index(model: &Model) -> String {
    let kinds = &model.kind_list;
    let held: Vec<&KindDef> = kinds.iter().filter(|k| k.stored).collect();
    // Emit `mod.rs` header, `LAYOUT_HASH`, `KIND_COUNT`, `KIND_*` constants,
    // the `Kind` enum, the `Store` struct holding per-kind `Arena<T>` slabs and
    // singleton instances, and the wire sync/import/command/event dispatchers.
    let mut lines = vec![format!("// Generated by tools/sim-schema.mjs from {}/; do not edit.", model.target.schema)];
    if model.target.test {
        lines.push("//! The generator's test schemas' code (the crate's tests only).".to_string());
    } else {
        lines.push("//! The store's generated code: a module a schema file, and here the kinds'".to_string());
        lines.push("//! codes, the store that holds every kind with fields, and its sync".to_string());
        lines.push("//! dispatch (docs/sim-store.md).".to_string());
    }
    lines.push("#![allow(clippy::all, unused_variables, unused_imports, dead_code)]".to_string());
    lines.push(String::new());
    lines.push("pub use crate::store::block::Block;".to_string());
    lines.push("pub use crate::store::{word_i32, word_u32, DefId, KindCode, Mat4, Nullable, Quat, Ref, RefSet, Slab, StrId, StrMap, Vec3};".to_string());
    if store_types(model) { lines.push("pub use crate::store::{IntObj, NumMap, RefMap, TypedArray};".to_string()); }
    lines.push(String::new());
    for f in &model.files { let sn = snake(f); lines.push(format!("mod {sn};")); lines.push(format!("pub use self::{sn}::*;")); }
    lines.extend([
        "#[cfg(feature = \"replay\")]".to_string(),
        "mod replay;".to_string(),
        "#[cfg(feature = \"replay\")]".to_string(),
        "pub use self::replay::Replay;".to_string(),
        String::new(),
    ]);
    for k in kinds { lines.push(format!("pub const {}: KindCode = {};", kind_const(&k.name), k.code)); }
    lines.push("/// How many kinds there are (codes run 1 to KINDS).".to_string());
    lines.push(format!("pub const KINDS: usize = {};", kinds.len()));
    let kind_names: Vec<String> = kinds.iter().map(|k| quote_str(&k.name)).collect();
    lines.push(format!("pub const KIND_NAMES: [&str; {}] = [\"\", {}];", kinds.len() + 1, kind_names.join(", ")));
    lines.push("/// Each kind's layout hash (its fields, types, flags and order), by code: the".to_string());
    lines.push("/// generated JavaScript has the same, and the bridge checks them.".to_string());
    let kind_hashes: Vec<String> = kinds.iter().map(|k| format!("0x{:08x}", k.hash)).collect();
    lines.push(format!("pub const KIND_HASHES: [u32; {}] = [0, {}];", kinds.len() + 1, kind_hashes.join(", ")));
    lines.push("/// Whether each kind's objects carry their own number (its slot), by code.".to_string());
    let kind_keyed: Vec<String> = kinds.iter().map(|k| k.key.is_some().to_string()).collect();
    lines.push(format!("pub const KIND_KEYED: [bool; {}] = [false, {}];", kinds.len() + 1, kind_keyed.join(", ")));
    lines.push(String::new());
    // Unstored non-singleton kinds (`person`, external host objects) get a distinct
    // zero-sized marker struct so `Ref<Kind>` handles remain strongly typed in Rust.
    for k in kinds.iter().filter(|x| !x.stored && !x.singleton) {
        lines.extend([
            "/// A kind the store holds no fields of (only references to).".to_string(),
            "#[derive(Clone, Copy, Debug, Default)]".to_string(),
            format!("pub struct {};", k.rust),
            String::new(),
        ]);
    }
    lines.extend([
        "/// Every kind with fields: a singleton by value, the rest by slot.".to_string(),
        "#[derive(Default)]".to_string(),
        "pub struct Store {".to_string(),
    ]);
    for k in &held {
        let ty = if k.singleton { k.rust.clone() } else { format!("Slab<{}>", k.rust) };
        lines.push(format!("    pub {}: {ty},", snake(&k.name)));
    }
    lines.push("}".to_string());
    lines.push(String::new());

    let order = |k: &KindDef| -> String {
        if let (Some(hk), Some(hf)) = (&k.home_kind_name, &k.home_field_name) {
            format!("self.{}.{}.clone()", snake(hk), snake(hf))
        } else {
            format!("self.{}.iter().map(|(r, _)| r).collect::<Vec<_>>()", snake(&k.name))
        }
    };

    lines.extend([
        "impl Store {".to_string(),
        "    /// Empty canonical rows for const instance storage.".to_string(),
        "    pub const fn cold() -> Self {".to_string(),
        "        Self {".to_string(),
    ]);
    for k in &held {
        let init = if k.singleton {
            let fs: Vec<String> = k
                .fields
                .iter()
                .map(|f| format!("{}: {}", snake(&f.name), rust_cold_default(model, &f.ty)))
                .collect();
            format!("{} {{ {} }}", k.rust, fs.join(", "))
        } else {
            "Slab::new()".to_string()
        };
        lines.push(format!("            {}: {init},", snake(&k.name)));
    }
    lines.extend([
        "        }".to_string(),
        "    }".to_string(),
        String::new(),
        "    /// A kind's export block: a row for each entity (in its home list's order, or".to_string(),
        "    /// its slots'). `mode` 0: its mirrored fields, after the rows of the entities".to_string(),
        "    /// retired since the last such export (handed over: the slab forgets them);".to_string(),
        "    /// 1: every field, live entities only; 2: as 0, leaving the retired where they".to_string(),
        "    /// are. False for a kind the store holds no fields of.".to_string(),
        "    pub fn export(&mut self, kind: KindCode, block: &mut Block, mode: u32) -> bool {".to_string(),
        "        let full = mode == 1;".to_string(),
        "        match kind {".to_string(),
    ]);
    // Emit one field on `Store` per stored kind: singletons are stored inline
    // as `pub <name>: <Pascal>`, while slotted entity kinds are stored in a
    // generational `Table<<Pascal>>`.
    for k in &held {
        if k.singleton {
            lines.extend([
                format!("            {} => {{", kind_const(&k.name)),
                format!("                block.begin(1, if full {{ {}::FULL_STRIDE }} else {{ {}::EXPORT_STRIDE }});", k.rust, k.rust),
                "                let (row, heap) = block.row(0);".to_string(),
                format!("                if full {{ self.{}.put_full(row, heap) }} else {{ self.{}.put_export(row, heap) }}", snake(&k.name), snake(&k.name)),
                "            }".to_string(),
            ]);
        } else {
            let slab = format!("self.{}", snake(&k.name));
            lines.extend([
                format!("            {} => {{", kind_const(&k.name)),
                format!("                let list = {};", order(k)),
                format!("                let retired = if mode == 0 {{ std::mem::take(&mut {slab}.retired) }} else {{ Vec::new() }};"),
                format!("                let shown = if mode == 2 {{ {slab}.retired.len() }} else {{ retired.len() }};"),
                format!("                block.begin(shown + list.len(), if full {{ {}::FULL_STRIDE }} else {{ {}::EXPORT_STRIDE }});", k.rust, k.rust),
                "                block.set_retired(shown);".to_string(),
                format!("                let old = if mode == 2 {{ &{slab}.retired }} else {{ &retired }};"),
                "                for (i, (r, v)) in old.iter().enumerate() {".to_string(),
                "                    let (row, heap) = block.row(i);".to_string(),
                "                    row[0] = r.slot as f64;".to_string(),
                "                    row[1] = r.gen as f64;".to_string(),
                "                    v.put_export(row, heap);".to_string(),
                "                }".to_string(),
                "                for (i, &r) in list.iter().enumerate() {".to_string(),
                "                    let (row, heap) = block.row(shown + i);".to_string(),
                "                    row[0] = r.slot as f64;".to_string(),
                "                    row[1] = r.gen as f64;".to_string(),
                format!("                    if let Some(v) = {slab}.get(r) {{ if full {{ v.put_full(row, heap) }} else {{ v.put_export(row, heap) }} }}"),
                "                }".to_string(),
                "            }".to_string(),
            ]);
        }
    }
    lines.extend([
        "            _ => return false,".to_string(),
        "        }".to_string(),
        "        block.finish();".to_string(),
        "        true".to_string(),
        "    }".to_string(),
        String::new(),
        "    /// `count` new entities of a kind (its fields' defaults) at the end of its home".to_string(),
        "    /// list, if it has one (tests and benchmarks).".to_string(),
        "    pub fn fill(&mut self, kind: KindCode, count: usize) {".to_string(),
        "        match kind {".to_string(),
    ]);
    for k in &held {
        if k.singleton || k.key.is_some() { continue; }
        lines.push(format!("            {} => for _ in 0..count {{", kind_const(&k.name)));
        lines.push(format!("                let r = self.{}.insert(Default::default());", snake(&k.name)));
        if let (Some(hk), Some(hf)) = (&k.home_kind_name, &k.home_field_name) {
            lines.push(format!("                self.{}.{}.push(r);", snake(hk), snake(hf)));
        }
        lines.push("            },".to_string());
    }
    lines.extend([
        "            _ => {}".to_string(),
        "        }".to_string(),
        "    }".to_string(),
        String::new(),
        "    /// Retires an entity of a kind the store holds by slot (`Slab::retire`).".to_string(),
        "    pub fn retire(&mut self, kind: KindCode, slot: u32, gen: u32) -> bool {".to_string(),
        "        match kind {".to_string(),
    ]);
    for k in &held {
        if !k.singleton { lines.push(format!("            {} => self.{}.retire(Ref::new(slot, gen)),", kind_const(&k.name), snake(&k.name))); }
    }
    lines.extend([
        "            _ => false,".to_string(),
        "        }".to_string(),
        "    }".to_string(),
        String::new(),
        "    /// Takes in an import block JavaScript wrote, its import fields or all of them".to_string(),
        "    /// (`full`): each row's entity (made if its slot holds none of that generation)".to_string(),
        "    /// gets the row's fields, and what the block no longer lists is removed, unless".to_string(),
        "    /// the block holds `some` of the kind (then every entity it does not list is left".to_string(),
        "    /// as it is).".to_string(),
        "    pub fn import(&mut self, kind: KindCode, block: &Block, full: bool, some: bool) -> bool {".to_string(),
        "        match kind {".to_string(),
    ]);
    for k in &held {
        if k.singleton {
            lines.extend([
                format!("            {} => {{", kind_const(&k.name)),
                "                let (row, heap) = block.read(0);".to_string(),
                format!("                if full {{ self.{}.take_full(row, heap) }} else {{ self.{}.take_import(row, heap) }}", snake(&k.name), snake(&k.name)),
                "            }".to_string(),
            ]);
        } else {
            let sn = snake(&k.name);
            lines.extend([
                format!("            {} => {{", kind_const(&k.name)),
                "                let mut listed: Vec<bool> = Vec::new();".to_string(),
                "                for i in 0..block.rows() {".to_string(),
                "                    let (row, heap) = block.read(i);".to_string(),
                "                    let r = Ref::new(word_u32(row[0]), word_u32(row[1]));".to_string(),
                format!("                    let v = self.{sn}.ensure(r);"),
                "                    if full { v.take_full(row, heap) } else { v.take_import(row, heap) }".to_string(),
                "                    let s = r.slot as usize;".to_string(),
                "                    if listed.len() <= s { listed.resize(s + 1, false); }".to_string(),
                "                    listed[s] = true;".to_string(),
                "                }".to_string(),
                format!("                if !some {{ self.{sn}.retain(|r, _| listed.get(r.slot as usize).copied().unwrap_or(false)); }}"),
                "            }".to_string(),
            ]);
        }
    }
    lines.extend([
        "            _ => return false,".to_string(),
        "        }".to_string(),
        "        true".to_string(),
        "    }".to_string(),
        "}".to_string(),
    ]);
    lines.extend(rust_calls(model));
    format!("{}\n", lines.join("\n"))
}

// Generates the command, event, and import code constants (`CMD_*`, `EVT_*`,
// `IMP_*`), the `Commands` / `Events` / `Imports` traits, and the wire
// dispatch functions appended to `mod.rs`. Mirrors `rustCalls(model)` in
// the schema generator.
// Command, event, and import wire glue (`rustCalls(model)`):
//   - For commands: emits `Store::dispatch_command` (decoding args from `row`/`heap`
//     and encoding any result record into `out`/`out_heap`) plus per-command C-ABI
//     or trait veneers when `c.rust` names a handler path.
//   - For operations (`operations_of`): emits unified `OP_*` dispatch tables used
//     by the Wasm boundary (`bfe_sim_op`).
//   - For events: emits `Store::emit_<event>(&mut self, e: &<Record>)` which pushes
//     `[EVT_<NAME>, words...]` onto the store's outgoing event buffer.
//   - For imports: emits the `Imports` trait and `call_<import>` helpers that pack
//     arguments into the import request buffer and decode the host's response.
fn rust_calls(model: &Model) -> Vec<String> {
    let mut lines = vec![String::new()];
    // Helper to emit `pub const <PREFIX>_<NAME>: u32 = <code>;` blocks for
    // commands (`CMD_`), events (`EVT_`), and imports (`IMP_`).
    let mut consts = |prefix: &str, list: &[(String, usize)]| {
        for (name, code) in list { lines.push(format!("pub const {prefix}_{}: u32 = {code};", upper(name))); }
        let quoted: Vec<String> = list.iter().map(|(n, _)| quote_str(n)).collect();
        lines.push(format!("pub const {prefix}_NAMES: [&str; {}] = [\"\", {}];", list.len() + 1, quoted.join(", ")));
        lines.push(String::new());
    };
    consts("COMMAND", &model .commands_list .iter() .map(|c| (c.name.clone(), c.code)) .collect::<Vec<_>>(),);
    consts("EVENT", &model .events_list .iter() .map(|e| (e.name.clone(), e.code)) .collect::<Vec<_>>(),);
    consts("IMPORT", &model .imports_list .iter() .map(|i| (i.name.clone(), i.code)) .collect::<Vec<_>>(),);

    lines.push("/// Runs the command `code` with the arguments JavaScript wrote in `sim.calls`,".to_string());
    lines.push("/// its result written there after it; false for no such command.".to_string());
    lines.push("pub fn run_command(sim: &mut crate::abi::Sim, code: u32) -> bool {".to_string());
    if !model.target.test { lines.push("    if !crate::abi::command_available(code) { return false; }".to_string()); }
    lines.push("    match code {".to_string());
    for c in &model.commands_list {
        if c.detached { lines.push(format!("        COMMAND_{} => return false,", upper(&c.name))); continue; }
        lines.push(format!("        COMMAND_{} => {{", upper(&c.name)));
        lines.push(format!("            let args = {{ let (row, heap) = sim.calls.args(); {}::take(row, heap) }};", c.args));
        if let Some(res) = &c.result {
            lines.push(format!("            let result = crate::{}(sim, args);", c.rust));
            lines.push(format!("            sim.calls.result({res}::WORDS, |row, heap| result.put(row, heap));"));
        } else {
            lines.push(format!("            crate::{}(sim, args);", c.rust));
            lines.push("            sim.calls.result(0, |_, _| {});".to_string());
        }
        lines.push("        }".to_string());
    }
    lines.extend(["        _ => return false,".to_string(), "    }".to_string(), "    true".to_string(), "}".to_string(), String::new(),]);

    let mut veneers = Vec::new();
    if !model.target.test {
        for c in &model.commands_list {
            if !flat(model, &c.args) { continue; }
            let n = record_words(model, &c.args);
            let name = format!("cmd_{}", snake_bare(&c.name));
            let values: Vec<String> = (0..n).map(|i| format!("w{i}")).collect();
            let params: String = values.iter().map(|v| format!("{v}: f64")).collect::<Vec<_>>().join(", ");
            let run_param = if c.detached {
                format!("run: impl FnOnce(&mut crate::world::owner::World, {}) -> {}, ", c.args, c.result.as_deref().unwrap_or("()"))
            } else {
                String::new()
            };
            lines.push(format!("/// The command {}, decoded from its scalar boundary words.", c.name));
            lines.push(format!(
                "pub fn {name}_in(w: &mut crate::world::owner::World, {run_param}{params}) -> Option<&crate::store::block::Block> {{"
            ));
            lines.push(format!("    if !crate::abi::command_available(COMMAND_{}) {{ return None; }}", upper(&c.name)));
            lines.push(format!("    let row: [f64; {n}] = [{}];", values.join(", ")));
            lines.push(format!("    let args = {}::take(&row, &[]);", c.args));
            if c.detached {
                lines.push("    let result = run(w, args);".to_string());
                lines.push("    let sim = &mut w.sim;".to_string());
            } else {
                lines.push("    let sim = &mut w.sim;".to_string());
                let assign = if c.result.is_some() { "let result = " } else { "" };
                lines.push(format!("    {assign}crate::{}(sim, args);", c.rust));
            }
            if let Some(res) = &c.result {
                lines.push(format!("    sim.calls.result({res}::WORDS, |row, heap| result.put(row, heap));"));
                lines.push("    Some(&sim.calls.block)".to_string());
            } else {
                lines.push("    None".to_string());
            }
            lines.push("}".to_string());
            lines.push(String::new());

            let owned_arg = if c.detached { format!("crate::{}, ", c.owned.as_ref().unwrap()) } else { String::new() };
            veneers.push(format!("    /// The command {}, its arguments' words as parameters.", c.name));
            veneers.push("    #[no_mangle]".to_string());
            veneers.push(format!(
                "    pub extern \"C\" fn {name}({params}) -> *const f64 {{ {name}_in(crate::abi::facade::world(), {owned_arg}{}).map_or(std::ptr::null(), crate::store::block::Block::ptr) }}",
                values.join(", ")
            ));
            if c.name == "relay" {
                let unused_params = values.iter().map(|v| format!("_{v}: f64")).collect::<Vec<_>>().join(", ");
                lines.push("#[cfg(not(target_arch = \"wasm32\"))]".to_string());
                lines.push(format!("pub fn {name}({unused_params}) -> *const f64 {{ std::ptr::null() }}"));
                lines.push(String::new());
            }
        }

        for c in &model.commands_list {
            if !c.detached { continue; }
            let symbol = format!("cmd_{}_block", snake_bare(&c.name));
            lines.extend([
                format!("/// Runs {} from the generated call block without retaining its store loan.", c.name),
                "#[cfg(target_arch = \"wasm32\")]".to_string(),
                "#[no_mangle]".to_string(),
                format!("pub extern \"C\" fn {symbol}() -> *const f64 {{"),
                format!("    if !crate::abi::command_available(COMMAND_{}) {{ return std::ptr::null(); }}", upper(&c.name)),
                "    let w = crate::abi::facade::world();".to_string(),
                format!("    let args = {{ let (row, heap) = w.sim.calls.args(); {}::take(row, heap) }};", c.args),
            ]);
            let owned = c.owned.as_ref().unwrap();
            if let Some(res) = &c.result {
                lines.push(format!("    let result = crate::{owned}(w, args);"));
                lines.push(format!("    w.sim.calls.result({res}::WORDS, |row, heap| result.put(row, heap));"));
                lines.push("    w.sim.calls.block.ptr()".to_string());
            } else {
                lines.push(format!("    crate::{owned}(w, args);"));
                lines.push("    std::ptr::null()".to_string());
            }
            lines.push("}".to_string());
            lines.push(String::new());
        }

        let operations = operations_of(model);
        for o in &operations { lines.push(format!("pub const OPERATION_{}: u32 = {};", upper(&o.name), o.code)); }
        let op_names: Vec<String> = operations.iter().map(|o| quote_str(&o.name)).collect();
        lines.push(format!("pub const OPERATION_NAMES: [&str; {}] = [\"\", {}];", operations.len() + 1, op_names.join(", ")));
        lines.push(String::new());
        lines.extend([
            "/// One operation's shape: `scalars` words, then its record's `record` row words".to_string(),
            "/// and, when `heap`, the record's heap words; `answers` says a block comes back.".to_string(),
            "pub struct Operation { pub name: &'static str, pub scalars: usize, pub record: usize, pub heap: bool, pub answers: bool }".to_string(),
            format!("pub const OPERATIONS: [Operation; {}] = [", operations.len()),
        ]);
        for o in &operations {
            let rec_w = o.record.as_ref().map(|r| record_words(model, r)).unwrap_or(0);
            let ans = o.answers != OpAnswers::None;
            lines.push(format!(
                "    Operation {{ name: {}, scalars: {}, record: {rec_w}, heap: {}, answers: {ans} }},",
                quote_str(&o.name),
                o.scalars.len(),
                o.heap
            ));
        }
        lines.extend([
            "];".to_string(),
            "/// The operation an id names; None for a code outside the table.".to_string(),
            "pub fn operation(op: u32) -> Option<&'static Operation> {".to_string(),
            "    OPERATIONS.get(op.checked_sub(1)? as usize)".to_string(),
            "}".to_string(),
            String::new(),
            "/// Runs operation `op` over its argument row (shaped as `operation` says, checked".to_string(),
            "/// by the caller), answering with the words its export returns a pointer to;".to_string(),
            "/// None where that export answers null or returns nothing.".to_string(),
            "pub fn run_operation<'a>(w: &'a mut crate::world::owner::World, op: u32, args: &[f64]) -> Option<&'a [f64]> {".to_string(),
            "    match op {".to_string(),
        ]);
        for o in &operations {
            if let Some(c) = &o.command {
                let n = record_words(model, &c.args);
                let values: Vec<String> = (0..n).map(|i| format!("args[{i}]")).collect();
                let owned_arg = if c.detached { format!("crate::{}, ", c.owned.as_ref().unwrap()) } else { String::new() };
                lines.push(format!(
                    "        OPERATION_{} => cmd_{}_in(w, {owned_arg}{}).map(|block| &block.words[..]),",
                    upper(&o.name),
                    snake_bare(&c.name),
                    values.join(", ")
                ));
                continue;
            }
            let scalars: String = o
                .scalars
                .iter()
                .enumerate()
                .map(|(i, &ty)| if ty == "f64" { format!("args[{i}]") } else { format!("args[{i}] as {ty}") })
                .collect::<Vec<_>>()
                .join(", ");
            let call = format!("crate::{}(w, {scalars})", o.rust.as_ref().unwrap());
            lines.push(format!("        OPERATION_{} => {{", upper(&o.name)));
            if let Some(rec) = &o.record {
                let sc_len = o.scalars.len();
                lines.push(format!("            let record = &args[{sc_len}..];"));
                lines.push(format!(
                    "            w.sim.calls.result({rec}::WORDS, |row, heap| {{ row.copy_from_slice(&record[..{rec}::WORDS]); heap.extend_from_slice(&record[{rec}::WORDS..]); }});"
                ));
            }
            match o.answers {
                OpAnswers::Scalar => {
                    lines.push(format!("            let value = {call};"));
                    lines.push("            w.sim.calls.result(1, |row, _| row[0] = value as f64);".to_string());
                    lines.push("            Some(&w.sim.calls.block.words[..])".to_string());
                }
                OpAnswers::Block => { lines.push(format!("            {call}")); }
                OpAnswers::None => { lines.push(format!("            {call};")); lines.push("            None".to_string()); }
            }
            lines.push("        }".to_string());
        }
        lines.extend(["        _ => None,".to_string(), "    }".to_string(), "}".to_string(), String::new(),]);
    }

    // Emit strongly typed `Store::emit_<event>(&mut self, ...)` methods that
    // serialize each outgoing event record onto the store's event ring buffer.
    for e in &model.events_list {
        lines.extend([
            format!("impl {} {{", e.record),
            format!("    /// Emits it as the event {} (drained in order at the end of the phase).", e.name),
            "    pub fn emit(&self, events: &mut crate::store::calls::Events) {".to_string(),
            format!("        events.emit(EVENT_{}, {}::WORDS, |row, heap| self.put(row, heap));", upper(&e.name), e.record),
            "    }".to_string(),
            "}".to_string(),
            String::new(),
        ]);
    }

    if !model.imports_list.is_empty() {
        lines.extend([
            "#[cfg(target_arch = \"wasm32\")]".to_string(),
            "#[link(wasm_import_module = \"bfe\")]".to_string(),
            "extern \"C\" {".to_string(),
        ]);
        for c in &model.imports_list { lines.push(format!("    fn bfe_import_{}();", snake_bare(&c.name))); }
        lines.push("}".to_string());
        lines.push(String::new());
        for c in &model.imports_list {
            let fn_name = snake_bare(&c.name);
            let ret = c.result.as_deref().unwrap_or("()");
            lines.extend([
                format!("/// Asks JavaScript (the import {}): its arguments go out in the import block,", c.name),
                "/// its result comes back there (store::calls). The module only (a native build serves".to_string(),
                "/// its boundary from elsewhere: the replay harness, or Rust).".to_string(),
                "#[cfg(target_arch = \"wasm32\")]".to_string(),
                format!("pub fn import_{fn_name}(args: &{}) -> {ret} {{", c.args),
                format!("    crate::store::calls::import_out({}::WORDS, |row, heap| args.put(row, heap));", c.args),
                format!("    unsafe {{ bfe_import_{fn_name}() }};"),
            ]);
            if let Some(res) = &c.result { lines.push(format!("    crate::store::calls::import_in(|row, heap| {res}::take(row, heap))")); }
            lines.push("}".to_string());
            lines.push(String::new());
            lines.extend([
                "#[cfg(not(target_arch = \"wasm32\"))]".to_string(),
                "#[cold]".to_string(),
                format!("pub fn import_{fn_name}(_args: &{}) -> {ret} {{ crate::abi::no_native_host({}) }}", c.args, quote_str(&c.name)),
                String::new(),
            ]);
            if c.borrow && c.result.is_some() {
                lines.extend([
                    "/// Read a variable result into retained caller storage.".to_string(),
                    "#[cfg(target_arch = \"wasm32\")]".to_string(),
                    format!("pub fn import_{fn_name}_read<T>(args: &{}, read: impl FnOnce(&[f64], &[f64]) -> T) -> T {{", c.args),
                    format!("    crate::store::calls::import_out({}::WORDS, |row, heap| args.put(row, heap));", c.args),
                    format!("    unsafe {{ bfe_import_{fn_name}() }};"),
                    "    crate::store::calls::import_in(read)".to_string(),
                    "}".to_string(),
                    String::new(),
                    "#[cfg(not(target_arch = \"wasm32\"))]".to_string(),
                    "#[cold]".to_string(),
                    format!(
                        "pub fn import_{fn_name}_read<T>(_args: &{}, _read: impl FnOnce(&[f64], &[f64]) -> T) -> T {{ crate::abi::no_native_host({}) }}",
                        c.args,
                        quote_str(&c.name)
                    ),
                    String::new(),
                ]);
            }
            if !model.target.test {
                lines.extend([
                    "/// Calls the explicitly owned native host; no context borrow spans dispatch.".to_string(),
                    "#[cfg(all(not(target_arch = \"wasm32\"), panic = \"unwind\"))]".to_string(),
                    format!(
                        "pub fn import_{fn_name}_in(context: &crate::abi::context::Context, args: &{}) -> Result<{ret}, crate::abi::context::Failure> {{",
                        c.args
                    ),
                    format!("    let args = crate::abi::context::Record::new({}::WORDS, |row, heap| args.put(row, heap));", c.args),
                    format!("    let reply = context.call_import(IMPORT_{}, args)?;", upper(&c.name)),
                ]);
                if let Some(res) = &c.result {
                    lines.push(format!("    Ok(reply.read({res}::take))"));
                } else {
                    lines.push("    Ok(())".to_string());
                }
                lines.push("}".to_string());
                lines.push(String::new());
            }
        }
    }
    if !veneers.is_empty() {
        lines.push("#[cfg(target_arch = \"wasm32\")]".to_string());
        lines.push("mod exports {".to_string());
        lines.push("    use super::*;".to_string());
        lines.extend(veneers);
        lines.push("}".to_string());
        lines.push(String::new());
    }
    lines
}
