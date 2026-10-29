// Read-only runtime audit against an extracted game served by Vite.
// No drawing: these timings describe CPU updates, not hardware frame rate.
// The battle runs in the world worker, stepped in lockstep with the page's
// frames at each display cadence (game.lockstep): the world's systems are
// timed there, per tick, and the page's own (its presentation) per frame,
// whose total includes waiting for the frame's ticks.
// Usage: PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node tools/audit-runtime.mjs
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const base = process.env.RUNTIME_AUDIT_URL || 'http://127.0.0.1:18449';
const out = process.env.RUNTIME_AUDIT_OUTPUT || '/tmp/bf-runtime-audit.json';
const seconds = Number(process.env.RUNTIME_AUDIT_SECONDS || 12);
const warmupSeconds = Number(process.env.RUNTIME_AUDIT_WARMUP_SECONDS || 10);
const map = process.env.RUNTIME_AUDIT_MAP || 'nab2';
const cpuEffects = process.env.RUNTIME_AUDIT_CPU_EFFECTS === '1';
const cases = [{ hz: 30 }, { hz: 60 }, { hz: 144 }, { hz: 240 }, { hz: 'jitter' }]
  .filter(scenario => !process.env.RUNTIME_AUDIT_HZ || String(scenario.hz) === process.env.RUNTIME_AUDIT_HZ);
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
const results = [];
// Times every call of object[key] into metrics[label] (and the open row),
// on whichever side runs it; returns an undo.
const timing = `(metrics, rows) => (object, key, label) => {
  if (!object || typeof object[key] !== 'function') return () => {};
  const own = Object.hasOwn(object, key), original = object[key];
  object[key] = function (...args) {
    const start = performance.now();
    try { return original.apply(this, args); }
    finally {
      const ms = performance.now() - start;
      const record = metrics[label] ||= { calls: 0, totalMs: 0, maxCallMs: 0 };
      record.calls++; record.totalMs += ms; record.maxCallMs = Math.max(record.maxCallMs, ms);
      const row = rows.at(-1); if (row) row[label] = (row[label] || 0) + ms;
    }
  };
  return () => own ? object[key] = original : delete object[key];
}`;
const summary = `values => {
  const sorted = [...values].sort((a, b) => a - b);
  const at = q => sorted[Math.max(0, Math.ceil(q * sorted.length) - 1)] ?? 0;
  return { p50Ms: at(.5), p95Ms: at(.95), p99Ms: at(.99), maxMs: at(1) };
}`;
try {
  for (const scenario of cases) {
    const page = await browser.newPage({ viewport: { width: 640, height: 360 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
    await page.route('**/@vite/client', route => route.fulfill({ contentType: 'application/javascript', body: '' }));
    await page.addInitScript(() => {
      const raf = requestAnimationFrame.bind(window);
      window.requestAnimationFrame = callback => callback.name === 'loop' ? 0 : raf(callback);
      localStorage.setItem('battlefront-eternal.settings', JSON.stringify({
        waterQuality: 'low', treeDetail: 'original', environmentEffects: 'original',
        oceanEffects: 'original', rainEffects: 'original', antialiasing: 'off',
        jetpackEffects: 'original', droidekaEffects: 'original', shadows: 'off',
      }));
      let seed = 47291;
      Math.random = () => (seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 4294967296;
    });
    console.log(`Audit ${map}: render=${scenario.hz}`);
    await page.goto(`${base}/?map=${map}&unit=rep_inf_clone_trooper&ai=classic&tier=medium&heroes=0&seed=1`);
    await page.waitForFunction(() => window.__bf?.client?.ready || document.querySelector('#hud')?.textContent.includes('Failed to load'), null, { timeout: 120000 });
    assert.equal(await page.evaluate(() => Boolean(window.__bf?.client?.ready)), true, 'startup');
    const world = page.workers().find(worker => worker.url().includes('local-server-worker'));
    assert.ok(world, 'the world worker');
    await page.evaluate(async ({ warmupSeconds, cpuEffects }) => {
      const b = __bf;
      await Promise.race([b.ready, new Promise((_, reject) => setTimeout(() => reject(new Error('readiness timeout')), 60000))]);
      b.pause();
      if (cpuEffects) { b.effects.gpu?.dispose(); b.effects.gpu = null; }
      for (let i = 0; i < Math.round(warmupSeconds * 60); i++) await b.lockstep(1 / 60, false);
    }, { warmupSeconds, cpuEffects });
    // The world's systems, timed per tick in the worker.
    await world.evaluate(({ timing }) => {
      const g = self.__bf, metrics = {}, rows = [], wrap = (0, eval)(timing)(metrics, rows), undo = [];
      for (const [object, key, label] of [
        [g.ai, 'update', 'ai'], [g.playerUnit.soldier, 'tick', 'playerMovement'],
        [g.combat, 'update', 'playerCombat'], [g.ordnance, 'update', 'projectiles'],
        [g.posts, 'update', 'capture'], [g.battle, 'update', 'battle'],
        [g.vehicles, 'update', 'vehicles'], [g.live, 'update', 'services'], [g.items, 'update', 'items'],
        [g.factory, 'build', 'actorBuild'], [g.factory, 'acquire', 'actorAcquire'],
        [g.collision, 'cast', 'collisionCast'], [g.collision, 'gather', 'collisionGather'],
        [Object.getPrototypeOf(g.playerUnit), 'tick', 'botTick'],
      ]) undo.push(wrap(object, key, label));
      // A row per tick: the step (its packet is the worker's, after it).
      const step = g.step;
      g.step = function (...args) { rows.push({}); const start = performance.now(); try { return step.apply(this, args); } finally { rows.at(-1).tick = performance.now() - start; } };
      undo.push(() => { g.step = step; });
      self.__audit = { metrics, rows, undo, reader: g.simEvents.reader(), events: {}, startEntities: g.units.all().filter(unit => unit.alive && unit.soldier).map(unit => ({ id: unit.id, generation: unit.generation, position: unit.position.toArray(), health: unit.health })) };
    }, { timing });
    const pageResult = await page.evaluate(async ({ scenario, seconds, timing }) => {
      const b = __bf, rows = [], metrics = {}, wrap = (0, eval)(timing)(metrics, rows), undo = [];
      for (const [object, key, label] of [
        [b.foliage, 'update', 'foliage'], [b.effects, 'update', 'effects'], [b.audio, 'update', 'audio'],
        [Object.getPrototypeOf(b.playerUnit), 'present', 'botPresentation'],
      ]) undo.push(wrap(object, key, label));
      const deltas = scenario.hz === 'jitter' ? [1 / 120, 1 / 120, 1 / 120, .075] : [1 / scenario.hz];
      let elapsed = 0, frame = 0;
      try {
        while (elapsed < seconds - 1e-9) {
          const dt = Math.min(deltas[frame % deltas.length], seconds - elapsed);
          const row = { dt };
          rows.push(row);
          const start = performance.now();
          await b.lockstep(dt, false);
          row.total = performance.now() - start;
          elapsed += dt; frame++;
        }
      } finally { for (const restore of undo.reverse()) restore(); }
      return {
        rows, metrics,
        inputQueue: (() => {
          b.controls.clear(); b.controls.press('Mouse0'); b.controls.press('Mouse0');
          return { first: b.controls.take('firePrimary'), second: b.controls.take('firePrimary') };
        })(),
      };
    }, { scenario, seconds, timing });
    const worldResult = await world.evaluate(() => {
      const g = self.__bf, audit = self.__audit;
      audit.reader.read(event => audit.events[event.kind] = (audit.events[event.kind] || 0) + 1);
      for (const restore of audit.undo.reverse()) restore();
      return { metrics: audit.metrics, rows: audit.rows, events: audit.events, eventDrops: audit.reader.dropped, startEntities: audit.startEntities,
        endEntities: g.units.all().filter(unit => unit.alive && unit.soldier).map(unit => ({ id: unit.id, generation: unit.generation, position: unit.position.toArray(), health: unit.health })),
        posts: g.posts.posts.map(post => post.team), ticks: g.worldClock.tick };
    });
    const summarize = (0, eval)(summary);
    for (const [key, metric] of Object.entries(pageResult.metrics)) Object.assign(metric, summarize(pageResult.rows.map(row => row[key] || 0)));
    for (const [key, metric] of Object.entries(worldResult.metrics)) Object.assign(metric, summarize(worldResult.rows.map(row => row[key] || 0)));
    const result = {
      scenario, seconds, frames: pageResult.rows.length, ticks: worldResult.rows.length,
      timing: summarize(pageResult.rows.map(row => row.total)), tickTiming: summarize(worldResult.rows.map(row => row.tick)),
      metrics: pageResult.metrics, worldMetrics: worldResult.metrics,
      startEntities: worldResult.startEntities, endEntities: worldResult.endEntities, events: worldResult.events, eventDrops: worldResult.eventDrops,
      posts: worldResult.posts, inputQueue: pageResult.inputQueue,
    };
    assert.deepEqual(errors, [], 'browser errors');
    assert.equal(result.eventDrops, 0, 'event journal overflow');
    assert.equal(result.ticks, Math.round(seconds * 60), 'a tick per sixtieth of a second, whatever the cadence');
    results.push({ ...result, errors });
    await fs.writeFile(out, JSON.stringify({ map, warmupSeconds, cpuEffects, draw: false, timingNote: 'Instrumented inclusive CPU durations; nested groups overlap; frame totals include waiting for the world worker; not GPU/FPS measurements.', results }, null, 2));
    console.log(JSON.stringify({ scenario, timing: result.timing, tickTiming: result.tickTiming, aiSteps: result.worldMetrics.ai?.calls, playerWeaponSteps: result.worldMetrics.playerCombat?.calls, projectileSteps: result.worldMetrics.projectiles?.calls, events: result.events }));
    await page.close();
  }
} finally { await browser.close(); }
