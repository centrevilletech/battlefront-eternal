//! JavaScript strings are UTF-16 code unit sequences; Rust's are
//! UTF-8. Anything in the sim that measures a string, hashes it by
//! `charCodeAt`, compares it with `<`, or sorts it by the default
//! comparator sees code units, so those operations go through here.
//! A JavaScript string that must keep lone surrogates (names off the
//! wire, content keys) is held as [`JsString`], a `Vec<u16>`.

use std::cmp::Ordering;

/// A JavaScript string: its UTF-16 code units, lone surrogates kept.
pub type JsString = Vec<u16>;

/// The UTF-16 code units of a Rust string (`[...]` by unit, as
/// `charCodeAt` walks them).
pub fn encode_utf16(s: &str) -> JsString {
    s.encode_utf16().collect()
}

/// `s.length`: the count of UTF-16 code units.
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `s.charCodeAt(i)` where `s` is held as code units: the unit, or
/// NaN (as JavaScript returns) when out of range. Index arithmetic
/// follows ToIntegerOrInfinity, as the spec's does.
pub fn char_code_at(units: &[u16], index: f64) -> f64 {
    let i = crate::conv::to_integer_or_infinity(index);
    if i < 0.0 || i >= units.len() as f64 {
        return f64::NAN;
    }
    units[i as usize] as f64
}

/// `s.codePointAt(i)`: the code point at a unit index, joining a
/// surrogate pair; undefined (None) out of range.
pub fn code_point_at(units: &[u16], index: usize) -> Option<u32> {
    let first = *units.get(index)? as u32;
    if (0xd800..0xdc00).contains(&first) {
        if let Some(&second) = units.get(index + 1) {
            let second = second as u32;
            if (0xdc00..0xe000).contains(&second) {
                return Some(0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00));
            }
        }
    }
    Some(first)
}

/// The order `<` on strings and the default sort comparator use:
/// code unit by code unit, then the shorter first. (Not Rust's &str
/// order: a code point above U+FFFF sorts by its high surrogate,
/// which precedes U+E000-U+FFFF although the code point is larger.)
pub fn compare_utf16(a: &[u16], b: &[u16]) -> Ordering {
    for (x, y) in a.iter().zip(b.iter()) {
        match x.cmp(y) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    a.len().cmp(&b.len())
}

/// [`compare_utf16`] for Rust strings.
pub fn compare_str_utf16(a: &str, b: &str) -> Ordering {
    let mut ai = a.encode_utf16();
    let mut bi = b.encode_utf16();
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => match x.cmp(&y) {
                Ordering::Equal => {}
                other => return other,
            },
        }
    }
}

/// `String.fromCharCode(...units)`: each number through ToUint16.
pub fn from_char_codes(values: &[f64]) -> JsString {
    values
        .iter()
        .map(|&v| crate::conv::to_uint32(v) as u16)
        .collect()
}

/// What `TextEncoder.encode` makes of a JavaScript string: UTF-8,
/// with every lone surrogate replaced by U+FFFD.
pub fn text_encode(units: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        let cp = code_point_at(units, i).unwrap_or(0xfffd);
        let ch = char::from_u32(cp).unwrap_or(char::REPLACEMENT_CHARACTER);
        let mut buf = [0u8; 4];
        out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        i += if cp >= 0x10000 { 2 } else { 1 };
    }
    out
}

/// A JavaScript string as a Rust `String`, losing nothing but the
/// lone surrogates (which become U+FFFD, as `TextDecoder` makes of
/// ill-formed input and as any UTF-8 sink must).
pub fn to_rust_string(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// The units of a Rust string slice held as a JavaScript string.
/// (The sim interns its names; this is for boundaries and tests.)
pub fn js_string(s: &str) -> JsString {
    encode_utf16(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_units_follow_js() {
        let units = encode_utf16("a\u{1f600}b");
        assert_eq!(units, vec![0x61, 0xd83d, 0xde00, 0x62]);
        assert_eq!(utf16_len("a\u{1f600}b"), 4);
        assert_eq!(char_code_at(&units, 1.0), 0xd83d as f64);
        assert!(char_code_at(&units, 4.0).is_nan());
        assert!(char_code_at(&units, -1.0).is_nan());
        assert_eq!(code_point_at(&units, 1), Some(0x1f600));
        assert_eq!(code_point_at(&units, 2), Some(0xde00));
    }

    #[test]
    fn utf16_order_differs_from_codepoint_order() {
        // U+10000 (units D800 DC00) precedes U+E000 in code-unit order.
        assert_eq!(
            compare_str_utf16("\u{10000}", "\u{e000}"),
            Ordering::Less
        );
        assert_eq!(compare_utf16(&[0x61], &[0x61, 0x62]), Ordering::Less);
        assert_eq!(compare_utf16(&[], &[]), Ordering::Equal);
    }

    #[test]
    fn text_encode_replaces_lone_surrogates() {
        assert_eq!(text_encode(&[0x61, 0xd800, 0x62]), vec![0x61, 0xef, 0xbf, 0xbd, 0x62]);
        assert_eq!(text_encode(&[0xd83d, 0xde00]), vec![0xf0, 0x9f, 0x98, 0x80]);
    }
}
