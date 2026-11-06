import './fixtures/native-anim.js';
// Fall damage (src/collision-damage.js): the 2004 game's collision
// damage, threshold T0 + T1 (n.up) + T2 (n.forward) on the speed into a
// surface, damage S x (speed - threshold)^2, for soldiers (10 3 0, 1 0 0)
// and droidekas (7.5 2.5 0, 1 0 0), and the soldier's world bounds. Synthetic
// worlds; no retail content.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CollisionWorld } from '../tools/collision-fixture.mjs';
import { Soldier } from '../src/soldier.js';

import { SOLDIER_COLLISION, DROIDEKA_COLLISION, collisionRule } from '../src/collision-damage.js';

const near = (a, b, eps, message = '') => assert.ok(Math.abs(a - b) <= eps, `${message} ${a} vs ${b}`);
const TICK = 1 / 60;

// A floor 200 m across at y = 0: as building polygons, or as terrain.
function floor({ terrain = false } = {}) {
  const positions = new Float32Array([-100, 0, -100, 100, 0, -100, 100, 0, 100, -100, 0, 100]);
  const indices = new Uint32Array([0, 2, 1, 0, 3, 2]);
  if (terrain) return new CollisionWorld(new Float32Array(0), new Uint32Array(0), new Uint32Array(0), positions, indices);
  return new CollisionWorld(positions, indices, new Uint32Array([0, 1]));
}

// Dropped still from h metres: the damage his landing did, and the speed he
// hit the ground at (the tick's velocity after gravity, as the contact sees it).
function drop(world, h, def = {}) {
  const s = new Soldier(world, def);
  s.spawn(0, 0, 0, 0);
  s.position.set(0, h, 0); s.settled.copy(s.position); s.previous.copy(s.position);
  s.velocity.set(0, 0, 0); s.setAir('fall'); s.grounded = false;
  const input = { forward: 0, right: 0, turn: 0 };
  let damage = 0, impact = 0;
  for (let t = 0; t < 600 && !s.grounded; t++) {
    const before = s.velocity.y;
    s.tick(input, TICK);
    damage += s.collisionDamage;
    if (s.grounded) impact = -(before - 18 * TICK);
  }
  assert.ok(s.grounded, `landed from ${h} m`);
  return { damage, impact, y: s.position.y };
}

test('the class keys: CollisionThreshold and CollisionScale over the defaults, a single value setting the first', () => {
  assert.deepEqual(collisionRule({}), { threshold: [10, 3, 0], scale: [1, 0, 0] });
  assert.deepEqual(collisionRule({}, DROIDEKA_COLLISION), { threshold: [7.5, 2.5, 0], scale: [1, 0, 0] });
  // The jet trooper, the dark trooper and the Geonosian: no fall damage.
  assert.deepEqual(collisionRule({ CollisionScale: [0, 0, 0] }).scale, [0, 0, 0]);
  // "%f %f %f" on "1.5" sets the first only.
  assert.deepEqual(collisionRule({ CollisionScale: [1.5], CollisionThreshold: '15' }), { threshold: [15, 3, 0], scale: [1.5, 0, 0] });
  assert.deepEqual(collisionRule({ CollisionScale: '1.0 -0.5 0' }).scale, [1, -0.5, 0]);
});

test('a trooper dropped onto level ground: unhurt to about 4.7 m, (v - 13)^2 beyond, dead from about 26 m', () => {
  for (const terrain of [false, true]) {
    const world = floor({ terrain });
    for (const h of [1, 3, 4.5, 4.6]) assert.equal(drop(world, h).damage, 0, `${h} m on ${terrain ? 'terrain' : 'a building'}`);
    for (const h of [6, 8, 10, 15, 20, 25]) {
      const { damage, impact } = drop(world, h);
      near(damage, (impact - 13) ** 2, 1e-6, `${h} m`);
    }
    // Some 10 m costs a 300-health trooper about a ninth; 25.5 m all but 3 of it.
    near(drop(world, 10).damage, 34.8, 0.5, '10 m');
    assert.ok(drop(world, 25.5).damage < 300);
    assert.ok(drop(world, 26).damage >= 300);
  }
});

test('the jet classes (CollisionScale 0 0 0) land from any height unhurt', () => {
  assert.equal(drop(floor(), 40, { CollisionScale: [0, 0, 0] }).damage, 0);
  assert.equal(drop(floor({ terrain: true }), 40, { CollisionScale: [0, 0, 0] }).damage, 0);
});

test('landing on a moving body does no damage; the port has no velocity exchange with a hull', () => {
  const world = floor({ terrain: true });
  const box = new THREE.BoxGeometry(10, 1, 10).translate(0, 0.5, 0);
  const shape = world.createBodyShape(box.attributes.position.array, box.index.array, new Uint32Array(box.index.count / 3));
  world.addMovingBody(shape, new THREE.Matrix4().makeTranslation(0, 2, 0));
  const { damage, y } = drop(world, 15);
  assert.equal(damage, 0);
  near(y, 3, 0.05, 'stood on the hull, 3 m up');
});



