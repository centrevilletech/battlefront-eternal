// Real Classic actors and the game's HUD hooks, held in a controlled scene.
// Run against Vite after extraction. Screenshots/reports stay outside git.
// AI_TEST_URL, AI_TEST_ARTIFACTS and PLAYWRIGHT_MODULE as the other checks.
// The battle runs in the world worker, stepped frame by frame
// (tests/battle-page.mjs): the scene is set up, and the shots, hits and
// kills made, in the world; the page's HUD shows them from its packets.
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import { launchBrowser, openBattle } from "./battle-page.mjs";

const base = process.env.AI_TEST_URL || "http://127.0.0.1:18449";
const out = process.env.AI_TEST_ARTIFACTS || "/tmp/bf-ai-hud";
await fs.mkdir(out, { recursive: true });
const browser = await launchBrowser();
try {
  const battle = await openBattle(browser, { base, query: "map=nab2&unit=rep_inf_clone_trooper&ai=classic&tier=medium&seed=1&shadows=off",
    viewport: { width: 1280, height: 720 }, settings: null, errors: (text) => !/Failed to load resource.*404/.test(text) });
  const { page, world, errors } = battle;
  page.on("pageerror", (error) => console.error(error.stack));

  // Let the normal spawn manager supply real characters and combat hooks.
  await page.evaluate(async () => {
    const bf = window.__bf, player = bf.playerUnit;
    const teamReady = (friendly) => bf.units.all().some((u) => u.alive && u.soldier && !u.human && (u.team === player.team) === friendly);
    for (let i = 0; i < 900 && !(teamReady(true) && teamReady(false)); i++) await bf.lockstep(1 / 60, false);
  });
  // Freeze bot decisions/physics only; the world's tick still performs the
  // ordinary battle and event work, and the page its reticle and HUD. No
  // competing bot gunfire.
  const staged = await world.evaluate(() => {
    const bf = self.__bf, { THREE, units, playerUnit: player } = bf;
    const soldiers = units.all().filter((u) => u.alive && u.soldier && !u.human);
    const enemy = soldiers.find((u) => u.team !== player.team);
    const friend = soldiers.find((u) => u.team === player.team);
    if (!enemy || !friend) throw new Error("Classic did not supply both teams for HUD test");
    bf.ai.update = () => {};
    for (const u of soldiers) {
      u.tick = () => {};
      u.intent.buttons = [0, 0];
      if (u !== enemy && u !== friend) u.remove();
    }
    const hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
    let yaw = null;
    const eye = player.origin(new THREE.Vector3());
    for (let i = 0; i < 16; i++) {
      const angle = i * Math.PI / 8;
      if (!bf.collision.cast(eye, new THREE.Vector3(Math.sin(angle), 0, Math.cos(angle)), 16, hit)) { yaw = angle; break; }
    }
    if (yaw === null) throw new Error("No clear reticle test direction at spawn");
    // His aim stays where it is set: the page's commands carry no look.
    bf.soldier.aimYaw = yaw; bf.soldier.aimPitch = 0;
    bf.combat.aim();
    const center = bf.combat.origin.clone().addScaledVector(bf.combat.aimDir, 12);
    const place = (unit, point) => {
      const offset = unit.sphere.center.clone().sub(unit.position);
      unit.soldier.position.copy(point).sub(offset); unit.tickCount++;
      unit.presentation.reset();
    };
    place(enemy, center);
    place(friend, center.clone().add(new THREE.Vector3(Math.cos(yaw) * 6, 0, -Math.sin(yaw) * 6)));
    enemy.character.name = "HUD Enemy"; friend.character.name = "HUD Friend";
    bf.battle.reinforcements[enemy.team] = 1000;
    bf.battle.reinforcements[player.team] = 1000;
    self.__hudTest = { enemy, friend };
    return { enemy: enemy.id, friend: friend.id };
  });
  const initial = await page.evaluate(async ({ enemy: enemyId, friend: friendId }) => {
    const bf = window.__bf, hud = bf.hud, enemy = bf.units.byId(enemyId), friend = bf.units.byId(friendId);
    hud.radar.states = new WeakMap();
    await bf.lockstep(1 / 60, false);
    enemy.presentation.reset(); friend.presentation.reset();
    window.__hudTest = { enemy, friend };
    return { targetRelation: hud.targetRelation, enemyBar: hud.enemy?.fill, friendRadar: hud.radar.icons.some((i) => i.unit === friend), enemyHidden: !hud.radar.icons.some((i) => i.unit === enemy) };
  }, staged);
  assert.equal(initial.targetRelation, "enemy", "main reticle selects live enemy");
  assert.equal(initial.enemyBar, 1, "main enemy health hook");
  assert.equal(initial.friendRadar, true, "friendly registry actor on radar");
  assert.equal(initial.enemyHidden, true, "Medium hides unrevealed enemy");

  // The game's own combat event adapters and damage method, in the world;
  // what they tell the HUD reaches the page with the next tick.
  await world.evaluate(() => {
    const bf = self.__bf, { enemy } = self.__hudTest;
    enemy.combat.onFired({ weapon: enemy.combat.arsenal.current(0) });
    enemy.combat.damage(10, 10, null, bf.playerUnit, { known: true });
  });
  const damaged = await page.evaluate(async () => {
    const bf = window.__bf, { enemy } = window.__hudTest, hud = bf.hud;
    await bf.lockstep(1 / 60, false);
    const revealed = hud.radar.icons.find((i) => i.unit === enemy);
    return { revealed: Boolean(revealed), alpha: revealed?.color[3], ring: hud.hitRing.color(), health: hud.enemy?.fill, kills: bf.battle.human.kills };
  });
  await world.evaluate(() => {
    const bf = self.__bf, { enemy } = self.__hudTest;
    enemy.combat.damage(Infinity, Infinity, null, bf.playerUnit, { known: true });
  });
  const killed = await page.evaluate(async (beforeKills) => {
    const bf = window.__bf, hud = bf.hud;
    await bf.lockstep(1 / 60, false);
    return { kills: bf.battle.human.kills - beforeKills, message: hud.messages.at(-1)?.text, stats: hud.statValues.slice(), shown: hud.statsLeft > 0 };
  }, damaged.kills);
  delete damaged.kills;
  await world.evaluate(() => {
    const bf = self.__bf, { friend } = self.__hudTest;
    friend.combat.damage(10, 10, null, bf.playerUnit, { known: true });
  });
  const events = await page.evaluate(async ([damaged, killed]) => {
    const bf = window.__bf, hud = bf.hud;
    await bf.lockstep(1 / 60, false);
    const mixedRing = hud.hitRing.color();
    hud.squadCommand("follow");
    return { damaged, killed, mixedRing, command: hud.command };
  }, [damaged, killed]);
  assert.equal(events.damaged.revealed, true, "main shot hook reveals enemy");
  assert.ok(events.damaged.alpha > .9, "fresh radar reveal");
  assert.ok(events.damaged.ring[0] > .8 && events.damaged.ring[3] > .4, "main damage hook makes red hit ring");
  assert.ok(events.damaged.health < 1 && events.damaged.health > 0, "enemy health follows damage");
  assert.equal(events.killed.kills, 1, "real death credits human Character");
  assert.match(events.killed.message, /PLAYER 1 KILLED HUD ENEMY/, "main death hook posts localized feed");
  assert.equal(events.killed.shown, true, "Character stat change wakes popup");
  assert.ok(events.mixedRing[0] > .8 && events.mixedRing[1] > .8, "friend hit mixes with enemy ring");
  assert.equal(events.command, 2, "follow command display");

  // Resolve asynchronous texture loads before rendering the review artifact.
  await page.waitForFunction(() => {
    const hud = window.__bf.hud;
    return ["hit_indicator", "troop_icon", "stats_kills", "stats_deaths", "stats_cpp"].every((name) => hud.texture(name)?.userData.loaded);
  }, null, { timeout: 30000 });
  const image = await page.evaluate(async () => {
    const bf = window.__bf;
    await bf.lockstep(0, true);
    return { data: bf.renderer.domElement.toDataURL("image/png"), quads: bf.hud.batch.count };
  });
  assert.ok(image.quads > 40, "HUD geometry drawn");
  await fs.writeFile(`${out}/feedback.png`, Buffer.from(image.data.split(",")[1], "base64"));

  const faded = await page.evaluate(async () => {
    const bf = window.__bf;
    for (let i = 0; i < 181; i++) await bf.lockstep(1 / 60, false);
    await bf.lockstep(0, true);
    return { ring: bf.hud.hitRing.color()[3], enemy: bf.hud.enemyLeft, stats: bf.hud.statsLeft, command: bf.hud.commandLeft, data: bf.renderer.domElement.toDataURL("image/png") };
  });
  assert.equal(faded.ring, 0, "hit feedback expires");
  assert.equal(faded.enemy, 0, "enemy health fades after target death");
  assert.equal(faded.stats, 0, "stats popup expires");
  assert.equal(faded.command, 0, "squad command fades");
  await fs.writeFile(`${out}/faded.png`, Buffer.from(faded.data.split(",")[1], "base64"));
  delete faded.data;
  const fixture = await world.evaluate(() => {
    const bf = self.__bf, { enemy, friend } = self.__hudTest, player = bf.playerUnit;
    const post = bf.posts.posts.find((p) => p.capture && p.team === player.team);
    if (!post) throw new Error("No owned capture region for deterministic fixture");
    const center = new bf.THREE.Vector3().setFromMatrixPosition(post.capture.inverse.clone().invert());
    // Two ordinary allied capturers and one hostile hero who cannot
    // contribute. Native capture credit has a separate, broader award pass.
    player.soldier.spawn(center.x, center.y, center.z, 0);
    player.presentation.reset(); player.tickCount++;
    friend.spawn(center.x + .5, center.y, center.z, 0);
    enemy.classDef = { ...enemy.classDef, CapturePosts: 0 };
    enemy.hero = true;
    enemy.spawn(center.x - .5, center.y, center.z, 0);
    enemy.character.unit = enemy;
    enemy.character.captures = 65535;
    // Population hooks can refill removed slots during the fade interval;
    // remove those new actors before counting this isolated capture.
    for (const other of bf.units.all()) {
      if (other.soldier && other !== player && other !== friend && other !== enemy && !other.removed) other.remove();
    }
    const before = [player, friend, enemy].map((u) => u.character.captures || 0);
    post.team = 0; post.toward = player.team; post.progress = .999999; post.neutralize = 0;
    return { post: bf.posts.posts.indexOf(post), before };
  });
  const shown = await page.evaluate(async () => {
    const bf = window.__bf;
    await bf.lockstep(1 / 60, false);
    return { popup: bf.hud.statValues[2], popupVisible: bf.hud.statsLeft > 0, message: bf.hud.messages.at(-1)?.text };
  });
  const capture = await world.evaluate(({ post: index, before }) => {
    const bf = self.__bf, { enemy, friend } = self.__hudTest, player = bf.playerUnit, post = bf.posts.posts[index];
    return { team: post.team, expectedTeam: player.team, contributors: post.counts.get(player.team),
      hostileContributors: post.counts.get(enemy.team) || 0,
      before, after: [player, friend, enemy].map((u) => u.character.captures || 0) };
  }, fixture);
  Object.assign(capture, shown);
  capture.data = await page.evaluate(async () => {
    const bf = window.__bf;
    bf.presentation.reset();
    await bf.lockstep(0, true);
    return bf.renderer.domElement.toDataURL("image/png");
  });
  assert.equal(capture.team, capture.expectedTeam, "actual command post capture");
  assert.equal(capture.contributors, 2, "two allied soldiers contribute");
  assert.equal(capture.hostileContributors, 0, "hostile non-capturing hero does not contest");
  assert.equal(capture.after[0], capture.before[0] + 1, "player gets capture credit");
  assert.equal(capture.after[1], capture.before[1] + 1, "second capturer also gets credit");
  assert.equal(capture.after[2], 0, "non-contributing hostile Character gets credit with uint16 wrap");
  assert.equal(capture.popup, capture.after[0], "capture credit reaches HUD");
  assert.equal(capture.popupVisible, true, "capture change wakes popup");
  assert.match(capture.message, /CAPTURED/, "capture message uses existing feed");
  await fs.writeFile(`${out}/capture.png`, Buffer.from(capture.data.split(",")[1], "base64"));
  delete capture.data;
  const report = { initial, events, faded, capture, errors };
  await fs.writeFile(`${out}/report.json`, JSON.stringify(report, null, 2));
  assert.deepEqual(errors, [], "browser errors");
  console.log(JSON.stringify(report));
} finally {
  await browser.close();
}
