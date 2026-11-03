// The rounds on the simulation's module against the JavaScript rounds
// (src/ordnance-sim.js, src/ordnance.js): two Ordnances with the same
// fakes, one handed to an instance of the module, fire the same rounds and
// fly them side by side; after every tick each list's rounds must be the
// same, field by field (the schema's comparisons, Object.is on every
// number), and every call they made into the battle the same, in order.
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { Ordnance } from "../src/ordnance.js";
import { loadSim } from "../src/sim/module.js";
import { SimBridge } from "../src/sim/runtime.js";
import { attachOrdnanceSim, ordnanceTests } from "../src/ordnance-sim.js";
import { KIND_BEAM, KIND_BOLT, KIND_GRENADE, KIND_ROUND, KIND_TABLE } from "../src/sim/gen/index.js";

const LISTS = [["bolts", KIND_BOLT], ["beams", KIND_BEAM], ["grenades", KIND_GRENADE], ["rounds", KIND_ROUND]];

class FakeUnit {
  constructor(id, x, y, z, radius, droid = false) {
    this.id = id;
    this.generation = 1;
    this.alive = true;
    this.team = 2;
    this.hitShapes = true;
    this.seat = null;
    this.position = new THREE.Vector3(x, y, z);
    this.velocity = new THREE.Vector3(0, 0, 1);
    this.radius = radius;
    this.classDef = { HealthType: droid ? "droid" : "person" };
  }
  get sphere() { return { center: this.position, radius: this.radius }; }
  aimPoint(out) { return out.copy(this.position); }
}

// A field: the ground at y 0 and a building's wall at x 30.
function battle() {
  const wall = { name: "wall" };
  const units = [new FakeUnit(1, 0, 1, 40, 1), new FakeUnit(2, -6, 1, 25, 1.2, true), new FakeUnit(3, 6, 1, 18, 1)];
  const collision = {
    cast(origin, dir, far, out, ignoreBody) {
      let best = null;
      if (dir.y < 0) { const t = -origin.y / dir.y; if (t >= 0 && t <= far) best = { t, n: [0, 1, 0], terrain: true, body: null }; }
      if (dir.x > 0 && wall !== ignoreBody) { const t = (30 - origin.x) / dir.x; if (t >= 0 && t <= far && (!best || t < best.t)) best = { t, n: [-1, 0, 0], terrain: false, body: wall }; }
      out.body = null;
      if (!best) return null;
      out.distance = best.t;
      out.point.copy(origin).addScaledVector(dir, best.t);
      out.normal.set(...best.n);
      out.terrain = best.terrain;
      out.body = best.body;
      return out;
    },
    castMoving: () => null,
  };
  const registry = {
    byId: (id) => units.find((u) => u.id === id),
    raycast(origin, direction, far, shooter, out, filter) {
      let best = null;
      for (const u of units) {
        if (u === shooter || (filter && !filter(u))) continue;
        const oc = origin.clone().sub(u.position), b = oc.dot(direction), c = oc.lengthSq() - u.radius * u.radius, disc = b * b - c;
        if (disc < 0) continue;
        const t = -b - Math.sqrt(disc);
        if (t < 0 || t > far || (best && t >= best.t)) continue;
        best = { u, t };
      }
      if (!best) return null;
      out.unit = best.u;
      out.distance = best.t;
      out.point.copy(origin).addScaledVector(direction, best.t);
      out.normal.copy(out.point).sub(best.u.position).normalize();
      out.part = out.point.y > best.u.position.y + 0.5 ? "head" : "body";
      return out;
    },
    query(center, radius, out, filter) {
      out.length = 0;
      for (const u of units) if (u.position.distanceTo(center) <= radius + u.radius && filter(u)) out.push(u);
      return out;
    },
  };
  // A lag-compensated shot: the player's shots meet the others three ticks back.
  const hitHistory = { tick: 8, shotTick: (owner) => (owner === "player" ? 5 : null), raycast: (tick, ...rest) => registry.raycast(...rest) };
  return { wall, units, collision, registry, hitHistory };
}

const explosion = { class: "boom", Damage: 50, DamageRadiusInner: 1, DamageRadiusOuter: 5, DroidScale: 0.5 };
const DEFS = {
  bolt: { class: "bolt", base: "laser", Velocity: 120, LifeSpan: 1.2, Damage: 20, LaserLength: 2 },
  beam: { class: "beam", base: "beam", LifeSpan: 0.4, Damage: 90 },
  grenade: { class: "grenade", base: "grenade", Velocity: 14, LifeSpan: 1.5, Gravity: 1, ExplosionExpire: explosion },
  sticky: { class: "sticky", base: "sticky", Velocity: 16, LifeSpan: 2, Gravity: 1, StickTerrain: 1, ExplosionExpire: explosion },
  rocket: { class: "rocket", base: "bullet", Velocity: 60, LifeSpan: 2, ExplosionImpact: explosion },
  missile: { class: "missile", base: "missile", Velocity: 30, LifeSpan: 4, TurnRate: 2, ScaleTime: 0.5, WaverTurn: 0.3, WaverRate: 2, ExplosionImpact: explosion },
  shell: { class: "shell", base: "shell", Velocity: 40, LifeSpan: 5, Gravity: 1, ExplosionImpact: explosion, ExplosionExpire: explosion },
  fatray: { class: "fatray", base: "fatray", Velocity: 100, LifeSpan: 0.45, Damage: 30 },
};

// A seeded draw stream (the same numbers on both sides).
function stream(seed) {
  let s = seed >>> 0;
  return () => { s = (Math.imul(s, 1664525) + 1013904223) >>> 0; return s / 4294967296; };
}

const name = (x) => (x === null || x === undefined ? x : typeof x === "string" ? x : x.id ?? x.name ?? x.class ?? "?");
const v3 = (v) => (v ? [v.x, v.y, v.z] : v);

function ordnance(world, log) {
  const o = new Ordnance({ scene: new THREE.Scene(), collision: world.collision, effects: { play: () => null }, lights: { add() {}, flash() {}, update() {} }, texture: () => null, model: () => null,
    onExplode: (def, position, owner, context) => log.push(["explode", def.class, v3(position), name(owner), context.wet]),
    onHit: (body, def, owner, direction) => log.push(["hit", name(body), def.class, def.Damage, name(owner), v3(direction)]) });
  o.units = world.registry;
  o.hitHistory = world.hitHistory;
  o.random = stream(7);
  o.spinRandom = stream(11);
  o.onHitUnit = (unit, def, point, owner, part, direction) => log.push(["hitUnit", unit.id, def.class, def.Damage, v3(point), name(owner), part, v3(direction)]);
  o.onProjectile = (p, type) => log.push(["projectile", type, v3(p.position), v3(p.velocity)]);
  o.onLaunch = (bolt, context) => log.push(["launch", v3(bolt.position), v3(bolt.direction), bolt.life, context.tick]);
  o.onImpact = (round, hit) => log.push(["impact", v3(hit.point), v3(hit.normal), hit.surface]);
  o.onGrenadeContact = (g) => log.push(["contact", v3(g.position)]);
  return o;
}

async function pair() {
  const world = battle(), jsLog = [], rustLog = [];
  const js = ordnance(world, jsLog), rust = ordnance(world, rustLog);
  // The battle tells its units apart by class (game.js gives Unit and Vehicle).
  attachOrdnanceSim(rust, new SimBridge(await loadSim(), { tests: { ...ordnanceTests, unit: (o) => o instanceof FakeUnit } }));
  return { world, js, rust, jsLog, rustLog };
}

function same(js, rust, when) {
  for (const [list, kind] of LISTS) {
    assert.equal(rust[list].length, js[list].length, `${when}: ${list}`);
    js[list].forEach((x, i) => assert.equal(KIND_TABLE[kind].same(x, rust[list][i]), null, `${when}: ${list}[${i}]`));
  }
  assert.deepEqual(v3(rust.start), v3(js.start), `${when}: start`);
}

function fire(o, kind, origin, direction, owner = "player", options = {}) {
  return o.fire(DEFS[kind], new THREE.Vector3(...origin), new THREE.Vector3(...direction).normalize(), owner, options.visual ?? null, null, null,
    options.guidance ?? { weapon: { channel: 0 } });
}

test("rounds on the module fly as the JavaScript rounds do, tick by tick", async () => {
  const { world, js, rust, jsLog, rustLog } = await pair();
  const shots = [
    ["bolt", [0, 1.5, 0], [0, 0, 1]],
    ["bolt", [0, 1.5, 0], [0.3, -0.05, 1], world.units[2]],
    ["bolt", [2, 1.4, 0], [1, 0, 0.1]],
    ["beam", [-2, 1.5, 0], [-0.24, 0, 1]],
    ["grenade", [0, 1.5, 0], [0.2, 0.6, 1]],
    ["sticky", [1, 1.5, 0], [0.1, 0.4, 1]],
    ["rocket", [0, 1.5, 0], [0.05, -0.02, 1]],
    ["rocket", [0, 2, 0], [1, 0.01, 0.2]],
    ["missile", [0, 3, 0], [0.4, 0.2, 1], world.units[2], { guidance: { weapon: { channel: 1 }, targetId: 1, targetGeneration: 1 } }],
    ["shell", [0, 2, 0], [0, 0.5, 1]],
    ["fatray", [-6, 1, 0], [0, 0, 1]],
  ];
  for (let tick = 0; tick < 240; tick++) {
    if (tick % 20 === 0 && tick / 20 < shots.length) {
      const [kind, origin, direction, owner, options] = shots[tick / 20];
      const a = fire(js, kind, origin, direction, owner, options), b = fire(rust, kind, origin, direction, owner, options);
      assert.equal(typeof b, typeof a, `tick ${tick}: what ${kind} gives`);
      if (a) assert.equal(KIND_TABLE[a.speed !== undefined ? KIND_BOLT : KIND_ROUND].same(a, b), null, `tick ${tick}: the ${kind} fired`);
      same(js, rust, `tick ${tick} after the ${kind}`);
    }
    js.update(1 / 60, false);
    rust.update(1 / 60, false);
    same(js, rust, `tick ${tick}`);
    assert.deepEqual(rustLog, jsLog, `tick ${tick}: the calls into the battle`);
  }
  const kinds = new Set(jsLog.map((c) => c[0]));
  for (const k of ["explode", "hit", "hitUnit", "projectile", "launch", "impact", "contact"]) assert.ok(kinds.has(k), `the scenario makes a ${k}`);
});

test("a flyer's missile and a bolt ended by its catch-up keep their mirrors", async () => {
  const { js, rust, jsLog, rustLog } = await pair();
  const a = fire(js, "missile", [0, 3, 0], [0, 0, 1], "player", { guidance: { weapon: { channel: 0 }, targetId: 1, targetGeneration: 1 } });
  const b = fire(rust, "missile", [0, 3, 0], [0, 0, 1], "player", { guidance: { weapon: { channel: 0 }, targetId: 1, targetGeneration: 1 } });
  js.flightMissile(a, "fighter", 3, 45);
  rust.flightMissile(b, "fighter", 3, 45);
  assert.equal(KIND_TABLE[KIND_ROUND].same(a, b), null);
  // Fired from beside the near unit with three ticks to catch up: it meets him
  // before it is ever listed.
  const c = fire(js, "bolt", [-6, 1, 21], [0, 0, 1]), d = fire(rust, "bolt", [-6, 1, 21], [0, 0, 1]);
  assert.ok(c.life <= 0 && js.bolts.length === 0);
  assert.equal(KIND_TABLE[KIND_BOLT].same(c, d), null);
  assert.equal(rust.bolts.length, 0);
  for (let tick = 0; tick < 120; tick++) {
    js.update(1 / 60, false);
    rust.update(1 / 60, false);
    same(js, rust, `tick ${tick}`);
  }
  assert.deepEqual(rustLog, jsLog);
  // The ended bolt's mirror kept its last state through its retirement.
  assert.equal(KIND_TABLE[KIND_BOLT].same(c, d), null);
});
