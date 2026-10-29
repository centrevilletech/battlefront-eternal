import { loadNav } from "./data.js";
import { PathPlanner } from "./planner.js";
import { NavControl } from "./control.js";
import { FlyGotoHelper, GroundGotoHelper } from "./goto.js";
import { KIND, traversalMask, traversalRadius } from "./units.js";
import { analogs, navigationInput } from "./analogs.js";
import { JetHintService } from "./jet-hints.js";

// Classic AI navigation: world.nav (docs/ai.md, docs/ai-nav.md).
//
//   const nav = new ClassicNav(json, bin, { posts });
//   world.nav = nav;
//   // scheduler: nav.service(1) when the planner is not idle, and again
//   // after line-of-sight work if that left room
//   // agents:    helper = nav.helper(controller, world); helper.goto(point)
//   // AI input:  navigationInput(controller.navControl, unit, unit.intent, h, aimPoint)
export class ClassicNav {
  constructor(json, bin, options = {}) {
    const data = loadNav(json, bin, options);
    this.map = data.map;
    this.source = data.source;
    this.graph = data.graph;
    this.barriers = data.barriers;
    this.hints = data.hints;
    this.boundary = data.boundary;
    this.paths = data.paths;
    this.regions = data.regions;
    this.warnings = data.warnings;
    this.issues = data.issues;
    this.planner = new PathPlanner(this.barriers, options.pools ?? {});
    this.jetHints = new JetHintService(this);
  }

  // Navigation for a map with no extracted ai-nav.json: no graph, barriers,
  // hints or boundary. Goto still plans (straight local paths).
  static empty(map = null, options = {}) {
    const json = { schema: 1, map, graph: null, hints: [], barriers: [], boundary: { configs: [], references: [] }, paths: [], regions: [], warnings: [] };
    return new ClassicNav(json, null, options);
  }

  // One planner service with the given budget (1.0 per scheduler offer).
  service(budget = 1) {
    this.planner.service(budget);
  }

  // No search running and nothing queued.
  get idle() {
    return this.planner.idle;
  }

  // The controller's navigation state, created on first use.
  control(controller) {
    return controller.navControl ?? (controller.navControl = new NavControl(this, controller));
  }

  // The movement helper a unit's agents use: Flyer bodies get the
  // FlyGotoHelper, everything else the GroundGotoHelper.
  helper(controller, world, options = {}) {
    this.control(controller);
    const flyer = controller.unit?.kind === KIND.flyer;
    return flyer ? new FlyGotoHelper(controller, world, options) : new GroundGotoHelper(controller, world, options);
  }

  // Does the boundary leave this point outside?
  outside(point) {
    return this.boundary.outside(point);
  }

  path(name) {
    return this.paths.find((p) => p.name === name) ?? null;
  }

  // The hint query in object form: { mask, center, halfExtent, cap,
  // requester, now }.
  queryHints(q) {
    return this.hints.query(q.mask, q.center, q.halfExtent, q.cap ?? Infinity, [], q.requester ?? null, q.now ?? 0);
  }

  hintAvailable(hint, now) {
    return this.hints.available(hint, now);
  }

  markHint(hint, owner, now) {
    this.hints.reserve(hint, owner, now);
  }

  plannerIdle() {
    return this.planner.idle;
  }
}

// The services the Classic entry point binds (src/ai/classic/index.js
// navAdapter), against whatever ai.world.nav holds: a ClassicNav, or a
// partial stand-in with some of its parts (tests).
export function classicBinding(ai) {
  const nav = () => ai.world?.nav ?? null;
  return {
    plannerIdle: () => {
      const n = nav();
      if (typeof n?.plannerIdle === "function") return n.plannerIdle();
      return n?.planner ? Boolean(n.planner.idle) : true;
    },
    service: (budget) => {
      const n = nav();
      if (typeof n?.service === "function") return n.service(budget);
      return n?.planner?.service?.(budget);
    },
    pointClear: (p, r, mask) => nav()?.barriers?.pointClear?.(p, r, mask) ?? true,
    clipSegment: (s, e, r, mask, normal) => nav()?.barriers?.obstruct?.(s, e, r, mask, normal) ?? false,
    traversalRadius,
    traversalMask,
    queryHints: (q) => {
      const n = nav();
      if (typeof n?.queryHints === "function") return n.queryHints(q);
      if (typeof n?.hints?.query === "function") return n.hints.query(q.mask, q.center, q.halfExtent, q.cap ?? Infinity, [], q.requester ?? null, q.now ?? 0);
      return [];
    },
    hintAvailable: (hint, now) => !(now < (hint.reservedAt ?? -10) + 4),
    markHint: (hint, owner, now) => {
      let h = hint;
      do {
        h.reservedAt = now;
        h.reservedBy = owner;
        h = h.next ?? h.link ?? null;
      } while (h && h !== hint);
    },
    analogs,
    navigationInput,
  };
}

export { loadNav, routeWords } from "./data.js";
export { JetHintService } from "./jet-hints.js";
export { NavGraph, otherEnd, containsHub, containsArc, corridorGap } from "./graph.js";
export { GraphCursor } from "./cursor.js";
export { BarrierSet, barrierFromRecord, inside, corner, crossing, segment, ejection, detour } from "./barriers.js";
export { BarrierSearch, OpenHeap } from "./search.js";
export { PathPlanner, AIPath, Pool } from "./planner.js";
export { PathExecutor, EXEC, lookahead, nearRadius, passRadius, flyerCircle } from "./executor.js";
export { NavControl, MovementRequest } from "./control.js";
export { GroundGotoHelper, FlyGotoHelper } from "./goto.js";
export { analogs, alongPath, coefficients, navigationInput } from "./analogs.js";
export { HintSet, buildHints, pickStance, pickSecondary, HINT_BITS, STANCE } from "./hints.js";
export { Boundary } from "./boundary.js";
export { KIND, SIZE_BITS, traversalMask, traversalRadius, nominalSpeed, DEFAULT_RADIUS } from "./units.js";
