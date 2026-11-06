import { HullReplay, hullInput } from '../tools/hull-replay-fixture.mjs';
import test from 'node:test';import assert from 'node:assert/strict';
import {fixture} from './flight-fixture.js';
import {presentationHull} from '../src/runtime/client-hull.js';
import {captureFlightCorrection,restoreFlightCorrection,flightCorrectionBytes} from '../src/runtime/network-entity-flight.js';
import {encodeEntityPacket,decodeEntityPacket} from '../src/runtime/network-entity-codec.js';

test('exact flyer correction fits independently and restores all audited replay fields',()=>{
 const a=fixture().v;a.resetPresentationHistory();const bytes=captureFlightCorrection(a);
 const packet=encodeEntityPacket({epoch:1,packetId:1,tick:1,records:[{kind:'correction',id:1,generation:1,tick:1,schema:2,payload:bytes}]});
 assert.ok(packet.length<=1100);console.log(`Exact flyer correction: ${flightCorrectionBytes} payload bytes, ${packet.length} complete message bytes`);
 const b=presentationHull(a);b.position.addScalar(50);b.power=2;
 const record=decodeEntityPacket(packet).records[0];restoreFlightCorrection(b,record.payload,record.schema);
 assert.deepEqual(captureFlightCorrection(b),bytes);
 assert.throws(()=>captureFlightCorrection({...a,flying:false}),/unsupported/);
 const bad=bytes.slice();new DataView(bad.buffer).setFloat64(0,Infinity,true);assert.throws(()=>restoreFlightCorrection(b,bad));assert.deepEqual(captureFlightCorrection(b),bytes);
});

// A correction resumes the private page solver, without authority pad respawns.
// Native world damage/lifecycle tests cover the separate fleet owner.
test('flyer correction resumes takeoff, turn, boost, trick and braking replay without drift',()=>{
 const source=fixture().v,a=new HullReplay(source);let b;
 try {
  for(let tick=1;tick<=360;tick++){
   const command=hullInput(tick,true);if(tick>180&&tick<220)command.down.push('ShiftLeft');
   const current=a.step(command);
   if([30,130,170,260].includes(tick)){b?.dispose();b=new HullReplay(source);b.restore(captureFlightCorrection(current),tick,command.down);}
   else if(b)b.step(command);
   if(b)assert.deepEqual(captureFlightCorrection(b.current),captureFlightCorrection(current),`flight replay tick${tick}`);
  }
 } finally {a.dispose();b?.dispose();}
});
