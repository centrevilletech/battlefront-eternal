import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { JetpackWake } from '../src/jetpack-wake.js';

function fixture() {
  const scene = new THREE.Scene();
  const wake = new JetpackWake({ scene, random: () => .5 });
  const nozzle = { position: new THREE.Vector3(), direction: new THREE.Vector3(0, -1, 0), radius: .034 };
  const input = { nozzles: [nozzle], velocity: new THREE.Vector3(6, 0, 0), active: true, throttle: 1 };
  const run = (seconds, rate = 60) => { for (let i = 0; i < Math.round(seconds * rate); i++) { nozzle.position.x += 6 / rate; wake.update(1 / rate, input); } };
  return { scene, wake, nozzle, input, run };
}

test('moving nozzle emissions fill the actual between-frame path rather than clumping at the endpoint', () => {
  const f = fixture(); f.wake.update(0, f.input);
  f.nozzle.position.x = 1.2; f.wake.update(.2, f.input);
  const pool = f.wake.wisps, birth = pool.attributes.birth.array;
  assert.equal(pool.count, 7);
  for (let i = 0; i < pool.count; i++) {
    assert.ok(Math.abs(birth[i * 4] - (i + 1) / 6) < 1e-6);
    assert.ok(Math.abs(birth[i * 4 + 3] - (i + 1) / 36) < 1e-6);
  }
  assert.ok(birth[0] < .2 && birth[24] > 1.1);
});

test('emission timestamps and positions are stable at 30, 60 and 144 FPS', () => {
  const results = [30, 60, 144].map(rate => {
    const f = fixture(); f.wake.update(0, f.input); f.run(1, rate);
    return f.wake.pools.map(pool => {
      const values = [];
      for (let i = 0; i < pool.count; i++) values.push([pool.attributes.birth.array[i * 4 + 3], pool.attributes.birth.array[i * 4]]);
      return values.sort((a, b) => a[0] - b[0]);
    });
  });
  for (const result of results.slice(1)) {
    for (let kind = 0; kind < 3; kind++) {
      assert.equal(result[kind].length, results[0][kind].length);
      for (let i = 0; i < result[kind].length; i++) {
        assert.ok(Math.abs(result[kind][i][0] - results[0][kind][i][0]) < 1e-6);
        assert.ok(Math.abs(result[kind][i][1] - results[0][kind][i][1]) < 1e-5);
      }
    }
  }
});

test('wake stops emitting after cutoff, decays completely, and has sparse embers and low-opacity smoke', () => {
  const f = fixture(); f.run(1);
  assert.ok(f.wake.wisps.count > f.wake.embers.count * 3);
  assert.ok(f.wake.smoke.attributes.style.array[1] <= .121);
  const emitted = f.wake.emitted; f.input.active = false; f.run(.1);
  assert.equal(f.wake.emitted, emitted); assert.ok(f.wake.smoke.count > 0);
  f.run(1.5); assert.ok(f.wake.pools.every(pool => pool.count === 0));
});

test('wake stays within three draws and fixed buffers throughout a long burn', () => {
  const f = fixture();
  const buffers = f.wake.pools.map(pool => pool.attributes.birth.array);
  f.run(20);
  assert.equal(f.scene.children.length, 3);
  f.wake.pools.forEach((pool, i) => {
    assert.ok(pool.count <= pool.capacity); assert.equal(pool.attributes.birth.array, buffers[i]);
    for (const attr of pool.attributeList) for (const v of attr.array) assert.ok(Number.isFinite(v));
  });
  const counts = f.wake.pools.map(pool => pool.count), emitted = f.wake.emitted;
  f.wake.update(0, f.input);
  assert.deepEqual(f.wake.pools.map(pool => pool.count), counts); assert.equal(f.wake.emitted, emitted);
});

test('teleport clears the previous wake and never interpolates across the map', () => {
  const f = fixture(); f.run(.5); f.nozzle.position.x += 100; f.nozzle.previous = new THREE.Vector3(0, 0, 0);
  f.wake.update(.1, f.input);
  for (const pool of f.wake.pools) for (let i = 0; i < pool.count; i++) assert.ok(pool.attributes.birth.array[i * 4] > 100);
  f.wake.clear(); assert.ok(f.wake.pools.every(pool => !pool.mesh.visible && pool.count === 0));
  f.wake.dispose(); assert.equal(f.scene.children.length, 0);
});
