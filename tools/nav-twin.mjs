// The JavaScript side of the navigation twin gate: generates a
// scenario (synthetic ai-nav data, or a real map's with --mapdir,
// plus units and an operations list), runs the operations against
// src/ai/classic/nav, and writes the trace rust/sim's nav-twin binary
// must reproduce byte for byte (every f64 as its bit pattern).
//
//   node tools/nav-twin.mjs --seed 7 --out DIR [--mapdir MAPDIR]
//
// writes DIR/{ai-nav.json,ai-nav.bin,ops.json,js-trace.txt}. The Rust
// side is `cargo run -p sim --features nav-twin --bin nav-twin -- DIR/ai-nav.json
// DIR/ai-nav.bin DIR/ops.json DIR/rs-trace.txt`; the traces must diff
// empty. Recorded operations are the V1 gate's planner requests and
// executor ticks, with the rest of navigation around them.
import fs from "node:fs";
import path from "node:path";
import * as THREE from "three";
import { ClassicRandom } from "../src/ai/random.js";
import {
  ClassicNav, GroundGotoHelper, FlyGotoHelper,
  inside, detour, ejection, GraphCursor, pickStance, pickSecondary, KIND,
  corridorGap, navigationInput,
} from "../src/ai/classic/nav/index.js";

const f32 = Math.fround;
const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};
const seed = Number(opt("seed", "1")) >>> 0;
const outDir = opt("out", null);
if (!outDir) throw new Error("nav-twin: --out DIR");
const mapDir = opt("mapdir", null);
fs.mkdirSync(outDir, { recursive: true });

// ---------------------------------------------------------------- units
function unitDef(id, kind, position, extra = {}) {
  return {
    id, kind, position, velocity: [0, 0, 0], generation: 1, alive: true,
    classDef: {
      MaxSpeed: 6.5, MaxStrafeSpeed: 4.5, MaxTurnSpeed: 4, MaxPitchSpeed: 0.5,
      ForwardSpeed: 12, TurnRate: 2.2, JetFuelRechargeRate: 0, JetFuelMinBorder: 0.2,
      AISizeType: kind === KIND.hover ? "Hover" : kind === KIND.flyer ? "Flyer" : "Soldier",
      ...(extra.classDef ?? {}),
    },
    soldier: {
      aimYaw: 0.3, aimPitch: 0.05, posture: "stand", air: null, lock: 0, dive: false,
      fuel: 1, avoidance: [0, 0, 0], jet: { minBorder: 0.2, recharge: 0 },
      ...(extra.soldier ?? {}),
    },
    forward: extra.forward ?? [0, 0, 1],
    origin: null,
    ...(extra.rest ?? {}),
  };
}

// ------------------------------------------------------------- synthetic
function syntheticNav(gen) {
  // An 4x3 hub grid, radius 8, nearest-neighbour arcs, BFS route words
  // (one candidate at weight 31, or two at 20/11 where two shortest
  // first hops exist), 14 barriers, hints of every kind, one boundary.
  const hubs = [];
  const at = (i, j) => j * 4 + i;
  for (let j = 0; j < 3; j++) for (let i = 0; i < 4; i++) hubs.push({ p: [i * 30 - 45, 0, j * 30 - 30], r: 8 });
  const arcList = [];
  const addArc = (a, b) => { arcList.push({ hubs: [a, b], mask: 63 }); };
  for (let j = 0; j < 3; j++) for (let i = 0; i < 4; i++) {
    if (i < 3) addArc(at(i, j), at(i + 1, j));
    if (j < 2) addArc(at(i, j), at(i, j + 1));
  }
  const slots = hubs.map(() => []);
  arcList.forEach((arc, index) => { slots[arc.hubs[0]].push(index); slots[arc.hubs[1]].push(index); });
  const N = hubs.length;
  const other = (arc, h) => (arc.hubs[0] === h ? arc.hubs[1] : arc.hubs[0]);
  const words = new Uint32Array(5 * N * N);
  for (let from = 0; from < N; from++) {
    // BFS distances from every destination, for next-hop picks.
    for (let to = 0; to < N; to++) {
      if (from === to) continue;
      const dist = new Array(N).fill(Infinity);
      dist[to] = 0;
      const queue = [to];
      while (queue.length) {
        const h = queue.shift();
        for (const ai of slots[h]) {
          const o = other(arcList[ai], h);
          if (dist[o] === Infinity) { dist[o] = dist[h] + 1; queue.push(o); }
        }
      }
      const good = slots[from].map((ai, s) => ({ ai, s, d: dist[other(arcList[ai], from)] })).filter((x) => x.d < dist[from]);
      good.sort((a, b) => a.d - b.d || a.s - b.s);
      let w = 0;
      if (good.length === 1) w = (31 << 3) | good[0].s;
      else if (good.length > 1) w = ((20 << 3) | good[0].s) + (((11 << 3) | good[1].s) << 8);
      for (let t = 0; t < 5; t++) words[(from * 5 + t) * N + to] = w >>> 0;
    }
  }
  const barrier = (index, cx, cz, hx, hz, flags, angle) => {
    const c = Math.cos(angle), s = Math.sin(angle);
    return {
      name: `b${index}`, flags,
      xfrm: [f32(c), 0, f32(-s), 0, 1, 0, f32(-s), 0, f32(-c), cx, 0, -cz],
      size: [hx, 0, hz], properties: [],
    };
  };
  const barriers = [];
  for (let i = 0; i < 14; i++) {
    barriers.push(barrier(i, gen.range(-55, 55), gen.range(-40, 40), gen.range(1, 6), gen.range(1, 6), i % 5 === 4 ? 2 : 63, gen.range(0, 3)));
  }
  const hintKinds = ["snipe", "patrol", "cover", "jetjump", "minefield", "land", "jetjump", "cover"];
  const hints = hintKinds.map((kind, i) => ({
    kind, name: `hintnode${i}`,
    xfrm: [1, 0, 0, 0, 1, 0, 0, 0, 1, gen.range(-50, 50), 0, gen.range(-35, 35)],
    properties: [
      ["bf0f1cd1", "PrimaryStance", String(1 << (i % 3))],
      ["92c05a59", "SecondaryStance", String(3 | (8 << (i % 2)))],
      ["ec6ee012", "Mode", i % 4 === 3 ? "3" : String(i % 2)],
      ...(i % 3 === 0 ? [["0dba4cb3", "Radius", (2 + i).toFixed(1)]] : []),
    ],
  }));
  const json = {
    schema: 1, map: "synthetic", source: "nav-twin",
    graph: {
      info: [N, arcList.length, 5],
      hubs: hubs.map((h, i) => ({ name: `Hub${i}`, position: h.p, radius: h.r, arcs: [...slots[i], 255, 255, 255, 255, 255, 255, 255, 255].slice(0, 8) })),
      arcs: arcList.map((a, i) => ({ name: `Connection${i}`, hubs: a.hubs, mask: a.mask })),
      routes: { file: "ai-nav.bin", byteOffset: 0, count: words.length, tables: 5, order: "hub,table,destination" },
    },
    barriers, hints,
    boundary: { configs: [], references: [{ path: 0 }] },
    paths: [{ name: "bnd", info: [4, 0], points: [[-60, 0, -45], [60, 0, -45], [60, 0, 45], [-60, 0, 45]] }],
    regions: [], warnings: [],
  };
  const bin = Buffer.from(words.buffer, words.byteOffset, words.byteLength);
  return { json, bin, box: [-60, -45, 60, 45] };
}

// ---------------------------------------------------------------- setup
const gen = new ClassicRandom(seed ^ 0x5eed);
let navJson, navBin, box;
if (mapDir) {
  navJson = JSON.parse(fs.readFileSync(path.join(mapDir, "ai-nav.json"), "utf8"));
  navBin = fs.readFileSync(path.join(mapDir, "ai-nav.bin"));
  const pts = [];
  for (const h of navJson.graph?.hubs ?? []) pts.push(h.position);
  for (const b of navJson.barriers ?? []) pts.push([b.xfrm[9], b.xfrm[10], -b.xfrm[11]]);
  const xs = pts.map((p) => p[0]), zs = pts.map((p) => p[2]);
  box = [Math.min(...xs), Math.min(...zs), Math.max(...xs), Math.max(...zs)];
  fs.copyFileSync(path.join(mapDir, "ai-nav.json"), path.join(outDir, "ai-nav.json"));
  fs.copyFileSync(path.join(mapDir, "ai-nav.bin"), path.join(outDir, "ai-nav.bin"));
} else {
  const made = syntheticNav(gen);
  navJson = made.json; navBin = made.bin; box = made.box;
  fs.writeFileSync(path.join(outDir, "ai-nav.json"), JSON.stringify(navJson));
  fs.writeFileSync(path.join(outDir, "ai-nav.bin"), navBin);
}
const [bx0, bz0, bx1, bz1] = box;
const rp = (y = 0) => [gen.range(bx0, bx1), y, gen.range(bz0, bz1)];
const hubPts = (navJson.graph?.hubs ?? []).map((h) => h.position);
const nearHub = () => {
  const h = hubPts[gen.int(0, hubPts.length - 1)] ?? [0, 0, 0];
  return [h[0] + gen.range(-4, 4), h[1], h[2] + gen.range(-4, 4)];
};

const units = [
  unitDef(1, KIND.soldier, nearHub()),
  unitDef(2, KIND.soldier, nearHub(), { soldier: { aimYaw: -1.1, jet: { minBorder: 0.2, recharge: 0.5 } }, classDef: { JetFuelRechargeRate: 0.5 } }),
  unitDef(3, KIND.droideka, nearHub()),
  unitDef(4, KIND.hover, nearHub()),
  unitDef(5, KIND.flyer, nearHub()),
  unitDef(6, KIND.walker, nearHub()),
  unitDef(7, KIND.droid, nearHub(), { forward: null }),
];

// ------------------------------------------------------------------ ops
const ops = [];
const op = (o) => { ops.push(o); return o; };
op({ op: "seed", a: seed * 7 + 1, b: seed * 13 + 5, time: 10, far: 3 });
op({ op: "hintsDump" });
if (navJson.graph) op({ op: "graphDump" });
for (let i = 0; i < 40; i++) op({ op: ["rngNext", "rngUniform", "rngInt", "rngRange", "rngMod", "rngNextB"][i % 6], lo: -3 + (i % 5), hi: 4 + (i % 9), n: 1 + (i % 7) });
const NB = navJson.barriers.length;
for (let i = 0; i < 120; i++) {
  const kind = ["pointClear", "segmentBlocked", "nearest", "obstruct", "correct", "detour", "inside", "ejection"][i % 8];
  op({ op: kind, p: rp(), s: rp(), e: rp(), q: rp(), r: [0, 0.476, 1.5, 4][i % 4], mask: [1, 2, 63, 32][i % 4], barrier: i % Math.max(1, NB), i: i % 4 });
}
if (navJson.graph) {
  const N = navJson.graph.hubs.length, A = navJson.graph.arcs.length;
  for (let i = 0; i < 80; i++) op({ op: "attach", p: i % 2 ? rp() : nearHub(), mask: [1, 2, 63, 32][i % 4] });
  for (let i = 0; i < 80; i++) op({ op: "select", prev: i % 3 === 0 ? null : gen.int(0, A - 1), from: gen.int(0, N - 1), to: gen.int(0, N - 1), mask: [1, 2, 4, 32][i % 4] });
  for (let i = 0; i < 30; i++) op({ op: "nearestArc", p: rp(), mask: 63 });
  for (let i = 0; i < 30; i++) op({ op: "corridorGap", arc: gen.int(0, A - 1), p: rp() });
  for (let c = 0; c < 12; c++) {
    op({ op: "cursorInit", id: `c${c}`, start: nearHub(), end: nearHub(), mask: [1, 63][c % 2], clearance: c % 3 === 0 ? 4 : 0 });
    for (let a = 0; a < 8; a++) op({ op: "cursorAdvance", id: `c${c}` });
    op({ op: "cursorRetry", id: `c${c}`, radius: 0.476 });
  }
}
// Planner: interleaved requests, services, consume, walk-along updates.
for (let round = 0; round < 26; round++) {
  const u = units[round % units.length];
  const from = u.position;
  // Local legs (under ~30 m) so searches finish; every fourth round is
  // a long one across the map.
  const to = round % 4 === 3 ? nearHub() : [from[0] + gen.range(-25, 25), from[1], from[2] + gen.range(-25, 25)];
  op({ op: "request", unit: u.id, start: from, end: to, radius: 0.476, mask: 1, priority: round % 3, ...(round % 5 === 4 ? { segments: 12 } : {}) });
  op({ op: "service", budget: 1, times: 400 });
  if (round % 4 === 3) op({ op: "cancel", unit: u.id });
  op({ op: "consume", unit: u.id, mode: round % 2 });
  // Walk the unit toward `to` in fixed steps, running executor ticks.
  for (let step = 1; step <= 10; step++) {
    const t = step / 10;
    op({
      op: "setUnit", unit: u.id,
      position: [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t, from[2] + (to[2] - from[2]) * t],
      velocity: [(to[0] - from[0]) * 0.06, 0, (to[2] - from[2]) * 0.06],
      avoidance: step % 5 === 2 ? [0.2, 0, -0.1] : [0, 0, 0],
    });
    op({ op: "update", unit: u.id, dt: 1 / 60 });
    if (step === 5) op({ op: "service", budget: 1, times: 4 });
  }
  u.position = to;
}
// Explicit installs (hover/flyer/walker steering paths).
for (const u of units) {
  const base = u.position;
  const points = [0, 1, 2, 3].map((i) => [base[0] + i * 7, base[1], base[2] + i * 3]);
  op({ op: "install", unit: u.id, points });
  for (let step = 1; step <= 12; step++) {
    const t = step / 12;
    op({ op: "setUnit", unit: u.id, position: [base[0] + 21 * t, base[1], base[2] + 9 * t], velocity: [3, 0, 1], avoidance: step % 4 === 1 ? [-0.3, 0, 0.2] : [0, 0, 0] });
    op({ op: "update", unit: u.id, dt: 1 / 60 });
  }
  op({ op: "navInput", unit: u.id, dt: 1 / 60, aim: rp(1.5), facing: u.id % 2 === 0 });
  op({ op: "navInput", unit: u.id, dt: 1 / 60, aim: null, facing: false });
}
// Hints and boundary.
for (let i = 0; i < 40; i++) op({ op: "queryHints", mask: [1, 2, 4, 8, 16, 32, 63][i % 7], center: rp(), half: [5, 20, 60][i % 3], cap: [1, 5, 30][i % 3], owner: i % 2 ? 1 : null, now: 10 + i * 0.3 });
for (let i = 0; i < 10; i++) op({ op: "pickStance", mask: [0, 1, 2, 3, 5, 7][i % 6] });
for (let i = 0; i < 8; i++) op({ op: "pickSecondary", choices: i % 4 });
for (let i = 0; i < 20; i++) op({ op: "outside", p: rp() });
if ((navJson.hints ?? []).length) {
  op({ op: "jetFind", unit: 2 });
  op({ op: "markHint", hint: 0, now: 12 });
  op({ op: "queryHints", mask: 63, center: [0, 0, 0], half: 500, cap: 400, owner: 1, now: 13 });
}
// Goto: route/direct/straight with services; follow a moving leader.
op({ op: "clearAbove", answers: Array(64).fill(true) });
for (const [unit, mode] of [[1, "route"], [2, "direct"], [3, "route"], [4, "direct"], [6, "straight"], [1, "direct"]]) {
  const target = nearHub();
  op({ op: "time", time: 20 });
  op({ op: "goto", unit, point: target, mode });
  for (let t = 0; t < 24; t++) {
    op({ op: "service", budget: 1, times: 2 });
    op({ op: "gotoTick", unit, dt: 1 / 60 });
    op({ op: "update", unit, dt: 1 / 60 });
    const u = units[unit - 1];
    const p = u.position;
    u.position = [p[0] + (target[0] - p[0]) * 0.08, p[1], p[2] + (target[2] - p[2]) * 0.08];
    op({ op: "setUnit", unit, position: u.position });
    op({ op: "time", time: 20 + (t + 1) / 60 });
  }
}
op({ op: "follow", unit: 1, target: 7, distance: 2, remembered: false });
for (let t = 0; t < 30; t++) {
  const leader = units[6];
  leader.position = [leader.position[0] + 0.7, leader.position[1], leader.position[2] + 0.2];
  op({ op: "setUnit", unit: 7, position: leader.position, velocity: [2, 0, 0.5] });
  op({ op: "time", time: 30 + t / 10 });
  op({ op: "gotoTick", unit: 1, dt: 0.1 });
  op({ op: "update", unit: 1, dt: 0.1 });
}
op({ op: "setLeader", unit: 1, leader: 7 });
op({ op: "recordTrail", unit: 1, on: true });
for (let t = 0; t < 16; t++) {
  const leader = units[6];
  leader.position = [leader.position[0] + 0.4, leader.position[1], leader.position[2] - 0.3];
  op({ op: "setUnit", unit: 7, position: leader.position });
  op({ op: "update", unit: 1, dt: 0.09 });
}
op({ op: "recordTrail", unit: 1, on: false });
for (let t = 0; t < 8; t++) op({ op: "update", unit: 1, dt: 0.1 });
op({ op: "flyGoto", unit: 5, point: nearHub(), mode: "route" });
op({ op: "service", budget: 1, times: 6 });
op({ op: "flyTick", unit: 5 });
op({ op: "flyGoto", unit: 5, point: nearHub(), mode: "direct" });
op({ op: "flyTick", unit: 5 });
op({ op: "update", unit: 5, dt: 1 / 60 });

fs.writeFileSync(path.join(outDir, "ops.json"), JSON.stringify({ units, ops }));

// ------------------------------------------------------------ JS runner
const dv = new DataView(new ArrayBuffer(8));
const bits = (x) => { dv.setFloat64(0, x); return `"x${dv.getBigUint64(0).toString(16).padStart(16, "0")}"`; };
const v3s = (v) => `[${bits(v.x)},${bits(v.y)},${bits(v.z)}]`;
const optIndex = (v) => (v === null || v === undefined ? "null" : String(v.index ?? v));

const nav = new ClassicNav(navJson, navBin, {});
const jsUnits = new Map();
function makeUnit(def) {
  const u = {
    id: def.id, kind: def.kind, generation: def.generation, alive: def.alive,
    position: new THREE.Vector3(...def.position), velocity: new THREE.Vector3(...def.velocity),
    classDef: def.classDef,
    soldier: {
      ...def.soldier,
      avoidance: def.soldier.avoidance ? new THREE.Vector3(...def.soldier.avoidance) : null,
    },
  };
  if (def.forward) { const f = def.forward; u.forward = (out) => out.set(f[0], f[1], f[2]); }
  if (def.origin) { const o = def.origin; u.origin = (out) => out.set(o[0], o[1], o[2]); }
  jsUnits.set(def.id, u);
  return u;
}
for (const def of units) makeUnit(def);

const world = {
  nav, time: 0,
  random: new ClassicRandom(0), gameRandom: new ClassicRandom(0), randomB: null,
  losAnswers: [],
  los() { return this.losAnswers.length ? this.losAnswers.shift() : true; },
  remembered: new Map(),
};
world.randomB = world.gameRandom;
const resolveRef = (ref) => {
  if (!ref) return null;
  const u = ref.unit;
  return u && u.alive !== false && (ref.generation === undefined || u.generation === ref.generation) ? u : null;
};
world.units = { resolve: resolveRef };

const controllers = new Map();
function controllerOf(unit) {
  let c = controllers.get(unit.id);
  if (!c) {
    c = { unit, world, ai: { followRadii: { far: world.far ?? 3 } }, navControl: null, aimState: 2, aimPoint: new THREE.Vector3(), aimTarget: null };
    c.lookAt = function (point) { this.aimState = 1; this.aimPoint.copy(point); };
    c.aimAt = function (ref) { this.aimState = 0; this.aimTarget = ref ?? null; };
    c.lookAlongPath = function () { this.aimState = 2; };
    controllers.set(unit.id, c);
    nav.control(c);
  }
  return c;
}
const controlOf = (unit) => {
  const control = controllerOf(unit).navControl;
  control.attach(unit);
  return control;
};
const grounds = new Map(), flys = new Map();
function groundOf(unit) {
  let h = grounds.get(unit.id);
  if (!h) { h = new GroundGotoHelper(controllerOf(unit), world, { remembered: (c, target) => world.remembered.get(target.id) ?? null }); grounds.set(unit.id, h); }
  return h;
}
function flyOf(unit) {
  let h = flys.get(unit.id);
  if (!h) { h = new FlyGotoHelper(controllerOf(unit), world, {}); flys.set(unit.id, h); }
  return h;
}

const vecOf = (a) => (a ? { x: a[0], y: a[1], z: a[2] } : null);
function controlLine(control) {
  const published = control.published
    ? `[${control.published.nodes.map((n) => `[${v3s(n)},${n.flag}]`).join(",")}]`
    : "null";
  const e = control.executor;
  const executor = e
    ? `[${e.index},${v3s(e.waypoint)},${v3s(e.prev)},${v3s(e.direction)},${bits(e.mode)},${bits(e.status)},${e.complete},${e.path.nodes.length}]`
    : "null";
  const r = control.request;
  return `[${bits(control.serial)},${control.segments},${control.ready},${published},${executor},[${r.valid},${v3s(r.steeringTarget)},${v3s(r.waypoint)},${v3s(r.segmentDirection)},${bits(r.mode)},${bits(r.lower)}],${bits(control.strandedFor)}]`;
}
const cursorLine = (c) => `[${v3s(c.waypoint)},${optIndex(c.current)},${optIndex(c.primary)},${optIndex(c.secondary)},${optIndex(c.previous)},${world.random.seed}]`;
const gotoLine = (h) => `[${bits(h.state)},${v3s(h.destination)},${h.failed},${h.arrived},${bits(h.jetDeadline)},${controlLine(h.control)}]`;

const trace = [];
const runStart = performance.now();
const cursors = new Map();
ops.forEach((op, index) => {
  const unit = op.unit !== undefined ? jsUnits.get(op.unit) : null;
  let line;
  switch (op.op) {
    case "seed":
      world.random = new ClassicRandom(op.a); world.gameRandom = new ClassicRandom(op.b); world.randomB = world.gameRandom;
      world.time = op.time; world.far = op.far ?? 3;
      for (const c of controllers.values()) c.ai.followRadii.far = world.far;
      line = `[${world.random.seed},${world.gameRandom.seed}]`; break;
    case "time": world.time = op.time; line = bits(world.time); break;
    case "rngNext": line = String(world.random.next()); break;
    case "rngUniform": line = bits(world.random.uniform()); break;
    case "rngInt": line = bits(world.random.int(op.lo, op.hi)); break;
    case "rngRange": line = bits(world.random.range(op.lo, op.hi)); break;
    case "rngMod": line = bits(world.random.mod(op.n)); break;
    case "rngNextB": line = String(world.gameRandom.next()); break;
    case "setUnit": {
      if (op.position) unit.position.set(...op.position);
      if (op.velocity) unit.velocity.set(...op.velocity);
      if (op.avoidance !== undefined && unit.soldier) unit.soldier.avoidance = op.avoidance ? new THREE.Vector3(...op.avoidance) : null;
      if (op.fuel !== undefined && unit.soldier) unit.soldier.fuel = op.fuel;
      line = v3s(unit.position); break;
    }
    case "pointClear": {
      const out = {};
      const clear = nav.barriers.pointClear(vecOf(op.p), op.r, op.mask, out);
      line = `[${clear},${out.barrier ? out.barrier.index : "null"}]`; break;
    }
    case "segmentBlocked": line = String(nav.barriers.segmentBlocked(vecOf(op.s), vecOf(op.e), op.r, op.mask)); break;
    case "nearest": {
      const out = {};
      const any = nav.barriers.nearest(vecOf(op.s), vecOf(op.e), op.r, op.mask, out);
      line = `[${any},${out.barrier ? out.barrier.index : "null"},${bits(out.distanceSq)}]`; break;
    }
    case "obstruct": {
      const e = vecOf(op.e), normal = { x: 0, y: 0, z: 0 };
      const any = nav.barriers.obstruct(vecOf(op.s), e, op.r, op.mask, normal);
      line = `[${any},${v3s(e)},${v3s(normal)}]`; break;
    }
    case "correct": {
      const p = vecOf(op.p);
      const ok = nav.barriers.correct(p, op.r, op.mask);
      line = `[${ok},${v3s(p)}]`; break;
    }
    case "detour": {
      const l = { x: 0, y: 0, z: 0 }, r = { x: 0, y: 0, z: 0 };
      const found = detour(nav.barriers.list[op.barrier], vecOf(op.p), vecOf(op.q), op.r, l, r);
      line = `[${found},${v3s(l)},${v3s(r)}]`; break;
    }
    case "inside": line = String(inside(nav.barriers.list[op.barrier], op.p[0], op.p[1], op.p[2], op.r)); break;
    case "ejection": {
      const out = { x: 0, y: 0, z: 0 };
      const score = ejection(nav.barriers.list[op.barrier], vecOf(op.p), op.r, op.i, out);
      line = `[${bits(score)},${v3s(out)}]`; break;
    }
    case "attach": {
      const a = nav.graph.attach(vecOf(op.p), op.mask);
      line = `[${a.type},${optIndex(a.hub)},${optIndex(a.arc)}]`; break;
    }
    case "select": {
      const prev = op.prev === null ? null : nav.graph.arcs[op.prev];
      const picked = nav.graph.select(prev, nav.graph.hubs[op.from], nav.graph.hubs[op.to], op.mask, world.random);
      line = `[${optIndex(picked)},${world.random.seed}]`; break;
    }
    case "nearestArc": line = optIndex(nav.graph.nearestArc(vecOf(op.p), op.mask)); break;
    case "corridorGap": line = bits(corridorGap(nav.graph.arcs[op.arc], vecOf(op.p))); break;
    case "cursorInit": {
      const cursor = new GraphCursor(nav.graph, nav.barriers);
      const ok = cursor.init(vecOf(op.start), vecOf(op.end), op.mask, op.clearance, world.random);
      cursors.set(op.id, cursor);
      line = `[${ok},${cursorLine(cursor)}]`; break;
    }
    case "cursorAdvance": {
      const cursor = cursors.get(op.id);
      const ok = cursor.advance(world.random);
      line = `[${ok},${cursorLine(cursor)}]`; break;
    }
    case "cursorRetry": {
      const cursor = cursors.get(op.id);
      const made = cursor.retry(op.radius, world.random);
      line = `[${made},${cursorLine(cursor)}]`; break;
    }
    case "request": {
      const control = controlOf(unit);
      if (op.segments !== undefined && op.segments !== null) control.segments = op.segments;
      const ok = nav.planner.request(control, vecOf(op.start), vecOf(op.end), op.radius, op.mask, op.priority);
      line = `[${ok},${controlLine(control)}]`; break;
    }
    case "service": {
      for (let i = 0; i < (op.times ?? 1); i++) nav.service(op.budget);
      line = `[${nav.planner.idle},${bits(nav.planner.served)},${bits(nav.planner.pools.paths.used)},${bits(nav.planner.pools.nodes.used)}]`; break;
    }
    case "cancel": nav.planner.cancel(controlOf(unit)); line = controlLine(controlOf(unit)); break;
    case "consume": {
      const answer = controlOf(unit).consume(op.mode);
      // The Rust executor writes its zeroed contact back into its unit
      // snapshot; the JS executor writes the same object directly.
      line = `[${answer.ready},${answer.success},${controlLine(controlOf(unit))}]`; break;
    }
    case "install": {
      const p = nav.planner.path();
      for (const pt of op.points) p.append(vecOf(pt));
      controlOf(unit).install(p, true, false);
      line = controlLine(controlOf(unit)); break;
    }
    case "update": controlOf(unit).update(op.dt); line = controlLine(controlOf(unit)); break;
    case "navInput": {
      const intent = {};
      navigationInput(controlOf(unit), unit, intent, op.dt, op.aim ? vecOf(op.aim) : null, { facing: op.facing });
      const jet = intent.jetDirection === undefined ? "null" : intent.jetDirection === null ? '"null"' : v3s(intent.jetDirection);
      line = `[${bits(intent.forward ?? 0)},${bits(intent.right ?? 0)},${bits(intent.turn ?? 0)},${bits(intent.pitch ?? 0)},${Boolean(intent.jump)},${Boolean(intent.crouch)},${Boolean(intent.prone)},${jet}]`; break;
    }
    case "setLeader": {
      const ref = op.leader ? { unit: jsUnits.get(op.leader), generation: 1 } : null;
      controlOf(unit).setLeader(ref);
      line = controlLine(controlOf(unit)); break;
    }
    case "recordTrail": controlOf(unit).recordTrail(op.on); line = controlLine(controlOf(unit)); break;
    case "goto": groundOf(unit).goto(new THREE.Vector3(...op.point), { mode: op.mode }); line = gotoLine(groundOf(unit)); break;
    case "gotoTick": groundOf(unit).tick(op.dt); line = gotoLine(groundOf(unit)); break;
    case "gotoStop": groundOf(unit).stop(); line = gotoLine(groundOf(unit)); break;
    case "follow": {
      const target = op.target ? { unit: jsUnits.get(op.target), generation: 1 } : null;
      groundOf(unit).follow(target, op.distance, op.remembered);
      line = gotoLine(groundOf(unit)); break;
    }
    case "remember": world.remembered.set(op.target, vecOf(op.point)); line = "true"; break;
    case "clearAbove": world.losAnswers = [...op.answers]; line = String(op.answers.length); break;
    case "flyGoto": flyOf(unit).goto(new THREE.Vector3(...op.point), { mode: op.mode }); line = `[${bits(flyOf(unit).state)},${v3s(flyOf(unit).destination)},${flyOf(unit).failed},${controlLine(controlOf(unit))}]`; break;
    case "flyTick": flyOf(unit).tick(); line = `[${bits(flyOf(unit).state)},${flyOf(unit).arrived},${controlLine(controlOf(unit))}]`; break;
    case "queryHints": {
      const found = nav.queryHints({ mask: op.mask, center: vecOf(op.center), halfExtent: op.half, cap: op.cap ?? Infinity, requester: op.owner ? jsUnits.get(1) : null, now: op.now });
      line = `[${found.map((h) => h.index).join(",")}]`; break;
    }
    case "markHint": {
      const hint = nav.hints.list[op.hint];
      nav.markHint(hint, jsUnits.get(1), op.now);
      line = bits(hint.reservedAt); break;
    }
    case "pickStance": line = bits(pickStance(op.mask, world.random)); break;
    case "pickSecondary": line = bits(pickSecondary(op.choices, world.random)); break;
    case "jetFind": {
      const controller = { unit, world: { time: world.time }, ai: { randomA: world.random } };
      const found = nav.jetHints.find(controller);
      line = `[${found ? found.index : "null"},${world.random.seed}]`; break;
    }
    case "outside": line = String(nav.outside(vecOf(op.p))); break;
    case "hintsDump": {
      line = `[${nav.hints.list.map((h) => `[${h.index},${h.source},${h.copy},${h.bit},${v3s(h.position)},${v3s(h.forward)},${v3s(h.right)},${h.primaryStance},${h.secondaryStance},${h.secondaryChoices},${bits(h.mode)},${bits(h.radius)},${h.link ? h.link.index : "null"}]`).join(",")}]`; break;
    }
    case "graphDump": line = `[${nav.graph.hubs.map((h) => `[[${bits(h.x)},${bits(h.y)},${bits(h.z)}],${bits(h.radius)},${h.mask}]`).join(",")}]`; break;
    default: line = '"unknown"';
  }
  trace.push(`[${index},"${op.op}",${line}]`);
});
fs.writeFileSync(path.join(outDir, "js-trace.txt"), trace.join("\n") + "\n");
console.log(`nav-twin: ${ops.length} ops, js trace in ${outDir}, runner ${(performance.now() - runStart).toFixed(1)} ms`);
