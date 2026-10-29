// Retail-map visual checks; use ENV_ARTIFACTS to save Classic/Enhanced screenshots.
// The world worker puts the soldier out of the shot; the page draws the
// environment from a fixed camera (tests/battle-page.mjs).
import fs from 'node:fs/promises';
import {launchBrowser,openBattle} from './battle-page.mjs';
const server=process.env.ENV_START_SERVER?await (await import('vite')).createServer({cacheDir:'/tmp/environment-map-test-cache',optimizeDeps:{entries:['index.html']},server:{watch:null,port:18457,strictPort:true,host:'127.0.0.1'}}):null;
if(server)await server.listen();
const browser=await launchBrowser();
try {
for(const map of ['nab2','bes2']){
 const battle=await openBattle(browser,{base:process.env.RENDER_TEST_URL||'http://127.0.0.1:18457',query:`map=${map}&side=rep&unit=rep_inf_clone_trooper&shadows=low&spawn=CP3`,
  viewport:{width:800,height:500},settings:null,beforeLoad:page=>page.setDefaultNavigationTimeout(120000)});
 const {page,world,errors}=battle;
 page.on('pageerror',e=>console.error(e.message));
 const view=map==='nab2'?{position:[-25,-44,123],target:[-36,-46.8,111]}:{position:[-167,-59,-9],target:[-178,-63,-20]};
 // Under the camera, below the floor; a tick shows him there.
 await world.evaluate(([x,z])=>self.__bf.soldier.position.set(x,-70,z),[view.position[0],view.position[2]]);
 await battle.frame(1/60);
 const stats=await page.evaluate(view=>{
  const b=__bf;b.settings.set('shadows','off');b.settings.set('antialiasing','4x');b.settings.set('softFocus',false);
  document.querySelectorAll('body > :not(canvas)').forEach(e=>{if(!e.querySelector('canvas'))e.style.display='none'});
  const c=b.camera;c.position.fromArray(view.position);c.lookAt(...view.target);c.updateMatrixWorld();
  const out=[];
  for(const mode of ['classic','enhanced']){
   b.settings.set('environmentEffects',mode);for(let i=0;i<100;i++)b.environment.update(.05,c,[]);
   b.environment.reflect(c,b.water);b.graphics.render(b.environment.time);
   const gl=b.renderer.getContext(),pixel=new Uint8Array(4);gl.readPixels(0,0,1,1,gl.RGBA,gl.UNSIGNED_BYTE,pixel);
   out.push({mode,stats:{...b.environment.stats},calls:b.graphics.frameInfo.calls,error:gl.getError(),waterMeshes:b.environment.water?.original.length});
   window.__captures??={};__captures[mode]=b.renderer.domElement.toDataURL();
  }
  return out;
 },view);
 if(errors.length||stats.some(s=>s.error||s.stats.activeSources===0))throw Error(JSON.stringify({map,stats,errors}));
 console.log(map,JSON.stringify(stats));
 if(process.env.ENV_ARTIFACTS)await fs.mkdir(process.env.ENV_ARTIFACTS,{recursive:true});
 for(const mode of process.env.ENV_ARTIFACTS?['classic','enhanced']:[]){
  const data=await page.evaluate(m=>__captures[m],mode);await fs.writeFile(`${process.env.ENV_ARTIFACTS}/${map}-${mode}.png`,Buffer.from(data.split(',')[1],'base64'));
 }
 await battle.close();
}
} finally {await browser.close();await server?.close();}
