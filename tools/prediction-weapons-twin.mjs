// One owned corpus for the JavaScript, WebAssembly and native prediction
// twins. Every trace is emitted only after its visible replay matches JS.
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { createWeaponPredictionSim } from '../src/runtime/weapon-prediction-sim.js';
import { weaponPredictionCases, runWeaponPredictionCase, mountedPredictionCases, runMountedPredictionCase } from '../tests/fixtures/prediction-weapon-cases.js';

let label = '';
const traces = [], native = createWeaponPredictionSim(await loadSim(), { trace(event) {
  traces.push({ label, session: String(event.session), op: event.op, input: Array.from(event.input), output: Array.from(event.output), error: event.error });
} });
let observations = 0;
for (const scenario of weaponPredictionCases()) {
  label = scenario.name;
  let actual, expected;
  try {
    actual = runWeaponPredictionCase(scenario, { native, after({ prediction, operation }) {
      if (operation.op === 'begin') assert.ok(prediction.nativeSession?.handle, label);
    } });
    expected = runWeaponPredictionCase(scenario);
    assert.deepEqual(actual.observations, expected.observations, label);
    observations += actual.observations.length;
  } finally { actual?.prediction.dispose(); expected?.prediction.restore(); expected?.prediction.dispose?.(); }
}
for (const scenario of mountedPredictionCases()) {
  label = scenario.name;
  let actual, expected;
  try {
    actual = runMountedPredictionCase(scenario, { native }); expected = runMountedPredictionCase(scenario);
    for (const key of ['before', 'ticks', 'observations', 'restored']) assert.deepEqual(actual[key], expected[key], `${label} ${key}`);
    observations += actual.observations.length;
  } finally { actual?.prediction.dispose(); expected?.prediction.restore(); expected?.prediction.dispose?.(); }
}
assert.ok(traces.length > 1000); assert.ok(observations >= 950);
const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
for (const trace of traces) process.stdout.write(JSON.stringify({ ...trace, input: words(trace.input), output: words(trace.output), error: trace.error ?? null }) + '\n');
process.stderr.write(`weapon prediction: ${observations} JS/WASM observations, ${traces.length} owned native operations\n`);
