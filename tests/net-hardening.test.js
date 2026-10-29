// Regression tests for the native UDP transport's security review
// (docs/native-udp-security-review.md): each attack the review demonstrated,
// and the bound that now holds.
import test from 'node:test';
import assert from 'node:assert/strict';
import { generateKeyPairSync, randomBytes } from 'node:crypto';
import { DATA_HEADER, FRAME, PACKET, RangeSet, writeVarint, varintSize, canonicalAddress } from '../src/net/codec.js';
import { Aead, identityKeys } from '../src/net/crypto.js';
import { CLOSE_CODE, Connection, ReplayWindow, encodeAck } from '../src/net/connection.js';
import { UdpEndpoint, addressPrefix, directPath } from '../src/net/endpoint.js';
import { ATTR, CLASS, METHOD, address, decodeMessage, encodeMessage, newTransaction } from '../src/net/stun.js';
import { TurnClient } from '../src/net/turn.js';
import { createNetService, onlineCandidates } from '../electron/net-service.js';
import { Simulation, connectedPair, seeded } from './net-sim.js';

const varint = value => { const out = Buffer.alloc(varintSize(value)); writeVarint(out, 0, value); return out; };
const streamFrame = (id, offset, bytes) => Buffer.concat([Buffer.of(FRAME.STREAM, id), varint(offset), varint(bytes.length), bytes]);
const message = payload => Buffer.concat([varint(payload.length), Buffer.of(0), payload]);
const ackOf = numbers => { const set = new RangeSet(); for (const number of numbers) set.add(number); return encodeAck(set, 0); };
const rawKeys = () => { const pair = generateKeyPairSync('ed25519'); return { publicKey: pair.publicKey.export({ format: 'der', type: 'spki' }).toString('base64'), privateKey: pair.privateKey.export({ format: 'pem', type: 'pkcs8' }) }; };
const identity = () => identityKeys(rawKeys());

// A connection under attack by its own authenticated peer: the test holds
// the peer's sending key and seals whatever frames it likes.
function victim(sim, options = {}) {
  const inbound = randomBytes(32), outbound = randomBytes(32), sent = [], received = [], closed = [];
  const connection = new Connection({ role: 'server', localSession: 2, remoteSession: 1, send: new Aead(outbound), receive: new Aead(inbound),
    now: sim.now, setTimer: sim.setTimer, clearTimer: sim.clearTimer, schedule: sim.schedule,
    transmit: (packet, path) => sent.push({ packet: Buffer.from(packet), path: path?.key }),
    onMessage: (lane, payload) => received.push([lane, payload]), onClose: info => closed.push(info), path: { key: 'peer' }, ...options });
  const peer = new Aead(inbound);
  let number = 0;
  const inject = (frames, path = { key: 'peer' }) => {
    const header = Buffer.alloc(DATA_HEADER); header[0] = PACKET.DATA; header.writeUInt32LE(2, 1); header.writeUInt32LE(number, 5);
    return connection.receive(peer.seal(header, number++, Buffer.concat(frames)), path);
  };
  return { connection, inject, sent, received, closed };
}

test('stream reassembly is exact under any arrival order, overlap and duplication', () => {
  const random = seeded(7);
  for (let round = 0; round < 40; round++) {
    const sim = new Simulation(), { inject, received, closed } = victim(sim);
    const payloads = Array.from({ length: 1 + Math.floor(random() * 6) }, () => randomBytes(1 + Math.floor(random() * 3000)));
    const stream = Buffer.concat(payloads.map(message)), pieces = [];
    for (let offset = 0; offset < stream.length;) {
      const length = 1 + Math.floor(random() * 700), start = Math.max(0, offset - Math.floor(random() * 50));
      pieces.push([start, stream.subarray(start, Math.min(stream.length, offset + length))]);
      offset += length;
    }
    for (let i = pieces.length / 3; i > 0; i--) pieces.push(pieces[Math.floor(random() * pieces.length)]);
    for (let i = pieces.length - 1; i > 0; i--) { const j = Math.floor(random() * (i + 1)); [pieces[i], pieces[j]] = [pieces[j], pieces[i]]; }
    for (const [start, bytes] of pieces) inject([streamFrame(1, start, bytes)]);
    assert.equal(closed.length, 0);
    assert.deepEqual(received.map(([, payload]) => payload.toString('hex')), payloads.map(payload => payload.toString('hex')), `round ${round}`);
  }
});

test('scattered tiny stream frames hit the reassembly limit after a few packets', () => {
  const sim = new Simulation(), { inject, closed } = victim(sim);
  let packets = 0;
  for (let offset = 2; !closed.length && packets < 100; packets++) {
    const frames = [];
    for (let i = 0; i < 150; i++, offset += 2) frames.push(streamFrame(1, offset, Buffer.of(1)));
    inject(frames);
  }
  assert.equal(closed[0]?.code, CLOSE_CODE.PROTOCOL);
  assert.match(closed[0].reason, /reassembly limit/);
  assert.ok(packets <= 8, `closed after ${packets} packets`);
});

test('contiguous bytes behind a gap merge into one run, and tiny pieces are bounded too', () => {
  const sim = new Simulation(), { inject, closed, connection } = victim(sim);
  const stream = connection.receiveStreams.get(1);
  for (let offset = 1, packets = 0; !closed.length && packets < 100; packets++) {
    const frames = [];
    for (let i = 0; i < 150; i++, offset++) frames.push(streamFrame(1, offset, Buffer.of(1)));
    inject(frames);
    if (!closed.length) assert.equal(stream.segments.length, 1);
  }
  assert.match(closed[0]?.reason ?? '', /reassembly limit/);
});

test('stream bytes far beyond the delivery point are refused', () => {
  const sim = new Simulation(), { inject, closed } = victim(sim);
  inject([streamFrame(2, 5 << 20, Buffer.of(1))]);
  assert.match(closed[0]?.reason ?? '', /stream buffer exceeded/);
});

test('a peer that stays alive but never acknowledges is closed, not buffered without bound', () => {
  const sim = new Simulation(), { connection, inject, closed } = victim(sim);
  const chunk = Buffer.alloc(30000);
  let accepted = 0;
  for (let i = 0; i < 400 && !closed.length; i++) {
    if (connection.send('bulk', chunk)) accepted++;
    sim.run(sim.time + 50);
    inject([Buffer.of(FRAME.PING)]);
  }
  assert.equal(closed[0]?.reason, 'reliable data not acknowledged');
  assert.ok(accepted * chunk.length <= 4 << 20);
  assert.equal(connection.send('bulk', chunk), false);
});

test('acknowledgements are one per packet, bounded in ranges, and only for packets sent', () => {
  const tooMany = () => { const fields = [200, 0, 64, 0]; for (let i = 0; i < 64; i++) fields.push(0, 0); return Buffer.concat([Buffer.of(FRAME.ACK), ...fields.map(varint)]); };
  for (const [name, frames, pattern] of [
    ['repeated', [ackOf([0]), ackOf([0])], /repeated ack/],
    ['unsent', [ackOf([50])], /unsent/],
    ['ranges', [tooMany()], /invalid ack/],
  ]) {
    const sim = new Simulation(), { connection, inject, closed } = victim(sim);
    for (let i = 0; i < 3; i++) { connection.send('state', Buffer.alloc(10)); sim.drain(); }
    inject(frames);
    assert.equal(closed[0]?.code, CLOSE_CODE.PROTOCOL, name);
    assert.match(closed[0].reason, pattern, name);
  }
});

test('acknowledgement work stays linear in packets in flight', () => {
  const sim = new Simulation(), { connection, inject, closed } = victim(sim);
  for (let i = 0; i < 4200; i++) { connection.send('state', Buffer.alloc(10)); sim.drain(); }
  assert.equal(connection.sentPackets.size, 4096);
  const top = connection.nextNumber - 1, missing = [];
  for (let i = 0; i < 24; i++) missing.push(top - i * 3);
  const begun = performance.now();
  for (let i = 0; i < 200; i++) inject([ackOf(missing)]);
  const elapsed = performance.now() - begun;
  assert.equal(closed.length, 0);
  assert.ok(elapsed < 1500, `200 acknowledgements took ${elapsed.toFixed(0)} ms`);
  assert.ok(!connection.sentPackets.has(top) && connection.sentPackets.size < 4096 - 23);
});

test('a packet full of path challenges earns a bounded answer', () => {
  const sim = new Simulation(), { inject, sent } = victim(sim);
  inject(Array.from({ length: 100 }, (_, i) => Buffer.concat([Buffer.of(FRAME.PATH_CHALLENGE), Buffer.alloc(8, i)])));
  const bytes = sent.reduce((total, item) => total + item.packet.length, 0);
  assert.ok(bytes > 0 && bytes < 400, `${bytes} bytes answered 100 challenges`);
});

test('a spoofed source earns one small challenge; the session never moves there', () => {
  const sim = new Simulation(), { client, server, received } = connectedPair(sim);
  const toVictim = [], deliver = server.transmit;
  server.transmit = (packet, path) => { if (path?.key === 'victim') { toVictim.push(packet.length); return; } deliver(packet, path); };
  sim.run(50);
  const original = client.transmit;
  client.transmit = packet => server.receive(Buffer.from(packet), { key: 'victim' });
  for (let i = 0; i < 5; i++) { client.send('events', 'spoofed ' + i); sim.drain(); }
  client.transmit = original;
  for (let i = 0; i < 30; i++) sim.at(sim.time + i * 33, () => { client.send('state', Buffer.alloc(8)); server.send('state', Buffer.alloc(300)); });
  sim.run(sim.time + 2000);
  assert.equal(server.path.key, 'client');
  assert.ok(toVictim.length === 1 && toVictim[0] < 64, `victim received ${JSON.stringify(toVictim)}`);
  assert.ok(received.client.filter(([lane]) => lane === 'state').length >= 25, 'the real address kept its traffic');
});

test('NAT rebinding moves the session after one challenge round trip', () => {
  const sim = new Simulation(), { client, server, received, toServer, toClient } = connectedPair(sim);
  sim.run(50);
  const moves = [];
  server.onPath = (path, previous) => moves.push([previous.key, path.key]);
  // The old mapping is gone: packets to it vanish.
  server.transmit = (packet, path) => { if (path?.key !== 'client') toClient.send(packet, bytes => client.receive(bytes, { key: 'server' })); };
  client.transmit = packet => toServer.send(packet, bytes => server.receive(bytes, { key: 'client-rebound' }));
  client.send('state', Buffer.alloc(8));
  sim.run(sim.time + 100);
  assert.deepEqual(moves, [['client', 'client-rebound']]);
  server.send('events', 'after rebinding');
  sim.run(sim.time + 100);
  assert.ok(received.client.some(([, value]) => value === 'after rebinding'));
});

test('the replay window matches a reference model across jumps and its trailing edge', () => {
  const random = seeded(3), window = new ReplayWindow(256), marked = new Set();
  let largest = -1;
  for (let i = 0; i < 20000; i++) {
    const jump = random() < 0.1 ? Math.floor(random() * 600) : Math.floor(random() * 40) - 20;
    const number = Math.max(0, Math.max(largest, 0) + jump);
    const expected = number <= largest - 256 || marked.has(number);
    assert.equal(window.seen(number), expected, `number ${number}, largest ${largest}`);
    if (!expected) { window.mark(number); marked.add(number); largest = Math.max(largest, number); }
  }
});

test('handshake rate limits key on networks and never lock out newcomers', () => {
  const endpoint = new UdpEndpoint({ identity: identity(), server: { hello: () => ({}), admit: () => ({ accept: true }) } });
  endpoint.transmit = () => {};
  const path = (address, port = 5000) => ({ key: `direct|${address}|${port}`, kind: 'direct', family: address.includes(':') ? 'IPv6' : 'IPv4', address, port });
  for (let i = 0; i < 5000; i++) endpoint.receive(Buffer.of(PACKET.INITIATION), path(`10.${i >> 16 & 255}.${i >> 8 & 255}.${i & 255}`));
  assert.equal(endpoint.buckets.size, 0, 'a packet too short to be a handshake creates no state');
  for (let i = 0; i < 5000; i++) assert.equal(endpoint.allowHandshake(path(`10.0.${i >> 8 & 255}.${i & 255}`)), true);
  assert.equal(endpoint.buckets.size, 4096);
  assert.equal(endpoint.allowHandshake(path('192.0.2.77')), true, 'a newcomer still gets a bucket');
  for (let i = 0; i < 25; i++) endpoint.allowHandshake(path(`2001:db8:1:2::${i.toString(16)}`, 6000 + i));
  assert.equal(endpoint.allowHandshake(path('2001:db8:1:2:ffff::1')), false, 'one /64 shares one bucket');
  assert.ok(endpoint.counters.rateLimited > 0);
  endpoint.close();
});

test('address prefixes and canonical candidates', () => {
  assert.equal(addressPrefix('203.0.113.9'), '203.0.113.9');
  assert.equal(addressPrefix('::ffff:203.0.113.9'), '203.0.113.9');
  assert.equal(addressPrefix('2001:db8:1:2::5'), '2001:db8:1:2::/64');
  assert.equal(addressPrefix('2001:DB8:1:2:3:4:5:6'), '2001:db8:1:2::/64');
  assert.equal(addressPrefix('2001:db8::1'), '2001:db8:0:0::/64');
  assert.equal(addressPrefix('::1'), '0:0:0:0::/64');
  assert.equal(canonicalAddress('2001:DB8:0:0:0:0:0:1'), '2001:db8::1');
  assert.equal(canonicalAddress('::FFFF:198.51.100.4'), '198.51.100.4');
  assert.equal(canonicalAddress('fe80::1%eth0'), 'fe80::1%eth0');
});

test('sessions per network are capped, loopback and reconnects exempt', () => {
  const endpoint = new UdpEndpoint({ identity: identity(), maxSessionsPerPrefix: 2, server: { hello: () => ({}), admit: () => ({ accept: true }) } });
  const fake = (id, prefix) => ({ id, prefix, role: 'server', closed: false, close() {} });
  endpoint.sessions.set(1, fake('a', '198.51.100.4')); endpoint.sessions.set(2, fake('b', '198.51.100.4'));
  const from = address => ({ key: `direct|${address}|1`, address });
  assert.equal(endpoint.crowded(from('198.51.100.4'), 'c'), true);
  assert.equal(endpoint.crowded(from('198.51.100.4'), 'a'), false, 'a reconnecting identity is not counted against itself');
  assert.equal(endpoint.crowded(from('198.51.100.5'), 'c'), false);
  endpoint.sessions.set(3, fake('d', '127.0.0.1')); endpoint.sessions.set(4, fake('e', '127.0.0.1'));
  assert.equal(endpoint.crowded(from('127.0.0.1'), 'f'), false);
  endpoint.close();
});

test('a connect answers only the paths it tried, and a bounded number of retries each', async t => {
  const server = identity(), client = new UdpEndpoint({ identity: identity() });
  await client.listen({ host: '127.0.0.1' });
  t.after(() => client.close());
  const sent = [];
  client.transmit = (packet, path) => sent.push({ type: packet[0], key: path.key, token: packet.readUInt16LE(54) });
  const attempt = client.connect({ serverId: server.id, candidates: [{ address: '127.0.0.1', port: 9 }, { address: '127.0.0.1', port: 10 }], timeoutMs: 400 });
  const session = [...client.connecting.keys()][0];
  const retry = Buffer.alloc(12); retry[0] = PACKET.RETRY; retry[1] = 1; retry.writeUInt32LE(session, 2); retry.writeUInt16LE(4, 6);
  const at = (address, port) => directPath(client.sockets.get('IPv4'), 'IPv4', address, port);
  for (let i = 0; i < 20; i++) client.receive(retry, at('192.0.2.200', 4000 + i));
  assert.equal(sent.filter(item => item.key.includes('192.0.2.200')).length, 0, 'a reply from an address the client never tried is ignored');
  assert.equal(client.counters.strayReplies, 20);
  for (let i = 0; i < 10; i++) client.receive(retry, at('127.0.0.1', 9));
  assert.equal(sent.filter(item => item.key === at('127.0.0.1', 9).key && item.token === 4).length, 2, 'two retries per path at most');
  const version = Buffer.alloc(6); version[0] = PACKET.VERSION; version[1] = 7; version.writeUInt32LE(session, 2);
  client.receive(version, at('127.0.0.1', 9));
  assert.equal(client.connecting.size, 1, 'one path answering with another version does not end the attempt');
  client.receive(version, at('127.0.0.1', 10));
  await assert.rejects(attempt, /Update required/);
});

test('malformed relay addresses read as null and never throw into the socket', () => {
  const transaction = newTransaction();
  const packet = encodeMessage(METHOD.DATA, CLASS.INDICATION, transaction, [[ATTR.XOR_PEER_ADDRESS, Buffer.alloc(3)], [ATTR.DATA, Buffer.from('x')]]);
  assert.equal(address(decodeMessage(packet), ATTR.XOR_PEER_ADDRESS), null);
  const delivered = [];
  const turn = new TurnClient({ send: () => {}, server: { address: '127.0.0.1', port: 3478 }, username: 'u', password: 'p', onData: (...args) => delivered.push(args) });
  assert.doesNotThrow(() => turn.handle(packet));
  assert.equal(delivered.length, 0);
  turn.close();
});

// A battle port as the network process sees it, driven by a test page.
function channel() {
  const out = [];
  let handler = null, closeHandler = null;
  return { out, send: message => handler?.(message), close: () => closeHandler?.(),
    channel: { post: message => out.push(message), onMessage: fn => { handler = fn; }, onClose: fn => { closeHandler = fn; }, close() {} } };
}
const settle = () => new Promise(resolve => setTimeout(resolve, 20));

test('the network process bounds what a page can make it send', async t => {
  let now = 0;
  const service = createNetService({ identity: rawKeys(), listen: { port: 0, host: '127.0.0.1' }, now: () => now });
  t.after(() => service.close());
  const connect = port => port.send({ op: 'connect', serverId: 'a'.repeat(64), candidates: [{ address: '127.0.0.1', port: 9 }], hello: {}, timeoutMs: 2000 });
  const open = Array.from({ length: 5 }, channel);
  for (const port of open) service.serve(port.channel);
  assert.match(open[4].out[0]?.message ?? '', /Too many open network sessions/);
  connect(open[0]); await settle(); connect(open[0]); await settle();
  assert.match(open[0].out.at(-1)?.message ?? '', /already open/, 'one connect per battle port');
  open[0].send({ op: 'gather' }); await settle(); open[0].send({ op: 'gather' }); await settle();
  assert.match(open[0].out.at(-1)?.message ?? '', /Too many connection attempts/, 'one gather per battle port');
  for (const port of open.slice(0, 4)) port.close();
  const refused = [];
  for (let i = 0; i < 8; i++) {
    const port = channel(); service.serve(port.channel); connect(port); await settle();
    if (/Too many connection attempts/.test(port.out.at(-1)?.message ?? '')) refused.push(i);
    port.close();
  }
  assert.deepEqual(refused, [5, 6, 7], 'a burst of six connects, then a wait');
  now += 4000;
  const later = channel(); service.serve(later.channel); connect(later); await settle();
  assert.doesNotMatch(later.out.at(-1)?.message ?? '', /Too many/);
  later.close();
  assert.equal(service.endpoint.connecting.size, 0, 'closing a battle port cancels its connect');
});

test('online candidates from a server never point at this machine, and few at its network', () => {
  const list = ['127.0.0.1', '0.0.0.0', '169.254.169.254', '::1', 'fe80::1', '10.0.0.2', '192.168.1.4', 'fd00::5', '203.0.113.9', '2001:db8::7'].map((address, i) => ({ address, port: 18451 + i }));
  assert.deepEqual(onlineCandidates(list).map(candidate => candidate.address), ['10.0.0.2', '192.168.1.4', '203.0.113.9', '2001:db8::7']);
  const state = { private: 0 };
  onlineCandidates([{ address: '10.0.0.2', port: 1 }], state);
  assert.deepEqual(onlineCandidates([{ address: '10.0.0.3', port: 1 }, { address: '10.0.0.4', port: 1 }], state).map(candidate => candidate.address), ['10.0.0.3'], 'trickled candidates share the limit');
});
