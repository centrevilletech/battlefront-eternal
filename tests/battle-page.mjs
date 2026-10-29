// One battle page for the browser checks. The battle runs in the world
// worker, as it always does; the check steps it in lockstep with the page's
// frames (game.lockstep in src/game.js), so every run comes out the same:
// each frame's ticks run in the worker with the page's commands, every
// packet goes on, and the frame shows the battle between the last two ticks
// at the frame clock's phase. Nothing is predicted.
//
//   const battle = await openBattle(browser, { base, query: 'map=nab2&side=rep&unit=rep_inf_clone_trooper' });
//   await battle.world.evaluate(() => self.__bf.soldier.spawn(-168, -49, -25, 0)); // the world, in the worker
//   await battle.frames(60);                                    // a second at 60 Hz, not drawn
//   await battle.page.evaluate(async () => {                     // or frame by frame on the page
//     for (let i = 0; i < 120; i++) await __bf.lockstep(1 / 120, false);
//     return __bf.actor.root.position.y;                         // what the page shows
//   });
//   battle.errors                                               // page and console errors so far
//
// The page's game is window.__bf; the world's is self.__bf in the worker
// (battle.world, a Playwright Worker). Set the world up there: the page's
// objects are its copy of the world, and the next packet overwrites what is
// changed on them. Read either; the page's copy is the world's as of the
// last tick it applied. After changing the player's life or seat in the
// world, step a frame before pressing keys: the page learns of it from that
// tick's packet (a command for his old life is dropped, and leaving a seat
// clears the keys held).
export const LOW_SETTINGS = { waterQuality: 'low', treeDetail: 'original', environmentEffects: 'original', rainEffects: 'original',
  oceanEffects: 'original', antialiasing: 'off', shadows: 'off', jetpackEffects: 'original', droidekaEffects: 'original' };

// Chromium as the checks launch it: software GL unless ANGLE says otherwise
// (CHROMIUM: a browser binary in place of Playwright's).
export async function launchBrowser({ args = [] } = {}) {
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
  const angle = process.env.ANGLE || 'swiftshader';
  const gpu = angle === 'swiftshader' ? ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] : [`--use-angle=${angle}`, '--ignore-gpu-blocklist'];
  return chromium.launch({ ...(process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {}), args: [...gpu, ...args] });
}

// `query`: the battle's URL query (a leading ? is optional). `settings`
// (null: the page's own) goes into localStorage first; `errors` filters what
// counts as one (a message in, true to keep it); `beforeLoad(page)` runs
// before the page is opened; `timeout` bounds the load.
export async function openBattle(browser, { base = 'http://127.0.0.1:18449', query, viewport = { width: 640, height: 360 },
  settings = LOW_SETTINGS, errors: keep = () => true, beforeLoad = null, timeout = 180000 } = {}) {
  const page = await browser.newPage({ viewport });
  const errors = [];
  page.on('pageerror', (e) => { if (keep(e.message)) errors.push(e.message); });
  page.on('console', (m) => { if (m.type() === 'error' && keep(m.text())) errors.push(m.text()); });
  await page.route('**/@vite/client', (r) => r.fulfill({ contentType: 'application/javascript', body: '' }));
  // The page's own display loop stays off: the check draws the frames.
  await page.addInitScript((settings) => {
    const raf = requestAnimationFrame.bind(window);
    window.requestAnimationFrame = (cb) => (cb.name === 'loop' ? 0 : raf(cb));
    if (settings) localStorage.setItem('battlefront-eternal.settings', JSON.stringify(settings));
  }, settings);
  if (beforeLoad) await beforeLoad(page);
  await page.goto(`${base}/?${String(query).replace(/^\?/, '')}`, { timeout });
  await page.waitForFunction(() => window.__bf?.client?.ready || /Failed/.test(document.querySelector('#hud')?.textContent || ''), null, { timeout });
  const failed = await page.evaluate(() => (window.__bf?.client?.ready ? null : document.querySelector('#hud')?.textContent));
  if (failed) throw new Error(failed);
  await page.evaluate(async () => { __bf.pause(); await __bf.ready; });
  const world = page.workers().find((w) => w.url().includes('local-server-worker'));
  if (!world) throw new Error('no world worker on the page');
  return {
    page, world, errors,
    // One frame of `dt` seconds (drawn if `draw`): the page's tick.
    frame: (dt = 1 / 60, draw = false) => page.evaluate(([dt, draw]) => __bf.lockstep(dt, draw), [dt, draw]),
    // `count` frames of `dt` seconds; the last drawn if `draw`.
    frames: (count, dt = 1 / 60, draw = false) => page.evaluate(async ([count, dt, draw]) => {
      let tick = __bf.worldClock.tick;
      for (let i = 0; i < count; i++) tick = await __bf.lockstep(dt, draw && i === count - 1);
      return tick;
    }, [count, dt, draw]),
    close: () => page.close(),
  };
}
