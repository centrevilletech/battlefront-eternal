import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as THREE from 'three';
import {BodyPreview,BodyAimBatch} from '../src/presentation/body.js';
import {owner} from './helpers/body.js';
const rows=JSON.parse(fs.readFileSync(new URL('./fixtures/presentation/aim-goldens.json',import.meta.url)));

// Frozen outputs from PlayerCombat.aimBasis and eye, before their deletion.
// Absolute coordinates exercise the sequential gun offsets and eye setback.
test('native combat bases and eyes match frozen posture and airborne records',()=>{
  const native=owner(null),phase=new BodyPreview(null,native);
  for(const row of rows){
    const body={...row,position:new THREE.Vector3().fromArray(row.position),aimYaw:row.yaw,aimPitch:row.pitch};
    phase.begin(body,{zoom:row.zoom});phase.run();
    const b=phase.basis;
    const bits=v=>Array.from(new Uint32Array(Float64Array.from(v).buffer));
    assert.deepEqual(bits([b.origin,b.pivot,b.left,b.up,b.aimDir].flatMap(v=>v.toArray())),row.basisBits);
    assert.deepEqual(bits(b.eyePoint.toArray()),row.eyeBits);
  }
});

// Capacity is native-owned. A malformed count independently tests the native
// clamp without relying on the JavaScript writer to protect its span.
test('an oversized shot aim block reuses its reserved memory',()=>{
  const native=owner(null),batch=new BodyAimBatch({rays:{x:native}}),buffer=native.memory.buffer;
  native.body_aim_batch(0xffffffff,0);native.body_aim_batch(0xffffffff,1);
  assert.equal(native.memory.buffer,buffer);
  assert.equal(batch.write(batch.capacity,{},{}),false);
});
