// Keep native phases only after the same public helper has matched the
// original reader's values, identities, callbacks, failures and restore state.
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { loadSim } from '../src/sim/module.js';
import { createCorrectionSim } from '../src/runtime/correction-sim.js';
import { stateReaderCases, runStateReaderCase } from '../tests/fixtures/prediction-state-reader-cases.js';

const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
let traces = [], accepted = 0, rejected = 0, cases = 0, operations = 0;
const counts = Array(19).fill(0), native = createCorrectionSim(await loadSim(), { trace: event => traces.push(event) });
try {
  for (const scenario of stateReaderCases()) {
    traces = [];
    const actual = runStateReaderCase(scenario, native), expected = runStateReaderCase(scenario);
    assert.deepEqual(actual.result, expected.result, scenario.name);
    if (scenario.name.endsWith(':valid')) assert.ok(actual.calls > 0, `${scenario.name} native delegation`);
    for (const trace of traces) {
      const event = { label: scenario.name, op: trace.op, input: words(trace.input), output: words(trace.output), error: trace.error ?? null };
      if (!process.stdout.write(JSON.stringify(event) + '\n')) await once(process.stdout, 'drain');
      counts[trace.op]++; operations++;
    }
    cases++; if (actual.result.error) rejected++; else accepted++;
  }
  assert.ok(cases >= 2200 && accepted > 300 && rejected > 1500 && operations > 2500 && counts.slice(9).every(n => n > 0));
  console.error(JSON.stringify({ cases, accepted, rejected, operations, counts }));
} finally { native.dispose(); }
