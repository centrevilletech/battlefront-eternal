// Bounds and allocation work in the BFE-UDP/1 connection: the bytes on the
// wire stay exactly what they were, loss samples stay bounded, and timers
// re-arm lazily without changing when sessions time out.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import dgram from 'node:dgram';
import { PACKET } from '../src/net/codec.js';
import { Aead } from '../src/net/crypto.js';
import { CLOSE_CODE, Connection } from '../src/net/connection.js';
import { UdpEndpoint } from '../src/net/endpoint.js';
import { Simulation, connectedPair } from './net-sim.js';
import { identity } from './net-udp-fixtures.js';

// Two sessions with fixed keys, a manual clock, no timers and microtasks run
// by hand: every packet either end seals is a function of the inputs alone.
function fixedPair() {
  let time = 1000;
  const tasks = [], wire = [], inbox = { client: [], server: [] };
  const common = { now: () => time, setTimer: () => null, clearTimer: () => {}, schedule: fn => tasks.push(fn), initialRtt: 50 };
  const make = (role, local, remote, sendKey, receiveKey) => new Connection({ role, localSession: local, remoteSession: remote,
    send: new Aead(Buffer.alloc(32, sendKey)), receive: new Aead(Buffer.alloc(32, receiveKey)), ...common, path: { key: role === 'client' ? 'server' : 'client' },
    transmit: packet => { wire.push(Buffer.from(packet)); inbox[role === 'client' ? 'server' : 'client'].push(Buffer.from(packet)); } });
  const client = make('client', 1, 2, 3, 4), server = make('server', 2, 1, 4, 3);
  const run = () => { while (tasks.length) tasks.shift()(); };
  const deliver = (to, keep = () => true) => { const packets = inbox[to].splice(0); packets.forEach((packet, index) => { if (keep(index)) (to === 'client' ? client : server).receive(packet, { key: to === 'client' ? 'server' : 'client' }); }); run(); };
  return { client, server, wire, run, deliver, advance: ms => { time += ms; } };
}

test('sealed packets for fixed inputs are byte-identical to the reference transport', () => {
  const { client, server, wire, run, deliver, advance } = fixedPair();
  const bytes = (length, seed) => Buffer.from(Array.from({ length }, (_, i) => (i * 31 + seed) & 0xff));
  client.send('state', bytes(40, 1)); client.send('state', 'text state'); client.send('state', bytes(900, 2)); client.send('state', bytes(500, 3));
  client.send('events', bytes(30, 4)); client.send('events', 'event text'); client.send('bulk', bytes(5000, 5));
  client.send('state', bytes(9000, 6));
  run();
  advance(12);
  deliver('server', index => index % 3 !== 1);
  server.send('state', bytes(700, 7)); server.send('events', bytes(64, 8)); server.ping();
  run();
  advance(30);
  deliver('client');
  for (let i = 0; i < 6; i++) client.send('state', bytes(300 + i, 9 + i));
  client.send('events', bytes(2000, 20));
  run();
  advance(15);
  deliver('server');
  advance(40);
  deliver('client');
  advance(5);
  deliver('server');
  server.flush(); client.flush();
  client.close(CLOSE_CODE.NORMAL, 'done');
  const digest = createHash('sha256');
  for (const packet of wire) digest.update(packet);
  assert.ok(client.counters.retransmittedBytes > 0 && client.counters.datagramPacketsLost > 0);
  assert.equal(wire.length, 23);
  assert.equal(digest.digest('hex'), 'c24ee3458ff756c17517927f72f043e5a49b574449111c0766fe9d869cce1f29');
});

test('recent loss is a bounded one-second measure, however many packets are sampled', () => {
  const sim = new Simulation();
  const { client, server } = connectedPair(sim, { up: { delay: 20 }, down: { delay: 20, loss: 0.25, seed: 5 } });
  // About 2,000 state packets a second for six seconds.
  for (let i = 0; i < 12000; i++) sim.at(i / 2, () => server.send('state', Buffer.alloc(100)));
  for (let i = 0; i < 600; i++) sim.at(i * 10, () => client.send('state', Buffer.alloc(20)));
  sim.run(6000);
  const loss = server.stats().loss, sampled = server.counters.datagramPacketsLost + server.counters.datagramPacketsAcked;
  assert.ok(sampled > 10000, `sampled ${sampled}`);
  assert.ok(loss > 0.18 && loss < 0.32, `loss ${loss}`);
  assert.equal(server.lossBuckets.total.length, 21, 'fixed storage');
  assert.equal(server.lossSamples, undefined);
  // Samples older than the window no longer count.
  sim.run(sim.time + 1100);
  assert.equal(server.stats().loss, 0);
  assert.ok(server.stats().lossTotal > 0.18);
});

test('timers re-arm only for an earlier deadline, in whole milliseconds, and silence still times out on time', () => {
  const sim = new Simulation();
  let armed = 0, cleared = 0;
  const delays = [];
  const counting = {
    setTimer: (fn, ms) => { armed++; delays.push(ms); return sim.setTimer(fn, ms); },
    clearTimer: id => { cleared++; sim.clearTimer(id); },
  };
  const { client, server, closed, toServer, toClient } = connectedPair(sim, { client: { idleTimeoutMs: 3000, ...counting }, server: { idleTimeoutMs: 3000 } });
  for (let i = 0; i < 2000; i++) sim.at(i * 2, () => { server.send('state', Buffer.alloc(200)); client.send('state', Buffer.alloc(30)); });
  sim.run(4000);
  const received = client.counters.packetsReceived + client.counters.packetsSent;
  assert.ok(received > 3000, `packets ${received}`);
  assert.ok(armed < received / 4, `timers armed ${armed} for ${received} packets`);
  assert.ok(cleared < received / 4, `timers cleared ${cleared}`);
  assert.ok(delays.every(ms => Number.isInteger(ms)), 'whole-millisecond delays');
  // The peer falls silent: the idle close lands one idle timeout after the
  // last packet heard, not later because of a lazily kept timer.
  toServer.blocked = toClient.blocked = true;
  sim.until(() => closed.client, 10000);
  const lastHeard = client.lastReceivedAt;
  assert.equal(closed.client.code, CLOSE_CODE.IDLE);
  assert.ok(sim.time - lastHeard >= 3000 && sim.time - lastHeard < 3003, `closed ${sim.time - lastHeard} ms after the last packet`);
});

test('a listening endpoint reports socket buffers below the request and reuses source paths', async t => {
  const warnings = [];
  t.mock.method(console, 'warn', message => warnings.push(message));
  const endpoint = new UdpEndpoint({ identity: identity(), server: { hello: () => ({}), admit: () => ({ accept: true }) } });
  t.after(() => endpoint.close());
  // Far above any default net.core.rmem_max: the kernel grants less.
  await endpoint.listen({ host: '127.0.0.1', receiveBuffer: 1 << 30, sendBuffer: 1 << 30 });
  const granted = endpoint.stats().socketBuffers['IPv4 receive'];
  assert.ok(granted > 0 && granted < 1 << 30);
  assert.ok(warnings.some(message => message.includes('IPv4 receive buffer') && message.includes('rmem_max')), warnings.join('\n'));
  const seen = [];
  endpoint.receive = (packet, path) => seen.push(path);
  const sender = dgram.createSocket('udp4');
  t.after(() => sender.close());
  await new Promise(resolve => sender.bind(0, '127.0.0.1', resolve));
  for (let i = 0; i < 3; i++) await new Promise(resolve => sender.send(Buffer.of(PACKET.PUNCH), endpoint.port, '127.0.0.1', resolve));
  const deadline = Date.now() + 2000;
  while (seen.length < 3 && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 5));
  assert.equal(seen.length, 3);
  assert.ok(seen[0] === seen[1] && seen[1] === seen[2], 'one path object per source');
  assert.equal(seen[0].key, `direct|${endpoint.port}|127.0.0.1|${sender.address().port}`);
});
