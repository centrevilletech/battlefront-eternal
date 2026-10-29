// PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs GAME_URL=http://127.0.0.1:18449 node tests/fusion-cutter-browser.mjs
// A pilot works a turret pad with the fusion cutter in third and first
// person on the live page (the world in its worker, stepped in lockstep).
// While it ticks the cutter is the model in the hand (its first-person model
// in the arms), each tick's repairkit sparks play, the arms play the tool's
// shoot clip, and the body holds the alert stance; let go, the primary comes
// back. CUTTER_UNITS picks the classes (default: each side's pilot on nab2,
// which has pads for both teams); CUTTER_OUTPUT is where the frames go.
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { launchBrowser, openBattle } from './battle-page.mjs';

const base = process.env.GAME_URL || 'http://127.0.0.1:18449';
const folder = process.env.CUTTER_OUTPUT || '/tmp/bf/fusion-cutter';
const units = process.env.CUTTER_UNITS?.split(',') || ['rep_inf_clone_pilot', 'cis_inf_pilotdroid'];
mkdirSync(folder, { recursive: true });

const browser = await launchBrowser();
const deadline = setTimeout(() => { console.error('Fusion cutter check timed out'); process.exit(1); }, units.length * 240000);
const failures = [], summary = [];
try {
  for (const unit of units) {
    const battle = await openBattle(browser, { base, query: `map=nab2&side=${unit.slice(0, 3)}&unit=${unit}&seed=1`, viewport: { width: 800, height: 450 } });
    const { page, world, errors } = battle;
    await battle.frames(30);
    const id = await page.evaluate(() => __bf.playerUnit.id);
    // Beside the first pad his cutter can build, aimed at it.
    const pad = await world.evaluate((id) => {
      const g = self.__bf, u = g.units.byId(id), c = u.combat, fixer = c.arsenal.weapons.find((w) => w.repair);
      const building = fixer && g.live.buildings.find((b) => g.live.repairs(fixer.def, b, c.team));
      if (!building) return null;
      const center = new g.THREE.Vector3();
      building.sphere(center);
      u.soldier.spawn(center.x - 2.2, center.y + 1, center.z, Math.PI / 2);
      u.presentation?.reset?.();
      u.tickCount += 1;
      return { center: center.toArray(), channel: fixer.channel, model: fixer.def.GeometryName, arms: fixer.def.HighResGeometry || fixer.def.GeometryName };
    }, id);
    if (!pad) { failures.push(`${unit}: no repair weapon or pad`); await battle.close(); continue; }
    const aim = () => world.evaluate(([c, id]) => {
      const s = self.__bf.units.byId(id).soldier, dx = c[0] - s.position.x, dz = c[2] - s.position.z;
      s.aimYaw = Math.atan2(dx, dz);
      s.aimPitch = Math.atan2(c[1] - (s.position.y + 1.5), Math.hypot(dx, dz));
    }, [pad.center, id]);
    await aim();
    await page.evaluate(() => {
      const b = __bf, play = b.effects.play.bind(b.effects), v = b.firstPersonView, arms = v.play.bind(v);
      b.__cutter = { sparks: 0, arms: [] };
      b.effects.play = (name, ...rest) => { if (name === 'repairkit') b.__cutter.sparks++; return play(name, ...rest); };
      v.play = (event, weapon) => { b.__cutter.arms.push([event, weapon?.name]); return arms(event, weapon); };
    });
    const state = () => page.evaluate(() => {
      const b = __bf, c = b.combat, v = b.firstPersonView;
      return { shown: c.shown?.name ?? null, held: [...c.held].filter(([, h]) => h.group.visible).map(([n]) => n),
        arms: c.firstPerson ? [...v.weapons].filter(([, w]) => w.group.visible).map(([n]) => n) : null,
        bank: b.animator.bank, alert: b.animator.alert, clip: c.firstPerson ? v.clip?.name ?? null : null,
        sparks: b.__cutter.sparks, plays: b.__cutter.arms.splice(0) };
    });
    const frame = async (label) => {
      const url = await page.evaluate(async () => { await __bf.lockstep(1 / 60, true); return __bf.renderer.domElement.toDataURL('image/jpeg', 0.9); });
      writeFileSync(`${folder}/${unit}-${label}.jpg`, Buffer.from(url.split(',')[1], 'base64'));
    };
    const report = { unit };
    for (const first of [false, true]) {
      const view = first ? 'fp' : 'tp';
      await page.evaluate((first) => { __bf.setPerspective(first); __bf.presentation.reset(); }, first);
      await battle.frames(40);
      await page.evaluate(() => { __bf.__cutter.sparks = 0; __bf.__cutter.arms.length = 0; });
      // The secondary trigger held (the cutter is the pilots' first secondary).
      await page.evaluate(() => { __bf.controls.press('Mouse2'); __bf.controls.hold('Mouse2'); });
      for (let i = 0; i < 4; i++) { await aim(); await battle.frames(15); }
      const working = await state();
      // A frame just after a tick, to see its sparks.
      for (let i = 0; i < 60; i++) {
        const before = await page.evaluate(() => __bf.__cutter.sparks);
        await battle.frames(1);
        if (await page.evaluate((n) => __bf.__cutter.sparks > n, before)) { await battle.frames(2); break; }
      }
      await frame(`${view}-repairing`);
      await page.evaluate(() => __bf.controls.down.delete('Mouse2'));
      await battle.frames(90);
      const after = await state();
      report[view] = { working, after };
      const where = `${unit} ${view}`;
      if (!working.sparks) failures.push(`${where}: no repairkit sparks`);
      if (!working.held.includes(pad.model)) failures.push(`${where}: ${pad.model} not in the hand (${working.held})`);
      if (working.bank !== 'tool') failures.push(`${where}: body bank ${working.bank}, not tool`);
      if (!(working.alert > 0)) failures.push(`${where}: body not alert while repairing`);
      if (first && !working.arms?.includes(pad.arms)) failures.push(`${where}: ${pad.arms} not in the arms (${working.arms})`);
      if (first && !working.plays.some(([event, weapon]) => event === 'shoot' && weapon === working.shown)) failures.push(`${where}: the arms played no tool shoot`);
      if (after.held.includes(pad.model)) failures.push(`${where}: the cutter stayed in the hand after letting go`);
    }
    if (errors.length) failures.push(`${unit}: page errors ${JSON.stringify(errors.slice(0, 3))}`);
    summary.push(report);
    await battle.close();
  }
} finally { clearTimeout(deadline); await browser.close(); }
console.log(JSON.stringify({ folder, summary }, null, 1));
if (failures.length) console.error(failures.join('\n'));
assert.deepEqual(failures, []);
