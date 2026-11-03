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

// Two Maps with the same keys in the same order, each pair of values the
// same by `same`.
export function sameMap(a, b, same) {
  if (a === b) return true;
  if (!a || !b || a.size !== b.size) return false;
  const x = [...a], y = [...b];
  for (let i = 0; i < x.length; i++) if (x[i][0] !== y[i][0] || !same(x[i][1], y[i][1])) return false;
  return true;
}

// Two arrays of the same length whose items are the same by `same`. (Its
// name is no kind's or record's: theirs are same<Name>, and the generator
// refuses a kind or record named after one of these comparisons.)
export function sameEach(a, b, same) {
  if (a === b) return true;
  if (!a || !b || a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (!same(a[i], b[i])) return false;
  return true;
}
