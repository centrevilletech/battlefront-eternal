import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { createWireSemanticBridge } from '../src/runtime/wire-semantic.js';
import { captureCompactFlight } from '../src/runtime/network-entity-flight-compact.js';
import { correctionDescriptors } from './fixtures/prediction-correction-cases.js';

const fromBits = bits => { const view = new DataView(new ArrayBuffer(8)); view.setBigUint64(0, bits, true); return view.getFloat64(0, true); };
const floorBits = bytes => new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getBigUint64(6 + 13 * 8, true);
const descriptors = () => correctionDescriptors().filter(d => d.name.startsWith('flight-compact-'));
async function countedEncoder() {
  const instance = await loadSim(), original = bridgeFor(instance), bridge = Object.create(original); let calls = 0;
  bridge.x = { ...original.x, wire_semantic_run(...args) { calls++; return original.x.wire_semantic_run(...args); } };
  return { instance, native: createWireSemanticBridge(bridge), calls: () => calls };
}

test('native flight capture preserves finite, signed zero and quiet/signaling NaN payload bits in every layout', async () => {
  const { native, calls } = await countedEncoder(); let captures = 0;
  try {
    for (const descriptor of descriptors()) for (const bits of [
      0n, 0x8000000000000000n, 1n, 0x400921fb54442d18n,
      0x7ff8000000000001n, 0x7ff8123456789abcn, 0xfff8000000001234n,
      0x7ff0000000000001n, 0x7ff0123456789abcn, 0xfff0000000001234n,
    ]) {
      const body = descriptor.create(2); body.floorY = fromBits(bits);
      const expected = captureCompactFlight(body), actual = captureCompactFlight(body, native); captures++;
      assert.deepEqual(actual, expected, `${descriptor.name} ${bits.toString(16)}`);
      assert.equal(floorBits(actual), bits);
    }
    assert.equal(calls(), captures, 'every valid capture enters the native writer');
    for (const bad of [Infinity, -Infinity, null, undefined, '0']) {
      const body = descriptors()[0].create(2); body.floorY = bad;
      assert.throws(() => captureCompactFlight(body), /nonfinite flight correction/);
      assert.throws(() => captureCompactFlight(body, native), /nonfinite flight correction/);
    }
  } finally { native.dispose(); }
});

test('sampled flight bits survive later source mutation, nested capture and shared memory growth', async () => {
  const { instance, native, calls } = await countedEncoder(), descriptor = descriptors().find(d => d.name === 'flight-compact-Flying-none');
  const originalBits = 0x7ff0000000000001n, changedBits = 0xfff8000000001234n;
  try {
    const capture = backend => {
      const body = descriptor.create(2), nested = descriptor.create(7), events = [];
      body.floorY = fromBits(originalBits); nested.floorY = fromBits(changedBits);
      const bank = body.bank;
      Object.defineProperty(body, 'bank', { get() {
        events.push('bank'); body.floorY = fromBits(changedBits);
        assert.equal(floorBits(captureCompactFlight(nested, native)), changedBits);
        instance.exports.memory.grow(1);
        return bank;
      } });
      return { bytes: captureCompactFlight(body, backend), events };
    };
    const before = calls(), expected = capture(null), actual = capture(native);
    assert.deepEqual(actual, expected); assert.deepEqual(actual.events, ['bank']);
    assert.equal(floorBits(actual.bytes), originalBits);
    assert.equal(calls() - before, 3, 'both nested captures and the outer native writer ran');
  } finally { native.dispose(); }
});
