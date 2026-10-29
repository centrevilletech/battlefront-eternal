// The opt-in runtime profiler (profileRuntime=1) changes nothing in the
// battle and counts the page's frames and input. The world worker is stepped
// by the page's frames (tests/battle-page.mjs), so its ticks cost the page's
// frames nothing: their cost reaches the page in their packets.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { launchBrowser, openBattle } from './battle-page.mjs';
const browser = await launchBrowser();
const base = process.env.RUNTIME_AUDIT_URL || 'http://127.0.0.1:18449';
const results = [];
try {
  for (const enabled of [false, true]) {
    const battle = await openBattle(browser, {
      base, query: `map=nab2&unit=rep_inf_clone_trooper&ai=classic&rate=60&seed=1&profileRuntime=${Number(enabled)}`,
      beforeLoad: page => page.addInitScript(() => {
        let seed = 47291;
        Math.random = () => (seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 4294967296;
      }),
    });
    const { page, world, errors } = battle;
    const result = await page.evaluate(async () => {
      const b = __bf;
      for (let i = 0; i < 120; i++) await b.lockstep(1 / 60, false);
      b.runtimeProfile?.reset();
      window.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyW' }));
      b.controls.locked = true;
      for (let i = 0; i < 60; i++) {
        document.dispatchEvent(new MouseEvent('mousemove', { movementX: 1, movementY: 0 }));
        await b.lockstep(1 / 60, false);
      }
      window.dispatchEvent(new KeyboardEvent('keyup', { code: 'KeyW' }));
      await b.lockstep(0, true);
      return { profile: b.runtimeProfile?.report() ?? null, stepMs: b.client.health?.stepMs ?? 0 };
    });
    // The battle as it stands, where it runs.
    const state = await world.evaluate(() => {
      const b = self.__bf;
      return {
        position: b.soldier.position.toArray(), yaw: b.soldier.aimYaw,
        units: b.units.all().filter(unit => unit.soldier).map(unit => [unit.id, unit.generation, ...unit.position.toArray(), unit.health]),
      };
    });
    assert.deepEqual(errors, []);
    results.push({ ...state, ...result });
    await battle.close();
  }
  assert.equal(results[0].profile, null, 'opt in only');
  const { profile, stepMs, ...onState } = results[1], { profile: ignored, stepMs: ignoredMs, ...offState } = results[0];
  assert.deepEqual(onState, offState, 'instrumentation must not alter gameplay state');
  assert.equal(profile.frames, 61);
  assert.equal(profile.metrics.inputCount.total, 62, 'keydown, mouse events, keyup observed');
  // The ticks run in the world worker, outside the page's frames: their
  // cost is the worker's, from its packets.
  assert.ok(stepMs > 0, 'the world\'s tick cost reaches the page');
  assert.ok(profile.metrics.render.total > 0);
  assert.equal(profile.pendingInputs, 0);
  await fs.writeFile('/tmp/bf-runtime-profiler-browser.json', JSON.stringify({ pass: true, identicalState: true, stepMs, profile }, null, 2));
  console.log(JSON.stringify({ pass: true, identicalState: true, inputCount: profile.metrics.inputCount.total, frames: profile.frames, stepMs }));
} finally { await browser.close(); }
