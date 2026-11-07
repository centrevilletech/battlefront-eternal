import test from 'node:test';
import assert from 'node:assert/strict';
import { f3World } from './fixtures/f3-world.js';
import { playerCombatWorld } from './fixtures/player-combat-world.js';
import { weaponDefinition } from './fixtures/client-weapons.js';
import { ClassicRandom, uniformRandom } from '../src/ai/random.js';
import { reconRandom } from '../src/runtime/recon-random.js';
import { KIND_RECON_DROID } from '../src/sim/gen/index.js';
import { captureRemoteReplay } from '../src/runtime/network-remote-replay.js';
import { encodeRemoteCorrection } from '../src/runtime/network-remote-codec.js';

function remoteDefinition(extra = {}) {
  const blaster = weaponDefinition('remote-blaster',0,{
    TriggerSingle:1,ShotDelay:0.1,SpreadThreshold:0,SpreadPerShot:0,HeatPerShot:0,
    YawSpread:0,PitchSpread:0,StandStillSpread:0,StandMoveSpread:0,
    ordnance:{class:'remote-bolt',base:'bolt',Velocity:60,LifeSpan:1,Damage:25},
  });
  const droid = {class:'probe',base:'droid',MaxHealth:40,AddHealth:-1,weapons:[blaster],...extra};
  return weaponDefinition('probe-launcher',1,{
    base:'remote',droid,ordnance:null,SwitchImmediately:1,
    InitialSalvoDelay:0,ShotDelay:0.1,YawSpread:0,PitchSpread:0,
    SpreadThreshold:0,SpreadPerShot:0,HeatPerShot:0,
    StandStillSpread:0,StandMoveSpread:0,
  });
}
async function fixture(extra, definitions = {}, ingress = false) {
  const remote = remoteDefinition(extra);
  const classes = {
    scout:{class:'scout',base:'soldier',MaxHealth:2000,weapons:[remote]},
    soldier:{class:'soldier',base:'soldier',MaxHealth:2000,weapons:[]},
    ...definitions,
  };
  const world = ingress ? await playerCombatWorld(classes, {}, {className:'rifleman',position:[5,0,0],defer:true}) : await f3World(classes);
  world.phase.recon.randomFor = key=>reconRandom(7,key);
  const owner = world.spawn('scout',0,0,true);
  const context = world.phase.recon.attach(12,owner);
  const weaponRandom = uniformRandom(new ClassicRandom(9));
  for (const weapon of owner.combat.arsenal.weapons) weapon.random = weaponRandom;
  world.phase.random.refresh(world.phase.bindings[owner.id]);
  return {...world,owner,context,weaponRandom};
}
function launch(world) {
  world.owner.intent.buttons[1] = 1;
  if(world.step)world.step();else world.phase.phase(1/60);
  world.owner.intent.buttons[1] = 0;
  // The native remote release is a pending shot rather than a facade call.
  for (let i = 0; i < 30 && !world.context.droids.length; i++) {if(world.step)world.step();else world.phase.phase(1/60);}
  assert.equal(world.context.droids.length,1);
  return world.context.droids[0];
}

test('owned remote launch, flight and shots use native services with no recon imports', {timeout:10000}, async(t) => {
  const world = await fixture();
  world.phase.counters.reset();
  const droid = launch(world), position = droid.position, velocity = droid.velocity;
  assert.ok(world.context.piloted === droid);
  assert.equal(world.context.active,true);
  assert.ok(droid.owner.sourceUnit === world.owner);
  assert.deepEqual(world.phase.recon.identityFor(droid),{id:0,generation:1});
  const launchCounts = world.phase.counters.snapshot();
  assert.equal(launchCounts.commandCalls,0);
  assert.equal(launchCounts.importCalls,launchCounts.ticks,'only the owner aimer crosses');
  assert.equal(launchCounts.rustAllocations,0);
  assert.equal(launchCounts.rustReallocations,0);
  const fired = [];
  world.phase.recon.onFired = (shot,at)=>fired.push([shot.weapon.name,at.toArray()]);
  const soldierPosition = world.owner.position.clone();
  world.owner.intent.forward = 1;
  world.owner.intent.buttons[0] = 1;
  for (let i = 0; i < 120; i++) world.phase.phase(1/60);
  assert.ok(droid.position === position);assert.ok(droid.velocity === velocity);
  assert.ok(droid.position.z > 1);
  assert.ok(world.owner.position.distanceTo(soldierPosition) < 0.001);
  assert.equal(fired.length,1,'press-only blaster consumes its edge once');
  assert.ok(world.ordnance.bolts.length <= 1);
  const replay = captureRemoteReplay(world.context,droid);
  assert.equal(replay.camera.aspect,1);
  assert.ok(replay.camera.fov > 0);
  const counts = world.phase.counters.snapshot();
  assert.equal(counts.commandCalls,0);assert.equal(counts.importCalls,counts.ticks);
  assert.equal(counts.rustAllocations,0);assert.equal(counts.rustReallocations,0);
  t.diagnostic(`${counts.ticks} ticks, ${counts.phaseCalls} phase calls, ${counts.importCalls} aimers, ${counts.rustAllocations} allocations`);
});

test('remote death blasts stay native and slot reuse preserves terminal and launch identity', {timeout:10000}, async() => {
  const world = await fixture({explosion:{class:'probe-death',Damage:5,DamageRadiusInner:2,DamageRadiusOuter:5,Push:0}});
  const droid = launch(world), oldPosition = droid.position;
  const oldOwner = droid.owner, oldLife = world.owner.generation;
  world.context.kill(droid);
  assert.equal(droid.dead,true);assert.equal(world.context.active,false);
  assert.ok(world.owner.combat.health < 2000);
  world.phase.phase(1/60);
  assert.equal(world.context.droids.length,0);
  // MultiplayerWorld rebinds first, then spawn replaces the arsenal/life.
  const rebound = world.phase.recon.rebind(12,world.owner);
  world.owner.spawn(0,0,0,0);
  assert.equal(world.owner.generation,oldLife+1);
  for (const weapon of world.owner.combat.arsenal.weapons) weapon.random = world.weaponRandom;
  world.phase.random.refresh(world.phase.bindings[world.owner.id]);
  assert.ok(rebound === world.context);
  world.phase.counters.reset();
  const next = launch(world);
  assert.ok(next !== droid);assert.ok(droid.position === oldPosition);
  assert.ok(droid.owner === oldOwner);assert.equal(oldOwner.generation,oldLife);
  assert.deepEqual(world.phase.recon.identityFor(next),{id:0,generation:2});
  assert.equal(next.owner.generation,oldLife+1);
  assert.equal(world.phase.counters.snapshot().rustAllocations,0);
});

test('a second launch keeps the existing camera pose until its first control step', async() => {
  const world = await fixture();
  world.owner.soldier.look(1280,-160,1,false);
  const first = launch(world);
  for (let tick=0;tick<8;tick++) world.phase.phase(1/60);
  const position = world.context.aimCamera.position.toArray();
  // SoldierCamera's YXZ order write recomposes the existing quaternion.
  // These exact synthetic values also cover the constructor's low-bit change.
  const rotation = [-0.01149446716179262,0.9637365077582728,0.04207764464408274,0.2632665809778395];
  assert.ok(position.some(value=>value !== 0));
  world.context.kill(first); world.phase.phase(1/60);
  // Piloting paused the soldier's throw animation and timers. Let that
  // original weapon finish before issuing the next press edge.
  for (let tick=0;tick<30;tick++) world.phase.phase(1/60);
  const second = launch(world);
  assert.notEqual(second,first); assert.equal(world.context.piloted,second);
  assert.deepEqual(world.context.aimCamera.position.toArray(),position);
  assert.deepEqual(world.context.aimCamera.quaternion.toArray(),rotation);
  assert.equal(world.context.simulationView.fresh,true);
  assert.equal(world.context.simulationView.reach,1);
});

test('remote exit keeps later control edges for the soldier after short-circuit consumption', {timeout:10000}, async() => {
  const world = await fixture();
  launch(world);
  world.owner.intent.use = true;
  world.owner.intent.nextPrimary = true;
  world.phase.phase(1/60);
  assert.equal(world.context.active,false);
  assert.equal(world.owner.intent.use,false);
  assert.equal(world.owner.intent.nextPrimary,false,'the resumed soldier consumes the unvisited edge');
  assert.equal(world.context.droids[0].dead,false);
});

test('removed owners finish detached native strikes with idle controls and no imports', {timeout:10000}, async(t) => {
  const destruct = weaponDefinition('orbital',1,{
    base:'destruct',ordnance:null,MaxPressedTime:0.01,
    strike:{class:'orbital-bolt',base:'bolt',Velocity:60,LifeSpan:1,Damage:0},
    SalvoCount:3,SalvoDelay:0.1,AimDistance:10,AimElevation:90,ScatterDistance:1,
  });
  const world = await fixture({weapons:[destruct]});
  const launches = [];
  world.ordnance.onLaunch = shot=>launches.push(shot.def.class);
  const droid = launch(world);
  // The launch used secondary; a release tick makes the next press an edge.
  world.phase.phase(1/60);
  world.owner.intent.buttons[1] = 1;
  world.phase.phase(1/60);
  assert.ok(droid.salvo);
  assert.ok(droid.arsenal.weapons.some(weapon=>weapon.levels.includes(droid.salvo.def)));
  assert.ok(encodeRemoteCorrection(captureRemoteReplay(world.context,droid),droid).length > 0);
  assert.equal(launches.length,1);
  world.context.kill(droid);
  world.phase.recon.retire(12);
  world.owner.remove();
  world.phase.counters.reset();
  for (let i = 0; i < 30; i++) world.phase.phase(1/60);
  assert.deepEqual(launches,['orbital-bolt','orbital-bolt','orbital-bolt']);
  assert.equal(droid.dead,true);assert.equal(droid.salvo,null);
  assert.equal(world.phase.recon.contexts.size,0);
  const counts = world.phase.counters.snapshot();
  assert.equal(counts.phaseCalls,30);assert.equal(counts.importCalls,0);
  assert.equal(counts.rustAllocations,0);assert.equal(counts.rustReallocations,0);
  t.diagnostic('30 detached-strike ticks, 30 phase calls, zero imports or native allocations');
});


test('same-phase slot reuse keeps an earlier context pointing at its terminal droid', {timeout:10000}, async() => {
  const rifle = weaponDefinition('instant-rifle',0,{
    TriggerSingle:1,ShotDelay:1,SpreadThreshold:0,SpreadPerShot:0,HeatPerShot:0,
    YawSpread:0,PitchSpread:0,StandStillSpread:0,StandMoveSpread:0,
    ordnance:{class:'instant-bolt',base:'bolt',Velocity:60,LifeSpan:1,Damage:5000},
  });
  const arc = weaponDefinition('arc-reader',0,{
    TriggerSingle:1,ShotDelay:1,SpreadThreshold:0,SpreadPerShot:0,HeatPerShot:0,
    YawSpread:0,PitchSpread:0,StandStillSpread:0,StandMoveSpread:0,
    ordnance:{class:'arc-probe',base:'emitterordnance',ConeLength:1,ConeAngle:120,
      Damage:0,MaxDamage:0,DamageThreshold:1,MaxTargets:1},
  });
  const world = await fixture({}, {
    arc:{class:'arc',base:'soldier',MaxHealth:2000,weapons:[arc]},
    rifleman:{class:'rifleman',base:'soldier',MaxHealth:2000,weapons:[rifle]},
    bomb:{class:'bomb',base:'soldier',MaxHealth:100,weapons:[],chunks:[{}],ChunkFrequency:1,
      explosion:{class:'reuse-blast',Damage:1000,DamageRadiusInner:10,DamageRadiusOuter:11,Push:0}},
  }, true);
  const old = launch(world);
  const shooter = await world.admit();
  const bomb = world.spawn('bomb',5,1.5);
  bomb.combat.random = world.ordnance.random;
  const nextOwner = world.spawn('scout',30,0,true);
  const nextContext = world.phase.recon.attach(13,nextOwner);
  for (const weapon of nextOwner.combat.arsenal.weapons) weapon.random = world.weaponRandom;
  world.phase.random.refresh(world.phase.bindings[nextOwner.id]);
  // A still later arc acquisition must skip the retired native row too.
  const caster = world.spawn('arc',60,0);
  caster.combat.arsenal.weapons[0].random = world.ordnance.random;
  world.phase.random.refresh(world.phase.bindings[caster.id]);
  world.capture(world.game.worldClock.tick+10);
  assert.equal(world.fire({viewTick:world.game.worldClock.tick-2}),true);
  caster.intent.buttons[0] = 1;
  nextOwner.intent.buttons[1] = 1;
  // The first owner already advanced before the instant blast kills its
  // droid. A later owner reuses that slot before the first context steps again.
  world.step();
  assert.equal(old.dead,true);
  assert.equal(world.context.droids.length,1);
  assert.ok(world.context.droids[0] === old);
  const next = nextContext.droids[0];
  assert.ok(next !== old);
  assert.deepEqual(world.phase.recon.identityFor(next),{id:0,generation:2});
  assert.equal(next.dead,false);
  world.step();
  assert.equal(world.context.droids.length,0);
  assert.equal(world.phase.bridge.refs[KIND_RECON_DROID].displaced.size,0,
    'the next context publication releases the retained generation');
});

test('a newly captured launch owner is bound before an immediate death publishes its attacker', {timeout:10000}, async() => {
  const rifle = weaponDefinition('instant-rifle',0,{
    TriggerSingle:1,ShotDelay:1,SpreadThreshold:0,SpreadPerShot:0,HeatPerShot:0,
    YawSpread:0,PitchSpread:0,StandStillSpread:0,StandMoveSpread:0,
    ordnance:{class:'instant-bolt',base:'bolt',Velocity:60,LifeSpan:1,Damage:5000},
  });
  const world = await fixture({
    explosion:{class:'newborn-blast',Damage:10,DamageRadiusInner:10,DamageRadiusOuter:11,Push:0},
  }, {
    rifleman:{class:'rifleman',base:'soldier',MaxHealth:2000,weapons:[rifle]},
    bomb:{class:'bomb',base:'soldier',MaxHealth:100,weapons:[],chunks:[{}],ChunkFrequency:1,
      explosion:{class:'body-blast',Damage:1000,DamageRadiusInner:10,DamageRadiusOuter:11,Push:0}},
  }, true);
  const shooter = await world.admit();
  const bomb = world.spawn('bomb',5,1.5);
  bomb.combat.random = world.ordnance.random;
  world.phase.random.refresh(world.phase.bindings[bomb.id]);
  world.capture(world.game.worldClock.tick+10);
  assert.equal(world.fire({viewTick:world.game.worldClock.tick-2}),true);
  world.owner.intent.buttons[1] = 1;
  const prepared = world.phase.owners.frames.length;
  const reservedOwner = world.phase.owners.frames[0].snapshot;
  let barriers = 0;
  world.factory.context.deathNotice = () => {
    assert.equal(world.phase.active,false);
    assert.equal(world.phase.owners.frames.length,prepared);
    assert.ok(world.owner.combat.lastAttacker === reservedOwner);
    barriers++;
  };
  world.phase.counters.reset();
  world.step();
  assert.equal(barriers,1);
  const droid = world.phase.recon.slots[0].droid;
  assert.equal(droid.dead,true);
  assert.ok(world.owner.combat.lastAttacker === droid.owner);
  assert.equal(droid.owner.generation,world.owner.generation);
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.phaseCalls,1);
  assert.equal(world.phase.counters.rustAllocations,0);
});


test('network connection strings keep their seed key and pending Character before first spawn', async() => {
  const world = await f3World({scout:{class:'scout',base:'soldier',MaxHealth:300,weapons:[]}});
  const owner = world.factory.acquire('scout'), character = {unit:owner};
  owner.character = character;
  owner.human = true; owner.networkHuman = true;
  world.phase.recon.randomFor = key=>{assert.equal(key,'local');return reconRandom(7,key);};
  const context = world.phase.recon.rebind('local',owner);
  assert.ok(world.phase.recon.contexts.get('local').context === context);
  assert.equal(context.key,'local'); assert.ok(owner.character === character);
  owner.spawn(1,0,2,0);
  assert.ok(owner.character === character); assert.equal(owner.networkHuman,true);
  assert.equal(context.ownerKey.life,owner.generation);
});
