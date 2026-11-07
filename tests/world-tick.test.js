import { FixedClock } from '../src/runtime/fixed-clock.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import { CommandPosts } from '../src/commandposts.js';
import { F4Live } from '../src/f4-live.js';
import { F4Tail } from '../src/f4-tail.js';
import { HumanSpawnMirror } from '../src/sim/gen/index.js';
import { IMPORT_TABLE } from '../src/sim/gen/index.js';
import { WaterState } from '../src/runtime/water-state.js';
import { WorldTick } from '../src/runtime/world-tick.js';
import { WorldTickInputMirror, WorldTickInputLayout, putWorldTickInput,
  WorldTickOutputMirror, syncWorldTickOutput } from '../src/sim/gen/world.js';
import { f3World } from './fixtures/f3-world.js';
import { battleFixture } from './fixtures/f4-battle.js';

// All phases use their real canonical stores over synthetic content. A host
// phase callback would throw, as would a separate JS call to a phase entry.
async function fixture(water = null) {
  const posts = new CommandPosts(null, {}, {headless:true, playerTeam:1});
  const battle = battleFixture(posts), character = battle.characters[0];
  const world = await f3World({soldier:{class:'soldier', base:'soldier', MaxHealth:100, weapons:[]}}, {
    water, debug:false, prepareLive:()=>({buildings:[], animated:[], traps:[]}),
    prepareUnit:unit=>{unit.character=character;unit.team=unit.combat.team=1;},
  });
  const unit = world.spawn('soldier', 0, 0, true, null);
  character.unit=unit;character.className='soldier';
  world.phase.registerViewpoint(unit);
  new F4Live(world.phase, {local:unit, sound(){}});
  const human = new HumanSpawnMirror();human.phase='alive';human.className='soldier';human.spawnedOnce=true;
  new F4Tail(world.phase, battle, {posts, human, local:unit, character,
    gameplay:world.ordnance.random, rosters:[[],['soldier'],['soldier']], classes:['soldier']});
  const x = world.phase.x;
  world.phase.x = {...x};
  for(const name of ['ai_world_phase','f3_step','f4_live','ord_update','items_update','f4_tail','ai_world_dangers']) {
    world.phase.x[name]=()=>{throw Error(`host dispatched ${name}`);};
  }
  const tick = new WorldTick(world.phase,new FixedClock());
  return {world, unit, battle, human, tick, x};
}

test('one world entry retains body publication through empty later phases', async()=>{
  const f=await fixture();
  assert.equal(IMPORT_TABLE.some(row=>row?.name==='worldStep'),false);
  let calls=0;
  f.world.phase.x.world_step_run=()=>{calls++;return f.x.world_step_run();};
  const initialTicks=f.unit.tickCount;
  f.unit.intent.forward=1;
  for(let i=1;i<=120;i++){
    f.tick.run(1/60,i,true,null,false,true,0,false,0);f.tick.finish();f.tick.complete();
    if(i>1){assert.equal(f.tick.output.allocations,0);assert.equal(f.tick.output.reallocations,0);}
  }
  assert.equal(calls,120);
  assert.equal(f.unit.tickCount-initialTicks,240);
  assert.ok(f.unit.soldier.position.z>0);
  assert.ok(Math.abs(f.battle.stats.time-2)<1e-12);
  assert.equal(f.tick.output.accepted,true);
});

test('world input clamps a paused human command burst and preserves native death credit', async()=>{
  const f=await fixture();
  const commands=Array.from({length:4096},()=>({kind:'suicide'}));
  f.tick.run(1/60,1,false,commands,false,true,0,false,0);f.tick.finish();f.tick.complete();
  assert.equal(f.unit.combat.dead,true);
  assert.equal(f.battle.characters[0].deaths,1);
  assert.equal(f.battle.reinforcements[1],99);
  assert.equal(f.human.phase,'dying');
  for(let i=2;i<240;i++){f.tick.run(1/60,i,false,null,false,true,0,false,0);f.tick.finish();f.tick.complete();}
  assert.equal(f.battle.characters[0].deaths,1);
});

test('invalid world time returns a bounded result before any phase runs',async()=>{
  const f=await fixture(), b=f.world.phase.bridge, input=new WorldTickInputMirror();
  const initialTicks=f.unit.tickCount;
  input.dt=NaN;input.tick=1;
  b.views();putWorldTickInput(b,input,b.f,f.tick.address,b.heap);
  const result=b.callResult({Mirror:WorldTickOutputMirror,sync:syncWorldTickOutput},f.x.world_step_run());
  assert.equal(result.accepted,false);
  assert.equal(f.unit.tickCount,initialTicks);
  assert.equal(f.battle.stats.time,0);
});


test('the enclosing tick advances gameplay water before the body and item phases',async()=>{
  const water=new WaterState({height:1,cell:10,offset:[0,0],grid:2,cells:[[0,0]],look:{OscillationEnable:[1]}});
  const f=await fixture(water);
  f.tick.run(1/60,1,true,null,false,true,0,false,0);f.tick.finish();f.tick.complete();
  assert.equal(water.simulationTime,1/60);
  assert.ok(water.physicsWave>0.0018&&water.physicsWave<0.0019);
  assert.equal(water.level,f.tick.output.level);
  for(let i=2;i<=120;i++){f.tick.run(1/60,i,true,null,false,true,0,false,0);f.tick.finish();f.tick.complete();}
  assert.ok(Math.abs(water.simulationTime-2)<1e-12);
  assert.ok(water.physicsWave<0);
});
