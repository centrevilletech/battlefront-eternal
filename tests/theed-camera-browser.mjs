// Actual Theed input -> presentation -> camera regression. No GPU/FPS claim:
// manual display schedules isolate camera response from host rendering speed.
// The world worker steps in lockstep with the frames (tests/battle-page.mjs):
// a look goes to the world with the first tick a frame crosses after it and
// shows as the frames pass that tick, so each row follows the turn over the
// frames that show it.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import {launchBrowser,openBattle} from './battle-page.mjs';
const browser=await launchBrowser();
try {
  const battle=await openBattle(browser,{base:process.env.RUNTIME_AUDIT_URL || 'http://127.0.0.1:18449',query:'map=nab2&unit=rep_inf_clone_trooper&ai=off',
    settings:{shadows:'off',antialiasing:'off',waterQuality:'low',treeDetail:'original',environmentEffects:'classic'}});
  const {page,world,errors}=battle, rows=[];
  for(const responsive of [false,true]) for(const hz of [30,60,144,240]) {
    // The world puts him on the plaza; the page shows him there from the next tick.
    await page.evaluate(()=>__bf.controls.clear());
    await world.evaluate(()=>{const s=self.__bf.soldier; s.spawn(-51.76,-50.04,-27.93,Math.PI); s.aimPitch=.15;});
    await battle.frame(1/60);
    rows.push(await page.evaluate(async([responsive,hz])=>{
      const b=__bf, T=b.THREE;
      b.presentation.reset(); b.view.responsive=responsive; b.view.reset(); b.setPerspective(false);
      // From the start of a tick, as a rebased world clock would be.
      const phase=b.client.playerAlpha; await b.lockstep(phase>1e-9?(1-phase)/60:0,false);
      const start=b.worldClock.tick, yaw=b.presentation.state.aimYaw;
      const targetOf=state=>new T.Quaternion().setFromEuler(new T.Euler(state.aimPitch-b.view.tilt,state.aimYaw+Math.PI,0,'YXZ'));
      b.controls.looks.push([120,0]);
      // Until the tick after the look's: by then the whole turn shows.
      let turnError=0, target=null;
      while(b.worldClock.tick<start+2) {
        await b.lockstep(1/hz,false);
        target=targetOf(b.presentation.state);
        turnError=Math.max(turnError,b.camera.quaternion.angleTo(target));
      }
      const turned=b.presentation.state.aimYaw-yaw, position=b.camera.position.clone();
      // Another render opportunity with no new input must not chase the old
      // orbit further (there may still be an independent wall release).
      await b.lockstep(1/hz,false);
      return {responsive,hz,turned,turnError,settledError:b.camera.quaternion.angleTo(target),positionChange:b.camera.position.distanceTo(position)};
    },[responsive,hz]));
  }
  const result={rows,...await page.evaluate(async()=>{
    const b=__bf;
    b.view.responsive=true;
    // Exercise moving/turning through the actual collision world and draw
    // both perspectives to catch camera, shader or transform failures.
    b.controls.hold('KeyW');
    for(let i=0;i<180;i++) {
      b.controls.looks.push([2,0]); await b.lockstep(1/60,false);
      if(!b.camera.position.toArray().every(Number.isFinite)) throw new Error('invalid camera transform');
    }
    b.controls.clear(); await b.lockstep(0,true); b.setPerspective(true); await b.lockstep(0,true);
    return {glError:b.renderer.getContext().getError(),tick:b.worldClock.tick};
  })};
  assert.deepEqual(errors,[]); assert.equal(result.glError,0);
  for(const row of result.rows) {
    assert.ok(Math.abs(row.turned)>.2,`the look reaches the drawn aim: ${JSON.stringify(row)}`);
    if(row.responsive) {assert.ok(row.turnError<1e-6,JSON.stringify(row));assert.ok(row.settledError<1e-6,JSON.stringify(row));}
    else assert.ok(row.turnError>.05,'the reference must reproduce third-person rotation lag');
  }
  await fs.writeFile('/tmp/bf-theed-camera.json',JSON.stringify(result,null,2));
  console.log(JSON.stringify({pass:true,...result}));
} finally { await browser.close(); }
