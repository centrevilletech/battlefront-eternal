// Walkers on their legs and their guns' barrels, as the original game has them
// (docs/vehicles.md "Standing on the ground", "Aimers and barrels"), on the
// prepared Hoth and Geonosis packs (skipped without them).
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Vehicles } from '../src/vehicles.js';
import { packWith } from '../tools/vehicle-packs.mjs';

const maps = new URL('../public/maps/', import.meta.url);
// A walker alone on a plane tilted `slope` radians about x, driven.
function walker(name, map, slope = 0) {
  const pack = packWith(maps, [name], map);
  if (!pack) return null;
  const dir = new URL('./', pack);
  const bin = (file) => { const b = fs.readFileSync(new URL(file, dir)); return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength); };
  const data = JSON.parse(fs.readFileSync(pack));
  const g = new THREE.PlaneGeometry(4000, 4000).rotateX(-Math.PI / 2).rotateX(slope);
  const collision = new CollisionWorld(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
  const system = new Vehicles({ scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera(), collision,
    data: { ...data, blockers: null, spawns: [{ name: 'test', matrix: new THREE.Matrix4().toArray(), team: 1, post: '', classes: { 1: name } }] },
    buffer: bin('models.bin'), indexBuffer: bin('models_index.bin'), animationBuffer: bin('animations.bin'),
    texture: () => null, effects: { play() { return null; } }, ordnance: { explosion() {}, fire() { return null; }, lights: { flash() {} } } });
  const v = system.vehicles[0];
  v.driver = {}; v.seatIndex = 0;
  return { system, v };
}
const has = (name, map) => Boolean(packWith(maps, [name], map));

test('a four-legged walker pitches with the slope under its feet; the AT-ST stays level on its one pair', { skip: !has('imp_walk_atat', 'hot1') }, () => {
  const slope = 0.15;
  for (const [name, map, pitched] of [['imp_walk_atat', 'hot1', true], ['imp_walk_atst_snow', 'hot1', false], ['cis_walk_spider', 'geo1', true]]) {
    if (!has(name, map)) continue;
    const { system, v } = walker(name, map, slope);
    assert.ok(v.def.fx.legs?.pairs.length >= 1, `${name} has leg pairs`);
    for (let t = 0; t < 600; t++) system.update(1 / 60, v, t < 30 ? {} : { forward: 1 });
    // The plane rises toward -z; walking +z goes downhill: nose down.
    if (pitched) assert.ok(Math.abs(v.tilt + slope) < 0.01, `${name} pitched ${v.tilt}`);
    else assert.ok(Math.abs(v.tilt) < 1e-6, `${name} level`);
    // Its feet on the ground: the lowest foot's ray finds ground within a
    // few centimetres.
    const ground = -Math.tan(slope) * v.position.z;
    assert.ok(Math.abs(v.position.y - ground) < (pitched ? 0.25 : 0.6), `${name} at ${v.position.y} over ${ground}`);
    system.dispose();
  }
});

test('the AT-AT pilot fires both chin barrels, the AT-ST both chin guns, each from its own barrel', { skip: !has('imp_walk_atat', 'hot1') }, () => {
  const nodes = (v, seat) => v.seats[seat].weapons[0].muzzles.map((m) => m.fireNode.name.toLowerCase());
  let { system, v } = walker('imp_walk_atat', 'hot1');
  assert.deepEqual(nodes(v, 0), ['barrel_lcannon', 'barrel_rcannon']);
  assert.deepEqual(nodes(v, 1), ['hp_fire_lgun', 'hp_fire_rgun']);
  assert.equal(v.seats[0].weapons[0].muzzles[0].recoil, 0.4);
  system.dispose();
  ({ system, v } = walker('imp_walk_atst_snow', 'hot1'));
  assert.deepEqual(nodes(v, 0), ['hp_cannon_1', 'hp_cannon_2']);
  const [rear, left] = v.seats[1].weapons[0].aimers;
  assert.deepEqual(rear.yawLimits.map((x) => Math.round(x * 180 / Math.PI)), [-90, 25]);
  assert.deepEqual(left.yawLimits.map((x) => Math.round(x * 180 / Math.PI)), [-25, 90]);
  system.dispose();
});

test('a barrel held at its aimer\'s limit fires nothing without FireOutsideLimits', { skip: !has('imp_walk_atst_snow', 'hot1') }, () => {
  const { system, v } = walker('imp_walk_atst_snow', 'hot1');
  const [rear] = v.seats[1].weapons[0].aimers;
  rear.inLimits = false;
  assert.equal(v.heldByLimits(1, rear), true);
  rear.FireOutsideLimits = '1';
  assert.equal(v.heldByLimits(1, rear), false);
  rear.inLimits = true; delete rear.FireOutsideLimits;
  assert.equal(v.heldByLimits(1, rear), false);
  system.dispose();
});

test('another hull driving into a walker does not bulldoze it', { skip: !has('rep_walk_atte', 'geo1') }, () => {
  // A hailfire stopped by an AT-TE's body still sat inside the AT-TE's probe
  // spheres, which reach past it, and the AT-TE pushed itself fully out of
  // it every step: driven across Geonosis at the hailfire's 8 m/s.
  const { system, v } = walker('rep_walk_atte', 'geo1');
  for (let t = 0; t < 60; t++) system.update(1 / 60, v, {});
  const c = system.collision;
  const g = new THREE.BoxGeometry(4, 3, 4);
  const shape = c.createBodyShape(g.attributes.position.array, g.index.array, new Uint32Array(g.index.count / 3));
  const m = new THREE.Matrix4();
  // Beside its flank, inside the probes' reach, creeping in 0.14 m a step.
  let x = v.position.x - v.size.x / 2 - 1.5;
  const body = c.addMovingBody(shape, m.makeTranslation(x, 1.5, v.position.z), { name: 'pusher' });
  const start = v.position.clone();
  for (let t = 0; t < 120; t++) {
    x = Math.min(x + 0.14, v.position.x - v.size.x / 2 + 1.5);
    c.moveBody(body, m.makeTranslation(x, 1.5, v.position.z));
    system.update(1 / 60, v, {});
  }
  assert.ok(Math.hypot(v.position.x - start.x, v.position.z - start.z) < 0.05, `pushed from ${start.toArray()} to ${v.position.toArray()}`);
  // Walking into a hull still stops it there.
  c.moveBody(body, m.makeTranslation(v.position.x, 1.5, v.position.z + v.size.z / 2 + 3));
  for (let t = 0; t < 600; t++) system.update(1 / 60, v, { forward: 1 });
  assert.ok(v.position.z - start.z < 3, `walked through it to ${v.position.z}`);
  system.dispose();
});
