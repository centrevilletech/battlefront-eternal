import test from 'node:test';
import assert from 'node:assert/strict';
import { f3World } from './fixtures/f3-world.js';

// Passive registry bodies isolate the actual end-of-phase separation from
// movement. They still use canonical slabs, publication and lazy hit shapes.
async function crowd(names, prepare = null) {
  const definitions = {};
  for (const name of names) definitions[name] = {
    class:name,base:'soldier',MaxHealth:2000,weapons:[],
  };
  return f3World(definitions, {
    passiveClasses:names,
    prepareUnit:unit=>{
      unit.alive = true;unit.removed = false;
      prepare?.(unit);
    },
  });
}

test('the owned phase separates coincident bodies and publishes only pushed rows', {timeout:10000}, async() => {
  const world = await crowd(['first','second','far'],unit=>{
    if (unit.className === 'far') unit.position.set(100,0,0);
  });
  const [first,second,far] = world.passive;
  const position = first.position, previous = first.soldier.previous;
  first.updateSpheres();const stale = first.spheresTick;
  world.phase.bodiesIn();world.phase.counters.reset();world.phase.phase(1/60);
  // Frozen Node results from separateSoldiers' coincident-id direction. The
  // original resolves by sin(first.id), cos(first.id), retaining pair order.
  assert.equal(first.position.x,0.479638461340501);
  assert.equal(first.position.z,0.3079723143448397);
  assert.equal(second.position.x,-0.479638461340501);
  assert.equal(second.position.z,-0.3079723143448397);
  assert.equal(first.tickCount,stale+1);assert.equal(second.tickCount,1);
  assert.equal(far.tickCount,0);assert.equal(far.position.x,100);
  assert.ok(first.position === position);assert.ok(first.soldier.previous === previous);
  assert.equal(previous.x,0);assert.equal(previous.z,0);
  assert.equal(first.spheresTick,stale,'publication does not sample the invalidated shape');
  first.updateSpheres();assert.equal(first.spheresTick,first.tickCount);
  assert.equal(world.phase.layout.outputRows,2);
  assert.equal(world.phase.counters.phaseCalls,1);
  assert.equal(world.phase.counters.importCalls,0);
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

test('separation reads the camera and canonical viewpoint state without body imports', {timeout:10000}, async() => {
  const world = await crowd(['viewpoint','other']);
  const [player,other] = world.passive;
  world.phase.registerViewpoint(player);
  world.separation.global = false;world.separation.camera.set(100,0,0);
  world.phase.bodiesIn();world.phase.counters.reset();world.phase.phase(1/60);
  assert.equal(world.phase.layout.outputRows,0);assert.equal(player.tickCount,0);
  world.separation.global = true;player.combat.dead = true;
  world.phase.phase(1/60);
  assert.equal(world.phase.layout.outputRows,0);assert.equal(other.tickCount,0);
  player.combat.dead = false;world.phase.phase(1/60);
  assert.equal(world.phase.layout.outputRows,2);assert.equal(player.tickCount,1);
  assert.equal(world.phase.counters.wasmCalls,5);
  assert.equal(world.phase.counters.phaseCalls,3);
  assert.equal(world.phase.counters.commandCalls,2);
  assert.equal(world.phase.counters.importCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

test('the final separation includes lingering corpses and skips seats and held bodies', {timeout:10000}, async() => {
  const world = await crowd(['live','corpse','seated','held','removed'],unit=>{
    if (unit.className === 'corpse') {
      unit.alive = false;unit.dead = true;
      unit.combat.state.dead = true;unit.combat.state.health = 0;
    } else if (unit.className === 'seated') unit.seat = {};
    else if (unit.className === 'held') unit.held = {prop:{},since:0,frame:null,time:undefined};
    else if (unit.className === 'removed') unit.removed = true;
  });
  const [live,corpse,seated,held,removed] = world.passive;
  world.phase.bodiesIn();world.phase.counters.reset();world.phase.phase(1/60);
  assert.equal(live.tickCount,1);assert.equal(corpse.tickCount,1);
  assert.ok(live.position.distanceToSquared(corpse.position) > 0);
  assert.equal(seated.tickCount,0);assert.equal(held.tickCount,0);assert.equal(removed.tickCount,0);
  assert.equal(world.phase.layout.outputRows,2);
  assert.equal(world.phase.counters.importCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});
