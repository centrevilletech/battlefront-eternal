import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
try{
 const page=await browser.newPage({viewport:{width:1200,height:800}}),errors=[];
 page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error')errors.push(m.text())});
 await page.route('**/@vite/client',r=>r.fulfill({contentType:'application/javascript',body:''}));
 await page.addInitScript(()=>{const raf=requestAnimationFrame.bind(window);window.requestAnimationFrame=cb=>cb.name==='loop'?0:raf(cb)});
 await page.goto(`${process.env.WEATHER_TEST_URL||'http://127.0.0.1:18449'}/?map=kam1&side=cis&unit=cis_inf_assault&shadows=off`);
 await page.waitForFunction(()=>window.__bf,null,{timeout:120000});
 const result=await page.evaluate(async()=>{
  const b=__bf;await b.ready;b.pause();b.actor.root.visible=false;b.settings.set('softFocus',false);b.settings.set('bloom',true);
  const c=b.camera,o=b.water.ocean;c.position.set(-60,48,155);c.lookAt(-300,30,100);c.updateMatrixWorld();
  const gl=b.renderer.getContext(),size=b.renderer.getDrawingBufferSize(new b.THREE.Vector2());
  const render=()=>{b.water.reflect(c);b.graphics.render(0);const pixels=new Uint8Array(size.x*size.y*4);gl.readPixels(0,0,size.x,size.y,gl.RGBA,gl.UNSIGNED_BYTE,pixels);return pixels};
  const diff=(a,z)=>{let pixels=0,energy=0;for(let i=0;i<a.length;i+=4){const d=Math.abs(a[i]-z[i])+Math.abs(a[i+1]-z[i+1])+Math.abs(a[i+2]-z[i+2]);if(d>9)pixels++;energy+=d;}return {pixels,energy}};
  const captures=[],capture=name=>captures.push({name,image:b.renderer.domElement.toDataURL()});
  b.settings.set('oceanEffects','classic');b.water.update(0,c);const classic=render();capture('classic');
  b.settings.set('oceanEffects','enhanced');b.water.update(3,c);const enhanced=render();capture('enhanced');
  o.waves.spray.visible=false;const sprayDifference=diff(enhanced,render());o.waves.spray.visible=true;
  const modeDifference=diff(classic,enhanced),sky=o.waves.storm,geometry=o.geometry;
  b.water.update(2,c);const moved=render();capture('moving');const motion=diff(enhanced,moved);
  const cloudRenders=sky.cloudRenders;
  c.rotation.y+=.02;c.updateMatrixWorld();b.water.update(0,c);render();
  const cached=sky.cloudRenders===cloudRenders;
  // A scheduled strike, then view it from the platform at peak brightness.
  b.water.update(sky.nextStrike-o.time+.001,c);b.water.update(.06,c);
  const strike=sky.uniforms.lightningPosition.value;
  c.lookAt(strike.x,65,strike.z);c.updateMatrixWorld();
  const lightning=render();capture('lightning');const flash=sky.uniforms.lightningFlash.value;
  sky.bolt.visible=false;const boltDifference=diff(lightning,render());sky.bolt.visible=true;
  b.water.update(.6,c);const after=render();const flashDifference=diff(lightning,after);
  c.lookAt(strike.x,200,strike.z);c.updateMatrixWorld();render();capture('clouds');
  const memory={...b.renderer.info.memory};
  const draws=b.graphics.frameInfo.calls,triangles=o.geometry.index.count/3;
  const quality=[];
  for(const q of ['low','high','ultra']){b.settings.set('waterQuality',q);b.water.update(0,c);render();quality.push({q,visible:o.near.visible,error:gl.getError()});}
  for(let i=0;i<4;i++){b.settings.set('oceanEffects','classic');b.water.update(0,c);render();b.settings.set('oceanEffects','enhanced');b.water.update(0,c);render();}
  const reused=o.geometry===geometry&&o.waves.storm===sky,finalMemory={...b.renderer.info.memory};
  // Actual authored deck view, then just above the swells to review silhouette.
  c.position.set(-12,47,166);c.lookAt(-55,20,90);c.updateMatrixWorld();b.water.update(0,c);render();capture('platform');
  c.position.set(-160,4,130);c.lookAt(-320,8,110);c.updateMatrixWorld();b.water.update(0,c);render();capture('surface');
  // The models on the dome's horizon (Kamino's city) stay in the storm, drawn
  // over its panorama: from the deck they fill the top of the view. Their own
  // pixels are those they change; a flash lights them.
  c.position.set(-12,47,166);c.lookAt(-55,20,90);c.updateMatrixWorld();
  const horizon=b.graphics.skyLayers.filter(l=>l.layer).map(l=>l.mesh);
  b.water.update(.6,c);const shown=render();capture('city');
  horizon.forEach(m=>m.visible=false);const hidden=render();horizon.forEach(m=>m.visible=true);
  const city=diff(shown,hidden),mask=[];
  for(let i=0;i<shown.length;i+=4)if(Math.abs(shown[i]-hidden[i])+Math.abs(shown[i+1]-hidden[i+1])+Math.abs(shown[i+2]-hidden[i+2])>9)mask.push(i);
  const luma=p=>mask.reduce((s,i)=>s+p[i]*.3+p[i+1]*.59+p[i+2]*.11,0)/Math.max(1,mask.length);
  const horizonMaterials=horizon.map(m=>m.material.name);
  b.water.update(sky.nextStrike-o.time+.001,c);b.water.update(.06,c);
  const lit=render();capture('city-lightning');const cityFlash=sky.uniforms.lightningFlash.value;
  const cityRest=luma(shown),cityLit=luma(lit);
  return {captures,modeDifference,motion,sprayDifference,boltDifference,flashDifference,flash,cached,reused,memory,finalMemory,quality,draws,triangles,
   city,cityPixels:mask.length,cityRest,cityLit,cityFlash,horizonMaterials,gl:gl.getError()};
 });
 const {captures,...metrics}=result;console.log(JSON.stringify({metrics,errors}));
 for(const c of captures)await fs.writeFile(`/tmp/bf-ocean-${c.name}.png`,Buffer.from(c.image.split(',')[1],'base64'));
 assert.ok(result.modeDifference.pixels>10000);assert.ok(result.motion.pixels>10000);assert.ok(result.flashDifference.pixels>10000);assert.ok(result.flash>.9);
 assert.ok(result.sprayDifference.pixels>50);assert.ok(result.boltDifference.pixels>20);
 assert.ok(result.cached&&result.reused);assert.deepEqual(result.memory,result.finalMemory);assert.ok(result.triangles<66000);
 assert.ok(result.quality.every(q=>q.visible&&!q.error));assert.equal(result.gl,0);assert.deepEqual(errors,[]);
 assert.ok(result.horizonMaterials.length&&result.horizonMaterials.every(name=>name==='storm-horizon'));
 assert.ok(result.cityPixels>20000,'the city stands on the storm horizon');
 assert.ok(result.cityFlash>.9);assert.ok(result.cityLit>result.cityRest*1.1,'a flash lights the city');
}finally{await browser.close()}
