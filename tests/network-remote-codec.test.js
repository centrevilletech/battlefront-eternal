import {CameraFrame} from './helpers/cameras.js';
import test from 'node:test';import assert from 'node:assert/strict';import {Vector3,PerspectiveCamera} from 'three';
import {RemoteDroids} from '../src/remote.js';import {reconRandom} from '../src/runtime/recon-random.js';import {captureRemoteReplay,restoreRemoteReplay} from '../src/runtime/network-remote-replay.js';import {encodeRemoteCorrection,decodeRemoteCorrection} from '../src/runtime/network-remote-codec.js';
function fixture(){const context=new RemoteDroids({cameraPhase:new CameraFrame().owner,scene:{add(){},remove(){}},camera:null,collision:{cast:()=>null,gather:()=>[]},lights:{flash(){}},model:()=>null,fire(){},explode(){}});context.random=reconRandom(34,1);context.aimCamera=new PerspectiveCamera();context.firstPerson=false;
 const def={SwitchImmediately:1,droid:{weapons:[{base:'cannon',class:'gun',channel:0,RoundsPerClip:0,ShotDelay:.2,ReloadTime:1,ordnance:{}},{base:'destruct',class:'strike',channel:1,RoundsPerClip:0,ShotDelay:.2,ReloadTime:1,strike:{},SalvoCount:2,SalvoDelay:.5,AimDistance:10,ScatterDistance:4}],AddHealth:-1}};
 const droid=context.createDroid(def.droid,null,new Vector3(0,2,0),new Vector3(),0,1);context.droids.push(droid);context.enter(droid);return {context,droid};}
const controls={take:()=>false,held:()=>false};
test('full recon correction stays datagram-sized and retains every explicit mutable record',()=>{
 const a=fixture(),b=fixture();a.context.updateCamera(1/60,false,true);a.droid.arsenal.current(0).heat=.6;
 a.droid.salvo={def:a.droid.arsenal.current(1).def,at:new Vector3(0,2,0),x:new Vector3(1,0,0),y:new Vector3(0,0,-1),z:new Vector3(0,1,0),left:2,wait:.5};
 const state=captureRemoteReplay(a.context,a.droid),bytes=encodeRemoteCorrection(state,a.droid);assert.ok(bytes.length<650,`recon block ${bytes.length}`);
 restoreRemoteReplay(b.context,b.droid,decodeRemoteCorrection(bytes,b.droid));assert.deepEqual(captureRemoteReplay(a.context,a.droid),captureRemoteReplay(b.context,b.droid));
});
test('truncated, malformed and trailing corrections cannot mutate live recon state',()=>{
 const f=fixture(),state=captureRemoteReplay(f.context,f.droid),bytes=encodeRemoteCorrection(state,f.droid);for(let n=0;n<bytes.length;n++)assert.throws(()=>decodeRemoteCorrection(bytes.slice(0,n),f.droid));assert.deepEqual(captureRemoteReplay(f.context,f.droid),state);assert.throws(()=>decodeRemoteCorrection(new Uint8Array([...bytes,0]),f.droid),/trailing/);const bad=bytes.slice();bad[1]=255;assert.throws(()=>decodeRemoteCorrection(bad,f.droid),/flags/);
});

test('public encoder rejects states its decoder cannot accept',()=>{
 const f=fixture(),s=captureRemoteReplay(f.context,f.droid);for(const state of [{...s,zoomStep:2},{...s,selected:[99,0]},{...s,camera:{...s.camera,aspect:0}},{...s,camera:{...s.camera,fresh:2}},{...s,camera:{...s.camera,quaternion:[0,0,0,0]}}])assert.throws(()=>encodeRemoteCorrection(state,f.droid));
});
