import './fixtures/native-anim.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {Unit} from '../src/units.js';
import {Units} from '../src/registry.js';
import {unitHost} from './helpers/presentation.js';

function body(walker=false){
 const soldier={position:new THREE.Vector3(),velocity:new THREE.Vector3(),aimYaw:0,aimPitch:0,posture:'stand',air:null,dive:0,diveTime:.6,groundAdjustment:0,mode:0,ballRadius:.65};
 const unit=new Unit({className:'fixture',def:{},soldier,walker,actor:{root:new THREE.Group(),bones:[]},animator:{clips:new Map()}});
 unit.id=1;unit.generation=1;unit.removed=false;unit.alive=true;
 const host=unitHost(unit);host.owner.collision.bind(host.row,unit);
 const units=new Units();units.add(unit);host.owner.bindRegistry(units);
 return{unit,soldier,host,units};
}
// This is the page replica path: it has no canonical F3 authority binding.
// Its registry refreshes all native geometry once before per-target readers.
test('page registry publishes native posture shapes without object exports',()=>{
 const {unit,soldier,host,units}=body();let calls=0;const kernel=host.owner.kernel,run=kernel.collide_units.bind(kernel);
 kernel.collide_units=()=>{calls++;run();};
 const out=[];units.query(new THREE.Vector3(),10,out);assert.equal(calls,1);assert.equal(out[0],unit);
 const shapes=unit.hitShapes();assert.equal(shapes.length,2);assert.equal(shapes[0].type,'cylinder');assert.equal(shapes[0].half,.775);
 assert.deepEqual(shapes[1].center.toArray(),[-.09,1.65,.16]);
 const sphere=unit.sphere;unit.hitShapes();assert.equal(calls,1,'individual mirrors never export');
 const met=units.raycast(new THREE.Vector3(-.09,1.65,-5),new THREE.Vector3(0,0,1),10);
 assert.equal(calls,2);assert.equal(met.part,'head');
 soldier.position.x=3;units.query(new THREE.Vector3(),10,out);assert.equal(unit.sphere,sphere);assert.equal(shapes[0].center.x,0,'same tick retains the lazy observation');
 unit.tickCount++;soldier.posture='prone';units.query(new THREE.Vector3(),10,out);
 assert.equal(unit.hitShapes(),shapes);assert.equal(shapes[0].type,'box');assert.equal(shapes[0].half,shapes[0].halfSize);assert.equal(shapes[0].center.x,3);
});
test('page droideka collision keeps deployed primitives and its folded ball',()=>{
 const {unit,soldier,units}=body(true),out=[];
 units.query(new THREE.Vector3(),10,out);assert.equal(unit.hitShapes().length,2);
 assert.deepEqual(unit.hitShapes()[1].center.toArray(),[0,1,.506]);assert.ok(unit.hitShapes().every(s=>s.part==='body'));
 soldier.mode=12;unit.tickCount++;units.query(new THREE.Vector3(),10,out);
 assert.equal(unit.hitShapes().length,1);assert.equal(unit.hitShapes()[0].radius,.65);
});
