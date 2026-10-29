// The Dark Trooper's blast cannon as the player sees it, with the world in
// its worker and his own rounds predicted on the page: each press draws five
// bolts fanned over the weapon's spread (3 degrees of yaw, 1.5 of pitch), each
// flying the world's own pellet's path, and lights the muzzle once. Fired at
// a Rebel trooper 6 and 20 m off, the bolts drawn end on him exactly as many
// as the world's pellets strike him, each for its 55.
// BFE_TEST_URL: the Vite server (default http://127.0.0.1:18449).
import assert from 'node:assert/strict';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const base = process.env.BFE_TEST_URL || 'http://127.0.0.1:18449';
const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  const page = await browser.newPage({ viewport: { width: 480, height: 270 } }), errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  await page.route('**/@vite/client', (r) => r.fulfill({ contentType: 'application/javascript', body: '' }));
  await page.addInitScript(() => localStorage.setItem('battlefront-eternal.settings', JSON.stringify({ waterQuality: 'low', antialiasing: 'off', shadows: 'off', renderScale: .5 })));
  await page.goto(`${base}/?map=hot1&side=imp&unit=imp_inf_dark_troopersnow&ai=off&seed=1&loading=0`);
  await page.waitForFunction(() => window.__bf?.client?.ready && window.__bf.combat?.owner?.alive && !window.__bf.combat.dead, null, { timeout: 180000 });
  // The world's own pellets, as its Ordnance launches them (the page only
  // learns of a pellet that lands at once from its terminal record).
  const world = page.workers().find((w) => w.url().includes('local-server-worker'));
  assert.ok(world, 'the battle runs in its world worker');
  // The page's player is a network human in the world (its own id there).
  const player = await page.evaluate(() => __bf.combat.owner.id);
  await world.evaluate((player) => {
    const o = self.__bf.ordnance, observe = o.onLaunch;
    self.__pellets = [];
    o.onLaunch = (bolt, context) => {
      if (bolt.owner?.id === player) self.__pellets.push({ fireId: bolt.networkFireId, direction: bolt.direction.toArray() });
      return observe(bolt, context);
    };
  }, player);
  const presses = [];
  for (let press = 0; press < 3; press++) {
    const result = await page.evaluate(async () => {
      const b = __bf, sleep = (ms) => new Promise((r) => setTimeout(r, ms));
      b.graphics.render = () => {};
      b.controls.locked = true;
      const known = new Set(b.predictedShots.shots.keys());
      // Muzzle lights: the weapon's FlashLightRadius and FlashLightDuration.
      const lights = [], flash = b.lights.flash;
      b.lights.flash = function (...args) {
        const light = flash.apply(this, args);
        if (args[2] === 2 && args[3] === 0.25) lights.push(light);
        return light;
      };
      b.controls.hold('Mouse0'); b.controls.press('Mouse0');
      // The cosmetic rounds appear with the press's first predicted tick.
      let cosmetic = [];
      for (let i = 0; i < 400 && !cosmetic.length; i++) {
        await sleep(10);
        cosmetic = [...b.predictedShots.shots].filter(([key]) => !known.has(key)).map(([, shot]) => shot);
      }
      b.controls.down.delete('Mouse0');
      await sleep(100);
      b.lights.flash = flash;
      return { fireIds: [...new Set(cosmetic.map((s) => s.metadata.fireId))], cosmetic: cosmetic.map((s) => s.direction.toArray()), light: lights.length };
    });
    // Wait for the world to fire the press's pellets.
    let pellets = [];
    for (let i = 0; i < 100 && pellets.length < 5; i++) {
      await page.waitForTimeout(100);
      pellets = await world.evaluate((ids) => self.__pellets.filter((p) => ids.includes(p.fireId)), result.fireIds);
    }
    presses.push({ ...result, world: pellets.map((p) => p.direction), worldFireIds: [...new Set(pellets.map((p) => p.fireId))] });
    await page.waitForTimeout(1200); // past the 0.75 s ShotDelay
  }
  // A trooper at measured ranges on open floor, shot in the chest in first
  // person (the round leaves along the aim).
  const range = await world.evaluate(async (player) => {
    const b = self.__bf, T = b.THREE, u = b.units.byId(player), p = u.soldier.position;
    const hit = { distance: 0, point: new T.Vector3(), normal: new T.Vector3() }, down = new T.Vector3(0, -1, 0);
    const ground = (x, z) => { const h = b.collision.cast(new T.Vector3(x, p.y + 3, z), down, 8, hit); return h ? p.y + 3 - h.distance : null; };
    // A heading with level ground and a clear line to the chest at each range.
    let yaw = null;
    for (let i = 0; i < 72 && yaw === null; i++) {
      const a = i * Math.PI / 36;
      const open = [6, 20].every((d) => {
        const x = p.x + Math.sin(a) * d, z = p.z + Math.cos(a) * d, g = ground(x, z);
        if (g === null || Math.abs(g - p.y) > 0.75) return false;
        const eye = new T.Vector3(p.x, p.y + 1.6, p.z), to = new T.Vector3(x, g + 1.2, z).sub(eye), length = to.length();
        return !b.collision.cast(eye, to.divideScalar(length), length, hit);
      });
      if (open) yaw = a;
    }
    await b.factory.prepare('all_inf_soldier');
    const t = self.__target = b.factory.build('all_inf_soldier');
    t.team = t.combat.team = u.team === 1 ? 2 : 1;
    return { target: t.id, yaw };
  }, player);
  assert.ok(range.yaw !== null, 'level open ground to 20 m');
  await page.evaluate(() => __bf.setPerspective(true));
  const targets = [];
  for (const distance of [6, 20]) {
    const at = await world.evaluate(([player, yaw, distance]) => {
      const u = self.__bf.units.byId(player), p = u.soldier.position, t = self.__target;
      t.spawn(p.x + Math.sin(yaw) * distance, p.y, p.z + Math.cos(yaw) * distance, yaw + Math.PI);
      return t.position.toArray();
    }, [player, range.yaw, distance]);
    await page.waitForFunction(([id, at]) => { const u = __bf.units.byId(id); return u?.alive && Math.hypot(u.position.x - at[0], u.position.z - at[2]) < 0.3; }, [range.target, at], { timeout: 30000 });
    const aim = await page.evaluate(([id, yaw]) => {
      const b = __bf, c = b.combat, s = b.soldier, t = b.units.byId(id).position;
      s.aimYaw = yaw; s.aimPitch = 0;
      for (let i = 0; i < 4; i++) {
        c.aimBasis();
        const dx = t.x - c.origin.x, dy = t.y + 1.2 - c.origin.y, dz = t.z - c.origin.z;
        s.aimYaw = Math.atan2(dx, dz); s.aimPitch = Math.atan2(dy, Math.hypot(dx, dz));
      }
      return [s.aimYaw, s.aimPitch];
    }, [range.target, range.yaw]);
    await world.evaluate(([player, aim]) => { const s = self.__bf.units.byId(player).soldier; [s.aimYaw, s.aimPitch] = aim; self.__target.combat.health = self.__target.combat.maxHealth; }, [player, aim]);
    await page.waitForTimeout(1200);
    // The drawn bolts, stepped at 60 Hz on a held clock.
    const drawn = await page.evaluate(async () => {
      const b = __bf, sleep = (ms) => new Promise((r) => setTimeout(r, ms)), p = b.predictedShots;
      const known = new Set(p.shots.keys());
      b.controls.hold('Mouse0'); b.controls.press('Mouse0');
      let fresh = [];
      for (let i = 0; i < 400 && !fresh.length; i++) { await sleep(5); fresh = [...p.shots].filter(([k]) => !known.has(k)).map(([, s]) => s); }
      b.controls.down.delete('Mouse0');
      const now = p.now, born = Math.min(...fresh.map((s) => s.born)), lives = fresh.map((s) => s.life);
      for (let i = 0; i <= 30; i++) { p.now = () => born + i / 60; p.update(); }
      p.now = now;
      return fresh.map((s, i) => ({ flown: s.bolt.flown, stopped: s.life < lives[i] - 1e-9 }));
    });
    await page.waitForTimeout(1500);
    const health = await world.evaluate(() => self.__target.combat.health);
    targets.push({ distance, drawn, struck: Math.round((300 - health) / 55) });
  }
  assert.deepEqual(errors, []);
  for (const t of targets) {
    const stopped = t.drawn.filter((d) => d.stopped);
    console.log(JSON.stringify({ distance: t.distance, struck: t.struck, drawnStopped: stopped.length, stoppedAt: stopped.map((d) => +d.flown.toFixed(2)) }));
    assert.equal(t.drawn.length, 5);
    assert.equal(stopped.length, t.struck, `${t.distance} m: the bolts drawn ending on him are the pellets that struck him`);
    for (const d of stopped) assert.ok(Math.abs(d.flown - t.distance) < 0.8, `${t.distance} m: a bolt ends at the man, ${d.flown} m out`);
  }
  assert.ok(targets[0].struck >= 3, 'at 6 m most of the five pellets strike a trooper');
  const wrap = (a) => Math.atan2(Math.sin(a), Math.cos(a));
  const DEG = Math.PI / 180;
  const angles = ([x, y, z]) => [Math.atan2(x, z), Math.asin(Math.max(-1, Math.min(1, y)))];
  const fan = (list, i) => list.length ? (Math.max(...list.map((a) => a[i])) - Math.min(...list.map((a) => a[i]))) / DEG : 0;
  // How far each drawn bolt is from the nearest of the world's pellets.
  const offsets = (p) => p.cosmetic.map(([yaw, pitch]) => Math.min(...p.world.map(([y, x]) => Math.hypot(wrap(y - yaw), x - pitch))) / DEG);
  for (const p of presses) { p.cosmetic = p.cosmetic.map(angles); p.world = p.world.map(angles); }
  console.log(JSON.stringify(presses.map((p) => ({ fireIds: p.fireIds, drawn: p.cosmetic.length, fired: p.world.length, lights: p.light,
    drawnFanDeg: [+fan(p.cosmetic, 0).toFixed(2), +fan(p.cosmetic, 1).toFixed(2)], firedFanDeg: [+fan(p.world, 0).toFixed(2), +fan(p.world, 1).toFixed(2)],
    worstOffsetDeg: +Math.max(...offsets(p)).toFixed(3) }))));
  for (const p of presses) {
    assert.equal(p.cosmetic.length, 5, 'five bolts drawn for a press');
    assert.equal(p.world.length, 5, 'five pellets fired by the world');
    assert.deepEqual(p.worldFireIds, p.fireIds, 'the world names the same press');
    assert.equal(p.light, 1, 'one muzzle light a salvo');
    // A fan, not one bolt five times over.
    const yaws = p.cosmetic.map(([y]) => y), pitches = p.cosmetic.map(([, x]) => x);
    assert.ok(Math.max(...yaws) - Math.min(...yaws) > 0.2 * DEG, `yaw fan ${(Math.max(...yaws) - Math.min(...yaws)) / DEG} deg`);
    assert.ok(Math.max(...pitches) - Math.min(...pitches) > 0.1 * DEG, 'pitch fan');
    assert.ok(Math.max(...yaws) - Math.min(...yaws) < 6 * DEG + 1e-3 && Math.max(...pitches) - Math.min(...pitches) < 3 * DEG + 1e-3, 'inside the spread');
    // Every drawn bolt flies one of the world's pellets' paths.
    const left = [...p.world];
    for (const [yaw, pitch] of p.cosmetic) {
      let best = -1, error = Infinity;
      left.forEach(([y, x], i) => { const e = Math.hypot(wrap(y - yaw), x - pitch); if (e < error) { error = e; best = i; } });
      assert.ok(error < 0.15 * DEG, `drawn bolt off the world's pellet by ${(error / DEG).toFixed(3)} deg`);
      left.splice(best, 1);
    }
  }
  console.log('pass');
} finally { await browser.close(); }
