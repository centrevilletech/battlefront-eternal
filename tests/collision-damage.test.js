// Fall damage (src/collision-damage.js): the 2004 game's collision
// damage, threshold T0 + T1 (n.up) + T2 (n.forward) on the speed into a
// surface, damage S x (speed - threshold)^2, for soldiers (10 3 0, 1 0 0)
// and droidekas (7.5 2.5 0, 1 0 0), and the soldier's world bounds. Synthetic
// worlds; no retail content.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CollisionWorld } from '../src/collision.js';
import { Soldier } from '../src/soldier.js';
import { Droideka } from '../src/droideka.js';
import { SOLDIER_COLLISION, DROIDEKA_COLLISION, collisionRule, contactDamage, outOfWorld, takeImpacts } from '../src/collision-damage.js';

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

test('the damage of a contact: nothing up to the threshold, the square of the excess past it', () => {
  const rule = collisionRule({});
  // Level ground: threshold 10 + 3 = 13 m/s.
  assert.equal(contactDamage(rule, -13, 1, 0), 0);
  assert.equal(contactDamage(rule, -12, 1, 0), 0);
  near(contactDamage(rule, -20, 1, 0), 49, 1e-9);
  // A wall: 10 m/s.
  near(contactDamage(rule, -12, 0, 0), 4, 1e-9);
  // Moving away from it, or along it, never hurts.
  assert.equal(contactDamage(rule, 0, 1, 0), 0);
  assert.equal(contactDamage(rule, 30, 1, 0), 0);
  assert.equal(contactDamage(collisionRule({ CollisionScale: [0, 0, 0] }), -60, 1, 0), 0);
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

test('a building counts only the upward share of a contact: its walls and ceilings never hurt', () => {
  const s = new Soldier(floor(), {});
  const lower = { role: 'lower' }, upper = { role: 'upper' };
  const hit = (part, n, v, kind) => {
    s.collisionDamage = 0;
    s.velocity.set(...v);
    s.contact(part, new THREE.Vector3(...n).normalize(), 0, new THREE.Vector3(...n).normalize(), kind);
    return s.collisionDamage;
  };
  near(hit(lower, [0, 1, 0], [0, -20, 0], 'building'), 49, 1e-9, 'floor');
  near(hit(lower, [0, 1, 0], [0, -20, 0], 'terrain'), 49, 1e-9, 'terrain');
  assert.equal(hit(lower, [1, 0, 0], [-25, 0, 0], 'building'), 0, 'a wall');
  near(hit(lower, [1, 0, 0], [-15, 0, 0], 'terrain'), 25, 1e-9, 'a terrain face as steep as a wall');
  assert.equal(hit(upper, [0, -1, 0], [0, 25, 0], 'building'), 0, 'a ceiling');
  // Nothing he doesn't hit (a moving hull, kind null), and nothing a contact
  // doesn't stop: his upper body over a ledge lets his fall go on.
  assert.equal(hit(lower, [0, 1, 0], [0, -30, 0], null), 0, 'a moving hull');
  assert.equal(hit(upper, [0, 1, 0], [0, -30, 0], 'building'), 0, 'the upper body over a ledge');
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

test('a droideka coming down too hard: past 10 m/s on level ground', () => {
  const world = {
    gather: () => [],
    cast(from, direction, reach, hit) {
      if (direction.y >= 0) return null;
      const distance = -from.y / direction.y;
      if (distance < 0 || distance > reach) return null;
      hit.distance = distance; hit.point.copy(from).addScaledVector(direction, distance); hit.normal.set(0, 1, 0); hit.terrain = true;
      return hit;
    },
  };
  const actor = { root: new THREE.Group(), bones: [Object.assign(new THREE.Bone(), { name: 'DummyRoot' })] };
  for (const [h, unhurt] of [[4, true], [20, false]]) {
    const d = new Droideka(world, {}, new Map(), actor);
    assert.deepEqual(d.collisionRule, { threshold: [7.5, 2.5, 0], scale: [1, 0, 0] });
    d.position.set(0, h, 0); d.velocity.set(0, 0, 0); d.grounded = false;
    let damage = 0, impact = 0;
    for (let t = 0; t < 600; t++) {
      const before = d.velocity.y;
      d.tick({ forward: 0, right: 0, turn: 0 }, TICK);
      damage += d.collisionDamage;
      if (d.collisionDamage > 0 || d.grounded && !impact) impact = -(before - 9.8 * TICK);
      if (d.grounded) break;
    }
    if (unhurt) assert.equal(damage, 0, `${h} m`);
    else near(damage, (impact - 10) ** 2, 1e-6, `${h} m`);
  }
});

test('the soldier\'s world bounds: falling past 60 m/s, or 1000 m out, or 500 m up or down', () => {
  const at = (x, y, z) => new THREE.Vector3(x, y, z);
  assert.equal(outOfWorld(at(0, 0, 0), at(0, -59.9, 0)), false);
  assert.equal(outOfWorld(at(0, 0, 0), at(0, -60.1, 0)), true);
  assert.equal(outOfWorld(at(1000.5, 0, 0), at(0, 0, 0)), true);
  assert.equal(outOfWorld(at(0, 0, -1000.5), at(0, 0, 0)), true);
  assert.equal(outOfWorld(at(0, 500.5, 0), at(0, 0, 0)), true);
  assert.equal(outOfWorld(at(0, -500.5, 0), at(0, 0, 0)), true);
  assert.equal(outOfWorld(at(999, 499, -999), at(30, 59, 30)), false);
});

test('a body takes what its contacts did after its step: health, then the bounds kill; never on a seat or dead', () => {
  const unit = (fields = {}) => {
    const taken = [];
    return {
      taken, removed: false, seat: null, walker: false,
      soldier: { collisionDamage: 0, position: new THREE.Vector3(), velocity: new THREE.Vector3() },
      combat: { dead: false, damage(amount) { taken.push(amount); if (amount === Infinity) this.dead = true; } },
      ...fields,
    };
  };
  const hurt = unit(); hurt.soldier.collisionDamage = 49; takeImpacts(hurt);
  assert.deepEqual(hurt.taken, [49]);
  const falling = unit(); falling.soldier.velocity.y = -61; takeImpacts(falling);
  assert.deepEqual(falling.taken, [Infinity]);
  const droideka = unit({ walker: true }); droideka.soldier.velocity.y = -61; takeImpacts(droideka);
  assert.deepEqual(droideka.taken, []);
  const seated = unit({ seat: {} }); seated.soldier.collisionDamage = 49; takeImpacts(seated);
  assert.deepEqual(seated.taken, []);
  const dead = unit(); dead.combat.dead = true; dead.soldier.collisionDamage = 49; takeImpacts(dead);
  assert.deepEqual(dead.taken, []);
  assert.deepEqual(SOLDIER_COLLISION, { threshold: [10, 3, 0], scale: [1, 0, 0] });
});
