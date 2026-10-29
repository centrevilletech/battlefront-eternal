// Offline audio checks in Chromium: panning and routing per layout, the
// LFE, the limiter, the loudness meter, the reverb, and the voice engine.
// Start Vite separately (or set AUDIO_TEST_URL). Set PLAYWRIGHT_MODULE to an
// installed Playwright module if it isn't in this project's node path. No
// game assets required.
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const base = process.env.AUDIO_TEST_URL || "http://127.0.0.1:18449";
const browser = await chromium.launch({ args: ["--autoplay-policy=no-user-gesture-required"] });
try {
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => {
    // The headphone decoder's files are optional (tools/make_binaural.py).
    if (m.type() === "error" && !m.location()?.url?.includes("/audio/hrtf/")) errors.push(m.text());
  });
  await page.route("**/__audio-check__", (route) => route.fulfill({ contentType: "text/html", body: "<!doctype html><body></body>" }));
  await page.goto(`${base}/__audio-check__`);
  const result = await page.evaluate(async () => (await import("/tests/audio-checks.js")).checkAudio());
  if (errors.length) throw new Error(errors.join("\n"));
  for (const r of result) console.log(`${r.ok ? "ok  " : "FAIL"} ${r.name}`);
  console.log(`${result.length} checks passed`);
} finally {
  await browser.close();
}
