//! Plain-object key order, the order `Object.keys`, `Object.values`,
//! `Object.entries`, `for...in` and `JSON.stringify` observe:
//! integer-index keys first in ascending numeric order, then the
//! other string keys in insertion order. (Symbol keys never appear
//! in any of those; content JSON has none.) The sim observes this in
//! team tables (`Object.keys(teams)`), turret camera settings read
//! in JSON key order, and every `worldState` digest.

use crate::map::JsMap;
use crate::text::JsString;

/// Whether a key is an integer index ("array index" in the spec): a
/// canonical numeric string naming a uint32 other than 2^32 - 1,
/// i.e. `String(u) === key` for `u = ToUint32(key)` and
/// `u != 4294967295`. Returns the index. Checked on code units, so
/// a key held as a Rust string or as [`JsString`] agrees.
pub fn array_index_of_units(units: &[u16]) -> Option<u32> {
    if units.is_empty() || units.len() > 10 {
        return None;
    }
    // "0" is an index; any other leading zero is not canonical.
    if units.len() > 1 && units[0] == 0x30 {
        return None;
    }
    let mut value: u64 = 0;
    for &unit in units {
        if !(0x30..=0x39).contains(&unit) {
            return None;
        }
        value = value * 10 + (unit - 0x30) as u64;
    }
    if value >= 4294967295 {
        return None;
    }
    Some(value as u32)
}

/// [`array_index_of_units`] for a Rust string key.
pub fn array_index_of(key: &str) -> Option<u32> {
    if key.is_empty() || key.len() > 10 || !key.is_ascii() {
        return None;
    }
    let units: Vec<u16> = key.bytes().map(|b| b as u16).collect();
    array_index_of_units(&units)
}

/// Whether a key is an integer index.
pub fn is_array_index(key: &str) -> bool {
    array_index_of(key).is_some()
}

/// Object key order applied to keys in insertion order: indices
/// ascending, then the rest as inserted. Duplicate keys keep their
/// first insertion place (a repeated assignment in JavaScript does
/// not move a key), so callers pass each key once or accept
/// first-place-wins from the filter here.
pub fn ordered_object_keys(insertion: &[String]) -> Vec<String> {
    let mut indices: Vec<(u32, &String)> = Vec::new();
    let mut rest: Vec<&String> = Vec::new();
    for key in insertion {
        match array_index_of(key) {
            Some(i) => indices.push((i, key)),
            None => rest.push(key),
        }
    }
    indices.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out: Vec<String> = indices.into_iter().map(|(_, k)| k.clone()).collect();
    out.extend(rest.into_iter().cloned());
    out
}

/// A plain JavaScript object with string keys, backed by a `JsMap`
/// (which holds insertion order) and reporting its keys in object
/// order. Deleting and re-adding a non-index key moves it to the end
/// of the string keys, as in JavaScript; index keys always report
/// in numeric order wherever they were inserted.
pub struct JsObject<V> {
    map: JsMap<String, V>,
}

impl<V> Default for JsObject<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: Clone> Clone for JsObject<V> {
    fn clone(&self) -> Self {
        JsObject { map: self.map.clone() }
    }
}

impl<V: core::fmt::Debug> core::fmt::Debug for JsObject<V> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut m = f.debug_map();
        for key in self.keys() {
            m.entry(&key, self.map.get(&key).unwrap());
        }
        m.finish()
    }
}

impl<V> JsObject<V> {
    /// `{}`.
    pub fn new() -> Self {
        JsObject { map: JsMap::new() }
    }

    /// The number of keys.
    pub fn len(&self) -> usize {
        self.map.size()
    }

    pub fn is_empty(&self) -> bool {
        self.map.size() == 0
    }

    /// `obj[key] = value`.
    pub fn set(&mut self, key: String, value: V) -> Option<V> {
        self.map.set(key, value)
    }

    /// `obj[key]`.
    pub fn get(&self, key: &str) -> Option<&V> {
        self.map.get(&key.to_string())
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        self.map.get_mut(&key.to_string())
    }

    /// `key in obj`.
    pub fn has(&self, key: &str) -> bool {
        self.map.has(&key.to_string())
    }

    /// `delete obj[key]`.
    pub fn delete(&mut self, key: &str) -> bool {
        self.map.delete(&key.to_string())
    }

    /// `Object.keys(obj)`.
    pub fn keys(&self) -> Vec<String> {
        ordered_object_keys(&self.map.keys())
    }

    /// `Object.values(obj)`, in key order.
    pub fn values(&self) -> Vec<&V> {
        self.keys()
            .into_iter()
            .map(|k| self.map.get(&k).unwrap())
            .collect()
    }

    /// `Object.entries(obj)`, in key order.
    pub fn entries_ref(&self) -> Vec<(String, &V)> {
        self.keys()
            .into_iter()
            .map(|k| {
                let v = self.map.get(&k).unwrap();
                (k, v)
            })
            .collect()
    }
}

impl<V: Clone> JsObject<V> {
    /// `Object.entries(obj)`, owned.
    pub fn entries(&self) -> Vec<(String, V)> {
        self.entries_ref()
            .into_iter()
            .map(|(k, v)| (k, v.clone()))
            .collect()
    }
}

/// A key held as code units, for objects whose keys arrive as
/// [`JsString`] (wire text, content keys). Same order rules.
pub fn ordered_object_keys_units(insertion: &[JsString]) -> Vec<JsString> {
    let mut indices: Vec<(u32, &JsString)> = Vec::new();
    let mut rest: Vec<&JsString> = Vec::new();
    for key in insertion {
        match array_index_of_units(key) {
            Some(i) => indices.push((i, key)),
            None => rest.push(key),
        }
    }
    indices.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out: Vec<JsString> = indices.into_iter().map(|(_, k)| k.clone()).collect();
    out.extend(rest.into_iter().cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn array_index_recognition() {
        assert_eq!(array_index_of("0"), Some(0));
        assert_eq!(array_index_of("42"), Some(42));
        assert_eq!(array_index_of("4294967294"), Some(4294967294));
        assert_eq!(array_index_of("4294967295"), None);
        assert_eq!(array_index_of("4294967296"), None);
        assert_eq!(array_index_of("00"), None);
        assert_eq!(array_index_of("01"), None);
        assert_eq!(array_index_of("-1"), None);
        assert_eq!(array_index_of("+1"), None);
        assert_eq!(array_index_of("1.0"), None);
        assert_eq!(array_index_of(" 1"), None);
        assert_eq!(array_index_of("1e2"), None);
        assert_eq!(array_index_of(""), None);
    }

    #[test]
    fn object_keys_indices_first_then_insertion() {
        let mut o: JsObject<i32> = JsObject::new();
        o.set("b".into(), 1);
        o.set("10".into(), 2);
        o.set("2".into(), 3);
        o.set("a".into(), 4);
        o.set("4294967295".into(), 5); // not an index: string key
        assert_eq!(o.keys(), vec!["2", "10", "b", "a", "4294967295"]);
        o.delete("b");
        o.set("b".into(), 6);
        assert_eq!(o.keys(), vec!["2", "10", "a", "4294967295", "b"]);
    }
}
