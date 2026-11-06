import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { ObjectiveTarget } from '../src/objectives.js';
import { f3World } from './fixtures/f3-world.js';
import { weaponDefinition } from './fixtures/client-weapons.js';

test('the owned phase consumes changed foreign targets with intents through one export', async() => {
  const launcher = weaponDefinition('launcher',0,{
    base:'launcher',LockOnRange:100,LockOnAngle:10,LockOffAngle:15,LockTime:1,
    TargetEnemy:1,TargetBuilding:1,ordnance:{ class:'missile',base:'missile' },
  });
  const world = await f3World({
    soldier:{ class:'soldier',base:'soldier',MaxHealth:2000,weapons:[launcher] },
  });
  const soldier = world.spawn('soldier',0,0,true);
  world.phase.begin(world.phase.bindings[soldier.id],'team');
  world.phase.change.value = 1;
  world.phase.commit();
  const building = {
    desc:{class:'objective',settings:{}}, position:new Vector3(0,1.5,12),
    health:1000,maxHealth:1000,alive:true,team:2,bodies:{built:null},
    sphere(out) { out.copy(this.position); return 2; },
  };
  assert.equal(world.phase.registry.foreignMembers.length,0,'owned units do not enter the foreign scan');
  const target = world.units.add(new ObjectiveTarget(building));
  assert.deepEqual(world.phase.registry.foreignMembers,[target]);
  world.phase.bodiesIn();world.phase.counters.reset();
  world.phase.phase(1/60);
  assert.equal(soldier.combat.arsenal.weapons[0].lockTargetId,target.id);
  assert.equal(world.phase.counters.wasmCalls,1);
  assert.equal(world.phase.counters.importCalls,1);
  building.health = 0;building.alive = false;
  world.phase.phase(1/60);
  assert.equal(soldier.combat.arsenal.weapons[0].lockTargetId,-1);
  assert.equal(world.phase.counters.wasmCalls,2);
  assert.equal(world.phase.counters.commandCalls,0);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
  world.units.remove(target);
  assert.equal(world.phase.registry.foreignMembers.length,0,'registry removal retires its foreign input too');
  world.phase.phase(1/60);
  assert.equal(soldier.combat.arsenal.weapons[0].lockTargetId,-1);
});
