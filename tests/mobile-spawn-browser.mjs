// Hoth mobile command post acceptance: Hoth's spawn screen offers the AT-ATs; SPAWN puts him beside one.
import fs from 'node:fs/promises';
import assert from 'node:assert/strict';
import { launchBrowser, openBattle } from './battle-page.mjs';
const BASE = process.env.HOTH_URL || 'http://127.0.0.1:18501';
const OUT = process.env.HOTH_SHOTS || '/tmp/bfe-mobile-spawn';
await fs.mkdir(OUT, { recursive: true });
const browser = await launchBrowser();
const shot = async (page, file) => {
  const image = await page.evaluate(async () => { await window.__bf.lockstep(1 / 60, true); return window.__bf.renderer.domElement.toDataURL('image/png'); });
  await fs.writeFile(file, Buffer.from(image.split(',')[1], 'base64'));
};
try {
  // The battle page's lockstep can lose a race with the world worker's
  // first tick (it throws at once); a fresh page then starts clean.
  let page, errors;
  for (let attempt = 1; ; attempt++) {
    ({ page, errors } = await openBattle(browser, { base: BASE, query: 'map=hot1&ai=classic&shadows=off&spawnscreen=1&side=imp', viewport: { width: 1024, height: 768 }, settings: null }));
    await page.evaluate(() => { if (window.__bf.spawnScreen) window.__bf.spawnScreen.clock = 'manual'; });
    const first = await page.evaluate(async () => { try { await window.__bf.lockstep(1 / 60, false); return null; } catch (e) { return e.message; } });
    if (!first) break;
    console.log('attempt', attempt, first);
    await page.close();
    if (attempt >= 6) throw new Error(first);
  }
  const run = (frames, wait = 0) => page.evaluate(async ({ frames, wait }) => {
    const b = window.__bf;
    for (let i = 0; i < frames; i++) {
      if (wait) await new Promise((r) => setTimeout(r, wait));
      b.spawnScreen?.update(1 / 60);
      await b.lockstep(1 / 60, true);
    }
  }, { frames, wait });
  await run(60, 40);
  const state = () => page.evaluate(() => {
    const b = window.__bf, m = b.spawnScreen?.model;
    return { open: Boolean(b.spawnScreen?.isOpen), team: m?.team, post: m?.post, human: b.spawnPort.humanState(),
      posts: b.spawnPort.posts(m?.team ?? 1).map((p) => ({ name: p.name, label: p.displayName, owner: p.owner, eligible: p.eligible, at: p.position.map((n) => Math.round(n)) })) };
  });
  const s0 = await state();
  console.log(JSON.stringify({ open: s0.open, team: s0.team, post: s0.post, eligible: s0.posts.filter((p) => p.eligible) }));
  await shot(page, `${OUT}/hoth-open.png`);
  // Tab until an AT-AT is picked.
  for (let i = 0; i < 4; i++) {
    const s = await state();
    if (s.post?.startsWith('transport-')) break;
    // Tab picks the next post while it is held (src/spawn-screen/input.js).
    await page.evaluate(() => __bf.spawnScreen.model.cyclePost());
    await run(3);
  }
  const s1 = await state();
  assert.ok(s1.post?.startsWith('transport-'), 'the screen selects a mobile walker post');
  console.log(JSON.stringify({ picked: s1.post, label: s1.posts.find((p) => p.name === s1.post)?.label }));
  await run(20, 20);
  await shot(page, `${OUT}/hoth-atat-picked.png`);
  const spawnAt = await page.evaluate(() => {
    const b = window.__bf, layout = b.spawnScreen.view.layout, rect = b.renderer.domElement.getBoundingClientRect();
    return { x: rect.left + (layout.spawn.x / 1024) * rect.width, y: rect.top + (layout.spawn.y / b.hud.height) * rect.height };
  });
  await page.mouse.move(spawnAt.x, spawnAt.y);
  await page.mouse.down();
  await page.mouse.up();
  await run(60, 20);
  const s2 = await state();
  const near = await page.evaluate(() => {
    const b = window.__bf, me = b.playerUnit.position;
    return b.vehicles.vehicles.filter((v) => v.alive && v.base === 'commandwalker').map((v) => ({ name: v.name, distance: Math.round(me.distanceTo(v.position) * 10) / 10 }));
  });
  console.log(JSON.stringify({ phase: s2.human.phase, post: s2.human.post, open: s2.open, walkers: near, errors }));
  assert.equal(s2.human.phase, 'alive');
  assert.ok(near.some(v => v.distance < 12), 'spawned on the ground beside a walker');
  assert.deepEqual(errors, []);
  await run(30, 10);
  await shot(page, `${OUT}/hoth-spawned.png`);
} finally {
  await browser.close();
}
