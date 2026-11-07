import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { ObjectiveTarget } from '../src/objectives.js';
import { f3World } from './fixtures/f3-world.js';
import { playersWorld } from './fixtures/players-world.js';
import { building } from './fixtures/f3-building.js';

test('bounds observations defer limb publication and preserve stale death shapes',async()=>{
  const world=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:300,weapons:[]}});
  const unit=world.spawn('soldier',4,7),bounds=unit.bounds;
  world.phase.phase(1/60);
  world.phase.counters.reset();
  assert.equal(unit.sphere,bounds);assert.equal(bounds.center.x,3.955);assert.ok(bounds.radius>0);
  assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(unit.sphere,bounds);assert.equal(world.phase.counters.wasmCalls,1);
  const shapes=unit.hitShapes();assert.ok(shapes.length>0);
  assert.equal(shapes[0].center.x,4);assert.equal(shapes[0].center.z,7);
  assert.equal(world.phase.counters.wasmCalls,2);
  const tick=unit.tickCount;
  unit.combat.damage(1000);
  assert.equal(unit.tickCount,tick);
  world.phase.counters.reset();
  assert.equal(unit.hitShapes(),shapes);
  assert.equal(world.phase.counters.wasmCalls,0,'death alone retains the observed pose');
});

test('ordinary sight rays send endpoint exclusions without a registry mask',async()=>{
  const world=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:300,weapons:[]}});
  const first=world.spawn('soldier',0,4),second=world.spawn('soldier',0,4),third=world.spawn('soldier',0,7);
  const origin=new Vector3(0,1,0),direction=new Vector3(0,0,1);
  world.phase.counters.reset();
  assert.equal(world.units.raycast(origin,direction,10).unit,first);
  const plainWords=world.phase.counters.inputFields;
  world.phase.counters.reset();
  assert.equal(world.units.raycast(origin,direction,10,null,undefined,()=>true).unit,first);
  assert.equal(world.phase.counters.inputFields-plainWords,world.units.nextId);
  assert.equal(world.units.raycast(origin,direction,10,first).unit,second);
  assert.equal(world.units.raycast(origin,direction,10,[first,second]).unit,third);
  assert.equal(world.units.raycast(origin,direction,10,[first,second,third]),null);
  // A custom predicate may ask another sensor while its mask is staged.
  const near=[];
  const hit=world.units.raycast(origin,direction,10,null,undefined,u=>{
    world.units.query(origin,20,near);return u!==first && u!==second;
  });
  assert.equal(hit.unit,third);
});

// Complete native capture is also exercised by the instant-shot, headshot,
// chained-blast and recon fixtures. None imports JS body shapes into history.
test('owned history captures pre-existing registry members and survives snapshot-host release', {timeout:10000}, async() => {
  const prop = building('objective',0,4);
  const f = await playersWorld({ unitOptions: {
    prepareLive:(collision,units)=>{
      prop.aiTarget = units.add(new ObjectiveTarget(prop));
      return {buildings:[prop]};
    },
  } });
  const history = f.phase.history, lease = history.lease;
  assert.equal(lease.source,history);
  await f.world.addPlayer('body',{team:1});
  const unit = f.world.players.get('body').unit;
  f.game.unitsSim.placeBody(unit,0,0,0,0);
  for (let tick=0;tick<=60;tick++) {
    f.world.step(1000);
    assert.equal(f.tick.output.allocations,0);
    assert.equal(f.tick.output.reallocations,0);
  }
  lease.dispose();f.world.step(1000);
  assert.equal(f.game.worldClock.tick,62);
  assert.equal(f.units.raycast(new Vector3(0,1,-2),new Vector3(0,0,1),4).unit,unit);
});
