// Run against the addon built without lifecycle-probe. Production must retain
// its diagnostics, capacity protocol and real world entry without test hooks.
import assert from 'node:assert/strict';
import test from 'node:test';
import {createRequire} from 'node:module';
import {WorldTickSetupLayout as S,WorldTickInputLayout as I,WorldTickOutputLayout as O} from '../../../src/sim/gen/world.js';
const addon = createRequire(import.meta.url)('../../../.host-addon/bfe-node.node');
test('production world boundaries retain diagnostics and exactly-once counters', () => {
  assert.equal(addon.probePanic, undefined);
  assert.equal(typeof addon.NativeOwner.prototype.probeStep, 'undefined');
  const hello = addon.handshake(), before = addon.lifecycleCounts();
  assert.equal(hello.addonAbi, 2); assert.equal(hello.nativeWorld, true);
  const host = new addon.NativeHost(), owner = host.create();
  assert.equal(host.probeFailure, undefined);
  try {
    owner.stepInit(new Float64Array(S.words));
    const input = new Float64Array(I.words), short = new Float64Array(O.words - 1).fill(73);
    assert.equal(owner.step(input, short), O.words);
    assert.ok(short.every(word => word === 73));
    assert.equal(owner.diagnostics().lastFault, 'Capacity');
    const output = new Float64Array(O.words);
    assert.equal(owner.step(input, output), O.words); assert.equal(output[O.accepted], 0);
    assert.equal(owner.readBank(6, new Float64Array(1)), 1);
    const events = new Float64Array(1);
    assert.equal(owner.events(events), 1); assert.equal(events[0], 0);
    assert.equal(owner.diagnostics().created - before[0], 1);
    assert.equal(owner.diagnostics().poisoned, false);
  } finally {assert.equal(owner.close(), true); assert.equal(owner.close(), false);}
  const after = addon.lifecycleCounts(), diagnostics = owner.diagnostics();
  assert.equal(after[0] - before[0], 1); assert.equal(after[1] - before[1], 1);
  assert.equal(diagnostics.released - before[1], 1);
  assert.equal(diagnostics.live, diagnostics.created - diagnostics.released);
  assert.equal(diagnostics.closed, true); assert.equal(diagnostics.reservedWords, 0);
  assert.equal(host.liveOwners(), 0);
});
