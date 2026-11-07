// Run the production client unchanged in an isolated page-like realm.
// Only compact status crosses back to the supervisor; borrowed packet views
// and native owners remain inside the receiver's own lifetime.
import {parentPort,workerData} from 'node:worker_threads';
import {clientFactory} from './client-driver.mjs';
let client,timer;
const state=()=>({...client.report(),pair:client.pair});
try{
  const {engine,server,directory,index,info}=workerData;
  const connect=await clientFactory(engine,server,directory);client=await connect(index,info);
  parentPort.postMessage({type:'ready',state:state()});
  timer=setInterval(()=>parentPort.postMessage({type:'status',state:state()}),100);
  parentPort.on('message',async m=>{
    try{
      if(m.op==='slow'){client.slow(m.value);return;}
      const result=m.op==='inspect'?await client.inspect():m.op==='close'?await client.close():undefined;
      parentPort.postMessage({id:m.id,result,state:state()});
      if(m.op==='close'){clearInterval(timer);parentPort.close();}
    }catch(error){parentPort.postMessage({id:m.id,error:String(error.stack??error),state:state()});}
  });
}catch(error){parentPort.postMessage({type:'failure',error:String(error.stack??error),client:error.qualificationClient});clearInterval(timer);parentPort.close();}
