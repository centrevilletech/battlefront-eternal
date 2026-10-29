// GPU-side render benchmark over the Chrome DevTools Protocol (no Playwright
// needed): headless Chromium on SwiftShader, so frame completion time stands
// in for fill and shading cost, and draw, triangle and GL call counts are
// exact. A fixed viewport and settings, a 20 s warm-up of ticks without
// drawing, a capture of the frame at rest (eight jittered frames at dt 0,
// then readPixels of the canvas), N driven frames of walking, strafing,
// turning and firing (one tick each) timed for CPU submission and completion,
// a capture of the end frame, and per-pass draw counts. Compare two trees by
// serving each (BASE) and diffing the .rgba captures (tools/pixel-diff.mjs).
// The ticks run in the world worker in lockstep with the frames
// (game.lockstep), so a frame's CPU time includes waiting for its tick.
//
//   node tools/render-bench-cdp.mjs            # serves the working tree itself
//   BASE=http://127.0.0.1:18475 LABEL=before node tools/render-bench-cdp.mjs
//   FRAMES=600 WARM=20 W=640 H=400 SETTINGS='{"shadows":"low"}' OUT=/tmp/bf-bench
//   PRE='__bf.graphics.cacheShadows = false' (evaluated in the page after warm-up)
//   Q='map=nab2&unit=rep_inf_clone_trooper&ai=classic&tier=medium&heroes=0&seed=1&noreload=1&profileRuntime=1'
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import zlib from 'node:zlib';
import { createServer } from 'vite';
const port = Number(process.env.CDP_PORT || 9444);
const servePort = Number(process.env.PORT || 18474);
const server = process.env.BASE ? null : await createServer({
  root: process.cwd(), configFile: false, cacheDir: `/tmp/bf-bench-vite-${servePort}`, logLevel: 'error',
  optimizeDeps: { entries: ['index.html'] }, server: { watch: null, hmr: false, port: servePort, host: '127.0.0.1', strictPort: true },
});
if (server) await server.listen();
const base = process.env.BASE || `http://127.0.0.1:${servePort}`;
const query = process.env.Q || 'map=nab2&unit=rep_inf_clone_trooper&ai=classic&tier=medium&heroes=0&seed=1&noreload=1&profileRuntime=1';
const label = process.env.LABEL || 'run', out = process.env.OUT || '/tmp/bf-bench';
const FRAMES = Number(process.env.FRAMES || 600), WARM = Number(process.env.WARM || 20);
const W = Number(process.env.W || 640), H = Number(process.env.H || 400);
const settings = JSON.parse(process.env.SETTINGS || '{}');
fs.mkdirSync(out, { recursive: true });
const chrome = spawn('/usr/bin/chromium', ['--headless=new', `--remote-debugging-port=${port}`, '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--disable-renderer-backgrounding', '--disable-background-timer-throttling', '--no-sandbox', `--user-data-dir=${out}/chrome-${port}`, `--window-size=${W},${H}`, '--autoplay-policy=no-user-gesture-required', '--mute-audio', 'about:blank'], { stdio: 'ignore' });
const sleep = ms => new Promise(r => setTimeout(r, ms));
let info; for (let i = 0; i < 50; i++) { try { info = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json(); break; } catch { await sleep(200); } }
const ws = new WebSocket(info.webSocketDebuggerUrl); await new Promise(r => ws.onopen = r);
let id = 0; const waits = new Map(); const errors = [];
ws.onmessage = ({ data }) => { const m = JSON.parse(data); if (m.id && waits.has(m.id)) { waits.get(m.id)(m); waits.delete(m.id); return; }
  if (m.method === 'Runtime.exceptionThrown') errors.push('EXC ' + (m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text).slice(0, 300));
  if (m.method === 'Runtime.consoleAPICalled' && (m.params.type === 'error' || m.params.type === 'warning')) errors.push(m.params.args.map(a => a.value ?? a.description).join(' ').slice(0, 300)); };
const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => { const i = ++id; waits.set(i, m => m.error ? reject(new Error(method + ': ' + JSON.stringify(m.error))) : resolve(m.result)); ws.send(JSON.stringify({ id: i, method, params, sessionId })); });
const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
const { sessionId: page } = await send('Target.attachToTarget', { targetId, flatten: true });
await send('Runtime.enable', {}, page);
await send('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false }, page);
await send('Page.addScriptToEvaluateOnNewDocument', { source: `
  localStorage.setItem('battlefront-eternal.settings', ${JSON.stringify(JSON.stringify(settings))});
  // Seeded Math.random: flower beds and other build-time randomness repeat run to run.
  let seed = 0x9e3779b9; Math.random = () => { seed = (seed + 0x6d2b79f5) | 0; let t = seed; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
  const raf = requestAnimationFrame.bind(window);
  window.requestAnimationFrame = f => f.name === 'loop' ? 0 : raf(f);
` }, page);
const GLWRAP = "  window.__gl = { counts: Object.create(null), on: false };\n  for (const name of Object.getOwnPropertyNames(WebGL2RenderingContext.prototype)) {\n    const d = Object.getOwnPropertyDescriptor(WebGL2RenderingContext.prototype, name);\n    if (!d || typeof d.value !== `function` || name === `constructor`) continue;\n    const original = d.value;\n    WebGL2RenderingContext.prototype[name] = function (...args) { if (window.__gl.on) window.__gl.counts[name] = (window.__gl.counts[name] || 0) + 1; return original.apply(this, args); };\n  }\n";
const ev = async (expr) => { const r = await send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true, timeout: 1800000 }, page); if (r.exceptionDetails) throw new Error('page: ' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text)); return r.result.value; };
await send('Page.navigate', { url: `${base}/?${query}` }, page);
for (let i = 0; i < 600; i++) { if (await ev('!!window.__bf?.frame')) break; await sleep(500); }
await ev(`(async () => { if (!window.__gl) { ${GLWRAP} } const b = __bf; await Promise.race([b.ready, new Promise(r => setTimeout(r, 120000))]); await b.loaded?.(); b.begin?.(); b.pause(); b.controls.locked = true; b.controls.enabled = true;
  for (let i = 0; i < ${WARM} * 60; i++) await b.lockstep(1 / 60, false);
  // Per-render-call breakdown: draws/triangles per (scene, target) pair.
  const r = b.renderer, g = b.graphics; window.__passes = Object.create(null);
  const name = (t) => t === null ? 'canvas' : t === g.farTarget ? 'far' : t === g.sceneTarget ? 'scene' : t === g.postTarget ? 'post' : t === g.aaTarget ? 'aa' : t === g.quarterTarget ? 'quarter' : (t.texture?.name || t.name || ('rt' + t.width + 'x' + t.height));
  const render = r.render.bind(r);
  r.render = (scene, camera) => { const c0 = r.info.render.calls, t0 = r.info.render.triangles; const t = performance.now(); render(scene, camera); const k = (scene === g.scene ? 'near' : scene === g.farScene ? 'far' : scene === g.viewScene ? 'view' : scene.children[0]?.material?.name || scene.children[0]?.name || 'pass') + '->' + name(r.getRenderTarget()); const p = window.__passes[k] ||= { n: 0, calls: 0, tris: 0, ms: 0 }; p.n++; p.calls += r.info.render.calls - c0; p.tris += r.info.render.triangles - t0; p.ms += performance.now() - t; };
  return 'ok'; })()`);
if (process.env.PRE) console.log(label, 'PRE ->', JSON.stringify(await ev(process.env.PRE)));
console.log(label, 'game up, warmed', WARM, 's; diag', JSON.stringify(await ev('(() => { const d = __bf.graphics.diagnostics(); return { resolution: d.resolution, output: d.output, upscaler: d.upscaler, samples: d.samples, cascades: d.cascades, shadowSize: d.shadowSize, bloom: d.bloom, softFocus: d.softFocus, water: __bf.settings.get("waterQuality") }; })()')));
// At rest: no simulation time, eight jittered frames for the temporal history, then the canvas pixels.
const restCapture = async (tag) => {
  const rgba = await ev(`(async () => { const b = __bf, g = b.graphics, gl = b.renderer.getContext();
    for (let i = 0; i < 8; i++) await b.lockstep(0, true);
    b.renderer.setRenderTarget(null); const w = gl.drawingBufferWidth, h = gl.drawingBufferHeight; const buf = new Uint8Array(w * h * 4); gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, buf);
    let s = ''; for (let i = 0; i < buf.length; i += 0x8000) s += String.fromCharCode.apply(null, buf.subarray(i, i + 0x8000)); return { w, h, data: btoa(s) }; })()`);
  const data = Buffer.from(rgba.data, 'base64');
  fs.writeFileSync(`${out}/${label}-${tag}.rgba`, Buffer.concat([Buffer.from(new Uint32Array([rgba.w, rgba.h]).buffer), data]));
  // PNG (flip rows: GL reads bottom-up).
  const raw = Buffer.alloc((rgba.w * 4 + 1) * rgba.h);
  for (let y = 0; y < rgba.h; y++) { raw[y * (rgba.w * 4 + 1)] = 0; data.copy(raw, y * (rgba.w * 4 + 1) + 1, (rgba.h - 1 - y) * rgba.w * 4, (rgba.h - y) * rgba.w * 4); }
  const crc = (buf) => { let c, crc = 0xffffffff; for (let n = 0; n < buf.length; n++) { c = (crc ^ buf[n]) & 0xff; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; crc = (crc >>> 8) ^ c; } return (crc ^ 0xffffffff) >>> 0; };
  const chunk = (type, body) => { const len = Buffer.alloc(4); len.writeUInt32BE(body.length); const tb = Buffer.concat([Buffer.from(type), body]); const c = Buffer.alloc(4); c.writeUInt32BE(crc(tb)); return Buffer.concat([len, tb, c]); };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(rgba.w, 0); ihdr.writeUInt32BE(rgba.h, 4); ihdr[8] = 8; ihdr[9] = 6;
  fs.writeFileSync(`${out}/${label}-${tag}.png`, Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]));
};
await restCapture('rest');
// Driven frames: walk, strafe, look, fire; one tick each.
const result = await ev(`(async () => { const b = __bf, g = b.graphics, gl = b.renderer.getContext(), rows = [];
  const key = (code, down) => window.dispatchEvent(new KeyboardEvent(down ? 'keydown' : 'keyup', { code, key: code, bubbles: true }));
  const look = (x, y) => document.dispatchEvent(new MouseEvent('mousemove', { movementX: x, movementY: y, bubbles: true }));
  const px = new Uint8Array(4);
  key('KeyW', true); window.__gl.counts = Object.create(null); window.__gl.on = true; window.__passes = Object.create(null);
  const shadowTotals = { calls: 0, triangles: 0 };
  for (let i = 0; i < ${FRAMES}; i++) {
    await new Promise(requestAnimationFrame);
    if (i % 120 === 0) key('KeyA', true); if (i % 120 === 60) key('KeyA', false);
    if (i % 90 === 0) window.dispatchEvent(new MouseEvent('mousedown', { button: 0, bubbles: true }));
    if (i % 90 === 20) window.dispatchEvent(new MouseEvent('mouseup', { button: 0, bubbles: true }));
    look(Math.round(6 * Math.sin(i * 0.05)), Math.round(2 * Math.cos(i * 0.03)));
    const start = performance.now();
    await b.lockstep(1 / 60, true);
    const cpu = performance.now() - start;
    b.renderer.setRenderTarget(null); gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    const completed = performance.now() - start;
    const f = g.frameInfo || {}, s = g.shadowInfo || {};
    shadowTotals.calls += s.calls || 0; shadowTotals.triangles += s.triangles || 0;
    rows.push([cpu, completed, f.calls || 0, f.triangles || 0, b.renderer.info.render.calls, b.renderer.info.render.triangles]);
  }
  window.__gl.on = false; key('KeyW', false);
  const n = ${FRAMES};
  const gl2 = Object.fromEntries(Object.entries(window.__gl.counts).map(([k, v]) => [k, +(v / n).toFixed(1)]).sort((a, b) => b[1] - a[1]));
  const passes = Object.fromEntries(Object.entries(window.__passes).map(([k, p]) => [k, { perFrame: +(p.n / n).toFixed(2), calls: +(p.calls / n).toFixed(1), tris: +(p.tris / n).toFixed(0), ms: +(p.ms / n).toFixed(2) }]).sort((a, b) => b[1].calls - a[1].calls));
  return { rows, gl: gl2, passes, shadow: { calls: +(shadowTotals.calls / n).toFixed(1), triangles: +(shadowTotals.triangles / n).toFixed(0) }, profiler: g.profiler.report(), diag: (() => { const d = g.diagnostics(); delete d.timings; return d; })(), units: b.units.all().filter(u => u.alive).length };
})()`);
await restCapture('end');
const pct = (a, q) => { const s = [...a].sort((x, y) => x - y); return s[Math.min(s.length - 1, Math.floor(q * s.length))]; };
const rows = result.rows.slice(Math.min(30, Math.floor(result.rows.length / 2)));
const col = (i) => rows.map(r => r[i]);
const summary = { label, W, H, frames: rows.length, diag: result.diag, units: result.units,
  cpuMs: { p50: +pct(col(0), .5).toFixed(1), p95: +pct(col(0), .95).toFixed(1) },
  completedMs: { p50: +pct(col(1), .5).toFixed(1), p95: +pct(col(1), .95).toFixed(1), mean: +(col(1).reduce((a, b) => a + b, 0) / rows.length).toFixed(1) },
  worldDraws: { p50: pct(col(2), .5), p95: pct(col(2), .95) }, worldTriangles: { p50: pct(col(3), .5), p95: pct(col(3), .95) },
  totalDraws: { p50: pct(col(4), .5) }, totalTriangles: { p50: pct(col(5), .5) },
  shadow: result.shadow, glCallsPerFrame: +Object.values(result.gl).reduce((a, b) => a + b, 0).toFixed(0), gl: result.gl, passes: result.passes, errors: [...new Set(errors)].slice(0, 10) };
fs.writeFileSync(`${out}/${label}.json`, JSON.stringify({ summary, rows: result.rows }, null, 1));
console.log(JSON.stringify({ ...summary, gl: Object.fromEntries(Object.entries(result.gl).slice(0, 14)) }, null, 1));
chrome.kill('SIGKILL'); await server?.close(); process.exit(0);
