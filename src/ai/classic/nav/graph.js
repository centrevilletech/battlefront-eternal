import { f32, lowestBit } from "./math.js";

// The authored planning graph (the `plan` chunk) and its stochastic routing,
// as the 2004 game runs it (docs/ai-nav.md).
//
// Hubs are spheres; arcs join two hubs and taper between their radii. Every
// hub stores five routing tables (one per traversal capability below Flyer),
// indexed by destination hub. A table word packs up to four candidate next
// arcs, least significant byte first: slot = byte & 7 names one of the hub's
// eight incident arcs, weight = (byte >> 3) & 31. Picking a next arc draws
// one raw number from the shared AI generator modulo 31 and walks the bytes
// subtracting weights until the remainder is <= 0. The weights are the
// original's route-diversity prior; normalizing them would change Classic.
//
// Capability bits: 1 Soldier, 2 Hover, 4 Small, 8 Medium, 16 Huge, 32 Flyer.
// A query's mask picks the routing table of its lowest set bit; masks whose
// lowest bit is 32 or above have no table (the 2004 game indexes past its
// five arrays there). The port refuses those queries and reports them.

export const UNUSED = 255;
export const NO_TABLE = -1;

const SEARCH_START = f32(10000);
const FAR = f32(1e20);
// A selector pass that cannot finish (all weights zero, or only the
// excluded arc left) loops forever in the 2004 game. Weights are at most
// 31 and the draw at most 30, so any finishing word ends within 32 passes.
const MAX_PASSES = 64;

export class NavGraph {
  // graph: ai-nav.json `graph`; words: Uint32Array of the routing words.
  constructor(graph, words, report = () => {}) {
    const [hubCount, arcCount] = graph.info;
    if (!(hubCount >= 0 && arcCount >= 0)) throw new Error("ai-nav: negative graph counts");
    if (graph.hubs.length !== hubCount || graph.arcs.length !== arcCount) throw new Error("ai-nav: graph counts do not match INFO");
    const tables = graph.routes?.tables ?? 5;
    if (words.length !== tables * hubCount * hubCount) throw new Error(`ai-nav: ${words.length} routing words, expected ${tables * hubCount * hubCount}`);
    this.report = report;
    this.tables = tables;
    this.words = words;
    this.hubs = graph.hubs.map((h, index) => {
      if (h.arcs.length !== 8) throw new Error(`ai-nav: hub ${index} does not have 8 arc slots`);
      for (const v of [...h.position, h.radius]) if (!Number.isFinite(v)) throw new Error(`ai-nav: hub ${index} is not finite`);
      for (const a of h.arcs) if (a !== UNUSED && !(a >= 0 && a < arcCount)) throw new Error(`ai-nav: hub ${index} names arc ${a}`);
      return {
        index,
        name: h.name,
        x: f32(h.position[0]), y: f32(h.position[1]), z: f32(h.position[2]),
        radius: f32(h.radius),
        slots: Uint8Array.from(h.arcs),
        mask: 0,
        base: index * tables * hubCount,
      };
    });
    this.arcs = graph.arcs.map((a, index) => {
      const [i, j] = a.hubs;
      if (!(i >= 0 && i < hubCount && j >= 0 && j < hubCount)) throw new Error(`ai-nav: arc ${index} joins missing hubs`);
      return { index, name: a.name, a: this.hubs[i], b: this.hubs[j], mask: a.mask >>> 0, degenerate: false };
    });
    for (const arc of this.arcs) {
      arc.degenerate = arc.a.x === arc.b.x && arc.a.y === arc.b.y && arc.a.z === arc.b.z;
      if (arc.degenerate) report("zeroLengthArc", { arc: arc.index });
    }
    // The loader's hub mask: the OR of the arcs in the hub's eight slots.
    for (const hub of this.hubs) {
      let mask = 0;
      for (const s of hub.slots) if (s !== UNUSED) mask |= this.arcs[s].mask;
      hub.mask = mask >>> 0;
    }
  }

  // The routing table a traversal mask uses, or NO_TABLE.
  table(mask) {
    const t = lowestBit(mask);
    return t < this.tables ? t : NO_TABLE;
  }

  word(from, table, to) {
    return this.words[from.base + table * this.hubs.length + to.index];
  }

  // The weighted next arc from hub `from` toward hub `to`, or
  // null when the table has no route. `previous` (an arc or null) is
  // excluded, and the draw total shrinks by its weight; with nothing left
  // the alternate-arc pick decides. The selector does not recheck candidate
  // arcs against the mask; shipped data relies on that in a few places.
  select(previous, from, to, mask, random) {
    const table = this.table(mask);
    if (table === NO_TABLE) {
      this.report("routeTable", { mask });
      return null;
    }
    const first = this.word(from, table, to);
    let total = 31;
    let excluded = 8;
    if (previous) {
      for (let i = 0; i < 8; i++) {
        if (from.slots[i] === previous.index) {
          excluded = i;
          break;
        }
      }
      let w = first;
      do {
        if ((w & 7) === excluded) {
          total = 31 - ((w >>> 3) & 31);
          if (total < 1) return this.alternate(from, previous, mask, random);
          break;
        }
        w >>>= 8;
      } while (w !== 0);
    }
    if (first === 0) return null;
    let r = random.next() % total;
    let w = first;
    let passes = 0;
    for (;;) {
      r = (r - ((w >>> 3) & 31)) | 0;
      if (r <= 0 && (w & 7) !== excluded) break;
      w >>>= 8;
      if (w === 0) {
        w = first;
        if (++passes > MAX_PASSES) {
          this.report("routeHang", { hub: from.index, table, destination: to.index, word: first });
          return null;
        }
      }
    }
    return this.arcs[from.slots[w & 7]];
  }

  // The uniform pick among the hub's other mask-compatible arcs;
  // `previous` itself when there is no other. Counting stops at the first
  // unused slot, as the 2004 game's does.
  alternate(hub, previous, mask, random) {
    let count = 0;
    for (let i = 0; i < 8; i++) {
      const s = hub.slots[i];
      if (s === UNUSED) break;
      const arc = this.arcs[s];
      if ((arc.mask & mask) !== 0 && arc !== previous) count++;
    }
    if (count === 0) return previous;
    let r = random.next() % count;
    for (let i = 0; i < 8; i++) {
      const s = hub.slots[i];
      if (s === UNUSED) continue;
      const arc = this.arcs[s];
      if ((arc.mask & mask) === 0 || arc === previous) continue;
      r -= 1;
      if (r < 0) return arc;
    }
    return null;
  }

  // Attach a point to the graph: the smallest compatible hub that
  // contains it (type 1), else the compatible arc corridor with the smallest
  // radius sum that contains it (type 2), else type 0. Radii of 10000 or more
  // never win. Ties keep the earlier record.
  attach(p, mask, out = { type: 0, hub: null, arc: null }) {
    out.type = 0;
    out.hub = null;
    out.arc = null;
    let best = SEARCH_START;
    for (const hub of this.hubs) {
      if ((hub.mask & mask) === 0 || !(hub.radius < best)) continue;
      if (!containsHub(hub, p)) continue;
      best = hub.radius;
      out.hub = hub;
    }
    if (best < SEARCH_START) {
      out.type = 1;
      return out;
    }
    best = SEARCH_START;
    for (const arc of this.arcs) {
      if ((arc.mask & mask) === 0 || arc.degenerate) continue;
      const sum = f32(arc.a.radius + arc.b.radius);
      if (!(sum < best) || !containsArc(arc, p)) continue;
      best = sum;
      out.arc = arc;
    }
    if (best < SEARCH_START) {
      out.type = 2;
      return out;
    }
    out.arc = null;
    return out;
  }

  // The compatible arc whose tapered corridor is nearest: how far
  // outside the corridor the point lies, 0 inside. Null without one.
  nearestArc(p, mask) {
    let best = FAR;
    let found = null;
    for (const arc of this.arcs) {
      if ((arc.mask & mask) === 0 || arc.degenerate) continue;
      const d = corridorGap(arc, p);
      if (d < best) {
        best = d;
        found = arc;
      }
    }
    return found;
  }
}

// The other end of an arc from hub h.
export function otherEnd(arc, h) {
  return h === arc.a ? arc.b : arc.a;
}

// Strictly inside the hub's sphere.
export function containsHub(hub, p) {
  const dx = p.x - hub.x;
  const dy = p.y - hub.y;
  const dz = p.z - hub.z;
  return dx * dx + dy * dy + dz * dz < hub.radius * hub.radius;
}

// Inside the arc's tapered corridor: projecting within the
// segment, and nearer the axis than the radius interpolated there.
export function containsArc(arc, p) {
  const A = arc.a;
  const B = arc.b;
  const abx = B.x - A.x, aby = B.y - A.y, abz = B.z - A.z;
  const t = f32((abx * (p.x - A.x) + aby * (p.y - A.y) + abz * (p.z - A.z)) / (abx * abx + aby * aby + abz * abz));
  if (!(t >= 0 && t <= 1)) return false;
  const dx = p.x - (A.x + t * abx);
  const dy = p.y - (A.y + t * aby);
  const dz = p.z - (A.z + t * abz);
  return Math.sqrt(dx * dx + dy * dy + dz * dz) < (1 - t) * A.radius + t * B.radius;
}

// Distance outside the corridor, past an end the distance outside
// that end's sphere.
export function corridorGap(arc, p) {
  const A = arc.a;
  const B = arc.b;
  const abx = B.x - A.x, aby = B.y - A.y, abz = B.z - A.z;
  const t = f32((abx * (p.x - A.x) + aby * (p.y - A.y) + abz * (p.z - A.z)) / (abx * abx + aby * aby + abz * abz));
  if (t >= 0 && t <= 1) {
    const r = (1 - t) * A.radius + t * B.radius;
    const dx = p.x - (A.x + t * abx);
    const dy = p.y - (A.y + t * aby);
    const dz = p.z - (A.z + t * abz);
    return Math.max(0, Math.sqrt(dx * dx + dy * dy + dz * dz) - r);
  }
  const end = t < 0 ? A : B;
  const dx = p.x - end.x;
  const dy = p.y - end.y;
  const dz = p.z - end.z;
  return Math.max(0, Math.sqrt(dx * dx + dy * dy + dz * dz) - end.radius);
}
