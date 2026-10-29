// Iris blur runs inside the existing composite; the HUD is drawn afterwards.
// The world (in its worker) boards him; the page draws (tests/battle-page.mjs).
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import {launchBrowser,openBattle} from './battle-page.mjs';
const browser=await launchBrowser();
try {
 const battle=await openBattle(browser,{base:process.env.VEHICLE_TEST_URL || 'http://127.0.0.1:18449',query:'map=nab2&unit=rep_inf_clone_trooper&shadows=off',viewport:{width:960,height:600},settings:null});
 const {page,world,errors}=battle;
 page.on('console',m=>{if(m.type()!=='error'&&/GL_INVALID|WebGL.*error/.test(m.text()))errors.push(m.text())});
 const entered=await world.evaluate(()=>{const b=self.__bf;return b.driver.enter(b.vehicles.vehicles.find(v=>v.name==='rep_hover_fightertank'))});
 const result=await page.evaluate(async()=>{
  const b=__bf;b.setPerspective(false);
  b.settings.set('waterQuality','low');b.settings.set('treeDetail','original');
  // Two frames compared pixel for pixel: no temporal jitter between them.
  b.settings.set('upscaler','off');
  for(let i=0;i<180;i++)await b.lockstep(1/60,false);
  await b.lockstep(0,true);
  await Promise.all([...b.hud.textures.values()].map(t=>t.image?.decode?.().catch(()=>{}) || Promise.resolve()));
  const gl=b.renderer.getContext(),w=b.renderer.domElement.width,h=b.renderer.domElement.height;
  const pixels=()=>{const p=new Uint8Array(w*h*4);gl.readPixels(0,0,w,h,gl.RGBA,gl.UNSIGNED_BYTE,p);return p};
  b.settings.set('vehicleMotionBlur',true);
  b.graphics.setVehicleMotion(0,0);b.graphics.render(0);b.hud.render();const sharp=pixels(),calls=b.renderer.info.render.calls;
  b.graphics.setVehicleMotion(1,1);b.graphics.render(0);b.hud.render();const blurred=pixels(),blurCalls=b.renderer.info.render.calls;
  const image=b.renderer.domElement.toDataURL();
  let center=0,edges=0,nc=0,ne=0;
  for(let y=0;y<h;y++)for(let x=0;x<w;x++){
   const radius=Math.hypot(x/w-.5,y/h-.5);const i=(y*w+x)*4;
   const d=Math.abs(sharp[i]-blurred[i])+Math.abs(sharp[i+1]-blurred[i+1])+Math.abs(sharp[i+2]-blurred[i+2]);
   if(radius<.1){center+=d;nc+=3}else if(radius>.4){edges+=d;ne+=3}
  }
  b.settings.set('vehicleMotionBlur',false);b.graphics.setVehicleMotion(1,1);
  return {driving:b.driver.active,center:center/nc,edges:edges/ne,calls,blurCalls,disabled:b.graphics.composite.material.uniforms.speedBlur.value,image};
 });
 const {image,...metrics}=result;console.log({entered,...metrics});
 assert.ok(result.center<.2,'aiming area must stay sharp');assert.ok(result.edges>.3,'peripheral speed streaks must be visible');
 assert.equal(result.calls,result.blurCalls,'speed blur must not add a render pass');assert.equal(result.disabled,0);assert.deepEqual(errors,[]);
 await fs.writeFile('/tmp/bf-vehicle-speed-blur.png',Buffer.from(image.split(',')[1],'base64'));
} finally { await browser.close(); }
