//! The shared path pools (nav/planner.js:31-91). The planner itself
//! lives in the Nav service (nav/mod.rs), because its requests name
//! their owning controls; the queue, search and service there use these
//! pools. Paths are indices into the service's path list where the
//! JavaScript holds AIPath objects.

use super::math::f32;

/// planner.js:31.
pub const STARVED: f64 = 20.0;

/// A counted pool (planner.js:33). Counts stay f64: the JavaScript's
/// `used` is a number and the comparisons are numeric.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pool {
    pub capacity: f64,
    pub used: f64,
}

impl Pool {
    pub fn new(capacity: f64) -> Self {
        Pool {
            capacity,
            used: 0.0,
        }
    }

    pub fn full(&self) -> bool {
        self.used >= self.capacity
    }

    pub fn free(&self) -> f64 {
        self.capacity - self.used
    }
}

/// The two shared pools (planner.js:96).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pools {
    pub paths: Pool,
    pub nodes: Pool,
}

/// One path point; flag bit 0 marks a segment boundary (planner.js:48).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PathNode {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub flag: i32,
}

/// An ordered list of path points (planner.js:50). Making, filling and
/// releasing one moves the pools' counts, so those live on the service;
/// this is only the points and the released mark.
#[derive(Clone, Debug, Default)]
pub struct AIPath {
    pub nodes: Vec<PathNode>,
    pub released: bool,
}

/// Active paths retain their source construction ids. Each id resolves to
/// a generation-qualified reusable slot; released ids never resolve to a new
/// path. Node buffers grow by exchanging reserved storage, not allocating.
#[derive(Clone, Debug)]
pub struct PathRows {
    slots: Vec<AIPath>,
    generations: Vec<u32>,
    free: Vec<usize>,
    handles: std::collections::HashMap<usize, (usize, u32)>,
    next: usize,
    buffers: Vec<Vec<PathNode>>,
}
static RELEASED: AIPath = AIPath {
    nodes: Vec::new(),
    released: true,
};
impl PathRows {
    fn new(slots: usize, nodes: usize) -> Self {
        let mut buffers = Vec::new();
        let mut size = 64usize;
        while size <= nodes.max(64).next_power_of_two() {
            for _ in 0..nodes / size + 1 {
                buffers.push(Vec::with_capacity(size));
            }
            size *= 2;
        }
        Self {
            slots: (0..slots)
                .map(|_| AIPath {
                    nodes: Vec::with_capacity(32),
                    released: true,
                })
                .collect(),
            generations: vec![1; slots],
            free: (0..slots).rev().collect(),
            handles: std::collections::HashMap::with_capacity(slots),
            next: 0,
            buffers,
        }
    }
    /// World construction can reserve additional simultaneous straight paths
    /// without changing the source's separate logical path-pool capacity.
    pub fn reserve(&mut self, slots: usize) {
        if slots <= self.slots.len() {
            return;
        }
        let old = self.slots.len();
        self.slots.reserve(slots - old);
        self.generations.reserve(slots - old);
        self.free.reserve(slots - old);
        self.handles
            .reserve(slots.saturating_sub(self.handles.len()));
        for slot in old..slots {
            self.slots.push(AIPath {
                nodes: Vec::with_capacity(32),
                released: true,
            });
            self.generations.push(1);
            self.free.push(slot);
        }
    }
    fn slot(&self, key: usize) -> Option<usize> {
        let &(slot, generation) = self.handles.get(&key)?;
        (self.generations[slot] == generation).then_some(slot)
    }
    fn make(&mut self) -> usize {
        let slot = self.free.pop().expect("prepared navigation path slots");
        let key = self.next;
        self.next = self
            .next
            .checked_add(1)
            .expect("navigation path identity exhaustion");
        let path = &mut self.slots[slot];
        assert!(path.nodes.is_empty());
        path.released = false;
        self.handles.insert(key, (slot, self.generations[slot]));
        key
    }
    fn reclaim(&mut self, needed: usize) {
        // A long path may have consumed most of its nodes while retaining its
        // large allocation. Move that short tail into smaller reserved space.
        let donor = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.released && p.nodes.capacity() >= needed)
            .min_by_key(|(_, p)| p.nodes.len())
            .map(|(i, _)| i)
            .expect("prepared navigation node storage");
        let count = self.slots[donor].nodes.len();
        if let Some(free) = self
            .free
            .iter()
            .copied()
            .find(|&i| self.slots[i].nodes.capacity() >= count)
        {
            let mut small = std::mem::take(&mut self.slots[free].nodes);
            small.extend_from_slice(&self.slots[donor].nodes);
            let mut large = std::mem::replace(&mut self.slots[donor].nodes, small);
            large.clear();
            self.slots[free].nodes = large;
        } else {
            let i = self
                .buffers
                .iter()
                .enumerate()
                .filter(|(_, b)| b.capacity() >= count)
                .min_by_key(|(_, b)| b.capacity())
                .map(|(i, _)| i)
                .expect("prepared short navigation tail storage");
            let mut small = self.buffers.swap_remove(i);
            small.extend_from_slice(&self.slots[donor].nodes);
            let mut large = std::mem::replace(&mut self.slots[donor].nodes, small);
            large.clear();
            self.buffers.push(large);
        }
    }
    fn room(&mut self, key: usize, needed: usize) {
        let slot = self.slot(key).expect("current navigation path");
        if self.slots[slot].nodes.capacity() >= needed {
            return;
        }
        if !self
            .free
            .iter()
            .any(|&i| self.slots[i].nodes.capacity() >= needed)
            && !self.buffers.iter().any(|b| b.capacity() >= needed)
        {
            self.reclaim(needed);
        }
        // Reuse a sufficiently large free path's allocation before taking a
        // spare tier. Large buffers remain available after their path dies.
        let donor = self
            .free
            .iter()
            .copied()
            .find(|&i| self.slots[i].nodes.capacity() >= needed);
        let mut replacement = if let Some(donor) = donor {
            std::mem::take(&mut self.slots[donor].nodes)
        } else {
            let i = self
                .buffers
                .iter()
                .enumerate()
                .filter(|(_, b)| b.capacity() >= needed)
                .min_by_key(|(_, b)| b.capacity())
                .map(|(i, _)| i)
                .expect("prepared navigation node buffers");
            self.buffers.swap_remove(i)
        };
        replacement.extend_from_slice(&self.slots[slot].nodes);
        let mut previous = std::mem::replace(&mut self.slots[slot].nodes, replacement);
        previous.clear();
        if let Some(donor) = donor {
            self.slots[donor].nodes = previous;
        } else {
            self.buffers.push(previous);
        }
    }
    fn release(&mut self, key: usize) -> Option<usize> {
        let slot = self.slot(key)?;
        self.handles.remove(&key);
        let path = &mut self.slots[slot];
        let count = path.nodes.len();
        path.nodes.clear();
        path.released = true;
        self.generations[slot] = self.generations[slot].wrapping_add(1).max(1);
        self.free.push(slot);
        Some(count)
    }
    pub fn len(&self) -> usize {
        self.next
    }
}
impl std::ops::Index<usize> for PathRows {
    type Output = AIPath;
    fn index(&self, key: usize) -> &AIPath {
        assert!(key < self.next, "constructed navigation path");
        self.slot(key).map_or(&RELEASED, |slot| &self.slots[slot])
    }
}
impl std::ops::IndexMut<usize> for PathRows {
    fn index_mut(&mut self, key: usize) -> &mut AIPath {
        let slot = self.slot(key).expect("current navigation path");
        &mut self.slots[slot]
    }
}

/// The source's counted pools remain distinct from reserved physical storage.
#[derive(Clone, Debug)]
pub struct PathStore {
    pub pools: Pools,
    pub paths: PathRows,
}

impl PathStore {
    pub fn new(paths: f64, path_nodes: f64) -> Self {
        PathStore {
            pools: Pools {
                paths: Pool::new(paths),
                nodes: Pool::new(path_nodes),
            },
            paths: PathRows::new(
                paths.max(8.).ceil() as usize,
                path_nodes.max(0.).ceil() as usize,
            ),
        }
    }
    pub fn reserve_paths(&mut self, simultaneous: usize) {
        self.paths.reserve(simultaneous);
    }

    /// planner.js:51 (new AIPath) via planner.js:108 path(). Its public id
    /// follows construction order while its physical storage returns on release.
    pub fn make(&mut self) -> usize {
        self.pools.paths.used += 1.0;
        self.paths.make()
    }

    /// Insert at the front, rounding to float32 (planner.js:59).
    pub fn prepend(&mut self, path: usize, x: f64, y: f64, z: f64) -> bool {
        if self.pools.nodes.full() {
            return false;
        }
        self.pools.nodes.used += 1.0;
        self.paths.room(path, self.paths[path].nodes.len() + 1);
        self.paths[path].nodes.insert(
            0,
            PathNode {
                x: f32(x),
                y: f32(y),
                z: f32(z),
                flag: 0,
            },
        );
        true
    }

    /// planner.js:66. The node's numbers are copied as they are.
    pub fn append(&mut self, path: usize, node: PathNode) -> bool {
        if self.pools.nodes.full() {
            return false;
        }
        self.pools.nodes.used += 1.0;
        self.paths.room(path, self.paths[path].nodes.len() + 1);
        self.paths[path].nodes.push(node);
        true
    }

    /// Drop the first k nodes (planner.js:74). Splice can drop fewer nodes;
    /// the logical count still goes down by k, as the source intentionally does.
    pub fn drop_front(&mut self, path: usize, k: f64) {
        if k <= 0.0 {
            return;
        }
        let n = self.paths[path].nodes.len();
        let drop = (k as usize).min(n);
        self.paths[path].nodes.drain(..drop);
        self.pools.nodes.used -= k;
    }
    pub fn length(&self, path: usize) -> usize {
        self.paths[path].nodes.len()
    }
    /// planner.js:84. A stale construction id cannot release a reused slot.
    pub fn release(&mut self, path: usize) {
        if let Some(count) = self.paths.release(path) {
            self.pools.nodes.used -= count as f64;
            self.pools.paths.used -= 1.;
        }
    }
    /// Two-point path start -> end (planner.js:113 segment).
    pub fn segment(&mut self, start: super::math::Vec3, end: super::math::Vec3) -> usize {
        let path = self.make();
        self.prepend(path, end.x, end.y, end.z);
        self.prepend(path, start.x, start.y, start.z);
        path
    }
}
