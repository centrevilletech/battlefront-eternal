import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { Character } from '../src/battle.js';
import { f3World } from './fixtures/f3-world.js';
import { weaponDefinition } from './fixtures/client-weapons.js';
import { ClassicRandom, uniformRandom } from '../src/ai/random.js';

const classes = {
  soldier:{class:'soldier',base:'soldier',MaxHealth:300,weapons:[]},
  hero:{class:'hero',base:'soldier',MaxHealth:1000000,weapons:[weaponDefinition('hero-gun',0)]},
};

test('cold stock is absent from the registry until birth and keeps pooled aliases',async()=>{
  const w = await f3World(classes);
  assert.equal(w.units.list.length,0);
  const unit = w.factory.acquire('soldier');
  assert.equal(w.units.list.length,1);
  assert.equal(w.factory.built[0],unit);
  assert.equal(w.units.byId(unit.id),unit);
  const position = unit.position, velocity = unit.soldier.velocity;
  unit.spawn(1,0,2,0);unit.remove();
  const again = w.factory.acquire('soldier');
  assert.equal(again,unit);assert.equal(again.position,position);
  assert.equal(again.itemTarget.position,position);assert.equal(again.itemTarget.velocity,velocity);
  assert.equal(w.units.list.length,1);
  assert.equal(w.phase.bindings[unit.id].view.key.generation,1);
});

test('native connection realization rebinds spread and preserves full hero health',async()=>{
  const w = await f3World(classes), character = new Character(0,1,true);
  const draw = uniformRandom(new ClassicRandom(31));
  w.phase.construction.bindPlayer(character,null,{weaponRandom:draw});
  const first = w.phase.construction.spawnPlayer(character,null,'soldier',new Vector3(),0);
  assert.equal(first.human,true);assert.equal(first.networkHuman,true);
  const hero = w.phase.construction.spawnPlayer(character,null,'hero',new Vector3(2,0,3),0);
  assert.notEqual(hero,first);assert.equal(first.removed,true);
  assert.equal(hero.combat.health,1000000);
  assert.equal(hero.combat.arsenal.weapons[0].random,draw);
  w.phase.phase(0);
  assert.equal(hero.combat.arsenal.weapons[0].random,draw);
  assert.equal(w.units.list[0],first);assert.equal(w.units.list[1],hero);
});

test('native local class realization retains the viewpoint Unit identity',async()=>{
  const w = await f3World(classes,{passiveClasses:['soldier']});
  const unit = w.passive[0], character = new Character(0,1,true);
  const generation = unit.generation;
  w.phase.construction.bindPlayer(character,unit,{local:true});
  const realized = w.phase.construction.spawnPlayer(character,unit,'hero',new Vector3(3,0,4),0,true);
  assert.equal(realized,unit);assert.equal(unit.className,'hero');
  assert.equal(unit.generation,generation+1);assert.equal(unit.combat.health,1000000);
  assert.equal(w.units.list.length,1);assert.equal(w.factory.built.length,0);
  assert.equal(unit.itemTarget.position,unit.position);
  const heroPosition = unit.position;
  for (let i = 0; i < 8; i++) {
    const name = i % 2 ? 'hero' : 'soldier';
    w.phase.construction.spawnPlayer(character,unit,name,new Vector3(),0,true);
    assert.equal(w.units.list.length,1);
    if (name === 'hero') assert.equal(unit.position,heroPosition,'cached class retains its vectors');
  }
});

// Registry admission reserves numeric ids independently of rewind entries.
// All cold stock, including preparation after a first history capture, must
// already own its frame/shape storage when the native factory activates it.
test('prepared births and late stock capture history without native allocation',async()=>{
  const w = await f3World(classes,{debug:false});
  const first = w.spawn('soldier',0,0);
  w.phase.phase(1/60);w.phase.history.capture(0);
  w.phase.construction.prepare('hero',4);
  const bodies = [first];
  for (let i=0;i<7;i++) {
    const unit=w.factory.acquire('hero');unit.spawn(i*3,0,0,0);bodies.push(unit);
  }
  for (let tick=1;tick<=80;tick++) {
    w.phase.counters.reset();w.phase.phase(1/60);w.phase.history.capture(tick);
    assert.equal(w.phase.counters.rustAllocations,0);
    assert.equal(w.phase.counters.rustReallocations,0);
    assert.equal(w.phase.history.tick,tick);
  }
  assert.equal(bodies.length,8);assert.equal(w.units.list.length,8);
});
