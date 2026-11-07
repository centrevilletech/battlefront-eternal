import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { createWireBridge } from '../src/runtime/wire-sim.js';
import { WireCensusLayout as L } from '../src/sim/gen/wire-census.js';

// Bracket the codec as well as its generated argument/result exchange. A
// phase-only counter would miss the retained wire path's current allocations.
test('publication census includes codec allocations and keeps instances separate', async () => {
  const a = bridgeFor(await loadSim()), b = bridgeFor(await loadSim());
  const wire = createWireBridge(a);
  const read = bridge => {
    const ptr = bridge.x.wire_census_end(); bridge.views();
    return Array.from(bridge.f.subarray(ptr / 8, ptr / 8 + L.words));
  };
  a.x.wire_census_begin(); b.x.wire_census_begin();
  wire.encodeWordDelta(new Uint8Array(300).fill(17));
  const used = read(a);
  assert.ok(used[L.allocations] > 0);
  assert.ok(used[L.allocationBytes] >= 300);
  assert.deepEqual(read(b), [0, 0, 0]);
  // Reading fixed diagnostic storage itself adds no allocator traffic, and
  // finishing an absent bracket reports no work rather than stale counts.
  a.x.wire_census_begin(); read(a);
  assert.deepEqual(read(a), [0, 0, 0]);
});
