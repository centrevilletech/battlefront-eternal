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

test("the motion pass: the proxies three would draw, in its order, then the view's locked mask, drawn by the renderer", () => {
  const { core, calls, renderer, properties } = bridge();
  assert.equal(motionVectors.drawer, core.byName.motion, "the pass draws for motion-vectors.js");
  const { root, body, gun } = bodies();
  const { camera, view } = cameras();
  const skinned = motionVectors.track(body), rigid = motionVectors.track(gun);
  try {
    // The view scene: a part the view pass hides from three, a hidden one.
    const viewScene = new THREE.Scene();
    const part = new THREE.Mesh(gun.geometry, new THREE.MeshBasicMaterial({ visible: false }));
    const gone = new THREE.Mesh(gun.geometry, new THREE.MeshBasicMaterial({ visible: false }));
    viewScene.add(part, gone);
    viewScene.updateMatrixWorld(true);
    core.byName.view.parts.set(part, { material: part.material });
    core.byName.view.records.push({ mesh: part, material: part.material });
    const target = { texture: new THREE.Texture(), width: 8, height: 4 }, depth = new THREE.DepthTexture(8, 4);
    const current = new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse), previous = current.clone();
    // Until three has drawn a proxy of each kind and the mask: three.
    motionVectors.render(renderer, { target, root, camera, viewScene, view, depth, current, previous });
    assert.ok(calls.some((c) => c[0] === "render") && !calls.some((c) => c[0] === "motion_frame"), "three renders it");
    programs(properties, [skinned, rigid], motionVectors.locked);
    calls.length = 0;
    motionVectors.render(renderer, { target, root, camera, viewScene, view, depth, current, previous });
    assert.ok(!calls.some((c) => c[0] === "render"), "three renders none of it");
    assert.deepEqual(calls.find((c) => c[0] === "motion_frame"), ["motion_frame", 3], "two proxies and the hidden part");
    assert.deepEqual(calls.filter((c) => c[0] === "add_program").map((c) => c[1]), [9, 9, 9], "motion programs: skinned, rigid, the mask");
    const input = core.motionInput, at = 106, stride = 88;
    assert.deepEqual([...input.subarray(0, 16)], camera.projectionMatrix.elements);
    assert.deepEqual([...input.subarray(32, 48)], view.projectionMatrix.elements, "the mask's camera");
    assert.deepEqual([input[96], input[97]], [0.5, 200], "the depth range");
    assert.deepEqual([input[98], input[99], input[100]], [0, 8, 4], "the target, by its texture");
    assert.equal(input[102], 1, "cleared: no extras");
    assert.equal(input[103], 2, "one skeleton's two bones");
    // In three's order: the skinned proxy's material was made first.
    assert.deepEqual([input[at], input[at + stride], input[at + 2 * stride]], [1, 1, 2], "proxy, proxy, mask");
    assert.deepEqual([input[at + 3], input[at + 4], input[at + 5]], [0, 3, 2], "the whole index, both sides");
    assert.deepEqual([...input.subarray(at + 8, at + 24)], body.matrixWorld.elements);
    assert.deepEqual([...input.subarray(at + 40, at + 56)], skinned.bindMatrix.elements);
    assert.deepEqual([...core.motionBones.subarray(0, 32)], [...body.skeleton.boneMatrices]);
    assert.deepEqual([...input.subarray(at + 2 * stride + 8, at + 2 * stride + 24)], part.matrixWorld.elements);
    assert.equal(skinned.userData.seen, 1, "committed as three's frame would be");
    // A proxy three has not drawn yet: its kind's program.
    const second = bodies().gun;
    root.add(second);
    root.updateMatrixWorld(true);
    const fresh = motionVectors.track(second);
    calls.length = 0;
    motionVectors.render(renderer, { target, root, camera, viewScene, view, depth, current, previous });
    assert.deepEqual(calls.find((c) => c[0] === "motion_frame"), ["motion_frame", 4]);
    assert.ok(!calls.some((c) => c[0] === "render" || c[0] === "add_program"), "no program made again");
    motionVectors.release(fresh);
    // The mask's program not made by three yet: three draws the frame's
    // motion, the part the view pass hides shown to its override render.
    properties.get(motionVectors.locked).currentProgram = undefined;
    calls.length = 0;
    motionVectors.render(renderer, { target, root, camera, viewScene, view, depth, current, previous });
    assert.ok(!calls.some((c) => c[0] === "motion_frame"));
    assert.deepEqual(calls.filter((c) => c[0] === "render").map((c) => c[3]), [null, true], "the proxies, then the mask with the hidden part shown");
    assert.equal(part.material.visible, false, "hidden again after");
    assert.equal(core.byName.motion.why, "mask: no program yet");
    // Off: three.
    programs(properties, [], motionVectors.locked);
    core.setActive(false);
    calls.length = 0;
    motionVectors.render(renderer, { target, root, camera, viewScene: null, view, depth, current, previous });
    assert.ok(calls.some((c) => c[0] === "render") && !calls.some((c) => c[0] === "motion_frame"));
  } finally {
    motionVectors.release(skinned);
    motionVectors.release(rigid);
    core.restore();
  }
  assert.equal(motionVectors.drawer, null, "restored: three draws");
});
