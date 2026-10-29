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
      vectors.some(key => !d[key]?.isVector3) || !d.previous?.isVector3 || !d.settled?.isVector3) throw Error('droideka state prerequisite missing');
}

export function captureDroidekaState(d) {
  prerequisite(d);
  if (!Number.isInteger(d.mode) || d.mode < 0 || d.mode > 13 || !Number.isSafeInteger(d.gun) || d.gun < 0) throw Error('invalid droideka mode or barrel');
  let flags = 0;
  for (const [i, key] of ['grounded','reverse','dead'].entries()) {
    if (typeof d[key] !== 'boolean') throw Error('invalid droideka flag');
    flags |= Number(d[key]) << i;
  }
  if (d.shield) {
    if (typeof d.shield.on !== 'boolean' || d.shield.value < 0 || d.shield.value > d.shield.max) throw Error('invalid droideka shield');
    flags |= 8 | (Number(d.shield.on) << 4);
  }
  if (d.lastGun !== undefined) {
    if (!validBarrel(d, d.lastGun)) throw Error('invalid droideka last barrel');
    flags |= 32;
  }
  const bytes = new Uint8Array(baseBytes + (flags & 8 ? 8 : 0) + (flags & 32 ? 16 : 0)), view = new DataView(bytes.buffer);
  bytes[0] = d.mode; bytes[1] = flags; let at = 2;
  const put = value => { view.setFloat64(at, finite(value), true); at += 8; };
  for (const key of numbers) put(d[key]);
  for (const key of vectors) for (const value of d[key].toArray()) put(value);
  if (flags & 8) put(d.shield.value);
  if (flags & 32) { put(d.lastGun.index); put(d.lastGun.barrel); }
  return bytes;
}

export function decodeDroidekaState(bytes) {
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

export function restoreDroidekaState(d, bytes) {
  prerequisite(d); const state = decodeDroidekaState(bytes);
  if (Boolean(state.shield) !== Boolean(d.shield) || state.shield && state.shield.value > d.shield.max ||
      state.lastGun !== undefined && !validBarrel(d, state.lastGun)) throw Error('droideka content prerequisite mismatch');
  for (const key of [...numbers, 'mode', 'grounded', 'reverse', 'dead', 'lastGun']) d[key] = state[key];
  for (const key of vectors) d[key].fromArray(state[key]);
  if (state.shield) Object.assign(d.shield, state.shield);
  // Presentation endpoints are local. Solver scratch is assigned before read.
  d.previous.copy(d.position); d.settled.copy(d.position);
}
