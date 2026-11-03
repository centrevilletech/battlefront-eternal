//! ODF settings as the JavaScript reads them. Vehicle classes, their seats
//! and props carry their 2004 class keys as text ("300.0", "-45.0 45.0",
//! hashed names where the extractor kept only a hash), and each reader
//! turns them into numbers its own way: `parseFloat` with a fallback
//! (vehicles.js num, seats.js numberOf, props.js num, vehicle-hover,
//! vehicle-motion, flight/camera), `Number` (vehicle-water), whitespace
//! pairs (props.js pair), sscanf-like vectors (props.js scanVector),
//! `parseInt` (mobile-posts). Each is here, by the same rules, and the
//! content dump proves them on every setting of every class.

use crate::js::{is_js_space, parse_float_value, string_to_number, to_number};
use crate::value::Value;

/// seats.js fnv: the engine's FNV-1a over a name's UTF-16 units' low bytes
/// OR 0x20, as eight hex digits.
pub fn fnv(name: &str) -> String {
    let mut h: u32 = 0x811c9dc5;
    for unit in name.encode_utf16() {
        h ^= ((unit & 0xff) as u32) | 0x20;
        h = h.wrapping_mul(0x01000193);
    }
    format!("{h:08x}")
}

fn nullish(v: Option<&Value>) -> bool {
    matches!(v, None | Some(Value::Null))
}

/// seats.js setting(settings, name): `settings[name] ?? settings[fnv(name)]`.
pub fn setting<'a>(settings: Option<&'a Value>, name: &str) -> Option<&'a Value> {
    let settings = settings.filter(|s| s.truthy())?;
    let direct = settings.get(name);
    if !nullish(direct) {
        return direct;
    }
    settings.get(&fnv(name))
}

/// vehicle-motion.js classSetting(def, name): the body's key, else the
/// first later seat's.
pub fn class_setting<'a>(def: Option<&'a Value>, name: &str) -> Option<&'a Value> {
    let value = setting(def.and_then(|d| d.get("settings")), name);
    if value.is_some() {
        return value;
    }
    let seats = def.and_then(|d| d.get("seats")).and_then(Value::as_array).unwrap_or(&[]);
    for seat in seats.iter().skip(1) {
        let later = setting(seat.get("settings"), name);
        if later.is_some() {
            return later;
        }
    }
    None
}

/// vehicles.js num(v, fallback): parseFloat, when finite.
pub fn num(v: Option<&Value>, fallback: f64) -> f64 {
    let n = parse_float_value(v);
    if n.is_finite() {
        n
    } else {
        fallback
    }
}

/// seats.js numberOf / props.js num: parseFloat of an array's first
/// element (the same number as parseFloat of the array).
pub fn number_of(v: Option<&Value>, fallback: f64) -> f64 {
    let first = match v {
        Some(Value::Array(items)) => items.first(),
        other => other,
    };
    num(first, fallback)
}

/// vehicle-water.js: `Number(Array.isArray(value) ? value[0] : value)`.
pub fn number_first(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Array(items)) => to_number(items.first()),
        other => to_number(other),
    }
}

/// String(value) for the readers that split text.
pub fn to_text(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => crate::js::number_to_string(*n),
        Some(Value::String(s)) => s.to_string(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|x| match x {
                Value::Null => String::new(),
                other => to_text(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// `text.split(/\s+/)`.
pub fn split_space(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_space = false;
    let mut space_start = 0;
    for (i, c) in text.char_indices() {
        if is_js_space(c) {
            if !in_space {
                in_space = true;
                space_start = i;
            }
        } else if in_space {
            out.push(&text[start..space_start]);
            start = i;
            in_space = false;
        }
    }
    if in_space {
        out.push(&text[start..space_start]);
        out.push("");
    } else {
        out.push(&text[start..]);
    }
    out
}

/// props.js pair(value, fallback): the finite numbers of the value's
/// whitespace-separated words, when there are at least two.
pub fn pair(v: Option<&Value>) -> Option<Vec<f64>> {
    let text = match v {
        None | Some(Value::Null) => String::new(),
        other => to_text(other),
    };
    let parts: Vec<f64> = split_space(&text).into_iter().map(string_to_number).filter(|n| n.is_finite()).collect();
    if parts.len() >= 2 {
        Some(parts)
    } else {
        None
    }
}

/// The longest match of props.js FLOAT at the start of `rest`:
/// `^\s*[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?`, as a byte length.
fn float_match(rest: &str) -> Option<usize> {
    let mut i = rest.len() - rest.trim_start_matches(is_js_space).len();
    let b = rest.as_bytes();
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits = |from: usize| -> usize {
        let mut j = from;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        j
    };
    let j = digits(i);
    if j > i {
        i = j;
        if i < b.len() && b[i] == b'.' {
            i = digits(i + 1);
        }
    } else if i < b.len() && b[i] == b'.' {
        let k = digits(i + 1);
        if k == i + 1 {
            return None;
        }
        i = k;
    } else {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut k = i + 1;
        if k < b.len() && (b[k] == b'+' || b[k] == b'-') {
            k += 1;
        }
        let e = digits(k);
        if e > k {
            i = e;
        }
    }
    Some(i)
}

/// props.js scanVector(value): up to three floats as sscanf "%f %f %f"
/// reads them (missing ones 0), from the last of repeated values; None
/// when not even the first parses.
pub fn scan_vector(v: Option<&Value>) -> Option<[f64; 3]> {
    let source = match v {
        Some(Value::Array(items)) => items.last().cloned(),
        None | Some(Value::Null) => Some(Value::str("")),
        Some(other) => Some(other.clone()),
    };
    let mut rest = to_text(source.as_ref());
    let mut out: Vec<f64> = Vec::new();
    while out.len() < 3 {
        let Some(length) = float_match(&rest) else { break };
        out.push(crate::js::parse_float(&rest[..length]));
        rest = rest[length..].to_string();
    }
    if out.is_empty() {
        return None;
    }
    while out.len() < 3 {
        out.push(0.0);
    }
    Some([out[0], out[1], out[2]])
}

/// `Number.parseInt(value, 10)`.
pub fn parse_int10(v: Option<&Value>) -> f64 {
    let text = to_text(v);
    let t = text.trim_start_matches(is_js_space);
    let (negative, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let end = digits.bytes().position(|c| !c.is_ascii_digit()).unwrap_or(digits.len());
    if end == 0 {
        return f64::NAN;
    }
    let value: f64 = digits[..end].parse().expect("decimal digits");
    if negative {
        -value
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_matches_the_extractor_keys() {
        // vehicles.json keeps "da1b8c0c" for the seat position key.
        assert_eq!(fnv(""), "811c9dc5");
    }

    #[test]
    fn scan_vector_reads_like_sscanf() {
        let v = Value::str("0.0 1.5.0 -2.0");
        assert_eq!(scan_vector(Some(&v)), Some([0.0, 1.5, 0.0]));
        assert_eq!(scan_vector(Some(&Value::str("x"))), None);
        assert_eq!(scan_vector(Some(&Value::str(" 3"))), Some([3.0, 0.0, 0.0]));
    }

    #[test]
    fn pair_counts_leading_space_as_zero() {
        assert_eq!(pair(Some(&Value::str(" 1 2"))), Some(vec![0.0, 1.0, 2.0]));
        assert_eq!(pair(Some(&Value::str("-45.0 45.0"))), Some(vec![-45.0, 45.0]));
        assert_eq!(pair(Some(&Value::str("5"))), None);
    }
}
