import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");

const STATS = [1, 2, 3, 4, 5, 1, 2, 0, 0, 0, 0, 0, 0, 0];

// The bridge over a stand-in for the WASM renderer (its memory, and a log of
// the calls made to it) and for the page's graphics and three's renderer.
function bridge() {
  const calls = [];
  const memory = { buffer: new ArrayBuffer(1 << 16) };
  // The segments the renderer found (transparent.rs), as the page reads them.
  const segments = [0, 0];
  const entry = (name) => function (...args) { calls.push([name, ...args]); return STATS; };
  class BfeRenderer {
    input_ptr() { return 0; }
    input_len() { return 200 + 76 * 4; }
    bones_ptr() { return 8192; }
    bones_len() { return 64; }
    records_at() { return 200; }
    record_stride() { return 76; }
    fx_input_ptr() { return 40960; }
    fx_input_len() { return 600; }
    fx_data_ptr() { return 49152; }
    fx_data_len() { return 2048; }
    fx_matrices_ptr() { return 57344; }
    fx_matrices_len() { return 512; }
    add_texture() { calls.push(["add_texture"]); return 0; }
    set_far_map(texture) { calls.push(["set_far_map", texture]); }
    sort(byProgram) { calls.push(["sort", byProgram]); }
    transparent_keys_ptr() { return 16384; }
    transparent_keys_len() { return 64; }
    transparent_segments_ptr() { return 32768; }
    transparent_segments_len() { return segments.length; }
    transparent_segment(k) { calls.push(["transparent_segment", k]); return STATS; }
    transparent_bucket_shown() {}
  }
  for (const name of ["far_frame", "frame", "transparent_frame", "view_frame", "post_frame"]) BfeRenderer.prototype[name] = entry(name);
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera();
  camera.updateMatrixWorld(true);
  const sceneTarget = { name: "scene" }, farTarget = { texture: new THREE.Texture() };
  const properties = new Map();
  let target = sceneTarget;
  // three's render list of the near scene (opaque, transparent: render items).
  const list = { opaque: [], transmissive: [], transparent: [] };
  const renderer = {
    renderLists: { get: () => list },
    getContext: () => ({}),
    properties: { get: (o) => { if (!properties.has(o)) properties.set(o, { __webglTexture: {} }); return properties.get(o); } },
    initTexture() {},
    shadowMap: { enabled: false },
    getRenderTarget: () => target,
    setRenderTarget(t) { calls.push(["setRenderTarget", t]); target = t; },
    resetState() { calls.push(["resetState"]); },
  };
  const background = new THREE.Object3D();
  const graphics = { renderer, scene, camera, sceneTarget, farTarget, background, settings: { get: () => false }, upscaling: "off", cullRoots: [], playerRoot: null };
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  const setSegments = (values) => { segments.length = 0; segments.push(...values); new Uint32Array(memory.buffer, 32768, values.length).set(values); };
  return { core, calls, graphics, renderer, scene, camera, memory, list, setSegments, setTarget: (t) => { target = t; } };
}

test("the passes are made in the renderer's order, each in its phase", () => {
  const { core } = bridge();
  assert.deepEqual(core.passes.map((p) => p.name), ["world", "instances", "far", "terrain", "transparent", "effects", "foliage", "view", "post"]);
  const phases = Object.fromEntries(Object.entries(core.phases).map(([name, passes]) => [name, passes.map((p) => p.name)]));
  assert.deepEqual(phases, { far: ["far"], near: ["world", "instances", "terrain", "foliage"], transparent: ["transparent", "effects"], view: ["view"], post: ["post"], mirror: ["world", "instances", "terrain", "foliage"] });
  assert.throws(() => core.addPass({ name: "x", phase: "nowhere" }), /no phase nowhere/);
});

test("the near phase: the far scene's image and frame block, its passes in order, one renderer call, three's state reset", () => {
  const { core, calls, renderer, scene, camera, graphics } = bridge();
  const drawn = [];
  core.addPass({ name: "a", phase: "near", draw: (ctx) => { drawn.push(["a", ctx.records]); ctx.records += 2; } });
  core.addPass({ name: "b", phase: "near", draw: (ctx) => { drawn.push(["b", ctx.records]); ctx.records += 1; } });
  core.addPass({ name: "c", phase: "far", draw: () => drawn.push(["c"]) });
  core.pass(renderer, scene, camera);
  assert.deepEqual(drawn, [["a", 0], ["b", 2]]);
  assert.deepEqual(calls, [["add_texture"], ["set_far_map", 0], ["sort", false], ["frame", 3], ["resetState"], ["setRenderTarget", graphics.sceneTarget]]);
  assert.equal(core.stats, STATS);
  // The frame block: the camera's matrices and position, as three uploads them.
  assert.deepEqual([...core.input.subarray(0, 16)], camera.projectionMatrix.elements);
  // The far scene's image is registered once, as is the world once nothing
  // in it waits for three to draw it first.
  calls.length = 0;
  core.pass(renderer, scene, camera);
  assert.deepEqual(calls.map((c) => c[0]), ["frame", "resetState", "setRenderTarget"]);
});

test("the near hook leaves other cameras and targets to three", () => {
  const { core, calls, renderer, scene, camera, setTarget } = bridge();
  core.pass(renderer, scene, new THREE.PerspectiveCamera());
  setTarget({ name: "elsewhere" });
  core.pass(renderer, scene, camera);
  assert.deepEqual(calls, []);
});

test("every other phase calls its own renderer entry and binds back the target it found", () => {
  const { core, calls, renderer, scene, camera, setTarget } = bridge();
  const target = { name: "far" };
  setTarget(target);
  core.phase("far", renderer, scene, camera);
  assert.deepEqual(calls, [["far_frame"], ["resetState"], ["setRenderTarget", target]]);
  for (const [name, entry] of [["transparent", "transparent_frame"], ["view", "view_frame"], ["post", "post_frame"]]) {
    calls.length = 0;
    core.phase(name, renderer, scene, camera);
    assert.equal(calls.filter((c) => c[0].endsWith("frame"))[0][0], entry);
    assert.equal(core.phaseStats[name], STATS);
  }
});

test("on, off and restore go through the passes", () => {
  const { core, calls, renderer, scene, camera, graphics } = bridge();
  const seen = [];
  core.addPass({ name: "spy", phase: "near", setActive: (on) => seen.push(on), restore: () => seen.push("restore") });
  core.setActive(false);
  core.setActive(false);
  core.pass(renderer, scene, camera);
  assert.deepEqual(calls, [], "off: three draws everything");
  core.setActive(true);
  assert.deepEqual(seen, [false, true]);
  // A configuration the renderer does not cover gives everything back.
  const hidden = new THREE.ShaderMaterial();
  hidden.visible = false;
  scene.add(new THREE.Mesh(new THREE.BufferGeometry(), hidden));
  graphics.upscaling = "temporal";
  core.pass(renderer, scene, camera);
  assert.equal(core.enabled, false);
  assert.equal(hidden.visible, true);
  assert.deepEqual(seen, [false, true, "restore"]);
});

test("a pass's views of the renderer's memory are made again when it grows", () => {
  const { core, memory } = bridge();
  core.addView("extra", Uint32Array, () => 16384, () => 8);
  core.views();
  const [input, extra] = [core.input, core.extra];
  assert.equal(extra.length, 8);
  core.views();
  assert.equal(core.input, input, "the same memory: the same views");
  memory.buffer = new ArrayBuffer(1 << 17);
  core.views();
  assert.notEqual(core.input, input);
  assert.equal(core.extra.buffer, memory.buffer);
});

test("an anchor is drawn with no fragments and calls back after three draws it", () => {
  const { core, renderer, scene, camera } = bridge();
  const fired = [];
  const anchor = core.anchor(scene, { transparent: true, renderOrder: -5 }, (...args) => fired.push(args));
  assert.equal(anchor.parent, scene);
  const p = anchor.geometry.attributes.position;
  assert.equal(p.count, 3);
  assert.ok([1, 2].every((i) => p.getX(i) === p.getX(0) && p.getY(i) === p.getY(0) && p.getZ(i) === p.getZ(0)), "one degenerate triangle");
  const m = anchor.material;
  assert.deepEqual([m.transparent, m.colorWrite, m.depthWrite, m.depthTest, anchor.frustumCulled, anchor.renderOrder], [true, false, false, false, false, -5]);
  anchor.onAfterRender(renderer, scene, camera, anchor.geometry, m, null);
  assert.deepEqual(fired, [[renderer, scene, camera]]);
});

test("the transparent list: armed after the near phase on three's last opaque item, later segments on three's own items", () => {
  const { core, calls, renderer, scene, camera, graphics, list, setSegments } = bridge();
  const item = (object, z, extra = {}) => ({ object, material: {}, group: null, groupOrder: 0, renderOrder: 0, z, id: object.id, ...extra });
  const wall = new THREE.Object3D(), a = new THREE.Object3D(), b = new THREE.Object3D();
  list.opaque.push(item(graphics.background, 0), item(wall, 0.5));
  list.transparent.push(item(a, 0.9), item(b, 0.2, { renderOrder: 3 }));
  // Segment 0 holds one item, none falls between three's two, two follow b.
  setSegments([0, 1, 1, 3]);
  core.pass(renderer, scene, camera);
  assert.ok(Object.prototype.hasOwnProperty.call(wall, "onAfterRender"), "the last opaque item is armed");
  calls.length = 0;
  // Elsewhere (another camera) the hook does nothing.
  wall.onAfterRender(renderer, scene, new THREE.PerspectiveCamera(), null, list.opaque[1].material, null);
  assert.deepEqual(calls, []);
  wall.onAfterRender(renderer, scene, camera, null, list.opaque[1].material, null);
  assert.equal(calls.filter((c) => c[0] === "transparent_frame").length, 1);
  assert.deepEqual(calls.find((c) => c[0] === "transparent_frame").slice(1), [2]);
  assert.ok(!Object.prototype.hasOwnProperty.call(wall, "onAfterRender"), "its hook is taken off once it ran");
  // three's items' keys, in three's order.
  assert.deepEqual([...core.transparentKeys.subarray(0, 8)], [0, 0, 0.9, a.id, 0, 3, 0.2, b.id]);
  // Nothing after a; segment 2 right after b.
  assert.ok(!Object.prototype.hasOwnProperty.call(a, "onAfterRender"));
  calls.length = 0;
  b.onAfterRender(renderer, scene, camera, null, list.transparent[1].material, null);
  assert.deepEqual(calls.map((c) => c[0]), ["transparent_segment", "resetState", "setRenderTarget"]);
  assert.equal(calls[0][1], 2);
  assert.ok(!Object.prototype.hasOwnProperty.call(b, "onAfterRender"));
});

test("the transparent list follows at once when the far scene's image is three's only opaque item", () => {
  const { core, calls, renderer, scene, camera, graphics, list, setSegments } = bridge();
  list.opaque.push({ object: graphics.background, material: {}, group: null, groupOrder: 0, renderOrder: -1e9, z: 0, id: graphics.background.id });
  setSegments([0, 0]);
  core.pass(renderer, scene, camera);
  assert.deepEqual(calls.map((c) => c[0]).filter((n) => n.endsWith("frame")), ["frame", "transparent_frame"]);
});
