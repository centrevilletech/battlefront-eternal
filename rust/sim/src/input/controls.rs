//! runtime/action-queue.js and runtime/commands.js CommandControls.
//! Opaque value tokens preserve Set/Map identity at the host boundary.
use crate::gen::{IcInput, IcOperation, IcResult, IqCount, IqInput, IqOperation, IqResult, IqSlot, IqState};
use crate::store::{word_u32, Strings};

// The adapter reserves these tokens for the values whose queue semantics
// differ from ordinary names. Null is also the deleted-slot marker.
const NULL: f64 = 0.0;
const UNDEFINED: f64 = 1.0;
const NAN: f64 = 2.0;
const ZERO: f64 = 3.0;
const NEGATIVE_ZERO: f64 = 4.0;
fn canonical(name: f64) -> f64 { if name == NEGATIVE_ZERO { ZERO } else { name } }
fn equal(a: f64, b: f64) -> bool { canonical(a) == canonical(b) }

struct Queue {
    state: IqState,
    items: Vec<Option<f64>>,
    replaced_items: bool,
}

impl Queue {
    fn new(state: IqState) -> Self {
        let mut items = vec![None; word_u32(state.items_length) as usize];
        for slot in &state.items { items[word_u32(slot.index) as usize] = Some(slot.value); }
        Self { state, items, replaced_items: false }
    }
    fn index(&self, i: f64) -> usize { word_u32(i % self.state.capacity) as usize }
    fn item(&self, i: f64) -> f64 { self.items.get(self.index(i)).copied().flatten().unwrap_or(UNDEFINED) }
    fn set(&mut self, i: f64, value: f64) {
        let index = self.index(i);
        if self.items.len() <= index { self.items.resize(index + 1, None); }
        self.items[index] = Some(value);
    }
    fn count(&self, name: f64) -> f64 { self.state.counts.iter().find(|v| equal(v.name, name)).map_or(0.0, |v| v.count) }
    fn has(&self, name: f64) -> bool { self.count(name) > 0.0 }
    fn count_set(&mut self, name: f64, count: f64) {
        if let Some(entry) = self.state.counts.iter_mut().find(|v| equal(v.name, name)) { entry.count = count; }
        else { self.state.counts.push(IqCount { name: canonical(name), count }); }
    }
    fn values(&self) -> Vec<f64> {
        let mut values = Vec::new();
        let mut i = self.state.read;
        while i < self.state.write {
            let value = self.item(i);
            if value != NULL { values.push(value); }
            i += 1.0;
        }
        values
    }
    fn clear(&mut self) {
        self.state.read = 0.0; self.state.write = 0.0; self.state.length = 0.0; self.state.counts.clear();
    }
    fn add(&mut self, name: f64) -> Result<(), &'static str> {
        if self.state.write - self.state.read == self.state.capacity {
            let active = self.values();
            if active.len() as f64 == self.state.capacity { return Err("input action queue overflow"); }
            self.items = vec![None; word_u32(self.state.capacity) as usize];
            for (i, value) in active.iter().enumerate() { self.items[i] = Some(*value); }
            self.state.read = 0.0; self.state.write = active.len() as f64; self.replaced_items = true;
        }
        self.set(self.state.write, name);
        self.state.write += 1.0; self.state.length += 1.0;
        let count = self.count(name);
        self.count_set(name, if count.is_nan() { 1.0 } else { count + 1.0 });
        Ok(())
    }
    fn delete(&mut self, name: f64) -> bool {
        if !self.has(name) { return false; }
        let mut i = self.state.read;
        while i < self.state.write {
            // Map uses SameValueZero, but the source ring scan uses ===.
            if name != NAN && equal(self.item(i), name) {
                self.set(i, NULL); self.state.length -= 1.0;
                self.count_set(name, self.count(name) - 1.0);
                while self.state.read < self.state.write && self.item(self.state.read) == NULL { self.state.read += 1.0; }
                if self.state.read == self.state.write { self.clear(); }
                return true;
            }
            i += 1.0;
        }
        false
    }
    fn finish(mut self) -> (IqState, bool) {
        self.state.items_length = self.items.len() as f64;
        self.state.items = self.items.into_iter().enumerate().filter_map(|(index, value)| value.map(|value| IqSlot { index: index as f64, value })).collect();
        (self.state, self.replaced_items)
    }
}

pub fn queue(input: IqInput, strings: &mut Strings) -> IqResult {
    let mut q = Queue::new(input.state);
    let mut result = IqResult::default();
    match input.op {
        IqOperation::Add => if let Err(error) = q.add(input.name) { result.error = Some(strings.intern_str(error)); },
        IqOperation::Delete => result.accepted = q.delete(input.name),
        IqOperation::Has => result.accepted = q.has(input.name),
        IqOperation::Clear => q.clear(),
    }
    (result.state, result.replaced_items) = q.finish();
    result
}

fn same(a: Option<f64>, b: Option<f64>) -> bool { a == b }
fn less(a: Option<f64>, b: Option<f64>) -> bool { matches!((a, b), (Some(a), Some(b)) if a < b) }
fn less_equal(a: Option<f64>, b: Option<f64>) -> bool { matches!((a, b), (Some(a), Some(b)) if a <= b) }

pub fn controls(input: IcInput) -> IcResult {
    let mut out = IcResult { state: input.state, ..Default::default() };
    let mut q = Queue::new(std::mem::take(&mut out.state.queue));
    match input.op {
        IcOperation::Accept => {
            let s = &mut out.state;
            if !(less(input.epoch, s.epoch) || (same(input.epoch, s.epoch)
                && (less_equal(input.sequence, s.sequence) || less(input.generation, s.generation)))) {
                if !same(input.epoch, s.epoch) || !same(input.generation, s.generation) { s.down.clear(); q.clear(); }
                s.epoch = input.epoch; s.generation = input.generation; s.sequence = input.sequence;
                s.down.clear();
                out.accepted = true;
            }
        }
        IcOperation::Clear => { out.state.down.clear(); q.clear(); }
    }
    (out.state.queue, out.replaced_items) = q.finish();
    out
}

// A binding is resolved at the instant it is queried, since changing a
// seat changes the held-key map even inside the current tick.
pub fn axis(down: &[f64], plus: &[f64], minus: &[f64]) -> f64 {
    let held = |codes: &[f64]| codes.iter().any(|name| down.iter().any(|key| equal(*name, *key)));
    f64::from(held(plus)) - f64::from(held(minus))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(capacity: f64) -> IqState { IqState { capacity, items_length: capacity, ..Default::default() } }

    #[test]
    fn deleted_slots_compact_without_losing_duplicate_edges() {
        let mut q = Queue::new(state(3.0));
        q.add(5.0).unwrap();
        for _ in 0..100 { q.add(6.0).unwrap(); assert!(q.delete(6.0)); }
        q.add(5.0).unwrap(); q.add(7.0).unwrap();
        assert_eq!(q.add(8.0), Err("input action queue overflow"));
        assert_eq!(q.values(), vec![5.0, 5.0, 7.0]);
        assert!(q.delete(5.0)); assert!(q.delete(5.0)); assert!(!q.delete(5.0));
        assert_eq!(q.values(), vec![7.0]);
    }

    #[test]
    fn counts_and_ring_search_keep_the_sources_nan_difference() {
        let mut q = Queue::new(state(3.0)); q.add(NAN).unwrap(); q.add(UNDEFINED).unwrap();
        assert!(q.has(NAN)); assert!(!q.delete(NAN)); assert!(q.delete(UNDEFINED));
        assert_eq!(q.values(), vec![NAN]);
    }
}
