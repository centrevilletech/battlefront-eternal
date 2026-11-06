import test from 'node:test';
import assert from 'node:assert/strict';
import { F4Live } from '../src/f4-live.js';
import { F4Battle } from '../src/f4-battle.js';
import { f3World } from './fixtures/f3-world.js';
import { battleFixture } from './fixtures/f4-battle.js';
const soldier = {class:'soldier',base:'soldier',MaxHealth:100,weapons:[]};

test('native battle commands retain scoreboard objects and publish only touched records', async()=>{
  const world = await f3World({soldier},{prepareLive:()=>({buildings:[],animated:[],traps:[]})});
  const live = new F4Live(world.phase,{sound(){}});
  const battle = battleFixture({posts:[]}), [a,b] = battle.characters;
  const owned = new F4Battle(world.phase,battle,{debug:true});
  const words = live.native.outputWords, mirrored = owned.mirrors.words, commands = live.native.commands;
  const phaseWords = world.phase.counters.outputFields;
  battle.credit(b,a,-1);
  assert.equal(live.native.outputWords - words,owned.mirrors.words - mirrored);
  assert.equal(live.native.commands - commands,1);
  assert.equal(owned.counters.outputFields,owned.mirrors.words - mirrored + world.phase.counters.outputFields - phaseWords);
  assert.equal(battle.characters[0],a); assert.equal(battle.characters[1],b);
  assert.equal(a.kills,1); assert.equal(b.deaths,1);
  assert.equal(a.record.slots[0].victims.get(b),1);
  const record = a.record, slot = record.slots[0], victims = slot.victims;
  battle.headshot(a); battle.vehicleDestroyed(a); battle.point(a);
  assert.equal(a.record,record); assert.equal(a.record.slots[0],slot); assert.equal(slot.victims,victims);
  assert.equal(slot.headshots,1); assert.equal(slot.vehiclesDestroyed,1); assert.equal(a.captures,1);
  battle.spend(1,2); assert.equal(battle.reinforcements[1],98);
  assert.equal(owned.counters.importCalls,0);
  const h = owned.entries.get(a).handle; h.life++;
  assert.equal(battle.point(a),false); assert.equal(a.captures,1);
});

test('headshot credit mutates the canonical record before the unit phase returns', async()=>{
  const { Vector3 } = await import('three');
  const battle = battleFixture({posts:[]}), [a,b] = battle.characters;
  const world = await f3World({shooter:soldier,victim:soldier},{
    prepareLive:()=>({buildings:[],animated:[],traps:[]}),
    prepareUnit:unit=>{unit.character = unit.className === 'shooter' ? a : b;},
    headshotCredit:()=>{throw Error('foreign Character head credit');},
  });
  const shooter = world.spawn('shooter',0,0), victim = world.spawn('victim',0,2);
  new F4Live(world.phase,{sound(){}}); new F4Battle(world.phase,battle);
  victim.combat.damage(1,1,null,shooter);
  const definition = {class:'head-bolt',base:'bolt',Velocity:120,LifeSpan:1,Damage:10};
  world.ordnance.fire(definition,new Vector3(0,1.7,0),new Vector3(0,0,1),shooter);
  world.ordnance.update(1/60); world.ordnance.update(1/60);
  assert.equal(a.record.slots[0].headshots,1);
  assert.equal(a.kills,0); assert.equal(b.deaths,0);
});

test('unit observer barriers retain idle battle epochs and publish the next score command', async()=>{
  const world = await f3World({soldier},{prepareLive:()=>({buildings:[],animated:[],traps:[]})});
  world.spawn('soldier',0,0);
  const live = new F4Live(world.phase,{sound(){}}), battle = battleFixture({posts:[]});
  new F4Battle(world.phase,battle);
  world.phase.sync();
  const version = live.layout.version;
  // Movement callbacks still see canonical bodies, but have no live-object or
  // scoreboard mutation to invalidate. A later command must remain immediate.
  world.phase.counters.reset();
  for (let tick=0;tick<120;tick++) world.phase.phase(1/60);
  assert.ok(world.phase.counters.importCalls>0);
  live.readLayout();
  assert.equal(live.layout.version,version);
  assert.equal(battle.point(battle.characters[0]),true);
  assert.equal(battle.characters[0].captures,1);
  assert.notEqual(live.layout.version,version);
});
