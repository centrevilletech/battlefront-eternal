import test from 'node:test';
import assert from 'node:assert/strict';
import { InputMailbox } from './fixtures/input-mailbox.js';

const state = (sequence, extra = {}) => ({ epoch:1, generation:1, sequence, tick:sequence,
  down:['KeyW'], actions:[], look:[], lookTotal:[sequence * 2, -sequence], ...extra });

test('held-state recovery uses consumed client time, never future or previous-life state', () => {
  const mailbox = new InputMailbox();
  mailbox.push(state(5)); mailbox.take(8,1);
  assert.equal(mailbox.stateTick,5);
  mailbox.push(state(12)); mailbox.take(10,1);
  assert.equal(mailbox.stateTick,5,'server time cannot make an old release appear newer');
  mailbox.take(12,2);
  assert.equal(mailbox.stateTick,0,'old-life input cannot release a new-life trigger');
  mailbox.push(state(13,{generation:2})); mailbox.take(13,2);
  assert.equal(mailbox.stateTick,13);
  mailbox.clear(); assert.equal(mailbox.stateTick,0);
});

test('unreliable input recovers mouse counts across loss, reordered duplicates and future delivery', () => {
  const mailbox = new InputMailbox();
  mailbox.push(state(1));
  assert.deepEqual(mailbox.take(1,1).look, [[2,-1]]);
  mailbox.push(state(6)); // Four consecutive packets disappeared.
  assert.equal(mailbox.push(state(4)), false);
  assert.deepEqual(mailbox.take(5,1).look, [], 'future input waits for its tick');
  assert.deepEqual(mailbox.take(6,1).look, [[10,-5]]);
  assert.equal(mailbox.push(state(6)), false);
  assert.deepEqual(mailbox.take(7,1).look, []);
  mailbox.clear(); // A focus pause does not reset the current life's mouse sum.
  mailbox.push(state(8,{down:[]}));
  assert.deepEqual(mailbox.take(8,1).look, [[4,-2]]);
  mailbox.push(state(9,{generation:2,lookTotal:[3,4]}));
  assert.deepEqual(mailbox.take(9,2).look, [[3,4]], 'a new life starts its own sum');
  assert.deepEqual(mailbox.take(10,2).look, []);
});

test('cumulative input ignores duplicate delta representation and rejects malformed totals', () => {
  const mailbox = new InputMailbox();
  mailbox.push(state(1,{look:[[100,100]]}));
  assert.deepEqual(mailbox.take(1,1).look, [[2,-1]]);
  for (const lookTotal of [null, [1], [1,NaN], [Infinity,0], [2**49,0]]) {
    assert.equal(mailbox.push(state(2,{lookTotal})),false);
  }
});

test('native outgoing triggers preserve short clicks, holds and life changes', async () => {
  const {EntityInputSender}=await import('../src/runtime/network-entity-sender.js');
  const {decodeEntityInput,INPUT_CODES,DEFAULT_BINDING_PROFILES}=await import('../src/runtime/network-entity-input.js');
  const registry={inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES},seen=[],held=[];let sequence=0,generation=1;
  const sender=new EntityInputSender({registry,onFire:event=>{if(event.held)held.push(event.button);},send:bytes=>{
    const packet=decodeEntityInput(bytes,registry);for(const event of packet.edges)if(event.kind==='fire')seen.push([event.button==='fireSecondary'?1:0,event.pressed]);
    sender.edges.receipt({epoch:1,generation,ids:packet.edges.map(edge=>edge.id)});return true;
  }});
  const run=command=>{generation=command.generation??1;sender.send([{epoch:1,generation,sequence:++sequence,tick:sequence,...command}]);};
  run({down:[],actions:['firePrimary']});assert.deepEqual(seen.splice(0),[[0,true],[0,false]]);
  run({down:['Mouse0'],actions:['firePrimary']});assert.deepEqual(seen.splice(0),[[0,true]]);
  run({down:['Mouse0'],actions:[]});run({down:['Mouse0','Mouse2'],actions:['fireSecondary']});
  assert.deepEqual(seen.splice(0),[[1,true]]);assert.deepEqual(held.splice(0),[0,0]);
  run({down:[],actions:[]});assert.deepEqual(seen.splice(0),[[0,false],[1,false]]);
  run({down:['Mouse0'],actions:[],generation:2});assert.deepEqual(seen.splice(0),[[0,true]]);sender.dispose();
});
test('a failed outgoing packet retains its reliable trigger without duplicating the edge', async () => {
  const {EntityInputSender}=await import('../src/runtime/network-entity-sender.js');
  const {decodeEntityInput,INPUT_CODES,DEFAULT_BINDING_PROFILES}=await import('../src/runtime/network-entity-input.js');
  const registry={inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES};let accept=false,packet;
  const sender=new EntityInputSender({registry,send:bytes=>{packet=decodeEntityInput(bytes,registry);return accept;}});
  const command=sequence=>({epoch:1,generation:1,sequence,tick:sequence,down:['Mouse0'],actions:[],look:[]});
  assert.equal(sender.send([command(1)]),false);assert.equal(sender.edges.pending.size,1);
  accept=true;assert.equal(sender.send([command(2)]),true);assert.equal(packet.edges.length,1);assert.equal(packet.edges[0].pressed,true);
  sender.clearPending();sender.send([command(3)]);assert.equal(packet.edges.length,2);sender.dispose();
});
