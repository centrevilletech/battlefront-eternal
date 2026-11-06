import test from 'node:test';
import assert from 'node:assert/strict';
import { Identity, MirrorPool } from '../src/sim/runtime.js';
import { KIND_RECON_DROID, KIND_BOLT } from '../src/sim/gen/index.js';

test('a retained native facade reads its old generation and encodes stale commands faithfully', () => {
  const refs = new Identity(KIND_RECON_DROID);
  const old = {dead:true}, next = {dead:false}, row = new Float64Array(2);
  refs.bind(0,1,old);
  refs.retain(0,1);
  refs.bind(0,2,next);
  assert.ok(refs.at(0,1) === old);assert.ok(refs.at(0,2) === next);
  refs.put(old,row,0);assert.deepEqual(Array.from(row),[0,1]);
  refs.put(next,row,0);assert.deepEqual(Array.from(row),[0,2]);
  refs.forget(0,1);
  assert.throws(()=>refs.at(0,1),/generation 1/);
  refs.put(old,row,0);assert.deepEqual(Array.from(row),[0,1]);
  assert.equal(refs.displaced.size,0);
  refs.release(next);
  refs.bind(0,3,{});
  assert.equal(refs.free.length,0,'native binding consumes the released slot');
});

test('generated mirrors reuse the same retained generation until its final retirement', () => {
  const refs = new MirrorPool(KIND_BOLT);
  const old = refs.at(0,1), next = refs.at(0,2);
  old.age = 0.25;next.age = 0.5;
  assert.ok(refs.at(0,1) === old);
  assert.ok(refs.at(0,2) === next);
  assert.ok(refs.retire(0,1) === old);
  assert.equal(old.age,0.25);assert.equal(next.age,0.5);
  assert.equal(refs.displaced.size,0);
});


test('reserved facade capacity does not consume logical identity slots', () => {
  const refs = new Identity(KIND_RECON_DROID);
  const native = {}, foreign = {};
  refs.reserve(16);refs.prepare(native);
  assert.equal(refs.nextSlot,0);
  assert.equal(refs.slotOf(foreign),0);
  refs.bind(1,1,native);
  assert.equal(refs.nextSlot,2);
  refs.reserve(32);
  assert.ok(refs.at(0,1) === foreign);assert.ok(refs.at(1,1) === native);
  assert.equal(refs.slotOf({}),2);
});
