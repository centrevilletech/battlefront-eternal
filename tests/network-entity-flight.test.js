import test from 'node:test';import assert from 'node:assert/strict';
import {fixture} from './flight-fixture.js';
import {predictionVehicle} from '../src/runtime/vehicle-prediction.js';
import {captureFlightCorrection,restoreFlightCorrection,flightCorrectionBytes} from '../src/runtime/network-entity-flight.js';
import {encodeEntityPacket,decodeEntityPacket} from '../src/runtime/network-entity-codec.js';

test('exact flyer correction fits independently and restores all audited replay fields',()=>{
 const a=fixture().v;a.captureTick();const bytes=captureFlightCorrection(a);
 const packet=encodeEntityPacket({epoch:1,packetId:1,tick:1,records:[{kind:'correction',id:1,generation:1,tick:1,schema:2,payload:bytes}]});
 assert.ok(packet.length<=1100);console.log(`Exact flyer correction: ${flightCorrectionBytes} payload bytes, ${packet.length} complete message bytes`);
 const b=predictionVehicle(a);b.position.addScalar(50);b.power=2;
 const record=decodeEntityPacket(packet).records[0];restoreFlightCorrection(b,record.payload,record.schema);
 assert.deepEqual(captureFlightCorrection(b),bytes);
 assert.throws(()=>captureFlightCorrection({...a,flying:false}),/unsupported/);
 const bad=bytes.slice();new DataView(bad.buffer).setFloat64(0,Infinity,true);assert.throws(()=>restoreFlightCorrection(b,bad));assert.deepEqual(captureFlightCorrection(b),bytes);
});

test('flyer correction resumes takeoff, turn, boost, trick and braking replay without drift',()=>{
 const a=predictionVehicle(fixture().v);a.captureTick();let b;
 for(let tick=1;tick<=360;tick++){
  const input={takeoff:tick===1,forward:tick<250?1:-1,stickX:tick>130?.3:0,stickY:tick>150?.1:0,boost:tick>180&&tick<220,trick:tick===160||tick===166,right:tick>170&&tick<180?.5:0};
  a.captureTick();a.tick(1/60,input);
  if([30,130,170,260].includes(tick)){b=predictionVehicle(fixture().v);b.captureTick();restoreFlightCorrection(b,captureFlightCorrection(a));}
  else if(b){b.captureTick();b.tick(1/60,input);}
  if(b)assert.deepEqual(captureFlightCorrection(b),captureFlightCorrection(a),`flight replay tick${tick}`);
 }
});
