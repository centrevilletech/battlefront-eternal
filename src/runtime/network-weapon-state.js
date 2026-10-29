// Mutable shared Weapon state. Physics/eligibility timers remain float64.
// Definitions, RNG ownership and presentation callbacks belong to the caller.
export const WEAPON_STATE_NUMBERS = ['timer','clip','reserve','heat','kick','spreadScale','sniperScale','yawSpreadOverride','wait','reloadLeft','throwTime','held','charge','pattern','salvosLeft','salvoWait','lockTargetId','lockGeneration','lockElapsed','limit'];
export const WEAPON_STATE_FLAGS = ['deployed','firing','overheated','released','hidden','charging','lockComplete'];
export const weaponStateBytes = 2 + WEAPON_STATE_NUMBERS.length * 8;

export function captureWeaponState(weapon) {
  const bytes = new Uint8Array(weaponStateBytes), view = new DataView(bytes.buffer);
  const level = weapon.levels.indexOf(weapon.def);
  if (level < 0 || level > 255) throw Error('weapon state definition mismatch');
  bytes[0] = level;
  for (let i = 0; i < WEAPON_STATE_FLAGS.length; i++) {
    const value = weapon[WEAPON_STATE_FLAGS[i]];
    if (typeof value !== 'boolean') throw Error('invalid weapon state flag');
    bytes[1] |= Number(value) << i;
  }
  WEAPON_STATE_NUMBERS.forEach((key, index) => {
    const value = weapon[key];
    if (!Number.isFinite(value) && !(key === 'reserve' && value === Infinity) &&
        !(key === 'yawSpreadOverride' && value === null)) throw Error('invalid weapon state number');
    view.setFloat64(2 + index * 8, value === null ? NaN : value, true);
  });
  return bytes;
}

export function decodeWeaponState(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.length !== weaponStateBytes || bytes[1] > 127) throw Error('invalid weapon state payload');
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength), state = { level: bytes[0] };
  WEAPON_STATE_FLAGS.forEach((key, index) => { state[key] = Boolean(bytes[1] & (1 << index)); });
  WEAPON_STATE_NUMBERS.forEach((key, index) => {
    const value = view.getFloat64(2 + index * 8, true);
    if (!Number.isFinite(value) && !(key === 'reserve' && value === Infinity) &&
        !(key === 'yawSpreadOverride' && Number.isNaN(value))) throw Error('invalid weapon state number');
    state[key] = Number.isNaN(value) ? null : value;
  });
  return state;
}

export function validateWeaponState(weapon, state) {
  if (!weapon?.levels[state.level]) throw Error('weapon state definition mismatch');
  return { weapon, state };
}

export function applyValidatedWeaponState({ weapon, state }) {
  weapon.def = weapon.levels[state.level];
  for (const key of [...WEAPON_STATE_NUMBERS, ...WEAPON_STATE_FLAGS]) weapon[key] = state[key];
}
