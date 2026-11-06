// Frozen-build HUD capture. Every role opens a battle and retains GPU draws;
// scalar fixture commands exercise presentation states without changing world
// authority. Shell surfaces have a separate owner and their own pixel corpus.
import fs from 'node:fs';
import { gzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import { cases, states } from './manifest.mjs';
import { connect, loadBattle, HELPERS_SRC } from '../renderer-corpus/battle.mjs';
const id = process.env.BFE_CORPUS_CASE, testCase = cases.find(c => c.id === id);
if (!testCase) throw Error(`Unknown HUD case ${id}`);
const out = process.env.OUT; fs.mkdirSync(out, { recursive: true });
const report = { id, pass: false, complete: false, points: [] }, c = await connect();
try {
  report.loaded = await loadBattle(c, { settings: testCase.settings, query: testCase.query });
  if (!report.loaded.spawned) throw Error(`Player not spawned: ${JSON.stringify(report.loaded)}`);
  if (!(await c.ev('__gateRandom?.uuidCalls'))) throw Error('Capture requires a build with retained function names');
  await c.ev(HELPERS_SRC);
  // All fixture objects are built once, before the measured frame. Real game
  // weapons supply authored art and font pages; the synthetic seat supplies
  // controlled values for pips, reload/heat, spread and flight prompt states.
  report.fixture = await c.ev(`(() => {
    const b=__bf,h=b.hud,V=b.THREE.Vector3;
    const source=b.combat.arsenal.current(0),def={...source.def};
    const weapon={...source,def,loaded:7,spare:42,reserve:3,bottomless:false,reloading:true,reloadLeft:1.2,heat:.6,overheated:false,wait:0,lockTargetId:-1,lockElapsed:0,kick:0,levels:[{MaxPressedTime:1}],spread(){return [2,3];}};
    weapon.def.LockOffAngle=8;weapon.def.ReloadTime=2;weapon.def.RoundsPerClip=20;weapon.def.StandStillSpread=0;weapon.def.YawSpread=2;weapon.def.PitchSpread=3;
    const secondary={...weapon,def:{...weapon.def,ReticuleTexture:null,IconTexture:null},bottomless:true,heat:.4,reloading:false};
    const seats=[{settings:{da1b8c0c:'common.vehiclepositions.pilot'},lease:{unit:b.playerUnit}},{lease:{unit:{character:{}}}},{lease:{unit:{}}},{lease:null},{lease:null}];
    const vehicle={flying:true,alive:true,seatIndex:0,seats,flightState:'Landed',flightWarning:null,landingReason:null};
    const arsenal={current:ch=>ch?secondary:weapon};
    const p=h.posts.posts.find(p=>p.active!==false&&p.mapped!==false&&p.team!==h.player.team)||h.posts.posts[0],pos=p.position;
    const fixtureBattle={own:b.battle.own,foe:b.battle.foe,reinforcements:{...b.battle.reinforcements},start:{...b.battle.start},bleed:{...b.battle.bleed},countdown:12.4,losing:true,gameOver:false};
    const captureCounts=new Map([[h.player.team,1],[p.team,1]]);
    const realSeats=[],seenVehicles=new Set();
    for(const v of b.vehicles?.vehicles||[]){if(!v.alive||seenVehicles.has(v.name))continue;seenVehicles.add(v.name);for(let seat=0;seat<v.seats.length;seat++)realSeats.push({vehicle:v,index:seat,label:'vehicle:'+v.name+':seat'+seat});}
    realSeats.sort((a,b)=>a.label.localeCompare(b.label));
    const F=window.__hudFixture={scenario:0,prior:-1,weapon,secondary,vehicle,arsenal,source,capture:p,captureTeam:p.team,captureCounts,fixtureBattle,realSeats,player:{position:new V(pos.x,pos.y,pos.z),yaw:.7,team:h.player.team,dead:false,speed:55},layer:{battlefield:{team:h.player.team,rings:[{x:pos.x,z:pos.z,size:h.postIconSize,alpha:.8}]},teams:{own:b.battle.own,foe:b.battle.foe,flip:.35}},overlay:{hidesHud:true}};
    const original=h.render;
    h.render=function(){const s=F.scenario;
      if(s!==F.prior){
        if(F.prior===4)this.toggleMap();this.setLayer(null);this.setOverlay(null);
        if(s===1){this.message('OBJECTIVE SECURED','friendly');this.stats({kills:2,deaths:0,captures:1});this.squadCommand('follow');this.damageFrom(1.2);}
        if(s===2){this.hit('enemy');this.hit('friendly');}
        if(s===4)this.toggleMap();F.prior=s;
      }
      if(s>0){
        F.player.dead=s===5;
        F.capture.team=s===2?0:F.captureTeam;F.capture.neutralize=.5;F.capture.progress=.4;F.capture.toward=F.captureTeam;F.capture.contested=true;F.capture.counts=F.captureCounts;
        this.setVehicle(s===3?F.vehicle:null);this.setPlayer(s===3?F.arsenal:this.arsenal,.35);this.setHull(s===3?'hud_vehiclehealth':null,.4);
        this.setSecondBar('hud_jetpack_icon',.2,'pulse');this.setSupply(s===1,s===1);this.setLeaving(s===1?.7:0);
        if(s===2){this.setScope(F.source.def.ScopeTexture||'hud1');this.setReticle(.43,F.weapon);this.setReticleTarget('enemy',{x:.6,y:.4,progress:.6},'friendly');this.enemyHealth({alive:true,health:40,maxHealth:100,classDef:{HealthTexture:'hud_health'}});}
        if(s===3){this.setScope(null);this.setReticle(.46,F.weapon,.52,[F.weapon,F.secondary]);this.setReticleTarget('enemy',null,'friendly');}
        if(s===6)this.setLayer(F.layer);if(s===7)this.setOverlay(F.overlay);
        if(s>=8){const seat=F.realSeats[s-8],v=seat.vehicle;v.seatIndex=seat.index;
          this.setLayer(null);this.setOverlay(null);this.setScope(null);this.setSecondBar(null);this.setSupply(false,false);this.setLeaving(0);
          this.setVehicle(v);this.setPlayer(v.arsenal,.65);this.setHull(v.def.settings.HealthTexture||'hud_vehiclehealth',.4);
          this.setReticle(.45,v.current(0),.5,[v.current(0),v.current(1)]);this.setReticleTarget('enemy',null,'friendly');
        }
        this.update(1/60,this.posts,F.player,s===1?F.fixtureBattle:this.battle);
      }
      return original.call(this);
    };
    return {weapon:def.label,posts:h.posts.posts.length,units:b.units.all().length,vehicleSeats:realSeats.map(s=>s.label)};
  })()`);
  const captureStates = process.env.BFE_HUD_VEHICLES === '1' ? states.concat(report.fixture.vehicleSeats) : states;
  report.states = captureStates;
  for (let k = 0; k < captureStates.length; k++) {
    await c.ev(`(async()=>{const b=__bf,g=b.graphics;__hudFixture.scenario=${k};for(const w of g.cascadeWindows||[])w.invalidate();g.upscaler?.invalidate();for(let i=0;i<5;i++)await b.lockstep(0,true);})()`);
    const raw = await c.ev(`(async()=>{const b=__bf,R=b.renderer,gl=R.getContext();__gate.resetFrame();await b.lockstep(0,true);R.setRenderTarget(null);const w=gl.drawingBufferWidth,h=gl.drawingBufferHeight,buf=new Uint8Array(w*h*4);gl.readPixels(0,0,w,h,gl.RGBA,gl.UNSIGNED_BYTE,buf);let text='';for(let i=0;i<buf.length;i+=32768)text+=String.fromCharCode.apply(null,buf.subarray(i,i+32768));return {w,h,data:btoa(text),tick:b.worldClock.tick,draws:__gate.rustDraws(),errors:b.graphics.rust.errors,battleThreeCalls:R.battleThreeCalls,contextLost:gl.isContextLost(),error:gl.getError(),events:__gateGl};})()`);
    if (raw.contextLost || raw.error || raw.errors.length || raw.battleThreeCalls || raw.draws <= 0 || raw.events.lost) throw Error(`Invalid frame ${JSON.stringify(raw)}`);
    const pixels = Buffer.from(raw.data, 'base64'); delete raw.data;
    raw.state = captureStates[k]; raw.file = `${id}-${k}.rgba.gz`; raw.hash = createHash('sha256').update(pixels).digest('hex');
    fs.writeFileSync(`${out}/${raw.file}`, gzipSync(pixels)); report.points.push(raw);
  }
  report.complete = report.points.length === captureStates.length; report.pass = report.complete;
} catch (error) { report.error = String(error.stack || error); process.exitCode = 1; }
finally { report.console = c.consoleErrors; c.close(); fs.writeFileSync(`${out}/${id}.json`, JSON.stringify(report, null, 2)); console.log(JSON.stringify({ id, pass: report.pass, error: report.error })); }
