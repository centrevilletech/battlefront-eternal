// Requires Vite plus extracted Naboo/Bespin and Republic/CIS assets.
// The world (in its worker) mans the first turret and fires it while the
// page holds the trigger; the page draws the bolts it sends, in third and
// first person.
import fs from 'node:fs/promises';
import {launchBrowser,openBattle} from './battle-page.mjs';
const base=process.env.TURRET_TEST_URL || 'http://127.0.0.1:18449';
const artifacts=process.env.TURRET_TEST_ARTIFACTS;
const browser=await launchBrowser();
try{for(const map of ['nab2','bes2'])for(const side of ['rep','cis']){
 const battle=await openBattle(browser,{base,query:`map=${map}&side=${side}&unit=${side==='rep'?'rep_inf_clone_trooper':'cis_inf_assault'}&shadows=low`,viewport:{width:640,height:400},settings:null});
 const {page,world,errors}=battle;page.on('console',m=>{if(m.type()!=='error'&&/GL_INVALID_OPERATION/.test(m.text()))errors.push(m.text())});
 await world.evaluate(()=>{const t=self.__bf.live.turrets[0];t.setHealth(t.maxHealth)});
 await page.evaluate(()=>{const t=__bf.live.turrets[0];window.__turretTexture=__bf.ordnance.texture(t.weapon.def.ordnance.LaserTexture)});await page.waitForFunction(()=>window.__turretTexture?.image?.width>0);
 for(const first of [false,true]){
 // At the gun in the world, raised a little, with no rounds in flight; the page shows him there from the next tick.
 await world.evaluate(()=>{const b=self.__bf,t=b.live.turrets[0];b.gunner.enter(t,b.soldier);t.pitch=.05;t.weapon.cooldown=0;b.ordnance.bolts.length=0});
 await page.evaluate(async({first})=>{const b=__bf;b.keys.clear();b.setPerspective(first);b.graphics.settings.set('softFocus',false);await b.lockstep(1/60,false);b.ordnance.renderer.begin();b.ordnance.renderer.end()},{first});
 // The trigger held for 0.24 s, then let go for the drawn frame.
 const r=await page.evaluate(async({first})=>{
  const b=__bf,g=b.graphics;b.keys.add('Mouse0');
  for(let i=0;i<12;i++)await b.lockstep(.02,false);
  b.keys.delete('Mouse0');await b.lockstep(1/60,true);
  const gl=b.renderer.getContext(),capture=()=>{b.water?.reflect(b.camera);g.render(0);const pixels=new Uint8Array(g.sceneTarget.width*g.sceneTarget.height*4);gl.readPixels(0,0,g.sceneTarget.width,g.sceneTarget.height,gl.RGBA,gl.UNSIGNED_BYTE,pixels);return pixels};
  const visible=capture(),image=b.renderer.domElement.toDataURL();for(const batch of b.ordnance.renderer.batches.values())batch.mesh.visible=false;const hidden=capture();let changed=0,red=0;for(let i=0;i<visible.length;i+=4){if(visible[i]-hidden[i]>8){changed++;if(visible[i]>visible[i+1]*1.2)red++;}}
  return {first,manned:b.gunner.active,image,changed,red,camera:b.camera.position.toArray(),projected:b.ordnance.bolts.map(x=>x.position.clone().project(b.camera).toArray()),glError:gl.getError()};
 },{first});
 // The rounds as the world has them (what they hit is the world's).
 r.bolts=await world.evaluate(()=>{const b=self.__bf,t=b.live.turrets[0];return b.ordnance.bolts.map(x=>({life:x.life,flown:x.flown,selfHit:x.impact?.body?.owner===t}))});
 if(artifacts){await fs.mkdir(artifacts,{recursive:true});await fs.writeFile(`${artifacts}/turret-${map}-${side}-${first?'first':'third'}.png`,Buffer.from(r.image.split(',')[1],'base64'));}delete r.image;console.log(JSON.stringify({map,side,...r}));if(r.changed<3||r.glError)throw new Error('Turret bolts not visible');
 }
 console.log(JSON.stringify({map,side,errors}));if(errors.length)throw new Error('Browser errors');await battle.close();
}}finally{await browser.close()}
