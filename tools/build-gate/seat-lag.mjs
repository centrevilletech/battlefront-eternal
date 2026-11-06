// How many ticks a seated player's own aim and camera lag the mouse in a LIVE
// session (the world runs on its own clock and the page predicts its hull, which
// the gate's lockstep sessions do not). The player walks to a hull, boards it,
// takes seat N, then a scripted mouse swing is fed one step per display frame.
// Per frame the page's seat aim, the soldier's aim and the camera's heading are
// compared with the cumulative mouse counts, and the delay that fits best is
// reported in frames and in world ticks.
//   BFE_GATE_START / BFE_GATE_STOP as for run-case.mjs, CDP_PORT.
//   node tools/build-gate/seat-lag.mjs --dist DIST --map nab1 [--seat 0] [--hull REGEXP] --out FILE
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
import { connect, sleep, writeSettings, SEED_SRC, CONTEXT_SRC, HELPERS_SRC } from '../renderer-corpus/battle.mjs';
import { gateCases } from './cases.mjs';

const args = process.argv.slice(2), arg = (name, fallback) => args.includes(name) ? args[args.indexOf(name) + 1] : fallback;
const dist = arg('--dist'), map = arg('--map', 'nab1'), seatIndex = Number(arg('--seat', '0')), hull = arg('--hull', ''), outFile = arg('--out');
if (!dist || !outFile) throw Error('Supply --dist and --out');
const base = gateCases().find(c => c.id === `cockpit-balanced-${map}`);
if (!base) throw Error('No cockpit case for ' + map);
const sh = (command, extra = []) => command && spawnSync('sh', ['-c', command, '_', ...extra], { stdio: 'inherit' });
sh(process.env.BFE_GATE_START, [dist]);
const report = { dist, map, seatIndex, hull };
const c = await connect();
try {
  writeSettings(base.settings);
  const { identifier } = await c.psend('Page.addScriptToEvaluateOnNewDocument', { source: `${SEED_SRC}${CONTEXT_SRC}localStorage.setItem('battlefront-eternal.settings', ${JSON.stringify(JSON.stringify(base.settings))});` });
  await c.psend('Page.navigate', { url: `${process.env.BFE_APP_URL || 'app://bfe/'}?${base.query}` });
  for (let i = 0; i < 600 && !(await c.ev('Boolean(window.__bf?.frame)').catch(() => false)); i++) await sleep(500);
  await c.psend('Page.removeScriptToEvaluateOnNewDocument', { identifier });
  await c.psend('Emulation.setDeviceMetricsOverride', { width: 1600, height: 900, deviceScaleFactor: 1, mobile: false });
  report.loaded = await c.ev(`(async()=>{const b=__bf;await b.ready;await b.loaded?.();b.begin?.();b.controls.locked=true;b.controls.enabled=true;
    for(let i=0;i<600&&!(b.playerUnit&&b.playerUnit.alive);i++)await new Promise(r=>setTimeout(r,100));
    await new Promise(r=>setTimeout(r,3000));return {alive:!!b.playerUnit?.alive,tick:b.worldClock.tick};})()`);
  await c.ev(HELPERS_SRC);
  // The real display loop runs; wait a frame by waiting for the next callback.
  report.seat = await c.ev(`(async()=>{
    const b=__bf,G=window.__gate,wrap=a=>Math.atan2(Math.sin(a),Math.cos(a)),me=b.playerUnit,frame=()=>new Promise(r=>requestAnimationFrame(()=>setTimeout(r,12)));
    const re=new RegExp(${JSON.stringify(hull)});
    const hulls=()=>b.vehicles.vehicles.filter(v=>v.alive&&re.test(v.name)&&!v.seats.some(s=>s.lease));
    const flat=v=>Math.hypot(v.position.x-me.position.x,v.position.z-me.position.z);
    const target=hulls().sort((a,z)=>(a.flying-z.flying)||flat(a)-flat(z))[0];
    if(!target)return {error:'no such hull'};
    if(flat(target)>150)return {error:'hull too far',at:Math.round(flat(target))};
    const y0=b.soldier.aimYaw;G.look(100,0);for(let i=0;i<8;i++)await frame();const scale=wrap(b.soldier.aimYaw-y0)/100;if(!scale)return {error:'view did not turn'};
    let pressed=false;const t0=performance.now();
    for(let i=0;i<4000;i++){
      if(b.driver?.vehicle||me.seat?.vehicle)break;
      const dx=target.position.x-me.position.x,dz=target.position.z-me.position.z,d=Math.hypot(dx,dz),e=wrap(Math.atan2(dx,dz)-b.soldier.aimYaw);
      G.look(Math.max(-120,Math.min(120,Math.round(0.4*e/scale))),0);G.key('KeyW',d>3);
      if(d<=3.5&&Math.abs(e)<.2){if(!pressed){G.key('KeyE',true);pressed=true;}else{G.key('KeyE',false);pressed=false;}}
      await frame();
    }
    G.key('KeyW',false);G.key('KeyE',false);
    for(let i=0;i<90;i++)await frame();
    const v=b.driver?.vehicle;if(!v)return {error:'could not board '+target.name,at:+flat(target).toFixed(1),yawErr:+wrap(Math.atan2(target.position.x-me.position.x,target.position.z-me.position.z)-b.soldier.aimYaw).toFixed(2),tick:b.worldClock.tick,alive:!!me.alive,secs:+((performance.now()-t0)/1000).toFixed(1)};
    for(let k=0;k<${seatIndex}&&v.seatIndex<${seatIndex};k++){G.key('KeyF',true);await frame();G.key('KeyF',false);for(let i=0;i<30;i++)await frame();}
    for(let i=0;i<60;i++)await frame();
    return {name:v.name,seat:v.seatIndex,seats:v.seats.length,flying:!!v.flying};})()`);
  if (!report.seat.error && report.seat.seat === seatIndex) {
    // One mouse step per display frame, a slow swing so no seat arc clamps it.
    report.samples = await c.ev(`(async()=>{
      const b=__bf,G=window.__gate,frame=()=>new Promise(r=>requestAnimationFrame(()=>setTimeout(r,12))),out=[];let cum=0;
      const yawOf=q=>{const d=new b.THREE.Vector3(0,0,-1).applyQuaternion(q);return Math.atan2(d.x,d.z);};
      for(let f=0;f<720;f++){
        const dx=f<30?0:Math.round(10*Math.sin((f-30)*2*Math.PI/120));
        if(dx){G.look(dx,0);cum+=dx;}
        await frame();
        const v=b.driver.vehicle,s=v.seats[v.seatIndex];
        out.push([f,performance.now(),b.worldClock.tick,cum,s.aimYaw,s.lookYaw??null,b.soldier.aimYaw,yawOf(b.camera.quaternion),s.pitch]);
      }
      return out;})()`);
    report.analysis = {};
    const names = { seatAim: 4, seatLook: 5, soldierAim: 6, cameraYaw: 7 };
    const rows = report.samples.filter(r => r[0] >= 30), ticksPerFrame = (rows.at(-1)[2] - rows[0][2]) / (rows.at(-1)[0] - rows[0][0]);
    for (const [name, col] of Object.entries(names)) {
      if (rows.some(r => r[col] === null)) continue;
      const wrapA = a => Math.atan2(Math.sin(a), Math.cos(a));
      let best = null;
      for (let lag = 0; lag <= 40; lag++) {
        // observed(t) = k * cum(t - lag) + o, least squares over the swing.
        const xs = [], ys = [];
        for (const r of rows) { const src = report.samples[r[0] - lag]; if (!src || r[0] - lag < 30) continue; xs.push(src[3]); ys.push(r[col]); }
        for (let i = 1; i < ys.length; i++) ys[i] = ys[i - 1] + wrapA(ys[i] - ys[i - 1]);
        const n = xs.length, mx = xs.reduce((a, b) => a + b) / n, my = ys.reduce((a, b) => a + b) / n;
        let sxx = 0, sxy = 0; for (let i = 0; i < n; i++) { sxx += (xs[i] - mx) ** 2; sxy += (xs[i] - mx) * (ys[i] - my); }
        const k = sxy / sxx; let err = 0; for (let i = 0; i < n; i++) err += (ys[i] - my - k * (xs[i] - mx)) ** 2;
        if (!best || err < best.err) best = { lagFrames: lag, err: err / n, gainRadPerCount: k };
      }
      report.analysis[name] = { ...best, lagTicks: +(best.lagFrames * ticksPerFrame).toFixed(2) };
    }
    report.ticksPerFrame = +ticksPerFrame.toFixed(3);
    delete report.samples;
  }
} catch (error) { report.error = String(error.stack || error); }
finally { report.console = c.consoleErrors.slice(0, 5); c.close(); sh(process.env.BFE_GATE_STOP); fs.writeFileSync(outFile, JSON.stringify(report, null, 1)); console.log(JSON.stringify({ ...report, console: undefined })); }
