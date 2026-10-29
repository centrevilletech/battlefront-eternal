// Cryptography for BFE-UDP/1. Only primitives Node and Electron's BoringSSL
// both provide: X25519, Ed25519, HKDF-SHA256 and AES-256-GCM (Electron's
// Node has no ChaCha20-Poly1305). The construction is SIGMA-I with
// TLS 1.3-style key derivation: ephemeral X25519, each side signing the
// running transcript with its long-term Ed25519 identity.
import {
  createCipheriv, createDecipheriv, createHash, createHmac, createPrivateKey, createPublicKey,
  diffieHellman, generateKeyPairSync, randomBytes, sign, timingSafeEqual, verify,
} from 'node:crypto';

const X25519_SPKI = Buffer.from('302a300506032b656e032100', 'hex');
const ED25519_SPKI = Buffer.from('302a300506032b6570032100', 'hex');
export const KEY_BYTES = 32, IV_BYTES = 12, TAG_BYTES = 16;
const empty = Buffer.alloc(0);

export function hkdfExtract(salt, ikm) { return createHmac('sha256', salt.length ? salt : Buffer.alloc(32)).update(ikm).digest(); }
export function hkdfExpand(prk, info, length) {
  const blocks = [];
  let previous = empty;
  for (let i = 1; blocks.length * 32 < length; i++) {
    previous = createHmac('sha256', prk).update(previous).update(info).update(Buffer.of(i)).digest();
    blocks.push(previous);
  }
  return Buffer.concat(blocks).subarray(0, length);
}
// Labels keep every derived value in its own domain.
export const expandLabel = (secret, label, length = 32) => hkdfExpand(secret, Buffer.from('bfe-udp1 ' + label), length);
export const sha256 = (...parts) => { const hash = createHash('sha256'); for (const part of parts) hash.update(part); return hash.digest(); };

export function ephemeralKeyPair() {
  const { privateKey, publicKey } = generateKeyPairSync('x25519');
  return { privateKey, publicKey: publicKey.export({ format: 'der', type: 'spki' }).subarray(X25519_SPKI.length) };
}
export function x25519(privateKey, peerPublic) {
  if (!Buffer.isBuffer(peerPublic) || peerPublic.length !== 32) throw Error('invalid ephemeral key');
  const publicKey = createPublicKey({ key: Buffer.concat([X25519_SPKI, peerPublic]), format: 'der', type: 'spki' });
  const shared = diffieHellman({ privateKey, publicKey });
  // An all-zero result means a low-order point: reject (RFC 7748 section 6.1).
  if (shared.every(byte => byte === 0)) throw Error('invalid ephemeral key');
  return shared;
}

// Identities are the existing base64 SPKI Ed25519 keys; a server's id is the
// SHA-256 hex of that base64 text, as in listings and LAN discovery.
export const identityId = publicKeyBase64 => createHash('sha256').update(publicKeyBase64).digest('hex');
export function identityKeys(keys) {
  const publicDer = Buffer.from(keys.publicKey, 'base64');
  if (publicDer.length !== 44 || !publicDer.subarray(0, 12).equals(ED25519_SPKI) || weakEd25519(publicDer.subarray(12))) throw Error('invalid identity public key');
  const privateKey = createPrivateKey(keys.privateKey);
  if (privateKey.asymmetricKeyType !== 'ed25519') throw Error('invalid identity private key');
  return { publicKey: keys.publicKey, publicDer, privateKey, id: identityId(keys.publicKey) };
}
// Ed25519 points of small order verify signatures without any private key
// on OpenSSL (BoringSSL refuses them), and non-canonical encodings alias
// other keys. A point has order dividing 8 exactly when its y coordinate is
// 0, 1, -1 or +-Y8, whatever the sign of x, so checking y suffices.
const P25519 = 2n ** 255n - 19n, Y8 = 0x05fc536d880238b13933c6d305acdfd5f098eff289f4c345b027b2c28f95e826n;
const SMALL_ORDER_Y = new Set([0n, 1n, P25519 - 1n, Y8, P25519 - Y8]);
export function weakEd25519(raw) {
  let y = 0n;
  for (let i = 31; i >= 0; i--) y = (y << 8n) | BigInt(i === 31 ? raw[i] & 0x7f : raw[i]);
  return y >= P25519 || SMALL_ORDER_Y.has(y);
}
export function publicIdentity(der) {
  if (!Buffer.isBuffer(der) || der.length !== 44 || !der.subarray(0, 12).equals(ED25519_SPKI) || weakEd25519(der.subarray(12))) throw Error('invalid identity');
  const base64 = der.toString('base64');
  return { publicKey: base64, key: createPublicKey({ key: der, format: 'der', type: 'spki' }), id: identityId(base64) };
}
export const signTranscript = (privateKey, label, digest) => sign(null, Buffer.concat([Buffer.from(label), Buffer.of(0), digest]), privateKey);
export function verifyTranscript(publicKey, label, digest, signature) {
  try { return signature.length === 64 && verify(null, Buffer.concat([Buffer.from(label), Buffer.of(0), digest]), publicKey, signature); }
  catch { return false; }
}

// AES-256-GCM over one packet. The nonce is the static IV XOR the packet
// number, so a key never repeats a nonce while numbers keep increasing.
export class Aead {
  constructor(secret) {
    this.secret = secret;
    this.key = expandLabel(secret, 'key', KEY_BYTES);
    this.iv = expandLabel(secret, 'iv', IV_BYTES);
    this.nonce = Buffer.alloc(IV_BYTES);
    this.ivTail = this.iv.readUInt32BE(8);
  }
  next() { return new Aead(expandLabel(this.secret, 'ku', 32)); }
  nonceFor(number) {
    this.iv.copy(this.nonce, 0, 0, 8);
    this.nonce.writeUInt32BE((this.ivTail ^ number) >>> 0, 8);
    return this.nonce;
  }
  // Header bytes are authenticated, the plaintext encrypted; returns
  // header || ciphertext || tag in one buffer.
  seal(header, number, plaintext) {
    const cipher = createCipheriv('aes-256-gcm', this.key, this.nonceFor(number), { authTagLength: TAG_BYTES });
    cipher.setAAD(header);
    const out = Buffer.allocUnsafe(header.length + plaintext.length + TAG_BYTES);
    header.copy(out, 0);
    const body = cipher.update(plaintext);
    body.copy(out, header.length);
    let written = header.length + body.length;
    const tail = cipher.final();
    if (tail.length) { tail.copy(out, written); written += tail.length; }
    cipher.getAuthTag().copy(out, written);
    return out;
  }
  // Returns the plaintext, or null when authentication fails.
  open(packet, headerLength, number) {
    if (packet.length < headerLength + TAG_BYTES) return null;
    try {
      const decipher = createDecipheriv('aes-256-gcm', this.key, this.nonceFor(number), { authTagLength: TAG_BYTES });
      decipher.setAAD(packet.subarray(0, headerLength));
      decipher.setAuthTag(packet.subarray(packet.length - TAG_BYTES));
      const body = decipher.update(packet.subarray(headerLength, packet.length - TAG_BYTES));
      const tail = decipher.final();
      return tail.length ? Buffer.concat([body, tail]) : body;
    } catch { return null; }
  }
}

// Stateless retry tokens bind a source address to one handshake attempt for
// a few seconds, so a flood of spoofed initiations costs the server an HMAC
// rather than key agreement and signatures.
export class RetryTokens {
  constructor(secret = randomBytes(32), lifetimeMs = 10000) { this.secret = secret; this.lifetimeMs = lifetimeMs; }
  mac(time, source, session, ephemeral) {
    return createHmac('sha256', this.secret).update(String(time)).update('|').update(source).update('|').update(String(session)).update(ephemeral).digest().subarray(0, 16);
  }
  mint(source, session, ephemeral, now = Date.now()) {
    const token = Buffer.alloc(24);
    token.writeDoubleLE(now, 0);
    this.mac(now, source, session, ephemeral).copy(token, 8);
    return token;
  }
  check(token, source, session, ephemeral, now = Date.now()) {
    if (!Buffer.isBuffer(token) || token.length !== 24) return false;
    const time = token.readDoubleLE(0);
    if (!Number.isFinite(time) || time > now + 1000 || now - time > this.lifetimeMs) return false;
    return timingSafeEqual(token.subarray(8), this.mac(time, source, session, ephemeral));
  }
}
