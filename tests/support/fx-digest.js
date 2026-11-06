// A digest of the destruction effects' records as the renderer would read
// them: the live counts and counters, and a hash of every record in use. The
// scenarios that hold the Rust phase to its recorded runs compare these.

const floatBits = new Uint32Array(1);
const asFloat = new Float32Array(floatBits.buffer);

// FNV-1a over the bit patterns of float or index values.
function hashRange(h, array, from, to) {
  for (let i = from; i < to; i++) {
    asFloat[0] = array[i];
    h ^= array instanceof Uint16Array ? array[i] : floatBits[0];
    h = Math.imul(h, 16777619) >>> 0;
  }
  return h;
}

export function digest(fx) {
  let h = 2166136261;
  for (const pool of [fx.hot, fx.smoke]) {
    for (const array of pool.arrays) h = hashRange(h, array, 0, pool.count * 4);
  }
  const arcs = fx.arcs;
  h = hashRange(h, arcs.positions, 0, arcs.vertex * 3);
  h = hashRange(h, arcs.tangents, 0, arcs.vertex * 3);
  h = hashRange(h, arcs.meta, 0, arcs.vertex * 4);
  h = hashRange(h, arcs.indices, 0, arcs.index);
  return [fx.hot.count, fx.smoke.count, arcs.live, arcs.vertex, arcs.index, fx.hot.dropped, fx.smoke.dropped, fx.hot.emitted, fx.smoke.emitted, arcs.spawned, h];
}
