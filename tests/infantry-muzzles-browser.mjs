// Verify actual rendered streak tails against the posed weapon fire point.
// The battle runs in the world worker, whose actor pose must never supply the cosmetic muzzle.
import assert from 'node:assert/strict';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  {
    const page = await browser.newPage({ viewport: { width: 320, height: 180 } }), errors = [];
    page.on('pageerror', e => errors.push(e.message));
    page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
    await page.route('**/@vite/client', r => r.fulfill({ contentType: 'application/javascript', body: '' }));
    await page.addInitScript(() => localStorage.setItem('battlefront-eternal.settings', JSON.stringify({ waterQuality: 'low', antialiasing: 'off', shadows: 'off', renderScale: .5 })));
    await page.goto(`${process.env.RUNTIME_AUDIT_URL || 'http://127.0.0.1:18449'}/?map=nab2&side=cis&unit=cis_inf_battledroid&ai=classic&seed=1`);
    await page.waitForFunction(() => window.__bf?.client?.ready, null, { timeout: 180000 });
    const result = await page.evaluate(async () => {
      const b = __bf, o = b.ordnance, T = b.THREE, sleep = ms => new Promise(r => setTimeout(r, ms));
      b.graphics.render = () => {}; b.controls.locked = true;
      const seen = new WeakSet(), rows = [], draw = o.drawBolt, span = o.boltSpan;
      let stage = null;
      o.boltSpan = function (...args) { const length = span.apply(this, args); this.testSpan = length; return length; };
      o.drawBolt = function (bolt, alpha) {
        draw.call(this, bolt, alpha);
        if (seen.has(bolt)) return;
        seen.add(bolt);
        const unit = bolt.owner, combat = unit?.combat;
        if (!combat || !bolt.weapon || stage === null || bolt.muzzleGeneration !== unit.generation) return;
        const at = combat.roundMuzzle(bolt);
        if (!at) return;
        const tail = this.drawnAt.clone().addScaledVector(new T.Vector3(...this.boltAxis), -this.testSpan);
        rows.push({ stage, human: unit.human, type: unit.className, error: tail.distanceTo(at) });
      };
      for (const first of [false, true]) {
        stage = null; b.controls.clear(); b.setPerspective(first);
        await sleep(1000); stage = first;
        b.controls.hold('Mouse0');
        const end = performance.now() + 30000;
        while (rows.filter(row => row.stage === first && row.human).length < 5 && performance.now() < end) await sleep(25);
      }
      b.controls.clear();
      return rows;
    });
    assert.deepEqual(errors, []);
    for (const first of [false, true]) {
      const rows = result.filter(row => row.stage === first && row.human);
      assert.ok(rows.length >= 5, `firstPerson=${first}: enough SBD shots`);
    }
    for (const row of result) assert.ok(row.error < 1e-5, JSON.stringify(row));
    console.log(JSON.stringify({ shots: result.length, bots: result.filter(row => !row.human).length, maxTailError: Math.max(...result.map(row => row.error)) }));
    await page.close();
  }
} finally { await browser.close(); }
