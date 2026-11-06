// Loading diagnostics for a calibrated local machine. Farm timings are not a
// performance gate. Run in fresh applications with cold/warm cache noted.
import fs from 'node:fs';
import { expand, readManifest } from './manifest.mjs';
import { programLayouts } from '../../src/render-programs.js';
import { connect, loadBattle } from './battle.mjs';
const manifest = expand(readManifest());
const testCase = manifest.cases.find(c => c.id === (process.env.BFE_CORPUS_CASE || 'classic-nab2'));
const extraSrc = `(() => {
 const T=window.__shaderLoad={methods:{},tasks:[],sources:0,uniqueSources:0,sourceBytes:0};
 const sources=new Set();
 new PerformanceObserver(list=>{for(const t of list.getEntries())T.tasks.push({start:t.startTime,ms:t.duration});}).observe({type:'longtask',buffered:true});
 for(const name of ['shaderSource','compileShader','linkProgram','getShaderParameter','getProgramParameter','getActiveUniform','getActiveAttrib','getUniformLocation','drawArrays','drawElements','drawArraysInstanced','drawElementsInstanced']) {
   const proto=WebGL2RenderingContext.prototype,original=proto[name];
   proto[name]=function(...args){const start=performance.now();try{
     if(name==='shaderSource'){T.sources++;T.sourceBytes+=args[1].length;sources.add(args[1]);T.uniqueSources=sources.size;}
     return original.apply(this,args);
   } finally {const dt=performance.now()-start,m=T.methods[name] ||= {count:0,ms:0,maxMs:0};m.count++;m.ms+=dt;m.maxMs=Math.max(dt,m.maxMs);}};
 }
})()`;
const c = await connect();
try {
  const loaded = await loadBattle(c, { settings: testCase.settings, query: testCase.query, extraSrc });
  const metrics = await c.ev(`({coverage:(() => {const g=__bf.graphics, R=g.renderer,materials=[g.downsample.material,g.composite.material,g.fxaa.material,g.fxaaFast?.material,g.bloom.extract.material,g.bloom.blur.material,...[g.upscaler.motionPass,g.upscaler.resolvePass,g.upscaler.sharpenPass,g.upscaler.spatialPass,g.upscaler.bilinearPass].map(p=>p.material),...__bf.hud.batch.mesh.material].filter(Boolean);const rows=materials.map(m=>({key:m.userData.bfeProgram,three:!!R.properties.get(m).currentProgram,owned:g.rust.ownedPrograms?.has(m.userData.bfeProgram)||false}));for(const key of ${JSON.stringify(Object.keys(programLayouts).filter(k=>k.startsWith('effects/')))})rows.push({key,owned:g.rust.ownedPrograms?.has(key)||false});return rows;})(),load:window.__shaderLoad, programs:__bf.graphics.rust.shaderPrograms || [],enabled:__bf.graphics.rust.enabled,errors:__bf.graphics.rust.errors})`);
  const firstUse = await c.ev(`(async()=>{
    const b=__bf,g=b.graphics,T=window.__shaderLoad;
    const counts=()=>Object.fromEntries(Object.entries(T.methods).map(([name,m])=>[name,m.count]));
    const before=counts(),programs=g.rust.shaderPrograms.length;
    const render=g.render,frameMs=[];
    g.render=function(...args){const start=performance.now();try{return render.apply(this,args);}finally{frameMs.push(performance.now()-start);}};
    try {for(let i=0;i<60;i++)await b.lockstep(1/60,true);} finally {g.render=render;}
    const sorted=frameMs.slice().sort((a,b)=>a-b);
    const renderSubmission={frames:frameMs.length,meanMs:frameMs.reduce((a,b)=>a+b,0)/frameMs.length,p50Ms:sorted[Math.floor(sorted.length*.5)],p95Ms:sorted[Math.floor(sorted.length*.95)],maxMs:sorted.at(-1)};
    const after=counts();
    return {frames:60,renderSubmission,methods:Object.fromEntries(Object.keys(after).map(n=>[n,after[n]-(before[n]||0)])),ownedPrograms:g.rust.shaderPrograms.slice(programs)};
  })()`);
  const cacheProbe = process.env.BFE_SHADER_CACHE_PROBE ? await c.ev(`(() => {
    const g=__bf.graphics,r=g.rust.rust,rows=[];
    for(const key of ${JSON.stringify(Object.keys(programLayouts))}) {
      let start=performance.now();const first=r.add_owned_program(key),firstMs=performance.now()-start;
      start=performance.now();const second=r.add_owned_program(key),cachedMs=performance.now()-start;
      rows.push({key,firstMs,cachedMs,same:first===second});
    }
    g.renderer.resetState();return {rows,error:g.renderer.getContext().getError()};
  })()`) : null;
  const result = { environment: { node: process.version, warm: Number(process.env.WARM ?? 12), renderWarm: Number(process.env.RENDER_WARM ?? 20), drawsSuppressed: !!process.env.EMU_NODRAW, settings: testCase.settings }, cacheProbe, firstUse, case: testCase.id, loaded, ...metrics };
  fs.writeFileSync(`${process.env.OUT}/${process.env.LABEL}.json`, JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
  if (loaded.error || !loaded.spawned || !metrics.enabled) process.exitCode = 1;
} finally { c.close(); }
