// Mans one turret of each class on the given maps and writes third- and
// first-person screenshots (HUD included) to TURRET_PROBE_OUT.
import fs from 'node:fs/promises';
import {launchBrowser,openBattle} from '../tests/battle-page.mjs';
const base=process.env.TURRET_TEST_URL||'http://127.0.0.1:18493';
const out=process.env.TURRET_PROBE_OUT||'/tmp/bfe-turret-probe';
const maps=(process.argv[2]||'kas2,hot1').split(',');
const cw={side:'rep',unit:'rep_inf_clone_trooper'},gcw={side:'imp',unit:'imp_inf_storm_trooper'},snow={side:'imp',unit:'imp_inf_storm_troopersnow'};
const eras={hot1:snow,tat2:gcw,yav1:gcw,end1:gcw,bes2:gcw,tat1:cw,kas2:cw,geo1:cw,bes1:gcw,nab2:cw,kam1:cw,rhn1:gcw,rhn2:gcw,nab1:cw,yav2:gcw};
await fs.mkdir(out,{recursive:true});
const browser=await launchBrowser();
try{for(const map of maps){const {side,unit}=eras[map]||cw;
 const battle=await openBattle(browser,{base,query:`map=${map}&side=${side}&unit=${unit}&shadows=low`,viewport:{width:960,height:540},settings:null});
 const {page,world}=battle;
 for(let i=0;i<40&&!(await world.evaluate(()=>self.__server.actor.unit.alive));i++)await battle.frames(30);
 const classes=await world.evaluate(()=>{const b=self.__bf,seen={};b.live.turrets.forEach((t,i)=>{const c=t.desc.class;if(!(c in seen))seen[c]=i});return seen});
 console.log(map,classes);
 for(const [cls,index] of Object.entries(classes)){
  await world.evaluate((i)=>{const b=self.__bf,t=b.live.turrets[i];const u=self.__server.actor.unit;t.setHealth(t.maxHealth);b.turretSeats.leave(u,{placement:false});b.turretSeats.man(u,t)},index);
  for(const first of [false,true]){
   await page.evaluate(async({first})=>{const b=__bf;b.keys.clear();b.setPerspective(first);for(let i=0;i<30;i++)await b.lockstep(1/60,i===29)},{first});
   // A bot target 25 m out along the view.

   const info=await page.evaluate(()=>{const b=__bf,t=b.gunner.turret;return {manned:b.gunner.active,cam:b.camera.position.toArray().map(v=>+v.toFixed(2)),turret:t?.position.toArray().map(v=>+v.toFixed(2)),facing:t?.facing,hides:t?.hidesGunner}});
   await page.evaluate(()=>window.__loading?.close?.());
   await page.screenshot({path:`${out}/${map}-${cls}-${first?'first':'third'}.png`});
   console.log(JSON.stringify({map,cls,first,...info}));
  }
  await world.evaluate(()=>{self.__bf.turretSeats.leave(self.__server.actor.unit)});
  await page.evaluate(async()=>{for(let i=0;i<3;i++)await __bf.lockstep(1/60,false)});
 }
 console.log(JSON.stringify({map,errors:battle.errors.slice(0,5)}));await battle.close();
}}finally{await browser.close()}
