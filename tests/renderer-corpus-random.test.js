import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import { SEED_SRC } from '../tools/renderer-corpus/battle.mjs';

test('procedural capture randomness is independent of object UUID allocations', () => {
 const capture=extra=>vm.runInNewContext(`${SEED_SRC}\nfunction generateUUID(){return [Math.random(),Math.random(),Math.random(),Math.random()];}\nfor(let i=0;i<${extra};i++)generateUUID();\nJSON.stringify({values:[Math.random(),Math.random(),Math.random()],stats:__gateRandom});`);
 const a=JSON.parse(capture(1)),b=JSON.parse(capture(20));
 assert.deepEqual(a.values,b.values);
 assert.equal(a.stats.uuidCalls,4);
 assert.equal(b.stats.uuidCalls,80);
 assert.equal(a.stats.valueCalls,3);
});
