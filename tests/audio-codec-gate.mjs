// Start the repository's Vite server first, as for audio-engine-gate.mjs.
// This checks the real worker, byte-range I/O and streaming output hosts.
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const browser = await chromium.launch({ args: ["--autoplay-policy=no-user-gesture-required"] });
try {
  const page = await browser.newPage(), errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.route("**/__codec-check__", (r) => r.fulfill({ contentType: "text/html", body: "<!doctype html>" }));
  await page.goto(`${process.env.AUDIO_TEST_URL || "http://127.0.0.1:18449"}/__codec-check__`);
  await page.exposeFunction("codecReport", (r) => console.log(`${r.pass ? "ok" : "FAIL"} ${r.name}: ${r.detail}`));
  const results = await page.evaluate(() => Promise.race([
    import("/tests/audio-codec-checks.js").then((m) => m.codecChecks(window.codecReport)),
    new Promise((_, reject) => setTimeout(() => reject(Error("codec checks did not finish within 30 seconds")), 30000)),
  ]));
  // Loading and instantiation failures must reject at the required module
  // boundary. Each case gets a fresh worker and module cache.
  for (const failure of ["worker-load", "wasm-instantiation"]) {
    const failedPage = await browser.newPage();
    await failedPage.route("**/__codec-check__", (r) => r.fulfill({ contentType: "text/html", body: "<!doctype html>" }));
    if (failure === "worker-load") await failedPage.route("**/src/audio/codec-worker.js*", (r) => r.abort());
    else await failedPage.route("**/src/audio-codec-wasm.js*", (r) => r.fulfill({ contentType: "text/javascript", body: 'export const AUDIO_CODEC_WASM = "AA==";' }));
    await failedPage.goto(`${process.env.AUDIO_TEST_URL || "http://127.0.0.1:18449"}/__codec-check__`);
    try {
      const message = await failedPage.evaluate(async () => {
        const { openAudio } = await import("/src/audio/codec.js");
        try { await openAudio("/tests/fixtures/audio-codecs/opus-2ch.opus"); return "unexpected success"; }
        catch (error) { return error.message; }
      });
      const pass = /^audio:.*Rust.*(failed|decoding failed)/.test(message);
      results.push({ name: failure, pass, detail: message });
      console.log(`${pass ? "ok" : "FAIL"} ${failure}: ${message}`);
    } finally { await failedPage.close(); }
  }
  const failed = results.filter((r) => !r.pass).length;
  console.log(`${results.length - failed}/${results.length} codec/stream checks passed`);
  if (failed || errors.length) { console.error(errors.join("\n")); process.exitCode = 1; }
} finally { await browser.close(); }
