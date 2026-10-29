// What the sound stage costs, measured in Chromium (no game assets):
//
// - The whole Renderer offline, per rendering and number of voices, with the
//   voices moving as they do in play: every frame each gets a new direction,
//   distance, level, playback rate, filter cutoff and reverb send.
// - Each worklet alone (the spatial mixer per panning law, the reverb, the
//   limiter, the stabilizer, the meter, the clarity stage).
// - The voice engine's main-thread cost per frame (AudioEngine.update with
//   made-up sounds: 48 voices, 12 of them heard).
//
// Audio costs are a percentage of real time on one core: the CPU time of the
// offline render thread (read from /proc on Linux, so other work on the
// machine moves the figures less), or wall time elsewhere. Each figure is the
// best of BENCH_REPS runs.
//
//   PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs AUDIO_TEST_URL=http://127.0.0.1:18450 \
//     node tests/audio-bench.mjs
//
// Options (environment): BENCH_REPS (3), BENCH_SECONDS of audio per render
// (6), BENCH_ONLY (comma-separated parts of the row names to run). The
// headphone decoder's rows need its filters (tools/make_binaural.py).
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const base = process.env.AUDIO_TEST_URL || "http://127.0.0.1:18449";
const REPS = Number(process.env.BENCH_REPS || 3);
const SECONDS = Number(process.env.BENCH_SECONDS || 6);
const ONLY = (process.env.BENCH_ONLY || "").split(",").filter(Boolean);
const wanted = (name) => !ONLY.length || ONLY.some((o) => name.includes(o));

// The render thread's CPU time, from /proc (Linux), for the browser whose
// profile is `dir`.
const linux = fs.existsSync("/proc/self/task");
const cmdline = (p) => {
  try {
    return fs.readFileSync(`/proc/${p}/cmdline`, "utf8").split("\0").join(" ");
  } catch {
    return "";
  }
};
const parent = (p) => {
  try {
    return fs.readFileSync(`/proc/${p}/stat`, "utf8").split(") ")[1].split(" ")[1];
  } catch {
    return null;
  }
};
function renderThreadNs(dir) {
  const pids = fs.readdirSync("/proc").filter((p) => /^\d+$/.test(p));
  const browser = pids.find((p) => cmdline(p).includes(dir) && !cmdline(p).includes("--type="));
  let ns = 0;
  for (const p of pids) {
    if (!cmdline(p).includes("--type=renderer") || parent(parent(p)) !== browser) continue;
    let tasks = [];
    try {
      tasks = fs.readdirSync(`/proc/${p}/task`);
    } catch {
      continue;
    }
    for (const t of tasks) {
      try {
        if (!fs.readFileSync(`/proc/${p}/task/${t}/comm`, "utf8").startsWith("Offline Audio")) continue;
        ns += Number(fs.readFileSync(`/proc/${p}/task/${t}/schedstat`, "utf8").split(" ")[0]);
      } catch {
        // gone
      }
    }
  }
  return ns;
}

const dir = fs.mkdtempSync(path.join(os.tmpdir(), "audio-bench-"));
const context = await chromium.launchPersistentContext(dir, { args: ["--autoplay-policy=no-user-gesture-required"] });
try {
  const page = context.pages()[0] || (await context.newPage());
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  // Cross-origin isolated, for fine-grained timers.
  await page.route("**/__audio-bench__", (route) =>
    route.fulfill({
      contentType: "text/html",
      headers: { "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp" },
      body: "<!doctype html><body></body>",
    }),
  );
  await page.goto(`${base}/__audio-bench__`);
  const decoder = await page.evaluate(() => fetch("/audio/hrtf/sadie2_d1_3oa.json").then((r) => r.ok).catch(() => false));

  // Set a render up in the page (window.job), then render it, timing the
  // render thread.
  async function measure(setup, args) {
    let best = Infinity;
    let info = "";
    for (let rep = 0; rep < REPS; rep++) {
      info = await page.evaluate(setup, args);
      const before = linux ? renderThreadNs(dir) : 0;
      const { wall, handlers } = await page.evaluate(async () => {
        const t0 = performance.now();
        await window.job.ctx.startRendering();
        return { wall: performance.now() - t0, handlers: window.job.handlers?.() || 0 };
      });
      const ms = linux ? (renderThreadNs(dir) - before) / 1e6 : wall - handlers;
      best = Math.min(best, ms);
      await page.evaluate(() => {
        window.job = null;
      });
    }
    return { ms: best, info };
  }

  // The Renderer with moving voices.
  async function renderer(args) {
    return measure(
      async ({ quality, output, channels, decoder, voices, seconds }) => {
        const { Renderer } = await import("/src/audio/spatial.js");
        const RATE = 48000;
        const ctx = new OfflineAudioContext(channels, RATE * seconds, RATE);
        const r = new Renderer(ctx);
        await r.load(decoder ? "/audio" : null);
        r.configure({ quality, output, dynamics: "normal", mono: false });
        let seed = 12345;
        const rand = () => ((seed = (Math.imul(seed, 1103515245) + 12345) >>> 0) / 4294967296);
        // Noise at 22.05 kHz, the rate of most of the game's samples.
        const noise = ctx.createBuffer(1, 22050, 22050);
        const d = noise.getChannelData(0);
        for (let i = 0; i < d.length; i++) d[i] = (rand() * 2 - 1) * 0.05;
        const vs = [];
        for (let v = 0; v < voices; v++) {
          const source = new AudioBufferSourceNode(ctx, { buffer: noise, loop: true });
          const gain = new GainNode(ctx, { gain: 0 });
          source.connect(gain);
          const out = r.voice({ mode3D: 1 });
          out.attach(gain);
          source.start(0, (v * 0.013) % 1);
          vs.push({ source, gain, out });
        }
        const local = { x: 0, y: 0, z: -1 };
        // What the engine does each frame for every heard voice.
        const frame = (t, first) => {
          const now = ctx.currentTime;
          vs.forEach((s, v) => {
            const a = t * 0.7 + v * 2.4;
            const dist = 5 + ((v * 37) % 150) + 10 * Math.sin(t * 0.9 + v);
            local.x = Math.sin(a);
            local.y = 0.1 * Math.sin(v);
            local.z = -Math.cos(a);
            const n = Math.hypot(local.x, local.y, local.z);
            local.x /= n;
            local.y /= n;
            local.z /= n;
            const level = Math.min(1, 12 / dist);
            if (first) s.gain.gain.setValueAtTime(level, now);
            else s.gain.gain.setTargetAtTime(level, now, 0.015);
            s.source.playbackRate.setTargetAtTime(0.459 * (1 + 0.02 * Math.sin(t * 2 + v)), now, 0.015);
            const cutoff = dist > 10 ? Math.min(Math.max(20000 / (1 + (dist - 10) / 35), 1800), 20000) : 20000;
            s.out.update(local, dist, 0, { cutoff, send: quality === "original" ? 0 : 0.3, lfe: v % 8 === 0 ? 1 : 0, gain: 1 });
          });
          r.flush();
        };
        let handlers = 0;
        frame(0, true);
        for (let k = 1; k / 60 < seconds; k++) {
          const at = (Math.round((k / 60) * RATE / 128) * 128) / RATE;
          ctx.suspend(at).then(() => {
            const t0 = performance.now();
            frame(k / 60, false);
            handlers += performance.now() - t0;
            ctx.resume();
          });
        }
        window.job = { ctx, handlers: () => handlers };
        return `${r.mode}${r.binaural ? ", decoder" : r.hrtf ? ", HRTF" : ""}`;
      },
      { ...args, seconds: SECONDS },
    );
  }

  // One worklet (or nothing, for the graph's own cost) fed noise.
  async function worklet(args) {
    return measure(
      async ({ kind, channels, voices, law, seconds }) => {
        const RATE = 48000;
        const ctx = new OfflineAudioContext(2, RATE * seconds, RATE);
        for (const m of ["spatial-mixer", "reverb", "limiter", "stabilizer", "meter", "clarity"]) {
          await ctx.audioWorklet.addModule(`/src/audio/worklets/${m}.js`).catch(() => null);
        }
        const { LAYOUTS, ALSOFT } = await import("/src/audio/layouts.js");
        let seed = 99;
        const rand = () => ((seed = (Math.imul(seed, 1103515245) + 12345) >>> 0) / 4294967296);
        const noise = ctx.createBuffer(1, RATE, RATE);
        const d = noise.getChannelData(0);
        for (let i = 0; i < d.length; i++) d[i] = (rand() * 2 - 1) * 0.3;
        const sources = (n) => {
          const merger = new ChannelMergerNode(ctx, { numberOfInputs: n });
          for (let k = 0; k < n; k++) {
            const s = new AudioBufferSourceNode(ctx, { buffer: noise, loop: true });
            s.connect(merger, 0, k);
            s.start(0, k * 0.017);
          }
          return merger;
        };
        const sink = new GainNode(ctx, { channelCount: 2, channelCountMode: "explicit", channelInterpretation: "discrete" });
        sink.connect(ctx.destination);
        const discrete = { channelCount: channels, channelCountMode: "explicit", channelInterpretation: "discrete" };
        if (kind === "mixer") {
          const layout = { panpot: LAYOUTS.stereo, alsoft: LAYOUTS.stereo, vbap: LAYOUTS["7.1.4"], ambi3: null }[law];
          const m = new AudioWorkletNode(ctx, "spatial-mixer", {
            numberOfInputs: 96,
            numberOfOutputs: 2,
            outputChannelCount: [channels, 1],
            processorOptions: { channels, law, speakers: layout?.speakers || [], lfe: layout?.lfe ?? -1, decoder: law === "alsoft" ? ALSOFT.stereo : null },
          });
          m.connect(sink, 0);
          for (let v = 0; v < voices; v++) {
            const s = new AudioBufferSourceNode(ctx, { buffer: noise, loop: true });
            s.connect(m, 0, v);
            s.start(0, (v * 0.013) % 1);
          }
          // New directions every frame, as in play.
          for (let k = 0; k / 60 < seconds; k++) {
            const send = () => {
              const p = new Float32Array(voices * 6);
              for (let v = 0; v < voices; v++) p.set([v, Math.sin(v * 2.4 + k * 0.01), 0.1 * Math.sin(v), -Math.cos(v * 2.4 + k * 0.01), 0, v % 8 === 0 ? 0.5 : 0], v * 6);
              m.port.postMessage({ type: "params", data: p });
              if (k === 0) for (let v = 0; v < voices; v++) m.port.postMessage({ type: "start", slot: v });
            };
            if (k === 0) send();
            else
              ctx.suspend((Math.round((k / 60) * RATE / 128) * 128) / RATE).then(() => {
                send();
                ctx.resume();
              });
          }
        } else if (kind === "reverb") {
          const r = new AudioWorkletNode(ctx, "fdn-reverb", { outputChannelCount: [channels], processorOptions: { channels, params: { room: 0.2, decay: 1.2 } } });
          sources(1).connect(r);
          r.connect(sink);
        } else if (kind === "limiter") {
          const l = new AudioWorkletNode(ctx, "true-peak-limiter", {
            outputChannelCount: [channels],
            ...discrete,
            processorOptions: { ceiling: -1, release: 0.08, dynamics: { threshold: -18, ratio: 2, knee: 8, attack: 0.025, release: 0.2, makeup: -2.3, skip: channels > 2 ? 3 : -1 } },
          });
          sources(channels).connect(l);
          l.connect(sink);
        } else if (kind === "stabilizer") {
          const s = new AudioWorkletNode(ctx, "front-stabilizer", { outputChannelCount: [channels], ...discrete, processorOptions: { channels, left: 0, right: 1, centre: 2 } });
          sources(channels).connect(s);
          s.connect(sink);
        } else if (kind === "meter") {
          sources(channels).connect(new AudioWorkletNode(ctx, "loudness-meter", { numberOfOutputs: 0, ...discrete }));
        } else if (kind === "clarity") {
          const c = new AudioWorkletNode(ctx, "clarity", { outputChannelCount: [channels], ...discrete, processorOptions: { channels, skip: channels > 2 ? 3 : -1 } });
          sources(channels).connect(c);
          c.connect(sink);
        } else if (kind === "sources") {
          // The same sources alone (or nothing), to take out of the figures
          // above.
          for (let v = 0; v < voices; v++) {
            const s = new AudioBufferSourceNode(ctx, { buffer: noise, loop: true });
            s.connect(sink);
            s.start(0, (v * 0.013) % 1);
          }
          if (channels) sources(channels).connect(sink);
        }
        window.job = { ctx };
        return "";
      },
      { ...args, seconds: SECONDS },
    );
  }

  const pct = (ms) => (ms / 10 / SECONDS).toFixed(1).padStart(6);
  console.log(`${linux ? "render thread CPU" : "render wall time"}, % of real time on one core, best of ${REPS}, ${SECONDS} s renders at 48 kHz`);

  const renderings = [
    ["Original, stereo", "original", "stereo", 2, false],
    ["Original, 5.1", "original", "5.1", 6, false],
    ["Original, headphones (HRTF)", "original", "headphones", 2, false],
    ["Enhanced, stereo", "enhanced", "stereo", 2, false],
    ["Enhanced, 5.1", "enhanced", "5.1", 6, false],
    ["Enhanced, 7.1.4", "enhanced", "7.1.4", 12, false],
    ["Enhanced, headphones (decoder)", "enhanced", "headphones", 2, true],
    ["Enhanced, headphones (HRTF)", "enhanced", "headphones", 2, false],
  ].filter(([name, , , , dec]) => wanted(name) && (!dec || decoder));
  if (renderings.length) {
    console.log(`\n${"Rendering (moving voices)".padEnd(34)}    0 voices  32 voices  64 voices`);
    for (const [name, quality, output, channels, dec] of renderings) {
      const cells = [];
      let info = "";
      for (const voices of [0, 32, 64]) {
        const r = await renderer({ quality, output, channels, decoder: dec, voices });
        cells.push(`${pct(r.ms)}%   `);
        info = r.info;
      }
      console.log(`${name.padEnd(34)} ${cells.join("  ")}  (${info})`);
    }
  }

  const worklets = [
    ["Spatial mixer, panpot, 0 voices", { kind: "mixer", law: "panpot", channels: 2, voices: 0 }],
    ["Spatial mixer, panpot, 32 voices", { kind: "mixer", law: "panpot", channels: 2, voices: 32 }],
    ["Spatial mixer, alsoft, 32 voices", { kind: "mixer", law: "alsoft", channels: 2, voices: 32 }],
    ["Spatial mixer, vbap 7.1.4, 32 voices", { kind: "mixer", law: "vbap", channels: 12, voices: 32 }],
    ["Spatial mixer, ambi3, 32 voices", { kind: "mixer", law: "ambi3", channels: 16, voices: 32 }],
    ["Reverb, 2 channels", { kind: "reverb", channels: 2 }],
    ["Reverb, 12 channels", { kind: "reverb", channels: 12 }],
    ["Limiter, 2 channels", { kind: "limiter", channels: 2 }],
    ["Limiter, 12 channels", { kind: "limiter", channels: 12 }],
    ["Stabilizer, 6 channels", { kind: "stabilizer", channels: 6 }],
    ["Meter, 2 channels", { kind: "meter", channels: 2 }],
    ["Meter, 12 channels", { kind: "meter", channels: 12 }],
    ["Clarity, 2 channels", { kind: "clarity", channels: 2 }],
    ["Clarity, 12 channels", { kind: "clarity", channels: 12 }],
  ].filter(([name]) => wanted(name));
  if (worklets.length) {
    console.log(`\n${"Worklet alone (less its test sources)".padEnd(38)}  cost`);
    const sources = new Map();
    const sourceCost = async (channels, voices) => {
      const key = `${channels}|${voices}`;
      if (!sources.has(key)) sources.set(key, (await worklet({ kind: "sources", channels, voices })).ms);
      return sources.get(key);
    };
    for (const [name, args] of worklets) {
      const r = await worklet(args);
      const less = args.kind === "mixer" ? await sourceCost(0, args.voices) : await sourceCost(args.channels, 0);
      console.log(`${name.padEnd(38)} ${pct(Math.max(r.ms - less, 0))}%`);
    }
  }

  if (wanted("Engine")) {
    // The engine's update() on the main thread, during an offline render (so
    // param automation is consumed as in play), 48 voices, 12 heard.
    const engine = async (quality) =>
      page.evaluate(
        async ({ quality, seconds }) => {
          const { AudioEngine } = await import("/src/audio/engine.js");
          const { SoundData } = await import("/src/audio/data.js");
          const RATE = 48000;
          const ctx = new OfflineAudioContext(2, RATE * seconds, RATE);
          const pcm = new Int16Array(22050);
          for (let i = 0; i < pcm.length; i++) pcm[i] = Math.round(8000 * Math.sin(i / 7));
          const voices = 48;
          const heard = 12;
          const manifest = {
            objects: [
              { type: "Bus", Name: "0x00000001", Gain: 1 },
              { type: "Bus", Name: "0x00000002", Gain: 0.8, Parent: "0x00000001" },
              ...Array.from({ length: voices }, (_, i) => ({
                type: "SoundProperties",
                Name: `0x0000b${String(i).padStart(3, "0")}`,
                Bus: "0x00000002",
                Mode3D: 1,
                MinDistance: 2,
                MaxDistance: i < heard ? 400 : 30,
                MuteDistance: i < heard ? 400 : 30,
                RollOff: 1,
                Looping: 1,
                Bias: 0.0001,
                ReverbGain: 0.5,
                SampleList: [["0x00000100", 1]],
              })),
            ],
            samples: { "0x00000100": [0, pcm.length, 22050] },
          };
          const engine = new AudioEngine({ context: ctx });
          await engine.renderer.load(null);
          engine.renderer.configure({ quality, output: "stereo", dynamics: "normal", mono: false });
          engine.setData(new SoundData(manifest, pcm.buffer));
          engine.shape = (voice) => {
            const e = voice.extra;
            const d = voice.mode3D ? voice.distance : 0;
            e.cutoff = d > 10 ? Math.min(Math.max(20000 / (1 + (d - 10) / 35), 1800), 20000) : 20000;
            e.gain = 1;
            e.send = quality === "enhanced" ? 1 : 0;
            e.lfe = 0;
            return e;
          };
          for (let i = 0; i < voices; i++) {
            const a = (i / voices) * Math.PI * 2;
            const r = i < heard ? 40 : 60;
            engine.play(`0x0000b${String(i).padStart(3, "0")}`, { position: { x: r * Math.sin(a), y: 0, z: r * Math.cos(a) }, loop: true, category: "ambience", offline: true });
          }
          let spent = 0;
          let frames = 0;
          const step = (k) => {
            const t = k / 60;
            engine.setListener({ x: 3 * Math.sin(t), y: 1.6, z: Math.sin(t * 0.3) }, { x: -Math.sin(t * 0.8), y: 0, z: -Math.cos(t * 0.8) }, { x: 0, y: 1, z: 0 });
            const t0 = performance.now();
            engine.update(1 / 60);
            if (t > 1) {
              spent += performance.now() - t0;
              frames++;
            }
          };
          step(0);
          for (let k = 1; k / 60 < seconds; k++) {
            ctx.suspend((Math.round((k / 60) * RATE / 128) * 128) / RATE).then(() => {
              step(k);
              ctx.resume();
            });
          }
          await ctx.startRendering();
          return (spent * 1000) / frames;
        },
        { quality, seconds: SECONDS },
      );
    console.log(`\n${"Engine, main thread".padEnd(38)}  us per update()`);
    for (const quality of ["original", "enhanced"]) {
      let best = Infinity;
      for (let rep = 0; rep < REPS; rep++) best = Math.min(best, await engine(quality));
      console.log(`${`${quality}, 48 voices (12 heard)`.padEnd(38)} ${best.toFixed(0).padStart(6)}`);
    }
  }
  if (errors.length) throw new Error(errors.join("\n"));
} finally {
  await context.close();
  fs.rmSync(dir, { recursive: true, force: true });
}
