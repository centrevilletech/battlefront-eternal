// Frozen reference: the snapshot host's capture as it was before capture
// stopped building what a snapshot does not take (engine 4ffa3a3). Copied
// unchanged: the entity records (network-entity-capture.js,
// network-entity-items.js), their conversion to entries and the world-field
// stagger (snapshot-records.js), the station and tow records
// (entity-world-state.js), and the host's capture sequence (snapshot-host.js
// captureSnapshot, here a factory over the state it closed over). Capture
// must keep producing exactly these snapshots, entry for entry and field for
// field in the same order; tests/snapshot-capture-identity.test.js compares
// them. Never edit this copy to follow a capture change: a deliberate change
// to what snapshots hold replaces the identity test instead.
import { SNAPSHOT_SLOT_COUNT, DEFAULT_SNAPSHOT_BOUNDS, emptySnapshot, worldFieldKey, quantizePosition, quantizeVelocity, dequantizeVelocity,
  quantizeYaw, quantizePitch, quantizeQuaternion } from '../../src/runtime/snapshot-codec.js';
import { TURRET_STATES, ENTITY_POSTURES, ENTITY_SOLDIER_STATES, encodeEntityRecord } from '../../src/runtime/network-entity-codec.js';
import { EntityRemoteCapture } from '../../src/runtime/network-entity-remotes.js';
import { captureEntityMount, captureEntityTurret, ENTITY_VEHICLE_STATES } from '../../src/runtime/network-entity-capture.js';
import { CREATURE_STATE_SCHEMA, CREATURE_STATE_ID, TRAP_STATE_SCHEMA, TRAP_STATE_ID } from '../../src/runtime/entity-world-state.js';
import { captureUnitState, UNIT_STATE_ID } from '../../src/runtime/entity-unit-state.js';
import { captureBattleSummary, capturePostSummary, captureRosterSummaries, captureRecordSummaries } from '../../src/runtime/network-entity-summary.js';
import { chargeStep } from '../../src/weapons.js';

// network-entity-items.js
// Slots survive removal; only their generation changes when reused. The total
// high-water history is bounded by the mission's actual item pools.
export class EntityItemCapture {
 constructor(items,registry,{capacity=(items.pools?.mine??50)+(items.pools?.powerupitem??25)}={}){
  if(!Number.isSafeInteger(capacity)||capacity<1||capacity>256)throw Error('unsupported item pool capacity');
  this.items=items;this.registry=registry;this.slots=Array.from({length:capacity},()=>({generation:0,object:null,record:null,bytes:null}));this.objects=new Map();
 }
 capture(tick){
  const live=new Set(this.items.items.filter(item=>!item.gone));
  for(const[item,index]of this.objects)if(!live.has(item)){const slot=this.slots[index];slot.object=null;slot.record={kind:'item',id:index,generation:slot.generation,tick,active:false};slot.bytes=null;this.objects.delete(item);}
  for(const item of live){let index=this.objects.get(item);if(index===undefined){index=this.slots.findIndex(slot=>!slot.object);if(index<0)throw Error('item capture capacity exceeded');const slot=this.slots[index];if(slot.generation===0xffffffff)throw Error('item slot generation exhausted');slot.generation++;slot.object=item;this.objects.set(item,index);}
   const slot=this.slots[index],raw=[item.position.x,item.position.y,item.position.z,item.velocity.x,item.velocity.y,item.velocity.z,item.yaw,item.up.x,item.up.y,item.up.z];
   if(slot.record?.active&&slot.record.generation===slot.generation&&slot.raw&&raw.every((n,i)=>n===slot.raw[i]))continue;
   slot.raw=raw;const record={kind:'item',id:index,generation:slot.generation,tick,active:true,definitionId:this.registry.definitionId(item.def),position:item.position.toArray(),velocity:item.velocity.toArray(),yaw:item.yaw,up:item.up.toArray()},bytes=encodeEntityRecord(record);
   if(slot.bytes&&bytes.length===slot.bytes.length&&bytes.every((value,i)=>i>=9&&i<=12||value===slot.bytes[i]))continue;
   slot.record=record;slot.bytes=bytes;
  }
  return this.slots.flatMap(slot=>slot.record?[slot.record]:[]);
 }
}

// A page's items online, flown here by the world's own rules (Items.fly:
// gravity, the ray against this page's collision, bounce and settle) one
// 60 Hz tick at a time as the playout clock crosses ticks, and drawn between
// the last two at the clock's fraction, so a thrown item falls smoothly
// however its records bunch. When the clock reaches a record, a copy further
// off than the records' rounding takes the world's state behind a short
// fading offset; a closer one is nudged halfway, so the rounding averages
// out. The wire has no landed bit: a record at rest (no velocity) lands the
// copy where the world says, so a resting item never drifts or falls through
// ground this page lacks. An item appears when the clock reaches its first
// record and goes when it reaches its removal. This is presentation only:
// no update(), watch(), offer(), damage or grant runs here; the world alone
// network-entity-capture.js
const enumerate=(values,name)=>{const index=values.indexOf(name);if(index<0)throw Error(`unsupported entity state ${name}`);return index;};
const reverse=map=>{const result=new Map();for(const[id,name]of map){if(result.has(name))throw Error('ambiguous entity registry');result.set(name,id);}return result;};
const boundedHealth=value=>{if(!Number.isFinite(value))throw Error('entity health is not a number');return Math.round(Math.min(65535,Math.max(0,value)));};
const required=(map,name)=>{const id=map.get(name);if(id===undefined)throw Error(`entity registry missing ${name}`);return id;};
export function captureEntitySoldier(unit,{tick,classId,weaponId=0,fireCounter=0}){
 const s=unit.soldier,state=unit.dead?(s.air==='thrown'?'thrownDead':'dead'):s.dive?(s.dive>0?'diveRight':'diveLeft'):s.air||'ground';
 return {kind:'soldier',id:unit.id,generation:unit.generation,tick,classId,team:unit.team,position:s.position.toArray(),velocity:s.velocity.toArray(),yaw:s.aimYaw,pitch:s.aimPitch,health:boundedHealth(unit.combat.health),posture:enumerate(ENTITY_POSTURES,s.posture),state:enumerate(ENTITY_SOLDIER_STATES,state),weapon:weaponId,fireCounter,charge:unit.dead?0:chargeStep(unit.combat.arsenal?.current(0))};
}
export function captureEntityVehicle(vehicle,{tick,classId}){
 return {kind:'vehicle',id:vehicle.aiTarget.id,generation:vehicle.aiTarget.generation,tick,classId,team:vehicle.team,position:vehicle.position.toArray(),velocity:vehicle.velocity.toArray(),quaternion:vehicle.quaternion.toArray(),health:boundedHealth(vehicle.health),state:enumerate(ENTITY_VEHICLE_STATES,vehicle.dying>0?'Dying':vehicle.flightState||'Ground'),
  power:vehicle.dying>0?Math.min(65535,Math.ceil(vehicle.dying*1000)):Math.round(vehicle.power||0)};
}
// Fixed actor roster capture. World summaries use network-entity-summary.js;
// projectile/effect lifecycle coverage remains separate work.
export class EntityWorldCapture {
 constructor(game,registry,{maxEntities=256,fireCounter=()=>0}={}){this.game=game;this.classes=reverse(registry.classes);this.weapons=reverse(registry.weapons);this.maxEntities=maxEntities;this.fireCounter=fireCounter;this.known=new Map();this.seats=new Map();this.leaseSerial=0;this.unsupported=[];this.items=game.items?new EntityItemCapture(game.items,registry.combat):null;this.remotes=new EntityRemoteCapture(game,registry.combat);}
 capture(tick){
  const live=new Map(),records=[...(this.items?.capture(tick)??[]),...this.remotes.capture(tick)];this.unsupported=[];
  for(const unit of this.game.units.all()){
   if(!unit.soldier||!unit.combat||unit.removed)continue;
   const weapon=unit.combat.arsenal.current(0),record=captureEntitySoldier(unit,{tick,classId:required(this.classes,unit.className),weaponId:weapon?required(this.weapons,weapon.name):0,fireCounter:this.fireCounter(unit)});
   if(unit.walker){const d=unit.soldier;Object.assign(record,{kind:'droideka',mode:d.mode,reverse:d.reverse,dead:d.dead,shieldOn:!!d.shield?.on,shield:d.shield?.value||0,time:d.time,roll:d.roll,lean:d.lean,legs:d.legs});}
   live.set(unit.id,record);records.push(record);
   // A seat names its mount by registry id: a hull or a stationary turret.
   // The same lease record covers both; the client resolves the id.
   const lease=unit.seat,mount=(lease?.vehicle||lease?.turret)?.aiTarget||null;
   const signature=`${unit.generation}:${mount?.id||0}:${mount?.generation||0}:${lease?.index||0}`,old=this.seats.get(unit.id);
   if(mount||old){if(old?.signature!==signature)this.seats.set(unit.id,{signature,record:{kind:'seat',id:unit.id,generation:unit.generation,tick,vehicleId:mount?.id||0,vehicleGeneration:mount?.generation||0,seatIndex:lease?.index||0,leaseVersion:++this.leaseSerial}});records.push(this.seats.get(unit.id).record);}
  }
  const system=this.game.vehicles;
  // A dying walker stays (its record says so); its crew pose has nothing to say.
  for(const vehicle of system?.vehicles||[]){if(!vehicle.alive&&!(vehicle.dying>0))continue;const record=captureEntityVehicle(vehicle,{tick,classId:required(this.classes,vehicle.name)});live.set(record.id,record);records.push(record);if(vehicle.alive)records.push(captureEntityMount(vehicle,{tick,slot:system.slots?.indexOf(vehicle.slot)??-1}));}
  for(const turret of this.game.turretSeats?.turrets||[]){const record=captureEntityTurret(turret,{tick,building:this.game.live?.buildings?.indexOf(turret)??-1});if(record)records.push(record);}
  for(const[id,old]of this.known)if(!live.has(id)){
   const tombstone=old.kind==='tombstone'?old:{kind:'tombstone',id,generation:old.generation,tick};records.push(tombstone);live.set(id,tombstone);
  }
  if(live.size>this.maxEntities)throw Error('semantic fixed-roster capacity exceeded');
  this.known=live;for(const id of this.seats.keys())if(!live.has(id)||live.get(id).kind==='tombstone')this.seats.delete(id);return records;
 }
}
// snapshot-records.js
const angleCode = value => Math.max(-32767, Math.min(32767, Math.round(value * 32767 / Math.PI)));
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
// Whole-entry reuse also needs a clear disc on both sides: the previous
// entry's disc described its own arrival and must not stick to this snapshot.
const reusableEntry = (built, before) => built.same && !built.entry.disc && !before.disc;
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
// entity-world-state.js
const encode=new TextEncoder(),decode=new TextDecoder('utf-8',{fatal:true});
function make(id,generation,tick,schema,value){const payload=encode.encode(JSON.stringify(value));if(payload.length>900)throw Error('World presentation record too large');return {kind:'summary',id,generation,tick,schema,payload};}
export function captureWorldPresentation(game,tick,epoch){
  const records=[];
  for(const [index,b]of (game.live?.buildings||[]).entries())if(!(game.turretSeats?.turrets||[]).includes(b))records.push(make(200000+index,epoch,tick,7,[index,b.state,Number.isFinite(b.health)?b.health:null,b.team||0,b.mode??null,b.active||0,b.pulse||0,b.facing||0,b.yaw||0,b.servedHealth||false,b.servedAmmo||false]));
  for(const [index,prop]of (game.live?.animated||[]).entries())if(prop.grabs?.length&&prop.slot>=0)records.push(make(CREATURE_STATE_ID+index,epoch,tick,CREATURE_STATE_SCHEMA,[index,...prop.state()]));
  for(const [index,trap]of (game.live?.traps||[]).entries())records.push(make(TRAP_STATE_ID+index,epoch,tick,TRAP_STATE_SCHEMA,[index,...trap.stateRecord()]));
  for(const v of game.vehicles?.vehicles||[])if(v.alive){const target=game.vehicles.vehicles[v.towTarget];records.push(make(300000+v.aiTarget.id,v.aiTarget.generation,tick,8,[v.aiTarget.id,target?.aiTarget.id||0,target?.aiTarget.generation||0,v.towLocal?.toArray()||[0,0,0],v.towLeg||'',v.towAngle||0,v.towWinding||0]));}
  return records;
}
// snapshot-host.js
// The common world fields (battle, posts, roster, records) are captured only
// when one of them is due, and the roster (refreshed every 60 ticks, the
// costly one) only when one of its own chunks is due or its chunk set
// changed; the records (every 300 ticks) likewise when one of theirs is
// due. The presentation schemas are captured every snapshot. A captured
// schema is complete, so a field it no longer produces is removed.
const COMMON_WORLD_SCHEMAS = new Set([1, 2, 4, 5]), BATTLE_COMPLETE_SCHEMAS = [1, 2];
const ROSTER_WORLD_SCHEMAS = new Set([4]);
const RECORD_WORLD_SCHEMAS = new Set([5]);
const PRESENTATION_WORLD_SCHEMAS = [7, 8, 9, 10];
// The players attached, by Character, with the link the session reports
// (network-entity-summary.js captureWorldSummaries builds the same map).
function attachedPlayers(world) {
  const players = new Map();
  for (const p of world.players?.values?.() || []) if (p.character) players.set(p.character, { link: p.link || null });
  return players;
}
// captureSnapshot as the host ran it, over the state it closed over (the
// host's ring and encode cache aside). fireCounter is the host's
// EntityShotCounters.counter.
export function referenceCapture({ game, world, registry, epoch = 1, fireCounter }) {
  const capture = new EntityWorldCapture(game, registry, { fireCounter });
  const slots = new SnapshotSlotTable(), stagger = new WorldFieldStagger();
  let lastSnapshot = null, rosterLength = -1;
  function captureSnapshot(tick) {
    const characters = game.battle.characters;
    for (const [slot, c] of characters.entries()) c.rosterSlot = slot;
    const records = capture.capture(tick);
    // The fields captureWorldSummaries produces for everyone. Skipping the
    // roster when none of its chunks is due and its chunk count (set by the
    // character count) is unchanged publishes exactly what capturing it would.
    const captureBattle = stagger.anyDue(tick, COMMON_WORLD_SCHEMAS);
    const captureRoster = captureBattle && (characters.length !== rosterLength || stagger.schemaDue(tick, ROSTER_WORLD_SCHEMAS));
    const captureRecords = captureBattle && stagger.schemaDue(tick, RECORD_WORLD_SCHEMAS);
    const complete = new Set(PRESENTATION_WORLD_SCHEMAS), common = [];
    if (captureBattle) {
      common.push(captureBattleSummary(game.battle, { tick, epoch }), ...game.posts.posts.map(post => capturePostSummary(post, { tick, epoch, registry })));
      for (const schema of BATTLE_COMPLETE_SCHEMAS) complete.add(schema);
    }
    if (captureRoster) {
      common.push(...captureRosterSummaries(game.battle, attachedPlayers(world), { tick, epoch }));
      complete.add(4); rosterLength = characters.length;
    }
    // Everyone's records (battle-stats.js), for DETAILS from Tab's table in
    // whole seconds, and exact once the scores stop (they no longer
    // change) for the end table's DETAILS and AWARDS.
    if (captureRecords) {
      common.push(...captureRecordSummaries(game.battle, registry, { tick, epoch, quantum: game.battle.gameOver ? 1 : 1000 }));
      complete.add(5);
    }
    const worldRecords = stagger.update(tick, [
      ...common,
      ...[...game.units.all()].filter(u => u.soldier && u.combat && !u.removed && stagger.isDue(tick, `6:${UNIT_STATE_ID + u.id}`)).map(u => captureUnitState(u, registry, tick)),
      ...captureWorldPresentation(game, tick, epoch),
    ], complete);
    const snapshot = snapshotFromRecords({ tick, epoch, records, worldRecords, slots, previous: lastSnapshot });
    lastSnapshot = snapshot;
    return snapshot;
  }
  return captureSnapshot;
}
