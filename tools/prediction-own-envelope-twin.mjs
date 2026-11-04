// Emit the same native envelope commands whose decoded values and failures
// have matched the original reader, including byte ownership and key order.
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { loadSim } from '../src/sim/module.js';
import { createCorrectionSim } from '../src/runtime/correction-sim.js';
import { ownEnvelopeCases, runOwnEnvelopeCase } from '../tests/fixtures/prediction-own-envelope-cases.js';

const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
let traces = [], operations = 0, accepted = 0, rejected = 0;
const native = createCorrectionSim(await loadSim(), { trace: event => traces.push(event) });
try {
  for (const scenario of ownEnvelopeCases()) {
    traces = [];
    const actual = runOwnEnvelopeCase(scenario, native), expected = runOwnEnvelopeCase(scenario);
    assert.deepEqual(actual.error, expected.error, `${scenario.label} error`);
    assert.deepEqual(actual.value, expected.value, `${scenario.label} value, key order and source aliases`);
    if (scenario.label.endsWith(':valid')) assert.ok(actual.calls > 0, `${scenario.label} native delegation`);
    if (actual.error) rejected++; else accepted++;
    for (const trace of traces) {
      assert.equal(trace.op, 8);
      const event = { label: scenario.label, op: trace.op, input: words(trace.input), output: words(trace.output), error: trace.error ?? null };
      if (!process.stdout.write(JSON.stringify(event) + '\n')) await once(process.stdout, 'drain');
      operations++;
    }
  }
  assert.ok(operations > 5000 && accepted > 1000 && rejected > 3000);
  console.error(JSON.stringify({ operations, accepted, rejected }));
} finally { native.dispose(); }
