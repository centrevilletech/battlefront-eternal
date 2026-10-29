import { otherEnd } from "./graph.js";
import { d2, f32, normalize, set, copy, vec } from "./math.js";

// A route through the authored graph, one hub at a time.
//
// Initialization attaches start and destination separately and then follows
// a fixed branch tree: same hub or arc goes straight to the destination;
// otherwise it picks the starting and finishing endpoints with fresh
// weighted draws and nearest-endpoint tie rules, and the first hub waypoint
// is a random point on that hub's shell. Each later advance draws the next
// arc (excluding the one just used) and moves to its far hub. Arriving at
// either accepted destination hub restores the exact requested point. The
// draws happen even where a later write discards their result: they move
// the shared generator, and Classic keeps that.

export class GraphCursor {
  constructor(graph, barriers) {
    this.graph = graph;
    this.barriers = barriers;
    this.mask = 0;
    this.clearance = 0; // 4 for Droideka, else 0
    this.primary = null; // destination hub
    this.secondary = null; // the other accepted destination hub (arc destinations)
    this.exact = vec();
    this.attachType = 0; // the destination's original attachment type
    this.current = null;
    this.waypoint = vec();
    this.previous = null; // the arc just used, excluded from the next draw
  }

  // Route from start to end. Returns false when the first advance
  // finds no next arc.
  init(start, end, mask, clearance, random) {
    const g = this.graph;
    copy(this.exact, end);
    this.mask = mask;
    this.clearance = clearance;
    this.previous = null;
    this.secondary = null;
    const s = g.attach(start, mask, { type: 0, hub: null, arc: null });
    const e = g.attach(end, mask, { type: 0, hub: null, arc: null });
    this.attachType = e.type;
    let S = s.arc;
    let T = e.arc;
    if (s.type === 0) S = g.nearestArc(start, mask);
    if (e.type === 0) T = g.nearestArc(end, mask);
    const Q = (u, v) => g.select(null, u, v, mask, random);

    if (s.type === 1) {
      const H = s.hub;
      if (e.type === 1) {
        this.primary = e.hub;
        if (H === e.hub) return this.finish(e.hub);
        this.current = H;
        copy(this.waypoint, start);
        return this.advance(random);
      }
      if (!T) return this.lost();
      // Hub to an arc: done if the hub is one of its ends.
      if (H === T.a || H === T.b) {
        this.primary = H;
        return this.finish(H);
      }
      this.current = H;
      copy(this.waypoint, start);
      const toC = Q(T.a, H);
      const toD = Q(T.b, H);
      this.pickDestination(T, toC, toD, end);
      return this.advance(random);
    }

    if (!S) return this.lost();
    const A = S.a;
    const B = S.b;
    if (e.type === 1) {
      const J = e.hub;
      this.primary = J;
      if (J === A || J === B) return this.finish(J);
      copy(this.waypoint, start);
      const fromA = Q(A, J);
      const fromB = Q(B, J);
      if (fromA === S) {
        this.current = A;
      } else if (fromB === S) {
        this.current = B;
      } else {
        this.current = d2(A, start) < d2(B, start) ? B : A;
        this.previous = g.alternate(this.current, S, mask, random);
      }
      return this.advance(random);
    }

    if (!T) return this.lost();
    if (S === T) {
      this.primary = null;
      this.current = null;
      copy(this.waypoint, this.exact);
      return true;
    }
    if (A === T.a || A === T.b) {
      this.primary = A;
      this.current = B;
      copy(this.waypoint, start);
      return this.advance(random);
    }
    if (B === T.a || B === T.b) {
      this.primary = B;
      this.current = A;
      copy(this.waypoint, start);
      return this.advance(random);
    }
    // Two arcs with no shared end: four draws decide the starting end, two
    // more the finishing end; the first hub waypoint is placed directly.
    copy(this.waypoint, start);
    const q1 = Q(A, T.a);
    const q2 = Q(A, T.b);
    const q3 = Q(B, T.a);
    const q4 = Q(B, T.b);
    let current;
    if (q1 === S && q2 === S) {
      current = A;
    } else if (q3 === S && q4 === S) {
      current = B;
    } else {
      if (q1 === q2) current = B;
      else if (q3 === q4) current = A;
      else current = d2(A, start) < d2(B, start) ? B : A;
      // Its result is replaced below, but the draw stays.
      this.previous = g.alternate(current, S, mask, random);
    }
    const R = otherEnd(S, current);
    this.pickDestination(T, Q(T.a, R), Q(T.b, R), end);
    this.previous = S;
    this.current = R;
    this.shell(this.current, random, this.waypoint);
    return true;
  }

  // The finishing end of destination arc T from the two draws toward it:
  // a draw that comes back along T means the route enters from the other
  // end; otherwise the end nearer the exact destination, ties to the second.
  pickDestination(T, fromC, fromD, end) {
    let chosen;
    if (fromC === T) chosen = T.b;
    else if (fromD === T) chosen = T.a;
    else chosen = d2(T.a, end) < d2(T.b, end) ? T.a : T.b;
    this.primary = chosen;
    this.secondary = otherEnd(T, chosen);
  }

  finish(hub) {
    this.current = hub;
    copy(this.waypoint, this.exact);
    return true;
  }

  // No compatible arc anywhere: the 2004 game would dereference null here.
  lost() {
    this.primary = this.secondary = this.current = null;
    copy(this.waypoint, this.exact);
    return false;
  }

  // Next hub. Reaching either destination hub restores the exact
  // destination; entering a true hub destination aims straight at the point,
  // any other hub at a random point on its shell.
  advance(random) {
    if (this.current === this.primary || this.current === this.secondary) {
      this.primary = this.secondary = this.current = this.previous = null;
      copy(this.waypoint, this.exact);
      return true;
    }
    const arc = this.graph.select(this.previous, this.current, this.primary, this.mask, random);
    if (!arc) return false;
    this.previous = arc;
    this.current = otherEnd(arc, this.current);
    if (this.current !== this.primary || this.attachType !== 1) {
      this.shell(this.current, random, this.waypoint);
      return true;
    }
    copy(this.waypoint, this.exact);
    return true;
  }

  // A random point on the hub's shell, inset by the clearance: the
  // direction is a normalized random cube vector (three uniform draws, the
  // first becoming z), not a uniform sphere sample.
  shell(hub, random, out) {
    const r = Math.max(0, f32(hub.radius - this.clearance));
    const z = f32(random.uniform() - 0.5) * 2;
    const y = f32(random.uniform() - 0.5) * 2;
    const x = f32(random.uniform() - 0.5) * 2;
    const v = normalize(vec(x, y, z));
    return set(out, f32(hub.x + v.x * r), f32(hub.y + v.y * r), f32(hub.z + v.z * r));
  }

  // The waypoint is a hub shell point (not the exact destination).
  get regenerable() {
    return Boolean(this.current) && (this.current !== this.primary || this.attachType !== 1);
  }

  // Replace a waypoint that sits inside a barrier: up to 100 new
  // shell points. True when any replacement was made, even if the last one is
  // still blocked; the planner rejects a blocked destination later.
  retry(radius, random) {
    if (!this.regenerable) return false;
    let made = false;
    let clear = this.barriers.pointClear(this.waypoint, radius, this.mask);
    for (let i = 0; !clear && i < 100; i++) {
      this.shell(this.current, random, this.waypoint);
      made = true;
      clear = this.barriers.pointClear(this.waypoint, radius, this.mask);
    }
    return made;
  }
}
