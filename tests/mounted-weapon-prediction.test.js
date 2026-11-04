import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {gunzipSync} from 'node:zlib';
import {createHash} from 'node:crypto';
import {Weapon} from '../src/weapons.js';
import {loadSim} from '../src/sim/module.js';
import {loadRayKernel} from '../src/ray-kernel.js';
import {createClientPrediction} from '../src/runtime/client-prediction.js';
import {MountedWeaponPrediction} from '../src/runtime/mounted-weapon-prediction.js';
import {captureWeaponState} from '../src/runtime/network-weapon-state.js';
import {actorCourse} from './fixtures/client-actors.js';
const def={class:'seat-cannon',base:'cannon',channel:0,RoundsPerClip:6,ammo:5,ShotDelay:.1,ReloadTime:.3,ShotsPerSalvo:1,HeatPerShot:.3,HeatRecoverRate:.4,HeatThreshold:.2};
const commands=Array.from({length:90},(_,i)=>({sequence:i+1,tick:i+1,generation:2,down:i<25||i>50?['Mouse0']:[],look:[],actions:i===0?['firePrimary']:i===35?['reload']:[]}));

// Reference: 123cb8dc (JavaScript mounted replay).
test('one mounted session matches frozen timers, ammo, heat and barrel checkpoints and restores the authority',async()=>{
 const text=gunzipSync(readFileSync(new URL('./fixtures/prediction-reference/mounted.json.gz',import.meta.url))).toString('utf8').trimEnd();
 assert.equal(createHash('sha256').update(text).digest('hex'),'e9dc569aaa66b0ebba9ae9afa57eebbd8f83942fe74e7b81f2abdd6992b23050');
 const instance=await loadSim(),course=actorCourse(await loadRayKernel(instance)),session=createClientPrediction(instance,course.world);
 try {
  for(const [scenario,frames] of Object.entries(JSON.parse(text))) {
   const entry={state:new Weapon(def,{random:()=>.5}),barrel:0,muzzles:scenario==='no-muzzles'?[]:[{}]},turret={weapon:entry.state,barrel:0};
   const vehicle={flying:scenario==='flyer-frozen',flightState:'TakingOff',seats:[{weapons:[entry]}],towHeld:()=>scenario==='tow'};
   const unit={id:19,alive:true,generation:2,combat:{dead:false},seat:scenario==='stationary'?{turret}:{index:0,vehicle}};
   const replay=new MountedWeaponPrediction(session),baseline=captureWeaponState(entry.state);
   const random=entry.state.random=()=>{throw Error('authoritative random used');},admission=entry.state.admitShot=()=>{throw Error('authoritative admission used');};
   for(let n=1;n<=90;n++) {
    const before=session.counters.calls;
    replay.predict(unit,commands.slice(0,n),0,[],{});
    const actual={tick:replay.tick,bytes:Buffer.from(captureWeaponState(entry.state)).toString('hex'),barrel:scenario==='stationary'?turret.barrel:entry.barrel};
    assert.deepEqual(actual,frames[n-1],`${scenario} frame ${n}`);
    assert.equal(session.counters.calls-before,scenario==='flyer-frozen'?0:1,'one boundary for all pending ticks');
    assert.equal(entry.state.random,random);assert.equal(entry.state.admitShot,admission);
    replay.restore();assert.deepEqual(captureWeaponState(entry.state),baseline);assert.equal(entry.barrel,0);assert.equal(turret.barrel,0);
   }
   replay.dispose();
  }
 } finally {session.dispose();course.dispose();}
});
