// The Rust CI job supplies the instrumented binary. Content-free real Context
// tests run independently of the long game-data qualification tools.
import test from 'node:test';
import {fileURLToPath} from 'node:url';
import {qualifyAddon} from '../tools/host-qualification/addon.mjs';
test('Context resources release after failed construction, GC and active worker termination',async()=>{
  await qualifyAddon(fileURLToPath(new URL('../.host-addon/bfe-node.node',import.meta.url)));
});

// Termination is requested while the production step boundary holds its owner.
// The gate delays the actual Context entry; it never supplies a fake step.
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFileSync} from 'node:fs';
import {Worker} from 'node:worker_threads';
import {once} from 'node:events';
const require = createRequire(import.meta.url);
const addonPath = require.resolve('../.host-addon/bfe-node.node');
const addon = require(addonPath);
const worldFixture = JSON.parse(readFileSync(new URL('../.host-addon/world-fixtures.json', import.meta.url)));

test('termination during the native step boundary releases the Context once', async () => {
  const before = addon.lifecycleCounts(), signal = new Int32Array(new SharedArrayBuffer(8));
  const worker = new Worker(`const {parentPort,workerData}=require('node:worker_threads');
    const addon=require(workerData.path),host=new addon.NativeHost();global.owner=host.create();
    const setup=Float64Array.from(workerData.fixture.setup);
    try{owner.stepInit(setup);}catch(e){if(!e.message.includes('Unavailable'))throw e;}
    parentPort.postMessage('ready');
    try{owner.probeStep(Float64Array.from(workerData.fixture.input),new Float64Array(workerData.fixture.outputWords),new Uint32Array(workerData.signal),5000);}
    catch(e){if(!e.message.includes('Unavailable'))throw e;}
    setInterval(()=>{},1000);`, {eval: true, execArgv: [], workerData: {path: addonPath, fixture: worldFixture, signal: signal.buffer}});
  try {
    await Promise.race([once(worker, 'message'), once(worker, 'error').then(([error]) => {throw error;})]);
    const deadline = Date.now() + 5000;
    while (Atomics.load(signal, 0) !== 1 && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 1));
    assert.equal(Atomics.load(signal, 0), 1);
    const stopping = worker.terminate();
    await new Promise(resolve => setTimeout(resolve, 20)); Atomics.store(signal, 1, 1);
    assert.equal(await stopping, 1);
  } finally {Atomics.store(signal, 1, 1); await worker.terminate();}
  const after = addon.lifecycleCounts();
  assert.equal(after[0] - before[0], 1); assert.equal(after[1] - before[1], 1);
});
