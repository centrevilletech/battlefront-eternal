import { KIND_UNIT, KIND_UNITS, KIND_VEHICLE_TARGET, KIND_TURRET_TARGET, KIND_OBJECTIVE_TARGET,
  F3SensorMirror, F3SensorResultMirror, F3ShapeViewMirror, F3SensorShapeMirror, HitShapeMirror,
  F3PropFactMirror, F3TargetBoxMirror, F3RelationFactMirror, putF3RelationFact, putF3PropFact, syncF3SensorResult, syncF3ShapeView, syncF3BoundsView, refreshF3BoundsView, F3_BOUNDS_VIEW_REFRESH_WORDS } from './sim/gen/index.js';
import { F3ForeignHeaderMirror, putF3ForeignHeader } from './sim/gen/index.js';
import { F3BoundsViewLayout as V, F3UnitKeyLayout as K, F3SensorLayout as S, putF3Sensor_mode, putF3Sensor_key, putF3Sensor_origin,
  putF3Sensor_direction, putF3Sensor_far, putF3Sensor_radius, putF3Sensor_riders,
  putF3Sensor_masked, putF3Sensor_exclude0, putF3Sensor_exclude1 } from './sim/gen/units.js';
import { makeUnitHit } from './registry.js';

import {BlockHeaderLayout as BLOCK} from './sim/gen/boundary.js';
const HEADER = BLOCK.words;
import { categoryOf } from './ai/classic/weapons-ai.js';

const MEMBER_KINDS = [KIND_UNIT, KIND_VEHICLE_TARGET, KIND_TURRET_TARGET, KIND_OBJECTIVE_TARGET];
const ZERO = Object.freeze({x:0,y:0,z:0});

// Units keeps ordinary object identities and list storage. The canonical list
// and shape queries live in the instance; JS predicates only filter sensors.
export class F3Registry {
  // Units owns one mixed insertion sequence. The JS list keeps stable object
  // identities for AI and capture readers, while native commands own membership.
  // A corpse returned to UnitFactory's pool remains in that sequence.
  constructor(phase, units) {
    this.phase = phase;
    this.units = units;
    this.request = new F3SensorMirror();
    this.result = new F3SensorResultMirror();
    this.fact = new F3PropFactMirror();
    this.box = new F3TargetBoxMirror();
    this.boxes = [];
    this.relationFact = new F3RelationFactMirror();
    this.defaultHit = makeUnitHit();
    this.defaultQuery = [];
    this.facts = [];
    this.foreignMembers = [];
    this.readDepth = 0;
    this.readReady = false;
    this.foreignHeader = new F3ForeignHeaderMirror();
    this.mask = new Uint8Array(phase.layout.inputCapacity);
    const b = phase.bridge;
    phase.one[0] = units; b.writeSome(KIND_UNITS, phase.one);
    // The registry may already contain hulls, objectives and the dormant
    // viewpoint. Reserve their native history frames before the first capture.
    phase.x.f3_registry_prepare();
    phase.counters.wasmCalls++; phase.counters.lifecycleCalls++;
    const remove = units.remove;
    // Units.add assigns one id namespace across soldiers, hulls and props.
    // Kind-specific slab slots therefore cannot substitute for registry ids.
    // Register the mirror before any query can return this new member.
    units.add = member => {
      let kind = 0;
      for (let i = 0; i < MEMBER_KINDS.length; i++) {
        const k = MEMBER_KINDS[i]; if (b.refs[k].test?.(member)) { kind = k; break; }
      }
      if (!kind) throw Error('unknown F3 registry member');
      const id = phase.x.f3_registry_add(kind, 0);
      member.id = id; member.generation ||= 0;
      units.list.push(member); units.byIdMap.set(id, member); units.nextId = id + 1;
      b.refs[kind].slotOf(member);
      if (kind !== KIND_UNIT) this.prepareFact(id);
      phase.counters.wasmCalls++; phase.counters.lifecycleCalls++; phase.counters.inputFields += 2;
      return member;
    };
    // Existing hull and objective targets precede UnitFactory construction.
    // Reserve their changed-fact records now; a later sensor never allocates one.
    for (let i = 0; i < units.list.length; i++) {
      const member = units.list[i]; if (!member.soldier) this.prepareFact(member.id);
    }
    this.relations();
    // A relation mutation is rare and synchronous. Expand the table here so
    // later launcher scans read numeric pairs without boundary calls per target.
    const setTeams = units.setTeams;
    units.setTeams = table => { setTeams.call(units, table);
    this.relations(); };
    const setRelation = units.setRelation;
    units.setRelation = (a, c, value) => { setRelation.call(units, a, c, value);
    this.relations(); };
    units.remove = member => {
      phase.x.f3_registry_remove(member.id); remove.call(units, member);
      const at = this.foreignMembers.indexOf(member);
      if (at >= 0) this.foreignMembers.splice(at,1);
    };
    units.query = (center, radius, out = this.defaultQuery, filter = null) => this.query(center, radius, out, filter);
    units.raycast = (origin, direction, far, exclude = null, out = this.defaultHit, filter = null, riders = false) => this.raycast(origin, direction, far, exclude, out, filter, riders);
  }

  // Mission relations and explicit alliances are construction/change commands.
  // Rust sensors never need to format pair keys while scanning candidates.
  relations() {
    const p = this.phase, b = p.bridge, row = this.relationFact;
    const count = Math.max(6, this.units.teams?.relations?.length ?? 0);
    const words = count * count * p.layout.relationWords;
    const ptr = p.x.sim_call_room(words, 0); b.views();
    let at = ptr / 8 + HEADER;
    for (let a = 0; a < count; a++) for (let c = 0; c < count; c++) {
      row.a = a; row.b = c; row.value = this.units.relation(a, c);
      putF3RelationFact(b, row, b.f, at, p.heap); at += p.layout.relationWords;
    }
    p.x.f3_relations(count * count);
  }

  prepareFact(id) {
    const member = this.units.byId(id);
    if (!this.facts[id]) this.foreignMembers.push(member);
    const fact = this.facts[id] = new F3PropFactMirror();
    // Registry target classes do not change while their pooled object lives.
    // Resolve category once rather than coercing a health-type name per tick.
    fact.id = -1; fact.category = categoryOf(member); fact.kind = member.kind ?? 0;
    this.boxes[id] = new F3TargetBoxMirror();
  }

  // F3Props and F3Hulls attach after registry construction. Once those
  // owners exist, keep their targets out of every foreign-fact poll too.
  // Late members still pass the ownership check in foreign until prepared.
  prepareOwned() {
    const p=this.phase,list=this.foreignMembers;let count=0;
    for(let i=0;i<list.length;i++) {
      const member=list[i];
      if (!p.props.members[member.id] && !p.hulls.members[member.id]) list[count++]=member;
    }
    while(list.length>count)list.pop();
  }

  // Game.step's AI update makes several sensors over the same foreign
  // prop facts. Poll once in that span; native prop mirror writes invalidate
  // it immediately, and each later world phase starts with a fresh span.
  beginReads() { this.readDepth++; this.readReady = false; }
  endReads() { this.readDepth--; this.readReady = false; }
  invalidateReads() { this.readReady = false; }

  // Unit.hitShapes has a small fixed maximum, including exposed rider limbs.
  // Keep both mirror arrays across posture changes and pooled respawns. Their
  // nested vectors must stay identical to those already held by readers.
  prepare(binding) {
    const unit = binding.unit;
    const view = binding.shapes = new F3ShapeViewMirror();
    view.key = binding.view.key; view.bounds = unit.bounds;
    binding.sensorShapes = []; binding.hitList = [];
    binding.shapesTick = -1;
    for (let i = 0; i < 10; i++) binding.sensorShapes.push(new F3SensorShapeMirror());
    view.shapes = [];
    unit.riderHits = [];
    for (let i = 0; i < 10; i++) unit.riderHits.push(new HitShapeMirror());
  }

  // Units.query and launcher sensors see still-foreign targets too. Send only
  // changed facts, and never serialize a soldier body through this block.
  // Bounds are existing simulation facts; this path performs no JS rig math.
  foreign(phaseUnits = -1, networks = 0) {
    // Units.query and history capture need changed foreign targets only.
    // Infantry never enters this cold membership list, so an AI query does
    // not scan the entire native-owned fleet to discover the same few props.
    const phase = this.phase, b = phase.bridge, list = this.foreignMembers, fact = this.fact;
    const deferred = phaseUnits >= 0;
    if (!deferred && this.readDepth && this.readReady) return;
    b.views(); let count = 0, at = phase.layout.input / 8 + (deferred ? phaseUnits * phase.layout.inputStride + networks * phase.layout.networkWords : 0);
    for (let i = 0; i < list.length; i++) {
      const member = list[i];
      if (member.soldier || phase.props.members[member.id] || phase.hulls.members[member.id]) continue;
      const prior = this.facts[member.id], sphere = member.sphere, center = sphere.center;
      const alive = member.alive, dead = member.dead, team = member.team;
      const health = member.health ?? member.combat?.health ?? 1;
      const category = prior.category, kind = prior.kind, life = member.generation ?? 0;
      const position = member.position ?? center;
      const velocity = member.velocity ?? ZERO;
      const rollRate = (member.vehicle ?? member).flightRollRate || 0;
      const vehicle = member.vehicle, body = vehicle?.body ?? member.turret?.bodies?.built ?? member.building?.bodies?.built ?? null;
      const box = vehicle?.bounds && vehicle.matrix ? this.box : null;
      // A hull's sphere can remain unchanged while its box turns. Track its
      // existing matrix as well so lockSpan cannot retain the previous angle.
      let matrixChanged = Boolean(box) !== Boolean(prior.box);
      if (box) {
        box.min = vehicle.bounds.min; box.max = vehicle.bounds.max; box.matrix = vehicle.matrix;
        if (!matrixChanged) for (let k = 0; k < 16; k++) {
          if (!Object.is(prior.box.matrix.elements[k], box.matrix.elements[k])) { matrixChanged = true; break; }
        }
      }
      if (prior.id === member.id && prior.alive === alive && prior.dead === dead && Object.is(prior.team, team) &&
          Object.is(prior.health, health) && prior.category === category && prior.kind === kind && prior.life === life && prior.body === body && !matrixChanged &&
          Object.is(prior.velocity.x, velocity.x) && Object.is(prior.velocity.y, velocity.y) && Object.is(prior.velocity.z, velocity.z) && Object.is(prior.rollRate, rollRate) &&
          Object.is(prior.position.x, position.x) && Object.is(prior.position.y, position.y) && Object.is(prior.position.z, position.z) &&
          Object.is(prior.bounds.center.x, center.x) && Object.is(prior.bounds.center.y, center.y) && Object.is(prior.bounds.center.z, center.z) && Object.is(prior.bounds.radius, sphere.radius)) continue;
      fact.id = member.id; fact.alive = alive; fact.dead = dead; fact.team = team; fact.bounds = sphere;
      fact.health = health; fact.category = category; fact.kind = kind; fact.life = life;
      fact.position = position; fact.aim = center; fact.body = body; fact.box = box;
      fact.velocity = velocity; fact.rollRate = rollRate;
      putF3PropFact(b, fact, b.f, at, phase.heap); at += phase.foreignStride; count++;
      prior.id = member.id; prior.alive = alive; prior.dead = dead; prior.team = team;
      prior.health = health; prior.category = category; prior.kind = kind; prior.life = life; prior.body = body;
      prior.position.copy(position); prior.bounds.center.copy(center); prior.bounds.radius = sphere.radius;
      prior.velocity.copy(velocity); prior.rollRate = rollRate;
      // The saved box owns its comparison vectors. Aliasing the live matrix
      // here would hide a later turn before the next changed-fact comparison.
      prior.box = box ? this.boxes[member.id] : null;
      if (box) { prior.box.min.copy(box.min); prior.box.max.copy(box.max); prior.box.matrix.copy(box.matrix); }
    }
    const buildings = phase.liveOwner ? 0 : phase.props.foreign(at);
    this.foreignHeader.count = count; this.foreignHeader.buildings = buildings; this.foreignHeader.network = networks;
    putF3ForeignHeader(b, this.foreignHeader, b.f, phase.layout.foreignHeader / 8, phase.heap);
    phase.counters.inputFields += buildings * phase.layout.buildingStateWords;
    if (deferred) {
      // An AI query consumes its facts synchronously. Ordinary ticks instead
      // return this changed span beside intents through the single phase call.
      phase.counters.inputFields += count * phase.foreignStride + phase.layout.foreignHeaderWords;
      return;
    }
    if (this.readDepth) this.readReady = true;
    if (!count && !buildings) return;
    phase.x.f3_foreign(count);
    phase.counters.wasmCalls++; phase.counters.inputFields += count * phase.foreignStride + 1;
  }

  // Sensor answers use their own block, separate from body publication.
  // An AI query made by the synchronous aimer cannot overwrite pending mirrors.
  call() {
    const p = this.phase, b = p.bridge;
    b.views(); p.heap.clear();
    const r = this.request, f = b.f, command = p.layout.command / 8, heap = p.heap;
    putF3Sensor_mode(b,r,f,command,heap);
    let words = S.modeWords;
    // Units.query consumes only its sphere; shape observations consume a
    // qualified key. Avoid reading and encoding unused ray and life fields.
    // Rust ignores those sibling fields for the selected sensor operation.
    if (r.mode === 'query') {
      putF3Sensor_origin(b,r,f,command,heap);putF3Sensor_radius(b,r,f,command,heap);
      words += S.originWords + S.radiusWords;
    } else if (r.mode === 'raycast') {
      putF3Sensor_origin(b,r,f,command,heap);putF3Sensor_direction(b,r,f,command,heap);
      putF3Sensor_far(b,r,f,command,heap);putF3Sensor_riders(b,r,f,command,heap);
      putF3Sensor_masked(b,r,f,command,heap);
      putF3Sensor_exclude0(b,r,f,command,heap);putF3Sensor_exclude1(b,r,f,command,heap);
      words += S.originWords + S.directionWords + S.farWords + S.ridersWords +
        S.maskedWords + S.exclude0Words + S.exclude1Words;
    } else {
      putF3Sensor_key(b,r,f,command,heap);words += S.keyWords;
    }
    const at = p.x.f3_sensor() / 8;
    if (!at) throw Error('native unit sensor rejected a stale life');
    b.views(); p.counters.wasmCalls++; p.counters.inputFields += words;
    p.counters.outputFields += b.f[at] * b.f[at + BLOCK.stride] + b.f[at + BLOCK.heapWords] + HEADER;
    return at;
  }

  // Unit.sphere still observes the canonical lazy shape cache, but AI
  // bounds readers need no limb list copied and decoded into JavaScript.
  bounds(unit) {
    const p = this.phase, b = p.bridge, binding = p.bindings[unit.id], r = this.request;
    const view = binding.shapes, key = binding.view.key;
    b.views();
    const cached = (p.layout.boundsCache / 8 + unit.id * V.words) | 0;
    const tick = unit.id < p.layout.boundsCapacity ? b.f[cached + V.tick] : NaN;
    p.counters.outputFields++;
    // Native history and earlier bounds queries retain the same lazy sample.
    // Native ticks invalidate it even before public Unit tick stamps catch up.
    if (tick === unit.tickCount) {
      const slot=b.f[cached + V.key + K.slot], generation=b.f[cached + V.key + K.generation],
        life=b.f[cached + V.key + K.life];
      p.counters.outputFields += K.words;
      if (slot === unit.id && generation === key.generation && life === unit.generation) {
        refreshF3BoundsView(b,b.f,cached,0,view);
        p.counters.outputFields += F3_BOUNDS_VIEW_REFRESH_WORDS;
        view.tick=tick;unit.spheresTick=tick;
        return unit.bounds;
      }
    }
    r.mode = 'bounds'; r.key = key;
    const at = this.call();
    syncF3BoundsView(b, b.f, at + HEADER, at + b.f[at + BLOCK.heap], view);
    unit.spheresTick = view.tick;
    return unit.bounds;
  }

  // Unit.sphere/hitShapes is an observation boundary, not a body write.
  // Native tickCount decides whether its cache is stale; publication does not.
  shapes(unit) {
    const p = this.phase, b = p.bridge, binding = p.bindings[unit.id], r = this.request;
    r.mode = 'shapes'; r.key = binding.view.key;
    const at = this.call(), view = binding.shapes;
    // Restore the reusable capacity after a shorter previous shape set.
    for (let i = 0; i < 10; i++) view.shapes[i] = binding.sensorShapes[i];
    syncF3ShapeView(b, b.f, at + HEADER, at + b.f[at + BLOCK.heap], view);
    const n = view.shapes.length, hits = n > 2 ? unit.riderHits : unit.shapes;
    for (let i = 0; i < n; i++) {
      const source = view.shapes[i], hit = hits[i];
      hit.type = source.type; hit.part = source.part; hit.center.copy(source.center); hit.radius = source.radius;
      hit.halfSize.copy(source.halfSize); hit.half = source.halfIsSize ? hit.halfSize : source.half; hit.yaw = source.yaw;
      binding.hitList[i] = hit;
    }
    binding.hitList.length = n; unit.hitSet = hits; unit.shapeCount = n; unit.spheresTick = view.tick; binding.shapesTick = view.tick;
    return binding.hitList;
  }

  // Units.query observes broadphase spheres before invoking its predicate.
  // The predicate remains JS AI policy and sees stable member objects in order.
  query(center, radius, out, filter) {
    const r = this.request, p = this.phase, b = p.bridge;
    this.foreign(); r.mode = 'query'; r.origin = center; r.radius = radius;
    const at = this.call(); syncF3SensorResult(b, b.f, at + HEADER, at + b.f[at + BLOCK.heap], this.result);
    while (out.length) out.pop();
    const members = this.result.members;
    for (let i = 0; i < members.length; i++) { const member = members[i]; if (!filter || filter(member)) out.push(member); }
    return out;
  }

  // Units.raycast rejects flags and its predicate before asking for shapes.
  // Stage the mask outside module memory: a predicate can issue another sensor
  // request, which reuses the input reservation but must not erase this mask.
  raycast(origin, direction, far, exclude, out, filter, riders) {
    const p = this.phase, b = p.bridge, list = this.units.list, r = this.request;
    r.exclude0 = r.exclude1 = null;
    const pair = Array.isArray(exclude);
    const masked = Boolean(filter || pair && exclude.length > 2);
    if (!masked) {
      // Game.los names at most two endpoints. Resolve their live identities
      // directly; Rust already owns the alive, seat and corpse tests.
      const a = pair ? exclude[0] : exclude, c = pair ? exclude[1] : null;
      if (a?.soldier && p.bindings[a.id]?.unit === a) r.exclude0 = a;
      if (c?.soldier && p.bindings[c.id]?.unit === c) r.exclude1 = c;
    } else {
      b.views();
      for (let i = 0; i < list.length; i++) {
        const unit = list[i]; if (!unit.soldier) continue;
        let accepted = Boolean((unit.alive || unit.dead && !unit.removed) && (!unit.seat || riders && unit.seat.exposed));
        if (unit === exclude || pair && exclude.includes(unit)) accepted = false;
        if (accepted && filter && !filter(unit)) accepted = false;
        this.mask[unit.id] = accepted ? 1 : 0;
      }
      b.views();
      for (let i = 0; i < this.units.nextId; i++) b.f[p.layout.input / 8 + i] = this.mask[i];
      p.counters.inputFields += this.units.nextId;
      r.exclude0 = r.exclude1 = null;
    }
    r.masked = masked;
    r.mode = 'raycast'; r.origin = origin; r.direction = direction; r.far = far; r.riders = riders;
    const at = this.call(); syncF3SensorResult(b, b.f, at + HEADER, at + b.f[at + BLOCK.heap], this.result);
    const hit = this.result.hit;
    if (!hit.found) return null;
    out.unit = hit.unit; out.distance = hit.distance; out.part = hit.part; out.point.copy(hit.point); out.normal.copy(hit.normal);
    return out;
  }
}
