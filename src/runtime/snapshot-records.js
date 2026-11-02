// Conversion between the capture layer's semantic records (the shapes
// network-entity-capture.js produces and network-entity-game.js consumes)
// and quantized snapshot entries. The slot table is the net identity; each
// entry also carries the server unit id and u32 generation its `new`
// record announces, because combat, seats and own corrections name bodies
// by those, not by slot.
import {
  SNAPSHOT_SLOT_COUNT, DEFAULT_SNAPSHOT_BOUNDS, emptySnapshot, worldFieldKey,
  dequantizePosition, dequantizeVelocity,
  quantizeYaw, dequantizeYaw, quantizePitch, dequantizePitch,
  quantizeQuaternion, dequantizeQuaternion,
} from './snapshot-codec.js';
import { TURRET_STATES } from './network-entity-codec.js';

const angleCode = value => Math.max(-32767, Math.min(32767, Math.round(value * 32767 / Math.PI)));
const angleValue = code => code * Math.PI / 32767;
export class SnapshotSlotTable {
  constructor() { this.byKey = new Map(); this.byUnitId = new Map(); this.free = []; this.next = 0; }
  assign(key, unitId = null) {
    let slot = this.byKey.get(key);
    if (slot !== undefined) return slot;
    slot = this.free.pop() ?? this.next++;
    if (slot >= SNAPSHOT_SLOT_COUNT) throw Error('snapshot slot table full');
    this.byKey.set(key, slot);
    if (unitId !== null) this.byUnitId.set(unitId, slot);
    return slot;
  }
  // assign(`unit:${unitId}`, unitId) without building the key when the unit
  // holds a slot: byUnitId names exactly the slots unit keys hold.
  assignUnit(unitId) { return this.byUnitId.get(unitId) ?? this.assign(unitKey(unitId), unitId); }
  slotFor(key) { return this.byKey.get(key) ?? -1; }
  slotForUnit(unitId) { return this.byUnitId.get(unitId) ?? -1; }
  release(key) {
    const slot = this.byKey.get(key); if (slot === undefined) return;
    this.byKey.delete(key); this.free.push(slot);
    for (const [id, value] of this.byUnitId) if (value === slot) this.byUnitId.delete(id);
  }
  // A tombstone repeats every snapshot until its id lives again: once the
  // slot is freed there is nothing to look for.
  releaseUnit(unitId) {
    const slot = this.byUnitId.get(unitId); if (slot === undefined) return;
    for (const [key, value] of this.byKey) if (value === slot) { this.release(key); return; }
  }
}
// Item and remote ids are small slot indices: each key is built once.
const itemKeys = [], remoteKeys = [];
const unitKey = id => `unit:${id}`, itemKey = id => itemKeys[id] ??= `item:${id}`, remoteKey = id => remoteKeys[id] ??= `remote:${id}`;
// `previous` is the prior snapshot's entry for the same slot. Nested
// quantized values that come out identical keep the previous object, and
// the whole entry is reused when every group compares equal, so unchanged
// actors cost the encoder one identity check instead of fresh objects plus
// per-group walks. Nested entry values are never
// mutated after capture (codec readers replace them), so sharing is safe.
function mountEntry(mount) {
  return mount ? { slot: mount.slot, tilt: angleCode(mount.tilt), roll: angleCode(mount.roll),
    walkPhase: Math.round(Math.max(0, Math.min(65.535, mount.walkPhase || 0)) * 1000),
    walkBlend: Math.round(Math.max(0, Math.min(1, mount.walkBlend || 0)) * 255),
    seats: (mount.seats || []).map(s => [angleCode(s.yaw), angleCode(s.pitch)]) }
    : { slot: null, tilt: 0, roll: 0, walkPhase: 0, walkBlend: 0, seats: [] };
}
function sameMount(mount, prev) {
  return prev !== undefined && prev.slot === mount.slot && prev.tilt === mount.tilt && prev.roll === mount.roll
    && prev.walkPhase === mount.walkPhase && prev.walkBlend === mount.walkBlend
    && prev.seats.length === mount.seats.length && mount.seats.every((s, i) => prev.seats[i][0] === s[0] && prev.seats[i][1] === s[1]);
}
const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
const NO_MOTION = Object.freeze([0, 0, 0]);
// Entries, and their positions and velocities, from snapshots the host's
// ring has let go (snapshot-host.js), for the next captures to fill in
// place of new ones. The ring keeps every snapshot for 128 publications,
// long enough that each new entry was moved to the old generation, and most
// of the worker's full collections went on taking them back. Turrets,
// soldiers, droidekas and hulls are reused; items and remotes are rare.
const SPARE_CLASSES = ['turret', 'soldier', 'droideka', 'vehicle'];
export class EntrySpare {
  constructor(limit = 2048) {
    this.limit = limit; this.pos = []; this.vel = [];
    this.byClass = Object.fromEntries(SPARE_CLASSES.map(cls => [cls, []]));
  }
  // `evicted` has left the ring, and `next` is the oldest snapshot it still
  // holds. A capture reuses only the previous snapshot's entry (or its
  // position or velocity) at the same slot, so what `next` does not hold
  // at that slot no snapshot holds.
  recycle(evicted, next) {
    for (const [slot, entry] of evicted.slots) {
      const kept = next?.slots.get(slot);
      if (kept === entry) continue;
      const list = this.byClass[entry.cls];
      if (list && list.length < this.limit) list.push(entry);
      if (entry.pos && entry.pos !== kept?.pos && this.pos.length < this.limit) this.pos.push(entry.pos);
      if (entry.vel && entry.vel !== kept?.vel && this.vel.length < this.limit) this.vel.push(entry.vel);
    }
  }
}
function spareFor(spare, cls) { return spare ? spare.byClass[cls].pop() : undefined; }

// The entry for `record`, equality with `previous` decided where the
// comparisons happen anyway. When every value matches and `previous` carries
// no discontinuity (its disc described its own arrival and must not stick to
// this snapshot), `previous` itself is the entry and nothing is allocated:
// snapshotFromRecords' disc checks could find nothing on it. Otherwise a
// fresh entry, each class's fields in one literal (snapshotFromRecords then
// sets its disc). The quantization is snapshot-codec.js's quantize* written
// out for the three-element arrays records carry.
function buildEntry(record, bounds, mount, seatSlot, seatIndex, previous, spare = null) {
  const kind = record.kind, prev = previous ?? null;
  if (kind === 'turret') {
    const classId = record.building, team = record.team ?? 0, health = record.health, state = TURRET_STATES.indexOf(record.state);
    const yaw = angleCode(record.yaw), pitch = angleCode(record.pitch), gunYaw = angleCode(record.gunYaw), gunPitch = angleCode(record.gunPitch);
    if (prev !== null && !prev.disc && prev.cls === kind && prev.entityId === record.id && prev.generation === record.generation && prev.classId === classId
      && prev.yaw === yaw && prev.pitch === pitch && prev.gunYaw === gunYaw && prev.gunPitch === gunPitch && prev.health === health && prev.state === state && prev.team === team) return prev;
    const e = spareFor(spare, kind);
    if (!e) return { cls: kind, entityId: record.id, generation: record.generation, classId, team, disc: false, yaw, pitch, gunYaw, gunPitch, health, state };
    e.cls = kind; e.entityId = record.id; e.generation = record.generation; e.classId = classId; e.team = team; e.disc = false;
    e.yaw = yaw; e.pitch = pitch; e.gunYaw = gunYaw; e.gunPitch = gunPitch; e.health = health; e.state = state;
    return e;
  }
  const classId = record.classId ?? 0, team = record.team ?? 0;
  let same = prev !== null && prev.cls === kind && prev.entityId === record.id && prev.generation === record.generation && prev.classId === classId && prev.team === team;
  // Quantize once; keep the previous array/object when the codes match.
  const p = record.position ?? NO_MOTION;
  const x = clamp(Math.round((p[0] - bounds.xMin) / bounds.xExtent * 65535), 0, 65535);
  const y = clamp(Math.round((p[1] - bounds.yMin) / bounds.yExtent * 65535), 0, 65535);
  const z = clamp(Math.round((p[2] - bounds.zMin) / bounds.zExtent * 65535), 0, 65535);
  let pos = prev?.pos;
  if (!(pos && x === pos[0] && y === pos[1] && z === pos[2])) {
    pos = spare?.pos.pop();
    if (pos) { pos[0] = x; pos[1] = y; pos[2] = z; } else pos = [x, y, z];
    same = false;
  }
  // An item's velocity always goes at 1/32 m/s: a page flies a thrown
  // item on from its records (network-entity-items.js), and 0.25 m/s
  // steps would round a slow slide to rest.
  const v = record.velocity ?? NO_MOTION, v0 = v[0], v1 = v[1], v2 = v[2];
  const wide = kind === 'item' || !(Number.isFinite(v0) && Math.abs(v0) <= 31.75 && Number.isFinite(v1) && Math.abs(v1) <= 31.75 && Number.isFinite(v2) && Math.abs(v2) <= 31.75);
  const c0 = wide ? clamp(Math.round(v0 * 32), -32767, 32767) : clamp(Math.round(v0 / 0.25), -127, 127);
  const c1 = wide ? clamp(Math.round(v1 * 32), -32767, 32767) : clamp(Math.round(v1 / 0.25), -127, 127);
  const c2 = wide ? clamp(Math.round(v2 * 32), -32767, 32767) : clamp(Math.round(v2 / 0.25), -127, 127);
  let vel = prev?.vel;
  if (!(vel && wide === vel.wide && c0 === vel.v[0] && c1 === vel.v[1] && c2 === vel.v[2])) {
    vel = spare?.vel.pop();
    if (vel) { vel.wide = wide; vel.v[0] = c0; vel.v[1] = c1; vel.v[2] = c2; } else vel = { wide, v: [c0, c1, c2] };
    same = false;
  }
  if (kind === 'soldier' || kind === 'droideka') {
    const yaw = quantizeYaw(record.yaw), pitch = quantizePitch(record.pitch), health = record.health, posture = record.posture & 3,
      state = record.state, weapon = record.weapon, fireCounter = (record.fireCounter ?? 0) & 0xff;
    same &&= yaw === prev.yaw && pitch === prev.pitch && health === prev.health && posture === prev.posture && state === prev.state
      && weapon === prev.weapon && seatSlot === prev.seatSlot && seatIndex === prev.seatIndex && fireCounter === prev.fireCounter;
    if (kind === 'soldier') {
      const charge = record.charge ?? 0;
      if (same && charge === prev.charge && !prev.disc) return prev;
      const e = spareFor(spare, kind);
      if (!e) return { cls: kind, entityId: record.id, generation: record.generation, classId, team, disc: false, pos, vel,
        yaw, pitch, health, posture, charge, state, weapon, seatSlot, seatIndex, fireCounter };
      e.cls = kind; e.entityId = record.id; e.generation = record.generation; e.classId = classId; e.team = team; e.disc = false; e.pos = pos; e.vel = vel;
      e.yaw = yaw; e.pitch = pitch; e.health = health; e.posture = posture; e.charge = charge; e.state = state; e.weapon = weapon; e.seatSlot = seatSlot; e.seatIndex = seatIndex; e.fireCounter = fireCounter;
      return e;
    }
    const anim = prev?.anim && prev.anim[0] === record.time && prev.anim[1] === record.roll && prev.anim[2] === record.lean && prev.anim[3] === record.legs && prev.anim[4] === record.shield
      ? prev.anim : [record.time, record.roll, record.lean, record.legs, record.shield];
    if (same && record.mode === prev.mode && record.reverse === prev.reverse && record.dead === prev.dead && record.shieldOn === prev.shieldOn && anim === prev.anim && !prev.disc) return prev;
    const e = spareFor(spare, kind);
    if (!e) return { cls: kind, entityId: record.id, generation: record.generation, classId, team, disc: false, pos, vel,
      yaw, pitch, health, posture, state, weapon, seatSlot, seatIndex, fireCounter, mode: record.mode, reverse: record.reverse, dead: record.dead, shieldOn: record.shieldOn, anim };
    e.cls = kind; e.entityId = record.id; e.generation = record.generation; e.classId = classId; e.team = team; e.disc = false; e.pos = pos; e.vel = vel;
    e.yaw = yaw; e.pitch = pitch; e.health = health; e.posture = posture; e.state = state; e.weapon = weapon; e.seatSlot = seatSlot; e.seatIndex = seatIndex; e.fireCounter = fireCounter;
    e.mode = record.mode; e.reverse = record.reverse; e.dead = record.dead; e.shieldOn = record.shieldOn; e.anim = anim;
    return e;
  }
  if (kind === 'vehicle') {
    const quat = quantizeQuaternion(record.quaternion), mountValue = mountEntry(mount);
    const quatSame = prev?.quat && quat.index === prev.quat.index && quat.v[0] === prev.quat.v[0] && quat.v[1] === prev.quat.v[1] && quat.v[2] === prev.quat.v[2];
    const mountSame = prev !== null && sameMount(mountValue, prev.mount);
    const health = record.health, state = record.state, power = record.power, fireCounter = (record.fireCounter ?? 0) & 0xff;
    if (same && quatSame && mountSame && health === prev.health && state === prev.state && power === prev.power && fireCounter === prev.fireCounter && !prev.disc) return prev;
    const e = spareFor(spare, kind);
    if (!e) return { cls: kind, entityId: record.id, generation: record.generation, classId, team, disc: false, pos, vel,
      quat: quatSame ? prev.quat : quat, health, state, power, fireCounter, mount: mountSame ? prev.mount : mountValue };
    e.cls = kind; e.entityId = record.id; e.generation = record.generation; e.classId = classId; e.team = team; e.disc = false; e.pos = pos; e.vel = vel;
    e.quat = quatSame ? prev.quat : quat; e.health = health; e.state = state; e.power = power; e.fireCounter = fireCounter; e.mount = mountSame ? prev.mount : mountValue;
    return e;
  }
  if (kind === 'item') {
    const up = record.up ?? [0, 1, 0];
    const upSame = prev?.up !== undefined && up.every((x, i) => prev.up[i] === Math.max(-32767, Math.min(32767, Math.round(x * 32767))));
    const yaw = quantizeYaw(record.yaw ?? 0), active = record.active, definitionId = record.definitionId ?? 0;
    if (same && upSame && yaw === prev.yaw && active === prev.active && definitionId === prev.definitionId && !prev.disc) return prev;
    return { cls: kind, entityId: record.id, generation: record.generation, classId, team, disc: false, pos, vel,
      yaw, active, definitionId, up: upSame ? prev.up : up.map(x => Math.max(-32767, Math.min(32767, Math.round(x * 32767)))) };
  }
  if (kind === 'remote') {
    const yaw = quantizeYaw(record.yaw ?? 0), pitch = quantizePitch(record.pitch ?? 0), active = record.active, definitionId = record.definitionId ?? 0, health = record.health ?? 0;
    if (same && yaw === prev.yaw && pitch === prev.pitch && active === prev.active && definitionId === prev.definitionId && health === prev.health && !prev.disc) return prev;
    return { cls: kind, entityId: record.id, generation: record.generation, classId, team, disc: false, pos, vel, yaw, pitch, active, definitionId, health };
  }
  throw Error(`snapshot entry from ${record.kind} record`);
}
// A fresh entry, even when every value matches `previous`.
export function entryFromRecord(record, { bounds = DEFAULT_SNAPSHOT_BOUNDS, mount = null, seatSlot = -1, seatIndex = 0, previous = null } = {}) {
  const entry = buildEntry(record, bounds, mount, seatSlot, seatIndex, previous);
  return entry === previous ? { ...previous } : entry;
}
export function recordFromEntry(entry, tick, bounds = DEFAULT_SNAPSHOT_BOUNDS) {
  const base = { kind: entry.cls, id: entry.entityId, generation: entry.generation, tick, classId: entry.classId, team: entry.team };
  if (entry.cls === 'turret') { delete base.classId; return { ...base, building: entry.classId, state: TURRET_STATES[entry.state],
    yaw: angleValue(entry.yaw), pitch: angleValue(entry.pitch), gunYaw: angleValue(entry.gunYaw), gunPitch: angleValue(entry.gunPitch), health: entry.health }; }
  base.position = dequantizePosition(entry.pos, bounds); base.velocity = dequantizeVelocity(entry.vel);
  if (entry.cls === 'soldier' || entry.cls === 'droideka') {
    Object.assign(base, { yaw: dequantizeYaw(entry.yaw), pitch: dequantizePitch(entry.pitch), health: entry.health,
      posture: entry.posture, charge: entry.charge, state: entry.state, weapon: entry.weapon, fireCounter: entry.fireCounter });
    if (entry.cls === 'droideka') Object.assign(base, { mode: entry.mode, reverse: entry.reverse, dead: entry.dead, shieldOn: entry.shieldOn,
      time: entry.anim[0], roll: entry.anim[1], lean: entry.anim[2], legs: entry.anim[3], shield: entry.anim[4] });
  } else if (entry.cls === 'vehicle') {
    Object.assign(base, { quaternion: dequantizeQuaternion(entry.quat), health: entry.health, state: entry.state, power: entry.power });
  } else if (entry.cls === 'item') {
    Object.assign(base, { active: entry.active });
    if (entry.active) Object.assign(base, { definitionId: entry.definitionId, yaw: dequantizeYaw(entry.yaw), up: entry.up.map(x => x / 32767) });
    else { delete base.position; delete base.velocity; }
  } else if (entry.cls === 'remote') {
    Object.assign(base, { active: entry.active });
    if (entry.active) Object.assign(base, { definitionId: entry.definitionId, yaw: dequantizeYaw(entry.yaw), pitch: dequantizePitch(entry.pitch), health: entry.health });
    else { delete base.position; delete base.velocity; }
  }
  return base;
}
export function mountRecordFromEntry(entry, tick) {
  const m = entry.mount;
  return { kind: 'mount', id: entry.entityId, generation: entry.generation, tick, slot: m.slot,
    tilt: angleValue(m.tilt), roll: angleValue(m.roll), walkPhase: m.walkPhase / 1000, walkBlend: m.walkBlend / 255,
    seats: m.seats.map(([yaw, pitch]) => ({ yaw: angleValue(yaw), pitch: angleValue(pitch) })) };
}
// Assemble one snapshot from a publication's capture records. Mount records
// merge into their vehicle entry; seat records become the rider's seat
// slot; tombstones free their slot; summary records given as worldRecords
// become world fields (or `world`, WorldFieldStagger.world(), is the world
// as is). Teleports within one life (displacement beyond a speed allowance
// since the previous snapshot) set the discontinuity bit.
export function snapshotFromRecords({ tick, epoch = 1, records, worldRecords = [], world = null, slots, bounds = DEFAULT_SNAPSHOT_BOUNDS, previous = null, spare = null }) {
  const snapshot = emptySnapshot(tick, epoch);
  const mounts = new Map(), seats = new Map();
  for (const r of records) { if (r.kind === 'mount') mounts.set(r.id, r); else if (r.kind === 'seat') seats.set(r.id, r); }
  const byId = new Map();
  for (const r of records) if (r.kind === 'vehicle') byId.set(r.id, slots.assignUnit(r.id));
  const previousSlots = previous?.slots;
  for (const r of records) {
    const kind = r.kind;
    if (kind === 'tombstone') { slots.releaseUnit(r.id); continue; }
    if (kind === 'vehicle' || kind === 'soldier' || kind === 'droideka' || kind === 'turret') {
      const slot = byId.get(r.id) ?? slots.assignUnit(r.id);
      const seat = seats.get(r.id);
      const seatSlot = seat?.vehicleId ? slots.slotForUnit(seat.vehicleId) : -1;
      const before = previousSlots?.get(slot);
      const entry = buildEntry(r, bounds, mounts.get(r.id), seatSlot >= 0 ? seatSlot : -1, seat?.seatIndex ?? 0, before, spare);
      if (entry !== before && before && before.generation === entry.generation && before.cls === entry.cls) {
        if (entry.seatSlot !== undefined && before.seatSlot !== entry.seatSlot) entry.disc = true;
        if (entry.pos && before.pos) {
          const gap = Math.max(1 / 60, (tick - previous.tick) / 60);
          // dequantizeVelocity's values, unallocated.
          const v = entry.vel.v, speed = entry.vel.wide ? Math.hypot(v[0] / 32, v[1] / 32, v[2] / 32) : Math.hypot(v[0] * 0.25, v[1] * 0.25, v[2] * 0.25);
          const allowance = 3 + Math.max(entry.cls === 'vehicle' ? 60 : 12, speed) * gap;
          const dx = (entry.pos[0] - before.pos[0]) * bounds.xExtent / 65535;
          const dy = (entry.pos[1] - before.pos[1]) * bounds.yExtent / 65535;
          const dz = (entry.pos[2] - before.pos[2]) * bounds.zExtent / 65535;
          entry.disc = entry.disc || Math.hypot(dx, dy, dz) > allowance;
        }
      }
      snapshot.slots.set(slot, entry);
    } else if (kind === 'item' || kind === 'remote') {
      const slot = slots.assign(kind === 'item' ? itemKey(r.id) : remoteKey(r.id));
      snapshot.slots.set(slot, buildEntry(r, bounds, null, -1, 0, previousSlots?.get(slot), spare));
    }
  }
  if (world) snapshot.world = world;
  else for (const r of worldRecords) {
    const key = worldFieldKey(r), before = previous?.world.get(key);
    snapshot.world.set(key, before && before.payload === r.payload && before.id === r.id && before.schema === r.schema && before.generation === r.generation
      ? before : { id: r.id, schema: r.schema, generation: r.generation, payload: r.payload });
  }
  return snapshot;
}
export function ownSummaryFields(records) {
  const map = new Map();
  for (const r of records) map.set(worldFieldKey(r), { id: r.id, schema: r.schema, generation: r.generation, payload: r.payload });
  return map;
}
export function worldRecordFromField(field, tick) {
  return { kind: 'summary', id: field.id, generation: field.generation, tick, schema: field.schema, payload: field.payload };
}
// World fields are snapshot state, but some of their payloads carry timers
// that change every tick (building pulse, weapon timers). Refreshing every
// field into every snapshot would spend more bytes than the entities. Each
// schema refreshes on a staggered cadence instead (the old path's summary
// intervals, kept as state): between refreshes the snapshot keeps the last
// captured value, so a lost datagram still needs no bookkeeping. Building
// state or team changes (destroyed, rebuilt, captured) refresh at once.
// Everyone's records (schema 5, DETAILS from Tab's table) refresh every
// 5 s: their times change every second.
// state or team changes (destroyed, rebuilt, captured) refresh at once, as
// does any change of a creature's state (schema 9: a grab, a seizure, its
// end), which carries no timers.
export const SNAPSHOT_WORLD_CADENCE_TICKS = Object.freeze({ 1: 6, 2: 6, 4: 60, 5: 300, 6: 30, 7: 48, 8: 12, 9: 60, 10: 60 });
const samePayload = (a, b) => a === b || (a?.length === b?.length && a.every((byte, i) => byte === b[i]));
const jsonDecoder = new TextDecoder();
function buildingStateTeam(payload) {
  try { const value = JSON.parse(jsonDecoder.decode(payload)); return Array.isArray(value) ? `${value[1]}:${value[3]}` : null; } catch { return null; }
}
export class WorldFieldStagger {
  constructor(cadence = SNAPSHOT_WORLD_CADENCE_TICKS) {
    this.cadence = cadence;
    // Fields by key in the order first offered (their order in every
    // snapshot's world section), and the same entries by schema, then id, so
    // a lookup builds no key string. `round` marks the fields offered to the
    // newest update.
    this.fields = new Map(); this.bySchema = new Map(); this.round = 0;
  }
  entry(schema, id) { return this.bySchema.get(schema)?.get(id); }
  update(tick, records, completeSchemas = new Set()) { this.take(tick, records, completeSchemas); return this.values(); }
  // update() without building the list of values.
  take(tick, records, completeSchemas = new Set()) {
    const round = ++this.round;
    for (const record of records) {
      const schema = record.schema, cadence = this.cadence[schema] ?? 6;
      let entry = this.entry(schema, record.id);
      // A creature's state is taken at once (it changes only when it acts).
      if (!entry) {
        entry = { key: worldFieldKey(record), id: record.id, schema, record: null, field: null, stateTeam: null, round,
          nextDue: schema === 9 || schema === 10 ? tick : tick + (((record.id * 2654435761) >>> 0) % cadence) };
        this.fields.set(entry.key, entry);
        let ids = this.bySchema.get(schema);
        if (!ids) this.bySchema.set(schema, ids = new Map());
        ids.set(record.id, entry);
      }
      entry.round = round;
      // The held record offered again (held()): the comparisons below would
      // find it unchanged.
      if (record === entry.record && tick < entry.nextDue) continue;
      // A building's state and team are parsed once per capture, not again
      // for the stored record.
      const stateTeam = schema === 7 && entry.record ? buildingStateTeam(record.payload) : null;
      const immediate = schema === 7 && entry.record && stateTeam !== entry.stateTeam
        || (schema === 9 || schema === 10) && entry.record && !samePayload(entry.record.payload, record.payload);
      if (immediate || tick >= entry.nextDue) {
        entry.record = record; entry.nextDue = tick + cadence;
        entry.field = { id: record.id, schema, generation: record.generation, payload: record.payload };
        if (schema === 7) entry.stateTeam = stateTeam ?? buildingStateTeam(record.payload);
      }
    }
    for (const [key, entry] of this.fields) if (entry.round !== round && completeSchemas.has(entry.record?.schema)) { this.fields.delete(key); this.bySchema.get(entry.schema).delete(entry.id); }
  }
  values() { const records = []; for (const entry of this.fields.values()) if (entry.record) records.push(entry.record); return records; }
  // The values as a snapshot's world fields, keyed and ordered as
  // snapshotFromRecords would make them from values(). A field object lasts
  // as long as its record, so an unchanged field is the same object from
  // snapshot to snapshot (the encoder's identity fast path).
  world() { const world = new Map(); for (const entry of this.fields.values()) if (entry.record) world.set(entry.key, entry.field); return world; }
  // For a field held that update() at `tick` would take no fresh record of:
  // the record it holds (null when none is taken yet), to offer again
  // instead of building one. Undefined when a fresh record is needed: a
  // field never offered (update() adds fields in offer order), a due one, a
  // building whose state or team text (`stateTeam`, as buildingStateTeam
  // reads it) changed, a creature or trap (compared by payload).
  held(tick, schema, id, stateTeam = null) {
    const entry = this.entry(schema, id);
    if (!entry || tick >= entry.nextDue) return undefined;
    if (entry.record && (schema === 7 ? stateTeam !== entry.stateTeam : schema === 9 || schema === 10)) return undefined;
    return entry.record;
  }
  // Capture-side hints so the host can skip capturing fields whose refresh
  // is not due yet (their cached value stays in the snapshot either way).
  isDue(tick, key) { const entry = this.fields.get(key); return !entry || tick >= entry.nextDue; }
  isFieldDue(tick, schema, id) { const entry = this.entry(schema, id); return !entry || tick >= entry.nextDue; }
  anyDue(tick, schemas) {
    if (!this.fields.size) return true;
    for (const schema of schemas) for (const entry of this.bySchema.get(schema)?.values() ?? []) if (entry.record && tick >= entry.nextDue) return true;
    return false;
  }
  // Whether capturing `schemas` now could change anything: a held field is
  // due, or a field first seen (and not yet taken) has come due. Fields of
  // these schemas not captured are kept as they are, so when this is false
  // (and the set of field keys cannot have changed) skipping the capture
  // leaves the stagger exactly as capturing would.
  schemaDue(tick, schemas) {
    let seen = false;
    for (const schema of schemas) for (const entry of this.bySchema.get(schema)?.values() ?? []) { seen = true; if (tick >= entry.nextDue) return true; }
    return !seen;
  }
}
