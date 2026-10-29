import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
try {
 const page=await browser.newPage({viewport:{width:1000,height:700}}),errors=[];
 page.on('pageerror',e=>errors.push(e.message));page.on('console',m=>{if(m.type()==='error')errors.push(m.text())});
 await page.route('**/@vite/client',r=>r.fulfill({contentType:'application/javascript',body:''}));
 await page.addInitScript(()=>{const raf=requestAnimationFrame.bind(window);window.requestAnimationFrame=callback=>callback.name==='loop'?0:raf(callback)});
 await page.goto(`${process.env.WEATHER_TEST_URL||'http://127.0.0.1:18449'}/?map=kam1&unit=rep_inf_jet_trooper&shadows=off`);
 await page.waitForFunction(()=>window.__bf,null,{timeout:120000});
 const result=await page.evaluate(async()=>{
  const b=__bf;await b.ready;b.pause();b.settings.set('rainEffects','classic');b.settings.set('oceanEffects','classic');b.settings.set('softFocus',false);b.settings.set('antialiasing','off');b.actor.root.visible=false;
  const world=await fetch('/maps/kam1/map.json').then(r=>r.json());const {makeHit}=await import('/src/collision.js');
  const hit=makeHit(),origin=new b.THREE.Vector3(),down=new b.THREE.Vector3(0,-1,0);let supported=0,total=0;
  for(const points of Object.values(world.spawns))for(const p of points){origin.set(p[0],p[1]+1,p[2]);total++;if(b.collision.cast(origin,down,5,hit))supported++;}
  const camera=b.camera;camera.position.set(-12,47,166);camera.lookAt(-55,20,90);camera.updateMatrixWorld();
  b.water.update(.2,camera);b.water.reflect(camera);b.weather.update(.2,camera);
  const gl=b.renderer.getContext(),size=b.renderer.getDrawingBufferSize(new b.THREE.Vector2());
  const pixels=()=>{const a=new Uint8Array(size.x*size.y*4);gl.readPixels(0,0,size.x,size.y,gl.RGBA,gl.UNSIGNED_BYTE,a);return a};
  const render=()=>{b.graphics.render(0);return pixels()};
  const difference=(a,c)=>{let n=0;for(let i=0;i<a.length;i+=4)if(Math.abs(a[i]-c[i])+Math.abs(a[i+1]-c[i+1])+Math.abs(a[i+2]-c[i+2])>6)n++;return n};
  b.weather.mesh.visible=false;const dry=render();b.weather.mesh.visible=true;const rainy=render();
  const image=b.renderer.domElement.toDataURL();
  b.water.ocean.near.visible=b.water.ocean.far.visible=false;const noSea=render();
  const oceanPixels=difference(rainy,noSea);b.water.ocean.near.visible=b.water.ocean.far.visible=true;
  const rainPixels=difference(dry,rainy);
  b.weather.update(.3,camera);const animated=render();const movingPixels=difference(rainy,animated);
  // Cover the whole precipitation volume: no rain may survive below a roof.
  b.weather.cover.heights.fill(camera.position.y+40);b.weather.cover.texture.needsUpdate=true;
  const sheltered=render();b.weather.mesh.visible=false;const shelteredDry=render();const indoorRainPixels=difference(sheltered,shelteredDry);
  // The texture-based ocean must survive every water-quality choice.
  const quality=[];for(const value of ['low','high','ultra']){b.settings.set('waterQuality',value);b.water.reflect(camera);render();quality.push({value,visible:b.water.ocean.near.visible,error:gl.getError()})}
  return {terrainMeshes:world.meshes.filter(m=>m.terrain).length,total,supported,rainPixels,oceanPixels,movingPixels,indoorRainPixels,quality,rainCount:b.weather.count,oceanTriangles:b.water.ocean.geometry.index.count/3,drawCalls:b.graphics.frameInfo.calls,image};
 });
 const {image,...metrics}=result;console.log(JSON.stringify({metrics,errors}));
 assert.equal(result.terrainMeshes,0);assert.equal(result.total,64);assert.equal(result.supported,64);assert.ok(result.rainPixels>50);assert.ok(result.oceanPixels>5000);assert.ok(result.movingPixels>50);
 assert.equal(result.indoorRainPixels,0);assert.ok(result.oceanTriangles<8300);assert.equal(result.rainCount,256);
 assert.ok(result.quality.every(q=>q.visible&&!q.error));assert.deepEqual(errors,[]);
 await fs.writeFile('/tmp/bf-kamino-weather.png',Buffer.from(image.split(',')[1],'base64'));
 await page.close();
} finally {await browser.close()}
