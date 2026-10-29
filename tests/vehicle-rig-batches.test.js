import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { vehicleBatches, attachVehicleBatches } from '../src/vehicle-rig.js';

const tri = () => {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute([0, 0, 0, 1, 0, 0, 0, 1, 0], 3));
  geometry.setAttribute('normal', new THREE.Float32BufferAttribute([0, 1, 0, 0, 1, 0, 0, 1, 0], 3));
  geometry.setAttribute('uv', new THREE.Float32BufferAttribute([0, 0, 1, 0, 0, 1], 2));
  geometry.setIndex([0, 1, 2]);
  return geometry;
};
// base > drum > gun, as a turret is rigged.
const RIG = { bones: [
  { name: 'base', parent: -1, local: new THREE.Matrix4().toArray() },
  { name: 'Drum', parent: 0, local: new THREE.Matrix4().makeTranslation(0, 1, 0).toArray() },
  { name: 'gun', parent: 1, local: new THREE.Matrix4().makeTranslation(0, 0, 1).toArray() },
] };
const parts = () => [
  { geometry: tri(), texture: 'metal', flags: 1, bone: 'base' },
  { geometry: tri(), texture: 'metal', flags: 1, bone: 'drum' },
  { geometry: tri(), texture: 'metal', flags: 1, bone: 'GUN' },
  { geometry: tri(), texture: 'metal', flags: 1, bone: null },
];
const texture = () => null;

test('without separate bones every part of a material is one batch, as vehicles pack', () => {
  const batches = vehicleBatches(RIG, parts(), texture);
  assert.equal(batches.length, 1);
  assert.equal(batches[0].attach, null);
  assert.equal(batches[0].geometry.getAttribute('position').count, 12);
  const skin = batches[0].geometry.getAttribute('skinIndex');
  assert.deepEqual([skin.getX(0), skin.getX(3), skin.getX(6), skin.getX(9)], [0, 1, 2, 3], 'the loose part on the static bone');
});

test('separate bones get batches of their own, hung under those bones with an identity bind', () => {
  const source = parts();
  const batches = vehicleBatches(RIG, source, texture, { separate: ['drum', 'gun'], dispose: false });
  assert.equal(batches.length, 3);
  assert.deepEqual(batches.map((b) => b.attach).sort(), ['drum', 'gun', null].sort());
  assert.ok(source.every((p) => p.geometry.attributes.position), 'dispose false keeps the pack');
  const root = new THREE.Group();
  const bones = RIG.bones.map((d) => { const b = new THREE.Object3D(); b.name = d.name; new THREE.Matrix4().fromArray(d.local).decompose(b.position, b.quaternion, b.scale); return b; });
  RIG.bones.forEach((d, i) => (d.parent >= 0 ? bones[d.parent] : root).add(bones[i]));
  const byName = Object.fromEntries(bones.map((b) => [b.name.toLowerCase(), b]));
  const meshes = [];
  const casters = [];
  const skeleton = attachVehicleBatches(root, bones, batches, (m) => casters.push(m), { byName, motion: false, meshes });
  assert.equal(meshes.length, 3);
  assert.equal(casters.length, 3);
  const under = (name) => meshes.find((m) => m.parent === byName[name]);
  assert.ok(under('drum') && under('gun'), 'the drum and gun meshes sit under their bones');
  assert.ok(meshes.find((m) => m.parent === root));
  for (const mesh of meshes) {
    assert.deepEqual(mesh.bindMatrix.elements, new THREE.Matrix4().elements);
    assert.equal(mesh.skeleton, skeleton);
  }
  // Hiding by walking the drum, as Gunner.hideRig does, reaches both.
  let found = 0;
  byName.drum.traverse((o) => { if (o.isMesh) found++; });
  assert.equal(found, 2);
  // Posed, the drum batch's vertices follow the drum: world = drumWorld x bindInverse x p.
  byName.drum.rotation.y = Math.PI / 2;
  root.updateMatrixWorld(true);
  skeleton.update();
  const drumMesh = under('drum');
  const skin = drumMesh.geometry.getAttribute('skinIndex');
  assert.equal(skin.getX(0), 1);
  const bone = new THREE.Matrix4().fromArray(skeleton.boneMatrices, 16);
  const p = new THREE.Vector3(1, 1, 0); // the drum's part vertex (1,0,0) in bind pose, baked at y+1
  const world = p.clone().applyMatrix4(drumMesh.bindMatrix).applyMatrix4(bone).applyMatrix4(drumMesh.bindMatrixInverse).applyMatrix4(drumMesh.matrixWorld);
  assert.ok(world.distanceTo(new THREE.Vector3(0, 1, -1)) < 1e-6, `turned with the drum: ${world.toArray()}`);
});
