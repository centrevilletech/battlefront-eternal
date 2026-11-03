//! `JsMap` and `JsSet`: JavaScript's `Map` and `Set` with V8's
//! ordered-hash-table semantics, which the sim observes everywhere:
//!
//! - iteration is insertion order;
//! - `set` on a live key keeps its place; only its value changes;
//! - `delete` leaves a hole: the entry is skipped from then on;
//! - a key set again after deletion goes to the end;
//! - an entry added while an iteration is under way is visited, as
//!   long as the iteration has not already run off the end; an entry
//!   deleted before its turn is skipped; a deleted-and-re-added
//!   entry is visited (again) at its new place at the end.
//!
//! Keys compare by SameValueZero (NaN equals NaN, +0 equals -0), and
//! a -0 key is stored as +0, as the spec demands of `Map`/`Set`.
//!
//! Storage is an insertion-ordered slot list with tombstones plus an
//! open-addressing index from key to slot. The index's probe order
//! is never observed; the slot list is the iteration order. No
//! `HashMap`: its iteration order is unspecified and must never
//! reach a tick. Iteration under mutation goes through the cursor
//! methods ([`JsMap::next_after`]), which keep an index into the
//! slot list exactly as V8's ordered hash table iterator keeps its
//! entry index: slots never move, so a cursor survives any mutation
//! the loop body makes, with the visiting rules above. (`clear`
//! empties the slot list; a JavaScript iterator created before a
//! `clear` likewise sees nothing after it, except entries added
//! after the clear at their fresh indices — a cursor, being an
//! index, reproduces even that.)

/// A key a `JsMap`/`JsSet` can hold, with SameValueZero spelled out.
pub trait JsKey: Clone {
    /// SameValueZero equality.
    fn svz_eq(&self, other: &Self) -> bool;
    /// A hash that agrees with `svz_eq` (equal keys hash alike). The
    /// hash only finds slots; it is never observed.
    fn svz_hash(&self) -> u64;
    /// The stored form of the key: the identity for almost every
    /// type, but a number key of -0 is stored as +0 (and every NaN
    /// in one canonical pattern, so lookups find it).
    fn canon(&self) -> Self {
        self.clone()
    }
}

fn mix64(mut x: u64) -> u64 {
    // SplitMix64's finalizer: plenty for an unobserved slot index.
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58476d1ce4e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

macro_rules! int_key {
    ($t:ty) => {
        impl JsKey for $t {
            fn svz_eq(&self, other: &Self) -> bool {
                self == other
            }
            fn svz_hash(&self) -> u64 {
                mix64(*self as u64 ^ 0x9e3779b97f4a7c15)
            }
        }
    };
}

int_key!(u8);
int_key!(u16);
int_key!(u32);
int_key!(u64);
int_key!(usize);
int_key!(i32);
int_key!(i64);

impl JsKey for bool {
    fn svz_eq(&self, other: &Self) -> bool {
        self == other
    }
    fn svz_hash(&self) -> u64 {
        mix64(*self as u64 ^ 0x2545f4914f6cdd1d)
    }
}

impl JsKey for f64 {
    fn svz_eq(&self, other: &Self) -> bool {
        crate::same_value_zero(*self, *other)
    }
    fn svz_hash(&self) -> u64 {
        mix64(self.canon().to_bits())
    }
    fn canon(&self) -> Self {
        if self.is_nan() {
            f64::NAN
        } else if *self == 0.0 {
            0.0 // -0 is stored as +0
        } else {
            *self
        }
    }
}

impl JsKey for String {
    fn svz_eq(&self, other: &Self) -> bool {
        self == other
    }
    fn svz_hash(&self) -> u64 {
        // FNV-1a over the UTF-8 bytes: a key's identity only.
        let mut h = 0xcbf29ce484222325u64;
        for &b in self.as_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

impl JsKey for Vec<u16> {
    fn svz_eq(&self, other: &Self) -> bool {
        self == other
    }
    fn svz_hash(&self) -> u64 {
        let mut h = 0xcbf29ce484222325u64;
        for &unit in self.iter() {
            h ^= unit as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= (unit >> 8) as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}

struct Slot<K, V> {
    key: K,
    value: V,
    live: bool,
}

/// JavaScript's `Map`, ordered. See the module notes.
pub struct JsMap<K: JsKey, V> {
    slots: Vec<Slot<K, V>>,
    // Open addressing: each bucket is a slot index + 1, 0 empty.
    // Deleted slots stay findable until rehash so probing remains
    // sound; liveness is on the slot.
    buckets: Vec<usize>,
    live: usize,
    // Buckets holding a key (live or deleted), for the load factor.
    filled: usize,
}

impl<K: JsKey, V> Default for JsMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: JsKey, V: Clone> Clone for JsMap<K, V> {
    fn clone(&self) -> Self {
        let mut out = JsMap::new();
        for (k, v) in self.entries() {
            out.set(k, v);
        }
        out
    }
}

impl<K: JsKey, V: core::fmt::Debug> core::fmt::Debug for JsMap<K, V>
where
    K: core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut m = f.debug_map();
        for slot in &self.slots {
            if slot.live {
                m.entry(&slot.key, &slot.value);
            }
        }
        m.finish()
    }
}

impl<K: JsKey, V> JsMap<K, V> {
    /// `new Map()`.
    pub fn new() -> Self {
        JsMap {
            slots: Vec::new(),
            buckets: Vec::new(),
            live: 0,
            filled: 0,
        }
    }

    /// `map.size`.
    pub fn size(&self) -> usize {
        self.live
    }

    fn find_slot(&self, key: &K) -> Option<usize> {
        if self.buckets.is_empty() {
            return None;
        }
        let mask = self.buckets.len() - 1;
        let mut i = (key.svz_hash() as usize) & mask;
        loop {
            let b = self.buckets[i];
            if b == 0 {
                return None;
            }
            let slot = &self.slots[b - 1];
            if slot.key.svz_eq(key) {
                return if slot.live { Some(b - 1) } else { None };
            }
            i = (i + 1) & mask;
        }
    }

    fn rehash(&mut self, capacity: usize) {
        self.buckets = vec![0; capacity];
        self.filled = 0;
        let mask = capacity - 1;
        for (index, slot) in self.slots.iter().enumerate() {
            if !slot.live {
                continue;
            }
            let mut i = (slot.key.svz_hash() as usize) & mask;
            while self.buckets[i] != 0 {
                i = (i + 1) & mask;
            }
            self.buckets[i] = index + 1;
            self.filled += 1;
        }
    }

    fn ensure_capacity(&mut self) {
        if self.buckets.is_empty() {
            self.rehash(16);
        } else if (self.filled + 1) * 4 > self.buckets.len() * 3 {
            let want = if self.live * 2 <= self.buckets.len() {
                self.buckets.len()
            } else {
                self.buckets.len() * 2
            };
            self.rehash(want);
        }
    }

    /// `map.set(key, value)`: a live key keeps its place; a fresh or
    /// deleted key goes to the end. Returns the previous value of a
    /// live key, if any (JavaScript returns the map; this is the
    /// half callers can use).
    pub fn set(&mut self, key: K, value: V) -> Option<V> {
        let key = key.canon();
        if let Some(index) = self.find_slot(&key) {
            return Some(core::mem::replace(&mut self.slots[index].value, value));
        }
        self.ensure_capacity();
        let index = self.slots.len();
        self.slots.push(Slot { key, value, live: true });
        let mask = self.buckets.len() - 1;
        let mut i = (self.slots[index].key.svz_hash() as usize) & mask;
        loop {
            let b = self.buckets[i];
            if b == 0 || !self.slots[b - 1].live {
                if b == 0 {
                    self.filled += 1;
                }
                self.buckets[i] = index + 1;
                break;
            }
            i = (i + 1) & mask;
        }
        self.live += 1;
        None
    }

    /// `map.get(key)`.
    pub fn get(&self, key: &K) -> Option<&V> {
        self.find_slot(&key.canon()).map(|i| &self.slots[i].value)
    }

    /// Mutable `map.get(key)`, for state the JavaScript mutates in
    /// place through the reference it holds.
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        match self.find_slot(&key.canon()) {
            Some(i) => Some(&mut self.slots[i].value),
            None => None,
        }
    }

    /// `map.has(key)`.
    pub fn has(&self, key: &K) -> bool {
        self.find_slot(&key.canon()).is_some()
    }

    /// `map.delete(key)`: whether a live entry was removed. The slot
    /// stays as a hole in the order, exactly as V8's table leaves it.
    pub fn delete(&mut self, key: &K) -> bool {
        match self.find_slot(&key.canon()) {
            Some(i) => {
                self.slots[i].live = false;
                self.live -= 1;
                true
            }
            None => false,
        }
    }

    /// `map.clear()`.
    pub fn clear(&mut self) {
        self.slots.clear();
        self.buckets.clear();
        self.live = 0;
        self.filled = 0;
    }

    /// The keys, in iteration order (`[...map.keys()]`).
    pub fn keys(&self) -> Vec<K> {
        self.slots
            .iter()
            .filter(|s| s.live)
            .map(|s| s.key.clone())
            .collect()
    }

    /// The values, in iteration order.
    pub fn values(&self) -> Vec<&V> {
        self.slots
            .iter()
            .filter(|s| s.live)
            .map(|s| &s.value)
            .collect()
    }

    /// The entries, in iteration order (`[...map.entries()]`, keys
    /// cloned and values borrowed).
    pub fn entries_ref(&self) -> Vec<(K, &V)> {
        self.slots
            .iter()
            .filter(|s| s.live)
            .map(|s| (s.key.clone(), &s.value))
            .collect()
    }

    /// One step of a Map iterator: the next live entry at or after
    /// `cursor` (a slot index, advance it past what is returned),
    /// with the slot index it was found at. This is the iteration
    /// that keeps JavaScript's visiting rules while the loop body
    /// mutates the map — entries appended after the cursor are
    /// reached, entries deleted before their turn are skipped. A
    /// finished JavaScript iterator stays finished even if entries
    /// are added later; a cursor cannot tell that state, so loops
    /// that must reproduce it keep their own `done` flag once this
    /// returns None.
    pub fn next_after(&self, cursor: usize) -> Option<(usize, K, &V)> {
        let mut i = cursor;
        while i < self.slots.len() {
            let slot = &self.slots[i];
            if slot.live {
                return Some((i, slot.key.clone(), &slot.value));
            }
            i += 1;
        }
        None
    }

    /// `map.forEach(callback)` with the visiting rules: the callback
    /// receives each key and may mutate the map (it gets `&mut`
    /// self); values are re-read at their turn, so a value replaced
    /// before its turn is seen replaced, as in JavaScript.
    pub fn for_each_key(&mut self, mut f: impl FnMut(&mut Self, K)) {
        let mut cursor = 0usize;
        loop {
            let found = {
                let mut i = cursor;
                let mut hit = None;
                while i < self.slots.len() {
                    if self.slots[i].live {
                        hit = Some((i, self.slots[i].key.clone()));
                        break;
                    }
                    i += 1;
                }
                hit
            };
            match found {
                None => return,
                Some((i, key)) => {
                    cursor = i + 1;
                    f(self, key);
                }
            }
        }
    }
}

impl<K: JsKey, V: Clone> JsMap<K, V> {
    /// The entries, in iteration order, owned.
    pub fn entries(&self) -> Vec<(K, V)> {
        self.slots
            .iter()
            .filter(|s| s.live)
            .map(|s| (s.key.clone(), s.value.clone()))
            .collect()
    }
}

/// JavaScript's `Set`, ordered: a `JsMap` whose values are its keys,
/// as the spec models it.
pub struct JsSet<K: JsKey> {
    map: JsMap<K, ()>,
}

impl<K: JsKey> Default for JsSet<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: JsKey> Clone for JsSet<K> {
    fn clone(&self) -> Self {
        JsSet { map: self.map.clone() }
    }
}

impl<K: JsKey + core::fmt::Debug> core::fmt::Debug for JsSet<K> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_set().entries(self.values()).finish()
    }
}

impl<K: JsKey> JsSet<K> {
    /// `new Set()`.
    pub fn new() -> Self {
        JsSet { map: JsMap::new() }
    }

    /// `set.size`.
    pub fn size(&self) -> usize {
        self.map.size()
    }

    /// `set.add(value)`: whether it was new (JavaScript returns the
    /// set).
    pub fn add(&mut self, value: K) -> bool {
        self.map.set(value, ()).is_none()
    }

    /// `set.has(value)`.
    pub fn has(&self, value: &K) -> bool {
        self.map.has(value)
    }

    /// `set.delete(value)`.
    pub fn delete(&mut self, value: &K) -> bool {
        self.map.delete(value)
    }

    /// `set.clear()`.
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// The values, in iteration order. A -0 added reads back as +0.
    pub fn values(&self) -> Vec<K> {
        self.map.keys()
    }

    /// One step of a Set iterator: see [`JsMap::next_after`].
    pub fn next_after(&self, cursor: usize) -> Option<(usize, K)> {
        self.map.next_after(cursor).map(|(i, k, _)| (i, k))
    }

    /// `set.forEach(callback)` with the visiting rules.
    pub fn for_each_value(&mut self, f: impl FnMut(&mut Self, K)) {
        // Run over the inner map, handing the set back to the body.
        let mut f = f;
        let mut cursor = 0usize;
        loop {
            match self.map.next_after(cursor) {
                None => return,
                Some((i, key, _)) => {
                    cursor = i + 1;
                    f(self, key);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insertion_order_and_reinsertion() {
        let mut m: JsMap<String, i32> = JsMap::new();
        m.set("a".into(), 1);
        m.set("b".into(), 2);
        m.set("c".into(), 3);
        m.set("b".into(), 20); // keeps its place
        assert_eq!(m.keys(), vec!["a", "b", "c"]);
        assert_eq!(m.get(&"b".to_string()), Some(&20));
        assert!(m.delete(&"b".to_string()));
        m.set("b".into(), 200); // goes to the end
        assert_eq!(m.keys(), vec!["a", "c", "b"]);
        assert_eq!(m.size(), 3);
        assert!(m.delete(&"b".to_string()));
        assert!(!m.delete(&"b".to_string()));
    }

    #[test]
    fn number_keys_use_same_value_zero() {
        let mut m: JsMap<f64, i32> = JsMap::new();
        m.set(-0.0, 1);
        assert_eq!(m.get(&0.0), Some(&1));
        assert_eq!(m.keys()[0].to_bits(), 0.0f64.to_bits(), "-0 stored as +0");
        m.set(f64::NAN, 2);
        assert_eq!(m.get(&f64::NAN), Some(&2));
        m.set(f64::from_bits(0x7ff8000000000001), 3);
        assert_eq!(m.size(), 2, "every NaN is the one key");
        assert_eq!(m.get(&f64::NAN), Some(&3));
    }

    #[test]
    fn iteration_visits_entries_added_during_it() {
        // for (const k of map.keys()) if (k === "a") { map.delete("b"); map.set("d", 4); }
        let mut m: JsMap<String, i32> = JsMap::new();
        m.set("a".into(), 1);
        m.set("b".into(), 2);
        m.set("c".into(), 3);
        let mut seen = Vec::new();
        let mut cursor = 0;
        while let Some((i, key, _)) = m.next_after(cursor) {
            cursor = i + 1;
            seen.push(key.clone());
            if key == "a" {
                m.delete(&"b".to_string());
                m.set("d".into(), 4);
            }
        }
        assert_eq!(seen, vec!["a", "c", "d"]);
    }

    #[test]
    fn deleted_and_readded_is_visited_again_at_the_end() {
        let mut s: JsSet<String> = JsSet::new();
        s.add("a".into());
        s.add("b".into());
        let mut seen = Vec::new();
        let mut cursor = 0;
        let mut moved = false;
        while let Some((i, key)) = s.next_after(cursor) {
            cursor = i + 1;
            seen.push(key.clone());
            if key == "a" && !moved {
                moved = true;
                s.delete(&"a".to_string());
                s.add("a".into());
            }
        }
        assert_eq!(seen, vec!["a", "b", "a"]);
    }

    #[test]
    fn set_minus_zero_reads_back_plus_zero() {
        let mut s: JsSet<f64> = JsSet::new();
        s.add(-0.0);
        assert_eq!(s.values()[0].to_bits(), 0.0f64.to_bits());
        assert!(s.has(&0.0));
    }

    #[test]
    fn many_keys_stay_ordered() {
        let mut m: JsMap<u32, u32> = JsMap::new();
        for i in 0..5000u32 {
            m.set(i * 2654435761 % 100003, i);
        }
        for i in (0..5000u32).step_by(3) {
            m.delete(&(i * 2654435761 % 100003));
        }
        let keys = m.keys();
        assert_eq!(keys.len(), m.size());
        for k in &keys {
            assert!(m.has(k));
        }
    }
}
