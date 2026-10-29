import { f32, FLT_MAX, TINY, TINY_DIR, normalize, set, vec } from "./math.js";

// Planning barriers: authored rectangles agents route around
// (docs/ai-nav.md).
//
// A barrier is a footprint: center C, the transform's first and third basis
// rows X and Z (kept as authored, unnormalized), and half extents hx, hz.
// An agent of clearance r treats it as the rectangle C ± X(hx + r) ± Z(hz + r);
// SIZE.y is never compared. Every query first filters by traversal mask
// (barrier.flags & mask), so a barrier can block soldiers but not hovers.
//
// The loader negates the stored translation's z and keeps the basis rows as
// stored; barrierFromRecord applies exactly that.
//
// Broadphase: the 2004 game gathers candidates from its spatial grid by the
// barrier's bounding sphere (radius |SIZE|) against the query box. Narrow
// phase results do not depend on the grid, but two orderings do: the nearest
// hit query keeps the first of equally near barriers, and recursive search
// counts work per tested barrier. The port visits candidates in source
// order, once each; the 2004 game's grid order is not reproduced (Provisional).

export function barrierFromRecord(record, index) {
  const m = record.xfrm;
  const [sx, sy, sz] = record.size;
  for (const v of [...m, sx, sy, sz]) if (!Number.isFinite(v)) throw new Error(`ai-nav: barrier ${index} is not finite`);
  return {
    index,
    name: record.name,
    flags: record.flags >>> 0,
    cx: f32(m[9]), cy: f32(m[10]), cz: f32(-m[11]),
    xx: f32(m[0]), xy: f32(m[1]), xz: f32(m[2]),
    zx: f32(m[6]), zy: f32(m[7]), zz: f32(m[8]),
    hx: f32(sx), hy: f32(sy), hz: f32(sz),
    bound: f32(Math.sqrt(sx * sx + sy * sy + sz * sz)),
  };
}

// Strictly inside the footprint grown by r: |X·(C−P)| < hx + r and
// |Z·(C−P)| < hz + r. Equality is outside; height enters only through tilted
// basis rows.
export function inside(b, px, py, pz, r) {
  const dx = b.cx - px;
  const dy = b.cy - py;
  const dz = b.cz - pz;
  return Math.abs(b.xx * dx + b.xy * dy + b.xz * dz) < b.hx + r
    && Math.abs(b.zx * dx + b.zy * dy + b.zz * dz) < b.hz + r;
}

// Corner i of the footprint grown by r + e, in the order
// C+U+V, C+U−V, C−U−V, C−U+V with U = X(hx + r + e), V = Z(hz + r + e).
export function corner(b, i, r, e, out) {
  const u = b.hx + r + e;
  const v = b.hz + r + e;
  const su = i < 2 ? 1 : -1;
  const sv = i === 0 || i === 3 ? 1 : -1;
  return set(out,
    f32(b.cx + su * b.xx * u + sv * b.zx * v),
    f32(b.cy + su * b.xy * u + sv * b.zy * v),
    f32(b.cz + su * b.xz * u + sv * b.zz * v));
}

// A query segment: ends, midpoint and 3D half length.
export function segment(s, e, out = {}) {
  out.sx = s.x; out.sy = s.y; out.sz = s.z;
  out.ex = e.x; out.ey = e.y; out.ez = e.z;
  out.mx = (s.x + e.x) / 2; out.my = (s.y + e.y) / 2; out.mz = (s.z + e.z) / 2;
  const hx = out.mx - s.x, hy = out.my - s.y, hz = out.mz - s.z;
  out.half = Math.sqrt(hx * hx + hy * hy + hz * hz);
  return out;
}

const corners = [vec(), vec(), vec(), vec()];

// Segment against one barrier grown by r. The cheap rejections
// come first: the midpoint's distance from the center against the bounding
// radius plus half length plus r, then the midpoint strictly inside the
// footprint grown by half length plus r (so a segment ending exactly on an
// edge is rejected here). A barrier that passes counts one unit of planner
// work. Then the flattened segment meets the four edges; parameters are
// inclusive, the nearest crossing wins, and a segment wholly inside crosses
// no edge. On a hit, hit.distanceSq = t² |D|² and hit.normal is the crossed
// edge's normalized (−Ez, 0, Ex).
export function crossing(b, seg, r, work, hit) {
  const mx = seg.mx - b.cx;
  const mz = seg.mz - b.cz;
  const grow = seg.half + r;
  const reach = b.bound + grow;
  if (!(mx * mx + mz * mz < reach * reach)) return false;
  if (!inside(b, seg.mx, seg.my, seg.mz, grow)) return false;
  if (work) work.count++;
  for (let i = 0; i < 4; i++) corner(b, i, r, 0, corners[i]);
  const dx = seg.ex - seg.sx;
  const dz = seg.ez - seg.sz;
  let best = 1e10;
  let nx = 0;
  let nz = 0;
  for (let i = 0; i < 4; i++) {
    const c = corners[i];
    const n = corners[(i + 1) & 3];
    const ex = n.x - c.x;
    const ez = n.z - c.z;
    const det = dx * ez - dz * ex;
    if (!(Math.abs(det) > TINY)) continue;
    const inv = 1 / det;
    const wx = c.x - seg.sx;
    const wz = c.z - seg.sz;
    const u = (wx * dz - wz * dx) * inv;
    const t = (wx * ez - wz * ex) * inv;
    if (t >= 0 && t <= 1 && u >= 0 && u <= 1 && t < best) {
      best = t;
      nx = -ez;
      nz = ex;
    }
  }
  if (!(best < 1e10)) return false;
  if (hit) {
    hit.distanceSq = best * best * (dx * dx + dz * dz);
    normalize(set(hit.normal, nx, 0, nz));
  }
  return true;
}

// The barriers of a map with a uniform grid for candidate gathering.
export class BarrierSet {
  constructor(barriers, cell = 16) {
    this.list = barriers;
    this.cell = cell;
    this.cells = new Map();
    this.stamp = new Uint32Array(barriers.length);
    this.serial = 0;
    this.found = [];
    this.work = null; // the planner's work counter while it is searching
    this.seg = {};
    this.hit = { distanceSq: 0, normal: vec() };
    for (const b of barriers) {
      const x0 = Math.floor((b.cx - b.bound) / cell);
      const x1 = Math.floor((b.cx + b.bound) / cell);
      const z0 = Math.floor((b.cz - b.bound) / cell);
      const z1 = Math.floor((b.cz + b.bound) / cell);
      for (let x = x0; x <= x1; x++) {
        for (let z = z0; z <= z1; z++) {
          const key = x * 73856093 ^ z * 19349663;
          let list = this.cells.get(key);
          if (!list) this.cells.set(key, (list = []));
          list.push(b);
        }
      }
    }
  }

  // Barriers whose bounding circle meets the box [x0, x1] × [z0, z1] (any
  // height), in source order.
  gather(x0, z0, x1, z1) {
    const out = this.found;
    out.length = 0;
    if (++this.serial === 0xffffffff) {
      this.stamp.fill(0);
      this.serial = 1;
    }
    const c = this.cell;
    const cx0 = Math.floor(x0 / c);
    const cx1 = Math.floor(x1 / c);
    const cz0 = Math.floor(z0 / c);
    const cz1 = Math.floor(z1 / c);
    for (let x = cx0; x <= cx1; x++) {
      for (let z = cz0; z <= cz1; z++) {
        const list = this.cells.get(x * 73856093 ^ z * 19349663);
        if (!list) continue;
        for (const b of list) {
          if (this.stamp[b.index] === this.serial) continue;
          this.stamp[b.index] = this.serial;
          const qx = Math.min(Math.max(b.cx, x0), x1) - b.cx;
          const qz = Math.min(Math.max(b.cz, z0), z1) - b.cz;
          if (qx * qx + qz * qz <= b.bound * b.bound) out.push(b);
        }
      }
    }
    if (out.length > 1) out.sort((a, b) => a.index - b.index);
    return out;
  }

  // Is p clear of every compatible barrier grown by r? out.barrier
  // is the blocking one.
  pointClear(p, r, mask, out = null) {
    const list = this.gather(p.x - r, p.z - r, p.x + r, p.z + r);
    for (const b of list) {
      if ((b.flags & mask) === 0) continue;
      if (inside(b, p.x, p.y, p.z, r)) {
        if (out) out.barrier = b;
        return false;
      }
    }
    if (out) out.barrier = null;
    return true;
  }

  // Does s→e cross any compatible barrier grown by r? Ends closer
  // than 0.0001 squared never do. Stops at the first crossing.
  segmentBlocked(s, e, r, mask) {
    const dx = s.x - e.x, dy = s.y - e.y, dz = s.z - e.z;
    if (dx * dx + dy * dy + dz * dz < TINY) return false;
    const seg = segment(s, e, this.seg);
    const list = this.gather(Math.min(s.x, e.x) - r, Math.min(s.z, e.z) - r, Math.max(s.x, e.x) + r, Math.max(s.z, e.z) + r);
    for (const b of list) {
      if ((b.flags & mask) === 0) continue;
      if (crossing(b, seg, r, this.work, null)) return true;
    }
    return false;
  }

  // The compatible barrier crossed nearest along s→e: equally
  // near ones keep the first. out.barrier, out.distanceSq; false if none.
  nearest(s, e, r, mask, out) {
    const seg = segment(s, e, this.seg);
    const list = this.gather(Math.min(s.x, e.x) - r, Math.min(s.z, e.z) - r, Math.max(s.x, e.x) + r, Math.max(s.z, e.z) + r);
    let any = false;
    let best = FLT_MAX;
    out.barrier = null;
    for (const b of list) {
      if ((b.flags & mask) === 0) continue;
      if (!crossing(b, seg, r, this.work, this.hit)) continue;
      any = true;
      if (this.hit.distanceSq < best) {
        best = this.hit.distanceSq;
        out.barrier = b;
      }
    }
    out.distanceSq = best;
    return any;
  }

  // Push p out of the barrier that blocks it: test p, then the
  // four edge projections of that barrier in order of distance, accepting
  // the first clear one. Changes x and z only. False if none is clear.
  correct(p, r, mask) {
    const probe = { barrier: null };
    if (this.pointClear(p, r, mask, probe)) return true;
    const b = probe.barrier;
    const scores = [0, 0, 0, 0];
    const cands = [vec(), vec(), vec(), vec()];
    for (let i = 0; i < 4; i++) scores[i] = ejection(b, p, r, i, cands[i]);
    for (let attempt = 0; attempt < 4; attempt++) {
      let pick = 0;
      let low = FLT_MAX;
      for (let i = 0; i < 4; i++) {
        if (scores[i] < low) {
          low = scores[i];
          pick = i;
        }
      }
      if (this.pointClear(cands[pick], r, mask)) {
        p.x = cands[pick].x;
        p.z = cands[pick].z;
        return true;
      }
      scores[pick] = FLT_MAX;
    }
    return false;
  }

  // The first compatible barrier edge crossed along s→e, shortening e to the
  // crossing. Each crossing found shortens the segment the later
  // barriers are tested against, so the final e is the nearest crossing and
  // normal (if given) is its edge normal. True when anything was crossed.
  obstruct(s, e, r, mask, normal = null) {
    const dir = normalize(vec(e.x - s.x, e.y - s.y, e.z - s.z));
    let seg = segment(s, e, this.seg);
    const grow = 2 * r;
    const list = this.gather(Math.min(s.x, e.x) - grow, Math.min(s.z, e.z) - grow, Math.max(s.x, e.x) + grow, Math.max(s.z, e.z) + grow).slice();
    let any = false;
    for (const b of list) {
      if ((b.flags & mask) === 0) continue;
      if (!crossing(b, seg, r, this.work, this.hit)) continue;
      const d = Math.sqrt(this.hit.distanceSq);
      e.x = s.x + dir.x * d;
      e.y = s.y + dir.y * d;
      e.z = s.z + dir.z * d;
      seg = segment(s, e, this.seg);
      if (normal) set(normal, this.hit.normal.x, this.hit.normal.y, this.hit.normal.z);
      any = true;
    }
    return any;
  }
}

// Ejection candidate i: the point of edge i (corner i to i+1 of
// the footprint grown by r) nearest the flattened p, nudged 0.01 outward
// along (Ez, 0, −Ex). Returns the squared distance as the candidate's score.
export function ejection(b, p, r, i, out) {
  const a = corner(b, i, r, 0, vec());
  const c = corner(b, i > 2 ? 0 : i + 1, r, 0, vec());
  // Nearest point of segment a-c to (p.x, 0, p.z).
  const px = p.x, py = 0, pz = p.z;
  const ex = a.x - c.x, ey = a.y - c.y, ez = a.z - c.z;
  const t = (px - c.x) * ex + (py - c.y) * ey + (pz - c.z) * ez;
  let qx, qy, qz;
  if (t > 0) {
    const len = ex * ex + ey * ey + ez * ez;
    if (t < len) {
      const k = t / len;
      qx = c.x + k * ex; qy = c.y + k * ey; qz = c.z + k * ez;
    } else {
      qx = a.x; qy = a.y; qz = a.z;
    }
  } else {
    qx = c.x; qy = c.y; qz = c.z;
  }
  const score = (px - qx) * (px - qx) + (py - qy) * (py - qy) + (pz - qz) * (pz - qz);
  const n = normalize(vec(c.z - a.z, 0, -(c.x - a.x)));
  set(out, qx + 0.01 * n.x, qy, qz + 0.01 * n.z);
  return score;
}

// The two extreme detour corners of barrier b seen from p heading for q
//, on the footprint grown by r + 0.5. Each side keeps the corner
// making the widest angle with the heading; `left` takes corners with a
// negative cross product, `right` positive ones; a corner exactly on the
// line counts for neither. Returns bit 1 when left was set, bit 2 for right.
export function detour(b, p, q, r, left, right) {
  let dx = q.x - p.x;
  let dz = q.z - p.z;
  if (dx * dx + dz * dz < TINY_DIR) {
    dx = 1;
    dz = 0;
  }
  let l = Math.sqrt(dx * dx + dz * dz);
  dx /= l;
  dz /= l;
  let bestLeft = 1;
  let bestRight = 1;
  let found = 0;
  const c = vec();
  for (let i = 0; i < 4; i++) {
    corner(b, i, r, 0.5, c);
    let vx = c.x - p.x;
    let vz = c.z - p.z;
    const v2 = vx * vx + vz * vz;
    if (!(v2 > TINY_DIR)) continue;
    l = Math.sqrt(v2);
    vx /= l;
    vz /= l;
    const along = dx * vx + dz * vz;
    const cross = dz * vx - dx * vz; // y of cross(d, v)
    if (cross < 0 && along < bestLeft) {
      set(left, c.x, c.y, c.z);
      bestLeft = along;
      found |= 1;
    } else if (cross > 0 && along < bestRight) {
      set(right, c.x, c.y, c.z);
      bestRight = along;
      found |= 2;
    }
  }
  return found;
}
