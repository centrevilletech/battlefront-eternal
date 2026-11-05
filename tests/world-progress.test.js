import test from 'node:test';
import assert from 'node:assert/strict';
import { worldProgress } from '../tools/lib/world-progress.mjs';
const samples = (page, world = page) => page.map((tick, i) => ({ at: i * 1000, pageTick: tick, worldTick: world[i], running: true, stepping: false }));
test('desktop gate measures live page and worker clocks', () => {
  const r = worldProgress(samples([0, 60, 120, 180, 240]), { minimumTps: 50 });
  assert.equal(r.pageTps, 60); assert.equal(r.worldTps, 60); assert.deepEqual(r.failures, []);
});
test('desktop gate rejects a stuck page even when the worker advances', () => {
  assert.ok(worldProgress(samples([12, 12, 12, 12, 12], [10, 70, 130, 190, 250])).failures.includes('pageTick stopped advancing'));
});
test('desktop gate rejects a stopped worker and a repeatable low rate', () => {
  assert.ok(worldProgress(samples([0, 0, 0, 0, 0])).failures.includes('worldTick stopped advancing'));
  assert.ok(worldProgress(samples([0, 15, 30, 45, 60]), { minimumTps: 40 }).failures.length);
});
test('desktop gate rejects suspended and lockstep measurements', () => {
  for (const flags of [{ running: false }, { stepping: true }]) {
    const s = samples([0, 60]); Object.assign(s[1], flags);
    assert.ok(worldProgress(s).failures.includes('World is suspended or in lockstep'));
  }
});
