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
      const seen = new WeakSet(), rows = [], hand = o.draw.bolt.bind(o.draw), finish = o.draw.finish.bind(o.draw);
      let stage = null, handed = [];
      // The bolts a frame draws are handed to the ordnance phase and drawn by
      // one call: when a frame draws a single bolt, new to the page, every
      // streak of the frame is that bolt's, and its tail is read off the
      // quads (or capsules) the batches received.
      o.draw.bolt = function (bolt, ...rest) { handed.push(bolt); return hand(bolt, ...rest); };
      const tails = () => [...o.renderer.batches.values()].flatMap((batch) => {
        const a = batch.arrays, out = [];
        for (let i = 0; i < batch.count; i++) out.push(a.iTail
          ? new T.Vector3(a.iTail[i * 3], a.iTail[i * 3 + 1], a.iTail[i * 3 + 2])
          : new T.Vector3(a.iPosition[i * 3], a.iPosition[i * 3 + 1], a.iPosition[i * 3 + 2]).addScaledVector(new T.Vector3(a.iAxis[i * 3], a.iAxis[i * 3 + 1], a.iAxis[i * 3 + 2]), -a.iShape[i * 4 + 2]));
        return out;
      });
      o.draw.finish = function () {
        finish();
        const bolts = handed; handed = [];
        if (bolts.length !== 1 || seen.has(bolts[0])) return;
        const bolt = bolts[0];
        seen.add(bolt);
        const unit = bolt.owner, combat = unit?.combat;
        if (!combat || !bolt.weapon || stage === null || bolt.muzzleGeneration !== unit.generation) return;
        const at = combat.roundMuzzle(bolt);
        if (!at) return;
        for (const tail of tails()) rows.push({ stage, human: unit.human, type: unit.className, error: tail.distanceTo(at) });
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
