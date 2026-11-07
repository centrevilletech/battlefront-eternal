// Apply the generated publication mask to a retained encoded row. A command
// touches only its owner. Generated mirror readers use this local buffer, so
// unchanged words and rig tables never cross from WASM again.
import {PHASE_DELTA_WORDS} from '../gen/boundary.js';
import { F2DeltaLayout as D } from '../gen/f2.js';
const MASK_BITS = 32;
export function applyDelta(f, at, heap, row, dirty = null) {
  if (row && f[at + D.length] > row.length) throw Error('F2 delta row capacity exceeded');
  // Unit publications also use this mask to avoid decoding unchanged rows.
  // The retained mask belongs to the mirror and is cleared before reuse.
  if (dirty) dirty.fill(0);
  let value = heap + (f[at + D.values] | 0);
  const source = (f[at + D.source] / 8) | 0;
  const mask = heap + (f[at + D.mask] | 0), length = f[at + D.maskLength] | 0;
  for (let i = 0; i < length; i++) {
    let bits = f[mask + i] >>> 0;
    if (dirty) dirty[i] = bits;
    // A borrowed publication can decode its complete native cache directly.
    // Only its small mutable mask is retained when no encoded copy is needed.
    if (!row) continue;
    while (bits) {
      const bit = 31 - Math.clz32(bits & -bits);
      const word = i * MASK_BITS + bit;
      row[word] = f[source ? source + word : value++]; bits &= bits - 1;
    }
  }
  return source;
}
// Buffers are allocated with a hull, lease or actor, outside ordinary ticks.
export function deltaBuffer() { return new Float64Array(PHASE_DELTA_WORDS); }
