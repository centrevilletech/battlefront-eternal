import test from 'node:test';
import assert from 'node:assert/strict';
import {Soldier} from '../src/soldier.js';
import {PlayerCombat} from '../src/combat.js';
import {CollisionWorld} from '../src/collision.js';
import {InfantryPrediction} from '../src/runtime/infantry-prediction.js';
import {captureSoldierCorrection,SOLDIER_REPLAY_SCHEMA} from '../src/runtime/network-entity-correction.js';
const world=()=>new CollisionWorld(new Float32Array([-100,0,-100,100,0,-100,100,0,100,-100,0,100]),new Uint32Array([0,2,1,0,3,2]),new Uint32Array([0,1]));
function actor(){const soldier=new Soldier(world(),{});soldier.spawn(0,0,0,0);const combat=Object.create(PlayerCombat.prototype);Object.assign(combat,{soldier,kick:{rate:.4,hold:.8,offset:.1},arsenal:{clip:17},fire(){throw Error('prediction must not fire');},tick(){throw Error('prediction must not step weapons');}});return{soldier,combat};}
const input={forward:1,right:.2,turn:0,jump:false,crouch:false,prone:false};
const state=s=>captureSoldierCorrection(s,SOLDIER_REPLAY_SCHEMA);
test('production infantry step matches authoritative post-shot recoil without mutating combat or weapons',()=>{
 const authority=actor(),client=actor(),before=structuredClone(client.combat.kick),weapon=structuredClone(client.combat.arsenal);
 const prediction=new InfantryPrediction(client.soldier,client.combat);
 for(let tick=0;tick<10;tick++){
  authority.soldier.lookRaw(2,1,1,false);client.soldier.lookRaw(2,1,1,false);
  authority.soldier.tick(input,1/60);authority.combat.updateKick(1/60);
  prediction.step(input,1/60);
  assert.deepEqual(state(client.soldier),state(authority.soldier));assert.deepEqual(prediction.kick,authority.combat.kick);
 }
 assert.notEqual(client.soldier.aimPitch,0);assert.deepEqual(client.combat.kick,before);assert.deepEqual(client.combat.arsenal,weapon);
});
test('rolling frame checkpoints continue decaying recoil instead of restarting the authoritative kick',()=>{
 const authority=actor(),client=actor();let checkpoint;
 for(let frame=0;frame<12;frame++){
  const prediction=new InfantryPrediction(client.soldier,client.combat,checkpoint);
  for(let step=0;step<3;step++){authority.soldier.tick(input,1/60);authority.combat.updateKick(1/60);prediction.step(input,1/60);}
  checkpoint=prediction.kick;assert.deepEqual(state(client.soldier),state(authority.soldier));assert.deepEqual(checkpoint,authority.combat.kick);
 }
 assert.equal(client.combat.kick.rate,.4);
});
test('a fresh correction replaces replay recoil and an exhausted kick does not reappear',()=>{
 const client=actor();let replay=new InfantryPrediction(client.soldier,client.combat);
 for(let i=0;i<180;i++)replay.step(input,1/60);assert.deepEqual(replay.kick,{rate:0,hold:0,offset:0});
 const held=replay.kick;replay=new InfantryPrediction(client.soldier,client.combat,held);replay.step(input,1/60);assert.deepEqual(replay.kick,held);
 client.combat.kick={rate:.8,hold:.2,offset:.05};replay=new InfantryPrediction(client.soldier,client.combat);assert.deepEqual(replay.kick,client.combat.kick);assert.notEqual(replay.kick,client.combat.kick);
});
