import test from 'node:test';
import assert from 'node:assert/strict';
import { networkInterfaces } from 'node:os';
import { UdpEndpoint, CLOSE_CODE, validPeerAddress } from '../src/net/endpoint.js';
import { PACKET } from '../src/net/codec.js';
import { identity, UdpProxy, stunServer, turnServer, waitFor, bind } from './net-udp-fixtures.js';

const serverHello = { protocol: 3, profile: 'snapshot-delta-v1', simulation: 'bfe-sim-test', content: 'abc' };
async function pair(t, { admit = () => ({ accept: true }), serverOptions = {}, clientOptions = {} } = {}) {
  const serverIdentity = identity(), clientIdentity = identity();
  const server = new UdpEndpoint({ identity: serverIdentity, server: { hello: () => serverHello, admit }, ...serverOptions });
  await server.listen({ host: '127.0.0.1' });
  const client = new UdpEndpoint({ identity: clientIdentity, ...clientOptions });
  await client.listen({ families: ['IPv4'] });
  t.after(() => { client.close(); server.close(); });
  return { server, client, serverIdentity, clientIdentity };
}
const collect = session => { const out = []; session.on('message', (lane, payload) => out.push([lane, payload])); return out; };

test('a client session authenticates the server and carries every lane over real UDP', async t => {
  const { server, client, serverIdentity, clientIdentity } = await pair(t);
  const sessions = [];
  server.on('session', session => { sessions.push(session); collect(session); session.send('events', JSON.stringify({ type: 'entity-registry' })); session.send('state', Buffer.from('BNS1....')); });
  const session = await client.connect({ serverId: serverIdentity.id, candidates: [{ address: '127.0.0.1', port: server.port }], hello: { name: 'Josh', protocol: 3 } });
  const received = collect(session); session.resume();
  assert.deepEqual(session.hello, { ...serverHello, transport: { probing: 1 } }, 'the transport adds its features to the server hello');
  assert.equal(session.id, serverIdentity.id);
  await waitFor(() => received.length === 2);
  assert.deepEqual(received.map(([lane]) => lane).sort(), ['events', 'state']);
  assert.equal(sessions[0].id, clientIdentity.id);
  assert.deepEqual(sessions[0].hello, { name: 'Josh', protocol: 3 });
  const { localPort, ...remote } = session.path;
  assert.deepEqual(remote, { kind: 'direct', family: 'IPv4', address: '127.0.0.1', port: server.port });
  assert.equal(localPort, client.port, 'a session without a port race keeps the endpoint port');
  const big = Buffer.alloc(150000, 3);
  session.send('bulk', big); session.send('events', 'after');
  const serverReceived = [];
  sessions[0].on('message', (lane, payload) => serverReceived.push([lane, payload]));
  await waitFor(() => serverReceived.length === 2);
  assert.ok(serverReceived.find(([lane]) => lane === 'bulk')[1].equals(big));
  assert.ok(session.rtt < 50);
});

test('the wrong server identity, a refusal and a silent address all fail clearly', async t => {
  const { server, client } = await pair(t, { admit: ({ client }) => client.hello.banned ? { code: CLOSE_CODE.REJECTED, reason: 'You are banned from this server.' } : { accept: true } });
  await assert.rejects(client.connect({ serverId: 'a'.repeat(64), candidates: [{ address: '127.0.0.1', port: server.port }] }), /does not match/);
  await assert.rejects(client.connect({ serverId: server.identity.id, candidates: [{ address: '127.0.0.1', port: server.port }], hello: { banned: true } }), /banned/);
  const silent = await bind();
  t.after(() => silent.close());
  await assert.rejects(client.connect({ serverId: server.identity.id, candidates: [{ address: '127.0.0.1', port: silent.address().port }], timeoutMs: 600 }), /timed out/);
  await assert.rejects(client.connect({ serverId: server.identity.id, candidates: [{ address: '255.255.255.255', port: 1 }, { address: '224.0.0.1', port: 1 }] }), /No usable/);
  assert.equal(validPeerAddress('::'), false);
  assert.equal(validPeerAddress('fe80::1%eth0'), true);
});

test('the first working candidate wins among unreachable ones', async t => {
  const { server, client } = await pair(t);
  const dead = await bind();
  t.after(() => dead.close());
  const started = Date.now();
  const session = await client.connect({ serverId: server.identity.id, candidates: [{ address: '127.0.0.1', port: dead.address().port }, { address: '192.0.2.1', port: 9 }, { address: '127.0.0.1', port: server.port }] });
  assert.ok(Date.now() - started < 500);
  assert.equal(session.path.port, server.port);
});

// Multipath routing in miniature: each client source port gets its own
// route to the server, and only one of them is fast.
// `slowStart` holds the fast route's first replies back, so the handshake
// race picks a slow port and only probing can find the fast one.
async function multipathProxy(target, { fastIndex, slowMs, slowStart = 0 }) {
  const front = await bind(), routes = new Map();
  let closed = false;
  // Delayed sends may come due after the test closed the proxy.
  const later = (fn, ms) => setTimeout(() => { if (!closed) fn(); }, ms);
  front.on('message', async (packet, info) => {
    let route = routes.get(info.port);
    if (!route) {
      route = { delay: routes.size === fastIndex ? 0 : slowMs, upstream: await bind(), replies: 0 };
      routes.set(info.port, route);
      route.upstream.on('message', reply => {
        const held = route.delay === 0 && route.replies++ < slowStart ? 150 : route.delay;
        later(() => front.send(reply, info.port, info.address), held);
      });
    }
    later(() => route.upstream.send(packet, target.port, target.address), route.delay);
  });
  return { port: front.address().port, routes, close() { closed = true; front.close(); for (const route of routes.values()) route.upstream.close(); } };
}
const probed = endpoint => new Promise(resolve => endpoint.once('path-probed', (session, report) => resolve(report)));

test('admission may answer asynchronously (policy on another thread)', async t => {
  let release;
  const { server, client } = await pair(t, { admit: ({ client: peer }) => new Promise(resolve => { release = () => resolve(peer.hello.refuse ? { code: CLOSE_CODE.REJECTED, reason: 'You are banned from this server.' } : { accept: true }); }) });
  const sessions = [];
  server.on('session', session => sessions.push(session));
  const joining = client.connect({ serverId: server.identity.id, candidates: [{ address: '127.0.0.1', port: server.port }] });
  await waitFor(() => release);
  await new Promise(resolve => setTimeout(resolve, 400));
  assert.equal(sessions.length, 0, 'nothing is admitted, and retransmitted CONFIRMs confirm nothing, until policy answers');
  release();
  const session = await joining;
  assert.ok(session.ready);
  await waitFor(() => sessions.length === 1);
  release = null;
  const refused = client.connect({ serverId: server.identity.id, candidates: [{ address: '127.0.0.1', port: server.port }], hello: { refuse: true } });
  await waitFor(() => release);
  release();
  await assert.rejects(refused, /banned/);
});

test('a port race keeps the fastest of several routes to one server and closes the rest', async t => {
  const { server, client } = await pair(t, { clientOptions: { raceAddress: () => true } });
  const proxy = await multipathProxy({ address: '127.0.0.1', port: server.port }, { fastIndex: 3, slowMs: 60 });
  t.after(() => proxy.close());
  const report = probed(client);
  const session = await client.connect({ serverId: server.identity.id, candidates: [{ address: '127.0.0.1', port: proxy.port }], ports: 6 });
  session.resume();
  const fast = [...proxy.routes].find(([, route]) => route.delay === 0)[0];
  assert.equal(proxy.routes.size, 6, 'six local ports raced');
  assert.equal(session.path.localPort, fast, 'the session uses the port with the fast route');
  assert.equal((await report).ports.length, 16, 'every raced port and the probe-only ones were timed');
  assert.ok(session.rtt < 40, `round trip ${session.rtt} ms`);
  assert.equal(client.extraSockets.size, fast === client.port ? 0 : 1, 'losing ports are closed');
  session.close();
  await waitFor(() => client.extraSockets.size === 0, 4000);
});

test('probing moves a session the race put on a slow port, and the server follows', async t => {
  const { server, client } = await pair(t, { clientOptions: { raceAddress: () => true } });
  const proxy = await multipathProxy({ address: '127.0.0.1', port: server.port }, { fastIndex: 2, slowMs: 40, slowStart: 2 });
  t.after(() => proxy.close());
  let serverSession;
  server.on('session', session => { serverSession = session; });
  const report = probed(client);
  const session = await client.connect({ serverId: server.identity.id, candidates: [{ address: '127.0.0.1', port: proxy.port }], ports: 4 });
  session.resume();
  const fast = [...proxy.routes].find(([, route]) => route.delay === 0)[0];
  assert.notEqual(session.path.localPort, fast, 'the held-back fast route lost the handshake race');
  const result = await report;
  assert.equal(result.moved, true);
  assert.equal(session.path.localPort, fast, 'probing found the fast port');
  assert.equal(Math.min(...result.ports.map(item => item.ms)), result.ports.find(item => item.port === fast).ms);
  // The server validates the new address and moves; traffic flows both ways.
  const received = [], echoes = [];
  serverSession.on('message', (lane, payload) => received.push(String(payload)));
  session.on('message', (lane, payload) => echoes.push(String(payload)));
  session.send('events', 'after the move');
  await waitFor(() => received.includes('after the move'));
  await waitFor(() => serverSession.path.port === [...proxy.routes.values()].find(route => route.delay === 0).upstream.address().port, 3000);
  serverSession.send('events', 'reply');
  await waitFor(() => echoes.includes('reply'));
  session.close();
  await waitFor(() => client.extraSockets.size === 0, 5000);
});

test('without a race the endpoint port carries the session', async t => {
  const { server, client } = await pair(t, { clientOptions: { raceAddress: () => true } });
  const session = await client.connect({ serverId: server.identity.id, candidates: [{ address: '127.0.0.1', port: server.port }] });
  assert.equal(session.path.localPort, client.port);
  assert.equal(client.extraSockets.size, 0);
});

test('handshake and lanes survive a lossy, jittery path; datagrams are never resent', async t => {
  const { server, client } = await pair(t);
  const proxy = await UdpProxy.create({ address: '127.0.0.1', port: server.port }, { loss: 0.2, delay: 15, jitter: 10 });
  t.after(() => proxy.close());
  let serverSession;
  server.on('session', session => { serverSession = session; });
  const session = await client.connect({ serverId: server.identity.id, candidates: [{ address: '127.0.0.1', port: proxy.port }] });
  session.resume();
  const received = [];
  serverSession.on('message', (lane, payload) => received.push([lane, String(payload)]));
  for (let i = 0; i < 100; i++) { session.send('events', 'event ' + i); session.send('state', 'state ' + i); await new Promise(resolve => setTimeout(resolve, 2)); }
  await waitFor(() => received.filter(([lane]) => lane === 'events').length === 100, 10000);
  assert.deepEqual(received.filter(([lane]) => lane === 'events').map(([, value]) => value), Array.from({ length: 100 }, (_, i) => 'event ' + i));
  const states = received.filter(([lane]) => lane === 'state');
  assert.ok(states.length < 100 && states.length > 50, `state ${states.length}`);
  assert.equal(new Set(states.map(([, value]) => value)).size, states.length, 'no duplicate datagrams');
});

test('a NAT rebinding moves the session without a new handshake', async t => {
  const { server, client } = await pair(t);
  const proxy = await UdpProxy.create({ address: '127.0.0.1', port: server.port });
  t.after(() => proxy.close());
  let serverSession;
  server.on('session', session => { serverSession = session; });
  const session = await client.connect({ serverId: server.identity.id, candidates: [{ address: '127.0.0.1', port: proxy.port }] });
  session.resume();
  const before = serverSession.path.port, moves = [];
  serverSession.on('path', path => moves.push(path.port));
  await proxy.rebind();
  const received = [];
  serverSession.on('message', (lane, payload) => received.push(String(payload)));
  const echoes = [];
  session.on('message', (lane, payload) => echoes.push(String(payload)));
  session.send('events', 'after rebinding');
  await waitFor(() => received.includes('after rebinding'));
  serverSession.send('events', 'reply');
  await waitFor(() => echoes.includes('reply'));
  assert.notEqual(serverSession.path.port, before);
  assert.deepEqual(moves, [serverSession.path.port]);
  assert.equal(server.stats().sessions, 1);
});

test('stateless retry under load still connects', async t => {
  const { server, client } = await pair(t);
  server.underLoad = () => true;
  const session = await client.connect({ serverId: server.identity.id, candidates: [{ address: '127.0.0.1', port: server.port }] });
  assert.ok(session.ready);
  assert.equal(server.handshakes.pending.size, 0);
});

test('an unknown transport version is told to update', async t => {
  const { server } = await pair(t);
  const socket = await bind();
  t.after(() => socket.close());
  const initiation = Buffer.alloc(1200); initiation[0] = PACKET.INITIATION; initiation[1] = 99; initiation.writeUInt32LE(5, 2);
  const reply = new Promise(resolve => socket.once('message', resolve));
  socket.send(initiation, server.port, '127.0.0.1');
  const packet = await reply;
  assert.equal(packet[0], PACKET.VERSION);
  assert.equal(packet.readUInt32LE(2), 5);
});

test('closing a session reaches the peer with its reason; a vanished server times out', async t => {
  const { server, client } = await pair(t, { clientOptions: { connection: { idleTimeoutMs: 800 } } });
  let serverSession;
  server.on('session', session => { serverSession = session; });
  const session = await client.connect({ serverId: server.identity.id, candidates: [{ address: '127.0.0.1', port: server.port }] });
  const closed = new Promise(resolve => session.once('close', resolve));
  serverSession.close(CLOSE_CODE.KICKED, 'Kicked by the server operator.');
  assert.deepEqual(await closed, { code: CLOSE_CODE.KICKED, reason: 'Kicked by the server operator.', remote: true });
  const again = await client.connect({ serverId: server.identity.id, candidates: [{ address: '127.0.0.1', port: server.port }] });
  const timeout = new Promise(resolve => again.once('close', resolve));
  for (const socket of server.sockets.values()) socket.close();
  server.sockets.clear();
  assert.equal((await timeout).code, CLOSE_CODE.IDLE);
});

test('IPv6 loopback works when the host has IPv6', { skip: !Object.values(networkInterfaces()).flat().some(entry => entry?.family === 'IPv6' && entry.internal) }, async t => {
  const serverIdentity = identity();
  const server = new UdpEndpoint({ identity: serverIdentity, server: { hello: () => ({}), admit: () => ({ accept: true }) } });
  await server.listen({ host: '::1' });
  const client = new UdpEndpoint({ identity: identity() });
  await client.listen({ families: ['IPv6'] });
  t.after(() => { client.close(); server.close(); });
  const session = await client.connect({ serverId: serverIdentity.id, candidates: [{ address: '::1', port: server.port }] });
  assert.equal(session.path.family, 'IPv6');
});

test('a STUN server reports the reflexive address', async t => {
  const stun = await stunServer(), client = new UdpEndpoint({ identity: identity() });
  await client.listen({ families: ['IPv4'] });
  t.after(() => { client.close(); stun.close(); });
  const result = await client.reflexive({ host: '127.0.0.1', port: stun.port });
  assert.deepEqual(result.IPv4, { address: '127.0.0.1', port: client.port });
});

test('a server behind a TURN relay accepts sessions on its relayed address', async t => {
  const turn = await turnServer();
  const { server, client } = await pair(t);
  t.after(() => turn.close());
  const relay = await server.allocateRelay({ host: '127.0.0.1', port: turn.port, username: turn.username, password: turn.password });
  assert.equal(relay.relayed.address, '127.0.0.1');
  // A dual-stack player offers IPv6 addresses too; an IPv4 allocation
  // permits only its own family instead of failing the whole request.
  await relay.permit(['127.0.0.1', '2001:db8:84:1::5b31']);
  assert.deepEqual([...relay.turn.permissions.keys()], ['127.0.0.1']);
  let serverSession;
  server.on('session', session => { serverSession = session; });
  const session = await client.connect({ serverId: server.identity.id, candidates: [{ address: relay.relayed.address, port: relay.relayed.port, kind: 'relay' }] });
  session.resume();
  assert.equal(session.path.kind, 'relay');
  assert.equal(serverSession.path.kind, 'relay');
  const received = [];
  session.on('message', (lane, payload) => received.push(String(payload)));
  for (let i = 0; i < 20; i++) serverSession.send('events', 'relayed ' + i);
  await waitFor(() => received.length === 20);
  assert.ok(turn.stats.channels >= 1, 'data moves over a bound channel');
  assert.ok(turn.stats.relayedDown > 0 && turn.stats.relayedUp > 0);
});

test('a client can route through its own relay allocation', async t => {
  const turn = await turnServer();
  const { server, client } = await pair(t);
  t.after(() => turn.close());
  const relay = await client.allocateRelay({ host: '127.0.0.1', port: turn.port, username: turn.username, password: turn.password });
  await relay.permit(['127.0.0.1']);
  let serverSession;
  server.on('session', session => { serverSession = session; });
  const session = await client.connect({ serverId: server.identity.id, candidates: [{ address: '127.0.0.1', port: server.port }], relay });
  assert.equal(session.path.kind, 'relay');
  assert.equal(serverSession.path.port, relay.relayed.port, 'the server only sees the relay');
});
