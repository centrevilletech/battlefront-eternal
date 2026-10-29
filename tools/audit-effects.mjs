// Controlled cosmetic CPU/transfer audit. Excludes rendering and GPU execution.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
try {
 const page=await browser.newPage();
 await page.route('**/__effects_audit__',r=>r.fulfill({contentType:'text/html',body:'<body></body>'}));
 await page.goto((process.env.RUNTIME_AUDIT_URL||'http://127.0.0.1:18449')+'/__effects_audit__');
 const result=await page.evaluate(async()=>{
  const T=await import('/node_modules/three/build/three.module.js');
  const {Effects}=await import('/src/effects.js'),{emitter,seeded}=await import('/tests/fixtures/effects.js');
  const texture=new T.DataTexture(new Uint8Array([255,255,255,255]),1,1);
  const long=emitter({burstCount:[128,128],maxParticles:[128,128]});long.transformer.lifetime=30;
  const churn=emitter({burstCount:[4,4],maxParticles:[-1,-1],burstDelay:[.04,.04]});
  const chamber=await fetch('/maps/bes2/environment/effects.json').then(r=>r.ok?r.json():null);
  const cases=[{name:'5120 persistent smoke sprites',defs:{smoke:[long]},sources:Array(40).fill('smoke'),warmup:0},
    {name:'eight continuous emitters',defs:{smoke:[churn]},sources:Array(8).fill('smoke'),warmup:3}];
  if(chamber)cases.push({name:'nine authored chamber sources',defs:chamber.effects,sources:chamber.instances.map(i=>i.effect),warmup:4});
  const results=[];
  for(const scenario of cases) {
    const entries=[false,true].map(gpu=>{
      const fx=new Effects(new T.Scene(),scenario.defs,{texture:()=>texture,gpu});
      seeded(()=>{for(const name of scenario.sources)fx.play(name,new T.Vector3());fx.update(scenario.warmup);});
      return {fx,gpu,samples:[],bytes:0,particles:0,versions:new WeakMap()};
    });
    for(let i=0;i<100;i++)for(const e of entries)e.fx.update(0);
    for(let frame=0;frame<480;frame++)for(const e of frame%2?[entries[1],entries[0]]:entries) {
      const {fx,versions}=e;
      const start=performance.now();seeded(()=>fx.update(1/144),frame+1);e.samples.push(performance.now()-start);
      for(const batch of fx.renderer.batches.values())for(const a of Object.values(batch.geometry.attributes)){
        if(versions.get(a)!==a.version&&a.isInstancedBufferAttribute){
          e.bytes+=a.updateRanges.length?a.updateRanges.reduce((sum,r)=>sum+r.count*4,0):a.array.byteLength;
          versions.set(a,a.version);a.clearUpdateRanges();
        }
      }
      if(fx.gpu)e.bytes+=fx.gpu.data.byteLength;
      e.particles+=fx.effects.reduce((n,e)=>n+e.emitters.reduce((m,em)=>m+em.particles.length,0),0);
    }
    for(const {fx,gpu,samples,bytes,particles} of entries) {
      samples.sort((a,b)=>a-b);
      results.push({scenario:scenario.name,gpu,frames:samples.length,meanParticles:particles/samples.length,
        p50Ms:samples[Math.floor(samples.length*.5)],p95Ms:samples[Math.floor(samples.length*.95)],
        totalMs:samples.reduce((a,b)=>a+b,0),uploadBytesPerFrame:bytes/samples.length,batches:fx.renderer.batches.size});
      fx.gpu?.dispose();
    }
  }
  return {note:'CPU update and requested upload bytes only; alternating CPU/GPU order each frame; no GPU/FPS claim. Synthetic texture, authored chamber spawn/track data.',results};
 });
 for(let i=0;i<result.results.length;i+=2)assert.equal(result.results[i].meanParticles,result.results[i+1].meanParticles);
 await fs.writeFile(process.env.EFFECTS_AUDIT_OUTPUT||'/tmp/bf-effects-audit.json',JSON.stringify(result,null,2));
 console.log(JSON.stringify(result,null,2));
}finally{await browser.close();}
