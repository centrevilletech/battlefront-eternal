// The items on the simulation's module against the JavaScript items
// (src/items-sim.js, src/items.js): two Items with the same fakes, one
// handed to an instance of the module, get the same power-ups placed, mines
// thrown, drops, blasts and hits, and the same targets walking among them;
// after every tick the list's items must be the same, field by field, the
// dispensers' lists the same, and every call into the battle the same.
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { Items } from "../src/items.js";
import { Ordnance } from "../src/ordnance.js";
import { loadSim } from "../src/sim/module.js";
import { SimBridge } from "../src/sim/runtime.js";
import { attachOrdnanceSim, ordnanceTests } from "../src/ordnance-sim.js";
import { attachItemsSim } from "../src/items-sim.js";
import { KIND_ITEM, KIND_TABLE } from "../src/sim/gen/index.js";

class FakeUnit {
  constructor(id, team) { this.id = id; this.generation = 1; this.team = team; }
}

const explosion = { class: "boom", Damage: 60, DamageRadiusInner: 1, DamageRadiusOuter: 6, DroidScale: 0.5 };
const mine = { class: "mine", base: "mine", GeometryName: "mine", Gravity: 1, LifeSpan: 30, MaxHealth: 40, ExplosionDeath: explosion, ExplosionTrigger: explosion, TriggerRadius: 2, SuppressRadius: 1 };
const bomb = { class: "bomb", base: "mine", GeometryName: "bomb", Gravity: 1, LifeSpan: 1.5, MaxHealth: 10, ExplosionExpire: explosion };
const health = { class: "health", base: "powerupitem", GeometryName: "health", Gravity: 1, Rebound: 0.4, Friction: 0.6, LifeSpan: 20 };
const ammo = { class: "ammo", base: "powerupitem", Gravity: 1, LifeSpan: 20 };

// The ground at y 0; a model a sphere of radius 0.4 a little off its origin.
const collision = {
  cast(origin, dir, far, out) {
    out.body = null;
    if (!(dir.y < 0)) return null;
    const t = -origin.y / dir.y;
    if (t < 0 || t > far) return null;
    out.distance = t;
    out.point.copy(origin).addScaledVector(dir, t);
    out.normal.set(0, 1, 0);
    out.terrain = true;
    return out;
  },
};
const model = (name) => () => {
  const geometry = new THREE.SphereGeometry(name === "bomb" ? 0.25 : 0.4);
  geometry.translate(0, 0.1, 0.05);
  geometry.computeBoundingSphere();
  return new THREE.Mesh(geometry);
};

// Soldiers walking a line through the field: an enemy of team 1 and a
// friend, each taking what they touch.
function targets(log) {
  const make = (name, team, x, z, vz) => ({ name, team, position: new THREE.Vector3(x, 0, z), center: new THREE.Vector3(x, 1, z), radius: 0.5, velocity: new THREE.Vector3(0, 0, vz),
    relation: (other) => (other === 0 ? 0 : other === team ? 1 : -1),
    give(def) { log.push(["give", name, def.class]); return def.class !== "ammo"; } });
  return [make("enemy", 2, 0, -6, 2), make("friend", 1, 3, 8, -1.5)];
}
function walk(list, dt) {
  for (const t of list) { t.position.addScaledVector(t.velocity, dt); t.center.set(t.position.x, 1, t.position.z); }
}

async function pair() {
  const owner = new FakeUnit(5, 1), jsLog = [], rustLog = [];
  const make = (log) => {
    const items = new Items({ scene: new THREE.Scene(), collision, effects: { play: () => null }, lights: { add() {} }, model,
      explode: (def, position, who) => log.push(["explode", def.class, position.toArray(), who?.id ?? who]) });
    items.random = (() => { let s = 3; return () => { s = (Math.imul(s, 1664525) + 1013904223) >>> 0; return s / 4294967296; }; })();
    return items;
  };
  const js = make(jsLog), rust = make(rustLog);
  const bridge = new SimBridge(await loadSim(), { tests: { ...ordnanceTests, unit: (o) => o instanceof FakeUnit } });
  const rounds = attachOrdnanceSim(new Ordnance({ scene: new THREE.Scene(), collision, effects: { play: () => null }, lights: { add() {}, flash() {} }, texture: () => null }), bridge);
  attachItemsSim(rust, rounds);
  return { owner, js, rust, jsLog, rustLog, jsTargets: targets(jsLog), rustTargets: targets(rustLog) };
}

function same(js, rust, when) {
  assert.equal(rust.items.length, js.items.length, `${when}: items`);
  js.items.forEach((x, i) => assert.equal(KIND_TABLE[KIND_ITEM].same(x, rust.items[i]), null, `${when}: items[${i}]`));
  assert.deepEqual([...rust.owned.keys()], [...js.owned.keys()], `${when}: dispensers`);
  for (const [key, list] of js.owned) assert.deepEqual(rust.owned.get(key).map((i) => rust.items.indexOf(i)), list.map((i) => js.items.indexOf(i)), `${when}: ${key}`);
}

test("items on the module lie, fly, go off and are taken as the JavaScript items are, tick by tick", async () => {
  const { owner, js, rust, jsLog, rustLog, jsTargets, rustTargets } = await pair();
  const v = (x, y, z) => new THREE.Vector3(x, y, z);
  for (const items of [js, rust]) {
    items.spawn(health, v(0, 1, 0), v(0, 0, 0), 0.3);
    items.spawn(ammo, v(3, 2, 6), v(0.5, 3, 0), 1.1);
    items.spawn(health, v(3, 0.2, 7), v(0, 0, 0), 0, { team: 1 });
  }
  same(js, rust, "placed");
  for (let tick = 0; tick < 480; tick++) {
    if (tick === 10 || tick === 30 || tick === 50 || tick === 70) {
      const a = js.dispense("minelayer", 3, mine, v(tick / 20, 1, 2), v(0, 2, 1), 0.5, owner, 1);
      const b = rust.dispense("minelayer", 3, mine, v(tick / 20, 1, 2), v(0, 2, 1), 0.5, owner, 1);
      assert.equal(Boolean(b), Boolean(a), `tick ${tick}: a mine thrown`);
    }
    if (tick === 40) for (const items of [js, rust]) items.dispense("timebomb", 0, bomb, v(-2, 1, 1), v(0, 1, 0), 0, owner, 1);
    if (tick === 90) for (const items of [js, rust]) items.dropFor([{ class: "nothing", probability: 0 }, { class: "health", probability: 0.5 }, { class: "ammo", probability: "0.5" }], { health, ammo }, v(1, 1, -2), v(4, 0, 0), 2);
    if (tick === 200) for (const items of [js, rust]) items.blast(explosion, v(3.1, 0.5, 2));
    if (tick === 220) for (const items of [js, rust]) { const m = items.items.find((i) => i.mine && !i.gone); if (m) items.hurt(m, 25); }
    walk(jsTargets, 1 / 60);
    walk(rustTargets, 1 / 60);
    js.update(1 / 60, jsTargets);
    rust.update(1 / 60, rustTargets);
    same(js, rust, `tick ${tick}`);
    assert.deepEqual(rustLog, jsLog, `tick ${tick}: the calls into the battle`);
  }
  const kinds = new Set(jsLog.map((c) => c[0]));
  assert.ok(kinds.has("explode") && kinds.has("give"), "the scenario sets mines off and gives power-ups");
});
