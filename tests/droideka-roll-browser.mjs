// Optional real-map visual check; PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node tests/droideka-roll-browser.mjs
// The world (in its worker) puts him on the plaza; the page's own body is
// then posed through a whole roll, as drawn, with no tick in between.
import {mkdirSync,writeFileSync} from 'node:fs';
import {launchBrowser,openBattle} from './battle-page.mjs';
const browser=await launchBrowser({args:['--ignore-gpu-blocklist']});
const deadline=setTimeout(()=>{console.error('Droideka visual check timed out');process.exit(1);},120000);
try {
 const battle=await openBattle(browser,{base:process.env.GAME_URL || 'http://127.0.0.1:18449',query:'side=cis&unit=cis_inf_droideka',viewport:{width:960,height:600},settings:null});
 const {page,world,errors}=battle;
 await world.evaluate(()=>self.__bf.soldier.spawn(-51.76,-50.04,-27.93,0));
 await battle.frame(1/60);
 const result=await page.evaluate(async()=>{
  const b=__bf,d=b.soldier,a=b.animator,T=b.THREE;b.presentation.reset();await b.lockstep(0,false);
  const shots=[],centers=new T.Box3(),p=new T.Vector3(),bounds=new T.Box3();
  d.mode=12;d.roll=0;a.update(0,d);if(b.bubble)b.bubble.mesh.visible=false;
  for(let i=0;i<=24;i++){
   d.roll=i*Math.PI*2/24;a.update(1/60,d);b.actor.root.updateMatrixWorld(true);bounds.makeEmpty();
   for(let k=0;k<b.actor.body.geometry.attributes.position.count;k++){b.actor.body.getVertexPosition(k,p);bounds.expandByPoint(p);}
   centers.expandByPoint(bounds.getCenter(p));
   if(i%6===0){
    b.camera.position.copy(d.position).add(new T.Vector3(3,1.6,3));b.camera.lookAt(d.position.clone().add(new T.Vector3(0,.75,0)));
    b.renderer.render(b.scene,b.camera);shots.push(b.renderer.domElement.toDataURL('image/jpeg',.85));
   }
  }
  d.mode=13;d.time=0;a.update(1/144,d);b.actor.root.updateMatrixWorld(true);
  b.renderer.render(b.scene,b.camera);shots.push(b.renderer.domElement.toDataURL('image/jpeg',.85));
  return {shots,centerExcursion:centers.getSize(new T.Vector3()).length(),pivot:a.pivot.toArray(),center:a.rollCenter.toArray()};
 });
 const folder=process.env.DROIDEKA_OUTPUT || '/tmp/bf/droideka-centered';mkdirSync(folder,{recursive:true});
 result.shots.forEach((url,i)=>writeFileSync(`${folder}/${i}.jpg`,Buffer.from(url.split(',')[1],'base64')));
 delete result.shots;writeFileSync(`${folder}/metrics.json`,JSON.stringify({...result,errors},null,2));console.log({folder,...result,errors});
 if(errors.length || result.centerExcursion>.18)process.exitCode=1;
} finally {await browser.close();clearTimeout(deadline);}
