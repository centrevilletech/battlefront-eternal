//! A kind's slab: its entities by slot, each slot with a generation.

use super::Ref;

struct Entry<T> {
    gen: u32,
    value: Option<T>,
}

/// Entities by slot. A slab is used one of two ways: its own slots
/// (`insert`, which takes the slot freed last, else a new one: no
/// JavaScript order depends on which), or slots its kind's JavaScript
/// numbers (`insert_at`: a unit's `id`, whose generation is 0).
///
/// Removing an entity bumps its slot's generation, so a reference to it is
/// seen to be stale (`get` gives None) rather than naming whatever takes the
/// slot next. Retiring one (`retire`) removes it the same way and keeps its
/// last state until the kind is next exported, so JavaScript that still
/// holds its mirror (an event that kept a grenade's position) sees that
/// state, as it saw the last state of the JavaScript object it held.
pub struct Slab<T> {
    entries: Vec<Entry<T>>,
    free: Vec<u32>,
    live: usize,
    /// Entities retired since the kind was last exported, in order.
    pub retired: Vec<(Ref<T>, T)>,
}

impl<T> Default for Slab<T> {
    fn default() -> Self { Slab::new() }
}

impl<T> Slab<T> {
    pub const fn new() -> Self {
        Slab { entries: Vec::new(), free: Vec::new(), live: 0, retired: Vec::new() }
    }

    /// Room for `n` entities in all, made now (the battle's memory is sized
    /// as it loads, docs/sim-store.md).
    pub fn with_capacity(n: usize) -> Self {
        Slab { entries: Vec::with_capacity(n), free: Vec::with_capacity(n), live: 0, retired: Vec::new() }
    }

    pub fn reserve(&mut self, n: usize) {
        let more = n.saturating_sub(self.entries.len());
        self.entries.reserve(more);
        self.free.reserve(more);
    }

    /// An entity in the slot freed last, or a new slot.
    pub fn insert(&mut self, value: T) -> Ref<T> {
        self.live += 1;
        if let Some(slot) = self.free.pop() {
            let e = &mut self.entries[slot as usize];
            e.value = Some(value);
            return Ref::new(slot, e.gen);
        }
        let slot = self.entries.len() as u32;
        self.entries.push(Entry { gen: 1, value: Some(value) });
        Ref::new(slot, 1)
    }

    /// An entity in the slot its JavaScript numbers (`slot`), with the
    /// generation given; the slot must be empty.
    pub fn insert_at(&mut self, slot: u32, gen: u32, value: T) -> Ref<T> {
        let i = slot as usize;
        while self.entries.len() <= i {
            self.entries.push(Entry { gen: 0, value: None });
        }
        let e = &mut self.entries[i];
        assert!(e.value.is_none(), "slot {slot} is taken");
        e.gen = gen;
        e.value = Some(value);
        self.live += 1;
        Ref::new(slot, gen)
    }

    /// Takes the entity out; its slot's generation moves on (never to 0).
    pub fn remove(&mut self, r: Ref<T>) -> Option<T> {
        let e = self.entries.get_mut(r.slot as usize)?;
        if e.gen != r.gen || e.value.is_none() { return None; }
        let value = e.value.take();
        e.gen = e.gen.wrapping_add(1).max(1);
        self.free.push(r.slot);
        self.live -= 1;
        value
    }

    /// Removes an entity, keeping its last state for the next export
    /// (`retired`); false if `r` is stale.
    pub fn retire(&mut self, r: Ref<T>) -> bool {
        match self.remove(r) {
            Some(value) => { self.retired.push((r, value)); true }
            None => false,
        }
    }

    #[inline]
    pub fn get(&self, r: Ref<T>) -> Option<&T> {
        let e = self.entries.get(r.slot as usize)?;
        if e.gen == r.gen { e.value.as_ref() } else { None }
    }

    #[inline]
    pub fn get_mut(&mut self, r: Ref<T>) -> Option<&mut T> {
        let e = self.entries.get_mut(r.slot as usize)?;
        if e.gen == r.gen { e.value.as_mut() } else { None }
    }

    pub fn contains(&self, r: Ref<T>) -> bool { self.get(r).is_some() }

    /// The entity in a slot, whatever its generation, with its reference.
    pub fn at(&self, slot: u32) -> Option<(Ref<T>, &T)> {
        let e = self.entries.get(slot as usize)?;
        e.value.as_ref().map(|v| (Ref::new(slot, e.gen), v))
    }

    pub fn at_mut(&mut self, slot: u32) -> Option<(Ref<T>, &mut T)> {
        let e = self.entries.get_mut(slot as usize)?;
        let gen = e.gen;
        e.value.as_mut().map(|v| (Ref::new(slot, gen), v))
    }

    /// How many entities it holds.
    pub fn len(&self) -> usize { self.live }
    pub fn is_empty(&self) -> bool { self.live == 0 }

    /// One past the highest slot it has used.
    pub fn slots(&self) -> u32 { self.entries.len() as u32 }

    /// Every entity in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (Ref<T>, &T)> + '_ {
        self.entries.iter().enumerate().filter_map(|(i, e)| e.value.as_ref().map(|v| (Ref::new(i as u32, e.gen), v)))
    }

    /// The entity `r` names, made (with its default fields) if its slot
    /// holds no entity of that generation, whatever else was there going:
    /// how JavaScript's objects of a kind it still owns are kept in step.
    pub fn ensure(&mut self, r: Ref<T>) -> &mut T
    where
        T: Default,
    {
        let i = r.slot as usize;
        while self.entries.len() <= i {
            self.free.push(self.entries.len() as u32);
            self.entries.push(Entry { gen: 0, value: None });
        }
        if self.entries[i].gen != r.gen || self.entries[i].value.is_none() {
            if self.entries[i].value.is_some() { self.live -= 1; } else if let Some(at) = self.free.iter().position(|&s| s == r.slot) { self.free.remove(at); }
            self.entries[i] = Entry { gen: r.gen, value: Some(T::default()) };
            self.live += 1;
        }
        self.entries[i].value.as_mut().unwrap()
    }

    /// Keeps the entities `keep` says to (in slot order), removing the rest.
    pub fn retain(&mut self, mut keep: impl FnMut(Ref<T>, &T) -> bool) {
        for i in 0..self.entries.len() {
            let gen = self.entries[i].gen;
            let drop = match &self.entries[i].value { Some(v) => !keep(Ref::new(i as u32, gen), v), None => false };
            if drop { self.remove(Ref::new(i as u32, gen)); }
        }
    }

    /// Empties it; every slot's generation moves on.
    pub fn clear(&mut self) {
        self.retired.clear();
        self.free.clear();
        for (i, e) in self.entries.iter_mut().enumerate().rev() {
            if e.value.take().is_some() { e.gen = e.gen.wrapping_add(1).max(1); }
            self.free.push(i as u32);
        }
        self.free.reverse();
        self.live = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_references_miss_and_freed_slots_come_back_last_first() {
        let mut s = Slab::new();
        let a = s.insert(1.5f64);
        let b = s.insert(2.5);
        let c = s.insert(3.5);
        assert_eq!((a.slot, b.slot, c.slot), (0, 1, 2));
        assert_eq!(s.remove(b), Some(2.5));
        assert_eq!(s.get(b), None);
        assert_eq!(s.remove(b), None);
        s.remove(a);
        let d = s.insert(4.5);
        assert_eq!((d.slot, d.gen), (0, 2));
        let e = s.insert(5.5);
        assert_eq!((e.slot, e.gen), (1, 2));
        assert_eq!(s.get(a), None);
        assert_eq!(s.get(d), Some(&4.5));
        assert_eq!(s.iter().map(|(r, v)| (r.slot, *v)).collect::<Vec<_>>(), vec![(0, 4.5), (1, 5.5), (2, 3.5)]);
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn ensure_follows_the_generation_javascript_gives() {
        let mut s: Slab<f64> = Slab::new();
        *s.ensure(Ref::new(3, 1)) = 1.0;
        *s.ensure(Ref::new(3, 1)) += 1.0;
        assert_eq!(s.get(Ref::new(3, 1)), Some(&2.0));
        *s.ensure(Ref::new(3, 2)) = 7.0;
        assert_eq!(s.get(Ref::new(3, 1)), None);
        assert_eq!(s.len(), 1);
        let r = s.insert(9.0);
        assert!(r.slot < 3);
        s.retain(|_, v| *v > 8.0);
        assert_eq!(s.len(), 1);
        assert_eq!(s.get(r), Some(&9.0));
    }

    #[test]
    fn a_retired_entity_keeps_its_last_state_and_frees_its_slot() {
        let mut s = Slab::new();
        let a = s.insert(1.0);
        assert!(s.retire(a));
        assert!(!s.retire(a));
        assert_eq!(s.get(a), None);
        assert_eq!(s.retired.iter().map(|(r, v)| (*r, *v)).collect::<Vec<_>>(), vec![(a, 1.0)]);
        let b = s.insert(2.0);
        assert_eq!((b.slot, b.gen), (a.slot, a.gen + 1));
    }

    #[test]
    fn numbered_slots_keep_their_number() {
        let mut s = Slab::new();
        let u = s.insert_at(7, 0, "unit 7");
        assert_eq!((u.slot, u.gen), (7, 0));
        assert_eq!(s.get(Ref::new(7, 0)), Some(&"unit 7"));
        assert_eq!(s.at(3).map(|(_, v)| *v), None);
        assert_eq!(s.slots(), 8);
        assert_eq!(s.len(), 1);
    }
}
