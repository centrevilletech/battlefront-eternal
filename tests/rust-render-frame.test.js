import {Family,Mode} from '../src/rust-render/scene.js';
import {sceneComponent,sceneProperty,sceneTransform} from "../src/rust-render/scene-changes.js";
import {prepareSceneFixture} from "./support/rust-standin.mjs";
import './fixtures/native-render-descriptors.js';
import {actorAsset,glowLook,glowNode} from '../src/actor-resources.js';
import { presentationModule } from "./helpers/page-presentation.js";
import { bindInterfaceBatch } from "../src/interface/batch.js";
bindInterfaceBatch(presentationModule());
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { Upscaler } from "../src/upscaler.js";
import { BINDING as B, META as M, ROLE as R } from "../src/rust-render/frame-layout.js";
import { STATE } from "../src/rust-render/post-layout.js";
import { actorMaterial } from "../src/materials.js";
import { ownedBasicEffect, ownedEffectProgram } from "../src/rust-render/effect-material.js";

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
  const core = prepareSceneFixture(new RustRender(graphics, { memory }, BfeRenderer));
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
function partMaterial() {
  const material=actorMaterial('dynamic',{map:new THREE.Texture()});
  material.uniforms.sunScale.value=0.5;material.uniforms.ambientPull.value.set(0.1,0.2);material.uniforms.brightness.value=0.9;
  return material;
}

test("cover: three's start of a render, the phase, and three's end when every pass of the phase drew all of the scene", () => {
  const { core, calls, renderer } = bridge();
  const scene = new THREE.Scene(), camera = new THREE.PerspectiveCamera();
  const child = new THREE.Object3D();
  sceneTransform(child.position.set(1, 2, 3));
  scene.add(child);
  let before = 0, after = 0, complete = false;
  scene.onBeforeRender = () => before++;
  scene.onAfterRender = () => after++;
  core.addPass({ name: "spy", phase: "view", draw() { this.complete = complete; } });
  assert.throws(() => core.cover("view", renderer, scene, camera), /Incomplete Rust/);
  assert.equal(before, 1, "the scene's onBeforeRender (its lights packed) before the phase");
  assert.equal(child.matrixWorld.elements[12], 1, "its world matrices brought up to date first");
  assert.ok(calls.some((c) => c[0] === "frame_aux_frame" && c[2] === R.VIEW));
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
  core.publishView("mirror", renderer, scene, mirror);
  assert.ok(calls.some((c) => c[0] === "frame_aux_frame" && c[2] === R.MIRROR0_NEAR));
  assert.ok(!calls.some((c) => c[0] === "frame_aux_frame" && c[2] === R.NEAR));
  calls.length = 0;
  assert.throws(() => core.coverMirror(renderer, scene, new THREE.PerspectiveCamera()), /Incomplete Rust mirror/);
});

test("foliage: Rust selects every registered card class", async () => {
  const { core, calls, renderer, scene, properties } = bridge();
  const map = new THREE.Texture();
  const instance = { position: new THREE.Vector3(1, 2, 3), matrix: new THREE.Matrix4().makeTranslation(1, 2, 3), tint: [0.5, 0.6, 0.7], sphere: null, cullDistance: Infinity, maxDistance: 100 };
  const kind = { grass:true,map,def:{Alpha:.5},cards:[{x:0,y:0,z:0,grey:.7,width:.5,height:1,phase:2,rate:1,part:0}],instances:[instance] };
  const foliage = { kinds: new Map([["a", kind]]), time: 2.5, nearEnd: () => 200, drawer: null };
  scene.userData.foliage = foliage;
  core.publishView("near", renderer, scene, core.graphics.camera);
  assert.equal(foliage.drawer, core.byName.foliage, "the near phase attaches the drawer");
  const camera = new THREE.PerspectiveCamera();
  sceneTransform(camera.position.set(4, 5, 6));
  camera.updateMatrixWorld(true);
  foliage.drawer.select(foliage, camera);
  const added = calls.find((c) => c[0] === "foliage_class");
  assert.equal(added[2],true,"grass");
  assert.equal(added[3],false,"shadow feature");
  assert.deepEqual([...added[5]],[0,0,0,.7,.5,1,2,1,0],"authored cards, not GPU layout");
  assert.deepEqual([...added[6]].slice(0, 6).map((v) => +v.toFixed(2)), [1, 2, 3, 0.5, 0.6, 0.7]);
  assert.deepEqual([...added[6]].slice(15, 16), [100]);
  assert.deepEqual([...added[7]].slice(6, 9), [-1, Infinity, 100], "no sphere, always tested");
  assert.equal(calls.some(c=>c[0]==="owned_program"&&String(c[1]).includes("foliage")),false,"native class owns catalog selection");
  assert.ok(foliage.drawer.classes.has(kind));
  assert.ok(calls.some((c) => c[0] === "foliage_frame"));
  const input = core.foliageInput;
  assert.deepEqual([input[48], input[49], input[50], input[51], input[52]], [4, 5, 6, 200, 2.5]);
  });

test("foliage under shadows: a card program that reads modelMatrix (three's world position for USE_SHADOWMAP) is taken", () => {
  const { core, calls, renderer, scene, properties } = bridge();
  core.graphics.csm={};
  const kind = {grass:true,map:new THREE.Texture(),def:{},cards:[],instances:[]};
  const foliage = { kinds: new Map([["a", kind]]), time: 0, nearEnd: () => 200, drawer: null };
  scene.userData.foliage = foliage;
  core.publishView("near", renderer, scene, core.graphics.camera);
  foliage.drawer.select(foliage, core.graphics.camera);
  assert.equal(calls.find(c=>c[0]==="foliage_class")[3],true,"native shadow feature");
  assert.ok(foliage.drawer.classes.has(kind));
});

test("HUD numeric descriptors retain the batch and publish only changed vertex spans", async () => {
  const { core, calls, renderer } = bridge();
  const { Batch } = await import("../src/interface/batch.js");
  const batch = new Batch(), texture = 0;
  const hud = { batch, scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera() };
  const m = new THREE.Matrix4();
  const frame = (edge) => {
    batch.begin();
    batch.quad(m,0,0,10,10,texture,[0,0,1,1],[1,1,1,1]);
    batch.quad(m,0,0,edge,edge,texture,[0,0,1,1],[1,1,1,1]);
    batch.end();
    assert.equal(core.coverHud(renderer,hud),true);
  };
  frame(20);
  assert.equal(calls.filter(c=>c[0]==="hud_descriptor_init").length,1);
  assert.equal(calls.filter(c=>c[0]==="hud_appearance").length,1);
  assert.equal(calls.some(c=>c[0]==="initTexture"),false);
  const input=core.hudInput, IN={GROUPS:0,UPLOAD:41,VIEWPORT:43,FIRST:45};
  assert.deepEqual([input[IN.GROUPS],input[IN.UPLOAD],input[IN.UPLOAD+1]],[1,0,8]);
  assert.deepEqual([...input.subarray(IN.FIRST,IN.FIRST+5)],[0,12,-2,0,0]);
  assert.deepEqual([...core.hudPosition.subarray(12,24)],[...sceneTransform(batch.position.subarray(12,24))]);
  frame(30);
  assert.deepEqual([input[IN.UPLOAD],input[IN.UPLOAD+1]],[4,8]);
  frame(30);
  assert.equal(calls.filter(c=>c[0]==="hud_appearance").length,1);
});

test("an error in a pass stops rendering with the GPU in the error", () => {
 const {core,renderer,scene,camera,calls}=bridge();
 const material=new THREE.ShaderMaterial();sceneProperty(material,"visible",false);scene.add(new THREE.Mesh(new THREE.BufferGeometry(),material));
 renderer.compile = () => {};
 core.addPass({name:"broken",phase:"view",draw(){throw Error("broken pass");}});
 assert.throws(()=>core.cover("view",renderer,scene,camera), /Battle renderer failed on unknown GPU: broken pass/);
 assert.equal(material.visible,false);
 assert.equal(core.failure.rendererFailure,true);
 assert.equal(calls.some(c=>c[0]==="resetState"),false,"a failed frame is not continued by three");
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
  sceneProperty(background,"frustumCulled",false);
  const three = standinThree(calls, { graphics: { background } });
  three.scene.add(background);
  three.properties.get(background.material).currentProgram = program([["map", GL.SAMPLER_2D]], ["position", "uv"]);
  background.material.userData.bfeProgram="effects/far-image";
  const core = prepareSceneFixture(new RustRender(three.graphics, { memory }, BfeRenderer));
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
  sceneTransform(behind.position.set(0, 0, 50));
  const material = new THREE.MeshBasicMaterial({ transparent: true });
  const holo = new THREE.Mesh(partGeometry(), material);
  sceneTransform(holo.position.set(1, 0, -5));
  sceneProperty(holo,"renderOrder",2);
  scene.add(hidden, behind, holo);
  scene.updateMatrixWorld(true);
  const threeProgram = { id: 3 };
  properties.get(material).currentProgram = threeProgram;
  properties.get(material).__version = material.version;
  core.byName.transparent.taken.set(holo, { bucket: false, id: 7, material, program: threeProgram, frame: 0, version: material.version });
  list.opaque.push({ object: graphics.background });
  assert.equal(core.coverNear(renderer, scene, camera), true);
  assert.equal(before, 1, "three's start of a render");
  assert.deepEqual(calls.filter(c=>c[0]==="frame_aux_frame").map(c=>c[2]),[R.NEAR,R.IMAGE,R.TRANSPARENT],"standalone packets retain semantic roles");
  assert.deepEqual([...core.frameBindings.subarray(R.NEAR*B.LEN+B.CLEAR,R.NEAR*B.LEN+B.CLEAR+4)],[0,0,0,1]);
  assert.equal(core.postInput[1], -2, "into the target bound now");
  // The hologram's record, keyed as projectObject keys it.
  const z = sceneTransform(sceneTransform(new THREE.Vector4().copy(holo.geometry.boundingSphere.center).applyMatrix4(holo.matrixWorld))
    .applyMatrix4(new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse))).z;
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
  sceneTransform(wall.position.set(0, 0, -5));
  scene.add(wall);
  scene.updateMatrixWorld(true);
  assert.equal(core.coverNear(renderer, scene, camera), true);
  assert.equal(core.registry.admitIfNew(wall).families & Family.NEAR,0);
  assert.ok(calls.some((c) => c[0] === "frame_publish_frame"), "everything owned is still drawn");
  assert.equal(core.failure, undefined, "no sticky failure: later frames still render");
  assert.equal(core.errors.length, 1);
  assert.match(core.errors[0], /Not drawn: stray-wall Mesh material MeshBasicMaterial/);
  // The same object on the next frames is not reported again.
  for (let i = 0; i < 3; i++) assert.equal(core.coverNear(renderer, scene, camera), true);
  assert.equal(core.errors.length, 1, "reported once");
  // A configuration the pass cannot draw (another camera, three's render) is still refused.
  assert.equal(core.byName.near.cover(renderer, scene, new THREE.PerspectiveCamera()), false);
});

test("phases drawn between three's renders reset three's state once, when three is next used", () => {
  const { core, calls, renderer, scene, camera } = bridge();
  core.addPass({ name: "a", phase: "view", draw() { this.complete = true; } });
  core.byName.view.draw = function () { this.complete = true; };
  const viewScene = new THREE.Scene();
  assert.equal(core.cover("view", renderer, viewScene, camera), true);
  core.publishView("image", renderer, null, camera);
  core.publishView("hud", renderer, scene, camera);
  assert.equal(calls.filter((c) => c[0] === "resetState").length, 0, "none yet");
  assert.equal(core.stale, true);
  core.settle();
  assert.deepEqual(calls.slice(-2).map((c) => c[0]), ["resetState", "setRenderTarget"]);
  assert.equal(calls.filter((c) => c[0] === "resetState").length, 1, "one for the three phases");
  // Publishing another scene does not introduce a state-reset continuation.
  calls.length = 0;
  core.publishView("near", renderer, scene, camera);
  assert.equal(calls.some(c=>c[0]==="resetState"),false);
  assert.equal(core.stale, true);
});

test("a command post's beam, hidden until the player comes near: registered from its catalog program, taken from three's list the first frame it shows", () => {
  const { core, calls, renderer, scene, camera, graphics } = bridge();
  const material = ownedBasicEffect("basic", { transparent: true, blending: THREE.AdditiveBlending, depthWrite: false, map: new THREE.Texture() });
  const beam = new THREE.Mesh(partGeometry(), material);
  sceneProperty(beam,"visible",false);
  scene.add(beam);
  const lightning = new THREE.Mesh(partGeometry(), ownedBasicEffect("basic", { transparent: true, map: new THREE.Texture() }));
  lightning.geometry.getAttribute("position").setUsage(THREE.DynamicDrawUsage);
  scene.add(lightning);
  // three starts the frame's render of the near scene.
  core.prepareScene(renderer,scene,camera,graphics.sceneTarget);
  scene.onBeforeRender(renderer, scene, camera, graphics.sceneTarget);
  const entry = core.byName.transparent.taken.get(beam);
  assert.ok(entry, `registered before it is ever drawn (${core.errors.join("; ")})`);
  assert.equal(calls.filter((c) => c[0] === "transparent_object").length, 1, "not the lightning, whose vertices change");
  // The player comes near: three lists it, and the pass takes it at once.
  sceneProperty(beam,"visible",true);
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
  core.prepareScene(renderer,scene,camera,graphics.sceneTarget);
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
  assert.deepEqual(calls.filter(c=>c[0]==="frame_aux_frame").map(c=>c[2]),[R.MIRROR0_NEAR,R.MIRROR0_IMAGE,R.MIRROR0_TRANSPARENT]);
  assert.equal(core.frameMeta[R.MIRROR0_TRANSPARENT*M.LEN+M.STEPS],0,"the image has a separate packet");
  assert.equal(core.postInput[1], -2, "the image into the target bound now");
  assert.equal(core.stale, true, "three's state reset when three is next used");
  // Not the frame's camera, nor a render something three draws itself is in.
  assert.throws(() => core.coverMirror(renderer, scene, new THREE.PerspectiveCamera()), /Incomplete Rust/);
  const wall = new THREE.Mesh(partGeometry(), new THREE.MeshBasicMaterial());
  properties.get(wall.material).currentProgram = program([], ["position"]);
  sceneTransform(wall.position.set(0, 0, -5));
  scene.add(wall);
  scene.updateMatrixWorld(true);
  calls.length = 0;
  assert.equal(core.coverMirror(renderer, scene, mirror), true);
  assert.equal(core.registry.admitIfNew(wall).families & Family.NEAR,0);
  assert.ok(calls.some((c) => c[0] === "frame_aux_frame" && c[2] === R.MIRROR0_NEAR), "the reflection still draws what is owned");
  assert.match(core.errors.join("\n"), /Not drawn: \(unnamed\) Mesh material MeshBasicMaterial/, "the stray object is reported, not fatal");
  // Switched off alone: three's.
  sceneProperty(wall.material,"visible",false);
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
  assert.equal(arc.mesh.material, undefined);
  assert.equal(arc.mesh.visible, true, "view anchors retain visibility across native draws");
  assert.equal(core.viewInput[0], 1);
  assert.equal(core.viewInput[1], -2, "the effects batch retains its position in the view list");
  assert.equal(core.viewInput[1 + 14 + 1], 1, "one instanced capsule");
  assert.deepEqual([...core.fxData.subarray(0, 20)], [...arc.data.subarray(0, 20)]);
  assert.ok(calls.some(c=>c[0]==="effect_batch" && c[1]===6));
  assert.equal(calls.some(c=>c[0]==="add_program"), false);
});

test('a reflection registers a newly added transparent object before the main camera sees it',()=>{
 const {core,calls,renderer,graphics,scene,camera,setTarget,properties}=nearBridge();
 const compiled=[];
 renderer.compile=list=>list.traverse(o=>{compiled.push(o);properties.get(o.material).currentProgram=program([['modelViewMatrix',GL.FLOAT_MAT4],['diffuse',GL.FLOAT_VEC3],['opacity',GL.FLOAT]],['position']);});
 scene.onBeforeRender(renderer,scene,camera,graphics.sceneTarget);
 const logo=glowNode(actorAsset({kind:'plane'}),glowLook(null));
 const group=new THREE.Group();sceneProperty(group.userData,"material",logo.material);group.add(logo);
 sceneComponent(logo.position,"z","=",-5);scene.add(group);
 const mirror=camera.clone();mirror.userData.mirror=true;mirror.updateMatrixWorld(true);
 setTarget({name:'reflection',samples:0,width:8,height:4});
 assert.equal(core.coverMirror(renderer,scene,mirror),true);
 assert.ok(!compiled.includes(logo),'the reflected owned object never invokes the browser compiler');
 assert.ok(core.byName.effects.glows.get(logo));
 assert.ok(calls.some(c=>c[0]==='frame_aux_frame'&&c[2]===R.MIRROR0_TRANSPARENT));
 const count=compiled.length;
 scene.onBeforeRender(renderer,scene,camera,graphics.sceneTarget);
 assert.equal(compiled.length,count,'the main view reuses resources discovered by the reflection');
});


// A copied depth target may first allocate while writing a transparent draw.
// Moving the backing buffer detaches old views exactly as WASM growth does.
test('transparent hooks retain written rows after renderer memory growth',()=>{
 const {core,scene,camera,renderer,memory}=bridge(),pass=core.byName.transparent;
 const first=new THREE.Mesh(partGeometry(),ownedBasicEffect('basic',{transparent:true,depthWrite:false,map:new THREE.Texture()}));
 const second=new THREE.Mesh(partGeometry(),ownedBasicEffect('basic',{transparent:true,depthWrite:false,map:new THREE.Texture()}));
 first.position.set(1,0,-4);second.position.set(2,0,-5);scene.add(first,second);scene.updateMatrixWorld(true);
 core.prepareScene(renderer,scene,camera,core.graphics.sceneTarget);
 const before=memory.buffer;
 second.onBeforeRender=()=>{memory.buffer=structuredClone(memory.buffer,{transfer:[memory.buffer]});};
 const items=[{object:first,material:first.material,geometry:first.geometry,groupOrder:0,renderOrder:0,z:0,id:first.id},{object:second,material:second.material,geometry:second.geometry,groupOrder:0,renderOrder:0,z:0,id:second.id}];
 pass.take(items,{renderer,scene,camera});
 assert.equal(before.byteLength,0);assert.equal(core.transparentObjects[0],2);
 const stride=core.rust.transparent_object_stride();
 assert.deepEqual([...core.transparentObjects.slice(6,22)],first.matrixWorld.elements);
 assert.deepEqual([...core.transparentObjects.slice(6+stride,22+stride)],second.matrixWorld.elements);
 assert.equal(items.length,0);
});
