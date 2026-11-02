import test from 'node:test';
import assert from 'node:assert/strict';
import { batchedPoster, postedItems } from '../src/runtime/local-message-batch.js';
import { LocalTransport } from '../src/runtime/local-transport.js';

// The local world posts a step's messages to the page as one; the page takes
// them in the order they were sent, exactly as if each had been posted alone.
test('a step\'s messages go as one post, in order, its buffers transferred', async () => {
  const posts = [], { send, flush } = batchedPoster((message, transfer) => posts.push({ message, transfer }));
  const a = new ArrayBuffer(24), b = new ArrayBuffer(30);
  send('state', '{"type":"input-ack"}'); send('state', a); send('events', '{"type":"entity-edge-results"}'); send('state', b);
  flush();
  assert.equal(posts.length, 1);
  assert.deepEqual(postedItems(posts[0].message).map(m => [m.channel, m.payload]), [['state', '{"type":"input-ack"}'], ['state', a], ['events', '{"type":"entity-edge-results"}'], ['state', b]]);
  assert.deepEqual(posts[0].transfer, [a, b]);
  flush();
  assert.equal(posts.length, 1, 'nothing queued, nothing posted');
  // Sent outside a step (a reply, an error): posted once the task's microtasks run.
  send('events', '{"type":"reply"}');
  assert.equal(posts.length, 1);
  await null;
  assert.equal(posts.length, 2);
  assert.deepEqual(posts[1].message, { channel: 'events', payload: '{"type":"reply"}' }, 'a lone message goes as before');
  assert.deepEqual(postedItems(posts[1].message), [posts[1].message]);
  // Two steps in one task stay two posts, each right after its step.
  send('state', new ArrayBuffer(20)); flush(); send('state', new ArrayBuffer(21)); flush();
  await null;
  assert.equal(posts.length, 4);
});

test('the page receives a batch\'s messages in order, and a lone message as before', async () => {
  const worker = { postMessage() {}, terminate() {} }, received = [];
  const transport = new LocalTransport({ workerFactory: () => worker });
  await transport.connect({ query: '', settings: {}, content: '', playerName: 'P', channels: new Map(), receive: async (channel, payload) => { received.push([channel, payload]); }, fail: error => { throw error; } });
  const a = new ArrayBuffer(24);
  worker.onmessage({ data: { batch: [{ channel: 'state', payload: 'ack' }, { channel: 'state', payload: a }, { channel: 'events', payload: 'edges' }] } });
  worker.onmessage({ data: { channel: 'events', payload: 'reply' } });
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.deepEqual(received, [['state', 'ack'], ['state', a], ['events', 'edges'], ['events', 'reply']]);
  transport.dispose();
});
