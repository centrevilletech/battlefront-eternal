import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { f3World } from './fixtures/f3-world.js';
import { weaponDefinition } from './fixtures/client-weapons.js';

const emitter = {
  class:'arc-emitter',base:'emitterordnance',ConeLength:12,ConeAngle:120,
  Damage:120,MaxDamage:120,DamageDropPerTarget:20,MaxTargets:3,
  MaxJumpDistance:4,DamageThreshold:1,FirstRadius:0,
};
const gun = weaponDefinition('arc-caster',0,{
  TriggerSingle:1,ShotDelay:1,SpreadThreshold:0,SpreadPerShot:0,HeatPerShot:0,
  YawSpread:0,PitchSpread:0,StandStillSpread:0,StandMoveSpread:0,ordnance:emitter,
});
const classes = {
  caster:{class:'caster',base:'soldier',MaxHealth:2000,weapons:[gun]},
  target:{class:'target',base:'soldier',MaxHealth:2000,weapons:[]},
};

test('the owned arc phase keeps along ties stable and damages acquired soldiers without imports', async() => {
  const world = await f3World(classes);
  const caster = world.spawn('caster',0,0);
  const first = world.spawn('target',-0.3,4), second = world.spawn('target',0.3,4);
  const last = world.spawn('target',0,6);
  caster.combat.arsenal.weapons[0].random = world.ordnance.random;
  const hits = [];
  first.combat.onDamaged = (amount,owner,info)=>hits.push([first,amount,owner,info.known,info.point.toArray(),info.source.toArray()]);
  second.combat.onDamaged = amount=>hits.push([second,amount]);
  last.combat.onDamaged = amount=>hits.push([last,amount]);
  caster.intent.buttons[0] = 1;world.phase.bodiesIn();world.phase.counters.reset();world.phase.phase(1/60);
  assert.deepEqual(hits.map(hit=>[hit[0].id,hit[1]]),[[first.id,120],[second.id,100],[last.id,80]]);
  assert.equal(hits[0][2],caster);assert.equal(hits[0][3],true);
  assert.deepEqual(hits[0][4],caster.position.toArray());
  assert.deepEqual(hits[0][5],caster.combat.origin.toArray());
  assert.equal(first.combat.health,1880);assert.equal(second.combat.health,1900);assert.equal(last.combat.health,1920);
  assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.importCalls,4,'only the four post-movement aimers cross');
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

test('a native arc mine link completes its blast before a later acquired soldier link', async() => {
  const events = [];
  const world = await f3World({
    caster:classes.caster,
    target:{...classes.target,MaxHealth:30},
  }, {deathCredit:unit=>events.push(['credit',unit.id])});
  const caster = world.spawn('caster',0,0), target = world.spawn('target',0,4);
  caster.combat.arsenal.weapons[0].random = world.ordnance.random;
  const mine = world.items.spawn({
    class:'arc-mine',base:'mine',MaxHealth:5,LifeSpan:30,
    ExplosionDeath:{class:'arc-mine-pop',Damage:50,DamageRadiusInner:8,DamageRadiusOuter:8},
  },new Vector3(0,0.1,2),new Vector3(),0);
  target.combat.onDamaged = amount=>events.push(['damage',amount]);
  world.ordnance.onExplosion = ()=>events.push(['finished']);
  caster.intent.buttons[0] = 1;world.phase.bodiesIn();world.phase.counters.reset();world.phase.phase(1/60);
  assert.equal(mine.gone,true);assert.equal(target.dead,true);
  assert.equal(caster.combat.health,1950);
  assert.deepEqual(events,[['credit',target.id],['damage',50],['finished']]);
  assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.importCalls,2,'the dead target has no later aimer');
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

// The visual effect's presence changes gameplay RNG: the first link advances
// its source without a roll, and each later drawn link consumes exactly one.
test('owned arc links preserve the effect-dependent random draws', async() => {
  const { ClassicRandom } = await import('../src/ai/random.js');
  for (const visible of [false,true]) {
    const definition = {...emitter,LightningEffect:'fixture-lightning'};
    const weapon = {...gun,ordnance:definition};
    const world = await f3World({
      caster:{...classes.caster,weapons:[weapon]},target:classes.target,
    }, {prepareUnit:unit=>{
      unit.combat.effects = {lightning:visible ? {'fixture-lightning':{}} : {}};
    }});
    const caster = world.spawn('caster',0,0);
    world.spawn('target',0,4);world.spawn('target',0,6);world.spawn('target',0,8);
    caster.combat.random = world.ordnance.random;
    caster.combat.arsenal.weapons[0].random = world.ordnance.random;
    const binding = world.phase.random.entries[world.phase.random.globals.gameplay].binding;
    const expected = new ClassicRandom(binding.streams.initialState(binding.name));
    // Four spread draws precede the two optional arc-link draws.
    for (let draw=0;draw<(visible ? 6 : 4);draw++) expected.uniform();
    const bolts = [];
    caster.combat.onPresentation = event=>{ if (event.kind==='lightning') bolts.push(event.channel); };
    caster.intent.buttons[0] = 1;world.phase.bodiesIn();world.phase.counters.reset();world.phase.phase(1/60);
    assert.deepEqual(bolts,visible ? [0,undefined,undefined] : []);
    assert.equal(binding.streams.get(binding.name).state,expected.state);
    assert.equal(world.phase.counters.wasmCalls,1);
    assert.equal(world.phase.counters.importCalls,4);
    assert.equal(world.phase.counters.commandCalls,0);
    assert.equal(world.phase.counters.rustAllocations,0);
    assert.equal(world.phase.counters.rustReallocations,0);
  }
});
