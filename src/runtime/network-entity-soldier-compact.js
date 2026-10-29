// Exact post-tick human Soldier correction. Class constants come from the
// verified class/world/animation prerequisites; this is not an AI save state.
export const COMPACT_SOLDIER_SCHEMA = 6;
const numbers = ['fuel', 'aimYaw', 'aimPitch', 'dive', 'stateTime', 'lock'];
const vectors = ['position', 'velocity', 'normal'];
const postures = ['stand', 'crouch', 'prone'];
const airs = [null, 'jump', 'fall', 'thrown', 'jet'];
export function captureCompactSoldier(s) {
  if (s.contactCount !== 0 || s.contactMean.lengthSq() !== 0) throw Error('Soldier correction requires completed collision pass');
  const settled = !['x','y','z'].every(key => Object.is(s.settled[key], s.position[key])), diving = s.dive !== 0;
  const bytes = new Uint8Array(124 + (settled ? 24 : 0) + (diving ? 1 : 0));
  const view = new DataView(bytes.buffer); let at = 4;
  const number = n => { if (!Number.isFinite(n)) throw Error('nonfinite Soldier correction'); view.setFloat64(at, n, true); at += 8; };
  const posture = postures.indexOf(s.posture), air = airs.indexOf(s.air);
  if (posture < 0 || air < 0 || typeof s.grounded !== 'boolean') throw Error('invalid Soldier state');
  bytes[0] = Number(settled) | (Number(diving) << 1); bytes[1] = posture; bytes[2] = air; bytes[3] = Number(s.grounded);
  for (const key of numbers) number(s[key]);
  for (const key of [...vectors, ...(settled ? ['settled'] : [])]) {
    if (!s[key]?.isVector3) throw Error(`missing Soldier vector ${key}`);
    for (const n of s[key].toArray()) number(n);
  }
  if (diving) { const n = postures.indexOf(s.diveFrom); if (n < 0) throw Error('invalid dive posture'); bytes[at++] = n; }
  return bytes;
}
export function restoreCompactSoldier(s, bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.length < 124 || bytes[0] > 3 || bytes[1] > 2 || bytes[2] > 4 || bytes[3] > 1) throw Error('invalid compact Soldier correction');
  const settled = !!(bytes[0] & 1), diving = !!(bytes[0] & 2);
  if (bytes.length !== 124 + (settled ? 24 : 0) + (diving ? 1 : 0)) throw Error('invalid compact Soldier length');
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength), values = {}; let at = 4;
  const number = () => { const n = view.getFloat64(at, true); at += 8; if (!Number.isFinite(n)) throw Error('nonfinite Soldier correction'); return n; };
  for (const key of numbers) values[key] = number();
  if ((values.dive !== 0) !== diving) throw Error('inconsistent dive state');
  for (const key of [...vectors, ...(settled ? ['settled'] : [])]) {
    if (!s[key]?.isVector3) throw Error(`missing Soldier vector ${key}`);
    values[key] = [number(), number(), number()];
  }
  const diveFrom = diving ? postures[bytes[at++]] : postures[bytes[1]];
  if (!diveFrom || !s.previous?.isVector3 || !s.settled?.isVector3 || !s.contactMean?.isVector3) throw Error('invalid Soldier prerequisite');
  // Validate first. Reset draw history locally; never transmit another machine's
  // accumulated smoothing displacement or mutate the shared collision world.
  for (const key of numbers) s[key] = values[key];
  for (const key of vectors) s[key].fromArray(values[key]);
  if (settled) s.settled.fromArray(values.settled); else s.settled.copy(s.position);
  s.previous.copy(s.position); s.contactMean.set(0, 0, 0); s.contactCount = 0;
  s.lookX = s.lookY = 0; s.stepBlocked = false;
  s.posture = postures[bytes[1]]; s.air = airs[bytes[2]]; s.grounded = !!bytes[3]; s.diveFrom = diveFrom;
}
