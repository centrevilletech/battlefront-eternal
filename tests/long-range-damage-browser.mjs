// Exercise the real game damage hooks and retail jet-trooper rocket data.
import assert from 'node:assert/strict';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  const page = await browser.newPage(), errors = [];
  page.on('pageerror', e => errors.push(e.message));
  await page.goto(`${process.env.RUNTIME_AUDIT_URL || 'http://127.0.0.1:18449'}/src/runtime/commands.js`);
  const results = await page.evaluate(async () => {
    const { createGame } = await import('/src/game.js');
    const g = await createGame({ headless: true, query: '?map=nab2&side=rep&unit=rep_inf_jet_trooper&ai=off&seed=1' });
    await g.factory.prepare('cis_inf_droideka');
    const target = g.factory.build('cis_inf_droideka'), T = g.THREE, results = [];
    target.team = target.combat.team = 2;
    const def = g.combat.arsenal.current(0).def.ordnance;
    for (const shielded of [false, true]) for (const distance of [10, 100, 250]) {
      target.spawn(0, 200, distance, 0);
      target.soldier.position.set(0, 200, distance); target.tickCount++;
      target.soldier.shield.on = shielded; target.soldier.shield.value = 4000;
      const origin = new T.Vector3(0, target.sphere.center.y, 0);
      const direction = target.sphere.center.clone().sub(origin).normalize();
      const hits = [];
      for (let shot = 0; shot < 8 && !target.combat.dead; shot++) {
        g.ordnance.fire(def, origin, direction, g.playerUnit);
        for (let tick = 0; tick < 600 && g.ordnance.rounds.length; tick++) g.ordnance.update(1 / 60, false);
        hits.push({ health: target.combat.health, shield: target.soldier.shield.value });
      }
      results.push({ shielded, distance, dead: target.combat.dead, hits });
    }
    return results;
  });
  assert.deepEqual(errors, []);
  for (const shielded of [false, true]) {
    const cases = results.filter(r => r.shielded === shielded);
    for (const result of cases) {
      assert.ok(result.dead, `target dies at ${result.distance} m (shield=${shielded})`);
      assert.equal(result.hits.length, cases[0].hits.length, 'range must not change the number of hits');
      result.hits.forEach((hit, i) => {
        assert.ok(Math.abs(hit.health - cases[0].hits[i].health) < 1e-6);
        assert.ok(Math.abs(hit.shield - cases[0].hits[i].shield) < 1e-6);
      });
    }
  }
  console.log(JSON.stringify({ pass: true, results }));
} finally { await browser.close(); }
