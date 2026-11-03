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
        Pool { capacity, used: 0.0 }
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

/// The path store: every AIPath made, by index, with the pools they draw
/// on. The JavaScript's AIPath methods, with the pools explicit.
#[derive(Clone, Debug)]
pub struct PathStore {
    pub pools: Pools,
    pub paths: Vec<AIPath>,
}

impl PathStore {
    pub fn new(paths: f64, path_nodes: f64) -> Self {
        PathStore {
            pools: Pools { paths: Pool::new(paths), nodes: Pool::new(path_nodes) },
            paths: Vec::new(),
        }
    }

    /// planner.js:51 (`new AIPath(pools)`) via planner.js:108 `path()`.
    pub fn make(&mut self) -> usize {
        self.pools.paths.used += 1.0;
        self.paths.push(AIPath::default());
        self.paths.len() - 1
    }

    /// Insert at the front, rounding to float32 (planner.js:59).
    pub fn prepend(&mut self, path: usize, x: f64, y: f64, z: f64) -> bool {
        if self.pools.nodes.full() {
            return false;
        }
        self.pools.nodes.used += 1.0;
        self.paths[path].nodes.insert(0, PathNode { x: f32(x), y: f32(y), z: f32(z), flag: 0 });
        true
    }

    /// planner.js:66. The node's numbers are copied as they are.
    pub fn append(&mut self, path: usize, node: PathNode) -> bool {
        if self.pools.nodes.full() {
            return false;
        }
        self.pools.nodes.used += 1.0;
        self.paths[path].nodes.push(node);
        true
    }

    /// Drop the first k nodes back to the pool (planner.js:74). The
    /// JavaScript's splice drops fewer when the path is shorter; its
    /// count then goes down by k all the same, as written.
    pub fn drop_front(&mut self, path: usize, k: f64) {
        if k <= 0.0 {
            return;
        }
        let n = self.paths[path].nodes.len();
        let drop = (k as usize).min(n);
        self.paths[path].nodes.drain(..drop);
        self.pools.nodes.used -= k;
    }

    /// planner.js:80.
    pub fn length(&self, path: usize) -> usize {
        self.paths[path].nodes.len()
    }

    /// planner.js:84.
    pub fn release(&mut self, path: usize) {
        if self.paths[path].released {
            return;
        }
        self.paths[path].released = true;
        self.pools.nodes.used -= self.paths[path].nodes.len() as f64;
        self.pools.paths.used -= 1.0;
        self.paths[path].nodes.clear();
    }

    /// Two-point path start -> end (planner.js:113 `segment`).
    pub fn segment(&mut self, start: super::math::Vec3, end: super::math::Vec3) -> usize {
        let path = self.make();
        self.prepend(path, end.x, end.y, end.z);
        self.prepend(path, start.x, start.y, start.z);
        path
    }
}
