import test from 'node:test';
import assert from 'node:assert/strict';
import { NetworkSimulator } from '../src/runtime/network-simulator.js';
import { InputMailbox } from '../src/runtime/transport.js';

for (const rtt of [0, 50, 100, 200]) test(`commands and reliable events survive ${rtt}ms RTT with loss/jitter/reordering`, () => {
  const link = new NetworkSimulator({ seed: 123, rtt, jitter: 15, loss: .05, duplicate: .05, reorder: .1 });
  const mailbox = new InputMailbox(), events = [], snapshots = [];
  let ack = 0, commands = [], actions = 0;
  link.receive('server', packet => {
    if (packet.commands) for (const command of packet.commands) mailbox.push(command);
  });
  link.receive('client', packet => {
    if (packet.event) events.push(packet.event);
    if (packet.tick) { snapshots.push(packet.tick); ack = Math.max(ack, packet.ack); }
  });
  for (let tick = 1; tick <= 600; tick++) {
    commands.push({ epoch: 1, generation: 1, sequence: tick, tick: tick + 20,
      down: ['KeyW'], actions: tick % 30 === 0 ? ['reload'] : [], look: [] });
    commands = commands.filter(command => command.sequence > ack);
    link.send('client', { commands: commands.slice(-32) });
    const command = mailbox.take(tick, 1); actions += command.actions.length;
    if (tick % 2 === 0) link.send('server', { tick, ack: mailbox.ack });
    if (tick % 30 === 0) link.send('server', { event: tick }, { reliable: true });
    link.advance(1000 / 60);
  }
  // Drain flight and apply the last commands at their intended server ticks.
  for (let tick = 601; tick <= 650; tick++) {
    link.send('client', { commands }); link.advance(1000 / 60);
    actions += mailbox.take(tick, 1).actions.length;
  }
  link.advance(1000);
  assert.equal(mailbox.ack, 600);
  assert.equal(actions, 20, 'redundancy applies every action once despite loss and duplication');
  assert.deepEqual(events, Array.from({ length: 20 }, (_, i) => (i + 1) * 30));
  assert.ok(snapshots.length > 250);
  assert.ok(link.stats.lost > 0);
  assert.ok(link.stats.duplicated > 0);
  assert.equal(link.queue.length, 0);
});

test('disconnect discards in-flight state and events; reconnect accepts only new epoch', () => {
  const link = new NetworkSimulator({ rtt: 200 }), received = [];
  link.receive('client', (packet, metadata) => received.push([packet, metadata.epoch]));
  link.send('server', 'old state'); link.send('server', 'old event', { reliable: true });
  link.disconnect(); assert.equal(link.send('server', 'offline'), false);
  const epoch = link.reconnect(); link.send('server', 'new baseline', { reliable: true });
  link.advance(500);
  assert.deepEqual(received, [['new baseline', epoch]]);
});

test('a blackholed reliable channel has bounded backpressure and can recover', () => {
  const link = new NetworkSimulator({ loss: 1, capacity: 8 }), received = [];
  link.receive('server', value => received.push(value));
  for (let i = 0; i < 8; i++) assert.equal(link.send('client', i, { reliable: true }), true);
  assert.equal(link.send('client', 9, { reliable: true }), false);
  link.advance(5000); assert.equal(received.length, 0); assert.equal(link.queue.length, 8);
  assert.ok(link.stats.retransmitted > 0);
  link.loss = 0; link.advance(1000);
  assert.deepEqual(received, [0, 1, 2, 3, 4, 5, 6, 7]);
});

test('same seed reproduces delays and reordering; sends copy the payload', () => {
  const run = () => {
    const link = new NetworkSimulator({ seed: 44, rtt: 50, jitter: 30, reorder: .5 }), received = [];
    link.receive('server', (data, metadata) => received.push([data.n, metadata.now]));
    for (let n = 0; n < 20; n++) { const packet = { n }; link.send('client', packet); packet.n = -1; }
    link.advance(1000); return received;
  };
  const result = run(); assert.deepEqual(result, run()); assert.equal(result.length, 20);
  assert.ok(result.every(([n]) => n >= 0));
  assert.notDeepEqual(result.map(([n]) => n), Array.from({ length: 20 }, (_, n) => n));
});
