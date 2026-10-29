import test from 'node:test';
import assert from 'node:assert/strict';
import {PresentationBacklog} from '../src/runtime/presentation-backlog.js';
test('first viewer receives current state without historical gunfire',()=>{
 const queue=new PresentationBacklog();for(let tick=0;tick<600;tick++)queue.push(tick,[['old',tick]]);
 assert.deepEqual(queue.take(600),[]);
 queue.push(601,[['fire',1]]);queue.push(602,[['impact',2]]);
 assert.deepEqual(queue.take(603),[['fire',1],['impact',2]]);
});
test('inactive viewers cannot accumulate obsolete effects and live overflow is bounded',()=>{
 const queue=new PresentationBacklog({maxCalls:2});queue.take(0);queue.push(1,[1]);queue.push(7,[2]);assert.deepEqual(queue.take(8),[]);
 assert.equal(queue.push(9,[1,2,3]),false);assert.deepEqual(queue.take(10),[]);
 queue.push(11,[4]);assert.deepEqual(queue.take(12),[4]);
});
