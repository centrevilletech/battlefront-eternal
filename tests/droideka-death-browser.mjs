// A droideka's death on the page, frame by frame, in Enhanced and Classic,
// on Theed, with the battle in the world worker.
//
// With Vite running (set PLAYWRIGHT_MODULE if Playwright is elsewhere):
//   node tests/droideka-death-browser.mjs
// Env: GAME_URL (http://127.0.0.1:18449), DROIDEKA_DEATH_OUTPUT (frames,
// default /tmp/bf-punch/droideka-death), DROIDEKA_DEATH_FRAMES=0 to skip the
// pictures, DROIDEKA_DEATH_REPORT=1 to only print the frames (no checks).
//
// A bot's droideka is killed on its feet and rolled, the world stepped in
// lockstep with the page's frames (tests/battle-page.mjs). Then a bot's and
// the player's own are killed with the world on its own clock and the page
// drawing what arrives, as in play (a lockstepped page never takes a
// droideka player's first correction, before this change too).
//
// It plays its death clip once, its body dissolves as it goes up (never in
// the bind pose) and is never drawn again in that life, though the world
// keeps sending its lingering body's records, summaries and corrections for
// 4 s. Its shield was drained first, so those records change every tick, as
// after a fight. Its pieces fly in both settings.
import { mkdirSync, writeFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { launchBrowser, openBattle } from './battle-page.mjs';

const URL = process.env.GAME_URL || 'http://127.0.0.1:18449';
const OUT = process.env.DROIDEKA_DEATH_OUTPUT || '/tmp/bf-punch/droideka-death';
const FRAMES = process.env.DROIDEKA_DEATH_FRAMES !== '0';
const REPORT = process.env.DROIDEKA_DEATH_REPORT === '1';
const SPOT = [-51.76, -50.04, -27.93]; // an open stretch of Theed's plaza
const SECONDS = 4.5;
const SHOTS = [0.3, 1.0, 1.3, 1.4, 1.5, 1.6, 1.75, 2.0, 2.6, 3.4];
mkdirSync(OUT, { recursive: true });

function worldHelpers() {
  const b = self.__bf;
  self.__dekaDeath = {
    setup(spot, rolled, own) {
      const human = self.__server.actor.unit;
      for (const u of b.factory.built) if (u !== b.playerUnit && u !== human && !u.removed) u.remove();
      if (own) {
        // His own droideka (the local human's unit).
        human.soldier.spawn(spot[0], spot[1], spot[2], 2.2);
        if (rolled) human.soldier.enter(11);
        this.unit = human;
        return human.id;
      }
      human.soldier.position.set(-20, -50, -10);
      const u = b.factory.acquire('cis_inf_droideka');
      u.team = 2; u.combat.team = 2;
      u.spawn(spot[0], spot[1], spot[2], 2.2);
      if (rolled) u.soldier.enter(11);
      this.unit = u;
      return u.id;
    },
    kill() {
      const u = this.unit;
      // Drained in a fight: its records change every tick while it refills.
      if (u.soldier.shield) u.soldier.shield.value = 0;
      u.combat.random = () => 0.99;
      u.combat.damage(100000, 100000, u.position.clone(), 'player');
    },
  };
}

function pageHelpers() {
  const b = __bf, T = b.THREE;
  const head = new T.Vector3();
  window.__dekaDeath = {
    look(spot) {
      b.camera.position.set(spot[0] + 3, spot[1] + 2, spot[2] + 6); b.camera.lookAt(spot[0], spot[1] + 1, spot[2]); b.camera.updateMatrixWorld();
      b.graphics.render(0);
      return new Promise((r) => setTimeout(r, 6000));
    },
    async prepare(spot) {
      for (let i = 0; i < 5; i++) await b.lockstep(1 / 60, false);
      await this.look(spot);
    },
    async settle(id, rolled, own) {
      for (let i = 0; i < (rolled ? 89 : 29); i++) await b.lockstep(1 / 60, false);
      return this.watch(id, own);
    },
    watch(id, own) {
      b.destruction?.clear(); b.debris.clear();
      this.unit = own ? b.playerUnit : b.units.byId(id);
      this.own = own;
      return this.unit.soldier.mode;
    },
    // What is drawn of it after the page's presentation.
    sample() {
      const u = this.unit, a = u.actor, s = u.soldier;
      const bone = a.byName.bone_head;
      bone.updateWorldMatrix(true, false);
      head.setFromMatrixPosition(bone.matrixWorld);
      const look = a.body.userData.heated;
      return {
        tick: b.worldClock.tick, visible: a.root.visible, removed: u.removed, dead: u.dead,
        mode: s.mode, time: +s.time.toFixed(3), shown: u.presentation.networkPose?.mode ?? null,
        head: +(head.y - u.presentation.position.y).toFixed(3), dissolve: look ? +look.dissolve.toFixed(3) : 0,
        classic: b.debris.pieces.length, pieces: b.destruction?.pieces.count ?? 0, gone: Boolean(u.gone),
      };
    },
    // One lockstepped frame.
    async frame(draw) {
      await b.lockstep(1 / 60, draw);
      return this.sample();
    },
    // The page on its own clock for `seconds`, the world on its own: every
    // frame's sample.
    async free(seconds) {
      b.resume();
      const rows = [], start = performance.now();
      let last = start;
      while (performance.now() - start < seconds * 1000) {
        await new Promise((r) => requestAnimationFrame(r));
        const now = performance.now(), dt = Math.min((now - last) / 1000, 0.1);
        last = now;
        b.frame(dt, false, dt);
        if (this.unit) rows.push({ t: +((now - start) / 1000).toFixed(3), ...this.sample() });
      }
      b.pause();
      return rows;
    },
    shot(spot) {
      b.camera.position.set(spot[0] + 3.6, spot[1] + 1.9, spot[2] + 3.9);
      b.camera.lookAt(spot[0], spot[1] + 0.8, spot[2]);
      b.camera.updateMatrixWorld();
      // His own body is drawn as it is; another's from outside his.
      const shown = b.actor.root.visible;
      if (!this.own) b.actor.root.visible = false;
      b.graphics.render(performance.now() / 1000);
      b.actor.root.visible = shown;
      return b.renderer.domElement.toDataURL('image/jpeg', 0.9);
    },
  };
}

async function open(browser, mode, own) {
  const battle = await openBattle(browser, { base: URL, query: own ? 'map=nab2&side=cis&unit=cis_inf_droideka&spawnscreen=1' : 'map=nab2&side=rep&unit=rep_inf_clone_trooper',
    viewport: { width: 960, height: 600 }, settings: { destructionEffects: mode, upscaler: 'off' } });
  await battle.page.evaluate(pageHelpers);
  await battle.world.evaluate(worldHelpers);
  await battle.world.evaluate(async () => { await self.__bf.factory.prepare('cis_inf_droideka'); });
  await battle.page.evaluate(async () => { await __bf.factory.prepare('cis_inf_droideka'); });
  return battle;
}

// Lockstepped: the world a tick per page frame, filmed at SHOTS.
async function lockstepped(browser, mode, rolled) {
  const battle = await open(browser, mode, false);
  const { page, world } = battle;
  const id = await world.evaluate(([spot, rolled]) => self.__dekaDeath.setup(spot, rolled, false), [SPOT, rolled]);
  await page.evaluate((spot) => __dekaDeath.prepare(spot), SPOT);
  const before = await page.evaluate(([id, rolled]) => __dekaDeath.settle(id, rolled, false), [id, rolled]);
  await world.evaluate(() => self.__dekaDeath.kill());
  const rows = [];
  const label = `${rolled ? 'rolled' : 'deployed'}-${mode}`;
  let shot = 0;
  for (let i = 0; i < SECONDS * 60; i++) {
    const t = (i + 1) / 60;
    const film = FRAMES && shot < SHOTS.length && t >= SHOTS[shot] - 1e-6;
    rows.push({ t: +t.toFixed(3), ...(await page.evaluate((draw) => __dekaDeath.frame(draw), film)) });
    if (film) {
      const url = await page.evaluate((spot) => __dekaDeath.shot(spot), SPOT);
      writeFileSync(`${OUT}/${label}-${String(Math.round(t * 1000)).padStart(5, '0')}ms.jpg`, Buffer.from(url.split(',')[1], 'base64'));
      shot++;
    }
  }
  const errors = battle.errors.slice();
  await battle.close();
  return { label, before, rows, errors, free: false };
}

// On their own clocks, as in play.
async function freeRunning(browser, mode, own) {
  const battle = await open(browser, mode, own);
  const { page, world } = battle;
  if (own) {
    await page.evaluate(() => __bf.begin());
    for (let attempt = 0; attempt < 60; attempt++) {
      await page.evaluate(() => __dekaDeath.free(.25));
      const ready = await page.evaluate(() => {
        const phase = __bf.spawnPort.humanState().phase;
        if (phase === 'alive') return true;
        if (phase === 'unspawned' || phase === 'awaiting') __bf.spawnPort.request({ className: 'cis_inf_droideka', post: null });
        return false;
      });
      if (ready) break;
    }
    assert.equal(await page.evaluate(() => __bf.spawnPort.humanState().phase), 'alive');
    await page.evaluate(() => __bf.setPerspective(false));
  }
  const id = await world.evaluate(([spot, own]) => self.__dekaDeath.setup(spot, false, own), [SPOT, own]);
  for (let attempt = 0; attempt < 30; attempt++) {
    await page.evaluate(() => __dekaDeath.free(.5));
    if (await page.evaluate(([id, own]) => Boolean(own ? __bf.playerUnit : __bf.units.byId(id)), [id, own])) break;
  }
  const before = await page.evaluate(([id, own]) => __dekaDeath.watch(id, own), [id, own]);
  await page.evaluate((spot) => __dekaDeath.look(spot), SPOT);
  await world.evaluate(() => self.__dekaDeath.kill());
  const rows = await page.evaluate((s) => __dekaDeath.free(s), SECONDS + 0.5);
  const errors = battle.errors.slice();
  await battle.close();
  return { label: `${own ? 'own' : 'free'}-deployed-${mode}`, before, rows, errors, free: true };
}

// Frames as runs of the same drawn state: "truex80 falsex1 ...".
function runs(rows, key) {
  const out = [];
  for (const r of rows) {
    const v = r[key];
    if (out.length && out.at(-1)[0] === v) out.at(-1)[1]++;
    else out.push([v, 1]);
  }
  return out.map(([v, n]) => `${v}x${n}`).join(' ');
}

const browser = await launchBrowser({ args: ['--ignore-gpu-blocklist'] });
const deadline = setTimeout(() => { console.error('droideka death browser check timed out'); process.exit(1); }, 2700000);
try {
  const results = [];
  for (const mode of ['enhanced', 'classic']) {
    for (const rolled of [false, true]) results.push(await lockstepped(browser, mode, rolled));
    results.push(await freeRunning(browser, mode, true));
  }
  results.push(await freeRunning(browser, 'enhanced', false));
  writeFileSync(`${OUT}/frames.json`, JSON.stringify(results, null, 1));
  for (const { label, before, rows, errors } of results) {
    const wentUp = rows.findIndex((r) => r.dead && r.mode === 4);
    console.log(label, JSON.stringify({ before, frames: rows.length, wentUpAt: rows[wentUp]?.t, removedAt: rows.find((r) => r.removed)?.t,
      visible: runs(rows, 'visible'), mode: runs(rows, 'mode'), headWhileDrawn: [...new Set(rows.filter((r) => r.visible).map((r) => r.head))].slice(-6),
      classicPieces: Math.max(...rows.map((r) => r.classic)), pieces: Math.max(...rows.map((r) => r.pieces)), errors }));
  }
  if (!REPORT) {
    for (const { label, rows, errors, free } of results) {
      assert.deepEqual(errors, [], `${label}: no page or WebGL errors`);
      const rolled = label.includes('rolled');
      const dying = rows.findIndex((r) => r.dead);
      const wentUp = rows.findIndex((r) => r.dead && r.mode === 4);
      assert.ok(dying >= 0 && wentUp >= 0, `${label}: it dies and goes up`);
      const clip = rows[wentUp].t - rows[dying].t;
      if (!rolled) {
        // Its death clip plays out first (1.35 s at AnimationSpeed 2), drawn throughout.
        assert.ok(clip > 1.2 && clip < 1.5, `${label}: goes up at the end of death01 (${clip.toFixed(2)} s)`);
        assert.ok(rows.slice(dying, wentUp).every((r) => r.visible && r.mode === 3), `${label}: drawn in death01 until it goes up`);
      } else assert.ok(clip < 0.1, `${label}: goes up at once (${clip} s)`);
      // Then it dissolves (drawn, never the bind pose: its head no higher
      // than it stood) and is never drawn again, though its records go on.
      const lastDrawn = rows.findLastIndex((r) => r.visible);
      const fade = rows[lastDrawn].t - rows[wentUp].t;
      assert.ok(fade > 0.3 && fade < 0.7, `${label}: it dissolves for about half a second (${fade.toFixed(2)} s)`);
      assert.ok(rows.slice(wentUp, lastDrawn + 1).every((r) => r.visible), `${label}: no flashing while it dissolves: ${runs(rows.slice(wentUp), 'visible')}`);
      assert.ok(rows.slice(lastDrawn + 1).every((r) => !r.visible), `${label}: never drawn again: ${runs(rows.slice(wentUp), 'visible')}`);
      assert.ok(rows.slice(lastDrawn + 1).some((r) => !r.removed), `${label}: its body lingers in the world unseen`);
      for (let i = wentUp + 1; i <= lastDrawn; i++) assert.ok(rows[i].dissolve >= rows[i - 1].dissolve, `${label}: the dissolve only grows`);
      assert.ok(rows[lastDrawn].dissolve > 0.7, `${label}: nearly dissolved when it goes (${rows[lastDrawn].dissolve})`);
      if (!rolled) {
        const standing = Math.max(...rows.slice(dying, dying + 10).map((r) => r.head));
        assert.ok(rows.slice(wentUp, lastDrawn + 1).every((r) => r.head < standing - 0.3), `${label}: dissolving in its collapsed pose, not the bind pose`);
      }
      // Free-running, the pieces need the blow-apart event, which lockstep
      // always delivers; the body goes either way.
      if (!free) {
        if (label.endsWith('classic')) assert.ok(Math.max(...rows.map((r) => r.classic)) >= 5, `${label}: the stock pieces fly`);
        else assert.ok(Math.max(...rows.map((r) => r.pieces)) >= 5, `${label}: its Enhanced pieces fly`);
      }
    }
    console.log(JSON.stringify({ pass: true, frames: FRAMES ? OUT : null }));
  }
} finally {
  clearTimeout(deadline);
  await browser.close();
}
