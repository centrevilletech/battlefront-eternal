import test from 'node:test';
import assert from 'node:assert/strict';
import { Weapon } from '../src/weapons.js';
import { MountedWeaponPrediction } from '../src/runtime/mounted-weapon-prediction.js';
import { captureWeaponState } from '../src/runtime/network-weapon-state.js';
const def={class:'seat-cannon',base:'cannon',channel:0,RoundsPerClip:6,ammo:5,ShotDelay:.1,ReloadTime:.3,ShotsPerSalvo:1,HeatPerShot:.3,HeatRecoverRate:.4,HeatThreshold:.2};
const make=()=>({state:new Weapon(def,{random:()=>.5}),barrel:0,muzzles:[{}]});
const commands=Array.from({length:90},(_,i)=>({sequence:i+1,tick:i+1,generation:2,down:i<25||i>50?['Mouse0']:[],look:[],actions:i===0?['firePrimary']:i===35?['reload']:[]}));
function unit(entry,turret=false){return {alive:true,generation:2,combat:{dead:false},seat:turret?{turret:{weapon:entry.state,barrel:0}}:{index:0,vehicle:{flying:false,seats:[{weapons:[entry]}]}}};}
test('mounted eligibility replays shared heat, cooldown, ammo and reload and restores exactly',()=>{
 const actual=make(),predicted=make(),u=unit(predicted),replay=new MountedWeaponPrediction();
 const baseline=captureWeaponState(predicted.state);let rng=0,admissions=0;
 const random=predicted.state.random=()=>{rng++;return .2;},admit=predicted.state.admitShot=()=>{admissions++;return false;};
 for(const c of commands)actual.barrel+=actual.state.update(1/60,true,{held:c.down.includes('Mouse0'),pressed:c.actions.includes('firePrimary')},c.actions.includes('reload'),{stick:0,posture:'stand',zoomed:false}).length;
 assert.equal(replay.predict(u,commands,0,[],{}),90);
 assert.deepEqual(captureWeaponState(predicted.state),captureWeaponState(actual.state));assert.equal(predicted.barrel,actual.barrel);
 assert.equal(rng,0);assert.equal(admissions,0);assert.equal(predicted.state.random,random);assert.equal(predicted.state.admitShot,admit);
 replay.restore();assert.deepEqual(captureWeaponState(predicted.state),baseline);assert.equal(predicted.barrel,0);
 replay.predict(u,commands,0,[],{});assert.deepEqual(captureWeaponState(predicted.state),captureWeaponState(actual.state),'repeated frames do not accumulate predictions');
});
test('stationary cooldown becomes ready ahead of correction without emitting an ordnance event',()=>{
 const e=make(),u=unit(e,true),replay=new MountedWeaponPrediction();e.state.wait=.1;
 const pending=commands.slice(0,10).map(c=>({...c,down:[],actions:[]}));
 replay.predict(u,pending,0,[],{});assert.ok(e.state.wait<=0);assert.equal(u.seat.turret.barrel,0);
 u.seat=null;replay.restore();assert.equal(e.state.wait,.1,'old lease state restores even after exit');
});
test('takeoff and landing use the shared vehicle fire-seat freeze policy',()=>{
 const e=make(),u=unit(e),replay=new MountedWeaponPrediction();u.seat.vehicle.flying=true;u.seat.vehicle.flightState='TakingOff';e.state.wait=.5;
 assert.equal(replay.predict(u,commands,0,[],{}),0);assert.equal(e.state.wait,.5);
 u.seat.vehicle.flightState='Flying';replay.predict(u,commands,0,[],{});assert.notEqual(e.state.wait,.5);replay.restore();assert.equal(e.state.wait,.5);
});

test('vehicle fallback muzzle does not advance a missing barrel list',()=>{
 const e=make(),u=unit(e),replay=new MountedWeaponPrediction();e.muzzles=[];
 replay.predict(u,commands.slice(0,1),0,[],{});assert.equal(e.barrel,0);assert.notEqual(e.state.clip,1);
 replay.restore();assert.equal(e.state.clip,1);
});

test('a tow gun whose cable holds takes no trigger in the prediction either (Vehicle.towHeld)',()=>{
 const e=make(),u=unit(e),replay=new MountedWeaponPrediction();
 u.seat.vehicle.towHeld=(entry)=>entry===e;
 const clip=e.state.clip;
 replay.predict(u,commands.slice(0,20),0,[],{});
 assert.equal(e.state.clip,clip,'no round spent while the cable holds');assert.equal(e.barrel,0);
 replay.restore();u.seat.vehicle.towHeld=()=>false;
 replay.predict(u,commands.slice(0,20),0,[],{});
 assert.notEqual(e.state.clip,clip,'free again, the press fires');
 replay.restore();
});
