import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");
const { Bloom, ownedFullScreenPass } = await import("../src/render-passes.js");
const { Upscaler } = await import("../src/upscaler.js");

import { GL, fakeProgram as program, standinModule, standinThree } from "./support/rust-standin.mjs";

// The bridge over the shared stand-ins for the WASM renderer (every pass's
// views and calls, logged in `calls`) and for three's renderer and the
// page's graphics (support/rust-standin.mjs).
function bridge() {
  const { BfeRenderer, memory, calls } = standinModule();
  const { renderer, graphics, scene, camera, properties, setTarget } = standinThree(calls);
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  core.sceneItems = core.renderer.renderLists?.get(core.graphics.scene, 0);
  return { core, calls, gl: renderer.getContext(), graphics, renderer, scene, camera, memory, properties, setTarget };
}

// A part's geometry: one triangle, indexed, no groups.
function partGeometry() {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]), 3));
  geometry.setAttribute("normal", new THREE.BufferAttribute(new Float32Array(9), 3));
  geometry.setAttribute("uv", new THREE.BufferAttribute(new Float32Array(6), 2));
  geometry.setIndex([0, 1, 2]);
  return geometry;
}

// A zero material's stand-in: a rigid part (DYNAMIC) with its look.
function partMaterial(props) {
  const material = new THREE.ShaderMaterial({ defines: { DYNAMIC: "" }, uniforms: {
    map: { value: new THREE.Texture() }, sunScale: { value: 0.5 }, ambientPull: { value: new THREE.Vector2(0.1, 0.2) }, brightness: { value: 0.9 } } });
  props.get(material).currentProgram = program([["modelViewMatrix", GL.FLOAT_MAT4], ["map", GL.SAMPLER_2D], ["sunScale", GL.FLOAT]], ["position", "normal", "uv"]);
  return material;
}

test("cover: three's start of a render, the phase, and three's end when every pass of the phase drew all of the scene", () => {
  const { core, calls, renderer } = bridge();
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera();
  const child = new THREE.Object3D();
  child.position.set(1, 2, 3);
  scene.add(child);
  let before = 0, after = 0, complete = false;
  scene.onBeforeRender = () => before++;
  scene.onAfterRender = () => after++;
  core.addPass({ name: "spy", phase: "view", draw() { this.complete = complete; } });
  assert.throws(() => core.cover("view", renderer, scene, camera), /Incomplete Rust/);
  assert.equal(before, 1, "the scene's onBeforeRender (its lights packed) before the phase");
  assert.equal(child.matrixWorld.elements[12], 1, "its world matrices brought up to date first");
  assert.ok(calls.some((c) => c[0] === "view_frame"));
  assert.equal(calls.filter((c) => c[0] === "blitFramebuffer").length, 0, "three's render resolves the target");
  // Every pass of the phase complete: three is left out, and what its render
  // does as it ends is done here.
  core.byName.view.draw = function () { this.complete = true; };
  complete = true;
  calls.length = 0;
  assert.equal(core.cover("view", renderer, scene, camera), true);
  assert.equal(after, 1);
  const blit = calls.find((c) => c[0] === "blitFramebuffer");
  assert.deepEqual(blit.slice(1), [0, 0, 8, 4, 0, 0, 8, 4, GL.COLOR_BUFFER_BIT, GL.NEAREST]);
  assert.deepEqual(calls.filter((c) => c[0] === "bindFramebuffer").map((c) => c.slice(1)), [[GL.READ_FRAMEBUFFER, "ms"], [GL.DRAW_FRAMEBUFFER, "fb"], [GL.READ_FRAMEBUFFER, null], [GL.DRAW_FRAMEBUFFER, null], [GL.DRAW_FRAMEBUFFER, "ms"]]);
  complete = false;
  assert.throws(() => core.cover("view", renderer, scene, camera), /Incomplete Rust/);
  complete = true;
  });

test("a mirror's render of the near scene draws the near phase's passes for the mirror camera", () => {
  const { core, calls, renderer, scene, setTarget } = bridge();
  const mirror = new THREE.PerspectiveCamera();
  mirror.updateMatrixWorld(true);
  mirror.userData.mirror = true;
  setTarget({ name: "reflection" });
  core.phase("mirror", renderer, scene, mirror);
  assert.ok(calls.some((c) => c[0] === "mirror_frame"));
  assert.ok(!calls.some((c) => c[0] === "frame"));
  calls.length = 0;
  assert.throws(() => core.coverMirror(renderer, scene, new THREE.PerspectiveCamera()), /Incomplete Rust mirror/);
});

test("foliage: Rust selects every registered card class", async () => {
  const { core, calls, renderer, scene, properties } = bridge();
  const geometry = new THREE.InstancedBufferGeometry();
  const card = (name, size) => geometry.setAttribute(name, new THREE.BufferAttribute(new Float32Array(4 * size).map((_, i) => i), size));
  for (const [name, size] of [["position", 3], ["color", 4], ["uv", 2], ["foliageCorner", 2], ["foliageShape", 4], ["foliageRate", 1]]) card(name, size);
  geometry.setIndex(new THREE.BufferAttribute(new Uint32Array([0, 2, 1, 1, 2, 3]), 1));
  const material = new THREE.MeshBasicMaterial({ map: new THREE.Texture(), alphaTest: 0.25 });
  const batch = { geometry, material, uniforms: { foliageWiggle: { value: 0.11 } } };
  const instance = { position: new THREE.Vector3(1, 2, 3), matrix: new THREE.Matrix4().makeTranslation(1, 2, 3), tint: [0.5, 0.6, 0.7], sphere: null, cullDistance: Infinity, maxDistance: 100 };
  const kind = { grass: true, batch, instances: [instance] };
  const foliage = { kinds: new Map([["a", kind]]), time: 2.5, nearEnd: () => 200, drawer: null };
  scene.userData.foliage = foliage;
  core.phase("near", renderer, scene, core.graphics.camera);
  assert.equal(foliage.drawer, core.byName.foliage, "the near phase attaches the drawer");
  const camera = new THREE.PerspectiveCamera();
  camera.position.set(4, 5, 6);
  camera.updateMatrixWorld(true);
  foliage.drawer.select(foliage, camera);
  const added = calls.find((c) => c[0] === "foliage_class");
  assert.equal(added[3], true, "grass");
  assert.deepEqual([...added[7]], [0, 3, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12], "attribute locations: the card's, then the patch row's");
  assert.deepEqual([...added[8]].slice(0, 6).map((v) => +v.toFixed(2)), [1, 2, 3, 0.5, 0.6, 0.7]);
  assert.deepEqual([...added[8]].slice(15, 16), [100]);
  assert.deepEqual([...added[9]].slice(6, 9), [-1, Infinity, 100], "no sphere, always tested");
  assert.equal(properties.get(material).currentProgram,undefined);
  assert.ok(foliage.drawer.classes.has(kind));
  assert.ok(calls.some((c) => c[0] === "foliage_frame"));
  const input = core.foliageInput;
  assert.deepEqual([input[48], input[49], input[50], input[51], input[52]], [4, 5, 6, 200, 2.5]);
  });

test("foliage under shadows: a card program that reads modelMatrix (three's world position for USE_SHADOWMAP) is taken", () => {
  const { core, calls, renderer, scene, properties } = bridge();
  core.graphics.csm={};
  const geometry = new THREE.InstancedBufferGeometry();
  for (const [name, size] of [["position", 3], ["color", 4], ["uv", 2], ["foliageCorner", 2], ["foliageShape", 4], ["foliageRate", 1]]) geometry.setAttribute(name, new THREE.BufferAttribute(new Float32Array(4 * size), size));
  geometry.setIndex(new THREE.BufferAttribute(new Uint32Array([0, 2, 1, 1, 2, 3]), 1));
  const material = new THREE.MeshBasicMaterial({ map: new THREE.Texture() });
  const kind = { grass: true, batch: { geometry, material, uniforms: { foliageWiggle: { value: 0 } } }, instances: [] };
  const foliage = { kinds: new Map([["a", kind]]), time: 0, nearEnd: () => 200, drawer: null };
  scene.userData.foliage = foliage;
  core.phase("near", renderer, scene, core.graphics.camera);
  foliage.drawer.select(foliage, core.graphics.camera);
  assert.ok(calls.some((c) => c[0] === "foliage_class"), "registered");
  assert.ok(foliage.drawer.classes.has(kind));
});

test("the view scene: three's start of a render, the phase, and three left out when every part was drawn", () => {
  const { core, calls, renderer, properties } = bridge();
  const viewScene = new THREE.Scene(), view = new THREE.PerspectiveCamera();
  view.updateMatrixWorld(true);
  const hand = new THREE.Group();
  const material = partMaterial(properties);
  const mesh = new THREE.Mesh(partGeometry(), material);
  hand.add(mesh);
  viewScene.add(hand);
  let before = 0;
  viewScene.onBeforeRender = () => before++;
  assert.equal(core.cover("view", renderer, viewScene, view), true);
  assert.equal(before, 1, "the scene's onBeforeRender (its lights packed) before the phase");
  assert.equal(material.visible, false, "the part is hidden from three");
  const frame = calls.find((c) => c[0] === "view_frame");
  assert.deepEqual(frame, ["view_frame", 1], "one record");
  const o = core.recordsAt;
  assert.equal(core.input[o], 2, "a rigid record");
  assert.deepEqual([core.input[o + 52], core.input[o + 53], core.input[o + 54], core.input[o + 55]].map((v) => +v.toFixed(3)), [0.5, 0.1, 0.2, 0.9]);
  // three's resolve of the multisampled target, as its render ends.
  const blit = calls.find((c) => c[0] === "blitFramebuffer");
  assert.deepEqual(blit.slice(1), [0, 0, 8, 4, 0, 0, 8, 4, GL.COLOR_BUFFER_BIT, GL.NEAREST]);
  assert.deepEqual(calls.filter((c) => c[0] === "bindFramebuffer").map((c) => c.slice(1)), [[GL.READ_FRAMEBUFFER, "ms"], [GL.DRAW_FRAMEBUFFER, "fb"], [GL.READ_FRAMEBUFFER, null], [GL.DRAW_FRAMEBUFFER, null], [GL.DRAW_FRAMEBUFFER, "ms"]]);
});

test("the view scene: a part the renderer can't draw leaves three to render the scene, without the parts it drew", () => {
  const { core, calls, renderer, properties } = bridge();
  const viewScene = new THREE.Scene(), view = new THREE.PerspectiveCamera();
  const part = new THREE.Mesh(partGeometry(), partMaterial(properties));
  const flash = new THREE.Mesh(new THREE.BoxGeometry(), new THREE.MeshBasicMaterial({ transparent: true }));
  properties.get(flash.material).currentProgram = program([["map", GL.SAMPLER_2D]]);
  viewScene.add(part, flash);
  assert.throws(() => core.cover("view", renderer, viewScene, view), /Incomplete Rust/);
  assert.equal(part.material.visible, false);
  assert.equal(flash.material.visible, true);
  assert.equal(calls.filter((c) => c[0] === "blitFramebuffer").length, 0, "three's render resolves the target");
  // Hidden objects, and their subtrees, are not three's to draw.
  flash.visible = false;
  assert.equal(core.cover("view", renderer, viewScene, view), true);
  });

test("post uses declared Rust programs and initializes targets without a three draw", () => {
  const { core, calls, renderer, graphics, properties } = bridge();
  const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  const postTarget = { width: 8, height: 4, texture: new THREE.Texture() };
  properties.get(postTarget.texture).__webglTexture = undefined;
  renderer.initRenderTarget = (target) => { calls.push(["initRenderTarget", target]); properties.get(target.texture).__webglTexture ||= {}; };
  const composite = ownedFullScreenPass("post-hud/composite", {
    map: { value: graphics.farTarget.texture }, blurred: { value: null }, glow: { value: null },
    blend: { value: .3 }, bloomStrength: { value: 0 }, speedBlur: { value: 0 }, speedTurn: { value: 0 },
  });
  const fxaa = ownedFullScreenPass("post-hud/fxaa", { tDiffuse: { value: postTarget.texture }, resolution: { value: new THREE.Vector2(1 / 8, 1 / 4) } });
  assert.equal(core.post(renderer, [{ pass: composite, target: postTarget, camera }, { pass: fxaa, target: null, camera }]), true);
  assert.deepEqual(calls.filter(c => c[0] === "add_owned_program").map(c => c[1]), ["post-hud/composite", "post-hud/fxaa"]);
  assert.equal(calls.filter(c => c[0] === "add_program").length, 0);
  assert.equal(calls.filter(c => c[0] === "initRenderTarget").length, 2);
  const input = core.postInput, step = 5 + 12 * 18;

  assert.deepEqual([...input.subarray(0, 5)], [0, 0, 8, 4, 7]);
  assert.deepEqual([...input.subarray(5 + 2 * 18, 5 + 2 * 18 + 3)], [2, 32, -1]);
  assert.deepEqual([...input.subarray(5 + 3 * 18, 5 + 3 * 18 + 3)], [3, 1, .3]);
  assert.deepEqual([...input.subarray(step, step + 5)], [1, -1, 800, 600, 4]);
  assert.deepEqual([...input.subarray(step + 5 + 3 * 18, step + 5 + 3 * 18 + 4)], [3, 2, .125, .25]);
  assert.throws(() => core.post(renderer, [{ run: () => {} }]), /Incomplete Rust/);

});

test("the post steps' values: set on a step's material as it is written, bloom's blur drawn twice with other values", () => {
  const { core, calls, renderer, properties } = bridge();
  const programOf = (mesh) => { properties.get(mesh.material).currentProgram = program(Object.keys(mesh.material.uniforms).map((name) => [name, name === "texel" || name === "direction" ? GL.FLOAT_VEC2 : GL.SAMPLER_2D])); };
  const bloom = new Bloom(new THREE.Texture());
  bloom.resize(16, 8);
  programOf(bloom.extract);
  programOf(bloom.blur);
  const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  const steps = bloom.steps(camera);
  assert.deepEqual(steps.map((s) => s.target), [bloom.target, bloom.scratch, bloom.target], "extract, blur across, blur down");
  assert.equal(core.post(renderer, steps), true);
  assert.deepEqual(calls.find((c) => c[0] === "post_frame"), ["post_frame", 3]);
  const input = core.postInput, step = 5 + 12 * 18;
  // add_texture's ids, in the order the steps registered them: the source,
  // the target (as the extract's target), the scratch target.
  const texture = (t) => core.textures.get(properties.get(t).__webglTexture);
  const uniform = (s, k, n) => [...input.subarray(s * step + 5 + k * 18, s * step + 5 + k * 18 + n)];
  assert.deepEqual(uniform(1, 0, 3), [0, 32, texture(bloom.target.texture)], "across: the extract's target");
  assert.deepEqual(uniform(1, 1, 4), [1, 2, 1 / 4, 0], "one texel across the quarter-size target");
  assert.deepEqual(uniform(2, 0, 3), [0, 32, texture(bloom.scratch.texture)], "down: the scratch target");
  assert.deepEqual(uniform(2, 1, 4), [1, 2, 0, 1 / 2]);
  assert.equal(bloom.blur.material.uniforms.map.value, bloom.scratch.texture, "the material left as three's draws leave it");
});

test("the temporal upscaler's steps: the motion resolve, the resolve into the history, the sharpen, then its commit", () => {
  const { core, calls, renderer, properties } = bridge();
  const upscaler = new Upscaler({ extensions: { has: () => true } });
  upscaler.resize(8, 4, 16, 8);
  const types = { inverseJittered: GL.FLOAT_MAT4, currentViewProjection: GL.FLOAT_MAT4, previousViewProjection: GL.FLOAT_MAT4, inputSize: GL.FLOAT_VEC2, outputSize: GL.FLOAT_VEC2,
    jitter: GL.FLOAT_VEC2, reset: GL.FLOAT, maxWeight: GL.FLOAT, kernel: GL.FLOAT, sharpness: GL.FLOAT };
  for (const pass of [upscaler.motionPass, upscaler.resolvePass, upscaler.sharpenPass]) {
    properties.get(pass.material).currentProgram = program(Object.keys(pass.material.uniforms).map((name) => [name, types[name] ?? GL.SAMPLER_2D]));
  }
  upscaler.motionPass.material.uniforms.depthMap.value = new THREE.Texture();
  const image = new THREE.Texture();
  let steps = upscaler.steps(image, "temporal");
  assert.deepEqual(steps.map((s) => s.target), [upscaler.motion, upscaler.history[1], null]);
  assert.equal(core.post(renderer, steps), true);
  assert.deepEqual(calls.find((c) => c[0] === "post_frame"), ["post_frame", 3]);
  const input = core.postInput, step = 5 + 12 * 18;
  const texture = (t) => core.textures.get(properties.get(t).__webglTexture);
  const uniform = (s, name, n) => { const k = Object.keys(steps[s].pass.material.uniforms).indexOf(name); return [...input.subarray(s * step + 5 + k * 18 + 2, s * step + 5 + k * 18 + 2 + n)]; };
  assert.deepEqual(uniform(1, "colorMap", 1), [texture(image)]);
  assert.deepEqual(uniform(1, "historyMap", 1), [texture(upscaler.history[0].texture)]);
  assert.deepEqual(uniform(1, "reset", 1), [1], "no history yet");
  assert.deepEqual(uniform(2, "map", 1), [texture(upscaler.history[1].texture)], "the sharpen reads the history just resolved");
  assert.equal(upscaler.frame, 0, "the commit is graphics.js's to run");
  for (const s of steps) s.after?.();
  assert.deepEqual([upscaler.read, upscaler.valid, upscaler.frame], [1, true, 1]);
  steps = upscaler.steps(image, "temporal");
  assert.equal(steps[1].target, upscaler.history[0], "the other history written next");
  assert.equal(steps[1].values.historyMap, upscaler.history[1].texture);
  assert.equal(steps[1].values.reset, 0);
  // Spatial: one step to the screen, and no history.
  steps = upscaler.steps(image, "spatial");
  assert.deepEqual(steps.map((s) => [s.pass, s.target, s.values.map]), [[upscaler.spatialPass, null, image]]);
  assert.equal(upscaler.valid, false);
});

test("the HUD's batch: every vertex the first time, the batch's update range after, three's own copy made whole when it draws again", async () => {
  const { core, calls, renderer, properties } = bridge();
  const { Batch } = await import("../src/interface/batch.js");
  const batch = new Batch();
  const texture = new THREE.Texture();
  texture.name = "hud1";
  texture.image = { width: 1, height: 1 };
  texture.version = 1;
  const hud = { batch, scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera() };
  const m = new THREE.Matrix4();
  batch.begin();
  batch.quad(m, 0, 0, 10, 10, texture, [0, 0, 1, 1], [1, 1, 1, 1]);
  batch.quad(m, 0, 0, 20, 20, texture, [0, 0, 1, 1], [1, 1, 1, 1]);
  batch.end();
  properties.get(batch.mesh.material[0]).currentProgram = program([["projectionMatrix", GL.FLOAT_MAT4], ["modelViewMatrix", GL.FLOAT_MAT4], ["map", GL.SAMPLER_2D], ["mask", GL.SAMPLER_2D], ["masked", GL.BOOL]],
    ["position", "uv", "maskUv", "tint"]);
  assert.equal(core.coverHud(renderer, hud), true);
  assert.equal(calls.find((c) => c[0] === "hud_init")[1].length, 2048 * 6);
  assert.ok(calls.some((c) => c[0] === "initTexture" && c[1] === "hud1"), "a loaded texture goes up as three's draw would send it");
  const input = core.hudInput, IN = { GROUPS: 0, UPLOAD: 41, VIEWPORT: 43, FIRST: 45 };
  assert.deepEqual([input[IN.GROUPS], input[IN.UPLOAD], input[IN.UPLOAD + 1], input[IN.VIEWPORT], input[IN.VIEWPORT + 1]], [1, 0, 8, 800, 600]);
  assert.deepEqual([...input.subarray(IN.FIRST, IN.FIRST + 5)], [0, 12, 0, 0, 0], "one run of two quads, its texture as its mask, unmasked");
  assert.deepEqual([...core.hudPosition.subarray(12, 24)], [...batch.position.subarray(12, 24)]);
  // Next frame only the second quad changes: only it is copied.
  batch.begin();
  batch.quad(m, 0, 0, 10, 10, texture, [0, 0, 1, 1], [1, 1, 1, 1]);
  batch.quad(m, 0, 0, 30, 30, texture, [0, 0, 1, 1], [1, 1, 1, 1]);
  batch.end();
  assert.equal(core.coverHud(renderer, hud), true);
  assert.deepEqual([input[IN.UPLOAD], input[IN.UPLOAD + 1]], [4, 8]);
  });

test("water: its Low surface and rings are items of the transparent list, with three's sort key, range and uniforms", () => {
  const { core, calls, scene, camera, properties } = bridge();
  const surface = (renderOrder, uniforms, values, attributes) => {
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(12), 3));
    geometry.setAttribute("uv", new THREE.BufferAttribute(new Float32Array(8), 2));
    geometry.setIndex([0, 1, 2, 2, 1, 3]);
    const material = new THREE.ShaderMaterial({ uniforms: values, transparent: true, side: THREE.DoubleSide, forceSinglePass: true });
    properties.get(material).currentProgram = program(uniforms, attributes);
    const mesh = new THREE.Mesh(geometry, material);
    mesh.renderOrder = renderOrder;
    mesh.frustumCulled = false;
    return mesh;
  };
  const lowPass = surface(-20, [["modelMatrix", GL.FLOAT_MAT4], ["viewMatrix", GL.FLOAT_MAT4], ["projectionMatrix", GL.FLOAT_MAT4], ["cameraPosition", GL.FLOAT_VEC3], ["time", GL.FLOAT], ["mainMap", GL.SAMPLER_2D]],
    { time: { value: 12.5 }, mainMap: { value: null } });
  const rings = surface(-18, [["modelViewMatrix", GL.FLOAT_MAT4], ["projectionMatrix", GL.FLOAT_MAT4], ["map", GL.SAMPLER_2D]], { map: { value: null } }, ["position", "uv", "alpha"]);
  rings.geometry.setAttribute("alpha", new THREE.BufferAttribute(new Float32Array(4), 1));
  rings.geometry.setDrawRange(0, 0);
  const group = new THREE.Group();
  group.add(lowPass);
  scene.add(group, rings);
  scene.updateMatrixWorld(true);
  scene.userData.water = { lowPass, tintPass: null, mirrorPass: null, rings };
  camera.position.set(0, 0, 1);
  camera.updateMatrixWorld(true);
  core.phase("transparent", core.renderer, scene, camera);
  assert.equal(calls.filter((c) => c[0] === "water_surface").length, 1);
  assert.equal(calls.filter((c) => c[0] === "water_rings").length, 1);
  assert.deepEqual(calls.find((c) => c[0] === "water_surface").slice(3), [0x0203, 1 | 2 | 4, 2], "depth test, write and colour, both sides at once");
  assert.equal(lowPass.material.visible, false);
  const input = core.waterInput, SURFACE = 27 + 32 * 18 + 35;
  assert.deepEqual([...input.subarray(0, 5)], [1, 0, 0, -20, lowPass.id], "shown, program, group order, render order, id");
  assert.deepEqual([input[24], input[25], input[26]], [0, 6, 6], "the index's range, six uniforms");
  assert.deepEqual([...input.subarray(27 + 3 * 18, 27 + 3 * 18 + 5)], [3, 3, 0, 0, 1], "the camera's position");
  assert.deepEqual([...input.subarray(27 + 4 * 18, 27 + 4 * 18 + 3)], [4, 1, 12.5], "the water's own time");
  assert.equal(input[SURFACE], 0, "rings with nothing in their range: not listed");
  rings.geometry.setDrawRange(0, 6);
  rings.geometry.attributes.position.array.set([1, 2, 3], 3);
  core.phase("transparent", core.renderer, scene, camera);
  assert.deepEqual([input[SURFACE], input[SURFACE + 3]], [1, -18]);
  assert.deepEqual([...core.waterRingsPosition.subarray(3, 6)], [1, 2, 3], "every vertex of the rings the first time");
  assert.deepEqual([input[8 * SURFACE], input[8 * SURFACE + 1]], [0, 4]);
  group.visible = false;
  core.phase("transparent", core.renderer, scene, camera);
  assert.equal(input[0], 0, "a hidden parent hides it");
  });

test("water at High and Ultra: the tint and mirror passes with their waders, the opaque copy first, and the shore drawn instanced", () => {
  const { core, calls, scene, camera, properties, graphics } = bridge();
  const geometry = () => {
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.BufferAttribute(new Float32Array(12), 3));
    g.setAttribute("uv", new THREE.BufferAttribute(new Float32Array(8), 2));
    g.setIndex([0, 1, 2, 2, 1, 3]);
    return g;
  };
  const waders = { value: Array.from({ length: 8 }, (_, i) => new THREE.Vector4(i, 2 * i, 3 * i, i === 0 ? 1 : 0)) };
  const pass = (renderOrder, uniforms, values, options = {}) => {
    const material = new THREE.ShaderMaterial({ uniforms: values, transparent: true, side: THREE.DoubleSide, forceSinglePass: true, ...options });
    properties.get(material).currentProgram = program(uniforms);
    const mesh = new THREE.Mesh(geometry(), material);
    mesh.renderOrder = renderOrder;
    mesh.frustumCulled = false;
    return mesh;
  };
  const WADERS = ["waders[0]", GL.FLOAT_VEC4, 8];
  const tintPass = pass(-20, [["modelMatrix", GL.FLOAT_MAT4], ["time", GL.FLOAT], ["sceneColor", GL.SAMPLER_2D], WADERS], { time: { value: 3 }, sceneColor: { value: null }, waders }, { blending: THREE.NoBlending });
  const mirrorPass = pass(-19, [["modelMatrix", GL.FLOAT_MAT4], WADERS, ["reflection", GL.SAMPLER_2D]], { waders, reflection: { value: null } });
  // The shore: one quad, instanced, two materials.
  const shoreGeometry = new THREE.InstancedBufferGeometry();
  const plane = geometry();
  shoreGeometry.index = plane.index;
  shoreGeometry.attributes.position = plane.attributes.position;
  shoreGeometry.attributes.uv = plane.attributes.uv;
  const shore = new THREE.InstancedBufferAttribute(new Float32Array(4 * 4), 4), phase = new THREE.InstancedBufferAttribute(new Float32Array(4), 1);
  shoreGeometry.setAttribute("shore", shore);
  shoreGeometry.setAttribute("phase", phase);
  shoreGeometry.instanceCount = 0;
  const shoreMeshes = [0, 1].map((splash) => {
    const material = new THREE.ShaderMaterial({ uniforms: { splash: { value: splash } }, transparent: true, depthWrite: false, side: THREE.DoubleSide, forceSinglePass: true });
    properties.get(material).currentProgram = program([["splash", GL.FLOAT]], ["position", "uv", "shore", "phase"]);
    const mesh = new THREE.Mesh(shoreGeometry, material);
    mesh.renderOrder = -17;
    mesh.frustumCulled = false;
    return mesh;
  });
  const group = new THREE.Group();
  group.add(tintPass, mirrorPass, ...shoreMeshes);
  scene.add(group);
  scene.updateMatrixWorld(true);
  // The tint pass's onBeforeRender copies the opaque scene (water.js captureSurface).
  const captures = [];
  tintPass.onBeforeRender = (renderer, s, cam) => { captures.push(cam); tintPass.material.uniforms.time.value = 4; };
  scene.userData.water = { lowPass: null, tintPass, mirrorPass, rings: null, shore: { meshes: shoreMeshes } };
  core.phase("transparent", core.renderer, scene, camera);
  assert.equal(calls.filter((c) => c[0] === "water_surface").length, 4);
  assert.equal(captures.length, 1, "the opaque scene copied once, before the list");
  const input = core.waterInput, SURFACE = 27 + 32 * 18 + 35, ARRAY = 27 + 32 * 18;
  assert.deepEqual([...input.subarray(27 + 18, 27 + 18 + 3)], [1, 1, 4], "the tint's time as the copy left it");
  assert.deepEqual([input[ARRAY], input[ARRAY + 1], input[ARRAY + 2]], [3, 4, 8], "the waders: after its three uniforms, vec4 x 8");
  assert.deepEqual([...input.subarray(ARRAY + 3, ARRAY + 3 + 8)], [0, 0, 0, 1, 1, 2, 3, 0]);
  assert.deepEqual([input[SURFACE + ARRAY], input[SURFACE + ARRAY + 3 + 4]], [2, 1], "the mirror pass's waders after its two uniforms");
  assert.equal(tintPass.material.visible, false);
  // The shore: one set for its geometry, both meshes reading it.
  assert.deepEqual(calls.filter((c) => c[0] === "water_set").map((c) => [c[1], [...c[2]]]), [[4, [4, 1]]]);
  assert.deepEqual(calls.filter((c) => c[0] === "water_instanced").map((c) => c.slice(2)), [[0, 0, 2], [0, 1, 3], [0, 0, 2], [0, 1, 3]]);
  const SETS = 8 * SURFACE + 2;
  assert.deepEqual([input[SETS], input[SETS + 1]], [1, 0], "its values sent the first time, no instances");
  shore.array.set([1, 2, 3, 4], 4);
  shore.needsUpdate = true;
  shoreGeometry.instanceCount = 2;
  input[SETS] = 0;
  core.phase("transparent", core.renderer, scene, camera);
  assert.deepEqual([input[SETS], input[SETS + 1]], [1, 2], "sent again when three would upload them");
  assert.deepEqual([...core.waterInstances.subarray(4, 8)], [1, 2, 3, 4]);
  assert.equal(captures.length, 2);
  input[SETS] = 0;
  core.phase("transparent", core.renderer, scene, camera);
  assert.deepEqual([input[SETS], input[SETS + 1]], [0, 2], "unchanged: not sent");
  // Hidden (the eye under the surface, a mirror's render): no copy.
  group.visible = false;
  core.phase("transparent", core.renderer, scene, camera);
  assert.equal(captures.length, 3, "no copy with the tint pass hidden");
  assert.equal(input[0], 0);
  assert.equal(graphics.scene, scene);
});

test("an error in a pass stops rendering with the GPU in the error", () => {
 const {core,renderer,scene,camera,calls}=bridge();
 const material=new THREE.ShaderMaterial();material.visible=false;scene.add(new THREE.Mesh(new THREE.BufferGeometry(),material));
 renderer.compile = () => {};
 core.addPass({name:"broken",phase:"view",draw(){throw Error("broken pass");}});
 assert.throws(()=>core.cover("view",renderer,scene,camera), /Battle renderer failed on unknown GPU: broken pass/);
 assert.equal(material.visible,false);
 assert.equal(core.failure.rendererFailure,true);
 assert.equal(calls.some(c=>c[0]==="resetState"),false,"a failed frame is not continued by three");
});

test("the view scene's muzzle flash: a blended basic draw after the parts, in three's order, both sides in turn", () => {
  const { core, calls, renderer, properties } = bridge();
  const viewScene = new THREE.Scene(), view = new THREE.PerspectiveCamera();
  view.updateMatrixWorld(true);
  const part = new THREE.Mesh(partGeometry(), partMaterial(properties));
  const flashMaterial = new THREE.MeshBasicMaterial({ map: new THREE.Texture(), transparent: true, blending: THREE.AdditiveBlending, depthWrite: false, side: THREE.DoubleSide });
  properties.get(flashMaterial).currentProgram = program([["projectionMatrix", GL.FLOAT_MAT4], ["modelViewMatrix", GL.FLOAT_MAT4], ["diffuse", GL.FLOAT_VEC3], ["opacity", GL.FLOAT], ["map", GL.SAMPLER_2D], ["mapTransform", GL.FLOAT_MAT3]], ["position", "uv"]);
  flashMaterial.color.setRGB(1, 0.5, 0.25);
  flashMaterial.opacity = 0.75;
  const flash = new THREE.Group();
  flash.add(new THREE.Mesh(partGeometry(), flashMaterial));
  flash.children[0].position.z = -1;
  viewScene.add(part, flash);
  viewScene.updateMatrixWorld(true);
  assert.equal(core.cover("view", renderer, viewScene, view), true, "the parts and the flash: three left out");
  assert.equal(flashMaterial.visible, false);
  assert.ok(calls.some((c) => c[0] === "add_program" && c[1] === 3 && c[4] === "position:0,uv:1"), "three's attributes at 0, 1");
  const input = core.viewInput, D = 1;
  assert.equal(input[0], 1, "one blended draw");
  assert.deepEqual([...input.subarray(D + 2, D + 9)], [1, 0x8006, 0x8006, 0x0302, 1, 0x0302, 1], "additive blending");
  assert.deepEqual([input[D + 10], input[D + 11], input[D + 12], input[D + 13]], [1 | 4, 2, 1, 0], "depth tested, not written; back faces, then front");
  assert.deepEqual([input[D + 14], input[D + 15], input[D + 16]], [0, 3, 6]);
  assert.deepEqual([...input.subarray(D + 17 + 2 * 18, D + 17 + 2 * 18 + 5)], [2, 3, 1, 0.5, 0.25], "its colour as three's refreshUniforms sends it");
  assert.deepEqual([...input.subarray(D + 17 + 3 * 18, D + 17 + 3 * 18 + 3)], [3, 1, 0.75]);
  // A flash with a hooked shader stays three's: three renders the scene.
  const hooked = new THREE.MeshBasicMaterial({ transparent: true });
  hooked.onBeforeCompile = () => {};
  properties.get(hooked).currentProgram = program([["diffuse", GL.FLOAT_VEC3]], ["position"]);
  viewScene.add(new THREE.Mesh(partGeometry(), hooked));
  assert.throws(() => core.cover("view", renderer, viewScene, view), /Incomplete Rust/);
});

test("the view scene: an unlit part (a sight whose texture scrolls) is drawn as three compiled it, opaque, before what blends", () => {
  const { core, renderer, properties } = bridge();
  const viewScene = new THREE.Scene(), view = new THREE.PerspectiveCamera();
  view.updateMatrixWorld(true);
  const sight = new THREE.ShaderMaterial({ defines: { UNLIT: "", SCROLL: "" }, uniforms: { map: { value: new THREE.Texture() }, scroll: { value: new THREE.Vector2(0.5, 0) }, time: { value: 3 } } });
  properties.get(sight).currentProgram = program([["projectionMatrix", GL.FLOAT_MAT4], ["modelViewMatrix", GL.FLOAT_MAT4], ["map", GL.SAMPLER_2D], ["scroll", GL.FLOAT_VEC2], ["time", GL.FLOAT]], ["position", "uv"]);
  const flash = new THREE.MeshBasicMaterial({ transparent: true, blending: THREE.AdditiveBlending });
  properties.get(flash).currentProgram = program([["diffuse", GL.FLOAT_VEC3]], ["position"]);
  viewScene.add(new THREE.Mesh(partGeometry(), flash), new THREE.Mesh(partGeometry(), sight), new THREE.Mesh(partGeometry(), partMaterial(properties)));
  viewScene.updateMatrixWorld(true);
  assert.equal(core.cover("view", renderer, viewScene, view), true);
  assert.equal(sight.visible, false);
  const input = core.viewInput, DRAW = 17 + 32 * 18;
  assert.equal(input[0], 2);
  assert.deepEqual([input[1 + 2], input[1 + 10], input[1 + 11], input[1 + 12]], [0, 1 | 2 | 4, 1, 0], "the sight first: no blending, depth tested and written, front faces");
  assert.deepEqual([...input.subarray(1 + 17 + 3 * 18, 1 + 17 + 3 * 18 + 4)], [3, 2, 0.5, 0], "its scroll");
  assert.deepEqual([...input.subarray(1 + 17 + 4 * 18, 1 + 17 + 4 * 18 + 3)], [4, 1, 3], "the material's own time");
  assert.equal(input[1 + DRAW + 2], 1, "then the flash, blended");
});

test("the view scene: an unlit part's point lights, which it reads for nothing it draws, are left at zero", () => {
  const { core, calls, renderer, properties } = bridge();
  const viewScene = new THREE.Scene(), view = new THREE.PerspectiveCamera();
  view.updateMatrixWorld(true);
  const sight = new THREE.ShaderMaterial({ defines: { UNLIT: "" }, uniforms: { map: { value: new THREE.Texture() }, zeroLights: { value: new Float32Array(8) }, zeroLightCount: { value: 1 } } });
  const zeroLights = program([["modelViewMatrix", GL.FLOAT_MAT4], ["normalMatrix", GL.FLOAT_MAT3], ["map", GL.SAMPLER_2D], ["zeroLights", GL.FLOAT_VEC4], ["zeroLightCount", GL.INT]], ["position", "uv"]);
  // An array: three's active uniform for it reports its length.
  const getActiveUniform = renderer.getContext().getActiveUniform;
  renderer.getContext().getActiveUniform = (p, i) => ({ ...getActiveUniform(p, i), size: p.uniforms[i][0] === "zeroLights" ? 32 : 1 });
  properties.get(sight).currentProgram = zeroLights;
  const mesh = new THREE.Mesh(partGeometry(), sight);
  viewScene.add(mesh);
  viewScene.updateMatrixWorld(true);
  assert.equal(core.cover("view", renderer, viewScene, view), true, "drawn by the renderer");
  const added = calls.find((c) => c[0] === "add_program" && c[1] === 3);
  assert.equal(added[5], "modelViewMatrix,normalMatrix,map", "the lights are not among the uniforms it sets");
  const input = core.viewInput;
  assert.equal(input[1 + 16], 3);
  assert.deepEqual([...input.subarray(1 + 17 + 18, 1 + 17 + 18 + 2)], [1, 9], "three's normal matrix of the part");
});

test("the transparent pass's objects draw with programs of their own, apart from another pass's for the same three program", () => {
  const { core, calls, properties } = bridge();
  const material = new THREE.MeshBasicMaterial({ transparent: true, blending: THREE.AdditiveBlending });
  const p = program([["modelViewMatrix", GL.FLOAT_MAT4], ["diffuse", GL.FLOAT_VEC3], ["opacity", GL.FLOAT]], ["position"]);
  properties.get(material).currentProgram = p;
  properties.get(material).__version = material.version;
  // Another pass (the effects' glow models) made one for it first, and keeps
  // what it last set in it.
  const shared = core.program(p, 0, material, { uniforms: new Set(["modelViewMatrix", "diffuse", "opacity"]), material: new Set() });
  const beam = new THREE.Mesh(partGeometry(), material);
  const entry = core.byName.transparent.registerObject({ object: beam, material, geometry: beam.geometry, group: null });
  assert.ok(entry, "registered");
  assert.equal(calls.filter((c) => c[0] === "add_program").length, 2, "made again for the transparent pass");
  const pass = calls.find((c) => c[0] === "transparent_object_pass");
  assert.notEqual(pass[2], shared.id, "drawn with its own");
});

// The near scene with the far scene's image in it (three has made its
// program), as graphics.js has it.
function nearBridge() {
  const { BfeRenderer, memory, calls } = standinModule();
  const background = new THREE.Mesh(partGeometry(), new THREE.ShaderMaterial({ uniforms: { map: { value: null } }, depthTest: false, depthWrite: false }));
  background.frustumCulled = false;
  const three = standinThree(calls, { graphics: { background } });
  three.scene.add(background);
  three.properties.get(background.material).currentProgram = program([["map", GL.SAMPLER_2D]], ["position", "uv"]);
  const core = new RustRender(three.graphics, { memory }, BfeRenderer);
  core.sceneItems = core.renderer.renderLists?.get(core.graphics.scene, 0);
  three.renderer.setRenderTarget(three.graphics.sceneTarget);
  calls.length = 0;
  return { core, calls, ...three };
}

test("the near scene: Rust draws every registered item", () => {
  const { core, calls, renderer, graphics, scene, camera, properties, list } = nearBridge();
  let before = 0;
  scene.onBeforeRender = () => { before++; };
  // A mesh a pass draws (its material hidden from three), one out of view,
  // and a hologram the transparent pass takes, in view.
  const hidden = new THREE.Mesh(partGeometry(), new THREE.MeshBasicMaterial({ visible: false }));
  const behind = new THREE.Mesh(partGeometry(), new THREE.MeshBasicMaterial());
  behind.position.set(0, 0, 50);
  const material = new THREE.MeshBasicMaterial({ transparent: true });
  const holo = new THREE.Mesh(partGeometry(), material);
  holo.position.set(1, 0, -5);
  holo.renderOrder = 2;
  scene.add(hidden, behind, holo);
  scene.updateMatrixWorld(true);
  const threeProgram = { id: 3 };
  properties.get(material).currentProgram = threeProgram;
  properties.get(material).__version = material.version;
  core.byName.transparent.taken.set(holo, { bucket: false, id: 7, material, program: threeProgram, frame: 0, version: material.version });
  list.opaque.push({ object: graphics.background });
  assert.equal(core.coverNear(renderer, scene, camera), true);
  assert.equal(before, 1, "three's start of a render");
  assert.deepEqual(calls.map((c) => c[0]).filter((n) => /frame$|^near_begin$/.test(n)), ["near_begin", "frame", "screen_frame", "transparent_frame"]);
  const begin = calls.find((c) => c[0] === "near_begin");
  assert.deepEqual([...begin[1]], [0, 0, 0, 1], "three's clear colour");
  assert.equal(begin[2], 0, "clear before opaque geometry; the far image follows it");
  assert.equal(core.postInput[1], -2, "into the target bound now");
  // The hologram's record, keyed as projectObject keys it.
  const z = new THREE.Vector4().copy(holo.geometry.boundingSphere.center).applyMatrix4(holo.matrixWorld)
    .applyMatrix4(new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse)).z;
  assert.deepEqual([...core.transparentObjects.subarray(0, 6)], [1, 7, 0, 2, z, holo.id]);
  assert.equal(renderer.info.render.frame, 1);
  assert.ok(calls.some((c) => c[0] === "blitFramebuffer"), "the target resolved");
  assert.ok(!calls.some((c) => c[0] === "resetState"), "three's state is reset when three is next used");
  assert.equal(core.stale, true);
});

test("the near scene: an unregistered item fails the required Rust draw", () => {
  const { core, calls, renderer, scene, camera, properties } = nearBridge();
  const wall = new THREE.Mesh(partGeometry(), new THREE.MeshBasicMaterial());
  properties.get(wall.material).currentProgram = program([], ["position"]);
  wall.position.set(0, 0, -5);
  scene.add(wall);
  scene.updateMatrixWorld(true);
  assert.throws(() => core.coverNear(renderer, scene, camera), /Incomplete Rust/);
  assert.equal(core.byName.near.stopped, wall);
  assert.ok(!calls.some((c) => /frame$|^near_begin$/.test(c[0])), "nothing drawn: three renders the scene");
  // Nor when three is to draw the scene for another camera or target.
  wall.material.visible = false;
  assert.throws(() => core.coverNear(renderer, scene, new THREE.PerspectiveCamera()), /Incomplete Rust/);
  assert.equal(core.coverNear(renderer, scene, camera), true);
});

test("phases drawn between three's renders reset three's state once, when three is next used", () => {
  const { core, calls, renderer, scene, camera } = bridge();
  core.addPass({ name: "a", phase: "view", draw() { this.complete = true; } });
  core.byName.view.draw = function () { this.complete = true; };
  const viewScene = new THREE.Scene();
  assert.equal(core.cover("view", renderer, viewScene, camera), true);
  core.phase("post", renderer, null, camera, true);
  core.phase("hud", renderer, scene, camera, true);
  assert.equal(calls.filter((c) => c[0] === "resetState").length, 0, "none yet");
  assert.equal(core.stale, true);
  renderer.setRenderTarget(null);
  assert.deepEqual(calls.slice(-3).map((c) => c[0]), ["resetState", "setRenderTarget", "setRenderTarget"]);
  assert.equal(calls.filter((c) => c[0] === "resetState").length, 1, "one for the three phases");
  // A phase drawn inside three's render resets at once.
  calls.length = 0;
  core.phase("near", renderer, scene, camera);
  assert.deepEqual(calls.slice(-2).map((c) => c[0]), ["resetState", "setRenderTarget"]);
  assert.equal(core.stale, false);
});

test("a command post's beam, hidden until the player comes near: registered before three ever draws it, taken from three's list the first frame it shows", () => {
  const { core, calls, renderer, scene, camera, properties, graphics } = bridge();
  const material = new THREE.MeshBasicMaterial({ transparent: true, blending: THREE.AdditiveBlending, depthWrite: false });
  const beam = new THREE.Mesh(partGeometry(), material);
  beam.visible = false;
  scene.add(beam);
  const p = program([["modelViewMatrix", GL.FLOAT_MAT4], ["diffuse", GL.FLOAT_VEC3], ["opacity", GL.FLOAT]], ["position"]);
  // three's compile: a program for each material it is given, no draw (no __version).
  const compiled = [];
  renderer.compile = (list, cam, s) => { list.traverse((o) => { compiled.push(o); properties.get(o.material).currentProgram = p; }); };
  const lightning = new THREE.Mesh(partGeometry(), new THREE.MeshBasicMaterial({ transparent: true }));
  lightning.geometry.getAttribute("position").setUsage(THREE.DynamicDrawUsage);
  scene.add(lightning);
  // three starts the frame's render of the near scene.
  scene.onBeforeRender(renderer, scene, camera, graphics.sceneTarget);
  assert.ok(compiled.includes(beam), "the beam is compiled without a draw");
  const entry = core.byName.transparent.taken.get(beam);
  assert.ok(entry, `registered before three drew it (${core.errors.join("; ")})`);
  assert.equal(calls.filter((c) => c[0] === "transparent_object").length, 1);
  // The player comes near: three lists it, and the pass takes it at once.
  beam.visible = true;
  const transparent = core.byName.transparent;
  transparent.frameNo++;
  const item = { object: beam, material, geometry: beam.geometry, group: null, groupOrder: 0, renderOrder: 0, z: 1, id: beam.id };
  const items = [item];
  transparent.take(items);
  assert.equal(items.length, 0, "taken out of three's list");
  // A change to its material since (another program, maybe): three's again until three draws it.
  material.needsUpdate = true;
  const again = [item];
  transparent.frameNo++;
  transparent.take(again);
  assert.equal(again.length, 1, "left to three");
  // A side's logo, made as the side takes the post: registered before three's next list of the scene.
  const holder = new THREE.Group();
  scene.add(holder);
  const logo = new THREE.Mesh(partGeometry(), new THREE.MeshBasicMaterial({ transparent: true }));
  holder.add(logo);
  compiled.length = 0;
  scene.onBeforeRender(renderer, scene, camera, graphics.sceneTarget);
  assert.deepEqual(compiled, [logo], "only what was added looked over");
  assert.ok(core.byName.transparent.taken.get(logo));
  scene.onBeforeRender(renderer, scene, camera, graphics.sceneTarget);
  assert.equal(compiled.length, 1, "nothing added since: nothing looked over");
});

test("a water mirror's render of the near scene: drawn in place of three's when everything three would list is the renderer's", () => {
  const { core, calls, renderer, scene, setTarget, properties } = nearBridge();
  const mirror = new THREE.PerspectiveCamera();
  mirror.userData.mirror = true;
  mirror.updateMatrixWorld(true);
  const reflection = { name: "reflection", samples: 0, width: 8, height: 4 };
  setTarget(reflection);
  const hidden = new THREE.Mesh(partGeometry(), new THREE.MeshBasicMaterial({ visible: false }));
  scene.add(hidden);
  scene.updateMatrixWorld(true);
  assert.equal(core.coverMirror(renderer, scene, mirror), true);
  assert.deepEqual(calls.map((c) => c[0]).filter((n) => /frame$|^near_begin$/.test(n)), ["near_begin", "mirror_frame", "screen_frame", "mirror_transparent_frame"]);
  assert.deepEqual(calls.find((c) => c[0] === "mirror_transparent_frame").slice(1), [0, 0], "the image drawn by the begin phase, not again");
  assert.equal(core.postInput[1], -2, "the image into the target bound now");
  assert.equal(core.stale, true, "three's state reset when three is next used");
  // Not the frame's camera, nor a render something three draws itself is in.
  assert.throws(() => core.coverMirror(renderer, scene, new THREE.PerspectiveCamera()), /Incomplete Rust/);
  const wall = new THREE.Mesh(partGeometry(), new THREE.MeshBasicMaterial());
  properties.get(wall.material).currentProgram = program([], ["position"]);
  wall.position.set(0, 0, -5);
  scene.add(wall);
  scene.updateMatrixWorld(true);
  calls.length = 0;
  assert.throws(() => core.coverMirror(renderer, scene, mirror), /Incomplete Rust/);
  assert.equal(core.byName.mirrorCover.stopped, wall);
  assert.ok(!calls.some((c) => /frame$|^near_begin$/.test(c[0])), "three renders it");
  // Switched off alone: three's.
  wall.material.visible = false;
  });

test("the view scene: a gun's part whose zero material blends is a record of its own kind, drawn from its row in three's order among what blends", () => {
  const { core, calls, renderer, properties } = bridge();
  const viewScene = new THREE.Scene(), view = new THREE.PerspectiveCamera();
  view.updateMatrixWorld(true);
  const part = new THREE.Mesh(partGeometry(), partMaterial(properties));
  // The glass: a zero material that blends (BLENDED, DYNAMIC), its lights and lamps per instance.
  const glassMaterial = partMaterial(properties);
  glassMaterial.transparent = true;
  glassMaterial.depthWrite = false;
  glassMaterial.defines.BLENDED = "";
  const glass = new THREE.Mesh(partGeometry(), glassMaterial);
  glass.position.z = -2;
  const flashMaterial = new THREE.MeshBasicMaterial({ map: new THREE.Texture(), transparent: true, blending: THREE.AdditiveBlending, depthWrite: false });
  properties.get(flashMaterial).currentProgram = program([["projectionMatrix", GL.FLOAT_MAT4], ["modelViewMatrix", GL.FLOAT_MAT4], ["diffuse", GL.FLOAT_VEC3], ["opacity", GL.FLOAT], ["map", GL.SAMPLER_2D], ["mapTransform", GL.FLOAT_MAT3]], ["position", "uv"]);
  const flash = new THREE.Mesh(partGeometry(), flashMaterial);
  flash.position.z = -1;
  viewScene.add(part, glass, flash);
  viewScene.updateMatrixWorld(true);
  assert.equal(core.cover("view", renderer, viewScene, view), true, `every part the renderer's: three left out (${core.errors.join("; ")})`);
  assert.equal(glassMaterial.visible, false);
  assert.ok(calls.some((c) => c[0] === "inst_add_blend"), "its blending and passes registered");
  const input = core.viewInput, D = 1, DRAW = 17 + 32 * 18;
  assert.equal(input[0], 2, "two draws that blend");
  assert.deepEqual([input[D], input[D + 1]], [-1, 0], "the farther, the glass: the first record that blends");
  assert.ok(input[D + DRAW] >= 0, "then the flash, three's program");
  const records = calls.filter((c) => c[0] === "view_frame").pop()[1];
  assert.equal(records, 2, "the part and the glass written");
  const o = core.recordsAt + core.stride;
  assert.equal(core.input[o], 4, "the glass's record of its own kind");
});

test('a reflection registers a newly added transparent object before the main camera sees it',()=>{
 const {core,calls,renderer,graphics,scene,camera,setTarget,properties}=nearBridge();
 const compiled=[];
 renderer.compile=list=>list.traverse(o=>{compiled.push(o);properties.get(o.material).currentProgram=program([['modelViewMatrix',GL.FLOAT_MAT4],['diffuse',GL.FLOAT_VEC3],['opacity',GL.FLOAT]],['position']);});
 scene.onBeforeRender(renderer,scene,camera,graphics.sceneTarget);
 const logo=new THREE.Mesh(partGeometry(),new THREE.MeshBasicMaterial({transparent:true}));
 const group=new THREE.Group();group.userData.material=logo.material;group.add(logo);
 logo.position.z=-5;scene.add(group);
 const mirror=camera.clone();mirror.userData.mirror=true;mirror.updateMatrixWorld(true);
 setTarget({name:'reflection',samples:0,width:8,height:4});
 assert.equal(core.coverMirror(renderer,scene,mirror),true);
 assert.ok(compiled.includes(logo),'the reflected object compiles before its first Rust draw');
 assert.ok(core.byName.effects.glows.get(logo));
 assert.ok(calls.some(c=>c[0]==='mirror_transparent_frame'));
 const count=compiled.length;
 scene.onBeforeRender(renderer,scene,camera,graphics.sceneTarget);
 assert.equal(compiled.length,count,'the main view reuses resources discovered by the reflection');
});
