import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");

const STATS = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const GL = { VERTEX_SHADER: 0x8b31, FRAGMENT_SHADER: 0x8b30, SHADER_TYPE: 0x8b4f, ACTIVE_UNIFORMS: 0x8b86, ACTIVE_ATTRIBUTES: 0x8b89,
  FLOAT: 0x1406, FLOAT_VEC2: 0x8b50, FLOAT_VEC3: 0x8b51, FLOAT_VEC4: 0x8b52, INT: 0x1404, BOOL: 0x8b56, FLOAT_MAT3: 0x8b5b, FLOAT_MAT4: 0x8b5c, SAMPLER_2D: 0x8b5e,
  READ_FRAMEBUFFER: 0x8ca8, DRAW_FRAMEBUFFER: 0x8ca9, COLOR_BUFFER_BIT: 0x4000, DEPTH_BUFFER_BIT: 0x100, STENCIL_BUFFER_BIT: 0x400, NEAREST: 0x2600 };

// three's program for a material: its active uniforms ([name, GL type]) and
// attributes, and a source each stage.
function program(uniforms, attributes = ["position", "uv"]) {
  return { id: Math.floor(Math.random() * 1e6), program: { uniforms, attributes } };
}

// The bridge over a stand-in for the WASM renderer (a log of its calls, its
// memory), WebGL and the page's graphics.
function bridge() {
  const calls = [];
  const memory = { buffer: new ArrayBuffer(1 << 20) };
  const entry = (name) => function (...args) { calls.push([name, ...args]); return STATS; };
  let programs = 0, targets = 0;
  class BfeRenderer {
    input_ptr() { return 0; }
    input_len() { return 200 + 76 * 8; }
    bones_ptr() { return 16384; }
    bones_len() { return 64; }
    records_at() { return 200; }
    record_stride() { return 76; }
    add_texture() { calls.push(["add_texture"]); return 0; }
    set_far_map(texture) { calls.push(["set_far_map", texture]); }
    sort() {}
    add_program(kind, vs, fs, attributes, uniforms) { calls.push(["add_program", kind, attributes, uniforms]); return programs++; }
    add_geometry() { return 0; }
    geometry_buffer() { return 0; }
    geometry_pointer() {}
    geometry_index() {}
    geometry_group() {}
    foliage_input_ptr() { return 65536; }
    foliage_input_len() { return 53; }
    foliage_class(...args) { calls.push(["foliage_class", ...args]); return 0; }
    foliage_frame() { calls.push(["foliage_frame"]); return 7; }
    post_input_ptr() { return 131072; }
    post_input_len() { return 8 * 221; }
    post_target(texture) { calls.push(["post_target", texture]); return targets++; }
    hud_input_ptr() { return 262144; }
    hud_input_len() { return 47 + 1024 * 5; }
    hud_position_ptr() { return 327680; }
    hud_position_len() { return 64 * 3; }
    hud_uv_ptr() { return 393216; }
    hud_uv_len() { return 64 * 2; }
    hud_mask_uv_ptr() { return 458752; }
    hud_mask_uv_len() { return 64 * 2; }
    hud_tint_ptr() { return 524288; }
    hud_tint_len() { return 64 * 4; }
    hud_init(index) { calls.push(["hud_init", index.length]); }
  }
  for (const name of ["far_frame", "frame", "transparent_frame", "view_frame", "post_frame", "hud_frame", "mirror_frame"]) BfeRenderer.prototype[name] = entry(name);
  const gl = {
    ...GL,
    getProgramParameter: (p, what) => (what === GL.ACTIVE_UNIFORMS ? p.uniforms.length : p.attributes.length),
    getActiveUniform: (p, i) => ({ name: p.uniforms[i][0], type: p.uniforms[i][1], size: 1 }),
    getActiveAttrib: (p, i) => ({ name: p.attributes[i] }),
    getAttribLocation: (p, name) => p.attributes.indexOf(name),
    getAttachedShaders: () => [{ type: GL.FRAGMENT_SHADER }, { type: GL.VERTEX_SHADER }],
    getShaderParameter: (s) => s.type,
    getShaderSource: (s) => (s.type === GL.VERTEX_SHADER ? "vs" : "fs"),
    blitFramebuffer: (...args) => calls.push(["blitFramebuffer", ...args]),
  };
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera();
  camera.updateMatrixWorld(true);
  const sceneTarget = { name: "scene", samples: 2, width: 8, height: 4, depthBuffer: true, resolveDepthBuffer: false },
    farTarget = { texture: new THREE.Texture() };
  const properties = new Map();
  properties.set(sceneTarget, { __webglMultisampledFramebuffer: "ms", __webglFramebuffer: "fb" });
  let target = sceneTarget;
  const renderer = {
    getContext: () => gl,
    properties: { get: (o) => { if (!properties.has(o)) properties.set(o, { __webglTexture: {} }); return properties.get(o); } },
    initTexture(texture) { calls.push(["initTexture", texture.name]); properties.get(texture).__version = texture.version; },
    shadowMap: { enabled: false },
    state: { bindFramebuffer: (t, fb) => calls.push(["bindFramebuffer", t, fb]) },
    getRenderTarget: () => target,
    setRenderTarget(t) { calls.push(["setRenderTarget", t]); target = t; },
    resetState() { calls.push(["resetState"]); },
    getViewport: (v) => v.set(0, 0, 400, 300),
    getPixelRatio: () => 2,
  };
  const graphics = { renderer, scene, camera, sceneTarget, farTarget, settings: { get: () => false }, upscaling: "off", cullRoots: [], playerRoot: null };
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  return { core, calls, gl, graphics, renderer, scene, camera, memory, properties: renderer.properties, setTarget: (t) => { target = t; } };
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
  assert.equal(core.cover("view", renderer, scene, camera), false, "a pass of the phase short of the scene: three renders it");
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
  assert.equal(core.cover("view", renderer, scene, camera), false, "one pass short: three renders it");
  // Off: three draws everything; the phase does nothing.
  complete = true;
  core.setActive(false);
  calls.length = 0;
  assert.equal(core.cover("view", renderer, scene, camera), false);
  assert.deepEqual(calls, []);
});

test("a mirror's render of the near scene draws the near phase's passes for the mirror camera", () => {
  const { core, calls, renderer, scene, setTarget } = bridge();
  const mirror = new THREE.PerspectiveCamera();
  mirror.updateMatrixWorld(true);
  mirror.userData.mirror = true;
  setTarget({ name: "reflection" });
  core.pass(renderer, scene, mirror);
  assert.ok(calls.some((c) => c[0] === "mirror_frame"));
  assert.ok(!calls.some((c) => c[0] === "frame"));
  calls.length = 0;
  core.pass(renderer, scene, new THREE.PerspectiveCamera());
  assert.deepEqual(calls, [], "any other camera: nothing");
});

test("foliage: the drawer selects for the classes it registered, three keeps the rest", async () => {
  const { core, calls, renderer, scene, properties } = bridge();
  const geometry = new THREE.InstancedBufferGeometry();
  const card = (name, size) => geometry.setAttribute(name, new THREE.BufferAttribute(new Float32Array(4 * size).map((_, i) => i), size));
  for (const [name, size] of [["position", 3], ["color", 4], ["uv", 2], ["foliageCorner", 2], ["foliageShape", 4], ["foliageRate", 1]]) card(name, size);
  geometry.setIndex(new THREE.BufferAttribute(new Uint32Array([0, 2, 1, 1, 2, 3]), 1));
  const material = new THREE.MeshBasicMaterial({ map: new THREE.Texture(), alphaTest: 0.25 });
  properties.get(material).currentProgram = program([["modelViewMatrix", GL.FLOAT_MAT4], ["map", GL.SAMPLER_2D], ["foliageEye", GL.FLOAT_VEC3]],
    ["position", "color", "uv", "foliageCorner", "foliageShape", "foliageRate", "foliagePatch", "foliageTint", "foliageAxisX", "foliageAxisY", "foliageAxisZ", "foliageDistance"]);
  const batch = { geometry, material, uniforms: { foliageWiggle: { value: 0.11 } } };
  const instance = { position: new THREE.Vector3(1, 2, 3), matrix: new THREE.Matrix4().makeTranslation(1, 2, 3), tint: [0.5, 0.6, 0.7], sphere: null, cullDistance: Infinity, maxDistance: 100 };
  const kind = { grass: true, batch, instances: [instance] };
  const other = { grass: false, batch: { geometry: new THREE.BufferGeometry(), material: new THREE.MeshBasicMaterial() }, instances: [] };
  const foliage = { kinds: new Map([["a", kind], ["b", other]]), time: 2.5, nearEnd: () => 200, drawer: null };
  scene.userData.foliage = foliage;
  core.pass(renderer, scene, core.graphics.camera);
  assert.equal(foliage.drawer, core.byName.foliage, "the near phase attaches the drawer");
  const camera = new THREE.PerspectiveCamera();
  camera.position.set(4, 5, 6);
  camera.updateMatrixWorld(true);
  foliage.drawer.select(foliage, camera);
  const added = calls.find((c) => c[0] === "foliage_class");
  assert.equal(added[3], true, "grass");
  assert.deepEqual([...added[7]], [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11], "attribute locations: the card's, then the patch row's");
  assert.deepEqual([...added[8]].slice(0, 6).map((v) => +v.toFixed(2)), [1, 2, 3, 0.5, 0.6, 0.7]);
  assert.deepEqual([...added[8]].slice(15, 16), [100]);
  assert.deepEqual([...added[9]].slice(6, 9), [-1, Infinity, 100], "no sphere, always tested");
  assert.equal(material.visible, false);
  assert.ok(foliage.drawer.covers(kind));
  assert.ok(!foliage.drawer.covers(other), "a CPU batch stays three's");
  assert.ok(calls.some((c) => c[0] === "foliage_frame"));
  const input = core.foliageInput;
  assert.deepEqual([input[48], input[49], input[50], input[51], input[52]], [4, 5, 6, 200, 2.5]);
  core.setActive(false);
  assert.equal(material.visible, true);
  assert.equal(foliage.drawer, null);
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
  assert.equal(core.cover("view", renderer, viewScene, view), false);
  assert.equal(part.material.visible, false);
  assert.equal(flash.material.visible, true);
  assert.equal(calls.filter((c) => c[0] === "blitFramebuffer").length, 0, "three's render resolves the target");
  // Hidden objects, and their subtrees, are not three's to draw.
  flash.visible = false;
  assert.equal(core.cover("view", renderer, viewScene, view), true);
  // Off: three draws everything; the phase does nothing.
  core.setActive(false);
  assert.equal(part.material.visible, true);
  assert.equal(core.cover("view", renderer, viewScene, view), false);
});

test("the post steps: drawn by the renderer when every one is a pass it has a program for", () => {
  const { core, calls, renderer, graphics, properties } = bridge();
  const pass = (uniforms, values) => {
    const material = new THREE.ShaderMaterial({ uniforms: values });
    properties.get(material).currentProgram = program(uniforms);
    const mesh = new THREE.Mesh(new THREE.BufferGeometry(), material);
    return mesh;
  };
  const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  const postTarget = { width: 8, height: 4, texture: new THREE.Texture() };
  const composite = pass([["projectionMatrix", GL.FLOAT_MAT4], ["modelViewMatrix", GL.FLOAT_MAT4], ["map", GL.SAMPLER_2D], ["glow", GL.SAMPLER_2D], ["blend", GL.FLOAT]],
    { map: { value: graphics.farTarget.texture }, glow: { value: null }, blend: { value: 0.3 } });
  const fxaa = pass([["tDiffuse", GL.SAMPLER_2D], ["resolution", GL.FLOAT_VEC2]], { tDiffuse: { value: postTarget.texture }, resolution: { value: new THREE.Vector2(1 / 8, 1 / 4) } });
  const steps = [{ pass: composite, target: postTarget, camera }, { pass: fxaa, target: null, camera }];
  assert.equal(core.post(renderer, steps), true);
  assert.deepEqual(calls.filter((c) => c[0] === "add_program").map((c) => c.slice(1)), [[3, "position:0,uv:1", "projectionMatrix,modelViewMatrix,map,glow,blend"], [3, "position:0,uv:1", "tDiffuse,resolution"]]);
  assert.deepEqual(calls.find((c) => c[0] === "post_frame"), ["post_frame", 2]);
  const input = core.postInput, step = 5 + 12 * 18;
  assert.deepEqual([...input.subarray(0, 5)], [0, 0, 8, 4, 5], "program, target, its size, five uniforms");
  assert.deepEqual([...input.subarray(5 + 3 * 18, 5 + 3 * 18 + 3)], [3, 32, -1], "a sampler of none: three's empty texture");
  assert.deepEqual([...input.subarray(5 + 4 * 18, 5 + 4 * 18 + 3)].map((v) => +v.toFixed(3)), [4, 1, 0.3]);
  assert.deepEqual([...input.subarray(step, step + 5)], [1, -1, 800, 600, 2], "the canvas: three's viewport in pixels");
  assert.deepEqual([...input.subarray(step + 5 + 18, step + 5 + 18 + 4)], [1, 2, 0.125, 0.25]);
  // A step only three can draw (bloom): three draws them all.
  assert.equal(core.post(renderer, [{ run: () => {} }, ...steps]), false);
});
