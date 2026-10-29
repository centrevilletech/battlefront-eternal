// PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs GAME_URL=http://127.0.0.1:18449 node tests/droideka-shield-worker-browser.mjs
// AI droidekas' shields in a battle shown from the world worker, stepped
// frame by frame (tests/battle-page.mjs): every bot keeps its bubble once
// packets arrive, and a shield the world raises is drawn where the droideka
// stands.
import { mkdirSync, writeFileSync } from 'node:fs';
import { launchBrowser, openBattle } from './battle-page.mjs';
const folder = process.env.SHIELD_OUTPUT || '/tmp/bf/droideka-shield-worker';
mkdirSync(folder, { recursive: true });
const browser = await launchBrowser({ args: ['--ignore-gpu-blocklist'] });
const deadline = setTimeout(() => { console.error('Droideka shield worker check timed out'); process.exit(1); }, 600000);
const failures = [];
try {
  const battle = await openBattle(browser, { base: process.env.GAME_URL || 'http://127.0.0.1:18449', viewport: { width: 800, height: 500 },
    query: 'map=nab2&side=cis&unit=cis_inf_droideka&ai=classic&seed=1&loading=0',
    settings: { upscaler: 'off', antialiasing: 'off', shadows: 'off', waterQuality: 'low' } });
  const { page, errors } = battle;
  // Two seconds of battle, and him alive (within two minutes).
  const started = await page.evaluate(async () => {
    for (let i = 0; i < 120 * 60 && (__bf.worldClock.tick <= 120 || !__bf.playerUnit.alive); i++) await __bf.lockstep(1 / 60, false);
    return __bf.worldClock.tick > 120 && __bf.playerUnit.alive;
  });
  if (!started) throw new Error('the battle did not start with him alive in two minutes');
  const kept = await page.evaluate(() => __bf.units.all().filter((u) => u.walker && u !== __bf.playerUnit).map((u) => ({ id: u.id, bubble: Boolean(u.bubble) })));
  if (!kept.length || kept.some((u) => !u.bubble)) failures.push(`bots without a bubble ${JSON.stringify(kept)}`);
  // An AI droideka with its shield up (within five minutes of battle), framed from five metres.
  const id = await page.evaluate(async () => {
    const find = () => __bf.units.all().find((u) => u.walker && u !== __bf.playerUnit && !u.removed && u.alive && u.soldier.shield?.on && u.soldier.mode === 0)?.id ?? null;
    for (let i = 0; i < 300 * 60 && find() === null; i++) await __bf.lockstep(1 / 60, false);
    return find();
  });
  if (id === null) throw new Error('no AI droideka raised its shield in five minutes');
  const result = await page.evaluate(async (id) => {
    const b = __bf, T = b.THREE, unit = b.units.byId(id), gl = b.renderer.getContext(), bubble = unit.bubble;
    const at = unit.presentation.position.clone().add(new T.Vector3(0, 1.1, 0)), yaw = unit.soldier.aimYaw;
    const from = at.clone().add(new T.Vector3(Math.sin(yaw) * 5, 1.2, Math.cos(yaw) * 5));
    b.spawnPort.setViewShot({ position: from.toArray(), quaternion: new T.Quaternion().setFromRotationMatrix(new T.Matrix4().lookAt(from, at, new T.Vector3(0, 1, 0))).toArray() });
    // The same posed frame drawn twice, the bubble shown then hidden.
    const grab = () => { b.graphics.render(1); const px = new Uint8Array(gl.drawingBufferWidth * gl.drawingBufferHeight * 4); gl.readPixels(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight, gl.RGBA, gl.UNSIGNED_BYTE, px); return px; };
    for (let i = 0; i < 20; i++) await b.lockstep(1 / 60, false);
    await b.lockstep(1 / 60, true);
    const shown = grab(), image = b.renderer.domElement.toDataURL('image/png');
    if (bubble) bubble.mesh.visible = false;
    const without = grab();
    let changed = 0;
    for (let i = 0; i < shown.length; i += 4) if (Math.abs(shown[i] - without[i]) + Math.abs(shown[i + 1] - without[i + 1]) + Math.abs(shown[i + 2] - without[i + 2]) > 12) changed++;
    return { id, tick: b.worldClock.tick, on: unit.soldier.shield.on, bubble: Boolean(bubble), fade: bubble?.fade ?? null, changed, image };
  }, id);
  writeFileSync(`${folder}/worker.png`, Buffer.from(result.image.split(',')[1], 'base64'));
  delete result.image;
  console.log(JSON.stringify({ kept: kept.length, ...result, errors }));
  if (result.changed < 2000) failures.push(`raised shield not drawn ${JSON.stringify(result)}`);
  if (errors.length) failures.push(errors.join('; '));
} finally { clearTimeout(deadline); await browser.close(); }
if (failures.length) { console.error(failures.join('\n')); process.exitCode = 1; }
else console.log(`ok: ${folder}`);
