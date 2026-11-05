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
  const water = core.byName.water, prepare = water.draw;
  water.draw = function (...args) {
    assert.ok(calls.some((c) => c[0] === "screen_frame"), "the opaque copy includes the far image before water prepares its draw");
    return prepare.apply(this, args);
  };
  render(b);
  water.draw = prepare;
  assert.equal(material.visible, false);
  const frame = calls.filter((c) => c[0].endsWith("frame"));
  assert.deepEqual(frame.map((c) => c[0]), ["frame", "screen_frame", "transparent_frame"]);
  assert.deepEqual(frame[1].slice(1), [1], "the image's one step");
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
  assert.deepEqual(calls.find((c) => c[0] === "mirror_transparent_frame").slice(1), [0, 0]);
  // Off: three draws it again.
  core.setActive(false);
  assert.equal(material.visible, true);
  render(b);
  assert.equal(material.visible, true);
  core.setActive(true);
  calls.length = 0;
  render(b);
  assert.equal(material.visible, false);
  assert.deepEqual(calls.find((c) => c[0] === "transparent_frame").slice(1), [0, 0]);
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
  assert.deepEqual(calls.find((c) => c[0] === "transparent_frame").slice(1), [0, 0]);
});

// The bridge for a full-screen pass three renders on its own, with a
// context that knows 3D samplers.
function screenBridge() {
  const { BfeRenderer, memory, calls } = standinModule();
  const { standinContext } = support;
  const three = standinThree(calls, { context: { ...standinContext(calls), SAMPLER_3D: 0x8b5f } });
  three.renderer.autoClear = false;
  const core = new RustRender(three.graphics, { memory }, BfeRenderer);
  return { core, calls, ...three };
}
const support = await import("./support/rust-standin.mjs");

test("the storm's cloud panorama: drawn into its target by the renderer, its noise volume a 3D sampler, once three made its program", () => {
  const { core, calls, renderer, setTarget } = screenBridge();
  const noise = new THREE.Data3DTexture(new Uint8Array(8), 2, 2, 2);
  const material = new THREE.ShaderMaterial({ uniforms: { volumeNoise: { value: noise }, wind: { value: new THREE.Vector2(1, 2) }, eyePosition: { value: new THREE.Vector3(3, 4, 5) },
    hazeColor: { value: new THREE.Vector3(0.1, 0.2, 0.3) }, time: { value: 7 } }, depthTest: false, depthWrite: false, blending: THREE.NoBlending });
  const pass = new THREE.Mesh(new THREE.BufferGeometry(), material);
  pass.frustumCulled = false;
  const target = { texture: new THREE.Texture(), width: 768, height: 384 }, camera = new THREE.Camera();
  setTarget(target);
  assert.equal(core.cover("skyClouds", renderer, pass, camera), false, "three draws it until it has made its program");
  renderer.properties.get(material).currentProgram = fakeProgram([["volumeNoise", 0x8b5f], ["wind", GL.FLOAT_VEC2], ["eyePosition", GL.FLOAT_VEC3], ["hazeColor", GL.FLOAT_VEC3], ["time", GL.FLOAT]]);
  calls.length = 0;
  assert.equal(core.cover("skyClouds", renderer, pass, camera), true);
  assert.ok(calls.some((c) => c[0] === "post_target"), "its target, by its texture");
  assert.deepEqual(calls.find((c) => c[0] === "screen_frame"), ["screen_frame", 1]);
  core.views();
  const step = core.postInput;
  assert.deepEqual([step[2], step[3], step[4]], [768, 384, 5], "its size, five uniforms");
  assert.equal(step[5 + 1], 33, "a 3D sampler");
  assert.ok(step[5 + 2] >= 0, "the noise volume, registered");
  assert.deepEqual([...step.subarray(5 + 18 + 1, 5 + 18 + 4)], [2, 1, 2], "the wind");
  assert.deepEqual([...core.postStates.subarray(0, 2)], [0, 0], "a full-screen pass's state");
  renderer.autoClear = true;
  assert.equal(core.cover("skyClouds", renderer, pass, camera), false, "three's clear first: three's");
});

test("the Low water's underwater tint: over the finished frame, blended as three blends it", () => {
  const { core, calls, renderer, setTarget } = screenBridge();
  const material = new THREE.ShaderMaterial({ uniforms: { color: { value: new THREE.Vector4(0.1, 0.2, 0.3, 0.4) } }, transparent: true, depthTest: false, depthWrite: false });
  const tint = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), material);
  tint.frustumCulled = false;
  const scene = new THREE.Scene();
  scene.add(tint);
  renderer.properties.get(material).currentProgram = fakeProgram([["color", GL.FLOAT_VEC4]], ["position"]);
  setTarget(null);
  calls.length = 0;
  assert.equal(core.cover("underwater", renderer, scene, new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1)), true);
  assert.deepEqual(calls.find((c) => c[0] === "screen_frame"), ["screen_frame", 1]);
  core.views();
  assert.deepEqual([...core.postInput.subarray(1, 4)], [-1, 800, 600], "the canvas, three's viewport in pixels");
  // Blended (source alpha, one minus source alpha; one, one minus source alpha), no depth.
  assert.deepEqual([...core.postStates.subarray(0, 8)], [4, 0, 0x8006, 0x8006, 0x0302, 0x0303, 1, 0x0303]);
  // Off: three draws it.
  core.byName.underwater.setActive(false);
  assert.equal(core.cover("underwater", renderer, scene, new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1)), false);
});

test("underwater fog takes the completed scene before its screen draw and returns to three when disabled", () => {
  const b = bridge(), { core, renderer, scene, camera, calls, graphics } = b;
  const fog = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), new THREE.ShaderMaterial({
    uniforms: { sceneColor: { value: new THREE.Texture() } },
    transparent: true, blending: THREE.NoBlending, depthTest: false, depthWrite: false,
  }));
  fog.frustumCulled = false;
  scene.add(fog);
  scene.userData.water = { underwaterFog: fog, captureSurface() { calls.push(["fog-copy"]); } };
  renderer.properties.get(fog.material).currentProgram = fakeProgram([["sceneColor", GL.SAMPLER_2D]]);
  const pass = core.byName.fog, ctx = { renderer, scene, camera };
  pass.draw(ctx);
  pass.place(renderer, scene, camera, graphics.sceneTarget);
  assert.equal(fog.material.visible, false);
  pass.draw(ctx);
  calls.length = 0;
  core.byName.transparent.atEnd();
  assert.deepEqual(calls.filter((c) => ["fog-copy", "screen_frame"].includes(c[0])).map((c) => c[0]), ["fog-copy", "screen_frame"]);
  pass.setActive(false);
  assert.equal(fog.material.visible, true);
});
