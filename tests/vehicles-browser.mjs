// Requires extracted nab2, geo1, hot1, and bes2 (or VEHICLE_TEST_MAPS). PLAYWRIGHT_MODULE can point at an external
// Playwright install; no browser package is added to the game's dependency tree.
// The world worker is stepped by the page's frames (tests/battle-page.mjs): seats,
// aims, hooks and the battle's own reads go through the world; keys, frames,
// the camera and the HUD are the page's.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import {launchBrowser,openBattle} from './battle-page.mjs';
const base = process.env.VEHICLE_TEST_URL || 'http://127.0.0.1:18449';
const out = process.env.VEHICLE_TEST_ARTIFACTS || '/tmp/bf-vehicles';
await fs.mkdir(out,{recursive:true});
const browser=await launchBrowser();
try {
 for(const map of (process.env.VEHICLE_TEST_MAPS || 'nab2,geo1,hot1,bes2').split(',')) {
  const errors=[];
  const battle=await openBattle(browser,{base,query:`map=${map}&unit=rep_inf_clone_trooper&shadows=off`,viewport:{width:960,height:600},
   settings:{waterQuality:'low',treeDetail:'original'},errors:()=>false,beforeLoad:page=>{
    page.on('pageerror',e=>errors.push(e.message));
    page.on('console',m=>{if(/GL_INVALID|WebGL.*error/.test(m.text()))errors.push(m.text());else if(m.type()==='error')console.error(`console: ${m.text()}`);});
   }});
  const {page,world}=battle;
  const frames=(count,dt=1/60,draw=false)=>battle.frames(count,dt,draw);
  const initial=await world.evaluate(()=>self.__bf.vehicles.vehicles.filter(v=>v.alive).map(v=>({name:v.name,health:v.health,seats:v.seats.length,position:v.position.toArray(),animated:Boolean(v.pose)})));
  console.log(JSON.stringify({map,initial}));
  if(map==='bes2'){assert.equal(initial.length,0);assert.deepEqual(errors,[]);await battle.close();continue;}
  if(map==='nab2')assert.deepEqual(initial.map(v=>v.name).sort(),['cis_hover_aat','cis_hover_aat','rep_hover_fightertank','rep_hover_fightertank'].sort());
  assert.ok(initial.length >= (map==='nab2' ? 4 : 6));
  const names=[...new Set(initial.map(v=>v.name))];
  // His fire, counted where the world fires it.
  await world.evaluate(()=>{const o=self.__bf.ordnance,fire=o.fire;self.__fired=0;o.fire=function(...args){self.__fired++;return fire.apply(this,args);};});
  for(const name of names) {
   const {entered,flying,seats}=await world.evaluate(name=>{
    const b=self.__bf,v=self.__vehicle=b.vehicles.vehicles.find(v=>v.name===name&&v.alive);b.driver.exit(true);b.combat.reset();
    const entered=b.driver.enter(v);self.__start=v.position.clone();
    return {entered,flying:v.flying,seats:v.seats.length};
   },name);
   await page.evaluate(flying=>{const b=__bf;b.keys.clear();b.keys.add(flying?'Space':'KeyW');},flying);
   await frames(60);
   let landing=null;
   if(flying){
    await page.evaluate(()=>{__bf.keys.clear();__bf.keys.add('ControlLeft');});
    await frames(360);
    const y=await world.evaluate(()=>self.__vehicle.position.y);
    await frames(60);
    landing=await world.evaluate(y=>{const v=self.__vehicle;return {state:v.flightState,altitude:v.altitude,drift:Math.abs(v.position.y-y),health:v.health/v.maxHealth};},y);
    await page.evaluate(()=>{__bf.keys.clear();__bf.keys.add('Space');});
    await frames(60);
   }
   await page.evaluate(()=>__bf.keys.clear());
   await frames(1,1/60,true);
   const moved=await world.evaluate(()=>self.__vehicle.position.distanceTo(self.__start));
   const seatChecks=[];
   for(let seat=0;seat<seats;seat++){
    // The seat, aimed along the hull and up a little, in the world.
    const beforeFire=await world.evaluate(seat=>{
     const b=self.__bf,v=self.__vehicle;
     while(v.seatIndex!==seat)b.driver.nextSeat(v);
     v.aimYaw=v.yaw;v.pitch=.2;v.render(1,0);
     return self.__fired;
    },seat);
    await page.evaluate(()=>{const b=__bf;b.keys.add('Mouse0');b.keys.add('Mouse2');b.pending.firePrimary=true;b.pending.fireSecondary=true;});
    await frames(10,1/120);
    await page.evaluate(()=>__bf.keys.clear());
    const check=await world.evaluate(([seat,beforeFire])=>{
     const b=self.__bf,v=self.__vehicle;
     const shots=[...b.ordnance.bolts,...b.ordnance.rounds].filter(r=>r.owner===b.playerUnit||r.owner===v);
     return {seat,weapons:v.seat.weapons.length,fired:self.__fired-beforeFire,shots:shots.length,selfHit:shots.some(r=>r.impact?.body===v.body),finite:shots.every(r=>r.position.toArray().every(Number.isFinite))};
    },[seat,beforeFire]);
    // The same rounds on the page, drawn as the world's packets bring them.
    check.shown=await page.evaluate(()=>{const b=__bf,v=b.driver.vehicle;return [...b.ordnance.bolts,...b.ordnance.rounds].filter(r=>r.owner===b.playerUnit||r.owner===v).length;});
    seatChecks.push(check);
   }
   const first=await page.evaluate(async()=>{const b=__bf;b.setPerspective(true);await b.lockstep(1/60,true);const first=b.camera.position.toArray();b.setPerspective(false);return first;});
   await world.evaluate(()=>{const b=self.__bf,v=self.__vehicle;while(v.seatIndex!==0)b.driver.nextSeat(v);});
   const shown=await page.evaluate(async()=>{
    const b=__bf;await b.lockstep(1/60,true);const v=b.driver.vehicle;
    const hud={vehicle:Boolean(v)&&b.hud.vehicle===v,health:b.hud.health,visible:b.hud.healthFader.alpha,caseLookup:Boolean(v)&&b.hud.url(v.def.settings.HealthTexture)===b.hud.url(v.def.settings.HealthTexture?.toLowerCase())};
    return {camera:b.camera.position.toArray(),hud,drawCalls:b.renderer.info.render.calls};
   });
   const left=await world.evaluate(()=>{const b=self.__bf,v=self.__vehicle,after=v.position.toArray();const exit=b.driver.exit();return {after,exit,alive:v.alive,bodyCount:b.collision.bodies.length};});
   const result={entered,moved,alive:left.alive,seatChecks,first,after:left.after,camera:shown.camera,exit:left.exit,landing,hud:shown.hud,bodyCount:left.bodyCount,drawCalls:shown.drawCalls};
   console.log(JSON.stringify({map,name,...result}));
   assert.equal(result.entered,true);assert.ok(result.moved>.1,`${name} did not move`);
   assert.equal(result.alive,true);assert.ok(result.first.every(Number.isFinite));assert.ok(result.after.every(Number.isFinite));
   assert.ok(result.seatChecks.every(s=>!s.selfHit&&s.finite));
   assert.ok(result.seatChecks.filter(s=>s.weapons>0).every(s=>s.fired>0),`${name} cannot fire`);
   assert.ok(result.seatChecks.every(s=>s.shown===s.shots),`${name}: the page shows the world's rounds`);
   assert.equal(result.hud.vehicle,true);assert.equal(result.hud.visible,1);assert.equal(result.hud.caseLookup,true);
   if(result.landing){assert.equal(result.landing.state,'LANDED');assert.ok(result.landing.altitude<.05);assert.ok(result.landing.drift<.01);assert.equal(result.landing.health,1);}
   await world.evaluate(()=>{const b=self.__bf,v=self.__vehicle;b.driver.exit(true);b.driver.enter(v);v.pitch=-.15;});
   const capture=await page.evaluate(async()=>{await __bf.lockstep(1/60,true);await __bf.lockstep(0,true);return __bf.renderer.domElement.toDataURL()});
   await fs.writeFile(`${out}/${map}-${name}.png`,Buffer.from(capture.split(',')[1],'base64'));
  }
  const destroyed=await world.evaluate(()=>{
   const b=self.__bf,v=self.__vehicle=b.driver.vehicle;const old=b.battle.reinforcements[b.battle.own];v.damage(Infinity);v.damage(Infinity);
   return {dead:b.combat.dead,reinforcements:old-b.battle.reinforcements[b.battle.own]};
  });
  await page.evaluate(()=>__bf.keys.clear());
  await frames(240);
  const shownUnmounted=await page.evaluate(()=>!__bf.driver.active&&!__bf.combat.mounted);
  const lifecycle=await world.evaluate(destroyed=>{
   const b=self.__bf,v=self.__vehicle;
   const unmounted=!b.driver.active&&!b.combat.mounted;
   b.vehicles.spawnOccupant=null;b.vehicles.updateSpawns(v.slot.desc.respawnTime+1);
   return {...destroyed,unmounted,respawn:v.slot.vehicle.alive};
  },destroyed);
  lifecycle.shownUnmounted=shownUnmounted;
  assert.equal(lifecycle.dead,true);assert.equal(lifecycle.unmounted,true);assert.equal(lifecycle.reinforcements,1);assert.equal(lifecycle.respawn,true);
  assert.equal(lifecycle.shownUnmounted,true,'the page lets go of the seat');
  console.log(JSON.stringify({map,lifecycle,errors}));assert.deepEqual(errors,[]);
  await battle.close();
 }
} finally { await browser.close(); }
