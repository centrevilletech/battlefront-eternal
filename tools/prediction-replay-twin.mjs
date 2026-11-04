// Retain native transcripts only after live replay values and aliases match.
import assert from 'node:assert/strict';
import { once } from 'node:events';
import { loadSim } from '../src/sim/module.js';
import { createReplaySim } from '../src/runtime/replay-sim.js';
import { replayCases, runReplayCase } from '../tests/fixtures/prediction-replay-cases.js';

const words = values => {
  const bytes = Buffer.alloc(values.length * 8);
  for (let i = 0; i < values.length; i++) bytes.writeDoubleLE(values[i], i * 8);
  return bytes.toString('hex');
};
let traces = [], cases = 0, throws = 0, sessions = 0, interleaved = false;
const counts = Array(4).fill(0), live = new Set(), seen = new Set();
const native = createReplaySim(await loadSim(), { trace: event => traces.push(event) });
try {
  for (const scenario of replayCases()) {
    traces = [];
    const actual = runReplayCase(scenario, native), expected = runReplayCase(scenario);
    assert.deepEqual(actual.result, expected.result, scenario.name);
    if (scenario.native) assert.ok(actual.calls > 0, `${scenario.name} native delegation`);
    for (const trace of traces) {
      assert.equal(trace.error, null, scenario.name);
      if (trace.op === 'create') {
        assert.ok(!seen.has(trace.session), 'session serials cannot be reused'); seen.add(trace.session); live.add(trace.session); sessions++;
        if (live.size > 1) interleaved = true;
      } else if (trace.op === 'drop') {
        assert.ok(live.delete(trace.session), 'only a live session can be dropped');
      } else {
        assert.ok(live.has(trace.session), 'numeric exchanges require an owned session');
        assert.ok(Number.isInteger(trace.op) && trace.op >= 0 && trace.op < 4); counts[trace.op]++;
      }
      const event = { label: scenario.name, session: trace.session, op: trace.op, input: words(trace.input), output: words(trace.output), error: trace.error ?? null };
      if (!process.stdout.write(JSON.stringify(event) + '\n')) await once(process.stdout, 'drain');
    }
    assert.equal(native.activeSessions, 0, `${scenario.name} releases its owner`);
    assert.equal(live.size, 0); cases++; throws += +actual.result.thrown;
  }
  assert.ok(cases >= 80 && throws >= 10 && sessions >= 40 && counts[0] >= 40 && counts[1] > 500 && counts[2] > 500 && interleaved);
  console.error(JSON.stringify({ cases, throws, sessions, counts, interleaved }));
} finally { native.dispose(); }
