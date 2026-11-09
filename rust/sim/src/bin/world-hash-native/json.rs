//! `JSON.stringify` as V8 writes it, for the two observation hashes: the
//! Node tool hashes `JSON.stringify(worldState(game))` and
//! `JSON.stringify(digest())`, so key order, number spelling and string
//! escaping must be V8's to the byte. Numbers go through
//! `jsrt::numfmt::json_number` (`Number::toString`, `null` for NaN and the
//! infinities); strings follow `QuoteJSONString` (ASCII control characters
//! as `\uXXXX`, `"` and `\` escaped, lone surrogates as `\uXXXX`).
use jsrt::numfmt::json_number;

/// A growing JSON text. `Vec<bool>` is the "first element written" stack,
/// one entry per open array or object, so commas come out as V8 places them.
#[derive(Default)]
pub struct Json {
    text: String,
    first: Vec<bool>,
    /// A key was just written: its value takes no comma.
    after_key: bool,
}

impl Json {
    pub fn raw(&mut self, text: &str) { self.separate(); self.text.push_str(text); } // matrix-b
    pub fn new() -> Self {
        Self {
            text: String::with_capacity(1 << 16),
            first: Vec::with_capacity(16),
            after_key: false,
        }
    }
    pub fn clear(&mut self) {
        self.text.clear();
        self.first.clear();
        self.after_key = false;
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    /// A comma before every element but the first of the enclosing list.
    fn separate(&mut self) {
        if self.after_key {
            self.after_key = false;
            return;
        }
        if let Some(first) = self.first.last_mut() {
            if *first {
                *first = false;
            } else {
                self.text.push(',');
            }
        }
    }
    pub fn begin_array(&mut self) {
        self.separate();
        self.text.push('[');
        self.first.push(true);
    }
    pub fn end_array(&mut self) {
        self.first.pop();
        self.text.push(']');
    }
    pub fn begin_object(&mut self) {
        self.separate();
        self.text.push('{');
        self.first.push(true);
    }
    pub fn end_object(&mut self) {
        self.first.pop();
        self.text.push('}');
    }
    /// `"key":` before a value inside an object.
    pub fn key(&mut self, key: &str) {
        self.separate();
        quote(&mut self.text, key);
        self.text.push(':');
        self.after_key = true;
    }
    pub fn number(&mut self, value: f64) {
        self.separate();
        self.text.push_str(&json_number(value));
    }
    pub fn boolean(&mut self, value: bool) {
        self.separate();
        self.text.push_str(if value { "true" } else { "false" });
    }
    pub fn null(&mut self) {
        self.separate();
        self.text.push_str("null");
    }
    pub fn string(&mut self, value: &str) {
        self.separate();
        quote(&mut self.text, value);
    }
    /// A UTF-16 string as the store keeps it (`StrId` text), escaped as V8
    /// escapes a JS string: a lone surrogate becomes `\uXXXX`.
    pub fn string_utf16(&mut self, units: &[u16]) {
        self.separate();
        self.text.push('"');
        for unit in char::decode_utf16(units.iter().copied()) {
            match unit {
                Ok(c) => escape_char(&mut self.text, c),
                Err(e) => {
                    self.text
                        .push_str(&format!("\\u{:04x}", e.unpaired_surrogate()));
                }
            }
        }
        self.text.push('"');
    }
    /// `[x,y,z]` of a `Vector3.toArray()`.
    pub fn vec3(&mut self, v: &[f64; 3]) {
        self.begin_array();
        for value in v {
            self.number(*value);
        }
        self.end_array();
    }
    /// A nullable vector: `null` or `[x,y,z]` (`xyz(v)` in world-hash.mjs).
    pub fn vec3_or_null(&mut self, v: Option<&[f64; 3]>) {
        match v {
            Some(v) => self.vec3(v),
            None => self.null(),
        }
    }
}

/// `QuoteJSONString`: V8 escapes `"`, `\`, and control characters below
/// 0x20 with the short forms `\b \t \n \f \r` where they exist and `\u00XX`
/// otherwise; everything else, including non-ASCII, is written as is.
fn escape_char(out: &mut String, c: char) {
    match c {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        '\u{8}' => out.push_str("\\b"),
        '\t' => out.push_str("\\t"),
        '\n' => out.push_str("\\n"),
        '\u{c}' => out.push_str("\\f"),
        '\r' => out.push_str("\\r"),
        c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
        c => out.push(c),
    }
}

fn quote(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        escape_char(out, c);
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::Json;
    #[test]
    fn writes_v8_shapes() {
        let mut j = Json::new();
        j.begin_object();
        j.key("tick");
        j.number(1.0);
        j.key("list");
        j.begin_array();
        j.number(0.5);
        j.number(-0.0);
        j.null();
        j.string("a\"b\n");
        j.boolean(true);
        j.end_array();
        j.key("empty");
        j.begin_object();
        j.end_object();
        j.end_object();
        assert_eq!(
            j.text(),
            r#"{"tick":1,"list":[0.5,0,null,"a\"b\n",true],"empty":{}}"#
        );
    }
}
