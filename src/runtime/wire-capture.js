// Read live objects in capture order, then hand owned values to the native
// publication state. Identity tokens name objects without retaining them.
import { bridgeFor } from '../sim/runtime.js';
import { WcResultMirror, putWcCall, syncWcResult } from '../sim/gen/index.js';

const output = { Mirror: WcResultMirror, sync: syncWcResult };
const reverse = map => { const out = new Map(); for (const [id, name] of map) { if (out.has(name)) throw Error('ambiguous entity registry'); out.set(name, id); } return out; };
const required = (map, name) => { const id = map.get(name); if (id === undefined) throw Error(`entity registry missing ${name}`); return id; };
const vector = p => [p.x, p.y, p.z];
function semantic(value) {
  const r = value.record, base = { kind: r.kind, id: r.id, generation: r.generation, tick: value.tick };
  if (r.kind === 'tombstone') return base;
  if (r.kind === 'seat') return { ...base, vehicleId: r.vehicleId, vehicleGeneration: value.vehicleGeneration, seatIndex: r.seatIndex, leaseVersion: value.leaseVersion };
  if (r.kind === 'mount') return { ...base, slot: r.slot, tilt: r.tilt, roll: r.roll, walkPhase: r.walkPhase, walkBlend: r.walkBlend, seats: r.seats.map(s => ({ yaw: s.yaw, pitch: s.pitch })) };
  if (r.kind === 'turret') return { ...base, building: r.building, state: r.state, team: r.team, health: r.health, yaw: r.yaw, pitch: r.pitch, gunYaw: r.gunYaw, gunPitch: r.gunPitch };
  if (r.kind === 'item' || r.kind === 'remote') {
    if (!r.active) return { ...base, active: false };
    const active = { ...base, active: true, definitionId: r.definitionId, position: r.position, velocity: r.velocity, yaw: r.yaw };
    return r.kind === 'item' ? { ...active, up: r.up } : { ...active, team: r.team, pitch: r.pitch, health: r.health };
  }
  const common = { ...base, classId: r.classId, team: r.team, position: r.position, velocity: r.velocity, health: r.health, state: r.state };
  if (r.kind === 'vehicle') return { ...common, quaternion: r.quaternion, power: r.power };
  const body = { ...common, yaw: r.yaw, pitch: r.pitch, posture: r.posture, weapon: r.weapon, fireCounter: r.fireCounter, charge: r.charge };
  return r.kind === 'soldier' ? body : { ...body, mode: r.mode, reverse: r.reverse, dead: r.dead, shieldOn: r.shieldOn, shield: r.shield, time: r.time, roll: r.roll, lean: r.lean, legs: r.legs };
}

export class WireEntityCapture {
  constructor(game, registry, { instance, bridge = instance ? bridgeFor(instance) : null, maxEntities = 256, fireCounter = () => 0 } = {}) {
    this.game = game; this.registry = registry; this.bridge = bridge; this.fireCounter = fireCounter;
    this.classes = reverse(registry.classes); this.weapons = reverse(registry.weapons); this.unsupported = [];
    const capacity = game.items ? (game.items.pools?.mine ?? 50) + (game.items.pools?.powerupitem ?? 25) : 0;
    if (game.items && (!Number.isSafeInteger(capacity) || capacity < 1 || capacity > 256)) throw Error('unsupported item pool capacity');
    this.handle = bridge.x.wire_capture_source_create(capacity, maxEntities);
    this.input = { words: bridge.x.wire_capture_source_words(), put: putWcCall };
    this.tokens = new WeakMap(); this.nextToken = 1;
    this.items = game.items ? { capture: tick => this.captureItems(tick).map(semantic) } : null;
    this.remotes = { capture: tick => this.captureRemotes(tick).map(semantic) };
  }
  run(op, args = {}) {
    if (!this.handle) throw Error('entity capture disposed');
    this.bridge.callArgs(this.input, { op, tick: 0, capacity: 0, itemCapacity: 0, token: 0, live: [], record: null, records: [],
      air: '', posture: '', dive: 0, dead: false, walker: false, charging: false, held: 0, maxPressedTime: 0,
      dying: 0, flightState: '', mounted: false, vehicleId: 0, vehicleGeneration: 0, seatIndex: 0, ...args });
    const out = this.bridge.callResult(output, this.bridge.x.wire_capture_source_run(this.handle));
    if (out.error !== null) throw Error(out.error);
    this.leaseSerial = out.leaseSerial;
    return out;
  }
  token(object) { let id = this.tokens.get(object); if (id === undefined) { id = this.nextToken++; this.tokens.set(object, id); } return id; }
  captureItems(tick, nativeOnly = false) {
    const items = this.game.items;
    if (!items) return [];
    const live = new Set(items.items.filter(item => !item.gone));
    this.run(6, { tick, live: [...live].map(item => this.token(item)) });
    for (const item of live) {
      const raw = { kind: 'item', position: vector(item.position), velocity: vector(item.velocity), yaw: item.yaw, up: vector(item.up) };
      const pending = this.run(7, { token: this.token(item), record: raw });
      if (!pending.changed) continue;
      // Definitions are looked up only for a changed sample, preserving the
      // old raw-value shortcut and its timing if content is incomplete.
      const record = { ...raw, definitionId: this.registry.combat.definitionId(item.def), position: item.position.toArray(), velocity: item.velocity.toArray(), up: item.up.toArray() };
      this.run(8, { tick, token: pending.slot, record });
    }
    return nativeOnly ? null : this.run(9).records;
  }
  captureRemotes(tick, nativeOnly = false) {
    const world = this.game.networkRemotes;
    if (!world) return [];
    for (const { droid } of world.droids()) if (!droid.dead) world.identityFor(droid);
    for (let id = 0; id < world.slots.length; id++) {
      const { droid, generation } = world.slots[id];
      if (!droid) continue;
      const record = droid.dead ? { kind: 'remote', id, generation } : { kind: 'remote', id, generation,
        definitionId: this.registry.combat.definitionId(droid.def), team: droid.team, position: droid.position.toArray(), velocity: droid.velocity.toArray(), yaw: droid.yaw, pitch: droid.pitch, health: droid.health };
      this.run(10, { tick, record, dead: !!droid.dead });
    }
    return nativeOnly ? null : this.run(11).records;
  }
  capture(tick, nativeOnly = false) {
    if (nativeOnly) { this.captureItems(tick, true); this.captureRemotes(tick, true); this.run(13); }
    else { const records = [...this.captureItems(tick), ...this.captureRemotes(tick)]; this.run(0, { records }); }
    this.unsupported = [];
    for (const unit of this.game.units.all()) {
      if (!unit.soldier || !unit.combat || unit.removed) continue;
      const weapon = unit.combat.arsenal.current(0), classId = required(this.classes, unit.className), weaponId = weapon ? required(this.weapons, weapon.name) : 0;
      const fireCounter = this.fireCounter(unit), s = unit.soldier;
      const charge = unit.dead ? null : unit.combat.arsenal?.current(0), charging = !!charge?.charging;
      const maxPressedTime = charging ? charge.levels?.[0].MaxPressedTime || 0 : 0;
      const lease = unit.seat, mount = (lease?.vehicle || lease?.turret)?.aiTarget || null;
      this.run(1, { tick, dead: !!unit.dead, dive: s.dive, air: s.air || '', posture: s.posture, walker: !!unit.walker,
        charging, maxPressedTime, held: maxPressedTime > 0 ? charge.held : 0,
        mounted: !!mount, vehicleId: mount?.id || 0, vehicleGeneration: mount?.generation || 0, seatIndex: lease?.index || 0,
        record: { kind: 'soldier', id: unit.id, generation: unit.generation, classId, team: unit.team,
          position: vector(s.position), velocity: vector(s.velocity), yaw: s.aimYaw, pitch: s.aimPitch, health: unit.combat.health,
          weapon: weaponId, fireCounter: fireCounter === undefined ? 0 : fireCounter,
          mode: s.mode, reverse: s.reverse, dead: s.dead, shieldOn: !!s.shield?.on, shield: s.shield?.value || 0,
          time: s.time, roll: s.roll, lean: s.lean, legs: s.legs } });
    }
    const system = this.game.vehicles;
    for (const vehicle of system?.vehicles || []) {
      if (!vehicle.alive && !(vehicle.dying > 0)) continue;
      const classId = required(this.classes, vehicle.name);
      this.run(2, { tick, dying: vehicle.dying, flightState: vehicle.flightState || '', record: {
        kind: 'vehicle', id: vehicle.aiTarget.id, generation: vehicle.aiTarget.generation, classId, team: vehicle.team,
        position: vector(vehicle.position), velocity: vector(vehicle.velocity), quaternion: vehicle.quaternion.toArray(), health: vehicle.health, power: vehicle.power } });
      if (vehicle.alive) this.run(3, { tick, record: { kind: 'mount', id: vehicle.aiTarget.id, generation: vehicle.aiTarget.generation,
        slot: system.slots?.indexOf(vehicle.slot) ?? -1, tilt: vehicle.tilt, roll: vehicle.roll, walkPhase: vehicle.walkPhase, walkBlend: vehicle.walkBlend,
        seats: (vehicle.seats || []).map(seat => ({ yaw: seat.aimYaw, pitch: seat.pitch })) } });
    }
    for (const turret of this.game.turretSeats?.turrets || []) {
      if (!turret.aiTarget) continue;
      this.run(4, { tick, record: { kind: 'turret', id: turret.aiTarget.id, generation: turret.aiTarget.generation,
        building: this.game.live?.buildings?.indexOf(turret) ?? -1, state: turret.state, team: turret.aiTarget.team,
        health: turret.health, yaw: turret.yaw, pitch: turret.pitch, gunYaw: turret.gunYaw, gunPitch: turret.gunPitch } });
    }
    const result = this.run(nativeOnly ? 12 : 5, { tick });
    return nativeOnly ? undefined : result.records.map(semantic);
  }
  captureNative(tick) { this.capture(tick, true); }
  get nativeHandle() { if (!this.handle) throw Error('entity capture disposed'); return this.handle; }
  dispose() { if (this.handle) { this.bridge.x.wire_capture_source_drop(this.handle); this.handle = 0; } }
}
