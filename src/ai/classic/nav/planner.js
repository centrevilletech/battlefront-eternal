import { BarrierSearch } from "./search.js";
import { f32 } from "./math.js";

// The local path planner shared by every AI unit: a request queue serviced a
// few barrier tests at a time, plus the immediate straight-segment shortcut
// (docs/ai-nav.md).
//
// A request names its owner (a NavControl, by serial so a dead owner's work
// is dropped), the two ends, clearance, traversal mask and a priority byte.
// The owner's outstanding-segment count is added to the priority, and while
// that count is 10 or less a clear straight segment is answered at once with
// a two-point path. Otherwise the request waits in a 75-slot queue; a full
// queue refuses it. Service takes the first live request of priority 0, then
// 1, ... 10, in arrival order (dead owners' requests at a visited priority are
// dropped on the way; priorities above 10 are never served), and runs the
// barrier search with a budget of int(budget × 10 × 5 / 5) barrier tests per
// call. The scheduler offers 1.0 per tick, and again when line-of-sight work
// leaves room. A finished search publishes its path, or null when it failed,
// to a still-live owner.
//
// Port: a request passed over while STARVED later searches finished is taken
// next, oldest first, whatever its priority (if it is one the queue serves).
// The 2004 game's strict priority order lets a stream of priority-0 asks
// (fighters repositioning every few updates) keep a unit's first route leg
// (priority 1) waiting for as long as the fight lasts, and a bot waiting for
// its path stands still; with 60 bots on Cloud City they stood in dozens.
//
// AIPath and AIPathNode come from shared pools of 900 and 7200. Adding a
// point to a path when the node pool is full silently drops the point.

export const STARVED = 20;

export class Pool {
  constructor(capacity) {
    this.capacity = capacity;
    this.used = 0;
  }

  get full() {
    return this.used >= this.capacity;
  }

  get free() {
    return this.capacity - this.used;
  }
}

// An ordered list of path points; node.flag bit 0 marks a segment boundary
// (the last node of a path handed to an executor).
export class AIPath {
  constructor(pools) {
    this.pools = pools;
    this.nodes = [];
    this.released = false;
    pools.paths.used++;
  }

  // Insert at the front: the search walks back from the goal.
  prepend(x, y, z) {
    if (this.pools.nodes.full) return false;
    this.pools.nodes.used++;
    this.nodes.unshift({ x: f32(x), y: f32(y), z: f32(z), flag: 0 });
    return true;
  }

  append(node) {
    if (this.pools.nodes.full) return false;
    this.pools.nodes.used++;
    this.nodes.push({ x: node.x, y: node.y, z: node.z, flag: node.flag ?? 0 });
    return true;
  }

  // Drop the first k nodes back to the pool.
  dropFront(k) {
    if (k <= 0) return;
    this.nodes.splice(0, k);
    this.pools.nodes.used -= k;
  }

  get length() {
    return this.nodes.length;
  }

  release() {
    if (this.released) return;
    this.released = true;
    this.pools.nodes.used -= this.nodes.length;
    this.pools.paths.used--;
    this.nodes.length = 0;
  }
}

export class PathPlanner {
  constructor(barriers, options = {}) {
    this.barriers = barriers;
    this.pools = {
      paths: new Pool(options.paths ?? 900),
      nodes: new Pool(options.pathNodes ?? 7200),
    };
    this.capacity = options.requests ?? 75;
    this.queue = [];
    this.current = null;
    this.search = new BarrierSearch(barriers, this.pools, { nodes: options.searchNodes, heap: options.heap });
    this.work = { count: 0, cap: 0 };
    this.served = 0;
  }

  path() {
    return new AIPath(this.pools);
  }

  // Two-point path start → end.
  segment(start, end) {
    const path = this.path();
    path.prepend(end.x, end.y, end.z);
    path.prepend(start.x, start.y, start.z);
    return path;
  }

  // Ask for a local path from start to end. True when answered at
  // once or queued, false when the queue was full.
  request(owner, start, end, radius, mask, priority) {
    const total = (priority + owner.segments) & 0xff;
    this.cancel(owner);
    if (owner.segments <= 10) {
      this.barriers.work = null;
      if (!this.barriers.segmentBlocked(start, end, radius, mask)) {
        owner.publish(this.segment(start, end), true);
        return true;
      }
    }
    if (this.queue.length >= this.capacity) return false;
    this.queue.push({
      owner,
      serial: owner.serial,
      start: { x: start.x, y: start.y, z: start.z },
      end: { x: end.x, y: end.y, z: end.z },
      radius,
      mask,
      priority: total,
      served: this.served, // searches finished when it was queued
    });
    return true;
  }

  live(request) {
    return request.owner.serial === request.serial && !request.owner.disposed;
  }

  // Forget the owner's pending work: its publication is reset, a
  // search running for it stops, and its first queued request goes.
  cancel(owner) {
    owner.publish(null, false);
    if (this.current && this.current.owner === owner) {
      this.search.abort();
      this.current = null;
    }
    const i = this.queue.findIndex((r) => r.owner === owner);
    if (i >= 0) this.queue.splice(i, 1);
  }

  // An outstanding segment of this owner was consumed: its queued
  // request (not one being searched) moves up a priority.
  promote(owner) {
    if (this.current && this.current.owner === owner) return;
    const r = this.queue.find((q) => q.owner === owner);
    if (r && r.priority !== 0) r.priority -= 1;
  }

  // The next request to search: one passed over too long (the queue is in
  // arrival order, so the first found is the oldest), else by priority.
  select() {
    for (let i = 0; i < this.queue.length; i++) {
      const r = this.queue[i];
      if (r.priority > 10 || this.served - r.served < STARVED || !this.live(r)) continue;
      this.queue.splice(i, 1);
      return r;
    }
    for (let p = 0; p <= 10; p++) {
      for (let i = 0; i < this.queue.length;) {
        const r = this.queue[i];
        if (r.priority !== p) {
          i++;
          continue;
        }
        this.queue.splice(i, 1);
        if (this.live(r)) return r;
      }
    }
    return null;
  }

  // No search in progress and nothing queued.
  get idle() {
    return this.search.finished && this.queue.length === 0;
  }

  // One planner service.
  service(budget = 1) {
    const work = this.work;
    work.cap = Math.trunc(f32(f32(f32(budget * 10) * 5) / 5));
    work.count = 0;
    this.barriers.work = work;
    const search = this.search;
    while (work.count < work.cap) {
      if (search.finished) {
        this.current = this.select();
        if (!this.current) break;
        const r = this.current;
        search.begin(r.start, r.end, r.radius, r.mask);
      }
      if (!search.finished) search.step(work);
      if (search.finished) {
        const path = search.reconstruct(() => this.path());
        if (this.current && this.live(this.current)) this.current.owner.publish(path, true);
        else path?.release();
        this.current = null;
        this.served++;
      }
    }
    this.barriers.work = null;
  }
}
