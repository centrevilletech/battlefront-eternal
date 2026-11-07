import { standinResources } from "./support/rust-standin.mjs";
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { ROLE as R, META as M, UI, UI_CAPACITY } from "../src/rust-render/frame-layout.js";
import { ownedBasicEffect } from "../src/rust-render/effect-material.js";

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
  standinResources(renderer,calls);
  const core = new RustRender(graphics, { memory }, BfeRenderer);
  calls.length = 0; // These assertions cover phase calls after source prewarming.
  core.sceneItems = core.renderer.renderLists?.get(core.graphics.scene, 0);
  const setSegments = (values) => { segments.length = 0; segments.push(...values); new Uint32Array(memory.buffer, region("transparent_segments"), values.length).set(values); };
  return { core, calls, graphics, renderer, scene, camera, memory, list, setSegments, setTarget: (t) => { target = t; } };
}

test("the passes are made in the renderer's order, each in its phase", () => {
  const { core } = bridge();
  assert.deepEqual(core.passes.map((p) => p.name), ["world", "instances", "far", "terrain", "transparent", "effects", "foliage", "view", "hud", "water", "near", "canopies", "sea", "motion", "shadow", "clouds", "underwater", "mirrorCover", "fog", "lines"]);
  const phases = Object.fromEntries(Object.entries(core.phases).map(([name, passes]) => [name, passes.map((p) => p.name)]));
  assert.deepEqual(phases, { far: ["far"], near: ["world", "instances", "terrain", "foliage", "near", "canopies", "sea", "mirrorCover"], transparent: ["transparent", "effects", "water", "fog"], view: ["view"], hud: ["hud"], mirror: ["world", "instances", "terrain", "foliage", "near", "canopies", "sea", "mirrorCover"], mirrorTransparent: ["transparent", "effects", "water", "fog"], farMirror: ["far"], probe: ["far"], motion: ["motion"], shadow: ["shadow"], image: [], skyClouds: ["clouds"], underwater: ["underwater"], lines: ["lines"] });
  assert.throws(() => core.addPass({ name: "x", phase: "nowhere" }), /no phase nowhere/);
});

test("near producers publish one semantic packet without drawing or resuming hooks", () => {
  const {core,calls,renderer,scene,camera}=bridge(),produced=[];
  core.frame.begin(true);calls.length=0;
  core.addPass({name:"a",phase:"near",draw:ctx=>{produced.push(ctx.records);ctx.records+=2;},afterPhase(){throw Error("obsolete continuation");}});
  core.addPass({name:"b",phase:"near",draw:ctx=>{produced.push(ctx.records);ctx.records++;}});
  core.recording={keepFrames:1,frames:[],textures:[],calls:[]};
  const ctx=core.publishView("near",renderer,scene,camera);
  assert.deepEqual(produced,[0,2]);assert.equal(ctx.records,3);
  assert.equal(core.recording.frames[0].records,3);assert.equal(core.recording.keepFrames,0);
  assert.equal(core.recording.frames[0].input.length,core.recordsAt+3*core.stride);
  assert.notEqual(core.recording.frames[0].input.buffer,core.input.buffer);
  assert.deepEqual(calls.filter(c=>c[0]==="frame_publish_frame"),[["frame_publish_frame",R.NEAR]]);
  assert.equal(calls.some(c=>["frame","transparent_frame","resetState","setRenderTarget"].includes(c[0])),false);
  assert.equal(core.frameMeta[R.NEAR*M.LEN+M.RECORDS],3);
  assert.deepEqual([...core.input.subarray(0,16)],camera.projectionMatrix.elements);
  core.frame.cancel();
});

test("view preparation order cannot select execution order", () => {
  const {core,calls,renderer,scene,camera}=bridge();core.frame.begin(true);calls.length=0;
  core.publishView("transparent",renderer,scene,camera);
  core.publishView("far",renderer,scene,camera);
  assert.deepEqual(calls.filter(c=>c[0]==="frame_publish_frame").map(c=>c[1]),[R.TRANSPARENT,R.FAR]);
  assert.equal(calls.some(c=>["far_frame","transparent_frame","setRenderTarget"].includes(c[0])),false);
  core.frame.cancel();
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
  const material = ownedBasicEffect('basic', { visible: true, transparent: true, color: 0x336699, opacity: 0.25 });
  const holo = new THREE.Mesh(new THREE.BufferGeometry(), material), other = new THREE.Object3D();
  holo.position.set(1, 2, 3);
  holo.updateMatrixWorld(true);
  let prepared = 0;
  holo.onBeforeRender = (_renderer, _scene, view) => { assert.equal(view, camera); prepared++; holo.position.x = 9; holo.updateMatrixWorld(true); };
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
  pass.take(list.transparent,{renderer,scene,camera});
  assert.deepEqual(list.transparent.map((item) => item.object), [other], "unclaimed items remain visible to the near-scene completeness check");
  const r = core.transparentObjects;
  assert.equal(r[0], 1);
  assert.deepEqual([...r.subarray(1, 6)], [7, 0, 2, 0.75, holo.id]);
  assert.deepEqual([...r.subarray(6, 22)], holo.matrixWorld.elements);
  assert.equal(prepared, 1);
  assert.equal(r[18], 9, "the owned object's draw hook runs before its matrix is published");
  assert.deepEqual([...r.subarray(22, 26)], [material.color.r, material.color.g, material.color.b, 0.25]);
});





test("the ground admits authored native descriptors and publishes visibility", () => {
  const { core, calls, renderer, scene, camera } = bridge();
  const group = new THREE.Group(); group.userData.worldOrder = 8; scene.add(group);
  const record = { terrain: true, vertexCount: 3, stride: 3, attributes: ["position"] };
  const vertices = Float32Array.of(0,0,0, 1,0,0, 0,1,0), index = Uint32Array.of(0,1,2);
  core.byName.terrain.admit(group,record,vertices,index,new Uint8Array(),Int32Array.of(1,-1,2,3,4,4,4,4,4,4,4,4,-1));
  core.publishView("near",renderer,scene,camera);
  const id = core.byName.terrain.grounds[0].id;
  const add = calls.find(c=>c[0]==="static_source");
  assert.equal(JSON.parse(add[1]).terrain,true);
  assert.deepEqual([...add[2]],[...vertices]);
  assert.equal(group.material,undefined);
  calls.length=0; core.publishView("near",renderer,scene,camera);
  assert.equal(calls.some(c=>c[0]==="static_world"),false);
  group.visible=false; core.publishView("near",renderer,scene,camera);
  assert.deepEqual(calls.find(c=>c[0]==="world_shown"),["world_shown",id,false]);
});

test("static world admissions retain handles and replace resources without descriptor reads", () => {
  const { core, calls, renderer, scene, camera } = bridge();
  const node = new THREE.Object3D(); node.userData.worldOrder=12; scene.add(node);
  for (const name of ["geometry","material"]) Object.defineProperty(node,name,{get(){throw Error("descriptor read");}});
  const record={flags:1,vertexCount:3,stride:3,attributes:["position"]};
  const vertices=Float32Array.of(0,0,0,1,0,0,0,1,0), index=Uint32Array.of(0,1,2);
  const world=core.byName.world;
  world.admit(node,record,vertices,index,null,null);
  core.publishView("near",renderer,scene,camera);
  const id=world.meshes[0].id;
  assert.deepEqual([...calls.find(c=>c[0]==="static_source")[2]],[...vertices]);
  calls.length=0;core.publishView("near",renderer,scene,camera);
  assert.equal(calls.some(c=>c[0]==="static_world"),false);
  world.admit(node,{...record,flags:0},vertices,index,null,null);
  assert.deepEqual(calls.find(c=>c[0]==="world_shown"),["world_shown",id,false]);
  core.publishView("near",renderer,scene,camera);
  assert.notEqual(world.meshes.at(-1).id,id);
  assert.equal(world.meshes[0].stale,true);
});

test("overlay bursts clip later packets without a frame exception", () => {
  const {core,calls,renderer,scene,camera}=bridge();core.frame.begin(true);core.frame.uiStage(UI.OVERLAY);calls.length=0;
  for(let i=0;i<128;i++){
    const ctx=core.frame.context("hud",scene,camera,null,null);
    core.frame.publish("hud",ctx);
  }
  assert.equal(calls.filter(c=>c[0]==="frame_publish_frame").length,UI_CAPACITY.OVERLAY);
  assert.equal(calls.filter(c=>c[0]==="frame_overflow_frame").length,128-UI_CAPACITY.OVERLAY);
  core.frame.cancel();
});

// Native descriptor and backend fixtures cover painted/empty sampler bytes;
// the host adapter above must never reconstruct a DataTexture for those slots.
