import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");
const { standinModule } = await import("./support/rust-standin.mjs");

const STATS = [1, 2, 3, 4, 5, 1, 2, 0, 0, 0, 0, 0, 0, 0];

// The bridge over a stand-in for the WASM renderer (its memory, and a log of
// the calls made to it) and for the page's graphics and three's renderer.
function bridge() {
  // The segments the renderer found (transparent.rs), as the page reads them.
  const segments = [0, 0];
  // The shared stand-in for the WASM renderer (every pass's views), with the
  // world pass's registration as this test reads it.
  const { BfeRenderer, memory, calls, region } = standinModule({
    stats: STATS,
    lengths: { transparent_segments: () => segments.length },
    methods: ({ calls }) => Object.fromEntries(["add_world", "world_runs", "world_shown", "world_texture", "world_uniform", "world_cells"]
      .map((name) => [name, (...args) => { calls.push([name, ...args]); return name === "add_world" ? 9 : true; }])),
  });
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
    getClearColor: (c) => c.setRGB(0, 0, 0),
    getClearAlpha: () => 1,
    getContextAttributes: () => ({ premultipliedAlpha: true }),
  };
  const background = new THREE.Object3D();
  const graphics = { renderer, scene, camera, sceneTarget, farTarget, background, settings: { get: () => false }, upscaling: "off", cullRoots: [], playerRoot: null };
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  const setSegments = (values) => { segments.length = 0; segments.push(...values); new Uint32Array(memory.buffer, region("transparent_segments"), values.length).set(values); };
  return { core, calls, graphics, renderer, scene, camera, memory, list, setSegments, setTarget: (t) => { target = t; } };
}

test("the passes are made in the renderer's order, each in its phase", () => {
  const { core } = bridge();
  assert.deepEqual(core.passes.map((p) => p.name), ["world", "instances", "far", "terrain", "transparent", "effects", "foliage", "view", "post", "hud", "water", "near", "canopies", "sea", "motion", "image"]);
  const phases = Object.fromEntries(Object.entries(core.phases).map(([name, passes]) => [name, passes.map((p) => p.name)]));
  assert.deepEqual(phases, { far: ["far"], near: ["world", "instances", "terrain", "foliage", "canopies", "sea"], transparent: ["transparent", "effects", "water", "image"], view: ["view"], post: ["post"], hud: ["hud"], mirror: ["world", "instances", "terrain", "foliage", "canopies", "sea"], mirrorTransparent: ["transparent", "effects", "water", "image"], begin: ["near"], farMirror: ["far"], probe: ["far"], motion: ["motion"] });
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
  // The configuration the battle started in is the one the renderer covers
  // (its first phase's); another one mid-battle gives everything back.
  core.pass(renderer, scene, camera);
  assert.equal(core.enabled, true);
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
  memory.buffer = new ArrayBuffer(2 * memory.buffer.byteLength);
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
  assert.deepEqual(calls.find((c) => c[0] === "transparent_frame").slice(1), [2, 0], "three's two items' keys; no image of the renderer's (three draws it)");
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

test("the transparent list follows the far scene's image, drawn last of three's opaque list", () => {
  const { core, calls, renderer, scene, camera, graphics, list, setSegments } = bridge();
  assert.equal(core.nearAnchor.parent, scene, "the near phase's hook is in the near scene");
  assert.equal(core.nearAnchor.renderOrder, -1e9, "first in three's opaque list");
  const item = (object, renderOrder) => ({ object, material: {}, group: null, groupOrder: 0, renderOrder, z: 0, id: object.id });
  list.opaque.push(item(core.nearAnchor, -1e9), item(graphics.background, 1e9));
  setSegments([0, 0]);
  core.pass(renderer, scene, camera);
  assert.deepEqual(calls.map((c) => c[0]).filter((n) => n.endsWith("frame")), ["frame"], "not before the image");
  graphics.background.onAfterRender(renderer, scene, camera, null, list.opaque[1].material, null);
  assert.deepEqual(calls.map((c) => c[0]).filter((n) => n.endsWith("frame")), ["frame", "transparent_frame"]);
});

test("objects the transparent pass draws leave three's list as records with three's keys", () => {
  const { core, renderer, scene, camera, graphics, list, setSegments } = bridge();
  const pass = core.byName.transparent;
  const material = new THREE.MeshBasicMaterial({ transparent: true, color: 0x336699, opacity: 0.25 });
  const holo = new THREE.Mesh(new THREE.BufferGeometry(), material), other = new THREE.Object3D();
  holo.position.set(1, 2, 3);
  holo.updateMatrixWorld(true);
  // Registered in an earlier frame, three's program for it unchanged since.
  const program = { id: 3 };
  renderer.properties.get(material).currentProgram = program;
  renderer.properties.get(material).__version = material.version;
  pass.taken.set(holo, { bucket: false, id: 7, material, program, frame: 0 });
  list.opaque.push({ object: graphics.background, material: {}, group: null, groupOrder: 0, renderOrder: 0, z: 0, id: graphics.background.id });
  list.transparent.push({ object: holo, material, group: null, groupOrder: 0, renderOrder: 2, z: 0.75, id: holo.id },
    { object: other, material: {}, group: null, groupOrder: 0, renderOrder: 0, z: 0.5, id: other.id });
  setSegments([0, 1, 1]);
  core.pass(renderer, scene, camera);
  assert.deepEqual(list.transparent.map((item) => item.object), [other], "three keeps only what it still draws");
  const r = core.transparentObjects;
  assert.equal(r[0], 1);
  assert.deepEqual([...r.subarray(1, 6)], [7, 0, 2, 0.75, holo.id]);
  assert.deepEqual([...r.subarray(6, 22)], holo.matrixWorld.elements);
  assert.deepEqual([...r.subarray(22, 26)], [material.color.r, material.color.g, material.color.b, 0.25]);
});

test("with the transparent pass off, three keeps its whole list and the other sources' items still draw around it", () => {
  const { core, calls, renderer, scene, camera, graphics, list, setSegments } = bridge();
  const pass = core.byName.transparent;
  const material = new THREE.MeshBasicMaterial({ transparent: true });
  const holo = new THREE.Mesh(new THREE.BufferGeometry(), material);
  const program = { id: 3 };
  renderer.properties.get(material).currentProgram = program;
  renderer.properties.get(material).__version = material.version;
  pass.taken.set(holo, { bucket: false, id: 7, material, program, frame: 0 });
  list.opaque.push({ object: core.nearAnchor, material: {}, group: null, groupOrder: 0, renderOrder: -1e9, z: 0, id: core.nearAnchor.id },
    { object: graphics.background, material: {}, group: null, groupOrder: 0, renderOrder: 1e9, z: 0, id: graphics.background.id });
  list.transparent.push({ object: holo, material, group: null, groupOrder: 0, renderOrder: 0, z: 0.75, id: holo.id });
  setSegments([0, 0, 0]);
  core.views();
  core.transparentObjects[0] = 5;
  pass.setActive(false);
  core.pass(renderer, scene, camera);
  graphics.background.onAfterRender(renderer, scene, camera, null, list.opaque[1].material, null);
  assert.deepEqual(list.transparent.map((item) => item.object), [holo], "three draws what the pass would have taken");
  assert.equal(core.transparentObjects[0], 0, "the pass draws none of its own");
  assert.equal(calls.filter((c) => c[0] === "transparent_frame").length, 1, "the phase still runs");
  assert.deepEqual([...core.transparentKeys.subarray(0, 4)], [0, 0, 0.75, holo.id]);
});

test("an item whose program three changed goes back to three, to be registered again", () => {
  const { core, renderer, scene, camera, graphics, list, setSegments } = bridge();
  const pass = core.byName.transparent;
  const material = new THREE.MeshBasicMaterial({ transparent: true }), holo = new THREE.Mesh(new THREE.BufferGeometry(), material);
  const props = renderer.properties.get(material);
  props.currentProgram = { id: 4 };
  props.__version = material.version;
  const old = { bucket: false, id: 1, material, program: { id: 3 }, frame: 0 };
  pass.taken.set(holo, old);
  list.opaque.push({ object: core.nearAnchor, material: {}, group: null, groupOrder: 0, renderOrder: -1e9, z: 0, id: core.nearAnchor.id },
    { object: graphics.background, material: {}, group: null, groupOrder: 0, renderOrder: 1e9, z: 0, id: graphics.background.id });
  list.transparent.push({ object: holo, geometry: holo.geometry, material, group: null, groupOrder: 0, renderOrder: 0, z: 0.5, id: holo.id });
  setSegments([0, 0, 0]);
  core.pass(renderer, scene, camera);
  graphics.background.onAfterRender(renderer, scene, camera, null, list.opaque[1].material, null);
  assert.equal(list.transparent.length, 1, "three draws it");
  assert.notEqual(pass.taken.get(holo), old, "looked at again in the transparent phase");
  assert.deepEqual(pass.candidates, []);
});

// The core's program, geometry and texture calls, stood in for: every
// program reads what `names` lists.
function standIns(core, names) {
  core.program = () => ({ id: 5, names: new Set(names), attributes: [] });
  core.geometry = () => 3;
  core.texture = () => 0;
}

test("the ground registers once three has drawn it as it is now, as a world bucket with the patches' cells", () => {
  const { core, calls, renderer, scene, camera } = bridge();
  standIns(core, ["map", "layer1Map", "brightness", "layerScales"]);
  const material = new THREE.ShaderMaterial({ defines: { TERRAIN: "", WORLD_SPACE: "" }, uniforms: { brightness: { value: 1 }, layerScales: { value: new THREE.Vector2(8, 4) } } });
  const group = new THREE.Group(), index = new THREE.BufferAttribute(new Uint32Array(6), 1), geometry = new THREE.BufferGeometry();
  geometry.setIndex(index);
  group.add(new THREE.Mesh(geometry, material));
  group.material = material;
  const box = (x) => new THREE.Box3(new THREE.Vector3(x, 0, 0), new THREE.Vector3(x + 1, 1, 1));
  group.userData.patches = { full: { index, cells: [{ start: 0, end: 3, box: box(0) }, { start: 3, end: 6, box: box(5) }] }, bounds: new THREE.Sphere(new THREE.Vector3(), 9) };
  scene.add(group);
  const props = renderer.properties.get(material);
  core.pass(renderer, scene, camera);
  assert.equal(calls.filter((c) => c[0] === "add_world").length, 0, "not drawn by three yet");
  props.currentProgram = {};
  props.__version = material.version;
  core.pass(renderer, scene, camera);
  const add = calls.find((c) => c[0] === "add_world");
  assert.ok(add && add[4] & 32 && add[4] & 8, "a world bucket counted for the terrain, writing depth");
  assert.deepEqual([...calls.find((c) => c[0] === "world_runs")[2]], [0, 3, 3, 6]);
  assert.deepEqual([...calls.find((c) => c[0] === "world_uniform" && c[2] === "layerScales")[3]], [8, 4]);
  assert.equal(material.visible, false);
  assert.equal(group.userData.drawnElsewhere, true);
  // Changed since: three's again, until it has drawn it.
  material.needsUpdate = true;
  calls.length = 0;
  core.pass(renderer, scene, camera);
  assert.deepEqual(calls.find((c) => c[0] === "world_shown"), ["world_shown", 9, false]);
  assert.equal(material.visible, true);
  assert.equal(group.userData.drawnElsewhere, false);
});

test("a bucket culled by cell registers whatever draw range three's last view left it", () => {
  const { core, calls, renderer, scene, camera } = bridge();
  standIns(core, ["map"]);
  const material = new THREE.ShaderMaterial({ defines: { WORLD_SPACE: "" }, uniforms: {} });
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(9), 3));
  geometry.setIndex([0, 1, 2]);
  geometry.setDrawRange(0, 0);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.matrixAutoUpdate = false;
  mesh.userData.cellLights = true;
  scene.add(mesh);
  const props = renderer.properties.get(material);
  props.currentProgram = {};
  props.__version = material.version;
  core.pass(renderer, scene, camera);
  assert.equal(calls.filter((c) => c[0] === "add_world").length, 1);
  assert.equal(calls.filter((c) => c[0] === "world_cells").length, 1);
});

test("the world and instance passes switched off alone give their draws back to three", () => {
  const { core, calls, renderer, scene, camera } = bridge();
  standIns(core, ["map"]);
  const material = new THREE.ShaderMaterial({ defines: { WORLD_SPACE: "" }, uniforms: {} });
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(9), 3));
  geometry.setIndex([0, 1, 2]);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.matrixAutoUpdate = false;
  scene.add(mesh);
  const props = renderer.properties.get(material);
  props.currentProgram = {};
  props.__version = material.version;
  core.pass(renderer, scene, camera);
  assert.equal(material.visible, false, "registered: the renderer's");
  calls.length = 0;
  core.byName.world.setActive(false);
  assert.equal(material.visible, true);
  assert.deepEqual(calls.filter((c) => c[0] === "world_shown"), [["world_shown", 9, false]], "and not the renderer's too");
  core.byName.world.setActive(true);
  assert.equal(material.visible, false);
  assert.deepEqual(calls.filter((c) => c[0] === "world_shown").at(-1), ["world_shown", 9, true]);
  // The instance pass writes no records while it is off.
  const instances = core.byName.instances;
  let looked = 0;
  instances.soldier = () => { looked++; };
  core.graphics.cullRoots.push({});
  instances.setActive(false);
  core.pass(renderer, scene, camera);
  assert.equal(looked, 0);
  instances.setActive(true);
  core.pass(renderer, scene, camera);
  assert.equal(looked, 1);
});

test("a phase that made no GL call leaves three's state alone", () => {
  const { core, calls, renderer, scene, camera } = bridge();
  core.rust.transparent_frame = () => { calls.push(["transparent_frame"]); return [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]; };
  core.phase("transparent", renderer, scene, camera);
  assert.deepEqual(calls.map((c) => c[0]).filter((n) => n !== "add_texture" && n !== "set_far_map"), ["transparent_frame"]);
});
