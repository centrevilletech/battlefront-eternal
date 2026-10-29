// Exact mounted aim checkpoint. This block accompanies the player's own life
// and input acknowledgement; remote hull poses are not an aim baseline.
import {applyValidatedWeaponState} from './network-weapon-state.js';
import {captureArsenalWeapon,validateArsenalWeapon} from './network-arsenal-state.js';
export const VEHICLE_PASSENGER_CONTROL = 2;
export const STATIONARY_TURRET_CONTROL = 3;
export const VEHICLE_PILOT_CONTROL = 4;
const passengerFields = ['aimYaw', 'pitch', 'lookYaw', 'lookPitch'];
const turretFields = ['yaw', 'pitch', 'yawVel', 'pitchVel', 'gunYaw', 'gunPitch'];
const weaponsFor = (target, mode) => mode !== STATIONARY_TURRET_CONTROL
  ? (target.weapons || []).map(entry => ({ weapon: entry.state, barrel: entry.barrel, owner: entry }))
  : target.weapon ? [{ weapon: target.weapon, barrel: target.barrel, owner: target }] : [];

function identity(unit) {
  const lease = unit?.seat;
  const mount = lease?.vehicle || lease?.turret;
  const mode = lease?.vehicle ? (lease.index === 0 ? VEHICLE_PILOT_CONTROL : VEHICLE_PASSENGER_CONTROL) : STATIONARY_TURRET_CONTROL;
  const index = lease?.vehicle ? lease.index : 0;
  const id = mount?.aiTarget?.id, generation = mount?.aiTarget?.generation;
  if (!mount || !Number.isInteger(id) || id <= 0 || id > 0xffffffff ||
      !Number.isInteger(generation) || generation <= 0 || generation > 0xffffffff ||
      !Number.isInteger(index) || index < 0 || index > 255 ||
      mode !== STATIONARY_TURRET_CONTROL && !mount.seats[index]) {
    throw Error('mounted control lease prerequisite missing');
  }
  return { mode, id, generation, index, target: lease.vehicle ? mount.seats[index] : mount };
}

export function captureMountedControl(unit) {
  const { mode, id, generation, index, target } = identity(unit);
  const fields = mode === VEHICLE_PILOT_CONTROL ? [] : mode === VEHICLE_PASSENGER_CONTROL ? passengerFields : turretFields;
  // Null look targets mean no mouse override; preserve them independently.
  let flags = 0;
  if (mode === VEHICLE_PASSENGER_CONTROL) {
    if (target.lookYaw !== null) flags |= 1;
    if (target.lookPitch !== null) flags |= 2;
  }
  const values = fields.filter(key => key !== 'lookYaw' || flags & 1)
    .filter(key => key !== 'lookPitch' || flags & 2);
  const weapons = weaponsFor(target, mode);
  if (weapons.length > 2) throw Error('mounted weapon count exceeds correction budget');
  const encoded = weapons.map(({ weapon }) => captureArsenalWeapon(weapon));
  const bytes = new Uint8Array(12 + values.length * 8 + encoded.reduce((total,bytes)=>total+bytes.length+10,0)), view = new DataView(bytes.buffer);
  view.setUint8(0, mode); view.setUint32(1, id, true); view.setUint32(5, generation, true);
  view.setUint8(9, index); view.setUint8(10, flags);
  values.forEach((key, n) => {
    if (!Number.isFinite(target[key])) throw Error('nonfinite mounted control');
    view.setFloat64(11 + n * 8, target[key], true);
  });
  let at = 11 + values.length * 8; bytes[at++] = weapons.length;
  weapons.forEach(({ barrel }, index) => {
    if (!Number.isSafeInteger(barrel) || barrel < 0) throw Error('invalid mounted barrel');
    view.setFloat64(at, barrel, true); at += 8;
    view.setUint16(at,encoded[index].length,true);at+=2;
    bytes.set(encoded[index], at); at += encoded[index].length;
  });
  return bytes;
}

export function decodeMountedControl(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.length < 11) throw Error('truncated mounted control');
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const mode = view.getUint8(0), id = view.getUint32(1, true), generation = view.getUint32(5, true);
  const index = view.getUint8(9), flags = view.getUint8(10);
  if (![VEHICLE_PASSENGER_CONTROL, STATIONARY_TURRET_CONTROL, VEHICLE_PILOT_CONTROL].includes(mode) || !id || !generation ||
      flags > 3 || mode !== VEHICLE_PASSENGER_CONTROL && (flags !== 0 || index !== 0) ||
      mode === VEHICLE_PASSENGER_CONTROL && index === 0) throw Error('invalid mounted control');
  const fields = mode === VEHICLE_PILOT_CONTROL ? [] : mode === VEHICLE_PASSENGER_CONTROL ? passengerFields : turretFields;
  const values = fields.filter(key => key !== 'lookYaw' || flags & 1)
    .filter(key => key !== 'lookPitch' || flags & 2);
  let at = 11 + values.length * 8;
  if (bytes.length < at + 1) throw Error('invalid mounted control length');
  const count = bytes[at++];
  if (count > 2 || mode === STATIONARY_TURRET_CONTROL && count > 1) throw Error('invalid mounted control length');
  const state = mode === VEHICLE_PASSENGER_CONTROL ? { lookYaw: null, lookPitch: null } : {};
  values.forEach((key, n) => {
    const value = view.getFloat64(11 + n * 8, true);
    if (!Number.isFinite(value)) throw Error('nonfinite mounted control');
    state[key] = value;
  });
  const weapons = [];
  for (let i = 0; i < count; i++) {
    if(at+10>bytes.length)throw Error('invalid mounted control length');
    const barrel = view.getFloat64(at, true); at += 8;
    if (!Number.isSafeInteger(barrel) || barrel < 0) throw Error('invalid mounted barrel');
    const length=view.getUint16(at,true);at+=2;if(at+length>bytes.length)throw Error('invalid mounted control length');
    weapons.push({barrel,payload:bytes.slice(at,at+length)});at+=length;
  }
  if(at!==bytes.length)throw Error('invalid mounted control length');
  return { mode, id, generation, index, state, weapons };
}

// Separate validation allows the containing own correction to validate every
// prerequisite before changing the soldier, lease, recoil, or mounted aim.
export function validateMountedControl(unit, bytes) {
  const data = decodeMountedControl(bytes), lease = identity(unit);
  for (const key of ['mode', 'id', 'generation', 'index']) {
    if (data[key] !== lease[key]) throw Error('mounted control lease mismatch');
  }
  const targets = weaponsFor(lease.target, lease.mode);
  if (targets.length !== data.weapons.length) throw Error('mounted weapon content mismatch');
  const weapons = targets.map(({ weapon, owner }, index) => ({
    ...validateArsenalWeapon(weapon, data.weapons[index].payload), owner, barrel: data.weapons[index].barrel,
  }));
  return { target: lease.target, state: data.state, weapons };
}

export function applyValidatedMountedControl({ target, state, weapons }) {
  Object.assign(target, state);
  for (const entry of weapons) { applyValidatedWeaponState(entry); entry.owner.barrel = entry.barrel; }
}

export function restoreMountedControl(unit, bytes) {
  applyValidatedMountedControl(validateMountedControl(unit, bytes));
}
