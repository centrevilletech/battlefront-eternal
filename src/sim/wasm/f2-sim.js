import {BlockHeaderLayout as BLOCK} from '../gen/boundary.js';
// One whole ground/flight phase. Ordinary ticks import foreign inputs only.
import * as THREE from 'three';
import { VehicleContent } from './vehicle-content.js';
import { applyDelta, deltaBuffer } from './f2-delta.js';
import { F2Seats } from './f2-seats.js';
import { F2Firing } from './f2-firing.js';
import { F2Riders } from './f2-riders.js';
import { Weapon } from '../../weapons.js';
import { Vehicle } from '../../vehicles.js';
import { FLIGHT_CLASSES, flightConfig } from '../../flight/config.js';
import { F2HullLifeMirror, F2PadMirror, syncF2HullLife, syncF2Pad, F2PhaseFlag_INDEX, F2BodyHeaderLayout as BODY_HEADER, F2BodyToggleLayout as TOGGLE, F2BodyPlacedLayout as PLACED, F2PhaseLayout as PHASE, F2PoseMirror, F2PoseSeatMirror, F2AimerMirror, syncF2Pose, F2InputLayout, F2PadTeamLayout, F2InputMirror, putF2Input, putF2PadTeam } from '../gen/f2.js';

import { MirrorPool } from '../runtime.js';
import { KIND_TABLE, KIND_VEHICLE, KIND_VEHICLES, KIND_BODY, VEHICLE_KEYS } from '../gen/index.js';
import { json, classKeys, giveMapRules, pullPose } from './vehicle-phase-data.js';
import { putOrdnanceOwner } from '../gen/ordnance.js';
import { ordnanceTests } from './ordnance-sim.js';
import { F2MirrorCheck } from './f2-contract.js';
const MISS = { found: false, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: -1 };
const OWNED_FIELDS = VEHICLE_KEYS;
const FLAG_POSES = 1 << F2PhaseFlag_INDEX.get('poses'), FLAG_COSMETIC = 1 << F2PhaseFlag_INDEX.get('cosmetic');
const FLAG_FLIGHT = 1 << F2PhaseFlag_INDEX.get('flight'), FLAG_ASSISTS = 1 << F2PhaseFlag_INDEX.get('assists');

// Adopt the existing Vehicle objects. Respawns keep them; retirement/reuse uses
// a new object and a new slab generation, never retargeting an old reference.
// Vehicle identity is shared with registry targets and opaque body owners.
// A retired handle keeps its last object for already-issued effects; only a
// matching live generation can receive a command. Reusing a slot never changes
// what an older reference means.
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

// Vehicles.update delegates only after construction has supplied the pack.
// Page prediction loads shared resources through VehicleContent but never
// constructs this canonical fleet, its clock, or its lease/weapon phase.
export function attachF2(vehicles, bridge, map, pack, unitClass, debug = false) {
  const phase = new F2(vehicles, bridge, map, pack, unitClass, debug);
  vehicles.f2 = phase;
  return phase;
}

class F2 extends VehicleContent {
  // Vehicle construction is the only full hull import. Immutable class keys,
  // rig topology, collision mappings and definition handles are resolved here.
  // Ordinary ticks have a separate generated input layout containing foreign
  // facts, rather than a snapshot of the Vehicle objects.
  constructor(vehicles, bridge, map, pack, unitClass, debug) {
    super(vehicles, bridge);
    bridge.identify({ tests: ordnanceTests });
    this.world = vehicles.world;
    this.unitClass = unitClass; this.debug = debug;
    this.detector = new F2MirrorCheck();
    this.lastVersion = -1;
    // A coordinated unit owner supplies the shared crossing census. Native
    // fleet fixtures can still run this phase independently without one.
    this.counters = null;

    this.mirrorRows = new WeakMap(); this.syncedRows = []; this.syncedCount = 0; this.syncStamp = 0;
    this.lifeScratch = new F2HullLifeMirror(); this.padScratch = new F2PadMirror();
    this.ownerScratch = new Float64Array(3);
    this.prepared = [];
    this.classes = new Set();
    this.inputHeader = new F2InputMirror();
    this.padTeam = {hasTeam:false,team:0};
    this.padPosts = vehicles.slots.map(slot => slot.desc.post ? vehicles.posts?.posts.find(p => p.desc.name.toLowerCase() === slot.desc.post) : null); this.aimPoints = [];
    this.unitHit = { unit: null, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), part: null };
    giveMapRules(bridge, vehicles.water, map);
    this.posing = this.pack(pack);
    // A map without hulls has an empty pack and an empty fleet to step.
    if (!this.posing && (vehicles.vehicles.length || vehicles.slots.length)) throw Error('F2 requires the vehicle pack');
    this.x.veh_flight_env(bridge.strings.id(json(vehicles.flightEnvironment ?? {})), vehicles.maxFlyHeight);
    for (const [name, c] of Object.entries(FLIGHT_CLASSES)) for (const [key, value] of Object.entries(c)) this.retune(name, key, value);
    bridge.register({ ...this.handlers(),
      // fireCrews and updateTethers have already run natively. This callback
      // publishes their ordered visual events before turret crews run. Body writes
      // and lazy rider geometry remain native; no JS rig is sampled here.
      f2Foreign: () => {
        this.sync();
        this.firing.drain();
        if (vehicles.cosmetic !== false) vehicles.drawTethers(false);
      },
      // Vehicles.variant may construct a class never used on this pad before.
      // Publish first, since constructor hooks can inspect other hulls, then adopt
      // only the new object. Existing pending input belongs to its original life.
      f2Variant: (a) => {
        const slot = vehicles.slots[a.pad], name = a.name;
        this.sync();
        const v = vehicles.variant(slot, name);
        this.adopt(v);
        this.bodiesIn();
      },
      // Vehicle.spawn has reset only this life. Weapon objects used by JS
      // readers must match that reset without clearing another hull's controls.
      // Rebuilding pending foreign input is safe because caches commit after latch.
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
    // Keep the original identity table behind the mirror pool. Bodies and
    // registry entries already hold these objects; replacing them during a spawn
    // would make a live reference silently point at a different vehicle.
    this.pool = new FleetMirrors(refs);
    // Existing identity is retained for union classification and body owners.
    bridge.refs[KIND_VEHICLE] = this.pool;
    this.seats = new F2Seats(this);
    this.firing = new F2Firing(this);
    this.riders = new F2Riders(this);
    for (const v of vehicles.vehicles) this.adopt(v);
    this.bodiesIn();
    const ptr = this.x.f2_init(bridge.strings.id(json(vehicles.slots.map(s => s.desc))), vehicles.time, vehicles.acc);
    bridge.views(); this.descriptor = (ptr / 8) | 0;
    this.inputBase = (bridge.f[this.descriptor + PHASE.input] / 8) | 0;
    this.capacity = bridge.f[this.descriptor + PHASE.capacity];
    bridge.reserve(16 * 1024 * 1024);
    this.bodyFlags();
    this.sync();
  }

  // Vehicle constructor data and pullPose initialize retained native rows.
  // The encoded output buffers belong to this exact mirror generation. Later
  // word deltas update those buffers without copying the rest of the fleet.
  adopt(v) {
    const b = this.bridge, slot = b.refs[KIND_VEHICLE].slotOf(v);
    v.simSlot = slot; v.simGen = this.pool.identity.gens[slot];
    if (!this.classes.has(v.name)) {
      this.classes.add(v.name);
      this.x.veh_class(b.strings.id(v.name), b.strings.id(json(classKeys(v.def))), v.turnRate);
    }
    b.writeSome(KIND_VEHICLE, [v]);
    const values = pullPose(v), at = this.input(values.length);
    b.f.set(values, at);
    // A world's hull spawned at load has its spawn pose seeded natively.
    const seed = v.alive && !v.system.page ? 1 : 0;
    if (!this.x.veh_pull(slot, v.aiTarget.generation, values.length, seed)) throw Error('F2 could not retain a vehicle rig');
    // Registration sizes working leg buffers from this retained class rig.
    this.x.f2_register(v.simSlot, v.simGen, v.f2Prepared ? 1 : 0);
    // Mirror readers keep the usual objects and shared arrays. Their encoded
    // rows are retained separately, so native output can omit unchanged words
    // without changing legSide or seat identity seen by prediction and capture.
    v.f2HullWords = deltaBuffer(); v.f2PoseWords = deltaBuffer();
    v.f2Pose = new F2PoseMirror();
    for (const seat of v.seats) {
      const row = new F2PoseSeatMirror();
      for (const weapon of seat.weapons) for (const aimer of weapon.aimers) row.aimers.push(new F2AimerMirror());
      v.f2Pose.seats.push(row);
    }
    this.seats.adopt(v);
    this.firing.adopt(v);
    this.record(v);
    this.onPrepared?.(v);
  }

  // Vehicles.variant resolves every possible pad class during loading, while
  // the active fleet, registry and collision order keep their original shape.
  // The native spawn later chooses when each retained facade becomes visible.
  prepareVariants() {
    const vs = this.vehicles, c = this.collision;
    for (const pad of vs.slots) {
      const seen = new Set(pad.variants.keys());
      for (const name of Object.values(pad.desc.classes)) {
        if (!name || seen.has(name)) continue;
        seen.add(name);
        const serial = c.bodySerial;
        const v = new Vehicle(vs,pad,name);
        v.f2Prepared = true; v.aiTarget.id = 0;
        c.removeBody(v.body); c.bodySerial = serial;
        c.rayBodies();
        this.adopt(v); this.collider(v);
        this.x.f2_prepare_body(v.body.raySlot);
        this.prepared.push(v);
      }
    }
    // Consume cold publications while the private facades are already bound.
    // No constructor, rig pull or body naming is needed by their first spawn.
    this.sync();
  }

  // A hook sees already-published canonical state and sends only commands back.
  // Vehicle.damage and explosion hooks can synchronously call back into seats
  // or damage another hull. Every hook sees committed mirrors, and body flags
  // are refreshed afterward because unported damage may disable collision.
  hook(call, a, c, d) {
    this.sync();
    try { call.call(this.world, a, c, d); } finally {
      this.bodyFlags();
    }
  }

  // Vehicles.update: import foreign facts once, run the native scheduler, then
  // read its final publication. Native callbacks publish at the earlier slots
  // where gameplay listeners need current state. There is no per-hull export
  // loop or rig update on this path.
  step(dt, tick) {
    const vs = this.vehicles, owned = vs.f3;
    // Structural hull replacement prepares shared native query storage before
    // the no-growth window, just as the owning unit coordinator does.
    owned.phase.bodiesIn();
    this.prepareStep();
    // A standalone fleet entry can still kill an owned crew. Join the same
    // outer command boundary used by incoming hull damage, including RNG.
    owned.beforeCommand(null, 2, 0);
    try { this.x.f2_step(dt, tick); }
    finally { owned.afterCommand(); }
    this.commitForeign();
    this.finishStep();
  }

  // The unit coordinator prepares fleet foreign inputs before its one native
  // entry. Register changed mounted RNG callbacks before publishing the shared
  // generator states, then retain the exact input batch for cache commitment.
  prepareStep() {
    if (this.debug) this.check();
    this.foreignIn(true);
    this.preparedWords = this.seats.prepare();
    this.preparedWords += this.inputHeader.end;
    this.phaseMemory = this.bridge.memory.buffer;
    this.firing.drainedWords = 0;
  }
  // Vehicles.update's presentation alpha follows the final canonical clock.
  // Sync has already consumed the fleet delta when either outer entry gets here.
  finishStep() {
    const vs = this.vehicles;
    if (this.debug && this.bridge.memory.buffer !== this.phaseMemory) throw Error('F2 memory grew during the phase');
    vs.tickAlpha = Math.max(0, vs.acc / (1 / 120)); vs.presented = false;
    if (this.debug) this.check();
  }

  // Vehicles.update imports only pad ownership, flight keys and weapon policy.
  // Registry order, relations and body geometry already belong to native units.
  foreignIn(phaseStart = false) {
    const vs = this.vehicles, b = this.bridge;
    b.views(); const f = b.f, base = this.inputBase, h = this.inputHeader;
    if (phaseStart) {
      h.clock = vs.clock(); h.level = vs.water?.level ?? 0;
      h.flags = FLAG_POSES | FLAG_FLIGHT | (vs.cosmetic !== false ? FLAG_COSMETIC : 0) | (vs.flightAssists ? FLAG_ASSISTS : 0);
    }
    let at = base + F2InputLayout.words;
    h.pads = at - base;
    for (let i = 0; i < vs.slots.length; i++) {
      const post = this.padPosts[i], row = this.padTeam;
      row.hasTeam = Boolean(post); row.team = post?.team ?? 0;
      putF2PadTeam(b, row, f, at, null); at += F2PadTeamLayout.words;
    }
    h.occupant = vs.spawnOccupant;
    h.memberLength = h.memberCount = h.teamCount = 0;
    h.members = h.teams = h.relations = at - base;
    // Network pilot keys are set by the players' phase (rust/sim/src/players)
    // into the fleet's retained slots; no flight row crosses here.
    h.flights = at - base; h.flightCount = 0;
    // Weapon modifiers and AI target hints use their own generated change rows.
    // Pressed, held, reload and aim are absent here: importing their JS mirrors
    // would restore pre-latch edges after the native seat phase consumed them.
    h.firing = at - base; at += this.firing.input(at); h.end = at - base;
    putF2Input(b, h, f, base, null);

  }

  // A spawn may rebuild the pending input before native latching reads it.
  // Commit caches only at the first post-latch callback, so an earlier batch's
  // changes survive every lifecycle callback in the same phase.
  // The bytes here are the batch native code actually read. Committing the
  // current JS objects instead would hide deaths or AI edits made by later
  // callbacks in the same phase. Copy only the submitted changed records.
  commitForeign() {
    const base = this.inputBase, h = this.inputHeader;
    this.firing.commitInput(base + h.firing);
  }
  // Retain the backing array as well as the mirrors. Truncating an Array to
  // zero discards its storage in V8 even when it is filled again immediately.
  synced(v) {
    if (v.f2Synced === this.syncStamp) return;
    v.f2Synced = this.syncStamp; this.syncedRows[this.syncedCount++] = v;
  }
  // Read the publication before a synchronous hook can inspect its owner.
  sync() { this.syncState(); }

  // Block counts and word addresses are integers in the wasm32 ABI. Keep
  // them integral in JS too, avoiding boxed doubles at decoder call sites.
  // Generated delta masks address retained numeric rows, not JS properties.
  // Resolve leases first so hull and unit references can name new occupancies;
  // then update hulls, life metadata, weapons, poses and rider bodies in order.
  syncState() {
    const b = this.bridge; b.views();
    if (this.descriptor === undefined) return;
    const d = this.descriptor;
    const version = b.f[d + PHASE.version];
    if (this.counters) this.counters.outputFields++;
    if (this.lastVersion === version) return;
    this.lastVersion = version;
    if (this.counters) this.counters.outputFields += PHASE.words - 1;
    this.seats.syncLeases();
    this.syncedCount = 0; this.syncStamp++;
    let base = (b.f[d + PHASE.hulls] / 8) | 0;
    if (this.counters) this.counters.outputBlock(b.f,base);
    if (base) {
      const f = b.f, k = KIND_TABLE[KIND_VEHICLE], rows = f[base] | 0, stride = f[base + BLOCK.stride] | 0, h = base + (f[base + BLOCK.heap] | 0);
      for (let i = 0; i < rows; i++) {
        const at = base + BLOCK.words + i * stride;
        const v = i < f[base + BLOCK.retired] ? this.pool.retire(f[at] >>> 0, f[at + 1] >>> 0) : this.pool.at(f[at] >>> 0, f[at + 1] >>> 0);
        this.synced(v);
        applyDelta(f, at, h, v.f2HullWords);
        k.sync(b, v.f2HullWords, 5, v.f2HullWords[2] | 0, v);
      }

    }
    base = (b.f[d + PHASE.pads] / 8) | 0;
    if (this.counters) this.counters.outputBlock(b.f,base);
    if (base) {
      const f = b.f, rows = f[base] | 0, stride = f[base + BLOCK.stride] | 0, h = base + (f[base + BLOCK.heap] | 0);
      for (let i = 0; i < rows; i++) {
        const pad = syncF2Pad(b, f, base + BLOCK.words + i * stride, h, this.padScratch);
        const slot = this.vehicles.slots[pad.index]; slot.remaining = pad.remaining; slot.vehicle = pad.vehicle;
      }

    }
    // Gameplay life is separate from slab identity. A respawn reuses the
    // Vehicle and target object, but changes generation, visibility and fixed
    // flight configuration. These records are omitted during ordinary motion.
    base = (b.f[d + PHASE.lives] / 8) | 0;
    if (this.counters) this.counters.outputBlock(b.f,base);
    if (base) {
      const f = b.f, rows = f[base] | 0, stride = f[base + BLOCK.stride] | 0, h = base + (f[base + BLOCK.heap] | 0);
      for (let i = 0; i < rows; i++) {
        const life = syncF2HullLife(b, f, base + BLOCK.words + i * stride, h, this.lifeScratch), v = life.vehicle;
        const changed = v.aiTarget.generation !== life.generation;
        v.aiTarget.generation = life.generation; v.root.visible = life.visible;
        if (v.flying && changed) {
          v.flightClass = life.flightClass; v.minSpeed = life.minSpeed; v.midSpeed = life.midSpeed;
          v.boostSpeed = life.boostSpeed; v.speed = life.speed; v.turnRate = life.turnRate;
          v.flightConfig = flightConfig(v.def, v.flightClass);
        }
      }

    }
    this.firing.syncDescriptor(b.f[d + PHASE.firing] >>> 0);
    this.bodiesOut();
    // poseOut retains only world-readable seat and animation scalars. The
    // skeleton and all rig transforms stay with the native phase.
    let o = (b.f[d + PHASE.poses] / 8) | 0;
    if (this.counters) this.counters.outputBlock(b.f,o);
    if (o) {
      const n = b.f[o] | 0, stride = b.f[o + 1] | 0, heap = o + (b.f[o + 2] | 0);
      for (let i = 0; i < n; i++) {
        const at = o + 5 + i * stride, v = this.pool.at(b.f[at], b.f[at + 1]);
        applyDelta(b.f, at, heap, v.f2PoseWords);
        const row = syncF2Pose(b, v.f2PoseWords, 5, v.f2PoseWords[2] | 0, v.f2Pose);
        v.age = row.age; v.walkPhase = row.walkPhase; v.walkBlend = row.walkBlend; v.visual.copy(row.visual);
        v.root.position.copy(row.visual);
        // Keep the root fields observable without invoking three.js Euler
        // callbacks. World consumers read native seat geometry; the page computes
        // its independent render rig from replicated state.
        const q = v.root.quaternion, r = v.root.rotation;
        q._x = row.rootQ.x; q._y = row.rootQ.y; q._z = row.rootQ.z; q._w = row.rootQ.w;
        r._x = row.rootEuler.x; r._y = row.rootEuler.y; r._z = row.rootEuler.z;
        for (let si = 0; si < v.seats.length; si++) {
          const s = v.seats[si], posed = row.seats[si];
          s.lastYaw = posed.lastYaw; s.yawRate = posed.yawRate;
          s.target = posed.hasTarget ? posed.target : null;
          let ai = 0;
          for (let wi = 0; wi < s.weapons.length; wi++) for (let aj = 0; aj < s.weapons[wi].aimers.length; aj++) {
            const a = s.weapons[wi].aimers[aj], p = posed.aimers[ai++]; a.yaw = p.yaw; a.pitch = p.pitch;
            if (p.inLimits !== undefined) a.inLimits = p.inLimits;
            a.onTarget = p.onTarget; a.facing.copy(p.facing); a.posed = p.posed;
          }
        }
        this.synced(v);
      }

    }
    this.seats.syncBodies();
    this.vehicles.acc = b.f[d + PHASE.acc]; this.vehicles.flightAcc = b.f[d + PHASE.flightAcc]; this.vehicles.time = b.f[d + PHASE.time];
    if (this.debug) for (let i = 0; i < this.syncedCount; i++) this.record(this.syncedRows[i]);
  }

  // Vehicle retirement releases identity only after the final mirror is read.
  // Outstanding references may inspect that retired object, while the native
  // slab generation prevents them from writing into the next occupant.
  retire(v) {
    if (!this.x.f2_retire(v.simSlot, v.simGen)) throw Error('stale F2 retirement');
    this.sync(); this.detector.retire(v); this.pool.identity.release(v);
    const index = this.vehicles.vehicles.indexOf(v);
    if (index >= 0) this.vehicles.vehicles.splice(index, 1);
    for (const slot of this.vehicles.slots) for (const [name, variant] of slot.variants) if (variant === v) slot.variants.delete(name);
    // Retirement is outside the owning phase. Prepare its replacement now,
    // so the later pad spawn never needs a constructor or rig import.
    this.prepareVariants();
  }

  // Vehicle.damage, setHealth, wake and attribution are immediate writes.
  // The native command validates both lifetimes and publishes before returning;
  // a hook can therefore inspect another command's result in the same tick.
  command(v, code, value = 0, attacker = null) {
    const owned = this.vehicles.f3;
    const owner = this.ownerScratch;
    putOrdnanceOwner(this.bridge, attacker, owner, 0, null);
    owned.beforeCommand(attacker, this.x.f2_command.length, 1);
    try {
      if (!this.x.f2_command(v.simSlot, v.simGen, v.aiTarget.generation, code, value, owner[0], owner[1], owner[2])) throw Error('stale F2 command');
    } finally {
      owned.afterCommand();
    }
  }

  // Debug snapshots describe ownership, not the import format. Constructor
  // code resolves paths once; hot debug checks follow those paths without
  // rebuilding nested arrays. Production ticks do not create these snapshots.
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
  // A shared vector can change in place, so storing its reference would miss
  // a mirror write. Save scalar values at the boundary and preserve Object.is
  // semantics for signed zero and NaN in the ownership detector.
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


  // CollisionWorld's enabled flags may change between vehicle phases. Safe
  // exits and synchronous damage must see them even before the first tick.
  // Retain the native byte view and copy only changed flags, including after
  // a hook disables a body or a lifecycle callback changes the body order.
  bodyFlags() {
    const c = this.collision, b = this.bridge;
    c.rayBodies();
    if (this.bodiesVersion !== c.bodiesVersion) this.bodiesIn();
    b.views();
    for (let i = 0; i < c.bodies.length; i++) {
      const enabled = c.bodies[i].enabled ? 1 : 0;
      if (b.u8[this.flags + i] === enabled) continue;
      b.u8[this.flags + i] = enabled;
    }

  }

  // CollisionWorld.moveBody leaves bounds, matrix and inverse for unported
  // readers. Copy those generated records directly. Scalar stores here avoid
  // boxing float arguments to Vector3.set on this frequently used boundary.
  bodiesOut() {
    if (!this.kernel) return;
    const ptr = this.bridge.f[this.descriptor + PHASE.bodies];
    this.bridge.views();
    const f = this.bridge.f, bodies = this.collision.rays.bodies;
    let o = (ptr / 8) | 0;
    const toggled = f[o + BODY_HEADER.toggled], placed = f[o + BODY_HEADER.placed];
    if (this.counters) this.counters.outputFields += BODY_HEADER.words + toggled * TOGGLE.words + placed * PLACED.words;

    o += BODY_HEADER.words;
    for (let i = 0; i < toggled; i++, o += TOGGLE.words) bodies[f[o + TOGGLE.kernel]].enabled = f[o + TOGGLE.enabled] !== 0;
    for (let i = 0; i < placed; i++, o += PLACED.words) {
      const body = bodies[f[o + PLACED.kernel]];
      const min = body.bounds.min, max = body.bounds.max;
      min.x = f[o + PLACED.min]; min.y = f[o + PLACED.min + 1]; min.z = f[o + PLACED.min + 2];
      max.x = f[o + PLACED.max]; max.y = f[o + PLACED.max + 1]; max.z = f[o + PLACED.max + 2];
      body.matrix.fromArray(f, o + PLACED.matrix);
      body.inverse.fromArray(f, o + PLACED.inverse);
    }
  }

  // Firing target facts still name canonical collision bodies while units
  // and building targets remain foreign to the hull phase.
  bodySlot(body) {
    return body ? this.bridge.refs[KIND_BODY].slotOf(body) : -1;
  }

  // VehicleWorld queries remain synchronous where unported units or damage
  // can affect the current phase. Query result objects are reused; hooks read
  // already-published state before making any nested authoritative command.
  handlers() {
    const w = this.world, vs = this.vehicles;
    const hullAt = (i) => vs.vehicles[i];
    const origin = new THREE.Vector3(), direction = new THREE.Vector3(), point = new THREE.Vector3();
    const aimPointResult = { position: point }, aimPointsResult = { points: this.aimPoints }, aimPointRows = [];
    const raycastResult = { found: true, distance: 0, point: MISS.point, normal: MISS.normal, terrain: false, body: -1 };
    return {
      vehRaycastUnits: (a) => {
        origin.copy(a.origin); direction.copy(a.direction);
        const hit = w.raycastUnits(origin, direction, a.far, this.unitHit);
        if (!hit) return MISS;
        raycastResult.distance = hit.distance; return raycastResult;
      },
      vehDamaged: (a) => {
        const v = hullAt(a.hull), amount = a.amount, attacker = a.attacker;
        this.hook(w.damaged, v, amount, attacker);
      },
      vehDestroyed: (a) => { const v = hullAt(a.hull); this.hook(w.destroyed, v); },
      vehExplosion: (a) => {
        const v = hullAt(a.hull);
        // At the hull's own position (the very vector, as the JavaScript
        // hands it) when its class sets no offset, else a point of its own.
        const def = a.final ? v.def.finalExplosion || v.def.explosion : v.def.explosion;
        const at = a.final || v.def.fx?.explosionOffset ? a.position.clone() : v.position;
        this.hook(w.explosion, def, at, v);
      },
      vehEffect: (a) => { const name = a.name, at = a.position.clone(); w.effect(name, at); },
      vehAimPoint: (a) => { const i = a.hull; vs.units.all()[i].aimPoint(point); return aimPointResult; },
      // Flight targeting asks lazy unit bounds in candidate order. Keep both
      // the result array and each Vector3 when a later query asks fewer points;
      // shrinking the active result must not discard their reusable storage.
      vehAimPoints: (a) => {
        const indices = a.indices, all = vs.units.all(), points = this.aimPoints;
        points.length = indices.length;
        for (let k = 0; k < indices.length; k++) all[indices[k]].aimPoint(points[k] = aimPointRows[k] ??= new THREE.Vector3());
        return aimPointsResult;
      },
    };
  }
}
