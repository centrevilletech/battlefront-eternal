// The scoreboard in the real game, headless (SwiftShader), the world worker
// stepped with the page's frames (tests/battle-page.mjs); the battle's end,
// the kill and the death are set up in the world:
// - Tab mid-match on geo1 (the Classic AI's bots): held, the table shows
//   both rosters, bots included, and closes when Tab is let go after half
//   a second; tapped, it stays up until the next press is let go; Esc
//   puts the pause menu over it, and Esc again comes back to it; the HUD
//   hides under it;
// - with the spawn screen up, Tab picks posts and the table doesn't show;
// - the battle's end: a team out of reinforcements, the game over 2 s
//   later, and the end table 7 s after that; DONE and 30 s idle restart;
// - DETAILS (the selected row's record) and AWARDS, and BACK to the table;
// - the stat popup flashing after a kill; dead, only the reinforcements, the
//   messages ("... KILLED PLAYER 1") and the popup show;
// - screenshots at 1024x768 and 1280x720 into SCOREBOARD_SHOTS (default
//   /tmp/bf-punch/scoreboard): the table in play, the end table, the two
//   retail captures' tables rebuilt (tests/scoreboard-capture-fixture.js),
//   DETAILS, AWARDS, and the popup.
//
//   PLAYWRIGHT_MODULE=.../playwright/index.mjs node tests/scoreboard-browser.mjs
//   SCOREBOARD_SIZES=1024x768 SCOREBOARD_CHECKS=0 to narrow it;
//   SCOREBOARD_PARTS=play,captures,popup,death,spawn picks the parts.
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import { launchBrowser, openBattle } from "./battle-page.mjs";

const BASE = process.env.SCOREBOARD_TEST_URL || "http://127.0.0.1:18449";
const OUT = process.env.SCOREBOARD_SHOTS || "/tmp/bf-punch/scoreboard";
const SIZES = (process.env.SCOREBOARD_SIZES || "1024x768,1280x720").split(",").map((s) => s.split("x").map(Number));
const CHECKS = process.env.SCOREBOARD_CHECKS !== "0";
const PARTS = new Set((process.env.SCOREBOARD_PARTS || "play,captures,popup,death,spawn").split(","));

await fs.mkdir(OUT, { recursive: true });
const browser = await launchBrowser();
const failures = [];

// The battle's page (its own settings): { page, world, errors, frames, ... }
// (tests/battle-page.mjs), the board and the spawn screen on the check's clock.
// The Classic board (menus=classic): the battle's own is the page view.
async function open(query, width, height) {
  const battle = await openBattle(browser, { base: BASE, query: `${query}&menus=classic`, viewport: { width, height }, settings: null });
  await battle.page.evaluate(() => {
    const b = window.__bf;
    b.scoreboard.clock = "manual";
    if (b.spawnScreen) b.spawnScreen.clock = "manual";
    // The page reloads to restart; here it only notes it.
    b.scoreboardPort.restart = () => { window.__restarts = (window.__restarts || 0) + 1; };
  });
  return battle;
}

// World and board together, frame by frame.
const run = (page, frames, draw = false) => page.evaluate(async ({ frames, draw }) => {
  const b = window.__bf;
  for (let i = 0; i < frames; i++) {
    b.spawnScreen?.update(1 / 60);
    await b.lockstep(1 / 60, draw);
    b.scoreboard.update(1 / 60);
  }
}, { frames, draw });

// A frame drawn and read back in the same task (the canvas isn't kept).
async function shot(page, file) {
  const image = await page.evaluate(async () => {
    const b = window.__bf;
    await b.lockstep(0, true);
    return b.renderer.domElement.toDataURL("image/png");
  });
  await fs.writeFile(file, Buffer.from(image.split(",")[1], "base64"));
}

// Textures and atlases: a few drawn frames with pauses, so loads land.
async function settle(page) {
  for (let i = 0; i < 6; i++) {
    await page.evaluate(() => window.__bf.lockstep(0, true));
    await new Promise((r) => setTimeout(r, 400));
  }
}

// A living enemy soldier with a Character, as the world has him: the battle
// run on until one is up (half a minute at most). { id, name }.
async function livingEnemy(battle) {
  for (let i = 0; i < 60; i++) {
    const enemy = await battle.world.evaluate(() => {
      const b = self.__bf;
      const unit = b.units.all().find((u) => u.soldier && u.alive && u.team !== b.playerUnit.team && u.character);
      return unit ? { id: unit.id, name: unit.character.name } : null;
    });
    if (enemy) return enemy;
    await battle.frames(30);
  }
  throw new Error("no living enemy");
}

const state = (page) => page.evaluate(() => {
  const b = window.__bf;
  const board = b.scoreboard;
  const model = board.model;
  return {
    state: model.state, layer: b.hud.layer === board.view.layer, controls: b.controls.enabled,
    rows: [model.lists[1].length, model.lists[2].length], roster: [1, 2].map((t) => b.battle.characters.filter((c) => c.team === t).length),
    bots: model.lists[1].concat(model.lists[2]).filter((r) => !r.human).length, named: model.lists[1].concat(model.lists[2]).every((r) => r.name),
    side: model.side, selected: model.selected, local: model.row(model.side, model.selected)?.local ?? false,
    over: b.battle.over, gameOver: b.battle.gameOver, finished: b.battle.finished, restarts: window.__restarts || 0,
  };
});

async function check(name, fn) {
  if (!CHECKS) return;
  try {
    await fn();
    console.log(`ok - ${name}`);
  } catch (error) {
    failures.push(`${name}: ${error.message}`);
    console.log(`not ok - ${name}: ${error.message}`);
  }
}

try {
  // ---------------------------------------------------------- in play
  for (const [width, height] of PARTS.has("play") ? SIZES : []) {
    const { page, world, errors } = await open("map=geo1&ai=classic&side=cis", width, height);
    await run(page, 120);
    await check(`${width}x${height}: Tab held shows both rosters, bots included, and closes on release after 0.5 s`, async () => {
      await page.keyboard.down("Tab");
      await run(page, 10);
      let s = await state(page);
      assert.equal(s.state, "peek");
      assert.equal(s.layer, true, "the board is the HUD's layer");
      assert.equal(s.controls, false, "the soldier takes no input");
      assert.deepEqual(s.rows, s.roster, "every Character of both teams");
      assert.ok(s.bots > 40 && s.named, `bots listed by name (${s.bots})`);
      assert.equal(s.local, true, "the selection starts on the player's row");
      await run(page, 30); // 0.67 s held
      s = await state(page);
      assert.equal(s.state, "peek", "still held");
      await page.keyboard.up("Tab");
      await run(page, 1);
      s = await state(page);
      assert.equal(s.state, "closed", "released after half a second");
      assert.equal(s.layer, false);
      assert.equal(s.controls, true, "the soldier has his controls back");
    });
    await check(`${width}x${height}: a tap pins it until the next press is let go; Esc pauses over it`, async () => {
      await page.keyboard.down("Tab");
      await run(page, 6);
      await page.keyboard.up("Tab");
      await run(page, 60);
      let s = await state(page);
      assert.equal(s.state, "peek", "tapped: pinned");
      await page.keyboard.down("Tab");
      await run(page, 5);
      assert.equal((await state(page)).state, "peek", "held again: still up");
      await page.keyboard.up("Tab");
      await run(page, 1);
      assert.equal((await state(page)).state, "closed", "closed on the release");
      await page.keyboard.down("Tab");
      await page.keyboard.up("Tab");
      await run(page, 40);
      assert.equal((await state(page)).state, "peek");
      // Esc puts the pause menu over it (src/pause); Esc again comes back
      // to it; Tab held and let go closes it.
      await page.keyboard.press("Escape");
      await run(page, 1);
      assert.equal(await page.evaluate(() => window.__bf.menu.isOpen), true, "Esc pauses over it");
      assert.equal((await state(page)).state, "peek", "the table stays under the pause menu");
      await page.keyboard.press("Escape");
      await run(page, 1);
      assert.equal(await page.evaluate(() => window.__bf.menu.isOpen), false);
      assert.equal((await state(page)).state, "peek", "and is there again after it");
      await page.keyboard.down("Tab");
      await run(page, 5);
      await page.keyboard.up("Tab");
      await run(page, 1);
      assert.equal((await state(page)).state, "closed");
    });
    // The table in play, pinned, after some fighting.
    await run(page, 2400);
    await page.keyboard.down("Tab");
    await page.keyboard.up("Tab");
    await run(page, 40);
    await settle(page);
    await run(page, 40, true);
    await shot(page, `${OUT}/play-${width}x${height}.png`);
    await check(`${width}x${height}: DETAILS from Tab's table shows a bot's record during play`, async () => {
      // The world worker sends everyone's records every 5 s (docs/scoreboard.md).
      const record = await page.evaluate(() => {
        const m = window.__bf.scoreboard.model;
        const side = m.lists[1].some((r) => !r.human && r.deaths > 0) ? 1 : 2;
        m.select(side, m.lists[side].findIndex((r) => !r.human && r.deaths > 0));
        m.showDetails();
        return window.__bf.scoreboard.view.pages.details.content.map((r) => [r.label, r.value]);
      });
      await run(page, 2);
      const value = (label) => record.find(([l]) => l === label)?.[1];
      assert.ok(value("FAVORITE CLASS"), `his favourite class (${JSON.stringify(record)})`);
      assert.notEqual(value("LONGEST LIVING STREAK"), " 0:00");
      assert.ok(!record.some(([l]) => l === "CONNECTION"), "offline nobody has a connection");
      await page.evaluate(() => window.__bf.scoreboard.model.back());
      await run(page, 2);
    });
    await page.keyboard.down("Tab");
    await run(page, 5);
    await page.keyboard.up("Tab");
    await run(page, 1);

    // The battle's end, as the world runs it.
    await check(`${width}x${height}: the end table follows the game over, and DONE restarts`, async () => {
      await world.evaluate(() => {
        const b = self.__bf;
        b.battle.reinforcements[b.battle.foe] = 0;
      });
      await run(page, 2);
      let s = await state(page);
      assert.equal(s.over, true);
      assert.equal(s.state, "closed", "the banner first");
      await page.keyboard.down("Tab");
      await page.keyboard.up("Tab");
      await run(page, 30);
      assert.equal((await state(page)).state, "peek", "Tab still works until the game is over");
      await run(page, 95);
      s = await state(page);
      assert.equal(s.gameOver, true, "game over 2 s after");
      assert.equal(s.state, "closed", "the game over takes Tab's table down");
      await page.keyboard.down("Tab");
      await page.keyboard.up("Tab");
      await run(page, 30);
      assert.equal((await state(page)).state, "closed", "and Tab no longer shows it");
      await run(page, 7 * 60 - 30);
      s = await state(page);
      assert.equal(s.finished, true);
      assert.equal(s.state, "end", "the end table 7 s later");
      const scores = await page.evaluate(() => window.__bf.battle.characters.map((c) => c.kills + c.deaths + c.captures).join());
      await run(page, 120);
      assert.equal(await page.evaluate(() => window.__bf.battle.characters.map((c) => c.kills + c.deaths + c.captures).join()), scores, "the scores are frozen");
    });
    await settle(page);
    await run(page, 40, true);
    await shot(page, `${OUT}/end-${width}x${height}.png`);
    await check(`${width}x${height}: DETAILS shows the selected row's record; AWARDS the winners; BACK returns`, async () => {
      const click = (kind) => page.evaluate((kind) => {
        const board = window.__bf.scoreboard;
        const box = board.view.buttonBox(kind);
        board.input.click(box.x0 + 4, box.top + box.height / 2);
      }, kind);
      await click("details");
      await run(page, 30, true);
      let page1 = await page.evaluate(() => ({ page: window.__bf.scoreboard.model.page, rows: window.__bf.scoreboard.view.pages.details.content.map((r) => r.label) }));
      assert.equal(page1.page, "details");
      assert.deepEqual(page1.rows, ["HEAD SHOTS", "FAVORITE CLASS", "FAVORITE WEAPON", "FAVORITE VEHICLE", "LONGEST LIVING STREAK", "NARROW ESCAPES", "VEHICLES DESTROYED", "NEMESIS", "BAIT"]);
      await settle(page);
      await run(page, 30, true);
      await shot(page, `${OUT}/details-${width}x${height}.png`);
      await page.keyboard.press("Escape");
      await run(page, 2);
      assert.equal(await page.evaluate(() => window.__bf.scoreboard.model.page), "table", "Esc goes back");
      assert.equal((await state(page)).restarts, 0, "and doesn't end the table");
      await click("awards");
      await run(page, 30, true);
      page1 = await page.evaluate(() => ({ page: window.__bf.scoreboard.model.page, rows: window.__bf.scoreboard.view.pages.awards.content }));
      assert.equal(page1.page, "awards");
      assert.deepEqual(page1.rows.map((r) => r.label), ["TANK BUSTER", "KILLING SPREE", "CAMPER", "SURVIVALIST", "TRAITOR", "PUBLIC ENEMY", "BANTHA FODDER", "DEAD EYE"]);
      await settle(page);
      await run(page, 30, true);
      await shot(page, `${OUT}/awards-${width}x${height}.png`);
      await click("back");
      await run(page, 2);
      assert.equal(await page.evaluate(() => window.__bf.scoreboard.model.page), "table", "BACK goes back");
    });
    await check(`${width}x${height}: DONE and 30 s idle restart the battle`, async () => {
      const done = await page.evaluate(() => {
        const view = window.__bf.scoreboard.view;
        const box = view.buttonBox("done");
        return { x: box.x0 + 4, y: box.top + box.height / 2 };
      });
      await page.evaluate(({ x, y }) => window.__bf.scoreboard.input.click(x, y), done);
      assert.equal((await state(page)).restarts, 1, "DONE");
      await page.evaluate(() => { window.__restarts = 0; });
      // A new page would follow; here the same board's idle clock runs out.
      await page.evaluate(() => {
        const board = window.__bf.scoreboard;
        board.model.close();
        board.model.open("end");
      });
      await run(page, 29 * 60);
      assert.equal((await state(page)).restarts, 0, "29 s: still up");
      await run(page, 90);
      assert.equal((await state(page)).restarts, 1, "30 s without input");
    });
    if (CHECKS) assert.deepEqual(errors, [], `${width}x${height} page errors`);
    await page.close();
  }

  // ---------------------------------------------------------- the captures
  // Each retail capture's table rebuilt; Hoth's DETAILS and AWARDS too.
  const QUERIES = { geo1: "map=geo1&ai=classic&side=cis", nab1: "map=nab2&ai=classic&side=rep", hot1: "map=hot1&ai=classic&side=imp" };
  for (const which of PARTS.has("captures") ? ["geo1", "nab1", "hot1"] : []) {
    for (const [width, height] of SIZES) {
      const { page } = await open(QUERIES[which], width, height);
      await run(page, 30);
      await page.evaluate(async (which) => {
        const b = window.__bf;
        const { capturePort } = await import("/tests/scoreboard-capture-fixture.js");
        const board = b.scoreboard;
        board.model.port = board.view.port = capturePort(b.scoreboardPort, which);
        board.model.open("end");
      }, which);
      await settle(page);
      await run(page, 60, true);
      await page.evaluate(() => window.__bf.scoreboard.input.move(512, 420));
      await run(page, 1);
      await shot(page, `${OUT}/capture-${which}-${width}x${height}.png`);
      if (which === "hot1") {
        for (const kind of ["details", "awards"]) {
          await page.evaluate((kind) => {
            const board = window.__bf.scoreboard;
            if (kind === "details") board.model.showDetails();
            else board.model.showAwards();
          }, kind);
          await settle(page);
          await run(page, 60, true);
          await page.evaluate(() => window.__bf.scoreboard.input.move(512, 680));
          await run(page, 1);
          await shot(page, `${OUT}/capture-hot1-${kind}-${width}x${height}.png`);
          await page.evaluate(() => window.__bf.scoreboard.model.back());
        }
      }
      await page.close();
    }
  }

  // ---------------------------------------------------------- the popup
  if (PARTS.has("popup")) {
    const battle = await open("map=geo1&ai=classic&side=cis", 1024, 768);
    const { page, world } = battle;
    await check("the stat popup flashes after a kill", async () => {
      // The player kills him in the world; the page's HUD has it a tick later.
      const enemy = await livingEnemy(battle);
      await world.evaluate((id) => {
        const b = self.__bf;
        b.units.byId(id).combat.damage(Infinity, Infinity, null, b.playerUnit, { known: true });
      }, enemy.id);
      const popup = await page.evaluate(async () => {
        const b = window.__bf;
        await b.lockstep(1 / 60, false);
        return { values: b.hud.statValues.slice(), left: b.hud.statsLeft, flash: b.hud.statFlash.slice() };
      });
      assert.equal(popup.values[0], 1, "KILLS 1");
      assert.ok(popup.left > 2.9 && popup.flash[0] > 0.9, "shown and flashing");
    });
    await settle(page);
    await shot(page, `${OUT}/popup-flash-1024x768.png`);
    await run(page, 45);
    await shot(page, `${OUT}/popup-fading-1024x768.png`);
    await page.close();
  }

  // ---------------------------------------------------------- dead
  if (PARTS.has("death")) {
    const battle = await open("map=geo1&ai=classic&side=cis", 1024, 768);
    const { page, world, errors } = battle;
    await check("dead: no minimap, weapons or health; the feed names PLAYER 1; the popup shows DEATHS", async () => {
      // He kills the player in the world; the page shows it twelve frames on.
      const enemy = await livingEnemy(battle);
      await world.evaluate((id) => {
        const b = self.__bf;
        b.combat.damage(Infinity, Infinity, null, b.units.byId(id));
      }, enemy.id);
      const dead = await page.evaluate(async () => {
        const b = window.__bf;
        for (let i = 0; i < 12; i++) await b.lockstep(1 / 60, false);
        return { dead: b.hud.player?.dead, message: b.hud.messages.at(-1)?.text, values: b.hud.statValues.slice(), flash: b.hud.statFlash.slice() };
      });
      assert.equal(dead.dead, true);
      assert.equal(dead.message, `${enemy.name.toUpperCase()} KILLED PLAYER 1`);
      assert.equal(dead.values[1], 1, "DEATHS 1");
      assert.ok(dead.flash[1] > 0.7, `DEATHS flashing (${dead.flash[1]})`);
    });
    await settle(page);
    await shot(page, `${OUT}/dead-1024x768.png`);
    if (CHECKS) assert.deepEqual(errors, [], "death page errors");
    await page.close();
  }

  // ---------------------------------------------------------- the spawn screen
  if (PARTS.has("spawn")) {
    const { page, errors } = await open("map=geo1&ai=classic", 1024, 768);
    await run(page, 30);
    await check("with the spawn screen up, Tab picks posts and no table shows", async () => {
      const before = await page.evaluate(() => window.__bf.spawnScreen.model.post);
      await page.keyboard.down("Tab");
      await run(page, 40);
      await page.keyboard.up("Tab");
      await run(page, 2);
      const s = await state(page);
      assert.equal(s.state, "closed");
      assert.equal(await page.evaluate(() => window.__bf.spawnScreen.isOpen), true);
      assert.ok(before !== null);
    });
    if (CHECKS) assert.deepEqual(errors, [], "spawn screen page errors");
    await page.close();
  }
} finally {
  await browser.close();
}

if (failures.length) {
  console.error(`${failures.length} failed:\n${failures.join("\n")}`);
  process.exit(1);
}
console.log(`screenshots in ${OUT}`);
