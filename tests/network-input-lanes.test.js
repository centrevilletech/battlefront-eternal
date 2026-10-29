import test from 'node:test';
import assert from 'node:assert/strict';
import { InputMailbox } from '../src/runtime/transport.js';
import { InputActions } from '../src/runtime/input-actions.js';

const state = (sequence, extra = {}) => ({ epoch:1, generation:1, sequence, tick:sequence,
  down:['KeyW'], actions:[], look:[], lookTotal:[sequence * 2, -sequence], ...extra });
const action = (id, extra = {}) => ({ epoch:1, generation:1, id, tick:id, actions:['jump'], ...extra });

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
    assert.throws(()=>mailbox.push(state(2,{lookTotal})), /cumulative look/);
  }
});

test('delayed reliable actions survive a newer movement acknowledgment exactly once', () => {
  const movement = new InputMailbox(), actions = new InputActions();
  movement.push(state(20)); movement.take(20,1);
  assert.equal(actions.push(action(5),20,1),true);
  assert.equal(actions.ack,0,'receipt is not an execution acknowledgment');
  assert.deepEqual(actions.take(21,1),{actions:['jump'],human:[]});
  assert.equal(actions.ack,5); assert.equal(movement.ack,20);
  assert.equal(actions.push(action(5),21,1),false);
  assert.deepEqual(actions.take(22,1),{actions:[],human:[]});
  assert.equal(actions.push(action(25,{actions:['reload']}),22,1),true);
  assert.deepEqual(actions.take(24,1),{actions:[],human:[]});
  assert.deepEqual(actions.take(25,1).actions,['reload']);
  assert.equal(actions.ack,25,'gaps in command sequence are expected');
});

test('reliable actions preserve repeated presses, spawn order, lifetime and bounded recovery', () => {
  const actions = new InputActions(1,2);
  const human=[{kind:'team',team:2},{kind:'spawn',className:'soldier',post:null}];
  actions.push(action(1,{actions:['use','use'],human}),1,1);
  assert.deepEqual(actions.take(1,1),{actions:['use'],human});
  assert.equal(actions.ack,0);
  assert.deepEqual(actions.take(2,1),{actions:['use'],human:[]});
  assert.equal(actions.ack,1);
  actions.push(action(3),3,1);
  assert.deepEqual(actions.take(4,2),{actions:[],human:[]});
  assert.equal(actions.ack,1);assert.equal(actions.discarded,3,'old-life queued edges cannot act on a new body');
  assert.equal(actions.push(action(4),4,2),false);
  actions.push(action(5,{generation:2}),5,2);
  assert.deepEqual(actions.take(126,2),{actions:[],human:[]});
  assert.equal(actions.ack,1);assert.equal(actions.discarded,5,'obsolete edges are reported as discarded');
  actions.push(action(127,{generation:2}),127,2);
  actions.push(action(128,{generation:2}),128,2);
  assert.equal(actions.push(action(129,{generation:2}),129,2),false,'a full queue refuses the press without failing the player');
  assert.equal(actions.queue.length,actions.capacity);
  actions.clear(); assert.equal(actions.ack,1);assert.equal(actions.discarded,128);
  assert.throws(()=>actions.push(action(129,{generation:2,actions:['firePrimary']}),129,2),/reliable actions/);
});
