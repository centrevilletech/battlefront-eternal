// Requires Vite and the extracted Naboo/Bespin maps and CIS assault unit.
// Turning and stairs as drawn at 60, 120 and 144 Hz, the world worker
// stepped frame by frame (tests/battle-page.mjs).
import { launchBrowser, openBattle } from './battle-page.mjs';
const base = process.env.MOVEMENT_TEST_URL || 'http://127.0.0.1:18449';
const browser = await launchBrowser();
try {
  for (const map of ['nab2', 'bes2']) {
    const battle = await openBattle(browser, { base, query: `map=${map}&side=cis&unit=cis_inf_assault`, viewport: { width: 800, height: 500 }, settings: null });
    const { page, world, errors } = battle;
    page.on('console', m => { if (m.type() !== 'error' && /GL_INVALID_OPERATION/.test(m.text())) errors.push(m.text()); });
    // A strafe (the arrows strafe; the mouse alone turns) drawn at an even
    // pace at each display rate, from the same spot each time: once at
    // speed, every frame moves him the same distance.
    await battle.frames(30);
    const start = await world.evaluate(() => { const s = self.__bf.soldier; return [s.position.x, s.position.y, s.position.z, s.aimYaw]; });
    const turns = [];
    for (const hz of [60, 120, 144]) {
      await page.evaluate(() => __bf.keys.clear());
      await world.evaluate(start => self.__bf.soldier.spawn(...start), start);
      await battle.frame(1 / 60);
      turns.push(await page.evaluate(async hz => {
        const b = __bf;
        b.setPerspective(true); b.presentation.reset();
        for (let i = 0; i < hz; i++) await b.lockstep(1 / hz, false);
        b.keys.add('ArrowLeft'); const steps = [];
        for (let i = 0; i < hz; i++) {
          const before = b.actor.root.position.clone();
          await b.lockstep(1 / hz, false);
          if (i > hz / 2) steps.push(Math.hypot(b.actor.root.position.x - before.x, b.actor.root.position.z - before.z));
        }
        b.keys.clear();
        const mean = steps.reduce((sum, x) => sum + x, 0) / steps.length;
        return { hz, mean, maxError: Math.max(...steps.map(x => Math.abs(x - mean))) / mean };
      }, hz));
    }
    const routes = map === 'nab2'
      ? [[-168, -49, -25, Math.PI], [-168, -44, -48, 0]]
      : [[-163.068, -41, 81.088, Math.PI / 2], [-143.068, -33, 81.088, -Math.PI / 2]];
    const stairs = [];
    for (const firstPerson of [false, true]) for (const [direction, route] of routes.entries()) {
      await page.evaluate(() => __bf.keys.clear());
      // The world puts him at the foot (or head) of the stairs; the page
      // shows him there from the next tick.
      await world.evaluate(route => self.__bf.soldier.spawn(...route), route);
      await battle.frame(1 / 60);
      stairs.push(await page.evaluate(async ([route, firstPerson, direction]) => {
        const b = __bf, s = b.soldier;
        b.animator.reset(route[3]); b.presentation.reset();
        b.setPerspective(true); await b.lockstep(0, false); b.setPerspective(firstPerson);
        for (let i = 0; i < 60; i++) await b.lockstep(1 / 120, false);
        b.keys.add('KeyW');
        let lastY = b.actor.root.position.y, lastDelta = 0, reversals = 0, maxStep = 0, airFrames = 0;
        for (let i = 0; i < 360; i++) {
          await b.lockstep(1 / 120, false);
          const y = b.actor.root.position.y, delta = y - lastY;
          if (i > 30) {
            maxStep = Math.max(maxStep, Math.abs(delta));
            if (delta * lastDelta < -1e-5) reversals++;
            if (s.air) airFrames++;
          }
          lastY = y; lastDelta = delta;
          if (!Number.isFinite(b.camera.position.lengthSq())) throw Error('Invalid camera');
        }
        b.keys.clear();
        return { firstPerson, direction: direction ? 'down' : 'up', maxStep, reversals, airFrames };
      }, [route, firstPerson, direction]));
    }
    const gl = await page.evaluate(async () => { __bf.setPerspective(false); await __bf.lockstep(1 / 60, true); return __bf.renderer.getContext().getError(); });
    console.log(JSON.stringify({ map, turns, stairs, gl, errors }));
    if (errors.length || gl || turns.some(x => !(x.mean > 0) || x.maxError > 0.05)
      || stairs.some(x => x.maxStep > 0.1 || x.reversals > 5 || x.airFrames > 4)) throw Error('Movement continuity regression');
    await battle.close();
  }
} finally { await browser.close(); }
