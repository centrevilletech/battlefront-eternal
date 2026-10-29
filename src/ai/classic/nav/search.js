import { detour } from "./barriers.js";
import { f32, TINY, TINY_DIR, vec } from "./math.js";

// The local barrier search: a bounded best-first search over barrier corners
// between two points, run a little at a time by the planner.
//
// The best open node is expanded toward the goal. A straight segment that
// crosses a barrier branches recursively to that barrier's two extreme
// corners (grown by clearance + 0.5), up to a path depth of 32 and never
// further than max(start-to-goal distance, 25) sideways of the direct line.
// A clear segment yields a candidate whose cost is its parent's plus the
// horizontal length; a candidate at a position already open or closed
// replaces that node's parent only when strictly cheaper (closed nodes are
// updated in place, not reopened). Scores are
// max(0, 100000 − (g + horizontal distance to goal + 0.01)) in float32: at
// that magnitude many nodes tie, and the heap's tie rules then decide the
// route. The heap is a one-based max-heap where only a strictly greater
// score climbs past its parent and removal prefers the left child on ties,
// so equal scores do not come out in insertion order.
//
// Storage: 1024 search nodes (a full pool skips new candidates) and a
// 512-entry open heap. The 2004 game writes past the heap's end when it
// overflows; the port refuses the insert, frees the node, and counts an
// overflow instead (a safe-port divergence on unsupported input).

const SCORE_TOP = f32(100000);
const SCORE_PAD = f32(0.01);
const DEPTH = 32;
const LATERAL_MIN = f32(25);

export class OpenHeap {
  constructor(capacity = 512) {
    this.capacity = capacity;
    this.score = new Float32Array(capacity + 1);
    this.node = new Array(capacity + 1).fill(null);
    this.count = 0;
    this.score[0] = SCORE_TOP; // the sentinel
  }

  get empty() {
    return this.count === 0;
  }

  top() {
    return this.node[1];
  }

  // Entry i, zero based, in array order.
  at(i) {
    return this.node[i + 1];
  }

  push(node, score) {
    if (this.count >= this.capacity) return false;
    const i = ++this.count;
    this.score[i] = score;
    this.node[i] = node;
    node.heap = i;
    this.up(i);
    return true;
  }

  // A raised score for the node at zero-based index i.
  raise(i, score) {
    this.score[i + 1] = score;
    this.up(i + 1);
  }

  up(i) {
    const s = this.score;
    const n = this.node;
    while (i > 1) {
      const p = i >> 1;
      if (!(s[i] > s[p])) break;
      this.swap(i, p);
      i = p;
    }
  }

  pop() {
    if (this.count === 0) return null;
    const top = this.node[1];
    const last = this.count--;
    this.score[1] = this.score[last];
    this.node[1] = this.node[last];
    this.node[last] = null;
    if (this.node[1]) this.node[1].heap = 1;
    const s = this.score;
    let i = 1;
    for (;;) {
      const l = i * 2;
      if (l > this.count) break;
      let c = l;
      if (l + 1 <= this.count && s[l + 1] > s[l]) c = l + 1;
      if (s[i] >= s[c]) break;
      this.swap(i, c);
      i = c;
    }
    top.heap = 0;
    return top;
  }

  swap(a, b) {
    const s = this.score[a];
    this.score[a] = this.score[b];
    this.score[b] = s;
    const n = this.node[a];
    this.node[a] = this.node[b];
    this.node[b] = n;
    this.node[a].heap = a;
    this.node[b].heap = b;
  }

  clear() {
    for (let i = 1; i <= this.count; i++) this.node[i] = null;
    this.count = 0;
  }
}

export class BarrierSearch {
  constructor(barriers, pools, options = {}) {
    this.barriers = barriers;
    this.pools = pools; // { paths, nodes } shared path pools (planner.js)
    this.nodeCapacity = options.nodes ?? 1024;
    this.heap = new OpenHeap(options.heap ?? 512);
    this.closed = [];
    this.live = 0; // search nodes allocated (open + closed)
    this.external = 0; // pool slots held elsewhere (tests; the pool is shared natively)
    this.scratch = []; // points visited in the current expansion
    this.start = vec();
    this.goal = vec();
    this.perpX = 0;
    this.perpZ = 0;
    this.lateral = 0;
    this.radius = 0;
    this.mask = 0;
    this.finished = true;
    this.success = false;
    this.failures = 0;
    this.overflows = 0;
    this.hit = { barrier: null, distanceSq: 0 };
    this.left = vec();
    this.right = vec();
  }

  get full() {
    return this.live + this.external >= this.nodeCapacity;
  }

  node(x, y, z, g, parent) {
    this.live++;
    return { x, y, z, g, parent, heap: 0 };
  }

  // Start a search. A destination inside a compatible barrier
  // finishes at once, unsuccessfully. The root needs a free path, 32 free
  // path nodes and a free search node; without them the search finishes
  // empty on its first step.
  begin(start, goal, radius, mask) {
    if (!this.barriers.pointClear(goal, radius, mask)) {
      this.finished = true;
      this.success = false;
      return false;
    }
    this.heap.clear();
    this.closed.length = 0;
    this.live = 0;
    this.start.x = start.x; this.start.y = start.y; this.start.z = start.z;
    this.goal.x = goal.x; this.goal.y = goal.y; this.goal.z = goal.z;
    this.radius = radius;
    this.mask = mask;
    this.finished = false;
    this.success = false;
    const dx = goal.x - start.x;
    const dz = goal.z - start.z;
    const len = f32(Math.sqrt(dx * dx + dz * dz));
    const inv = 1 / len;
    this.perpX = dz * inv;
    this.perpZ = -dx * inv;
    this.lateral = Math.max(len, LATERAL_MIN);
    if (!this.pools.paths.full && this.pools.nodes.free >= 32 && !this.full) {
      const root = this.node(start.x, start.y, start.z, 0, null);
      this.insert(root);
    }
    return true;
  }

  score(n) {
    const dx = f32(this.goal.x - n.x);
    const dz = f32(this.goal.z - n.z);
    const h = f32(Math.sqrt(f32(f32(dx * dx) + f32(dz * dz))));
    const s = f32(n.g + h + SCORE_PAD);
    return s < SCORE_TOP ? f32(SCORE_TOP - s) : 0;
  }

  insert(n) {
    if (this.heap.push(n, this.score(n))) return;
    this.live--;
    this.overflows++;
  }

  // Search until finished or the work budget runs out. The budget
  // is checked before each expansion only, so one expansion can overshoot.
  step(work) {
    while (!this.heap.empty && work.count < work.cap) {
      this.scratch.length = 0;
      const top = this.heap.top();
      if (same2(top, this.goal)) {
        this.finished = true;
        this.success = true;
        return;
      }
      this.heap.pop();
      this.closed.push(top);
      let depth = 0;
      for (let n = top; n; n = n.parent) depth++;
      this.expand(top, this.goal.x, this.goal.y, this.goal.z, depth);
    }
    if (this.heap.empty) this.finished = true;
  }

  // Candidates from parent toward (x, y, z).
  expand(parent, x, y, z, depth) {
    if (depth >= DEPTH) return;
    const lateral = Math.abs(this.perpX * (x - this.goal.x) + this.perpZ * (z - this.goal.z));
    if (!(lateral <= this.lateral)) return;
    for (const p of this.scratch) if (sameXZ(p.x, p.z, x, z)) return;
    this.scratch.push({ x, y, z });
    const from = parent;
    if (this.barriers.nearest(from, { x, y, z }, this.radius, this.mask, this.hit)) {
      const left = vec();
      const right = vec();
      const sides = detour(this.hit.barrier, from, { x, y, z }, this.radius, left, right);
      // A side with no corner leaves the 2004 game's output uninitialized;
      // the port skips that branch.
      if (sides & 1) this.expand(parent, left.x, left.y, left.z, depth + 1);
      if (sides & 2) this.expand(parent, right.x, right.y, right.z, depth + 1);
      return;
    }
    const dx = f32(x - from.x);
    const dz = f32(z - from.z);
    const step = f32(Math.sqrt(f32(f32(dx * dx) + f32(dz * dz))));
    const g = f32(parent.g + step);
    const heap = this.heap;
    for (let i = 0; i < heap.count; i++) {
      const n = heap.at(i);
      if (!sameXZ(n.x, n.z, x, z)) continue;
      if (n.g <= g) return;
      relink(n, x, y, z, step, parent);
      heap.raise(i, this.score(n));
      return;
    }
    for (const n of this.closed) {
      if (!sameXZ(n.x, n.z, x, z)) continue;
      if (n.g <= g) return;
      relink(n, x, y, z, step, parent);
      return;
    }
    if (this.full) return;
    this.insert(this.node(x, y, z, f32(parent.g + step), parent));
  }

  // The path to the goal, start first, or null (counted as a
  // failure). Node heights of zero inherit the last nonzero height met
  // walking back from the goal.
  reconstruct(makePath) {
    if (!this.success) {
      this.failures++;
      return null;
    }
    const path = makePath();
    let carried = 0;
    for (let n = this.heap.top(); n; n = n.parent) {
      let y = n.y;
      if (Math.abs(y) >= TINY) carried = y;
      else y = carried;
      path.prepend(n.x, y, n.z);
    }
    return path;
  }

  // Abandon the current search.
  abort() {
    this.finished = true;
  }
}

function relink(n, x, y, z, step, parent) {
  n.x = x;
  n.y = y;
  n.z = z;
  n.parent = parent;
  n.g = f32(parent.g + step);
}

// Horizontal goal equality.
function same2(a, b) {
  return sameXZ(a.x, a.z, b.x, b.z);
}

function sameXZ(ax, az, bx, bz) {
  const dx = f32(ax - bx);
  const dz = f32(az - bz);
  return f32(f32(dx * dx) + f32(dz * dz)) < TINY_DIR;
}
