import { prepareSceneFixture } from "./support/rust-standin.mjs";
// The event fixture tests ownership during loss, not GPU rasterization.
// Materials stay claimed by Rust while drawing is suspended; restore cannot
// reveal them to Three as a substitute renderer. The external texture owner
// supplies a new GL handle, but its Rust logical id must remain unchanged.
// The generated backend fixture covers actual program/buffer reconstruction.
// Both tests are needed: matching pixels alone would miss a hidden handback.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { standinModule, standinThree } from './support/rust-standin.mjs';
globalThis.location ??= { search: '' };
const { RustRender } = await import('../src/rust-render/index.js');

test('context loss suspends drawing without releasing materials, then Rust rebuilds stable handles', () => {
  const { BfeRenderer, memory, calls } = standinModule();
  const { renderer, graphics, scene } = standinThree(calls);
  graphics.warmup = () => calls.push(["warmup"]);
  const core = prepareSceneFixture(new RustRender(graphics, { memory }, BfeRenderer));
  const material = new THREE.ShaderMaterial();material.visible = false;
  scene.add(new THREE.Mesh(new THREE.BufferGeometry(), material));
  const texture = new THREE.Texture(), id = core.texture(texture);
  const oldHandle = renderer.properties.get(texture).__webglTexture;
  const lost = new Event('webglcontextlost', { cancelable: true });
  // Loss cancels native default teardown while keeping materials Rust-owned.
  // An old handback would reveal the material here before any restore event.
  renderer.domElement.dispatchEvent(lost);
  assert.equal(lost.defaultPrevented, true);
  assert.equal(core.contextLost, true);
  assert.equal(material.visible, false, "authored visibility survives loss");
  assert.deepEqual(core.errors, []);
  // The new context provides different physical handles under the same logical
  // slot. Existing frame records must not need to be rebuilt to use that slot.
  const newHandle = {};
  renderer.properties.get(texture).__webglTexture = newHandle;
  renderer.domElement.dispatchEvent(new Event('webglcontextrestored'));
  const rebuild = calls.find(c => c[0] === 'rebuild');
  assert.ok(rebuild, 'the Rust renderer rebuilds its GPU resources');
  assert.equal(rebuild[2], renderer.resources.gpu);
  assert.equal(rebuild[3][id], renderer.resources.textureId(texture));
  assert.equal(core.textures.has(oldHandle), false);
  assert.equal(core.texture(texture), id, 'logical texture identity is retained');
  assert.equal(core.contextLost, false);
  assert.equal(core.byName.hud.full, true);
});
