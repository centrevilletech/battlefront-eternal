// Classic AI navigation (src/ai/classic/nav): synthetic fixtures, plus
// optional reference traces measured in the original game and the extracted
// public/maps data when present (skipped otherwise; never copied here).
import { test, describe } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import * as THREE from "three";
import { ClassicRandom } from "../src/ai/random.js";
import {
  ClassicNav, NavGraph, GraphCursor, BarrierSet, barrierFromRecord, inside, crossing, segment, detour, ejection,
  BarrierSearch, OpenHeap, PathPlanner, PathExecutor, NavControl, MovementRequest, GroundGotoHelper,
  analogs, navigationInput, lookahead, flyerCircle, buildHints, HintSet, pickStance, Boundary, KIND, EXEC,
} from "../src/ai/classic/nav/index.js";

const f32 = Math.fround;
const RESEARCH = new URL("../extract/ai-research/", import.meta.url);
const MAPS = new URL("../public/maps/", import.meta.url);

function research(path) {
  const url = new URL(path, RESEARCH);
  return fs.existsSync(url) ? JSON.parse(fs.readFileSync(url, "utf8")) : null;
}

// ---- synthetic builders ----

function graphJson(hubs, arcs) {
  const N = hubs.length;
  return {
    info: [N, arcs.length, 5],
    hubs: hubs.map((h, i) => ({ name: `Hub${i}`, position: h.p, radius: h.r, arcs: [...h.arcs, 255, 255, 255, 255, 255, 255, 255, 255].slice(0, 8) })),
    arcs: arcs.map((a, i) => ({ name: `Connection${i}`, hubs: a.hubs, mask: a.mask ?? 0x3f })),
    routes: { file: "ai-nav.bin", byteOffset: 0, count: 5 * N * N, tables: 5, order: "hub,table,destination" },
  };
}

function makeGraph(hubs, arcs, route = () => 0, report) {
  const json = graphJson(hubs, arcs);
  const N = hubs.length;
  const words = new Uint32Array(5 * N * N);
  for (let h = 0; h < N; h++) for (let t = 0; t < 5; t++) for (let d = 0; d < N; d++) words[(h * 5 + t) * N + d] = route(h, t, d) >>> 0;
  return new NavGraph(json, words, report);
}

function packed(candidates) {
  let w = 0;
  candidates.forEach(([slot, weight], i) => {
    w += ((weight << 3) | slot) * 2 ** (8 * i);
  });
  return w >>> 0;
}

// An unrotated-in-world footprint, as the retail data stores barriers: unit
// basis rows with the mirrored handedness (det -1), translation z negated by
// the loader. center is the runtime center.
function barrier(index, center, hx, hz, flags = 1, angle = 0) {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  return barrierFromRecord({
    name: `b${index}`,
    xfrm: [f32(c), 0, f32(-s), 0, 1, 0, f32(-s), 0, f32(-c), center[0], center[1], -center[2]],
    size: [hx, 0, hz],
    flags,
    properties: [],
  }, index);
}

function kinematicUnit(position, options = {}) {
  return {
    id: options.id ?? 1,
    kind: options.kind ?? KIND.soldier,
    generation: 1,
    alive: true,
    position: new THREE.Vector3(...position),
    velocity: new THREE.Vector3(),
    classDef: { MaxSpeed: 6.5, MaxStrafeSpeed: 4.5, MaxTurnSpeed: 4, MaxPitchSpeed: 0.5, ...options.classDef },
    soldier: { aimYaw: options.yaw ?? 0, aimPitch: 0, posture: "stand", air: null, lock: 0, fuel: 0, avoidance: new THREE.Vector3(), jet: { minBorder: 0.1, recharge: 0 } },
    origin(out) {
      return out.set(this.position.x, this.position.y + 1.7, this.position.z);
    },
    forward(out) {
      const s = this.soldier;
      return out.set(Math.sin(s.aimYaw) * Math.cos(s.aimPitch), Math.sin(s.aimPitch), Math.cos(s.aimYaw) * Math.cos(s.aimPitch));
    },
  };
}

// The port's drive axes for a yaw: forward (sin, 0, cos), right (-cos, 0, sin).
function step(unit, intent, dt) {
  const s = unit.soldier;
  s.aimYaw -= unit.classDef.MaxTurnSpeed * intent.turn * dt;
  s.aimPitch += unit.classDef.MaxPitchSpeed * intent.pitch * dt;
  const sin = Math.sin(s.aimYaw);
  const cos = Math.cos(s.aimYaw);
  const vf = unit.classDef.MaxSpeed * intent.forward;
  const vr = unit.classDef.MaxStrafeSpeed * intent.right;
  unit.velocity.set(sin * vf - cos * vr, 0, cos * vf + sin * vr);
  unit.position.addScaledVector(unit.velocity, dt);
}

// ---- routing ----

describe("packed weighted routing", () => {
  // Hub 0 with arcs 0, 1, 2 (all to hub 1), the selector model the native
  // fixture validated: one raw draw modulo (31 − previous weight), weights
  // subtracted in byte order including the excluded slot's, accept at <= 0
  // unless excluded, restart at the end; previous weight 31 picks uniformly
  // among the other compatible arcs.
  const cases = [
    [[[0, 15], [1, 16]], null],
    [[[0, 15], [1, 15]], null],
    [[[0, 0], [1, 31]], null],
    [[[0, 10], [1, 10], [2, 11]], 0],
    [[[0, 31]], 0],
    [[], null],
  ];

  function model(weights, prev, seed) {
    const a = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    const b = (Math.imul(a, 1664525) + 1013904223) >>> 0;
    const draw = ((b & 0xffff0000) | (a >>> 16)) >>> 0;
    if (!weights.length) return { arc: null, state: seed };
    const prevWeight = prev === null ? 0 : (weights.find(([s]) => s === prev)?.[1] ?? 0);
    const total = 31 - prevWeight;
    if (total <= 0) {
      const choices = [0, 1, 2].filter((i) => i !== prev);
      return { arc: choices[draw % choices.length], state: b };
    }
    let rem = draw % total;
    for (;;) {
      for (const [s, v] of weights) {
        rem -= v;
        if (rem <= 0 && s !== prev) return { arc: s, state: b };
      }
    }
  }

  test("six native fixture cases × 128 seeds: arc and generator state", () => {
    const oracle = research("round8/navigation/routes.json");
    if (oracle) assert.deepEqual(oracle.native_packed_cases.map((c) => [c.weights, c.previous]), cases);
    for (const [weights, prev] of cases) {
      const word = packed(weights);
      const g = makeGraph(
        [{ p: [0, 0, 0], r: 1, arcs: [0, 1, 2] }, { p: [10, 0, 0], r: 1, arcs: [0, 1, 2] }],
        [{ hubs: [0, 1], mask: 1 }, { hubs: [0, 1], mask: 1 }, { hubs: [0, 1], mask: 1 }],
        (h, t, d) => (h === 0 && t === 0 && d === 1 ? word : 0),
      );
      for (let seed = 0; seed < 128; seed++) {
        const rng = new ClassicRandom(seed);
        const got = g.select(prev === null ? null : g.arcs[prev], g.hubs[0], g.hubs[1], 1, rng);
        const want = model(weights, prev, seed);
        assert.equal(got ? got.index : null, want.arc, `case ${JSON.stringify(weights)} prev ${prev} seed ${seed}`);
        assert.equal(rng.state, want.state, `generator after case ${JSON.stringify(weights)} seed ${seed}`);
      }
    }
  });

  test("sum 30 words still finish within one pass; an excluded slot's weight still subtracts", () => {
    // 15+15 with no previous: draws of 30 land on the second candidate.
    const g = makeGraph(
      [{ p: [0, 0, 0], r: 1, arcs: [0, 1, 2] }, { p: [10, 0, 0], r: 1, arcs: [0, 1, 2] }],
      [{ hubs: [0, 1], mask: 1 }, { hubs: [0, 1], mask: 1 }, { hubs: [0, 1], mask: 1 }],
      () => packed([[0, 10], [1, 10], [2, 11]]),
    );
    // With slot 0 excluded the total is 21 and slot 0's 10 is still taken
    // off first, so slot 1 always wins.
    for (let seed = 0; seed < 64; seed++) assert.equal(g.select(g.arcs[0], g.hubs[0], g.hubs[1], 1, new ClassicRandom(seed)).index, 1);
  });

  test("table selector: lowest set bit; masks past the five tables are refused", () => {
    const reports = [];
    const g = makeGraph([{ p: [0, 0, 0], r: 1, arcs: [0] }, { p: [5, 0, 0], r: 1, arcs: [0] }], [{ hubs: [0, 1], mask: 0x3f }], () => packed([[0, 31]]), (k, d) => reports.push([k, d]));
    assert.deepEqual([0, 1, 2, 3, 4, 8, 16, 31, 63].map((m) => g.table(m)), [0, 0, 1, 0, 2, 3, 4, 0, 0]);
    assert.equal(g.table(32), -1);
    assert.equal(g.select(null, g.hubs[0], g.hubs[1], 32, new ClassicRandom(1)), null);
    assert.equal(reports[0][0], "routeTable");
  });

  test("candidate arcs are not rechecked against the mask (shipped data relies on it)", () => {
    const g = makeGraph([{ p: [0, 0, 0], r: 1, arcs: [0] }, { p: [5, 0, 0], r: 1, arcs: [0] }], [{ hubs: [0, 1], mask: 0 }], () => packed([[0, 31]]));
    assert.equal(g.select(null, g.hubs[0], g.hubs[1], 1, new ClassicRandom(1)).index, 0);
  });

  test("alternate arc: uniform among other compatible arcs, else the previous arc", () => {
    const g = makeGraph(
      [{ p: [0, 0, 0], r: 1, arcs: [0, 1, 2] }, { p: [5, 0, 0], r: 1, arcs: [0, 1, 2] }],
      [{ hubs: [0, 1], mask: 1 }, { hubs: [0, 1], mask: 2 }, { hubs: [0, 1], mask: 1 }],
    );
    const counts = [0, 0, 0];
    const rng = new ClassicRandom(7);
    for (let i = 0; i < 200; i++) counts[g.alternate(g.hubs[0], g.arcs[0], 1, rng).index]++;
    assert.deepEqual(counts, [0, 0, 200]); // arc 1 is incompatible, arc 0 excluded
    assert.equal(g.alternate(g.hubs[0], g.arcs[2], 2, new ClassicRandom(1)).index, 1);
    const only = makeGraph([{ p: [0, 0, 0], r: 1, arcs: [0] }, { p: [5, 0, 0], r: 1, arcs: [0] }], [{ hubs: [0, 1], mask: 1 }]);
    assert.equal(only.alternate(only.hubs[0], only.arcs[0], 1, new ClassicRandom(1)), only.arcs[0]);
  });

  test("a word the native selector could never finish is refused (safe port)", () => {
    const reports = [];
    const g = makeGraph([{ p: [0, 0, 0], r: 1, arcs: [0, 1] }, { p: [5, 0, 0], r: 1, arcs: [0, 1] }], [{ hubs: [0, 1], mask: 1 }, { hubs: [0, 1], mask: 1 }], () => packed([[1, 0]]), (k) => reports.push(k));
    // Draw 0 accepts at once; any other draw loops natively.
    let refused = 0;
    for (let seed = 0; seed < 32; seed++) if (g.select(null, g.hubs[0], g.hubs[1], 1, new ClassicRandom(seed)) === null) refused++;
    assert.ok(refused > 0);
    assert.ok(reports.includes("routeHang"));
  });
});

describe("graph attachment", () => {
  const g = makeGraph(
    [
      { p: [0, 0, 0], r: 5, arcs: [0, 1] },
      { p: [1, 0, 0], r: 3, arcs: [0] },
      { p: [40, 0, 0], r: 4, arcs: [1, 2] },
      { p: [40, 0, 40], r: 2, arcs: [2] },
    ],
    [{ hubs: [0, 1], mask: 1 }, { hubs: [0, 2], mask: 1 }, { hubs: [2, 3], mask: 3 }],
  );

  test("smallest containing compatible hub, strict containment", () => {
    assert.equal(g.attach({ x: 0.5, y: 0, z: 0 }, 1).hub.index, 1);
    assert.equal(g.attach({ x: -4, y: 0, z: 0 }, 1).hub.index, 0);
    assert.equal(g.attach({ x: 5, y: 0, z: 0 }, 1).type !== 1 || g.attach({ x: 5, y: 0, z: 0 }, 1).hub.index !== 0, true);
  });

  test("arcs by tapered corridor when no hub contains the point", () => {
    const a = g.attach({ x: 20, y: 0, z: 3 }, 1);
    assert.equal(a.type, 2);
    assert.equal(a.arc.index, 1);
    const none = g.attach({ x: 20, y: 0, z: 30 }, 1);
    assert.equal(none.type, 0);
    assert.equal(g.nearestArc({ x: 20, y: 0, z: 30 }, 1).index, 2);
  });

  test("an incompatible mask skips hubs and arcs", () => {
    assert.equal(g.attach({ x: 40, y: 0, z: 39 }, 2).hub.index, 3);
    assert.equal(g.attach({ x: 0, y: 0, z: 0 }, 4).type, 0);
  });
});

describe("graph cursor", () => {
  // A chain 0-1-2-3 plus a branch 1-4; every word points straight along the
  // chain with weight 31.
  const hubs = [
    { p: [0, 0, 0], r: 2, arcs: [0] },
    { p: [20, 0, 0], r: 2, arcs: [0, 1, 3] },
    { p: [40, 0, 0], r: 2, arcs: [1, 2] },
    { p: [60, 0, 0], r: 2, arcs: [2] },
    { p: [20, 0, 20], r: 2, arcs: [3] },
  ];
  const arcs = [{ hubs: [0, 1], mask: 1 }, { hubs: [1, 2], mask: 1 }, { hubs: [2, 3], mask: 1 }, { hubs: [1, 4], mask: 1 }];
  // Slot of the next arc, per (hub, destination): hub 1's slots are arcs 0,
  // 1, 3; hub 2's are 1, 2; the end hubs have one slot each.
  const next = { "0,1": 0, "0,2": 0, "0,3": 0, "0,4": 0, "1,0": 0, "1,2": 1, "1,3": 1, "1,4": 2, "2,0": 0, "2,1": 0, "2,3": 1, "2,4": 0, "3,0": 0, "3,1": 0, "3,2": 0, "3,4": 0, "4,0": 0, "4,1": 0, "4,2": 0, "4,3": 0 };
  const route = (h, t, d) => (t === 0 && next[`${h},${d}`] !== undefined ? packed([[next[`${h},${d}`], 31]]) : 0);
  const g = makeGraph(hubs, arcs, route);
  const barriers = new BarrierSet([]);

  test("hub to the same hub aims straight at the point, no draws", () => {
    const c = new GraphCursor(g, barriers);
    const rng = new ClassicRandom(3);
    c.init({ x: 0, y: 0, z: 0 }, { x: 1, y: 0, z: 0 }, 1, 0, rng);
    assert.equal(rng.state, 3);
    assert.deepEqual([c.waypoint.x, c.waypoint.z], [1, 0]);
  });

  test("hub to hub: advance draws one arc and one shell point per hub", () => {
    const c = new GraphCursor(g, barriers);
    const rng = new ClassicRandom(11);
    c.init({ x: 0, y: 0, z: 0 }, { x: 60, y: 0, z: 0 }, 1, 0, rng);
    assert.equal(c.current.index, 1);
    // One selector draw, then three shell draws (z, y, x).
    const probe = new ClassicRandom(11);
    probe.next();
    const z = (probe.uniform() - 0.5) * 2;
    const y = (probe.uniform() - 0.5) * 2;
    const x = (probe.uniform() - 0.5) * 2;
    const l = Math.hypot(x, y, z);
    assert.ok(Math.abs(c.waypoint.x - (20 + 2 * x / l)) < 1e-4 && Math.abs(c.waypoint.z - 2 * z / l) < 1e-4);
    assert.equal(rng.state, probe.state);
    let steps = 0;
    while (c.current && steps++ < 10) assert.ok(c.advance(rng));
    assert.deepEqual([c.waypoint.x, c.waypoint.z], [60, 0]);
  });

  test("Droideka clearance insets the shell by 4", () => {
    const c = new GraphCursor(g, barriers);
    const out = { x: 0, y: 0, z: 0 };
    c.clearance = 4;
    c.shell({ x: 0, y: 0, z: 0, radius: 10 }, new ClassicRandom(5), out);
    assert.ok(Math.abs(Math.hypot(out.x, out.y, out.z) - 6) < 1e-4);
  });

  test("arc start to an unrelated hub: the end whose draw comes back along the arc starts", () => {
    // Start on arc 1 (hubs 1-2, outside both), go to hub 4: Q(1,4) is arc 3,
    // Q(2,4) is arc 1 itself, so the route starts at hub 2 and goes back
    // through hub 1. Two draws, then one more and a shell in the advance.
    const c = new GraphCursor(g, barriers);
    const rng = new ClassicRandom(21);
    c.init({ x: 25, y: 0, z: 0 }, { x: 20, y: 0, z: 20 }, 1, 0, rng);
    assert.equal(c.primary.index, 4);
    assert.equal(c.current.index, 1);
    assert.equal(c.previous.index, 1);
    const probe = new ClassicRandom(21);
    for (let i = 0; i < 6; i++) probe.next();
    assert.equal(rng.state, probe.state);
  });

  test("arc start to an unrelated hub, neither draw back along it: the farther end starts, alternate drawn", () => {
    // S = arc 1 (hubs 1-2), destination hub 4. Hub 1 routes on along arc 3
    // and hub 2 along arc 2, so neither comes back along S: hub 1 is nearer
    // the start, so hub 2 starts and its alternate arc (arc 2) is drawn. The
    // advance from hub 2 excludes arc 2, whose full weight leaves nothing,
    // so the alternate pick takes arc 1 back to hub 1, then a shell point.
    const words = (h, t) => (t === 0 ? packed([[h === 1 ? 2 : h === 2 ? 1 : 0, 31]]) : 0);
    const flat = makeGraph(hubs, arcs, words);
    const c = new GraphCursor(flat, barriers);
    const rng = new ClassicRandom(8);
    c.init({ x: 24, y: 0, z: 0 }, { x: 20, y: 0, z: 20 }, 1, 0, rng);
    assert.deepEqual([c.primary.index, c.current.index, c.previous.index], [4, 1, 1]);
    const probe = new ClassicRandom(8);
    for (let i = 0; i < 2 + 1 + 1 + 3; i++) probe.next();
    assert.equal(rng.state, probe.state, "two route draws, the alternate, the advance's alternate, a shell");
  });

  test("disjoint arcs: four draws pick the start end, two the finishing end, then a shell", () => {
    // S = arc 0 (hubs 0-1), T = arc 2 (hubs 2-3). Q(0,2) and Q(0,3) both come
    // back along S, so hub 0 starts; from hub 1, Q(3,1) is T itself, so the
    // route finishes through hub 2.
    const c = new GraphCursor(g, barriers);
    const rng = new ClassicRandom(33);
    c.init({ x: 10, y: 0, z: 0 }, { x: 50, y: 0, z: 0 }, 1, 0, rng);
    assert.deepEqual([c.current.index, c.primary.index, c.secondary.index, c.previous.index], [1, 2, 3, 0]);
    const probe = new ClassicRandom(33);
    for (let i = 0; i < 6 + 3; i++) probe.next();
    assert.equal(rng.state, probe.state, "six route draws and three shell draws");
    assert.ok(Math.abs(Math.hypot(c.waypoint.x - 20, c.waypoint.y, c.waypoint.z) - 2) < 1e-4);
    assert.ok(c.advance(rng));
    assert.equal(c.current.index, 2);
    assert.ok(c.advance(rng));
    assert.deepEqual([c.current, c.waypoint.x, c.waypoint.z], [null, 50, 0]);
  });

  test("same arc goes straight to the point", () => {
    const c = new GraphCursor(g, barriers);
    const rng = new ClassicRandom(2);
    c.init({ x: 26, y: 0, z: 0 }, { x: 34, y: 0, z: 0 }, 1, 0, rng);
    assert.equal(c.current, null);
    assert.equal(rng.state, 2);
    assert.deepEqual([c.waypoint.x, c.waypoint.z], [34, 0]);
  });

  test("arcs sharing an end: the shared hub is the destination, the start's far end current", () => {
    const c = new GraphCursor(g, barriers);
    c.init({ x: 10, y: 0, z: 0 }, { x: 30, y: 0, z: 0 }, 1, 0, new ClassicRandom(4));
    // Hub 0 → hub 1 along arc 0; an arc destination keeps a shell waypoint
    // on hub 1 and finishes on the next advance.
    assert.equal(c.primary.index, 1);
    assert.equal(c.current.index, 1);
    assert.equal(c.previous.index, 0);
    assert.ok(Math.abs(Math.hypot(c.waypoint.x - 20, c.waypoint.y, c.waypoint.z) - 2) < 1e-4);
    assert.ok(c.advance(new ClassicRandom(4)));
    assert.equal(c.primary, null);
    assert.deepEqual([c.waypoint.x, c.waypoint.z], [30, 0]);
  });

  test("hub to an arc ending at that hub aims at the point", () => {
    const c = new GraphCursor(g, barriers);
    c.init({ x: 20, y: 0, z: 0 }, { x: 30, y: 0, z: 0 }, 1, 0, new ClassicRandom(4));
    assert.deepEqual([c.waypoint.x, c.waypoint.z], [30, 0]);
  });

  test("equal endpoint distance: the second end wins the tie", () => {
    // Hub 0 → arc 2 (hubs 2 and 3) with no draw coming back along arc 2 and
    // the exact end equidistant from both ends.
    const flat = makeGraph(hubs, arcs, () => 0);
    const c = new GraphCursor(flat, barriers);
    c.attachType = 0;
    c.pickDestination(flat.arcs[2], null, null, { x: 50, y: 0, z: 0 });
    assert.equal(c.primary.index, 3);
    assert.equal(c.secondary.index, 2);
  });
});

// ---- barriers ----

describe("barrier geometry", () => {
  function fixtureBarrier(angle) {
    const c = f32(Math.cos(angle));
    const s = f32(Math.sin(angle));
    // The native fixture's runtime matrix rows (c,0,s), (0,1,0), (-s,0,c).
    const b = barrierFromRecord({ name: "f", xfrm: [c, 0, s, 0, 1, 0, -s, 0, c, 0, 0, 0], size: [2, 100, 1], flags: 1, properties: [] }, 0);
    b.bound = f32(Math.sqrt(5));
    return b;
  }

  test("native point and segment cases (round9 barriers.json)", (t) => {
    const cases = research("round9/navigation/barriers.json");
    if (!cases) return t.skip("reference values absent");
    for (const c of cases) {
      if (c.kind === "point") {
        const b = fixtureBarrier(c.angle);
        const co = Math.cos(c.angle);
        const si = Math.sin(c.angle);
        const [x, y, z] = c.local;
        const p = { x: f32(co * x - si * z), y: f32(y), z: f32(si * x + co * z) };
        assert.equal(inside(b, p.x, p.y, p.z, 0) ? 1 : 0, c.result, JSON.stringify(c));
      } else {
        const b = fixtureBarrier(0);
        const work = { count: 0 };
        const hit = { distanceSq: 0, normal: { x: 0, y: 0, z: 0 } };
        const seg = segment({ x: c.start[0], y: c.start[1], z: c.start[2] }, { x: c.end[0], y: c.end[1], z: c.end[2] });
        const got = crossing(b, seg, 0, work, hit);
        assert.equal(got ? 1 : 0, c.hit, JSON.stringify(c));
        assert.equal(work.count, c.work, `work ${JSON.stringify(c)}`);
        if (got) {
          assert.ok(Math.abs(hit.distanceSq - c.distanceSquared) < 1e-6);
          assert.deepEqual([hit.normal.x, hit.normal.y, hit.normal.z].map((v) => v + 0), c.normal);
        }
      }
    }
  });

  test("rotated footprint, exactly-on-edge point, wholly inside segment", () => {
    const b = barrier(0, [10, 0, 10], 2, 1, 1, Math.PI / 4);
    assert.ok(inside(b, 10, 0, 10, 0));
    assert.ok(inside(b, 10, 50, 10, 0)); // height does not count for flat barriers
    const edge = barrier(1, [0, 0, 0], 2, 1);
    assert.equal(inside(edge, 2, 0, 0, 0), false);
    assert.equal(inside(edge, 1.9, 0, 0, 0), true);
    assert.equal(inside(edge, 2.4, 0, 0, 0.5), true); // grown by the clearance
    const set = new BarrierSet([edge]);
    assert.equal(set.segmentBlocked({ x: -0.5, y: 0, z: 0 }, { x: 0.5, y: 0, z: 0 }, 0, 1), false);
    assert.equal(set.segmentBlocked({ x: -3, y: 0, z: 0 }, { x: 3, y: 0, z: 0 }, 0, 1), true);
    assert.equal(set.segmentBlocked({ x: -3, y: 0, z: 0 }, { x: 3, y: 0, z: 0 }, 0, 2), false); // mask
    assert.equal(set.pointClear({ x: 0, y: 0, z: 0 }, 0, 1), false);
  });

  test("point correction pushes out through the nearest edge, x/z only", () => {
    const b = barrier(0, [0, 0, 0], 2, 1);
    const set = new BarrierSet([b]);
    const p = { x: 0.5, y: 7, z: 0.8 };
    assert.ok(set.correct(p, 0, 1));
    assert.ok(Math.abs(p.z - 1.01) < 1e-6 && p.x === 0.5 && p.y === 7);
    assert.ok(set.pointClear(p, 0, 1));
    const score = ejection(b, { x: 0.5, y: 0, z: 0.8 }, 0, 0, { x: 0, y: 0, z: 0 });
    assert.ok(Number.isFinite(score));
  });

  test("symmetric detour corners: the left output takes the negative cross side", () => {
    const b = barrier(0, [0, 0, 0], 2, 1);
    const left = { x: 0, y: 0, z: 0 };
    const right = { x: 0, y: 0, z: 0 };
    assert.equal(detour(b, { x: -5, y: 0, z: 0 }, { x: 5, y: 0, z: 0 }, 0, left, right), 3);
    assert.deepEqual([left.x, left.z, right.x, right.z], [-2.5, 1.5, -2.5, -1.5]);
  });
});

// ---- search ----

describe("barrier search", () => {
  test("open heap: equal priorities come out A, D, C, B", () => {
    const h = new OpenHeap(8);
    const nodes = ["A", "B", "C", "D"].map((name) => ({ name, heap: 0 }));
    for (const n of nodes) h.push(n, 1);
    const order = [];
    while (!h.empty) order.push(h.pop().name);
    assert.deepEqual(order, ["A", "D", "C", "B"]);
  });

  test("open heap: mixed scores descend; raise moves only past strictly smaller parents", () => {
    const h = new OpenHeap(512);
    const rng = new ClassicRandom(9);
    const scores = [];
    for (let i = 0; i < 300; i++) {
      const s = rng.int(0, 99);
      scores.push(s);
      h.push({ i, heap: 0 }, s);
    }
    const out = [];
    let last = Infinity;
    while (!h.empty) {
      const n = h.pop();
      out.push(n.i);
      assert.ok(scores[n.i] <= last);
      last = scores[n.i];
    }
    assert.equal(new Set(out).size, 300);
    const r = new OpenHeap(4);
    const a = { name: "a", heap: 0 };
    const b = { name: "b", heap: 0 };
    r.push(a, 5);
    r.push(b, 1);
    r.raise(1, 5);
    assert.equal(r.top(), a);
    r.raise(1, 6);
    assert.equal(r.top(), b);
  });

  test("a full open heap refuses the insert and counts it (safe port)", () => {
    const set = new BarrierSet([]);
    const planner = new PathPlanner(set, { heap: 1 });
    const s = planner.search;
    s.begin({ x: 0, y: 0, z: 0 }, { x: 10, y: 0, z: 0 }, 0, 1);
    assert.equal(s.heap.count, 1);
    s.insert(s.node(1, 0, 0, 1, null));
    assert.equal(s.overflows, 1);
    assert.equal(s.heap.count, 1);
  });

  test("native integrated cases (round9 integrated.json): paths, nodes, closed, work, queries", (t) => {
    const cases = research("round9/navigation/integrated.json");
    if (!cases) return t.skip("reference values absent");
    for (const c of cases) {
      const b = barrierFromRecord({ name: "b", xfrm: [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0], size: [2, 100, 1], flags: 1, properties: [] }, 0);
      b.bound = f32(Math.sqrt(5));
      const list = c.name === "clear" ? [] : [b];
      const set = new BarrierSet(list);
      set.gather = () => list.slice(); // the native fixture supplied every barrier
      let queries = 0;
      const nearest = set.nearest.bind(set);
      set.nearest = (...args) => {
        queries++;
        return nearest(...args);
      };
      const planner = new PathPlanner(set, { searchNodes: c.name === "poolfull" ? 1 : 1024 });
      const s = planner.search;
      if (c.name === "poolfull1024") s.external = 1023;
      const mirror = c.name === "mirror";
      s.begin({ x: mirror ? 5 : -5, y: 0, z: 0 }, { x: mirror ? -5 : 5, y: 0, z: 0 }, 0, 1);
      const work = { count: 0, cap: 10000 };
      set.work = work;
      s.step(work);
      const path = s.reconstruct(() => planner.path());
      assert.equal(s.success, Boolean(c.success), c.name);
      assert.equal(s.finished, Boolean(c.finished), c.name);
      assert.equal(s.live + s.external, c.nodes, `${c.name} nodes`);
      assert.equal(s.closed.length, c.closed, `${c.name} closed`);
      assert.equal(work.count, c.work, `${c.name} work`);
      assert.equal(queries, c.broadphaseQueries, `${c.name} queries`);
      assert.deepEqual(path ? path.nodes.map((n) => [n.x, n.y, n.z]) : [], c.reconstructed, c.name);
    }
  });

  test("lifecycle: empty frontier, root at goal, exhausted budget, exhausted depth", (t) => {
    const set = new BarrierSet([]);
    const planner = new PathPlanner(set);
    const s = planner.search;
    // Empty frontier.
    s.finished = false;
    s.heap.clear();
    s.step({ count: 0, cap: 10 });
    assert.deepEqual([s.finished, s.success, s.heap.count], [true, false, 0]);
    // Root already at the goal.
    s.begin({ x: 1, y: 0, z: 1 }, { x: 1, y: 3, z: 1 }, 0, 1);
    s.step({ count: 0, cap: 10 });
    assert.deepEqual([s.finished, s.success, s.heap.count], [true, true, 1]);
    // Budget exhausted before the first expansion.
    s.begin({ x: 0, y: 0, z: 0 }, { x: 9, y: 0, z: 0 }, 0, 1);
    s.step({ count: 0, cap: 0 });
    assert.deepEqual([s.finished, s.success, s.heap.count], [false, false, 1]);
    // A frontier node 32 deep can expand nothing.
    s.begin({ x: 0, y: 0, z: 0 }, { x: 9, y: 0, z: 0 }, 0, 1);
    s.heap.clear();
    let n = null;
    for (let i = 0; i < 32; i++) n = s.node(-i - 1, 0, 0, i, n);
    s.heap.push(n, 1);
    s.step({ count: 0, cap: 10 });
    assert.deepEqual([s.finished, s.success, s.heap.count], [true, false, 0]);
    const lifecycle = research("round9/navigation/lifecycle.json");
    if (lifecycle) assert.deepEqual(lifecycle.map((c) => [c.kind, c.finished, c.success, c.open]), [["empty", 1, 0, 0], ["goal", 1, 1, 1], ["budget-exhausted", 0, 0, 1], ["depth-exhausted", 1, 0, 0]]);
  });

  test("blocked destination finishes at once, unsuccessfully", () => {
    const set = new BarrierSet([barrier(0, [10, 0, 0], 2, 2)]);
    const planner = new PathPlanner(set);
    assert.equal(planner.search.begin({ x: 0, y: 0, z: 0 }, { x: 10, y: 0, z: 0 }, 0, 1), false);
    assert.equal(planner.search.finished, true);
    assert.equal(planner.search.reconstruct(() => planner.path()), null);
  });

  test("one expansion can overshoot the work budget", () => {
    // Two barriers across the first straight segment: one nearest query
    // tests both, and the recursion to the corners tests more.
    const list = [barrier(0, [0, 0, 0], 2, 1), barrier(1, [5, 0, 0], 1, 1)];
    const set = new BarrierSet(list);
    const planner = new PathPlanner(set);
    planner.search.begin({ x: -10, y: 0, z: 0 }, { x: 10, y: 0, z: 0 }, 0, 1);
    const work = { count: 0, cap: 1 };
    set.work = work;
    planner.search.step(work);
    assert.ok(work.count > work.cap);
  });

  test("search depth and lateral limits route around a barrier field", () => {
    const list = [];
    for (let i = 0; i < 6; i++) list.push(barrier(i, [i * 6 - 15, 0, (i % 2 ? 1 : -1) * 3], 2, 4));
    const set = new BarrierSet(list);
    const planner = new PathPlanner(set);
    const s = planner.search;
    s.begin({ x: -25, y: 0, z: 0 }, { x: 25, y: 0, z: 0 }, 0.5, 1);
    const work = { count: 0, cap: 100000 };
    set.work = work;
    s.step(work);
    assert.ok(s.success);
    const path = s.reconstruct(() => planner.path());
    for (let i = 1; i < path.nodes.length; i++) assert.equal(set.segmentBlocked(path.nodes[i - 1], path.nodes[i], 0.5, 1), false);
  });
});

// ---- planner ----

describe("path planner", () => {
  function owner(planner, nav) {
    const c = new NavControl(nav ?? { planner });
    c.attach(kinematicUnit([0, 0, 0]));
    return c;
  }

  test("a clear segment is answered at once with a two-point path", () => {
    const planner = new PathPlanner(new BarrierSet([]));
    const o = owner(planner);
    assert.ok(planner.request(o, { x: 0, y: 0, z: 0 }, { x: 5, y: 0, z: 0 }, 0.5, 1, 0));
    assert.ok(o.ready && o.published.nodes.length === 2);
    assert.equal(planner.queue.length, 0);
  });

  test("more than 10 outstanding segments disable the shortcut; priority adds the count", () => {
    const planner = new PathPlanner(new BarrierSet([]));
    const o = owner(planner);
    o.segments = 11;
    planner.request(o, { x: 0, y: 0, z: 0 }, { x: 5, y: 0, z: 0 }, 0.5, 1, 1);
    assert.equal(planner.queue.length, 1);
    assert.equal(planner.queue[0].priority, 12);
    planner.service(1);
    assert.equal(planner.queue.length, 1, "priorities above 10 are never served");
    o.consumeSegment();
    o.consumeSegment();
    assert.equal(planner.queue[0].priority, 10);
    planner.service(1);
    assert.ok(o.ready && o.published);
  });

  test("75-slot queue; priority order then arrival order; dead owners dropped", () => {
    const set = new BarrierSet([barrier(0, [5, 0, 0], 1, 1)]);
    const planner = new PathPlanner(set);
    const owners = [];
    for (let i = 0; i < 76; i++) {
      const o = owner(planner);
      owners.push(o);
      const ok = planner.request(o, { x: 0, y: 0, z: 0 }, { x: 10, y: 0, z: 0 }, 0, 1, i % 2);
      assert.equal(ok, i < 75);
    }
    owners[1].reset(); // dead owner: its serial moved on (its request was cancelled)
    owners[2].serial++; // stale request left in the queue
    const r = planner.select();
    assert.equal(r.owner, owners[0]);
    assert.equal(planner.select().owner, owners[4], "priority 0 before 1; stale owner 2 dropped");
  });

  test("cancel resets the publication and forgets the owner's work", () => {
    const set = new BarrierSet([barrier(0, [5, 0, 0], 1, 1)]);
    const planner = new PathPlanner(set);
    const o = owner(planner);
    planner.request(o, { x: 0, y: 0, z: 0 }, { x: 10, y: 0, z: 0 }, 0, 1, 0);
    planner.service(0.1); // cap 0: selects nothing
    planner.cancel(o);
    assert.equal(planner.queue.length, 0);
    assert.equal(o.ready, false);
  });

  test("service publishes the search result; idle afterwards", () => {
    const set = new BarrierSet([barrier(0, [5, 0, 0], 1, 1)]);
    const planner = new PathPlanner(set);
    const o = owner(planner);
    planner.request(o, { x: 0, y: 0, z: 0 }, { x: 10, y: 0, z: 0 }, 0, 1, 0);
    assert.equal(planner.idle, false);
    for (let i = 0; i < 10 && !o.ready; i++) planner.service(1);
    assert.ok(o.ready && o.published.nodes.length >= 3);
    assert.equal(planner.idle, true);
  });

  test("a blocked destination publishes a completed failure", () => {
    const set = new BarrierSet([barrier(0, [10, 0, 0], 2, 2)]);
    const planner = new PathPlanner(set);
    const o = owner(planner);
    planner.request(o, { x: 0, y: 0, z: 0 }, { x: 10, y: 0, z: 0 }, 0, 1, 0);
    planner.service(1);
    assert.deepEqual([o.ready, o.published], [true, null]);
    assert.deepEqual(o.consume(0), { ready: true, success: false });
  });
});

// ---- executor and steering ----

describe("path executor", () => {
  function setup(nodes, position, options = {}) {
    const planner = new PathPlanner(new BarrierSet([]));
    const unit = kinematicUnit(position, options);
    const control = new NavControl({ planner });
    control.attach(unit);
    const path = planner.path();
    for (const n of [...nodes].reverse()) path.prepend(...n);
    control.install(path, true, false);
    return { unit, control, e: control.executor };
  }

  test("native executor and pipeline traces (round9 executor.json, pipeline.json)", (t) => {
    const pipe = research("round9/navigation/pipeline.json");
    if (!pipe) return t.skip("reference values absent");
    const { unit, control, e } = setup(pipe.search.reconstructed, [-5, 0, 0]);
    unit.velocity.set(1, 0, 0);
    control.update(0); // reach the start node: no, the fixture advances directly
    const fresh = setup(pipe.search.reconstructed, [-5, 0, 0]);
    fresh.unit.velocity.set(1, 0, 0);
    for (const a of pipe.advances) {
      fresh.e.advance(fresh.unit);
      assert.deepEqual([fresh.e.waypoint.x, fresh.e.waypoint.y, fresh.e.waypoint.z], a.target);
      assert.equal(fresh.e.complete ? 1 : 0, a.complete);
      assert.equal(fresh.e.path.nodes.length, a.remainingNodes);
      assert.equal(fresh.control.segments, a.flaggedSegments);
      assert.deepEqual(fresh.control.request.steeringTarget.toArray(), a.moveRequest);
    }
    void e;
  });

  test("stuck thresholds: soldier 3 s stationary / 5 s no progress, Droideka 6 / 6", () => {
    for (const [kind, stationary, noProgress] of [[KIND.soldier, EXEC.stationary, EXEC.noProgress], [KIND.droideka, EXEC.droidekaStationary, EXEC.droidekaNoProgress]]) {
      const { unit, e } = setup([[0, 0, 0], [20, 0, 0]], [0, 0, 0], { kind });
      e.advance(unit); // aim at (20, 0, 0)
      let first2 = null;
      for (let t = 0; t < 10; t += 0.25) {
        e.progress(0.25, unit);
        if (e.status === 2 && first2 === null) first2 = t + 0.25;
      }
      assert.equal(first2, stationary + 0.25, "status 2 strictly after the threshold");
      // Moving in a circle around the waypoint: no improvement, anchor resets.
      const ring = setup([[0, 0, 0], [20, 0, 0]], [10, 0, 0], { kind });
      ring.e.advance(ring.unit);
      let first1 = null;
      for (let i = 1; i <= 80; i++) {
        const a = i * 0.5;
        ring.unit.position.set(20 + 10 * Math.cos(a), 0, 10 * Math.sin(a));
        ring.e.progress(0.25, ring.unit);
        if (ring.e.status === 1 && first1 === null) first1 = i * 0.25;
      }
      assert.ok(first1 > noProgress && first1 <= noProgress + 0.5, `status 1 at ${first1}`);
    }
  });

  test("within 0.2 m of the waypoint the bookkeeping is skipped", () => {
    const { unit, e } = setup([[0, 0, 0], [20, 0, 0]], [0, 0, 0]);
    e.advance(unit);
    unit.position.set(19.9, 0, 0);
    for (let i = 0; i < 40; i++) e.progress(0.25, unit);
    assert.equal(e.status, 0);
  });

  test("waypoint passage: plane equality does not pass; degenerate segments do; Flyer within 10 m", () => {
    const { unit, e } = setup([[0, 0, 0], [10, 0, 0], [20, 0, 0]], [0, 0, 0]);
    e.advance(unit);
    unit.position.set(10, 0, 5);
    assert.equal(e.passed(unit), false);
    unit.position.set(10.01, 0, 5);
    assert.equal(e.passed(unit), true);
    e.prev.x = 10;
    e.prev.z = 0;
    assert.equal(e.passed(unit), true);
    const fly = setup([[0, 0, 0], [10, 0, 0], [20, 0, 0]], [0, 0, 0], { kind: KIND.flyer });
    fly.e.advance(fly.unit);
    fly.e.prev.x = 0;
    fly.unit.position.set(20, 0, 0);
    assert.equal(fly.e.passed(fly.unit), true);
    fly.unit.position.set(20.01, 0, 0);
    assert.equal(fly.e.passed(fly.unit), false);
  });

  test("reach radius: 1 m for intermediate and final infantry waypoints", () => {
    const { unit, e } = setup([[0, 0, 0], [10, 0, 0], [20, 0, 0]], [0, 0, 0]);
    e.advance(unit);
    unit.position.set(9.05, 0, 0.3);
    assert.equal(e.reached(unit), true);
    unit.position.set(9, 0, 0.01);
    assert.equal(e.reached(unit), false);
  });

  test("an executor advances at most one node per tick and completes on the last", () => {
    const { unit, control } = setup([[0, 0, 0], [0.1, 0, 0], [0.2, 0, 0]], [0, 0, 0]);
    const e = control.executor;
    control.update(1 / 60);
    assert.equal(e.index, 1);
    control.update(1 / 60);
    assert.equal(e.waypoint.x, f32(0.2));
    control.update(1 / 60);
    assert.equal(control.executor, null, "completed path is dropped after its last steering");
    assert.ok(control.request.valid);
    void unit;
  });
});

describe("steering", () => {
  test("native Soldier/Droideka/Hover/Walker steering cases (round9 steering.json)", (t) => {
    const cases = research("round9/navigation/steering.json");
    if (!cases) return t.skip("reference values absent");
    const planner = new PathPlanner(new BarrierSet([]));
    // The fixture labels Soldier and Droideka the other way round; both
    // behave the same. Its Flyer rows ran with an uninitialized sincos table
    // and are not used (docs/ai-nav.md, Findings).
    const kinds = { soldier: KIND.droideka, droideka: KIND.soldier, hover: KIND.hover, walker: KIND.walker };
    let checked = 0;
    for (const c of cases) {
      if (c.kind === "flyer") continue;
      const unit = kinematicUnit([0, 0, 0], { kind: kinds[c.kind] });
      unit.velocity.set(1, 0, 0);
      const control = new NavControl({ planner });
      control.attach(unit);
      const path = planner.path();
      path.prepend(...c.target);
      path.prepend(-10, 0, 0);
      const e = new PathExecutor(path, control, unit);
      const zero = c.target.every((v) => v === 0);
      Object.assign(e.prev, zero ? { x: 0, y: 0, z: 0 } : { x: -10, y: 0, z: 0 });
      Object.assign(e.waypoint, { x: c.target[0], y: c.target[1], z: c.target[2] });
      unit.soldier.avoidance.set(...c.contact);
      control.avoidSide = c.priorSide;
      e.steer(unit);
      assert.deepEqual(control.request.steeringTarget.toArray().map((v) => v + 0), c.requestedMove, JSON.stringify(c));
      if (c.kind === "soldier" || c.kind === "droideka") {
        assert.equal(control.avoidSide, c.nextSide);
        assert.equal(unit.soldier.avoidance.lengthSq(), 0, "contact vector consumed");
      }
      checked++;
    }
    assert.equal(checked, 20);
  });

  test("lookahead: point past the projection; negative projection; far behind falls to B", () => {
    const out = { x: 0, y: 0, z: 0 };
    lookahead({ x: -10, y: 5, z: 0 }, { x: 10, y: 5, z: 0 }, { x: 0, y: 1, z: 0 }, 1, out);
    assert.deepEqual([out.x, out.y, out.z], [f32(-10 + f32(11.2)), 1, 0]); // 1.1999998, as natively
    lookahead({ x: -2.5, y: 0, z: 1.5 }, { x: 2.5, y: 0, z: 1.5 }, { x: -5, y: 0, z: 0 }, 1, out);
    assert.deepEqual([out.x, out.z], [f32(-3.8), 1.5]);
    lookahead({ x: 0, y: 0, z: 0 }, { x: 2, y: 0, z: 0 }, { x: -30, y: 0, z: 0 }, 1, out);
    assert.deepEqual([out.x, out.z], [2, 0]);
  });

  test("contact sidestep keeps its side while contacts recur and resets without them", () => {
    const planner = new PathPlanner(new BarrierSet([]));
    const unit = kinematicUnit([0, 0, 0]);
    const control = new NavControl({ planner });
    control.attach(unit);
    control.install(planner.segment({ x: 0, y: 0, z: 0 }, { x: 10, y: 0, z: 0 }), true, false);
    const e = control.executor;
    e.advance(unit);
    unit.soldier.avoidance.set(1, 0, 0);
    e.steer(unit);
    assert.equal(control.avoidSide, 1);
    assert.deepEqual(control.request.steeringTarget.toArray().map((v) => v + 0), [0, 0, -5]);
    unit.soldier.avoidance.set(0.5, 0, 0.5);
    e.steer(unit);
    assert.equal(control.avoidSide, 1);
    e.steer(unit);
    assert.equal(control.avoidSide, 0);
  });

  test("Flyer circle steering from the circle formulas", () => {
    const out = { x: 0, y: 0, z: 0 };
    // The steering.json geometry with a working sincos: tangent (10, 0),
    // one radius beyond it.
    flyerCircle({ x: 10, y: 0, z: 0 }, { x: 1, y: 0, z: 0 }, { x: 0, y: 0, z: 0 }, { x: 0.0001, y: 0, z: 1 }, 10, 0, out);
    assert.ok(Math.abs(out.x - 20) < 1e-3 && Math.abs(out.z) < 1e-3);
    flyerCircle({ x: 0, y: 0, z: 0 }, { x: 1, y: 0, z: 0 }, { x: 0, y: 0, z: 0 }, { x: 0.0001, y: 0, z: 1 }, 10, 0, out);
    assert.ok(Math.abs(out.x - 10) < 1e-3 && Math.abs(out.z) < 1e-3);
  });
});

// ---- analogs ----

describe("request to analogs", () => {
  const request = (x, z) => new MovementRequest().write({ x, y: 0, z }, { x, y: 0, z }, { x: 1, y: 0, z: 0 }, 2, -1, 1);
  const out = () => ({ forward: 0, right: 0, turn: 0, pitch: 0 });

  test("square stick envelope makes the velocity point at the target", () => {
    const unit = kinematicUnit([0, 0, 0]);
    for (const [x, z] of [[10, 10], [-10, 10], [3, -8], [-7, -1], [0, 10], [10, 0]]) {
      const a = analogs(unit, request(x, z), { x: 0, y: 1.7, z: 30 }, 1 / 60, out(), { executor: {} });
      assert.ok(Math.max(Math.abs(a.forward), Math.abs(a.right)) > 0.999);
      const vf = 6.5 * a.forward;
      const vr = 4.5 * a.right;
      const vx = -vr; // yaw 0: forward (0, 0, 1), right (-1, 0, 0)
      const vz = vf;
      const cos = (vx * x + vz * z) / (Math.hypot(vx, vz) * Math.hypot(x, z));
      assert.ok(cos > 0.9999, `${x},${z}: ${cos}`);
    }
  });

  test("no-strafe class walks forward and faces the target", () => {
    const unit = kinematicUnit([0, 0, 0], { classDef: { MaxStrafeSpeed: 0 } });
    const a = analogs(unit, request(10, 10), null, 1 / 60, out(), { executor: {} });
    assert.equal(a.right, 0);
    assert.ok(Math.abs(a.forward - Math.SQRT1_2) < 1e-6);
    assert.equal(a.turn, -1, "target on the +x side: turn negative raises aimYaw");
    const behind = analogs(unit, request(0, -10), null, 1 / 60, out(), { executor: {} });
    assert.equal(behind.forward, 0);
  });

  test("slowdown only inside the near radius without a path; Droideka stops under 0.2 m", () => {
    const unit = kinematicUnit([0, 0, 0]);
    const near = request(0, 0.5);
    assert.ok(Math.abs(analogs(unit, near, null, 1 / 60, out(), { executor: null }).forward - 0.5) < 1e-6);
    assert.equal(analogs(unit, near, null, 1 / 60, out(), { executor: {} }).forward, 1);
    const droideka = kinematicUnit([0, 0, 0], { kind: KIND.droideka });
    const a = analogs(droideka, request(0, 0.19), null, 1 / 60, out(), { executor: {} });
    assert.deepEqual([a.forward, a.right], [0, 0]);
    assert.deepEqual(Object.values(analogs(unit, request(0, 0), null, 1 / 60, out())).slice(0, 2), [0, 0]);
  });

  test("turn: one-tick closure clamped to ±1, ±2 with the facing request; pitch 0 behind", () => {
    const unit = kinematicUnit([0, 0, 0]);
    const r = request(0, 10);
    const small = analogs(unit, r, { x: Math.sin(0.01) * 30, y: 1.7, z: Math.cos(0.01) * 30 }, 1 / 60, out());
    assert.ok(Math.abs(small.turn + Math.sin(0.01) / (4 / 60)) < 1e-4);
    assert.equal(analogs(unit, r, { x: 30, y: 1.7, z: 0 }, 1 / 60, out()).turn, -1);
    assert.equal(analogs(unit, r, { x: 30, y: 1.7, z: 0 }, 1 / 60, out(), { facing: true }).turn, -2);
    assert.equal(analogs(unit, r, { x: 0, y: 30, z: 30 }, 1 / 60, out()).pitch, 1);
    assert.equal(analogs(unit, r, { x: 0, y: 30, z: -30 }, 1 / 60, out()).pitch, 0);
  });

  test("driving the analogs reaches a point (port signs)", () => {
    const unit = kinematicUnit([0, 0, 0], { yaw: 2 });
    const control = new NavControl({ planner: new PathPlanner(new BarrierSet([])) });
    control.attach(unit);
    control.request.write({ x: -20, y: 0, z: 15 }, { x: -20, y: 0, z: 15 }, { x: 0, y: 0, z: 1 }, 2, -1, 1);
    const intent = { forward: 0, right: 0, turn: 0, pitch: 0 };
    for (let i = 0; i < 60 * 10; i++) {
      navigationInput(control, unit, intent, 1 / 60, null);
      step(unit, intent, 1 / 60);
    }
    assert.ok(unit.position.distanceTo(new THREE.Vector3(-20, 0, 15)) < 0.5);
  });
});

// ---- hints and boundary ----

describe("hints", () => {
  const record = (kind, props, position = [0, 0, 0]) => ({
    type: "x", kind, bit: 0, name: "h",
    xfrm: [1, 0, 0, 0, 1, 0, 0, 0, 1, ...position],
    properties: props,
  });

  test("radius hints become a ring of four facing in; Minefield keeps its radius", () => {
    const hints = buildHints([record("cover", [["0dba4cb3", "Radius", "5"]], [10, 0, 10]), record("minefield", [["0dba4cb3", "Radius", "7"]])]);
    assert.equal(hints.length, 5);
    const ring = hints.slice(0, 4);
    assert.deepEqual(ring.map((h) => [Math.round(h.position.x), Math.round(h.position.z)]), [[10, 15], [15, 10], [10, 5], [5, 10]]);
    for (const h of ring) assert.ok(h.forward.dot(new THREE.Vector3(10 - h.position.x, 0, 10 - h.position.z).normalize()) > 0.999);
    assert.equal(ring[3].link, ring[0]);
    assert.equal(hints[4].registerRadius, 7);
  });

  test("pool capacity: no room for four copies still deletes the original", () => {
    const records = [record("cover", []), record("cover", [["0dba4cb3", "Radius", "2"]]), record("cover", [])];
    assert.equal(buildHints(records, { capacity: 4 }).length, 2);
    assert.equal(buildHints([record("cover", []), record(null, []), record("cover", [])], { capacity: 1 }).length, 1);
  });

  test("property parsing keeps sscanf quirks", () => {
    const [h] = buildHints([record("cover", [["bf0f1cd1", "PrimaryStance", "6"], ["92c05a59", "SecondaryStance", "x"], ["ec6ee012", "Mode", "3"], ["844ccdb4", "CommandPost", ""]])]);
    assert.equal(h.primaryStance, 6);
    assert.deepEqual([h.secondaryStance, h.secondaryChoices], [1, 3]);
    assert.equal(h.mode, 0);
    assert.equal(h.commandPost, null);
    assert.equal(pickStance(6, new ClassicRandom(3)) > 0, true);
  });

  test("availability: reserved within 4 s is unavailable, equality passes; rings reserve together", () => {
    const set = new HintSet(buildHints([record("cover", [["0dba4cb3", "Radius", "2"]])]));
    const out = [];
    set.reserve(set.list[0], "me", 10);
    assert.equal(set.query(4, { x: 0, y: 0, z: 0 }, 40, 30, out, "you", 13.9).length, 0);
    assert.equal(set.query(4, { x: 0, y: 0, z: 0 }, 40, 30, out, "you", 14).length, 4);
    assert.equal(set.query(8, { x: 0, y: 0, z: 0 }, 40, 30, out).length, 0);
    assert.equal(set.query(4, { x: 100, y: 0, z: 0 }, 40, 30, out).length, 0);
    assert.equal([...set].length, 4);
    assert.equal(set.list[0].siblings.length, 4);
    assert.equal(set.list[0].next, set.list[1]);
    assert.equal(set.list[0].primaryMask, 1);
  });
});

describe("ClassicNav", () => {
  test("an empty nav plans straight local paths and has no boundary", () => {
    const nav = ClassicNav.empty("none");
    assert.equal(nav.graph, null);
    assert.equal(nav.outside({ x: 1e5, y: 0, z: 0 }), false);
    const w = { nav, random: new ClassicRandom(1), time: 0 };
    const unit = kinematicUnit([0, 0, 0]);
    const controller = { unit };
    const helper = nav.helper(controller, w);
    helper.goto(new THREE.Vector3(80, 0, 0));
    helper.tick(1 / 60);
    assert.equal(helper.state, 6);
    assert.ok(controller.navControl.executor);
  });

  test("schema mismatch and bad tables throw", () => {
    assert.throws(() => new ClassicNav({ schema: 2 }, null));
    const json = { schema: 1, graph: graphJson([{ p: [0, 0, 0], r: 1, arcs: [0] }, { p: [1, 0, 0], r: 1, arcs: [0] }], [{ hubs: [0, 1] }]), hints: [], barriers: [], boundary: { references: [] }, paths: [], regions: [] };
    assert.throws(() => new ClassicNav(json, new Uint8Array(8)));
    json.graph.hubs[0].arcs[0] = 7;
    assert.throws(() => new ClassicNav(json, new Uint8Array(4 * 5 * 4)));
  });
});

describe("boundary", () => {
  test("half-plane polygons, union, empty means inside", () => {
    // Clockwise in (x, z) as the stock boundary paths are.
    const square = [[0, 0, 0], [0, 0, 10], [10, 0, 10], [10, 0, 0]];
    const b = new Boundary([square, [[20, 0, 0], [20, 0, 10], [30, 0, 10], [30, 0, 0]]]);
    assert.equal(b.outside({ x: 5, z: 5 }), false);
    assert.equal(b.outside({ x: 25, z: 5 }), false);
    assert.equal(b.outside({ x: 15, z: 5 }), true);
    assert.equal(b.outside({ x: 10, z: 5 }), false, "on an edge is inside");
    assert.equal(new Boundary([]).outside({ x: 1e6, z: 0 }), false);
  });
});

// ---- the ground helper ----

describe("GroundGotoHelper", () => {
  function world(graph, barriers, seed = 1) {
    const json = {
      schema: 1,
      graph: graph ? graph.json : null,
      hints: [],
      barriers,
      boundary: { configs: [], references: [] },
      paths: [],
      regions: [],
      warnings: [],
    };
    const nav = new ClassicNav(json, graph ? graph.bin : null);
    return { nav, random: new ClassicRandom(seed), gameRandom: new ClassicRandom(seed + 1), time: 0 };
  }

  function run(w, helper, unit, controller, seconds, done) {
    const intent = { forward: 0, right: 0, turn: 0, pitch: 0, jump: false, crouch: false, prone: false };
    const dt = 1 / 60;
    for (let i = 0; i < seconds * 60; i++) {
      w.time += dt;
      helper.tick(dt);
      if (w.nav.planner.idle === false) w.nav.service(1);
      navigationInput(controller.navControl, unit, intent, dt, null);
      step(unit, intent, dt);
      if (done()) return i * dt;
    }
    return Infinity;
  }

  const record = (center, hx, hz, flags = 1) => ({ name: "b", xfrm: [1, 0, 0, 0, 1, 0, 0, 0, -1, center[0], center[1], -center[2]], size: [hx, 0, hz], flags, properties: [] });

  test("a local goto walks around a barrier and arrives", () => {
    const w = world(null, [record([0, 0, 0], 3, 6)]);
    const unit = kinematicUnit([-15, 0, 0], { yaw: Math.PI / 2 });
    const controller = { unit };
    const helper = new GroundGotoHelper(controller, w);
    helper.goto(new THREE.Vector3(15, 0, 0));
    assert.equal(helper.state, 2);
    let entered = false;
    const t = run(w, helper, unit, controller, 30, () => {
      if (Math.abs(unit.position.x) < 3 && Math.abs(unit.position.z) < 6) entered = true;
      return helper.arrived && helper.state === 7;
    });
    assert.ok(t < 30, "arrived");
    assert.equal(entered, false, "never crossed the barrier");
    assert.ok(unit.position.distanceTo(new THREE.Vector3(15, 0, 0)) < 1.5);
    assert.deepEqual([w.nav.planner.pools.paths.used, w.nav.planner.pools.nodes.used], [0, 0], "paths go back to the pools");
  });

  test("a long goto follows the graph hub by hub", () => {
    const hubs = [
      { p: [0, 0, 0], r: 3, arcs: [0] },
      { p: [40, 0, 0], r: 3, arcs: [0, 1] },
      { p: [40, 0, 40], r: 3, arcs: [1, 2] },
      { p: [80, 0, 40], r: 3, arcs: [2] },
    ];
    const arcs = [{ hubs: [0, 1], mask: 1 }, { hubs: [1, 2], mask: 1 }, { hubs: [2, 3], mask: 1 }];
    const next = { "0,1": 0, "0,2": 0, "0,3": 0, "1,0": 0, "1,2": 1, "1,3": 1, "2,0": 0, "2,1": 0, "2,3": 1, "3,0": 0, "3,1": 0, "3,2": 0 };
    const json = graphJson(hubs, arcs);
    const N = hubs.length;
    const words = new Uint32Array(5 * N * N);
    for (let h = 0; h < N; h++) for (let d = 0; d < N; d++) if (next[`${h},${d}`] !== undefined) words[(h * 5) * N + d] = packed([[next[`${h},${d}`], 31]]);
    const w = world({ json, bin: new Uint8Array(words.buffer) }, [record([40, 0, 20], 20, 2, 1)]);
    const unit = kinematicUnit([0, 0, 0]);
    const controller = { unit };
    const helper = new GroundGotoHelper(controller, w);
    helper.goto(new THREE.Vector3(80, 0, 40));
    assert.ok(helper.cursor, "graph route for a trip over 50 m");
    let nearMiddle = false;
    const t = run(w, helper, unit, controller, 60, () => {
      if (unit.position.distanceTo(new THREE.Vector3(40, 0, 40)) < 4) nearMiddle = true;
      return helper.arrived && helper.state === 7;
    });
    assert.ok(t < 60, "arrived");
    assert.ok(nearMiddle, "went through hub 2");
    assert.ok(unit.position.distanceTo(new THREE.Vector3(80, 0, 40)) < 1.5);
    assert.equal(helper.failed, false);
  });

  test("an unreachable destination reports failure and stops", () => {
    // A destination walled in: ring of barriers wider than the search may stray.
    const walls = [];
    for (let i = 0; i < 4; i++) {
      const a = (i * Math.PI) / 2;
      walls.push(record([30 + 8 * Math.cos(a), 0, 8 * Math.sin(a)], i % 2 ? 9 : 1, i % 2 ? 1 : 9));
    }
    const w = world(null, walls);
    const unit = kinematicUnit([0, 0, 0]);
    const controller = { unit };
    const helper = new GroundGotoHelper(controller, w);
    helper.goto(new THREE.Vector3(30, 0, 0));
    run(w, helper, unit, controller, 20, () => helper.state === 7);
    assert.equal(helper.failed, true);
    assert.equal(helper.state, 7);
  });

  test("direct mode cuts the path 1 m before a barrier and retries at most five times", () => {
    const w = world(null, [record([10, 0, 0], 1, 20)]);
    const unit = kinematicUnit([0, 0, 0], { yaw: Math.PI / 2 });
    const controller = { unit };
    const helper = new GroundGotoHelper(controller, w);
    helper.goto(new THREE.Vector3(20, 0, 0), { mode: "direct" });
    assert.equal(helper.state, 3);
    assert.equal(helper.adjusted, true);
    const end = controller.navControl.executor.path.nodes.at(-1);
    assert.ok(Math.abs(end.x - (9 - 0.476 - 1)) < 0.01);
    run(w, helper, unit, controller, 60, () => helper.state === 7);
    assert.equal(helper.state, 7);
    assert.ok(helper.retries > 5 || helper.arrived);
  });

  test("a Droideka's direct move takes the corner detour and stops within its 4 m radius", () => {
    const w = world(null, [record([0, 0, 0], 3, 6)]);
    const unit = kinematicUnit([-15, 0, 0], { kind: KIND.droideka, yaw: Math.PI / 2, classDef: { MaxSpeed: 8, MaxStrafeSpeed: 0, MaxTurnSpeed: 3 } });
    const controller = { unit };
    const helper = new GroundGotoHelper(controller, w);
    helper.goto(new THREE.Vector3(15, 0, -9), { mode: "direct" });
    assert.equal(helper.state, 4);
    assert.ok(helper.detourSide > 0);
    let entered = false;
    const t = run(w, helper, unit, controller, 30, () => {
      if (Math.abs(unit.position.x) < 3 && Math.abs(unit.position.z) < 6) entered = true;
      return helper.state === 7;
    });
    assert.ok(t < 30);
    assert.equal(entered, false);
    const left = Math.hypot(unit.position.x - 15, unit.position.z + 9);
    assert.ok(left < 4.5 && left > 2, `stopped ${left} m away`);
  });

  test("opportunistic jet: deadline arithmetic and zero-vector request", () => {
    const w = world(null, []);
    const unit = kinematicUnit([0, 0, 0], { classDef: { JetFuelRechargeRate: 0.06, JetFuelMinBorder: 0.24 } });
    unit.soldier.fuel = 0.1;
    const controller = { unit };
    const helper = new GroundGotoHelper(controller, w);
    helper.goto(new THREE.Vector3(100, 0, 0), { mode: "direct" });
    const probe = new ClassicRandom(w.gameRandom.state);
    w.time = 3;
    helper.jetAttempt();
    assert.equal(helper.jetDeadline, f32(3 + 5 + f32(probe.uniform() * 10)));
    unit.soldier.fuel = 1;
    w.los = () => true;
    let requested = false;
    for (let i = 0; i < 400 && !requested; i++) {
      w.time += 1;
      helper.jetAttempt();
      requested = controller.navControl.jet.pending;
    }
    assert.ok(requested);
    assert.equal(controller.navControl.jet.vector.lengthSq(), 0);
  });
});

// ---- extracted map smoke test ----

describe("nab2 extracted data", () => {
  const jsonUrl = new URL("nab2/ai-nav.json", MAPS);
  const present = fs.existsSync(jsonUrl) && fs.existsSync(new URL("nab2/map.json", MAPS));

  test("graph routes between the nearest hubs of every command post pair (Soldier mask)", (t) => {
    if (!present) return t.skip("public/maps/nab2 not extracted");
    const json = JSON.parse(fs.readFileSync(jsonUrl, "utf8"));
    const bin = fs.readFileSync(new URL("nab2/ai-nav.bin", MAPS));
    const nav = new ClassicNav(json, bin);
    const map = JSON.parse(fs.readFileSync(new URL("nab2/map.json", MAPS), "utf8"));
    const posts = map.commandPosts.map((p) => new THREE.Vector3(p.matrix[12], p.matrix[13], p.matrix[14]));
    const nearest = (p) => nav.graph.hubs.filter((h) => h.mask & 1).reduce((a, h) => (!a || (h.x - p.x) ** 2 + (h.y - p.y) ** 2 + (h.z - p.z) ** 2 < (a.x - p.x) ** 2 + (a.y - p.y) ** 2 + (a.z - p.z) ** 2 ? h : a), null);
    const rng = new ClassicRandom(20261008);
    let pairs = 0;
    for (const a of posts) {
      for (const b of posts) {
        if (a === b) continue;
        const ha = nearest(a);
        const hb = nearest(b);
        const cursor = new GraphCursor(nav.graph, nav.barriers);
        cursor.init({ x: ha.x, y: ha.y, z: ha.z }, { x: hb.x, y: hb.y, z: hb.z }, 1, 0, rng);
        let steps = 0;
        while (cursor.current && steps < 500) {
          assert.ok(cursor.advance(rng), `route ${ha.name} → ${hb.name} stalled`);
          steps++;
        }
        assert.ok(steps < 500, `route ${ha.name} → ${hb.name} did not finish`);
        assert.deepEqual([cursor.waypoint.x, cursor.waypoint.z], [hb.x, hb.z]);
        pairs++;
      }
    }
    assert.equal(pairs, posts.length * (posts.length - 1));
    for (const p of posts) assert.equal(nav.outside(p), false);
  });

  test("a soldier walks from one command post to another through the graph and barriers", (t) => {
    if (!present) return t.skip("public/maps/nab2 not extracted");
    const json = JSON.parse(fs.readFileSync(jsonUrl, "utf8"));
    const nav = new ClassicNav(json, fs.readFileSync(new URL("nab2/ai-nav.bin", MAPS)));
    const map = JSON.parse(fs.readFileSync(new URL("nab2/map.json", MAPS), "utf8"));
    const post = (name) => {
      const p = map.commandPosts.find((c) => c.name === name);
      return new THREE.Vector3(p.matrix[12], p.matrix[13], p.matrix[14]);
    };
    const from = post("cp2");
    const to = post("cp3");
    const w = { nav, random: new ClassicRandom(5), gameRandom: new ClassicRandom(6), time: 0 };
    const unit = kinematicUnit([from.x + 6, from.y, from.z]);
    const controller = { unit };
    const helper = new GroundGotoHelper(controller, w);
    helper.goto(to);
    const intent = { forward: 0, right: 0, turn: 0, pitch: 0 };
    const dt = 1 / 30;
    let arrived = false;
    for (let i = 0; i < 300 / dt && !arrived; i++) {
      w.time += dt;
      helper.tick(dt);
      if (!nav.idle) nav.service(1);
      navigationInput(controller.navControl, unit, intent, dt, null);
      step(unit, intent, dt);
      // No terrain here: keep the height of the next waypoint.
      unit.position.y = controller.navControl.request.waypoint.y;
      arrived = helper.arrived && helper.state === 7;
    }
    assert.ok(arrived, `stopped ${unit.position.distanceTo(helper.destination).toFixed(1)} m short in state ${helper.state}`);
    assert.ok(Math.hypot(unit.position.x - helper.destination.x, unit.position.z - helper.destination.z) < 1.5);
  });
});
