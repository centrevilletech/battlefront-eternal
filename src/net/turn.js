// A TURN (RFC 8656) UDP allocation client for relayed BFE-UDP/1 paths.
// The allocation shares its socket with the transport: the endpoint hands
// every packet from the TURN server to handle(). Data to and from peers uses
// ChannelData (4 bytes of overhead) once a channel is bound.
import {
  ATTR, CLASS, METHOD, address, checkIntegrity, decodeMessage, encodeAddress, encodeMessage, errorCode, isChannelData,
  longTermKey, newTransaction, u32,
} from './stun.js';
import { normalizeAddress } from './codec.js';

const RTO = 250, ATTEMPTS = 5, FIRST_CHANNEL = 0x4000, LAST_CHANNEL = 0x4fff;
// Permissions last 300 s and channel bindings 600 s (RFC 8656); refresh
// both well before. coturn keeps refreshing an allocation after its REST
// credential's own timestamp has passed (checked against coturn 4.18).
const PERMISSION_REFRESH = 240000, CHANNEL_REFRESH = 540000;

export class TurnClient {
  constructor({ send, server, username, password, family = 'IPv4', onData = () => {}, onExpired = () => {},
    setTimer = (fn, ms) => setTimeout(fn, ms), clearTimer = id => clearTimeout(id), now = () => Date.now(),
    permissionRefreshMs = PERMISSION_REFRESH, channelRefreshMs = CHANNEL_REFRESH }) {
    Object.assign(this, { send, server, username, password, family, onData, onExpired, setTimer, clearTimer, now, permissionRefreshMs, channelRefreshMs });
    this.realm = null; this.nonce = null; this.key = null;
    this.transactions = new Map(); this.channels = new Map(); this.channelPeers = new Map(); this.nextChannel = FIRST_CHANNEL;
    this.permissions = new Map(); this.timers = new Set(); this.closed = false; this.relayed = null;
  }
  later(fn, ms) { const id = this.setTimer(() => { this.timers.delete(id); fn(); }, ms); this.timers.add(id); return id; }
  // One request with retransmission; resolves with the success response.
  // `attributes` may be a function of the transaction id, which masks the
  // XOR address attributes of IPv6 peers.
  request(method, attributes, { authenticate = true } = {}) {
    return new Promise((resolve, reject) => {
      const attempt = (retried = false) => {
        if (this.closed) { reject(Error('TURN client closed')); return; }
        const transaction = newTransaction(), key = transaction.toString('hex');
        const credentials = authenticate && this.key ? [[ATTR.USERNAME, Buffer.from(this.username)], [ATTR.REALM, Buffer.from(this.realm)], [ATTR.NONCE, Buffer.from(this.nonce)]] : [];
        const fields = typeof attributes === 'function' ? attributes(transaction) : attributes;
        const packet = encodeMessage(method, CLASS.REQUEST, transaction, [...fields, ...credentials], credentials.length ? this.key : null);
        let tries = 0, timer = null;
        const transmit = () => {
          if (tries++ >= ATTEMPTS) { this.transactions.delete(key); reject(Error('TURN server did not answer')); return; }
          this.send(packet, this.server.port, this.server.address);
          timer = this.later(transmit, RTO * 2 ** (tries - 1));
        };
        this.transactions.set(key, message => {
          this.transactions.delete(key); if (timer !== null) { this.clearTimer(timer); this.timers.delete(timer); }
          if (message.klass === CLASS.SUCCESS) {
            if (credentials.length && !checkIntegrity(message, this.key)) { reject(Error('TURN response failed integrity')); return; }
            resolve(message); return;
          }
          const error = errorCode(message);
          const realm = message.attributes.get(ATTR.REALM), nonce = message.attributes.get(ATTR.NONCE);
          // 401 asks for credentials; 438 means the nonce went stale.
          if ((error?.code === 401 || error?.code === 438) && nonce && !retried && authenticate) {
            if (realm) this.realm = realm.toString('utf8');
            this.nonce = nonce.toString('utf8');
            this.key = longTermKey(this.username, this.realm, this.password);
            attempt(true); return;
          }
          reject(Object.assign(Error(`TURN ${error?.code ?? 'error'} ${error?.reason ?? ''}`.trim()), { code: error?.code }));
        });
        transmit();
      };
      attempt();
    });
  }
  async allocate(lifetime = 600) {
    const response = await this.request(METHOD.ALLOCATE, [
      [ATTR.REQUESTED_TRANSPORT, Buffer.from([17, 0, 0, 0])], [ATTR.LIFETIME, u32(lifetime)],
      ...(this.family === 'IPv6' ? [[ATTR.REQUESTED_ADDRESS_FAMILY, Buffer.from([2, 0, 0, 0])]] : []),
    ]);
    this.relayed = address(response, ATTR.XOR_RELAYED_ADDRESS);
    this.mapped = address(response, ATTR.XOR_MAPPED_ADDRESS);
    if (!this.relayed) throw Error('TURN allocation has no relayed address');
    this.lifetime = response.attributes.get(ATTR.LIFETIME)?.readUInt32BE(0) ?? lifetime;
    this.scheduleRefresh();
    return { relayed: this.relayed, mapped: this.mapped, lifetime: this.lifetime };
  }
  scheduleRefresh() {
    this.later(async () => {
      try { await this.request(METHOD.REFRESH, [[ATTR.LIFETIME, u32(this.lifetime)]]); this.scheduleRefresh(); }
      catch (error) { if (!this.closed) this.onExpired(error); }
    }, Math.max(30000, this.lifetime * 500));
  }
  // Lets packets from these peer IPs through the relay (ports are ignored),
  // and keeps every permitted peer's permission alive from then on.
  async permit(addresses) {
    const fresh = [...new Set(addresses.map(normalizeAddress))];
    if (!fresh.length) return;
    await this.createPermission(fresh);
    for (const peer of fresh) this.permissions.set(peer, this.now());
    this.permissionTimer ??= this.later(() => this.refreshPermissions(), this.permissionRefreshMs);
  }
  createPermission(peers) { return this.request(METHOD.CREATE_PERMISSION, transaction => peers.map(peer => [ATTR.XOR_PEER_ADDRESS, encodeAddress(peer, 0, transaction)])); }
  async refreshPermissions() {
    this.permissionTimer = null;
    if (this.closed) return;
    const peers = [...this.permissions.keys()];
    // A request carries a bounded number of addresses; refresh in groups.
    try { for (let i = 0; i < peers.length; i += 8) await this.createPermission(peers.slice(i, i + 8)); }
    catch (error) { if (!this.closed) this.onExpired(error); return; }
    this.permissionTimer = this.later(() => this.refreshPermissions(), this.permissionRefreshMs);
  }
  async bind(peerAddress, peerPort) {
    const key = `${normalizeAddress(peerAddress)}|${peerPort}`;
    const existing = this.channels.get(key);
    if (existing) return existing.ready;
    if (this.nextChannel > LAST_CHANNEL) throw Error('TURN channels exhausted');
    const channel = { number: this.nextChannel++, bound: false };
    channel.ready = this.channelBind(channel, peerAddress, peerPort).then(() => { channel.bound = true; return channel.number; });
    this.channels.set(key, channel);
    this.channelPeers.set(channel.number, { address: normalizeAddress(peerAddress), port: peerPort });
    channel.ready.catch(() => { if (this.channels.get(key) === channel) this.channels.delete(key); });
    return channel.ready;
  }
  async channelBind(channel, peerAddress, peerPort) {
    const number = Buffer.alloc(4); number.writeUInt16BE(channel.number, 0);
    await this.request(METHOD.CHANNEL_BIND, transaction => [[ATTR.CHANNEL_NUMBER, number], [ATTR.XOR_PEER_ADDRESS, encodeAddress(normalizeAddress(peerAddress), peerPort, transaction)]]);
    this.permissions.set(normalizeAddress(peerAddress), this.now());
    if (!channel.refresh) channel.refresh = () => this.later(() => this.channelBind(channel, peerAddress, peerPort).then(channel.refresh, () => {}), this.channelRefreshMs);
    channel.refresh();
  }
  // Data for a peer: ChannelData once bound, a Send indication before.
  sendTo(peerAddress, peerPort, data) {
    if (this.closed) return false;
    const channel = this.channels.get(`${normalizeAddress(peerAddress)}|${peerPort}`);
    if (channel?.bound) {
      const packet = Buffer.allocUnsafe(4 + data.length);
      packet.writeUInt16BE(channel.number, 0); packet.writeUInt16BE(data.length, 2); data.copy(packet, 4);
      this.send(packet, this.server.port, this.server.address);
      return true;
    }
    if (!channel) this.bind(peerAddress, peerPort).catch(() => {});
    const transaction = newTransaction();
    this.send(encodeMessage(METHOD.SEND, CLASS.INDICATION, transaction, [[ATTR.XOR_PEER_ADDRESS, encodeAddress(normalizeAddress(peerAddress), peerPort, transaction)], [ATTR.DATA, data]]), this.server.port, this.server.address);
    return true;
  }
  // A packet from the TURN server. Returns true when it was TURN traffic.
  handle(packet) {
    if (isChannelData(packet)) {
      const peer = this.channelPeers.get(packet.readUInt16BE(0)), length = packet.readUInt16BE(2);
      if (peer && 4 + length <= packet.length) this.onData(peer.address, peer.port, packet.subarray(4, 4 + length));
      return true;
    }
    const message = decodeMessage(packet);
    if (!message) return false;
    if (message.method === METHOD.DATA && message.klass === CLASS.INDICATION) {
      const peer = address(message, ATTR.XOR_PEER_ADDRESS), data = message.attributes.get(ATTR.DATA);
      if (peer && data) this.onData(peer.address, peer.port, data);
      return true;
    }
    this.transactions.get(message.transaction.toString('hex'))?.(message);
    return true;
  }
  close() {
    if (this.closed) return;
    if (this.relayed) {
      const transaction = newTransaction();
      const credentials = this.key ? [[ATTR.USERNAME, Buffer.from(this.username)], [ATTR.REALM, Buffer.from(this.realm)], [ATTR.NONCE, Buffer.from(this.nonce)]] : [];
      try { this.send(encodeMessage(METHOD.REFRESH, CLASS.REQUEST, transaction, [[ATTR.LIFETIME, u32(0)], ...credentials], this.key), this.server.port, this.server.address); } catch {}
    }
    this.closed = true;
    for (const id of this.timers) this.clearTimer(id);
    this.timers.clear();
    for (const callback of this.transactions.values()) callback({ klass: CLASS.ERROR, attributes: new Map() });
    this.transactions.clear();
  }
}
