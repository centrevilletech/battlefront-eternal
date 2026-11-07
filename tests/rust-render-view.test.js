// First-person rigid parts use the same native material rows as world parts.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { RustRender } from '../src/rust-render/index.js';
import { actorMaterial } from '../src/materials.js';
import { standinModule, standinThree } from './support/rust-standin.mjs';

globalThis.location ??= { search: '' };

test('a scrolling repair-tool part registers and publishes its native view row', () => {
  const { BfeRenderer, memory, calls } = standinModule();
  const { graphics } = standinThree(calls);
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  const material = actorMaterial('dynamic', { map: new THREE.Texture(), scroll: [0.25, -0.5] });
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(), material);
  mesh.geometry.clearGroups();
  const pass = core.byName.view;
  const record = pass.registerPart(mesh);
  assert.ok(record, 'the scrolling rigid part is registered');
  pass.write(record, { records: 0 });
  const extra = core.recordsAt + 76;
  assert.deepEqual(Array.from(core.input.subarray(extra, extra + 2)), [0.25, -0.5]);
  material.uniforms.scroll.value.set(-1, 2);
  pass.write(record, { records: 0 });
  assert.deepEqual(Array.from(core.input.subarray(extra, extra + 2)), [-1, 2]);
  delete material.uniforms.scroll;
  pass.write(record, { records: 0 });
  assert.deepEqual(Array.from(core.input.subarray(extra, extra + 2)), [0, 0], 'a reused row clears the previous material scroll');
});
