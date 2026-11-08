// Simulation store schema compiler (part 2): Rust JSON replay harness (`replay.rs`),
// JavaScript mirror/sync/export/delta/observed codegen (`src/sim/gen/` and
// `tests/sim-gen/`), HUD and native client input layout emitters, and top-level
// file verification/writing (`run_sim_schema`).
//
// Direct port of the second half of `tools/sim-schema.mjs`.
//
// Byte-identical JavaScript and Rust replay formatting rules:
//   1. Generated JS files use 2-space indentation, double-quoted string literals
//      via `serde_json`-compatible escaping (`to_json_compact`), and V8 property
//      order for all object literals and switch tables.
//   2. `js_zero(f, base, from, to)` emits direct word assignments (`f[base + i] = 0;`)
//      when `to - from <= 4` and a `for` loop (`for (let z = from; z < to; z++)`)
//      when `to - from > 4`.
//   3. `js_file` imports only the runtime helpers (`f32`, `readStr`, `writeStr`,
//      `sameArray`, `alloc`, etc.) actually referenced in the generated body,
//      emitted in the exact fixed helper table order from `tools/sim-schema.mjs`.
//   4. HUD layout (`rust/bfe-render/src/hud_model/layout.rs` and `src/hud-layout.js`)
//      and native input layout (`rust/bfe-client/src/generated_input.rs` and
//      `src/sim/gen/client-input.js`) derive their word offsets directly from the
//      compiled `hud` and `client` schema models.

use crate::json::{quote_str, JsonValue};
use crate::schema_eval::load_schemas;
use crate::sim_schema::*;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

// ---- Rust: the replay harness's values (the `replay` feature) ----
//
// The replay module (`rust/sim/src/gen/replay.rs`) bridges JSON state snapshots
// produced by JavaScript's `exportState(name, obj)` and Rust's typed `Store`
// tables. Unlike the per-tick binary `f64` wire protocol (which uses raw slot
// indices and `StrId` table offsets), JSON snapshots represent:
//   - strings as UTF-8 JSON strings (interned into `Store::strings` on read)
//   - stored entity references at their home list (`shelf.things`) as full inline
//     JSON objects `{ "$id": <number>, ...fields }` (or without `$id` for
//     identity-tracked kinds)
//   - non-home entity references as `{ "$ref": "<kind>", "$id": <key> }` handles
//   - Three.js `Vector3` as `{ "x": n, "y": n, "z": n }` and `Quaternion` as
//     `{ "x": n, "y": n, "z": n, "w": n }`
//   - `Map` instances as arrays of `[key, value]` pairs preserving insertion order
//   - numeric-keyed plain objects (`obj:<V>`) with keys sorted in ascending numeric order.

// Generates the Rust expression that converts a `serde_json::Value` (`x`) at
// path `p` into a non-null Rust value of bare type `t` for the `replay` feature
// (`replayIn(model, t, x, p)` in `tools/sim-schema.mjs`).
fn replay_in(model: &Model, t: &TypeSpec, x: &str, p: &str) -> String {
    let k = if matches!(t.base.as_str(), "ref" | "set") { model.kinds.get(t.of.as_ref().unwrap()) } else { None };
    let one = |e: &str, path: &str| -> String {
        let kd = k.unwrap();
        if kd.stored { format!("{}::ref_in({e}, cx, {path})?", kd.rust) } else { format!("cx.ref_in({}, {e}, {path})?", kind_const(&kd.name)) }
    };
    // Decode a non-null `Node` (`x`) from the JSON replay value tree into bare
    // Rust type `t`:
    //   - `index` resolves an element's identity inside the sibling list field
    //   - numbers/integers/bools/strings validate via `num`/`bool`/`str` helpers
    //   - `vec3`, `quat`, `mat4`, `f64xN`, `typed`, and `list` decode array/object nodes
    //   - `ref:<kind>` resolves the handle through `cx.<kind>_in(x, p)?`
    //   - `map` / `obj` decode entries preserving insertion or ascending numeric key order.
    match t.base.as_str() {
        "index" => {
            let of = t.of.as_ref().unwrap();
            format!(
                "{{ let id = {x}.node_id(); list(v.get({}), {p})?.iter().position(|item| item.node_id().is_some() && item.node_id() == id).ok_or_else(|| format!(\"{{}}: not one of {of}\", {p}))? as u32 }}",
                quote_str(of)
            )
        }
        "f64" => format!("num({x}, {p})?"),
        "f32" => format!("num_f32({x}, {p})?"),
        "i32" => format!("(int({x}, {p}, i32::MIN as f64, i32::MAX as f64)? as i32)"),
        "u32" => format!("(int({x}, {p}, 0.0, u32::MAX as f64)? as u32)"),
        "u8" => format!("(int({x}, {p}, 0.0, 255.0)? as u8)"),
        "bool" => format!("boolean({x}, {p})?"),
        "vec3" => format!("vec3({x}, {p})?"),
        "quat" => format!("quat({x}, {p})?"),
        "mat4" => format!("mat4({x}, {p})?"),
        "f64xN" => format!("numbers::<{}>({x}, {p})?", t.n),
        "typed" => {
            let cls = quote_str(t.class.as_ref().unwrap());
            let elem = t.elem.as_ref().unwrap();
            let n = t.n;
            format!("{{ let vals = crate::store::replay::typed_values({x}, {p}, {cls}, {n})?; let mut a = TypedArray::<{elem}, {n}>::default(); for i in 0..{n} {{ a.0[i] = vals[i] as {elem}; }} a }}")
        }
        "str" => format!("cx.str_in({x}, {p})?"),
        "enum" => { let of = t.of.as_ref().unwrap(); format!("{of}::from_u8(name_of({x}, &{of}::NAMES, {p})?).unwrap()") }
        "def" => format!("cx.def_in({}, {x}, {p})?", quote_str(t.of.as_ref().unwrap())),
        "ref" => one(x, p),
        "union" | "rec" => format!("{}::from_value({x}, cx, {p})?", t.of.as_ref().unwrap()),
        "list" => {
            let inner = replay_in_field(model, t.value.as_ref().unwrap(), "e", "&at", false);
            format!("{{ let mut out = Vec::new(); for (i, e) in list({x}, {p})?.into_iter().enumerate() {{ let at = format!(\"{{}}'[{{i}}]\", {p}).replace(\"'[\", \"[\"); out.push({inner}); }} out }}")
                .replace("format!(\"{}'[{i}]\", &p).replace(\"'[\", \"[\")", "format!(\"{}[{i}]\", &p)")
                .replace("format!(\"{}'[{i}]\", &at).replace(\"'[\", \"[\")", "format!(\"{}[{i}]\", &at)")
        }
        "set" => {
            let inner = one("e", "&at");
            format!("{{ let mut out = RefSet::default(); for (i, e) in set({x}, {p})?.into_iter().enumerate() {{ let at = format!(\"{{}}[{{i}}]\", {p}); out.items.push({inner}); }} out }}")
        }
        "obj" => {
            let inner = replay_in_field(model, t.value.as_ref().unwrap(), "e", "&at", false);
            format!("{{ let mut out = IntObj::default(); for (key, e) in crate::store::replay::object_entries({x}, {p})? {{ let at = format!(\"{{}}[{{}}]\", {p}, key); let id = crate::store::replay::index_key(key, &at)?; let value = {inner}; out.set(id, value); }} out }}")
        }
        "map" => {
            let v = t.value.as_ref().unwrap();
            let inner = replay_in_field(model, v, "e", "&at", false);
            let Some(key_spec) = &t.key else {
                return format!("{{ let mut out = StrMap::default(); for (key, e) in map({x}, {p})? {{ let at = format!(\"{{}}<{{}}>\", {p}, key.show()); let id = cx.str_in(key, &at)?; let value = {inner}; out.entries.push((id, value)); }} out }}");
            };
            let (make, id_expr) = if key_spec.base == "num" {
                ("NumMap", "num(key, &at)?".to_string())
            } else {
                let k_of = key_spec.of.as_ref().unwrap();
                if let Some(kk) = model.kinds.get(k_of) {
                    let expr = if kk.stored {
                        format!("{}::ref_in(key, cx, &at)?", kk.rust)
                    } else {
                        format!("cx.ref_in({}, key, &at)?", kind_const(&kk.name))
                    };
                    ("RefMap", expr)
                } else {
                    ("RefMap", format!("{k_of}::from_value(key, cx, &at)?"))
                }
            };
            format!("{{ let mut out = {make}::default(); for (key, e) in map({x}, {p})? {{ let at = format!(\"{{}}<{{}}>\", {p}, key.show()); let id = {id_expr}; let value = {inner}; out.set(id, value); }} out }}")
        }
        other => panic!("unexpected base {other}"),
    }
}

// Generates the Rust expression that decodes a potentially nullable JSON field
// `x` into a Rust struct field of type `t` (`replayInField` in `tools/sim-schema.mjs`).
// When `home` is true on a singleton's home list/table, entities are allocated
// and populated in the store via their home collection.
// Nullable and home-list wrapper around `replay_in` (`replayInField` in
// `tools/sim-schema.mjs`):
//   - When `home` is true (`list:<kind>` on a singleton's home field), each
//     array element in JSON is a full entity snapshot `{ $id, ...fields }`,
//     allocated and populated via `cx.<kind>_home(item, a, path)?`.
//   - When `t` is nullable/optional, checks `is_null()` / `is_undefined()` on
//     the JSON node before delegating to `replay_in`.
fn replay_in_field(model: &Model, t: &TypeSpec, x: &str, p: &str, home: bool) -> String {
    let inner = if home {
        let k_rust = &model.kinds.get(t.of.as_ref().unwrap()).unwrap().rust;
        format!("{{ let mut out = Vec::new(); for (i, e) in list({x}, {p})?.into_iter().enumerate() {{ let at = format!(\"{{}}[{{i}}]\", {p}); out.push({k_rust}::value_in(e, cx, &at)?); }} out }}")
    } else {
        replay_in(model, &bare(t), x, p)
    };
    if !optional(t) { return inner; }
    let bad = |what: &str| format!("return Err(format!(\"{{}}: {what}\", {p}))");
    if t.nullable.null && t.nullable.undefined {
        format!("match {x}.value {{ Value::Null => Nullable::Null, Value::Undefined => Nullable::Undefined, _ => Nullable::Value({inner}) }}")
    } else if t.nullable.null {
        format!(
            "match {x}.value {{ Value::Null => None, Value::Undefined => {}, _ => Some({inner}) }}",
            bad("undefined where its schema says null")
        )
    } else {
        format!(
            "match {x}.value {{ Value::Undefined => None, Value::Null => {}, _ => Some({inner}) }}",
            bad("null where its schema says undefined")
        )
    }
}

// Generates the Rust expression that converts a non-null Rust value `s` of bare
// type `t` into a `serde_json::Value` matching the JS `exportState` snapshot
// format (`replayOut(model, t, s)` in `tools/sim-schema.mjs`).
fn replay_out(model: &Model, t: &TypeSpec, s: &str) -> String {
    let k = if matches!(t.base.as_str(), "ref" | "set") { model.kinds.get(t.of.as_ref().unwrap()) } else { None };
    let one = |r: &str| -> String {
        let kd = k.unwrap();
        if kd.stored { format!("{}::ref_out({r}, cx, a)", kd.rust) } else { format!("cx.ref_out({}, {r})", kind_const(&kd.name)) }
    };
    // Encode a non-null Rust value `s` of bare type `t` into a JSON replay
    // `Value` (`to_value`), mirroring JS `export_<kind>` output.
    match t.base.as_str() {
        "index" => format!("item_of(a, &fields, {}, {s} as usize)", quote_str(t.of.as_ref().unwrap())),
        "f64" => format!("Value::Num({s})"),
        "f32" | "i32" | "u32" | "u8" => format!("Value::Num({s} as f64)"),
        "bool" => format!("Value::Bool({s})"),
        "vec3" => format!("a.vec3({s})"),
        "quat" => format!("a.quat({s})"),
        "mat4" => format!("mat4_value(a, {s})"),
        "f64xN" => format!("{{ let items = {s}.iter().map(|&x| Value::Num(x)).collect(); a.array(items) }}"),
        "typed" => format!("a.typed(sim_replay::Typed::{}({s}.0.to_vec()))", t.elem.as_ref().unwrap().to_uppercase()),
        "str" => format!("cx.str_out({s})"),
        "enum" => format!("Value::Str({}::NAMES[{s} as usize].to_string())", t.of.as_ref().unwrap()),
        "def" => format!("cx.def_out({}, {s})", quote_str(t.of.as_ref().unwrap())),
        "ref" => one(s),
        "union" | "rec" => format!("{s}.to_value(cx, a)"),
        "list" => {
            let v = t.value.as_ref().unwrap();
            let item = item_of(v);
            let inner = replay_out_field(model, v, item, false);
            format!("{{ let mut items = Vec::new(); for e in {s}.iter() {{ let x = {inner}; items.push(x); }} a.array(items) }}")
        }
        "set" => {
            let inner = one("r");
            format!("{{ let mut items = Vec::new(); for &r in {s}.items.iter() {{ items.push({inner}); }} a.set(items) }}")
        }
        "obj" => {
            let v = t.value.as_ref().unwrap();
            let value = item_of(v);
            let inner = replay_out_field(model, v, value, false);
            format!("{{ let mut entries: Vec<(String, Value)> = Vec::new(); for (key, e) in {s}.iter() {{ let x = {inner}; entries.push((crate::store::replay::index_text(key), x)); }} a.object(\"\", entries.iter().map(|(k, x)| (k.as_str(), x.clone())).collect()) }}")
        }
        "map" => {
            let v = t.value.as_ref().unwrap();
            let value = item_of(v);
            let inner = replay_out_field(model, v, value, false);
            let Some(key_spec) = &t.key else {
                return format!("{{ let mut entries = Vec::new(); for (key, e) in {s}.entries.iter() {{ let k = cx.str_out(*key); let x = {inner}; entries.push((k, x)); }} a.map(entries) }}");
            };
            let key_expr = if key_spec.base == "num" {
                "Value::Num(key)".to_string()
            } else {
                let k_of = key_spec.of.as_ref().unwrap();
                if let Some(kk) = model.kinds.get(k_of) {
                    if kk.stored { format!("{}::ref_out(key, cx, a)", kk.rust) } else { format!("cx.ref_out({}, key)", kind_const(&kk.name)) }
                } else {
                    "key.to_value(cx, a)".to_string()
                }
            };
            format!("{{ let mut entries = Vec::new(); for (key, e) in {s}.iter() {{ let k = {key_expr}; let x = {inner}; entries.push((k, x)); }} a.map(entries) }}")
        }
        other => panic!("unexpected base {other}"),
    }
}

// Generates the Rust expression that converts a potentially nullable Rust field
// `s` of type `t` into a `serde_json::Value` (`replayOutField` in `tools/sim-schema.mjs`).
// Nullable and home-list wrapper around `replay_out` (`replayOutField` in
// `tools/sim-schema.mjs`):
//   - When `home` is true, serializes each owned entity in the home list in full
//     via `cx.<kind>_home_out(id, a)` (emitting `{ $id, ...fields }`).
//   - When `t` is optional, maps `None` to `Value::Null` or `Value::Undefined`
//     matching the schema's `|null` or `|undefined` declaration.
fn replay_out_field(model: &Model, t: &TypeSpec, s: &str, home: bool) -> String {
    if home {
        let k_rust = &model.kinds.get(t.of.as_ref().unwrap()).unwrap().rust;
        return format!("{{ let mut items = Vec::new(); for &r in {s}.iter() {{ let x = {k_rust}::value_out(r, cx, a); items.push(x); }} a.array(items) }}");
    }
    if !optional(t) { return replay_out(model, t, s); }
    let inner = replay_out(model, &bare(t), deref(t));
    if t.nullable.null && t.nullable.undefined {
        format!("match &{s} {{ Nullable::Value(v) => {inner}, Nullable::Null => Value::Null, Nullable::Undefined => Value::Undefined }}")
    } else {
        let none_val = if t.nullable.null { "Null" } else { "Undefined" };
        format!("match &{s} {{ Some(v) => {inner}, None => Value::{none_val} }}")
    }
}

// Emits `from_value` and `to_value` JSON replay methods on a generated Rust
// record or kind struct (`replayStruct` in `tools/sim-schema.mjs`).
// JSON replay struct codegen (`replayStruct` in `tools/sim-schema.mjs`):
//   - Emits `from_value(v: &Node, cx: &mut Replay, path: &str) -> Result<Self, String>`
//     which validates that `v` is a JSON object and decodes each field using
//     `replay_in_field`.
//   - Emits `to_value(&self, cx: &Replay, a: &Store) -> Value` which constructs a
//     `Value::Obj` with fields in declaration order (prefixed by `"$id"` when
//     serializing a slotted store entity at its home list).
//   - For store kinds (`what: Some(k)`), also emits `<kind>_in` and `<kind>_out`
//     helpers on `Replay` to resolve entity handles by numeric `$id` or object identity.
fn replay_struct(model: &Model, name: &str, fields: &[Field], what: Option<&KindDef>) -> Vec<String> {
    let mut lines = vec![
        format!("impl {name} {{"),
        "    /// Its fields from a value (the export of the JavaScript object).".to_string(),
        "    pub fn from_value(v: V, cx: &mut Replay, path: &str) -> Result<Self> {".to_string(),
        "        object(v, path)?;".to_string(),
        format!("        Ok({name} {{"),
    ];
    for f in fields {
        let get_expr = format!("v.get({})", quote_str(&f.name));
        let in_expr = replay_in_field(model, &f.ty, &get_expr, "&p", model.is_home(f));
        lines.push(format!("            {}: {{ let p = format!(\"{{path}}.{}\"); {in_expr} }},", snake(&f.name), f.name));
    }
    lines.extend([
        "        })".to_string(),
        "    }".to_string(),
        String::new(),
        "    /// Its value, as the export of the JavaScript object it stands for.".to_string(),
        "    pub fn to_value(&self, cx: &mut Replay, a: &mut Arena) -> Value {".to_string(),
        format!("        let mut fields: Vec<(&str, Value)> = Vec::with_capacity({});", fields.len()),
    ]);
    for f in fields {
        let out_expr = replay_out_field(model, &f.ty, &format!("self.{}", snake(&f.name)), model.is_home(f));
        lines.push(format!("        {{ let x = {out_expr}; fields.push(({}, x)); }}", quote_str(&f.name)));
    }
    lines.push("        a.object(\"\", fields)".to_string());
    lines.push("    }".to_string());

    if let Some(k) = what {
        let slab = format!("cx.store.{}", snake(&k.name));
        let make = if k.key.is_some() {
            format!("|cx, key| {{ let slot = key.unwrap_or(0); if let Some((old, _)) = {slab}.at(slot) {{ {slab}.remove(old); }} let r = {slab}.insert_at(slot, 0, {name}::default()); (r.slot, r.gen) }}")
        } else {
            format!("|cx, _| {{ let r = {slab}.insert({name}::default()); (r.slot, r.gen) }}")
        };
        let kc = kind_const(&k.name);
        lines.extend([
            String::new(),
            format!("    /// The {} a handle names (made with its fields' defaults the first time;", k.name),
            "    /// its value fills them in where it lives).".to_string(),
            format!("    pub fn ref_in(v: V, cx: &mut Replay, path: &str) -> Result<Ref<{name}>> {{"),
            format!("        let (slot, gen) = cx.stored_in({kc}, v, path, {make})?;"),
            "        Ok(Ref::new(slot, gen))".to_string(),
            "    }".to_string(),
            String::new(),
            format!("    /// A {}'s value where it lives (its home list): its handle (`$id`) and fields.", k.name),
            format!("    pub fn value_in(v: V, cx: &mut Replay, path: &str) -> Result<Ref<{name}>> {{"),
            "        object(v, path)?;".to_string(),
            format!("        let r = {name}::ref_in(v.get(\"$id\"), cx, &format!(\"{{path}}.$id\"))?;"),
            format!("        let value = {name}::from_value(v, cx, path)?;"),
            format!("        *{slab}.get_mut(r).ok_or_else(|| format!(\"{{path}}: its entity is gone\"))? = value;"),
            "        Ok(r)".to_string(),
            "    }".to_string(),
            String::new(),
            format!("    /// The handle of the {} a reference names.", k.name),
            format!("    pub fn ref_out(r: Ref<{name}>, cx: &mut Replay, a: &mut Arena) -> Value {{ cx.stored_out({kc}, r.slot, r.gen) }}"),
            String::new(),
            format!("    /// A {}'s value where it lives: its handle first, then its fields.", k.name),
            format!("    pub fn value_out(r: Ref<{name}>, cx: &mut Replay, a: &mut Arena) -> Value {{"),
            format!("        let id = cx.stored_out({kc}, r.slot, r.gen);"),
            format!("        let e = {slab}.get(r).cloned().unwrap_or_default();"),
            "        let v = e.to_value(cx, a);".to_string(),
            "        if let Value::Node(n) = v { if let Node::Object { fields, .. } = &mut a.nodes[n as usize] { fields.insert(0, (\"$id\".to_string(), id)); } }".to_string(),
            "        v".to_string(),
            "    }".to_string(),
        ]);
    }
    lines.push("}".to_string());
    lines.push(String::new());
    lines
}

// Emits `from_value` and `to_value` JSON replay methods on a generated Rust
// union enum (`replayUnion(model, u)` in `tools/sim-schema.mjs`).
// JSON replay union codegen (`replayUnion` in `tools/sim-schema.mjs`):
//   - `from_value` inspects the runtime shape of `v: &Node` in deterministic order:
//     1. `Node::Null` / `Node::Undefined` match `"null"` / `"undefined"` variants.
//     2. `Node::Ref(kind, _)` matches `ref:<kind>` handle variants.
//     3. `Node::Obj` checks `$class` or unique field keys to pick among `rec:<R>`
//        variants, or falls back to `ref:<kind>` handle lookup by key field.
//     4. `Node::Vec3` / `Node::Quat` / `Node::Mat4` / `Node::Arr` match vector/tuple variants.
//     5. `Node::Num` / `Node::Bool` / `Node::Str` match primitive, `lit:<s >`, and
//        `enum:<E>` variants.
//   - `to_value` converts each Rust enum variant back into its canonical `Value`.
fn replay_union(model: &Model, u: &UnionDef) -> Vec<String> {
    let values: Vec<&UnionVariant> = u.variants.iter().filter(|x| x.tag == "value").collect();
    let recs: Vec<&UnionVariant> = values
        .iter()
        .copied()
        .filter(|x| x.ty.as_ref().unwrap().base == "rec")
        .collect();
    let tagged = recs.len() > 1;
    let mut lines = vec![
        format!("impl {} {{", u.name),
        "    pub fn from_value(v: V, cx: &mut Replay, path: &str) -> Result<Self> {".to_string(),
        "        match v.value {".to_string(),
    ];
    // Dispatch order in `from_value` mirrors `replayUnion` in `tools/sim-schema.mjs`:
    // Variant decoding order in `replay_union` (`from_value`) and `js_union`
    // (`variantOf<Name>`) is critical for byte-identical output and unambiguous
    // runtime dispatch when a union mixes unit, handle, record, vector, and
    // scalar variants:
    //   1. `null` / `undefined` unit variants
    //   2. Object values with `{"$ref": kind, "$id": id}` (entity handle variants)
    //   3. Object values discriminated by record tag field or class name
    //   4. Primitive `f64`, `bool`, and string/enum variants
    if let Some(nul) = u.variants.iter().find(|x| x.tag == "null") {
        lines.push(format!("            Value::Null => return Ok({}::{}),", u.name, nul.rust));
    }
    if let Some(und) = u.variants.iter().find(|x| x.tag == "undefined") {
        lines.push(format!("            Value::Undefined => return Ok({}::{}),", u.name, und.rust));
    }
    let lits: Vec<&UnionVariant> = u.variants.iter().filter(|x| x.tag == "lit").collect();
    let strs: Vec<&UnionVariant> = values
        .iter()
        .copied()
        .filter(|x| matches!(x.ty.as_ref().unwrap().base.as_str(), "str" | "enum"))
        .collect();
    if !lits.is_empty() || !strs.is_empty() {
        let arms: Vec<String> = lits
            .iter()
            .map(|x| format!("{} => return Ok({}::{}),", quote_str(x.value.as_ref().unwrap()), u.name, x.rust))
            .collect();
        let rest: Vec<String> = strs
            .iter()
            .map(|x| {
                let vt = x.ty.as_ref().unwrap();
                if vt.base == "str" {
                    format!("return Ok({}::{}(cx.str_in(v, path)?));", u.name, x.rust)
                } else {
                    let of = vt.of.as_ref().unwrap();
                    format!("if let Ok(i) = name_of(v, &{of}::NAMES, path) {{ return Ok({}::{}({of}::from_u8(i).unwrap())); }}", u.name, x.rust)
                }
            })
            .collect();
        lines.push(format!("            Value::Str(s) => {{ match s.as_str() {{ {} _ => {{}} }} {} }}", arms.join(" "), rest.join(" ")));
    }
    if let Some(num) = values
        .iter()
        .copied()
        .find(|x| matches!(x.ty.as_ref().unwrap().base.as_str(), "f64" | "f32" | "i32" | "u32" | "u8"))
    {
        lines.push(format!(
            "            Value::Num(_) => return Ok({}::{}({})),",
            u.name,
            num.rust,
            replay_in(model, num.ty.as_ref().unwrap(), "v", "path")
        ));
    }
    if let Some(b) = values.iter().copied().find(|x| x.ty.as_ref().unwrap().base == "bool") {
        lines.push(format!("            Value::Bool(b) => return Ok({}::{}(*b)),", u.name, b.rust));
    }
    let handles: Vec<&UnionVariant> = u.variants.iter().filter(|x| x.tag == "ref").collect();
    if !handles.is_empty() {
        let arms: Vec<String> = handles
            .iter()
            .map(|x| {
                let k_name = x.kind.as_ref().unwrap();
                let kd = model.kinds.get(k_name).unwrap();
                let r_in = if kd.stored {
                    format!("{}::ref_in(v, cx, path)?", kd.rust)
                } else {
                    format!("cx.ref_in({}, v, path)?", kind_const(&kd.name))
                };
                format!("{} => return Ok({}::{}({r_in})),", quote_str(k_name), u.name, x.rust)
            })
            .collect();
        lines.push(format!("            Value::Handle(kind, _) => match kind.as_str() {{ {} _ => {{}} }},", arms.join(" ")));
    }
    let shapes: Vec<&UnionVariant> = values
        .iter()
        .copied()
        .filter(|x| matches!(x.ty.as_ref().unwrap().base.as_str(), "vec3" | "quat" | "mat4" | "f64xN" | "rec"))
        .collect();
    if !shapes.is_empty() {
        let mut arms = Vec::new();
        for x in &values {
            let vt = x.ty.as_ref().unwrap();
            match vt.base.as_str() {
                "vec3" => arms.push(format!("Some(Node::Vec3(_)) => return Ok({}::{}(vec3(v, path)?)),", u.name, x.rust)),
                "quat" => arms.push(format!("Some(Node::Quat(_)) => return Ok({}::{}(quat(v, path)?)),", u.name, x.rust)),
                "mat4" => arms.push(format!("Some(Node::Mat4(_)) => return Ok({}::{}(mat4(v, path)?)),", u.name, x.rust)),
                "f64xN" => arms.push(format!("Some(Node::Array(_)) => return Ok({}::{}(numbers::<{}>(v, path)?)),", u.name, x.rust, vt.n)),
                _ => {}
            }
        }
        if !recs.is_empty() {
            let body = if tagged {
                let rec_arms: Vec<String> = recs
                    .iter()
                    .map(|x| {
                        let of = x.ty.as_ref().unwrap().of.as_ref().unwrap();
                        format!("Some({}) => return Ok({}::{}({of}::from_value(v, cx, path)?)),", quote_str(of), u.name, x.rust)
                    })
                    .collect();
                format!("match v.get(\"$record\").str() {{ {} _ => {{}} }}", rec_arms.join(" "))
            } else {
                let of = recs[0].ty.as_ref().unwrap().of.as_ref().unwrap();
                format!("return Ok({}::{}({of}::from_value(v, cx, path)?));", u.name, recs[0].rust)
            };
            arms.push(format!("Some(Node::Object {{ .. }}) => {{ {body} }}"));
        }
        lines.push(format!("            Value::Node(_) => match v.node() {{ {} _ => {{}} }},", arms.join(" ")));
    }
    lines.extend([
        "            _ => {}".to_string(),
        "        }".to_string(),
        format!("        Err(format!(\"{{path}}: none of {}'s variants ({{}})\", v.show()))", u.name),
        "    }".to_string(),
        String::new(),
        "    pub fn to_value(&self, cx: &mut Replay, a: &mut Arena) -> Value {".to_string(),
        "        match self {".to_string(),
    ]);
    for x in &u.variants {
        match x.tag.as_str() {
            "null" => lines.push(format!("            {}::{} => Value::Null,", u.name, x.rust)),
            "undefined" => lines.push(format!("            {}::{} => Value::Undefined,", u.name, x.rust)),
            "lit" => lines.push(format!(
                "            {}::{} => Value::Str({}.to_string()),",
                u.name,
                x.rust,
                quote_str(x.value.as_ref().unwrap())
            )),
            "ref" => {
                let k = model.kinds.get(x.kind.as_ref().unwrap()).unwrap();
                let expr = if k.stored { format!("{}::ref_out(*r, cx, a)", k.rust) } else { format!("cx.ref_out({}, *r)", kind_const(&k.name)) };
                lines.push(format!("            {}::{}(r) => {expr},", u.name, x.rust));
            }
            _ => {
                let vt = x.ty.as_ref().unwrap();
                if vt.base == "rec" && tagged {
                    let of = vt.of.as_ref().unwrap();
                    lines.push(format!(
                        "            {}::{}(v) => {{ let out = v.to_value(cx, a); if let Value::Node(n) = out {{ if let Node::Object {{ fields, .. }} = &mut a.nodes[n as usize] {{ fields.insert(0, (\"$record\".to_string(), Value::Str({}.to_string()))); }} }} out }}",
                        u.name,
                        x.rust,
                        quote_str(of)
                    ));
                } else {
                    lines.push(format!("            {}::{}(v) => {},", u.name, x.rust, replay_out(model, vt, deref(vt))));
                }
            }
        }
    }
    lines.extend(["        }".to_string(), "    }".to_string(), "}".to_string(), String::new(),]);
    lines
}

// Generates `replay.rs` (`rust/sim/src/gen/replay.rs` or `gen_test/replay.rs`),
// providing `Replay` handle-remapping tables and `Store::read_state` /
// `Store::write_state` for deterministic JSON state snapshots. Mirrors
// `rustReplay(model)` in `tools/sim-schema.mjs`.
// Complete `replay.rs` module generator (`rustReplay(model)` in `tools/sim-schema.mjs`):
//   - Emits the `Replay` context struct holding handle-remapping tables (`HashMap<u32, Id<K>>`
//     and identity maps) for every kind in `model.kind_list`.
//   - Emits `Store::read_state(&mut self, name: &str, v: &Node, cx: &mut Replay)` to
//     populate a singleton and all of its home-list entities from a JSON snapshot.
//   - Emits `Store::write_state(&self, name: &str, cx: &Replay) -> Result<Value, String>`
//     to export a singleton and its owned entities back to a JSON snapshot for
//     round-trip parity tests against JS `exportState`.
pub fn rust_replay(model: &Model) -> String {
    let mut lines = vec![
        format!("// Generated by tools/sim-schema.mjs from {}/; do not edit.", model.target.schema),
        "//! Every kind, record and union read from and written as the replay harness's".to_string(),
        "//! values (the `replay` feature; rust/sim/src/store/replay.rs).".to_string(),
        "#![allow(clippy::all, unused_braces, unused_parens, unused_variables, unused_mut, unused_imports)]".to_string(),
        String::new(),
        "use super::*;".to_string(),
        "use crate::store::replay::{boolean, int, item_of, list, map, mat4, mat4_value, name_of, num, num_f32, numbers, object, quat, set, vec3, Arena, Node, ReplayOf, Result, StoreKinds, Value, V};".to_string(),
        String::new(),
        "/// The replay context of this store.".to_string(),
        "pub type Replay = ReplayOf<Store>;".to_string(),
        String::new(),
        "impl StoreKinds for Store {".to_string(),
        "    const NAMES: &'static [&'static str] = &KIND_NAMES;".to_string(),
        "    const KEYED: &'static [bool] = &KIND_KEYED;".to_string(),
        "}".to_string(),
        String::new(),
    ];
    for u in model.unions.values() { lines.extend(replay_union(model, u)); }
    for r in model.records.values() { lines.extend(replay_struct(model, &r.name, &r.fields, None)); }
    for k in &model.kind_list {
        if k.stored { lines.extend(replay_struct(model, &k.rust, &k.fields, if k.singleton { None } else { Some(k) })); }
    }
    lines.extend([
        "impl Replay {".to_string(),
        "    /// Reads a singleton's state value (`exportState(name, object)`) into the store:".to_string(),
        "    /// the singleton's fields, and every entity its lists hold, made anew.".to_string(),
        "    pub fn read_value(&mut self, kind: KindCode, v: V) -> Result<()> {".to_string(),
        "        match kind {".to_string(),
    ]);
    for k in &model.kind_list {
        if k.singleton {
            lines.push(format!(
                "            {} => {{ let value = {}::from_value(v, self, {})?; self.store.{} = value; }}",
                kind_const(&k.name),
                k.rust,
                quote_str(&k.name),
                snake(&k.name)
            ));
        }
    }
    lines.extend([
        "            _ => return Err(format!(\"{} is no singleton\", KIND_NAMES.get(kind as usize).copied().unwrap_or(\"?\"))),".to_string(),
        "        }".to_string(),
        "        Ok(())".to_string(),
        "    }".to_string(),
        String::new(),
        "    /// A singleton's state as its exporter gives it.".to_string(),
        "    pub fn write_value(&mut self, kind: KindCode, a: &mut Arena) -> Value {".to_string(),
        "        match kind {".to_string(),
    ]);
    for k in &model.kind_list {
        if k.singleton {
            lines.push(format!(
                "            {} => {{ let s = self.store.{}.clone(); s.to_value(self, a) }}",
                kind_const(&k.name),
                snake(&k.name)
            ));
        }
    }
    lines.extend(["            _ => Value::Undefined,".to_string(), "        }".to_string(), "    }".to_string(), "}".to_string(),]);
    format!("{}\n", lines.join("\n"))
}

// ---- JavaScript code generation ----
//
// Each schema module `<file>.mjs` compiles to a corresponding ES module
// `src/sim/gen/<file>.js` (or `tests/sim-gen/<file>.js`) containing:
//   - shared schema constants and frozen `<ENUM>_NAMES` / `<ENUM>_INDEX` tables
//   - for each union `<U>`:
//     * `variantOf<U>(b, x)` to classify a live JS value into its 0-based tag
//     * `sync<U>(b, f, at, h, cur)` to decode a tagged union from `f[at..]`
//     * `put<U>(b, v, f, at, heap)` to encode a live JS value into `f[at..]`
//     * `same<U>(a, b)` and `export<U>(v, cx)` for equality and snapshot export
//   - for each record `<R>`:
//     * `<R>` mirror class, `sync<R>`, optional `syncDelta<R>` / `syncRefresh<R>`,
//       `put<R>`, optional `<R>View` / `publish<R>`, `same<R>`, and `export<R>`
//   - for each stored kind `<K>`:
//     * `<K>Mirror` class, `<K>_KEYS` / `<K>_SKIP` metadata, `sync<K>`, optional
//       `syncDelta<K>` / `syncHot<K>` / `syncCold<K>`, `syncFull<K>`, `put<K>`,
//       `putFull<K>`, `check<K>`, `same<K>`, and `export<K>`.

// Emits the frozen JS array of variant strings for enum `e` (`jsEnum(e)` in
// `tools/sim-schema.mjs`).
// Returns the `<ENUM>_NAMES` identifier for the frozen array of variant strings
// (`Object.freeze(["...", ...])`) emitted for enum `e` (`jsEnum(e)` in
// `tools/sim-schema.mjs`). Used when decoding a numeric enum wire word `f[at + k]`
// back into its canonical string value on the JS mirror (`<ENUM>_NAMES[f[at + k]]`).
fn js_enum(e: &EnumDef) -> String { format!("{}_NAMES", e.name) }

// Emits the frozen JS object mapping each enum variant string to its 0-based
// wire index (`jsEnumIndex(e)` in `tools/sim-schema.mjs`).
// Returns the `<ENUM>_INDEX` identifier for the frozen object mapping each
// variant string to its 0-based numeric code (`Object.freeze({ "v0": 0, ... })`).
// Used by `put*` and `variantOf*` to validate and encode JS strings into wire words.
fn js_enum_index(e: &EnumDef) -> String { format!("{}_INDEX", e.name) }

// Returns the JS expression that reads field `f` from live JS object `o`,
// following `f.from` optional-chaining paths when specified (`jsGet(f, o)` in
// `tools/sim-schema.mjs`).
// Builds the JS property-read expression on source object `o` for field `f`:
//   - Single-segment paths (`f.js.len() == 1`) use direct property access (`o.foo`
//     or `o["foo"]`) because `o` is guaranteed non-null at the top of `put*`.
//   - Multi-segment paths (`js: "boxes.0.size"`) use optional chaining (`?.`) on
//     intermediate segments so missing intermediate sub-objects evaluate to
//     `undefined` instead of throwing a `TypeError` (`jsGet` in `tools/sim-schema.mjs`).
fn js_get(f: &Field, o: &str) -> String {
    if f.js.len() > 1 {
        let rest: String = f.js[1..].iter().map(|s| js_opt_key(s)).collect();
        format!("{o}{}{rest}", js_key(&f.js[0]))
    } else {
        format!("{o}{}", js_path(&f.js))
    }
}

// Returns the JS assignment target for writing field `f` onto mirror object `o`
// (`jsSet(f, o)` in `tools/sim-schema.mjs`).
// Builds the JS property-write target expression on mirror object `o` for field `f`:
//   - Mirror objects always store fields flat under their schema field name (`f.name`),
//     even when `f.js` specifies a nested import path on the live game object.
fn js_set(f: &Field, o: &str) -> String {
    let mut x = o.to_string();
    for i in 0..f.js.len() - 1 {
        let next_is_num = f.js[i + 1].chars().all(|c| c.is_ascii_digit()) && !f.js[i + 1].is_empty();
        let init = if next_is_num { "[]" } else { "{}" };
        x = format!("({x}{} ??= {init})", js_key(&f.js[i]));
    }
    format!("{x}{}", js_key(f.js.last().unwrap()))
}

// Generates the JS statement that reads a non-null value of bare type `t` from
// `f[at + k]` (and heap `h`) into `target`, mutating existing vectors/records/
// collections in place when possible. Mirrors `jsSync(model, t, target, k)` in
// `tools/sim-schema.mjs`.
fn js_sync(model: &Model, t: &TypeSpec, target: &str, k: usize) -> String {
    // Emit the JS statement syncing bare type `t` at word offset `k` from
    // export row `f[at + k]` and heap `h` into mirror property `target`:
    //   - numeric scalars read `f[at + k]`; `i32` / `u32` / `u8` coerce bit-width
    //   - `bool` converts via `f[at + k] !== 0`
    //   - `vec3`, `quat`, `mat4`, `f64xN`, and `typed` mutate the existing
    //     destination object/TypedArray in place when already allocated
    //   - `ref`, `def`, `index`, `list`, `set`, `map`, `obj` delegate to `runtime.js`
    //     bridge helpers (`b.refs`, `readList`, `readMap`, `readObj`, etc.).
    match t.base.as_str() {
        "f64" | "f32" => format!("{target} = f[at + {k}];"),
        "i32" => format!("{target} = f[at + {k}] | 0;"),
        "u32" | "u8" => format!("{target} = f[at + {k}] >>> 0;"),
        "index" => {
            // Strip trailing `.ident` from target
            let prefix = if let Some(dot) = target.rfind('.') { &target[..dot] } else { target };
            format!("{target} = {prefix}.{}[f[at + {k}]];", t.of.as_ref().unwrap())
        }
        "bool" => format!("{target} = f[at + {k}] !== 0;"),
        "vec3" => format!(
            "{{ const $v = ({target} ??= new THREE.Vector3()); $v.x = f[at + {k}]; $v.y = f[at + {}]; $v.z = f[at + {}]; }}",
            k + 1,
            k + 2
        ),
        "quat" => format!(
            "{{ const $q = ({target} ??= new THREE.Quaternion()); $q._x = f[at + {k}]; $q._y = f[at + {}]; $q._z = f[at + {}]; $q._w = f[at + {}]; $q._onChangeCallback(); }}",
            k + 1,
            k + 2,
            k + 3
        ),
        "mat4" => format!("{{ const e = ({target} ??= new THREE.Matrix4()).elements; for (let i = 0; i < 16; i++) e[i] = f[at + {k} + i]; }}"),
        "f64xN" => {
            let assigns: Vec<String> = (0..t.n).map(|i| format!("a[{i}] = f[at + {}];", k + i)).collect();
            format!("{{ const a = ({target} ??= new Array({})); {} }}", t.n, assigns.join(" "))
        }
        "typed" => format!(
            "{{ const $a = ({target} ??= new {}({})); for (let i = 0; i < {}; i++) $a[i] = f[at + {k} + i]; }}",
            t.class.as_ref().unwrap(),
            t.n,
            t.n
        ),
        "str" => format!("{target} = b.strings.at(f[at + {k}]);"),
        "enum" => { let e = model.enums.get(t.of.as_ref().unwrap()).unwrap(); format!("{target} = {}[f[at + {k}]];", js_enum(e)) }
        "def" => format!("{target} = b.defs.{}.at(f[at + {k}]);", t.of.as_ref().unwrap()),
        "ref" => format!("{target} = b.refs[{}].at(f[at + {k}], f[at + {}]);", kind_const(t.of.as_ref().unwrap()), k + 1),
        "union" => format!("{target} = sync{}(b, f, at + {k}, h, {target});", t.of.as_ref().unwrap()),
        "rec" => { let of = t.of.as_ref().unwrap(); format!("{target} = sync{of}(b, f, at + {k}, h, {target} ?? new {of}Mirror());") }
        "list" => {
            let v = t.value.as_ref().unwrap();
            let w = words(model, v);
            let inner = js_sync_field(model, v, "v", 0);
            format!(
                "{{ const items = ({target} ??= []), start = h + f[at + {k}], n = f[at + {}]; while (items.length > n) items.pop(); for (let i = 0; i < n; i++) {{ let v = items[i]; {{ const at = start + i * {w}; {inner} }} items[i] = v; }} }}",
                k + 1
            )
        }
        "set" => format!(
            "{target} = b.set({}, f, h + f[at + {k}], f[at + {}], {target} ?? new Set());",
            kind_const(t.of.as_ref().unwrap()),
            k + 1
        ),
        "map" => {
            let v = t.value.as_ref().unwrap();
            let w = entry_words(model, t);
            let kw = key_words(model, t);
            let Some(key_spec) = &t.key else {
                let inner = js_sync_field(model, v, "v", 1);
                return format!(
                    "{{ const map = ({target} ??= new Map()), start = h + f[at + {k}], n = f[at + {}]; map.clear(); for (let i = 0; i < n; i++) {{ const e = start + i * {w}; let v; {{ const at = e; {inner} }} map.set(b.strings.at(f[e]), v); }} }}",
                    k + 1
                );
            };
            let key_fn = |e: &str| -> String {
                if key_spec.base == "num" {
                    format!("f[{e}]")
                } else {
                    let k_of = key_spec.of.as_ref().unwrap();
                    if model.kinds.has(k_of) {
                        format!("b.refs[{}].at(f[{e}], f[{e} + 1])", kind_const(k_of))
                    } else {
                        format!("sync{k_of}(b, f, {e}, h, undefined)")
                    }
                }
            };
            let k_start_i = key_fn(&format!("start + i * {w}"));
            let k_start_kept = key_fn(&format!("start + kept * {w}"));
            let k_e = key_fn("e");
            let inner = js_sync_field(model, v, "v", kw);
            format!(
                "{{ const map = ({target} ??= new Map()), start = h + f[at + {k}], n = f[at + {}]; for (const key of map.keys()) {{ let i = 0; while (i < n && !Object.is({k_start_i}, key)) i++; if (i === n) map.delete(key); }} let kept = 0; for (const key of map.keys()) {{ if (kept < n && Object.is({k_start_kept}, key)) kept++; else break; }} for (let i = 0; i < n; i++) {{ const e = start + i * {w}, key = {k_e}; let v = map.get(key); {{ const at = e; {inner} }} if (i >= kept) map.delete(key); map.set(key, v); }} }}",
                k + 1
            )
        }
        "obj" => {
            let v = t.value.as_ref().unwrap();
            let w = entry_words(model, t);
            let inner = js_sync_field(model, v, "v", 1);
            format!(
                "{{ const o = ({target} ??= {{}}), start = h + f[at + {k}], n = f[at + {}]; for (const key in o) {{ let i = 0; while (i < n && f[start + i * {w}] !== +key) i++; if (i === n) delete o[key]; }} for (let i = 0; i < n; i++) {{ const e = start + i * {w}, key = f[e]; let v = o[key]; {{ const at = e; {inner} }} o[key] = v; }} }}",
                k + 1
            )
        }
        other => panic!("unexpected base {other}"),
    }
}

// Generates the JS statement that reads a potentially nullable field of type `t`
// at word offset `k` from `f[at + k]` into `target` (`jsSyncField` in
// `tools/sim-schema.mjs`).
// Nullable wrapper around `js_sync` (`jsSyncField` in `tools/sim-schema.mjs`):
//   - When `t.nullable.sentinel == Some("nan")`, checks `Number.isNaN(f[at + k])`
//     on word 0 and assigns `null` or `undefined` when NaN.
//   - When `t.nullable.tag == 1`, reads the leading tag word `f[at + k]`: `0` ->
//     `null`, `2` -> `undefined`, and `1` -> syncs the payload at `k + 1`.
fn js_sync_field(model: &Model, t: &TypeSpec, target: &str, k: usize) -> String {
    if !optional(t) { return js_sync(model, t, target, k); }
    let inner = js_sync(model, &bare(t), target, k + 1);
    format!("{{ const tag = f[at + {k}]; if (tag === 0) {{ {inner} }} else {target} = tag === 1 ? null : undefined; }}")
}

// Generates JS statements to zero words `from..to` in Float64Array `f` starting
// at `base`. Unrolls up to 4 assignments inline and emits a `for` loop for 5 or
// more words (`jsZero(f, base, from, to)` in `tools/sim-schema.mjs`).
fn js_zero(f: &str, base: &str, from: usize, to: usize) -> String {
    if to - from <= 8 {
        (0..(to - from))
            .map(|i| format!("{f}[{base} + {}] = 0;", from + i))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        format!("{f}.fill(0, {base} + {from}, {base} + {to});")
    }
}

// Generates the JS statement that serializes a non-null JS value `v` of bare
// type `t` into `f[at + k]` and side-buffer `heap`. Mirrors `jsPut(model, t, v, k)`
// in `tools/sim-schema.mjs`.
fn js_put(model: &Model, t: &TypeSpec, v: &str, k: usize) -> String {
    // Emit the JS statement packing bare type `v` into `f[at + k]` and `heap`:
    //   - numbers write `f[at + k] = v`; `bool` writes `v ? 1 : 0`
    //   - `vec3` packs `.x, .y, .z`; `quat` packs `.x, .y, .z, .w`
    //   - `str` calls `b.putStr(v)`
    //   - `list`, `set`, `map`, `obj`, `typed` call the corresponding `heap.*` writer.
    match t.base.as_str() {
        "f64" | "f32" | "i32" | "u32" | "u8" => format!("f[at + {k}] = {v};"),
        "index" => {
            let of = t.of.as_ref().unwrap();
            format!("{{ const $i = (o.{of} ?? []).indexOf({v}); if ($i < 0) throw Error(\"{of}: the item is none of them\"); f[at + {k}] = $i; }}")
        }
        "bool" => format!("f[at + {k}] = {v} ? 1 : 0;"),
        "vec3" => format!("{{ const $v = {v}; f[at + {k}] = $v.x; f[at + {}] = $v.y; f[at + {}] = $v.z; }}", k + 1, k + 2),
        "quat" => format!(
            "{{ const $v = {v}; f[at + {k}] = $v.x; f[at + {}] = $v.y; f[at + {}] = $v.z; f[at + {}] = $v.w; }}",
            k + 1,
            k + 2,
            k + 3
        ),
        "mat4" => format!("{{ const e = {v}.elements; for (let i = 0; i < 16; i++) f[at + {k} + i] = e[i]; }}"),
        "f64xN" => {
            let assigns: Vec<String> = (0..t.n).map(|i| format!("f[at + {}] = $v[{i}];", k + i)).collect();
            format!("{{ const $v = {v}; {} }}", assigns.join(" "))
        }
        "typed" => {
            let cls = t.class.as_ref().unwrap();
            let n = t.n;
            format!("{{ const $v = {v}; if ($v.length !== {n}) throw Error(\"a {cls} of \" + $v.length + \", not {n}\"); for (let i = 0; i < {n}; i++) f[at + {k} + i] = $v[i]; }}")
        }
        "str" => format!("f[at + {k}] = b.strings.id({v});"),
        "enum" => { let e = model.enums.get(t.of.as_ref().unwrap()).unwrap(); format!("f[at + {k}] = {}.get({v});", js_enum_index(e)) }
        "def" => format!("f[at + {k}] = b.defs.{}.id({v});", t.of.as_ref().unwrap()),
        "ref" => format!("b.refs[{}].put({v}, f, at + {k});", kind_const(t.of.as_ref().unwrap())),
        "union" | "rec" => format!("put{}(b, {v}, f, at + {k}, heap);", t.of.as_ref().unwrap()),
        "set" => format!("heap.list(b.refs[{}], {v}, f, at + {k});", kind_const(t.of.as_ref().unwrap())),
        "list" => {
            let val_t = t.value.as_ref().unwrap();
            let w = words(model, val_t);
            let inner = js_put_field(model, val_t, "v", 0);
            format!("heap.items({v}, f, at + {k}, {w}, (f, at, v) => {{ {inner} }});")
        }
        "obj" => {
            let val_t = t.value.as_ref().unwrap();
            let ew = entry_words(model, t);
            let inner = js_put_field(model, val_t, "v", 1);
            format!("heap.object({v}, f, at + {k}, {ew}, (f, at, v) => {{ {inner} }});")
        }
        "map" => {
            let val_t = t.value.as_ref().unwrap();
            let ew = entry_words(model, t);
            let Some(key_spec) = &t.key else {
                let inner = js_put_field(model, val_t, "v", 1);
                return format!("heap.map(b.strings, {v}, f, at + {k}, {ew}, (f, at, v) => {{ {inner} }});");
            };
            let key_code = if key_spec.base == "num" {
                "if (typeof key !== \"number\") throw Error(\"a Map keyed by numbers has the key \" + String(key)); f[at] = key;".to_string()
            } else {
                let k_of = key_spec.of.as_ref().unwrap();
                if model.kinds.has(k_of) {
                    format!("b.refs[{}].put(key, f, at);", kind_const(k_of))
                } else {
                    format!("put{k_of}(b, key, f, at, heap);")
                }
            };
            let inner = js_put_field(model, val_t, "v", key_words(model, t));
            format!("heap.pairs({v}, f, at + {k}, {ew}, (f, at, key, v) => {{ {key_code} {inner} }});")
        }
        other => panic!("unexpected base {other}"),
    }
}

// Generates the JS statement that serializes a potentially nullable JS value `v`
// of type `t` into `f[at + k]`, writing `NaN`, `-1`, or a `0`/`1` tag word when
// null (`jsPutField(model, t, v, k)` in `tools/sim-schema.mjs`).
// Nullable wrapper around `js_put` (`jsPutField` in `tools/sim-schema.mjs`):
//   - For inline `NaN` sentinels (`f64`, `vec3`, `quat`, `mat4`, `f64xN`, `typed`),
//     writes `NaN` to `f[at + k]` and zeroes the remaining payload words via
//     `js_zero` when `v == null`.
//   - For tagged nullables (`t.nullable.tag == 1`), writes `0` (for `null`) or
//     `2` (for `undefined`) to `f[at + k]` and zeroes the payload words, or
//     writes `1` and packs the non-null payload at `k + 1`.
fn js_put_field(model: &Model, t: &TypeSpec, v: &str, k: usize) -> String {
    if !optional(t) { return js_put(model, t, v, k); }
    let n = words(model, t);
    let zeros = js_zero("f", "at", k + 1, k + n);
    let inner = js_put(model, &bare(t), "$x", k + 1);
    format!("{{ const $x = {v}; if ($x === null || $x === undefined) {{ f[at + {k}] = $x === null ? 1 : 2; {zeros} }} else {{ f[at + {k}] = 0; {inner} }} }}")
}

// Generates the JS boolean expression testing whether two non-null scalar,
// handle, enum, or fixed-vector values `a` and `b` of type `t` are equal
// (`jsSameValue(model, t, a, b)` in `tools/sim-schema.mjs`).
// Value equality expression generator (`jsSameValue` in `tools/sim-schema.mjs`):
//   - Used by `js_same_item` when comparing container elements (`list`, `map`, `obj`).
//   - Uses `Object.is(a, b)` for `f64` so `-0` vs `+0` and `NaN` vs `NaN` are
//     distinguished/matched identically to `runtime.js`.
//   - Uses `sameF32(a, b)` for `f32` (`Math.fround(a) === Math.fround(b)`).
//   - Uses strict `===` for integers, booleans, strings, enums, and handles.
//   - Delegates `vec3`, `quat`, `mat4`, `f64xN`, and `typed` to `sameVec`,
//     `sameQuat`, and `sameArray`, and `rec` / `union` to `same<Name>(a, b)`.
fn js_same_value(model: &Model, t: &TypeSpec, a: &str, b: &str) -> String {
    // Emit the JS expression comparing two non-null values `a` and `b` of bare
    // type `t` for equality (`Object.is` for `f64`, `sameF32` for `f32`,
    // `===` for primitives/handles, and `sameVec`/`sameQuat`/`sameArray` for vectors).
    match t.base.as_str() {
        "vec3" => format!("sameVec({a}, {b})"),
        "quat" => format!("sameQuat({a}, {b})"),
        "mat4" => format!("sameArray({a}?.elements, {b}?.elements, 16)"),
        "f64xN" => format!("sameArray({a}, {b}, {})", t.n),
        "rec" => format!("same{}({a}, {b}) === null", t.of.as_ref().unwrap()),
        "union" => format!("same{}({a}, {b})", t.of.as_ref().unwrap()),
        "list" => format!("sameEach({a}, {b}, (x, y) => {})", js_same_item(model, t.value.as_ref().unwrap(), "x", "y")),
        "set" => format!("sameList({a} && [...{a}], {b} && [...{b}])"),
        _ => format!("Object.is({a}, {b})"),
    }
}

// Generates the JS equality expression for container elements `a` and `b` of
// type `t` (`jsSameItem(model, t, a, b)` in `tools/sim-schema.mjs`).
// Builds the JS equality test for two elements `a` and `b` inside a `list:<T>`,
// `map:<K, V>`, or `obj:<V>` container (`jsSameItem` in `tools/sim-schema.mjs`):
//   - Wraps `js_same_value` with null/undefined handling when the element type `t`
//     is itself optional (`list:(ref:thing|null)` or `obj:(rec:Spot|null)`).
fn js_same_item(model: &Model, t: &TypeSpec, a: &str, b: &str) -> String {
    if !optional(t) { return js_same_value(model, t, a, b); }
    format!("({a} === null || {a} === undefined || {b} === null || {b} === undefined ? {a} === {b} : {})", js_same_value(model, &bare(t), a, b))
}

// Generates the JS check that returns the offending field `path` if non-null
// values `a` and `b` of bare type `t` differ (used by `check_<kind>` to detect
// unauthorized JS mutations of Rust-owned state). Mirrors `jsSame` in
// `tools/sim-schema.mjs`.
// Field mismatch detector (`jsSame` in `tools/sim-schema.mjs`):
//   - Used inside `same<Name>(a, b)` and `check<Kind>(b, f, at, h, m, scratch)`.
//   - Returns early with `path` (`false` inside `same<Name>`, or `"<field>"`
//     inside `check<Kind>`) as soon as a difference between `a` and `b` is found.
//   - For `list`, `set`, `map`, and `obj`, delegates to `sameEach`, `sameList`,
//     and `sameMap` from `src/sim/runtime.js` with element comparison callbacks.
fn js_same(model: &Model, t: &TypeSpec, a: &str, b: &str, path: &str) -> String {
    let p = quote_str(path);
    match t.base.as_str() {
        "vec3" => format!("if (!sameVec({a}, {b})) return {p};"),
        "quat" => format!("if (!sameQuat({a}, {b})) return {p};"),
        "mat4" => format!("if (!sameArray({a}?.elements, {b}?.elements, 16)) return {p};"),
        "f64xN" => format!("if (!sameArray({a}, {b}, {})) return {p};", t.n),
        "typed" => format!("if (!sameTyped({a}, {b}, {})) return {p};", t.n),
        "rec" => format!("{{ const d = same{}({a}, {b}); if (d) return {p} + d; }}", t.of.as_ref().unwrap()),
        "union" => format!("if (!same{}({a}, {b})) return {p};", t.of.as_ref().unwrap()),
        "list" => format!("if (!sameEach({a}, {b}, (x, y) => {})) return {p};", js_same_item(model, t.value.as_ref().unwrap(), "x", "y")),
        "set" => format!("if (!sameList({a} && [...{a}], {b} && [...{b}])) return {p};"),
        "map" => format!("if (!sameMap({a}, {b}, (x, y) => {})) return {p};", js_same_item(model, t.value.as_ref().unwrap(), "x", "y")),
        "obj" => format!("if (!sameObject({a}, {b}, (x, y) => {})) return {p};", js_same_item(model, t.value.as_ref().unwrap(), "x", "y")),
        _ => format!("if (!Object.is({a}, {b})) return {p};"),
    }
}

// Wraps `js_same` with null-check handling for optional fields (`jsSameField`
// in `tools/sim-schema.mjs`).
// Nullable wrapper around `js_same` (`jsSameField` in `tools/sim-schema.mjs`):
//   - If `t` is optional, first checks whether either side is `null` or `undefined`
//     (`a == null || b == null`) and verifies `a === b` before comparing non-null
//     payloads via `js_same`.
fn js_same_field(model: &Model, t: &TypeSpec, a: &str, b: &str, path: &str) -> String {
    if !optional(t) { return js_same(model, t, a, b, path); }
    let inner = js_same(model, &bare(t), "p", "q", path);
    format!(
        "{{ const p = {a}, q = {b}; if (p === null || p === undefined || q === null || q === undefined) {{ if (p !== q) return {}; }} else {{ {inner} }} }}",
        quote_str(path)
    )
}

// Generates the JS expression that converts a live or mirror value `v` of bare
// type `t` into a plain JSON-serializable snapshot value for `exportState`
// (`jsExport(model, t, v)` in `tools/sim-schema.mjs`).
// Value-to-snapshot converter (`jsExport` in `tools/sim-schema.mjs`):
//   - Clones Three.js math objects (`v.clone()`) and typed arrays (`v.slice()`),
//     spreads `f64xN` tuples (`[...v]`), and recursively maps `list`, `set`,
//     `map`, and `obj` collections into plain JSON-serializable structures.
//   - Converts entity references into `{ $ref, $id }` descriptors via `cx.held`
//     (for stored kinds) or `cx.ref` (for external host kinds).
fn js_export(model: &Model, t: &TypeSpec, v: &str) -> String {
    let one = |name: &str, x: &str| format!("cx.ref({}, {x})", quote_str(name));
    // Convert a live JS value `v` of bare type `t` into its plain JSON-friendly
    // export representation (`v.clone()` for Three.js math objects, `[...]` for
    // arrays/sets/maps, or entity handles via `cx.ref` / `cx.held`).
    match t.base.as_str() {
        "vec3" | "quat" | "mat4" => format!("{v}.clone()"),
        "f64xN" => format!("[...{v}]"),
        "typed" => format!("{v}.slice()"),
        "rec" | "union" => format!("export{}({v}, cx)", t.of.as_ref().unwrap()),
        "def" => format!("cx.def({}, {v})", quote_str(t.of.as_ref().unwrap())),
        "ref" => one(t.of.as_ref().unwrap(), v),
        "list" => format!("{v}.map((x) => {})", js_export_item(model, t.value.as_ref().unwrap(), "x")),
        "set" => format!("new Set([...{v}].map((x) => {}))", one(t.of.as_ref().unwrap(), "x")),
        "map" => {
            let inner = js_export_item(model, t.value.as_ref().unwrap(), "x");
            if let Some(key_spec) = &t.key {
                if key_spec.base == "ref" {
                    let k_of = key_spec.of.as_ref().unwrap();
                    let k_expr = if model.kinds.has(k_of) { one(k_of, "k") } else { format!("export{k_of}(k, cx)") };
                    return format!("new Map([...{v}].map(([k, x]) => [{k_expr}, {inner}]))");
                }
            }
            if inner == "x" { format!("new Map({v})") } else { format!("new Map([...{v}].map(([k, x]) => [k, {inner}]))") }
        }
        "obj" => {
            let inner = js_export_item(model, t.value.as_ref().unwrap(), "x");
            if inner == "x" {
                format!("({{ ...{v} }})")
            } else {
                format!("Object.fromEntries(Object.entries({v}).map(([k, x]) => [k, {inner}]))")
            }
        }
        _ => v.to_string(),
    }
}

// Generates the JS snapshot expression for a single container item `x` of type
// `t` (`jsExportItem(model, t, x)` in `tools/sim-schema.mjs`).
// Builds the JS snapshot expression for one container element `x` of type `t`
// (`jsExportItem` in `tools/sim-schema.mjs`):
//   - Stored entity handles (`ref:<K>` where `K.stored` is true) export as
//     `cx.held(KIND_<K>, x)` (`{ $ref, $id }`).
//   - External reference handles (`ref:<K>` where `!K.stored`) export as
//     `cx.ref(KIND_<K>, x)`.
//   - All other element types delegate to `js_export_field`.
fn js_export_item(model: &Model, t: &TypeSpec, x: &str) -> String {
    let inner = js_export(model, &bare(t), x);
    if !optional(t) || inner == x { return inner; }
    format!("({x} === null || {x} === undefined ? {x} : {inner})")
}

// Wraps `js_export` with null propagation (`v == null ? null : ...`) for
// optional fields (`jsExportField(model, t, v)` in `tools/sim-schema.mjs`).
// Nullable wrapper around `js_export` (`jsExportField` in `tools/sim-schema.mjs`):
//   - Propagates `null` and `undefined` (`v == null ? v : ...`) when `t` is
//     optional, and otherwise delegates directly to `js_export`.
fn js_export_field(model: &Model, t: &TypeSpec, v: &str) -> String {
    let inner = js_export(model, &bare(t), "x");
    if inner == "x" { return v.to_string(); }
    if !optional(t) { return js_export(model, t, v); }
    format!("((x) => (x === null || x === undefined ? x : {inner}))({v})")
}

// Returns the JS default initializer expression (`0`, `false`, `""`, `null`,
// `new Float64Array(...)`, `[]`, `new Set()`, `new Map()`, or `mirror_<rec>()`)
// for a field of type `t` on a newly constructed JS mirror object. Mirrors
// `jsMirrorInit(model, t)` in `tools/sim-schema.mjs`.
// Default field initializer on `<Name>` and `<Kind>Mirror` classes (`jsMirrorInit`
// in `tools/sim-schema.mjs`):
//   - If `t` is nullable or can be `undefined`, initializes to `null` or `undefined`
//     (preferring `null` when both `|null` and `|undefined` are allowed).
//   - Otherwise initializes scalars (`NaN` for floats, `0` for ints, `false` for
//     `bool`, `""` for `str`, variant 0 string for `enum`), allocates fresh
//     `new THREE.Vector3()`, `new THREE.Quaternion()`, `new THREE.Matrix4()`,
//     `new Float64Array(N)`, `new <Typed>Array(N)`, `[]`, `new Set()`, `new Map()`,
//     or `Object.create(null)` (`obj:<V>`), or constructs `new <Rec>()` /
//     the default union branch.
fn js_mirror_init(model: &Model, t: &TypeSpec) -> String {
    if optional(t) { return if t.nullable.null { "null" } else { "undefined" }.to_string(); }
    match t.base.as_str() {
        "f64" | "f32" => "NaN".to_string(),
        "i32" | "u32" | "u8" => "0".to_string(),
        "bool" => "false".to_string(),
        "vec3" => "new THREE.Vector3()".to_string(),
        "quat" => "new THREE.Quaternion()".to_string(),
        "mat4" => "new THREE.Matrix4()".to_string(),
        "f64xN" => format!("[{}]", vec!["NaN"; t.n].join(", ")),
        "str" => "''".to_string(),
        "enum" => quote_str(&model.enums.get(t.of.as_ref().unwrap()).unwrap().values[0]),
        "rec" => format!("new {}Mirror()", t.of.as_ref().unwrap()),
        "list" => "[]".to_string(),
        "set" => "new Set()".to_string(),
        "map" => "new Map()".to_string(),
        "obj" => "{}".to_string(),
        "typed" => format!("new {}({})", t.class.as_ref().unwrap(), t.n),
        _ => "null".to_string(),
    }
}

// Emits the `mirror_<name>(m = {})` constructor function that initializes all
// mirrored fields on a JS object (`jsMirror` in `tools/sim-schema.mjs`).
// Mirror class generator (`jsMirror` in `tools/sim-schema.mjs`):
//   - Emits `export class <Name> { constructor() { ... } }` with every field
//     initialized to its deterministic default (`js_mirror_init`) in exact
//     schema declaration order so all mirror instances share a single V8 hidden
//     class (map) without dictionary-mode transitions.
//   - When `slots` is true (slotted entity kinds), prepends `this.$slot = -1;`
//     and calls `Object.seal(this)` at the end of the constructor.
fn js_mirror(model: &Model, name: &str, fields: &[Field], slots: bool) -> Vec<String> {
    let mut lines = vec![format!("export class {name} {{"), "  constructor() {".to_string()];
    for f in fields { lines.push(format!("    this{} = {};", js_key(&f.name), js_mirror_init(model, &f.ty))); }
    if slots { lines.push("    this.simSlot = -1;".to_string()); lines.push("    this.simGen = -1;".to_string()); }
    lines.extend(["  }".to_string(), "}".to_string(), String::new()]);
    lines
}

// Emits the `same_<name>(a, b, path)` deep-comparison function for record `name`
// (`jsSameFn` in `tools/sim-schema.mjs`).
// Deep-equality checker (`jsSameFn` in `tools/sim-schema.mjs`):
//   - Emits `export function same<Name>(a, b)` for records and kinds, checking
//     `a === b` and `a != null && b != null` before comparing every mirrored
//     field in declaration order.
fn js_same_fn(model: &Model, name: &str, fields: &[Field]) -> Vec<String> {
    let mut lines = vec![format!("export function same{name}(a, b) {{")];
    for f in fields {
        if f.ty.base == "index" {
            let x = js_get(f, "a");
            let y = js_get(f, "b");
            let list = f.ty.of.as_ref().unwrap();
            lines.push(format!(
                "  {{ const x = {x}, y = {y}; if (x === null || x === undefined || y === null || y === undefined ? x !== y : (a.{list} ?? []).indexOf(x) !== (b.{list} ?? []).indexOf(y)) return {}; }}",
                quote_str(&format!(".{}", f.name))
            ));
            continue;
        }
        lines.push(format!("  {}", js_same_field(model, &f.ty, &js_get(f, "a"), &js_get(f, "b"), &format!(".{}", f.name))));
    }
    lines.extend(["  return null;".to_string(), "}".to_string(), String::new()]);
    lines
}

// Emits the `export_<name>(o)` snapshot serializer for a record or kind
// (`jsExportFn` in `tools/sim-schema.mjs`).
// JSON snapshot exporter (`jsExportFn` in `tools/sim-schema.mjs`):
//   - Emits `export function export<Name>(o, cx)` which produces a plain JS
//     object suitable for `JSON.stringify` and Rust `replay.rs` comparison.
//   - For slotted kinds with a numeric key (`k.key`), writes `$id: o[key]` as
//     the first property; for home-list fields on a singleton, serializes owned
//     entities in full via `cx.home(k, item)`.
fn js_export_fn(model: &Model, name: &str, fields: &[Field], kind: Option<&KindDef>) -> Vec<String> {
    let mut lines = vec![
        format!("export function export{name}(o, cx) {{"),
        "  let v = cx.seen.get(o);".to_string(),
        "  if (v) return v;".to_string(),
        "  v = {};".to_string(),
        "  cx.seen.set(o, v);".to_string(),
    ];
    if let Some(k) = kind { if !k.singleton { lines.push(format!("  v.$id = cx.ref({}, o);", quote_str(&k.name))); } }
    for f in fields {
        let home = model.is_home(f);
        if f.ty.base == "index" {
            let list_of = f.ty.of.as_ref().unwrap();
            let rec = fields
                .iter()
                .find(|g| &g.name == list_of)
                .unwrap()
                .ty
                .value
                .as_ref()
                .unwrap()
                .of
                .as_ref()
                .unwrap();
            lines.push(format!(
                "  v{} = ((x) => (x === null || x === undefined ? x : export{rec}(x, cx)))({});",
                js_key(&f.name),
                js_get(f, "o")
            ));
            continue;
        }
        let rhs = if home {
            let k_rust = &model.kinds.get(f.ty.of.as_ref().unwrap()).unwrap().rust;
            format!("{}.map((x) => export{k_rust}(x, cx))", js_get(f, "o"))
        } else {
            js_export_field(model, &f.ty, &js_get(f, "o"))
        };
        lines.push(format!("  v{} = {rhs};", js_key(&f.name)));
    }
    lines.extend(["  return v;".to_string(), "}".to_string(), String::new()]);
    lines
}

// Emits a `sync_<name>(f, at, h, m)` function that decodes placed fields from
// Float64Array `f` at `at` (and heap `h`) into JS mirror `m` (`jsSyncFn` in
// `tools/sim-schema.mjs`).
// Row-to-mirror reader (`jsSyncFn` in `tools/sim-schema.mjs`):
//   - Emits `export function <name>(b, f, at, h, m)` which decodes each placed
//     field from `Float64Array` `f` (at base word `at`) and heap `h` into object `m`.
fn js_sync_fn(model: &Model, name: &str, placed: &[PlacedField], target: &str) -> Vec<String> {
    let mut lines = vec![format!("export function {name}(b, f, at, h, m) {{")];
    for PlacedField { field, at } in placed { lines.push(format!("  {}", js_sync_field(model, &field.ty, &js_set(field, target), *at))); }
    lines.extend(["  return m;".to_string(), "}".to_string(), String::new()]);
    lines
}

// Returns true if record `name` (or any nested type within it) reads or writes
// the side-buffer heap (`recordHeap(model, name)` in `tools/sim-schema.mjs`).
// Recursive heap-usage predicate (`recordHeap` in `tools/sim-schema.mjs`):
//   - Returns true when record `name` has any container (`list`, `set`, `map`,
//     `obj`), nested heap-using record, or union with a heap-using variant.
fn record_heap(model: &Model, name: &str) -> bool {
    model.records.get(name).unwrap().fields.iter().any(|f| {
        container(&f.ty)
            || f.ty.base == "set"
            || (bare(&f.ty).base == "rec" && record_heap(model, f.ty.of.as_ref().unwrap()))
    })
}

// Computes the set of records and unions that require a `<name>_heap(f, at, h, p)`
// deep heap-comparison helper for delta detection (`heapHelpers(model)` in
// `tools/sim-schema.mjs`).
// Delta heap-helper reachability analysis (`heapHelpers` in `tools/sim-schema.mjs`):
//   - Walks every record with `$delta` / `$refresh` and every kind with `$delta` /
//     `$defer` to collect the transitive closure of records and unions that own
//     heap data (`list`, `set`, `map`, `obj`, `typed`) and therefore need a
//     generated `heapChanged<Name>(d, f, at, base)` function in their JS module.
fn heap_helpers(model: &Model) -> HashSet<String> {
    // Traverse all delta-tracked records and deferred-sync kinds to find every
    // nested record and union that needs a `heapChanged<Name>` comparison helper.
    let mut needed = HashSet::new();
    let mut visited = HashSet::new();
    fn visit(model: &Model, ty: &TypeSpec, needed: &mut HashSet<String>, visited: &mut HashSet<String>) {
        if container(ty) {
            let child = ty.value.as_ref().unwrap();
            if child.base == "rec" && record_heap(model, child.of.as_ref().unwrap()) { needed.insert(child.of.clone().unwrap()); }
            visit(model, child, needed, visited);
        } else if ty.base == "rec" {
            let of = ty.of.as_ref().unwrap();
            if visited.insert(of.clone()) { for f in &model.records.get(of).unwrap().fields { visit(model, &f.ty, needed, visited); } }
        }
    }
    for r in model.records.values() { for f in &r.fields { if r.delta.contains(&f.name) { visit(model, &f.ty, &mut needed, &mut visited); } } }
    for k in model.kinds.values() { if k.delta { for f in &k.fields { visit(model, &f.ty, &mut needed, &mut visited); } } }
    needed
}

// Generates the JS statements that compare heap-backed data at word offset `k`
// between the current (`f`, `h`) and previous (`p`) export buffers (`jsDeltaHeapTests`
// in `tools/sim-schema.mjs`).
// Heap-change test generator (`jsDeltaHeapTests` in `tools/sim-schema.mjs`):
//   - Because a container's 2-word `[offset, len]` header in the row can stay
//     identical across ticks when the heap allocator places a new list at the
//     same offset, `syncDelta` must also check `heapChanged(d, f[at + k], ...)`
//     over the heap words occupied by the container's elements.
fn js_delta_heap_tests(model: &Model, t: &TypeSpec, k: usize) -> Vec<String> {
    let value = bare(t);
    let start = k + if optional(t) { 1 } else { 0 };
    if value.base == "set" { return vec![format!("deltaChanged(d, h + f[at + {start}], f[at + {}] * 2, base)", start + 1)]; }
    if container(&value) {
        let key_w = if matches!(value.base.as_str(), "map" | "obj") { key_words(model, &value) } else { 0 };
        let child = value.value.as_ref().unwrap();
        let width = key_w + words(model, child);
        let mut tests = vec![format!("deltaChanged(d, h + f[at + {start}], f[at + {}] * {width}, base)", start + 1)];
        let bare_child = bare(child);
        if bare_child.base == "rec" && record_heap(model, bare_child.of.as_ref().unwrap()) {
            let c_of = bare_child.of.as_ref().unwrap();
            let c_opt = optional(child);
            tests.push(format!(
                "changed{c_of}HeapList(f, h + f[at + {start}] + {}, f[at + {}], h, d, base, {width}, {c_opt})",
                key_w + if c_opt { 1 } else { 0 },
                start + 1
            ));
        } else if container(&bare_child) {
            tests.push("deltaChanged(d, h, (d.length << 5) - (h - base), base)".to_string());
        }
        return tests;
    }
    if value.base != "rec" { return Vec::new(); }
    let mut tests = Vec::new();
    let mut offset = start;
    for field in &model.records.get(value.of.as_ref().unwrap()).unwrap().fields {
        tests.extend(js_delta_heap_tests(model, &field.ty, offset));
        offset += words(model, &field.ty);
    }
    tests
}

// Generates the fast inline word-comparison expression between `f[at + k..]`
// and `p[at + k..]` for a field of type `t` (`jsDeltaTest` in `tools/sim-schema.mjs`).
// Builds the combined word + heap dirty test expression for a field of type `t`
// at word offset `k` inside `syncDelta<Name>` (`jsDeltaTest` in `tools/sim-schema.mjs`):
//   - Always starts with `deltaChanged(d, at + k, words(t), base)` to test whether
//     any inline word in the parent row changed.
//   - Appends `js_delta_heap_tests(model, t, k)` with `||` when `t` owns or
//     transitively contains side-buffer heap allocations.
fn js_delta_test(model: &Model, t: &TypeSpec, k: usize) -> String {
    let mut all = vec![format!("deltaChanged(d, at + {k}, {}, base)", words(model, t))];
    all.extend(js_delta_heap_tests(model, t, k));
    format!("({})", all.join(" || "))
}

// Returns true if `ty` is or transitively contains a record with `observed`
// fields (`observedType(model, type, seen)` in `tools/sim-schema.mjs`).
// Recursively checks whether `ty` is a record (or list/map/obj of records)
// that declares `$observe` fields (`observedType` in `tools/sim-schema.mjs`),
// using `seen` to guard against cycles.
fn observed_type(model: &Model, ty: &TypeSpec, seen: &mut HashSet<String>) -> bool {
    if container(ty) { return observed_type(model, ty.value.as_ref().unwrap(), seen); }
    let t = bare(ty);
    if t.base != "rec" { return false; }
    let of = t.of.as_ref().unwrap();
    if !seen.insert(of.clone()) { return false; }
    let r = model.records.get(of).unwrap();
    !r.observe.is_empty() || r.fields.iter().any(|f| observed_type(model, &f.ty, seen))
}

// Generates the JS expression testing whether any observed sub-field inside `ty`
// at word offset `k` changed between `(f, at, h)` and `(p, pat, ph)`
// (`observedTest(model, type, k)` in `tools/sim-schema.mjs`).
// Builds the JS boolean expression testing whether any `$observe` field inside
// `ty` (at word offset `k`) changed during a delta sync (`observedTest` in
// `tools/sim-schema.mjs`).
fn observed_test(model: &Model, ty: &TypeSpec, k: usize) -> String {
    let t = bare(ty);
    let start = k + if optional(ty) { 1 } else { 0 };
    let mut tests = Vec::new();
    if optional(ty) { tests.push(format!("deltaChanged(d, at + {k}, 1, base)")); }
    if t.base == "rec" {
        let r = model.records.get(t.of.as_ref().unwrap()).unwrap();
        let mut offset = start;
        for f in &r.fields {
            if !r.observe.contains(&f.name) { tests.push(observed_test(model, &f.ty, offset)); }
            offset += words(model, &f.ty);
        }
    } else if t.base == "list"
        && !optional(t.value.as_ref().unwrap())
        && bare(t.value.as_ref().unwrap()).base == "rec"
        && observed_type(model, t.value.as_ref().unwrap(), &mut HashSet::new())
    {
        tests.push(format!("deltaChanged(d, at + {start}, 2, base)"));
        let v = t.value.as_ref().unwrap();
        let child = bare(v);
        if child.base == "rec" && observed_test(model, v, 0) != "(false)" {
            tests.push(format!(
                "changed{}ObservedList(f, h + f[at + {start}], f[at + {}], h, d, base, {})",
                child.of.as_ref().unwrap(),
                start + 1,
                words(model, v)
            ));
        }
    } else {
        tests.push(js_delta_test(model, &t, start));
    }
    format!("({})", if tests.is_empty() { "false".to_string() } else { tests.join(" || ") })
}

// Generates the JS statements that selectively sync only changed observed fields
// inside `ty` at offset `k` (`jsSyncObserved` in `tools/sim-schema.mjs`).
// Generates the JS statements that sync only the `$observe` sub-fields of `ty`
// at word offset `k` into `target` (`jsSyncObserved` in `tools/sim-schema.mjs`),
// leaving deferred/unobserved sibling fields untouched until cold-read.
fn js_sync_observed(model: &Model, ty: &TypeSpec, target: &str, k: usize) -> Vec<String> {
    let t = bare(ty);
    let start = k + if optional(ty) { 1 } else { 0 };
    let mut lines = Vec::new();
    if t.base == "rec" {
        lines.push(format!("sync{}Observed(b, f, at + {start}, h, {target});", t.of.as_ref().unwrap()));
    } else if t.base == "list"
        && !optional(t.value.as_ref().unwrap())
        && t.value.as_ref().unwrap().base == "rec"
        && observed_type(model, t.value.as_ref().unwrap(), &mut HashSet::new())
    {
        let v = t.value.as_ref().unwrap();
        lines.push(format!(
            "{{ const start=(h + f[at + {start}]) | 0, n=f[at + {}] | 0; for(let i=0;i<n;i++)sync{}Observed(b,f,start+i*{},h,{target}[i]); }}",
            start + 1,
            v.of.as_ref().unwrap(),
            words(model, v)
        ));
    }
    if optional(ty) && !lines.is_empty() { return vec![format!("if(f[at + {k}]===0) {{ {} }}", lines.join(" "))]; }
    lines
}

// Emits `<rec>_observed(f, at, h, p, pat, ph)` and `<rec>_sync_observed(...)`
// helpers for records that declare `observed` fields (`jsObservedReader` in
// `tools/sim-schema.mjs`).
// Observed-field reader generator (`jsObservedReader` in `tools/sim-schema.mjs`):
//   - For records declaring `$observe: [...]`, emits `syncObserved<Name>` and
//     `markObserved<Name>` so nested observed sub-fields can be updated without
//     re-reading unobserved fields on the parent record.
fn js_observed_reader(model: &Model, rec: &RecordDef) -> Vec<String> {
    let mut lines = vec![format!("export function sync{}Observed(b, f, at, h, m) {{", rec.name), "  at |= 0; h |= 0;".to_string(),];
    let mut offset = 0usize;
    for field in &rec.fields {
        if rec.observe.contains(&field.name) {
            lines.push(format!("  {}", js_sync_field(model, &field.ty, &js_set(field, "m"), offset)));
        } else if observed_type(model, &field.ty, &mut HashSet::new()) {
            for line in js_sync_observed(model, &field.ty, &js_set(field, "m"), offset) { lines.push(format!("  {line}")); }
        }
        offset += words(model, &field.ty);
    }
    lines.extend(["}".to_string(), String::new()]);
    lines
}

// Generates JS statements to mark observed fields dirty when syncing a delta
// root record (`jsMarkObserved` in `tools/sim-schema.mjs`).
// Generates the JS statements that record dirty flags for `$observe` sub-fields
// inside `ty` at word offset `k` when a deferred parent row is skipped during
// hot sync (`jsMarkObserved` in `tools/sim-schema.mjs`).
fn js_mark_observed(model: &Model, ty: &TypeSpec, k: usize) -> Vec<String> {
    let t = bare(ty);
    let start = k + if optional(ty) { 1 } else { 0 };
    let mut lines = Vec::new();
    if t.base == "rec" {
        let mut offset = start;
        let r = model.records.get(t.of.as_ref().unwrap()).unwrap();
        for f in &r.fields {
            if !r.observe.contains(&f.name) { lines.extend(js_mark_observed(model, &f.ty, offset)); }
            offset += words(model, &f.ty);
        }
    } else if container(&t) {
        lines.push(format!("markDelta(d, at + {start}, 2, base);"));
        let child = t.value.as_ref().unwrap();
        if t.base == "list"
            && !optional(child)
            && child.base == "rec"
            && observed_type(model, child, &mut HashSet::new())
        {
            let read = js_mark_observed(model, child, 0);
            if !read.is_empty() {
                lines.push(format!(
                    "{{ const start = (h + f[at + {start}]) | 0, n = f[at + {}] | 0; for(let i=0;i<n;i++) {{ const at=start+i*{}; {} }} }}",
                    start + 1,
                    words(model, child),
                    read.join(" ")
                ));
            }
        } else {
            let keys = if matches!(t.base.as_str(), "map" | "obj") { key_words(model, &t) } else { 0 };
            lines.push(format!("markDelta(d, h + f[at + {start}], f[at + {}] * {}, base);", start + 1, keys + words(model, child)));
            if child.base == "rec" && record_heap(model, child.of.as_ref().unwrap()) {
                lines.push("markDelta(d, h, (d.length << 5) - (h - base), base);".to_string());
            }
        }
    } else {
        lines.push(format!("markDelta(d, at + {start}, {}, base);", words(model, &t)));
    }
    if optional(ty) {
        let mut out = vec![format!("markDelta(d, at + {k}, 1, base);")];
        if !lines.is_empty() { out.push(format!("if(f[at + {k}]===0) {{ {} }}", lines.join(" "))); }
        return out;
    }
    lines
}

// Emits the incremental delta-sync function `sync_delta_<rec>(f, at, h, p, m)`
// for a singleton root record (`jsDeltaRoot` in `tools/sim-schema.mjs`).
// Incremental delta-sync codegen (`jsDeltaRoot` in `tools/sim-schema.mjs`):
//   - Emits `syncDelta<Name>(b, f, at, h, m, d, base)` where `d` is the word-level
//     diff bitset/buffer produced by the Wasm boundary (`deltaChanged`).
//   - Groups contiguous scalar fields when `rec.delta` covers the entire record so
//     unchanged word ranges can be skipped with a single loop test.
//   - For heap-backed fields (`str`, `list`, `set`, `map`, `obj`, or nested records
//     with heap fields), combines inline word checks with `heapChanged*` helpers
//     so mutations inside the side-buffer heap are never missed.
//   - When `rec.observe` is non-empty, also maintains the `m.$changed` bitmask /
//     set of observed fields modified during the tick.
fn js_delta_root(model: &Model, rec: &RecordDef, placed: &[PlacedField], width: usize) -> Vec<String> {
    let deferred = rec
        .fields
        .iter()
        .any(|f| rec.parts.contains(&f.name) && matches!(f.ty.base.as_str(), "list" | "vec3"));
    let observed = !rec.observe.is_empty()
        || rec
            .fields
            .iter()
            .any(|f| observed_type(model, &f.ty, &mut HashSet::new()));
    let mut lines = vec![
        format!(
            "export function sync{}Delta(b, f, at, h, m, d, force = false, base = 0{}{}) {{",
            rec.name,
            if deferred { ", deferred = false" } else { "" },
            if observed { ", observer = null" } else { "" }
        ),
        "  at |= 0; h |= 0; base |= 0;".to_string(),
    ];
    if !rec.observe.is_empty() { lines.push(format!("  if (observer) observeRecord(m, {}ObservedFields, observer);", rec.name)); }

    let read_field = |p: &PlacedField, indent: &str| -> String {
        let field = &p.field;
        let at = p.at;
        let target = js_set(field, "m");
        let ty = &field.ty;
        let nested = ty.base == "rec" && !model.records.get(ty.of.as_ref().unwrap()).unwrap().delta.is_empty();
        let child_args = if observed_type(model, ty, &mut HashSet::new()) { ", observer" } else { "" };
        let mut read = if nested {
            format!("{target} = sync{}Delta(b, f, at + {at}, h, {target}, d, force, base{child_args});", ty.of.as_ref().unwrap())
        } else {
            js_sync_field(model, ty, &target, at)
        };
        if nested && optional(ty) {
            let of = ty.of.as_ref().unwrap();
            read = format!(
                "{{ const tag = f[at + {at}]; if (tag === 0) {{ const fresh = !{target} || deltaChanged(d, at + {at}, 1, base); {target} = sync{of}Delta(b, f, at + {}, h, {target} ?? new {of}Mirror(), d, force || fresh, base{child_args}); }} else {target} = tag === 1 ? null : undefined; }}",
                at + 1
            );
        }
        if ty.base == "list" && !optional(ty) {
            let val_t = ty.value.as_ref().unwrap();
            if val_t.base == "rec"
                && !optional(val_t)
                && !model.records.get(val_t.of.as_ref().unwrap()).unwrap().delta.is_empty()
            {
                let child = val_t.of.as_ref().unwrap();
                let w = words(model, val_t);
                let item_test = if observed_type(model, val_t, &mut HashSet::new()) {
                    format!("(observer ? {} : {})", observed_test(model, val_t, 0), js_delta_test(model, val_t, 0))
                } else {
                    js_delta_test(model, val_t, 0)
                };
                read = format!(
                    "{{ const items = ({target} ??= []), start = (h + f[at + {at}]) | 0, n = f[at + {}] | 0, fresh = force || deltaChanged(d, at + {at}, 2, base); while (items.length > n) items.pop(); for (let i = 0; i < n; i++) {{ const at = start + i * {w}; if (fresh || {item_test}) items[i] = sync{child}Delta(b, f, at, h, items[i] ?? new {child}Mirror(), d, fresh, base{child_args}); }} }}",
                    at + 1
                );
            }
        }
        let gate = if rec.observe.contains(&field.name) {
            "!observer && "
        } else if deferred && rec.parts.contains(&field.name) && matches!(ty.base.as_str(), "list" | "vec3") {
            "!deferred && "
        } else {
            ""
        };
        let eager = if rec.defer.as_ref().map(|d| d.contains(&field.name)).unwrap_or(false) {
            "observer.eagerChanged".to_string()
        } else {
            observed_test(model, ty, at)
        };
        let test = if observed_type(model, ty, &mut HashSet::new()) {
            format!("(observer ? {eager} : {})", js_delta_test(model, ty, at))
        } else {
            js_delta_test(model, ty, at)
        };
        if rec.delta.contains(&field.name) { format!("{indent}if ({gate}(force || {test})) {{ {read} }}") } else { format!("{indent}{read}") }
    };

    let is_scalar = |p: &PlacedField| { matches!(p.field.ty.base.as_str(), "f64" | "f32" | "i32" | "u32" | "u8" | "bool" | "enum") };
    if rec.delta.len() == placed.len() && placed.iter().any(is_scalar) {
        if deferred || observed {
            lines.push("  if (force) {".to_string());
            for p in placed { lines.push(read_field(p, "    ")); }
            lines.push("    return m;".to_string());
            lines.push("  }".to_string());
        } else {
            lines.push(format!("  if (force) return sync{}(b, f, at, h, m);", rec.name));
        }
        lines.extend([
            format!("  const origin = at - base, end = origin + {width}, first = origin >>> 5, last = (end - 1) >>> 5;"),
            "  for (let word = first; word <= last; word++) {".to_string(),
            "    let bits = d[word] >>> 0;".to_string(),
            "    if (word === first) bits &= 0xffffffff << (origin & 31);".to_string(),
            "    if (word === last && (end & 31)) bits &= 0xffffffff >>> (32 - (end & 31));".to_string(),
            "    while (bits) {".to_string(),
            "      const bit = 31 - Math.clz32(bits & -bits);".to_string(),
            "      switch ((word << 5) + bit - origin) {".to_string(),
        ]);
        for p in placed.iter().filter(|p| is_scalar(p)) {
            let w = words(model, &p.field.ty);
            for k in 0..w { lines.push(format!("        case {}:", p.at + k)); }
            let read = js_sync_field(model, &p.field.ty, &js_set(&p.field, "m"), p.at);
            let body = if rec.observe.contains(&p.field.name) { format!("if (!observer) {{ {read} }}") } else { read };
            lines.push(format!("          {{ {body} break; }}"));
        }
        lines.extend(["      }".to_string(), "      bits &= bits - 1;".to_string(), "    }".to_string(), "  }".to_string(),]);
        for p in placed.iter().filter(|p| !is_scalar(p)) { lines.push(read_field(p, "  ")); }
    } else {
        for p in placed { lines.push(read_field(p, "  ")); }
    }
    lines.extend(["  return m;".to_string(), "}".to_string(), String::new()]);
    lines
}

// Emits `put_<name>(o, f, at, heap)` to serialize a JS object `o` into a flat
// `f64` row (`jsPutFn` in `tools/sim-schema.mjs`).
// Row serializer codegen (`jsPutFn` in `tools/sim-schema.mjs`):
//   - When `split` is true and `placed.len() > 24` (12 * 2), splits the field
//     assignments across helper functions `<name>_0`, `<name>_1`, ... of 12 fields
//     each so V8 can optimize and inline each chunk without hitting bytecode size
//     limits on large records like `HudFrame`.
fn js_put_fn(model: &Model, name: &str, placed: &[PlacedField], split: bool) -> Vec<String> {
    let chunk = 12usize;
    if split && placed.len() > chunk * 2 {
        let mut lines = vec![format!("export function {name}(b, o, f, at, heap) {{")];
        let mut i = 0;
        while i < placed.len() { lines.push(format!("  {name}Part{}(b, o, f, at, heap);", i / chunk)); i += chunk; }
        lines.push("}".to_string());
        lines.push(String::new());
        let mut i = 0;
        while i < placed.len() {
            lines.push(format!("function {name}Part{}(b, o, f, at, heap) {{", i / chunk));
            let end = (i + chunk).min(placed.len());
            for PlacedField { field, at } in &placed[i..end] {
                lines.push(format!("  {}", js_put_field(model, &field.ty, &js_get(field, "o"), *at)));
            }
            lines.push("}".to_string());
            lines.push(String::new());
            i += chunk;
        }
        return lines;
    }
    let mut lines = vec![format!("export function {name}(b, o, f, at, heap) {{")];
    for PlacedField { field, at } in placed { lines.push(format!("  {}", js_put_field(model, &field.ty, &js_get(field, "o"), *at))); }
    lines.extend(["}".to_string(), String::new()]);
    lines
}

// Emits a zero-copy JS view class (`<Name>View`) with getters over a flat
// `Float64Array` slice for flat records (`jsFlatView` in `tools/sim-schema.mjs`).
// Zero-copy flat record view class (`jsFlatView` in `tools/sim-schema.mjs`):
//   - Emits `export class <Name>View` backed directly by a `Float64Array` (`#f`)
//     and word offset (`#at`), avoiding per-element object allocation when reading
//     arrays of flat records (`list:rec:<Name>`).
//   - Pre-allocates reusable Three.js `Vector3` / `Quaternion` / `Matrix4` scratch
//     fields for math properties and exposes a getter/setter pair for every field.
fn js_flat_view(model: &Model, rec: &RecordDef, placed: &[PlacedField], size: usize) -> Vec<String> {
    let is_math = |f: &Field| matches!(f.ty.base.as_str(), "vec3" | "quat" | "mat4");
    let names_arr: Vec<JsonValue> = placed
        .iter()
        .map(|p| JsonValue::String(p.field.name.clone()))
        .collect();
    let names = JsonValue::Array(names_arr).to_json_compact();
    let mut lines = vec![
        format!("const {}ViewFields = new Set({names});", rec.name),
        format!("export function create{}View(b, source) {{", rec.name),
        format!("  const words = new Float64Array({size});"),
        format!("  put{}(b, source, words, 0, null);", rec.name),
        "  const view = Object.create(Object.getPrototypeOf(source));".to_string(),
        "  Object.defineProperties(view, {".to_string(),
        "    predictionWords: { value: words },".to_string(),
    ];
    for PlacedField { field, at } in placed {
        if is_math(field) {
            lines.push(format!(
                "    {}: {{ enumerable: true, writable: true, value: source.{}?.clone() ?? source.{} }},",
                field.name, field.name, field.name
            ));
        } else {
            lines.push(format!(
                "    {}: {{ enumerable: true, get() {{ const f = words, at = 0, h = 0; let value; {} return value; }}, set(value) {{ const f = words, at = 0, heap = null; {} }} }},",
                field.name,
                js_sync_field(model, &field.ty, "value", *at),
                js_put_field(model, &field.ty, "value", *at)
            ));
        }
    }
    lines.extend([
        "  });".to_string(),
        format!("  for (const key of Object.keys(source)) if (!{}ViewFields.has(key)) view[key] = source[key];", rec.name),
        "  return view;".to_string(),
        "}".to_string(),
        String::new(),
        format!("export function sync{}ViewMath(b, view) {{", rec.name),
        "  const f = view.predictionWords, at = 0, h = 0;".to_string(),
    ]);
    for PlacedField { field, at } in placed {
        if is_math(field) { lines.push(format!("  {}", js_sync_field(model, &field.ty, &js_set(field, "view"), *at))); }
    }
    lines.extend(["}".to_string(), String::new()]);
    lines
}

// Emits all JS functions and constants (`<NAME>_WORDS`, `mirror_<name>`,
// `sync_<name>`, `put_<name>`, `same_<name>`, `export_<name>`, plus optional
// flat view, heap comparison, and delta/observed helpers) for record `rec`.
// Mirrors `jsRecord(model, rec)` in `tools/sim-schema.mjs`.
// Complete JS record codegen (`jsRecord(model, rec)` in `tools/sim-schema.mjs`):
//   - Places all fields contiguously starting at word offset 0 (`<NAME>_WORDS`).
//   - Emits `<Name>` mirror class, `sync<Name>`, optional `syncDelta<Name>` and
//     `syncRefresh<Name>`, `put<Name>` (plus any `$parts` sub-writers), optional
//     `<Name>View` and `publish<Name>` helpers, `same<Name>`, and `export<Name>`.
fn js_record(model: &Model, rec: &RecordDef) -> Vec<String> {
    let mut at = 0usize;
    let mut placed = Vec::new();
    for f in &rec.fields { placed.push(PlacedField { field: f.clone(), at, }); at += words(model, &f.ty); }
    let mut lines = Vec::new();
    // When `$layout: true` is set on a record, emit `<NAME>_WORDS` and the
    // frozen `<NAME>_OFFSET` object mapping each field name to its word offset.
    if rec.layout {
        let mut layout_entries: Vec<String> = placed
            .iter()
            .flat_map(|p| {
                if p.field.ty.base == "list" {
                    vec![format!("{}: {}", p.field.name, p.at), format!("{}Length: {}", p.field.name, p.at + 1),]
                } else {
                    vec![format!("{}: {}", p.field.name, p.at)]
                }
            })
            .collect();
        for p in &placed {
            if rec.parts.contains(&p.field.name) { layout_entries.push(format!("{}Words: {}", p.field.name, words(model, &p.field.ty))); }
        }
        let tail = if layout_entries.is_empty() { String::new() } else { format!(", {}", layout_entries.join(", ")) };
        lines.extend([
            format!("export const {}Layout = Object.freeze({{ words: {at}{tail} }});", rec.name),
            format!("export const {}_WORDS = {}Layout.words;", upper(&rec.name), rec.name),
            format!("export const {}_OFFSETS = {}Layout;", upper(&rec.name), rec.name),
            String::new(),
        ]);
    }
    lines.extend(js_mirror(model, &format!("{}Mirror", rec.name), &rec.fields, false));
    lines.extend(js_sync_fn(model, &format!("sync{}", rec.name), &placed, "m"));
    // Emit incremental delta-sync (`syncDelta<Name>`) and observed-field reader
    // helpers when `$delta` or `$observe` is configured on the record.
    if !rec.delta.is_empty() { lines.extend(js_delta_root(model, rec, &placed, at)); }
    if !rec.refresh.is_empty() {
        let refresh_placed: Vec<PlacedField> = placed
            .iter()
            .filter(|p| rec.refresh.contains(&p.field.name))
            .cloned()
            .collect();
        let refresh_words: usize = refresh_placed.iter().map(|p| words(model, &p.field.ty)).sum();
        lines.push(format!("export const {}_REFRESH_WORDS = {refresh_words};", upper(&rec.name)));
        lines.extend(js_sync_fn(model, &format!("refresh{}", rec.name), &refresh_placed, "m"));
    }
    lines.extend(js_put_fn(model, &format!("put{}", rec.name), &placed, rec.view));
    for part_name in &rec.parts {
        let part_placed: Vec<PlacedField> = placed
            .iter()
            .filter(|p| &p.field.name == part_name)
            .cloned()
            .collect();
        lines.extend(js_put_fn(model, &format!("put{}_{part_name}", rec.name), &part_placed, false));
    }
    // Emit the zero-copy `<Name>View` class over a flat `Float64Array` slice
    // when `$view: true` is set, and `<Name>Publication` double-buffered helpers
    // when `$publish` is configured.
    if rec.view { lines.extend(js_flat_view(model, rec, &placed, at)); }
    if !rec.publish.is_empty() {
        lines.push(format!("export function publish{}(o, m) {{", rec.name));
        for name in &rec.publish {
            let f = rec.fields.iter().find(|field| &field.name == name).unwrap();
            if f.ty.base == "vec3" {
                lines.push(format!("  {}.copy(m{});", js_get(f, "o"), js_key(name)));
            } else {
                lines.push(format!("  {} = m{};", js_get(f, "o"), js_key(name)));
            }
        }
        lines.extend(["}".to_string(), String::new()]);
    }
    lines.extend(js_same_fn(model, &rec.name, &rec.fields));
    lines.extend(js_export_fn(model, &rec.name, &rec.fields, None));
    lines
}

// Emits JS tag lookup, `sync_<union>`, `put_<union>`, `same_<union>`, and
// `export_<union>` functions for union `u` (`jsUnion(model, u)` in
// `tools/sim-schema.mjs`).
// Complete JS union codegen (`jsUnion(model, u)` in `tools/sim-schema.mjs`):
//   - Determines how to distinguish variants at runtime in JS (`variantOf<Name>`):
//     * `null` and `undefined` unit checks first
//     * primitive `typeof` (`"number"`, `"boolean"`, `"string"`) and enum set membership
//     * `Vector3` (`isVector3`), `Quaternion` (`isQuaternion`), `Matrix4` (`isMatrix4`),
//       and `Array.isArray` checks
//     * record discrimination via `$class` constructor tag (`b.recTag`) when multiple
//       record variants exist, or `b.kindOf(x)` when multiple entity handle variants exist.
//   - Emits `sync<Name>`, `put<Name>`, `same<Name>`, and `export<Name>` switching
//     on the 0-based variant tag word at `f[at]`.
fn js_union(model: &Model, u: &UnionDef) -> Vec<String> {
    let mut lines = Vec::new();
    let values: Vec<(&UnionVariant, usize)> = u
        .variants
        .iter()
        .enumerate()
        .filter(|(_, v)| v.tag == "value")
        .map(|(i, v)| (v, i))
        .collect();
    let recs: Vec<(&UnionVariant, usize)> = values
        .iter()
        .copied()
        .filter(|(v, _)| v.ty.as_ref().unwrap().base == "rec")
        .collect();
    let tagged = recs.len() > 1;
    let refs: Vec<(&UnionVariant, usize)> = u
        .variants
        .iter()
        .enumerate()
        .filter(|(_, v)| v.tag == "ref")
        .map(|(i, v)| (v, i))
        .collect();

    let mut tests = Vec::new();
    for (i, v) in u.variants.iter().enumerate() {
        match v.tag.as_str() {
            "null" => tests.push(format!("if (x === null) return {i};")),
            "undefined" => tests.push(format!("if (x === undefined) return {i};")),
            "lit" => tests.push(format!("if (x === {}) return {i};", quote_str(v.value.as_ref().unwrap()))),
            _ => {}
        }
    }
    for &(v, i) in &values {
        let t = v.ty.as_ref().unwrap();
        match t.base.as_str() {
            "f64" | "f32" | "i32" | "u32" | "u8" => tests.push(format!("if (typeof x === \"number\") return {i};")),
            "bool" => tests.push(format!("if (typeof x === \"boolean\") return {i};")),
            "str" => tests.push(format!("if (typeof x === \"string\") return {i};")),
            "enum" => {
                let e = model.enums.get(t.of.as_ref().unwrap()).unwrap();
                tests.push(format!("if ({}.has(x)) return {i};", js_enum_index(e)));
            }
            "vec3" => tests.push(format!("if (x?.isVector3) return {i};")),
            "quat" => tests.push(format!("if (x?.isQuaternion) return {i};")),
            "mat4" => tests.push(format!("if (x?.isMatrix4) return {i};")),
            "f64xN" => tests.push(format!("if (Array.isArray(x) && x.length === {}) return {i};", t.n)),
            _ => {}
        }
    }
    for &(v, i) in &recs {
        let r_name = v.ty.as_ref().unwrap().of.as_ref().unwrap();
        if !model.records.get(r_name).unwrap().classes.is_empty() { tests.push(format!("if (b?.recordTests?.{r_name}?.(x)) return {i};")); }
    }
    let several = refs
        .iter()
        .filter(|(v, _)| model.kinds.get(v.kind.as_ref().unwrap()).unwrap().stored)
        .count()
        > 1;
    let rank = |v: &UnionVariant| -> usize {
        let kd = model.kinds.get(v.kind.as_ref().unwrap()).unwrap();
        if kd.stored { 0 } else if !kd.classes.is_empty() { 1 } else { 2 }
    };
    let mut tested = refs.clone();
    if several { tested.sort_by_key(|(v, _)| rank(v)); }
    for &(v, i) in &tested {
        let k_name = v.kind.as_ref().unwrap();
        if several || !model.kinds.get(k_name).unwrap().stored { tests.push(format!("if (b?.refs[{}].is(x)) return {i};", kind_const(k_name))); }
    }
    let rest = if several { None } else { refs.iter() .copied() .find(|(v, _)| model.kinds.get(v.kind.as_ref().unwrap()).unwrap().stored) }
    .or_else(|| {
        recs.iter().copied().find(|(v, _)| { model .records .get(v.ty.as_ref().unwrap().of.as_ref().unwrap()) .unwrap() .classes .is_empty() })
    });
    if let Some((_, idx)) = rest { tests.push(format!("if (x !== null && typeof x === \"object\") return {idx};")); }

    // Emit `variantOf<Union>(b, x)` to classify a live JS value `x` into its
    // 0-based union variant tag using `null`/`undefined` checks, `typeof` tests,
    // record `$class` / tag discrimination, and `b.kindOf(x)` handle lookup.
    lines.push(format!("export function variantOf{}(b, x) {{", u.name));
    for t in tests { lines.push(format!("  {t}")); }
    lines.extend([
        "  return -1;".to_string(),
        "}".to_string(),
        String::new(),
        format!("// Which of {}'s variants a value is.", u.name),
        format!("export function which{}(b, x) {{", u.name),
        format!("  const i = variantOf{}(b, x);", u.name),
        format!("  if (i < 0) throw Error(`{}: ${{x?.constructor?.name ?? x}} is none of its variants`);", u.name),
        "  return i;".to_string(),
        "}".to_string(),
        String::new(),
    ]);

    let w = union_words(model, u);
    lines.extend([
        "// An export row's union (from `at`) as a value: a vector or record the field held, if it is of the variant, read into in place.".to_string(),
        format!("export function sync{}(b, f, at, h, current) {{", u.name),
        "  switch (f[at]) {".to_string(),
    ]);
    for (i, v) in u.variants.iter().enumerate() {
        match v.tag.as_str() {
            "null" => lines.push(format!("    case {i}: return null;")),
            "undefined" => lines.push(format!("    case {i}: return undefined;")),
            "lit" => lines.push(format!("    case {i}: return {};", quote_str(v.value.as_ref().unwrap()))),
            "ref" => lines.push(format!("    case {i}: return b.refs[{}].at(f[at + 1], f[at + 2]);", kind_const(v.kind.as_ref().unwrap()))),
            _ => lines.push(format!(
                "    case {i}: {{ let v = variantOf{}(b, current) === {i} ? current : undefined; {} return v; }}",
                u.name,
                js_sync(model, v.ty.as_ref().unwrap(), "v", 1)
            )),
        }
    }
    lines.extend([
        format!("    default: throw Error('{}: no such variant');", u.name),
        "  }".to_string(),
        "}".to_string(),
        String::new(),
        format!("export function put{}(b, x, f, at, heap) {{", u.name),
        format!("  const i = which{}(b, x);", u.name),
        format!("  {}", js_zero("f", "at", 1, w)),
        "  f[at] = i;".to_string(),
        "  switch (i) {".to_string(),
    ]);
    for &(v, i) in &refs { lines.push(format!("    case {i}: b.refs[{}].put(x, f, at + 1); break;", kind_const(v.kind.as_ref().unwrap()))); }
    for &(v, i) in &values { lines.push(format!("    case {i}: {{ {} break; }}", js_put(model, v.ty.as_ref().unwrap(), "x", 1))); }
    lines.extend([
        "  }".to_string(),
        "}".to_string(),
        String::new(),
        format!("export function same{}(x, y) {{", u.name),
        "  if (Object.is(x, y)) return true;".to_string(),
        "  if (x === null || y === null || x === undefined || y === undefined) return false;".to_string(),
    ]);
    for &(v, _) in &values {
        let t = v.ty.as_ref().unwrap();
        match t.base.as_str() {
            "f64" | "f32" | "i32" | "u32" | "u8" | "bool" | "str" | "enum" => continue,
            "vec3" => lines.push("  if (x.isVector3 && y.isVector3) return sameVec(x, y);".to_string()),
            "quat" => lines.push("  if (x.isQuaternion && y.isQuaternion) return sameQuat(x, y);".to_string()),
            "mat4" => lines.push("  if (x.isMatrix4 && y.isMatrix4) return sameArray(x.elements, y.elements, 16);".to_string()),
            "f64xN" => lines.push(format!("  if (Array.isArray(x) && Array.isArray(y)) return sameArray(x, y, {});", t.n)),
            "rec" => lines.push(format!(
                "  if (typeof x === \"object\" && typeof y === \"object\" && same{}(x, y) === null) return true;",
                t.of.as_ref().unwrap()
            )),
            _ => {}
        }
    }
    lines.extend([
        "  return false;".to_string(),
        "}".to_string(),
        String::new(),
        format!("export function export{}(x, cx) {{", u.name),
        format!("  switch (which{}(cx, x)) {{", u.name),
    ]);
    for (i, v) in u.variants.iter().enumerate() {
        let out = match v.tag.as_str() {
            "null" => "null".to_string(),
            "undefined" => "undefined".to_string(),
            "lit" => quote_str(v.value.as_ref().unwrap()),
            "ref" => format!("cx.ref({}, x)", quote_str(v.kind.as_ref().unwrap())),
            _ => {
                let vt = v.ty.as_ref().unwrap();
                if vt.base == "rec" {
                    let of = vt.of.as_ref().unwrap();
                    if tagged {
                        format!("Object.assign({{ $record: {} }}, export{of}(x, cx))", quote_str(of))
                    } else {
                        format!("export{of}(x, cx)")
                    }
                } else {
                    js_export(model, vt, "x")
                }
            }
        };
        lines.push(format!("    case {i}: return {out};"));
    }
    lines.extend(["  }".to_string(), "}".to_string(), String::new()]);
    lines
}

// Emits JS row stride constants and `mirror_<kind>`, `sync_<kind>`,
// `sync_full_<kind>`, `put_imports_<kind>`, `put_full_<kind>`, `check_<kind>`,
// and `export_<kind>` functions for store kind `k`. Mirrors `jsKind(model, k)`
// in `tools/sim-schema.mjs`.
// Complete JS kind codegen (`jsKind(model, k)` in `tools/sim-schema.mjs`):
//   - Emits `<Name>Mirror` class, `<NAME>_KEYS` (Set of mirrored + skipped property
//     names allowed on the live JS object), and `<NAME>_SKIP` (Map of skipped property
//     names to their documented reasons).
//   - Emits `sync<Name>` (export row -> mirror), optional `syncDelta<Name>`,
//     optional `syncHot<Name>` / `syncCold<Name>` (when `$defer` is set),
//     `syncFull<Name>`, `put<Name>` (mirror -> import row), `putFull<Name>`,
//     `check<Name>` (detecting illegal direct JS writes to Rust-owned export fields),
//     `same<Name>`, and `export<Name>`.
fn js_kind(model: &Model, k: &KindDef) -> Vec<String> {
    let ex = layout(model, k, "export");
    let im = layout(model, k, "import");
    let full = layout(model, k, "full");
    let u_name = upper(&k.name);
    let facets_str: Vec<String> = k
        .facets
        .iter()
        .map(|f| format!("{}/{f}.mjs", model.target.schema))
        .collect();
    let mut lines = vec![format!("// {}: from {}.", k.name, facets_str.join(", "))];
    let mirrored_fields: Vec<Field> = k.fields.iter().filter(|f| f.mirror).cloned().collect();
    lines.extend(js_mirror(model, &format!("{}Mirror", k.rust), &mirrored_fields, !k.singleton));

    let mut own_keys = Vec::new();
    for f in &k.fields { if !own_keys.contains(&f.js[0]) { own_keys.push(f.js[0].clone()); } }
    for (sk, _) in &k.skip { if !own_keys.contains(sk) { own_keys.push(sk.clone()); } }
    let quoted_keys: Vec<String> = own_keys.iter().map(|n| quote_str(n)).collect();
    lines.push(format!("export const {u_name}_KEYS = new Set([{}]);", quoted_keys.join(", ")));
    lines.push(format!(
        "export const {u_name}_EXPORT_STRIDE = {}, {u_name}_IMPORT_STRIDE = {}, {u_name}_FULL_STRIDE = {};",
        ex.stride, im.stride, full.stride
    ));
    lines.push(String::new());
    lines.push("// An export row (from `at`; the heap from `h`) into mirror `m`.".to_string());
    lines.extend(js_sync_fn(model, &format!("sync{}", k.rust), &ex.fields, "m"));

    // For kinds with `$delta: true`, synthesize a pseudo-record over the export
    // layout so `js_delta_root` emits `syncDelta<Kind>` for incremental row updates.
    if k.delta {
        let synthetic_rec = RecordDef {
            name: k.rust.clone(),
            file: String::new(),
            observe: Vec::new(),
            layout: false,
            publish: Vec::new(),
            refresh: Vec::new(),
            delta: ex.fields.iter().map(|p| p.field.name.clone()).collect(),
            view: false,
            reuse: false,
            parts: Vec::new(),
            classes: Vec::new(),
            fields: ex.fields.iter().map(|p| p.field.clone()).collect(),
            defer: k.defer.clone(),
        };
        lines.extend(js_delta_root(model, &synthetic_rec, &ex.fields, ex.stride));
    }
    // For kinds with `$defer: [...]`, emit `syncHot<Kind>` (syncing only non-deferred
    // export fields every tick while marking deferred dirty bits) and `syncCold<Kind>`
    // (syncing the deferred fields on demand when read).
    if let Some(defer) = &k.defer {
        if !defer.is_empty() {
            let fields: Vec<&PlacedField> = ex.fields.iter().filter(|p| defer.contains(&p.field.name)).collect();
            lines.push(format!("export function sync{}Deferred(b, f, at, h, m) {{", k.rust));
            for p in &fields {
                for line in js_sync_observed(model, &p.field.ty, &js_set(&p.field, "m"), p.at) { lines.push(format!("  {line}")); }
            }
            lines.extend([
                "}".to_string(),
                String::new(),
                format!("export function identity{}Deferred(m) {{ return {}; }}", k.rust, js_set(&fields[0].field, "m")),
                String::new(),
                format!("export function changed{}Deferred(f, at, h, d, base) {{", k.rust),
                format!("  return {};", fields .iter() .map(|p| js_delta_test(model, &p.field.ty, p.at)) .collect::<Vec<_>>() .join(" || ")),
                "}".to_string(),
                String::new(),
                format!("export function mark{}Deferred(f, at, h, d, base) {{", k.rust),
            ]);
            for p in &fields { for line in js_mark_observed(model, &p.field.ty, p.at) { lines.push(format!("  {line}")); } }
            lines.extend([
                "}".to_string(),
                String::new(),
                format!("export function removed{}Deferred(f, at, m) {{", k.rust),
                format!(
                    "  return {};",
                    fields
                        .iter()
                        .map(|p| format!("{} != null && f[at + {}] !== 0", js_set(&p.field, "m"), p.at))
                        .collect::<Vec<_>>()
                        .join(" || ")
                ),
                "}".to_string(),
                String::new(),
            ]);
        }
    }

    lines.push("// A full row into object `m`.".to_string());
    lines.extend(js_sync_fn(model, &format!("syncFull{}", k.rust), &full.fields, "m"));
    lines.push("// Object `o`'s import fields into a row (lists to `heap`).".to_string());
    lines.extend(js_put_fn(model, &format!("put{}", k.rust), &im.fields, false));
    lines.push("// Every field of object `o` into a row.".to_string());
    lines.extend(js_put_fn(model, &format!("putFull{}", k.rust), &full.fields, false));
    lines.extend([
        "// The first mirrored field of `m` that is not what the export row says (a".to_string(),
        "// write JavaScript made to Rust's state without a command), or null.".to_string(),
        format!("export function check{}(b, f, at, h, m) {{", k.rust),
        format!("  const want = sync{}(b, f, at, h, new {}Mirror());", k.rust, k.rust),
    ]);
    for PlacedField { field, .. } in &ex.fields {
        lines.push(format!(
            "  {}",
            js_same_field(
                model,
                &field.ty,
                &format!("m{}", js_key(&field.name)),
                &format!("want{}", js_key(&field.name)),
                &format!(".{}", field.name)
            )
        ));
    }
    lines.extend(["  return null;".to_string(), "}".to_string(), String::new()]);
    lines.extend(js_same_fn(model, &k.rust, &k.fields));
    lines.extend(js_export_fn(model, &k.rust, &k.fields, Some(k)));
    lines
}

// Generates the complete `<file>.js` module for one schema file, scanning the
// generated body to import only the needed runtime helpers from `runtime.js`
// and cross-file record/union/enum symbols in deterministic declaration order.
// Mirrors `jsFile(model, file)` in `tools/sim-schema.mjs`.
// Per-schema-file ES module generator (`jsFile(model, file)` in `tools/sim-schema.mjs`):
//   - Emits imports in a strict canonical order so the output is byte-identical:
//     1. Runtime helpers from `runtime.js` (`f32`, `readList`, `writeList`, `sameVec`, etc.),
//        filtered to only those identifiers referenced in the generated body and
//        ordered by the fixed runtime symbol list.
//     2. `KIND_*` constants from `./kinds.js` when any handle/set/map-ref field is used.
//     3. Cross-file record/union/enum symbols (`./<other>.js`) in `BTreeMap` file
//        and symbol order.
//     4. `THREE` namespace import when `Vector3`, `Quaternion`, or `Matrix4` is constructed.
//   - Followed by constants, enums, unions, records, and kinds declared in `file`.
pub fn js_file(model: &Model, file: &str) -> String {
    let helpers_set = heap_helpers(model);
    // Dependency analysis pass for `<file>.js` (`jsFile` in `tools/sim-schema.mjs`):
    //   - Walks every record, kind, and union declared in `file` via `need(ty, field)`
    //     to discover which helper functions from `runtime.js` (`heap_read`,
    //     `heap_write`, `compare`, `delta_used`), which `KIND_*` constants from
    //     `./kinds.js`, and which peer record/union/enum symbols from `./<other>.js`
    //     must be imported at the top of the module.
    //   - After generating the module body (`joined`), filters each candidate
    //     import list with `has_ident(&joined, sym)` so unused helpers are never
    //     emitted in the `import { ... } from` header.
    // Scan every record, kind, and union declared in `file` to determine which
    // cross-file symbols (`uses`), kind code constants (`codes`), and `runtime.js`
    // helpers (`compare`, `heap_read`, `heap_write`, `delta_used`) are referenced,
    // so the generated ES module imports only what it actually uses.
    let mut uses: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut use_sym = |name: String, from: &str| { if from != file { uses.entry(from.to_string()).or_default().insert(name); } };
    let mut codes: BTreeSet<String> = BTreeSet::new();
    let mut compare: BTreeSet<String> = ["sameArray", "sameEach", "sameList", "sameMap", "sameQuat", "sameVec"]
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    if model
        .kind_list
        .iter()
        .any(|k| k.file.as_deref() == Some(file) && k.defer.as_ref().map(|d| !d.is_empty()).unwrap_or(false))
    { compare.insert("markDelta".to_string()); }
    if model
        .records
        .values()
        .any(|r| r.file == file && (!r.delta.is_empty() || helpers_set.contains(&r.name)))
        || model
            .kinds
            .values()
            .any(|k| k.facets.iter().any(|f| f == file) && k.delta)
    { compare.insert("deltaChanged".to_string()); }

    let mut need = |t0: &TypeSpec, t0_field: Option<&Field>| {
        if t0.base == "obj" { compare.insert("sameObject".to_string()); }
        if t0.base == "typed" { compare.insert("sameTyped".to_string()); }
        if let Some(key) = &t0.key {
            if key.base == "ref" {
                let k_of = key.of.as_ref().unwrap();
                if model.kinds.has(k_of) {
                    codes.insert(kind_const(k_of));
                } else {
                    let u = model.unions.get(k_of).unwrap();
                    for p in ["sync", "put", "export"] { use_sym(format!("{p}{}", u.name), &u.file); }
                }
            }
        }
        let t = if container(t0) { t0.value.as_ref().unwrap() } else { t0 };
        if t.base == "enum" {
            let e = model.enums.get(t.of.as_ref().unwrap()).unwrap();
            use_sym(js_enum(e), &e.file);
            use_sym(js_enum_index(e), &e.file);
        }
        if t.base == "union" {
            let u = model.unions.get(t.of.as_ref().unwrap()).unwrap();
            for p in ["sync", "put", "export", "same"] { use_sym(format!("{p}{}", u.name), &u.file); }
        }
        if t.base == "rec" {
            let r = model.records.get(t.of.as_ref().unwrap()).unwrap();
            for p in ["sync", "put", "same", "export"] { use_sym(format!("{p}{}", r.name), &r.file); }
            use_sym(format!("{}Mirror", r.name), &r.file);
            if !r.delta.is_empty() { use_sym(format!("sync{}Delta", r.name), &r.file); }
            if helpers_set.contains(&r.name) { use_sym(format!("changed{}HeapList", r.name), &r.file); }
        }
        if matches!(t.base.as_str(), "ref" | "set") || (t0.base == "list" && t.base == "ref") {
            codes.insert(kind_const(t.of.as_ref().unwrap()));
        }
        if t0.base == "list" && t.base == "ref" {
            if let Some(f) = t0_field {
                let target_k = model.kinds.get(t.of.as_ref().unwrap()).unwrap();
                if let (Some(hk), Some(hf)) = (&target_k.home_kind_name, &target_k.home_field_name) {
                    if let Some(owner) = model.kinds.get(hk) {
                        if let Some(owner_f) = owner.fields.iter().find(|of| &of.name == hf) {
                            if owner_f.where_loc == f.where_loc && owner_f.name == f.name {
                                use_sym(format!("export{}", target_k.rust), target_k.file.as_ref().unwrap());
                            }
                        }
                    }
                }
            }
        }
    };

    for r in model.records.values() { if r.file == file { for f in &r.fields { need(&f.ty, Some(f)); } } }
    for k in &model.kind_list { if k.file.as_deref() == Some(file) { for f in &k.fields { need(&f.ty, Some(f)); } } }
    for u in model.unions.values() { if u.file == file { for v in &u.variants { if v.tag == "value" { need(v.ty.as_ref().unwrap(), None); } } } }
    drop(need);
    for u in model.unions.values() {
        if u.file == file { for v in &u.variants { if v.tag == "ref" { codes.insert(kind_const(v.kind.as_ref().unwrap())); } } }
    }

    let compare_list: Vec<String> = compare.into_iter().collect();
    let mut lines = vec![
        format!("// Generated by tools/sim-schema.mjs from {}/{file}.mjs; do not edit.", model.target.schema),
        "import * as THREE from \"three\";".to_string(),
        format!("import {{ {} }} from \"{}\";", compare_list.join(", "), model.target.compare),
    ];
    if !codes.is_empty() {
        let codes_list: Vec<String> = codes.into_iter().collect();
        lines.push(format!("import {{ {} }} from \"./kinds.js\";", codes_list.join(", ")));
    }
    for (from, names) in uses {
        let names_list: Vec<String> = names.into_iter().collect();
        lines.push(format!("import {{ {} }} from \"./{from}.js\";", names_list.join(", ")));
    }
    if model
        .records
        .values()
        .any(|r| r.file == file && !r.observe.is_empty())
    { lines.push("import { observedFields, observeRecord } from \"../observed.js\";".to_string()); }
    lines.push(String::new());

    for c in model.constants.values() { if c.file == file { lines.push(format!("export const {} = {};", c.name, c.value)); } }
    for e in model.enums.values() {
        if e.file != file { continue; }
        let vals_json = JsonValue::Array(e.values.iter().map(|v| JsonValue::String(v.clone())).collect()).to_json_compact();
        let val_obj_entries: Vec<(String, JsonValue)> = e
            .values
            .iter()
            .enumerate()
            .map(|(i, name)| (name.clone(), JsonValue::Number(i as f64)))
            .collect();
        let val_obj_json = JsonValue::Object(val_obj_entries).to_json_compact();
        lines.extend([
            format!("export const {} = {vals_json};", js_enum(e)),
            format!("export const {} = new Map({}.map((name, i) => [name, i]));", js_enum_index(e), js_enum(e)),
            format!("export const {}_VALUE = Object.freeze({val_obj_json});", e.name),
            String::new(),
        ]);
    }
    for u in model.unions.values() { if u.file == file { lines.extend(js_union(model, u)); } }
    for r in model.records.values() {
        if r.file == file {
            if !r.observe.is_empty() {
                let obs_keys: Vec<JsonValue> = r
                    .observe
                    .iter()
                    .map(|name| { let f = r.fields.iter().find(|f| &f.name == name).unwrap(); JsonValue::String(f.js[0].clone()) })
                    .collect();
                lines.push(format!("const {}ObservedFields = observedFields({});", r.name, JsonValue::Array(obs_keys).to_json_compact()));
                lines.push(String::new());
            }
            lines.extend(js_record(model, r));
            let rec_ty = TypeSpec {
                text: String::new(),
                nullable: Nullable::default(),
                base: "rec".to_string(),
                of: Some(r.name.clone()),
                value: None,
                key: None,
                n: 0,
                class: None,
                elem: None,
            };
            if observed_type(model, &rec_ty, &mut HashSet::new()) { lines.extend(js_observed_reader(model, r)); }
            let in_list = model.records.values().any(|parent| {
                parent.fields.iter().any(|f| {
                    f.ty.base == "list"
                        && !optional(f.ty.value.as_ref().unwrap())
                        && f.ty.value.as_ref().unwrap().base == "rec"
                        && f.ty.value.as_ref().unwrap().of.as_deref() == Some(r.name.as_str())
                })
            });
            if in_list && observed_type(model, &rec_ty, &mut HashSet::new()) && observed_test(model, &rec_ty, 0) != "(false)" {
                let test = observed_test(model, &rec_ty, 0);
                lines.extend([
                    format!("export function changed{}ObservedList(f, at, n, h, d, base, stride) {{", r.name),
                    "  at |= 0; n |= 0; h |= 0;".to_string(),
                    "  for(let i=0;i<n;i++,at+=stride) {".to_string(),
                    format!("    if ({test}) return true;"),
                    "  }".to_string(),
                    "  return false;".to_string(),
                    "}".to_string(),
                    String::new(),
                ]);
            }
            if helpers_set.contains(&r.name) {
                let mut offset = 0usize;
                let mut tests = Vec::new();
                for f in &r.fields { tests.extend(js_delta_heap_tests(model, &f.ty, offset)); offset += words(model, &f.ty); }
                lines.extend([
                    format!("export function changed{}HeapList(f, at, n, h, d, base, stride, nullable) {{", r.name),
                    "  for (let i=0; i<n; i++, at+=stride) {".to_string(),
                    format!("    if ((!nullable || f[at - 1] === 0) && ({})) return true;", tests.join(" || ")),
                    "  }".to_string(),
                    "  return false;".to_string(),
                    "}".to_string(),
                    String::new(),
                ]);
            }
        }
    }
    for k in &model.kind_list { if k.file.as_deref() == Some(file) { lines.extend(js_kind(model, k)); } }
    let joined = lines.join("\n");
    format!("{}\n", joined.trim_end_matches('\n'))
}

// Generates `kinds.js` (`src/sim/gen/kinds.js` or `tests/sim-gen/kinds.js`),
// exporting `LAYOUT_HASH`, `KIND_*` code constants, `KIND_NAMES`, and
// `KIND_BY_NAME`. Mirrors `jsKinds(model)` in `tools/sim-schema.mjs`.
// `kinds.js` generator (`jsKinds(model)` in `tools/sim-schema.mjs`):
//   - Emits `LAYOUT_HASH` as an unsigned 32-bit hex literal (`0x{:08x}`) so the
//     JS runtime can verify at startup that the loaded Wasm binary was compiled
//     against the exact same schema layout.
//   - Emits `KIND_<NAME>` (1..=KINDS), `COMMAND_<NAME>`, `EVENT_<NAME>`,
//     `IMPORT_<NAME>`, and `OP_<NAME>` numeric constants in sorted name order.
pub fn js_kinds(model: &Model) -> String {
    let mut lines = vec![
        format!("// Generated by tools/sim-schema.mjs from {}/; do not edit.", model.target.schema),
        "// Each kind's code (its place among the kinds' names, from 1).".to_string(),
    ];
    for k in &model.kind_list { lines.push(format!("export const {} = {};", kind_const(&k.name), k.code)); }
    lines.push(format!("export const KINDS = {};", model.kind_list.len()));
    for c in &model.commands_list { lines.push(format!("export const COMMAND_{} = {};", upper(&c.name), c.code)); }
    for e in &model.events_list { lines.push(format!("export const EVENT_{} = {};", upper(&e.name), e.code)); }
    for i in &model.imports_list { lines.push(format!("export const IMPORT_{} = {};", upper(&i.name), i.code)); }
    let operations = if model.target.test { Vec::new() } else { operations_of(model) };
    for o in &operations { lines.push(format!("export const OPERATION_{} = {};", upper(&o.name), o.code)); }
    format!("{}\n", lines.join("\n"))
}

// Generates `imports.js` (`src/sim/gen/imports.js`), exporting `IMP_*` code
// constants and the `IMPORTS` descriptor table for Wasm host callbacks.
// Mirrors `jsImports(model)` in `tools/sim-schema.mjs`.
// `imports.js` generator (`jsImports(model)` in `tools/sim-schema.mjs`):
//   - Emits `IMPORT_SYMBOLS`, a 1-indexed array mapping each `IMPORT_<NAME>`
//     code (`1..=N`, with `null` at index 0) to its JS callback name so the Wasm
//     host bridge can dispatch synchronous import calls in O(1) by table lookup.
pub fn js_imports(model: &Model) -> String {
    let mut lines = vec![
        format!("// Generated by tools/sim-schema.mjs from {}/; do not edit.", model.target.schema),
        "// What the module imports from JavaScript, by code: the bridge serves each.".to_string(),
    ];
    let items: Vec<String> = model
        .imports_list
        .iter()
        .map(|c| {
            format!(
                "{{ name: {}, code: {}, symbol: {} }}",
                quote_str(&c.name),
                c.code,
                quote_str(&format!("bfe_import_{}", snake_bare(&c.name)))
            )
        })
        .collect();
    let tail = if items.is_empty() { String::new() } else { format!(", {}", items.join(", ")) };
    lines.push(format!("export const IMPORT_SYMBOLS = [null{tail}];"));
    format!("{}\n", lines.join("\n"))
}

// Generates `index.js` (`src/sim/gen/index.js` or `tests/sim-gen/index.js`),
// wiring every kind, union class table, command, event, and import into the
// runtime metadata tables consumed by `src/sim/runtime.js`. Mirrors
// `jsIndex(model)` in `tools/sim-schema.mjs`.
// Complete JS `index.js` barrel and runtime descriptor table (`jsIndex(model)`
// in `tools/sim-schema.mjs`):
//   - Imports per-kind/per-record functions from `./<file>.js` and re-exports
//     `./kinds.js` and every `./<file>.js` module.
//   - Exports `KINDS_META` (indexed by 1-based `KIND_*` code) containing layout
//     strides, key/home metadata, and function pointers (`mirror`, `sync`, `put`, etc.).
//   - Exports `REC_CLASSES` (for union class discrimination), `COMMANDS`,
//     `OPERATIONS`, `EVENTS`, `IMPORTS`, and `DEF_TABLES` for `src/sim/runtime.js`.
pub fn js_index(model: &Model) -> String {
    let mut lines = vec![
        format!("// Generated by tools/sim-schema.mjs from {}/; do not edit.", model.target.schema),
        "// Every kind: its code, name, layout and generated functions, for the".to_string(),
        "// store's runtime (src/sim/runtime.js) to dispatch on (docs/sim-store.md).".to_string(),
    ];
    let operations = if model.target.test { Vec::new() } else { operations_of(model) };
    let mut call_records: Vec<String> = Vec::new();
    let mut add_rec = |r: &str| { if !call_records.iter().any(|x| x == r) { call_records.push(r.to_string()); } };
    for c in &model.commands_list { add_rec(&c.args); if let Some(res) = &c.result { add_rec(res); } }
    for e in &model.events_list { add_rec(&e.record); }
    for c in &model.imports_list { add_rec(&c.args); if let Some(res) = &c.result { add_rec(res); } }
    for o in &operations { if let Some(rec) = &o.record { add_rec(rec); } }

    for f in &model.files {
        let mut names = Vec::new();
        for k in &model.kind_list {
            if k.file.as_deref() == Some(f.as_str()) {
                names.extend([
                    format!("{}Mirror", k.rust),
                    format!("sync{}", k.rust),
                    format!("syncFull{}", k.rust),
                    format!("put{}", k.rust),
                    format!("putFull{}", k.rust),
                    format!("check{}", k.rust),
                    format!("same{}", k.rust),
                    format!("export{}", k.rust),
                    format!("{}_KEYS", upper(&k.name)),
                ]);
            }
        }
        for r in &call_records {
            if model.records.get(r).unwrap().file == *f { names.extend([format!("{r}Mirror"), format!("sync{r}"), format!("put{r}")]); }
        }
        if !names.is_empty() { lines.push(format!("import {{ {} }} from \"./{f}.js\";", names.join(", "))); }
    }
    lines.push("export * from \"./kinds.js\";".to_string());
    for f in &model.files { lines.push(format!("export * from \"./{f}.js\";")); }
    lines.extend([
        String::new(),
        "// By code: name, layout hash (the same as the module's), singleton, key (the".to_string(),
        "// numeric field that names an object), home (singleton kind and list field),".to_string(),
        "// whether the store holds fields of it, row strides, and its functions.".to_string(),
        "export const KIND_TABLE = [null,".to_string(),
    ]);
    for k in &model.kind_list {
        let key_json = match &k.key { Some(key) => quote_str(key), None => "null".to_string(), };
        let home_json = match (&k.home_kind_code, &k.home_field_name) {
            (Some(code), Some(field)) => format!("{{ kind: {code}, field: {} }}", quote_str(field)),
            _ => "null".to_string(),
        };
        let classes_json = JsonValue::Array(k.classes.iter().map(|c| JsonValue::String(c.clone())).collect()).to_json_compact();
        let base = format!(
            "name: {}, code: {}, hash: 0x{:08x}, singleton: {}, key: {key_json}, home: {home_json}, stored: {}, classes: {classes_json}",
            quote_str(&k.name),
            k.code,
            k.hash,
            k.singleton,
            k.stored
        );
        if !k.stored { lines.push(format!("  {{ {base} }},")); continue; }
        let ex_s = layout(model, k, "export").stride;
        let im_s = layout(model, k, "import").stride;
        let full_s = layout(model, k, "full").stride;
        let r = &k.rust;
        let u = upper(&k.name);
        lines.push(format!(
            "  {{ {base}, exportStride: {ex_s}, importStride: {im_s}, fullStride: {full_s}, Mirror: {r}Mirror, sync: sync{r}, syncFull: syncFull{r}, put: put{r}, putFull: putFull{r}, check: check{r}, same: same{r}, export: export{r}, keys: {u}_KEYS }},"
        ));
    }
    lines.push("];".to_string());
    lines.push(String::new());

    let rec_classes_entries: Vec<(String, JsonValue)> = model
        .records
        .values()
        .filter(|r| !r.classes.is_empty())
        .map(|r| { (r.name.clone(), JsonValue::Array(r.classes.iter().map(|c| JsonValue::String(c.clone())).collect()),) })
        .collect();
    lines.extend([
        "// The records a union tells apart by class: their classes.".to_string(),
        format!("export const RECORD_CLASSES = {};", JsonValue::Object(rec_classes_entries).to_json_compact()),
        String::new(),
    ]);

    let fns_of = |r: &str| -> String {
        let w = record_words(model, r);
        format!("{{ words: {w}, Mirror: {r}Mirror, sync: sync{r}, put: put{r} }}")
    };
    lines.push("// Commands, events and imports by code: name, and their records' words and functions.".to_string());
    let cmd_entries: Vec<String> = model
        .commands_list
        .iter()
        .map(|c| {
            let res = c.result.as_ref().map(|r| fns_of(r)).unwrap_or_else(|| "null".to_string());
            let bulk = if c.detached && !model.target.test {
                quote_str(&format!("cmd_{}_block", snake_bare(&c.name)))
            } else {
                "null".to_string()
            };
            format!("{{ name: {}, code: {}, args: {}, result: {res}, bulk: {bulk} }}", quote_str(&c.name), c.code, fns_of(&c.args))
        })
        .collect();
    lines.push(format!(
        "export const COMMAND_TABLE = [null{}{}{}];",
        if cmd_entries.is_empty() { "" } else { ", " },
        cmd_entries.join(", "),
        ""
    ));

    let op_entries: Vec<String> = operations
        .iter()
        .map(|o| {
            let scalars_json = JsonValue::Array(o.scalars.iter().map(|s| JsonValue::String(s.to_string())).collect()).to_json_compact();
            let rec = o.record.as_ref().map(|r| fns_of(r)).unwrap_or_else(|| "null".to_string());
            let ans = o.answers != OpAnswers::None;
            format!(
                "{{ name: {}, code: {}, scalars: {scalars_json}, record: {rec}, heap: {}, answers: {ans} }}",
                quote_str(&o.name),
                o.code,
                o.heap
            )
        })
        .collect();
    lines.push(format!("export const OPERATION_TABLE = [null, {}];", op_entries.join(", ")));

    let ev_entries: Vec<String> = model
        .events_list
        .iter()
        .map(|e| format!("{{ name: {}, code: {}, record: {} }}", quote_str(&e.name), e.code, fns_of(&e.record)))
        .collect();
    lines.push(format!("export const EVENT_TABLE = [null{}{}];", if ev_entries.is_empty() { "" } else { ", " }, ev_entries.join(", ")));

    let imp_entries: Vec<String> = model
        .imports_list
        .iter()
        .map(|c| {
            let res = c.result.as_ref().map(|r| fns_of(r)).unwrap_or_else(|| "null".to_string());
            format!(
                "{{ name: {}, code: {}, symbol: {}, args: {}, result: {res} }}",
                quote_str(&c.name),
                c.code,
                quote_str(&format!("bfe_import_{}", snake_bare(&c.name))),
                fns_of(&c.args)
            )
        })
        .collect();
    lines.push(format!("export const IMPORT_TABLE = [null{}{}];", if imp_entries.is_empty() { "" } else { ", " }, imp_entries.join(", ")));
    lines.push(String::new());

    let mut def_tables: BTreeSet<String> = BTreeSet::new();
    for k in &model.kind_list { for f in &k.fields { if f.ty.base == "def" { def_tables.insert(f.ty.of.clone().unwrap()); } } }
    for r in model.records.values() { for f in &r.fields { if f.ty.base == "def" { def_tables.insert(f.ty.of.clone().unwrap()); } } }
    let def_arr: Vec<JsonValue> = def_tables.into_iter().map(JsonValue::String).collect();
    lines.push(format!("export const DEF_TABLES = {};", JsonValue::Array(def_arr).to_json_compact()));

    format!("{}\n", lines.join("\n"))
}

// Compiles both `TARGETS` (`rust/sim/schema` and `rust/sim/schema-test`) and
// produces the complete list of `(output_path, content)` pairs for all Rust and
// JS generated files, including the derived HUD layout files
// (`rust/bfe-render/src/hud_model/layout.rs`, `src/hud-layout.js`) and native
// client input layout files (`rust/bfe-client/src/generated_input.rs`,
// `src/sim/gen/client-input.js`). Mirrors the main loop of `tools/sim-schema.mjs`.
// Top-level multi-target generator (`main` in `tools/sim-schema.mjs`):
//   - Evaluates all `.mjs` files in `rust/sim/schema/` and `rust/sim/schema-test/`
//     using `schema_eval::load_schemas`.
//   - For each target, generates:
//     * `<rust_dir>/<file>.rs` for each non-`layoutOnly` schema file
//     * `<rust_dir>/mod.rs` and (for the production store) `<rust_dir>/replay.rs`
//     * `<js_dir>/<file>.js`, `<js_dir>/kinds.js`, `<js_dir>/imports.js` (main only),
//       and `<js_dir>/index.js`
//   - For the production target, also generates the four derived layout files:
//     * `rust/bfe-client/src/generated_input.rs`
//     * `src/sim/gen/client-input.js`
//     * `rust/bfe-render/src/hud_model/layout.rs`
//     * `src/hud-layout.js`
pub fn generate_all(root: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    let mut set_out = |path: PathBuf, content: String| {
        if let Some(existing) = out.iter_mut().find(|(p, _)| p == &path) { existing.1 = content; } else { out.push((path, content)); }
    };

    // Compile each `Target` (`rust/sim/schema` and `rust/sim/schema-test`) and
    // emit its per-file Rust modules, `mod.rs`, `replay.rs` (for the main store),
    // per-file JS modules, `kinds.js`, `imports.js`, and `index.js`. For the main
    // store target, also emit the derived client input and HUD layout files.
    for target in TARGETS {
        let schemas = load_schemas(&root.join(target.schema), true)?;
        let non_layout: Vec<_> = schemas
            .iter()
            .filter(|s| !s.layout_only)
            .map(|s| crate::schema_eval::LoadedSchema { file: s.file.clone(), schema: s.schema.clone(), layout_only: s.layout_only, })
            .collect();
        let model = build_model(&non_layout, target.clone())?;
        let rust_dir = root.join(target.rust);
        let js_dir = root.join(target.js);

        set_out(rust_dir.join("mod.rs"), rust_index(&model));
        set_out(rust_dir.join("replay.rs"), rust_replay(&model));
        for f in &model.files { set_out(rust_dir.join(format!("{}.rs", snake(f))), rust_file(&model, f)); }
        set_out(js_dir.join("kinds.js"), js_kinds(&model));
        set_out(js_dir.join("imports.js"), js_imports(&model));
        set_out(js_dir.join("index.js"), js_index(&model));
        for f in &model.files { set_out(js_dir.join(format!("{f}.js")), js_file(&model, f)); }

        // Derived cross-crate layout generation (`tools/sim-schema.mjs` bottom section):
        //
        // In addition to the simulation store modules under `rust/sim/src/gen/`
        // and `src/sim/gen/`, two other subsystem boundaries derive their flat
        // `f64` word offset tables directly from `rust/sim/schema/`:
        //   - Native client input (`rust/bfe-client/src/generated_input.rs` and
        //     `src/sim/gen/client-input.js`) extracts word offsets for `ClientInput`,
        //     `ClientCommand`, `ClientLook`, and `ClientFlightCommand` from `client.mjs`.
        //   - HUD layout (`rust/bfe-render/src/hud_model/layout.rs` and `src/hud-layout.js`)
        //     collects every enum and `$layout: true` record across all schema files
        //     (primarily `hud.mjs` and `client.mjs`) and emits their `*_WORDS` strides
        //     and field offset constants for both Rust and browser HUD renderers.
        // For the production store target (`!target.test`), also derive:
        //   1. `rust/bfe-client/src/generated_input.rs` and `src/sim/gen/client-input.js`
        //      from `rust/sim/schema/client.mjs` (`ClientCommand`, `ClientLook`, `ClientFlightCommand`).
        //   2. `rust/bfe-render/src/hud_model/layout.rs` and `src/hud-layout.js`
        //      from all `$layout: true` records across the schema directory (`hud.mjs`).
        if !target.test {
            let mut native_input = vec![
                "// Generated by tools/sim-schema.mjs from rust/sim/schema/client.mjs; do not edit.".to_string(),
                "#![allow(clippy::all, unused_braces, unused_parens, unused_variables, dead_code)]".to_string(),
                "fn word_u32(value: f64) -> u32 { value as u32 }".to_string(),
            ];
            for name in [
                "ClientHeld",
                "ClientAction",
                "ClientCommandSource",
                "ClientFlightHeld",
                "ClientFlightAction",
            ] {
                native_input.extend(rust_enum(model.enums.get(name).unwrap()));
            }
            for name in ["ClientCommand", "ClientLook", "ClientFlightCommand"] {
                native_input.extend(rust_record(&model, model.records.get(name).unwrap()));
            }
            set_out(root.join("rust/bfe-client/src/generated_input.rs"), format!("{}\n", native_input.join("\n").trim_end()),);

            let layout_only_schemas: Vec<_> = schemas
                .iter()
                .filter(|s| s.layout_only)
                .map(|s| crate::schema_eval::LoadedSchema { file: s.file.clone(), schema: s.schema.clone(), layout_only: s.layout_only, })
                .collect();
            let layout_model = build_model(&layout_only_schemas, target.clone())?;
            let mut rs = vec![
                "// Generated by tools/sim-schema.mjs; do not edit.".to_string(),
                "#![allow(dead_code)]".to_string(),
            ];
            let mut browser = vec!["// Generated by tools/sim-schema.mjs; do not edit.".to_string()];
            for e in layout_model.enums.values() {
                if e.file == "hud" {
                    rs.push(format!("pub struct {};", e.name));
                    rs.push(format!("impl {} {{", e.name));
                    for (i, name) in e.values.iter().enumerate() { rs.push(format!("pub const {}: u32 = {i};", upper(name))); }
                    rs.push("}".to_string());
                    let b_fields: Vec<String> = e
                        .values
                        .iter()
                        .enumerate()
                        .map(|(i, name)| format!("{name}: {i}"))
                        .collect();
                    browser.push(format!("export const {} = Object.freeze({{ {} }});", e.name, b_fields.join(", ")));
                }
            }
            for r in layout_model.records.values() {
                if r.file == "hud" {
                    let mut at = 0usize;
                    let mut fields: Vec<(String, usize)> = Vec::new();
                    for field in &r.fields {
                        let offset = at;
                        at += words(&layout_model, &field.ty);
                        if field.ty.base == "list" {
                            fields.push((field.name.clone(), offset));
                            fields.push((format!("{}Length", field.name), offset + 1));
                        } else {
                            fields.push((field.name.clone(), offset));
                        }
                    }
                    rs.push(format!("pub struct {};", r.name));
                    rs.push(format!("impl {} {{", r.name));
                    rs.push(format!("pub const WORDS: usize = {at};"));
                    for (name, offset) in &fields { rs.push(format!("pub const {}: usize = {offset};", upper(name))); }
                    rs.push("}".to_string());
                    let b_fields: Vec<String> = fields.iter().map(|(n, o)| format!("{n}: {o}")).collect();
                    browser.push(format!("export const {} = Object.freeze({{ words: {at}, {} }});", r.name, b_fields.join(", ")));
                }
            }
            set_out(root.join("rust/bfe-render/src/hud_model/layout.rs"), format!("{}\n", rs.join("\n")),);
            set_out(root.join("src/hud-layout.js"), format!("{}\n", browser.join("\n")));
        }
    }
    Ok(out)
}

// Lists all `.mjs` files in `dir` sorted in UTF-16 code-unit order
// (`Array.prototype.sort()`) over filenames, matching `listed(dir)` in
// `tools/sim-schema.mjs`. Also verifies that no stale `.rs` or `.js` files
// remain in an output directory when `--check` runs.
// Deterministic directory listing (`listed(dir)` in `tools/sim-schema.mjs`):
//   - Collects all `.rs` and `.js` files in `dir` and sorts them by filename
//     using UTF-16 code-unit comparison (`encode_utf16().cmp(...)`), matching
//     JavaScript's `fs.readdirSync(dir).sort()`.
//   - Used after `generate_all` to verify that no stale generated files remain
//     in `rust/sim/src/gen`, `rust/sim/src/gen_test`, `src/sim/gen`, or
//     `tests/sim-gen`.
fn listed(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    v.sort();
    v
}

// Mirrors `main(args)` in tools/sim-schema.mjs.
// Entry point for `cargo xtask schema [--check]` (`main` in `tools/sim-schema.mjs`):
//   - Calls `generate_all(root)` to compile both production and test schema targets
//     in memory.
//   - Checks every generated file against disk and scans all output directories
//     for unexpected extra `.rs` / `.js` files.
//   - When `check` is true, fails with a newline-separated list of stale or extra
//     relative paths if anything differs, or prints:
//     `sim-schema: <N> generated files match the schemas.`
//   - When `check` is false, writes only the changed files and removes any stale
//     leftover files so build mtimes stay clean for Cargo incremental builds.
pub fn run_sim_schema(root: &Path, check: bool) -> Result<(), String> {
    let out = generate_all(root)?;
    let out_paths: HashSet<PathBuf> = out.iter().map(|(p, _)| p.clone()).collect();
    // Verification and atomic file update pass (`run_sim_schema`):
    //   - First checks every expected `(path, text)` pair in `out` against the
    //     existing file on disk; if a file is missing or its UTF-8 bytes differ,
    //     records its repo-relative path in `problems`.
    //   - Next scans every target output directory (`rust/sim/src/gen`,
    //     `rust/sim/src/gen_test`, `src/sim/gen`, `tests/sim-gen`) via `listed(&dir)`
    //     to detect orphaned `.rs` or `.js` files left behind by deleted schemas.
    //   - In `--check` mode, any entry in `problems` aborts with a non-zero exit;
    //     in write mode, only mismatched files are written and orphaned files are
    //     removed.
    // Compare every generated `(path, text)` pair against disk, and also scan
    // each target's Rust and JS output directories (`listed(&dir)`) to catch
    // stale leftover `.rs` or `.js` files from renamed/removed schema modules.
    let mut problems = Vec::new();

    for (path, text) in &out {
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        if check {
            if !path.exists() {
                problems.push(format!("{rel} is missing"));
            } else if fs::read_to_string(path).ok().as_ref() != Some(text) {
                problems.push(format!("{rel} differs from what the schemas make"));
            }
        } else {
            if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
            if !path.exists() || fs::read_to_string(path).ok().as_ref() != Some(text) { fs::write(path, text).map_err(|e| e.to_string())?; }
        }
    }

    for target in TARGETS {
        for dir in [root.join(target.rust), root.join(target.js)] {
            for path in listed(&dir) {
                if out_paths.contains(&path) { continue; }
                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                if check { problems.push(format!("{rel} is not made by the schemas")); } else { let _ = fs::remove_file(&path); }
            }
        }
    }

    if !problems.is_empty() {
        return Err(format!(
            "{}\n\nThe store's generated code does not match rust/sim/schema/. Run `node tools/sim-schema.mjs` and commit what it writes.",
            problems.join("\n")
        ));
    }
    if check { println!("The store's generated code matches its schemas."); } else { println!("wrote {} files", out.len()); }
    Ok(())
}
