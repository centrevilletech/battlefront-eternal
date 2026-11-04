// Check private hull replay against JavaScript before emitting copied native
// operations and their query replies. Float words retain their exact bits.
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { createVehiclePredictionSim } from '../src/runtime/vehicle-prediction-sim.js';
import { hullPredictionCases, runHullPredictionCase } from '../tests/fixtures/prediction-hull-cases.js';

const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
let label = '', observations = 0, queries = 0;
const events = [], native = createVehiclePredictionSim(await loadSim(), { trace(event) {
  events.push({ label, session: String(event.session), op: event.op, input: words(event.input), output: words(event.output), error: event.error ?? null,
    queries: (event.queries || []).map(q => ({ op: q.op, args: words(q.args), values: words(q.values) })) });
} });
try {
  for (const scenario of hullPredictionCases()) {
    label = scenario.name; let actual, expected;
    try {
      actual = runHullPredictionCase(scenario, { native }); expected = runHullPredictionCase(scenario);
      assert.deepEqual(actual.observations, expected.observations, label);
      assert.deepEqual(actual.events, expected.events, `${label} query order`);
      observations += actual.observations.length; queries += actual.events.length;
    } finally { actual?.dispose(); expected?.dispose(); }
  }
  assert.ok(observations >= 100); assert.ok(queries > 2000); assert.ok(events.length > 1000);
  for (const event of events) process.stdout.write(JSON.stringify(event) + '\n');
  process.stderr.write(`hull prediction: ${observations} JS/WASM observations, ${queries} foreign calls, ${events.length} native operations\n`);
} finally { native.dispose(); }
