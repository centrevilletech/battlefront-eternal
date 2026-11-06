import test from 'node:test';
import assert from 'node:assert/strict';
import { f3World } from './fixtures/f3-world.js';
import { building, buildingBody } from './fixtures/f3-building.js';
import { ObjectiveTarget } from '../src/objectives.js';
import { ClassicRandom, uniformRandom } from '../src/ai/random.js';

test('owned hero phase acquires and damages units natively and preserves explicit deflection', async(t) => {
  const saber = {
    class:'blade', base:'melee', channel:0, AnimationBank:'saber',
    swings:[{ duration:.2, hit:.05 }],
  };
  const defenderBlade = { ...saber, class:'defender', CanDeflect:1 };
  const world = await f3World({
    hero:{ class:'hero', base:'soldier', MaxHealth:2000, weapons:[saber] },
    victim:{ class:'victim', base:'soldier', MaxHealth:2000, weapons:[] },
    defender:{ class:'defender', base:'soldier', MaxHealth:2000, weapons:[defenderBlade] },
  });
  const hero = world.spawn('hero', 0, 0);
  const victim = world.spawn('victim', -.5, 1);
  const defender = world.spawn('defender', .5, 1);
  hero.combat.arsenal.weapons[0].random = uniformRandom(new ClassicRandom(7));
  const notices = [];
  hero.combat.onFired = (shot, event) => notices.push(['shot', shot.swing, event.key.life]);
  hero.combat.onPresentation = event => notices.push(['action', event.kind, event.action]);
  victim.combat.onDamaged = (amount, attacker) => notices.push(['damage', amount, attacker === hero]);
  defender.combat.onPresentation = event => notices.push(['block', event.effect]);
  hero.intent.buttons[0] = 1;
  world.phase.bodiesIn();
  world.phase.counters.reset();
  for (let tick = 0; tick < 16; tick++) world.phase.phase(1 / 60);
  assert.equal(victim.combat.health, 1000);
  assert.equal(defender.combat.health, 2000);
  assert.deepEqual(notices, [
    ['shot', 1, 1], ['action', 'weaponAction', 'shoot'],
    ['block', 'meleeDeflect'], ['damage', 1000, true],
    ['action', 'endWeaponAction', null],
  ]);
  const measured = world.phase.counters.snapshot();
  assert.equal(measured.phaseCalls, 16);
  assert.equal(measured.commandCalls, 0);
  assert.equal(measured.importCalls, 48, 'only the three post-movement aimers cross during each phase');
  assert.equal(measured.rustAllocations, 0);
  assert.equal(measured.rustReallocations, 0);
  t.diagnostic(`hero phase: ${measured.wasmCalls / 16} exports and ${measured.importCalls / 16} aimer imports per tick`);
});

test('million-health heroes keep full precision through damage, regeneration and pooled respawn', async() => {
  const world = await f3World({
    hero:{class:'hero',base:'soldier',MaxHealth:1000000,AddHealth:15,weapons:[]},
  }, {prepareUnit:unit=>{ unit.hero = true; }});
  const hero = world.spawn('hero',0,0);
  const position = hero.position, velocity = hero.velocity;
  assert.equal(hero.combat.health,1000000);
  assert.equal(hero.combat.maxHealth,1000000);
  hero.combat.damage(12.5);
  assert.equal(hero.combat.health,999987.5);
  world.phase.counters.reset();world.phase.phase(1/60);
  assert.equal(hero.combat.health,999987.75);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
  hero.combat.damage(Infinity);
  assert.equal(hero.dead,true);
  hero.spawn(3,0,4,0);
  assert.equal(hero.generation,2);
  assert.equal(hero.combat.health,1000000);
  assert.equal(hero.combat.maxHealth,1000000);
  assert.ok(hero.position === position);assert.ok(hero.velocity === velocity);
});

test('hero melee combines registry and extra buildings in reverse acquisition order', {timeout:10000}, async() => {
  const a = building('registered',-.2,1), b = building('extra',.2,1);
  const deaths = [];
  a.onDestroyed = ()=>deaths.push('registered'); b.onDestroyed = ()=>deaths.push('extra');
  const saber = {class:'saber',base:'melee',channel:0,swings:[{duration:.3,hit:.05}]};
  const world = await f3World({hero:{class:'hero',base:'soldier',MaxHealth:1000000,weapons:[saber]}}, {
    prepareLive:(collision,units)=>{
      buildingBody(collision,a); buildingBody(collision,b);
      a.aiTarget = units.add(new ObjectiveTarget(a));
      return {buildings:[a,b]};
    },
  });
  const hero = world.spawn('hero',0,0,true);
  hero.combat.arsenal.weapons[0].random = uniformRandom(new ClassicRandom(7));
  world.ordnance.onHit = ()=>{ throw Error('JavaScript body hit adapter reached'); };
  hero.intent.buttons[0] = 1;
  world.phase.bodiesIn();world.phase.counters.reset();
  for (let i=0;i<8;i++) world.phase.phase(1/60);
  assert.deepEqual(deaths,['extra','registered']);
  assert.equal(a.health,0);assert.equal(b.health,0);
  assert.equal(world.phase.counters.phaseCalls,8);
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.importCalls,14,'eight aimers and three lifecycle leaves for each prop');
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

test('hero melee retains the first ten extra buildings and rays each body independently', {timeout:10000}, async() => {
  const objects = [], deaths = [];
  for (let i=0;i<12;i++) {
    const object = building(`building-${i}`,0,1);
    object.onDestroyed = ()=>deaths.push(i); objects.push(object);
  }
  const saber = {class:'saber',base:'melee',channel:0,swings:[{duration:.3,hit:.05}]};
  const world = await f3World({hero:{class:'hero',base:'soldier',MaxHealth:1000000,weapons:[saber]}}, {
    prepareLive:collision=>{
      for (const object of objects) buildingBody(collision,object);
      return {buildings:objects};
    },
  });
  const hero = world.spawn('hero',0,0,true);
  hero.combat.arsenal.weapons[0].random = uniformRandom(new ClassicRandom(7));
  hero.intent.buttons[0] = 1;
  world.phase.bodiesIn();world.phase.counters.reset();
  for (let i=0;i<8;i++) world.phase.phase(1/60);
  assert.deepEqual(deaths,[9,8,7,6,5,4,3,2,1,0]);
  assert.equal(objects[10].health,100);assert.equal(objects[11].health,100);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});
