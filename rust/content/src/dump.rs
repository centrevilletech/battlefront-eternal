//! The canonical content dump: the text tools/content-dump.mjs writes from
//! the JavaScript loaders and the content-dump binary writes from these, so
//! that comparing the two files compares every value bit for bit.
//!
//! A dump is lines of `section<TAB>key<TAB>payload`. A payload is a value's
//! canonical text, or for large ones `sha256:<hex>:<length>` of that text.
//! Canonical text: `u` (undefined), `null`, `true`, `false`, a number as `#`
//! and its f64 bits in 16 hex digits (`NaN` for any NaN), a string as
//! `JSON.stringify` writes it, arrays as `[a,b]`, objects as `{"k":v}` in
//! JavaScript's key order. A typed array is `f32[n]:sha256:<hex>` over its
//! elements' little-endian bytes.

use crate::value::Value;
use std::fmt::Write as _;

/// Payloads longer than this are written as their digest.
pub const INLINE_LIMIT: usize = 512;

pub fn number(out: &mut String, x: f64) {
    if x.is_nan() {
        out.push_str("NaN");
    } else {
        let _ = write!(out, "#{:016x}", x.to_bits());
    }
}

/// `JSON.stringify(s)`.
pub fn string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn value(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => number(out, *n),
        Value::String(s) => string(out, s),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                value(out, item);
            }
            out.push(']');
        }
        Value::Object(o) => {
            out.push('{');
            for (i, (k, item)) in o.entries().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                string(out, k);
                out.push(':');
                value(out, item);
            }
            out.push('}');
        }
    }
}

pub fn value_text(v: &Value) -> String {
    let mut out = String::new();
    value(&mut out, v);
    out
}

/// Write every payload in full (to find where two dumps part).
pub static INLINE_ALL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A payload: the text itself when short, else its digest.
pub fn payload(text: &str) -> String {
    if text.len() <= INLINE_LIMIT || INLINE_ALL.load(std::sync::atomic::Ordering::Relaxed) {
        text.to_string()
    } else {
        format!("sha256:{}:{}", hex(&sha256(text.as_bytes())), text.len())
    }
}

/// A typed array's payload over its little-endian bytes.
pub trait Elements {
    const NAME: &'static str;
    fn le_bytes(items: &[Self], out: &mut Vec<u8>)
    where
        Self: Sized;
}

macro_rules! elements {
    ($t:ty, $name:expr) => {
        impl Elements for $t {
            const NAME: &'static str = $name;
            fn le_bytes(items: &[Self], out: &mut Vec<u8>) {
                out.reserve(items.len() * std::mem::size_of::<$t>());
                for x in items {
                    out.extend_from_slice(&x.to_le_bytes());
                }
            }
        }
    };
}
elements!(f32, "f32");
elements!(f64, "f64");
elements!(u32, "u32");
elements!(u16, "u16");
elements!(u8, "u8");
elements!(i32, "i32");

pub fn typed<T: Elements>(items: &[T]) -> String {
    let mut bytes = Vec::new();
    T::le_bytes(items, &mut bytes);
    format!("{}[{}]:sha256:{}", T::NAME, items.len(), hex(&sha256(&bytes)))
}

/// Lines of a dump, in the order written.
#[derive(Default)]
pub struct Dump {
    pub text: String,
}

impl Dump {
    pub fn line(&mut self, section: &str, key: &str, payload: &str) {
        debug_assert!(!section.contains(['\t', '\n']) && !key.contains(['\t', '\n']));
        self.text.push_str(section);
        self.text.push('\t');
        self.text.push_str(key);
        self.text.push('\t');
        self.text.push_str(payload);
        self.text.push('\n');
    }

    /// A value's line: its canonical text, or digest when long.
    pub fn value(&mut self, section: &str, key: &str, v: &Value) {
        self.line(section, key, &payload(&value_text(v)));
    }

    pub fn typed<T: Elements>(&mut self, section: &str, key: &str, items: &[T]) {
        self.line(section, key, &typed(items));
    }

    pub fn number(&mut self, section: &str, key: &str, x: f64) {
        let mut s = String::new();
        number(&mut s, x);
        self.line(section, key, &s);
    }

    pub fn numbers(&mut self, section: &str, key: &str, xs: &[f64]) {
        let mut s = String::from("[");
        for (i, x) in xs.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            number(&mut s, *x);
        }
        s.push(']');
        self.line(section, key, &payload(&s));
    }

    pub fn text_line(&mut self, section: &str, key: &str, s: &str) {
        let mut t = String::new();
        string(&mut t, s);
        self.line(section, key, &t);
    }
}

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// SHA-256 (FIPS 180-4), for dump payloads only.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
        0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
        0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
        0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut tail = Vec::with_capacity(128);
    let whole = data.len() / 64 * 64;
    tail.extend_from_slice(&data[whole..]);
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&bit_len.to_be_bytes());
    let mut w = [0u32; 64];
    let mut block = |chunk: &[u8]| {
        for t in 0..16 {
            w[t] = u32::from_be_bytes([chunk[t * 4], chunk[t * 4 + 1], chunk[t * 4 + 2], chunk[t * 4 + 3]]);
        }
        for t in 16..64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for t in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[t]).wrapping_add(w[t]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    };
    for chunk in data[..whole].chunks_exact(64) {
        block(chunk);
    }
    for chunk in tail.chunks_exact(64) {
        block(chunk);
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_answers() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = vec![b'a'; 1000];
        assert_eq!(hex(&sha256(&long)), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
    }

    #[test]
    fn strings_escape_as_json_stringify() {
        let mut s = String::new();
        string(&mut s, "a\"b\\c\u{1}\u{1f}\n\u{2028}é");
        assert_eq!(s, "\"a\\\"b\\\\c\\u0001\\u001f\\n\u{2028}é\"");
    }
}
