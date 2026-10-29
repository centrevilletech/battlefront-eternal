import test from 'node:test';
import assert from 'node:assert/strict';
import { PredictionWindow, retainPredictionCommands, resetPredictionIdentity } from '../src/runtime/prediction-window.js';
import { InputLead, advanceInputClock, replayTicks, CorrectionSmoother } from '../src/runtime/reconcile.js';
import { FixedClock } from '../src/runtime/fixed-clock.js';
import { InputMailbox } from '../src/runtime/transport.js';
import {captureCompactSoldier,restoreCompactSoldier} from '../src/runtime/network-entity-soldier-compact.js';
import { Soldier } from '../src/soldier.js';
import { CollisionWorld } from '../src/collision.js';

test('server identity handover cannot continue the provisional body at the same generation and tick', () => {
  const world = new CollisionWorld(new Float32Array([-100,0,-100,100,0,-100,100,0,100,-100,0,100]), new Uint32Array([0,2,1,0,3,2]), new Uint32Array([0,1]));
  const soldier = new Soldier(world, {}); soldier.spawn(0,0,0,0);
  const client = { predictionWindow:new PredictionWindow(),vehicleWindow:new PredictionWindow(),smoother:new CorrectionSmoother(),vehicleSmoother:new CorrectionSmoother(),predictedTick:25,vehiclePrediction:{},held:['KeyW'],heldGeneration:1 };
  const state=captureCompactSoldier(soldier);
  client.predictionWindow.save({source:soldier,identity:1,epoch:1,generation:1,authorityTick:20,tick:25,commands:[],held:['KeyW'],state});
  const context={source:soldier,identity:90,epoch:1,generation:1,authorityTick:20,pending:[],held:[]};
  assert.equal(client.predictionWindow.select(context).checkpoint,null,'unit ID is part of prediction identity even if the object and generation match');
  client.smoother.note(0,0,0,.2,0,0);
  resetPredictionIdentity(client);
  soldier.spawn(80,0,30,0);
  const replay=client.predictionWindow.select(context);
  const checkTick=client.predictedTick>replay.from?client.predictedTick:0;
  replayTicks(replay.commands,replay.from,1,replay.held,()=>assert.fail('no provisional input should replay'));
  assert.equal(checkTick,0,'handover cannot compare old prediction coordinates with the assigned body');
  assert.deepEqual(soldier.position.toArray(),[80,0,30]);assert.equal(client.smoother.length,0);
  assert.equal(client.smoother.snaps,0,'identity change is not recorded as a same-life rubberband');
});

test('five-second downstream blackout preserves upstream cadence and real Soldier prediction with bounded history', () => {
  const collision = new CollisionWorld(new Float32Array([-100,0,-100,100,0,-100,100,0,100,-100,0,100]), new Uint32Array([0,2,1,0,3,2]), new Uint32Array([0,1]));
  const authority = new Soldier(collision, {}), local = new Soldier(collision, {});
  authority.spawn(0,0,0,0); local.spawn(0,0,0,0);
  const window = new PredictionWindow(), clock = new FixedClock(), lead = new InputLead({maxLead:24,extrapolate:true}), mailbox = new InputMailbox();
  let saved, continuation;
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
    const context={source:local,epoch:1,generation,authorityTick:applied,pending,held};
    const selected=window.select(context);
    if(selected.checkpoint)restoreCompactSoldier(local,selected.checkpoint.state);
    let work=0;
    const end=replayTicks(selected.commands,selected.from,generation,selected.held,(_t,c)=>{work++;step(local,c);});
    maxReplay=Math.max(maxReplay,work);
    continuation=captureCompactSoldier(local);
    window.save({...context,tick:end,commands:selected.commands,held:selected.held,state:continuation});
    predicted=true;positions.set(tick,local.position.z);
  }
  assert.ok(positions.get(290)>positions.get(200)+5,'own body continues moving after the original 120-command limit');
  assert.ok(positions.get(300)>=positions.get(299),'late stale authority does not rewind local pose');
  assert.equal(generation,2,'life changes replace the local checkpoint');
  assert.ok(maxGap<=2,'server never reaches its 500ms stale-input reset');
  assert.ok(maxPending<=120,'history remains bounded throughout the blackout');
  assert.ok(maxReplay<=120,'per-frame replay work remains bounded');
  assert.ok(local.position.distanceTo(authority.position)<.01,'fresh authority reconciles after the blackout');
});

test('prediction checkpoints reject life/epoch/source changes and reconcile only usable authority',()=>{
  const window=new PredictionWindow(),source={},state={position:42};
  const base={source,epoch:1,generation:1,authorityTick:10,pending:[],held:[]};
  window.save({...base,tick:300,commands:[{sequence:290,down:['KeyW']}],state});
  assert.equal(window.select({...base,authorityTick:100}).checkpoint?.state,state,'very stale state cannot replay missing history');
  assert.equal(window.select({...base,authorityTick:290}).checkpoint,null,'fresh authority wins');
  for(const changed of [{generation:2},{epoch:2},{source:{}}]) {
    window.save({...base,tick:300,commands:[],state});
    assert.equal(window.select({...base,...changed}).checkpoint,null);
  }
  window.save({...base,tick:300,commands:[],state});window.reset();
  assert.equal(window.select(base).checkpoint,null);
});

test('local worker clock retains its existing bounded behavior',()=>{
  const worker=new InputLead({maxLead:24}),network=new InputLead({maxLead:24,extrapolate:true});
  worker.arrive(60,1);network.arrive(60,1);
  assert.ok(worker.target(6,60)<=84);
  assert.ok(network.target(6,60)>360);
});
