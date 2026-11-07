import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,writeFile,readFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {options,backendProvider} from '../tools/host-qualification/common.mjs';
import {smaps,counters,maximum,cgroupMemory} from '../tools/host-qualification/telemetry.mjs';
import {checkSample} from '../tools/host-qualification/service.mjs';
test('qualification counts reject nonfinite, fractional and excess advertised load',()=>{
  const good=options(['--backend=wasm','--clients=64','--concurrency=4','--battles=24','--seconds=1800']);
  assert.equal(good.clients,64);assert.equal(good.seconds,1800);
  for(const args of [['--clients=65'],['--clients=1.5'],['--seconds=NaN'],['--concurrency=0'],['--backend=unknown'],['--seconds=1','--seconds=2'],['--second=1'],['--rotate=NaN']])assert.throws(()=>options(args));
});
test('missing native capability fails instead of running a WASM world',async t=>{
  const dir=await mkdtemp(join(tmpdir(),'host-backend-'));t.after(()=>rm(dir,{recursive:true,force:true}));
  const file=join(dir,'backend.mjs');await writeFile(file,'export function createQualificationBackend(){return {capabilities:{nativeWorld:false,nativePublication:false}}}');
  await assert.rejects(backendProvider({backend:'native'}),/explicit harness/);
  await assert.rejects(backendProvider({backend:'native',module:file}),/native world and publication/);
});
test('memory evidence preserves exact cgroup peak and OOM events',async t=>{
  const dir=await mkdtemp(join(tmpdir(),'host-memory-'));t.after(()=>rm(dir,{recursive:true,force:true}));
  for(const [name,value]of Object.entries({'memory.current':'90','memory.peak':'1073741824','memory.max':'1073741824','memory.events':'low 0\nhigh 0\nmax 9\noom 1\noom_kill 1\n'}))await writeFile(join(dir,name),value);
  const value=await cgroupMemory(dir);assert.equal(value.peak,1073741824);assert.equal(value.events.oom_kill,1);
  assert.deepEqual(smaps('Rss: 2048 kB\nPss: 1536 kB\n'),{rss:2097152,pss:1572864});
  assert.throws(()=>smaps('Rss: 1 kB\n'),/Missing Pss/);assert.throws(()=>counters('oom nope'),/Invalid/);
  assert.deepEqual(maximum({rss:9},{rss:3,pss:7,native:null}),{rss:9,pss:7});
});
test('queue qualification rejects stalled admission, errors and excess socket backlog',()=>{
  const world={queues:{joining:0},network:{queues:{snapshots:2,acks:2,edges:2,inbox:4,inputs:4}},sessions:[{stateBuffered:0}]};
  const sample={worlds:[world],errors:[]};checkSample(sample,2);
  world.network.queues.snapshots=3;assert.throws(()=>checkSample(sample,2),/snapshots/);world.network.queues.snapshots=2;
  world.sessions[0].stateBuffered=32769;assert.throws(()=>checkSample(sample,2),/backlog/);world.sessions[0].stateBuffered=0;
  world.queues.joining=1;assert.throws(()=>checkSample(sample,2),/admission/);world.queues.joining=0;
  sample.errors.push('world failure');assert.throws(()=>checkSample(sample,2),/world failure/);
});
test('service death rejects active RPCs and closes without orphan waiters',async()=>{
  const {Service}=await import('../tools/host-qualification/rpc.mjs');
  const source="console.log(JSON.stringify({type:'ready',pid:process.pid}));process.stdin.once('data',()=>process.exit(7))";
  const service=new Service(process.execPath,['-e',source]);
  await service.ready;await assert.rejects(service.call('sample'),/Service exited 7/);
  assert.equal((await service.close()).code,7);assert.equal(service.pending.size,0);
});
test('lifecycle control exercises every stage and requires native teardown counts',async t=>{
  const dir=await mkdtemp(join(tmpdir(),'host-lifecycle-'));t.after(()=>rm(dir,{recursive:true,force:true}));
  await writeFile(join(dir,'host-runtime.json'),JSON.stringify({engineRevision:'a'.repeat(40),serverRevision:'b'.repeat(40),engineBuild:'fixture'}));
  const module=join(dir,'backend.mjs');
  // A small protocol fixture tests the driver without content or a backend
  // binary. Real solver and transport evidence comes from packaged farm runs.
  await writeFile(module,`export function createQualificationBackend(){return {
    capabilities:{nativeWorld:true,nativePublication:true},counts:()=>({created:0,released:0,live:0}),acquisitionStages:['owner','publication'],
    async open({failAfter}){if(failAfter)throw Error('Construction fixture: '+failAfter);let tick=0,closed=false,joined=false;
    return {references:[],join:async()=>{joined=true;return {unitId:1}},leave:()=>{const was=joined;joined=false;return was},
    identity:label=>({stringId:1,unitId:1,string:label}),step:(n=1)=>{if(closed)throw Error('closed');tick+=n},tick:()=>tick,checksum:()=>tick,
    inspect:()=>({tick,clients:[{complete:1}]}),diagnostics:()=>({}),close:()=>{const was=closed;closed=true;return !was}}}}}`);
  const {qualifyLifecycle}=await import('../tools/host-qualification/lifecycle.mjs');
  const previous=global.gc;if(!previous)global.gc=()=>{};t.after(()=>{if(previous)global.gc=previous;else delete global.gc;});
  const result=await qualifyLifecycle({backend:'native','backend-module':module,engine:dir,clients:1,battles:24,ticks:2,'stress-ticks':20,seconds:0.1});
  const failing=join(dir,'failing.mjs');await writeFile(failing,(await readFile(module,'utf8')).replace("tick+=n","if(n===20)throw Error('stress fault');tick+=n"));
  await assert.rejects(qualifyLifecycle({backend:'native','backend-module':failing,engine:dir,clients:1,battles:2,ticks:2,'stress-ticks':20,seconds:0.1}),error=>error.qualification.phase==='stress'&&error.qualification.sequential.length===2&&/stress fault/.test(error.message));
  assert.equal(result.passed,true);assert.equal(result.phase,'complete');assert.equal(result.sequential.length,24);assert.equal(result.construction.length,2);assert.equal(result.stress.ticks,20);assert.equal(result.nativeDrops.live,0);
});
