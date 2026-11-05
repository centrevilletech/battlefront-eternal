// Exact post-tick human Soldier correction. Class constants come from the
// verified class/world/animation prerequisites; this is not an AI save state.
export const COMPACT_SOLDIER_SCHEMA = 6;
const numbers = ['fuel', 'aimYaw', 'aimPitch', 'dive', 'stateTime', 'lock'];
const vectors = ['position', 'velocity', 'normal'];
// captureCompactSoldier spells these out, in this order.
if (vectors.join() !== 'position,velocity,normal') throw Error('Soldier vectors changed: update captureCompactSoldier');
const postures = ['stand', 'crouch', 'prone'];
const airs = [null, 'jump', 'fall', 'thrown', 'jet'];
export function captureCompactSoldier(s, native = null) { return typeof native?.soldier === 'function' ? native.soldier(s) : capture(s, null); }
// The same correction written into `into` ({bytes, view} over at least 149
// bytes) and returned as a view of it, valid until the next capture into
// it: a checkpoint copies the correction straight into its own payload.
export function captureCompactSoldierInto(s, into, native = null) { return typeof native?.soldier === 'function' ? native.soldier(s, into) : capture(s, into); }
// The replay boundary keeps one maximum-size writer. Return its byte count,
// rather than allocating a subarray merely to describe the current checkpoint.
export const COMPACT_SOLDIER_MAX_BYTES = 149;
function number(into, n) {
  if (!Number.isFinite(n)) throw Error('nonfinite Soldier correction');
  into.view.setFloat64(into.at, n, true); into.at += 8;
}
function vector(into, v, key) {
  if (!v?.isVector3) throw Error(`missing Soldier vector ${key}`);
  number(into, v.x); number(into, v.y); number(into, v.z);
}
export function writeCompactSoldier(s, into) {
  if (s.contactCount !== 0 || s.contactMean.lengthSq() !== 0) throw Error('Soldier correction requires completed collision pass');
  const settled = !(Object.is(s.settled.x, s.position.x) && Object.is(s.settled.y, s.position.y) && Object.is(s.settled.z, s.position.z)), diving = s.dive !== 0;
  const posture = postures.indexOf(s.posture), air = airs.indexOf(s.air), bytes = into.bytes;
  if (posture < 0 || air < 0 || typeof s.grounded !== 'boolean') throw Error('invalid Soldier state');
  bytes[0] = Number(settled) | (Number(diving) << 1); bytes[1] = posture; bytes[2] = air; bytes[3] = Number(s.grounded);
  into.at = 4;
  // captureCompactSoldier's existing wire order, without dynamic field reads
  // or per-call number/vector closures. Full-precision doubles stay unchanged.
  number(into, s.fuel); number(into, s.aimYaw); number(into, s.aimPitch);
  number(into, s.dive); number(into, s.stateTime); number(into, s.lock);
  vector(into, s.position, 'position'); vector(into, s.velocity, 'velocity'); vector(into, s.normal, 'normal');
  if (settled) vector(into, s.settled, 'settled');
  if (diving) { const n = postures.indexOf(s.diveFrom); if (n < 0) throw Error('invalid dive posture'); bytes[into.at++] = n; }
  return into.at;
}
function capture(s, into) {
  const settled = !(Object.is(s.settled.x, s.position.x) && Object.is(s.settled.y, s.position.y) && Object.is(s.settled.z, s.position.z));
  const length = 124 + (settled ? 24 : 0) + (s.dive !== 0 ? 1 : 0);
  const bytes = into ? into.bytes : new Uint8Array(length);
  const writer = into || { bytes, view: new DataView(bytes.buffer), at: 0 };
  writeCompactSoldier(s, writer);
  return into ? bytes.subarray(0, length) : bytes;
}
function readCompactSoldier(s, bytes) {
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
  return { values, settled, diveFrom };
}
export function restoreCompactSoldier(s, bytes, native = null) {
  const decoded = typeof native?.readCompactSoldier === 'function' ? native.readCompactSoldier(s, bytes) : null;
  const { values, settled, diveFrom } = decoded ?? readCompactSoldier(s, bytes);
  // Validate first. Reset draw history locally; never transmit another machine's
  // accumulated smoothing displacement or mutate the shared collision world.
  for (const key of numbers) s[key] = values[key];
  for (const key of vectors) s[key].fromArray(values[key]);
  if (settled) s.settled.fromArray(values.settled); else s.settled.copy(s.position);
  s.previous.copy(s.position); s.contactMean.set(0, 0, 0); s.contactCount = 0;
  s.lookX = s.lookY = 0; s.stepBlocked = false;
  s.posture = postures[bytes[1]]; s.air = airs[bytes[2]]; s.grounded = !!bytes[3]; s.diveFrom = diveFrom;
}
