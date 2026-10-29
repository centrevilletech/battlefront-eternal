// The battle's loudness per dynamics profile (BS.1770-4 / EBU R128), for
// setting each profile's output level (DYNAMICS in src/audio/spatial.js).
//
// It plays Theed in Chromium for a while with a scripted player (walking,
// turning, firing in bursts, among the AI's battle) and records what enters
// the master chain's dynamics, once in the enhanced mode and once in the
// original, the same battle both times (a seeded Math.random). Each
// recording then goes, offline, through the master chain's limiter worklet
// with each profile, and the output is measured: integrated
// loudness (gated), loudness range (EBU Tech 3342, from 3 s short-term
// readings), and true peak. Targets: Home theater -24 LUFS (ASWG-R001's
// home figure), Normal -20, Night -18, each within 2 LU; true peak at or
// under -1 dBTP; the original and enhanced modes within 1.5 LU of each other
// on the same profile (a fair A/B). The battle runs in the world worker,
// stepped by the page's frames (tests/battle-page.mjs) at 60 a second in
// real time; the page plays what the world's ticks send it.
//
// Needs the extracted map (tools/extract.py nab2 ...) and a dev server:
//   GAME_URL=http://127.0.0.1:18449 LOUDNESS_SECONDS=60 \
//   PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node tests/audio-loudness.mjs
import { launchBrowser, openBattle } from "./battle-page.mjs";
const base = process.env.GAME_URL || "http://127.0.0.1:18449";
const seconds = Number(process.env.LOUDNESS_SECONDS || 60);
const TARGETS = { home: -24, normal: -20, night: -18 };

const SEED = 20261008;

const browser = await launchBrowser({ args: ["--ignore-gpu-blocklist", "--autoplay-policy=no-user-gesture-required"] });
let failed = false;

// One mode's battle: a fresh page whose Math.random is seeded, so both
// modes play the same battle (the world's own streams are seeded; the
// sound's dice by this; stream timing still follows the clock, so they can
// drift apart a little), run at a fixed 60 steps a second, its pre-dynamics
// signal recorded and measured offline through each profile.
async function battle(quality) {
  const errors = [];
  // unit= keeps the spawn screen away: the old default unit, placed at load.
  const run = await openBattle(browser, {
    base, query: "map=nab2&unit=rep_inf_jet_trooper", settings: { audioQuality: quality, audioOutput: "stereo", audioDynamics: "home" },
    errors: () => false,
    beforeLoad: (page) => {
      page.on("pageerror", (e) => errors.push(e.message));
      page.setDefaultNavigationTimeout(120000);
      return page.addInitScript((seed) => {
        let s = seed >>> 0;
        Math.random = () => {
          s = (s + 0x6d2b79f5) >>> 0;
          let t = s;
          t = Math.imul(t ^ (t >>> 15), t | 1);
          t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
          return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
        };
      }, SEED);
    },
  });
  const { page } = run;
  await page.evaluate(async () => {
    const a = window.__bf.audio;
    for (let i = 0; i < 600 && !(a.ready && a.started); i++) await new Promise((r) => setTimeout(r, 100));
    if (!(a.ready && a.started)) throw new Error("the battle's sound didn't load (run tools/extract.py)");
  });
  const result = await page.evaluate(
    async ({ seconds, quality }) => {
      const b = window.__bf;
      const engine = b.audio.engine;
      const ctx = engine.ctx;
      await ctx.resume();
      const { DYNAMICS } = await import("/src/audio/spatial.js");
      const r = engine.renderer;
      if (r.enhanced !== (quality === "enhanced") || r.mode !== "stereo") throw new Error(`renderer is ${r.enhanced ? "enhanced" : "original"} ${r.mode}`);

      // A tap on what enters the dynamics.
      const code = `class Tap extends AudioWorkletProcessor {
        constructor() { super(); this.on = false; this.buf = []; this.n = 0; this.port.onmessage = (e) => { this.on = e.data; }; }
        process(inputs) {
          const input = inputs[0];
          if (this.on && input.length) {
            this.buf.push(input.map((c) => c.slice()));
            if (++this.n >= 32) { this.port.postMessage(this.buf); this.buf = []; this.n = 0; }
          }
          return true;
        }
      }
      registerProcessor("loudness-tap", Tap);`;
      await ctx.audioWorklet.addModule(URL.createObjectURL(new Blob([code], { type: "text/javascript" })));
      const tap = new AudioWorkletNode(ctx, "loudness-tap", { numberOfInputs: 1, numberOfOutputs: 0, channelCount: 2, channelCountMode: "explicit", channelInterpretation: "discrete" });
      r.preDynamics.connect(tap);
      const chunks = [];
      tap.port.onmessage = (e) => chunks.push(...e.data);
      tap.port.postMessage(true);

      // The scripted player: walking, turning, firing in bursts; 60 steps a
      // second, paced by the clock. The turn goes to the world as mouse
      // counts, at what a count turns him (lookRaw, on a copy of his body);
      // after a death the world places him again (unit=).
      const probe = Object.create(b.soldier);
      probe.aimYaw = probe.aimPitch = 0;
      probe.lookRaw(1, 0, b.settings.mouseScale, false);
      const yawPerCount = -probe.aimYaw;
      const steps = Math.round(seconds * 60);
      const start = performance.now();
      let behind = 0;
      for (let i = 0; i < steps; i++) {
        const t = i / 60;
        const due = start + t * 1000;
        const wait = due - performance.now();
        if (wait > 0) await new Promise((res) => setTimeout(res, wait));
        else behind++;
        b.keys.clear();
        if (t % 6 < 4) b.keys.add("KeyW");
        b.controls.looks.push([-((0.4 / 60) * Math.sin(t * 0.3)) / yawPerCount, 0]);
        if (t % 5 > 3.5) {
          b.keys.add("Mouse0");
          if (t % 5 < 3.5 + 1 / 60) b.pending.firePrimary = true;
        }
        await b.lockstep(1 / 60, false);
      }
      b.keys.clear();
      tap.port.postMessage(false);
      await new Promise((res) => setTimeout(res, 300));
      r.preDynamics.disconnect(tap);
      const length = chunks.length * 128;
      const pcm = [new Float32Array(length), new Float32Array(length)];
      chunks.forEach((block, i) => block.forEach((c, ch) => ch < 2 && pcm[ch].set(c, i * 128)));

      // Through the master chain's dynamics with each profile, measured.
      const measure = async (profile) => {
        const rate = ctx.sampleRate;
        const off = new OfflineAudioContext(2, length, rate);
        await off.audioWorklet.addModule("/src/audio/worklets/limiter.js");
        await off.audioWorklet.addModule("/src/audio/worklets/meter.js");
        const buffer = off.createBuffer(2, length, rate);
        buffer.copyToChannel(pcm[0], 0);
        buffer.copyToChannel(pcm[1], 1);
        const src = new AudioBufferSourceNode(off, { buffer });
        const limiter = new AudioWorkletNode(off, "true-peak-limiter", {
          numberOfInputs: 1,
          numberOfOutputs: 1,
          outputChannelCount: [2],
          channelCount: 2,
          channelCountMode: "explicit",
          channelInterpretation: "discrete",
          processorOptions: { ceiling: -1, release: quality === "enhanced" ? 0.08 : 0.06, dynamics: { ...DYNAMICS[profile], skip: -1 } },
        });
        const meter = new AudioWorkletNode(off, "loudness-meter", { numberOfInputs: 1, numberOfOutputs: 0, channelCount: 2, channelCountMode: "explicit", channelInterpretation: "discrete", processorOptions: { weights: [1, 1] } });
        const readings = [];
        meter.port.onmessage = (e) => readings.push(e.data);
        src.connect(limiter);
        limiter.connect(meter);
        src.start();
        await off.startRendering();
        await new Promise((res) => setTimeout(res, 300));
        const last = readings[readings.length - 1] || {};
        // Loudness range (EBU Tech 3342): short-term values from 3 s on,
        // gated at -70 LUFS and 20 LU under their own power mean, P95 - P10.
        const st = readings.slice(30).map((m) => m.shortTerm).filter((x) => Number.isFinite(x) && x > -70);
        let lra = NaN;
        if (st.length > 10) {
          const mean = 10 * Math.log10(st.reduce((sum, x) => sum + 10 ** (x / 10), 0) / st.length);
          const kept = st.filter((x) => x > mean - 20).sort((x, y) => x - y);
          const at = (p) => kept[Math.min(kept.length - 1, Math.max(0, Math.round(p * (kept.length - 1))))];
          lra = at(0.95) - at(0.1);
        }
        return { profile, makeup: DYNAMICS[profile].makeup, integrated: last.integrated, lra, truePeak: Math.max(...readings.map((m) => m.truePeak)) };
      };
      const rows = [];
      for (const profile of Object.keys(DYNAMICS)) rows.push(await measure(profile));
      return { rate: ctx.sampleRate, behind, steps, rows };
    },
    { seconds, quality },
  );
  await run.close();
  if (errors.length) throw new Error(errors.join("\n"));
  return result;
}

try {
  const runs = {};
  for (const quality of ["enhanced", "original"]) runs[quality] = await battle(quality);
  const f = (x) => (Number.isFinite(x) ? x.toFixed(1).padStart(6) : "   n/a");
  console.log(`${seconds} s of Theed per mode (seed ${SEED}) at ${runs.enhanced.rate} Hz; steps late: ${runs.enhanced.behind}/${runs.enhanced.steps}, ${runs.original.behind}/${runs.original.steps}`);
  console.log("quality   profile  makeup  integrated  range  true peak  target");
  for (const [quality, run] of Object.entries(runs)) {
    for (const r of run.rows) {
      const target = TARGETS[r.profile];
      const ok = Math.abs(r.integrated - target) <= 2 && r.truePeak <= -0.9;
      if (!ok) failed = true;
      console.log(`${quality.padEnd(9)} ${r.profile.padEnd(7)} ${f(r.makeup)} ${f(r.integrated)} LUFS ${f(r.lra)} LU ${f(r.truePeak)} dBTP ${String(target).padStart(4)} ${ok ? "ok" : "OFF"}`);
    }
  }
  for (const profile of Object.keys(TARGETS)) {
    const e = runs.enhanced.rows.find((r) => r.profile === profile);
    const o = runs.original.rows.find((r) => r.profile === profile);
    const gap = e.integrated - o.integrated;
    const ok = Math.abs(gap) <= 1.5;
    if (!ok) failed = true;
    console.log(`${profile}: enhanced minus original ${gap.toFixed(1)} LU ${ok ? "ok" : "OFF"}`);
  }
} finally {
  await browser.close();
}
if (failed) process.exitCode = 1;
