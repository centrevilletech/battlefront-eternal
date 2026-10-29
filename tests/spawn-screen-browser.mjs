// The spawn screen in the real game, headless (SwiftShader), the world
// worker stepped with the page's frames (tests/battle-page.mjs); the
// screen's choices reach the world as a player's do, with the next command:
// - renders it as the map loads on geo1 (Republic), kam1 (CIS) and hot1
//   (Empire), at 1024x768 and 1280x720, into SPAWN_SHOTS (default
//   /tmp/bf-punch/spawn-screen/<map>-<w>x<h>.png);
// - on geo1: a class and post picked and SPAWN pressed spawn him there; a
//   team switch moves the logos, map and reinforcements to the other team;
//   a death reopens the screen about 4 s later on a camera shot;
// - with unit= the screen is never built.
//
//   PLAYWRIGHT_MODULE=.../playwright/index.mjs node tests/spawn-screen-browser.mjs
//   SPAWN_MAPS=geo1 SPAWN_SIZES=1024x768 SPAWN_CHECKS=0 to narrow it.
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import { launchBrowser, openBattle } from "./battle-page.mjs";

const BASE = process.env.SPAWN_TEST_URL || "http://127.0.0.1:18449";
const OUT = process.env.SPAWN_SHOTS || "/tmp/bf-punch/spawn-screen";
const MAPS = (process.env.SPAWN_MAPS || "geo1,kam1,hot1").split(",");
const SIZES = (process.env.SPAWN_SIZES || "1024x768,1280x720").split(",").map((s) => s.split("x").map(Number));
const CHECKS = process.env.SPAWN_CHECKS !== "0";
// The team each reference screenshot shows: the screen forced on with the
// side named (side= alone would skip it).
const QUERY = { geo1: "", kam1: "&spawnscreen=1&side=cis", hot1: "&spawnscreen=1&side=imp" };

await fs.mkdir(OUT, { recursive: true });
const browser = await launchBrowser();
const failures = [];

// The battle's page (its own settings), the screen on the check's clock.
async function open(query, width, height) {
  const battle = await openBattle(browser, { base: BASE, query, viewport: { width, height }, settings: null });
  await battle.page.evaluate(() => {
    const b = window.__bf;
    if (b.spawnScreen) b.spawnScreen.clock = "manual";
  });
  return battle;
}

// Frames of world and screen together, with a pause each for textures.
const run = (page, frames, wait = 0) => page.evaluate(async ({ frames, wait }) => {
  const b = window.__bf;
  for (let i = 0; i < frames; i++) {
    if (wait) await new Promise((r) => setTimeout(r, wait));
    b.spawnScreen?.update(1 / 60);
    await b.lockstep(1 / 60, true);
  }
}, { frames, wait });

// A frame drawn and read back in the same task (the canvas isn't kept).
const shot = async (page, file) => {
  const image = await page.evaluate(async () => {
    await window.__bf.lockstep(1 / 60, true);
    return window.__bf.renderer.domElement.toDataURL("image/png");
  });
  await fs.writeFile(file, Buffer.from(image.split(",")[1], "base64"));
};

const state = (page) => page.evaluate(() => {
  const b = window.__bf;
  const screen = b.spawnScreen;
  const human = b.spawnPort.humanState();
  return {
    open: Boolean(screen?.isOpen), layer: Boolean(b.hud.layer), human,
    team: screen?.model.team, className: screen?.model.className, post: screen?.model.post,
    roster: screen?.model.roster().map((r) => r.displayName), teams: b.hud.layer?.teams ?? null,
    mapTeam: b.hud.layer?.battlefield?.team ?? null, controls: b.controls.enabled,
  };
});

try {
  for (const map of MAPS) {
    for (const [width, height] of SIZES) {
      const { page, errors } = await open(`map=${map}&ai=off&shadows=off${QUERY[map] ?? ""}`, width, height);
      // Textures, atlases and the bodies arrive; the intro plays through.
      await run(page, 60, 40);
      const file = `${OUT}/${map}-${width}x${height}.png`;
      await shot(page, file);
      const s = await state(page);
      console.log(JSON.stringify({ map, width, height, file, open: s.open, team: s.team, className: s.className, post: s.post, roster: s.roster, errors }));
      try {
        assert.ok(s.open && s.layer, "the screen is open at load");
        assert.equal(s.controls, false, "the game's controls are off while it is open");
        assert.equal(s.roster.length, 5);
        assert.deepEqual(errors, []);
      } catch (error) {
        failures.push(`${map} ${width}x${height}: ${error.message}`);
      }
      if (CHECKS && map === MAPS[0] && width === SIZES[0][0]) {
        try {
          await checks(page, errors);
        } catch (error) {
          failures.push(`${map} checks: ${error.stack}`);
        }
      }
      await page.close();
    }
  }
  if (CHECKS) {
    const { page, errors } = await open(`map=${MAPS[0]}&unit=rep_inf_clone_trooper&ai=off&shadows=off`, 1024, 768);
    await run(page, 30);
    const bypass = await page.evaluate(() => ({ screen: Boolean(window.__bf.spawnScreen), layer: Boolean(window.__bf.hud.layer), phase: window.__bf.spawnPort.humanState().phase }));
    console.log(JSON.stringify({ bypass, errors }));
    try {
      assert.deepEqual(bypass, { screen: false, layer: false, phase: "alive" }, "unit= never shows the screen");
    } catch (error) {
      failures.push(`bypass: ${error.message}`);
    }
    await page.close();
  }
} finally {
  await browser.close();
}

// The screen driven as a player would: a team switch and back, a class and
// a post by clicks, SPAWN, then a death.
async function checks(page, errors) {
  const before = await state(page);
  // Left switches team: the logos, the map and the counter follow.
  await page.keyboard.press("ArrowLeft");
  await run(page, 3);
  const switched = await state(page);
  assert.notEqual(switched.team, before.team);
  assert.equal(switched.teams.own, switched.team);
  assert.equal(switched.mapTeam, switched.team);
  assert.equal(switched.human.team, before.human.team, "nothing reaches the world before SPAWN");
  await run(page, 3);
  await page.keyboard.press("ArrowRight");
  await run(page, 3);
  // A click on the fourth class window, then Tab for the next post.
  const picked = await page.evaluate(() => {
    const b = window.__bf;
    const screen = b.spawnScreen;
    const slot = screen.view.layout.slots[3];
    const rect = b.renderer.domElement.getBoundingClientRect();
    const at = (x, y) => ({ x: rect.left + (x / 1024) * rect.width, y: rect.top + (y / b.hud.height) * rect.height });
    return { slot: at(slot.x, slot.y), spawn: at(screen.view.layout.spawn.x, screen.view.layout.spawn.y) };
  });
  await page.mouse.move(picked.slot.x, picked.slot.y);
  await page.mouse.down();
  await page.mouse.up();
  await run(page, 2);
  const chosen = await state(page);
  const fourth = await page.evaluate(() => window.__bf.spawnScreen.model.roster()[3].className);
  assert.equal(chosen.team, before.team);
  assert.equal(chosen.className, fourth);
  // SPAWN: he spawns as that class at that post, and the screen closes.
  await page.mouse.move(picked.spawn.x, picked.spawn.y);
  await page.mouse.down();
  await page.mouse.up();
  await run(page, 6);
  const spawned = await state(page);
  console.log(JSON.stringify({ spawned: spawned.human, open: spawned.open }));
  assert.equal(spawned.open, false);
  assert.equal(spawned.layer, false);
  assert.equal(spawned.human.phase, "alive");
  assert.equal(spawned.human.className, chosen.className);
  assert.equal(spawned.human.post, chosen.post);
  assert.equal(spawned.controls, true);
  // The pause menu's RESPAWN (src/pause), with its question: a death.
  await page.keyboard.press("Escape");
  await run(page, 1);
  assert.equal(await page.evaluate(() => window.__bf.menu.isOpen), true, "Esc pauses");
  const offered = await page.evaluate(() => window.__bf.menu.screen.widget("pause").visibleRows().some(({ row }) => row.id === "suicide"));
  assert.ok(offered, "RESPAWN is offered");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  assert.equal(await page.evaluate(() => window.__bf.menu.stack.top.popup === true), true, "it asks first");
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("Enter");
  assert.equal(await page.evaluate(() => window.__bf.menu.isOpen), false);
  // Play on: the mouse captured again (a click, if the browser wanted one).
  if (await page.evaluate(() => window.__bf.menu.waiting)) await page.mouse.click(512, 380);
  // The screen is back about 4 s later, on one of the shots.
  await run(page, 60);
  const dying = await state(page);
  assert.equal(dying.open, false, "nothing shows during the 4 s");
  await run(page, 200);
  const again = await state(page);
  assert.equal(again.open, true, "the screen reopens after the death");
  assert.equal(again.className, chosen.className, "with the class last played");
  await shot(page, `${OUT}/${MAPS[0]}-after-death.png`);
  // Esc puts the pause menu over it; Esc again comes back to it, mouse still
  // free.
  await page.keyboard.press("Escape");
  const menu = await page.evaluate(() => ({ menu: window.__bf.menu.isOpen, over: window.__bf.hud.covered && window.__bf.hud.layer === window.__bf.spawnScreen.view.layer }));
  assert.deepEqual(menu, { menu: true, over: true }, "the pause menu opens over the screen");
  await page.keyboard.press("Escape");
  await run(page, 2);
  const back = await state(page);
  assert.equal(await page.evaluate(() => window.__bf.menu.isOpen), false);
  assert.equal(back.open, true);
  assert.equal(back.controls, false);
  assert.deepEqual(errors, []);
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
