import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { createWireBridge } from '../src/runtime/wire-sim.js';
import { predictVehicle, predictionVehicle } from '../src/runtime/vehicle-prediction.js';
import { createVehiclePredictionSim, createVehiclePredictionBridge } from '../src/runtime/vehicle-prediction-sim.js';
import { hullPredictionCases, runHullPredictionCase, syntheticHull, hullCommand, hullObservation, observeHullQueries } from './fixtures/prediction-hull-cases.js';

test('private native hull replay matches correction bytes, crew, clone sharing and retained history', async () => {
  const traces = [], native = createVehiclePredictionSim(await loadSim(), { trace: event => traces.push(event) });
  let observations = 0, queries = 0;
  try {
    for (const scenario of hullPredictionCases()) {
      let a, b;
      try {
        a = runHullPredictionCase(scenario, { native, after({ prediction, batch }) {
          if (prediction && batch.some(c => c.generation === 3)) {
            assert.ok(prediction.nativeSession, `${scenario.name} delegated`);
            assert.equal(prediction.nativeSession.handle, 0, 'completed replay releases its native session');
          }
        } });
        b = runHullPredictionCase(scenario);
        assert.deepEqual(a.observations, b.observations, scenario.name);
        assert.deepEqual(a.events, b.events, `${scenario.name} callback order and exact arguments`);
        observations += a.observations.length; queries += a.events.length;
        if (scenario.options && ['deepWater', 'deathRegion', 'belowWorld', 'crash'].some(k => scenario.options[k])) {
          assert.equal(a.prediction.alive, true, `${scenario.name} prediction suppresses damage`);
          assert.equal(a.prediction.health, a.source.health);
        }
      } finally { a?.dispose(); b?.dispose(); }
    }
    assert.ok(observations >= 100); assert.ok(queries > 40000); assert.ok(traces.length > 1000);
  } finally { native.dispose(); }
});

test('ground takes two physics steps and flight one before each walker presentation step', async () => {
  const native = createVehiclePredictionSim(await loadSim());
  try {
    for (const kind of ['springs', 'walker', 'flyer']) {
      const scenario = { name: kind, kind, ticks: 12, batch: 0 }, reference = [], delegated = [];
      const create = events => () => {
        const fixture = syntheticHull(kind), v = fixture.v;
        for (const method of ['tick', 'walk']) {
          const original = v[method];
          v[method] = function (dt, ...args) { events.push([method, dt, this.walkPhase, this.walkBlend]); return original.call(this, dt, ...args); };
        }
        return fixture;
      };
      let a, b;
      try {
        a = runHullPredictionCase(scenario, { native, create: create(delegated) });
        b = runHullPredictionCase(scenario, { create: create(reference) });
        assert.deepEqual(a.observations, b.observations);
        assert.deepEqual(delegated, [], 'native replay does not call JavaScript physics or walk');
        const stride = kind === 'flyer' ? 2 : 3;
        assert.equal(reference.length, 12 * stride);
        for (let at = 0; at < reference.length; at += stride) {
          assert.deepEqual(reference[at].slice(0, 2), ['tick', kind === 'flyer' ? 1 / 60 : 1 / 120]);
          if (kind !== 'flyer') assert.deepEqual(reference[at + 1].slice(0, 2), ['tick', 1 / 120]);
          assert.deepEqual(reference[at + stride - 1].slice(0, 2), ['walk', 1 / 60]);
        }
        if (kind === 'walker') assert.ok(a.prediction.walkBlend > 0 && a.prediction.walkPhase > 0);
      } finally { a?.dispose(); b?.dispose(); }
    }
  } finally { native.dispose(); }
});

test('prediction leaves authority damage and collider publication intact', async () => {
  const native = createVehiclePredictionSim(await loadSim()), fixture = syntheticHull('hover', { deathRegion: true });
  try {
    const source = fixture.v, before = hullObservation(source), matrix = source.body.matrix.toArray();
    const predicted = predictVehicle(source, [hullCommand(1)], 0, 3, [], { native });
    assert.equal(predicted.alive, true); assert.equal(predicted.health, source.maxHealth);
    assert.deepEqual(hullObservation(source), before); assert.deepEqual(source.body.matrix.toArray(), matrix);
    source.tick(1 / 120, {});
    assert.equal(source.alive, false, 'the default authoritative damage path remains active');
    assert.equal(source.health, 0);
  } finally { native.dispose(); fixture.system.dispose(); }
});

test('query and trace callbacks retain owned state across reentry and memory growth', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance), wire = createWireBridge(bridge), traces = [];
  let grew = false, reentered = false, foreign = 0;
  const inner = syntheticHull('hover');
  const exercise = () => {
    assert.ok(wire.encodeWordDelta(new Uint8Array([1, 4, 8, 16])) instanceof Uint8Array);
    foreign++;
    if (!grew) { grew = true; instance.exports.memory.grow(1); }
  };
  const native = createVehiclePredictionBridge(bridge, { trace(event) { traces.push(structuredClone(event)); exercise(); } });
  const scenario = { name: 'query-reentry', kind: 'walker', ticks: 18, batch: 6, options: { legs: true, sharedLegSide: true } };
  const mutate = () => {
    let calls = 0, shared;
    return (event, source) => {
      shared ||= source.legSide; assert.equal(source.legSide, shared, 'query mutation retains the shared array');
      if (event[0] === 'cast' && ++calls % 3 === 0) {
        source.legSide[0] += .03125; source.legSide[1] = -source.legSide[1] + .015625;
      }
    };
  };
  const actualMutation = mutate(), expectedMutation = mutate();
  let a, b;
  try {
    a = runHullPredictionCase(scenario, { native, onQuery(event, source) {
      actualMutation(event, source);
      exercise();
      if (!reentered) { reentered = true; const result = predictVehicle(inner.v, [hullCommand(1)], 0, 3, [], { native }); assert.ok(result.nativeSession); }
    } });
    b = runHullPredictionCase(scenario, { onQuery: expectedMutation });
    assert.deepEqual(a.observations, b.observations); assert.deepEqual(a.events, b.events);
    assert.ok(grew && reentered && foreign > 100); assert.ok(traces.length > 20);
    for (const event of traces) {
      assert.ok(Array.isArray(event.input)); assert.ok(Array.isArray(event.output));
      for (const q of event.queries || []) { assert.ok(Array.isArray(q.args)); assert.ok(Array.isArray(q.values)); }
    }
    const rays = await loadRayKernel(instance);
    for (const kind of ['springs', 'walker']) {
      const c = { name: `${kind}-shared-native-collision`, kind, ticks: 12, batch: 4 };
      const create = () => syntheticHull(kind, { rays });
      let actual, expected, rayGrew = false;
      try {
        actual = runHullPredictionCase(c, { native, create, onQuery() {
          exercise(); if (!rayGrew) { rayGrew = true; instance.exports.memory.grow(1); }
        } });
        expected = runHullPredictionCase(c, { create });
        assert.deepEqual(actual.observations, expected.observations); assert.deepEqual(actual.events, expected.events);
        assert.equal(actual.prediction.system.collision.rays, rays, 'private queries keep the shared native collision');
        assert.equal(actual.source.system.collision.rays, rays, 'authority keeps its native collision');
        assert.ok(rayGrew);
      } finally { actual?.dispose(); expected?.dispose(); }
    }
  } finally { a?.dispose(); b?.dispose(); native.dispose(); inner.system.dispose(); }
});

test('a failed query restores the shared collider and releases the native replay', async () => {
  const native = createVehiclePredictionSim(await loadSim());
  const run = backend => {
    const fixture = syntheticHull('springs', { disabled: true }), before = hullObservation(fixture.v), error = Error('collision observer failed');
    const matrix = fixture.v.body.matrix.toArray(); let calls = 0;
    const observer = observeHullQueries(fixture.v, () => { if (++calls === 3) throw error; });
    try {
      assert.throws(() => predictVehicle(fixture.v, [hullCommand(1)], 0, 3, [], { native: backend }), e => e === error);
      assert.equal(fixture.v.body.enabled, false); assert.deepEqual(fixture.v.body.matrix.toArray(), matrix);
      assert.deepEqual(hullObservation(fixture.v), before);
      return observer.events;
    } finally { observer.dispose(); fixture.system.dispose(); }
  };
  try { assert.deepEqual(run(native), run(null)); }
  finally { native.dispose(); }
});

test('repeated prediction owners release sessions while retaining usable JS history', async () => {
  const instance = await loadSim(), native = createVehiclePredictionSim(instance), fixture = syntheticHull('walker'), handles = [];
  const before = hullObservation(fixture.v); let previous = fixture.v;
  try {
    for (let tick = 1; tick <= 80; tick++) {
      previous = predictVehicle(previous, [hullCommand(tick)], tick - 1, 3, [], { native });
      const session = previous.nativeSession; assert.ok(session); handles.push(session); assert.equal(session.handle, 0);
      session.dispose(); session.dispose(); if (tick % 13 === 0) instance.exports.memory.grow(1);
      assert.deepEqual(hullObservation(fixture.v), before);
    }
    assert.equal(previous.predictionPath.size, 61); assert.equal(previous.predictionStates.size, 61);
    native.dispose(); native.dispose(); assert.ok(handles.every(h => h.handle === 0));
    assert.throws(() => predictVehicle(previous, [hullCommand(81)], 80, 3, [], { native }), /disposed/);
    assert.deepEqual(hullObservation(fixture.v), before);
    const continuation = predictVehicle(predictionVehicle(previous), [hullCommand(81)], 80, 3);
    assert.equal(continuation.predictionTick, 81, 'native disposal does not invalidate copied state');
  } finally { native.dispose(); fixture.system.dispose(); }
});
