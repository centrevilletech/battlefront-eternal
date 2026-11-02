import test from 'node:test';
import assert from 'node:assert/strict';
import { batchedPoster, postedItems } from '../src/runtime/local-message-batch.js';
import { LocalTransport } from '../src/runtime/local-transport.js';

// The local world posts a step's messages to the page as one; the page takes
// them in the order they were sent, exactly as if each had been posted alone.
test('a step\'s messages go as one post, in order, its buffers transferred', async () => {
  let clock = 1000;
  const posts = [], { send, flush } = batchedPoster((message, transfer) => posts.push({ message, transfer }), () => clock++);
  const a = new ArrayBuffer(24), b = new ArrayBuffer(30);
  send('state', '{"type":"input-ack"}'); send('state', a); send('events', '{"type":"entity-edge-results"}'); send('state', b);
  flush();
  assert.equal(posts.length, 1);
  assert.deepEqual(postedItems(posts[0].message).map(m => [m.channel, m.payload]), [['state', '{"type":"input-ack"}'], ['state', a], ['events', '{"type":"entity-edge-results"}'], ['state', b]]);
  assert.deepEqual(posts[0].transfer, [a, b]);
  assert.equal(posts[0].message.sentAt, 1000, 'stamped with when it left');
  flush();
  assert.equal(posts.length, 1, 'nothing queued, nothing posted');
  // Sent outside a step (a reply, an error): posted once the task's microtasks run.
  send('events', '{"type":"reply"}');
  assert.equal(posts.length, 1);
  await null;
  assert.equal(posts.length, 2);
  assert.deepEqual(posts[1].message, { channel: 'events', payload: '{"type":"reply"}', sentAt: 1001 }, 'a lone message goes as before');
  assert.deepEqual(postedItems(posts[1].message), [posts[1].message]);
  // Two steps in one task stay two posts, each right after its step.
  send('state', new ArrayBuffer(20)); flush(); send('state', new ArrayBuffer(21)); flush();
  await null;
  assert.equal(posts.length, 4);
});

test('the page receives a batch\'s messages in order, and a lone message as before', async () => {
  const worker = { postMessage() {}, terminate() {} }, received = [];
  const transport = new LocalTransport({ workerFactory: () => worker });
  await transport.connect({ query: '', settings: {}, content: '', playerName: 'P', channels: new Map(), receive: async (channel, payload, at, sentAt) => { received.push([channel, payload, sentAt]); }, fail: error => { throw error; } });
  const a = new ArrayBuffer(24);
  // When the world posted them, on this side's clock.
  worker.onmessage({ data: { batch: [{ channel: 'state', payload: 'ack' }, { channel: 'state', payload: a }, { channel: 'events', payload: 'edges' }], sentAt: performance.timeOrigin + 50 } });
  worker.onmessage({ data: { channel: 'events', payload: 'reply' } });
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.deepEqual(received.map(([channel, payload]) => [channel, payload]), [['state', 'ack'], ['state', a], ['events', 'edges'], ['events', 'reply']]);
  assert.deepEqual(received.map(([, , sentAt]) => sentAt === undefined ? undefined : Math.round(sentAt)), [50, 50, 50, undefined]);
  transport.dispose();
});
