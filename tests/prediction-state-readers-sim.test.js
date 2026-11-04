import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { createCorrectionSim, createCorrectionBridge } from '../src/runtime/correction-sim.js';
import { createWireBridge } from '../src/runtime/wire-sim.js';
import { validateArsenalWeapon } from '../src/runtime/network-arsenal-state.js';
import { decodeRemoteCorrection } from '../src/runtime/network-remote-codec.js';
import { stateReaderCases, stateReaderCallbackCases, reconReaderCallbackCases, runStateReaderCase, arsenalReaderCase,
  createReaderWeapon, deltaBytes, createReaderRemote, captureReaderRemote, observeStateReader, readerWeaponSnapshot } from './fixtures/prediction-state-reader-cases.js';

const object = entries => Object.fromEntries(entries);
const callback = name => stateReaderCallbackCases().find(c => c.name === name);
function twin(scenario, native) {
  const actual = runStateReaderCase(scenario, native), expected = runStateReaderCase(scenario);
  assert.deepEqual(actual.result, expected.result, scenario.name); return actual;
}

test('native state-reader phases match full delta, melee, recon and malformed-byte matrix', async () => {
  const counts = Array(19).fill(0), native = createCorrectionSim(await loadSim(), { trace: e => counts[e.op]++ });
  const callbacks = new Set(stateReaderCallbackCases().map(c => c.name)); let accepted = 0, rejected = 0;
  try {
    for (const scenario of stateReaderCases().filter(c => !callbacks.has(c.name))) {
      const actual = twin(scenario, native);
      if (scenario.name.endsWith(':valid')) assert.ok(actual.calls > 0, `${scenario.name} native delegation`);
      if (actual.result.error) rejected++; else accepted++;
    }
    assert.ok(accepted > 300 && rejected > 1500 && native.calls > 2500); assert.ok(counts.slice(9).every(n => n > 0));
  } finally { native.dispose(); }
});

test('arsenal phases preserve cold and shared caches, copied values, melee limits and partial writes', async () => {
  const traces = [], native = createCorrectionSim(await loadSim(), { trace: e => traces.push(e) });
  try {
    for (const scenario of stateReaderCallbackCases().filter(c => c.name.startsWith('arsenal:'))) {
      const start = traces.length, actual = twin(scenario, native), result = actual.result;
      if (scenario.name === 'arsenal:saved-mask-after-cold-constructor') {
        const state = object(result.result.state); assert.equal(state.level, 1); assert.equal(state.heat, .75); assert.equal(state.reserve, 3);
      }
      if (scenario.name === 'arsenal:melee-current-three-incoming-one') {
        assert.equal(result.after.definition, 'level1'); assert.equal(result.after.melee.index, 2); assert.equal(result.after.melee.active, false); assert.equal(result.after.melee.swing, undefined);
      }
      if (scenario.name === 'arsenal:melee-invalid-time-skips-definition') assert.deepEqual(result.events, []);
      if (scenario.name === 'arsenal:warm-shared-definition-cache') {
        assert.equal(result.first.state.reserve, 3); assert.equal(result.shared.state.reserve, 3); assert.equal(result.changed.state.reserve, 9); assert.deepEqual(result.events, []);
      }
      if (scenario.name === 'arsenal:retained-baseline-slice-partial-copy') {
        assert.equal(result.outcome.error.message, 'truncated arsenal weapon state'); assert.equal(result.partial.length, 162);
        assert.deepEqual(result.partial.slice(0, 2), [1, 65]); assert.equal(new DataView(Uint8Array.from(result.partial).buffer).getFloat64(2, true), .25);
        assert.ok(traces.slice(start).some(e => e.op === 10 && e.error && e.output.length === 163));
      }
      if (scenario.name === 'arsenal:overlapping-baseline-slice-sequential-copy') {
        assert.equal(result.shared, true); assert.equal(result.outcome.state.clip, 0); assert.equal(result.outcome.state.reserve, 0);
        assert.ok(!traces.slice(start).some(e => e.op === 10), 'overlapping expansion resumes in the original loop');
      }
    }
  } finally { native.dispose(); }
});

test('recon phases retain live prerequisite order, exact definition references and original DataView semantics', async () => {
  const native = createCorrectionSim(await loadSim());
  try {
    for (const scenario of reconReaderCallbackCases().filter(c => !c.name.includes('camera-hypot'))) {
      const actual = twin(scenario, native), result = actual.result;
      if (scenario.name.includes('weapon-before-length')) assert.deepEqual(result.events, ['weapon0']);
      if (scenario.name === 'recon:invalid-count-skips-arsenal' || scenario.name === 'recon:invalid-wait-skips-salvo-count') assert.deepEqual(result.events, []);
      if (scenario.name === 'recon:nested-payload-copy-false') assert.ifError(result.error);
      if (scenario.name === 'recon:nested-payload-copy-true') assert.equal(result.error.message, 'invalid weapon state payload');
      if (scenario.name === 'recon:retained-salvo-definition-and-count-order') {
        const salvo = object(object(result.result).salvo);
        assert.deepEqual(salvo.def, { identity: 'weapon1.level0' }); assert.equal(salvo.left, 2); assert.equal(salvo.wait, .875); assert.equal(object(salvo.at).x, 123);
        assert.deepEqual(result.events, ['strike', 'old SalvoCount']);
      }
      if (scenario.name === 'recon:resizable-source-retains-original-view-bounds') {
        assert.equal(result.error.name, 'RangeError'); assert.ok(actual.calls > 0, 'earlier phases used native before the source resized');
      }
      if (scenario.name === 'recon:restore-keeps-melee-and-numeric-level-omissions') {
        const w = result.after.weapons[0]; assert.equal(w.level, 73); assert.equal(w.definition, 'weapon0.level1');
        assert.equal(w.melee.time, .987); assert.equal(w.melee.index, 2); assert.equal(w.melee.active, true);
        assert.equal(Object.hasOwn(object(object(result.result).weapons[0]), 'melee'), false);
      }
    }
  } finally { native.dispose(); }
});

test('camera quaternion acceptance matches V8 hypot at adjacent tolerance values', async () => {
  const traces = [], native = createCorrectionSim(await loadSim(), { trace: e => traces.push(e.op) }); let accepted = 0, rejected = 0;
  try {
    for (const scenario of reconReaderCallbackCases().filter(c => c.name.includes('camera-hypot'))) {
      const at = traces.length, actual = twin(scenario, native);
      assert.ok(traces.slice(at).includes(18), `${scenario.name} native camera math`);
      if (actual.result.error) rejected++; else accepted++;
    }
    assert.ok(accepted >= 10 && rejected >= 10);
  } finally { native.dispose(); }
});

test('owned phases survive nested shared-bridge work, memory growth, diagnostic mutation and disposal', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance), wire = createWireBridge(bridge); let nested = false, grew = false;
  const other = createReaderWeapon(), otherBytes = deltaBytes({ numbers: { heat: .75 } });
  const native = createCorrectionBridge(bridge, { trace(event) { event.input.fill(-999); event.output.fill(123); } });
  try {
    const scenario = arsenalReaderCase('growth-between-phases', { setup(f) {
      Object.defineProperty(f.definition, 'ammo', { get() {
        f.events.push('ammo');
        if (!nested) { nested = true; try {
          const state = validateArsenalWeapon(other.weapon, otherBytes, f.native).state; f.events.push(['nested heat', state.heat]);
          if (f.native) { wire.encodeWordDelta(new Uint8Array([1, 2, 3, 4])); if (!grew) { instance.exports.memory.grow(1); grew = true; } }
        } finally { nested = false; } }
        return 4;
      } });
    } });
    const result = twin(scenario, native); assert.ifError(result.result.error); assert.ok(grew && result.calls > 0);
    twin(callback('arsenal:retained-baseline-slice-partial-copy'), native);
    const remote = createReaderRemote(), bytes = captureReaderRemote(remote);
    const actual = decodeRemoteCorrection(bytes, remote.droid, native), expected = decodeRemoteCorrection(bytes, remote.droid);
    assert.deepEqual(observeStateReader(actual, remote.ids), observeStateReader(expected, remote.ids));
  } finally { native.dispose(); }
  const target = createReaderWeapon(), before = readerWeaponSnapshot(target); let decoder;
  Object.defineProperty(target.definition, 'ammo', { get() { decoder.dispose(); return 4; } });
  decoder = createCorrectionBridge(bridge);
  assert.throws(() => validateArsenalWeapon(target.weapon, deltaBytes(), decoder), /disposed/); assert.equal(decoder.handle, 0);
  assert.deepEqual(readerWeaponSnapshot(target), before);
  let final;
  final = createCorrectionBridge(bridge, { trace(e) { if (e.op === 12) final.dispose(); } });
  const fresh = createReaderWeapon(), result = validateArsenalWeapon(fresh.weapon, deltaBytes(), final);
  assert.equal(result.weapon, fresh.weapon); assert.equal(final.handle, 0);
  assert.throws(() => validateArsenalWeapon(fresh.weapon, deltaBytes(), final), /disposed/);
  let warm = 0;
  for (let i = 0; i < 44; i++) {
    const session = createCorrectionBridge(bridge), f = createReaderWeapon();
    assert.equal(validateArsenalWeapon(f.weapon, deltaBytes(), session).state.reserve, 3); session.dispose(); session.dispose();
    if (i === 3) warm = instance.exports.memory.buffer.byteLength;
  }
  assert.ok(instance.exports.memory.buffer.byteLength <= warm + 2 * 65536);
});

test('unusual byte storage and methods preserve source access and fall back at the affected phase', async () => {
  const traces = [], instance = await loadSim(), native = createCorrectionSim(instance, { trace: e => traces.push(e.op) });
  const forge = (Ctor, bytes) => {
    const value = Ctor === Float64Array ? new Float64Array(6) : new Ctor(bytes.slice().buffer);
    Object.setPrototypeOf(value, Uint8Array.prototype); Object.defineProperty(value, Symbol.toStringTag, { get() { throw Error('unobserved tag getter'); } }); return value;
  };
  try {
    for (const Ctor of [Int8Array, Uint8ClampedArray, Float64Array]) {
      const source = deltaBytes({ flags: 255 }), scenario = arsenalReaderCase(`forged-${Ctor.name}`, { payload: () => forge(Ctor, source) });
      const at = traces.length; twin(scenario, native); assert.ok(!traces.slice(at).some(op => op >= 9 && op <= 12));
    }
    const normal = arsenalReaderCase('ordinary-custom-tag', { setup(f) { Object.defineProperty(f.bytes, Symbol.toStringTag, { get() { throw Error('unobserved tag getter'); } }); } });
    const at = traces.length; const result = twin(normal, native); assert.ifError(result.result.error); assert.ok(traces.slice(at).includes(9));
    const remote = createReaderRemote(), source = captureReaderRemote(remote);
    for (const Ctor of [Int8Array, Float64Array]) {
      const a = createReaderRemote(), b = createReaderRemote(), before = traces.length; let actual, expected;
      try { actual = { value: observeStateReader(decodeRemoteCorrection(forge(Ctor, source), a.droid, native), a.ids) }; } catch (error) { actual = { name: error.name, message: error.message }; }
      try { expected = { value: observeStateReader(decodeRemoteCorrection(forge(Ctor, source), b.droid), b.ids) }; } catch (error) { expected = { name: error.name, message: error.message }; }
      assert.deepEqual(actual, expected); assert.ok(!traces.slice(before).includes(13));
    }
    const page = instance.exports.memory.grow(1), moduleBytes = new Uint8Array(instance.exports.memory.buffer, page * 65536 + 13, source.length);
    const shared = new Uint8Array(new SharedArrayBuffer(source.length));
    for (const bytes of [moduleBytes, shared]) {
      bytes.set(source); const at = traces.length, a = createReaderRemote(), b = createReaderRemote();
      assert.deepEqual(observeStateReader(decodeRemoteCorrection(bytes, a.droid, native), a.ids), observeStateReader(decodeRemoteCorrection(bytes, b.droid), b.ids));
      assert.ok(!traces.slice(at).some(op => op >= 13), 'original source stays with its Fields reader');
    }
  } finally { native.dispose(); }
});
