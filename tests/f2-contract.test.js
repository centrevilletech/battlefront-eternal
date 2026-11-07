import test from 'node:test';
import assert from 'node:assert/strict';
import { F2MirrorCheck } from '../src/sim/wasm/f2-contract.js';
import { F2Counters } from './helpers/f2-counters.js';

test('F2 counters snapshot actual calls and copied scalar words independently', () => {
  const c = new F2Counters();
  c.ticks++; c.phaseCalls++; c.inputFields += 27; c.outputFields += 51;
  c.commandCalls += 2;
  const snapshot = c.snapshot();
  c.reset();
  assert.equal(snapshot.phaseCalls, 1);
  assert.equal(snapshot.commandCalls, 2);
  assert.equal(snapshot.inputFields + snapshot.outputFields, 78);
  assert.equal(c.ticks, 0);
});

test('F2 mirror detector preserves numeric bits and excludes foreign state', () => {
  const checker = new F2MirrorCheck(), v = { health: NaN, yaw: -0, weapon: 0 };
  const read = (o, k) => o[k];
  checker.published(v, [['health', v.health], ['yaw', v.yaw]]);
  v.weapon++;
  checker.check(v, read);
  v.yaw = 0;
  assert.throws(() => checker.check(v, read), /F2 mirror write: yaw/);
  checker.retire(v);
  checker.check(v, read);
});
