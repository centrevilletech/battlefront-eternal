import { stepPrediction } from './helpers/hull-prediction.js';
import { HullReplay, hullInput } from '../tools/hull-replay-fixture.mjs';
import test from 'node:test';import assert from 'node:assert/strict';
import {fixture} from './flight-fixture.js';
import {captureCompactFlight,restoreCompactFlight,COMPACT_FLIGHT_SCHEMA} from '../src/runtime/network-entity-flight-compact.js';
function fresh(){const v=fixture().v;v.resetPresentationHistory();stepPrediction(v.system,0,v,{});return v;}
// Page continuations must remain independent of the authority pad's life.
// The compact wire body supplies active state; omitted history stays local.
test('compact flyer preserves exact continuation through takeoff, steering, boost, trick and brake',()=>{
 const source=fixture().v,a=new HullReplay(source);let b;const sizes=new Set();
 try {
  for(let tick=1;tick<=420;tick++){
   const command=hullInput(tick,true);if(tick>180&&tick<220)command.down.push('ShiftLeft');
   const current=a.step(command),payload=captureCompactFlight(current);sizes.add(payload.length);
   if([1,30,130,170,260,320].includes(tick)){
    b?.dispose();b=new HullReplay(source);b.restore(payload,tick,command.down,COMPACT_FLIGHT_SCHEMA);
    assert.deepEqual(b.source.tickFrom,current.position);assert.deepEqual(b.source.flightTickQ,current.quaternion);
   }else if(b)b.step(command);
   if(b)assert.deepEqual(captureCompactFlight(b.current),payload,`tick ${tick}`);
  }
 } finally {a.dispose();b?.dispose();}
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
   a.resetPresentationHistory();stepPrediction(a.system,1/60,a,input);
   if(tick===15){assert.equal(a.trick,kind);observed=true;b=fresh();restoreCompactFlight(b,captureCompactFlight(a));}
   else if(b){b.resetPresentationHistory();stepPrediction(b.system,1/60,b,input);}
   if(b)assert.deepEqual(captureCompactFlight(b),captureCompactFlight(a),`${kind} tick ${tick}`);
  }
  assert.ok(observed);assert.equal(a.trick,'');
 }
});
