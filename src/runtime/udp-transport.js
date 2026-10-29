import { NETWORK_CONTRACT } from './network-contract.js';

const hex = bytes => [...bytes].map(v => v.toString(16).padStart(2, '0')).join('');
const STUN_DEFAULT = { host: 'relay.battlefronteternal.com', port: 3478 };
// turn:host:port?transport=udp as the master issues it.
export function turnServer(url) {
  const match = /^turn:([^:?]+):(\d+)\?transport=udp$/.exec(String(url));
  return match ? { host: match[1], port: Number(match[2]) } : null;
}

// Online play over the desktop network process's native UDP transport
// (BFE-UDP/1, protocol 3). The page holds one MessagePort per battle; the
// network process does the handshake, so everything arriving here is
// already authenticated as the expected server. LAN and JOIN IP need no
// master: the handshake goes straight to the address. ONLINE exchanges UDP
// candidates through master rendezvous, then connects the same way.
export class UdpTransport {
  constructor(options = {}) {
    Object.assign(this, { bridge: globalThis.bfeDesktop?.network, port: 18451, relay: false, target: globalThis.window, openPort: null }, options);
    this.closed = false; this.connected = false; this.outgoing = []; this.scheduled = false;
  }
  // The MessagePort arrives as a window message from the preload script.
  requestPort() {
    if (this.openPort) return this.openPort();
    return new Promise((resolve, reject) => {
      const id = hex(crypto.getRandomValues(new Uint8Array(16)));
      const timer = setTimeout(() => { this.target.removeEventListener('message', listener); reject(Error('The desktop network process did not start.')); }, 10000);
      const listener = event => {
        if (event.source !== this.target || event.data?.type !== 'bfe:net-port' || event.data.id !== id || !event.ports?.[0]) return;
        clearTimeout(timer); this.target.removeEventListener('message', listener); resolve(event.ports[0]);
      };
      this.target.addEventListener('message', listener);
      this.bridge.openSession(id);
    });
  }
  // Waits for the network process's `reply` op (an error for that request
  // rejects it), posting `message` first when there is one.
  request(message, reply) {
    return new Promise((resolve, reject) => {
      this.waiters.push({ reply, resolve, reject });
      if (message) this.netPort.postMessage(message);
    });
  }
  async connect({ query, content, playerName, receive, fail, status, channels }) {
    if (!this.bridge?.openSession) throw Error('Online play requires the desktop client.');
    Object.assign(this, { content, playerName, receiveSession: receive, fail, status, channels });
    const params = new URLSearchParams(query);
    this.address ||= params.get('address'); this.port = Number(params.get('port') || this.port);
    this.serverId ||= params.get('server'); this.relay ||= params.get('relay') === 'always';
    if (!/^[a-f0-9]{64}$/.test(this.serverId || '')) throw Error('Invalid server identity');
    this.status('Connecting to server...');
    this.waiters = [];
    this.netPort = await this.requestPort();
    if (this.closed) { this.netPort.close(); return; }
    this.onExit = event => { if (event.source === this.target && event.data?.type === 'bfe:net-exit' && !this.closed) { this.closed = true; this.fail(Error('The desktop network process stopped. Reconnect to rejoin.')); } };
    this.target?.addEventListener?.('message', this.onExit);
    this.netPort.onmessage = event => this.message(event.data);
    this.netPort.start?.();
    for (const label of ['state', 'events', 'bulk']) channels.set(label, { label, readyState: 'connecting', bufferedAmount: 0, send: payload => this.send(label, payload) });
    const hello = { protocol: NETWORK_CONTRACT.protocol, profile: NETWORK_CONTRACT.profile, simulation: NETWORK_CONTRACT.simulation, content, ...(playerName ? { name: playerName } : {}) };
    const connected = this.request(null, 'connected');
    if (this.address) this.netPort.postMessage({ op: 'connect', serverId: this.serverId, candidates: [{ address: this.address, port: this.port, kind: 'host' }], hello });
    else await this.rendezvous(hello);
    const result = await connected;
    const server = result.server?.hello ?? {};
    if (server.protocol !== NETWORK_CONTRACT.protocol || server.simulation !== NETWORK_CONTRACT.simulation || server.profile !== NETWORK_CONTRACT.profile)
      throw Error('Multiplayer version mismatch. Update the desktop client and server to the same release.');
    this.spectator = server.content !== content;
    this.path = { ...result.path, rtt: Math.round(result.rtt ?? 0) };
    for (const channel of channels.values()) channel.readyState = 'open';
    this.connected = true;
    this.status('Authenticated. Loading battle...');
  }
  // ONLINE: our candidates (public host and reflexive addresses, from the
  // network process) to the server through the master; its answer, and any
  // relay candidate that follows, go straight to the network process.
  async rendezvous(hello) {
    const credentials = await this.bridge.request('credentials', this.serverId).catch(error => { if (this.relay) throw error; return null; });
    const relayServer = credentials?.urls?.map(turnServer).find(Boolean) ?? null;
    const relay = this.relay && relayServer ? { ...relayServer, username: credentials.username, password: credentials.credential } : null;
    if (this.relay && !relay) throw Error('No relay is available for this server.');
    const gathered = this.relay ? { candidates: [] } : await this.request({ op: 'gather', stun: relayServer ?? STUN_DEFAULT }, 'gathered');
    const nonce = hex(crypto.getRandomValues(new Uint8Array(16)));
    let started = false;
    this.unsubscribe = this.bridge.onSignal(message => {
      if (message.direct || message.from !== this.serverId || !['answer', 'candidate'].includes(message.type)) return;
      let signal;
      try { signal = JSON.parse(message.payload); } catch { return; }
      if (signal?.udp !== 1 || signal.nonce !== nonce || !Array.isArray(signal.candidates)) return;
      const candidates = signal.candidates.slice(0, 8);
      if (!started) { started = true; this.netPort.postMessage({ op: 'connect', scope: 'online', serverId: this.serverId, candidates, hello, relay }); }
      else this.netPort.postMessage({ op: 'add', candidates });
    });
    await this.bridge.request('connect');
    await this.bridge.request('signal', { to: this.serverId, type: 'offer', payload: JSON.stringify({ udp: 1, nonce, candidates: gathered.candidates }) });
  }
  message(data) {
    if (this.closed || !data || typeof data !== 'object') return;
    if (data.op === 'recv') {
      if (!this.connected || !Array.isArray(data.items)) return;
      for (const [label, payload, at] of data.items) {
        // Receipt times are the network process's, so arrival clocks see
        // when a packet landed, not when this thread got to it.
        const local = Number.isFinite(at) ? Math.min(performance.now(), at - performance.timeOrigin) : undefined;
        this.receiveSession(label, payload, local)?.catch?.(error => this.fail(error));
      }
      return;
    }
    if (data.op === 'stats') {
      this.path = { ...data.path, rtt: Math.round(data.rtt ?? 0), loss: data.loss };
      this.updateStatus();
      return;
    }
    if (data.op === 'closed') { this.closed = true; this.fail(Error(data.reason || 'Connection to server closed')); return; }
    const index = this.waiters.findIndex(waiter => waiter.reply === (data.op === 'error' ? data.for : data.op));
    if (index < 0) { if (data.op === 'error') this.fail(Error(data.message)); return; }
    const [waiter] = this.waiters.splice(index, 1);
    if (data.op === 'error') waiter.reject(Error(data.message)); else waiter.resolve(data);
  }
  // Sends made in one task cross to the network process together, where they
  // are coalesced into as few UDP packets as fit.
  send(label, payload) {
    if (this.closed || !this.connected) return false;
    if (ArrayBuffer.isView(payload)) payload = payload.buffer.slice(payload.byteOffset, payload.byteOffset + payload.byteLength);
    this.outgoing.push([label, payload]);
    if (!this.scheduled) { this.scheduled = true; queueMicrotask(() => this.flush()); }
    return true;
  }
  // Cloned rather than transferred: transferred buffers do not survive the
  // hop into Electron's utility process, and these are at most 1,100 bytes.
  flush() {
    this.scheduled = false;
    if (!this.outgoing.length || this.closed) return;
    const items = this.outgoing; this.outgoing = [];
    this.netPort.postMessage({ op: 'send', items });
  }
  updateStatus() {
    if (!this.path || this.closed) return;
    // The original's CONNECTION column: Direct or Relay, IPv4 or IPv6.
    this.status(`${this.spectator ? 'Spectating (content differs) · ' : ''}${this.path.kind === 'relay' ? 'Relay' : 'Direct'} ${this.path.family || 'IP'} · ${this.path.rtt} ms`);
  }
  async matchReady(query) {
    const target = new URLSearchParams(query), token = target.get('match');
    if (this.rotationCancelled || !/^[a-f0-9]{32}$/.test(token || '')) return false;
    if (this.address) {
      const info = await this.bridge.request('directInfo', { address: this.address, port: this.port });
      const actual = new URLSearchParams(info.query);
      return !this.rotationCancelled && info.id === this.serverId && ['match', 'map', 'side'].every(key => actual.get(key) === target.get(key));
    }
    this.rotationConnect ||= this.bridge.request('connect').catch(error => { this.rotationConnect = null; throw error; });
    await this.rotationConnect;
    if (this.rotationCancelled) { this.bridge.request('close').catch(() => {}); return false; }
    const nonce = hex(crypto.getRandomValues(new Uint8Array(16)));
    return new Promise(resolve => {
      let settled = false;
      const finish = ready => { if (settled) return; settled = true; clearTimeout(timer); unsubscribe(); this.cancelRotationProbe = null; resolve(ready); };
      const unsubscribe = this.bridge.onSignal(message => {
        if (message.direct || message.from !== this.serverId || message.type !== 'matchReady') return;
        try {
          const value = JSON.parse(message.payload);
          if (value.type === 'matchReady' && value.matchReady === token && value.nonce === nonce) finish(true);
        } catch { /* Unrelated signaling does not establish readiness. */ }
      });
      const timer = setTimeout(() => finish(false), 1000);
      this.cancelRotationProbe = () => finish(false);
      this.bridge.request('signal', { to: this.serverId, type: 'matchProbe', payload: JSON.stringify({ type: 'matchProbe', matchProbe: token, nonce }) })
        .then(sent => { if (!sent) { this.rotationConnect = null; finish(false); } }, () => { this.rotationConnect = null; finish(false); });
    });
  }
  cancelMatchProbe() {
    this.rotationCancelled = true; this.cancelRotationProbe?.();
    if (this.rotationConnect) this.bridge.request('close').catch(() => {});
  }
  dispose() {
    if (this.disposed) return;
    this.disposed = true;
    this.target?.removeEventListener?.('message', this.onExit);
    const wasClosed = this.closed;
    this.closed = true;
    this.unsubscribe?.();
    for (const waiter of this.waiters ?? []) waiter.reject(Error('Disconnected from server'));
    this.waiters = [];
    if (this.netPort) { if (!wasClosed) { try { this.netPort.postMessage({ op: 'close', reason: 'left the battle' }); } catch {} } this.netPort.onmessage = null; setTimeout(() => { try { this.netPort.close(); } catch {} }, 100); }
    // Readiness owns its signaling connection once rotation starts.
    if (!this.address && !this.rotationConnect) this.bridge?.request('close').catch(() => {});
  }
}
