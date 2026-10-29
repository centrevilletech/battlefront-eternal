// Synthetic browser experiment; imports the actual Soldier/CollisionWorld.
// Run against Vite. No application/game defaults or hosting headers change.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const base = process.env.RUNTIME_AUDIT_URL || 'http://127.0.0.1:18449';
const browser = await chromium.launch();
try {
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/runtime-lab', route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><title>Runtime worker experiment</title>' }));
  await page.goto(`${base}/runtime-lab`);
  const report = await page.evaluate(async () => {
    const { movementFixture, replayCommands } = await import('/tools/runtime-lab/fixture.js');
    const worker = new Worker('/tools/runtime-lab/simulation-worker.js', { type: 'module' });
    const pending = new Map(), samples = [];
    let serial = 0;
    const now = () => performance.timeOrigin + performance.now();
    const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
    const rpc = (kind, fields = {}, transfer = []) => new Promise((resolve, reject) => {
      const id = ++serial;
      const timer = setTimeout(() => { pending.delete(id); reject(new Error(`worker ${kind} timeout`)); }, 15000);
      pending.set(id, { resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } });
      worker.postMessage({ id, kind, ...fields }, transfer);
    });
    worker.onerror = event => { for (const request of pending.values()) request.reject(new Error(event.message)); pending.clear(); };
    worker.onmessage = ({ data }) => {
      if (data.kind === 'snapshot') {
        const values = [...new Float64Array(data.buffer)];
        samples.push({ values, receivedAt: now() });
        worker.postMessage({ kind: 'recycle', epoch: data.epoch, buffer: data.buffer }, [data.buffer]);
      } else {
        pending.get(data.id)?.resolve(data); pending.delete(data.id);
      }
    };
    try {
      const commands = replayCommands(), direct = movementFixture();
      for (const command of commands) direct.step(command);
      const replay = await rpc('replay', { commands });
      const started = await rpc('start', { epoch: 1 });
      await wait(100);
      const stallStart = now();
      while (now() - stallStart < 150) { /* intentional main-thread hitch */ }
      const stallEnd = now();
      const input = await rpc('input', { epoch: 1, seq: 1, command: { forward: 0 } });
      const stale = await rpc('input', { epoch: 0, seq: 2, command: { forward: -1 } });
      const duplicate = await rpc('input', { epoch: 1, seq: 1, command: { forward: -1 } });
      await wait(100);
      const stopped = await rpc('stop');
      const transport = [];
      for (const actors of [44, 128, 256]) for (const transfer of [false, true]) {
        let buffer = new ArrayBuffer(actors * 16 * 4);
        const times = [];
        for (let i = 0; i < 80; i++) {
          new Float32Array(buffer)[0] = i;
          const t = performance.now();
          const result = await rpc('echo', { buffer, transfer }, transfer ? [buffer] : []);
          if (transfer && buffer.byteLength !== 0) throw new Error('transfer did not detach sender');
          buffer = result.buffer;
          if (new Float32Array(buffer)[0] !== i) throw new Error('corrupt transport');
          if (i >= 10) times.push(performance.now() - t);
        }
        times.sort((a, b) => a - b);
        transport.push({ actors, bytes: buffer.byteLength, transfer, samples: times.length,
          p50RoundTripMs: times[Math.floor(times.length / 2)], p95RoundTripMs: times[Math.ceil(times.length * .95) - 1] });
      }
      return {
        direct: direct.snapshot(), worker: replay.state,
        isolated: crossOriginIsolated, sharedBuffer: typeof SharedArrayBuffer !== 'undefined',
        stall: { start: stallStart, end: stallEnd, durationMs: stallEnd - stallStart,
          workerTicksDuringStall: stopped.tickTimes.filter(time => time >= stallStart && time <= stallEnd).length,
          snapshotsGeneratedDuringStall: samples.filter(sample => sample.values[3] >= stallStart && sample.values[3] <= stallEnd).length,
          inputReceivedAfterStallMs: input.receivedAt - stallEnd },
        started, stopped, stale, duplicate, transport,
        snapshotSequence: samples.map(sample => sample.values[1]),
      };
    } finally { worker.terminate(); }
  });
  assert.deepEqual(report.worker, report.direct, 'same movement code and commands produce identical state');
  assert.ok(report.stall.workerTicksDuringStall > 0, 'worker must continue while main is stalled');
  assert.ok(report.stopped.skipped > 0, 'snapshot pool should exert backpressure during stalled consumer');
  assert.ok(report.stopped.poolAvailable <= 3, 'bounded snapshot buffers');
  assert.equal(report.stale.accepted, false, 'old epoch input rejected');
  assert.equal(report.duplicate.accepted, false, 'duplicate sequence rejected');
  assert.ok(report.snapshotSequence.every((tick, i, all) => i === 0 || tick > all[i - 1]), 'monotonic snapshot ticks');
  assert.deepEqual(errors, []);
  report.errors = errors;
  const out = process.env.RUNTIME_WORKER_OUTPUT || '/tmp/bf-runtime-worker.json';
  await fs.writeFile(out, JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ pass: true, identicalMovement: true, stall: report.stall,
    skippedSnapshots: report.stopped.skipped, isolated: report.isolated, transport: report.transport, out }));
} finally { await browser.close(); }
