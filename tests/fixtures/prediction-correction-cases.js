import assert from 'node:assert/strict';
import { Quaternion, Vector3 } from 'three';
import { Soldier } from '../helpers/body.js';
import { Weapon } from '../../src/weapons.js';
import { captureWeaponState, decodeWeaponState } from '../../src/runtime/network-weapon-state.js';
import { captureSoldierCorrection, restoreSoldierCorrection } from '../../src/runtime/network-entity-correction.js';
import { captureCompactSoldier, restoreCompactSoldier } from '../../src/runtime/network-entity-soldier-compact.js';
import { captureDroidekaState, decodeDroidekaState, restoreDroidekaState } from '../../src/runtime/network-droideka-state.js';
import { captureGroundCorrection, restoreGroundCorrection } from '../../src/runtime/network-entity-ground.js';
import { captureFlightCorrection, restoreFlightCorrection } from '../../src/runtime/network-entity-flight.js';
import { captureCompactFlight, restoreCompactFlight } from '../../src/runtime/network-entity-flight-compact.js';
import { captureMountedControl, decodeMountedControl, restoreMountedControl } from '../../src/runtime/network-mounted-control.js';

const words = value => value.split(' ');
const flightNumbers = words('health team yaw previousYaw tilt previousTilt roll previousRoll yawRate forwardInput rightInput altitude throttle airspeed tiltRate stateTime floorY bank power stickX stickY flightRollRate trickTime trickCooldown trickPending trickDirection gear landingMessageTime flightClock flightAssistId flightLockId flightLockGeneration flightLockTimer flightLostLOS tickFromYaw tickFromTilt tickFromRoll');
const flightVectors = words('position previous velocity tickFrom flightStart flightEnd flightTrickRight');
const flightQuaternions = words('quaternion flightPreviousQ flightTickQ flightStartQ flightTrickLoopQ flightTrickEndQ');
const flightFlags = words('alive sleeping grounded rollLock boosting boostExhausted terrainWarning flightActionHeld trickHeld immelmannHeld');
const groundNumbers = words('health team yaw tilt roll yawRate forwardInput rightInput tiltRate rollRate support');
const droidNumbers = words('aimYaw aimPitch stateTime time legs spin roll lean blastClock lookX lookY gun');
const seatFields = words('aimYaw pitch lookYaw lookPitch yawRate lastYaw');
export const correctionGun = () => ({ class: 'correction-gun', base: 'blaster', channel: 0, RoundsPerClip: 10, ammo: 4,
  ShotDelay: .125, ReloadTime: .25, charges: [{ at: .5, ShotsPerSalvo: 2 }], ordnance: {} });
function seat(seed) { return Object.fromEntries(seatFields.map((name, i) => [name, i % 3 === 0 ? undefined : i % 3 === 1 ? null : seed + i / 16])); }
function vehicle(seed, flying) {
  const v = { flying, flightConfig: flying ? {} : null, seats: [seat(seed)], body: { matrix: { elements: [7, 11, 13] } },
    legSide: [17, -19], walkPhase: 23, wheelPhase: 29, omitted: { identity: true } };
  for (const [i, name] of flightNumbers.entries()) v[name] = seed + i / 32;
  for (const name of ['rollRate', 'support']) v[name] = seed + .125;
  for (const [i, name] of flightVectors.entries()) v[name] = new Vector3(seed + i, seed - i / 2, -seed - i);
  for (const name of flightQuaternions) v[name] = new Quaternion(-0, 0, 0, 1);
  for (const [i, name] of flightFlags.entries()) v[name] = i % 3 === 2 ? undefined : i % 2 === 0;
  v.alive = true; v.sleeping = false; v.floorY = NaN;
  Object.assign(v, { flightState: 'Flying', trick: '', flightLockState: 'Locking', flightWarning: '', landingMessage: '', landingReason: '' });
  return v;
}
function droid(seed) {
  const d = { clips: new Map(), clip() {}, toggleShield() {}, guns: [{}, {}], gunBarrels: [[{}], [{}, {}]],
    mode: 7, grounded: true, reverse: false, dead: false, gun: 3, shield: { max: 100, value: 40, on: true, drain: -5, refill: 2 },
    lastGun: { index: 1, barrel: 1 }, waterClock: 37, collisionDamage: 41, muzzleAnimator: { retained: true } };
  for (const [i, name] of droidNumbers.entries()) if (name !== 'gun') d[name] = seed + i / 16;
  for (const [i, name] of ['position', 'velocity', 'avoidance', 'previous', 'settled'].entries()) d[name] = new Vector3(seed + i, -seed, i / 8);
  return d;
}
function soldier(seed) {
  const s = new Soldier({ cast: () => null }, {}); s.spawn(seed, seed / 2, -seed, .125);
  s.fuel = .75; s.aimPitch = -.25; s.stateTime = seed / 8;
  s.position.x = -0; s.settled.copy(s.position); s.contactCount = 0; s.contactMean.set(0, 0, 0);
  s.forwardAxis.set(43, 47, 53); s.groundAdjustment = 59; return s;
}
function weapon(seed) {
  const w = new Weapon(correctionGun()); w.def = w.levels[1]; w.level = 17;
  Object.assign(w, { timer: -0, clip: .625, heat: seed / 16, reserve: Infinity, yawSpreadOverride: null,
    salvoSeed: 123, salvoCount: 8, random: () => .25, omitted: { retained: true } }); return w;
}
function mount(seed, mode) {
  const entries = [0, 1].map(i => ({ state: weapon(seed + i), barrel: i === 0 ? -0 : 9007199254740991 }));
  const target = { aimYaw: seed + .125, pitch: -.25, lookYaw: null, lookPitch: .375, weapons: entries,
    yaw: seed, yawVel: .1, pitchVel: -.2, gunYaw: -.3, gunPitch: .4, weapon: entries[0].state, barrel: entries[0].barrel };
  const mounted = { aiTarget: { id: 0xffffffff, generation: 7 }, seats: mode === 2 ? [{}, target] : [target] };
  return { seat: mode === 3 ? { turret: Object.assign(target, { aiTarget: mounted.aiTarget }) } : { vehicle: mounted, index: mode === 2 ? 1 : 0 }, target };
}

export function correctionDescriptors() {
  const descriptors = [
    { name: 'weapon', op: 0, create: weapon, capture: captureWeaponState, decode: decodeWeaponState, start: 2, numeric: () => Array.from({ length: 20 }, (_, i) => 2 + i * 8) },
    { name: 'soldier-full', op: 2, create: soldier, capture: s => captureSoldierCorrection(s, 1), restore: (s, b, native) => restoreSoldierCorrection(s, b, 1, native),
      numeric: b => Array.from({ length: (b.length - 5) / 8 }, (_, i) => i * 8) },
    { name: 'soldier-replay', op: 2, create: soldier, capture: s => captureSoldierCorrection(s, 4), restore: (s, b, native) => restoreSoldierCorrection(s, b, 4, native),
      numeric: b => Array.from({ length: (b.length - 5) / 8 }, (_, i) => i * 8) },
    { name: 'droideka', op: 3, create: droid, capture: captureDroidekaState, restore: restoreDroidekaState, decode: decodeDroidekaState,
      numeric: b => Array.from({ length: (b.length - 2) / 8 }, (_, i) => 2 + i * 8) },
    { name: 'ground', op: 4, create: seed => vehicle(seed, false), capture: captureGroundCorrection, restore: restoreGroundCorrection,
      numeric: b => [...Array.from({ length: 21 }, (_, i) => i * 8), ...Array.from({ length: 6 }, (_, i) => b.length - 53 + i * 9)] },
    { name: 'flight-full', op: 6, create: seed => vehicle(seed, true), capture: captureFlightCorrection,
      restore: (s, b, native) => restoreFlightCorrection(s, b, 2, native),
      numeric: b => [...Array.from({ length: (b.length - 70) / 8 }, (_, i) => i * 8), ...Array.from({ length: 6 }, (_, i) => b.length - 53 + i * 9)] },
  ];
  for (const settled of [false, true]) for (const diving of [false, true]) descriptors.push({ name: `soldier-compact-${Number(settled)}${Number(diving)}`, op: 1,
    create: seed => { const s = soldier(seed); if (settled) s.settled.x = 5; if (diving) { s.dive = -.25; s.diveFrom = 'crouch'; } return s; },
    capture: captureCompactSoldier, restore: restoreCompactSoldier, numeric: b => Array.from({ length: (b.length - 4 - Number(diving)) / 8 }, (_, i) => 4 + i * 8) });
  for (const state of ['Landed', 'TakingOff', 'Flying', 'Landing', 'Destroyed']) for (const trick of ['', 'barrel', 'loop', 'immelmann']) descriptors.push({ name: `flight-compact-${state}-${trick || 'none'}`, op: 5,
    create: seed => Object.assign(vehicle(seed, true), { flightState: state, trick }), capture: captureCompactFlight, restore: restoreCompactFlight,
    numeric: b => [...Array.from({ length: (b.length - 70) / 8 }, (_, i) => 6 + i * 8), ...Array.from({ length: 6 }, (_, i) => b.length - 53 + i * 9)] });
  for (const mode of [2, 3, 4]) descriptors.push({ name: `mounted-${mode}`, op: 7, create: seed => mount(seed, mode), capture: captureMountedControl,
    decode: decodeMountedControl, restore: restoreMountedControl, numeric: () => [] });
  return descriptors;
}

function value(v) {
  if (v?.isVector3 || v?.isQuaternion) return v.toArray();
  if (v === null || ['undefined', 'number', 'string', 'boolean'].includes(typeof v)) return v;
  return null;
}
export function correctionObservation(body) {
  if (body.target) return { target: correctionObservation(body.target), weapons: body.target.weapons.map(entry => ({ barrel: entry.barrel, state: correctionObservation(entry.state) })) };
  const fields = {};
  for (const [name, field] of Object.entries(body)) if (field === null || ['undefined', 'number', 'string', 'boolean'].includes(typeof field) || field?.isVector3 || field?.isQuaternion) fields[name] = value(field);
  if (body.seats) fields.seat = Object.fromEntries(seatFields.map(name => [name, body.seats[0]?.[name]]));
  if (body.shield) fields.shield = { ...body.shield };
  if (Object.hasOwn(body, 'lastGun')) fields.lastGun = body.lastGun && { ...body.lastGun };
  if (body.body) fields.collider = body.body.matrix.elements.slice();
  if (body.legSide) fields.legSide = body.legSide.slice();
  if (body.levels) fields.definition = body.levels.indexOf(body.def);
  return fields;
}
export function framedCorrection(bytes, offset = 7, buffer = false) {
  const storage = buffer ? Buffer.alloc(bytes.length + offset + 11, 0xa5) : new Uint8Array(bytes.length + offset + 11).fill(0xa5);
  storage.set(bytes, offset); return storage.subarray(offset, offset + bytes.length);
}
export function runCorrectionCase(descriptor, variation = {}, native = null) {
  const source = descriptor.create(2), target = descriptor.create(13), original = descriptor.capture(source);
  let payload = variation.payload ? variation.payload(original.slice()) : original.slice();
  if (variation.frame && payload instanceof Uint8Array) payload = framedCorrection(payload, variation.frame, variation.buffer);
  variation.target?.(target, payload);
  const before = correctionObservation(target), identities = Object.fromEntries(Object.entries(target).filter(([, v]) => v && typeof v === 'object'));
  const calls = native?.calls ?? 0;
  let decoded, error = null;
  try {
    if (descriptor.restore && !variation.decodeOnly) descriptor.restore(target, payload, native);
    else decoded = descriptor.decode(payload, native);
  } catch (caught) { error = { name: caught.name, message: caught.message }; }
  const after = correctionObservation(target);
  if (error && !variation.publicationThrows) assert.deepEqual(after, before, `${descriptor.name} validation is atomic`);
  if (descriptor.restore && !error) for (const [key, object] of Object.entries(identities)) {
    if (['lastGun'].includes(key)) continue;
    assert.equal(target[key], object, `${descriptor.name} ${key} identity`);
  }
  return { error, decoded, before, after, calls: (native?.calls ?? 0) - calls, target, payload };
}

const changedNumber = (at, number) => bytes => { new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).setFloat64(at, number, true); return bytes; };
export function correctionCases() {
  const cases = [];
  for (const descriptor of correctionDescriptors()) {
    const payload = descriptor.capture(descriptor.create(2));
    const add = (suffix, variation) => cases.push({ label: `${descriptor.name}:${suffix}`, descriptor, variation });
    add('valid', {}); add('offset1', { frame: 1 }); add('offset7', { frame: 7 }); add('Buffer-view', { frame: 5, buffer: true });
    if (descriptor.decode && descriptor.restore) add('decode', { decodeOnly: true });
    for (const length of new Set([0, 1, 2, 3, 5, 6, 10, 11, 12, payload.length >> 1, payload.length - 1])) if (length < payload.length) add(`cut${length}`, { payload: b => b.subarray(0, length) });
    add('trailing', { payload: b => new Uint8Array([...b, 0]) });
    add('wrong-type', { payload: b => new DataView(b.buffer) });
    const numeric = descriptor.numeric(payload);
    for (const [index, at] of numeric.entries()) {
      add(`nan${index}`, { payload: changedNumber(at, NaN) });
      if (index % 7 === 0 || index === numeric.length - 1) {
        add(`infinity${index}`, { payload: changedNumber(at, Infinity) });
        add(`negative-infinity${index}`, { payload: changedNumber(at, -Infinity) });
        add(`negative-zero${index}`, { payload: changedNumber(at, -0) });
        add(`subnormal${index}`, { payload: changedNumber(at, Number.MIN_VALUE) });
      }
    }
    if (descriptor.name.startsWith('flight-') || descriptor.name === 'ground') {
      for (const tag of [0, 1, 2, 3, 255]) add(`seat-tag${tag}-nan`, { payload: b => {
        const at = b.length - 54; b[at] = tag; return changedNumber(at + 1, NaN)(b);
      } });
      add('missing-seat-with-nan', { target: v => { v.seats = []; }, payload: changedNumber(numeric[0], NaN) });
    }
    if (descriptor.name.startsWith('soldier-')) {
      add('missing-position-with-first-nan', { target: v => { v.position = null; }, payload: changedNumber(numeric[0], NaN) });
      add('missing-position-with-vector-nan', { target: v => { v.position = null; }, payload: changedNumber(descriptor.op === 1 ? 52 : 160, NaN) });
    }
  }
  return cases;
}
