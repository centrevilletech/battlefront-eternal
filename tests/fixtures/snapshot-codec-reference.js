// Frozen reference: src/runtime/snapshot-codec.js as it was before the
// scratch-buffer encoder replaced it (engine fe8fe64). The encoder must stay
// byte-identical to this one; tests/snapshot-codec-identity.test.js compares
// them. Never edit this copy to follow a codec change: a deliberate wire
// change replaces the identity test instead.
// Snapshot-delta wire codec (docs/snapshot-delta-design.md). Pure module:
// quantized snapshot entries in, independent datagram parts out. A snapshot
// is {tick, epoch, slots: Map<slot, entry>, world: Map<key, field>} where an
// entry holds only quantized integers (and f32 animation values for the
// droideka), so a client that applies a part against the same baseline the
// server encoded against reconstructs the entry bit for bit.
//
// Position "small delta" is an i8 difference of the u16 position codes, not
// a 2 cm float step: code differences reconstruct the exact code the server
// captured, which the bit-for-bit baseline rule requires. The byte cost is
// the same as the design's 2 cm form.
export const SNAPSHOT_ENTITIES_MAGIC = 0x31534e42; // 'BNS1'
export const SNAPSHOT_OWN_MAGIC = 0x314f4e42; // 'BNO1'
export const SNAPSHOT_HEADER_BYTES = 20;
export const SNAPSHOT_MAX_DATAGRAM = 1100;
export const SNAPSHOT_SLOT_COUNT = 512;
export const SNAPSHOT_CLASSES = Object.freeze(['soldier', 'droideka', 'vehicle', 'turret', 'item', 'remote']);
const CLS_CODE = Object.fromEntries(SNAPSHOT_CLASSES.map((name, i) => [name, i]));
// BF1 terrain is 2048 m across, centred on the origin. y spans the terrain
// minimum (below sea-level maps sit near -50 m) up to fly height + margin.
export const DEFAULT_SNAPSHOT_BOUNDS = Object.freeze({ xMin: -1024, xExtent: 2048, zMin: -1024, zExtent: 2048, yMin: -400, yExtent: 700 });
const OP_UPDATE = 0, OP_NEW = 1, OP_REMOVED = 2;
const ANGLE = 65536 / (2 * Math.PI);

const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
const int = (v, lo, hi, what) => { if (!Number.isSafeInteger(v) || v < lo || v > hi) throw Error(`snapshot ${what} out of range`); return v; };
export function quantizePosition([x, y, z], b = DEFAULT_SNAPSHOT_BOUNDS) {
  return [clamp(Math.round((x - b.xMin) / b.xExtent * 65535), 0, 65535),
          clamp(Math.round((y - b.yMin) / b.yExtent * 65535), 0, 65535),
          clamp(Math.round((z - b.zMin) / b.zExtent * 65535), 0, 65535)];
}
export function dequantizePosition([cx, cy, cz], b = DEFAULT_SNAPSHOT_BOUNDS) {
  return [b.xMin + cx / 65535 * b.xExtent, b.yMin + cy / 65535 * b.yExtent, b.zMin + cz / 65535 * b.zExtent];
}
export function quantizeVelocity(v) {
  if (v.every(x => Number.isFinite(x) && Math.abs(x) <= 31.75)) return { wide: false, v: v.map(x => clamp(Math.round(x / 0.25), -127, 127)) };
  return { wide: true, v: v.map(x => clamp(Math.round(x * 32), -32767, 32767)) };
}
export function dequantizeVelocity({ wide, v }) { return wide ? v.map(x => x / 32) : v.map(x => x * 0.25); }
export function quantizeYaw(yaw) { const wrapped = ((yaw % (2 * Math.PI)) + 2 * Math.PI) % (2 * Math.PI); return Math.round(wrapped * ANGLE) & 0xffff; }
export function dequantizeYaw(code) { return code / ANGLE; }
export function quantizePitch(pitch) { return clamp(Math.round(pitch * ANGLE), -32768, 32767); }
export function dequantizePitch(code) { return code / ANGLE; }
// Smallest-three quaternion: index of the dropped (largest) component plus
// the other three in 15 bits each over [-1/sqrt(2), 1/sqrt(2)].
const QUAT_RANGE = Math.SQRT1_2;
export function quantizeQuaternion(q) {
  let index = 0; for (let i = 1; i < 4; i++) if (Math.abs(q[i]) > Math.abs(q[index])) index = i;
  const sign = q[index] < 0 ? -1 : 1, v = [];
  for (let i = 0; i < 4; i++) if (i !== index) v.push(clamp(Math.round((q[i] * sign / QUAT_RANGE * 0.5 + 0.5) * 32767), 0, 32767));
  return { index, v };
}
export function dequantizeQuaternion({ index, v }) {
  const out = [0, 0, 0, 0]; let sum = 0, n = 0;
  for (let i = 0; i < 4; i++) if (i !== index) { out[i] = (v[n++] / 32767 - 0.5) * 2 * QUAT_RANGE; sum += out[i] * out[i]; }
  out[index] = Math.sqrt(Math.max(0, 1 - sum));
  const length = Math.hypot(...out) || 1; return out.map(x => x / length);
}

class Writer {
  constructor() { this.buffer = new Uint8Array(256); this.view = new DataView(this.buffer.buffer); this.length = 0; }
  grow(extra) {
    if (this.length + extra <= this.buffer.length) return;
    const next = new Uint8Array(Math.max(this.buffer.length * 2, this.length + extra));
    next.set(this.buffer.subarray(0, this.length)); this.buffer = next; this.view = new DataView(next.buffer);
  }
  get bytes() { return this.buffer.subarray(0, this.length); }
  u8(v) { this.grow(1); this.buffer[this.length++] = int(v, 0, 255, 'u8'); }
  i8(v) { this.grow(1); this.buffer[this.length++] = int(v, -128, 127, 'i8') & 0xff; }
  u16(v) { int(v, 0, 65535, 'u16'); this.grow(2); this.buffer[this.length++] = v & 0xff; this.buffer[this.length++] = (v >> 8) & 0xff; }
  i16(v) { int(v, -32768, 32767, 'i16'); this.grow(2); this.buffer[this.length++] = v & 0xff; this.buffer[this.length++] = (v >> 8) & 0xff; }
  u32(v) { int(v, 0, 0xffffffff, 'u32'); this.grow(4); this.buffer[this.length++] = v & 0xff; this.buffer[this.length++] = (v >>> 8) & 0xff; this.buffer[this.length++] = (v >>> 16) & 0xff; this.buffer[this.length++] = (v >>> 24) & 0xff; }
  f32(v) { if (!Number.isFinite(v)) throw Error('snapshot nonfinite f32'); this.grow(4); this.view.setFloat32(this.length, v, true); this.length += 4; }
  varint(v) { int(v, 0, 0xffffffff, 'varint'); this.grow(5); do { let byte = v % 128; v = Math.floor(v / 128); if (v) byte |= 0x80; this.buffer[this.length++] = byte; } while (v); }
  raw(bytes) { this.grow(bytes.length); this.buffer.set(bytes, this.length); this.length += bytes.length; }
  finish() { return this.buffer.slice(0, this.length); }
  reset() { this.length = 0; return this; }
}
class Reader {
  constructor(bytes) { this.bytes = bytes; this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength); this.at = 0; }
  need(n) { if (this.at + n > this.bytes.length) throw Error('truncated snapshot datagram'); }
  u8() { this.need(1); return this.view.getUint8(this.at++); }
  i8() { this.need(1); return this.view.getInt8(this.at++); }
  u16() { this.need(2); const v = this.view.getUint16(this.at, true); this.at += 2; return v; }
  i16() { this.need(2); const v = this.view.getInt16(this.at, true); this.at += 2; return v; }
  u32() { this.need(4); const v = this.view.getUint32(this.at, true); this.at += 4; return v; }
  f32() { this.need(4); const v = this.view.getFloat32(this.at, true); this.at += 4; if (!Number.isFinite(v)) throw Error('snapshot nonfinite f32'); return v; }
  varint() { let v = 0, shift = 0; for (;;) { const byte = this.u8(); v += (byte & 0x7f) * 2 ** shift; if (!(byte & 0x80)) return v; shift += 7; if (shift > 28) throw Error('snapshot varint too long'); } }
  raw(n) { this.need(n); const out = this.bytes.slice(this.at, this.at + n); this.at += n; return out; }
}
function writeHeader(w, { magic, epoch, tick, interval, baselineTick, partIndex, partCount, ackInput }) {
  w.u32(magic); w.u16(epoch); w.u32(tick); w.u8(interval); w.u32(baselineTick);
  w.u8(((partIndex & 0xf) << 4) | (partCount & 0xf)); w.u32(ackInput);
}
export function readSnapshotHeader(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.length < SNAPSHOT_HEADER_BYTES || bytes.length > SNAPSHOT_MAX_DATAGRAM) throw Error('invalid snapshot datagram');
  const r = new Reader(bytes), magic = r.u32();
  if (magic !== SNAPSHOT_ENTITIES_MAGIC && magic !== SNAPSHOT_OWN_MAGIC) throw Error('snapshot magic mismatch');
  const header = { magic, epoch: r.u16(), tick: r.u32(), interval: r.u8(), baselineTick: r.u32() };
  const part = r.u8(); header.partIndex = part >> 4; header.partCount = part & 0xf; header.ackInput = r.u32();
  return header;
}

// Field groups per class, in wire order. Each group knows how to write
// itself (against the baseline entry, or null for a `new` record), read
// itself back onto a working entry, and whether two entries agree on it.
const posGroup = {
  write(w, e, base) {
    const small = base && e.pos.every((code, i) => Math.abs(code - base.pos[i]) <= 127);
    w.u8((small ? 0 : 1) | (e.disc ? 0x80 : 0));
    if (small) for (let i = 0; i < 3; i++) w.i8(e.pos[i] - base.pos[i]);
    else for (const code of e.pos) w.u16(code);
  },
  read(r, e, base) {
    const mode = r.u8(); e.disc = !!(mode & 0x80);
    if (mode & 1) e.pos = [r.u16(), r.u16(), r.u16()];
    else { if (!base) throw Error('snapshot small delta without baseline'); e.pos = base.pos.map(code => code + r.i8()); }
  },
  equal: (a, b) => !a.disc && (a.pos === b.pos || a.pos.every((code, i) => code === b.pos[i])),
};
const velGroup = {
  write(w, e) { w.u8(e.vel.wide ? 1 : 0); for (const x of e.vel.v) e.vel.wide ? w.i16(x) : w.i8(x); },
  read(r, e) { const wide = r.u8() !== 0; e.vel = { wide, v: [0, 1, 2].map(() => wide ? r.i16() : r.i8()) }; },
  equal: (a, b) => a.vel === b.vel || (a.vel.wide === b.vel.wide && a.vel.v.every((x, i) => x === b.vel.v[i])),
};
const aimGroup = {
  write(w, e) { w.u16(e.yaw); w.i16(e.pitch); },
  read(r, e) { e.yaw = r.u16(); e.pitch = r.i16(); },
  equal: (a, b) => a.yaw === b.yaw && a.pitch === b.pitch,
};
const fireGroup = {
  write(w, e) { w.u8(e.fireCounter & 0xff); },
  read(r, e) { e.fireCounter = r.u8(); },
  equal: (a, b) => (a.fireCounter & 0xff) === (b.fireCounter & 0xff),
};
const soldierVitals = {
  write(w, e) { w.u16(e.health); },
  read(r, e) { e.health = r.u16(); },
  equal: (a, b) => a.health === b.health,
};
const soldierStatus = {
  write(w, e) { w.u8(e.posture | (e.charge << 2)); w.u8(e.state); w.u16(e.weapon); w.u16(e.seatSlot < 0 ? 0xffff : e.seatSlot); w.u8(e.seatIndex); },
  read(r, e) { const p = r.u8(); e.posture = p & 3; e.charge = p >> 2; e.state = r.u8(); e.weapon = r.u16(); const seat = r.u16(); e.seatSlot = seat === 0xffff ? -1 : seat; e.seatIndex = r.u8(); },
  equal: (a, b) => a.posture === b.posture && a.charge === b.charge && a.state === b.state && a.weapon === b.weapon && a.seatSlot === b.seatSlot && a.seatIndex === b.seatIndex,
};
const droidekaVitalsStatus = {
  write(w, e) { w.u16(e.health); w.u8(e.posture); w.u8(e.state); w.u16(e.weapon); w.u16(e.seatSlot < 0 ? 0xffff : e.seatSlot); w.u8(e.seatIndex); },
  read(r, e) { e.health = r.u16(); e.posture = r.u8(); e.state = r.u8(); e.weapon = r.u16(); const seat = r.u16(); e.seatSlot = seat === 0xffff ? -1 : seat; e.seatIndex = r.u8(); },
  equal: (a, b) => a.health === b.health && a.posture === b.posture && a.state === b.state && a.weapon === b.weapon && a.seatSlot === b.seatSlot && a.seatIndex === b.seatIndex,
};
const droidekaGroup = {
  write(w, e) { w.u8(e.mode); w.u8((e.reverse ? 1 : 0) | (e.dead ? 2 : 0) | (e.shieldOn ? 4 : 0)); for (const x of e.anim) w.f32(x); },
  read(r, e) { e.mode = r.u8(); const f = r.u8(); e.reverse = !!(f & 1); e.dead = !!(f & 2); e.shieldOn = !!(f & 4); e.anim = [r.f32(), r.f32(), r.f32(), r.f32(), r.f32()]; },
  equal: (a, b) => a.mode === b.mode && a.reverse === b.reverse && a.dead === b.dead && a.shieldOn === b.shieldOn && (a.anim === b.anim || a.anim.every((x, i) => Object.is(Math.fround(x), Math.fround(b.anim[i])))),
};
const orientationGroup = {
  write(w, e) { let n = e.quat.index + e.quat.v[0] * 4 + e.quat.v[1] * 2 ** 17 + e.quat.v[2] * 2 ** 32; for (let i = 0; i < 6; i++) { w.u8(n % 256); n = Math.floor(n / 256); } },
  read(r, e) { let n = 0; for (let i = 5; i >= 0; i--) n = n * 256 + r.bytes[r.at + i]; r.at += 6; e.quat = { index: n % 4, v: [Math.floor(n / 4) % 32768, Math.floor(n / 2 ** 17) % 32768, Math.floor(n / 2 ** 32) % 32768] }; },
  equal: (a, b) => a.quat === b.quat || (a.quat.index === b.quat.index && a.quat.v.every((x, i) => x === b.quat.v[i])),
};
const vehicleVitals = {
  write(w, e) { w.u16(e.health); w.u8(e.state); w.u16(e.power); },
  read(r, e) { e.health = r.u16(); e.state = r.u8(); e.power = r.u16(); },
  equal: (a, b) => a.health === b.health && a.state === b.state && a.power === b.power,
};
const mountGroup = {
  write(w, e) { const m = e.mount; w.u8(m.slot ?? 255); w.i16(m.tilt); w.i16(m.roll); w.u16(m.walkPhase); w.u8(m.walkBlend); w.u8(m.seats.length); for (const [yaw, pitch] of m.seats) { w.i16(yaw); w.i16(pitch); } },
  read(r, e) { const slot = r.u8(), m = { slot: slot === 255 ? null : slot, tilt: r.i16(), roll: r.i16(), walkPhase: r.u16(), walkBlend: r.u8(), seats: [] }; const count = r.u8(); if (count > 16) throw Error('snapshot mount seats'); for (let i = 0; i < count; i++) m.seats.push([r.i16(), r.i16()]); e.mount = m; },
  equal: (a, b) => a.mount === b.mount || (a.mount.slot === b.mount.slot && a.mount.tilt === b.mount.tilt && a.mount.roll === b.mount.roll && a.mount.walkPhase === b.mount.walkPhase && a.mount.walkBlend === b.mount.walkBlend && a.mount.seats.length === b.mount.seats.length && a.mount.seats.every((seat, i) => seat[0] === b.mount.seats[i][0] && seat[1] === b.mount.seats[i][1])),
};
const turretAim = {
  write(w, e) { for (const k of ['yaw', 'pitch', 'gunYaw', 'gunPitch']) w.i16(e[k]); },
  read(r, e) { for (const k of ['yaw', 'pitch', 'gunYaw', 'gunPitch']) e[k] = r.i16(); },
  equal: (a, b) => ['yaw', 'pitch', 'gunYaw', 'gunPitch'].every(k => a[k] === b[k]),
};
const turretVitals = {
  write(w, e) { w.u16(e.health); w.u8(e.state); w.u8(e.team); },
  read(r, e) { e.health = r.u16(); e.state = r.u8(); e.team = r.u8(); },
  equal: (a, b) => a.health === b.health && a.state === b.state && a.team === b.team,
};
const yawOnlyGroup = {
  write(w, e) { w.u16(e.yaw); },
  read(r, e) { e.yaw = r.u16(); },
  equal: (a, b) => a.yaw === b.yaw,
};
const itemVitals = {
  write(w, e) { w.u8(e.active ? 1 : 0); if (e.active) { w.u16(e.definitionId); for (const x of e.up) w.i16(x); } },
  read(r, e) { e.active = r.u8() !== 0; if (e.active) { e.definitionId = r.u16(); e.up = [r.i16(), r.i16(), r.i16()]; } else { e.definitionId = 0; e.up = [0, 32767, 0]; } },
  equal: (a, b) => a.active === b.active && (!a.active || (a.definitionId === b.definitionId && (a.up === b.up || a.up.every((x, i) => x === b.up[i])))),
};
const remoteVitals = {
  write(w, e) { w.u8(e.active ? 1 : 0); if (e.active) { w.u16(e.definitionId); w.u8(e.team); w.u16(e.health); } },
  read(r, e) { e.active = r.u8() !== 0; if (e.active) { e.definitionId = r.u16(); e.team = r.u8(); e.health = r.u16(); } else { e.definitionId = 0; e.health = 0; } },
  equal: (a, b) => a.active === b.active && (!a.active || (a.definitionId === b.definitionId && a.team === b.team && a.health === b.health)),
};
export const SNAPSHOT_GROUPS = Object.freeze({
  soldier: [posGroup, velGroup, aimGroup, soldierVitals, soldierStatus, fireGroup],
  droideka: [posGroup, velGroup, aimGroup, droidekaVitalsStatus, droidekaGroup, fireGroup],
  vehicle: [posGroup, velGroup, orientationGroup, vehicleVitals, mountGroup, fireGroup],
  turret: [turretAim, turretVitals],
  item: [posGroup, velGroup, yawOnlyGroup, itemVitals],
  remote: [posGroup, velGroup, aimGroup, remoteVitals],
});
export function emptySnapshot(tick = 0, epoch = 0) { return { tick, epoch, slots: new Map(), world: new Map() }; }
// Shallow: group readers always replace nested values, never mutate them.
export function cloneEntry(entry) { return entry ? { ...entry } : entry; }
export function cloneSnapshot(snapshot) {
  if (!snapshot) return emptySnapshot();
  return { tick: snapshot.tick, epoch: snapshot.epoch, slots: new Map([...snapshot.slots].map(([k, v]) => [k, cloneEntry(v)])), world: new Map([...snapshot.world].map(([k, v]) => [k, { ...v, payload: v.payload?.slice() }])) };
}

const scratchWriters = [new Writer(), new Writer()];
// One encoded record (without its slot prefix, which depends on the part).
function encodeRecord(current, baseline) {
  // Identity fast path: capture reuses the previous entry object when every
  // quantized value is unchanged, so the same object on both sides encodes
  // to nothing. A disc entry is exempt: its pos group must still be written
  // to carry the snap, exactly as the group comparison below would decide.
  if (current === baseline && !current.disc) return null;
  const w = scratchWriters[0].reset(), groups = SNAPSHOT_GROUPS[current.cls];
  const isNew = !baseline || baseline.cls !== current.cls || baseline.generation !== current.generation ||
    baseline.classId !== current.classId || baseline.entityId !== current.entityId ||
    // Team rides in the `new` header (the turret's vitals group carries it).
    (current.cls !== 'turret' && baseline.team !== current.team);
  if (isNew) {
    w.u8(OP_NEW | (((1 << groups.length) - 1) << 2));
    w.u8(CLS_CODE[current.cls]); w.u32(current.generation); w.u32(current.entityId); w.u16(current.classId); w.u8(current.team);
    for (const group of groups) group.write(w, current, null);
  } else {
    let mask = 0; const body = scratchWriters[1].reset();
    groups.forEach((group, i) => { if (!group.equal(current, baseline)) { mask |= 1 << i; group.write(body, current, baseline); } });
    if (!mask) return null;
    w.u8(OP_UPDATE | (mask << 2)); w.raw(body.finish());
  }
  return w.finish();
}
function encodeRemoved() { const w = new Writer(); w.u8(OP_REMOVED); return w.finish(); }
// Word-granularity payload delta: changed 8-byte words travel verbatim,
// everything else is copied from the baseline payload, so every encoded
// value (the own correction's f64s above all) arrives exact. Shared by the
// own-correction delta and world fields, whose timer-carrying payloads
// would otherwise resend whole on every refresh inside a baseline window.
export const WORD_DELTA_ABSOLUTE = 0, WORD_DELTA_CHANGED = 1;
export function encodeWordDelta(current, baseline) {
  const w = new Writer();
  if (!baseline || baseline.length !== current.length) {
    w.u8(WORD_DELTA_ABSOLUTE); w.u16(current.length); w.raw(current); return w.finish();
  }
  const words = Math.ceil(current.length / 8), changed = [];
  for (let i = 0; i < words; i++) {
    let differs = false;
    for (let b = i * 8; b < Math.min(current.length, i * 8 + 8); b++) if (current[b] !== baseline[b]) { differs = true; break; }
    if (differs) changed.push(i);
  }
  if (1 + 2 + 2 + Math.ceil(words / 8) + changed.length * 8 >= current.length + 3) { w.u8(WORD_DELTA_ABSOLUTE); w.u16(current.length); w.raw(current); return w.finish(); }
  w.u8(WORD_DELTA_CHANGED); w.u16(current.length); w.u16(words);
  const bitmap = new Uint8Array(Math.ceil(words / 8));
  for (const i of changed) bitmap[i >> 3] |= 1 << (i & 7);
  w.raw(bitmap);
  for (const i of changed) { const word = new Uint8Array(8); word.set(current.subarray(i * 8, Math.min(current.length, i * 8 + 8))); w.raw(word); }
  return w.finish();
}
export function decodeWordDelta(bytes, baseline) {
  const r = new Reader(bytes), mode = r.u8();
  if (mode === WORD_DELTA_ABSOLUTE) { const out = r.raw(r.u16()); if (r.at !== bytes.length) throw Error('word delta trailing bytes'); return out; }
  if (mode !== WORD_DELTA_CHANGED) throw Error('word delta mode');
  if (!baseline) throw Error('word delta baseline not held');
  const length = r.u16(), words = r.u16();
  if (baseline.length !== length || words !== Math.ceil(length / 8)) throw Error('word delta shape');
  const bitmap = r.raw(Math.ceil(words / 8)), out = baseline.slice();
  for (let i = 0; i < words; i++) if (bitmap[i >> 3] & (1 << (i & 7))) out.set(r.raw(8).subarray(0, Math.min(8, length - i * 8)), i * 8);
  if (r.at !== bytes.length) throw Error('word delta trailing bytes');
  return out;
}
// The world section: typed fields (the existing summary payloads), sent
// when their bytes differ from the baseline's, removed when they vanish.
export function worldFieldKey(field) { return `${field.schema}:${field.id}`; }
function worldFieldBytes(field, removed, baselineField = null) {
  const w = new Writer();
  w.u32(field.id); w.u16(field.schema); w.u32(field.generation);
  if (removed) { w.u8(1); return w.finish(); }
  if (baselineField && baselineField.generation === field.generation) {
    const delta = encodeWordDelta(field.payload, baselineField.payload);
    if (delta[0] === WORD_DELTA_CHANGED) { w.u8(2); w.u16(delta.length); w.raw(delta); return w.finish(); }
  }
  w.u8(0); w.u16(field.payload.length); w.raw(field.payload);
  return w.finish();
}
// The changed world fields, each encoded on its own so the part packer can
// distribute them across datagrams (a whole world delta need not fit one).
function encodeWorldEntries(current, baseline) {
  const entries = [];
  for (const [key, field] of current.world) {
    const base = baseline?.world.get(key);
    if (base === field) continue;
    if (base && base.generation === field.generation && (base.payload === field.payload || (base.payload.length === field.payload.length && base.payload.every((b, i) => b === field.payload[i])))) continue;
    entries.push(worldFieldBytes(field, false, base));
  }
  if (baseline) for (const field of baseline.world.values()) if (!current.world.has(worldFieldKey(field))) entries.push(worldFieldBytes(field, true));
  return entries;
}
function encodeWorld(current, baseline) {
  const w = new Writer(), entries = encodeWorldEntries(current, baseline);
  w.varint(entries.length);
  for (const entry of entries) w.raw(entry);
  return w.finish();
}
function decodeWorld(r, baselineWorld = null) {
  const count = r.varint(); if (count > 4096) throw Error('snapshot world field count');
  const fields = [];
  for (let i = 0; i < count; i++) {
    const field = { id: r.u32(), schema: r.u16(), generation: r.u32() }, mode = r.u8();
    if (mode > 2) throw Error('snapshot world field flag');
    field.removed = mode === 1;
    if (mode === 0) field.payload = r.raw(r.u16());
    else if (mode === 2) field.payload = decodeWordDelta(r.raw(r.u16()), baselineWorld?.get(worldFieldKey(field))?.payload ?? null);
    fields.push(field);
  }
  return fields;
}
// Encode a snapshot against a baseline (null = empty baseline) into one or
// more independent `entities` datagrams of at most 1100 bytes. A vehicle,
// its riders' records and its mount group always share a part; world fields
// pack into the parts alongside the records. At least one datagram.
export function encodeEntities(current, baseline, { interval = 2 } = {}) {
  const records = new Map(); // slot -> bytes
  for (const [slot, entry] of current.slots) { const bytes = encodeRecord(entry, baseline?.slots.get(slot)); if (bytes) records.set(slot, bytes); }
  if (baseline) for (const slot of baseline.slots.keys()) if (!current.slots.has(slot)) records.set(slot, encodeRemoved());
  const worldEntries = encodeWorldEntries(current, baseline);
  // Packing units: a vehicle plus the soldiers seated in it.
  const riderOf = new Map();
  for (const [slot, entry] of current.slots) if (entry.seatSlot >= 0) riderOf.set(slot, entry.seatSlot);
  const units = []; const claimed = new Set();
  const ordered = [...records.keys()].sort((a, b) => a - b);
  for (const slot of ordered) {
    if (claimed.has(slot)) continue;
    const unit = [slot]; claimed.add(slot);
    const entry = current.slots.get(slot);
    if (entry?.cls === 'vehicle') for (const other of ordered) if (!claimed.has(other) && riderOf.get(other) === slot) { unit.push(other); claimed.add(other); }
    if (riderOf.has(slot) && records.has(riderOf.get(slot)) && !claimed.has(riderOf.get(slot))) { unit.push(riderOf.get(slot)); claimed.add(riderOf.get(slot)); }
    units.push(unit.sort((a, b) => a - b));
  }
  // Part overhead: header + world length + world count + record count.
  const OVERHEAD = SNAPSHOT_HEADER_BYTES + 2 + 1 + 1;
  const parts = [{ slots: [], world: [], bytes: OVERHEAD }];
  const place = size => {
    let part = parts.at(-1);
    if ((part.slots.length || part.world.length) && part.bytes + size > SNAPSHOT_MAX_DATAGRAM) { part = { slots: [], world: [], bytes: OVERHEAD }; parts.push(part); }
    return part;
  };
  for (const unit of units) {
    const size = unit.reduce((n, slot) => n + records.get(slot).length + 2, 0);
    const part = place(size); part.slots.push(unit); part.bytes += size;
  }
  for (const entry of worldEntries) {
    const part = place(entry.length); part.world.push(entry); part.bytes += entry.length;
  }
  if (parts.length > 15) throw Error('snapshot needs too many parts');
  return parts.map((part, partIndex) => {
    const w = new Writer();
    writeHeader(w, { magic: SNAPSHOT_ENTITIES_MAGIC, epoch: current.epoch, tick: current.tick, interval, baselineTick: baseline?.tick ?? 0, partIndex, partCount: parts.length, ackInput: 0 });
    const section = new Writer(); section.varint(part.world.length); for (const entry of part.world) section.raw(entry);
    const sectionBytes = section.finish();
    w.u16(sectionBytes.length); w.raw(sectionBytes);
    const flat = part.slots.flat().sort((a, b) => a - b);
    w.varint(flat.length);
    let previous = 0;
    flat.forEach((slot, i) => { if (i === 0) w.u16(slot); else w.varint(slot - previous); previous = slot; w.raw(records.get(slot)); });
    const bytes = w.finish();
    if (bytes.length > SNAPSHOT_MAX_DATAGRAM && (flat.length > 1 || part.world.length > 1)) throw Error('snapshot part overflow');
    return bytes;
  });
}
// Decode one `entities` part against the baseline snapshot it names (or an
// empty snapshot for baselineTick 0). Returns the header, the full entries
// to set, the slots to remove, and the world fields. Throws when the
// baseline does not match what the datagram names.
export function decodeEntities(bytes, baseline) {
  const header = readSnapshotHeader(bytes);
  if (header.magic !== SNAPSHOT_ENTITIES_MAGIC) throw Error('not an entities datagram');
  if (header.baselineTick === 0) baseline = emptySnapshot(0, header.epoch);
  if (!baseline || baseline.tick !== header.baselineTick || baseline.epoch !== header.epoch) throw Error('snapshot baseline not held');
  const r = new Reader(bytes); r.at = SNAPSHOT_HEADER_BYTES;
  const worldLength = r.u16(), worldAt = r.at;
  const world = decodeWorld(r, baseline.world);
  if (r.at - worldAt !== worldLength) throw Error('snapshot world section size');
  const count = r.varint(); if (count > SNAPSHOT_SLOT_COUNT) throw Error('snapshot record count');
  const set = new Map(), removed = [];
  let slot = -1;
  for (let i = 0; i < count; i++) {
    slot = i === 0 ? r.u16() : slot + r.varint();
    if (slot >= SNAPSHOT_SLOT_COUNT) throw Error('snapshot slot out of range');
    const opByte = r.u8(), op = opByte & 3, mask = opByte >> 2, base = baseline.slots.get(slot);
    if (op === OP_REMOVED) { removed.push(slot); continue; }
    if (op === OP_NEW) {
      const cls = SNAPSHOT_CLASSES[r.u8()]; if (!cls) throw Error('snapshot class');
      const entry = { cls, generation: r.u32(), entityId: r.u32(), classId: r.u16(), team: r.u8(), disc: true };
      const groups = SNAPSHOT_GROUPS[cls];
      if (mask !== (1 << groups.length) - 1) throw Error('snapshot new record mask');
      for (const group of groups) group.read(r, entry, null);
      set.set(slot, entry);
    } else if (op === OP_UPDATE) {
      if (!base) throw Error('snapshot update without baseline entry');
      const entry = cloneEntry(base);
      SNAPSHOT_GROUPS[entry.cls].forEach((group, gi) => { if (mask & (1 << gi)) group.read(r, entry, base); });
      set.set(slot, entry);
    } else throw Error('snapshot op');
  }
  if (r.at !== bytes.length) throw Error('snapshot trailing bytes');
  return { header, set, removed, world };
}
// Apply a decoded part to a working snapshot (mutates and returns it).
export function applyEntities(snapshot, decoded) {
  for (const slot of decoded.removed) snapshot.slots.delete(slot);
  for (const [slot, entry] of decoded.set) snapshot.slots.set(slot, entry);
  for (const field of decoded.world) {
    if (field.removed) snapshot.world.delete(worldFieldKey(field));
    else snapshot.world.set(worldFieldKey(field), { id: field.id, schema: field.schema, generation: field.generation, payload: field.payload });
  }
  snapshot.tick = decoded.header.tick; snapshot.epoch = decoded.header.epoch;
  return snapshot;
}
export { Writer as SnapshotWriter, Reader as SnapshotReader, writeHeader as writeSnapshotHeader };
// The world-field section on its own, for the per-client summaries that
// ride in the `own` datagram with the same delta rule.
export function encodeWorldSection(currentWorld, baselineWorld) {
  return encodeWorld({ world: currentWorld }, baselineWorld ? { world: baselineWorld } : null);
}
export function decodeWorldSection(bytes, baselineWorld = null) {
  const r = new Reader(bytes), fields = decodeWorld(r, baselineWorld);
  if (r.at !== bytes.length) throw Error('snapshot world section trailing bytes');
  return fields;
}
