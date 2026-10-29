// The loading screen over a real battle load, in headless Chromium, on
// whatever data the server has (public/ or a game-data link): the sequence
// runs its whole course (galaxy, planet, surface, the level) before the
// screen goes, however fast the battle loads; the battle is ready and held
// until then; the pulse and the zooms are on the audio clock, each heard
// when the frame that shows its moment does; and the screen's own frames,
// the load's steps and the page's long tasks are measured and printed.
//
// Under SwiftShader (the default) the GPU process is software and every
// shader link and upload of the battle stalls the screen's canvas for
// hundreds of milliseconds, so the hitches are only reported. With
// LOADING_GPU=gl (or vulkan) Chromium draws on the machine's GPU, as a
// player's does, and the screen must not hitch (docs/shell.md).
//
//   PLAYWRIGHT_MODULE=.../playwright/index.mjs CHROMIUM=/usr/bin/chromium \
//     node tests/shell-loading-browser.mjs
//   LOADING_QUERY="map=nab2&ai=off&shadows=off" picks the battle (geo1 by
//   default); SHELL_TEST_URL another server (http://127.0.0.1:18449).
import assert from "node:assert/strict";

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const BASE = process.env.SHELL_TEST_URL || "http://127.0.0.1:18449";
const QUERY = process.env.LOADING_QUERY || "map=geo1&ai=off&shadows=off";
const GPU = process.env.LOADING_GPU || "";
const FLAGS = {
  gl: ["--use-angle=gl", "--ignore-gpu-blocklist"],
  vulkan: ["--use-angle=vulkan", "--enable-features=Vulkan", "--ignore-gpu-blocklist"],
  "": ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"],
};
// A sound is heard AV after its moment (loading-audio.js).
const AV = 1000 / 30;

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM || undefined,
  args: [...(FLAGS[GPU] || FLAGS[""]), "--autoplay-policy=no-user-gesture-required"],
});
const failures = [];

async function check(name, fn) {
  try {
    await fn();
    console.log(`ok - ${name}`);
  } catch (error) {
    failures.push(name);
    console.log(`not ok - ${name}\n  ${error.stack.split("\n").slice(0, 4).join("\n  ")}`);
  }
}

// The screen's state and the battle's, sampled every 100 ms until the
// screen closes.
async function watch(page) {
  const samples = [];
  for (let i = 0; i < 1200; i++) {
    const sample = await page.evaluate(() => {
      const loading = window.__loading;
      return {
        t: Math.round(performance.now()), closed: loading.closed, value: loading.value,
        stage: loading.sequence?.stage ?? 0, phase: loading.sequence?.phase ?? "", done: loading.sequence?.done === true,
        cue: loading.sequence?.cue ?? "", frames: loading.frames, battle: Boolean(window.__bf),
      };
    });
    samples.push(sample);
    if (sample.closed) break;
    await page.waitForTimeout(100);
  }
  return samples;
}

try {
  await check(`?${QUERY}: the screen runs its whole course, the battle waits for it, each sound is heard with its frame`, async () => {
    const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
    const errors = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.addInitScript(() => {
      window.__long = [];
      try {
        new PerformanceObserver((list) => {
          for (const e of list.getEntries()) window.__long.push([Math.round(e.startTime), Math.round(e.duration)]);
        }).observe({ type: "longtask", buffered: true });
      } catch {
        // No long task timing here.
      }
    });
    await page.goto(`${BASE}/?${QUERY}`);
    await page.waitForFunction(() => window.__loading, null, { timeout: 60000 });
    const samples = await watch(page);
    const last = samples.at(-1);
    await page.waitForFunction(() => window.__bf, null, { timeout: 180000 });
    await page.evaluate(() => window.__bf.ready);
    // The sound's context closes a moment after the screen.
    await page.waitForTimeout(400);
    const after = await page.evaluate(() => {
      const loading = window.__loading;
      return { steps: loading.steps, long: window.__long, origin: performance.timeOrigin, plan: loading.plan, start: loading.start, endsAt: loading.endsAt,
        shown: loading.sequence?.shown ?? [], frames: loading.frames, sound: loading.sound?.state() ?? null };
    });
    const { steps, long, plan, start, endsAt, sound } = after;
    const page0 = (ms) => ms - after.origin;

    // The timeline as seen: when each stage and phase first showed.
    const timeline = [];
    let key = null;
    for (const s of samples) {
      const k = `${s.stage}${s.phase[0] || ""}`;
      if (k !== key) { timeline.push(`${(s.t / 1000).toFixed(1)}s ${k}`); key = k; }
    }
    const ready = steps.find(([step]) => step === "ready")?.[1];
    const frames = after.frames || last.frames;
    console.log(`  steps (ms): ${steps.map(([step, at]) => `${step} ${at}`).join(", ")}`);
    console.log(`  sequence: ${plan?.stages} pictures, ${plan?.duration} s from ${Math.round(page0(start))} ms; timeline: ${timeline.join("  ")}; closed at ${(last.t / 1000).toFixed(1)} s on stage ${last.stage} ${last.phase}`);
    console.log(`  screen frames: ${frames?.count} drawn, worst gap ${Math.round(frames?.worst ?? 0)} ms, ${frames?.late} over 50 ms ${JSON.stringify(frames?.hitches ?? [])}`);
    console.log(`  main thread: ${long.length} long tasks, ${long.reduce((a, [, d]) => a + d, 0)} ms blocked; longest ${long.slice().sort((a, b) => b[1] - a[1]).slice(0, 5).map(([s, d]) => `${d} ms @${(s / 1000).toFixed(1)}s`).join(", ")}`);

    assert.ok(last.closed, "the screen closed");
    assert.deepEqual(steps.map(([step]) => step), ["data", "world", "shaders", "shaders", "ready"], "createGame's steps, then ready");
    assert.ok(plan && start !== null, "the pictures came in and the sequence started");
    const pictures = await page.evaluate(() => fetch("/common/shell/shell.json").then((r) => r.json()).then((d) => d.maps.find((m) => m.id === new URLSearchParams(location.search).get("map"))?.load?.stages?.length ?? 0).catch(() => 0));
    if (pictures >= 2) {
      assert.equal(plan.stages, pictures, `every picture loaded (${plan.stages} of ${pictures})`);
      assert.ok(Math.max(...samples.map((s) => s.stage)) >= pictures - 2, "the pictures went by");
      // The original game's timing: 3.5 s of tracking and a 1 s zoom a picture.
      assert.ok(Math.abs(plan.duration - (pictures - 1) * 4.5) < 1e-9, `the sequence's length (${plan.duration} s)`);
    }
    const closedAt = last.t;
    assert.ok(closedAt >= page0(endsAt) - 1, `closed (${closedAt}) no earlier than the sequence's end (${Math.round(page0(endsAt))})`);
    assert.ok(ready !== undefined && closedAt >= ready, `closed (${closedAt}) after the battle was ready (${ready})`);
    assert.ok(samples.every((s, i) => i === 0 || s.value >= samples[i - 1].value), "progress only goes up");

    // The sound: the context ran (autoplay is allowed here), the three
    // sounds were fetched, a pulse for each pair of lines and a one-shot
    // for each zoom went on the audio clock, each heard when planned, and
    // after the first frame that showed its moment, by about AV.
    assert.ok(sound, "the screen had sound");
    assert.equal(sound.context, "closed", "its context is closed with the screen");
    assert.deepEqual([...sound.loaded].sort(), ["load_xtracking", "load_ytracking", "load_zoom"], "the three sounds, from the sound sub-level");
    if (pictures >= 2) {
      assert.equal(sound.loops, 2 * (pictures - 1), `a pulse for each pair of lines (${sound.loops})`);
      assert.equal(sound.once, pictures - 1, `a zoom sound for each zoom (${sound.once})`);
      assert.equal(sound.missed, 0, "none too late to play");
    }
    const shown = after.shown.slice();
    const rows = sound.log.map((entry) => {
      const planned = plan.sounds.find((s) => s.cue === entry.cue && Math.abs(start + (s.at * 1000 + AV) - entry.at) < 1);
      assert.ok(planned, `${entry.cue} at ${entry.at} is in the plan`);
      assert.ok(Math.abs(entry.heard - entry.at) < 2, `${entry.cue} heard ${(entry.heard - entry.at).toFixed(1)} ms off its plan`);
      const i = shown.findIndex(([cue, at]) => cue === entry.cue && Math.abs(at + AV - entry.at) < 1);
      const frame = i >= 0 ? shown.splice(i, 1)[0][2] : null;
      return { cue: entry.cue, at: Math.round(page0(entry.at)), frame: frame === null ? null : entry.heard - frame };
    });
    console.log(`  sounds (page ms, heard after the frame showing them): ${rows.map((r) => `${r.cue}@${r.at} ${r.frame === null ? "-" : `+${r.frame.toFixed(1)}`}`).join("  ")}`);
    if (GPU) {
      // On a GPU the screen draws every refresh: the frame showing each
      // moment comes within a frame of it, and the sound after it.
      for (const r of rows) assert.ok(r.frame !== null && r.frame >= 0 && r.frame <= AV + 1, `${r.cue}@${r.at}: heard ${r.frame} ms after its frame`);
      assert.ok((frames?.worst ?? 0) < 120 && (frames?.late ?? 0) <= 2, `the screen drew smoothly: worst ${Math.round(frames?.worst ?? 0)} ms, ${frames?.late} over 50 ms`);
    }
    assert.ok(errors.every((e) => /404/.test(e)), `no page errors: ${errors.join(" | ")}`);
    await page.close();
  });

  await check("?loading=0: no screen, no sound, the battle starts at once", async () => {
    const page = await browser.newPage({ viewport: { width: 640, height: 360 } });
    await page.goto(`${BASE}/?${QUERY}&loading=0`);
    await page.waitForFunction(() => window.__bf, null, { timeout: 180000 });
    assert.equal(await page.evaluate(() => Boolean(document.querySelector("#loading"))), false);
    assert.equal(await page.evaluate(() => window.__loading.sound), null);
    await page.close();
  });
} finally {
  await browser.close();
}

if (failures.length) {
  console.log(`\n${failures.length} failed: ${failures.join("; ")}`);
  process.exit(1);
}
console.log("\nall loading screen checks passed");
