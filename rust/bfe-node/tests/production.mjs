// Run against a build without lifecycle-probe. Counters and arenas are part
// of the production surface; only diagnostic fixtures depend on the feature.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
const addon = createRequire(import.meta.url)('../../../.host-addon/bfe-node.node');
test('production owners expose arena diagnostics and exactly-once counters', () => {
  assert.equal(addon.probePanic, undefined);
  const host = new addon.NativeHost(), before = addon.lifecycleCounts(), owner = host.create();
  assert.equal(host.probeFailure, undefined);
  const out = new Float64Array(1);
  assert.equal(owner.events(out), 1); assert.equal(out[0], 0);
  assert.equal(owner.diagnostics().created - before[0], 1);
  assert.equal(owner.close(), true); assert.equal(owner.close(), false);
  const after = owner.diagnostics();
  assert.equal(after.released - before[1], 1);
  assert.equal(after.live, after.created - after.released);
  assert.equal(after.reservedWords, 0); assert.equal(after.closed, true);
  assert.equal(addon.handshake().nativeWorld, false);
});
