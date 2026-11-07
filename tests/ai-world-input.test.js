import test from 'node:test';
import assert from 'node:assert/strict';
import { SimEvents } from '../src/sim-events.js';
import { AiJournalFacts } from '../src/sim/wasm/ai-world-input.js';

test('AI journal facts preserve sparse positions, overflow and reference presence',()=>{
  const journal=new SimEvents(3),input=new AiJournalFacts(3),unit={alive:true,generation:1};
  journal.emit('shot',{source:{unit,generation:0}});journal.emit('vo');journal.emit('damage',{target:unit,amount:0,generic:0,typed:0,known:false,point:[0,-0,2]});
  assert.equal(input.capture(journal),input);assert.equal(input.count,2);assert.equal(input.sequence,3);
  const shot=input.records[0],damage=input.records[1],point=damage.point;
  assert.equal(shot.sequence,0);assert.equal(shot.source.member,unit);assert.equal(shot.source.generation,0);assert.equal(shot.source.direct,false);
  assert.equal(damage.sequence,2);assert.equal(damage.target.direct,true);assert.equal(damage.amount,0);assert.equal(damage.generic,0);assert.equal(damage.typed,0);
  assert.ok(Object.is(point.y,-0));assert.equal(damage.known,false);
  // Capturing facts cannot resolve weak references or test a direct target's
  // life early. Native ingest performs those reads after prior phase actions.
  unit.alive=false;unit.generation=4;assert.equal(shot.source.generation,0);assert.equal(shot.source.member,unit);
  input.capture(journal);assert.equal(input.count,0);assert.equal(input.records[1],damage);
  for(let i=0;i<5;i++)journal.emit('vo');
  journal.emit('shot',{source:unit});input.capture(journal);
  assert.equal(input.sequence,9);assert.equal(input.count,1);assert.equal(input.records[0].sequence,8);assert.equal(input.records[0],shot);
  assert.equal(input.records[0].source.direct,true);assert.equal(input.records[0].source.generation,null);
  journal.reset();journal.emit('damage',{target:unit});input.capture(journal);
  assert.equal(input.sequence,1);assert.equal(input.count,1);assert.equal(input.records[0].sequence,0);
  assert.equal(input.records[0].point,null);assert.equal(input.records[0].amount,null);
});


test('a long pause and excess journal capacity retain the newest bounded facts',()=>{
  const journal=new SimEvents(8192),input=new AiJournalFacts(4),records=input.records.slice();
  for(let index=0;index<20000;index++)journal.emit('shot');
  input.capture(journal);assert.equal(input.count,4);assert.equal(input.sequence,20000);
  assert.deepEqual(input.records.map(row=>row.sequence),[19996,19997,19998,19999]);
  for(let index=0;index<4;index++)assert.equal(input.records[index],records[index]);
});
