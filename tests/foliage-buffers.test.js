import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Foliage } from '../src/foliage.js';

function batch() {
  const f = new Foliage(new THREE.Scene(), {
    classes: { leaves: { base: 'leafpatch', NumParticles: 4, Radius: 2, Height: 2, Seed: 17 } },
    instances: [{ class: 'leaves', matrix: new THREE.Matrix4().toArray() }],
  }, { texture: () => new THREE.Texture(), nearEnd: () => 250 });
  return f.kinds.get('leaves').batch;
}

test('allocation-free foliage card writes exactly retain prior positions, colors and mirrored UVs', () => {
  const b = batch();
  const positions = new Float32Array(b.positions.length);
  const colors = new Float32Array(b.colors.length);
  const uvs = new Float32Array(b.uvs.length);
  for (let k = 0; k < b.capacity; k++) {
    const corners = Array.from({ length: 4 }, (_, j) => new THREE.Vector3(k + j * 0.17, -k - j * 1.03, Math.sin(j + k)));
    const color = [0.13 * k, 0.57, 0.89, 0.33];
    const uv = [0.25, 0.125, 0.75, 0.875];
    const mirror = k % 2 === 1;
    // The previous card writer, retained here as an exact attribute oracle.
    for (const [j, v] of corners.entries()) positions.set([v.x, v.y, v.z], (k * 4 + j) * 3);
    for (let j = 0; j < 4; j++) colors.set(color, (k * 4 + j) * 4);
    const [u0, v0, u1, v1] = uv;
    const [a, z] = mirror ? [u1, u0] : [u0, u1];
    uvs.set([a, v1, z, v1, a, v0, z, v0], k * 8);
    b.push(...corners, ...color, uv, mirror);
  }
  assert.deepEqual(b.positions, positions);
  assert.deepEqual(b.colors, colors);
  assert.deepEqual(b.uvs, uvs);
});

test('foliage uploads only rebuilt visible cards and clears old ranges on each frame', () => {
  const b = batch();
  const corner = new THREE.Vector3(1, 2, 3);
  const push = () => b.push(corner, corner, corner, corner, 1, 1, 1, 0.5, [0, 0, 1, 1], false);
  push(); push(); b.end();
  for (const attribute of Object.values(b.geometry.attributes)) {
    assert.deepEqual(attribute.updateRanges, [{ start: 0, count: 8 * attribute.itemSize }]);
  }
  assert.equal(b.geometry.drawRange.count, 12);
  b.begin(); push(); b.end();
  for (const attribute of Object.values(b.geometry.attributes)) {
    assert.deepEqual(attribute.updateRanges, [{ start: 0, count: 4 * attribute.itemSize }]);
  }
  const versions = Object.values(b.geometry.attributes).map(a => a.version);
  b.begin(); b.end();
  assert.equal(b.mesh.visible, false);
  assert.equal(b.geometry.drawRange.count, 0);
  assert.deepEqual(Object.values(b.geometry.attributes).map(a => a.version), versions);
  for (const attribute of Object.values(b.geometry.attributes)) assert.deepEqual(attribute.updateRanges, []);
});
