import {sceneComponent,sceneProperty,sceneTransform} from "../src/rust-render/scene-changes.js";
import {prepareSceneFixture} from "./support/rust-standin.mjs";
import { ROLE as R, META as M } from "../src/rust-render/frame-layout.js";
// Motion consumes native geometry descriptors while retaining the existing
// current/previous pose publication boundary. GL fixtures check velocity output.
import './fixtures/native-render-descriptors.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {actorAsset,actorNode} from '../src/actor-resources.js';
import {ACTOR} from '../src/rust-render/actor-layout.js';
import {motionVectors} from '../src/motion-vectors.js';
import {standinModule,standinThree} from './support/rust-standin.mjs';
import {actorPart} from './helpers/actor-resources.js';
globalThis.location??={search:''};
const {RustRender}=await import('../src/rust-render/index.js');
function fixture(){
 let extras=0;const {BfeRenderer,memory,calls}=standinModule({lengths:{motion_input:106+256*88,motion_bones:8192*16,motion_previous:8192*16},methods:{actor_flyer_motion(source,capacity,matrices){calls.push(['actor_flyer_motion',source,capacity,matrices]);return Float64Array.of(extras++,0,0,0,1);}}});
 const host=standinThree(calls),core=prepareSceneFixture(new RustRender(host.graphics,{memory},BfeRenderer));return {core,calls,...host};
}
function view(){const camera=new THREE.PerspectiveCamera(60,2,.5,200);sceneTransform(camera.position.set(0,.5,5));camera.updateMatrixWorld(true);return camera;}
function packet(camera,root,viewScene=null){return {target:{texture:new THREE.Texture(),width:8,height:4},root,camera,viewScene,view:camera,depth:new THREE.DepthTexture(8,4),current:new THREE.Matrix4().multiplyMatrices(camera.projectionMatrix,camera.matrixWorldInverse),previous:new THREE.Matrix4()};}
test('native motion descriptors preserve current/previous skins and a locked first-person mask',()=>{
 const f=fixture(),root=new THREE.Scene(),camera=view(),geometry=new THREE.PlaneGeometry();
 geometry.setAttribute('skinIndex',new THREE.Uint16BufferAttribute(Array(16).fill(0),4));geometry.setAttribute('skinWeight',new THREE.Float32BufferAttribute(Array.from({length:16},(_,i)=>i%4===0?1:0),4));
 const body=actorPart(geometry,null,true),bone=new THREE.Bone();body.add(bone);body.bind(new THREE.Skeleton([bone]));root.add(body);
 const gun=actorNode(actorAsset({kind:'plane'}),null);root.add(gun);root.updateMatrixWorld(true);body.skeleton.update();
 const a=motionVectors.track(body),b=motionVectors.track(gun),locked=new THREE.Scene(),mask=actorNode(gun.userData.actorAsset,null);locked.add(mask);locked.updateMatrixWorld(true);const input=packet(camera,root,locked);
 try{
  motionVectors.render(f.renderer,input);assert.equal(f.core.frameMeta[R.MOTION*M.LEN+M.RECORDS],3);assert.ok(f.calls.some(c=>c[0]==='frame_aux_frame'&&c[2]===R.MOTION));
  const row=106,stride=88,out=f.core.motionInput;assert.equal(out[row+1],ACTOR.KIND.RETAINED);assert.deepEqual([out[row],out[row+stride],out[row+2*stride]],[1,1,2]);assert.deepEqual([...out.slice(row+8,row+24)],body.matrixWorld.elements);assert.deepEqual([...f.core.motionBones.slice(0,16)],[...body.skeleton.boneMatrices]);
  const previous=body.matrixWorld.toArray();sceneComponent(body.position,"x","=",.25);root.updateMatrixWorld(true);motionVectors.render(f.renderer,input);assert.deepEqual([...out.slice(row+24,row+40)],previous);assert.deepEqual([...out.slice(row+8,row+24)],body.matrixWorld.elements);assert.equal(f.calls.filter(c=>c[0]==='actor_motion').length,3,'warm draw retains all three descriptors');assert.equal(body.geometry,undefined);
 }finally{motionVectors.release(a);motionVectors.release(b);motionVectors.drawer=null;}
});
test('native sky motion publishes ordered current/previous instance rows and runs its hooks',()=>{
 const f=fixture(),camera=view(),root=new THREE.Scene(),asset=actorAsset({kind:'plane'}),hooks=[];
 const make=(count,z)=>{const node=actorNode(asset,null);sceneComponent(node.position,"z","=",z);sceneProperty(node,"frustumCulled",false);node.count=count;node.userData.order=0;const matrices=new Float32Array(count*16);for(let i=0;i<count;i++)new THREE.Matrix4().makeTranslation(i,0,0).toArray(matrices,i*16);node.userData.flyerMotion={asset,capacity:count,owner:{still:0},arrays:{matrices,previous:Float32Array.from(matrices,v=>v+.5),shown:new Float32Array(count).fill(1)}};node.updateMatrixWorld(true);node.onBeforeRender=()=>hooks.push(node);motionVectors.addExtra(node);return node;};
 const far=make(3,-400),near=make(2,-100),input=packet(camera,root);
 assert.equal(f.calls.filter(c=>c[0]==='actor_flyer_motion').length,0,'bounds wait until current instance matrices exist');
 try{motionVectors.render(f.renderer,input);assert.deepEqual(hooks,[near,far]);assert.equal(f.calls.filter(c=>c[0]==='actor_flyer_motion').length,2);assert.equal(f.core.motionExtras[0],2);assert.equal(f.core.motionExtras[2],2);assert.deepEqual([...f.core.motionExtraValues.slice(0,32)],[...near.userData.flyerMotion.arrays.matrices]);assert.deepEqual([...f.core.motionExtraValues.slice(32,64)],[...near.userData.flyerMotion.arrays.previous]);assert.deepEqual([...f.core.motionExtraValues.slice(64,66)],[1,1]);assert.equal(near.geometry,undefined);motionVectors.render(f.renderer,input);assert.equal(f.calls.filter(c=>c[0]==='actor_flyer_motion').length,2);
 }finally{motionVectors.removeExtra(far);motionVectors.removeExtra(near);motionVectors.drawer=null;}
});
