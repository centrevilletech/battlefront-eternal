//! The local barrier search (nav/search.js): a bounded best-first search
//! over barrier corners. Search nodes are indices into the search's
//! arena where the JavaScript holds node objects; parent links and the
//! heap's back-index (`node.heap`) are kept as written.

use super::barriers::{detour, BarrierSet, Work};
use super::math::{f32, tiny, tiny_dir, Vec3};
use super::planner::PathStore;

fn score_top() -> f64 {
    f32(100000.0)
}
fn score_pad() -> f64 {
    f32(0.01)
}
const DEPTH: f64 = 32.0;
fn lateral_min() -> f64 {
    f32(25.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchNode {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub g: f64,
    pub parent: Option<usize>,
    pub heap: usize,
}

/// The one-based max-heap (search.js:31). Scores are Float32Array
/// entries: storing rounds to f32, reading widens, as `Vec<f32>` does.
#[derive(Clone, Debug)]
pub struct OpenHeap {
    pub capacity: usize,
    pub score: Vec<f32>,
    pub node: Vec<Option<usize>>,
    pub count: usize,
}

impl OpenHeap {
    pub fn new(capacity: usize) -> Self {
        let mut score = vec![0.0f32; capacity + 1];
        score[0] = score_top() as f32; // the sentinel (search.js:37)
        OpenHeap { capacity, score, node: vec![None; capacity + 1], count: 0 }
    }

    pub fn empty(&self) -> bool {
        self.count == 0
    }

    pub fn top(&self) -> Option<usize> {
        self.node[1]
    }

    /// Entry i, zero based, in array order (search.js:49).
    pub fn at(&self, i: usize) -> Option<usize> {
        self.node[i + 1]
    }

    pub fn push(&mut self, nodes: &mut [SearchNode], node: usize, score: f64) -> bool {
        if self.count >= self.capacity {
            return false;
        }
        self.count += 1;
        let i = self.count;
        self.score[i] = score as f32;
        self.node[i] = Some(node);
        nodes[node].heap = i;
        self.up(nodes, i);
        true
    }

    /// A raised score for the node at zero-based index i (search.js:64).
    pub fn raise(&mut self, nodes: &mut [SearchNode], i: usize, score: f64) {
        self.score[i + 1] = score as f32;
        self.up(nodes, i + 1);
    }

    fn up(&mut self, nodes: &mut [SearchNode], mut i: usize) {
        while i > 1 {
            let p = i >> 1;
            if !(self.score[i] > self.score[p]) {
                break;
            }
            self.swap(nodes, i, p);
            i = p;
        }
    }

    pub fn pop(&mut self, nodes: &mut [SearchNode]) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        let top = self.node[1].expect("a nonempty heap has a top");
        let last = self.count;
        self.count -= 1;
        self.score[1] = self.score[last];
        self.node[1] = self.node[last];
        self.node[last] = None;
        if let Some(n) = self.node[1] {
            nodes[n].heap = 1;
        }
        let mut i = 1usize;
        loop {
            let l = i * 2;
            if l > self.count {
                break;
            }
            let mut c = l;
            if l + 1 <= self.count && self.score[l + 1] > self.score[l] {
                c = l + 1;
            }
            if self.score[i] >= self.score[c] {
                break;
            }
            self.swap(nodes, i, c);
            i = c;
        }
        nodes[top].heap = 0;
        Some(top)
    }

    fn swap(&mut self, nodes: &mut [SearchNode], a: usize, b: usize) {
        self.score.swap(a, b);
        self.node.swap(a, b);
        let na = self.node[a].expect("heap entries hold nodes");
        let nb = self.node[b].expect("heap entries hold nodes");
        nodes[na].heap = a;
        nodes[nb].heap = b;
    }

    pub fn clear(&mut self) {
        for i in 1..=self.count {
            self.node[i] = None;
        }
        self.count = 0;
    }
}

/// BarrierSearch (search.js:120). The barrier set and the path pools are
/// the planner's, passed to the calls that use them.
#[derive(Clone, Debug)]
pub struct BarrierSearch {
    pub node_capacity: f64,
    pub heap: OpenHeap,
    pub closed: Vec<usize>,
    pub nodes: Vec<SearchNode>,
    pub live: f64,
    pub external: f64,
    pub scratch: Vec<Vec3>,
    pub start: Vec3,
    pub goal: Vec3,
    pub perp_x: f64,
    pub perp_z: f64,
    pub lateral: f64,
    pub radius: f64,
    pub mask: u32,
    pub finished: bool,
    pub success: bool,
    pub failures: f64,
    pub overflows: f64,
}

impl BarrierSearch {
    /// search.js:121 with its defaults (1024 nodes, a 512-entry heap);
    /// `nodes` and `heap` are the JavaScript's options of the same names.
    pub fn new(nodes: Option<f64>, heap: Option<f64>) -> Self {
        BarrierSearch {
            node_capacity: nodes.unwrap_or(1024.0),
            heap: OpenHeap::new(heap.unwrap_or(512.0) as usize),
            closed: Vec::new(),
            nodes: Vec::new(),
            live: 0.0,
            external: 0.0,
            scratch: Vec::new(),
            start: Vec3::default(),
            goal: Vec3::default(),
            perp_x: 0.0,
            perp_z: 0.0,
            lateral: 0.0,
            radius: 0.0,
            mask: 0,
            finished: true,
            success: false,
            failures: 0.0,
            overflows: 0.0,
        }
    }

    pub fn full(&self) -> bool {
        self.live + self.external >= self.node_capacity
    }

    fn node(&mut self, x: f64, y: f64, z: f64, g: f64, parent: Option<usize>) -> usize {
        self.live += 1.0;
        self.nodes.push(SearchNode { x, y, z, g, parent, heap: 0 });
        self.nodes.len() - 1
    }

    /// Start a search (search.js:159).
    pub fn begin(&mut self, barriers: &mut BarrierSet, store: &PathStore, start: Vec3, goal: Vec3, radius: f64, mask: u32) -> bool {
        if !barriers.point_clear(goal, radius, mask).0 {
            self.finished = true;
            self.success = false;
            return false;
        }
        self.heap.clear();
        self.closed.clear();
        self.nodes.clear();
        self.live = 0.0;
        self.start = start;
        self.goal = goal;
        self.radius = radius;
        self.mask = mask;
        self.finished = false;
        self.success = false;
        let dx = goal.x - start.x;
        let dz = goal.z - start.z;
        let len = f32(v8math::sqrt(dx * dx + dz * dz));
        let inv = 1.0 / len;
        self.perp_x = dz * inv;
        self.perp_z = -dx * inv;
        self.lateral = v8math::max(len, lateral_min());
        if !store.pools.paths.full() && store.pools.nodes.free() >= 32.0 && !self.full() {
            let root = self.node(start.x, start.y, start.z, 0.0, None);
            self.insert(root);
        }
        true
    }

    fn score(&self, n: usize) -> f64 {
        let n = &self.nodes[n];
        let dx = f32(self.goal.x - n.x);
        let dz = f32(self.goal.z - n.z);
        let h = f32(v8math::sqrt(f32(f32(dx * dx) + f32(dz * dz))));
        let s = f32(n.g + h + score_pad());
        if s < score_top() {
            f32(score_top() - s)
        } else {
            0.0
        }
    }

    fn insert(&mut self, n: usize) {
        let score = self.score(n);
        // The heap borrows the arena; split the fields for the call.
        let BarrierSearch { heap, nodes, live, overflows, .. } = self;
        if heap.push(nodes, n, score) {
            return;
        }
        *live -= 1.0;
        *overflows += 1.0;
        // The JavaScript frees the node it just made; it is the arena's
        // last, and nothing names it yet.
        if n == nodes.len() - 1 {
            nodes.pop();
        }
    }

    /// Search until finished or the work budget runs out (search.js:204).
    pub fn step(&mut self, barriers: &mut BarrierSet, work: &mut Work) {
        while !self.heap.empty() && work.count < work.cap {
            self.scratch.clear();
            let top = self.heap.top().expect("a nonempty heap has a top");
            let goal = self.goal;
            if same_xz(self.nodes[top].x, self.nodes[top].z, goal.x, goal.z) {
                self.finished = true;
                self.success = true;
                return;
            }
            let BarrierSearch { heap, nodes, .. } = self;
            heap.pop(nodes);
            self.closed.push(top);
            let mut depth = 0.0;
            let mut n = Some(top);
            while let Some(i) = n {
                depth += 1.0;
                n = self.nodes[i].parent;
            }
            self.expand(barriers, work, top, goal.x, goal.y, goal.z, depth);
        }
        if self.heap.empty() {
            self.finished = true;
        }
    }

    /// Candidates from parent toward (x, y, z) (search.js:223).
    fn expand(&mut self, barriers: &mut BarrierSet, work: &mut Work, parent: usize, x: f64, y: f64, z: f64, depth: f64) {
        if depth >= DEPTH {
            return;
        }
        let lateral = v8math::abs(self.perp_x * (x - self.goal.x) + self.perp_z * (z - self.goal.z));
        if !(lateral <= self.lateral) {
            return;
        }
        for p in &self.scratch {
            if same_xz(p.x, p.z, x, z) {
                return;
            }
        }
        self.scratch.push(Vec3::new(x, y, z));
        let from = {
            let n = &self.nodes[parent];
            Vec3::new(n.x, n.y, n.z)
        };
        let to = Vec3::new(x, y, z);
        let (hit, barrier, _distance_sq) = barriers.nearest(from, to, self.radius, self.mask, Some(work));
        if hit {
            let barrier = &barriers.list[barrier.expect("nearest names its barrier")];
            let (mut left, mut right) = (Vec3::default(), Vec3::default());
            let sides = detour(barrier, from, to, self.radius, &mut left, &mut right);
            if sides & 1 != 0 {
                self.expand(barriers, work, parent, left.x, left.y, left.z, depth + 1.0);
            }
            if sides & 2 != 0 {
                self.expand(barriers, work, parent, right.x, right.y, right.z, depth + 1.0);
            }
            return;
        }
        let dx = f32(x - from.x);
        let dz = f32(z - from.z);
        let step = f32(v8math::sqrt(f32(f32(dx * dx) + f32(dz * dz))));
        let parent_g = self.nodes[parent].g;
        let g = f32(parent_g + step);
        for i in 0..self.heap.count {
            let n = self.heap.at(i).expect("heap entries hold nodes");
            let node = self.nodes[n];
            if !same_xz(node.x, node.z, x, z) {
                continue;
            }
            if node.g <= g {
                return;
            }
            relink(&mut self.nodes[n], x, y, z, step, parent_g, parent);
            let score = self.score(n);
            let BarrierSearch { heap, nodes, .. } = self;
            heap.raise(nodes, i, score);
            return;
        }
        for i in 0..self.closed.len() {
            let n = self.closed[i];
            let node = self.nodes[n];
            if !same_xz(node.x, node.z, x, z) {
                continue;
            }
            if node.g <= g {
                return;
            }
            relink(&mut self.nodes[n], x, y, z, step, parent_g, parent);
            return;
        }
        if self.full() {
            return;
        }
        let n = self.node(x, y, z, f32(parent_g + step), Some(parent));
        self.insert(n);
    }

    /// The path to the goal, start first, or None (search.js:266). The
    /// path is made in the store, as the planner's makePath makes it.
    pub fn reconstruct(&mut self, store: &mut PathStore) -> Option<usize> {
        if !self.success {
            self.failures += 1.0;
            return None;
        }
        let path = store.make();
        let mut carried = 0.0;
        let mut n = self.heap.top();
        while let Some(i) = n {
            let node = self.nodes[i];
            let mut y = node.y;
            if v8math::abs(y) >= tiny() {
                carried = y;
            } else {
                y = carried;
            }
            store.prepend(path, node.x, y, node.z);
            n = node.parent;
        }
        Some(path)
    }

    /// Abandon the current search (search.js:283).
    pub fn abort(&mut self) {
        self.finished = true;
    }
}

fn relink(n: &mut SearchNode, x: f64, y: f64, z: f64, step: f64, parent_g: f64, parent: usize) {
    // search.js:288: g is f32(parent.g + step) at relink time.
    n.x = x;
    n.y = y;
    n.z = z;
    n.parent = Some(parent);
    n.g = f32(parent_g + step);
}

/// Horizontal goal equality (search.js:301).
fn same_xz(ax: f64, az: f64, bx: f64, bz: f64) -> bool {
    let dx = f32(ax - bx);
    let dz = f32(az - bz);
    f32(f32(dx * dx) + f32(dz * dz)) < tiny_dir()
}
