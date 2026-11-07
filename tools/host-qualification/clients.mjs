// Each real page has its own WASM memory and event loop. Keeping every
// synthetic receiver in one realm makes unrelated clients invalidate each
// other's typed views on growth and eventually starves transport keepalives.
import {Worker} from 'node:worker_threads';
export async function clientFactory(engine,server,directory){
  return async function connect(index,info){
    const worker=new Worker(new URL('./client-worker.mjs',import.meta.url),{workerData:{engine,server,directory,index,info}});
    let state,ended=false,closing=false,next=0;
    const pending=new Map();
    let readyResolve,readyReject;
    const ready=new Promise((resolve,reject)=>{readyResolve=resolve;readyReject=reject;});
    const fail=error=>{readyReject(error);for(const p of pending.values()){clearTimeout(p.timer);p.reject(error);}pending.clear();};
    worker.on('error',fail);
    const exit=new Promise(resolve=>worker.once('exit',code=>{ended=true;if(!closing){const error=Error(`Client worker exited ${code}`);state?.errors.push(error.message);fail(error);}resolve(code);}));
    worker.on('message',m=>{
      if(m.state)state=m.state;
      if(m.type==='ready')readyResolve();
      if(m.type==='failure'){const error=Error(m.error);error.qualificationClient=m.client;fail(error);}
      const p=pending.get(m.id);if(p){pending.delete(m.id);clearTimeout(p.timer);m.error?p.reject(Error(m.error)):p.resolve(m.result);}
    });
    const call=(op,value)=>new Promise((resolve,reject)=>{
      if(ended){reject(Error('Client worker already exited'));return;}
      const id=++next,timer=setTimeout(()=>{pending.delete(id);reject(Error(`Client ${op} timeout`));},15000);
      pending.set(id,{resolve,reject,timer});worker.postMessage({id,op,value});
    });
    const timeout=setTimeout(()=>fail(Error('Client worker readiness timeout')),45000);
    try{await ready;}catch(error){closing=true;await worker.terminate();throw error;}finally{clearTimeout(timeout);}
    // The opaque token proves that a rejoin used a fresh receiver isolate.
    return {id:state.identity,pair:state.pair,receiver:{},get errors(){return state.errors;},get complete(){return state.complete;},get tick(){return state.tick;},
      report:()=>({...state,pair:undefined}),slow(value){if(!ended)worker.postMessage({op:'slow',value});},inspect:()=>call('inspect'),
      async close(){if(closing)return false;closing=true;try{await call('close');}finally{if(!ended)await worker.terminate();await exit;}return true;}};
  };
}
