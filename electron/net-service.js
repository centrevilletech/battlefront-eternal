// The network process's work, independent of Electron: one client UDP
// endpoint shared by every battle (a stable local port keeps NAT mappings),
// and one session per renderer channel. `channel` is {post(message),
// onMessage(handler), onClose(handler), close()}; net-process.js adapts
// Electron's MessagePortMain to it and tests use in-memory channels.
import { networkInterfaces } from 'node:os';
import { isIP } from 'node:net';
import { UdpEndpoint } from '../src/net/endpoint.js';
import { identityKeys } from '../src/net/crypto.js';
import { CLOSE_CODE } from '../src/net/connection.js';
import { normalizeAddress } from '../src/net/codec.js';

const LANES = new Set(['state', 'events', 'bulk']);
// The page is the less trusted side of this boundary: a compromised
// renderer must not turn this process into a UDP flooder or scanner. Open
// battle ports, connects and gathers are bounded (a burst of six, then one
// every few seconds), each port connects once, and an online join's server
// candidates exclude this machine's own and link-local addresses.
const MAX_CHANNELS = 4, BURST = 6, REFILL_PER_SECOND = 0.3, MAX_PRIVATE_ONLINE = 2;
// Routers spread flows over parallel routes by hashing their ports, and the
// routes can differ by many milliseconds (one measured ISP path to a server:
// 17.5, 23.5 or 29.5 ms by source port, while ICMP got the fastest). Each
// join races its first initiations from this many local ports and keeps the
// fastest; with that path mix, 6 ports miss the fastest route about 3% of
// the time.
const RACE_PORTS = 6;
const PRIVATE_IPV4 = /^(10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.|100\.(6[4-9]|[7-9]\d|1[01]\d|12[0-7])\.)/;
function bucket(now = () => performance.now()) {
  let tokens = BURST, at = now();
  return () => {
    const time = now();
    tokens = Math.min(BURST, tokens + (time - at) / 1000 * REFILL_PER_SECOND); at = time;
    if (tokens < 1) return false;
    tokens--;
    return true;
  };
}
// Candidates a server sent through the master: no loopback, unspecified or
// link-local address, and at most two private ones (a server on the
// player's own network lists those last).
export function onlineCandidates(list, state = { private: 0 }) {
  return (Array.isArray(list) ? list : []).filter(candidate => {
    const address = normalizeAddress(String(candidate?.address ?? '')).split('%')[0].toLowerCase(), family = isIP(address);
    if (!family) return false;
    if (family === 4 ? /^(127\.|0\.|169\.254\.)/.test(address) : address === '::1' || address === '::' || /^fe[89ab]/.test(address)) return false;
    if (family === 4 ? PRIVATE_IPV4.test(address) : /^f[cd]/.test(address)) return ++state.private <= MAX_PRIVATE_ONLINE;
    return true;
  }).slice(0, 8);
}
const relayRequest = relay => {
  const host = String(relay.host ?? ''), port = Number(relay.port ?? 3478);
  if (!(isIP(host) || /^[a-z0-9.-]{1,253}$/i.test(host)) || !Number.isInteger(port) || port < 1 || port > 65535) throw Error('Invalid relay');
  return { host, port, username: String(relay.username ?? '').slice(0, 512), password: String(relay.password ?? '').slice(0, 512), family: relay.family === 'IPv6' ? 'IPv6' : 'IPv4' };
};
// Binary from the renderer may arrive as an ArrayBuffer, a typed array or a
// Buffer depending on the IPC path (Electron's utility-process ports differ
// from structured cloning in one process). Accept every binary form.
const binary = value => Object.prototype.toString.call(value) === '[object ArrayBuffer]' ? Buffer.from(value)
  : ArrayBuffer.isView(value) ? Buffer.from(value.buffer, value.byteOffset, value.byteLength) : null;
const clock = () => performance.timeOrigin + performance.now();

// candidateFilter screens online candidates (tests stand loopback in for
// public addresses).
export function createNetService({ identity: keys = null, listen = { port: 0 }, now, candidateFilter = onlineCandidates } = {}) {
  let identity = keys ? identityKeys(keys) : null, endpoint = null, binding = null;
  const limits = { channels: new Set(), connect: bucket(now), gather: bucket(now), candidateFilter };
  async function ready() {
    if (!identity) throw Error('Network identity unavailable');
    if (endpoint) return endpoint;
    binding ??= (async () => {
      const created = new UdpEndpoint({ identity });
      await created.listen(listen);
      endpoint = created;
      return created;
    })().finally(() => { binding = null; });
    return binding;
  }
  return {
    setIdentity(value) { identity ??= identityKeys(value); },
    serve(channel) {
      if (limits.channels.size >= MAX_CHANNELS) {
        channel.post({ op: 'error', for: 'connected', message: 'Too many open network sessions.' });
        try { channel.close(); } catch {}
        return;
      }
      limits.channels.add(channel);
      serve(channel, ready, limits);
    },
    get endpoint() { return endpoint; },
    close() { endpoint?.close(); endpoint = null; },
  };
}

// Public interface addresses only: a private LAN address means nothing to
// an online server, and the master should not learn it.
function hostCandidates(port, families) {
  const out = [];
  for (const entries of Object.values(networkInterfaces())) for (const entry of entries ?? []) {
    if (entry.internal || !families.includes(entry.family)) continue;
    const address = entry.address;
    const privateAddress = entry.family === 'IPv4'
      ? /^(10\.|127\.|169\.254\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.|100\.(6[4-9]|[7-9]\d|1[01]\d|12[0-7])\.)/.test(address)
      : /^(f[cd]|fe[89ab])/i.test(address);
    if (!privateAddress) out.push({ address, port, kind: 'host' });
  }
  return out.slice(0, 4);
}

function serve(port, ready, limits) {
  let session = null, connecting = null, relay = null, closed = false, statsTimer = null, attempted = false, gathered = false, scope = null;
  const privateCandidates = { private: 0 };
  const candidatesFor = list => scope === 'online' ? limits.candidateFilter(list, privateCandidates) : list;
  let outgoing = [], scheduled = false;
  const post = message => { if (!closed) port.post(message); };
  // Messages arriving in one socket callback share one port message.
  const flush = () => { scheduled = false; if (outgoing.length) { const items = outgoing; outgoing = []; post({ op: 'recv', items }); } };
  const finish = info => {
    if (closed) return;
    flush();
    post({ op: 'closed', ...info });
    closed = true; clearInterval(statsTimer); connecting?.cancel?.(); relay?.close(); relay = null;
    limits.channels.delete(port);
    try { port.close(); } catch {}
  };
  const stats = () => {
    if (!session) return;
    const value = session.stats();
    post({ op: 'stats', path: value.path, rtt: value.rtt, minRtt: value.minRtt, loss: value.loss, packetsLost: value.packetsLost, packetsSent: value.packetsSent, packetsReceived: value.packetsReceived });
  };
  async function connect(request) {
    if (attempted) throw Error('Session already open');
    attempted = true;
    if (!limits.connect()) throw Error('Too many connection attempts. Wait a moment and try again.');
    scope = request.scope === 'online' ? 'online' : 'direct';
    const candidates = candidatesFor(request.candidates);
    const timeoutMs = Math.min(30000, Math.max(2000, Number(request.timeoutMs) || 15000));
    const local = await ready();
    if (request.relay) {
      relay = await local.allocateRelay(relayRequest(request.relay));
      await relay.permit(candidates.map(candidate => candidate.address));
    }
    connecting = local.connect({ serverId: request.serverId, candidates, hello: request.hello, timeoutMs, relay, ports: RACE_PORTS });
    try { session = await connecting; } finally { connecting = null; }
    if (closed) { session.close(CLOSE_CODE.NORMAL, 'closed'); return; }
    session.on('message', (lane, payload) => {
      outgoing.push([lane, typeof payload === 'string' ? payload : payload.buffer.slice(payload.byteOffset, payload.byteOffset + payload.byteLength), clock()]);
      if (!scheduled) { scheduled = true; queueMicrotask(flush); }
    });
    session.once('close', info => finish(info));
    post({ op: 'connected', server: { id: session.id, publicKey: session.publicKey, hello: session.hello }, path: session.path, rtt: session.rtt });
    session.resume();
    statsTimer = setInterval(stats, 1000);
  }
  port.onMessage(async data => {
    if (closed || !data || typeof data !== 'object') return;
    try {
      if (data.op === 'send') {
        if (!session || !Array.isArray(data.items) || data.items.length > 256) return;
        for (const [lane, payload] of data.items) {
          if (!LANES.has(lane)) continue;
          if (typeof payload === 'string') { if (payload.length <= 262144) session.send(lane, payload); continue; }
          const bytes = binary(payload);
          if (bytes && bytes.length <= 262144) session.send(lane, bytes);
        }
      } else if (data.op === 'connect') {
        await connect(data).catch(error => post({ op: 'error', for: 'connected', message: String(error?.message ?? error).slice(0, 240) }));
      } else if (data.op === 'gather') {
        if (gathered || !limits.gather()) throw Error('Too many connection attempts. Wait a moment and try again.');
        gathered = true;
        const local = await ready(), families = [...local.sockets.keys()];
        const candidates = hostCandidates(local.port, families);
        if (data.stun?.host) {
          const reflexive = await local.reflexive({ host: String(data.stun.host), port: Number(data.stun.port) || 3478 }).catch(() => ({}));
          for (const entry of Object.values(reflexive)) if (!candidates.some(c => c.address === entry.address && c.port === entry.port)) candidates.push({ ...entry, kind: 'srflx' });
        }
        post({ op: 'gathered', candidates });
      } else if (data.op === 'add') {
        connecting?.addCandidates?.(candidatesFor(data.candidates));
      } else if (data.op === 'close') {
        const reason = String(data.reason ?? 'closed').slice(0, 200);
        if (session) session.close(CLOSE_CODE.NORMAL, reason); else finish({ code: CLOSE_CODE.NORMAL, reason, remote: false });
      }
    } catch (error) { post({ op: 'error', for: data.op === 'gather' ? 'gathered' : data.op, message: String(error?.message ?? error).slice(0, 240) }); }
  });
  port.onClose(() => { limits.channels.delete(port); if (!closed) { closed = true; clearInterval(statsTimer); connecting?.cancel?.(); session?.close(CLOSE_CODE.NORMAL, 'closed'); relay?.close(); } });
}
