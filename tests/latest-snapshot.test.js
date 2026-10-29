import test from 'node:test';import assert from 'node:assert/strict';
import {LatestSnapshot} from '../src/runtime/latest-snapshot.js';
test('slow decoding retains only the freshest waiting snapshot',async()=>{
 const consumed=[];let resume;const gate=new Promise(resolve=>resume=resolve);
 const queue=new LatestSnapshot(async value=>{consumed.push(value);if(value===1)await gate;});
 const running=queue.push(1);for(let i=2;i<=100;i++)queue.push(i);
 assert.deepEqual(consumed,[1]);assert.equal(queue.pending,100);resume();await running;assert.deepEqual(consumed,[1,100]);assert.equal(queue.pending,null);assert.equal(queue.running,null);
});
test('reset discards a waiting decode without interrupting the active generation',async()=>{
 let resume;const consumed=[],gate=new Promise(resolve=>resume=resolve);
 const queue=new LatestSnapshot(async value=>{consumed.push(value);await gate;});const running=queue.push(1);queue.push(2);queue.clear();resume();await running;assert.deepEqual(consumed,[1]);
});
