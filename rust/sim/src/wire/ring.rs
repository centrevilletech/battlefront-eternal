//! Tick-stamped snapshot retention, following runtime/snapshot-ring.js.

const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Clone, Debug)]
struct Stamped<T> {
    epoch: u32,
    value: T,
}

/// Retains the newest `capacity` distinct ticks, irrespective of spacing.
/// Replacing a tick does not evict its old value or change its position.
#[derive(Clone, Debug)]
pub struct SnapshotRing<T> {
    capacity: usize,
    entries: Vec<Stamped<T>>,
    ticks: Vec<i64>,
    newest_tick: i64,
}

impl<T> SnapshotRing<T> {
    pub fn new(capacity: usize) -> Result<Self, &'static str> {
        if capacity == 0 || capacity as u64 > MAX_SAFE_INTEGER as u64 {
            return Err("invalid snapshot ring capacity");
        }
        // SnapshotRing's common 128 positions are reserved at attachment.
        // Keep values aligned with sorted ticks: a BTree node per captured
        // tick was an allocator request even for rings of scalar policy data.
        let reserve = capacity.min(4096);
        Ok(Self { capacity, entries: Vec::with_capacity(reserve), ticks: Vec::with_capacity(reserve), newest_tick: -1 })
    }

    pub fn capacity(&self) -> usize { self.capacity }
    pub fn len(&self) -> usize { self.ticks.len() }
    pub fn is_empty(&self) -> bool { self.ticks.is_empty() }
    pub fn oldest_tick(&self) -> i64 { self.ticks.first().copied().unwrap_or(-1) }
    pub fn newest_tick(&self) -> i64 { self.newest_tick }
    pub fn ticks(&self) -> &[i64] { &self.ticks }

    /// The callback observes the ring after removal and before insertion,
    /// just as a JS entry recycler does. No ring borrow outlives the call.
    pub fn set_with_eviction(
        &mut self,
        tick: i64,
        epoch: u32,
        value: T,
        mut on_evict: impl FnMut(T, &mut Self),
    ) -> Result<bool, &'static str> {
        if !(-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&tick) {
            return Err("invalid snapshot ring tick");
        }
        if let Ok(at) = self.ticks.binary_search(&tick) {
            self.entries[at] = Stamped { epoch, value };
            return Ok(true);
        }
        if self.ticks.len() >= self.capacity {
            if tick < self.ticks[0] { return Ok(false); }
            self.ticks.remove(0);
            let evicted = self.entries.remove(0);
            on_evict(evicted.value, self);
        }
        let mut at = self.ticks.len();
        while at > 0 && self.ticks[at - 1] > tick { at -= 1; }
        self.ticks.insert(at, tick);
        self.entries.insert(at, Stamped { epoch, value });
        if tick > self.newest_tick { self.newest_tick = tick; }
        Ok(true)
    }

    pub fn set(&mut self, tick: i64, epoch: u32, value: T) -> Result<bool, &'static str> {
        self.set_with_eviction(tick, epoch, value, |_, _| {})
    }

    pub fn get(&self, tick: i64, epoch: Option<u32>) -> Option<&T> {
        self.ticks.binary_search(&tick).ok().and_then(|at| self.entries.get(at))
            .filter(|entry| epoch.is_none_or(|e| e == entry.epoch)).map(|entry| &entry.value)
    }

    pub fn has(&self, tick: i64, epoch: Option<u32>) -> bool { self.get(tick, epoch).is_some() }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.ticks.clear();
        self.newest_tick = -1;
    }
}

impl<T> Default for SnapshotRing<T> {
    fn default() -> Self { Self::new(128).expect("snapshot ring capacity") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_late_arrivals_epochs_and_replacement() {
        let mut ring = SnapshotRing::new(3).unwrap();
        for tick in [4, 12, 8] { assert!(ring.set(tick, 1, tick * 10).unwrap()); }
        assert_eq!(ring.ticks(), [4, 8, 12]);
        assert!(!ring.set(2, 1, 20).unwrap());
        assert!(ring.set(8, 2, 81).unwrap());
        assert_eq!(ring.get(8, Some(1)), None);
        assert_eq!(ring.get(8, Some(2)), Some(&81));
        let mut observed = None;
        ring.set_with_eviction(10, 1, 100, |value, held| {
            observed = Some((value, held.ticks().to_vec(), held.oldest_tick(), held.newest_tick()));
            assert!(!held.has(4, None));
            assert!(!held.has(10, None));
        }).unwrap();
        assert_eq!(observed, Some((40, vec![8, 12], 8, 12)));
        assert_eq!(ring.ticks(), [8, 10, 12]);
        ring.clear();
        assert_eq!((ring.len(), ring.oldest_tick(), ring.newest_tick()), (0, -1, -1));
    }

    #[test]
    fn negative_and_safe_integer_ticks_keep_js_newest_rule() {
        let mut ring = SnapshotRing::new(1).unwrap();
        ring.set(-8, 1, ()).unwrap();
        assert_eq!(ring.newest_tick(), -1);
        assert_eq!(ring.oldest_tick(), -8);
        assert!(ring.set(MAX_SAFE_INTEGER, 2, ()).unwrap());
        assert_eq!(ring.newest_tick(), MAX_SAFE_INTEGER);
        assert!(ring.set(MAX_SAFE_INTEGER + 1, 2, ()).is_err());
        assert!(SnapshotRing::<()>::new(0).is_err());
    }
}
