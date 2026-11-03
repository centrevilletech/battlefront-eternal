import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");
const { standinModule, standinThree, fakeProgram, GL } = await import("./support/rust-standin.mjs");

// post.rs's step and state layouts.
const STRIDE = 5 + 12 * 18, STATE = 10;
const TARGET_BOUND = -2;

// The bridge over the stand-ins, with the far scene's image as graphics.js
// makes it: a full-screen pass drawn last of the near scene's opaque
// objects, depth tested at the far plane, writing no depth.
function bridge() {
  const { BfeRenderer, memory, calls } = standinModule();
  const material = new THREE.ShaderMaterial({ uniforms: { map: { value: new THREE.Texture() } }, depthTest: false, depthWrite: false, blending: THREE.NoBlending });
  material.depthTest = true;
  const background = new THREE.Mesh(new THREE.BufferGeometry(), material);
  background.renderOrder = 1e9;
  background.frustumCulled = false;
  const three = standinThree(calls, { graphics: { background } });
  three.scene.add(background);
  const core = new RustRender(three.graphics, { memory }, BfeRenderer);
  const item = (object) => ({ object, material: object.material, group: null, groupOrder: 0, renderOrder: object.renderOrder, z: 0, id: object.id });
  return { core, calls, ...three, background, material, item };
}

// three's render of the near scene as far as the renderer sees it: its
// start (the scene's onBeforeRender, before it lists anything), its opaque
// list (the near phase's hook first, then the image while three lists it),
// the hook's near phase, then the image's own draw by three where it is
// listed (the transparent phase is armed on it).
function render(b, camera = b.camera, target = b.graphics.sceneTarget) {
  const { core, renderer, scene, list, background, item } = b;
  b.setTarget(target);
  scene.onBeforeRender(renderer, scene, camera, target);
  list.init();
  list.opaque.push(item(core.nearAnchor));
  if (background.material.visible) list.opaque.push(item(background));
  core.nearAnchor.onAfterRender(renderer, scene, camera);
  if (background.material.visible) background.onAfterRender(renderer, scene, camera, background.geometry, background.material, null);
}

test("the far image: three's until the renderer has its program, then left out of three's list and drawn first in the transparent phase", () => {
  const b = bridge();
  const { core, calls, renderer, material } = b;
  const pass = core.byName.image;
  assert.equal(pass.phase, "transparent");
  // Three has not drawn it yet: three keeps it.
  render(b);
  assert.equal(material.visible, true);
  assert.deepEqual(calls.find((c) => c[0] === "transparent_frame").slice(1), [0, 0]);
  // Three made its program: the renderer makes its own in the transparent
  // phase (three's state reset after it, even with nothing else drawn).
  renderer.properties.get(material).currentProgram = fakeProgram([["map", GL.SAMPLER_2D]]);
  calls.length = 0;
  render(b);
  assert.equal(material.visible, true, "this render was three's already");
  assert.ok(calls.some((c) => c[0] === "add_program" && c[1] === 3), "a plain program of three's");
  assert.deepEqual(calls.find((c) => c[0] === "transparent_frame").slice(1), [0, 0]);
  assert.equal(calls.filter((c) => c[0] === "resetState").length, 2, "after the near phase and after the transparent phase that made the program");
  // From the next render: left out of three's list, drawn first in the
  // transparent phase, which follows the near phase at once (nothing of
  // three's opaque after the hook).
  calls.length = 0;
  render(b);
  assert.equal(material.visible, false);
  const frame = calls.filter((c) => c[0].endsWith("frame"));
  assert.deepEqual(frame.map((c) => c[0]), ["frame", "transparent_frame"]);
  assert.deepEqual(frame[1].slice(1), [0, 1], "the image's one step");
  core.views();
  const step = core.postInput.subarray(0, STRIDE), state = core.postStates.subarray(0, STATE);
  assert.equal(step[1], TARGET_BOUND, "into the framebuffer three is drawing");
  assert.deepEqual([...step.subarray(4, 7)], [1, 0, 32], "one uniform: the map, a sampler");
  assert.ok(step[7] >= 0, "the far target's texture, registered");
  // Depth tested less or equal, no depth written, no blending, the
  // triangle's own geometry (post.rs's is three's).
  assert.deepEqual([...state.subarray(0, 2)], [1, 0x0203]);
  assert.equal(pass.drawn, 1);
  assert.equal(pass.missed, 0);
});

test("the far image: three's in a render the near hook doesn't draw in, a mirror's drawn by the renderer, and off or restored three's", () => {
  const b = bridge();
  const { core, calls, renderer, material } = b;
  renderer.properties.get(material).currentProgram = fakeProgram([["map", GL.SAMPLER_2D]]);
  render(b);
  // Another camera's render of the near scene (the fountain's reflection):
  // no near phase there, so three draws the image.
  calls.length = 0;
  render(b, new THREE.PerspectiveCamera(), { name: "fountain" });
  assert.equal(material.visible, true);
  assert.equal(calls.filter((c) => c[0].endsWith("frame")).length, 0);
  // A water mirror's: the mirror's transparent phase draws it.
  const mirror = new THREE.PerspectiveCamera();
  mirror.userData.mirror = true;
  mirror.updateMatrixWorld(true);
  calls.length = 0;
  render(b, mirror, { name: "mirror" });
  assert.equal(material.visible, false);
  assert.deepEqual(calls.find((c) => c[0] === "mirror_transparent_frame").slice(1), [0, 1]);
  // Off: three draws it again.
  core.setActive(false);
  assert.equal(material.visible, true);
  render(b);
  assert.equal(material.visible, true);
  core.setActive(true);
  calls.length = 0;
  render(b);
  assert.equal(material.visible, false);
  assert.deepEqual(calls.find((c) => c[0] === "transparent_frame").slice(1), [0, 1]);
  // Given back for good.
  core.restore();
  assert.equal(material.visible, true);
  render(b);
  assert.equal(material.visible, true);
});

test("the far image after three's own opaque items: the transparent phase, and the image, follow the last of them", () => {
  const b = bridge();
  const { core, calls, renderer, scene, camera, list, material, item } = b;
  renderer.properties.get(material).currentProgram = fakeProgram([["map", GL.SAMPLER_2D]]);
  render(b);
  calls.length = 0;
  scene.onBeforeRender(renderer, scene, camera, b.graphics.sceneTarget);
  assert.equal(material.visible, false);
  const body = new THREE.Mesh(new THREE.BufferGeometry(), new THREE.MeshBasicMaterial());
  list.init();
  list.opaque.push(item(core.nearAnchor), item(body));
  core.nearAnchor.onAfterRender(renderer, scene, camera);
  assert.deepEqual(calls.filter((c) => c[0].endsWith("frame")).map((c) => c[0]), ["frame"], "not before three's last opaque item");
  body.onAfterRender(renderer, scene, camera, body.geometry, body.material, null);
  assert.deepEqual(calls.find((c) => c[0] === "transparent_frame").slice(1), [0, 1]);
});
