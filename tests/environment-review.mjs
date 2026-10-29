// Scripted motion review and paired measurements. Serves the current workspace
// and an optional frozen baseline inside this process; no background Vite needed.
// The world is never stepped (tests/battle-page.mjs opens it): the page's copy
// of the soldier is moved through the fountain for its environment to answer.
import fs from 'node:fs/promises';
import {createServer} from 'vite';
import {launchBrowser,openBattle} from './battle-page.mjs';
const artifactDir=process.env.ENV_ARTIFACTS||'/tmp/environment-review';
const roots=process.env.ENV_BEFORE_ROOT?[process.env.ENV_BEFORE_ROOT,process.cwd()]:[process.cwd()];
const servers=[],results=[];let browser;
const frames=Number(process.env.ENV_REVIEW_FRAMES||36),width=640,height=400;
try{
 for(let i=0;i<roots.length;i++){
  const server=await createServer({root:roots[i],configFile:false,cacheDir:`/tmp/environment-review-cache-${i}`,optimizeDeps:{entries:['index.html']},server:{watch:null,port:18461+i,host:'127.0.0.1',strictPort:true}});
  await server.listen();servers.push(server);
 }
 browser=await launchBrowser();
 for(const map of (process.env.ENV_REVIEW_MAPS||'nab2,bes2').split(',')){
  const run=[];
  for(let index=0;index<roots.length;index++){
   const name=roots.length===1?'after':index===0?'before':'after';
   // A baseline from before every page ran the world worker opts in to it
   // with worker=1 (ignored since).
   const battle=await openBattle(browser,{base:`http://127.0.0.1:${18461+index}`,query:`map=${map}&side=rep&unit=rep_inf_clone_trooper&spawn=CP3&shadows=off${name==='before'?'&worker=1':''}`,
    viewport:{width,height},settings:null,beforeLoad:page=>{page.setDefaultNavigationTimeout(120000);
     return page.addInitScript(()=>{let a=917,b=17;Math.random=()=>new Error().stack.includes('generateUUID')?((b=Math.imul(b,1664525)+1013904223>>>0)/4294967296):((a=Math.imul(a,1664525)+1013904223>>>0)/4294967296);});}});
   const {page,errors}=battle;
   await page.evaluate(async map=>{
    const b=__bf;if(b.lockstep)await b.lockstep(0,false);else b.frame(0,false);
    for(const [key,value] of Object.entries({environmentEffects:'enhanced',shadows:'off',antialiasing:'4x',softFocus:false,bloom:true}))b.settings.set(key,value);
    const c=b.camera;if(map==='nab2'){c.position.set(-25,-44,123);c.lookAt(-36,-46.8,111);}else{c.position.set(-167,-59,-9);c.lookAt(-178,-63,-20);}c.updateMatrixWorld();
    for(let j=0;j<60;j++)b.environment.update(.05,c,[]);
    if(map==='bes2'){const hit={point:new b.THREE.Vector3(),normal:new b.THREE.Vector3()};
     window.reviewFloor=b.collision.cast(new b.THREE.Vector3(-178,-56,-16),new b.THREE.Vector3(0,-1,0),30,hit)?.point.y;console.log('Review chamber floor',window.reviewFloor);}
    b.actor.root.visible=true;b.actor.body.visible=true;b.actor.body.layers.set(0);
    window.review={map,pixel:new Uint8Array(4),gl:b.renderer.getContext()};
   },map);
   await fs.mkdir(`${artifactDir}/${map}-${name}`,{recursive:true});
   run.push({battle,page,name,errors,samples:[]});
  }
  // Alternate A/B frames to reduce drift from other work on this machine.
  for(let frame=0;frame<frames;frame++){
   for(const item of frame%2?[...run].reverse():run){
    const sample=await item.page.evaluate(({frame,frames})=>{
     const b=__bf,e=b.environment,t=frame/12,progress=frame/Math.max(1,frames-1),jump=Math.sin(Math.min(1,Math.max(0,(progress-.2)/.55))*Math.PI)*1.8;
     if(review.map==='nab2')b.soldier.position.set(-38+progress*4,-49.8+jump,112);
     else b.soldier.position.set(-180.5+progress*4,(window.reviewFloor??-61.8)+jump,-19.5);
     b.actor.root.position.copy(b.soldier.position);b.actor.root.rotation.y=Math.PI*.5;b.actor.root.updateMatrixWorld(true);
     const start=performance.now();e.update(1/12,b.camera,[b.soldier]);const update=performance.now()-start;
     b.water?.reflect(b.camera);const calls=b.renderer.info.render.calls;e.reflect(b.camera,b.water);const mirror=b.renderer.info.render.calls-calls;
     b.graphics.render(e.time);review.gl.readPixels(0,0,1,1,review.gl.RGBA,review.gl.UNSIGNED_BYTE,review.pixel);
     const completed=performance.now()-start,error=review.gl.getError();
     return{update,completed,draws:b.graphics.frameInfo.calls+mirror,error,png:b.renderer.domElement.toDataURL()};
    },{frame,frames});
    const {png,...metrics}=sample;item.samples.push(metrics);
    await fs.writeFile(`${artifactDir}/${map}-${item.name}/${String(frame).padStart(3,'0')}.png`,Buffer.from(png.split(',')[1],'base64'));
   }
   if(frame%12===0)console.error(`${map}: ${frame}/${frames} frames`);
  }
  for(const item of run){
   if(item.errors.length||item.samples.some(s=>s.error))throw Error(JSON.stringify({name:item.name,errors:item.errors}));
   const median=key=>{const sorted=item.samples.slice(6).map(s=>s[key]).sort((a,b)=>a-b);return sorted[Math.floor(sorted.length/2)];};
   const result={map,version:item.name,frames,width,height,completedMedianMs:median('completed'),updateMedianMs:median('update'),draws:[Math.min(...item.samples.map(s=>s.draws)),Math.max(...item.samples.map(s=>s.draws))],samples:item.samples};
   results.push(result);console.log(JSON.stringify({...result,samples:undefined}));await item.battle.close();
  }
 }
 await fs.writeFile(`${artifactDir}/measurements.json`,JSON.stringify(results,null,2));
}finally{await browser?.close();for(const s of servers)await s.close();}
