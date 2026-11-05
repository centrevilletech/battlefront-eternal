// Recorded full-graph synthetic renders, in Chromium OfflineAudioContext.
// Start Vite separately. AUDIO_TEST_URL chooses it; OUTPUT and QUALITY
// optionally select a subset. CI exercises the same inputs with a PCM host.
import { readFile } from "node:fs/promises";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const base = process.env.AUDIO_TEST_URL || "http://127.0.0.1:18449";
const manifest = JSON.parse(await readFile(new URL("./fixtures/audio/renders.json", import.meta.url)));
const browser = await chromium.launch({ args: ["--autoplay-policy=no-user-gesture-required"] });
let failed = 0, total = 0;
try {
  for (const scene of manifest.scenes) {
    if (process.env.OUTPUT && scene.output !== process.env.OUTPUT) continue;
    if (process.env.QUALITY && scene.quality !== process.env.QUALITY) continue;
    const page = await browser.newPage(); const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
    await page.route("**/__audio-check__", (route) => route.fulfill({ contentType: "text/html", body: "<!doctype html><body></body>" }));
    try {
      await page.goto(`${base}/__audio-check__`);
      const result = await page.evaluate(async (name) => {
        const checks = import("/tests/audio-checks.js").then((m) => m.audioChecks({ name }));
        return Promise.race([checks, new Promise((_, reject) => setTimeout(() => reject(new Error("audio render did not complete within 30 seconds")), 30000))]);
      }, scene.name);
      const r = result[0];
      const pass = r?.pass && !errors.length;
      console.log(`${pass ? "ok" : "FAIL"} ${scene.name}: ${r?.detail || "no render"}${errors.length ? "; " + errors.join("; ") : ""}`);
      if (!pass) failed++;
    } catch (error) { console.log(`FAIL ${scene.name}: ${error.message}; ${errors.join("; ")}`); failed++; }
    finally { total++; await page.close(); }
  }
} finally { await browser.close(); }
console.log(`${total - failed}/${total} recorded render checks passed`);
if (failed) process.exitCode = 1;
