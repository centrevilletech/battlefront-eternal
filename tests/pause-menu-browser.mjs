// The pause menu in the real game, headless (SwiftShader), with real key
// and mouse events, the page as a player has it: its own display loop draws,
// the menus and screens run on their own clocks, and the battle runs live in
// the world worker until something pauses it (the checks wait on what they
// look for, not on frame counts):
// - in play on geo1: Esc (the mouse captured first) opens GAME PAUSED, the
//   world stands still (the worker, and the page's copy of it), the HUD
//   hides, the game's sound is paused; RESUME captures the mouse again and
//   the world runs; a refused capture leaves CLICK TO RESUME until a click;
// - OPTIONS and each of its pages (GAME, VIDEO, AUDIO, CONTROL), a change
//   made on one, the CONTROL page's tabs;
// - RESPAWN, RESTART MISSION, QUIT and EXIT TO WINDOWS through their YES /
//   NO questions, NO focused (the page's leaving is caught, not followed);
// - over the spawn screen (Esc from it, and back to it) and over the
//   scoreboard (Esc on Tab's table and on the end table);
// - screenshots at 1024x768 and 1280x720 into MENUS_SHOTS (default
//   /tmp/bf-punch/menus), each beside the live capture of the 2004 game
//   at 1024x768 (MENUS_LIVE, default /tmp/bf/research/shell/live; its
//   in-game frames are 12 px low) as sidebyside-<name>-<size>.png.
//
// - the menus with no battle at all (the front end's case): a page of their
//   own with only the interface kit, OPTIONS and its pages walked by keys.
//
//   PLAYWRIGHT_MODULE=.../playwright/index.mjs node tests/pause-menu-browser.mjs
//   MENUS_SIZES=1024x768 MENUS_PARTS=play,pages,confirms,spawn,board,alone to narrow it.
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import { launchBrowser } from "./battle-page.mjs";

const BASE = process.env.MENUS_TEST_URL || "http://127.0.0.1:18449";
const OUT = process.env.MENUS_SHOTS || "/tmp/bf-punch/menus";
const LIVE = process.env.MENUS_LIVE || "/tmp/bf/research/shell/live";
const SIZES = (process.env.MENUS_SIZES || "1024x768,1280x720").split(",").map((s) => s.split("x").map(Number));
const PARTS = new Set((process.env.MENUS_PARTS || "play,pages,confirms,spawn,board,alone").split(","));

await fs.mkdir(OUT, { recursive: true });
const browser = await launchBrowser();
const failures = [];

async function check(name, fn) {
  try {
    await fn();
    console.log(`ok - ${name}`);
  } catch (error) {
    failures.push(`${name}: ${error.message}`);
    console.log(`not ok - ${name}: ${error.message}`);
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The battle's page with its display loop running; the battle starts at
// once, not after the loading screen's sequence. { page, world, errors },
// world: the world worker (a Playwright Worker), whose self.__bf is the
// battle itself.
async function open(query, width, height) {
  const page = await browser.newPage({ viewport: { width, height } });
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  await page.route("**/@vite/client", (r) => r.fulfill({ contentType: "application/javascript", body: "" }));
  await page.addInitScript(() => {
    localStorage.setItem("battlefront-eternal.settings", JSON.stringify({ shadows: "off", antialiasing: "off", upscaler: "off" }));
  });
  await page.goto(`${BASE}/?${query}`);
  await page.waitForFunction(() => window.__bf?.client?.ready || /Failed/.test(document.querySelector("#hud")?.textContent || ""), null, { timeout: 180000 });
  const failed = await page.evaluate(() => (window.__bf?.client?.ready ? null : document.querySelector("#hud").textContent));
  if (failed) throw new Error(failed);
  await page.evaluate(async () => {
    const b = window.__bf;
    await b.ready;
    b.begin();
    // Leaving the page is noted, not followed.
    window.__left = [];
    b.menu.navigate = (url) => window.__left.push(url);
    b.menu.closeWindow = () => window.__left.push("window.close");
  });
  const world = page.workers().find((w) => w.url().includes("local-server-worker"));
  if (!world) throw new Error("no world worker on the page");
  return { page, world, errors };
}

// Until fn(arg) holds on the page, or the time is up (the assertions after
// it say what didn't happen).
const until = (page, fn, arg = null, timeout = 20000) => page.waitForFunction(fn, arg, { timeout, polling: 50 }).then(() => true, () => false);

// The world's own tick, in the worker.
const worldTick = (world) => world.evaluate(() => self.__bf.worldClock.tick);

// The world's tick and the page's over a second: the same both times while
// it stands still. (The worker has the page's suspend within a frame or two
// of a pause, so the first reading waits that long.)
async function ticksOver(page, world, ms = 1000) {
  await sleep(500);
  const before = [(await state(page)).tick, await worldTick(world)];
  await sleep(ms);
  return { before, after: [(await state(page)).tick, await worldTick(world)] };
}

// The world running on: the page shows half a second (30 ticks) more of it
// than now, so the worker has run them.
async function runsOn(page, world, message) {
  const [tick, worker] = [(await state(page)).tick, await worldTick(world)];
  await until(page, (t) => window.__bf.worldClock.tick >= t, tick + 30);
  assert.ok((await state(page)).tick >= tick + 30, `${message} (the page's copy)`);
  assert.ok((await worldTick(world)) >= worker + 30, `${message} (the worker)`);
}

// Textures and atlases: a few of the display loop's frames, so loads land.
const settle = () => sleep(1800);

const state = (page) => page.evaluate(() => {
  const b = window.__bf;
  const top = b.menu.stack.top;
  const focused = top?.focused;
  return {
    open: b.menu.isOpen, waiting: b.menu.waiting, screen: top?.id ?? null, popup: Boolean(top?.popup),
    focused: focused ? (focused.widget.rows?.[focused.item]?.id ?? focused.widget.buttons?.[focused.item]?.id ?? focused.widget.id) : null,
    covered: b.hud.covered, overlay: b.hud.overlay === b.menu.overlay, controls: b.controls.enabled, locked: b.controls.locked,
    tick: b.worldClock.tick, paused: b.audio.paused ?? null, left: window.__left.slice(),
    spawn: b.spawnScreen?.isOpen ?? null, board: b.scoreboard.state, layer: b.hud.layer === b.spawnScreen?.view.layer ? "spawn" : b.hud.layer === b.scoreboard.view.layer ? "board" : null,
  };
});

// A frame drawn and read back (the canvas isn't kept between tasks).
async function shot(page, name, width, height, reference = null) {
  const image = await page.evaluate(() => {
    const b = window.__bf;
    b.frame(0, true);
    return b.renderer.domElement.toDataURL("image/png");
  });
  await fs.writeFile(`${OUT}/${name}-${width}x${height}.png`, Buffer.from(image.split(",")[1], "base64"));
  if (!reference) return;
  // The live capture (left, at 1024x768, moved up its 12 px) beside the
  // port (right, at this size).
  const [file, shift] = reference;
  const live = await fs.readFile(`${LIVE}/${file}`).catch(() => null);
  if (!live) return;
  const both = await page.evaluate(async ({ live, ours, shift }) => {
    const load = (src) => new Promise((resolve, reject) => {
      const img = new Image();
      img.onload = () => resolve(img);
      img.onerror = reject;
      img.src = src;
    });
    const [a, b] = await Promise.all([load(`data:image/png;base64,${live}`), load(ours)]);
    const canvas = document.createElement("canvas");
    canvas.width = a.width + 8 + b.width;
    canvas.height = Math.max(a.height, b.height);
    const g = canvas.getContext("2d");
    g.fillStyle = "#f0f";
    g.fillRect(0, 0, canvas.width, canvas.height);
    g.drawImage(a, 0, -shift);
    g.drawImage(b, a.width + 8, 0);
    return canvas.toDataURL("image/png");
  }, { live: live.toString("base64"), ours: image, shift });
  await fs.writeFile(`${OUT}/sidebyside-${name}-${width}x${height}.png`, Buffer.from(both.split(",")[1], "base64"));
}

// The menu's own focus, by row id (the keys get there as a player's would).
async function focusRow(page, id) {
  const s = await state(page);
  if (s.focused === id) return;
  for (let i = 0; i < 8 && (await state(page)).focused !== id; i++) await page.keyboard.press("ArrowDown");
  assert.equal((await state(page)).focused, id, `focus on ${id}`);
}

async function capture(page) {
  await page.mouse.click(500, 380);
  await page.waitForFunction(() => window.__bf.controls.locked, null, { timeout: 5000 }).catch(() => {});
}

try {
  for (const [width, height] of SIZES) {
    if (["play", "pages", "confirms", "board"].some((part) => PARTS.has(part))) {
      const { page, world, errors } = await open("map=geo1&side=rep&seed=1", width, height);
      // Half a second of the battle.
      await until(page, () => window.__bf.worldClock.tick >= 30, null, 60000);

      if (PARTS.has("play")) {
        await check(`${width}x${height}: Esc pauses: GAME PAUSED, the world still, the HUD hidden, the sound paused`, async () => {
          await capture(page);
          await page.keyboard.press("Escape");
          await until(page, () => window.__bf.menu.isOpen && !window.__bf.controls.locked);
          const s = await state(page);
          assert.equal(s.open, true, "open");
          assert.equal(s.screen, "pause");
          assert.equal(s.focused, "resume", "RESUME GAME focused");
          assert.equal(s.covered && s.overlay, true, "the HUD's overlay, covering it");
          assert.equal(s.controls, false, "the soldier takes no input");
          assert.equal(s.locked, false, "the mouse let go");
          assert.equal(s.paused, true, "the game's sound paused");
          const { before, after } = await ticksOver(page, world);
          assert.deepEqual(after, before, "the world stands still (the page's tick, the worker's)");
        });
        await settle();
        await shot(page, "pause", width, height, ["05a_pause_menu_ingame_hover_RESPAWN.png", 12]);
        await page.keyboard.press("ArrowDown");
        await page.keyboard.press("ArrowDown");
        await page.keyboard.press("ArrowDown");
        await sleep(300);
        await shot(page, "pause-restart-focused", width, height, ["05b_pause_menu_ingame_after_Down_RESTART_MISSION.png", 12]);
        await check(`${width}x${height}: RESUME captures the mouse again and the world runs`, async () => {
          await focusRow(page, "resume");
          await page.keyboard.press("Enter");
          await page.waitForFunction(() => window.__bf.controls.locked, null, { timeout: 5000 });
          await until(page, () => !window.__bf.menu.isOpen && !window.__bf.hud.covered);
          const s = await state(page);
          assert.equal(s.open, false);
          assert.equal(s.covered, false, "the HUD back");
          assert.equal(s.controls, true);
          assert.equal(s.paused, false, "the sound back");
          await runsOn(page, world, "the world runs");
        });
        await check(`${width}x${height}: a refused capture leaves CLICK TO RESUME, and a click plays on`, async () => {
          await page.keyboard.press("Escape");
          await until(page, () => window.__bf.menu.isOpen);
          assert.equal((await state(page)).open, true);
          await page.evaluate(() => {
            const c = window.__bf.controls;
            c.realLock = c.lock;
            c.lock = async () => false;
          });
          await page.keyboard.press("Enter");
          await until(page, () => window.__bf.menu.waiting && !window.__bf.hud.covered);
          let s = await state(page);
          assert.equal(s.open, false);
          assert.equal(s.waiting, true, "waiting for the mouse");
          assert.equal(s.covered, false, "the HUD shows");
          const { before, after } = await ticksOver(page, world);
          assert.deepEqual(after, before, "the world waits too");
          await shot(page, "click-to-resume", width, height);
          await page.evaluate(() => {
            const c = window.__bf.controls;
            c.lock = c.realLock;
          });
          await capture(page);
          await runsOn(page, world, "the click played on");
          s = await state(page);
          assert.equal(s.waiting, false);
          assert.equal(s.locked, true);
        });
      }

      if (PARTS.has("pages")) {
        await check(`${width}x${height}: OPTIONS and each page, a change made, the CONTROL page's tabs`, async () => {
          if (!(await state(page)).open) await page.keyboard.press("Escape");
          await until(page, () => window.__bf.menu.isOpen);
          await focusRow(page, "opts");
          await page.keyboard.press("Enter");
          await until(page, () => window.__bf.menu.stack.top?.id === "options");
          assert.equal((await state(page)).screen, "options");
          await settle();
          await shot(page, "options", width, height, ["05c_pause_OPTIONS_submenu.png", 12]);
          const pages = [["general", "options-game", "05d_pause_OPTIONS_GAME_OPTIONS.png"], ["video", "options-video", "05e_pause_OPTIONS_VIDEO_OPTIONS.png"],
            ["sound", "options-audio", "05f_pause_OPTIONS_AUDIO_OPTIONS.png"], ["controls", "options-controls", "05g_pause_OPTIONS_CONTROL_OPTIONS_infantry.png"]];
          for (const [row, id, reference] of pages) {
            await focusRow(page, row);
            await page.keyboard.press("Enter");
            await until(page, (id) => window.__bf.menu.stack.top?.id === id, id);
            assert.equal((await state(page)).screen, id, `${row} opens ${id}`);
            await settle();
            await shot(page, id, width, height, [reference, 12]);
            if (id === "options-video") {
              // QUALITY PRESET is the first row: Right to the next preset,
              // then every setting back as it was.
              await page.evaluate(() => { window.__before = { ...window.__bf.settings.values }; });
              const before = await page.evaluate(() => window.__bf.settings.get("performancePreset"));
              await page.keyboard.press("ArrowRight");
              const after = await page.evaluate(() => window.__bf.settings.get("performancePreset"));
              assert.notEqual(after, before, "the preset changed");
              assert.equal(await page.evaluate(() => JSON.parse(localStorage.getItem("battlefront-eternal.settings")).performancePreset), after, "and saved");
              await page.evaluate(() => {
                const settings = window.__bf.settings;
                for (const [key, value] of Object.entries(window.__before)) if (key !== "performancePreset") settings.set(key, value);
                settings.set("performancePreset", window.__before.performancePreset);
              });
              await settle();
            }
            if (id === "options-controls") {
              await page.keyboard.press("ArrowDown");
              await page.keyboard.press("ArrowDown");
              await page.keyboard.press("ArrowDown");
              await page.keyboard.press("ArrowRight");
              await sleep(300);
              const tab = await page.evaluate(() => window.__bf.menu.stack.top.widget("tabs").active);
              assert.equal(tab, 1, "VEHICLE");
              await shot(page, "options-controls-vehicle", width, height);
            }
            await page.keyboard.press("Escape");
            await until(page, () => window.__bf.menu.stack.top?.id === "options");
            assert.equal((await state(page)).screen, "options", "BACK to OPTIONS");
          }
          await page.keyboard.press("Escape");
          await until(page, () => window.__bf.menu.stack.top?.id === "pause");
          assert.equal((await state(page)).screen, "pause");
        });
      }

      if (PARTS.has("confirms")) {
        await check(`${width}x${height}: RESPAWN, RESTART MISSION, QUIT and EXIT TO WINDOWS ask first, NO focused`, async () => {
          if (!(await state(page)).open) await page.keyboard.press("Escape");
          await until(page, () => window.__bf.menu.isOpen);
          const asks = [["suicide", "05h_pause_RESPAWN_confirm_popup.png"], ["restart", "05i_pause_RESTART_MISSION_confirm_popup.png"],
            ["quit", "05j_pause_QUIT_confirm_popup.png"], ["exit", "05k_pause_EXIT_TO_WINDOWS_confirm_popup.png"]];
          for (const [row, reference] of asks) {
            await focusRow(page, row);
            await page.keyboard.press("Enter");
            await until(page, () => window.__bf.menu.stack.top?.popup === true);
            const s = await state(page);
            assert.equal(s.popup, true, `${row} asks`);
            assert.equal(s.focused, "no", "NO focused");
            await settle();
            await shot(page, `confirm-${row}`, width, height, [reference, 12]);
            await page.keyboard.press("Enter");
            await until(page, () => window.__bf.menu.stack.top?.id === "pause");
            const back = await state(page);
            assert.equal(back.screen, "pause", "NO: back to the menu");
            assert.deepEqual(back.left, [], "nothing done");
          }
          // RESTART MISSION, YES: the battle's URL again.
          await focusRow(page, "restart");
          await page.keyboard.press("Enter");
          await page.keyboard.press("ArrowLeft");
          await page.keyboard.press("Enter");
          let s = await state(page);
          assert.equal(s.left.length, 1);
          const restart = new URL(s.left[0]);
          assert.equal(restart.searchParams.get("map"), "geo1");
          assert.equal(restart.searchParams.get("side"), "rep");
          // QUIT, YES: the front end's main menu, the battle gone from the URL.
          await page.evaluate(() => { window.__left.length = 0; });
          await focusRow(page, "quit");
          await page.keyboard.press("Enter");
          await page.keyboard.press("ArrowLeft");
          await page.keyboard.press("Enter");
          s = await state(page);
          assert.equal(s.left.length, 1);
          const quit = new URL(s.left[0], "http://x/");
          assert.equal(quit.searchParams.get("map"), null);
          assert.equal(quit.searchParams.get("screen"), "main");
          // EXIT TO WINDOWS, YES: the tab closed if allowed, else the menu.
          await page.evaluate(() => { window.__left.length = 0; });
          await focusRow(page, "exit");
          await page.keyboard.press("Enter");
          await page.keyboard.press("ArrowLeft");
          await page.keyboard.press("Enter");
          await sleep(500);
          s = await state(page);
          assert.equal(s.left[0], "window.close");
          assert.ok(new URL(s.left[1], "http://x/").searchParams.get("screen") === "main", "then the main menu");
          // RESPAWN, YES: a death, and play on (the world has the choice
          // with the first command once it runs again).
          await page.evaluate(() => { window.__left.length = 0; });
          await focusRow(page, "suicide");
          await page.keyboard.press("Enter");
          await page.keyboard.press("ArrowLeft");
          await page.keyboard.press("Enter");
          await page.waitForFunction(() => !window.__bf.menu.isOpen, null, { timeout: 5000 });
          await capture(page);
          await until(page, () => window.__bf.combat.dead);
          assert.equal(await page.evaluate(() => window.__bf.combat.dead), true, "the soldier died");
        });
      }

      if (PARTS.has("board")) {
        await check(`${width}x${height}: over the scoreboard: Esc on Tab's table pauses over it, and back`, async () => {
          if ((await state(page)).open) await page.keyboard.press("Escape");
          // Play on (a menu Esc closed waits for a click), five seconds of it.
          await capture(page);
          await until(page, (t) => window.__bf.worldClock.tick >= t, (await state(page)).tick + 300, 60000);
          await page.keyboard.down("Tab");
          await page.keyboard.up("Tab");
          await sleep(700);
          assert.equal((await state(page)).board, "peek", "the table pinned");
          await page.keyboard.press("Escape");
          await until(page, () => window.__bf.menu.isOpen && window.__bf.hud.covered);
          await settle();
          let s = await state(page);
          assert.equal(s.open, true, "the pause menu");
          assert.equal(s.board, "peek", "over the table");
          assert.equal(s.covered, true);
          await shot(page, "pause-over-board", width, height);
          await page.keyboard.press("Escape");
          await until(page, () => !window.__bf.menu.isOpen);
          s = await state(page);
          assert.equal(s.open, false);
          assert.equal(s.board, "peek", "the table again");
          assert.equal(s.layer, "board");
        });
        await check(`${width}x${height}: the end table: Esc pauses over it; DONE is still the way on`, async () => {
          await world.evaluate(() => {
            const b = self.__bf;
            b.battle.reinforcements[b.battle.foe] = 0;
          });
          // The game over 2 s on, the end table 7 s after that.
          await until(page, () => window.__bf.scoreboard.state === "end", null, 90000);
          assert.equal((await state(page)).board, "end");
          await page.keyboard.press("Escape");
          await until(page, () => window.__bf.menu.isOpen);
          const s = await state(page);
          assert.equal(s.open, true);
          assert.equal(s.board, "end");
          await page.keyboard.press("Escape");
          await until(page, () => !window.__bf.menu.isOpen);
          assert.equal((await state(page)).open, false);
        });
      }
      await check(`${width}x${height}: no page errors`, async () => assert.deepEqual(errors.filter((e) => !/KHR_parallel_shader_compile|isn't extracted|no sound for/.test(e)), []));
      await page.close();
    }

    if (PARTS.has("spawn")) {
      const { page, errors } = await open("map=geo1&seed=1", width, height);
      // A second of the battle under the screen.
      await until(page, () => window.__bf.worldClock.tick >= 60, null, 60000);
      await check(`${width}x${height}: Esc over the spawn screen: the pause menu over it, its room and body showing, and back to it`, async () => {
        let s = await state(page);
        assert.equal(s.spawn, true, "the spawn screen is up");
        await page.mouse.move(500, 300);
        await page.mouse.move(520, 320);
        await page.keyboard.press("Escape");
        await until(page, () => window.__bf.menu.isOpen && window.__bf.hud.covered);
        s = await state(page);
        assert.equal(s.open, true);
        assert.equal(s.spawn, true);
        assert.equal(s.layer, "spawn", "the screen keeps its layer");
        assert.equal(s.covered, true, "covered by the menu");
        await settle();
        await shot(page, "pause-over-spawn", width, height, ["05_pause_menu_from_spawn_screen_GAME_PAUSED.png", 12]);
        await page.keyboard.press("Escape");
        await until(page, () => !window.__bf.menu.isOpen);
        s = await state(page);
        assert.equal(s.open, false);
        assert.equal(s.spawn, true, "the screen again");
        assert.equal(s.controls, false, "with the mouse its own");
        assert.equal(s.covered, false);
      });
      await check(`${width}x${height}: no page errors`, async () => assert.deepEqual(errors.filter((e) => !/KHR_parallel_shader_compile|isn't extracted|no sound for/.test(e)), []));
      await page.close();
    }

    if (PARTS.has("alone")) {
      // A page with nothing but a canvas: the interface kit, the stack and
      // the options, no map, no world, no HUD.
      const page = await browser.newPage({ viewport: { width, height } });
      const errors = [];
      page.on("pageerror", (e) => errors.push(e.message));
      page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
      await page.route("**/@vite/client", (r) => r.fulfill({ contentType: "application/javascript", body: "" }));
      await page.goto(`${BASE}/tests/menus-alone.html`);
      await check(`${width}x${height}: the menus with no battle: OPTIONS and its pages on a page of their own`, async () => {
        await page.waitForFunction(() => window.__alone, null, { timeout: 60000 });
        const draw = (n) => page.evaluate((n) => { for (let i = 0; i < n; i++) window.__alone.draw(); }, n);
        await draw(60);
        for (let i = 0; i < 4; i++) {
          await draw(1);
          await new Promise((r) => setTimeout(r, 300));
        }
        const rows = await page.evaluate(() => window.__alone.stack.top.widget("options").visibleRows().map(({ row }) => row.id));
        assert.deepEqual(rows, ["general", "video", "sound", "controls", "online", "credits"], "the front end's OPTIONS rows");
        const image = await page.evaluate(() => { window.__alone.draw(); return document.querySelector("canvas").toDataURL("image/png"); });
        await fs.writeFile(`${OUT}/alone-options-${width}x${height}.png`, Buffer.from(image.split(",")[1], "base64"));
        // OPTIONS comes back with its first row focused, as the original's
        // does: Down to each row in turn.
        const ids = [];
        for (let i = 0; i < 4; i++) {
          for (let k = 0; k < i; k++) await page.keyboard.press("ArrowDown");
          await page.keyboard.press("Enter");
          await draw(30);
          ids.push(await page.evaluate(() => window.__alone.stack.top.id));
          await page.keyboard.press("Escape");
          await draw(5);
        }
        assert.deepEqual(ids, ["options-game", "options-video", "options-audio", "options-controls"]);
        assert.deepEqual(errors, []);
      });
      await page.close();
    }
  }
} finally {
  await browser.close();
}

if (failures.length) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("pause menu: all checks passed");
