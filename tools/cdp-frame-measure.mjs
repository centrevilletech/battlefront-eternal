// Per-frame render counters without Playwright: drives the game under
// headless Chromium (SwiftShader) over the Chrome DevTools Protocol and
// records, per frame, renderer.info (calls, triangles), selected WebGL calls
// (bufferSubData, texSubImage2D, draw*, uniform*), Object3D.updateMatrix
// compositions, and the game's runtime-profiler phases (?profileRuntime=1).
//
// det mode (default): the loop paused and the world worker stepped in
// lockstep with the frames (game.lockstep), a deterministic warmup of WARM_S
// simulated seconds, then FRAMES frames of one tick each with walking,
// strafing and firing (a frame's CPU time includes waiting for its tick).
// Math.random is seeded (three's UUIDs on a separate stream) so two trees
// draw the same battle, and the end takes screenshots (the live view and the
// mission's camera shots) as PNG and raw RGBA for tools/pixel-diff.mjs.
// worker mode: the live loop, the world worker on its own clock, sampled per
// animation frame.
//
// Usage: BASE=http://127.0.0.1:18490 OUT=/tmp/m/base node tools/cdp-frame-measure.mjs
//   MODE=det|worker FRAMES=600 WARM_S=25 CDP_PORT=9340 W=640 H=360
//   SETTINGS='{"upscaler":"off","dynamicResolution":"0"}' SHOTS=1
import { spawn } from 'node:child_process';
import fs from 'node:fs';
const base = process.env.BASE, out = process.env.OUT, mode = process.env.MODE || 'det', FRAMES = Number(process.env.FRAMES || 600), WARM = Number(process.env.WARM_S || 25), port = Number(process.env.CDP_PORT || 9340);
const settings = process.env.SETTINGS || '{"upscaler":"off","dynamicResolution":"0"}';
const W = Number(process.env.W || 640), H = Number(process.env.H || 360);
const profile = `${process.env.TMPDIR || '/tmp'}/cdp-frame-measure-${port}`;
if (!base || !out) { console.error('BASE and OUT are required'); process.exit(2); }
fs.mkdirSync(out, { recursive: true });
fs.rmSync(profile, { recursive: true, force: true });
const chrome = spawn(process.env.CHROMIUM || '/usr/bin/chromium', [`--headless=new`, `--remote-debugging-port=${port}`, '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--disable-renderer-backgrounding', '--disable-background-timer-throttling', '--no-sandbox', `--user-data-dir=${profile}`, `--window-size=${W},${H}`, '--enable-precise-memory-info', 'about:blank'], { stdio: 'ignore' });
const sleep = ms => new Promise(r => setTimeout(r, ms));
let info; for (let i = 0; i < 50; i++) { try { info = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json(); break; } catch { await sleep(200); } }
if (!info) { console.error('chromium did not start'); chrome.kill('SIGKILL'); process.exit(1); }
const ws = new WebSocket(info.webSocketDebuggerUrl); await new Promise(r => ws.onopen = r);
let id = 0; const waits = new Map(); const workers = new Map(); const errors = [];
ws.onmessage = ({ data }) => { const m = JSON.parse(data); if (m.id && waits.has(m.id)) { waits.get(m.id)(m); waits.delete(m.id); return; }
  if (m.method === 'Target.attachedToTarget' && m.params.targetInfo.type === 'worker') { workers.set(m.params.sessionId, m.params.targetInfo.url); send('Runtime.enable', {}, m.params.sessionId); send('Runtime.runIfWaitingForDebugger', {}, m.params.sessionId); }
  if (m.method === 'Target.detachedFromTarget') workers.delete(m.params.sessionId);
  if (m.method === 'Runtime.exceptionThrown') errors.push(`[${workers.has(m.sessionId) ? 'worker' : 'page'}] EXC ${(m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text).slice(0, 400)}`);
  if (m.method === 'Runtime.consoleAPICalled' && (m.params.type === 'error' || m.params.type === 'warning')) errors.push(`[${workers.has(m.sessionId) ? 'worker' : 'page'}] ${m.params.args.map(a => a.value ?? a.description).join(' ').slice(0, 300)}`); };
const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => { const i = ++id; waits.set(i, m => m.error ? reject(new Error(method + ': ' + JSON.stringify(m.error))) : resolve(m.result)); ws.send(JSON.stringify({ id: i, method, params, sessionId })); });
const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
const { sessionId: page } = await send('Target.attachToTarget', { targetId, flatten: true });
await send('Runtime.enable', {}, page);
await send('Page.enable', {}, page);
await send('Page.addScriptToEvaluateOnNewDocument', { source: `
  localStorage.setItem('battlefront-eternal.settings', ${JSON.stringify(settings)});
  // Seeded randomness: the game's on one stream, three's UUIDs on another, so
  // object counts never shift the battle.
  { const mk = (seed) => { let st = seed >>> 0; return () => { st = (Math.imul(st ^ (st >>> 15), 0x2c1b3c6d) + 0x1b873593) >>> 0; st ^= st >>> 13; st = Math.imul(st, 0x5bd1e995) >>> 0; return ((st ^ (st >>> 16)) >>> 0) / 4294967296; }; };
    const game = mk(0x9e3779b9), uuid = mk(12345); Math.random = () => { const e = {}; Error.captureStackTrace(e, Math.random); return (e.stack || '').includes('generateUUID') ? uuid() : game(); }; }
  ${mode === 'det' ? `const raf = requestAnimationFrame.bind(window); window.requestAnimationFrame = f => f.name === 'loop' ? 0 : raf(f);` : ''}
  window.__gl = { counts: Object.create(null) };
  const count = (name) => { window.__gl.counts[name] = (window.__gl.counts[name] || 0) + 1; };
  const names = ['bufferSubData','bufferData','texSubImage2D','texImage2D','texSubImage3D','drawElements','drawArrays','drawElementsInstanced','drawArraysInstanced','useProgram','bindTexture','bindBuffer','bindVertexArray','bindFramebuffer','uniformMatrix4fv','uniformMatrix3fv','uniform1f','uniform1i','uniform2f','uniform3f','uniform4f','uniform1fv','uniform2fv','uniform3fv','uniform4fv','uniform1iv','clear','getParameter','getError','readPixels','blitFramebuffer','compressedTexSubImage2D','compressedTexImage2D','texStorage2D','clientWaitSync','fenceSync'];
  const proto = WebGL2RenderingContext.prototype;
  for (const name of names) { const d = Object.getOwnPropertyDescriptor(proto, name); if (!d || typeof d.value !== 'function') continue; const original = d.value; proto[name] = function (...args) { count(name); return original.apply(this, args); }; }
  const getExt = proto.getExtension; const wrapped = new WeakSet();
  proto.getExtension = function (name) { const ext = getExt.call(this, name); if (ext && name === 'WEBGL_multi_draw' && !wrapped.has(ext)) { wrapped.add(ext); for (const k of ['multiDrawElementsInstancedWEBGL','multiDrawArraysInstancedWEBGL','multiDrawElementsWEBGL','multiDrawArraysWEBGL']) { const f = ext[k]; if (typeof f === 'function') ext[k] = function (...a) { count(k); window.__gl.counts['multiDrawCount'] = (window.__gl.counts['multiDrawCount'] || 0) + (a[a.length - 1] ?? 0); return f.apply(ext, a); }; } } return ext; };
` }, page);
await send('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: true, flatten: true }, page);
const ev = async (expr) => { const r = await send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true }, page); if (r.exceptionDetails) throw new Error('EXC: ' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text)); return r.result?.value; };
const query = process.env.QUERY || `map=nab2&unit=rep_inf_clone_trooper&ai=classic&tier=medium&heroes=0&seed=1&noreload=1&profileRuntime=1`;
await send('Page.navigate', { url: `${base}/?${query}` }, page);
for (let i = 0; i < 360; i++) { if (await ev('!!window.__bf?.frame')) break; await sleep(500); }
await ev('Promise.race([__bf.ready, new Promise(r => setTimeout(r, 90000))]).then(() => true)');
await ev('Promise.race([__bf.loaded(), new Promise(r => setTimeout(r, 90000))]).then(() => true)');
// Matrix compositions per frame (updateMatrix: the cost matrixAutoUpdate=false avoids).
await ev(`(() => { const P = __bf.THREE.Object3D.prototype; const o = P.updateMatrix; window.__umw = 0; P.updateMatrix = function () { window.__umw++; return o.call(this); }; return true; })()`);
const percentile = (values, q) => { const s = [...values].sort((a, b) => a - b); return s.length ? s[Math.min(s.length - 1, Math.max(0, Math.ceil(q * s.length) - 1))] : 0; };
const stat = (values) => ({ n: values.length, p50: +percentile(values, .5).toFixed(2), p95: +percentile(values, .95).toFixed(2), max: +Math.max(0, ...values).toFixed(2), mean: +(values.reduce((a, b) => a + b, 0) / Math.max(1, values.length)).toFixed(2) });
const inputs = `const key = (code, down) => window.dispatchEvent(new KeyboardEvent(down ? 'keydown' : 'keyup', { code, key: code, bubbles: true }));
    const look = (x, y) => document.dispatchEvent(new MouseEvent('mousemove', { movementX: x, movementY: y, bubbles: true }));
    const drive = (i) => { if (i % 120 === 0) key('KeyA', true); if (i % 120 === 60) key('KeyA', false);
      if (i % 90 === 0) window.dispatchEvent(new MouseEvent('mousedown', { button: 0, bubbles: true }));
      if (i % 90 === 20) window.dispatchEvent(new MouseEvent('mouseup', { button: 0, bubbles: true }));
      look(Math.round(6 * Math.sin(i * 0.05)), Math.round(2 * Math.cos(i * 0.03))); };`;
let result;
if (mode === 'det') {
  await ev(`(async () => { const b = __bf; b.pause(); b.begin?.(); for (let i = 0; i < ${Math.round(WARM * 60)}; i++) await b.lockstep(1 / 60, false); b.controls.locked = true; b.controls.enabled = true; return true; })()`);
  console.log('warmed; units alive', await ev('__bf.units.all().filter(u => u.alive).length'), '; random', await ev('JSON.stringify(__bf.randomState())'));
  result = await ev(`(async () => {
    const b = __bf, rows = []; ${inputs}
    b.runtimeProfile?.reset?.();
    key('KeyW', true);
    let prev = { ...window.__gl.counts }, prevU = window.__umw;
    for (let i = 0; i < ${FRAMES}; i++) {
      await new Promise(requestAnimationFrame);
      drive(i);
      const start = performance.now();
      await b.lockstep(1 / 60, true);
      const end = performance.now();
      const c = window.__gl.counts, d = {}; for (const k in c) d[k] = c[k] - (prev[k] || 0); prev = { ...c };
      rows.push({ cpu: end - start, calls: b.renderer.info.render.calls, triangles: b.renderer.info.render.triangles, gl: d, umw: window.__umw - prevU, heap: performance.memory?.usedJSHeapSize });
      prevU = window.__umw;
    }
    key('KeyW', false);
    return { rows, report: b.runtimeProfile?.report?.(), diag: b.graphics.diagnostics?.() };
  })()`);
} else {
  await ev(`(async () => { const b = __bf; b.begin?.(); b.controls.locked = true; b.controls.enabled = true; return true; })()`);
  console.log('worker mode: warming', WARM, 's; workers:', [...workers.values()].map(u => u.split('/').pop().split('?')[0]).join(','));
  await sleep(WARM * 1000);
  result = await ev(`(async () => {
    const b = __bf, rows = []; ${inputs}
    b.runtimeProfile?.reset?.();
    key('KeyW', true);
    let prev = { ...window.__gl.counts }, prevU = window.__umw, prevFrame = b.renderer.info.render.frame;
    for (let i = 0; i < ${FRAMES}; i++) {
      await new Promise(requestAnimationFrame);
      drive(i);
      const c = window.__gl.counts, d = {}; for (const k in c) d[k] = c[k] - (prev[k] || 0); prev = { ...c };
      const drew = b.renderer.info.render.frame !== prevFrame; prevFrame = b.renderer.info.render.frame;
      if (drew) rows.push({ calls: b.renderer.info.render.calls, triangles: b.renderer.info.render.triangles, gl: d, umw: window.__umw - prevU, heap: performance.memory?.usedJSHeapSize });
      prevU = window.__umw;
    }
    key('KeyW', false);
    return { rows, report: b.runtimeProfile?.report?.(), diag: b.graphics.diagnostics?.(), stats: document.querySelector('#stats')?.textContent };
  })()`);
}
const rows = result.rows;
const summary = { mode, frames: rows.length, calls: stat(rows.map(r => r.calls)), triangles: stat(rows.map(r => r.triangles)), updateMatrix: stat(rows.map(r => r.umw)), cpu: rows[0]?.cpu !== undefined ? stat(rows.map(r => r.cpu)) : null, gl: {}, phases: {} };
const glNames = new Set(); for (const r of rows) for (const k in r.gl) glNames.add(k);
for (const k of glNames) summary.gl[k] = stat(rows.map(r => r.gl[k] || 0));
for (const [k, v] of Object.entries(result.report?.metrics || {})) summary.phases[k] = { p50: v.p50, p95: v.p95, p99: v.p99, max: v.max };
summary.stats = result.stats; summary.diag = result.diag;
fs.writeFileSync(`${out}/summary-${mode}.json`, JSON.stringify(summary, null, 1));
fs.writeFileSync(`${out}/rows-${mode}.json`, JSON.stringify(rows));
console.log(`${mode}: ${rows.length} frames; calls p50 ${summary.calls.p50} tris p50 ${summary.triangles.p50} updateMatrix p50 ${summary.updateMatrix.p50}${summary.cpu ? ` cpu p50/p95 ${summary.cpu.p50}/${summary.cpu.p95}` : ''}`);
for (const k of ['bufferSubData','bufferData','texSubImage2D','texImage2D','drawElements','drawArrays','drawElementsInstanced','drawArraysInstanced','multiDrawElementsInstancedWEBGL','multiDrawCount','useProgram','bindTexture','bindBuffer','bindVertexArray','uniformMatrix4fv','uniform1f','uniform1i','uniform3fv','uniform4fv','uniform1fv','clear','getParameter','getError']) if (summary.gl[k]) console.log(`  gl.${k.padEnd(28)} p50 ${String(summary.gl[k].p50).padStart(7)} p95 ${String(summary.gl[k].p95).padStart(7)} mean ${summary.gl[k].mean}`);
console.log('  phases p50 / p95 / p99 / max (ms):'); for (const k of ['simulation','presentation','world','hud','render','cpuMs']) { const v = summary.phases[k]; if (v) console.log(`    ${k.padEnd(14)} ${[v.p50, v.p95, v.p99, v.max].map(x => (x ?? 0).toFixed(2).padStart(7)).join(' ')}`); }
if (summary.stats) console.log('  stats:', summary.stats);
if (process.env.SHOTS !== '0' && mode === 'det') {
  // Fidelity shots: the live view, then the mission's camera shots, each drawn with dt 0.
  const shots = await ev(`(async () => {
    const b = __bf; const out = {};
    const grab = (name) => { const gl = b.renderer.domElement; const c = document.createElement('canvas'); c.width = gl.width; c.height = gl.height; const ctx = c.getContext('2d'); ctx.drawImage(gl, 0, 0); const d = ctx.getImageData(0, 0, c.width, c.height).data; let h = 0; for (let i = 0; i < d.length; i++) h = (h * 31 + d[i]) >>> 0; let bin = ''; for (let i = 0; i < d.length; i += 0x8000) bin += String.fromCharCode.apply(null, d.subarray(i, i + 0x8000)); out[name] = { w: c.width, h: c.height, hash: h, png: c.toDataURL('image/png'), raw: btoa(bin) }; };
    await b.lockstep(0, true); grab('live');
    const cams = b.spawnPort?.cameraShots?.() || [];
    for (let i = 0; i < Math.min(3, cams.length); i++) { b.spawnPort.setViewShot(cams[i]); await b.lockstep(0, true); await b.lockstep(0, true); grab('shot' + i); }
    b.spawnPort?.setViewShot(null);
    return out;
  })()`);
  for (const [name, s] of Object.entries(shots)) { fs.writeFileSync(`${out}/${name}.png`, Buffer.from(s.png.split(',')[1], 'base64')); fs.writeFileSync(`${out}/${name}.rgba`, Buffer.from(s.raw, 'base64')); fs.writeFileSync(`${out}/${name}.size`, `${s.w} ${s.h}`); console.log(`  shot ${name}: ${s.w}x${s.h} hash ${s.hash}`); }
}
console.log('errors:', [...new Set(errors)].slice(0, 8));
chrome.kill('SIGKILL'); process.exit(0);
