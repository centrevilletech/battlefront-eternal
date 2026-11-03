//! A JSON value as `JSON.parse` leaves it in JavaScript: numbers are f64,
//! and an object's properties are in JavaScript's order (array-index keys
//! first, ascending, then the others in the order first written; a key
//! written twice keeps its first place and its last value). Everything the
//! simulation reads of a content file reads it through this, so a loop over
//! an object's entries visits them as `Object.entries` would.
//!
//! Values are reference-counted: a subtree the JavaScript shares between
//! two places (a picked mission is both `world.mission` and
//! `world.missions[name].mission`) is one allocation here too, so walks
//! that skip what they have seen (the combat registry) can tell.

use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(Rc<str>),
    Array(Rc<Vec<Value>>),
    Object(Rc<Object>),
}

/// An object's own properties in JavaScript's iteration order. Lookups by
/// key go through a hash index once the object has more than a few keys
/// (the index is never iterated: order comes from `entries` alone).
#[derive(Clone, Debug, Default)]
pub struct Object {
    entries: Vec<(Rc<str>, Value)>,
    index: Option<HashMap<Rc<str>, usize>>,
}

const INDEXED: usize = 12;

/// Whether a key is an array index (a canonical decimal integer below
/// 2^32 - 1), which JavaScript orders before every other key.
pub fn array_index(key: &str) -> Option<u32> {
    let b = key.as_bytes();
    if b.is_empty() || b.len() > 10 || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if b.len() > 1 && b[0] == b'0' {
        return None;
    }
    let v: u64 = key.parse().ok()?;
    if v < 4294967295 {
        Some(v as u32)
    } else {
        None
    }
}

impl Object {
    pub fn new() -> Self {
        Object { entries: Vec::new(), index: None }
    }

    /// Builds an object from properties in the order they were written,
    /// applying JavaScript's ordering and duplicate rules: array-index keys
    /// first in ascending order, then the rest in first-written order; a
    /// repeated key keeps its first place and takes its last value.
    pub fn from_written(written: Vec<(Rc<str>, Value)>) -> Self {
        let mut firsts: Vec<(Rc<str>, Value)> = Vec::with_capacity(written.len());
        if written.len() <= INDEXED {
            for (key, value) in written {
                match firsts.iter_mut().find(|(k, _)| **k == *key) {
                    Some(slot) => slot.1 = value,
                    None => firsts.push((key, value)),
                }
            }
        } else {
            let mut seen: HashMap<Rc<str>, usize> = HashMap::with_capacity(written.len());
            for (key, value) in written {
                match seen.get(&key) {
                    Some(&at) => firsts[at].1 = value,
                    None => {
                        seen.insert(key.clone(), firsts.len());
                        firsts.push((key, value));
                    }
                }
            }
        }
        let mut indices: Vec<(u32, (Rc<str>, Value))> = Vec::new();
        let mut others: Vec<(Rc<str>, Value)> = Vec::with_capacity(firsts.len());
        for (key, value) in firsts {
            match array_index(&key) {
                Some(i) => indices.push((i, (key, value))),
                None => others.push((key, value)),
            }
        }
        indices.sort_by_key(|(i, _)| *i);
        let mut entries: Vec<(Rc<str>, Value)> = indices.into_iter().map(|(_, e)| e).collect();
        entries.extend(others);
        let mut object = Object { entries, index: None };
        object.reindex();
        object
    }

    fn reindex(&mut self) {
        self.index = if self.entries.len() > INDEXED {
            Some(self.entries.iter().enumerate().map(|(i, (k, _))| (k.clone(), i)).collect())
        } else {
            None
        };
    }

    fn position(&self, key: &str) -> Option<usize> {
        match &self.index {
            Some(index) => index.get(key).copied(),
            None => self.entries.iter().position(|(k, _)| &**k == key),
        }
    }

    /// `object[key] = value`: an existing key keeps its place.
    pub fn set(&mut self, key: Rc<str>, value: Value) {
        if let Some(at) = self.position(&key) {
            self.entries[at].1 = value;
            return;
        }
        match array_index(&key) {
            Some(index) => {
                // After every smaller index, before the first larger index or
                // the first string key.
                let at = self
                    .entries
                    .iter()
                    .position(|(k, _)| match array_index(k) {
                        Some(other) => other > index,
                        None => true,
                    })
                    .unwrap_or(self.entries.len());
                self.entries.insert(at, (key, value));
            }
            None => self.entries.push((key, value)),
        }
        self.reindex();
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.position(key).map(|at| &self.entries[at].1)
    }

    pub fn has(&self, key: &str) -> bool {
        self.position(key).is_some()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `Object.entries`, in order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.entries.iter().map(|(k, v)| (&**k, v))
    }

    /// `Object.keys`, in order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| &**k)
    }

    /// `Object.values`, in order.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.entries.iter().map(|(_, v)| v)
    }
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::String(Rc::from(s))
    }

    /// `value[key]` for an object (None for a missing key or a non-object).
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(o) => o.get(key),
            _ => None,
        }
    }

    /// `value[index]` for an array.
    pub fn at(&self, index: usize) -> Option<&Value> {
        match self {
            Value::Array(a) => a.get(index),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The number, if the value is one (no conversion).
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// JavaScript truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => !(*n == 0.0 || n.is_nan()),
            Value::String(s) => !s.is_empty(),
            Value::Array(_) | Value::Object(_) => true,
        }
    }

    /// Whether two values are the same allocation (JavaScript identity) for
    /// objects and arrays; primitives compare by SameValue.
    pub fn same(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
            (Value::Array(a), Value::Array(b)) => Rc::ptr_eq(a, b),
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()),
            (Value::String(a), Value::String(b)) => a == b,
            _ => false,
        }
    }
}

/// `value?.[key]` through an optional value.
pub fn get<'a>(value: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    value.and_then(|v| v.get(key))
}

/// A number field read as JavaScript reads `def.Key` where it expects a
/// number: Some only when the property holds a number.
pub fn num_field(value: Option<&Value>, key: &str) -> Option<f64> {
    get(value, key).and_then(Value::as_f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_follow_javascript_order() {
        let written = ["b", "a", "b", "10", "2", "01", "4294967295", "4294967294", "-1"]
            .iter()
            .enumerate()
            .map(|(i, k)| (Rc::from(*k), Value::Number(i as f64)))
            .collect();
        let o = Object::from_written(written);
        let keys: Vec<&str> = o.keys().collect();
        assert_eq!(keys, ["2", "10", "4294967294", "b", "a", "01", "4294967295", "-1"]);
        assert_eq!(o.get("b").and_then(Value::as_f64), Some(2.0));
    }
}
