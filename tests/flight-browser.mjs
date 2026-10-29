import fs from 'node:fs/promises';
import {launchBrowser,openBattle} from './battle-page.mjs';
const base=process.env.FLIGHT_TEST_URL||'http://127.0.0.1:18612',out=process.env.FLIGHT_TEST_ARTIFACTS||'/tmp/bf-flight-acceptance';await fs.mkdir(out,{recursive:true});
const browser=await launchBrowser();
try{
 const errors=[];
 let result={results:[]};
 if(!process.env.FLIGHT_MAP_ONLY){
 const page=await browser.newPage({viewport:{width:640,height:400}});
 page.on('pageerror',e=>{errors.push(e.message);console.error('Browser error:',e.message);});
 await page.route('**/@vite/client',r=>r.fulfill({contentType:'application/javascript',body:''}));
 await page.route('**/flight-harness',r=>r.fulfill({contentType:'text/html',body:'<html><body style="margin:0;background:#081020"></body></html>'}));
 await page.goto(`${base}/flight-harness`);
 result=await page.evaluate(async()=>{
  const {flightAcceptance}=await import('/tests/flight-acceptance.js');const THREE=await import('/node_modules/three/build/three.module.js');const {VehicleCamera}=await import('/src/vehicles.js');
  const renderer=new THREE.WebGLRenderer({preserveDrawingBuffer:true});renderer.setSize(640,400);document.body.append(renderer.domElement);const captures=[];
  const results=flightAcceptance((f,cam)=>{
   const geometry=new THREE.ConeGeometry(2,5,4);geometry.rotateX(Math.PI/2);f.v.root.add(new THREE.Mesh(geometry,new THREE.MeshNormalMaterial()));
   const floor=new THREE.Mesh(new THREE.PlaneGeometry(2000,2000,20,20),new THREE.MeshBasicMaterial({color:0x436177,wireframe:true}));floor.rotation.x=-Math.PI/2;f.system.scene.add(floor);
   f.v.present(1);if(!cam){cam=new THREE.PerspectiveCamera(70,1.6,.03,10000);new VehicleCamera(cam,f.c).update(f.v,1/60,false);}
   renderer.render(f.system.scene,cam);captures.push(renderer.domElement.toDataURL());
  });
  return {results,captures};
 });
 for(let i=0;i<result.captures.length;i++)await fs.writeFile(`${out}/scene-${i+1}.png`,Buffer.from(result.captures[i].split(',')[1],'base64'));
 console.log(JSON.stringify(result.results,null,2));await fs.writeFile(`${out}/results.json`,JSON.stringify({results:result.results,errors},null,2));
 await page.close();
 }
 // The imported battle: the world (in its worker) seats him and lets him
 // go; the page sends his presses and draws the launch.
 const battle=await openBattle(browser,{base,query:'map=bes1&unit=rep_inf_clone_trooper&ai=off&shadows=off&flightDebug',viewport:{width:640,height:400},settings:null,timeout:240000,
  errors:()=>false,beforeLoad:page=>page.on('pageerror',e=>{errors.push(e.message);console.error('Browser error:',e.message);})});
 const {page,world}=battle;
 const boarded=await world.evaluate(()=>{const b=self.__bf;b.combat.reset();const v=self.__craft=b.vehicles.vehicles.find(v=>v.flying&&v.alive);return {entered:b.driver.enter(v),name:v.name}});
 const capture=await page.evaluate(async()=>{const b=__bf;b.menu?.close?.();b.pending.takeoff=true;for(let i=0;i<100;i++)await b.lockstep(1/60,false);await b.lockstep(0,true);return b.renderer.domElement.toDataURL()});
 const imported={...boarded,...await world.evaluate(()=>{
  const b=self.__bf,v=self.__craft;
  const launch={state:v.flightState,speed:v.airspeed,health:v.health,maxHealth:v.maxHealth,warning:v.flightWarning,tick:b.worldClock?.tick};
  // E in flight bails him out beside the craft, at rest, to fall (src/boarding.js).
  const exitAirborne=b.driver.exit();
  const bailed={active:b.driver.active,mounted:b.combat.mounted,seat:Boolean(b.playerUnit.seat),air:b.soldier.air,velocity:b.soldier.velocity.length(),
    height:b.soldier.position.y-v.position.y,clear:Math.hypot(b.soldier.position.x-v.position.x,b.soldier.position.z-v.position.z),radius:v.radius};
  // A hull destroyed under him lets him go.
  b.respawn(0);b.combat.reset();const w=b.vehicles.vehicles.find(x=>x.flying&&x.alive&&x!==v)||v;b.driver.enter(w);w.damage(Infinity);
  const destroyed={active:b.driver.active,mounted:b.combat.mounted,seat:Boolean(b.playerUnit.seat),velocity:b.soldier.velocity.length(),dead:b.combat.dead};
  return {...launch,exitAirborne,bailed,destroyed};
 })};
 // Back on foot, he walks: W pressed once the page shows his new life (the
 // seat it lets go of then clears its keys, and the world takes no command
 // stamped for the life before).
 await world.evaluate(()=>{const b=self.__bf;b.respawn(0);b.combat.reset()});
 await battle.frame(1/60);
 const start=await world.evaluate(()=>self.__bf.soldier.position.toArray());
 await page.evaluate(async()=>{const b=__bf;b.keys.add('KeyW');for(let i=0;i<60;i++)await b.lockstep(1/60,false);b.keys.clear()});
 imported.respawn=await world.evaluate(start=>{const b=self.__bf;return {active:b.driver.active,mounted:b.combat.mounted,seat:Boolean(b.playerUnit.seat),speed:b.soldier.velocity.length(),moved:b.soldier.position.distanceTo(new b.THREE.Vector3(...start))}},start);
 await fs.writeFile(`${out}/imported.png`,Buffer.from(capture.split(',')[1],'base64'));
 console.log(JSON.stringify({imported,errors}));await fs.writeFile(`${out}/imported.json`,JSON.stringify({imported,errors},null,2));

 const bail=imported.bailed;
 if(errors.length||result.results.some(r=>r.status!=='pass')||imported.state!=='Flying'||!imported.exitAirborne||bail.active||bail.mounted||bail.seat||bail.velocity>0||bail.height>1||bail.clear<bail.radius||imported.destroyed.active||imported.destroyed.mounted||imported.destroyed.seat||imported.destroyed.velocity>0||imported.respawn.active||imported.respawn.mounted||imported.respawn.seat||imported.respawn.moved<1||imported.respawn.speed>20)process.exitCode=1;
}finally{await browser.close();}
