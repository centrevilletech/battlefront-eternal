// Loading diagnostics for a calibrated local machine. Farm timings are not a
// performance gate. Run in fresh applications with cold/warm cache noted.
import fs from 'node:fs';
import { expand } from './manifest.mjs';
import { connect, loadBattle } from './battle.mjs';
const manifest = expand(JSON.parse(fs.readFileSync(new URL('./manifest.json', import.meta.url))));
const testCase = manifest.cases.find(c => c.id === (process.env.BFE_CORPUS_CASE || 'classic-nab2'));
const bridge = process.env.BFE_SHADER_BRIDGE === '1';
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
  const loaded = await loadBattle(c, { settings: testCase.settings, query: testCase.query + (bridge ? '&rustShaderBridge' : ''), extraSrc });
  const metrics = await c.ev(`({load:window.__shaderLoad, programs:__bf.graphics.rust.shaderPrograms || [],enabled:__bf.graphics.rust.enabled,errors:__bf.graphics.rust.errors})`);
  const result = { case: testCase.id, bridge, loaded, ...metrics };
  fs.writeFileSync(`${process.env.OUT}/${process.env.LABEL}.json`, JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
  if (loaded.error || !loaded.spawned || !metrics.enabled) process.exitCode = 1;
} finally { c.close(); }
