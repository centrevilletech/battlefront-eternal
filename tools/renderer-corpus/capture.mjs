// Same-state three / bridge / owned / three controls, including a temporal sequence.
import fs from 'node:fs';
import { expand } from './manifest.mjs';
import { shaderSources } from '../../src/render-shaders.js';
import { connect, loadBattle, settingsFor, GL_STATE, HELPERS_SRC } from './battle.mjs';
const manifest = expand(JSON.parse(fs.readFileSync(new URL('./manifest.json', import.meta.url))));
const LABEL = process.env.LABEL || 'enparity', OUT = process.env.OUT || 'renderer-corpus-out';
const RGBA = process.env.RGBA || OUT;
const CAPTURES = Number(process.env.CAPTURES || manifest.captures), GAP = Number(process.env.GAP || manifest.gap);
const MIN_VALID = Number(process.env.MIN_VALID || CAPTURES);
const WANT_SAMPLES = Number(process.env.WANT_SAMPLES || 2);
const LIMIT = 0.01;
const LOG = process.env.BFE_GPU_LOG || null;
const GPU_DEATH = /GPU process exited|GPU process isn't usable|GPU process crashed|Lost UI shared context|exit_code=512/i;
fs.mkdirSync(OUT, { recursive: true });
fs.mkdirSync(RGBA, { recursive: true });
const write = (o) => fs.writeFileSync(`${OUT}/${LABEL}.json`, JSON.stringify(o, null, 1));
// The slot's electron.log, from its start: the GPU process deaths in it.
const gpuDeaths = () => {
  if (!LOG || !fs.existsSync(LOG)) return null;
  return fs.readFileSync(LOG, 'utf8').split('\n').filter((l) => GPU_DEATH.test(l)).map((l) => l.slice(0, 200));
};
const caseId = process.env.BFE_CORPUS_CASE;
const testCase = manifest.cases.find((c) => c.id === caseId);
if (!testCase) throw Error('Unknown corpus case ' + caseId);
process.env.Q = testCase.query;
const base = testCase.settings, named = {};
const c = await connect();
let loaded = null;
if (!process.env.NOLOAD) loaded = await loadBattle(c, { settings: settingsFor({ ...base, ...named, ...JSON.parse(process.env.SETTINGS || '{}') }), query: process.env.Q });
console.log('loaded', JSON.stringify(loaded));
if (loaded && (loaded.error || loaded.spawned === false)) {
  write({ error: loaded.error || 'player not spawned', loaded, query: process.env.Q, gpuDeaths: gpuDeaths(), console: c.consoleErrors.slice(0, 20) });
  c.close(); process.exit(2);
}
await c.ev(HELPERS_SRC);
// LAMPNOW=1: the lamps' selection refreshed at every frame's first ask (its clock moved a second a frame), so a
// capture's lamps don't depend on when the last refresh happened (local-lights.js caches a body's for 0.1 s of
// wall time).
// DEBUGLAMPS=1: each object's lamps (lamp table indices) as select() gives them in the captured frame, per variant.
if (process.env.DEBUGLAMPS) await c.ev(`(() => { const g = __bf.graphics, L = g.localLights; if (!L) return 0; const s0 = L.select; window.__lampLog = [];
  L.select = function (object, viewModel) { const lights = s0.call(this, object, viewModel);
    if (window.__gatePar?.want) { const ix = g.rust?.lampIndex; const e = new THREE_VEC(); e.setFromMatrixPosition(object.matrixWorld);
      window.__lampLog.push([object.uuid.slice(0, 8), object.name || object.parent?.name || object.type, !!object.isSkinnedMesh, !!viewModel, lights.map((l) => ix?.get(l) ?? -2), [e.x, e.y, e.z].map((v) => +v.toFixed(4))]); }
    return lights; }; return 1; })()`.replace('new THREE_VEC()', '({ x: 0, y: 0, z: 0, setFromMatrixPosition(m) { const a = m.elements; this.x = a[12]; this.y = a[13]; this.z = a[14]; return this; } })'));
if (process.env.LAMPNOW) await c.ev(`(() => { const L = __bf.graphics.localLights; if (!L) return 0; const b0 = L.begin; let t = 0; L.begin = function (...a) { b0.apply(this, a); this.now = t += 1; }; return 1; })()`);
const setup = await c.ev(`(async () => { const b = __bf, g = b.graphics, R = b.renderer, gl = R.getContext();
  for (let i = 0; i < 100 && !g.rust; i++) await new Promise((r) => setTimeout(r, 100));
  if (!g.rust) return { error: 'no rust renderer (load with ?rust)' };
  // The presented frame, read inside the frame right after the HUD has drawn into it.
  const T = window.__gatePar = { want: false, shot: null };
  const hudRender = b.hud.render;
  b.hud.render = function (...a) { const r = hudRender.apply(this, a);
    if (T.want) { const w = gl.drawingBufferWidth, h = gl.drawingBufferHeight, buf = new Uint8Array(w * h * 4); R.setRenderTarget(null); gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, buf); T.shot = { w, h, buf }; }
    return r; };
  return { errors: [...new Set(g.rust.errors)].slice(0, 30), enabled: g.rust.enabled, supported: typeof g.rust.supported === 'function' ? g.rust.supported() : null, hasLost: 'lost' in g.rust,
    buffer: [gl.drawingBufferWidth, gl.drawingBufferHeight], samples: g.sceneTarget.samples, farSamples: g.farTarget.samples,
    target: [g.sceneTarget.width, g.sceneTarget.height], supportedSamples: g.supportedSamples,
    antialiasing: g.settings.get('antialiasing'), dynamicResolution: g.settings.get('dynamicResolution'), renderScale: g.settings.get('renderScale'), upscaling: g.upscaling,
    renderer: (() => { try { const e = gl.getExtension('WEBGL_debug_renderer_info'); return e ? gl.getParameter(e.UNMASKED_RENDERER_WEBGL) : null; } catch { return null; } })() }; })()`);
await c.ev(`(() => { const g = __bf.graphics, R = g.renderer, gl = R.getContext(), T = window.__gatePar, before = g.postSteps;
 g.postSteps = function (...args) {
   if (T.want) { T.stages = {}; const old = gl.getParameter(gl.READ_FRAMEBUFFER_BINDING);
     for (const [name, target] of [['far', g.farTarget], ['scene', g.sceneTarget], ...(g.upscaling === 'temporal' ? [['velocity',g.upscaler.velocity],['motion',g.upscaler.motion],['history',g.upscaler.history[g.upscaler.read]]] : []), ...(g.scene.userData.water?.reflectionFar ? [['reflection',g.scene.userData.water.target],['reflectionFar',g.scene.userData.water.reflectionFar]] : [])]) {
       const half = target.texture.type === 1016; const buf = half ? new Uint16Array(target.width * target.height * 4) : new Uint8Array(target.width * target.height * 4);
       gl.bindFramebuffer(gl.READ_FRAMEBUFFER, R.properties.get(target).__webglFramebuffer);
       gl.readPixels(0, 0, target.width, target.height, gl.RGBA, half ? gl.HALF_FLOAT : gl.UNSIGNED_BYTE, buf); T.stages[name] = buf;
     } gl.bindFramebuffer(gl.READ_FRAMEBUFFER, old);
   } return before.apply(this, args);
 }; return 1; })()`);
await c.ev(`(() => {
 const g=__bf.graphics, rust=g.rust, R=g.renderer, gl=R.getContext(), T=window.__gatePar;
 T.pairs=[];
 const read=(target)=> { const w=target?.width || gl.drawingBufferWidth,h=target?.height || gl.drawingBufferHeight,buf=target?.texture.type === 1016 ? new Uint16Array(w*h*4) : target?.texture.type === 1015 ? new Float32Array(w*h*4) : new Uint8Array(w*h*4); if(target) R.readRenderTargetPixels(target,0,0,w,h,buf); else { R.setRenderTarget(null);gl.readPixels(0,0,w,h,gl.RGBA,gl.UNSIGNED_BYTE,buf); } const error=gl.getError();if(error)throw Error('stage read GL '+error+' type '+target?.texture.type);return buf; };
 const diff=(a,b)=> {let differing=0,maxDelta=0;for(let i=0;i<a.length;i+=4){let d=0;for(let j=0;j<4;j++)d=Math.max(d,Math.abs(a[i+j]-b[i+j]));if(d){differing++;maxDelta=Math.max(maxDelta,d);}}return {differing,maxDelta};};
 const post=rust.post;
 rust.post=function(renderer,steps) {
   if(!T.want || !this.active || !this.enabled) return post.call(this,renderer,steps);
   const mode=this.ownedShaderSources;
   if (mode === false || ${process.env.WHOLE_CHAIN ? 'true' : 'false'}) {
     const images={},drawn={};
     for(const owned of (mode === false ? [true,false] : [false,true])) {
       this.ownedShaderSources=owned;drawn[owned]=post.call(this,renderer,steps);
       if(!drawn[owned]) for(const step of steps)g.postStep(renderer,step);
       images[owned]=read(steps[steps.length-1].target);
     }
     T.pairs.push({key:'whole-chain',bridge:drawn.false,owned:drawn.true,...diff(images.false,images.true)});
     if (mode === false) { this.ownedShaderSources=mode; return true; }
   }
   for(const step of steps) {
     const images={}, drawn={};
     for(const owned of (mode === false ? [true,false] : [false,true])) {
       this.ownedShaderSources=owned; drawn[owned]=post.call(this,renderer,[step]);
       if(!drawn[owned]) g.postStep(renderer,step); images[owned]=read(step.target);
     }
     const bridge=drawn.false, owned=drawn.true, a=images.false, b=images.true;
     T.pairs.push({key:step.pass?.material.userData.bfeProgram || 'run',bridge,owned,...diff(a,b)});
   }
   this.ownedShaderSources=mode; return true;
 };
 // Keep the identical pre-HUD canvas for both alpha blended batches.
 const hud=rust.coverHud;
 let texture=null,fb=null;
 rust.coverHud=function(renderer,batch) {
   if(!T.want || !this.active || !this.enabled) return hud.call(this,renderer,batch);
   const w=gl.drawingBufferWidth,h=gl.drawingBufferHeight;
   if(!texture) { texture=gl.createTexture();fb=gl.createFramebuffer(); }
   R.setRenderTarget(null); gl.activeTexture(gl.TEXTURE0);gl.bindTexture(gl.TEXTURE_2D,texture);
   gl.copyTexImage2D(gl.TEXTURE_2D,0,gl.RGBA,0,0,w,h,0);
   gl.bindFramebuffer(gl.READ_FRAMEBUFFER,fb);gl.framebufferTexture2D(gl.READ_FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,texture,0);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,null);
   R.resetState();R.setRenderTarget(null);
   const mode=this.ownedShaderSources, images={},drawn={};
   for(const owned of (mode === false ? [true,false] : [false,true])) {
     gl.bindFramebuffer(gl.READ_FRAMEBUFFER,fb);gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER,null);gl.blitFramebuffer(0,0,w,h,0,0,w,h,gl.COLOR_BUFFER_BIT,gl.NEAREST);gl.bindFramebuffer(gl.READ_FRAMEBUFFER,null);
     R.resetState();R.setRenderTarget(null);this.ownedShaderSources=owned;
     drawn[owned]=hud.call(this,renderer,batch);images[owned]=read(null);
   }
   const bridge=drawn.false,owned=drawn.true,a=images.false,b=images.true;
   T.pairs.push({key:'post-hud/hud',bridge,owned,...diff(a,b)});this.ownedShaderSources=mode;return mode === false ? bridge : owned;
 };
 return 1;
})()`);
console.log('setup', JSON.stringify(setup));
if (setup.error) { write({ error: setup.error, loaded, query: process.env.Q }); c.close(); process.exit(1); }

const diff = (A, B) => {
  if (A.w !== B.w || A.h !== B.h) return { error: 'size', a: [A.w, A.h], b: [B.w, B.h] };
  let n = 0, max = 0, sum = 0;
  for (let i = 0; i < A.d.length; i += 4) { let m = 0; for (let k = 0; k < 3; k++) m = Math.max(m, Math.abs(A.d[i + k] - B.d[i + k])); if (m > 0) { n++; sum += m; if (m > max) max = m; } }
  const px = A.w * A.h;
  return { pixels: px, differing: n, pct: +((100 * n) / px).toFixed(5), maxDelta: max, meanDeltaOfDiffering: n ? +(sum / n).toFixed(2) : 0 };
};
// A control is a per-pixel envelope, never just a worst-count allowance.
const controlResidual = (a, b, c) => {
  let differing = 0, stablePixels = 0, maxExcess = 0;
  for (let i=0;i<a.d.length;i+=4) {
    let delta=0,control=0;
    for(let j=0;j<3;j++) { delta=Math.max(delta,Math.abs(a.d[i+j]-b.d[i+j]));control=Math.max(control,Math.abs(a.d[i+j]-c.d[i+j])); }
    if(delta>control) { differing++; if(!control)stablePixels++;maxExcess=Math.max(maxExcess,delta-control); }
  }
  return { differing, stablePixels, maxExcess };
};
// The share of lit pixels (any channel over 8): a blank or cleared frame is not a capture.
const lit = (S) => { let n = 0; for (let i = 0; i < S.d.length; i += 64) if (S.d[i] > 8 || S.d[i + 1] > 8 || S.d[i + 2] > 8) n++; return +(n / (S.d.length / 64)).toFixed(3); };
if (testCase.camera === 'water') await c.ev(`(() => {
 const b=__bf,g=b.graphics,water=g.scene.userData.water,T=window.__gatePar,V=b.THREE.Vector3;
 if(!water || water.ocean) throw Error('No planar mirror water');
 T.place=k=>{const eye=g.camera.position,cells=[...water.cellBounds];cells.sort((a,z)=>((a.min.x+a.max.x)/2-eye.x)**2+((a.min.z+a.max.z)/2-eye.z)**2-((z.min.x+z.max.x)/2-eye.x)**2-((z.min.z+z.max.z)/2-eye.z)**2);const box=cells[k%cells.length];if(!box)throw Error('No water cells');const x=(box.min.x+box.max.x)/2,z=(box.min.z+box.max.z)/2,angle=.9+1.7*k;T.pose={eye:new V(x+Math.cos(angle)*14,water.level+5,z+Math.sin(angle)*14),at:new V(x,water.level,z)};};
 const aim=c=>{if(T.pose){c.position.copy(T.pose.eye);c.lookAt(T.pose.at);c.updateMatrixWorld();}};
 const reflect=water.reflect;water.reflect=function(c){aim(c);this.reflectFrames=0;return reflect.call(this,c);};
 const render=g.render;g.render=function(...a){aim(g.camera);return render.apply(this,a);};return 1;
})()`);
const deathsAtStart = gpuDeaths();
const points = [];
let worst = null;
for (let k = 0; k < CAPTURES; k++) {
  await c.ev(`(async () => { const b = __bf, g = b.graphics, G = window.__gate; g.rust.setActive(true); g.rust.ownedShaderSources = true; window.__gatePar.bridgeStages = null;
    G.key('KeyW', true); window.dispatchEvent(new MouseEvent('mousedown', {button:0,bubbles:true}));
    for (let i = 0; i < ${GAP}; i++) { const f = ${k} * ${GAP} + i; if (f % 90 === 0) G.key('KeyA', true); if (f % 90 === 45) G.key('KeyA', false); G.look(Math.round(9 * Math.sin(f * 0.07)), Math.round(3 * Math.cos(f * 0.05))); await b.lockstep(1 / 60, true); }
    window.dispatchEvent(new MouseEvent('mouseup', {button:0,bubbles:true})); G.key('KeyW', false); G.key('KeyA', false); window.__gatePar.place?.(${k}); return 1; })()`);
  const shots = {}, meta = {};
  for (const v of (process.env.SWAP ? ['three', 'rust', 'bridge', 'three2'] : ['three', 'bridge', 'rust', 'three2'])) {
    const raw = await c.ev(`(async () => { const k = ${k}; const b = __bf, g = b.graphics, G = window.__gate, T = window.__gatePar, gl = b.renderer.getContext();
      g.rust.setActive(${v === 'rust' || v === 'bridge'}); g.rust.ownedShaderSources = ${v === 'rust'};
      const pin = () => { if (g.upscaling === 'temporal') g.upscaler.frame = k; };
      for (const w of g.cascadeWindows || []) w.invalidate();
      for (let i = 0; i < ${manifest.settleFrames || 2}; i++) await b.lockstep(0, true);
      g.upscaler?.invalidate(); pin();
      for (let i = 0; i < ${manifest.historyFrames}; i++) await b.lockstep(0, true);
      G.resetFrame(); if (window.__lampLog) window.__lampLog.length = 0; T.want = true; T.shot = null; T.pairs = [];
      const tick = await b.lockstep(0, true);
      T.want = false;
      const stages = {}; if (${v === 'three'}) T.stageRef = T.stages;
      for (const [name, buf] of Object.entries(T.stages || {})) { const a = T.stageRef[name]; let differing=0,maxDelta=0; for(let i=0;i<buf.length;i+=4) {let delta=0;for(let j=0;j<3;j++) delta=Math.max(delta,Math.abs(a[i+j]-buf[i+j]));if(delta){differing++;maxDelta=Math.max(maxDelta,delta);}} stages[name]={differing,maxDelta}; }
      if (${v === 'bridge'}) T.bridgeStages = T.stages;
      if (${v === 'rust'} && T.bridgeStages) for (const [name, buf] of Object.entries(T.stages || {})) {
        const a = T.bridgeStages[name]; let differing = 0, maxDelta = 0;
        for (let i=0;i<buf.length;i+=4) { let d=0; for(let j=0;j<3;j++) d=Math.max(d, Math.abs(a[i+j]-buf[i+j])); if(d) { differing++; maxDelta=Math.max(d,maxDelta); } }
        stages[name].owned = { differing, maxDelta };
      }
      const s = T.shot; if (!s) return { error: 'no capture (the HUD did not draw)' };
      let str = ''; for (let i = 0; i < s.buf.length; i += 0x8000) str += String.fromCharCode.apply(null, s.buf.subarray(i, i + 0x8000));
      return { pairs: T.pairs, stages, w: s.w, h: s.h, data: btoa(str), tick: typeof tick === 'number' ? tick : null, threeCalls: g.frameInfo?.calls ?? null, rustDraws: G.rustDraws(), rustPhases: Object.keys(G.frameStats), errors: g.rust.errors.length, lampLog: window.__lampLog ? [...window.__lampLog] : null, gl: { ...${GL_STATE}, error: gl.getError() } }; })()`);
    if (raw.error) { meta[v] = { error: raw.error }; break; }
    const S = { w: raw.w, h: raw.h, d: Buffer.from(raw.data, 'base64') };
    shots[v] = S;
    meta[v] = { pairs: raw.pairs, stages: raw.stages, w: raw.w, h: raw.h, tick: raw.tick, threeCalls: raw.threeCalls, rustDraws: raw.rustDraws, rustPhases: raw.rustPhases, lit: lit(S), gl: raw.gl, rustErrors: raw.errors, lampLog: raw.lampLog || undefined };
    console.log(JSON.stringify({ k, v, ...meta[v], gl: undefined, ...raw.gl, events: undefined }));
  }
  const deaths = gpuDeaths();
  const why = [];
  for (const v of ['three', 'bridge', 'rust', 'three2']) {
    const m = meta[v];
    if (!m || m.error) { why.push(`${v}: ${m?.error || 'no capture'}`); continue; }
    if (m.gl.lostEvents) why.push(`${v}: webglcontextlost fired (${m.gl.lostEvents})`);
    if (m.gl.lostEvents === null) why.push(`${v}: no context listener`);
    if (m.gl.contextLost) why.push(`${v}: gl.isContextLost()`);
    if (m.gl.rustLost === true) why.push(`${v}: rust.lost`);
    if (m.gl.rustEnabled !== true) why.push(`${v}: rust disabled`);
    if (m.lit < 0.2) why.push(`${v}: blank frame (lit ${m.lit})`);
    if (m.w !== 1600 || m.h !== 900) why.push(`${v}: ${m.w}x${m.h}, not 1600x900`);
  }
  if (meta.rust && !meta.rust.error && !(meta.rust.rustDraws > 0)) why.push('rust: the renderer drew nothing');
  for (const v of ['three', 'three2']) if (meta[v] && !meta[v].error && meta[v].rustDraws > 0) why.push(`${v}: the renderer drew while off`);
  if (meta.three?.tick !== meta.rust?.tick || meta.rust?.tick !== meta.three2?.tick) why.push('ticks differ');
  if (deaths && deaths.length) why.push(`electron.log: ${deaths.length} GPU process death(s)`);
  if (Object.values(meta).some((m) => m.gl?.error)) why.push('GL error');
  const valid = why.length === 0;
  const point = { k, valid, why, meta };
  if (shots.three && shots.rust && shots.three2) {
    point.controlResidual = controlResidual(shots.three, shots.rust, shots.three2);
    point.owned = diff(shots.bridge, shots.rust);
    point.bridge = diff(shots.three, shots.bridge);
    point.rust = diff(shots.three, shots.rust);
    point.control = diff(shots.three, shots.three2);
    point.rust2 = diff(shots.three2, shots.rust);
    console.log(`  k=${k} ${valid ? 'valid' : 'INVALID (' + why.join('; ') + ')'}: rust ${point.rust.differing}/${point.rust.pixels} (${point.rust.pct}%) max ${point.rust.maxDelta}; control ${point.control.differing} (${point.control.pct}%)`);
    // The worst differing valid pair is written once, at the end: an intermediate file in the shared
    // out directory would be copied home by another map's job on the farm (rslot copies what is new).
    if (valid && point.rust.differing > 0 && (!worst || point.rust.differing > worst.differing)) {
      worst = { k, differing: point.rust.differing, shots: { three: shots.three, rust: shots.rust } };
    } else if (process.env.KEEP_RGBA) {
      for (const v of ['three', 'bridge', 'rust', 'three2']) fs.writeFileSync(`${RGBA}/${LABEL}-${v}-${k}.rgba`, Buffer.concat([Buffer.from(new Uint32Array([shots[v].w, shots[v].h]).buffer), shots[v].d]));
    }
  } else console.log(`  k=${k} INVALID: ${why.join('; ')}`);
  points.push(point);
  if (!valid && !process.env.KEEP_GOING) break;
}
await c.ev(`(__bf.graphics.rust && __bf.graphics.rust.setActive(true), 1)`).catch(() => {});
if (worst) {
  worst.files = ['three', 'rust'].map((v) => `${RGBA}/${LABEL}-${v}-${worst.k}.rgba`);
  ['three', 'rust'].forEach((v, i) => fs.writeFileSync(worst.files[i], Buffer.concat([Buffer.from(new Uint32Array([worst.shots[v].w, worst.shots[v].h]).buffer), worst.shots[v].d])));
}
const inputHashes = await c.ev(`(async () => {
 const g=__bf.graphics,R=g.renderer,gl=R.getContext();
 const hash=async data=> {const digest=await crypto.subtle.digest('SHA-256',data);return [...new Uint8Array(digest)].map(v=>v.toString(16).padStart(2,'0')).join('');};
 const text=s=>new TextEncoder().encode(s);
 const shaders=[],programs=[];
 for(const p of R.info.programs || []) {
   const attributes=[],uniforms=[];
   for(let i=0;i<gl.getProgramParameter(p.program,gl.ACTIVE_ATTRIBUTES);i++){const a=gl.getActiveAttrib(p.program,i);attributes.push({name:a.name,type:a.type,size:a.size,location:gl.getAttribLocation(p.program,a.name)});}
   for(let i=0;i<gl.getProgramParameter(p.program,gl.ACTIVE_UNIFORMS);i++){const u=gl.getActiveUniform(p.program,i);uniforms.push({name:u.name,type:u.type,size:u.size});}
   const stages=[];
   for(const shader of gl.getAttachedShaders(p.program) || []){const source=gl.getShaderSource(shader),h=await hash(text(source));shaders.push({stage:gl.getShaderParameter(shader,gl.SHADER_TYPE),hash:h,source});stages.push(h);}
   programs.push({key:p.cacheKey,stages,attributes,uniforms});
 }
 const textures=new Set();
 const material=m=> {if(!m)return;for(const u of Object.values(m.uniforms || {}))if(u.value?.isTexture)textures.add(u.value);if(m.map)textures.add(m.map);};
 for(const scene of [g.scene,g.farScene,g.viewScene,__bf.hud.scene]) scene?.traverse(o=>{for(const m of (Array.isArray(o.material)?o.material:[o.material]))material(m);});
 const payloads=[];
 for(const t of textures){const parts=[];const images=[t.image,...(t.mipmaps || [])];for(const im of images){if(!im)continue;if(im.data && ArrayBuffer.isView(im.data))parts.push(await hash(im.data));else if(im.width && im.height && !t.isRenderTargetTexture){try{const c=document.createElement('canvas');c.width=im.width;c.height=im.height;const x=c.getContext('2d');x.drawImage(im,0,0);parts.push(await hash(x.getImageData(0,0,c.width,c.height).data));}catch{parts.push('unreadable');}}}payloads.push({format:t.format,type:t.type,wrap:[t.wrapS,t.wrapT],filter:[t.minFilter,t.magFilter],flip:t.flipY,mips:parts});}
 return {shaders,programs,textures:payloads,textureHash:await hash(text(JSON.stringify(payloads)))};
})()`);
const shaderDir = `${OUT}/shaders`;
fs.mkdirSync(shaderDir, { recursive: true });
for (const shader of inputHashes.shaders) { fs.writeFileSync(`${shaderDir}/${shader.hash}.glsl`, shader.source); delete shader.source; }
const good = points.filter((p) => p.valid && p.rust && !p.rust.error);
const sizeOk = setup.buffer[0] === 1600 && setup.buffer[1] === 900 && setup.target[0] === Math.round(1600 * testCase.settings.renderScale) && setup.target[1] === Math.round(900 * testCase.settings.renderScale);
const samplesNote = setup.samples === WANT_SAMPLES ? `${setup.samples} samples` : (setup.supportedSamples || []).includes(WANT_SAMPLES) ? `${setup.samples} samples, not the ${WANT_SAMPLES} asked` : `${setup.samples} samples: the GPU offers ${JSON.stringify(setup.supportedSamples)}, no ${WANT_SAMPLES}`;
const out = {
  contract: { version: manifest.version, captures: CAPTURES, gap: GAP, historyFrames: manifest.historyFrames, settleFrames: manifest.settleFrames || 2 },
  complete: CAPTURES === manifest.captures && GAP === manifest.gap && MIN_VALID === manifest.captures,
  label: LABEL, query: process.env.Q, loaded, setup, samplesNote, sizeOk,
  emuArgs: process.env.EMU_ARGS || null, gpuDeathsAtStart: deathsAtStart, gpuDeaths: gpuDeaths(),
  inputHashes,
  shaderPrograms: await c.ev(`__bf.graphics.rust.shaderPrograms || []`),
  points, validPoints: good.length, capturesAsked: CAPTURES, minValid: MIN_VALID,
  worstRustPct: good.length ? Math.max(...good.map((p) => p.rust.pct)) : null,
  worstRustPixels: good.length ? Math.max(...good.map((p) => p.rust.differing)) : null,
  worstRust2Pixels: good.length ? Math.max(...good.map((p) => p.rust2.differing)) : null,
  meanRustPct: good.length ? +(good.reduce((a, p) => a + p.rust.pct, 0) / good.length).toFixed(5) : null,
  worstControlPct: good.length ? Math.max(...good.map((p) => p.control.pct)) : null,
  rgba: worst ? worst.files : [],
  limitPct: LIMIT,
  console: c.consoleErrors.slice(0, 30),
};
out.valid = good.length >= MIN_VALID && sizeOk;
out.worstOwnedPixels = good.length ? Math.max(...good.map(p => p.owned.differing)) : null;
const byHash = new Map(inputHashes.shaders.map(s => [s.hash, fs.readFileSync(`${shaderDir}/${s.hash}.glsl`, 'utf8')]));
const seen = [...new Set(out.shaderPrograms.filter(p => p.hash && p.accepted).map(p => p.key))];
out.missingOracles = seen.filter(key => !inputHashes.programs.some(p => {
  const sources=p.stages.map(h=>byHash.get(h));
  return sources.some(s=>s.includes(shaderSources[key].vertex.trim())) && sources.some(s=>s.includes(shaderSources[key].fragment.trim()));
}));
out.mirrorCovered = testCase.camera !== 'water' || good.every(p => p.meta.rust.rustPhases.includes('mirror') && p.meta.rust.stages.reflection);
out.exactPairs = good.flatMap(p => [...(p.meta.bridge.pairs || []), ...(p.meta.rust.pairs || [])]);
out.worstPairPixels = out.exactPairs.length ? Math.max(...out.exactPairs.map(p => p.differing)) : null;
out.pass = out.valid && out.missingOracles.length === 0 && out.mirrorCovered && out.shaderPrograms.some(p => p.accepted && p.hash) && out.shaderPrograms.every(p => p.accepted) && out.exactPairs.length > 0 && out.exactPairs.every(p => p.bridge && p.owned && p.differing === 0);
out.baselinePass = out.valid && good.every(p => p.rust.differing <= (testCase.preset === 'classic' ? 0 : 144) || p.controlResidual.differing === 0 || p.rust2.differing === 0);
if (testCase.contextLoss) {
  out.contextLoss = await c.ev(`(async () => { const g=__bf.graphics, gl=g.renderer.getContext(), ext=gl.getExtension('WEBGL_lose_context'); if(!ext) return {supported:false}; ext.loseContext(); await new Promise(r=>setTimeout(r,100)); const lost=g.rust.lost; ext.restoreContext(); await new Promise(r=>setTimeout(r,1500)); await __bf.lockstep(0,true); return {supported:true,lost,active:g.rust.active,enabled:g.rust.enabled,restored:!gl.isContextLost()}; })()`);
  out.pass &&= out.contextLoss.supported && out.contextLoss.lost && !out.contextLoss.enabled && out.contextLoss.restored;
}
write(out);
console.log(JSON.stringify({ valid: out.valid, validPoints: good.length, sizeOk, samples: setup.samples, samplesNote, worstRustPct: out.worstRustPct, worstRustPixels: out.worstRustPixels, worstControlPct: out.worstControlPct, pass: out.pass }));
c.close();
process.exit(out.pass ? 0 : 3);
