import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CollisionWorld } from '../../src/collision.js';
import { Vehicles } from '../../src/vehicles.js';
import { predictVehicle, predictionVehicle, hullState, predictedExactly } from '../../src/runtime/vehicle-prediction.js';
import { captureGroundCorrection, restoreGroundCorrection } from '../../src/runtime/network-entity-ground.js';
import { captureFlightCorrection } from '../../src/runtime/network-entity-flight.js';
import { fixture as flightFixture } from '../flight-fixture.js';

const box = (w, h, d, y = 0) => {
  const g = new THREE.BoxGeometry(w, h, d).translate(0, y, 0);
  return { positions: g.attributes.position.array, indices: g.index.array, polygons: new Uint32Array(g.index.count / 3) };
};

// Both synthetic and imported hulls use the same flat collision world. The
// imported model, class, suspension and animation data remain unmodified.
export function hullFromPack(data, buffers, name, options = {}) {
  const floor = box(20000, 2, 20000, -1);
  const collision = new CollisionWorld(floor.positions, floor.indices, floor.polygons, undefined, undefined, options.rays || null);
  const system = new Vehicles({ scene: new THREE.Scene(), camera: new THREE.PerspectiveCamera(50, 16 / 9, .1, 2000), collision,
    data: { ...data, blockers: null, spawns: [{ name: 'pad', matrix: new THREE.Matrix4().toArray(), team: 1, post: '', classes: { 1: name } }] },
    ...buffers, texture: () => null, effects: { play() { return null; } }, ordnance: { explosion() {} },
    flightEnvironment: options.environment || {} });
  const v = system.vehicles[0]; v.driver = {}; v.seatIndex = 0; v.captureTick();
  return { system, v, c: collision };
}

export function syntheticHull(kind = 'hover', options = {}) {
  let result;
  if (kind === 'flyer') result = flightFixture(undefined, 'ship', options.environment || {});
  else {
    const name = kind === 'wheeled' ? 'test_hailfire' : kind;
    const settings = { MaxHealth: 1000, ForwardSpeed: 12, ReverseSpeed: 6, StrafeSpeed: ['hover', 'springs'].includes(kind) ? 5 : 0,
      HealthType: kind === 'creature' ? 'animal' : 'vehicle', WaterDamageInterval: .05, WaterDamageAmount: 10 };
    const def = { model: 'hull', base: ['walker', 'creature'].includes(kind) ? 'walker' : 'hover', settings,
      seats: [{ name: 'BODY', settings: {}, weapons: [] }, { name: 'GUNNER', settings: { TurnRate: 1.25, PitchRate: .7 }, weapons: [] }], explosion: null };
    if (kind === 'springs') {
      Object.assign(settings, { GravityScale: 4, Traction: 20, VelocitySpring: 5, VelocityDamp: 2.5, OmegaXSpring: 3,
        OmegaXDamp: 3, OmegaZSpring: 5, OmegaZDamp: 1.5, LevelSpring: 2, LevelDamp: 1 });
      def.springs = [{ at: [-.8, .4, 1], radius: 1, length: .5, factors: [1, 1, -1, 1, -1, 1] },
        { at: [.8, .4, 1], radius: 1, length: .5, factors: [1, 1, -1, 1, 1, 1] },
        { at: [0, .8, -1], radius: 1.4, length: .5, factors: [1, 1, 2, 1, 0, 1] }];
    }
    result = hullFromPack({ models: { hull: { bones: [], parts: [] } }, collision: { hull: box(2, 2, 4, 1) }, classes: { [name]: def } },
      { buffer: new ArrayBuffer(0), indexBuffer: new ArrayBuffer(0) }, name, options);
    if (kind === 'wheeled') result.v.wheels = [-.8, .8].map(x => ({ x, radius: .35, phase: 0, previous: 0, tickFrom: 0,
      axis: new THREE.Vector3(1, 0, 0), bone: new THREE.Bone() }));
    if (result.v.walking) {
      result.v.pose = {}; result.v.clips = new Map([
        ['walk_leftfoot_rightfoot', { duration: .45, stride: .8 }], ['walk_rightfoot_leftfoot', { duration: .55, stride: 1.1 }],
      ]);
    }
  }
  const { v, system } = result;
  v.captureTick(); v.age = 3.25;
  if (options.legs) {
    v.def.fx = { legs: { rayLength: 1.2, width: 2, orientRoll: false, pairs: [
      { left: { at: [-.7, 0, 1] }, right: { at: [.7, .2, 1] } },
      { left: { at: [-.7, .15, -1] }, right: { at: [.7, 0, -1] } },
    ] } };
    if (options.sharedLegSide) v.legSide = [.7, -.3];
    else delete v.legSide;
  }
  if (options.disabled) v.body.enabled = false;
  if (options.deathRegion) system.inDeathRegion = () => true;
  if (options.deepWater) system.water = { level: 100, over: () => true };
  if (options.belowWorld) { v.position.y = -400; v.previous.copy(v.position); v.captureTick(); }
  if (options.crash) {
    v.flightState = 'Flying'; v.position.set(0, 1, 0); v.velocity.set(0, -70, 40); v.airspeed = 80;
    v.quaternion.setFromAxisAngle(new THREE.Vector3(1, 0, 0), Math.PI / 3); v.captureTick();
  }
  if (options.targets) {
    v.flightState = 'Flying'; v.position.set(0, 20, 0); v.velocity.set(0, 0, 50); v.airspeed = 50; v.captureTick();
    v.seats[0].weapons.push({ state: { lockTargetId: 71, lockGeneration: 6, lockElapsed: 9, lockComplete: true } });
    const target = (id, x, team, named = true) => ({ id, generation: 2, alive: true, team, position: new THREE.Vector3(x, 20, 200),
      ...(named ? { aimPoint(out) { return out.copy(this.position); } } : {}) });
    const targets = [v.aiTarget, target(21, 0, 1), target(22, -1, 2), target(23, 1, 2, false), { ...target(24, 0, 2), alive: false }];
    system.flightAssists = true;
    system.units = { all: () => targets, byId: id => targets.find(t => t.id === id), relation: (a, b) => a === b ? 1 : -1, remove() {} };
  }
  if (options.dead) v.alive = false;
  return result;
}

export function hullCommand(tick, flying = false, extra = {}) {
  return { sequence: tick, tick, epoch: 1, generation: 3,
    down: tick < 75 ? ['KeyW', ...(tick > 40 ? ['KeyD'] : []), ...(flying && tick > 30 ? ['ShiftLeft'] : [])]
      : tick < 125 ? ['KeyS', 'KeyA'] : tick < 155 ? ['Space'] : ['KeyW'],
    actions: flying && tick === 1 ? ['takeoff'] : flying && tick === 100 ? ['trick'] : [],
    look: tick > 12 ? [[(tick % 7 - 3) * 9.25, (tick % 11 - 5) * 3.75]] : [],
    mouseScale: tick % 13 === 0 ? 3 : .75, invertMouse: tick % 17 === 0,
    flightSettings: { sensitivity: tick % 19 === 0 ? .001 : tick % 29 === 0 ? 20 : .9, invert: tick % 23 === 0 }, ...extra };
}

export function hullPredictionCases() {
  const cases = ['hover', 'springs', 'wheeled', 'walker', 'creature', 'flyer'].map(kind => ({ name: `${kind}-rolling-history`, kind, ticks: 180, batch: 15, crew: true }));
  cases.push({ name: 'walker-deep-water', kind: 'walker', ticks: 36, batch: 9, options: { deepWater: true } },
    { name: 'walker-shared-leg-array', kind: 'walker', ticks: 20, batch: 5, options: { legs: true, sharedLegSide: true } },
    { name: 'walker-query-mutates-shared-leg-array', kind: 'walker', ticks: 20, batch: 5, mutateLegSide: true, options: { legs: true, sharedLegSide: true } },
    { name: 'walker-allocated-leg-array', kind: 'walker', ticks: 20, batch: 5, options: { legs: true } },
    { name: 'hover-death-region-disabled-body', kind: 'hover', ticks: 32, batch: 8, options: { deathRegion: true, disabled: true } },
    { name: 'creature-below-world', kind: 'creature', ticks: 20, batch: 5, options: { belowWorld: true } },
    { name: 'flyer-crash-keeps-authority', kind: 'flyer', ticks: 60, batch: 10, options: { crash: true } },
    { name: 'flyer-target-lock-and-shared-weapons', kind: 'flyer', ticks: 90, batch: 15, options: { targets: true } },
    { name: 'flyer-bounded-flight', kind: 'flyer', ticks: 180, batch: 20, options: { environment: { ceiling: 7,
      bounds: { min: [-10, -300, -10], max: [10, 7, 10] } } } },
    { name: 'sparse-unsent-and-late-input', kind: 'springs', from: 40, ticks: 110, batch: 0, sparse: true },
    { name: 'wrong-generation', kind: 'walker', ticks: 25, batch: 0, wrongGeneration: true },
    { name: 'empty-pending', kind: 'hover', from: 42, ticks: 0, batch: 0 },
    { name: 'dead-authority', kind: 'flyer', ticks: 5, batch: 0, options: { dead: true } });
  return cases;
}

const primitive = value => value === null || ['number', 'boolean', 'string', 'undefined'].includes(typeof value);
const primitives = object => Object.fromEntries(Object.entries(object).filter(([, value]) => primitive(value)));
const vectorFields = 'position previous velocity quaternion rotation matrix itemCenter tickFrom supportNormal flightStart flightEnd flightTrickRight flightPreviousQ flightTickQ flightStartQ flightTrickLoopQ flightTrickEndQ flightFrameQ flightLeanQ'.split(' ');
export function hullObservation(v, history = true) {
  if (!v) return null;
  const scalar = primitives(v);
  delete scalar.predictionTick;
  return { correction: Array.from(v.flying ? captureFlightCorrection(v) : captureGroundCorrection(v)), scalar,
    vectors: Object.fromEntries(vectorFields.filter(key => v[key]?.toArray).map(key => [key, v[key].toArray()])),
    seats: v.seats.map(primitives), wheels: v.wheels.map(primitives), legSide: v.legSide ? [...v.legSide] : null,
    weaponLocks: v.seats.map(seat => seat.weapons.map(w => ['lockTargetId', 'lockGeneration', 'lockElapsed', 'lockComplete'].map(key => w.state[key]))),
    predictionTick: v.predictionTick,
    path: history ? Array.from(v.predictionPath || [], ([tick, p]) => [tick, p.toArray()]) : null,
    states: history ? Array.from(v.predictionStates || [], ([tick, bytes]) => [tick, bytes && Array.from(bytes)]) : null };
}

export function assertPredictionClone(clone, source) {
  assert.equal(clone.predictionSource, source.predictionSource || source);
  assert.equal(Object.getPrototypeOf(clone), Object.getPrototypeOf(source));
  assert.equal(Object.getPrototypeOf(clone.system), clone.predictionSource.system);
  assert.equal(Object.getPrototypeOf(clone.system.collision), clone.predictionSource.system.collision);
  for (const key of ['body', 'root', 'bones', 'byName', 'probes', 'wallCaches', 'springCaches', 'clips', 'def']) assert.equal(clone[key], source[key], `${key} remains shared`);
  for (const [key, value] of Object.entries(source)) if (value?.isVector3 || value?.isQuaternion || value?.isMatrix4 || value?.isEuler) assert.notEqual(clone[key], value, `${key} owns its math state`);
  for (const key of ['hit', 'aimHit', 'unitHit', 'frameScratch']) if (source[key]) {
    assert.notEqual(clone[key], source[key]);
    for (const [field, value] of Object.entries(source[key])) if (value?.isVector3 || value?.isQuaternion) assert.notEqual(clone[key][field], value);
  }
  for (let i = 0; i < source.wheels.length; i++) { assert.notEqual(clone.wheels[i], source.wheels[i]); assert.equal(clone.wheels[i].bone, source.wheels[i].bone); }
  if (Array.isArray(source.legSide)) assert.equal(clone.legSide, source.legSide, 'existing plain leg arrays remain shared');
  for (let i = 0; i < source.seats.length; i++) { assert.notEqual(clone.seats[i], source.seats[i]); assert.equal(clone.seats[i].lease, null); assert.equal(clone.seats[i].weapons, source.seats[i].weapons); }
  assert.notEqual(clone.itemTarget, source.itemTarget); assert.notEqual(clone.itemTarget.center, source.itemTarget.center);
  assert.equal(clone.itemTarget.position, source.itemTarget.position); assert.equal(clone.itemTarget.velocity, source.itemTarget.velocity);
  assert.equal(clone.system.collision, predictionVehicle(clone.predictionSource).system.collision, 'one collision view per source');
}

// Observe the original collision owner, including the shared body's temporary
// exclusion. Inputs are copied before a callback can reuse query scratch.
export function observeHullQueries(source, onQuery = null) {
  const c = source.system.collision, events = [], originals = new Map(), observers = [];
  const units = source.system.units;
  const watch = (object, key, args) => {
    const original = object[key]; if (typeof original !== 'function') return;
    object[key] = function (...input) { const event = [key, ...args(input)]; events.push(event); onQuery?.(event, source); return original.apply(this, input); };
    observers.push(() => { object[key] = original; });
  };
  if (units) {
    const targets = units.all?.() || [];
    for (const target of targets) watch(target, 'aimPoint', () => [target.id]);
    for (const key of ['all', 'byId', 'relation']) watch(units, key, input => input);
  }
  const cacheIds = new Map([...source.wallCaches, ...source.springCaches].filter(Boolean).map((cache, i) => [cache, i]));
  const argsFor = (key, args) => key === 'cast' || key === 'castMoving' ? [args[0].toArray(), args[1].toArray(), args[2], args[4] === source.body]
    : key === 'gather' ? [args[0].toArray(), args[1], args[2], cacheIds.get(args[3]) ?? null]
      : key === 'terrainNear' ? [args[0].toArray(), args[1]] : key === 'ground' ? args.slice(0, 3)
        : key === 'overlapsSphere' ? [args[0].toArray(), args[1]] : [];
  for (const key of ['cast', 'castMoving', 'gather', 'ground', 'overlapsSphere', 'terrainNear']) if (typeof c[key] === 'function') {
    const original = c[key]; originals.set(key, original);
    c[key] = function (...args) {
      if (key !== 'terrainNear') assert.equal(this, c, `${key} retains the query callback owner`);
      const event = [key, ...argsFor(key, args), source.legSide ? [...source.legSide] : null]; events.push(event); onQuery?.(event, source);
      return original.apply(this, args);
    };
  }
  let enabled = source.body.enabled;
  Object.defineProperty(source.body, 'enabled', { configurable: true, get: () => enabled, set(value) { events.push(['enabled', value]); enabled = value; } });
  return { events, dispose() {
    for (const restore of observers) restore();
    for (const [key, original] of originals) c[key] = original;
    Object.defineProperty(source.body, 'enabled', { configurable: true, writable: true, enumerable: true, value: enabled });
  } };
}

export function runHullPredictionCase(scenario, { native = null, replay = null, create = null, onQuery = null, after = null, queries = true } = {}) {
  const fixture = create ? create() : syntheticHull(scenario.kind, scenario.options);
  const source = fixture.v, original = hullObservation(source), collider = source.body.matrix.toArray(), enabled = source.body.enabled;
  let legQueries = 0;
  const query = queries ? observeHullQueries(source, (event, owner) => {
    if (scenario.mutateLegSide && event[0] === 'cast' && ++legQueries % 3 === 0) {
      owner.legSide[0] += .03125; owner.legSide[1] = -owner.legSide[1] + .015625;
    }
    onQuery?.(event, owner);
  }) : null, observations = [], predictions = [];
  const from = scenario.from || 0, pending = Array.from({ length: scenario.ticks }, (_, i) => hullCommand(i + 1, source.flying));
  if (scenario.options?.targets) for (const c of pending) Object.assign(c, { down: [], look: [], actions: [1, 70].includes(c.tick) ? ['targetLock'] : [] });
  if (scenario.sparse) for (let i = pending.length - 1; i >= 0; i--) {
    if (i % 5 === 0) pending.splice(i, 1);
    else Object.assign(pending[i], { generation: i % 13 === 0 ? 99 : 3, sent: i % 9 !== 0, tick: Math.floor(i / 3) * 3 + 1,
      worldLook: [[i % 3, -.1]], mouseScale: i % 7 === 0 ? -1 : 2 });
  }
  if (scenario.wrongGeneration) for (const c of pending) c.generation = 99;
  let previous = source, tick = from, held = [], caught;
  try {
    const size = scenario.batch || Math.max(1, pending.length), batches = [];
    for (let at = 0; at < pending.length; at += size) batches.push(pending.slice(at, at + size));
    if (!batches.length) batches.push([]);
    for (const batch of batches) {
      if (scenario.crew && previous !== source) {
        source.seats[0].yawRate = tick / 17; source.seats[0].lastYaw = tick / 11;
        if (source.seats[1]) Object.assign(source.seats[1], { aimYaw: tick / 37, pitch: tick / 97, lease: { human: true } });
      }
      const authority = hullObservation(source), sharedLegSide = Array.isArray(source.legSide);
      const next = predictVehicle(previous, batch, tick, 3, held, { native, replay });
      if (next) {
        predictions.push(next); assertPredictionClone(next, previous);
        assert.deepEqual(source.body.matrix.toArray(), collider); assert.equal(source.body.enabled, enabled);
        const authorityAfter = hullObservation(source);
        if (sharedLegSide) authority.legSide = authorityAfter.legSide;
        if (source.flying) authority.weaponLocks = authorityAfter.weaponLocks;
        assert.deepEqual(authorityAfter, authority, `${scenario.name} authority preserves clone sharing`);
        if (previous !== source) {
          for (const [at, p] of previous.predictionPath) if (at > tick - 60) assert.equal(next.predictionPath.get(at), p, 'retained path entries keep identity');
          for (const [at, b] of previous.predictionStates) if (at > tick - 60) assert.equal(next.predictionStates.get(at), b, 'retained correction entries keep identity');
          if (scenario.crew) {
            assert.equal(next.seats[0].yawRate, source.seats[0].yawRate); assert.equal(next.seats[0].lastYaw, source.seats[0].lastYaw);
            if (source.seats[1]) assert.equal(next.seats[1].aimYaw, source.seats[1].aimYaw);
          }
        }
        if (!next.flying && next.predictionStates.has(next.predictionTick)) {
          const checkpoint = predictionVehicle(source); restoreGroundCorrection(checkpoint, hullState(next));
          assert.ok(predictedExactly(next, checkpoint, next.predictionTick));
          checkpoint.position.x += 1e-9; assert.equal(predictedExactly(next, checkpoint, next.predictionTick), false);
        }
        tick = next.predictionTick;
      }
      const observation = hullObservation(next);
      if (observation) { observation.authorityLegSide = source.legSide ? [...source.legSide] : null; observation.authorityWeaponLocks = hullObservation(source).weaponLocks; }
      observations.push(observation); after?.({ source, previous, prediction: next, observations, batch });
      previous = next || previous; held = batch.at(-1)?.down || held;
    }
  } catch (error) { caught = error; }
  finally { query?.dispose(); }
  if (caught) { fixture.system.dispose(); throw caught; }
  return { source, prediction: previous === source ? null : previous, predictions, original, observations, events: query?.events || [],
    dispose() { for (const v of predictions) v.nativeSession?.dispose?.(); fixture.system.dispose(); } };
}
