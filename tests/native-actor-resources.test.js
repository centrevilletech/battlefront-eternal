import {SceneNode} from '../src/rust-render/scene-layout.js';
import {Family,Mode} from '../src/rust-render/scene.js';
import {sceneComponent,sceneLayer,sceneProperty} from "../src/rust-render/scene-changes.js";
import { ROLE as R, META as M } from "../src/rust-render/frame-layout.js";
import './fixtures/native-render-descriptors.js';
import test from 'node:test';import assert from 'node:assert/strict';import * as THREE from 'three';
import {actorAsset,actorNode,actorLook,glowLook,glowNode,lightActor} from '../src/actor-resources.js';
import {ACTOR} from '../src/rust-render/actor-layout.js';
import {standinModule,standinThree,prepareSceneFixture} from './support/rust-standin.mjs';
globalThis.location??={search:''};const {RustRender}=await import('../src/rust-render/index.js');
function fixture(){const {BfeRenderer,memory,calls}=standinModule({lengths:{input:200+256*112,bones:65536},layout:{record_stride:112}}),{renderer,graphics,scene,camera}=standinThree(calls),core=prepareSceneFixture(new RustRender(graphics,{memory},BfeRenderer));return {core,calls,renderer,graphics,scene,camera};}
const asset=()=>actorAsset({kind:'plane',width:1,height:1});
function part(look=actorLook('dynamic')){const node=actorNode(asset(),look);sceneComponent(node.position,"z","=",-5);return node;}
test('native actor admission retains handles and publishes live pose, heat and lighting',()=>{
 const {core,calls,scene,camera,renderer}=fixture(),look=actorLook('dynamic',{heated:true}),node=part(look);node.userData.heated={heat:.75,base:.2,scale:.8,soot:.5,tear:new THREE.Vector4(1,2,3,.5),dissolve:.125,sun:.8,darken:.3,lighten:.4,brightness:1.2};scene.add(node);scene.updateMatrixWorld(true);scene.onBeforeRender(renderer,scene,camera,core.graphics.sceneTarget);core.publishView('near',renderer,scene,camera);
 const at=core.recordsAt;assert.equal(node.geometry,undefined);assert.equal(core.registry.admitIfNew(node).row[SceneNode.RANGE+1],6);assert.equal(look.isMaterial,undefined);assert.equal(calls.filter(c=>c[0]==='actor_material').length,1);assert.equal(core.input[at+2],-1);assert.deepEqual(Array.from(core.input.slice(at+20,at+36)),[...node.matrixWorld.elements]);assert.deepEqual(Array.from(core.input.slice(at+52,at+56)),[.8,.3,.4,1.2]);assert.deepEqual(Array.from(core.input.slice(at+80,at+89)),[.75,.2,.8,.5,1,2,3,.5,.125]);
 scene.onBeforeRender(renderer,scene,camera,core.graphics.sceneTarget);core.publishView('near',renderer,scene,camera);assert.equal(calls.filter(c=>c[0]==='actor_material').length,1);sceneProperty(node,"material",actorLook('unlit'));scene.onBeforeRender(renderer,scene,camera,core.graphics.sceneTarget);core.publishView('near',renderer,scene,camera);assert.equal(calls.filter(c=>c[0]==='actor_material').length,2);scene.remove(node);assert.equal(core.registry.select(scene,camera,Family.INSTANCE,Mode.INSTANCE),0);assert.equal(core.registry.shown(node),false);
});
test('native actor collection preserves parent visibility, layers, culling and reattachment',()=>{
 const {core,calls,scene,camera,renderer}=fixture(),group=new THREE.Group(),node=part();group.add(node);scene.add(group);const draw=()=>{scene.updateMatrixWorld(true);scene.onBeforeRender(renderer,scene,camera,core.graphics.sceneTarget);core.publishView('near',renderer,scene,camera);return core.frameMeta[R.NEAR*M.LEN+M.RECORDS];};assert.equal(draw(),1);sceneProperty(group,"visible",false);assert.equal(draw(),0);sceneProperty(group,"visible",true);sceneLayer(node.layers,"set",2);assert.equal(draw(),0);sceneLayer(node.layers,"set",0);sceneComponent(node.position,"z","=",5);assert.equal(draw(),0);sceneComponent(node.position,"z","=",-5);scene.remove(group);assert.equal(draw(),0);scene.add(group);assert.equal(draw(),1);
});
test('view actor parts publish native handles after material replacement and unlit scroll',()=>{
 const {core,calls,graphics,renderer}=fixture(),scene=graphics.viewScene,camera=graphics.viewCamera,node=part(actorLook('unlit',{scroll:[.2,.3]}));scene.add(node);scene.updateMatrixWorld(true);core.cover('view',renderer,scene,camera);assert.equal(calls.filter(c=>c[0]==='actor_material').length,1);assert.equal(core.input[core.recordsAt+2],-1);assert.ok(calls.find(c=>c[0]==='actor_material')[6]);sceneProperty(node,"material",actorLook('dynamic'));core.cover('view',renderer,scene,camera);assert.equal(calls.filter(c=>c[0]==='actor_material').length,2);assert.equal(node.geometry,undefined);
});
test('view glow models use the native effect command and generated live row',()=>{
 const {core,calls,graphics,renderer}=fixture(),look=glowLook(null,0xffffff,.25),node=glowNode(asset(),look);sceneComponent(node.position,"z","=",-2);graphics.viewScene.add(node);graphics.viewScene.updateMatrixWorld(true);core.cover('view',renderer,graphics.viewScene,graphics.viewCamera);assert.equal(calls.filter(c=>c[0]==='actor_effect').length,1);assert.equal(core.viewInput[1],ACTOR.KIND.VIEW_MODEL);assert.equal(core.fxData[28],.25);assert.equal(node.material.isMaterial,undefined);
});

// modelGroup is shared by muzzle, command-post and placed environment models.
// Their factories must all publish semantic glow appearances before admission.
test('model glow groups retain native effect sources for every part',async()=>{
 const {modelGroup}=await import('../src/models.js');const look=glowLook(null);const group=modelGroup([{asset:asset()},{asset:asset()}],()=>look);for(const node of group.children){assert.equal(node.userData.effectModel.family,'actorEffect');assert.equal(node.userData.effectModel.visibility,'anchor');assert.equal(node.userData.effectModel.owner,look);assert.equal(node.geometry,undefined);}
});

// A full native skin bank must not allocate another image on every paused
// preview frame. Admission fails once; the same identity stays clipped.
test('a clipped preview skin retires its image and stays bounded while warm',async()=>{
 const {actorSkin}=await import('../src/actor-resources.js');let images=0,retired=0,admissions=0;
 const core={rust:{actor_skin_words:()=>64,actor_skin:()=>{admissions++;return undefined;}},resources:{gpu:{skin_image:()=>++images,release_skin_image:()=>retired++}},texture:id=>id};
 const skeleton={bones:[{}],boneMatrices:new Float32Array(16)};
 const {skinOwners}=await import('../src/rust-render/skin-joins.js');const object={skeleton,userData:{presentationRow:true}},source={slot:1,generation:1,object};skinOwners.set(skeleton,object);
 core.graphics={};core.registry={nodes:[source],admitIfNew:()=>source,worlds:{flush(){}}};core.addView=()=>{};core.views=()=>{};core.sceneSkinAdmit=new Float32Array(64);core.rust.scene_skin_admit_frame=()=>{};

 for(let i=0;i<10000;i++)assert.equal(actorSkin(core,skeleton),null);
 assert.deepEqual([images,retired,admissions],[1,1,1]);
});


// Pool reattachment can assign another native presentation identity while the
// same immutable source and appearance stay admitted. Its order must refresh.
test('pooled native actor admission refreshes order without recreating appearance',()=>{
 const {core,calls,scene,camera,renderer}=fixture(),node=part(),root=new THREE.Group();
 node.userData.presentationRow={id:2,actor:{root}};root.add(node);scene.add(root);
 const draw=()=>{scene.updateMatrixWorld(true);core.publishView('near',renderer,scene,camera);};
 draw();const admitted=core.registry.admitIfNew(node),slot=admitted.slot;
 assert.equal(admitted.instanceOrder,20000+slot+1);
 scene.remove(root);node.userData.presentationRow={id:7,actor:{root}};scene.add(root);draw();
 assert.equal(admitted.slot,slot);assert.equal(admitted.instanceOrder,70000+slot+1);
 assert.equal(calls.filter(c=>c[0]==='actor_material').length,1);
});
