import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { InfantryPrediction } from '../src/runtime/infantry-prediction.js';
import { DroidekaPrediction } from '../src/runtime/droideka-prediction.js';
import { createActorPredictionSim } from '../src/runtime/actor-prediction-sim.js';
import { actorCourse, actorInput, actorObservation, createPredictionActor } from './fixtures/prediction-actor-cases.js';

function caught(run) {
  try { return { value: run(), thrown: false }; }
  catch (value) { return { value, thrown: true }; }
}
function polluted(key, descriptor, run) {
  const saved = Object.getOwnPropertyDescriptor(Object.prototype, key);
  Object.defineProperty(Object.prototype, key, { __proto__: null, configurable: true, ...descriptor });
  try { return run(); }
  finally {
    delete Object.prototype[key];
    if (saved) Object.defineProperty(Object.prototype, key, saved);
  }
}

test('actor input accessors retain reads and partial-state throws under inherited descriptor values', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance);
  for (const throws of [false, true]) {
    const run = useNative => {
      const course = actorCourse(useNative ? rays : null), scenario = { kind: 'soldier', pattern: 'walk' };
      const actor = createPredictionActor(scenario, course), native = useNative ? createActorPredictionSim(instance, course.world) : null;
      const prediction = new InfantryPrediction(actor.soldier, actor.combat, undefined, { native });
      const input = actorInput(scenario, 0), events = [], marker = { reason: 'input stopped' };
      Object.defineProperty(input, 'forward', { configurable: true, get() {
        events.push(actor.soldier.stateTime); if (throws) throw marker; return .5;
      } });
      try {
        const result = polluted('value', { value: 0, writable: true }, () => caught(() => prediction.step(input, 1 / 60)));
        return { events, thrown: result.thrown, original: result.value === marker, steps: prediction.nativeSession?.steps ?? 0,
          observation: actorObservation(actor.soldier, prediction.kick) };
      } finally { prediction.dispose(); native?.dispose(); course.dispose(); }
    };
    assert.deepEqual(run(true), run(false), String(throws));
  }
});

test('actor method descriptors inspect only their own value, get and set fields', async () => {
  const instance = await loadSim(), rays = await loadRayKernel(instance);
  for (const kind of ['soldier', 'droideka']) for (const key of ['value', 'get', 'set']) {
    const run = useNative => {
      const course = actorCourse(useNative ? rays : null), scenario = { kind, pattern: 'walk' };
      const actor = createPredictionActor(scenario, course), native = useNative ? createActorPredictionSim(instance, course.world) : null;
      const Prediction = kind === 'soldier' ? InfantryPrediction : DroidekaPrediction;
      const prediction = new Prediction(actor.soldier, actor.combat, undefined, { native });
      const input = actorInput(scenario, 0), events = [], marker = { reason: 'descriptor stopped' };
      try {
        const result = polluted(key, { get() { events.push(key); throw marker; } }, () => caught(() => prediction.step(input, 1 / 60)));
        return { events, thrown: result.thrown, original: result.value === marker,
          observation: actorObservation(actor.soldier, prediction.kick), steps: prediction.nativeSession?.steps ?? 0 };
      } finally { prediction.dispose(); native?.dispose(); course.dispose(); }
    };
    const actual = run(true), expected = run(false);
    assert.equal(actual.steps, 1); delete actual.steps; delete expected.steps;
    assert.deepEqual(actual, expected, `${kind} ${key}`);
  }
});
