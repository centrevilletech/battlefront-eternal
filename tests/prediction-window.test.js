import test from 'node:test';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/sim/wasm/ray-kernel.js';
import { MovementKernel } from '../src/sim/wasm/movement-kernel.js';
import { createClientPrediction } from '../src/sim/wasm/runtime/client-prediction.js';
import { Arsenal } from '../src/weapons.js';
import { PlayerCombat } from '../src/combat.js';
import assert from 'node:assert/strict';
import { retainPredictionCommands } from '../src/runtime/prediction-window.js';
import { InputLead, advanceInputClock } from '../src/runtime/reconcile.js';
import { FixedClock } from '../src/runtime/fixed-clock.js';
import { InputMailbox } from './fixtures/input-mailbox.js';
import {captureCompactSoldier,restoreCompactSoldier} from '../src/runtime/network-entity-soldier-compact.js';
import { Soldier } from './helpers/body.js';
import { CollisionWorld } from '../tools/collision-fixture.mjs';

test('five-second downstream blackout preserves upstream cadence and real Soldier prediction with bounded history', async () => {
  const instance=await loadSim(), rays=await loadRayKernel(instance);
  const collision = new CollisionWorld(new Float32Array([-100,0,-100,100,0,-100,100,0,100,-100,0,100]), new Uint32Array([0,2,1,0,3,2]), new Uint32Array([0,1]),undefined,undefined,rays);
  collision.movement=new MovementKernel(collision);
  const authority = new Soldier(collision, {}), local = new Soldier(collision, {});
  authority.spawn(0,0,0,0); local.spawn(0,0,0,0);
  const session=createClientPrediction(instance,collision);
  const unit={id:1,generation:1,soldier:local,combat:Object.assign(Object.create(PlayerCombat.prototype),{arsenal:new Arsenal([]),kick:{rate:0,hold:0,offset:0},zoomStep:0,gunUnderwater:false})};
  const options={epoch:1,tick:0,held:[]};
  const clock = new FixedClock(), lead = new InputLead({maxLead:24,extrapolate:true}), mailbox = new InputMailbox();
  let saved;
  let pending=[], held=[], sequence=0, latest=0, applied=0, maxPending=0, lastSent=0, maxGap=0, maxReplay=0, predicted=false, generation=1, serverGeneration=1, oldState;
  const positions=new Map();
  const step=(body, command)=>body.tick({forward:command.down.includes('KeyW')?1:0,right:0,turn:0,jump:false,crouch:false,prone:false},1/60);
  lead.arrive(0,0);
  for(let tick=1;tick<=480;tick++) {
    if(tick===430){serverGeneration=2;authority.spawn(10,0,0,0);}
    const command=mailbox.take(tick,serverGeneration);step(authority,command);
    if(tick===60){oldState=captureCompactSoldier(authority);}
    if(predicted)restoreCompactSoldier(local,saved);
    // Drop every snapshot AND its ACK for 300 server ticks, while upstream is intact.
    if(tick%3===0 && (tick<60 || tick>=360)) {
      const state=captureCompactSoldier(authority);restoreCompactSoldier(local,state);
      latest=applied=tick;generation=serverGeneration;lead.arrive(tick,tick/60);
      while(pending.length&&pending[0].sequence<=mailbox.ack)held=pending.shift().down;
    }
    if(tick===300){restoreCompactSoldier(local,oldState);applied=60;}
    const target=lead.target(tick/60,latest,.1);
    advanceInputClock(clock,target,1/60,(_dt,stamp)=>{
      const c={epoch:1,generation,sequence:++sequence,tick:stamp,down:tick>=90&&tick<300?['KeyW']:[],actions:[],look:[]};
      pending.push(c);pending=retainPredictionCommands(pending,stamp,generation,1);
      assert.ok(Math.abs(stamp-tick)<=120,'fresh upstream stamps remain inside the server admission window');
      mailbox.push(c);maxGap=Math.max(maxGap,tick-lastSent);lastSent=tick;
    },target+7);
    maxPending=Math.max(maxPending,pending.length);
    saved=captureCompactSoldier(local);
    options.tick=applied;options.held=held;unit.generation=generation;
    const result=session.predict(unit,pending,options);
    maxReplay=Math.max(maxReplay,result.ticks);
    predicted=true;positions.set(tick,local.position.z);
  }
  assert.ok(positions.get(290)>positions.get(200)+5,'own body continues moving after the original 120-command limit');
  assert.ok(positions.get(300)>=positions.get(299),'late stale authority does not rewind local pose');
  assert.equal(generation,2,'life changes replace the local checkpoint');
  assert.ok(maxGap<=2,'server never reaches its 500ms stale-input reset');
  assert.ok(maxPending<=120,'history remains bounded throughout the blackout');
  assert.ok(maxReplay<=120,'per-frame replay work remains bounded');
  assert.ok(local.position.distanceTo(authority.position)<.01,'fresh authority reconciles after the blackout');
  session.dispose();
});

test('local worker clock retains its existing bounded behavior',()=>{
  const worker=new InputLead({maxLead:24}),network=new InputLead({maxLead:24,extrapolate:true});
  worker.arrive(60,1);network.arrive(60,1);
  assert.ok(worker.target(6,60)<=84);
  assert.ok(network.target(6,60)>360);
});
