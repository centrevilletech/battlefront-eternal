import test from 'node:test';
import {worldFrame} from './helpers/world-frame.js';
import assert from 'node:assert/strict';
import { f3World } from './fixtures/f3-world.js';
import { weaponDefinition } from './fixtures/client-weapons.js';
import { fleetFixture } from './fixtures/f3-fleet.js';
import { Vector3 } from 'three';

const mine = {class:'fixture-mine',base:'mine',Gravity:1,LifeSpan:30,MaxHealth:40};
const dispenser = weaponDefinition('layer',0,{
  base:'dispenser',item:mine,ordnance:null,MaxItems:2,TriggerSingle:1,
  RoundsPerClip:2000,ammo:0,ShotDelay:0,HeatPerShot:0,
  YawSpread:0,PitchSpread:0,StandStillSpread:0,StandMoveSpread:0,SpreadPerShot:0,
});
const definition = {class:'soldier',base:'soldier',MaxHealth:2000,weapons:[dispenser]};

test('owned dispensers preserve lazy owner numbering, capped lists and pooled item vectors', async() => {
  const world = await f3World({soldier:definition});
  const first = world.spawn('soldier',0,0);
  const second = world.spawn('soldier',10,0);
  first.combat.arsenal.weapons[0].random = world.ordnance.random;
  second.combat.arsenal.weapons[0].random = world.ordnance.random;
  world.phase.bodiesIn();world.phase.counters.reset();
  second.intent.buttons[0] = 1;world.phase.phase(1/60);
  assert.deepEqual([...world.items.owned.keys()],['layer:unit:1:1']);
  assert.equal(world.items.world.ownerIds.get(second),1);
  assert.equal(world.items.world.ownerIds.get(first),undefined);
  const initial = world.items.items[0], position = initial.position;
  first.intent.buttons[0] = 1;second.intent.buttons[0] = 0;world.phase.phase(1/60);
  assert.deepEqual([...world.items.owned.keys()],['layer:unit:1:1','layer:unit:2:1']);
  for (let tick=0;tick<8;tick++) {
    first.intent.buttons[0] = 0;second.intent.buttons[0] = tick % 2;
    world.phase.phase(1/60);
  }
  assert.equal(world.items.owned.get('layer:unit:1:1').length,2);
  assert.equal(world.items.owned.get('layer:unit:2:1').length,1);
  assert.equal(initial.gone,true);
  assert.equal(initial.position,position);
  assert.equal(world.phase.counters.phaseCalls,10);
  assert.equal(world.phase.counters.wasmCalls,10);
  assert.equal(world.phase.counters.importCalls,20,'only the two post-movement aimers cross');
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

test('an empty failed throw remains a Map entry until item update and reactivates in order', async() => {
  const world = await f3World({soldier:definition},{itemPools:{mine:0}});
  const first = world.spawn('soldier',0,0);
  const second = world.spawn('soldier',10,0);
  first.combat.arsenal.weapons[0].random = world.ordnance.random;
  second.combat.arsenal.weapons[0].random = world.ordnance.random;
  second.intent.buttons[0] = 1;world.phase.phase(1/60);
  first.intent.buttons[0] = 1;second.intent.buttons[0] = 0;world.phase.phase(1/60);
  assert.deepEqual([...world.items.owned.keys()],['layer:unit:1:1','layer:unit:2:1']);
  assert.equal(world.items.items.length,0);
  worldFrame(world)();
  assert.equal(world.items.owned.size,0);
  first.intent.buttons[0] = 0;second.intent.buttons[0] = 1;
  world.phase.counters.reset();world.phase.phase(1/60);
  assert.deepEqual([...world.items.owned.keys()],['layer:unit:1:1']);
  assert.equal(world.items.world.nextOwnerId,3);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

test('native item flight and retirement preserve old mirrors before reusing a slot', async() => {
  const timed = {...mine,class:'timed-mine',LifeSpan:0.02};
  const gun = {...dispenser,item:timed};
  const world = await f3World({soldier:{...definition,weapons:[gun]}});
  const unit = world.spawn('soldier',0,0);
  unit.combat.arsenal.weapons[0].random = world.ordnance.random;
  unit.intent.buttons[0] = 1;world.phase.phase(1/60);
  const old = world.items.items[0], position = old.position;
  const bridge = world.phase.bridge;
  const step=worldFrame(world);
  world.phase.counters.reset();bridge.phaseCounters = world.phase.counters;
  try {
    for (let tick=0;tick<3;tick++) step();
    assert.equal(world.items.items.length,0);
    assert.equal(world.items.owned.size,0);
    assert.equal(world.phase.counters.commandCalls,0,'the coordinator supplies native item targets');
  } finally { bridge.phaseCounters = null; }
  assert.equal(old.gone,true);
  assert.equal(old.position,position);
  const terminal = position.toArray();
  unit.intent.buttons[0] = 0;world.phase.phase(1/60);
  unit.intent.buttons[0] = 1;world.phase.counters.reset();world.phase.phase(1/60);
  const next = world.items.items[0];
  assert.equal(next.simSlot,old.simSlot);
  assert.notEqual(next.simGen,old.simGen);
  assert.notEqual(next,old);
  assert.deepEqual(old.position.toArray(),terminal);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

test('the pre-phase mine policy can assign an owner number before that owner throws', async() => {
  const world = await f3World({soldier:definition});
  const first = world.spawn('soldier',0,0), second = world.spawn('soldier',10,0);
  first.combat.arsenal.weapons[0].random = world.ordnance.random;
  second.combat.arsenal.weapons[0].random = world.ordnance.random;
  assert.deepEqual(world.items.ownedBy('layer',first),[]);
  second.intent.buttons[0] = 1;world.phase.phase(1/60);
  assert.deepEqual([...world.items.owned.keys()],['layer:unit:2:1']);
  assert.equal(world.items.world.ownerIds.get(first),1);
  assert.equal(world.items.world.ownerIds.get(second),2);
});

test('ordered native pickups heal hero health and supply ammunition without setter reentry', async() => {
  const powerup = {class:'pickup',base:'powerupitem',PersonHealth:50,PersonAmmo:3,LifeSpan:30};
  const rifle = weaponDefinition('rifle',0);
  const maker = weaponDefinition('supply-pack',1,{base:'dispenser',item:powerup,ordnance:null});
  const world = await f3World({hero:{class:'hero',base:'soldier',MaxHealth:1000000,weapons:[rifle,maker]}});
  const first = world.spawn('hero',0,0,true), second = world.spawn('hero',0,0,true);
  first.hero = second.hero = true;
  world.phase.publishMetadata(first); world.phase.publishMetadata(second);
  const firstRifle = first.combat.arsenal.weapons[0], secondRifle = second.combat.arsenal.weapons[0];
  firstRifle.reserve = 4; first.combat.arsenal.weapons[1].reserve = 0;
  secondRifle.reserve = 0; second.combat.health = 999900;
  const position = second.position, effects = [];
  world.phase.presentEffect = name=>effects.push(name);
  world.items.world.pickup = ()=>effects.push('pickup');
  const item = world.items.spawn(powerup,new Vector3(0,.1,0),new Vector3(),0);
  const counters = world.phase.counters, bridge = world.phase.bridge;
  const step=worldFrame(world,first);
  counters.reset(); bridge.phaseCounters = counters;
  try { for (let tick=0;tick<40 && !item.gone;tick++) step(); }
  finally { bridge.phaseCounters = null; }
  assert.equal(item.gone,true);
  assert.equal(first.combat.health,1000000,'the full first target declines the pickup');
  assert.equal(second.combat.health,999950); assert.equal(secondRifle.reserve,3);
  assert.equal(first.combat.arsenal.weapons[1].reserve,0,'power-up dispensers take no power-ups');
  assert.equal(second.position,position);
  assert.deepEqual(effects,['Powerup_blue','Powerup_red','pickup']);
  assert.equal(counters.commandCalls,0);
});

test('an ammunition-only hull pickup publishes its touched mounted weapon immediately', async() => {
  const powerup = {class:'vehicle-pickup',base:'powerupitem',VehicleAmmo:3,LifeSpan:30};
  const world = await f3World({soldier:definition},{
    prepareFleet:fleetFixture({positions:[0],weapon:weaponDefinition('mounted-rifle',0),reserve:0}),
  });
  const hull = world.factory.context.vehicles.vehicles[0];
  const weapon = hull.seats[0].weapons[0].state, position = hull.position;
  // The hull's root is above this synthetic floor. Drop through that root
  // after the original half-second pickup delay, rather than below its reach.
  const item = world.items.spawn(powerup,new Vector3(position.x,position.y + 1.5,position.z),new Vector3(),0);
  const step=worldFrame(world,world.spawn('soldier',80,80,true,null));
  const counters = world.phase.counters; counters.reset();
  for (let tick=0;tick<40 && !item.gone;tick++) step();
  assert.equal(item.gone,true); assert.equal(hull.health,hull.maxHealth);
  assert.equal(weapon.reserve,3,'no later unit or fleet phase is needed to see the supply');
  assert.equal(hull.seats[0].weapons[0].state,weapon); assert.equal(hull.position,position);
  assert.equal(counters.commandCalls,0);
});

// Timer explosions must pass through the real enclosing tick, including the
// thrower's canonical identity and the item retirement publication.
test('a native timed dispenser item expires once and credits its real thrower',async()=>{
  const blasts=[];
  const expiry={class:'timed-expiry',Damage:1,DamageRadiusInner:.1,DamageRadiusOuter:.1};
  const timed={...mine,class:'timed-charge',LifeSpan:.1,ExplosionExpire:expiry};
  const weapon={...dispenser,item:timed};
  const world=await f3World({soldier:{...definition,weapons:[weapon]}},{blastNotice:(def,event)=>blasts.push({def,owner:event.owner})});
  const unit=world.spawn('soldier',0,0,true,null),step=worldFrame(world,unit);
  unit.combat.arsenal.weapons[0].random=world.ordnance.random;
  unit.intent.buttons[0]=1;step();unit.intent.buttons[0]=0;
  const item=world.items.items[0];assert.ok(item);
  for(let i=0;i<30;i++)step();
  assert.equal(item.gone,true);assert.equal(world.items.items.length,0);
  assert.equal(blasts.length,1);assert.equal(blasts[0].def,expiry);assert.equal(blasts[0].owner,unit);
});

// The coordinator and command entries share retirement, but deliver their
// explosion notices at different boundaries. Observe both so a later expiry
// cannot hide behind an early harmless pop or a dispenser's eviction.
async function timedFixture(maxItems=2) {
  const blasts=[];
  const expiry={class:'timed-blast',Damage:1,DamageRadiusInner:.1,DamageRadiusOuter:.1};
  const pop={class:'timed-pop',Damage:0,DamageRadiusInner:.1,DamageRadiusOuter:.1};
  const timed={...mine,class:'timed-bomb',LifeSpan:5,ExplosionExpire:expiry,ExplosionDeath:pop};
  const world=await f3World({soldier:{...definition,weapons:[{...dispenser,item:timed,MaxItems:maxItems}]}},{
    blastNotice:(def,event)=>blasts.push({def,owner:event.owner}),
    itemOptions:{explode:(def,position,owner)=>blasts.push({def,owner})},
  });
  const unit=world.spawn('soldier',0,0,true,null),step=worldFrame(world,unit);
  unit.combat.arsenal.weapons[0].random=world.ordnance.random;
  return {world,unit,step,blasts,expiry,pop};
}

test('a timed bomb destroyed before its time pops harmlessly and never expires',async()=>{
  const {world,unit,step,blasts,pop}=await timedFixture();
  unit.intent.buttons[0]=1;step();unit.intent.buttons[0]=0;
  const bomb=world.items.items[0];assert.equal(bomb.health,40);
  world.items.hurt(bomb,41);
  assert.equal(bomb.gone,true);assert.deepEqual(blasts,[{def:pop,owner:unit}]);
  for(let i=0;i<360;i++)step();
  assert.deepEqual(blasts,[{def:pop,owner:unit}]);assert.deepEqual(world.items.items,[]);
  assert.deepEqual(world.items.ownedBy('layer',unit),[]);assert.equal(unit.health,2000);
});

test('items without an expiry explosion go away while an untimed land mine remains',async()=>{
  const blasts=[];
  const world=await f3World({soldier:{...definition,weapons:[]}},{
    blastNotice:(def,event)=>blasts.push({def,owner:event.owner}),
    itemOptions:{explode:(def,position,owner)=>blasts.push({def,owner})},
  });
  const step=worldFrame(world,world.spawn('soldier',80,80,true,null));
  const pickup=world.items.spawn({class:'timed-pickup',base:'powerupitem',LifeSpan:30},new Vector3(0,.2,0),new Vector3(),0);
  const landmine=world.items.spawn({...mine,class:'untimed-mine',LifeSpan:undefined,TriggerRadius:1},new Vector3(3,.2,0),new Vector3(),0);
  for(let i=0;i<31*60;i++)step();
  assert.equal(pickup.gone,true);assert.deepEqual(world.items.items,[landmine]);
  assert.deepEqual(blasts,[]);assert.equal(landmine.health,40);
});

test('a thrower over MaxItems loses only the oldest bomb as a pop',async()=>{
  const {world,unit,step,blasts,pop,expiry}=await timedFixture(1);
  unit.intent.buttons[0]=1;step();const first=world.items.items[0];
  unit.intent.buttons[0]=0;step();unit.intent.buttons[0]=1;step();unit.intent.buttons[0]=0;
  const second=world.items.items[0];assert.notEqual(second,first);
  assert.equal(first.gone,true);assert.deepEqual(world.items.items,[second]);
  assert.deepEqual(world.items.ownedBy('layer',unit),[second]);
  assert.deepEqual(blasts,[{def:pop,owner:unit}]);
  for(let i=0;i<330;i++)step();
  assert.equal(second.gone,true);assert.deepEqual(world.items.items,[]);
  assert.deepEqual(world.items.ownedBy('layer',unit),[]);
  assert.deepEqual(blasts,[{def:pop,owner:unit},{def:expiry,owner:unit}]);
});
