import test from 'node:test';
import assert from 'node:assert/strict';
import {f3World} from './fixtures/f3-world.js';

const classes={soldier:{class:'soldier',base:'soldier',MaxHealth:300,weapons:[]}};

// A native bounds sensor has already made its lazy observation. Reading that
// exact qualified sample changes neither the canonical cache nor its order.
test('history bounds retain vector identity without another sensor export',async()=>{
  const w=await f3World(classes,{debug:false}),unit=w.spawn('soldier',0,0);
  w.phase.phase(1/60);w.phase.registry.bounds(unit);
  const before=w.phase.counters.wasmCalls,bounds=unit.bounds,center=bounds.center;
  assert.equal(unit.sphere,bounds);assert.equal(unit.sphere.center,center);
  assert.equal(w.phase.counters.wasmCalls,before);
  // Admission can grow the shared cache outside ticking. Its descriptor
  // refresh must keep existing bounds reads on the relocated reservation.
  w.phase.construction.reserve(2048);
  assert.equal(w.phase.registry.bounds(unit),bounds);
  assert.equal(unit.bounds.center,center);
  unit.spawn(12,0,3,0);
  const calls=w.phase.counters.wasmCalls;
  w.phase.registry.bounds(unit);
  assert.equal(w.phase.counters.wasmCalls,calls+1,'an old-life cache cannot answer a new life');
  // The head's lateral offset also contributes to this broadphase sphere.
  assert.equal(unit.bounds.center.x,11.955);
  w.phase.registry.bounds(unit);
  w.phase.placeBody(unit,30,0,4,0);
  const placed=w.phase.counters.wasmCalls;
  w.phase.registry.bounds(unit);
  assert.equal(w.phase.counters.wasmCalls,placed+1,'a body-only reset retires its same-life sample');
  // Soldier.spawn does not advance Unit.tickCount. Its already observed
  // same-tick shape deliberately stays stale until the next Unit tick.
  assert.equal(unit.bounds.center.x,11.955);
});

// Ordinary completed rows wait for the outer boundary. An uncontrolled
// target has no aimer import to advance its public tick stamp meanwhile.
// Its native tick must invalidate the shared sample before an AI can read it.
test('bounds reject a completed native tick hidden by delayed body publication',async()=>{
  const w=await f3World(classes,{debug:false});
  const target=w.spawn('soldier',0,0),observer=w.spawn('soldier',10,0);
  w.phase.controller(target,null);
  w.phase.phase(1/60);w.phase.registry.bounds(target);
  const oldTick=target.tickCount;
  target.intent.forward=1;
  let seen=null;
  observer.onMoved=()=>{
    assert.equal(target.tickCount,oldTick);
    const calls=w.phase.counters.wasmCalls;
    seen=target.sphere.center.toArray();
    assert.equal(w.phase.counters.wasmCalls,calls+1);
  };
  w.phase.phase(1/60);
  assert.ok(seen);
  target.updateSpheres();
  assert.deepEqual(seen,target.bounds.center.toArray());
});
