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
