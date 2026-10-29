// BFE-UDP/1 handshake: one round trip plus a confirmation, mutual Ed25519
// identities, ephemeral X25519 forward secrecy (docs/native-udp-transport.md).
//
//   client -> INITIATION  session, ephemeral, random, [retry token], padded to 1200
//   server -> RESPONSE    session, ephemeral, random, AEAD{identity, signature, hello}
//   client -> CONFIRM     AEAD{identity, signature, hello}
//
// Each signature covers the running transcript hash, binding both ephemeral
// keys and the signer's hello. The initiation is padded so a reply is never
// larger than the request (no amplification); under load the server answers
// with a stateless RETRY token before doing any key agreement. The
// transcript covers the initiation's fixed fields, not its token or
// padding: a RESPONSE stays verifiable whichever initiation it answers, so a
// stray or spoofed RETRY cannot stall a join.
import { randomBytes } from 'node:crypto';
import { PACKET, TRANSPORT_VERSION, MAX_DATAGRAM } from './codec.js';
import {
  Aead, ephemeralKeyPair, expandLabel, hkdfExtract, publicIdentity, sha256, signTranscript, verifyTranscript, x25519,
} from './crypto.js';

export const INITIATION_BYTES = MAX_DATAGRAM;
const RESPONSE_HEADER = 60, CONFIRM_HEADER = 8, MAX_TOKEN = 64, INITIATION_FIELDS = 54;
// A RESPONSE (186 bytes of framing plus the hello) must fit one datagram.
export const MAX_HELLO = MAX_DATAGRAM - RESPONSE_HEADER - 110 - 16;
export const CONFIRM_MIN = CONFIRM_HEADER + 110 + 16;
const LABEL = Object.freeze({ initiation: 'BFE-UDP/1 initiation', server: 'BFE-UDP/1 server', client: 'BFE-UDP/1 client' });

const initiationTranscript = packet => sha256(Buffer.from(LABEL.initiation), packet.subarray(0, INITIATION_FIELDS));
const randomSession = () => { for (;;) { const value = randomBytes(4).readUInt32LE(0); if (value) return value; } };
export { randomSession };

function encodeHello(hello) {
  const bytes = Buffer.from(JSON.stringify(hello ?? {}));
  if (bytes.length > MAX_HELLO) throw Error('handshake hello too large');
  return bytes;
}
function decodeHello(bytes) {
  const value = JSON.parse(bytes.toString('utf8'));
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw Error('invalid hello');
  return value;
}
// identity || signature || u16 length || hello
function proofPlaintext(identity, signature, hello) {
  const out = Buffer.alloc(44 + 64 + 2 + hello.length);
  identity.copy(out, 0); signature.copy(out, 44); out.writeUInt16LE(hello.length, 108); hello.copy(out, 110);
  return out;
}
function readProof(plaintext) {
  if (plaintext.length < 110) throw Error('truncated proof');
  const helloLength = plaintext.readUInt16LE(108);
  if (helloLength > MAX_HELLO || 110 + helloLength !== plaintext.length) throw Error('invalid proof');
  return { identity: plaintext.subarray(0, 44), signature: plaintext.subarray(44, 108), helloBytes: plaintext.subarray(110) };
}
function trafficKeys(transcript, handshakeSecret) {
  const master = hkdfExtract(transcript, handshakeSecret);
  return { client: new Aead(expandLabel(master, 'c ap')), server: new Aead(expandLabel(master, 's ap')) };
}

export function readInitiation(packet) {
  if (packet.length < INITIATION_BYTES || packet.length > 1500 || packet[0] !== PACKET.INITIATION) return null;
  const tokenLength = packet.readUInt16LE(54);
  if (tokenLength > MAX_TOKEN) return null;
  return { version: packet[1], clientSession: packet.readUInt32LE(2), ephemeral: packet.subarray(6, 38), random: packet.subarray(38, 54), token: packet.subarray(56, 56 + tokenLength) };
}

export class ClientHandshake {
  // identity: identityKeys(); expectedServerId: hex id the listing or LAN
  // discovery named (required: there is no unauthenticated join).
  constructor({ identity, hello, expectedServerId }) {
    if (!/^[a-f0-9]{64}$/.test(expectedServerId || '')) throw Error('Invalid server identity');
    this.identity = identity; this.hello = encodeHello(hello); this.expectedServerId = expectedServerId;
    this.clientSession = randomSession();
    this.ephemeral = ephemeralKeyPair();
    this.random = randomBytes(16);
    this.result = null;
    this.transcript1 = initiationTranscript(this.initiation());
  }
  // The token is per path: a RETRY's token is bound to the address the
  // server saw, so the endpoint keeps one for each candidate path.
  initiation(token = Buffer.alloc(0)) {
    const packet = Buffer.alloc(INITIATION_BYTES);
    packet[0] = PACKET.INITIATION; packet[1] = TRANSPORT_VERSION;
    packet.writeUInt32LE(this.clientSession, 2);
    this.ephemeral.publicKey.copy(packet, 6); this.random.copy(packet, 38);
    packet.writeUInt16LE(token.length, 54); token.copy(packet, 56);
    return packet;
  }
  // A server reply: {retry, token}, {version}, {error} (another server's
  // identity), or the established session with the CONFIRM to send.
  // Anything malformed or unauthentic returns null.
  receive(packet) {
    if (packet.length < 6 || packet.readUInt32LE(2) !== this.clientSession) return null;
    if (packet[0] === PACKET.RETRY) {
      if (this.result || packet.length < 8) return null;
      const length = packet.readUInt16LE(6);
      if (!length || length > MAX_TOKEN || packet.length !== 8 + length) return null;
      return { type: 'retry', token: Buffer.from(packet.subarray(8)) };
    }
    // A VERSION naming our own version is not a version problem.
    if (packet[0] === PACKET.VERSION) return this.result || packet.length !== 6 || packet[1] === TRANSPORT_VERSION ? null : { type: 'version', version: packet[1] };
    if (packet[0] !== PACKET.RESPONSE || packet.length < RESPONSE_HEADER + 110 + 16 || packet[1] !== TRANSPORT_VERSION) return null;
    // A duplicate RESPONSE (our CONFIRM was lost): send the same CONFIRM again.
    if (this.result) return this.response?.equals(packet) ? { type: 'duplicate', confirm: this.result.confirm } : null;
    const length = packet.readUInt16LE(58);
    if (RESPONSE_HEADER + length !== packet.length) return null;
    try {
      const transcript2 = sha256(this.transcript1, packet.subarray(0, 58));
      const handshakeSecret = hkdfExtract(transcript2, x25519(this.ephemeral.privateKey, Buffer.from(packet.subarray(10, 42))));
      const serverAead = new Aead(expandLabel(handshakeSecret, 's hs')), clientAead = new Aead(expandLabel(handshakeSecret, 'c hs'));
      const plaintext = serverAead.open(packet, RESPONSE_HEADER, 0);
      if (!plaintext) return null;
      const proof = readProof(plaintext), server = publicIdentity(Buffer.from(proof.identity));
      if (server.id !== this.expectedServerId) throw Error('Server identity does not match the directory');
      if (!verifyTranscript(server.key, LABEL.server, sha256(transcript2, proof.identity, proof.helloBytes), proof.signature)) return null;
      const serverHello = decodeHello(proof.helloBytes);
      const transcript3 = sha256(transcript2, packet.subarray(58));
      const signature = signTranscript(this.identity.privateKey, LABEL.client, sha256(transcript3, this.identity.publicDer, this.hello));
      const sealedHeader = Buffer.alloc(CONFIRM_HEADER);
      sealedHeader[0] = PACKET.CONFIRM; sealedHeader[1] = TRANSPORT_VERSION;
      const serverSession = packet.readUInt32LE(6);
      sealedHeader.writeUInt32LE(serverSession, 2);
      const proofBytes = proofPlaintext(this.identity.publicDer, signature, this.hello);
      sealedHeader.writeUInt16LE(proofBytes.length + 16, 6);
      const confirm = clientAead.seal(sealedHeader, 0, proofBytes);
      const keys = trafficKeys(sha256(transcript3, confirm), handshakeSecret);
      this.response = Buffer.from(packet);
      this.ephemeral = null;
      this.result = { type: 'response', confirm, clientSession: this.clientSession, serverSession, send: keys.client, receive: keys.server,
        server: { id: server.id, publicKey: server.publicKey, hello: serverHello } };
      return this.result;
    } catch (error) {
      if (/identity does not match/.test(error.message)) return { type: 'error', error };
      return null;
    }
  }
}

// Server side: answers initiations, holds bounded half-open state, and turns
// a verified CONFIRM into session keys and the client's proven identity.
export class ServerHandshakes {
  // Half-open state is bounded per source network (`prefix`, from the
  // endpoint) and in total; when the table is full the oldest entry goes,
  // so a flood can delay a join by at most its own round trip. A client that
  // races several local ports holds one entry per port, so the per-network
  // share leaves room for a few such joins from one household at once.
  constructor({ identity, hello = () => ({}), retry, now = () => Date.now(), maxPending = 1024, maxPendingPerPrefix = 32, pendingMs = 5000, retryAbove = 256, sessions = new Set() }) {
    Object.assign(this, { identity, hello, retry, now, maxPending, maxPendingPerPrefix, pendingMs, retryAbove, sessions });
    this.pending = new Map(); // serverSession -> half-open state, oldest first
    this.byInitiation = new Map(); // source|clientSession|ephemeral -> serverSession
    this.prefixes = new Map(); // source network -> half-open count
  }
  forget(session, state) {
    this.pending.delete(session); this.byInitiation.delete(state.key);
    const count = (this.prefixes.get(state.prefix) ?? 1) - 1;
    if (count > 0) this.prefixes.set(state.prefix, count); else this.prefixes.delete(state.prefix);
  }
  expire() {
    const now = this.now();
    for (const [session, state] of this.pending) {
      if (now - state.created <= this.pendingMs) break;
      this.forget(session, state);
    }
  }
  // Returns a reply buffer (RESPONSE, RETRY or VERSION) or null to drop.
  initiation(packet, source, { requireRetry = false, prefix = source } = {}) {
    const init = readInitiation(packet);
    if (!init || !init.clientSession) return null;
    if (init.version !== TRANSPORT_VERSION) {
      const reply = Buffer.alloc(6); reply[0] = PACKET.VERSION; reply[1] = TRANSPORT_VERSION; reply.writeUInt32LE(init.clientSession, 2);
      return reply;
    }
    this.expire();
    const key = `${source}|${init.clientSession}|${init.ephemeral.toString('hex')}`;
    const existing = this.byInitiation.get(key);
    if (existing !== undefined) return this.pending.get(existing)?.response ?? null;
    if (requireRetry || this.pending.size >= this.retryAbove) {
      if (!this.retry.check(Buffer.from(init.token), source, init.clientSession, init.ephemeral, this.now())) {
        const token = this.retry.mint(source, init.clientSession, init.ephemeral, this.now());
        const reply = Buffer.alloc(8 + token.length);
        reply[0] = PACKET.RETRY; reply[1] = TRANSPORT_VERSION; reply.writeUInt32LE(init.clientSession, 2); reply.writeUInt16LE(token.length, 6); token.copy(reply, 8);
        return reply;
      }
    }
    if ((this.prefixes.get(prefix) ?? 0) >= this.maxPendingPerPrefix) return null;
    if (this.pending.size >= this.maxPending) { const [session, state] = this.pending.entries().next().value; this.forget(session, state); }
    let serverSession;
    do serverSession = randomSession(); while (this.pending.has(serverSession) || this.sessions.has(serverSession));
    const ephemeral = ephemeralKeyPair(), random = randomBytes(16);
    let handshakeSecret;
    try { handshakeSecret = x25519(ephemeral.privateKey, Buffer.from(init.ephemeral)); } catch { return null; }
    const helloBytes = encodeHello(this.hello());
    const header = Buffer.alloc(RESPONSE_HEADER);
    header[0] = PACKET.RESPONSE; header[1] = TRANSPORT_VERSION;
    header.writeUInt32LE(init.clientSession, 2); header.writeUInt32LE(serverSession, 6);
    ephemeral.publicKey.copy(header, 10); random.copy(header, 42);
    const transcript2 = sha256(initiationTranscript(packet), header.subarray(0, 58));
    const secret = hkdfExtract(transcript2, handshakeSecret);
    const serverAead = new Aead(expandLabel(secret, 's hs')), clientAead = new Aead(expandLabel(secret, 'c hs'));
    const signature = signTranscript(this.identity.privateKey, LABEL.server, sha256(transcript2, this.identity.publicDer, helloBytes));
    const plaintext = proofPlaintext(this.identity.publicDer, signature, helloBytes);
    header.writeUInt16LE(plaintext.length + 16, 58);
    const response = serverAead.seal(header, 0, plaintext);
    const transcript3 = sha256(transcript2, response.subarray(58));
    this.pending.set(serverSession, { key, source, prefix, created: this.now(), clientSession: init.clientSession, secret, clientAead, transcript3, response, sentAt: this.now() });
    this.byInitiation.set(key, serverSession);
    this.prefixes.set(prefix, (this.prefixes.get(prefix) ?? 0) + 1);
    return response;
  }
  // A CONFIRM: {session} with keys and the client's identity, or null. It
  // must come from the address that sent the initiation: a valid CONFIRM
  // from there proves that address received the RESPONSE. From anywhere
  // else, a client could open its session on a victim's address.
  confirm(packet, source) {
    if (packet.length < CONFIRM_MIN || packet[0] !== PACKET.CONFIRM || packet[1] !== TRANSPORT_VERSION) return null;
    const serverSession = packet.readUInt32LE(2), state = this.pending.get(serverSession);
    if (!state || state.source !== source || CONFIRM_HEADER + packet.readUInt16LE(6) !== packet.length) return null;
    const plaintext = state.clientAead.open(packet, CONFIRM_HEADER, 0);
    if (!plaintext) return null;
    let proof, client;
    try { proof = readProof(plaintext); client = publicIdentity(Buffer.from(proof.identity)); }
    catch { return null; }
    if (!verifyTranscript(client.key, LABEL.client, sha256(state.transcript3, proof.identity, proof.helloBytes), proof.signature)) return null;
    let hello;
    try { hello = decodeHello(proof.helloBytes); } catch { return null; }
    this.forget(serverSession, state);
    const keys = trafficKeys(sha256(state.transcript3, packet), state.secret);
    return { serverSession, clientSession: state.clientSession, source: state.source, send: keys.server, receive: keys.client,
      client: { id: client.id, publicKey: client.publicKey, hello }, rtt: this.now() - state.sentAt, confirm: Buffer.from(packet) };
  }
}
