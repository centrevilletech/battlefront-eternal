// Real worker presentation: roster names, held launcher and missile rotation,
// plus local movement prediction and pause/resume without transport errors.
import assert from 'node:assert/strict';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  const page = await browser.newPage({ viewport: { width: 320, height: 180 } }), errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('crash', () => errors.push('browser page crashed'));
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
  await page.route('**/@vite/client', route => route.fulfill({ contentType: 'application/javascript', body: '' }));
  await page.addInitScript(() => localStorage.setItem('battlefront-eternal.settings', JSON.stringify({ waterQuality: 'low', antialiasing: 'off', shadows: 'off', renderScale: 0.5 })));
  await page.goto(`${process.env.RUNTIME_AUDIT_URL || 'http://127.0.0.1:18449'}/?map=nab2&unit=rep_inf_arc_trooper&ai=classic&seed=1`);
  await page.waitForFunction(() => window.__bf?.client?.ready, null, { timeout: 180000 });
  await page.evaluate(() => { __bf.graphics.render = () => {}; __bf.controls.locked = true; });
  // Spawns follow simulation time, which can run slower than wall time on
  // a loaded software-rendering test host.
  await page.waitForFunction(() => __bf.units.all().some(unit => !unit.human && unit.alive && unit.soldier), null, { timeout: 180000 });
  const result = await page.evaluate(async () => {
    const b = __bf, c = b.client, sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
    b.graphics.render = () => {};
    b.controls.locked = true;
    let named = 0, shown = false, predicted = false, missiles = 0, badRotation = false;
    b.controls.hold('KeyW', 'Mouse0');
    b.controls.looks.push([0, -100]);
    for (let i = 0; i < 240; i++) {
      if (i % 60 === 0) b.controls.press('Mouse0'); // launcher is single-trigger
      await sleep(25);
      named = Math.max(named, b.units.all().filter(unit => !unit.human && unit.alive && unit.character?.name).length);
      shown ||= Boolean(b.combat.shown?.def.GeometryName && [...b.combat.held.values()].some(held => held.group.visible));
      predicted ||= c.predicted && b.soldier.position.distanceToSquared(c.saved.get('position')) > 1e-8;
      for (const round of b.ordnance.rounds) {
        if (!round.mesh) continue;
        missiles++;
        badRotation ||= !round.quaternion?.isQuaternion || !round.mesh.quaternion.equals(round.quaternion);
      }
    }
    b.controls.clear();
    b.pause();
    const restored = !c.predicted;
    await c.host.call('suspend');
    await sleep(100);
    b.resume();
    await c.host.call('start');
    await sleep(500);
    return { named, shown, predicted, missiles, badRotation, restored, tick: b.worldClock.tick };
  });
  console.log(JSON.stringify(result));
  assert.deepEqual(errors, []);
  assert.ok(result.named > 0, 'live bots have roster names');
  assert.ok(result.shown, 'launcher is held and visible');
  assert.ok(result.predicted, 'movement is predicted ahead of acknowledged state');
  assert.ok(result.missiles > 0, 'fired missiles were displayed');
  assert.equal(result.badRotation, false, 'missile meshes use the received quaternion');
  assert.equal(result.restored, true, 'pause restores authoritative state');
  console.log(JSON.stringify({ pass: true, ...result }));
} finally { await browser.close(); }
