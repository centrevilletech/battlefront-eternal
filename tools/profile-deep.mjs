// Deep main-thread profile of a full battle: per-frame phase timings (the
// game's own runtime profiler), a V8 CPU sampling profile, allocation sampling,
// GC pauses from a trace, and WebGL calls per frame. Frames are driven at a
// fixed 60 Hz cadence (one world tick each) so a slow software GPU cannot add
// catch-up ticks; timings are CPU work on this host, not the player's FPS.
// The ticks run in the world worker in lockstep with the frames
// (game.lockstep): the page's profile shows its own work and the wait for
// each tick. Profile the world itself under Node (tools/headless-world.mjs
// with node --cpu-prof).
//
// Usage: PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node tools/profile-deep.mjs
//   PROFILE_MAP=nab2 PROFILE_WARMUP=25 PROFILE_FRAMES=600 PROFILE_DRAW=1
//   PROFILE_OUTPUT=/tmp/bf-profile  PROFILE_SETTINGS='{"shadows":"high"}'
import fs from 'node:fs/promises';
import { createServer } from 'vite';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const map = process.env.PROFILE_MAP || 'nab2';
const warmup = Number(process.env.PROFILE_WARMUP || 25);
const frames = Number(process.env.PROFILE_FRAMES || 600);
const draw = process.env.PROFILE_DRAW !== '0';
const out = process.env.PROFILE_OUTPUT || '/tmp/bf-profile';
const port = Number(process.env.PROFILE_PORT || 18474);
const extraSettings = JSON.parse(process.env.PROFILE_SETTINGS || '{}');
const query = process.env.PROFILE_QUERY || `map=${map}&unit=rep_inf_clone_trooper&ai=classic&tier=medium&heroes=0&seed=1&profileRuntime=1`;
await fs.mkdir(out, { recursive: true });

const server = process.env.PROFILE_URL ? null : await createServer({
  root: process.cwd(), configFile: false, cacheDir: `/tmp/bf-profile-vite-${port}`, logLevel: 'error',
  optimizeDeps: { entries: ['index.html'] }, server: { watch: null, hmr: false, port, host: '127.0.0.1', strictPort: true },
});
if (server) await server.listen();
const base = process.env.PROFILE_URL || `http://127.0.0.1:${port}`;
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--enable-precise-memory-info',
  '--disable-renderer-backgrounding', '--disable-background-timer-throttling'] });

const percentile = (values, q) => { const s = [...values].sort((a, b) => a - b); return s.length ? s[Math.min(s.length - 1, Math.max(0, Math.ceil(q * s.length) - 1))] : 0; };
const stats = (values) => ({ n: values.length, p50: +percentile(values, .5).toFixed(2), p95: +percentile(values, .95).toFixed(2), p99: +percentile(values, .99).toFixed(2), max: +Math.max(0, ...values).toFixed(2), mean: +(values.reduce((a, b) => a + b, 0) / Math.max(1, values.length)).toFixed(2) });

async function open(countGl) {
  const page = await browser.newPage({ viewport: { width: 640, height: 360 } });
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
  await page.addInitScript(({ countGl, settings }) => {
    const raf = requestAnimationFrame.bind(window);
    window.requestAnimationFrame = f => f.name === 'loop' ? 0 : raf(f);
    localStorage.setItem('battlefront-eternal.settings', JSON.stringify(settings));
    if (countGl) {
      // Count every WebGL call per frame (wrapping costs time; separate run).
      window.__gl = { counts: Object.create(null) };
      for (const proto of [WebGL2RenderingContext.prototype]) for (const name of Object.getOwnPropertyNames(proto)) {
        const d = Object.getOwnPropertyDescriptor(proto, name);
        if (!d || typeof d.value !== 'function' || name === 'constructor') continue;
        const original = d.value;
        proto[name] = function (...args) { window.__gl.counts[name] = (window.__gl.counts[name] || 0) + 1; return original.apply(this, args); };
      }
    }
  }, { countGl, settings: extraSettings });
  await page.goto(`${base}/?${query}`, { waitUntil: 'domcontentloaded', timeout: 180000 });
  await page.waitForFunction(() => window.__bf?.frame || document.querySelector('#hud')?.textContent.includes('Failed'), null, { timeout: 180000 });
  await page.evaluate(async (warmup) => {
    const b = __bf;
    await Promise.race([b.ready, new Promise(r => setTimeout(r, 60000))]);
    b.pause();
    // Let the battle fill: simulated seconds without drawing.
    for (let i = 0; i < Math.round(warmup * 60); i++) await b.lockstep(1 / 60, false);
    // Controls as if pointer-locked, for injected mouse look.
    b.controls.locked = true; b.controls.enabled = true;
  }, warmup);
  return { page, errors };
}

// One frame per animation callback, exactly one world tick, with walking,
// strafing, turning and firing so the controls path is exercised too.
const drive = (page, count, draw) => page.evaluate(async ({ count, draw }) => {
  const b = __bf, rows = [];
  const key = (code, down) => window.dispatchEvent(new KeyboardEvent(down ? 'keydown' : 'keyup', { code, key: code, bubbles: true }));
  const look = (x, y) => document.dispatchEvent(new MouseEvent('mousemove', { movementX: x, movementY: y, bubbles: true }));
  const units = () => b.units.all().filter(u => u.alive && u.soldier).length;
  let last = performance.now();
  key('KeyW', true);
  for (let i = 0; i < count; i++) {
    await new Promise(requestAnimationFrame);
    if (i % 120 === 0) key('KeyA', true); if (i % 120 === 60) key('KeyA', false);
    if (i % 90 === 0) window.dispatchEvent(new MouseEvent('mousedown', { button: 0, bubbles: true }));
    if (i % 90 === 20) window.dispatchEvent(new MouseEvent('mouseup', { button: 0, bubbles: true }));
    look(Math.round(6 * Math.sin(i * 0.05)), Math.round(2 * Math.cos(i * 0.03)));
    const tick = b.worldClock.tick, calls0 = b.renderer.info.render.calls;
    const start = performance.now();
    await b.lockstep(1 / 60, draw);
    const end = performance.now();
    rows.push({ cpu: end - start, gap: start - last, ticks: b.worldClock.tick - tick, calls: b.renderer.info.render.calls,
      triangles: b.renderer.info.render.triangles, units: i % 30 === 0 ? units() : undefined,
      heap: performance.memory?.usedJSHeapSize });
    last = start;
  }
  key('KeyW', false);
  const report = b.runtimeProfile?.report?.();
  return { rows, report, gpu: b.graphics.profiler?.report?.() };
}, { count, draw });

try {
  // Pass 1: timing with CPU profile, allocation sampling and GC trace.
  const { page, errors } = await open(false);
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Profiler.enable');
  await cdp.send('Profiler.setSamplingInterval', { interval: 100 });
  await cdp.send('HeapProfiler.enable');
  const traceEvents = [];
  cdp.on('Tracing.dataCollected', e => traceEvents.push(...e.value));
  const traceDone = new Promise(r => cdp.once('Tracing.tracingComplete', r));
  await cdp.send('Tracing.start', { categories: 'devtools.timeline,v8,v8.gc,disabled-by-default-v8.gc,blink.user_timing', transferMode: 'ReportEvents' });
  await page.evaluate(() => { __bf.runtimeProfile?.reset?.(); performance.mark('profile-start'); });
  await cdp.send('HeapProfiler.startSampling', { samplingInterval: 16384, includeObjectsCollectedByMajorGC: true, includeObjectsCollectedByMinorGC: true });
  await cdp.send('Profiler.start');
  const t0 = Date.now();
  const result = await drive(page, frames, draw);
  const { profile } = await cdp.send('Profiler.stop');
  const { profile: heap } = await cdp.send('HeapProfiler.stopSampling');
  await cdp.send('Tracing.end'); await traceDone;
  console.log(`measured ${frames} frames in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
  await fs.writeFile(`${out}/cpu.cpuprofile`, JSON.stringify(profile));
  await fs.writeFile(`${out}/heap.json`, JSON.stringify(heap));
  await fs.writeFile(`${out}/frames.json`, JSON.stringify(result));

  // CPU: self time by function and by file.
  const byId = new Map(profile.nodes.map(n => [n.id, n]));
  const self = new Map(), files = new Map();
  const dts = profile.timeDeltas;
  for (let i = 0; i < profile.samples.length; i++) {
    const node = byId.get(profile.samples[i]), cf = node.callFrame, ms = (dts[i] || 0) / 1000;
    const file = (cf.url || cf.functionName || '(native)').replace(/^.*\/(src|node_modules)\//, '$1/').replace(/\?.*$/, '');
    const keyName = `${cf.functionName || '(anonymous)'} ${file}:${cf.lineNumber + 1}`;
    self.set(keyName, (self.get(keyName) || 0) + ms);
    files.set(file, (files.get(file) || 0) + ms);
  }
  const total = [...self.values()].reduce((a, b) => a + b, 0);
  const top = [...self].sort((a, b) => b[1] - a[1]).slice(0, 45).map(([k, v]) => `${v.toFixed(0).padStart(7)} ms ${(100 * v / total).toFixed(1).padStart(5)}%  ${k}`);
  const topFiles = [...files].sort((a, b) => b[1] - a[1]).slice(0, 30).map(([k, v]) => `${v.toFixed(0).padStart(7)} ms ${(100 * v / total).toFixed(1).padStart(5)}%  ${k}`);

  // Allocation sampling: bytes by allocating function.
  const alloc = new Map();
  const walk = (node) => {
    const cf = node.callFrame, file = (cf.url || '').replace(/^.*\/(src|node_modules)\//, '$1/').replace(/\?.*$/, '');
    const k = `${cf.functionName || '(anonymous)'} ${file}:${cf.lineNumber + 1}`;
    alloc.set(k, (alloc.get(k) || 0) + node.selfSize);
    for (const c of node.children || []) walk(c);
  };
  walk(heap.head);
  const allocTotal = [...alloc.values()].reduce((a, b) => a + b, 0);
  const topAlloc = [...alloc].sort((a, b) => b[1] - a[1]).slice(0, 30).map(([k, v]) => `${(v / 1048576).toFixed(1).padStart(7)} MB ${(100 * v / allocTotal).toFixed(1).padStart(5)}%  ${k}`);

  // GC pauses from the trace.
  const gc = traceEvents.filter(e => /^(MinorGC|MajorGC|V8\.GC_SCAVENGER|V8\.GC_MARK_COMPACTOR|V8\.GCFinalizeMC|V8\.GCIncrementalMarking)$/.test(e.name) && e.ph === 'X');
  const gcByName = {};
  for (const e of gc) { const r = gcByName[e.name] ||= { count: 0, totalMs: 0, maxMs: 0 }; r.count++; r.totalMs += e.dur / 1000; r.maxMs = Math.max(r.maxMs, e.dur / 1000); }

  const rows = result.rows.slice(30);
  const summary = {
    map, draw, frames: rows.length, errors,
    frameCpu: stats(rows.map(r => r.cpu)),
    ticksPerFrame: stats(rows.map(r => r.ticks)),
    drawCalls: stats(rows.map(r => r.calls)), triangles: stats(rows.map(r => r.triangles)),
    units: rows.map(r => r.units).filter(Boolean),
    heapMB: stats(rows.map(r => r.heap / 1048576).filter(Number.isFinite)),
    gc: gcByName,
    allocMBPerSecond: +(allocTotal / 1048576 / (rows.length / 60)).toFixed(1),
    runtimeProfile: result.report,
  };
  await fs.writeFile(`${out}/summary.json`, JSON.stringify(summary, null, 2));
  const slow = [...result.rows.keys()].sort((a, b) => result.rows[b].cpu - result.rows[a].cpu).slice(0, 12).map(i => ({ frame: i, ...result.rows[i] }));
  console.log(JSON.stringify({ ...summary, runtimeProfile: undefined }, null, 1));
  console.log('\nSlowest frames:', JSON.stringify(slow.map(s => [s.frame, +s.cpu.toFixed(1), s.ticks, s.calls])));
  console.log('\nTop self time:\n' + top.join('\n'));
  console.log('\nBy file:\n' + topFiles.join('\n'));
  console.log(`\nTop allocations (${(allocTotal / 1048576).toFixed(0)} MB sampled):\n` + topAlloc.join('\n'));
  if (result.report) await fs.writeFile(`${out}/runtime-report.json`, JSON.stringify(result.report, null, 2));
  await page.close();

  // Pass 2: WebGL calls per frame.
  if (draw && process.env.PROFILE_GL !== '0') {
    const { page: glPage } = await open(true);
    const counts = await glPage.evaluate(async () => {
      const b = __bf;
      for (let i = 0; i < 30; i++) { await new Promise(requestAnimationFrame); await b.lockstep(1 / 60, true); }
      window.__gl.counts = Object.create(null);
      const n = 120;
      for (let i = 0; i < n; i++) { await new Promise(requestAnimationFrame); await b.lockstep(1 / 60, true); }
      return Object.fromEntries(Object.entries(window.__gl.counts).map(([k, v]) => [k, +(v / n).toFixed(1)]).sort((a, b) => b[1] - a[1]));
    });
    await fs.writeFile(`${out}/gl-calls.json`, JSON.stringify(counts, null, 2));
    const totalCalls = Object.values(counts).reduce((a, b) => a + b, 0);
    console.log(`\nWebGL calls per frame: ${totalCalls.toFixed(0)}\n` + Object.entries(counts).slice(0, 30).map(([k, v]) => `${String(v).padStart(8)}  ${k}`).join('\n'));
    await glPage.close();
  }
} finally {
  await browser.close();
  await server?.close();
}
