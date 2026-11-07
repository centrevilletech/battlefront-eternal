#!/usr/bin/env node
// Farm entry point: all raw logs/transcripts stay beneath raw, while each job
// exports exactly one bounded verdict. Failures preserve the first evidence.
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir,readdir,rm,symlink} from 'node:fs/promises';
import {resolve,join,dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {execFileSync} from 'node:child_process';
import {manifest,backendProvider} from './common.mjs';
import {run,requireRun} from './process.mjs';
import {shardRows,maps} from './jobs.mjs';
import {engineBuild} from '../host-contract.mjs';
const config=JSON.parse(await readFile(process.argv[2],'utf8'));
const job=config.job,started=performance.now(),raw=resolve(config.raw,job.id),out=resolve(config.out,`${job.id}.json`);
await mkdir(raw,{recursive:true});
const node=process.execPath;
const env={BFE_CONTENT_DIR:config.data,BFE_QUALIFICATION_ENGINE:config.engine,BFE_QUALIFICATION_BACKEND:config.backend,
  BFE_QUALIFICATION_MODULE:config.module??'',NODE_OPTIONS:`--import=${fileURLToPath(new URL('./factory.mjs',import.meta.url))}`};
const invoke=async(script,args=[],extra={})=>requireRun(await run(node,[script,...args],{log:join(raw,`${extra.label??'run'}.log`),env:{...env,...extra.env},...extra}));
const command=async(kind,extra,id)=>{
  const file=join(raw,`${id}.json`);
  const args=[kind,`--backend=${config.backend}`,`--engine=${config.engine}`,`--data=${config.data}`,`--out=${file}`,...extra];
  if(config.module)args.push(`--backend-module=${config.module}`);
  if(config.worldWorker)args.push(`--world-worker=${config.worldWorker}`);
  const exit=await run(node,['--expose-gc','tools/host-qualification.mjs',...args],{log:join(raw,`${id}.log`),env:{BFE_CONTENT_DIR:config.data}});
  const result=JSON.parse(await readFile(file,'utf8'));
  if(!result.passed)throw Object.assign(Error(`${id}: ${result.error??result.errors?.join('; ')??'qualification failed'}`),{evidence:result});
  requireRun(exit);return result;
};
let detail={},passed=false,error;
try{
  // General farm mirrors omit nested dependency directories. A paired archive
// retains their exact bytes; serialize extraction when jobs share a worker.
if(config.bundleArchive)execFileSync('flock',[join(config.raw,'bundle.lock'),process.execPath,fileURLToPath(new URL('./hydrate.mjs',import.meta.url)),config.bundleArchive,config.engine,join(config.raw,'bundle.sha256')]);
  const identity=await manifest(config.engine);detail.bundle={engine:identity.engineRevision,server:identity.serverRevision,build:identity.engineBuild,schema:identity.schema,supplement:identity.qualificationSupplement};
  if(config.backend==='native'){
    assert.equal(identity.format,2,'Native qualification requires the paired target-aware bundle');
    assert.ok(identity.targets?.some(t=>t.target==='linux-x64-gnu'),'Native target artifact is not declared');
    await backendProvider({backend:config.backend,module:config.module,engine:config.engine,manifest:identity});
  }
  if(job.kind==='matrix'){
    const matrix=join(raw,'matrix.txt');await writeFile(matrix,shardRows(await readFile('tools/world-hash-matrix.txt','utf8'),job.shard));
    await invoke('tools/world-hash-matrix.mjs',['--matrix',matrix,'--out',join(raw,'matrix'),'--against',config.references,'--require-comparison','--jobs','4']);
    const result=JSON.parse(await readFile(join(raw,'matrix/summary.json'),'utf8'));
    assert.equal(result.failures,0);detail={...detail,cases:result.rows.length,ticks:result.ticksPerVariant,hashes:['state','digest','published'],
      rows:result.rows.map(r=>({label:r.label,ticks:r.ticks,matched:r.comparisons.every(c=>c.verdict==='same')}))};
    if(config.backend==='native'){
      const provider=await backendProvider({backend:config.backend,module:config.module,engine:config.engine,manifest:identity});
      assert.equal(typeof provider.replayMatrix,'function','Native direct replayMatrix hook required in addition to bridge matrix');
      detail.direct=await provider.replayMatrix({matrix,references:config.references,corpus:config.worldCorpus,raw});
      assert.equal(detail.direct.passed,true);
    }
  }else if(job.kind==='packets'){
    assert.ok(config.packetReferences,'Frozen packet reference directory is required');
    const files=(await readdir(config.packetReferences)).filter(name=>name.endsWith('.jsonl')).sort();
    assert.ok(files.length>=5,'At least five frozen F6 cases required');const cases=[];
    for(const file of files){
      const summary=JSON.parse(await readFile(join(config.packetReferences,file.replace(/jsonl$/,'json')),'utf8'));
      const label=summary.label??file.slice(0,-6),parts=label.split('-'),args=['--out',raw,'--against',config.packetReferences,'--map',parts[0],'--ticks',String(summary.ticks),'--budget',parts[2]];
      if(parts[1]==='local')args.push('--local');
      for(const flag of ['stress','spectator','remote','edges','flyer'])if(parts.includes(flag))args.push(`--${flag}`);
      if(label.includes('late-host'))args.push('--late-host');
      if(parts.includes('ordnance'))args.push(parts.includes('valid')?'--ordnance-valid':'--ordnance');
      await invoke('tools/wire-publication-corpus.mjs',args,{label});
      const observed=JSON.parse(await readFile(join(raw,`${label}.json`),'utf8'));assert.equal(observed.matched,true);
      cases.push({label,publications:observed.publications,sha256:observed.sha256,coverage:observed.coverage});
      await rm(join(raw,`${label}.jsonl`));
    }
    // Boundary faults, same-tick replacement, baseline eviction and held ACK
    // cases complement the content corpus and remain content-free CI tests.
    const boundaryFiles=['wire-host','wire-sim','wire-edges','snapshot-host-publication','snapshot-host-client','snapshot-codec','snapshot-reuse'].map(s=>`tests/${s}.test.js`);
    await invoke('--test',boundaryFiles,{label:'boundary-tests'});
    detail={...detail,cases,publications:cases.reduce((n,c)=>n+c.publications,0),boundaryTests:boundaryFiles};
  }else if(job.kind==='lifecycle'){
    const r=await command('lifecycle',[`--map=${job.map}`,`--side=${job.side}`,`--seconds=${job.seconds}`,'--battles=24','--ticks=120','--stress-ticks=12000','--clients=4'],'lifecycle');
    detail={...detail,map:job.map,sequential:r.sequential.length,construction:r.construction,long:r.long,stress:r.stress,isolation:r.isolation,workers:r.workers,peak:r.peak,nativeDrops:r.nativeDrops};
  }else if(job.kind==='memory'){
    detail.cases=[];let failed=false;
    for(const [clients,concurrency] of [[32,1],[64,1],[64,2],[64,4]]){
      const id=`memory-${clients}-${concurrency}`;
      try{const r=await command('service',[`--clients=${clients}`,`--concurrency=${concurrency}`,`--seconds=${job.seconds}`,'--rotate=1','--rotation=nab2:rep,hot1:imp',`--raw-dir=${raw}`],id);
      detail.cases.push({clients,concurrency,capacity:r.capacity,highWater:r.highWater,queueHighWater:r.queueHighWater,cgroup:r.cgroup,reconnects:r.reconnects,rotations:r.rotations});
      }catch(error){failed=true;const r=error.evidence??{};detail.cases.push({clients,concurrency,passed:false,capacity:r.capacity,cgroup:r.cgroup,highWater:r.highWater,queueHighWater:r.queueHighWater,error:error.message});}
    }
    assert.equal(failed,false,'One or more measured capacities did not qualify; concurrent admission remains forbidden');
  }else if(job.kind==='network'){
    const r=await command('network',[`--map=${job.map}`,`--side=${job.side}`,'--clients=4','--seconds=60','--blackout-seconds=3','--shape-up-kbps=1000','--shape-down-kbps=2000',`--raw-dir=${raw}`],'network');
    detail={...detail,map:job.map,clients:r.clients.map(c=>({tick:c.tick,complete:c.complete,errors:c.errors})),reconnects:r.reconnects,blackout:r.blackout,queueHighWater:r.queueHighWater,
      kernel:r.kernelFinal,cgroup:r.cgroup};
    if(config.nativeClient&&config.dist){
      const peerExit=await run(node,['tools/host-qualification/peers.mjs',process.argv[2]],{log:join(raw,'rendered-peers.log'),env:{NODE_OPTIONS:''}});
      detail.peers=JSON.parse(await readFile(join(raw,'peers.json'),'utf8'));requireRun(peerExit);assert.equal(detail.peers.passed,true);
    }else throw Error('Network qualification requires nativeClient and dist for actual native and browser peers');
  }else if(job.kind==='cpu'){
    detail.roles=[];
    const affinity=execFileSync('taskset',['-pc',String(process.pid)],{encoding:'utf8'}).trim().split(':').at(-1).trim();
    const cpu=affinity.split(/[,-]/)[0];detail.cpu=cpu;
    const roles=config.backend==='native'?['native','wasm','wasm','native']:['wasm','wasm'];
    for(const [map,side] of maps)for(const [index,backend] of roles.entries()){
      const file=join(raw,`${map}-${index}.json`),data={backend,module:config.module,engine:config.engine,data:config.data,map,side,out:file};
      requireRun(await run('taskset',['-c',cpu,node,'tools/host-qualification/cpu-worker.mjs'],{log:join(raw,`${map}-${index}.log`),env:{BFE_QUALIFICATION_CASE:JSON.stringify(data)}}));
      const row=JSON.parse(await readFile(file,'utf8'));detail.roles.push({...row,diagnostics:undefined});
    }
    detail.order=roles;detail.nativeComparison=config.backend==='native';
  }else if(job.kind==='desktop'){
    await invoke('tools/host-qualification/desktop.mjs',[process.argv[2]],{env:{NODE_OPTIONS:'',BFE_HOST_ADMIN_PORT:'20460',BFE_HOST_GAME_PORT:'20461'}});
    detail.host=JSON.parse(await readFile(join(raw,'desktop.json'),'utf8'));
  }else if(job.kind==='mismatch'){
    const {qualifyMismatch}=await import('./mismatch.mjs');detail.cases=await qualifyMismatch(config,raw);
  }else if(job.kind==='server-suite'){
    assert.ok(config.serverArchive,'Pinned server source archive is required for its complete suite');
    const archive=await readFile(config.serverArchive),revision=execFileSync('git',['get-tar-commit-id'],{input:archive.subarray(0,1024),encoding:'utf8'}).trim();
    assert.equal(revision,identity.serverRevision,'Server test source is not the packaged pin');
    const server=join(raw,'server-source');await mkdir(server,{recursive:true});execFileSync('tar',['-xf',config.serverArchive,'-C',server]);
    try{await symlink(resolve(config.engine,'../server/node_modules'),join(server,'node_modules'),'dir');}catch(error){if(error.code!=='EEXIST')throw error;}
    const files=(await readdir(join(server,'test'))).filter(f=>f.endsWith('.test.js')).map(f=>join(server,'test',f));
    const source=fileURLToPath(new URL('../../',import.meta.url));
    assert.equal(engineBuild(source),identity.engineBuild,'Server suite source differs from the paired bundle');
    const r=await invoke('--test',files,{cwd:server,env:{NODE_OPTIONS:'',BFE_ENGINE_DIR:source,BFE_DATA_DIR:config.data},label:'server-suite'});
    detail.suite=r.tail.split('\n').filter(line=>/^# (tests|pass|fail|skipped)|ℹ (tests|pass|fail|skipped)/.test(line));
    assert.ok(detail.suite.some(line=>/(skipped 0|skipped: 0)/.test(line)),'Server suite must not skip integration tests');
  }else throw Error(`Unknown job ${job.kind}`);
  passed=true;
}catch(e){error=String(e.stack??e).slice(0,6000);if(e.cases)detail.cases=e.cases;if(e.evidence){const r=e.evidence;detail.failure={phase:r.phase,capacity:r.capacity,cgroup:r.cgroup,highWater:r.highWater,queueHighWater:r.queueHighWater,errors:r.errors};}}
await mkdir(dirname(out),{recursive:true});
const result={job:job.id,harnessRevision:config.harnessRevision,kind:job.kind,backend:config.backend,passed,wallSeconds:(performance.now()-started)/1000,node:process.version,...detail,error};
await writeFile(out,JSON.stringify(result)+'\n');console.log(JSON.stringify(result));
process.exitCode=passed?0:1;
