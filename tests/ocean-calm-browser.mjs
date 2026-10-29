// Kashyyyk: Islands with Ocean and sky on Enhanced: a sea under a dry sky
// takes the Enhanced waves alone. Its own sky (the dome and the mountains on
// its horizon) stays exactly as Classic draws it, with no storm panorama,
// veil or lightning, and the waves reflect that sky (sky-probe.js).
//   WEATHER_TEST_URL=http://127.0.0.1:<port> PLAYWRIGHT_MODULE=... node tests/ocean-calm-browser.mjs
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
try{
 const page=await browser.newPage({viewport:{width:1200,height:800}}),errors=[];
 page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error')errors.push(m.text())});
 await page.route('**/@vite/client',r=>r.fulfill({contentType:'application/javascript',body:''}));
 await page.addInitScript(()=>{const raf=requestAnimationFrame.bind(window);window.requestAnimationFrame=cb=>cb.name==='loop'?0:raf(cb)});
 await page.goto(`${process.env.WEATHER_TEST_URL||'http://127.0.0.1:18449'}/?map=kas1&side=rep&unit=rep_inf_clone_trooper&shadows=off`);
 await page.waitForFunction(()=>window.__bf,null,{timeout:120000});
 const result=await page.evaluate(async()=>{
  const b=__bf;await b.ready;b.pause();b.actor.root.visible=false;b.settings.set('softFocus',false);b.settings.set('bloom',false);
  // Pixel for pixel: no temporal jitter.
  b.settings.set('upscaler','off');
  // From the landing zone, east over the sea to the mountains on the horizon.
  const c=b.camera;c.fov=55;c.updateProjectionMatrix();c.position.set(167,-22,147);c.lookAt(700,-30,160);c.updateMatrixWorld();
  const gl=b.renderer.getContext(),size=b.renderer.getDrawingBufferSize(new b.THREE.Vector2());
  const render=()=>{b.water.reflect(c);b.graphics.render(0);const pixels=new Uint8Array(size.x*size.y*4);gl.readPixels(0,0,size.x,size.y,gl.RGBA,gl.UNSIGNED_BYTE,pixels);return pixels};
  // Rows are read bottom up; `rows` picks the share of the frame from its top.
  const diff=(a,z,from=0,to=1)=>{let pixels=0,energy=0;const first=Math.floor(size.y*(1-to))*size.x*4,last=Math.floor(size.y*(1-from))*size.x*4;
   for(let i=first;i<last;i+=4){const d=Math.abs(a[i]-z[i])+Math.abs(a[i+1]-z[i+1])+Math.abs(a[i+2]-z[i+2]);if(d>9)pixels++;energy+=d;}return {pixels,energy}};
  const captures=[],capture=name=>captures.push({name,image:b.renderer.domElement.toDataURL()});
  // The horizon's row: where the eye's level line crosses the screen.
  const level=new b.THREE.Vector3(700,c.position.y,160).project(c),horizon=(1-level.y)/2;
  b.settings.set('oceanEffects','classic');b.water.update(0,c);const classic=render();capture('classic');
  b.settings.set('oceanEffects','enhanced');b.water.update(1,c);const enhanced=render();capture('enhanced');
  const o=b.water.ocean,w=o.waves,far=b.graphics.farScene;
  const stormMeshes=['storm-volume-clouds','storm-lightning','ocean-horizon-haze'].filter(name=>far.getObjectByName(name));
  const skyShown=b.graphics.skyLayers.every(({mesh})=>mesh.visible&&mesh.material.name==='zero:sky');
  // Above the horizon nothing changes; below it the sea does.
  const sky=diff(classic,enhanced,0,horizon-.02),sea=diff(classic,enhanced,horizon+.02,1);
  b.water.update(2,c);const moved=render();const motion=diff(enhanced,moved,horizon+.02,1);
  // What it reflects is the probe's sky: without it the sea changes.
  const amount=w.uniforms.reflectAmount.value;w.uniforms.reflectAmount.value=0;const unreflected=render();w.uniforms.reflectAmount.value=amount;
  const reflection=diff(moved,unreflected,horizon+.02,1);
  // The Classic layer's animated normal and bump frames upload when first
  // drawn: one switch at this time first, then the count must hold.
  b.settings.set('oceanEffects','classic');b.water.update(0,c);render();b.settings.set('oceanEffects','enhanced');b.water.update(0,c);render();
  const memory={...b.renderer.info.memory};
  for(let i=0;i<4;i++){b.settings.set('oceanEffects','classic');b.water.update(0,c);render();b.settings.set('oceanEffects','enhanced');b.water.update(0,c);render();}
  const finalMemory={...b.renderer.info.memory};
  c.position.set(0,30,0);c.lookAt(800,-20,300);c.updateMatrixWorld();b.water.update(0,c);render();capture('overview');
  return {captures,horizon,stormMeshes,skyShown,stormy:o.stormy,storm:Boolean(w.storm),haze:Boolean(w.haze),captured:w.probe.captures,
   defines:Object.keys(w.material.defines),sky,sea,motion,reflection,memory,finalMemory,gl:gl.getError()};
 });
 const {captures,...metrics}=result;console.log(JSON.stringify({metrics,errors}));
 for(const c of captures)await fs.writeFile(`/tmp/bf-ocean-calm-${c.name}.png`,Buffer.from(c.image.split(',')[1],'base64'));
 assert.equal(result.stormy,false);assert.equal(result.storm,false);assert.equal(result.haze,false);
 assert.deepEqual(result.stormMeshes,[]);assert.ok(result.skyShown);assert.ok(!result.defines.includes('STORM'));
 assert.ok(result.captured>=1);
 assert.equal(result.sky.pixels,0,'the sky above the horizon is exactly Classic\'s');
 assert.ok(result.sea.pixels>20000,'the Enhanced waves replace the flat layer');
 assert.ok(result.motion.pixels>10000,'the waves move');
 assert.ok(result.reflection.pixels>10000,'the sea reflects the sky');
 assert.deepEqual(result.memory,result.finalMemory);assert.equal(result.gl,0);assert.deepEqual(errors,[]);
}finally{await browser.close()}
