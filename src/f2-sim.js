// One whole ground/flight phase. Ordinary ticks import foreign inputs only.
import * as THREE from 'three';
import { applyDelta, deltaBuffer } from './f2-delta.js';
import { F2Seats } from './f2-seats.js';
import { F2Firing } from './f2-firing.js';
import { F2Riders } from './f2-riders.js';
import { makeGround, makeHit } from './collision.js';
import { Weapon } from './weapons.js';
import { FLIGHT_CLASSES, flightConfig } from './flight/config.js';
import { F2HullLifeMirror, F2PadMirror, syncF2HullLife, syncF2Pad, putF2Bodies, F2PhaseFlag_INDEX, F2ColliderHeaderLayout as COLLIDER, F2BodyHeaderLayout as BODY_HEADER, F2BodyToggleLayout as TOGGLE, F2BodyPlacedLayout as PLACED, F2PhaseLayout as PHASE, F2PoseMirror, F2PoseSeatMirror, F2AimerMirror, syncF2Pose, F2InputLayout, F2FlightInLayout, F2FlightInMirror, putF2FlightIn, F2MemberLayout, F2PadTeamLayout, F2InputMirror, F2MemberMirror, putF2Input, putF2Member, putF2PadTeam } from './sim/gen/f2.js';
import { MirrorPool } from './sim/runtime.js';
import { KIND_TABLE, KIND_VEHICLE, KIND_VEHICLES, KIND_BODY, VEHICLE_KEYS } from './sim/gen/index.js';
import { json, classKeys, giveMapRules, pullPose, flightKeys } from './vehicle-phase-data.js';
import { putOrdnanceOwner } from './sim/gen/ordnance.js';
import { ordnanceTests } from './ordnance-sim.js';
import { F2Counters, F2MirrorCheck, countedF2Exports } from './f2-contract.js';
const MISS = { found: false, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: -1 };
const NO_GROUND = { found: false, height: 0, normal: new THREE.Vector3() };
const NO_INPUT = {};
const EMPTY_UNITS = [];
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
// Page prediction creates private Vehicle copies with no F2 command owner, so
// its existing JS methods cannot mutate this world's canonical fleet.
export function attachF2(vehicles, bridge, map, pack, unitClass, debug = false) {
  const phase = new F2(vehicles, bridge, map, pack, unitClass, debug);
  vehicles.f2 = phase;
  return phase;
}

class F2 {
  // Vehicle construction is the only full hull import. Immutable class keys,
  // rig topology, collision mappings and definition handles are resolved here.
  // Ordinary ticks have a separate generated input layout containing foreign
  // facts, rather than a snapshot of the Vehicle objects.
  constructor(vehicles, bridge, map, pack, unitClass, debug) {
    bridge.identify({ tests: ordnanceTests });
    this.vehicles = vehicles; this.bridge = bridge;
    this.counters = new F2Counters(); this.x = countedF2Exports(bridge.x, this.counters);
    this.world = vehicles.world; this.collision = vehicles.collision;
    this.unitClass = unitClass; this.debug = debug;
    this.detector = new F2MirrorCheck();
    this.kernel = false; this.bodiesVersion = -1; this.colliders = new WeakSet();
    this.mirrorRows = new WeakMap(); this.syncedRows = []; this.syncedCount = 0; this.syncStamp = 0;
    this.lifeScratch = new F2HullLifeMirror(); this.padScratch = new F2PadMirror();
    this.ownerScratch = new Float64Array(3);
    this.classes = new Set(); this.teams = []; this.teamCount = 0;
    this.flightIn = new F2FlightInMirror(); this.flightScratch = new Float64Array(F2FlightInLayout.words); this.flightCache = new WeakMap();
    this.inputHeader = new F2InputMirror(); this.memberRow = new F2MemberMirror();
    this.memberCache = new Float64Array(8192 * F2MemberLayout.words); this.memberCache.fill(NaN);
    this.memberScratch = new Float64Array(F2MemberLayout.words); this.memberLength = 0; this.padTeam = { hasTeam: false, team: 0 };
    this.padPosts = vehicles.slots.map(slot => slot.desc.post ? vehicles.posts?.posts.find(p => p.desc.name.toLowerCase() === slot.desc.post) : null); this.memberPoint = new THREE.Vector3(); this.aimPoints = [];
    this.hit = makeHit(); this.groundHit = makeGround();
    this.unitHit = { unit: null, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), part: null };
    this.flag = { value: false }; this.number = { value: 0 };
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
    this.bodyArgs = { words: this.x.f2_body_words(), put: putF2Bodies }; this.bodyRefs = { bodies: [] };
    for (const v of vehicles.vehicles) this.adopt(v);
    this.bodiesIn();
    const ptr = this.x.f2_init(bridge.strings.id(json(vehicles.slots.map(s => s.desc))), vehicles.time, vehicles.acc);
    bridge.views(); this.descriptor = (ptr / 8) | 0;
    this.inputBase = (bridge.f[this.descriptor + PHASE.input] / 8) | 0;
    this.capacity = bridge.f[this.descriptor + PHASE.capacity];
    bridge.reserve(16 * 1024 * 1024);
    this.bodyFlags();
    this.counters.reset();
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
    this.x.f2_register(v.simSlot, v.simGen);
    const values = pullPose(v), at = this.input(values.length);
    b.f.set(values, at);
    if (!this.x.veh_pull(slot, v.aiTarget.generation, values.length)) throw Error('F2 could not retain a vehicle rig');
    const flightCache = new Float64Array(F2FlightInLayout.words); flightCache.fill(NaN); this.flightCache.set(v, flightCache);
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
    const b = this.bridge, vs = this.vehicles;
    if (this.debug) this.check();
    this.bodyFlags();
    this.foreignIn(true);
    this.seats.prepare();
    this.counters.ticks++;
    const memory = b.memory.buffer;
    this.x.f2_step(dt, tick);
    if (this.debug && b.memory.buffer !== memory) throw Error('F2 memory grew during the phase');
    this.sync();
    vs.tickAlpha = Math.max(0, vs.acc / (1 / 120)); vs.presented = false;
    if (this.debug) this.check();
  }

  // Units.relation uses null as a distinct team key and preserves insertion
  // order. Keep the backing array and an active count: truncating it to zero
  // would allocate storage again when the next tick fills it.
  addTeam(team) {
    for (let i = 0; i < this.teamCount; i++) if (Object.is(this.teams[i], team)) return;
    this.teams[this.teamCount++] = team;
  }
  // units.all is still foreign until AI moves. Keep its stable rows and
  // import only changed facts; native hull rows are never input snapshots.
  foreignIn(phaseStart = false) {
    const vs = this.vehicles, b = this.bridge, refs = b.refs, p = this.memberPoint;
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
    // Registry order matters for first-equal target selection. Cache numeric
    // rows by position, including generation and whether a team is present.
    // Hull entries keep handles; only foreign objects provide copied aim points.
    const all = vs.units?.all?.() ?? EMPTY_UNITS;
    h.memberLength = all.length; h.members = at - base; h.memberCount = 0;
    for (let i = 0; i < all.length; i++) {
      const t = all[i], hull = t.vehicle ?? null, current = hull && this.pool.objects[hull.simSlot] === hull;
      // Units and current hull targets have native query paths. A retired hull
      // still referenced by the registry becomes a foreign entry, rather than
      // being retargeted through the slot now occupied by its replacement.
      const kind = t instanceof this.unitClass ? 0 : current ? (hull.aiTarget === t ? 1 : 3) : 2;
      if (kind === 2) { if (t.aimPoint) t.aimPoint(p); else p.copy(t.position); }
      const row = this.memberRow;
      row.index = i; row.kind = kind; row.id = t.id; row.generation = t.generation ?? NaN;
      row.alive = Boolean(t.alive); row.hasTeam = t.team != null; row.team = t.team ?? 0;
      row.hull = hull ? refs[KIND_VEHICLE].slotOf(hull) : -1;
      row.point.set(kind === 2 ? p.x : 0, kind === 2 ? p.y : 0, kind === 2 ? p.z : 0);
      putF2Member(b, row, this.memberScratch, 0, null);
      let changed = i >= this.memberLength;
      const offset = i * F2MemberLayout.words;
      for (let j = 0; j < F2MemberLayout.words; j++) {
            const old = this.memberCache[offset + j], next = this.memberScratch[j];
            if (old === next ? old === 0 && 1 / old !== 1 / next : old === old || next === next) changed = true;
          }
      if (changed) {
        for (let j = 0; j < F2MemberLayout.words; j++) { f[at + j] = this.memberScratch[j];  }
        at += F2MemberLayout.words; h.memberCount++;
      }
    }
    // Relations are small mission data. Read each pair in the registry's team
    // order, including the fallback for worlds without a relation callback.
    // These values do not grant JS ownership of hull or lease state.
    const teams = this.teams; this.teamCount = 0;
    for (let i = 0; i < all.length; i++) this.addTeam(all[i].team ?? null);
    for (let i = 0; i < vs.vehicles.length; i++) this.addTeam(vs.vehicles[i].team ?? null);
    h.teams = at - base; h.teamCount = this.teamCount;
    for (let i = 0; i < this.teamCount; i++) { const row = this.padTeam; row.hasTeam = teams[i] !== null; row.team = teams[i] ?? 0; putF2PadTeam(b, row, f, at, null); at += F2PadTeamLayout.words; }
    h.relations = at - base;
    for (let i = 0; i < this.teamCount; i++) for (let j = 0; j < this.teamCount; j++) f[at++] = vs.units?.relation?.(teams[i], teams[j]) ?? (teams[i] === teams[j] ? 1 : -1);
    // Only network pilot keys remain foreign. Bot controls are frozen from
    // canonical leases inside the phase, after exits and promotion. Retain each
    // network row and send changes, including the transition back to no pilot.
    h.flights = at - base; h.flightCount = 0;
    for (let i = 0; i < vs.vehicles.length; i++) {
      const v = vs.vehicles[i]; if (!v.flying) continue;
      const l = v.seats[0]?.lease, network = l?.unit?.networkHuman && l.intent.flight;
      const row = this.flightIn; row.hull = v.simSlot; row.network = Boolean(network);
      flightKeys(row.remote, network || NO_INPUT);
      putF2FlightIn(b, row, this.flightScratch, 0, null);
      const cache = this.flightCache.get(v); let changed = false;
      for (let j = 0; j < cache.length; j++) {
        const old = cache[j], next = this.flightScratch[j];
        if (old === next ? old === 0 && 1 / old !== 1 / next : old === old || next === next) changed = true;
      }
      if (changed) { f.set(this.flightScratch, at); at += F2FlightInLayout.words; h.flightCount++; }
    }
    // Weapon modifiers and AI target hints use their own generated change rows.
    // Pressed, held, reload and aim are absent here: importing their JS mirrors
    // would restore pre-latch edges after the native seat phase consumed them.
    h.firing = at - base; at += this.firing.input(at); h.end = at - base;
    putF2Input(b, h, f, base, null);
    this.counters.inputFields += at - base;
    this.counters.inputRegistry += h.memberCount * F2MemberLayout.words;
    this.counters.inputFiring += h.end - h.firing;
    this.counters.inputFlight += h.flightCount * F2FlightInLayout.words;
    this.counters.inputWorld += h.end - h.memberCount * F2MemberLayout.words - (h.end - h.firing) - h.flightCount * F2FlightInLayout.words;
  }

  // A spawn may rebuild the pending input before native latching reads it.
  // Commit caches only at the first post-latch callback, so an earlier batch's
  // changes survive every lifecycle callback in the same phase.
  // The bytes here are the batch native code actually read. Committing the
  // current JS objects instead would hide deaths or AI edits made by later
  // callbacks in the same phase. Copy only the submitted changed records.
  commitForeign() {
    const b = this.bridge; b.views(); const f = b.f, base = this.inputBase, h = this.inputHeader;
    for (let i = 0; i < h.memberCount; i++) {
      const at = base + h.members + i * F2MemberLayout.words, offset = f[at + F2MemberLayout.index] * F2MemberLayout.words;
      for (let j = 0; j < F2MemberLayout.words; j++) this.memberCache[offset + j] = f[at + j];
    }
    this.memberLength = h.memberLength;
    for (let i = 0; i < h.flightCount; i++) {
      const at = base + h.flights + i * F2FlightInLayout.words, v = this.pool.objects[f[at + F2FlightInLayout.hull]];
      const cache = this.flightCache.get(v);
      for (let j = 0; j < cache.length; j++) cache[j] = f[at + j];
    }
    this.firing.commitInput(base + h.firing);
  }
  // Retain the backing array as well as the mirrors. Truncating an Array to
  // zero discards its storage in V8 even when it is filled again immediately.
  synced(v) {
    if (v.f2Synced === this.syncStamp) return;
    v.f2Synced = this.syncStamp; this.syncedRows[this.syncedCount++] = v;
  }
  // A synchronous hook may arrive through the shared simulation bridge.
  // Use this phase's counted export table only while reading its result, and
  // restore the previous table even if a gameplay callback throws.
  sync() {
    const b = this.bridge, prior = b.x; b.x = this.x;
    try { return this.syncState(); } finally { b.x = prior; }
  }

  // Block counts and word addresses are integers in the wasm32 ABI. Keep
  // them integral in JS too, avoiding boxed doubles at decoder call sites.
  // Generated delta masks address retained numeric rows, not JS properties.
  // Resolve leases first so hull and unit references can name new occupancies;
  // then update hulls, life metadata, weapons, poses and rider bodies in order.
  syncState() {
    const b = this.bridge; b.views();
    if (this.descriptor === undefined) return;
    const d = this.descriptor;
    this.seats.syncLeases();
    this.syncedCount = 0; this.syncStamp++;
    let base = (b.f[d + PHASE.hulls] / 8) | 0;
    if (base) {
      const f = b.f, k = KIND_TABLE[KIND_VEHICLE], rows = f[base] | 0, stride = f[base + 1] | 0, h = base + (f[base + 2] | 0);
      for (let i = 0; i < rows; i++) {
        const at = base + 5 + i * stride;
        const v = i < f[base + 4] ? this.pool.retire(f[at] >>> 0, f[at + 1] >>> 0) : this.pool.at(f[at] >>> 0, f[at + 1] >>> 0);
        this.synced(v);
        applyDelta(f, at, h, v.f2HullWords);
        k.sync(b, v.f2HullWords, 5, v.f2HullWords[2] | 0, v);
      }
      this.counters.outputFields += rows * stride + f[base + 3];
      this.counters.outputHulls += rows * stride + f[base + 3];
    }
    base = (b.f[d + PHASE.pads] / 8) | 0;
    if (base) {
      const f = b.f, rows = f[base] | 0, stride = f[base + 1] | 0, h = base + (f[base + 2] | 0);
      for (let i = 0; i < rows; i++) {
        const pad = syncF2Pad(b, f, base + 5 + i * stride, h, this.padScratch);
        const slot = this.vehicles.slots[pad.index]; slot.remaining = pad.remaining; slot.vehicle = pad.vehicle;
      }
      this.counters.outputFields += rows * stride + f[base + 3];
      this.counters.outputPads += rows * stride + f[base + 3];
    }
    // Gameplay life is separate from slab identity. A respawn reuses the
    // Vehicle and target object, but changes generation, visibility and fixed
    // flight configuration. These records are omitted during ordinary motion.
    base = (b.f[d + PHASE.lives] / 8) | 0;
    if (base) {
      const f = b.f, rows = f[base] | 0, stride = f[base + 1] | 0, h = base + (f[base + 2] | 0);
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
      this.counters.outputLives += rows * stride + f[base + 3];
    }
    this.firing.syncDescriptor(b.f[d + PHASE.firing] >>> 0);
    this.bodiesOut();
    // poseOut retains only world-readable seat and animation scalars. The
    // skeleton and all rig transforms stay with the native phase.
    let o = (b.f[d + PHASE.poses] / 8) | 0;
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
      this.counters.outputFields += n * stride + b.f[o + 3];
      this.counters.outputPoses += n * stride + b.f[o + 3];
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
  }

  // Vehicle.damage, setHealth, wake and attribution are immediate writes.
  // The native command validates both lifetimes and publishes before returning;
  // a hook can therefore inspect another command's result in the same tick.
  command(v, code, value = 0, attacker = null) {
    this.bodyFlags();
    const owner = this.ownerScratch;
    putOrdnanceOwner(this.bridge, attacker, owner, 0, null);
    if (!this.x.f2_command(v.simSlot, v.simGen, v.aiTarget.generation, code, value, owner[0], owner[1], owner[2])) throw Error('stale F2 command');
    this.sync();
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
  retune(name, key, value) {
    if (typeof value === "number") this.x.veh_flight_tune(this.bridge.strings.id(name), this.bridge.strings.id(key), value);
  }

  // Vehicle model and animation buffers are birth data. Preserve the JSON
  // number spelling used by the old vehicle bridge, including negative zero,
  // then let Rust retain the rigs instead of importing nodes every tick.
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

  // CollisionWorld.rayBodies establishes one stable order for native bodies.
  // The F2 mapping names existing opaque body objects; it does not recreate the
  // scene or let a stale hull command address a replacement collider.
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

  // CollisionWorld's enabled flags may change between vehicle phases. Safe
  // exits and synchronous damage must see them even before the first tick.
  // Retain the native byte view and copy only changed flags, including after
  // a hook disables a body or a lifecycle callback changes the body order.
  bodyFlags() {
    const c = this.collision, b = this.bridge;
    c.rayBodies();
    if (this.bodiesVersion !== c.bodiesVersion) this.bodiesIn();
    b.views();
    let changed = 0;
    for (let i = 0; i < c.bodies.length; i++) {
      const enabled = c.bodies[i].enabled ? 1 : 0;
      if (b.u8[this.flags + i] === enabled) continue;
      b.u8[this.flags + i] = enabled; changed++;
    }
    this.counters.inputFields += changed; this.counters.inputFlags += changed;
  }

  // Vehicle wall and spring gather caches are retained with its collider.
  // This is construction work, including cache registration that may grow WASM
  // memory. Ordinary phase queries use those native cache handles directly.
  collider(v) {
    const rays = this.collision.rays, body = v.body;
    // (Making a cache's copy may grow the memory: the ids first.)
    const walls = (v.wallCaches ?? []).map((cache) => (cache ? rays.cache(cache) : -1));
    const springs = (v.springCaches ?? []).map((cache) => (cache ? rays.cache(cache) : -1));
    const slot = this.bridge.refs[KIND_BODY].slotOf(body);
    const at = this.input(COLLIDER.words + walls.length + springs.length), f = this.bridge.f, b = body.localBounds;
    f[at + COLLIDER.body] = slot; f[at + COLLIDER.kernel] = body.raySlot;
    f[at + COLLIDER.min] = b.min.x; f[at + COLLIDER.min + 1] = b.min.y; f[at + COLLIDER.min + 2] = b.min.z;
    f[at + COLLIDER.max] = b.max.x; f[at + COLLIDER.max + 1] = b.max.y; f[at + COLLIDER.max + 2] = b.max.z;
    for (let i = 0; i < walls.length; i++) f[at + COLLIDER.words + i] = walls[i];
    for (let i = 0; i < springs.length; i++) f[at + COLLIDER.words + walls.length + i] = springs[i];
    this.x.veh_collider(walls.length, springs.length);
    this.colliders.add(body);
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
    this.counters.outputFields += toggled * TOGGLE.words + placed * PLACED.words;
    this.counters.outputCollision += toggled * TOGGLE.words + placed * PLACED.words;
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

  body(slot) {
    return slot < 0 ? null : this.bridge.refs[KIND_BODY].objects[slot];
  }

  bodySlot(body) {
    return body ? this.bridge.refs[KIND_BODY].slotOf(body) : -1;
  }

  // VehicleWorld queries remain synchronous where unported units or damage
  // can affect the current phase. Query result objects are reused; hooks read
  // already-published state before making any nested authoritative command.
  handlers() {
    const w = this.world, vs = this.vehicles;
    const hullAt = (i) => vs.vehicles[i];
    const origin = new THREE.Vector3(), direction = new THREE.Vector3(), position = new THREE.Vector3(), velocity = new THREE.Vector3();
    const previous = new THREE.Vector3(), q = new THREE.Quaternion(), own = new THREE.Quaternion(), matrix = new THREE.Matrix4();
    const center = new THREE.Vector3(), point = new THREE.Vector3();
    const probes = [];
    const aimPointResult = { position: point }, aimPointsResult = { points: this.aimPoints }, aimPointRows = [];
    const raycastResult = { found: true, distance: 0, point: MISS.point, normal: MISS.normal, terrain: false, body: -1 };
    const castResult = { found: false, distance: 0, point: this.hit.point, normal: this.hit.normal, terrain: false, body: -1 };
    const pushResult = { undo: false, position, velocity };
    const polygonResult = { polygons: [] }, polygonRows = [];
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
        const gathered = w.springPolygons(hull, index, center, radius), rows = polygonResult.polygons;
        rows.length = gathered.length;
        for (let i = 0; i < gathered.length; i++) {
          const p = gathered[i], row = polygonRows[i] ??= { id: 0, moving: false, triangles: null };
          row.id = p.id; row.moving = Boolean(p.moving); row.triangles = p.triangles; rows[i] = row;
        }
        return polygonResult;
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
        if (!hit) return MISS;
        raycastResult.distance = hit.distance; return raycastResult;
      },
      vehClock: () => { this.number.value = w.clock(); return this.number; },
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
