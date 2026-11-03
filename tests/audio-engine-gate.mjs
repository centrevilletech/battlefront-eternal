// The voice engine's exactness gate, its second rule (docs/audio.md, "The
// voice engine"): with three or more voices into one input, the engine's
// render may differ from a native render by no more than two native renders
// of the same scene differ from each other. Chromium adds a node input's
// connections in an order of its own that changes from graph to graph, so
// its renders of one scene differ in the last bits; the engine adds them in
// slot order. Each scene (the enhanced mode with its reverb on, and the
// light mix) renders natively in PAGES pages and with ?voices=rust in one,
// offline, many voices moving every frame; the engine's largest difference
// from any native render must be within the largest between two of them.
// The first rule (one or two voices, bit for bit) is in tests/audio-checks.js.
//
// Start Vite separately (or set AUDIO_TEST_URL). Set PLAYWRIGHT_MODULE to an
// installed Playwright module if it isn't in this project's node path.
// The largest difference is an extreme value: over a handful of pages it
// moves by a dB or two from run to run, so each line gives the median
// difference too (the engine's against the natives', and theirs among
// themselves).
// env: VOICES (24), SECONDS (3), PAGES (6)
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const base = process.env.AUDIO_TEST_URL || "http://127.0.0.1:18449";
const VOICES = +(process.env.VOICES || 24);
const SECONDS = +(process.env.SECONDS || 3);
const PAGES = Math.max(2, +(process.env.PAGES || 6));

// One render of the scene in a fresh page: `quality` enhanced (reverb on) or
// original (on stereo: the light mix); `engine` with ?voices=rust.
async function render(browser, quality, engine) {
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => {
    if (m.type() === "error" && !m.location()?.url?.includes("/audio/hrtf/")) errors.push(m.text());
  });
  await page.route("**/__engine-gate__*", (route) => route.fulfill({ contentType: "text/html", body: "<!doctype html><body></body>" }));
  await page.goto(`${base}/__engine-gate__${engine ? "?voices=rust" : ""}`);
  const result = await page.evaluate(
    async ({ voices, seconds, quality }) => {
      const { AudioEngine } = await import("/src/audio/engine.js");
      const { SoundData } = await import("/src/audio/data.js");
      const RATE = 48000;
      const ctx = new OfflineAudioContext(2, RATE * seconds, RATE);
      const pcm = new Int16Array(22050);
      let seed = 1;
      const rand = () => ((seed = (Math.imul(seed, 1103515245) + 12345) >>> 0) / 4294967296) * 2 - 1;
      for (let i = 0; i < pcm.length; i++) pcm[i] = Math.round(6000 * Math.sin(i / 7) + 3000 * rand());
      const name = (i) => `0x0000e${String(i).padStart(3, "0")}`;
      const manifest = {
        objects: [
          { type: "Bus", Name: "0x00000001", Gain: 1 },
          { type: "Bus", Name: "0x00000002", Gain: 0.8, Parent: "0x00000001" },
          ...Array.from({ length: voices }, (_, i) => ({
            type: "SoundProperties", Name: name(i), Bus: "0x00000002", Mode3D: 1, MinDistance: 2, MaxDistance: 400, MuteDistance: 400,
            RollOff: 1, Looping: 1, Bias: 0.0001, ReverbGain: 0.5, PitchVariation: 0, ModeDoppler: 1, SampleList: [["0x00000100", 1]],
          })),
        ],
        samples: { "0x00000100": [0, pcm.length, 22050] },
      };
      const engine = new AudioEngine({ context: ctx });
      if (quality === "enhanced") engine.renderer.setRoom({ room: 0.5, decay: 1.2, reflections: 0.3 });
      const config = { quality, output: "stereo", dynamics: "normal", mono: false };
      await engine.renderer.load(null, config);
      await engine.renderer.configure(config);
      engine.setData(new SoundData(manifest, pcm.buffer));
      engine.shape = (voice) => {
        const e = voice.extra;
        e.cutoff = voice.distance > 10 ? Math.min(Math.max(20000 / (1 + (voice.distance - 10) / 35), 1800), 20000) : 20000;
        e.gain = 1;
        e.send = 1;
        e.lfe = 0;
        return e;
      };
      for (let i = 0; i < voices; i++) {
        const a = (i / voices) * Math.PI * 2;
        const r = 5 + ((i * 13) % 90);
        engine.play(name(i), { position: { x: r * Math.sin(a), y: 0, z: r * Math.cos(a) }, loop: true, category: "ambience" });
      }
      // Each frame's messages reach the worklet before its block renders.
      const port = engine.renderer.mixer?.port;
      const waits = new Map();
      let ids = 0;
      if (port) {
        port.onmessage = (e) => {
          if (e.data?.type === "sync") waits.get(e.data.id)?.();
        };
      }
      const sync = () => (port ? new Promise((r) => waits.set(++ids, r) && port.postMessage({ type: "sync", id: ids })) : Promise.resolve());
      const step = (k) => {
        const t = k / 60;
        engine.setListener({ x: 3 * Math.sin(t), y: 1.6, z: Math.sin(t * 0.3) }, { x: -Math.sin(t * 0.8), y: 0, z: -Math.cos(t * 0.8) }, { x: 0, y: 1, z: 0 });
        engine.update(1 / 60);
      };
      step(0);
      for (let k = 1; k / 60 < seconds; k++) {
        ctx.suspend((Math.round(((k / 60) * RATE) / 128) * 128) / RATE).then(async () => {
          step(k);
          await sync();
          ctx.resume();
        });
      }
      await sync();
      const out = await ctx.startRendering();
      return { channels: [Array.from(out.getChannelData(0)), Array.from(out.getChannelData(1))], engine: engine.renderer.engine, light: engine.renderer.light };
    },
    { voices: VOICES, seconds: SECONDS, quality },
  );
  await page.close();
  if (errors.length) throw new Error(errors.join("\n"));
  return result;
}

// The largest difference between two renders.
function largest(a, b) {
  let max = 0;
  for (let c = 0; c < a.channels.length; c++) for (let i = 0; i < a.channels[c].length; i++) max = Math.max(max, Math.abs(a.channels[c][i] - b.channels[c][i]));
  return max;
}
const db = (x) => (x > 0 ? `${(20 * Math.log10(x)).toFixed(1)} dBFS` : "identical");
const median = (list) => {
  const a = [...list].sort((x, y) => x - y);
  return a.length % 2 ? a[(a.length - 1) / 2] : (a[a.length / 2 - 1] + a[a.length / 2]) / 2;
};

const browser = await chromium.launch({ args: ["--autoplay-policy=no-user-gesture-required"] });
let failed = 0;
try {
  for (const [quality, label] of [
    ["enhanced", "Enhanced, reverb on"],
    ["original", "the light mix"],
  ]) {
    const natives = [];
    for (let p = 0; p < PAGES; p++) natives.push(await render(browser, quality, false));
    const engine = await render(browser, quality, true);
    if (!engine.engine || natives.some((n) => n.engine) || engine.light !== (quality === "original")) throw new Error(`${label}: the engine did not engage as asked`);
    const pairs = [];
    for (let i = 0; i < natives.length; i++) for (let j = i + 1; j < natives.length; j++) pairs.push(largest(natives[i], natives[j]));
    const offs = natives.map((n) => largest(engine, n));
    const spread = Math.max(...pairs);
    const off = Math.max(...offs);
    const ok = off <= spread;
    if (!ok) failed++;
    console.log(`${ok ? "ok  " : "FAIL"} ${label}, ${VOICES} voices, ${SECONDS} s, ${PAGES} native pages: native against native ${db(spread)} at most (median ${db(median(pairs))}), engine against native ${db(off)} at most (median ${db(median(offs))})`);
  }
} finally {
  await browser.close();
}
if (failed) process.exit(1);
