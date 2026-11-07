import './fixtures/native-anim.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {Droideka} from './helpers/droideka.js';
import {bodyNative} from '../src/presentation/body.js';
import {DroidekaSpawnLayout as D} from '../src/sim/gen/units.js';
import {BlockHeaderLayout as H} from '../src/sim/gen/boundary.js';
import {CollisionWorld} from '../tools/collision-fixture.mjs';

// The old spawn grounded the body, entered its deployed mode and restored
// the shield. Animator commands follow publication and retain the roll anchor.
test('native droideka spawn grounds and resets a dirty body without a host probe',()=>{
  const world=new CollisionWorld(new Float32Array([-10,0,-10,10,0,-10,10,0,10,-10,0,10]),
    new Uint32Array([0,2,1,0,3,2]),new Uint32Array([0,1]));
  world.cast=()=>{throw Error('spawn used the host collision facade');};
  const body=new Droideka(world,{},new Map(),{root:new THREE.Group(),bones:[]});
  body.mode=12;body.time=5;body.reverse=true;body.dead=true;body.roll=4;
  body.spin=3;body.legs=.6;body.velocity.set(9,8,7);
  body.shield={max:80,value:2,on:true};body.spawn(2,3,4,.6);
  for(const key of ['position','previous','settled'])assert.deepEqual(body[key].toArray(),[2,0,4]);
  assert.deepEqual(body.velocity.toArray(),[0,0,0]);
  assert.deepEqual([body.aimYaw,body.aimPitch,body.mode,body.time,body.spin,body.legs],[.6,0,0,0,0,0]);
  assert.equal(body.dead,false);assert.equal(body.reverse,false);
  assert.equal(body.shield.value,80);assert.equal(body.shield.on,false);
  assert.equal(body.roll,4);assert.equal(body.animator.facing,.6);assert.equal(body.animator.rollOrigin,4);
  assert.equal('spawnOrigin' in body,false);assert.equal('hit' in body,false);
});

// A bad cold world id cannot become an unchecked native collision index.
// The same bound protects a viewpoint whose former world has been disposed.
test('an invalid cold spawn world is refused without trapping',()=>{
  const body=new Droideka({}, {},new Map(),{root:new THREE.Group(),bones:[]}),native=bodyNative(body);
  const ptr=native.sim_call_room(D.words,0);
  new Float64Array(native.memory.buffer,ptr+H.words*8,D.words).fill(0);
  assert.ok(native.body_spawn_droideka(0x7fffffff)>0);
});
