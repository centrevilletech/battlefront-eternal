// Droid deaths and wrecked power-up droids on Theed, Enhanced and Classic.
//
// With Vite running (set PLAYWRIGHT_MODULE if Playwright is elsewhere):
//   node tests/destruction-browser.mjs
// Env: GAME_URL (http://127.0.0.1:18449), DESTRUCTION_OUTPUT (frames, default
// /tmp/bf-punch/destruction), DESTRUCTION_ONLY (comma list of scenarios),
// DESTRUCTION_FRAMES=0 to skip the pictures.
//
// The same scripted kills run once with each setting; the simulation's event
// journal, random state and units must come out identical. Enhanced must
// throw its own pieces (which settle on the floor, nothing standing), Classic
// the stock ones. A player playing a droid or a droideka comes apart too.
//
// The battle runs in the world worker, stepped frame by frame
// (tests/battle-page.mjs): the droids are placed and killed in the world,
// whose journal is read there; the page draws their deaths from its packets.
import { mkdirSync, writeFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { launchBrowser, openBattle } from './battle-page.mjs';
const URL = process.env.GAME_URL || 'http://127.0.0.1:18449';
const OUT = process.env.DESTRUCTION_OUTPUT || '/tmp/bf-punch/destruction';
const FRAMES = process.env.DESTRUCTION_FRAMES !== '0';
const ONLY = (process.env.DESTRUCTION_ONLY || '').split(',').filter(Boolean);
mkdirSync(OUT, { recursive: true });

// Times are seconds after the kill (the first, 0, is just before it).
const SCENARIOS = [
  { name: 'battle-droid', cls: 'cis_inf_assault', kill: 'apart', times: [0, 0.03, 0.12, 0.25, 0.5, 0.9, 1.6, 2.6, 8] },
  { name: 'battle-droid-on-its-feet', cls: 'cis_inf_assault', kill: 'clip', times: [0, 0.12, 0.3, 0.45, 0.6, 1, 2, 8] },
  { name: 'super-battle-droid', cls: 'cis_inf_battledroid', kill: 'apart', times: [0, 0.03, 0.12, 0.3, 0.6, 1.2, 2.5, 8] },
  { name: 'droideka-deployed', cls: 'cis_inf_droideka', kill: 'clip', times: [0, 0.3, 0.8, 1.2, 1.4, 1.5, 1.8, 2.5, 4, 9] },
  { name: 'droideka-rolled', cls: 'cis_inf_droideka', kill: 'rolled', times: [0, 0.03, 0.12, 0.3, 0.6, 1.2, 2.5, 8] },
  { name: 'ammo-droid', kind: 'station', times: [0, 0.05, 0.15, 0.4, 1, 2.5, 6, 15, 45, 100] },
].filter((s) => !ONLY.length || ONLY.includes(s.name));
const SPOT = [-51.76, -50.04, -27.93]; // an open stretch of Theed's plaza

// A battle page with the setting (the world worker starts with the same
// settings, so the two runs differ only in it), and both sides' helpers.
async function open(browser, query, mode) {
  const battle = await openBattle(browser, { base: URL, query: `map=nab2&${query}`, viewport: { width: 960, height: 600 },
    // Frames are filmed far apart; no history to blend.
    settings: { destructionEffects: mode, upscaler: 'off' } });
  await battle.page.evaluate(helpers);
  await battle.world.evaluate(worldHelpers);
  return battle;
}

// The world's helpers (self.__destruction in the worker): the scenario's
// bodies, the kill, the journal.
function worldHelpers() {
  const b = self.__bf;
  self.__destruction = {
    prepare(classes, spot) {
      return classes.map((c, i) => { const u = b.factory.acquire(c); u.team = 2; u.combat.team = 2; u.spawn(spot[0] + i * 2.5, spot[1], spot[2], 0); return u.id; });
    },
    remove(ids) { for (const id of ids) b.units.byId(id).remove(); },
    setup(sc, spot) {
      for (const u of b.factory.built) if (u !== b.playerUnit && !u.removed) u.remove();
      b.soldier.position.set(-20, -50, -10);
      if (sc.kind === 'station') {
        const station = b.live.stations.find((s) => s.desc.model === 'com_item_weaponrecharge' && Math.abs(s.position.x + 76.2) < 1);
        this.target = { station };
        return { station: b.live.stations.indexOf(station) };
      }
      const u = b.factory.acquire(sc.cls);
      u.team = 2; u.combat.team = 2;
      u.spawn(spot[0], spot[1], spot[2], 2.2);
      this.target = { unit: u };
      if (sc.kill === 'rolled') u.soldier.enter(11);
      return { unit: u.id };
    },
    kill(sc) {
      const { station, unit } = this.target;
      if (station) { station.damage(10000, 'player'); return; }
      // As the simulation would: a blast marks it to blow apart, or its roll
      // gives it the death clip. Identical in both modes.
      if (sc.kill === 'apart') unit.combat.blowApart = true;
      else unit.combat.random = () => 0.99;
      unit.combat.damage(100000, 100000, unit.position.clone(), 'player');
    },
    // Several bodies of one class at once, in a line.
    line(cls, count, spot) {
      for (const u of b.factory.built) if (u !== b.playerUnit && !u.removed) u.remove();
      this.lineUp = Array.from({ length: count }, (_, i) => {
        const u = b.factory.acquire(cls);
        u.team = 2; u.combat.team = 2;
        u.spawn(spot[0] + (i - 1) * 7, spot[1], spot[2] + (i === 1 ? 5 : 0), 2.2);
        return u;
      });
      return this.lineUp.map((u) => u.id);
    },
    killInLine(i) {
      const u = this.lineUp[i];
      u.combat.random = () => 0.99;
      u.combat.damage(100000, 100000, u.position.clone(), 'player');
      return { at: u.position.toArray(), offset: [3.6, 1.9, 3.9], look: u.walker ? 0.8 : 0.9 };
    },
    // What the simulation did: its journal, randomness, and units.
    journal() {
      // Records as plain data. Some hold live objects (scene parts, units):
      // a unit-like one by its id and generation, anything else by its kind.
      const plain = (v, depth = 0) => {
        if (typeof v === 'number') return Math.round(v * 1e4) / 1e4;
        if (!v || typeof v !== 'object') return v;
        if (Array.isArray(v)) return depth > 4 ? '[array]' : v.map((x) => plain(x, depth + 1));
        if (v.isVector3) return [v.x, v.y, v.z].map((x) => plain(x));
        if (Object.getPrototypeOf(v) !== Object.prototype) {
          return 'id' in v && 'generation' in v ? `[${v.constructor?.name} ${v.id}/${v.generation}]` : `[${v.constructor?.name || 'object'}]`;
        }
        if (depth > 4) return '[deep]';
        const out = {};
        for (const key of Object.keys(v)) out[key] = plain(v[key], depth + 1);
        return out;
      };
      const events = [];
      for (let seq = 0; seq < b.simEvents.seq; seq++) {
        const r = b.simEvents.ring[seq % b.simEvents.capacity];
        if (r) events.push(JSON.stringify(plain(r)));
      }
      const units = b.factory.built.map((u) => [u.className, u.alive, u.dead, u.removed, Math.round(u.combat.health * 100), u.position.toArray().map((x) => Math.round(x * 1000))]);
      const stations = b.live.stations.map((s) => [s.state, Math.round(s.health * 100)]);
      return { seq: b.simEvents.seq, events, random: b.randomState(), units, stations, player: [b.combat.health, b.soldier.position.toArray().map((x) => Math.round(x * 1000))] };
    },
  };
}

// The page's helpers: frames, the scenario's view, a measure, a picture.
function helpers() {
  const b = __bf, T = b.THREE;
  const frames = async (count) => { for (let i = 0; i < count; i++) await b.lockstep(1 / 60, false); };
  window.__destruction = {
    // Bodies and their textures loaded before anything is filmed.
    async prepare(spot) {
      await frames(5);
      b.camera.position.set(spot[0] + 3, spot[1] + 2, spot[2] + 6); b.camera.lookAt(spot[0], spot[1] + 1, spot[2]); b.camera.updateMatrixWorld();
      b.graphics.render(0);
      await new Promise((r) => setTimeout(r, 6000));
    },
    clear() {
      b.destruction?.clear();
      b.debris.clear();
    },
    // The world has placed the scenario's body (or picked its power-up
    // droid); the page shows it from the next tick.
    async setup(sc, target) {
      await frames(1);
      b.presentation.reset();
      if (target.station !== undefined) {
        const station = b.live.stations[target.station];
        this.target = { station };
        await frames(19);
        const c = station.center(new T.Vector3());
        // The first angle round it with a clear view of it.
        let offset = [4.2, 2.4, -4.6];
        const hit = { distance: 0, point: new T.Vector3(), normal: new T.Vector3() };
        for (let k = 0; k < 16; k++) {
          const a = (k / 16) * Math.PI * 2, o = [Math.cos(a) * 5.2, 2.3, Math.sin(a) * 5.2];
          const eye = new T.Vector3(c.x + o[0], station.position.y + o[1], c.z + o[2]), to = new T.Vector3(c.x, station.position.y + 0.9, c.z);
          const dir = to.clone().sub(eye), side = new T.Vector3(-dir.z, 0, dir.x).normalize().multiplyScalar(1.2);
          const clear = [0, 1, -1].every((s) => { const e = eye.clone().addScaledVector(side, s), d = to.clone().sub(e), l = d.length(); return !b.collision.cast(e, d.divideScalar(l), l - 0.9, hit); });
          if (clear) { offset = o; break; }
        }
        return { at: [c.x, station.position.y, c.z], offset, look: 0.9 };
      }
      const u = b.units.byId(target.unit);
      this.target = { unit: u };
      await frames(sc.kill === 'rolled' ? 89 : 29);
      return { at: [u.position.x, u.position.y, u.position.z], offset: [3.6, 1.9, 3.9], look: sc.cls === 'cis_inf_droideka' ? 0.8 : 0.9, rolled: u.soldier.mode === 12 };
    },
    async step(seconds) {
      await frames(Math.round(seconds * 60));
      const { station, unit } = this.target;
      const pieces = (b.destruction?.pieces.pieces || []).map((p) => {
        const up = new T.Vector3(0, 1, 0), box = p.shape.half;
        // How far its centre stands above the floor, in its smallest half sizes.
        const hit = b.collision.cast(p.body.position.clone().setY(p.body.position.y + 0.05), new T.Vector3(0, -1, 0), 4, { distance: 0, point: new T.Vector3(), normal: new T.Vector3() });
        const height = hit ? p.body.position.y - hit.point.y : Infinity;
        return { asleep: p.body.sleeping, height: +height.toFixed(3), tall: +(height / Math.min(box.x, box.y, box.z)).toFixed(2), heat: +p.heat.toFixed(2), up: +Math.abs(up.dot(new T.Vector3(1, 0, 0).applyMatrix3(p.body.rotation))).toFixed(2) };
      });
      return {
        visible: unit ? unit.actor.root.visible : undefined, state: station?.state, mode: unit?.soldier.mode,
        classic: b.debris.pieces.length, stats: b.destruction?.stats() || null, pieces,
        effects: b.effects.effects.length,
      };
    },
    shot({ at, offset, look }) {
      b.camera.position.set(at[0] + offset[0], at[1] + offset[1], at[2] + offset[2]);
      b.camera.lookAt(at[0], at[1] + look, at[2]);
      b.camera.updateMatrixWorld();
      b.actor.root.visible = false;
      b.graphics.render(performance.now() / 1000);
      return b.renderer.domElement.toDataURL('image/jpeg', 0.9);
    },
    // Several bodies of one class at once (every one shares its model's
    // geometry), each killed in turn on its feet.
    async line(ids) {
      await frames(30);
      this.lineUp = ids.map((id) => b.units.byId(id));
      const owners = new Set(this.lineUp.map((u) => u.actor.body.geometry));
      return { bodies: new Set(this.lineUp.map((u) => u.actor)).size, geometries: owners.size };
    },
    // While unit i shorts out: are its arcs, sparks, flashes and lights on
    // its own body, and none on the others'?
    async shortingOut(i, seconds) {
      const u = this.lineUp[i];
      // Lights asked for by Destruction itself (the light pool also re-adds
      // the last blast's fading flash, wherever that was).
      const lit = [], add = b.lights.add.bind(b.lights), update = b.destruction.update.bind(b.destruction);
      let inside = false;
      b.lights.add = (position, ...rest) => { if (inside) lit.push(position.clone()); return add(position, ...rest); };
      b.destruction.update = (...args) => { inside = true; try { return update(...args); } finally { inside = false; } };
      try { await frames(Math.round(seconds * 60)); }
      finally { b.lights.add = add; delete b.destruction.update; }
      const d = b.destruction, own = new Set(u.actor.bones);
      const others = new Set(this.lineUp.filter((x) => x !== u).flatMap((x) => x.actor.bones));
      const ends = [];
      for (const arc of d.fx.arcs.arcs) if (arc.live) for (const end of [arc.a, arc.b]) if (end.object) ends.push(end.object);
      let sparksFrom = 0, lightsFrom = 0;
      for (let k = 0; k < d.fx.hot.count; k++) sparksFrom = Math.max(sparksFrom, new T.Vector3().fromArray(d.fx.hot.arrays[0], k * 4).distanceTo(u.position));
      for (const at of lit) lightsFrom = Math.max(lightsFrom, at.distanceTo(u.position));
      return {
        visible: u.actor.root.visible, shorting: d.deaths.some((x) => x.unit === u),
        arcs: ends.length, ownArcs: ends.every((o) => own.has(o)), othersArcs: ends.filter((o) => others.has(o)).length,
        sparks: d.fx.hot.count, lights: lit.length, sparksFrom: +sparksFrom.toFixed(2), lightsFrom: +lightsFrom.toFixed(2),
      };
    },
  };
}

// Each class prepared on both sides (the page builds the world's new bodies
// as their packets name them), one of each shown until its textures load.
async function prepare({ page, world }, classes) {
  await world.evaluate(async (classes) => { for (const c of classes) await self.__bf.factory.prepare(c); }, classes);
  await page.evaluate(async (classes) => { for (const c of classes) await __bf.factory.prepare(c); }, classes);
  const shown = await world.evaluate(([classes, spot]) => self.__destruction.prepare(classes, spot), [classes, SPOT]);
  await page.evaluate((spot) => __destruction.prepare(spot), SPOT);
  await world.evaluate((ids) => self.__destruction.remove(ids), shown);
}

async function battle(browser, mode, film) {
  const run = await open(browser, 'side=rep&unit=rep_inf_clone_trooper', mode);
  const { page, world } = run;
  await prepare(run, ['cis_inf_assault', 'cis_inf_battledroid', 'cis_inf_droideka']);
  const results = {};
  for (const sc of SCENARIOS) {
    await page.evaluate(() => __destruction.clear());
    const target = await world.evaluate(([sc, spot]) => self.__destruction.setup(sc, spot), [sc, SPOT]);
    const view = await page.evaluate(([sc, target]) => __destruction.setup(sc, target), [sc, target]);
    const rows = [];
    let t = 0;
    for (const time of sc.times) {
      const state = await page.evaluate((s) => __destruction.step(s), time - t);
      t = time;
      if (film) {
        const url = await page.evaluate((v) => __destruction.shot(v), view);
        writeFileSync(`${OUT}/${sc.name}-${mode}-${String(Math.round(time * 1000)).padStart(6, '0')}ms.jpg`, Buffer.from(url.split(',')[1], 'base64'));
      }
      rows.push({ time, ...state });
      if (time === sc.times[0]) await world.evaluate((sc) => self.__destruction.kill(sc), sc);
    }
    results[sc.name] = { view, rows };
    console.log(mode, sc.name, JSON.stringify(rows.at(-1).stats || { classic: rows.at(-1).classic }));
  }
  const journal = await world.evaluate(() => self.__destruction.journal());
  await run.close();
  return { results, journal, errors: run.errors };
}

// The second and third of a class, after the first, short out on their own
// bodies: bodies of one model share their geometry, and nothing about one
// body may be kept against it.
async function repeats(browser, film) {
  const run = await open(browser, 'side=rep&unit=rep_inf_clone_trooper', 'enhanced');
  const { page, world } = run;
  await prepare(run, ['cis_inf_assault', 'cis_inf_droideka']);
  const results = {};
  for (const [label, cls, during, after] of [['battle-droid', 'cis_inf_assault', 0.25, 0.4], ['droideka', 'cis_inf_droideka', 0.7, 0.9]]) {
    await page.evaluate(() => __bf.destruction?.clear());
    const ids = await world.evaluate(([cls, spot]) => self.__destruction.line(cls, 3, spot), [cls, SPOT]);
    const line = await page.evaluate((ids) => __destruction.line(ids), ids);
    const rows = [];
    for (let i = 0; i < 3; i++) {
      await page.evaluate(() => __bf.destruction.clear());
      const view = await world.evaluate((i) => self.__destruction.killInLine(i), i);
      const state = await page.evaluate(([i, s]) => __destruction.shortingOut(i, s), [i, during]);
      if (film) {
        const url = await page.evaluate((v) => __destruction.shot(v), view);
        writeFileSync(`${OUT}/second-droid-${label}-${i + 1}-shorting-out-${String(Math.round(during * 1000)).padStart(6, '0')}ms.jpg`, Buffer.from(url.split(',')[1], 'base64'));
      }
      const blown = await page.evaluate(([i, s]) => __destruction.shortingOut(i, s), [i, after]);
      if (film) {
        const url = await page.evaluate((v) => __destruction.shot(v), view);
        writeFileSync(`${OUT}/second-droid-${label}-${i + 1}-blown-${String(Math.round((during + after) * 1000)).padStart(6, '0')}ms.jpg`, Buffer.from(url.split(',')[1], 'base64'));
      }
      rows.push({ shortingOut: state, afterwards: { visible: blown.visible } });
    }
    results[label] = { line, rows };
    console.log('repeats', label, JSON.stringify(rows.map((r) => r.shortingOut)));
  }
  await run.close();
  return { results, errors: run.errors };
}

async function player(browser, unit) {
  const run = await open(browser, `side=cis&unit=${unit}`, 'enhanced');
  await run.frames(30);
  await run.world.evaluate(() => { const b = self.__bf; b.combat.damage(100000, 100000, b.soldier.position.clone(), null); });
  const result = await run.page.evaluate(async () => {
    const b = __bf;
    const seen = [];
    for (let i = 0; i < 150; i++) {
      await b.lockstep(1 / 60, false);
      if (i % 15 === 14) seen.push({ t: +((i + 1) / 60).toFixed(2), visible: b.actor.root.visible, pieces: b.destruction.pieces.count, deaths: b.destruction.deaths.length, classic: b.debris.pieces.length });
    }
    return { seen, dead: b.combat.dead };
  });
  await run.close();
  return { result, errors: run.errors };
}

const browser = await launchBrowser({ args: ['--ignore-gpu-blocklist'] });
const deadline = setTimeout(() => { console.error('destruction browser check timed out'); process.exit(1); }, 3600000);
try {
  const enhanced = await battle(browser, 'enhanced', FRAMES);
  const classic = await battle(browser, 'classic', FRAMES);
  const droid = await player(browser, 'cis_inf_assault');
  const droideka = await player(browser, 'cis_inf_droideka');
  const again = await repeats(browser, FRAMES);
  writeFileSync(`${OUT}/metrics.json`, JSON.stringify({ enhanced: enhanced.results, classic: classic.results, droid, droideka, repeats: again.results }, null, 1));
  for (const run of [enhanced, classic, droid, droideka, again]) assert.deepEqual(run.errors, [], 'no page or WebGL errors');
  for (const [label, { line, rows }] of Object.entries(again.results)) {
    assert.deepEqual(line, { bodies: 3, geometries: 1 }, `${label}: three bodies sharing one model geometry`);
    rows.forEach(({ shortingOut: s, afterwards }, i) => {
      assert.ok(s.visible && s.shorting && s.arcs > 0 && s.sparks > 0 && s.lights > 0, `${label} ${i + 1}: shorts out (${JSON.stringify(s)})`);
      assert.ok(s.ownArcs && s.othersArcs === 0, `${label} ${i + 1}: arcs on its own body only (${JSON.stringify(s)})`);
      assert.ok(s.sparksFrom < 3 && s.lightsFrom < 3, `${label} ${i + 1}: its sparks, flashes and light within 3 m of it (${JSON.stringify(s)})`);
      assert.equal(afterwards.visible, false, `${label} ${i + 1}: then blows apart`);
    });
  }

  // The simulation is the same either way.
  assert.equal(enhanced.journal.seq, classic.journal.seq, 'as many simulation events');
  assert.deepEqual(enhanced.journal.events, classic.journal.events, 'the same simulation events');
  assert.deepEqual(enhanced.journal.random, classic.journal.random, 'the same gameplay random state');
  assert.deepEqual(enhanced.journal.units, classic.journal.units, 'the same units, health and positions');
  assert.deepEqual(enhanced.journal.stations, classic.journal.stations, 'the same power-up droids');
  assert.deepEqual(enhanced.journal.player, classic.journal.player);

  for (const [name, { rows }] of Object.entries(enhanced.results)) {
    const last = rows.at(-1);
    if (name === 'ammo-droid') {
      assert.ok(rows.some((r) => r.stats.sites === 1 && r.stats.smoke > 5), 'the wreck smoulders');
      assert.equal(rows.find((r) => r.time === 15).state, 'destroyed');
      continue;
    }
    assert.equal(last.visible, false, `${name}: the body is gone`);
    assert.equal(last.classic, 0, `${name}: no stock pieces in Enhanced`);
    assert.ok(last.pieces.length >= 5, `${name}: its pieces are out`);
    for (const piece of last.pieces) {
      assert.ok(piece.asleep, `${name}: every piece has come to rest`);
      assert.ok(piece.height > -0.02 && piece.height < 0.8, `${name}: a piece rests on the floor (${piece.height} m up)`);
      assert.ok(piece.tall < 2.2, `${name}: a piece is standing on end (${piece.tall})`);
    }
  }
  for (const [name, { rows }] of Object.entries(classic.results)) {
    assert.ok(rows.every((r) => !r.stats || (r.stats.pieces === 0 && r.stats.hot === 0 && r.stats.sites === 0)), `${name}: Enhanced stays out of Classic`);
    if (name === 'battle-droid' || name.startsWith('droideka')) assert.ok(rows.some((r) => r.classic >= 5), `${name}: the stock pieces fly`);
  }
  for (const [label, run] of [['droid', droid], ['droideka', droideka]]) {
    const end = run.result.seen.at(-1);
    assert.ok(run.result.dead && !end.visible && end.pieces >= 5 && end.classic === 0, `the player as a ${label} comes apart: ${JSON.stringify(run.result.seen)}`);
  }
  console.log(JSON.stringify({ pass: true, frames: FRAMES ? OUT : null, simulationEvents: enhanced.journal.seq }));
} finally {
  clearTimeout(deadline);
  await browser.close();
}
