import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { nativeCombat } from './fixtures/native-combat.js';

const rolling = () => nativeCombat({base:'walkerdroid',mode:12,health:1000000,
  definition:{BallRollingFriction:50,BallAcceleration:100}});

// Droideka.tick's steering-only input changes heading without inventing
// propulsion. Movement and its final floor contact stay in the owning phase.
test('native rolling turns without propulsion and retains its resolved floor position', async() => {
  const {world,unit,soldier}=await rolling();
  unit.intent.right=1;
  world.phase.counters.reset();
  for(let tick=0;tick<60;tick++)world.phase.phase(1/60);
  assert.ok(soldier.aimYaw < -1.9);
  assert.equal(soldier.spin,0);
  assert.ok(Math.hypot(soldier.position.x,soldier.position.z)<1e-8);
  assert.ok(Math.abs(soldier.position.y)<1e-8);
  assert.equal(world.phase.counters.rustAllocations,0);
  assert.equal(world.phase.counters.rustReallocations,0);
});

test('native rolling preserves its speed cap, steers in the new heading and brakes', async() => {
  const {world,unit,soldier}=await rolling();
  unit.intent.forward=1;
  for(let tick=0;tick<60;tick++)world.phase.phase(1/60);
  assert.ok(Math.abs(soldier.velocity.length()-13)<1e-6);
  unit.intent.right=1;world.phase.phase(1/60);
  assert.ok(soldier.velocity.x<0);
  const before=soldier.aimYaw;unit.intent.forward=0;
  for(let tick=0;tick<60;tick++)world.phase.phase(1/60);
  assert.equal(soldier.spin,0);assert.ok(soldier.aimYaw<before-1.9);
});

test('native rolling stays resolved across supported step durations', async() => {
  const runs=[];
  for(const hz of [30,60,120]){
    const {world,unit,soldier}=await rolling();unit.intent.forward=1;
    for(let tick=0;tick<hz*2;tick++)world.phase.phase(1/hz);
    assert.ok(Math.abs(soldier.position.y)<1e-8);runs.push(soldier.position.z);
  }
  assert.ok(Math.max(...runs)-Math.min(...runs)<.2);
});

test('native droideka contacts retain the ten metre-per-second fall threshold', async() => {
  for(const [height,unhurt] of [[4,true],[20,false]]){
    const {world,soldier,combat}=await nativeCombat({base:'walkerdroid',health:1000000,position:new Vector3(0,height,0)});
    let impact=0;
    for(let tick=0;tick<600;tick++){
      const before=soldier.velocity.y;world.phase.phase(1/60);
      if(soldier.grounded&&!impact)impact=-(before-9.8/60);
      if(soldier.grounded)break;
    }
    const damage=1000000-combat.health;
    if(unhurt)assert.equal(damage,0);
    else assert.ok(Math.abs(damage-(impact-10)**2)<1e-6);
  }
});
