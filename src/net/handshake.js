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
import {COMMON as H,INITIATION as I,RESPONSE as R,CONFIRM as C,RETRY as T,PROOF as P,LIMITS as L} from './layout.js';
import { PACKET, TRANSPORT_VERSION, MAX_DATAGRAM } from './codec.js';
import {
  Aead, ephemeralKeyPair, expandLabel, hkdfExtract, publicIdentity, sha256, signTranscript, verifyTranscript, x25519,
} from './crypto.js';

export const INITIATION_BYTES = MAX_DATAGRAM;
const RESPONSE_HEADER=R.BYTES,CONFIRM_HEADER=C.BYTES,MAX_TOKEN=L.MAX_TOKEN,INITIATION_FIELDS=L.INITIATION_FIELDS;
// A RESPONSE (186 bytes of framing plus the hello) must fit one datagram.
export const MAX_HELLO = L.MAX_HELLO;
export const CONFIRM_MIN = CONFIRM_HEADER + P.BYTES + L.TAG;
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
  const out = Buffer.alloc(P.BYTES + hello.length);
  identity.copy(out,P.IDENTITY);signature.copy(out,P.SIGNATURE);out.writeUInt16LE(hello.length,P.HELLO_LENGTH);hello.copy(out,P.BYTES);
  return out;
}
function readProof(plaintext) {
  if (plaintext.length < P.BYTES) throw Error('truncated proof');
  const helloLength = plaintext.readUInt16LE(P.HELLO_LENGTH);
  if (helloLength > MAX_HELLO || P.BYTES + helloLength !== plaintext.length) throw Error('invalid proof');
  return { identity: plaintext.subarray(P.IDENTITY,P.SIGNATURE), signature: plaintext.subarray(P.SIGNATURE,P.HELLO_LENGTH), helloBytes: plaintext.subarray(P.BYTES) };
}
function trafficKeys(transcript, handshakeSecret) {
  const master = hkdfExtract(transcript, handshakeSecret);
  return { client: new Aead(expandLabel(master, 'c ap')), server: new Aead(expandLabel(master, 's ap')) };
}

export function readInitiation(packet) {
  if (packet.length < INITIATION_BYTES || packet.length > 1500 || packet[I.TYPE] !== PACKET.INITIATION) return null;
  const tokenLength = packet.readUInt16LE(I.TOKEN_LENGTH);
  if (tokenLength > MAX_TOKEN) return null;
  return { version: packet[I.VERSION], clientSession: packet.readUInt32LE(I.SESSION), ephemeral: packet.subarray(I.EPHEMERAL,I.RANDOM), random: packet.subarray(I.RANDOM,I.TOKEN_LENGTH), token: packet.subarray(I.BYTES,I.BYTES+tokenLength) };
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
    packet[I.TYPE] = PACKET.INITIATION; packet[I.VERSION] = TRANSPORT_VERSION;
    packet.writeUInt32LE(this.clientSession,I.SESSION);
    this.ephemeral.publicKey.copy(packet,I.EPHEMERAL); this.random.copy(packet,I.RANDOM);
    packet.writeUInt16LE(token.length,I.TOKEN_LENGTH); token.copy(packet,I.BYTES);
    return packet;
  }
  // A server reply: {retry, token}, {version}, {error} (another server's
  // identity), or the established session with the CONFIRM to send.
  // Anything malformed or unauthentic returns null.
  receive(packet) {
    if (packet.length < H.BYTES || packet.readUInt32LE(H.SESSION) !== this.clientSession) return null;
    if (packet[H.TYPE] === PACKET.RETRY) {
      if (this.result || packet.length < T.BYTES) return null;
      const length = packet.readUInt16LE(T.TOKEN_LENGTH);
      if (!length || length > MAX_TOKEN || packet.length !== T.BYTES+length) return null;
      return { type: 'retry', token: Buffer.from(packet.subarray(T.BYTES)) };
    }
    // A VERSION naming our own version is not a version problem.
    if (packet[H.TYPE] === PACKET.VERSION) return this.result || packet.length !== H.BYTES || packet[H.VERSION] === TRANSPORT_VERSION ? null : { type: 'version', version: packet[H.VERSION] };
    if (packet[H.TYPE] !== PACKET.RESPONSE || packet.length < RESPONSE_HEADER+P.BYTES+L.TAG || packet[H.VERSION] !== TRANSPORT_VERSION) return null;
    // A duplicate RESPONSE (our CONFIRM was lost): send the same CONFIRM again.
    if (this.result) return this.response?.equals(packet) ? { type: 'duplicate', confirm: this.result.confirm } : null;
    const length = packet.readUInt16LE(R.SEALED_LENGTH);
    if (RESPONSE_HEADER + length !== packet.length) return null;
    try {
      const transcript2 = sha256(this.transcript1, packet.subarray(0,R.SEALED_LENGTH));
      const handshakeSecret = hkdfExtract(transcript2, x25519(this.ephemeral.privateKey, Buffer.from(packet.subarray(R.EPHEMERAL,R.RANDOM))));
      const serverAead = new Aead(expandLabel(handshakeSecret, 's hs')), clientAead = new Aead(expandLabel(handshakeSecret, 'c hs'));
      const plaintext = serverAead.open(packet, RESPONSE_HEADER, 0);
      if (!plaintext) return null;
      const proof = readProof(plaintext), server = publicIdentity(Buffer.from(proof.identity));
      if (server.id !== this.expectedServerId) throw Error('Server identity does not match the directory');
      if (!verifyTranscript(server.key, LABEL.server, sha256(transcript2, proof.identity, proof.helloBytes), proof.signature)) return null;
      const serverHello = decodeHello(proof.helloBytes);
      const transcript3 = sha256(transcript2, packet.subarray(R.SEALED_LENGTH));
      const signature = signTranscript(this.identity.privateKey, LABEL.client, sha256(transcript3, this.identity.publicDer, this.hello));
      const sealedHeader = Buffer.alloc(CONFIRM_HEADER);
      sealedHeader[C.TYPE] = PACKET.CONFIRM; sealedHeader[C.VERSION] = TRANSPORT_VERSION;
      const serverSession = packet.readUInt32LE(R.SERVER_SESSION);
      sealedHeader.writeUInt32LE(serverSession,C.SESSION);
      const proofBytes = proofPlaintext(this.identity.publicDer, signature, this.hello);
      sealedHeader.writeUInt16LE(proofBytes.length+L.TAG,C.SEALED_LENGTH);
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
      const reply = Buffer.alloc(H.BYTES); reply[H.TYPE] = PACKET.VERSION; reply[H.VERSION] = TRANSPORT_VERSION; reply.writeUInt32LE(init.clientSession,H.SESSION);
      return reply;
    }
    this.expire();
    const key = `${source}|${init.clientSession}|${init.ephemeral.toString('hex')}`;
    const existing = this.byInitiation.get(key);
    if (existing !== undefined) return this.pending.get(existing)?.response ?? null;
    if (requireRetry || this.pending.size >= this.retryAbove) {
      if (!this.retry.check(Buffer.from(init.token), source, init.clientSession, init.ephemeral, this.now())) {
        const token = this.retry.mint(source, init.clientSession, init.ephemeral, this.now());
        const reply = Buffer.alloc(T.BYTES+token.length);
        reply[H.TYPE] = PACKET.RETRY; reply[H.VERSION] = TRANSPORT_VERSION; reply.writeUInt32LE(init.clientSession,H.SESSION); reply.writeUInt16LE(token.length,T.TOKEN_LENGTH); token.copy(reply,T.BYTES);
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
    header[R.TYPE] = PACKET.RESPONSE; header[R.VERSION] = TRANSPORT_VERSION;
    header.writeUInt32LE(init.clientSession,H.SESSION); header.writeUInt32LE(serverSession,R.SERVER_SESSION);
    ephemeral.publicKey.copy(header,R.EPHEMERAL); random.copy(header,R.RANDOM);
    const transcript2 = sha256(initiationTranscript(packet), header.subarray(0,R.SEALED_LENGTH));
    const secret = hkdfExtract(transcript2, handshakeSecret);
    const serverAead = new Aead(expandLabel(secret, 's hs')), clientAead = new Aead(expandLabel(secret, 'c hs'));
    const signature = signTranscript(this.identity.privateKey, LABEL.server, sha256(transcript2, this.identity.publicDer, helloBytes));
    const plaintext = proofPlaintext(this.identity.publicDer, signature, helloBytes);
    header.writeUInt16LE(plaintext.length+L.TAG,R.SEALED_LENGTH);
    const response = serverAead.seal(header, 0, plaintext);
    const transcript3 = sha256(transcript2, response.subarray(R.SEALED_LENGTH));
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
    if (packet.length < CONFIRM_MIN || packet[C.TYPE] !== PACKET.CONFIRM || packet[C.VERSION] !== TRANSPORT_VERSION) return null;
    const serverSession = packet.readUInt32LE(C.SESSION), state = this.pending.get(serverSession);
    if (!state || state.source !== source || CONFIRM_HEADER + packet.readUInt16LE(C.SEALED_LENGTH) !== packet.length) return null;
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
