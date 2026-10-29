// Operator-owned map geometry, real ground hull classes and exact replay.
// This checks correction parity, not transport, rendering or player capacity.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {resolve,sep} from 'node:path';
import {execFileSync} from 'node:child_process';
import {createGame} from '../src/game.js';
import {predictionVehicle} from '../src/runtime/vehicle-prediction.js';
import {captureGroundCorrection,restoreGroundCorrection,groundCorrectionBytes} from '../src/runtime/network-entity-ground.js';
const root=resolve(process.env.BFE_DATA_DIR||'public'),map=process.env.BFE_TEST_MAP||'nab2',side=process.env.BFE_TEST_SIDE||'rep';
globalThis.fetch=async path=>{const filename=resolve(root,`.${path}`);assert.ok(filename.startsWith(root+sep));try{return new Response(await readFile(filename));}catch(e){if(e.code==='ENOENT')return new Response('',{status:404});throw e;}};
const game=await createGame({headless:true,query:`?map=${map}&side=${side}&ai=classic&seed=1`});
const sources=[...new Map(game.vehicles.vehicles.filter(v=>!v.flying).map(v=>[v.name,v])).values()];assert.ok(sources.length,'map has ground vehicles');
const results=[];
for(const source of sources){
 const a=predictionVehicle(source);a.captureTick();const initial=predictionVehicle(a);let b,maxPositionError=0;
 for(let tick=1;tick<=240;tick++){
  const input={forward:tick<150?1:-1,right:tick>40?.4:0,turn:tick>100?.2:0,brake:tick>200};
  a.captureTick();a.tick(1/120,input);a.tick(1/120,input);
  if([10,80,155,210].includes(tick)){
   b=predictionVehicle(initial);for(const value of Object.values(b))if(value?.isVector3)value.set(31,-17,29);
   const collider=b.body.matrix.toArray();restoreGroundCorrection(b,captureGroundCorrection(a));assert.deepEqual(b.body.matrix.toArray(),collider);
  }else if(b){b.captureTick();b.tick(1/120,input);b.tick(1/120,input);}
  if(b){maxPositionError=Math.max(maxPositionError,a.position.distanceTo(b.position));assert.deepEqual(captureGroundCorrection(b),captureGroundCorrection(a),`${source.name} tick${tick}`);}
 }
 results.push({vehicle:source.name,base:source.base,ticks:240,maxPositionError,payloadBytes:groundCorrectionBytes});
}
console.log(JSON.stringify({source:{revision:execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),dirty:!!execFileSync('git',['status','--porcelain'],{encoding:'utf8'}).trim()},map,results,scope:'owned-content correction and physics replay, not transport/render/capacity'},null,2));
