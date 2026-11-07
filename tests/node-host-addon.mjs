// Run after tools/build-node-host.mjs; the CI Rust job builds this fixture's
// real addon. Ordinary WASM suites need neither a compiler nor a native binary.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { Worker } from 'node:worker_threads';
import { once } from 'node:events';
import { readFileSync } from 'node:fs';
import {collect} from '../tools/host-qualification/common.mjs';
const require = createRequire(import.meta.url);
const path = require.resolve('../.host-addon/bfe-node.node');
const addon = require(path);
const metadata = JSON.parse(readFileSync(new URL('../.host-addon/target.json', import.meta.url)));

test('collected hosts remove hooks and open owners retain their cleanup scope', async () => {
  const before = addon.cleanupHookCounts(), refs = [];
  (() => {for (let i = 0; i < 64; i++) refs.push(new WeakRef(new addon.NativeHost()));})();
  assert.ok(await collect(refs));
  let after = addon.cleanupHookCounts();
  assert.equal(after[0] - before[0], 64);assert.equal(after[1] - before[1], 64);
  let owner;
  const hostRef = (() => {const host = new addon.NativeHost();owner = host.create();return new WeakRef(host);})();
  assert.ok(await collect([hostRef]));
  after = addon.cleanupHookCounts();assert.equal(after[0] - after[1], before[0] - before[1] + 1);
  owner.probeIdentity('surviving owner', 3);assert.equal(owner.close(), true);
  after = addon.cleanupHookCounts();assert.equal(after[0] - after[1], before[0] - before[1]);
});

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
  const output = new Float64Array(input.length);
  for (let i = 0; i < 10000; i++) owner.copyBlock(input, output);
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

test('close and finalizer panics remain inside the Rust boundary', async () => {
  const host = new addon.NativeHost(), owner = host.create();owner.probeClosePanic();
  assert.throws(() => owner.close(), /Native host panic/);
  assert.equal(owner.close(), false);assert.equal(host.liveOwners(), 0);
  const before = addon.lifecycleCounts();
  const ref = (() => {const each=host.create();each.probeClosePanic();return new WeakRef(each);})();
  assert.ok(await collect([ref]));assert.equal(host.liveOwners(), 0);
  const after = addon.lifecycleCounts();assert.equal(after[0]-before[0],1);assert.equal(after[1]-before[1],1);
});

test('environment cleanup catches one close panic and still releases another owner', async () => {
  const before = addon.lifecycleCounts(), hooks = addon.cleanupHookCounts();
  const worker = new Worker(`const {parentPort,workerData}=require('node:worker_threads');
    const addon=require(workerData.path),host=new addon.NativeHost();
    global.first=host.create();global.second=host.create();first.probeClosePanic();
    parentPort.postMessage('ready');setInterval(()=>{},1000);`, {eval:true,execArgv:[],workerData:{path}});
  try {await once(worker,'message');assert.equal(await worker.terminate(),1);}
  finally {await worker.terminate();}
  const after=addon.lifecycleCounts(), removed=addon.cleanupHookCounts();
  assert.equal(after[0]-before[0],2);assert.equal(after[1]-before[1],2);
  assert.equal(removed[0]-hooks[0],1);assert.equal(removed[1]-hooks[1],1);
});

test('overlapping ordinary arenas preserve exact bits', () => {
  const owner=new addon.NativeHost().create(), arena=new Float64Array(12);
  arena.set([1,2,7,0,0,-0,NaN]);new DataView(arena.buffer).setBigUint64(48,0x7ff8000000000042n,true);
  const source=arena.subarray(0,7), output=arena.subarray(2,9), expected=new Uint8Array(source.buffer,0,source.byteLength).slice();
  assert.equal(owner.copyBlock(source,output),7);assert.deepEqual(new Uint8Array(output.buffer,output.byteOffset,output.byteLength),expected);
  assert.equal(owner.copyBlock(output,output),7);owner.close();
});

test('shared source or destination is rejected before any numeric memory access', async () => {
  const owner=new addon.NativeHost().create(), source=new Float64Array(new SharedArrayBuffer(56));source.set([1,2,7,0,0,-0,NaN]);
  const ordinary=new Float64Array([1,2,7,0,0,-0,NaN]), output=new Float64Array(7).fill(73);
  const sharedOutput=new Float64Array(new SharedArrayBuffer(56)).fill(73);
  assert.throws(()=>owner.copyBlock(ordinary,sharedOutput),/SharedArrayBuffer/);assert.ok(sharedOutput.every(n=>n===73));
  const worker=new Worker(`const {parentPort,workerData}=require('node:worker_threads');
    const words=new Float64Array(workerData);parentPort.postMessage('ready');for(;;){words[0]=Math.random();}`, {eval:true,execArgv:[],workerData:source.buffer});
  try {await once(worker,'message');for(let i=0;i<100;i++)assert.throws(()=>owner.copyBlock(source,output),/SharedArrayBuffer/);assert.ok(output.every(n=>n===73));}
  finally {await worker.terminate();owner.close();}
});

// Keep the provider's arena tests in the existing native CI entrypoint.
await import('./node-host-backend.mjs');
