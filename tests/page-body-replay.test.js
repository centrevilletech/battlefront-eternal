import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { createPagePrediction } from '../src/runtime/page-prediction.js';
import { predictVehicle } from '../src/runtime/vehicle-prediction.js';
import { hullPredictionCases, runHullPredictionCase, syntheticHull, hullCommand, hullObservation, observeHullQueries } from './fixtures/prediction-hull-cases.js';
import { remotePredictionCases, runRemotePredictionCase } from './fixtures/prediction-remote-cases.js';
import { mountedPredictionCases, runMountedPredictionCase } from './fixtures/prediction-weapon-cases.js';
import { actorCourse } from './fixtures/prediction-actor-cases.js';

async function pageOwner() {
  const instance = await loadSim(), course = actorCourse(await loadRayKernel(instance));
  const owner = createPagePrediction(instance, course.world);
  return { instance, owner, close() { try { owner.dispose(); } finally { course.dispose(); } } };
}

test('page hull and flight predictions share native command cursors with their body sessions', async () => {
  const { owner, close } = await pageOwner();
  try {
    for (const scenario of hullPredictionCases()) {
      let actual, expected;
      try {
        actual = runHullPredictionCase(scenario, { native: owner.vehicles, replay: owner.replay });
        expected = runHullPredictionCase(scenario);
        assert.deepEqual(actual.observations, expected.observations, scenario.name);
        assert.deepEqual(actual.events, expected.events, `${scenario.name} callbacks`);
        assert.equal(owner.replay.activeSessions, 0);
        if (actual.prediction) assert.equal(actual.prediction.nativeSession.handle, 0);
      } finally { actual?.dispose(); expected?.dispose(); }
    }
    assert.ok(owner.replay.calls > 1000);
  } finally { close(); }
});

test('page recon predictions preserve callbacks and selective restores with native command cursors', async () => {
  const { owner, close } = await pageOwner();
  try {
    for (const scenario of remotePredictionCases()) {
      const actual = runRemotePredictionCase(scenario, { native: owner.remotes, replay: owner.replay });
      const expected = runRemotePredictionCase(scenario);
      assert.deepEqual(actual.observation, expected.observation, scenario.name);
      assert.deepEqual(actual.events, expected.events, `${scenario.name} callbacks`);
      assert.deepEqual(actual.shots, expected.shots, `${scenario.name} shots`);
      assert.equal(actual.result?.tick, expected.result?.tick);
      assert.equal(owner.replay.activeSessions, 0);
      assert.equal(owner.remotes.activeSessions, 0);
      if (actual.result) assert.equal(actual.result.nativeSession.handle, 0);
    }
    assert.ok(owner.replay.calls > 1000);
  } finally { close(); }
});

test('mounted native weapons share command cursors and restore the original weapon and barrel state', async () => {
  const { owner, close } = await pageOwner();
  try {
    for (const scenario of mountedPredictionCases()) {
      let actual, expected;
      try {
        actual = runMountedPredictionCase(scenario, { native: owner.weapons, replay: owner.replay });
        expected = runMountedPredictionCase(scenario);
        for (const field of ['before', 'ticks', 'observations', 'restored']) {
          assert.deepEqual(actual[field], expected[field], `${scenario.name} ${field}`);
        }
        assert.equal(owner.replay.activeSessions, 0);
        assert.equal(actual.prediction.nativeSession, null);
      } finally { actual?.prediction.dispose(); expected?.prediction.dispose(); }
    }
    assert.ok(owner.replay.calls > 1000);
  } finally { close(); }
});

test('nested hull, recon and mounted prediction keep their cursors and copied outputs across growth', async () => {
  const { instance, owner, close } = await pageOwner();
  const scenario = { name: 'nested-ground', kind: 'springs', ticks: 3, batch: 0 };
  const mounted = mountedPredictionCases()[0];
  const smallMounted = { ...mounted, pending: mounted.pending.slice(0, 4) };
  let peak = 0, grew = false;
  const run = native => {
    let entered = false, fired = false, remote, weapons, hull;
    const measure = () => {
      if (!native) return;
      peak = Math.max(peak, owner.replay.activeSessions);
      if (!grew && owner.replay.activeSessions === 3) { instance.exports.memory.grow(1); grew = true; }
    };
    try {
      hull = runHullPredictionCase(scenario, { native: native ? owner.vehicles : null, replay: native ? owner.replay : null,
        onQuery() {
          if (entered) return;
          entered = true; measure();
          remote = runRemotePredictionCase({ ticks: 3 }, { native: native ? owner.remotes : null, replay: native ? owner.replay : null,
            onFired() {
              if (fired) return;
              fired = true; measure();
              weapons = runMountedPredictionCase(smallMounted, { native: native ? owner.weapons : null, replay: native ? owner.replay : null,
                ownerRead: measure, ownerWrite: measure });
            } });
        } });
      assert.ok(entered && fired);
      return { hull: hull.observations, queries: hull.events, remote: remote.observation, shots: remote.shots,
        mounted: weapons.observations, restored: weapons.restored };
    } finally { hull?.dispose(); weapons?.prediction.dispose(); }
  };
  try {
    assert.deepEqual(run(true), run(false));
    assert.equal(peak, 3); assert.equal(grew, true);
    assert.equal(owner.replay.activeSessions, 0); assert.equal(owner.remotes.activeSessions, 0);
  } finally { close(); }
});

test('hull callback failures preserve original thrown values and release the native command cursor', async () => {
  const { owner, close } = await pageOwner();
  try {
    for (const marker of [undefined, null, Error('query failed')]) {
      const run = native => {
        const fixture = syntheticHull('springs', { disabled: true });
        let queries = 0, failed = false, thrown;
        const observer = observeHullQueries(fixture.v, () => { if (++queries === 3) throw marker; });
        try {
          try { predictVehicle(fixture.v, [hullCommand(1)], 0, 3, [], { native: native ? owner.vehicles : null, replay: native ? owner.replay : null }); }
          catch (error) { failed = true; thrown = error; }
          assert.equal(failed, true); assert.equal(thrown, marker);
          assert.equal(fixture.v.body.enabled, false);
          return { state: hullObservation(fixture.v), events: observer.events };
        } finally { observer.dispose(); fixture.system.dispose(); }
      };
      assert.deepEqual(run(true), run(false));
      assert.equal(owner.replay.activeSessions, 0);
    }
  } finally { close(); }
});
