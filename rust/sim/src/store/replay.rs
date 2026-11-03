//! The store's side of the replay harness (rust/sim-replay, the `replay`
//! feature): reading a kind's state from the values a capture holds into
//! the store, and writing the store's state back as values to compare with
//! the capture's (`sim_replay::diff`). The generated `from_value` and
//! `to_value` (rust/sim/src/gen/) are written against this.
//!
//! A capture holds a singleton's state as its exporter gave it
//! (src/sim/runtime.js `exportState`): its fields by name in the schema's
//! order, and every entity of a kind the store holds written out whole where
//! it lives (its home list), led by its own handle (`$id`). Any reference
//! to an entity (of a kind the store holds or not), and to a definition, is
//! a handle of the harness's: kind and key. `Replay` maps those to slots
//! and definition indices as it reads (an entity named before its value is
//! read is made then, and filled in when it is), and back as it writes, so a
//! value comes back with the handles it went in with.

use std::collections::HashMap;

pub use sim_replay::{Arena, Node, Value, V};

use super::{DefId, KindCode, Ref, StrId, Strings};

/// Why a value could not be read: where (its path) and what was wrong.
pub type Result<T> = std::result::Result<T, String>;

/// What a generated store tells the replay context about its kinds: their
/// names by code (a handle's kind is one) and whether each kind's objects
/// carry their own number (a handle's numeric key is then the slot).
pub trait StoreKinds: Default {
    const NAMES: &'static [&'static str];
    const KEYED: &'static [bool];
}

#[derive(Default)]
struct Stored {
    by_key: HashMap<String, (u32, u32)>,
    by_ref: HashMap<(u32, u32), Value>,
    /// The next number to give an entity made in the store, past every
    /// number a handle of the kind was read with.
    next: f64,
}

#[derive(Default)]
struct Keys {
    /// A handle's key (as the harness wrote it) for each slot, and back.
    by_slot: Vec<Value>,
    by_key: HashMap<String, u32>,
}

/// The replay context: the store being read into or written from, the
/// handles met, the definitions met by table, and the entities met. Each
/// generated store has its own (`crate::gen::Replay`).
pub struct ReplayOf<S> {
    pub store: S,
    pub strings: Strings,
    handles: Vec<Keys>,
    /// The entities of kinds the store holds, by their handles' keys, and
    /// back (slot and generation).
    stored: Vec<Stored>,
    defs: HashMap<String, Keys>,
}

impl<S: StoreKinds> Default for ReplayOf<S> {
    fn default() -> Self { ReplayOf::new(S::default()) }
}

/// A key's text, to find it again (a number's bits, or a string).
fn key_text(v: &V) -> Result<String> {
    match v.value {
        Value::Num(x) => Ok(format!("n{:016x}", x.to_bits())),
        Value::Str(s) => Ok(format!("s{s}")),
        _ => Err(format!("a handle key that is no number or string: {}", v.show())),
    }
}

impl<S: StoreKinds> ReplayOf<S> {
    pub fn new(store: S) -> Self {
        ReplayOf {
            store,
            strings: Strings::default(),
            handles: (0..S::NAMES.len()).map(|_| Keys::default()).collect(),
            stored: (0..S::NAMES.len()).map(|_| Stored::default()).collect(),
            defs: HashMap::new(),
        }
    }

    /// The slot a handle of a kind the store holds no fields of names: its
    /// number for a keyed kind, else the next slot for a key not met before.
    pub fn ref_in<T>(&mut self, kind: KindCode, v: V, path: &str) -> Result<Ref<T>> {
        let Some((name, key)) = v.handle() else { return Err(format!("{path}: not a handle ({})", v.show())) };
        if name != S::NAMES[kind as usize] {
            return Err(format!("{path}: a {name} handle where a {} belongs", S::NAMES[kind as usize]));
        }
        let keys = &mut self.handles[kind as usize];
        if S::KEYED[kind as usize] {
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
        let name = S::NAMES[kind as usize];
        if S::KEYED[kind as usize] { return Value::handle(name, Value::Num(r.slot as f64)); }
        match self.handles[kind as usize].by_slot.get(r.slot as usize) {
            Some(key) => Value::handle(name, key.clone()),
            None => Value::handle(name, Value::Num(r.slot as f64)),
        }
    }

    /// The entity of a kind the store holds that a handle names: the one met
    /// with that handle before, else the one `make` makes (given the key, if
    /// it is a number), whose fields its value fills in where it lives.
    pub fn stored_in(&mut self, kind: KindCode, v: V, path: &str, make: impl FnOnce(&mut Self, Option<u32>) -> (u32, u32)) -> Result<(u32, u32)> {
        let Some((name, key)) = v.handle() else { return Err(format!("{path}: not a handle ({})", v.show())) };
        if name != S::NAMES[kind as usize] {
            return Err(format!("{path}: a {name} handle where a {} belongs", S::NAMES[kind as usize]));
        }
        let text = key_text(&key)?;
        if let Some(&r) = self.stored[kind as usize].by_key.get(&text) { return Ok(r); }
        let number = key.opt_num().filter(|n| *n >= 0.0 && *n <= u32::MAX as f64 && *n == n.trunc()).map(|n| n as u32);
        let r = make(self, number);
        let s = &mut self.stored[kind as usize];
        if let Some(n) = key.opt_num() { if n + 1.0 > s.next { s.next = n + 1.0; } }
        s.by_key.insert(text, r);
        s.by_ref.insert(r, key.value.clone());
        Ok(r)
    }

    /// The handle of an entity of a kind the store holds: the one it was read
    /// with, else the next number of its kind (an entity the port made).
    pub fn stored_out(&mut self, kind: KindCode, slot: u32, gen: u32) -> Value {
        let name = S::NAMES[kind as usize];
        if S::KEYED[kind as usize] { return Value::handle(name, Value::Num(slot as f64)); }
        let s = &mut self.stored[kind as usize];
        if let Some(key) = s.by_ref.get(&(slot, gen)) { return Value::handle(name, key.clone()); }
        let key = Value::Num(s.next);
        s.next += 1.0;
        s.by_ref.insert((slot, gen), key.clone());
        Value::handle(name, key)
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

pub fn mat4(v: V, path: &str) -> Result<[f64; 16]> {
    match v.node() {
        Some(Node::Mat4(x)) => Ok(*x),
        _ => Err(format!("{path}: not a Matrix4 ({})", v.show())),
    }
}

/// A Matrix4's value (the harness's arena has no builder for one).
pub fn mat4_value(a: &mut Arena, m: [f64; 16]) -> Value {
    a.nodes.push(Node::Mat4(m));
    Value::Node(a.nodes.len() as u32 - 1)
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

/// A Map's entries (a key and a value each), in order.
pub fn map<'a>(v: V<'a>, path: &str) -> Result<Vec<(V<'a>, V<'a>)>> {
    match v.node() {
        Some(Node::Map(_)) => Ok(v.entries().collect()),
        _ => Err(format!("{path}: not a Map ({})", v.show())),
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
