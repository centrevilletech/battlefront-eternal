// A slow page must never wait for rendering to free a HUD event queue. This
// probe deliberately withholds publication and throttles the page CPU; it is
// a liveness regression, separate from deterministic pixel and timing gates.
import fs from 'node:fs';
import { connect, loadBattle } from '../renderer-corpus/battle.mjs';
const c = await connect(), report = { pass: false };
try {
  report.loaded = await loadBattle(c, { warm: 0 });
  if (!report.loaded.spawned) throw Error('Battle did not spawn');
  await c.psend('Emulation.setCPUThrottlingRate', { rate: 6 });
  report.stall = await c.ev(`(() => {
    const b=__bf,h=b.hud,input=h.input,publish=input.publish;
    const killer={human:true,name:'A',team:1},victim={human:true,name:'B',team:2};
    let held=0;input.publish=function(){held++;};
    try {
      for(let i=0;i<100000;i++){
        h.setSecondBar('hud_jetpack_icon',i/100000);
        h.setHull('hud_vehiclehealth',i/100000);
        h.setSupply(true,false);h.stats({kills:i,deaths:0,captures:0});
      }
      for(let i=0;i<10000;i++){
        h.message('LOADING','friendly');h.killMessage({killer,victim});
        h.hit('enemy');h.damageFrom(1.2);h.noteShot(b.playerUnit,b.combat.arsenal.current(0));
      }
      for(let i=0;i<600;i++){
        h.setSecondBar('hud_jetpack_icon',i/600);
        h.message('READY','friendly');h.killMessage({killer,victim});
        h.update(1/60,h.posts,h.player,h.battle);
      }
    } finally {input.publish=publish;}
    h.render();
    return {held,updates:600,previews:100000,messages:10600,kills:10600,quads:b.graphics.rust.rust.hud_model_build(0,true)};
  })()`);
  if (report.stall.held !== 600 || !report.stall.quads) throw Error('Stall did not exercise native HUD');
  report.advance = await c.ev(`(async()=>{const b=__bf,start=b.worldClock.tick;for(let i=0;i<4;i++)await b.lockstep(1/60,true);return {start,end:b.worldClock.tick,errors:b.graphics.rust.errors,lost:b.renderer.getContext().isContextLost(),glError:b.renderer.getContext().getError(),threeCalls:b.renderer.battleThreeCalls};})()`);
  const a = report.advance;
  if (a.end < a.start + 4 || a.errors.length || a.lost || a.glError || a.threeCalls || c.consoleErrors.length) throw Error('Battle failed after withheld publication');
  report.pass = true;
} catch (error) {
  report.error = String(error.stack || error); process.exitCode = 1;
} finally {
  await c.psend('Emulation.setCPUThrottlingRate', { rate: 1 });
  report.console = c.consoleErrors;
  if (process.env.OUT) { fs.mkdirSync(process.env.OUT, { recursive: true }); fs.writeFileSync(`${process.env.OUT}/stall.json`, JSON.stringify(report, null, 2)); }
  console.log(JSON.stringify(report)); c.close();
}
