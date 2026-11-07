// One application session of the whole-build gate. The reference session
// records its renderer inputs once; a repeat session of the reference and the
// candidate session replay them. Every role also reads the finished frame, so
// builds with no stage interface (a three.js-only build) still get compared,
// against the repeat reference's own run-to-run noise.
//
// Environment: BFE_GATE_CASE, BFE_GATE_ROLE (reference | repeat | candidate),
// OUT (the case's directory), BFE_GATE_RAW (pixel and input files, removed by
// the comparison), BFE_GATE_STAGES=1 to record and replay stage inputs, CDP_PORT,
// BFE_GATE_TRACE=1 to record the player's position, aim, wall time and every unit's position at every tick.
import fs from 'node:fs';
import { createHash } from 'node:crypto';
import { gateCase, gateManifest as manifest } from './cases.mjs';
import { installReplay } from '../renderer-corpus/replay.mjs';
import { installWorldTrace, installWorldReplay } from '../renderer-corpus/world-replay.mjs';
import { programLayouts } from '../../src/render-programs.js';
import { connect, loadBattle, sleep, writeSettings, SEED_SRC, CONTEXT_SRC, GL_STATE, HELPERS_SRC } from '../renderer-corpus/battle.mjs';

const role = process.env.BFE_GATE_ROLE, out = process.env.OUT, raw = process.env.BFE_GATE_RAW || out;
if (!['reference', 'repeat', 'candidate'].includes(role) || !out) throw Error('Supply OUT and BFE_GATE_ROLE');
const testCase = gateCase(process.env.BFE_GATE_CASE);
const stages = process.env.BFE_GATE_STAGES === '1' && testCase.kind !== 'shell';
const dir = `${out}/${role}`, rawDir = `${raw}/${role}`;
fs.mkdirSync(dir, { recursive: true });
fs.mkdirSync(rawDir, { recursive: true });
const result = { role, id: testCase.id, kind: testCase.kind, stages, points: [] };
const hash = (buf) => createHash('sha256').update(buf).digest('hex');
const store = (name, buf) => { const file = `${rawDir}/${name}`; fs.writeFileSync(file, buf); return file; };
// The page's own bytes, as base64 in pieces a protocol message can carry.
const pageBytes = (c, expr) => c.ev(`(() => {const buf=${expr};let s='';for(let i=0;i<buf.length;i+=0x8000)s+=String.fromCharCode.apply(null,buf.subarray(i,i+0x8000));return btoa(s);})()`);

// Everything about the page that decides what a frame shows, read the same way
// from every build: a difference in pixels with a difference here has a cause
// in the world, not the renderer.
const STATE_SRC = `(() => { const b=__bf,g=b.graphics,n=a=>[a.x,a.y,a.z].map(v=>+v.toFixed(4)),r=a=>a.map(v=>+v.toFixed(5));
  const out={tick:b.worldClock?.tick??null};
  try{const u=b.playerUnit;out.player=n(u.position);out.alive=!!u.alive;out.seat=u.seat?.vehicle?.name??null;out.aim=r([b.soldier.aimYaw,b.soldier.aimPitch]);}catch{}
  try{const c=g.camera;out.camera=n(c.position);out.cameraTurn=r([c.quaternion.x,c.quaternion.y,c.quaternion.z,c.quaternion.w]);out.lens=r([c.fov,c.near,c.far,c.aspect]);}catch{}
  try{const f=g.scene.fog;out.fog=f?r([f.color.r,f.color.g,f.color.b,f.near??0,f.far??0,f.density??0]):null;}catch{}
  // Where every live soldier and hull stands as drawn, folded into one number: a frame that differs
  // with this equal differs in the renderer; with it different, the world was not the same.
  try{const live=b.units.all().filter(u=>u.alive&&u.position);out.units=live.length;
    let h=0x811c9dc5;for(const u of live.slice().sort((a,z)=>a.id-z.id))for(const v of [u.id,u.position.x,u.position.y,u.position.z])h=Math.imul(h^Math.round(v*1000),0x01000193)>>>0;out.unitsHash=h;
    out.vehicles=b.vehicles.vehicles.filter(v=>v.alive).length;}catch{}
  return out; })()`;

// A shell screen is drawn by the front end's own canvas menus, stepped by hand
// (the classic menus: no movies, sound or DOM animation), so a screen is a
// fixed function of the build. The page must first have drawn a battle for
// the GPU check of a farm job, so a minimal battle is loaded and then left.
const SHELL_BATTLE = 'map=nab2&unit=rep_inf_clone_trooper&ai=classic&tier=medium&heroes=0&seed=1&noreload=1&profileRuntime=1';
const SUBSCREENS = ['general', 'video', 'sound', 'controls'];
async function captureShell(c) {
  result.loaded = await loadBattle(c, { settings: testCase.settings, query: SHELL_BATTLE, warm: 0, extraSrc: READY_SRC });
  writeSettings(testCase.settings);
  // The front end's frame loop is held by its mark (minified builds rename it); a reference built before the
  // mark is matched by its unminified name.
  await c.psend('Page.addScriptToEvaluateOnNewDocument', { source: `${SEED_SRC}${CONTEXT_SRC}const raf=requestAnimationFrame.bind(window);window.requestAnimationFrame=f=>f.bfeFrameLoop||f.name==='draw'?0:raf(f);` });
  await c.psend('Page.navigate', { url: `${process.env.BFE_APP_URL || 'app://bfe/'}?shell=1&menus=classic&screen=main&sound=0` });
  for (let i = 0; i < 300 && !(await c.ev('Boolean(window.__shell?.surface&&__shell.stack)').catch(() => false)); i++) await sleep(100);
  if (!(await c.ev('Boolean(window.__shell?.surface&&__shell.stack)'))) throw Error('The front end did not come up');
  const sub = SUBSCREENS.includes(testCase.screen);
  await c.ev(`(()=>{const s=__shell;s.stack.clear();s.stack.push(s.screens[${JSON.stringify(sub ? 'options' : testCase.screen)}]());${sub ? `const top=s.stack.top;top.widget('options').rows.find(r=>r.id===${JSON.stringify(testCase.screen)}).action(top);` : ''}return 1;})()`);
  for (let i = 0; i < 15; i++) { await c.ev('(()=>{const s=__shell;s.renderer.clear();s.surface.drawFrame(b=>s.stack.draw(s.surface,b));return 1;})()'); await sleep(100); }
  await c.ev('(()=>{const s=__shell;for(let i=0;i<120;i++)s.stack.update(1/60);s.stack.pulse=.75;s.stack.pointer={x:650,y:400,visible:true,held:false};return 1;})()');
  const shot = await c.ev(`(()=>{const s=__shell,R=s.renderer;R.clear();s.surface.drawFrame(b=>s.stack.draw(s.surface,b));const gl=R.getContext(),w=gl.drawingBufferWidth,h=gl.drawingBufferHeight,p=new Uint8Array(w*h*4);gl.readPixels(0,0,w,h,gl.RGBA,gl.UNSIGNED_BYTE,p);let t='';for(let i=0;i<p.length;i+=32768)t+=String.fromCharCode.apply(null,p.subarray(i,i+32768));return {w,h,data:btoa(t),glError:gl.getError(),renderer:(()=>{const e=gl.getExtension('WEBGL_debug_renderer_info');return e?gl.getParameter(e.UNMASKED_RENDERER_WEBGL):'';})()};})()`);
  const rgba = Buffer.from(shot.data, 'base64');
  result.setup = { renderer: shot.renderer };
  result.points.push({ k: 0, w: shot.w, h: shot.h, lit: litFraction(rgba), gl: { error: shot.glError }, frame: { file: store('0-presented.bin', rgba), hash: hash(rgba) } });
}

// A build may hand out its game object before the world connection is up;
// the load waits on the game's ready promise, so make that wait for it too.
const READY_SRC = `(() => { let game; Object.defineProperty(window, '__bf', { configurable: true, get() { return game; }, set(v) { game = v;
  const ready = v && v.ready; if (ready && typeof ready.then === 'function') v.ready = ready.then(async () => { for (let i = 0; i < 600 && !(v.client && v.client.ready); i++) await new Promise(r => setTimeout(r, 100)); }); } }); })();`;
const litFraction = (buf) => { let lit = 0; for (let i = 0; i < buf.length; i += 64) if (buf[i] > 8 || buf[i + 1] > 8 || buf[i + 2] > 8) lit++; return lit / (buf.length / 64); };

// Seating the player the way a player does: walk to the nearest hull and press
// E. The mouse counts and keys go through the page's own input path under
// lockstep, so the walk is the same in every session. Only hulls the player
// can reach in a straight walk count; a map where none is near the spawn
// is not applicable, rather than a failure of either build.
const SEAT_RANGE = 120;
const SEAT_SRC = `(async () => {
  const b=__bf,G=window.__gate,wrap=a=>Math.atan2(Math.sin(a),Math.cos(a)),me=b.playerUnit;
  const hulls=()=>b.vehicles.vehicles.filter(v=>v.alive&&!v.seats.some(s=>s.lease));
  const flat=v=>Math.hypot(v.position.x-me.position.x,v.position.z-me.position.z);
  const target=hulls().sort((a,z)=>(a.flying-z.flying)||flat(a)-flat(z)||(a.name<z.name?-1:1))[0];
  if(!target)return {error:'no vehicle on the map'};
  if(flat(target)>${SEAT_RANGE})return {error:'no vehicle within '+${SEAT_RANGE}+' m of the spawn',nearest:Math.round(flat(target))};
  // Find the counts-to-radians scale by turning once.
  const y0=b.soldier.aimYaw;G.look(100,0);await b.lockstep(1/60,false);
  const scale=wrap(b.soldier.aimYaw-y0)/100;if(!scale)return {error:'the view did not turn'};
  let pressed=false;
  for(let i=0;i<1500;i++){
    if(b.driver?.vehicle||me.seat?.vehicle)break;
    const dx=target.position.x-me.position.x,dz=target.position.z-me.position.z,d=Math.hypot(dx,dz);
    const error=wrap(Math.atan2(dx,dz)-b.soldier.aimYaw);
    G.look(Math.max(-250,Math.min(250,Math.round(error/scale))),0);
    G.key('KeyW',d>3);
    if(d<=3.5&&Math.abs(error)<.2){if(!pressed){G.key('KeyE',true);pressed=true;}else{G.key('KeyE',false);pressed=false;}}
    await b.lockstep(1/60,false);
  }
  G.key('KeyW',false);G.key('KeyE',false);
  for(let i=0;i<20;i++)await b.lockstep(1/60,false);
  const seat=b.driver?.vehicle||me.seat?.vehicle;
  return seat?{name:seat.name,flying:!!seat.flying,seats:seat.seats.length}:{error:'could not board '+target.name,at:Math.round(flat(target))};
})()`;
const c = await connect();
try {
  if (testCase.kind === 'shell') await captureShell(c);
  else {
    result.loaded = await loadBattle(c, { settings: testCase.settings, query: testCase.query, extraSrc: `${READY_SRC}${stages ? `(${installWorldTrace.toString()})()` : ''}` });
    if (result.loaded.error || !result.loaded.spawned) throw Error(result.loaded.error || 'player not spawned');
    await c.ev(HELPERS_SRC);
    if (stages) await c.ev('window.__worldTrace.ready=true;1');
    result.caps = await c.ev(`(() => { const g=__bf.graphics,core=g.rust; return {rust:!!core,stageApi:!!(core&&core.rust&&core.rust.post_frame&&core.rust.hud_frame)}; })()`);
    if (stages && !result.caps.stageApi) { result.stagesUnavailable = 'this build has no stage interface'; }
    const replaying = stages && result.caps.stageApi;
    if (testCase.kind === 'vehicle') {
      result.seat = await c.ev(SEAT_SRC);
      if (result.seat.error) result.notApplicable = result.seat.error;
      // The seat packet reaches the page on the next ticks.
      else await c.ev(`(async()=>{for(let i=0;i<30;i++)await __bf.lockstep(1/60,false);})()`);
    }
    if (!result.notApplicable) {
      result.setup = await c.ev(`(() => {
        const b=__bf,g=b.graphics,R=g.renderer,gl=R.getContext();
        const T=window.__corpus={want:false,stages:{}};
        const read=(name)=>{const w=gl.drawingBufferWidth,h=gl.drawingBufferHeight,buf=new Uint8Array(w*h*4);R.setRenderTarget(null);gl.readPixels(0,0,w,h,gl.RGBA,gl.UNSIGNED_BYTE,buf);
          const error=gl.getError();if(error)throw Error(name+' read GL '+error);T.stages[name]={w,h,buf};};
        const hud=b.hud.render;b.hud.render=function(...a){if(T.want)read('post');const r=hud.apply(this,a);if(T.want)read('presented');return r;};
        // Light selection reads a clock; give it the same advancing value every run.
        if(g.localLights){const begin=g.localLights.begin;let now=1e9;g.localLights.begin=function(...args){begin.apply(this,args);this.now=now+=1000;};}
        return {buffer:[gl.drawingBufferWidth,gl.drawingBufferHeight],renderer:(()=>{const e=gl.getExtension('WEBGL_debug_renderer_info');return e?gl.getParameter(e.UNMASKED_RENDERER_WEBGL):'';})()};
      })()`);
      if (replaying) {
        await c.ev(`(${installReplay.toString()})()`);
        await c.ev(`(${installWorldReplay.toString()})(${JSON.stringify(programLayouts)})`);
        await c.ev(`window.__corpus.captureReplay=${role === 'reference'};window.__corpus.role=${JSON.stringify(role === 'repeat' ? 'control' : role)}`);
      }
      const drive = 'KeyW';
      if (process.env.BFE_GATE_TRACE === '1') await c.ev('window.__trace=[];1');
      for (let k = 0; k < manifest.captures; k++) {
        const point = await c.ev(`(async () => {
          const b=__bf,g=b.graphics,G=window.__gate,T=window.__corpus,gl=g.renderer.getContext();
          G.key(${JSON.stringify(drive)},true);${testCase.kind === 'foot' ? "window.dispatchEvent(new MouseEvent('mousedown',{button:0,bubbles:true}));" : ''}
          for(let i=0;i<${manifest.gap};i++){const f=${k}*${manifest.gap}+i;if(f%90===0)G.key('KeyA',true);if(f%90===45)G.key('KeyA',false);G.look(Math.round(9*Math.sin(f*.07)),Math.round(3*Math.cos(f*.05)));await b.lockstep(1/60,true);if(window.__trace)window.__trace.push([b.worldClock.tick,+b.playerUnit.position.x.toFixed(4),+b.playerUnit.position.z.toFixed(4),+b.soldier.aimYaw.toFixed(4),Math.round(performance.now()),b.units.all().filter(u=>u.alive&&u.position).map(u=>[u.id,+u.position.x.toFixed(3),+u.position.z.toFixed(3)]),b.vehicles.vehicles.filter(v=>v.alive).map(v=>[v.name,+v.position.x.toFixed(3),+v.position.z.toFixed(3),+v.yaw.toFixed(3)])]);}
          ${testCase.kind === 'foot' ? "window.dispatchEvent(new MouseEvent('mouseup',{button:0,bubbles:true}));" : ''}G.key(${JSON.stringify(drive)},false);G.key('KeyA',false);
          for(const w of g.cascadeWindows||[])w.invalidate();
          for(let i=0;i<${manifest.settleFrames};i++)await b.lockstep(0,true);
          g.upscaler?.invalidate?.();if(g.upscaling==='temporal')g.upscaler.frame=${k};
          for(let i=0;i<${manifest.historyFrames};i++)await b.lockstep(0,true);
          G.resetFrame();T.stages={};T.records=[];T.blobs={};T.want=true;const tick=await b.lockstep(0,true);T.want=false;
          const s=T.stages.presented;if(!s)throw Error('HUD did not draw');
          let lit=0;for(let i=0;i<s.buf.length;i+=64)if(s.buf[i]>8||s.buf[i+1]>8||s.buf[i+2]>8)lit++;
          const trace=window.__trace?window.__trace.splice(0):undefined;
          return {k:${k},trace,tick,state:${STATE_SRC},lit:lit/(s.buf.length/64),w:s.w,h:s.h,rustDraws:G.rustDraws(),errors:g.rust?[...g.rust.errors]:[],
            gl:{...${GL_STATE},error:gl.getError()}};
        })()`);
        point.frame = { file: store(`${k}-presented.bin`, Buffer.from(await pageBytes(c, 'new Uint8Array(window.__corpus.stages.presented.buf.buffer)'), 'base64')) };
        point.frame.hash = hash(fs.readFileSync(point.frame.file));
        point.post = { file: store(`${k}-post.bin`, Buffer.from(await pageBytes(c, 'new Uint8Array(window.__corpus.stages.post.buf.buffer)'), 'base64')) };
        if (replaying && role === 'reference') {
          const payload = await c.ev(`({records:window.__corpus.records,blobs:Object.fromEntries(Object.entries(window.__corpus.blobs).map(([key,buf])=>[key,{type:buf.constructor.name,bytes:buf.byteLength}]))})`);
          for (const [key, blob] of Object.entries(payload.blobs)) {
            blob.file = `${rawDir}/${k}-replay-${key}.bin`;
            const fd = fs.openSync(blob.file, 'w');
            try { for (let offset = 0; offset < blob.bytes; offset += 1048576) fs.writeSync(fd, Buffer.from(await pageBytes(c, `new Uint8Array(window.__corpus.blobs[${JSON.stringify(key)}].buffer,window.__corpus.blobs[${JSON.stringify(key)}].byteOffset+${offset},Math.min(1048576,window.__corpus.blobs[${JSON.stringify(key)}].byteLength-${offset}))`), 'base64')); }
            finally { fs.closeSync(fd); }
          }
          point.replayFile = `${rawDir}/${k}-replay.json`; fs.writeFileSync(point.replayFile, JSON.stringify(payload));
          point.selfReplay = await c.ev('window.__corpus.replay(window.__corpus.records,window.__corpus.blobs)');
        } else if (replaying) {
          const payload = JSON.parse(fs.readFileSync(`${raw}/reference/${k}-replay.json`));
          await c.ev(`window.__corpus.payload={records:${JSON.stringify(payload.records)},blobs:{}};1`);
          for (const [key, blob] of Object.entries(payload.blobs)) {
            const data = fs.readFileSync(blob.file);
            await c.ev(`window.__corpus.payload.blobs[${JSON.stringify(key)}]=new window[${JSON.stringify(blob.type)}](new ArrayBuffer(${data.length}));1`);
            for (let offset = 0; offset < data.length; offset += 1048576) {
              const chunk = data.subarray(offset, offset + 1048576).toString('base64');
              await c.ev(`(() => {const r=atob(${JSON.stringify(chunk)}),b=window.__corpus.payload.blobs[${JSON.stringify(key)}],bytes=new Uint8Array(b.buffer);for(let i=0;i<r.length;i++)bytes[${offset}+i]=r.charCodeAt(i);return 1;})()`);
            }
          }
          point.replay = await c.ev('window.__corpus.replay(window.__corpus.payload.records,window.__corpus.payload.blobs)');
          await c.ev('window.__corpus.payload=null;1');
        }
        result.points.push(point);
        console.log(JSON.stringify({ role, k, tick: point.tick, replay: (point.replay || point.selfReplay || []).length, errors: point.errors, glError: point.gl.error }));
      }
      const log = process.env.BFE_GPU_LOG;
      result.gpuDeaths = log && fs.existsSync(log) ? fs.readFileSync(log, 'utf8').split('\n').filter(l => /GPU process exited|GPU process isn't usable|GPU process crashed|Lost UI shared context|render-process-gone/i.test(l)) : [];
    }
  }
  result.valid = result.notApplicable ? true : result.points.length === (testCase.kind === 'shell' ? 1 : manifest.captures)
    && !(result.gpuDeaths || []).length
    && result.points.every(p => p.lit >= .05 && p.w === 1600 && p.h === 900 || testCase.kind === 'shell')
    && result.points.every(p => !p.gl || (!p.gl.lostEvents && !p.gl.contextLost && !p.gl.error))
    && (!result.caps?.rust || result.points.every(p => p.rustDraws > 0 && (role !== 'candidate' || !p.errors.length)));
} catch (error) {
  result.error = String(error.stack || error); result.valid = false;
} finally {
  result.console = c.consoleErrors.slice(0, 30);
  fs.writeFileSync(`${dir}/capture.json`, JSON.stringify(result, null, 2));
  c.close();
}
process.exitCode = result.valid ? 0 : 2;
