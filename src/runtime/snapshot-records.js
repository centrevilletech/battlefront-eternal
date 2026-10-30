// Conversion between the capture layer's semantic records (the shapes
// network-entity-capture.js produces and network-entity-game.js consumes)
// and quantized snapshot entries. The slot table is the net identity; each
// entry also carries the server unit id and u32 generation its `new`
// record announces, because combat, seats and own corrections name bodies
// by those, not by slot.
import {
  SNAPSHOT_SLOT_COUNT, DEFAULT_SNAPSHOT_BOUNDS, emptySnapshot, worldFieldKey,
  quantizePosition, dequantizePosition, quantizeVelocity, dequantizeVelocity,
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
  slotFor(key) { return this.byKey.get(key) ?? -1; }
  slotForUnit(unitId) { return this.byUnitId.get(unitId) ?? -1; }
  release(key) {
    const slot = this.byKey.get(key); if (slot === undefined) return;
    this.byKey.delete(key); this.free.push(slot);
    for (const [id, value] of this.byUnitId) if (value === slot) this.byUnitId.delete(id);
  }
  releaseUnit(unitId) { for (const [key, slot] of [...this.byKey]) if (slot === this.byUnitId.get(unitId)) this.release(key); }
}
const unitKey = id => `unit:${id}`, itemKey = id => `item:${id}`, remoteKey = id => `remote:${id}`;
// `previous` is the prior snapshot's entry for the same slot. Nested
// quantized values that come out identical keep the previous object, and
// snapshotFromRecords reuses the whole entry when every group compares
// equal, so unchanged actors cost the encoder one identity check instead
// of fresh objects plus per-group walks. Nested entry values are never
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
// Builds the entry and reports whether every field matches `previous`
// exactly. Equality is decided at construction, where the comparisons
// already happen, instead of a second per-group walk after the fact.
function buildEntry(record, { bounds = DEFAULT_SNAPSHOT_BOUNDS, mount = null, seatSlot = -1, seatIndex = 0, previous = null } = {}) {
  const e = { cls: record.kind, entityId: record.id, generation: record.generation, classId: record.classId ?? 0, team: record.team ?? 0, disc: false };
  let same = previous !== null && previous !== undefined && previous.cls === e.cls && previous.entityId === e.entityId
    && previous.generation === e.generation && previous.classId === e.classId && previous.team === e.team;
  if (record.kind !== 'turret') {
    // Quantize once; keep the previous array/object when the codes match.
    const pos = quantizePosition(record.position ?? [0, 0, 0], bounds);
    if (previous?.pos && pos[0] === previous.pos[0] && pos[1] === previous.pos[1] && pos[2] === previous.pos[2]) e.pos = previous.pos; else { e.pos = pos; same = false; }
    // An item's velocity always goes at 1/32 m/s: a page flies a thrown
    // item on from its records (network-entity-items.js), and 0.25 m/s
    // steps would round a slow slide to rest.
    const vel = quantizeVelocity(record.velocity ?? [0, 0, 0], record.kind === 'item');
    if (previous?.vel && vel.wide === previous.vel.wide && vel.v[0] === previous.vel.v[0] && vel.v[1] === previous.vel.v[1] && vel.v[2] === previous.vel.v[2]) e.vel = previous.vel; else { e.vel = vel; same = false; }
  }
  if (record.kind === 'soldier' || record.kind === 'droideka') {
    Object.assign(e, { yaw: quantizeYaw(record.yaw), pitch: quantizePitch(record.pitch), health: record.health,
      posture: record.posture & 3, charge: record.charge ?? 0, state: record.state, weapon: record.weapon,
      seatSlot, seatIndex, fireCounter: (record.fireCounter ?? 0) & 0xff });
    same &&= e.yaw === previous.yaw && e.pitch === previous.pitch && e.health === previous.health && e.posture === previous.posture
      && e.state === previous.state && e.weapon === previous.weapon && e.seatSlot === previous.seatSlot && e.seatIndex === previous.seatIndex
      && e.fireCounter === previous.fireCounter;
    if (record.kind === 'droideka') { delete e.charge; Object.assign(e, { mode: record.mode, reverse: record.reverse, dead: record.dead, shieldOn: record.shieldOn,
      anim: previous?.anim && previous.anim[0] === record.time && previous.anim[1] === record.roll && previous.anim[2] === record.lean && previous.anim[3] === record.legs && previous.anim[4] === record.shield
        ? previous.anim : [record.time, record.roll, record.lean, record.legs, record.shield] });
      same &&= e.mode === previous.mode && e.reverse === previous.reverse && e.dead === previous.dead && e.shieldOn === previous.shieldOn && e.anim === previous.anim;
    } else same &&= e.charge === previous.charge;
  } else if (record.kind === 'vehicle') {
    const quat = quantizeQuaternion(record.quaternion), mountValue = mountEntry(mount);
    const quatSame = previous?.quat && quat.index === previous.quat.index && quat.v[0] === previous.quat.v[0] && quat.v[1] === previous.quat.v[1] && quat.v[2] === previous.quat.v[2];
    const mountSame = previous !== null && previous !== undefined && sameMount(mountValue, previous.mount);
    Object.assign(e, { quat: quatSame ? previous.quat : quat,
      health: record.health, state: record.state, power: record.power,
      fireCounter: (record.fireCounter ?? 0) & 0xff,
      mount: mountSame ? previous.mount : mountValue });
    same &&= !!quatSame && mountSame && e.health === previous.health && e.state === previous.state && e.power === previous.power && e.fireCounter === previous.fireCounter;
  } else if (record.kind === 'turret') {
    e.classId = record.building;
    Object.assign(e, { yaw: angleCode(record.yaw), pitch: angleCode(record.pitch), gunYaw: angleCode(record.gunYaw), gunPitch: angleCode(record.gunPitch),
      health: record.health, state: TURRET_STATES.indexOf(record.state) });
    same = previous !== null && previous !== undefined && previous.cls === e.cls && previous.entityId === e.entityId && previous.generation === e.generation
      && previous.classId === e.classId && e.yaw === previous.yaw && e.pitch === previous.pitch && e.gunYaw === previous.gunYaw && e.gunPitch === previous.gunPitch
      && e.health === previous.health && e.state === previous.state && e.team === previous.team;
  } else if (record.kind === 'item') {
    const up = record.up ?? [0, 1, 0];
    const upSame = previous?.up !== undefined && up.every((x, i) => previous.up[i] === Math.max(-32767, Math.min(32767, Math.round(x * 32767))));
    Object.assign(e, { yaw: quantizeYaw(record.yaw ?? 0), active: record.active, definitionId: record.definitionId ?? 0,
      up: upSame ? previous.up : up.map(x => Math.max(-32767, Math.min(32767, Math.round(x * 32767)))) });
    same &&= upSame && e.yaw === previous.yaw && e.active === previous.active && e.definitionId === previous.definitionId;
  } else if (record.kind === 'remote') {
    Object.assign(e, { yaw: quantizeYaw(record.yaw ?? 0), pitch: quantizePitch(record.pitch ?? 0), active: record.active, definitionId: record.definitionId ?? 0, health: record.health ?? 0 });
    same &&= e.yaw === previous.yaw && e.pitch === previous.pitch && e.active === previous.active && e.definitionId === previous.definitionId && e.health === previous.health;
  } else throw Error(`snapshot entry from ${record.kind} record`);
  return { entry: e, same };
}
export function entryFromRecord(record, options = {}) { return buildEntry(record, options).entry; }
// Whole-entry reuse also needs a clear disc on both sides: the previous
// entry's disc described its own arrival and must not stick to this snapshot.
const reusableEntry = (built, before) => built.same && !built.entry.disc && !before.disc;
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
// become world fields. Teleports within one life (displacement beyond a
// speed allowance since the previous snapshot) set the discontinuity bit.
export function snapshotFromRecords({ tick, epoch = 1, records, worldRecords = [], slots, bounds = DEFAULT_SNAPSHOT_BOUNDS, previous = null }) {
  const snapshot = emptySnapshot(tick, epoch);
  const mounts = new Map(), seats = new Map();
  for (const r of records) { if (r.kind === 'mount') mounts.set(r.id, r); else if (r.kind === 'seat') seats.set(r.id, r); }
  const byId = new Map();
  for (const r of records) if (r.kind === 'vehicle') byId.set(r.id, slots.assign(unitKey(r.id), r.id));
  for (const r of records) {
    if (r.kind === 'tombstone') { slots.releaseUnit(r.id); continue; }
    if (r.kind === 'vehicle' || r.kind === 'soldier' || r.kind === 'droideka' || r.kind === 'turret') {
      const slot = byId.get(r.id) ?? slots.assign(unitKey(r.id), r.id);
      const seat = seats.get(r.id);
      const seatSlot = seat?.vehicleId ? slots.slotForUnit(seat.vehicleId) : -1;
      const before = previous?.slots.get(slot);
      const built = buildEntry(r, { bounds, mount: mounts.get(r.id), seatSlot: seatSlot >= 0 ? seatSlot : -1, seatIndex: seat?.seatIndex ?? 0, previous: before });
      const entry = built.entry;
      if (before && before.generation === entry.generation && before.cls === entry.cls && entry.seatSlot !== undefined && before.seatSlot !== entry.seatSlot) entry.disc = true;
      if (before && before.generation === entry.generation && before.cls === entry.cls && entry.pos && before.pos) {
        const gap = Math.max(1 / 60, (tick - previous.tick) / 60);
        const v = dequantizeVelocity(entry.vel);
        const allowance = 3 + Math.max(entry.cls === 'vehicle' ? 60 : 12, Math.hypot(v[0], v[1], v[2])) * gap;
        const dx = (entry.pos[0] - before.pos[0]) * bounds.xExtent / 65535;
        const dy = (entry.pos[1] - before.pos[1]) * bounds.yExtent / 65535;
        const dz = (entry.pos[2] - before.pos[2]) * bounds.zExtent / 65535;
        entry.disc = entry.disc || Math.hypot(dx, dy, dz) > allowance;
      }
      snapshot.slots.set(slot, before && reusableEntry(built, before) ? before : entry);
    } else if (r.kind === 'item' || r.kind === 'remote') {
      const slot = slots.assign(r.kind === 'item' ? itemKey(r.id) : remoteKey(r.id));
      const before = previous?.slots.get(slot);
      const built = buildEntry(r, { bounds, previous: before });
      snapshot.slots.set(slot, before && reusableEntry(built, before) ? before : built.entry);
    }
  }
  for (const r of worldRecords) {
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
  constructor(cadence = SNAPSHOT_WORLD_CADENCE_TICKS) { this.cadence = cadence; this.fields = new Map(); }
  update(tick, records, completeSchemas = new Set()) {
    const seen = new Set();
    for (const record of records) {
      const key = worldFieldKey(record); seen.add(key);
      const cadence = this.cadence[record.schema] ?? 6;
      let entry = this.fields.get(key);
      // A creature's state is taken at once (it changes only when it acts).
      if (!entry) { entry = { record: null, schema: record.schema, stateTeam: null, nextDue: record.schema === 9 || record.schema === 10 ? tick : tick + (((record.id * 2654435761) >>> 0) % cadence) }; this.fields.set(key, entry); }
      // A building's state and team are parsed once per capture, not again
      // for the stored record.
      const stateTeam = record.schema === 7 && entry.record ? buildingStateTeam(record.payload) : null;
      const immediate = record.schema === 7 && entry.record && stateTeam !== entry.stateTeam
        || (record.schema === 9 || record.schema === 10) && entry.record && !samePayload(entry.record.payload, record.payload);
      if (immediate || tick >= entry.nextDue) {
        entry.record = record; entry.nextDue = tick + cadence;
        if (record.schema === 7) entry.stateTeam = stateTeam ?? buildingStateTeam(record.payload);
      }
    }
    for (const [key, entry] of this.fields) if (!seen.has(key) && completeSchemas.has(entry.record?.schema)) this.fields.delete(key);
    return this.values();
  }
  values() { return [...this.fields.values()].map(e => e.record).filter(Boolean); }
  // Capture-side hints so the host can skip capturing fields whose refresh
  // is not due yet (their cached value stays in the snapshot either way).
  isDue(tick, key) { const entry = this.fields.get(key); return !entry || tick >= entry.nextDue; }
  anyDue(tick, schemas) {
    if (!this.fields.size) return true;
    for (const [key, entry] of this.fields) if (entry.record && schemas.has(entry.record.schema) && tick >= entry.nextDue) return true;
    return false;
  }
  // Whether capturing `schemas` now could change anything: a held field is
  // due, or a field first seen (and not yet taken) has come due. Fields of
  // these schemas not captured are kept as they are, so when this is false
  // (and the set of field keys cannot have changed) skipping the capture
  // leaves the stagger exactly as capturing would.
  schemaDue(tick, schemas) {
    let seen = false;
    for (const entry of this.fields.values()) if (schemas.has(entry.schema)) { seen = true; if (tick >= entry.nextDue) return true; }
    return !seen;
  }
}
