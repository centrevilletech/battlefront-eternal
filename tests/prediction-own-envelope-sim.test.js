import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { createCorrectionSim, createCorrectionBridge } from '../src/runtime/correction-sim.js';
import { createWireBridge } from '../src/runtime/wire-sim.js';
import { decodeOwnPlayer, applyOwnPlayer } from '../src/runtime/network-entity-player.js';
import { decodeMountedControl } from '../src/runtime/network-mounted-control.js';
import { correctionObservation, framedCorrection } from './fixtures/prediction-correction-cases.js';
import { ownEnvelopeKinds, ownEnvelopeCases, createOwnEnvelopeFixture, ownEnvelopeLayout,
  observeOwnEnvelope, runOwnEnvelopeCase, patchOwnField, replaceOwnRange } from './fixtures/prediction-own-envelope-cases.js';

const cloneRecord = record => ({ ...record, payload: record.payload.slice() });
const errors = caught => ({ name: caught.name, message: caught.message });
function attempt(record, source, native) {
  const before = native?.calls ?? 0; let decoded = null, error = null;
  try { decoded = decodeOwnPlayer(record, native); } catch (caught) { error = errors(caught); }
  return { error, value: observeOwnEnvelope(decoded, source), calls: (native?.calls ?? 0) - before };
}
function compare(a, b, name) { assert.deepEqual(a.error, b.error, `${name} error`); assert.deepEqual(a.value, b.value, `${name} value, order and aliases`); }

test('native own envelopes match every view flag, structural boundary, scalar rule and nested error precedence', async () => {
  const native = createCorrectionSim(await loadSim()); let accepted = 0, rejected = 0;
  try {
    for (const scenario of ownEnvelopeCases()) {
      const a = runOwnEnvelopeCase(scenario, native), b = runOwnEnvelopeCase(scenario);
      compare(a, b, scenario.label);
      if (scenario.label.endsWith(':valid')) assert.ok(a.calls > 0, `${scenario.label} native delegation`);
      if (a.error) rejected++; else accepted++;
    }
    assert.ok(native.calls > 5000 && accepted > 1000 && rejected > 3000);
  } finally { native.dispose(); }
});

test('decode keeps inner payloads opaque and accepts envelopes larger than the writer budget', async () => {
  const native = createCorrectionSim(await loadSim());
  try {
    for (const kind of ['infantry', 'recon']) {
      const fixture = createOwnEnvelopeFixture(kind), base = fixture.record;
      for (const name of ['body', 'weapon0', ...(kind === 'recon' ? ['remote'] : [])]) {
        for (const bytes of [new Uint8Array(), new Uint8Array([255, 0, 128]), new Uint8Array(65535).fill(255)]) {
          const record = replaceOwnRange(cloneRecord(base), name, bytes), before = native.calls;
          const actual = decodeOwnPlayer(record, native), expected = decodeOwnPlayer(record);
          assert.deepEqual(observeOwnEnvelope(actual, record.payload), observeOwnEnvelope(expected, record.payload));
          assert.ok(native.calls > before);
          if (bytes.length === 65535) assert.ok(record.payload.length > 1100);
        }
      }
    }
    const fixture = createOwnEnvelopeFixture(), record = replaceOwnRange(cloneRecord(fixture.record), 'body', new Uint8Array([255]));
    assert.ok(decodeOwnPlayer(record, native));
    const before = correctionObservation(fixture.unit.soldier);
    assert.throws(() => applyOwnPlayer({ ...fixture, record, native }), /compact Soldier/);
    assert.deepEqual(correctionObservation(fixture.unit.soldier), before, 'inner validation remains in apply');
  } finally { native.dispose(); }
});

test('all returned payload spans preserve Buffer aliases, Uint8Array copies and source offsets', async () => {
  const native = createCorrectionSim(await loadSim());
  try {
    for (const kind of ownEnvelopeKinds) for (const buffer of [false, true]) {
      const base = createOwnEnvelopeFixture(kind).record, record = { ...base, payload: framedCorrection(base.payload, 7, buffer) };
      const before = native.calls, actual = decodeOwnPlayer(record, native), expected = decodeOwnPlayer(record);
      assert.deepEqual(observeOwnEnvelope(actual, record.payload), observeOwnEnvelope(expected, record.payload)); assert.ok(native.calls > before);
      const spans = [actual.body, actual.heldBits, ...actual.weapons.map(w => w.payload), ...(actual.mounted ? [actual.mounted] : []), ...(actual.remote ? [actual.remote.payload] : [])];
      if (actual.mounted) spans.push(...decodeMountedControl(actual.mounted, native).weapons.map(w => w.payload));
      for (const span of spans) { assert.equal(Buffer.isBuffer(span), buffer); assert.equal(span.buffer === record.payload.buffer, buffer); }
      const saved = spans.map(span => Array.from(span)); record.payload.fill(238);
      for (const [i, span] of spans.entries()) assert.deepEqual(Array.from(span), buffer ? saved[i].map(() => 238) : saved[i]);
    }
    const base = createOwnEnvelopeFixture('passenger').record, record = { ...base, payload: Buffer.from(base.payload) };
    const expected = decodeOwnPlayer(record), before = native.calls;
    Object.defineProperty(record.payload, 'constructor', { get() { throw Error('Buffer does not use typed array species'); } });
    assert.deepEqual(decodeOwnPlayer(record, native), expected); assert.ok(native.calls > before);
  } finally { native.dispose(); }
});

test('record and byte getters retain original short-circuiting, repeated reads and primitive exceptions', async () => {
  const native = createCorrectionSim(await loadSim());
  const run = (shape, backend) => {
    const record = cloneRecord(createOwnEnvelopeFixture().record), source = record.payload, layout = ownEnvelopeLayout(record), events = [];
    const marker = new Error('observable envelope'); let input = record;
    if (shape === 'payload-own' || shape === 'payload-inherited' || shape === 'payload-final-length') {
      const changed = source.slice(); patchOwnField({ payload: changed }, layout, 'health', 123.5); let reads = 0;
      const property = { configurable: true, get() {
        events.push(['payload', ++reads]);
        if (reads === 1) return source;
        if (reads === 2 || shape !== 'payload-final-length') return changed;
        return new Uint8Array(changed.length + 1);
      } };
      if (shape === 'payload-inherited') { delete record.payload; Object.setPrototypeOf(record, Object.create(Object.prototype, { payload: property })); }
      else Object.defineProperty(record, 'payload', property);
    } else if (shape === 'kind-short-circuit') {
      Object.defineProperties(record, { kind: { get() { events.push('kind'); return 'delta'; } }, schema: { get() { throw marker; } }, payload: { get() { throw marker; } } });
    } else if (shape === 'schema-short-circuit') {
      Object.defineProperties(record, { schema: { get() { events.push('schema'); return 0; } }, payload: { get() { throw marker; } } });
    } else if (shape === 'payload-throws') Object.defineProperty(record, 'payload', { get() { events.push('payload'); throw marker; } });
    else if (typeof shape === 'string' && shape.startsWith('byte-')) {
      const key = shape.slice(5), value = source[key];
      Object.defineProperty(source, key, { get() { events.push(key); if (key === 'buffer') { patchOwnField({ payload: new Uint8Array(value) }, layout, 'health', 234.5); } return value; } });
    } else if (shape === 'unreached-payload') {
      record.kind = 'delta'; Object.defineProperty(record, 'payload', { get() { throw marker; } });
    } else input = shape;
    const actual = attempt(input, source, backend); return { ...actual, events };
  };
  try {
    const shapes = ['payload-own', 'payload-inherited', 'payload-final-length', 'kind-short-circuit', 'schema-short-circuit', 'payload-throws', 'unreached-payload',
      'byte-buffer', 'byte-byteOffset', 'byte-byteLength', 'byte-length', null, undefined, 1, true, 'record', Symbol('record'), 1n];
    for (const shape of shapes) {
      const a = run(shape, native), b = run(shape, null); compare(a, b, String(shape)); assert.deepEqual(a.events, b.events, String(shape));
      if (typeof shape === 'string' && shape !== 'unreached-payload' && shape !== 'record') assert.equal(a.calls, 0, `${shape} exact fallback`);
    }
  } finally { native.dispose(); }
});

test('custom slice and species callbacks can mutate unread envelope bytes or throw at the original stage', async () => {
  const native = createCorrectionSim(await loadSim());
  const run = (shape, backend) => {
    const record = cloneRecord(createOwnEnvelopeFixture('passenger').record), bytes = record.payload, layout = ownEnvelopeLayout(record), events = [], marker = new Error('slice stopped');
    const original = bytes.slice; let calls = 0;
    const mutate = (start, end) => {
      events.push(['slice', start, end]); calls++;
      if (shape === 'throw' && calls === 2) throw marker;
      if (shape === 'health' && calls === 1) patchOwnField(record, layout, 'health', 456.25);
      if (shape === 'weapon-length' && start === layout.ranges.weapon0.offset) patchOwnField(record, layout, 'weapon1.length', 65535);
      if (shape === 'held-flags' && start === layout.ranges.heldBits.offset) patchOwnField(record, layout, 'viewFlags', 65535);
      if (shape === 'mounted' && start === layout.ranges.mounted.offset) bytes[start] = 255;
    };
    if (shape.startsWith('species')) {
      Object.defineProperty(bytes, 'constructor', { get() { events.push('constructor'); return { get [Symbol.species]() {
        events.push('species'); if (shape === 'species-throw') throw marker;
        patchOwnField(record, layout, 'health', 789.5); return Uint8Array;
      } }; } });
    } else Object.defineProperty(bytes, 'slice', { get() { events.push('slice getter'); return function (start, end) {
      mutate(start, end); if (shape === 'unusual-body' && calls === 1) return { opaque: 'body' }; return original.call(this, start, end);
    }; } });
    const actual = attempt(record, bytes, backend); return { ...actual, events };
  };
  try {
    for (const shape of ['health', 'weapon-length', 'held-flags', 'mounted', 'throw', 'species-mutate', 'species-throw', 'unusual-body']) {
      const a = run(shape, native), b = run(shape, null); compare(a, b, shape); assert.deepEqual(a.events, b.events, shape); assert.equal(a.calls, 0);
    }
  } finally { native.dispose(); }
});

test('apply observes aliasing payload mutations made by existing prerequisite callbacks', async () => {
  const traces = [], native = createCorrectionSim(await loadSim(), { trace: e => traces.push(e.op) });
  const state = unit => ({ body: correctionObservation(unit.soldier), health: unit.combat.health, kick: { ...unit.combat.kick }, weapons: unit.combat.arsenal.weapons.map(correctionObservation) });
  const run = (backend, buffer, mutation) => {
    const source = createOwnEnvelopeFixture(), target = createOwnEnvelopeFixture('infantry', { seed: 13 });
    const record = { ...source.record, payload: framedCorrection(source.record.payload, 5, buffer) }, layout = ownEnvelopeLayout(record), events = [];
    const classes = target.registry.classes, get = classes.get;
    classes.get = function (id) {
      events.push(['class', id]);
      if (mutation === 'body') new DataView(record.payload.buffer, record.payload.byteOffset).setFloat64(layout.ranges.body.offset + 4, NaN, true);
      else record.payload[layout.ranges.weapon0.offset + 1] = 255;
      return get.call(this, id);
    };
    const before = state(target.unit); let error = null; const calls = backend?.calls ?? 0;
    try { applyOwnPlayer({ ...target, record, native: backend }); } catch (caught) { error = errors(caught); }
    const after = state(target.unit);
    if (buffer) { assert.ok(error, 'Buffer mutations reach later restore validation'); assert.deepEqual(after, before); }
    else { assert.ifError(error); assert.notDeepEqual(after, before); }
    return { error, after, events, calls: (backend?.calls ?? 0) - calls };
  };
  try {
    for (const buffer of [false, true]) for (const mutation of ['body', 'weapon']) {
      const at = traces.length, a = run(native, buffer, mutation), b = run(null, buffer, mutation);
      for (const key of ['error', 'after', 'events']) assert.deepEqual(a[key], b[key], `${buffer} ${mutation} ${key}`);
      assert.ok(a.calls > 0); assert.ok(traces.slice(at).includes(8), 'apply delegates the envelope itself');
    }
  } finally { native.dispose(); }
});

test('owned envelope results survive same-instance reentry, trace mutation, growth, disposal and reuse', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance), wire = createWireBridge(bridge);
  const record = cloneRecord(createOwnEnvelopeFixture('recon').record), expected = observeOwnEnvelope(decodeOwnPlayer(record), record.payload);
  let nested = false, grew = false, calls = 0;
  const native = createCorrectionBridge(bridge, { trace(event) {
    event.input.fill(-999); event.output.fill(123); calls++;
    if (!nested) { nested = true; try {
      assert.deepEqual(observeOwnEnvelope(decodeOwnPlayer(record, native), record.payload), expected);
      wire.encodeWordDelta(new Uint8Array([1, 3, 5, 7]));
      if (!grew) { instance.exports.memory.grow(1); grew = true; }
    } finally { nested = false; } }
  } });
  try {
    assert.deepEqual(observeOwnEnvelope(decodeOwnPlayer(record, native), record.payload), expected); assert.ok(grew && calls === 2);
    const invalid = { ...record, payload: record.payload.subarray(0, 12) };
    assert.throws(() => decodeOwnPlayer(invalid, native), /envelope exceeds budget/);
    assert.deepEqual(observeOwnEnvelope(decodeOwnPlayer(record, native), record.payload), expected);
  } finally { native.dispose(); }
  let disposed;
  disposed = createCorrectionBridge(bridge, { trace() { disposed.dispose(); } });
  assert.deepEqual(observeOwnEnvelope(decodeOwnPlayer(record, disposed), record.payload), expected);
  assert.equal(disposed.handle, 0); assert.throws(() => decodeOwnPlayer(record, disposed), /disposed/);
  const marker = new Error('diagnostic stopped'), throwing = createCorrectionBridge(bridge, { trace() { throw marker; } });
  assert.throws(() => decodeOwnPlayer(record, throwing), e => e === marker); throwing.dispose();
  let warm = 0;
  for (let i = 0; i < 44; i++) {
    const decoder = createCorrectionBridge(bridge); assert.deepEqual(observeOwnEnvelope(decodeOwnPlayer(record, decoder), record.payload), expected);
    decoder.dispose(); decoder.dispose(); assert.equal(decoder.handle, 0);
    if (i === 3) warm = instance.exports.memory.buffer.byteLength;
  }
  assert.ok(instance.exports.memory.buffer.byteLength <= warm + 2 * 65536, 'disposed decoder storage is reused');
});

test('module and shared source storage use the original reader without borrowing native memory', async () => {
  const instance = await loadSim(), native = createCorrectionSim(instance), base = createOwnEnvelopeFixture('passenger').record;
  try {
    const page = instance.exports.memory.grow(1), moduleBytes = new Uint8Array(instance.exports.memory.buffer, page * 65536 + 13, base.payload.length);
    const shared = new Uint8Array(new SharedArrayBuffer(base.payload.length + 7), 7);
    for (const payload of [moduleBytes, shared]) {
      payload.set(base.payload); const record = { ...base, payload }, before = native.calls;
      const actual = decodeOwnPlayer(record, native), expected = decodeOwnPlayer(record);
      assert.deepEqual(observeOwnEnvelope(actual, payload), observeOwnEnvelope(expected, payload)); assert.equal(native.calls, before);
      payload.fill(255); assert.deepEqual(actual, expected);
    }
  } finally { native.dispose(); }
});
