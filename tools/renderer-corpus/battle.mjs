// Seeded CDP battle loader and context validity checks.
import fs from 'node:fs';

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function connect(port = Number(process.env.CDP_PORT || 9334)) {
  const info = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
  const ws = new WebSocket(info.webSocketDebuggerUrl);
  await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
  let id = 0; const waits = new Map(); const listeners = new Set();
  ws.onmessage = ({ data }) => { const m = JSON.parse(data); if (m.id && waits.has(m.id)) { waits.get(m.id)(m); waits.delete(m.id); return; } for (const l of listeners) l(m); };
  const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => { const i = ++id; waits.set(i, (m) => m.error ? reject(new Error(method + ': ' + JSON.stringify(m.error))) : resolve(m.result)); ws.send(JSON.stringify({ id: i, method, params, sessionId })); });
  const { targetInfos } = await send('Target.getTargets');
  const t = targetInfos.find((x) => x.type === 'page');
  const { sessionId } = await send('Target.attachToTarget', { targetId: t.targetId, flatten: true });
  const consoleErrors = [];
  listeners.add((m) => {
    if (m.sessionId !== sessionId) return;
    if (m.method === 'Runtime.exceptionThrown') consoleErrors.push('exception ' + String(m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text).slice(0, 300));
    if (m.method === 'Runtime.consoleAPICalled' && ['error', 'warning'].includes(m.params.type)) {
      const text = m.params.args.map((a) => a.value ?? a.description).join(' ');
      if (!/Electron Security Warning|KHR_parallel_shader_compile/.test(text)) consoleErrors.push(m.params.type + ' ' + text.slice(0, 300));
    }
  });
  await send('Runtime.enable', {}, sessionId); await send('Page.enable', {}, sessionId);
  const ev = async (expression, timeout = 900000) => { const r = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true, timeout }, sessionId); if (r.exceptionDetails) throw new Error('page: ' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text)); return r.result.value; };
  return { ws, send, on: (fn) => { listeners.add(fn); return () => listeners.delete(fn); }, sessionId, ev, psend: (m, p) => send(m, p, sessionId), close: () => ws.close(), consoleErrors };
}

export function settingsFor(extra = {}) { return extra; }
export function writeSettings(settings) {
  if (process.env.BFE_SETTINGS_FILE) fs.writeFileSync(process.env.BFE_SETTINGS_FILE, JSON.stringify({ complete: true, values: settings }));
}

// Object UUID allocation changes when dead render helpers are deleted. Keep
// that stream and audio scheduling separate from procedural visual randomness, in both
// the page and its workers. Capture builds retain names (minify: false).
export const SEED_SRC = `let seed = 0x9e3779b9, uuidSeed = 0x12345678, audioSeed = 0x6a09e667;
  globalThis.__gateRandom = {uuidCalls:0,audioCalls:0,valueCalls:0};
  Math.random = () => {
    const stack = new Error().stack;
    const uuid = stack.includes('at generateUUID ');
    const audio = /(?:AudioEngine|AudioPanel|Voice|LayeredHandle|ParameterizedHandle)[0-9]*[ .]|at (?:pickSample|rollGain|rollPitch|gate)[0-9]*[ (]/.test(stack);
    if(uuid)__gateRandom.uuidCalls++;else if(audio)__gateRandom.audioCalls++;else __gateRandom.valueCalls++;
    let t = uuid ? (uuidSeed = (uuidSeed + 0x6d2b79f5) | 0) : audio ? (audioSeed = (audioSeed + 0x6d2b79f5) | 0) : (seed = (seed + 0x6d2b79f5) | 0);
    t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };`;


// Context loss, seen from the first line of the page: a capture listener on window gets the
// canvas's (non-bubbling) events. Counts and times (ms since navigation).
export const CONTEXT_SRC = `window.__gateGl = { lost: 0, restored: 0, events: [] };
  window.addEventListener('webglcontextlost', () => { __gateGl.lost++; __gateGl.events.push(['lost', Math.round(performance.now())]); }, true);
  window.addEventListener('webglcontextrestored', () => { __gateGl.restored++; __gateGl.events.push(['restored', Math.round(performance.now())]); }, true);`;

// The page expression for the context's state now: lost events since the load, the context's
// own flag, and the bridge's (RustRender.lost on builds that have it).
export const GL_STATE = `(() => { const b = __bf, g = b.graphics, gl = b.renderer.getContext(), G = window.__gateGl || null;
  return { lostEvents: G ? G.lost : null, restoredEvents: G ? G.restored : null, events: G ? G.events.slice(0, 8) : null,
    contextLost: gl.isContextLost(), rustLost: g.rust ? (g.rust.contextLost ?? g.rust.lost ?? false) : null }; })()`;

// Xvfb is EMU_SCREEN, but the Electron window opens smaller. Pixel parity needs the
// window to fill that screen. Fullscreen is the window state that matches it.
export async function fitWindow(c) {
  const screen = process.env.EMU_SCREEN || '';
  if (!/^\d+x\d+/.test(screen)) return;
  try {
    const { targetInfos } = await c.send('Target.getTargets');
    const t = targetInfos.find((x) => x.type === 'page');
    const { windowId } = await c.send('Browser.getWindowForTarget', { targetId: t.targetId });
    await c.send('Browser.setWindowBounds', { windowId, bounds: { windowState: 'fullscreen' } });
  } catch (e) { /* Electron's CDP has no Browser.getWindowForTarget; the metrics override below sizes the page */ }
}

// Stop the worker at the same authority tick before cosmetic warmup. Its
// initial start acknowledgement can otherwise race one tick between sessions.
export const LOCKSTEP_START_SRC = `async b => {
  const host=b.client.host, target=60;
  const start=await host.call('lockstep');
  if(start.tick>target)throw Error('Worker ran past the capture start tick');
  host.drainEntities();
  while(b.worldClock.tick<target){await host.call('advance',{ticks:1});host.drainEntities();}
  await b.lockstep(0,false);
  return target;
}`;

export async function loadBattle(c, { settings, map = process.env.MAP || 'nab2', warm = Number(process.env.WARM || 12), extraSrc = '', query } = {}) {
  writeSettings(settings);
  await fitWindow(c);
  await sleep(1500);
  const drawn = Number(process.env.RENDER_WARM || 20);
  const nodraw = process.env.EMU_NODRAW ? `(() => { for (const C of [self.WebGL2RenderingContext, self.WebGLRenderingContext]) if (C) for (const m of ['drawElements','drawArrays','drawElementsInstanced','drawArraysInstanced','drawRangeElements']) if (C.prototype[m]) C.prototype[m] = function () {};
    const ge = WebGL2RenderingContext.prototype.getExtension; WebGL2RenderingContext.prototype.getExtension = function (n) { const e = ge.call(this, n); if (e && /multi_draw/i.test(n)) for (const k of Object.keys(Object.getPrototypeOf(e))) if (/^multiDraw/.test(k)) e[k] = function () {}; return e; }; })();` : '';
  const { identifier } = await c.psend('Page.addScriptToEvaluateOnNewDocument', { source: `${nodraw}${SEED_SRC}${CONTEXT_SRC}
    const raf = requestAnimationFrame.bind(window);
    window.requestAnimationFrame = f => f.name === 'loop' ? 0 : raf(f);
    localStorage.setItem('battlefront-eternal.settings', ${JSON.stringify(JSON.stringify(settings))});
    ${extraSrc}` });
  const workers = [];
  const off = c.on(async (m) => {
    if (m.method === 'Target.attachedToTarget' && m.sessionId === c.sessionId) {
      const ws = m.params.sessionId; workers.push(m.params.targetInfo.type);
      try { await c.send('Runtime.evaluate', { expression: SEED_SRC }, ws); } catch {}
      try { await c.send('Runtime.runIfWaitingForDebugger', {}, ws); } catch {}
    }
  });
  await c.psend('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: true, flatten: true });
  const q = query || process.env.Q || `map=${map}&unit=rep_inf_clone_trooper&ai=classic&tier=medium&heroes=0&seed=1&noreload=1&profileRuntime=1`;
  const t0 = Date.now();
  await c.psend('Page.navigate', { url: `${process.env.BFE_APP_URL || "app://bfe/"}?${q}` });
  let failed = null;
  for (let i = 0; i < 600; i++) {
    await sleep(500);
    try {
      const state = await c.ev(`window.__bf?.frame ? 'ready' : (document.body?.innerText || '').startsWith('Failed to load game') ? document.body.innerText.slice(0, 200) : null`);
      if (state === 'ready') break;
      if (state) { failed = state; break; }
    } catch {}
  }
  await c.psend('Page.removeScriptToEvaluateOnNewDocument', { identifier });
  await c.psend('Target.setAutoAttach', { autoAttach: false, waitForDebuggerOnStart: false, flatten: true });
  off();
  if (failed) return { error: failed, seconds: (Date.now() - t0) / 1000, spawned: false };
  const wh = (process.env.EMU_SCREEN || '').match(/^(\d+)x(\d+)/);
  if (wh) {
    try { await c.psend('Emulation.setDeviceMetricsOverride', { width: Number(wh[1]), height: Number(wh[2]), deviceScaleFactor: 1, mobile: false }); }
    catch (e) { console.log('metrics override failed', e.message); }
  }
  if (!process.env.WINDOWED) { await fitWindow(c); await c.psend('Runtime.evaluate', { expression: 'document.fullscreenElement || document.documentElement.requestFullscreen().catch(e => String(e))', userGesture: true, awaitPromise: true }); }
  const load = await c.ev(`(async () => { const b = __bf; await Promise.race([b.ready, new Promise(r => setTimeout(r, 180000))]); const prepareStart=performance.now(); await b.loaded?.(); const prepareMs=performance.now()-prepareStart; b.begin?.(); b.pause(); b.controls.locked = true; b.controls.enabled = true;
    const startTick=await (${LOCKSTEP_START_SRC})(b);
    for (let i = 0; i < ${warm} * 60; i++) await b.lockstep(1 / 60, false);
    for (let i = 0; i < ${drawn}; i++) await b.lockstep(1 / 60, true);
    const u = b.playerUnit;
    return { prepareMs, startTick, size: [innerWidth, innerHeight], spawned: !!(u && u.alive), unit: u ? (u.className || u.name || null) : null }; })()`);
  return { ...load, seconds: (Date.now() - t0) / 1000, workers: workers.length };
}

// The in-page helpers every gate tool uses (evaluate once after the load): window.__gate with
// key/look input, the Rust phase wrapper that keeps each phase's stats per frame (fresh: a
// phase that did not run this frame reports nothing), and a frame counter.
export const HELPERS_SRC = `(() => { const b = __bf, g = b.graphics;
  if (window.__gate) return 1;
  const G = window.__gate = { frameStats: {}, phaseCalls: 0, inRust: 0 };
  G.key = (code, down) => window.dispatchEvent(new KeyboardEvent(down ? 'keydown' : 'keyup', { code, key: code, bubbles: true }));
  G.look = (x, y) => document.dispatchEvent(new MouseEvent('mousemove', { movementX: x, movementY: y || 0, bubbles: true }));
  G.resetFrame = () => { G.frameStats = {}; };
  G.rustDraws = () => Object.values(G.frameStats).reduce((a, s) => a + s.slice(5).reduce((x, y) => x + y, 0), 0);
  const rust = g.rust;
  if (rust) {
    const phase = rust.phase;
    rust.phase = function (name, ...args) {
      G.inRust++;
      try { if (this.phaseStats) delete this.phaseStats[name]; return phase.call(this, name, ...args); }
      finally {
        G.inRust--; G.phaseCalls++;
        const s = this.phaseStats && this.phaseStats[name];
        if (s) { const acc = G.frameStats[name] ||= new Array(s.length).fill(0); for (let i = 0; i < s.length; i++) acc[i] += s[i]; }
      }
    };
  }
  return 1; })()`;
