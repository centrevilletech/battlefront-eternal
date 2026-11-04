// Private hull replay on the battle's shared bridge. Queries are copied
// before entering foreign code, including code that grows memory or reenters.
import * as THREE from 'three';
import { bridgeFor, Heap } from '../sim/runtime.js';
import { KIND_BODY, KIND_VEHICLE_DRIVER } from '../sim/gen/kinds.js';
import { putVehicle, syncVehicle, VehicleMirror, VEHICLE_FULL_STRIDE, VEHICLE_EXPORT_STRIDE } from '../sim/gen/flight.js';

const dispatchers = new WeakMap();
let nextSession = 1;
const configKeys = ['minSpeed', 'cruiseSpeed', 'maxSpeed', 'acceleration', 'turnRate', 'turnFilter', 'pitchRate', 'pitchFilter',
  'bankAngle', 'bankFilter', 'levelFilter', 'takeoffHeight', 'takeoffTime', 'takeoffSpeed', 'landedHeight', 'landingTime', 'landingSpeed', 'boostSpeed'];
const tuningKeys = ['boostAcceleration', 'boostTurn', 'stickReturn', 'responseCurve', 'landingHeight', 'maxSlope', 'powerDrain', 'powerRecharge',
  'trickCost', 'trickCooldown', 'barrelDisplacement', 'missileTurnRate', 'missileSeeker', 'missileSpeed', 'missileLifetime', 'missileNavigation',
  'missileRollBreakRate', 'missileEvadeRange', 'barrelTime', 'loopTime', 'immelmannTime', 'trickWindow', 'terrainLookahead', 'recoverySharpness',
  'lethalImpactSpeed', 'lethalImpactCosine', 'scrapeDamageLimit', 'impactDamageSpeed', 'assistCone', 'assistStrength', 'stickyScale', 'cameraBlendTime',
  'lockRange', 'lockCone', 'retainCone', 'lockDwell', 'losGrace', 'rollRate'];
const groundKeys = ['position', 'previous', 'velocity', 'yaw', 'previousYaw', 'tilt', 'roll', 'previousTilt', 'previousRoll',
  'yawRate', 'tiltRate', 'rollRate', 'support', 'forwardInput', 'rightInput', 'sleeping', 'waterClock', 'quaternion', 'matrix',
  'itemCenter', 'supportNormal', 'legSide', 'tickFrom', 'tickFromYaw', 'tickFromTilt', 'tickFromRoll'];
const flightKeys = ['flightState', 'grounded', 'altitude', 'throttle', 'airspeed', 'stateTime', 'floorY', 'bank', 'power', 'stickX', 'stickY',
  'flightRollRate', 'rollLock', 'boosting', 'trick', 'trickTime', 'trickCooldown', 'trickPending', 'trickDirection', 'gear',
  'landingMessage', 'landingMessageTime', 'flightTrickRight', 'flightClock', 'boostExhausted', 'landingReason', 'flightWarning',
  'terrainWarning', 'flightAssistId', 'flightLockId', 'flightLockGeneration', 'flightLockTimer', 'flightLostLOS', 'flightLockState',
  'flightStart', 'flightEnd', 'flightStartQ', 'flightTrickLoopQ', 'flightTrickEndQ', 'flightPreviousQ', 'flightTickQ',
  'flightActionHeld', 'trickHeld', 'immelmannHeld', 'flightFrameQ'];
const nativeTexts = ['', 'barrel', 'loop', 'immelmann', 'Locking', 'Locked', 'Takeoff path blocked', 'Landing path blocked',
  'Too fast', 'No landing zone', 'Press SPACE to land', 'Flight ceiling', 'Leaving battlefield'];

function text(words, value) {
  words.push(value.length);
  for (let i = 0; i < value.length; i++) words.push(value.charCodeAt(i));
}
function definition(words, value) {
  if (value == null) words.push(0);
  else if (typeof value === 'boolean') words.push(value ? 2 : 1);
  else if (typeof value === 'number') words.push(3, value);
  else if (typeof value === 'string') { words.push(4); text(words, value); }
  else if (Array.isArray(value)) { words.push(5, value.length); for (const v of value) definition(words, v); }
  else {
    const entries = Object.entries(value).filter(([, v]) => v !== undefined && typeof v !== 'function');
    words.push(6, entries.length);
    for (const [key, v] of entries) { text(words, key); definition(words, v); }
  }
}
function identities() {
  const objects = [], ids = new WeakMap();
  return {
    objects,
    slotOf(object) { let id = ids.get(object); if (id === undefined) { id = objects.length; objects.push(object); ids.set(object, id); } return id; },
    put(object, words, at) { words[at] = this.slotOf(object); words[at + 1] = 0; },
    at(slot) { const object = objects[slot]; if (!object) throw Error(`no private hull reference ${slot}`); return object; },
    is(object) { return object != null && ids.has(object); },
  };
}
function stringTable() {
  const list = [], ids = new Map();
  const table = {
    list,
    id(s) { if (typeof s !== 'string') throw Error('prediction hull string is not text'); let id = ids.get(s); if (id === undefined) { id = list.length; list.push(s); ids.set(s, id); } return id; },
    at(id) { const s = list[id]; if (s === undefined) throw Error(`no prediction hull string ${id}`); return s; },
  };
  nativeTexts.forEach(s => table.id(s));
  return table;
}
const v3 = (a, i = 0) => new THREE.Vector3(a[i], a[i + 1], a[i + 2]);
const q4 = (a, i = 0) => new THREE.Quaternion(a[i], a[i + 1], a[i + 2], a[i + 3]);
const add = (a, v) => a.push(v.x, v.y, v.z);
const flag = v => v ? 1 : 0;
const maybe = (a, value) => { a.push(flag(value != null)); if (value != null) a.push(Number(value)); };

function copyField(into, from, key) {
  const value = from[key];
  if (value === undefined && !(key in into)) return;
  if (value?.isVector3 || value?.isQuaternion || value?.isMatrix4) {
    if (into[key]?.copy) into[key].copy(value); else into[key] = value.clone();
  } else if (Array.isArray(value)) {
    const array = into[key] ??= []; array.length = value.length;
    for (let i = 0; i < value.length; i++) array[i] = value[i];
  } else into[key] = value;
}

function dispatcher(bridge) {
  let sessions = dispatchers.get(bridge);
  if (sessions) return sessions;
  sessions = new Map();
  bridge.register({ predictionHullQuery(record) {
    // Generated import mirrors are reused by nested calls. Own everything
    // read from this one before calling any world function.
    const serial = record.session, op = record.op, args = Array.from(record.args);
    const session = sessions.get(serial);
    if (!session) return { values: [0] };
    return { values: session.query(op, args) };
  } });
  dispatchers.set(bridge, sessions);
  return sessions;
}

export function createVehiclePredictionSim(instance, options) {
  return createVehiclePredictionBridge(bridgeFor(instance), options);
}

export function createVehiclePredictionBridge(bridge, { trace = null } = {}) {
  const x = bridge.x, registry = dispatcher(bridge), owners = new Set();
  let disposed = false;
  const live = () => { if (disposed) throw Error('vehicle prediction bridge is disposed'); };

  function create(vehicle) {
    live();
    const serial = nextSession++, context = Object.create(bridge), heap = new Heap(), mirror = new VehicleMirror();
    const bodies = identities();
    context.strings = stringTable(); context.refs = [...bridge.refs];
    context.refs[KIND_BODY] = bodies; context.refs[KIND_VEHICLE_DRIVER] = identities();
    const world = vehicle.system.world, system = vehicle.system;
    let members = [], busy = false, dropAfter = false, callbackFailed = false, callbackError = null, queries = null, excludedBody = null;
    let handle = x.prediction_hulls_create();

    function answer(op, a) {
      switch (op) {
        case 0: {
          const out = { point: new THREE.Vector3(), normal: new THREE.Vector3() };
          const hit = world.cast(v3(a), v3(a, 3), a[6], out, a[7] < 0 ? undefined : bodies.at(a[7]));
          if (!hit) return [0];
          const b = hit.body ? bodies.slotOf(hit.body) : -1;
          return [1, hit.distance, hit.point.x, hit.point.y, hit.point.z, hit.normal.x, hit.normal.y, hit.normal.z, flag(hit.terrain), b];
        }
        case 1: {
          let i = 1; const probes = [];
          for (let n = 0; n < a[0]; n++, i += 3) probes.push(v3(a, i));
          const radius = a[i++], q = q4(a, i); i += 4;
          const own = a[i++] ? q4(a, i) : null; if (own) i += 4;
          const previous = v3(a, i), position = v3(a, i + 3), velocity = v3(a, i + 6); i += 9;
          const undo = world.pushWalls(vehicle, probes, radius, q, own, previous, position, velocity, Boolean(a[i]));
          return [flag(undo), position.x, position.y, position.z, velocity.x, velocity.y, velocity.z];
        }
        case 2: {
          const hit = world.ground(a[0], a[1], a[2], { normal: new THREE.Vector3() });
          return hit ? [1, hit.height, hit.normal.x, hit.normal.y, hit.normal.z] : [0];
        }
        case 3: {
          const polygons = world.springPolygons(vehicle, a[0], v3(a, 1), a[4]), values = [polygons.length];
          for (const polygon of polygons) {
            values.push(polygon.id, flag(polygon.moving), polygon.triangles.length);
            for (const tri of polygon.triangles) { add(values, tri.a); add(values, tri.b); add(values, tri.c); }
          }
          return values;
        }
        case 4: {
          const body = bodies.at(a[0]);
          world.enableBody(body, Boolean(a[1])); excludedBody = a[1] ? null : body;
          return [];
        }
        case 5: return [flag(world.bodyEnabled(bodies.at(a[0])))];
        case 6: return [flag(world.waterOver(a[0], a[1]))];
        case 7: return [world.waterLevel()];
        case 8: return [flag(system.water)];
        case 9: return [flag(world.inDeathRegion(v3(a)))];
        case 10: {
          const env = system.flightEnvironment ?? {}, values = [];
          maybe(values, env.ceiling); values.push(system.maxFlyHeight, flag(env.bounds));
          if (env.bounds) values.push(...env.bounds.min, ...env.bounds.max);
          values.push(flag(env.space), flag(env.landingZones != null));
          if (env.landingZones != null) {
            values.push(env.landingZones.length);
            for (const zone of env.landingZones) values.push(...zone.min, ...zone.max, zone.floorY ?? zone.min[1]);
          }
          values.push(flag(system.flightAssists)); return values;
        }
        case 11: return tuningKeys.map(key => vehicle.flightConfig[key]);
        case 12: members = Array.from(system.units?.all ? system.units.all() : system.units || []); return [members.length];
        case 13: {
          const t = members[a[0]], values = [t.id, flag(t.generation != null), t.generation ?? 0, flag(t.alive), flag(t.team != null), t.team ?? 0,
            flag(t === vehicle.aiTarget), flag(t.aimPoint), t.vehicle?.body ? bodies.slotOf(t.vehicle.body) : -1];
          // A callable aim point is read only at its original query point.
          add(values, t.aimPoint ? { x: 0, y: 0, z: 0 } : t.position); return values;
        }
        case 14: {
          const t = system.units?.byId?.(a[0]);
          if (!t) return [-1];
          let i = members.indexOf(t); if (i < 0) { i = members.length; members.push(t); } return [i];
        }
        case 15: {
          const first = a[0] ? a[1] : undefined, second = a[2] ? a[3] : undefined;
          return [system.units?.relation?.(first, second) ?? (first === second ? 1 : -1)];
        }
        case 16: {
          const t = members[a[0]], point = new THREE.Vector3();
          if (t.aimPoint) t.aimPoint(point); else point.copy(t.position);
          return point.toArray();
        }
        case 17: return [flag(bodies.at(a[0]).owner?.system === system)];
        case 18: return [flag(system.inDeathRegion?.(v3(a)))];
        case 19: {
          const hit = world.raycastUnits(v3(a), v3(a, 3), a[6], { point: new THREE.Vector3(), normal: new THREE.Vector3() });
          return hit ? [1, hit.distance] : [0];
        }
        case 20: return [world.clock()];
        case 21: {
          const side = vehicle.legSide ??= [];
          if (a[0]) { side.length = a.length - 1; for (let i = 1; i < a.length; i++) side[i - 1] = a[i]; }
          return Array.from(side);
        }
        default: throw Error(`unknown prediction hull query ${op}`);
      }
    }
    function run(op, words) {
      live();
      if (!handle) throw Error('vehicle prediction session is disposed');
      if (busy) throw Error('vehicle prediction session is busy');
      const input = Float64Array.from(words);
      const ptr = x.prediction_hulls_room(handle, input.length);
      if (!ptr) throw Error('vehicle prediction session is busy');
      bridge.views(); bridge.f.set(input, ptr / 8);
      callbackError = null; callbackFailed = false; queries = trace ? [] : null; busy = true;
      let output, error, recorded;
      try {
        const ok = x.prediction_hulls_run(handle, op, serial);
        bridge.views();
        const start = x.prediction_hulls_output(handle) / 8, length = x.prediction_hulls_output_len(handle);
        output = Array.from(bridge.f.subarray(start, start + length));
        error = ok ? null : new TextDecoder().decode(new Uint8Array(x.memory.buffer,
          x.prediction_hulls_error(handle), x.prediction_hulls_error_len(handle)));
        recorded = queries;
      } finally {
        busy = false; queries = null;
        if (dropAfter) session.dispose();
      }
      const thrown = callbackError;
      trace?.({ session: serial, op, input: Array.from(input), output, error, queries: recorded });
      if (callbackFailed) throw thrown;
      if (error) throw Error(error);
      return output;
    }
    function state() {
      const row = new Array(VEHICLE_FULL_STRIDE).fill(0); heap.clear();
      putVehicle(context, vehicle, row, 0, heap);
      const words = [context.strings.list.length];
      for (const s of context.strings.list) text(words, s);
      words.push(...row, heap.words.length, ...heap.words, flag(vehicle.pose), vehicle.walkPhase, vehicle.walkBlend);
      for (const name of ['walk_leftfoot_rightfoot', 'walk_rightfoot_leftfoot']) {
        const clip = vehicle.clips?.get(name); words.push(flag(clip)); if (clip) words.push(clip.duration, clip.stride);
      }
      for (const key of configKeys) words.push(vehicle.flightConfig?.[key] ?? NaN);
      return words;
    }
    function sync(output) {
      const h = VEHICLE_EXPORT_STRIDE + 1, n = output[VEHICLE_EXPORT_STRIDE];
      syncVehicle(context, output, 0, h, mirror);
      for (const key of groundKeys) copyField(vehicle, mirror, key);
      if (vehicle.flying) for (const key of flightKeys) copyField(vehicle, mirror, key);
      else if (vehicle.alive) vehicle.rotation.set(0, vehicle.yaw, 0);
      for (let i = 0; i < vehicle.wheels.length; i++) for (const key of ['phase', 'previous', 'tickFrom']) copyField(vehicle.wheels[i], mirror.wheels[i], key);
      for (let i = 0; i < vehicle.seats.length; i++) for (const key of ['aimYaw', 'pitch', 'lookYaw', 'lookPitch']) copyField(vehicle.seats[i], mirror.seats[i], key);
      let at = h + n;
      vehicle.walkFromPhase = output[at++]; vehicle.walkFromBlend = output[at++];
      vehicle.walkPhase = output[at++]; vehicle.walkBlend = output[at++];
      if (output[at++]) for (const seat of vehicle.seats) for (const w of seat.weapons) {
        w.state.lockTargetId = output[at]; w.state.lockGeneration = output[at + 1];
        w.state.lockElapsed = output[at + 2]; w.state.lockComplete = Boolean(output[at + 3]);
      }
    }
    const session = {
      bridge, serial,
      get handle() { return handle; },
      get disposed() { return !handle; },
      query(op, args) {
        if (callbackFailed) return [0];
        let values;
        try {
          values = [1, ...answer(op, args)];
          // SimBridge writes the reply after this handler returns. Keep a
          // result-size failure inside the exception containment boundary.
          if (values.length + 7 > bridge.exchangeWords) throw Error('prediction hull query exceeds the exchange block');
        } catch (error) {
          callbackFailed = true; callbackError = error; values = [0];
          // The original ground tick's finally leaves its exclusion before
          // predictVehicle restores the caller's enabled state. Complete
          // only that pending cleanup; later native queries are suppressed.
          const body = excludedBody; excludedBody = null;
          if (body && op !== 4) try { world.enableBody(body, vehicle.alive); } catch {}
        }
        queries?.push({ op, args: [...args], values: [...values] });
        return values;
      },
      step(input) {
        run(1, state());
        const words = [input.forward, input.right, input.turn, flag(input.brake)];
        for (const key of ['forward', 'right', 'turn', 'pitch', 'up', 'down', 'takeoff']) words.push(Number(input[key]));
        maybe(words, input.stickX); maybe(words, input.stickY);
        for (const key of ['boost', 'trick', 'immelmann', 'rollLock', 'targetLock', 'bot', 'responseCurve']) words.push(Number(input[key]));
        words.push(flag(input.steeringAssist === false));
        sync(run(2, words));
      },
      dispose() {
        if (!handle) return;
        if (busy) { dropAfter = true; return; }
        x.prediction_hulls_drop(handle); handle = 0; registry.delete(serial); owners.delete(session);
      },
    };
    registry.set(serial, session); owners.add(session);
    try {
      const def = vehicle.def, words = [];
      definition(words, { settings: def.settings ?? null, springs: def.springs ?? [], fx: def.fx ?? null,
        explosion: Boolean(def.explosion), finalExplosion: Boolean(def.finalExplosion),
        seats: (def.seats ?? []).map(seat => ({ settings: seat.settings ?? null })) });
      run(0, words);
    } catch (error) { session.dispose(); throw error; }
    return session;
  }
  return {
    bridge, create,
    dispose() { if (disposed) return; for (const owner of owners) owner.dispose(); disposed = true; },
  };
}
