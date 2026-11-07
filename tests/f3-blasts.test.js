import { CombatCapture } from './fixtures/projectile-capture-reference.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { f3World } from './fixtures/f3-world.js';
import { playerCombatWorld } from './fixtures/player-combat-world.js';
import { weaponDefinition } from './fixtures/client-weapons.js';
import { attachCombatCapture } from '../src/runtime/network-entity-combat.js';
import { SimEvents } from '../src/sim-events.js';

const explosion = {class:'body-blast',Damage:1000,DamageRadiusInner:6,DamageRadiusOuter:7,Push:0};

test('an instant lethal shot completes a native class blast before its source death', async() => {
  const calls = [];
  const bolt = {class:'fatal-bolt',base:'bolt',Velocity:60,LifeSpan:1,Damage:5000};
  const gun = weaponDefinition('rifle',0,{
    TriggerSingle:1,ShotDelay:1,SpreadThreshold:0,SpreadPerShot:0,HeatPerShot:0,
    YawSpread:0,PitchSpread:0,StandStillSpread:0,StandMoveSpread:0,ordnance:bolt,
  });
  const world = await playerCombatWorld({
    shooter:{class:'shooter',base:'soldier',MaxHealth:2000,weapons:[gun]},
    primed:{class:'primed',base:'soldier',MaxHealth:100,weapons:[],chunks:[{}],ChunkFrequency:1,explosion},
    neighbor:{class:'neighbor',base:'soldier',MaxHealth:100,weapons:[]},
  }, {
    deathCredit:unit=>calls.push(`credit:${unit.className}`),
    deathNotice:unit=>calls.push(`death:${unit.className}`),
    bodyNotice:unit=>calls.push(`apart:${unit.className}`),
    blastNotice:()=>calls.push('explosion'),
  });
  const shooter = world.shooter;
  const primed = world.spawn('primed',0,1.5), neighbor = world.spawn('neighbor',0,3);
  primed.combat.random = world.ordnance.random;
  shooter.combat.onDamaged = ()=>calls.push('damage:shooter');
  primed.combat.onDamaged = ()=>calls.push('damage:primed');
  neighbor.combat.onDamaged = ()=>calls.push('damage:neighbor');
  const capture = new CombatCapture({tick:()=>10,definitionId:()=>1});
  const dispose = attachCombatCapture({ordnance:world.ordnance,simEvents:new SimEvents()},capture);
  world.capture();
  assert.equal(world.fire(),true);
  world.phase.bodiesIn();world.phase.counters.reset();world.step();
  assert.equal(primed.dead,true);assert.equal(neighbor.dead,true);
  assert.equal(shooter.combat.health,1000);
  assert.deepEqual(calls,[
    'damage:primed','damage:shooter','damage:neighbor','death:neighbor',
    'explosion','apart:primed','death:primed',
  ]);
  // stepBolt notifies its terminal before applying the hit and the class blast.
  assert.deepEqual(capture.drain().map(record=>record.view.schema),[1,2,5]);
  assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.importCalls,0,'player input and battle credit stay native');
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
  dispose();
});

test('an outside blast publishes affected bodies once and preserves stale lazy shapes', async() => {
  const world = await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:2000,weapons:[]}});
  const near = world.spawn('soldier',0,0), far = world.spawn('soldier',20,0);
  const before = near.sphere.center.toArray(), stamp = near.spheresTick;
  world.phase.bodiesIn();world.phase.counters.reset();
  world.ordnance.explosion(explosion,new Vector3(0,1,1),null);
  assert.equal(near.combat.health,1000);assert.equal(far.combat.health,2000);
  assert.equal(near.spheresTick,stamp);
  assert.deepEqual(near.sphere.center.toArray(),before);
  assert.equal(world.phase.layout.outputRows,1,'an out-of-reach body has no publication');
  assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.commandCalls,1);
  world.ordnance.explosion({...explosion,PersonScale:0},new Vector3(0,1,1),null);
  assert.equal(near.combat.health,1000);
  assert.equal(world.phase.layout.outputRows,0,'zero-typed damage publishes no unchanged body');
});

// Gone is committed before a mine's own blast. The next live index is read
// after that recursive call returns, so neither gone mine explodes twice.
test('a native class blast finishes chained mines depth first inside the unit phase', async() => {
  const pop = {class:'mine-pop',Damage:10,PersonScale:0,VehicleScale:1,DamageRadiusInner:8,DamageRadiusOuter:8};
  const start = {...pop,class:'chain-start'};
  const mine = {class:'chain-mine',base:'mine',HealthType:'vehicle',MaxHealth:5,LifeSpan:30,ExplosionDeath:pop};
  const bolt = {class:'fatal-bolt',base:'bolt',Velocity:60,LifeSpan:1,Damage:5000};
  const gun = weaponDefinition('rifle',0,{
    TriggerSingle:1,ShotDelay:1,SpreadThreshold:0,SpreadPerShot:0,HeatPerShot:0,
    YawSpread:0,PitchSpread:0,StandStillSpread:0,StandMoveSpread:0,ordnance:bolt,
  });
  const events = [], completed = [];
  const world = await playerCombatWorld({
    shooter:{class:'shooter',base:'soldier',MaxHealth:2000,weapons:[gun]},
    primed:{class:'primed',base:'soldier',MaxHealth:100,weapons:[],chunks:[{}],ChunkFrequency:1,explosion:start},
  }, {blastNotice:def=>events.push(def.class)});
  const shooter = world.shooter, primed = world.spawn('primed',0,1.5);
  primed.combat.random = world.ordnance.random;
  const first = world.items.spawn(mine,new Vector3(1,0.1,1),new Vector3(),0,{owner:shooter});
  const second = world.items.spawn(mine,new Vector3(2,0.1,1),new Vector3(),0,{owner:shooter});
  for (let tick=0;tick<12;tick++) world.step();
  assert.equal(first.landed,true);assert.equal(second.landed,true);
  world.ordnance.onExplosion = def=>completed.push(def.class);
  world.capture();
  assert.equal(world.fire(),true);
  world.phase.bodiesIn();world.phase.counters.reset();world.step();
  assert.equal(primed.dead,true);assert.equal(shooter.combat.health,2000);
  assert.equal(first.gone,true);assert.equal(second.gone,true);
  assert.deepEqual(events,['chain-start','mine-pop','mine-pop']);
  assert.deepEqual(completed,['mine-pop','mine-pop','chain-start']);
  assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.importCalls,0,'player input and battle credit stay native');
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});
