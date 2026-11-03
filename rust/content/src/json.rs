//! `JSON.parse` as V8 runs it, over a file's bytes as the server reads them
//! (UTF-8, invalid sequences replaced, no byte-order mark stripped:
//! `JSON.parse(bytes.toString())`). Numbers are correctly rounded, as V8's
//! StringToDouble rounds them ("-0" is -0); objects take JavaScript's key
//! order (src/value.rs). Anything JSON.parse rejects is an error here too.

use crate::value::{Object, Value};
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub struct JsonError {
    pub at: usize,
    pub message: &'static str,
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON: {} at byte {}", self.message, self.at)
    }
}

impl std::error::Error for JsonError {}

/// Parses a file's bytes as the server does.
pub fn parse_bytes(bytes: &[u8]) -> Result<Value, JsonError> {
    let text = String::from_utf8_lossy(bytes);
    parse(&text)
}

/// `JSON.parse(text)`.
pub fn parse(text: &str) -> Result<Value, JsonError> {
    let mut p = Parser { b: text.as_bytes(), text, i: 0 };
    p.space();
    let value = p.value(0)?;
    p.space();
    if p.i != p.b.len() {
        return Err(p.error("unexpected text after the value"));
    }
    Ok(value)
}

struct Parser<'a> {
    b: &'a [u8],
    text: &'a str,
    i: usize,
}

impl<'a> Parser<'a> {
    fn error(&self, message: &'static str) -> JsonError {
        JsonError { at: self.i, message }
    }

    // JSON's white space: space, tab, line feed, carriage return.
    fn space(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, JsonError> {
        if depth > 10_000 {
            return Err(self.error("too deep"));
        }
        match self.b.get(self.i) {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Value::String(Rc::from(self.string()?.as_str()))),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'n') => self.word("null", Value::Null),
            Some(b'-') | Some(b'0'..=b'9') => self.number(),
            _ => Err(self.error("unexpected token")),
        }
    }

    fn word(&mut self, word: &'static str, value: Value) -> Result<Value, JsonError> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(value)
        } else {
            Err(self.error("unexpected token"))
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.i;
        while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
            self.i += 1;
        }
        self.i - start
    }

    fn number(&mut self) -> Result<Value, JsonError> {
        let start = self.i;
        if self.b[self.i] == b'-' {
            self.i += 1;
        }
        match self.b.get(self.i) {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.error("no number after minus sign")),
        }
        if self.b.get(self.i) == Some(&b'.') {
            self.i += 1;
            if self.digits() == 0 {
                return Err(self.error("unterminated fractional number"));
            }
        }
        if matches!(self.b.get(self.i), Some(b'e') | Some(b'E')) {
            self.i += 1;
            if matches!(self.b.get(self.i), Some(b'+') | Some(b'-')) {
                self.i += 1;
            }
            if self.digits() == 0 {
                return Err(self.error("exponent part is missing a number"));
            }
        }
        // The grammar above is a subset of Rust's float syntax; its parse
        // rounds to nearest, ties to even, as V8's does.
        let value = self.text[start..self.i].parse::<f64>().map_err(|_| self.error("bad number"))?;
        Ok(Value::Number(value))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        if self.i + 4 > self.b.len() {
            return Err(self.error("bad unicode escape"));
        }
        let mut v = 0u32;
        for k in 0..4 {
            let d = (self.b[self.i + k] as char).to_digit(16).ok_or_else(|| self.error("bad unicode escape"))?;
            v = v * 16 + d;
        }
        self.i += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.i += 1; // the opening quote
        let mut out = String::new();
        loop {
            let start = self.i;
            while self.i < self.b.len() && self.b[self.i] != b'"' && self.b[self.i] != b'\\' && self.b[self.i] >= 0x20 {
                self.i += 1;
            }
            out.push_str(&self.text[start..self.i]);
            match self.b.get(self.i) {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    let c = *self.b.get(self.i).ok_or_else(|| self.error("unterminated string"))?;
                    self.i += 1;
                    match c {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let unit = self.hex4()?;
                            if (0xD800..0xDC00).contains(&unit) && self.b[self.i..].starts_with(b"\\u") {
                                let save = self.i;
                                self.i += 2;
                                let low = self.hex4()?;
                                if (0xDC00..0xE000).contains(&low) {
                                    let c = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                                    out.push(char::from_u32(c).expect("a surrogate pair's code point"));
                                    continue;
                                }
                                self.i = save;
                            }
                            // JavaScript keeps a lone surrogate in the string;
                            // a Rust string cannot hold one, and no content
                            // file has one.
                            match char::from_u32(unit) {
                                Some(c) => out.push(c),
                                None => return Err(self.error("lone surrogate in a string")),
                            }
                        }
                        _ => return Err(self.error("bad escape")),
                    }
                }
                Some(_) => return Err(self.error("control character in a string")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, JsonError> {
        self.i += 1;
        let mut items = Vec::new();
        self.space();
        if self.b.get(self.i) == Some(&b']') {
            self.i += 1;
            return Ok(Value::Array(Rc::new(items)));
        }
        loop {
            self.space();
            items.push(self.value(depth + 1)?);
            self.space();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::Array(Rc::new(items)));
                }
                _ => return Err(self.error("expected , or ]")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, JsonError> {
        self.i += 1;
        let mut written: Vec<(Rc<str>, Value)> = Vec::new();
        self.space();
        if self.b.get(self.i) == Some(&b'}') {
            self.i += 1;
            return Ok(Value::Object(Rc::new(Object::from_written(written))));
        }
        loop {
            self.space();
            if self.b.get(self.i) != Some(&b'"') {
                return Err(self.error("expected a property name"));
            }
            let key = self.string()?;
            self.space();
            if self.b.get(self.i) != Some(&b':') {
                return Err(self.error("expected :"));
            }
            self.i += 1;
            self.space();
            let value = self.value(depth + 1)?;
            written.push((Rc::from(key.as_str()), value));
            self.space();
            match self.b.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Value::Object(Rc::new(Object::from_written(written))));
                }
                _ => return Err(self.error("expected , or }")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_like_v8() {
        let v = parse(r#" {"b":1,"a":[2,-0,1.5e-3,0.1,"xé😀"],"b":3,"10":4,"2":true,"n":null} "#).unwrap();
        let o = v.as_object().unwrap();
        assert_eq!(o.keys().collect::<Vec<_>>(), ["2", "10", "b", "a", "n"]);
        assert_eq!(o.get("b").unwrap().as_f64(), Some(3.0));
        let a = o.get("a").unwrap().as_array().unwrap();
        assert!(a[1].as_f64().unwrap().is_sign_negative());
        assert_eq!(a[2].as_f64().unwrap(), 0.0015);
        assert_eq!(a[4].as_str().unwrap(), "x\u{e9}\u{1f600}");
    }

    #[test]
    fn rejects_what_v8_rejects() {
        for text in ["", "01", "1.", ".5", "-", "+1", "[1,]", "{\"a\":1,}", "NaN", "Infinity", "\"a\nb\"", "[1] x", "\u{feff}1", "'a'"] {
            assert!(parse(text).is_err(), "{text:?}");
        }
    }
}
