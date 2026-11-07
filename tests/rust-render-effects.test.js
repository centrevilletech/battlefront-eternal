import {sceneComponent,sceneProperty,sceneTransform} from "../src/rust-render/scene-changes.js";
import {prepareSceneFixture} from "./support/rust-standin.mjs";
import { ROLE as R } from "../src/rust-render/frame-layout.js";
import {wakeFixture} from './helpers/surfaces.js';
import './fixtures/native-render-descriptors.js';
import {actorAsset,glowLook,glowNode} from '../src/actor-resources.js';
import { ownedBasicEffect } from '../src/rust-render/effect-material.js';
import { standinResources } from "./support/rust-standin.mjs";
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { actorMaterial } from "../src/materials.js";
import { EFFECT } from "../src/rust-render/effect-layout.js";
import { Draw as D, Effect as E } from "../src/presentation/layout.js";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");
const { standinModule } = await import("./support/rust-standin.mjs");
const { ParticleRenderer, particleRenderers, STREAK } = await import("../src/particles.js");
const { GpuEffects } = await import("../src/effects-gpu.js");
const { Lightning, classicLightning } = await import("../src/lightning.js");

const STATS = [1, 2, 3, 4, 5, 1, 2, 0, 0, 0, 0, 0, 0, 0];
const GL = { ACTIVE_UNIFORMS: 0x8b86, ACTIVE_ATTRIBUTES: 0x8b89, SHADER_TYPE: 0x8b4f, VERTEX_SHADER: 0x8b31, FRAGMENT_SHADER: 0x8b30 };
// The frame's draws (effects.rs DRAW_*).
const DRAW_AT = 8 + 8 * 4, DRAW_STRIDE = 12;

// Programs as three's report their active uniforms and attributes (and,
// for a side of a double-sided material, its vertex shader's defines).
const program = (uniforms, attributes, vertex = "vertex") => ({ uniforms, attributes, shaders: [{ type: GL.VERTEX_SHADER, source: vertex }, { type: GL.FRAGMENT_SHADER, source: "fragment" }] });
const CPU_PROGRAM = program(["projectionMatrix", "modelViewMatrix", "map", "fade", "sceneDepth", "softParticles", "depthResolution", "cameraRange"],
  ["corner", "iPosition", "iColor", "iShape", "iAxis", "iAxis2", "iFrame"]);
const ANALYTIC_PROGRAM = program([...CPU_PROGRAM.uniforms, "tracks", "emitters", "trackRows", "emitterRows"],
  ["corner", "spawnPosition", "spawnVelocity", "spawnColor", "spawnShape", "spawnMeta"]);
const BASIC = ["projectionMatrix", "modelViewMatrix", "mapTransform", "diffuse", "opacity", "map"];

// The bridge over stand-ins for the WASM renderer (its memory, and a log of
// the calls made to it), three's renderer (its render lists, empty) and its
// context (enough of it for a program's uniforms, attributes and shaders).
function bridge() {
  const next = { program: 0, texture: 0, batch: 0, owner: 0, mesh: 0, model: 0, geometry: 0 };
  // The shared stand-in for the WASM renderer (every pass's views), with the
  // effects' registration calls as this test reads them.
  const { BfeRenderer, memory, calls } = standinModule({
    methods: ({ calls }) => ({
      transparent_bucket_shown() {},
      add_texture() { calls.push(["add_texture"]); return next.texture++; },
      set_far_map(texture) { calls.push(["set_far_map", texture]); },
      add_program(kind, vs, fs, attributes, uniforms) { calls.push(["add_program", kind, vs, attributes, uniforms]); return next.program++; },
      effect_batch(...args){calls.push(["effect_batch",...args]);return next.batch++;},
      effect_basin(...args){calls.push(["effect_basin",...args]);return Float64Array.of(next.model++,0,0,-6);},
      effect_mesh(...args){calls.push(["effect_mesh",...args]);return next.mesh++;},
      effect_multiplier(){return 1;},
      effect_fixed(){return false;},
      fx_add_owner(tracks, rows) { calls.push(["fx_add_owner", tracks, rows]); return next.owner++; },
      fx_add_batch(program, texture, blend, corner, attributes, owner) { calls.push(["fx_add_batch", program, texture, [...blend], corner, [...attributes], owner]); return next.batch++; },
      fx_batch_texture(batch, texture) { calls.push(["fx_batch_texture", batch, texture]); },
      fx_batch_values(batch, values) { calls.push(["fx_batch_values", batch, values]); },
      fx_batch_interleaved(batch) { calls.push(["fx_batch_interleaved", batch]); },
      fx_batch_side(batch, side) { calls.push(["fx_batch_side", batch, side]); },
      fx_batch_sampler(batch, name, unit, texture) { calls.push(["fx_batch_sampler", batch, name, unit, texture]); },
      fx_batch_base(batch, attributes, vertices, data, index) { calls.push(["fx_batch_base", batch, [...attributes], vertices, data.length, [...index]]); },
      fx_add_mesh(texture, blend, flags, func, sizes, values) { calls.push(["fx_add_mesh", texture, [...blend], flags, func, [...sizes], values]); return next.mesh++; },
      fx_mesh_pass(mesh, program, side, locations) { calls.push(["fx_mesh_pass", mesh, program, side, [...locations]]); },
      fx_mesh_texture(mesh, texture) { calls.push(["fx_mesh_texture", mesh, texture]); },
      fx_add_model(texture, blend, flags, func, values, sphere) { calls.push(["fx_add_model", texture, [...blend], flags, func, values, [...sphere]]); return next.model++; },
      fx_model_pass(model, program, side, geometry) { calls.push(["fx_model_pass", model, program, side, geometry]); },
      fx_model_sampler(model, name, unit, texture) { calls.push(["fx_model_sampler", model, name, unit, texture]); },
      add_geometry() { calls.push(["add_geometry"]); return next.geometry++; },
      geometry_buffer() { return 0; },
      geometry_pointer() {},
      geometry_index() {},
      geometry_group() {},
      sort() {},
    }),
  });
  const gl = {
    ...GL,
    getProgramParameter: (p, what) => (what === GL.ACTIVE_UNIFORMS ? p.uniforms.length : p.attributes.length),
    getActiveUniform: (p, i) => ({ name: p.uniforms[i] }),
    getActiveAttrib: (p, i) => ({ name: p.attributes[i] }),
    getAttribLocation: (p, name) => p.attributes.indexOf(name),
    getAttachedShaders: (p) => p.shaders,
    getShaderParameter: (s) => s.type,
    getShaderSource: (s) => s.source,
  };
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera();
  camera.updateMatrixWorld(true);
  const sceneTarget = { name: "scene", width: 800, height: 600 }, farTarget = { texture: new THREE.Texture() };
  const properties = new Map();
  const list = { opaque: [], transmissive: [], transparent: [] };
  const renderer = {
    renderLists: { get: () => list },
    getContext: () => gl,
    properties: { get: (o) => { if (!properties.has(o)) properties.set(o, { __webglTexture: {} }); return properties.get(o); } },
    compile() {},
    initTexture() {},
    initRenderTarget() {},
    shadowMap: { enabled: false },
    getRenderTarget: () => sceneTarget,
    setRenderTarget() {},
    resetState() { calls.push(["resetState"]); },
  };
  const background = new THREE.Object3D();
  list.opaque.push({ object: background });
  const graphics = { renderer, scene, camera, sceneTarget, farTarget, background, settings: { get: () => false }, upscaling: "off", cullRoots: [], playerRoot: null };
  standinResources(renderer,calls);
  const core = prepareSceneFixture(new RustRender(graphics, { memory }, BfeRenderer));
  graphics.rust=core;
  core.sceneItems = core.renderer.renderLists?.get(core.graphics.scene, 0);
  // A frame's transparent phase, as the transparent pass runs it.
  const frame=()=>{return core.publishView("transparent",renderer,scene,camera);};
  // three has drawn a material once: its program (and its programs by side).
  const drawn = (material, current, programs = [current]) => {
    if(!material)return;
    const props = renderer.properties.get(material);
    props.currentProgram = { program: current };
    props.programs = new Map(programs.map((p, i) => [i, { program: p }]));
    props.__version = material.version;
  };
  const entry = (d) => [...core.fxInput.subarray(DRAW_AT + d * DRAW_STRIDE, DRAW_AT + (d + 1) * DRAW_STRIDE)];
  return { core, calls, scene, camera, renderer, list, frame, drawn, entry, fx: core.byName.effects };
}

await (await import("../src/environment/module.js")).loadEnvironment();
const { Destruction } = await import("../src/destruction.js");

test("the effects are the transparent phase's second pass, after the map's blended buckets", () => {
  const { core } = bridge();
  assert.deepEqual(core.phases.transparent.map((p) => p.name), ["transparent", "effects", "water", "fog"]);
});

test("a CPU batch registers an owned program before any three draw and writes each frame with its key", () => {
  const { core, calls, scene, frame, drawn, entry, fx } = bridge();
  const particles = new ParticleRenderer(scene);
  assert.ok(particleRenderers.get(scene).includes(particles));
  const batch = particles.batch(new THREE.Texture(), "additive", "bolt");
  const fill = (n) => { particles.begin(); for (let i = 0; i < n; i++) batch.push(i, 2 * i, 3 * i, 1, 1, 1, 0.5, 0.2, 0, STREAK, 4, [0, 0, 1], [0.5, 0, 0]); particles.end(); };
  fill(2);
  assert.equal(batch.mesh.visible, true);
  // Source ownership needs no prior browser compile or draw.
  frame();
  const added = calls.find(c=>c[0]==="effect_batch");
  assert.ok(added,"registered cold without a browser descriptor");
  assert.equal(added[1],EFFECT.CPU.ID);assert.equal(added[2],3);
  assert.equal(batch.geometry,undefined);assert.equal(batch.material,undefined);
  assert.equal(core.fxInput[0],1);assert.equal(batch.external,undefined);
  assert.equal(batch.mesh.visible,true);
  fill(3);
  assert.equal(batch.mesh.visible, true);
  frame();
  const data = core.fxData;
  assert.equal(core.fxInput[0], 1, "one draw");
  // Kind, batch, instances, from, where, capacity; then three's key: group
  // order, render order, centre (the origin: no positions), id.
  assert.deepEqual(entry(0), [0, 0, 3, 0, 0, batch.capacity, 0, 10, 0, 0, 0, batch.mesh.id]);
  assert.deepEqual([...data.subarray(0, 9)], [0, 0, 0, 1, 2, 3, 2, 4, 6], "iPosition, its three instances");
  assert.deepEqual([...data.subarray(9, 13)], [1, 1, 1, 0.5], "then iColor");
  assert.equal(fx.draws, 1);
  assert.equal(fx.instances, 3);
  });

test("a mirror's render draws what blends too: the transparent phase for the mirror's camera, after its near phase", () => {
  const { core, calls, scene, renderer, list, drawn, entry } = bridge();
  const particles = new ParticleRenderer(scene);
  const batch = particles.batch(new THREE.Texture(), "additive", "bolt");
  const fill = (n) => { particles.begin(); for (let i = 0; i < n; i++) batch.push(i, 2 * i, -3 * i - 5, 1, 1, 1, 0.5, 0.2, 0, STREAK, 4, [0, 0, 1], [0.5, 0, 0]); particles.end(); };
  fill(2);
  drawn(batch.material, CPU_PROGRAM);
  core.publishView("transparent", renderer, scene, core.graphics.camera);
  assert.equal(batch.external, undefined, "registered by the frame's own render");
  // water.js's mirror: a camera of its own drawing into a target of its own.
  const mirror = new THREE.PerspectiveCamera();
  mirror.userData.mirror = true;
  sceneTransform(mirror.position.set(0, -2, 0));
  mirror.updateMatrixWorld(true);
  const reflection = { name: "reflection", width: 400, height: 300 };
  const target = renderer.getRenderTarget;
  renderer.getRenderTarget = () => reflection;
  try {
    fill(3);
    calls.length = 0;
    // The far scene's image is the last opaque item: the mirror's
    // transparent phase follows it, in the mirror's render.
    const ctx=core.publishView("mirror", renderer, scene, mirror);
    core.byName.transparent.prepare(ctx);core.publishView("mirrorTransparent",renderer,scene,mirror);
    assert.ok(calls.some(c=>c[0]==="frame_aux_frame"&&c[2]===R.MIRROR0_TRANSPARENT), "the explicit Rust mirror phase includes transparency");
  } finally {
    renderer.getRenderTarget = target;
  }
  const roles=calls.filter(c=>c[0]==="frame_aux_frame").map(c=>c[2]);
  assert.deepEqual(roles,[R.MIRROR0_NEAR,R.MIRROR0_TRANSPARENT]);
  assert.equal(core.fxInput[0], 1, "the effects drawn in the reflection");
  assert.deepEqual(entry(0).slice(0, 3), [0, 0, 3]);
});

test("an analytic batch: its owner's emitters, and its instances from the first changed", () => {
  const { core, calls, scene, frame, drawn, entry } = bridge();
  const particles = new ParticleRenderer(scene);
  const def = { geometry: { type: "particle", texture: "spark", blend: "additive" }, transformer: { lifetime: 1, position: [], size: [], color: [] }, registerStep: true };
  const gpu = new GpuEffects({ spark: [def] }, particles);
  assert.equal(particles.gpu, gpu);
  const texture = new THREE.Texture();
  const effect = new Float64Array(E.STRIDE);
  effect.set([1, 2, 3], E.POSITION); effect[E.QUATERNION + 3] = 1;
  effect[E.TINT] = effect[E.SCALE] = 1; effect[E.TIME] = 5;
  // Synthetic native output keeps the renderer's record upload covered
  // without retaining a second object-based particle interface.
  const made = Array.from({ length: 4 }, (_, i) => {
    const row = new Float64Array(D.STRIDE);
    row[D.PARTICLE] = i + 1; row[D.POSITION] = row[D.BIRTH] = i;
    row.set([1, 1, 1, 1], D.COLOR); row[D.SIZE] = row[D.LIFE] = 1;
    return row;
  });
  let batch = null;
  const fill = (list) => {
    particles.begin(); gpu.begin();
    const slot = gpu.registerRecord(effect, 0);
    batch = gpu.batch(def, texture);
    for (const p of list) batch.pushRecord(p, 0, slot, gpu.definitions.get(def), gpu.epochs[slot]);
    particles.end(); gpu.end();
  };
  fill(made);
  drawn(batch.material, ANALYTIC_PROGRAM);
  frame();
  assert.ok(calls.find((c) => c[0] === "fx_add_owner"), "its owner registered");
  assert.equal(calls.find((c) => c[0] === "effect_batch")[6], 0, "with its owner");
  // Drawn: every instance, and the emitter row of the effect playing.
  fill(made);
  frame();
  const input = core.fxInput, data = core.fxData;
  assert.deepEqual([...input.subarray(8, 12)], [0, gpu.emitterRows, 1, 0], "the owner's rows, one written at 0");
  assert.deepEqual([...data.subarray(0, 3)], [1, 2, 3], "the effect's place");
  assert.deepEqual(entry(0).slice(0, 5), [0, 0, 4, 4, 12], "unchanged instances were already uploaded in the first frame");
  // Nothing changed: none written.
  fill(made);
  frame();
  assert.deepEqual(entry(0).slice(2, 4), [4, 4]);
  // The third changed (over two frames, the first not drawn): from it on.
  const changed = new Float64Array(made[2]); changed[D.PARTICLE] = 5; changed[D.BIRTH] = 9;
  fill([made[0], made[1], changed, made[3]]);
  fill([made[0], made[1], changed, made[3]]);
  frame();
  assert.deepEqual(entry(0).slice(2, 4), [4, 2]);
  assert.deepEqual([...data.subarray(12, 15)], [2, 0, 0], "spawnPosition of the third, first");
});

test('native glow models share source handles and publish current values and matrices',()=>{
 const {core,calls,scene,frame,entry}=bridge(),asset=actorAsset({kind:'plane',width:1,height:1}),look=glowLook(null,0xffffff,.75);look.color.setRGB(1,.5,.25);
 const group=new THREE.Group(),part=glowNode(asset,look),other=glowNode(asset,look);sceneTransform(group.position.set(1,2,-6));group.add(part,other);scene.add(group);scene.updateMatrixWorld(true);frame();
 assert.equal(core.fxInput[0],2);assert.equal(entry(0)[1],entry(1)[1]);assert.deepEqual(Array.from(core.fxMatrices.slice(0,16)),part.matrixWorld.elements);assert.equal(calls.filter(c=>c[0]==='actor_source').length,1);assert.equal(part.geometry,undefined);assert.equal(look.isMaterial,undefined);
 const row=EFFECT.ACTOR_GLOW;assert.deepEqual(Array.from(core.fxData.slice(row.DIFFUSE,row.DIFFUSE+3)),[1,.5,.25]);assert.equal(core.fxData[row.OPACITY],.75);sceneProperty(other,"visible",false);frame();assert.equal(core.fxInput[0],1);sceneProperty(group,"visible",false);frame();assert.equal(core.fxInput[0],0);
});

test('native glow admission carries shadow policy as a cold input',()=>{
 const {calls,scene,frame}=bridge(),node=glowNode(actorAsset({kind:'plane'}),glowLook(null));sceneComponent(node.position,"z","=",-5);scene.add(node);scene.updateMatrixWorld(true);frame();assert.ok(calls.some(c=>c[0]==='actor_effect'&&c[2]===EFFECT.ACTOR_GLOW.ID));assert.equal(calls.some(c=>c[0]==='fx_add_model'),false);
});

test("glow and command-post shaders keep separate owned uniform caches", async () => {
  const { core } = bridge();
  const { warmEffectPrograms } = await import("../src/rust-render/effect-material.js");
  const settled = [];
  await warmEffectPrograms(core, async () => { settled.push(core.ownedPrograms.size); });
  assert.deepEqual(settled, settled.map((_, i) => i + 1), "one program linked between settles");
  assert.ok(!core.ownedPrograms.has("effects/shock-s0") || !core.ownedPrograms.has("effects/shock-s0-shadow"), "one shadow setting at a time");
  const glow = core.ownedPrograms.get("effects/glow-basic-s0");
  const beam = core.ownedPrograms.get("effects/basic-s0");
  assert.notEqual(glow.id, beam.id);
});

test("an effects pass error stops the required renderer", () => {
  const { core, scene, frame, drawn } = bridge();
  const particles = new ParticleRenderer(scene);
  const batch = particles.batch(new THREE.Texture(), "additive");
  const fill = () => { particles.begin(); batch.push(0, 0, 0, 1, 1, 1, 1, 0.2, 0, STREAK, 1, [0, 0, 1], [1, 0, 0]); particles.end(); };
  fill();
  drawn(batch.material, CPU_PROGRAM);
  frame();
  fill();
  assert.equal(batch.external, undefined);
  // A batch the pass can't read.
  core.rust.frame_aux_frame=()=>false;core.rust.frame_error_frame=()=>"GPU draw failure";
  assert.throws(() => frame(), /GPU draw failure/);
  assert.equal(batch.external, undefined, "ownership is not handed back on failure");
});

test("the scene's depth: asked for as three starts the render, before the first draw whose onBeforeRender would take it, and taken there", async () => {
  const { JetpackVolume } = await import("../src/jetpack-volume.js");
  const { EnvironmentVolume } = await import("../src/environment-volume.js");
  const { ParticleDepth, particleDepth } = await import("../src/particle-depth.js");
  const { core, scene, camera, renderer, list, frame, drawn, fx } = bridge();
  core.shadowsOn = () => true;
  core.writeShadow = () => {};
  const saved = { ...particleDepth }, map = particleDepth.map.value;
  try {
    const depth = new ParticleDepth(renderer);
    depth.copy = () => { depth.copies++; };
    // The pass asks the page's helper (made here after the bridge).
    fx.unrequest?.();
    fx.unrequest = particleDepth.requester(scene, (c) => fx.requestDepth(c));
    // A flame (its onBeforeRender asks for the depth whatever the setting) and a CPU batch (only while soft particles are on).
    const volume = new JetpackVolume({ scene, count: 2 });
    const nozzle = (x) => ({ position: new THREE.Vector3(x, 1, -5), direction: new THREE.Vector3(0, -1, 0), radius: 0.03 });
    const particles = new ParticleRenderer(scene);
    const batch = particles.batch(new THREE.Texture(), "additive", "bolt");
    const fill = (n) => { particles.begin(); for (let i = 0; i < n; i++) batch.push(i, 1, -6, 1, 1, 1, 0.5, 0.2, 0, STREAK, 4, [0, 0, 1], [0.5, 0, 0]); particles.end(); };
    volume.update(1.5, 1, [nozzle(-0.2), nozzle(0.2)]);
    fill(2);
    list.transparent.push({ object: volume.mesh, material: volume.mesh.material, group: null });
    drawn(volume.mesh.material, program(["projectionMatrix", "viewMatrix", "cameraPosition", "flameTime", "power", "hasDepth", "noiseField", "sceneDepth", "depthResolution", "cameraRange"],
      ["position", "nozzle", "direction", "bend", "nozzleRadius", "phase"]));
    drawn(batch.material, CPU_PROGRAM);
    frame();
    list.transparent.length = 0;
    assert.equal(fx.records.length, 2);
    const target = core.graphics.sceneTarget;
    // A frame: graphics.js begin(), three's render starting (the scene's onBeforeRender), the transparent phase.
    const render = (soft, n = 2) => {
      volume.update(1.6, 1, [nozzle(-0.2), nozzle(0.2)]);
      fill(n);
      depth.begin(target, camera, soft);
      core.prepareScene(renderer,scene,camera,target);
      scene.onBeforeRender(renderer, scene, camera, target);
    };
    render(false);
    assert.equal(depth.armed, true, "the flame asks whatever the setting");
    assert.deepEqual([depth.group.renderOrder, depth.copier.renderOrder], [0, 10], "just before it: its group order and render order");
    frame();
    assert.equal(depth.copies, 1, "the copy is published as a sorted Rust item");
    assert.equal(core.fxInput[DRAW_AT + 2 * DRAW_STRIDE], 3);
    assert.equal(volume.values[EFFECT.FLAME.HAS_DEPTH], 1, "yet knows it will have it");
    particleDepth.sortedCopy(renderer, camera);
    assert.equal(depth.copies, 1, "taken at the copier");
    assert.equal(fx.requested.length, 0, "the frame's requests used");
    // Soft particles on: the batch asks too.
    render(true);
    assert.equal(fx.records.filter((r) => r.requested).length, 2);
    frame();
    particleDepth.sortedCopy(renderer, camera);
    assert.equal(depth.copies, 1);
    // A batch shown only after three started the render (asked for by none): the copy taken at once, the copier's none.
    render(true, 0);
    assert.equal(fx.records.filter((r) => r.requested).length, 1);
    fill(2);
    frame();
    assert.equal(depth.copies, 1, "taken as the phase starts");
    particleDepth.sortedCopy(renderer, camera);
    assert.equal(depth.copies, 1);
    // A volume rendering with the depth at once (its own pass): a copy of its own as the phase starts; the copier's still taken.
    const wakes = wakeFixture(), steam = new EnvironmentVolume(scene, 2, wakes);
    steam.update(1, [{ position: new THREE.Vector3(0, 0, -6), direction: new THREE.Vector3(0, 1, 0), extent: new THREE.Vector3(6, 5.5, 6), kind: 0 }]);
    list.transparent.push({ object: steam.mesh, material: steam.mesh.material, group: null });
    drawn(steam.mesh.material, program(["volumeMap", "sceneDepth", "volumeResolution", "depthResolution", "cameraRange", "enabled"], ["position"]));
    frame();
    list.transparent.length = 0;
    const rendered = [];
    Object.assign(renderer, { getClearColor: (c) => c, getClearAlpha: () => 1, setClearColor() {}, clear() {}, render: (s) => rendered.push(s) });
    render(false);
    frame();
    assert.equal(depth.copies, 2, "the volumes' early copy and the published sorted copy");
    assert.equal(rendered.length, 0, "volumes draw through Rust");
    assert.equal(steam.values[EFFECT.VOLUME_COMPOSITE.ENABLED], 1);
    particleDepth.sortedCopy(renderer, camera);
    assert.equal(depth.copies, 2, "and the copier's for the draws after it");
  } finally {
    Object.assign(particleDepth, saved);
    particleDepth.map.value = map;
  }
});


test('new native glow sources are admitted before their first main view',()=>{
 const {core,calls,scene,camera,renderer,frame}=bridge(),target=core.graphics.sceneTarget;scene.onBeforeRender(renderer,scene,camera,target);
 const holder=new THREE.Group(),node=glowNode(actorAsset({kind:'plane'}),glowLook(null));sceneComponent(node.position,"z","=",-5);scene.add(holder);holder.add(node);scene.updateMatrixWorld(true);core.prepareScene(renderer,scene,camera,target);scene.onBeforeRender(renderer,scene,camera,target);
 assert.ok(calls.some(c=>c[0]==='actor_effect'));assert.equal(node.geometry,undefined);frame();assert.equal(core.fxInput[0],1);
});

test("native basin and tier descriptors share an ordered opaque copy and restore the disabled appearance", async () => {
  const { FountainWater } = await import("../src/fountain-water.js");
  const { TIER } = await import("../src/fountain-jets.js");
  const { core, calls, scene, list, frame, drawn, renderer } = bridge();
  // Its onBeforeRender copies the frame so far (fountain-water.js capture): three's blit, stood in.
  const blits = [];
  renderer.state = { bindFramebuffer() {} };
  renderer.getContext().blitFramebuffer = (...args) => blits.push(args);
  const basinGeometry = sceneTransform(new THREE.CircleGeometry(2, 12).rotateX(-Math.PI / 2)).translate(0, 0, -6);
  const basin = new THREE.Mesh(basinGeometry, new THREE.MeshBasicMaterial());
  basin.name = "nab2_prop_fountain_water";
  scene.add(basin);
  const water = new FountainWater(core.graphics, [{ texture: "nab2_prop_fountain_water", center: [0, 0, -6], radius: 2 }], wakeFixture(), new THREE.Texture(), TIER);
  water.setEnabled(true);
  water.update(1.5);
  assert.equal(water.material,undefined);assert.equal(water.tier.geometry,undefined);
  assert.equal(basin.visible,false,'the enhanced native surface owns this appearance');
  frame();
  const models=calls.filter(c=>c[0]==='effect_basin');assert.equal(models.length,2,'one coarse native admission per authored mesh or tier recipe');
  assert.equal(core.fxInput[0],2);assert.equal(water.values[EFFECT.FOUNTAIN_WATER.WATER_TIME],1.5);
  assert.equal(calls.filter(c=>c[0]==='copyTarget').length,1,'ordered shared opaque copy');
  frame();assert.equal(calls.filter(c=>c[0]==='effect_basin').length,2,'resources stay retained');
  water.setEnabled(false);frame();assert.equal(core.fxInput[0],0);assert.equal(basin.visible,true);

});

test('effect capacity clips excess owners without stopping the frame', () => {
  const {fx, frame}=bridge();
  for(let i=0;i<9;i++)fx.ownerList.push({gpu:{count:0,emitterRows:0,data:new Float32Array(0)},id:i});
  assert.doesNotThrow(frame);assert.ok(fx.clipped>0);
});
