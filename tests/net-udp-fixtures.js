// Real-socket fixtures for BFE-UDP/1 endpoint tests: identities, a lossy
// UDP proxy that can rebind its source port (NAT rebinding), and minimal
// STUN and TURN servers implementing just what the client uses.
import dgram from 'node:dgram';
import { generateKeyPairSync } from 'node:crypto';
import { identityKeys } from '../src/net/crypto.js';
import { normalizeAddress } from '../src/net/codec.js';
import { ATTR, CLASS, METHOD, address, checkIntegrity, decodeAddress, decodeMessage, encodeAddress, encodeMessage, isChannelData, longTermKey, u32 } from '../src/net/stun.js';
import { seeded } from './net-sim.js';

export function identity() {
  const pair = generateKeyPairSync('ed25519');
  return identityKeys({ publicKey: pair.publicKey.export({ format: 'der', type: 'spki' }).toString('base64'), privateKey: pair.privateKey.export({ format: 'pem', type: 'pkcs8' }) });
}
export const bind = (type = 'udp4', port = 0, host = type === 'udp6' ? '::1' : '127.0.0.1') => new Promise((resolve, reject) => {
  const socket = dgram.createSocket({ type, ipv6Only: type === 'udp6' });
  socket.once('error', reject);
  socket.bind(port, host, () => { socket.off('error', reject); resolve(socket); });
});
export const waitFor = async (condition, ms = 5000) => { const end = Date.now() + ms; while (!condition()) { if (Date.now() > end) throw Error('timed out'); await new Promise(resolve => setTimeout(resolve, 5)); } };

// Forwards between one client and a server, impairing each direction.
// rebind() moves the proxy's server-facing socket to a new port, which is
// what a NAT rebinding looks like to the server.
export class UdpProxy {
  static async create(target, options = {}) { const proxy = new UdpProxy(target, options); await proxy.start(); return proxy; }
  constructor(target, { loss = 0, delay = 0, jitter = 0, seed = 7 } = {}) { Object.assign(this, { target, loss, delay, jitter }); this.random = seeded(seed); this.client = null; this.counts = { up: 0, down: 0, dropped: 0 }; }
  async start() {
    this.front = await bind();
    this.port = this.front.address().port;
    this.front.on('message', (packet, info) => { this.client = info; this.forward(packet, bytes => this.back.send(bytes, this.target.port, this.target.address), 'up'); });
    await this.rebind();
  }
  async rebind() {
    const previous = this.back;
    this.back = await bind();
    this.back.on('message', packet => this.forward(packet, bytes => this.client && this.front.send(bytes, this.client.port, this.client.address), 'down'));
    previous?.close();
  }
  forward(packet, send, direction) {
    this.counts[direction]++;
    if (this.blocked?.[direction] || this.random() < this.loss) { this.counts.dropped++; return; }
    const latency = Math.max(0, this.delay + (this.random() * 2 - 1) * this.jitter);
    // A delayed packet may come due after the test closed the proxy.
    const deliver = () => { if (!this.closed) send(packet); };
    if (latency) setTimeout(deliver, latency); else deliver();
  }
  close() { this.closed = true; this.front.close(); this.back.close(); }
}

export async function stunServer() {
  const socket = await bind();
  socket.on('message', (packet, info) => {
    const message = decodeMessage(packet);
    if (message?.method === METHOD.BINDING && message.klass === CLASS.REQUEST)
      socket.send(encodeMessage(METHOD.BINDING, CLASS.SUCCESS, message.transaction, [[ATTR.XOR_MAPPED_ADDRESS, encodeAddress(info.address, info.port, message.transaction)]]), info.port, info.address);
  });
  return { socket, port: socket.address().port, close: () => socket.close() };
}

// Long-term credentials, one realm, IPv4 relays on loopback.
export async function turnServer({ username = 'user', password = 'secret', realm = 'test' } = {}) {
  const control = await bind(), key = longTermKey(username, realm, password), nonce = 'nonce-1';
  const allocations = new Map();
  const stats = { allocations: 0, permissions: 0, channels: 0, relayedUp: 0, relayedDown: 0 };
  const reply = (info, method, klass, transaction, attributes, sign = true) => control.send(encodeMessage(method, klass, transaction, attributes, sign ? key : null), info.port, info.address);
  const fail = (info, method, transaction, code, reason) => {
    const value = Buffer.alloc(4 + Buffer.byteLength(reason)); value[2] = Math.floor(code / 100); value[3] = code % 100; value.write(reason, 4);
    reply(info, method, CLASS.ERROR, transaction, [[ATTR.ERROR_CODE, value], [ATTR.REALM, Buffer.from(realm)], [ATTR.NONCE, Buffer.from(nonce)]], false);
  };
  control.on('message', async (packet, info) => {
    const id = `${info.address}|${info.port}`, allocation = allocations.get(id);
    if (isChannelData(packet)) {
      const peer = allocation?.channels.get(packet.readUInt16BE(0));
      if (peer && allocation.permissions.has(peer.address)) { stats.relayedUp++; allocation.relay.send(packet.subarray(4, 4 + packet.readUInt16BE(2)), peer.port, peer.address); }
      return;
    }
    const message = decodeMessage(packet);
    if (!message || message.klass === CLASS.SUCCESS) return;
    if (message.method === METHOD.SEND && message.klass === CLASS.INDICATION) {
      const peer = address(message, ATTR.XOR_PEER_ADDRESS);
      if (allocation && peer && allocation.permissions.has(normalizeAddress(peer.address))) { stats.relayedUp++; allocation.relay.send(message.attributes.get(ATTR.DATA), peer.port, peer.address); }
      return;
    }
    if (!message.attributes.get(ATTR.USERNAME) || !checkIntegrity(message, key)) return fail(info, message.method, message.transaction, 401, 'Unauthorized');
    if (message.method === METHOD.ALLOCATE) {
      if (allocation) return reply(info, METHOD.ALLOCATE, CLASS.SUCCESS, message.transaction, [[ATTR.XOR_RELAYED_ADDRESS, encodeAddress('127.0.0.1', allocation.port, message.transaction)], [ATTR.XOR_MAPPED_ADDRESS, encodeAddress(info.address, info.port, message.transaction)], [ATTR.LIFETIME, u32(600)]]);
      const relay = await bind(), entry = { relay, port: relay.address().port, permissions: new Set(), channels: new Map(), peers: new Map() };
      allocations.set(id, entry); stats.allocations++;
      relay.on('message', (data, from) => {
        if (!entry.permissions.has(normalizeAddress(from.address))) return;
        stats.relayedDown++;
        const channel = entry.peers.get(`${normalizeAddress(from.address)}|${from.port}`);
        if (channel) { const out = Buffer.alloc(4 + data.length); out.writeUInt16BE(channel, 0); out.writeUInt16BE(data.length, 2); data.copy(out, 4); control.send(out, info.port, info.address); }
        else { const transaction = Buffer.alloc(12, 3); control.send(encodeMessage(METHOD.DATA, CLASS.INDICATION, transaction, [[ATTR.XOR_PEER_ADDRESS, encodeAddress(from.address, from.port, transaction)], [ATTR.DATA, data]]), info.port, info.address); }
      });
      return reply(info, METHOD.ALLOCATE, CLASS.SUCCESS, message.transaction, [[ATTR.XOR_RELAYED_ADDRESS, encodeAddress('127.0.0.1', entry.port, message.transaction)], [ATTR.XOR_MAPPED_ADDRESS, encodeAddress(info.address, info.port, message.transaction)], [ATTR.LIFETIME, u32(600)]]);
    }
    if (!allocation) return fail(info, message.method, message.transaction, 437, 'Allocation Mismatch');
    if (message.method === METHOD.REFRESH) {
      if (message.attributes.get(ATTR.LIFETIME)?.readUInt32BE(0) === 0) { allocation.relay.close(); allocations.delete(id); }
      return reply(info, METHOD.REFRESH, CLASS.SUCCESS, message.transaction, [[ATTR.LIFETIME, u32(600)]]);
    }
    if (message.method === METHOD.CREATE_PERMISSION) {
      for (const [type, value] of message.list) if (type === ATTR.XOR_PEER_ADDRESS) { allocation.permissions.add(normalizeAddress(decodeAddress(value, message.transaction).address)); stats.permissions++; }
      return reply(info, METHOD.CREATE_PERMISSION, CLASS.SUCCESS, message.transaction, []);
    }
    if (message.method === METHOD.CHANNEL_BIND) {
      const peer = address(message, ATTR.XOR_PEER_ADDRESS), number = message.attributes.get(ATTR.CHANNEL_NUMBER)?.readUInt16BE(0);
      allocation.permissions.add(normalizeAddress(peer.address));
      allocation.channels.set(number, { address: normalizeAddress(peer.address), port: peer.port });
      allocation.peers.set(`${normalizeAddress(peer.address)}|${peer.port}`, number);
      stats.channels++;
      return reply(info, METHOD.CHANNEL_BIND, CLASS.SUCCESS, message.transaction, []);
    }
  });
  return { port: control.address().port, username, password, stats, close: () => { control.close(); for (const allocation of allocations.values()) allocation.relay.close(); } };
}
