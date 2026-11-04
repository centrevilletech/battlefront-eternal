// Vehicles' native seat phase. These facades only publish canonical lease and
// body rows; AI intents remain foreign input, copied once before the latch.
import * as THREE from 'three';
import { SeatLease, seatWeaponPolicy } from './seats.js';
import { Building } from './props.js';
import { MirrorPool, Heap } from './sim/runtime.js';
import { makeHit } from './collision.js';
import { COMMAND_F2_SEAT, COMMAND_TABLE, KIND_TABLE, KIND_SEAT_LEASE, KIND_UNIT, KIND_ORDNANCE, KIND_EMITTER } from './sim/gen/index.js';
import { F2SeatIoMirror, F2SeatHeaderMirror, F2SeatUnitInMirror, F2SeatUnitOutMirror, F2SeatCommandMirror, F2SeatResultMirror,
  F2SeatAnswerMirror, F2SeatEffectMirror, F2LatchedMirror, 
  F2TurretOutMirror, F2TurretEventMirror, F2TurretHeaderMirror, F2HullNodeMirror, syncF2HullNode, F2TurretTargetMirror, syncF2TurretHeader, putF2TurretBirth, putF2TurretLife, syncF2TurretOut, syncF2TurretEvent,
  syncF2SeatIo, putF2SeatHeader, putF2SeatUnitIn, syncF2SeatUnitOut, syncF2SeatEffect, syncF2Latched, putF2SeatSample } from './sim/gen/f2.js';
import { rigNodes } from './vehicle-phase-data.js';
import { turretSettings, turretState } from './turrets-sim.js';
import { registerWeaponClass } from './weapon-content-sim.js';
import { hasMissileLock, seatLockTarget } from './weapon-targeting.js';
import { weaponAI, categoryOf } from './ai/classic/weapons-ai.js';
import { legacyRandom } from './ai/random.js';
import { putSeatHullFacts, putSeatAiControl } from './sim/gen/seats.js';

const WEAPON_STATE = ['level', 'deployed', 'firing', 'timer', 'bottomless', 'perShot', 'clip', 'reserve', 'heat', 'overheated',
  'floor', 'kick', 'spreadScale', 'sniperScale', 'yawSpreadOverride', 'wait', 'reloadLeft', 'throwTime', 'released', 'hidden',
  'charging', 'held', 'charge', 'limit', 'pattern', 'salvosLeft', 'salvoWait', 'salvoSeed', 'salvoCount', 'lockTargetId', 'lockGeneration', 'lockElapsed', 'lockComplete'];

class LeasePool extends MirrorPool {
  constructor(owner) {
    super(KIND_SEAT_LEASE);
    this.Mirror = class extends SeatLease {
      constructor() {
        super();

      }
    };
  }
  create(slot, generation) {
    const lease = super.at(slot, generation);
    if (!this.owner.emitterIds.has(lease.emitter)) this.owner.emitterIds.set(lease.emitter, { slot, generation: generation + this.owner.io.emitterMask });
    return lease;
  }
  retire(slot, generation) {
    const lease = super.retire(slot, generation);
    let lives = this.released.get(slot); if (!lives) this.released.set(slot, lives = new Map());
    lives.set(generation, lease); return lease;
  }
  at(slot, generation) {
    const lease = this.objects[slot];
    if (!lease || lease.simGen !== generation) {
      const old = this.released.get(slot)?.get(generation); if (old) return old;
      throw Error('stale F2 seat lease');
    }
    return lease;
  }
  is(value) { return value instanceof SeatLease; }
}

// Preserve the old occupancy when a beam's emitter is mirrored after release.
class LeaseEmitters {
  constructor(owner, foreign) { this.owner = owner; this.foreign = foreign; this.objects = foreign.objects; }
  is(value) { return this.owner.emitterIds.has(value) || this.foreign.is(value); }
  slotOf(value) { return this.owner.emitterIds.get(value)?.slot ?? this.foreign.slotOf(value); }
  put(value, f, at) {
    const ref = this.owner.emitterIds.get(value);
    if (ref) { f[at] = ref.slot; f[at + 1] = ref.generation; } else this.foreign.put(value, f, at);
  }
  at(slot, generation) {
    const mask = this.owner.io.emitterMask;
    return generation >= mask ? this.owner.pool.at(slot, generation - mask).emitter : this.foreign.at(slot, generation);
  }
}

export class F2Seats {
  constructor(phase) {
    this.phase = phase; this.bridge = phase.bridge; this.x = phase.x; this.vehicles = phase.vehicles;
    this.units = []; this.seen = []; this.stamp = 0; this.emitterIds = new WeakMap();
    this.leaseRows = new WeakMap();
    this.pool = new LeasePool(this); this.pool.owner = this; this.pool.released = new Map(); this.mounts = new WeakMap(); this.riderClips = new WeakMap(); this.riderSample = { clip: null, pitch: 0, turn: 0, time: null }; this.bridge.refs[KIND_SEAT_LEASE] = this.pool;
    this.emptyHeap = new Heap(); this.header = new F2SeatHeaderMirror(); this.io = new F2SeatIoMirror();
    this.unitIn = new F2SeatUnitInMirror(); this.unitOut = new F2SeatUnitOutMirror();
    this.controlIn = { seatUnit: null, driven: false, intent: null };
    this.command = new F2SeatCommandMirror(); this.result = new F2SeatResultMirror();
    this.answer = new F2SeatAnswerMirror(); this.effectRow = new F2SeatEffectMirror(); this.controlRow = new F2LatchedMirror();
    this.lookSamples = [[0, 0]]; this.networkScales = { scale: 1, invert: false, flightScale: 1, flightInvert: false };
    this.sample = { dx: 0, dy: 0 }; this.drop = new THREE.Vector3(); this.hit = makeHit();
    this.castResult = { found: false, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
    this.flag = { value: false }; this.drawAnswer = { value: 0 };
    this.ioBase = this.x.f2_seats_init() / 8; this.bridge.views(); this.readIo();
    this.inputBase = this.io.input / 8; this.capacity = this.io.capacity;
    this.bridge.refs[KIND_EMITTER] = new LeaseEmitters(this, this.bridge.refs[KIND_EMITTER]);
    this.header.mouseScale = this.header.flightScale = 1;
    this.header.invert = this.header.flightInvert = false;
    this.hullArgs = { words: this.x.f2_seats_adopt_words(), put: putSeatHullFacts };
    this.facts = new WeakMap(); this.turretRows = new WeakMap();
    this.hullNode = new F2HullNodeMirror(); this.turretHeader = new F2TurretHeaderMirror(); this.turretEvent = new F2TurretEventMirror();
    this.turretLifeRow = { turret: null, generation: 0, alive: false, attacker: null, center: new THREE.Vector3(), radius: 0 };
    this.turretBirthArgs = { words: this.x.f2_turret_words(0), put: putF2TurretBirth };
    this.turretLifeArgs = { words: this.x.f2_turret_words(1), put: putF2TurretLife };
    this.shot = { weapon: null, yaw: 0, pitch: 0, charge: 0, targetId: -1, targetGeneration: -1 };
    this.targetRow = new F2TurretTargetMirror(); this.targetCategories = new WeakMap(); this.noEnter = new WeakMap();
    this.legacySpread = legacyRandom('weapon-spread');
    const vs = this.vehicles, b = this.bridge;
    b.register({
      f2SeatAction: a => this.action(a),
      f2TurretDraw: a => { const w = this.crews.turrets[a.turret].weapon; this.drawAnswer.value = w.random ? w.random() : this.legacySpread(); return this.drawAnswer; },
      f2TurretTarget: a => this.target(a),
      seatCast: a => {
        const hit = vs.seatsWorld.cast(a.origin, a.direction, a.far, this.hit, a.ignore), out = this.castResult;
        out.found = Boolean(hit); out.distance = hit?.distance ?? 0; out.terrain = Boolean(hit?.terrain); out.body = hit?.body ?? null;
        if (hit) { out.point.copy(hit.point); out.normal.copy(hit.normal); }
        return out;
      },
      seatOverlaps: a => { this.flag.value = vs.seatsWorld.overlapsSphere(a.center, a.radius, a.ignore); return this.flag; },
    });
    // These are authoritative writes, not method adapters. The phase itself
    // calls none of them; its latch and crew work run inside f2_step.
    vs.board = (u, v, index = -1) => this.invoke('board', u, v, null, index).accepted;
    vs.requestExit = (u, options) => this.invoke('requestExit', u, u?.seat?.vehicle, null, 0, options).accepted;
    vs.unboard = (u, options) => this.invoke('unboard', u, u?.seat?.vehicle, null, 0, options).accepted;
    vs.vacate = (lease, options) => this.invoke('vacate', lease?.unit, lease?.vehicle, lease, 0, options).accepted;
    vs.move = (lease, index) => this.invoke('move', lease?.unit, lease?.vehicle, lease, index).lease;
    vs.displace = lease => this.invoke('displace', lease?.unit, lease?.vehicle, lease).lease;
    vs.promote = (v, index) => this.invoke('promote', null, v, null, index).lease;
    vs.destroyedCrew = v => this.invoke('destroyed', null, v).accepted;
  }
  readIo() { this.bridge.views(); syncF2SeatIo(this.bridge, this.bridge.f, this.ioBase, 0, this.io); this.phase.counters.outputFields += this.io.words; }

  // Vehicle construction supplies class facts once. Mutable state stays native.
  adopt(v) {
    let facts = this.facts.get(v);
    if (!facts) {
      facts = { vehicle: v, index: this.vehicles.vehicles.indexOf(v), hover: v.base === 'hover', animal: v.healthType === 'animal', attacker: 0, attackerId: 0,
        turretNodes: v.seats.map(s => Boolean(s.settings?.TurretNodeName)), weapons: v.seats.map(s => s.weapons.length) };
      this.facts.set(v, facts);
    }
    this.bridge.callArgs(this.hullArgs, facts); this.x.f2_seats_adopt();
  }
  add(unit) {
    if (!unit?.soldier || unit.walker || this.seen[unit.id] === this.stamp) return;
    this.seen[unit.id] = this.stamp; this.units.push(unit);
  }
  addHull(v) { if (v) for (const seat of v.seats) this.add(seat.lease?.unit); }

  // Foreign facts are fixed-width generated rows written into reserved memory.
  // A command imports its affected riders; a phase imports the active crew once.
  prepare(unit = null, hull = null, all = true) {
    this.units.length = 0; this.stamp++;
    this.add(unit);
    if (all) {
      for (const v of this.vehicles.vehicles) this.addHull(v);
      if (this.crews) for (const t of this.crews.turrets) this.add(t.lease?.unit);
    }
    else if (hull?.seats) this.addHull(hull); else this.add(hull?.lease?.unit);
    const b = this.bridge, h = this.header; b.views(); const f = b.f;
    let o = this.io.headerWords;
    h.units = o; h.unitCount = this.units.length;
    for (const unit of this.units) {
      const a = this.unitIn;
      a.unit = unit; a.generation = unit.generation; a.alive = unit.alive; a.dead = unit.dead;
      a.human = unit.human; a.networkHuman = unit.networkHuman; a.walker = unit.walker; a.team = unit.team;
      a.removed = unit.removed; a.hasActor = Boolean(unit.actor); a.tickCount = unit.tickCount;
      a.soldier = unit.soldier; a.intent = unit.intent;
      let noEnter = this.noEnter.get(unit.classDef);
      if (noEnter === undefined) { noEnter = Number.parseFloat(unit.classDef?.NoEnterVehicles ?? 0); if (unit.classDef) this.noEnter.set(unit.classDef, noEnter); }
      a.noEnter = noEnter;
      a.zoomStep = unit.combat?.zoomStep ?? 0; a.zoom = unit.combat?.zoom ?? 1;
      a.firstPerson = Boolean(unit.combat?.firstPerson); a.blowApart = Boolean(unit.combat?.blowApart); a.combatDead = Boolean(unit.combat?.dead);
      a.targetable = unit.targetable; a.mounted = unit.combat?.mounted; a.gunUnderwater = Boolean(unit.combat?.gunUnderwater); a.aimOverride = unit.combat?.aimOverride;
      putF2SeatUnitIn(b, a, f, this.inputBase + o, this.emptyHeap); o += this.io.unitWords;
    }
    h.controls = o; h.controlCount = 0;
    for (const unit of this.units) {
      const c = unit.seat?.controllable;
      if (!c) continue;
      const a = this.controlIn; a.seatUnit = c; a.driven = Boolean(c.controller); a.intent = c.intent;
      putSeatAiControl(b, a, f, this.inputBase + o, this.emptyHeap); o += this.io.controlWords; h.controlCount++;
    }
    h.looks = o; h.lookCount = 0;
    this.writeHeader(o);
    return o;
  }
  writeHeader(end, words = end) {
    if (end > this.capacity) throw Error('F2 seat input capacity exceeded');
    putF2SeatHeader(this.bridge, this.header, this.bridge.f, this.inputBase, this.emptyHeap);
    this.phase.counters.inputFields += words;
  }
  invoke(op, unit = null, hull = null, lease = null, index = 0, options = null, direction = 1, looks = null, scales = null) {
    this.phase.riders.ensure(unit, hull);
    const look = op === 'look' || op === 'network' || op === 'resetSticks';
    let o = look ? this.prepare(null, null, false) : this.prepare(unit, hull, false);
    const h = this.header;
    if (looks) {
      h.mouseScale = scales.scale; h.invert = scales.invert; h.flightScale = scales.flightScale; h.flightInvert = scales.flightInvert;
      for (let i = 0; i < looks.length; i++) {
        this.sample.dx = looks[i][0]; this.sample.dy = looks[i][1];
        putF2SeatSample(this.bridge, this.sample, this.bridge.f, this.inputBase + o, this.emptyHeap); o += this.io.lookWords;
      }
      h.lookCount = looks.length; this.writeHeader(o, this.io.headerWords + looks.length * this.io.lookWords);
    }
    const a = this.command;
    a.op = op; a.unit = unit; a.unitLife = unit?.generation ?? 0;
    a.hull = hull?.seats ? hull : null; a.hullLife = hull?.aiTarget?.generation ?? 0; a.lease = lease; a.index = index;
    a.placement = options?.placement ?? true; a.promote = options?.promote; a.post = options?.post ?? null;
    a.safe = options?.policy === 'safe'; a.force = Boolean(options?.force); a.reason = options?.reason ?? 'exit'; a.direction = direction;
    const b = this.bridge, prior = b.x; b.x = this.x;
    try { b.command(COMMAND_F2_SEAT, a, this.result); this.phase.counters.outputFields += COMMAND_TABLE[COMMAND_F2_SEAT].result.words; } finally { b.x = prior; }
    this.phase.sync();
    return this.result;
  }
  resetSticks(unit) { return this.invoke('resetSticks', unit, unit.seat?.vehicle, unit.seat).accepted; }
  cycle(unit, direction = 1) { return this.invoke('cycle', unit, unit?.seat?.vehicle, unit?.seat, 0, null, direction).accepted; }
  network(vehicle, index, controls, command) {
    const fs = command.flightSettings;
    const scales = this.networkScales;
    scales.scale = Math.min(2, command.mouseScale ?? 1); scales.invert = Boolean(command.invertMouse);
    scales.flightScale = Math.max(.05, Math.min(10, fs?.sensitivity ?? 1)); scales.flightInvert = Boolean(fs?.invert);
    this.invoke('network', vehicle.seats[index]?.lease?.unit, vehicle, vehicle.seats[index]?.lease, index, null, 1, command.look, scales);
  }
  look(vehicle, index, dx, dy, scales) {
    const samples = this.lookSamples;
    samples[0][0] = dx; samples[0][1] = dy;
    this.invoke('look', vehicle.seats[index]?.lease?.unit, vehicle, vehicle.seats[index]?.lease, index, null, 1, samples, scales);
  }

  captureLease(lease) {
    let row = this.leaseRows.get(lease);
    if (!row) {
      row = { input: new Float64Array(KIND_TABLE[KIND_SEAT_LEASE].importStride), saved: new Float64Array(KIND_TABLE[KIND_SEAT_LEASE].importStride), heap: new Heap(), savedHeap: [] };
      this.leaseRows.set(lease, row);
    }
    row.heap.clear(); KIND_TABLE[KIND_SEAT_LEASE].put(this.bridge, lease, row.input, 0, row.heap); row.saved.set(row.input);
    row.savedHeap.length = row.heap.words.length;
    for (let i = 0; i < row.heap.words.length; i++) row.savedHeap[i] = row.heap.words[i];
  }
  check() {
    if (!this.phase.debug) return;
    for (const lease of this.pool.objects) {
      if (!lease) continue;
      const row = this.leaseRows.get(lease); if (!row) continue;
      row.heap.clear(); KIND_TABLE[KIND_SEAT_LEASE].put(this.bridge, lease, row.input, 0, row.heap);
      for (let i = 0; i < row.saved.length; i++) if (!Object.is(row.saved[i], row.input[i])) throw Error('F2 lease mirror write');
      if (row.savedHeap.length !== row.heap.words.length) throw Error('F2 lease mirror write');
      for (let i = 0; i < row.savedHeap.length; i++) if (!Object.is(row.savedHeap[i], row.heap.words[i])) throw Error('F2 lease mirror write');
    }
  }
  // Lease rows are resolved before hull and unit rows can refer to a new lease.
  syncLeases() {
    this.readIo(); const b = this.bridge, f = b.f, base = this.io.leases / 8;
    if (!base) return;
    const n = f[base], stride = f[base + 1], heap = base + f[base + 2], retired = f[base + 4];
    for (let i = 0; i < n; i++) {
      const at = base + 5 + i * stride;
      const lease = i < retired ? this.pool.retire(f[at], f[at + 1]) : this.pool.create(f[at], f[at + 1]);
      KIND_TABLE[KIND_SEAT_LEASE].sync(b, f, at, heap, lease);
      if (this.phase.debug) this.captureLease(lease);
      if (lease.turret && this.crews) {
        if (!this.riderClips.has(lease)) {
          const pose = lease.turret.riderSample?.(lease.unit?.animator?.clips, this.riderSample);
          this.riderClips.set(lease, pose?.clip ?? null);
        }
        const list = this.crews.leases, index = list.indexOf(lease);
        if (lease.released && index >= 0) list.splice(index, 1);
        else if (!lease.released && index < 0) list.push(lease);
      }
    }
    this.phase.counters.outputFields += n * stride + f[base + 3];
  }
  syncNodes() {
    const b = this.bridge, f = b.f, base = this.io.nodes / 8;
    if (!base) return;
    const n = f[base], stride = f[base + 1], heap = base + f[base + 2];
    for (let i = 0; i < n; i++) {
      const row = syncF2HullNode(b, f, base + 5 + i * stride, heap, this.hullNode), node = rigNodes(row.hull)[row.node];
      node.matrix.copy(row.matrix); node.matrixWorld.copy(row.world); node.matrixWorldNeedsUpdate = false;
    }
    this.phase.counters.outputFields += n * stride + f[base + 3];
  }
  syncBodies() {
    const b = this.bridge, f = b.f;
    this.phase.riders.sync(this.io.riders);
    let base = this.io.units / 8;
    if (base) {
      const n = f[base], stride = f[base + 1], heap = base + f[base + 2];
      for (let i = 0; i < n; i++) {
        const at = base + 5 + i * stride, unit = b.refs[KIND_UNIT].at(f[at], f[at + 1]), out = this.unitOut;
        out.soldier = unit.soldier; out.intent = unit.intent;
        syncF2SeatUnitOut(b, f, at, heap, out);
        if (unit.seat?.turret) this.mounts.set(unit, unit.seat.turret);
        unit.seat = out.seat; unit.targetable = out.targetable; unit.tickCount = out.tickCount;
        if (unit.combat) { unit.combat.mounted = out.mounted; unit.combat.gunUnderwater = out.gunUnderwater; unit.combat.aimOverride = out.aimOverride; }
        if (out.riderPosed) this.phase.riders.present(unit);
      }
      this.phase.counters.outputFields += n * stride + f[base + 3];
    }
    base = this.io.controls / 8;
    if (base) {
      const n = f[base], stride = f[base + 1], heap = base + f[base + 2];
      for (let i = 0; i < n; i++) {
        const row = syncF2Latched(b, f, base + 5 + i * stride, heap, this.controlRow), intent = row.control.intent;
        intent.reload = intent.use = intent.exit = intent.jump = intent.crouch = intent.prone = false;
        intent.nextPrimary = intent.previousPrimary = intent.nextSecondary = intent.zoom = false;
      }
      this.phase.counters.outputFields += n * stride + f[base + 3];
    }
    base = this.io.effects / 8;
    if (base) {
      const n = f[base], stride = f[base + 1], heap = base + f[base + 2];
      for (let i = 0; i < n; i++) this.effect(syncF2SeatEffect(b, f, base + 5 + i * stride, heap, this.effectRow));
      this.phase.counters.outputFields += n * stride + f[base + 3];
    }
    this.syncTurrets();
    this.vehicles.ticks = this.io.ticks;
  }
  effect(e) {
    const u = e.unit;
    if (e.kind === 'seated') { if (u.combat) u.combat.zoomed = false; }
    else if (e.kind === 'unseated') {
      if (u.combat) u.combat.zoomed = false;
      u.animator?.gunner?.(null); u.actor?.root?.rotation?.set?.(0, e.yaw, 0);
      if (this.mounts.get(u)?.hidesGunner && u.actor?.root && !u.removed && !u.combat?.blowApart) u.actor.root.visible = true;
    } else if (e.kind === 'placed') {
      u.presentation?.reset?.(); u.animator?.gunner?.(null); u.animator?.reset?.(e.yaw);
      if (u.actor?.root) { u.actor.root.visible = true; u.actor.root.rotation?.set?.(0, e.yaw, 0); }
    } else if (e.kind === 'dropped') { u.presentation?.reset?.(); u.actor?.root?.position?.copy?.(e.point); }
  }
  // lockPick supplies foreign target facts; cone selection and line of sight
  // run natively without constructing per-candidate option objects.
  target(a) {
    const out = this.targetRow, units = this.crews.ordnance?.units, viewer = this.crews.turrets[a.turret].aiTarget;
    out.mode = 'unavailable'; out.alive = out.eligible = false; out.hull = out.body = null;
    if (!units) return out;
    let unit;
    if (a.index < 0) {
      unit = seatLockTarget(a.lease);
      if (unit === undefined) { out.mode = 'human'; return out; }
      if (!unit) { out.mode = 'none'; return out; }
    } else unit = units.all()[a.index];
    out.mode = 'unit'; out.id = unit.id; out.generation = unit.generation ?? 0; out.alive = Boolean(unit.alive);
    out.eligible = unit !== viewer && out.alive && !unit.seat && (unit.health ?? unit.combat?.health ?? 1) > 0 && unit.kind !== 10;
    let category = unit.category;
    if (!Number.isInteger(category)) {
      category = this.targetCategories.get(unit);
      if (category === undefined) { category = categoryOf(unit); this.targetCategories.set(unit, category); }
    }
    out.category = category; out.relation = units.relation?.(viewer?.team, unit.team) ?? (viewer?.team === unit.team ? 1 : -1);
    const hull = unit.vehicle;
    if (hull && this.phase.pool.objects[hull.simSlot] === hull) out.hull = hull;
    if (a.shapes && !out.hull) {
      const sphere = unit.sphere;
      out.radius = Math.max(0, sphere?.radius ?? 0);
      if (sphere?.center) out.center.copy(sphere.center);
      else if (unit.aimPoint) unit.aimPoint(out.center); else out.center.copy(unit.position);
    }
    if (a.aim) {
      if (unit.aimPoint) unit.aimPoint(out.aim); else out.aim.copy(unit.sphere?.center ?? unit.position);
      out.body = hull?.body ?? unit.turret?.bodies?.built ?? unit.building?.bodies?.built ?? null;
    }
    return out;
  }
  // TurretSeats construction retains rigs once; only lifecycle facts return.
  attachTurrets(crews) {
    this.crews = crews; crews.f2 = this; if (crews.live) crews.live.f2Seats = this;
    const b = this.bridge, point = this.drop;
    for (let index = 0; index < crews.turrets.length; index++) {
      const t = crews.turrets[index], w = t.weapon, state = turretState(t, index);
      if (w) registerWeaponClass(b, w.levels[0]);
      const radius = t.sphere(point), weapon = w ? { ...w, level: w.levels.indexOf(w.def), admits: Boolean(w.admitShot), hasRandom: Boolean(w.random), salvoSeed: null } : null;
      const locks = w ? w.levels.map(def => ({ range: Number(def.LockOnRange) || 0, on: Number(def.LockOnAngle) || 0,
        off: Number(def.LockOffAngle || def.LockOnAngle) || 0, relationMask: weaponAI(def).relationMask, categoryMask: weaponAI(def).categoryMask >>> 0 })) : [];
      const birth = { life: t.aiTarget?.generation ?? 0, timed: Boolean(t.rider && !t.rider.ninePose), locks, settings: turretSettings(t), turret: { state, turret: t, alive: t.alive, lease: null, gunner: null,
        attacker: t.lastAttacker ?? null, center: point, radius, weapon: Boolean(w), missileLock: hasMissileLock(w?.def), body: t.bodies?.built ?? null,
        rider: Boolean(t.rider), shootable: Boolean(t.rider?.shootable) }, weapon, ordnance: w?.def.ordnance ?? null,
        channel: w?.def.channel ?? 0, collide: Boolean(crews.collision) };
      if (birth.ordnance) crews.ordnance.sim.defId(birth.ordnance);
      const mirror = new F2TurretOutMirror(); mirror.state = state; mirror.weapon = weapon;
      this.turretRows.set(t, { mirror, state, center: point.clone(), radius, generation: t.aiTarget?.generation ?? 0, alive: t.alive, attacker: t.lastAttacker ?? null }); b.callArgs(this.turretBirthArgs, birth); this.x.f2_turret_adopt();
      t.f2Seats = this;
    }
    crews.man = (u, t) => this.invoke('man', u, t, null, crews.turrets.indexOf(t)).accepted;
    crews.requestExit = (u, options) => this.invoke('turretRequestExit', u, u?.seat?.turret, u?.seat, 0, options).accepted;
    crews.leave = (u, options) => this.invoke('turretLeave', u, u?.seat?.turret, u?.seat, 0, options).accepted;
    crews.vacate = (lease, options) => this.invoke('vacate', lease?.unit, lease?.turret, lease, 0, options).accepted;
    crews.destroyed = t => { this.turretLife(t); return this.invoke('turretDestroyed', null, t, t.lease, crews.turrets.indexOf(t)).accepted; };
  }
  idleTurrets(dt) {
    for (const t of this.crews.turrets) if (t.alive && !t.gunner && t.gun && (!Object.is(t.gunYaw, 0) || !Object.is(t.gunPitch, 0))) {
      this.x.f2_turrets_idle(dt); this.phase.sync(); return;
    }
  }
  turretLife(t) {
    const a = this.turretLifeRow;
    const previous = this.turretRows.get(t), attacker = t.lastAttacker ?? null;
    const generation = t.aiTarget?.generation ?? 0;
    if (!previous || previous.alive === t.alive && previous.attacker === attacker && previous.generation === generation) return;
    previous.alive = t.alive; previous.attacker = attacker; previous.generation = generation;
    // Building.sphere changes from the construction pad to the built model.
    // Read that lifecycle geometry directly when a props adapter is attached.
    previous.radius = Building.prototype.sphere.call(t, previous.center);
    a.turret = t; a.generation = generation; a.alive = t.alive; a.attacker = attacker; a.center.copy(previous.center); a.radius = previous.radius;
    this.bridge.callArgs(this.turretLifeArgs, a); this.x.f2_turret_life();
  }
  syncTurrets() {
    const b = this.bridge, f = b.f;
    const ord = this.io.ordnance / 8;
    if (ord && f[ord] && this.crews?.ordnance) {
      KIND_TABLE[KIND_ORDNANCE].sync(b, f, ord + 5, ord + f[ord + 2], this.crews.ordnance);
      this.phase.counters.outputFields += f[ord + 1] + f[ord + 3];
    }
    let base = this.io.turrets / 8;
    if (base) {
      const n = f[base], stride = f[base + 1], heap = base + f[base + 2];
      for (let i = 0; i < n; i++) {
        const at = base + 5 + i * stride;
        const header = syncF2TurretHeader(b, f, at, heap, this.turretHeader), t = header.turret;
        const out = syncF2TurretOut(b, f, at, heap, this.turretRows.get(t).mirror), state = out.state;
        t.yaw = state.yaw; t.pitch = state.pitch; t.yawVel = state.yawVel; t.pitchVel = state.pitchVel;
        t.gunYaw = state.gunYaw; t.gunPitch = state.gunPitch; t.barrel = state.barrel;
        t.rig.root.matrixWorld.copy(state.rootWorld);
        for (let j = 0; j < state.bones.length; j++) {
          const node = t.rig.bones[j], v = state.bones[j];
          node.position.copy(v.p); node.scale.copy(v.s); node.matrixWorld.copy(v.world);
          const q = node.quaternion; q._x = v.q.x; q._y = v.q.y; q._z = v.q.z; q._w = v.q.w;
        }
        t.lease = out.lease; t.gunner = out.gunner?.soldier ?? null;
        if (t.weapon && out.weapon) {
          for (const key of WEAPON_STATE) t.weapon[key] = out.weapon[key];
          t.weapon.def = t.weapon.levels[out.weapon.level];
        }
        if (out.posed && out.gunner) {
          const u = out.gunner; u.presentation?.reset?.();
          if (u.actor?.root) { u.actor.root.position.copy(u.soldier.position); u.actor.root.visible = !t.hidesGunner; }
          u.animator?.gunner?.(this.riderClips.get(out.lease) ?? null, out.samplePitch, out.sampleTurn, out.sampleTime, out.seatTurn);
        }
      }
      this.phase.counters.outputFields += n * stride + f[base + 3];
    }
    base = this.io.events / 8;
    if (base && this.crews) {
      const n = f[base], stride = f[base + 1], heap = base + f[base + 2], world = this.crews.world;
      for (let i = 0; i < n; i++) {
        const e = syncF2TurretEvent(b, f, base + 5 + i * stride, heap, this.turretEvent), shot = this.shot;
        shot.weapon = e.turret.weapon; shot.yaw = e.yaw; shot.pitch = e.pitch; shot.charge = e.charge; shot.targetId = e.targetId; shot.targetGeneration = e.targetGeneration;
        if (e.kind === 'bind') world.bindRound(e.round ?? e.bolt ?? undefined, e.turret, -1, -1, e.barrel);
        else if (e.kind === 'sound') world.sound('turretFire', e.turret, shot);
        else world.fired(e.turret, 0, shot, e.owner, e.origin, e.direction);
      }
      this.phase.counters.outputFields += n * stride + f[base + 3];
    }
    if (this.crews) { this.crews.ticks = this.io.turretTicks; this.crews.time = this.io.turretTime; }
  }
  action(a) {
    // Read all arguments before a death or board hook can reuse the import row.
    const kind = a.kind, unit = a.unit, holder = a.holder, index = a.index, lease = a.lease, post = a.post, reason = a.reason;
    const flag = a.flag, second = a.second, attacker = a.attacker, effect = a.effect;
    const w = a.turret ? this.crews.world : this.vehicles.seatsWorld, v = a.turret ? this.crews.turrets[holder] : this.vehicles.vehicles[holder];
    const answer = this.answer; answer.control = null; answer.value = undefined; answer.flag = false;
    if (kind === 'flush') this.phase.sync();
    else if (kind === 'controller') answer.value = w.controllerState(unit);
    else if (kind === 'control') { this.phase.sync(); answer.control = w.controllable(lease); }
    else if (kind === 'policy') { this.phase.sync(); if (a.turret) { seatWeaponPolicy(v.weapon, flag, second ? this.crews.seatRandom : null); answer.flag = Boolean(v.weapon?.random); } else v.setSeatPolicy(index, flag, second ? this.vehicles.seatRandom : null); }
    else if (kind === 'boarded') { this.phase.sync(); w.boarded(unit, v, index); }
    else if (kind === 'unboarded') { this.phase.sync(); w.unboarded(unit, v, index, post, reason); }
    else if (kind === 'moved') { this.phase.sync(); w.moved(unit, v, index); }
    else if (kind === 'kill') { this.phase.sync(); w.kill(unit, attacker); this.prepare(unit, unit?.seat?.vehicle, false); }
    else if (kind === 'raycast') { this.phase.sync(); answer.value = w.raycast(a.point, a.direction, a.yaw, unit, this.hit)?.distance; }
    else if (kind === 'body' && effect === 'dropped') {
      const point = w.riderDrop(v, index, unit, this.drop); answer.flag = Boolean(point); if (point) answer.point.copy(point);
    } else throw Error(`unknown F2 seat action ${kind}`);
    return answer;
  }
}
