import {replayOutcomes} from './helpers/replay-outcomes.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {gunzipSync} from 'node:zlib';
import * as T from 'three';
import {RemoteDroids} from '../src/remote.js';
import {CameraFrame} from './helpers/cameras.js';
import {reconRandom} from '../src/runtime/recon-random.js';
import {captureRemoteReplay,restoreRemoteReplay} from '../src/runtime/network-remote-replay.js';
import {bridgeFor} from '../src/sim/runtime.js';
import {createClientPrediction} from '../src/runtime/client-prediction.js';
// Frozen before removing the page controller. The synthetic cases cover flight,
// camera zoom, exit, strike and an extended missing-correction interval. Hex
// doubles preserve signed zero and non-finite values across the fixture file.
const encode=value=>value===undefined?{$undefined:true}:typeof value==='number'?{$number:Buffer.from(new Float64Array([value]).buffer).toString('hex')}:Array.isArray(value)?value.map(encode):value&&typeof value==='object'?Object.fromEntries(Object.keys(value).sort().map(k=>[k,encode(value[k])])):value;
const decode=value=>value&&typeof value==='object'&&'$number'in value?Buffer.from(value.$number,'hex').readDoubleLE():value&&typeof value==='object'&&'$undefined'in value?undefined:Array.isArray(value)?value.map(decode):value&&typeof value==='object'?Object.fromEntries(Object.entries(value).map(([k,v])=>[k,decode(v)])):value;
test('native recon replay and restoration match every frozen controller checkpoint',()=>{
let count=0;for(const entry of JSON.parse(gunzipSync(readFileSync(new URL('./fixtures/prediction-reference/recon.json.gz',import.meta.url))))){
const test=entry;
 const host=new CameraFrame(),context=new RemoteDroids({cameraPhase:host.owner,scene:{add(){},remove(){}},camera:null,collision:host.collision,lights:{flash(){}},model:()=>null,fire(){throw Error('authority fire');},explode(){throw Error('authority explosion');}});
 context.aimCamera=new T.PerspectiveCamera(60,16/9,.1,1000);context.random=reconRandom(44,2);context.firstPerson=false;
 const droid=context.createDroid(test.definition,null,new T.Vector3(0,2,0),new T.Vector3(),0,1);context.droids.push(droid);context.enter(droid);droid.networkGeneration=1;
 restoreRemoteReplay(context,droid,decode(test.initial));
 const originalState=encode(captureRemoteReplay(context,droid));
 const bridge=bridgeFor(host.collision.fixtureInstance),original=bridge.x;
 let calls=0;
 const traced={...original,client_predict_session(handle,words){calls++;return original.client_predict_session(handle,words);},client_prediction_run(handle,op){calls++;return original.client_prediction_run(handle,op);}};
 bridge.x=host.collision.rays.x=traced;
 const session=createClientPrediction(host.collision.fixtureInstance,host.collision);
 try{
  session.replaySources([],replayOutcomes(test.pending),[]);
  const tick=session.predictRecon(context,droid,test.pending,0,1,[],test.view);assert.equal(tick,test.tick);
  const actual=encode(captureRemoteReplay(context,droid));
  assert.deepEqual(actual,test.expected,`${test.scenario} at ${test.tick}`);
  session.restoreRecon();assert.deepEqual(encode(captureRemoteReplay(context,droid)),test.restoreExpected??originalState,'authority restored');
  calls=0;session.predictRecon(context,droid,test.pending,0,1,[],test.view);assert.equal(calls,1,'one prediction call after admission');
  session.restoreRecon();count++;
 }finally{session.dispose();bridge.x=host.collision.rays.x=original;}
}assert.equal(count,60);});
