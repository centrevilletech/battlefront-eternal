import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { FoliageGpuBatch } from '../src/foliage-gpu.js';

// The patches a batch draws, by testing every one (the selection before the
// cell index): the index must choose the same ones, in the same order.
function everyPatch(instances, camera, back, frustum, nearEnd, grass) {
  const selected = [], delta = new THREE.Vector3();
  for (let i = 0; i < instances.length; i++) {
    const inst = instances[i], maxDist = inst.maxDistance || nearEnd;
    if (inst.sphere && !frustum.intersectsSphere(inst.sphere)) continue;
    if (Math.hypot(inst.position.x - camera.position.x, inst.position.z - camera.position.z) > inst.cullDistance) continue;
    const depth = -delta.subVectors(inst.position, camera.position).dot(back);
    if (grass ? depth > maxDist || depth < -5 : depth > maxDist + 20 || depth < -25) continue;
    selected.push(i);
  }
  return selected;
}

test('the cell index selects exactly the patches testing every one does, in order', () => {
  let seed = 7;
  const random = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32);
  for (const grass of [true, false]) {
    const card = { x: 0, y: 0, z: 0, p: new THREE.Vector3(), width: 1, height: 1, size: 1, part: 0, grey: 1, phase: 0, rate: 1 };
    const batch = new FoliageGpuBatch({ add() {} }, null, [card], {}, grass, 4096);
    const instances = [];
    for (let i = 0; i < 6000; i++) {
      const position = new THREE.Vector3((random() - .5) * 2000, random() * 30, (random() - .5) * 2000);
      const terrain = random() < .9, distance = 5 + random() * 40;
      const sphere = grass && !terrain ? null : new THREE.Sphere(position.clone(), 1 + random() * 3);
      instances.push({ position, matrix: new THREE.Matrix4().setPosition(position), sphere, tint: [1, 1, 1],
        cullDistance: terrain ? 1.45 * distance + (sphere?.radius || 0) : Infinity, maxDistance: terrain ? (grass ? 0 : distance) : (grass ? 100 : 0) });
    }
    const camera = new THREE.PerspectiveCamera(70, 16 / 9, .5, 3000), frustum = new THREE.Frustum(), back = new THREE.Vector3();
    for (let view = 0; view < 300; view++) {
      camera.position.set((random() - .5) * 2100, random() * 40, (random() - .5) * 2100);
      camera.rotation.set((random() - .5) * 1.2, random() * Math.PI * 2, 0, 'YXZ');
      camera.updateMatrixWorld();
      frustum.setFromProjectionMatrix(new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse));
      back.setFromMatrixColumn(camera.matrixWorld, 2);
      const nearEnd = 20 + random() * 60;
      batch.update(instances, 0, camera, new THREE.Vector3(1, 0, 0), new THREE.Vector3(0, 1, 0), back, frustum, nearEnd);
      assert.deepEqual([...batch.selected], everyPatch(instances, camera, back, frustum, nearEnd, grass), `${grass ? 'grass' : 'leaves'} view ${view}`);
    }
  }
});
