import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { DroidekaTrail } from '../src/droideka-trail.js';

function fixture() {
  const env = { roof: false, floor: true, terrain: false, mode: 'enhanced', casts: 0, water: false };
  const collision = { cast(origin, direction, far, out) {
    env.casts++;
    if (direction.y > 0) return env.roof ? out : null;
    if (!env.floor) return null;
    out.point.set(origin.x, 0, origin.z); out.normal.set(0, 1, 0); out.terrain = env.terrain;
    return out;
  } };
  const trail = new DroidekaTrail({ scene: new THREE.Scene(), collision, water: { over: () => env.water, level: 0 }, settings: { get: () => env.mode }, random: () => 0.5 });
  const soldier = { mode: 12, dead: false, air: null, grounded: true, position: new THREE.Vector3(), velocity: new THREE.Vector3(10, 0, 0) };
  function run(seconds) { for (let i = 0; i < Math.round(seconds * 60); i++) { soldier.position.addScaledVector(soldier.velocity, 1 / 60); trail.update(1 / 60, soldier); } }
  return { env, trail, soldier, run };
}

test('outdoor paved courtyards emit a lighter trail than terrain, with cached probes', () => {
  const paved = fixture(), terrain = fixture(); terrain.env.terrain = true;
  paved.run(1); terrain.run(1);
  assert.ok(paved.trail.count > 40);
  assert.ok(paved.env.casts < 60, `expected cached rays, saw ${paved.env.casts}`);
  assert.equal(paved.trail.count, terrain.trail.count);
  assert.ok(paved.trail.records[10] < terrain.trail.records[10]);
});

test('roof, missing ground, water, air, death, stopped and unfolded prevent emission', () => {
  for (const condition of ['roof', 'floor', 'water', 'air', 'dead', 'stopped', 'unfolded']) {
    const f = fixture();
    if (condition === 'roof') f.env.roof = true;
    if (condition === 'floor') f.env.floor = false;
    if (condition === 'water') f.env.water = true;
    if (condition === 'air') f.soldier.air = 'fall';
    if (condition === 'dead') f.soldier.dead = true;
    if (condition === 'stopped') f.soldier.velocity.set(0, 0, 0);
    if (condition === 'unfolded') f.soldier.mode = 0;
    f.run(1);
    assert.equal(f.trail.emitted, 0, condition);
  }
});

test('outdoor particles decay after entering a building; Classic clears immediately', () => {
  const f = fixture(); f.run(1); const emitted = f.trail.emitted;
  f.env.roof = true; f.soldier.position.x += 1; f.run(0.5);
  assert.equal(f.trail.emitted, emitted);
  assert.ok(f.trail.count > 0);
  f.run(2); assert.equal(f.trail.count, 0); assert.equal(f.trail.mesh.visible, false);
  f.env.roof = false; f.run(1); assert.ok(f.trail.count > 0);
  f.env.mode = 'classic'; f.run(1 / 60); assert.equal(f.trail.count, 0);
  f.env.mode = 'enhanced'; f.run(0.5); assert.ok(f.trail.count > 0);
});

test('speed and turning strengthen the trail while its pool and buffers remain bounded', () => {
  const slow = fixture(), fast = fixture(), turning = fixture();
  slow.soldier.velocity.set(3, 0, 0); slow.run(1); fast.run(1);
  for (let i = 0; i < 60; i++) {
    turning.soldier.velocity.set(Math.cos(i / 10) * 10, 0, Math.sin(i / 10) * 10);
    turning.run(1 / 60);
  }
  assert.ok(fast.trail.emitted > slow.trail.emitted);
  assert.ok(turning.trail.emitted > fast.trail.emitted);
  const buffers = fast.trail.attributes.map(a => a.array);
  fast.run(15);
  assert.ok(fast.trail.count <= 160);
  fast.trail.attributes.forEach((a, i) => assert.equal(a.array, buffers[i]));
  for (const n of fast.trail.records) assert.ok(Number.isFinite(n));
});

test('teleporting clears the old wake and zero-time draws cannot emit', () => {
  const f = fixture(); f.run(1); const emitted = f.trail.emitted;
  f.trail.update(0, f.soldier); assert.equal(f.trail.emitted, emitted);
  f.soldier.position.x += 100; f.soldier.velocity.set(0, 0, 0); f.trail.update(1 / 60, f.soldier);
  assert.equal(f.trail.count, 0);
});
