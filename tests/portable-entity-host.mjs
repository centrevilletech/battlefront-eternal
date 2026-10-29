// Requires npm run host:prepare and imported content in public/.
// Tests the pinned portable adapter against its bundled engine.
import { Worker } from 'node:worker_threads';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
const root=fileURLToPath(new URL('../',import.meta.url)).replace(/\/$/,'');
const worker=new Worker(root+'/.host-runtime/server/src/world-worker.js',{workerData:{engine:root+'/.host-runtime/engine',data:root+'/public',query:'?map=nab2&side=rep&ai=off&seed=1',maxPlayers:1,autoSpawn:true}});
const messages=[],waiters=[];
worker.on('message',m=>{messages.push(m);for(const w of [...waiters])if(w.match(m)){waiters.splice(waiters.indexOf(w),1);clearTimeout(w.timer);w.resolve(m);}});
const wait=match=>new Promise((resolve,reject)=>{const w={match,resolve,timer:setTimeout(()=>reject(Error('timeout: '+JSON.stringify(messages.slice(-3)))),15000)};waiters.push(w);});
try{
 const ready=await wait(m=>m.type==='ready'||m.type==='error');assert.equal(ready.type,'ready',ready.message);assert.equal(ready.networkContract.serverAdapterABI,3);
 const joined=wait(m=>m.type==='joined'||m.type==='error');worker.postMessage({type:'join',client:'smoke',name:'Smoke',entity:true});const player=await joined;assert.equal(player.player.snapshotMode,'entity-v1');assert.ok(messages.some(m=>m.type==='entityRegistry'));
 const started=wait(m=>m.type==='reply'&&m.id===1);worker.postMessage({type:'call',client:'smoke',id:1,method:'start'});await started;
 const state=wait(m=>m.type==='state');worker.postMessage({type:'step',count:6});await state;
 const packets=messages.filter(m=>m.type==='entityPacket');assert.ok(packets.length);assert.ok(!messages.some(m=>['error','snapshotFault','snapshot','keyframe'].includes(m.type)),JSON.stringify(messages.filter(m=>['error','snapshotFault'].includes(m.type))));
 console.log(JSON.stringify({pass:true,simulation:ready.networkContract.simulation,packets:packets.length,join:'entity-only'}));
}finally{await worker.terminate();}
