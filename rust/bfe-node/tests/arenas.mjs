// Real addon boundary tests. Synthetic generated blocks need no game content.
// Run after build-node-host.mjs --fixtures with Node's --expose-gc flag.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {Worker} from 'node:worker_threads';
import {once} from 'node:events';
import {BlockHeaderLayout as BlockHeader} from '../../../src/sim/gen/boundary.js';
import {COMMAND_ECHO, COMMAND_FILL, KIND_BOLT, KINDS} from '../../../src/sim/gen/kinds.js';
import {BOLT_FULL_STRIDE, putFullBolt, BoltMirror} from '../../../src/sim/gen/ordnance.js';
const require = createRequire(import.meta.url);
const addonPath = require.resolve('../../../.host-addon/bfe-node.node');
const addon = require(addonPath);
const block = row => new Float64Array([1, row.length, BlockHeader.words + row.length, 0, 0, ...row]);
const echo = (events = 0) => block([41, -0, 0, 0, 0, 1, 0, events]);
const bits = words => new Uint8Array(words.buffer, words.byteOffset, words.byteLength);
const payload = 0x7ff8000000000042n;

// A capacity result is a word count larger than out.length, with no writes.
// Repeating that exact request copies the retained reply without a second op.
test('operation arenas retain capacity replies and emit events exactly once', () => {
  const owner = new addon.NativeHost().create();
  const args = echo(2), short = new Float64Array(12).fill(73), output = new Float64Array(20).fill(19);
  new BigUint64Array(args.buffer)[BlockHeader.words + 2] = payload;
  assert.equal(owner.operation(COMMAND_ECHO, args, short), 13);
  assert.ok(short.every(n => n === 73));
  assert.equal(owner.diagnostics().lastFault, 'Capacity');
  const changed = args.slice(); changed[BlockHeader.words + 1] = 0;
  assert.throws(() => owner.operation(COMMAND_ECHO, changed, output), /Busy/);
  assert.throws(() => owner.intern([65]), /Busy/);
  assert.equal(owner.operation(COMMAND_ECHO, args, output), 13);
  assert.equal(output[BlockHeader.words], 42);
  assert.ok(Object.is(output[BlockHeader.words + 1], -0));
  assert.equal(new BigUint64Array(output.buffer)[BlockHeader.words + 2], payload);
  assert.ok(output.subarray(13).every(n => n === 19));
  const events = new Float64Array(21);
  assert.equal(owner.events(new Float64Array(0)), events.length);
  assert.equal(owner.events(events), events.length);
  assert.equal(events[0], 20);
  assert.equal(owner.events(new Float64Array(1)), 1);
  owner.close();
});

test('kind import and export preserve all floating bits, offsets and retirement retries', () => {
  const owner = new addon.NativeHost().create(), output = new Float64Array(100);
  owner.operation(COMMAND_FILL, block([KIND_BOLT, 1]), output);
  const length = owner.exportKind(KIND_BOLT, 1, output);
  const input = output.slice(0, length), row = BlockHeader.words;
  // The generated encoder locates speed. Copy its baseline first so every
  // other field and optional tag remains valid while we replace raw bits.
  const mirror = new BoltMirror();
  const marked = input.slice();
  putFullBolt({ref: () => [0, 0], strings: {id: () => 0}, defs: {ordnance: {id: () => 0}}}, mirror, marked, row, []);
  const speed = (() => {
    mirror.speed = 12345;
    const next = marked.slice();
    putFullBolt({ref: () => [0, 0], strings: {id: () => 0}, defs: {ordnance: {id: () => 0}}}, mirror, next, row, []);
    return next.findIndex((value, at) => value === 12345 && marked[at] !== value);
  })();
  assert.ok(speed >= row);
  for (const value of [payload, 0xfff8000000000017n, 0x7ff0000000000042n, 0x8000000000000000n, 0n, 1n, 0x7ff0000000000000n]) {
    new BigUint64Array(input.buffer)[speed] = value;
    owner.importKind(KIND_BOLT, 1, input);
    assert.equal(owner.exportKind(KIND_BOLT, 1, output), length);
    assert.deepEqual(bits(output.subarray(0, length)), bits(input));
  }
  assert.equal(owner.retire(KIND_BOLT, input[row], input[row + 1]), true);
  const short = new Float64Array(1).fill(73);
  const required = owner.exportKind(KIND_BOLT, 0, short);
  assert.ok(required > short.length); assert.equal(short[0], 73);
  assert.throws(() => owner.exportKind(KIND_BOLT, 1, output), /Busy/);
  const retired = new Float64Array(required);
  assert.equal(owner.exportKind(KIND_BOLT, 0, retired), required);
  assert.equal(retired[BlockHeader.retired], 1);
  owner.exportKind(KIND_BOLT, 0, output);
  assert.equal(output[BlockHeader.retired], 0);
  owner.close();
});

test('ordinary overlapping arenas, empty outputs and detached backing are checked', () => {
  const owner = new addon.NativeHost().create(), storage = new Float64Array(32);
  storage.set(echo());
  assert.equal(owner.operation(COMMAND_ECHO, storage.subarray(0, 13), storage.subarray(2, 15)), 13);
  assert.equal(storage[2 + BlockHeader.words], 42);
  const detached = echo(); structuredClone(detached.buffer, {transfer: [detached.buffer]});
  assert.throws(() => owner.operation(COMMAND_ECHO, detached, storage), /Detached|Invalid/);
  assert.throws(() => owner.events(new Float32Array(20)), /Float64Array/);
  for (const shared of [true, false]) {
    const input = shared ? new Float64Array(new SharedArrayBuffer(104)) : echo();
    const output = shared ? storage : new Float64Array(new SharedArrayBuffer(256));
    assert.throws(() => owner.operation(COMMAND_ECHO, input, output), /SharedArrayBuffer/);
  }
  assert.equal(owner.diagnostics().poisoned, false);
  owner.close();
});

test('malformed headers, operation rows, kind layouts and huge counts fail before writes', () => {
  const owner = new addon.NativeHost().create(), out = new Float64Array(100).fill(73);
  for (const args of [new Float64Array(), block([]), block([1]), new Float64Array([1e300, 1, 5, 0, 0]), echo(1e9)]) {
    assert.throws(() => owner.operation(COMMAND_ECHO, args, out));
    assert.ok(out.every(n => n === 73));
  }
  assert.throws(() => owner.operation(0xffffffff, echo(), out), /Unsupported/);
  assert.throws(() => owner.operation(COMMAND_FILL, block([KIND_BOLT, 1e9]), out), /capacity/);
  assert.throws(() => owner.importKind(KIND_BOLT, 1, block([0, 0])), /layout/);
  const hugeSlot = new Float64Array(BlockHeader.words + BOLT_FULL_STRIDE);
  hugeSlot.set([1, BOLT_FULL_STRIDE, hugeSlot.length, 0, 0, 1e9]);
  assert.throws(() => owner.importKind(KIND_BOLT, 1, hugeSlot), /capacity/);
  assert.throws(() => owner.string(0xffffffff), /string ID/);
  assert.equal(owner.diagnostics().poisoned, false);
  owner.close();
});

test('UTF-16 IDs stay owner-local and counters remain available after close', () => {
  const host = new addon.NativeHost(), before = addon.lifecycleCounts(), a = host.create(), b = host.create();
  const units = [65, 0xd800, 0, 0xdc00];
  const id = a.intern(units);
  assert.equal(a.intern(units), id); assert.deepEqual(a.string(id), units);
  assert.equal(b.intern([66]), id); assert.deepEqual(b.string(id), [66]);
  assert.equal(a.close(), true); assert.equal(a.close(), false); b.close();
  const d = a.diagnostics();
  assert.equal(d.closed, true); assert.equal(d.usedWords, 0); assert.equal(d.reservedWords, 0);
  assert.equal(d.created - before[0], 2); assert.equal(d.released - before[1], 2);
  assert.equal(d.live, d.created - d.released);
  assert.throws(() => a.events(new Float64Array(1)), /closed/);
});

test('all stored kinds round trip their generated full blocks', () => {
  const owner = new addon.NativeHost().create();
  let kinds = 0;
  for (let kind = 1; kind <= KINDS; kind++) {
    let out = new Float64Array(4096), length;
    try { length = owner.exportKind(kind, 1, out); }
    catch (e) { assert.match(e.message, /no stored fields/); continue; }
    if (length > out.length) { out = new Float64Array(length); owner.exportKind(kind, 1, out); }
    const input = out.slice(0, length);
    owner.importKind(kind, 1, input);
    assert.equal(owner.exportKind(kind, 1, out), length);
    assert.deepEqual(bits(out.subarray(0, length)), bits(input)); kinds++;
  }
  assert.equal(kinds, 25); owner.close();
});

test('paused event burst is retained across short arenas without replay or panic', () => {
  const owner = new addon.NativeHost().create(), out = new Float64Array(13);
  for (let i = 0; i < 32; i++) owner.operation(COMMAND_ECHO, echo(1024), out);
  const short = new Float64Array(1).fill(73), required = owner.events(short);
  assert.equal(required, 1 + 32768 * 10); assert.equal(short[0], 73);
  const events = new Float64Array(required);
  assert.equal(owner.events(events), required);
  assert.equal(events[0], required - 1); assert.equal(owner.events(short), 1);
  owner.close();
});

test('construction acquisition failures and poisoned owners release once', () => {
  const host = new addon.NativeHost(), before = addon.lifecycleCounts();
  for (const acquisition of [1, 2, 3, 4]) assert.throws(() => host.probeFailure(acquisition), /acquisition fixture/);
  const owner = host.create(); assert.throws(() => owner.probeOperationPanic(), /Native host panic/);
  assert.equal(owner.diagnostics().poisoned, true);
  assert.throws(() => owner.intern([65]), /Poisoned/);
  assert.throws(() => owner.operation(COMMAND_ECHO, echo(), new Float64Array(13)), /Poisoned/);
  assert.equal(owner.close(), true); assert.equal(owner.close(), false);
  const after = addon.lifecycleCounts();
  assert.equal(after[0] - before[0], 5); assert.equal(after[1] - before[1], 5);
});

test('environment cleanup releases an owner holding an undelivered reply', async () => {
  const before = addon.lifecycleCounts();
  const worker = new Worker(`const {parentPort,workerData}=require('node:worker_threads');
    const addon=require(workerData),host=new addon.NativeHost();global.owner=host.create();
    owner.events(new Float64Array(0));parentPort.postMessage(owner.diagnostics().lastFault);setInterval(()=>{},1000);`,
    {eval: true, execArgv: [], workerData: addonPath});
  try { assert.deepEqual(await once(worker, 'message'), ['Capacity']); assert.equal(await worker.terminate(), 1); }
  finally { await worker.terminate(); }
  const after = addon.lifecycleCounts();
  assert.equal(after[0] - before[0], 1); assert.equal(after[1] - before[1], 1);
});


test('arena reservation belongs to admission and is released with the Context', () => {
  const host = new addon.NativeHost(), owner = host.create({inputWords: 64, outputWords: 128});
  assert.equal(owner.diagnostics().reservedWords, 192);
  assert.equal(owner.diagnostics().usedWords, 0);
  owner.close(); assert.equal(owner.diagnostics().reservedWords, 0);
  const before = addon.lifecycleCounts();
  assert.throws(() => host.create({inputWords: 0xffffffff, outputWords: 0}), /capacity/);
  const after = addon.lifecycleCounts();
  assert.equal(after[0] - before[0], 1); assert.equal(after[1] - before[1], 1);
});
