// UDP sockets and sessions for BFE-UDP/1. One endpoint owns an IPv4 and an
// IPv6 socket (either may be absent) and demultiplexes everything arriving
// on them: handshakes, session data by connection id (so a session follows
// its peer across NAT rebinding or a Wi-Fi to LTE switch), STUN replies and
// TURN-relayed packets. bfeserver runs a listening endpoint; the Electron
// network process runs a client endpoint per battle.
import dgram from 'node:dgram';
import { EventEmitter } from 'node:events';
import { isIP } from 'node:net';
import { lookup } from 'node:dns/promises';
import { PACKET, MAX_DATAGRAM, normalizeAddress, addressFamily, canonicalAddress } from './codec.js';
import { RetryTokens } from './crypto.js';
import { ClientHandshake, ServerHandshakes, INITIATION_BYTES, CONFIRM_MIN } from './handshake.js';
import { Connection, CLOSE_CODE } from './connection.js';
import { CLASS, METHOD, ATTR, address as stunAddress, decodeMessage, encodeMessage, isChannelData, isStun, newTransaction } from './stun.js';
import { TurnClient } from './turn.js';

export { CLOSE_CODE };
const INITIATION_SCHEDULE = [0, 250, 500, 1000, 1500, 2000, 3000, 4000, 5000, 6000, 8000, 10000, 12000];
const CONFIRM_SCHEDULE = [150, 300, 600, 1000, 1500, 2500, 4000];
const PUNCH_SCHEDULE = [0, 40, 100, 200, 400, 800, 1500];
// Candidate addresses one connect tries, and RETRY tokens accepted per path.
const MAX_PATHS = 8, MAX_RETRIES = 2, MAX_DRAINING = 256;
// Racing local ports send only these first initiations: the race is decided
// within a round trip, and a malicious server's candidate list gains little.
const RACE_SCHEDULE = [0, 250, 500], MAX_PORTS = 8;
// An asynchronous admission (policy kept on another thread) that has not
// answered by then refuses the join.
const ADMIT_TIMEOUT = 5000;
// What this transport supports beyond the base protocol, in every server
// hello. `probing`: challenges are answered on the path they arrive on and
// never move the session, so a client may time its other ports.
const FEATURES = Object.freeze({ probing: 1 });
// Port probing after a raced connect: probes are cheap where handshakes are
// not, so more ports are timed than raced (up to PROBE_PORTS in all), in
// PROBE_ROUNDS staggered rounds; the session moves when another port beats
// its own by PROBE_MARGIN ms. Six raced ports alone missed the fastest
// route class in 3 to 6 of 10 lab sessions; sixteen probed ones leave about
// 1 in 200 with that path mix.
const PROBE_PORTS = 16, PROBE_ROUNDS = 4, PROBE_SPACING = 60, PROBE_MARGIN = 1;
// Source paths a socket keeps built (address and port strings, keys), so a
// busy server does not rebuild them for every datagram.
const MAX_CACHED_PATHS = 4096, DEFAULT_SOCKET_BUFFER = 4 << 20;
// Each short buffer is reported once per process.
const bufferWarnings = new Set();

// Keys name the local port too: a client may race one server address from
// several local ports (multipath routing gives their flows different round
// trips). A server's sockets share one port, so its keys are unchanged.
export const directPath = (socket, family, address, port) => ({ key: `direct|${socket?.localPort ?? ''}|${normalizeAddress(address)}|${port}`, remote: `direct|${normalizeAddress(address)}|${port}`, kind: 'direct', family, address: normalizeAddress(address), port, socket });
const relayPath = (relay, address, port) => ({ key: `relay:${relay.id}|${normalizeAddress(address)}|${port}`, kind: 'relay', family: addressFamily(address), address: normalizeAddress(address), port, relay });
export const publicPath = path => path && { kind: path.kind, family: path.family, address: path.address, port: path.port, ...(path.socket?.localPort ? { localPort: path.socket.localPort } : {}) };

// Rejects addresses no game peer can have: unspecified, broadcast, multicast.
export function validPeerAddress(address) {
  const value = normalizeAddress(address), family = isIP(value.split('%')[0]);
  if (!family) return false;
  if (family === 4) { const [a, , , d] = value.split('.').map(Number); return a !== 0 && a < 224 && !(a === 255 && d === 255); }
  const lower = value.toLowerCase();
  return lower !== '::' && !lower.startsWith('ff');
}

// Rate limits and caps apply per network: one IPv4 address, or one IPv6
// /64 (a single host commonly holds a whole /64).
export function addressPrefix(address) {
  const value = normalizeAddress(address).split('%')[0].toLowerCase();
  if (isIP(value) !== 6) return value;
  const width = parts => parts.reduce((n, part) => n + (part.includes('.') ? 2 : 1), 0);
  const [head, tail] = value.split('::'), left = head ? head.split(':') : [];
  const right = tail === undefined ? [] : tail ? tail.split(':') : [];
  const groups = tail === undefined ? left : [...left, ...Array(Math.max(0, 8 - width(left) - width(right))).fill('0'), ...right];
  return groups.slice(0, 4).map(group => group.replace(/^0+(?=.)/, '')).join(':') + '::/64';
}
const loopbackAddress = address => { const value = normalizeAddress(address); return value === '::1' || value.startsWith('127.'); };
// Internet addresses, where flows can hash onto routes of different length.
// Loopback, private, shared (CGNAT) and link-local ones are on this network.
export function globalAddress(address) {
  const value = normalizeAddress(address).split('%')[0].toLowerCase(), family = isIP(value);
  if (family === 4) {
    const [a, b] = value.split('.').map(Number);
    return !(a === 10 || a === 127 || a === 0 || (a === 100 && b >= 64 && b < 128) || (a === 169 && b === 254) || (a === 172 && b >= 16 && b < 32) || (a === 192 && b === 168) || a >= 224);
  }
  return family === 6 && !/^(::1?$|f[cd]|fe[89ab]|ff)/.test(value);
}

export class Session extends EventEmitter {
  constructor(endpoint, connection, { role, remote }) {
    super();
    this.endpoint = endpoint; this.connection = connection; this.role = role;
    this.id = remote.id; this.publicKey = remote.publicKey; this.hello = remote.hello;
    this.closed = false; this.ready = role === 'server';
    // A client session's first messages can share the packet that proves the
    // session is up; hold them until the caller has attached its listeners.
    this.held = role === 'client' ? [] : null;
  }
  resume() {
    const held = this.held;
    this.held = null;
    for (const [lane, payload] of held ?? []) { if (this.closed && !this.listenerCount('message')) break; this.emit('message', lane, payload); }
  }
  get path() { return publicPath(this.connection.path); }
  get rtt() { return this.connection.rtt; }
  get localSession() { return this.connection.localSession; }
  send(lane, payload) { return !this.closed && this.connection.send(lane, payload); }
  flush() { this.connection.flush(); }
  stats() { return { ...this.connection.stats(), path: this.path }; }
  close(code = CLOSE_CODE.NORMAL, reason = '') { this.connection.close(code, reason); }
}

export class UdpEndpoint extends EventEmitter {
  // identity: identityKeys(). server: {hello(), admit({client, path}) ->
  // {accept} | {code, reason}} for a listening endpoint, null for a client.
  // maxSessionsPerPrefix caps open sessions from one network (loopback
  // exempt), so free identities cannot take every seat from one address.
  // raceAddress picks the candidates a port race is worth (internet ones).
  constructor({ identity, server = null, connection = {}, now = () => performance.now(), maxSessions = 512, maxSessionsPerPrefix = Infinity, handshakeRate = 10, raceAddress = globalAddress } = {}) {
    super();
    Object.assign(this, { identity, server, connectionOptions: connection, now, maxSessions, maxSessionsPerPrefix, handshakeRate, raceAddress });
    this.sockets = new Map(); this.sessions = new Map(); this.connecting = new Map(); this.stunWaiters = new Map();
    this.relays = new Map(); this.relayId = 0; this.buckets = new Map(); this.closed = false; this.live = 0; this.draining = 0; this.extraSockets = new Set();
    this.counters = { received: 0, sent: 0, unknownSession: 0, strayReplies: 0, rateLimited: 0, malformed: 0, sendErrors: 0, initiations: 0 };
    this.initiationWindow = { at: 0, count: 0 }; this.socketBuffers = {};
    if (server) {
      this.handshakes = new ServerHandshakes({ identity, hello: () => ({ ...server.hello(), transport: FEATURES }), retry: new RetryTokens(), sessions: this.sessions });
      this.expiry = setInterval(() => this.handshakes.expire(), 2000); this.expiry.unref?.();
    }
  }

  // Binds IPv4 and IPv6 sockets on one port (port 0 picks one). A host
  // address binds only its own family. Missing IPv6 support is not fatal.
  async listen({ port = 0, host = null, families = ['IPv4', 'IPv6'], receiveBuffer, sendBuffer } = {}) {
    const wanted = host ? [isIP(host) === 6 ? 'IPv6' : 'IPv4'] : families;
    let bound = port, firstError = null;
    for (const family of wanted) {
      let socket;
      try { socket = await this.bindSocket(family, { port: bound, host, receiveBuffer, sendBuffer }); }
      catch (error) { firstError ??= error; continue; }
      bound = socket.localPort;
      this.sockets.set(family, socket);
      this.reportBuffers(family, socket, { receiveBuffer, sendBuffer });
    }
    if (!this.sockets.size) throw firstError ?? Error('No UDP socket could be bound');
    this.port = bound;
    return { port: bound, families: [...this.sockets.keys()] };
  }
  // Linux clamps socket buffers to net.core.rmem_max / wmem_max (and reports
  // double the size it grants); a listening server says when it got less
  // than it asked for, since a short receive buffer drops bursts.
  reportBuffers(family, socket, { receiveBuffer = DEFAULT_SOCKET_BUFFER, sendBuffer = DEFAULT_SOCKET_BUFFER } = {}) {
    for (const [name, size, get] of [['receive', receiveBuffer, socket.getRecvBufferSize], ['send', sendBuffer, socket.getSendBufferSize]]) {
      let actual;
      try { actual = get.call(socket); } catch { continue; }
      this.socketBuffers[`${family} ${name}`] = actual;
      const warning = `${family} ${name} ${size}`;
      if (actual >= size || bufferWarnings.has(warning)) continue;
      bufferWarnings.add(warning);
      console.warn(`UDP ${family} ${name} buffer is ${actual} bytes, below the ${size} requested; raise net.core.${name === 'receive' ? 'rmem_max' : 'wmem_max'} to avoid dropped datagrams.`);
    }
  }
  async bindSocket(family, { port = 0, host = null, receiveBuffer = DEFAULT_SOCKET_BUFFER, sendBuffer = DEFAULT_SOCKET_BUFFER } = {}) {
    const socket = dgram.createSocket({ type: family === 'IPv6' ? 'udp6' : 'udp4', ipv6Only: family === 'IPv6' });
    try {
      await new Promise((resolve, reject) => { socket.once('error', reject); socket.bind({ port, address: host ?? (family === 'IPv6' ? '::' : '0.0.0.0') }, () => { socket.off('error', reject); resolve(); }); });
    } catch (error) { try { socket.close(); } catch {} throw error; }
    socket.localPort = socket.address().port;
    for (const [set, size] of [[socket.setRecvBufferSize, receiveBuffer], [socket.setSendBufferSize, sendBuffer]]) { try { set.call(socket, size); } catch {} }
    socket.on('error', () => { this.counters.sendErrors++; });
    // One path object per source address and port, built once.
    const paths = new Map();
    let cached = 0;
    socket.on('message', (packet, info) => {
      let byPort = paths.get(info.address), path = byPort?.get(info.port);
      if (!path) {
        if (cached >= MAX_CACHED_PATHS) { paths.clear(); cached = 0; byPort = null; }
        if (!byPort) paths.set(info.address, byPort = new Map());
        byPort.set(info.port, path = directPath(socket, family, info.address, info.port));
        cached++;
      }
      this.receive(packet, path);
    });
    return socket;
  }
  closeExtra(socket) { if (socket && this.extraSockets.delete(socket)) { try { socket.close(); } catch {} } }
  socketFor(address) { return this.sockets.get(addressFamily(address)) ?? null; }

  transmit(packet, path) {
    if (!path || this.closed) return;
    this.counters.sent++;
    if (path.relay) { path.relay.sendTo(path.address, path.port, packet); return; }
    try { path.socket.send(packet, path.port, path.address, error => { if (error) this.counters.sendErrors++; }); }
    catch { this.counters.sendErrors++; }
  }

  // ---- Receive and demultiplex ----

  receive(packet, path) {
    if (this.closed || packet.length < 1 || packet.length > 1500) return;
    this.counters.received++;
    const type = packet[0];
    try {
      if (isStun(packet)) { this.stunReply(packet); return; }
      if (type === PACKET.DATA || type === PACKET.DATA_PHASE) {
        if (packet.length < 26) return;
        const session = this.sessions.get(packet.readUInt32LE(1));
        if (!session) { this.counters.unknownSession++; return; }
        if (session.connection.receive(packet, path) && !session.ready) this.ready(session);
        return;
      }
      // Shape first: nothing too short to be a handshake touches any state.
      if (type === PACKET.INITIATION && this.server) { if (packet.length < INITIATION_BYTES) { this.counters.malformed++; return; } return this.initiation(packet, path); }
      if (type === PACKET.CONFIRM && this.server) { if (packet.length < CONFIRM_MIN) { this.counters.malformed++; return; } return this.confirm(packet, path); }
      if ((type === PACKET.RESPONSE || type === PACKET.RETRY || type === PACKET.VERSION) && packet.length >= 6) return this.reply(packet, path);
      if (type === PACKET.PUNCH) return;
      this.counters.malformed++;
    } catch { this.counters.malformed++; }
  }
  // Token buckets per network, least recently used first: a flood of new
  // (perhaps spoofed) sources evicts old buckets instead of locking out
  // newcomers, and the load check below then demands retry tokens.
  allowHandshake(path) {
    const now = this.now(), key = addressPrefix(path.address);
    let bucket = this.buckets.get(key);
    if (bucket) this.buckets.delete(key);
    else {
      bucket = { tokens: this.handshakeRate * 2, at: now };
      if (this.buckets.size >= 4096) this.buckets.delete(this.buckets.keys().next().value);
    }
    this.buckets.set(key, bucket);
    bucket.tokens = Math.min(this.handshakeRate * 2, bucket.tokens + (now - bucket.at) * this.handshakeRate / 1000);
    bucket.at = now;
    if (bucket.tokens < 1) { this.counters.rateLimited++; return false; }
    bucket.tokens--;
    return true;
  }
  underLoad() {
    const now = this.now();
    if (now - this.initiationWindow.at > 1000) this.initiationWindow = { at: now, count: 0 };
    return ++this.initiationWindow.count > 200 || this.handshakes.pending.size > 64;
  }
  initiation(packet, path) {
    if (!this.allowHandshake(path)) return;
    this.counters.initiations++;
    const reply = this.handshakes.initiation(packet, path.key, { requireRetry: this.underLoad(), prefix: addressPrefix(path.address) });
    if (reply) this.transmit(reply, path);
  }
  confirm(packet, path) {
    if (!this.allowHandshake(path)) return;
    const result = this.handshakes.confirm(packet, path.key);
    if (!result) {
      // A retransmitted CONFIRM whose HANDSHAKE_DONE was lost.
      const session = this.sessions.get(packet.readUInt32LE(2));
      if (session?.confirmBytes?.equals(packet) && !session.admitting) session.connection.confirmHandshake();
      return;
    }
    const decision = this.live >= this.maxSessions ? { code: CLOSE_CODE.REJECTED, reason: 'The server is full.' }
      : this.crowded(path, result.client.id) ? { code: CLOSE_CODE.REJECTED, reason: 'Too many players are connected from your network.' }
      : this.server.admit({ client: result.client, path: publicPath(path) }) ?? { accept: true };
    const session = this.createSession('server', result.serverSession, result.clientSession, result, result.client, path, Math.max(1, result.rtt));
    session.confirmBytes = result.confirm;
    const apply = verdict => {
      session.admitting = false;
      if (session.closed) return;
      if (!verdict?.accept) { session.close(verdict?.code ?? CLOSE_CODE.REJECTED, verdict?.reason ?? 'Connection refused'); return; }
      session.connection.confirmHandshake();
      this.emit('session', session, verdict);
    };
    // Admission may answer later (a server whose policy runs on another
    // thread); the session waits, unconfirmed, and refuses after a timeout.
    if (typeof decision?.then !== 'function') { apply(decision); return; }
    session.admitting = true;
    const timer = setTimeout(() => { if (session.admitting) apply({ code: CLOSE_CODE.REJECTED, reason: 'The server did not answer.' }); }, ADMIT_TIMEOUT);
    timer.unref?.();
    decision.then(verdict => { clearTimeout(timer); if (session.admitting) apply(verdict ?? { accept: true }); },
      () => { clearTimeout(timer); if (session.admitting) apply({ code: CLOSE_CODE.REJECTED, reason: 'Connection refused' }); });
  }
  // Open sessions from this source's network, other than this identity's
  // own (a reconnect replaces it).
  crowded(path, id) {
    if (!Number.isFinite(this.maxSessionsPerPrefix) || loopbackAddress(path.address)) return false;
    const prefix = addressPrefix(path.address);
    let count = 0;
    for (const session of this.sessions.values()) if (!session.closed && session.role === 'server' && session.id !== id && session.prefix === prefix && ++count >= this.maxSessionsPerPrefix) return true;
    return false;
  }
  createSession(role, localSession, remoteSession, keys, remote, path, initialRtt) {
    let session;
    const connection = new Connection({ role, localSession, remoteSession, send: keys.send, receive: keys.receive, path, initialRtt, now: this.now,
      ...this.connectionOptions,
      transmit: (packet, current) => this.transmit(packet, current),
      onMessage: (lane, payload) => {
        if (session.held) { if (session.held.length < 4096) session.held.push([lane, payload]); return; }
        try { session.emit('message', lane, payload); }
        catch (error) { connection.close(CLOSE_CODE.PROTOCOL, 'invalid message'); session.emit('fault', error); }
      },
      onPath: (next, previous) => { session.prefix = addressPrefix(next.address); session.emit('path', publicPath(next), publicPath(previous)); },
      onClose: info => {
        session.closed = true; this.live--;
        // Draining: keep the id briefly so stray packets get the CLOSE again
        // (a bounded number at once, so refusals cannot crowd the table).
        // A racing port that won belongs to its session and closes with it.
        const release = () => { if (this.sessions.get(localSession) === session) this.sessions.delete(localSession); this.closeExtra(connection.path?.socket); };
        if (this.draining >= MAX_DRAINING) release();
        else {
          this.draining++;
          setTimeout(() => { this.draining--; release(); }, 2000).unref?.();
        }
        session.emit('close', info);
      },
    });
    session = new Session(this, connection, { role, remote });
    session.prefix = addressPrefix(path.address);
    this.sessions.set(localSession, session); this.live++;
    return session;
  }

  // ---- Client ----

  // Opens a session to the server whose identity is `serverId`, trying every
  // candidate path at once (the first authenticated reply wins). `relay`
  // routes every attempt through this client's own TURN allocation. The
  // promise has addCandidates() for addresses that arrive later (trickle).
  // `ports` > 1 races the first initiations to internet candidates from
  // that many local ports (see RACE_SCHEDULE); the winning port stays with
  // its session and the others close.
  connect({ serverId, candidates, hello = {}, timeoutMs = 15000, relay = null, ports = 1 }) {
    if (this.closed) return Promise.reject(Error('Endpoint closed'));
    const usable = list => (Array.isArray(list) ? list : []).filter(candidate => validPeerAddress(candidate?.address) && Number.isInteger(candidate.port) && candidate.port > 0 && candidate.port < 65536)
      .map(candidate => ({ ...candidate, address: canonicalAddress(candidate.address) }));
    const initial = usable(candidates).slice(0, MAX_PATHS);
    if (!initial.length) return Promise.reject(Error('No usable server address'));
    let handshake;
    try { handshake = new ClientHandshake({ identity: this.identity, hello, expectedServerId: serverId }); }
    catch (error) { return Promise.reject(error); }
    let attempt;
    const promise = new Promise((resolve, reject) => {
      attempt = { handshake, resolve, reject, timers: new Set(), started: this.now(), paths: [], candidates: [], extras: [] };
      const later = (fn, ms) => { const timer = setTimeout(() => { attempt.timers.delete(timer); fn(); }, ms); attempt.timers.add(timer); };
      attempt.later = later;
      attempt.finish = (error, session) => {
        if (attempt.done) return;
        attempt.done = true;
        for (const timer of attempt.timers) clearTimeout(timer);
        this.connecting.delete(handshake.clientSession);
        // A server that answers probes lets the raced ports be timed properly
        // (a handshake reply is one sample, and its crypto skews the order);
        // otherwise racing ports the session does not use close now.
        if (!error && session && attempt.extras.length && session.hello?.transport?.probing) this.optimizePath(session, attempt.extras);
        else for (const { socket } of attempt.extras) if (socket !== session?.connection.path?.socket || error) this.closeExtra(socket);
        if (error) { session?.close(CLOSE_CODE.NORMAL, 'connect failed'); reject(error); } else resolve(session);
      };
      const pathFor = (candidate, socket) => {
        const state = { token: undefined, retries: 0, failed: null };
        if (relay) return { ...relayPath(relay, candidate.address, candidate.port), candidateKind: 'relay', ...state };
        return socket ? { ...directPath(socket, addressFamily(candidate.address), candidate.address, candidate.port), candidateKind: candidate.kind === 'relay' ? 'relay' : 'direct', ...state } : null;
      };
      // Each path gets the initiation at once, then again on a backoff
      // schedule: the lowest-latency working path answers first. Direct
      // paths lead relayed ones by a moment, so a relay wins only when needed.
      // Paths to one candidate send together, so a port race is fair; a
      // candidate's schedule runs from when it arrived (trickled ones later).
      const start = (path, candidate, schedule) => {
        const offset = (path.candidateKind === 'relay' ? 60 : 0) + attempt.candidates.indexOf(candidate) * 3, from = this.now() - candidate.addedAt;
        for (const delay of schedule) if (delay + offset < timeoutMs) later(() => { if (!attempt.session && !path.failed) this.transmit(handshake.initiation(path.token), path); }, Math.max(0, delay + offset - from));
      };
      const race = (candidate, extra) => {
        if (attempt.session || attempt.done || addressFamily(candidate.address) !== extra.family) return;
        const path = pathFor(candidate, extra.socket);
        if (!path || attempt.paths.some(existing => existing.key === path.key)) return;
        attempt.paths.push(path);
        start(path, candidate, RACE_SCHEDULE);
      };
      attempt.add = list => {
        for (const candidate of usable(list)) {
          if (attempt.candidates.length >= MAX_PATHS || attempt.session || attempt.done) break;
          if (attempt.candidates.some(existing => existing.address === candidate.address && existing.port === candidate.port)) continue;
          const path = pathFor(candidate, this.socketFor(candidate.address));
          if (!path) continue;
          candidate.addedAt = this.now();
          attempt.candidates.push(candidate); attempt.paths.push(path);
          start(path, candidate, INITIATION_SCHEDULE);
          if (this.raceAddress(candidate.address)) for (const extra of attempt.extras) race(candidate, extra);
        }
      };
      // A path that answered with another version or another server's
      // identity is out; the attempt ends early only when every path is.
      attempt.fail = (path, error) => {
        path.failed = error;
        if (attempt.paths.every(item => item.failed)) attempt.finish(error);
      };
      this.connecting.set(handshake.clientSession, attempt);
      let begun = false;
      const begin = () => {
        if (begun || attempt.done) return;
        begun = true;
        attempt.add(initial);
        if (!attempt.paths.length) { attempt.finish(Error('No socket for the server address family')); return; }
        later(() => attempt.finish(attempt.paths.find(path => path.failed)?.failed ?? Error('Server connection timed out')), timeoutMs);
      };
      // Racing ports bind first (about a millisecond), so none starts ahead;
      // one that binds late still joins the race for every candidate.
      const families = [...new Set(initial.filter(candidate => this.raceAddress(candidate.address)).map(candidate => addressFamily(candidate.address)))].filter(family => this.sockets.has(family));
      if (relay || ports < 2 || !families.length) { begin(); return; }
      const bindings = families.flatMap(family => Array.from({ length: Math.min(ports, MAX_PORTS) - 1 }, () => this.bindSocket(family).then(socket => {
        if (attempt.done || this.closed) { try { socket.close(); } catch {} return; }
        this.extraSockets.add(socket);
        const extra = { family, socket };
        attempt.extras.push(extra);
        for (const candidate of attempt.candidates) if (this.raceAddress(candidate.address)) race(candidate, extra);
      }, () => {})));
      Promise.all(bindings).then(begin);
      setTimeout(begin, 50);
    });
    promise.addCandidates = list => attempt?.add?.(list);
    promise.cancel = () => attempt?.finish?.(Error('Connection cancelled'));
    return promise;
  }
  reply(packet, path) {
    const attempt = this.connecting.get(packet.readUInt32LE(2));
    if (!attempt || attempt.done) return;
    // Only a path this attempt sent to may answer it. Replies (RETRY and
    // VERSION are unauthenticated) from anywhere else could otherwise steer
    // 1200-byte initiations at a third party or end the attempt.
    const chosen = attempt.paths.find(candidate => candidate.key === path.key);
    if (!chosen) { this.counters.strayReplies++; return; }
    const result = attempt.handshake.receive(packet);
    if (!result) return;
    if (result.type === 'retry') {
      // The token belongs to this path (it binds the address the server saw).
      if (attempt.session || chosen.failed || chosen.retries >= MAX_RETRIES) return;
      chosen.retries++; chosen.token = result.token;
      this.transmit(attempt.handshake.initiation(chosen.token), chosen);
      return;
    }
    if (result.type === 'version') { attempt.fail(chosen, Error(`Update required: this server uses network transport version ${result.version}.`)); return; }
    if (result.type === 'error') { attempt.fail(chosen, result.error); return; }
    if (result.type === 'duplicate') { if (attempt.session && !attempt.session.ready) this.transmit(result.confirm, attempt.session.connection.path); return; }
    // The first authentic RESPONSE fixes the path; confirm on it until the
    // server's first packet shows the session is up on both ends.
    const handshakeRtt = this.now() - attempt.started;
    const session = this.createSession('client', result.clientSession, result.serverSession, result, result.server, { ...chosen, kind: chosen.candidateKind ?? chosen.kind }, Math.max(1, Math.min(handshakeRtt, 1000)));
    attempt.session = session;
    session.attempt = attempt;
    const confirm = () => { if (!session.ready && !session.closed) this.transmit(result.confirm, session.connection.path); };
    confirm();
    for (const delay of CONFIRM_SCHEDULE) attempt.later(confirm, delay);
    session.once('close', info => { if (!session.ready) attempt.finish(Error(info.reason || 'Server closed the connection')); });
  }
  // Times every raced port to the session's server with a few probes each
  // (staggered, minimum per port) and moves the session to the fastest when
  // it beats the current port by PROBE_MARGIN. The server validates the new
  // address before sending there; the old port stays open meanwhile.
  async optimizePath(session, extras) {
    const connection = session.connection, current = connection.path, primary = this.sockets.get(current.family);
    // More ports than raced, bound fresh: they only exchange probes.
    const owned = extras.filter(extra => extra.family === current.family).length + 1;
    const fresh = await Promise.all(Array.from({ length: Math.max(0, PROBE_PORTS - owned) }, () => this.bindSocket(current.family).then(socket => {
      if (connection.closed || this.closed) { try { socket.close(); } catch {} return null; }
      this.extraSockets.add(socket);
      return { family: current.family, socket };
    }, () => null)));
    extras = [...extras, ...fresh.filter(Boolean)];
    if (connection.closed) { for (const { socket } of extras) this.closeExtra(socket); return; }
    const others = [{ family: current.family, socket: primary }, ...extras].filter(item => item.socket && item.socket !== current.socket && item.family === current.family);
    const routes = [current, ...others.map(item => ({ ...directPath(item.socket, item.family, current.address, current.port), kind: current.kind }))];
    const samples = new Map(routes.map(route => [route.key, []]));
    connection.onProbe = (route, ms) => samples.get(route.key)?.push(ms);
    const timers = [];
    routes.forEach((route, index) => { for (let round = 0; round < PROBE_ROUNDS; round++) timers.push(setTimeout(() => connection.probePath(route), round * PROBE_SPACING + index * PROBE_SPACING / routes.length)); });
    const settle = setTimeout(() => {
      connection.onProbe = null;
      const measured = routes.map(route => ({ route, min: Math.min(...samples.get(route.key)) })).filter(item => Number.isFinite(item.min));
      const here = measured.find(item => item.route === current), best = measured.reduce((a, b) => (b.min < a.min ? b : a), here ?? measured[0]);
      const move = !connection.closed && best && best.route !== current && (!here || best.min < here.min - PROBE_MARGIN);
      session.probeReport = { ports: measured.map(item => ({ port: item.route.socket.localPort, ms: +item.min.toFixed(2) })), moved: Boolean(move) };
      if (move) connection.usePath(best.route);
      const keep = connection.path.socket;
      for (const { socket } of extras) if (socket !== keep && socket !== current.socket) this.closeExtra(socket);
      // The old port hears the server until it has validated the new one.
      if (move) setTimeout(() => { if (connection.path.socket !== current.socket) this.closeExtra(current.socket); }, 3000).unref?.();
      this.emit('path-probed', session, session.probeReport);
    }, PROBE_ROUNDS * PROBE_SPACING + 250);
    session.once('close', () => { for (const timer of [...timers, settle]) clearTimeout(timer); connection.onProbe = null; for (const { socket } of extras) if (socket !== connection.path?.socket) this.closeExtra(socket); });
  }
  ready(session) {
    session.ready = true;
    session.attempt?.finish(null, session);
    session.attempt = null;
  }

  // ---- NAT traversal ----

  // Server side of hole punching: small packets toward a client's candidate
  // addresses open this host's NAT and firewall to the client's initiation.
  punch(candidates) {
    const targets = (candidates || []).filter(candidate => validPeerAddress(candidate?.address) && Number.isInteger(candidate.port)).slice(0, 8);
    const packet = Buffer.alloc(9); packet[0] = PACKET.PUNCH;
    for (const delay of PUNCH_SCHEDULE) setTimeout(() => {
      if (this.closed) return;
      for (const candidate of targets) { const socket = this.socketFor(candidate.address); if (socket) this.transmit(packet, directPath(socket, addressFamily(candidate.address), candidate.address, candidate.port)); }
    }, delay).unref?.();
  }
  // Reflexive addresses (this host as the internet sees it) from a STUN
  // server, per bound family: {IPv4: {address, port}, IPv6: ...}.
  async reflexive(server, { timeoutMs = 2000 } = {}) {
    const out = {};
    await Promise.all([...this.sockets.entries()].map(async ([family, socket]) => {
      let target;
      try { target = isIP(server.host) ? server.host : (await lookup(server.host, { family: family === 'IPv6' ? 6 : 4 })).address; } catch { return; }
      const transaction = newTransaction(), key = transaction.toString('hex');
      const packet = encodeMessage(METHOD.BINDING, CLASS.REQUEST, transaction);
      const answer = await new Promise(resolve => {
        const timers = [];
        this.stunWaiters.set(key, message => { for (const timer of timers) clearTimeout(timer); this.stunWaiters.delete(key); resolve(message); });
        for (const delay of [0, 250, 750]) timers.push(setTimeout(() => this.transmit(packet, directPath(socket, family, target, server.port)), delay));
        timers.push(setTimeout(() => { this.stunWaiters.delete(key); resolve(null); }, timeoutMs));
      });
      const mapped = answer && stunAddress(answer, ATTR.XOR_MAPPED_ADDRESS);
      if (mapped) out[family] = { address: normalizeAddress(mapped.address), port: mapped.port };
    }));
    return out;
  }
  stunReply(packet) {
    const message = decodeMessage(packet);
    if (message && message.klass !== CLASS.REQUEST) this.stunWaiters.get(message.transaction.toString('hex'))?.(message);
  }

  // A TURN allocation on a socket of its own (one allocation per local
  // port): {relayed, permit(addresses), close()}. Packets peers send to the
  // relayed address arrive here as sessions on a `relay` path.
  async allocateRelay({ host, port = 3478, username, password, family = 'IPv4', lifetime = 600, refresh = {} }) {
    const target = isIP(host) ? host : (await lookup(host, { family: family === 'IPv6' ? 6 : 4 })).address;
    const socket = dgram.createSocket({ type: family === 'IPv6' ? 'udp6' : 'udp4', ipv6Only: family === 'IPv6' });
    await new Promise((resolve, reject) => { socket.once('error', reject); socket.bind(0, () => { socket.off('error', reject); resolve(); }); });
    socket.on('error', () => { this.counters.sendErrors++; });
    const relay = { id: ++this.relayId, socket, family };
    const turn = new TurnClient({ server: { address: normalizeAddress(target), port }, username, password, family, ...refresh,
      send: (packet, toPort, toAddress) => { try { socket.send(packet, toPort, toAddress, () => {}); } catch { this.counters.sendErrors++; } },
      onData: (peerAddress, peerPort, data) => this.receive(Buffer.from(data), relayPath(relay, peerAddress, peerPort)),
      onExpired: error => this.emit('relay-expired', relay, error) });
    relay.turn = turn;
    relay.sendTo = (peerAddress, peerPort, data) => turn.sendTo(peerAddress, peerPort, data);
    // An allocation relays only to its own address family (RFC 6156): one
    // peer of the other family would fail the whole CreatePermission (443).
    relay.permit = addresses => turn.permit(addresses.filter(address => validPeerAddress(address) && addressFamily(address) === family));
    relay.close = () => { turn.close(); this.relays.delete(relay.id); try { socket.close(); } catch {} };
    socket.on('message', (packet, info) => {
      if (normalizeAddress(info.address) !== turn.server.address || info.port !== port || !(isStun(packet) || isChannelData(packet))) return;
      // A malformed relay datagram is dropped; it must never reach the
      // socket's event loop as an exception (that would end the process).
      try { turn.handle(packet); } catch { this.counters.malformed++; }
    });
    try { Object.assign(relay, await turn.allocate(lifetime)); }
    catch (error) { relay.close(); throw error; }
    this.relays.set(relay.id, relay);
    return relay;
  }

  stats() { return { ...this.counters, socketBuffers: this.socketBuffers, sessions: this.sessions.size, connecting: this.connecting.size, pendingHandshakes: this.handshakes?.pending.size ?? 0, relays: this.relays.size, port: this.port }; }
  close() {
    if (this.closed) return;
    for (const session of this.sessions.values()) session.close(CLOSE_CODE.SHUTDOWN, 'Server shutting down');
    for (const attempt of this.connecting.values()) attempt.finish(Error('Endpoint closed'));
    for (const relay of this.relays.values()) relay.close();
    clearInterval(this.expiry);
    this.closed = true;
    for (const socket of [...this.sockets.values(), ...this.extraSockets]) { try { socket.close(); } catch {} }
    this.sockets.clear(); this.extraSockets.clear();
  }
}

export { MAX_DATAGRAM };
