// What a player feels: how long after an input the drawn frame shows it, and
// how evenly frames come, in the live loop of a real battle, on this
// machine's GPU (headless Chromium; ANGLE=swiftshader without one).
//
// Input goes through the browser's own input pipeline (CDP), so a busy main
// thread delays its dispatch as it would a player's. Each latency runs from
// the event's timestamp (when the browser made it) to the end of the first
// drawn frame that shows it (after hud.render(): render submission, not the
// photon):
//   look   mouse, 40 counts        the drawn camera turned
//   move   W or S                  the drawn body moved 1 mm
//   jump   Space                   the drawn body rose 1 mm
//   fire   left button             a new round of the player's is drawn (or
//                                  his own shot from the trigger, PredictedShots)
//   drive  W or S in a hover tank  the drawn hull moved 1 mm (KINDS=drive;
//          boards him in the world worker, self.__bf there)
// Before the trials, PACE_S seconds without input measure frame pacing and
// the runtime profiler's frame cost (?profileRuntime=1).
//
// With Vite running and a map imported:
//   node tools/input-latency.mjs
// Env: LATENCY_URL (http://127.0.0.1:18449), AI (classic, off), MAP (nab2),
// TRIALS (20), WARM_S (20), PACE_S (15), KINDS (look,move,jump,fire), ANGLE
// (vulkan, gl, swiftshader), CHROMIUM (a browser binary), W and H (1280x720),
// OUT (a JSON report), PLAYWRIGHT_MODULE.
import fs from 'node:fs';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const base = process.env.LATENCY_URL || 'http://127.0.0.1:18449';
const ai = process.env.AI || 'classic', map = process.env.MAP || 'nab2';
const trials = Number(process.env.TRIALS || 20), warm = Number(process.env.WARM_S || 20), pace = Number(process.env.PACE_S || 15);
const kinds = (process.env.KINDS || 'look,move,jump,fire').split(',');
const angle = process.env.ANGLE || 'vulkan', W = Number(process.env.W || 1280), H = Number(process.env.H || 720);
const gpu = angle === 'swiftshader' ? ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] : [`--use-angle=${angle}`, '--enable-features=Vulkan', '--ignore-gpu-blocklist', '--enable-gpu'];
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined,
  args: [...gpu, '--autoplay-policy=no-user-gesture-required', '--disable-renderer-backgrounding', '--disable-background-timer-throttling'] });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const pct = (v, q) => { const s = [...v].sort((a, b) => a - b); return s.length ? s[Math.min(s.length - 1, Math.max(0, Math.ceil(q * s.length) - 1))] : null; };
const round = (x) => (x === null || x === undefined ? null : +x.toFixed(2));
const stat = (v) => ({ n: v.length, p50: round(pct(v, .5)), p95: round(pct(v, .95)), p99: round(pct(v, .99)), max: round(v.length ? Math.max(...v) : null) });
const result = { ai, map, angle, viewport: [W, H] };
try {
  const page = await browser.newPage({ viewport: { width: W, height: H } });
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  await page.route('**/@vite/client', (r) => r.fulfill({ contentType: 'application/javascript', body: '' }));
  await page.goto(`${base}/?map=${map}&unit=rep_inf_clone_trooper&ai=${ai}&seed=1&loading=0&profileRuntime=1`);
  await page.waitForFunction(() => window.__bf || document.querySelector('#hud')?.textContent.includes('Failed'), null, { timeout: 300000 });
  await page.evaluate(async () => { await __bf.ready; await Promise.race([__bf.loaded(), new Promise((r) => setTimeout(r, 90000))]); });
  await page.waitForFunction(() => __bf.client?.ready, null, { timeout: 120000 });
  result.renderer = await page.evaluate(() => { const gl = __bf.renderer.getContext(), e = gl.getExtension('WEBGL_debug_renderer_info'); return e ? gl.getParameter(e.UNMASKED_RENDERER_WEBGL) : ''; });
  // After each drawn frame: what it showed.
  await page.evaluate(() => {
    const b = __bf, direction = new b.THREE.Vector3(), seen = new WeakSet(), s = (window.__latency = { frames: [], events: [], raf: [] });
    b.controls.locked = true; b.controls.enabled = true;
    for (const bolt of b.ordnance.bolts) seen.add(bolt);
    const render = b.hud.render.bind(b.hud);
    b.hud.render = (...args) => {
      const out = render(...args);
      b.camera.getWorldDirection(direction);
      let rounds = 0;
      for (const list of [b.ordnance.bolts, b.ordnance.rounds]) for (const r of list) if (!seen.has(r)) { seen.add(r); if (r.owner === b.playerUnit || r.owner === 'player') rounds++; }
      // A shot the page shows from the trigger (src/runtime/predicted-shots.js).
      for (const shot of b.predictedShots?.shots.values() ?? []) if (!seen.has(shot)) { seen.add(shot); rounds++; }
      const p = b.actor.root.position, hull = b.driver?.vehicle?.root.position;
      s.frames.push({ t: performance.now(), yaw: Math.atan2(direction.x, direction.z), x: p.x, y: p.y, z: p.z, rounds, hx: hull?.x, hz: hull?.z, dead: b.combat.dead });
      if (s.frames.length > 20000) s.frames.splice(0, 10000);
      return out;
    };
    for (const type of ['keydown', 'mousedown']) window.addEventListener(type, (e) => s.events.push({ type, ts: e.timeStamp, at: performance.now() }), true);
    document.addEventListener('mousemove', (e) => s.events.push({ type: 'mousemove', ts: e.timeStamp, at: performance.now() }), true);
    const tick = (now) => { s.raf.push(now); if (s.raf.length > 40000) s.raf.splice(0, 20000); requestAnimationFrame(tick); };
    requestAnimationFrame(tick);
  });
  await sleep(warm * 1000);
  const paced = await page.evaluate(async (seconds) => {
    const s = window.__latency, b = __bf;
    b.runtimeProfile?.reset();
    const raf0 = s.raf.length, frames0 = s.frames.length, start = performance.now();
    await new Promise((r) => setTimeout(r, seconds * 1000));
    const raf = s.raf.slice(raf0), drawn = s.frames.slice(frames0).map((f) => f.t);
    return { seconds: (performance.now() - start) / 1000, raf: raf.slice(1).map((t, i) => t - raf[i]), drawn: drawn.slice(1).map((t, i) => t - drawn[i]),
      profile: b.runtimeProfile?.report().metrics, alive: b.units.all().filter((u) => u.alive && u.soldier).length, stats: document.querySelector('#stats')?.textContent };
  }, pace);
  result.pacing = { alive: paced.alive, stats: paced.stats, fps: +(paced.raf.length / paced.seconds).toFixed(1),
    over20ms: paced.raf.filter((x) => x > 20).length, frames: paced.raf.length, drawnIntervalMs: stat(paced.drawn),
    frameCpuMs: paced.profile?.cpuMs && { p50: round(paced.profile.cpuMs.p50), p95: round(paced.profile.cpuMs.p95), p99: round(paced.profile.cpuMs.p99) },
    simulationMs: paced.profile?.simulation && { p50: round(paced.profile.simulation.p50), p95: round(paced.profile.simulation.p95) } };
  // A trial: the input, then the first drawn frame after it that shows it.
  const trial = async (kind, n) => {
    const mark = await page.evaluate(() => ({ events: __latency.events.length, frames: __latency.frames.length }));
    const key = n % 2 ? 'KeyS' : 'KeyW';
    if (kind === 'look') await page.mouse.move(W / 2 + (n % 2 ? 0 : 40), H / 2);
    else if (kind === 'move' || kind === 'drive') { await page.keyboard.down(key); await sleep(kind === 'drive' ? 500 : 300); await page.keyboard.up(key); }
    else if (kind === 'jump') await page.keyboard.press('Space');
    else { await page.mouse.down({ button: 'left' }); await sleep(60); await page.mouse.up({ button: 'left' }); }
    // A jump must land before the next trial: a soldier in the air or
    // getting up cannot fire.
    await sleep(kind === 'fire' || kind === 'jump' ? 900 : 300);
    return page.evaluate(({ mark, kind }) => {
      const s = __latency, type = kind === 'look' ? 'mousemove' : kind === 'fire' ? 'mousedown' : 'keydown';
      const event = s.events.slice(mark.events).find((e) => e.type === type);
      if (!event) return { error: 'no event' };
      const before = [...s.frames.slice(0, mark.frames)].reverse().find((f) => f.t <= event.at), after = s.frames.slice(mark.frames);
      if (!before) return { error: 'no frame' };
      if (before.dead || after.some((f) => f.dead)) return { error: 'dead' };
      for (const f of after) {
        if (f.t < event.at) continue;
        const turned = Math.abs(Math.atan2(Math.sin(f.yaw - before.yaw), Math.cos(f.yaw - before.yaw)));
        if ((kind === 'look' && turned > 1e-3) || (kind === 'move' && Math.hypot(f.x - before.x, f.z - before.z) > 1e-3) || (kind === 'jump' && f.y - before.y > 1e-3)
          || (kind === 'fire' && f.rounds > 0) || (kind === 'drive' && Math.hypot(f.hx - before.hx, f.hz - before.hz) > 1e-3)) return { ms: f.t - event.ts, dispatch: event.at - event.ts };
      }
      return { error: 'no effect' };
    }, { mark, kind });
  };
  const alive = async () => { for (let i = 0; i < 200; i++) { if (await page.evaluate(() => !__bf.combat.dead && !__bf.playerUnit.removed)) return true; await sleep(100); } return false; };
  const latency = {};
  await page.mouse.move(W / 2, H / 2);
  for (const kind of kinds.filter((k) => k !== 'drive')) latency[kind] = { samples: [], dispatch: [], misses: 0, skipped: 0 };
  for (let n = 0; n < trials; n++) {
    for (const kind of Object.keys(latency)) {
      if (!await alive()) { latency[kind].skipped++; continue; }
      await sleep(400 + Math.random() * 40);
      const r = await trial(kind, n);
      if (r.ms !== undefined) { latency[kind].samples.push(r.ms); latency[kind].dispatch.push(r.dispatch); }
      else if (r.error === 'dead') latency[kind].skipped++; else latency[kind].misses++;
    }
    await sleep(600);
  }
  if (kinds.includes('drive')) {
    // Board the first live hover tank, in the world worker (self.__bf).
    const world = page.workers().find((w) => w.url().includes('local-server-worker'));
    const board = () => {
      const game = self.__bf, v = game.vehicles?.vehicles.find((v) => /hover_fightertank|hover_aat/.test(v.name) && v.alive);
      if (!v) return null;
      game.combat.reset?.(); game.soldier.position.copy(v.position);
      return { ok: game.driver.enter(v), name: v.name };
    };
    let boarded = null;
    for (let i = 0; i < 60 && !boarded?.ok; i++) {
      boarded = await world.evaluate(board);
      if (!boarded?.ok) await sleep(1000);
    }
    latency.drive = { samples: [], dispatch: [], misses: 0, skipped: 0, hull: boarded?.name ?? null };
    if (boarded?.ok) {
      await page.waitForFunction(() => __bf.driver?.active, null, { timeout: 20000 });
      // Each trial starts from a hull at rest as drawn: a drift left from
      // the last one would read as an answer to the next key.
      const rest = () => page.waitForFunction(() => {
        const f = __latency.frames.slice(-12);
        return f.length === 12 && f.every((x) => Math.hypot(x.hx - f[0].hx, x.hz - f[0].hz) < 2e-4);
      }, null, { timeout: 8000, polling: 50 }).then(() => true, () => false);
      for (let n = 0; n < trials; n++) {
        await sleep(600 + Math.random() * 40);
        if (!await rest()) { latency.drive.skipped++; continue; }
        const r = await trial('drive', n);
        if (r.ms !== undefined) { latency.drive.samples.push(r.ms); latency.drive.dispatch.push(r.dispatch); } else latency.drive.misses++;
      }
    }
  }
  result.latency = Object.fromEntries(Object.entries(latency).map(([kind, v]) => [kind, { ...stat(v.samples), dispatchP50: round(pct(v.dispatch, .5)), misses: v.misses, skipped: v.skipped, ...(v.hull !== undefined ? { hull: v.hull } : {}) }]));
  result.errors = errors.slice(0, 10);
} finally { await browser.close(); }
console.log(JSON.stringify(result, null, 1));
if (process.env.OUT) fs.writeFileSync(process.env.OUT, JSON.stringify(result, null, 1));
