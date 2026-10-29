// The front end's menus at 4:3 and wide (screenshots in /tmp/bf-menu-fixes),
// then a battle, its world worker stepped with the page's frames
// (tests/battle-page.mjs): the sharpshooter's scope (Z) puts the drawn
// camera on the world's line of fire, the chat's three channels, and Esc's
// pause menu giving the mouse back after it.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { launchBrowser, openBattle } from './battle-page.mjs';
const browser = await launchBrowser();
const base = process.env.SHELL_TEST_URL || 'http://127.0.0.1:18449';
const out = '/tmp/bf-menu-fixes';
await fs.mkdir(out, { recursive: true });
try {
  const errors = [];
  if (!process.env.MENU_FIXES_GAME_ONLY) {
  const page = await browser.newPage({ viewport: { width: 1024, height: 768 } });
  page.on('pageerror', e => errors.push(e.message));
  await page.goto(`${base}/?screen=main&noreload=1`);
  await page.waitForFunction(() => window.__shell?.stack);
  await page.waitForTimeout(1800);
  await page.screenshot({ path: `${out}/main-4x3.png` });
  await page.evaluate(() => { const s = __shell; s.stack.push(s.screens.options()); });
  await page.waitForTimeout(2000);
  await page.keyboard.press('ArrowDown'); await page.keyboard.press('ArrowDown'); await page.keyboard.press('ArrowDown'); await page.keyboard.press('Enter');
  await page.waitForFunction(() => __shell.stack.top.id === 'options-controls');
  await page.waitForTimeout(1200);
  await page.screenshot({ path: `${out}/controls-4x3.png` });
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.waitForTimeout(500);
  await page.screenshot({ path: `${out}/controls-wide.png` });
  assert.deepEqual(errors, []);
  await page.close();
  }
  const battle = await openBattle(browser, { base, query: 'map=nab2&side=rep&unit=rep_inf_clone_sharpshooter&ai=off&noreload=1', viewport: { width: 640, height: 480 },
    settings: { antialiasing:'off', shadows:'off', upscaler:'off', waterQuality:'low', renderScale:1 } });
  const { page: game, world } = battle;
  await battle.frames(120);
  // Z goes to the world with the next tick's command; the world fires along
  // its own aim, which the page never receives (a tick's scratch).
  await game.keyboard.press('KeyZ');
  await battle.frames(30);
  const shotDir = await world.evaluate(() => self.__bf.combat.shotDir.toArray());
  const scope = await game.evaluate(async (shotDir) => {
    const b = __bf;
    await b.lockstep(0, true);
    const forward = b.camera.getWorldDirection(new b.THREE.Vector3());
    return { zoom: b.combat.zoom, eye: b.combat.usesEyeView, y:b.hud.reticle.y, dot:forward.dot(new b.THREE.Vector3(...shotDir)) };
  }, shotDir);
  assert.ok(scope.zoom > 1); assert.equal(scope.eye, true);
  assert.ok(Math.abs(scope.y-.5)<.001, JSON.stringify(scope)); assert.ok(scope.dot>.9999, JSON.stringify(scope));
  await game.screenshot({path:`${out}/scope.png`});
  await game.keyboard.press('KeyT');
  await game.keyboard.type('Hello from single player');
  await game.keyboard.press('Enter');
  await game.keyboard.press('KeyY');
  await game.keyboard.type('Team message');
  await game.keyboard.press('Enter');
  await game.evaluate(() => __bf.chat.server('Server announcement'));
  const messages = await game.evaluate(() => __bf.chat.log.messages);
  assert.deepEqual(messages.map(m=>m.channel), ['global','team','server']);
  assert.equal(await game.evaluate(() => __bf.controls.enabled), true);
  await game.screenshot({path:`${out}/chat.png`});
  await game.mouse.click(400, 200);
  await game.waitForFunction(() => __bf.controls.locked);
  await game.keyboard.press('Escape');
  await game.waitForFunction(() => __bf.menu.isOpen);
  await game.evaluate(async () => { __bf.menu.update(1); await __bf.lockstep(0, true); });
  await game.screenshot({path:`${out}/pause.png`});
  await game.waitForTimeout(1200);
  await game.keyboard.press('Escape');
  assert.equal(await game.evaluate(() => __bf.menu.isOpen), false);
  await game.waitForTimeout(500);
  const escape = await game.evaluate(() => ({ locked:__bf.controls.locked, waiting:__bf.menu.waiting }));
  assert.ok(escape.locked || escape.waiting);
  assert.deepEqual([...errors, ...battle.errors], []);
  console.log(JSON.stringify({pass:true,scope,chat:messages.map(m=>m.channel),escape}));
} finally { await browser.close(); }
