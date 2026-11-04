// Emit native reads after the original readers have matched their values,
// validation failures, and complete restored state for the same payload.
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { loadSim } from '../src/sim/module.js';
import { createCorrectionSim } from '../src/runtime/correction-sim.js';
import { correctionCases, runCorrectionCase } from '../tests/fixtures/prediction-correction-cases.js';

const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
let traces = [], cases = 0, accepted = 0, rejected = 0;
const counts = Array(8).fill(0), native = createCorrectionSim(await loadSim(), { trace: event => traces.push(event) });
try {
  for (const { label, descriptor, variation } of correctionCases()) {
    traces = [];
    const actual = runCorrectionCase(descriptor, variation, native), expected = runCorrectionCase(descriptor, variation);
    for (const key of ['error', 'decoded', 'before', 'after']) assert.deepEqual(actual[key], expected[key], `${label} ${key}`);
    if (label.endsWith(':valid')) assert.ok(actual.calls > 0, `${label} native delegation`);
    for (const trace of traces) {
      const event = { label, op: trace.op, input: words(trace.input), output: words(trace.output), error: trace.error ?? null };
      if (!process.stdout.write(JSON.stringify(event) + '\n')) await once(process.stdout, 'drain');
      counts[trace.op]++;
    }
    cases++; if (actual.error) rejected++; else accepted++;
  }
  assert.ok(accepted > 500 && rejected > 2000 && native.calls > 2500 && counts.every(n => n > 0));
  console.error(JSON.stringify({ cases, accepted, rejected, operations: native.calls, counts }));
} finally { native.dispose(); }
