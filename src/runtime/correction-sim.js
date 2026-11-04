// Owned native readers for exact correction payloads. The original helpers
// publish their results, retaining live property reads and setter order.
import { bridgeFor } from '../sim/runtime.js';
import { WEAPON_STATE_NUMBERS, WEAPON_STATE_FLAGS } from './network-weapon-state.js';
import { NETWORK_CONTRACT } from './network-contract.js';

const compactSoldierNumbers = ['fuel', 'aimYaw', 'aimPitch', 'dive', 'stateTime', 'lock'];
const soldierNumbers = 'maxSpeed maxStrafe maxJump maxTurn maxPitch accel fuel maxFlyHeight getupTime diveTime aimYaw aimPitch dive stateTime lock lookX lookY tickLength contactCount groundAdjustment'.split(' ');
const soldierVectors = 'position previous settled velocity normal contactMean forwardAxis rightAxis left facing pull motion center closest point contactNormal surfaceNormal pushDir clipDir push before origin avoidance stepStart stepTarget stepVelocity stepResolved'.split(' ');
const replayVectors = 'position previous settled velocity normal contactMean avoidance'.split(' ');
const postures = ['stand', 'crouch', 'prone'], airs = [null, 'jump', 'fall', 'thrown', 'jet'];
const droidekaNumbers = ['aimYaw', 'aimPitch', 'stateTime', 'time', 'legs', 'spin', 'roll', 'lean', 'blastClock', 'lookX', 'lookY', 'gun'];
const groundNumbers = ['health', 'team', 'yaw', 'tilt', 'roll', 'yawRate', 'forwardInput', 'rightInput', 'tiltRate', 'rollRate', 'support'];
const compactFlightNumbers = 'health team yaw tilt roll yawRate forwardInput rightInput altitude throttle airspeed tiltRate stateTime floorY bank power stickX stickY flightRollRate trickCooldown trickPending gear landingMessageTime flightClock flightAssistId flightLockId flightLockGeneration flightLockTimer flightLostLOS'.split(' ');
const flightNumbers = 'health team yaw previousYaw tilt previousTilt roll previousRoll yawRate forwardInput rightInput altitude throttle airspeed tiltRate stateTime floorY bank power stickX stickY flightRollRate trickTime trickCooldown trickPending trickDirection gear landingMessageTime flightClock flightAssistId flightLockId flightLockGeneration flightLockTimer flightLostLOS tickFromYaw tickFromTilt tickFromRoll'.split(' ');
const flightVectors = ['position', 'previous', 'velocity', 'tickFrom', 'flightStart', 'flightEnd', 'flightTrickRight'];
const flightQuaternions = ['quaternion', 'flightPreviousQ', 'flightTickQ', 'flightStartQ', 'flightTrickLoopQ', 'flightTrickEndQ'];
const compactFlightVectors = ['position', 'velocity', 'flightStart', 'flightEnd', 'flightTrickRight'];
const compactFlightQuaternions = ['quaternion', 'flightStartQ', 'flightTrickLoopQ', 'flightTrickEndQ'];
const flightBooleans = 'alive sleeping grounded rollLock boosting boostExhausted terrainWarning flightActionHeld trickHeld immelmannHeld'.split(' ');
const messages = ['', 'Takeoff path blocked', 'Landing path blocked', 'Too fast', 'No landing zone', 'Press SPACE to land', 'Flight ceiling', 'Leaving battlefield'];
const flightEnums = { flightState: ['Landed', 'TakingOff', 'Flying', 'Landing', 'Destroyed'], trick: ['', 'barrel', 'loop', 'immelmann'], flightLockState: ['', 'Locking', 'Locked'], flightWarning: messages, landingMessage: messages, landingReason: messages };
const seatFields = ['aimYaw', 'pitch', 'lookYaw', 'lookPitch', 'yawRate', 'lastYaw'];
const passengerFields = ['aimYaw', 'pitch', 'lookYaw', 'lookPitch'];
const turretFields = ['yaw', 'pitch', 'yawVel', 'pitchVel', 'gunYaw', 'gunPitch'];
const unsupported = Symbol('observable correction prerequisite');
const isObject = value => value !== null && (typeof value === 'object' || typeof value === 'function');
const typedPrototype = Object.getPrototypeOf(Uint8Array.prototype);
const typedDescriptors = Object.getOwnPropertyDescriptors(typedPrototype);
const viewDescriptors = Object.getOwnPropertyDescriptors(DataView.prototype);
const bufferPrototype = typeof Buffer === 'function' ? Buffer.prototype : null;
const byteSlice = Uint8Array.prototype.slice, byteSubarray = Uint8Array.prototype.subarray, byteSet = Uint8Array.prototype.set;
const bufferSlice = bufferPrototype?.slice, bufferSubarray = bufferPrototype?.subarray;
const byteSpecies = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array), Symbol.species);

function descriptor(value, key) {
  for (let at = value; at; at = Object.getPrototypeOf(at)) {
    const found = Object.getOwnPropertyDescriptor(at, key);
    if (found) return found;
  }
}
function data(value, key) {
  if (value == null) return undefined;
  if (!isObject(value)) throw unsupported;
  const found = descriptor(value, key);
  if (!found) return undefined;
  if (!('value' in found)) throw unsupported;
  return found.value;
}
function tagged(value, key, tag = 'isVector3') {
  return +!!data(data(value, key), tag);
}
function seatPresent(value) { return +!!data(data(value, 'seats'), '0'); }
function groundSupported(value) {
  return +(!data(value, 'flying') && seatPresent(value) && tagged(value, 'position') && tagged(value, 'velocity') && tagged(value, 'quaternion', 'isQuaternion'));
}
function flightSupported(value, seat) {
  return +!!(data(value, 'flying') && data(value, 'flightConfig') && (!seat || seatPresent(value)));
}
function compactLayout(state, trick) {
  const transition = state === 'TakingOff' || state === 'Landing';
  return {
    numbers: [...compactFlightNumbers, ...(trick ? ['trickTime', 'trickDirection'] : [])],
    vectors: ['position', 'velocity', ...(transition ? ['flightStart', 'flightEnd'] : []), ...(trick === 'barrel' ? ['flightTrickRight'] : [])],
    quaternions: ['quaternion', ...(transition ? ['flightStartQ'] : []), ...(trick === 'immelmann' ? ['flightTrickLoopQ', 'flightTrickEndQ'] : [])],
  };
}

// Descriptor checks do not invoke prerequisite getters. Such getters can edit
// unread bytes, so those objects keep the original interleaved reader.
function metadata(make) {
  try { return make(); }
  catch (error) { if (error === unsupported) return null; throw error; }
}
function byteInput(bytes, memory, sliced) {
  if (!(bytes instanceof Uint8Array)) return { valid: 0, bytes: [] };
  // A replaced prototype does not change a typed array's element storage.
  // Inspect its intrinsic tag without invoking an overridden toStringTag.
  if (Reflect.apply(typedDescriptors[Symbol.toStringTag].get, bytes, []) !== 'Uint8Array') return null;
  const prototype = Object.getPrototypeOf(bytes);
  if (prototype !== Uint8Array.prototype && (!bufferPrototype || prototype !== bufferPrototype)) return null;
  for (const key of ['length', 'buffer', 'byteOffset', 'byteLength']) {
    const got = descriptor(bytes, key), expected = typedDescriptors[key];
    if (!got || got.get !== expected.get || got.set !== expected.set || 'value' in got) return null;
  }
  // Module growth must not detach a caller's correction during publication.
  if (bytes.buffer === memory.buffer || typeof SharedArrayBuffer === 'function' && bytes.buffer instanceof SharedArrayBuffer) return null;
  if (sliced) {
    const method = sliced === 'subarray' ? 'subarray' : 'slice';
    const expected = prototype === bufferPrototype ? (method === 'slice' ? bufferSlice : bufferSubarray) : (method === 'slice' ? byteSlice : byteSubarray);
    if (descriptor(bytes, method)?.value !== expected) return null;
    if (prototype === bufferPrototype) {
      if (descriptor(bytes, 'subarray')?.value !== bufferSubarray) return null;
    } else {
      if (descriptor(bytes, 'constructor')?.value !== Uint8Array) return null;
      const species = descriptor(Uint8Array, Symbol.species);
      if (!species || species.get !== byteSpecies.get || species.set !== byteSpecies.set || 'value' in species) return null;
    }
  }
  const copy = new Array(bytes.length);
  for (let i = 0; i < copy.length; i++) copy[i] = bytes[i];
  return { valid: 1, bytes: copy };
}
function reader(bytes) {
  let at = 0;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const number = () => { const value = view.getFloat64(at, true); at += 8; return value; };
  return {
    number,
    numbers(value, keys) { for (const key of keys) value[key] = number(); },
    vectors(value, keys, size = 3) { for (const key of keys) value[key] = Array.from({ length: size }, number); },
    booleans(value, keys) { for (const key of keys) { const n = number(); value[key] = n === 2 ? undefined : !!n; } },
    enums(value, fields) { for (const [key, items] of Object.entries(fields)) value[key] = items[number()]; },
    seat() {
      const seat = {};
      for (const key of seatFields) { const tag = number(), value = number(); seat[key] = tag === 2 ? undefined : tag === 1 ? null : value; }
      return seat;
    },
  };
}
function weaponState(r) {
  const state = { level: r.number() }, flags = r.number();
  for (let i = 0; i < WEAPON_STATE_FLAGS.length; i++) state[WEAPON_STATE_FLAGS[i]] = !!(flags & 1 << i);
  for (const key of WEAPON_STATE_NUMBERS) { const n = r.number(); state[key] = Number.isNaN(n) ? null : n; }
  return state;
}

export function createCorrectionSim(instance, options) { return createCorrectionBridge(bridgeFor(instance), options); }
// One reusable owned parser belongs to this bridge until dispose(). Neither
// payloads nor decoded results retain views into its native allocation.
export function createCorrectionBridge(bridge, { trace = null } = {}) {
  const x = bridge.x;
  let handle = 0, calls = 0, disposed = false;
  const live = () => { if (disposed) throw Error('correction bridge is disposed'); };
  function run(op, meta, input, unpack, failed = null) {
    live();
    const words = Float64Array.from([meta.length, ...meta, input.valid, input.bytes.length, ...input.bytes]);
    if (!handle) handle = x.prediction_corrections_create();
    const ptr = x.prediction_corrections_room(handle, words.length);
    bridge.views(); bridge.f.set(words, ptr / 8);
    const ok = x.prediction_corrections_run(handle, op); calls++;
    bridge.views();
    const at = x.prediction_corrections_output(handle), length = x.prediction_corrections_output_len(handle);
    const output = new Uint8Array(x.memory.buffer, at, length * 8).slice();
    const error = ok ? null : new TextDecoder().decode(new Uint8Array(x.memory.buffer, x.prediction_corrections_error(handle), x.prediction_corrections_error_len(handle)));
    // A packed Number array can quiet signaling NaNs. Read copied float bytes
    // directly into fields, just as the original DataView reader does.
    // Construct the result before a trace can reenter or grow memory.
    const result = ok ? unpack(reader(output)) : failed && output.length ? failed(reader(output)) : null;
    if (trace) trace({ op, input: Array.from(words), output: Array.from(new Float64Array(output.buffer)), error });
    if (!ok) throw Error(error);
    return result;
  }
  function decode(op, bytes, unpack, sliced = false) {
    live();
    const input = byteInput(bytes, x.memory, sliced);
    live();
    return input ? run(op, [], input, unpack) : null;
  }
  function body(op, value, bytes, make, unpack) {
    live();
    if (!isObject(value)) return null;
    const input = byteInput(bytes, x.memory, false);
    live();
    if (!input) return null;
    const meta = metadata(() => make(input));
    live();
    return meta ? run(op, meta, input, unpack) : null;
  }
  function recon(op, source, extra, unpack, sliced = false) {
    live();
    const fields = metadata(() => ({ bytes: data(source, 'bytes'), view: data(source, 'view'), at: data(source, 'at') }));
    live();
    if (!fields || !(fields.view instanceof DataView) || !Number.isSafeInteger(fields.at) || fields.at < 0
      || !descriptor(source, 'at')?.writable) return null;
    const input = byteInput(fields.bytes, x.memory, sliced);
    live();
    if (!input?.valid) return null;
    for (const key of ['getUint8', 'getUint16', 'getUint32', 'getFloat64']) {
      if (descriptor(fields.view, key)?.value !== viewDescriptors[key].value) return null;
    }
    for (const key of ['buffer', 'byteOffset', 'byteLength']) {
      const got = descriptor(fields.view, key), expected = viewDescriptors[key];
      if (!got || got.get !== expected.get || got.set !== expected.set || 'value' in got) return null;
    }
    // Fields keeps its original fixed DataView across foreign reads. A resized
    // or detached source must continue on that view, with its original errors.
    try {
      if (fields.view.buffer !== fields.bytes.buffer || fields.view.byteOffset !== fields.bytes.byteOffset
        || fields.view.byteLength !== fields.bytes.byteLength) return null;
    } catch { return null; }
    return run(op, op === 13 ? [] : [fields.at, ...extra], input, r => {
      const at = r.number(), value = unpack(r, fields.bytes);
      source.at = at; return value;
    });
  }
  return {
    bridge,
    get calls() { return calls; },
    get handle() { return handle; },
    decodeWeapon(bytes) {
      return decode(0, bytes, weaponState);
    },
    readCompactSoldier(value, bytes) {
      return body(1, value, bytes, () => ['position', 'velocity', 'normal', 'settled', 'previous', 'contactMean'].map(key => tagged(value, key)), r => {
        const flags = r.number(); r.number(); r.number(); r.number();
        const values = {}; r.numbers(values, compactSoldierNumbers); r.vectors(values, ['position', 'velocity', 'normal']);
        if (flags & 1) r.vectors(values, ['settled']);
        return { values, settled: !!(flags & 1), diveFrom: postures[r.number()] };
      });
    },
    readSoldier(value, bytes, schema) {
      if (typeof schema !== 'number') { live(); return null; }
      const vectors = schema === 4 ? replayVectors : soldierVectors;
      return body(2, value, bytes, () => [schema, ...vectors.map(key => tagged(value, key))], r => {
        const values = {}; r.numbers(values, soldierNumbers); r.vectors(values, vectors);
        r.enums(values, { posture: postures, air: airs, diveFrom: postures }); r.booleans(values, ['grounded', 'stepBlocked']);
        return values;
      });
    },
    decodeDroideka(bytes) {
      return decode(3, bytes, r => {
        const mode = r.number(), flags = r.number();
        const state = { mode, grounded: !!(flags & 1), reverse: !!(flags & 2), dead: !!(flags & 4) };
        r.numbers(state, droidekaNumbers); r.vectors(state, ['position', 'velocity', 'avoidance']);
        state.shield = flags & 8 ? { value: r.number(), on: !!(flags & 16) } : null;
        state.lastGun = flags & 32 ? { index: r.number(), barrel: r.number() } : undefined;
        return state;
      });
    },
    readGround(value, bytes) {
      return body(4, value, bytes, () => [groundSupported(value), tagged(value, 'previous'), tagged(value, 'tickFrom')], r => {
        const values = {}; r.numbers(values, groundNumbers); r.vectors(values, ['position', 'velocity']); r.vectors(values, ['quaternion'], 4);
        r.booleans(values, ['alive', 'sleeping']); return { values, seat: r.seat() };
      });
    },
    readCompactFlight(value, bytes) {
      return body(5, value, bytes, input => {
        const l = compactLayout(flightEnums.flightState[input.bytes[0]], flightEnums.trick[input.bytes[1]]);
        return [flightSupported(value, true),
          ...compactFlightVectors.map(key => l.vectors.includes(key) ? tagged(value, key) : 0),
          ...compactFlightQuaternions.map(key => l.quaternions.includes(key) ? tagged(value, key, 'isQuaternion') : 0),
          tagged(value, 'previous'), tagged(value, 'tickFrom'), tagged(value, 'flightPreviousQ', 'isQuaternion'), tagged(value, 'flightTickQ', 'isQuaternion')];
      }, r => {
        const values = {}; r.enums(values, flightEnums);
        const l = compactLayout(values.flightState, values.trick);
        r.numbers(values, l.numbers); r.vectors(values, l.vectors); r.vectors(values, l.quaternions, 4); r.booleans(values, flightBooleans);
        return { values, seat: r.seat() };
      });
    },
    readFlight(value, bytes, schema) {
      if (typeof schema !== 'number') { live(); return null; }
      return body(6, value, bytes, () => [schema, flightSupported(value, false), ...flightVectors.map(key => tagged(value, key)), ...flightQuaternions.map(key => tagged(value, key, 'isQuaternion')), seatPresent(value)], r => {
        const values = {}; r.numbers(values, flightNumbers); r.vectors(values, flightVectors); r.vectors(values, flightQuaternions, 4);
        r.booleans(values, flightBooleans); r.enums(values, flightEnums); return { values, seat: r.seat() };
      });
    },
    decodeMounted(bytes) {
      return decode(7, bytes, r => {
        const mode = r.number(), id = r.number(), generation = r.number(), index = r.number(), flags = r.number();
        const fields = mode === 4 ? [] : mode === 2 ? passengerFields : turretFields;
        const state = mode === 2 ? { lookYaw: null, lookPitch: null } : {};
        r.number();
        for (const key of fields) if ((key !== 'lookYaw' || flags & 1) && (key !== 'lookPitch' || flags & 2)) state[key] = r.number();
        const count = r.number(), weapons = [];
        for (let i = 0; i < count; i++) { const barrel = r.number(), offset = r.number(), length = r.number(); weapons.push({ barrel, payload: bytes.slice(offset, offset + length) }); }
        return { mode, id, generation, index, state, weapons };
      }, true);
    },
    decodeOwn(record) {
      live();
      if (!isObject(record)) return null;
      const fields = metadata(() => {
        const supported = data(record, 'kind') === 'correction' && data(record, 'schema') === NETWORK_CONTRACT.ownCorrectionSchema;
        return { supported, payload: supported ? data(record, 'payload') : null };
      });
      live();
      if (!fields) return null;
      const input = fields.supported ? byteInput(fields.payload, x.memory, true) : { valid: 0, bytes: [] };
      live();
      if (!input) return null;
      return run(8, [+fields.supported], input, r => {
        // Materialize spans before tracing. Standard Buffer slices share their
        // source; Uint8Array slices own bytes, as in the original field reader.
        const take = () => { const offset = r.number(), length = r.number(); return fields.payload.slice(offset, offset + length); };
        const result = { epoch: r.number(), contentEpoch: r.number(), classId: r.number(), team: r.number(), bodySchema: r.number(), vehicleId: r.number(), vehicleGeneration: r.number() };
        result.body = take(); result.health = r.number(); const dead = r.number();
        result.deathTime = r.number(); result.unitDeadTime = r.number(); const removed = r.number();
        result.dead = !!dead; result.removed = !!removed; result.kick = {};
        r.numbers(result.kick, ['rate', 'hold', 'offset']);
        result.zoomStep = r.number(); result.zoom = r.number(); const first = r.number();
        result.firstPerson = !!(first & 1); result.gunUnderwater = !!(first & 2);
        result.selected = [r.number(), r.number()];
        result.weapons = Array.from({ length: r.number() }, () => ({ id: r.number(), payload: take() }));
        result.heldBits = take(); const flags = r.number();
        const view = { invertMouse: !!(flags & 1), firstPerson: !!(flags & 2) };
        if (flags & 32) view.mouseScale = r.number();
        if (flags & 64) view.aspect = r.number();
        if (flags & 128) view.viewTick = r.number();
        result.bindingProfileId = r.number();
        if (flags & 4) view.flightSettings = { invert: !!(flags & 8) };
        if (flags & 256) view.flightSettings.sensitivity = r.number();
        if (flags & 512) view.flightSettings.curve = r.number();
        result.hasBindings = !!(flags & 1024);
        if (flags & 16) {
          const axes = Array.from({ length: 4 }, () => r.number()), buttons = r.number();
          view.gamepad = { axes, buttons: Array.from({ length: 16 }, (_, i) => ({ pressed: !!(buttons & (1 << i)) })) };
        }
        result.discardedInput = r.number(); const remote = r.number();
        result.controlMode = remote; result.mounted = null;
        if (remote >= 2) result.mounted = take();
        result.remote = null;
        if (remote === 1) {
          result.remote = { id: r.number(), generation: r.number(), definitionId: r.number() };
          result.remote.payload = take();
        }
        result.view = view; return result;
      });
    },
    readArsenalMask(bytes) { return decode(9, bytes, r => r.number()); },
    expandArsenal(full, bytes, mask) {
      live();
      if (descriptor(full, 'set')?.value !== byteSet) return null;
      const base = byteInput(full, x.memory, false), input = byteInput(bytes, x.memory, 'subarray');
      live();
      if (!base?.valid || base.bytes.length !== 162 || !input?.valid || !input.bytes.length) return null;
      // A custom defaults slice may return a view into the delta itself. Each
      // original set can then change the next source word, so retain that loop.
      if (full.buffer === bytes.buffer) return null;
      const publish = r => {
        const at = r.number(), expanded = Uint8Array.from({ length: 162 }, () => r.number());
        full.set(expanded); return at;
      };
      // A later truncated word still leaves the preceding header/word copies
      // in the caller's full array. Copy that owned partial output on error.
      return run(10, [mask, ...base.bytes], input, publish, publish);
    },
    readArsenalMelee(bytes, at) {
      live(); const input = byteInput(bytes, x.memory, false); live();
      if (!input?.valid) return null;
      return run(11, [at], input, r => ({ time: r.number(), index: r.number(), active: r.number(), at: r.number() }));
    },
    finishArsenal(full, bytes, at, tail, bound) {
      live();
      const input = byteInput(full, x.memory, false), source = byteInput(bytes, x.memory, false);
      live();
      if (!input?.valid || !source?.valid) return null;
      return run(12, [at, source.bytes.length, +!!tail, tail?.time ?? 0, tail?.index ?? 0, tail?.active ?? 0, bound], input, weaponState);
    },
    readReconHeader(source) {
      return recon(13, source, [], r => {
        const bits = r.number(), state = { dead: !!(bits & 1), piloted: !!(bits & 2), firstPerson: !!(bits & 4) };
        r.numbers(state, ['yaw', 'pitch', 'health', 'countdown', 'lookX', 'lookY']);
        state.zoomStep = r.number(); state.randomSeed = r.number();
        r.vectors(state, ['position', 'velocity']); state.selected = [r.number(), r.number()];
        return { state, count: r.number() };
      });
    },
    readReconRange(source) {
      return recon(14, source, [], (r, bytes) => { const at = r.number(), length = r.number(); return bytes.slice(at, at + length); }, true);
    },
    readReconVector(source, length) {
      return recon(15, source, [length], r => Array.from({ length }, () => r.number()));
    },
    readReconSalvo(source) {
      return recon(16, source, [], r => r.number() ? { present: true, index: r.number(), level: r.number() } : { present: false });
    },
    readReconSalvoState(source) {
      return recon(17, source, [], r => ({ left: r.number(), wait: r.number() }));
    },
    readReconCamera(source) {
      return recon(18, source, [], r => {
        if (!r.number()) return { present: false };
        const value = {}; r.vectors(value, ['position']); r.vectors(value, ['quaternion'], 4);
        r.numbers(value, ['aspect', 'fov', 'reach']); value.fresh = !!r.number();
        return { present: true, value };
      });
    },
    dispose() { if (disposed) return; disposed = true; if (handle) { x.prediction_corrections_drop(handle); handle = 0; } },
  };
}
