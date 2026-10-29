// Wire constants and small codecs for the native UDP transport (BFE-UDP/1,
// docs/native-udp-transport.md). Node-only: the Electron network process and
// bfeserver share these modules; the renderer never imports them.
//
// First-byte ranges stay clear of STUN (0x00-0x03) and TURN ChannelData
// (0x40-0x4f), so game, STUN and relayed packets share one socket (RFC 7983).
export const TRANSPORT_VERSION = 1;
export const PACKET = Object.freeze({
  INITIATION: 0xb1, RESPONSE: 0xb2, RETRY: 0xb3, DATA: 0xb4, DATA_PHASE: 0xb5,
  VERSION: 0xb6, CONFIRM: 0xb7, PUNCH: 0xb8,
});
export const FRAME = Object.freeze({
  PADDING: 0x00, PING: 0x01, ACK: 0x02, DATAGRAM: 0x03, FRAGMENT: 0x04, STREAM: 0x05,
  CLOSE: 0x06, HANDSHAKE_DONE: 0x07, PATH_CHALLENGE: 0x08, PATH_RESPONSE: 0x09,
});
// Every datagram stays under the 1280-byte IPv6 minimum MTU, with room for
// a TURN ChannelData header, so nothing depends on IP fragmentation.
export const MAX_DATAGRAM = 1200;
export const DATA_HEADER = 9, TAG = 16;
export const MAX_PLAINTEXT = MAX_DATAGRAM - DATA_HEADER - TAG;
// Streams carry ordered reliable messages; the state lane is unreliable.
export const STREAMS = Object.freeze({ events: 1, bulk: 2 });
export const LANES = Object.freeze(['state', 'events', 'bulk']);
export const MAX_MESSAGE = 1 << 20;

// QUIC variable-length integers (RFC 9000 section 16): 1, 2, 4 or 8 bytes.
export function varintSize(value) {
  if (value < 0x40) return 1;
  if (value < 0x4000) return 2;
  if (value < 0x40000000) return 4;
  if (value <= Number.MAX_SAFE_INTEGER) return 8;
  throw RangeError('varint too large');
}
export function writeVarint(buffer, offset, value) {
  if (!Number.isSafeInteger(value) || value < 0) throw RangeError('invalid varint');
  if (value < 0x40) { buffer[offset] = value; return offset + 1; }
  if (value < 0x4000) { buffer[offset] = 0x40 | (value >> 8); buffer[offset + 1] = value & 0xff; return offset + 2; }
  if (value < 0x40000000) { buffer.writeUInt32BE(value, offset); buffer[offset] |= 0x80; return offset + 4; }
  const high = Math.floor(value / 0x100000000), low = value >>> 0;
  buffer.writeUInt32BE(high, offset); buffer[offset] |= 0xc0; buffer.writeUInt32BE(low, offset + 4);
  return offset + 8;
}
// Returns [value, next offset]; throws on truncation.
export function readVarint(buffer, offset) {
  if (offset >= buffer.length) throw RangeError('truncated varint');
  const first = buffer[offset], length = 1 << (first >> 6);
  if (offset + length > buffer.length) throw RangeError('truncated varint');
  if (length === 1) return [first & 0x3f, offset + 1];
  if (length === 2) return [((first & 0x3f) << 8) | buffer[offset + 1], offset + 2];
  if (length === 4) return [(buffer.readUInt32BE(offset) & 0x3fffffff) >>> 0, offset + 4];
  const high = buffer.readUInt32BE(offset) & 0x3fffffff, low = buffer.readUInt32BE(offset + 4);
  const value = high * 0x100000000 + low;
  if (!Number.isSafeInteger(value)) throw RangeError('varint too large');
  return [value, offset + 8];
}

// Sorted, disjoint half-open ranges [start, end) of integers. Received
// packet numbers, acknowledged stream bytes and bytes to retransmit all use
// it; the sets stay small because traffic is mostly in order.
export class RangeSet {
  constructor() { this.ranges = []; }
  get size() { return this.ranges.length; }
  get empty() { return !this.ranges.length; }
  first() { return this.ranges[0]; }
  last() { return this.ranges.at(-1); }
  has(value) {
    let low = 0, high = this.ranges.length - 1;
    while (low <= high) {
      const middle = (low + high) >> 1, [start, end] = this.ranges[middle];
      if (value < start) high = middle - 1;
      else if (value >= end) low = middle + 1;
      else return true;
    }
    return false;
  }
  // True when every value in [start, end) is present.
  covers(start, end) {
    for (const [a, b] of this.ranges) { if (a <= start && b >= end) return true; if (a > start) return false; }
    return false;
  }
  add(start, end = start + 1) {
    if (end <= start) return;
    const ranges = this.ranges;
    let index = ranges.length;
    while (index > 0 && ranges[index - 1][0] > start) index--;
    // Merge with the predecessor when it touches or overlaps.
    if (index > 0 && ranges[index - 1][1] >= start) { index--; start = ranges[index][0]; end = Math.max(end, ranges[index][1]); ranges.splice(index, 1); }
    let remove = 0;
    while (index + remove < ranges.length && ranges[index + remove][0] <= end) { end = Math.max(end, ranges[index + remove][1]); remove++; }
    ranges.splice(index, remove, [start, end]);
  }
  remove(start, end = start + 1) {
    if (end <= start) return;
    const next = [];
    for (const [a, b] of this.ranges) {
      if (b <= start || a >= end) { next.push([a, b]); continue; }
      if (a < start) next.push([a, start]);
      if (b > end) next.push([end, b]);
    }
    this.ranges = next;
  }
  // Drop everything below `value` (bounded memory for old packet numbers).
  trimBelow(value) {
    while (this.ranges.length && this.ranges[0][1] <= value) this.ranges.shift();
    if (this.ranges.length && this.ranges[0][0] < value) this.ranges[0] = [value, this.ranges[0][1]];
  }
  // Pieces of [start, end) not in the set.
  missing(start, end) {
    const out = [];
    let cursor = start;
    for (const [a, b] of this.ranges) {
      if (b <= cursor) continue;
      if (a >= end) break;
      if (a > cursor) out.push([cursor, Math.min(a, end)]);
      cursor = Math.max(cursor, b);
      if (cursor >= end) break;
    }
    if (cursor < end) out.push([cursor, end]);
    return out;
  }
  clear() { this.ranges = []; }
}

// Address helpers. Paths are compared as `family|address|port` strings;
// IPv4-mapped IPv6 sources from dual-stack sockets normalize to IPv4.
export function normalizeAddress(address) {
  const value = String(address);
  const mapped = /^::ffff:(\d{1,3}(?:\.\d{1,3}){3})$/i.exec(value);
  return mapped ? mapped[1] : value;
}
// The text a socket reports for an address (RFC 5952 for IPv6), so a typed
// or signaled candidate keys the same path as the packets it answers with.
export function canonicalAddress(address) {
  const value = normalizeAddress(address);
  if (!value.includes(':') || value.includes('%')) return value;
  try { return normalizeAddress(new URL(`http://[${value}]/`).hostname.slice(1, -1)); } catch { return value; }
}
export function addressFamily(address) { return normalizeAddress(address).includes(':') ? 'IPv6' : 'IPv4'; }
export const pathKey = (address, port, via = '') => `${via}|${normalizeAddress(address)}|${port}`;
