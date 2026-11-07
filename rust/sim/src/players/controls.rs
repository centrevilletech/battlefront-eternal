//! A network player's command controls (src/runtime/commands.js
//! CommandControls over src/runtime/action-queue.js ActionQueue): the keys
//! his last accepted command holds down and the presses it queued, read by
//! the world's own key tables. Key codes and action names are the string
//! table's ids, so a lookup is a word comparison and never a string compare.
use crate::store::StrId;

/// ActionQueue's capacity: a paused client's whole burst of presses.
pub const QUEUE_CAPACITY: usize = 2048;

/// Bounded ordered action edges (ActionQueue): a ring of names behind a
/// count per name, so repeated presses stay separate edges while `has` is
/// one lookup. `read` and `write` only grow, as the source's do.
pub struct ActionQueue {
    items: Vec<Option<StrId>>,
    spare: Vec<Option<StrId>>,
    read: u64,
    write: u64,
    length: usize,
    counts: Vec<(StrId, u32)>,
}

impl Default for ActionQueue {
    fn default() -> Self { Self::new() }
}

impl ActionQueue {
    pub fn new() -> Self {
        Self { items: vec![None; QUEUE_CAPACITY], spare: vec![None; QUEUE_CAPACITY], read: 0, write: 0, length: 0, counts: Vec::with_capacity(64) }
    }
    // The ring index of a monotonic position (`i % capacity`).
    fn slot(i: u64) -> usize { (i % QUEUE_CAPACITY as u64) as usize }
    // The source's Map of counts; a handful of names, so a scan beats a map.
    fn count(&self, name: StrId) -> u32 { self.counts.iter().find(|(n, _)| *n == name).map_or(0, |(_, c)| *c) }
    fn count_set(&mut self, name: StrId, count: u32) {
        if let Some(entry) = self.counts.iter_mut().find(|(n, _)| *n == name) { entry.1 = count; }
        else { self.counts.push((name, count)); }
    }
    pub fn len(&self) -> usize { self.length }
    pub fn is_empty(&self) -> bool { self.length == 0 }
    pub fn has(&self, name: StrId) -> bool { self.count(name) > 0 }

    /// ActionQueue.add. A full ring first compacts its live names to the
    /// front. A ring of 2048 live names threw in JavaScript and would have
    /// ended the server's tick; here the press is dropped and reported.
    pub fn add(&mut self, name: StrId) -> bool {
        if self.write - self.read == QUEUE_CAPACITY as u64 {
            if self.length == QUEUE_CAPACITY { return false; }
            // Compact into the spare ring, then swap: no allocation on a tick.

            let mut n = 0;
            for i in self.read..self.write {
                if let Some(value) = self.items[Self::slot(i)] { self.spare[n] = Some(value); n += 1; }
            }
            for slot in self.spare[n..].iter_mut() { *slot = None; }
            std::mem::swap(&mut self.items, &mut self.spare);
            self.read = 0;
            self.write = n as u64;
        }
        let at = Self::slot(self.write);
        self.items[at] = Some(name);
        self.write += 1;
        self.length += 1;
        let count = self.count(name);
        self.count_set(name, count + 1);
        true
    }

    /// ActionQueue.delete: the oldest edge of that name goes; the read
    /// point skips the holes it leaves, and an emptied ring resets.
    pub fn delete(&mut self, name: StrId) -> bool {
        if !self.has(name) { return false; }
        let mut i = self.read;
        while i < self.write {
            let at = Self::slot(i);
            if self.items[at] == Some(name) {
                self.items[at] = None;
                self.length -= 1;
                let count = self.count(name);
                self.count_set(name, count - 1);
                while self.read < self.write && self.items[Self::slot(self.read)].is_none() { self.read += 1; }
                if self.read == self.write { self.clear(); }
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn clear(&mut self) {
        self.read = 0;
        self.write = 0;
        self.length = 0;
        self.counts.clear();
    }
}

/// CommandControls: the stamps of the last accepted command, its held key
/// codes and its queued presses. The mailbox already limits a command to 64
/// held codes, so the held list is a small vector searched linearly.
pub struct Controls {
    pub epoch: f64,
    pub generation: f64,
    pub sequence: f64,
    down: Vec<StrId>,
    pub queued: ActionQueue,
}

impl Default for Controls {
    fn default() -> Self { Self::new() }
}

impl Controls {
    pub fn new() -> Self {
        Self { epoch: 0.0, generation: 0.0, sequence: 0.0, down: Vec::with_capacity(64), queued: ActionQueue::new() }
    }

    /// CommandControls.accept: an older epoch, a repeated sequence or an
    /// older life is refused; a new epoch or life clears what the old one
    /// queued; the held set is replaced and the presses are appended.
    pub fn accept(&mut self, epoch: f64, sequence: f64, generation: f64, down: &[StrId], actions: &[StrId]) -> bool {
        // The source compares the stamps as numbers; a NaN stamp fails every
        // comparison and is accepted, as it was.
        if epoch < self.epoch || (epoch == self.epoch && (sequence <= self.sequence || generation < self.generation)) { return false; }
        if epoch != self.epoch || generation != self.generation { self.clear(); }
        self.epoch = epoch;
        self.generation = generation;
        self.sequence = sequence;
        self.down.clear();
        self.down.extend(down.iter().copied().take(64));
        for &action in actions { self.queued.add(action); }
        true
    }

    /// `controls.down.has(code)`.
    pub fn has(&self, code: StrId) -> bool { self.down.contains(&code) }
    /// CommandControls.held over a binding's codes: any of them is down.
    pub fn held(&self, codes: &[StrId]) -> bool { codes.iter().any(|code| self.has(*code)) }
    /// CommandControls.take: one queued press of the action, if any.
    pub fn take(&mut self, name: StrId) -> bool { self.queued.delete(name) }
    /// CommandControls.axis: the plus keys' count less the minus keys',
    /// each side one when any of its codes is down.
    pub fn axis(&self, plus: &[StrId], minus: &[StrId]) -> f64 {
        f64::from(self.held(plus)) - f64::from(self.held(minus))
    }
    pub fn clear(&mut self) {
        self.down.clear();
        self.queued.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(n: u32) -> StrId { StrId(n) }

    #[test]
    fn repeated_presses_stay_separate_edges_and_holes_compact() {
        let mut q = ActionQueue::new();
        assert!(q.add(id(5)));
        for _ in 0..3000 { assert!(q.add(id(6))); assert!(q.delete(id(6))); }
        assert!(q.add(id(5)));
        assert!(q.has(id(5)) && !q.has(id(6)));
        assert!(q.delete(id(5)));
        assert!(q.delete(id(5)));
        assert!(!q.delete(id(5)));
        assert!(q.is_empty());
    }

    #[test]
    fn a_full_ring_of_live_presses_drops_the_new_press_instead_of_throwing() {
        let mut q = ActionQueue::new();
        for i in 0..QUEUE_CAPACITY { assert!(q.add(id(i as u32 % 7))); }
        assert!(!q.add(id(9)));
        assert_eq!(q.len(), QUEUE_CAPACITY);
        assert!(!q.has(id(9)));
        assert!(q.delete(id(0)));
        assert!(q.add(id(9)));
        assert!(q.has(id(9)));
    }

    #[test]
    fn stale_commands_are_refused_and_a_new_life_clears_the_queue() {
        let mut c = Controls::new();
        assert!(c.accept(1.0, 1.0, 1.0, &[id(1)], &[id(7), id(7)]));
        assert!(!c.accept(1.0, 1.0, 1.0, &[], &[]));
        assert!(!c.accept(0.0, 2.0, 1.0, &[], &[]));
        assert!(c.has(id(1)));
        assert!(c.take(id(7)) && c.take(id(7)) && !c.take(id(7)));
        assert!(c.accept(1.0, 2.0, 1.0, &[id(2)], &[id(8)]));
        assert!(!c.has(id(1)) && c.has(id(2)));
        assert!(c.accept(1.0, 3.0, 2.0, &[], &[]));
        assert!(!c.take(id(8)));
        assert_eq!(c.axis(&[id(3), id(4)], &[id(5)]), 0.0);
        assert!(c.accept(1.0, 4.0, 2.0, &[id(4), id(5)], &[]));
        assert_eq!(c.axis(&[id(3), id(4)], &[id(5)]), 0.0);
        assert!(c.accept(1.0, 5.0, 2.0, &[id(4)], &[]));
        assert_eq!(c.axis(&[id(3), id(4)], &[id(5)]), 1.0);
    }
}
