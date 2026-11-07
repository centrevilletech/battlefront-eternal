// These tests inspect full-screen step packing rather than rasterize pixels.
// Clouds, tint and fog each have independent target/sampler state even though
// they use the same Rust screen-step boundary. A valid step must preserve that
// state without a native first draw. Driver smoke supplies actual GL coverage.
// The stand-ins expose active uniforms explicitly so a missing sampler cannot
// pass merely because a generated shader happened to optimise it away.
import test from "node:test";
import { META as M, ROLE as R } from "../src/rust-render/frame-layout.js";
import assert from "node:assert/strict";
import * as THREE from "three";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");
const { standinModule, standinThree, fakeProgram, GL } = await import("./support/rust-standin.mjs");
const { ownedEffectMaterial } = await import("../src/rust-render/effect-material.js");
const { bindProgram } = await import("../src/rust-render/programs.js");

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
  core.sceneItems = core.renderer.renderLists?.get(core.graphics.scene, 0);
  const item = (object) => ({ object, material: object.material, group: null, groupOrder: 0, renderOrder: object.renderOrder, z: 0, id: object.id });
  return { core, calls, ...three, background, material, item };
}

// The bridge for a full-screen pass three renders on its own, with a
// context that knows 3D samplers.
function screenBridge() {
  const { BfeRenderer, memory, calls } = standinModule();
  const { standinContext } = support;
  const three = standinThree(calls, { context: { ...standinContext(calls), SAMPLER_3D: 0x8b5f } });
  three.renderer.autoClear = false;
  const core = new RustRender(three.graphics, { memory }, BfeRenderer);
  core.sceneItems = core.renderer.renderLists?.get(core.graphics.scene, 0);
  return { core, calls, ...three };
}
const support = await import("./support/rust-standin.mjs");

// Cloud integration uses a 3D noise sampler and its own render target.
// The frame record must bind the volume texture and all camera/material values.
// Making a plain program without those values would still link but draw wrong.
// No native render call is used to prepare the material or populate its target.
// The fixture checks exact state flags as well as the emitted screen draw.
test("the storm's cloud panorama: drawn into its target by the renderer from its catalog program, its noise volume a 3D sampler", async () => {
  const { core, calls, renderer, setTarget } = screenBridge();
  const { ownedFullScreenPass } = await import("../src/render-passes.js");
  const noise = new THREE.Data3DTexture(new Uint8Array(8), 2, 2, 2);
  const pass = ownedFullScreenPass("effects/storm-clouds", { volumeNoise: { value: noise }, wind: { value: new THREE.Vector2(1, 2) }, eyePosition: { value: new THREE.Vector3(3, 4, 5) },
    hazeColor: { value: new THREE.Vector3(0.1, 0.2, 0.3) }, time: { value: 7 } });
  const material = pass.material;
  const target = { texture: new THREE.Texture(), width: 768, height: 384 }, camera = new THREE.Camera();
  setTarget(target);
  calls.length = 0;
  assert.equal(core.cover("skyClouds", renderer, pass, camera), true);
  assert.ok(calls.some((c) => c[0] === "post_target"), "its target, by its texture");
  assert.ok(calls.some(c=>c[0]==="frame_aux_frame"));
  assert.equal(core.frameMeta[R.CLOUDS*M.LEN+M.STEPS],1);
  core.views();
  const step = core.postInput;
  assert.deepEqual([step[2], step[3], step[4]], [768, 384, 5], "its size, five uniforms");
  // The catalog orders the uniforms: wind, eye, haze, time, then the noise volume.
  assert.equal(step[5 + 4 * 18 + 1], 33, "a 3D sampler");
  assert.ok(step[5 + 4 * 18 + 2] >= 0, "the noise volume, registered");
  assert.deepEqual([...step.subarray(5 + 1, 5 + 4)], [2, 1, 2], "the wind");
  assert.deepEqual([...core.postStates.subarray(0, 2)], [0, 0], "a full-screen pass's state");
  renderer.autoClear = true;
  assert.equal(core.cover("skyClouds", renderer, pass, camera),true,"native screen policy does not consult autoClear");
});

// Tint draws over the completed canvas with the authored separate-alpha blend.
// Its target and viewport differ from the scene's retained offscreen buffer.
// Depth testing/writing must stay off even if the previous world phase used it.
// The known material colour is packed as a vec4 rather than inherited state.
// The expected blend factors are checked at the shared-state boundary.
test("the Low water's underwater tint: over the finished frame, blended as three blends it", () => {
  const { core, calls, renderer, setTarget } = screenBridge();
  const material = ownedEffectMaterial("effects/water-underwater-tint", { uniforms: { color: { value: new THREE.Vector4(0.1, 0.2, 0.3, 0.4) } }, transparent: true, depthTest: false, depthWrite: false, visible: true });
  const tint = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), material);
  tint.frustumCulled = false;
  const scene = new THREE.Scene();
  scene.add(tint);
  setTarget(null);
  calls.length = 0;
  assert.equal(core.cover("underwater", renderer, scene, new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1)), true);
  assert.ok(calls.some(c=>c[0]==="frame_aux_frame"));
  assert.equal(core.frameMeta[R.UNDERWATER*M.LEN+M.STEPS],1);
  core.views();
  assert.deepEqual([...core.postInput.subarray(1, 4)], [-1, 800, 600], "the canvas, three's viewport in pixels");
  // Blended (source alpha, one minus source alpha; one, one minus source alpha), no depth.
  assert.deepEqual([...core.postStates.subarray(0, 8)], [4, 0, 0x8006, 0x8006, 0x0302, 0x0303, 1, 0x0303]);
  });

// Fog first samples the completed scene and then composites into its target.
// Disabling the effect omits that authored pass; it cannot select a native draw.
// The texture copy and shader draw are separate ordered resource operations.
// An old target alias must not survive a resize or overwrite the scene early.
// The fixture checks claimed ownership and required draw submission together.
test("underwater fog publishes its screen packet without a transparent draw callback", () => {
  const {core,renderer,scene,graphics,camera,calls}=bridge();
  const fogMaterial=bindProgram(new THREE.Material(),"effects/water-underwater-fog-s0");
  fogMaterial.uniforms={cameraRange:{value:new THREE.Vector2(.1,100)},tint:{value:new THREE.Color()},time:{value:0},sceneColor:{value:new THREE.Texture()},sceneDepth:{value:new THREE.Texture()}};fogMaterial.depthTest=false;fogMaterial.depthWrite=false;
  const fog=new THREE.Mesh(new THREE.PlaneGeometry(2,2),fogMaterial);scene.add(fog);scene.userData.water={underwaterFog:fog,captureSurface(){calls.push(["fog-copy-fact"]);}};
  const pass=core.byName.fog;
  pass.place(renderer,scene,new THREE.PerspectiveCamera(),null);assert.equal(fogMaterial.visible,true);
  pass.place(renderer,scene,camera,graphics.sceneTarget);assert.equal(fogMaterial.visible,true);
  calls.length=0;core.frame.begin(true);pass.draw({renderer,scene,camera});
  assert.equal(calls.filter(c=>c[0]==="fog-copy-fact").length,1);
  assert.equal(calls.some(c=>c[0]==="frame_aux_frame"),false);
  assert.equal(calls.filter(c=>c[0]==="frame_publish_frame").length,1);
  assert.equal(core.byName.transparent.afterPhase,undefined);
  assert.equal(renderer.properties.get(fogMaterial).currentProgram,undefined);
});
