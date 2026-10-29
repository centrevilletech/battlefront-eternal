import test from 'node:test';import assert from 'node:assert/strict';import {Vector3} from 'three';
import {reconRandom} from '../src/runtime/network-remote-world.js';
import {RemoteDroids} from '../src/remote.js';import {captureRemoteReplay,restoreRemoteReplay} from '../src/runtime/network-remote-replay.js';
function fixture(){const context=new RemoteDroids({scene:{add(){},remove(){}},camera:null,collision:{cast:()=>null,gather:()=>[]},lights:{flash(){}},model:()=>null,fire(){},explode(){}});context.random=reconRandom(123,1);
 const def={SwitchImmediately:1,droid:{weapons:[{base:'blaster',class:'probe_gun',channel:0,RoundsPerClip:0,ShotDelay:.2,ReloadTime:1,ordnance:{}}],AddHealth:-1}};
 return {context,droid:context.launch({def},new Vector3(0,2,0),new Vector3(),0,1)};}
const controls={take:()=>false,held:()=>false};
test('exact recon checkpoint restores every mutable weapon field and shared movement',()=>{
 const a=fixture(),b=fixture(),weapon=a.droid.arsenal.current(0);weapon.heat=.8;weapon.kick=.25;weapon.wait=.3;weapon.salvosLeft=2;weapon.salvoWait=.12;weapon.charging=true;weapon.held=.2;a.context.look(10,5,.2,false);a.context.zoomStep=1;
 const saved=captureRemoteReplay(a.context,a.droid);restoreRemoteReplay(b.context,b.droid,saved);assert.deepEqual(captureRemoteReplay(b.context,b.droid),saved);
 for(let i=0;i<30;i++){a.context.update(1/60,{forward:1,right:.2,turn:.1},controls);b.context.update(1/60,{forward:1,right:.2,turn:.1},controls);assert.deepEqual(captureRemoteReplay(b.context,b.droid),captureRemoteReplay(a.context,a.droid));}
});
test('checkpoint restores outstanding salvo without aliasing authority vectors',()=>{
 const a=fixture(),b=fixture();a.context.strike(a.droid,{SalvoCount:2,SalvoDelay:.5,AimDistance:10,strike:{}});a.context.kill(a.droid);const saved=captureRemoteReplay(a.context,a.droid);restoreRemoteReplay(b.context,b.droid,saved);assert.equal(b.context.strikes[0],b.droid);assert.equal(b.context.active,false);b.droid.salvo.at.x=20;assert.notEqual(a.droid.salvo.at.x,20);assert.notEqual(saved.salvo.at.x,20);
});

test('replayed strike uses its restored RNG without advancing authority stream',()=>{
 const a=fixture(),b=fixture(),actual=[],predicted=[];a.context.fire=(def,at)=>actual.push(at.toArray());b.context.fire=(def,at)=>predicted.push(at.toArray());a.context.strike(a.droid,{SalvoCount:2,SalvoDelay:.5,AimDistance:10,ScatterDistance:5,strike:{}});const saved=captureRemoteReplay(a.context,a.droid);restoreRemoteReplay(b.context,b.droid,saved);const authoritySeed=a.context.random.seed;
 b.context.updateStrikes(0);assert.equal(a.context.random.seed,authoritySeed);a.context.updateStrikes(0);assert.deepEqual(predicted,actual);assert.equal(a.context.random.seed,b.context.random.seed);b.context.updateStrikes(.5);a.context.updateStrikes(.5);assert.deepEqual(predicted,actual);
});

test('bounded command replay advances shared recon rules without authority effects or RNG mutation',async()=>{
 const {predictRemote}=await import('../src/runtime/network-remote-prediction.js');
 const a=fixture(),source=a.context.createContext({id:2,generation:1,team:1},{random:reconRandom(44,2)});
 let effects=0;source.fire=()=>effects++;source.explode=()=>effects++;
 const droid=source.launch({def:a.droid.weapon.def},new Vector3(0,2,0),new Vector3(),0,1);
 source.strike(droid,{SalvoCount:3,SalvoDelay:.1,AimDistance:10,ScatterDistance:5,strike:{}});
 droid.health=.2;
 const before=captureRemoteReplay(source,droid),commands=Array.from({length:30},(_,i)=>({tick:i+1,generation:1,down:['KeyW'],actions:[],look:[]}));
 const predicted=predictRemote(source,droid,commands,0,1);
 assert.equal(effects,0);assert.deepEqual(captureRemoteReplay(source,droid),before,'prediction does not mutate authority or its random stream');
 for(let i=0;i<30;i++){if(source.active)source.updateCamera(1/60,false,true);source.update(1/60,{forward:1,right:0,turn:0},controls);}
 assert.deepEqual(predicted.state,captureRemoteReplay(source,droid));
 assert.equal(predicted.droid.dead,true);assert.ok(effects>0,'authority emits the salvo that prediction suppresses');
 // Commands stamped past the window wait for a newer checkpoint.
 assert.equal(predictRemote(source,droid,[{...commands[0],tick:121}],0,1).tick,0);
});

test('a downstream blackout predicts the flown droid to the end of the replay window, not past it',async()=>{
 const {predictRemote,REMOTE_REPLAY_TICKS}=await import('../src/runtime/network-remote-prediction.js');
 const a=fixture(),source=a.context.createContext({id:2,generation:1,team:1},{random:reconRandom(44,2)});
 const droid=source.launch({def:a.droid.weapon.def},new Vector3(0,2,0),new Vector3(),0,1);
 // Three seconds of input stamped after the last checkpoint at tick 40.
 const pending=Array.from({length:180},(_,i)=>({tick:41+i,generation:1,down:['KeyW'],actions:[],look:[]}));
 const before=captureRemoteReplay(source,droid);
 const predicted=predictRemote(source,droid,pending,40,1);
 assert.equal(predicted.tick,40+REMOTE_REPLAY_TICKS);
 assert.deepEqual(captureRemoteReplay(source,droid),before,'the authority is untouched');
 for(let i=0;i<REMOTE_REPLAY_TICKS;i++){if(source.active)source.updateCamera(1/60,false,true);source.update(1/60,{forward:1,right:0,turn:0},controls);}
 assert.deepEqual(predicted.state,captureRemoteReplay(source,droid),'the window is replayed as the world would run it');
});

test('browser context restores a computational camera without replacing its render camera',()=>{
 const a=fixture(),source=a.context.createContext({id:1,generation:1,team:1},{random:reconRandom(7,1)});
 const droid=source.launch({def:a.droid.weapon.def},new Vector3(0,2,0),new Vector3(),0,1);
 source.updateCamera(1/60,false,true);const state=captureRemoteReplay(source,droid),renderCamera=a.context.camera;
 assert.equal(a.context.aimCamera,undefined);restoreRemoteReplay(a.context,a.droid,state);
 assert.ok(a.context.aimCamera);assert.ok(a.context.simulationView);assert.equal(a.context.camera,renderCamera);
 assert.deepEqual(captureRemoteReplay(a.context,a.droid),state);
});

test('reused recon controller matches fresh replay through corrections, strikes, death and exit',async()=>{
 const {predictRemote,RemotePredictionContext}=await import('../src/runtime/network-remote-prediction.js');
 const {context,droid}=fixture(),cache=new RemotePredictionContext();droid.networkGeneration=1;
 let builds=0;const create=context.createContext.bind(context);context.createContext=(...args)=>{builds++;return create(...args);};
 const commands=Array.from({length:20},(_,i)=>({tick:i+1,generation:1,epoch:1,sequence:i+1,down:['KeyW'],actions:i===10?['use']:[],look:[[2,-1]],mouseScale:.2,firstPerson:false,aspect:1.5}));
 const saved=captureRemoteReplay(context,droid),authoritySeed=context.random.seed;
 for(let length=1;length<=20;length++){
  const fresh=predictRemote(context,droid,commands.slice(0,length),0,1);const before=builds;
  const reused=predictRemote(context,droid,commands.slice(0,length),0,1,[],{},cache);
  assert.deepEqual(reused.state,fresh.state);assert.equal(builds-before,length===1?1:0);
  assert.deepEqual(captureRemoteReplay(context,droid),saved);assert.equal(context.random.seed,authoritySeed);
 }
 context.strike(droid,{SalvoCount:3,SalvoDelay:.1,AimDistance:10,ScatterDistance:5,strike:{}});droid.countdown=.05;droid.def.SelfDestructTime=.1;
 for(let n=0;n<4;n++){
  const pending=commands.slice(0,20);const fresh=predictRemote(context,droid,pending,0,1),reused=predictRemote(context,droid,pending,0,1,[],{},cache);
  assert.deepEqual(reused.state,fresh.state);assert.equal(reused.droid.dead,true);
 }
 const old=cache.value;droid.networkGeneration=2;predictRemote(context,droid,[],0,1,[],{},cache);assert.notEqual(cache.value,old);
 cache.reset();assert.equal(cache.value,null);assert.equal(cache.original,null);
});

test('cached recon replay restores computational camera and isolates replacement owners',async()=>{
 const {PerspectiveCamera}=await import('three');const {predictRemote,RemotePredictionContext}=await import('../src/runtime/network-remote-prediction.js');
 const a=fixture(),b=fixture(),cache=new RemotePredictionContext();a.droid.owner={id:1};b.droid.owner={id:2};
 for(const value of [a,b]){value.context.aimCamera=new PerspectiveCamera();value.context.enter(value.droid);}
 for(let i=0;i<20;i++){
  const {context,droid}=i<10?a:b;context.look(1,-2,.2,false);context.updateCamera(1/60,false,true);context.update(1/60,{forward:.5,right:.2,turn:0},controls);
  const pending=Array.from({length:8},(_,n)=>({tick:i+n+1,generation:1,epoch:1,sequence:i+n+1,down:['KeyW'],actions:[],look:[[2,1]],mouseScale:.2,firstPerson:false,aspect:1.6}));
  const before=captureRemoteReplay(context,droid),fresh=predictRemote(context,droid,pending,i,1),reused=predictRemote(context,droid,pending,i,1,[],{},cache);
  assert.deepEqual(reused.state,fresh.state);assert.deepEqual(captureRemoteReplay(context,droid),before);assert.equal(reused.droid.owner,droid.owner);
 }
});
