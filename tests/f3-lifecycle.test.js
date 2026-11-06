import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { f3World } from './fixtures/f3-world.js';
import { weaponDefinition } from './fixtures/client-weapons.js';
import { ClassicRandom, uniformRandom } from '../src/ai/random.js';

test('a whole droideka death publishes its body flag before the next movement tick', async() => {
  const world = await f3World({droid:{class:'droid',base:'walkerdroid',MaxHealth:20,weapons:[]}});
  const unit = world.spawn('droid',0,0);
  assert.equal(unit.soldier.dead,false);
  const mode = unit.soldier.mode;
  unit.combat.damage(25);
  assert.equal(unit.dead,true);
  assert.equal(unit.combat.dead,true);
  assert.equal(unit.soldier.dead,true);
  assert.equal(unit.soldier.mode,mode,'the death clip starts in the later body tick');
});

test('corpse respawn notifications run after the owning phase without an import', async() => {
  const world = await f3World({corpse:{class:'corpse',base:'soldier',MaxHealth:2000,weapons:[]}}, {
    prepareUnit(unit) {
      unit.alive = false; unit.dead = true; unit.removed = false;
      unit.soldier.dead = true;
    },
    afterAttach(unit) {
      unit.combat.dead = true; unit.combat.health = 0;
      unit.combat.deathTime = 4;
    },
  });
  const unit = world.factory.acquire('corpse');
  let notices = 0;
  unit.combat.onRespawn = () => { notices++; assert.equal(unit.combat.dead,true); };
  world.phase.counters.reset();
  world.phase.phase(1/60);
  assert.equal(notices,1);
  assert.equal(world.phase.counters.importCalls,0);
  assert.equal(world.phase.counters.commandCalls,0);
});

test('viewpoint weapon policy can be prepared before completed-body adoption', async() => {
  const draw = uniformRandom(new ClassicRandom(23));
  const world = await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:2000,
    weapons:[weaponDefinition('cold-gun',0)]}},{
    passiveClasses:['soldier'],
    afterAttach(unit) {
      assert.equal(unit.f3,undefined);
      const weapon = unit.combat.arsenal.weapons[0];
      weapon.random = draw; weapon.clip = 0.25;
    },
  });
  const unit = world.passive[0], weapon = unit.combat.arsenal.weapons[0];
  assert.equal(weapon.random,draw); assert.equal(weapon.clip,0.25);
  world.phase.phase(0);
  assert.equal(weapon.random,draw); assert.equal(weapon.clip,0.25);
  weapon.clip = 0.5; assert.equal(weapon.clip,0.5);
});

test('reconnect advances life without replacing weapons, while combat reset replaces them', async() => {
  const gun = weaponDefinition('reconnect-gun',0);
  const world = await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:2000,weapons:[gun]}});
  const unit = world.spawn('soldier',0,0,true), position = unit.position;
  const arsenal = unit.combat.arsenal, weapon = arsenal.weapons[0];
  const draw = uniformRandom(new ClassicRandom(9)); weapon.random = draw;
  weapon.clip = 2; const life = unit.generation;
  world.phase.advanceLife(unit);
  assert.equal(unit.generation,life + 1); assert.equal(unit.position,position);
  assert.equal(unit.combat.arsenal,arsenal); assert.equal(arsenal.weapons[0],weapon);
  assert.equal(weapon.random,draw); assert.equal(weapon.clip,2);
  weapon.clip = 1; world.phase.phase(0);
  assert.equal(weapon.clip,1,'the retained arsenal key follows the reconnect');
  unit.combat.reset();
  assert.equal(unit.generation,life + 1);
  assert.notEqual(unit.combat.arsenal,arsenal);
  assert.notEqual(unit.combat.arsenal.weapons[0],weapon);
  const current = unit.combat.arsenal.weapons[0], clip = current.clip;
  world.phase.counters.reset(); weapon.clip = 0;
  assert.equal(current.clip,clip); assert.equal(arsenal.select(weapon),false);
  assert.equal(world.phase.counters.commandCalls,0,'retired arsenals cannot mutate a replacement');
});

// The stream's identity can change while hasRandom stays true. Its setter
// must bind that one native weapon before firing, without a loadout poll.
test('a true-to-true weapon generator replacement takes effect before the next owned shot',async()=>{
  const gun=weaponDefinition('rebound-gun',0,{ShotDelay:.01,RoundsPerClip:999,HeatPerShot:0});
  const world=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:300,weapons:[gun]}});
  const unit=world.spawn('soldier',0,0),weapon=unit.combat.arsenal.weapons[0];
  const first=new ClassicRandom(17),second=new ClassicRandom(29);
  weapon.random=uniformRandom(first);unit.intent.buttons[0]=1;
  world.phase.phase(1/60);
  const kept=first.state;
  weapon.random=uniformRandom(second);
  const before=second.state;
  world.phase.counters.reset();world.phase.phase(1/60);
  assert.equal(first.state,kept);assert.notEqual(second.state,before);
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
});

// Unit.tick reaches the actual four-second removal branch. The complete corpse
// is construction state; the fixture never writes a live owned body back in.
test('owned corpse removal detaches its Character synchronously and returns one pooled body', async() => {
  const character = {unit:null};
  const removed = [];
  const world = await f3World({
    corpse:{class:'corpse',base:'soldier',MaxHealth:2000,weapons:[]},
  }, {prepareUnit:unit=>{
    unit.character = character;character.unit = unit;
    unit.alive = false;unit.dead = true;unit.removed = false;unit.deadTime = 4;
    unit.soldier.dead = true;
    unit.combat.state.dead = true;unit.combat.state.health = 0;
    unit.combat.state.deathTime = 4;
    unit.onRemoved = body=>removed.push(body);
  }});
  const unit = world.factory.acquire('corpse');
  const position = unit.position, velocity = unit.velocity;
  world.phase.bodiesIn();world.phase.counters.reset();
  world.phase.phase(1/60);
  assert.equal(unit.removed,true);
  assert.equal(character.unit,null);
  assert.equal(unit.character,null);
  assert.deepEqual(removed,[unit]);
  assert.equal(unit.actor.root.visible,false);
  assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.importCalls,1,'only the declared Character pointer leaf crosses');
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
  const reused = world.factory.acquire('corpse');
  assert.equal(reused,unit);
  assert.equal(reused.position,position);
  assert.equal(reused.velocity,velocity);
  assert.equal(world.phase.x.f3_acquire(world.phase.bridge.strings.id('corpse')),-1);
});

// An instant shot enters damage while the shooter is still inside Unit.tick.
// Character detachment returns a scalar pointer; drops and corpse mechanics
// remain native, and pure notices are delivered after that span has returned.
test('a lethal owned shot detaches its controller before a native side-local item drop', async() => {
  const { weaponDefinition } = await import('./fixtures/client-weapons.js');
  const bolt = {class:'fatal-bolt',base:'bolt',Velocity:60,LifeSpan:1,Damage:5000};
  const gun = weaponDefinition('rifle',0,{
    TriggerSingle:1,ShotDelay:1,SpreadThreshold:0,SpreadPerShot:0,HeatPerShot:0,
    YawSpread:0,PitchSpread:0,StandStillSpread:0,StandMoveSpread:0,ordnance:bolt,
  });
  const characters = {shooter:{unit:null},victim:{unit:null}};
  const calls = [];
  const medpack = {class:'medpack',base:'health',LifeSpan:30};
  let world;
  world = await f3World({
    shooter:{class:'shooter',base:'soldier',MaxHealth:2000,weapons:[gun]},
    victim:{class:'victim',base:'soldier',MaxHealth:2000,weapons:[],drops:[{class:'medpack',probability:1}]},
  }, {
    itemClasses:{medpack},
    prepareUnit:unit=>{ unit.character = characters[unit.className];unit.character.unit = unit; },
    deathCredit:(unit,killer,relation)=>{
      calls.push(['credit',unit.alive,unit.dead,killer,relation,unit.soldier.air]);
      unit.character.unit = null;
      world.phase.controller(unit,null);
    },
    deathNotice:(unit,event)=>calls.push(['death',unit,event.key.life,event.position.y]),
  });
  const shooter = world.spawn('shooter',0,0,true), victim = world.spawn('victim',0,1.5);
  world.phase.begin(world.phase.bindings[victim.id],'bodyAir');
  world.phase.change.field = 3;world.phase.commit();
  for (let tick=0;tick<=10;tick++) world.phase.history.capture(tick);
  shooter.networkShot = {button:0,id:41,viewTick:8,late:0,aimYaw:0,aimPitch:0};
  shooter.networkViewTick = 8;shooter.intent.buttons[0] = 1;
  world.phase.bodiesIn();world.phase.counters.reset();
  world.phase.phase(1/60);
  assert.equal(victim.alive,false);
  assert.equal(victim.dead,true);
  assert.equal(victim.controller,null);
  assert.equal(characters.victim.unit,null);
  assert.deepEqual(calls[0],['credit',false,true,characters.shooter,0,'fall']);
  assert.equal(calls[1][0],'death');assert.equal(calls[1][1],victim);
  assert.equal(calls[1][2],victim.generation);
  const dropped = world.items.items[0];
  assert.equal(dropped.def,medpack);
  assert.equal(dropped.position.y,calls[1][3]+1.048);
  assert.equal(dropped.owner,null);assert.equal(dropped.team,0);
  assert.equal(dropped.velocity.length(),3);
  assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.importCalls,2,'one aimer and the Character credit leaf');
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

test('a passive viewpoint shares canonical geometry without ticking or entering factory pools', {timeout:10000}, async() => {
  const world = await f3World({viewpoint:{class:'viewpoint',base:'soldier',MaxHealth:2000,weapons:[]}}, {
    passiveClasses:['viewpoint'],
  });
  const unit = world.passive[0], position = unit.position;
  unit.spawn(0,0,3,0);
  const stamp = unit.tickCount;
  let moved = 0;unit.onMoved = ()=>{ moved++; };
  unit.intent.forward = 1;
  world.phase.bodiesIn();world.phase.counters.reset();world.phase.phase(1/60);
  assert.equal(moved,0);assert.equal(unit.tickCount,stamp);
  assert.ok(unit.position === position);assert.ok(unit.soldier.position === position);
  assert.equal(world.factory.built.length,0);
  assert.equal(world.phase.counters.phaseCalls,1);
  assert.equal(world.phase.counters.importCalls,0);
  assert.equal(world.phase.layout.outputRows,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  const hit = world.units.raycast(new Vector3(0,1,1),new Vector3(0,0,1),4);
  assert.ok(hit.unit === unit);
  unit.combat.damage(25);assert.equal(unit.combat.health,1975);
  assert.equal(world.phase.layout.outputRows,1);
  unit.remove();assert.equal(unit.removed,true);
  const factoryBody = world.factory.acquire('viewpoint');
  assert.ok(factoryBody !== unit,'a passive body cannot be acquired from the class corpse pool');
});


test('viewpoint parking preserves its original non-corpse flags and metadata commands keep full hero health', async() => {
  const world = await f3World({viewpoint:{class:'viewpoint',base:'soldier',MaxHealth:1000000,weapons:[]}},
    {passiveClasses:['viewpoint']});
  const unit = world.passive[0];
  unit.spawn(2,0,3,0);
  world.phase.registerViewpoint(unit);
  const generation = unit.generation, ticks = unit.tickCount;
  unit.hero = true; unit.team = 2;
  world.phase.publishMetadata(unit);
  assert.equal(unit.hero,true); assert.equal(unit.team,2); assert.equal(unit.combat.team,2);
  assert.equal(unit.combat.health,1000000);
  world.phase.parkViewpoint(unit);
  assert.equal(unit.alive,false); assert.equal(unit.removed,true); assert.equal(unit.dead,false);
  assert.equal(unit.combat.dead,true); assert.equal(unit.deadTime,0);
  assert.equal(unit.generation,generation); assert.equal(unit.tickCount,ticks);
});


test('body-only placement and removed-body clocks keep their existing gameplay life', async() => {
  const world = await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:300,weapons:[]}});
  const unit = world.spawn('soldier',0,0,true), position = unit.position;
  unit.combat.damage(17);
  const life = unit.generation, stamp = unit.tickCount;
  world.phase.placeBody(unit,5,0,7,.5);
  assert.ok(unit.position === position); assert.deepEqual(unit.position.toArray(),[5,0,7]);
  assert.equal(unit.generation,life); assert.equal(unit.tickCount,stamp); assert.equal(unit.combat.health,283);
  world.phase.unspawn(unit); world.phase.deadTime(unit,1.25);
  assert.equal(unit.deadTime,1.25); assert.equal(unit.removed,true); assert.equal(unit.generation,life);
  unit.human = false; unit.networkHuman = false; unit.remove();
  assert.ok(world.factory.acquire('soldier') === unit);
});


test('explicit warm-body releases enter the canonical class stack in source order', async() => {
  const world = await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:300,weapons:[]}});
  const first = world.factory.build('soldier'), last = world.factory.build('soldier');
  world.factory.release(first); world.factory.release(last); world.factory.release(last);
  assert.equal(world.phase.bindings[first.id].view.pooled,true);
  assert.ok(world.factory.acquire('soldier') === last);
  assert.ok(world.factory.acquire('soldier') === first);
  assert.equal(world.factory.built.length,2);
  assert.equal(world.phase.bindings[first.id].view.pooled,false);
  assert.equal(first.generation,0); assert.equal(last.generation,0);
});

test('unread native publications retain cumulative deltas and an exact restoration',async()=>{
  const {putF3Change}=await import('../src/sim/gen/index.js');
  const w=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:300,weapons:[]}});
  const u=w.spawn('soldier',0,0),p=w.phase,b=p.bridge,binding=p.bindings[u.id];
  for(const value of [299,300]) {
    p.begin(binding,'health');p.change.value=value;
    b.views();putF3Change(b,p.change,b.f,p.layout.command/8,p.heap);
    assert.equal(p.x.f3_command(),1);
  }
  p.sync();assert.equal(u.combat.health,300);
  // Replacing a block for a different victim must retain the first victim's
  // unread words too, rather than making its intermediate native state vanish.
  const other=w.spawn('soldier',4,0);
  for(const unit of [u,other]) {
    p.begin(p.bindings[unit.id],'health');p.change.value=280;
    b.views();putF3Change(b,p.change,b.f,p.layout.command/8,p.heap);
    assert.equal(p.x.f3_command(),1);
  }
  p.sync();assert.equal(u.combat.health,280);assert.equal(other.combat.health,280);
});
