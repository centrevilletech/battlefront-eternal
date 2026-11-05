import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { worldMaterial } from "../src/materials.js";
import { ownWorldMaterial } from "../src/rust-render/world-programs.js";

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
    compile() {},
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
  core.sceneItems = core.renderer.renderLists?.get(core.graphics.scene, 0);
  const setSegments = (values) => { segments.length = 0; segments.push(...values); new Uint32Array(memory.buffer, region("transparent_segments"), values.length).set(values); };
  return { core, calls, graphics, renderer, scene, camera, memory, list, setSegments, setTarget: (t) => { target = t; } };
}

test("the passes are made in the renderer's order, each in its phase", () => {
  const { core } = bridge();
  assert.deepEqual(core.passes.map((p) => p.name), ["world", "instances", "far", "terrain", "transparent", "effects", "foliage", "view", "post", "hud", "water", "near", "canopies", "sea", "motion", "shadow", "clouds", "underwater", "mirrorCover", "fog"]);
  const phases = Object.fromEntries(Object.entries(core.phases).map(([name, passes]) => [name, passes.map((p) => p.name)]));
  assert.deepEqual(phases, { far: ["far"], near: ["world", "instances", "terrain", "foliage", "canopies", "sea"], transparent: ["transparent", "effects", "water", "fog"], view: ["view"], post: ["post"], hud: ["hud"], mirror: ["world", "instances", "terrain", "foliage", "canopies", "sea"], mirrorTransparent: ["transparent", "effects", "water", "fog"], begin: ["near", "mirrorCover"], farMirror: ["far"], probe: ["far"], motion: ["motion"], shadow: ["shadow"], image: [], skyClouds: ["clouds"], underwater: ["underwater"] });
  assert.throws(() => core.addPass({ name: "x", phase: "nowhere" }), /no phase nowhere/);
});

test("the near phase: the far scene's image and frame block, its passes in order, one renderer call, three's state reset", () => {
  const { core, calls, renderer, scene, camera, graphics } = bridge();
  const drawn = [];
  core.addPass({ name: "a", phase: "near", draw: (ctx) => { drawn.push(["a", ctx.records]); ctx.records += 2; } });
  core.addPass({ name: "b", phase: "near", draw: (ctx) => { drawn.push(["b", ctx.records]); ctx.records += 1; } });
  core.addPass({ name: "c", phase: "far", draw: () => drawn.push(["c"]) });
  core.phase("near", renderer, scene, camera);
  assert.deepEqual(drawn, [["a", 0], ["b", 2]]);
  assert.deepEqual(calls, [["add_texture"], ["set_far_map", 0], ["sort", false], ["frame", 3], ["resetState"], ["setRenderTarget", graphics.sceneTarget], ["transparent_frame", 0, 0], ["resetState"], ["setRenderTarget", graphics.sceneTarget]]);
  assert.equal(core.stats, STATS);
  // The frame block: the camera's matrices and position, as three uploads them.
  assert.deepEqual([...core.input.subarray(0, 16)], camera.projectionMatrix.elements);
  // The far scene's image is registered once, as is the world once nothing
  // in it waits for three to draw it first.
  calls.length = 0;
  core.phase("near", renderer, scene, camera);
  assert.deepEqual(calls.map((c) => c[0]), ["frame", "resetState", "setRenderTarget", "transparent_frame", "resetState", "setRenderTarget"]);
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
  pass.taken.set(holo, { bucket: false, id: 7, material, program, frame: 0, version: material.version });
  list.opaque.push({ object: graphics.background, material: {}, group: null, groupOrder: 0, renderOrder: 0, z: 0, id: graphics.background.id });
  list.transparent.push({ object: holo, material, group: null, groupOrder: 0, renderOrder: 2, z: 0.75, id: holo.id },
    { object: other, material: {}, group: null, groupOrder: 0, renderOrder: 0, z: 0.5, id: other.id });
  setSegments([0, 1, 1]);
  pass.frameNo = 1;
  pass.take(list.transparent);
  assert.deepEqual(list.transparent.map((item) => item.object), [other], "unclaimed items remain visible to the near-scene completeness check");
  const r = core.transparentObjects;
  assert.equal(r[0], 1);
  assert.deepEqual([...r.subarray(1, 6)], [7, 0, 2, 0.75, holo.id]);
  assert.deepEqual([...r.subarray(6, 22)], holo.matrixWorld.elements);
  assert.deepEqual([...r.subarray(22, 26)], [material.color.r, material.color.g, material.color.b, 0.25]);
});





// The core's program, geometry and texture calls, stood in for: every
// program reads what `names` lists.
function standIns(core, names) {
  core.program = () => ({ id: 5, names: new Set(names), attributes: [] });
  core.geometry = () => 3;
  core.texture = () => 0;
}

function mapMaterial(scene, terrain = true) {
  for(let i=0;i<14;i++)scene.add(new THREE.PointLight(0,0,1));
  const material=worldMaterial(terrain?"terrain":"unlit",{map:new THREE.Texture(),lights:terrain});
  material.defines.WORLD_SPACE="";
  if(terrain){material.defines.TERRAIN_LAYERS=3;Object.assign(material.uniforms,{layerMap0:{value:material.map},layerMap1:{value:null},layerMap2:{value:null}});}
  ownWorldMaterial(material);return material;
}

test("the ground registers its owned program before any browser draw", () => {
  const { core, calls, renderer, scene, camera } = bridge();
  standIns(core, ["map", "layer1Map", "brightness", "layerScales"]);
  const material=mapMaterial(scene);
  material.uniforms.layerScale0.value.set(8,4,2,1);
  const group = new THREE.Group(), index = new THREE.BufferAttribute(new Uint32Array(6), 1), geometry = new THREE.BufferGeometry();
  geometry.setIndex(index);
  group.add(new THREE.Mesh(geometry, material));
  group.material = material;
  const box = (x) => new THREE.Box3(new THREE.Vector3(x, 0, 0), new THREE.Vector3(x + 1, 1, 1));
  group.userData.patches = { full: { index, cells: [{ start: 0, end: 3, box: box(0) }, { start: 3, end: 6, box: box(5) }] }, bounds: new THREE.Sphere(new THREE.Vector3(), 9) };
  scene.add(group);
  core.phase("near", renderer, scene, camera);
  assert.equal(renderer.properties.get(material).currentProgram,undefined);
  const add = calls.find((c) => c[0] === "add_world");
  assert.ok(add && add[4] & 32 && add[4] & 8, "a world bucket counted for the terrain, writing depth");
  assert.deepEqual([...calls.find((c) => c[0] === "world_runs")[2]], [0, 3, 3, 6]);
  assert.deepEqual([...calls.find((c) => c[0] === "world_uniform" && c[2] === "layerScale0")[3]], [8, 4, 2, 1]);
  assert.equal(material.visible, false);
  assert.equal(group.userData.drawnElsewhere, true);
  // A material change hides the old native bucket and registers its replacement.
  material.needsUpdate = true;
  calls.length = 0;
  core.phase("near", renderer, scene, camera);
  assert.deepEqual(calls.find((c) => c[0] === "world_shown"), ["world_shown", 9, false]);
  assert.equal(material.visible, false);
  assert.equal(group.userData.drawnElsewhere, true);
  assert.ok(calls.some(c => c[0] === "add_world"), "changed materials are recompiled and registered before drawing");
});

test("a bucket culled by cell registers whatever draw range three's last view left it", () => {
  const { core, calls, renderer, scene, camera } = bridge();
  standIns(core, ["map"]);
  const material=mapMaterial(scene,false);
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(9), 3));
  geometry.setIndex([0, 1, 2]);
  geometry.setDrawRange(0, 0);
  const mesh = new THREE.Mesh(geometry, material);
  mesh.matrixAutoUpdate = false;
  mesh.userData.cellLights = true;
  scene.add(mesh);
  core.phase("near", renderer, scene, camera);
  assert.equal(calls.filter((c) => c[0] === "add_world").length, 1);
  assert.equal(calls.filter((c) => c[0] === "world_cells").length, 1);
});



test("a phase that made no GL call leaves three's state alone", () => {
  const { core, calls, renderer, scene, camera } = bridge();
  core.rust.transparent_frame = () => { calls.push(["transparent_frame"]); return [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]; };
  core.phase("transparent", renderer, scene, camera);
  assert.deepEqual(calls.map((c) => c[0]).filter((n) => n !== "add_texture" && n !== "set_far_map"), ["transparent_frame"]);
});

test("painted terrain binds an empty color texture for an unpainted layer between active layers", () => {
  const { core, calls, renderer, scene, camera } = bridge();
  standIns(core, ["layerMap0", "layerMap1", "layerMap2"]);
  const textures = [], first = new THREE.Texture(), last = new THREE.Texture();
  core.texture = (texture) => { textures.push(texture); return textures.length - 1; };
  const material=mapMaterial(scene);
  Object.assign(material.uniforms,{layerMap0:{value:first},layerMap1:{value:null},layerMap2:{value:last}});
  const geometry = new THREE.BufferGeometry(), index = new THREE.BufferAttribute(new Uint32Array([0, 1, 2]), 1);
  geometry.setIndex(index);
  const group = new THREE.Group();
  group.add(new THREE.Mesh(geometry, material));
  group.material = material;
  group.userData.patches = { full: { index, cells: [{ start: 0, end: 3, box: new THREE.Box3(new THREE.Vector3(-1, -1, -1), new THREE.Vector3(1, 1, 1)) }] }, bounds: new THREE.Sphere(new THREE.Vector3(), 2) };
  scene.add(group);
  core.phase("near", renderer, scene, camera);
  const bindings = calls.filter((c) => c[0] === "world_texture");
  assert.deepEqual(bindings.filter(c=>c[2]>=6).map((c) => c[2]), [6, 7, 8], "every active sampler gets a binding, including the zero-weight layer");
  const bound = bindings.filter(c=>c[2]>=6).map((c) => textures[c[3]]);
  assert.equal(bound[0], first);
  assert.equal(bound[2], last);
  assert.ok(bound[1].isDataTexture);
  assert.deepEqual([...bound[1].image.data], [0, 0, 0, 0], "the same empty color texel three binds for null");
  assert.equal(material.visible, false, "the phase takes the complete terrain draw");
  core.phase("near", renderer, scene, camera);
  assert.equal(calls.filter((c) => c[0] === "world_texture").length, bindings.length, "registered once, with no per-frame texture allocation");
});
