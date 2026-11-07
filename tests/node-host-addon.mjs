// Run after tools/build-node-host.mjs; the CI Rust job builds this fixture's
// real addon. Ordinary WASM suites need neither a compiler nor a native binary.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { Worker } from 'node:worker_threads';
import { once } from 'node:events';
import { readFileSync } from 'node:fs';
import { NativeBlocks } from '../src/sim/block-view.js';
const require = createRequire(import.meta.url);
const path = require.resolve('../.host-addon/bfe-node.node');
const addon = require(path);
const metadata = JSON.parse(readFileSync(new URL('../.host-addon/target.json', import.meta.url)));

test('addon handshake identifies the exact build without claiming native battles', () => {
  const hello = addon.handshake();
  assert.equal(hello.addonAbi, 1);
  assert.equal(hello.storeAbi, 1);
  assert.equal(hello.schema, metadata.schema);
  assert.equal(hello.engineBuild, metadata.engineBuild);
  assert.equal(hello.napiMinimum, 8);
  assert.equal(hello.nativeWorld, false);
  assert.equal(hello.nativePublication, false);
});

test('Rust panics stop at the N-API boundary', () => {
  assert.throws(() => addon.probePanic(), /Native host panic/);
  const host = new addon.NativeHost();
  host.create().close();
});

test('reusable native blocks preserve payload bits and outlive owner close', () => {
  const host = new addon.NativeHost(), owner = host.create();
  const input = new Float64Array([1, 2, 7, 0, 0, -0, NaN]);
  new DataView(input.buffer).setBigUint64(48, 0x7ff8000000000042n, true);
  const output = new Float64Array(input.length), blocks = new NativeBlocks();
  for (let i = 0; i < 10000; i++) owner.copyBlock(input, output);
  blocks.accept(output);
  assert.deepEqual(new Uint8Array(output.buffer), new Uint8Array(input.buffer));
  assert.equal(owner.close(), true);
  assert.equal(owner.close(), false);
  assert.equal(host.liveOwners(), 0);
  assert.deepEqual(new Uint8Array(output.buffer), new Uint8Array(input.buffer));
  assert.throws(() => owner.copyBlock(input, output), /closed/);
  structuredClone(output.buffer, { transfer: [output.buffer] });
  assert.equal(output.byteLength, 0);
});

test('invalid blocks and insufficient capacity leave output untouched', () => {
  const host = new addon.NativeHost(), owner = host.create(), output = new Float64Array(12).fill(73);
  for (const values of [[], [1, 1, 6, 0, 2, 1], [1e300, 1, 5, 0, 0], [1, NaN, 5, 0, 0], [1, -1, 5, 0, 0], [1, 0.5, 5, 0, 0]]) {
    assert.throws(() => owner.copyBlock(new Float64Array(values), output), /block/);
    assert.ok(output.every(n => n === 73));
  }
  assert.throws(() => owner.copyBlock(new Float64Array([1, 1, 6, 0, 0, 2]), new Float64Array(5)), /capacity/);
  owner.close();
});

test('sequential and concurrent owners release once, including GC', async () => {
  const host = new addon.NativeHost();
  for (let i = 0; i < 24; i++) { const a = host.create(), b = host.create(); assert.equal(host.liveOwners(), 2); a.close(); b.close(); }
  assert.equal(host.liveOwners(), 0);
  (() => { host.create(); })();
  for (let i = 0; i < 100 && host.liveOwners(); i++) { global.gc(); await new Promise(resolve => setTimeout(resolve, 5)); }
  assert.equal(host.liveOwners(), 0);
});

for (const afterCopy of [false, true]) test(`worker termination releases environment afterCopy=${afterCopy}`, async () => {
  const before = addon.lifecycleCounts();
  const worker = new Worker(`const {parentPort,workerData}=require('node:worker_threads');
    const a=require(workerData.path),host=new a.NativeHost();global.owner=host.create();
    if(workerData.afterCopy) owner.copyBlock(new Float64Array([0,0,5,0,0]),new Float64Array(5));
    parentPort.postMessage(host.liveOwners());setInterval(()=>{},1000);`, { eval: true, workerData: { path, afterCopy } });
  const [live] = await once(worker, 'message');
  assert.equal(live, 1);
  assert.equal(await worker.terminate(), 1);
  const after = addon.lifecycleCounts();
  assert.equal(after[0] - before[0], 1);
  assert.equal(after[1] - before[1], 1);
});
