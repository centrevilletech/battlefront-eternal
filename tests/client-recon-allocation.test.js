import {replayOutcomes} from './helpers/replay-outcomes.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {gunzipSync} from 'node:zlib';
import * as T from 'three';
import {RemoteDroids} from '../src/remote.js';
import {CameraFrame} from './helpers/cameras.js';
import {reconRandom} from '../src/runtime/recon-random.js';
import {createClientPrediction} from '../src/sim/wasm/runtime/client-prediction.js';

import {restoreRemoteReplay} from '../src/runtime/network-remote-replay.js';
const decode=value=>value&&typeof value==='object'&&'$number'in value?Buffer.from(value.$number,'hex').readDoubleLE():value&&typeof value==='object'&&'$undefined'in value?undefined:Array.isArray(value)?value.map(decode):value&&typeof value==='object'?Object.fromEntries(Object.entries(value).map(([k,v])=>[k,decode(v)])):value;
// Heap sampling has its own worker: constructing the varied serialization
// fixtures must not change the representations of the measured live inputs.
test('retained recon facts and publication allocate no objects in a warm frame',async()=>{
  const {Session}=await import('node:inspector/promises');
  const source=JSON.parse(gunzipSync(readFileSync(new URL('./fixtures/prediction-reference/recon.json.gz',import.meta.url))))[1];
  const host=new CameraFrame(),context=new RemoteDroids({cameraPhase:host.owner,scene:{add(){},remove(){}},camera:null,collision:host.collision,lights:{flash(){}},model:()=>null,fire(){},explode(){}});
  context.aimCamera=new T.PerspectiveCamera(60,16/9,.1,1000);context.random=reconRandom(44,2);context.firstPerson=false;
  const droid=context.createDroid(source.definition,null,new T.Vector3(0,2,0),new T.Vector3(),0,1);context.droids.push(droid);context.enter(droid);droid.networkGeneration=1;
  restoreRemoteReplay(context,droid,decode(source.initial));
  const native=createClientPrediction(host.collision.fixtureInstance,host.collision),profiler=new Session();profiler.connect();
  native.replaySources([],replayOutcomes(source.pending),[]);
  const held=[];
  const frame=()=>{native.predictRecon(context,droid,source.pending,0,1,held,source.view);native.restoreRecon();};
  const warm=async()=>{for(let batch=0;batch<64;batch++){for(let n=0;n<128;n++)frame();await new Promise(resolve=>setTimeout(resolve,1));}};
  try{
    const options={samplingInterval:1,includeObjectsCollectedByMajorGC:true,includeObjectsCollectedByMinorGC:true};
    // Starting the profiler can invalidate optimized publication stores. Both
    // windows run the same retained source; the second must still report zero.
    await profiler.post('HeapProfiler.collectGarbage');await warm();await profiler.post('HeapProfiler.startSampling',options);
    for(let n=0;n<128;n++)frame();await profiler.post('HeapProfiler.stopSampling');
    await profiler.post('HeapProfiler.collectGarbage');await warm();await profiler.post('HeapProfiler.startSampling',options);
    for(let n=0;n<512;n++)frame();
    const {profile}=await profiler.post('HeapProfiler.stopSampling'),allocations=[];
    const visit=node=>{if(node.selfSize&&/\/src\/(runtime\/(client-(recon|prediction)|recon-random)|sim\/gen\/(client|combat))\.js$/.test(node.callFrame.url))allocations.push({name:node.callFrame.functionName,line:node.callFrame.lineNumber+1,bytes:node.selfSize});for(const child of node.children)visit(child);};
    visit(profile.head);assert.deepEqual(allocations,[]);
  }finally{profiler.disconnect();native.dispose();}
});
