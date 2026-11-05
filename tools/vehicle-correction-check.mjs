// Operator-owned Bespin geometry and real TIE flight solver. No capacity, GPU,
// or transport qualification; this isolates exact semantic correction replay.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {resolve,sep} from 'node:path';
import {execFileSync} from 'node:child_process';
import {createGame} from '../src/game.js';
import {HullReplay,hullInput} from './hull-replay-fixture.mjs';
import {captureFlightCorrection,flightCorrectionBytes} from '../src/runtime/network-entity-flight.js';
import {encodeEntityPacket,decodeEntityPacket} from '../src/runtime/network-entity-codec.js';
const root=resolve(process.env.BFE_DATA_DIR||'public');
globalThis.fetch=async path=>{const filename=resolve(root,`.${path}`);assert.ok(filename.startsWith(root+sep));try{return new Response(await readFile(filename));}catch(error){if(error.code==='ENOENT')return new Response('',{status:404});throw error;}};
const game=await createGame({headless:true,query:'?map=bes1&side=all&ai=classic&seed=1'});
const a=game.vehicles.vehicles.find(v=>v.flying&&/tie/i.test(v.name));assert.ok(a,'Bespin contains an actual TIE');
const replay=new HullReplay(a),states=new Set(),checkpoints=[30,120,180,260];let resumed,maxPositionError=0,bytes=0;
try {
 for(let tick=1;tick<=360;tick++){
  const command=hullInput(tick,true),current=replay.step(command);states.add(current.flightState);
  if(checkpoints.includes(tick)){
   const payload=captureFlightCorrection(current),wire=encodeEntityPacket({epoch:1,packetId:tick,tick,records:[{kind:'correction',id:1,generation:1,tick,schema:2,payload}]});
   bytes=Math.max(bytes,wire.length);resumed?.dispose();resumed=new HullReplay(a);
   const record=decodeEntityPacket(wire).records[0];resumed.restore(record.payload,tick,command.down,record.schema);
  }else if(resumed){
   const other=resumed.step(command);maxPositionError=Math.max(maxPositionError,current.position.distanceTo(other.position));
   assert.deepEqual(captureFlightCorrection(other),captureFlightCorrection(current),`native flight tick${tick}`);
  }
 }
 assert.ok(states.has('Flying'));assert.equal(maxPositionError,0);
} finally {replay.dispose();resumed?.dispose();}
console.log(JSON.stringify({source:{revision:execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),dirty:!!execFileSync('git',['status','--porcelain'],{encoding:'utf8'}).trim()},node:process.version,map:'bes1',vehicle:a.name,ticks:360,checkpoints,states:[...states],payloadBytes:flightCorrectionBytes,packetBytes:bytes,maxPositionError,alive:a.alive,scope:'exact semantic correction with owned geometry; not transport or player capacity'},null,2));
