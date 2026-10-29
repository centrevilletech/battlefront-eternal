// The front end and the loading screen in a real browser, headless
// (SwiftShader):
// - no parameters: the front end, its start movie playing under the click
//   gate; a key press starts the sound (the soundtrack's first track) and
//   moves on to the main menu with its intro and a whoosh; tracks follow
//   each other with the gap between; a forward move into OPTIONS dips the
//   music and brings it back, Back cuts to the loop;
// - ?screen=main: straight to the main menu;
// - ?map=geo1: the battle boots as before under the loading screen, which
//   zooms through every picture on the original game's timeline and is gone
//   once its sequence has run and the battle is ready;
// - screenshots into SHELL_SHOTS (default /tmp/bf-punch/shell).
//
// The front end's data is served from synthetic fixtures made here with
// ffmpeg and ImageMagick (test patterns, tones and drawn pictures; nothing
// retail), and a tiny sound sub-level (a menu sound and the loading
// screen's, on synthetic pings), whatever public/common/shell holds, so the
// checks can name what plays.
//
//   PLAYWRIGHT_MODULE=.../playwright/index.mjs node tests/shell-browser.mjs
//   SHELL_CHECKS="no parameters,screen=main" runs the checks whose names
//   have one of these in them; SHELL_TEST_URL points it at another server;
//   CHROMIUM=/usr/bin/chromium drives that browser instead of Playwright's.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const BASE = process.env.SHELL_TEST_URL || "http://127.0.0.1:18449";
const OUT = process.env.SHELL_SHOTS || "/tmp/bf-punch/shell";
const FIXTURES = process.env.SHELL_FIXTURES || path.join(os.tmpdir(), "bf-shell-fixtures");

// Synthetic stand-ins for the extracted front end: movies with their names
// written on them, two short tones, two noise whooshes, drawn pictures.
async function makeFixtures(dir) {
  const shell = path.join(dir, "common/shell");
  if (await fs.stat(path.join(shell, "shell.json")).then(() => true, () => false)) return;
  await fs.mkdir(path.join(shell, "movies"), { recursive: true });
  await fs.mkdir(path.join(shell, "load"), { recursive: true });
  await fs.mkdir(path.join(dir, "audio/shell"), { recursive: true });
  const ffmpeg = (...args) => execFileSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", ...args]);
  const movie = (name, label, color, seconds, size = "800x600") => ffmpeg("-f", "lavfi", "-i",
    `color=c=${color}:s=${size}:d=${seconds}:r=30,drawtext=text='${label}':fontsize=64:fontcolor=white:x=(w-text_w)/2:y=(h-text_h)/2,` +
    `drawbox=x='mod(t*240\\,${size.split("x")[0]})':y=ih-120:w=60:h=60:color=cyan:t=fill`,
    "-c:v", "libvpx-vp9", "-b:v", "250k", "-pix_fmt", "yuv420p", path.join(shell, "movies", `${name}.webm`));
  movie("start_screen", "START SCREEN", "0x0a2a6a", 4);
  movie("main_trans", "MAIN INTRO", "0x6a3a0a", 1);
  movie("main", "MAIN MENU LOOP", "0x0a4a8a", 4);
  movie("coreship_in", "OPTIONS INTRO", "0x5a2a5a", 1.5);
  movie("coreship_screen", "OPTIONS LOOP", "0x2a2a7a", 4);
  movie("geo1fly", "GEO1 FLYOVER", "0x7a4a1a", 3, "720x480");
  const tone = (name, freq, seconds) => ffmpeg("-f", "lavfi", "-i", `sine=frequency=${freq}:duration=${seconds}`, "-ac", "2", "-c:a", "libopus",
    path.join(dir, "audio/shell", `${name}.ogg`));
  tone("track1", 330, 2);
  tone("track2", 440, 2);
  for (const [i, f] of [[1, 900], [2, 1400]]) {
    ffmpeg("-f", "lavfi", "-i", `anoisesrc=d=0.8:c=pink:a=0.3,lowpass=f=${f}`, "-ac", "2", "-c:a", "libopus", path.join(dir, "audio/shell", `whoosh${i}.ogg`));
  }
  const convert = (...args) => execFileSync("convert", args);
  const picture = (name, size, colors, label) => convert("-size", size, `plasma:${colors}`, "-gravity", "center", "-pointsize", "72",
    "-fill", "white", "-annotate", "0", label, path.join(shell, "load", `${name}.png`));
  picture("galaxy", "1024x1024", "black-purple", "GALAXY");
  picture("locale", "1024x1024", "black-orange", "PLANET");
  picture("landsat", "1024x1024", "sienna-tan", "SURFACE");
  picture("level", "1024x512", "navy-steelblue", "LEVEL");
  convert("-size", "256x256", "xc:none", "-fill", "rgba(40,160,220,0.6)", "-stroke", "none",
    "-draw", "path 'M 128,128 L 128,8 A 120,120 0 0,1 232,68 Z'", "-draw", "path 'M 128,128 L 20,180 A 120,120 0 0,1 60,30 Z'",
    path.join(shell, "load", "disk.png"));
  for (const [i, grey] of [[0, "#151515"], [1, "#707070"], [2, "#d0d0d0"]]) convert("-size", "16x16", `xc:${grey}`, path.join(shell, "load", `led${i}.png`));
  const geo1 = { id: "geo1", name: "GEONOSIS: SPIRE", extracted: true, eras: { cw: { teams: ["rep", "cis"], attacker: 1 } }, fly: "movies/geo1fly.webm",
    load: { planet: "GEONOSIS", stages: [
      { image: "load/galaxy.png", target: [400, 690, 40, 44] },
      { image: "load/locale.png", target: [300, 520, 48, 48] },
      { image: "load/landsat.png", target: [600, 300, 60, 60] },
      { image: "load/level.png", target: [0, 0, 0, 0] },
    ] } };
  const data = {
    movies: {
      start: { intro: null, loop: "movies/start_screen.webm" },
      main: { intro: "movies/main_trans.webm", loop: "movies/main.webm", fadeIn: 0, fadeOut: 0.5 },
      options: { intro: "movies/coreship_in.webm", loop: "movies/coreship_screen.webm" },
    },
    maps: [geo1, { id: "nab2", name: "NABOO: THEED", extracted: true, eras: { cw: { teams: ["rep", "cis"], attacker: 1 } } }],
    music: [{ name: "track1", file: "/audio/shell/track1.ogg" }, { name: "track2", file: "/audio/shell/track2.ogg" }],
    whooshes: ["/audio/shell/whoosh1.ogg", "/audio/shell/whoosh2.ogg"],
    load: { disk: "load/disk.png", leds: ["load/led0.png", "load/led1.png", "load/led2.png"], timing: [3.5, 1] },
  };
  await fs.writeFile(path.join(shell, "shell.json"), JSON.stringify(data, null, 1));
}

// A sound sub-level in the form tools/zero_shell.py writes the shell's:
// the shellfx bus, a menu sound and the loading screen's three sounds, their
// samples synthetic pings in bank.bin. Names are hashed as the data's are.
async function makeSoundFixture(dir) {
  const out = path.join(dir, "audio/shell");
  if (await fs.stat(path.join(out, "sound.json")).then(() => true, () => false)) return;
  await fs.mkdir(out, { recursive: true });
  const { hashKey } = await import("../src/audio/hash.js");
  const ping = (rate, seconds, hz) => {
    const n = Math.round(rate * seconds);
    const pcm = new Int16Array(n);
    for (let i = 0; i < n; i++) pcm[i] = Math.round(Math.sin((2 * Math.PI * hz * i) / rate) * Math.exp((-6 * i) / n) * 20000);
    return pcm;
  };
  const samples = {};
  const chunks = [];
  let offset = 0;
  const add = (id, pcm, rate) => {
    samples[id] = [offset, pcm.length, rate];
    chunks.push(Buffer.from(pcm.buffer));
    offset += pcm.length * 2;
    if (offset % 4) {
      chunks.push(Buffer.alloc(4 - (offset % 4)));
      offset += 4 - (offset % 4);
    }
  };
  add("0x00000001", ping(7056, 0.35, 440), 7056);
  add("0x00000002", ping(22050, 0.43, 300), 22050);
  add("0x00000003", ping(22050, 0.12, 900), 22050);
  const sound = (name, props, sample) => ({ type: "SoundProperties", Name: hashKey(name), Bus: hashKey("shellfx"), PlayInterval: 0.05, ...props, SampleList: [[sample, 1]] });
  const objects = [
    { type: "Bus", Name: hashKey("shellfx"), Gain: 0.3 },
    sound("load_ytracking", { Pitch: 1.8, Gain: 0.7, Looping: 1 }, "0x00000001"),
    sound("load_xtracking", { Pitch: 2.1, Gain: 0.7, Looping: 1 }, "0x00000001"),
    sound("load_zoom", { Pitch: 1.5, Gain: 1, Looping: 0 }, "0x00000002"),
    sound("shell_select_change", { Pitch: 1.5, Gain: 1, Looping: 0 }, "0x00000003"),
  ];
  await fs.writeFile(path.join(out, "sound.json"), JSON.stringify({ sublevel: "shell", map: null, sides: [], objects, samples, streams: [],
    mission: {}, classes: {}, world: { regions: [], emitters: [], posts: {}, materials: [] }, names: {} }));
  await fs.writeFile(path.join(out, "bank.bin"), Buffer.concat(chunks));
}

await makeFixtures(FIXTURES);
await makeSoundFixture(FIXTURES);
await fs.mkdir(OUT, { recursive: true });
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined,
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--autoplay-policy=no-user-gesture-required"] });
const failures = [];
const types = { ".json": "application/json", ".webm": "video/webm", ".ogg": "audio/ogg", ".png": "image/png", ".bin": "application/octet-stream" };

// Pages open in the check running, closed when it ends however it ends (a
// battle left running starves the next one of the CPU).
const pages = new Set();

async function open(query, { width = 1024, height = 768, wait = () => window.__shell } = {}) {
  const page = await browser.newPage({ viewport: { width, height } });
  pages.add(page);
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  await page.route("**/@vite/client", (r) => r.fulfill({ contentType: "application/javascript", body: "" }));
  {
    // On the context, so the loading screen's worker's requests are served too.
    await page.context().route(/\/(common\/shell|audio\/shell)\//, async (route) => {
      const url = new URL(route.request().url());
      const file = path.join(FIXTURES, decodeURIComponent(url.pathname));
      const body = await fs.readFile(file).catch(() => null);
      if (!body) return route.fulfill({ status: 404, body: "" });
      return route.fulfill({ status: 200, contentType: types[path.extname(file)] || "application/octet-stream", body });
    });
  }
  await page.goto(`${BASE}/${query ? `?${query}` : ""}`);
  if (wait) await page.waitForFunction(wait, null, { timeout: 180000 });
  return { page, errors };
}

const ONLY = process.env.SHELL_CHECKS ? process.env.SHELL_CHECKS.split(",") : null;

async function check(name, fn) {
  if (ONLY && !ONLY.some((part) => name.includes(part))) return;
  try {
    await fn();
    console.log(`ok - ${name}`);
  } catch (error) {
    failures.push(name);
    console.log(`not ok - ${name}\n  ${error.stack.split("\n").slice(0, 4).join("\n  ")}`);
  } finally {
    for (const page of pages) await page.close().catch(() => {});
    pages.clear();
  }
}

// The shell's state as plain values.
const state = (page) => page.evaluate(() => {
  const s = window.__shell;
  const audio = s.media.audio;
  return {
    screen: s.screen,
    moves: s.media.moves,
    gate: Boolean(document.querySelector("#gate")),
    hud: document.querySelector("#hud")?.textContent ?? null,
    movies: s.media.movies.state(),
    running: audio.running,
    music: { current: audio.music.current, phase: audio.music.phase, cycle: audio.music.cycle,
      playing: Boolean(audio.music.handle && !audio.music.handle.element.paused), track: audio.music.handle?.track.name ?? null },
    dip: audio.nodes ? audio.nodes.dip.gain.value : null,
    whoosh: audio.whooshIndex,
  };
});

const until = (page, fn, arg, timeout = 15000) => page.waitForFunction(fn, arg, { timeout, polling: 50 });

try {
  await check("no parameters: the front end, its start movie under the click gate; nothing of the battle", async () => {
    const { page, errors } = await open("");
    await until(page, () => window.__shell.media.movies.state().some((m) => m.playing && m.opacity > 0.99));
    const s = await state(page);
    assert.equal(s.screen, "start");
    assert.equal(s.gate, true);
    assert.equal(s.hud, "", "the help line is gone");
    assert.match(s.movies.at(-1).url, /start_screen\.webm$/);
    assert.equal(s.movies.at(-1).loop, true);
    assert.equal(s.running, false, "no sound before the press");
    assert.equal(await page.evaluate(() => Boolean(window.__bf)), false, "no battle");
    const scripts = await page.evaluate(() => performance.getEntriesByType("resource").map((e) => e.name).filter((n) => /\/src\/game\.js/.test(n)));
    assert.deepEqual(scripts, [], "game.js isn't loaded");
    await page.waitForTimeout(400);
    await page.screenshot({ path: `${OUT}/gate-1024x768.png` });

    // The press: the sound starts, the main menu's intro plays with a whoosh.
    await page.keyboard.press("Enter");
    await until(page, () => window.__shell.screen === "main" && !document.querySelector("#gate"));
    // The whoosh goes with the intro's first frame.
    await until(page, () => window.__shell.media.audio.whooshIndex === 1, null, 5000);
    const pressed = await state(page);
    assert.equal(pressed.running, true, "the press unlocked the sound");
    assert.equal(pressed.music.current, "shell_soundtrack");
    assert.equal(pressed.whoosh, 1, "a whoosh with the intro");
    assert.equal(pressed.moves, 1);
    await until(page, () => window.__shell.media.movies.state().some((m) => /\/main\.webm$/.test(m.url) && m.playing && m.opacity > 0.99));
    // The intro is gone once it ends.
    await until(page, () => !window.__shell.media.movies.state().some((m) => /main_trans/.test(m.url)), null, 2000);
    const settled = await state(page);
    assert.equal(settled.music.track, "track1");
    assert.equal(settled.dip, 1, "no dip: the soundtrack was already playing");
    await page.screenshot({ path: `${OUT}/main-1024x768.png` });

    // The tracks follow each other with the gap between (shortened here).
    await page.evaluate(() => { window.__shell.media.audio.music.interval = 1; });
    await until(page, () => window.__shell.media.audio.music.phase === "gap", null, 10000);
    await until(page, () => window.__shell.media.audio.music.handle?.track.name === "track2", null, 5000);
    await until(page, () => !window.__shell.media.audio.music.handle.element.paused, null, 5000);

    // Forward into OPTIONS (no music of its own): the dip, the intro and a
    // whoosh, then the music back at 1.
    const entering = page.evaluate(() => window.__shell.enter("options").then(() => true));
    await until(page, () => window.__shell.media.audio.nodes.dip.gain.value < 0.6, null, 5000);
    assert.equal((await state(page)).whoosh, 0, "the second whoosh (back to the first after it)");
    assert.equal(await entering, true);
    await until(page, () => window.__shell.media.audio.nodes.dip.gain.value > 0.99, null, 5000);
    await until(page, () => window.__shell.media.movies.state().some((m) => /coreship_screen/.test(m.url) && m.opacity > 0.99));
    await page.screenshot({ path: `${OUT}/options-1024x768.png` });

    // A menu sound, as the menus ask for one: core.lvl's shell_select_change,
    // from a battle's sound sub-level by byte range (when one is extracted).
    if (await page.evaluate(() => fetch("/audio/index.json").then((r) => r.ok && /json/.test(r.headers.get("content-type") || ""), () => false))) {
      await page.evaluate(async () => (await import("/src/audio/events.js")).emitSound("interface", "shell_select_change"));
      await until(page, () => window.__shell.media.audio.sounds.get("shell_select_change")?.buffer?.duration > 0, null, 10000);
      const sound = await page.evaluate(() => {
        const s = window.__shell.media.audio.sounds.get("shell_select_change");
        return { pitch: s.pitch, interval: s.interval, duration: s.buffer.duration };
      });
      assert.equal(sound.pitch, 1.5, "its own pitch");
      assert.equal(sound.interval, 0.05, "its PlayInterval");
      assert.ok(sound.duration > 0.01 && sound.duration < 2, `${sound.duration} s`);
    }

    // Back to the main menu: its loop at once, no intro, no whoosh.
    await page.evaluate(() => window.__shell.enter("main", { back: true }));
    const back = await state(page);
    assert.equal(back.whoosh, 0);
    assert.ok(back.movies.some((m) => /\/main\.webm$/.test(m.url)));
    assert.ok(!back.movies.some((m) => /main_trans/.test(m.url)), "no intro going back");
    assert.equal(back.moves, 2);
    assert.deepEqual(errors, []);
    await page.close();
  });

  await check("?screen=main (the pause menu's QUIT): straight to the main menu, no gate", async () => {
    const { page, errors } = await open("screen=main&noreload");
    await until(page, () => window.__shell.media.movies.state().some((m) => /\/main\.webm$/.test(m.url) && m.opacity > 0.99));
    const s = await state(page);
    assert.equal(s.gate, false);
    assert.equal(s.screen, "main");
    assert.equal(s.moves, 1, "with the main menu's intro");

    // Instant Action: no movie of its own; the highlighted map's flyover in
    // the lower right, half a second after the highlight.
    await page.evaluate(() => window.__shell.enter("ia"));
    await until(page, () => window.__shell.media.movies.state().length === 0, null, 5000);
    await page.evaluate(() => { window.__preview = window.__shell.media.preview("geo1"); });
    await page.waitForTimeout(250);
    assert.equal((await state(page)).movies.length, 0, "not before half a second");
    await until(page, () => window.__shell.media.movies.state().some((m) => /geo1fly/.test(m.url) && m.opacity > 0.99), null, 8000);
    const rect = await page.evaluate(() => {
      const video = document.querySelector(".shell-movies video");
      return [video.style.left, video.style.top, video.style.width, video.style.height, getComputedStyle(video).objectFit].join(" ");
    });
    assert.equal(rect, "345.6px 256px 678.4px 512px fill", "the original's rectangle at 1024 x 768, stretched");
    await page.screenshot({ path: `${OUT}/ia-preview-1024x768.png` });
    await page.evaluate(() => window.__shell.media.preview("nab2"));
    await until(page, () => window.__shell.media.movies.state().length === 0, null, 5000);
    assert.deepEqual(errors.filter((e) => !/AudioContext/.test(e)), []);
    await page.close();
  });

  await check("?shell=1 forces the front end even with a map; a missing shell.json leaves a working gate", async () => {
    const { page, errors } = await open("shell=1&map=nab2");
    assert.equal(await page.evaluate(() => window.__shell.screen), "start");
    await page.close();
    const bare = await browser.newPage({ viewport: { width: 1280, height: 720 } });
    pages.add(bare);
    await bare.route("**/@vite/client", (r) => r.fulfill({ contentType: "application/javascript", body: "" }));
    await bare.route(/\/common\/shell\//, (r) => r.fulfill({ status: 404, body: "" }));
    const bareErrors = [];
    bare.on("pageerror", (e) => bareErrors.push(e.message));
    await bare.goto(`${BASE}/`);
    await bare.waitForFunction(() => window.__shell && document.querySelector("#gate"), null, { timeout: 60000 });
    await bare.waitForTimeout(500);
    await bare.screenshot({ path: `${OUT}/gate-no-data-1280x720.png` });
    await bare.keyboard.press("Space");
    await bare.waitForFunction(() => window.__shell.screen === "main", null, { timeout: 10000 });
    assert.deepEqual(bareErrors, []);
    assert.deepEqual(errors, []);
    await bare.close();
  });

  await check("?map=geo1: the battle boots as before, under the loading screen, which zooms through every picture and goes once its sequence has run and the battle is ready", async () => {
    const { page, errors } = await open("map=geo1&ai=off&shadows=off", { wait: () => window.__loading });
    // Samples of the loading screen while the battle loads: its progress
    // (createGame's steps) and its stage, and a screenshot a second.
    const seen = [];
    for (let i = 0; i < 600; i++) {
      const sample = await page.evaluate(() => ({
        open: !window.__loading.closed,
        t: Math.round(performance.now()),
        value: window.__loading.value,
        stage: window.__loading.sequence?.stage ?? 0,
        phase: window.__loading.sequence?.phase ?? "",
      }));
      if (!sample.open) break;
      seen.push(sample);
      if (i % 4 === 0) await page.screenshot({ path: `${OUT}/loading-geo1-${String(i / 4).padStart(2, "0")}.png`, timeout: 10000 }).catch(() => {});
      await page.waitForTimeout(250);
    }
    await page.waitForFunction(() => window.__bf, null, { timeout: 180000 });
    await page.evaluate(() => window.__bf.ready);
    await page.waitForFunction(() => window.__loading.closed && !document.querySelector("#loading"), null, { timeout: 30000 });
    const steps = await page.evaluate(() => window.__loading.steps);
    console.log(`  loading samples: ${seen.map((s) => `${s.value.toFixed(2)}/${s.stage}${s.phase[0]}`).join(" ")}`);
    console.log(`  the load's steps (ms): ${steps.map(([step, at]) => `${step} ${at}`).join(", ")}`);
    assert.deepEqual(steps.map(([step]) => step), ["data", "world", "shaders", "shaders", "ready"],
      "createGame's steps in order (done() repeats shaders), then ready");
    assert.ok(steps.every(([, at], i) => i === 0 || at >= steps[i - 1][1]));
    assert.ok(seen.length >= 4, `the loading screen showed (${seen.length} samples)`);
    const values = seen.map((s) => s.value);
    assert.ok(values.every((v, i) => i === 0 || v >= values[i - 1]), "its progress only goes up");
    assert.ok(values.some((v) => v >= 0.3 && v < 1), "the load's steps reached it");
    const ready = steps.find(([step]) => step === "ready")[1];
    // The original game's timeline: 3.5 s of tracking and a 1 s zoom on each
    // picture but the level, the screen gone once the last zoom has ended.
    const run = await page.evaluate(() => ({ plan: window.__loading.plan, start: window.__loading.start - performance.timeOrigin, ends: window.__loading.endsAt - performance.timeOrigin }));
    assert.equal(run.plan.stages, 4, "every picture loaded");
    assert.equal(run.plan.duration, 13.5);
    assert.ok(Math.max(...seen.map((s) => s.stage)) >= 2, "the pictures went by");
    assert.ok(seen.at(-1).t >= 13500 && seen.at(-1).t >= run.ends - 300, `the screen ran its course (open at ${seen.at(-1).t} ms, its end at ${Math.round(run.ends)} ms) with the battle ready at ${ready} ms`);
    const sound = await page.evaluate(() => window.__loading.sound?.state() ?? null);
    assert.ok(sound && sound.loops === 6 && sound.once === 3 && sound.missed === 0, `the pulses and the zooms went on the audio clock: ${JSON.stringify({ ...sound, log: undefined })}`);
    assert.equal(await page.evaluate(() => typeof window.__bf.createWorkerHost), "function", "the dev hook is still set");
    assert.equal(await page.evaluate(() => Boolean(window.__shell)), false, "no front end in a battle");
    assert.deepEqual(errors, []);
    await page.close();
  });

  await check("?map=nab2 without loading pictures: the planet's name and the disk; ?loading=0 leaves it out", async () => {
    const { page, errors } = await open("map=nab2&ai=off&shadows=off", { wait: () => window.__loading });
    await page.waitForTimeout(1500);
    await page.screenshot({ path: `${OUT}/loading-nab2-no-pictures.png`, timeout: 10000 }).catch(() => {});
    await page.waitForFunction(() => window.__bf, null, { timeout: 180000 });
    await page.waitForFunction(() => window.__loading.closed, null, { timeout: 30000 });
    assert.deepEqual(errors, []);
    await page.close();
    const off = await open("map=nab2&ai=off&shadows=off&loading=0", { wait: () => window.__bf });
    assert.equal(await off.page.evaluate(() => Boolean(document.querySelector("#loading"))), false);
    await off.page.close();
  });

  await check("?side= and ?unit= without a map still boot a battle (the older tests' URLs)", async () => {
    const { page, errors } = await open("side=cis&unit=cis_inf_droideka&shadows=off", { wait: () => window.__bf });
    assert.equal(await page.evaluate(() => Boolean(window.__shell)), false);
    assert.deepEqual(errors, []);
    await page.close();
  });
} finally {
  await browser.close();
}

console.log(`screenshots in ${OUT}`);
if (failures.length) {
  console.log(`${failures.length} failed: ${failures.join("; ")}`);
  process.exit(1);
}
