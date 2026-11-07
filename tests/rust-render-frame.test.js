import { presentationModule } from "./helpers/page-presentation.js";
import { bindInterfaceBatch } from "../src/interface/batch.js";
bindInterfaceBatch(presentationModule());
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { Upscaler } from "../src/upscaler.js";
import { STATE } from "../src/rust-render/post-layout.js";
import { actorMaterial } from "../src/materials.js";
import { ownedEffectMaterial, ownedBasicEffect, ownedEffectProgram } from "../src/rust-render/effect-material.js";

// The bridge reads its flags from the page's query.
globalThis.location ??= { search: "" };
const { RustRender } = await import("../src/rust-render/index.js");

import { programLayouts } from "../src/render-programs.js";

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

function waterMaterial(key, overrides = {}, options = {}) {
  const uniforms = {};
  for (const u of programLayouts[key].uniforms) {
    let value;
    if (u.size > 1) value = Array.from({length:u.size}, () => new THREE.Vector4());
    else if (u.type === 1 || u.type === 5) value = 0;
    else if (u.type === 2) value = new THREE.Vector2();
    else if (u.type === 3) value = new THREE.Vector3();
    else if (u.type === 4) value = new THREE.Vector4();
    else if (u.type === 9) value = new THREE.Matrix3();
    else if (u.type === 16) value = new THREE.Matrix4();
    else value = null;
    uniforms[u.name] = {value};
  }
  return ownedEffectMaterial(key, { uniforms: {...uniforms, ...overrides}, transparent:true, side:THREE.DoubleSide, ...options });
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
function partMaterial() {
  const material=actorMaterial('dynamic',{map:new THREE.Texture()});
  material.uniforms.sunScale.value=0.5;material.uniforms.ambientPull.value.set(0.1,0.2);material.uniforms.brightness.value=0.9;
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
  assert.ok(calls.some(c=>c[0]==="resolveTarget"),"the Rust resource owner resolves the target");
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
  assert.ok(calls.some(c=>c[0]==="resolveTarget"),"the Rust resource owner resolves the target");
});

test("the view scene retains ownership when an actor material changes", () => {
  const {core,renderer}=bridge(),scene=new THREE.Scene(),camera=new THREE.PerspectiveCamera();
  const mesh=new THREE.Mesh(partGeometry(),partMaterial());scene.add(mesh);
  assert.equal(core.cover('view',renderer,scene,camera),true);
  mesh.material=new THREE.MeshBasicMaterial();
  core.byName.view.draw({scene,camera,renderer,records:0});
  assert.match(core.errors.join("\n"),/view part whose material no descriptor covers/);assert.equal(core.failure,undefined);
});

test("an unclaimed view object is skipped and reported, the owned parts drawn, owned materials still hidden",()=>{
 const {core,renderer}=bridge(),scene=new THREE.Scene(),camera=new THREE.PerspectiveCamera();
 const actor=new THREE.Mesh(partGeometry(),partMaterial()),effect=new THREE.Mesh(partGeometry(),new THREE.ShaderMaterial({transparent:true}));scene.add(actor,effect);
 assert.equal(core.cover('view',renderer,scene,camera),true);
 assert.equal(actor.material.visible,false);assert.equal(effect.material.visible,true);
 assert.match(core.errors.join("\n"),/Not drawn: .* Mesh material ShaderMaterial: view mesh no pass owns/);assert.equal(core.failure,undefined);
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
    const material = waterMaterial(renderOrder === -18 ? "effects/water-rings-s2" : "effects/water-textured-s2", values);
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
  assert.deepEqual([...input.subarray(0, 5)], [1, core.ownedPrograms.get("effects/water-textured-s2").id, 0, -20, lowPass.id], "shown, program, group order, render order, id");
  assert.deepEqual([input[24], input[25], input[26]], [0, 6, 16], "the indexed range and owned uniform slots");
  assert.deepEqual([...input.subarray(27 + 3 * 18, 27 + 3 * 18 + 5)], [3, 3, 0, 0, 1], "the camera's position");
  assert.deepEqual([...input.subarray(27 + 5 * 18, 27 + 5 * 18 + 3)], [5, 1, 12.5], "the water's own time");
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
    const material = waterMaterial(renderOrder === -20 ? "effects/water-refraction-s2" : "effects/water-reflection-s2", values, options);
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
    const material = waterMaterial("effects/water-shore-s2", {splash:{value:splash}}, {depthWrite:false});
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
  assert.deepEqual([...input.subarray(27 + 8 * 18, 27 + 8 * 18 + 3)], [8, 1, 4], "the tint's time as the copy left it");
  assert.deepEqual([input[ARRAY], input[ARRAY + 1], input[ARRAY + 2]], [11, 4, 8], "the waders follow the scalar uniform slots");
  assert.deepEqual([...input.subarray(ARRAY + 3, ARRAY + 3 + 8)], [0, 0, 0, 1, 1, 2, 3, 0]);
  assert.deepEqual([input[SURFACE + ARRAY], input[SURFACE + ARRAY + 3 + 4]], [26, 1], "the mirror pass's waders follow its scalar slots");
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
  const flashMaterial = ownedBasicEffect("glow-basic", { map: new THREE.Texture(), transparent: true, blending: THREE.AdditiveBlending, depthWrite: false, side: THREE.DoubleSide });
  flashMaterial.color.setRGB(1, 0.5, 0.25);
  flashMaterial.opacity = 0.75;
  const flash = new THREE.Group();
  flash.add(new THREE.Mesh(partGeometry(), flashMaterial));
  flash.children[0].position.z = -1;
  let prepared = 0;
  flash.children[0].onBeforeRender = (_renderer, _scene, camera) => { assert.equal(camera, view); prepared++; flash.children[0].position.z = -2; flash.children[0].updateMatrixWorld(true); };
  viewScene.add(part, flash);
  viewScene.updateMatrixWorld(true);
  assert.equal(core.cover("view", renderer, viewScene, view), true, "the parts and the flash: three left out");
  assert.equal(flashMaterial.visible, false);
  assert.ok(calls.some((c) => c[0] === "add_owned_program" && c[1] === "effects/view-glow-basic-s0"), "the owned first-person variant");
  const input = core.viewInput, D = 1;
  assert.equal(input[0], 1, "one blended draw");
  assert.equal(prepared, 1);
  assert.equal(input[D + 17 + 2 + 14], -2, "the owned view draw publishes its hook's matrix");
  assert.deepEqual([...input.subarray(D + 2, D + 9)], [1, 0x8006, 0x8006, 0x0302, 1, 0x0302, 1], "additive blending");
  assert.deepEqual([input[D + 10], input[D + 11], input[D + 12], input[D + 13]], [1 | 4, 2, 1, 0], "depth tested, not written; back faces, then front");
  assert.deepEqual([input[D + 14], input[D + 15], input[D + 16]], [0, 3, 6]);
  assert.deepEqual([...input.subarray(D + 17 + 3 * 18, D + 17 + 3 * 18 + 5)], [3, 3, 1, 0.5, 0.25], "its colour as three's refreshUniforms sends it");
  assert.deepEqual([...input.subarray(D + 17 + 4 * 18, D + 17 + 4 * 18 + 3)], [4, 1, 0.75]);

});

test("the view scene: an unlit part (a sight whose texture scrolls) uses an owned rigid row before the blended flash", () => {
  const { core, renderer, properties } = bridge();
  const viewScene = new THREE.Scene(), view = new THREE.PerspectiveCamera();
  view.updateMatrixWorld(true);
  const sight = actorMaterial('unlit',{map:new THREE.Texture(),scroll:[0.5,0]});
  sight.uniforms.time.value=3;
  const flash = ownedBasicEffect("glow-basic", { map: new THREE.Texture(), transparent: true, blending: THREE.AdditiveBlending });
  viewScene.add(new THREE.Mesh(partGeometry(), flash), new THREE.Mesh(partGeometry(), sight), new THREE.Mesh(partGeometry(), partMaterial(properties)));
  viewScene.updateMatrixWorld(true);
  assert.equal(core.cover("view", renderer, viewScene, view), true);
  assert.equal(sight.visible, false);
  assert.equal(core.viewInput[0],1,'only the flash uses a per-object blended draw');
  assert.equal(core.input[core.recordsAt],2,'the sight uses an instanced rigid row');
  assert.deepEqual(Array.from(core.input.subarray(core.recordsAt+76,core.recordsAt+78)),[0.5,0],'scroll from its instance row');
  assert.ok(core.ownedPrograms.size>=3);

});

test("the view scene registers an unlit sight as an owned rigid program", () => {
  const { core, calls, renderer } = bridge();
  const scene=new THREE.Scene(),camera=new THREE.PerspectiveCamera();
  scene.add(new THREE.Mesh(partGeometry(),actorMaterial('unlit',{map:new THREE.Texture(),scroll:[0.1,0]})));
  assert.equal(core.cover('view',renderer,scene,camera),true);
  assert.ok(calls.some(c=>c[0]==='add_owned_program' && c[1].startsWith('actors/rigid-')));
  assert.ok(!calls.some(c=>c[0]==='add_program'));
  assert.equal(core.input[core.recordsAt],2);
});

test("an owned basic mesh registers from its catalog program, with a program of its own apart from another pass's", () => {
  const { core, calls } = bridge();
  const map = new THREE.Texture();
  const material = ownedBasicEffect("basic", { transparent: true, blending: THREE.AdditiveBlending, map });
  // Another pass (the effects' glow models) made its own program from the catalog first.
  const shared = ownedEffectProgram(core, ownedBasicEffect("glow-basic", { transparent: true, blending: THREE.AdditiveBlending, map }), 0, 0, false);
  const beam = new THREE.Mesh(partGeometry(), material);
  const entry = core.byName.transparent.registerObject({ object: beam, material, geometry: beam.geometry, group: null });
  assert.ok(entry, "registered");
  const pass = calls.find((c) => c[0] === "transparent_object_pass");
  assert.notEqual(pass[2], shared.id, "drawn with its own");
  assert.equal(core.byName.transparent.registerObject({ object: beam, material: new THREE.MeshBasicMaterial(), geometry: beam.geometry, group: null }), null, "a material without a catalog program is not this pass's");
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
  assert.ok(calls.some((c) => c[0] === "resolveTarget"), "the target resolved");
  assert.ok(!calls.some((c) => c[0] === "resetState"), "three's state is reset when three is next used");
  assert.equal(core.stale, true);
});

test("the near scene: an item no pass claims is skipped and reported once, the frame still drawn", () => {
  const { core, calls, renderer, scene, camera, properties } = nearBridge();
  const wall = new THREE.Mesh(partGeometry(), new THREE.MeshBasicMaterial());
  wall.name = "stray-wall";
  properties.get(wall.material).currentProgram = program([], ["position"]);
  wall.position.set(0, 0, -5);
  scene.add(wall);
  scene.updateMatrixWorld(true);
  assert.equal(core.coverNear(renderer, scene, camera), true);
  assert.equal(core.byName.near.stopped, wall);
  assert.ok(calls.some((c) => c[0] === "near_begin"), "everything owned is still drawn");
  assert.equal(core.failure, undefined, "no sticky failure: later frames still render");
  assert.equal(core.errors.length, 1);
  assert.match(core.errors[0], /Not drawn: stray-wall Mesh material MeshBasicMaterial/);
  // The same object on the next frames is not reported again.
  for (let i = 0; i < 3; i++) assert.equal(core.coverNear(renderer, scene, camera), true);
  assert.equal(core.errors.length, 1, "reported once");
  // A configuration the pass cannot draw (another camera, three's render) is still refused.
  assert.equal(core.byName.near.cover(renderer, scene, new THREE.PerspectiveCamera()), false);
});

test("the near scene: hero blades (additive planes with a camera-facing hook) are claimed, not skipped", async () => {
  const { attachBlade } = await import("../src/heroes.js");
  const { core, renderer, scene, camera, properties } = nearBridge();
  const held = new THREE.Group();
  held.position.set(0, 0, -3);
  scene.add(held);
  const blade = attachBlade(held, [0, 0, 0], { Texture: "blade" }, () => new THREE.Texture());
  const uniforms = [["modelViewMatrix", GL.FLOAT_MAT4], ["diffuse", GL.FLOAT_VEC3], ["opacity", GL.FLOAT]];
  // Double-sided: three links a program for each side, the back one flipped.
  const front = program(uniforms), back = program(uniforms), gl = renderer.getContext();
  gl.getAttachedShaders = (p) => [{ type: GL.VERTEX_SHADER, source: p === back.program ? "#define FLIP_SIDED\nvs" : "vs" }, { type: GL.FRAGMENT_SHADER, source: "fs" }];
  gl.getShaderSource = (shader) => shader.source;
  for (const mesh of blade.children) Object.assign(properties.get(mesh.material), { currentProgram: front, programs: new Map([[0, front], [1, back]]), __version: mesh.material.version });
  scene.updateMatrixWorld(true);
  // The first frame registers each plane; the next draws them.
  for (let i = 0; i < 2; i++) assert.equal(core.coverNear(renderer, scene, camera), true);
  assert.deepEqual(core.errors, [], "neither plane was refused");
  assert.equal(core.transparentObjects[0], blade.children.length, "both planes are items of the transparent pass");
});

test("phases drawn between three's renders reset three's state once, when three is next used", () => {
  const { core, calls, renderer, scene, camera } = bridge();
  core.addPass({ name: "a", phase: "view", draw() { this.complete = true; } });
  core.byName.view.draw = function () { this.complete = true; };
  const viewScene = new THREE.Scene();
  assert.equal(core.cover("view", renderer, viewScene, camera), true);
  core.phase("image", renderer, null, camera, true);
  core.phase("hud", renderer, scene, camera, true);
  assert.equal(calls.filter((c) => c[0] === "resetState").length, 0, "none yet");
  assert.equal(core.stale, true);
  core.settle();
  assert.deepEqual(calls.slice(-2).map((c) => c[0]), ["resetState", "setRenderTarget"]);
  assert.equal(calls.filter((c) => c[0] === "resetState").length, 1, "one for the three phases");
  // A phase drawn inside three's render resets at once.
  calls.length = 0;
  core.phase("near", renderer, scene, camera);
  assert.deepEqual(calls.slice(-2).map((c) => c[0]), ["resetState", "setRenderTarget"]);
  assert.equal(core.stale, false);
});

test("a command post's beam, hidden until the player comes near: registered from its catalog program, taken from three's list the first frame it shows", () => {
  const { core, calls, renderer, scene, camera, graphics } = bridge();
  const material = ownedBasicEffect("basic", { transparent: true, blending: THREE.AdditiveBlending, depthWrite: false, map: new THREE.Texture() });
  const beam = new THREE.Mesh(partGeometry(), material);
  beam.visible = false;
  scene.add(beam);
  const lightning = new THREE.Mesh(partGeometry(), ownedBasicEffect("basic", { transparent: true, map: new THREE.Texture() }));
  lightning.geometry.getAttribute("position").setUsage(THREE.DynamicDrawUsage);
  scene.add(lightning);
  // three starts the frame's render of the near scene.
  scene.onBeforeRender(renderer, scene, camera, graphics.sceneTarget);
  const entry = core.byName.transparent.taken.get(beam);
  assert.ok(entry, `registered before it is ever drawn (${core.errors.join("; ")})`);
  assert.equal(calls.filter((c) => c[0] === "transparent_object").length, 1, "not the lightning, whose vertices change");
  // The player comes near: three lists it, and the pass takes it at once.
  beam.visible = true;
  const transparent = core.byName.transparent;
  transparent.frameNo++;
  const item = { object: beam, material, geometry: beam.geometry, group: null, groupOrder: 0, renderOrder: 0, z: 1, id: beam.id };
  const items = [item];
  transparent.take(items);
  assert.equal(items.length, 0, "taken out of three's list");
  // A side's logo, made as the side takes the post: registered before three's next list of the scene.
  const holder = new THREE.Group();
  scene.add(holder);
  const logo = new THREE.Mesh(partGeometry(), ownedBasicEffect("basic", { transparent: true, map: new THREE.Texture() }));
  holder.add(logo);
  scene.onBeforeRender(renderer, scene, camera, graphics.sceneTarget);
  assert.ok(core.byName.transparent.taken.get(logo), "registered when added");
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
  assert.equal(core.coverMirror(renderer, scene, mirror), true);
  assert.equal(core.byName.mirrorCover.stopped, wall);
  assert.ok(calls.some((c) => c[0] === "mirror_frame"), "the reflection still draws what is owned");
  assert.match(core.errors.join("\n"), /Not drawn: \(unnamed\) Mesh material MeshBasicMaterial/, "the stray object is reported, not fatal");
  // Switched off alone: three's.
  wall.material.visible = false;
  });

test("the view scene: a gun's part whose zero material blends is a record of its own kind, drawn from its row in three's order among what blends", () => {
  const { core, calls, renderer, properties } = bridge();
  const viewScene = new THREE.Scene(), view = new THREE.PerspectiveCamera();
  view.updateMatrixWorld(true);
  const part = new THREE.Mesh(partGeometry(), partMaterial(properties));
  // The glass: a zero material that blends (BLENDED, DYNAMIC), its lights and lamps per instance.
  const glassMaterial = actorMaterial('unlit',{map:new THREE.Texture(),scroll:[0.1,0],blended:true});
  glassMaterial.transparent = true;
  glassMaterial.depthWrite = false;
  glassMaterial.defines.BLENDED = "";
  const glass = new THREE.Mesh(partGeometry(), glassMaterial);
  glass.position.z = -2;
  const flashMaterial = ownedBasicEffect("glow-basic", { map: new THREE.Texture(), transparent: true, blending: THREE.AdditiveBlending, depthWrite: false });
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
  assert.ok(input[D + DRAW] >= 0, "then the flash, its owned program");
  const records = calls.filter((c) => c[0] === "view_frame").pop()[1];
  assert.equal(records, 2, "the part and the glass written");
  const o = core.recordsAt + core.stride;
  assert.equal(core.input[o], 4, "the glass's record of its own kind");
});

test('owned view resource failures throw without exposing an actor material',()=>{
 const {core}=bridge();core.geometry=()=>null;
 for(const material of [partMaterial(),actorMaterial('unlit',{map:new THREE.Texture(),scroll:[.1,0],blending:'normal'})]){
  const mesh=new THREE.Mesh(partGeometry(),material);
  assert.throws(()=>core.byName.view.registerPart(mesh),/View (actor|flash).*resources|View flash geometry/);
  assert.equal(material.visible,false);
 }
});

test("scene discovery leaves source-free actor and caster materials to their Rust owners", () => {
  const { core, scene, camera, renderer } = bridge();
  const caster = new THREE.Mesh(partGeometry(), new THREE.Material());
  caster.userData.shadowOnly = true;
  const actor = new THREE.Mesh(partGeometry(), actorMaterial("dynamic"));
  actor.material.visible = true; // The shadow/motion walk can reveal its shell.
  const browser = new THREE.Mesh(partGeometry(), new THREE.MeshBasicMaterial());
  scene.add(caster, actor, browser);
  const compiled = [];
  renderer.compile = root => root.traverse(mesh => compiled.push(mesh));
  core.byName.effects.listed = () => {};
  core.registerItems(renderer, scene, camera, [scene]);
  assert.deepEqual(compiled, [browser]);
});

test("view geometry cannot borrow a linked Basic program instead of an owned descriptor", () => {
  const { core, renderer, properties } = bridge();
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera();
  const material = new THREE.MeshBasicMaterial({ transparent: true });
  properties.get(material).currentProgram = program([["modelViewMatrix", GL.FLOAT_MAT4]], ["position"]);
  scene.add(new THREE.Mesh(partGeometry(), material));
  // Skipped and reported, never drawn through the linked program.
  assert.equal(core.cover("view", renderer, scene, camera), true);
  assert.match(core.errors.join("\n"), /view mesh no pass owns/);
});

test("first-person arcs publish their instanced data as a sorted owned view draw", async () => {
  const { ArcBatch } = await import("../src/arc-lightning.js");
  const { core, calls, renderer } = bridge();
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera(60, 1, .3, 10);
  camera.updateMatrixWorld(true);
  const arc = new ArcBatch(4, { soft: false });
  arc.mesh.name = "arc-lightning-view";
  scene.add(arc.mesh);
  arc.begin();
  arc.push(new THREE.Vector3(0,0,-1), new THREE.Vector3(.1,.2,-1), null, null, [1,.5,.25], 1,.01,.02);
  arc.end();
  scene.updateMatrixWorld(true);
  assert.equal(core.cover("view", renderer, scene, camera), true);
  assert.equal(arc.material.visible, false);
  assert.equal(core.viewInput[0], 1);
  assert.equal(core.viewInput[1], -2, "the effects batch retains its position in the view list");
  assert.equal(core.viewInput[1 + 14 + 1], 1, "one instanced capsule");
  assert.deepEqual([...core.fxData.subarray(0, 20)], [...arc.data.subarray(0, 20)]);
  assert.ok(calls.some(c=>c[0]==="add_owned_program" && c[1]==="effects/view-arc-s2"));
  assert.equal(calls.some(c=>c[0]==="add_program"), false);
});

test('a reflection registers a newly added transparent object before the main camera sees it',()=>{
 const {core,calls,renderer,graphics,scene,camera,setTarget,properties}=nearBridge();
 const compiled=[];
 renderer.compile=list=>list.traverse(o=>{compiled.push(o);properties.get(o.material).currentProgram=program([['modelViewMatrix',GL.FLOAT_MAT4],['diffuse',GL.FLOAT_VEC3],['opacity',GL.FLOAT]],['position']);});
 scene.onBeforeRender(renderer,scene,camera,graphics.sceneTarget);
 const logo=new THREE.Mesh(partGeometry(),ownedBasicEffect('glow-basic',{visible:true,transparent:true,map:new THREE.Texture()}));
 const group=new THREE.Group();group.userData.material=logo.material;group.add(logo);
 logo.position.z=-5;scene.add(group);
 const mirror=camera.clone();mirror.userData.mirror=true;mirror.updateMatrixWorld(true);
 setTarget({name:'reflection',samples:0,width:8,height:4});
 assert.equal(core.coverMirror(renderer,scene,mirror),true);
 assert.ok(!compiled.includes(logo),'the reflected owned object never invokes the browser compiler');
 assert.ok(core.byName.effects.glows.get(logo));
 assert.ok(calls.some(c=>c[0]==='mirror_transparent_frame'));
 const count=compiled.length;
 scene.onBeforeRender(renderer,scene,camera,graphics.sceneTarget);
 assert.equal(compiled.length,count,'the main view reuses resources discovered by the reflection');
});

// The battle has near/far/view cameras. An extra caller must not turn a
// bounded camera shell store into a frame exception or corrupt restoration.
test("published camera adjustments are bounded and restore every admitted shell", () => {
  const u=new Upscaler({extensions:{has:()=>true}}),state=new Float64Array(STATE.LEN);
  u.core={postState:state};state[STATE.SHIFT]=.002;state[STATE.SHIFT+1]=-.003;
  const cameras=Array.from({length:5},()=>new THREE.PerspectiveCamera(71,1.7,.3,900));
  const originals=cameras.map(c=>c.projectionMatrix.elements.slice());
  const slots=u.shiftedCameras,offsets=u.shiftedOffsets;
  for(const camera of cameras)assert.doesNotThrow(()=>u.shift(camera));
  assert.equal(u.shiftedCount,3);
  for(let i=0;i<cameras.length;i++){
    const e=cameras[i].projectionMatrix.elements;
    assert.equal(e[8],originals[i][8]-(i<3?state[STATE.SHIFT]:0));
    assert.equal(e[9],originals[i][9]-(i<3?state[STATE.SHIFT+1]:0));
  }
  u.restore();u.restore();
  for(let i=0;i<cameras.length;i++)assert.deepEqual(cameras[i].projectionMatrix.elements,originals[i]);
  u.shift(cameras[0]);u.restore();
  assert.equal(u.shiftedCameras,slots);assert.equal(u.shiftedOffsets,offsets);assert.equal(u.shiftedCount,0);
  assert.ok(slots.every(camera=>camera===null));u.dispose();
});
