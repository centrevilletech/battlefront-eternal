#![allow(dead_code)]
// Order-preserving JSON parser and V8-compatible JSON serializer for xtask.
//
// Why this module exists instead of using `serde_json::Value`:
// `rust/Cargo.lock` compiles `serde_json` without the `preserve_order` feature,
// so `serde_json::Map` is backed by a `BTreeMap` that sorts object keys
// lexicographically. In contrast, Node/V8 iterates object properties and emits
// `JSON.stringify` output in property insertion order (none of our schema or
// shader manifest objects use array-index keys). Using `BTreeMap` would
// scramble layout constant order, uniform binding indices, and generated JS
// object literals across `render-shaders`, `sim-schema`, and layout generators.
//
// Byte-identity rules matching V8's `JSON.parse` and `JSON.stringify`:
// 1. Insertion order: `JsonValue::Object` stores `Vec<(String, JsonValue)>`.
//    When an object literal or JSON input repeats a key, the existing slot's
//    value is updated in place without moving the key to the end.
// 2. Number formatting: IEEE-754 doubles with zero fractional part and
//    magnitude below 1e15 format as base-10 integers without a `.0` suffix
//    (`42.0` -> `"42"`, `-0.0` -> `"0"`), matching `Number.prototype.toString()`
//    in V8 for all integer constants and float thresholds used in schemas.
// 3. String escaping: `quote_str` delegates to `serde_json::to_string`, which
//    uses the same `\"`, `\\`, `\b`, `\f`, `\n`, `\r`, `\t`, and `\u00xx`
//    control-character escapes as `JSON.stringify`.
// 4. Compact mode (`to_json_compact`, matching `JSON.stringify(value)`):
//    no spaces after `,` or `:`, and no trailing newline.
// 5. Pretty mode (`to_json_pretty`, matching `JSON.stringify(value, null, 2)`):
//    - Empty arrays format as `[]` and empty objects format as `{}` on a
//      single line with no interior whitespace.
//    - Non-empty arrays and objects put each element/entry on its own line
//      indented by `(depth + 1) * 2` spaces, with a single space after `:` in
//      object entries (`"key": value`), and close with `depth * 2` spaces.

// Ordered JSON value tree mirroring V8's in-memory representation for schema
// evaluation and deterministic JSON codegen (`JSON.parse` / `JSON.stringify`).
//   - `Undefined` represents JS `undefined` inside evaluator environments and
//     is omitted when serializing object properties or converted to `null` in
//     arrays, matching ECMAScript `JSON.stringify`.
//   - `Object(Vec<(String, JsonValue)>)` stores properties in V8 enumeration
//     order (ascending array-index keys first, then string keys in insertion order).
#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    // Parses a complete UTF-8 JSON document (such as `tools/shaders/manifest.json`),
    // preserving the exact source order of object properties.
        // Parses a complete RFC 8259 JSON document into a `JsonValue`, rejecting
    // trailing non-whitespace tokens and enforcing V8 property ordering on every
    // parsed JSON object via `insert_object_key`.
    pub fn parse(input: &str) -> Result<JsonValue, String> {
        let mut p = Parser { bytes: input.as_bytes(), chars: input, pos: 0 };
        p.skip_ws();
        let val = p.parse_value()?;
        p.skip_ws();
        if p.pos != p.bytes.len() {
            return Err(format!("trailing characters at byte {}", p.pos));
        }
        Ok(val)
    }

    // Returns the ordered `(key, value)` slice when this value is a JSON object.
    pub fn as_object(&self) -> Option<&[(String, JsonValue)]> {
        match self {
            JsonValue::Object(entries) => Some(entries),
            _ => None,
        }
    }

    // Returns the element slice when this value is a JSON array.
    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(items) => Some(items),
            _ => None,
        }
    }

    // Returns the borrowed UTF-8 string slice when this value is a JSON string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s),
            _ => None,
        }
    }

    // Returns the underlying IEEE-754 `f64` when this value is a JSON number.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            JsonValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    // Truncates a JSON number to `i64` for signed integer schema constants.
    pub fn as_i64(&self) -> Option<i64> {
        self.as_f64().map(|n| n as i64)
    }

    // Truncates a JSON number to `usize` for array lengths, strides, and offsets.
    pub fn as_usize(&self) -> Option<usize> {
        self.as_f64().map(|n| n as usize)
    }

    // Truncates a JSON number to `u32` for unsigned 32-bit schema constants.
    pub fn as_u32(&self) -> Option<u32> {
        self.as_f64().map(|n| n as u32)
    }

    // Returns the boolean value when this node is `true` or `false`.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    // Looks up an object property by key in linear insertion order.
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.as_object()?.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    // Returns true when this value is `JsonValue::Null`.
    pub fn is_null(&self) -> bool {
        matches!(self, JsonValue::Null)
    }

    // Mirrors V8 `JSON.stringify(val)` with no whitespace between tokens.
    // Used by `render_shaders` when emitting inline manifest arrays and tables.
        // Serializes `self` as single-line compact JSON (`JSON.stringify(value)` with
    // no spacer argument). Object entries are emitted in their stored V8 property
    // order (`Index` keys ascending, then string keys in insertion order).
    pub fn to_json_compact(&self) -> String {
        let mut out = String::new();
        self.write_json(&mut out, None, 0);
        out
    }

    // Mirrors V8 `JSON.stringify(val, null, 2)` with 2-space indentation.
    // Used by `render_shaders` for `SHADER_MANIFEST` and layout generators for
    // exported JS layout objects.
        // Serializes `self` as 2-space indented JSON (`JSON.stringify(value, null, 2)`).
    // Empty arrays serialize as `"[]"` and empty objects serialize as `"{}"` on a
    // single line without interior whitespace, matching V8's exact formatting.
    pub fn to_json_pretty(&self) -> String {
        let mut out = String::new();
        self.write_json(&mut out, Some(2), 0);
        out
    }

    // Recursive formatter shared by compact (`indent = None`) and pretty
    // (`indent = Some(2)`) serialization.
    fn write_json(&self, out: &mut String, indent: Option<usize>, depth: usize) {
        match self {
            JsonValue::Null => out.push_str("null"),
            JsonValue::Bool(true) => out.push_str("true"),
            JsonValue::Bool(false) => out.push_str("false"),
            JsonValue::Number(n) => {
                // Integral floats in safe integer range print without `.0`,
                // and casting `-0.0 as i64` yields `0`, matching V8 `JSON.stringify(-0)`.
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    out.push_str(&format!("{}", *n as i64));
                } else {
                    out.push_str(&format!("{}", n));
                }
            }
            JsonValue::String(s) => out.push_str(&quote_str(s)),
            JsonValue::Array(items) => {
                // V8 formats empty arrays as `[]` on one line even in pretty mode.
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                if let Some(step) = indent {
                    let inner_pad = " ".repeat((depth + 1) * step);
                    let outer_pad = " ".repeat(depth * step);
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        out.push('\n');
                        out.push_str(&inner_pad);
                        item.write_json(out, indent, depth + 1);
                    }
                    out.push('\n');
                    out.push_str(&outer_pad);
                } else {
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        item.write_json(out, None, depth + 1);
                    }
                }
                out.push(']');
            }
            JsonValue::Object(entries) => {
                // V8 formats empty objects as `{}` on one line even in pretty mode.
                if entries.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                if let Some(step) = indent {
                    let inner_pad = " ".repeat((depth + 1) * step);
                    let outer_pad = " ".repeat(depth * step);
                    for (i, (k, v)) in entries.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        out.push('\n');
                        out.push_str(&inner_pad);
                        out.push_str(&quote_str(k));
                        out.push_str(": ");
                        v.write_json(out, indent, depth + 1);
                    }
                    out.push('\n');
                    out.push_str(&outer_pad);
                } else {
                    for (i, (k, v)) in entries.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        out.push_str(&quote_str(k));
                        out.push(':');
                        v.write_json(out, None, depth + 1);
                    }
                }
                out.push('}');
            }
        }
    }
}

// Formats a string literal with standard JSON double-quote escaping, matching
// V8 `JSON.stringify(str)`.
pub fn quote_str(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

// Hand-rolled recursive-descent parser for UTF-8 JSON documents.
struct Parser<'a> {
    bytes: &'a [u8],
    chars: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    // Advances `pos` past ASCII whitespace (`' '`, `'\t'`, `'\r'`, `'\n'`).
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\r' | b'\n' => self.pos += 1,
                _ => break,
            }
        }
    }

    // Dispatches on the next non-whitespace byte to parse a JSON value.
    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_ws();
        if self.pos >= self.bytes.len() {
            return Err("unexpected end of JSON".into());
        }
        match self.bytes[self.pos] {
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b'"' => self.parse_string().map(JsonValue::String),
            b't' if self.bytes[self.pos..].starts_with(b"true") => {
                self.pos += 4;
                Ok(JsonValue::Bool(true))
            }
            b'f' if self.bytes[self.pos..].starts_with(b"false") => {
                self.pos += 5;
                Ok(JsonValue::Bool(false))
            }
            b'n' if self.bytes[self.pos..].starts_with(b"null") => {
                self.pos += 4;
                Ok(JsonValue::Null)
            }
            b'-' | b'0'..=b'9' => self.parse_number(),
            c => Err(format!("unexpected char '{}' at {}", c as char, self.pos)),
        }
    }

    // Parses `{ "k": v, ... }` preserving first-seen key order and updating
    // duplicate keys in place without changing their position.
    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.pos += 1;
        self.skip_ws();
        let mut entries: Vec<(String, JsonValue)> = Vec::new();
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'}' {
            self.pos += 1;
            return Ok(JsonValue::Object(entries));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            if self.pos >= self.bytes.len() || self.bytes[self.pos] != b':' {
                return Err(format!("expected ':' at {}", self.pos));
            }
            self.pos += 1;
            let val = self.parse_value()?;
            if let Some(existing) = entries.iter_mut().find(|(k, _)| k == &key) {
                existing.1 = val;
            } else {
                entries.push((key, val));
            }
            self.skip_ws();
            if self.pos >= self.bytes.len() {
                return Err("unterminated object".into());
            }
            match self.bytes[self.pos] {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    break;
                }
                c => return Err(format!("expected ',' or '}}' at {}, got '{}'", self.pos, c as char)),
            }
        }
        Ok(JsonValue::Object(entries))
    }

    // Parses `[ v1, v2, ... ]` in element order.
    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.pos += 1;
        self.skip_ws();
        let mut items = Vec::new();
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b']' {
            self.pos += 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            items.push(self.parse_value()?);
            self.skip_ws();
            if self.pos >= self.bytes.len() {
                return Err("unterminated array".into());
            }
            match self.bytes[self.pos] {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    break;
                }
                c => return Err(format!("expected ',' or ']' at {}, got '{}'", self.pos, c as char)),
            }
        }
        Ok(JsonValue::Array(items))
    }

    // Scans a JSON string token and delegates unescaping (including surrogate
    // pairs and `\uXXXX` sequences) to `serde_json::from_str`.
    fn parse_string(&mut self) -> Result<String, String> {
        if self.pos >= self.bytes.len() || self.bytes[self.pos] != b'"' {
            return Err(format!("expected '\"' at {}", self.pos));
        }
        let start = self.pos;
        self.pos += 1;
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'\\' => self.pos += 2,
                b'"' => {
                    self.pos += 1;
                    let raw = &self.chars[start..self.pos];
                    return serde_json::from_str::<String>(raw).map_err(|e| e.to_string());
                }
                _ => self.pos += 1,
            }
        }
        Err("unterminated string".into())
    }

    // Scans a JSON number token and parses it into an `f64`.
    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let start = self.pos;
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E' => self.pos += 1,
                _ => break,
            }
        }
        let raw = &self.chars[start..self.pos];
        let n: f64 = raw.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
        Ok(JsonValue::Number(n))
    }
}
