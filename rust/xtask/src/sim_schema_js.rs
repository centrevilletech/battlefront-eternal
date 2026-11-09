// Simulation store schema compiler (part 2): Rust JSON replay harness (`replay.rs`),
// JavaScript mirror/sync/export/delta/observed codegen (`src/sim/gen/` and
// `tests/sim-gen/`), HUD and native client input layout emitters, and top-level
// file verification/writing (`run_sim_schema`).
//
// Direct port of the second half of the schema generator.
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
//      emitted in the exact fixed helper table order.
//   4. HUD layout (`rust/bfe-render/src/hud_model/layout.rs` and `src/hud-layout.js`)
//      and native input layout (`rust/bfe-client/src/generated_input.rs` and
//      `src/sim/gen/client-input.js`) derive their word offsets directly from the
//      compiled `hud` and `client` schema models.

use crate::json::quote_str;
use crate::schema_eval::load_schemas;
use crate::sim_schema::*;
use std::collections::HashSet;
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
// (`replayIn(model, t, x, p)`).
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
// `x` into a Rust struct field of type `t` (`replayInField`).
// When `home` is true on a singleton's home list/table, entities are allocated
// and populated in the store via their home collection.
// Nullable and home-list wrapper around `replay_in` (`replayInField` in
// the schema generator):
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
// format (`replayOut(model, t, s)`).
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
// `s` of type `t` into a `serde_json::Value` (`replayOutField`).
// Nullable and home-list wrapper around `replay_out` (`replayOutField` in
// the schema generator):
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
// record or kind struct (`replayStruct`).
// JSON replay struct codegen (`replayStruct`):
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
// union enum (`replayUnion(model, u)`).
// JSON replay union codegen (`replayUnion`):
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
    // Dispatch order in `from_value` mirrors `replayUnion`:
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
// `rustReplay(model)`.
// Complete `replay.rs` module generator (`rustReplay(model)`):
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

// Compiles both `TARGETS` (`rust/sim/schema` and `rust/sim/schema-test`) and
// produces the complete list of `(output_path, content)` pairs for all Rust
// generated files, including `rust/bfe-render/src/hud_model/layout.rs` and
// `rust/bfe-client/src/generated_input.rs`.
pub fn generate_all(root: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    let mut set_out = |path: PathBuf, content: String| {
        if let Some(existing) = out.iter_mut().find(|(p, _)| p == &path) {
            existing.1 = content;
        } else {
            out.push((path, content));
        }
    };

    for target in TARGETS {
        let schemas = load_schemas(&root.join(target.schema), true)?;
        let non_layout: Vec<_> = schemas
            .iter()
            .filter(|s| !s.layout_only)
            .map(|s| crate::schema_eval::LoadedSchema {
                file: s.file.clone(),
                schema: s.schema.clone(),
                layout_only: s.layout_only,
            })
            .collect();
        let model = build_model(&non_layout, target.clone())?;
        let rust_dir = root.join(target.rust);

        set_out(rust_dir.join("mod.rs"), rust_index(&model));
        set_out(rust_dir.join("replay.rs"), rust_replay(&model));
        for f in &model.files {
            set_out(rust_dir.join(format!("{}.rs", snake(f))), rust_file(&model, f));
        }

        if !target.test {
            let mut native_input = vec![
                "// Generated by tools/sim-schema.mjs from rust/sim/schema/client.mjs; do not edit."
                    .to_string(),
                "#![allow(clippy::all, unused_braces, unused_parens, unused_variables, dead_code)]"
                    .to_string(),
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
            set_out(
                root.join("rust/bfe-client/src/generated_input.rs"),
                format!("{}\n", native_input.join("\n").trim_end()),
            );

            let layout_only_schemas: Vec<_> = schemas
                .iter()
                .filter(|s| s.layout_only)
                .map(|s| crate::schema_eval::LoadedSchema {
                    file: s.file.clone(),
                    schema: s.schema.clone(),
                    layout_only: s.layout_only,
                })
                .collect();
            let layout_model = build_model(&layout_only_schemas, target.clone())?;
            let mut rs = vec![
                "// Generated by tools/sim-schema.mjs; do not edit.".to_string(),
                "#![allow(dead_code)]".to_string(),
            ];
            for e in layout_model.enums.values() {
                if e.file == "hud" {
                    rs.push(format!("pub struct {};", e.name));
                    rs.push(format!("impl {} {{", e.name));
                    for (i, name) in e.values.iter().enumerate() {
                        rs.push(format!("pub const {}: u32 = {i};", upper(name)));
                    }
                    rs.push("}".to_string());
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
                    for (name, offset) in &fields {
                        rs.push(format!("pub const {}: usize = {offset};", upper(name)));
                    }
                    rs.push("}".to_string());
                }
            }
            set_out(
                root.join("rust/bfe-render/src/hud_model/layout.rs"),
                format!("{}\n", rs.join("\n")),
            );
        }
    }
    Ok(out)
}

fn listed(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    v.sort();
    v
}

pub fn run_sim_schema(root: &Path, check: bool) -> Result<(), String> {
    let out = generate_all(root)?;
    let out_paths: HashSet<PathBuf> = out.iter().map(|(p, _)| p.clone()).collect();
    let mut problems = Vec::new();

    for (path, text) in &out {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if check {
            if !path.exists() {
                problems.push(format!("{rel} is missing"));
            } else if fs::read_to_string(path).ok().as_ref() != Some(text) {
                problems.push(format!("{rel} differs from what the schemas make"));
            }
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            if !path.exists() || fs::read_to_string(path).ok().as_ref() != Some(text) {
                fs::write(path, text).map_err(|e| e.to_string())?;
            }
        }
    }

    for target in TARGETS {
        let dir = root.join(target.rust);
        for path in listed(&dir) {
            if out_paths.contains(&path) {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if check {
                problems.push(format!("{rel} is not made by the schemas"));
            } else {
                let _ = fs::remove_file(&path);
            }
        }
    }

    if !problems.is_empty() {
        return Err(format!(
            "{}\n\nThe store's generated code does not match rust/sim/schema/. Run `cargo xtask schema` and commit what it writes.",
            problems.join("\n")
        ));
    }
    if check {
        println!("The store's generated code matches its schemas.");
    } else {
        println!("wrote {} files", out.len());
    }
    Ok(())
}
