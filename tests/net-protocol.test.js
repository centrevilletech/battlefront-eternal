import test from 'node:test';
import assert from 'node:assert/strict';
import { generateKeyPairSync, randomBytes } from 'node:crypto';
import { RangeSet, readVarint, writeVarint, varintSize, PACKET, MAX_DATAGRAM } from '../src/net/codec.js';
import { Aead, RetryTokens, identityKeys, publicIdentity, weakEd25519 } from '../src/net/crypto.js';
import { ClientHandshake, ServerHandshakes, INITIATION_BYTES } from '../src/net/handshake.js';
import { decodeAck, encodeAck, CLOSE_CODE, MAX_STATE_MESSAGE } from '../src/net/connection.js';
import { Simulation, connectedPair, seeded } from './net-sim.js';

const identity = () => {
  const pair = generateKeyPairSync('ed25519');
  return identityKeys({ publicKey: pair.publicKey.export({ format: 'der', type: 'spki' }).toString('base64'), privateKey: pair.privateKey.export({ format: 'pem', type: 'pkcs8' }) });
};

test('varints round-trip at every width boundary', () => {
  for (const value of [0, 63, 64, 16383, 16384, 1073741823, 1073741824, 2 ** 40 + 5, Number.MAX_SAFE_INTEGER]) {
    const buffer = Buffer.alloc(9), end = writeVarint(buffer, 0, value);
    assert.equal(end, varintSize(value));
    assert.deepEqual(readVarint(buffer, 0), [value, end]);
  }
  assert.throws(() => readVarint(Buffer.of(0x80, 1), 0), /truncated/);
});

test('range sets merge, subtract and bound their history', () => {
  const set = new RangeSet();
  for (const n of [5, 1, 2, 3, 9, 7, 8]) set.add(n);
  assert.deepEqual(set.ranges, [[1, 4], [5, 6], [7, 10]]);
  set.add(4); assert.deepEqual(set.ranges, [[1, 6], [7, 10]]);
  assert.ok(set.has(5) && !set.has(6) && set.covers(7, 10) && !set.covers(5, 8));
  assert.deepEqual(set.missing(0, 12), [[0, 1], [6, 7], [10, 12]]);
  set.remove(2, 8); assert.deepEqual(set.ranges, [[1, 2], [8, 10]]);
  set.trimBelow(9); assert.deepEqual(set.ranges, [[9, 10]]);
});

test('acknowledgement frames carry ranges and delay exactly', () => {
  const received = new RangeSet();
  for (const [a, b] of [[0, 10], [12, 13], [15, 40], [41, 42], [100, 101]]) received.add(a, b);
  const frame = encodeAck(received, 12345);
  const { ack, offset } = decodeAck(frame, 1);
  assert.equal(offset, frame.length);
  assert.equal(ack.largest, 100);
  assert.equal(Math.abs(ack.delay - 12345) <= 8, true);
  assert.deepEqual(ack.ranges, [[100, 101], [41, 42], [15, 40], [12, 13], [0, 10]]);
});

test('AEAD rejects tampering, wrong keys and wrong packet numbers', () => {
  const secret = randomBytes(32), aead = new Aead(secret), header = Buffer.from('123456789');
  const sealed = aead.seal(header, 7, Buffer.from('payload'));
  assert.equal(new Aead(secret).open(sealed, 9, 7).toString(), 'payload');
  assert.equal(new Aead(secret).open(sealed, 9, 8), null);
  assert.equal(new Aead(randomBytes(32)).open(sealed, 9, 7), null);
  for (const index of [0, 9, sealed.length - 1]) { const copy = Buffer.from(sealed); copy[index] ^= 1; assert.equal(new Aead(secret).open(copy, 9, 7), null); }
  const next = aead.next();
  assert.equal(next.open(next.seal(header, 1, Buffer.from('x')), 9, 1).toString(), 'x');
  assert.equal(aead.open(next.seal(header, 1, Buffer.from('x')), 9, 1), null, 'key update derives an independent key');
});

test('handshake authenticates both identities and agrees on traffic keys', () => {
  const server = identity(), client = identity();
  const handshakes = new ServerHandshakes({ identity: server, hello: () => ({ protocol: 3, content: 'abc' }), retry: new RetryTokens() });
  const handshake = new ClientHandshake({ identity: client, hello: { protocol: 3, name: 'Josh' }, expectedServerId: server.id });
  const initiation = handshake.initiation();
  assert.equal(initiation.length, INITIATION_BYTES);
  const response = handshakes.initiation(initiation, 'peer');
  assert.ok(response.length < initiation.length, 'a reply never amplifies the request');
  const established = handshake.receive(response);
  assert.equal(established.type, 'response');
  assert.deepEqual(established.server, { id: server.id, publicKey: server.publicKey, hello: { protocol: 3, content: 'abc' } });
  assert.equal(handshakes.confirm(established.confirm, 'victim'), null, 'a CONFIRM from another address opens nothing');
  const session = handshakes.confirm(established.confirm, 'peer');
  assert.equal(session.client.id, client.id);
  assert.deepEqual(session.client.hello, { protocol: 3, name: 'Josh' });
  assert.equal(session.serverSession, established.serverSession);
  assert.equal(session.clientSession, established.clientSession);
  const header = Buffer.alloc(9);
  assert.equal(session.receive.open(established.send.seal(header, 3, Buffer.from('up')), 9, 3).toString(), 'up');
  assert.equal(established.receive.open(session.send.seal(header, 4, Buffer.from('down')), 9, 4).toString(), 'down');
  assert.equal(handshakes.confirm(established.confirm, 'peer'), null, 'a completed handshake cannot be confirmed again');
});

test('handshake duplicates replay cached replies without new key agreement', () => {
  const server = identity(), client = identity();
  const handshakes = new ServerHandshakes({ identity: server, retry: new RetryTokens() });
  const handshake = new ClientHandshake({ identity: client, hello: {}, expectedServerId: server.id });
  const initiation = handshake.initiation();
  const first = handshakes.initiation(initiation, 'peer'), second = handshakes.initiation(initiation, 'peer');
  assert.ok(first.equals(second));
  assert.equal(handshakes.pending.size, 1);
  const established = handshake.receive(first);
  assert.deepEqual(handshake.receive(second), { type: 'duplicate', confirm: established.confirm });
});

test('handshake rejects impostors, tampering, short initiations and unknown versions', () => {
  const server = identity(), impostor = identity(), client = identity();
  const fake = new ServerHandshakes({ identity: impostor, retry: new RetryTokens() });
  const handshake = new ClientHandshake({ identity: client, hello: {}, expectedServerId: server.id });
  const result = handshake.receive(fake.initiation(handshake.initiation(), 'peer'));
  assert.equal(result.type, 'error');
  assert.match(result.error.message, /does not match/);
  const real = new ServerHandshakes({ identity: server, retry: new RetryTokens() });
  const other = new ClientHandshake({ identity: client, hello: {}, expectedServerId: server.id });
  const response = real.initiation(other.initiation(), 'peer');
  const tampered = Buffer.from(response); tampered[response.length - 20] ^= 1;
  assert.equal(other.receive(tampered), null);
  const established = other.receive(response);
  const badConfirm = Buffer.from(established.confirm); badConfirm[20] ^= 1;
  assert.equal(real.confirm(badConfirm, 'peer'), null);
  assert.ok(real.confirm(established.confirm, 'peer'));
  assert.equal(real.initiation(new ClientHandshake({ identity: client, hello: {}, expectedServerId: server.id }).initiation().subarray(0, 600), 'peer'), null, 'short initiations are not answered');
  const future = Buffer.from(new ClientHandshake({ identity: client, hello: {}, expectedServerId: server.id }).initiation()); future[1] = 9;
  const version = real.initiation(future, 'peer');
  assert.equal(version[0], PACKET.VERSION);
  assert.ok(version.length < 16);
  const waiting = new ClientHandshake({ identity: client, hello: {}, expectedServerId: server.id });
  const own = Buffer.from(version); own[1] = 1; own.writeUInt32LE(waiting.clientSession, 2);
  assert.equal(waiting.receive(own), null, 'a VERSION naming our own version is ignored');
  const newer = Buffer.from(own); newer[1] = 9;
  assert.deepEqual(waiting.receive(newer), { type: 'version', version: 9 });
});

test('retry tokens are required under load and bound to the source address', () => {
  const server = identity(), client = identity();
  const handshakes = new ServerHandshakes({ identity: server, retry: new RetryTokens() });
  const handshake = new ClientHandshake({ identity: client, hello: {}, expectedServerId: server.id });
  const retry = handshakes.initiation(handshake.initiation(), '192.0.2.1|5000', { requireRetry: true });
  assert.equal(retry[0], PACKET.RETRY);
  assert.equal(handshakes.pending.size, 0, 'a retry keeps no state');
  const { type, token } = handshake.receive(retry);
  assert.equal(type, 'retry');
  const again = handshake.initiation(token);
  assert.equal(handshakes.initiation(again, '192.0.2.2|5000', { requireRetry: true })[0], PACKET.RETRY, 'another address cannot use the token');
  const response = handshakes.initiation(again, '192.0.2.1|5000', { requireRetry: true });
  assert.equal(response[0], PACKET.RESPONSE);
  assert.ok(handshakes.confirm(handshake.receive(response).confirm, '192.0.2.1|5000'));
});

test('a RESPONSE stays verifiable whichever initiation it answers (a spoofed RETRY cannot stall a join)', () => {
  const server = identity(), client = identity();
  const handshakes = new ServerHandshakes({ identity: server, retry: new RetryTokens() });
  const handshake = new ClientHandshake({ identity: client, hello: {}, expectedServerId: server.id });
  const response = handshakes.initiation(handshake.initiation(), 'peer');
  const spoofed = Buffer.alloc(12); spoofed[0] = PACKET.RETRY; spoofed[1] = 1; spoofed.writeUInt32LE(handshake.clientSession, 2); spoofed.writeUInt16LE(4, 6);
  const retry = handshake.receive(spoofed);
  assert.equal(retry.type, 'retry');
  assert.ok(handshakes.initiation(handshake.initiation(retry.token), 'peer').equals(response), 'the server answers the tokened duplicate from its cache');
  const established = handshake.receive(response);
  assert.equal(established.type, 'response');
  assert.ok(handshakes.confirm(established.confirm, 'peer'));
});

test('weak Ed25519 identities (small order, non-canonical) are refused', () => {
  const p = 2n ** 255n - 19n, encode = (y, sign = 0) => { const raw = Buffer.alloc(32); for (let i = 0; i < 32; i++) raw[i] = Number((y >> BigInt(8 * i)) & 0xffn); raw[31] |= sign << 7; return raw; };
  const spki = raw => Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), raw]);
  const y8 = 0x05fc536d880238b13933c6d305acdfd5f098eff289f4c345b027b2c28f95e826n;
  for (const y of [0n, 1n, p - 1n, y8, p - y8, p, p + 1n]) for (const sign of [0, 1]) {
    assert.equal(weakEd25519(encode(y, sign)), true, `y=${y} sign=${sign}`);
    assert.throws(() => publicIdentity(spki(encode(y, sign))), /invalid identity/);
  }
  const genuine = identity();
  assert.equal(weakEd25519(genuine.publicDer.subarray(12)), false);
  assert.equal(publicIdentity(genuine.publicDer).id, genuine.id);
});

test('half-open handshakes are bounded and expire', () => {
  let now = 0;
  const server = identity(), client = identity();
  const handshakes = new ServerHandshakes({ identity: server, retry: new RetryTokens(), now: () => now, maxPending: 3, maxPendingPerPrefix: 2, retryAbove: 100 });
  const sources = [];
  for (let i = 0; i < 5; i++) { sources.push('peer' + i); handshakes.initiation(new ClientHandshake({ identity: client, hello: {}, expectedServerId: server.id }).initiation(), 'peer' + i, { prefix: 'net' + i }); }
  assert.deepEqual([...handshakes.pending.values()].map(state => state.source), sources.slice(2), 'a full table drops its oldest half-open state, not newcomers');
  for (let i = 0; i < 3; i++) handshakes.initiation(new ClientHandshake({ identity: client, hello: {}, expectedServerId: server.id }).initiation(), 'busy' + i, { prefix: 'busy' });
  assert.equal([...handshakes.pending.values()].filter(state => state.prefix === 'busy').length, 2, 'one network holds a bounded share');
  now = 20000;
  handshakes.expire();
  assert.equal(handshakes.pending.size, 0);
  assert.equal(handshakes.prefixes.size, 0);
});

test('lanes deliver: state unreliable, events and bulk ordered and intact', () => {
  const sim = new Simulation(), { client, server, received } = connectedPair(sim);
  client.send('events', JSON.stringify({ type: 'hello' }));
  client.send('state', Buffer.from([1, 2, 3]));
  client.send('bulk', Buffer.alloc(5000, 7));
  client.send('state', 'text state');
  sim.run(100);
  const lanes = received.server.map(([lane, payload]) => [lane, typeof payload === 'string' ? payload : payload.length]);
  assert.deepEqual(lanes.filter(([lane]) => lane === 'state'), [['state', 3], ['state', 'text state']]);
  assert.deepEqual(lanes.filter(([lane]) => lane !== 'state'), [['events', '{"type":"hello"}'], ['bulk', 5000]]);
  assert.ok(Buffer.isBuffer(received.server.find(([lane]) => lane === 'bulk')[1]));
});

test('messages queued in one task coalesce into one packet', () => {
  const sim = new Simulation(), { client, toServer } = connectedPair(sim);
  sim.run(10);
  const before = toServer.sent;
  client.send('state', Buffer.alloc(700)); client.send('state', Buffer.alloc(300)); client.send('events', '{"type":"ack"}');
  sim.drain();
  assert.equal(toServer.sent - before, 1);
  client.send('state', Buffer.alloc(700)); client.send('state', Buffer.alloc(700));
  sim.drain();
  assert.equal(toServer.sent - before, 3, 'two packets when they cannot share one');
});

test('reliable streams survive loss, jitter, reordering and duplication in order', () => {
  const sim = new Simulation();
  const impairment = { loss: 0.1, delay: 30, jitter: 25, duplicate: 0.05 };
  const { client, server, received } = connectedPair(sim, { up: impairment, down: { ...impairment, seed: 99 } });
  const random = seeded(5), sent = [];
  for (let i = 0; i < 300; i++) {
    sim.at(i * 7, () => {
      const message = JSON.stringify({ i, pad: 'x'.repeat(Math.floor(random() * 2000)) });
      sent.push(message); client.send('events', message);
      client.send('state', Buffer.alloc(200));
      server.send('state', Buffer.alloc(900));
    });
  }
  const big = randomBytes(200000);
  client.send('bulk', big);
  assert.ok(sim.until(() => received.server.filter(([lane]) => lane === 'events').length === 300 && received.server.some(([lane]) => lane === 'bulk'), 30000));
  assert.deepEqual(received.server.filter(([lane]) => lane === 'events').map(([, value]) => value), sent);
  assert.ok(received.server.find(([lane]) => lane === 'bulk')[1].equals(big));
  const states = received.server.filter(([lane]) => lane === 'state').length;
  assert.ok(states < 300 * 1.06 && states > 300 * 0.75, 'state datagrams are neither retransmitted nor lost beyond the link');
  assert.ok(client.counters.retransmittedBytes > 0);
  assert.ok(server.counters.duplicates > 0, 'duplicated packets were discarded');
});

test('a lost event is resent from the next acknowledgement, not a timeout', () => {
  const sim = new Simulation();
  let dropFirstEvent = true;
  const { client, server, received } = connectedPair(sim, {
    up: { delay: 25 }, down: { delay: 25, drop: packet => packet.length > 200 && dropFirstEvent && !(dropFirstEvent = false) },
  });
  // Steady state traffic both ways, as in a match (30 Hz each way).
  for (let i = 0; i < 60; i++) sim.at(i * 33, () => { client.send('state', Buffer.alloc(60)); server.send('state', Buffer.alloc(100)); });
  sim.run(500);
  const sentAt = sim.time;
  server.send('events', 'x'.repeat(400));
  assert.ok(sim.until(() => received.client.some(([lane]) => lane === 'events'), 2000));
  const latency = received.client.find(([lane]) => lane === 'events')[2] - sentAt;
  // One-way 25 ms: the next state packets' acknowledgement (or the probe
  // timeout racing it) resends within about two round trips.
  assert.ok(latency < 50 + 33 + 50 + 30, `recovered in ${latency} ms (one-way 25 ms)`);
  assert.ok(server.counters.ptoProbes <= 1);
});

test('round-trip time converges and loss is measured', () => {
  const sim = new Simulation();
  const { client, server } = connectedPair(sim, { up: { delay: 40 }, down: { delay: 40, loss: 0.2, seed: 7 } });
  for (let i = 0; i < 300; i++) sim.at(i * 16, () => { client.send('state', Buffer.alloc(40)); server.send('state', Buffer.alloc(600)); });
  sim.run(5000);
  assert.ok(Math.abs(server.rtt - 80) < 6, `rtt ${server.rtt}`);
  assert.ok(server.stats().lossTotal > 0.12 && server.stats().lossTotal < 0.28, `loss ${server.stats().lossTotal}`);
  assert.equal(client.stats().lossTotal, 0);
});

test('large state messages fragment and arrive whole, or not at all', () => {
  const sim = new Simulation(), { client, server, received } = connectedPair(sim);
  const big = randomBytes(5000);
  client.send('state', big);
  sim.run(100);
  assert.ok(received.server[0][1].equals(big));
  assert.throws(() => client.send('state', Buffer.alloc(MAX_STATE_MESSAGE + 1)), /too large/);
  let drop = 2;
  const lossy = connectedPair(sim, { up: { drop: () => drop-- === 1 } });
  lossy.client.send('state', big);
  sim.run(sim.time + 200);
  assert.equal(lossy.received.server.length, 0, 'a lost fragment loses the whole message');
});

test('forged, replayed and foreign packets are dropped without effect', () => {
  const sim = new Simulation(), { client, server, received } = connectedPair(sim);
  let captured;
  const send = client.transmit;
  client.transmit = (packet, path) => { captured ??= Buffer.from(packet); send(packet, path); };
  client.send('state', Buffer.from('once'));
  sim.run(50);
  assert.equal(server.receive(captured, { key: 'client' }), false, 'replay');
  const forged = Buffer.from(captured); forged[12] ^= 0xff;
  forged.writeUInt32LE(forged.readUInt32LE(5) + 50, 5);
  assert.equal(server.receive(forged, { key: 'client' }), false, 'forgery');
  assert.equal(received.server.filter(([, value]) => String(value) === 'once').length, 1);
  assert.ok(server.counters.duplicates >= 1 && server.counters.authFailures >= 1);
});

test('close reaches the peer with its reason; silence times out', () => {
  const sim = new Simulation(), { client, server, closed } = connectedPair(sim);
  sim.run(20);
  server.close(CLOSE_CODE.VERSION, 'Update required');
  sim.run(100);
  assert.deepEqual(closed.client, { code: CLOSE_CODE.VERSION, reason: 'Update required', remote: true });
  assert.equal(closed.server.remote, false);
  assert.equal(client.send('state', Buffer.alloc(1)), false);
  const quiet = connectedPair(sim, { client: { idleTimeoutMs: 3000 }, server: { idleTimeoutMs: 3000 } });
  sim.run(sim.time + 10000);
  assert.equal(quiet.closed.client, undefined, 'keepalives hold an idle session open');
  quiet.toServer.blocked = quiet.toClient.blocked = true;
  sim.run(sim.time + 4000);
  assert.equal(quiet.closed.client?.code, CLOSE_CODE.IDLE);
  assert.equal(quiet.closed.server?.code, CLOSE_CODE.IDLE);
});

test('key updates rotate both directions without losing traffic', () => {
  const sim = new Simulation();
  const { client, server, received } = connectedPair(sim, { client: { keyUpdateEvery: 40 }, server: { keyUpdateEvery: 25 }, up: { jitter: 8, seed: 3 }, down: { jitter: 8, seed: 4 } });
  for (let i = 0; i < 400; i++) sim.at(i * 5, () => { client.send('events', String(i)); server.send('events', String(i)); });
  sim.run(10000);
  assert.deepEqual(received.server.map(([, value]) => value), Array.from({ length: 400 }, (_, i) => String(i)));
  assert.deepEqual(received.client.map(([, value]) => value), Array.from({ length: 400 }, (_, i) => String(i)));
  assert.ok(client.nextNumber > 120 && server.nextNumber > 120);
});

test('the newest authentic packet moves the path (roaming)', () => {
  const sim = new Simulation(), { client, server } = connectedPair(sim);
  const moves = [];
  server.onPath = (path, previous) => moves.push([previous.key, path.key]);
  client.transmit = packet => server.receive(Buffer.from(packet), { key: 'client-lte' });
  client.send('state', Buffer.alloc(8));
  sim.run(10);
  assert.deepEqual(moves, [['client', 'client-lte']]);
  assert.equal(server.path.key, 'client-lte');
});

test('initiations fill the 1200-byte budget exactly', () => assert.equal(INITIATION_BYTES, MAX_DATAGRAM));
