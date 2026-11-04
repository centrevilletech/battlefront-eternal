// Emit owned native commands only after the same recon replay has matched
// JavaScript's state, query order and every partially published shot.
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { loadSim } from '../src/sim/module.js';
import { createRemotePredictionSim } from '../src/runtime/remote-prediction-sim.js';
import { remotePredictionCases, runRemotePredictionCase } from '../tests/fixtures/prediction-remote-cases.js';

const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
const instance = await loadSim();
let cases = 0, steps = 0, operations = 0, shots = 0;
for (const scenario of remotePredictionCases()) {
  const traces = [], native = createRemotePredictionSim(instance, { trace: event => traces.push(event) });
  try {
    const actual = runRemotePredictionCase(scenario, { native }), expected = runRemotePredictionCase(scenario);
    assert.deepEqual(actual.observation, expected.observation, `${scenario.name} state`);
    assert.deepEqual(actual.events, expected.events, `${scenario.name} foreign call order`);
    assert.deepEqual(actual.shots, expected.shots, `${scenario.name} shots`);
    assert.equal(actual.result?.tick, expected.result?.tick);
    assert.equal(native.activeSessions, 0);
    if (actual.result) assert.ok(actual.result.nativeSession.steps > 0, `${scenario.name} native delegation`);
    assert.ok(traces.some(trace => trace.op === 2), `${scenario.name} native command`);
    for (const trace of traces) {
      const event = { label: scenario.name, session: String(trace.session), op: trace.op,
        input: words(trace.input), output: words(trace.output), error: trace.error ?? null };
      if (!process.stdout.write(JSON.stringify(event) + '\n')) await once(process.stdout, 'drain');
      operations++; if (trace.op === 2) steps++;
    }
    for (const session of new Set(traces.map(trace => String(trace.session)))) {
      if (!process.stdout.write(JSON.stringify({ type: 'dispose', label: scenario.name, session }) + '\n')) await once(process.stdout, 'drain');
    }
    cases++; shots += actual.shots.length;
  } finally { native.dispose(); }
}
assert.ok(cases >= 19 && steps > 1400 && shots > 100);
console.error(JSON.stringify({ cases, steps, shots, operations }));
