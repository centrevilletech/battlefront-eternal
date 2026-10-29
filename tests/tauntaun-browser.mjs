// The tauntaun ridden, as drawn: the world (in its worker) seats him and
// lets him off; the page's keys ride it, frame by frame
// (tests/battle-page.mjs), and the page's rider and mount are measured.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import {launchBrowser,openBattle} from './battle-page.mjs';
const browser=await launchBrowser();
try {
 const battle=await openBattle(browser,{base:process.env.VEHICLE_TEST_URL || 'http://127.0.0.1:18449',query:'map=hot1&unit=rep_inf_clone_trooper&shadows=off',viewport:{width:1100,height:750},settings:null});
 const {page,world,errors}=battle;
 await page.evaluate(()=>{const b=__bf;b.setPerspective(false);b.settings.set('waterQuality','low');b.settings.set('treeDetail','original');});
 const index=await world.evaluate(()=>{const b=self.__bf,v=b.vehicles.vehicles.find(v=>v.name==='all_walk_tauntaun');b.driver.enter(v);return b.vehicles.vehicles.indexOf(v);});
 const result=await page.evaluate(async index=>{
  const b=__bf,v=b.vehicles.vehicles[index];
  const V=b.THREE.Vector3,seat=new V(),pelvis=new V();
  let gap=0,breath=0;const chest=b.actor.byName.bone_ribcage,initialChest=chest.quaternion.clone();
  const times=[],heights=[],sample=()=>{
   v.byName.hp_active.getWorldPosition(seat);b.actor.byName.bone_pelvis.getWorldPosition(pelvis);
   gap=Math.max(gap,seat.distanceTo(pelvis));heights.push(seat.y-v.position.y);
   breath=Math.max(breath,initialChest.angleTo(chest.quaternion));times.push(b.animator.gunning.time);
  };
  for(let i=0;i<120;i++){await b.lockstep(1/60,false);if(i===0)initialChest.copy(chest.quaternion);sample();}
  const idleBob=Math.max(...heights)-Math.min(...heights);const start=v.position.clone();
  b.keys.add('KeyW');let cycles=0,lastPhase=v.walkPhase;const runningImages=[];
  for(let i=0;i<120;i++){
   await b.lockstep(1/60,false);sample();if(v.walkPhase<lastPhase)cycles++;lastPhase=v.walkPhase;
   if(i===89 || i===95 || i===101){
    for(const other of b.vehicles.vehicles)if(other!==v)other.root.visible=false;
    const yaw=v.yaw+Math.PI/2;
    b.camera.position.set(v.position.x+Math.sin(yaw)*8,v.position.y+3,v.position.z+Math.cos(yaw)*8);
    b.camera.lookAt(v.position.x,v.position.y+1.7,v.position.z);b.camera.updateMatrixWorld();b.graphics.render(0);
    runningImages.push(b.renderer.domElement.toDataURL());
   }
  }
  const movingBlend=v.walkBlend,runSpeed=v.velocity.length();
  b.keys.add('ArrowRight');for(let i=0;i<60;i++){await b.lockstep(1/60,false);sample();}
  b.keys.clear();for(let i=0;i<180;i++){await b.lockstep(1/60,false);sample();}
  const skin=b.vehicles.data.models[v.def.model].parts;
  const data={cycles,runSpeed,gap,breath,idleBob,timeRange:Math.max(...times)-Math.min(...times),moved:v.position.distanceTo(start),movingBlend,stoppedBlend:v.walkBlend,skinned:skin.every(p=>p.joints?.length===p.vertexCount),parts:skin.length};
  for(const other of b.vehicles.vehicles)if(other!==v)other.root.visible=false;
  const yaw=v.yaw+Math.PI/2;
  b.camera.position.set(v.position.x+Math.sin(yaw)*7,v.position.y+3.4,v.position.z+Math.cos(yaw)*7);
  b.camera.lookAt(v.position.x,v.position.y+1.9,v.position.z);b.camera.updateMatrixWorld();b.graphics.render(0);
  return {...data,image:b.renderer.domElement.toDataURL(),runningImages};
 },index);
 // Off again in the world; the page shows him on foot from the next tick.
 result.exited=await world.evaluate(()=>self.__bf.driver.exit());
 Object.assign(result,await page.evaluate(async()=>{await __bf.lockstep(1/60,false);return {mounted:__bf.combat.mounted,riding:Boolean(__bf.animator.gunning)};}));
 const {image,runningImages,...metrics}=result;console.log({...metrics,errors});
 assert.ok(result.gap<.001);assert.ok(result.breath>.01);assert.ok(result.idleBob>.005);assert.ok(result.timeRange>5);
 assert.ok(result.moved>20);assert.ok(result.cycles>=2);assert.ok(result.movingBlend>.95);assert.ok(result.stoppedBlend<.01);
 assert.equal(result.skinned,true);assert.equal(result.exited,true);assert.equal(result.mounted,false);assert.equal(result.riding,false);assert.deepEqual(errors,[]);
 for(let i=0;i<runningImages.length;i++)await fs.writeFile(`/tmp/bf-tauntaun-running-${i}.png`,Buffer.from(runningImages[i].split(',')[1],'base64'));
 await fs.writeFile('/tmp/bf-tauntaun-fixed.png',Buffer.from(image.split(',')[1],'base64'));
} finally {await browser.close();}
