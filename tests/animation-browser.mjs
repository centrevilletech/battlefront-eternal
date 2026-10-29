// Optional real-map smoke: PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node tests/animation-browser.mjs
// ANIMATOR_BASELINE=/path/to/animator.js runs the identical route against an older animator.
// The world worker is stepped a frame at a time (tests/battle-page.mjs): keys
// and presses go in as the page's commands, the scripted aim is set on the
// world before each frame, and the page's body is measured as drawn.
import {readFileSync,mkdirSync,writeFileSync} from 'node:fs';
import {launchBrowser,openBattle} from './battle-page.mjs';
const deadline=setTimeout(()=>{console.error('Animation browser smoke exceeded 180 seconds');process.exit(1);},180000);
const browser=await launchBrowser({args:['--ignore-gpu-blocklist']});
try {
const beforeLoad=process.env.ANIMATOR_BASELINE?page=>page.route('**/src/animator.js*',async route=>{
 const response=await route.fetch();const transformed=await response.text();
 const dependency=transformed.match(/import \* as THREE from ("[^"]+")/)[1];
 const source=readFileSync(process.env.ANIMATOR_BASELINE,'utf8').replace('"three"',dependency).replace('"./anim.js"','"/src/anim.js"');
 await route.fulfill({response,body:source});
}):null;
const battle=await openBattle(browser,{base:process.env.GAME_URL || 'http://127.0.0.1:18449',query:'side=cis&unit=cis_inf_assassindroid',viewport:{width:960,height:600},settings:null,beforeLoad});
const {page,world,errors}=battle;
await world.evaluate(()=>self.__bf.soldier.spawn(-51.76,-50.04,-27.93,-Math.PI/2));
await battle.frame(1/60);
await page.evaluate(()=>{
 const b=window.__bf,a=b.animator,T=b.THREE;
 b.presentation?.reset();a.reset(b.soldier.aimYaw);
 window.__animation={lastArm:new T.Quaternion(),lastBody:new T.Quaternion(),arm:a.byName.bone_l_upperarm};
});
const segments=[],snaps=[];
// One segment: keys held, `look(i)` run on the world's soldier before frame i.
async function step(label,frames,keys=[],look=null){
 await page.evaluate(keys=>{const b=window.__bf;b.keys.clear();for(const key of keys)b.keys.add(key);Object.assign(window.__animation,{bodyStep:0,armStep:0,weights:1});},keys);
 const frame=async([label,from,to])=>{
  const b=window.__bf,a=b.animator,m=window.__animation;
  for(let i=from;i<to;i++){
   m.lastArm.copy(m.arm.quaternion);m.lastBody.copy(b.actor.root.quaternion);
   await b.lockstep(1/60,false);
   if(i>0||label.includes('roll')){
    m.bodyStep=Math.max(m.bodyStep,m.lastBody.angleTo(b.actor.root.quaternion)*180/Math.PI);
    m.armStep=Math.max(m.armStep,m.lastArm.angleTo(m.arm.quaternion)*180/Math.PI);
   }
   m.weights=Math.min(m.weights,a.layers.reduce((sum,l)=>sum+l.weight,0));
   if(!b.actor.bones.every(bone=>[...bone.position.toArray(),...bone.quaternion.toArray()].every(Number.isFinite)))throw new Error('non-finite animation pose');
  }
 };
 if(look)for(let i=0;i<frames;i++){await world.evaluate(look,i);await page.evaluate(frame,[label,i,i+1]);}
 else await page.evaluate(frame,[label,0,frames]);
 const {segment,url}=await page.evaluate(async label=>{
  const b=window.__bf,a=b.animator,s=b.soldier,m=window.__animation;
  const segment={label,bodyStep:m.bodyStep,armStep:m.armStep,weights:m.weights,action:a.action,gait:a.gait,posture:s.posture,grounded:s.grounded};
  b.keys.clear();await b.lockstep(0,true);
  return {segment,url:b.renderer.domElement.toDataURL('image/jpeg',0.85)};
 },label);
 segments.push(segment);snaps.push({label,url});
}
const press=name=>page.evaluate(name=>{window.__bf.pending[name]=true;},name);
await step('walk',60,['KeyW']);
await step('turn while walking',60,['KeyW'],()=>{self.__bf.soldier.aimYaw+=0.013;});
await step('pitch while walking',60,['KeyW'],i=>{self.__bf.soldier.aimPitch=Math.sin(i/15)*0.65;});
await step('standing turn',90,[],()=>{self.__bf.soldier.aimYaw+=0.02;});
await press('jump');await step('right roll',25,['KeyD']);
await step('right roll recovery',65,['KeyD']);
await press('jump');await step('left roll',25,['KeyA']);
await step('left roll recovery',65,['KeyA']);
await step('settled',60);
const folder=process.env.ANIMATION_OUTPUT || '/tmp/bf/animation-continuity';mkdirSync(folder,{recursive:true});
for(const [i,snap]of snaps.entries())writeFileSync(`${folder}/${i}.jpg`,Buffer.from(snap.url.split(',')[1],'base64'));
writeFileSync(`${folder}/metrics.json`,JSON.stringify({segments,errors},null,2));
console.log(JSON.stringify({segments,errors,folder}));
if(errors.length)process.exitCode=1;
} finally {
 await browser.close();clearTimeout(deadline);
}
