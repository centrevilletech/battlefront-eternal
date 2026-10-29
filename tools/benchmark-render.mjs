// Frozen A/B servers must differ only in the render/control modules under test.
// GPU completion is deliberately synchronized here, never in production.
// SwiftShader results measure software rendering, not hardware gaming FPS.
import fs from 'node:fs/promises';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const urls = (process.env.RENDER_BENCH_URLS || 'http://127.0.0.1:18449').split(',');
const maps = (process.env.RENDER_BENCH_MAPS || 'nab2,bes2').split(',');
const frames = Number(process.env.RENDER_BENCH_FRAMES || 40);
const rounds = Number(process.env.RENDER_BENCH_ROUNDS || 3);
const width = Number(process.env.RENDER_BENCH_WIDTH || 640), height = Number(process.env.RENDER_BENCH_HEIGHT || 400);
const args = process.env.RENDER_BENCH_GPU === 'hardware' ? [] : ['--use-angle=swiftshader','--enable-unsafe-swiftshader'];
const browser = await chromium.launch({args});
const results = [];
try {
  for (const map of maps) {
    const pages = [];
    for (const url of urls) {
      const page = await browser.newPage({viewport:{width,height}});
      await page.route('**/@vite/client',r=>r.fulfill({contentType:'application/javascript',body:''}));
      await page.addInitScript(() => {
        const raf=window.requestAnimationFrame.bind(window);
        window.requestAnimationFrame=callback=>callback.name==='loop'?0:raf(callback);
        // Three's UUIDs consume Math.random too. Keep their stream separate so
        // adding render resources cannot move spawns or redistribute foliage.
        let gameSeed=47291,uuidSeed=8191;
        Math.random=()=> {
          if (new Error().stack.includes('generateUUID')) return ((uuidSeed=Math.imul(uuidSeed,1664525)+1013904223>>>0)/4294967296);
          return ((gameSeed=Math.imul(gameSeed,1664525)+1013904223>>>0)/4294967296);
        };
      });
      const errors=[];page.on('pageerror',e=>errors.push(e.message));
      console.error(`Loading ${map}: ${url}`);
      await page.goto(`${url}/?map=${map}&side=rep&unit=rep_inf_clone_trooper&shadows=low&spawn=CP3`);
      await page.waitForFunction(()=>window.__bf,null,{timeout:120000});
      console.error('Waiting for textures');
      await page.evaluate(async()=>{__bf.pause();await Promise.race([__bf.ready,new Promise((_,reject)=>setTimeout(()=>reject(new Error('Texture readiness timeout')),30000))]);});
      console.error('Warming render passes');
      await page.evaluate(async()=>{
        const b=__bf,g=b.graphics,gl=b.renderer.getContext();
        await b.lockstep(0,false);
        const camera=b.camera.clone();
        const syncPixel=new Uint8Array(4);
        const counters={};
        for(const key of ['getParameter','clear','blitFramebuffer','bufferData','bufferSubData','texSubImage2D']) {
          const original=gl[key].bind(gl);
          gl[key]=(...args)=>{counters[key]=(counters[key]||0)+1;return original(...args)};
        }
        const debug=gl.getExtension('WEBGL_debug_renderer_info');
        window.__renderBench={counters,errors:[],gpu:debug?gl.getParameter(debug.UNMASKED_RENDERER_WEBGL):gl.getParameter(gl.RENDERER),run(frames){
          b.camera.copy(camera);
          for(const key of Object.keys(counters))counters[key]=0;
          const cpu=[],complete=[];
          for(let i=0;i<frames;i++) {
            b.camera.rotation.y=camera.rotation.y+Math.sin(i/frames*Math.PI*2)*.65;
            b.camera.updateMatrixWorld();b.foliage?.update(0,b.camera);
            const start=performance.now();
            b.water?.reflect(b.camera);g.render(0);b.water?.drawUnderwater(b.renderer,b.camera);b.hud.render();
            cpu.push(performance.now()-start);
            // A one-pixel readback forces the browser/GPU-process round trip;
            // gl.finish alone can still leave work queued in Chromium.
            gl.readPixels(0,0,1,1,gl.RGBA,gl.UNSIGNED_BYTE,syncPixel);
            complete.push(performance.now()-start);
          }
          const counts={...counters};
          const w=b.renderer.domElement.width,h=b.renderer.domElement.height;
          const pixels=new Uint8Array(w*h*4);gl.readPixels(0,0,w,h,gl.RGBA,gl.UNSIGNED_BYTE,pixels);
          let hash=2166136261;for(const value of pixels)hash=Math.imul(hash^value,16777619)>>>0;
          const error=gl.getError();if(error)throw new Error(`WebGL error ${error}`);
          return {cpu,complete,counts,pixelHash:hash,draws:g.frameInfo.calls,triangles:g.frameInfo.triangles};
        }};
        __renderBench.run(12);
      });
      console.error("Warmup complete");
      pages.push({page,url,errors});
    }
    const samples=pages.map(()=>[]);
    // Alternate which version runs first to reduce shared-machine drift.
    for(let round=0;round<rounds;round++) {
      for(const i of pages.map((_,i)=>i).sort((a,b)=>round%2?b-a:a-b)) {
        samples[i].push(await pages[i].page.evaluate(n=>__renderBench.run(n),frames));
        const sample=samples[i].at(-1);
        const completed=[...sample.complete].sort((a,b)=>a-b);
        console.error(`${map} round ${round+1}: ${pages[i].url}, completed median ${completed[Math.floor(completed.length/2)].toFixed(2)} ms`);
      }
    }
    for(let i=0;i<pages.length;i++) {
      const {page,url,errors}=pages[i];if(errors.length)throw new Error(errors.join('\n'));
      const summary=key=>{const values=samples[i].flatMap(s=>s[key]).sort((a,b)=>a-b);return {medianMs:values[Math.floor(values.length/2)],p95Ms:values[Math.ceil(values.length*.95)-1],maxMs:values.at(-1)}};
      const result={map,url,resolution:[width,height],gpu:await page.evaluate(()=>__renderBench.gpu),frames:frames*rounds,cpu:summary('cpu'),completed:summary('complete'),rounds:samples[i].map(({cpu,complete,...rest})=>rest)};
      results.push(result);console.log(JSON.stringify(result));await page.close();
    }
  }
} finally {await browser.close();if(process.env.RENDER_BENCH_OUTPUT)await fs.writeFile(process.env.RENDER_BENCH_OUTPUT,JSON.stringify(results,null,2));}
