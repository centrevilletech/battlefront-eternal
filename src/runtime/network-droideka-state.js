// Exact shared Droideka solver checkpoint. Authenticated class content supplies
// clips, turn rates, collision geometry and shield constants. No animation or
// gameplay callbacks run while restoring a correction.
export const DROIDEKA_BODY_SCHEMA = 10;
const numbers = ['aimYaw','aimPitch','stateTime','time','legs','spin','roll','lean','blastClock','lookX','lookY','gun'];
const vectors = ['position','velocity','avoidance'];
const baseBytes = 2 + numbers.length * 8 + vectors.length * 24;
const finite = value => { if (!Number.isFinite(value)) throw Error('nonfinite droideka state'); return value; };
function validBarrel(d, selected) {
  return Number.isInteger(selected?.index) && selected.index >= 0 && selected.index < d.guns.length &&
    Number.isInteger(selected.barrel) && selected.barrel >= 0 && selected.barrel < (d.gunBarrels?.[selected.index]?.length || 1);
}
function prerequisite(d) {
  if (!(d.clips instanceof Map) || typeof d.clip !== 'function' || typeof d.toggleShield !== 'function' ||
      !d.position?.isVector3 || !d.velocity?.isVector3 || !d.avoidance?.isVector3 || !d.previous?.isVector3 || !d.settled?.isVector3) throw Error('droideka state prerequisite missing');
}

// captureDroidekaState's optional shield and last-gun fields give the maximum
// length. Replay writes into this retained storage and carries a byte count.
export const DROIDEKA_STATE_MAX_BYTES = baseBytes + 8 + 16;
function checkpointFlags(d) {
  prerequisite(d);
  if (!Number.isInteger(d.mode) || d.mode < 0 || d.mode > 13 || !Number.isSafeInteger(d.gun) || d.gun < 0) throw Error('invalid droideka mode or barrel');
  if (typeof d.grounded !== 'boolean' || typeof d.reverse !== 'boolean' || typeof d.dead !== 'boolean') throw Error('invalid droideka flag');
  let flags = Number(d.grounded) | Number(d.reverse) << 1 | Number(d.dead) << 2;
  if (d.shield) {
    if (typeof d.shield.on !== 'boolean' || d.shield.value < 0 || d.shield.value > d.shield.max) throw Error('invalid droideka shield');
    flags |= 8 | Number(d.shield.on) << 4;
  }
  if (d.lastGun !== undefined) {
    if (!validBarrel(d, d.lastGun)) throw Error('invalid droideka last barrel');
    flags |= 32;
  }
  return flags;
}
function write(d, into, flags) {
  const bytes = into.bytes, view = into.view;
  bytes[0] = d.mode; bytes[1] = flags; let at = 2;
  // Keep captureDroidekaState's byte order. Straight field writes avoid the
  // temporary vector arrays and field-name iteration in each page frame.
  view.setFloat64(at, finite(d.aimYaw), true); at += 8;
  view.setFloat64(at, finite(d.aimPitch), true); at += 8;
  view.setFloat64(at, finite(d.stateTime), true); at += 8;
  view.setFloat64(at, finite(d.time), true); at += 8;
  view.setFloat64(at, finite(d.legs), true); at += 8;
  view.setFloat64(at, finite(d.spin), true); at += 8;
  view.setFloat64(at, finite(d.roll), true); at += 8;
  view.setFloat64(at, finite(d.lean), true); at += 8;
  view.setFloat64(at, finite(d.blastClock), true); at += 8;
  view.setFloat64(at, finite(d.lookX), true); at += 8;
  view.setFloat64(at, finite(d.lookY), true); at += 8;
  view.setFloat64(at, finite(d.gun), true); at += 8;
  view.setFloat64(at, finite(d.position.x), true); at += 8;
  view.setFloat64(at, finite(d.position.y), true); at += 8;
  view.setFloat64(at, finite(d.position.z), true); at += 8;
  view.setFloat64(at, finite(d.velocity.x), true); at += 8;
  view.setFloat64(at, finite(d.velocity.y), true); at += 8;
  view.setFloat64(at, finite(d.velocity.z), true); at += 8;
  view.setFloat64(at, finite(d.avoidance.x), true); at += 8;
  view.setFloat64(at, finite(d.avoidance.y), true); at += 8;
  view.setFloat64(at, finite(d.avoidance.z), true); at += 8;
  if (flags & 8) { view.setFloat64(at, finite(d.shield.value), true); at += 8; }
  if (flags & 32) {
    view.setFloat64(at, finite(d.lastGun.index), true); at += 8;
    view.setFloat64(at, finite(d.lastGun.barrel), true); at += 8;
  }
  return at;
}
export function writeDroidekaState(d, into) { return write(d, into, checkpointFlags(d)); }
export function captureDroidekaState(d, native = null) {
  if (typeof native?.droideka === 'function') return native.droideka(d);
  const flags = checkpointFlags(d);
  const bytes = new Uint8Array(baseBytes + (flags & 8 ? 8 : 0) + (flags & 32 ? 16 : 0));
  write(d, { bytes, view: new DataView(bytes.buffer) }, flags);
  return bytes;
}

export function decodeDroidekaState(bytes, native = null) {
  const decoded = typeof native?.decodeDroideka === 'function' ? native.decodeDroideka(bytes) : null;
  if (decoded !== null) return decoded;
  if (!(bytes instanceof Uint8Array) || bytes.length < baseBytes) throw Error('invalid droideka state length');
  const mode = bytes[0], flags = bytes[1];
  if (mode > 13 || flags > 63 || flags & 16 && !(flags & 8) ||
      bytes.length !== baseBytes + (flags & 8 ? 8 : 0) + (flags & 32 ? 16 : 0)) throw Error('invalid droideka state');
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength); let at = 2;
  const get = () => { const value = finite(view.getFloat64(at, true)); at += 8; return value; };
  const state = { mode, grounded: Boolean(flags & 1), reverse: Boolean(flags & 2), dead: Boolean(flags & 4) };
  for (const key of numbers) state[key] = get();
  for (const key of vectors) state[key] = [get(), get(), get()];
  state.shield = flags & 8 ? { value: get(), on: Boolean(flags & 16) } : null;
  state.lastGun = flags & 32 ? { index: get(), barrel: get() } : undefined;
  if (!Number.isSafeInteger(state.gun) || state.gun < 0 || state.lastGun !== undefined &&
      (!Number.isSafeInteger(state.lastGun.index) || state.lastGun.index < 0 || !Number.isSafeInteger(state.lastGun.barrel) || state.lastGun.barrel < 0) || state.shield?.value < 0) throw Error('invalid droideka state');
  return state;
}

export function restoreDroidekaState(d, bytes, native = null) {
  prerequisite(d); const state = decodeDroidekaState(bytes, native);
  if (Boolean(state.shield) !== Boolean(d.shield) || state.shield && state.shield.value > d.shield.max ||
      state.lastGun !== undefined && !validBarrel(d, state.lastGun)) throw Error('droideka content prerequisite mismatch');
  for (const key of [...numbers, 'mode', 'grounded', 'reverse', 'dead', 'lastGun']) d[key] = state[key];
  for (const key of vectors) d[key].fromArray(state[key]);
  if (state.shield) Object.assign(d.shield, state.shield);
  // Presentation endpoints are local. Solver scratch is assigned before read.
  d.previous.copy(d.position); d.settled.copy(d.position);
}
