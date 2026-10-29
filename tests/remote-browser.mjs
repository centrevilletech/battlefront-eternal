import assert from 'node:assert/strict';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  for (const [side, unit] of [['rep','rep_inf_clone_sharpshooter'], ['cis','cis_inf_assassindroid'], ['all','all_inf_marksman'], ['imp','imp_inf_scout_trooper']]) {
    const page = await browser.newPage({ viewport: { width: 640, height: 360 } });
    console.log('loading', unit);
    page.on('crash', () => console.error('renderer crashed', unit));
    const errors = []; page.on('pageerror', e => { errors.push(e.message); console.error(e.message); });
    await page.route('**/@vite/client', r => r.fulfill({ contentType: 'application/javascript', body: '' }));
    await page.addInitScript(() => localStorage.setItem('battlefront-eternal.settings', JSON.stringify({ shadows: 'off', antialiasing: 'off', destructionEffects: 'enhanced', waterQuality: 'low', treeDetail: 'original', environmentEffects: 'classic', upscaler: 'off', bloom: false, softFocus: false, textureCompression: false, audioQuality: 'original' })));
    await page.goto(`${process.env.REMOTE_URL || 'http://127.0.0.1:18449'}/?map=nab2&side=${side}&unit=${unit}&ai=off&seed=42`);
    await page.waitForFunction(() => window.__bf?.client?.ready, null, { timeout: 180000 });
    console.log('worker ready', unit);
    await page.evaluate(async () => { await __bf.ready; __bf.begin(); __bf.controls.locked = true; });
    await page.waitForFunction(() => __bf.client.running && __bf.client.latest > 10);
    await page.evaluate(() => { __bf.pending.nextSecondary = true; });
    await page.waitForTimeout(200);
    await page.evaluate(() => { __bf.pending.fireSecondary = true; });
    // Inspect the active remote in the same page task that observes it.
    const cameraHandle = await page.waitForFunction(() => {
      if (!__bf.remotes.piloted) return false;
      const b = __bf, d = b.remotes.piloted;
      b.setPerspective(false); b.remotes.updateCamera(1, false);
      const chase = b.camera.position.distanceTo(d.position);
      b.setPerspective(true); b.remotes.updateCamera(1, true); b.remotes.present(0, true, true);
      const eye = b.camera.position.distanceTo(d.eye.clone().addScaledVector(d.aimDir, -.3));
      const hidden = !d.mesh.visible;
      b.setPerspective(false);
      b._reconEffects = {}; b._reconShots = new Set(); b._reconDown = true; b._reconImpacts = 0;
      const draw = b.ordnance.drawBolt.bind(b.ordnance);
      b.ordnance.drawBolt = (bolt, ...args) => { if (bolt.def.class.includes('orbital')) { b._reconShots.add(bolt); b._reconDown &&= bolt.direction.y < -.9; } return draw(bolt, ...args); };
      const play = b.effects.play.bind(b.effects);
      b.effects.play = (name, ...args) => { b._reconEffects[name] = (b._reconEffects[name] || 0) + 1; if (name === 'atat_explosion') b._reconImpacts++; return play(name, ...args); };
      b._reconBlasts = b.destruction.blasts;
      b.pending.fireSecondary = true;
      return { chase, eye, hidden, view: Boolean(b.remotes.view) };
    }, null, { timeout: 15000 });
    const camera = await cameraHandle.jsonValue();
    assert.ok(camera.view && camera.chase < 5 && camera.eye < 1e-5 && camera.hidden, JSON.stringify(camera));
    try { await page.waitForFunction(() => !__bf.remotes.active && __bf._reconImpacts >= 4, null, { timeout: 30000 }); } catch (error) { console.log('orbital timeout', await page.evaluate(() => ({ tick: __bf.worldClock.tick, active: __bf.remotes.active, effects: __bf._reconEffects, bolts: __bf._reconShots.size, impacts: __bf._reconImpacts, blasts: __bf.destruction.blasts - __bf._reconBlasts }))); throw error; }
    const result = await page.evaluate(() => ({ bolts: __bf._reconShots.size, down: __bf._reconDown, impacts: __bf._reconImpacts, enhanced: __bf.destruction.blasts - __bf._reconBlasts }));
    assert.ok(result.bolts > 0 && result.bolts <= 4, "descending bolts reached the renderer"); assert.ok(result.down); assert.equal(result.impacts, 4); assert.ok(result.enhanced >= 5);
    assert.deepEqual(errors, []);
    console.log(unit, camera, result);
    await page.close();
  }
} finally { await browser.close(); }
