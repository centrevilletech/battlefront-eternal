// STUN (RFC 8489) and TURN (RFC 8656) messages, enough for reflexive
// address discovery and a UDP relay allocation through coturn. These share
// the game socket: STUN's first byte is 0-3 and ChannelData's 0x40-0x4f.
import { createHash, createHmac, randomBytes } from 'node:crypto';
import { isIP } from 'node:net';
import { crc32 } from 'node:zlib';

export const MAGIC = 0x2112a442;
export const METHOD = Object.freeze({ BINDING: 0x001, ALLOCATE: 0x003, REFRESH: 0x004, SEND: 0x006, DATA: 0x007, CREATE_PERMISSION: 0x008, CHANNEL_BIND: 0x009 });
export const CLASS = Object.freeze({ REQUEST: 0, INDICATION: 1, SUCCESS: 2, ERROR: 3 });
export const ATTR = Object.freeze({
  MAPPED_ADDRESS: 0x0001, USERNAME: 0x0006, MESSAGE_INTEGRITY: 0x0008, ERROR_CODE: 0x0009, CHANNEL_NUMBER: 0x000c,
  LIFETIME: 0x000d, XOR_PEER_ADDRESS: 0x0012, DATA: 0x0013, REALM: 0x0014, NONCE: 0x0015, XOR_RELAYED_ADDRESS: 0x0016,
  REQUESTED_ADDRESS_FAMILY: 0x0017, REQUESTED_TRANSPORT: 0x0019, XOR_MAPPED_ADDRESS: 0x0020, SOFTWARE: 0x8022, FINGERPRINT: 0x8028,
});
const ADDRESS_ATTRIBUTES = new Set([ATTR.XOR_MAPPED_ADDRESS, ATTR.XOR_PEER_ADDRESS, ATTR.XOR_RELAYED_ADDRESS, ATTR.MAPPED_ADDRESS]);

export const messageType = (method, klass) => (method & 0x0f) | ((method & 0x70) << 1) | ((method & 0xf80) << 2) | ((klass & 1) << 4) | ((klass & 2) << 7);
export function splitType(type) {
  return { method: (type & 0x0f) | ((type >> 1) & 0x70) | ((type >> 2) & 0xf80), klass: ((type >> 4) & 1) | ((type >> 7) & 2) };
}
export const isStun = packet => packet.length >= 20 && packet[0] < 4 && packet.readUInt32BE(4) === MAGIC;
export const isChannelData = packet => packet.length >= 4 && packet[0] >= 0x40 && packet[0] <= 0x4f;

function ipBytes(address) {
  if (isIP(address) === 4) return Buffer.from(address.split('.').map(Number));
  const [head, tail = ''] = address.split('%')[0].split('::');
  const words = side => side ? side.split(':').flatMap(part => part.includes('.') ? (([a, b, c, d]) => [(a << 8) | b, (c << 8) | d])(part.split('.').map(Number)) : [parseInt(part, 16)]) : [];
  const left = words(head), right = words(tail), all = [...left, ...Array(8 - left.length - right.length).fill(0), ...right];
  const out = Buffer.alloc(16);
  all.forEach((word, i) => out.writeUInt16BE(word, i * 2));
  return out;
}
// The same text inet_ntop produces, so decoded addresses compare equal to
// the `rinfo.address` Node reports for the same host.
export function ipString(bytes) {
  if (bytes.length === 4) return [...bytes].join('.');
  const words = Array.from({ length: 8 }, (_, i) => bytes.readUInt16BE(i * 2));
  if (words.slice(0, 5).every(word => !word) && words[5] === 0xffff) return '::ffff:' + [...bytes.subarray(12)].join('.');
  let best = -1, bestLength = 1;
  for (let i = 0; i < 8;) {
    if (words[i]) { i++; continue; }
    let j = i; while (j < 8 && !words[j]) j++;
    if (j - i > bestLength) { best = i; bestLength = j - i; }
    i = j;
  }
  const hex = list => list.map(word => word.toString(16)).join(':');
  return best < 0 ? hex(words) : hex(words.slice(0, best)) + '::' + hex(words.slice(best + bestLength));
}
export function encodeAddress(address, port, transaction, xor = true) {
  const bytes = ipBytes(address), out = Buffer.alloc(4 + bytes.length);
  out[1] = bytes.length === 4 ? 1 : 2;
  out.writeUInt16BE(xor ? port ^ (MAGIC >>> 16) : port, 2);
  const mask = Buffer.alloc(16); mask.writeUInt32BE(MAGIC, 0); transaction.copy(mask, 4);
  for (let i = 0; i < bytes.length; i++) out[4 + i] = xor ? bytes[i] ^ mask[i] : bytes[i];
  return out;
}
export function decodeAddress(value, transaction, xor = true) {
  if (value.length < 8) throw Error('invalid STUN address');
  const family = value[1], length = family === 1 ? 4 : family === 2 ? 16 : 0;
  if (!length || value.length < 4 + length) throw Error('invalid STUN address');
  const mask = Buffer.alloc(16); mask.writeUInt32BE(MAGIC, 0); transaction.copy(mask, 4);
  const bytes = Buffer.alloc(length);
  for (let i = 0; i < length; i++) bytes[i] = xor ? value[4 + i] ^ mask[i] : value[4 + i];
  return { address: ipString(bytes), port: xor ? value.readUInt16BE(2) ^ (MAGIC >>> 16) : value.readUInt16BE(2), family: length === 4 ? 'IPv4' : 'IPv6' };
}

export const longTermKey = (username, realm, password) => createHash('md5').update(`${username}:${realm}:${password}`).digest();

// Builds a message. `attributes` is [[type, Buffer], ...]; `key` adds
// MESSAGE-INTEGRITY; FINGERPRINT is always last.
export function encodeMessage(method, klass, transaction, attributes = [], key = null) {
  const parts = [];
  for (const [type, value] of attributes) {
    const header = Buffer.alloc(4); header.writeUInt16BE(type, 0); header.writeUInt16BE(value.length, 2);
    parts.push(header, value, Buffer.alloc((4 - (value.length % 4)) % 4));
  }
  let body = Buffer.concat(parts);
  const header = Buffer.alloc(20);
  header.writeUInt16BE(messageType(method, klass), 0); header.writeUInt32BE(MAGIC, 4); transaction.copy(header, 8);
  if (key) {
    header.writeUInt16BE(body.length + 24, 2);
    const integrity = createHmac('sha1', key).update(header).update(body).digest();
    const attribute = Buffer.alloc(24); attribute.writeUInt16BE(ATTR.MESSAGE_INTEGRITY, 0); attribute.writeUInt16BE(20, 2); integrity.copy(attribute, 4);
    body = Buffer.concat([body, attribute]);
  }
  header.writeUInt16BE(body.length + 8, 2);
  const crc = (crc32(Buffer.concat([header, body])) ^ 0x5354554e) >>> 0;
  const fingerprint = Buffer.alloc(8); fingerprint.writeUInt16BE(ATTR.FINGERPRINT, 0); fingerprint.writeUInt16BE(4, 2); fingerprint.writeUInt32BE(crc, 4);
  return Buffer.concat([header, body, fingerprint]);
}

export function decodeMessage(packet) {
  if (!isStun(packet)) return null;
  const length = packet.readUInt16BE(2);
  if (length % 4 || 20 + length > packet.length) return null;
  const { method, klass } = splitType(packet.readUInt16BE(0));
  const transaction = packet.subarray(8, 20), attributes = new Map(), list = [];
  let offset = 20, integrityAt = -1;
  while (offset + 4 <= 20 + length) {
    const type = packet.readUInt16BE(offset), size = packet.readUInt16BE(offset + 2);
    if (offset + 4 + size > 20 + length) return null;
    // Nothing after MESSAGE-INTEGRITY but FINGERPRINT is covered by it.
    if (integrityAt >= 0 && type !== ATTR.FINGERPRINT) break;
    if (type === ATTR.MESSAGE_INTEGRITY) integrityAt = offset;
    const value = packet.subarray(offset + 4, offset + 4 + size);
    if (!attributes.has(type)) attributes.set(type, value);
    list.push([type, value]);
    offset += 4 + size + ((4 - (size % 4)) % 4);
  }
  return { method, klass, transaction, attributes, list, packet: packet.subarray(0, 20 + length), integrityAt };
}
// True when the message carries a MESSAGE-INTEGRITY valid under `key`.
export function checkIntegrity(message, key) {
  if (message.integrityAt < 0) return false;
  const copy = Buffer.from(message.packet.subarray(0, message.integrityAt));
  copy.writeUInt16BE(message.integrityAt - 20 + 24, 2);
  const expected = createHmac('sha1', key).update(copy).digest();
  return expected.equals(message.packet.subarray(message.integrityAt + 4, message.integrityAt + 24));
}
// A missing or malformed address attribute reads as null (these come from
// the network, so they must not throw into a socket callback).
export function address(message, type) {
  const value = message.attributes.get(type);
  try { return value ? decodeAddress(value, message.transaction, type !== ATTR.MAPPED_ADDRESS) : null; } catch { return null; }
}
export function errorCode(message) {
  const value = message.attributes.get(ATTR.ERROR_CODE);
  return value && value.length >= 4 ? { code: (value[2] & 7) * 100 + value[3], reason: value.subarray(4).toString('utf8') } : null;
}
export const newTransaction = () => randomBytes(12);
export const u32 = value => { const out = Buffer.alloc(4); out.writeUInt32BE(value >>> 0); return out; };
export { ADDRESS_ATTRIBUTES };
