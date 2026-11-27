//! Locate only the added family members. Serde skips complete JSON values,
//! so an unrelated member keeps its original spelling, depth and number range.
use serde::de::{DeserializeOwned, IgnoredAny};
use std::borrow::Cow;

fn trim(bytes: &[u8]) -> &[u8] {
    let first = bytes.iter().position(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n')).unwrap_or(bytes.len());
    &bytes[first..]
}

fn take_value<'a>(rest: &mut &'a [u8]) -> Option<&'a [u8]> {
    let bytes = trim(rest);
    let mut stream = serde_json::Deserializer::from_slice(bytes).into_iter::<IgnoredAny>();
    stream.next()?.ok()?;
    let (value, tail) = bytes.split_at(stream.byte_offset());
    *rest = trim(tail);
    Some(value)
}

// Values immediately inside an object or array. The eventual typed reader
// checks the whole document, including its separators and any trailing data.
fn fields(bytes: &[u8], open: u8) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut rest = trim(bytes).strip_prefix(&[open]).unwrap_or_default();
    let mut first = true;
    std::iter::from_fn(move || {
        if !first {
            rest = rest.strip_prefix(b",")?;
        }
        first = false;
        let key = if open == b'{' {
            let key = take_value(&mut rest)?;
            rest = rest.strip_prefix(b":")?;
            key
        } else {
            &[][..]
        };
        Some((key, take_value(&mut rest)?))
    })
}

fn named(key: &[u8], name: &str) -> bool {
    key.strip_prefix(b"\"").and_then(|s| s.strip_suffix(b"\"")).is_some_and(|s| s == name.as_bytes())
        || (key.contains(&b'\\') && serde_json::from_slice::<String>(key).is_ok_and(|s| s == name))
}

fn in_object<'a>(object: &'a [u8], visit: &mut impl FnMut(&'a [u8], &'a [u8])) {
    for (key, value) in fields(object, b'{') {
        if named(key, "family") {
            visit(key, value);
        }
    }
}

fn in_row_field<'a>(key: &[u8], value: &'a [u8], visit: &mut impl FnMut(&'a [u8], &'a [u8])) {
    if named(key, "battle") {
        in_object(value, visit);
    } else if named(key, "rotation") {
        for (_, entry) in fields(value, b'[') {
            in_object(entry, visit);
        }
    }
}

/// The released untagged master reader buffers every value before choosing
/// the envelope or bare row. Give it an ignored placeholder for each added
/// member. The chosen row then reads the original declarations.
pub(crate) fn master_shape(bytes: &[u8]) -> Cow<'_, [u8]> {
    mask_family(bytes, Some("server"), true, |_| true)
}

/// The released row reader skips non-text family members, including strings
/// inside them that have no UTF-8 representation. Keep invalid text in known
/// fields visible to the typed reader.
pub(crate) fn read_row<T: DeserializeOwned>(bytes: &[u8], envelope: Option<&str>) -> serde_json::Result<T> {
    match serde_json::from_slice(bytes) {
        Err(error) if std::str::from_utf8(bytes).is_err() => {
            let filtered = mask_family(bytes, envelope, envelope.is_none(), |value| {
                matches!(value.first(), Some(b'{' | b'[')) && std::str::from_utf8(value).is_err()
            });
            match filtered {
                Cow::Borrowed(_) => Err(error),
                Cow::Owned(filtered) => serde_json::from_slice(&filtered),
            }
        }
        result => result,
    }
}

fn mask_family<'a>(bytes: &'a [u8], envelope: Option<&str>, bare: bool, mask: impl Fn(&[u8]) -> bool) -> Cow<'a, [u8]> {
    let mut output = Vec::new();
    let mut cursor = 0;
    let mut replace = |key: &[u8], value: &[u8]| {
        if !mask(value) {
            return;
        }
        let start = key.as_ptr() as usize - bytes.as_ptr() as usize;
        let end = value.as_ptr() as usize - bytes.as_ptr() as usize + value.len();
        output.extend_from_slice(&bytes[cursor..start]);
        output.extend_from_slice(br#""_":null"#);
        cursor = end;
    };
    for (key, value) in fields(bytes, b'{') {
        if envelope.is_some_and(|name| named(key, name)) {
            for (key, value) in fields(value, b'{') {
                in_row_field(key, value, &mut replace);
            }
        } else if bare {
            in_row_field(key, value, &mut replace);
        }
    }
    if cursor == 0 {
        return Cow::Borrowed(bytes);
    }
    output.extend_from_slice(&bytes[cursor..]);
    Cow::Owned(output)
}
