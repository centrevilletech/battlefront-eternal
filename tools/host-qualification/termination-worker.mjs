// A real battle lives only in this worker. Termination is V8/OS reclamation
// for WASM; no Rust Drop total is invented for that backend.
import {parentPort,workerData} from 'node:worker_threads';
import {backendProvider,manifest} from './common.mjs';
const config=workerData.config;
const provider=await backendProvider({backend:config.backend,module:config['backend-module'],engine:config.engine,manifest:await manifest(config.engine)});
const battle=await provider.open({query:workerData.query,data:config.data,clients:2});
await battle.join('terminated-client');
if(workerData.phase==='after-step')battle.step(120);
parentPort.postMessage({ready:true,tick:battle.tick()});
if(workerData.phase==='during-step-loop'){
  const signal=new Int32Array(workerData.signal);
  for(;;){battle.step(1);Atomics.store(signal,0,battle.tick());}
}else setInterval(()=>{},1000);
