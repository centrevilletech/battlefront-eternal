// Walkers and the droideka in deep water (src/vehicle-water.js,
// docs/vehicles.md "Water"): the original game's clock and per-update damage,
// and a walker that wades in on the bed rather than standing on the
// surface (on the prepared Kashyyyk: Docks pack; skipped without it).
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles } from '../src/vehicles.js';
import { droidekaWaterDepth, waterDamageRule, waterDamageStep } from '../src/vehicle-water.js';
import { packWith } from '../tools/vehicle-packs.mjs';

// Water over x < 0 at `level`.
const pool = (level) => ({ level, over: (x) => x < 0 });

test('the clock runs down from the interval, then every update deals the amount until the hull leaves', () => {
  const rule = waterDamageRule({ WaterDamageInterval: '1.0', WaterDamageAmount: '10.0' });
  assert.deepEqual(rule, { interval: 1, amount: 10 });
  const water = pool(6);
  const state = {};
  const at = new THREE.Vector3(-5, 0, 0);
  let dealt = 0;
  const first = [];
  for (let i = 0; i < 120; i++) {
    const hurt = waterDamageStep(state, 1 / 60, water, rule, at, 5.5);
    if (hurt) first.push(i);
    dealt += hurt;
  }
  // Nothing for the first second, then 10 an update: 600 a second.
  // (The clock's float steps reach zero on the 60th update or the 59th.)
  assert.ok(first[0] >= 59 && first[0] <= 60, `first hit at update ${first[0]}`);
  assert.equal(first.length, 120 - first[0]);
  assert.ok(Math.abs(dealt - first.length * 10) < 1e-6, `dealt ${dealt}`);
  // At 120 steps a second it is the same per second.
  const fine = {};
  let fineDealt = 0;
  for (let i = 0; i < 240; i++) fineDealt += waterDamageStep(fine, 1 / 120, water, rule, at, 5.5);
  assert.ok(Math.abs(fineDealt - dealt) < 10 + 1e-6, `${fineDealt} against ${dealt}`);
  // Out of the water (shallower than the depth, or off the cells) the clock
  // is the interval again.
  assert.equal(waterDamageStep(state, 1 / 60, water, rule, new THREE.Vector3(-5, 1, 0), 5.5), 0);
  assert.equal(state.waterClock, 1);
  assert.equal(waterDamageStep(state, 1 / 60, water, rule, new THREE.Vector3(5, 0, 0), 5.5), 0);
  assert.equal(state.waterClock, 1);
});

test('classes without an amount take none; the droideka hurts deeper standing than rolled', () => {
  assert.equal(waterDamageRule({}), null);
  assert.equal(waterDamageRule({ WaterDamageInterval: 1, WaterDamageAmount: 0 }), null);
  assert.equal(waterDamageStep({}, 1, pool(10), null, new THREE.Vector3(-1, 0, 0), 0), 0);
  const def = { UprightWaterDamageHeight: 1.5, BallWaterDamageHeight: 0.5, WaterDamageInterval: 1, WaterDamageAmount: 10 };
  assert.equal(droidekaWaterDepth(def, false), 1.5);
  assert.equal(droidekaWaterDepth(def, true), 0.5);
  assert.equal(droidekaWaterDepth({}, false), 1.5);
  // One metre of water: a rolled droideka is under, a standing one is not.
  const rule = waterDamageRule(def);
  const rolled = {}, standing = {};
  let r = 0, s = 0;
  for (let i = 0; i < 90; i++) {
    r += waterDamageStep(rolled, 1 / 60, pool(1), rule, new THREE.Vector3(-1, 0, 0), droidekaWaterDepth(def, true));
    s += waterDamageStep(standing, 1 / 60, pool(1), rule, new THREE.Vector3(-1, 0, 0), droidekaWaterDepth(def, false));
  }
  assert.ok(r > 0);
  assert.equal(s, 0);
});

const maps = new URL('../public/maps/', import.meta.url);
const has = (name, map) => Boolean(packWith(maps, [name], map));

// An AT-ST alone on a plane at y 0, with water `level` over x < 0.
function atst(level) {
  const pack = packWith(maps, ['imp_walk_atst'], 'kas2');
  const dir = new URL('./', pack);
  const bin = (file) => { const b = fs.readFileSync(new URL(file, dir)); return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength); };
  const data = JSON.parse(fs.readFileSync(pack));
  const g = new THREE.PlaneGeometry(4000, 4000).rotateX(-Math.PI / 2);
  const collision = new CollisionWorld(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
  const destroyed = [];
  const system = new Vehicles({ scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera(), collision, water: pool(level),
    data: { ...data, blockers: null, spawns: [{ name: 'test', matrix: new THREE.Matrix4().makeTranslation(-20, 0, 0).toArray(), team: 1, post: '', classes: { 1: 'imp_walk_atst' } }] },
    buffer: bin('models.bin'), indexBuffer: bin('models_index.bin'), animationBuffer: bin('animations.bin'),
    texture: () => null, effects: { play() { return null; } }, ordnance: { explosion() {}, fire() { return null; }, lights: { flash() {} } },
    onDestroyed: (v) => destroyed.push(v) });
  const v = system.vehicles[0];
  v.driver = {}; v.seatIndex = 0;
  return { system, v, destroyed };
}

test('an AT-ST wades in on the bed; water over its sphere wears it down, shallower water does not', { skip: !has('imp_walk_atst', 'kas2') }, () => {
  // Shallow: 2 m of water. It stands on the bed and takes nothing.
  let { system, v } = atst(2);
  assert.ok(v.waterRule && v.waterDepth > 5 && v.waterDepth < 6, `depth ${v.waterDepth}`);
  for (let t = 0; t < 300; t++) system.update(1 / 60, v, {});
  assert.ok(Math.abs(v.position.y) < 0.6, `stands on the bed at ${v.position.y}, not the surface`);
  assert.equal(v.health, v.maxHealth);
  system.dispose();
  // Deep: 7 m. After a second it loses 10 an update until it is destroyed.
  let destroyed;
  ({ system, v, destroyed } = atst(7));
  for (let t = 0; t < 60; t++) system.update(1 / 60, v, {});
  assert.ok(v.position.y < 0.6, `under the water at ${v.position.y}`);
  assert.ok(v.health > v.maxHealth - 15, `health ${v.health} after a second`);
  for (let t = 0; t < 60; t++) system.update(1 / 60, v, {});
  assert.ok(Math.abs(v.maxHealth - v.health - 600) <= 20, `lost ${v.maxHealth - v.health} in the second second`);
  for (let t = 0; t < 60 * 20 && v.alive; t++) system.update(1 / 60, v, {});
  assert.equal(v.alive, false);
  assert.equal(destroyed.length, 1);
  system.dispose();
});
