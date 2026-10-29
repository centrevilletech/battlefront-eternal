// Recon droids in the drawn game: the page as a player has it (its world
// worker on its own clock, the page predicting the flown droid ahead of it,
// rendering, HUD and sound), not the lockstep of tests/battle-page.mjs. Each
// battle throws, flies, shoots, re-enters, strikes, destructs, loses and
// times out droids, kills their owner and respawns him as other classes;
// every page error, console error, frame failure and world worker failure
// is reported.
//
//   node tests/recon-stress-browser.mjs   (Vite on BFE_URL, default :18449)
//   BFE_RECON_BATTLES=nab2:rep,hot1:imp node tests/recon-stress-browser.mjs
//
// Set PLAYWRIGHT_MODULE when Playwright is installed elsewhere.
import { launchBrowser, LOW_SETTINGS } from './battle-page.mjs';

const RECON = { rep: 'rep_inf_clone_sharpshooter', cis: 'cis_inf_assassindroid', all: 'all_inf_marksman', imp: 'imp_inf_scout_trooper' };
const base = process.env.BFE_URL || 'http://127.0.0.1:18449';
const battles = (process.env.BFE_RECON_BATTLES || 'nab2:rep').split(',').map((entry) => entry.split(':'));
// Small by default: under software GL a big frame takes long enough for the
// world to count the page's input as stale and hand control of the droid back.
const viewport = { width: Number(process.env.BFE_RECON_WIDTH || 640), height: Number(process.env.BFE_RECON_HEIGHT || 360) };
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const browser = await launchBrowser();
const failures = [];

async function battle(map, side) {
  const unit = RECON[side];
  const era = ['all', 'imp'].includes(side) ? '&era=gcw' : '';
  const errors = [];
  const keep = (kind) => (text) => { errors.push(`${kind}: ${text}`); console.error(`  ${map} ${side} ${kind}: ${text.slice(0, 300)}`); };
  const page = await browser.newPage({ viewport });
  page.on('pageerror', (e) => keep('pageerror')(e.stack || e.message));
  page.on('console', (m) => { if (m.type() === 'error') keep('console')(m.text()); });
  page.on('crash', () => keep('crash')('renderer crashed'));
  page.on('worker', (worker) => worker.on?.('console', (m) => { if (m.type() === 'error') keep('worker')(m.text()); }));
  await page.route('**/@vite/client', (r) => r.fulfill({ contentType: 'application/javascript', body: '' }));
  await page.addInitScript((settings) => localStorage.setItem('battlefront-eternal.settings', JSON.stringify(settings)),
    { ...LOW_SETTINGS, destructionEffects: 'enhanced', upscaler: 'off', bloom: false, softFocus: false });
  const started = Date.now();
  await page.goto(`${base}/?map=${map}&side=${side}&unit=${unit}&ai=classic&seed=11${era}`, { timeout: 600000 });
  await page.waitForFunction(() => window.__bf?.client?.ready || window.__bf?.client?.host?.closed, null, { timeout: 900000, polling: 1000 });
  await page.evaluate(async () => { await __bf.ready; __bf.begin(); __bf.controls.locked = true; });
  const world = page.workers().find((w) => w.url().includes('local-server-worker'));
  console.log(`${map} ${side}: loaded in ${Math.round((Date.now() - started) / 1000)} s`);

  // The page's state, and anything that ended or broke the battle.
  const state = () => page.evaluate(() => {
    const b = __bf, d = b.remotes.piloted, w = b.combat.arsenal.weapons.find((x) => x.def.droid);
    return { tick: b.worldClock.tick, piloted: Boolean(d), droids: b.remotes.droids.length, dead: b.combat.dead, className: b.playerUnit.className,
      selected: b.combat.arsenal.current(1)?.def.droid ? 'remote' : b.combat.arsenal.current(1)?.name, remote: Boolean(w),
      closed: Boolean(b.client.host.closed), status: b.client.host.statusText || null, predicted: b.client.predicted,
      hud: [...document.querySelectorAll('#hud *')].map((e) => e.textContent).filter((t) => /error|failed/i.test(t)).slice(0, 3) };
  });
  const check = async (label) => {
    const s = await state();
    if (s.closed || s.hud.length) { keep('battle')(`${label}: ${s.status || s.hud.join(' | ')}`); return false; }
    return s;
  };
  const press = (name) => page.evaluate((name) => { __bf.pending[name] = true; }, name);
  const keys = (down, up = []) => page.evaluate(([down, up]) => { for (const k of up) __bf.keys.delete(k); for (const k of down) __bf.keys.add(k); }, [down, up]);
  const look = (dx, dy) => page.evaluate(([dx, dy]) => __bf.controls.looks.push([dx, dy]), [dx, dy]);
  const until = async (fn, ms, arg) => { const end = Date.now() + ms; for (;;) { if (await page.evaluate(fn, arg)) return true; if (Date.now() > end) return false; await sleep(150); } };
  const inWorld = (fn, arg) => world.evaluate(fn, arg);
  // A tap of a held key: down long enough for a command or two to carry it.
  const tap = async (code, ms = 250) => { await keys([code]); await sleep(ms); await keys([], [code]); };
  const select = async () => {
    for (let i = 0; i < 6; i++) {
      if (await page.evaluate(() => __bf.combat.arsenal.current(1)?.def.droid)) return true;
      const before = await page.evaluate(() => __bf.combat.arsenal.current(1)?.name);
      await press('nextSecondary');
      await until((before) => __bf.combat.arsenal.current(1)?.name !== before, 4000, before);
    }
    return false;
  };
  const refill = () => inWorld(() => { const w = self.__server.actor.unit.combat.arsenal.weapons.find((x) => x.def.droid); if (w) w.reserve = Math.max(w.reserve, 1); });
  const launch = async () => {
    if (await page.evaluate(() => __bf.combat.dead)) await until(() => !__bf.combat.dead, 9000);
    if (!(await select())) return false;
    await until(() => { const w = __bf.combat.arsenal.weapons.find((x) => x.def.droid); return w && !w.busy && w.canShoot; }, 6000);
    for (let attempt = 0; attempt < 3; attempt++) {
      await press('fireSecondary'); await tap('Mouse2', 300);
      if (await until(() => Boolean(__bf.remotes.piloted), 6000)) return true;
    }
    return false;
  };
  const fly = async (ms, { shoot = true } = {}) => {
    const end = Date.now() + ms; let t = 0;
    while (Date.now() < end) {
      await keys(['KeyW', t % 2 ? 'KeyA' : 'KeyD'], ['KeyA', 'KeyD']);
      if (shoot) await keys(['Mouse0']);
      await look(Math.sin(t) * 60, Math.cos(t) * 20); await sleep(300);
      if (shoot) await keys([], ['Mouse0']);
      t++;
    }
    await keys([], ['KeyW', 'KeyA', 'KeyD', 'Mouse0']);
  };
  const killOwner = () => inWorld(() => { const u = self.__server.actor.unit; if (u.alive) u.combat.damage(Infinity); });
  const respawn = async (className) => {
    await inWorld(() => { self.__server.actor.autoRespawn = false; });
    await until(() => __bf.combat.dead, 5000);
    await sleep(4500);
    const post = await page.evaluate(() => __bf.spawnPort.posts(__bf.playerTeam).find((p) => p.eligible)?.name ?? null);
    await page.evaluate(([className, post]) => __bf.controls.human.push({ kind: 'spawn', className, post }), [className, post]);
    return until(() => !__bf.combat.dead, 8000);
  };
  const steps = [];
  const step = async (label, fn) => {
    if (failures.some((f) => f.page === page)) return;
    let outcome;
    try { outcome = await fn(); } catch (error) { outcome = `threw ${error.message}`; keep('harness')(`${label}: ${error.message}`); }
    const s = await check(label);
    steps.push(`${label}: ${outcome}`);
    console.log(`  ${label}: ${outcome}${s ? ` (tick ${s.tick}, ${s.className})` : ''}`);
    if (!s) failures.push({ page, map, side, label });
  };

  await sleep(1500);
  await step('launch and fly, blaster firing', async () => (await launch()) ? (await fly(6000), 'flown') : 'no launch');
  await step('first person and zoom while flying', async () => {
    if (!(await page.evaluate(() => Boolean(__bf.remotes.piloted)))) { await refill(); if (!(await launch())) return 'no launch'; }
    await page.evaluate(() => __bf.setPerspective(true)); await fly(1500);
    await press('zoom'); await fly(1500); await press('zoom');
    await page.evaluate(() => __bf.setPerspective(false)); return 'ok';
  });
  await step('exit, walk, re-enter', async () => {
    await press('use'); await sleep(600);
    await keys(['KeyW']); await sleep(1000); await keys([], ['KeyW']);
    await press('fireSecondary'); await tap('Mouse2');
    return (await until(() => Boolean(__bf.remotes.piloted), 5000)) ? 're-entered' : 'no re-entry';
  });
  await step('orbital strike', async () => {
    await press('fireSecondary'); await tap('Mouse2');
    return (await until(() => !__bf.remotes.piloted && !__bf.remotes.droids.some((d) => !d.dead && d.countdown >= 0), 12000)) ? 'struck' : 'still out';
  });
  await step('relaunch as soon as allowed', async () => {
    if (await page.evaluate(() => __bf.combat.dead)) await until(() => !__bf.combat.dead, 9000);
    await refill(); return (await launch()) ? (await fly(1500), 'out') : 'no launch';
  });
  await step('owner killed while flying', async () => {
    await killOwner(); await sleep(500);
    return (await respawn(RECON[side])) ? 'respawned' : 'not respawned';
  });
  await step('destruct with reload, out of ammo', async () => {
    await refill(); if (!(await launch())) return 'no launch';
    await press('use'); await sleep(500); await press('reload'); await sleep(800);
    return (await launch()) ? 'launched with no ammo?' : 'empty';
  });
  await step('class change while a droid flies on', async () => {
    await refill(); const out = await launch(); await fly(800, { shoot: false });
    await killOwner();
    const other = await page.evaluate(() => __bf.spawnPort.roster(__bf.playerTeam).map((c) => c.className).find((c) => c !== __bf.playerUnit.className));
    const spawned = await respawn(other); await sleep(2000);
    const back = await killOwner().then(() => respawn(RECON[side]));
    return `${out ? 'out' : 'none'}, as ${other} ${spawned}, back ${back}`;
  });
  await step('out of bounds and into water', async () => {
    const out = [];
    for (const place of ['far', 'water']) {
      await refill(); if (!(await launch())) { out.push(`${place}: no launch`); continue; }
      const moved = await inWorld((place) => {
        const u = self.__server.actor.unit, d = u.combat.remotes?.piloted, g = self.__bf; if (!d) return false;
        if (place === 'far') d.position.set(900, 60, 900);
        else if (Number.isFinite(g.water?.level)) d.position.set(u.position.x, g.water.level - 1, u.position.z);
        else return false;
        d.velocity.set(0, 0, 0); return true;
      }, place);
      await fly(moved ? 11500 : 500, { shoot: false });
      out.push(`${place}: ${moved ? 'moved' : 'skipped'}`);
      if (await page.evaluate(() => __bf.combat.dead)) await respawn(RECON[side]);
      else { await press('use'); await sleep(300); await press('reload'); await sleep(500); }
    }
    return out.join(', ');
  });
  await step('left to time out', async () => {
    await refill(); if (!(await launch())) return 'no launch';
    await press('use'); await sleep(41000);
    return (await page.evaluate(() => __bf.remotes.droids.some((d) => !d.dead))) ? 'still out' : 'timed out';
  });
  const predicted = await page.evaluate(() => __bf.client.predicted);
  console.log(`${map} ${side}: ${steps.length} steps, ${errors.length} errors${predicted !== undefined ? '' : ''}`);
  if (errors.length) failures.push({ page, map, side, label: 'errors', errors });
  await page.close();
  return errors;
}

try {
  for (const [map, side] of battles) {
    try { await battle(map, side); } catch (error) { failures.push({ map, side, label: 'load', errors: [error.message] }); console.error(`${map} ${side}: ${error.message}`); }
  }
} finally { await browser.close(); }
console.log(JSON.stringify(failures.map(({ map, side, label, errors }) => ({ map, side, label, errors })), null, 1));
process.exit(failures.length ? 1 : 0);
