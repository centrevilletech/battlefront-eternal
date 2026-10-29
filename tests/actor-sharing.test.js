import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {buildActor} from '../src/actor.js';

test('pooled actors share geometry and retain independent pose and lighting',()=>{
 const def={bones:[{name:'root',parent:-1,local:new THREE.Matrix4().toArray()}],attach:'root',
  parts:[{texture:'body',vertexCount:3,indexCount:3,vertexOffset:0,indexOffset:0}]};
 const vertices=new Float32Array([0,0,0,0,1,0,0,0,0, 1,0,0,0,1,0,1,0,0, 0,1,0,0,1,0,0,1,0]).buffer;
 const indices=new Uint32Array([0,1,2]).buffer;
 const create=()=>buildActor(def,vertices,indices,()=>null,{caster(){}});
 const a=create(),b=create();
 assert.equal(a.body.geometry,b.body.geometry);
 assert.notEqual(a.bones[0],b.bones[0]);assert.notEqual(a.materials[0],b.materials[0]);
 a.bones[0].position.x=12;a.materials[0].uniforms.sunScale.value=.3;
 assert.equal(b.bones[0].position.x,0);assert.equal(b.materials[0].uniforms.sunScale.value,1);
});
