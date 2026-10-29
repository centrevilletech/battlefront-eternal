import * as THREE from "three";
import { d2, f32, FLT_MAX, NEAR_WAYPOINT, TINY, normalize, same, set, copy, vec } from "./math.js";
import { KIND } from "./units.js";

// Following one path: progress bookkeeping, waypoint advancement and the
// steering point written into the movement request each tick.
//
// Each tick runs, in order: the stuck bookkeeping (status 1 when the best
// distance to the waypoint has not improved for 5 s, status 2 when the unit
// has stayed within 2 m of an anchor for 3 s; 6 s for both on a Droideka),
// then, unless advancing is held, the reach test (horizontal distance under
// the final or intermediate radius) or the passage test (past the plane
// through the waypoint across the segment), either of which advances one
// node at most, then steering. Status is only reported; behaviors decide
// what to do with it.
//
// Steering: Soldier and Droideka sidestep 5 m perpendicular to the contact
// normals their collision accumulated this tick (choosing a side once and
// keeping it while contacts recur), else look ahead along the segment by
// their speed + 0.2; Hover looks ahead by speed, Walker by 20 m, Flyer turns
// on circles. Everything else aims at the waypoint itself.

export const EXEC = {
  anchorRadius: 2,
  stationary: 3,
  noProgress: 5,
  droidekaStationary: 6,
  droidekaNoProgress: 6,
};

export class PathExecutor {
  // The unit's position becomes the segment start, except a Hover
  // or Flyer starting within 10 m of the first node, which starts there.
  constructor(path, control, unit) {
    this.path = path;
    this.control = control;
    this.index = 0;
    this.prev = vec(); // segment start
    this.waypoint = vec(); // current waypoint
    this.next = vec(); // the node after it
    this.final = vec(); // the path's last node
    this.direction = vec(); // outgoing direction
    this.mode = 2;
    this.side = 0; // forced turning side (Flyer circles)
    this.complete = false;
    this.status = 0;
    this.anchor = vec();
    this.stationary = 0;
    this.best = FLT_MAX;
    this.noProgress = 0;
    this.hold = false; // no reach/passage advancement
    const last = path.nodes[path.nodes.length - 1];
    if (!last) {
      // A path whose points were all dropped by a full node pool.
      this.complete = true;
      return;
    }
    last.flag |= 1;
    const first = path.nodes[0];
    if (first && d2(unit.position, first) < 100 && (unit.kind === KIND.hover || unit.kind === KIND.flyer)) {
      this.setup(first, unit);
    } else {
      this.setup(unit.position, unit);
    }
  }

  get node() {
    return this.path.nodes[this.index];
  }

  get last() {
    return this.index >= this.path.nodes.length - 1;
  }

  // One tick.
  tick(dt, unit) {
    if (!this.path.nodes.length) return;
    this.progress(dt, unit);
    if (!this.hold && (this.reached(unit) || this.passed(unit))) this.advance(unit);
    this.steer(unit);
  }

  // Stuck bookkeeping.
  progress(dt, unit) {
    const droideka = unit.kind === KIND.droideka;
    const stationaryLimit = droideka ? EXEC.droidekaStationary : EXEC.stationary;
    const noProgressLimit = droideka ? EXEC.droidekaNoProgress : EXEC.noProgress;
    this.status = 0;
    const p = unit.position;
    const w = this.waypoint;
    const dx = f32(w.x - p.x);
    const dz = f32(w.z - p.z);
    if (!(NEAR_WAYPOINT <= f32(f32(dx * dx) + f32(dz * dz)))) return;
    let t = 0;
    if (d2(p, this.anchor) <= EXEC.anchorRadius * EXEC.anchorRadius) {
      t = this.stationary = f32(this.stationary + dt);
    } else {
      copy(this.anchor, p);
      this.stationary = 0;
    }
    if (stationaryLimit < t) this.status = 2;
    const dist = f32(d2(p, w));
    if (dist < this.best) {
      this.best = dist;
      this.noProgress = 0;
      return;
    }
    this.noProgress = f32(this.noProgress + dt);
    if (noProgressLimit < this.noProgress && this.status === 0) this.status = 1;
  }

  // Within reach of the waypoint, horizontally.
  reached(unit) {
    const r = this.last ? nearRadius(unit) : passRadius(unit);
    const dx = this.waypoint.x - unit.position.x;
    const dz = this.waypoint.z - unit.position.z;
    return dx * dx + dz * dz < r * r;
  }

  // Past the plane through the waypoint across the segment. A
  // degenerate segment counts as passed; a Flyer must also be within 10 m.
  passed(unit) {
    const sx = this.waypoint.x - this.prev.x;
    const sz = this.waypoint.z - this.prev.z;
    if (sx * sx + sz * sz < TINY) return true;
    const vx = unit.position.x - this.waypoint.x;
    const vz = unit.position.z - this.waypoint.z;
    if (unit.kind === KIND.flyer && vx * vx + vz * vz > 100) return false;
    return sx * vx + sz * vz > 0;
  }

  // Consume one node: a node with the boundary flag returns a
  // segment to the controller; the last node completes the path instead of
  // moving on. Nodes before the previous one are dropped.
  advance(unit) {
    if (!this.complete && (this.node.flag & 1)) this.control.consumeSegment();
    if (this.last) {
      this.complete = true;
    } else {
      const was = vec(this.waypoint.x, this.waypoint.y, this.waypoint.z);
      this.index++;
      this.setup(was, unit);
    }
    if (this.index > 1) {
      this.path.dropFront(this.index - 1);
      this.index = 1;
    }
    this.best = f32(d2(unit.position, this.waypoint));
    this.noProgress = 0;
  }

  // Segment setup for the current node, then steering.
  setup(start, unit) {
    copy(this.prev, start);
    copy(this.waypoint, this.node);
    this.mode = 2;
    this.side = 0;
    const nodes = this.path.nodes;
    if (this.index >= nodes.length - 1) {
      copy(this.next, this.waypoint);
      if (!same(this.waypoint, this.prev)) {
        normalize(set(this.direction, this.waypoint.x - this.prev.x, this.waypoint.y - this.prev.y, this.waypoint.z - this.prev.z));
        this.mode = 2;
      }
    } else {
      copy(this.next, nodes[this.index + 1]);
      if (same(unit.position, start)) {
        normalize(set(this.direction, this.next.x - this.waypoint.x, this.next.y - this.waypoint.y, this.next.z - this.waypoint.z));
        this.mode = 1;
        this.side = 0;
      } else if (!same(this.next, this.waypoint)) {
        const ex = this.next.x - this.waypoint.x, ey = this.next.y - this.waypoint.y, ez = this.next.z - this.waypoint.z;
        normalize(set(this.direction, ex, ey, ez));
        this.mode = 2;
        // Zero in exact arithmetic; the float residue picks the side.
        this.side = this.direction.z * ex - this.direction.x * ez >= 0 ? 1 : -1;
      }
    }
    copy(this.final, nodes[nodes.length - 1]);
    this.steer(unit);
  }

  // Append a further path: keep at most 32 nodes by dropping the
  // oldest (never the current one), skip a first point equal to the current
  // last, flag the new last node, and refresh the segment.
  append(path, unit, flagLast = true) {
    if (this.complete && this.node) this.node.flag &= ~1;
    const nodes = this.path.nodes;
    let total = nodes.length + path.nodes.length;
    while (total > 32 && this.index > 0) {
      this.path.dropFront(1);
      this.index--;
      total--;
    }
    let from = 0;
    const tail = nodes[nodes.length - 1];
    if (tail && path.nodes[0] && same(tail, path.nodes[0])) from = 1;
    for (let i = from; i < path.nodes.length; i++) this.path.append(path.nodes[i]);
    const last = nodes[nodes.length - 1];
    if (last && flagLast) last.flag |= 1;
    path.release();
    this.setup(this.prev, unit);
  }

  // Follow's continuing stream has a cap of 30, independent of the
  // planner's appended-segment limit32. If trimming catches the current
  // node, restart from the oldest remaining node.
  appendPoint(point, unit, cap = 30) {
    if (this.complete && this.node) this.node.flag &= ~1;
    const remove = Math.max(0, this.path.length + 1 - cap);
    if (remove) {
      this.path.dropFront(remove);
      this.index = Math.max(0, this.index - remove);
    }
    this.path.append(point);
    this.complete = false;
    this.setup(this.prev, unit);
  }

  // The steering point into the controller's movement request.
  steer(unit) {
    const target = this.target ?? (this.target = vec());
    copy(target, this.waypoint);
    const p = unit.position;
    switch (unit.kind) {
      case KIND.flyer:
        flyerSteer(this, unit, target);
        break;
      case KIND.hover:
        lookahead(this.prev, this.waypoint, p, speed(unit), target);
        break;
      case KIND.walker:
        lookahead(this.prev, this.waypoint, p, 20, target);
        break;
      case KIND.soldier:
      case KIND.droideka:
        contactSidestep(this, unit, target);
        break;
      default:
        break;
    }
    this.control.writeRequest(target, this.waypoint, this.direction, this.mode, -1, 1);
  }

  release() {
    this.path.release();
  }
}

function speed(unit) {
  const v = unit.velocity;
  return Math.sqrt(v.x * v.x + v.y * v.y + v.z * v.z);
}

// Soldier and Droideka: the collision contact vector overrides lookahead.
function contactSidestep(exec, unit, target) {
  const contact = unit.soldier?.avoidance;
  const p = unit.position;
  if (!contact || contact.x * contact.x + contact.y * contact.y + contact.z * contact.z <= 0) {
    exec.control.avoidSide = 0;
    lookahead(exec.prev, exec.waypoint, p, speed(unit), target);
    return;
  }
  const n = normalize(vec(contact.x, contact.y, contact.z));
  const rx = n.z;
  const rz = -n.x;
  if (exec.control.avoidSide === 0) {
    const away = normalize(vec(p.x - exec.waypoint.x, p.y - exec.waypoint.y, p.z - exec.waypoint.z));
    exec.control.avoidSide = rx * away.x + rz * away.z <= 0 ? 1 : -1;
  }
  const s = 5 * exec.control.avoidSide;
  set(target, p.x + s * rx, p.y, p.z + s * rz);
  contact.x = contact.y = contact.z = 0;
}

// Segment lookahead: with A and B lifted to the unit's height,
// the point lookAhead + 0.2 beyond the unit's projection on A→B, or B when
// that passes the end. The comparison is on the squared distance, so a
// projection far behind A also falls back to B; one just behind A still
// returns a point before A.
export function lookahead(a, b, p, ahead, out) {
  const ax = a.x, az = a.z, bx = b.x, bz = b.z;
  const y = p.y;
  const ex = bx - ax;
  const ez = bz - az;
  const len2 = ex * ex + ez * ez;
  if (len2 < TINY) return set(out, bx, y, bz);
  const len = Math.sqrt(f32(len2));
  const dx = f32(ex / len);
  const dz = f32(ez / len);
  const s = f32(f32(dx * f32(p.x - ax)) + f32(dz * f32(p.z - az)));
  const t = f32(s + ahead + 0.2);
  if (f32(t * t) <= f32(len2)) return set(out, f32(ax + f32(t * dx)), y, f32(az + f32(t * dz)));
  return set(out, bx, y, bz);
}

// The radius within which the final waypoint counts as reached:
// 1 for soldiers and droids, half the class ForwardSpeed for a Hover, 4 for
// everything else.
export function nearRadius(unit) {
  const k = unit.kind;
  if (k === KIND.soldier || k === KIND.droid) return 1;
  if (k === KIND.hover) return 0.5 * (unit.classDef?.ForwardSpeed ?? 0);
  return 4;
}

// Intermediate waypoints: a quarter of MaxSpeed for a Flyer, half
// ForwardSpeed for a Hover, 1 otherwise.
export function passRadius(unit) {
  const k = unit.kind;
  if (k === KIND.flyer) return 0.25 * (unit.classDef?.MaxSpeed ?? 30);
  if (k === KIND.hover) return 0.5 * (unit.classDef?.ForwardSpeed ?? 0);
  return 1;
}

const forwardScratch = new THREE.Vector3();

// Flyer circle steering.
function flyerSteer(exec, unit, target) {
  const turn = Math.max(unit.classDef?.TurnRate ?? 2, 1.5);
  const fwd = unit.forward ? unit.forward(forwardScratch) : forwardScratch.set(0, 0, 1);
  const v = unit.velocity;
  const along = Math.abs(v.x * fwd.x + v.y * fwd.y + v.z * fwd.z);
  const R = Math.max(5, 2 * (along / turn));
  const f = normalize(vec(fwd.x + 0.0001, 0, fwd.z));
  flyerCircle(exec.waypoint, exec.direction, unit.position, f, R, exec.side, target);
}

// Circle construction: centers one radius either side of the actor's
// heading and of the waypoint's outgoing direction; a forced side picks the
// waypoint circle, otherwise the nearer one (flipped when the circles
// overlap and their sides differ). The output aims one radius past the
// tangent point, at the waypoint's height.
export function flyerCircle(q, u, p, v, R, side, out) {
  const U = normalize(vec(u.x, 0, u.z));
  const V = normalize(vec(v.x, 0, v.z));
  const perp = (w) => vec(w.z, 0, -w.x);
  const pv = perp(V);
  const pu = perp(U);
  const aPlus = vec(p.x + R * pv.x, 0, p.z + R * pv.z);
  const aMinus = vec(p.x - R * pv.x, 0, p.z - R * pv.z);
  const h2 = (a, b) => (a.x - b.x) * (a.x - b.x) + (a.z - b.z) * (a.z - b.z);
  const actorPlus = h2(aPlus, q) < h2(aMinus, q);
  const cPlus = vec(q.x + R * pu.x, 0, q.z + R * pu.z);
  const cMinus = vec(q.x - R * pu.x, 0, q.z - R * pu.z);
  let plus;
  if (side === 1) plus = true;
  else if (side === -1) plus = false;
  else {
    plus = h2(cPlus, p) < h2(cMinus, p);
    const a = actorPlus ? aPlus : aMinus;
    const c = plus ? cPlus : cMinus;
    if (h2(a, c) < 4 * R * R && actorPlus !== plus) plus = !plus;
  }
  const C = plus ? cPlus : cMinus;
  const dx = p.x - C.x;
  const dz = p.z - C.z;
  const d = Math.sqrt(dx * dx + dz * dz);
  let tx, tz;
  if (d < R) {
    tx = C.x + R * dx / (d + 0.00001);
    tz = C.z + R * dz / (d + 0.00001);
  } else {
    const base = Math.atan2(dz, dx);
    const swing = Math.acos(R / d);
    const angle = plus ? base - swing : base + swing;
    tx = C.x + R * Math.cos(angle);
    tz = C.z + R * Math.sin(angle);
  }
  const wx = tx - C.x;
  const wz = tz - C.z;
  if (plus) return set(out, tx + wz, q.y, tz - wx);
  return set(out, tx - wz, q.y, tz + wx);
}
