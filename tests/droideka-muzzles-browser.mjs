// PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node tests/droideka-muzzles-browser.mjs
// The world (in its worker) fires the droideka's guns in turn; the page
// draws each bolt the next tick brings from the gun its body shows: the
// drawn tail starts at that muzzle, the round itself left the hidden rig's.
import {mkdirSync,writeFileSync} from 'node:fs';
import {launchBrowser,openBattle} from './battle-page.mjs';
const browser=await launchBrowser({args:['--ignore-gpu-blocklist']});
const deadline=setTimeout(()=>{console.error('Droideka muzzle visual check timed out');process.exit(1);},150000);
try{
 const battle=await openBattle(browser,{base:process.env.GAME_URL || 'http://127.0.0.1:18449',query:'side=cis&unit=cis_inf_droideka',viewport:{width:960,height:600},settings:null});
 const {page,world,errors}=battle,result=[];
 await world.evaluate(()=>{const d=self.__bf.soldier;d.spawn(-51.76,-50.04,-27.93,-Math.PI/2);d.shield.on=false;});
 await battle.frame(1/60);
 for(const first of [false,true])for(const pitch of [0,.35,-.35]){
  await world.evaluate(pitch=>{const d=self.__bf.soldier;d.aimPitch=pitch;d.aimYaw=-Math.PI/2;d.gun=0;},pitch);
  await page.evaluate(first=>{__bf.setPerspective(first);__bf.presentation.reset();},first);
  await battle.frames(70);
  for(let shot=0;shot<4;shot++){
   // Fired straight from the world between ticks, as a tick would: sample
   // the rig, fire, emit. The round reaches the page with the next tick.
   const fired=await world.evaluate(()=>{
    const b=self.__bf,d=b.soldier,c=b.combat,weapon=c.arsenal.current(0);
    d.sampleMuzzles(0);c.aim();c.fire({weapon,yaw:0,pitch:0});c.flushMuzzles();
    return {physical:d.gunAt(new b.THREE.Vector3()).toArray(),selected:{...d.lastGun}};
   });
   result.push(await page.evaluate(async([first,pitch,shot,fired])=>{
    const b=__bf,d=b.soldier,T=b.THREE,c=b.combat,v=b.firstPersonView;
    await b.lockstep(1/60,shot===0);
    const bolt=b.ordnance.bolts.at(-1);if(!bolt)throw new Error('No live bolt');
    const visual=first?v.muzzlePoint(fired.selected,new T.Vector3()):d.visualGunAt(new T.Vector3(),fired.selected);
    const length=b.ordnance.boltSpan(bolt,Math.min(bolt.drawn,bolt.flown));
    const tail=bolt.position.clone().addScaledVector(bolt.offset || new T.Vector3(),Math.max(1-bolt.flown/bolt.merge,0)).addScaledVector(new T.Vector3(...b.ordnance.boltAxis),-length);
    const record={first,pitch,selected:fired.selected,bolt:{index:bolt.muzzleIndex,barrel:bolt.muzzleBarrel,flown:bolt.flown},tailError:tail.distanceTo(visual),physicalOriginError:bolt.position.clone().addScaledVector(bolt.direction,-bolt.flown).distanceTo(new T.Vector3(...fired.physical)),forward:bolt.direction.dot(c.aimDir),screen:visual.clone().project(b.camera).toArray()};
    if(shot===0)record.image=b.renderer.domElement.toDataURL('image/jpeg',.9);
    return record;
   },[first,pitch,shot,fired]));
  }
  if(pitch===0)result.at(-1).travel=await page.evaluate(async()=>{await __bf.lockstep(1/60,true);return __bf.renderer.domElement.toDataURL('image/jpeg',.9);});
 }
 const folder=process.env.MUZZLE_OUTPUT || '/tmp/bf/droideka-muzzles';mkdirSync(folder,{recursive:true});
 for(const [i,r]of result.entries()){
  if(r.image){writeFileSync(`${folder}/${i}.jpg`,Buffer.from(r.image.split(',')[1],'base64'));delete r.image;}
  if(r.travel){writeFileSync(`${folder}/${i}-travel.jpg`,Buffer.from(r.travel.split(',')[1],'base64'));delete r.travel;}
 }
 writeFileSync(`${folder}/metrics.json`,JSON.stringify({result,errors},null,2));
 const maxTail=Math.max(...result.map(r=>r.tailError)),maxOrigin=Math.max(...result.map(r=>r.physicalOriginError));
 // The bolt the page drew is the one the world fired, from the gun it chose.
 const mismatched=result.filter(r=>r.bolt.index!==r.selected.index || r.bolt.barrel!==r.selected.barrel).length;
 console.log({folder,cases:result.length,maxTail,maxOrigin,minForward:Math.min(...result.map(r=>r.forward)),mismatched,errors});
 if(errors.length || mismatched || maxTail>1e-5 || maxOrigin>1e-5)process.exitCode=1;
}finally{clearTimeout(deadline);await browser.close();}
