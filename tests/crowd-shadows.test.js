import './fixtures/native-render-descriptors.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {CrowdShadows} from '../src/crowd-shadows.js';
import {buildActor} from '../src/actor.js';
import {ACTOR} from '../src/rust-render/actor-layout.js';
test('crowd sources publish sparse visibility by native span identity',()=>{
 const input=new Float64Array(ACTOR.CROWD_INPUT.LEN),calls=[];let span=0;
 const core={rust:{actor_source(){return 0;},actor_source_crowd(){return 1;},actor_source_facts(){return JSON.stringify({bounds:[0,0,0,0,0,0,0,0,0,0]});},actor_source_crowd_add(){return [1,span++];},actor_crowd_frame(){calls.push(input[ACTOR.CROWD_INPUT.DIRTY]);input[ACTOR.CROWD_INPUT.DIRTY]=0;}},addView(){this.actorCrowdInput=input;},views(){}};
 const graphics={rust:core,scene:new THREE.Scene(),casters:new Set(),csm:{},caster(n){this.casters.add(n);n.castShadow=true;}};
 const def={bones:[{name:'root',parent:-1,local:new THREE.Matrix4().toArray()}],parts:[{texture:'body',vertexCount:3,indexCount:3,vertexOffset:0,indexOffset:0}],attach:'root'};
 const vertices=new Float32Array([0,0,0,0,1,0,0,0,0,1,0,0,0,1,0,1,0,0,0,1,0,0,1,0,0,1,0]).buffer;
 const actor=buildActor(def,vertices,new Uint32Array([0,1,2]).buffer,()=>null,graphics),unit={actor,shadowNear:true};graphics.scene.add(actor.root);
 const crowd=new CrowdShadows(graphics),slot=crowd.add(unit);assert.equal(slot.span,0);assert.equal(crowd.mesh.geometry,undefined);assert.equal(crowd.mesh.material.isMaterial,undefined);assert.ok(!graphics.casters.has(actor.shadow));
 crowd.update();assert.equal(input[ACTOR.CROWD_INPUT.FLAGS],1);assert.equal(crowd.draws,1);crowd.update();assert.deepEqual(calls,[1,0]);unit.shadowNear=false;crowd.update();assert.equal(input[ACTOR.CROWD_INPUT.FLAGS],0);assert.equal(crowd.mesh.castShadow,false);assert.ok(crowd.skeleton.boneInverses[0].elements.every(v=>v===0));
});
