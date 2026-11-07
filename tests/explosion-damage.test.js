import './fixtures/native-anim.js';
import { KIND_ITEM } from '../src/sim/gen/index.js';
import { rustItems } from './helpers/rust-simulation.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';

import {f3World} from './fixtures/f3-world.js';
import {building} from './fixtures/f3-building.js';

// One area damage rule for everything a blast reaches (src/ordnance.js,
// "What an explosion does"), as the 2004 game's ExplosionClass::Explode
// measures it: the distance to the target's bounding sphere's centre less
// half its radius (a soldier's sphere by stance), line of sight to the
// target's sight point through terrain and buildings only, and the target's
// HealthType scale. The numbers are the retail rocket launcher explosion's.
const ROCKET = { Damage: 320, DamageRadiusInner: 0.75, DamageRadiusOuter: 2, Push: 10, PushRadiusInner: 0.75, PushRadiusOuter: 0.75,
  PersonScale: 1, DroidScale: 1, VehicleScale: 0.25, BuildingScale: 0.25, AnimalScale: 1 };
const near = (a, b, tolerance = 1e-6) => assert.ok(Math.abs(a - b) <= tolerance, `${a} is not within ${tolerance} of ${b}`);

test('a building is measured from its model\'s bounding sphere and shielded by a wall', async () => {
  for(const wall of [false,true]){
    const object=building('building',0,0,{health:1000});
    object.looks.built.model.sphere=[0,1.65,.32,2.25];
    const w=await f3World({}, {prepareLive:()=>({buildings:[object]}),prepareCollision:collision=>{
      if(wall)collision.addBody(new Float32Array([1,0,-50,1,5,-50,1,5,50,1,0,50]),new Uint32Array([0,1,2,0,2,3]),new Uint32Array([0,0]));
    }});
    w.ordnance.explosion(ROCKET,new THREE.Vector3(2,1.65,.32),null);
    near(1000-object.health,wall?0:320*(2-.875)/1.25*.25);
  }
});
test('native mine blast damage reaches only landed items', () => {
  const items = rustItems({ scene: new THREE.Scene(), collision: { cast: () => null }, effects: {}, lights: { add() {} }, model: () => null, explode() {} });
  const mine = items.spawn({ base: 'mine', MaxHealth: 200 }, new THREE.Vector3(1, 0.1, 0), new THREE.Vector3(), 0);
  items.blast(ROCKET, new THREE.Vector3(0, 0.1, 0));
  assert.equal(mine.health, 200, 'in flight');
  mine.landed = true;
  items.sim.bridge.writeSome(KIND_ITEM, [mine], true);
  items.blast(ROCKET, new THREE.Vector3(0, 0.1, 0));
  assert.ok(mine.health < 200);
});
