import test from 'node:test';
import assert from 'node:assert/strict';
import {deltaChanged,markDelta} from '../src/sim/compare.js';

// Publication groups cross mask words, while borrowed vectors can straddle
// either end. Empty groups and neighbouring fields must remain untouched.
test('publication ranges preserve both partial mask words',()=>{
  const mask=new Uint32Array(4);
  markDelta(mask,127,34,97);
  assert.deepEqual([...mask],[0xc0000000,0xffffffff,0,0]);
  assert.equal(deltaChanged(mask,126,1,97),false);
  assert.equal(deltaChanged(mask,127,1,97),true);
  assert.equal(deltaChanged(mask,160,1,97),true);
  assert.equal(deltaChanged(mask,161,1,97),false);
  assert.equal(deltaChanged(mask,97,64,97),true);
  markDelta(mask,97,0,97);
  assert.equal(deltaChanged(mask,97,0,97),false);
});

test('publication ranges distinguish high bits from the following word',()=>{
  const mask=new Uint32Array([0x80000000,0,0x80000000]);
  assert.equal(deltaChanged(mask,31,1),true);
  assert.equal(deltaChanged(mask,32,63),false);
  assert.equal(deltaChanged(mask,32,64),true);
  assert.equal(deltaChanged(mask,0,32),true);
  markDelta(mask,33,3);
  assert.equal(mask[1],14);
});
