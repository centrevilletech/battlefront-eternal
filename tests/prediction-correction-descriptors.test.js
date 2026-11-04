import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { createCorrectionSim } from '../src/runtime/correction-sim.js';
import { decodeOwnPlayer } from '../src/runtime/network-entity-player.js';
import { decodeWeaponState } from '../src/runtime/network-weapon-state.js';
import { decodeMountedControl } from '../src/runtime/network-mounted-control.js';
import { restoreGroundCorrection } from '../src/runtime/network-entity-ground.js';
import { correctionDescriptors, correctionObservation } from './fixtures/prediction-correction-cases.js';

const byName = name => correctionDescriptors().find(value => value.name === name);
function caught(run) {
  try { return { value: run(), thrown: false }; }
  catch (value) { return { value, thrown: true }; }
}
function polluted(key, descriptor, run) {
  const saved = Object.getOwnPropertyDescriptor(Object.prototype, key);
  Object.defineProperty(Object.prototype, key, { __proto__: null, configurable: true, ...descriptor });
  try { return run(); }
  finally {
    delete Object.prototype[key];
    if (saved) Object.defineProperty(Object.prototype, key, saved);
  }
}

test('correction prerequisite accessors retain their errors under inherited descriptor values', async () => {
  const native = createCorrectionSim(await loadSim());
  try {
    for (const kind of ['own-kind', 'ground-flying']) {
      const run = backend => {
        const marker = { reason: kind }, events = [], before = backend?.calls ?? 0;
        const target = kind === 'own-kind' ? {} : byName('ground').create(13);
        Object.defineProperty(target, kind === 'own-kind' ? 'kind' : 'flying', {
          configurable: true, get() { events.push(kind); throw marker; },
        });
        const result = polluted('value', { value: false, writable: true }, () => caught(() => kind === 'own-kind'
          ? decodeOwnPlayer(target, backend) : restoreGroundCorrection(target, null, backend)));
        return { events, thrown: result.thrown, original: result.value === marker, calls: (backend?.calls ?? 0) - before };
      };
      assert.deepEqual(run(native), run(null), kind);
    }
  } finally { native.dispose(); }
});

test('typed geometry and species data descriptors reject before inherited getter fields are read', async () => {
  const native = createCorrectionSim(await loadSim());
  try {
    for (const kind of ['length', 'species']) {
      const run = backend => {
        const descriptor = byName(kind === 'length' ? 'weapon' : 'mounted-2');
        const bytes = descriptor.capture(descriptor.create(2)), events = [], marker = { reason: kind };
        const previousSpecies = Object.getOwnPropertyDescriptor(Uint8Array, Symbol.species);
        if (kind === 'length') Object.defineProperty(bytes, 'length', { value: bytes.length, configurable: true });
        else Object.defineProperty(Uint8Array, Symbol.species, { value: Uint8Array, configurable: true });
        const before = backend?.calls ?? 0;
        let result;
        try {
          result = polluted('get', { get() { events.push('inherited-get'); throw marker; } }, () => caught(() =>
            kind === 'length' ? decodeWeaponState(bytes, backend) : decodeMountedControl(bytes, backend)));
        } finally {
          if (kind === 'species') {
            delete Uint8Array[Symbol.species];
            if (previousSpecies) Object.defineProperty(Uint8Array, Symbol.species, previousSpecies);
          }
        }
        return { result, events, calls: (backend?.calls ?? 0) - before };
      };
      assert.deepEqual(run(native), run(null), kind);
    }
  } finally { native.dispose(); }
});

test('correction slice accessors keep source aliases without consulting inherited descriptor values', async () => {
  const native = createCorrectionSim(await loadSim()), descriptor = byName('mounted-2');
  try {
    for (const buffer of [false, true]) {
      const run = backend => {
        const captured = descriptor.capture(descriptor.create(2)), bytes = buffer ? Buffer.from(captured) : captured;
        const method = bytes.slice, events = [], before = backend?.calls ?? 0;
        Object.defineProperty(bytes, 'slice', { configurable: true, get() { events.push('slice'); return method; } });
        const result = polluted('value', { get() { events.push('inherited-value'); throw Error('descriptor value read'); } },
          () => caught(() => decodeMountedControl(bytes, backend)));
        if (result.thrown) return { result, events, calls: (backend?.calls ?? 0) - before };
        const payload = result.value.weapons[0].payload, copied = Array.from(payload); bytes.fill(255);
        return { events, copied, after: Array.from(payload), buffer: Buffer.isBuffer(payload), calls: (backend?.calls ?? 0) - before };
      };
      assert.deepEqual(run(native), run(null), String(buffer));
    }
  } finally { native.dispose(); }
});

test('recon view data geometry rejects before inherited accessor fields are read', async () => {
  const native = createCorrectionSim(await loadSim());
  try {
    const bytes = new Uint8Array(24), view = new DataView(bytes.buffer), events = [];
    Object.defineProperty(view, 'buffer', { value: bytes.buffer, configurable: true });
    const before = native.calls;
    const result = polluted('get', { get() { events.push('inherited-get'); throw Error('descriptor getter read'); } },
      () => caught(() => native.readReconVector({ bytes, view, at: 0 }, 3)));
    assert.deepEqual(result, { value: null, thrown: false });
    assert.deepEqual(events, []); assert.equal(native.calls, before);
  } finally { native.dispose(); }
});

test('ordinary corrections remain native when Object.prototype carries a value field', async () => {
  const native = createCorrectionSim(await loadSim()), descriptor = byName('ground');
  try {
    const bytes = descriptor.capture(descriptor.create(2)), target = descriptor.create(13), expected = descriptor.create(13);
    const position = target.position, before = native.calls;
    const result = polluted('value', { value: 17, writable: true }, () => caught(() => restoreGroundCorrection(target, bytes, native)));
    restoreGroundCorrection(expected, bytes);
    assert.equal(result.thrown, false); assert.equal(native.calls, before + 1);
    assert.equal(target.position, position); assert.deepEqual(correctionObservation(target), correctionObservation(expected));
  } finally { native.dispose(); }
});
