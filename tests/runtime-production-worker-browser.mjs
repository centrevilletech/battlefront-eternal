import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser=await chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
try {
 const page=await browser.newPage({viewport:{width:640,height:360}}),errors=[];
 page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(()=>{
   const raf=requestAnimationFrame.bind(window);window.requestAnimationFrame=cb=>cb.toString().includes(' fps  60 Hz')?0:raf(cb);
   localStorage.setItem('battlefront-eternal.settings',JSON.stringify({waterQuality:'low',treeDetail:'original',environmentEffects:'original',rainEffects:'original',oceanEffects:'original',antialiasing:'off',shadows:'off'}));
 });
 await page.goto((process.env.RUNTIME_PRODUCTION_URL || 'http://127.0.0.1:18452')+'/?map=nab2&unit=rep_inf_clone_trooper&ai=classic&seed=1');
 await page.waitForFunction(()=>window.__bf?.createWorkerHost,null,{timeout:120000,polling:100});
 const result=await page.evaluate(async()=>{
   __bf.pause();await __bf.ready;
   const snapshots=[],events=[],faults=[];
   const host=__bf.createWorkerHost({onSnapshot:data=>snapshots.push({tick:data[1],ack:data[2],entities:data[3]}),onEvents:batch=>events.push(...batch),onError:e=>faults.push(e.message)});
   try {
     const ready=await host.call('init',{query:location.search,settings:__bf.settings.values});
     await host.call('start');
     await host.inputAcknowledged({epoch:ready.epoch,sequence:1,tick:1,generation:ready.generation,down:['KeyW'],actions:['jump'],look:[[8,0]]});
     await new Promise(resolve=>setTimeout(resolve,300));
     await host.call('suspend');const before=await host.call('inspect');
     const suspendedInput = await host.inputAcknowledged({epoch:ready.epoch,sequence:2,tick:before.tick+1,generation:ready.generation,down:['Mouse0'],actions:['jump'],look:[]});
     await new Promise(resolve=>setTimeout(resolve,80));const after=await host.call('inspect');
     return {before,after,suspendedInput,snapshots,events:events.length,faults};
   } finally {host.dispose();}
 });
 assert.deepEqual(errors,[]);assert.deepEqual(result.faults,[]);
 assert.ok(result.snapshots.length>3);assert.ok(result.snapshots.some(s=>s.ack===1));
 assert.equal(result.before.tick,result.after.tick);assert.equal(result.after.queuedInputs,0);
 assert.equal(result.suspendedInput.accepted,false);
 console.log(JSON.stringify({pass:true,...result}));
} finally {await browser.close();}
