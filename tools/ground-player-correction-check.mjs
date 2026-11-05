// Operator-owned map geometry, real ground hull classes and exact replay.
// This checks correction parity, not transport, rendering or player capacity.
// Replay uses the required native session, including gaps between commands.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFile} from 'node:fs/promises';
import {resolve,sep} from 'node:path';
import {execFileSync} from 'node:child_process';
import {createGame} from '../src/game.js';
import {HullReplay,hullInput} from './hull-replay-fixture.mjs';
import {captureGroundCorrection,groundCorrectionBytes} from '../src/runtime/network-entity-ground.js';
const root=resolve(process.env.BFE_DATA_DIR||'public'),map=process.env.BFE_TEST_MAP||'nab2',side=process.env.BFE_TEST_SIDE||'rep';
globalThis.fetch=async path=>{const filename=resolve(root,`.${path}`);assert.ok(filename.startsWith(root+sep));try{return new Response(await readFile(filename));}catch(e){if(e.code==='ENOENT')return new Response('',{status:404});throw e;}};
const movement=true;
const game=await createGame({headless:true,query:`?map=${map}&side=${side}&ai=classic&seed=1`});
assert.equal(Boolean(game.collision.movement),movement,'movement kernel as asked');
const sources=[...new Map(game.vehicles.vehicles.filter(v=>!v.flying).map(v=>[v.name,v])).values()];assert.ok(sources.length,'map has ground vehicles');
const results=[];
for(const source of sources){
 const a=new HullReplay(source);let b,maxPositionError=0;const digest=createHash('sha1');
 try {
  for(let tick=1;tick<=240;tick++){
   const command=hullInput(tick,false),current=a.step(command);
   if([10,80,155,210].includes(tick)){
    b?.dispose();b=new HullReplay(source);const collider=b.source.body.matrix.toArray();
    b.restore(captureGroundCorrection(current),tick,command.down);assert.deepEqual(b.source.body.matrix.toArray(),collider);
   }else if(b)b.step(command);
   if(b){maxPositionError=Math.max(maxPositionError,current.position.distanceTo(b.current.position));const replayed=captureGroundCorrection(b.current);assert.deepEqual(replayed,captureGroundCorrection(current),`${source.name} tick${tick}`);digest.update(replayed);}
  }
  results.push({vehicle:source.name,base:source.base,ticks:240,maxPositionError,payloadBytes:groundCorrectionBytes,replayDigest:digest.digest('hex')});
 } finally {a.dispose();b?.dispose();}
}
console.log(JSON.stringify({source:{revision:execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),dirty:!!execFileSync('git',['status','--porcelain'],{encoding:'utf8'}).trim()},map,movement:movement?'wasm':'js',results,scope:'owned-content correction and physics replay, not transport/render/capacity'},null,2));
