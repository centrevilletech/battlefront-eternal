import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { F3Counters, F3UnitBinding } from '../src/sim/wasm/f3-contract.js';
import { F3UnitMirrorMirror, F3SoldierMotionMirror, F3DroidekaMotionMirror, F3ShieldViewMirror, putF3UnitMirror, syncF3UnitMirror } from '../src/sim/gen/units.js';
import { Heap } from '../src/sim/runtime.js';

function fixture(walker = false) {
  const body = walker ? new F3DroidekaMotionMirror() : new F3SoldierMotionMirror();
  for (const name of ['position','previous','settled','velocity','avoidance']) body[name] = new THREE.Vector3();
  if (walker) body.shield = Object.assign(new F3ShieldViewMirror(), {value:100,on:true,max:100,drain:2});
  const unit = {soldier:body,combat:{remotes:null},walker,generation:1,alive:true,dead:false,removed:false,team:1,deadTime:0,tickCount:1,targetable:true};
  unit.supply = {position:body.position}; unit.itemTarget = {position:body.position,velocity:body.velocity};
  return unit;
}
function row(walker = false) {
  const view = new F3UnitMirrorMirror(); view.key.slot=1; view.key.generation=3; view.key.life=2;
  view.position.set(4,5,6); view.alive=true; view.team=2; view.tickCount=20;
  if (walker) { view.droideka=new F3DroidekaMotionMirror(); view.droideka.shield=Object.assign(new F3ShieldViewMirror(),{value:75,on:false}); }
  else { view.soldier=new F3SoldierMotionMirror(); view.soldier.normal.set(0,1,0); view.soldier.fuel=.25; view.soldier.aimPitch=-0; }
  const f=new Float64Array(256), heap=new Heap(); putF3UnitMirror({},view,f,0,heap); assert.equal(heap.words.length,0);
  return f;
}

for (const walker of [false,true]) test(`generated unit publication keeps pooled identities (${walker ? 'droideka' : 'soldier'})`, () => {
  const unit=fixture(walker), body=unit.soldier, position=body.position, velocity=body.velocity, normal=body.normal, shield=body.shield;
  const binding=new F3UnitBinding(unit,{slot:1,generation:3,life:1},true), f=row(walker);
  binding.sync({},f,0,0); binding.check();
  assert.equal(unit.soldier,body); assert.equal(body.position,position); assert.equal(body.velocity,velocity);
  assert.equal(unit.supply.position,position); assert.equal(unit.itemTarget.position,position); assert.equal(unit.itemTarget.velocity,velocity);
  assert.deepEqual(position.toArray(),[4,5,6]); assert.equal(unit.generation,2);
  if (walker) { assert.equal(body.shield,shield); assert.equal(shield.max,100); assert.equal(shield.value,75); }
  else { assert.equal(body.normal,normal); assert.equal(body.fuel,.25); }
  // Intent belongs to the controller; no trap or mirror field covers it.
  unit.intent={forward:1}; binding.check();
  body.position.x++; assert.throws(()=>binding.check(),/F3 mirror write: soldier.position.x/);
});

test('mirror detector retains NaN and signed zero and reuses publication storage',()=>{
  const unit=fixture(), binding=new F3UnitBinding(unit,{slot:1,generation:1,life:1},true);
  binding.sync({},row(),0,0); binding.check();
  const values=binding.detector.values.get(unit), pair=values[0];
  binding.published(); assert.equal(binding.detector.values.get(unit),values); assert.equal(values[0],pair);
  unit.soldier.aimPitch=0; assert.throws(()=>binding.check(),/soldier.aimPitch/);
});

test('generated body view has no variable payload or class and rig output',()=>{
  const view=new F3UnitMirrorMirror(); syncF3UnitMirror({},row(),0,0,view);
  assert.equal('maxSpeed' in view.soldier,false); assert.equal('muzzleAnimator' in view.soldier,false); assert.equal('shapes' in view,false);
  const c=new F3Counters(); c.phaseCalls++; c.inputFields+=11; c.outputFields+=83;
  const snapshot=c.snapshot(); c.reset(); assert.equal(snapshot.phaseCalls,1); assert.equal(c.phaseCalls,0);
});

test('combat and weapon writes are detected on the same pooled mirror',()=>{
  const unit=fixture(), binding=new F3UnitBinding(unit,{slot:1,generation:0,life:1},true);
  const state={kick:{rate:0,hold:0,offset:0},arsenal:{selected:[0,0],weapons:[{clip:9}]}};
  for(const name of ['origin','target','aimDir','shotDir','left','up','pivot']) state[name]=new THREE.Vector3();
  binding.bindCombat({state});binding.published();binding.check();
  state.arsenal.weapons[0].clip=8;assert.throws(()=>binding.check(),/weapon.0.clip/);
  binding.published();state.origin.x=1;assert.throws(()=>binding.check(),/combat.origin.x/);
});

test('invalid typed unit settings reject without trapping the battle module',async()=>{
  const {f3World}=await import('./fixtures/f3-world.js');
  const {putF3Change}=await import('../src/sim/gen/index.js');
  const w=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:300,weapons:[]}});
  const unit=w.spawn('soldier',0,0),p=w.phase,b=p.bridge,binding=p.bindings[unit.id];
  for(const action of ['poolRelease','metadata','mountedRandom','bodyAir','combatField','weaponField']) {
    p.begin(binding,action);p.change.field=256;
    b.views();putF3Change(b,p.change,b.f,p.layout.command/8,p.heap);
    assert.equal(p.x.f3_command(),0,action);
  }
  p.scalar(unit.id,'health',0,0,299);
  assert.equal(unit.combat.health,299,'valid commands still run after rejection');
});
