import test from 'node:test';
import assert from 'node:assert/strict';
import { Matrix4, Object3D, Vector3 } from 'three';
import { ObjectiveTarget } from '../src/objectives.js';
import { Trap } from '../src/traps.js';
import { f3World } from './fixtures/f3-world.js';
import { building } from './fixtures/f3-building.js';
import {PropsSim} from '../src/props-sim.js';
import { weaponDefinition } from './fixtures/client-weapons.js';

const soldier = {class:'soldier',base:'soldier',MaxHealth:2000,weapons:[]};

test('owned repair selection keeps ordered ties and imports only the touched prop', {timeout:10000}, async() => {
  const tool = weaponDefinition('repair',0,{
    base:'repair',BuildingHealth:1,LockOnRange:12,LockOnAngle:10,
    HeatPerShot:0,HeatThreshold:1,ShotDelay:1/60,
  });
  const a = building('a',-.2,2,{health:2000}), b = building('b',.2,2,{health:2000});
  a.health = 1990; b.health = 1990;
  const world = await f3World({soldier:{...soldier,weapons:[tool]}}, {
    prepareLive:(collision,units)=>{
      a.aiTarget = units.add(new ObjectiveTarget(a)); b.aiTarget = units.add(new ObjectiveTarget(b));
      return {buildings:[a,b]};
    },
  });
  assert.equal(world.phase.registry.foreignMembers.length,0,'owned targets need no foreign polling');
  const unit = world.spawn('soldier',0,0,true);
  unit.intent.buttons[0] = 1;
  world.phase.bodiesIn(); world.phase.counters.reset();
  world.phase.phase(1/60);
  assert.ok(unit.combat.repairTarget === a);
  assert.equal(unit.combat.repairLock,'l0');
  assert.equal(a.health,1991); assert.equal(b.health,1990);
  assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.importCalls,2,'one aimer and one touched health publication');
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
  // The current target wins an exact tie even if another one was constructed
  // first. A full target then stops qualifying before the next acquisition.
  for (let i=0;i<10;i++) world.phase.phase(1/60);
  assert.equal(a.health,2000); assert.equal(b.health,1991);
  assert.ok(unit.combat.repairTarget === b);
  assert.equal(unit.combat.repairLock,'l1');
});

test('native building death blasts complete depth first and retain their null owner', {timeout:10000}, async() => {
  const chain = {class:'chain',Damage:100,DamageRadiusInner:8,DamageRadiusOuter:8};
  const a = building('a',0,3,{health:20,explosion:chain}), b = building('b',0,6,{health:50});
  const order = [];
  a.onDestroyed = value=>order.push(['a',value.state,value.health]);
  b.onDestroyed = value=>order.push(['b',value.state,value.health]);
  const world = await f3World({soldier}, {
    prepareLive:()=>({buildings:[a,b]}),
    blastNotice:(definition,event)=>order.push(['blast',definition.class,event.team]),
  });
  const unit = world.spawn('soldier',0,6,true);
  world.phase.begin(world.phase.bindings[unit.id],'team');
  world.phase.change.value = 1; world.phase.commit();
  const damage = [];
  unit.combat.onDamaged = (amount,attacker)=>damage.push([amount,attacker]);
  world.phase.bodiesIn();world.phase.counters.reset();
  world.ordnance.explosion({class:'root',Damage:30,DamageRadiusInner:1,DamageRadiusOuter:1},new Vector3(0,1,3),unit);
  assert.equal(a.state,'destroyed'); assert.equal(b.state,'destroyed');
  assert.equal(unit.combat.health,1900);
  assert.deepEqual(damage,[[100,null]]);
  assert.deepEqual(order,[['a','built',0],['b','built',0],['blast','root',1],['blast','chain',0]]);
});

test('F1 raw body hits damage an unregistered building without the JavaScript hit adapter', {timeout:10000}, async() => {
  let b;
  const gun = weaponDefinition('rifle',0,{
    TriggerSingle:1,ShotDelay:1,SpreadThreshold:0,SpreadPerShot:0,HeatPerShot:0,
    YawSpread:0,PitchSpread:0,StandStillSpread:0,StandMoveSpread:0,
    ordnance:{class:'bolt',base:'bolt',Velocity:120,LifeSpan:1,Damage:25,BuildingScale:.5},
  });
  const world = await f3World({soldier:{...soldier,weapons:[gun]}}, {
    prepareLive:collision=>{
      b = building('wall',0,2,{health:100});
      const body = collision.addBody(new Float32Array([-2,0,2,2,0,2,0,3,2]),
        new Uint32Array([0,1,2]),new Uint32Array([0]),b);
      b.bodies.built = body;
      return {buildings:[b]};
    },
  });
  world.ordnance.onHit = ()=>{ throw Error('legacy raw hit callback reached'); };
  const unit = world.spawn('soldier',0,0,true);
  for (let tick=0;tick<=10;tick++) world.phase.history.capture(tick);
  unit.networkShot = {button:0,id:7,viewTick:8,late:0,aimYaw:0,aimPitch:0};
  unit.networkViewTick = 8;unit.intent.buttons[0] = 1;
  world.phase.bodiesIn();world.phase.counters.reset();
  world.phase.phase(1/60);
  assert.equal(b.health,87.5);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

test('a built trap reaches zero health by springing and a repair starts its unticked reset', {timeout:10000}, async() => {
  const trap = new Trap({class:'trap',base:'trap',settings:{MaxHealth:5},matrix:new Matrix4().toArray()},
    {built:{root:new Object3D(),bones:[],byName:{}},pad:null},{},null);
  trap.health = 4;
  const tool = weaponDefinition('repair',0,{
    base:'repair',BuildingBuild:1,BuildingHealth:1,LockOnRange:12,LockOnAngle:45,
    HeatPerShot:0,HeatThreshold:1,ShotDelay:1/60,
  });
  const world = await f3World({soldier:{...soldier,weapons:[tool]}}, {
    prepareLive:()=>({buildings:[trap]}),
  });
  const unit = world.spawn('soldier',0,-1,true);
  unit.intent.buttons[0] = 1;
  world.phase.bodiesIn();world.phase.counters.reset();world.phase.phase(1/60);
  assert.equal(trap.state,'built');assert.equal(trap.health,5);
  assert.equal(trap.phase,'reset');assert.equal(trap.started,null);assert.equal(trap.time,0);
  assert.equal(trap.sprung,false);assert.equal(trap.rig.root.visible,true);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
  world.ordnance.explosion({class:'trap-hit',Damage:10,DamageRadiusInner:2,DamageRadiusOuter:2},new Vector3());
  assert.equal(trap.state,'built');assert.equal(trap.health,0);
  assert.equal(trap.phase,'reset');assert.equal(trap.started,null);assert.equal(trap.sprung,true);
});

// Real foreign lifecycle writes end the reused read span before the next
// native damage service consumes it. Nested spans must also leave no lease.
test('AI sensor spans retain facts until a foreign prop changes',async()=>{
  const prop=building('station',0,2,{health:1000});prop.team=2;
  const live={buildings:[prop],animated:[]};
  const w=await f3World({soldier},{prepareLive:()=>live});
  new PropsSim(w.phase.bridge,live);
  const unit=w.spawn('soldier',0,0,true),registry=w.phase.registry;
  w.units.setRelation(1,2,-1);
  w.phase.begin(w.phase.bindings[unit.id],'team');
  w.phase.change.value=1;w.phase.commit();
  w.phase.bodiesIn();
  const out=[];
  registry.beginReads();
  try {
    w.units.query(unit.position,10,out);
    assert.equal(registry.readReady,true);
    w.units.query(unit.position,10,out);
    assert.equal(registry.readReady,true);
    prop.damage(100);
    assert.equal(registry.readReady,false);
    // Use the model centre so the fixture's floor cannot occlude the blast.
    w.ordnance.explosion({class:'blast',Damage:20,DamageRadiusInner:10,DamageRadiusOuter:10},new Vector3(0,1,2),unit);
    assert.equal(prop.health,880);
    registry.beginReads();registry.endReads();
    assert.equal(registry.readDepth,1);
  } finally { registry.endReads(); }
  assert.equal(registry.readDepth,0);
});
