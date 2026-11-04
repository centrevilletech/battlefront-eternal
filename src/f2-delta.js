// Apply the generated publication mask to a retained encoded row. A command
// touches only its owner. Generated mirror readers use this local buffer, so
// unchanged words and rig tables never cross from WASM again.
import { F2DeltaLayout as D } from './sim/gen/f2.js';
const MASK_BITS = 32;
export function applyDelta(f, at, heap, row) {
  if (f[at + D.length] > row.length) throw Error('F2 delta row capacity exceeded');
  let value = heap + f[at + D.values];
  const mask = heap + f[at + D.mask], length = f[at + D.maskLength];
  for (let i = 0; i < length; i++) {
    let bits = f[mask + i] >>> 0;
    while (bits) {
      const bit = 31 - Math.clz32(bits & -bits);
      row[i * MASK_BITS + bit] = f[value++]; bits &= bits - 1;
    }
  }
}
export function deltaWords(f, at) { return D.words + f[at + D.maskLength] + f[at + D.valuesLength]; }
// Buffers are allocated with a hull, lease or actor, outside ordinary ticks.
export function deltaBuffer() { return new Float64Array(4096); }
