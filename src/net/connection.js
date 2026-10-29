// One established BFE-UDP/1 session: packet protection, acknowledgement,
// loss recovery and the three lanes. Pure protocol logic: the endpoint owns
// sockets and paths and hands this class whole datagrams.
//
// - `state` messages are unreliable datagrams, sent at once and never
//   retransmitted or queued behind anything.
// - `events` and `bulk` are independent ordered reliable streams, so neither
//   blocks the other or the state lane (no cross-lane head-of-line blocking).
// - Acknowledgements ride every outgoing packet (QUIC-style ranges with an
//   ack delay), so loss is detected from the next acknowledgement instead of
//   a retransmission timer, and lost stream bytes travel in the next packet.
// - Messages queued in one task are coalesced into as few packets as fit:
//   a snapshot's entity parts and its own datagram usually share one packet.
import { randomBytes } from 'node:crypto';
import { DATA_HEADER, FRAME, MAX_MESSAGE, MAX_PLAINTEXT, PACKET, RangeSet, STREAMS, readVarint, varintSize, writeVarint } from './codec.js';

export const CLOSE_CODE = Object.freeze({ NORMAL: 0, IDLE: 1, PROTOCOL: 2, VERSION: 3, REJECTED: 4, KICKED: 5, SHUTDOWN: 6, APPLICATION: 7 });
const STREAM_LANES = new Map(Object.entries(STREAMS).map(([lane, id]) => [id, lane]));
const PACKET_THRESHOLD = 3, TIME_THRESHOLD = 9 / 8, GRANULARITY = 1;
const MAX_ACK_RANGES = 24, MAX_DECODED_ACK_RANGES = 64, REPLAY_WINDOW = 4096, MAX_RECEIVED_RANGES = 256, MAX_REORDER_THRESHOLD = 16;
// Reliable bytes a stream holds at either end: out-of-order bytes ahead of
// the delivery cursor, or written bytes the peer has not acknowledged. The
// segment and part limits bound the work a scattered sender can cause.
const MAX_STREAM_BUFFER = 4 << 20, MAX_STREAM_SEGMENTS = 1024, MAX_STREAM_PARTS = 4096, MAX_CONTROL_FRAMES = 16;
// A new peer address is challenged at most this often (and once per probe
// timeout while it keeps sending), so spoofed packets earn a victim no more
// than one small challenge each.
const CHALLENGE_INTERVAL = 50;
// Answers to challenges arriving on a path other than the session's (a peer
// measuring other routes) are budgeted: a burst, then this many per second.
const PROBE_BURST = 64, PROBE_RATE = 128, MAX_PROBES = 96, PROBE_LIFETIME = 2000;
// A path's far end: a client that moves to another local port reaches the
// same remote, which is not roaming.
const remoteOf = path => path?.remote ?? path?.key;
const MAX_FRAGMENTS = 32, MAX_PENDING_FRAGMENTS = 16, FRAGMENT_LIFETIME = 1000;
const FRAGMENT_BYTES = MAX_PLAINTEXT - 64, MAX_SENT_PACKETS = 4096, MAX_QUEUED_DATAGRAMS = 512;
const MAX_PACKET_NUMBER = 0xffffffff, MIN_WINDOW = 8 * 1200, MAX_WINDOW = 4 << 20;
const textEncoder = new TextEncoder();
// Every packet's plaintext is assembled here just before sealing (sealing
// copies it out, so one buffer serves every connection).
const plaintextScratch = Buffer.allocUnsafe(MAX_PLAINTEXT);
// Recent loss is counted in fixed time buckets (a busy server records
// thousands of samples a second across its sessions): LOSS_BUCKETS of
// LOSS_BUCKET_MS cover the one-second window plus the bucket in progress.
const LOSS_WINDOW = 1000, LOSS_BUCKET_MS = 50, LOSS_BUCKETS = LOSS_WINDOW / LOSS_BUCKET_MS + 1;
const toBuffer = value => Buffer.isBuffer(value) ? value : ArrayBuffer.isView(value) ? Buffer.from(value.buffer, value.byteOffset, value.byteLength) : Buffer.from(value);
export const MAX_STATE_MESSAGE = MAX_FRAGMENTS * FRAGMENT_BYTES;

// Bytes queued on a reliable stream: chunks at absolute offsets, the ranges
// acknowledged and the ranges declared lost (resent before new bytes).
class SendStream {
  constructor(id) { this.id = id; this.chunks = []; this.written = 0; this.sent = 0; this.acked = new RangeSet(); this.lost = new RangeSet(); this.base = 0; this.lostTotal = 0; }
  write(bytes) { if (bytes.length) { this.chunks.push({ offset: this.written, bytes }); this.written += bytes.length; } }
  get pending() { return this.written - this.sent + this.lostTotal; }
  // Written bytes not yet acknowledged (sent or not).
  get retained() { return this.written - this.base; }
  // The pieces of the chunks covering [offset, offset+length), uncopied:
  // the packet they go into copies them once when it is sealed.
  read(offset, length) {
    const parts = [];
    for (const chunk of this.chunks) {
      if (chunk.offset >= offset + length) break;
      const start = Math.max(offset, chunk.offset), end = Math.min(offset + length, chunk.offset + chunk.bytes.length);
      if (end > start) parts.push(chunk.bytes.subarray(start - chunk.offset, end - chunk.offset));
    }
    return parts;
  }
  // The next range to send: lost bytes first (up to `max`), then new ones
  // (up to `newMax`, the congestion window's remainder).
  take(max, newMax) {
    if (max <= 0) return null;
    if (!this.lost.empty) {
      const [start, end] = this.lost.first(), length = Math.min(end - start, max);
      this.lost.remove(start, start + length); this.lostTotal -= length;
      return { offset: start, length, parts: this.read(start, length), retransmit: true };
    }
    if (newMax <= 0 || this.sent >= this.written) return null;
    const length = Math.min(this.written - this.sent, max, newMax), offset = this.sent;
    this.sent += length;
    return { offset, length, parts: this.read(offset, length), retransmit: false };
  }
  ack(offset, length) {
    this.acked.add(offset, offset + length);
    for (const [a, b] of this.lost.ranges) if (a < offset + length && b > offset) this.lostTotal -= Math.min(b, offset + length) - Math.max(a, offset);
    this.lost.remove(offset, offset + length);
    const first = this.acked.first();
    if (first && first[0] <= this.base && first[1] > this.base) { this.base = first[1]; this.acked.trimBelow(this.base); }
    while (this.chunks.length && this.chunks[0].offset + this.chunks[0].bytes.length <= this.base) this.chunks.shift();
  }
  // Bytes of [offset, offset+length) neither acknowledged nor already queued.
  loss(offset, length) {
    for (const [a, b] of this.acked.missing(Math.max(offset, this.base), offset + length)) {
      for (const [c, d] of this.lost.missing(a, b)) { this.lost.add(c, d); this.lostTotal += d - c; }
    }
  }
}

// Reassembles one ordered stream and splits it into messages framed as
// varint length, kind (0 binary, 1 text), payload. Bytes ahead of the
// delivery cursor are held as runs merged on arrival, one per gap, so a
// sender scattering tiny frames hits a limit instead of quadratic work.
class ReceiveStream {
  constructor(id) { this.id = id; this.offset = 0; this.segments = []; this.parts = 0; this.buffered = 0; this.queue = []; this.queued = 0; this.need = 0; this.kind = -1; }
  receive(offset, bytes, deliver) {
    const end = offset + bytes.length;
    if (end <= this.offset) return;
    if (end - this.offset > MAX_STREAM_BUFFER) throw Error('stream buffer exceeded');
    if (offset < this.offset) { bytes = bytes.subarray(this.offset - offset); offset = this.offset; }
    if (offset > this.offset) { this.store(offset, bytes); return; }
    this.push(bytes);
    this.offset = end;
    while (this.segments.length && this.segments[0].start <= this.offset) {
      const segment = this.segments.shift();
      this.buffered -= segment.end - segment.start; this.parts -= segment.parts.length;
      if (segment.end <= this.offset) continue;
      let position = segment.start;
      for (const part of segment.parts) {
        if (position + part.length > this.offset) this.push(part.subarray(Math.max(0, this.offset - position)));
        position += part.length;
      }
      this.offset = segment.end;
    }
    this.parse(deliver);
  }
  // Out-of-order bytes join every run they touch; only bytes not already
  // held are copied.
  store(start, bytes) {
    const end = start + bytes.length, segments = this.segments;
    let low = 0, high = segments.length;
    while (low < high) { const mid = (low + high) >>> 1; if (segments[mid].end < start) low = mid + 1; else high = mid; }
    let last = low;
    while (last < segments.length && segments[last].start <= end) last++;
    if (low === last) {
      if (segments.length >= MAX_STREAM_SEGMENTS) throw Error('stream reassembly limit exceeded');
      segments.splice(low, 0, { start, end, parts: [this.piece(bytes, 0, bytes.length)] });
      return;
    }
    const first = segments[low];
    let parts = first.parts;
    if (start < first.start) parts = [this.piece(bytes, 0, first.start - start), ...parts];
    let position = first.end;
    for (let index = low + 1; index < last; index++) {
      const segment = segments[index];
      parts.push(this.piece(bytes, position - start, segment.start - start));
      for (const part of segment.parts) parts.push(part);
      position = segment.end;
    }
    if (end > position) { parts.push(this.piece(bytes, position - start, end - start)); position = end; }
    first.start = Math.min(start, first.start); first.end = position; first.parts = parts;
    if (last - low > 1) segments.splice(low + 1, last - low - 1);
  }
  piece(bytes, from, to) {
    if (++this.parts > MAX_STREAM_PARTS) throw Error('stream reassembly limit exceeded');
    this.buffered += to - from;
    return Buffer.from(bytes.subarray(from, to));
  }
  push(bytes) { if (bytes.length) { this.queue.push(Buffer.from(bytes)); this.queued += bytes.length; } }
  peek(count) {
    const out = Buffer.alloc(Math.min(count, this.queued));
    let written = 0;
    for (const piece of this.queue) { written += piece.copy(out, written, 0, Math.min(piece.length, out.length - written)); if (written >= out.length) break; }
    return out;
  }
  take(count) {
    const parts = [];
    let remaining = count;
    while (remaining > 0) {
      const piece = this.queue[0];
      if (piece.length <= remaining) { parts.push(piece); this.queue.shift(); remaining -= piece.length; }
      else { parts.push(piece.subarray(0, remaining)); this.queue[0] = piece.subarray(remaining); remaining = 0; }
    }
    this.queued -= count;
    return parts.length === 1 ? parts[0] : Buffer.concat(parts, count);
  }
  parse(deliver) {
    for (;;) {
      if (this.kind < 0) {
        const head = this.peek(9);
        let length, next;
        try { [length, next] = readVarint(head, 0); } catch { return; }
        if (length > MAX_MESSAGE) throw Error('stream message too large');
        if (head.length < next + 1) return;
        if (head[next] > 1) throw Error('invalid stream message kind');
        this.take(next + 1);
        this.kind = head[next]; this.need = length;
      }
      if (this.queued < this.need) return;
      const payload = this.need ? this.take(this.need) : Buffer.alloc(0), kind = this.kind;
      this.kind = -1; this.need = 0;
      deliver(kind === 1 ? payload.toString('utf8') : payload);
    }
  }
}

// Sliding anti-replay window over packet numbers (as IPsec and WireGuard):
// a bit per number below the largest seen, independent of loss patterns.
export class ReplayWindow {
  constructor(size = REPLAY_WINDOW) { this.size = size; this.bits = new Uint32Array(size >>> 5); this.largest = -1; }
  index(number) { const slot = number % this.size; return [slot >>> 5, 1 << (slot & 31)]; }
  seen(number) {
    if (number > this.largest) return false;
    if (number <= this.largest - this.size) return true;
    const [word, bit] = this.index(number);
    return (this.bits[word] & bit) !== 0;
  }
  mark(number) {
    if (number > this.largest) {
      if (number - this.largest >= this.size) this.bits.fill(0);
      // Clear the skipped numbers, a whole word at a time where aligned.
      else for (let n = this.largest + 1; n < number;) {
        const slot = n % this.size;
        if ((slot & 31) === 0 && number - n >= 32) { this.bits[slot >>> 5] = 0; n += 32; }
        else { this.bits[slot >>> 5] &= ~(1 << (slot & 31)); n++; }
      }
      this.largest = number;
    }
    const [word, bit] = this.index(number);
    this.bits[word] |= bit;
  }
}

export function encodeAck(received, delayMicros) {
  const ranges = received.ranges, top = ranges.length - 1;
  if (top < 0) return null;
  const count = Math.min(ranges.length, MAX_ACK_RANGES);
  const largest = ranges[top][1] - 1, delay = Math.max(0, Math.round(delayMicros / 8)), first = largest - ranges[top][0];
  let size = 1 + varintSize(largest) + varintSize(delay) + varintSize(count - 1) + varintSize(first);
  for (let i = top - 1; i > top - count; i--) size += varintSize(ranges[i + 1][0] - ranges[i][1] - 1) + varintSize(ranges[i][1] - 1 - ranges[i][0]);
  const out = Buffer.allocUnsafe(size);
  out[0] = FRAME.ACK;
  let offset = writeVarint(out, writeVarint(out, writeVarint(out, writeVarint(out, 1, largest), delay), count - 1), first);
  for (let i = top - 1; i > top - count; i--) offset = writeVarint(out, writeVarint(out, offset, ranges[i + 1][0] - ranges[i][1] - 1), ranges[i][1] - 1 - ranges[i][0]);
  return out;
}
// Returns the acknowledgement {largest, delay (µs), ranges:[[start,end)...]}
// in descending order, and the offset after the frame.
export function decodeAck(buffer, offset) {
  let largest, delay, count, first;
  [largest, offset] = readVarint(buffer, offset);
  [delay, offset] = readVarint(buffer, offset);
  [count, offset] = readVarint(buffer, offset);
  [first, offset] = readVarint(buffer, offset);
  if (first > largest || count >= MAX_DECODED_ACK_RANGES) throw Error('invalid ack');
  const ranges = [[largest - first, largest + 1]];
  let smallest = largest - first;
  for (let i = 0; i < count; i++) {
    let gap, length;
    [gap, offset] = readVarint(buffer, offset);
    [length, offset] = readVarint(buffer, offset);
    const high = smallest - gap - 2, low = high - length;
    if (low < 0) throw Error('invalid ack');
    ranges.push([low, high + 1]);
    smallest = low;
  }
  return { ack: { largest, delay: delay * 8, ranges }, offset };
}
// Binary search over the descending ranges.
function ackedBy(ranges, number) {
  let low = 0, high = ranges.length - 1;
  while (low <= high) {
    const mid = (low + high) >>> 1, [start, end] = ranges[mid];
    if (number >= end) high = mid - 1; else if (number < start) low = mid + 1; else return true;
  }
  return false;
}

export class Connection {
  constructor({ role, localSession, remoteSession, send, receive, transmit, onMessage = () => {}, onClose = () => {}, onPath = () => {},
    now = () => performance.now(), setTimer = (fn, ms) => setTimeout(fn, ms), clearTimer = id => clearTimeout(id), schedule = queueMicrotask,
    initialRtt = 100, idleTimeoutMs = 20000, keepaliveMs = 1000, maxAckDelayMs = 25, datagramAckDelayMs = 40,
    keyUpdateEvery = 1 << 20, initialWindow = 32 * 1200, path = null }) {
    Object.assign(this, { role, localSession, remoteSession, transmit, onMessage, onClose, onPath, now, setTimer, clearTimer, schedule,
      idleTimeoutMs, keepaliveMs, maxAckDelayMs, datagramAckDelayMs, keyUpdateEvery, path });
    this.sendAead = send; this.sendPhase = 0; this.receiveAead = receive; this.receivePhase = 0; this.receiveNext = null; this.receivePrevious = null; this.phaseStart = 0;
    this.nextNumber = 0; this.sentPackets = new Map(); this.reliableInFlight = 0; this.largestAcked = -1; this.lossTime = Infinity; this.ptoCount = 0; this.lastReliableSent = -Infinity;
    this.received = new RangeSet(); this.replay = new ReplayWindow(); this.largestReceived = -1; this.largestReceivedAt = -Infinity; this.ackDeadline = Infinity; this.ackNeeded = false;
    this.smoothedRtt = initialRtt; this.rttVar = initialRtt / 2; this.minRtt = Infinity; this.latestRtt = initialRtt; this.hasRttSample = false;
    this.window = initialWindow; this.threshold = Infinity; this.streamInFlight = 0; this.recoveryStart = -Infinity;
    this.reorderThreshold = PACKET_THRESHOLD; this.timeThreshold = TIME_THRESHOLD; this.declaredLost = new Map();
    this.sendStreams = new Map(Object.values(STREAMS).map(id => [id, new SendStream(id)]));
    this.receiveStreams = new Map(Object.values(STREAMS).map(id => [id, new ReceiveStream(id)]));
    this.datagrams = []; this.fragmentId = 0; this.fragments = new Map();
    this.control = []; this.pingQueued = false; this.handshakeDoneQueued = false; this.handshakeConfirmed = false;
    this.challenge = null; this.lastChallengeAt = -Infinity;
    this.probes = new Map(); this.onProbe = null; this.probeTokens = PROBE_BURST; this.probeTokensAt = -Infinity;
    this.closed = false; this.closeFrame = null; this.timer = null; this.timerAt = Infinity; this.flushQueued = false;
    const at = this.now();
    this.lastReceivedAt = at; this.lastSentAt = at; this.createdAt = at;
    this.counters = { packetsSent: 0, packetsReceived: 0, bytesSent: 0, bytesReceived: 0, datagramsSent: 0, datagramsReceived: 0,
      datagramPacketsLost: 0, datagramPacketsAcked: 0, packetsLost: 0, spuriousLosses: 0, retransmittedBytes: 0, duplicates: 0, authFailures: 0, dropped: 0, ptoProbes: 0 };
    this.lossBuckets = { id: new Float64Array(LOSS_BUCKETS).fill(-1), lost: new Uint32Array(LOSS_BUCKETS), total: new Uint32Array(LOSS_BUCKETS) };
    this.arm();
  }

  // ---- Application API ----

  // Queues one message. It leaves in the coalesced packets built when the
  // current task ends (or at once with flush()). False once closed.
  send(lane, payload) {
    if (this.closed) return false;
    const text = typeof payload === 'string';
    const bytes = text ? Buffer.from(textEncoder.encode(payload)) : toBuffer(payload);
    if (lane === 'state') {
      if (bytes.length > MAX_STATE_MESSAGE) throw Error('state message too large');
      this.datagrams.push({ bytes, text });
      if (this.datagrams.length > MAX_QUEUED_DATAGRAMS) { this.datagrams.shift(); this.counters.dropped++; }
    } else {
      const id = STREAMS[lane];
      if (!id) throw Error('unknown lane ' + lane);
      if (bytes.length > MAX_MESSAGE) throw Error('message too large');
      const header = Buffer.alloc(varintSize(bytes.length) + 1);
      header[writeVarint(header, 0, bytes.length)] = text ? 1 : 0;
      const stream = this.sendStreams.get(id);
      // A peer that stays alive but never acknowledges cannot make this end
      // buffer without bound.
      if (stream.retained + header.length + bytes.length > MAX_STREAM_BUFFER) { this.close(CLOSE_CODE.PROTOCOL, 'reliable data not acknowledged'); return false; }
      stream.write(header); stream.write(bytes);
    }
    this.queueFlush();
    return true;
  }
  queueFlush() { if (!this.flushQueued && !this.closed) { this.flushQueued = true; this.schedule(() => { this.flushQueued = false; this.flush(); }); } }
  ping() { this.pingQueued = true; this.queueFlush(); }
  confirmHandshake() { this.handshakeDoneQueued = true; this.queueFlush(); }
  // Bytes waiting on the reliable streams (unsent or awaiting resend).
  get streamBacklog() { let n = 0; for (const stream of this.sendStreams.values()) n += stream.pending; return n; }
  get rtt() { return this.smoothedRtt; }
  stats() {
    const lost = this.counters.datagramPacketsLost, acked = this.counters.datagramPacketsAcked;
    return { rtt: this.smoothedRtt, minRtt: Number.isFinite(this.minRtt) ? this.minRtt : null, rttVar: this.rttVar, latestRtt: this.latestRtt,
      window: this.window, streamInFlight: this.streamInFlight, streamBacklog: this.streamBacklog, inFlight: this.sentPackets.size,
      loss: this.recentLoss(), lossTotal: lost + acked ? lost / (lost + acked) : 0, ...this.counters };
  }
  // Fraction of state-carrying packets lost over roughly the last second
  // (the last 1000 to 1050 ms, by bucket).
  recentLoss() {
    const { id, lost, total } = this.lossBuckets, oldest = Math.floor((this.now() - LOSS_WINDOW) / LOSS_BUCKET_MS);
    let lostCount = 0, count = 0;
    for (let slot = 0; slot < LOSS_BUCKETS; slot++) if (id[slot] >= oldest) { lostCount += lost[slot]; count += total[slot]; }
    return count ? lostCount / count : 0;
  }
  lossSample(now, lostPacket) {
    const bucket = Math.floor(now / LOSS_BUCKET_MS), slot = ((bucket % LOSS_BUCKETS) + LOSS_BUCKETS) % LOSS_BUCKETS, { id, lost, total } = this.lossBuckets;
    if (id[slot] !== bucket) { id[slot] = bucket; lost[slot] = 0; total[slot] = 0; }
    total[slot]++;
    if (lostPacket) lost[slot]++;
  }

  close(code = CLOSE_CODE.NORMAL, reason = '') {
    if (this.closed) return;
    const text = Buffer.from(String(reason).slice(0, 200));
    const frame = Buffer.alloc(1 + varintSize(code) + varintSize(text.length) + text.length);
    frame[0] = FRAME.CLOSE;
    text.copy(frame, writeVarint(frame, writeVarint(frame, 1, code), text.length));
    this.closeFrame = frame;
    this.datagrams = []; this.control = [];
    this.emitPacket([frame], { reliable: false, datagrams: 0, streams: [] });
    this.finish({ code, reason: String(reason), remote: false });
  }
  finish(info) {
    if (this.closed) return;
    this.closed = true;
    if (this.timer !== null) { this.clearTimer(this.timer); this.timer = null; }
    // Stray packets after a local close get the CLOSE again for a moment.
    this.drainUntil = this.now() + Math.max(1000, 3 * this.pto());
    this.onClose(info);
  }

  // ---- Receive ----

  // A DATA packet addressed to this session. Returns true when authentic.
  receive(packet, path = null) {
    if (this.closed) {
      const now = this.now();
      if (this.closeFrame && now < this.drainUntil && now - (this.lastCloseEcho ?? -Infinity) > 100) { this.lastCloseEcho = now; this.emitPacket([this.closeFrame], { reliable: false, datagrams: 0, streams: [] }); }
      return false;
    }
    if (packet.length < DATA_HEADER + 17) return false;
    const phase = packet[0] === PACKET.DATA_PHASE ? 1 : 0, number = packet.readUInt32LE(5);
    if (this.replay.seen(number)) { this.counters.duplicates++; return false; }
    let plaintext = null, rotated = false;
    if (phase === this.receivePhase) plaintext = this.receiveAead.open(packet, DATA_HEADER, number);
    else {
      // A new key phase from the peer, or a straggler from the previous one.
      if (this.receivePrevious && number < this.phaseStart) plaintext = this.receivePrevious.open(packet, DATA_HEADER, number);
      if (!plaintext) { this.receiveNext ??= this.receiveAead.next(); plaintext = this.receiveNext.open(packet, DATA_HEADER, number); rotated = !!plaintext; }
    }
    if (!plaintext) { this.counters.authFailures++; return false; }
    if (rotated) { this.receivePrevious = this.receiveAead; this.receiveAead = this.receiveNext; this.receiveNext = null; this.receivePhase = phase; this.phaseStart = number; }
    const now = this.now();
    this.replay.mark(number);
    this.received.add(number);
    const newest = number > this.largestReceived;
    if (newest) { this.largestReceived = number; this.largestReceivedAt = now; }
    // Acknowledgement ranges only need recent history; replay checks use
    // the bitmap, so trimming here cannot readmit an old packet.
    if (this.received.size > MAX_RECEIVED_RANGES) this.received.trimBelow(this.received.ranges.at(-MAX_ACK_RANGES)[0]);
    this.received.trimBelow(this.largestReceived - REPLAY_WINDOW + 1);
    this.counters.packetsReceived++; this.counters.bytesReceived += packet.length;
    this.lastReceivedAt = now;
    let probing;
    try { probing = this.frames(plaintext, now, path); }
    catch (error) { this.close(CLOSE_CODE.PROTOCOL, 'protocol error: ' + error.message); return true; }
    if (this.closed) return true;
    // Roaming (NAT rebinding, Wi-Fi to LTE): the newest authentic packet
    // from a new remote address starts a path challenge, and the session
    // moves there only when the answer comes back (RFC 9000 section 8.2).
    // Without that, a peer could spoof its own packets from a victim's
    // address and point this session's traffic at it. Probing packets
    // (challenges and responses only) never move the session: a peer sends
    // them to measure other routes.
    if (newest && !probing && path && this.path && remoteOf(path) !== remoteOf(this.path)) this.challengePath(path, now);
    if ((this.ackNeeded && now >= this.ackDeadline) || this.control.length || this.handshakeDoneQueued) this.flush();
    else this.arm();
    return true;
  }
  // Returns whether the packet only probed (PATH_CHALLENGE, PATH_RESPONSE
  // and PADDING frames, as RFC 9000 section 9.1 defines probing).
  frames(plaintext, now, path = null) {
    let offset = 0, elicit = 0, acked = false, probing = true, answered = false;
    const need = count => { if (offset + count > plaintext.length) throw Error('truncated frame'); };
    while (offset < plaintext.length) {
      const type = plaintext[offset++];
      if (type === FRAME.PADDING) continue;
      if (type !== FRAME.PATH_CHALLENGE && type !== FRAME.PATH_RESPONSE) probing = false;
      if (type === FRAME.PING) { elicit = 2; continue; }
      if (type === FRAME.HANDSHAKE_DONE) { elicit = 2; this.handshakeConfirmed = true; continue; }
      if (type === FRAME.ACK) {
        // One acknowledgement per packet, of packets actually sent: each
        // costs a pass over the packets in flight.
        if (acked) throw Error('repeated ack frame');
        const result = decodeAck(plaintext, offset); offset = result.offset; acked = true;
        if (result.ack.largest >= this.nextNumber) throw Error('ack of an unsent packet');
        this.onAck(result.ack, now);
        continue;
      }
      if (type === FRAME.DATAGRAM) {
        need(1);
        const flags = plaintext[offset++];
        let length; [length, offset] = readVarint(plaintext, offset);
        need(length);
        const bytes = plaintext.subarray(offset, offset + length);
        offset += length; elicit = Math.max(elicit, 1);
        this.counters.datagramsReceived++;
        // The plaintext is a fresh buffer per packet (Aead.open), so the
        // message can be a view of it rather than a copy.
        this.deliver('state', flags & 1 ? bytes.toString('utf8') : bytes);
        continue;
      }
      if (type === FRAME.FRAGMENT) {
        need(1);
        const flags = plaintext[offset++];
        let id, length; [id, offset] = readVarint(plaintext, offset);
        need(2);
        const index = plaintext[offset++], count = plaintext[offset++];
        [length, offset] = readVarint(plaintext, offset);
        need(length);
        if (!count || count > MAX_FRAGMENTS || index >= count) throw Error('invalid fragment');
        this.fragment(id, index, count, flags & 1, plaintext.subarray(offset, offset + length), now);
        offset += length; elicit = Math.max(elicit, 1);
        continue;
      }
      if (type === FRAME.STREAM) {
        need(1);
        const id = plaintext[offset++];
        let start, length; [start, offset] = readVarint(plaintext, offset); [length, offset] = readVarint(plaintext, offset);
        need(length);
        const stream = this.receiveStreams.get(id);
        if (!stream) throw Error('unknown stream');
        const lane = STREAM_LANES.get(id);
        stream.receive(start, plaintext.subarray(offset, offset + length), message => this.deliver(lane, message));
        offset += length; elicit = 2;
        continue;
      }
      if (type === FRAME.PATH_CHALLENGE) {
        need(8);
        // One answer per packet, however many challenges it carries.
        if (!answered) { answered = true; this.answerChallenge(plaintext.subarray(offset, offset + 8), path, now); }
        offset += 8; elicit = 2;
        continue;
      }
      if (type === FRAME.PATH_RESPONSE) {
        need(8);
        const data = plaintext.subarray(offset, offset + 8);
        if (this.challenge?.data.equals(data)) this.migrate(); else this.probeAnswered(data, now);
        offset += 8;
        continue;
      }
      if (type === FRAME.CLOSE) {
        let code, length; [code, offset] = readVarint(plaintext, offset); [length, offset] = readVarint(plaintext, offset);
        need(length);
        this.finish({ code, reason: plaintext.subarray(offset, offset + length).toString('utf8'), remote: true });
        return;
      }
      throw Error('unknown frame ' + type);
    }
    if (elicit) {
      this.ackNeeded = true;
      // Reliable data is acknowledged promptly for fast loss recovery;
      // state-only packets wait for the next outgoing packet to carry it.
      this.ackDeadline = Math.min(this.ackDeadline, now + (elicit > 1 ? this.maxAckDelayMs : this.datagramAckDelayMs));
    }
    return probing;
  }
  // A PATH_RESPONSE goes back on the path its challenge came on (RFC 9000
  // section 8.2.2), so a peer can time that route. On the session's own path
  // it rides the next packet; on another path it leaves at once, within a
  // budget (it goes to an address this end has not validated).
  answerChallenge(data, path, now) {
    const frame = Buffer.alloc(9); frame[0] = FRAME.PATH_RESPONSE; data.copy(frame, 1);
    if (!path || !this.path || path.key === this.path.key) { if (this.control.length < MAX_CONTROL_FRAMES) this.control.push(frame); return; }
    this.probeTokens = Math.min(PROBE_BURST, this.probeTokens + (now - this.probeTokensAt) * PROBE_RATE / 1000); this.probeTokensAt = now;
    if (this.probeTokens < 1) return;
    this.probeTokens--;
    this.emitPacket([frame], { reliable: false, datagrams: 0, streams: [] }, path);
  }
  // Times another route to the peer: a PATH_CHALLENGE on `path`, whose
  // round trip onProbe(path, ms) reports. Probing never moves the session.
  // (Not probe(): that is the loss-recovery probe timeout below.)
  probePath(path) {
    if (this.closed) return false;
    const now = this.now();
    for (const [key, record] of this.probes) if (now - record.sentAt > PROBE_LIFETIME) this.probes.delete(key);
    if (this.probes.size >= MAX_PROBES) return false;
    const data = randomBytes(8), frame = Buffer.alloc(9); frame[0] = FRAME.PATH_CHALLENGE; data.copy(frame, 1);
    this.probes.set(data.toString('hex'), { path, sentAt: now });
    this.emitPacket([frame], { reliable: false, datagrams: 0, streams: [] }, path);
    return true;
  }
  probeAnswered(data, now) {
    const key = Buffer.from(data).toString('hex'), record = this.probes.get(key);
    if (!record) return;
    this.probes.delete(key);
    this.onProbe?.(record.path, now - record.sentAt);
  }
  // This end moves to another local route to the same peer (a faster
  // port). The peer sees a new address and validates it before moving.
  usePath(path) {
    const previous = this.path;
    this.path = path;
    this.onPath(path, previous);
    this.ping();
  }
  deliver(lane, payload) { if (!this.closed) this.onMessage(lane, payload); }
  // One outstanding challenge, to the newest address; resent only while
  // that address keeps sending and a probe timeout has passed.
  challengePath(path, now) {
    let challenge = this.challenge;
    if (challenge?.path.key !== path.key) {
      if (now - this.lastChallengeAt < CHALLENGE_INTERVAL) return;
      challenge = this.challenge = { path, data: randomBytes(8), sentAt: -Infinity };
    } else if (now - challenge.sentAt < Math.max(this.pto(), CHALLENGE_INTERVAL)) return;
    challenge.sentAt = now; this.lastChallengeAt = now;
    const frame = Buffer.alloc(9); frame[0] = FRAME.PATH_CHALLENGE; challenge.data.copy(frame, 1);
    this.emitPacket([frame], { reliable: false, datagrams: 0, streams: [] }, path);
  }
  migrate() {
    const previous = this.path;
    this.path = this.challenge.path; this.challenge = null;
    this.onPath(this.path, previous);
  }
  fragment(id, index, count, text, bytes, now) {
    for (const [key, item] of this.fragments) if (now - item.created > FRAGMENT_LIFETIME) this.fragments.delete(key);
    let item = this.fragments.get(id);
    if (!item) {
      if (this.fragments.size >= MAX_PENDING_FRAGMENTS) this.fragments.delete(this.fragments.keys().next().value);
      item = { created: now, count, parts: new Array(count), received: 0 };
      this.fragments.set(id, item);
    }
    if (item.count !== count || item.parts[index]) return;
    item.parts[index] = bytes; item.received++;
    if (item.received < count) return;
    this.fragments.delete(id);
    const whole = Buffer.concat(item.parts);
    this.counters.datagramsReceived++;
    this.deliver('state', text ? whole.toString('utf8') : whole);
  }

  // ---- Acknowledgements and loss (after RFC 9002) ----

  onAck(ack, now) {
    let largestTime = -Infinity, newlyAcked = false, ackedStream = 0;
    // A packet declared lost but acknowledged after all was only reordered:
    // tolerate more reordering before declaring loss (as RACK adapts).
    for (const [number, at] of this.declaredLost) {
      if (now - at > 2000) { this.declaredLost.delete(number); continue; }
      if (ackedBy(ack.ranges, number)) {
        this.declaredLost.delete(number); this.counters.spuriousLosses++;
        this.reorderThreshold = Math.min(MAX_REORDER_THRESHOLD, this.reorderThreshold + 1);
        this.timeThreshold = Math.min(2, this.timeThreshold + 0.125);
      }
    }
    // Packets in flight ascend and ranges descend: one merged pass.
    const ranges = ack.ranges;
    let range = ranges.length - 1;
    for (const [number, packet] of this.sentPackets) {
      if (number > ack.largest) break;
      while (number >= ranges[range][1]) range--;
      if (number < ranges[range][0]) continue;
      this.forget(number, packet);
      newlyAcked = true;
      if (number === ack.largest && (packet.reliable || packet.datagrams)) largestTime = packet.time;
      for (const [id, offset, length] of packet.streams) { this.sendStreams.get(id).ack(offset, length); ackedStream += length; }
      if (packet.datagrams) { this.counters.datagramPacketsAcked++; this.lossSample(now, false); }
    }
    if (!newlyAcked) return;
    if (ack.largest > this.largestAcked) this.largestAcked = ack.largest;
    if (Number.isFinite(largestTime)) this.rttSample(now - largestTime, Math.min(ack.delay / 1000, Math.max(this.maxAckDelayMs, this.datagramAckDelayMs)));
    if (ackedStream) this.grow(ackedStream);
    this.ptoCount = 0;
    this.detectLoss(now);
    if (this.streamBacklog) this.queueFlush();
  }
  forget(number, packet) {
    this.sentPackets.delete(number);
    if (packet.reliable) this.reliableInFlight--;
    for (const [, , length] of packet.streams) this.streamInFlight -= length;
  }
  // Both ends run this implementation, so the reported acknowledgement delay
  // (the time a packet waited to ride outgoing traffic) is trusted and
  // removed: PING shows the network round trip, not the send interval.
  rttSample(sample, ackDelay) {
    if (!(sample >= 0)) return;
    const adjusted = Math.max(GRANULARITY, sample - ackDelay);
    this.latestRtt = adjusted;
    this.minRtt = Math.min(this.minRtt, adjusted);
    if (!this.hasRttSample) { this.smoothedRtt = adjusted; this.rttVar = adjusted / 2; this.hasRttSample = true; return; }
    this.rttVar = 0.75 * this.rttVar + 0.25 * Math.abs(this.smoothedRtt - adjusted);
    this.smoothedRtt = 0.875 * this.smoothedRtt + 0.125 * adjusted;
  }
  lossDelay() { return Math.max(this.timeThreshold * Math.max(this.latestRtt, this.smoothedRtt), GRANULARITY); }
  pto() { return this.smoothedRtt + Math.max(4 * this.rttVar, GRANULARITY) + this.maxAckDelayMs; }
  detectLoss(now) {
    this.lossTime = Infinity;
    const delay = this.lossDelay();
    let lostStream = false, newestLostTime = -Infinity;
    for (const [number, packet] of this.sentPackets) {
      if (number > this.largestAcked) break;
      if (number + this.reorderThreshold <= this.largestAcked || packet.time <= now - delay) {
        this.lost(number, packet, now);
        if (packet.streams.length) { lostStream = true; newestLostTime = Math.max(newestLostTime, packet.time); }
      } else this.lossTime = Math.min(this.lossTime, packet.time + delay);
    }
    // One window reduction per round trip (NewReno-style recovery period).
    if (lostStream && newestLostTime > this.recoveryStart) {
      this.recoveryStart = now;
      this.window = Math.max(MIN_WINDOW, Math.floor(this.window / 2));
      this.threshold = this.window;
    }
    if (lostStream) this.queueFlush();
  }
  lost(number, packet, now) {
    this.forget(number, packet);
    this.declaredLost.set(number, now);
    if (this.declaredLost.size > 256) this.declaredLost.delete(this.declaredLost.keys().next().value);
    this.counters.packetsLost++;
    if (packet.datagrams) { this.counters.datagramPacketsLost++; this.lossSample(now, true); }
    for (const [id, offset, length] of packet.streams) this.sendStreams.get(id).loss(offset, length);
  }
  grow(bytes) {
    if (this.window < this.threshold) this.window += bytes;
    else this.window += Math.max(1, Math.floor(1200 * bytes / this.window));
    this.window = Math.min(this.window, MAX_WINDOW);
  }

  // ---- Timers ----

  ptoDeadline() { return this.reliableInFlight > 0 ? this.lastReliableSent + this.pto() * 2 ** Math.min(this.ptoCount, 6) : Infinity; }
  // Every packet moves some deadline, so the timer is replaced only when the
  // new deadline comes before the armed one (a timer that fires early finds
  // nothing due and re-arms). Delays are whole milliseconds: Node keeps one
  // timer list per distinct duration.
  arm() {
    if (this.closed) return;
    const deadline = Math.min(this.lastReceivedAt + this.idleTimeoutMs, this.lastSentAt + this.keepaliveMs, this.ackDeadline, this.lossTime, this.ptoDeadline());
    if (this.timer !== null && this.timerAt <= deadline + 2) return;
    if (this.timer !== null) this.clearTimer(this.timer);
    const now = this.now(), delay = Math.min(Math.ceil(Math.max(0, deadline - now) + 0.25), 2 ** 31 - 1);
    this.timerAt = now + delay;
    this.timer = this.setTimer(() => { this.timer = null; this.timerAt = Infinity; this.onTimer(); }, delay);
  }
  onTimer() {
    if (this.closed) return;
    const now = this.now();
    if (now - this.lastReceivedAt >= this.idleTimeoutMs) { this.close(CLOSE_CODE.IDLE, 'idle timeout'); return; }
    if (now >= this.lossTime) this.detectLoss(now);
    if (now >= this.ptoDeadline()) this.probe();
    if (now - this.lastSentAt >= this.keepaliveMs) this.pingQueued = true;
    this.flush();
  }
  // Probe timeout: reliable data unacknowledged for a PTO. Resend the
  // oldest unacknowledged stream bytes (or a PING) to elicit an ACK.
  probe() {
    this.ptoCount++; this.counters.ptoProbes++;
    for (const packet of this.sentPackets.values()) {
      if (!packet.streams.length) continue;
      for (const [id, offset, length] of packet.streams) this.sendStreams.get(id).loss(offset, length);
      return;
    }
    this.pingQueued = true;
  }

  // ---- Send ----

  flush() {
    if (this.closed) return;
    const now = this.now();
    for (let first = true, guard = 0; guard < 256; first = false, guard++) {
      const frames = [], meta = { streams: [], datagrams: 0, reliable: false };
      let space = MAX_PLAINTEXT, ack = null;
      // Acknowledge in every packet while traffic flows: a few bytes, and a
      // lost acknowledgement is repaired by the very next packet.
      if (first && this.largestReceived >= 0 && (this.ackNeeded || now - this.largestReceivedAt < 2000)) {
        ack = encodeAck(this.received, (now - this.largestReceivedAt) * 1000);
        frames.push(ack); space -= ack.length;
      }
      while (this.control.length && this.control[0].length <= space) { const frame = this.control.shift(); frames.push(frame); space -= frame.length; meta.reliable = true; }
      if (this.handshakeDoneQueued) { frames.push(Buffer.of(FRAME.HANDSHAKE_DONE)); space--; meta.reliable = true; this.handshakeDoneQueued = false; }
      if (this.pingQueued) { frames.push(Buffer.of(FRAME.PING)); space--; meta.reliable = true; this.pingQueued = false; }
      space = this.packDatagrams(frames, meta, space);
      this.packStreams(frames, meta, space);
      if (!meta.reliable && !meta.datagrams) {
        // Only an acknowledgement: send it alone only when one is due.
        if (ack && this.ackNeeded && now >= this.ackDeadline) { this.emitPacket(frames, meta); this.ackSent(); }
        break;
      }
      this.emitPacket(frames, meta);
      if (ack) this.ackSent();
      if (!this.datagrams.length && !this.control.length && !this.streamSendable()) break;
    }
    this.arm();
  }
  ackSent() { this.ackNeeded = false; this.ackDeadline = Infinity; }
  streamSendable() {
    for (const stream of this.sendStreams.values()) if (stream.lostTotal || (stream.sent < stream.written && this.streamInFlight < this.window)) return true;
    return false;
  }
  packDatagrams(frames, meta, space) {
    // First fit: a later small datagram may share a packet an earlier large
    // one could not. The unreliable lane promises no order anyway.
    for (let i = 0; i < this.datagrams.length && space > 4;) {
      const item = this.datagrams[i], header = 2 + varintSize(item.bytes.length);
      if (header + item.bytes.length <= space) {
        // Frame header and payload go in as two pieces; sealing copies both.
        const head = Buffer.allocUnsafe(header);
        head[0] = FRAME.DATAGRAM; head[1] = item.text ? 1 : 0;
        writeVarint(head, 2, item.bytes.length);
        frames.push(head, item.bytes); space -= header + item.bytes.length; meta.datagrams++; this.counters.datagramsSent++;
        this.datagrams.splice(i, 1);
        continue;
      }
      // Larger than any packet: fragments, each in a packet of its own.
      if (header + item.bytes.length > MAX_PLAINTEXT - 48 && (!frames.length || (frames.length === 1 && frames[0][0] === FRAME.ACK))) {
        this.datagrams.splice(i, 1);
        this.fragmentDatagram(item, frames, meta);
        return 0;
      }
      i++;
    }
    return space;
  }
  fragmentDatagram(item, frames, meta) {
    const id = this.fragmentId++, count = Math.ceil(item.bytes.length / FRAGMENT_BYTES);
    for (let index = 0; index < count; index++) {
      const part = item.bytes.subarray(index * FRAGMENT_BYTES, (index + 1) * FRAGMENT_BYTES);
      const frame = Buffer.alloc(2 + varintSize(id) + 2 + varintSize(part.length) + part.length);
      frame[0] = FRAME.FRAGMENT; frame[1] = item.text ? 1 : 0;
      let offset = writeVarint(frame, 2, id);
      frame[offset++] = index; frame[offset++] = count;
      part.copy(frame, writeVarint(frame, offset, part.length));
      if (index === 0) { frames.push(frame); meta.datagrams++; }
      else this.emitPacket([frame], { streams: [], datagrams: 1, reliable: false });
    }
    this.counters.datagramsSent++;
  }
  packStreams(frames, meta, space) {
    let pending = 0;
    for (const stream of this.sendStreams.values()) {
      while (space > 16) {
        const piece = stream.take(space - 12, this.window - this.streamInFlight - pending);
        if (!piece) break;
        const head = Buffer.allocUnsafe(2 + varintSize(piece.offset) + varintSize(piece.length));
        head[0] = FRAME.STREAM; head[1] = stream.id;
        writeVarint(head, writeVarint(head, 2, piece.offset), piece.length);
        frames.push(head, ...piece.parts); space -= head.length + piece.length;
        meta.streams.push([stream.id, piece.offset, piece.length]); meta.reliable = true;
        pending += piece.length;
        if (piece.retransmit) this.counters.retransmittedBytes += piece.length;
      }
    }
    return space;
  }
  emitPacket(frames, meta, path = this.path) {
    if (this.nextNumber >= MAX_PACKET_NUMBER) { this.finish({ code: CLOSE_CODE.PROTOCOL, reason: 'packet numbers exhausted', remote: false }); return; }
    if (this.nextNumber && this.nextNumber % this.keyUpdateEvery === 0) { this.sendAead = this.sendAead.next(); this.sendPhase ^= 1; }
    const number = this.nextNumber++, now = this.now();
    const header = this.header ??= Buffer.allocUnsafe(DATA_HEADER);
    header[0] = this.sendPhase ? PACKET.DATA_PHASE : PACKET.DATA;
    header.writeUInt32LE(this.remoteSession, 1);
    header.writeUInt32LE(number, 5);
    // Frames (headers and uncopied payload pieces) are gathered once into
    // the shared scratch, which the seal copies out of before returning.
    let length = 0;
    for (const frame of frames) length += frame.length;
    let plaintext;
    if (frames.length === 1) plaintext = frames[0];
    else if (length <= plaintextScratch.length) {
      let offset = 0;
      for (const frame of frames) offset += frame.copy(plaintextScratch, offset);
      plaintext = plaintextScratch.subarray(0, length);
    } else plaintext = Buffer.concat(frames, length);
    const packet = this.sendAead.seal(header, number, plaintext);
    if (meta.reliable || meta.datagrams || meta.streams.length) {
      this.sentPackets.set(number, { time: now, bytes: packet.length, reliable: meta.reliable, streams: meta.streams, datagrams: meta.datagrams });
      if (meta.reliable) { this.reliableInFlight++; this.lastReliableSent = now; }
      for (const [, , length] of meta.streams) this.streamInFlight += length;
      // A peer that never acknowledges cannot grow this without bound; the
      // oldest entry counts as lost, so its stream bytes are resent.
      if (this.sentPackets.size > MAX_SENT_PACKETS) { const [oldest, entry] = this.sentPackets.entries().next().value; this.lost(oldest, entry, now); }
    }
    this.lastSentAt = now;
    this.counters.packetsSent++; this.counters.bytesSent += packet.length;
    this.transmit(packet, path);
  }
}
