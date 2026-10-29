// Paired map views: missing effects (before), restored Classic, and Enhanced.
// Each frame includes reflections and a one-pixel GPU completion readback.
import fs from 'node:fs/promises';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const frames=Number(process.env.ENV_BENCH_FRAMES||18),width=480,height=300;
const browser=await chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
const results=[];
try{
 for(const map of (process.env.ENV_BENCH_MAPS||'nab2,bes2').split(',')){
  const page=await browser.newPage({viewport:{width,height}}),errors=[];
  page.on('pageerror',e=>{errors.push(e.message);console.error(e.message);});page.on('console',m=>{if(m.type()==='error')errors.push(m.text())});
  await page.route('**/@vite/client',r=>r.fulfill({contentType:'application/javascript',body:''}));
  await page.addInitScript(()=>{
   const raf=requestAnimationFrame.bind(window);window.requestAnimationFrame=fn=>fn.name==='loop'?0:raf(fn);
   let seed=1347,uuid=42;Math.random=()=>new Error().stack.includes('generateUUID')?((uuid=Math.imul(uuid,1664525)+1013904223>>>0)/4294967296):((seed=Math.imul(seed,1664525)+1013904223>>>0)/4294967296);
  });
  await page.goto(`${process.env.RENDER_TEST_URL||'http://127.0.0.1:18457'}/?map=${map}&side=rep&unit=rep_inf_clone_trooper&spawn=CP3`,{waitUntil:"domcontentloaded",timeout:120000});
  await page.waitForFunction(()=>window.__bf,null,{timeout:120000});
  await page.evaluate(async map=>{
   const b=__bf;b.pause();await b.ready;await b.lockstep(0,false);
   let effectSeed=713;Math.random=()=>((effectSeed=Math.imul(effectSeed,1664525)+1013904223>>>0)/4294967296);
   for(const [key,value] of Object.entries({shadows:'off',antialiasing:'off',softFocus:false,bloom:true,renderScale:1,environmentEffects:'classic'}))b.settings.set(key,value);
   if(map==='nab2'){b.camera.position.set(-25,-44,123);b.camera.lookAt(-36,-46.8,111);}else{b.camera.position.set(-167,-59,-9);b.camera.lookAt(-178,-63,-20);}
   b.camera.updateMatrixWorld();b.environment.update(0,b.camera,[]);
  },map);
  const cases=(process.env.ENV_BENCH_MODES||'missing,classic,enhanced').split(',').flatMap(mode=>
    mode==='enhanced'?(process.env.ENV_VOLUME_SCALES||'default').split(',').map(scale=>({mode,scale})):[{mode,scale:'default'}]);
  for(const {mode,scale} of cases){
   console.error(`${map}: ${mode}, volume scale ${scale}`);
   if(scale!=='default')await page.evaluate(scale=>{__bf.environment.volume.scale=scale;},Number(scale));
   const result=await page.evaluate(({mode,frames})=>{
    const b=__bf,e=b.environment,g=b.graphics,r=b.renderer,gl=r.getContext(),pixel=new Uint8Array(4);
    b.settings.set('environmentEffects',mode==='enhanced'?'enhanced':'classic');e.update(0,b.camera,[]);
    e.group.visible=mode!=='missing';e.water?.setEnabled(mode==='enhanced');
    if(mode!=='missing')for(let i=0;i<100;i++)e.update(.05,b.camera,[]);
    const complete=[],cpu=[],updates=[],draws=[],reflectionDraws=[];
    function frame(record){
     const start=performance.now();if(mode!=='missing')e.update(1/60,b.camera,[]);const updated=performance.now();
     b.water?.reflect(b.camera);const calls=r.info.render.calls;
     if(mode!=='missing')e.reflect(b.camera,b.water);const reflected=r.info.render.calls-calls;
     g.render(6);const submitted=performance.now();gl.readPixels(0,0,1,1,gl.RGBA,gl.UNSIGNED_BYTE,pixel);
     if(record){complete.push(performance.now()-start);cpu.push(submitted-start);updates.push(updated-start);draws.push(g.frameInfo.calls+reflected);reflectionDraws.push(reflected);}
    }
    for(let i=0;i<6;i++)frame(false);for(let i=0;i<frames;i++)frame(true);
    if(gl.getError())throw Error('WebGL benchmark error');
    const summarize=values=>{const v=[...values].sort((a,b)=>a-b);return{medianMs:v[Math.floor(v.length/2)],p95Ms:v[Math.ceil(v.length*.95)-1]};};
    const debug=gl.getExtension('WEBGL_debug_renderer_info');
    return{mode,volumeScale:e.volume.scale,frames,resolution:[480,300],gpu:debug?gl.getParameter(debug.UNMASKED_RENDERER_WEBGL):gl.getParameter(gl.RENDERER),
      complete:summarize(complete),submit:summarize(cpu),update:summarize(updates),draws:{min:Math.min(...draws),max:Math.max(...draws)},
      reflectionFrames:reflectionDraws.filter(n=>n>0).length,reflectionDraws:Math.max(...reflectionDraws),sources:mode==='missing'?{activeSources:0,volumes:0,classicParticles:0}:{...e.stats}};
   },{mode,frames});
   results.push({map,...result});console.log(JSON.stringify({map,...result}));
  }
  // Turning away must cull emission and enhanced volume work completely.
  const culled=await page.evaluate(()=>{const b=__bf;b.camera.position.set(1000,1000,1000);b.environment.update(.1,b.camera,[]);return{...b.environment.stats};});
  if(culled.activeSources||culled.volumes||culled.classicParticles)throw Error(`Culling failure ${JSON.stringify(culled)}`);
  if(errors.length)throw Error(errors.join('\n'));await page.close();
 }
}finally{
 await browser.close();if(process.env.ENV_BENCH_OUTPUT)await fs.writeFile(process.env.ENV_BENCH_OUTPUT,JSON.stringify(results,null,2));
}
