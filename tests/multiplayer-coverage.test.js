import test from 'node:test';
import assert from 'node:assert/strict';
import {actorCoverage} from '../tools/multiplayer-coverage.mjs';

test('coverage exposes never-delivered current generations and does not count the own soldier', () => {
  const roster = {tick: 600, actors: [{kind: 'soldier', id: 1, generation: 2}, {kind: 'soldier', id: 2, generation: 3}, {kind: 'vehicle', id: 3, generation: 1}]};
  const result = actorCoverage(roster, [{index: 0, ownId: 1, observedActorLives: [['soldier:2:2', 900], ['vehicle:3:1', 50], ['soldier:99:1', 9000]]}]);
  assert.deepEqual(result.clients, [{index: 0, expected: 2, received: 1, missing: [{kind: 'soldier', id: 2, generation: 3}], receivedUpdateWallAgeMs: {count: 1, p50: 50, p95: 50, max: 50}}]);
  assert.equal(result.referenceTick, 600);
  const complete = actorCoverage(roster, [{index: 1, ownId: 2, observedActorLives: [['soldier:1:2', 100], ['vehicle:3:1', 20]]}]);
  assert.equal(complete.clients[0].missing.length, 0);
});

test('an older server without the measurement census is explicitly unavailable', () => {
  assert.deepEqual(actorCoverage(undefined, []), {available: false});
});

test('never received live actors do not produce a misleading zero-age result', () => {
  const result = actorCoverage({tick: 1, actors: [{kind: 'soldier', id: 2, generation: 1}]}, [{index: 0, ownId: 1, observedActorLives: []}]);
  assert.equal(result.clients[0].missing.length, 1);
  assert.deepEqual(result.receivedUpdateWallAgeMs, {count: 0, p50: null, p95: null, max: null});
});
