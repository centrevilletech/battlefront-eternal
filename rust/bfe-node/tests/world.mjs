// The same addon surface is tested while native bodies are unavailable and
// after they arrive. A capability flag never substitutes for calling the API.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFileSync} from 'node:fs';
import {WorldTickInputLayout as I, WorldTickOutputLayout as O, WorldTickSetupLayout as S} from '../../../src/sim/gen/world.js';
import {WireWorldFactsLayout as F} from '../../../src/sim/gen/wire-store.js';
import {WirePublicationOutputLayout as P, WirePublishedGroupLayout as G, WirePublishedOwnLayout as A} from '../../../src/sim/gen/wire-publish.js';
import {BlockHeaderLayout as H} from '../../../src/sim/gen/boundary.js';
import {OPERATION_ECHO} from '../../../src/sim/gen/kinds.js';
const require = createRequire(import.meta.url);
const path = require.resolve('../../../.host-addon/bfe-node.node');
const addon = require(path), hello = addon.handshake();
const metadata = JSON.parse(readFileSync(new URL('../../../.host-addon/target.json', import.meta.url)));
const fixture = JSON.parse(readFileSync(new URL('../../../.host-addon/world-fixtures.json', import.meta.url)));
const setup = () => Float64Array.from(fixture.setup);
const input = () => Float64Array.from(fixture.input);
const facts = () => ({session: 0, words: new Float64Array(F.words)});
const bits = words => new Uint8Array(words.buffer, words.byteOffset, words.byteLength);
const emptyEvents = 6;
function step(owner, source, output) {
  if (!hello.nativeWorld) {
    assert.throws(() => owner.step(source, output), /Unavailable: world_step/);
    return null;
  }
  assert.equal(owner.step(source, output), O.words);
  return output;
}
function initialize(owner) {
  if (!hello.nativeWorld && !hello.nativePublication) assert.throws(() => owner.stepInit(setup()), /Unavailable: world_step_init/);
  else owner.stepInit(setup());
}

test('world capabilities and operation identity describe the linked Context', () => {
  assert.equal(hello.nativeWorld, metadata.nativeWorld);
  assert.equal(hello.nativePublication, metadata.nativePublication);
  assert.equal(hello.operationTable, metadata.operationTable);
  for (const name of ['stepInit', 'step', 'publish', 'readBank']) assert.equal(typeof addon.NativeOwner.prototype[name], 'function');
  assert.equal(I.words, fixture.input.length); assert.equal(O.words, fixture.outputWords); assert.equal(S.words, fixture.setup.length);
  assert.equal(hello.addonAbi, 2);
});

test('unavailable world entries leave output and both owners usable', () => {
  const host = new addon.NativeHost(), a = host.create(), b = host.create(), out = new Float64Array(1024).fill(73);
  try {
    for (const owner of [a, b, a, b]) {
      initialize(owner); step(owner, input(), out);
      if (!hello.nativePublication) assert.throws(() => owner.publish(facts(), out), /Unavailable: world_publish/);
      else assert.throws(() => owner.publish(facts(), out), /Invalid|Refused|Host|session/i);
      if (!hello.nativeWorld && !hello.nativePublication) assert.throws(() => owner.readBank(emptyEvents, out), /Unavailable: read_bank/);
      else assert.ok(owner.readBank(emptyEvents, out) <= out.length);
      assert.equal(owner.diagnostics().poisoned, false);
      const units = [65, 0xd800, 0]; assert.deepEqual(owner.string(owner.intern(units)), units);
    }
    if (!hello.nativeWorld && !hello.nativePublication) assert.ok(out.every(word => word === 73));
  } finally {a.close(); b.close();}
});

test('two owners interleave world calls without exchanging clocks or store IDs', () => {
  const host = new addon.NativeHost(), a = host.create(), b = host.create();
  const aOut = new Float64Array(O.words), bOut = new Float64Array(O.words);
  try {
    initialize(a); initialize(b);
    const aid = a.intern([65]), bid = b.intern([66]); assert.equal(aid, bid);
    const x = input(), y = input(); x[I.tick] = 101; y[I.tick] = 202;
    // Bare owners refuse a tick with its clock unchanged. Distinct raw clock
    // payloads expose cross-owner copies as well as NaN canonicalization.
    const aClock = x.subarray(I.clock, I.units), bClock = y.subarray(I.clock, I.units);
    const aBits = new BigUint64Array(aClock.buffer, aClock.byteOffset, aClock.length);
    const bBits = new BigUint64Array(bClock.buffer, bClock.byteOffset, bClock.length);
    for (let i = 0; i < aBits.length; i++) {aBits[i] = 0x7ff8000000000042n + BigInt(i); bBits[i] = 0xfff8000000000017n + BigInt(i);}
    aBits[0] = 0x8000000000000000n; bBits[0] = 0n;
    for (let tick = 0; tick < 16; tick++) {
      const previous = step(a, x, aOut); const saved = previous && bits(previous).slice();
      step(b, y, bOut);
      if (saved) assert.deepEqual(bits(aOut), saved);
      if (hello.nativeWorld) {
        assert.equal(aOut[O.accepted], 0); assert.equal(bOut[O.accepted], 0);
        assert.deepEqual(bits(aOut.subarray(O.clock, O.ordnance)), bits(aClock));
        assert.deepEqual(bits(bOut.subarray(O.clock, O.ordnance)), bits(bClock));
      }
      assert.deepEqual(a.string(aid), [65]); assert.deepEqual(b.string(bid), [66]);
    }
    assert.equal(a.close(), true); assert.equal(a.close(), false);
    step(b, y, bOut); assert.deepEqual(b.string(bid), [66]);
    assert.equal(b.close(), true); assert.equal(b.close(), false);
    assert.equal(host.liveOwners(), 0);
  } finally {a.close(); b.close();}
});

test('replay fixture validation leaves the real owner usable', () => {
  const owner = new addon.NativeHost().create(), setup = new Float64Array(S.words);
  try {
    const short = new Float64Array(S.words - 1).fill(73);
    assert.equal(owner.probeWorldAdmit('{', short), S.words);
    assert.ok(short.every(word => word === 73));
    assert.throws(() => owner.probeWorldAdmit('{', setup), /Invalid replay admission JSON/);
    assert.throws(() => owner.probeWorldPrepare('{'), /Invalid replay frame JSON/);
    assert.throws(() => owner.probeWorldObserve('{}', new Float64Array(O.words - 1)), /Invalid replay output rows/);
    assert.deepEqual(owner.string(owner.intern([65])), [65]);
    assert.equal(owner.diagnostics().poisoned, false);
  } finally {owner.close();}
});

test('world row validation and shared/detached arenas fail before output writes', () => {
  const owner = new addon.NativeHost().create(), out = new Float64Array(O.words).fill(73);
  try {
    assert.throws(() => owner.stepInit(new Float64Array(S.words - 1)), /setup row/);
    assert.throws(() => owner.step(new Float64Array(I.words - 1), out), /tick row/);
    assert.throws(() => owner.step(new Float64Array(new SharedArrayBuffer(I.words * 8)), out), /SharedArrayBuffer/);
    const detached = input(); structuredClone(detached.buffer, {transfer: [detached.buffer]});
    assert.throws(() => owner.step(detached, out), /Detached|Invalid/);
    assert.throws(() => owner.publish({session: 0, words: new Float64Array(F.words - 1)}, out), /facts row/);
    assert.ok(out.every(word => word === 73)); assert.equal(owner.diagnostics().poisoned, false);
  } finally {owner.close();}
});

test('publication input capacity grows only through explicit admission', () => {
  const owner = new addon.NativeHost().create({inputWords: I.words, outputWords: O.words});
  const words = new Float64Array(128), out = new Float64Array(128).fill(73);
  const reserved = owner.diagnostics().reservedWords;
  try {
    assert.throws(() => owner.publish({session: 0, words}, out), /admission capacity/);
    assert.equal(owner.diagnostics().reservedWords, reserved);
    assert.ok(out.every(word => word === 73));
    owner.reserve({inputWords: words.length, outputWords: out.length});
    assert.throws(() => owner.publish({session: 0, words}, out),
      hello.nativePublication ? /InvalidKind/ : /Unavailable: world_publish/);
    assert.equal(owner.diagnostics().poisoned, false);
  } finally {owner.close();}
});

test('world capacity retries retain one reply and require identical input bits', () => {
  const owner = new addon.NativeHost().create(), source = input(), short = new Float64Array(O.words - 1).fill(73);
  try {
    initialize(owner);
    if (!hello.nativeWorld) {
      assert.throws(() => owner.step(source, short), /Unavailable: world_step/);
      assert.ok(short.every(word => word === 73)); return;
    }
    assert.equal(owner.step(source, short), O.words); assert.ok(short.every(word => word === 73));
    const changed = source.slice(); changed[I.tick]++;
    assert.throws(() => owner.step(changed, new Float64Array(O.words)), /Busy/);
    assert.throws(() => owner.stepInit(setup()), /Busy/);
    owner.reserve({inputWords: I.words, outputWords: O.words});
    const out = new Float64Array(O.words); assert.equal(owner.step(source, out), O.words);
    assert.equal(owner.diagnostics().usedWords, 0);
  } finally {owner.close();}
});

test('bank capacity retains the snapshot and does not drain Context events', () => {
  const owner = new addon.NativeHost().create({inputWords: 32, outputWords: 32});
  const short = new Float64Array(1).fill(73), out = new Float64Array(32);
  try {
    if (!hello.nativeWorld && !hello.nativePublication) {
      assert.throws(() => owner.readBank(emptyEvents, short), /Unavailable: read_bank/);
      assert.equal(short[0], 73); return;
    }
    const args = new Float64Array([1, 8, 13, 0, 0, 41, -0, 0, 0, 0, 1, 0, 2]);
    owner.operation(OPERATION_ECHO, args, out);
    const required = owner.readBank(emptyEvents, short);
    assert.equal(required, 21); assert.equal(short[0], 73);
    assert.equal(owner.diagnostics().lastFault, 'Capacity');
    assert.throws(() => owner.readBank(1, out), /Busy/);
    assert.throws(() => owner.stepInit(setup()), /Busy/);
    owner.reserve({inputWords: 64, outputWords: required});
    assert.equal(owner.readBank(emptyEvents, out), required);
    const snapshot = bits(out.subarray(0, required)).slice();
    assert.equal(owner.readBank(emptyEvents, out), required);
    assert.deepEqual(bits(out.subarray(0, required)), snapshot);
    assert.equal(owner.events(out), required);
    assert.deepEqual(bits(out.subarray(0, required)), snapshot);
    assert.equal(owner.readBank(emptyEvents, out), 1);
    assert.throws(() => owner.readBank(0xffffffff, out), /InvalidKind/);
    assert.equal(owner.diagnostics().poisoned, false);
    assert.equal(owner.diagnostics().usedWords, 0);
  } finally {owner.close();}
});

test('owned packet buffers have exact spans, survive close and transfer independently', () => {
  const owner = new addon.NativeHost().create(), out = new Float64Array(256).fill(73);
  const short = new Float64Array(1).fill(73), capacity = owner.probePacketBuffers(short, false);
  assert.ok(capacity.words > short.length); assert.equal(short[0], 73); assert.deepEqual(capacity.packets, []);
  assert.throws(() => owner.probePacketBuffers(out, true), /byte span/);
  const result = owner.probePacketBuffers(out, false);
  assert.deepEqual(result.packets.map(packet => packet.byteLength), [3, 2, 1, 1]);
  assert.deepEqual(result.packets.map(packet => [...new Uint8Array(packet)]), [[0, 1, 255], [3, 4], [5], [6]]);
  const heap = out[H.heap], row = H.words;
  const shared = heap + out[row + P.shared], parts = heap + out[shared + G.parts];
  const own = heap + out[row + P.own];
  assert.equal(out[parts], out[own + A.span]);
  assert.ok(Object.is(out[row + P.epoch], -0));
  assert.equal(new BigUint64Array(out.buffer)[row + P.tick], 0x7ff8000000000042n);
  owner.close(); owner.close();
  assert.equal(owner.diagnostics().publicationUsedBytes, 0);
  const moved = structuredClone(result.packets, {transfer: result.packets});
  assert.ok(result.packets.every(packet => packet.byteLength === 0));
  assert.deepEqual(moved.map(packet => [...new Uint8Array(packet)]), [[0, 1, 255], [3, 4], [5], [6]]);
});

test('a Context panic becomes GenericFailure, poisons later entries and permits double close', () => {
  const owner = new addon.NativeHost().create();
  assert.throws(() => owner.probeContextPanic(), {code: 'GenericFailure'});
  for (const call of [() => owner.stepInit(setup()), () => owner.step(input(), new Float64Array(O.words)),
    () => owner.publish(facts(), new Float64Array(1024)), () => owner.readBank(emptyEvents, new Float64Array(1024)),
    () => owner.intern([65])]) assert.throws(call, /Poisoned/);
  assert.equal(owner.diagnostics().poisoned, true);
  assert.equal(owner.close(), true); assert.equal(owner.close(), false);
});

// Exported for the lifecycle entrypoint, so the Worker fixture's native gate
// remains identical when Context changes from Unavailable to a real step.
export {fixture};
