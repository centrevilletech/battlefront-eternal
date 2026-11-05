// Live final-output loudness by quality and dynamics profile. Each seeded
// Theed battle runs through the sole Rust output graph and Rust meter.
// Targets: Home -24 LUFS, Normal -20, Night -18, within 2 LU; true peak
// at or under -1 dBTP (Classic stereo soft clip: full scale), quality gap
// within 1.5 LU. Stream/page timing can vary between repeated battles.
// Needs extracted Theed, a dev server and a real-time browser:
// GAME_URL=http://127.0.0.1:18449 LOUDNESS_SECONDS=60 node tests/audio-loudness.mjs
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
// drift apart a little), run at a fixed 60 steps a second, its final
// output metered in the active Rust graph.
async function battle(quality, profile) {
  const errors = [];
  const run = await openBattle(browser, {
    base, query: "map=nab2&unit=rep_inf_jet_trooper", settings: { audioQuality: quality, audioOutput: "stereo", audioDynamics: profile },
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
    async ({ seconds, quality, profile }) => {
      const b = window.__bf;
      const engine = b.audio.engine;
      const ctx = engine.ctx;
      await ctx.resume();
      const { DYNAMICS, LIGHT_LEVELS } = await import("/src/audio/spatial.js");
      const r = engine.renderer;
      if (r.enhanced !== (quality === "enhanced") || r.mode !== "stereo") throw new Error(`renderer is ${r.enhanced ? "enhanced" : "original"} ${r.mode}`);
      // The original on stereo is the light mix: a level per profile and
      // a soft clip in place of the limiter.
      const light = Boolean(r.light);

      const readings = [];
      r.onMeter = (reading) => readings.push(reading);
      r.meter(true);

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
      await new Promise((res) => setTimeout(res, 300));
      r.meter(false);
      const last = readings.at(-1);
      if (!last) throw new Error("Rust meter produced no readings");
      const st = readings.slice(30).map((m) => m.shortTerm).filter((x) => Number.isFinite(x) && x > -70);
      const gated = st.filter((x) => x > last.integrated - 20).sort((a, b) => a - b);
      const quantile = (q) => gated[Math.floor((gated.length - 1) * q)];
      const row = { profile, makeup: light ? LIGHT_LEVELS[profile] : DYNAMICS[profile].makeup, light,
        integrated: last.integrated, lra: gated.length ? quantile(0.95) - quantile(0.1) : NaN,
        truePeak: Math.max(...readings.map((m) => m.truePeak)) };
      return { rate: ctx.sampleRate, behind, steps, row };
    },
    { seconds, quality, profile },
  );
  await run.close();
  if (errors.length) throw new Error(errors.join("\n"));
  return result;
}

try {
  const runs = {};
  for (const quality of ["enhanced", "original"]) {
    const rows = [];
    for (const profile of Object.keys(TARGETS)) { const run = await battle(quality, profile); rows.push(run.row); runs[quality] = { ...run, rows }; }
  }
  const f = (x) => (Number.isFinite(x) ? x.toFixed(1).padStart(6) : "   n/a");
  console.log(`${seconds} s of Theed per mode (seed ${SEED}) at ${runs.enhanced.rate} Hz; steps late: ${runs.enhanced.behind}/${runs.enhanced.steps}, ${runs.original.behind}/${runs.original.steps}`);
  console.log("quality   profile  makeup  integrated  range  true peak  target");
  for (const [quality, run] of Object.entries(runs)) {
    for (const r of run.rows) {
      const target = TARGETS[r.profile];
      // The light mix clips softly at full scale rather than limiting at
      // -1 dBTP.
      const ok = Math.abs(r.integrated - target) <= 2 && r.truePeak <= (r.light ? 0.5 : -0.9);
      if (!ok) failed = true;
      console.log(`${(r.light ? "light" : quality).padEnd(9)} ${r.profile.padEnd(7)} ${f(r.makeup)} ${f(r.integrated)} LUFS ${f(r.lra)} LU ${f(r.truePeak)} dBTP ${String(target).padStart(4)} ${ok ? "ok" : "OFF"}`);
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
