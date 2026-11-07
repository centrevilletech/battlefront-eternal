import test from 'node:test';
import assert from 'node:assert/strict';
import {Session} from 'node:inspector/promises';
import {EntityInputSender} from '../src/runtime/network-entity-sender.js';
import {INPUT_CODES,DEFAULT_BINDING_PROFILES} from '../src/runtime/network-entity-input.js';
// Transport callbacks consume borrowed bytes and events synchronously. The
// same retained input exercises held fire, cadence, look and successful ACKs.
test('outgoing command batches and binary publication allocate no frame objects',async()=>{
  const registry={inputCodes:INPUT_CODES,bindingProfiles:DEFAULT_BINDING_PROFILES},aim={aimYaw:0,aimPitch:0},ack={epoch:1,tick:1};
  const sender=new EntityInputSender({registry,getAim:()=>aim,onFire(){},snapshotAck:()=>ack,send:()=>true,stateIntervalTicks:2});
  const command={epoch:1,generation:1,sequence:0,tick:0,down:['KeyW','Mouse0'],actions:[],look:[[.125,-.25]],sent:false,worldLook:null,lookSent:false};
  const commands=[command],profiler=new Session();profiler.connect();
  const frame=()=>{command.sequence++;command.tick++;sender.send(commands);};
  const warm=async()=>{for(let batch=0;batch<64;batch++){for(let n=0;n<128;n++)frame();await new Promise(resolve=>setTimeout(resolve,1));}};
  try{
    const options={samplingInterval:1,includeObjectsCollectedByMajorGC:true,includeObjectsCollectedByMinorGC:true};
    await profiler.post('HeapProfiler.collectGarbage');await warm();await profiler.post('HeapProfiler.startSampling',options);
    for(let n=0;n<128;n++)frame();await profiler.post('HeapProfiler.stopSampling');
    // The first profiler window can invalidate optimized property stores.
    // The second window must independently observe zero owned-path objects.
    await profiler.post('HeapProfiler.collectGarbage');await warm();await profiler.post('HeapProfiler.startSampling',options);
    for(let n=0;n<512;n++)frame();
    const {profile}=await profiler.post('HeapProfiler.stopSampling'),allocations=[];
    const visit=node=>{if(node.selfSize&&/\/src\/(runtime\/(network-inputs|network-entity-sender|wire-input|page-network|network-entity-edges)|presentation-pkg\/bfe_presentation)\.js$/.test(node.callFrame.url))allocations.push({name:node.callFrame.functionName,line:node.callFrame.lineNumber+1,bytes:node.selfSize});for(const child of node.children)visit(child);};
    visit(profile.head);assert.deepEqual(allocations,[]);
  }finally{profiler.disconnect();sender.dispose();}
});
