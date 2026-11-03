//! The entity store: the simulation's state, held in Rust for the kinds of
//! entity Rust owns and copied in for the kinds JavaScript still owns that a
//! Rust phase reads (docs/sim-store.md).
//!
//! Each kind of entity (a bolt, a unit, a vehicle) is declared once in a
//! schema under rust/sim/schema/, and tools/sim-schema.mjs generates from
//! the schemas both sides of everything that crosses between JavaScript and
//! Rust: the Rust structs (`crate::gen`), the JavaScript mirror classes,
//! the loops that copy fields across (src/sim/gen/), and the exporters that
//! turn a kind's state into the values the replay harness records. This
//! module holds what the generated code is written against: handles,
//! slabs, strings and the sync blocks.
//!
//! Identity is JavaScript's object identity, kept by slot: an entity is a
//! slot in its kind's slab, and a reference to it is the slot and the
//! slot's generation, so a reference to a removed entity is seen to be
//! stale instead of naming the slot's next occupant. A kind whose
//! JavaScript objects carry their own number (a unit's `id`) uses that
//! number as its slot, with generation 0.

pub mod block;
pub mod calls;
#[cfg(feature = "replay")]
pub mod replay;
mod slab;
mod strings;

use std::marker::PhantomData;

pub use slab::Slab;
pub use strings::Strings;

/// A three.js Vector3 (x, y, z).
pub type Vec3 = [f64; 3];
/// A three.js Quaternion (x, y, z, w).
pub type Quat = [f64; 4];
/// A three.js Matrix4's elements (column-major).
pub type Mat4 = [f64; 16];

/// A kind's code: its place among the declared kinds' names in order,
/// from 1 (`crate::gen::kinds`); 0 is no kind.
pub type KindCode = u16;

/// A reference to an entity of kind `T`: its slot and the slot's
/// generation (0 for a kind whose objects carry their own number).
pub struct Ref<T> {
    pub slot: u32,
    pub gen: u32,
    kind: PhantomData<fn() -> T>,
}

impl<T> Ref<T> {
    #[inline(always)]
    pub const fn new(slot: u32, gen: u32) -> Self {
        Ref { slot, gen, kind: PhantomData }
    }
}

/// Slot 0 of generation 0: what a struct holding a reference starts as
/// before its fields are read (a generated `Default`), never an entity's.
impl<T> Default for Ref<T> {
    #[inline(always)]
    fn default() -> Self { Ref::new(0, 0) }
}

impl<T> Clone for Ref<T> {
    #[inline(always)]
    fn clone(&self) -> Self { *self }
}
impl<T> Copy for Ref<T> {}
impl<T> PartialEq for Ref<T> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool { self.slot == other.slot && self.gen == other.gen }
}
impl<T> Eq for Ref<T> {}
impl<T> std::fmt::Debug for Ref<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.slot, self.gen)
    }
}

/// A content definition (an ordnance class, a weapon, an explosion): its
/// index in the battle's table of that sort of definition (src/sim/runtime.js
/// `DefTable`; docs/sim-store.md, "Definitions").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefId(pub u32);

/// A string the simulation keeps (a name it compares, not a fixed set of
/// values: those are enums): its index in the instance's table of strings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StrId(pub u32);

/// A field that JavaScript sets to null at times and leaves undefined at
/// others: both are kept, as code can tell them apart (`=== null`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Nullable<T> {
    Value(T),
    Null,
    Undefined,
}

impl<T> Default for Nullable<T> {
    fn default() -> Self { Nullable::Undefined }
}

/// A JavaScript Set of references: insertion order kept (a Set iterates in
/// the order its members were first added), each member once.
pub struct RefSet<T> {
    pub items: Vec<Ref<T>>,
}

impl<T> Default for RefSet<T> {
    fn default() -> Self { RefSet { items: Vec::new() } }
}
impl<T> Clone for RefSet<T> {
    fn clone(&self) -> Self { RefSet { items: self.items.clone() } }
}
impl<T> PartialEq for RefSet<T> {
    fn eq(&self, other: &Self) -> bool { self.items == other.items }
}
impl<T> std::fmt::Debug for RefSet<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_set().entries(self.items.iter()).finish() }
}

impl<T> RefSet<T> {
    pub fn has(&self, r: Ref<T>) -> bool { self.items.contains(&r) }
    /// Set.add: a member already there keeps its place.
    pub fn add(&mut self, r: Ref<T>) {
        if !self.has(r) { self.items.push(r); }
    }
    /// Set.delete.
    pub fn delete(&mut self, r: Ref<T>) -> bool {
        match self.items.iter().position(|&x| x == r) {
            Some(i) => { self.items.remove(i); true }
            None => false,
        }
    }
    pub fn len(&self) -> usize { self.items.len() }
    pub fn is_empty(&self) -> bool { self.items.is_empty() }
}

/// A sync block's word that holds a slot, a generation, an index or a
/// count: a whole number from 0 to 2^32 - 1 by construction (the generated
/// loops on both sides write only those there).
#[inline(always)]
pub fn word_u32(w: f64) -> u32 {
    debug_assert!(w >= 0.0 && w <= u32::MAX as f64 && w == w.trunc(), "not a u32 word: {w}");
    w as u32
}

/// A sync block's word that holds an i32 field.
#[inline(always)]
pub fn word_i32(w: f64) -> i32 {
    debug_assert!(w >= i32::MIN as f64 && w <= i32::MAX as f64 && w == w.trunc(), "not an i32 word: {w}");
    w as i32
}

/// A JavaScript Map with string keys: its entries in insertion order (a
/// key set again keeps its place; one deleted and set again goes last).
#[derive(Clone, Debug, PartialEq)]
pub struct StrMap<V> {
    pub entries: Vec<(StrId, V)>,
}

impl<V> Default for StrMap<V> {
    fn default() -> Self { StrMap { entries: Vec::new() } }
}

impl<V> StrMap<V> {
    pub fn get(&self, key: StrId) -> Option<&V> { self.entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v) }
    pub fn get_mut(&mut self, key: StrId) -> Option<&mut V> { self.entries.iter_mut().find(|(k, _)| *k == key).map(|(_, v)| v) }
    pub fn has(&self, key: StrId) -> bool { self.get(key).is_some() }
    /// Map.set.
    pub fn set(&mut self, key: StrId, value: V) {
        match self.get_mut(key) {
            Some(v) => *v = value,
            None => self.entries.push((key, value)),
        }
    }
    /// Map.delete.
    pub fn delete(&mut self, key: StrId) -> bool {
        match self.entries.iter().position(|(k, _)| *k == key) {
            Some(i) => { self.entries.remove(i); true }
            None => false,
        }
    }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    pub fn clear(&mut self) { self.entries.clear(); }
}

/// Object.is for numbers: NaN is NaN whatever its bits, and -0 is not 0.
#[inline(always)]
pub fn same_f64(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// The number a plain object's key `key` is when it is an array index (a
/// whole number from 0 to 2^32 - 2: `obj[1]` and `obj[1.0]` are one key,
/// and `obj[-0]` is `obj[0]`), else None (`obj[1.5]` names the key "1.5",
/// which an object keyed by numbers never has).
#[inline]
pub fn array_index(key: f64) -> Option<f64> {
    if key >= 0.0 && key < 4294967295.0 && key == key.trunc() { Some(key + 0.0) } else { None }
}

/// A plain JavaScript object keyed by array indices (`obj:T`: a table by
/// team number, `{ 1: 150, 2: 150 }`). Its keys iterate in ascending numeric
/// order wherever they were set, as V8 orders an object's integer keys, and
/// a key never set reads as undefined (None).
#[derive(Clone, Debug, PartialEq)]
pub struct IntObj<V> {
    entries: Vec<(f64, V)>,
}

impl<V> Default for IntObj<V> {
    fn default() -> Self { IntObj { entries: Vec::new() } }
}

impl<V> IntObj<V> {
    pub fn new() -> Self { Self::default() }

    fn find(&self, key: f64) -> Result<usize, usize> {
        match array_index(key) {
            Some(k) => self.entries.binary_search_by(|(x, _)| x.partial_cmp(&k).unwrap()),
            None => Err(usize::MAX),
        }
    }

    /// `object[key]`, or None for undefined.
    pub fn get(&self, key: f64) -> Option<&V> {
        self.find(key).ok().map(|i| &self.entries[i].1)
    }

    pub fn get_mut(&mut self, key: f64) -> Option<&mut V> {
        match self.find(key) {
            Ok(i) => Some(&mut self.entries[i].1),
            Err(_) => None,
        }
    }

    /// `key in object`.
    pub fn has(&self, key: f64) -> bool { self.find(key).is_ok() }

    /// `object[key] = value`: the key takes its place in ascending order.
    /// Panics on a key that is no array index (JavaScript would make a
    /// string key of it, which this object does not hold).
    pub fn set(&mut self, key: f64, value: V) {
        match self.find(key) {
            Ok(i) => self.entries[i].1 = value,
            Err(usize::MAX) => panic!("an object keyed by numbers given the key {key}"),
            Err(i) => self.entries.insert(i, (array_index(key).unwrap(), value)),
        }
    }

    /// `delete object[key]`: whether it was there.
    pub fn delete(&mut self, key: f64) -> bool {
        match self.find(key) {
            Ok(i) => { self.entries.remove(i); true }
            Err(_) => false,
        }
    }

    /// Its entries in its keys' order (ascending), as `Object.entries`.
    pub fn iter(&self) -> impl Iterator<Item = (f64, &V)> + '_ { self.entries.iter().map(|(k, v)| (*k, v)) }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (f64, &mut V)> + '_ { self.entries.iter_mut().map(|(k, v)| (*k, v)) }

    /// Its keys, ascending (`Object.keys(object).map(Number)`).
    pub fn keys(&self) -> impl Iterator<Item = f64> + '_ { self.entries.iter().map(|(k, _)| *k) }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    pub fn clear(&mut self) { self.entries.clear(); }
}

impl<V: Copy> IntObj<V> {
    /// `object[key]` as a value (None: undefined).
    pub fn at(&self, key: f64) -> Option<V> { self.get(key).copied() }
}

impl<V> FromIterator<(f64, V)> for IntObj<V> {
    fn from_iter<I: IntoIterator<Item = (f64, V)>>(iter: I) -> Self {
        let mut o = IntObj::new();
        for (k, v) in iter { o.set(k, v); }
        o
    }
}

/// A JavaScript Map whose keys are numbers (`map:num:T`, `NumMap`) or
/// objects (`map:ref:K:T`, `RefMap`: a reference to an entity, or a union
/// of references): `jsrt::map::JsMap`, whose order is V8's (insertion; a key
/// set again keeps its place, one deleted and set again goes last; keys
/// compared as SameValueZero, -0 kept as 0; an entry added while a loop
/// runs over the map is visited: its cursor, `next_after`). Its methods are
/// JsMap's (`set`, `get`, `delete`, `has`, `size`, ...).
pub struct OrdMap<K: JsKey, V>(pub JsMap<K, V>);

pub type NumMap<V> = OrdMap<f64, V>;
pub type RefMap<K, V> = OrdMap<K, V>;

pub use jsrt::map::{JsKey, JsMap};

impl<K: JsKey, V> Default for OrdMap<K, V> {
    fn default() -> Self { OrdMap(JsMap::new()) }
}

impl<K: JsKey, V: Clone> Clone for OrdMap<K, V> {
    fn clone(&self) -> Self { OrdMap(self.0.clone()) }
}

impl<K: JsKey + std::fmt::Debug, V: std::fmt::Debug> std::fmt::Debug for OrdMap<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
}

/// The same entries in the same order (keys as SameValueZero).
impl<K: JsKey, V: PartialEq> PartialEq for OrdMap<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().zip(other.iter()).all(|((a, x), (b, y))| a.svz_eq(&b) && x == y)
    }
}

impl<K: JsKey, V> std::ops::Deref for OrdMap<K, V> {
    type Target = JsMap<K, V>;
    fn deref(&self) -> &JsMap<K, V> { &self.0 }
}

impl<K: JsKey, V> std::ops::DerefMut for OrdMap<K, V> {
    fn deref_mut(&mut self) -> &mut JsMap<K, V> { &mut self.0 }
}

impl<K: JsKey, V> OrdMap<K, V> {
    /// Its entries in order (`[...map]`, keys cloned), as a loop that does
    /// not change the map sees them.
    pub fn iter(&self) -> impl Iterator<Item = (K, &V)> + '_ {
        let mut cursor = 0;
        std::iter::from_fn(move || {
            let (i, k, v) = self.0.next_after(cursor)?;
            cursor = i + 1;
            Some((k, v))
        })
    }

    /// `map.size`.
    pub fn len(&self) -> usize { self.0.size() }
    pub fn is_empty(&self) -> bool { self.0.size() == 0 }
}

/// A Map's key's hash for an entity's reference (`variant`: its kind's place
/// in a union of kinds): it finds the key's slot in the table, nothing
/// observes it.
pub fn key_hash(variant: u32, slot: u32, gen: u32) -> u64 {
    let mut x = ((variant as u64) << 56) ^ ((gen as u64) << 32) ^ slot as u64;
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58476d1ce4e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

/// The same object: the same slot of the same generation.
impl<T> JsKey for Ref<T> {
    fn svz_eq(&self, other: &Self) -> bool { self == other }
    fn svz_hash(&self) -> u64 { key_hash(0, self.slot, self.gen) }
}

/// A typed array of a fixed length (`Float64Array(100)`, `Int32Array(100)`):
/// its elements as the array holds them (an Int32Array's i32s; a store into
/// one converts as JavaScript does, ToInt32, which the port does where the
/// JavaScript stores).
#[derive(Clone, Debug, PartialEq)]
pub struct TypedArray<T, const N: usize>(pub [T; N]);

impl<T: Copy + Default, const N: usize> Default for TypedArray<T, N> {
    fn default() -> Self { TypedArray([T::default(); N]) }
}

impl<T, const N: usize> std::ops::Deref for TypedArray<T, N> {
    type Target = [T; N];
    fn deref(&self) -> &[T; N] { &self.0 }
}

impl<T, const N: usize> std::ops::DerefMut for TypedArray<T, N> {
    fn deref_mut(&mut self) -> &mut [T; N] { &mut self.0 }
}
