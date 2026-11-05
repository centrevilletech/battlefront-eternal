// Live whole-engine cost over synthetic PCM. Offline renders do not reach
// real-time optimizing tiers and must not be reported as playback cost.
// Connect to a dedicated, already running Chromium (use a local emulator slot for
// comparable numbers), with a Vite server serving this checkout:
//   AUDIO_CDP_URL=http://127.0.0.1:9222 AUDIO_TEST_URL=http://127.0.0.1:18449 \
//     node tests/audio-bench.mjs
// QUALITY=original|enhanced, OUTPUT=stereo|5.1|7.1|7.1.4|headphones,
// MONO=1, BINAURAL=1, VOICES=32, BENCH_WARM=10, BENCH_SECONDS=15,
// BENCH_REPS=4. Binaural uses generated 256-tap filters, no game assets.
import fs from "node:fs";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const endpoint = process.env.AUDIO_CDP_URL;
if (!endpoint) throw new Error("Set AUDIO_CDP_URL to a running Chromium debug endpoint; no measurement started");
if (!fs.existsSync("/proc/self/task")) throw new Error("This benchmark needs Linux audio-thread CPU counters");
const base = process.env.AUDIO_TEST_URL || "http://127.0.0.1:18449";
const quality = process.env.QUALITY || "enhanced", output = process.env.OUTPUT || "stereo";
const mono = process.env.MONO === "1", binaural = process.env.BINAURAL === "1";
const voices = Number(process.env.VOICES || 32), warm = Number(process.env.BENCH_WARM || 10);
const seconds = Number(process.env.BENCH_SECONDS || 15), reps = Number(process.env.BENCH_REPS || 4);
const pause = (seconds) => new Promise((resolve) => setTimeout(resolve, seconds * 1000));
function counters(pids) {
  const result = new Map();
  for (const pid of pids) {
    for (const tid of fs.readdirSync(`/proc/${pid}/task`)) {
      try {
        const at = `/proc/${pid}/task/${tid}/`;
        if (/audio/i.test(fs.readFileSync(at + "comm", "utf8"))) result.set(`${pid}/${tid}`, Number(fs.readFileSync(at + "schedstat", "utf8").split(" ")[0]));
      } catch { /* A thread may retire while enumerating. */ }
    }
  }
  return result;
}
const browser = await chromium.connectOverCDP(endpoint, { timeout: 5000 });
const context = browser.contexts()[0];
if (!context) throw new Error("The connected browser has no audio-capable context");
// Electron exposes its existing app tab but cannot create another target.
const page = context.pages()[0];
if (!page) throw new Error("Open a blank tab in the dedicated benchmark browser");
try {
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("requestfailed", (r) => errors.push(`${r.url()}: ${r.failure()?.errorText}`));
  await page.route("**/__audio-bench__", (r) => r.fulfill({ contentType: "text/html", body: "<!doctype html>" }));
  const system = await browser.newBrowserCDPSession(), values = [];
  for (let rep = 0; rep < reps; rep++) {
    await page.goto(`${base}/__audio-bench__`, { timeout: 5000 });
    const info = await page.evaluate(async ({ quality, output, mono, binaural, voices }) => {
      const [{ AudioEngine }, { SoundData }] = await Promise.all([import("/src/audio/engine.js"), import("/src/audio/data.js")]);
      const ctx = new AudioContext({ latencyHint: "interactive", sampleRate: 48000 });
      const channels = ({ "5.1": 6, "7.1": 8, "7.1.4": 12 })[output] || 2;
      // A unity output adapter lets a stereo null device accept the full
      // requested graph. It performs no game mixing or signal processing.
      const sink = ctx.createGain(); sink.channelCount = channels; sink.channelCountMode = "explicit"; sink.channelInterpretation = "discrete";
      sink.connect(ctx.destination); Object.defineProperty(sink, "maxChannelCount", { value: channels }); Object.defineProperty(ctx, "destination", { value: sink });
      const engine = new AudioEngine({ context: ctx }), r = engine.renderer;
      const config = { quality, output, mono, dynamics: "normal", clarity: true };
      if (binaural) r.decoder = { filters: Array.from({ length: 16 }, (_, c) => {
        const b = ctx.createBuffer(2, 256, 48000);
        for (let e = 0; e < 2; e++) b.getChannelData(e)[c + e + 1] = 0.1 / (c + 1);
        return b;
      }) };
      r.setRoom({ room: 0.5, decay: 1.2, reflections: 0.3 });
      await r.load(null, config); await r.configure(config);
      const pcm = new Int16Array(22050);
      for (let i = 0; i < pcm.length; i++) pcm[i] = Math.round(8000 * Math.sin(i / 7));
      const name = (i) => `0x0000e${String(i).padStart(3, "0")}`;
      engine.setData(new SoundData({ objects: Array.from({ length: voices }, (_, i) => ({
        type: "SoundProperties", Name: name(i), Mode3D: 1, MinDistance: 2, MaxDistance: 400, MuteDistance: 400,
        RollOff: 1, Looping: 1, PitchVariation: 0, ReverbGain: 0.5, SampleList: [["0x00000100", 1]],
      })), samples: { "0x00000100": [0, pcm.length, 22050] } }, pcm.buffer));
      for (let i = 0; i < voices; i++) engine.play(name(i), { position: { x: 30 * Math.sin(i), y: 2, z: 30 * Math.cos(i) }, loop: true, category: "ambience" });
      for (const [channels, kind] of [[4, "ambience"], [2, "music"], [1, "voice"]]) {
        const b = ctx.createBuffer(channels, 96000, 48000);
        for (let c = 0; c < channels; c++) for (let i = 0; i < b.length; i++) b.getChannelData(c)[i] = 0.06 * Math.sin(i * (c + 1) * 0.03);
        const lane = r.stream(channels, kind); lane.gain.value = 0.6; lane.start(b, ctx.currentTime, 1, true);
      }
      const probe = ctx.createAnalyser(); r.output.connect(probe);
      let tick = 0;
      const timer = setInterval(() => {
        const t = ++tick / 60;
        engine.setListener({ x: 3 * Math.sin(t), y: 1.6, z: Math.sin(t * 0.3) }, { x: -Math.sin(t * 0.8), y: 0, z: -Math.cos(t * 0.8) }, { x: 0, y: 1, z: 0 });
        engine.update(1 / 60);
      }, 1000 / 60);
      window.audioBench = { ctx, timer, probe, r };
      await ctx.resume();
      return { rate: ctx.sampleRate, state: ctx.state, channels: r.channels, output: r.mode, binaural: r.binaural };
    }, { quality, output, mono, binaural, voices });
    if (info.state !== "running" || info.output !== output || info.binaural !== binaural) throw new Error(`Graph preflight failed: ${JSON.stringify(info)}`);
    await pause(warm);
    const check = async () => page.evaluate(() => {
      const { probe, r } = window.audioBench, data = new Float32Array(2048); probe.getFloatTimeDomainData(data);
      return { peak: data.reduce((p, v) => Math.max(p, Math.abs(v)), 0), error: r.error?.message };
    });
    const signal = await check();
    if (errors.length || signal.error || !(signal.peak > 0.001)) throw new Error(`Audio preflight failed: ${JSON.stringify({ signal, errors })}`);
    const pids = (await system.send("SystemInfo.getProcessInfo")).processInfo.filter((p) => p.type === "renderer").map((p) => p.id);
    const before = counters(pids);
    if (!before.size) throw new Error("No audio thread counters; refusing to report zero cost");
    const start = process.hrtime.bigint(); await pause(seconds); const after = counters(pids), wall = Number(process.hrtime.bigint() - start);
    let cpu = 0; for (const [key, ns] of after) if (before.has(key)) cpu += ns - before.get(key);
    const final = await check();
    if (errors.length || final.error || !(final.peak > 0.001)) throw new Error(`Audio failed during measurement: ${JSON.stringify({ final, errors })}`);
    values.push(cpu / wall * 100);
    await page.evaluate(async () => { clearInterval(window.audioBench.timer); await window.audioBench.ctx.close(); });
    console.log(`round ${rep + 1}: ${values.at(-1).toFixed(1)}% of a core; ${JSON.stringify(info)}`);
  }
  values.sort((a, b) => a - b);
  console.log(`${quality} ${output}, mono=${mono}, binaural=${binaural}, ${voices} voices + 3 beds: median ${((values[Math.floor((values.length - 1) / 2)] + values[Math.floor(values.length / 2)]) / 2).toFixed(1)}% of a core`);
} finally { await page.goto("about:blank"); await browser.close(); }
