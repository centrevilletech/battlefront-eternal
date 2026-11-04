// The voice engine's exactness gate, its second rule (docs/audio.md, "The
// voice engine"): with three or more voices into one input, Chromium adds a
// node input's connections in an order of its own that changes from graph
// to graph, so its renders of one scene differ in the last bits; the engine
// adds them in slot order. Each scene (the enhanced mode with its reverb on,
// and the light mix) renders with the browser's nodes (?voices=native) in
// PAGES pages and with the voice engine (the default) in one, offline, many
// voices moving every frame, once a seed (the scene's noise and the
// listener's path).
//
// - The light mix: the engine's largest difference from any native render
//   must be within the largest between any two of them.
// - The enhanced mode: every sample of the engine's render must be within
//   LIMIT_DB of every native render, under the smallest step of 16-bit
//   sound. Its reverb spreads the rounding over its tail, so the largest
//   difference is an extreme value that moves from seed to seed, and the
//   engine meets the natives' own largest about half the time, as one more
//   native render would; that comparison is still reported, like with like
//   (the engine against the PAGES natives is PAGES pairs, so PAGES native
//   pairs are drawn at random, from the seed: the engine's largest within
//   theirs, its median within 0.5 dB of theirs), as a rate over the seeds.
// The first rule (one or two voices, bit for bit) is in tests/audio-checks.js.
//
// Start Vite separately (or set AUDIO_TEST_URL). Set PLAYWRIGHT_MODULE to an
// installed Playwright module if it isn't in this project's node path.
// env: VOICES (24), SECONDS (3), PAGES (6), SEEDS (5); MASTER=1 renders the
// engine's side with its master chain in the module too, as the game does, and
// BEDS=1 adds a battle's three stream beds to the scene on both sides.
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const base = process.env.AUDIO_TEST_URL || "http://127.0.0.1:18449";
const VOICES = +(process.env.VOICES || 24);
const SECONDS = +(process.env.SECONDS || 3);
const PAGES = Math.max(2, +(process.env.PAGES || 6));
const SEEDS = Math.max(1, +(process.env.SEEDS || 5));
const LIMIT_DB = -96;
const MASTER = Boolean(process.env.MASTER);
const BEDS = Boolean(process.env.BEDS);

// One render of the scene in a fresh page: `quality` enhanced (reverb on) or
// original (on stereo: the light mix); `engine` the voice engine (the
// default), else the browser's nodes (?voices=native).
async function render(browser, quality, engine, sceneSeed) {
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => {
    if (m.type() === "error" && !m.location()?.url?.includes("/audio/hrtf/")) errors.push(m.text());
  });
  await page.route("**/__engine-gate__*", (route) => route.fulfill({ contentType: "text/html", body: "<!doctype html><body></body>" }));
  await page.goto(`${base}/__engine-gate__${engine ? (MASTER ? "" : "?master=native") : "?voices=native"}`);
  const result = await page.evaluate(
    async ({ voices, seconds, quality, sceneSeed, beds }) => {
      const { AudioEngine } = await import("/src/audio/engine.js");
      const { SoundData } = await import("/src/audio/data.js");
      const RATE = 48000;
      const ctx = new OfflineAudioContext(2, RATE * seconds, RATE);
      const pcm = new Int16Array(22050);
      let seed = sceneSeed;
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
      // Ambience, music and a voice-over line, each through a stream's gain.
      for (const [count, kind, freq] of beds ? [[4, "ambience", 90], [2, "music", 180], [1, "voice", 400]] : []) {
        const buffer = ctx.createBuffer(count, RATE * seconds, RATE);
        for (let c = 0; c < count; c++) {
          const d = buffer.getChannelData(c);
          for (let i = 0; i < d.length; i++) d[i] = 0.1 * Math.sin((2 * Math.PI * freq * (c + 1) * i) / RATE + sceneSeed + c) + 0.02 * rand();
        }
        const gain = ctx.createGain();
        gain.gain.value = 0.6;
        const source = ctx.createBufferSource();
        source.buffer = buffer;
        source.connect(engine.renderer.bed(count, gain, kind).input);
        source.start();
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
        const t = k / 60 + sceneSeed * 0.7;
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
      return { channels: [Array.from(out.getChannelData(0)), Array.from(out.getChannelData(1))], engine: engine.renderer.engine, master: engine.renderer.master, light: engine.renderer.light };
    },
    { voices: VOICES, seconds: SECONDS, quality, sceneSeed, beds: BEDS },
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
const dB = (x) => (x > 0 ? 20 * Math.log10(x) : -Infinity);
const median = (list) => {
  const a = [...list].sort((x, y) => x - y);
  return a.length % 2 ? a[(a.length - 1) / 2] : (a[a.length / 2 - 1] + a[a.length / 2]) / 2;
};
// A seeded draw (mulberry32), for the native pairs.
const draw = (seed) => () => {
  seed = (seed + 0x6d2b79f5) >>> 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};

const browser = await chromium.launch({ args: ["--autoplay-policy=no-user-gesture-required"] });
let failed = 0;
const rate = { max: 0, medians: 0, both: 0 };
let worst = 0;
try {
  for (let seed = 1; seed <= SEEDS; seed++) {
    for (const [quality, label] of [
      ["enhanced", "Enhanced, reverb on"],
      ["original", "the light mix"],
    ]) {
      const natives = [];
      for (let p = 0; p < PAGES; p++) natives.push(await render(browser, quality, false, seed));
      const engine = await render(browser, quality, true, seed);
      if (!engine.engine || natives.some((n) => n.engine) || engine.light !== (quality === "original") || engine.master !== (MASTER && quality === "enhanced")) throw new Error(`${label}: the engine did not engage as asked`);
      const all = [];
      for (let i = 0; i < natives.length; i++) for (let j = i + 1; j < natives.length; j++) all.push(largest(natives[i], natives[j]));
      const offs = natives.map((n) => largest(engine, n));
      const off = Math.max(...offs);
      const head = `seed ${seed}, ${label}, ${VOICES} voices, ${SECONDS} s, ${PAGES} native pages:`;
      if (quality === "original") {
        const spread = Math.max(...all);
        const ok = off <= spread;
        if (!ok) failed++;
        console.log(`${ok ? "ok  " : "FAIL"} ${head} engine against native ${db(off)} at most, native against native ${db(spread)} at most (all ${all.length} pairs)`);
        continue;
      }
      // As many native pairs as the engine has, drawn at random.
      const next = draw(seed);
      const pool = [...all];
      const pairs = [];
      while (pairs.length < offs.length && pool.length) pairs.push(pool.splice(Math.floor(next() * pool.length), 1)[0]);
      const maxOk = off <= Math.max(...pairs);
      const medOk = Math.abs(dB(median(offs)) - dB(median(pairs))) <= 0.5;
      rate.max += maxOk;
      rate.medians += medOk;
      rate.both += maxOk && medOk;
      worst = Math.max(worst, off);
      const ok = dB(off) < LIMIT_DB;
      if (!ok) failed++;
      console.log(
        `${ok ? "ok  " : "FAIL"} ${head} engine against native ${db(off)} at most, median ${db(median(offs))}; ${pairs.length} native pairs ${db(Math.max(...pairs))} at most, median ${db(median(pairs))}${maxOk && medOk ? ", like with like" : ""}`,
      );
    }
  }
  console.log(`Enhanced over ${SEEDS} seeds: the largest difference ${db(worst)} (the limit ${LIMIT_DB} dBFS); like with like, the largest within the natives' ${rate.max}/${SEEDS}, the medians within 0.5 dB ${rate.medians}/${SEEDS}, both ${rate.both}/${SEEDS}`);
} finally {
  await browser.close();
}
if (failed) process.exit(1);
