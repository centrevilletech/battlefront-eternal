// Names in the sound data are 32-bit FNV-1a hashes over each byte OR 0x20,
// as everywhere in the 2004 engine (tools/zero_hash.py): the OR folds letters
// to lower case but also turns "_" into 0x7f. The extractor writes hashes as
// "0x" and eight hex digits; this makes the same key from a name.
export function fnv1a(name) {
  let h = 0x811c9dc5;
  for (let i = 0; i < name.length; i++) {
    h ^= name.charCodeAt(i) & 0xff | 0x20;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
}

export function hashKey(name) {
  return `0x${fnv1a(name).toString(16).padStart(8, "0")}`;
}

// A name as the data spells it, or a key already: "0x1234abcd" stays as is.
export function keyOf(nameOrKey) {
  if (!nameOrKey) return null;
  return /^0x[0-9a-f]{8}$/.test(nameOrKey) ? nameOrKey : hashKey(nameOrKey);
}
