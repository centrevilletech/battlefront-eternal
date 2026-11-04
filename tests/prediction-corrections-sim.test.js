import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { bridgeFor } from '../src/sim/runtime.js';
import { createCorrectionSim, createCorrectionBridge } from '../src/runtime/correction-sim.js';
import { createWireBridge } from '../src/runtime/wire-sim.js';
import { decodeWeaponState } from '../src/runtime/network-weapon-state.js';
import { decodeMountedControl } from '../src/runtime/network-mounted-control.js';
import { restoreSoldierCorrection } from '../src/runtime/network-entity-correction.js';
import { restoreFlightCorrection } from '../src/runtime/network-entity-flight.js';
import { captureOwnPlayer, applyOwnPlayer, decodeOwnPlayer } from '../src/runtime/network-entity-player.js';
import { PlayerCombat } from '../src/combat.js';
import { Arsenal } from '../src/weapons.js';
import { correctionGun, correctionDescriptors, correctionCases, correctionObservation, framedCorrection, runCorrectionCase } from './fixtures/prediction-correction-cases.js';

const byName = name => correctionDescriptors().find(d => d.name === name);
const compare = (a, b, label) => { for (const key of ['error', 'decoded', 'before', 'after']) assert.deepEqual(a[key], b[key], `${label} ${key}`); };

test('native correction readers match views, every layout, nonfinite rules and compound validation precedence', async () => {
  const native = createCorrectionSim(await loadSim()); let accepted = 0, rejected = 0;
  try {
    for (const { label, descriptor, variation } of correctionCases()) {
      const a = runCorrectionCase(descriptor, variation, native), b = runCorrectionCase(descriptor, variation);
      compare(a, b, label);
      if (label.endsWith(':valid')) assert.ok(a.calls > 0, `${label} native parser used`);
      if (a.error) rejected++; else accepted++;
    }
    assert.ok(accepted > 500 && rejected > 2000); assert.ok(native.calls > 2500);
  } finally { native.dispose(); }
});

test('selective correction restores preserve omitted state and ordinary shared vector identities', async () => {
  const native = createCorrectionSim(await loadSim());
  try {
    for (const descriptor of correctionDescriptors().filter(d => d.restore && !d.name.startsWith('mounted'))) {
      const variation = { target(body) {
        body.velocity = body.position;
        if (body.previous) body.previous = body.velocity;
        if (body.tickFrom) body.tickFrom = body.position;
        if (body.settled) body.settled = body.position;
        if (body.flightPreviousQ) body.flightPreviousQ = body.quaternion;
        if (body.flightTickQ) body.flightTickQ = body.quaternion;
        if (body.forwardAxis) body.forwardAxis.set(71, 73, 79);
        if (body.walkPhase !== undefined) { body.walkPhase = 123; body.wheelPhase = 456; body.legSide[0] = .9; }
        if (body.flightStart) body.flightStart.set(321, 654, 987);
      } };
      const a = runCorrectionCase(descriptor, variation, native), b = runCorrectionCase(descriptor, variation);
      compare(a, b, descriptor.name); assert.ifError(a.error); assert.ok(a.calls > 0, `${descriptor.name} alias path stays native`);
      if (descriptor.name.startsWith('soldier') && descriptor.name !== 'soldier-full') assert.deepEqual(a.after.forwardAxis, [71, 73, 79]);
      if (descriptor.name === 'ground') { assert.equal(a.after.walkPhase, 123); assert.equal(a.after.wheelPhase, 456); assert.deepEqual(a.after.collider, [7, 11, 13]); }
      if (descriptor.name === 'flight-compact-Flying-none') assert.deepEqual(a.after.flightStart, [321, 654, 987]);
    }
  } finally { native.dispose(); }
});

function observedRestore(descriptor, native, { mutateHeader = false, throwAt = null, foreign = null } = {}) {
  const body = descriptor.create(13), bytes = framedCorrection(descriptor.capture(descriptor.create(2))), events = [], values = new Map();
  const sentinel = new Error('publication stopped');
  const watch = (object, key, name = key) => {
    let value = object[key]; values.set(name, value);
    Object.defineProperty(object, key, { configurable: true, enumerable: true, get() { return value; }, set(next) {
      events.push(['set', name, next]); if (name === throwAt) throw sentinel; value = next;
      if (mutateHeader && name === 'fuel') { bytes[1] = 2; bytes[2] = 4; bytes[3] = 7; }
    } });
  };
  for (const key of ['fuel', 'aimYaw', 'health', 'team', 'yaw', 'tilt', 'roll', 'lookX', 'lookY', 'posture', 'air', 'grounded', 'diveFrom', 'previousYaw', 'tickFromYaw']) if (key in body) watch(body, key);
  for (const [key, vector] of Object.entries(body)) if (vector?.isVector3 || vector?.isQuaternion) {
    for (const method of ['fromArray', 'copy', 'set']) {
      const original = vector[method];
      vector[method] = function (...args) {
        const shown = method === 'fromArray' ? Array.from(args[0]) : method === 'copy' ? args[0].toArray() : args.slice();
        events.push([method, key, shown]); if (`${key}.${method}` === throwAt) throw sentinel;
        const result = original.apply(this, args); foreign?.({ body, bytes, key, method }); return result;
      };
    }
  }
  if (body.seats?.[0]) for (const key of ['aimYaw', 'pitch', 'lookYaw', 'lookPitch', 'yawRate', 'lastYaw']) watch(body.seats[0], key, `seat.${key}`);
  let error = null; const beforeCalls = native?.calls ?? 0;
  try { descriptor.restore(body, bytes, native); } catch (caught) { error = caught; }
  const recorded = events.slice(), state = correctionObservation(body);
  if (throwAt) assert.equal(error, sentinel); else assert.ifError(error);
  return { events: recorded, state, failed: !!error, calls: (native?.calls ?? 0) - beforeCalls };
}

test('native readers retain publication order, late compact header reads and partial writes when callbacks throw', async () => {
  const native = createCorrectionSim(await loadSim());
  try {
    for (const name of ['soldier-full', 'soldier-replay', 'soldier-compact-00', 'soldier-compact-11', 'droideka', 'ground', 'flight-full', 'flight-compact-Landing-immelmann']) {
      const descriptor = byName(name);
      for (const throwAt of [null, name.startsWith('soldier') ? 'aimYaw' : 'position.fromArray']) {
        const a = observedRestore(descriptor, native, { mutateHeader: name.startsWith('soldier-compact'), throwAt });
        const b = observedRestore(descriptor, null, { mutateHeader: name.startsWith('soldier-compact'), throwAt });
        assert.deepEqual(a.events, b.events, `${name} publication order`); assert.deepEqual(a.state, b.state, `${name} partial state`); assert.ok(a.calls > 0);
        if (!throwAt && name === 'soldier-compact-00') {
          assert.equal(a.state.posture, 'prone'); assert.equal(a.state.air, 'jet'); assert.equal(a.state.grounded, true); assert.equal(a.state.diveFrom, 'stand');
          assert.ok(a.events.findIndex(e => e[1] === 'lookY') < a.events.findIndex(e => e[1] === 'lookX'));
        }
        if (!throwAt && name === 'ground') assert.ok(a.events.findIndex(e => e[1] === 'tickFromYaw') < a.events.findIndex(e => e[1] === 'previousYaw'));
      }
    }
  } finally { native.dispose(); }
});

test('observable prerequisite reads use the original reader without extra getters or changed error timing', async () => {
  const native = createCorrectionSim(await loadSim());
  const run = (descriptor, backend, corrupt) => {
    const body = descriptor.create(13), payload = descriptor.capture(descriptor.create(2)), vector = body.position, events = [];
    Object.defineProperty(body, 'position', { enumerable: true, configurable: true, get() { events.push('position'); return vector; } });
    if (corrupt) new DataView(payload.buffer).setFloat64(descriptor.op === 1 ? 4 : descriptor.op === 5 ? 6 : 0, NaN, true);
    const calls = backend?.calls ?? 0; let error = null;
    try { descriptor.restore(body, payload, backend); } catch (caught) { error = caught.message; }
    const saved = events.slice(); return { error, events: saved, state: correctionObservation(body), calls: (backend?.calls ?? 0) - calls };
  };
  try {
    for (const name of ['soldier-full', 'soldier-compact-00', 'ground', 'flight-full', 'flight-compact-Flying-none']) for (const corrupt of [false, true]) {
      const a = run(byName(name), native, corrupt), b = run(byName(name), null, corrupt);
      assert.deepEqual(a, b, `${name} ${corrupt} getter fallback`); assert.equal(a.calls, 0);
    }
    for (const schema of ['2', null, {}, Symbol('schema')]) {
      const body = byName('flight-full').create(13), payload = byName('flight-full').capture(byName('flight-full').create(2));
      Object.defineProperty(body, 'flying', { get() { throw Error('schema should reject first'); } });
      assert.throws(() => restoreFlightCorrection(body, payload, schema, native), /unsupported flight correction/);
    }
    assert.throws(() => restoreSoldierCorrection(null, null, '1', native), /unsupported soldier correction/);
  } finally { native.dispose(); }
});

test('prerequisite getters can change unread payload values without an early snapshot', async () => {
  const native = createCorrectionSim(await loadSim());
  try {
    for (const name of ['soldier-full', 'soldier-compact-00', 'ground', 'flight-full', 'flight-compact-Flying-none']) for (const shape of ['own', 'inherited-position', 'inherited-tag']) {
      const run = backend => {
        const descriptor = byName(name), body = descriptor.create(13), bytes = descriptor.capture(descriptor.create(2));
        const position = body.position, events = [];
        const at = descriptor.op === 1 ? 52 : descriptor.op === 2 ? 160 : descriptor.op === 4 ? 88 : descriptor.op === 5 ? 238 : 296;
        const property = { configurable: true, enumerable: true, get() {
          events.push('position'); new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).setFloat64(at, 987.125, true); return position;
        } };
        if (shape === 'own') Object.defineProperty(body, 'position', property);
        else if (shape === 'inherited-position') { delete body.position; Object.setPrototypeOf(body, Object.create(Object.getPrototypeOf(body), { position: property })); }
        else Object.setPrototypeOf(position, Object.create(Object.getPrototypeOf(position), { isVector3: { get() { property.get(); return true; } } }));
        const calls = backend?.calls ?? 0;
        descriptor.restore(body, bytes, backend);
        const saved = events.slice(), state = correctionObservation(body); state.position = position.toArray();
        return { state, events: saved, calls: (backend?.calls ?? 0) - calls };
      };
      const a = run(native), b = run(null); assert.deepEqual(a, b, `${name} ${shape} live bytes`);
      assert.equal(a.calls, 0); assert.equal(a.state.position[0], 987.125);
    }
  } finally { native.dispose(); }
});

test('floating point payload bits and orientation validation retain each correction format policy', async () => {
  const native = createCorrectionSim(await loadSim());
  try {
    for (const name of ['flight-full', 'flight-compact-Flying-none']) {
      const descriptor = byName(name), at = name === 'flight-full' ? 16 * 8 : 6 + 13 * 8;
      for (const bits of [0x7ff8000000000001n, 0x7ff8123456789abcn, 0xfff8000000001234n, 0x7ff0000000000001n]) {
        const variation = { payload(bytes) { new DataView(bytes.buffer).setBigUint64(at, bits, true); return bytes; } };
        const a = runCorrectionCase(descriptor, variation, native), b = runCorrectionCase(descriptor, variation);
        compare(a, b, `${name} NaN ${bits.toString(16)}`); assert.ifError(a.error); assert.ok(a.calls > 0);
        assert.deepEqual(descriptor.capture(a.target), descriptor.capture(b.target), `${name} NaN recapture bytes`);
      }
    }
    for (const [name, offset, accepted] of [['ground', 136, false], ['flight-full', 464, true], ['flight-compact-Flying-none', 286, false]]) {
      const descriptor = byName(name), variation = { payload(bytes) { new DataView(bytes.buffer).setFloat64(offset, 1, true); return bytes; } };
      const a = runCorrectionCase(descriptor, variation, native), b = runCorrectionCase(descriptor, variation);
      compare(a, b, `${name} finite nonunit quaternion`); assert.equal(a.error === null, accepted);
    }
  } finally { native.dispose(); }
});

test('mounted decode preserves Buffer slice aliases and custom byte method precedence', async () => {
  const native = createCorrectionSim(await loadSim()), descriptor = byName('mounted-2');
  try {
    for (const buffer of [false, true]) {
      const bytes = framedCorrection(descriptor.capture(descriptor.create(2)), 5, buffer), decoded = decodeMountedControl(bytes, native);
      assert.equal(Buffer.isBuffer(decoded.weapons[0].payload), buffer);
      const before = decoded.weapons[0].payload[0]; bytes.fill(255);
      assert.equal(decoded.weapons[0].payload[0], buffer ? 255 : before);
    }
    const run = backend => {
      const bytes = descriptor.capture(descriptor.create(2)), events = [], slice = bytes.slice;
      Object.defineProperty(bytes, 'slice', { configurable: true, get() { events.push('slice'); return function (...args) { events.push(args); return slice.apply(this, args); }; } });
      const calls = backend?.calls ?? 0, value = decodeMountedControl(bytes, backend);
      return { value, events, calls: (backend?.calls ?? 0) - calls };
    };
    assert.deepEqual(run(native), run(null));
    for (const action of ['constructor-throw', 'species-throw', 'species-mutate']) {
      const run = backend => {
        const bytes = descriptor.capture(descriptor.create(2)), decoded = decodeMountedControl(bytes), events = [], marker = new Error('slice observed');
        const second = 36 + 10 + decoded.weapons[0].payload.length;
        const constructor = { get [Symbol.species]() {
          events.push('species'); if (action === 'species-throw') throw marker;
          new DataView(bytes.buffer).setFloat64(second, NaN, true); return Uint8Array;
        } };
        Object.defineProperty(bytes, 'constructor', { get() { events.push('constructor'); if (action === 'constructor-throw') throw marker; return constructor; } });
        let error = null; const before = backend?.calls ?? 0;
        try { decodeMountedControl(bytes, backend); } catch (caught) { error = { message: caught.message, original: caught === marker }; }
        return { events, error, calls: (backend?.calls ?? 0) - before };
      };
      const a = run(native), b = run(null); assert.deepEqual(a, b); assert.ok(a.error); assert.equal(a.calls, 0);
    }
    const buffer = Buffer.from(descriptor.capture(descriptor.create(2))), expected = decodeMountedControl(buffer), before = native.calls;
    Object.defineProperty(buffer, 'constructor', { get() { throw Error('Buffer slices do not use species'); } });
    assert.deepEqual(decodeMountedControl(buffer, native), expected); assert.ok(native.calls > before);
    for (const invalid of [false, true]) {
      const bytes = byName('weapon').capture(byName('weapon').create(2)), marker = new Error('buffer observed'); let reads = 0;
      if (invalid) bytes[1] = 255;
      Object.defineProperty(bytes, 'buffer', { get() { reads++; throw marker; } });
      assert.throws(() => decodeWeaponState(bytes, native), error => invalid ? error.message === 'invalid weapon state payload' : error === marker);
      assert.equal(reads, invalid ? 0 : 1);
    }
  } finally { native.dispose(); }
});

test('payloads borrowed from WASM memory or a shared buffer retain original read and slice behavior', async () => {
  const instance = await loadSim(), native = createCorrectionSim(instance), descriptor = byName('mounted-2');
  try {
    const source = descriptor.capture(descriptor.create(2));
    const page = instance.exports.memory.grow(1), memory = new Uint8Array(instance.exports.memory.buffer, page * 65536 + 17, source.length);
    const shared = new Uint8Array(new SharedArrayBuffer(source.length + 13), 13);
    for (const bytes of [memory, shared]) {
      bytes.set(source); const before = native.calls;
      const a = decodeMountedControl(bytes, native), b = decodeMountedControl(bytes);
      assert.deepEqual(a, b); assert.equal(native.calls, before, 'borrowed byte storage stays with the original reader');
      bytes.fill(255); assert.deepEqual(a, b, 'ordinary Uint8Array slices remain owned');
    }
  } finally { native.dispose(); }
});

test('own-player restore propagates native readers through body, arsenal and mounted validation without changing partial writes', async () => {
  const native = createCorrectionSim(await loadSim());
  const registry = { contentEpoch: 42, classes: new Map([[1, 'trooper']]), weapons: new Map([[1, 'correction-gun']]), inputCodes: ['KeyW'], bindingProfiles: new Map([[0, {}]]) };
  const unit = seed => {
    const soldier = byName('soldier-compact-00').create(seed), combat = Object.create(PlayerCombat.prototype);
    Object.assign(combat, { arsenal: new Arsenal([correctionGun()], { player: true }), health: seed + 60, zoomStep: 0, firstPerson: true, soldier,
      kick: { rate: seed / 8, hold: .8, offset: -.1 }, dead: false, deathTime: 0 });
    combat.arsenal.weapons[0].clip = seed / 32;
    return { id: 1, generation: 7, team: 1, className: 'trooper', soldier, combat };
  };
  const observe = u => ({ team: u.team, deadTime: u.deadTime, removed: u.removed, body: correctionObservation(u.soldier),
    combat: correctionObservation(u.combat), kick: { ...u.combat.kick }, selected: u.combat.arsenal.selected.slice(),
    weapons: u.combat.arsenal.weapons.map(correctionObservation), mount: u.seat ? correctionObservation(u.seat.vehicle.seats[1]) : null });
  const run = (backend, mode) => {
    const source = unit(2), target = unit(13), marker = new Error('own publication stopped');
    if (mode === 'mounted') { source.seat = byName('mounted-2').create(2).seat; target.seat = byName('mounted-2').create(13).seat; }
    const captured = captureOwnPlayer({ unit: source, tick: 100, epoch: 3, registry, classId: 1, held: ['KeyW'] });
    const record = { ...captured, payload: framedCorrection(captured.payload) }, bodyLength = decodeOwnPlayer(record).body.length;
    if (mode === 'bad-body') new DataView(record.payload.buffer, record.payload.byteOffset).setFloat64(22 + 4, NaN, true);
    if (mode === 'bad-weapon') record.payload[22 + bodyLength + 63 + 4 + 1] = 255;
    if (mode === 'publication-throw') target.soldier.position.fromArray = () => { throw marker; };
    const before = observe(target), calls = backend?.calls ?? 0; let context = null, error = null;
    try { context = applyOwnPlayer({ unit: target, epoch: 3, registry, record, native: backend }); }
    catch (caught) { error = { name: caught.name, message: caught.message, original: caught === marker }; }
    const after = observe(target);
    if (mode.startsWith('bad')) assert.deepEqual(after, before, 'complete validation precedes body and combat publication');
    if (mode === 'publication-throw') { assert.equal(error.original, true); assert.notDeepEqual(after.body, before.body); assert.deepEqual(after.weapons, before.weapons); assert.deepEqual(after.kick, before.kick); }
    if (context) {
      const next = applyOwnPlayer({ unit: target, epoch: 3, registry, record, previous: context, native: backend });
      assert.equal(next.reset, false);
    }
    return { context, error, after, calls: (backend?.calls ?? 0) - calls };
  };
  try {
    for (const mode of ['valid', 'mounted', 'bad-body', 'bad-weapon', 'publication-throw']) {
      const a = run(native, mode), b = run(null, mode);
      for (const key of ['context', 'error', 'after']) assert.deepEqual(a[key], b[key], `${mode} own ${key}`);
      assert.ok(a.calls > 0, `${mode} propagated native reader`);
      if (!mode.startsWith('bad') && mode !== 'publication-throw') assert.ifError(a.error);
    }
  } finally { native.dispose(); }
});

test('decoded values remain owned through nested shared bridge calls, diagnostics, memory growth and disposal', async () => {
  const instance = await loadSim(), bridge = bridgeFor(instance), wire = createWireBridge(bridge);
  const gun = byName('weapon'), payload = gun.capture(gun.create(2)); let nested = false, grew = false, altered = 0;
  const native = createCorrectionBridge(bridge, { trace(event) {
    if (event.output.length) { event.output[0] = 999; altered++; }
    if (!nested) { nested = true; try { assert.deepEqual(decodeWeaponState(payload, native), decodeWeaponState(payload)); } finally { nested = false; } }
  } });
  try {
    const descriptor = byName('soldier-full');
    const a = observedRestore(descriptor, native, { foreign() {
      wire.encodeWordDelta(new Uint8Array([3, 1, 7, 9])); decodeWeaponState(payload, native);
      if (!grew) { instance.exports.memory.grow(1); grew = true; }
    } });
    const b = observedRestore(descriptor, null); assert.deepEqual(a.events, b.events); assert.deepEqual(a.state, b.state); assert.ok(grew && altered > 10);
    const c = observedRestore(descriptor, native, { foreign({ key, method }) { if (key === 'position' && method === 'fromArray') native.dispose(); } });
    assert.deepEqual(c.state, b.state); native.dispose(); assert.throws(() => decodeWeaponState(payload, native), /disposed/);
  } finally { native.dispose(); }
});
