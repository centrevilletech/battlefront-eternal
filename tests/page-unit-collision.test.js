import './fixtures/native-anim.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {Unit} from '../src/units.js';
import {Units} from '../src/registry.js';
import {unitHost,presentationLoaded} from './helpers/presentation.js';
import {PageUnits} from '../src/presentation/units.js';

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
 kernel.collide_units=count=>{calls++;run(count);};
 const out=[];units.query(new THREE.Vector3(),10,out);assert.equal(calls,1);assert.equal(out[0],unit);
 const shapes=unit.hitShapes();assert.equal(shapes.length,2);assert.equal(shapes[0].type,'cylinder');assert.equal(shapes[0].half,.775);
 assert.deepEqual(shapes[1].center.toArray(),[-.09,1.65,.16]);
 const sphere=unit.sphere;unit.hitShapes();assert.equal(calls,1,'individual mirrors never export');
 const met=units.raycast(new THREE.Vector3(-.09,1.65,-5),new THREE.Vector3(0,0,1),10);
 assert.equal(calls,1,'a second query reuses the observed batch');assert.equal(met.part,'head');
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

// Drawing must not spend collision time on bodies nobody queried. A first
// direct reader restores the legacy lazy observation through a single batch.
test('body frames prepare no collision records until a bounds or limb read',()=>{
 const {unit,soldier,host}=body(),collision=host.owner.collision;
 let writes=0,calls=0,publishes=0;
 const write=collision.write.bind(collision),run=host.owner.kernel.collide_units.bind(host.owner.kernel),publish=collision.publish.bind(collision);
 collision.write=(row,unit)=>{writes++;return write(row,unit);};
 collision.publish=count=>{publishes++;return publish(count);};
 host.owner.kernel.collide_units=count=>{calls++;return run(count);};
 for(let frame=0;frame<4;frame++){
  unit.tickCount++;soldier.position.x=frame+.125;
  host.owner.begin(1/60,frame,null);host.owner.write(host.row,unit,1,true);host.owner.run();
 }
 assert.deepEqual([writes,calls,publishes],[0,0,0]);
 const bounds=unit.sphere;assert.equal(bounds.center.x,3.08);
 assert.deepEqual([writes,calls,publishes],[1,1,1]);
 unit.hitShapes();unit.sphere;assert.deepEqual([writes,calls,publishes],[1,1,1]);
 unit.tickCount++;soldier.position.x=7.125;unit.hitShapes();
 assert.equal(unit.sphere,bounds);assert.equal(bounds.center.x,7.08);
 assert.deepEqual([writes,calls,publishes],[2,2,2]);
});

test('one lazy batch shares readers and submits only a changed body',()=>{
 const first=body(),second=body(),owner=new PageUnits(presentationLoaded);
 try {
 owner.attach(first.unit.actor,first.host.row.source,first.unit.presentation,first.unit);
 owner.attach(second.unit.actor,second.host.row.source,second.unit.presentation,second.unit);
 owner.bindRegistry(first.units);first.units.add(second.unit);
 const counts=[],run=owner.kernel.collide_units.bind(owner.kernel);
 owner.kernel.collide_units=count=>{counts.push(count);return run(count);};
 const firstBounds=first.unit.sphere,secondBounds=second.unit.sphere;
 assert.deepEqual(counts,[2]);
 first.units.query(new THREE.Vector3(),10,[]);assert.deepEqual(counts,[2]);
 second.unit.tickCount++;second.soldier.posture='crouch';second.unit.hitShapes();
 assert.deepEqual(counts,[2,1]);assert.equal(owner.collision.actors[0],1);
 assert.equal(first.unit.sphere,firstBounds);assert.equal(second.unit.sphere,secondBounds);
 // A generation or prepared-resource binding change also invalidates once.
 second.unit.generation++;second.unit.sphere;assert.deepEqual(counts,[2,1,1]);
 owner.collision.bind(owner.byUnit.get(second.unit),second.unit);second.unit.sphere;
 assert.deepEqual(counts,[2,1,1,1]);
 } finally {owner.kernel.free();}
});

// Cold lease admission may detach all held views, even in a collision query.
test('collision input reacquires every view after lease admission grows memory',async()=>{
 const {UnitCollisionInput:I}=await import('../src/presentation/layout.js');
 const {unit,soldier,host}=body(),owner=host.owner,collision=owner.collision;
 const previous=owner.lease;let admissions=0;
 owner.lease=()=>{admissions++;owner.loaded.memory.grow(1);unit.seat.posePick={clip:3,timed:true,time:2};};
 unit.seat={index:2,exposed:true,sourceRig:7};soldier.position.set(3,4,5);
 const old=collision.input;collision.write(host.row,unit);
 assert.equal(admissions,1);assert.notEqual(collision.input.buffer,old.buffer);
 const at=host.row.id*I.STRIDE,x=collision.input;
 assert.equal(x[at+I.POSITION],3);assert.equal(x[at+I.SEAT],1);assert.equal(x[at+I.SOURCE_SEAT],2);assert.equal(x[at+I.CLIP],3);
 assert.equal(collision.actors.buffer,owner.loaded.memory.buffer);assert.equal(collision.shapes.buffer,owner.loaded.memory.buffer);
 owner.lease=previous;
});
