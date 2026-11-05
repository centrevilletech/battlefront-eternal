import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { standinModule, standinThree, fakeProgram, GL } from './support/rust-standin.mjs';

globalThis.location ??= { search: '' };
const { RustRender } = await import('../src/rust-render/index.js');

test('the colour rig phase leaves crowd depth casters to the shadow pass', () => {
  const { BfeRenderer, memory, calls } = standinModule();
  const { graphics, renderer, scene, camera } = standinThree(calls);
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  core.sceneItems = core.renderer.renderLists?.get(core.graphics.scene, 0);
  const crowd = new THREE.SkinnedMesh(new THREE.BufferGeometry(), new THREE.MeshBasicMaterial());
  crowd.name = 'crowd-shadows';
  crowd.castShadow = true;
  crowd.bind(new THREE.Skeleton([]));
  renderer.properties.get(crowd.material).currentProgram = fakeProgram([['diffuse', GL.FLOAT_VEC3]], ['position']);
  scene.add(crowd);
  core.byName.instances.rigs(camera, { records: 0, bones: 0 });
  assert.deepEqual(core.errors, [], 'the basic depth caster must not be compiled as a colour rig');
  assert.equal(core.byName.instances.rigList.includes(crowd), false);
  assert.equal(crowd.parent, scene);
  assert.equal(crowd.castShadow, true);
  assert.equal(crowd.material.visible, true, 'shadow ownership is retained');

  const vehicle = new THREE.SkinnedMesh(new THREE.BufferGeometry(), new THREE.ShaderMaterial());
  scene.add(vehicle);
  core.byName.instances.findRigs();
  assert.equal(core.byName.instances.rigList.includes(vehicle), true, 'ordinary rigs are still discovered');
});
