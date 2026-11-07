import {worldMaterial} from '../src/materials.js';
import {ownWorldMaterial} from '../src/rust-render/world-programs.js';
// Native descriptors retain their resource identity across main, mirror and
// probe views. The CPU registry supplies ordered anchors without mesh shells.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {RustRender} from '../src/rust-render/index.js';
import {sceneProperty,sceneLayer} from '../src/rust-render/scene-changes.js';
import {standinModule,standinThree,prepareSceneFixture} from './support/rust-standin.mjs';
globalThis.location??={search:''};
function fixture(){
 const {BfeRenderer,memory,calls}=standinModule(),host=standinThree(calls);
 const core=prepareSceneFixture(new RustRender(host.graphics,{memory},BfeRenderer));
 return {...host,core,calls,far:core.byName.far};
}
function anchor(f,id,parent=f.graphics.farScene){
 const node=new THREE.Object3D();node.userData.worldOrder=id;parent.add(node);
 const item=f.far.admit(node,{terrain:false},new Float32Array(),new Uint32Array(),null,null,new Uint8Array(),new Int32Array(9).fill(-1),id);
 return {node,item};
}
function draw(f,camera=f.camera,name='far'){return f.core.cover(name,f.renderer,f.graphics.farScene,camera);}
test('far descriptors retain cold ids and authored insertion across warm views',()=>{
 const f=fixture(),a=anchor(f,4),b=anchor(f,9);
 assert.equal(draw(f),true);assert.equal(f.far.count,2);
 assert.deepEqual(f.far.found.slice(0,2).map(x=>x.object),[a.node,b.node]);
 assert.equal(a.node.geometry,undefined);assert.equal(a.node.material,undefined);
 const cold=f.calls.filter(c=>c[0]==='static_world');assert.equal(cold.length,2);
 assert.equal(cold[0][1],4);assert.equal(cold[1][1],9);
 draw(f);assert.equal(f.calls.filter(c=>c[0]==='static_world').length,2);
 assert.equal(f.core.farInput[a.item.slot],1);assert.equal(f.core.farInput[b.item.slot],1);
});
test('hidden and detached far anchors obey native parent visibility and camera layers',()=>{
 const f=fixture(),group=new THREE.Group();f.graphics.farScene.add(group);
 const a=anchor(f,1,group),b=anchor(f,2);draw(f);assert.equal(f.far.count,2);
 sceneProperty(group,'visible',false);draw(f);assert.equal(f.far.count,1);assert.equal(f.far.found[0].object,b.node);
 sceneProperty(group,'visible',true);sceneLayer(group.layers,'set',5);draw(f);assert.equal(f.far.count,2,'a parent layer mismatch does not prune descendants');
 sceneLayer(a.node.layers,'set',5);draw(f);assert.equal(f.far.count,1);
 f.graphics.farScene.remove(b.node);draw(f);assert.equal(f.far.count,0);
 sceneLayer(a.node.layers,'set',0);draw(f);assert.equal(f.far.count,1);
 assert.equal(a.node.visible,true);assert.equal(a.node.layers.mask,1);
});
test('mirror and six-face probe inputs share descriptors and retain mip generation timing',()=>{
 const f=fixture();anchor(f,2);const mirror=f.camera.clone();mirror.userData.mirror=true;
 draw(f,mirror,'farMirror');assert.equal(f.far.count,1);
 const texture=new THREE.CubeTexture(),target={texture};
 for(let face=0;face<6;face++){
  const camera=f.camera.clone();camera.userData.nativeProbe={face,last:face===5,target};
  draw(f,camera,'probe');assert.equal(f.core.farInput[6],face===5?1+f.core.texture(texture):0);
 }
 assert.equal(f.calls.filter(c=>c[0]==='static_world').length,1);
 assert.equal(typeof f.far.walk,'undefined');assert.equal(typeof f.far.share,'undefined');
 assert.equal(typeof f.far.plainProgram,'undefined');
});


// Some blended low-detail buckets retain the world catalog while opaque
// surfaces already use numeric descriptors. Both enter the same native list.
test('low-detail world shells keep their owned far admission',()=>{
 const f=fixture(),geometry=new THREE.PlaneGeometry(),material=worldMaterial('lit',{map:new THREE.Texture(),far:true});
 material.defines.WORLD_SPACE='';ownWorldMaterial(material,geometry);
 const mesh=new THREE.Mesh(geometry,material);mesh.frustumCulled=false;f.graphics.farScene.add(mesh);
 assert.equal(draw(f),true);assert.equal(f.far.count,1);
 assert.equal(f.far.found[0].object,mesh);assert.ok(f.far.found[0].item);
 assert.ok(f.calls.some(call=>call[0]==='add_owned_program'));
 assert.equal(f.calls.some(call=>call[0]==='add_program'),false);
});
