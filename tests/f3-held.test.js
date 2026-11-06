import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { AnimatedProp } from '../src/props.js';
import { f3World } from './fixtures/f3-world.js';

const soldier = {class:'soldier',base:'soldier',MaxHealth:2000,weapons:[]};
const joints = [
  ['DummyRoot',-1,0,0,0], ['bone_root',0,0,1,0], ['bone_pelvis',1,0,0,0],
  ['bone_neck',1,0,0.55,0], ['bone_head',3,0,0.12,0],
  ['bone_l_thigh',2,0.1,-0.1,0], ['bone_l_calf',5,0,-0.42,0], ['bone_l_foot',6,0,-0.35,0],
  ['bone_r_thigh',2,-0.1,-0.1,0], ['bone_r_calf',8,0,-0.42,0], ['bone_r_foot',9,0,-0.35,0],
];

// Missing tracks retain bind joints. This supplies the same root and limb
// vocabulary as a loaded soldier, without needing animation or game files.
function prepareUnit(unit) {
  unit.alive = true; unit.removed = false;
  const bones = unit.actor.bones;
  for (const [name,parent,x,y,z] of joints) {
    const bone = new THREE.Bone(); bone.name = name; bone.position.set(x,y,z);
    (parent < 0 ? unit.actor.root : bones[parent]).add(bone); bones.push(bone);
  }
  unit.animator.clips.set('hold', {frames:1,fps:30,rootMotion:false,
    tracks:new Int32Array(joints.length).fill(-1),data:new Float32Array()});
  unit.soldier.velocity.set(3,4,5); unit.soldier.air = 'fall'; unit.soldier.dive = 0.4;
}
function prop() {
  const root = new THREE.Group(), node = new THREE.Object3D();
  root.add(node); node.position.set(10,7,4);
  return {rig:{root},grabs:[{clip:'hold',node}],
    seize:AnimatedProp.prototype.seize,carry:AnimatedProp.prototype.carry};
}

test('a prop command owns seizure, root placement and lazy held shapes', async() => {
  const world = await f3World({soldier},{prepareUnit});
  const unit = world.factory.acquire('soldier');
  const position = unit.position, previous = unit.soldier.previous, settled = unit.soldier.settled;
  const shapes = unit.hitShapes(), stamp = unit.spheresTick, tick = unit.tickCount;
  const p = prop(); p.seize(unit,p.grabs[0],12);
  assert.equal(unit.held.prop,p); assert.equal(unit.held.grab,p.grabs[0]);
  assert.equal(unit.held.since,12); assert.equal(unit.held.frame,undefined);
  assert.equal(unit.combat.mounted,true); assert.equal(unit.soldier.air,null);
  assert.equal(unit.soldier.dive,0); assert.deepEqual(unit.velocity.toArray(),[0,0,0]);
  assert.equal(unit.tickCount,tick); assert.equal(unit.spheresTick,stamp);
  p.carry(18);
  const held = unit.held, frame = held.frame;
  assert.deepEqual(position.toArray(),[10,6,4]);
  assert.deepEqual(previous.toArray(),[10,6,4]); assert.deepEqual(settled.toArray(),[10,6,4]);
  assert.equal(unit.position,position); assert.equal(unit.soldier.previous,previous);
  assert.equal(unit.soldier.settled,settled); assert.equal(unit.tickCount,tick + 1);
  assert.equal(unit.spheresTick,stamp,'placement leaves observation lazy');
  assert.equal(unit.hitShapes(),shapes); assert.equal(shapes.length,6);
  assert.equal(unit.spheresTick,unit.tickCount);
  assert.equal(held.time,0.1); assert.deepEqual(frame.elements.slice(12,15),[10,6,4]);
  world.phase.check();
  held.time = 99;
  assert.throws(()=>world.phase.check(),/mirror write: held.time/);
  p.grabs[0].node.position.x = 11; p.carry(19);
  world.phase.check();
  assert.equal(unit.held,held); assert.equal(held.frame,frame);
  assert.deepEqual(position.toArray(),[11,6,4]);
  world.phase.counters.reset(); world.phase.phase(1/60);
  assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
  unit.letGo(); assert.equal(unit.held,null); assert.equal(unit.combat.mounted,false);
  p.carry(20); assert.equal(p.victim,null);
});

test('a missing held clip preserves placement and cache invalidation', async() => {
  const world = await f3World({soldier},{prepareUnit});
  const unit = world.factory.acquire('soldier'), p = prop();
  p.grabs[0].clip = 'absent'; unit.hitShapes();
  const tick = unit.tickCount, stamp = unit.spheresTick, position = unit.position.toArray();
  p.seize(unit,p.grabs[0],0); p.carry(1);
  assert.deepEqual(unit.position.toArray(),position);
  assert.equal(unit.tickCount,tick); assert.equal(unit.spheresTick,stamp);
  assert.equal(unit.held.frame,undefined);
});
