// Motion proxies are selected and ordered separately from colour geometry.
// Their registered programs and instance/uniform rows must be current before
// Rust writes the velocity target, including the locked first-person mask.
// These fixtures check boundary records and callbacks without a GL driver;
// the generated backend test checks nonzero, repeatable motion pixels.
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");
const { motionVectors } = await import("../src/motion-vectors.js");

import { GL, fakeProgram as program, standinModule, standinThree } from "./support/rust-standin.mjs";

// The bridge over the shared stand-ins (support/rust-standin.mjs), with
// three's render logged, the motion pass's views given room.
function bridge() {
  const { BfeRenderer, memory, calls } = standinModule({ lengths: { motion_input: 106 + 256 * 88, motion_bones: 8192 * 16, motion_previous: 8192 * 16 } });
  const { renderer, graphics, properties } = standinThree(calls);
  // An override render notes whether its scene's first part was shown to three then.
  renderer.render = (scene, camera) => calls.push(["render", scene.isScene ? scene.type : scene, camera.type, scene.overrideMaterial ? scene.children[0]?.material.visible : null]);
  renderer.clear = () => calls.push(["clear"]);
  renderer.setClearColor = () => {};
  renderer.getClearColor = (c) => c.setRGB(0, 0, 0);
  renderer.shadowMap.autoUpdate = true;
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  core.sceneItems = core.renderer.renderLists?.get(core.graphics.scene, 0);
  return { core, calls, renderer, graphics, properties };
}

// A soldier's stand-in: a skinned mesh of two bones (and a gun, rigid) in
// front of the camera, on the default layer.
function bodies() {
  const root = new THREE.Scene();
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]), 3));
  geometry.setAttribute("skinIndex", new THREE.Uint16BufferAttribute([0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0], 4));
  geometry.setAttribute("skinWeight", new THREE.Float32BufferAttribute([1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0], 4));
  geometry.setIndex([0, 1, 2]);
  const bones = [new THREE.Bone(), new THREE.Bone()];
  bones[0].add(bones[1]);
  bones[1].position.set(0, 1, 0);
  const body = new THREE.SkinnedMesh(geometry, new THREE.MeshBasicMaterial());
  body.add(bones[0]);
  body.bind(new THREE.Skeleton(bones));
  body.boundingSphere = new THREE.Sphere(new THREE.Vector3(), 2);
  const gunGeometry = new THREE.BufferGeometry();
  gunGeometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array([0, 0, 0, 0.5, 0, 0, 0, 0.5, 0]), 3));
  gunGeometry.setIndex([0, 1, 2]);
  const gun = new THREE.Mesh(gunGeometry, new THREE.MeshBasicMaterial());
  gun.position.set(0.5, 0, 0);
  root.add(body, gun);
  root.updateMatrixWorld(true);
  body.skeleton.update();
  return { root, body, gun };
}

function cameras() {
  const camera = new THREE.PerspectiveCamera(60, 2, 0.5, 200);
  camera.position.set(0, 0.5, 5);
  camera.updateMatrixWorld(true);
  const view = new THREE.PerspectiveCamera(60, 2, 0.3, 10);
  view.position.copy(camera.position);
  view.updateMatrixWorld(true);
  return { camera, view };
}

// The programs three made for a skinned proxy, a rigid one and the locked
// mask (their active uniforms and attributes).
function programs(properties, proxies, locked) {
  const proxyUniforms = [["modelMatrix", GL.FLOAT_MAT4], ["modelViewMatrix", GL.FLOAT_MAT4], ["projectionMatrix", GL.FLOAT_MAT4], ["currentViewProjection", GL.FLOAT_MAT4],
    ["previousViewProjection", GL.FLOAT_MAT4], ["previousModel", GL.FLOAT_MAT4], ["sceneDepth", GL.SAMPLER_2D], ["depthRange", GL.FLOAT_VEC2]];
  const skinnedUniforms = [...proxyUniforms, ["bindMatrix", GL.FLOAT_MAT4], ["bindMatrixInverse", GL.FLOAT_MAT4], ["boneTexture", GL.SAMPLER_2D], ["previousBindInverse", GL.FLOAT_MAT4], ["previousBones", GL.SAMPLER_2D]];
  for (const proxy of proxies) {
    properties.get(proxy.material).currentProgram = proxy.isSkinnedMesh ? program(skinnedUniforms, ["position", "skinIndex", "skinWeight"]) : program(proxyUniforms, ["position"]);
  }
  if (locked) properties.get(locked).currentProgram = program([["modelViewMatrix", GL.FLOAT_MAT4], ["projectionMatrix", GL.FLOAT_MAT4]], ["position"]);
}

// Colour ownership is not enough for motion: proxies have different programs,
// previous transforms and first-person masking rules. Match their scene order
// before packing Rust rows, including callbacks that update proxy uniforms.
// This fixture exposes linked attributes explicitly to reject missing bindings.
// A locked mask must not inherit an animated object's previous matrix.
// The returned target belongs to the resource owner after the phase finishes.
// No native draw is available to repair an incomplete velocity submission.
// The driver fixture independently proves nonzero repeatable motion output.
test("the motion pass: the proxies three would draw, in its order, then the view's locked mask, drawn by the renderer", () => {
  const { core, calls, renderer, properties } = bridge();
  assert.equal(motionVectors.drawer, core.byName.motion, "the pass draws for motion-vectors.js");
  const { root, body, gun } = bodies();
  const { camera, view } = cameras();
  const skinned = motionVectors.track(body), rigid = motionVectors.track(gun);
  });

// A sky ship's motion mesh as sky-flyers.js makes one: a model part drawn
// instanced, each ship's matrices now and last frame and whether it shows
// as instance attributes, a material shared by every part.
function shipMotion(material, count, at) {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]), 3));
  geometry.setIndex([0, 1, 2]);
  const previous = new THREE.InstancedBufferAttribute(new Float32Array(count * 16).map((_, i) => i + 0.5), 16);
  const shown = new THREE.InstancedBufferAttribute(new Float32Array(count).fill(1), 1);
  geometry.setAttribute("previousMatrix", previous);
  geometry.setAttribute("shown", shown);
  const mesh = new THREE.InstancedMesh(geometry, material, count);
  for (let i = 0; i < count; i++) mesh.setMatrixAt(i, new THREE.Matrix4().makeTranslation(at.x + i, at.y, at.z));
  mesh.frustumCulled = false;
  mesh.updateMatrixWorld(true);
  return mesh;
}

// Instanced sky ships provide extra motion inputs outside the ordinary body
// proxy list. Their hook must run before values are copied into the Rust view.
// Instance count, layout and uniform data must describe the same ordered draw.
// The synthetic source makes those writes visible without needing ship assets.
// A zero-row phase would otherwise look harmless while dropping sky motion.
// The test retains the same program/geometry ids across repeated submissions.
// This is a motion resource contract, not a comparison with a native renderer.
// Its target state remains explicit rather than inherited from a colour pass.
test("the sky's ships' own motion: drawn by the renderer first, in three's order, their instances and uniforms written, their hook run", () => {
  const { core, calls, renderer, properties } = bridge();
  const { camera } = cameras();
  const material = new THREE.ShaderMaterial({
    uniforms: { currentViewProjection: { value: new THREE.Matrix4() }, previousViewProjection: { value: new THREE.Matrix4() }, sceneDepth: { value: null }, still: { value: 0 } },
    depthTest: false, depthWrite: false, side: THREE.DoubleSide,
  });
  // The nearer ship's part drawn first, as three sorts them (z, then id).
  const far = shipMotion(material, 3, new THREE.Vector3(0, 50, -400)), near = shipMotion(material, 2, new THREE.Vector3(0, 20, -100));
  const hooks = [];
  for (const mesh of [far, near]) { mesh.onBeforeRender = () => hooks.push(mesh); motionVectors.addExtra(mesh); }
  const target = { texture: new THREE.Texture(), width: 8, height: 4 }, depth = new THREE.DepthTexture(8, 4);
  const current = new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse), previous = current.clone();
  const root = new THREE.Scene();
  });
