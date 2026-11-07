#!/usr/bin/env node
// This supervisor does no game work. Farm access and private locations come
// from an untracked operator configuration, never repository defaults.
import {readFile,writeFile,mkdir,stat} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {spawnSync} from 'node:child_process';
import {run} from './process.mjs';
import {jobs} from './jobs.mjs';
const args=process.argv.slice(2),option=name=>args.find(a=>a.startsWith(`--${name}=`))?.slice(name.length+3);
const config=JSON.parse(await readFile(option('farm'),'utf8'));
const backend=option('backend')??'wasm',proof=args.includes('--proof');
config.harnessRevision=spawnSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).stdout.trim();
if(!['wasm','native'].includes(backend))throw Error('Invalid backend');
if(!config.slot||!config.node||!Array.isArray(config.hosts)||config.hosts.length!==5)throw Error('Farm config requires slot, node and five host/group entries');
const out=resolve(option('out')??config.out),started=performance.now();await mkdir(out,{recursive:true});
const selection=option('only'),selected=jobs({proof}).filter(j=>!selection||new RegExp(selection).test(j.id));
const execute=async job=>{
  const host=config.hosts[job.role],file=join(out,`${job.id}-config.json`);
  await writeFile(file,JSON.stringify({...config,backend,out,job}));
  const result=await run(config.slot,[host.group,config.node,'tools/host-qualification/job.mjs',file],{
    env:{RSLOT_HOST:host.host,RSLOT_SYNC:config.sync??'',BFE_CONTENT_DIR:config.data},log:join(out,`${job.id}.log`)});
  let verdict;
  try{verdict=JSON.parse(await readFile(join(out,`${job.id}.json`),'utf8'));}
  catch{verdict={job:job.id,backend,passed:false,error:`No farm verdict (exit ${result.code}): ${result.tail}`};}
  // A stale JSON must never hide scheduler or child failure.
  if(result.code!==0){verdict.passed=false;verdict.wrapper={code:result.code,tail:result.tail.slice(-1500)};}
  await writeFile(join(out,`${job.id}.json`),JSON.stringify(verdict)+'\n');
  verdict.host=host.host;console.log(`${verdict.passed?'PASS':'FAIL'} ${job.id} ${Math.round(verdict.wallSeconds??0)}s`);return verdict;
};
let probes;
if(args.includes('--reuse-probes')){
  probes=JSON.parse(await readFile(join(out,'probes.json'),'utf8'));
  if(probes.length!==config.hosts.length||!config.hosts.every(h=>probes.some(p=>p.host===h.host&&p.passed)))throw Error('Setup receipt does not match the configured workers');
}else probes=await Promise.all(config.hosts.map(async host=>{
  const r=await run(config.slot,[host.group,'python3','tools/host-qualification/probe.py'],{env:{RSLOT_HOST:host.host},log:join(out,`probe-${host.host}.log`)});
  const probe={host:host.host,passed:r.code===0};console.log(`${probe.passed?'PASS':'FAIL'} setup ${host.host}`);return probe;
}));
await writeFile(join(out,'probes.json'),JSON.stringify(probes)+'\n');
if(!probes.every(p=>p.passed))throw Error('A configured worker failed cgroup/netns setup; see probe logs');
const results=[],pending=args.includes('--summarize')?[]:selected.filter(j=>!j.exclusive);
let next=0;
// Eight slot wrappers leave room for other work. CPU roles wait until this
// run's other jobs finish, preserving their interleaved acceptance window.
await Promise.all(Array.from({length:Math.min(8,pending.length)},async()=>{for(;;){const i=next++;if(i>=pending.length)return;results.push(await execute(pending[i]));}}));
if(!args.includes('--summarize'))for(const job of selected.filter(j=>j.exclusive))results.push(await execute(job));
// Focused retries replace only their verdicts. Read every final file so the
// full summary also reflects successful repairs made during long soaks.
results.length=0;
for(const job of selected){
  const row=JSON.parse(await readFile(join(out,`${job.id}.json`),'utf8'));
  if(row.job!==job.id||row.backend!==backend)throw Error(`Mismatched verdict ${job.id}`);results.push(row);
}
const commit=config.harnessRevision;
const summary={backend,proof,commit,wallSeconds:(Date.now()-(await stat(out)).birthtimeMs)/1000,runnerSeconds:(performance.now()-started)/1000,passed:results.every(r=>r.passed),
  jobs:results.map(({job,passed,wallSeconds,error,host})=>({job,passed,wallSeconds,error,host}))};
await writeFile(join(out,'summary.json'),JSON.stringify(summary,null,2)+'\n');
console.log(`\nHost qualification ${backend}, ${proof?'five-minute proof':'full duration'}, ${commit.slice(0,12)}`);
for(const row of summary.jobs.sort((a,b)=>a.job.localeCompare(b.job,undefined,{numeric:true})))console.log(`${row.job.padEnd(10)} ${row.passed?'PASS':'FAIL'} ${Math.round(row.wallSeconds??0)}s`);
console.log(`${summary.passed?'PASS':'FAIL'}: ${results.filter(r=>r.passed).length}/${results.length} jobs; ${Math.round(summary.wallSeconds)}s wall`);
process.exitCode=summary.passed?0:1;
