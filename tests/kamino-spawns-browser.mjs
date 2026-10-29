// Requires Vite and the extracted Kamino map and CIS units.
// Every Kamino spawn point lands the soldier: stood at each authored point of
// each post's spawn path in the world worker and stepped half a second in
// lockstep with the page's frames (tests/battle-page.mjs), he starts near the
// authored height, stays put, overlaps nothing and stands on the ground. Then
// the formerly broken CIS bridge spawn, drawn looking toward the sea in the rain.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { launchBrowser, openBattle } from './battle-page.mjs';
const browser = await launchBrowser();
try {
  const battle = await openBattle(browser, { base: process.env.WEATHER_TEST_URL || 'http://127.0.0.1:18449',
    query: 'map=kam1&side=cis&unit=cis_inf_assault&shadows=off', viewport: { width: 1100, height: 750 }, settings: null });
  const { page, world, errors } = battle;
  // The world: every post the player's, so he may spawn at any, and the
  // battle can't end meanwhile (neither the tickets nor the other team's
  // clock with no post run out); put back afterwards.
  const counts = await world.evaluate(async () => {
    const b = self.__bf, map = await fetch('/maps/kam1/map.json').then(r => r.json());
    const { BODY } = await import('/src/soldier.js');
    const playerTeam = b.posts.posts.find(p => p.desc.name === 'cp5').team;
    self.__kamino = { map, BODY, teams: b.posts.posts.map(p => p.team), tickets: { ...b.battle.reinforcements }, noPost: { ...b.battle.noPost }, start: null };
    b.posts.posts.forEach(p => p.team = playerTeam);
    b.battle.reinforcements[b.battle.own] = b.battle.reinforcements[b.battle.foe] = 1000000;
    for (const team of b.battle.teamIds) b.battle.noPost[team] = 1000000;
    return b.posts.posts.map(p => map.spawns[p.desc.spawnPath].length);
  });
  const results = [];
  try {
    for (const [post, count] of counts.entries()) for (let i = 0; i < count; i++) {
      // The game's respawn at the post (its first point), then at point i:
      // respawn picks a random point from the seeded gameplay stream.
      await world.evaluate(([post, i]) => {
        const b = self.__bf, k = self.__kamino;
        b.respawn(post, false);
        b.soldier.spawn(...k.map.spawns[b.posts.posts[post].desc.spawnPath][i]);
        k.start = b.soldier.position.clone();
      }, [post, i]);
      await battle.frames(30);
      results.push(await world.evaluate(([post, i]) => {
        const b = self.__bf, k = self.__kamino, p = b.posts.posts[post], center = new b.THREE.Vector3();
        const blocked = k.BODY.parts.stand.some(part => b.collision.overlapsSphere(b.soldier.partCenter(part, center), part.r - .02));
        return { post: p.desc.name, index: i, authoredY: k.map.spawns[p.desc.spawnPath][i][1], startY: k.start.y, landedY: b.soldier.position.y,
          drift: b.soldier.position.distanceTo(k.start), blocked, grounded: b.soldier.grounded };
      }, [post, i]));
    }
  } finally {
    await world.evaluate(() => {
      const b = self.__bf, k = self.__kamino;
      b.posts.posts.forEach((p, i) => p.team = k.teams[i]); Object.assign(b.battle.reinforcements, k.tickets); Object.assign(b.battle.noPost, k.noPost);
    });
  }
  // A formerly broken CIS bridge spawn (CP5SpawnPath's second point), then
  // a look toward the sea: the world turns him, the page draws it.
  await world.evaluate(() => { const b = self.__bf; b.respawn(0, false); b.soldier.spawn(...self.__kamino.map.spawns.CP5SpawnPath[1]); });
  await page.evaluate(() => { const b = __bf; b.setPerspective(false); b.settings.set('softFocus', false); b.settings.set('rainEffects', 'enhanced'); });
  await battle.frames(120);
  await world.evaluate(() => { const s = self.__bf.soldier; s.aimYaw = Math.PI; s.aimPitch = -.3; });
  const { image, ...shown } = await page.evaluate(async () => {
    const b = __bf;
    await b.lockstep(1 / 60);
    b.presentation.reset();
    await b.lockstep(0, true);
    return { image: b.renderer.domElement.toDataURL(), weather: !!b.weather, ocean: !!b.water?.ocean, over: b.battle.over };
  });
  console.log(JSON.stringify({ metrics: { results, ...shown }, errors }));
  assert.equal(results.length, 64);
  for (const s of results) { assert.ok(Math.abs(s.startY - s.authoredY) < 2, JSON.stringify(s)); assert.ok(s.drift < .15, JSON.stringify(s)); assert.equal(s.blocked, false, JSON.stringify(s)); assert.ok(s.grounded, JSON.stringify(s)); }
  assert.equal(shown.over, false, 'the battle still on'); assert.ok(shown.weather && shown.ocean); assert.deepEqual(errors, []);
  await fs.writeFile('/tmp/bf-kamino-spawn-fixed.png', Buffer.from(image.split(',')[1], 'base64'));
} finally { await browser.close(); }
