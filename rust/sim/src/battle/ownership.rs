//! Persistent live-object rows and sparse phase publication. A command names
//! both the slab generation and the gameplay life, independently of JS ids.
use crate::gen::{F4Delta, F4Handle, F4ObjectKind};
use crate::store::{Ref, Slab};
use crate::store::block::{Block, HEADER};

/// A prepared object's compact reader record. Constructor tables and rigs
/// stay on the object; writing its reader record must not rebuild those tables.
pub trait Publish {
    const WORDS: usize;
    fn write(&self, row: &mut [f64], heap: &mut Vec<f64>);
}

struct Entry<T> {
    value: Option<T>,
    life: f64,
    dirty: bool,
    next: Vec<f64>,
    last: Vec<f64>,
    heap: Vec<f64>,
    delta: F4Delta,
}
impl<T: Publish> Entry<T> {
    fn new(value: T, life: f64, target: F4Handle) -> Self {
        let mut delta = F4Delta { target, ..F4Delta::default() };
        delta.mask.reserve(T::WORDS.div_ceil(32)); delta.values.reserve(T::WORDS);
        Self { value: Some(value), life, dirty: true, next: vec![0.0; T::WORDS],
            last: Vec::with_capacity(T::WORDS), heap: Vec::new(), delta }
    }
    fn prepare(&mut self, retired: bool) -> bool {
        if let Some(value) = &self.value {
            self.heap.clear(); value.write(&mut self.next, &mut self.heap);
        }
        // Variable reader records use the same numeric row+heap encoding as
        // ordinary store blocks. Current live rows have no variable payload.
        assert!(self.heap.is_empty(), "a fixed live row cannot publish a heap");
        self.delta.retired = retired; self.delta.length = T::WORDS as u32;
        self.delta.target.life = self.life;
        self.delta.mask.clear(); self.delta.mask.resize(T::WORDS.div_ceil(32), 0);
        self.delta.values.clear();
        for (i, &word) in self.next.iter().enumerate() {
            if self.last.get(i).is_some_and(|old| old.to_bits() == word.to_bits()) { continue; }
            self.delta.mask[i / 32] |= 1 << (i % 32);
            self.delta.values.push(word);
        }
        let changed = retired || self.last.len() != T::WORDS || !self.delta.values.is_empty();
        if changed { self.last.clear(); self.last.extend_from_slice(&self.next); }
        self.dirty = false; changed
    }
}

/// The order vector preserves creation order even when the slab reuses a slot.
/// Retired entries keep their own encoding until publication, so rebirth before
/// the next phase cannot overwrite the old life's final reader state.
pub struct Rows<T> {
    kind: F4ObjectKind,
    pending: bool,
    rows: Slab<Entry<T>>,
    order: Vec<Ref<Entry<T>>>,
    retired: Vec<Entry<T>>,
    selected: Vec<Ref<Entry<T>>>,
    retired_selected: Vec<usize>,
    pub publication: Block,
}
impl<T: Publish> Rows<T> {
    pub fn new(kind: F4ObjectKind, capacity: usize) -> Self {
        let mut publication = Block::default();
        publication.words.reserve(HEADER + capacity * (F4Delta::WORDS + T::WORDS + T::WORDS.div_ceil(32)));
        publication.heap.reserve(capacity * (T::WORDS + T::WORDS.div_ceil(32)));
        Self { kind, pending: false, rows: Slab::with_capacity(capacity), order: Vec::with_capacity(capacity),
            retired: Vec::with_capacity(capacity), selected: Vec::with_capacity(capacity),
            retired_selected: Vec::with_capacity(capacity), publication }
    }
    pub fn adopt(&mut self, life: f64, value: T) -> F4Handle {
        assert!(life.is_finite(), "finite gameplay life");
        let r = self.rows.insert(Entry::new(value, life, F4Handle::default()));
        let handle = F4Handle { kind: self.kind, slot: r.slot, generation: r.gen, life };
        self.rows.get_mut(r).unwrap().delta.target = handle.clone();
        self.order.push(r); self.pending = true;
        // Adoption prepares output and work storage too. A first phase after
        // cold construction must not grow buffers merely to publish births.
        let count = self.order.len() + self.retired.len();
        self.selected.reserve(count.saturating_sub(self.selected.len()));
        self.retired.reserve(count.saturating_sub(self.retired.len()));
        self.retired_selected.reserve(count.saturating_sub(self.retired_selected.len()));
        let masks = T::WORDS.div_ceil(32);
        let words = HEADER + count * (F4Delta::WORDS + T::WORDS + masks);
        self.publication.words.reserve(words.saturating_sub(self.publication.words.len()));
        self.publication.heap.reserve((count * (T::WORDS + masks)).saturating_sub(self.publication.heap.len()));
        handle
    }
    fn reference(&self, h: &F4Handle) -> Option<Ref<Entry<T>>> {
        if h.kind != self.kind { return None; }
        let r = Ref::new(h.slot, h.generation);
        self.rows.get(r).filter(|e| e.life == h.life).map(|_| r)
    }
    pub fn get(&self, h: &F4Handle) -> Option<&T> {
        self.reference(h).and_then(|r| self.rows.get(r).unwrap().value.as_ref())
    }
    /// Commands mutate canonical state and mark just their own row. Native
    /// phase work uses the same command entry, without copying through a facade.
    pub fn command(&mut self, h: &F4Handle, run: impl FnOnce(&mut T)) -> bool {
        let Some(r) = self.reference(h) else { return false };
        let row = self.rows.get_mut(r).unwrap();
        let Some(value) = &mut row.value else { return false };
        run(value); row.dirty = true; self.pending = true; true
    }
    /// Move a working rig out before native or foreign callbacks. No reference
    /// into this container then survives recursive damage on another row.
    /// The numeric snapshot remains publishable while the rig is checked out.
    pub fn checkout(&mut self, h: &F4Handle) -> Option<T> {
        let r = self.reference(h)?;
        let e = self.rows.get_mut(r).unwrap();
        let value = e.value.take()?;
        e.heap.clear(); value.write(&mut e.next, &mut e.heap);
        assert!(e.heap.is_empty(), "fixed working row"); e.dirty = true; self.pending = true;
        Some(value)
    }
    /// Commit the working frame before a callback can publish its changed row.
    /// Commands cannot mutate a checked-out rig, but retirement can preserve
    /// this exact snapshot before another construction reuses its slab slot.
    pub fn stage(&mut self, h: &F4Handle, value: &T) -> bool {
        self.stage_with(h, |row, heap| value.write(row, heap))
    }
    pub fn stage_with(&mut self, h: &F4Handle, write: impl FnOnce(&mut [f64], &mut Vec<f64>)) -> bool {
        let Some(r) = self.reference(h) else { return false };
        let e = self.rows.get_mut(r).unwrap();
        assert!(e.value.is_none(), "stage only a checked-out row");
        e.heap.clear(); write(&mut e.next, &mut e.heap);
        assert!(e.heap.is_empty(), "fixed working row"); e.dirty = true; self.pending = true; true
    }
    /// A recursive native reader sees the last committed working frame while
    /// its object is checked out. Ordinary reads use the live value instead.
    pub fn staged(&self, h: &F4Handle) -> Option<&[f64]> {
        let e = self.rows.get(self.reference(h)?)?;
        e.value.is_none().then_some(e.next.as_slice())
    }
    pub fn restore(&mut self, h: &F4Handle, value: T) -> Result<(), T> {
        let Some(r) = self.reference(h) else { return Err(value) };
        let e = self.rows.get_mut(r).unwrap();
        assert!(e.value.is_none(), "restore only a checked-out row");
        e.value = Some(value); e.dirty = true; self.pending = true; Ok(())
    }
    /// Native scheduler lists retain slab identity across a rebuild; external
    /// commands must still use reference(), which also validates the life.
    pub fn current(&self, h: &F4Handle) -> Option<F4Handle> {
        if h.kind != self.kind { return None; }
        let e = self.rows.get(Ref::new(h.slot, h.generation))?;
        Some(F4Handle { kind: self.kind, slot: h.slot, generation: h.generation, life: e.life })
    }
    pub fn lifecycle_command(&mut self, h: &F4Handle, run: impl FnOnce(&mut T) -> bool) -> bool {
        let Some(r) = self.reference(h) else { return false };
        let e = self.rows.get_mut(r).unwrap();
        let Some(value) = &mut e.value else { return false };
        if run(value) { e.life += 1.0; }
        e.dirty = true; self.pending = true; true
    }
    pub fn len(&self) -> usize { self.order.len() }
    pub fn is_empty(&self) -> bool { self.order.is_empty() }
    pub fn handle(&self, index: usize) -> F4Handle {
        let r = self.order[index]; let e = self.rows.get(r).unwrap();
        F4Handle { kind: self.kind, slot: r.slot, generation: r.gen, life: e.life }
    }
    /// Rebuild keeps the facade's slot but invalidates commands from the
    /// previous gameplay life. Replacing a pooled object uses retire/adopt.
    pub fn rebirth(&mut self, h: &F4Handle, life: f64, run: impl FnOnce(&mut T)) -> Option<F4Handle> {
        let r = self.reference(h)?;
        assert!(life.is_finite() && life != h.life, "a fresh gameplay life");
        let e = self.rows.get_mut(r).unwrap(); run(e.value.as_mut()?); e.life = life; e.dirty = true; self.pending = true;
        Some(F4Handle { kind: self.kind, slot: r.slot, generation: r.gen, life })
    }
    pub fn retire(&mut self, h: &F4Handle) -> bool {
        let Some(r) = self.reference(h) else { return false };
        self.retired.push(self.rows.remove(r).unwrap()); self.pending = true;
        self.order.retain(|old| *old != r); true
    }
    /// Called before a synchronous foreign reader and once at the phase's end.
    /// Unchanged rows, even if touched by a command, copy no scalar payload.
    pub fn flush(&mut self) -> usize {
        // Most unit observer barriers change no live object. Clear the previous
        // transfer without revisiting the prepared buildings, traps or rigs.
        if !self.pending {
            if self.publication.words.len() != HEADER { self.publication.begin(0, F4Delta::WORDS); self.publication.finish(); }
            return 0;
        }
        self.pending = false;
        self.selected.clear(); self.retired_selected.clear();
        for (i, e) in self.retired.iter_mut().enumerate() {
            e.prepare(true); self.retired_selected.push(i);
        }
        for &r in &self.order {
            let e = self.rows.get_mut(r).unwrap();
            // A new life must be published even if its gameplay scalars match.
            let life_changed = e.delta.target.life != e.life;
            if e.dirty && (e.prepare(false) || life_changed) {
                self.selected.push(r);
            }
        }
        self.publication.begin(self.retired_selected.len() + self.selected.len(), F4Delta::WORDS);
        let mut index = 0;
        for &i in &self.retired_selected {
            let (row, heap) = self.publication.row(index);
            self.retired[i].delta.put(row, heap); index += 1;
        }
        for &r in &self.selected {
            let (row, heap) = self.publication.row(index);
            self.rows.get(r).unwrap().delta.put(row, heap); index += 1;
        }
        self.publication.set_retired(self.retired_selected.len()); self.publication.finish();
        self.retired.clear(); index
    }
}
