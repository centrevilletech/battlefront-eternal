// Save native phases only after the public helper and its callbacks agree.
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { loadSim } from '../src/sim/module.js';
import { createPlayoutSim } from '../src/runtime/playout-sim.js';
import { playoutCases, runPlayoutCase } from '../tests/fixtures/prediction-playout-cases.js';

const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
let traces = [], cases = 0, operations = 0;
const counts = Array(13).fill(0), native = createPlayoutSim(await loadSim(), { trace: event => { traces[traces.length] = event; } });
try {
  for (const scenario of playoutCases()) {
    traces = [];
    const actual = runPlayoutCase(scenario, native), expected = runPlayoutCase(scenario);
    assert.deepEqual(actual.result, expected.result, scenario.name);
    if (scenario.native) assert.ok(actual.calls > 0, `${scenario.name} native delegation`);
    for (const trace of traces) {
      assert.equal(trace.error, null, scenario.name);
      const event = { label: scenario.name, op: trace.op, input: words(trace.input), output: words(trace.output), error: trace.error ?? null };
      if (!process.stdout.write(JSON.stringify(event) + '\n')) await once(process.stdout, 'drain');
      counts[trace.op]++; operations++;
    }
    cases++;
  }
  assert.ok(cases >= 40 && operations > 5000 && counts.every(n => n > 0));
  console.error(JSON.stringify({ cases, operations, counts }));
} finally { native.dispose(); }
