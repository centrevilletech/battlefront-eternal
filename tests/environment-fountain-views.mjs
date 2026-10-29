// Theed fountain review stills, Classic beside Enhanced: the map-check view,
// a third-person distance, a far view, a backlit view and a one-second motion
// sequence. Each still settles over a few 30 Hz frames, so the temporal
// upscaler shows what play shows. The world worker puts the soldier out of
// the shot (tests/battle-page.mjs); the page draws. Run against the local
// Vite server:
//   RENDER_TEST_URL=http://127.0.0.1:18449 FOUNTAIN_ARTIFACTS=/tmp/fountain \
//   node tests/environment-fountain-views.mjs
// FOUNTAIN_VIEWS and FOUNTAIN_MODES (comma lists) pick views (close and basin
// too) and looks; FOUNTAIN_PREFIX starts each file name.
import fs from 'node:fs/promises';
import {launchBrowser,openBattle} from './battle-page.mjs';
const dir=process.env.FOUNTAIN_ARTIFACTS||'/tmp/fountain-views',prefix=process.env.FOUNTAIN_PREFIX||'';
const modes=(process.env.FOUNTAIN_MODES||'classic,enhanced').split(',');
// The fountain's nozzle is at (-36, -48.8, 111); Theed's plaza floor is at -50.
const views={
 baseline:{position:[-25,-44,123],target:[-36,-46.8,111]},
 gameplay:{position:[-30.9,-48.1,116.5],target:[-36,-46.6,111]},
 far:{position:[-18.5,-48,80.7],target:[-36,-46.5,111]},
 backlit:{position:[-42.5,-48.1,114.75],target:[-36,-46.4,111]},
 close:{position:[-33.4,-48.3,114.6],target:[-36,-46.4,111]},
 basin:{position:[-31.2,-46.2,115.6],target:[-35.6,-48.9,111.4]},
};
const sequence={view:'gameplay',frames:6,step:.2};
const wanted=(process.env.FOUNTAIN_VIEWS||['baseline','gameplay','far','backlit','sequence'].join(',')).split(',');
await fs.mkdir(dir,{recursive:true});
const browser=await launchBrowser();
try{
 const battle=await openBattle(browser,{base:process.env.RENDER_TEST_URL||'http://127.0.0.1:18449',query:'map=nab2&side=rep&unit=rep_inf_clone_trooper&shadows=low&spawn=CP3',
  viewport:{width:800,height:500},settings:null,beforeLoad:page=>{page.setDefaultNavigationTimeout(180000);
   return page.addInitScript(()=>{let a=917;Math.random=()=>((a=Math.imul(a,1664525)+1013904223>>>0)/4294967296);});}});
 const {page,world,errors}=battle;
 // Under the baseline camera, below the floor; a tick shows him there.
 await world.evaluate(()=>self.__bf.soldier.position.set(-25,-70,123));
 await battle.frame(1/60);
 await page.evaluate(()=>{
  const b=__bf;
  for(const [key,value] of Object.entries({shadows:'off',antialiasing:'4x',softFocus:false,bloom:true}))b.settings.set(key,value);
  document.querySelectorAll('body > :not(canvas)').forEach(e=>{if(!e.querySelector('canvas'))e.style.display='none'});
  // One displayed frame: environment, basin capture and the full pipeline.
  window.fountainFrame=(dt)=>{const c=b.camera;c.updateMatrixWorld();b.environment.update(dt,c,[]);b.environment.reflect(c,b.water);b.graphics.render(b.environment.time);};
  window.fountainView=(view)=>{const c=b.camera;c.position.fromArray(view.position);c.lookAt(...view.target);c.updateMatrixWorld();};
 });
 for(const mode of modes){
  await page.evaluate(mode=>{const b=__bf;b.settings.set('environmentEffects',mode);fountainView({position:[-25,-44,123],target:[-36,-46.8,111]});
   for(let i=0;i<100;i++)b.environment.update(.05,b.camera,[]);},mode);
  // The canvas is read in the same task as its last frame, before it is presented and cleared.
  const save=async(name,png)=>{await fs.writeFile(`${dir}/${prefix}${name}_${mode}.png`,Buffer.from(png.split(',')[1],'base64'));console.log(`${prefix}${name}_${mode}`);};
  for(const name of wanted){
   if(name==='sequence'){
    await page.evaluate(view=>{fountainView(view);for(let i=0;i<3;i++)fountainFrame(1/30);},views[sequence.view]);
    for(let i=0;i<sequence.frames;i++){
     // Advance to the next sample time, drawing the last few 30 Hz frames.
     await save(`seq${i}`,await page.evaluate(step=>{const n=Math.round(step*30);
      for(let k=0;k<n;k++){if(k<n-3)__bf.environment.update(1/30,__bf.camera,[]);else fountainFrame(1/30);}return __bf.renderer.domElement.toDataURL();},i?sequence.step:1/30));
    }
    continue;
   }
   await save(name,await page.evaluate(view=>{fountainView(view);for(let i=0;i<4;i++)fountainFrame(1/30);return __bf.renderer.domElement.toDataURL();},views[name]));
   // FOUNTAIN_MASK: the same frame without the plume, to measure its outline.
   if(process.env.FOUNTAIN_MASK)await save(`${name}-bare`,await page.evaluate(()=>{const g=__bf.environment.group;g.visible=false;
    const b=__bf;b.environment.reflect(b.camera,b.water);b.graphics.render(b.environment.time);g.visible=true;return b.renderer.domElement.toDataURL();}));
  }
 }
 if(errors.length)throw Error(errors.join('\n'));
}finally{await browser.close();}
