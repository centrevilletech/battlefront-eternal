import { CombatCapture } from './fixtures/projectile-capture-reference.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { f3World } from './fixtures/f3-world.js';
import { playerCombatWorld } from './fixtures/player-combat-world.js';
import { weaponDefinition } from './fixtures/client-weapons.js';
import { attachCombatCapture } from '../src/runtime/network-entity-combat.js';
import { SimEvents } from '../src/sim-events.js';
import { captureOrdnanceOwner } from '../src/ordnance-owner.js';
import { navigateMissile } from '../src/flight/targeting.js';
import { WaterState } from '../src/runtime/water-state.js';

test('the first owned water contact uses its construction-time mirror record', async() => {
  const water = new WaterState({
    height:1,cell:10,offset:[0,0],grid:2,cells:[[0,0]],look:{OscillationEnable:[0]},
  });
  const world = await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:2000,weapons:[]}},{water});
  const unit = world.spawn('soldier',0,0);
  const contact = world.phase.bindings[unit.id].waterContact;
  assert.ok(contact); assert.equal(unit.waterContact,undefined);
  world.phase.counters.reset(); world.phase.phase(1/60);
  assert.equal(unit.waterContact,contact); assert.equal(contact.under,0);
  assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
  world.phase.phase(1/60); assert.equal(unit.waterContact,contact);
});

// Ordnance.update must retain the lists reserved for later native firing.
// Alternating the real phases catches capacity lost between consecutive shots.
test('unit shots retain their capacity across ordnance filtering',async()=>{
  const gun=weaponDefinition('rapid-rifle',0,{ShotDelay:.01,RoundsPerClip:999,HeatPerShot:0,
    ordnance:{class:'rapid-bolt',base:'bolt',Velocity:60,LifeSpan:.3,Damage:0}});
  const world=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:300,weapons:[gun]}});
  const unit=world.spawn('soldier',0,0);
  unit.combat.arsenal.weapons[0].random=world.ordnance.random;
  unit.intent.buttons[0]=1;
  let shots=0;unit.combat.onFired=()=>shots++;
  world.phase.counters.reset();
  for(let tick=0;tick<120;tick++){
    world.phase.phase(1/60);
    world.ordnance.update(1/60);
  }
  assert.ok(shots>=60,'the weapon fires throughout the measured span');
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

// Unreliable held fire has no edge identity. Reliable fire carries the
// accepted edge's identity and never manufactures another press from held state.
test('native player fire attributes reliable edges and leaves held-only fire unattributed',async()=>{
  const gun=weaponDefinition('edge-rifle',0,{TriggerSingle:1,ShotDelay:.01,HeatPerShot:0,
    ordnance:{class:'edge-bolt',base:'bolt',Velocity:60,LifeSpan:1,Damage:0}});
  const f=await playerCombatWorld({shooter:{class:'shooter',base:'soldier',MaxHealth:300,weapons:[gun]}});
  const actor=f.world.players.get('shooter');
  const capture=new CombatCapture({tick:()=>10,definitionId:()=>1});
  const dispose=attachCombatCapture({ordnance:f.ordnance,simEvents:new SimEvents()},capture);
  const held=down=>f.world.input('shooter',{epoch:1,sequence:f.game.worldClock.tick+1,tick:f.game.worldClock.tick+1,
    generation:actor.unit.generation,down,actions:[],look:[]});
  const fireId=()=>capture.drain().map(e=>e.view).find(e=>e.schema===1)?.fireId;
  f.world.removePlayer('shooter');await f.world.addPlayer('shooter',{reliableFire:false,separateActions:true});
  held(['Mouse0']);f.step();assert.equal(fireId(),0);
  held([]);f.step();
  f.world.removePlayer('shooter');await f.world.addPlayer('shooter',{reliableFire:true,separateActions:true});
  assert.equal(f.fire({viewTick:0}),true);f.step();assert.equal(fireId(),41);
  assert.equal(f.fire({id:42,pressed:false,viewTick:0}),true);f.step();capture.drain();
  held(['Mouse0']);f.step();assert.equal(fireId(),undefined,'held state cannot recreate a consumed reliable edge');
  assert.equal(actor.unit.networkShot,null);assert.deepEqual(actor.unit.networkShots,[]);
  dispose();
});

test('F1 fat sweeps use canonical spheres and publish touched damage at their outer boundary', async() => {
  const world = await f3World({
    soldier:{ class:'soldier', base:'soldier', MaxHealth:2000, weapons:[] },
  });
  const shooter = world.spawn('soldier',0,0);
  const victim = world.spawn('soldier',0,2);
  const notices = [];
  victim.combat.onDamaged = (amount,attacker) => notices.push([amount,attacker === shooter]);
  world.phase.bodiesIn();
  const definition = {
    class:'fat-ray', base:'fatray', Velocity:12, LifeSpan:1, Damage:100,
  };
  world.ordnance.fire(definition,new Vector3(0,1,0),new Vector3(0,0,1),shooter);
  for (let tick=0;tick<20;tick++) world.ordnance.update(1/60);
  assert.equal(victim.combat.health,1900);
  assert.equal(shooter.combat.health,2000);
  assert.deepEqual(notices,[[100,true]],'a fat ray strikes each Unit only once over its lifetime');
  // This later unit phase must not erase or redeliver F1's preceding notice.
  world.phase.phase(1/60);
  assert.deepEqual(notices,[[100,true]]);
});

test('an instant native bolt keeps its launch and terminal snapshots without combat imports', async(t) => {
  const definition = { class:'instant-bolt', base:'bolt', Velocity:60, LifeSpan:1, Damage:50 };
  const gun = weaponDefinition('rifle',0,{
    TriggerSingle:1,ShotDelay:1,SpreadThreshold:0,SpreadPerShot:0,HeatPerShot:0,
    YawSpread:0,PitchSpread:0,StandStillSpread:0,StandMoveSpread:0,ordnance:definition,
  });
  const world = await playerCombatWorld({
    shooter:{ class:'shooter',base:'soldier',MaxHealth:2000,weapons:[gun] },
    victim:{ class:'victim',base:'soldier',MaxHealth:2000,weapons:[] },
  });
  const shooter = world.shooter;
  const victim = world.spawn('victim',0,1.5);
  const capture = new CombatCapture({tick:()=>10,definitionId:()=>1});
  const dispose = attachCombatCapture({ordnance:world.ordnance,simEvents:new SimEvents()},capture);
  // These are complete native frames, not a stubbed ray result. The launch
  // catches up from tick eight to ten and ends before fireRound returns.
  world.capture();
  assert.equal(world.fire({id:41}),true);
  world.phase.bodiesIn();world.phase.counters.reset();
  world.step();
  assert.ok(victim.combat.health < 2000);
  const records = capture.drain().map(entry=>entry.view);
  assert.deepEqual(records.map(record=>record.schema),[1,2]);
  assert.equal(records[0].id,records[1].id);
  assert.equal(records[0].launchTick,8);
  assert.equal(records[0].executionTick,10);
  assert.equal(records[0].life,1,'launch keeps the life before catch-up retired the bolt');
  assert.equal(records[0].fireId,41);
  assert.equal(records[0].ownerGeneration,shooter.generation);
  assert.ok(records[1].position[2] < 2);
  const measured = world.phase.counters.snapshot();
  assert.equal(measured.phaseCalls,1);
  assert.equal(measured.importCalls,1,'one declared observer import during the native interval');
  assert.equal(measured.commandCalls,0);
  assert.equal(measured.rustAllocations,0);
  assert.equal(measured.rustReallocations,0);
  t.diagnostic(`instant bolt: ${measured.inputFields} input words, ${measured.outputFields} output words`);
  dispose();
});

test('a delayed native bolt can hit a reused source while retaining its launch attribution', async() => {
  const world = await f3World({
    soldier:{ class:'soldier',base:'soldier',MaxHealth:2000,weapons:[] },
  });
  const source = world.spawn('soldier',0,0,true);
  const binding = world.phase.bindings[source.id];
  world.phase.begin(binding,'team');world.phase.change.value = 1;world.phase.commit();
  const owner = captureOrdnanceOwner(source), oldLife = source.generation;
  source.spawn(0,0,0,0);
  world.phase.begin(binding,'team');world.phase.change.value = 2;world.phase.commit();
  const notices = [];
  source.combat.onDamaged = (amount,attacker,info,sample) => notices.push({
    amount,attacker,life:sample.attackerRef.generation,team:sample.attackerTeam,
  });
  let tick = 10;
  const capture = new CombatCapture({tick:()=>tick,definitionId:()=>1});
  const dispose = attachCombatCapture({ordnance:world.ordnance,simEvents:new SimEvents()},capture);
  world.phase.bodiesIn();
  const round = world.ordnance.fire(
    { class:'delayed-bolt',base:'bolt',Velocity:120,LifeSpan:1,Damage:30 },
    new Vector3(0,1,-1),new Vector3(0,0,1),owner,
  );
  assert.equal(round.networkFireUnit,source.id);
  assert.equal(round.networkFireGeneration,oldLife);
  const launched = capture.drain();
  assert.deepEqual(launched.map(entry=>entry.record.schema),[1]);
  tick++;
  world.ordnance.update(1/60);
  const terminal = capture.drain();
  assert.deepEqual(terminal.map(entry=>entry.record.schema),[2]);
  assert.equal(terminal[0].record.id,launched[0].record.id,'retirement preserves the original launch identity');
  assert.equal(source.combat.health,1970);
  assert.deepEqual(notices,[{amount:30,attacker:owner,life:oldLife,team:1}]);
  assert.equal(owner.team,1);
  assert.equal(source.team,2);
  dispose();
});

test('native bolt flight preserves its precomputed terrain impact surface', async() => {
  const world = await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:300,weapons:[]}},{terrain:true});
  const source = world.spawn('soldier',10,0,true);
  const capture = new CombatCapture({tick:()=>10,definitionId:()=>1});
  const dispose = attachCombatCapture({ordnance:world.ordnance,simEvents:new SimEvents()},capture);
  world.ordnance.fire({class:'ground-bolt',base:'bolt',Velocity:120,LifeSpan:1,Damage:10},
    new Vector3(0,1,0),new Vector3(0,-1,0),source);
  capture.drain(); world.ordnance.update(1/60);
  const records = capture.drain();
  assert.equal(records.length,1); assert.equal(records[0].view.schema,2);
  assert.equal(records[0].view.surface,0,'the moving sweep does not reclassify the retained terrain hit');
  assert.deepEqual(records[0].view.position,[0,0,0]);
  dispose();
});

test('native guided rounds preserve flight navigation and their distinct lost-life rules', async() => {
  const world = await f3World({
    soldier:{ class:'soldier',base:'soldier',MaxHealth:2000,weapons:[] },
  });
  const target = world.spawn('soldier',3,25);
  world.phase.phase(1/60);
  const guidance = {targetId:target.id,targetGeneration:target.generation};
  const def = {class:'guided-missile',base:'missile',Velocity:20,LifeSpan:3,ScaleTime:1,TurnRate:6};
  const flight = world.ordnance.fire(def,new Vector3(0,2,0),new Vector3(0,0,1),null,null,null,null,guidance);
  world.ordnance.flightMissile(flight,'fighter',3,20);
  const homing = world.ordnance.fire(def,new Vector3(-3,2,0),new Vector3(0,0,1),null,null,null,null,guidance);
  const expected = {position:flight.position.clone(),velocity:flight.velocity.clone(),flightClass:'fighter',targetId:target.id};
  const bridge = world.phase.bridge;
  world.phase.counters.reset();bridge.phaseCounters = world.phase.counters;
  try {
    for (let tick=0;tick<6;tick++) {
      expected.position.copy(flight.position);expected.velocity.copy(flight.velocity);
      navigateMissile(expected,target,1/60);
      world.ordnance.update(1/60);
      assert.deepEqual(flight.velocity.toArray(),expected.velocity.toArray());
    }
    // PN navigation retains the invalid target id; ordinary homing clears it.
    // Neither may follow the next life on the same pooled target object.
    target.spawn(3,0,25,0);
    world.ordnance.update(1/60);
    assert.equal(flight.targetId,target.id);
    assert.equal(homing.targetId,-1);
    assert.equal(world.phase.counters.importCalls,0);
  } finally { bridge.phaseCounters = null; }
});

test('native F1 water crossings share the phase surface and reject dry cells', async() => {
  const water = new WaterState({
    height:1,cell:10,offset:[0,0],grid:2,cells:[[0,0]],look:{OscillationEnable:[1]},
  });
  const world = await f3World({}, {water});
  // A completed world surface is an input to this standalone flight fixture.
  water.physicsWave=0.0125;world.phase.phase(1/60);
  const def = {class:'water-shell',base:'shell',Velocity:10,LifeSpan:2,Gravity:1};
  world.ordnance.fire(def,new Vector3(1,water.level+0.001,1),new Vector3(0,-1,0),null);
  const dry = world.ordnance.fire(def,new Vector3(15,water.level+0.001,15),new Vector3(0,-1,0),null);
  const up = world.ordnance.fire(def,new Vector3(1,water.level-0.001,1),new Vector3(0,1,0),null);
  const bridge = world.phase.bridge;
  world.phase.counters.reset();bridge.phaseCounters = world.phase.counters;
  try {
    world.ordnance.update(1/60);
    assert.deepEqual(world.ordnance.rounds,[dry,up]);
    assert.equal(world.phase.counters.importCalls,0);
  } finally { bridge.phaseCounters = null; }
});

test('a native head hit credits the prior captured Character before replacing the attacker', async() => {
  const bullet = {class:'head-credit-bolt',base:'bolt',Velocity:60,LifeSpan:1,Damage:30};
  const gun = weaponDefinition('rifle',0,{
    TriggerSingle:1,ShotDelay:1,SpreadThreshold:0,SpreadPerShot:0,HeatPerShot:0,
    YawSpread:0,PitchSpread:0,StandStillSpread:0,StandMoveSpread:0,ordnance:bullet,
  });
  const world = await playerCombatWorld({
    shooter:{class:'shooter',base:'soldier',MaxHealth:2000,weapons:[gun]},
    soldier:{class:'soldier',base:'soldier',MaxHealth:2000,weapons:[]},
  });
  await world.world.addPlayer('prior',{team:1,className:'soldier'});
  const priorActor = world.world.players.get('prior'), prior = priorActor.unit, oldCharacter = priorActor.character;
  world.game.unitsSim.placeBody(prior,10,0,0,0);
  const oldOwner = captureOrdnanceOwner(prior);
  world.world.removePlayer('prior');await world.world.addPlayer('prior',{team:1,className:'soldier'});
  assert.ok(prior.generation>oldOwner.generation);
  assert.equal(oldOwner.character,oldCharacter);
  const shooter = world.shooter;
  const victim = world.spawn('soldier',0,1.5);
  victim.combat.damage(1,1,null,oldOwner);
  world.capture();
  assert.equal(world.fire({id:42}),true);
  world.phase.bodiesIn();
  world.phase.counters.reset();world.step();
  assert.equal(oldCharacter.record.slots[0].headshots,1,'the prior launching Character receives the head hit');
  assert.equal(victim.combat.health,1909);
  assert.equal(victim.combat.lastAttacker,shooter);
  assert.equal(world.phase.counters.importCalls,1,'one declared observer import during the native interval');
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

test('literal player damage resolves credit while preserving its distinct self-hit and launch rules', {timeout:10000}, async() => {
  const character = {}, victimCharacter = {}, headCredits = [];
  const world = await f3World({
    soldier:{class:'soldier',base:'soldier',MaxHealth:2000,weapons:[]},
    victim:{class:'victim',base:'soldier',MaxHealth:2000,weapons:[]},
  }, {
    passiveClasses:['soldier'],
    prepareUnit:unit=>{
      unit.human = true; unit.team = 1; unit.combat.team = 1;
      unit.character = unit.className === 'soldier' ? character : victimCharacter;
    },
    headshotCredit:value=>headCredits.push(value),
  });
  const player = world.passive[0];player.spawn(0,0,0,0);
  world.phase.registerViewpoint(player);
  const victim = world.spawn('victim',0,2);
  const damage = [];
  player.combat.onDamaged = (amount,attacker,info,sample)=>damage.push({
    amount,attacker,id:sample.attackerRef.id,life:sample.attackerRef.generation,team:sample.attackerTeam,
  });
  world.phase.bodiesIn();
  const definition = {class:'bolt',base:'bolt',Velocity:120,LifeSpan:1,Damage:50};
  const literal = world.ordnance.fire(definition,new Vector3(0,1,0),new Vector3(0,0,1),'player');
  world.ordnance.update(1/60);world.ordnance.update(1/60);
  assert.equal(player.combat.health,1950);assert.equal(victim.combat.health,2000);
  assert.deepEqual(damage,[{amount:50,attacker:'player',id:player.id,life:player.generation,team:1}]);
  assert.equal(literal.networkFireUnit,0);assert.equal(literal.networkFireGeneration,0);
  // An ordinary Unit owner excludes its current body. The literal deliberately
  // does not, and only the damage/credit observers resolve it to the viewpoint.
  victim.combat.damage(1,1,null,'player');
  world.ordnance.fire(definition,new Vector3(0,1.7,0),new Vector3(0,0,1),player);
  world.ordnance.update(1/60);world.ordnance.update(1/60);
  assert.equal(headCredits.length,1);assert.ok(headCredits[0] === character);
  assert.equal(player.combat.health,1950);assert.ok(victim.combat.health < 1999);
});
