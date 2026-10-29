import assert from 'node:assert/strict';import fs from 'node:fs/promises';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
try{
 const page=await browser.newPage({viewport:{width:1100,height:750}}),errors=[];
 page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error')errors.push(m.text())});
 await page.route('**/@vite/client',r=>r.fulfill({contentType:'application/javascript',body:''}));
 await page.addInitScript(()=>{const raf=requestAnimationFrame.bind(window);window.requestAnimationFrame=cb=>cb.name==='loop'?0:raf(cb)});
 await page.goto(`${process.env.WEATHER_TEST_URL||'http://127.0.0.1:18449'}/?map=kam1&side=cis&unit=cis_inf_assault&shadows=high`);
 await page.waitForFunction(()=>window.__bf,null,{timeout:120000});
 const data=await page.evaluate(async()=>{
  const b=__bf;await b.ready;b.pause();b.actor.root.visible=false;b.settings.set('softFocus',false);b.settings.set('bloom',false);
  const c=b.camera;c.position.set(-12,46.5,166);c.lookAt(-32,41,124);c.updateMatrixWorld();
  b.water.update(.01,c);b.water.reflect(c);
  const gl=b.renderer.getContext(),size=b.renderer.getDrawingBufferSize(new b.THREE.Vector2());
  const render=()=>{b.graphics.render(0);const pixels=new Uint8Array(size.x*size.y*4);gl.readPixels(0,0,size.x,size.y,gl.RGBA,gl.UNSIGNED_BYTE,pixels);return pixels};
  const diff=(a,z)=>{let pixels=0,energy=0;for(let i=0;i<a.length;i+=4){const d=Math.abs(a[i]-z[i])+Math.abs(a[i+1]-z[i+1])+Math.abs(a[i+2]-z[i+2]);if(d>6)pixels++;energy+=d;}return {pixels,energy}};
  const captures=[],metrics=[];
  for(const mode of ['classic','enhanced']){
   b.settings.set('rainEffects',mode);for(let i=0;i<120;i++)b.weather.update(1/60,c);
   const withRain=render(),calls=b.graphics.frameInfo.calls;captures.push({mode,image:b.renderer.domElement.toDataURL()});
   const meshes=[b.weather.mesh,b.weather.mist,b.weather.impacts.mesh],visible=meshes.map(m=>m.visible);meshes.forEach(m=>m.visible=false);
   const without=render();meshes.forEach((m,i)=>m.visible=visible[i]);
   const o=b.water.ocean;o.near.visible=o.far.visible=false;const withoutWater=render();o.near.visible=o.far.visible=true;
   metrics.push({mode,rain:diff(withRain,without),water:diff(withRain,withoutWater),count:b.weather.count,calls,weatherDraws:visible.filter(Boolean).length,impacts:b.weather.impacts.next,gl:gl.getError()});
  }
  c.position.set(-10,46.2,141);c.lookAt(-10,44.5,174);c.updateMatrixWorld();
  for(let i=0;i<90;i++)b.weather.update(1/60,c);b.water.update(0,c);b.water.reflect(c);render();
  captures.push({mode:'enhanced-deck',image:b.renderer.domElement.toDataURL()});
  // No scene/geometry churn when switching modes repeatedly.
  const geometry=b.weather.mesh.geometry;for(let i=0;i<6;i++){b.settings.set('rainEffects',i%2?'enhanced':'classic');b.weather.update(1/60,c);render();}
  return {metrics,captures,reused:geometry===b.weather.mesh.geometry,gl:gl.getError()};
 });
 console.log(JSON.stringify({metrics:data.metrics,reused:data.reused,gl:data.gl,errors}));
 for(const c of data.captures)await fs.writeFile(`/tmp/bf-rain-${c.mode}.png`,Buffer.from(c.image.split(',')[1],'base64'));
 assert.ok(data.metrics.every(m=>m.rain.pixels>100&&m.water.pixels>5000&&!m.gl));
 assert.ok(data.metrics[1].rain.pixels>data.metrics[0].rain.pixels);assert.equal(data.metrics[1].weatherDraws,3);assert.ok(data.metrics[1].impacts>0);
 assert.ok(data.metrics[1].count<=8192);assert.ok(data.reused);assert.equal(data.gl,0);assert.deepEqual(errors,[]);
}finally{await browser.close()}
