// One whole ground/flight phase. Ordinary ticks import foreign inputs only.
import * as THREE from 'three';
import { F2Seats } from './f2-seats.js';
import { F2Firing } from './f2-firing.js';
import { F2Riders } from './f2-riders.js';
import { makeGround, makeHit } from './collision.js';
import { Weapon } from './weapons.js';
import { FLIGHT_CLASSES, flightConfig } from './flight/config.js';
import { F2HullLifeMirror, F2PadMirror, syncF2HullLife, syncF2Pad, putF2Bodies } from './sim/gen/f2.js';
import { MirrorPool } from './sim/runtime.js';
import { KIND_TABLE, KIND_VEHICLE, KIND_VEHICLES, KIND_BODY, KIND_UNIT, KIND_OWNER_SNAPSHOT, VEHICLE_KEYS } from './sim/gen/index.js';
import { rigNodes, json, classKeys, giveMapRules, pullPose, applyPose, flightNumbers } from './vehicle-phase-data.js';
import { putOrdnanceOwner } from './sim/gen/ordnance.js';
import { ordnanceTests } from './ordnance-sim.js';
import { F2Counters, F2MirrorCheck, countedF2Exports } from './f2-contract.js';
const MISS = { found: false, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: -1 };
const NO_GROUND = { found: false, height: 0, normal: new THREE.Vector3() };
const NO_INPUT = {};
const EMPTY_UNITS = [];
const OWNED_FIELDS = VEHICLE_KEYS;

// Adopt the existing Vehicle objects. Respawns keep them; retirement/reuse uses
// a new object and a new slab generation, never retargeting an old reference.
class FleetMirrors extends MirrorPool {
  constructor(identity) { super(KIND_VEHICLE); this.identity = identity; this.objects = identity.objects; this.retired = new Map(); }
  slotOf(v) { return v.simSlot ?? this.identity.slotOf(v); }
  put(v, f, o) {
    if (v.simSlot === undefined) this.identity.put(v, f, o);
    else { f[o] = v.simSlot; f[o + 1] = v.simGen; }
  }
  retire(slot, gen) {
    const v = super.retire(slot, gen);
    this.retired.set(`${slot}:${gen}`, v);
    return v;
  }
  is(v) { return this.identity.is(v); }
  at(slot, gen) {
    const v = this.objects[slot];
    if (v?.simGen === gen) return v;
    const retired = this.retired.get(`${slot}:${gen}`);
    if (retired) return retired;
    throw Error('stale F2 mirror handle');
  }
}

export function attachF2(vehicles, bridge, map, pack, unitClass, debug = false) {
  const phase = new F2(vehicles, bridge, map, pack, unitClass, debug);
  vehicles.f2 = phase;
  return phase;
}

class F2 {
  constructor(vehicles, bridge, map, pack, unitClass, debug) {
    bridge.identify({ tests: ordnanceTests });
    this.vehicles = vehicles; this.bridge = bridge;
    this.counters = new F2Counters(); this.x = countedF2Exports(bridge.x, this.counters);
    this.world = vehicles.world; this.collision = vehicles.collision;
    this.unitClass = unitClass; this.debug = debug;
    this.detector = new F2MirrorCheck();
    this.kernel = false; this.bodiesVersion = -1; this.colliders = new WeakSet();
    this.mirrorRows = new WeakMap(); this.syncedRows = [];
    this.lifeScratch = new F2HullLifeMirror(); this.padScratch = new F2PadMirror();
    this.ownerScratch = new Float64Array(3);
    this.classes = new Set(); this.values = []; this.teams = [];
    this.padPosts = vehicles.slots.map(slot => slot.desc.post ? vehicles.posts?.posts.find(p => p.desc.name.toLowerCase() === slot.desc.post) : null); this.memberPoint = new THREE.Vector3(); this.aimPoints = [];
    this.hit = makeHit(); this.groundHit = makeGround();
    this.unitHit = { unit: null, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), part: null };
    this.flag = { value: false }; this.number = { value: 0 };
    giveMapRules(bridge, vehicles.water, map);
    this.posing = this.pack(pack); this.flight = true;
    // A map without hulls has an empty pack and an empty fleet to step.
    if (!this.posing && (vehicles.vehicles.length || vehicles.slots.length)) throw Error('F2 requires the vehicle pack');
    this.x.veh_flight_env(bridge.strings.id(json(vehicles.flightEnvironment ?? {})), vehicles.maxFlyHeight);
    for (const [name, c] of Object.entries(FLIGHT_CLASSES)) for (const [key, value] of Object.entries(c)) this.retune(name, key, value);
    bridge.register({ ...this.handlers(),
      f2Foreign: a => {
        const dt = a.value;
        this.sync();
        this.firing.drain();
        if (vehicles.cosmetic !== false) vehicles.drawTethers(false);

      },
      f2Variant: (a) => {
        const slot = vehicles.slots[a.pad], name = a.name;
        this.sync();
        const v = vehicles.variant(slot, name);
        this.adopt(v);
        this.bodiesIn();
      },
      f2Life: (a) => {
        const v = vehicles.vehicles[a.hull];
        // A spawn precedes input latching. Reset this hull's aim, keeping
        // the other hulls' pending look samples until foreignIn consumes them.
        this.sync();
        for (const seat of v.seats) for (const w of seat.weapons) { w.state = new Weapon(w.def); w.barrel = 0; }
        this.firing.adopt(v);
        this.foreignIn();
      },
    });
    bridge.write(KIND_VEHICLE, vehicles.vehicles);
    bridge.write(KIND_VEHICLES, [vehicles]);
    const refs = bridge.refs[KIND_VEHICLE];
    this.pool = new FleetMirrors(refs);
    // Existing identity is retained for union classification and body owners.
    bridge.refs[KIND_VEHICLE] = this.pool;
    this.seats = new F2Seats(this);
    this.firing = new F2Firing(this);
    this.riders = new F2Riders(this);
    this.bodyArgs = { words: this.x.f2_body_words(), put: putF2Bodies }; this.bodyRefs = { bodies: [] };
    for (const v of vehicles.vehicles) this.adopt(v);
    this.bodiesIn();
    const ptr = this.x.f2_init(bridge.strings.id(json(vehicles.slots.map(s => s.desc))), vehicles.time, vehicles.acc);
    bridge.views(); this.descriptor = ptr / 8;
    this.inputBase = bridge.f[this.descriptor] / 8;
    this.capacity = bridge.f[this.descriptor + 1];
    bridge.reserve(16 * 1024 * 1024);
    this.counters.reset();
  }

  withBridge(call) {
    const x = this.bridge.x; this.bridge.x = this.x;
    try { return call(); } finally { this.bridge.x = x; }
  }

  adopt(v) {
    const b = this.bridge, slot = b.refs[KIND_VEHICLE].slotOf(v);
    v.simSlot = slot; v.simGen = this.pool.identity.gens[slot];
    if (!this.classes.has(v.name)) {
      this.classes.add(v.name);
      this.x.veh_class(b.strings.id(v.name), b.strings.id(json(classKeys(v.def))), v.turnRate);
    }
    b.writeSome(KIND_VEHICLE, [v]);
    this.x.f2_register(v.simSlot, v.simGen);
    const values = pullPose(v), at = this.input(values.length);
    b.f.set(values, at);
    if (!this.x.veh_pull(slot, v.aiTarget.generation, values.length)) throw Error('F2 could not retain a vehicle rig');
    this.seats.adopt(v);
    this.firing.adopt(v);
    this.record(v);
  }

  // A hook sees already-published canonical state and sends only commands back.
  hook(call) {
    this.sync();
    try { call(); } finally {
      this.bridge.views();
      if (this.kernel) for (let i = 0; i < this.collision.bodies.length; i++) this.bridge.u8[this.flags + i] = this.collision.bodies[i].enabled ? 1 : 0;
    }
  }

  step(dt, tick) {
    const b = this.bridge, vs = this.vehicles;
    if (this.debug) this.check();
    // Body order changes are lifecycle work. Flags need no getter crossing.
    if (this.collision.rays) this.collision.rayBodies();
    if (this.collision.rays && this.bodiesVersion !== this.collision.bodiesVersion) this.bodiesIn();
    b.views();
    if (this.kernel) this.counters.inputFields += this.collision.bodies.length;
    if (this.kernel) for (let i = 0; i < this.collision.bodies.length; i++) b.u8[this.flags + i] = this.collision.bodies[i].enabled ? 1 : 0;
    const values = this.values; values.length = 0;
    values.push(vs.clock(), vs.water?.level ?? 0, 1 | (vs.cosmetic !== false ? 2 : 0) | 4 | (vs.flightAssists ? 8 : 0));
    for (let i = 0; i < vs.slots.length; i++) {
      const post = this.padPosts[i];
      values.push(post ? 1 : 0, post?.team ?? 0);
    }
    const p = vs.spawnOccupant; values.push(p ? 1 : 0, p?.x ?? 0, p?.y ?? 0, p?.z ?? 0);
    this.writeInput(values, 0);
    this.foreignIn();
    this.seats.prepare();
    this.counters.ticks++;
    const memory = b.memory.buffer;
    this.x.f2_step(dt, tick);
    if (this.debug && b.memory.buffer !== memory) throw Error('F2 memory grew during the phase');
    this.sync();
    vs.tickAlpha = Math.max(0, vs.acc / (1 / 120)); vs.presented = false;
    if (this.debug) this.check();
  }

  writeInput(values, offset) {
    if (offset + values.length >= this.capacity - 2) throw Error('F2 foreign input capacity exceeded');
    this.bridge.views(); this.bridge.f.set(values, this.inputBase + offset);
    this.counters.inputFields += values.length;
  }

  addTeam(team) {
    for (const previous of this.teams) if (Object.is(previous, team)) return;
    this.teams.push(team);
  }
  foreignIn() {
    const vs = this.vehicles, b = this.bridge, refs = b.refs, values = this.values, p = this.memberPoint;
    values.length = 0;
    const all = vs.units?.all?.() ?? EMPTY_UNITS;
    values.push(all.length);
    for (const t of all) {
      const hull = t.vehicle ?? null;
      const current = hull && this.pool.objects[hull.simSlot] === hull;
      const kind = t instanceof this.unitClass ? 0 : current ? (hull.aiTarget === t ? 1 : 3) : 2;
      if (kind === 2) { if (t.aimPoint) t.aimPoint(p); else p.copy(t.position); }
      values.push(kind, t.id, t.generation ?? NaN, t.alive ? 1 : 0, t.team == null ? 0 : 1, t.team ?? 0,
        hull ? refs[KIND_VEHICLE].slotOf(hull) : -1, kind === 2 ? p.x : 0, kind === 2 ? p.y : 0, kind === 2 ? p.z : 0);
    }
    const teams = this.teams; teams.length = 0;
    for (const t of all) this.addTeam(t.team ?? null);
    for (const v of vs.vehicles) this.addTeam(v.team ?? null);
    values.push(teams.length);
    for (const t of teams) values.push(t === null ? 0 : 1, t ?? 0);
    for (const a of teams) for (const c of teams) values.push(vs.units?.relation?.(a, c) ?? (a === c ? 1 : -1));
    let count = 0; for (const v of vs.vehicles) if (v.flying) count++;
    values.push(count);
    for (const v of vs.vehicles) { if (!v.flying) continue;
      const l = v.seats[0]?.lease, network = l?.unit?.networkHuman && l.intent.flight;
      values.push(v.simSlot, network ? 1 : 0);
      flightNumbers(l?.input ?? NO_INPUT, values); flightNumbers(network || NO_INPUT, values);
    }
    const offset = 3 + vs.slots.length * 2 + 4;
    this.writeInput(values, offset);
    b.f[this.inputBase + this.capacity - 1] = offset;
    const firingWords = this.firing.input(this.inputBase + offset + values.length);
    b.f[this.inputBase + this.capacity - 2] = offset + values.length + firingWords;
    this.counters.inputFields += 2 + firingWords;
  }

  sync() {
    const b = this.bridge, prior = b.x; b.x = this.x;
    try { return this.syncState(); } finally { b.x = prior; }
  }

  syncState() {
    const b = this.bridge; b.views();
    if (this.descriptor === undefined) return;
    const d = this.descriptor;
    this.seats.syncLeases();
    this.syncedRows.length = 0;
    let base = b.f[d + 2] / 8;
    if (base) {
      const f = b.f, k = KIND_TABLE[KIND_VEHICLE], rows = f[base], stride = f[base + 1], h = base + f[base + 2];
      for (let i = 0; i < rows; i++) {
        const at = base + 5 + i * stride;
        const v = i < f[base + 4] ? this.pool.retire(f[at], f[at + 1]) : this.pool.at(f[at], f[at + 1]);
        this.syncedRows.push(v);
        k.sync(b, f, at, h, v);
      }
      this.counters.outputFields += rows * stride + f[base + 3];
    }
    base = b.f[d + 3] / 8;
    if (base) {
      const f = b.f, rows = f[base], stride = f[base + 1], h = base + f[base + 2];
      for (let i = 0; i < rows; i++) {
        const pad = syncF2Pad(b, f, base + 5 + i * stride, h, this.padScratch);
        const slot = this.vehicles.slots[pad.index]; slot.remaining = pad.remaining; slot.vehicle = pad.vehicle;
      }
      this.counters.outputFields += rows * stride + f[base + 3];
    }
    base = b.f[d + 11] / 8;
    if (base) {
      const f = b.f, rows = f[base], stride = f[base + 1], h = base + f[base + 2];
      for (let i = 0; i < rows; i++) {
        const life = syncF2HullLife(b, f, base + 5 + i * stride, h, this.lifeScratch), v = life.vehicle;
        const changed = v.aiTarget.generation !== life.generation;
        v.aiTarget.generation = life.generation; v.root.visible = life.visible;
        if (v.flying && changed) {
          v.flightClass = life.flightClass; v.minSpeed = life.minSpeed; v.midSpeed = life.midSpeed;
          v.boostSpeed = life.boostSpeed; v.speed = life.speed; v.turnRate = life.turnRate;
          v.flightConfig = flightConfig(v.def, v.flightClass);
        }
      }
      this.counters.outputFields += rows * stride + f[base + 3];
    }
    this.firing.syncDescriptor(b.f[d + 14]);
    this.bodiesOut();
    // Pose output is a compact flat record; retained tree state stays in Rust.
    let o = b.f[d + 5] / 8;
    if (o) {
      const n = b.f[o++];
      for (let i = 0; i < n; i++) {
        const v = this.pool.objects[b.f[o]], count = b.f[o + 1]; o += 2;
        applyPose(v, b.f, o); o += count;
        if (!this.syncedRows.includes(v)) this.syncedRows.push(v);
        this.counters.outputFields += count + 2;
      }
    }
    this.seats.syncNodes();
    o = b.f[d + 6] / 8;
    if (o) {
      const n = b.f[o++];
      for (let i = 0; i < n; i++, o += 5) {
        const v = this.pool.objects[b.f[o]];
        for (const s of v.seats) for (const w of s.weapons) {
          w.state.lockTargetId = b.f[o + 1]; w.state.lockGeneration = b.f[o + 2];
          w.state.lockElapsed = b.f[o + 3]; w.state.lockComplete = b.f[o + 4] === 1;
        }
        this.counters.outputFields += 5;
      }
    }
    this.seats.syncBodies();
    this.vehicles.acc = b.f[d + 7]; this.vehicles.flightAcc = b.f[d + 8]; this.vehicles.time = b.f[d + 9];
    if (this.debug) for (const v of this.syncedRows) this.record(v);
  }

  // Lifecycle output uses a side row so gameplay generations stay distinct
  // from the slab's generation. Populated by publication below.

  retire(v) {
    if (!this.x.f2_retire(v.simSlot, v.simGen)) throw Error('stale F2 retirement');
    this.sync(); this.detector.retire(v); this.pool.identity.release(v);
    const index = this.vehicles.vehicles.indexOf(v);
    if (index >= 0) this.vehicles.vehicles.splice(index, 1);
    for (const slot of this.vehicles.slots) for (const [name, variant] of slot.variants) if (variant === v) slot.variants.delete(name);
  }

  command(v, code, value = 0, attacker = null) {
    const owner = this.ownerScratch;
    putOrdnanceOwner(this.bridge, attacker, owner, 0, null);
    if (!this.x.f2_command(v.simSlot, v.simGen, v.aiTarget.generation, code, value, owner[0], owner[1], owner[2])) throw Error('stale F2 command');
    this.sync();
  }

  owned(v) {
    const out = [];
    const foreign = new Set(['seats', 'towTarget', 'towLocal', 'towLeg', 'towAngle', 'towWinding', 'lastAttacker', 'driver', 'seatIndex', 'body', 'root', 'aiTarget', 'seatViews', 'seatLeases', 'flightConfig']);
    const add = (path, value) => {
      if (value?.isVector3) for (const axis of ['x','y','z']) add(`${path}.${axis}`, value[axis]);
      else if (value?.isQuaternion) for (const axis of ['x','y','z','w']) add(`${path}.${axis}`, value[axis]);
      else if (value?.isMatrix4) value.elements.forEach((x, i) => add(`${path}.elements.${i}`, x));
      else if (Array.isArray(value)) value.forEach((x, i) => add(`${path}.${i}`, x));
      else if (value && typeof value === 'object') for (const [k, x] of Object.entries(value)) add(`${path}.${k}`, x);
      else out.push([path, value]);
    };
    for (const key of OWNED_FIELDS) if (!foreign.has(key)) {
      if (key === 'wheels') {
        for (let i = 0; i < v.wheels.length; i++) for (const k of ['x', 'radius', 'phase', 'previous', 'tickFrom']) add(`wheels.${i}.${k}`, v.wheels[i][k]);
      } else add(key, v[key]);
    }
    for (let i = 0; i < v.seats.length; i++) {
      const s = v.seats[i];
      for (const key of ['aimYaw', 'pitch', 'lookYaw', 'lookPitch', 'lease', 'lastYaw', 'yawRate']) out.push([`seats.${i}.${key}`, s[key]]);
      for (const axis of ['x', 'y', 'z']) out.push([`seats.${i}.target.${axis}`, s.target?.[axis]]);
    }
    out.push(['root.visible', v.root.visible], ['aiTarget.generation', v.aiTarget.generation],
      ['lastAttacker', v.lastAttacker], ['driver', v.driver], ['seatIndex', v.seatIndex]);
    return out;
  }
  record(v) {
    if (!this.debug) return;
    let row = this.mirrorRows.get(v);
    if (!row) {
      const fields = this.owned(v), paths = new Array(fields.length);
      for (let i = 0; i < fields.length; i++) paths[i] = fields[i][0].split('.');
      row = { fields, paths }; this.mirrorRows.set(v, row);
    }
    for (let i = 0; i < row.fields.length; i++) {
      let value = v;
      for (const key of row.paths[i]) value = value?.[key];
      row.fields[i][1] = value;
    }
    this.detector.published(v, row.fields);
  }
  check() {
    this.seats.check();
    for (const v of this.vehicles.vehicles) {
      const row = this.mirrorRows.get(v), expected = this.detector.values.get(v);
      if (!row || !expected) continue;
      for (let i = 0; i < row.fields.length; i++) {
        let value = v;
        for (const key of row.paths[i]) value = value?.[key];
        if (!Object.is(value, expected[i][1])) throw Error(`F2 mirror write: ${expected[i][0]}`);
      }
    }
  }
  retune(name, key, value) {
    if (this.flight && typeof value === "number") this.x.veh_flight_tune(this.bridge.strings.id(name), this.bridge.strings.id(key), value);
  }

  pack({ data, buffer, indexBuffer, animationBuffer }) {
    const text = new TextEncoder().encode(json({ models: data.models, collision: data.collision, animations: data.animations, classes: data.classes }));
    const head = Math.ceil(text.length / 4) * 4, floats = buffer?.byteLength ?? 0, indices = indexBuffer?.byteLength ?? 0, clips = animationBuffer?.byteLength ?? 0;
    const ptr = this.x.veh_bytes(head + floats + indices + clips);
    this.bridge.views();
    const u8 = this.bridge.u8;
    u8.set(text, ptr);
    if (floats) u8.set(new Uint8Array(buffer), ptr + head);
    if (indices) u8.set(new Uint8Array(indexBuffer), ptr + head + floats);
    if (clips) u8.set(new Uint8Array(animationBuffer), ptr + head + floats + indices);
    return this.x.veh_pack(text.length, floats / 4, indices / 4, clips / 4) > 0;
  }

  input(count) {
    const ptr = this.x.veh_input(count);
    this.bridge.views();
    return ptr / 8;
  }

  bodiesIn() {
    const c = this.collision, rays = c?.rays;
    if (!rays) {
      if (this.kernel) { this.x.veh_collision(-1, -1); this.kernel = false; }
      return;
    }
    if (!this.kernel) {
      const blockers = c.vehicleBlockers ? rays.fixedSet(c.blockerSet(c.vehicleBlockers), c.vehicleBlockers.bvh) : -1;
      this.x.veh_collision(c.rayWorld, blockers);
      this.kernel = true;
      this.bodiesVersion = -1;
    }
    c.rayBodies();
    const bodies = c.bodies;
    if (this.bodiesVersion !== c.bodiesVersion) {
      const rows = this.bodyRefs.bodies; rows.length = bodies.length;
      for (let i = 0; i < bodies.length; i++) { const row = rows[i] ??= { body: null, kernel: 0 }; row.body = bodies[i]; row.kernel = bodies[i].raySlot; }
      this.bridge.callArgs(this.bodyArgs, this.bodyRefs); this.x.f2_bodies();
      this.bodiesVersion = c.bodiesVersion;
    }
    for (const v of this.vehicles.vehicles) if (!this.colliders.has(v.body)) this.collider(v);
    this.flags = this.x.veh_flags();
  }

  collider(v) {
    const rays = this.collision.rays, body = v.body;
    // (Making a cache's copy may grow the memory: the ids first.)
    const walls = (v.wallCaches ?? []).map((cache) => (cache ? rays.cache(cache) : -1));
    const springs = (v.springCaches ?? []).map((cache) => (cache ? rays.cache(cache) : -1));
    const slot = this.bridge.refs[KIND_BODY].slotOf(body);
    const at = this.input(8 + walls.length + springs.length), f = this.bridge.f, b = body.localBounds;
    f[at] = slot; f[at + 1] = body.raySlot;
    f[at + 2] = b.min.x; f[at + 3] = b.min.y; f[at + 4] = b.min.z; f[at + 5] = b.max.x; f[at + 6] = b.max.y; f[at + 7] = b.max.z;
    for (let i = 0; i < walls.length; i++) f[at + 8 + i] = walls[i];
    for (let i = 0; i < springs.length; i++) f[at + 8 + walls.length + i] = springs[i];
    this.x.veh_collider(walls.length, springs.length);
    this.colliders.add(body);
  }

  bodiesOut() {
    if (!this.kernel) return;
    const ptr = this.bridge.f[this.descriptor + 4];
    this.bridge.views();
    const f = this.bridge.f, bodies = this.collision.rays.bodies;
    let o = ptr / 8;
    const toggled = f[o], placed = f[o + 1];
    this.counters.outputFields += toggled * 2 + placed * 39;
    o += 2;
    for (let i = 0; i < toggled; i++, o += 2) bodies[f[o]].enabled = f[o + 1] !== 0;
    for (let i = 0; i < placed; i++, o += 39) {
      const body = bodies[f[o]];
      body.bounds.min.set(f[o + 1], f[o + 2], f[o + 3]);
      body.bounds.max.set(f[o + 4], f[o + 5], f[o + 6]);
      body.matrix.fromArray(f, o + 7);
      body.inverse.fromArray(f, o + 23);
    }
  }

  body(slot) {
    return slot < 0 ? null : this.bridge.refs[KIND_BODY].objects[slot];
  }

  bodySlot(body) {
    return body ? this.bridge.refs[KIND_BODY].slotOf(body) : -1;
  }

  handlers() {
    const w = this.world, vs = this.vehicles;
    const hullAt = (i) => vs.vehicles[i];
    const origin = new THREE.Vector3(), direction = new THREE.Vector3(), position = new THREE.Vector3(), velocity = new THREE.Vector3();
    const previous = new THREE.Vector3(), q = new THREE.Quaternion(), own = new THREE.Quaternion(), matrix = new THREE.Matrix4();
    const center = new THREE.Vector3(), point = new THREE.Vector3();
    const probes = [];
    const castResult = { found: false, distance: 0, point: this.hit.point, normal: this.hit.normal, terrain: false, body: -1 };
    const pushResult = { undo: false, position, velocity };
    const groundResult = { found: false, height: 0, normal: this.groundHit.normal };
    return {
      vehCast: (a) => {
        origin.copy(a.origin); direction.copy(a.direction);
        const hit = w.cast(origin, direction, a.far, this.hit, this.body(a.ignore));
        if (!hit) return MISS;
        castResult.found = true; castResult.distance = hit.distance; castResult.terrain = Boolean(hit.terrain); castResult.body = this.bodySlot(hit.body);
        return castResult;
      },
      vehPushWalls: (a) => {
        const hull = hullAt(a.hull), radius = a.radius, terrain = a.terrain;
        probes.length = a.probes.length;
        for (let i = 0; i < probes.length; i++) (probes[i] ??= new THREE.Vector3()).copy(a.probes[i]);
        q.copy(a.q); previous.copy(a.previous); position.copy(a.position); velocity.copy(a.velocity);
        const mine = a.own ? own.copy(a.own) : null;
        const undo = w.pushWalls(hull, probes, radius, q, mine, previous, position, velocity, terrain);
        pushResult.undo = Boolean(undo); return pushResult;
      },
      vehGround: (a) => {
        const g = w.ground(a.x, a.z, a.top, this.groundHit);
        if (!g) return NO_GROUND;
        groundResult.found = true; groundResult.height = g.height; return groundResult;
      },
      vehSpringPolygons: (a) => {
        const hull = hullAt(a.hull), index = a.index, radius = a.radius;
        center.copy(a.center);
        return { polygons: w.springPolygons(hull, index, center, radius).map((p) => ({ id: p.id, moving: Boolean(p.moving), triangles: p.triangles })) };
      },
      vehMoveBody: (a) => { const body = this.body(a.body); matrix.copy(a.matrix); w.moveBody(body, matrix); },
      vehEnableBody: (a) => { w.enableBody(this.body(a.body), a.enabled); },
      vehBodyEnabled: (a) => { this.flag.value = Boolean(w.bodyEnabled(this.body(a.body))); return this.flag; },
      vehHasWater: () => { this.flag.value = Boolean(vs.water); return this.flag; },
      vehWaterOver: (a) => { this.flag.value = Boolean(w.waterOver(a.x, a.z)); return this.flag; },
      vehWaterLevel: () => { this.number.value = w.waterLevel(); return this.number; },
      vehInDeathRegion: (a) => { point.copy(a.position); this.flag.value = Boolean(w.inDeathRegion(point)); return this.flag; },
      vehRaycastUnits: (a) => {
        origin.copy(a.origin); direction.copy(a.direction);
        const hit = w.raycastUnits(origin, direction, a.far, this.unitHit);
        return hit ? { ...MISS, found: true, distance: hit.distance } : MISS;
      },
      vehClock: () => { this.number.value = w.clock(); return this.number; },
      vehDamaged: (a) => {
        const v = hullAt(a.hull), amount = a.amount, attacker = a.attacker;
        this.hook(() => w.damaged(v, amount, attacker));
      },
      vehDestroyed: (a) => { const v = hullAt(a.hull); this.hook(() => w.destroyed(v)); },
      vehExplosion: (a) => {
        const v = hullAt(a.hull);
        // At the hull's own position (the very vector, as the JavaScript
        // hands it) when its class sets no offset, else a point of its own.
        const def = a.final ? v.def.finalExplosion || v.def.explosion : v.def.explosion;
        const at = a.final || v.def.fx?.explosionOffset ? a.position.clone() : v.position;
        this.hook(() => w.explosion(def, at, v));
      },
      vehEffect: (a) => { const name = a.name, at = a.position.clone(); w.effect(name, at); },
      vehAimPoint: (a) => { const i = a.hull; vs.units.all()[i].aimPoint(point); return { position: point }; },
      vehAimPoints: (a) => {
        const indices = Array.from(a.indices), all = vs.units.all(), points = this.aimPoints;
        points.length = indices.length;
        for (let k = 0; k < indices.length; k++) all[indices[k]].aimPoint(points[k] ??= new THREE.Vector3());
        return { points };
      },
    };
  }
}
