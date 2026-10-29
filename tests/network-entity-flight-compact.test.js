import test from 'node:test';import assert from 'node:assert/strict';
import {fixture} from './flight-fixture.js';
import {predictionVehicle} from '../src/runtime/vehicle-prediction.js';
import {captureCompactFlight,restoreCompactFlight} from '../src/runtime/network-entity-flight-compact.js';
function fresh(){const v=predictionVehicle(fixture().v);v.captureTick();return v;}
test('compact flyer preserves exact continuation through takeoff, steering, boost, trick and brake',()=>{
 const a=fresh();let b;const sizes=new Set();
 for(let tick=1;tick<=420;tick++){
  const input={takeoff:tick===1,forward:tick<300?1:-1,stickX:tick>130?.3:0,stickY:tick>150?.1:0,boost:tick>180&&tick<220,trick:tick===160||tick===166,right:tick>170&&tick<180?.5:0};
  a.captureTick();a.tick(1/60,input);
  if([1,30,130,170,260,320].includes(tick)){
   b=fresh();for(const value of Object.values(b))if(value?.isVector3)value.set(71,-53,19);
   restoreCompactFlight(b,captureCompactFlight(a));assert.deepEqual(b.tickFrom,a.position);assert.deepEqual(b.flightTickQ,a.quaternion);
  }else if(b){b.captureTick();b.tick(1/60,input);}
  const payload=captureCompactFlight(a);sizes.add(payload.length);if(b)assert.deepEqual(captureCompactFlight(b),payload,`tick ${tick}`);
 }
 assert.ok(Math.max(...sizes)<500);console.log('Exact compact flyer body sizes:',[...sizes]);
});
test('every conditional trick/transition layout restores without stale history or partial mutation',()=>{
 for(const state of ['Landed','TakingOff','Flying','Landing','Destroyed'])for(const trick of ['','barrel','loop','immelmann']){
  const a=fresh(),b=fresh();a.flightState=state;a.trick=trick;a.trickTime=.25;a.flightStart.set(1,2,3);a.flightEnd.set(4,5,6);a.flightTrickRight.set(.8,0,.6);
  const payload=captureCompactFlight(a);restoreCompactFlight(b,payload);assert.deepEqual(captureCompactFlight(b),payload);
  const bad=payload.slice();bad[0]=255;assert.throws(()=>restoreCompactFlight(b,bad));assert.deepEqual(captureCompactFlight(b),payload);
 }
});
test('active barrel and immelmann conditional frames survive restore through completion',()=>{
 for(const kind of ['barrel','immelmann']){
  const a=fresh();a.flightState='Flying';a.position.y=100;a.airspeed=a.flightConfig.cruiseSpeed;let b, observed=false;
  for(let tick=1;tick<=180;tick++){
   const input={forward:1,trick:kind==='barrel'&&tick===1,right:kind==='barrel'?.7:0,immelmann:kind==='immelmann'&&tick===1,stickX:.1};
   a.captureTick();a.tick(1/60,input);
   if(tick===15){assert.equal(a.trick,kind);observed=true;b=fresh();restoreCompactFlight(b,captureCompactFlight(a));}
   else if(b){b.captureTick();b.tick(1/60,input);}
   if(b)assert.deepEqual(captureCompactFlight(b),captureCompactFlight(a),`${kind} tick ${tick}`);
  }
  assert.ok(observed);assert.equal(a.trick,'');
 }
});
