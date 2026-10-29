// Real-world parity gate. Render draws may consume arbitrary random numbers;
// authoritative state and events must be independent of display cadence.
// The page's frames at each cadence step the world worker
// (tests/battle-page.mjs); the world's systems are counted, and its state
// read, where it runs.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import { launchBrowser, openBattle } from './battle-page.mjs';
const browser = await launchBrowser();
const base = process.env.RUNTIME_AUDIT_URL || 'http://127.0.0.1:18449';
const results=[];
try {
  for (const hz of [30,60,144,240,'jitter']) {
    console.log(`Cadence ${hz}`);
    const battle=await openBattle(browser,{base,query:'map=nab2&unit=rep_inf_clone_trooper&ai=classic&seed=1'});
    const {page,world,errors}=battle;
    await world.evaluate(()=>{
      const b=self.__bf, counts=self.__counts={};
      for(const [obj,key,name] of [[b.combat,'update','combat'],[b.ordnance,'update','ordnance'],[b.posts,'update','posts'],[b.battle,'update','battle'],[b.ai,'update','ai']]) {
        if (!obj) continue;
        const fn=obj[key];obj[key]=function(...args){counts[name]=(counts[name]||0)+1;return fn.apply(this,args);};
      }
    });
    const pageTick=await page.evaluate(async hz=>{
      const b=__bf;
      b.controls.hold('Mouse0');
      const schedule=hz==='jitter'?[1/120,1/120,1/120,.075]:[1/hz];
      let elapsed=0,frame=0;
      while(elapsed<12-1e-10) {
        const dt=Math.min(schedule[frame++%schedule.length],12-elapsed);
        // Presentation entropy must never shift the gameplay random streams.
        for(let i=0;i<frame%7;i++) Math.random();
        await b.lockstep(dt,false);elapsed+=dt;
      }
      return b.worldClock.tick;
    },hz);
    const result=await world.evaluate(hz=>{
      const b=self.__bf;
      return {hz,counts:self.__counts,ticks:b.worldClock.tick,debt:b.worldClock.debt,
        units:b.units.all().filter(u=>u.soldier).map(u=>[u.id,u.generation,u.team,u.alive,...u.position.toArray(),u.health,u.soldier.aimYaw,u.soldier.aimPitch]),
        posts:b.posts.posts.map(p=>[p.team,p.progress]),tickets:b.battle.reinforcements,
        weapons:b.combat.arsenal.weapons.map(w=>[w.clip,w.ammo,w.heat])};
    },hz);
    assert.deepEqual(errors,[]);assert.equal(result.ticks,720);assert.equal(result.debt,0);
    assert.equal(pageTick,720,'the page applied every tick');
    for(const count of Object.values(result.counts)) assert.equal(count,720);
    results.push(result);await battle.close();
  }
  await fs.writeFile('/tmp/bf-runtime-cadence.json',JSON.stringify(results,null,2));
  for (const {hz,...state} of results.slice(1)) {
    const {hz:baseHz,...baseline}=results[0];
    assert.deepEqual(state,baseline,`identical world state at ${baseHz} and ${hz} Hz`);
  }
  console.log(JSON.stringify({pass:true,rates:results.map(r=>r.hz),ticks:720}));
} finally {await browser.close();}
