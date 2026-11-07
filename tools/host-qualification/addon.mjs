// Real Node-API owners, including an invocation known to be inside Rust when
// Worker.terminate is requested. These are foundation results, not native-world
// qualification: the current addon explicitly advertises that distinction.
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {Worker} from 'node:worker_threads';
import {once} from 'node:events';
import {resolve} from 'node:path';
import {sleep,until,collect} from './common.mjs';
const require=createRequire(import.meta.url);
export async function qualifyAddon(path){
  const addon=require(resolve(path));
  if(!addon.lifecycleCountsInto||!addon.NativeOwner.prototype.probeWork)throw Error('A lifecycle-probe addon is required');
  const arena=new Uint32Array(4),read=()=>{addon.lifecycleCountsInto(arena);return arena;};
  const before=read().slice(),host=new addon.NativeHost(),cases=[];
  for(let i=0;i<24;i++){const owner=host.create();assert.equal(owner.close(),true);assert.equal(owner.close(),false);}
  cases.push('24 sequential Contexts and double close');
  const a=host.create(),b=host.create();
  const aid=a.probeIdentity('first context',12.5),bid=b.probeIdentity('second context',-7);
  assert.deepEqual([aid.stringId,aid.slot,aid.generation],[bid.stringId,bid.slot,bid.generation]);
  assert.equal(a.probeRead(aid.stringId),'first context');assert.equal(b.probeRead(bid.stringId),'second context');
  assert.equal(a.probeSpeed(),12.5);assert.equal(b.probeSpeed(),-7);
  a.close();assert.equal(b.probeRead(bid.stringId),'second context');assert.equal(b.probeSpeed(),-7);b.close();
  cases.push('independent Contexts reuse string and entity IDs');
  for(const acquisition of [1,2]){assert.throws(()=>host.probeFailure(acquisition),/acquisition fixture/);assert.equal(host.liveOwners(),0);}
  cases.push('failure after Context and registry acquisition');
  const refs=[];(()=>{for(let i=0;i<8;i++)refs.push(new WeakRef(host.create()));})();
  assert.ok(await collect(refs),'unreachable owners must be collected');
  await until(()=>host.liveOwners()===0,'Context finalizers');cases.push('GC finalization');
  for(const phase of ['idle','after-step','during-native-work']){
    const counts=read().slice(),signal=new Int32Array(new SharedArrayBuffer(8));
    const worker=new Worker(`const {parentPort,workerData}=require('node:worker_threads');
      const addon=require(workerData.path),host=new addon.NativeHost();global.owner=host.create();
      if(workerData.phase==='after-step')owner.probeIdentity('worker',3);
      parentPort.postMessage('ready');
      if(workerData.phase==='during-native-work')owner.probeWork(new Uint32Array(workerData.signal),5000);
      setInterval(()=>{},1000);`,{eval:true,execArgv:[],workerData:{path:resolve(path),phase,signal:signal.buffer}});
    try{
      await Promise.race([once(worker,'message'),once(worker,'error').then(([e])=>{throw e;})]);
      if(phase==='during-native-work')await until(()=>Atomics.load(signal,0)===1,'active Rust invocation');
      const stopping=worker.terminate();
      if(phase==='during-native-work'){await sleep(20);Atomics.store(signal,1,1);}
      assert.equal(await stopping,1);
      await until(()=>read()[1]===counts[1]+1,'environment cleanup');
      assert.equal(read()[0]-counts[0],1);
    }finally{Atomics.store(signal,1,1);await worker.terminate();}
    cases.push(`Worker.terminate ${phase}`);
  }
  const after=read(),created=after[0]-before[0],released=after[1]-before[1];
  assert.equal(host.liveOwners(),0);assert.equal(created,released);
  return {scope:'native Context foundation',capabilities:addon.handshake(),cases,created,released,live:created-released};
}
