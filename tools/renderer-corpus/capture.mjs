// Capture one seeded Rust battle in a fresh session. Cross-build comparison is
// performed by renderer-corpus.mjs; no renderer can be disabled by this tool.
import fs from 'node:fs';
import { gzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import { expand } from './manifest.mjs';
import { connect, loadBattle, HELPERS_SRC } from './battle.mjs';
const manifest = expand(JSON.parse(fs.readFileSync(new URL('./manifest.json', import.meta.url))));
const id = process.env.BFE_CORPUS_CASE;
const testCase = manifest.cases.find(c => c.id === id);
if (!testCase) throw Error(`Unknown corpus case ${id}`);
const out = process.env.OUT || 'renderer-corpus-out';
fs.mkdirSync(out, { recursive: true });
const report = { id, pass: false, complete: false, points: [] };
const c = await connect();
try {
  report.loaded = await loadBattle(c, { settings: testCase.settings, query: testCase.query });
  if (!report.loaded.spawned) throw Error(`Player not spawned: ${JSON.stringify(report.loaded)}`);
  // The UUID stream must stay separate from visual randomness. Minified stack
  // names cannot identify that stream reliably, so reject such a capture build.
  // This restriction belongs to the harness, not to the player's renderer.
  report.random=await c.ev('__gateRandom');
  if(!report.random?.uuidCalls)throw Error('Capture builds must retain generateUUID names (minify: false)');
  await c.ev(HELPERS_SRC);
  await c.ev(`(() => {
    const b=__bf,g=b.graphics,R=b.renderer,gl=R.getContext();
    const T=window.__capture={want:false,shot:null};
    const render=b.hud.render;
    b.hud.render=function(...args){const value=render.apply(this,args);if(T.want){
      const w=gl.drawingBufferWidth,h=gl.drawingBufferHeight,buf=new Uint8Array(w*h*4);
      R.setRenderTarget(null);gl.readPixels(0,0,w,h,gl.RGBA,gl.UNSIGNED_BYTE,buf);T.shot={w,h,buf};
    }return value;};
    if(g.localLights){const begin=g.localLights.begin;let now=1e9;g.localLights.begin=function(...args){begin.apply(this,args);this.now=now+=1000;};}
    return 1;
  })()`);
  if (testCase.camera === 'water') await c.ev(`(() => {
    const b=__bf,g=b.graphics,water=g.scene.userData.water,T=__capture,V=b.THREE.Vector3;
    if(!water || water.ocean)throw Error('No planar mirror water');
    T.place=k=>{const eye=g.camera.position,cells=[...water.cellBounds];cells.sort((a,z)=>((a.min.x+a.max.x)/2-eye.x)**2+((a.min.z+a.max.z)/2-eye.z)**2-((z.min.x+z.max.x)/2-eye.x)**2-((z.min.z+z.max.z)/2-eye.z)**2);const box=cells[k%cells.length];if(!box)throw Error('No water cells');const x=(box.min.x+box.max.x)/2,z=(box.min.z+box.max.z)/2,angle=.9+1.7*k;T.pose={eye:new V(x+Math.cos(angle)*14,water.level+5,z+Math.sin(angle)*14),at:new V(x,water.level,z)};};
    const aim=c=>{if(T.pose){c.position.copy(T.pose.eye);c.lookAt(T.pose.at);c.updateMatrixWorld();}};
    const reflect=water.reflect;water.reflect=function(c){aim(c);this.reflectFrames=0;return reflect.call(this,c);};
    const render=g.render;g.render=function(...a){aim(g.camera);return render.apply(this,a);};return 1;
  })()`);
  // Traverse hidden materials as well as visible ones. Ownership cannot be
  // proved by one camera simply missing an unsupported stencil/blend state.
  // Alpha-to-coverage is recorded separately because Rust supports it.
  report.materials = await c.ev(`(() => {
    const b=__bf,g=b.graphics,seen=new Set(),bad=[];let count=0,alphaToCoverage=0;
    for(const scene of [g.scene,g.farScene,g.viewScene,b.hud.scene])scene.traverse(o=>{
      for(const m of [].concat(o.material||[])){if(seen.has(m))continue;seen.add(m);count++;if(m.alphaToCoverage)alphaToCoverage++;
        const factors=[m.blendSrc,m.blendDst,m.blendSrcAlpha,m.blendDstAlpha];
        if(m.stencilWrite||m.polygonOffset||m.transmission>0||factors.some(f=>[211,212,213,214].includes(f)))bad.push({object:o.name,material:m.name,type:m.type,stencil:m.stencilWrite,offset:m.polygonOffset,factors});
      }
    });return {count,alphaToCoverage,bad};
  })()`);
  if(report.materials.bad.length)throw Error(`Unexpected material states: ${JSON.stringify(report.materials.bad)}`);
  if(testCase.contextLoss && process.env.BFE_FORCE_RESTORE === '1') {
    report.restore = await c.ev(`(async()=>{
      const b=__bf,g=b.graphics,canvas=b.renderer.domElement,gl=b.renderer.getContext(),ext=gl.getExtension('WEBGL_lose_context');
      if(!ext)throw Error('No context loss extension');
      const once=name=>new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error(name+' timeout')),30000);canvas.addEventListener(name,()=>{clearTimeout(timer);resolve();},{once:true});});
      const lost=once('webglcontextlost');ext.loseContext();await lost;
      // Chromium completes loss dispatch before restoration becomes legal.
      // Resume in a later task, as the generated driver fixture does.
      await new Promise(resolve=>setTimeout(resolve,100));
      const restored=once('webglcontextrestored');ext.restoreContext();await restored;
      if(g.rust.contextLost||g.rust.failure)throw g.rust.failure||Error('Rust did not rebuild');
      // Three resets this diagnostic counter while recreating its context.
      // Match the retained Rust frame phase before comparing restored draws.
      b.renderer.info.render.frame=g.frameCount;
      return {lost:window.__gateGl.lost,restored:window.__gateGl.restored};
    })()`);
  }
  // Advance authority to the manifest's fixed ticks, then finish the required
  // warm draws before sampling. Session wall time is not a substitute for the
  // simulation tick: loading and GPU compilation vary across fresh launches.
  for(let k=0;k<manifest.captures;k++){
    await c.ev(`(async()=>{const b=__bf,g=b.graphics,G=__gate;G.key('KeyW',true);window.dispatchEvent(new MouseEvent('mousedown',{button:0,bubbles:true}));
      for(let i=0;i<${manifest.gap};i++){const f=${k}*${manifest.gap}+i;if(f%90===0)G.key('KeyA',true);if(f%90===45)G.key('KeyA',false);G.look(Math.round(9*Math.sin(f*.07)),Math.round(3*Math.cos(f*.05)));await b.lockstep(1/60,true);}
      window.dispatchEvent(new MouseEvent('mouseup',{button:0,bubbles:true}));G.key('KeyW',false);G.key('KeyA',false);__capture.place?.(${k});
      for(const w of g.cascadeWindows||[])w.invalidate();
      for(let i=0;i<${manifest.settleFrames||2};i++)await b.lockstep(0,true);
      g.upscaler?.invalidate();if(g.upscaling==='temporal')g.upscaler.frame=${k};
      for(let i=0;i<${manifest.historyFrames};i++)await b.lockstep(0,true);
    })()`);
    const raw=await c.ev(`(async()=>{const b=__bf,g=b.graphics,R=b.renderer,gl=R.getContext(),T=__capture;__gate.resetFrame();T.want=true;T.shot=null;const tick=await b.lockstep(0,true);T.want=false;
      if(!T.shot)throw Error('No drawn HUD capture');const {w,h,buf}=T.shot;let text='';for(let i=0;i<buf.length;i+=32768)text+=String.fromCharCode.apply(null,buf.subarray(i,i+32768));
      return {w,h,data:btoa(text),tick:b.worldClock.tick,renderFrame:g.frameCount,rendererFrame:R.info.render.frame,draws:__gate.rustDraws(),errors:g.rust.errors,battleThreeCalls:R.battleThreeCalls??null,contextLost:gl.isContextLost(),error:gl.getError(),events:window.__gateGl};})()`);
    // Read the finished target, including alpha. Retain a content hash in the
    // metadata so recorded measurements remain identifiable after pixel cleanup.
    // Compressed pixels are only an intermediate comparison artifact.
    const pixels=Buffer.from(raw.data,'base64');delete raw.data;
    let lit=0;for(let i=0;i<pixels.length;i+=64)if(pixels[i]>8||pixels[i+1]>8||pixels[i+2]>8)lit++;
    raw.lit=lit/(pixels.length/64);
    if(raw.contextLost||raw.error||raw.draws<=0||raw.lit<.2||raw.battleThreeCalls>0||raw.events.lost!==(report.restore?1:0)||raw.events.restored!==(report.restore?1:0))throw Error(`Invalid frame ${k}: ${JSON.stringify(raw)}`);
    // Old reference has a known policy diagnostic for depth-only crowd casters.
    // Candidate captures must have no renderer errors whatsoever.
    if(process.env.BFE_CANDIDATE==='1'&&(raw.errors.length||raw.battleThreeCalls!==0))throw Error(`Rust errors: ${JSON.stringify(raw.errors)}`);
    if(process.env.BFE_CANDIDATE!=='1'&&raw.errors.some(e=>!/^p[0-9]+ diffuse$/.test(e)))throw Error(`Unexpected reference errors: ${JSON.stringify(raw.errors)}`);
    raw.file=`${id}-${k}.rgba.gz`;raw.hash=createHash('sha256').update(pixels).digest('hex');
    fs.writeFileSync(`${out}/${raw.file}`,gzipSync(pixels));report.points.push(raw);
  }
  // A GPU-process death can yield a nonblank stale image. Check the host log
  // in addition to GL/context state so such a capture cannot become a false pass.
  const gpuLog=process.env.BFE_GPU_LOG ? fs.readFileSync(process.env.BFE_GPU_LOG,'utf8') : '';
  report.gpuDeaths=(gpuLog.match(/GPU process exited unexpectedly|GPU process isn't usable|render-process-gone/g)||[]).length;
  if(report.gpuDeaths)throw Error(`GPU process deaths: ${report.gpuDeaths}`);
// Retain registration identities for source-ownership audits after cleanup.
// These are diagnostics, not another rendering implementation or pixel baseline.
  report.shaderPrograms=await c.ev('__bf.graphics.rust.shaderPrograms || []');
  report.complete=report.points.length===manifest.captures;report.pass=report.complete;
// Write failure metadata even when loading or readback throws. The outer
// runner needs a concrete failed case rather than an apparently missing frame.
}catch(error){
  report.error=String(error.stack||error);process.exitCode=1;
  try{report.context=await c.ev(`(()=>{const g=__bf.graphics;return {events:window.__gateGl,lost:g.renderer.getContext().isContextLost(),rustLost:g.rust.contextLost,failure:g.rust.failure?.message};})()`);}catch{}
}finally{report.console=c.consoleErrors;
  c.close();fs.writeFileSync(`${out}/${id}.json`,JSON.stringify(report,null,2));console.log(JSON.stringify({id,pass:report.pass,error:report.error}));}
