import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { ObjectiveTarget } from '../src/objectives.js';
import { f3World } from './fixtures/f3-world.js';
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
  const world = await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:2000,weapons:[]}}, {
    prepareLive:(collision,units)=>{
      prop.aiTarget = units.add(new ObjectiveTarget(prop));
      return {buildings:[prop]};
    },
  });
  const history = world.phase.history;
  const lease = history.lease;
  assert.ok(lease.source === history);
  world.phase.counters.reset();
  history.capture(0);
  assert.equal(history.tick,0);
  assert.equal(world.phase.counters.captureCalls,1);
  assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.importCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
  // Unit construction reserves all later frames too. The first shape read in
  // its first native capture must not grow a ring or create shape storage.
  const unit = world.spawn('soldier',0,0,true);
  world.phase.counters.reset();
  for (let tick=1;tick<=60;tick++) history.capture(tick);
  assert.equal(world.phase.counters.captureCalls,60);
  assert.equal(world.phase.counters.wasmCalls,60);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
  lease.dispose();history.capture(61);
  assert.equal(history.tick,61);
  assert.ok(world.units.raycast(new Vector3(0,1,-2),new Vector3(0,0,1),4).unit === unit);
});
