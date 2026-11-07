// Exact packaged service under a kernel memory ceiling, with clients outside
// that ceiling. OOM and partial startup are results, never a heap-limit pass.
import assert from 'node:assert/strict';
import {resolve,dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {mkdir,readFile,mkdtemp,rm} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {Service} from './rpc.mjs';
import {clientFactory} from './clients.mjs';
import {manifest,sleep,until} from './common.mjs';
import {maximum,processMemory,cgroupMemory} from './telemetry.mjs';
import {shapeProbeTraffic} from '../net-probe-shaping.mjs';
const number=(value,fallback)=>value===undefined?fallback:Number(value);
export function checkSample(sample,advertised){
  if(sample.errors?.length)throw Error(sample.errors[0]);
  for(const world of sample.worlds){
    const q=world.network.queues;
    for(const key of ['snapshots','acks','edges'])assert.ok(q[key]<=advertised,`${key} queue exceeds connected-client bound`);
    assert.ok(q.inbox<=4096&&q.inputs<=4096,'world port queue exceeded qualification bound');
    assert.equal(world.queues.joining,0,'admission did not settle');
    for(const session of world.sessions)assert.ok(session.stateBuffered<=32768,'state socket backlog exceeded production bound');
  }
}
export async function qualifyService(config){
  const identity=await manifest(config.engine),server=resolve(dirname(config.engine),'server');
  const raw=resolve(config['raw-dir']);await mkdir(raw,{recursive:true});
  const state=await mkdtemp(join(raw,'service-')),cgroup=join(state,'cgroup.json');
  const helper=fileURLToPath(new URL('./cgroup.py',import.meta.url));
  const command=[process.execPath,join(server,'scripts/qualify-host.mjs'),`--engine=${config.engine}`,`--data=${config.data}`,`--state=${state}`,`--backend=${config.backend}`];
  if(config['world-worker'])command.push(`--world-worker=${resolve(config['world-worker'])}`);
  const service=new Service('python3',[helper,'--out',cgroup,'--bytes',String(1073741824),'--',...command]);
  const result={backend:config.backend,bundle:{engine:identity.engineRevision,server:identity.serverRevision,build:identity.engineBuild},
    configuration:{clients:config.clients,concurrency:config.concurrency,seconds:config.seconds,memoryBytes:1073741824},
    phases:[],highWater:{},queueHighWater:{},clients:[],errors:[],rotations:0,reconnects:0,blackout:null};
  let clients=[],shaping=null,info=[],factory,ready,servicePid;
  const observe=async phase=>{
    const sample=await service.call('sample');checkSample(sample,config.clients);
    const os=await processMemory(servicePid);
    const metadata=JSON.parse(await readFile(cgroup,'utf8'));sample.cgroup=await cgroupMemory(metadata.group);
    // Worker heaps and external arenas are isolate-local. Sum those fields
    // across world workers; RSS/PSS already cover the complete service PID.
    const totals={heapUsed:sample.js.heapUsed,external:sample.js.external,arrayBuffers:sample.js.arrayBuffers};
    for(const world of sample.worlds)for(const key of Object.keys(totals))totals[key]+=world.js[key]+(world.network.js?.[key]??0);
    result.highWater=maximum(result.highWater,{rss:os.rss,pss:os.pss,...totals,cgroupCurrent:sample.cgroup.current,wasmLinearTotalBytes:sample.worlds.reduce((n,w)=>n+(w.wasm.linearBytes??0),0)});
    for(const world of sample.worlds){
      result.highWater=maximum(result.highWater,{wasmLinearBytes:world.wasm.linearBytes});
      result.queueHighWater=maximum(result.queueHighWater,world.network.queues);
    }
    if(phase!=='running'&&result.phases.length<32)result.phases.push({phase,...sample,os});
    if(config.backend==='native')for(const world of sample.worlds){
      assert.equal(world.native.available,true,'native capacity evidence is required');
      for(const field of ['used','reserved','created','released','live'])assert.ok(Number.isFinite(world.native[field]),`missing native ${field}`);
    }
    result.last=sample;return sample;
  };
  const connectAll=async()=>{
    clients=[];
    for(let w=0;w<info.length;w++)for(let i=0;i<config.clients;i++)clients.push(await factory(w*config.clients+i,info[w]));
    await until(()=>{for(const c of clients)if(c.errors.length)throw Error(c.errors[0]);return clients.every(c=>c.complete>0);},'all clients receiving snapshots');
  };
  const closeClients=async()=>{for(const c of clients){result.clients.push(c.report());await c.close();}clients=[];};
  try{
    ready=await Promise.race([service.ready,new Promise((_,reject)=>setTimeout(()=>reject(Error('Service readiness timeout')),180000).unref())]);servicePid=ready.pid;
    assert.equal(ready.bundle.engine,identity.engineRevision);assert.equal(ready.bundle.server,identity.serverRevision);assert.equal(ready.backend,config.backend);
    factory=await clientFactory(config.engine,server,state);
    for(let index=0;index<config.concurrency;index++){
      info.push(await service.call('start',{index,map:config.map??'nab2',side:config.side??'rep',clients:config.clients}));
      await observe(`loaded-${index+1}`);
    }
    await connectAll();await observe('clients-ready');
    if(config.mode==='network'){
      shaping=await shapeProbeTraffic(clients.map(c=>c.pair),{upKbps:number(config['shape-up-kbps'],1000),downKbps:number(config['shape-down-kbps'],2000),
        delayMs:50,jitterMs:15,lossPercent:5,competingUpKbps:0,queuePackets:32});
      if(!shaping)throw Error('Network qualification requires kernel shaping');result.kernelBefore=shaping.stats();
    }
    const started=performance.now(),end=started+config.seconds*1000;let sampled=0,reconnected=false,rotated=0,blackout=false,recovered=false;
    const rotation=(config.rotation??`${config.map??'nab2'}:${config.side??'rep'}`).split(',').map(entry=>{if(!/^[a-z0-9]+:[a-z0-9]+$/.test(entry))throw Error('Invalid rotation');const [map,side]=entry.split(':');return {map,side};});
    const rotations=number(config.rotate,1),blackoutSeconds=number(config['blackout-seconds'],3);
    const initial=clients.map(c=>({id:c.id,receiver:c.receiver}));
    while(performance.now()<end){
      const elapsed=(performance.now()-started)/1000;
      for(const c of clients)if(c.errors.length)throw Error(c.errors[0]);
      if(elapsed>=sampled){await observe('running');sampled+=1;}
      // ACK withholding is deliberately bounded and repeats while load is
      // steady. Transport stays alive; the baseline ring must turn over safely.
      if(config.mode!=='network')for(let i=0;i<clients.length;i++)clients[i].slow(i%8===0&&elapsed%20<5);
      if(!reconnected&&elapsed>config.seconds*0.25){
        reconnected=true;
        if(config.mode==='network'){
          // Rebuild per-port queues for the fresh transport endpoint. Both
          // impaired epochs are exercised; the handshake remains authenticated.
          shaping.stop();execFileSync('tc',['qdisc','del','dev','lo','root']);shaping=null;
        }
        await closeClients();await connectAll();
        assert.deepEqual(clients.map(c=>c.id),initial.map(c=>c.id));assert.ok(clients.every((c,i)=>c.receiver!==initial[i].receiver));
        result.reconnects++;await observe('reconnected');
        if(config.mode==='network'){shaping=await shapeProbeTraffic(clients.map(c=>c.pair),{upKbps:1000,downKbps:2000,delayMs:50,jitterMs:15,lossPercent:5,competingUpKbps:0,queuePackets:32});}
      }
      if(config.mode==='network'&&!blackout&&elapsed>config.seconds*0.55){
        blackout=true;const completed=clients.map(c=>c.complete);
        for(let i=0;i<clients.length;i++)for(const id of [10+i*2,11+i*2])execFileSync('tc',['qdisc','change','dev','lo','parent',`1:${id}`,'handle',`${id}:`,'netem','limit','32','delay','50ms','15ms','loss','100%']);
        await sleep(blackoutSeconds*1000);
        for(let i=0;i<clients.length;i++)for(const id of [10+i*2,11+i*2])execFileSync('tc',['qdisc','change','dev','lo','parent',`1:${id}`,'handle',`${id}:`,'netem','limit','32','delay','50ms','15ms','loss','5%']);
        await until(()=>clients.every((c,i)=>c.complete>completed[i]),'blackout recovery',15000);recovered=true;
        result.blackout={seconds:blackoutSeconds,recovered:true};await observe('blackout-recovered');
      }
      if(config.mode!=='network'&&rotated<rotations&&elapsed>(rotated+1)*config.seconds/(rotations+1)){
        await closeClients();
        for(let index=0;index<info.length;index++){
          const previous=info[index];info[index]=await service.call('rotate',{index,...rotation[(rotated+1)%rotation.length]});
          assert.equal(info[index].id,previous.id);assert.equal(info[index].port,previous.port);
        }
        await connectAll();rotated++;result.rotations=rotated;await observe('rotated');
      }
      await sleep(50);
    }
    if(config.mode==='network'){assert.ok(recovered,'blackout was not exercised');result.kernelAfter=shaping.stats();}
    await observe('final');
    assert.ok(result.last.worlds.every(w=>w.tick>0),'no real world ticks');
    await closeClients();
    for(let index=0;index<info.length;index++){const first=await service.call('close',{index});const second=await service.call('close',{index});assert.equal(first.workerExited,true);assert.equal(second.alreadyClosed,true);}
    const closed=await service.call('sample');assert.equal(closed.worlds.length,0);result.closed=closed;
  }catch(error){result.errors.push(String(error.stack??error).slice(0,4000));}
  finally{
    if(shaping){try{result.kernelFinal=shaping.stats();shaping.stop();execFileSync('tc',['qdisc','del','dev','lo','root']);}catch(error){result.errors.push(String(error));}}
    await closeClients();result.exit=await service.close();result.stderr=service.stderr;
    try{result.cgroup=JSON.parse(await readFile(cgroup,'utf8'));}catch(error){result.errors.push(`Missing cgroup evidence: ${error.message}`);}
    await rm(state,{recursive:true,force:true});
  }
  const cg=result.cgroup;
  result.passed=!result.errors.length&&result.exit.code===0&&cg?.cleaned===true&&cg.remainingPids.length===0
    &&cg.memory.maximum===1073741824&&cg.memory.peak<=1073741824&&cg.memory.events.oom_kill===0;
  result.capacity=Number(cg?.memory?.events?.oom_kill)>0?'exceeded hard budget':result.passed?'qualified measured case':'not qualified';
  return result;
}
