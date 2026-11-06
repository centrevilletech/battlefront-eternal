import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Destruction } from '../src/destruction.js';
import { loadEnvironment } from '../src/environment/module.js';

await loadEnvironment();

const FLOOR = {
  cast(origin, direction, far, out) {
    if (direction.y >= 0 || origin.y < 0) return null;
    const t = origin.y / -direction.y;
    if (t > far) return null;
    out.distance = t;
    out.point.copy(origin).addScaledVector(direction, t);
    out.normal.set(0, 1, 0);
    return out;
  },
};
const settings = { get: () => 'enhanced', onChange: () => () => {} };
const lights = { flash() {}, add() {} };

function make() {
  const scene = new THREE.Scene();
  return { scene, d: new Destruction({ scene, collision: FLOOR, lights, settings, model: () => null, seed: 7 }) };
}

// The pools and arcs as meshes over the Rust phase's records. (Their births,
// retirement, rebase and arcs are held to a recorded run by the
// recorded battle in destruction-golden.test.js, and by native tests.)
test('a blast fills the pools and the arcs, each one draw, and they empty as their lives end', () => {
  const { d, scene } = make();
  assert.equal(d.fx.draws, 0);
  d.blast(new THREE.Vector3(1, 0.4, 3), 1);
  d.update(1 / 60);
  assert.ok(d.fx.hot.count > 50 && d.fx.smoke.count > 20 && d.fx.arcs.live > 0);
  assert.equal(d.fx.hot.mesh.geometry.instanceCount, d.fx.hot.count);
  assert.equal(d.fx.draws, 3, 'the hot pool, the smoke pool and the arcs');
  assert.equal(d.fx.hot.arrays[0].length, 1536 * 4, 'a fixed pool');
  for (let i = 0; i < 1700; i++) d.update(1 / 60); // (a scorch mark lasts 26 s)
  assert.deepEqual([d.fx.hot.count, d.fx.smoke.count, d.fx.arcs.live], [0, 0, 0]);
  assert.equal(d.fx.draws, 0, 'an empty pool draws nothing');
  d.dispose();
  assert.equal(scene.children.length, 0);
});

test('a pool that is full drops new births instead of growing', () => {
  const { d } = make();
  for (let i = 0; i < 40; i++) d.blast(new THREE.Vector3(i, 0.4, 0), 1.5);
  d.update(0);
  assert.ok(d.fx.hot.count <= 1536 && d.fx.hot.dropped > 0, `${d.fx.hot.count} kept, ${d.fx.hot.dropped} dropped`);
  assert.equal(d.fx.hot.emitted + d.fx.hot.dropped >= d.fx.hot.count, true);
});

test('births in frames stepped without a draw still reach the GPU', () => {
  const { d } = make();
  d.blast(new THREE.Vector3(0, 0.4, 0), 1);
  d.update(1 / 60);
  // Uploaded, as the renderer does after drawing.
  for (const attribute of d.fx.hot.attributes) attribute.clearUpdateRanges();
  const before = d.fx.hot.count;
  d.blast(new THREE.Vector3(0, 0.4, 0), 1);
  d.update(1 / 60);
  d.blast(new THREE.Vector3(0, 0.4, 0), 1);
  d.update(1 / 60); // stepped again before any draw
  for (const attribute of d.fx.hot.attributes) {
    assert.equal(attribute.updateRanges.length, 1);
    assert.ok(attribute.updateRanges[0].start <= before * 4 && attribute.updateRanges[0].count > 0, 'both undrawn births are in the range');
  }
});
