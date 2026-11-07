import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { SimEvents } from '../src/sim-events.js';
import { AiNotices } from '../src/sim/wasm/ai-world-notices.js';

test('native voice projection preserves source facts, optional presence and prior lives',()=>{
  const journal=new SimEvents(),projector=new AiNotices(journal,null),actor={generation:99,team:99};
  const notice={actor,generation:1,time:7,team:1,position:new Vector3(1,2,3),human:false,event:'charge',property:'ChargeSound',command:null,context:null,positional:null,notice:null};
  projector.voice(notice);const first=journal.ring[0];
  assert.equal(first.time,7);assert.equal(first.source.unit,actor);assert.equal(first.source.generation,1);assert.equal(first.team,1);assert.deepEqual(first.position,[1,2,3]);
  assert.equal('command' in first,false);assert.equal('context' in first,false);assert.equal('notice' in first,false);
  notice.generation=2;notice.command=0;notice.context=0;notice.positional=false;notice.notice=false;notice.position.set(4,5,6);
  projector.voice(notice);const next=journal.ring[1];
  assert.equal(next.command,0);assert.equal(next.context,0);assert.equal(next.positional,false);assert.equal(next.notice,false);
  assert.equal(first.source.generation,1);assert.deepEqual(first.position,[1,2,3]);assert.notEqual(first.source,next.source);
});
test('native item owner first use projects identity without adding a journal record',()=>{
  const journal=new SimEvents(),actor={},seen=[];
  const projector=new AiNotices(journal,{ownerKey(owner){seen.push(owner);}});
  projector.itemOwner({actor});assert.deepEqual(seen,[actor]);assert.equal(journal.seq,0);
});
