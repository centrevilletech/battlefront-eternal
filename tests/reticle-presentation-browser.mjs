// The reticle on the aim as drawn. Mouse counts go to the world (in its
// worker) with the ticks the frames cross (tests/battle-page.mjs), and every
// frame's reticle sits on the aim that frame draws while the mouse moves it.
import assert from 'node:assert/strict';
import { launchBrowser, openBattle } from './battle-page.mjs';
const browser = await launchBrowser();
try {
  const battle = await openBattle(browser, { base: process.env.RUNTIME_AUDIT_URL || 'http://127.0.0.1:18449', query: 'map=nab2&side=rep&unit=rep_inf_jet_trooper&ai=off&seed=1',
    viewport: { width: 320, height: 180 }, settings: { waterQuality: 'low', antialiasing: 'off', shadows: 'off', renderScale: .5 } });
  const { page, errors } = battle;
  const result = await page.evaluate(async () => {
    const b = __bf;
    b.controls.locked = true;
    const results = [];
    for (const first of [false, true]) {
      b.setPerspective(first);
      for (let i = 0; i < 40; i++) await b.lockstep(1 / 60, false);
      let maxError = 0, legacyError = 0, low = Infinity, high = -Infinity;
      for (let i = 0; i < 120; i++) {
        document.dispatchEvent(new MouseEvent('mousemove', { movementX: 2, movementY: i < 60 ? 2 : -2, bubbles: true }));
        await b.lockstep(1 / 144, false);
        if (!b.hud.reticle) throw Error(`no reticle ${first ? 'in first' : 'in third'} person, frame ${i} (dead ${b.combat.dead})`);
        const pose = b.presentation.state, aim = b.combat.viewBasis(pose);
        const expected = aim.origin.clone().addScaledVector(aim.aimDir, 1024).project(b.camera);
        const y = (1 - expected.y) / 2;
        maxError = Math.max(maxError, Math.abs(b.hud.reticle.y - y));
        low = Math.min(low, pose.aimPitch); high = Math.max(high, pose.aimPitch);
        // The newest tick's aim against the same frame (for the record: with
        // no look previewed, the drawn pitch is the tick's).
        b.combat.updateHud();
        legacyError = Math.max(legacyError, Math.abs(b.hud.reticle.y - y));
        b.combat.updateHud(null, pose);
      }
      results.push({ first, maxError, legacyError, pitchRange: high - low });
    }
    return results;
  });
  console.log(JSON.stringify({ result }));
  assert.deepEqual(errors, []);
  for (const row of result) {
    assert.ok(row.maxError < 1e-8, JSON.stringify(row));
    assert.ok(row.pitchRange > .1, 'probe covers a moving drawn aim');
  }
  console.log(JSON.stringify({ pass: true }));
} finally { await browser.close(); }
