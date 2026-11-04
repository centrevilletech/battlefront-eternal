import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { createWireBridge } from '../src/runtime/wire-sim.js';
import { createPlayoutSim, createPlayoutBridge } from '../src/runtime/playout-sim.js';
import { PlayoutClock } from '../src/runtime/playout.js';
import { SnapshotInterval, renderedViewTick } from '../src/runtime/snapshot-interval.js';
import { playoutCases, playoutNumericCases, playoutCallbackCases, intervalCallbackCases, playoutRawCases, playoutDescriptorCases,
  runPlayoutCase, playoutState, intervalState, playoutRing, playoutBits, playoutAttempt } from './fixtures/prediction-playout-cases.js';

const F64 = Float64Array;
const named = name => playoutCases().find(c => c.name === name);
function twin(scenario, native) {
  const actual = runPlayoutCase(scenario, native), expected = runPlayoutCase(scenario);
  assert.deepEqual(actual.result, expected.result, scenario.name);
  if (scenario.native) assert.ok(actual.calls > 0, `${scenario.name} delegates`);
  return actual.result;
}

test('native playout and brackets match arrival, startup, snap, trim and wait progression', async () => {
  const counts = Array(13).fill(0), native = createPlayoutSim(await loadSim(), { trace: e => counts[e.op]++ });
  try {
    for (const scenario of playoutNumericCases()) twin(scenario, native);
    assert.ok(counts.every(n => n > 0), `all thirteen phases execute: ${counts}`); assert.ok(native.calls > 5000);
    assert.deepEqual(Object.keys(new PlayoutClock({}, { native })), Object.keys(new PlayoutClock()));
    assert.deepEqual(Object.keys(new SnapshotInterval({ native })), Object.keys(new SnapshotInterval()));
    const c = new PlayoutClock({}, { native });
    c.arrive(10, 0); assert.equal(c.advance(0), 8); c.advance(1); assert.equal(c.clock, 10); assert.equal(c.waits, 1);
    c.delay = 0; c.measure = () => {}; c.clock = 30; c.latest = 5; c.snap = 1;
    assert.equal(c.advance(0), 5, 'the original snap branch can move back to its target');
  } finally { native.dispose(); }
});

test('reset replaces the ring at its original allocation boundary and preserves partial publication', async () => {
  const trace = [], native = createPlayoutSim(await loadSim(), { trace: e => { trace[trace.length] = e.op; } });
  try {
    const c = new PlayoutClock({ window: 3 }, { native }), old = c.gaps;
    new DataView(old.buffer).setBigUint64(0, 0x7ff0000000000001n, true); const bytes = playoutRing(old), at = trace.length;
    c.reset(); assert.notEqual(c.gaps, old); assert.equal(playoutRing(old), bytes); assert.equal(playoutRing(c.gaps), '00'.repeat(24));
    assert.deepEqual(trace.slice(at), [0, 1]);
    for (const scenario of playoutCallbackCases().filter(c => c.name.startsWith('reset:'))) twin(scenario, native);
    const replaced = twin(named('reset:constructor-new'), native);
    assert.equal(replaced.old, false); assert.equal(replaced.allocated, true); assert.equal(replaced.state.clock.number, 88); assert.equal(replaced.state.gapAt.number, 0);
    const failed = twin(named('reset:constructor-throw'), native);
    assert.ok(failed.result.thrown && failed.result.same); assert.equal(failed.old, true);
    assert.equal(failed.events[0][2].clock.number, 0); assert.equal(failed.events[0][2].latest.number, 0);
    assert.equal(failed.events[0][2].gapAt.number, 2); assert.equal(failed.state.gapAt.number, 99); assert.equal(failed.state.gapCount.number, 3);
    const lookup = twin(named('reset:constructor-lookup'), native);
    assert.deepEqual(lookup.events.slice(0, 2), ['constructor-lookup', 'window']);
  } finally { native.dispose(); }
});

test('percentile and advance callbacks retain key publication, later reads and reentry order', async () => {
  const native = createPlayoutSim(await loadSim());
  try {
    for (const scenario of playoutCallbackCases().filter(c => !c.name.startsWith('reset:'))) twin(scenario, native);
    const sorted = twin(named('measure:key-published-before-slice-sort-and-live-index'), native);
    assert.deepEqual(sorted.events, [['slice', 0, 5, 5 * 4096 + 3], 'sort']); assert.equal(sorted.result.number, 10);
    const cached = twin(named('measure:throw-retains-key-and-cached-call-skips-all-work'), native);
    assert.ok(cached.first.thrown && cached.first.same); assert.equal(cached.second.value.number, 7);
    assert.deepEqual(cached.events, [['slice', 8193]]);
    const policy = twin(named('measure:delay-clamp-read-and-coercion-order'), native);
    assert.deepEqual(policy.events, ['maxDelay', 'minDelay', 'margin', 'gap-value']);
    const nested = twin(named('advance:measure-override-reentry-and-late-field-reads'), native);
    assert.equal(nested.result.number, 9); assert.equal(nested.state.clock.number, 9);
    const setters = twin(named('advance:startup-and-return-read-setter-order'), native);
    assert.equal(setters.result.number, 7.75); assert.equal(setters.started, true);
    const resultSort = twin(named('measure:source-sort-does-not-mask-slice-result-sort'), native);
    assert.equal(resultSort.result.number, 8); assert.deepEqual(resultSort.events, [['sort-result', true, 4, 16384]]);
  } finally { native.dispose(); }
});

test('snapshot interval getters, setters and nullable guards preserve selective writes and coercion', async () => {
  const native = createPlayoutSim(await loadSim());
  try {
    for (const scenario of intervalCallbackCases()) twin(scenario, native);
    const alpha = twin(named('interval:double-current-and-previous-reads'), native);
    assert.equal(alpha.alpha.number, 1); assert.deepEqual(alpha.events, [['current', 1], ['previous', 1], 'clock-value', ['current', 2], ['previous', 2]]);
    const needs = twin(named('interval:needs-sample-rereads-current-before-coercion'), native);
    assert.equal(needs.result, true); assert.deepEqual(needs.events, [['current', 1], ['current', 2], 'clock-value']);
    const applied = twin(named('interval:apply-selective-publication-and-throw'), native);
    assert.equal(applied[0].previous.number, 4); assert.equal(applied[0].current.number, 99); assert.equal(applied[0].same, false);
    assert.ok(applied[0].result.same && applied[1].result.same); assert.ok(applied[1].same && applied[2].same);
  } finally { native.dispose(); }
});

test('raw input and output bits, inconsistent rings and descriptor pollution match the original phases', async () => {
  const trace = [], native = createPlayoutSim(await loadSim(), { trace: e => { trace[trace.length] = e.op; } });
  try {
    for (const scenario of playoutRawCases()) twin(scenario, native);
    for (const scenario of playoutDescriptorCases()) twin(scenario, native);
    for (const kind of ['math', 'slice', 'sort-result', 'species']) {
      const result = twin(named(`descriptor:accessor-${kind}-with-inherited-value`), native);
      assert.equal(result.events.filter(e => Array.isArray(e) && e[0].endsWith('-get')).length, 1, kind);
    }
    const geometry = twin(named('descriptor:accessor-geometry-with-inherited-value'), native);
    assert.deepEqual(geometry.events, [], 'the original slice uses internal geometry without reading an own geometry accessor');
    for (const kind of ['get', 'set']) {
      const at = trace.length, result = twin(named(`descriptor:inherited-${kind}`), native);
      assert.ok(!result.events.includes('unexpected-inherited-descriptor-hook')); assert.ok(trace.slice(at).includes(4));
    }
    const c = new PlayoutClock({ window: 4 }, { native }), ring = c.gaps;
    const view = new DataView(ring.buffer); view.setBigUint64(24, 0x7ff0000000000123n, true);
    c.arrive(1, 0); c.arrive(2, .1); assert.equal(view.getBigUint64(24, true), 0x7ff0000000000123n); assert.equal(c.gaps, ring);
  } finally { native.dispose(); }
});

test('owned phases tolerate shared-bridge nested work, memory growth and trace mutation', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance), wire = createWireBridge(bridge); let nested = false, armed = false, grew = false, other;
  const native = createPlayoutBridge(bridge, { trace(e) {
    e.input.fill(-123); e.output.fill(999);
    if (!armed || nested || e.op !== 4) return;
    nested = true;
    try {
      other.arrive(3, .15); other.measure(); wire.encodeWordDelta(new Uint8Array([1, 2, 3, 4]));
      if (!grew) { instance.exports.memory.grow(1); grew = true; }
    } finally { nested = false; }
  } });
  try {
    other = new PlayoutClock({ window: 3 }, { native }); other.arrive(1, .1);
    const actual = new PlayoutClock({ window: 4 }, { native }), expected = new PlayoutClock({ window: 4 });
    actual.gaps.set([1, 5, 2, 3]); expected.gaps.set([1, 5, 2, 3]); actual.gapCount = expected.gapCount = 4; armed = true;
    assert.equal(actual.measure(), expected.measure()); assert.deepEqual(playoutState(actual), playoutState(expected)); assert.ok(grew);
    const a = new SnapshotInterval({ native }), b = new SnapshotInterval(); a.apply(8); b.apply(8);
    assert.equal(a.alpha(7.25), b.alpha(7.25)); assert.deepEqual(intervalState(a), intervalState(b));
  } finally { native.dispose(); wire.dispose(); }
  let warm = 0;
  for (let i = 0; i < 48; i++) {
    const next = createPlayoutBridge(bridge), c = new PlayoutClock({}, { native: next }); c.arrive(10, .1); c.advance(.01);
    assert.ok(next.handle > 0); next.dispose(); next.dispose(); assert.equal(next.handle, 0);
    if (i === 4) warm = instance.exports.memory.buffer.byteLength;
  }
  assert.ok(instance.exports.memory.buffer.byteLength <= warm + 2 * 65536);
});

test('close during original callbacks finishes the current phase chain without another native call', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance);
  const execute = (kind, native) => {
    const clock = new PlayoutClock({ window: 3 }, { native }), events = [], token = Symbol('callback-stop'), old = clock.gaps;
    let callsAtClose;
    const close = () => { events.push('close'); if (native) { native.dispose(); callsAtClose = native.calls; } };
    let result;
    if (kind.startsWith('constructor')) {
      clock.clock = 20; clock.gapAt = 2; const prior = Object.getOwnPropertyDescriptor(globalThis, 'Float64Array');
      Object.defineProperty(globalThis, 'Float64Array', { configurable: true, writable: true, value: function (length) {
        events.push(['allocate', clock.clock, clock.gapAt, clock.gaps === old]); clock.clock = 80; close();
        if (kind === 'constructor-throw') throw token; return new F64(length);
      } });
      try { result = playoutAttempt(() => clock.reset(), token); }
      finally { Object.defineProperty(globalThis, 'Float64Array', prior); }
    } else if (kind === 'slice' || kind === 'sort' || kind === 'advance-slice') {
      clock.gapCount = 2; clock.gaps.set([2, 5, 1]);
      clock.latest = 9;
      clock.gaps.slice = () => { events.push('slice'); if (kind !== 'sort') close(); return { sort() { events.push('sort'); if (kind === 'sort') close(); clock.margin = 3; return [2, 5]; } }; };
      result = playoutAttempt(() => kind === 'advance-slice' ? clock.advance(.1) : clock.measure(), token);
    } else {
      clock.measure = () => { events.push('measure'); clock.latest = 9; clock.delay = 2; close(); if (kind === 'measure-throw') throw token; };
      result = playoutAttempt(() => clock.advance(.1), token);
    }
    return { result: { result, state: playoutState(clock), events, sameRing: clock.gaps === old }, clock, callsAtClose };
  };
  for (const kind of ['constructor', 'constructor-throw', 'slice', 'sort', 'advance-slice', 'measure', 'measure-throw']) {
    const native = createPlayoutBridge(bridge), expected = execute(kind, null), actual = execute(kind, native);
    assert.deepEqual(actual.result, expected.result, kind); assert.equal(native.handle, 0); assert.equal(native.calls, actual.callsAtClose);
    assert.throws(() => actual.clock.reset(), /disposed/); assert.equal(native.calls, actual.callsAtClose);
    const second = createPlayoutBridge(bridge), fresh = new PlayoutClock({}, { native: second }); fresh.arrive(5, 0); assert.equal(fresh.advance(0), 3); second.dispose();
  }
});

test('trace close or arbitrary trace throw keeps published values owned and preserves the original throw', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance);
  for (const op of [0, 3, 6]) for (const throwing of [false, true]) {
    let native, armed = false, callsAtClose, clock;
    const token = throwing ? undefined : Symbol('unused');
    native = createPlayoutBridge(bridge, { trace(e) {
      if (!armed || e.op !== op) return;
      native.dispose(); callsAtClose = native.calls;
      if (op === 6) clock.clock = 99;
      if (throwing) throw token;
    } });
    clock = new PlayoutClock({ window: 3 }, { native }); clock.gapCount = 2; clock.gaps.set([2, 4, 1]); clock.latest = 10; armed = true;
    const result = playoutAttempt(() => op === 0 ? clock.reset() : op === 3 ? clock.measure() : clock.advance(.01), token);
    assert.equal(result.thrown, throwing); if (throwing) assert.equal(result.same, true);
    assert.equal(native.handle, 0); assert.equal(native.calls, callsAtClose);
    if (!throwing && op === 6) assert.equal(result.value.number, 99, 'advance performs its final field read after publication and tracing');
    assert.throws(() => clock.measure(), /disposed/); native.dispose();
  }
});
