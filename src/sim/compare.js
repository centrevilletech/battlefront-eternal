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

// Two Maps with the same keys in the same order (as a Map compares them: a
// NaN key is NaN), each pair of values the same by `same`.
export function sameMap(a, b, same) {
  if (a === b) return true;
  if (!a || !b || a.size !== b.size) return false;
  const x = [...a], y = [...b];
  for (let i = 0; i < x.length; i++) if (!Object.is(x[i][0], y[i][0]) || !same(x[i][1], y[i][1])) return false;
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

// Two plain objects with the same own keys in the same order, each pair of
// values the same by `same` (an object keyed by numbers).
export function sameObject(a, b, same) {
  if (a === b) return true;
  if (!a || !b) return false;
  const x = Object.keys(a), y = Object.keys(b);
  if (x.length !== y.length) return false;
  for (let i = 0; i < x.length; i++) if (x[i] !== y[i] || !same(a[x[i]], b[y[i]])) return false;
  return true;
}

// Two typed arrays of one class and length n, each element the same by
// Object.is.
export function sameTyped(a, b, n) {
  if (a === b) return true;
  if (!a || !b || a.constructor !== b.constructor) return false;
  return sameArray(a, b, n);
}

// A phase mask addresses its retained encoded row, including heap spans.
// Check a whole record in word-sized chunks before decoding its fields.
export function deltaChanged(mask, at, count, base = 0) {
  if (!count) return false;
  at = (at - base) | 0;
  const end = (at + count - 1) | 0, first = at >>> 5, last = end >>> 5;
  const left = 0xffffffff << (at & 31), right = 0xffffffff >>> (31 - (end & 31));
  // Most generated fields fit in one word. Handle that range directly;
  // larger records check the two partial ends and whole words between them.
  if (first === last) return (mask[first] & left & right) !== 0;
  if (mask[first] & left) return true;
  for (let i = first + 1; i < last; i++) if (mask[i]) return true;
  return (mask[last] & right) !== 0;
}

// A foreign observer temporarily borrows canonical fields in its mirror.
// Restore those fields even when their final bits equal the consumed baseline.
export function markDelta(mask, at, count, base = 0) {
  if (!count) return;
  at = (at - base) | 0;
  const end = (at + count - 1) | 0, first = at >>> 5, last = end >>> 5;
  const left = 0xffffffff << (at & 31), right = 0xffffffff >>> (31 - (end & 31));
  if (first === last) { mask[first] |= left & right; return; }
  mask[first] |= left;
  for (let i = first + 1; i < last; i++) mask[i] = 0xffffffff;
  mask[last] |= right;
}
