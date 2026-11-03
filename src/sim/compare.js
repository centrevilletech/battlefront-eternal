// Comparing as the store's checks do: Object.is for numbers (NaN is NaN,
// -0 is not 0), identity for objects. The generated code (src/sim/gen/)
// imports these; this module imports nothing.
export function sameVec(a, b) {
  if (a === b) return true;
  if (!a || !b) return false;
  return Object.is(a.x, b.x) && Object.is(a.y, b.y) && Object.is(a.z, b.z);
}

export function sameQuat(a, b) {
  if (a === b) return true;
  if (!a || !b) return false;
  return Object.is(a.x, b.x) && Object.is(a.y, b.y) && Object.is(a.z, b.z) && Object.is(a.w, b.w);
}

export function sameArray(a, b, n) {
  if (a === b) return true;
  if (!a || !b || a.length !== n || b.length !== n) return false;
  for (let i = 0; i < n; i++) if (!Object.is(a[i], b[i])) return false;
  return true;
}

export function sameList(a, b) {
  if (a === b) return true;
  if (!a || !b || a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}
