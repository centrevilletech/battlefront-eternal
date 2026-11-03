//! The JavaScript semantics content loading needs: `parseFloat`,
//! `Number(...)`, `Math.min`/`Math.max`, `Math.hypot`, `Math.fround` and the
//! typed-array index rules, each as V8 13.6 (Node 24.22, the server's engine)
//! computes it. ODF values reach the simulation as strings that the
//! JavaScript reads with `parseFloat` or `Number`, so a number derived from
//! one in Rust must come from the same rules, prefix and all.
//!
//! The Math functions come from rust/v8math. The rest are private copies of
//! what `rust/jsrt` (the port's JavaScript semantics crate) provides or will
//! provide; once it is part of the build, callers move to it and the content
//! dump re-proves the result.

use crate::value::Value;

/// ECMAScript's WhiteSpace and LineTerminator code points: what `TrimString`
/// (and so `parseFloat`, `Number` and `String.prototype.trim`) removes.
/// U+180E is not one (Unicode 6.3 moved it out of Zs).
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}' | '\u{B}' | '\u{C}' | '\u{FEFF}' | '\u{A}' | '\u{D}' | '\u{2028}' | '\u{2029}' | '\u{20}' | '\u{A0}'
            | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}

/// The longest prefix of `t` that is a StrDecimalLiteral (no numeric
/// separators): its value and its length in bytes, or None when no prefix
/// is one. The value is the decimal rounded to the nearest double, ties to
/// even, as V8's StringToDouble rounds; "-0" gives -0.
fn decimal_prefix(t: &str) -> Option<(f64, usize)> {
    let b = t.as_bytes();
    let mut i = 0;
    let negative = match b.first() {
        Some(b'-') => {
            i = 1;
            true
        }
        Some(b'+') => {
            i = 1;
            false
        }
        _ => false,
    };
    if b[i..].starts_with(b"Infinity") {
        let value = if negative { f64::NEG_INFINITY } else { f64::INFINITY };
        return Some((value, i + 8));
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = &t[int_start..i];
    let mut frac_digits = "";
    if i < b.len() && b[i] == b'.' {
        let start = i + 1;
        let mut j = start;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        // "5." and ".5" are literals; "." alone is not.
        if !int_digits.is_empty() || j > start {
            frac_digits = &t[start..j];
            i = j;
        }
    }
    if int_digits.is_empty() && frac_digits.is_empty() {
        return None;
    }
    let mut exponent = "";
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        // "1e" and "1e+" end before the e: the exponent needs a digit.
        if j > start {
            exponent = &t[i + 1..j];
            i = j;
        }
    }
    // Rust's parser rounds correctly (to nearest, ties to even) as V8's
    // does; it is only ever given this normalised form.
    let mut text = String::with_capacity(i + 2);
    if negative {
        text.push('-');
    }
    text.push_str(if int_digits.is_empty() { "0" } else { int_digits });
    if !frac_digits.is_empty() {
        text.push('.');
        text.push_str(frac_digits);
    }
    if !exponent.is_empty() {
        text.push('e');
        text.push_str(exponent);
    }
    let value = text.parse::<f64>().expect("a normalised decimal literal");
    Some((value, i))
}

/// `parseFloat(string)`: leading white space skipped, then the longest
/// prefix that is a decimal literal ("Infinity" included, case-sensitive),
/// else NaN.
pub fn parse_float(s: &str) -> f64 {
    let t = s.trim_start_matches(is_js_space);
    match decimal_prefix(t) {
        Some((value, _)) => value,
        None => f64::NAN,
    }
}

/// The value of a NonDecimalIntegerLiteral's digits in `radix` (2, 8, 16),
/// rounded to the nearest double, ties to even; None if a digit is invalid
/// or there are none.
fn radix_value(digits: &str, radix: u32) -> Option<f64> {
    if digits.is_empty() {
        return None;
    }
    let bits_per_digit = match radix {
        2 => 1,
        8 => 3,
        _ => 4,
    };
    let mut bits: Vec<u8> = Vec::with_capacity(digits.len() * bits_per_digit);
    for c in digits.chars() {
        let d = c.to_digit(radix)?;
        for k in (0..bits_per_digit).rev() {
            bits.push(((d >> k) & 1) as u8);
        }
    }
    let first = match bits.iter().position(|&b| b == 1) {
        Some(p) => p,
        None => return Some(0.0),
    };
    let bits = &bits[first..];
    if bits.len() <= 53 {
        let mut v: u64 = 0;
        for &b in bits {
            v = (v << 1) | b as u64;
        }
        return Some(v as f64);
    }
    let mut mantissa: u64 = 0;
    for &b in &bits[..53] {
        mantissa = (mantissa << 1) | b as u64;
    }
    let round = bits[53] == 1;
    let sticky = bits[54..].iter().any(|&b| b == 1);
    if round && (sticky || mantissa & 1 == 1) {
        mantissa += 1;
    }
    // mantissa < 2^54, exact in f64; each doubling is exact until it
    // overflows to infinity, as the literal's value does.
    let mut value = mantissa as f64;
    for _ in 0..(bits.len() - 53) {
        value *= 2.0;
    }
    Some(value)
}

/// `Number(string)` (StringToNumber): white space trimmed at both ends, the
/// empty string 0, a whole decimal literal or 0x/0o/0b integer, else NaN.
pub fn string_to_number(s: &str) -> f64 {
    let t = s.trim_matches(is_js_space);
    if t.is_empty() {
        return 0.0;
    }
    let b = t.as_bytes();
    if b.len() > 2 && b[0] == b'0' {
        let radix = match b[1] {
            b'x' | b'X' => 16,
            b'o' | b'O' => 8,
            b'b' | b'B' => 2,
            _ => 0,
        };
        if radix != 0 {
            return radix_value(&t[2..], radix).unwrap_or(f64::NAN);
        }
    }
    match decimal_prefix(t) {
        Some((value, length)) if length == t.len() => value,
        _ => f64::NAN,
    }
}

/// `parseFloat(value)` for a content value: ToString first (an array joins
/// its elements with commas, so only its first element's text can count; a
/// number's text reads back as itself, but -0's is "0").
pub fn parse_float_value(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(_)) | Some(Value::Object(_)) => f64::NAN,
        Some(Value::Number(n)) => {
            if *n == 0.0 {
                0.0
            } else {
                *n
            }
        }
        Some(Value::String(s)) => parse_float(s),
        Some(Value::Array(items)) => match items.first() {
            // [] is "", NaN; [null] and [undefined] are "" then a comma or
            // the end, NaN too.
            None => f64::NAN,
            Some(first) => parse_float_value(Some(first)),
        },
    }
}

/// `Number(value)` (ToNumber) for a content value.
pub fn to_number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::Number(n)) => *n,
        Some(Value::String(s)) => string_to_number(s),
        Some(Value::Object(_)) => f64::NAN,
        Some(Value::Array(items)) => match items.len() {
            // "" is 0; any two elements leave a comma in the text: NaN.
            0 => 0.0,
            1 => match &items[0] {
                Value::Null => 0.0,
                Value::Number(n) => {
                    if *n == 0.0 {
                        0.0
                    } else {
                        *n
                    }
                }
                Value::String(s) => string_to_number(s),
                Value::Bool(_) | Value::Object(_) => f64::NAN,
                inner @ Value::Array(_) => to_number(Some(inner)),
            },
            _ => f64::NAN,
        },
    }
}

/// `Math.min(a, b)`: NaN if either is, and -0 below +0 (rust/v8math).
#[inline]
pub fn js_min(a: f64, b: f64) -> f64 {
    v8math::min(a, b)
}

/// `Math.max(a, b)`: NaN if either is, and +0 above -0 (rust/v8math).
#[inline]
pub fn js_max(a: f64, b: f64) -> f64 {
    v8math::max(a, b)
}

/// `Math.hypot(...values)` as V8 computes it (rust/v8math): every value
/// divided by the largest magnitude, the squares summed with Kahan's
/// compensation in argument order, the root scaled back; an infinity wins
/// over NaN.
#[inline]
pub fn js_hypot(values: &[f64]) -> f64 {
    v8math::hypot_n(values)
}

/// `Math.atan2(y, x)` (rust/v8math).
#[inline]
pub fn js_atan2(y: f64, x: f64) -> f64 {
    v8math::atan2(y, x)
}

/// `Math.fround(x)`.
#[inline]
pub fn fround(x: f64) -> f64 {
    x as f32 as f64
}

/// ToIntegerOrInfinity: NaN 0, infinities kept, else truncated (-0 to 0).
pub fn to_integer_or_infinity(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 {
        return 0.0;
    }
    if x.is_infinite() {
        return x;
    }
    x.trunc() + 0.0
}

/// ToUint32.
pub fn to_uint32(x: f64) -> u32 {
    if !x.is_finite() || x == 0.0 {
        return 0;
    }
    let m = x.trunc() % 4294967296.0;
    let m = if m < 0.0 { m + 4294967296.0 } else { m };
    m as u32
}

/// ToInt32.
pub fn to_int32(x: f64) -> i32 {
    to_uint32(x) as i32
}

/// ToUint16 (a store into a Uint16Array).
pub fn to_uint16(x: f64) -> u16 {
    to_uint32(x) as u16
}

/// A typed array's `[start, end)` from `subarray`/`slice` arguments on an
/// array of `length` elements (relative indices: negative from the end,
/// clamped, truncated).
pub fn relative_range(length: usize, start: f64, end: Option<f64>) -> (usize, usize) {
    let len = length as f64;
    let rel = |x: f64| {
        let r = to_integer_or_infinity(x);
        if r < 0.0 {
            js_max(len + r, 0.0)
        } else {
            js_min(r, len)
        }
    };
    let from = rel(start);
    let to = match end {
        None => len,
        Some(e) => rel(e),
    };
    let to = js_max(to, from);
    (from as usize, to as usize)
}

/// A typed array's element at a number index: None (undefined) unless the
/// index is an integer in range (-0 counts as 0).
pub fn typed_get<T: Copy>(array: &[T], index: f64) -> Option<T> {
    if index.is_nan() || index.fract() != 0.0 || index < 0.0 || index >= array.len() as f64 {
        return None;
    }
    Some(array[index as usize])
}

/// `ToString(x)` for a JavaScript number, as `Number::toString` writes it:
/// the shortest digits that read back as x, in plain notation for exponents
/// from -7 to 20, else as d.ddde+n. Only content keys and labels need it
/// (an object key made from a number, a joined name).
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-Infinity".into() } else { "Infinity".into() };
    }
    // Rust's {:e} gives the shortest round-trip digits: d[.ddd]e<exp>.
    let formatted = format!("{:e}", x.abs());
    let (mantissa, exponent) = formatted.split_once('e').expect("exponent form");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exponent.parse::<i32>().expect("an exponent") + 1;
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    if k <= n && n <= 21 {
        out.push_str(&digits);
        for _ in 0..(n - k) {
            out.push('0');
        }
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        for _ in 0..(-n) {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if n - 1 < 0 { '-' } else { '+' });
        out.push_str(&(n - 1).abs().to_string());
    }
    out
}

/// `JSON.stringify(value)` for a content value: numbers in their
/// `Number::toString` text (a non-finite one as null), strings escaped as
/// JSON.stringify escapes them, an absent array element (undefined) as null.
pub fn json_stringify(value: Option<&Value>) -> String {
    let mut out = String::new();
    fn write(out: &mut String, v: Option<&Value>) {
        match v {
            None | Some(Value::Null) => out.push_str("null"),
            Some(Value::Bool(b)) => out.push_str(if *b { "true" } else { "false" }),
            Some(Value::Number(n)) => {
                if n.is_finite() {
                    out.push_str(&number_to_string(*n));
                } else {
                    out.push_str("null");
                }
            }
            Some(Value::String(s)) => crate::dump::string(out, s),
            Some(Value::Array(items)) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(out, Some(item));
                }
                out.push(']');
            }
            Some(Value::Object(o)) => {
                out.push('{');
                for (i, (k, item)) in o.entries().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    crate::dump::string(out, k);
                    out.push(':');
                    write(out, Some(item));
                }
                out.push('}');
            }
        }
    }
    write(&mut out, value);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected bit patterns from Node 24.22 (`parseFloat`, `Number`).
    #[test]
    fn parse_float_follows_v8() {
        let cases: &[(&str, u64)] = &[
            ("1e", 0x3ff0000000000000),
            ("1e+", 0x3ff0000000000000),
            (".5", 0x3fe0000000000000),
            ("5.", 0x4014000000000000),
            (" \u{a0}\u{feff}\u{2028} 12abc", 0x4028000000000000),
            ("-0", 0x8000000000000000),
            ("-.0e5", 0x8000000000000000),
            ("+Infinity", 0x7ff0000000000000),
            ("-Infinityx", 0xfff0000000000000),
            ("1_0", 0x3ff0000000000000),
            ("0x10", 0),
            ("1e400", 0x7ff0000000000000),
            ("1e-400", 0),
            ("\u{2007} 3", 0x4008000000000000),
            ("00012", 0x4028000000000000),
            ("1.5e+2x", 0x4062c00000000000),
            ("4.9e-324", 1),
            ("2.4703282292062328e-324", 1),
            ("2.4703282292062327e-324", 0),
            ("5.e3", 0x40b3880000000000),
        ];
        for &(text, bits) in cases {
            assert_eq!(parse_float(text).to_bits(), bits, "{text:?}");
        }
        for text in [".", "infinity", "\u{180e}5", "  ", "-", "+.e1", ".e1", ""] {
            assert!(parse_float(text).is_nan(), "{text:?}");
        }
    }

    #[test]
    fn number_follows_v8() {
        assert_eq!(string_to_number(""), 0.0);
        assert_eq!(string_to_number(" 0x1F "), 31.0);
        assert_eq!(string_to_number("0b11"), 3.0);
        assert!(string_to_number("-0x10").is_nan());
        assert_eq!(string_to_number("1e3 "), 1000.0);
        assert_eq!(string_to_number(".5"), 0.5);
        assert_eq!(string_to_number("5."), 5.0);
        assert_eq!(string_to_number("Infinity"), f64::INFINITY);
        assert!(string_to_number("1e").is_nan());
        assert!(string_to_number("0x").is_nan());
        assert_eq!(string_to_number("0x20000000000001"), 9007199254740992.0);
        assert_eq!(string_to_number("0x20000000000003"), 9007199254740996.0);
    }

    #[test]
    fn number_text_follows_v8() {
        for (x, text) in [
            (1.5, "1.5"),
            (1e21, "1e+21"),
            (123456789012345680000.0, "123456789012345680000"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (-2.5e-8, "-2.5e-8"),
            (5e-324, "5e-324"),
            (100.0, "100"),
            (-0.0, "0"),
        ] {
            assert_eq!(number_to_string(x), text);
        }
    }

    #[test]
    fn min_max_keep_signed_zero_and_nan() {
        assert!(js_min(-0.0, 0.0).is_sign_negative());
        assert!(js_min(0.0, -0.0).is_sign_negative());
        assert!(js_max(-0.0, 0.0).is_sign_positive());
        assert!(js_max(1.0, f64::NAN).is_nan());
    }
}
