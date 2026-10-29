// Operator-owned Bespin geometry and real TIE flight solver. No capacity, GPU,
// or transport qualification; this isolates exact semantic correction replay.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {resolve,sep} from 'node:path';
import {execFileSync} from 'node:child_process';
import {createGame} from '../src/game.js';
import {predictionVehicle} from '../src/runtime/vehicle-prediction.js';
import {captureFlightCorrection,restoreFlightCorrection,flightCorrectionBytes} from '../src/runtime/network-entity-flight.js';
import {encodeEntityPacket,decodeEntityPacket} from '../src/runtime/network-entity-codec.js';
const root=resolve(process.env.BFE_DATA_DIR||'public');
globalThis.fetch=async path=>{const filename=resolve(root,`.${path}`);assert.ok(filename.startsWith(root+sep));try{return new Response(await readFile(filename));}catch(error){if(error.code==='ENOENT')return new Response('',{status:404});throw error;}};
const game=await createGame({headless:true,query:'?map=bes1&side=all&ai=classic&seed=1'});
const a=game.vehicles.vehicles.find(v=>v.flying&&/tie/i.test(v.name));assert.ok(a,'Bespin contains an actual TIE');
a.driver={};a.seatIndex=0;for(const seat of a.seats)seat.lease=null;a.captureTick();
const initial=predictionVehicle(a),states=new Set(),checkpoints=[30,120,180,260];let b,maxPositionError=0,bytes=0;
for(let tick=1;tick<=360;tick++){
 const input={takeoff:tick===1,forward:tick<260?1:-1,stickX:tick>160?.08:0,stickY:0,boost:tick>180&&tick<220,trick:tick===230||tick===236,right:0};
 a.captureTick();a.tick(1/60,input);states.add(a.flightState);
 if(checkpoints.includes(tick)){
  const wire=encodeEntityPacket({epoch:1,packetId:tick,tick,records:[{kind:'correction',id:1,generation:1,tick,schema:2,payload:captureFlightCorrection(a)}]});bytes=wire.length;
  b=predictionVehicle(initial);const record=decodeEntityPacket(wire).records[0];restoreFlightCorrection(b,record.payload,record.schema);
 }else if(b){b.captureTick();b.tick(1/60,input);}
 if(b){maxPositionError=Math.max(maxPositionError,a.position.distanceTo(b.position));assert.deepEqual(captureFlightCorrection(b),captureFlightCorrection(a),`actual TIE replay tick${tick}`);}
 assert.ok(a.alive,'actual TIE remains alive');
}
assert.ok(states.has('Flying'));
console.log(JSON.stringify({source:{revision:execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),dirty:!!execFileSync('git',['status','--porcelain'],{encoding:'utf8'}).trim()},node:process.version,map:'bes1',vehicle:a.name,ticks:360,checkpoints,states:[...states],payloadBytes:flightCorrectionBytes,packetBytes:bytes,maxPositionError,alive:a.alive,scope:'exact semantic correction with owned geometry; not transport or player capacity'},null,2));
