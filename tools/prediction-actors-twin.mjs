import assert from 'node:assert/strict';
import { once } from 'node:events';
import { loadSim } from '../src/sim/module.js';
import { loadRayKernel } from '../src/ray-kernel.js';
import { createActorPredictionSim } from '../src/runtime/actor-prediction-sim.js';
import { actorPredictionCases, runActorPredictionCase } from '../tests/fixtures/prediction-actor-cases.js';
import { actorCollisionGeometry, actorTraceRecord } from './prediction-actors-collision.mjs';

const instance = await loadSim(), rays = await loadRayKernel(instance);
let ticks = 0, operations = 0;
for (const scenario of actorPredictionCases()) {
  const events = []; let actual, expected;
  try {
    actual = runActorPredictionCase(scenario, { rays, makeNative(world) {
      events.push({ type: 'collision', label: scenario.name, geometry: actorCollisionGeometry(world) });
      return createActorPredictionSim(instance, world, { trace: event => events.push(actorTraceRecord(event, world, scenario.name)) });
    }, after({ prediction }) { assert.ok(prediction.nativeSession?.handle); assert.ok(prediction.nativeSession.steps > 0); } });
    expected = runActorPredictionCase(scenario);
    assert.deepEqual(actual.observations, expected.observations, scenario.name);
    ticks += actual.observations.length; operations += events.length - 1;
    for (const event of events) if (!process.stdout.write(JSON.stringify(event) + '\n')) await once(process.stdout, 'drain');
  } finally { actual?.dispose(); expected?.dispose(); }
}
assert.ok(ticks > 3500 && operations > ticks);
process.stderr.write(`actor prediction: ${ticks} exact JS/WASM steps, ${operations} owned native operations\n`);
