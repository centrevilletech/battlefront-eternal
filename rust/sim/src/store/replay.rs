//! The store's side of the replay harness (rust/sim-replay, the `replay`
//! feature): reading a kind's state from the values a capture holds into
//! the store, and writing the store's state back as values to compare with
//! the capture's (`sim_replay::diff`). The generated `from_value` and
//! `to_value` (rust/sim/src/gen/) are written against this.
//!
//! A capture holds a singleton's state as its exporter gave it
//! (src/sim/runtime.js `exportState`): its fields by name in the schema's
//! order, and every entity of a kind the store holds as an object where it
//! is first met (the same object, the same node, wherever it is met again).
//! What the store only refers to (a unit, a body, a definition) is a handle
//! of the harness's: kind and key. `Replay` maps those to slots and
//! definition indices as it reads, and back as it writes, so a value comes
//! back with the handles it went in with.

use std::collections::HashMap;

pub use sim_replay::{Arena, Node, Value, V};

use super::{DefId, KindCode, Ref, StrId, Strings};
use crate::gen::{Store, KINDS, KIND_NAMES};

/// Why a value could not be read: where (its path) and what was wrong.
pub type Result<T> = std::result::Result<T, String>;

/// The kinds whose objects carry their own number: a handle's numeric key
/// is the slot.
fn keyed(kind: KindCode) -> bool { crate::gen::KIND_KEYED[kind as usize] }

#[derive(Default)]
struct Keys {
    /// A handle's key (as the harness wrote it) for each slot, and back.
    by_slot: Vec<Value>,
    by_key: HashMap<String, u32>,
}

/// The replay context: the store being read into or written from, the
/// handles met, the definitions met by table, and the entities met.
pub struct Replay {
    pub store: Store,
    pub strings: Strings,
    handles: Vec<Keys>,
    defs: HashMap<String, Keys>,
    entities_in: HashMap<u32, (KindCode, u32, u32)>,
    entities_out: HashMap<(KindCode, u32, u32), Value>,
}

impl Default for Replay {
    fn default() -> Self { Replay::new(Store::default()) }
}

/// A key's text, to find it again (a number's bits, or a string).
fn key_text(v: &V) -> Result<String> {
    match v.value {
        Value::Num(x) => Ok(format!("n{:016x}", x.to_bits())),
        Value::Str(s) => Ok(format!("s{s}")),
        _ => Err(format!("a handle key that is no number or string: {}", v.show())),
    }
}

impl Replay {
    pub fn new(store: Store) -> Self {
        Replay {
            store,
            strings: Strings::default(),
            handles: (0..=KINDS).map(|_| Keys::default()).collect(),
            defs: HashMap::new(),
            entities_in: HashMap::new(),
            entities_out: HashMap::new(),
        }
    }

    /// Forgets which entity values were met (a new record's values are a
    /// new arena): handles and definitions keep their slots and indices.
    pub fn next_record(&mut self) {
        self.entities_in.clear();
        self.entities_out.clear();
    }

    /// The slot a handle of a kind the store holds no fields of names: its
    /// number for a keyed kind, else the next slot for a key not met before.
    pub fn ref_in<T>(&mut self, kind: KindCode, v: V, path: &str) -> Result<Ref<T>> {
        let Some((name, key)) = v.handle() else { return Err(format!("{path}: not a handle ({})", v.show())) };
        if name != KIND_NAMES[kind as usize] {
            return Err(format!("{path}: a {name} handle where a {} belongs", KIND_NAMES[kind as usize]));
        }
        let keys = &mut self.handles[kind as usize];
        if keyed(kind) {
            let n = key.opt_num().ok_or_else(|| format!("{path}: a {name} key that is not a number"))?;
            if !(n >= 0.0 && n <= u32::MAX as f64 && n == n.trunc()) { return Err(format!("{path}: a {name} key of {n}")); }
            let slot = n as u32;
            let text = key_text(&key)?;
            keys.by_key.insert(text, slot);
            return Ok(Ref::new(slot, 0));
        }
        let text = key_text(&key)?;
        if let Some(&slot) = keys.by_key.get(&text) { return Ok(Ref::new(slot, 0)); }
        let slot = keys.by_slot.len() as u32;
        keys.by_slot.push(key.value.clone());
        keys.by_key.insert(text, slot);
        Ok(Ref::new(slot, 0))
    }

    /// The handle a slot of a kind the store holds no fields of was read as.
    pub fn ref_out<T>(&mut self, kind: KindCode, r: Ref<T>) -> Value {
        let name = KIND_NAMES[kind as usize];
        if keyed(kind) { return Value::handle(name, Value::Num(r.slot as f64)); }
        match self.handles[kind as usize].by_slot.get(r.slot as usize) {
            Some(key) => Value::handle(name, key.clone()),
            None => Value::handle(name, Value::Num(r.slot as f64)),
        }
    }

    /// A definition's index in its table: the harness names one by a
    /// "def" handle (its class, `class#2` for a second object of it).
    pub fn def_in(&mut self, table: &str, v: V, path: &str) -> Result<DefId> {
        let Some(("def", key)) = v.handle() else { return Err(format!("{path}: not a definition's handle ({})", v.show())) };
        let text = key_text(&key)?;
        let keys = self.defs.entry(table.to_string()).or_default();
        if let Some(&id) = keys.by_key.get(&text) { return Ok(DefId(id)); }
        let id = keys.by_slot.len() as u32;
        keys.by_slot.push(key.value.clone());
        keys.by_key.insert(text, id);
        Ok(DefId(id))
    }

    pub fn def_out(&mut self, table: &str, id: DefId) -> Value {
        match self.defs.get(table).and_then(|k| k.by_slot.get(id.0 as usize)) {
            Some(key) => Value::handle("def", key.clone()),
            None => Value::handle("def", Value::Num(id.0 as f64)),
        }
    }

    pub fn str_in(&mut self, v: V, path: &str) -> Result<StrId> {
        match v.value {
            Value::Str(s) => Ok(self.strings.intern_str(s)),
            Value::Str16(units) => Ok(self.strings.intern(units)),
            _ => Err(format!("{path}: not a string ({})", v.show())),
        }
    }

    pub fn str_out(&self, id: StrId) -> Value {
        let units = self.strings.units(id);
        match String::from_utf16(units) {
            Ok(s) => Value::Str(s),
            Err(_) => Value::Str16(units.to_vec()),
        }
    }

    /// An entity of a kind the store holds, met in a value: the one already
    /// made for that object, or `make` (which makes it and reads its fields).
    pub fn entity_in(&mut self, kind: KindCode, v: V, path: &str, make: impl FnOnce(&mut Replay) -> Result<(u32, u32)>) -> Result<(u32, u32)> {
        let Some(node) = v.node_id() else { return Err(format!("{path}: not an object ({})", v.show())) };
        if let Some(&(_, slot, gen)) = self.entities_in.get(&node) { return Ok((slot, gen)); }
        let (slot, gen) = make(self)?;
        self.entities_in.insert(node, (kind, slot, gen));
        Ok((slot, gen))
    }

    /// Notes an entity's slot as soon as it is made, before its fields are
    /// read (so a field that names it again finds it).
    pub fn entity_made(&mut self, kind: KindCode, v: V, slot: u32, gen: u32) {
        if let Some(node) = v.node_id() { self.entities_in.insert(node, (kind, slot, gen)); }
    }

    pub fn entity_out(&self, kind: KindCode, slot: u32, gen: u32) -> Option<Value> { self.entities_out.get(&(kind, slot, gen)).cloned() }

    pub fn entity_written(&mut self, kind: KindCode, slot: u32, gen: u32, value: &Value) { self.entities_out.insert((kind, slot, gen), value.clone()); }
}

// Reading a value's fields, with the path in every error.

pub fn num(v: V, path: &str) -> Result<f64> { v.opt_num().ok_or_else(|| format!("{path}: not a number ({})", v.show())) }

pub fn num_f32(v: V, path: &str) -> Result<f32> {
    let x = num(v, path)?;
    let f = x as f32;
    if x.is_nan() || f as f64 == x { Ok(f) } else { Err(format!("{path}: {x} is not an f32")) }
}

pub fn int(v: V, path: &str, min: f64, max: f64) -> Result<f64> {
    let x = num(v, path)?;
    if x >= min && x <= max && x == x.trunc() && !(x == 0.0 && x.is_sign_negative()) { Ok(x) } else { Err(format!("{path}: {x} is not a whole number from {min} to {max}")) }
}

pub fn boolean(v: V, path: &str) -> Result<bool> { v.opt_bool().ok_or_else(|| format!("{path}: not a boolean ({})", v.show())) }

pub fn vec3(v: V, path: &str) -> Result<[f64; 3]> {
    match v.node() {
        Some(Node::Vec3(x)) => Ok(*x),
        _ => Err(format!("{path}: not a Vector3 ({})", v.show())),
    }
}

pub fn quat(v: V, path: &str) -> Result<[f64; 4]> {
    match v.node() {
        Some(Node::Quat(x)) => Ok(*x),
        _ => Err(format!("{path}: not a Quaternion ({})", v.show())),
    }
}

pub fn numbers<const N: usize>(v: V, path: &str) -> Result<[f64; N]> {
    match v.node() {
        Some(Node::Array(items)) if items.len() == N => {
            let mut out = [0.0; N];
            for (i, x) in v.items().enumerate() { out[i] = num(x, path)?; }
            Ok(out)
        }
        _ => Err(format!("{path}: not {N} numbers ({})", v.show())),
    }
}

pub fn list<'a>(v: V<'a>, path: &str) -> Result<Vec<V<'a>>> {
    match v.node() {
        Some(Node::Array(_)) => Ok(v.items().collect()),
        _ => Err(format!("{path}: not an array ({})", v.show())),
    }
}

pub fn set<'a>(v: V<'a>, path: &str) -> Result<Vec<V<'a>>> {
    match v.node() {
        Some(Node::Set(_)) => Ok(v.items().collect()),
        _ => Err(format!("{path}: not a Set ({})", v.show())),
    }
}

/// An enum's index by its name.
pub fn name_of(v: V, names: &[&str], path: &str) -> Result<u8> {
    let s = v.str().ok_or_else(|| format!("{path}: not a string ({})", v.show()))?;
    names.iter().position(|n| *n == s).map(|i| i as u8).ok_or_else(|| format!("{path}: \"{s}\" is none of {}", names.join(", ")))
}

/// A record's or entity's value must be a plain object.
pub fn object(v: V, path: &str) -> Result<()> {
    match v.node() {
        Some(Node::Object { .. }) => Ok(()),
        _ => Err(format!("{path}: not an object ({})", v.show())),
    }
}
