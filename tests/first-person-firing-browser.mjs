// PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs FIRST_PERSON_URL=http://127.0.0.1:18449 node tests/first-person-firing-browser.mjs
// Each class fires its primary weapons in first person on the live page: the
// world in its worker, his own rounds predicted on the page as in play. The
// arms play the shoot clip for every round with the weapon in the hand and in
// view; only a weapon switch lowers them (docs/formats.md, "First person").
// FIRST_PERSON_UNITS=all checks every class of nab2's two battles in place
// of each side's first; FIRST_PERSON_OUTPUT is where the frames go.
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { launchBrowser, LOW_SETTINGS } from './battle-page.mjs';

const base = process.env.FIRST_PERSON_URL || 'http://127.0.0.1:18449';
const folder = process.env.FIRST_PERSON_OUTPUT || '/tmp/bf/first-person-firing';
const FIRST = ['rep_inf_clone_trooper', 'cis_inf_battledroid', 'imp_inf_storm_trooper', 'all_inf_soldierurban'];
const EVERY = [
  'rep_inf_clone_trooper', 'rep_inf_arc_trooper', 'rep_inf_clone_pilot', 'rep_inf_clone_sharpshooter', 'rep_inf_jet_trooper',
  'cis_inf_battledroid', 'cis_inf_assault', 'cis_inf_pilotdroid', 'cis_inf_assassindroid', 'cis_inf_droideka',
  'imp_inf_storm_trooper', 'imp_inf_shock_trooper', 'imp_inf_pilotatst', 'imp_inf_scout_trooper', 'imp_inf_dark_trooper',
  'all_inf_soldierurban', 'all_inf_vanguard', 'all_inf_pilot', 'all_inf_marksman', 'all_inf_smuggler',
];
const units = process.env.FIRST_PERSON_UNITS === 'all' ? EVERY : process.env.FIRST_PERSON_UNITS?.split(',') || FIRST;
mkdirSync(folder, { recursive: true });

const browser = await launchBrowser();
const deadline = setTimeout(() => { console.error('First-person firing check timed out'); process.exit(1); }, units.length * 240000);
const failures = [], summary = [];
try {
  for (const unit of units) {
    const page = await browser.newPage({ viewport: { width: 640, height: 360 } }), errors = [];
    page.on('pageerror', (e) => { errors.push(e.message); console.error(unit, e.message); });
    page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
    await page.route('**/@vite/client', (r) => r.fulfill({ contentType: 'application/javascript', body: '' }));
    await page.addInitScript((s) => localStorage.setItem('battlefront-eternal.settings', JSON.stringify(s)), LOW_SETTINGS);
    await page.goto(`${base}/?map=nab2&side=${unit.slice(0, 3)}&unit=${unit}&seed=1`, { timeout: 180000 });
    await page.waitForFunction(() => window.__bf?.client?.ready, null, { timeout: 180000 });
    const result = await page.evaluate(async () => {
      const b = __bf, T = b.THREE, sleep = (ms) => new Promise((r) => setTimeout(r, ms));
      const until = async (test, ms = 8000) => { const end = performance.now() + ms; while (!test() && performance.now() < end) await sleep(20); return test(); };
      // The battle holds still until the loading screen hands over.
      await until(() => b.worldClock.tick > 60 && !b.combat.dead && b.firstPersonView, 120000);
      const v = b.firstPersonView, c = b.combat;
      if (!v) return { skipped: 'no first-person arms' };
      // Each frame the arms are drawn: the clip, the weapon in the hand and
      // where hp_weapons is on screen; each round they're told of.
      const rows = [], plays = [], images = {};
      let phase = 'raise', want = null;
      const update = v.update, play = v.play;
      v.update = function (dt, args) {
        update.call(this, dt, args);
        if (!args.active) return;
        const at = this.hand.getWorldPosition(new T.Vector3()).project(args.camera);
        rows.push({ phase, tick: b.worldClock.tick, slot: this.slot, clip: this.clip?.name ?? null, holding: this.holding?.name ?? null, shown: args.shown?.name ?? null,
          held: [...this.weapons].filter(([, w]) => w.group.visible).map(([name]) => name), x: at.x, y: at.y, drawn: this.root.visible });
      };
      v.play = function (event, weapon) {
        plays.push({ phase, event, weapon: weapon?.name ?? null, shownObject: weapon === c.shown });
        return play.call(this, event, weapon);
      };
      const render = b.graphics.render.bind(b.graphics);
      b.graphics.render = (...args) => {
        render(...args);
        if (want) { images[want] = b.renderer.domElement.toDataURL('image/jpeg', 0.85); want = null; }
      };
      const capture = (label) => { want = label; return until(() => !want, 5000); };
      // A trigger pull: the press (queued, so a click shorter than a frame
      // fires) and the hold.
      const pull = () => { b.controls.press('Mouse0'); b.controls.hold('Mouse0'); };
      const letGo = () => b.controls.down.delete('Mouse0');
      // Raised and still: idle, the blend done (the page may draw slowly).
      const settled = () => until(() => v.slot === 'idle' && v.blend === 0 && !v.forced && v.holding?.name === c.shown?.name, 20000);
      b.controls.locked = true;
      b.setPerspective(true);
      await settled();
      const weapons = [];
      for (let n = 0; n < c.arsenal.channels[0].length; n++) {
        const weapon = c.arsenal.current(0);
        if (n) {
          phase = `switch${n}`;
          b.controls.queued.add('nextPrimary');
          await until(() => c.shown?.name !== weapons.at(-1).name, 4000);
          await settled();
        }
        const name = c.shown?.name ?? weapon?.name, d = c.shown?.def ?? weapon?.def;
        weapons.push({ name, model: d?.HighResGeometry || d?.GeometryName || null, single: d?.TriggerSingle > 0, charge: (d?.MaxPressedTime || 0) > 0 });
        await capture(`${n}-idle`);
        phase = `fire${n}`;
        // Held fire for an automatic weapon, else a pull every half second
        // (a charging weapon fires on letting go), until eight rounds have
        // reached the arms: under load the world may run below real time.
        const rounds = () => plays.filter((p) => p.phase === phase).length;
        const auto = !weapons.at(-1).single && !weapons.at(-1).charge, end = performance.now() + 15000;
        if (auto) {
          pull();
          await until(() => rounds() >= 3, 15000); await capture(`${n}-firing`);
          await until(() => rounds() >= 8, end - performance.now()); letGo();
        } else {
          while (rounds() < 8 && performance.now() < end) {
            pull(); await sleep(weapons.at(-1).charge ? 400 : 80); letGo();
            if (rounds() >= 2 && !images[`${n}-firing`]) { await sleep(60); await capture(`${n}-firing`); }
            await sleep(450);
          }
        }
        phase = `after${n}`;
        await sleep(400); await capture(`${n}-after`);
        await sleep(600);
      }
      b.controls.clear();
      v.update = update; v.play = play; b.graphics.render = render;
      return { rows, plays, images, weapons, hand: Boolean(v.byName.hp_weapons) };
    });
    if (result.skipped) { summary.push({ unit, skipped: result.skipped }); await page.close(); continue; }
    for (const [label, url] of Object.entries(result.images)) writeFileSync(`${folder}/${unit}-${label}.jpg`, Buffer.from(url.split(',')[1], 'base64'));
    delete result.images;
    writeFileSync(`${folder}/${unit}.json`, JSON.stringify({ ...result, errors }, null, 1));
    const report = { unit, errors: errors.length, weapons: [] };
    result.weapons.forEach((weapon, n) => {
      const firing = result.rows.filter((row) => row.phase === `fire${n}`);
      const shots = result.plays.filter((p) => p.phase === `fire${n}` && p.event === 'shoot');
      // From the first round on, the arms hold the weapon up.
      const first = firing.findIndex((row) => row.slot === 'shoot');
      const after = first < 0 ? [] : firing.slice(first);
      const lowered = after.filter((row) => row.slot === 'handsdown').length;
      const empty = after.filter((row) => weapon.model && !row.held.includes(weapon.model)).length;
      // The grip stays about where it idles (the shoot clips move it by at
      // most about 1 on screen, a bazooka's the most; lowered, it is over
      // 10 away, inside the near plane). The droideka's arms have no
      // hp_weapons: their guns are their own. Reload clips deliberately move
      // the launcher down to load the next round, then blend back to idle.
      // Check position during the shoot clip, not during that transition.
      const start = result.rows.findIndex((row) => row.phase === `fire${n}`), rest = start > 0 ? result.rows[start - 1] : null;
      const off = result.hand && rest ? after.filter((row) => row.slot === 'shoot' && Math.hypot(row.x - rest.x, row.y - rest.y) > 2).length : 0;
      const shooting = after.filter((row) => row.slot === 'shoot').length;
      const line = { weapon: weapon.name, model: weapon.model, frames: firing.length, rounds: shots.length,
        predictedObjects: shots.filter((p) => !p.shownObject).length, lowered, empty, moved: off, shootFrames: shooting };
      report.weapons.push(line);
      const where = `${unit} ${weapon.name}`;
      if (!shots.length) failures.push(`${where}: no round reached the arms`);
      if (first < 0 && shots.length) failures.push(`${where}: the shoot clip never played`);
      if (lowered) failures.push(`${where}: arms lowered on ${lowered} of ${after.length} firing frames`);
      if (empty) failures.push(`${where}: ${weapon.model} not in the hand on ${empty} of ${after.length} firing frames`);
      if (off) failures.push(`${where}: the weapon left its place on ${off} of ${after.length} firing frames`);
    });
    if (errors.length) failures.push(`${unit}: page errors ${JSON.stringify(errors.slice(0, 3))}`);
    summary.push(report);
    await page.close();
  }
} finally { clearTimeout(deadline); await browser.close(); }
console.log(JSON.stringify({ folder, summary }, null, 1));
if (failures.length) console.error(failures.join('\n'));
assert.deepEqual(failures, []);
