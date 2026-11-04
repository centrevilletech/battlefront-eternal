// Real worker presentation: roster names, held launcher and missile rotation,
// plus local movement prediction and pause/resume without transport errors.
import assert from 'node:assert/strict';
import { exercisePredictionPage } from './fixtures/runtime-prediction-page.js';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const native = process.env.BFE_PREDICTION === 'rust';
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  const page = await browser.newPage({ viewport: { width: 320, height: 180 } }), errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('crash', () => errors.push('browser page crashed'));
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
  await page.route('**/@vite/client', route => route.fulfill({ contentType: 'application/javascript', body: '' }));
  await page.addInitScript(() => localStorage.setItem('battlefront-eternal.settings', JSON.stringify({ waterQuality: 'low', antialiasing: 'off', shadows: 'off', renderScale: 0.5 })));
  await page.goto(`${process.env.RUNTIME_AUDIT_URL || 'http://localhost:18449'}/?map=nab2&unit=rep_inf_arc_trooper&ai=classic&seed=1&prediction=${native ? 'rust' : 'js'}`);
  await page.waitForFunction(() => window.__bf?.client?.ready, null, { timeout: 180000 });
  await page.evaluate(() => { __bf.graphics.render = () => {}; __bf.controls.locked = true; });
  // Spawns follow simulation time, which can run slower than wall time on
  // a loaded software-rendering test host.
  await page.waitForFunction(() => __bf.units.all().some(unit => !unit.human && unit.alive && unit.soldier), null, { timeout: 180000 });
  const result = await page.evaluate(exercisePredictionPage, native);
  console.log(JSON.stringify(result));
  assert.deepEqual(errors, []);
  assert.ok(result.named > 0, 'live bots have roster names');
  assert.ok(result.shown, 'launcher is held and visible');
  assert.ok(result.predicted, 'movement is predicted ahead of acknowledged state');
  assert.ok(result.missiles > 0, 'fired missiles were displayed');
  assert.equal(result.badRotation, false, 'missile meshes use the received quaternion');
  assert.equal(result.restored, true, 'pause restores authoritative state');
  assert.equal(result.hasOwner, native, 'page switch selects the private native owner');
  if (native) {
    assert.ok(result.nativeActorSteps > 0, 'ordinary page replay executes native actor steps');
    assert.ok(result.nativeWeapon, 'ordinary page replay uses a native weapon session');
    assert.ok(result.correctionCalls > 0, 'received corrections use the native reader');
    assert.ok(result.reconcileCalls > 0, 'page timing and windows use the native reconciliation owner');
    assert.equal(result.shared, true, 'prediction shares the page collision instance');
  }
  assert.equal(result.disposed, true, 'connection disposal stops replay and releases native owners');
  assert.equal(result.closedDuringReplay, true, 'teardown occurs inside an active replay callback');
  console.log(JSON.stringify({ pass: true, ...result }));
} finally { await browser.close(); }
