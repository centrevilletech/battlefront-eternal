import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { createReconcileSim, createReconcileBridge } from '../src/runtime/reconcile-sim.js';
import { createWireBridge } from '../src/runtime/wire-sim.js';
import { WorldTickEstimator, InputLead, CorrectionSmoother } from '../src/runtime/reconcile.js';
import { PredictionWindow, retainPredictionCommands } from '../src/runtime/prediction-window.js';
import { reconcileNumericCases, reconcileCallbackCases, reconcileArithmeticCases, runReconcileCase, ringBytes, leadState, smootherState, estimatorState, attempt, windowFixture } from './fixtures/prediction-reconcile-cases.js';

const callback = name => reconcileCallbackCases().find(c => c.name === name);
function twin(scenario, native) {
  const actual = runReconcileCase(scenario, native), expected = runReconcileCase(scenario);
  assert.deepEqual(actual.result, expected.result, scenario.name);
  if (scenario.native) assert.ok(actual.calls > 0, `${scenario.name} delegates`);
  return actual.result;
}

test('native timing and window phases match numeric boundaries and retained command identity', async () => {
  const counts = Array(16).fill(0), native = createReconcileSim(await loadSim(), { trace: e => counts[e.op]++ });
  try {
    const scenarios = reconcileNumericCases();
    for (const scenario of scenarios) twin(scenario, native);
    assert.equal(scenarios.length, 45);
    assert.ok(counts.every(n => n > 0), `every phase executes: ${counts}`);
    assert.ok(native.calls > 5000);
  } finally { native.dispose(); }
});

test('timing callbacks see partial publication, live reads, throws and exact reset order', async () => {
  const native = createReconcileSim(await loadSim());
  try {
    for (const scenario of reconcileCallbackCases().filter(c => !c.name.startsWith('window:') && !c.name.startsWith('retain:'))) twin(scenario, native);
    const reset = twin(callback('smoother:reset-sees-published-note-and-increment-is-late'), native);
    assert.equal(reset.first.snaps, 41);
    assert.equal(reset.final.snaps, 70, 'a throwing reset prevents the later increment');
    assert.equal(reset.error.error.marker, 'reset-stop');
    const measured = twin(callback('lead:measure-publishes-key-before-live-slice-sort'), native);
    assert.deepEqual(measured.events[0], ['slice', 0, 5, 5 * 4096 + 3, 0]);
    assert.equal(measured.measured.worstGap, 8, 'sort callback changes the later percentile read');
    assert.equal(measured.events.filter(e => e[0] === 'throw').length, 1, 'the cached key survives failure');
    const target = twin(callback('lead:target-captures-default-latest-before-callbacks'), native);
    assert.equal(target.state.latest, 100); assert.equal(target.result - target.state.lead, 10);
  } finally { native.dispose(); }
});

test('window callbacks retain the old checkpoint and original array methods across mutation', async () => {
  const native = createReconcileSim(await loadSim());
  try {
    for (const scenario of reconcileCallbackCases().filter(c => c.name.startsWith('window:') || c.name.startsWith('retain:'))) twin(scenario, native);
    const retained = twin(callback('window:exact-keeps-old-checkpoint-after-nested-save'), native);
    assert.equal(retained.old, true); assert.equal(retained.currentDifferent, true); assert.equal(retained.currentTick, 90);
    assert.equal(retained.authorityTick, 25); assert.equal(retained.from, 31); assert.deepEqual(retained.indices, [2]);
    const filtered = twin(callback('window:filter-getters-see-live-checkpoint-and-saved-length'), native);
    assert.deepEqual(filtered.indices, [1, 2]); assert.equal(filtered.pendingLength, 6);
    const saved = twin(callback('window:save-at-and-late-property-reads'), native);
    assert.equal(saved.same, true); assert.equal(saved.held, true); assert.equal(saved.state, true); assert.ok(Object.is(saved.sequence, -0));
    const lead = new InputLead({}, { native }), e = new WorldTickEstimator({}, { native }), s = new CorrectionSmoother({}, { native });
    assert.deepEqual(Object.keys(lead), Object.keys(new InputLead()));
    assert.deepEqual(Object.keys(e), Object.keys(new WorldTickEstimator()));
    assert.deepEqual(Object.keys(s), Object.keys(new CorrectionSmoother()));
    assert.deepEqual(Object.keys(new PredictionWindow(undefined, { native })), Object.keys(new PredictionWindow()));
  } finally { native.dispose(); }
});

test('ring writes preserve untouched payload bits and percentile fallback stays local', async () => {
  const trace = [], native = createReconcileSim(await loadSim(), { trace: event => trace.push(event.op) });
  const scalarBits = n => { const b = Buffer.alloc(8); b.writeDoubleLE(n); return b.toString('hex'); };
  const sample = (native, count) => {
    const lead = new InputLead({ gapWindow: 25 }, { native }), view = new DataView(lead.gaps.buffer);
    for (let i = 0; i < 25; i++) view.setBigUint64(i * 8, i < 4 ? BigInt(i) : 0x7ff0000000000100n + BigInt(i), true);
    lead.gapCount = count; lead.gapAt = 1; const bytes = ringBytes(lead.gaps);
    lead.measure(); return { bits: scalarBits(lead.worstGap), before: bytes, after: ringBytes(lead.gaps), key: lead.measuredKey };
  };
  try {
    const payloads = twin(callback('models:untouched-scalar-nan-payloads-and-raw-arrival-arguments'), native);
    for (const entry of payloads) {
      for (const key of ['worstBefore', 'worst', 'extraBefore', 'extra', 'latestBefore', 'latest', 'arrivedAt', 'dropped']) assert.equal(entry[key], entry.pattern, key);
    }
    const inheritance = twin(callback('models:skipped-inherited-fields-remain-inherited'), native);
    assert.deepEqual(inheritance, { ack: { extra: 0, ownExtra: false, ownCalm: false }, arrival: { latest: 0, ownLatest: false, ownAt: false, ownCount: false }, smoother: { worst: 10, ownWorst: false }, estimator: { count: 1, ownCount: false } });
    const retained = twin(callback('estimator:untouched-ring-payload-and-zero-tie'), native);
    assert.equal(retained.before, retained.reset); assert.equal(retained.identity, true); assert.equal(retained.zeroTie, true);
    assert.equal(retained.first.bytes.slice(16), retained.before.slice(16));
    assert.equal(retained.second.bytes.slice(32), retained.before.slice(32));
    const at = trace.length, actual = sample(native, 25), expected = sample(null, 25);
    assert.deepEqual(actual, expected); assert.equal(actual.before, actual.after);
    assert.ok(trace.slice(at).includes(6)); assert.ok(!trace.slice(at).includes(7), 'only the NaN sort phase stays in JavaScript');
    const finite = trace.length; assert.deepEqual(sample(native, 4), sample(null, 4));
    assert.ok(trace.slice(finite).includes(7), 'NaNs outside the sampled prefix do not disable native percentile');
    const lead = new InputLead({ gapWindow: 4 }, { native }); lead.gaps.set([-0, 0, -0, 0]); lead.gapCount = 1;
    lead.measure(); assert.ok(Object.is(lead.worstGap, -0));
  } finally { native.dispose(); }
});

test('arithmetic NaN bits match JavaScript and extreme numeric phases retain their fallback', async () => {
  const phases = [], native = createReconcileSim(await loadSim(), { trace: e => phases.push(e.op) });
  const raw = n => { const b = Buffer.alloc(8); b.writeDoubleLE(n); return b.toString('hex'); };
  try {
    const cases = reconcileArithmeticCases(); assert.equal(cases.length, 40);
    for (const scenario of cases) twin(scenario, native);
    for (const margin of [0, -0, 1, Number.MAX_SAFE_INTEGER, -Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER + 1, Number.MAX_VALUE, -Number.MAX_VALUE, Infinity, -Infinity, NaN]) {
      const run = native => {
        const lead = new InputLead({}, { native }); lead.measure = () => {}; lead.estimator.estimate = () => 0; lead.margin = margin;
        const result = lead.target(0, 0); return { result: raw(result), lead: raw(lead.lead), capped: lead.capped };
      };
      const expected = run(null), at = phases.length;
      assert.deepEqual(run(native), expected, `target margin ${margin}`);
      assert.equal(phases.slice(at).includes(8), Number.isFinite(margin) && Math.abs(margin) <= Number.MAX_SAFE_INTEGER);
    }
    for (const tau of [.03, Number.MIN_VALUE, 0, -0, -.03, Number.MAX_VALUE, Infinity, -Infinity, NaN]) {
      const run = native => {
        const smoother = new CorrectionSmoother({ tau }, { native }); smoother.x = .5; smoother.y = -.25; smoother.z = .75; smoother.decay(.01);
        return { x: raw(smoother.x), y: raw(smoother.y), z: raw(smoother.z), length: raw(smoother.length) };
      };
      const expected = run(null), at = phases.length;
      assert.deepEqual(run(native), expected, `decay tau ${tau}`);
      assert.equal(phases.slice(at).includes(11), tau > 0 && tau <= Number.MAX_SAFE_INTEGER);
    }
  } finally { native.dispose(); }
});

test('owned phase results survive shared-bridge reentry, growth, trace mutation and teardown', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance), wire = createWireBridge(bridge);
  let nested = false, armed = false, grew = false, native, e, lead;
  const events = [];
  native = createReconcileBridge(bridge, { trace(event) {
    event.input.fill(-999); event.output.fill(123);
    if (!armed || nested) return;
    nested = true;
    try {
      if (event.op === 1) {
        events.push(['published', e.offset, e.at, e.count]);
        const result = lead.target(.4, 4, .018); events.push(['nested', result]);
        wire.encodeWordDelta(new Uint8Array([1, 2, 3, 4]));
        if (!grew) { instance.exports.memory.grow(1); grew = true; }
      }
    } finally { nested = false; }
  } });
  try {
    e = new WorldTickEstimator({ window: 3 }, { native }); lead = new InputLead({}, { native }); armed = true;
    const plain = new WorldTickEstimator({ window: 3 });
    e.arrive(40, .5); plain.arrive(40, .5);
    assert.deepEqual(estimatorState(e), estimatorState(plain)); assert.ok(grew);
    assert.deepEqual(events[0], ['published', 10, 1, 1]);
    assert.equal(events[1][1], new InputLead().target(.4, 4, .018));
    const { window, context } = windowFixture(native), original = window.checkpoint;
    const result = window.select({ ...context, authorityTick: 25 }, c => {
      assert.equal(c, original); e.arrive(50, .75); instance.exports.memory.grow(1); c.held = context.held; return true;
    });
    assert.equal(result.checkpoint, original); assert.equal(result.held, context.held); assert.equal(original.authorityTick, 25);
  } finally { native.dispose(); }
  assert.equal(native.handle, 0); assert.throws(() => e.estimate(0), /disposed/);

  let final, dispose = false;
  final = createReconcileBridge(bridge, { trace(event) { if (dispose && event.op === 8) final.dispose(); } });
  const owner = new InputLead({}, { native: final }); dispose = true;
  assert.equal(owner.target(0), new InputLead().target(0)); assert.equal(final.handle, 0);
  assert.throws(() => owner.target(0), /disposed/);

  let throwing, armedThrow = false; const error = new Error('trace-stop');
  throwing = createReconcileBridge(bridge, { trace(event) { if (armedThrow && event.op === 10) throw error; } });
  const smoother = new CorrectionSmoother({ snap: .1 }, { native: throwing }); armedThrow = true;
  assert.throws(() => smoother.note(1, 0, 0, 0, 0, 0), e => e === error);
  assert.deepEqual(smootherState(smoother), { snap: .1, tau: .03, x: 1, y: 0, z: 0, count: 1, snaps: 0, worst: 1 });
  throwing.dispose();
  let warm = 0;
  for (let i = 0; i < 48; i++) {
    const current = createReconcileBridge(bridge), l = new InputLead({}, { native: current });
    l.arrive(i, i / 60); l.target(i / 60); const h = current.handle; assert.ok(h > 0);
    current.dispose(); current.dispose(); assert.equal(current.handle, 0);
    if (i === 4) warm = instance.exports.memory.buffer.byteLength;
  }
  assert.ok(instance.exports.memory.buffer.byteLength <= warm + 2 * 65536);
});

test('observable and shared storage uses original operations without repeated prerequisites', async () => {
  const instance = await loadSim(), trace = [], native = createReconcileSim(instance, { trace: e => trace.push(e.op) });
  const run = (native, ring) => {
    const e = new WorldTickEstimator({ window: 3 }, { native }), events = [];
    e.offsets = ring;
    Object.defineProperty(e, 'hz', { get() { events.push('hz'); return 60; } });
    e.arrive(10, .1); return { state: { at: e.at, count: e.count, offset: e.offset, dropped: e.dropped }, events, bytes: ringBytes(ring) };
  };
  try {
    const at = trace.length;
    assert.deepEqual(run(native, new Float64Array(3)), run(null, new Float64Array(3)));
    assert.ok(!trace.slice(at).includes(1));
    const page = instance.exports.memory.grow(1), rings = [new Float64Array(new SharedArrayBuffer(24)), new Float64Array(instance.exports.memory.buffer, page * 65536, 3)];
    for (const ring of rings) {
      const e = new WorldTickEstimator({ window: 3 }, { native }), plain = new WorldTickEstimator({ window: 3 }); e.offsets = ring;
      const start = trace.length; e.arrive(10, .1); plain.arrive(10, .1);
      assert.deepEqual(estimatorState(e), estimatorState(plain)); assert.ok(!trace.slice(start).includes(1));
    }
    const events = [], commands = [{ generation: 1, epoch: 1, tick: 11 }];
    Object.defineProperty(commands, 'constructor', { get() { events.push('constructor'); return Array; } });
    const start = trace.length, kept = retainPredictionCommands(commands, 12, 1, 1, 3, { native });
    assert.equal(kept[0], commands[0]); assert.deepEqual(events, ['constructor']); assert.ok(!trace.slice(start).includes(15));
    const lead = new InputLead({}, { native }); let reads = 0;
    Object.defineProperty(lead, 'margin', { get() { reads++; native.dispose(); return 3; } });
    const result = attempt(() => lead.target(0));
    // A guarded phase falls back before invoking the getter. Disposal during
    // that original expression does not restart it or repeat its read.
    assert.ifError(result.error); assert.equal(result.value, 3.5); assert.equal(reads, 1); assert.equal(native.handle, 0);
  } finally { native.dispose(); }
});
