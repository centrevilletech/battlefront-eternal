// Long-lived content qualification and short CI use the same lifecycle driver.
// Weak collection results describe WASM/JS ownership; native Drop counters are
// supplied separately by the diagnostic addon or future native backend.
import assert from 'node:assert/strict';
import {Worker} from 'node:worker_threads';
import {once} from 'node:events';
import {backendProvider,manifest,sleep,collect} from './common.mjs';
import {maximum,processMemory} from './telemetry.mjs';
export async function qualifyLifecycle(config){
  const identity=await manifest(config.engine);
  const provider=await backendProvider({backend:config.backend,module:config['backend-module'],engine:config.engine,manifest:identity});
  if(config.backend==='native'&&typeof provider.counts!=='function')throw Error('Native owner counts are required');
  const initialCounts=provider.counts?.();
  const query=`?map=${config.map??'nab2'}&side=${config.side??'rep'}&ai=classic&seed=1`;
  const setup={query,data:config.data,clients:Math.min(config.clients,64)};
  const result={backend:config.backend,bundle:{engine:identity.engineRevision,server:identity.serverRevision,build:identity.engineBuild},capabilities:provider.capabilities,
    query,sequential:[],construction:[],long:null,peak:{},failures:[],nativeDrops:provider.capabilities.nativeWorld?'required from adapter':'not claimed for WASM'};
  try{
  const collected=async(refs,label)=>{
    const ok=await collect(refs,config.backend==='wasm'?1000:10000);
    if(!ok)result.failures.push(`${label}: ${refs.map(r=>r.deref()?.constructor.name??'collected').join(',')}`);
    return ok;
  };
  const sample=async battle=>{const row=battle.diagnostics(),memory=await processMemory();result.peak=maximum(result.peak,{rss:memory.rss,pss:memory.pss,linearBytes:row.wasm?.linearBytes??0,heapUsed:row.js?.heapUsed??0,external:row.js?.external??0});return row;};
  // Inject immediately after each resource acquisition, not into gameplay.
  for(const stage of provider.acquisitionStages??['instance','game','world','publisher','client']){
    result.phase=`construction:${stage}`;let battle,refs=[];
    try{battle=await provider.open({...setup,failAfter:stage});refs=battle.references;await battle.join('same-identity');assert.fail(`Missing construction failure: ${stage}`);}
    catch(error){assert.match(error.message,/Construction fixture/);refs=error.references??refs;}
    finally{battle?.close();battle=null;}
    result.construction.push({stage,collected:await collected(refs,`partial ${stage} ownership retained`)});
  }
  for(let index=0;index<config.battles;index++){
    result.phase=`sequential:${index}`;let battle=await provider.open(setup);const refs=battle.references;
    const first=await battle.join('same-identity');battle.step(config.ticks);
    assert.equal(battle.leave('same-identity'),true);assert.equal(battle.leave('same-identity'),false);
    const second=await battle.join('same-identity');battle.step(config.ticks);
    const state=battle.inspect();assert.ok(state.clients[0].complete>0,'fresh receiver never completed a snapshot');
    await sample(battle);assert.equal(battle.close(),true);assert.equal(battle.close(),false);
    assert.throws(()=>battle.step(),/closed/);battle=null;
    result.sequential.push({index,ticks:state.tick,first,second,collected:await collected(refs,`battle ${index} retained its instance`)});
  }
  // Equal local numbers are legal. Advance one world while the other's rows
  // and clock stay unchanged, then close it without invalidating its sibling.
  result.phase='isolation';let a=await provider.open(setup),b=await provider.open(setup);
  const refs=[...a.references,...b.references],aid=a.identity('context-a'),bid=b.identity('context-b');
  assert.equal(aid.stringId,bid.stringId);assert.equal(aid.unitId,bid.unitId);assert.notEqual(aid.string,bid.string);
  const ap=await a.join('same-identity'),bp=await b.join('same-identity');assert.equal(ap.unitId,bp.unitId);
  a.step(3);assert.equal(b.tick(),0);const saved=a.checksum();b.step(7);assert.equal(a.tick(),3);assert.equal(a.checksum(),saved);
  a.close();a=null;b.step(1);assert.equal(b.inspect().tick,8);b.close();b=null;
  result.isolation={aid,bid,collected:await collected(refs,'interleaved world retained an instance')};
  // A tick-driven run catches churn that a wall-clock soak may not reach on
  // an overloaded host. Its completed count is an independent requirement.
  result.phase='stress';let stress=await provider.open(setup);const stressRefs=stress.references;
  await stress.join('stress');stress.step(config['stress-ticks']);
  result.stress={ticks:stress.tick()};assert.equal(result.stress.ticks,config['stress-ticks']);
  stress.close();stress=null;result.stress.collected=await collected(stressRefs,'stress battle retained its instance');
  result.phase='long';let battle=await provider.open(setup);const longRefs=battle.references;
  for(let i=0;i<Math.min(config.clients,4);i++)await battle.join(`long-${i}`);
  const start=performance.now(),end=start+config.seconds*1000;let samples=0,nextSample=start;
  result.long={seconds:0,ticks:0,samples:0,requestedSeconds:config.seconds};
  while(performance.now()<end){
    // Bound catch-up work. Wall duration and actual completed ticks are both
    // recorded, so an overloaded run cannot masquerade as a 60 Hz soak.
    const target=Math.floor((performance.now()-start)*60/1000),tick=battle.tick();
    if(target>tick)battle.step(Math.min(60,target-tick));
    if(performance.now()>=nextSample){await sample(battle);samples++;nextSample+=1000;result.long={seconds:(performance.now()-start)/1000,ticks:battle.tick(),samples,requestedSeconds:config.seconds};}
    await sleep(10);
  }
  const final=battle.inspect();result.long={seconds:(performance.now()-start)/1000,ticks:final.tick,finishedAt:final.finishedAt,samples};
  assert.ok(final.tick>=Math.max(1,Math.floor(config.seconds*45)),'soak did not advance enough real ticks');
  assert.equal(battle.close(),true);assert.equal(battle.close(),false);battle=null;
  result.long.collected=await collected(longRefs,'long battle retained its instance');
  result.phase='workers';result.workers=await qualifyWorkers(config);
  if(config.backend==='native'){
    const counts=provider.counts();assert.equal(counts.live,0,'native resources survived teardown');
    assert.equal(counts.created-initialCounts.created,counts.released-initialCounts.released);
    result.nativeDrops={created:counts.created-initialCounts.created,released:counts.released-initialCounts.released,live:counts.live};
  }
  result.phase='complete';result.passed=result.failures.length===0;return result;
  }catch(error){error.qualification=result;throw error;}
}

export async function qualifyWorkers(config){
  let provider,before;
  if(config.backend==='native'){
    provider=await backendProvider({backend:config.backend,module:config['backend-module'],engine:config.engine,manifest:await manifest(config.engine)});
    if(typeof provider.counts!=='function')throw Error('Process-wide native owner counts are required');
    before=provider.counts();
  }
  const query=`?map=${config.map??'nab2'}&side=${config.side??'rep'}&ai=classic&seed=1`;
  const result=[];
  for(const phase of ['idle','after-step','during-step-loop']){
    const signal=new Int32Array(new SharedArrayBuffer(4));
    const worker=new Worker(new URL('./termination-worker.mjs',import.meta.url),{execArgv:[],workerData:{config,query,phase,signal:signal.buffer}});
    try{
      const [ready]=await Promise.race([once(worker,'message'),once(worker,'error').then(([error])=>{throw error;})]);
      if(phase==='during-step-loop'){
        const end=performance.now()+30000;
        while(Atomics.load(signal,0)<2){if(performance.now()>end)throw Error('Worker never advanced a battle');await sleep(1);}
      }
      const completed=Atomics.load(signal,0);assert.equal(await worker.terminate(),1);
      if(provider){const after=provider.counts();assert.equal(after.live,0);assert.equal(after.created-before.created,after.released-before.released);}
      result.push({phase,readyTick:ready.tick,completedTicks:completed,reclamation:config.backend==='wasm'?'worker environment reclaimed; no Rust Drop count':'requires provider resource counts'});
    }finally{await worker.terminate();}
  }
  return result;
}
