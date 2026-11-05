import test from 'node:test';
import assert from 'node:assert/strict';
import { hullFixture } from './fixtures/client-hull.js';
import { createHullReplay } from './helpers/hull-prediction.js';
import { captureGroundCorrection } from '../src/runtime/network-entity-ground.js';
import { instrumentClientPrediction } from './fixtures/client-prediction-diagnostics.js';

// Frozen byte/physics comparisons live in client-hull-prediction.test.js.
// These cases exercise the frame contract around missing and rolled-back input,
// body isolation and reconnect identity without retaining a JavaScript replay.
const command = (tick, sequence, generation = 1) => ({ tick, sequence, generation,
  down: ['KeyW'], actions: [], look: [], mouseScale: 1 });
test('native hull replay retains held keys through gaps and takes a rolled-back tick once', () => {
  const f = hullFixture('hover'), replay = createHullReplay(f.vehicle);
  instrumentClientPrediction(replay.session);
  try {
    const before = captureGroundCorrection(f.vehicle), body = f.vehicle.body.matrix.toArray();
    const predicted = replay.predict([command(4, 1)], 0, ['KeyW']);
    assert.equal(replay.session.result.tick, 4); assert.equal(replay.session.result.ticks, 4);
    assert.ok(predicted.position.z > f.vehicle.position.z);
    assert.deepEqual(captureGroundCorrection(f.vehicle), before);
    assert.deepEqual(f.vehicle.body.matrix.toArray(), body);
    const calls = replay.session.counters.calls;
    replay.predict([], 0, ['KeyW']);
    assert.equal(replay.session.result.tick, 4); assert.equal(replay.session.result.ticks, 0);
    assert.equal(replay.session.counters.calls, calls + 1, 'one frame call even with no new command');
    replay.predict([command(9, 2)], 10, ['KeyW']);
    assert.equal(replay.session.result.tick, 11); assert.equal(replay.session.result.ticks, 1);
    assert.equal(replay.session.result.shotsCount, 0, 'hull motion does not invent mounted shot notices');
  } finally { replay.dispose(); f.system.dispose(); }
});
test('a new unit life and a reconnect epoch each replace the private hull checkpoint', () => {
  const f = hullFixture('hover'), replay = createHullReplay(f.vehicle);
  try {
    replay.predict([command(12, 1)], 0);
    const unit = { id: 90, generation: 2, alive: true, combat: { dead: false }, seat: { vehicle: f.vehicle, index: 0 } };
    f.vehicle.position.x = 50;
    replay.session.mounted(unit, [], [command(5, 2, 2)], 4, [], f.vehicle, 1, captureGroundCorrection(f.vehicle), 10);
    assert.equal(replay.session.result.continued, false);
    assert.equal(replay.session.result.tick, 5);
    assert.equal(replay.session.vehicle.position.x, 50);
    f.vehicle.position.x = 80;
    replay.session.mounted(unit, [], [], 4, [], f.vehicle, 2, captureGroundCorrection(f.vehicle), 10);
    assert.equal(replay.session.result.continued, false);
    assert.equal(replay.session.vehicle.position.x, 80);
    assert.equal(replay.session.result.ticks, 0);
  } finally { replay.dispose(); f.system.dispose(); }
});
