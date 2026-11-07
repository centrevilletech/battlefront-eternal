import test from 'node:test';
import assert from 'node:assert/strict';
import {Soldier} from './helpers/body.js';
import {BodyPreview,bodyNative} from '../src/presentation/body.js';
import {BodyPreviewFrameLayout as F} from '../src/sim/gen/units.js';

// Frozen pointer results cover sample ordering at the prone pitch limits.
// One phase handles every pending sample without changing the source body.
test('ordered immediate look preserves limits, reversals and source state',()=>{
  const body=new Soldier({},{}),preview=new BodyPreview(body.world,bodyNative(body));
  body.posture='prone';body.aimYaw=.25;body.aimPitch=.35;
  preview.begin(body);preview.append([[256,-256],[128,128]],1,1,false);preview.run();
  assert.equal(preview.aimYaw,.0625);assert.equal(preview.aimPitch,.275);
  assert.equal(body.aimYaw,.25);assert.equal(body.aimPitch,.35);
  // A lock suppresses both axes without consuming source mouse history.
  body.lock=1;preview.begin(body);preview.append([[900,900]],1,3,true);preview.run();
  assert.equal(preview.aimYaw,.25);assert.equal(preview.aimPitch,.35);
});

// An idle frame has no pointer work. A moving frame consumes ordered look
// only; command aim explicitly requests the separate gun/eye calculation.
test('frame look preview does no aim work or native call without input',()=>{
  const body=new Soldier({},{}),preview=new BodyPreview(body.world,bodyNative(body));
  body.aimPitch=0;
  const native=preview.x,calls=[];
  preview.x={...native,body_preview(aim){calls.push(aim);return native.body_preview(aim);}};
  for(let frame=0;frame<4;frame++){preview.begin(body);preview.run();}
  assert.deepEqual(calls,[]);
  preview.begin(body);preview.append([[256,0]],1,1,false);preview.run();
  assert.deepEqual(calls,[0]);assert.equal(preview.aimYaw,-.25);
  preview.begin(body,{zoom:1});preview.run();
  assert.deepEqual(calls,[0,1]);assert.deepEqual(preview.basis.origin.toArray(),[-.06,1.5999999999999999,.25]);
});

// The native bound is checked independently of the host append limit. A
// paused burst must keep the reserved heap and return a refusal count.
test('oversized paused input clamps its native count without growing storage',()=>{
  const body=new Soldier({},{}),preview=new BodyPreview(body.world,bodyNative(body)),buffer=preview.looks.buffer;
  preview.begin(body);preview.frame[F.count]=0xffffffff;
  preview.x.body_preview();preview.refresh();
  assert.equal(preview.looks.buffer,buffer);
  assert.equal(preview.frame[F.skipped],0xffffffff-preview.capacity);
});

// A throwing host facade proves grounding borrows the native world. Dirty
// stance and velocity exercise the reset rather than just initial defaults.
test('a cold spawn resets its body against the native collision store',async()=>{
  const {CollisionWorld}=await import('../tools/collision-fixture.mjs');
  const world=new CollisionWorld(new Float32Array([-10,0,-10,10,0,-10,10,0,10,-10,0,10]),
    new Uint32Array([0,2,1,0,3,2]),new Uint32Array([0,1]));
  world.cast=()=>{throw Error('spawn must not call the host ray facade');};
  const body=new Soldier(world,{JetFuelRechargeRate:.06});
  body.velocity.set(9,8,7);body.posture='prone';body.air='thrown';
  body.stateTime=3;body.lock=1;body.spawn(1,4,2,.3);
  assert.deepEqual(body.position.toArray(),[1,0,2]);
  // Interpolation endpoints are reset with the landed position, so the
  // first rendered frame cannot sweep from the body's previous life.
  assert.deepEqual(body.previous.toArray(),[1,0,2]);
  assert.deepEqual(body.settled.toArray(),[1,0,2]);
  assert.deepEqual(body.velocity.toArray(),[0,0,0]);
  assert.equal(body.aimYaw,.3);assert.equal(body.aimPitch,-.07);
  assert.equal(body.posture,'stand');assert.equal(body.air,null);
  assert.equal(body.stateTime,0);assert.equal(body.lock,0);
  // A rechargeable jet starts half full and a real contact marks ground.
  assert.equal(body.fuel,.5);assert.equal(body.grounded,true);
});

// Page content failures return refusal without trapping the shared module.
test('soldier construction refuses malformed JSON and invalid scratch spans',()=>{
  const body=new Soldier({},{}),x=bodyNative(body);
  for(const source of ['{broken','null','[]']) {
    const bytes=new TextEncoder().encode(source),ptr=x.sim_scratch(bytes.length);
    new Uint8Array(x.memory.buffer,ptr,bytes.length).set(bytes);
    assert.equal(x.body_construct(ptr,bytes.length),0);
    assert.equal(x.body_construct(ptr,bytes.length+1),0);
  }
  assert.equal(x.body_construct(0,4),0);
  const bytes=new TextEncoder().encode('{}'),ptr=x.sim_scratch(bytes.length);
  new Uint8Array(x.memory.buffer,ptr,bytes.length).set(bytes);
  assert.ok(x.body_construct(ptr,bytes.length),'valid construction survives refusal');
});
