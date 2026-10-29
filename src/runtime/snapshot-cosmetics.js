// Combat events split by what losing one costs. Damage and death feedback
// (combat schemas 3 and 4) are gameplay events: they travel on the reliable
// events channel and are retired when delivered. Everything else (bolt
// launches and impacts, explosions, projectile samples, presentation
// events) is cosmetic: sent once in the next snapshot's own datagram,
// never repeated, dropped farthest-first when the space runs out.
//
// Launches and impacts (schemas 1 and 2) get a compact form: quantized
// position and aim in place of the f64 payload, rebuilt into the existing
// combat record shape on decode so presentation code is unchanged. Other
// cosmetic schemas travel as their existing payload bytes.
import { decodeCombat, combatRecord } from './network-entity-combat.js';
import {
  DEFAULT_SNAPSHOT_BOUNDS, quantizePosition, dequantizePosition,
  quantizeYaw, dequantizeYaw, quantizePitch, dequantizePitch,
  SnapshotWriter as Writer, SnapshotReader as Reader,
} from './snapshot-codec.js';

export function isGameplayCombat(record) { return record.kind === 'combat' && (record.schema === 3 || record.schema === 4); }
// Packing priority for one cosmetic event as seen by one client: the
// squared distance to the event, or null when it is not relevant to that
// client at all. The client's own events sort before everything (-1): a
// tank shell's samples move away from its shooter at full speed, and
// dropping them farthest-first like any other event made his own round
// vanish mid-flight whenever the datagram's cosmetic budget overflowed.
// A bolt launch is as near as its path comes: a tank firing at the client
// from beyond the range is still seen coming.
export function cosmeticPriority(view, position, ownerId, range = 300) {
  const at = view?.position;
  if (!at || !position) return 0;
  let x = at[0], y = at[1], z = at[2];
  if (view.schema === 1 && view.direction && view.speed > 0) {
    const [ux, uy, uz] = view.direction, t = Math.max(0, Math.min(view.speed * view.life, (position.x - x) * ux + (position.y - y) * uy + (position.z - z) * uz));
    x += ux * t; y += uy * t; z += uz * t;
  }
  const dx = x - position.x, dy = y - position.y, dz = z - position.z, d2 = dx * dx + dy * dy + dz * dz;
  if (view.ownerId === ownerId) return -1;
  return d2 <= range * range ? d2 : null;
}
const TAG_RAW = 0, TAG_LAUNCH = 1, TAG_IMPACT = 2;
const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
function directionAngles([x, y, z]) { return [Math.atan2(x, z), Math.asin(clamp(y, -1, 1))]; }
function anglesDirection(yaw, pitch) { const c = Math.cos(pitch); return [Math.sin(yaw) * c, Math.sin(pitch), Math.cos(yaw) * c]; }
// The compact form holds a position only inside the snapshot bounds (the
// default's height runs from -400 to 300 m) and a drawn offset within 128 m.
// Anything else travels as its exact payload, which every decoder reads:
// the recon droid's orbital strike leaves 515 m up, and clamped to 300 its
// bolts were drawn on a path 215 m too low, under the ground and out of view.
const within = (n, min, extent) => n >= min && n <= min + extent;
function compact(e, b) {
  const [x, y, z] = e.position;
  return within(x, b.xMin, b.xExtent) && within(y, b.yMin, b.yExtent) && within(z, b.zMin, b.zExtent)
    && (e.schema !== 1 || e.offset.every((n) => Math.abs(n) * 256 <= 32767));
}

export function encodeCosmeticEvent(record, bounds = DEFAULT_SNAPSHOT_BOUNDS) {
  const w = new Writer();
  w.u32(record.id); w.u32(record.generation); w.u32(record.tick); w.u16(record.schema);
  const e = record.schema === 1 || record.schema === 2 ? decodeCombat(record) : null;
  if (e && compact(e, bounds)) {
    w.u8(record.schema === 1 ? TAG_LAUNCH : TAG_IMPACT);
    w.u32(e.expiryTick); w.u32(e.definitionId); w.u32(e.ownerId); w.u32(e.ownerGeneration); w.u32(e.fireId);
    w.u32(Math.round(e.launchTick * 64)); w.u32(e.executionTick); w.u8(e.channel); w.u8(e.surface);
    for (const code of quantizePosition(e.position, bounds)) w.u16(code);
    const [yaw, pitch] = directionAngles(e.direction);
    w.u16(quantizeYaw(yaw)); w.i16(quantizePitch(pitch));
    if (record.schema === 1) {
      for (const x of e.offset) w.i16(clamp(Math.round(x * 256), -32767, 32767));
      w.f32(e.speed); w.f32(e.life); w.f32(e.merge);
    }
  } else {
    w.u8(TAG_RAW); w.u16(record.payload.length); w.raw(record.payload);
  }
  return w.finish();
}
export function decodeCosmeticEvent(bytes, bounds = DEFAULT_SNAPSHOT_BOUNDS) {
  const r = new Reader(bytes);
  const base = { kind: 'combat', id: r.u32(), generation: r.u32(), tick: r.u32(), schema: r.u16() };
  const tag = r.u8();
  if (tag === TAG_RAW) {
    base.payload = r.raw(r.u16());
    if (r.at !== bytes.length) throw Error('cosmetic event trailing bytes');
    return base;
  }
  if (tag !== TAG_LAUNCH && tag !== TAG_IMPACT) throw Error('cosmetic event tag');
  const e = { expiryTick: r.u32(), definitionId: r.u32(), ownerId: r.u32(), ownerGeneration: r.u32(), fireId: r.u32(),
    launchTick: r.u32() / 64, executionTick: r.u32(), channel: r.u8(), surface: r.u8() };
  e.position = dequantizePosition([r.u16(), r.u16(), r.u16()], bounds);
  e.direction = anglesDirection(dequantizeYaw(r.u16()), dequantizePitch(r.i16()));
  if (tag === TAG_LAUNCH) { e.offset = [r.i16() / 256, r.i16() / 256, r.i16() / 256]; e.speed = r.f32(); e.life = r.f32(); e.merge = r.f32(); }
  if (r.at !== bytes.length) throw Error('cosmetic event trailing bytes');
  return combatRecord({ ...base, ...e, offset: e.offset ?? [0, 0, 0] });
}
// Pack pre-sorted cosmetic records (nearest first; the tail is what drops)
// into at most budgetBytes. Returns the packed bytes and the split.
export function packCosmeticEvents(records, budgetBytes, bounds = DEFAULT_SNAPSHOT_BOUNDS) {
  return packEncodedCosmeticEvents(records.map(record => ({ record, bytes: encodeCosmeticEvent(record, bounds) })), budgetBytes);
}
// The same packing over events encoded once for every client.
export function packEncodedCosmeticEvents(encoded, budgetBytes) {
  const w = new Writer(), packed = [], dropped = [];
  for (const { record, bytes } of encoded) {
    if (w.length + 2 + bytes.length <= budgetBytes) { w.u16(bytes.length); w.raw(bytes); packed.push(record); }
    else dropped.push(record);
  }
  return { bytes: w.finish(), packed, dropped };
}
export function unpackCosmeticEvents(bytes, bounds = DEFAULT_SNAPSHOT_BOUNDS) {
  if (!bytes) return [];
  const r = new Reader(bytes), records = [];
  while (r.at < bytes.length) records.push(decodeCosmeticEvent(r.raw(r.u16()), bounds));
  return records;
}
