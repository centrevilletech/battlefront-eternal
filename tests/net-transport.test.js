import test from 'node:test';
import assert from 'node:assert/strict';
import { UdpEndpoint, CLOSE_CODE } from '../src/net/endpoint.js';
import { createNetService } from '../electron/net-service.js';
import { NetworkSession } from '../src/runtime/network-session.js';
import { UdpTransport, turnServer } from '../src/runtime/udp-transport.js';
import { NETWORK_CONTRACT } from '../src/runtime/network-contract.js';
import { identity, waitFor } from './net-udp-fixtures.js';
import { generateKeyPairSync } from 'node:crypto';

const wire = { protocol: NETWORK_CONTRACT.protocol, profile: NETWORK_CONTRACT.profile, simulation: NETWORK_CONTRACT.simulation };
// Content hashing reads the battle's files; a one-file world stands in.
globalThis.fetch = async path => path === '/maps/nab2/map.json' ? new Response('{}') : new Response(null, { status: 404 });
const rawKeys = () => { const pair = generateKeyPairSync('ed25519'); return { publicKey: pair.publicKey.export({ format: 'der', type: 'spki' }).toString('base64'), privateKey: pair.privateKey.export({ format: 'pem', type: 'pkcs8' }) }; };

// The renderer's MessagePort and the network process's channel, joined in
// memory with structured cloning. Binary arrives in the network process as
// typed arrays, as it can through Electron's utility-process ports.
const asViews = value => value instanceof ArrayBuffer ? new Uint8Array(value) : Array.isArray(value) ? value.map(asViews) : value && typeof value === 'object' ? Object.fromEntries(Object.entries(value).map(([key, item]) => [key, asViews(item)])) : value;
function portPair() {
  const renderer = { onmessage: null, closed: false, start() {}, close() { this.closed = true; serviceClose?.(); },
    postMessage(message, transfer = []) { const copy = asViews(structuredClone(message, { transfer })); setImmediate(() => serviceHandler?.(copy)); } };
  let serviceHandler = null, serviceClose = null;
  const channel = { post: message => { const copy = structuredClone(message); setImmediate(() => renderer.onmessage?.({ data: copy })); },
    onMessage: handler => { serviceHandler = handler; }, onClose: handler => { serviceClose = handler; }, close() {} };
  return { renderer, channel };
}

async function stack(t, { admit = () => ({ accept: true }), hello = () => ({ ...wire, content: 'content-a' }), onSession = () => {} } = {}) {
  const serverIdentity = identity();
  const server = new UdpEndpoint({ identity: serverIdentity, server: { hello, admit } });
  await server.listen({ host: '127.0.0.1' });
  server.on('session', onSession);
  const service = createNetService({ identity: rawKeys(), listen: { port: 0, families: ['IPv4'] }, candidateFilter: list => list });
  t.after(() => { service.close(); server.close(); });
  const open = () => { const { renderer, channel } = portPair(); service.serve(channel); return renderer; };
  const bridge = { request: async action => { throw Error('unexpected bridge request ' + action); }, openSession() {} };
  const transport = new UdpTransport({ bridge, openPort: async () => open() });
  return { server, serverIdentity, service, transport, open, bridge };
}

test('a network session joins over native UDP: registry, welcome, entity state and input', async t => {
  let serverSession;
  const received = [];
  const { server, serverIdentity, transport } = await stack(t, { onSession: session => {
    serverSession = session;
    session.on('message', (lane, payload) => received.push([lane, payload]));
    session.send('events', JSON.stringify({ type: 'entity-registry', registry: { contentEpoch: 1 } }));
    session.send('events', JSON.stringify({ type: 'welcome', snapshotMode: 'snapshot-delta-v1', epoch: 1, playerId: 7 }));
  } });
  const entities = [], registry = { contentEpoch: 1, inputCodes: [], bindingProfiles: new Map(), classIds: new Map(), postIds: new Map() };
  const session = new NetworkSession({ transport, lifecycle: null, playerName: 'Josh',
    onEntityRegistry: () => registry, onEntities: (bytes, at) => entities.push([bytes, at]) });
  const welcome = await session.init({ query: `?server=${serverIdentity.id}&address=127.0.0.1&port=${server.port}`, settings: {} }).catch(error => { throw error; });
  t.after(() => session.dispose());
  assert.equal(welcome.type, 'ready');
  assert.equal(welcome.playerId, 7);
  assert.deepEqual(serverSession.hello, { ...wire, content: session.content, name: 'Josh' });
  assert.equal(transport.spectator, true, 'different content joins as a spectator');
  assert.equal(transport.path.kind, 'direct');
  // A snapshot datagram reaches the entity queue with the network process's
  // receipt time, not the time this thread drained it.
  const packet = Buffer.alloc(40); packet.writeUInt32LE(0x31534e42, 0);
  const sentAt = performance.now();
  serverSession.send('state', packet);
  await waitFor(() => session.entityQueue.length === 1);
  const at = session.entityQueue[0].at * 1000;
  assert.ok(at >= sentAt - 5 && at <= performance.now() + 1, `receipt ${at - sentAt} ms after send`);
  session.drainEntities();
  assert.equal(entities.length, 1);
  // Input travels the state lane; events reach the server in order.
  session.channels.get('state').send(new Uint8Array([0x42, 0x4e, 0x49, 0x31, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]));
  session.chat({ text: 'one' }); session.chat({ text: 'two' });
  await waitFor(() => received.length === 3);
  assert.deepEqual(received.filter(([lane]) => lane === 'events').map(([, value]) => JSON.parse(value).text), ['one', 'two']);
  assert.equal(received.find(([lane]) => lane === 'state')[1].readUInt32LE(0), 0x31494e42);
});

test('a refusal, a version mismatch and a server close surface their reasons', async t => {
  const { server, serverIdentity, transport } = await stack(t, { admit: () => ({ code: CLOSE_CODE.REJECTED, reason: 'The server is full.' }) });
  const session = new NetworkSession({ transport, lifecycle: null, onEntityRegistry: () => ({}), onEntities: () => {} });
  await assert.rejects(session.init({ query: `?server=${serverIdentity.id}&address=127.0.0.1&port=${server.port}` }), /server is full/);

  const old = await stack(t, { hello: () => ({ ...wire, protocol: 2, content: 'x' }) });
  const stale = new NetworkSession({ transport: old.transport, lifecycle: null, onEntityRegistry: () => ({}), onEntities: () => {} });
  await assert.rejects(stale.init({ query: `?server=${old.serverIdentity.id}&address=127.0.0.1&port=${old.server.port}` }), /version mismatch/);

  let serverSession;
  const live = await stack(t, { onSession: session => { serverSession = session; session.send('events', JSON.stringify({ type: 'entity-registry', registry: {} })); session.send('events', JSON.stringify({ type: 'welcome', snapshotMode: 'snapshot-delta-v1' })); } });
  const errors = [];
  const player = new NetworkSession({ transport: live.transport, lifecycle: null, onEntityRegistry: () => ({ inputCodes: [], bindingProfiles: new Map(), classIds: new Map(), postIds: new Map() }), onEntities: () => {}, onError: error => errors.push(error.message) });
  await player.init({ query: `?server=${live.serverIdentity.id}&address=127.0.0.1&port=${live.server.port}` });
  serverSession.close(CLOSE_CODE.KICKED, 'Kicked by the server operator.');
  await waitFor(() => errors.length === 1);
  assert.match(errors[0], /Kicked/);
});

test('online rendezvous sends candidates through the master and follows trickled relay candidates', async t => {
  const { server, serverIdentity, transport, bridge } = await stack(t, { onSession: session => { session.send('events', JSON.stringify({ type: 'entity-registry', registry: {} })); session.send('events', JSON.stringify({ type: 'welcome', snapshotMode: 'snapshot-delta-v1' })); } });
  const signals = [];
  let deliver;
  bridge.request = async (action, value) => {
    if (action === 'credentials') return { urls: ['turns:relay.example:5349?transport=tcp', 'turn:127.0.0.1:9?transport=udp'], username: 'u', credential: 'p' };
    if (action === 'connect') return true;
    if (action === 'close') return true;
    if (action === 'signal') {
      signals.push(value);
      const offer = JSON.parse(value.payload);
      // The server answers with an unreachable address, then trickles the real one.
      setTimeout(() => deliver({ from: serverIdentity.id, type: 'answer', payload: JSON.stringify({ udp: 1, nonce: offer.nonce, candidates: [{ address: '192.0.2.10', port: 18451, kind: 'host' }] }) }), 5);
      setTimeout(() => deliver({ from: serverIdentity.id, type: 'candidate', payload: JSON.stringify({ udp: 1, nonce: offer.nonce, candidates: [{ address: '127.0.0.1', port: server.port, kind: 'relay' }] }) }), 30);
      setTimeout(() => deliver({ from: 'f'.repeat(64), type: 'answer', payload: JSON.stringify({ udp: 1, nonce: offer.nonce, candidates: [] }) }), 1);
      return true;
    }
    throw Error('unexpected ' + action);
  };
  bridge.onSignal = callback => { deliver = callback; return () => {}; };
  const session = new NetworkSession({ transport, lifecycle: null, onEntityRegistry: () => ({ inputCodes: [], bindingProfiles: new Map(), classIds: new Map(), postIds: new Map() }), onEntities: () => {} });
  t.after(() => session.dispose());
  await session.init({ query: `?server=${serverIdentity.id}` });
  assert.equal(signals.length, 1);
  assert.equal(signals[0].type, 'offer');
  const offer = JSON.parse(signals[0].payload);
  assert.equal(offer.udp, 1);
  assert.ok(Array.isArray(offer.candidates));
  assert.equal(transport.path.kind, 'relay', 'the trickled relay candidate carried the session');
  assert.deepEqual(turnServer('turn:relay.battlefronteternal.com:3478?transport=udp'), { host: 'relay.battlefronteternal.com', port: 3478 });
  assert.equal(turnServer('turns:relay.battlefronteternal.com:5349?transport=tcp'), null);
});
