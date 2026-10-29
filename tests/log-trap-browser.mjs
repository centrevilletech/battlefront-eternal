// Requires Vite and the extracted Endor (end1, maps generation 12: the log
// traps live). The world (in its worker) builds trap 0 with the Rebels'
// build rate, waits for it to be hauled up, and walks an Imperial AT-ST
// through it; the page draws the trap's clip from the world's state (schema
// 10): cocked, swinging, at rest. Checks that the page follows the world's
// phases, that the walker is gone after the crush, and that the trap is
// unbuilt again with its pad. Frames of the swing from a fixed camera go to
// LOGTRAP_TEST_ARTIFACTS.
import fs from 'node:fs/promises';
import { launchBrowser, openBattle, LOW_SETTINGS } from './battle-page.mjs';
const base = process.env.LOGTRAP_TEST_URL || 'http://127.0.0.1:18530';
const artifacts = process.env.LOGTRAP_TEST_ARTIFACTS;
const check = (ok, message, detail) => { if (!ok) throw new Error(`${message}: ${JSON.stringify(detail)}`); };
const browser = await launchBrowser();
try {
  const battle = await openBattle(browser, { base, query: 'map=end1&side=all&ai=off&seed=1',
    viewport: { width: 960, height: 540 }, settings: { ...LOW_SETTINGS } });
  const { page, world, errors } = battle;
  const setup = await world.evaluate(() => {
    const b = self.__bf, trap = b.live.traps[0];
    while (trap.state !== 'built') trap.setHealth(trap.health + 30);
    // Across the logs' line: the trap's own z axis.
    const across = new b.THREE.Vector3().setFromMatrixColumn(trap.matrix, 2).normalize();
    // The player stands 40 m off across the logs' line (events reach the
    // clients near them).
    const me = self.__server?.actor?.unit;
    if (me?.soldier) me.soldier.spawn(trap.point.x + across.x * 40, trap.point.y + 1, trap.point.z + across.z * 40, 0);
    return { at: trap.point.toArray(), across: across.toArray(), team: trap.team, count: b.live.traps.length };
  });
  check(setup.count === 2 && setup.team === 1, 'two traps, the Alliance\'s', setup);
  const pageTrap = () => page.evaluate(() => { const t = __bf.live.traps[0]; return { phase: t.phase, state: t.state, pad: t.pad?.root.visible }; });
  for (let i = 0; i < 40; i++) {
    await battle.frames(15);
    if ((await pageTrap()).phase === 'armed') break;
  }
  check((await pageTrap()).phase === 'armed', 'the page draws it cocked after its build clip', await pageTrap());
  // An AT-ST 30 m out, walking through the middle at 3 m/s.
  await world.evaluate(() => {
    const b = self.__bf, trap = b.live.traps[0];
    const v = b.vehicles.vehicles.find((h) => /atst/.test(h.name));
    if (!v.alive) v.spawn(2);
    v.team = 2;
    const dir = new b.THREE.Vector3(1, 0, 0.3).normalize();
    const from = trap.point.clone().addScaledVector(dir, -30);
    v.position.copy(from); v.yaw = Math.atan2(dir.x, dir.z); v.syncBody();
    self.__trapWalker = { v, dir, y: from.y };
    const step = b.step;
    b.step = (...args) => {
      const w = self.__trapWalker;
      if (w.v.alive) { w.v.position.addScaledVector(w.dir, 3 / 60); w.v.position.y = w.y; w.v.syncBody(); }
      return step(...args);
    };
  });
  // The page's effects, to find the crush's DeathEffect where the logs meet.
  await page.evaluate(() => {
    const fx = __bf.effects, play = fx.play.bind(fx);
    window.__trapEffects = [];
    fx.play = (name, position, ...rest) => { window.__trapEffects.push([name, position?.toArray?.() ?? null]); return play(name, position, ...rest); };
  });
  const look = setup.at;
  const eye = [look[0] + setup.across[0] * 42, look[1] + 9, look[2] + setup.across[2] * 42];
  const seen = new Set();
  let shot = 0;
  let walkerGoneOnPage = false;
  for (let frame = 0; frame < 900; frame++) {
    await battle.frame(1 / 60, false);
    const p = await page.evaluate(async ({ eye, look, draw }) => {
      const b = __bf, t = b.live.traps[0];
      const v = b.vehicles?.vehicles.find((h) => /atst/.test(h.name));
      let image = null;
      if (draw) {
        const saved = { p: b.camera.position.clone(), q: b.camera.quaternion.clone() };
        b.camera.position.fromArray(eye); b.camera.lookAt(look[0], look[1] + 6, look[2]); b.camera.updateMatrixWorld(true);
        b.graphics.render(0);
        image = b.renderer.domElement.toDataURL();
        b.camera.position.copy(saved.p); b.camera.quaternion.copy(saved.q);
      }
      return { phase: t.phase, state: t.state, pad: t.pad?.root.visible, walker: Boolean(v?.alive && v.root.visible), image };
    }, { eye, look, draw: Boolean(artifacts) && (seen.has('swing') ? frame % 6 === 0 : frame % 60 === 0) });
    seen.add(p.phase);
    if (seen.has('swing') && !p.walker) walkerGoneOnPage = true;
    if (p.image) {
      await fs.mkdir(artifacts, { recursive: true });
      await fs.writeFile(`${artifacts}/logtrap-${String(shot++).padStart(3, '0')}-${p.phase}.png`, Buffer.from(p.image.split(',')[1], 'base64'));
    }
    if (seen.has('swing') && p.phase === 'down') break;
  }
  const end = await pageTrap();
  check(seen.has('swing'), 'the page drew the swing', [...seen]);
  check(walkerGoneOnPage, 'the walker is gone from the page after the crush', end);
  check(end.state === 'unbuilt' && end.phase === 'down' && end.pad, 'unbuilt again, its pad out', end);
  const crushed = await world.evaluate(() => self.__bf.live.traps[0].crushed);
  check(crushed === 1, 'the world crushed the walker', crushed);
  const effectAt = await world.evaluate(() => self.__bf.live.traps[0].effectAt.toArray());
  const played = await page.evaluate(() => window.__trapEffects);
  check(played.some(([name, p]) => name === 'explosion' && p && Math.hypot(p[0] - effectAt[0], p[1] - effectAt[1], p[2] - effectAt[2]) < 0.01),
    'the page played the DeathEffect where the logs meet', { effectAt, played: played.slice(0, 10) });
  check(errors.length === 0, 'no page errors', errors);
  console.log(`log trap: ${[...seen].join(' -> ')}, crushed ${crushed}, ${shot} frames`);
} finally {
  await browser.close();
}
