// Desktop owns identity and master access. Game sessions (native UDP) run in
// the network utility process, which holds the identity key; the renderer
// never receives it and has nothing of the player's to sign.
import { createHash, generateKeyPairSync, randomBytes, sign } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync, existsSync, renameSync, rmSync } from 'node:fs';
import path from 'node:path';
import { connect as tcpConnect, isIP } from 'node:net';
import { get as httpGet } from 'node:http';
import WebSocket from 'ws';
import { discoverLan } from './lan.js';
const MASTER = 'https://online.battlefronteternal.com';
const NOTICE = '2026-10-11';
function directInfo(address, port) {
  return new Promise((resolve, reject) => {
    const request = httpGet({ hostname: address, port, path: '/info', timeout: 5000 }, response => {
      if (response.statusCode !== 200) { response.resume(); reject(Error('Cannot read server information')); return; }
      const chunks = []; let size = 0;
      response.on('data', chunk => { size += chunk.length; if (size > 32768) { request.destroy(Error('Server information too large')); return; } chunks.push(chunk); });
      response.on('error', reject);
      response.on('end', () => { try { resolve(JSON.parse(Buffer.concat(chunks).toString())); } catch (error) { reject(error); } });
    });
    request.on('timeout', () => request.destroy(Error('Server information timed out')));
    request.on('error', reject);
  });
}
// A request the online service never answered, for the player: why, by its
// cause's code (ETIMEDOUT, ENOTFOUND, ECONNREFUSED...).
export function unreachable(error) {
  const code = error?.name === 'TimeoutError' ? 'timed out' : error?.cause?.code || error?.cause?.name || error?.code || '';
  return Error(`Can't reach the online service${code ? ` (${code})` : ''}. Check your connection and try again.`);
}
// The player's ping to a server, for the server browser: how long a TCP
// handshake with its game port (its /info) takes, in ms, or null when it
// does not answer within timeoutMs.
export function measureLatency(address, port, timeoutMs = 1000) {
  return new Promise(resolve => {
    if (!isIP(address) || !Number.isInteger(port) || port < 1 || port > 65535) { resolve(null); return; }
    const started = process.hrtime.bigint(), socket = tcpConnect({ host: address, port, timeout: timeoutMs });
    const done = value => { socket.destroy(); resolve(value); };
    socket.once('connect', () => done(Math.max(1, Math.round(Number(process.hrtime.bigint() - started) / 1e6))));
    socket.once('timeout', () => done(null));
    socket.once('error', () => done(null));
  });
}
// Each listed server's latencyMs: the lowest over the addresses it is
// listed at (a direct server's IPv4 and IPv6), null where it lists none
// (one that may relay keeps its address private) or none answers.
export async function withLatency(servers, measure = measureLatency) {
  const queue = [...servers];
  await Promise.all(Array.from({ length: Math.min(16, queue.length) }, async () => {
    for (let server = queue.shift(); server; server = queue.shift()) {
      const times = await Promise.all((Array.isArray(server.endpoints) ? server.endpoints : []).slice(0, 4).map(endpoint => measure(endpoint?.address, endpoint?.port)));
      const answered = times.filter(Number.isFinite);
      server.latencyMs = answered.length ? Math.min(...answered) : null;
    }
  }));
  return servers;
}
// The player's locally generated identity (no account). The network process
// signs handshakes with it; the renderer only ever sees the public half.
export function loadNetworkIdentity(directory) {
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  const file = path.join(directory, 'network-identity.json');
  if (existsSync(file)) return JSON.parse(readFileSync(file, 'utf8'));
  const pair = generateKeyPairSync('ed25519');
  const keys = { publicKey: pair.publicKey.export({ format: 'der', type: 'spki' }).toString('base64'), privateKey: pair.privateKey.export({ format: 'pem', type: 'pkcs8' }) };
  try { writeFileSync(file, JSON.stringify(keys), { flag: 'wx', mode: 0o600 }); }
  catch (error) { if (error.code === 'EEXIST') return JSON.parse(readFileSync(file, 'utf8')); throw error; }
  return keys;
}
export function createNetworkBridge(directory, emit) {
  const keys = loadNetworkIdentity(directory);
  const consentFile = path.join(directory, 'online-consent.json');
  let consent = false;
  try { const record = JSON.parse(readFileSync(consentFile)); consent = record.version === NOTICE && record.master === MASTER; } catch {}
  let socket;
  const hash = text => createHash('sha256').update(text).digest('hex');
  // A signed request to the master. A read that never reached it (no
  // connection, no answer in time) is tried once more, signed afresh; then
  // the player is told it can't be reached, and why.
  async function request(route, method = 'GET', value) {
    if (!consent) throw Error('Accept the online privacy notice first.');
    const body = value === undefined ? '' : JSON.stringify(value);
    for (let attempt = 0; ; attempt++) {
      const time = String(Math.floor(Date.now() / 1000)), nonce = randomBytes(16).toString('hex');
      let response;
      try {
        response = await fetch(MASTER + route, { method, headers: { 'content-type': 'application/json', 'x-bfe-notice': NOTICE, 'x-bfe-key': keys.publicKey, 'x-bfe-time': time, 'x-bfe-nonce': nonce, 'x-bfe-signature': sign(null, Buffer.from(`${method}\n${route}\n${time}\n${nonce}\n${hash(body)}`), keys.privateKey).toString('base64') }, body: body || undefined, signal: AbortSignal.timeout(10000) });
      } catch (error) {
        if (method === 'GET' && attempt === 0) { await new Promise(resolve => setTimeout(resolve, 750)); continue; }
        throw unreachable(error);
      }
      const result = await response.json().catch(() => ({}));
      if (!response.ok) throw Error(result.error || `Online service returned ${response.status}`);
      return result;
    }
  }
  return async function dispatch(action, value) {
    if (action === 'discover') return discoverLan();
    if (action === 'directInfo') {
      if (!value || !isIP(value.address) || !Number.isInteger(value.port) || value.port < 1 || value.port > 65535) throw Error('Invalid server address');
      return directInfo(value.address, value.port);
    }
    if (action === 'notice') return { accepted: consent, version: NOTICE, operator: 'Josh Lambert', master: MASTER };
    if (action === 'accept') {
      if (value !== NOTICE) return false;
      const temporary = `${consentFile}.${randomBytes(8).toString('hex')}.tmp`;
      try {
        writeFileSync(temporary, JSON.stringify({ version: NOTICE, master: MASTER }), { flag: 'wx', mode: 0o600 });
        renameSync(temporary, consentFile);
        consent = true;
        return true;
      } finally { rmSync(temporary, { force: true }); }
    }
    if (action === 'identity') return { publicKey: keys.publicKey, id: hash(keys.publicKey) };
    if (action === 'servers') { const result = await request('/v1/servers'); await withLatency(Array.isArray(result.servers) ? result.servers : []); return result; }
    if (action === 'credentials') { if (!/^[a-f0-9]{64}$/.test(value)) throw Error('Invalid server identity'); return request('/v1/relay-credentials', 'POST', { serverId: value }); }
    if (action === 'connect') {
      socket?.close();
      const { ticket } = await request('/v1/signal-ticket', 'POST', {});
      socket = new WebSocket(`${MASTER.replace('https:', 'wss:')}/v1/signal?ticket=${encodeURIComponent(ticket)}`, { handshakeTimeout: 10000, maxPayload: 32768, perMessageDeflate: false });
      socket.addEventListener('message', event => { if (typeof event.data === 'string' && event.data.length <= 32768) { try { emit(JSON.parse(event.data)); } catch { socket.close(); } } });
      await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', () => reject(Error('Signaling connection failed')), { once: true }); });
      return true;
    }
    if (action === 'signal') {
      if (!value || !/^[a-f0-9]{64}$/.test(value.to) || !['offer', 'answer', 'candidate', 'matchProbe'].includes(value.type) || typeof value.payload !== 'string' || value.payload.length > 24000) throw Error('Invalid signal');
      if (value.type === 'matchProbe') {
        const probe = JSON.parse(value.payload);
        if (!probe || Array.isArray(probe) || Object.keys(probe).length !== 3 || probe.type !== 'matchProbe' ||
            typeof probe.matchProbe !== 'string' || typeof probe.nonce !== 'string' ||
            !/^[a-f0-9]{32}$/.test(probe.matchProbe) || !/^[a-f0-9]{32}$/.test(probe.nonce)) throw Error('Invalid match readiness probe');
      }
      if (socket?.readyState !== WebSocket.OPEN || socket.bufferedAmount > 65536) return false;
      socket.send(JSON.stringify(value)); return true;
    }
    if (action === 'close') { socket?.close(); socket = null; return true; }
    throw Error('Unknown network action');
  };
}
